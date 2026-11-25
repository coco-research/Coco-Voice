//! Tauri commands for Plan B2 Part 1 (`X-a`, `X-b`, `X-c`, `X-j`):
//! - Trash (`trash_meeting`, `delete_meeting_forever`, `purge_expired_trash_now`)
//! - Span delete & single-transcript search (`delete_transcript_segments`, `search_meeting_transcript`)
//! - Folders & note templates (`list_meeting_folders`, `create_meeting_folder`,
//!   `rename_meeting_folder`, `delete_meeting_folder`, `set_meeting_folder`,
//!   `list_meeting_templates`, `create_meeting_template`, `update_meeting_template`,
//!   `delete_meeting_template`, `set_folder_last_template`)
//! - Scoped on-device Ask (`ask_meetings_v2`) using local model -> Apple Intelligence -> quotes
//! - Hardened offline Markdown/text import (`count_importable_notes`, `import_meeting_notes_batch`)

#![cfg(feature = "meetings")]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Manager};

use crate::audio_toolkit::{
    is_microphone_access_denied, is_no_input_device_error, list_input_devices, read_wav_samples,
    save_wav_file, VadPolicy,
};
use crate::managers::audio::AudioRecordingManager;
use crate::managers::meetings::{
    DeleteOutcome, FolderFilter, MeetingFolder, MeetingTemplate, MeetingsStore,
};
use crate::managers::model::ModelManager;
use crate::managers::transcription::TranscriptionManager;
use crate::meeting_ask::{AskAnswer, AskScope, LocalAskModel, MeetingAskIndex, SpeechToText};
use crate::meeting_history_ops::{self, SpanDeleteOutcome, TranscriptSearchHit};
use crate::meeting_import::{
    count_importable_notes_paths, import_notes_paths, ImportCountResult, ImportSummary,
};
use crate::meeting_retention::sync_meeting_markdown_file;
use crate::settings::{self, LOCAL_LLM_DEFAULT_MODEL_ID, LOCAL_LLM_PROVIDER_ID};

#[derive(Clone, Debug, Serialize, Deserialize, Type)]
pub struct FolderMeetingItemDto {
    pub id: i64,
    pub title: String,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub notes_ready: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, Type)]
pub struct MeetingFolderViewDto {
    pub id: i64,
    pub name: String,
    pub created_at: i64,
    pub last_template_id: Option<i64>,
    pub meetings: Vec<FolderMeetingItemDto>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Type)]
pub struct AskTurnInputDto {
    pub question: String,
    pub answer: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, Type)]
pub struct AskCitationV2Dto {
    pub meeting_id: i64,
    pub meeting_title: String,
    pub date: String,
    pub at_ms: i64,
    pub segment_id: i64,
    pub label: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, Type)]
pub struct AskMeetingsV2Dto {
    pub question: String,
    pub answer: String,
    pub citations: Vec<AskCitationV2Dto>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Type)]
pub struct TrashedMeetingRowDto {
    pub id: i64,
    pub title: String,
    pub deleted_at: i64,
}

/// On-device Ask model adapter implementing Amendment 1 (`X-A1`):
/// 1. Local clean-up model on macOS when downloaded.
/// 2. Apple Intelligence on Apple Silicon macOS when available.
/// 3. Never calls any cloud provider.
struct OnDeviceAskModel {
    #[cfg(target_os = "macos")]
    local_model_path: Option<PathBuf>,
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    use_apple_intelligence: bool,
}

