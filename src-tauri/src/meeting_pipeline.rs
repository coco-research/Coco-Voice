//! Local-only meeting notes pipeline: rolling pieces during a meeting,
//! after-Stop note generation, title writing, audio retention cleanup,
//! Markdown folder sync, startup recovery of interrupted jobs, and snapshot
//! construction.
//!
//! All generation runs exclusively on-device using the local clean-up model;
//! this module performs no network requests and touches no cloud providers.

#![cfg(feature = "meetings")]

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
use chrono::DateTime;
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Emitter, Manager};

use crate::managers::audio::AudioRecordingManager;
use crate::managers::meetings::{MeetingSegment, MeetingsStore, StoredNote};
use crate::managers::model::ModelManager;
use crate::meeting_controller::{OPEN_CONSENT_EVENT, SEGMENT_EVENT, STATE_EVENT};
use crate::meeting_feed::pending_chunk_count;
use crate::meeting_notes::{build_notes_prompt, parse_notes_reply, NoteKind, PromptSegment};
use crate::meeting_notes_merge::merge_pieces;
use crate::meeting_notes_pieces::{
    estimate_tokens, is_one_shot, piece_budget, plan_pieces, SEGMENT_TAG_TOKENS,
};
use crate::meeting_notes_runner::{
    require_one_shot_pieces, NotesGenerator, DEFAULT_LOCAL_NOTES_N_CTX, NOTEPAD_KIND,
};
use crate::meeting_notes_v42::{
    generate_left_out_reasons, load_left_out_spans, parse_template_headings,
    run_notes_with_quality_and_words, save_left_out_spans,
};
use crate::meeting_retention::{maybe_remove_audio_after_notes, sync_meeting_markdown_file};
use crate::settings::{self, LOCAL_LLM_DEFAULT_MODEL_ID, LOCAL_LLM_PROVIDER_ID};

/// Canonical event names shared with the frontend (`src/components/meetings/meetingEvents.ts`).
pub const MEETING_SEGMENT_EVENT: &str = SEGMENT_EVENT;
pub const MEETING_STATE_EVENT: &str = STATE_EVENT;
pub const MEETINGS_OFFER_EVENT: &str = "meetings-offer";
pub const MEETINGS_OPEN_CONSENT_EVENT: &str = OPEN_CONSENT_EVENT;
pub const MEETING_NOTES_PROGRESS_EVENT: &str = "meeting-notes-progress";
pub const MEETING_NOTES_SO_FAR_EVENT: &str = "meeting-notes-so-far";
pub const MEETING_NOTES_READY_EVENT: &str = "meeting-notes-ready";
pub const MEETING_NOTES_FAILED_EVENT: &str = "meeting-notes-failed";
pub const MEETING_SPEAKERS_READY_EVENT: &str = "meeting-speakers-ready";
pub const MEETING_EXTRAS_READY_EVENT: &str = "meeting-extras-ready";

/// Minimum new spoken span (5 minutes) required before mapping a rolling piece.
pub const ROLLING_MIN_NEW_SPAN_MS: i64 = 300_000;
/// Polling interval for the rolling notes ticker while recording.
pub const ROLLING_TICK_SECS: u64 = 60;
/// Yield threshold: skip rolling tick if more than 2 audio chunks wait in the queue.
pub const ROLLING_MAX_PENDING_CHUNKS: usize = 2;
/// Maximum runtime allowed for an in-flight rolling piece once dictation starts.
pub const ROLLING_DICTATION_CUT_SECS: u64 = 30;
/// Default throughput estimate (tokens/second) before any piece has been timed.
pub const DEFAULT_TOKENS_PER_SEC: f64 = 20.0;
/// Maximum word count for a model-written meeting title.
pub const MAX_AUTO_TITLE_WORDS: usize = 6;
/// Minimum 3+-letter word overlap fraction for `from_notepad = true`.
pub const NOTEPAD_OVERLAP_RATIO: f64 = 0.60;

/// Machine-readable reason when meeting notes could not be generated.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum NotesFailReason {
    NoModel,
    Unavailable,
    Interrupted,
    Failed,
}

impl NotesFailReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NoModel => "no_model",
            Self::Unavailable => "unavailable",
            Self::Interrupted => "interrupted",
            Self::Failed => "failed",
        }
    }
}

/// Wire payload for `meeting-notes-progress`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct NotesProgressPayload {
    pub meeting_id: i64,
    pub phase: String,
    pub elapsed_secs: u64,
    pub estimate_secs: Option<u64>,
    pub progress_label: String,
    pub calls_done: u32,
    pub calls_total: u32,
}

/// Wire payload for `meeting-notes-so-far`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct NotesSoFarPayload {
    pub meeting_id: i64,
    pub lines: Vec<PipelineNoteLine>,
}

/// Wire payload for `meeting-notes-ready`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct NotesReadyPayload {
    pub meeting_id: i64,
    pub time_to_notes_secs: u64,
    pub title: String,
}

/// Wire payload for `meeting-notes-failed`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct NotesFailedPayload {
    pub meeting_id: i64,
    pub reason: String,
}

/// Wire payload for `meeting-speakers-ready` (emitted only when `speaker-split` runs).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct SpeakersReadyPayload {
    pub meeting_id: i64,
    pub speaker_count: usize,
}

/// Wire payload for `meeting-extras-ready` (left-out reasons, retention, Markdown, to-dos, Ask index).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct ExtrasReadyPayload {
    pub meeting_id: i64,
    pub left_out_count: usize,
}

/// One persisted left-out span returned inside `PipelineNotesSnapshot` (`AC-X26`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct LeftOutRecordDto {
    pub start_ms: i64,
    pub end_ms: i64,
    pub category: String,
    pub topic: String,
    pub reason: String,
    pub segment_ids: Vec<i64>,
}

/// One cited note line in snapshots and live rolling updates.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct PipelineNoteLine {
    pub kind: String,
    pub text: String,
    pub owner: Option<String>,
    pub start_ms: Option<i64>,
    pub from_notepad: bool,
}

/// Snapshot returned to meeting views, matching `MeetingNotesSnapshotDto`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct PipelineNotesSnapshot {
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
    pub lines: Vec<PipelineNoteLine>,
    pub oversized_count: usize,
    pub left_out: Vec<LeftOutRecordDto>,
}

/// Extension hooks reserved for calendar title / speaker labels / left-out spans.
/// Each hook is a no-op when `None`.
#[derive(Clone, Debug, Default)]
pub struct AfterStopHooks {
    pub calendar_title: Option<String>,
}

/// Runtime options passed into the after-Stop notes pipeline.
#[derive(Clone, Debug, Default)]
pub struct AfterStopConfig {
    pub delete_audio_after_notes: bool,
    pub save_notes_as_markdown: bool,
    pub notes_markdown_folder: Option<PathBuf>,
    pub custom_words: Vec<String>,
    pub template_body: Option<String>,
    pub hooks: AfterStopHooks,
}

/// Outcome of a single after-Stop run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AfterStopOutcome {
    /// Zero non-empty transcript segments; status stays `transcript_ready`.
    Empty,
    /// Notes generated, persisted, titled, retention-applied, and synced.
    Ready {
        title: String,
        notes_count: usize,
        used_rolling_pieces: bool,
        markdown_path: Option<PathBuf>,
        time_to_notes_secs: u64,
    },
    /// Notes failed; status set to `notes_failed`, transcript/notepad/audio kept.
    Failed { reason: NotesFailReason },
}

/// Mapped rolling piece entry: `((kind, owner), text, cited_segment_ids)`.
pub type MappedPieceLine = ((NoteKind, Option<String>), String, Vec<i64>);

/// Per-meeting in-memory pipeline state.
#[derive(Clone, Debug)]
pub struct JobState {
    pub pieces: Vec<Vec<MappedPieceLine>>,
    pub last_segment_id: i64,
    pub last_segment_end_ms: i64,
    pub running: bool,
    pub renamed: bool,
    pub cancel: Arc<AtomicBool>,
    pub phase: String,
    pub elapsed_secs: u64,
    pub estimate_secs: Option<u64>,
    pub calls_done: u32,
    pub calls_total: u32,
    pub time_to_notes_secs: Option<u64>,
    pub last_failure_reason: Option<NotesFailReason>,
    pub so_far_lines: Vec<PipelineNoteLine>,
    pub measured_tok_per_sec: f64,
}

impl Default for JobState {
    fn default() -> Self {
        Self {
            pieces: Vec::new(),
            last_segment_id: 0,
            last_segment_end_ms: 0,
            running: false,
            renamed: false,
            cancel: Arc::new(AtomicBool::new(false)),
            phase: "idle".to_string(),
            elapsed_secs: 0,
            estimate_secs: None,
            calls_done: 0,
            calls_total: 0,
            time_to_notes_secs: None,
            last_failure_reason: None,
            so_far_lines: Vec::new(),
            measured_tok_per_sec: DEFAULT_TOKENS_PER_SEC,
        }
    }
}

/// Managed Tauri state holding per-meeting pipeline jobs and rolling tickers.
#[derive(Default)]
pub struct MeetingPipelineState {
    jobs: Mutex<HashMap<i64, JobState>>,
    rolling_cancel: Mutex<HashMap<i64, Arc<AtomicBool>>>,
}

impl MeetingPipelineState {
    pub fn new() -> Self {
        Self::default()
    }

    fn lock_jobs(&self) -> std::sync::MutexGuard<'_, HashMap<i64, JobState>> {
        self.jobs.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn lock_rolling(&self) -> std::sync::MutexGuard<'_, HashMap<i64, Arc<AtomicBool>>> {
        self.rolling_cancel
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// Atomically claim the after-Stop job slot for `meeting_id`.
    /// Returns `false` if a job is already running for that meeting.
    pub fn try_begin_job(&self, meeting_id: i64, estimate_secs: Option<u64>) -> bool {
        let mut jobs = self.lock_jobs();
        let entry = jobs.entry(meeting_id).or_default();
        if entry.running {
            return false;
        }
        entry.running = true;
        entry.cancel.store(false, Ordering::Relaxed);
        entry.phase = "finishing".to_string();
        entry.elapsed_secs = 0;
        entry.estimate_secs = estimate_secs;
        entry.calls_done = 0;
        entry.calls_total = 1;
        entry.last_failure_reason = None;
        true
    }

