//! Meeting lifecycle: start, stop, finish, rollback, and the tap. `meeting_feed` writes each track.
//!
//! The tap starts before the row. A refusal leaves no row and no file. A write
//! error finalises with what already reached disk.

#![cfg(feature = "meetings")]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use tauri::{AppHandle, Manager};

use crate::audio_toolkit::audio::AppendWav;
use crate::managers::meeting_capture::MeetingMic;
use crate::managers::meeting_chunker::{
    plan_meeting_model, MeetingStartRefusal, CHANNEL_ME, CHANNEL_OTHERS,
};
use crate::managers::meetings::{DeleteOutcome, MeetingsStore};
use crate::managers::model::ModelManager;
use crate::managers::transcription::TranscriptionManager;
use crate::meeting_controller::{
    MeetingEnd, MeetingRecorders, MeetingRefusal, MeetingWarning, SessionPoll, StartedMeeting,
};
#[cfg(test)]
use crate::meeting_feed::write_optional;
use crate::meeting_feed::{chunker, push_warning, spawn_worker, Feed, TranscribeFn};

const SILENT_AFTER_SECS: u64 = 10;

fn poison<T>(err: std::sync::PoisonError<T>) -> T {
    err.into_inner()
}

fn refuse_io(err: impl std::fmt::Display) -> MeetingRefusal {
    log::warn!("meeting start failed: {err}");
    MeetingRefusal::SystemAudioUnavailable
}

struct Active {
    id: i64,
    feed: Arc<Feed>,
    worker: Option<JoinHandle<()>>,
    mic: Option<MeetingMic>,
    #[cfg(target_os = "macos")]
    tap: Option<crate::meetings_tap::SystemAudioSession>,
    mic_only: bool,
    fake: bool,
    store: MeetingsStore,
}

