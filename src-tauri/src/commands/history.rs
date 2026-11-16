use crate::actions::process_transcription_output;
use crate::managers::{
    history::{HistoryManager, PaginatedHistory},
    transcription::TranscriptionManager,
};
use std::sync::Arc;
use tauri::{AppHandle, State};

#[tauri::command]
#[specta::specta]
pub async fn get_history_entries(
    _app: AppHandle,
    history_manager: State<'_, Arc<HistoryManager>>,
    cursor: Option<i64>,
    limit: Option<usize>,
) -> Result<PaginatedHistory, String> {
    history_manager
        .get_history_entries(cursor, limit)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn toggle_history_entry_saved(
    _app: AppHandle,
    history_manager: State<'_, Arc<HistoryManager>>,
    id: i64,
) -> Result<(), String> {
    history_manager
        .toggle_saved_status(id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn get_audio_file_path(
    _app: AppHandle,
    history_manager: State<'_, Arc<HistoryManager>>,
    file_name: String,
) -> Result<String, String> {
    let path = history_manager.get_audio_file_path(&file_name);
    path.to_str()
        .ok_or_else(|| "Invalid file path".to_string())
        .map(|s| s.to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn delete_history_entry(
    _app: AppHandle,
    history_manager: State<'_, Arc<HistoryManager>>,
    id: i64,
) -> Result<(), String> {
    history_manager
        .delete_entry(id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn retry_history_entry_transcription(
    app: AppHandle,
    history_manager: State<'_, Arc<HistoryManager>>,
    transcription_manager: State<'_, Arc<TranscriptionManager>>,
    id: i64,
) -> Result<(), String> {
    let entry = history_manager
        .get_entry_by_id(id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("History entry {} not found", id))?;

    let audio_path = history_manager.get_audio_file_path(&entry.file_name);
    let samples = crate::audio_toolkit::read_wav_samples(&audio_path)
        .map_err(|e| format!("Failed to load audio: {}", e))?;

    if samples.is_empty() {
        return Err("Recording has no audio samples".to_string());
    }

    transcription_manager.initiate_model_load();

    let tm = Arc::clone(&transcription_manager);
    let transcription = tauri::async_runtime::spawn_blocking(move || tm.transcribe(samples))
        .await
        .map_err(|e| format!("Transcription task panicked: {}", e))?
        .map_err(|e| e.to_string())?;

    if transcription.is_empty() {
        return Err("Recording contains no speech".to_string());
    }

    // Re-transcribing a saved recording is never a live spoken correction, so
    // correction mode is off — this only re-runs transcription + post-processing.
    // There is no cancel shortcut on this path, so the local LLM gets no cancel signal.
    let processed = process_transcription_output(
        &app,
        &transcription,
        entry.post_process_requested,
        false,
        None,
    )
    .await;
    history_manager
        .update_transcription(
            id,
            transcription,
            processed.post_processed_text,
            processed.post_process_prompt,
        )
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Persist a user edit to a history entry's transcription text.
///
/// Backs the auto-learn flow: when the user corrects a transcription in the
/// history view, the new text is saved here (reusing `update_transcription`,
/// which also emits the `Updated` event so the list stays in sync) and the
/// word-level diff is turned into correction pairs on the frontend.
///
/// The edit is written back to whichever field the history view displayed: the
/// post-processed text when post-processing produced output, otherwise the raw
/// transcript. This mirrors the frontend `displayTextFor` helper and keeps the
/// displayed value, the persisted value, and the text auto-learn diffs against
/// consistent. The other field and the post-process prompt are preserved as-is.
#[tauri::command]
#[specta::specta]
pub async fn update_history_entry_text(
    app: AppHandle,
    history_manager: State<'_, Arc<HistoryManager>>,
    id: i64,
    text: String,
) -> Result<(), String> {
    use tauri::Emitter;
    let entry = history_manager
        .get_entry_by_id(id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("History entry {} not found", id))?;

    let has_post_processed = entry
        .post_processed_text
        .as_deref()
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false);
    let previous_display = if has_post_processed {
        entry.post_processed_text.clone().unwrap_or_default()
    } else {
        entry.transcription_text.clone()
    };

    let (transcription_text, post_processed_text) = if has_post_processed {
        (entry.transcription_text, Some(text.clone()))
    } else {
        (text.clone(), entry.post_processed_text)
    };

    history_manager
        .update_transcription(
            id,
            transcription_text,
            post_processed_text,
            entry.post_process_prompt,
        )
        .map_err(|e| e.to_string())?;

    let settings = crate::settings::get_settings(&app);
    let suggestions = crate::audio_toolkit::suggest_spelling_corrections(
        &previous_display,
        &text,
        &settings.custom_words,
        &settings.corrections,
    );
    if !suggestions.is_empty() {
        let _ = app.emit("spelling-correction-suggested", &suggestions);
    }
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn suggest_corrections_from_edit(
    app: AppHandle,
    original: String,
    edited: String,
) -> Vec<crate::settings::CorrectionPair> {
    let s = crate::settings::get_settings(&app);
    crate::audio_toolkit::suggest_spelling_corrections(
        &original,
        &edited,
        &s.custom_words,
        &s.corrections,
    )
}

#[tauri::command]
#[specta::specta]
pub fn remember_spelling_fix(app: AppHandle, from: String, to: String) -> Result<bool, String> {
    Ok(crate::settings::update_settings(&app, |s| {
        crate::audio_toolkit::remember_spelling_correction(s, &from, &to)
    }))
}

#[tauri::command]
#[specta::specta]
pub async fn update_history_limit(
    app: AppHandle,
    history_manager: State<'_, Arc<HistoryManager>>,
    limit: usize,
) -> Result<(), String> {
    crate::settings::update_settings(&app, |s| s.history_limit = limit);

    history_manager
        .cleanup_old_entries()
        .map_err(|e| e.to_string())?;

    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn update_recording_retention_period(
    app: AppHandle,
    history_manager: State<'_, Arc<HistoryManager>>,
    period: String,
) -> Result<(), String> {
    use crate::settings::RecordingRetentionPeriod;

    let retention_period = match period.as_str() {
        "never" => RecordingRetentionPeriod::Never,
        "preserve_limit" => RecordingRetentionPeriod::PreserveLimit,
        "days3" => RecordingRetentionPeriod::Days3,
        "weeks2" => RecordingRetentionPeriod::Weeks2,
        "months3" => RecordingRetentionPeriod::Months3,
        _ => return Err(format!("Invalid retention period: {}", period)),
    };

    crate::settings::update_settings(&app, |s| {
        s.recording_retention_period = retention_period;
    });

    history_manager
        .cleanup_old_entries()
        .map_err(|e| e.to_string())?;

    Ok(())
}