    pub fn update_progress(
        &self,
        meeting_id: i64,
        elapsed_secs: u64,
        estimate_secs: Option<u64>,
        calls_done: u32,
        calls_total: u32,
    ) {
        let mut jobs = self.lock_jobs();
        let entry = jobs.entry(meeting_id).or_default();
        entry.elapsed_secs = elapsed_secs;
        if estimate_secs.is_some() {
            entry.estimate_secs = estimate_secs;
        }
        entry.calls_done = calls_done;
        entry.calls_total = calls_total.max(1);
    }

    pub fn finish_job_ready(&self, meeting_id: i64, time_to_notes_secs: u64) {
        let mut jobs = self.lock_jobs();
        let entry = jobs.entry(meeting_id).or_default();
        entry.running = false;
        entry.phase = "ready".to_string();
        entry.elapsed_secs = time_to_notes_secs;
        entry.time_to_notes_secs = Some(time_to_notes_secs);
        entry.last_failure_reason = None;
    }

    pub fn finish_job_failed(&self, meeting_id: i64, reason: NotesFailReason) {
        let mut jobs = self.lock_jobs();
        let entry = jobs.entry(meeting_id).or_default();
        entry.running = false;
        entry.phase = "failed".to_string();
        entry.last_failure_reason = Some(reason);
    }

    pub fn finish_job_empty(&self, meeting_id: i64) {
        let mut jobs = self.lock_jobs();
        let entry = jobs.entry(meeting_id).or_default();
        entry.running = false;
        entry.phase = "idle".to_string();
        entry.last_failure_reason = None;
    }

    pub fn mark_renamed(&self, meeting_id: i64) {
        let mut jobs = self.lock_jobs();
        jobs.entry(meeting_id).or_default().renamed = true;
    }

    pub fn is_renamed(&self, meeting_id: i64) -> bool {
        self.lock_jobs()
            .get(&meeting_id)
            .map(|j| j.renamed)
            .unwrap_or(false)
    }

    pub fn is_running(&self, meeting_id: i64) -> bool {
        self.lock_jobs()
            .get(&meeting_id)
            .map(|j| j.running)
            .unwrap_or(false)
    }

    pub fn snapshot_job(&self, meeting_id: i64) -> Option<JobState> {
        self.lock_jobs().get(&meeting_id).cloned()
    }

    pub fn register_rolling(&self, meeting_id: i64) -> Arc<AtomicBool> {
        let cancel = Arc::new(AtomicBool::new(false));
        let mut rolling = self.lock_rolling();
        if let Some(prev) = rolling.insert(meeting_id, Arc::clone(&cancel)) {
            prev.store(true, Ordering::Relaxed);
        }
        cancel
    }

    pub fn stop_rolling(&self, meeting_id: i64) {
        if let Some(cancel) = self.lock_rolling().remove(&meeting_id) {
            cancel.store(true, Ordering::Relaxed);
        }
    }
}

/// Format countdown/count-up label for the finishing-notes progress bar.
/// Counts down to 0 while `elapsed_secs <= estimate_secs`, then counts up past the estimate.
pub fn format_finishing_label(elapsed_secs: u64, estimate_secs: Option<u64>) -> String {
    match estimate_secs {
        Some(est) if est > 0 && elapsed_secs <= est => {
            let remaining = est.saturating_sub(elapsed_secs);
            format!("Finishing notes: {remaining} s")
        }
        _ => format!("Finishing notes: {elapsed_secs} s"),
    }
}

/// Estimate after-Stop generation seconds from unmapped transcript tokens and measured tok/s.
pub fn estimate_seconds_remaining(tokens_left: usize, tok_per_sec: f64) -> u64 {
    let rate = if tok_per_sec.is_finite() && tok_per_sec >= 1.0 {
        tok_per_sec
    } else {
        DEFAULT_TOKENS_PER_SEC
    };
    let reply_budget = 256.0;
    let prompt_overhead = (tokens_left as f64) / (rate * 6.0);
    let secs = (reply_budget / rate + prompt_overhead).ceil() as u64;
    secs.max(1)
}

/// Normalize stored note kinds to singular (`summary`, `decision`, `action`, `question`).
pub fn normalize_note_kind(kind: &str) -> &str {
    match kind {
        "summary" => "summary",
        "decision" | "decisions" => "decision",
        "action" | "actions" => "action",
        "question" | "questions" => "question",
        other => other,
    }
}

fn kind_to_singular(kind: NoteKind) -> &'static str {
    match kind {
        NoteKind::Summary => "summary",
        NoteKind::Decision => "decision",
        NoteKind::Action => "action",
        NoteKind::Question => "question",
    }
}

fn significant_words(text: &str) -> HashSet<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .map(|w| w.to_lowercase())
        .filter(|w| w.chars().count() >= 3)
        .collect()
}

/// True when at least 60% of any notepad point's 3+-letter words appear in `line_text`.
pub fn matches_notepad_point(line_text: &str, notepad_points: &[String]) -> bool {
    let line_words = significant_words(line_text);
    if line_words.is_empty() {
        return false;
    }
    for point in notepad_points {
        let point_words = significant_words(point);
        if point_words.is_empty() {
            continue;
        }
        let hits = point_words
            .iter()
            .filter(|w| line_words.contains(*w))
            .count();
        if (hits as f64) / (point_words.len() as f64) >= NOTEPAD_OVERLAP_RATIO {
            return true;
        }
    }
    false
}

/// Split stored notepad rows into individual points, remaining backward-compatible
/// with legacy single-row `at_ms = 0` newline-delimited storage.
pub fn read_notepad_points(store: &MeetingsStore, meeting_id: i64) -> Result<Vec<(i64, String)>> {
    let rows = store.notepad(meeting_id)?;
    let mut out = Vec::new();
    for (at_ms, raw) in rows {
        if raw.contains('\n') {
            for (idx, line) in raw.lines().enumerate() {
                let trimmed = line.trim();
                if !trimmed.is_empty() {
                    out.push((at_ms.saturating_add(idx as i64), trimmed.to_string()));
                }
            }
        } else {
            let trimmed = raw.trim();
            if !trimmed.is_empty() {
                out.push((at_ms, trimmed.to_string()));
            }
        }
    }
    Ok(out)
}

/// Save notepad text as one row per non-empty point with monotonically rising `at_ms`.
pub fn save_meeting_notepad_points(
    store: &MeetingsStore,
    meeting_id: i64,
    text: &str,
    base_ms: i64,
) -> Result<()> {
    let existing = read_notepad_points(store, meeting_id).unwrap_or_default();
    let existing_by_text: HashMap<String, i64> = existing
        .into_iter()
        .map(|(ms, t)| (t.to_lowercase(), ms))
        .collect();

    let mut next_ms = base_ms.max(0);
    let mut points: Vec<(i64, String)> = Vec::new();
    for raw_line in text.lines() {
        let trimmed = raw_line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let assigned = existing_by_text
            .get(&trimmed.to_lowercase())
            .copied()
            .filter(|&ms| points.last().map(|(prev, _)| ms > *prev).unwrap_or(true))
            .unwrap_or_else(|| {
                let candidate = match points.last() {
                    Some((prev, _)) if next_ms <= *prev => prev.saturating_add(1),
                    _ => next_ms,
                };
                next_ms = candidate.saturating_add(1);
                candidate
            });
        points.push((assigned, trimmed.to_string()));
    }
    store.replace_notepad_points(meeting_id, &points)
}

/// Rename a meeting and record that the user chose its title so auto-title never overwrites it.
pub fn rename_meeting_title(
    store: &MeetingsStore,
    pipeline: Option<&MeetingPipelineState>,
    meeting_id: i64,
    title: &str,
) -> Result<String> {
    let stored = store.rename_meeting(meeting_id, title)?;
    if let Some(state) = pipeline {
        state.mark_renamed(meeting_id);
    }
    Ok(stored)
}

