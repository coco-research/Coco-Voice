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
    Paused,
    Stopping,
}

/// Whether a system-audio tap start failure points to permission or audio device setup.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum SystemAudioFailureCause {
    Permission,
    Device,
}

/// Why a start was refused, typed so the UI can say which rule stopped it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum MeetingRefusal {
    /// One meeting at a time: a Start arrived while a meeting records.
    AlreadyRecording,
    /// Meetings switch is off (or Meetings isn't visible on this host).
    MeetingsOff,
    /// No downloaded speech model can do long audio (every one is Voxtral, or none).
    NoLongAudioModel {
        model_id: String,
    },
    SystemAudioPermissionDenied,
    SystemAudioUsageStringMissing,
    SystemAudioUnavailable,
    /// Named Core Audio tap failure with cause (`permission` vs `device`), call name, and OSStatus.
    SystemAudioTapFailed {
        cause: SystemAudioFailureCause,
        call: String,
        code: String,
    },
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
    /// The pill and its Stop stay up while a meeting records, is paused, or flushes.
    pub fn pill_visible(self) -> bool {
        matches!(self, Self::Recording | Self::Paused | Self::Stopping)
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
    /// Whole seconds of recorded talk (pauses are excluded).
    pub elapsed_secs: u64,
    /// True while the active meeting is paused (`AC-X14`).
    pub paused: bool,
    /// Current meeting title for the older-note banner (`AC-X20`).
    pub title: Option<String>,
    /// Calendar attendee display names with EventKit's current user first (`I7`).
    pub attendees: Vec<String>,
    /// meetings.db row id. Some while recording, paused, stopping, and after stop
    /// until the next consent.
    pub meeting_id: Option<i64>,
    /// The microphone Me is actually recording from.
    pub mic_name: Option<String>,
    /// True when started in mic-only mode (system-audio tap skipped, `D-R3`).
    pub mic_only: bool,
    pub warnings: Vec<MeetingWarning>,
}

/// What a successful start created. `id` is the meetings.db row id.
pub struct StartedMeeting {
    pub id: i64,
    pub mic_name: Option<String>,
    pub warnings: Vec<MeetingWarning>,
    pub mic_only: bool,
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
            mic_only: false,
        })
    }

    /// A recorder set that can't skip the system tap refuses; it never falls back to a full start.
    fn start_mic_only(&self) -> Result<StartedMeeting, MeetingRefusal> {
        Err(MeetingRefusal::SystemAudioUnavailable)
    }

    /// Flush and finalise both tracks. Returns the warnings still in force.
    fn stop(&self, _meeting_id: i64, _reason: MeetingEnd) -> Vec<MeetingWarning> {
        Vec::new()
    }

    fn poll(&self, _meeting_id: i64, _elapsed_secs: u64) -> SessionPoll {
        SessionPoll::default()
    }

    /// Pause feeding both recorders and the speech engine (writes 0 frames while paused).
    fn pause(&self, _meeting_id: i64) {}

    /// Resume feeding both recorders and the speech engine without re-asking consent.
    fn resume(&self, _meeting_id: i64) {}
}

/// Used when no session recorders are wired in.
#[cfg_attr(not(test), allow(dead_code))]
pub struct NoopRecorders;

impl MeetingRecorders for NoopRecorders {}

struct MeetingInner {
    state: MeetingState,
    started_at: Option<Instant>,
    recorded_elapsed: Duration,
    title: Option<String>,
    attendees: Vec<String>,
    meeting_id: Option<i64>,
    mic_name: Option<String>,
    mic_only: bool,
    warnings: Vec<MeetingWarning>,
    pending_template_id: Option<i64>,
}

impl MeetingInner {
    fn elapsed_secs(&self) -> u64 {
        let active_span = if self.state == MeetingState::Recording {
            self.started_at
                .map(|started| started.elapsed())
                .unwrap_or_default()
        } else {
            Duration::ZERO
        };
        (self.recorded_elapsed + active_span).as_secs()
    }

