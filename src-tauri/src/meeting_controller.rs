//! Meetings M1, PR6a: the consent dialog, the recording pill and one meeting at
//! a time.
//!
//! Nothing here captures audio. `confirm_meeting_start` moves the state machine
//! and starts the elapsed clock; PR6b fills [`MeetingRecorders`] with the Me and
//! Others recorders. The commands are registered from `lib.rs` only in a build
//! with the `meetings` feature.

use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant, SystemTime};

use serde::Serialize;
use specta::Type;
use tauri::{AppHandle, Emitter, Manager};

use crate::overlay;

/// Emitted when the consent dialog should open. PR7's offer emits the same name,
/// so the dialog has one entry point.
pub const OPEN_CONSENT_EVENT: &str = "meetings-open-consent";

/// Emitted on every state transition so the views can follow the controller.
pub const STATE_EVENT: &str = "meeting-state-changed";

/// One stored chunk. Payload is [`MeetingSegmentView`].
pub const SEGMENT_EVENT: &str = "meeting-segment";

/// The overlay state that renders the meeting pill. It reuses the dictation
/// overlay window and its 256 x 46 size.
pub const PILL_OVERLAY_STATE: &str = "meeting";

/// A wall-clock jump this large with no matching advance of the monotonic clock
/// means the Mac slept. Coarse on purpose: an NTP correction of this size also
/// ends the meeting, which is the safe direction.
const SLEEP_DETECT_GAP: Duration = Duration::from_secs(5);

/// A second between wall-clock and monotonic readings while a meeting records.
const SLEEP_POLL_INTERVAL: Duration = Duration::from_secs(1);

/// Where the meeting lifecycle stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum MeetingState {
    Idle,
    ConsentOpen,
    Recording,
    Stopping,
}

/// Why a start was refused, typed so the UI can say which rule stopped it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum MeetingRefusal {
    /// One meeting at a time: a Start arrived while a meeting records.
    AlreadyRecording,
    /// No downloaded speech model can do long audio (every one is Voxtral, or none).
    NoLongAudioModel {
        model_id: String,
    },
    SystemAudioPermissionDenied,
    SystemAudioUsageStringMissing,
    SystemAudioUnavailable,
}

/// Why a recording ended. The recorders flush on every one of these.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeetingEnd {
    Stopped,
    Slept,
    /// A WAV write failed. The row is finalised with what reached disk.
    WriteError,
}

impl MeetingState {
    /// The pill and its Stop stay up while a meeting records or flushes.
    pub fn pill_visible(self) -> bool {
        matches!(self, Self::Recording | Self::Stopping)
    }
}

/// Tap ran 10 s with no audio, or the selected mic was missing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum MeetingWarning {
    SystemAudioSilent,
    MicFellBackToDefault,
}

/// One stored chunk, for `meeting-segment` and `meeting_segments`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct MeetingSegmentView {
    pub meeting_id: i64,
    pub channel: crate::managers::meetings::MeetingChannel,
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
}

/// What the frontend and the pill see.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct MeetingStatus {
    pub state: MeetingState,
    /// Whole seconds since Start; zero when no meeting is recording.
    pub elapsed_secs: u64,
    /// meetings.db row id. Some while recording or stopping, and after stop
    /// until the next consent.
    pub meeting_id: Option<i64>,
    /// The microphone Me is actually recording from.
    pub mic_name: Option<String>,
    pub warnings: Vec<MeetingWarning>,
}

/// What a successful start created. `id` is the meetings.db row id.
pub struct StartedMeeting {
    pub id: i64,
    pub mic_name: Option<String>,
    pub warnings: Vec<MeetingWarning>,
}

/// Live warnings and the write-error flag, read once a second.
#[derive(Debug, Clone, Default)]
pub struct SessionPoll {
    pub mic_name: Option<String>,
    pub warnings: Vec<MeetingWarning>,
    pub write_failed: bool,
}

/// The seam the session fills. Defaults record nothing.
pub trait MeetingRecorders: Send + Sync {
    fn start(&self) -> Result<StartedMeeting, MeetingRefusal> {
        Ok(StartedMeeting {
            id: 0,
            mic_name: None,
            warnings: Vec::new(),
        })
    }

    /// Flush and finalise both tracks. Returns the warnings still in force.
    fn stop(&self, _meeting_id: i64, _reason: MeetingEnd) -> Vec<MeetingWarning> {
        Vec::new()
    }

    fn poll(&self, _meeting_id: i64, _elapsed_secs: u64) -> SessionPoll {
        SessionPoll::default()
    }
}

