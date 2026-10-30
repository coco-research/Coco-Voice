//! Me track for a meeting: a second [`AudioRecorder`] on the input device the
//! user already picked, opened with [`VadPolicy::Disabled`], appending every
//! 16 kHz sample to `{app_data_dir}/meetings/{id}/mic.wav`.
//!
//! Dictation keeps its own recorder and its own `RecordingState`. Nothing here
//! calls `try_start_recording` or touches that state. The one thing this module
//! shares with dictation is the microphone, so a device change is refused by
//! [`crate::managers::audio::mic_change_refusal`] for as long as the Me track
//! holds it.
//!
//! VAD off is the point. With an offline or streaming policy the recorder drops
//! noise frames (`recorder.rs`), so the Me file would lose its place on the
//! meeting clock. Silero runs only in the chunker.

#![cfg(feature = "meetings")]

use anyhow::{anyhow, Result};
use log::{info, warn};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tauri::AppHandle;

use crate::audio_toolkit::audio::AppendWav;
use crate::audio_toolkit::{list_input_devices, AudioRecorder, VadPolicy};
use crate::settings::get_settings;

/// mic.wav is 16 kHz mono 16-bit, the meeting format. The recorder resamples
/// the device to this rate before the audio callback sees a frame.
pub const TRACK_RATE: u32 = 16_000;

/// How many Me tracks hold the microphone. Process-wide because the device is:
/// dictation opens the same one, so a swap restarts both streams and drops
/// whatever was in flight. One meeting at a time is the rule, so this is 0 or 1
/// in practice; a count keeps it exact if a start is retried.
static MIC_HOLDERS: AtomicUsize = AtomicUsize::new(0);

/// True while a meeting's Me track holds the microphone.
pub fn mic_recording_active() -> bool {
    MIC_HOLDERS.load(Ordering::SeqCst) > 0
}

/// A stretch where the Me track kept no audio, in milliseconds from the meeting
/// start (wall clock, so it lines up with the Others track).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MicGap {
    pub start_ms: u64,
    pub end_ms: u64,
}

/* ──────────────────────────────────────────────────────────────── */

/// The sample path and its bookkeeping, kept free of cpal and Tauri so a test
/// can drive it with generated PCM.
struct MicTrack {
    wav: AppendWav,
    /// The meeting clock. Gaps are measured from here.
    started: Instant,
    /// Set while the recorder is closed or dead: samples are dropped, and the
    /// hole is what the gap records.
    paused: bool,
    gaps: Vec<MicGap>,
}

impl MicTrack {
    fn started_at(wav: AppendWav, started: Instant) -> Self {
        Self {
            wav,
            started,
            paused: false,
            gaps: Vec::new(),
        }
    }

    fn elapsed_ms(&self) -> u64 {
        u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX)
    }

    /// What the cpal callback calls, once per 16 kHz frame, while the recorder
    /// runs `VadPolicy::Disabled` (every frame forwarded, silence included).
    /// f32 to i16 is the whole conversion: the stream is already 16 kHz mono.
    fn push(&mut self, samples: &[f32]) -> Result<()> {
        if self.paused {
            return Ok(());
        }
        let pcm: Vec<i16> = samples
            .iter()
            .map(|s| (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)
            .collect();
        self.wav.append(&pcm)
    }

    /// Stop keeping samples and open a gap at the current meeting time.
    fn pause(&mut self) {
        if self.paused {
            return;
        }
        self.paused = true;
        let at = self.elapsed_ms();
        self.gaps.push(MicGap {
            start_ms: at,
            end_ms: at,
        });
    }

    /// Close the open gap at the current meeting time. Called on a reopen and
    /// on stop, so `gaps()` is right whether the track came back or not.
    fn resume(&mut self) {
        if !self.paused {
            return;
        }
        self.paused = false;
        let now = self.elapsed_ms();
        if let Some(gap) = self.gaps.last_mut() {
            gap.end_ms = now.max(gap.start_ms);
        }
    }
}

/* ──────────────────────────────────────────────────────────────── */

