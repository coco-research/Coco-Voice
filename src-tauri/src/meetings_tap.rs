//! Others track: Core Audio process tap, read through a private aggregate device.
//! Samples go to a caller sink at 16 kHz. A later PR writes system.wav.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use cidre::cat::audio::Buf;
use cidre::cat::AudioBufList;
use cidre::cf::{self, ArrayOf, DictionaryOf};
use cidre::core_audio::hardware::{self, aggregate_device_keys, sub_tap_keys, AggregateDevice};
use cidre::core_audio::hardware_tapping::TapGuard;
use cidre::core_audio::{hardware_err, Obj, TapDesc, TapMuteBehavior};
use cidre::ns;
use cidre::os::{self, Status};

use crate::audio_toolkit::audio::FrameResampler;

const OUT_HZ: usize = 16_000;
const FRAME: Duration = Duration::from_millis(30);
/// Mono blocks in flight. Full queue drops a block instead of stalling the IO thread.
const QUEUE_BLOCKS: usize = 64;
const USAGE_KEY: &str = "NSAudioCaptureUsageDescription";

#[derive(Debug, PartialEq, Eq)]
pub enum TapError {
    UsageStringMissing,
    PermissionDenied,
    /// Empty or all-zero capture. Never a success, and not a permission denial.
    Silent,
    Device(String),
}

impl TapError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::UsageStringMissing => "system_audio_usage_string_missing",
            Self::PermissionDenied => "system_audio_permission_denied",
            Self::Silent => "system_audio_silent",
            Self::Device(_) => "system_audio_device",
        }
    }
}

/// Plist text must contain the usage key. This does not read the bundle and does not start IO.
pub fn require_usage_string(plist_text: &str) -> Result<(), TapError> {
    if plist_text.contains(USAGE_KEY) {
        Ok(())
    } else {
        Err(TapError::UsageStringMissing)
    }
}

/// What the read path needs from a device. Tests pass a fake. Core Audio is the other impl's caller.
pub trait SystemAudioDevice {
    fn sample_rate_hz(&self) -> usize;
    fn start(&mut self) -> Result<(), TapError>;
    fn pull(&mut self) -> Result<Vec<f32>, TapError>;
    fn stop(&mut self) -> Result<(), TapError>;
}

/// Shared resample + silence check. The fake pump and the tap worker thread both use this.
pub struct ReadPath {
    resampler: FrameResampler,
    saw_any: bool,
    saw_nonzero: bool,
}

impl ReadPath {
    pub fn new(in_hz: usize) -> Self {
        Self {
            resampler: FrameResampler::new(in_hz, OUT_HZ, FRAME),
            saw_any: false,
            saw_nonzero: false,
        }
    }

    pub fn push(&mut self, src: &[f32], sink: &mut dyn FnMut(&[f32])) {
        if !src.is_empty() {
            self.saw_any = true;
            if src.iter().any(|sample| *sample != 0.0) {
                self.saw_nonzero = true;
            }
        }
        self.resampler.push(src, |frame| sink(frame));
    }

    pub fn finish(&mut self, sink: &mut dyn FnMut(&[f32])) -> Result<(), TapError> {
        self.resampler.finish(|frame| sink(frame));
        if self.saw_nonzero {
            Ok(())
        } else {
            Err(TapError::Silent)
        }
    }
}

/// Check the usage string, then run one finite device through [`ReadPath`].
/// A missing usage string returns before `start`.
pub fn capture(
    plist_text: &str,
    device: &mut impl SystemAudioDevice,
    sink: &mut dyn FnMut(&[f32]),
) -> Result<(), TapError> {
    require_usage_string(plist_text)?;
    device.start()?;
    let pulled = device.pull();
    let stopped = device.stop();
    let samples = pulled?;
    stopped?;
    let mut path = ReadPath::new(device.sample_rate_hz());
    path.push(&samples, sink);
    path.finish(sink)
}

fn map_os(err: os::Error) -> TapError {
    if err == hardware_err::PERMISSIONS {
        TapError::PermissionDenied
    } else {
        TapError::Device(format!("{err:?}"))
    }
}

/// The caller's sink. Owned by the worker thread, never called from the IO thread.
type TapSink = Box<dyn FnMut(&[f32]) + Send>;

