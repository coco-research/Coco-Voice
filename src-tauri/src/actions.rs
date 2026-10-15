#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
use crate::apple_intelligence;
use crate::audio_feedback::{play_feedback_sound, play_feedback_sound_blocking, SoundType};
use crate::audio_toolkit::{is_microphone_access_denied, is_no_input_device_error, VadPolicy};
use crate::cleanup_resolver::{
    check_global_preconditions, resolve_cleanup, CleanupDecision, CleanupStatus, SkipReason,
};
use crate::managers::audio::AudioRecordingManager;
use crate::managers::history::HistoryManager;
use crate::managers::model::ModelManager;
use crate::managers::transcription::StreamWorkKind;
use crate::managers::transcription::TranscriptionManager;
use crate::settings::{
    get_settings, AppSettings, OverlayStyle, APPLE_INTELLIGENCE_PROVIDER_ID, LOCAL_LLM_PROVIDER_ID,
};
use crate::shortcut;
use crate::tray::{change_tray_icon, TrayIconState};
use crate::utils::{
    self, show_processing_overlay, show_recording_overlay, show_transcribing_overlay,
};
use crate::TranscriptionCoordinator;
use ferrous_opencc::{config::BuiltinConfig, OpenCC};
use log::{debug, error, info, warn};
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::Manager;
use tauri::{AppHandle, Emitter};
use tauri_specta::Event;

const CANCELLATION_POLL_INTERVAL: Duration = Duration::from_millis(25);

/// How long the overlay stays up after paste when a cleanup status was sent, so the
/// "not cleaned" row can be read.
const STATUS_OVERLAY_GRACE: Duration = Duration::from_millis(2500);

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

/// Hides the overlay once [`STATUS_OVERLAY_GRACE`] has passed, unless a newer overlay
/// was shown since `captured_overlay_gen` was taken: that take owns the overlay and the
/// tray icon now.
fn hide_overlay_after_status(app: &AppHandle, captured_overlay_gen: u64) {
    let app_clone = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(STATUS_OVERLAY_GRACE).await;
        if crate::overlay::hide_recording_overlay_if_current(&app_clone, captured_overlay_gen) {
            change_tray_icon(&app_clone, TrayIconState::Idle);
        }
    });
}