impl OnDeviceAskModel {
    fn resolve(app: &AppHandle) -> Option<Self> {
        #[cfg(target_os = "macos")]
        let local_model_path = {
            let s = settings::get_settings(app);
            let model_id = s
                .post_process_models
                .get(LOCAL_LLM_PROVIDER_ID)
                .map(String::as_str)
                .filter(|v| !v.trim().is_empty())
                .unwrap_or(LOCAL_LLM_DEFAULT_MODEL_ID);
            app.try_state::<Arc<ModelManager>>().and_then(|mm| {
                mm.get_model_path(model_id)
                    .or_else(|_| mm.get_model_path(LOCAL_LLM_DEFAULT_MODEL_ID))
                    .ok()
            })
        };

        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        let use_apple_intelligence = local_model_path.is_none()
            && crate::apple_intelligence::check_apple_intelligence_availability();

        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        {
            if local_model_path.is_some() || use_apple_intelligence {
                return Some(Self {
                    local_model_path,
                    use_apple_intelligence,
                });
            }
            None
        }

        #[cfg(all(target_os = "macos", not(target_arch = "aarch64")))]
        {
            local_model_path.map(|path| Self {
                local_model_path: Some(path),
            })
        }

        #[cfg(not(target_os = "macos"))]
        {
            let _ = app;
            None
        }
    }
}

impl LocalAskModel for OnDeviceAskModel {
    fn generate(&self, prompt: &str) -> anyhow::Result<String> {
        #[cfg(target_os = "macos")]
        if let Some(ref model_path) = self.local_model_path {
            return tauri::async_runtime::block_on(crate::local_llm::generate_text(
                model_path,
                "Answer questions strictly from the provided transcript excerpts and cite [n] on every line.",
                prompt,
                512,
                None,
                None,
            ))
            .map_err(|e| anyhow::anyhow!("{e}"));
        }
        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        if self.use_apple_intelligence {
            return crate::apple_intelligence::process_text_with_system_prompt(
                "Answer questions strictly from the provided transcript excerpts and cite [n] on every line.",
                prompt,
                512,
            )
            .map_err(|e| anyhow::anyhow!("{e}"));
        }
        Err(anyhow::anyhow!("no on-device ask model available"))
    }
}

fn format_short_date(started_at: i64) -> String {
    DateTime::from_timestamp(started_at, 0)
        .map(|dt| dt.format("%b %-d").to_string())
        .unwrap_or_default()
}

fn sync_ask_index_if_present(app: &AppHandle, context: &str) {
    if let Ok(data_dir) = crate::portable::app_data_dir(app) {
        crate::meeting_ask_index::sync_or_defer(&data_dir, false, context);
    }
}

fn resync_markdown_if_enabled(app: &AppHandle, store: &MeetingsStore, meeting_id: i64) {
    let s = settings::get_settings(app);
    if s.save_notes_as_markdown {
        let folder = s.notes_markdown_folder.as_deref().map(std::path::Path::new);
        let _ = sync_meeting_markdown_file(store, meeting_id, true, folder);
    }
}

// ---------------------------------------------------------------------------
// X-a: Trash, permanent delete, span delete, and single-transcript search
// ---------------------------------------------------------------------------

#[tauri::command(async)]
#[specta::specta]
pub fn trash_meeting(app: AppHandle, id: i64) -> Result<(), String> {
    let store = MeetingsStore::open_app(&app).map_err(|e| e.to_string())?;
    meeting_history_ops::trash_meeting_at(&store, id, Utc::now().timestamp())
        .map_err(|e| e.to_string())?;
    sync_ask_index_if_present(&app, "trash_meeting");
    Ok(())
}

#[tauri::command(async)]
#[specta::specta]
pub fn delete_meeting_forever(app: AppHandle, id: i64) -> Result<bool, String> {
    let store = MeetingsStore::open_app(&app).map_err(|e| e.to_string())?;
    let outcome = store
        .delete_meeting_forever(id)
        .map_err(|e| e.to_string())?;
    if let Ok(data_dir) = crate::portable::app_data_dir(&app) {
        crate::meeting_ask_index::sync_or_defer(&data_dir, true, "delete_meeting_forever");
    }
    Ok(matches!(outcome, DeleteOutcome::DeletedAudioLeft { .. }))
}

#[tauri::command(async)]
#[specta::specta]
pub fn list_trashed_meetings_v2(app: AppHandle) -> Result<Vec<TrashedMeetingRowDto>, String> {
    let store = MeetingsStore::open_app(&app).map_err(|e| e.to_string())?;
    let rows = meeting_history_ops::list_trashed_meetings(&store).map_err(|e| e.to_string())?;
    Ok(rows
        .into_iter()
        .map(|row| TrashedMeetingRowDto {
            id: row.meeting.id,
            title: row.meeting.title,
            deleted_at: row.trashed_at,
        })
        .collect())
}