/// Everything the IO thread touches: a bounded queue and two counters. No locks, no sink.
struct IoState {
    channels: u32,
    tx: SyncSender<Vec<f32>>,
    dropped: Arc<AtomicU64>,
}

/// Drains mono blocks, resamples to 16 kHz, calls the sink. Runs on the worker thread only.
fn run_worker<S: FnMut(&[f32]) + ?Sized>(
    in_hz: usize,
    rx: &Receiver<Vec<f32>>,
    sink: &mut S,
    heard: &AtomicBool,
) -> Result<(), TapError> {
    let mut path = ReadPath::new(in_hz);
    let mut mark = |frame: &[f32]| {
        if !heard.load(Ordering::Relaxed) && frame.iter().any(|sample| *sample != 0.0) {
            heard.store(true, Ordering::Relaxed);
        }
        sink(frame);
    };
    while let Ok(block) = rx.recv() {
        path.push(&block, &mut mark);
    }
    path.finish(&mut mark)
}

fn spawn_worker(
    in_hz: usize,
    sink: TapSink,
    heard: Arc<AtomicBool>,
) -> Result<(SyncSender<Vec<f32>>, JoinHandle<Result<(), TapError>>), TapError> {
    let (tx, rx) = mpsc::sync_channel::<Vec<f32>>(QUEUE_BLOCKS);
    let worker = std::thread::Builder::new()
        .name("system-audio-tap".into())
        .spawn(move || {
            let mut sink = sink;
            run_worker(in_hz, &rx, &mut *sink, &heard)
        })
        .map_err(|err| TapError::Device(format!("tap worker thread: {err}")))?;
    Ok((tx, worker))
}