/// Ends a take's overlay: right away, or after the grace period when `delay_hide`.
fn finish_overlay(app: &AppHandle, delay_hide: bool, captured_overlay_gen: u64) {
    if delay_hide {
        hide_overlay_after_status(app, captured_overlay_gen);
    } else {
        utils::hide_recording_overlay(app);
        change_tray_icon(app, TrayIconState::Idle);
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

/// Whether the overlay stays up after the paste: only for a take whose status row
/// is showing, so there must be an overlay to show it on.
fn should_hold_overlay(status_emitted: bool, style: OverlayStyle) -> bool {
    status_emitted && style != OverlayStyle::None
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

/// The settings half of "can this provider run": it is registered, has a model,
/// and has the API key it needs. The caller checks the platform and the machine.
fn provider_configured(settings: &AppSettings, provider_id: &str) -> Result<(), SkipReason> {
    if settings.post_process_provider(provider_id).is_none() {
        return Err(SkipReason::ProviderNotFound);
    }

    // `ensure_post_process_defaults` backfills the on-device model id, so only a
    // provider that really has no model fails here.
    let is_set = |value: Option<&String>| value.is_some_and(|v| !v.trim().is_empty());
    if !is_set(settings.post_process_models.get(provider_id)) {
        return Err(SkipReason::NoModelConfigured);
    }

    // The on-device providers and a custom (local server) endpoint take no key.
    let keyless = matches!(
        provider_id,
        LOCAL_LLM_PROVIDER_ID | APPLE_INTELLIGENCE_PROVIDER_ID | "custom"
    );
    if !keyless && !is_set(settings.post_process_api_keys.get(provider_id)) {
        return Err(SkipReason::NoApiKey);
    }

    Ok(())
}

/// Logs why cleanup was skipped and tells the UI. Returns whether the UI was told.
fn skip_cleanup(app: &AppHandle, reason: &SkipReason) -> bool {
    info!("Post-processing skipped: {reason}");
    let Some(status) = CleanupStatus::status_for_skip(reason) else {
        return false;
    };
    let _ = status.emit(app);
    true
}

/// Post-processes `transcription`. When `prior_output` is `Some`, the call runs
/// in *edit mode*: instead of cleaning a fresh transcript, the model edits the
/// previous output using `transcription` as the spoken correction instruction.
/// Sets `status_emitted` when it told the UI that cleanup did not run.
async fn post_process_transcription(
    app: &AppHandle,
    settings: &AppSettings,
    transcription: &str,
    prior_output: Option<&str>,
    local_llm_cancel: Option<LocalLlmCancel>,
    status_emitted: &mut bool,
) -> Option<String> {
    // Every provider path funnels through here: a blank result (zero-width
    // characters survive `trim`) must fall back to the raw transcript.
    non_blank(
        post_process_transcription_inner(
            app,
            settings,
            transcription,
            prior_output,
            local_llm_cancel,
            status_emitted,
        )
        .await,
    )
}

/// `None` for an output that is empty once invisible characters are removed.
fn non_blank(output: Option<String>) -> Option<String> {
    output
        .map(|text| strip_invisible_chars(&text))
        .filter(|text| !text.trim().is_empty())
}

async fn post_process_transcription_inner(
    app: &AppHandle,
    settings: &AppSettings,
    transcription: &str,
    prior_output: Option<&str>,
    local_llm_cancel: Option<LocalLlmCancel>,
    status_emitted: &mut bool,
) -> Option<String> {
    // Only the macOS-only on-device path reads the cancel signal.
    #[cfg(not(target_os = "macos"))]
    let _ = &local_llm_cancel;

    let prompt = match check_global_preconditions(
        transcription,
        settings.post_process_selected_prompt_id.as_deref(),
        |id| {
            settings
                .post_process_prompts
                .iter()
                .find(|prompt| prompt.id == id)
                .map(|prompt| prompt.prompt.as_str())
        },
    ) {
        Ok(prompt) => prompt.to_string(),
        Err(reason) => {
            *status_emitted |= skip_cleanup(app, &reason);
            return None;
        }
    };

    let model_for = |provider_id: &str| {
        settings
            .post_process_models
            .get(provider_id)
            .cloned()
            .unwrap_or_default()
    };

    let provider_usable = |provider_id: &str| -> Result<(), SkipReason> {
        provider_configured(settings, provider_id)?;

        if provider_id == APPLE_INTELLIGENCE_PROVIDER_ID {
            #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
            if !apple_intelligence::check_apple_intelligence_availability() {
                return Err(SkipReason::AppleIntelligenceUnavailable);
            }
            #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
            return Err(SkipReason::ProviderUnavailableInThisBuild);
        }

        if provider_id == LOCAL_LLM_PROVIDER_ID {
            #[cfg(not(target_os = "macos"))]
            return Err(SkipReason::ProviderUnavailableInThisBuild);
            #[cfg(target_os = "macos")]
            if app
                .state::<Arc<ModelManager>>()
                .get_model_path(&model_for(provider_id))
                .is_err()
            {
                return Err(SkipReason::LocalModelNotDownloaded);
            }
        }

        Ok(())
    };

    let provider_id = match resolve_cleanup(&settings.post_process_provider_id, provider_usable) {
        CleanupDecision::Run {
            provider_id,
            fallback_from,
        } => {
            if let Some((selected, reason)) = fallback_from {
                info!("Cleanup: {selected} unavailable ({reason}), using on-device model");
            }
            provider_id
        }
        CleanupDecision::Skip { reason } => {
            *status_emitted |= skip_cleanup(app, &reason);
            return None;
        }
    };

    // The resolver only returns providers that are in settings.
    let provider = settings.post_process_provider(&provider_id).cloned()?;
    let model = model_for(&provider_id);

    debug!(
        "Starting LLM post-processing with provider '{}' (model: {})",
        provider.id, model
    );

    // Local LLM runs llama.cpp on device, not the JSON-schema HTTP API, so it
    // is dispatched before the structured-output gate. The provider keeps
    // `supports_structured_output: false`; hoisting the call is what makes
    // local cleanup reachable.
    #[cfg(target_os = "macos")]
    if provider.id == LOCAL_LLM_PROVIDER_ID {
        return post_process_with_local_llm(
            app,
            &model,
            &prompt,
            transcription,
            prior_output,
            local_llm_cancel,
            status_emitted,
        )
        .await;
    }

    let api_key = settings
        .post_process_api_keys
        .get(&provider_id)
        .cloned()
        .unwrap_or_default();

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

/// The status for an on-device run that was refused or failed. A cancel is the
/// user's own doing, so it gets none.
#[cfg(target_os = "macos")]
fn local_llm_error_status(err: &crate::local_llm::LocalLlmError) -> Option<CleanupStatus> {
    use crate::local_llm::LocalLlmError;
    match err {
        LocalLlmError::OverBudget => Some(CleanupStatus::too_long()),
        LocalLlmError::StoppedEarly => Some(CleanupStatus::stopped_early()),
        LocalLlmError::Cancelled => None,
        LocalLlmError::Other(_) => Some(CleanupStatus::model_load_failed()),
    }
}

/// Cleans `transcription` with the on-device model (llama.cpp, macOS only).
/// `None` means the raw transcript is pasted: the model is missing, refused the
/// text, failed, or the dictation was cancelled. Sets `status_emitted` when it told
/// the UI why.
#[cfg(target_os = "macos")]
async fn post_process_with_local_llm(
    app: &AppHandle,
    model: &str,
    prompt: &str,
    transcription: &str,
    prior_output: Option<&str>,
    cancel: Option<LocalLlmCancel>,
    status_emitted: &mut bool,
) -> Option<String> {
    let model_path = match app.state::<Arc<ModelManager>>().get_model_path(model) {
        Ok(path) => path,
        Err(err) => {
            error!("Failed to resolve local LLM model path: {}", err);
            *status_emitted |= skip_cleanup(app, &SkipReason::LocalModelNotDownloaded);
            return None;
        }
    };
    let (system_prompt, user_content) =
        build_post_process_messages(prompt, transcription, prior_output);

    let cancel_watch = cancel.map(|cancel| {
        let flag = Arc::new(AtomicBool::new(false));
        let watcher = watch_generation_cancel(Arc::clone(&flag), cancel.started, cancel.audio);
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
    // Finished run: stop polling. A dropped future skips this and leaves the
    // watcher attached so it can still raise the flag.
    if let Some((_, watcher)) = &cancel_watch {
        watcher.abort();
    }

    match generated {
        Ok(result) => {
            debug!(
                "Local LLM post-processing succeeded. Output length: {} chars",
                result.len()
            );
            Some(result)
        }
        Err(crate::local_llm::LocalLlmError::Cancelled) => {
            debug!("Local LLM cleanup was cancelled");
            None
        }
        Err(err) => {
            error!("Local LLM post-processing failed: {}", err);
            if let Some(status) = local_llm_error_status(&err) {
                let _ = status.emit(app);
                *status_emitted = true;
            }
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
    /// A `cleanup-status` event was sent for this take, so the overlay shows a
    /// "not cleaned" row that the paste path keeps up for [`STATUS_OVERLAY_GRACE`].
    pub status_emitted: bool,
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
    local_llm_cancel: Option<LocalLlmCancel>,
) -> ProcessedTranscription {
    let settings = get_settings(app);
    let mut final_text = transcription.to_string();
    let mut post_processed_text: Option<String> = None;
    let mut post_process_prompt: Option<String> = None;
    let mut replace_char_count: usize = 0;
    let mut status_emitted = false;

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
        if let Some(prior_output) = refine_base_for_correction(&settings) {
            // The prior output is what the previous dictation pasted (plus the
            // trailing space `paste` appends when that setting is on), so its
            // grapheme length is how many Backspaces the replace must send before
            // typing the edit. Graphemes, not scalars or bytes: one Backspace
            // deletes one grapheme cluster (combining marks, joined emoji).
            let prev_char_count =
                crate::utils::pasted_grapheme_count(&prior_output, settings.append_trailing_space);
            match post_process_transcription(
                app,
                &settings,
                &final_text,
                Some(&prior_output),
                local_llm_cancel,
                &mut status_emitted,
            )
            .await
            {
                Some(edited) => {
                    post_processed_text = Some(edited.clone());
                    final_text = edited;
                    // The guard clamps an implausibly long prior output to 0
                    // (append instead of hammering Backspace across the document).
                    replace_char_count = crate::utils::backspaces_for_replace(prev_char_count);

                    if let Some(prompt_id) = &settings.post_process_selected_prompt_id {
                        if let Some(prompt) = settings
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
            &settings,
            &final_text,
            None,
            local_llm_cancel,
            &mut status_emitted,
        )
        .await
        {
            post_processed_text = Some(processed_text.clone());
            final_text = processed_text;

            if let Some(prompt_id) = &settings.post_process_selected_prompt_id {
                if let Some(prompt) = settings
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
    if !settings.corrections.is_empty() {
        final_text = crate::audio_toolkit::apply_corrections(&final_text, &settings.corrections);
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
        status_emitted,
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

                            // A status row stays up for a moment after the paste so it can
                            // be read.
                            let delay_hide = should_hold_overlay(processed.status_emitted, style);
                            let captured_overlay_gen = crate::overlay::capture_overlay_generation();

                            if processed.final_text.is_empty() {
                                finish_overlay(&ah, delay_hide, captured_overlay_gen);
                            } else {
                                let ah_clone = ah.clone();
                                let paste_time = Instant::now();
                                let final_text = processed.final_text;
                                // Non-zero only in correction mode: delete the
                                // prior output before typing the edit so the
                                // corrected text REPLACES it instead of appending.
                                let replace_char_count = processed.replace_char_count;
                                let rm_for_paste = Arc::clone(&rm);
                                // Sleeps in paste must not run on the UI thread.
                                // Enigo still hops there itself. spawn_blocking
                                // so those sleeps do not park this async worker.
                                let pasted = tauri::async_runtime::spawn_blocking(move || {
                                    if rm_for_paste.was_cancelled_since(cancel_generation) {
                                        return None;
                                    }
                                    Some(utils::replace_previous(
                                        replace_char_count,
                                        final_text,
                                        ah_clone,
                                    ))
                                })
                                .await;

                                let cancelled_before_paste = matches!(pasted, Ok(None));
                                match pasted {
                                    Ok(Some(Ok(()))) => debug!(
                                        "Text pasted successfully in {:?}",
                                        paste_time.elapsed()
                                    ),
                                    Ok(Some(Err(e))) => {
                                        error!("Failed to paste transcription: {}", e);
                                        let _ = ah.emit("paste-error", ());
                                    }
                                    Ok(None) => {
                                        debug!("Transcription operation cancelled before paste");
                                    }
                                    Err(e) => {
                                        // JoinError: the blocking paste task panicked or was
                                        // cancelled, not a main-thread dispatch failure.
                                        error!("Paste task panicked or was cancelled: {e:?}");
                                        let _ = ah.emit("paste-error", ());
                                    }
                                }
                                if cancelled_before_paste {
                                    utils::hide_recording_overlay(&ah);
                                    change_tray_icon(&ah, TrayIconState::Idle);
                                } else {
                                    finish_overlay(&ah, delay_hide, captured_overlay_gen);
                                }
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
    #[cfg(target_os = "macos")]
    use super::local_llm_error_status;
    use super::{
        build_post_process_messages, complete_unless_cancelled, non_blank,
        note_cancel_if_generation_moved, provider_configured, refine_base_for_correction,
        should_hold_overlay, should_use_streaming_overlay, store_refine_buffer, SkipReason,
    };
    use crate::settings::OverlayStyle;
    use std::future;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn non_blank_rejects_invisible_only_output() {
        assert_eq!(non_blank(Some("\u{200B}\u{FEFF}".into())), None);
        assert_eq!(non_blank(Some("  \n".into())), None);
        assert_eq!(non_blank(None), None);
        assert_eq!(non_blank(Some("Hi\u{200B}".into())), Some("Hi".into()));
    }

    #[test]
    fn a_provider_needs_a_model_and_the_key_it_takes() {
        let mut settings = crate::settings::get_default_settings();
        let set_model = |s: &mut crate::settings::AppSettings, id: &str, model: &str| {
            s.post_process_models.insert(id.into(), model.into());
        };

        assert_eq!(
            provider_configured(&settings, "nope"),
            Err(SkipReason::ProviderNotFound)
        );

        // A hosted provider needs a model, then a key.
        assert_eq!(
            provider_configured(&settings, "openai"),
            Err(SkipReason::NoModelConfigured)
        );
        set_model(&mut settings, "openai", "  ");
        assert_eq!(
            provider_configured(&settings, "openai"),
            Err(SkipReason::NoModelConfigured)
        );
        set_model(&mut settings, "openai", "gpt-4o-mini");
        assert_eq!(
            provider_configured(&settings, "openai"),
            Err(SkipReason::NoApiKey)
        );
        settings
            .post_process_api_keys
            .insert("openai".into(), "sk-test".into());
        assert_eq!(provider_configured(&settings, "openai"), Ok(()));

        // A custom (local server) endpoint needs no key.
        set_model(&mut settings, "custom", "llama3");
        assert_eq!(provider_configured(&settings, "custom"), Ok(()));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_refused_or_failed_on_device_run_has_a_status_but_a_cancel_does_not() {
        use crate::cleanup_resolver::CleanupStatus;
        use crate::local_llm::LocalLlmError;

        assert_eq!(
            local_llm_error_status(&LocalLlmError::OverBudget),
            Some(CleanupStatus::too_long())
        );
        assert_eq!(
            local_llm_error_status(&LocalLlmError::StoppedEarly),
            Some(CleanupStatus::stopped_early())
        );
        assert_eq!(
            local_llm_error_status(&LocalLlmError::Other("load failed".into())),
            Some(CleanupStatus::model_load_failed())
        );
        assert_eq!(local_llm_error_status(&LocalLlmError::Cancelled), None);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_on_device_provider_is_configured_by_default_without_a_key() {
        let settings = crate::settings::get_default_settings();
        assert_eq!(
            provider_configured(&settings, crate::settings::LOCAL_LLM_PROVIDER_ID),
            Ok(())
        );
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
    fn the_overlay_is_held_only_for_a_status_it_is_showing() {
        assert!(should_hold_overlay(true, OverlayStyle::Minimal));
        assert!(should_hold_overlay(true, OverlayStyle::Live));
        assert!(!should_hold_overlay(true, OverlayStyle::None));
        assert!(!should_hold_overlay(false, OverlayStyle::Live));
    }

    #[test]
    fn live_overlay_uses_streaming_states_only_for_streaming_models() {
        assert!(should_use_streaming_overlay(OverlayStyle::Live, true));
        assert!(!should_use_streaming_overlay(OverlayStyle::Live, false));
        assert!(!should_use_streaming_overlay(OverlayStyle::Minimal, true));
        assert!(!should_use_streaming_overlay(OverlayStyle::None, true));
    }
}
