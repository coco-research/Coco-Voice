//! Others track: Core Audio process tap, read through a private aggregate device.
//! Samples go to a caller sink at 16 kHz. A later PR writes system.wav.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use cidre::cat::audio::{Format, FormatFlags};
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
/// Pool buffers fit the device's largest IO slice (oversize drop: a guard). Floor, in mono frames.
const BLOCK_FRAMES: usize = 8192;
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

/// Plist text must carry the usage key with a non-empty string value.
/// This does not read the bundle and does not start IO.
pub fn require_usage_string(plist_text: &str) -> Result<(), TapError> {
    let key = format!("<key>{USAGE_KEY}</key>");
    let rest = plist_text
        .split_once(&key)
        .map(|(_, rest)| rest.trim_start())
        .ok_or(TapError::UsageStringMissing)?;
    let rest = rest
        .strip_prefix("<string>")
        .ok_or(TapError::UsageStringMissing)?;
    let (value, _) = rest
        .split_once("</string>")
        .ok_or(TapError::UsageStringMissing)?;
    if value.trim().is_empty() {
        return Err(TapError::UsageStringMissing);
    }
    Ok(())
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
    saw_nonzero: bool,
}

impl ReadPath {
    pub fn new(in_hz: usize) -> Self {
        Self {
            resampler: FrameResampler::new(in_hz, OUT_HZ, FRAME),
            saw_nonzero: false,
        }
    }

