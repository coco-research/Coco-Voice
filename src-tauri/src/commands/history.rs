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
    let transcription = tauri::async_runtime::spawn_blocking(move || retranscribe(&tm, samples))
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
    _app: AppHandle,
    history_manager: State<'_, Arc<HistoryManager>>,
    id: i64,
    text: String,
) -> Result<(), String> {
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

    let (transcription_text, post_processed_text) = if has_post_processed {
        (entry.transcription_text, Some(text))
    } else {
        (text, entry.post_processed_text)
    };

    history_manager
        .update_transcription(
            id,
            transcription_text,
            post_processed_text,
            entry.post_process_prompt,
        )
        .map(|_| ())
        .map_err(|e| e.to_string())
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

/// The engine call for a history re-transcribe. During a meeting it takes the same meeting
/// lock as batch dictation, so it waits for an in-flight chunk instead of racing it.
fn retranscribe<R: tauri::Runtime>(
    tm: &TranscriptionManager<R>,
    samples: Vec<f32>,
) -> anyhow::Result<String> {
    #[cfg(feature = "meetings")]
    return tm.transcribe_with_meeting_lock(samples);
    #[cfg(not(feature = "meetings"))]
    tm.transcribe(samples)
}

#[cfg(all(test, feature = "meetings"))]
mod tests {
    use super::*;
    use crate::managers::meetings::test_lock;
    use crate::settings::ModelUnloadTimeout;
    use std::sync::mpsc;
    use std::thread;
    use std::time::{Duration, Instant};

    const PARAKEET: &str = "handy-computer/parakeet-tdt-0.6b-v3-gguf";
    const SAMPLES: usize = 1600;
    const WAIT: Duration = Duration::from_secs(5);
    const ROUNDS: usize = 20;

    fn test_app() -> tauri::App<tauri::test::MockRuntime> {
        tauri::test::mock_builder()
            .plugin(tauri_plugin_store::Builder::default().build())
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock app")
    }

    /// The engine call the history re-transcribe command makes: the real one, not a copy.
    fn history_retranscribe<R: tauri::Runtime>(
        tm: &TranscriptionManager<R>,
        samples: Vec<f32>,
    ) -> anyhow::Result<String> {
        super::retranscribe(tm, samples)
    }

    /// True once a meeting chunk has checked the engine out (it holds the
    /// meeting lock for that whole window). False if no chunk was seen.
    fn wait_until_engine_out(tm: &TranscriptionManager<tauri::test::MockRuntime>) -> bool {
        let deadline = Instant::now() + WAIT;
        while Instant::now() < deadline {
            if !tm.is_engine_available() {
                return true;
            }
            thread::sleep(Duration::from_micros(200));
        }
        false
    }

    #[test]
    fn history_retranscribe_waits_for_meeting_lock() {
        let _serial = test_lock();
        let app = test_app();
        let tm = Arc::new(TranscriptionManager::new_for_test(app.handle().clone()));
        tm.install_fake_engine(PARAKEET);
        tm.set_test_unload_timeout(Some(ModelUnloadTimeout::Never));

        let mut windows = 0;
        for round in 0..ROUNDS {
            let chunk_tm = Arc::clone(&tm);
            let chunker = thread::spawn(move || {
                chunk_tm.transcribe_meeting_chunk(vec![0.2_f32; SAMPLES])
            });

            // While the chunk owns the engine it also holds the meeting lock,
            // so a re-transcribe started now has to queue behind it.
            if !wait_until_engine_out(&tm) {
                let _ = chunker.join().expect("meeting chunk panicked");
                continue;
            }
            windows += 1;

            let (tx, rx) = mpsc::channel();
            let history_tm = Arc::clone(&tm);
            thread::spawn(move || {
                let _ = tx.send(history_retranscribe(&history_tm, vec![0.2_f32; SAMPLES]));
            });

            let outcome = rx
                .recv_timeout(WAIT)
                .expect("history re-transcribe never finished while a chunk ran");
            let chunk = chunker.join().expect("meeting chunk panicked");

            assert!(
                chunk.is_ok(),
                "round {round}: the meeting chunk failed: {:?}",
                chunk.err()
            );
            assert!(
                outcome.is_ok(),
                "round {round}: history re-transcribe failed instead of waiting for the \
                 meeting lock: {:?}",
                outcome.err()
            );
        }

        assert!(
            windows > 0,
            "test never observed a meeting chunk owning the engine"
        );
    }
}
