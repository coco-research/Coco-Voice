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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum MeetingRefusal {
    /// One meeting at a time: a Start arrived while a meeting records.
    AlreadyRecording,
}

/// Why a recording ended. PR6b flushes on either.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeetingEnd {
    Stopped,
    Slept,
}

/// What the frontend and the pill see.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
pub struct MeetingStatus {
    pub state: MeetingState,
    /// Whole seconds since Start; zero when no meeting is recording.
    pub elapsed_secs: u64,
}

/// The seam PR6b fills with the real recorders. Defaults are no-ops so this PR
/// can move the state machine without capturing anything.
pub trait MeetingRecorders: Send + Sync {
    /// Begin capturing the Me and Others tracks for `meeting_id`.
    fn start(&self, _meeting_id: &str) {}

    /// Flush and finalise both tracks. Nothing resumes afterwards.
    fn stop(&self, _meeting_id: &str, _reason: MeetingEnd) {}
}

/// Used until PR6b supplies the real recorders.
pub struct NoopRecorders;

impl MeetingRecorders for NoopRecorders {}

struct MeetingInner {
    state: MeetingState,
    started_at: Option<Instant>,
    meeting_id: Option<String>,
    /// Only names the folder PR6b will write. Never touches the filesystem here.
    next_sequence: u64,
}

impl MeetingInner {
    fn status(&self) -> MeetingStatus {
        MeetingStatus {
            state: self.state,
            elapsed_secs: self
                .started_at
                .map(|started| started.elapsed().as_secs())
                .unwrap_or(0),
        }
    }
}

/// One meeting at a time, from consent to Idle.
pub struct MeetingController {
    inner: Mutex<MeetingInner>,
    recorders: Box<dyn MeetingRecorders>,
}

impl MeetingController {
    pub fn new(recorders: Box<dyn MeetingRecorders>) -> Self {
        Self {
            inner: Mutex::new(MeetingInner {
                state: MeetingState::Idle,
                started_at: None,
                meeting_id: None,
                next_sequence: 0,
            }),
            recorders,
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, MeetingInner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn state(&self) -> MeetingState {
        self.lock().state
    }

    fn meeting_id(&self) -> Option<String> {
        self.lock().meeting_id.clone()
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn status(&self) -> MeetingStatus {
        self.lock().status()
    }

    /// The shortcut path. It only ever opens the dialog: an earlier accept is
    /// not remembered, and a recording meeting is left alone.
    pub fn open_consent(&self) -> MeetingStatus {
        let mut inner = self.lock();
        if inner.state == MeetingState::Idle {
            inner.state = MeetingState::ConsentOpen;
        }
        inner.status()
    }

    /// Start refuses while a meeting records. The dialog owns the consent step;
    /// a start from Idle is allowed so the offer can open the dialog without
    /// having to know the controller's state.
    pub fn confirm_start(&self) -> Result<MeetingStatus, MeetingRefusal> {
        let (meeting_id, status) = {
            let mut inner = self.lock();
            match inner.state {
                MeetingState::Recording | MeetingState::Stopping => {
                    return Err(MeetingRefusal::AlreadyRecording)
                }
                MeetingState::Idle | MeetingState::ConsentOpen => {
                    inner.next_sequence += 1;
                    let meeting_id = format!("meeting-{}", inner.next_sequence);
                    inner.meeting_id = Some(meeting_id.clone());
                    inner.started_at = Some(Instant::now());
                    inner.state = MeetingState::Recording;
                    (meeting_id, inner.status())
                }
            }
        };
        self.recorders.start(&meeting_id);
        Ok(status)
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

    /// The Mac slept. 6b adds the flush to `MeetingRecorders::stop`.
    pub fn on_system_sleep(&self) -> MeetingStatus {
        self.finish(MeetingEnd::Slept)
    }

    /// Only the meeting's own Stop (or sleep) may hide the pill.
    pub fn pill_visible(&self) -> bool {
        matches!(
            self.state(),
            MeetingState::Recording | MeetingState::Stopping
        )
    }

    fn finish(&self, reason: MeetingEnd) -> MeetingStatus {
        let meeting_id = {
            let mut inner = self.lock();
            // Only Recording moves on: a Stop or sleep that lands while another
            // finish is flushing must not flush the same meeting twice.
            match inner.state {
                MeetingState::Recording => {
                    inner.state = MeetingState::Stopping;
                    inner.meeting_id.clone()
                }
                _ => return inner.status(),
            }
        };

        if let Some(meeting_id) = meeting_id {
            self.recorders.stop(&meeting_id, reason);
        }

        let mut inner = self.lock();
        inner.state = MeetingState::Idle;
        inner.started_at = None;
        inner.meeting_id = None;
        inner.status()
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
    emit_status(&app, status);
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
    let status = meeting.confirm_start()?;
    emit_status(&app, status);
    overlay::show_meeting_overlay(&app);
    if let Some(meeting_id) = meeting.meeting_id() {
        watch_for_sleep(app.clone(), meeting_id);
    }
    Ok(status)
}

#[tauri::command]
#[specta::specta]
pub fn cancel_meeting_consent(app: AppHandle) -> MeetingStatus {
    let status = controller(&app).cancel_consent();
    emit_status(&app, status);
    status
}

#[tauri::command]
#[specta::specta]
pub fn stop_meeting(app: AppHandle) -> MeetingStatus {
    let meeting = controller(&app);
    let was_recording = meeting.pill_visible();
    let status = meeting.stop();
    emit_status(&app, status);
    // State is Idle now, so the meeting guard lets this hide through. Only hide
    // what this call ended: a stray Stop on an idle controller must not take
    // down a dictation pill.
    if was_recording {
        overlay::hide_recording_overlay(&app);
    }
    status
}

/// Ends the meeting when the Mac sleeps.
///
/// macOS has no dependency-free sleep callback: `NSWorkspaceWillSleepNotification`
/// is Objective-C and the crate that wraps it arrives with the recorders in
/// PR6b. The monotonic clock (`Instant`, `mach_absolute_time` on macOS) stops
/// while the Mac sleeps and the wall clock keeps moving, so a gap between them
/// is a sleep. The thread lives only while its own meeting is current, so a
/// quick Stop and Start never leaves an old watcher running beside the new one.
fn watch_for_sleep(app: AppHandle, meeting_id: String) {
    std::thread::spawn(move || {
        let mut previous = (SystemTime::now(), Instant::now());
        loop {
            std::thread::sleep(SLEEP_POLL_INTERVAL);
            let meeting = controller(&app);
            if meeting.meeting_id().as_deref() != Some(meeting_id.as_str()) {
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
        starts: Mutex<Vec<String>>,
        stops: Mutex<Vec<(String, MeetingEnd)>>,
    }

    #[derive(Clone, Default)]
    struct FakeRecorders(Arc<RecorderLog>);

    impl MeetingRecorders for FakeRecorders {
        fn start(&self, meeting_id: &str) {
            self.0.starts.lock().unwrap().push(meeting_id.to_string());
        }

        fn stop(&self, meeting_id: &str, reason: MeetingEnd) {
            self.0
                .stops
                .lock()
                .unwrap()
                .push((meeting_id.to_string(), reason));
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

        let meeting_id = log.0.starts.lock().unwrap()[0].clone();
        assert_eq!(
            log.0.stops.lock().unwrap().as_slice(),
            &[(meeting_id, MeetingEnd::Slept)]
        );

        // Nothing is left to end a second time.
        assert_eq!(controller.on_system_sleep().state, MeetingState::Idle);
        assert_eq!(log.0.stops.lock().unwrap().len(), 1);
    }
}