    fn status(&self) -> MeetingStatus {
        MeetingStatus {
            state: self.state,
            elapsed_secs: self.elapsed_secs(),
            paused: self.state == MeetingState::Paused,
            title: self.title.clone(),
            attendees: self.attendees.clone(),
            meeting_id: self.meeting_id,
            mic_name: self.mic_name.clone(),
            mic_only: self.mic_only,
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
    append_gate: crate::meeting_append::AppendConsentGate,
    recorders: Box<dyn MeetingRecorders>,
}

impl MeetingController {
    pub fn new(recorders: Box<dyn MeetingRecorders>) -> Self {
        Self {
            inner: Mutex::new(MeetingInner {
                state: MeetingState::Idle,
                started_at: None,
                recorded_elapsed: Duration::ZERO,
                title: None,
                attendees: Vec::new(),
                meeting_id: None,
                mic_name: None,
                mic_only: false,
                warnings: Vec::new(),
                pending_template_id: None,
            }),
            start_gate: Mutex::new(()),
            append_gate: crate::meeting_append::AppendConsentGate::new(),
            recorders,
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
        self.open_consent_with_template(None)
    }

    /// Open the consent dialog and remember an optional note template id until
    /// recording confirms or consent is cancelled. A recording meeting is left alone.
    pub fn open_consent_with_template(&self, template_id: Option<i64>) -> MeetingStatus {
        let mut inner = self.lock();
        if inner.state == MeetingState::Idle {
            inner.state = MeetingState::ConsentOpen;
            inner.meeting_id = None;
            inner.title = None;
            inner.attendees.clear();
            inner.mic_name = None;
            inner.mic_only = false;
            inner.warnings.clear();
            inner.started_at = None;
            inner.recorded_elapsed = Duration::ZERO;
            inner.pending_template_id = template_id;
        } else if inner.state == MeetingState::ConsentOpen {
            inner.pending_template_id = template_id;
        }
        inner.status()
    }

    pub fn take_pending_template_id(&self) -> Option<i64> {
        self.lock().pending_template_id.take()
    }

    pub fn set_title(&self, title: Option<String>) {
        self.lock().title = title;
    }

    pub fn set_attendees(&self, attendees: Vec<String>) {
        self.lock().attendees = attendees;
    }

    /// Issue a single-use consent token before appending audio to a finished meeting (`AC-X15`).
    pub fn request_append_consent(&self, meeting_id: i64) -> Result<u64, String> {
        if self.state().pill_visible() {
            return Err("cannot append audio while a meeting is active".to_string());
        }
        Ok(self.append_gate.request_consent(meeting_id))
    }

    /// Verify and consume the fresh append-audio consent token (`AC-X15`).
    pub fn consume_append_consent(&self, meeting_id: i64, token: u64) -> Result<(), String> {
        if self.state().pill_visible() {
            return Err("cannot append audio while a meeting is active".to_string());
        }
        self.append_gate
            .consume(meeting_id, token)
            .map_err(|e| format!("{e:?}"))
    }

    /// Pause the active recording: stops feeding recorders and freezes recorded time (`AC-X14`).
    pub fn pause(&self) -> MeetingStatus {
        let paused_id = {
            let mut inner = self.lock();
            if inner.state != MeetingState::Recording {
                return inner.status();
            }
            if let Some(started) = inner.started_at.take() {
                inner.recorded_elapsed += started.elapsed();
            }
            inner.state = MeetingState::Paused;
            inner.meeting_id
        };
        if let Some(id) = paused_id {
            self.recorders.pause(id);
        }
        self.lock().status()
    }

    /// Resume a paused meeting without asking for consent again (`AC-X14`).
    pub fn resume(&self) -> MeetingStatus {
        let resumed_id = {
            let mut inner = self.lock();
            if inner.state != MeetingState::Paused {
                return inner.status();
            }
            inner.started_at = Some(Instant::now());
            inner.state = MeetingState::Recording;
            inner.meeting_id
        };
        if let Some(id) = resumed_id {
            self.recorders.resume(id);
        }
        self.lock().status()
    }

    /// Capture, but do not enter Recording yet. The permit's lock is what the
    /// overlay gate shares with a dictation hide: drop it only after the pill
    /// is shown.
    pub fn start_capture(&self) -> Result<CapturePermit<'_>, MeetingRefusal> {
        self.start_capture_mode(false)
    }

    pub fn start_capture_mode(&self, mic_only: bool) -> Result<CapturePermit<'_>, MeetingRefusal> {
        let guard = self
            .start_gate
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if self.state().pill_visible() {
            return Err(MeetingRefusal::AlreadyRecording);
        }
        let started = if mic_only {
            self.recorders.start_mic_only()?
        } else {
            self.recorders.start()?
        };
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
        inner.mic_only = started.mic_only;
        inner.warnings = started.warnings.clone();
        inner.started_at = Some(Instant::now());
        inner.recorded_elapsed = Duration::ZERO;
        inner.state = MeetingState::Recording;
        inner.status()
    }

    /// Start refuses while a meeting records. The dialog owns the consent step;
    /// a start from Idle is allowed so the offer can open the dialog without
    /// having to know the controller's state.
    pub fn confirm_start(&self) -> Result<MeetingStatus, MeetingRefusal> {
        let permit = self.start_capture_mode(false)?;
        Ok(self.commit_capture(permit.started()))
    }

    /// Mic-only start (`D-R3`): skips system-audio tap, arms the mic, sets `mic_only: true`.
    pub fn confirm_start_mic_only(&self) -> Result<MeetingStatus, MeetingRefusal> {
        let permit = self.start_capture_mode(true)?;
        Ok(self.commit_capture(permit.started()))
    }

    pub fn cancel_consent(&self) -> MeetingStatus {
        let mut inner = self.lock();
        if inner.state == MeetingState::ConsentOpen {
            inner.state = MeetingState::Idle;
            inner.started_at = None;
            inner.meeting_id = None;
            inner.title = None;
            inner.attendees.clear();
            inner.mic_only = false;
            inner.pending_template_id = None;
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
            // Recording or Paused moves on: a Stop or sleep that lands while another
            // finish is flushing must not flush the same meeting twice.
            match inner.state {
                MeetingState::Recording | MeetingState::Paused => {
                    if let Some(started) = inner.started_at.take() {
                        inner.recorded_elapsed += started.elapsed();
                    }
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

        let mut inner = self.lock();
        if inner.state == MeetingState::Stopping {
            inner.state = MeetingState::Idle;
            inner.started_at = None;
            inner.recorded_elapsed = Duration::ZERO;
            inner.pending_template_id = None;
            if !warnings.is_empty() {
                inner.warnings = warnings;
            }
        }
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

pub(crate) fn apply_pending_template(
    meeting: &MeetingController,
    store: &crate::managers::meetings::MeetingsStore,
    meeting_id: i64,
) {
    if let Some(template_id) = meeting.take_pending_template_id() {
        if let Err(err) = store.set_meeting_template(meeting_id, Some(template_id)) {
            log::warn!("meeting {meeting_id}: failed to record template {template_id}: {err:#}");
        }
    }
}

#[tauri::command]
#[specta::specta]
pub fn open_meeting_consent(app: AppHandle, template_id: Option<i64>) -> MeetingStatus {
    let status = controller(&app).open_consent_with_template(template_id);
    emit_status(&app, status.clone());
    if status.state == MeetingState::ConsentOpen {
        // The dialog lives in the main window: surface it when the shortcut is
        // pressed from a hidden or minimised app.
        crate::meeting_offer_panel::hide_meeting_offer(&app);
        crate::show_main_window(&app);
        let _ = app.emit(OPEN_CONSENT_EVENT, ());
    }
    status
}

/// Pure M-c4 start gate used by `confirm_meeting_start_inner`: refuses with
/// `MeetingsOff` before touching capture/recorders when Meetings is not visible
/// on this host; leaves any already-recording meeting untouched.
fn confirm_start_if_visible(
    meeting: &MeetingController,
    meetings_visible: bool,
    mic_only: bool,
) -> Result<MeetingStatus, MeetingRefusal> {
    if !meetings_visible {
        return Err(MeetingRefusal::MeetingsOff);
    }
    if mic_only {
        meeting.confirm_start_mic_only()
    } else {
        meeting.confirm_start()
    }
}

fn confirm_meeting_start_inner(
    app: AppHandle,
    mic_only: bool,
) -> Result<MeetingStatus, MeetingRefusal> {
    let meetings_visible = crate::settings::meetings_visible_on_this_host(
        crate::settings::get_settings(&app).meetings_enabled,
    );
    let meeting = controller(&app);
    // Same lock a dictation hide holds across its state read and its hide.
    let mut status = overlay::with_meeting_pill_gate(|| {
        let status = confirm_start_if_visible(&meeting, meetings_visible, mic_only)?;
        emit_status(&app, status.clone());
        crate::meeting_offer_panel::hide_meeting_offer(&app);
        overlay::show_meeting_overlay(&app);
        let started_at_ms = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        if let Some(ov) = app.get_webview_window("recording_overlay") {
            let _ = ov.emit(
                "meeting-pill",
                serde_json::json!({ "started_at_ms": started_at_ms }),
            );
        }
        Ok(status)
    })?;
    crate::tray::update_tray_menu(&app, None);
    if let Some(meeting_id) = status.meeting_id {
        if let Ok(store) = crate::managers::meetings::MeetingsStore::open_app(&app) {
            apply_pending_template(&meeting, &store, meeting_id);
        } else {
            let _ = meeting.take_pending_template_id();
        }
        if let Some(calendar) = app.try_state::<crate::calendar_commands::CalendarState>() {
            if let Some(event) = calendar.take_started_for_notes() {
                if let Ok(store) = crate::managers::meetings::MeetingsStore::open_app(&app) {
                    let clean_title = event.title.trim();
                    if !clean_title.is_empty() {
                        let _ = store.rename_meeting(meeting_id, clean_title);
                        meeting.set_title(Some(clean_title.to_string()));
                    }
                    if !event.attendee_display_names.is_empty() {
                        let _ =
                            store.set_meeting_attendees(meeting_id, &event.attendee_display_names);
                        meeting.set_attendees(event.attendee_display_names.clone());
                    }
                }
                status = meeting.status();
                emit_status(&app, status.clone());
            }
        }
        crate::meeting_pipeline::start_rolling_if_managed(&app, meeting_id);
        watch_for_sleep(app, meeting_id);
    } else {
        let _ = meeting.take_pending_template_id();
    }
    Ok(status)
}

#[tauri::command]
#[specta::specta]
pub fn confirm_meeting_start(app: AppHandle) -> Result<MeetingStatus, MeetingRefusal> {
    confirm_meeting_start_inner(app, false)
}

#[tauri::command]
#[specta::specta]
pub fn confirm_meeting_start_mic_only(app: AppHandle) -> Result<MeetingStatus, MeetingRefusal> {
    confirm_meeting_start_inner(app, true)
}

#[tauri::command]
#[specta::specta]
pub fn pause_meeting_cmd(app: AppHandle) -> MeetingStatus {
    let status = controller(&app).pause();
    emit_status(&app, status.clone());
    status
}

#[tauri::command]
#[specta::specta]
pub fn resume_meeting_cmd(app: AppHandle) -> MeetingStatus {
    let status = controller(&app).resume();
    emit_status(&app, status.clone());
    status
}

#[tauri::command]
#[specta::specta]
pub fn request_append_meeting_consent(app: AppHandle, meeting_id: i64) -> Result<u64, String> {
    let store =
        crate::managers::meetings::MeetingsStore::open_app(&app).map_err(|err| err.to_string())?;
    let meeting = store.meeting(meeting_id).map_err(|err| err.to_string())?;
    if meeting.status == "recording" {
        return Err("cannot append audio while a meeting is active".to_string());
    }
    controller(&app).request_append_consent(meeting_id)
}

#[tauri::command]
#[specta::specta]
pub fn confirm_append_meeting_audio(
    app: AppHandle,
    meeting_id: i64,
    token: u64,
) -> Result<MeetingStatus, String> {
    let ctrl = controller(&app);
    ctrl.consume_append_consent(meeting_id, token)?;
    let store =
        crate::managers::meetings::MeetingsStore::open_app(&app).map_err(|err| err.to_string())?;
    let meeting = store.meeting(meeting_id).map_err(|err| err.to_string())?;
    if meeting.status == "recording" {
        return Err("cannot append audio while a meeting is active".to_string());
    }
    Ok(ctrl.status())
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
        if let Some(tm) =
            app.try_state::<Arc<crate::managers::transcription::TranscriptionManager>>()
        {
            tm.set_meeting_language_override(None);
        }
        crate::tray::update_tray_menu(&app, None);
        if let Some(meeting_id) = status.meeting_id {
            crate::meeting_pipeline::stop_rolling_and_run_after_stop(&app, meeting_id);
        }
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
                crate::meeting_pipeline::stop_rolling_and_run_after_stop(&app, meeting_id);
                return;
            }
            let now = (SystemTime::now(), Instant::now());
            let wall = now.0.duration_since(previous.0).unwrap_or_default();
            let monotonic = now.1.duration_since(previous.1);
            previous = now;
            if wall.saturating_sub(monotonic) >= SLEEP_DETECT_GAP {
                emit_status(&app, meeting.on_system_sleep());
                overlay::hide_recording_overlay(&app);
                crate::meeting_pipeline::stop_rolling_and_run_after_stop(&app, meeting_id);
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
                mic_only: false,
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

    #[test]
    fn pause_and_resume_freeze_elapsed_and_stop_from_paused_finalises() {
        let (controller, log) = controller_with_fake();
        controller.open_consent();
        let started = controller.confirm_start().unwrap();
        assert_eq!(started.state, MeetingState::Recording);
        assert!(!started.paused);

        let paused = controller.pause();
        assert_eq!(paused.state, MeetingState::Paused);
        assert!(paused.paused);
        assert!(controller.pill_visible());
        let frozen_elapsed = paused.elapsed_secs;
        assert_eq!(controller.status().elapsed_secs, frozen_elapsed);

        let resumed = controller.resume();
        assert_eq!(resumed.state, MeetingState::Recording);
        assert!(!resumed.paused);
        assert_eq!(log.0.starts.lock().unwrap().len(), 1);

        let paused_again = controller.pause();
        assert_eq!(paused_again.state, MeetingState::Paused);
        let stopped = controller.stop();
        assert_eq!(stopped.state, MeetingState::Idle);
        assert!(!stopped.paused);
        assert!(!controller.pill_visible());
        assert_eq!(log.0.stops.lock().unwrap().len(), 1);
    }

    #[test]
    fn append_consent_gate_requires_fresh_single_use_token_and_refuses_while_active() {
        let (controller, _log) = controller_with_fake();
        let token = controller.request_append_consent(42).unwrap();
        assert!(controller.consume_append_consent(42, token + 1).is_err());
        assert!(controller.consume_append_consent(99, token).is_err());
        assert!(controller.consume_append_consent(42, token).is_ok());
        // Single-use: replaying the same token must fail.
        assert!(controller.consume_append_consent(42, token).is_err());

        // Refuses while a meeting is active or paused.
        controller.open_consent();
        controller.confirm_start().unwrap();
        assert!(controller.request_append_consent(42).is_err());
        controller.pause();
        assert!(controller.request_append_consent(42).is_err());
    }

    #[test]
    fn attendees_are_exposed_on_meeting_status_and_cleared_on_next_consent() {
        let (controller, _log) = controller_with_fake();
        controller.open_consent();
        controller.confirm_start().unwrap();
        let names = vec![
            "Sam".to_string(),
            "Dana".to_string(),
            "Raj".to_string(),
            "Mei".to_string(),
        ];
        controller.set_attendees(names.clone());
        assert_eq!(controller.status().attendees, names);

        let stopped = controller.stop();
        assert_eq!(stopped.attendees, names);

        let next = controller.open_consent();
        assert!(next.attendees.is_empty());
    }

    #[test]
    fn ac_b19g_mic_only_default_refuses_without_calling_start() {
        let (controller, log) = controller_with_fake();

        // FakeRecorders has no start_mic_only override: default must refuse without calling start().
        let err = controller.confirm_start_mic_only();
        assert_eq!(err, Err(MeetingRefusal::SystemAudioUnavailable));
        assert!(
            log.0.starts.lock().unwrap().is_empty(),
            "mic-only default refusal must not call start(); starts={:?}",
            *log.0.starts.lock().unwrap()
        );
        assert_ne!(controller.state(), MeetingState::Recording);

        // A normal confirm_start() after the refusal still succeeds and starts once.
        let started = controller.confirm_start();
        assert!(
            started.is_ok(),
            "confirm_start() after mic-only refusal must succeed, got {started:?}"
        );
        assert_eq!(log.0.starts.lock().unwrap().len(), 1);
    }

    #[derive(Clone, Default)]
    struct RefusingStartFakeRecorders {
        mic_only_starts: Arc<Mutex<usize>>,
    }

    impl MeetingRecorders for RefusingStartFakeRecorders {
        fn start(&self) -> Result<StartedMeeting, MeetingRefusal> {
            Err(MeetingRefusal::SystemAudioTapFailed {
                cause: SystemAudioFailureCause::Permission,
                call: "create_process_tap".to_string(),
                code: "!obj (560947818)".to_string(),
            })
        }

        fn start_mic_only(&self) -> Result<StartedMeeting, MeetingRefusal> {
            let mut count = self.mic_only_starts.lock().unwrap();
            *count += 1;
            Ok(StartedMeeting {
                id: *count as i64,
                mic_name: Some("Built-in".to_string()),
                warnings: Vec::new(),
                mic_only: true,
            })
        }
    }

    #[test]
    fn ac_b19i_refused_start_keeps_consent_open_for_mic_only_or_cancel() {
        // Run 1: open_consent -> refused confirm_start keeps ConsentOpen with 0 mic-only starts ->
        // confirm_start_mic_only succeeds, enters Recording with mic_only=true, count == 1.
        let fake1 = RefusingStartFakeRecorders::default();
        let ctrl1 = MeetingController::new(Box::new(fake1.clone()));
        assert_eq!(ctrl1.open_consent().state, MeetingState::ConsentOpen);

        let refusal = ctrl1.confirm_start();
        assert_eq!(
            refusal,
            Err(MeetingRefusal::SystemAudioTapFailed {
                cause: SystemAudioFailureCause::Permission,
                call: "create_process_tap".to_string(),
                code: "!obj (560947818)".to_string(),
            })
        );
        assert_eq!(ctrl1.state(), MeetingState::ConsentOpen);
        assert_eq!(*fake1.mic_only_starts.lock().unwrap(), 0);

        let started = ctrl1
            .confirm_start_mic_only()
            .expect("confirm_start_mic_only must succeed from ConsentOpen");
        assert_eq!(started.state, MeetingState::Recording);
        assert!(started.mic_only);
        assert_eq!(ctrl1.state(), MeetingState::Recording);
        assert_eq!(*fake1.mic_only_starts.lock().unwrap(), 1);

        // Run 2: open_consent -> refused confirm_start -> cancel_consent returns Idle with no meeting_id.
        let fake2 = RefusingStartFakeRecorders::default();
        let ctrl2 = MeetingController::new(Box::new(fake2.clone()));
        assert_eq!(ctrl2.open_consent().state, MeetingState::ConsentOpen);
        assert!(ctrl2.confirm_start().is_err());
        assert_eq!(ctrl2.state(), MeetingState::ConsentOpen);

        let cancelled = ctrl2.cancel_consent();
        assert_eq!(cancelled.state, MeetingState::Idle);
        assert_eq!(cancelled.meeting_id, None);
        assert_eq!(ctrl2.state(), MeetingState::Idle);
        assert_eq!(*fake2.mic_only_starts.lock().unwrap(), 0);
    }

    /// T-mc6 (M-c4): start and mic-only start refuse with `MeetingsOff` (`meetings_off`)
    /// when the switch is off, without starting capture or writing a row, and proceed
    /// normally when the switch is on.
    #[test]
    fn t_mc6_start_refuses_with_meetings_off_when_switch_is_off() {
        assert_eq!(
            serde_json::to_value(MeetingRefusal::MeetingsOff).unwrap(),
            serde_json::json!("meetings_off")
        );

        let (controller, log) = controller_with_fake();
        assert_eq!(
            confirm_start_if_visible(&controller, false, false),
            Err(MeetingRefusal::MeetingsOff)
        );
        assert_eq!(
            confirm_start_if_visible(&controller, false, true),
            Err(MeetingRefusal::MeetingsOff)
        );
        assert!(
            log.0.starts.lock().unwrap().is_empty(),
            "refused start must count zero capture starts"
        );
        assert_eq!(controller.state(), MeetingState::Idle);
        assert_eq!(controller.status().meeting_id, None);

        // With the switch on, full start proceeds normally.
        let started = confirm_start_if_visible(&controller, true, false)
            .expect("start must succeed when Meetings is visible");
        assert_eq!(started.state, MeetingState::Recording);
        assert_eq!(log.0.starts.lock().unwrap().len(), 1);

        // Turning the switch off while already recording leaves the active meeting untouched.
        assert_eq!(
            confirm_start_if_visible(&controller, false, false),
            Err(MeetingRefusal::MeetingsOff)
        );
        assert_eq!(controller.state(), MeetingState::Recording);

        // Mic-only start also refuses when off and proceeds when on.
        let fake_mic = RefusingStartFakeRecorders::default();
        let ctrl_mic = MeetingController::new(Box::new(fake_mic.clone()));
        assert_eq!(
            confirm_start_if_visible(&ctrl_mic, false, true),
            Err(MeetingRefusal::MeetingsOff)
        );
        assert_eq!(*fake_mic.mic_only_starts.lock().unwrap(), 0);
        let started_mic = confirm_start_if_visible(&ctrl_mic, true, true)
            .expect("mic-only start must succeed when Meetings is visible");
        assert_eq!(started_mic.state, MeetingState::Recording);
        assert!(started_mic.mic_only);
        assert_eq!(*fake_mic.mic_only_starts.lock().unwrap(), 1);
    }

    #[test]
    fn pending_template_id_attaches_once_and_clears_on_cancel_or_default_open() {
        let controller = MeetingController::default();

        controller.open_consent_with_template(Some(17));
        assert_eq!(controller.take_pending_template_id(), Some(17));
        assert_eq!(controller.take_pending_template_id(), None);

        controller.cancel_consent();
        controller.open_consent_with_template(Some(23));
        controller.cancel_consent();
        assert_eq!(controller.take_pending_template_id(), None);

        controller.open_consent_with_template(Some(29));
        controller.open_consent();
        assert_eq!(controller.take_pending_template_id(), None);
    }
}