// The meeting controller (PR6) constructs this; until then only tests use it.
#[allow(dead_code)]
pub struct MeetingMic {
    path: PathBuf,
    /// The device the user picked, or `None` for the system default (the
    /// recorder resolves that itself, cheaply).
    device_name: Option<String>,
    track: Arc<Mutex<MicTrack>>,
    /// `None` while paused. Rebuilt, never reused: the recorder caches its
    /// stream config by device name, so reusing it after a rate change would
    /// hand back the old rate.
    recorder: Mutex<Option<AudioRecorder>>,
    /// Why the Me track is paused, for the meeting screen.
    error: Mutex<Option<String>>,
    held: AtomicBool,
}

#[allow(dead_code)]
impl MeetingMic {
    /// Create `{app_data_dir}/meetings/{id}/mic.wav` and open the second
    /// recorder on the user's selected input with VAD disabled.
    ///
    /// `Err` only when the meeting folder or the file cannot be created: with
    /// no file there is no track. A device that refuses to open is not an
    /// error, because nothing else in the meeting may stop for it: the track
    /// comes back paused, with a gap, and `error()` says why.
    pub fn open(app: &AppHandle, meeting_id: i64) -> Result<Self> {
        let dir = crate::portable::app_data_dir(app)?
            .join("meetings")
            .join(meeting_id.to_string());
        fs::create_dir_all(&dir)?;
        let mic = Self::from_wav(
            dir.join("mic.wav"),
            get_settings(app).selected_microphone.clone(),
            Instant::now(),
        )?;
        mic.hold_microphone();
        if let Err(e) = mic.start_stream() {
            mic.fail(&e);
        }
        info!(
            "meeting {meeting_id}: me track at {} (VAD off)",
            mic.path().display()
        );
        Ok(mic)
    }

    fn from_wav(path: PathBuf, device_name: Option<String>, started: Instant) -> Result<Self> {
        let wav = AppendWav::create(&path)?;
        Ok(Self {
            path,
            device_name,
            track: Arc::new(Mutex::new(MicTrack::started_at(wav, started))),
            recorder: Mutex::new(None),
            error: Mutex::new(None),
            held: AtomicBool::new(false),
        })
    }