/// Used when no session recorders are wired in.
#[cfg_attr(not(test), allow(dead_code))]
pub struct NoopRecorders;

impl MeetingRecorders for NoopRecorders {}

/// Triggered once off the UI thread after an explicit Stop finalises tracks.
pub trait NotesTrigger: Send + Sync {
    fn run_after_stop(&self, _meeting_id: i64) {}
}

struct NoopNotesTrigger;

impl NotesTrigger for NoopNotesTrigger {}

struct MeetingInner {
    state: MeetingState,
    started_at: Option<Instant>,
    meeting_id: Option<i64>,
    mic_name: Option<String>,
    warnings: Vec<MeetingWarning>,
}

impl MeetingInner {
    fn status(&self) -> MeetingStatus {
        MeetingStatus {
            state: self.state,
            elapsed_secs: self
                .started_at
                .map(|started| started.elapsed().as_secs())
                .unwrap_or(0),
            meeting_id: self.meeting_id,
            mic_name: self.mic_name.clone(),
            warnings: self.warnings.clone(),
        }
    }
}

/// Holds the start lock until the caller has committed Recording and shown the pill.
pub struct CapturePermit<'a> {
    _guard: std::sync::MutexGuard<'a, ()>,
    started: StartedMeeting,
}

impl CapturePermit<'_> {
    pub fn started(&self) -> &StartedMeeting {
        &self.started
    }
}

/// One meeting at a time, from consent to Idle.
pub struct MeetingController {
    inner: Mutex<MeetingInner>,
    /// One start or stop at a time. Held from the tap through the pill show.
    start_gate: Mutex<()>,
    recorders: Box<dyn MeetingRecorders>,
    notes_trigger: Arc<dyn NotesTrigger>,
}

impl MeetingController {
    pub fn new(recorders: Box<dyn MeetingRecorders>) -> Self {
        Self::with_notes_trigger(recorders, Arc::new(NoopNotesTrigger))
    }

