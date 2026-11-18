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
    test_clear: Option<Arc<dyn Fn() + Send + Sync>>,
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
            test_clear: None,
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
            test_clear: Some(clear),
        }
    }

    /// `tap_ok` false is a denied System Audio Recording permission: no row, no file.
    #[cfg(test)]
    pub fn start_for_test(&self, tap_ok: bool) -> Result<i64, MeetingRefusal> {
        self.begin(tap_ok).map(|started| started.id)
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

    fn begin(&self, tap_ok: bool) -> Result<StartedMeeting, MeetingRefusal> {
        let model = self.model_plan()?;
        let live = self.app.is_some() && self.test_root.is_none();
        if live {
            #[cfg(not(target_os = "macos"))]
            {
                let _ = (model, tap_ok);
                return Err(MeetingRefusal::SystemAudioUnavailable);
            }
        }
        self.pin_model(&model);
        match self.open_tracks(&model, tap_ok, live) {
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
    }

    fn clear_runtime(&self) {
        crate::managers::meetings::set_meeting_active(false);
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
        let heard = heard_audio(active);
        let mut warnings = active.feed.warnings.lock().unwrap_or_else(poison).clone();
        if heard {
            warnings.retain(|warning| *warning != MeetingWarning::SystemAudioSilent);
        } else if elapsed_secs >= SILENT_AFTER_SECS
            && !warnings.contains(&MeetingWarning::SystemAudioSilent)
        {
            warnings.push(MeetingWarning::SystemAudioSilent);
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
        self.begin(true)
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
    if active.fake && !active.feed.heard.load(Ordering::Relaxed) {
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

#[cfg(target_os = "macos")]
fn bundle_plist() -> Result<String, MeetingRefusal> {
    let exe = std::env::current_exe().map_err(|_| MeetingRefusal::SystemAudioUsageStringMissing)?;
    let plist = exe
        .parent()
        .and_then(|dir| dir.parent())
        .map(|contents| contents.join("Info.plist"))
        .ok_or(MeetingRefusal::SystemAudioUsageStringMissing)?;
    std::fs::read_to_string(plist).map_err(|_| MeetingRefusal::SystemAudioUsageStringMissing)
}

#[cfg(target_os = "macos")]
fn map_tap(err: crate::meetings_tap::TapError) -> MeetingRefusal {
    match err {
        crate::meetings_tap::TapError::UsageStringMissing => {
            MeetingRefusal::SystemAudioUsageStringMissing
        }
        crate::meetings_tap::TapError::PermissionDenied => {
            MeetingRefusal::SystemAudioPermissionDenied
        }
        crate::meetings_tap::TapError::Silent => MeetingRefusal::SystemAudioUnavailable,
        crate::meetings_tap::TapError::Device(message) => {
            log::warn!("meeting system audio failed to start: {message}");
            MeetingRefusal::SystemAudioUnavailable
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
    ) -> Result<StartedMeeting, MeetingRefusal> {
        if !tap_ok {
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
        let hold = if live { self.hold_tap()? } else { None };
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
            None
        };
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
        };
        *self.active.lock().unwrap_or_else(poison) = Some(Active {
            id,
            feed,
            worker: Some(worker),
            mic,
            #[cfg(target_os = "macos")]
            tap: hold.map(|hold| hold.tap),
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
            .open_tracks(PARAKEET, true, true)
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
}
