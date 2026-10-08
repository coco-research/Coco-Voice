#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
use crate::apple_intelligence;
use crate::audio_feedback::{play_feedback_sound, play_feedback_sound_blocking, SoundType};
use crate::audio_toolkit::{is_microphone_access_denied, is_no_input_device_error, VadPolicy};
use crate::commands::app_profile::{query_active_app, ActiveAppInfo};
use crate::managers::audio::AudioRecordingManager;
use crate::managers::history::HistoryManager;
use crate::managers::model::ModelManager;
use crate::managers::transcription::StreamWorkKind;
use crate::managers::transcription::TranscriptionManager;
use crate::settings::{
    get_settings, AppProfile, AppSettings, OverlayStyle, APPLE_INTELLIGENCE_PROVIDER_ID,
    LOCAL_LLM_DEFAULT_MODEL_ID, LOCAL_LLM_PROVIDER_ID,
};
use crate::shortcut;
use crate::tray::{change_tray_icon, TrayIconState};
use crate::utils::{
    self, show_processing_overlay, show_recording_overlay, show_transcribing_overlay,
};
use crate::TranscriptionCoordinator;
use ferrous_opencc::{config::BuiltinConfig, OpenCC};
use log::{debug, error, warn};
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::future::Future;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::Manager;
use tauri::{AppHandle, Emitter};
use tauri_specta::Event;

const CANCELLATION_POLL_INTERVAL: Duration = Duration::from_millis(25);
/// Local cleanup length. A catalog model id is not a token count, so this is
/// not parsed out of the model string.
#[cfg(target_os = "macos")]
const LOCAL_LLM_MAX_TOKENS: i32 = 512;

/// How long to wait for a stream worker that overran its finalize timeout to
/// return the engine before giving up on the batch fallback.
const STREAM_ENGINE_RETURN_WAIT: Duration = Duration::from_secs(10);

/// How long a produced output stays eligible as the base for an iterative
/// correction. A follow-up utterance arriving after this window is treated as a
/// fresh dictation rather than an edit of the previous result.
const REFINE_BUFFER_TTL: Duration = Duration::from_secs(30);

/// The last {raw transcript, produced output} pair, kept briefly so a follow-up
/// correction can edit the previous result instead of transcribing fresh (see
/// [`REFINE_BUFFER_TTL`]). Overwritten by each new dictation and expired on read.
struct RefineBuffer {
    raw_transcript: String,
    produced_output: String,
    created_at: Instant,
}

static REFINE_BUFFER: Lazy<Mutex<Option<RefineBuffer>>> = Lazy::new(|| Mutex::new(None));

#[derive(Clone, serde::Serialize)]
struct RecordingErrorEvent {
    error_type: String,
    detail: Option<String>,
}

/// Drop guard that notifies the [`TranscriptionCoordinator`] when the
/// transcription pipeline finishes — whether it completes normally or panics.
struct FinishGuard(AppHandle);
impl Drop for FinishGuard {
    fn drop(&mut self) {
        if let Some(c) = self.0.try_state::<TranscriptionCoordinator>() {
            c.notify_processing_finished();
        }
    }
}

// Shortcut Action Trait
pub trait ShortcutAction: Send + Sync {
    fn start(&self, app: &AppHandle, binding_id: &str, shortcut_str: &str);
    fn stop(&self, app: &AppHandle, binding_id: &str, shortcut_str: &str);
}

// Transcribe Action
struct TranscribeAction {
    post_process: bool,
    /// When `true`, this dictation is an explicit spoken CORRECTION of the last
    /// output rather than a fresh dictation: after transcribing, the utterance is
    /// used to edit the buffered previous output and the result REPLACES it at the
    /// cursor (see [`process_transcription_output`]). Triggered only by the
    /// dedicated "correction" hotkey — never inferred from the words spoken.
    correction: bool,
}

/// Field name for structured output JSON schema
const TRANSCRIPTION_FIELD: &str = "transcription";

/// Strip invisible Unicode characters that some LLMs may insert
fn strip_invisible_chars(s: &str) -> String {
    s.replace(['\u{200B}', '\u{200C}', '\u{200D}', '\u{FEFF}'], "")
}

/// Build a system prompt from the user's prompt template.
/// Removes `${output}` placeholder since the transcription is sent as the user message.
fn build_system_prompt(prompt_template: &str) -> String {
    prompt_template.replace("${output}", "").trim().to_string()
}

async fn complete_unless_cancelled<F, C>(operation: F, is_cancelled: C) -> Option<F::Output>
where
    F: Future,
    C: Fn() -> bool,
{
    tokio::pin!(operation);

    loop {
        if is_cancelled() {
            return None;
        }

        if let Ok(result) =
            tokio::time::timeout(CANCELLATION_POLL_INTERVAL, operation.as_mut()).await
        {
            return Some(result);
        }
    }
}

/// Live-dictation cancel signal for the on-device LLM.
///
/// `started` is the generation counter snapshotted when recording stopped.
/// History re-transcribe has no cancel shortcut and passes `None`.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) struct LocalLlmCancel {
    started: u64,
    audio: Arc<AudioRecordingManager>,
}

/// Store `true` on `flag` once `current` leaves the generation snapshotted at stop.
///
/// Returns whether this sample is a cancel. Never clears the flag: the
/// llama.cpp thread only checks it between tokens, and a moved counter stays
/// moved. Same comparison as [`AudioRecordingManager::was_cancelled_since`].
fn note_cancel_if_generation_moved(flag: &AtomicBool, started: u64, current: u64) -> bool {
    if current == started {
        return false;
    }
    // Release pairs with the Acquire load in `local_llm` (once per token).
    flag.store(true, Ordering::Release);
    true
}

/// Poll the pipeline cancel counter and raise `flag` when it moves.
///
/// Dropping the `generate_text` future does not stop the llama.cpp thread,
/// and dropping this handle must not either: a cancel drops the cleanup
/// future, so the watcher has to keep running and set the flag. Abort it
/// only after generation returns.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn watch_generation_cancel(
    flag: Arc<AtomicBool>,
    started: u64,
    audio: Arc<AudioRecordingManager>,
) -> tauri::async_runtime::JoinHandle<()> {
    tauri::async_runtime::spawn(async move {
        loop {
            if note_cancel_if_generation_moved(&flag, started, audio.cancel_generation()) {
                break;
            }
            tokio::time::sleep(CANCELLATION_POLL_INTERVAL).await;
        }
    })
}

fn should_use_streaming_overlay(style: OverlayStyle, is_streaming: bool) -> bool {
    style == OverlayStyle::Live && is_streaming
}

/// Returns the most recent produced output to use as the edit base for an
/// explicit spoken correction, provided iterative correction is enabled and a
/// fresh (within [`REFINE_BUFFER_TTL`]) output is buffered. A stale buffer is
/// expired on read.
///
/// Unlike the removed word-sniffing path, this does NOT inspect the utterance
/// text: the dedicated "correction" hotkey is the sole trigger, so a normal
/// dictation can never be silently reinterpreted as an edit.
fn refine_base_for_correction(settings: &AppSettings) -> Option<String> {
    if !settings.iterative_correction_enabled {
        return None;
    }

    // Recover a poisoned lock rather than panicking the transcription task.
    let mut guard = REFINE_BUFFER.lock().unwrap_or_else(|e| e.into_inner());

    // Expire a stale buffer so a correction never edits an ancient output.
    if guard
        .as_ref()
        .is_some_and(|buf| buf.created_at.elapsed() > REFINE_BUFFER_TTL)
    {
        *guard = None;
    }

    guard.as_ref().map(|buf| {
        debug!(
            "Correction hotkey: editing previous output (prior raw transcript: {:?})",
            buf.raw_transcript
        );
        buf.produced_output.clone()
    })
}

/// Records this dictation's {raw transcript, produced output} as the base for a
/// possible follow-up correction. A blank output clears the buffer instead of
/// storing it, so an empty result never becomes an edit base.
fn store_refine_buffer(settings: &AppSettings, raw_transcript: &str, produced_output: &str) {
    if !settings.iterative_correction_enabled {
        return;
    }

    let mut guard = REFINE_BUFFER.lock().unwrap_or_else(|e| e.into_inner());
    if produced_output.trim().is_empty() {
        *guard = None;
        return;
    }

    *guard = Some(RefineBuffer {
        raw_transcript: raw_transcript.to_string(),
        produced_output: produced_output.to_string(),
        created_at: Instant::now(),
    });
}