/// Build the snapshot returned by `meeting_notes_snapshot` (strictly grounded citations,
/// singular kinds, 60% notepad overlap badge, and live job progress/failure state).
pub fn build_meeting_notes_snapshot(
    store: &MeetingsStore,
    pipeline: Option<&MeetingPipelineState>,
    meeting_id: i64,
) -> Result<PipelineNotesSnapshot> {
    let meeting = store.meeting(meeting_id)?;
    let segments = store.segments(meeting_id)?;
    let notes = store.notes(meeting_id)?;
    let notepad_rows = read_notepad_points(store, meeting_id)?;
    let notepad_points: Vec<String> = notepad_rows.into_iter().map(|(_, t)| t).collect();
    let notepad_text = notepad_points.join("\n");

    let seg_start_by_id: HashMap<i64, i64> = segments.iter().map(|s| (s.id, s.start_ms)).collect();
    let job = pipeline.and_then(|p| p.snapshot_job(meeting_id));

    let mut dropped_uncited = 0usize;
    let mut lines: Vec<PipelineNoteLine> = Vec::new();
    for note in notes {
        if note.kind == NOTEPAD_KIND {
            continue;
        }
        let first_valid_start = note
            .citations
            .iter()
            .find_map(|cid| seg_start_by_id.get(cid).copied());
        let Some(start_ms) = first_valid_start else {
            dropped_uncited += 1;
            continue;
        };
        let from_notepad = matches_notepad_point(&note.text, &notepad_points);
        lines.push(PipelineNoteLine {
            kind: normalize_note_kind(&note.kind).to_string(),
            text: note.text,
            owner: note.owner,
            start_ms: Some(start_ms),
            from_notepad,
        });
    }

    // While recording (before final notes exist), expose rolling "Notes so far" lines.
    if lines.is_empty() && meeting.status == "recording" {
        if let Some(ref j) = job {
            if !j.so_far_lines.is_empty() {
                lines = j.so_far_lines.clone();
            }
        }
    }

    let is_running = job.as_ref().map(|j| j.running).unwrap_or(false);
    let state = if is_running {
        "running".to_string()
    } else if meeting.status == "notes_failed" {
        "failed".to_string()
    } else if meeting.status == "notes_ready" || !lines.is_empty() {
        "ready".to_string()
    } else {
        "idle".to_string()
    };

    let (
        progress_label,
        progress_step,
        progress_total,
        elapsed_secs,
        estimate_secs,
        time_to_notes_secs,
        failure_reason,
    ) = if let Some(j) = job {
        let label = if j.running {
            Some(format_finishing_label(j.elapsed_secs, j.estimate_secs))
        } else {
            None
        };
        let step = if j.running {
            Some(j.calls_done.max(1))
        } else {
            None
        };
        let total = if j.running {
            Some(j.calls_total.max(1))
        } else {
            None
        };
        let reason = if state == "failed" {
            j.last_failure_reason
                .map(|r| r.as_str().to_string())
                .or_else(|| Some(NotesFailReason::Failed.as_str().to_string()))
        } else {
            None
        };
        (
            label,
            step,
            total,
            j.elapsed_secs,
            j.estimate_secs,
            j.time_to_notes_secs,
            reason,
        )
    } else {
        let reason = if state == "failed" {
            Some(NotesFailReason::Failed.as_str().to_string())
        } else {
            None
        };
        (None, None, None, 0, None, None, reason)
    };

    let left_out: Vec<LeftOutRecordDto> = load_left_out_spans(store, meeting_id)
        .unwrap_or_default()
        .into_iter()
        .map(|rec| LeftOutRecordDto {
            start_ms: rec.start_ms,
            end_ms: rec.end_ms,
            category: rec.kind,
            topic: rec.topic,
            reason: rec.reason,
            segment_ids: rec.segment_ids,
        })
        .collect();

    Ok(PipelineNotesSnapshot {
        meeting_id,
        state,
        progress_label,
        progress_step,
        progress_total,
        elapsed_secs,
        estimate_secs,
        time_to_notes_secs,
        failure_reason,
        notepad_text,
        lines,
        oversized_count: dropped_uncited,
        left_out,
    })
}

/// Startup sweep: any stopped meeting stuck at `transcript_ready` with spoken segments,
/// no saved notes, and no running job is marked `notes_failed` (`interrupted`) so it
/// surfaces a Retry button.
pub fn sweep_interrupted_meetings(
    store: &MeetingsStore,
    pipeline: &MeetingPipelineState,
) -> Result<Vec<i64>> {
    let mut swept = Vec::new();
    for meeting in store.list_meetings()? {
        if meeting.status != "transcript_ready" {
            continue;
        }
        if pipeline.is_running(meeting.id) {
            continue;
        }
        let has_spoken_segments = store
            .segments(meeting.id)?
            .iter()
            .any(|s| !s.text.trim().is_empty());
        if !has_spoken_segments {
            continue;
        }
        if !store.notes(meeting.id)?.is_empty() {
            continue;
        }
        store.set_status(meeting.id, "notes_failed")?;
        pipeline.finish_job_failed(meeting.id, NotesFailReason::Interrupted);
        swept.push(meeting.id);
    }
    Ok(swept)
}

/// Build a fallback title formatted as `"Meeting · <date time>"`.
pub fn fallback_meeting_title(started_at: i64) -> String {
    DateTime::from_timestamp(started_at, 0)
        .map(|dt| format!("Meeting · {}", dt.format("%b %-d %H:%M")))
        .unwrap_or_else(|| "Meeting".to_string())
}

/// Clean an untrusted model-written title into 1..=6 plain words, or `None` if unusable.
pub fn sanitize_generated_title(raw: &str) -> Option<String> {
    let first_line = raw.lines().map(str::trim).find(|line| !line.is_empty())?;
    let stripped = first_line
        .trim_start_matches("TITLE:")
        .trim_start_matches("Title:")
        .trim_start_matches("title:")
        .trim()
        .trim_matches(|c: char| matches!(c, '"' | '\'' | '`' | '#' | '*' | '-' | '•' | '.'))
        .trim();
    let words: Vec<&str> = stripped
        .split_whitespace()
        .filter(|w| !w.is_empty())
        .take(MAX_AUTO_TITLE_WORDS)
        .collect();
    if words.is_empty() {
        return None;
    }
    let cleaned = words
        .join(" ")
        .trim_end_matches(['.', ':', ';', ','])
        .to_string();
    if cleaned.is_empty() {
        return None;
    }
    Some(cleaned)
}

fn is_default_meeting_title(title: &str) -> bool {
    let trimmed = title.trim();
    trimmed.is_empty() || trimmed == "Meeting" || trimmed == "Meeting — Mac slept"
}

fn generate_short_title<G: NotesGenerator>(
    notes: &[StoredNote],
    generator: &mut G,
) -> Option<String> {
    let summary_lines: Vec<String> = notes
        .iter()
        .filter(|n| n.kind != NOTEPAD_KIND)
        .take(8)
        .map(|n| format!("- {}", n.text))
        .collect();
    if summary_lines.is_empty() {
        return None;
    }
    let prompt = format!(
        "Write a short title of 2 to 6 words for this meeting based only on these notes.\n\
         Reply with the title alone and no quotes or punctuation.\n\n{}\n",
        summary_lines.join("\n")
    );
    let reply = generator.generate(&prompt).ok()?;
    sanitize_generated_title(&reply)
}

fn prompt_segments(raw: &[MeetingSegment]) -> Vec<PromptSegment> {
    raw.iter()
        .filter(|s| !s.text.trim().is_empty())
        .map(|s| PromptSegment {
            id: s.id,
            channel: s.channel.clone(),
            text: s.text.clone(),
        })
        .collect()
}

/// Evaluate one rolling-notes tick during a recording meeting.
///
/// Yields (returns `Ok(None)`) when:
/// - dictation is active,
/// - more than `ROLLING_MAX_PENDING_CHUNKS` audio chunks are queued,
/// - the transcript still fits in one-shot context (`is_one_shot`), or
/// - fewer than `ROLLING_MIN_NEW_SPAN_MS` (5 minutes) of new talk arrived since the last piece.
pub fn rolling_tick_once<G: NotesGenerator>(
    store: &MeetingsStore,
    pipeline: &MeetingPipelineState,
    meeting_id: i64,
    dictation_active: bool,
    queued_chunks: usize,
    generator: &mut G,
) -> Result<Option<NotesSoFarPayload>> {
    if dictation_active || queued_chunks > ROLLING_MAX_PENDING_CHUNKS {
        return Ok(None);
    }

    let mut raw_segments = store.segments(meeting_id)?;
    raw_segments.sort_by_key(|s| (s.start_ms, s.id));
    let spoken: Vec<MeetingSegment> = raw_segments
        .into_iter()
        .filter(|s| !s.text.trim().is_empty())
        .collect();
    if spoken.is_empty() {
        return Ok(None);
    }

    let notepad_points: Vec<String> = read_notepad_points(store, meeting_id)?
        .into_iter()
        .map(|(_, t)| t)
        .collect();

    let n_ctx = generator.context_window()?;
    let prompt_tokens = estimate_tokens(&build_notes_prompt(&[], &[]));
    let notepad_tokens =
        estimate_tokens(&build_notes_prompt(&[], &notepad_points)).saturating_sub(prompt_tokens);
    let all_seg_tokens: Vec<usize> = spoken.iter().map(|s| estimate_tokens(&s.text)).collect();
    let all_pieces = plan_pieces(&all_seg_tokens, notepad_tokens, prompt_tokens, n_ctx);
    // ponytail: ceiling = 1 piece on a 4,096 context; upgrade path = P2 decides
    // whether map-merge comes back or is deleted.
    if is_one_shot(&all_pieces) || require_one_shot_pieces(meeting_id, all_pieces.len()).is_err() {
        return Ok(None);
    }

    let (last_seg_id, last_end_ms) = {
        let jobs = pipeline.lock_jobs();
        jobs.get(&meeting_id)
            .map(|j| (j.last_segment_id, j.last_segment_end_ms))
            .unwrap_or((0, 0))
    };

    let new_segs: Vec<MeetingSegment> = spoken
        .iter()
        .filter(|s| s.id > last_seg_id)
        .cloned()
        .collect();
    let Some(latest_end_ms) = new_segs.iter().map(|s| s.end_ms).max() else {
        return Ok(None);
    };
    let span_start = if last_seg_id == 0 {
        new_segs.first().map(|s| s.start_ms).unwrap_or(0)
    } else {
        last_end_ms
    };
    if latest_end_ms.saturating_sub(span_start) < ROLLING_MIN_NEW_SPAN_MS {
        return Ok(None);
    }

    let prompt_segs = prompt_segments(&new_segs);
    let prompt = build_notes_prompt(&prompt_segs, &notepad_points);
    let started = Instant::now();
    let reply = generator.generate(&prompt)?;
    let elapsed_s = started.elapsed().as_secs_f64().max(0.05);
    let reply_tokens = estimate_tokens(&reply).max(1) as f64;
    let tok_per_sec = (reply_tokens / elapsed_s).clamp(1.0, 200.0);

    let parsed: Vec<MappedPieceLine> = parse_notes_reply(&reply, &prompt_segs)
        .into_iter()
        .map(|l| ((l.kind, l.owner), l.text, l.citations))
        .collect();

    let max_new_id = new_segs.iter().map(|s| s.id).max().unwrap_or(last_seg_id);
    let seg_start_by_id: HashMap<i64, i64> = spoken.iter().map(|s| (s.id, s.start_ms)).collect();

    let so_far_lines: Vec<PipelineNoteLine> = {
        let mut jobs = pipeline.lock_jobs();
        let entry = jobs.entry(meeting_id).or_default();
        entry.pieces.push(parsed);
        entry.last_segment_id = max_new_id;
        entry.last_segment_end_ms = latest_end_ms;
        entry.measured_tok_per_sec = tok_per_sec;

        let merged = merge_pieces(entry.pieces.clone());
        let lines: Vec<PipelineNoteLine> = merged
            .into_iter()
            .filter_map(|((kind, owner), text, citations)| {
                let start_ms = citations
                    .iter()
                    .find_map(|cid| seg_start_by_id.get(cid).copied())?;
                Some(PipelineNoteLine {
                    kind: kind_to_singular(kind).to_string(),
                    text: text.clone(),
                    owner,
                    start_ms: Some(start_ms),
                    from_notepad: matches_notepad_point(&text, &notepad_points),
                })
            })
            .collect();
        entry.so_far_lines = lines.clone();
        lines
    };

    Ok(Some(NotesSoFarPayload {
        meeting_id,
        lines: so_far_lines,
    }))
}