#[tauri::command(async)]
#[specta::specta]
pub fn restore_trashed_meeting_v2(app: AppHandle, meeting_id: i64) -> Result<(), String> {
    let store = MeetingsStore::open_app(&app).map_err(|e| e.to_string())?;
    meeting_history_ops::restore_meeting(&store, meeting_id).map_err(|e| e.to_string())?;
    sync_ask_index_if_present(&app, "restore_trashed_meeting_v2");
    Ok(())
}

#[tauri::command(async)]
#[specta::specta]
pub fn purge_expired_trash_now(app: AppHandle) -> Result<usize, String> {
    let store = MeetingsStore::open_app(&app).map_err(|e| e.to_string())?;
    meeting_history_ops::purge_expired_trash(&store, Utc::now().timestamp())
        .map_err(|e| e.to_string())
}

#[tauri::command(async)]
#[specta::specta]
pub fn delete_transcript_segments(
    app: AppHandle,
    meeting_id: i64,
    segment_ids: Vec<i64>,
) -> Result<SpanDeleteOutcome, String> {
    let store = MeetingsStore::open_app(&app).map_err(|e| e.to_string())?;
    let outcome =
        meeting_history_ops::delete_transcript_segments_by_ids(&store, meeting_id, &segment_ids)
            .map_err(|e| e.to_string())?;
    resync_markdown_if_enabled(&app, &store, meeting_id);
    Ok(outcome)
}

#[tauri::command(async)]
#[specta::specta]
pub fn delete_transcript_span_secure(
    app: AppHandle,
    meeting_id: i64,
    start_ms: i64,
    end_ms: i64,
) -> Result<SpanDeleteOutcome, String> {
    let store = MeetingsStore::open_app(&app).map_err(|e| e.to_string())?;
    let outcome = meeting_history_ops::delete_transcript_span(&store, meeting_id, start_ms, end_ms)
        .map_err(|e| e.to_string())?;
    resync_markdown_if_enabled(&app, &store, meeting_id);
    Ok(outcome)
}

#[tauri::command(async)]
#[specta::specta]
pub fn search_meeting_transcript(
    app: AppHandle,
    meeting_id: i64,
    query: String,
) -> Result<Vec<TranscriptSearchHit>, String> {
    let store = MeetingsStore::open_app(&app).map_err(|e| e.to_string())?;
    meeting_history_ops::search_meeting_transcript_matches(&store, meeting_id, &query)
        .map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// X-b: Folders and note templates
// ---------------------------------------------------------------------------

#[tauri::command(async)]
#[specta::specta]
pub fn list_meeting_folders(app: AppHandle) -> Result<Vec<MeetingFolderViewDto>, String> {
    let store = MeetingsStore::open_app(&app).map_err(|e| e.to_string())?;
    let folders = store.list_folders().map_err(|e| e.to_string())?;
    let mut out = Vec::with_capacity(folders.len());
    for folder in folders {
        let rows = store
            .list_meetings_in_folder(FolderFilter::Folder(folder.id))
            .map_err(|e| e.to_string())?;
        let mut meetings = Vec::with_capacity(rows.len());
        for m in rows {
            let notes_ready = m.status == "notes_ready"
                || m.status == "imported"
                || !store.notes(m.id).map_err(|e| e.to_string())?.is_empty();
            meetings.push(FolderMeetingItemDto {
                id: m.id,
                title: m.title,
                started_at: m.started_at,
                ended_at: m.ended_at,
                notes_ready,
            });
        }
        out.push(MeetingFolderViewDto {
            id: folder.id,
            name: folder.name,
            created_at: folder.created_at,
            last_template_id: folder.last_template_id,
            meetings,
        });
    }
    Ok(out)
}

#[tauri::command(async)]
#[specta::specta]
pub fn create_meeting_folder(app: AppHandle, name: String) -> Result<MeetingFolder, String> {
    let store = MeetingsStore::open_app(&app).map_err(|e| e.to_string())?;
    let id = store.create_folder(&name).map_err(|e| e.to_string())?;
    store
        .list_folders()
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|f| f.id == id)
        .ok_or_else(|| "created folder not found".to_string())
}