/// System prompt used when editing a previous output from a spoken correction,
/// instead of cleaning a fresh transcript.
fn build_refine_system_prompt() -> String {
    "You are editing a piece of text based on a spoken correction from the user. \
You will be given the PREVIOUS text and the CORRECTION the user just dictated. \
Apply the requested change to the previous text and return only the full, updated text. \
Keep everything the user did not ask to change exactly as it was. \
Do not add explanations or quotation marks, and do not answer any question contained in the text."
        .to_string()
}

/// User message for the structured / system-prompt edit path: the previous
/// output plus the freshly dictated correction.
fn build_refine_user_content(previous_output: &str, correction: &str) -> String {
    format!(
        "PREVIOUS:\n{}\n\nCORRECTION:\n{}",
        previous_output, correction
    )
}

/// Single-message prompt for the legacy (no system role) edit path — folds the
/// refine instruction, previous output, and correction into one user message.
fn build_refine_legacy_prompt(previous_output: &str, correction: &str) -> String {
    format!(
        "{}\n\n{}",
        build_refine_system_prompt(),
        build_refine_user_content(previous_output, correction)
    )
}

/// Selects the `(system prompt, user content)` pair for the structured-output
/// post-processing call. When `prior_output` is `Some`, the *edit* prompt is
/// built (refine system prompt + PREVIOUS/CORRECTION user content) so the model
/// edits that prior output using `transcription` as the spoken correction;
/// otherwise the normal clean-up prompt is built from the user's template.
fn build_post_process_messages(
    prompt_template: &str,
    transcription: &str,
    prior_output: Option<&str>,
) -> (String, String) {
    match prior_output {
        Some(previous) => (
            build_refine_system_prompt(),
            build_refine_user_content(previous, transcription),
        ),
        None => (
            build_system_prompt(prompt_template),
            transcription.to_string(),
        ),
    }
}

/// Case-insensitive match of a saved identifier against the frontmost app's
/// name or its process file stem. Empty identifiers never match. Bundle ids
/// are not available from the active-window API and are not compared.
fn app_profile_matches(saved: &str, app_name: &str, process_stem: &str) -> bool {
    let saved = saved.trim();
    if saved.is_empty() {
        return false;
    }
    saved.eq_ignore_ascii_case(app_name) || saved.eq_ignore_ascii_case(process_stem)
}

/// Starts reading the app the user is dictating into. Called when recording
/// stops, before any transcription or overlay work, so the profile follows that
/// app even if the user switches windows while the take is transcribed and
/// cleaned up. Resolves to `None`, meaning global settings, when no profiles are
/// configured (nothing to match, so no window query) or when the read fails or
/// times out.
fn capture_frontmost_app(
    settings: &AppSettings,
) -> tauri::async_runtime::JoinHandle<Option<ActiveAppInfo>> {
    let has_profiles = !settings.app_profiles.is_empty();
    tauri::async_runtime::spawn(async move {
        if !has_profiles {
            return None;
        }
        match query_active_app().await {
            Ok(app) => app,
            Err(err) => {
                debug!("{err}; applying no per-app profile");
                None
            }
        }
    })
}

/// Non-empty trimmed model from the profile, if it supplied one.
fn profile_model_override(profile: &AppProfile) -> Option<String> {
    profile
        .model
        .as_deref()
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .map(str::to_string)
}

/// Applies a profile's provider and model onto `settings`.
///
/// A profile model only applies together with the provider the profile names:
/// - no provider named: no model override at all (the global provider and
///   model stay);
/// - provider missing from the settings: the global provider and its model stay;
/// - provider found: it is selected and the profile's trimmed non-empty model,
///   if any, is stored for it, otherwise its already stored model is used. When
///   neither is non-empty the provider is not switched.
fn apply_profile_provider_and_model(settings: &mut AppSettings, profile: &AppProfile) {
    let profile_model = profile_model_override(profile);
    let Some(provider_id) = profile.provider_id.as_deref() else {
        if profile_model.is_some() {
            debug!("Profile model ignored because the profile names no provider");
        }
        return;
    };

    if !settings
        .post_process_providers
        .iter()
        .any(|provider| provider.id == provider_id)
    {
        debug!(
            "Profile provider '{provider_id}' is missing; keeping the global provider and model"
        );
        return;
    }

    let has_stored_model = settings
        .post_process_models
        .get(provider_id)
        .is_some_and(|model| !model.trim().is_empty());
    if profile_model.is_none() && !has_stored_model {
        debug!("Profile provider '{provider_id}' has no model; keeping the global provider");
        return;
    }

    settings.post_process_provider_id = provider_id.to_string();
    if let Some(model) = profile_model {
        settings
            .post_process_models
            .insert(provider_id.to_string(), model);
    }
}

/// Applies the first per-app profile matching `app`, the app captured when the
/// dictation stopped, as a settings override. Returns a clone of `settings`
/// unchanged when no profile matches.
fn apply_app_profile_overrides(settings: &AppSettings, app: &ActiveAppInfo) -> AppSettings {
    let process_stem = Path::new(&app.process_path)
        .file_stem()
        .map(|stem| stem.to_string_lossy().to_string())
        .unwrap_or_default();

    let Some(profile) = settings
        .app_profiles
        .iter()
        .find(|p| app_profile_matches(&p.app_identifier, &app.app_name, &process_stem))
    else {
        return settings.clone();
    };

    debug!(
        "Per-app profile '{}' matched for app '{}'",
        profile.name, process_stem
    );
    let mut overridden = settings.clone();
    // A deleted prompt must fall back to the global one. Applying the stale id
    // makes post-processing skip the dictation entirely.
    if let Some(ref prompt_id) = profile.prompt_id {
        if settings
            .post_process_prompts
            .iter()
            .any(|prompt| &prompt.id == prompt_id)
        {
            overridden.post_process_selected_prompt_id = Some(prompt_id.clone());
        } else {
            debug!("Profile prompt '{prompt_id}' is missing; keeping the global prompt");
        }
    }
    apply_profile_provider_and_model(&mut overridden, profile);
    // Append profile-specific corrections to the global list
    if !profile.corrections.is_empty() {
        overridden.corrections.extend(profile.corrections.clone());
    }
    overridden
}

fn skip_cleanup(app: &AppHandle, reason: &crate::cleanup_resolver::SkipReason) {
    log::info!("Post-processing skipped: {}", reason);
    if let Some(status) = crate::cleanup_resolver::CleanupStatus::status_for_skip(reason) {
        let _ = status.emit(app);
    }
}