    pub fn with_notes_trigger(
        recorders: Box<dyn MeetingRecorders>,
        notes_trigger: Arc<dyn NotesTrigger>,
    ) -> Self {
        Self {
            inner: Mutex::new(MeetingInner {
                state: MeetingState::Idle,
                started_at: None,
                meeting_id: None,
                mic_name: None,
                warnings: Vec::new(),
            }),
            start_gate: Mutex::new(()),
            recorders,
            notes_trigger,
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, MeetingInner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn state(&self) -> MeetingState {
        self.lock().state
    }

    fn meeting_id(&self) -> Option<i64> {
        self.lock().meeting_id
    }

    pub fn status(&self) -> MeetingStatus {
        self.lock().status()
    }

    /// The shortcut path. It only ever opens the dialog: an earlier accept is
    /// not remembered, and a recording meeting is left alone.
    pub fn open_consent(&self) -> MeetingStatus {
        let mut inner = self.lock();
        if inner.state == MeetingState::Idle {
            inner.state = MeetingState::ConsentOpen;
            inner.meeting_id = None;
            inner.mic_name = None;
            inner.warnings.clear();
            inner.started_at = None;
        }
        inner.status()
    }

    /// Capture, but do not enter Recording yet. The permit's lock is what the
    /// overlay gate shares with a dictation hide: drop it only after the pill
    /// is shown.
    pub fn start_capture(&self) -> Result<CapturePermit<'_>, MeetingRefusal> {
        let guard = self
            .start_gate
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if self.state().pill_visible() {
            return Err(MeetingRefusal::AlreadyRecording);
        }
        let started = self.recorders.start()?;
        Ok(CapturePermit {
            _guard: guard,
            started,
        })
    }

    /// Enter Recording. Call under the overlay gate, still holding the permit.
    pub fn commit_capture(&self, started: &StartedMeeting) -> MeetingStatus {
        let mut inner = self.lock();
        inner.meeting_id = Some(started.id);
        inner.mic_name = started.mic_name.clone();
        inner.warnings = started.warnings.clone();
        inner.started_at = Some(Instant::now());
        inner.state = MeetingState::Recording;
        inner.status()
    }

    /// Start refuses while a meeting records. The dialog owns the consent step;
    /// a start from Idle is allowed so the offer can open the dialog without
    /// having to know the controller's state.
    pub fn confirm_start(&self) -> Result<MeetingStatus, MeetingRefusal> {
        let permit = self.start_capture()?;
        Ok(self.commit_capture(permit.started()))
    }

    pub fn cancel_consent(&self) -> MeetingStatus {
        let mut inner = self.lock();
        if inner.state == MeetingState::ConsentOpen {
            inner.state = MeetingState::Idle;
            inner.started_at = None;
            inner.meeting_id = None;
        }
        inner.status()
    }

    /// The pill's Stop, or the tray action that ends a meeting.
    pub fn stop(&self) -> MeetingStatus {
        self.finish(MeetingEnd::Stopped)
    }

    /// The Mac slept.
    pub fn on_system_sleep(&self) -> MeetingStatus {
        self.finish(MeetingEnd::Slept)
    }

    /// Only the meeting's own Stop (or sleep) may hide the pill.
    pub fn pill_visible(&self) -> bool {
        self.state().pill_visible()
    }

    /// One-second tick: refresh warnings, and end the meeting if a write failed.
    pub fn refresh(&self) -> MeetingStatus {
        let (id, elapsed, recording) = {
            let inner = self.lock();
            (
                inner.meeting_id,
                inner
                    .started_at
                    .map(|started| started.elapsed().as_secs())
                    .unwrap_or(0),
                inner.state == MeetingState::Recording,
            )
        };
        if recording {
            if let Some(id) = id {
                let poll = self.recorders.poll(id, elapsed);
                if poll.write_failed {
                    return self.finish(MeetingEnd::WriteError);
                }
                let mut inner = self.lock();
                if inner.state == MeetingState::Recording && inner.meeting_id == Some(id) {
                    inner.warnings = poll.warnings;
                    if poll.mic_name.is_some() {
                        inner.mic_name = poll.mic_name;
                    }
                }
            }
        }
        self.lock().status()
    }

    fn finish(&self, reason: MeetingEnd) -> MeetingStatus {
        let _gate = self
            .start_gate
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let meeting_id = {
            let mut inner = self.lock();
            // Only Recording moves on: a Stop or sleep that lands while another
            // finish is flushing must not flush the same meeting twice.
            match inner.state {
                MeetingState::Recording => {
                    inner.state = MeetingState::Stopping;
                    inner.meeting_id
                }
                _ => return inner.status(),
            }
        };

        let warnings = match meeting_id {
            Some(id) => self.recorders.stop(id, reason),
            None => Vec::new(),
        };

        let status = {
            let mut inner = self.lock();
            if inner.state == MeetingState::Stopping {
                inner.state = MeetingState::Idle;
                inner.started_at = None;
                if !warnings.is_empty() {
                    inner.warnings = warnings;
                }
            }
            inner.status()
        };
        if matches!(reason, MeetingEnd::Stopped) {
            if let Some(id) = meeting_id {
                self.notes_trigger.run_after_stop(id);
            }
        }
        status
    }
}

impl Default for MeetingController {
    fn default() -> Self {
        Self::new(Box::new(NoopRecorders))
    }
}

fn controller(app: &AppHandle) -> Arc<MeetingController> {
    Arc::clone(app.state::<Arc<MeetingController>>().inner())
}

/// Current state for callers outside the command layer (the overlay gate).
pub fn state(app: &AppHandle) -> Option<MeetingState> {
    app.try_state::<Arc<MeetingController>>()
        .map(|controller| controller.state())
}

fn emit_status(app: &AppHandle, status: MeetingStatus) {
    let _ = app.emit(STATE_EVENT, status);
}

#[tauri::command]
#[specta::specta]
pub fn open_meeting_consent(app: AppHandle) -> MeetingStatus {
    let status = controller(&app).open_consent();
    emit_status(&app, status.clone());
    if status.state == MeetingState::ConsentOpen {
        // The dialog lives in the main window: surface it when the shortcut is
        // pressed from a hidden or minimised app.
        crate::show_main_window(&app);
        let _ = app.emit(OPEN_CONSENT_EVENT, ());
    }
    status
}

#[tauri::command]
#[specta::specta]
pub fn confirm_meeting_start(app: AppHandle) -> Result<MeetingStatus, MeetingRefusal> {
    let meeting = controller(&app);
    // Same lock a dictation hide holds across its state read and its hide.
    let status = overlay::with_meeting_pill_gate(|| {
        let status = meeting.confirm_start()?;
        emit_status(&app, status.clone());
        overlay::show_meeting_overlay(&app);
        Ok(status)
    })?;
    if let Some(meeting_id) = status.meeting_id {
        watch_for_sleep(app, meeting_id);
    }
    Ok(status)
}

#[tauri::command]
#[specta::specta]
pub fn cancel_meeting_consent(app: AppHandle) -> MeetingStatus {
    let status = controller(&app).cancel_consent();
    emit_status(&app, status.clone());
    status
}

#[tauri::command]
#[specta::specta]
pub fn stop_meeting(app: AppHandle) -> MeetingStatus {
    let meeting = controller(&app);
    let was_recording = meeting.pill_visible();
    let status = meeting.stop();
    emit_status(&app, status.clone());
    // State is Idle now, so the meeting guard lets this hide through. Only hide
    // what this call ended: a stray Stop on an idle controller must not take
    // down a dictation pill.
    if was_recording {
        overlay::hide_recording_overlay(&app);
    }
    status
}

#[tauri::command]
#[specta::specta]
pub fn meeting_status(app: AppHandle) -> MeetingStatus {
    controller(&app).status()
}

#[tauri::command]
#[specta::specta]
pub fn meeting_segments(
    app: AppHandle,
    meeting_id: i64,
) -> Result<Vec<MeetingSegmentView>, String> {
    let store = crate::managers::meetings::MeetingsStore::open_app(&app).map_err(|err| {
        log::warn!("meeting segments: {err}");
        err.to_string()
    })?;
    let rows = store.segments(meeting_id).map_err(|err| err.to_string())?;
    let mut views = Vec::with_capacity(rows.len());
    for row in rows {
        let channel = crate::managers::meetings::MeetingChannel::from_label(&row.channel)
            .ok_or_else(|| "unknown meeting channel".to_string())?;
        views.push(MeetingSegmentView {
            meeting_id: row.meeting_id,
            channel,
            start_ms: row.start_ms,
            end_ms: row.end_ms,
            text: row.text,
        });
    }
    Ok(views)
}

/// Ends the meeting when the Mac sleeps.
///
/// The monotonic clock (`Instant`, `mach_absolute_time` on macOS) stops while
/// the Mac sleeps and the wall clock keeps moving, so a gap between them is a
/// sleep. The thread lives only while its own meeting is current, so a quick
/// Stop and Start never leaves an old watcher running beside the new one.
fn watch_for_sleep(app: AppHandle, meeting_id: i64) {
    std::thread::spawn(move || {
        let mut previous = (SystemTime::now(), Instant::now());
        loop {
            std::thread::sleep(SLEEP_POLL_INTERVAL);
            let meeting = controller(&app);
            if meeting.meeting_id() != Some(meeting_id) || !meeting.pill_visible() {
                return;
            }
            let status = meeting.refresh();
            emit_status(&app, status.clone());
            if !status.state.pill_visible() {
                overlay::hide_recording_overlay(&app);
                return;
            }
            let now = (SystemTime::now(), Instant::now());
            let wall = now.0.duration_since(previous.0).unwrap_or_default();
            let monotonic = now.1.duration_since(previous.1);
            previous = now;
            if wall.saturating_sub(monotonic) >= SLEEP_DETECT_GAP {
                emit_status(&app, meeting.on_system_sleep());
                overlay::hide_recording_overlay(&app);
                return;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct RecorderLog {
        starts: Mutex<Vec<i64>>,
        stops: Mutex<Vec<(i64, MeetingEnd)>>,
        poll_fail: Mutex<bool>,
    }

    #[derive(Clone, Default)]
    struct FakeRecorders(Arc<RecorderLog>);

    impl MeetingRecorders for FakeRecorders {
        fn start(&self) -> Result<StartedMeeting, MeetingRefusal> {
            let mut starts = self.0.starts.lock().unwrap();
            let id = starts.len() as i64 + 1;
            starts.push(id);
            Ok(StartedMeeting {
                id,
                mic_name: Some("Built-in".to_string()),
                warnings: Vec::new(),
            })
        }

        fn stop(&self, meeting_id: i64, reason: MeetingEnd) -> Vec<MeetingWarning> {
            self.0.stops.lock().unwrap().push((meeting_id, reason));
            Vec::new()
        }

        fn poll(&self, _meeting_id: i64, _elapsed_secs: u64) -> SessionPoll {
            SessionPoll {
                mic_name: None,
                warnings: Vec::new(),
                write_failed: *self.0.poll_fail.lock().unwrap(),
            }
        }
    }

    fn controller_with_fake() -> (MeetingController, FakeRecorders) {
        let log = FakeRecorders::default();
        let controller = MeetingController::new(Box::new(log.clone()));
        (controller, log)
    }

    #[test]
    fn shortcut_opens_consent_only() {
        let (controller, log) = controller_with_fake();

        // What the meeting shortcut does. It opens the dialog and never starts.
        assert_eq!(controller.open_consent().state, MeetingState::ConsentOpen);
        assert!(
            log.0.starts.lock().unwrap().is_empty(),
            "the shortcut must not start a meeting"
        );

        // An accept is not remembered as consent for a later press.
        controller.confirm_start().unwrap();
        assert_eq!(log.0.starts.lock().unwrap().len(), 1);
        controller.stop();
        assert_eq!(controller.open_consent().state, MeetingState::ConsentOpen);
        assert_eq!(
            log.0.starts.lock().unwrap().len(),
            1,
            "a later press must not start from an earlier accept"
        );

        // While a meeting records, the shortcut neither restarts nor resets it.
        controller.cancel_consent();
        controller.open_consent();
        controller.confirm_start().unwrap();
        let starts = log.0.starts.lock().unwrap().len();
        assert_eq!(controller.open_consent().state, MeetingState::Recording);
        assert_eq!(controller.state(), MeetingState::Recording);
        assert_eq!(log.0.starts.lock().unwrap().len(), starts);
    }

    #[test]
    fn second_start_refused() {
        let (controller, log) = controller_with_fake();
        controller.open_consent();
        controller.confirm_start().unwrap();

        assert_eq!(
            controller.confirm_start(),
            Err(MeetingRefusal::AlreadyRecording)
        );
        assert_eq!(controller.state(), MeetingState::Recording);
        assert_eq!(log.0.starts.lock().unwrap().len(), 1);
    }

    #[test]
    fn cancel_creates_nothing() {
        let (controller, log) = controller_with_fake();
        controller.open_consent();

        let status = controller.cancel_consent();
        assert_eq!(status.state, MeetingState::Idle);
        assert_eq!(status.elapsed_secs, 0);
        assert!(
            log.0.starts.lock().unwrap().is_empty(),
            "Cancel never starts a recorder and never creates a file"
        );
        assert!(log.0.stops.lock().unwrap().is_empty());
        assert!(!controller.pill_visible());
    }

    #[test]
    fn sleep_ends_a_recording_meeting() {
        let (controller, log) = controller_with_fake();
        controller.open_consent();
        controller.confirm_start().unwrap();
        assert!(controller.pill_visible());

        let status = controller.on_system_sleep();
        assert_eq!(status.state, MeetingState::Idle);
        assert!(!controller.pill_visible());

        let meeting_id = log.0.starts.lock().unwrap()[0];
        assert_eq!(
            log.0.stops.lock().unwrap().as_slice(),
            &[(meeting_id, MeetingEnd::Slept)]
        );

        // Nothing is left to end a second time.
        assert_eq!(controller.on_system_sleep().state, MeetingState::Idle);
        assert_eq!(log.0.stops.lock().unwrap().len(), 1);
    }

    #[test]
    fn stop_finalises_a_recording_meeting() {
        let (controller, log) = controller_with_fake();
        controller.open_consent();
        controller.confirm_start().unwrap();
        assert!(controller.pill_visible());

        let status = controller.stop();
        assert_eq!(status.state, MeetingState::Idle);
        assert!(!controller.pill_visible());

        let meeting_id = log.0.starts.lock().unwrap()[0];
        assert_eq!(
            log.0.stops.lock().unwrap().as_slice(),
            &[(meeting_id, MeetingEnd::Stopped)]
        );

        // Nothing is left to stop a second time.
        assert_eq!(controller.stop().state, MeetingState::Idle);
        assert_eq!(log.0.stops.lock().unwrap().len(), 1);
    }

    #[test]
    fn status_keeps_numeric_meeting_id_after_stop_and_write_error_ends_recording() {
        let (controller, log) = controller_with_fake();
        controller.open_consent();
        let started = controller.confirm_start().unwrap();
        assert_eq!(started.meeting_id, Some(1));
        assert_eq!(started.mic_name.as_deref(), Some("Built-in"));

        let stopped = controller.stop();
        assert_eq!(stopped.state, MeetingState::Idle);
        assert_eq!(
            controller.status().meeting_id,
            Some(1),
            "post-stop view needs meeting_id until the next consent"
        );

        let reopened = controller.open_consent();
        assert_eq!(reopened.meeting_id, None);
        controller.confirm_start().unwrap();
        *log.0.poll_fail.lock().unwrap() = true;
        let refreshed = controller.refresh();
        assert_eq!(refreshed.state, MeetingState::Idle);
        assert_eq!(
            log.0.stops.lock().unwrap().last(),
            Some(&(2, MeetingEnd::WriteError))
        );
    }
}