/// Core testable after-Stop job.
///
/// Order (`AC-X21`):
/// 1. Empty transcript -> stay `transcript_ready` (`AfterStopOutcome::Empty`).
/// 2. Check generator availability -> if `Err(reason)` mark `notes_failed`, keep audio/transcript/notepad.
/// 3. One-shot when whole transcript fits (ignoring rolling pieces); otherwise map tail after
///    `last_segment_id` + `merge_pieces` + preserve dropped notepad points + mark `notes_ready`.
/// 4. Title: skip if renamed; else `hooks.calendar_title`; else local short title (2..=6 words);
///    else `"Meeting · <date time>"`.
/// 5. Emit `meeting-notes-ready` (via `on_notes_ready`) BEFORE background extras.
/// 6. Optional speaker split hook (only compiled under `#[cfg(feature = "speaker-split")]`),
///    running while `system.wav` is still on disk.
/// 7. Compute and persist `meeting_left_out` spans with numbered-excerpt-only reasons.
/// 8. Retention: delete audio ONLY after notes (and optional speaker split) succeed.
/// 9. Markdown file sync when `save_notes_as_markdown` is on and folder is set.
/// 10. Refresh cross-meeting to-dos and Ask index, then return `AfterStopOutcome::Ready`.
pub fn run_after_stop_with_generator<G, F>(
    store: &MeetingsStore,
    pipeline: &MeetingPipelineState,
    meeting_id: i64,
    config: &AfterStopConfig,
    generator_result: Result<G, NotesFailReason>,
    on_progress: F,
) -> Result<AfterStopOutcome>
where
    G: NotesGenerator,
    F: FnMut(NotesProgressPayload),
{
    run_after_stop_with_hooks(
        store,
        pipeline,
        meeting_id,
        config,
        generator_result,
        on_progress,
        |_, _| {},
        || Ok(0),
    )
}

#[allow(clippy::too_many_arguments)]
pub fn run_after_stop_with_hooks<G, F, R, S>(
    store: &MeetingsStore,
    pipeline: &MeetingPipelineState,
    meeting_id: i64,
    config: &AfterStopConfig,
    generator_result: Result<G, NotesFailReason>,
    mut on_progress: F,
    mut on_notes_ready: R,
    mut run_speakers: S,
) -> Result<AfterStopOutcome>
where
    G: NotesGenerator,
    F: FnMut(NotesProgressPayload),
    R: FnMut(&str, u64),
    S: FnMut() -> Result<usize>,
{
    let mut raw_segments = store.segments(meeting_id)?;
    raw_segments.sort_by_key(|s| (s.start_ms, s.id));
    let spoken: Vec<MeetingSegment> = raw_segments
        .into_iter()
        .filter(|s| !s.text.trim().is_empty())
        .collect();
    if spoken.is_empty() {
        pipeline.finish_job_empty(meeting_id);
        return Ok(AfterStopOutcome::Empty);
    }

    let notepad_points: Vec<String> = read_notepad_points(store, meeting_id)?
        .into_iter()
        .map(|(_, t)| t)
        .collect();

    let (existing_pieces, last_seg_id, tok_per_sec) = {
        let jobs = pipeline.lock_jobs();
        jobs.get(&meeting_id)
            .map(|j| (j.pieces.clone(), j.last_segment_id, j.measured_tok_per_sec))
            .unwrap_or_else(|| (Vec::new(), 0, DEFAULT_TOKENS_PER_SEC))
    };

    let mut generator_result = generator_result;
    let n_ctx = match &mut generator_result {
        Ok(g) => match g.context_window() {
            Ok(v) => v,
            Err(_) => {
                let _ = store.set_status(meeting_id, "notes_failed");
                pipeline.finish_job_failed(meeting_id, NotesFailReason::Failed);
                return Ok(AfterStopOutcome::Failed {
                    reason: NotesFailReason::Failed,
                });
            }
        },
        Err(_) => DEFAULT_LOCAL_NOTES_N_CTX,
    };
    let prompt_tokens = estimate_tokens(&build_notes_prompt(&[], &[]));
    let notepad_tokens =
        estimate_tokens(&build_notes_prompt(&[], &notepad_points)).saturating_sub(prompt_tokens);
    let all_seg_tokens: Vec<usize> = spoken.iter().map(|s| estimate_tokens(&s.text)).collect();
    let all_pieces = plan_pieces(&all_seg_tokens, notepad_tokens, prompt_tokens, n_ctx);
    let fits_one_shot = is_one_shot(&all_pieces);
    let has_custom_headings = config
        .template_body
        .as_deref()
        .is_some_and(|tpl| !parse_template_headings(tpl).is_empty());

    let tokens_left: usize = if fits_one_shot || existing_pieces.is_empty() {
        all_seg_tokens.iter().sum()
    } else {
        spoken
            .iter()
            .filter(|s| s.id > last_seg_id)
            .map(|s| estimate_tokens(&s.text))
            .sum()
    };
    let estimate_secs = Some(estimate_seconds_remaining(tokens_left, tok_per_sec));

    if !pipeline.try_begin_job(meeting_id, estimate_secs) {
        return Ok(AfterStopOutcome::Empty);
    }

    let started = Instant::now();
    on_progress(NotesProgressPayload {
        meeting_id,
        phase: "finishing".to_string(),
        elapsed_secs: 0,
        estimate_secs,
        progress_label: format_finishing_label(0, estimate_secs),
        calls_done: 0,
        calls_total: if fits_one_shot {
            1
        } else {
            all_pieces.len().max(1) as u32
        },
    });

    let mut generator = match generator_result {
        Ok(g) => g,
        Err(reason) => {
            let _ = store.set_status(meeting_id, "notes_failed");
            pipeline.finish_job_failed(meeting_id, reason);
            return Ok(AfterStopOutcome::Failed { reason });
        }
    };

    // ponytail: ceiling = 1 piece on a 4,096 context; upgrade path = P2 decides
    // whether map-merge comes back or is deleted.
    if require_one_shot_pieces(meeting_id, all_pieces.len()).is_err() {
        let _ = store.set_status(meeting_id, "notes_failed");
        pipeline.finish_job_failed(meeting_id, NotesFailReason::Failed);
        return Ok(AfterStopOutcome::Failed {
            reason: NotesFailReason::Failed,
        });
    }

    let used_rolling_pieces;
    let saved_notes: Vec<StoredNote>;
    let mut oversized_segments: Vec<i64> = Vec::new();

    if fits_one_shot || existing_pieces.is_empty() || has_custom_headings {
        used_rolling_pieces = false;
        let run = run_notes_with_quality_and_words(
            store,
            meeting_id,
            config.template_body.as_deref(),
            &config.custom_words,
            &mut generator,
        );
        match run {
            Ok(report) => {
                oversized_segments = report.oversized_segments;
                saved_notes = store.notes(meeting_id)?;
            }
            Err(_) => {
                let _ = store.set_status(meeting_id, "notes_failed");
                pipeline.finish_job_failed(meeting_id, NotesFailReason::Failed);
                return Ok(AfterStopOutcome::Failed {
                    reason: NotesFailReason::Failed,
                });
            }
        }
    } else {
        used_rolling_pieces = true;
        let tail_segs: Vec<MeetingSegment> =
            spoken.into_iter().filter(|s| s.id > last_seg_id).collect();
        let mut combined_pieces = existing_pieces;

        if !tail_segs.is_empty() {
            let tail_prompt_segs = prompt_segments(&tail_segs);
            let tail_tokens: Vec<usize> = tail_prompt_segs
                .iter()
                .map(|s| estimate_tokens(&s.text))
                .collect();
            let tail_budget = piece_budget(n_ctx, prompt_tokens).saturating_sub(notepad_tokens);
            let tail_ranges: Vec<_> =
                plan_pieces(&tail_tokens, notepad_tokens, prompt_tokens, n_ctx)
                    .into_iter()
                    .filter(|r| {
                        tail_tokens[r.clone()]
                            .iter()
                            .fold(0usize, |acc, n| acc.saturating_add(n + SEGMENT_TAG_TOKENS))
                            <= tail_budget
                    })
                    .collect();

            let total_calls = tail_ranges.len().max(1) as u32;
            for (idx, range) in tail_ranges.iter().enumerate() {
                let slice = &tail_prompt_segs[range.clone()];
                let mut prompt = build_notes_prompt(slice, &notepad_points);
                if !config.custom_words.is_empty() {
                    prompt.push_str("\nPreferred spelling of meeting terms and names: ");
                    prompt.push_str(&config.custom_words.join(", "));
                    prompt.push('\n');
                }
                if let Some(tpl) = config.template_body.as_deref() {
                    prompt.push_str(
                        "\nUser template instructions (keep strict [S<id>] citations; never invent decisions or owners):\n",
                    );
                    prompt.push_str(tpl.trim());
                    prompt.push('\n');
                }
                let reply = match generator.generate(&prompt) {
                    Ok(r) => r,
                    Err(_) => {
                        let _ = store.set_status(meeting_id, "notes_failed");
                        pipeline.finish_job_failed(meeting_id, NotesFailReason::Failed);
                        return Ok(AfterStopOutcome::Failed {
                            reason: NotesFailReason::Failed,
                        });
                    }
                };
                let parsed: Vec<MappedPieceLine> = parse_notes_reply(&reply, slice)
                    .into_iter()
                    .map(|l| ((l.kind, l.owner), l.text, l.citations))
                    .collect();
                combined_pieces.push(parsed);

                let elapsed = started.elapsed().as_secs();
                pipeline.update_progress(
                    meeting_id,
                    elapsed,
                    estimate_secs,
                    (idx + 1) as u32,
                    total_calls,
                );
                on_progress(NotesProgressPayload {
                    meeting_id,
                    phase: "finishing".to_string(),
                    elapsed_secs: elapsed,
                    estimate_secs,
                    progress_label: format_finishing_label(elapsed, estimate_secs),
                    calls_done: (idx + 1) as u32,
                    calls_total: total_calls,
                });
            }
        }

        let mut merged_notes: Vec<StoredNote> = merge_pieces(combined_pieces)
            .into_iter()
            .map(|((kind, owner), text, citations)| StoredNote {
                kind: kind_to_singular(kind).to_string(),
                text,
                owner,
                citations,
            })
            .collect();

        for point in &notepad_points {
            if !point.trim().is_empty()
                && !merged_notes
                    .iter()
                    .any(|n| matches_notepad_point(&n.text, &[point.clone()]))
            {
                merged_notes.push(StoredNote {
                    kind: NOTEPAD_KIND.to_string(),
                    text: point.clone(),
                    owner: None,
                    citations: Vec::new(),
                });
            }
        }

        if store.save_notes(meeting_id, &merged_notes).is_err()
            || store.set_status(meeting_id, "notes_ready").is_err()
        {
            let _ = store.set_status(meeting_id, "notes_failed");
            pipeline.finish_job_failed(meeting_id, NotesFailReason::Failed);
            return Ok(AfterStopOutcome::Failed {
                reason: NotesFailReason::Failed,
            });
        }
        saved_notes = merged_notes;
    }

    // Step 4: Title (skip if already renamed by the user).
    let meeting = store.meeting(meeting_id)?;
    let final_title =
        if pipeline.is_renamed(meeting_id) || !is_default_meeting_title(&meeting.title) {
            meeting.title.clone()
        } else if let Some(event_title) = config
            .hooks
            .calendar_title
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            store
                .rename_meeting(meeting_id, event_title)
                .unwrap_or_else(|_| event_title.to_string())
        } else {
            let candidate = generate_short_title(&saved_notes, &mut generator)
                .unwrap_or_else(|| fallback_meeting_title(meeting.started_at));
            store
                .rename_meeting(meeting_id, &candidate)
                .unwrap_or(candidate)
        };

    let time_to_notes_secs = started.elapsed().as_secs();
    pipeline.finish_job_ready(meeting_id, time_to_notes_secs);

    // Step 5: Notes & title are ready; surface them immediately before extras (`AC-X21`).
    on_notes_ready(&final_title, time_to_notes_secs);

    // Step 6: Optional speaker split runs BEFORE audio retention so `system.wav` still exists.
    if let Err(err) = run_speakers() {
        log::warn!("meeting {meeting_id}: speaker split skipped: {err:#}");
    }

    // Step 7: Persist left-out spans with numbered-excerpt-only reasons (`AC-X26`, `AC-X27`).
    if let Ok(records) =
        generate_left_out_reasons(store, meeting_id, &oversized_segments, Some(&mut generator))
    {
        if let Err(err) = save_left_out_spans(store, meeting_id, &records) {
            log::warn!("meeting {meeting_id}: failed to persist left-out spans: {err:#}");
        }
    }

    // Step 8: Audio retention cleanup (ONLY after notes + optional speaker split succeed).
    maybe_remove_audio_after_notes(store, meeting_id, config.delete_audio_after_notes);

    // Step 9: Optional Markdown folder sync.
    let markdown_path = sync_meeting_markdown_file(
        store,
        meeting_id,
        config.save_notes_as_markdown,
        config.notes_markdown_folder.as_deref(),
    )
    .unwrap_or_else(|err| {
        log::warn!("meeting {meeting_id}: markdown sync skipped: {err}");
        None
    });

    // Step 10: Refresh cross-meeting to-dos and Ask index (best-effort).
    if let Some(app_data_dir) = store.db_path().parent().and_then(|p| p.parent()) {
        if let Ok(todo_store) = crate::meeting_todos::MeetingTodoStore::open(app_data_dir) {
            if let Err(err) = todo_store.sync() {
                log::warn!("meeting {meeting_id}: todo sync skipped: {err:#}");
            }
        }
        if let Ok(ask_index) = crate::meeting_ask_index::MeetingAskIndex::open(app_data_dir) {
            if let Err(err) = ask_index.sync() {
                log::warn!("meeting {meeting_id}: ask index sync skipped: {err:#}");
            }
        }
    }

    Ok(AfterStopOutcome::Ready {
        title: final_title,
        notes_count: saved_notes.len(),
        used_rolling_pieces,
        markdown_path,
        time_to_notes_secs,
    })
}