impl Drop for Active {
    fn drop(&mut self) {
        self.feed.tx.lock().unwrap_or_else(poison).take();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

struct TapHold {
    #[cfg(target_os = "macos")]
    gate: Arc<Mutex<TapBuf>>,
    #[cfg(target_os = "macos")]
    tap: crate::meetings_tap::SystemAudioSession,
}

#[cfg(target_os = "macos")]
struct TapBuf {
    pending: Vec<f32>,
    feed: Option<Arc<Feed>>,
}

/// Real recorders. Tests call [`MeetingSession::start_for_test`] and push samples.
pub struct MeetingSession {
    app: Option<AppHandle>,
    active: Mutex<Option<Active>>,
    test_root: Option<PathBuf>,
    test_models: Option<(String, Vec<String>)>,
    test_transcribe: Option<TranscribeFn>,
    test_pin: Option<Arc<dyn Fn(&str) + Send + Sync>>,
    test_load: Option<Arc<dyn Fn(i64, &str) + Send + Sync>>,
    test_clear: Option<Arc<dyn Fn() + Send + Sync>>,
    #[cfg(all(test, target_os = "macos"))]
    test_tap_start:
        Option<Arc<dyn Fn() -> Result<(), crate::meetings_tap::TapError> + Send + Sync>>,
    #[cfg(test)]
    test_mic_armed: Option<Arc<std::sync::atomic::AtomicUsize>>,
}

impl MeetingSession {
    pub fn new(app: AppHandle) -> Self {
        Self {
            app: Some(app),
            active: Mutex::new(None),
            test_root: None,
            test_models: None,
            test_transcribe: None,
            test_pin: None,
            test_load: None,
            test_clear: None,
            #[cfg(all(test, target_os = "macos"))]
            test_tap_start: None,
            #[cfg(test)]
            test_mic_armed: None,
        }
    }

    #[cfg(test)]
    fn for_test(
        root: PathBuf,
        selected: &str,
        downloaded: Vec<String>,
        transcribe: TranscribeFn,
        pin: Arc<dyn Fn(&str) + Send + Sync>,
        clear: Arc<dyn Fn() + Send + Sync>,
    ) -> Self {
        Self {
            app: None,
            active: Mutex::new(None),
            test_root: Some(root),
            test_models: Some((selected.to_string(), downloaded)),
            test_transcribe: Some(transcribe),
            test_pin: Some(pin),
            test_load: None,
            test_clear: Some(clear),
            #[cfg(target_os = "macos")]
            test_tap_start: None,
            test_mic_armed: None,
        }
    }

    #[cfg(test)]
    fn with_test_load(mut self, load: Arc<dyn Fn(i64, &str) + Send + Sync>) -> Self {
        self.test_load = Some(load);
        self
    }

    #[cfg(all(test, target_os = "macos"))]
    fn with_fake_tap(
        mut self,
        tap: Arc<dyn Fn() -> Result<(), crate::meetings_tap::TapError> + Send + Sync>,
    ) -> Self {
        self.test_tap_start = Some(tap);
        self
    }

    #[cfg(test)]
    fn with_mic_arm_counter(mut self, counter: Arc<std::sync::atomic::AtomicUsize>) -> Self {
        self.test_mic_armed = Some(counter);
        self
    }

    /// `tap_ok` false is a denied System Audio Recording permission: no row, no file.
    #[cfg(test)]
    pub fn start_for_test(&self, tap_ok: bool) -> Result<i64, MeetingRefusal> {
        self.begin(tap_ok, false).map(|started| started.id)
    }

    #[cfg(test)]
    pub fn push_me(&self, samples: &[f32]) {
        if let Some(active) = self.active.lock().unwrap_or_else(poison).as_ref() {
            if !write_optional(&active.feed.me_wav, &active.feed.write_failed, samples) {
                return;
            }
            active.feed.push_chunker(&active.feed.me, samples);
        }
    }

    #[cfg(test)]
    pub fn push_others(&self, samples: &[f32]) {
        if let Some(active) = self.active.lock().unwrap_or_else(poison).as_ref() {
            active.feed.push_others(samples);
        }
    }

    #[cfg(test)]
    pub fn set_heard(&self, heard: bool) {
        if let Some(active) = self.active.lock().unwrap_or_else(poison).as_ref() {
            active.feed.heard.store(heard, Ordering::Relaxed);
        }
    }

    #[cfg(test)]
    pub fn end(&self, reason: MeetingEnd) -> Vec<MeetingWarning> {
        let id = self
            .active
            .lock()
            .unwrap_or_else(poison)
            .as_ref()
            .map(|active| active.id);
        match id {
            Some(id) => self.stop_id(id, reason),
            None => Vec::new(),
        }
    }

    /// Drop the tracks without finalising, the way a crash leaves the row.
    #[cfg(test)]
    pub fn abandon(&self) {
        self.clear_runtime();
        self.active.lock().unwrap_or_else(poison).take();
    }

    fn begin(&self, tap_ok: bool, mic_only: bool) -> Result<StartedMeeting, MeetingRefusal> {
        let model = self.model_plan()?;
        let live = self.app.is_some() && self.test_root.is_none();
        if live && !mic_only {
            #[cfg(not(target_os = "macos"))]
            {
                let _ = (model, tap_ok);
                return Err(MeetingRefusal::SystemAudioUnavailable);
            }
        }
        self.pin_model(&model);
        match self.open_tracks(&model, tap_ok, live, mic_only) {
            Ok(started) => Ok(started),
            Err(err) => {
                self.clear_runtime();
                Err(err)
            }
        }
    }

    fn hold_tap(&self) -> Result<Option<TapHold>, MeetingRefusal> {
        #[cfg(not(target_os = "macos"))]
        {
            Err(MeetingRefusal::SystemAudioUnavailable)
        }
        #[cfg(target_os = "macos")]
        {
            let plist = bundle_plist()?;
            let gate = Arc::new(Mutex::new(TapBuf {
                pending: Vec::new(),
                feed: None,
            }));
            let sink_gate = Arc::clone(&gate);
            let sink: crate::meetings_tap::TapSink = Box::new(move |samples| {
                let mut buf = sink_gate.lock().unwrap_or_else(poison);
                if let Some(feed) = buf.feed.clone() {
                    drop(buf);
                    feed.push_others(samples);
                } else {
                    buf.pending.extend_from_slice(samples);
                }
            });
            match crate::meetings_tap::SystemAudioSession::start(&plist, sink) {
                Ok(tap) => Ok(Some(TapHold { gate, tap })),
                Err(err) => Err(map_tap(err)),
            }
        }
    }

    fn model_plan(&self) -> Result<String, MeetingRefusal> {
        if let Some((selected, downloaded)) = &self.test_models {
            return plan_meeting_model(selected, downloaded, &|_| None).map_err(|err| match err {
                MeetingStartRefusal::OnlyVoxtral { model_id } => {
                    MeetingRefusal::NoLongAudioModel { model_id }
                }
            });
        }
        let app = self
            .app
            .as_ref()
            .ok_or(MeetingRefusal::SystemAudioUnavailable)?;
        let selected = crate::settings::get_settings(app).selected_model;
        let mm = app.try_state::<Arc<ModelManager>>();
        let downloaded: Vec<String> = mm
            .as_ref()
            .map(|models| {
                models
                    .get_available_models()
                    .into_iter()
                    .filter(|model| model.is_downloaded)
                    .map(|model| model.id)
                    .collect()
            })
            .unwrap_or_default();
        plan_meeting_model(&selected, &downloaded, &|id| {
            mm.as_ref()
                .and_then(|models| models.get_model_info(id))
                .map(|model| model.engine_type)
        })
        .map_err(|err| match err {
            MeetingStartRefusal::OnlyVoxtral { model_id } => {
                MeetingRefusal::NoLongAudioModel { model_id }
            }
        })
    }

    fn pin_model(&self, model_id: &str) {
        if let Some(pin) = &self.test_pin {
            pin(model_id);
        }
        if let Some(app) = &self.app {
            if let Some(tm) = app.try_state::<Arc<TranscriptionManager>>() {
                tm.pin_meeting_model(model_id);
            }
        }
        crate::managers::meetings::set_meeting_active(true);
        crate::meetings::set_meeting_recording(true);
    }

    /// Kick off the meeting model load once the row, tap, WAVs, and mic are ready (M-c1).
    fn load_meeting_model(&self, meeting_id: i64, model_id: &str) {
        if let Some(load) = &self.test_load {
            load(meeting_id, model_id);
        }
        if let Some(app) = &self.app {
            if let Some(tm) = app.try_state::<Arc<TranscriptionManager>>() {
                tm.start_meeting_model_load(meeting_id, model_id);
            }
        }
    }

    fn clear_runtime(&self) {
        crate::managers::meetings::set_meeting_active(false);
        crate::meetings::set_meeting_recording(false);
        if let Some(clear) = &self.test_clear {
            clear();
        }
        if let Some(app) = &self.app {
            if let Some(tm) = app.try_state::<Arc<TranscriptionManager>>() {
                tm.clear_meeting_pin();
            }
        }
    }

    fn root(&self) -> Result<PathBuf, MeetingRefusal> {
        if let Some(root) = &self.test_root {
            return Ok(root.clone());
        }
        let app = self
            .app
            .as_ref()
            .ok_or(MeetingRefusal::SystemAudioUnavailable)?;
        crate::portable::app_data_dir(app).map_err(refuse_io)
    }

    fn stop_id(&self, id: i64, reason: MeetingEnd) -> Vec<MeetingWarning> {
        let active = {
            let mut slot = self.active.lock().unwrap_or_else(poison);
            match slot.as_ref() {
                Some(active) if active.id == id => slot.take(),
                _ => return Vec::new(),
            }
        };
        let Some(active) = active else {
            return Vec::new();
        };
        let warnings = finish_active(active, reason);
        self.clear_runtime();
        warnings
    }

    fn poll_id(&self, id: i64, elapsed_secs: u64) -> SessionPoll {
        let slot = self.active.lock().unwrap_or_else(poison);
        let Some(active) = slot.as_ref() else {
            return SessionPoll::default();
        };
        if active.id != id {
            return SessionPoll::default();
        }
        let mut warnings = active.feed.warnings.lock().unwrap_or_else(poison).clone();
        if active.mic_only {
            warnings.retain(|warning| *warning != MeetingWarning::SystemAudioSilent);
        } else {
            let heard = heard_audio(active);
            if heard {
                warnings.retain(|warning| *warning != MeetingWarning::SystemAudioSilent);
            } else if elapsed_secs >= SILENT_AFTER_SECS
                && !warnings.contains(&MeetingWarning::SystemAudioSilent)
            {
                warnings.push(MeetingWarning::SystemAudioSilent);
            }
        }
        *active.feed.warnings.lock().unwrap_or_else(poison) = warnings.clone();
        let write_failed = active.feed.write_failed.load(Ordering::Relaxed)
            || active.mic.as_ref().is_some_and(|mic| mic.write_failed());
        let mic_name = active.feed.mic_name.lock().unwrap_or_else(poison).clone();
        SessionPoll {
            mic_name,
            warnings,
            write_failed,
        }
    }
}

impl MeetingRecorders for MeetingSession {
    fn start(&self) -> Result<StartedMeeting, MeetingRefusal> {
        self.begin(true, false)
    }

    fn start_mic_only(&self) -> Result<StartedMeeting, MeetingRefusal> {
        self.begin(true, true)
    }

    fn stop(&self, meeting_id: i64, reason: MeetingEnd) -> Vec<MeetingWarning> {
        self.stop_id(meeting_id, reason)
    }

    fn poll(&self, meeting_id: i64, elapsed_secs: u64) -> SessionPoll {
        self.poll_id(meeting_id, elapsed_secs)
    }
}

#[cfg(target_os = "macos")]
impl TapHold {
    fn arm(&self, feed: &Arc<Feed>) {
        let pending = {
            let mut buf = self.gate.lock().unwrap_or_else(poison);
            buf.feed = Some(Arc::clone(feed));
            std::mem::take(&mut buf.pending)
        };
        if !pending.is_empty() {
            feed.push_others(&pending);
        }
    }
}

fn release_hold(hold: Option<TapHold>) {
    #[cfg(target_os = "macos")]
    if let Some(hold) = hold {
        let _ = hold.tap.stop();
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = hold;
    }
}

fn rollback(store: &MeetingsStore, id: i64, dir: &Path) {
    match store.delete_meeting(id) {
        Ok(DeleteOutcome::Deleted) => {}
        Ok(DeleteOutcome::DeletedAudioLeft { dir: left, error }) => {
            log::warn!("meeting {id} rollback left {}: {error}", left.display());
        }
        Err(err) => {
            log::warn!("meeting {id} rollback failed: {err}");
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}

fn heard_audio(active: &Active) -> bool {
    #[cfg(target_os = "macos")]
    if let Some(tap) = &active.tap {
        if tap.has_heard_audio() {
            return true;
        }
    }
    active.feed.heard.load(Ordering::Relaxed)
}

fn finish_active(mut active: Active, reason: MeetingEnd) -> Vec<MeetingWarning> {
    if let Some(mic) = active.mic.take() {
        mic.stop();
    }
    let mut warnings = active.feed.warnings.lock().unwrap_or_else(poison).clone();
    #[cfg(target_os = "macos")]
    if let Some(tap) = active.tap.take() {
        match tap.stop() {
            Ok(()) => {}
            Err(crate::meetings_tap::TapError::Silent) => {
                if !warnings.contains(&MeetingWarning::SystemAudioSilent) {
                    warnings.push(MeetingWarning::SystemAudioSilent);
                }
            }
            Err(err) => log::warn!("meeting tap stop: {}", err.code()),
        }
    }
    if !active.mic_only && active.fake && !active.feed.heard.load(Ordering::Relaxed) {
        if !warnings.contains(&MeetingWarning::SystemAudioSilent) {
            warnings.push(MeetingWarning::SystemAudioSilent);
        }
    }
    active.feed.finish_chunkers();
    active.feed.tx.lock().unwrap_or_else(poison).take();
    if let Some(worker) = active.worker.take() {
        let _ = worker.join();
    }
    flush_one(&active.feed.system_wav, &active.feed.write_failed);
    if let Some(wav) = active.feed.me_wav.lock().unwrap_or_else(poison).as_mut() {
        if let Err(err) = wav.flush() {
            active.feed.write_failed.store(true, Ordering::Relaxed);
            log::error!("meeting wav flush failed: {err}");
        }
    }
    if active.feed.write_failed.load(Ordering::Relaxed) {
        log::error!(
            "meeting {} write failed; finalising with audio already on disk",
            active.id
        );
    }
    if let Err(err) = active
        .store
        .finalise(active.id, matches!(reason, MeetingEnd::Slept))
    {
        log::error!("meeting {} finalise failed", active.id);
        let _ = err;
    }
    if warnings.contains(&MeetingWarning::SystemAudioSilent) {
        if let Err(err) = active.store.mark_system_audio_silent(active.id) {
            log::error!("meeting {} mark_system_audio_silent failed", active.id);
            let _ = err;
        }
    }
    warnings
}

fn flush_one(wav: &Mutex<AppendWav>, failed: &AtomicBool) {
    if let Err(err) = wav.lock().unwrap_or_else(poison).flush() {
        failed.store(true, Ordering::Relaxed);
        log::error!("meeting wav flush failed: {err}");
    }
}

fn arm_mic(app: &AppHandle, meeting_id: i64, feed: Arc<Feed>) -> anyhow::Result<MeetingMic> {
    let dir = crate::portable::app_data_dir(app)?
        .join("meetings")
        .join(meeting_id.to_string());
    let selected = crate::settings::get_settings(app).selected_microphone;
    let mic = MeetingMic::from_wav(dir.join("mic.wav"), selected, std::time::Instant::now())?;
    let hooked = Arc::clone(&feed);
    mic.set_on_samples(Arc::new(move |samples| {
        hooked.push_chunker(&hooked.me, samples)
    }));
    mic.arm();
    Ok(mic)
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn usage_string_missing_reason(plist_path: &std::path::Path, readable: bool) -> String {
    let reason = if readable {
        "no usage key in Contents/Info.plist"
    } else {
        "Info.plist not readable"
    };
    format!(
        "system_audio_usage_string_missing: {reason} ({})",
        plist_path.display()
    )
}

#[cfg(target_os = "macos")]
fn bundle_plist_path() -> std::path::PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|exe| {
            exe.parent()
                .and_then(|dir| dir.parent())
                .map(|contents| contents.join("Info.plist"))
        })
        .unwrap_or_else(|| std::path::PathBuf::from("Contents/Info.plist"))
}

#[cfg(target_os = "macos")]
fn log_usage_string_missing(plist_path: &std::path::Path, readable: bool) -> MeetingRefusal {
    log::error!("{}", usage_string_missing_reason(plist_path, readable));
    MeetingRefusal::SystemAudioUsageStringMissing
}

#[cfg(target_os = "macos")]
fn bundle_plist() -> Result<String, MeetingRefusal> {
    let plist = bundle_plist_path();
    std::fs::read_to_string(&plist).map_err(|_| log_usage_string_missing(&plist, false))
}

#[cfg(target_os = "macos")]
fn map_tap(err: crate::meetings_tap::TapError) -> MeetingRefusal {
    match err {
        crate::meetings_tap::TapError::UsageStringMissing => {
            let plist = bundle_plist_path();
            log_usage_string_missing(&plist, true)
        }
        crate::meetings_tap::TapError::PermissionDenied => {
            MeetingRefusal::SystemAudioPermissionDenied
        }
        crate::meetings_tap::TapError::Silent => MeetingRefusal::SystemAudioUnavailable,
        crate::meetings_tap::TapError::Device(message) => {
            log::warn!("meeting system audio failed to start: {message}");
            MeetingRefusal::SystemAudioUnavailable
        }
        crate::meetings_tap::TapError::CoreAudio { cause, call, code } => {
            let mapped = match cause {
                crate::meetings_tap::TapFailureCause::Permission => {
                    crate::meeting_controller::SystemAudioFailureCause::Permission
                }
                crate::meetings_tap::TapFailureCause::Device => {
                    crate::meeting_controller::SystemAudioFailureCause::Device
                }
            };
            log::warn!(
                "meeting system audio failed to start: call={call} cause={mapped:?} code={code}"
            );
            MeetingRefusal::SystemAudioTapFailed {
                cause: mapped,
                call: call.to_string(),
                code,
            }
        }
    }
}

#[cfg(not(target_os = "macos"))]
struct TapHold;

impl MeetingSession {
    fn open_tracks(
        &self,
        model: &str,
        tap_ok: bool,
        live: bool,
        mic_only: bool,
    ) -> Result<StartedMeeting, MeetingRefusal> {
        if !mic_only && !tap_ok {
            return Err(MeetingRefusal::SystemAudioPermissionDenied);
        }
        let me_chunker = chunker(self.app.as_ref(), CHANNEL_ME, !live).map_err(|err| {
            log::warn!("meeting silero model failed to load: {err}");
            MeetingRefusal::SystemAudioUnavailable
        })?;
        let others_chunker = chunker(self.app.as_ref(), CHANNEL_OTHERS, !live).map_err(|err| {
            log::warn!("meeting silero model failed to load: {err}");
            MeetingRefusal::SystemAudioUnavailable
        })?;
        let hold = if mic_only {
            None
        } else if live {
            self.hold_tap()?
        } else {
            #[cfg(all(test, target_os = "macos"))]
            if let Some(fake_tap) = &self.test_tap_start {
                fake_tap().map_err(map_tap)?;
            }
            None
        };
        let root = self.root()?;
        let store = MeetingsStore::open(&root).map_err(refuse_io)?;
        let id = match store.start_meeting("Meeting", model) {
            Ok(id) => id,
            Err(err) => {
                release_hold(hold);
                return Err(refuse_io(err));
            }
        };
        let dir = root.join("meetings").join(id.to_string());
        if let Err(err) = std::fs::create_dir_all(&dir) {
            rollback(&store, id, &dir);
            release_hold(hold);
            return Err(refuse_io(err));
        }
        let system = match AppendWav::create(&dir.join("system.wav")) {
            Ok(wav) => wav,
            Err(err) => {
                rollback(&store, id, &dir);
                release_hold(hold);
                return Err(refuse_io(err));
            }
        };
        let me_wav = if live {
            None
        } else {
            match AppendWav::create(&dir.join("mic.wav")) {
                Ok(wav) => Some(wav),
                Err(err) => {
                    rollback(&store, id, &dir);
                    release_hold(hold);
                    return Err(refuse_io(err));
                }
            }
        };
        let (tx, rx) = mpsc::channel();
        let feed = Arc::new(Feed {
            system_wav: Mutex::new(system),
            me_wav: Mutex::new(me_wav),
            me: Mutex::new(me_chunker),
            others: Mutex::new(others_chunker),
            tx: Mutex::new(Some(tx)),
            write_failed: AtomicBool::new(false),
            heard: AtomicBool::new(false),
            warnings: Mutex::new(Vec::new()),
            mic_name: Mutex::new(None),
        });
        #[cfg(target_os = "macos")]
        if let Some(hold) = hold.as_ref() {
            hold.arm(&feed);
        }
        let mic = if live {
            match self
                .app
                .as_ref()
                .map(|app| arm_mic(app, id, Arc::clone(&feed)))
            {
                Some(Ok(mic)) => {
                    if mic.fell_back() {
                        push_warning(&feed, MeetingWarning::MicFellBackToDefault);
                    }
                    *feed.mic_name.lock().unwrap_or_else(poison) = mic.mic_name();
                    Some(mic)
                }
                Some(Err(err)) => {
                    rollback(&store, id, &dir);
                    release_hold(hold);
                    return Err(refuse_io(err));
                }
                None => None,
            }
        } else {
            #[cfg(test)]
            if let Some(counter) = &self.test_mic_armed {
                counter.fetch_add(1, Ordering::SeqCst);
            }
            None
        };
        // Tracks, row, WAVs, and mic are all open; nothing past this point can refuse (M-c1, F1).
        self.load_meeting_model(id, model);
        let worker = spawn_worker(
            store.clone(),
            id,
            rx,
            self.test_transcribe.clone(),
            self.app.clone(),
        );
        let started = StartedMeeting {
            id,
            mic_name: feed.mic_name.lock().unwrap_or_else(poison).clone(),
            warnings: feed.warnings.lock().unwrap_or_else(poison).clone(),
            mic_only,
        };
        *self.active.lock().unwrap_or_else(poison) = Some(Active {
            id,
            feed,
            worker: Some(worker),
            mic,
            #[cfg(target_os = "macos")]
            tap: hold.map(|hold| hold.tap),
            mic_only,
            fake: !live,
            store,
        });
        Ok(started)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::audio_toolkit::read_wav_samples;
    use crate::managers::meetings::{test_lock, MeetingsStore};
    use crate::managers::transcription::TranscriptionManager;
    use crate::meeting_controller::{MeetingRefusal, MeetingWarning};

    const PARAKEET: &str = "handy-computer/parakeet-tdt-0.6b-v3-gguf";

    struct Harness {
        session: MeetingSession,
        dir: tempfile::TempDir,
        _app: tauri::App<tauri::test::MockRuntime>,
        _lock: std::sync::MutexGuard<'static, ()>,
    }

    fn harness() -> Harness {
        let _lock = test_lock();
        let dir = tempfile::tempdir().unwrap();
        let app = tauri::test::mock_builder()
            .plugin(tauri_plugin_store::Builder::default().build())
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock app");
        let tm = Arc::new(TranscriptionManager::new_for_test(app.handle().clone()));
        tm.install_fake_engine(PARAKEET);
        let transcribe_tm = Arc::clone(&tm);
        let pin_tm = Arc::clone(&tm);
        let clear_tm = Arc::clone(&tm);
        let session = MeetingSession::for_test(
            dir.path().to_path_buf(),
            PARAKEET,
            vec![PARAKEET.to_string()],
            Arc::new(move |audio| {
                transcribe_tm
                    .transcribe_meeting_chunk(audio)
                    .map_err(|err| err.to_string())
            }),
            Arc::new(move |model| pin_tm.pin_meeting_model(model)),
            Arc::new(move || clear_tm.clear_meeting_pin()),
        );
        Harness {
            session,
            dir,
            _app: app,
            _lock,
        }
    }

    fn track(dir: &std::path::Path, id: i64, name: &str) -> std::path::PathBuf {
        dir.join("meetings").join(id.to_string()).join(name)
    }

    #[test]
    fn sleep_finalises_meeting() {
        let harness = harness();
        let id = harness.session.start_for_test(true).unwrap();
        harness.session.push_me(&[0.0; 480]);
        harness.session.push_others(&[0.0; 480]);
        harness
            .session
            .end(crate::meeting_controller::MeetingEnd::Slept);
        assert!(!crate::meetings::meeting_is_recording());
        assert!(!set_meeting_active_now());
        let store = MeetingsStore::open(harness.dir.path()).unwrap();
        let row = store.meeting(id).unwrap();
        assert!(row.ended_at.is_some(), "sleep left ended_at null");
        assert_eq!(row.status, "transcript_ready");
        assert!(row.title.contains("Mac slept"), "title was {}", row.title);
        assert!(track(harness.dir.path(), id, "mic.wav").exists());
        assert!(track(harness.dir.path(), id, "system.wav").exists());
        let mic_len = std::fs::metadata(track(harness.dir.path(), id, "mic.wav"))
            .unwrap()
            .len();
        harness.session.push_me(&[0.0; 480]);
        assert_eq!(
            std::fs::metadata(track(harness.dir.path(), id, "mic.wav"))
                .unwrap()
                .len(),
            mic_len,
            "a stopped track kept accepting samples"
        );
    }

    fn set_meeting_active_now() -> bool {
        crate::managers::meetings::meeting_is_active()
    }

    #[test]
    fn stop_flushes_and_unfinished_row_recovers() {
        let harness = harness();
        let samples = vec![0.0_f32; 960];
        let id = harness.session.start_for_test(true).unwrap();
        harness.session.push_me(&samples);
        harness.session.push_others(&samples);
        harness
            .session
            .end(crate::meeting_controller::MeetingEnd::Stopped);
        let store = MeetingsStore::open(harness.dir.path()).unwrap();
        let row = store.meeting(id).unwrap();
        assert!(row.ended_at.is_some());
        assert_eq!(row.status, "transcript_ready");
        let me = read_wav_samples(&track(harness.dir.path(), id, "mic.wav")).unwrap();
        let system = read_wav_samples(&track(harness.dir.path(), id, "system.wav")).unwrap();
        assert_eq!(me.len(), samples.len());
        assert_eq!(system.len(), samples.len());

        let id2 = harness.session.start_for_test(true).unwrap();
        harness.session.push_me(&samples);
        harness.session.push_others(&samples);
        let mic = track(harness.dir.path(), id2, "mic.wav");
        let system_path = track(harness.dir.path(), id2, "system.wav");
        let mic_bytes = std::fs::read(&mic).unwrap();
        let system_bytes = std::fs::read(&system_path).unwrap();
        harness.session.abandon();
        let store = MeetingsStore::open(harness.dir.path()).unwrap();
        assert_eq!(store.meeting(id2).unwrap().status, "recording");
        assert_eq!(store.recover_unfinished().unwrap(), 1);
        let recovered = store.meeting(id2).unwrap();
        assert_eq!(recovered.status, "transcript_ready");
        assert!(recovered.ended_at.is_some());
        assert_eq!(std::fs::read(&mic).unwrap(), mic_bytes);
        assert_eq!(std::fs::read(&system_path).unwrap(), system_bytes);
    }

    #[test]
    fn tap_denied_leaves_no_row_or_file() {
        let harness = harness();
        let err = harness.session.start_for_test(false).unwrap_err();
        assert_eq!(err, MeetingRefusal::SystemAudioPermissionDenied);
        assert!(!harness.dir.path().join("meetings").exists());
        assert!(!crate::managers::meetings::meeting_is_active());
    }

    #[test]
    fn silero_unavailable_in_live_start_leaves_no_row_or_file() {
        let harness = harness();
        let err = harness
            .session
            .open_tracks(PARAKEET, true, true, false)
            .err()
            .expect("live start without Silero must fail");
        assert_eq!(err, MeetingRefusal::SystemAudioUnavailable);
        assert!(!harness.dir.path().join("meetings").exists());
    }

    #[test]
    fn silent_tap_raises_warning() {
        let harness = harness();
        let id = harness.session.start_for_test(true).unwrap();
        let early = harness.session.poll_id(id, 10);
        assert!(early.warnings.contains(&MeetingWarning::SystemAudioSilent));
        harness.session.set_heard(true);
        let heard = harness.session.poll_id(id, 11);
        assert!(!heard.warnings.contains(&MeetingWarning::SystemAudioSilent));
        harness.session.set_heard(false);
        let warnings = harness
            .session
            .end(crate::meeting_controller::MeetingEnd::Stopped);
        assert!(warnings.contains(&MeetingWarning::SystemAudioSilent));
        let store = MeetingsStore::open(harness.dir.path()).unwrap();
        let row = store.meeting(id).unwrap();
        assert_eq!(row.status, "transcript_ready");
        assert!(row.ended_at.is_some());
    }

    #[test]
    fn start_notes_records_both_tracks_feeds_live_segments_and_stop_saves_meeting() {
        let _lock = test_lock();
        let dir = tempfile::tempdir().unwrap();
        let app = tauri::test::mock_builder()
            .plugin(tauri_plugin_store::Builder::default().build())
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock app");
        let tm = Arc::new(TranscriptionManager::new_for_test(app.handle().clone()));
        tm.install_fake_engine(PARAKEET);
        let calls = Arc::new(Mutex::new(0usize));
        let calls_ref = Arc::clone(&calls);
        let pin_tm = Arc::clone(&tm);
        let clear_tm = Arc::clone(&tm);
        let session = Arc::new(MeetingSession::for_test(
            dir.path().to_path_buf(),
            PARAKEET,
            vec![PARAKEET.to_string()],
            Arc::new(move |_audio| {
                let mut n = calls_ref.lock().unwrap();
                *n += 1;
                Ok(format!("segment-{}", *n))
            }),
            Arc::new(move |model| pin_tm.pin_meeting_model(model)),
            Arc::new(move || clear_tm.clear_meeting_pin()),
        ));

        struct ArcRecorders(Arc<MeetingSession>);
        impl MeetingRecorders for ArcRecorders {
            fn start(&self) -> Result<StartedMeeting, MeetingRefusal> {
                self.0.start()
            }
            fn stop(&self, id: i64, reason: MeetingEnd) -> Vec<MeetingWarning> {
                self.0.stop(id, reason)
            }
            fn poll(&self, id: i64, elapsed: u64) -> SessionPoll {
                self.0.poll(id, elapsed)
            }
        }

        let controller = crate::meeting_controller::MeetingController::new(Box::new(ArcRecorders(
            Arc::clone(&session),
        )));
        assert_eq!(
            controller.open_consent().state,
            crate::meeting_controller::MeetingState::ConsentOpen
        );
        let started = controller.confirm_start().unwrap();
        assert_eq!(
            started.state,
            crate::meeting_controller::MeetingState::Recording
        );
        let id = started.meeting_id.expect("numeric meeting id");

        // Speech burst (amplitude 0.25 > energy threshold) followed by silence on both Me & Others
        let mut speech = vec![0.25_f32; 480 * 12];
        speech.extend(std::iter::repeat_n(0.0_f32, 480 * 20));
        session.push_me(&speech);
        session.push_others(&speech);

        let stopped = controller.stop();
        assert_eq!(stopped.state, crate::meeting_controller::MeetingState::Idle);
        assert_eq!(stopped.meeting_id, Some(id));

        let store = MeetingsStore::open(dir.path()).unwrap();
        let row = store.meeting(id).unwrap();
        assert_eq!(row.status, "transcript_ready");
        assert!(row.ended_at.is_some());

        let me_wav = read_wav_samples(&track(dir.path(), id, "mic.wav")).unwrap();
        let sys_wav = read_wav_samples(&track(dir.path(), id, "system.wav")).unwrap();
        assert_eq!(me_wav.len(), speech.len());
        assert_eq!(sys_wav.len(), speech.len());

        let segs = store.segments(id).unwrap();
        assert_eq!(
            segs.len(),
            2,
            "expected 1 Me + 1 Others segment, got {segs:?}"
        );
        let channels: Vec<&str> = segs.iter().map(|s| s.channel.as_str()).collect();
        assert!(channels.contains(&"me") && channels.contains(&"others"));
        assert!(segs.iter().all(|s| s.text.starts_with("segment-")));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn r3_step4_fake_tap_bad_obj_and_hog_reach_confirm_start_as_named_refusal() {
        use crate::meeting_controller::{MeetingState, SystemAudioFailureCause};
        use crate::meetings_tap::map_os_call;
        use cidre::core_audio::hardware_err;
        use cidre::os;

        struct ArcRecorders(Arc<MeetingSession>);
        impl MeetingRecorders for ArcRecorders {
            fn start(&self) -> Result<StartedMeeting, MeetingRefusal> {
                self.0.start()
            }
            fn stop(&self, id: i64, reason: MeetingEnd) -> Vec<MeetingWarning> {
                self.0.stop(id, reason)
            }
            fn poll(&self, id: i64, elapsed: u64) -> SessionPoll {
                self.0.poll(id, elapsed)
            }
        }

        let cases = [
            (
                "create_process_tap",
                hardware_err::BAD_OBJ,
                SystemAudioFailureCause::Device,
                "!obj",
            ),
            (
                "create_process_tap",
                hardware_err::PERMISSIONS,
                SystemAudioFailureCause::Permission,
                "!hog",
            ),
            (
                "device_start",
                os::Error::new(i32::from_be_bytes(*b"!dev")).unwrap(),
                SystemAudioFailureCause::Device,
                "!dev",
            ),
        ];

        for (call_name, os_err, expected_cause, expected_fcc) in cases {
            let mut h = harness();
            h.session = h
                .session
                .with_fake_tap(Arc::new(move || Err(map_os_call(call_name, os_err))));
            let controller = crate::meeting_controller::MeetingController::new(Box::new(
                ArcRecorders(Arc::new(h.session)),
            ));
            controller.open_consent();
            let err = controller
                .confirm_start()
                .expect_err("failing tap must refuse confirm_start");
            match err {
                MeetingRefusal::SystemAudioTapFailed { cause, call, code } => {
                    assert_eq!(cause, expected_cause);
                    assert_eq!(call, call_name);
                    assert!(
                        code.contains(expected_fcc),
                        "expected {expected_fcc} in code {code:?}"
                    );
                }
                other => panic!("expected SystemAudioTapFailed, got {other:?}"),
            }
            assert_eq!(controller.state(), MeetingState::ConsentOpen);
            assert!(!h.dir.path().join("meetings").exists());
            assert!(!crate::managers::meetings::meeting_is_active());
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn r3_mic_only_start_arms_mic_while_failed_normal_tap_refuses_and_arms_nothing() {
        use std::sync::atomic::AtomicUsize;

        use crate::meeting_controller::{MeetingState, SystemAudioFailureCause};
        use crate::meetings_tap::map_os_call;
        use cidre::core_audio::hardware_err;

        struct ArcRecorders(Arc<MeetingSession>);
        impl MeetingRecorders for ArcRecorders {
            fn start(&self) -> Result<StartedMeeting, MeetingRefusal> {
                self.0.start()
            }
            fn start_mic_only(&self) -> Result<StartedMeeting, MeetingRefusal> {
                self.0.start_mic_only()
            }
            fn stop(&self, id: i64, reason: MeetingEnd) -> Vec<MeetingWarning> {
                self.0.stop(id, reason)
            }
            fn poll(&self, id: i64, elapsed: u64) -> SessionPoll {
                self.0.poll(id, elapsed)
            }
        }

        let tap_calls = Arc::new(AtomicUsize::new(0));
        let tap_calls_cb = Arc::clone(&tap_calls);
        let mic_arms = Arc::new(AtomicUsize::new(0));

        let mut h = harness();
        h.session = h
            .session
            .with_fake_tap(Arc::new(move || {
                tap_calls_cb.fetch_add(1, Ordering::SeqCst);
                Err(map_os_call("create_process_tap", hardware_err::BAD_OBJ))
            }))
            .with_mic_arm_counter(Arc::clone(&mic_arms));
        let session = Arc::new(h.session);
        let controller = crate::meeting_controller::MeetingController::new(Box::new(ArcRecorders(
            Arc::clone(&session),
        )));

        controller.open_consent();
        // 1. Normal start with a failing tap returns the named refusal and arms nothing.
        let err = controller
            .confirm_start()
            .expect_err("normal start must refuse on failing tap");
        assert!(matches!(
            err,
            MeetingRefusal::SystemAudioTapFailed {
                cause: SystemAudioFailureCause::Device,
                ref call,
                ref code,
            } if call == "create_process_tap" && code.contains("!obj")
        ));
        assert_eq!(tap_calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            mic_arms.load(Ordering::SeqCst),
            0,
            "failed normal start must not arm mic"
        );
        assert_eq!(controller.state(), MeetingState::ConsentOpen);
        assert!(!h.dir.path().join("meetings").exists());

        // 2. Mic-only start skips the tap, arms the mic, and carries mic_only: true.
        let started = controller
            .confirm_start_mic_only()
            .expect("mic-only start must succeed even when system-audio tap would fail");
        assert_eq!(started.state, MeetingState::Recording);
        assert!(started.mic_only, "status must carry mic_only: true");
        assert_eq!(
            tap_calls.load(Ordering::SeqCst),
            1,
            "mic-only start must skip the system-audio tap"
        );
        assert_eq!(
            mic_arms.load(Ordering::SeqCst),
            1,
            "mic-only start must arm the mic"
        );
        let id = started.meeting_id.expect("meeting row id");
        assert!(
            !session
                .poll_id(id, 15)
                .warnings
                .contains(&MeetingWarning::SystemAudioSilent),
            "mic-only meeting must not raise SystemAudioSilent"
        );
        session.push_me(&[0.0; 480]);
        let stopped = controller.stop();
        assert_eq!(stopped.state, MeetingState::Idle);
        assert!(stopped.mic_only);
        assert!(track(h.dir.path(), id, "mic.wav").exists());
    }

    #[test]
    fn ac_b19n_stop_persists_system_audio_silent_and_list_from_store_exposes_it() {
        // 1. Silent system audio + Stop -> system_audio_silent == true in list_from_store and JSON.
        {
            let h1 = harness();
            let id1 = h1.session.start_for_test(true).unwrap();
            let w1 = h1
                .session
                .end(crate::meeting_controller::MeetingEnd::Stopped);
            assert!(w1.contains(&MeetingWarning::SystemAudioSilent));
            let store1 = MeetingsStore::open(h1.dir.path()).unwrap();
            assert!(store1.system_audio_silent(id1).unwrap());
            let items1 = crate::meetings_list::list_from_store(&store1).unwrap();
            let item1 = items1
                .iter()
                .find(|it| it.id == id1)
                .expect("saved meeting");
            assert!(item1.system_audio_silent);
            let val1 = serde_json::to_value(item1).unwrap();
            assert_eq!(
                val1["system_audio_silent"],
                serde_json::Value::Bool(true),
                "MeetingListItem JSON must expose system_audio_silent: true after silent Stop"
            );
            println!(
                "AC_B19N_REAL_STOP_ITEM_JSON={}",
                serde_json::to_string(item1).unwrap()
            );
        }

        // 2. Heard system audio + Stop -> system_audio_silent == false.
        {
            let h2 = harness();
            let id2 = h2.session.start_for_test(true).unwrap();
            h2.session.set_heard(true);
            let w2 = h2
                .session
                .end(crate::meeting_controller::MeetingEnd::Stopped);
            assert!(!w2.contains(&MeetingWarning::SystemAudioSilent));
            let store2 = MeetingsStore::open(h2.dir.path()).unwrap();
            assert!(!store2.system_audio_silent(id2).unwrap());
            let items2 = crate::meetings_list::list_from_store(&store2).unwrap();
            let item2 = items2
                .iter()
                .find(|it| it.id == id2)
                .expect("saved meeting");
            assert!(!item2.system_audio_silent);
            assert_eq!(
                serde_json::to_value(item2).unwrap()["system_audio_silent"],
                serde_json::Value::Bool(false)
            );
        }

        // 3. Silent system audio + Slept -> system_audio_silent == true.
        {
            let h3 = harness();
            let id3 = h3.session.start_for_test(true).unwrap();
            let w3 = h3.session.end(crate::meeting_controller::MeetingEnd::Slept);
            assert!(w3.contains(&MeetingWarning::SystemAudioSilent));
            let store3 = MeetingsStore::open(h3.dir.path()).unwrap();
            assert!(store3.system_audio_silent(id3).unwrap());
            let items3 = crate::meetings_list::list_from_store(&store3).unwrap();
            let item3 = items3
                .iter()
                .find(|it| it.id == id3)
                .expect("saved meeting");
            assert!(item3.system_audio_silent);
            assert_eq!(
                serde_json::to_value(item3).unwrap()["system_audio_silent"],
                serde_json::Value::Bool(true)
            );
        }

        // 4. Mic-only start + Stop -> system_audio_silent == false.
        {
            let h4 = harness();
            let started4 = h4.session.start_mic_only().unwrap();
            let id4 = started4.id;
            let w4 = h4
                .session
                .end(crate::meeting_controller::MeetingEnd::Stopped);
            assert!(!w4.contains(&MeetingWarning::SystemAudioSilent));
            let store4 = MeetingsStore::open(h4.dir.path()).unwrap();
            assert!(!store4.system_audio_silent(id4).unwrap());
            let items4 = crate::meetings_list::list_from_store(&store4).unwrap();
            let item4 = items4
                .iter()
                .find(|it| it.id == id4)
                .expect("saved meeting");
            assert!(!item4.system_audio_silent);
            assert_eq!(
                serde_json::to_value(item4).unwrap()["system_audio_silent"],
                serde_json::Value::Bool(false)
            );
        }
    }

    #[test]
    fn t_i1_usage_string_missing_reason_names_code_and_path() {
        let plist = std::path::Path::new("/Applications/Coco Voice.app/Contents/Info.plist");

        let missing_key = usage_string_missing_reason(plist, true);
        assert!(missing_key.contains("system_audio_usage_string_missing"));
        assert!(missing_key.contains("no usage key in Contents/Info.plist"));
        assert!(missing_key.contains("/Applications/Coco Voice.app/Contents/Info.plist"));

        let unreadable = usage_string_missing_reason(plist, false);
        assert!(unreadable.contains("system_audio_usage_string_missing"));
        assert!(unreadable.contains("Info.plist not readable"));
        assert!(unreadable.contains("/Applications/Coco Voice.app/Contents/Info.plist"));
    }

    /// T-mc2 (M-c1, session part, F1): `start_for_test(true)` invokes `test_load`
    /// exactly once with the new meeting id and pinned model, after the row exists
    /// in SQLite, and keeps the meeting started with tracks open.
    #[test]
    fn t_mc2_session_open_tracks_calls_test_load_once_after_id_exists() {
        let _lock = test_lock();
        let dir = tempfile::tempdir().unwrap();
        let app = tauri::test::mock_builder()
            .plugin(tauri_plugin_store::Builder::default().build())
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock app");
        let tm = Arc::new(TranscriptionManager::new_for_test(app.handle().clone()));
        let pin_tm = Arc::clone(&tm);
        let clear_tm = Arc::clone(&tm);
        let transcribe_tm = Arc::clone(&tm);
        let load_calls: Arc<Mutex<Vec<(i64, String, bool)>>> = Arc::new(Mutex::new(Vec::new()));
        let load_calls_ref = Arc::clone(&load_calls);
        let root_for_check = dir.path().to_path_buf();

        let session = MeetingSession::for_test(
            dir.path().to_path_buf(),
            PARAKEET,
            vec![PARAKEET.to_string()],
            Arc::new(move |audio| {
                transcribe_tm
                    .transcribe_meeting_chunk(audio)
                    .map_err(|err| err.to_string())
            }),
            Arc::new(move |model| pin_tm.pin_meeting_model(model)),
            Arc::new(move || clear_tm.clear_meeting_pin()),
        )
        .with_test_load(Arc::new(move |meeting_id, model_id| {
            let row_exists = MeetingsStore::open(&root_for_check)
                .ok()
                .and_then(|store| store.meeting(meeting_id).ok())
                .is_some();
            load_calls_ref
                .lock()
                .unwrap()
                .push((meeting_id, model_id.to_string(), row_exists));
        }));

        let id = session.start_for_test(true).expect("meeting should start");
        let recorded = load_calls.lock().unwrap().clone();
        assert_eq!(recorded, vec![(id, PARAKEET.to_string(), true)]);
        assert!(crate::managers::meetings::meeting_is_active());
        assert!(track(dir.path(), id, "mic.wav").exists());
        assert!(track(dir.path(), id, "system.wav").exists());

        session.end(crate::meeting_controller::MeetingEnd::Stopped);
    }

    /// T-mc2b (M-c1, F1): a refused start (`start_for_test(false)`) makes zero
    /// `test_load` calls and clears the meeting pin.
    #[test]
    fn t_mc2b_refused_start_makes_zero_load_calls_and_clears_pin() {
        let _lock = test_lock();
        let dir = tempfile::tempdir().unwrap();
        let app = tauri::test::mock_builder()
            .plugin(tauri_plugin_store::Builder::default().build())
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock app");
        let tm = Arc::new(TranscriptionManager::new_for_test(app.handle().clone()));
        let pin_tm = Arc::clone(&tm);
        let clear_tm = Arc::clone(&tm);
        let transcribe_tm = Arc::clone(&tm);
        let load_calls: Arc<Mutex<Vec<(i64, String)>>> = Arc::new(Mutex::new(Vec::new()));
        let load_calls_ref = Arc::clone(&load_calls);

        let session = MeetingSession::for_test(
            dir.path().to_path_buf(),
            PARAKEET,
            vec![PARAKEET.to_string()],
            Arc::new(move |audio| {
                transcribe_tm
                    .transcribe_meeting_chunk(audio)
                    .map_err(|err| err.to_string())
            }),
            Arc::new(move |model| pin_tm.pin_meeting_model(model)),
            Arc::new(move || clear_tm.clear_meeting_pin()),
        )
        .with_test_load(Arc::new(move |meeting_id, model_id| {
            load_calls_ref
                .lock()
                .unwrap()
                .push((meeting_id, model_id.to_string()));
        }));

        let err = session
            .start_for_test(false)
            .expect_err("denied tap must refuse start");
        assert_eq!(err, MeetingRefusal::SystemAudioPermissionDenied);
        assert!(
            load_calls.lock().unwrap().is_empty(),
            "refused start must make zero load calls"
        );
        assert!(
            !crate::managers::meetings::meeting_is_active(),
            "refused start must clear meeting active / pin state"
        );
    }

    const LIVE_CANARY_ID: &str =
        "handy-computer/canary-180m-flash-gguf/canary-180m-flash-Q8_0.gguf";
    const LIVE_PARAKEET_Q8_ID: &str = "handy-computer/parakeet-ultra-gguf/parakeet-ultra-Q8_0.gguf";

    /// Resolve a read-only GGUF file in the shared Hugging Face hub cache without
    /// downloading or copying anything.
    fn find_hf_cache_relative(hub_dir: &Path, repo_id: &str, filename: &str) -> String {
        let rel_base = format!("models--{}/snapshots", repo_id.replace('/', "--"));
        let snapshots = hub_dir.join(&rel_base);
        std::fs::read_dir(&snapshots)
            .ok()
            .and_then(|entries| {
                entries.filter_map(|entry| entry.ok()).find_map(|entry| {
                    let candidate = entry.path().join(filename);
                    if candidate.is_file() {
                        let rev = entry.file_name();
                        Some(format!("{rel_base}/{}/{filename}", rev.to_string_lossy()))
                    } else {
                        None
                    }
                })
            })
            .unwrap_or_else(|| {
                panic!(
                    "missing {filename} under {}; download the model into the shared HF cache first",
                    snapshots.display()
                )
            })
    }

    fn seed_hf_catalog_model(
        id: &str,
        name: &str,
        relative_filename: String,
    ) -> crate::managers::model::ModelInfo {
        crate::managers::model::ModelInfo {
            id: id.to_string(),
            name: name.to_string(),
            description: "Read-only HF cache fixture".to_string(),
            filename: relative_filename,
            source: crate::managers::model::ModelSource::Local,
            size_mb: 0,
            is_downloaded: true,
            is_downloading: false,
            partial_size: 0,
            is_directory: false,
            engine_type: crate::managers::model::EngineType::TranscribeCpp,
            accuracy_score: 0.9,
            speed_score: 0.9,
            supports_translation: false,
            is_recommended: false,
            measured_best: None,
            supported_languages: vec!["en".to_string()],
            supports_language_selection: true,
            is_custom: false,
            supports_streaming: false,
            supports_language_detection: true,
        }
    }

    /// T-mc1 shared runner (F2, C2): real `TranscriptionManager` with NO
    /// pre-installed engine, real `ModelManager::for_test_in_dir` seeded read-only
    /// from the shared HF cache, started via `MeetingSession::start_for_test(true)`
    /// with `test_pin`, `test_load`, and `test_transcribe` wired to the real manager.
    fn run_live_meeting_cold_start(target_model_id: &str) {
        let _lock = test_lock();
        let hub_dir = std::env::var("HOME")
            .map(PathBuf::from)
            .expect("HOME is set")
            .join(".cache/huggingface/hub");

        let mut models = std::collections::HashMap::new();
        for (id, name, repo_id, file) in [
            (
                LIVE_CANARY_ID,
                "Canary 180M Flash Q8_0",
                "handy-computer/canary-180m-flash-gguf",
                "canary-180m-flash-Q8_0.gguf",
            ),
            (
                LIVE_PARAKEET_Q8_ID,
                "Parakeet Ultra Q8_0",
                "handy-computer/parakeet-ultra-gguf",
                "parakeet-ultra-Q8_0.gguf",
            ),
        ] {
            let rel = find_hf_cache_relative(&hub_dir, repo_id, file);
            models.insert(id.to_string(), seed_hf_catalog_model(id, name, rel));
        }

        let app = tauri::test::mock_builder()
            .plugin(tauri_plugin_store::Builder::default().build())
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock app");
        let mm = Arc::new(ModelManager::for_test_in_dir(hub_dir, models));
        let tm = Arc::new(TranscriptionManager::new_for_test_with_models(
            app.handle().clone(),
            mm,
        ));
        assert!(
            !tm.is_model_loaded(),
            "T-mc1 must start cold with no engine pre-installed"
        );

        let dir = tempfile::tempdir().expect("create meeting tempdir");
        let fixture_dir = tempfile::tempdir().expect("create say fixture tempdir");
        let aiff = fixture_dir.path().join("fixture.aiff");
        let wav = fixture_dir.path().join("fixture.wav");
        let said = std::process::Command::new("say")
            .arg("-o")
            .arg(&aiff)
            .arg("The quarterly budget review starts on Monday morning")
            .status()
            .expect("run say");
        assert!(said.success(), "say failed: {said}");
        let converted = std::process::Command::new("afconvert")
            .args(["-f", "WAVE", "-d", "LEI16@16000", "-c", "1"])
            .arg(&aiff)
            .arg(&wav)
            .status()
            .expect("run afconvert");
        assert!(converted.success(), "afconvert failed: {converted}");

        let speech = read_wav_samples(&wav).expect("read 16 kHz fixture wav");
        let mut audio = vec![0.0_f32; 16_000];
        audio.extend(speech);
        audio.resize(audio.len() + 32_000, 0.0);

        let chunk_errors: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let chunk_errors_ref = Arc::clone(&chunk_errors);
        let pin_tm = Arc::clone(&tm);
        let load_tm = Arc::clone(&tm);
        let clear_tm = Arc::clone(&tm);
        let transcribe_tm = Arc::clone(&tm);

        let session = MeetingSession::for_test(
            dir.path().to_path_buf(),
            target_model_id,
            vec![LIVE_CANARY_ID.to_string(), LIVE_PARAKEET_Q8_ID.to_string()],
            Arc::new(move |samples| {
                let res = transcribe_tm
                    .transcribe_meeting_chunk(samples)
                    .map_err(|err| err.to_string());
                if let Err(err) = &res {
                    chunk_errors_ref.lock().unwrap().push(err.clone());
                }
                res
            }),
            Arc::new(move |model| pin_tm.pin_meeting_model(model)),
            Arc::new(move || clear_tm.clear_meeting_pin()),
        )
        .with_test_load(Arc::new(move |meeting_id, model_id| {
            load_tm.start_meeting_model_load(meeting_id, model_id);
        }));

        let id = session.start_for_test(true).expect("start cold meeting");
        for slice in audio.chunks(512) {
            session.push_me(slice);
        }
        session.end(crate::meeting_controller::MeetingEnd::Stopped);

        let errs = chunk_errors.lock().unwrap().clone();
        assert!(
            errs.iter().all(|e| !e.contains("Model is not loaded")),
            "cold-start meeting chunk hit unloaded engine: {errs:?}"
        );
        assert!(errs.is_empty(), "cold-start chunk errors: {errs:?}");
        assert_eq!(
            tm.get_current_model().as_deref(),
            Some(target_model_id),
            "current model must equal the pinned catalog id"
        );

        let store = MeetingsStore::open(dir.path()).expect("reopen meetings store");
        let segments = store.segments(id).expect("read stored segments");
        let joined = segments
            .iter()
            .map(|seg| seg.text.trim())
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        assert!(
            !joined.is_empty(),
            "expected non-empty transcribed text from cold-start meeting, got segments={segments:?}"
        );
    }

    #[test]
    #[ignore = "live: needs macOS say/afconvert and cached Canary 180M Flash GGUF"]
    fn live_meeting_cold_start_canary() {
        run_live_meeting_cold_start(LIVE_CANARY_ID);
    }

    #[test]
    #[ignore = "live: needs macOS say/afconvert and cached Parakeet Ultra Q8_0 GGUF"]
    fn live_meeting_cold_start_parakeet_q8() {
        run_live_meeting_cold_start(LIVE_PARAKEET_Q8_ID);
    }
}