/// Post-processes `transcription`. When `prior_output` is `Some`, the call runs
/// in *edit mode*: instead of cleaning a fresh transcript, the model edits the
/// previous output using `transcription` as the spoken correction instruction.
async fn post_process_transcription(
    app: &AppHandle,
    settings: &AppSettings,
    transcription: &str,
    prior_output: Option<&str>,
    local_llm_cancel: Option<LocalLlmCancel>,
) -> Option<String> {
    #[cfg(not(target_os = "macos"))]
    let _ = (app, &local_llm_cancel);

    let selected_provider_id = settings.post_process_provider_id.as_str();
    let model_manager = app.state::<Arc<ModelManager>>();

    let prompt = match crate::cleanup_resolver::check_global_preconditions(
        transcription,
        settings.post_process_selected_prompt_id.as_deref(),
        |id| {
            settings
                .post_process_prompts
                .iter()
                .find(|p| p.id == id)
                .map(|p| p.prompt.as_str())
        },
    ) {
        Ok(p) => p.to_string(),
        Err(reason) => {
            skip_cleanup(app, &reason);
            return None;
        }
    };

    let get_provider_model = |provider_id: &str| -> String {
        if provider_id == LOCAL_LLM_PROVIDER_ID {
            settings
                .post_process_models
                .get(provider_id)
                .filter(|s| !s.trim().is_empty())
                .map(|s| s.as_str())
                .unwrap_or(LOCAL_LLM_DEFAULT_MODEL_ID)
                .to_string()
        } else {
            settings
                .post_process_models
                .get(provider_id)
                .cloned()
                .unwrap_or_default()
        }
    };

    let get_api_key = |provider_id: &str| -> String {
        settings
            .post_process_api_keys
            .get(provider_id)
            .cloned()
            .unwrap_or_default()
    };

    let provider_usable = |provider_id: &str| -> Result<(), crate::cleanup_resolver::SkipReason> {
        if !settings
            .post_process_providers
            .iter()
            .any(|p| p.id == provider_id)
        {
            return Err(crate::cleanup_resolver::SkipReason::ProviderNotFound);
        }

        let model = get_provider_model(provider_id);

        if model.trim().is_empty() {
            return Err(crate::cleanup_resolver::SkipReason::NoModelConfigured);
        }

        if provider_id == APPLE_INTELLIGENCE_PROVIDER_ID {
            #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
            {
                if !crate::apple_intelligence::check_apple_intelligence_availability() {
                    return Err(crate::cleanup_resolver::SkipReason::AppleIntelligenceUnavailable);
                }
            }
            #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
            {
                return Err(crate::cleanup_resolver::SkipReason::ProviderUnavailableInThisBuild);
            }
        }

        if provider_id == LOCAL_LLM_PROVIDER_ID {
            #[cfg(not(target_os = "macos"))]
            {
                return Err(crate::cleanup_resolver::SkipReason::ProviderUnavailableInThisBuild);
            }
            #[cfg(target_os = "macos")]
            {
                if !model_manager.get_model_path(&model).is_ok() {
                    return Err(crate::cleanup_resolver::SkipReason::LocalModelNotDownloaded);
                }
            }
        }

        let needs_api_key = provider_id != LOCAL_LLM_PROVIDER_ID
            && provider_id != APPLE_INTELLIGENCE_PROVIDER_ID
            && provider_id != "custom";

        if needs_api_key {
            let api_key = get_api_key(provider_id);
            if api_key.trim().is_empty() {
                return Err(crate::cleanup_resolver::SkipReason::NoApiKey);
            }
        }

        Ok(())
    };

    let decision = crate::cleanup_resolver::resolve_cleanup(selected_provider_id, provider_usable);

    let resolved_provider_id = match decision {
        crate::cleanup_resolver::CleanupDecision::Run {
            provider_id,
            fallback_from,
        } => {
            if let Some((original_id, reason)) = fallback_from {
                log::info!(
                    "Cleanup: {} unavailable ({}), using on-device model",
                    original_id,
                    reason
                );
            }
            provider_id
        }
        crate::cleanup_resolver::CleanupDecision::Skip { reason } => {
            skip_cleanup(app, &reason);
            return None;
        }
    };

    let provider = match settings
        .post_process_providers
        .iter()
        .find(|p| p.id == resolved_provider_id)
    {
        Some(p) => p.clone(),
        None => {
            let reason = crate::cleanup_resolver::SkipReason::ProviderNotFound;
            skip_cleanup(app, &reason);
            return None;
        }
    };

    let model = get_provider_model(&resolved_provider_id);
    let api_key = get_api_key(&resolved_provider_id);

    debug!(
        "Starting LLM post-processing with provider '{}' (model: {})",
        provider.id, model
    );

    // Disable reasoning for providers where post-processing rarely benefits from it.
    // - custom: top-level reasoning_effort (works for local OpenAI-compat servers)
    // - openrouter: nested reasoning object; exclude:true also keeps reasoning text
    //   out of the response so it can't pollute structured-output JSON parsing
    let (reasoning_effort, reasoning) = match provider.id.as_str() {
        "custom" => (Some("none".to_string()), None),
        "openrouter" => (
            None,
            Some(crate::llm_client::ReasoningConfig {
                effort: Some("none".to_string()),
                exclude: Some(true),
            }),
        ),
        _ => (None, None),
    };

    // Local LLM runs llama.cpp on device, not the JSON-schema HTTP API, so it
    // is dispatched before the structured-output gate. The provider keeps
    // `supports_structured_output: false`; hoisting the call is what makes
    // local cleanup reachable.
    #[cfg(target_os = "macos")]
    if provider.id == crate::settings::LOCAL_LLM_PROVIDER_ID {
        let (system_prompt, user_content) =
            build_post_process_messages(&prompt, transcription, prior_output);
        match model_manager.get_model_path(&model) {
            Ok(model_path) => {
                let cancel_watch = local_llm_cancel.map(|cancel| {
                    let flag = Arc::new(AtomicBool::new(false));
                    let watcher = watch_generation_cancel(
                        Arc::clone(&flag),
                        cancel.started,
                        Arc::clone(&cancel.audio),
                    );
                    (flag, watcher)
                });
                let generated = crate::local_llm::generate_text(
                    &model_path,
                    &system_prompt,
                    &user_content,
                    LOCAL_LLM_MAX_TOKENS,
                    None,
                    cancel_watch.as_ref().map(|(flag, _)| Arc::clone(flag)),
                )
                .await;
                // Finished run: stop polling. A dropped future skips this and
                // leaves the watcher attached so it can still raise the flag.
                if let Some((_, watcher)) = &cancel_watch {
                    watcher.abort();
                }
                return match generated {
                    Ok(result) => {
                        if result.trim().is_empty() {
                            debug!("Local LLM returned an empty response");
                            None
                        } else {
                            let result = strip_invisible_chars(&result);
                            debug!(
                                "Local LLM post-processing succeeded. Output length: {} chars",
                                result.len()
                            );
                            Some(result)
                        }
                    }
                    Err(err) => {
                        error!("Local LLM post-processing failed: {}", err);
                        match err {
                            crate::local_llm::LocalLlmError::OverBudget => {
                                let _ =
                                    crate::cleanup_resolver::CleanupStatus::too_long().emit(app);
                            }
                            crate::local_llm::LocalLlmError::StoppedEarly => {
                                let _ = crate::cleanup_resolver::CleanupStatus::stopped_early()
                                    .emit(app);
                            }
                            crate::local_llm::LocalLlmError::Cancelled => {}
                            crate::local_llm::LocalLlmError::Other(_) => {
                                let _ = crate::cleanup_resolver::CleanupStatus::model_load_failed()
                                    .emit(app);
                            }
                        }
                        None
                    }
                };
            }
            Err(err) => {
                error!("Failed to resolve local LLM model path: {}", err);
                skip_cleanup(
                    app,
                    &crate::cleanup_resolver::SkipReason::LocalModelNotDownloaded,
                );
                return None;
            }
        }
    }

    if provider.supports_structured_output {
        debug!("Using structured outputs for provider '{}'", provider.id);

        let (system_prompt, user_content) =
            build_post_process_messages(&prompt, transcription, prior_output);

        // Handle Apple Intelligence separately since it uses native Swift APIs
        if provider.id == APPLE_INTELLIGENCE_PROVIDER_ID {
            #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
            {
                if !apple_intelligence::check_apple_intelligence_availability() {
                    debug!(
                        "Apple Intelligence selected but not currently available on this device"
                    );
                    return None;
                }

                let token_limit = model.trim().parse::<i32>().unwrap_or(0);
                return match apple_intelligence::process_text_with_system_prompt(
                    &system_prompt,
                    &user_content,
                    token_limit,
                ) {
                    Ok(result) => {
                        if result.trim().is_empty() {
                            debug!("Apple Intelligence returned an empty response");
                            None
                        } else {
                            let result = strip_invisible_chars(&result);
                            debug!(
                                "Apple Intelligence post-processing succeeded. Output length: {} chars",
                                result.len()
                            );
                            Some(result)
                        }
                    }
                    Err(err) => {
                        error!("Apple Intelligence post-processing failed: {}", err);
                        None
                    }
                };
            }

            #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
            {
                debug!("Apple Intelligence provider selected on unsupported platform");
                return None;
            }
        }

        // Define JSON schema for transcription output
        let json_schema = serde_json::json!({
            "type": "object",
            "properties": {
                (TRANSCRIPTION_FIELD): {
                    "type": "string",
                    "description": "The cleaned and processed transcription text"
                }
            },
            "required": [TRANSCRIPTION_FIELD],
            "additionalProperties": false
        });

        match crate::llm_client::send_chat_completion_with_schema(
            &provider,
            api_key.clone(),
            &model,
            user_content,
            Some(system_prompt),
            Some(json_schema),
            reasoning_effort.clone(),
            reasoning.clone(),
        )
        .await
        {
            Ok(Some(content)) => {
                // Parse the JSON response to extract the transcription field
                match serde_json::from_str::<serde_json::Value>(&content) {
                    Ok(json) => {
                        if let Some(transcription_value) =
                            json.get(TRANSCRIPTION_FIELD).and_then(|t| t.as_str())
                        {
                            let result = strip_invisible_chars(transcription_value);
                            debug!(
                                "Structured output post-processing succeeded for provider '{}'. Output length: {} chars",
                                provider.id,
                                result.len()
                            );
                            return Some(result);
                        } else {
                            error!("Structured output response missing 'transcription' field");
                            return Some(strip_invisible_chars(&content));
                        }
                    }
                    Err(e) => {
                        error!(
                            "Failed to parse structured output JSON: {}. Returning raw content.",
                            e
                        );
                        return Some(strip_invisible_chars(&content));
                    }
                }
            }
            Ok(None) => {
                error!("LLM API response has no content");
                return None;
            }
            Err(e) => {
                warn!(
                    "Structured output failed for provider '{}': {}. Falling back to legacy mode.",
                    provider.id, e
                );
                // Fall through to legacy mode below
            }
        }
    }

    // Legacy mode: single user message. In edit mode we fold the refine
    // instruction + previous output + correction into one prompt; otherwise we
    // substitute the transcription into the user's ${output} template.
    let processed_prompt = match prior_output {
        Some(previous) => build_refine_legacy_prompt(previous, transcription),
        None => prompt.replace("${output}", transcription),
    };
    debug!("Processed prompt length: {} chars", processed_prompt.len());

    match crate::llm_client::send_chat_completion(
        &provider,
        api_key,
        &model,
        processed_prompt,
        reasoning_effort,
        reasoning,
    )
    .await
    {
        Ok(Some(content)) => {
            let content = strip_invisible_chars(&content);
            debug!(
                "LLM post-processing succeeded for provider '{}'. Output length: {} chars",
                provider.id,
                content.len()
            );
            Some(content)
        }
        Ok(None) => {
            error!("LLM API response has no content");
            None
        }
        Err(e) => {
            error!(
                "LLM post-processing failed for provider '{}': {}. Falling back to original transcription.",
                provider.id,
                e
            );
            None
        }
    }
}