/// Resolve the on-device clean-up model path on macOS only.
#[cfg(target_os = "macos")]
fn resolve_local_notes_generator(
    app: &AppHandle,
) -> Result<crate::meeting_notes_runner::LocalLlmNotes, NotesFailReason> {
    let app_settings = settings::get_settings(app);
    let model_id = app_settings
        .post_process_models
        .get(LOCAL_LLM_PROVIDER_ID)
        .map(String::as_str)
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(LOCAL_LLM_DEFAULT_MODEL_ID);
    let Some(mm) = app.try_state::<Arc<ModelManager>>() else {
        return Err(NotesFailReason::NoModel);
    };
    let model_path = mm
        .get_model_path(model_id)
        .or_else(|_| mm.get_model_path(LOCAL_LLM_DEFAULT_MODEL_ID))
        .map_err(|_| NotesFailReason::NoModel)?;
    Ok(crate::meeting_notes_runner::LocalLlmNotes::new(model_path))
}

#[cfg(not(target_os = "macos"))]
struct UnavailableNotesGenerator;

#[cfg(not(target_os = "macos"))]
impl NotesGenerator for UnavailableNotesGenerator {
    fn generate(&mut self, _prompt: &str) -> Result<String> {
        Err(anyhow!("local notes model unavailable on this system"))
    }
}

#[cfg(not(target_os = "macos"))]
fn resolve_local_notes_generator(
    _app: &AppHandle,
) -> Result<UnavailableNotesGenerator, NotesFailReason> {
    Err(NotesFailReason::Unavailable)
}

fn config_from_settings(
    s: &settings::AppSettings,
    store: &MeetingsStore,
    meeting_id: i64,
) -> AfterStopConfig {
    let mut words = s.meeting_custom_words.clone();
    for w in &s.custom_words {
        if !words
            .iter()
            .any(|existing| existing.eq_ignore_ascii_case(w))
        {
            words.push(w.clone());
        }
    }
    let template_body = match store.meeting_template_body(meeting_id) {
        Ok(Some(body)) if !body.trim().is_empty() => Some(body),
        Ok(_) => None,
        Err(err) => {
            log::warn!("meeting {meeting_id}: failed to load template body: {err:#}");
            None
        }
    };
    AfterStopConfig {
        delete_audio_after_notes: s.delete_audio_after_notes,
        save_notes_as_markdown: s.save_notes_as_markdown,
        notes_markdown_folder: s.notes_markdown_folder.clone().map(PathBuf::from),
        custom_words: words,
        template_body,
        hooks: AfterStopHooks::default(),
    }
}

fn config_from_app(app: &AppHandle, store: &MeetingsStore, meeting_id: i64) -> AfterStopConfig {
    let s = settings::get_settings(app);
    config_from_settings(&s, store, meeting_id)
}

fn dictation_is_active(app: &AppHandle) -> bool {
    app.try_state::<Arc<AudioRecordingManager>>()
        .map(|am| am.is_recording())
        .unwrap_or(false)
}

/// Start the 60-second rolling-notes background ticker for a live recording meeting.
pub fn start_rolling_if_managed(app: &AppHandle, meeting_id: i64) {
    let Some(pipeline_state) = app.try_state::<Arc<MeetingPipelineState>>() else {
        return;
    };
    let pipeline = Arc::clone(pipeline_state.inner());
    let cancel = pipeline.register_rolling(meeting_id);
    let app_handle = app.clone();

    std::thread::spawn(move || {
        let mut waited_secs = 0u64;
        loop {
            std::thread::sleep(Duration::from_secs(1));
            if cancel.load(Ordering::Relaxed) {
                return;
            }
            waited_secs += 1;
            if waited_secs < ROLLING_TICK_SECS {
                continue;
            }
            waited_secs = 0;

            let Ok(store) = MeetingsStore::open_app(&app_handle) else {
                continue;
            };
            let Ok(mut generator) = resolve_local_notes_generator(&app_handle) else {
                continue;
            };
            let dictating = dictation_is_active(&app_handle);
            let queued = pending_chunk_count();
            match rolling_tick_once(
                &store,
                &pipeline,
                meeting_id,
                dictating,
                queued,
                &mut generator,
            ) {
                Ok(Some(payload)) => {
                    let _ = app_handle.emit(MEETING_NOTES_SO_FAR_EVENT, payload);
                }
                Ok(None) => {}
                Err(err) => {
                    log::warn!("meeting {meeting_id}: rolling piece skipped: {err:#}");
                }
            }
        }
    });
}