    pub fn push(&mut self, src: &[f32], sink: &mut dyn FnMut(&[f32])) {
        if src.iter().any(|sample| *sample != 0.0) {
            self.saw_nonzero = true;
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
    let samples = match (pulled, stopped) {
        (Ok(samples), Ok(())) => samples,
        (Err(err), Ok(())) | (Ok(_), Err(err)) => return Err(err),
        (Err(pull_err), Err(stop_err)) => {
            log::warn!("system audio tap: stop failed after pull error: {stop_err:?}");
            return Err(pull_err);
        }
    };
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

/// All the IO thread touches: the pool, a bounded queue, two counters and the stop flag.
/// No locks, no sink. `shutdown` is the worker's flag, shared so IO can see it after a stop.
struct IoState {
    channels: u32,
    free: Receiver<Vec<f32>>,
    tx: SyncSender<Vec<f32>>,
    dropped: Arc<AtomicU64>,
    worker_gone: Arc<AtomicBool>,
    shutdown: Arc<AtomicBool>,
}

/// Count one non-blocking send. `Full` means the worker fell behind, so the block is dropped.
/// `Disconnected` means the worker exited or panicked: capture is dead and nothing was dropped.
fn record_send(
    result: Result<(), TrySendError<Vec<f32>>>,
    dropped: &AtomicU64,
    worker_gone: &AtomicBool,
) {
    match result {
        Ok(()) => {}
        Err(TrySendError::Full(_)) => {
            dropped.fetch_add(1, Ordering::Relaxed);
        }
        Err(TrySendError::Disconnected(_)) => worker_gone.store(true, Ordering::Release),
    }
}

/// Drains mono blocks, resamples to 16 kHz, calls the sink. Runs on the worker thread only.
fn run_worker<S: FnMut(&[f32]) + ?Sized>(
    in_hz: usize,
    rx: &Receiver<Vec<f32>>,
    free: &SyncSender<Vec<f32>>,
    sink: &mut S,
    heard: &AtomicBool,
    shutdown: &AtomicBool,
) -> Result<(), TapError> {
    let mut path = ReadPath::new(in_hz);
    let mut mark = |frame: &[f32]| {
        if !heard.load(Ordering::Relaxed) && frame.iter().any(|sample| *sample != 0.0) {
            heard.store(true, Ordering::Relaxed);
        }
        sink(frame);
    };
    // ponytail: polling keeps both channels free of blocked waiters, so the IO thread's
    // try_recv/try_send never wake anyone. Upgrade: lock-free SPSC ring if 5 ms ever matters.
    loop {
        if shutdown.load(Ordering::Acquire) {
            break;
        }
        match rx.try_recv() {
            Ok(block) => {
                path.push(&block, &mut mark);
                let _ = free.try_send(block);
            }
            Err(TryRecvError::Empty) => std::thread::sleep(Duration::from_millis(5)),
            Err(TryRecvError::Disconnected) => break,
        }
    }
    path.finish(&mut mark)
}

type Worker = JoinHandle<Result<(), TapError>>;
type Spawned = (SyncSender<Vec<f32>>, Receiver<Vec<f32>>, Worker);
fn spawn_worker(
    in_hz: usize,
    block_frames: usize,
    mut sink: TapSink,
    heard: Arc<AtomicBool>,
    shutdown: Arc<AtomicBool>,
) -> Result<Spawned, TapError> {
    let (tx, rx) = mpsc::sync_channel::<Vec<f32>>(QUEUE_BLOCKS);
    let (free_tx, free) = mpsc::sync_channel::<Vec<f32>>(QUEUE_BLOCKS);
    for _ in 0..QUEUE_BLOCKS {
        let _ = free_tx.try_send(Vec::with_capacity(block_frames));
    }
    let worker = std::thread::Builder::new()
        .name("system-audio-tap".into())
        .spawn(move || run_worker(in_hz, &rx, &free_tx, &mut *sink, &heard, &shutdown))
        .map_err(|err| TapError::Device(format!("tap worker thread: {err}")))?;
    Ok((tx, free, worker))
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
            let buf = &input_data.buffers[0];
            if buf.data.is_null() || buf.data_bytes_size < 4 {
                return Status::NO_ERR;
            }
            let count = buf.data_bytes_size as usize / std::mem::size_of::<f32>();
            // SAFETY: HAL owns this buffer for the duration of the IO proc. Length is the byte size.
            let raw = unsafe { std::slice::from_raw_parts(buf.data as *const f32, count) };
            io_block(state, raw);
        }
    }
    Status::NO_ERR
}

/// IO thread: no alloc, no block, no lock. Any drop (no buffer, oversize, full queue) is counted.
fn io_block(state: &IoState, raw: &[f32]) {
    // A failed stop leaks this state with the queue still open, so IO can still call here after
    // the worker exited. Touching the pool then would free a block on the real-time thread.
    if state.shutdown.load(Ordering::Acquire) {
        state.dropped.fetch_add(1, Ordering::Relaxed);
        return;
    }
    match state.free.try_recv() {
        Ok(mut block) => {
            let fits = mono_into(raw, state.channels, &mut block); // oversize: sent empty, recycled
            let sent = state.tx.try_send(block);
            if !fits && sent.is_ok() {
                state.dropped.fetch_add(1, Ordering::Relaxed); // one count per lost block
            }
            record_send(sent, &state.dropped, &state.worker_gone);
        }
        Err(TryRecvError::Empty) => {
            state.dropped.fetch_add(1, Ordering::Relaxed);
        }
        // The worker owns the pool's sender, so a closed pool also means the worker is gone.
        Err(TryRecvError::Disconnected) => state.worker_gone.store(true, Ordering::Release),
    }
}

/// Downmixes `raw` into `out` (cleared, never grown); false, writing nothing, if it doesn't fit.
fn mono_into(raw: &[f32], channels: u32, out: &mut Vec<f32>) -> bool {
    let channels = channels.max(1) as usize;
    out.clear();
    if raw.len() > out.capacity() * channels {
        return false;
    }
    for frame in raw.chunks(channels) {
        out.push(frame.iter().sum::<f32>() / channels as f32);
    }
    true
}

/// `mono_into` reads the tap buffers as interleaved f32, so reject anything else before IO starts.
fn check_tap_format(asbd: &cidre::cat::audio::StreamBasicDesc) -> Result<(), TapError> {
    let channels = asbd.channels_per_frame;
    if channels == 0 {
        return Err(TapError::Device(
            "unsupported tap format: 0 channels".into(),
        ));
    }
    if asbd.format != Format::LINEAR_PCM {
        return Err(TapError::Device(format!(
            "unsupported tap format: format {:?}",
            asbd.format
        )));
    }
    if !asbd.format_flags.contains(FormatFlags::IS_FLOAT) {
        return Err(TapError::Device(
            "unsupported tap format: format_flags without IS_FLOAT".into(),
        ));
    }
    if asbd.format_flags.contains(FormatFlags::IS_NON_INTERLEAVED) {
        return Err(TapError::Device(
            "unsupported tap format: format_flags has IS_NON_INTERLEAVED".into(),
        ));
    }
    if asbd.bits_per_channel != 32 {
        return Err(TapError::Device(format!(
            "unsupported tap format: bits_per_channel {}",
            asbd.bits_per_channel
        )));
    }
    if u64::from(asbd.bytes_per_frame) != 4 * u64::from(channels) {
        return Err(TapError::Device(format!(
            "unsupported tap format: bytes_per_frame {} for {channels} channels",
            asbd.bytes_per_frame
        )));
    }
    Ok(())
}

// cidre 0.29 has no AggregateDevice::destroy. AudioHardwareDestroyAggregateDevice is the C call.
#[link(name = "CoreAudio", kind = "framework")]
extern "C" {
    fn AudioHardwareDestroyAggregateDevice(in_device: Obj) -> Status;
}

fn destroy_aggregate(id: u32) -> Result<(), TapError> {
    // SAFETY: id is an aggregate this process created. Apple frees it asynchronously.
    let status = unsafe { AudioHardwareDestroyAggregateDevice(Obj(id)) };
    if status == Status::NO_ERR {
        Ok(())
    } else {
        Err(TapError::Device(format!("destroy aggregate: {status:?}")))
    }
}

fn release_partial(tap: TapGuard, aggregate_id: Option<u32>, state: *mut IoState) {
    if let Some(id) = aggregate_id {
        // Nobody to return to while unwinding start().
        let _ = destroy_aggregate(id);
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
    worker_gone: Arc<AtomicBool>,
    heard: Arc<AtomicBool>,
    shutdown: Arc<AtomicBool>,
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
        if let Err(err) = check_tap_format(&asbd) {
            release_partial(tap, None, std::ptr::null_mut());
            return Err(err);
        }
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

        let max = aggregate.buf_frame_size_range().map_or(0.0, |r| r.max) as usize;
        let block_frames = max.clamp(BLOCK_FRAMES, 1 << 16); // a broken max can't size the pool
        let dropped = Arc::new(AtomicU64::new(0));
        let worker_gone = Arc::new(AtomicBool::new(false));
        let heard = Arc::new(AtomicBool::new(false));
        let shutdown = Arc::new(AtomicBool::new(false));
        let (tx, free, worker) = match spawn_worker(
            rate,
            block_frames,
            sink,
            Arc::clone(&heard),
            Arc::clone(&shutdown),
        ) {
            Ok(pair) => pair,
            Err(err) => {
                release_partial(tap, Some(aggregate_id), std::ptr::null_mut());
                return Err(err);
            }
        };

        let state = Box::into_raw(Box::new(IoState {
            channels,
            free,
            tx,
            dropped: Arc::clone(&dropped),
            worker_gone: Arc::clone(&worker_gone),
            shutdown: Arc::clone(&shutdown),
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
                worker_gone,
                heard,
                shutdown,
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

    /// True once the worker's queue closed under the IO thread: the worker exited or panicked,
    /// so capture is dead even though IO may still be running.
    pub fn worker_gone(&self) -> bool {
        self.worker_gone.load(Ordering::Acquire)
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
            self.abandon_state()
        } else {
            self.finish_worker()
        };
        let destroy_err = match self.aggregate_id.take() {
            Some(id) => destroy_aggregate(id).err(),
            None => None,
        };
        self.tap.take();
        if let Some(err) = finish_err.or(stop_err).or(destroy_err) {
            return Err(err);
        }
        if self.worker_gone.load(Ordering::Acquire) {
            return Err(TapError::Device("tap worker stopped during capture".into()));
        }
        Ok(())
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

    /// After a failed stop, IO may still call `on_input` with `state`: Apple frees the aggregate
    /// asynchronously, so nothing proves IO has quiesced. The state is leaked on purpose (one
    /// IoState plus its pool, 2-16 MB), only when the HAL refuses to stop. The worker is told to
    /// exit and joined, so stop() still reports finish()'s outcome. `stop()` and `Drop` still
    /// destroy the aggregate and drop the process tap, so no capture keeps running.
    fn abandon_state(&mut self) -> Option<TapError> {
        self.state = std::ptr::null_mut();
        self.shutdown.store(true, Ordering::Release);
        self.join_worker()
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
                let _ = self.abandon_state();
            }
        }
        if !self.state.is_null() {
            // SAFETY: IO stopped above (or never started), so the proc is not in on_input.
            drop(unsafe { Box::from_raw(self.state) });
            self.state = std::ptr::null_mut();
        }
        let _ = self.join_worker();
        if let Some(id) = self.aggregate_id.take() {
            // Nobody to return to while dropping.
            let _ = destroy_aggregate(id);
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
        pull_err: Option<TapError>,
        stop_err: Option<TapError>,
    }

    impl Default for FakeDevice {
        fn default() -> Self {
            Self {
                rate: 48_000,
                samples: Vec::new(),
                starts: 0,
                pull_err: None,
                stop_err: None,
            }
        }
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
            match self.pull_err.take() {
                Some(err) => Err(err),
                None => Ok(std::mem::take(&mut self.samples)),
            }
        }

        fn stop(&mut self) -> Result<(), TapError> {
            match self.stop_err.take() {
                Some(err) => Err(err),
                None => Ok(()),
            }
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
            ..FakeDevice::default()
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
            ..FakeDevice::default()
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
    fn usage_string_needs_a_non_empty_value() {
        let ok = format!("<key>{USAGE_KEY}</key>\n<string>records meetings</string>");
        require_usage_string(&ok).expect("key with a value");

        for empty in ["<string></string>", "<string>  </string>"] {
            let text = format!("<key>{USAGE_KEY}</key>{empty}");
            assert_eq!(
                require_usage_string(&text),
                Err(TapError::UsageStringMissing),
                "{empty} counted as a usage string"
            );
        }
    }

    #[test]
    fn usage_key_name_alone_is_missing() {
        for text in [
            String::new(),
            format!("<string>{USAGE_KEY}</string>"),
            format!("<!-- {USAGE_KEY} -->"),
        ] {
            assert_eq!(
                require_usage_string(&text),
                Err(TapError::UsageStringMissing),
                "{text} counted as a usage string"
            );
        }
    }

    #[test]
    fn tap_format_must_be_interleaved_f32() {
        use cidre::cat::audio::StreamBasicDesc;

        check_tap_format(&StreamBasicDesc::common_f32(48_000.0, 2, true))
            .expect("interleaved f32 stereo");
        check_tap_format(&StreamBasicDesc::common_f32(48_000.0, 1, true))
            .expect("interleaved f32 mono");

        let non_interleaved = StreamBasicDesc::common_f32(48_000.0, 2, false);
        assert!(
            check_tap_format(&non_interleaved).is_err(),
            "non-interleaved accepted"
        );

        let mut int16 = StreamBasicDesc::common_f32(48_000.0, 2, true);
        int16.bits_per_channel = 16;
        int16.bytes_per_frame = 4;
        int16.format_flags = FormatFlags::NATIVE_ENDIAN;
        assert!(check_tap_format(&int16).is_err(), "16-bit int accepted");

        let mut zero_channels = StreamBasicDesc::common_f32(48_000.0, 1, true);
        zero_channels.channels_per_frame = 0;
        assert!(
            check_tap_format(&zero_channels).is_err(),
            "0 channels accepted"
        );

        let mut wrong_bytes = StreamBasicDesc::common_f32(48_000.0, 2, true);
        wrong_bytes.bytes_per_frame = 4;
        assert!(
            check_tap_format(&wrong_bytes).is_err(),
            "bytes_per_frame mismatch accepted"
        );
    }

    #[test]
    fn capture_keeps_stop_and_pull_errors() {
        let mut device = FakeDevice {
            samples: sine(48_000, 440.0, 0.1),
            stop_err: Some(TapError::Device("stop failed".into())),
            ..FakeDevice::default()
        };
        let err = capture(PLIST_WITH_USAGE, &mut device, &mut |_| {}).expect_err("stop error");
        assert_eq!(err, TapError::Device("stop failed".into()));

        let mut device = FakeDevice {
            pull_err: Some(TapError::PermissionDenied),
            stop_err: Some(TapError::Device("stop failed".into())),
            ..FakeDevice::default()
        };
        let err = capture(PLIST_WITH_USAGE, &mut device, &mut |_| {}).expect_err("both failed");
        assert_eq!(
            err,
            TapError::PermissionDenied,
            "pull error lost its variant when stop also failed"
        );
    }

    #[test]
    fn record_send_ok_and_full() {
        let dropped = AtomicU64::new(0);
        let worker_gone = AtomicBool::new(false);
        record_send(Ok(()), &dropped, &worker_gone);
        assert_eq!(dropped.load(Ordering::Relaxed), 0);
        assert!(!worker_gone.load(Ordering::Relaxed));

        record_send(Err(TrySendError::Full(Vec::new())), &dropped, &worker_gone);
        assert_eq!(dropped.load(Ordering::Relaxed), 1, "full queue not counted");
        assert!(
            !worker_gone.load(Ordering::Relaxed),
            "full queue flagged the worker gone"
        );

        // Same split on the real IO path: a full queue drops and counts, the worker is alive.
        let (tx, _rx) = mpsc::sync_channel::<Vec<f32>>(1);
        tx.send(Vec::new()).expect("fill queue");
        let (free_tx, free) = mpsc::sync_channel::<Vec<f32>>(1);
        free_tx.send(Vec::with_capacity(2)).expect("seed pool");
        let state = IoState {
            channels: 1,
            free,
            tx,
            dropped: Arc::new(AtomicU64::new(0)),
            worker_gone: Arc::new(AtomicBool::new(false)),
            shutdown: Arc::new(AtomicBool::new(false)),
        };
        io_block(&state, &[1.0]);
        assert_eq!(
            state.dropped.load(Ordering::Relaxed),
            1,
            "full queue not counted"
        );
        assert!(
            !state.worker_gone.load(Ordering::Relaxed),
            "full queue flagged the worker gone"
        );
    }

    #[test]
    fn record_send_disconnected_marks_worker_gone() {
        let dropped = AtomicU64::new(0);
        let worker_gone = AtomicBool::new(false);
        record_send(
            Err(TrySendError::Disconnected(Vec::new())),
            &dropped,
            &worker_gone,
        );
        assert_eq!(
            dropped.load(Ordering::Relaxed),
            0,
            "disconnected queue counted as a drop"
        );
        assert!(
            worker_gone.load(Ordering::Relaxed),
            "disconnected queue left the worker flag down"
        );

        // On the real IO path both closed channels mean a dead worker, never a drop.
        for close_pool in [false, true] {
            let (tx, rx) = mpsc::sync_channel::<Vec<f32>>(1);
            let (free_tx, free) = mpsc::sync_channel::<Vec<f32>>(1);
            if close_pool {
                drop(free_tx); // pool empty and its sender gone
            } else {
                free_tx.send(Vec::with_capacity(2)).expect("seed pool");
                drop(rx); // worker's queue closed
            }
            let state = IoState {
                channels: 1,
                free,
                tx,
                dropped: Arc::new(AtomicU64::new(0)),
                worker_gone: Arc::new(AtomicBool::new(false)),
                shutdown: Arc::new(AtomicBool::new(false)),
            };
            io_block(&state, &[1.0]);
            assert_eq!(
                state.dropped.load(Ordering::Relaxed),
                0,
                "dead worker counted as a drop"
            );
            assert!(
                state.worker_gone.load(Ordering::Relaxed),
                "close_pool={close_pool}: worker not flagged"
            );
        }
    }

    #[test]
    fn silent_stream_is_not_success() {
        let mut device = FakeDevice {
            rate: 48_000,
            samples: vec![0.0; 48_000 / 10],
            starts: 0,
            ..FakeDevice::default()
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
        let (free_tx, _free) = mpsc::sync_channel::<Vec<f32>>(QUEUE_BLOCKS);
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
            &free_tx,
            &mut |frame: &[f32]| got.extend_from_slice(frame),
            &heard,
            &AtomicBool::new(false),
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
        let err = run_worker(
            48_000,
            &rx,
            &free_tx,
            &mut |_: &[f32]| {},
            &quiet,
            &AtomicBool::new(false),
        )
        .expect_err("all-zero blocks");
        assert_eq!(err, TapError::Silent);
        assert!(!quiet.load(Ordering::Relaxed), "zeros counted as audio");
    }

    #[test]
    fn failed_stop_worker_exits_and_reports_finish() {
        // The leaked IoState keeps `tx` open, so only `shutdown` can end the worker.
        let (tx, rx) = mpsc::sync_channel::<Vec<f32>>(QUEUE_BLOCKS);
        let (free_tx, _free) = mpsc::sync_channel::<Vec<f32>>(QUEUE_BLOCKS);
        let (shutdown, heard) = (
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicBool::new(false)),
        );
        let flag = Arc::clone(&shutdown);
        let worker = std::thread::spawn(move || {
            run_worker(48_000, &rx, &free_tx, &mut |_: &[f32]| {}, &heard, &flag)
        });
        tx.send(vec![0.0; 480]).expect("queue");
        shutdown.store(true, Ordering::Release);
        let (done_tx, done) = mpsc::channel();
        std::thread::spawn(move || done_tx.send(worker.join().expect("worker panicked")));
        let outcome = done
            .recv_timeout(Duration::from_secs(2))
            .expect("worker never exited");
        assert_eq!(outcome, Err(TapError::Silent), "finish() outcome lost");
        drop(tx);
    }

    #[test]
    fn io_block_reuses_pool_buffers_and_counts_every_drop() {
        let (tx, rx) = mpsc::sync_channel::<Vec<f32>>(1);
        let (free_tx, free) = mpsc::sync_channel::<Vec<f32>>(1);
        free_tx.send(Vec::with_capacity(2)).expect("seed pool");
        let dropped = Arc::new(AtomicU64::new(0));
        let state = IoState {
            channels: 2,
            free,
            tx,
            dropped: Arc::clone(&dropped),
            worker_gone: Arc::new(AtomicBool::new(false)),
            shutdown: Arc::new(AtomicBool::new(false)),
        };
        io_block(&state, &[1.0, 3.0, 5.0, 7.0]);
        let block = rx.try_recv().expect("mono block");
        assert_eq!((block.as_slice(), block.capacity()), (&[2.0, 6.0][..], 2));
        io_block(&state, &[1.0, 3.0]); // pool empty: dropped
        assert_eq!(dropped.load(Ordering::Relaxed), 1);
        free_tx.send(block).expect("return buffer");
        io_block(&state, &[0.0; 6]); // 3 frames > capacity 2: dropped, never grown
        assert_eq!(dropped.load(Ordering::Relaxed), 2);
        assert_eq!(rx.try_recv().map(|b| b.len()), Ok(0)); // buffer recycled, no oversize data
    }

    #[test]
    fn io_block_after_shutdown_leaves_pool_untouched() {
        // A failed stop leaks the state with the queue still open, so IO can still call this.
        let (tx, rx) = mpsc::sync_channel::<Vec<f32>>(1);
        let (free_tx, free) = mpsc::sync_channel::<Vec<f32>>(1);
        free_tx.send(Vec::with_capacity(2)).expect("seed pool");
        let dropped = Arc::new(AtomicU64::new(0));
        let state = IoState {
            channels: 2,
            free,
            tx,
            dropped: Arc::clone(&dropped),
            worker_gone: Arc::new(AtomicBool::new(false)),
            shutdown: Arc::new(AtomicBool::new(true)),
        };
        io_block(&state, &[1.0, 3.0]);
        let kept = state.free.try_recv().expect("shutdown path drained the pool");
        assert_eq!(kept.capacity(), 2, "pool buffer left the pool after shutdown");
        assert_eq!(
            rx.try_recv(),
            Err(TryRecvError::Empty),
            "block sent after shutdown"
        );
        assert_eq!(dropped.load(Ordering::Relaxed), 1, "shutdown not counted");
        assert!(
            !state.worker_gone.load(Ordering::Relaxed),
            "shutdown flagged the worker gone"
        );
    }
}