async fn maybe_convert_chinese_variant(
    effective_language: &str,
    transcription: &str,
) -> Option<String> {
    // Gate on the language the model actually transcribed in (the effective
    // language), not the persisted intent. A leftover zh-Hans/zh-Hant intent
    // from a previously selected model must not run OpenCC S2T/T2S over output a
    // non-Chinese model produced — that would silently rewrite any shared CJK
    // characters (e.g. Japanese kanji) in the result.
    let is_simplified = effective_language == "zh-Hans";
    let is_traditional = effective_language == "zh-Hant";

    if !is_simplified && !is_traditional {
        debug!("effective language is not Simplified or Traditional Chinese; skipping conversion");
        return None;
    }

    debug!(
        "Starting Chinese variant conversion using OpenCC for language: {}",
        effective_language
    );

    // Use OpenCC to convert based on selected language
    let config = if is_simplified {
        // Convert Traditional Chinese to Simplified Chinese
        BuiltinConfig::Tw2sp
    } else {
        // Convert Simplified Chinese to Traditional Chinese
        BuiltinConfig::S2tw
    };

    match OpenCC::from_config(config) {
        Ok(converter) => {
            let converted = converter.convert(transcription);
            debug!(
                "OpenCC translation completed. Input length: {}, Output length: {}",
                transcription.len(),
                converted.len()
            );
            Some(converted)
        }
        Err(e) => {
            error!("Failed to initialize OpenCC converter: {}. Falling back to original transcription.", e);
            None
        }
    }
}

pub(crate) struct ProcessedTranscription {
    pub final_text: String,
    pub post_processed_text: Option<String>,
    pub post_process_prompt: Option<String>,
    /// Number of characters the paste path should delete (via Backspace) before
    /// typing `final_text`. Non-zero only in correction mode, where it is the
    /// length of the prior output being REPLACED by this edit; `0` means a normal
    /// append paste with nothing to delete. See [`crate::utils::replace_previous`].
    pub replace_char_count: usize,
}

/// Resolve the persisted language *intent* into the language the currently-loaded
/// model will actually use — the same capability-aware coercion the transcription
/// paths apply (see [`crate::managers::model::effective_language`]). Post-processing
/// resolves it independently so it agrees with the language the transcription ran
/// in, without threading a value through the pipeline.
fn resolve_effective_language(app: &AppHandle, settings: &AppSettings) -> String {
    let tm = app.state::<Arc<TranscriptionManager>>();
    let model_manager = app.state::<Arc<ModelManager>>();
    let active_model = tm
        .get_current_model()
        .unwrap_or_else(|| settings.selected_model.clone());
    match model_manager.get_model_info(&active_model) {
        Some(info) => crate::managers::model::effective_language(
            &settings.selected_language,
            &info.supported_languages,
            info.supports_language_detection,
        ),
        None => settings.selected_language.clone(),
    }
}

pub(crate) async fn process_transcription_output(
    app: &AppHandle,
    transcription: &str,
    post_process: bool,
    correction: bool,
    captured_app: Option<&ActiveAppInfo>,
    local_llm_cancel: Option<LocalLlmCancel>,
) -> ProcessedTranscription {
    let settings = get_settings(app);
    // Live dictation applies the profile of the app captured when recording
    // stopped, once, before either paste path. It never re-reads the frontmost
    // window here: the user may have switched apps while this ran. History
    // re-runs pass `None` (no profile) so they do not follow whichever app
    // happens to be in front now, and a failed or timed-out capture is `None`.
    let effective_settings = match captured_app {
        Some(captured) => apply_app_profile_overrides(&settings, captured),
        None => settings.clone(),
    };
    let mut final_text = transcription.to_string();
    let mut post_processed_text: Option<String> = None;
    let mut post_process_prompt: Option<String> = None;
    let mut replace_char_count: usize = 0;

    // Resolve the language the transcription actually ran in (the persisted
    // intent coerced against the loaded model's capabilities) so OpenCC keys off
    // the effective language rather than a possibly-stale intent.
    let effective_language = resolve_effective_language(app, &settings);
    if let Some(converted_text) =
        maybe_convert_chinese_variant(&effective_language, transcription).await
    {
        final_text = converted_text;
    }

    if correction {
        // CORRECTION MODE (explicit hotkey): unconditionally treat this utterance
        // as an edit of the last output. We never sniff the words — the dedicated
        // "correction" hotkey is the only trigger.
        if let Some(prior_output) = refine_base_for_correction(&effective_settings) {
            // The prior output is what the previous dictation pasted (plus the
            // trailing space `paste` appends when that setting is on), so its
            // grapheme length is how many Backspaces the replace must send before
            // typing the edit. Graphemes, not scalars or bytes: one Backspace
            // deletes one grapheme cluster (combining marks, joined emoji).
            let prev_char_count = crate::utils::pasted_grapheme_count(
                &prior_output,
                effective_settings.append_trailing_space,
            );
            match post_process_transcription(
                app,
                &effective_settings,
                &final_text,
                Some(&prior_output),
                local_llm_cancel,
            )
            .await
            {
                Some(edited) => {
                    post_processed_text = Some(edited.clone());
                    final_text = edited;
                    // The guard clamps an implausibly long prior output to 0
                    // (append instead of hammering Backspace across the document).
                    replace_char_count = crate::utils::backspaces_for_replace(prev_char_count);

                    if let Some(prompt_id) = &effective_settings.post_process_selected_prompt_id {
                        if let Some(prompt) = effective_settings
                            .post_process_prompts
                            .iter()
                            .find(|prompt| &prompt.id == prompt_id)
                        {
                            post_process_prompt = Some(prompt.prompt.clone());
                        }
                    }
                }
                None => {
                    // No LLM edit was produced (e.g. no post-process provider or
                    // prompt is configured). There is nothing reliable to replace
                    // with, so fall back to appending the raw utterance.
                    debug!("Correction mode produced no edit; appending transcription instead");
                }
            }
        } else {
            // No fresh prior output within the TTL — nothing to replace, so this
            // utterance is pasted as a normal (append) dictation.
            debug!("Correction mode has no fresh prior output; appending transcription");
        }
    } else if post_process {
        // Normal post-processing. Word-sniffing has been removed, so a fresh
        // dictation is NEVER silently reinterpreted as an edit of a prior output.
        if let Some(processed_text) = post_process_transcription(
            app,
            &effective_settings,
            &final_text,
            None,
            local_llm_cancel,
        )
        .await
        {
            post_processed_text = Some(processed_text.clone());
            final_text = processed_text;

            if let Some(prompt_id) = &effective_settings.post_process_selected_prompt_id {
                if let Some(prompt) = effective_settings
                    .post_process_prompts
                    .iter()
                    .find(|prompt| &prompt.id == prompt_id)
                {
                    post_process_prompt = Some(prompt.prompt.clone());
                }
            }
        }
    } else if final_text != transcription {
        post_processed_text = Some(final_text.clone());
    }

    // Deterministic user correction map: the final output pass on the pasted text.
    // Runs after LLM post-processing (above) and after apply_custom_words (which
    // runs earlier in the transcription pipeline), so the user always gets a
    // predictable last-word override via case-insensitive whole-word replacement.
    if !effective_settings.corrections.is_empty() {
        final_text =
            crate::audio_toolkit::apply_corrections(&final_text, &effective_settings.corrections);
    }

    // Remember this dictation so a later correction-hotkey press can edit it.
    // Stored for BOTH plain and post-processed dictations, and updated after a
    // correction too, so the buffer always holds exactly what the user last
    // received and a subsequent correction deletes the right number of chars.
    store_refine_buffer(&settings, transcription, &final_text);

    ProcessedTranscription {
        final_text,
        post_processed_text,
        post_process_prompt,
        replace_char_count,
    }
}