/// Stop any active rolling ticker and spawn the after-Stop notes job (with a 1-second
/// progress emitter) if `MeetingPipelineState` is managed on `app`.
pub fn stop_rolling_and_run_after_stop(app: &AppHandle, meeting_id: i64) {
    let Some(pipeline_state) = app.try_state::<Arc<MeetingPipelineState>>() else {
        return;
    };
    let pipeline = Arc::clone(pipeline_state.inner());
    pipeline.stop_rolling(meeting_id);
    if pipeline.is_running(meeting_id) {
        return;
    }

    let app_handle = app.clone();
    std::thread::spawn(move || {
        let Ok(store) = MeetingsStore::open_app(&app_handle) else {
            log::warn!("meeting {meeting_id}: cannot open store for after-stop notes");
            return;
        };
        let config = config_from_app(&app_handle, &store, meeting_id);
        let gen_result = resolve_local_notes_generator(&app_handle);

        let ticker_done = Arc::new(AtomicBool::new(false));
        let ticker_flag = Arc::clone(&ticker_done);
        let ticker_app = app_handle.clone();
        let ticker_pipeline = Arc::clone(&pipeline);
        let ticker = std::thread::spawn(move || {
            let started = Instant::now();
            loop {
                std::thread::sleep(Duration::from_secs(1));
                if ticker_flag.load(Ordering::Relaxed) {
                    return;
                }
                let Some(job) = ticker_pipeline.snapshot_job(meeting_id) else {
                    continue;
                };
                if !job.running {
                    return;
                }
                let elapsed = started.elapsed().as_secs();
                ticker_pipeline.update_progress(
                    meeting_id,
                    elapsed,
                    job.estimate_secs,
                    job.calls_done,
                    job.calls_total,
                );
                let _ = ticker_app.emit(
                    MEETING_NOTES_PROGRESS_EVENT,
                    NotesProgressPayload {
                        meeting_id,
                        phase: "finishing".to_string(),
                        elapsed_secs: elapsed,
                        estimate_secs: job.estimate_secs,
                        progress_label: format_finishing_label(elapsed, job.estimate_secs),
                        calls_done: job.calls_done,
                        calls_total: job.calls_total.max(1),
                    },
                );
            }
        });

        let progress_app = app_handle.clone();
        let ready_app = app_handle.clone();
        let speakers_app = app_handle.clone();
        let outcome = run_after_stop_with_hooks(
            &store,
            &pipeline,
            meeting_id,
            &config,
            gen_result,
            move |payload| {
                let _ = progress_app.emit(MEETING_NOTES_PROGRESS_EVENT, payload);
            },
            move |title, time_to_notes_secs| {
                let _ = ready_app.emit(
                    MEETING_NOTES_READY_EVENT,
                    NotesReadyPayload {
                        meeting_id,
                        time_to_notes_secs,
                        title: title.to_string(),
                    },
                );
            },
            move || maybe_run_speaker_split_for_app(&speakers_app, meeting_id),
        );
        ticker_done.store(true, Ordering::Relaxed);
        let _ = ticker.join();

        match outcome {
            Ok(AfterStopOutcome::Empty) => {}
            Ok(AfterStopOutcome::Ready { .. }) => {
                let left_out_count = load_left_out_spans(&store, meeting_id)
                    .map(|rows| rows.len())
                    .unwrap_or(0);
                let _ = app_handle.emit(
                    MEETING_EXTRAS_READY_EVENT,
                    ExtrasReadyPayload {
                        meeting_id,
                        left_out_count,
                    },
                );
            }
            Ok(AfterStopOutcome::Failed { reason }) => {
                let _ = app_handle.emit(
                    MEETING_NOTES_FAILED_EVENT,
                    NotesFailedPayload {
                        meeting_id,
                        reason: reason.as_str().to_string(),
                    },
                );
            }
            Err(err) => {
                log::warn!("meeting {meeting_id}: after-stop notes error: {err:#}");
                let _ = store.set_status(meeting_id, "notes_failed");
                pipeline.finish_job_failed(meeting_id, NotesFailReason::Failed);
                let _ = app_handle.emit(
                    MEETING_NOTES_FAILED_EVENT,
                    NotesFailedPayload {
                        meeting_id,
                        reason: NotesFailReason::Failed.as_str().to_string(),
                    },
                );
            }
        }
    });
}

#[cfg(feature = "speaker-split")]
fn maybe_run_speaker_split_for_app(app: &AppHandle, meeting_id: i64) -> Result<usize> {
    let Some(mm) = app.try_state::<Arc<ModelManager>>() else {
        return Ok(0);
    };
    let Ok(seg_path) = mm.get_model_path(ModelManager::PYANNOTE_SEGMENTATION_MODEL_ID) else {
        return Ok(0);
    };
    let Ok(emb_path) = mm.get_model_path(ModelManager::WESPEAKER_EMBEDDING_MODEL_ID) else {
        return Ok(0);
    };
    let dir = crate::portable::app_data_dir(app)?;
    let spans = crate::meeting_speakers::run_speaker_split_with_cap(
        &dir, meeting_id, &seg_path, &emb_path,
    )?;
    if !spans.is_empty() {
        let store = MeetingsStore::open(&dir)?;
        let count = crate::meeting_speakers::speaker_slots(&store, meeting_id)
            .map(|s| s.len())
            .unwrap_or(0);
        let _ = app.emit(
            MEETING_SPEAKERS_READY_EVENT,
            SpeakersReadyPayload {
                meeting_id,
                speaker_count: count,
            },
        );
        return Ok(count);
    }
    Ok(0)
}

#[cfg(not(feature = "speaker-split"))]
fn maybe_run_speaker_split_for_app(_app: &AppHandle, _meeting_id: i64) -> Result<usize> {
    Ok(0)
}

/// Startup helper called from `lib.rs` once `MeetingPipelineState` is managed.
pub fn sweep_interrupted_on_startup(app: &AppHandle) {
    let Some(pipeline) = app.try_state::<Arc<MeetingPipelineState>>() else {
        return;
    };
    let Ok(store) = MeetingsStore::open_app(app) else {
        return;
    };
    if let Err(err) = sweep_interrupted_meetings(&store, pipeline.inner()) {
        log::warn!("meeting startup sweep failed: {err:#}");
    }
}