    /// The function the cpal callback calls. Frames arrive untouched because
    /// this recorder runs `VadPolicy::Disabled`; the callback below is the same
    /// code path, and tests feed generated PCM here with no device.
    pub fn push(&self, samples: &[f32]) -> Result<()> {
        self.track.lock().unwrap().push(samples)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Gaps recorded so far, in order. This is what the meeting screen shows,
    /// and what a later PR writes beside the segments.
    pub fn gaps(&self) -> Vec<MicGap> {
        self.track.lock().unwrap().gaps.clone()
    }

    pub fn is_paused(&self) -> bool {
        self.track.lock().unwrap().paused
    }

    /// Why the Me track is paused, if it is.
    pub fn error(&self) -> Option<String> {
        self.error.lock().unwrap().clone()
    }

    /// The stream died: device unplugged, cpal error, or a Bluetooth profile
    /// switch the recorder did not survive. Pause, keep the gap, let go of the
    /// recorder. Others keeps running; `reopen` brings Me back.
    pub fn on_stream_died(&self, reason: &str) {
        self.close_stream();
        self.fail(&anyhow!("{reason}"));
    }

    /// The input rate changed under the recorder (a Bluetooth headset dropping
    /// to 8 or 16 kHz when its mic opens). Rebuild the recorder at the new rate
    /// and record a gap; Others is untouched.
    pub fn on_input_rate_changed(&self, new_rate: u32) -> Result<()> {
        info!("meeting mic input rate changed to {new_rate} Hz; reopening and marking a gap");
        self.on_stream_died(&format!("input sample rate changed to {new_rate} Hz"));
        self.reopen()
    }

    /// Reopen after a pause and close the gap. On failure the track stays
    /// paused and the gap stays open, so a later success covers the whole hole.
    pub fn reopen(&self) -> Result<()> {
        self.close_stream();
        match self.start_stream() {
            Ok(()) => {
                *self.error.lock().unwrap() = None;
                self.track.lock().unwrap().resume();
                Ok(())
            }
            Err(e) => {
                self.fail(&e);
                Err(e)
            }
        }
    }

    /// Stop the recorder and let go of the microphone. The file stays; the last
    /// one-second flush is the durable point.
    pub fn stop(&self) {
        self.release_microphone();
        self.close_stream();
        let mut track = self.track.lock().unwrap();
        track.resume();
        if let Err(e) = track.wav.flush() {
            warn!("meeting mic flush failed: {e:#}");
        }
    }

    /* ---------- device ------------------------------------------------------ */

    /// Open the second recorder with VAD off and start it. A fresh recorder
    /// every time: its config cache is keyed by device name, so a Bluetooth
    /// rate change must not reuse the old one.
    fn start_stream(&self) -> Result<()> {
        let device = self.resolve_device();
        let track = Arc::clone(&self.track);
        let mut recorder = AudioRecorder::new()
            .map_err(|e| anyhow!("Failed to create the meeting recorder: {e}"))?
            .with_audio_callback(move |frame: &[f32]| {
                // Audio callback: never panic, never block. The writer refuses
                // after its own write error, and `push`/`error()` carry the
                // reason to the caller.
                if let Err(e) = track.lock().unwrap().push(frame) {
                    warn!("meeting mic write failed: {e:#}");
                }
            });
        recorder
            .open(device)
            .map_err(|e| anyhow!("Failed to open the meeting microphone: {e}"))?;
        // VAD disabled: every frame is kept, so silence stays in the file and
        // the Me track keeps the meeting clock.
        recorder
            .start(VadPolicy::Disabled)
            .map_err(|e| anyhow!("Failed to start the meeting microphone: {e}"))?;
        *self.recorder.lock().unwrap() = Some(recorder);
        Ok(())
    }

    /// The user's selected input, or `None` so the recorder opens the system
    /// default. Mirrors what dictation resolves, without its cached handle.
    fn resolve_device(&self) -> Option<cpal::Device> {
        let name = self.device_name.as_ref()?;
        list_input_devices()
            .ok()?
            .into_iter()
            .find(|d| d.name == *name)
            .map(|d| d.device)
    }

    /// Pause the Me track and record the hole. Nothing else stops: the Others
    /// track, dictation, and the meeting row are not ours to stop.
    fn fail(&self, reason: &anyhow::Error) {
        self.track.lock().unwrap().pause();
        *self.error.lock().unwrap() = Some(format!("{reason:#}"));
        warn!("meeting mic paused: {reason:#}");
    }

    fn close_stream(&self) {
        if let Some(mut recorder) = self.recorder.lock().unwrap().take() {
            let _ = recorder.close();
        }
    }

    fn hold_microphone(&self) {
        if !self.held.swap(true, Ordering::SeqCst) {
            MIC_HOLDERS.fetch_add(1, Ordering::SeqCst);
        }
    }

    fn release_microphone(&self) {
        if self.held.swap(false, Ordering::SeqCst) {
            let _ = MIC_HOLDERS.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| {
                Some(n.saturating_sub(1))
            });
        }
    }
}

impl Drop for MeetingMic {
    /// A meeting that never calls `stop` must not leave the microphone flagged
    /// as held, or no device change would ever be allowed again.
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio_toolkit::read_wav_samples;
    use crate::managers::audio::{
        mic_change_refusal, take_allowed, RecordingState, MIC_CHANGE_DURING_MEETING,
        MIC_CHANGE_DURING_TAKE,
    };
    use std::time::Duration;

    /// A Me track on a temp file, no device. It holds the microphone the way
    /// `open` does.
    fn mic(dir: &Path) -> MeetingMic {
        let mic = MeetingMic::from_wav(dir.join("mic.wav"), None, Instant::now()).unwrap();
        mic.hold_microphone();
        mic
    }

    fn read_back(mic: &MeetingMic) -> Vec<f32> {
        {
            let mut track = mic.track.lock().unwrap();
            track.wav.flush().unwrap();
        }
        read_wav_samples(mic.path()).unwrap()
    }

    fn tone(seconds: f32) -> Vec<f32> {
        let n = (TRACK_RATE as f32 * seconds) as usize;
        (0..n)
            .map(|i| {
                (i as f32 * 2.0 * std::f32::consts::PI * 220.0 / TRACK_RATE as f32).sin() * 0.5
            })
            .collect()
    }