impl ShortcutAction for TranscribeAction {
    fn start(&self, app: &AppHandle, binding_id: &str, _shortcut_str: &str) {
        let start_time = Instant::now();
        debug!("TranscribeAction::start called for binding: {}", binding_id);

        // Load model in the background
        let tm = app.state::<Arc<TranscriptionManager>>();
        let rm = app.state::<Arc<AudioRecordingManager>>();

        // Load ASR model and VAD model in parallel
        let kickoff_started = Instant::now();
        tm.initiate_model_load();
        let rm_clone = Arc::clone(&rm);
        std::thread::spawn(move || {
            if let Err(e) = rm_clone.preload_vad() {
                debug!("VAD pre-load failed: {}", e);
            }
        });
        let kickoff_elapsed = kickoff_started.elapsed();

        let binding_id = binding_id.to_string();
        let tray_started = Instant::now();
        change_tray_icon(app, TrayIconState::Recording);
        let tray_elapsed = tray_started.elapsed();

        // Get the microphone mode to determine audio feedback timing
        let plan_started = Instant::now();
        let settings = get_settings(app);
        let is_always_on = settings.always_on_microphone;

        let selected_model_info = app
            .state::<Arc<ModelManager>>()
            .get_model_info(&settings.selected_model);

        // Use the app-facing model capability as the single pre-recording source
        // for live streaming decisions. Unknown support is represented as false
        // until the model registry is updated by discovery or runtime load.
        let model_supports_streaming = selected_model_info
            .as_ref()
            .map(|m| m.supports_streaming)
            .unwrap_or(false);
        let vad_policy = if !settings.vad_enabled {
            VadPolicy::Disabled
        } else if model_supports_streaming {
            VadPolicy::Streaming
        } else {
            VadPolicy::Offline
        };
        if model_supports_streaming {
            tm.start_stream();
        }
        let plan_elapsed = plan_started.elapsed();

        // Sizing the overlay follows the same advertised capability. A model that
        // doesn't stream (or whose capability is not known yet) gets the compact
        // pill instead of an oversized transparent live window.
        let overlay_started = Instant::now();
        match settings.overlay_style {
            OverlayStyle::Live if model_supports_streaming => utils::show_streaming_overlay(app),
            OverlayStyle::Live | OverlayStyle::Minimal => show_recording_overlay(app),
            OverlayStyle::None => {} // show_overlay_state no-ops on None anyway
        }
        // Everything above runs before capture can begin, so each span here is
        // added keypress->capture latency.
        debug!(
            "start-path pre-recording steps: model_kickoff={:?} tray={:?} settings+stream_plan={:?} overlay={:?}",
            kickoff_elapsed,
            tray_elapsed,
            plan_elapsed,
            overlay_started.elapsed()
        );
        debug!("Microphone mode - always_on: {}", is_always_on);

        // Identifies this take, so a mute scheduled below is dropped if the take
        // has already stopped by the time the start sound finishes.
        let mute_token = rm.mute_token();
        let mut recording_error: Option<String> = None;
        if is_always_on {
            // Always-on mode: Play audio feedback immediately, then apply mute after sound finishes
            debug!("Always-on mode: Playing audio feedback immediately");
            let rm_clone = Arc::clone(&rm);
            let app_clone = app.clone();
            // The blocking helper exits immediately if audio feedback is disabled,
            // so we can always reuse this thread to ensure mute happens right after playback.
            std::thread::spawn(move || {
                play_feedback_sound_blocking(&app_clone, SoundType::Start);
                rm_clone.apply_mute(mute_token);
            });

            if let Err(e) = rm.try_start_recording(&binding_id, vad_policy) {
                debug!("Recording failed: {}", e);
                recording_error = Some(e);
            }
        } else {
            // On-demand mode: Start recording first, then play audio feedback, then apply mute
            // This allows the microphone to be activated before playing the sound
            debug!("On-demand mode: Starting recording first, then audio feedback");
            let recording_start_time = Instant::now();
            match rm.try_start_recording(&binding_id, vad_policy) {
                Ok(()) => {
                    debug!("Recording started in {:?}", recording_start_time.elapsed());
                    // Small delay to ensure microphone stream is active
                    let app_clone = app.clone();
                    let rm_clone = Arc::clone(&rm);
                    std::thread::spawn(move || {
                        std::thread::sleep(std::time::Duration::from_millis(100));
                        debug!("Handling delayed audio feedback/mute sequence");
                        // Helper handles disabled audio feedback by returning early, so we reuse it
                        // to keep mute sequencing consistent in every mode.
                        play_feedback_sound_blocking(&app_clone, SoundType::Start);
                        rm_clone.apply_mute(mute_token);
                    });
                }
                Err(e) => {
                    debug!("Failed to start recording: {}", e);
                    recording_error = Some(e);
                }
            }
        }

        if recording_error.is_none() {
            // Dynamically register the cancel shortcut in a separate task to avoid deadlock
            shortcut::register_cancel_shortcut(app);
        } else {
            // Starting failed (for example due to blocked microphone permissions).
            // Revert UI state so we don't stay stuck in the recording overlay.
            tm.cancel_stream();
            // Drop a pending start-sound mute: no stop will follow a failed start.
            rm.remove_mute();
            utils::hide_recording_overlay(app);
            change_tray_icon(app, TrayIconState::Idle);
            if let Some(err) = recording_error {
                let error_type = if is_microphone_access_denied(&err) {
                    "microphone_permission_denied"
                } else if is_no_input_device_error(&err) {
                    "no_input_device"
                } else {
                    "unknown"
                };
                let _ = app.emit(
                    "recording-error",
                    RecordingErrorEvent {
                        error_type: error_type.to_string(),
                        detail: Some(err),
                    },
                );
            }
        }

        debug!(
            "TranscribeAction::start completed in {:?}",
            start_time.elapsed()
        );
    }