#[tauri::command(async)]
#[specta::specta]
pub fn rename_meeting_folder(app: AppHandle, id: i64, name: String) -> Result<(), String> {
    let store = MeetingsStore::open_app(&app).map_err(|e| e.to_string())?;
    store.rename_folder(id, &name).map_err(|e| e.to_string())?;
    sync_ask_index_if_present(&app, "rename_meeting_folder");
    Ok(())
}

#[tauri::command(async)]
#[specta::specta]
pub fn delete_meeting_folder(app: AppHandle, id: i64) -> Result<(), String> {
    let store = MeetingsStore::open_app(&app).map_err(|e| e.to_string())?;
    store.delete_folder(id).map_err(|e| e.to_string())?;
    sync_ask_index_if_present(&app, "delete_meeting_folder");
    Ok(())
}

#[tauri::command(async)]
#[specta::specta]
pub fn set_meeting_folder(
    app: AppHandle,
    meeting_id: i64,
    folder_id: Option<i64>,
) -> Result<(), String> {
    let store = MeetingsStore::open_app(&app).map_err(|e| e.to_string())?;
    store
        .move_meetings_to_folder(&[meeting_id], folder_id)
        .map_err(|e| e.to_string())?;
    let folder_name = match folder_id {
        Some(fid) => store
            .list_folders()
            .map_err(|e| e.to_string())?
            .into_iter()
            .find(|f| f.id == fid)
            .map(|f| f.name),
        None => None,
    };
    if let Ok(data_dir) = crate::portable::app_data_dir(&app) {
        if let Ok(idx) = MeetingAskIndex::open(&data_dir) {
            let _ = idx.set_folder(meeting_id, folder_name.as_deref());
        }
        crate::meeting_ask_index::sync_or_defer(&data_dir, false, "set_meeting_folder");
    }
    Ok(())
}

#[tauri::command(async)]
#[specta::specta]
pub fn list_meeting_templates(app: AppHandle) -> Result<Vec<MeetingTemplate>, String> {
    let store = MeetingsStore::open_app(&app).map_err(|e| e.to_string())?;
    store.list_templates().map_err(|e| e.to_string())
}

#[tauri::command(async)]
#[specta::specta]
pub fn create_meeting_template(
    app: AppHandle,
    name: String,
    body: String,
) -> Result<MeetingTemplate, String> {
    let store = MeetingsStore::open_app(&app).map_err(|e| e.to_string())?;
    let id = store
        .create_template(&name, &body)
        .map_err(|e| e.to_string())?;
    store
        .list_templates()
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|t| t.id == id)
        .ok_or_else(|| "created template not found".to_string())
}

#[tauri::command(async)]
#[specta::specta]
pub fn update_meeting_template(
    app: AppHandle,
    id: i64,
    name: String,
    body: String,
) -> Result<(), String> {
    let store = MeetingsStore::open_app(&app).map_err(|e| e.to_string())?;
    store
        .update_template(id, &name, &body)
        .map_err(|e| e.to_string())
}

#[tauri::command(async)]
#[specta::specta]
pub fn delete_meeting_template(app: AppHandle, id: i64) -> Result<(), String> {
    let store = MeetingsStore::open_app(&app).map_err(|e| e.to_string())?;
    store.delete_template(id).map_err(|e| e.to_string())
}