    /// Push as 30 ms frames, the size the recorder's resampler hands the
    /// callback.
    fn feed(mic: &MeetingMic, input: &[f32]) {
        for frame in input.chunks(480) {
            mic.push(frame).unwrap();
        }
    }

    #[test]
    fn meeting_mic_does_not_take_dictation_state() {
        let dir = tempfile::tempdir().unwrap();
        let mic = mic(dir.path());

        let speech = tone(1.0);
        feed(&mic, &speech);
        let got = read_back(&mic);
        assert_eq!(got.len(), speech.len(), "samples did not land in mic.wav");

        // The Me track is a second recorder: it never enters the dictation
        // state, so `try_start_recording` still sees Idle and answers with a
        // take, not "Already recording".
        let dictation = RecordingState::Idle;
        assert!(
            take_allowed(&dictation),
            "a dictation take would be refused while the meeting records"
        );
    }

    #[test]
    fn meeting_mic_keeps_silence() {
        let dir = tempfile::tempdir().unwrap();
        let mic = mic(dir.path());

        let mut input = tone(1.0);
        input.extend(std::iter::repeat(0.0).take(5 * TRACK_RATE as usize));
        input.extend(tone(1.0));
        // VAD disabled means the callback hands over every frame. Nothing here
        // may drop the silent ones.
        feed(&mic, &input);

        let got = read_back(&mic);
        assert_eq!(
            got.len(),
            input.len(),
            "the WAV lost samples: {} of {}",
            got.len(),
            input.len()
        );
        let silence = TRACK_RATE as usize..6 * TRACK_RATE as usize;
        assert!(
            got[silence].iter().all(|s| *s == 0.0),
            "the 5 s of silence were dropped or altered"
        );
        assert!(!mic.is_paused());
    }

    #[test]
    fn mic_change_refused_while_meeting_records() {
        let dir = tempfile::tempdir().unwrap();
        let dictation = RecordingState::Idle;

        // Between takes dictation allows the swap...
        assert!(mic_change_refusal(&dictation, false).is_none());
        // ...a meeting adds its own refusal beside that rule.
        assert_eq!(
            mic_change_refusal(&dictation, true),
            Some(MIC_CHANGE_DURING_MEETING)
        );
        // A take is still refused mid-dictation, for the older reason.
        let take = RecordingState::Recording {
            binding_id: "transcribe".to_string(),
        };
        assert_eq!(
            mic_change_refusal(&take, false),
            Some(MIC_CHANGE_DURING_TAKE)
        );

        // And a live Me track really raises the flag the manager reads.
        let mic = mic(dir.path());
        assert!(
            mic_recording_active(),
            "the meeting never flagged the microphone as held"
        );
        mic.stop();
    }

    #[test]
    fn me_track_gap_is_recorded_when_the_stream_dies() {
        let dir = tempfile::tempdir().unwrap();
        // Back-date the meeting clock so the gap has a real start: the meeting
        // began five seconds ago and one second of audio is in the file.
        let started = Instant::now()
            .checked_sub(Duration::from_secs(5))
            .unwrap_or_else(Instant::now);
        let mic = MeetingMic::from_wav(dir.path().join("mic.wav"), None, started).unwrap();
        mic.hold_microphone();
        let speech = tone(1.0);
        feed(&mic, &speech);

        mic.on_stream_died("device unplugged");
        assert!(mic.is_paused());
        let reason = mic.error().unwrap_or_default();
        assert!(reason.contains("device unplugged"), "error is {reason:?}");

        // A paused track keeps nothing, and does not mistake the hole for audio.
        feed(&mic, &tone(1.0));
        let got = read_back(&mic);
        assert_eq!(got.len(), speech.len(), "a paused Me track wrote audio");

        mic.stop();
        let gaps = mic.gaps();
        assert_eq!(gaps.len(), 1, "the dead stream left no gap: {gaps:?}");
        assert!(
            (4_000..=6_500).contains(&gaps[0].start_ms),
            "gap starts at {} ms, not where the audio stopped",
            gaps[0].start_ms
        );
        assert!(gaps[0].end_ms >= gaps[0].start_ms);
        assert!(
            gaps[0].end_ms - gaps[0].start_ms < 1_500,
            "gap is longer than the pause: {gaps:?}"
        );
    }
}