    fn stop(&self, app: &AppHandle, binding_id: &str, _shortcut_str: &str) {
        // Snapshot the app being dictated into before any tray, overlay or
        // transcription work, so its profile applies even if the user switches
        // windows before the text is ready.
        let frontmost_capture = capture_frontmost_app(&get_settings(app));

        // Unregister the cancel shortcut when transcription stops
        shortcut::unregister_cancel_shortcut(app);

        let stop_time = Instant::now();
        debug!("TranscribeAction::stop called for binding: {}", binding_id);

        let ah = app.clone();
        let rm = Arc::clone(&app.state::<Arc<AudioRecordingManager>>());
        let tm = Arc::clone(&app.state::<Arc<TranscriptionManager>>());
        let hm = Arc::clone(&app.state::<Arc<HistoryManager>>());

        change_tray_icon(app, TrayIconState::Transcribing);
        // Stop should give immediate visual feedback. Live streaming can keep
        // the larger panel, but it still switches from listening to a working
        // spinner while the stream finalizes. Non-streaming paths use the
        // compact transcribing pill (None no-ops in show_*).
        let style = get_settings(app).overlay_style;
        // Capture this before finalizing the stream so every later working state
        // targets the same overlay that was shown for this transcription.
        let use_streaming_overlay = should_use_streaming_overlay(style, tm.is_streaming());
        if use_streaming_overlay {
            tm.emit_stream_working(StreamWorkKind::Transcribing);
        } else {
            show_transcribing_overlay(app);
        }

        // Unmute before playing audio feedback so the stop sound is audible
        rm.remove_mute();

        // Play audio feedback for recording stop
        play_feedback_sound(app, SoundType::Stop);

        let binding_id = binding_id.to_string(); // Clone binding_id for the async task
        let post_process = self.post_process;
        let correction = self.correction;
        let cancel_generation = rm.cancel_generation();

        tauri::async_runtime::spawn(async move {
            let _guard = FinishGuard(ah.clone());
            debug!(
                "Starting async transcription task for binding: {}",
                binding_id
            );

            let stop_recording_time = Instant::now();
            if let Some(samples) = rm.stop_recording(&binding_id, cancel_generation) {
                debug!(
                    "Recording stopped and samples retrieved in {:?}, sample count: {}",
                    stop_recording_time.elapsed(),
                    samples.len()
                );

                if rm.was_cancelled_since(cancel_generation) {
                    debug!("Transcription operation cancelled after recording stop");
                    tm.cancel_stream();
                    utils::hide_recording_overlay(&ah);
                    change_tray_icon(&ah, TrayIconState::Idle);
                    return;
                }

                if samples.is_empty() {
                    debug!("Recording produced no audio samples; skipping persistence");
                    // Tear down any streaming worker so its channel doesn't leak
                    // and block the next start_stream.
                    tm.cancel_stream();
                    utils::hide_recording_overlay(&ah);
                    change_tray_icon(&ah, TrayIconState::Idle);
                } else {
                    // Save WAV concurrently with transcription
                    let sample_count = samples.len();
                    let file_name = format!("coco-voice-{}.wav", chrono::Utc::now().timestamp());
                    let wav_path = hm.recordings_dir().join(&file_name);
                    let wav_path_for_verify = wav_path.clone();
                    let samples_for_wav = samples.clone();
                    let wav_handle = tauri::async_runtime::spawn_blocking(move || {
                        crate::audio_toolkit::save_wav_file(&wav_path, &samples_for_wav)
                    });

                    // Transcribe concurrently with WAV save. If a live stream was
                    // running, finalize it and use its text (all audio was already
                    // fed to the stream); otherwise batch-transcribe the samples.
                    let transcription_time = Instant::now();
                    let transcription_result = match tm.finalize_stream() {
                        // A finalized stream with usable text wins. An empty result
                        // (no active stream, produced nothing, or a finalize error
                        // after the engine was returned) falls back to a full batch
                        // transcription of the same audio. After a finalize timeout
                        // the worker may still hold the engine: wait a bounded time
                        // for it to come back and then fall back to batch, otherwise
                        // surface the timeout error.
                        Ok(Some(text)) if !text.trim().is_empty() => Ok(text),
                        Ok(_) => tm.transcribe(samples),
                        Err(err) if tm.wait_for_engine(STREAM_ENGINE_RETURN_WAIT) => {
                            warn!("Stream finalize failed ({err}); falling back to batch");
                            tm.transcribe(samples)
                        }
                        Err(err) => Err(err),
                    };

                    // Await WAV save and verify
                    let wav_saved = match wav_handle.await {
                        Ok(Ok(())) => {
                            match crate::audio_toolkit::verify_wav_file(
                                &wav_path_for_verify,
                                sample_count,
                            ) {
                                Ok(()) => true,
                                Err(e) => {
                                    error!("WAV verification failed: {}", e);
                                    false
                                }
                            }
                        }
                        Ok(Err(e)) => {
                            error!("Failed to save WAV file: {}", e);
                            false
                        }
                        Err(e) => {
                            error!("WAV save task panicked: {}", e);
                            false
                        }
                    };

                    if rm.was_cancelled_since(cancel_generation) {
                        debug!("Transcription operation cancelled before output handling");
                        utils::hide_recording_overlay(&ah);
                        change_tray_icon(&ah, TrayIconState::Idle);
                        return;
                    }

                    match transcription_result {
                        Ok(transcription) => {
                            debug!(
                                "Transcription completed in {:?}: '{}'",
                                transcription_time.elapsed(),
                                transcription
                            );

                            if post_process {
                                if use_streaming_overlay {
                                    tm.emit_stream_working(StreamWorkKind::Polishing);
                                } else {
                                    show_processing_overlay(&ah);
                                }
                            }
                            // Started in `stop`, so it has finished or timed out by now.
                            let captured_app = frontmost_capture.await.ok().flatten();
                            let local_llm_cancel = LocalLlmCancel {
                                started: cancel_generation,
                                audio: Arc::clone(&rm),
                            };
                            let Some(processed) = complete_unless_cancelled(
                                process_transcription_output(
                                    &ah,
                                    &transcription,
                                    post_process,
                                    correction,
                                    captured_app.as_ref(),
                                    Some(local_llm_cancel),
                                ),
                                || rm.was_cancelled_since(cancel_generation),
                            )
                            .await
                            else {
                                debug!("Transcription operation cancelled during output handling");
                                utils::hide_recording_overlay(&ah);
                                change_tray_icon(&ah, TrayIconState::Idle);
                                return;
                            };

                            if rm.was_cancelled_since(cancel_generation) {
                                debug!("Transcription operation cancelled before paste");
                                utils::hide_recording_overlay(&ah);
                                change_tray_icon(&ah, TrayIconState::Idle);
                                return;
                            }

                            // Save to history if WAV was saved
                            if wav_saved {
                                if let Err(err) = hm.save_entry(
                                    file_name,
                                    transcription,
                                    post_process,
                                    processed.post_processed_text.clone(),
                                    processed.post_process_prompt.clone(),
                                ) {
                                    error!("Failed to save history entry: {}", err);
                                }
                            }

                            if processed.final_text.is_empty() {
                                utils::hide_recording_overlay(&ah);
                                change_tray_icon(&ah, TrayIconState::Idle);
                            } else {
                                let ah_clone = ah.clone();
                                let paste_time = Instant::now();
                                let final_text = processed.final_text;
                                // Non-zero only in correction mode: delete the
                                // prior output before typing the edit so the
                                // corrected text REPLACES it instead of appending.
                                let replace_char_count = processed.replace_char_count;
                                let rm_for_paste = Arc::clone(&rm);
                                ah.run_on_main_thread(move || {
                                    if rm_for_paste.was_cancelled_since(cancel_generation) {
                                        debug!("Transcription operation cancelled before paste");
                                        utils::hide_recording_overlay(&ah_clone);
                                        change_tray_icon(&ah_clone, TrayIconState::Idle);
                                        return;
                                    }

                                    match utils::replace_previous(
                                        replace_char_count,
                                        final_text,
                                        ah_clone.clone(),
                                    ) {
                                        Ok(()) => debug!(
                                            "Text pasted successfully in {:?}",
                                            paste_time.elapsed()
                                        ),
                                        Err(e) => {
                                            error!("Failed to paste transcription: {}", e);
                                            let _ = ah_clone.emit("paste-error", ());
                                        }
                                    }
                                    utils::hide_recording_overlay(&ah_clone);
                                    change_tray_icon(&ah_clone, TrayIconState::Idle);
                                })
                                .unwrap_or_else(|e| {
                                    error!("Failed to run paste on main thread: {:?}", e);
                                    utils::hide_recording_overlay(&ah);
                                    change_tray_icon(&ah, TrayIconState::Idle);
                                });
                            }
                        }
                        Err(err) => {
                            if rm.was_cancelled_since(cancel_generation) {
                                debug!(
                                    "Transcription operation cancelled after transcription error"
                                );
                                utils::hide_recording_overlay(&ah);
                                change_tray_icon(&ah, TrayIconState::Idle);
                                return;
                            }

                            error!("Transcription failed: {}", err);
                            // Surface the failure to the UI (toast). The full
                            // message is also in coco-voice.log via the line above.
                            let _ = ah.emit("transcription-error", err.to_string());
                            // Save entry with empty text so user can retry
                            if wav_saved {
                                if let Err(save_err) = hm.save_entry(
                                    file_name,
                                    String::new(),
                                    post_process,
                                    None,
                                    None,
                                ) {
                                    error!("Failed to save failed history entry: {}", save_err);
                                }
                            }
                            utils::hide_recording_overlay(&ah);
                            change_tray_icon(&ah, TrayIconState::Idle);
                        }
                    }
                }
            } else {
                debug!("No samples retrieved from recording stop");
                // Tear down any streaming worker so its channel doesn't leak.
                tm.cancel_stream();
                utils::hide_recording_overlay(&ah);
                change_tray_icon(&ah, TrayIconState::Idle);
            }
        });

        debug!(
            "TranscribeAction::stop completed in {:?}",
            stop_time.elapsed()
        );
    }
}

