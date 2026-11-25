//! Cohesive Tauri command handlers wiring v0.2.2 UI views to local SQLite stores and settings.

use crate::first_run_and_updates::{
    evaluate_accessibility_status, evaluate_update_notice_and_advance as advance_update_notice,
    AccessibilityOnboardingState, UpdateNoticeState,
};
use crate::managers::model::ModelManager;
use crate::settings::{self, AppSettings, SavedBlock};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager};

#[derive(Clone, Debug, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct HomeHealthStatusView {
    pub stale_accessibility: bool,
    pub shortcut_unset: bool,
    pub shortcut_failure_cause: Option<String>,
    pub typing_off: bool,
    pub cleanup_enabled_without_local_model: bool,
    pub cleanup_notice_already_shown: bool,
    pub update_ready_version: Option<String>,
    pub rolled_back_from_version: Option<String>,
}

#[tauri::command]
#[specta::specta]
pub fn update_saved_blocks(app: AppHandle, blocks: Vec<SavedBlock>) -> Result<(), String> {
    let mut s = settings::get_settings(&app);
    s.saved_blocks = blocks;
    settings::write_settings(&app, s);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn set_read_focused_text_enabled(app: AppHandle, enabled: bool) -> Result<(), String> {
    settings::update_settings(&app, |s| {
        s.read_focused_text_enabled = enabled;
        if !enabled {
            s.read_focused_window_context_enabled = false;
        }
    });
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn set_read_focused_window_context_enabled(
    app: AppHandle,
    enabled: bool,
) -> Result<(), String> {
    settings::update_settings(&app, |s| {
        s.read_focused_window_context_enabled = enabled && s.read_focused_text_enabled;
    });
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn remember_spelling_fix(app: AppHandle, wrong: String, right: String) -> Result<(), String> {
    let mut s = settings::get_settings(&app);
    if crate::fix_offer::remember_spelling_fix_into_settings(
        &mut s.custom_words,
        &mut s.corrections,
        &wrong,
        &right,
    ) {
        settings::write_settings(&app, s);
    }
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_offer_remember_fix_setting(app: AppHandle, enabled: bool) -> Result<(), String> {
    settings::update_settings(&app, |s| {
        s.offer_remember_fix_enabled = enabled;
    });
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_repo_words_setting(app: AppHandle, enabled: bool) -> Result<(), String> {
    settings::update_settings(&app, |s| {
        s.repo_words_enabled = enabled;
    });
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_filler_word_removal_setting(app: AppHandle, enabled: bool) -> Result<(), String> {
    settings::update_settings(&app, |s| {
        s.filler_word_removal_enabled = enabled;
    });
    Ok(())
}

pub(crate) fn apply_agents_can_ask_to_bridge<R: tauri::Runtime>(
    app: &AppHandle<R>,
    effective: bool,
) {
    if let Some(bridge_state) =
        app.try_state::<Arc<std::sync::Mutex<crate::agent_ask_bridge::AgentAskBridgeState>>>()
    {
        let next_ev = {
            let mut guard = bridge_state.lock().unwrap_or_else(|e| e.into_inner());
            guard.set_enabled(effective);
            guard.current_overlay_event()
        };
        if !effective {
            if let Some(ov) = app.get_webview_window("recording_overlay") {
                let _ = ov.emit("agent-ask-question", next_ev);
            }
        }
    }
}

#[tauri::command]
#[specta::specta]
pub fn change_agents_can_ask_setting(app: AppHandle, enabled: bool) -> Result<(), String> {
    let effective =
        settings::update_settings(&app, |s| settings::apply_agents_can_ask_setting(s, enabled));
    settings::save_settings_store(&app);
    apply_agents_can_ask_to_bridge(&app, effective);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn update_blocked_sites(
    app: AppHandle,
    sites: Vec<crate::app_profiles::BlockedSite>,
) -> Result<(), String> {
    let normalized = crate::app_profiles::normalize_blocked_sites(sites);
    settings::update_settings(&app, |s| {
        s.blocked_sites = normalized;
    });
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn dismiss_fix_offer(app: AppHandle, id: String) -> Result<(), String> {
    if let Some(queue) = app.try_state::<std::sync::Mutex<crate::fix_offer::FixOfferQueue>>() {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let next = queue
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .dismiss(&id, now_ms);
        if let Some(payload) = next {
            let _ = app.emit("fix-offer", payload);
        }
    }
    Ok(())
}

pub fn compute_accessibility_stale(trusted: bool, probe_ok: bool) -> bool {
    matches!(
        evaluate_accessibility_status(Some(trusted), Some(probe_ok), None),
        AccessibilityOnboardingState::StaleEntry { .. }
    )
}

#[cfg(target_os = "macos")]
fn macos_accessibility_is_trusted() -> bool {
    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXIsProcessTrusted() -> u8;
    }
    unsafe { AXIsProcessTrusted() != 0 }
}

#[tauri::command]
#[specta::specta]
pub fn check_accessibility_stale(app: AppHandle) -> bool {
    #[cfg(target_os = "macos")]
    {
        let trusted = macos_accessibility_is_trusted();
        if !trusted {
            return false;
        }
        let probe_ok = app.try_state::<crate::input::EnigoState>().is_some()
            || crate::input::EnigoState::new().is_ok();
        compute_accessibility_stale(trusted, probe_ok)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = app;
        false
    }
}

pub fn compute_home_health_status(
    s: &AppSettings,
    _current_version: &str,
    stale_accessibility: bool,
    has_local_cleanup_model: bool,
    update_ready_version: Option<String>,
    registration: &crate::shortcut::Registration,
) -> HomeHealthStatusView {
    let binding_empty = s
        .bindings
        .get("transcribe")
        .map(|b| b.current_binding.trim().is_empty())
        .unwrap_or(true);
    // Part B (R10): `shortcut_unset` is true when the binding is empty, or when
    // shortcut registration failed for any cause (`Taken`, `Permission`,
    // `Rejected`, `Other`). `NotTried` and `Ok` use the binding rule only so
    // Home never flashes notice b before init runs.
    let registration_failed = matches!(registration, crate::shortcut::Registration::Failed { .. });
    let shortcut_unset = binding_empty || registration_failed;
    let shortcut_failure_cause = registration.failure_cause_str().map(str::to_string);
    let typing_off = matches!(s.paste_method, settings::PasteMethod::None);
    let cleanup_enabled_without_local_model = s.post_process_enabled
        && s.post_process_provider_id == settings::LOCAL_LLM_PROVIDER_ID
        && !has_local_cleanup_model;
    // Plan A rule: went-back card is unused until R2 (rendered from 36b fixture only).
    let rolled_back_from_version = None;
    HomeHealthStatusView {
        stale_accessibility: stale_accessibility && !typing_off,
        shortcut_unset,
        shortcut_failure_cause,
        typing_off,
        cleanup_enabled_without_local_model,
        cleanup_notice_already_shown: false,
        update_ready_version,
        rolled_back_from_version,
    }
}

#[tauri::command]
#[specta::specta]
pub fn home_health_status(app: AppHandle) -> Result<HomeHealthStatusView, String> {
    let s = settings::get_settings(&app);
    let stale_accessibility = check_accessibility_stale(app.clone());
    let has_local_cleanup_model = if s.post_process_provider_id == settings::LOCAL_LLM_PROVIDER_ID {
        let model_id = s
            .post_process_models
            .get(settings::LOCAL_LLM_PROVIDER_ID)
            .map(String::as_str)
            .unwrap_or("");
        app.try_state::<Arc<ModelManager>>()
            .map(|mm| !model_id.is_empty() && mm.get_model_path(model_id).is_ok())
            .unwrap_or(false)
    } else {
        true
    };
    let current_version = app.package_info().version.to_string();
    let registration = crate::shortcut::current_registration(&app);
    Ok(compute_home_health_status(
        &s,
        &current_version,
        stale_accessibility,
        has_local_cleanup_model,
        None,
        &registration,
    ))
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct TryItTake {
    pub dropped: Option<String>,
}

/// Inspects a live "Try it" dictation take (`raw` vs `final_text`) using the
/// same retraction cue list as `first_run_and_updates::evaluate_try_it_sample`
/// ("scratch that", "i mean", "no wait"). Returns the dropped prefix + cue
/// (e.g. `"Monday, scratch that"`) only when `final_text` no longer contains
/// the retracted words; returns `None` if no cue matched or the words stayed.
#[tauri::command]
#[specta::specta]
pub fn evaluate_try_it_take(raw: String, final_text: String) -> TryItTake {
    fn is_word_char(c: char) -> bool {
        c.is_alphanumeric() || c == '\'' || c == '_'
    }
    let lower = raw.to_ascii_lowercase();
    let mut best_match: Option<(usize, usize)> = None;
    for cue in ["scratch that", "i mean", "no wait"] {
        for (idx, _) in lower.match_indices(cue) {
            let end = idx + cue.len();
            if !raw.is_char_boundary(idx) || !raw.is_char_boundary(end) {
                continue;
            }
            let start_ok = idx == 0
                || raw[..idx]
                    .chars()
                    .next_back()
                    .is_none_or(|c| !is_word_char(c));
            let end_ok =
                end == raw.len() || raw[end..].chars().next().is_none_or(|c| !is_word_char(c));
            if start_ok && end_ok && best_match.is_none_or(|(best_idx, _)| idx > best_idx) {
                best_match = Some((idx, end));
            }
        }
    }
    let Some((idx, end)) = best_match else {
        return TryItTake { dropped: None };
    };
    let dropped_segment = raw[..end].trim().trim_end_matches(',').trim();
    if dropped_segment.is_empty() {
        return TryItTake { dropped: None };
    }
    let before_cue = raw[..idx]
        .trim()
        .trim_end_matches(|c: char| c == ',' || c == '.' || c.is_whitespace())
        .to_ascii_lowercase();
    let final_lower = final_text.to_ascii_lowercase();
    if final_lower.contains(&dropped_segment.to_ascii_lowercase())
        || (!before_cue.is_empty() && final_lower.contains(&before_cue))
    {
        return TryItTake { dropped: None };
    }
    TryItTake {
        dropped: Some(dropped_segment.to_string()),
    }
}

#[tauri::command]
#[specta::specta]
pub fn evaluate_update_notice_and_advance(app: AppHandle) -> Result<UpdateNoticeState, String> {
    let mut s = settings::get_settings(&app);
    let before = s.whats_new_last_seen_version.clone();
    let current_version = app.package_info().version.to_string();
    let notice = advance_update_notice(&mut s, &current_version);
    if s.whats_new_last_seen_version != before {
        settings::write_settings(&app, s);
    }
    Ok(notice)
}

#[cfg(feature = "meetings")]
pub mod meetings_ext {
    use super::*;
    use crate::managers::meetings::MeetingsStore;
    use crate::meeting_controller::MeetingStatus;
    use crate::meeting_notes_runner::NotesGenerator;
    use crate::meeting_notes_v42::{catch_up_window_v2, CatchUpOutcome};
    use crate::meeting_pipeline::LeftOutRecordDto;
    use crate::meeting_retention::MeetingRetentionState;
    use crate::meeting_todos::{MeetingTodoStore, TodoStatus};
    use chrono::{DateTime, Local, Utc};

    struct CatchUpExtractiveOnly;

    impl NotesGenerator for CatchUpExtractiveOnly {
        fn generate(&mut self, _prompt: &str) -> anyhow::Result<String> {
            Err(anyhow::anyhow!("extractive fallback"))
        }
    }

    #[derive(Clone, Debug, Serialize, Deserialize, Type)]
    pub struct MeetingNoteLineDto {
        pub kind: String,
        pub text: String,
        pub owner: Option<String>,
        pub start_ms: Option<i64>,
        pub from_notepad: bool,
    }

    #[derive(Clone, Debug, Serialize, Deserialize, Type)]
    pub struct MeetingNotesSnapshotDto {
        pub meeting_id: i64,
        pub state: String,
        pub progress_label: Option<String>,
        pub progress_step: Option<u32>,
        pub progress_total: Option<u32>,
        pub elapsed_secs: u64,
        pub estimate_secs: Option<u64>,
        pub time_to_notes_secs: Option<u64>,
        pub failure_reason: Option<String>,
        pub notepad_text: String,
        pub lines: Vec<MeetingNoteLineDto>,
        pub left_out: Vec<LeftOutRecordDto>,
        pub oversized_count: usize,
    }

    #[derive(Clone, Debug, Serialize, Deserialize, Type)]
    pub struct TrashedMeetingDto {
        pub id: i64,
        pub title: String,
        pub deleted_at: i64,
    }

    #[derive(Clone, Debug, Serialize, Deserialize, Type)]
    pub struct AskCitationDto {
        pub label: String,
        pub start_ms: i64,
    }

    #[derive(Clone, Debug, Serialize, Deserialize, Type)]
    pub struct AskMeetingsDto {
        pub answer: String,
        pub citations: Vec<AskCitationDto>,
    }

    #[derive(Clone, Debug, Serialize, Deserialize, Type)]
    pub struct MeetingTodoDto {
        pub id: String,
        pub text: String,
        pub meeting_id: i64,
        pub meeting_title: String,
        pub meeting_date: String,
        pub start_ms: i64,
        pub done: bool,
        pub said_by_me: bool,
    }

    #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
    pub struct AgentReadAuditDto {
        pub id: String,
        pub client: String,
        pub kind: String,
        pub target_title: String,
        pub ts: i64,
    }

    #[derive(Clone, Debug, Serialize, Deserialize, Type)]
    pub struct AgentAccessSnapshotDto {
        pub enabled: bool,
        pub allow_live_meeting: bool,
        pub allow_past_meetings: bool,
        pub recent_reads: Vec<AgentReadAuditDto>,
    }

    pub const MCP_FALLBACK_COMMAND: &str = "coco-voice mcp";

    pub fn format_mcp_command_line(exe: Option<&std::path::Path>) -> String {
        let Some(raw) = exe.and_then(|p| p.to_str()) else {
            return MCP_FALLBACK_COMMAND.to_string();
        };
        if raw.is_empty() {
            return MCP_FALLBACK_COMMAND.to_string();
        }
        let escaped = raw.replace('\'', "'\\''");
        format!("'{escaped}' mcp")
    }

    pub fn map_agent_read_audit(
        index: usize,
        entry: crate::mcp_store::AgentReadEntry,
    ) -> AgentReadAuditDto {
        let kind =
            crate::mcp_store::audit_read_kind(&entry.tool, entry.kind.as_deref()).to_string();
        let target_title = match kind.as_str() {
            "list" => String::new(),
            "live" if entry.target == "none" => String::new(),
            _ => entry.target,
        };
        AgentReadAuditDto {
            id: format!("r-{}-{index}", entry.ts),
            client: entry.client,
            kind,
            target_title,
            ts: entry.ts,
        }
    }

    #[tauri::command]
    #[specta::specta]
    pub fn meeting_notes_snapshot(
        app: AppHandle,
        meeting_id: i64,
    ) -> Result<MeetingNotesSnapshotDto, String> {
        let store = MeetingsStore::open_app(&app).map_err(|e| e.to_string())?;
        let pipeline = app.try_state::<Arc<crate::meeting_pipeline::MeetingPipelineState>>();
        let snap = crate::meeting_pipeline::build_meeting_notes_snapshot(
            &store,
            pipeline.as_deref().map(Arc::as_ref),
            meeting_id,
        )
        .map_err(|e| e.to_string())?;
        Ok(MeetingNotesSnapshotDto {
            meeting_id: snap.meeting_id,
            state: snap.state,
            progress_label: snap.progress_label,
            progress_step: snap.progress_step,
            progress_total: snap.progress_total,
            elapsed_secs: snap.elapsed_secs,
            estimate_secs: snap.estimate_secs,
            time_to_notes_secs: snap.time_to_notes_secs,
            failure_reason: snap.failure_reason,
            notepad_text: snap.notepad_text,
            lines: snap
                .lines
                .into_iter()
                .map(|l| MeetingNoteLineDto {
                    kind: l.kind,
                    text: l.text,
                    owner: l.owner,
                    start_ms: l.start_ms,
                    from_notepad: l.from_notepad,
                })
                .collect(),
            left_out: snap.left_out,
            oversized_count: snap.oversized_count,
        })
    }

    #[tauri::command]
    #[specta::specta]
    pub fn save_meeting_notepad(
        app: AppHandle,
        meeting_id: i64,
        text: String,
    ) -> Result<(), String> {
        let store = MeetingsStore::open_app(&app).map_err(|e| e.to_string())?;
        let elapsed_ms = app
            .try_state::<Arc<crate::meeting_controller::MeetingController>>()
            .map(|c| (c.status().elapsed_secs as i64).saturating_mul(1_000))
            .unwrap_or(0);
        crate::meeting_pipeline::save_meeting_notepad_points(&store, meeting_id, &text, elapsed_ms)
            .map_err(|e| e.to_string())
    }

    #[tauri::command]
    #[specta::specta]
    pub fn retry_meeting_notes(
        app: AppHandle,
        meeting_id: i64,
    ) -> Result<MeetingNotesSnapshotDto, String> {
        crate::meeting_pipeline::stop_rolling_and_run_after_stop(&app, meeting_id);
        meeting_notes_snapshot(app, meeting_id)
    }

    #[tauri::command]
    #[specta::specta]
    pub fn catch_up_meeting(
        app: AppHandle,
        meeting_id: i64,
        minutes: Option<u32>,
    ) -> Result<Vec<MeetingNoteLineDto>, String> {
        let store = MeetingsStore::open_app(&app).map_err(|e| e.to_string())?;
        let segments = store.segments(meeting_id).map_err(|e| e.to_string())?;
        let latest_end_ms = segments.iter().map(|s| s.end_ms).max().unwrap_or(0);
        let clamped_mins = match minutes.unwrap_or(5) {
            10 => 10,
            15 => 15,
            _ => 5,
        };
        let window_ms = (clamped_mins as i64) * 60_000;
        let outcome = catch_up_window_v2::<CatchUpExtractiveOnly>(
            &store,
            meeting_id,
            latest_end_ms,
            window_ms,
            None,
        )
        .map_err(|e| e.to_string())?;
        match outcome {
            CatchUpOutcome::NotEnoughSaid => Ok(Vec::new()),
            CatchUpOutcome::Lines { lines } => Ok(lines
                .into_iter()
                .map(|line| {
                    let start_ms = line
                        .citations
                        .first()
                        .and_then(|cid| segments.iter().find(|s| s.id == *cid).map(|s| s.start_ms));
                    MeetingNoteLineDto {
                        kind: line.kind,
                        text: line.text,
                        owner: line.owner,
                        start_ms,
                        from_notepad: false,
                    }
                })
                .collect()),
        }
    }

    #[tauri::command]
    #[specta::specta]
    pub fn pause_meeting(app: AppHandle) -> Result<MeetingStatus, String> {
        Ok(crate::meeting_controller::pause_meeting_cmd(app))
    }

    #[tauri::command]
    #[specta::specta]
    pub fn resume_meeting(app: AppHandle) -> Result<MeetingStatus, String> {
        Ok(crate::meeting_controller::resume_meeting_cmd(app))
    }

    #[tauri::command]
    #[specta::specta]
    pub fn append_meeting_audio(
        app: AppHandle,
        meeting_id: i64,
        token: Option<u64>,
    ) -> Result<MeetingStatus, String> {
        let consent_token =
            token.ok_or_else(|| "fresh append consent token is required".to_string())?;
        crate::meeting_controller::confirm_append_meeting_audio(app, meeting_id, consent_token)
    }

    #[tauri::command]
    #[specta::specta]
    pub fn list_trashed_meetings(app: AppHandle) -> Result<Vec<TrashedMeetingDto>, String> {
        let rows = crate::meeting_extras_commands::list_trashed_meetings_v2(app)?;
        Ok(rows
            .into_iter()
            .map(|r| TrashedMeetingDto {
                id: r.id,
                title: r.title,
                deleted_at: r.deleted_at,
            })
            .collect())
    }

    #[tauri::command]
    #[specta::specta]
    pub fn restore_trashed_meeting(app: AppHandle, meeting_id: i64) -> Result<(), String> {
        crate::meeting_extras_commands::restore_trashed_meeting_v2(app, meeting_id)
    }

    #[tauri::command]
    #[specta::specta]
    pub fn delete_transcript_span(
        app: AppHandle,
        meeting_id: i64,
        start_ms: i64,
        end_ms: i64,
    ) -> Result<(), String> {
        crate::meeting_extras_commands::delete_transcript_span_secure(
            app, meeting_id, start_ms, end_ms,
        )
        .map(|_| ())
    }

    #[tauri::command]
    #[specta::specta]
    pub fn change_transcript_delete_days_setting(
        app: AppHandle,
        days: Option<u32>,
    ) -> Result<MeetingRetentionState, String> {
        let mut s = settings::get_settings(&app);
        s.transcript_delete_after_days = days;
        settings::write_settings(&app, s);
        Ok(crate::meeting_retention::meeting_retention_state(app))
    }

    #[tauri::command]
    #[specta::specta]
    pub fn import_meeting_notes(
        app: AppHandle,
        paths: Vec<String>,
        folder: String,
    ) -> Result<usize, String> {
        let folder_opt = if folder.trim().is_empty() {
            None
        } else {
            Some(folder)
        };
        let summary =
            crate::meeting_extras_commands::import_meeting_notes_batch(app, paths, folder_opt)?;
        Ok(summary.imported_count)
    }

    // `async`: Ask may run the local model or wait on SQLite; never block the main thread.
    #[tauri::command(async)]
    #[specta::specta]
    pub fn ask_meetings(
        app: AppHandle,
        question: String,
        scope: String,
        meeting_id: Option<i64>,
    ) -> Result<AskMeetingsDto, String> {
        let ans = crate::meeting_extras_commands::ask_meetings_v2(
            app, question, scope, meeting_id, None, None, None,
        )?;
        Ok(AskMeetingsDto {
            answer: ans.answer,
            citations: ans
                .citations
                .into_iter()
                .map(|c| AskCitationDto {
                    label: c.label,
                    start_ms: c.at_ms,
                })
                .collect(),
        })
    }

    #[tauri::command]
    #[specta::specta]
    pub fn list_meeting_todos(app: AppHandle) -> Result<Vec<MeetingTodoDto>, String> {
        let todo_store = MeetingTodoStore::open_app(&app).map_err(|e| e.to_string())?;
        let items = todo_store
            .sync_then_list_last_14_days(Utc::now().timestamp())
            .map_err(|e| e.to_string())?;
        Ok(items
            .into_iter()
            .map(|t| {
                let meeting_date = DateTime::from_timestamp(t.origin.meeting_started_at, 0)
                    .map(|dt| dt.with_timezone(&Local).format("%b %-d").to_string())
                    .unwrap_or_else(|| t.origin.timestamp.clone());
                MeetingTodoDto {
                    id: t.id.to_string(),
                    text: t.text,
                    meeting_id: t.origin.meeting_id,
                    meeting_title: t.origin.meeting_title,
                    meeting_date,
                    start_ms: t.origin.start_ms,
                    done: t.status == TodoStatus::Done,
                    said_by_me: t.said_by_me,
                }
            })
            .collect())
    }

    #[tauri::command]
    #[specta::specta]
    pub fn complete_meeting_todo(
        app: AppHandle,
        todo_id: String,
        done: bool,
    ) -> Result<(), String> {
        let id_num: i64 = todo_id
            .parse()
            .map_err(|e: std::num::ParseIntError| e.to_string())?;
        let todo_store = MeetingTodoStore::open_app(&app).map_err(|e| e.to_string())?;
        let st = if done {
            TodoStatus::Done
        } else {
            TodoStatus::Open
        };
        todo_store.set_status(id_num, st).map_err(|e| e.to_string())
    }

    #[tauri::command]
    #[specta::specta]
    pub fn mcp_command_line() -> String {
        format_mcp_command_line(std::env::current_exe().ok().as_deref())
    }

    #[tauri::command]
    #[specta::specta]
    pub fn agent_access_state(app: AppHandle) -> Result<AgentAccessSnapshotDto, String> {
        let s = settings::get_settings(&app);
        let data_dir = crate::portable::app_data_dir(&app).map_err(|e| e.to_string())?;
        let reads = crate::mcp_store::recent_reads(&data_dir)
            .into_iter()
            .take(5)
            .enumerate()
            .map(|(i, r)| map_agent_read_audit(i, r))
            .collect();
        Ok(AgentAccessSnapshotDto {
            enabled: s.agent_access_enabled,
            allow_live_meeting: s.agent_access_live,
            allow_past_meetings: s.agent_access_past,
            recent_reads: reads,
        })
    }

    #[tauri::command]
    #[specta::specta]
    pub fn set_agent_access_enabled(
        app: AppHandle,
        enabled: bool,
    ) -> Result<AgentAccessSnapshotDto, String> {
        let mut s = settings::get_settings(&app);
        s.agent_access_enabled = enabled;
        settings::write_settings(&app, s);
        agent_access_state(app)
    }

    #[tauri::command]
    #[specta::specta]
    pub fn set_agent_access_scope(
        app: AppHandle,
        allow_live_meeting: bool,
        allow_past_meetings: bool,
    ) -> Result<AgentAccessSnapshotDto, String> {
        let mut s = settings::get_settings(&app);
        s.agent_access_live = allow_live_meeting;
        s.agent_access_past = allow_past_meetings;
        settings::write_settings(&app, s);
        agent_access_state(app)
    }

    #[tauri::command]
    #[specta::specta]
    pub fn get_meeting_custom_words(app: AppHandle) -> Result<Vec<String>, String> {
        Ok(settings::get_settings(&app).meeting_custom_words)
    }

    #[tauri::command]
    #[specta::specta]
    pub fn set_meeting_custom_words(
        app: AppHandle,
        words: Vec<String>,
    ) -> Result<Vec<String>, String> {
        let mut s = settings::get_settings(&app);
        s.meeting_custom_words = words.clone();
        settings::write_settings(&app, s);
        Ok(words)
    }
}

#[cfg(test)]
mod checkpoint_a_tests {
    use super::*;
    use crate::settings::{get_default_settings, PasteMethod, LOCAL_LLM_PROVIDER_ID};

    #[test]
    fn accessibility_stale_detects_trusted_with_failed_probe_only() {
        assert!(!compute_accessibility_stale(false, false));
        assert!(!compute_accessibility_stale(false, true));
        assert!(compute_accessibility_stale(true, false));
        assert!(!compute_accessibility_stale(true, true));
    }

    #[test]
    fn home_health_status_computes_real_stale_ax_missing_model_and_keeps_rollback_none() {
        let mut s = get_default_settings();
        s.paste_method = PasteMethod::CtrlV;
        s.post_process_enabled = true;
        s.post_process_provider_id = LOCAL_LLM_PROVIDER_ID.to_string();
        // Even when stored last-seen version is newer than current, 0.20 never sets rolled_back_from_version.
        s.whats_new_last_seen_version = "0.21.0".to_string();

        let view = compute_home_health_status(
            &s,
            "0.20.0",
            true,
            false,
            Some("0.22.0".into()),
            &crate::shortcut::Registration::NotTried,
        );
        assert!(view.stale_accessibility);
        assert!(!view.typing_off);
        assert!(view.cleanup_enabled_without_local_model);
        assert_eq!(view.update_ready_version.as_deref(), Some("0.22.0"));
        assert_eq!(view.rolled_back_from_version, None);

        // When typing is off, stale_accessibility is suppressed and typing_off is true.
        s.paste_method = PasteMethod::None;
        let view_typing_off = compute_home_health_status(
            &s,
            "0.21.0",
            true,
            true,
            None,
            &crate::shortcut::Registration::NotTried,
        );
        assert!(!view_typing_off.stale_accessibility);
        assert!(view_typing_off.typing_off);
        assert!(!view_typing_off.cleanup_enabled_without_local_model);
        assert_eq!(view_typing_off.rolled_back_from_version, None);
    }

    #[test]
    fn t_h1_home_health_shortcut_unset_follows_registration_part_a() {
        use crate::shortcut::{Registration, ShortcutFailureCause};

        let mut s = get_default_settings();
        if let Some(binding) = s.bindings.get_mut("transcribe") {
            binding.current_binding = "fn".to_string();
        }

        // Failed { Taken } with a set binding -> shortcut_unset true and cause "taken".
        let taken = compute_home_health_status(
            &s,
            "0.20.2",
            false,
            true,
            None,
            &Registration::Failed {
                cause: ShortcutFailureCause::Taken,
            },
        );
        assert!(taken.shortcut_unset);
        assert_eq!(taken.shortcut_failure_cause.as_deref(), Some("taken"));

        // Failed { Permission } -> shortcut_unset true in Part B (R10), cause "permission".
        let perm = compute_home_health_status(
            &s,
            "0.20.2",
            false,
            true,
            None,
            &Registration::Failed {
                cause: ShortcutFailureCause::Permission,
            },
        );
        assert!(perm.shortcut_unset);
        assert_eq!(perm.shortcut_failure_cause.as_deref(), Some("permission"));

        // Failed { Rejected } and { Other } -> shortcut_unset true in Part B (R10).
        let other = compute_home_health_status(
            &s,
            "0.20.2",
            false,
            true,
            None,
            &Registration::Failed {
                cause: ShortcutFailureCause::Other,
            },
        );
        assert!(other.shortcut_unset);
        assert_eq!(other.shortcut_failure_cause.as_deref(), Some("other"));

        // NotTried and Ok with a set binding -> shortcut_unset false, cause None.
        let not_tried =
            compute_home_health_status(&s, "0.20.2", false, true, None, &Registration::NotTried);
        assert!(!not_tried.shortcut_unset);
        assert_eq!(not_tried.shortcut_failure_cause, None);

        let ok = compute_home_health_status(&s, "0.20.2", false, true, None, &Registration::Ok);
        assert!(!ok.shortcut_unset);
        assert_eq!(ok.shortcut_failure_cause, None);

        // Empty-binding rule is unchanged for NotTried and Ok.
        if let Some(binding) = s.bindings.get_mut("transcribe") {
            binding.current_binding.clear();
        }
        assert!(
            compute_home_health_status(&s, "0.20.2", false, true, None, &Registration::NotTried)
                .shortcut_unset
        );
        assert!(
            compute_home_health_status(&s, "0.20.2", false, true, None, &Registration::Ok)
                .shortcut_unset
        );
    }

    #[test]
    fn evaluate_update_notice_advances_once_and_never_emits_went_back() {
        let mut s = get_default_settings();
        s.show_whats_new_on_update = true;
        s.whats_new_last_seen_version = "0.19.0".to_string();

        assert_eq!(
            advance_update_notice(&mut s, "0.20.0"),
            UpdateNoticeState::WhatsNew {
                version: "0.20.0".to_string(),
                previous_version: "0.19.0".to_string(),
            }
        );
        // Second call on same version returns None (shown once).
        assert_eq!(
            advance_update_notice(&mut s, "0.20.0"),
            UpdateNoticeState::None
        );

        // Downgrade stays silent (UpdateNoticeState::None) and advances stored version.
        s.whats_new_last_seen_version = "0.20.1".to_string();
        assert_eq!(
            advance_update_notice(&mut s, "0.20.0"),
            UpdateNoticeState::None
        );
        assert_eq!(s.whats_new_last_seen_version, "0.20.0");
    }

    #[test]
    fn evaluate_try_it_take_reports_dropped_words_for_all_cues_and_hides_when_kept_or_no_cue() {
        assert_eq!(
            evaluate_try_it_take(
                "Monday, scratch that, Tuesday".to_string(),
                "Tuesday".to_string()
            ),
            TryItTake {
                dropped: Some("Monday, scratch that".to_string())
            }
        );
        assert_eq!(
            evaluate_try_it_take(
                "Let's meet at three, no wait, four".to_string(),
                "Four".to_string()
            ),
            TryItTake {
                dropped: Some("Let's meet at three, no wait".to_string())
            }
        );
        assert_eq!(
            evaluate_try_it_take("Send to Sam, I mean Maya".to_string(), "Maya".to_string()),
            TryItTake {
                dropped: Some("Send to Sam, I mean".to_string())
            }
        );
        // Clean-up off / words kept: must return None so UI hides the dropped sentence.
        assert_eq!(
            evaluate_try_it_take(
                "Monday, scratch that, Tuesday".to_string(),
                "Monday, scratch that, Tuesday".to_string()
            ),
            TryItTake { dropped: None }
        );
        // No retraction cue: returns None.
        assert_eq!(
            evaluate_try_it_take("Hello Tuesday".to_string(), "Hello Tuesday".to_string()),
            TryItTake { dropped: None }
        );
    }
}

#[cfg(all(test, feature = "meetings"))]
mod plan_c_agent_access_tests {
    use super::meetings_ext::{
        format_mcp_command_line, map_agent_read_audit, MCP_FALLBACK_COMMAND,
    };
    use crate::mcp_store::AgentReadEntry;
    use std::path::Path;

    #[test]
    fn format_mcp_command_line_quotes_binary_paths_and_falls_back() {
        assert_eq!(
            format_mcp_command_line(Some(Path::new(
                "/Applications/Coco Voice.app/Contents/MacOS/coco-voice"
            ))),
            "'/Applications/Coco Voice.app/Contents/MacOS/coco-voice' mcp"
        );
        assert_eq!(
            format_mcp_command_line(Some(Path::new("/tmp/$HOME/coco-voice"))),
            "'/tmp/$HOME/coco-voice' mcp"
        );
        assert_eq!(
            format_mcp_command_line(Some(Path::new("/tmp/$(id)/coco-voice"))),
            "'/tmp/$(id)/coco-voice' mcp"
        );
        assert_eq!(
            format_mcp_command_line(Some(Path::new("/tmp/`id`/coco-voice"))),
            "'/tmp/`id`/coco-voice' mcp"
        );
        assert_eq!(
            format_mcp_command_line(Some(Path::new("/tmp/o'brien/coco-voice"))),
            "'/tmp/o'\\''brien/coco-voice' mcp"
        );
        assert_eq!(
            format_mcp_command_line(Some(Path::new(r#"/tmp/a "quoted"/coco-voice"#))),
            r#"'/tmp/a "quoted"/coco-voice' mcp"#
        );
        assert_eq!(
            format_mcp_command_line(Some(Path::new(r"/tmp/a\b/coco-voice"))),
            r"'/tmp/a\b/coco-voice' mcp"
        );
        assert_eq!(
            format_mcp_command_line(Some(Path::new("/tmp/hi!/coco-voice"))),
            "'/tmp/hi!/coco-voice' mcp"
        );
        assert_eq!(
            format_mcp_command_line(Some(Path::new(" /tmp/coco-voice "))),
            "' /tmp/coco-voice ' mcp"
        );
        assert_eq!(
            format_mcp_command_line(Some(Path::new("/tmp/a\nb/coco-voice"))),
            "'/tmp/a\nb/coco-voice' mcp"
        );
        assert_eq!(format_mcp_command_line(None), MCP_FALLBACK_COMMAND);
        assert_eq!(
            format_mcp_command_line(Some(Path::new(""))),
            MCP_FALLBACK_COMMAND
        );
        #[cfg(unix)]
        {
            use std::ffi::OsStr;
            use std::os::unix::ffi::OsStrExt;
            let non_utf8 = OsStr::from_bytes(b"/tmp/\xff/coco-voice");
            assert_eq!(
                format_mcp_command_line(Some(Path::new(non_utf8))),
                MCP_FALLBACK_COMMAND
            );
        }
    }

    #[test]
    fn map_agent_read_audit_covers_all_four_kinds() {
        let notes = map_agent_read_audit(
            0,
            AgentReadEntry {
                ts: 1_700_000_100,
                client: "claude-code".into(),
                tool: "get_meeting".into(),
                target: "Product sync".into(),
                kind: Some("notes".into()),
            },
        );
        assert_eq!(notes.client, "claude-code");
        assert_eq!(notes.kind, "notes");
        assert_eq!(notes.target_title, "Product sync");
        assert_eq!(notes.ts, 1_700_000_100);

        let transcript = map_agent_read_audit(
            1,
            AgentReadEntry {
                ts: 1_700_000_200,
                client: "cursor".into(),
                tool: "get_meeting".into(),
                target: "Design crit".into(),
                kind: Some("transcript".into()),
            },
        );
        assert_eq!(transcript.kind, "transcript");
        assert_eq!(transcript.target_title, "Design crit");

        let live = map_agent_read_audit(
            2,
            AgentReadEntry {
                ts: 1_700_000_300,
                client: "codex".into(),
                tool: "live_meeting_transcript".into(),
                target: "Standup".into(),
                kind: None,
            },
        );
        assert_eq!(live.kind, "live");
        assert_eq!(live.target_title, "Standup");

        let list = map_agent_read_audit(
            3,
            AgentReadEntry {
                ts: 1_700_000_400,
                client: "claude-code".into(),
                tool: "list_meetings".into(),
                target: "meetings".into(),
                kind: None,
            },
        );
        assert_eq!(list.kind, "list");
        assert_eq!(list.target_title, "");
    }
}

#[cfg(test)]
mod agents_can_ask_tests {
    use super::*;
    use crate::agent_ask_bridge::{AgentAskBridgeState, ERR_QUESTIONS_OFF};
    use std::sync::Mutex;

    #[test]
    fn d3_d4_apply_agents_can_ask_to_bridge_flips_managed_state_without_restart() {
        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock app");
        let bridge = Arc::new(Mutex::new(AgentAskBridgeState::new(false, "fn")));
        app.manage(Arc::clone(&bridge));

        // Starts disabled: incoming question is rejected.
        assert_eq!(
            bridge
                .lock()
                .unwrap()
                .enqueue_at("Claude Code", "Ship?", 100)
                .unwrap_err(),
            ERR_QUESTIONS_OFF
        );

        // Flip ON without restarting the app: bridge immediately accepts questions.
        apply_agents_can_ask_to_bridge(app.handle(), true);
        let (rx1, ev1) = bridge
            .lock()
            .unwrap()
            .enqueue_at("Claude Code", "First question?", 200)
            .expect("bridge accepts after flip ON");
        assert_eq!(ev1.expect("active overlay event").agent_name, "Claude Code");
        let (rx2, _) = bridge
            .lock()
            .unwrap()
            .enqueue_at("Codex", "Queued question?", 250)
            .expect("second question queues");
        assert_eq!(bridge.lock().unwrap().waiting_count(), 1);

        // Flip OFF without restarting: both waiting callers fail with ERR_QUESTIONS_OFF and pill clears.
        apply_agents_can_ask_to_bridge(app.handle(), false);
        assert_eq!(rx1.recv().unwrap(), Err(ERR_QUESTIONS_OFF.to_string()));
        assert_eq!(rx2.recv().unwrap(), Err(ERR_QUESTIONS_OFF.to_string()));
        assert!(!bridge.lock().unwrap().has_active_question());
        assert_eq!(bridge.lock().unwrap().waiting_count(), 0);
        assert!(bridge.lock().unwrap().current_overlay_event().is_none());
    }
}