#[tauri::command(async)]
#[specta::specta]
pub fn set_folder_last_template(
    app: AppHandle,
    folder_id: i64,
    template_id: Option<i64>,
) -> Result<(), String> {
    let store = MeetingsStore::open_app(&app).map_err(|e| e.to_string())?;
    store
        .set_folder_last_template(folder_id, template_id)
        .map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// X-c: Scoped on-device Ask & Ask by voice
// ---------------------------------------------------------------------------

const ASK_BY_VOICE_BINDING_ID: &str = "ask_by_voice";
static ASK_BY_VOICE_ACTIVE: AtomicBool = AtomicBool::new(false);
static ASK_BY_VOICE_CANCEL_GEN: AtomicU64 = AtomicU64::new(0);
static EPHEMERAL_WAV_SEQ: AtomicU64 = AtomicU64::new(1);

/// RAII guard for an ephemeral question WAV file. Unconditionally unlinks the
/// temporary file on drop across both success and error paths so Ask by voice
/// leaves zero audio files on disk.
struct TempWavGuard(PathBuf);

impl TempWavGuard {
    fn write_ephemeral_wav(dir: &Path, samples_16khz: &[f32]) -> anyhow::Result<Self> {
        let seq = EPHEMERAL_WAV_SEQ.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let path = dir.join(format!("ask-by-voice-{pid}-{seq}.wav"));
        let guard = Self(path);
        save_wav_file(&guard.0, samples_16khz)?;
        Ok(guard)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempWavGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

struct TranscriptionManagerStt<'a>(&'a TranscriptionManager);

impl SpeechToText for TranscriptionManagerStt<'_> {
    fn transcribe_pcm(&self, samples_16khz: &[f32]) -> anyhow::Result<String> {
        self.0.transcribe_with_meeting_lock(samples_16khz.to_vec())
    }
}

fn map_mic_start_error(err: &str) -> String {
    if is_microphone_access_denied(err) || is_no_input_device_error(err) {
        format!("mic_permission_denied: {err}")
    } else {
        err.to_string()
    }
}

fn resolve_ask_scope(
    scope: &str,
    meeting_id: Option<i64>,
    folder_name: Option<String>,
    meeting_ids: Option<Vec<i64>>,
) -> Result<AskScope, String> {
    match scope {
        "this" | "this_meeting" | "single" => match meeting_id {
            Some(mid) => Ok(AskScope::OneMeeting(mid)),
            None => Err("meeting_id is required for single-meeting Ask".to_string()),
        },
        "folder" => match folder_name
            .map(|f| f.trim().to_string())
            .filter(|f| !f.is_empty())
        {
            Some(name) => Ok(AskScope::Folder(name)),
            None => Ok(AskScope::AllMeetings),
        },
        "picked" => Ok(AskScope::PickedSet(meeting_ids.unwrap_or_default())),
        _ => Ok(AskScope::AllMeetings),
    }
}

fn ask_answer_to_dto(ans: AskAnswer) -> AskMeetingsV2Dto {
    let citations = ans
        .citations
        .into_iter()
        .map(|c| {
            let date = format_short_date(c.started_at);
            let label = if date.is_empty() {
                format!("{} · {}", c.meeting_title, c.timestamp)
            } else {
                format!("{}, {} · {}", c.meeting_title, date, c.timestamp)
            };
            AskCitationV2Dto {
                meeting_id: c.meeting_id,
                meeting_title: c.meeting_title,
                date,
                at_ms: c.start_ms,
                segment_id: c.segment_id,
                label,
            }
        })
        .collect();

    AskMeetingsV2Dto {
        question: ans.question,
        answer: ans.answer,
        citations,
    }
}

/// Transcribe recorded 16kHz PCM samples through a temporary WAV that is
/// unconditionally removed on drop. Never writes to dictation history or paste.
fn transcribe_ephemeral_take(
    temp_dir: &Path,
    samples_16khz: &[f32],
    stt: &dyn SpeechToText,
) -> anyhow::Result<String> {
    if samples_16khz.is_empty() {
        return Ok(String::new());
    }
    let wav_guard = TempWavGuard::write_ephemeral_wav(temp_dir, samples_16khz)?;
    let wav_samples = read_wav_samples(wav_guard.path())?;
    if wav_samples.is_empty() {
        return Ok(String::new());
    }
    let text = stt.transcribe_pcm(&wav_samples)?;
    Ok(text.trim().to_string())
}

/// Core testable Ask-by-voice execution path: writes 16kHz PCM samples to an
/// ephemeral WAV guarded by [`TempWavGuard`] inside `temp_dir`, invokes
/// [`MeetingAskIndex::ask_by_voice`] (or `ask_by_voice_with_history` when
/// conversation history is present), and unconditionally removes the WAV on drop.
pub fn run_ask_by_voice_take_in(
    temp_dir: &Path,
    idx: &MeetingAskIndex,
    scope: &AskScope,
    samples_16khz: &[f32],
    history: &[(String, String)],
    stt: &dyn SpeechToText,
    model: Option<&dyn LocalAskModel>,
) -> anyhow::Result<AskAnswer> {
    if samples_16khz.is_empty() {
        anyhow::bail!("question recording contains no audio samples");
    }
    let wav_guard = TempWavGuard::write_ephemeral_wav(temp_dir, samples_16khz)?;
    if history.is_empty() {
        idx.ask_by_voice(scope, wav_guard.path(), stt, model)
    } else {
        idx.ask_by_voice_with_history(scope, wav_guard.path(), history, stt, model)
    }
}

pub fn run_ask_by_voice_take(
    idx: &MeetingAskIndex,
    scope: &AskScope,
    samples_16khz: &[f32],
    history: &[(String, String)],
    stt: &dyn SpeechToText,
    model: Option<&dyn LocalAskModel>,
) -> anyhow::Result<AskAnswer> {
    run_ask_by_voice_take_in(
        &std::env::temp_dir(),
        idx,
        scope,
        samples_16khz,
        history,
        stt,
        model,
    )
}

#[tauri::command]
#[specta::specta]
pub fn start_ask_by_voice(app: AppHandle) -> Result<(), String> {
    match list_input_devices() {
        Ok(devices) if devices.is_empty() => {
            return Err("mic_permission_denied: No input device found".to_string());
        }
        Err(err) => {
            return Err(map_mic_start_error(&err.to_string()));
        }
        Ok(_) => {}
    }

    let rm = app
        .try_state::<Arc<AudioRecordingManager>>()
        .ok_or_else(|| "audio recording manager not available".to_string())?;

    let gen = rm.cancel_generation();
    ASK_BY_VOICE_CANCEL_GEN.store(gen, Ordering::Release);
    rm.try_start_recording(ASK_BY_VOICE_BINDING_ID, VadPolicy::Disabled)
        .map_err(|err| map_mic_start_error(&err))?;
    ASK_BY_VOICE_ACTIVE.store(true, Ordering::Release);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn cancel_ask_by_voice(app: AppHandle) -> Result<(), String> {
    if !ASK_BY_VOICE_ACTIVE.swap(false, Ordering::AcqRel) {
        return Ok(());
    }
    let gen = ASK_BY_VOICE_CANCEL_GEN.load(Ordering::Acquire);
    if let Some(rm) = app.try_state::<Arc<AudioRecordingManager>>() {
        if !rm.was_cancelled_since(gen) && rm.is_recording() {
            rm.cancel_recording();
        }
    }
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn finish_ask_by_voice(app: AppHandle) -> Result<String, String> {
    ASK_BY_VOICE_ACTIVE.store(false, Ordering::Release);
    let gen = ASK_BY_VOICE_CANCEL_GEN.load(Ordering::Acquire);
    let rm = app
        .try_state::<Arc<AudioRecordingManager>>()
        .ok_or_else(|| "audio recording manager not available".to_string())?;
    let Some(samples) = rm.stop_recording(ASK_BY_VOICE_BINDING_ID, gen) else {
        return Ok(String::new());
    };
    if samples.is_empty() {
        return Ok(String::new());
    }
    let tm = app
        .try_state::<Arc<TranscriptionManager>>()
        .ok_or_else(|| "transcription manager not available".to_string())?;
    let stt = TranscriptionManagerStt(&tm);
    transcribe_ephemeral_take(&std::env::temp_dir(), &samples, &stt).map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub fn ask_meetings_by_voice(
    app: AppHandle,
    scope: String,
    meeting_id: Option<i64>,
    folder_name: Option<String>,
    meeting_ids: Option<Vec<i64>>,
    history: Option<Vec<AskTurnInputDto>>,
) -> Result<AskMeetingsV2Dto, String> {
    ASK_BY_VOICE_ACTIVE.store(false, Ordering::Release);
    let gen = ASK_BY_VOICE_CANCEL_GEN.load(Ordering::Acquire);
    let rm = app
        .try_state::<Arc<AudioRecordingManager>>()
        .ok_or_else(|| "audio recording manager not available".to_string())?;
    let samples = rm
        .stop_recording(ASK_BY_VOICE_BINDING_ID, gen)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "question recording was empty or cancelled".to_string())?;

    let data_dir = crate::portable::app_data_dir(&app).map_err(|e| e.to_string())?;
    let idx = MeetingAskIndex::open(&data_dir).map_err(|e| e.to_string())?;
    let ask_scope = resolve_ask_scope(&scope, meeting_id, folder_name, meeting_ids)?;
    let history_pairs: Vec<(String, String)> = history
        .unwrap_or_default()
        .into_iter()
        .map(|turn| (turn.question, turn.answer))
        .collect();
    let tm = app
        .try_state::<Arc<TranscriptionManager>>()
        .ok_or_else(|| "transcription manager not available".to_string())?;
    let stt = TranscriptionManagerStt(&tm);
    let model = OnDeviceAskModel::resolve(&app);
    let model_ref: Option<&dyn LocalAskModel> = model.as_ref().map(|m| m as &dyn LocalAskModel);
    let ans = run_ask_by_voice_take(&idx, &ask_scope, &samples, &history_pairs, &stt, model_ref)
        .map_err(|e| e.to_string())?;
    Ok(ask_answer_to_dto(ans))
}

#[tauri::command]
#[specta::specta]
pub fn ask_meetings_v2(
    app: AppHandle,
    question: String,
    scope: String,
    meeting_id: Option<i64>,
    folder_name: Option<String>,
    meeting_ids: Option<Vec<i64>>,
    history: Option<Vec<AskTurnInputDto>>,
) -> Result<AskMeetingsV2Dto, String> {
    let data_dir = crate::portable::app_data_dir(&app).map_err(|e| e.to_string())?;
    let idx = MeetingAskIndex::open(&data_dir).map_err(|e| e.to_string())?;
    let ask_scope = resolve_ask_scope(&scope, meeting_id, folder_name, meeting_ids)?;

    let history_pairs: Vec<(String, String)> = history
        .unwrap_or_default()
        .into_iter()
        .map(|turn| (turn.question, turn.answer))
        .collect();
    let model = OnDeviceAskModel::resolve(&app);
    let model_ref: Option<&dyn LocalAskModel> = model.as_ref().map(|m| m as &dyn LocalAskModel);
    let ans = idx
        .ask_with_history(&ask_scope, &question, &history_pairs, model_ref)
        .map_err(|e| e.to_string())?;

    Ok(ask_answer_to_dto(ans))
}

// ---------------------------------------------------------------------------
// X-j: Hardened offline Markdown/text import
// ---------------------------------------------------------------------------

#[tauri::command(async)]
#[specta::specta]
pub fn count_importable_notes(paths: Vec<String>) -> Result<ImportCountResult, String> {
    let pb: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
    count_importable_notes_paths(&pb).map_err(|e| e.to_string())
}

#[tauri::command(async)]
#[specta::specta]
pub fn import_meeting_notes_batch(
    app: AppHandle,
    paths: Vec<String>,
    folder: Option<String>,
) -> Result<ImportSummary, String> {
    let store = MeetingsStore::open_app(&app).map_err(|e| e.to_string())?;
    let pb: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
    let summary = import_notes_paths(&store, &pb, folder.as_deref()).map_err(|e| e.to_string())?;
    sync_ask_index_if_present(&app, "import_meeting_notes_batch");
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::meeting_ask_index::tests::seed_three_meetings;

    struct FakeStt(&'static str);

    impl SpeechToText for FakeStt {
        fn transcribe_pcm(&self, samples_16khz: &[f32]) -> anyhow::Result<String> {
            assert!(!samples_16khz.is_empty(), "STT must receive recorded PCM");
            Ok(self.0.to_string())
        }
    }

    struct FailingStt;

    impl SpeechToText for FailingStt {
        fn transcribe_pcm(&self, _samples_16khz: &[f32]) -> anyhow::Result<String> {
            anyhow::bail!("simulated stt failure")
        }
    }

    fn count_wav_files(dir: &Path) -> usize {
        fn walk(dir: &Path) -> usize {
            let Ok(entries) = std::fs::read_dir(dir) else {
                return 0;
            };
            let mut n = 0;
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_dir() {
                    n += walk(&p);
                } else if p
                    .extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(|e| e.eq_ignore_ascii_case("wav"))
                {
                    n += 1;
                }
            }
            n
        }
        walk(dir)
    }

    #[test]
    fn ac_x45_ask_by_voice_matches_typed_ask_and_leaves_no_audio_file() {
        let data_dir = tempfile::tempdir().expect("data tempdir");
        let wav_dir = tempfile::tempdir().expect("wav tempdir");
        seed_three_meetings(data_dir.path());
        let idx = MeetingAskIndex::open(data_dir.path()).expect("open ask index");

        let typed_question = "What is the Thursday deadline, and did design critique move it?";
        let scope = AskScope::AllMeetings;
        let expected = idx
            .ask(&scope, typed_question, None)
            .expect("typed ask succeeds");
        assert_eq!(expected.citations.len(), 2);

        let fixture_pcm = vec![0.12_f32; 16_000];
        let spoken = run_ask_by_voice_take_in(
            wav_dir.path(),
            &idx,
            &scope,
            &fixture_pcm,
            &[],
            &FakeStt(typed_question),
            None,
        )
        .expect("ask by voice succeeds");

        assert_eq!(spoken.question, expected.question);
        assert_eq!(spoken.answer, expected.answer);
        assert_eq!(spoken.citations, expected.citations);

        let wav_files_after_ok = count_wav_files(wav_dir.path()) + count_wav_files(data_dir.path());
        assert_eq!(
            wav_files_after_ok, 0,
            "Ask by voice must leave zero WAV files after success"
        );

        // Error path must also unlink the ephemeral WAV via TempWavGuard Drop.
        let err = run_ask_by_voice_take_in(
            wav_dir.path(),
            &idx,
            &scope,
            &fixture_pcm,
            &[],
            &FailingStt,
            None,
        )
        .expect_err("failing STT must propagate error");
        assert!(err.to_string().contains("simulated stt failure"));
        let wav_files_after_err =
            count_wav_files(wav_dir.path()) + count_wav_files(data_dir.path());
        assert_eq!(
            wav_files_after_err, 0,
            "Ask by voice must leave zero WAV files after error"
        );

        // Standalone ephemeral transcription path (used by finish_ask_by_voice) also leaves zero WAVs.
        let text = transcribe_ephemeral_take(
            wav_dir.path(),
            &fixture_pcm,
            &FakeStt("  what is due thursday  "),
        )
        .expect("ephemeral transcription succeeds");
        assert_eq!(text, "what is due thursday");
        assert_eq!(count_wav_files(wav_dir.path()), 0);

        let _ = transcribe_ephemeral_take(wav_dir.path(), &fixture_pcm, &FailingStt)
            .expect_err("ephemeral transcription error propagates");
        assert_eq!(count_wav_files(wav_dir.path()), 0);

        // Mic denied / missing input device map cleanly to `mic_permission_denied`.
        assert!(map_mic_start_error("Permission denied").starts_with("mic_permission_denied:"));
        assert!(map_mic_start_error("No input device found").starts_with("mic_permission_denied:"));

        println!(
            "AC_X45_RUST_PROOF: matches_typed_ask=true citations={} wav_files_after_ok={} wav_files_after_err={}",
            spoken.citations.len(),
            wav_files_after_ok,
            wav_files_after_err
        );
    }
}