// Cancel Action
struct CancelAction;

impl ShortcutAction for CancelAction {
    fn start(&self, app: &AppHandle, _binding_id: &str, _shortcut_str: &str) {
        utils::cancel_current_operation(app);
    }

    fn stop(&self, _app: &AppHandle, _binding_id: &str, _shortcut_str: &str) {
        // Nothing to do on stop for cancel
    }
}

// Test Action
struct TestAction;

impl ShortcutAction for TestAction {
    fn start(&self, app: &AppHandle, binding_id: &str, shortcut_str: &str) {
        log::info!(
            "Shortcut ID '{}': Started - {} (App: {})", // Changed "Pressed" to "Started" for consistency
            binding_id,
            shortcut_str,
            app.package_info().name
        );
    }

    fn stop(&self, app: &AppHandle, binding_id: &str, shortcut_str: &str) {
        log::info!(
            "Shortcut ID '{}': Stopped - {} (App: {})", // Changed "Released" to "Stopped" for consistency
            binding_id,
            shortcut_str,
            app.package_info().name
        );
    }
}

// Static Action Map
pub static ACTION_MAP: Lazy<HashMap<String, Arc<dyn ShortcutAction>>> = Lazy::new(|| {
    let mut map = HashMap::new();
    map.insert(
        "transcribe".to_string(),
        Arc::new(TranscribeAction {
            post_process: false,
            correction: false,
        }) as Arc<dyn ShortcutAction>,
    );
    map.insert(
        "transcribe_with_post_process".to_string(),
        Arc::new(TranscribeAction {
            post_process: true,
            correction: false,
        }) as Arc<dyn ShortcutAction>,
    );
    // Explicit spoken-correction hotkey: records like a normal dictation, but the
    // utterance edits the last output and the result replaces it at the cursor.
    // `post_process: true` so the "polishing" overlay shows while the LLM edits;
    // the correction branch in `process_transcription_output` owns the behaviour.
    map.insert(
        "correction".to_string(),
        Arc::new(TranscribeAction {
            post_process: true,
            correction: true,
        }) as Arc<dyn ShortcutAction>,
    );
    map.insert(
        "cancel".to_string(),
        Arc::new(CancelAction) as Arc<dyn ShortcutAction>,
    );
    map.insert(
        "test".to_string(),
        Arc::new(TestAction) as Arc<dyn ShortcutAction>,
    );
    map
});

#[cfg(test)]
mod tests {
    use super::{
        app_profile_matches, apply_app_profile_overrides, apply_profile_provider_and_model,
        build_post_process_messages, complete_unless_cancelled, note_cancel_if_generation_moved,
        refine_base_for_correction, should_use_streaming_overlay, store_refine_buffer,
    };
    use crate::commands::app_profile::ActiveAppInfo;
    use crate::settings::OverlayStyle;
    use std::future;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn completed_operation_returns_its_output() {
        let result = tauri::async_runtime::block_on(complete_unless_cancelled(
            future::ready("done"),
            || false,
        ));

        assert_eq!(result, Some("done"));
    }

