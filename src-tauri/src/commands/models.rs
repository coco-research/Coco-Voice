use crate::managers::model::{
    download_failure_cause, ModelDownloadFailed, ModelInfo, ModelManager,
};
use crate::managers::transcription::{ModelStateEvent, TranscriptionManager};
use crate::settings::{get_settings, update_settings, ModelUnloadTimeout};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State};

#[tauri::command]
#[specta::specta]
pub async fn get_available_models(
    model_manager: State<'_, Arc<ModelManager>>,
) -> Result<Vec<ModelInfo>, String> {
    Ok(model_manager.get_available_models())
}

#[tauri::command]
#[specta::specta]
pub async fn get_model_info(
    model_manager: State<'_, Arc<ModelManager>>,
    model_id: String,
) -> Result<Option<ModelInfo>, String> {
    Ok(model_manager.get_model_info(&model_id))
}

/// Re-scan local sources (custom models dir + shared HF cache) for models added
/// since launch
#[tauri::command]
#[specta::specta]
pub async fn rescan_local_models(
    model_manager: State<'_, Arc<ModelManager>>,
) -> Result<(), String> {
    let mm = model_manager.inner().clone();
    tokio::task::spawn_blocking(move || mm.rescan_local_models())
        .await
        .map_err(|e| format!("rescan task panicked: {e}"))?
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn download_model(
    app_handle: AppHandle,
    model_manager: State<'_, Arc<ModelManager>>,
    model_id: String,
) -> Result<(), String> {
    let result = model_manager.download_model(&model_id).await;

    if let Err(ref e) = result {
        let _ = app_handle.emit(
            "model-download-failed",
            ModelDownloadFailed {
                model_id,
                error: e.to_string(),
                cause: download_failure_cause(e),
            },
        );
    }

    result.map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn delete_model(
    app_handle: AppHandle,
    model_manager: State<'_, Arc<ModelManager>>,
    transcription_manager: State<'_, Arc<TranscriptionManager>>,
    model_id: String,
) -> Result<(), String> {
    // If deleting the active model, unload it and clear the setting
    let settings = get_settings(&app_handle);
    if settings.selected_model == model_id {
        transcription_manager
            .unload_model()
            .map_err(|e| format!("Failed to unload model: {}", e))?;

        update_settings(&app_handle, |s| s.selected_model = String::new());
    }

    model_manager
        .delete_model(&model_id)
        .map_err(|e| e.to_string())
}

/// Shared logic for switching the active model, used by both the Tauri command
/// and the tray menu handler.
///
/// Validates the model, updates the persisted setting, and loads the model
/// unless the unload timeout is set to "Immediately" (in which case the model
/// will be loaded on-demand during the next transcription).
pub fn switch_active_model(app: &AppHandle, model_id: &str) -> Result<(), String> {
    let model_manager = app.state::<Arc<ModelManager>>();
    let transcription_manager = app.state::<Arc<TranscriptionManager>>();
    switch_active_model_inner(app, &model_manager, &transcription_manager, model_id)
}

pub(crate) fn switch_active_model_inner<R: tauri::Runtime>(
    app: &AppHandle<R>,
    model_manager: &ModelManager,
    transcription_manager: &TranscriptionManager<R>,
    model_id: &str,
) -> Result<(), String> {
    // Atomically claim the loading slot — prevents concurrent model loads
    // from tray double-clicks or overlapping commands. The guard resets the
    // flag on drop (including early returns, errors, and panics).
    let _loading_guard = transcription_manager
        .try_start_loading()
        .ok_or_else(|| "Model load already in progress".to_string())?;

    // Check if model exists and is available
    let model_info = model_manager
        .get_model_info(model_id)
        .ok_or_else(|| format!("Model not found: {}", model_id))?;

    if !model_info.is_downloaded {
        return Err(format!("Model not downloaded: {}", model_id));
    }

    // Local cleanup models are registered for download only, never selectable.
    if !model_info.engine_type.is_speech() {
        return Err(format!("Model {} is not a speech model", model_id));
    }

    let settings = get_settings(app);
    let unload_timeout = settings.model_unload_timeout;
    let old_model = settings.selected_model.clone();
    let old_onboarding_completed = settings.onboarding_completed;

    // Persist the new selection early so the frontend sees the correct model
    // when it reacts to events emitted by load_model. Re-read under the lock
    // and write only these two fields; load_model below is slow.
    update_settings(app, |s| {
        s.selected_model = model_id.to_string();
        s.onboarding_completed = true;
    });

    // M-c5 (MC5-N1, MC5-N2): while a meeting holds a pinned model, save the new
    // dictation choice, notify the frontend, log once, and do not load or swap
    // the engine — even if unload is set to Immediately.
    #[cfg(feature = "meetings")]
    if let Some(pinned_id) = transcription_manager.pinned_meeting_model() {
        let _ = app.emit(
            "model-state-changed",
            ModelStateEvent {
                event_type: "selection_changed".to_string(),
                model_id: Some(model_id.to_string()),
                model_name: Some(model_info.name.clone()),
                error: None,
            },
        );
        log::info!(
            "Model selection changed to {} (not loading: meeting holds {} until it ends)",
            model_id,
            pinned_id
        );
        return Ok(());
    }

    // Skip eager loading if unload is set to "Immediately" — the model
    // will be loaded on-demand during the next transcription.
    if unload_timeout == ModelUnloadTimeout::Immediately {
        // Notify frontend — load_model won't be called so no events
        // would otherwise be emitted.
        let _ = app.emit(
            "model-state-changed",
            ModelStateEvent {
                event_type: "selection_changed".to_string(),
                model_id: Some(model_id.to_string()),
                model_name: Some(model_info.name.clone()),
                error: None,
            },
        );
        log::info!(
            "Model selection changed to {} (not loading — unload set to Immediately).",
            model_id
        );
        return Ok(());
    }

    // Load the model. On failure, revert the persisted selection.
    if let Err(e) = transcription_manager.load_model(model_id) {
        update_settings(app, |s| {
            s.selected_model = old_model;
            s.onboarding_completed = old_onboarding_completed;
        });
        return Err(e.to_string());
    }

    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn set_active_model(
    app_handle: AppHandle,
    _model_manager: State<'_, Arc<ModelManager>>,
    _transcription_manager: State<'_, Arc<TranscriptionManager>>,
    model_id: String,
) -> Result<(), String> {
    switch_active_model(&app_handle, &model_id)
}

#[tauri::command]
#[specta::specta]
pub async fn get_current_model(app_handle: AppHandle) -> Result<String, String> {
    let settings = get_settings(&app_handle);
    Ok(settings.selected_model)
}

#[tauri::command]
#[specta::specta]
pub async fn get_transcription_model_status(
    transcription_manager: State<'_, Arc<TranscriptionManager>>,
) -> Result<Option<String>, String> {
    Ok(transcription_manager.get_current_model())
}

#[tauri::command]
#[specta::specta]
pub async fn is_model_loading(
    transcription_manager: State<'_, Arc<TranscriptionManager>>,
) -> Result<bool, String> {
    // Check if transcription manager has a loaded model
    let current_model = transcription_manager.get_current_model();
    Ok(current_model.is_none())
}

#[tauri::command]
#[specta::specta]
pub async fn cancel_download(
    model_manager: State<'_, Arc<ModelManager>>,
    model_id: String,
) -> Result<(), String> {
    model_manager
        .cancel_download(&model_id)
        .map_err(|e| e.to_string())
}

#[cfg(all(test, feature = "meetings"))]
mod tests {
    use super::*;
    use crate::managers::model::{EngineType, ModelSource};
    use std::collections::HashMap;
    use std::time::{Duration, Instant};

    struct RestoreSettings<'a, R: tauri::Runtime> {
        app: &'a AppHandle<R>,
        selected_model: String,
        onboarding_completed: bool,
        unload_timeout: ModelUnloadTimeout,
    }

    impl<R: tauri::Runtime> Drop for RestoreSettings<'_, R> {
        fn drop(&mut self) {
            update_settings(self.app, |s| {
                s.selected_model = self.selected_model.clone();
                s.onboarding_completed = self.onboarding_completed;
                s.model_unload_timeout = self.unload_timeout;
            });
        }
    }

    fn downloaded_speech_model(id: &str, name: &str, filename: &str) -> ModelInfo {
        ModelInfo {
            id: id.to_string(),
            name: name.to_string(),
            description: "Test speech model".to_string(),
            filename: filename.to_string(),
            source: ModelSource::Url {
                url: "https://example.invalid/model.gguf".to_string(),
                sha256: None,
            },
            size_mb: 1,
            is_downloaded: true,
            is_downloading: false,
            partial_size: 0,
            is_directory: false,
            engine_type: EngineType::TranscribeCpp,
            accuracy_score: 0.8,
            speed_score: 0.8,
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

    /// T-mc8 (M-c5): switching the dictation model during a pinned meeting persists
    /// the new choice without dropping or swapping the pinned meeting engine; after
    /// the meeting ends, `clear_meeting_pin` arms reload-on-next-use so the next
    /// dictation reloads the newly chosen model (and also restores dictation's prior
    /// model when no switch occurred during the meeting, C1).
    #[test]
    fn t_mc8_switch_during_meeting_keeps_pinned_engine_and_reloads_on_next_dictation() {
        let _lock = crate::managers::meetings::test_lock();
        let dir = tempfile::tempdir().expect("tempdir for test models");
        let app = tauri::test::mock_builder()
            .plugin(tauri_plugin_store::Builder::default().build())
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock app");

        let prior = get_settings(app.handle());
        let _restore = RestoreSettings {
            app: app.handle(),
            selected_model: prior.selected_model,
            onboarding_completed: prior.onboarding_completed,
            unload_timeout: prior.model_unload_timeout,
        };

        let dictation_initial = "handy-computer/voxtral-mini-gguf";
        let pinned_meeting = "handy-computer/parakeet-ultra-gguf/parakeet-ultra-Q8_0.gguf";
        let switched_dictation =
            "handy-computer/canary-180m-flash-gguf/canary-180m-flash-Q8_0.gguf";

        update_settings(app.handle(), |s| {
            s.selected_model = dictation_initial.to_string();
            s.model_unload_timeout = ModelUnloadTimeout::Never;
        });

        let mut models = HashMap::new();
        models.insert(
            switched_dictation.to_string(),
            downloaded_speech_model(
                switched_dictation,
                "Canary 180M Flash",
                "canary-180m-flash-Q8_0.gguf",
            ),
        );
        let mm = Arc::new(ModelManager::for_test_in_dir(
            dir.path().to_path_buf(),
            models,
        ));
        let tm =
            TranscriptionManager::new_for_test_with_models(app.handle().clone(), Arc::clone(&mm));

        tm.install_fake_engine(pinned_meeting);
        tm.pin_meeting_model(pinned_meeting);

        // 1. Switch dictation model while the meeting holds `pinned_meeting`.
        switch_active_model_inner(app.handle(), &mm, &tm, switched_dictation)
            .expect("switching dictation model during a pinned meeting must succeed");

        assert_eq!(
            tm.get_current_model().as_deref(),
            Some(pinned_meeting),
            "pinned meeting engine must stay loaded during dictation model switch"
        );
        assert_eq!(
            get_settings(app.handle()).selected_model,
            switched_dictation,
            "new dictation choice must be persisted immediately"
        );

        // Meeting chunks keep transcribing on the pinned engine after the switch.
        tm.transcribe_meeting_chunk(vec![0.1_f32; 160])
            .expect("post-switch meeting chunk must transcribe on pinned engine");
        assert_eq!(tm.get_current_model().as_deref(), Some(pinned_meeting));

        // 2. Meeting ends -> clear_meeting_pin arms reload-on-next-use; next dictation reloads X.
        tm.clear_meeting_pin();
        assert!(
            tm.reload_on_next_use_armed(),
            "clear_meeting_pin must arm reload-on-next-use when loaded model != selected_model"
        );

        tm.initiate_model_load();
        let deadline = Instant::now() + Duration::from_secs(5);
        while tm.is_loading_now() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(
            !tm.is_loading_now(),
            "dictation reload triggered by initiate_model_load must settle"
        );
        assert_eq!(
            tm.take_test_dictation_load_calls(),
            vec![switched_dictation.to_string()],
            "next dictation after meeting must reload the switched model"
        );
        assert!(
            !tm.reload_on_next_use_armed(),
            "reload_model_on_next_use must clear once initiate_model_load consumes it"
        );

        // 3. C1 broad restore (no switch during meeting): meeting pinned P while dictation was D.
        update_settings(app.handle(), |s| {
            s.selected_model = dictation_initial.to_string();
        });
        tm.install_fake_engine(pinned_meeting);
        tm.pin_meeting_model(pinned_meeting);
        tm.clear_meeting_pin();
        assert!(
            tm.reload_on_next_use_armed(),
            "clear_meeting_pin must also arm reload-on-next-use when meeting changed the engine without a user switch"
        );
    }
}