/// Trigger a real notes Retry and return the current snapshot immediately.
pub fn trigger_retry_meeting_notes(
    app: &AppHandle,
    meeting_id: i64,
) -> Result<PipelineNotesSnapshot, String> {
    let store = MeetingsStore::open_app(app).map_err(|e| e.to_string())?;
    stop_rolling_and_run_after_stop(app, meeting_id);
    let pipeline = app.try_state::<Arc<MeetingPipelineState>>();
    build_meeting_notes_snapshot(&store, pipeline.as_deref().map(Arc::as_ref), meeting_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub fn rename_meeting(app: AppHandle, meeting_id: i64, title: String) -> Result<String, String> {
    let store = MeetingsStore::open_app(&app).map_err(|e| e.to_string())?;
    let pipeline = app.try_state::<Arc<MeetingPipelineState>>();
    rename_meeting_title(
        &store,
        pipeline.as_deref().map(Arc::as_ref),
        meeting_id,
        &title,
    )
    .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::managers::meetings::MeetingChannel;
    use std::fs;

    struct FakeGen {
        fail: bool,
        calls: usize,
        title_reply: String,
    }

    impl FakeGen {
        fn ok(title_reply: &str) -> Self {
            Self {
                fail: false,
                calls: 0,
                title_reply: title_reply.to_string(),
            }
        }
        fn failing() -> Self {
            Self {
                fail: true,
                calls: 0,
                title_reply: String::new(),
            }
        }
    }

    impl NotesGenerator for FakeGen {
        fn generate(&mut self, prompt: &str) -> Result<String> {
            self.calls += 1;
            if self.fail {
                return Err(anyhow!("synthetic generator failure"));
            }
            if prompt.starts_with("Write a short title") {
                return Ok(self.title_reply.clone());
            }
            let mut out = Vec::new();
            for line in prompt.lines() {
                if let Some((id_str, body)) =
                    line.strip_prefix("[S").and_then(|l| l.split_once("] "))
                {
                    if body.contains("Friday") {
                        out.push(format!(
                            "DECISION: Ship desktop release on Friday [S{id_str}]"
                        ));
                    } else if body.contains("budget") {
                        out.push(format!("ACTION: Me | Send the budget sheet [S{id_str}]"));
                    } else if body.contains("question") {
                        out.push(format!("QUESTION: Who owns the checklist? [S{id_str}]"));
                    } else if out.is_empty() {
                        out.push(format!("SUMMARY: Reviewed launch plan [S{id_str}]"));
                    }
                }
            }
            if out.is_empty() {
                if let Some((id_str, _)) = prompt
                    .lines()
                    .find_map(|l| l.strip_prefix("[S").and_then(|r| r.split_once("] ")))
                {
                    out.push(format!("SUMMARY: Covered team updates [S{id_str}]"));
                }
            }
            Ok(out.join("\n"))
        }
    }

    fn seed_meeting_with_audio(
        store: &MeetingsStore,
        root: &std::path::Path,
    ) -> (i64, Vec<i64>, PathBuf) {
        let id = store.start_meeting("Meeting", "parakeet").unwrap();
        let s1 = store
            .insert_segment(id, 5_000, 12_000, MeetingChannel::Me, "Ship on Friday.")
            .unwrap();
        let s2 = store
            .insert_segment(
                id,
                15_000,
                22_000,
                MeetingChannel::Others,
                "Me will send the budget sheet.",
            )
            .unwrap();
        let s3 = store
            .insert_segment(
                id,
                25_000,
                31_000,
                MeetingChannel::Others,
                "Open question: who owns the checklist?",
            )
            .unwrap();
        store.finalise(id, false).unwrap();
        let audio = root.join("meetings").join(id.to_string());
        fs::create_dir_all(&audio).unwrap();
        fs::write(audio.join("mic.wav"), b"mic").unwrap();
        fs::write(audio.join("system.wav"), b"sys").unwrap();
        (id, vec![s1, s2, s3], audio)
    }

    #[test]
    fn after_stop_generates_notes_title_retention_and_markdown_file() {
        let dir = tempfile::tempdir().unwrap();
        let store = MeetingsStore::open(dir.path()).unwrap();
        let pipeline = MeetingPipelineState::new();
        let (id, _segs, audio_dir) = seed_meeting_with_audio(&store, dir.path());
        save_meeting_notepad_points(
            &store,
            id,
            "Ship desktop release Friday\nCheck budget",
            1_000,
        )
        .unwrap();

        let md_dir = dir.path().join("notes-md");
        let cfg = AfterStopConfig {
            delete_audio_after_notes: true,
            save_notes_as_markdown: true,
            notes_markdown_folder: Some(md_dir.clone()),
            ..Default::default()
        };
        let mut progress_events = Vec::new();
        let outcome = run_after_stop_with_generator(
            &store,
            &pipeline,
            id,
            &cfg,
            Ok(FakeGen::ok("\"Desktop Release Launch Plan\"")),
            |ev| progress_events.push(ev),
        )
        .unwrap();

        assert!(!progress_events.is_empty());
        match outcome {
            AfterStopOutcome::Ready {
                title,
                markdown_path,
                used_rolling_pieces,
                ..
            } => {
                assert_eq!(title, "Desktop Release Launch Plan");
                assert!(!used_rolling_pieces);
                let md = markdown_path.expect("markdown written");
                assert!(md.starts_with(fs::canonicalize(&md_dir).unwrap()));
                assert!(fs::read_to_string(&md)
                    .unwrap()
                    .contains("Ship desktop release"));
            }
            other => panic!("unexpected outcome: {other:?}"),
        }
        assert_eq!(store.meeting(id).unwrap().status, "notes_ready");
        assert!(
            !audio_dir.exists(),
            "audio deleted only after notes succeed"
        );

        let snap = build_meeting_notes_snapshot(&store, Some(&pipeline), id).unwrap();
        assert_eq!(snap.state, "ready");
        assert!(snap
            .lines
            .iter()
            .any(|l| l.kind == "decision" && l.from_notepad));
        assert!(snap.lines.iter().any(|l| l.kind == "action"));
        assert!(snap.lines.iter().any(|l| l.kind == "question"));
    }

    #[test]
    fn failed_notes_keeps_transcript_notepad_audio_and_allows_retry() {
        let dir = tempfile::tempdir().unwrap();
        let store = MeetingsStore::open(dir.path()).unwrap();
        let pipeline = MeetingPipelineState::new();
        let (id, _, audio_dir) = seed_meeting_with_audio(&store, dir.path());
        save_meeting_notepad_points(&store, id, "Keep this point safe", 500).unwrap();

        let cfg = AfterStopConfig {
            delete_audio_after_notes: true,
            ..Default::default()
        };
        let failed = run_after_stop_with_generator(
            &store,
            &pipeline,
            id,
            &cfg,
            Ok(FakeGen::failing()),
            |_| {},
        )
        .unwrap();
        assert_eq!(
            failed,
            AfterStopOutcome::Failed {
                reason: NotesFailReason::Failed
            }
        );
        assert_eq!(store.meeting(id).unwrap().status, "notes_failed");
        assert!(audio_dir.join("mic.wav").exists());
        assert!(audio_dir.join("system.wav").exists());
        assert_eq!(store.segments(id).unwrap().len(), 3);
        assert_eq!(read_notepad_points(&store, id).unwrap().len(), 1);

        // Retry with working generator succeeds cleanly.
        let retried = run_after_stop_with_generator(
            &store,
            &pipeline,
            id,
            &cfg,
            Ok(FakeGen::ok("Retry Recovery")),
            |_| {},
        )
        .unwrap();
        assert!(matches!(retried, AfterStopOutcome::Ready { .. }));
        assert_eq!(store.meeting(id).unwrap().status, "notes_ready");
        assert!(!audio_dir.exists());
    }

    #[test]
    fn no_model_and_startup_sweep_mark_failed_with_specific_reasons() {
        let dir = tempfile::tempdir().unwrap();
        let store = MeetingsStore::open(dir.path()).unwrap();
        let pipeline = MeetingPipelineState::new();
        let (id1, _, audio1) = seed_meeting_with_audio(&store, dir.path());
        let (id2, _, _) = seed_meeting_with_audio(&store, dir.path());

        let cfg = AfterStopConfig {
            delete_audio_after_notes: true,
            ..Default::default()
        };
        let out = run_after_stop_with_generator::<FakeGen, _>(
            &store,
            &pipeline,
            id1,
            &cfg,
            Err(NotesFailReason::NoModel),
            |_| {},
        )
        .unwrap();
        assert_eq!(
            out,
            AfterStopOutcome::Failed {
                reason: NotesFailReason::NoModel
            }
        );
        assert!(audio1.exists());
        let snap1 = build_meeting_notes_snapshot(&store, Some(&pipeline), id1).unwrap();
        assert_eq!(snap1.state, "failed");
        assert_eq!(snap1.failure_reason.as_deref(), Some("no_model"));

        // id2 is still at transcript_ready with spoken segments -> startup sweep marks interrupted.
        let swept = sweep_interrupted_meetings(&store, &pipeline).unwrap();
        assert_eq!(swept, vec![id2]);
        let snap2 = build_meeting_notes_snapshot(&store, Some(&pipeline), id2).unwrap();
        assert_eq!(snap2.state, "failed");
        assert_eq!(snap2.failure_reason.as_deref(), Some("interrupted"));
    }

    #[test]
    fn snapshot_hides_uncited_lines_uses_exact_start_ms_and_60_percent_notepad_match() {
        let dir = tempfile::tempdir().unwrap();
        let store = MeetingsStore::open(dir.path()).unwrap();
        let id = store.start_meeting("Sync", "parakeet").unwrap();
        let s1 = store
            .insert_segment(id, 10_000, 15_000, MeetingChannel::Me, "Opening")
            .unwrap();
        let s2 = store
            .insert_segment(id, 95_000, 102_000, MeetingChannel::Others, "Ship Friday")
            .unwrap();
        store
            .save_notes(
                id,
                &[
                    StoredNote {
                        kind: "decision".to_string(),
                        text: "Lock desktop release scope for Friday".to_string(),
                        owner: None,
                        citations: vec![s2],
                    },
                    StoredNote {
                        kind: "summary".to_string(),
                        text: "Hallucinated citation must be hidden".to_string(),
                        owner: None,
                        citations: vec![999_999],
                    },
                    StoredNote {
                        kind: "action".to_string(),
                        text: "Send summary notes".to_string(),
                        owner: None,
                        citations: vec![s1],
                    },
                ],
            )
            .unwrap();
        store.set_status(id, "notes_ready").unwrap();
        save_meeting_notepad_points(&store, id, "desktop release scope Friday", 0).unwrap();

        let snap = build_meeting_notes_snapshot(&store, None, id).unwrap();
        assert_eq!(snap.oversized_count, 1);
        assert_eq!(snap.lines.len(), 2);
        assert_eq!(snap.lines[0].start_ms, Some(95_000));
        assert!(snap.lines[0].from_notepad);
        assert_eq!(snap.lines[1].start_ms, Some(10_000));
        assert!(!snap.lines[1].from_notepad);

        // 3 yes + 3 no fixture pairs for 60% 3+-letter word overlap rule.
        assert!(matches_notepad_point(
            "Ship the desktop release on Friday",
            &["desktop release Friday".to_string()]
        ));
        assert!(matches_notepad_point(
            "Agreed to keep export plain Markdown",
            &["plain markdown export".to_string()]
        ));
        assert!(matches_notepad_point(
            "Budget review moves to Tuesday morning",
            &["budget review Tuesday".to_string()]
        ));
        assert!(!matches_notepad_point(
            "Ship the homepage on Monday",
            &["budget review Tuesday afternoon".to_string()]
        ));
        assert!(!matches_notepad_point(
            "Small coffee chat before kickoff",
            &["security audit signoff timeline".to_string()]
        ));
        assert!(!matches_notepad_point(
            "Updated onboarding icon colors",
            &["database migration rollback plan".to_string()]
        ));
    }

    #[test]
    fn rolling_yields_during_dictation_and_queue_depth_and_guards_multi_piece_meetings() {
        let dir = tempfile::tempdir().unwrap();
        let store = MeetingsStore::open(dir.path()).unwrap();
        let pipeline = MeetingPipelineState::new();
        let id = store.start_meeting("Meeting", "parakeet").unwrap();

        // Populate enough segments to exceed the 4,096 one-shot budget and span > 5 min.
        let filler = "quarterly engineering roadmap milestone discussion ".repeat(16);
        for i in 0..45i64 {
            let start = i * 10_000;
            store
                .insert_segment(
                    id,
                    start,
                    start + 9_000,
                    MeetingChannel::Others,
                    &format!("Segment {i}: Friday {filler}"),
                )
                .unwrap();
        }

        let mut gen = FakeGen::ok("Roadmap Review");
        // Yield when dictation active.
        assert!(rolling_tick_once(&store, &pipeline, id, true, 0, &mut gen)
            .unwrap()
            .is_none());
        // Yield when > 2 chunks queued.
        assert!(rolling_tick_once(&store, &pipeline, id, false, 3, &mut gen)
            .unwrap()
            .is_none());
        // Under the 4,096 one-shot guard, multi-piece meetings never call the
        // model during rolling ticks either.
        assert!(rolling_tick_once(&store, &pipeline, id, false, 2, &mut gen)
            .unwrap()
            .is_none());
        assert_eq!(gen.calls, 0);

        store.finalise(id, false).unwrap();
        let out = run_after_stop_with_generator(
            &store,
            &pipeline,
            id,
            &AfterStopConfig::default(),
            Ok(FakeGen::ok("Engineering Roadmap Review")),
            |_| {},
        )
        .unwrap();
        assert_eq!(
            out,
            AfterStopOutcome::Failed {
                reason: NotesFailReason::Failed
            }
        );
        assert_eq!(store.meeting(id).unwrap().status, "notes_failed");
        assert_eq!(store.segments(id).unwrap().len(), 45);
    }

    #[test]
    fn multi_piece_meeting_never_calls_map_or_merge_on_any_of_three_paths() {
        let dir = tempfile::tempdir().unwrap();
        let store = MeetingsStore::open(dir.path()).unwrap();
        let pipeline = MeetingPipelineState::new();
        let id = store
            .start_meeting("Long Board Review", "parakeet")
            .unwrap();

        // 40 segments (~8,000 est. tokens, > 6 min): unambiguously multi-piece at 4,096.
        let filler = "we reviewed platform architecture and rollout milestones ".repeat(14);
        for i in 0..40i64 {
            let start = i * 10_000;
            store
                .insert_segment(
                    id,
                    start,
                    start + 9_000,
                    MeetingChannel::Others,
                    &format!("Turn {i}: {filler}"),
                )
                .unwrap();
        }
        store.finalise(id, false).unwrap();

        let recorded_prompts = Arc::new(Mutex::new(Vec::<String>::new()));

        // Path 1: Rolling during recording (`rolling_tick_once`).
        let mut rolling_gen = PromptCapturingGen {
            inner: FakeGen::ok("Should Not Run"),
            prompts: Arc::clone(&recorded_prompts),
        };
        let rolling_out =
            rolling_tick_once(&store, &pipeline, id, false, 0, &mut rolling_gen).unwrap();
        assert!(
            rolling_out.is_none(),
            "rolling_tick_once must not produce rolling pieces for a multi-piece meeting"
        );
        assert!(
            recorded_prompts.lock().unwrap().is_empty(),
            "rolling_tick_once must never reach a map or merge prompt"
        );

        // Path 2: After-Stop (`run_after_stop_with_generator`).
        let after_stop_out = run_after_stop_with_generator(
            &store,
            &pipeline,
            id,
            &AfterStopConfig::default(),
            Ok(PromptCapturingGen {
                inner: FakeGen::ok("Should Not Run"),
                prompts: Arc::clone(&recorded_prompts),
            }),
            |_| {},
        )
        .unwrap();
        assert_eq!(
            after_stop_out,
            AfterStopOutcome::Failed {
                reason: NotesFailReason::Failed
            }
        );
        assert_eq!(store.meeting(id).unwrap().status, "notes_failed");
        assert_eq!(store.segments(id).unwrap().len(), 40);
        assert!(
            recorded_prompts.lock().unwrap().is_empty(),
            "after-Stop must never reach a map or merge prompt"
        );

        // Path 3: Retry (`run_after_stop_with_generator` on a `notes_failed` meeting,
        // the exact pipeline entry point invoked by `trigger_retry_meeting_notes` /
        // `retry_meeting_notes`), even if stale rolling pieces were present.
        {
            let mut jobs = pipeline.lock_jobs();
            let entry = jobs.entry(id).or_default();
            entry.pieces.push(vec![(
                (NoteKind::Summary, None),
                "stale rolling piece".to_string(),
                vec![1],
            )]);
            entry.last_segment_id = 1;
        }
        let retry_out = run_after_stop_with_generator(
            &store,
            &pipeline,
            id,
            &AfterStopConfig::default(),
            Ok(PromptCapturingGen {
                inner: FakeGen::ok("Should Not Run"),
                prompts: Arc::clone(&recorded_prompts),
            }),
            |_| {},
        )
        .unwrap();
        assert_eq!(
            retry_out,
            AfterStopOutcome::Failed {
                reason: NotesFailReason::Failed
            }
        );
        assert_eq!(store.meeting(id).unwrap().status, "notes_failed");
        assert_eq!(store.segments(id).unwrap().len(), 40);
        assert!(store.notes(id).unwrap().is_empty());
        assert!(
            recorded_prompts.lock().unwrap().is_empty(),
            "Retry must never reach a map or merge prompt"
        );

        // Direct runner entry point also returns the exact existing OverBudget reason.
        let mut direct_gen = PromptCapturingGen {
            inner: FakeGen::ok("Should Not Run"),
            prompts: Arc::clone(&recorded_prompts),
        };
        let direct_err = crate::meeting_notes_runner::generate_meeting_notes(
            &store,
            id,
            &mut direct_gen,
            &mut |_| {},
        )
        .expect_err("direct runner must refuse multi-piece meeting");
        assert!(
            format!("{direct_err:#}")
                .contains(crate::meeting_notes_runner::NOTES_OVER_BUDGET_REASON),
            "expected existing OverBudget reason, got: {direct_err:#}"
        );
        assert!(
            recorded_prompts.lock().unwrap().is_empty(),
            "direct runner must never reach a map or merge prompt"
        );
    }

    #[test]
    fn user_rename_survives_after_stop_and_legacy_notepad_splits_lines() {
        let dir = tempfile::tempdir().unwrap();
        let store = MeetingsStore::open(dir.path()).unwrap();
        let pipeline = MeetingPipelineState::new();
        let (id, _, _) = seed_meeting_with_audio(&store, dir.path());

        // Legacy single row at at_ms = 0 with newlines reads back split by line.
        store
            .add_notepad_point(id, 0, "Line one\n\nLine two")
            .unwrap();
        let legacy = read_notepad_points(&store, id).unwrap();
        assert_eq!(legacy.len(), 2);
        assert_eq!(legacy[0].1, "Line one");
        assert_eq!(legacy[1].1, "Line two");

        rename_meeting_title(&store, Some(&pipeline), id, "Custom User Title").unwrap();
        let out = run_after_stop_with_generator(
            &store,
            &pipeline,
            id,
            &AfterStopConfig::default(),
            Ok(FakeGen::ok("Model Suggested Title")),
            |_| {},
        )
        .unwrap();
        match out {
            AfterStopOutcome::Ready { title, .. } => assert_eq!(title, "Custom User Title"),
            other => panic!("expected ready: {other:?}"),
        }
        assert_eq!(store.meeting(id).unwrap().title, "Custom User Title");
    }

    #[test]
    fn after_stop_order_emits_notes_ready_before_speakers_and_deletes_audio_after_speakers() {
        let dir = tempfile::tempdir().unwrap();
        let store = MeetingsStore::open(dir.path()).unwrap();
        let pipeline = MeetingPipelineState::new();
        let (id, _, audio_dir) = seed_meeting_with_audio(&store, dir.path());
        store
            .insert_segment(
                id,
                40_000,
                46_000,
                MeetingChannel::Others,
                "Unrelated coffee chat about weekend weather.",
            )
            .unwrap();

        let cfg = AfterStopConfig {
            delete_audio_after_notes: true,
            ..Default::default()
        };
        let events = std::cell::RefCell::new(Vec::<String>::new());
        let out = run_after_stop_with_hooks(
            &store,
            &pipeline,
            id,
            &cfg,
            Ok(FakeGen::ok("Launch Sync")),
            |_| {},
            |_title, _secs| {
                assert!(
                    audio_dir.join("system.wav").exists(),
                    "audio must still exist when notes-ready fires"
                );
                events.borrow_mut().push("notes_ready".to_string());
            },
            || {
                assert!(
                    audio_dir.join("system.wav").exists(),
                    "system.wav must still exist when speaker split runs"
                );
                events.borrow_mut().push("speakers".to_string());
                Ok(0)
            },
        )
        .unwrap();
        assert!(matches!(out, AfterStopOutcome::Ready { .. }));
        assert_eq!(
            events.into_inner(),
            vec!["notes_ready".to_string(), "speakers".to_string()]
        );
        assert!(
            !audio_dir.exists(),
            "audio must be deleted after notes and speaker split complete"
        );
        let snap = build_meeting_notes_snapshot(&store, Some(&pipeline), id).unwrap();
        assert!(
            snap.left_out.iter().any(|l| l.category == "small_talk"),
            "persisted meeting_left_out spans must appear in snapshot"
        );
    }

    struct PromptCapturingGen {
        inner: FakeGen,
        prompts: Arc<Mutex<Vec<String>>>,
    }

    impl NotesGenerator for PromptCapturingGen {
        fn generate(&mut self, prompt: &str) -> Result<String> {
            self.prompts.lock().unwrap().push(prompt.to_string());
            self.inner.generate(prompt)
        }
    }

    #[test]
    fn after_stop_config_carries_stored_template_body_and_omits_when_unset() {
        let dir = tempfile::tempdir().unwrap();
        let store = MeetingsStore::open(dir.path()).unwrap();
        let pipeline = MeetingPipelineState::new();
        let settings = settings::get_default_settings();
        let template_body = "## Their needs\n## Decisions\n## Follow-ups\n";
        let tpl_id = store
            .create_template("Customer call", template_body)
            .unwrap();

        let controller = crate::meeting_controller::MeetingController::default();
        controller.open_consent_with_template(Some(tpl_id));
        let (templated_id, _, _) = seed_meeting_with_audio(&store, dir.path());
        crate::meeting_controller::apply_pending_template(&controller, &store, templated_id);

        let cfg_with = config_from_settings(&settings, &store, templated_id);
        assert_eq!(cfg_with.template_body.as_deref(), Some(template_body));

        let prompts_with = Arc::new(Mutex::new(Vec::new()));
        let out_with = run_after_stop_with_generator(
            &store,
            &pipeline,
            templated_id,
            &cfg_with,
            Ok(PromptCapturingGen {
                inner: FakeGen::ok("Customer Sync"),
                prompts: Arc::clone(&prompts_with),
            }),
            |_| {},
        )
        .unwrap();
        assert!(matches!(out_with, AfterStopOutcome::Ready { .. }));
        assert!(
            prompts_with
                .lock()
                .unwrap()
                .iter()
                .any(|p| p.contains("Their needs:") && p.contains("Follow-ups:")),
            "notes prompt for a meeting started with a template must include its headings"
        );

        controller.open_consent();
        let (default_id, _, _) = seed_meeting_with_audio(&store, dir.path());
        crate::meeting_controller::apply_pending_template(&controller, &store, default_id);

        let cfg_without = config_from_settings(&settings, &store, default_id);
        assert_eq!(cfg_without.template_body, None);

        let prompts_without = Arc::new(Mutex::new(Vec::new()));
        let out_without = run_after_stop_with_generator(
            &store,
            &pipeline,
            default_id,
            &cfg_without,
            Ok(PromptCapturingGen {
                inner: FakeGen::ok("Default Sync"),
                prompts: Arc::clone(&prompts_without),
            }),
            |_| {},
        )
        .unwrap();
        assert!(matches!(out_without, AfterStopOutcome::Ready { .. }));
        assert!(
            prompts_without
                .lock()
                .unwrap()
                .iter()
                .all(|p| !p.contains("User template instructions")),
            "notes prompt for a meeting started without a template must stay on default sections"
        );
    }
}