    #[test]
    fn pending_operation_stops_after_cancellation() {
        let cancelled = Arc::new(AtomicBool::new(false));
        let cancelled_for_thread = Arc::clone(&cancelled);
        let cancel_thread = thread::spawn(move || {
            thread::sleep(Duration::from_millis(10));
            cancelled_for_thread.store(true, Ordering::Release);
        });

        let result = tauri::async_runtime::block_on(complete_unless_cancelled(
            future::pending::<()>(),
            || cancelled.load(Ordering::Acquire),
        ));

        cancel_thread.join().unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn cancel_flag_is_set_only_when_the_generation_counter_moves() {
        let flag = AtomicBool::new(false);

        assert!(!note_cancel_if_generation_moved(&flag, 3, 3));
        assert!(!flag.load(Ordering::Relaxed));

        assert!(note_cancel_if_generation_moved(&flag, 3, 4));
        assert!(flag.load(Ordering::Relaxed));

        // A later sample of the new generation must not clear a cancel.
        assert!(!note_cancel_if_generation_moved(&flag, 4, 4));
        assert!(flag.load(Ordering::Relaxed));
    }

    #[test]
    fn edit_mode_selects_refine_prompt_when_prior_output_present() {
        // Correction mode (prior output present) must build the *edit* prompt —
        // the refine system prompt plus PREVIOUS/CORRECTION user content — rather
        // than the fresh clean-up prompt. This pins the branch selection that
        // `post_process_transcription` relies on to enter edit mode.
        let template = "Clean up the transcription: ${output}";

        let (sys_fresh, user_fresh) = build_post_process_messages(template, "hello world", None);
        assert_eq!(user_fresh, "hello world");
        assert!(!sys_fresh.contains("spoken correction"));

        let (sys_edit, user_edit) =
            build_post_process_messages(template, "make it a question", Some("Hello world."));
        assert!(sys_edit.contains("spoken correction"));
        assert!(user_edit.contains("PREVIOUS:\nHello world."));
        assert!(user_edit.contains("CORRECTION:\nmake it a question"));
    }

    #[test]
    fn refine_buffer_updates_after_correction() {
        // Touches the process-global REFINE_BUFFER; stores immediately before each
        // read so it is deterministic regardless of test ordering/parallelism.
        let mut settings = crate::settings::get_default_settings();
        settings.iterative_correction_enabled = true;

        // A first dictation records its output as the correction base.
        store_refine_buffer(&settings, "raw one", "The quick brown fox");
        assert_eq!(
            refine_base_for_correction(&settings).as_deref(),
            Some("The quick brown fox")
        );

        // After a correction, storing the new output REPLACES the base so a
        // subsequent correction edits (and deletes the length of) the new text.
        store_refine_buffer(&settings, "make it a dog", "The quick brown dog");
        assert_eq!(
            refine_base_for_correction(&settings).as_deref(),
            Some("The quick brown dog")
        );

        // A blank output clears the base rather than storing an empty edit target.
        store_refine_buffer(&settings, "whatever", "   ");
        assert_eq!(refine_base_for_correction(&settings), None);

        // With the feature disabled, no base is offered even if a buffer exists.
        store_refine_buffer(&settings, "raw", "still here");
        settings.iterative_correction_enabled = false;
        assert_eq!(refine_base_for_correction(&settings), None);
    }

    #[test]
    fn live_overlay_uses_streaming_states_only_for_streaming_models() {
        assert!(should_use_streaming_overlay(OverlayStyle::Live, true));
        assert!(!should_use_streaming_overlay(OverlayStyle::Live, false));
        assert!(!should_use_streaming_overlay(OverlayStyle::Minimal, true));
        assert!(!should_use_streaming_overlay(OverlayStyle::None, true));
    }

    #[test]
    fn profile_match_is_case_insensitive_on_name_and_stem() {
        assert!(app_profile_matches(
            "google chrome",
            "Google Chrome",
            "chrome"
        ));
        assert!(app_profile_matches("Code", "Visual Studio Code", "Code"));
        assert!(!app_profile_matches("com.apple.dt.Xcode", "Xcode", "Xcode"));
        assert!(!app_profile_matches("", "Xcode", "Xcode"));
        assert!(!app_profile_matches("   ", "Xcode", "Xcode"));
    }

    fn settings_with_openai() -> crate::settings::AppSettings {
        let mut settings = crate::settings::get_default_settings();
        settings.post_process_provider_id = "openai".to_string();
        settings
            .post_process_models
            .insert("openai".to_string(), "gpt-4o-mini".to_string());
        settings
    }

    fn apply_override(
        settings: &mut crate::settings::AppSettings,
        provider_id: Option<&str>,
        model: Option<&str>,
    ) {
        let profile = crate::settings::AppProfile {
            id: "profile".to_string(),
            name: "Profile".to_string(),
            app_identifier: "App".to_string(),
            prompt_id: None,
            provider_id: provider_id.map(str::to_string),
            model: model.map(str::to_string),
            corrections: Vec::new(),
        };
        apply_profile_provider_and_model(settings, &profile);
    }

    #[test]
    fn provider_without_model_is_ignored() {
        let mut settings = settings_with_openai();
        settings
            .post_process_models
            .insert("groq".to_string(), "   ".to_string());

        apply_override(&mut settings, Some("groq"), None);

        assert_eq!(settings.post_process_provider_id, "openai");
        assert_eq!(
            settings
                .post_process_models
                .get("openai")
                .map(String::as_str),
            Some("gpt-4o-mini")
        );
        assert_eq!(
            settings.post_process_models.get("groq").map(String::as_str),
            Some("   ")
        );
    }

    #[test]
    fn provider_with_profile_model_is_applied() {
        let mut settings = settings_with_openai();

        apply_override(&mut settings, Some("groq"), Some("  llama-3.3-70b  "));

        assert_eq!(settings.post_process_provider_id, "groq");
        assert_eq!(
            settings.post_process_models.get("groq").map(String::as_str),
            Some("llama-3.3-70b")
        );
        assert_eq!(
            settings
                .post_process_models
                .get("openai")
                .map(String::as_str),
            Some("gpt-4o-mini")
        );
    }

    #[test]
    fn blank_profile_model_is_ignored() {
        let mut settings = settings_with_openai();

        apply_override(&mut settings, None, Some("   "));

        assert_eq!(settings.post_process_provider_id, "openai");
        assert_eq!(
            settings
                .post_process_models
                .get("openai")
                .map(String::as_str),
            Some("gpt-4o-mini")
        );

        // A provider that already has a model is selected, but a blank profile
        // model does not replace that model.
        settings
            .post_process_models
            .insert("groq".to_string(), "llama-3.3-70b".to_string());
        apply_override(&mut settings, Some("groq"), Some(" \t "));

        assert_eq!(settings.post_process_provider_id, "groq");
        assert_eq!(
            settings.post_process_models.get("groq").map(String::as_str),
            Some("llama-3.3-70b")
        );
    }

    #[test]
    fn model_is_ignored_when_provider_override_is_rejected() {
        let mut settings = settings_with_openai();

        apply_override(&mut settings, Some("not-a-provider"), Some("sneaky-model"));

        assert_eq!(settings.post_process_provider_id, "openai");
        assert_eq!(
            settings
                .post_process_models
                .get("openai")
                .map(String::as_str),
            Some("gpt-4o-mini")
        );
        assert!(settings.post_process_models.get("not-a-provider").is_none());
    }

    #[test]
    fn model_only_profile_leaves_global_model_unchanged() {
        let mut settings = settings_with_openai();
        let models_before = settings.post_process_models.clone();

        apply_override(&mut settings, None, Some("  gpt-4.1-mini  "));

        assert_eq!(settings.post_process_provider_id, "openai");
        assert_eq!(
            settings
                .post_process_models
                .get("openai")
                .map(String::as_str),
            Some("gpt-4o-mini")
        );
        // No provider was named, so the model is not stored for any provider.
        assert_eq!(settings.post_process_models, models_before);
    }

    #[test]
    fn provider_with_stored_model_is_applied_without_a_profile_model() {
        let mut settings = settings_with_openai();
        settings
            .post_process_models
            .insert("groq".to_string(), "llama-3.3-70b".to_string());

        apply_override(&mut settings, Some("groq"), None);

        assert_eq!(settings.post_process_provider_id, "groq");
        assert_eq!(
            settings.post_process_models.get("groq").map(String::as_str),
            Some("llama-3.3-70b")
        );
    }

    fn app_info(app_name: &str, process_path: &str) -> ActiveAppInfo {
        ActiveAppInfo {
            app_name: app_name.to_string(),
            process_path: process_path.to_string(),
            title: String::new(),
        }
    }

    fn correction_profile(
        app_identifier: &str,
        from: &str,
        to: &str,
    ) -> crate::settings::AppProfile {
        crate::settings::AppProfile {
            id: app_identifier.to_string(),
            name: app_identifier.to_string(),
            app_identifier: app_identifier.to_string(),
            prompt_id: None,
            provider_id: None,
            model: None,
            corrections: vec![crate::settings::CorrectionPair {
                from: from.to_string(),
                to: to.to_string(),
            }],
        }
    }

    fn correction_pairs(settings: &crate::settings::AppSettings) -> Vec<(&str, &str)> {
        settings
            .corrections
            .iter()
            .map(|pair| (pair.from.as_str(), pair.to.as_str()))
            .collect()
    }

    #[test]
    fn override_follows_the_captured_app() {
        let mut settings = crate::settings::get_default_settings();
        settings.app_profiles = vec![
            correction_profile("Mail", "teh", "the"),
            correction_profile("Xcode", "nil", "null"),
        ];

        let mail = app_info("Mail", "/System/Applications/Mail.app/Contents/MacOS/Mail");
        let xcode = app_info("Xcode", "/Applications/Xcode.app/Contents/MacOS/Xcode");

        // The result depends only on the identity passed in, so a later window
        // switch cannot change which profile a finished dictation uses.
        assert_eq!(
            correction_pairs(&apply_app_profile_overrides(&settings, &mail)),
            vec![("teh", "the")]
        );
        assert_eq!(
            correction_pairs(&apply_app_profile_overrides(&settings, &xcode)),
            vec![("nil", "null")]
        );
    }

    #[test]
    fn override_matches_the_process_stem_of_the_captured_path() {
        let mut settings = crate::settings::get_default_settings();
        settings.app_profiles = vec![correction_profile("code", "teh", "the")];

        let vscode = app_info(
            "Visual Studio Code",
            "/Applications/Visual Studio Code.app/Contents/MacOS/Code",
        );

        assert_eq!(
            correction_pairs(&apply_app_profile_overrides(&settings, &vscode)),
            vec![("teh", "the")]
        );
    }

    #[test]
    fn unmatched_captured_app_gets_the_global_settings() {
        let mut settings = crate::settings::get_default_settings();
        settings.app_profiles = vec![correction_profile("Mail", "teh", "the")];

        let notes = app_info(
            "Notes",
            "/System/Applications/Notes.app/Contents/MacOS/Notes",
        );

        assert!(correction_pairs(&apply_app_profile_overrides(&settings, &notes)).is_empty());
    }
}