// ponytail: one AudioBuffer. A tap aggregate that exposes more streams needs a BufListN loop.
extern "C" fn on_input(
    _device: hardware::Device,
    _now: &cidre::cat::AudioTimeStamp,
    input_data: &AudioBufList<1>,
    _input_time: &cidre::cat::AudioTimeStamp,
    _output_data: &mut AudioBufList<1>,
    _output_time: &cidre::cat::AudioTimeStamp,
    client_data: Option<&mut IoState>,
) -> Status {
    if let Some(state) = client_data {
        if input_data.number_buffers > 0 {
            let frames = mono_frames(&input_data.buffers[0], state.channels);
            // Real-time thread: never block, never lock, never touch the sink. A full queue
            // drops this block and the counter records it.
            if state.tx.try_send(frames).is_err() {
                state.dropped.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
    Status::NO_ERR
}

fn mono_frames(buf: &Buf, channels: u32) -> Vec<f32> {
    if buf.data.is_null() || buf.data_bytes_size < 4 {
        return Vec::new();
    }
    let count = buf.data_bytes_size as usize / std::mem::size_of::<f32>();
    // SAFETY: HAL owns this buffer for the duration of the IO proc. Length is the byte size.
    let raw = unsafe { std::slice::from_raw_parts(buf.data as *const f32, count) };
    if channels <= 1 {
        return raw.to_vec();
    }
    let channels = channels as usize;
    raw.chunks(channels)
        .map(|frame| frame.iter().sum::<f32>() / channels as f32)
        .collect()
}

// cidre 0.29 has no AggregateDevice::destroy. AudioHardwareDestroyAggregateDevice is the C call.
#[link(name = "CoreAudio", kind = "framework")]
extern "C" {
    fn AudioHardwareDestroyAggregateDevice(in_device: Obj) -> Status;
}

fn destroy_aggregate(id: u32) {
    // SAFETY: id is an aggregate this process created. Apple frees it asynchronously.
    let _ = unsafe { AudioHardwareDestroyAggregateDevice(Obj(id)) };
}

fn release_partial(tap: TapGuard, aggregate_id: Option<u32>, state: *mut IoState) {
    if let Some(id) = aggregate_id {
        destroy_aggregate(id);
    }
    if !state.is_null() {
        // SAFETY: device_start has not returned Ok, so the IO proc is not running.
        drop(unsafe { Box::from_raw(state) });
    }
    drop(tap);
}

fn private_aggregate(tap_uid: &cf::String) -> Result<(AggregateDevice, u32), TapError> {
    let drift = cf::Number::from_i32(1);
    let private_flag = cf::Number::from_i32(1);
    let name = cf::String::from_str("Coco Voice Meeting Tap");
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let agg_uid = cf::String::from_str(&format!(
        "com.cocoresearch.cocovoice.tap.{}.{}",
        std::process::id(),
        nonce
    ));
    // Do not set tapautostart: a non-zero value makes AudioDeviceStart wait for a tapped process.
    let sub = DictionaryOf::<cf::String, cf::Type>::with_keys_values(
        &[sub_tap_keys::uid(), sub_tap_keys::drift_compensation()],
        &[tap_uid, &drift],
    );
    let taps = ArrayOf::from_slice(&[&*sub]);
    let desc = DictionaryOf::<cf::String, cf::Type>::with_keys_values(
        &[
            aggregate_device_keys::name(),
            aggregate_device_keys::uid(),
            aggregate_device_keys::is_private(),
            aggregate_device_keys::tap_list(),
        ],
        &[&name, &agg_uid, &private_flag, &taps],
    );
    let aggregate = AggregateDevice::with_desc(&desc).map_err(map_os)?;
    let id = aggregate.as_ref().0 .0;
    Ok((aggregate, id))
}

/// Live tap. `start` checks the usage string before any Core Audio call.
/// `stop` stops IO, frees the IO state (which closes the queue), joins the worker,
/// then destroys the aggregate and drops the tap.
pub struct SystemAudioSession {
    tap: Option<TapGuard>,
    started: Option<hardware::StartedDevice<AggregateDevice>>,
    aggregate_id: Option<u32>,
    state: *mut IoState,
    worker: Option<JoinHandle<Result<(), TapError>>>,
    dropped: Arc<AtomicU64>,
    heard: Arc<AtomicBool>,
}

impl SystemAudioSession {
    pub fn start(plist_text: &str, sink: TapSink) -> Result<Self, TapError> {
        require_usage_string(plist_text)?;

        let pid = ns::Number::with_u32(std::process::id());
        let excluded = ns::Array::from_slice(&[pid.as_ref()]);
        let mut desc = TapDesc::with_mono_global_tap_excluding_processes(&excluded);
        desc.set_mute_behavior(TapMuteBehavior::Unmuted);
        desc.set_private(true);
        let tap = desc.create_process_tap().map_err(map_os)?;

        let uid = match tap.uid() {
            Ok(uid) => uid,
            Err(err) => {
                release_partial(tap, None, std::ptr::null_mut());
                return Err(map_os(err));
            }
        };
        let asbd = match tap.asbd() {
            Ok(asbd) => asbd,
            Err(err) => {
                release_partial(tap, None, std::ptr::null_mut());
                return Err(map_os(err));
            }
        };
        let rate = asbd.sample_rate.round() as usize;
        if rate == 0 {
            release_partial(tap, None, std::ptr::null_mut());
            return Err(TapError::Device("tap sample rate is 0".into()));
        }
        let channels = asbd.channels_per_frame.max(1);

        let (aggregate, aggregate_id) = match private_aggregate(&uid) {
            Ok(pair) => pair,
            Err(err) => {
                release_partial(tap, None, std::ptr::null_mut());
                return Err(err);
            }
        };

        let dropped = Arc::new(AtomicU64::new(0));
        let heard = Arc::new(AtomicBool::new(false));
        let (tx, worker) = match spawn_worker(rate, sink, Arc::clone(&heard)) {
            Ok(pair) => pair,
            Err(err) => {
                release_partial(tap, Some(aggregate_id), std::ptr::null_mut());
                return Err(err);
            }
        };

        let state = Box::into_raw(Box::new(IoState {
            channels,
            tx,
            dropped: Arc::clone(&dropped),
        }));
        // SAFETY: `state` stays allocated until stop/drop, and only after AudioDeviceStop returns.
        let proc_id =
            aggregate.create_io_proc_id::<1, 1, IoState>(on_input, Some(unsafe { &mut *state }));
        let proc_id = match proc_id {
            Ok(proc_id) => proc_id,
            Err(err) => {
                release_partial(tap, Some(aggregate_id), state);
                let _ = worker.join();
                return Err(map_os(err));
            }
        };

        match hardware::device_start(aggregate, Some(proc_id)) {
            Ok(started) => Ok(Self {
                tap: Some(tap),
                started: Some(started),
                aggregate_id: Some(aggregate_id),
                state,
                worker: Some(worker),
                dropped,
                heard,
            }),
            Err(err) => {
                release_partial(tap, Some(aggregate_id), state);
                let _ = worker.join();
                Err(map_os(err))
            }
        }
    }

    /// Blocks dropped because the worker fell behind. The IO thread never blocks.
    pub fn dropped_blocks(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    /// True once the worker saw a non-zero 16 kHz frame.
    /// macOS often delivers all zeros rather than an error when System Audio Recording is
    /// off for this process, so callers can warn early instead of waiting for [`TapError::Silent`].
    pub fn has_heard_audio(&self) -> bool {
        self.heard.load(Ordering::Relaxed)
    }

    pub fn stop(mut self) -> Result<(), TapError> {
        let stop_err = match self.started.take() {
            Some(started) => match started.stop() {
                Ok(_aggregate) => None,
                Err(err) => Some(map_os(err)),
            },
            None => None,
        };
        let finish_err = if stop_err.is_some() {
            self.abandon_state();
            None
        } else {
            self.finish_worker()
        };
        if let Some(id) = self.aggregate_id.take() {
            destroy_aggregate(id);
        }
        self.tap.take();
        finish_err.or(stop_err).map_or(Ok(()), Err)
    }

    /// Free the IO state (dropping the only sender, which closes the queue), then join the
    /// worker and return its `finish` outcome. Only valid once IO has stopped.
    fn finish_worker(&mut self) -> Option<TapError> {
        if !self.state.is_null() {
            // SAFETY: started.take()/stop() has already stopped IO, so the proc is not in on_input.
            drop(unsafe { Box::from_raw(self.state) });
            self.state = std::ptr::null_mut();
        }
        self.join_worker()
    }

    /// After a failed stop, IO may still call `on_input` with `state`, so the state is leaked on
    /// purpose (never freed) and the worker detached; its queue stays open, so it just waits.
    fn abandon_state(&mut self) {
        self.state = std::ptr::null_mut();
        self.worker = None;
    }

    fn join_worker(&mut self) -> Option<TapError> {
        let worker = self.worker.take()?;
        match worker.join() {
            Ok(result) => result.err(),
            Err(_) => Some(TapError::Device("tap worker panicked".into())),
        }
    }
}

impl Drop for SystemAudioSession {
    fn drop(&mut self) {
        if let Some(started) = self.started.take() {
            if started.stop().is_err() {
                self.abandon_state();
            }
        }
        if !self.state.is_null() {
            // SAFETY: IO stopped above (or never started), so the proc is not in on_input.
            drop(unsafe { Box::from_raw(self.state) });
            self.state = std::ptr::null_mut();
        }
        let _ = self.join_worker();
        if let Some(id) = self.aggregate_id.take() {
            destroy_aggregate(id);
        }
        self.tap.take();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLIST_WITH_USAGE: &str = "\
        <key>NSAudioCaptureUsageDescription</key>\
        <string>Coco Voice records system audio from your meetings and transcribes it on this Mac.</string>";

    struct FakeDevice {
        rate: usize,
        samples: Vec<f32>,
        starts: u32,
    }

    impl SystemAudioDevice for FakeDevice {
        fn sample_rate_hz(&self) -> usize {
            self.rate
        }

        fn start(&mut self) -> Result<(), TapError> {
            self.starts += 1;
            Ok(())
        }

        fn pull(&mut self) -> Result<Vec<f32>, TapError> {
            Ok(std::mem::take(&mut self.samples))
        }

        fn stop(&mut self) -> Result<(), TapError> {
            Ok(())
        }
    }

    fn sine(rate: usize, hz: f32, seconds: f32) -> Vec<f32> {
        let n = (rate as f32 * seconds) as usize;
        (0..n)
            .map(|i| (2.0 * std::f32::consts::PI * hz * i as f32 / rate as f32).sin())
            .collect()
    }

    /// Magnitude of correlation against `hz` at `rate`. A matching sine is about 0.707.
    fn tone_score(samples: &[f32], hz: f32, rate: f32) -> f32 {
        let start = 480.min(samples.len());
        let mut end = samples.len();
        while end > start && samples[end - 1].abs() < 1e-3 {
            end -= 1;
        }
        let slice = &samples[start..end];
        if slice.len() < 320 {
            return 0.0;
        }
        let mut re = 0.0f32;
        let mut im = 0.0f32;
        let mut energy = 0.0f32;
        let step = 2.0 * std::f32::consts::PI * hz / rate;
        for (i, sample) in slice.iter().enumerate() {
            let phase = step * i as f32;
            re += sample * phase.cos();
            im += sample * phase.sin();
            energy += sample * sample;
        }
        if energy < 1e-6 {
            return 0.0;
        }
        (re * re + im * im).sqrt() / (energy * slice.len() as f32).sqrt()
    }

    #[test]
    fn fake_tap_sine_reaches_the_sink() {
        let hz = 440.0f32;
        let mut device = FakeDevice {
            rate: 48_000,
            samples: sine(48_000, hz, 0.25),
            starts: 0,
        };
        let mut got = Vec::new();
        capture(PLIST_WITH_USAGE, &mut device, &mut |frame| {
            got.extend_from_slice(frame)
        })
        .expect("tone capture");
        assert_eq!(device.starts, 1);
        assert!(got.len() > 480, "expected 16 kHz frames, got {}", got.len());
        assert!(got.iter().any(|sample| *sample != 0.0), "sink was silent");
        let score = tone_score(&got, hz, OUT_HZ as f32);
        assert!(score > 0.35, "440 Hz not present at 16 kHz (score {score})");
    }

    #[test]
    fn missing_usage_string_is_typed_error() {
        let mut device = FakeDevice {
            rate: 48_000,
            samples: sine(48_000, 440.0, 0.1),
            starts: 0,
        };
        let err = capture(
            "<plist><key>NSMicrophoneUsageDescription</key></plist>",
            &mut device,
            &mut |_| {},
        )
        .expect_err("missing usage string");
        assert_eq!(err.code(), "system_audio_usage_string_missing");
        assert_eq!(device.starts, 0, "IO started without the usage string");
    }

    #[test]
    fn silent_stream_is_not_success() {
        let mut device = FakeDevice {
            rate: 48_000,
            samples: vec![0.0; 48_000 / 10],
            starts: 0,
        };
        let err = capture(PLIST_WITH_USAGE, &mut device, &mut |_| {}).expect_err("all-zero stream");
        assert_eq!(err.code(), "system_audio_silent");
        assert_ne!(err.code(), "system_audio_permission_denied");
        assert_eq!(device.starts, 1);
    }

    /// No Core Audio: the IO thread is faked by pushing blocks into the worker's queue.
    #[test]
    fn worker_resamples_blocks_and_reports_silence() {
        let (tx, rx) = mpsc::sync_channel::<Vec<f32>>(QUEUE_BLOCKS);
        let heard = AtomicBool::new(false);
        let mut got = Vec::new();
        let tone = sine(48_000, 440.0, 0.25);
        for block in tone.chunks(1024) {
            tx.send(block.to_vec()).expect("queue tone block");
        }
        drop(tx);
        run_worker(
            48_000,
            &rx,
            &mut |frame: &[f32]| got.extend_from_slice(frame),
            &heard,
        )
        .expect("tone worker");
        assert!(got.len() > 480, "expected 16 kHz frames, got {}", got.len());
        assert!(heard.load(Ordering::Relaxed), "worker missed the tone");
        assert!(
            tone_score(&got, 440.0, OUT_HZ as f32) > 0.35,
            "440 Hz not present at 16 kHz"
        );

        let (tx, rx) = mpsc::sync_channel::<Vec<f32>>(QUEUE_BLOCKS);
        let quiet = AtomicBool::new(false);
        for _ in 0..8 {
            tx.send(vec![0.0; 1024]).expect("queue silent block");
        }
        drop(tx);
        let err =
            run_worker(48_000, &rx, &mut |_: &[f32]| {}, &quiet).expect_err("all-zero blocks");
        assert_eq!(err, TapError::Silent);
        assert!(!quiet.load(Ordering::Relaxed), "zeros counted as audio");
    }
}
