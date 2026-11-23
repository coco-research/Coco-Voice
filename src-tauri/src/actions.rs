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
use serde::{Deserialize, Serialize};
use specta::Type;
use std::collections::HashMap;
use std::future::Future;
use std::sync::atomic::AtomicBool;
#[cfg(all(test, feature = "meetings"))]
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::Manager;
use tauri::{AppHandle, Emitter};
use tauri_specta::Event;

const CANCELLATION_POLL_INTERVAL: Duration = Duration::from_millis(25);

/// How long the overlay stays up after paste when a cleanup status was sent, so the
/// "not cleaned" row can be read.
const STATUS_OVERLAY_GRACE: Duration = Duration::from_millis(2500);

const PILL_OUTCOME_SENT_DELAY: Duration = Duration::from_millis(1500);
const PILL_OUTCOME_CLIPBOARD_DELAY: Duration = Duration::from_millis(2500);
const PILL_OUTCOME_KEPT_DELAY: Duration = Duration::from_millis(4000);
const MAX_PILL_APP_NAME_CHARS: usize = 24;

/// Debug-only test seam to force a single paste failure so the tester can
/// verify the "Words kept · press ⌘V" pill on Mac (AC-E13). Strictly omitted
/// from release builds (`#[cfg(debug_assertions)]`).
#[cfg(debug_assertions)]
static FAIL_NEXT_PASTE: AtomicBool = AtomicBool::new(false);

#[cfg(debug_assertions)]
#[tauri::command]
#[specta::specta]
pub fn debug_set_fail_next_paste(enabled: bool) {
    FAIL_NEXT_PASTE.store(enabled, Ordering::SeqCst);
}

#[cfg(debug_assertions)]
fn take_fail_next_paste() -> bool {
    FAIL_NEXT_PASTE.swap(false, Ordering::SeqCst)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PillOutcome {
    Sent {
        #[serde(rename = "targetApp", skip_serializing_if = "Option::is_none")]
        target_app: Option<String>,
    },
    Clipboard,
    KeptPressPaste,
}

impl PillOutcome {
    /// Duration the outcome pill remains visible before hiding (D-E2).
    pub fn hide_delay(&self) -> Duration {
        match self {
            Self::Sent { .. } => PILL_OUTCOME_SENT_DELAY,
            Self::Clipboard => PILL_OUTCOME_CLIPBOARD_DELAY,
            Self::KeptPressPaste => PILL_OUTCOME_KEPT_DELAY,
        }
    }

    /// Plain-language announcement spoken by VoiceOver / screen readers (AC-E12).
    pub fn announcement_text(&self, is_macos: bool) -> String {
        match self {
            Self::Sent {
                target_app: Some(app),
            } => format!("Sent to {app}"),
            Self::Sent { target_app: None } => "Sent".to_string(),
            Self::Clipboard => "On the clipboard".to_string(),
            Self::KeptPressPaste => {
                if is_macos {
                    "Words kept, press Command V".to_string()
                } else {
                    "Words kept, press Control V".to_string()
                }
            }
        }
    }
}

/// Truncates target app names longer than 24 characters with an ellipsis.
pub fn truncate_app_name(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.chars().count() <= MAX_PILL_APP_NAME_CHARS {
        return Some(trimmed.to_string());
    }
    let mut out: String = trimmed.chars().take(MAX_PILL_APP_NAME_CHARS).collect();
    out.push('…');
    Some(out)
}

/// Pure outcome classification for the recording pill (AC-E1).
///
/// Covers every case returned by the blocking paste task:
/// - `Ok(Some(Ok(_)))` + `PasteMethod::None` => `Clipboard`
/// - `Ok(Some(Ok(_)))` + real paste => `Sent(Some(app))` or `Sent(None)`
/// - `Ok(Some(Err(f)))` + `f.kept_on_clipboard` => `KeptPressPaste`
/// - `Ok(Some(Err(f)))` + `!f.kept_on_clipboard` => `None` (toast still fires)
/// - `Ok(None)` (cancelled) => `None`
/// - `Err(_)` (panicked) => `None` (toast still fires)
pub fn pill_outcome(
    paste_result: &Result<Option<Result<bool, crate::clipboard::PasteFailure>>, ()>,
    paste_method: crate::settings::PasteMethod,
    active_app: Option<&crate::app_profiles::ActiveAppInfo>,
) -> Option<PillOutcome> {
    match paste_result {
        Ok(Some(Ok(_))) => {
            if paste_method == crate::settings::PasteMethod::None {
                Some(PillOutcome::Clipboard)
            } else {
                let target_app = active_app.and_then(|a| truncate_app_name(&a.app_name));
                Some(PillOutcome::Sent { target_app })
            }
        }
        Ok(Some(Err(failure))) => {
            if failure.kept_on_clipboard {
                Some(PillOutcome::KeptPressPaste)
            } else {
                None
            }
        }
        Ok(None) | Err(()) => None,
    }
}

/// Invokes `announcer` once with `text` when non-empty (AC-E12).
pub fn announce_with<F: FnMut(&str)>(text: &str, mut announcer: F) {
    let trimmed = text.trim();
    if !trimmed.is_empty() {
        announcer(trimmed);
    }
}

/// Local cleanup length. A catalog model id is not a token count, so this is
/// not parsed out of the model string.
#[cfg(target_os = "macos")]
const LOCAL_LLM_MAX_TOKENS: i32 = 512;

/// How long to wait for a stream worker that overran its finalize timeout to
/// return the engine before giving up on the batch fallback.
const STREAM_ENGINE_RETURN_WAIT: Duration = Duration::from_secs(10);

/// Dictation batch transcription. With the meetings feature this holds the
/// meeting lock so a chunk and a take cannot both take the engine, and gives up
/// while it waits only when the take was cancelled.
fn transcribe_batch(
    tm: &TranscriptionManager,
    samples: Vec<f32>,
    is_cancelled: &dyn Fn() -> bool,
) -> anyhow::Result<String> {
    #[cfg(feature = "meetings")]
    {
        tm.transcribe_with_meeting_lock_unless(samples, is_cancelled)
    }
    #[cfg(not(feature = "meetings"))]
    {
        let _ = is_cancelled;
        tm.transcribe(samples)
    }
}

/// Skip speaker mute for the whole meeting, including a dictation take.
fn apply_recording_mute(rm: &AudioRecordingManager, token: u64) {
    #[cfg(feature = "meetings")]
    if crate::managers::meetings::meeting_is_active() {
        return;
    }
    rm.apply_mute(token);
}

#[cfg(all(test, feature = "meetings"))]
pub(crate) static POST_PROCESS_CALLS: AtomicUsize = AtomicUsize::new(0);

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

/// Hides the overlay once `delay` has passed, unless a newer overlay was shown
/// since `captured_overlay_gen` was taken: that take owns the overlay and the
/// tray icon now.
fn hide_overlay_after_delay(app: &AppHandle, delay: Duration, captured_overlay_gen: u64) {
    let app_clone = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(delay).await;
        if crate::overlay::hide_recording_overlay_if_current(&app_clone, captured_overlay_gen) {
            change_tray_icon(&app_clone, TrayIconState::Idle);
        }
    });
}

/// Ends a take's overlay: right away, or after `delay` when `Some(delay)`.
fn finish_overlay_with_delay(app: &AppHandle, delay: Option<Duration>, captured_overlay_gen: u64) {
    if let Some(wait) = delay {
        hide_overlay_after_delay(app, wait, captured_overlay_gen);
    } else {
        utils::hide_recording_overlay(app);
        change_tray_icon(app, TrayIconState::Idle);
    }
}

/// Ends a take's overlay: right away, or after the grace period when `delay_hide`.
fn finish_overlay(app: &AppHandle, delay_hide: bool, captured_overlay_gen: u64) {
    finish_overlay_with_delay(
        app,
        delay_hide.then_some(STATUS_OVERLAY_GRACE),
        captured_overlay_gen,
    );
}

fn emit_pill_outcome(app: &AppHandle, outcome: &PillOutcome) {
    if let Some(overlay_window) = app.get_webview_window("recording_overlay") {
        let _ = overlay_window.emit("pill-outcome", outcome);
    }
}

#[cfg(target_os = "macos")]
fn spawn_fix_offer_watcher(app: AppHandle, pasted_text: String, app_name: String) {
    let Some(cancel) = app.try_state::<Arc<crate::fix_offer::WatcherCancellation>>() else {
        return;
    };
    let cancel_arc = Arc::clone(cancel.inner());
    let generation = cancel_arc.next_generation();
    let Some(ax_el) = crate::fix_offer::RetainedAxElement::capture_focused() else {
        return;
    };
    tauri::async_runtime::spawn(async move {
        let mut prev_delay = Duration::ZERO;
        for &target in crate::fix_offer::FIX_OFFER_POLL_DELAYS {
            let sleep_for = target.saturating_sub(prev_delay);
            prev_delay = target;
            tokio::time::sleep(sleep_for).await;
            if !cancel_arc.is_current(generation) {
                return;
            }
            let live = get_settings(&app);
            let blocked = !matches!(
                crate::app_profiles::check_active_app_start(&live),
                crate::app_profiles::StartDecision::Allow
            );
            let gate = crate::fix_offer::FixOfferGate {
                offer_remember_fix_enabled: live.offer_remember_fix_enabled,
                never_store_history: live.never_store_history,
                dictation_blocked: blocked,
            };
            match crate::fix_offer::evaluate_poll_tick(
                gate,
                &ax_el,
                &pasted_text,
                0,
                &live.custom_words,
                &live.corrections,
            ) {
                crate::fix_offer::PollStepOutcome::Stop => return,
                crate::fix_offer::PollStepOutcome::Continue => continue,
                crate::fix_offer::PollStepOutcome::Offer { old, new } => {
                    if let Some(queue) = app.try_state::<Mutex<crate::fix_offer::FixOfferQueue>>() {
                        let now_ms = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_millis() as u64)
                            .unwrap_or(0);
                        let pushed = queue.lock().unwrap_or_else(|e| e.into_inner()).push_offer(
                            old,
                            new,
                            app_name.clone(),
                            now_ms,
                        );
                        if let Some(payload) = pushed {
                            let _ = app.emit("fix-offer", payload);
                        }
                    }
                    return;
                }
            }
        }
    });
}

// Shortcut Action Trait
pub trait ShortcutAction: Send + Sync {
    fn start(&self, app: &AppHandle, binding_id: &str, shortcut_str: &str);
    fn stop(&self, app: &AppHandle, binding_id: &str, shortcut_str: &str);
}

/// Meeting shortcut. It only opens the consent dialog: never a start, and an
/// earlier accept is not remembered as consent (the state machine enforces the
/// second part). Ships with an empty binding, so it never fires until assigned.
#[cfg(feature = "meetings")]
struct MeetingConsentAction;

#[cfg(feature = "meetings")]
impl ShortcutAction for MeetingConsentAction {
    fn start(&self, app: &AppHandle, _binding_id: &str, _shortcut_str: &str) {
        let _ = crate::meeting_controller::open_meeting_consent(app.clone());
    }

    fn stop(&self, _app: &AppHandle, _binding_id: &str, _shortcut_str: &str) {}
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

/// Whether a dictation take runs clean-up: either the dedicated clean-up
/// shortcut forced it, or "Clean every take" (`post_process_enabled`) is on.
fn should_post_process_take(action_forces_post_process: bool, post_process_enabled: bool) -> bool {
    action_forces_post_process || post_process_enabled
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
            "Correction hotkey: editing previous output (prior raw transcript: {} chars)",
            buf.raw_transcript.chars().count()
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
/// Sets `status_emitted` when it told the UI that cleanup did not run, and sets
/// `used_local_stock_prompt` when the dispatched call actually ran the on-device
/// model with the unedited stock cleanup prompt (including after fallback).
async fn post_process_transcription(
    app: &AppHandle,
    settings: &AppSettings,
    transcription: &str,
    prior_output: Option<&str>,
    local_llm_cancel: Option<LocalLlmCancel>,
    status_emitted: &mut bool,
    used_local_stock_prompt: &mut bool,
) -> Option<String> {
    #[cfg(all(test, feature = "meetings"))]
    POST_PROCESS_CALLS.fetch_add(1, Ordering::SeqCst);
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
            used_local_stock_prompt,
        )
        .await,
    )
}

/// Resolves the prompt text and whether it is the unedited on-device stock
/// cleanup prompt when dispatching to the local LLM provider.
#[cfg(any(target_os = "macos", test))]
fn resolve_local_llm_prompt<'a>(
    settings: &'a AppSettings,
    fallback_prompt: &'a str,
) -> (&'a str, bool) {
    let local_prompt = settings
        .post_process_prompts
        .iter()
        .find(|candidate| {
            settings.post_process_selected_prompt_id.as_deref() == Some(candidate.id.as_str())
        })
        .map(crate::settings::prompt_for_local_llm)
        .unwrap_or(fallback_prompt);
    let is_stock = local_prompt == crate::settings::LOCAL_LLM_CLEANUP_PROMPT;
    (local_prompt, is_stock)
}

/// Resolves the prompt text and `keep_every_word` stock-prompt flag for a
/// cleanup provider. Both on-device providers (`local_llm` and
/// `apple_intelligence`) substitute the short on-device prompt and arm
/// `keep_every_word = true` when an unedited stock prompt is selected.
#[cfg(any(target_os = "macos", test))]
fn resolve_provider_prompt<'a>(
    provider_id: &str,
    settings: &'a AppSettings,
    fallback_prompt: &'a str,
) -> (&'a str, bool) {
    if provider_id == LOCAL_LLM_PROVIDER_ID || provider_id == APPLE_INTELLIGENCE_PROVIDER_ID {
        resolve_local_llm_prompt(settings, fallback_prompt)
    } else {
        (fallback_prompt, false)
    }
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
    used_local_stock_prompt: &mut bool,
) -> Option<String> {
    *used_local_stock_prompt = false;
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
        // On-device providers follow the short local prompt for unedited stock prompts.
        let (local_prompt, is_stock) =
            resolve_provider_prompt(&provider.id, settings, prompt.as_str());
        *used_local_stock_prompt = is_stock;
        return post_process_with_local_llm(
            app,
            &model,
            local_prompt,
            transcription,
            prior_output,
            local_llm_cancel,
            status_emitted,
            streams_cleanup_progress(&provider.id),
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

        #[cfg(target_os = "macos")]
        let (effective_prompt, provider_stock_prompt) =
            resolve_provider_prompt(&provider.id, settings, prompt.as_str());
        #[cfg(not(target_os = "macos"))]
        let (effective_prompt, provider_stock_prompt) = (prompt.as_str(), false);
        #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
        let _ = provider_stock_prompt;

        let (system_prompt, user_content) =
            build_post_process_messages(effective_prompt, transcription, prior_output);

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
                *used_local_stock_prompt = provider_stock_prompt;

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

/// Live cleaned text while the on-device model runs. Cloud providers do not emit it.
/// `done` is the last snapshot of a run whose text will be pasted.
#[derive(Clone, Debug, Serialize, Deserialize, Type, tauri_specta::Event)]
pub struct CleanupProgress {
    pub text: String,
    pub done: bool,
}

/// Gap between overlay snapshots. 67 ms is 1000/15 rounded up, so the rate stays
/// at or under 15 Hz. The final snapshot is not held to this gap.
const CLEANUP_PROGRESS_MIN_GAP_MS: u64 = 67;

/// Prefix that ends on whitespace, including that whitespace. A trailing partial
/// word is left out. Whitespace-only input is not a word.
///
/// ponytail: scripts without spaces (CJK) update on the final snapshot only.
/// Upgrade path: Unicode word breaks if a space is the wrong boundary.
#[cfg_attr(not(any(test, target_os = "macos")), allow(dead_code))]
fn completed_words(text: &str) -> &str {
    let Some(start) = text.rfind(char::is_whitespace) else {
        return "";
    };
    // `rfind` is the first byte of the whitespace char. A non-breaking space is
    // two bytes; slicing through only the first byte panics.
    let Some(ws) = text[start..].chars().next() else {
        return "";
    };
    let prefix = &text[..start + ws.len_utf8()];
    if prefix.trim().is_empty() {
        ""
    } else {
        prefix
    }
}

/// Next word-boundary snapshot. `elapsed_ms` is `None` until the first emit,
/// which is not delayed. `None` means nothing new, or the 15 Hz gap has not elapsed.
#[cfg_attr(not(any(test, target_os = "macos")), allow(dead_code))]
fn next_cleanup_preview<'a>(
    accumulated: &'a str,
    last_emitted: &str,
    elapsed_ms: Option<u64>,
    min_gap_ms: u64,
) -> Option<&'a str> {
    let visible = completed_words(accumulated);
    if visible.is_empty() || visible == last_emitted {
        return None;
    }
    if let Some(elapsed) = elapsed_ms {
        if elapsed < min_gap_ms {
            return None;
        }
    }
    Some(visible)
}

/// Progress events exist only for the on-device model.
#[cfg_attr(not(any(test, target_os = "macos")), allow(dead_code))]
fn streams_cleanup_progress(provider_id: &str) -> bool {
    provider_id == LOCAL_LLM_PROVIDER_ID
}

#[cfg(target_os = "macos")]
fn emit_cleanup_progress(app: &AppHandle, text: &str, done: bool) {
    let _ = CleanupProgress {
        text: text.to_string(),
        done,
    }
    .emit(app);
}

/// Cancelling cleanup drops the caller future. Abort the preview then, and leave
/// the cancel watcher alone so it can still stop the model.
#[cfg(target_os = "macos")]
struct AbortOnDrop(Option<tauri::async_runtime::JoinHandle<()>>);

#[cfg(target_os = "macos")]
impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        if let Some(handle) = self.0.take() {
            handle.abort();
        }
    }
}

#[cfg(target_os = "macos")]
fn elapsed_ms(since: Option<Instant>) -> Option<u64> {
    since.map(|t| u64::try_from(t.elapsed().as_millis()).unwrap_or(u64::MAX))
}

/// Forward model pieces at ≤ 15 Hz, only on a word boundary. The caller aborts
/// this task when generation ends and emits the final snapshot itself.
#[cfg(target_os = "macos")]
async fn forward_cleanup_progress(mut rx: tokio::sync::mpsc::Receiver<String>, app: AppHandle) {
    let mut accumulated = String::new();
    // Think-stripped text. Recomputed only after a new piece is appended.
    let mut visible = String::new();
    let mut last_emitted = String::new();
    let mut last_emit: Option<Instant> = None;
    loop {
        if let Some(text) = next_cleanup_preview(
            &visible,
            &last_emitted,
            elapsed_ms(last_emit),
            CLEANUP_PROGRESS_MIN_GAP_MS,
        ) {
            emit_cleanup_progress(&app, text, false);
            last_emitted = text.to_string();
            last_emit = Some(Instant::now());
            continue;
        }

        let pending = {
            let boundary = completed_words(&visible);
            !boundary.is_empty() && boundary != last_emitted
        };
        let piece = if pending {
            let elapsed = elapsed_ms(last_emit).unwrap_or(0);
            let wait = CLEANUP_PROGRESS_MIN_GAP_MS.saturating_sub(elapsed);
            tokio::select! {
                biased;
                piece = rx.recv() => piece,
                _ = tokio::time::sleep(Duration::from_millis(wait)) => continue,
            }
        } else {
            rx.recv().await
        };
        let Some(piece) = piece else {
            break;
        };
        accumulated.push_str(&piece);
        visible = crate::local_llm::strip_think_blocks(&accumulated);
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
/// the UI why. `stream_progress` is only the live-preview decision.
#[cfg(target_os = "macos")]
async fn post_process_with_local_llm(
    app: &AppHandle,
    model: &str,
    prompt: &str,
    transcription: &str,
    prior_output: Option<&str>,
    cancel: Option<LocalLlmCancel>,
    status_emitted: &mut bool,
    stream_progress: bool,
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

    // No words yet. The overlay switches to "Polishing…" and shows the raw
    // text in quiet ink. Word snapshots start once a boundary exists.
    let (token_tx, forwarder) = if stream_progress {
        emit_cleanup_progress(app, "", false);
        let (tx, rx) = tokio::sync::mpsc::channel(64);
        let app_forward = app.clone();
        (
            Some(tx),
            Some(AbortOnDrop(Some(tauri::async_runtime::spawn(async move {
                forward_cleanup_progress(rx, app_forward).await;
            })))),
        )
    } else {
        (None, None)
    };

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
        token_tx,
        cancel_watch.as_ref().map(|(flag, _)| Arc::clone(flag)),
    )
    .await;
    // Stop the preview before paste. Abort returns at the next await, so this
    // does not drain the 15 Hz backlog. Dropping this future earlier skips the
    // take and AbortOnDrop aborts the preview instead.
    if let Some(mut preview) = forwarder {
        if let Some(handle) = preview.0.take() {
            handle.abort();
            let _ = handle.await;
        }
    }
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
            // Same gate as paste (`non_blank`). A blank, whitespace-only, or
            // zero-width-only result is not a cleaned snapshot: the overlay
            // keeps the raw transcript, which is what gets pasted. A real
            // result is not held to the throttle, including a last word with
            // no trailing space.
            let cleaned = non_blank(Some(result));
            if stream_progress {
                if let Some(text) = &cleaned {
                    emit_cleanup_progress(app, text, true);
                }
            }
            cleaned
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
    pub effective_language: String,
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

/// Pure core for dictation post-process clean-up called by `process_transcription_output`.
///
/// When `post_process` is enabled (clean-up isn't Off), ensures spoken self-corrections run
/// before the clean-up model and runs a second pass on the final text after clean-up so a
/// clean-up model miss cannot keep retracted words (Plan A Amendment R1 / AC-A9b).
pub(crate) fn apply_post_process_cleanup_core<F>(
    transcription: &str,
    post_process: bool,
    run_cleanup_model: F,
) -> (String, Option<String>)
where
    F: FnOnce(&str) -> Option<String>,
{
    apply_post_process_cleanup_core_with_policy(
        transcription,
        post_process,
        false,
        run_cleanup_model,
    )
}

pub(crate) fn apply_post_process_cleanup_core_with_policy<F>(
    transcription: &str,
    post_process: bool,
    keep_every_word: bool,
    run_cleanup_model: F,
) -> (String, Option<String>)
where
    F: FnOnce(&str) -> Option<String>,
{
    if !post_process {
        return (transcription.to_string(), None);
    }
    let pre = crate::audio_toolkit::text::rewrite_spoken_self_corrections(transcription);
    let pre_has_cue = crate::audio_toolkit::text::has_repair_cue_hint(&pre);
    // ponytail: ceiling = a cue the rules leave is never resolved by the model; widen the rules, not the model, to take more back.
    // ponytail: ceiling = when any cue is left in `pre`, a model edit that drops or changes any non-filler word falls back to `pre` for the whole paste; upgrade path = per-sentence check.
    // ponytail: ceiling = on the stock on-device prompt (`keep_every_word = true`), a model edit that changes a word (for example a misheard word being fixed) falls back to the rules text, while punctuation, case, and filler removal still go through; any other prompt (`keep_every_word = false`) checks words only when a cue remains; upgrade path = phonetically-bounded word-fix allowlist or per-sentence verifier.
    let kept_model_text = non_blank(run_cleanup_model(&pre)).filter(|processed_text| {
        if !crate::audio_toolkit::text::model_kept_repair_cues(&pre, processed_text) {
            let before = crate::audio_toolkit::text::repair_cue_count(&pre);
            let after = crate::audio_toolkit::text::repair_cue_count(processed_text);
            info!(
                "cleanup: model dropped a correction cue the rules kept; pasting the text as said (cues before={before} after={after})"
            );
            return false;
        }
        if (pre_has_cue || keep_every_word)
            && !crate::audio_toolkit::text::model_kept_words(&pre, processed_text)
        {
            let before = crate::audio_toolkit::text::non_filler_word_count(&pre);
            let after = crate::audio_toolkit::text::non_filler_word_count(processed_text);
            info!(
                "cleanup: model dropped words the rules kept; pasting the text as said (words before={before} after={after})"
            );
            return false;
        }
        true
    });
    if let Some(processed_text) = kept_model_text {
        if pre_has_cue {
            (processed_text.clone(), Some(processed_text))
        } else {
            let guarded =
                crate::audio_toolkit::text::rewrite_spoken_self_corrections(&processed_text);
            (guarded.clone(), Some(guarded))
        }
    } else if pre != transcription {
        (pre.clone(), Some(pre))
    } else {
        (pre, None)
    }
}

pub(crate) async fn process_transcription_output(
    app: &AppHandle,
    transcription: &str,
    post_process: bool,
    correction: bool,
    active_app: Option<&crate::app_profiles::ActiveAppInfo>,
    local_llm_cancel: Option<LocalLlmCancel>,
) -> ProcessedTranscription {
    let raw_settings = get_settings(app);
    let resolved_profile =
        active_app.map(|info| crate::app_profiles::resolve_for_app(&raw_settings, Some(info)));
    let settings = resolved_profile
        .as_ref()
        .map(|r| &r.settings)
        .unwrap_or(&raw_settings);
    let post_process = resolved_profile
        .as_ref()
        .and_then(|r| r.post_process_override)
        .unwrap_or(post_process);
    let mut final_text = transcription.to_string();
    let mut post_processed_text: Option<String> = None;
    let mut post_process_prompt: Option<String> = None;
    let mut replace_char_count: usize = 0;
    let mut status_emitted = false;

    // Resolve the language the transcription actually ran in (the persisted
    // intent coerced against the loaded model's capabilities) so OpenCC keys off
    // the effective language rather than a possibly-stale intent.
    let effective_language = resolve_effective_language(app, settings);
    if let Some(converted_text) =
        maybe_convert_chinese_variant(&effective_language, transcription).await
    {
        final_text = converted_text;
    }

    // Plan C local-only repo symbols pass: runs after custom words and before
    // model clean-up when active app is a code editor and repo_words is on.
    let repo_words_on = resolved_profile
        .as_ref()
        .map(|r| r.repo_words_enabled)
        .unwrap_or(settings.repo_words_enabled);
    if repo_words_on && active_app.is_some_and(crate::repo_words::is_repo_capable_app) {
        if let (Some(url), Some(home)) = (
            crate::repo_words::read_front_window_document_url(),
            std::env::var_os("HOME").map(std::path::PathBuf::from),
        ) {
            if let Some(root) = crate::repo_words::repo_root_from_document(&url, &home) {
                if let Some(cache) = app.try_state::<Mutex<crate::repo_words::RepoSymbolsCache>>() {
                    let mut guard = cache.lock().unwrap_or_else(|e| e.into_inner());
                    final_text = guard.apply_to_take(
                        &final_text,
                        &root,
                        settings.word_correction_threshold,
                        Instant::now(),
                    );
                }
            }
        }
    }

    if correction {
        // CORRECTION MODE (explicit hotkey): unconditionally treat this utterance
        // as an edit of the last output. We never sniff the words — the dedicated
        // "correction" hotkey is the only trigger.
        if let Some(prior_output) = refine_base_for_correction(settings) {
            // The prior output is what the previous dictation pasted (plus the
            // trailing space `paste` appends when that setting is on), so its
            // grapheme length is how many Backspaces the replace must send before
            // typing the edit. Graphemes, not scalars or bytes: one Backspace
            // deletes one grapheme cluster (combining marks, joined emoji).
            let prev_char_count =
                crate::utils::pasted_grapheme_count(&prior_output, settings.append_trailing_space);
            let mut _used_local_stock_prompt = false;
            match post_process_transcription(
                app,
                settings,
                &final_text,
                Some(&prior_output),
                local_llm_cancel,
                &mut status_emitted,
                &mut _used_local_stock_prompt,
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
        let pre_text = crate::audio_toolkit::text::rewrite_spoken_self_corrections(&final_text);
        let mut used_local_stock_prompt = false;
        let model_out = post_process_transcription(
            app,
            settings,
            &pre_text,
            None,
            local_llm_cancel,
            &mut status_emitted,
            &mut used_local_stock_prompt,
        )
        .await;
        let model_ran = non_blank(model_out.clone()).is_some();
        let (next_final, next_processed) = apply_post_process_cleanup_core_with_policy(
            &final_text,
            true,
            used_local_stock_prompt,
            |_| model_out,
        );
        if let Some(processed_text) = next_processed {
            post_processed_text = Some(processed_text);
            final_text = next_final;

            if model_ran {
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

    // Surface style (per-app capitalization & trailing stop) runs BEFORE saved
    // blocks (`AC-P30`), and is skipped when the profile sets CleanupLevel::Off,
    // so saved blocks are always typed verbatim.
    if let Some(resolved) = &resolved_profile {
        if resolved.apply_surface_style && !final_text.is_empty() {
            let styled =
                crate::app_profiles::apply_surface_style(&final_text, resolved.surface_style);
            if styled != final_text {
                final_text = styled;
                post_processed_text = Some(final_text.clone());
            }
        }
    }

    if !settings.saved_blocks.is_empty() {
        let expanded =
            crate::audio_toolkit::expand_saved_blocks(&final_text, &settings.saved_blocks);
        if expanded != final_text {
            final_text = expanded;
            post_processed_text = Some(final_text.clone());
        }
    }

    // Remember this dictation so a later correction-hotkey press can edit it.
    // Stored for BOTH plain and post-processed dictations, and updated after a
    // correction too, so the buffer always holds exactly what the user last
    // received and a subsequent correction deletes the right number of chars.
    store_refine_buffer(settings, transcription, &final_text);

    ProcessedTranscription {
        final_text,
        post_processed_text,
        post_process_prompt,
        replace_char_count,
        status_emitted,
        effective_language,
    }
}

impl ShortcutAction for TranscribeAction {
    fn start(&self, app: &AppHandle, binding_id: &str, _shortcut_str: &str) {
        let start_time = Instant::now();
        debug!("TranscribeAction::start called for binding: {}", binding_id);

        let settings = get_settings(app);
        if crate::app_profiles::check_active_app_start(&settings).is_blocked() {
            debug!("Dictation shortcut ignored in blocked frontmost app");
            return;
        }

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
                apply_recording_mute(&rm_clone, mute_token);
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
                        apply_recording_mute(&rm_clone, mute_token);
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
        let current_settings = get_settings(app);
        let style = current_settings.overlay_style;
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
        let post_process =
            should_post_process_take(self.post_process, current_settings.post_process_enabled);
        let correction = self.correction;
        let cancel_generation = rm.cancel_generation();
        let active_app = crate::app_profiles::read_active_app();

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
                    let policy_snapshot = hm.policy_snapshot();
                    let take_settings = get_settings(&ah);
                    let (_, initial_persist_wav) = dictation_persistence_policy(
                        take_settings.never_store_history,
                        take_settings.keep_dictation_audio,
                    );

                    // Stage the take WAV INSIDE `HistoryManager`'s `policy_gate` (live re-read,
                    // insert name into `pending_audio_removal` before `create_new`, write, verify,
                    // close) concurrently with transcription so an in-flight Never/audio-off flip
                    // or crash never leaves an untracked orphan WAV on disk.
                    let wav_handle = if initial_persist_wav {
                        let samples_for_wav = samples.clone();
                        let hm_for_wav = Arc::clone(&hm);
                        Some(tauri::async_runtime::spawn_blocking(move || {
                            hm_for_wav
                                .stage_take_wav_for_snapshot(policy_snapshot, &samples_for_wav)
                        }))
                    } else {
                        None
                    };

                    // Transcribe concurrently with intent-first WAV staging. If a live stream was
                    // running, finalize it and use its text (all audio was already fed to the
                    // stream); otherwise batch-transcribe the samples.
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
                        Ok(_) => transcribe_batch(&tm, samples, &|| {
                            rm.was_cancelled_since(cancel_generation)
                        }),
                        Err(err) if tm.wait_for_engine(STREAM_ENGINE_RETURN_WAIT) => {
                            warn!("Stream finalize failed ({err}); falling back to batch");
                            transcribe_batch(&tm, samples, &|| {
                                rm.was_cancelled_since(cancel_generation)
                            })
                        }
                        Err(err) => Err(err),
                    };

                    let staged_file_name = match wav_handle {
                        Some(handle) => match handle.await {
                            Ok(Ok(Some(name))) => name,
                            Ok(Ok(None)) => String::new(),
                            Ok(Err(err)) => {
                                error!("Failed to stage take WAV inside policy gate: {}", err);
                                String::new()
                            }
                            Err(err) => {
                                error!("WAV stage task panicked: {}", err);
                                String::new()
                            }
                        },
                        None => String::new(),
                    };

                    if rm.was_cancelled_since(cancel_generation) {
                        if !staged_file_name.is_empty() {
                            let _ = hm.discard_staged_wav(&staged_file_name);
                        }
                        debug!("Transcription operation cancelled before output handling");
                        utils::hide_recording_overlay(&ah);
                        change_tray_icon(&ah, TrayIconState::Idle);
                        return;
                    }

                    match transcription_result {
                        Ok(transcription) => {
                            debug!(
                                "Transcription completed in {:?}: {} chars",
                                transcription_time.elapsed(),
                                transcription.chars().count()
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
                                    active_app.as_ref(),
                                    Some(local_llm_cancel),
                                ),
                                || rm.was_cancelled_since(cancel_generation),
                            )
                            .await
                            else {
                                if !staged_file_name.is_empty() {
                                    let _ = hm.discard_staged_wav(&staged_file_name);
                                }
                                debug!("Transcription operation cancelled during output handling");
                                utils::hide_recording_overlay(&ah);
                                change_tray_icon(&ah, TrayIconState::Idle);
                                return;
                            };

                            if rm.was_cancelled_since(cancel_generation) {
                                if !staged_file_name.is_empty() {
                                    let _ = hm.discard_staged_wav(&staged_file_name);
                                }
                                debug!("Transcription operation cancelled before paste");
                                utils::hide_recording_overlay(&ah);
                                change_tray_icon(&ah, TrayIconState::Idle);
                                return;
                            }

                            // Plan E: if a local agent question card is active on the pill,
                            // route the spoken answer back over the local socket instead of
                            // pasting or saving to Dictation history (I7).
                            if let Some(bridge_state) =
                                ah.try_state::<Arc<Mutex<crate::agent_ask_bridge::AgentAskBridgeState>>>()
                            {
                                let answered = bridge_state
                                    .lock()
                                    .unwrap_or_else(|e| e.into_inner())
                                    .answer_active(processed.final_text.clone());
                                if let Some((agent_name, next_ev)) = answered {
                                    if !staged_file_name.is_empty() {
                                        let _ = hm.discard_staged_wav(&staged_file_name);
                                    }
                                    if let Some(ov) = ah.get_webview_window("recording_overlay") {
                                        let _ = ov.emit(
                                            "agent-ask-answered",
                                            crate::agent_ask_bridge::AgentAskAnsweredEvent {
                                                agent_name,
                                            },
                                        );
                                        if let Some(ev) = next_ev {
                                            let _ = ov.emit("agent-ask-question", ev);
                                        }
                                    }
                                    change_tray_icon(&ah, TrayIconState::Idle);
                                    return;
                                }
                            }

                            if !processed.final_text.trim().is_empty() {
                                if let Some(last_take) =
                                    ah.try_state::<crate::last_take::LastTake>()
                                {
                                    last_take.set(processed.final_text.clone());
                                }
                            }

                            // Persist under `HistoryManager`'s serialized `HistoryPolicyGate`:
                            // row INSERT and `pending_audio_removal` removal execute in ONE SQLite
                            // transaction, or the staged WAV is unlinked inside the gate on Never
                            // / audio-off.
                            let hm_for_save = Arc::clone(&hm);
                            let save_post_processed = processed.post_processed_text.clone();
                            let save_prompt = processed.post_process_prompt.clone();
                            let save_app_name = active_app.as_ref().map(|a| a.app_name.clone());
                            let save_language = Some(processed.effective_language.clone());
                            let save_res = tauri::async_runtime::spawn_blocking(move || {
                                hm_for_save.save_entry_with_meta_for_snapshot(
                                    policy_snapshot,
                                    staged_file_name,
                                    transcription,
                                    post_process,
                                    save_post_processed,
                                    save_prompt,
                                    save_app_name,
                                    save_language,
                                )
                            })
                            .await;
                            let history_saved = match save_res {
                                Ok(Ok(crate::managers::history::NormalSaveStatus::Saved(entry))) => {
                                    debug!("History saved entry {}", entry.id);
                                    true
                                }
                                Ok(Ok(
                                    crate::managers::history::NormalSaveStatus::SavedWithIncompleteAudio {
                                        entry,
                                        incomplete_error,
                                    },
                                )) => {
                                    // Text row is persisted, so it counts as in history for the
                                    // paste notice; only audio verification was incomplete.
                                    debug!(
                                        "History saved text-only entry {} ({})",
                                        entry.id, incomplete_error
                                    );
                                    true
                                }
                                Ok(Err(err)) => {
                                    debug!("History entry not saved: {}", err);
                                    false
                                }
                                Err(err) => {
                                    error!("History save task panicked: {}", err);
                                    false
                                }
                            };
                            let notice_settings = get_settings(&ah);
                            let clipboard_handling = notice_settings.clipboard_handling;
                            let history_limit = notice_settings.history_limit;
                            let retention = notice_settings.recording_retention_period;
                            let never_store = notice_settings.never_store_history;

                            // A status row stays up for a moment after the paste so it can
                            // be read.
                            let delay_hide = should_hold_overlay(processed.status_emitted, style);
                            let captured_overlay_gen = crate::overlay::capture_overlay_generation();

                            if processed.final_text.is_empty() {
                                finish_overlay(&ah, delay_hide, captured_overlay_gen);
                            } else {
                                let ah_clone = ah.clone();
                                let paste_time = Instant::now();
                                let mut final_text = processed.final_text;
                                // Non-zero only in correction mode (or a local V-D10 spoken box
                                // edit): delete the target span before typing the replacement.
                                let mut replace_char_count = processed.replace_char_count;
                                if !correction && notice_settings.read_focused_text_enabled {
                                    if let Some(snapshot) =
                                        crate::focused_text::read_focused_box_snapshot(
                                            &notice_settings,
                                        )
                                    {
                                        let (formatted, extra_replace) =
                                            crate::focused_text::apply_focused_snapshot_to_take(
                                                &snapshot,
                                                &final_text,
                                                notice_settings.read_focused_window_context_enabled,
                                                &notice_settings.custom_words,
                                            );
                                        final_text = formatted;
                                        if extra_replace > 0 {
                                            replace_char_count = extra_replace;
                                        }
                                    }
                                }
                                let rm_for_paste = Arc::clone(&rm);
                                let paste_method = notice_settings.paste_method;
                                #[cfg(target_os = "macos")]
                                let pasted_for_fix_offer = final_text.clone();
                                // Sleeps in paste must not run on the UI thread.
                                // Enigo still hops there itself. spawn_blocking
                                // so those sleeps do not park this async worker.
                                let pasted = tauri::async_runtime::spawn_blocking(move || {
                                    if rm_for_paste.was_cancelled_since(cancel_generation) {
                                        return None;
                                    }
                                    #[cfg(debug_assertions)]
                                    if take_fail_next_paste() {
                                        use tauri_plugin_clipboard_manager::ClipboardExt;
                                        let kept =
                                            ah_clone.clipboard().write_text(&final_text).is_ok();
                                        return Some(Err(crate::clipboard::PasteFailure {
                                            error: "debug: forced paste failure".to_string(),
                                            kept_on_clipboard: kept,
                                        }));
                                    }
                                    Some(utils::replace_previous(
                                        replace_char_count,
                                        final_text,
                                        ah_clone,
                                    ))
                                })
                                .await;

                                let mut cancelled_before_paste = false;
                                let mut show_notice = false;
                                let mut paste_failed = false;
                                let mut paste_lost = false;
                                let outcome_input: Result<
                                    Option<Result<bool, crate::clipboard::PasteFailure>>,
                                    (),
                                > = match pasted {
                                    Ok(Some(Ok(restored))) => {
                                        debug!(
                                            "Text pasted successfully in {:?}",
                                            paste_time.elapsed()
                                        );
                                        // Restored means DontModify put the old clipboard
                                        // back, so we cannot tell whether the paste landed.
                                        show_notice = restored;
                                        Ok(Some(Ok(restored)))
                                    }
                                    Ok(Some(Err(e))) => {
                                        error!("Failed to paste transcription: {}", e);
                                        show_notice = true;
                                        paste_failed = true;
                                        paste_lost = !e.kept_on_clipboard;
                                        Ok(Some(Err(e)))
                                    }
                                    Ok(None) => {
                                        debug!("Transcription operation cancelled before paste");
                                        cancelled_before_paste = true;
                                        Ok(None)
                                    }
                                    Err(e) => {
                                        // JoinError: the blocking paste task panicked or was
                                        // cancelled, not a main-thread dispatch failure.
                                        error!("Paste task panicked or was cancelled: {e:?}");
                                        // The keep-on-clipboard step never ran, so don't
                                        // claim the clipboard holds the words.
                                        show_notice = true;
                                        paste_lost = true;
                                        Err(())
                                    }
                                };
                                if show_notice {
                                    let (on_clipboard, in_history) =
                                        crate::clipboard::paste_word_locations_with_privacy(
                                            clipboard_handling,
                                            paste_failed,
                                            history_saved,
                                            history_limit,
                                            retention,
                                            never_store,
                                        );
                                    let _ = ah.emit(
                                        "paste-notice",
                                        crate::clipboard::paste_notice(
                                            on_clipboard && !paste_lost,
                                            in_history,
                                        ),
                                    );
                                }
                                if cancelled_before_paste {
                                    utils::hide_recording_overlay(&ah);
                                    change_tray_icon(&ah, TrayIconState::Idle);
                                } else {
                                    let maybe_outcome = pill_outcome(
                                        &outcome_input,
                                        paste_method,
                                        active_app.as_ref(),
                                    );
                                    let hide_delay = if let Some(ref outcome) = maybe_outcome {
                                        emit_pill_outcome(&ah, outcome);
                                        Some(if delay_hide {
                                            outcome.hide_delay().max(STATUS_OVERLAY_GRACE)
                                        } else {
                                            outcome.hide_delay()
                                        })
                                    } else {
                                        delay_hide.then_some(STATUS_OVERLAY_GRACE)
                                    };
                                    finish_overlay_with_delay(
                                        &ah,
                                        hide_delay,
                                        captured_overlay_gen,
                                    );
                                    // Plan C: post-paste fix_offer watcher (macOS, local AX only).
                                    #[cfg(target_os = "macos")]
                                    if !paste_failed
                                        && !paste_lost
                                        && notice_settings.offer_remember_fix_enabled
                                        && !notice_settings.never_store_history
                                        && !pasted_for_fix_offer.trim().is_empty()
                                    {
                                        spawn_fix_offer_watcher(
                                            ah.clone(),
                                            pasted_for_fix_offer,
                                            active_app
                                                .as_ref()
                                                .map(|a| a.app_name.clone())
                                                .unwrap_or_default(),
                                        );
                                    }
                                }
                            }
                        }
                        Err(err) => {
                            if rm.was_cancelled_since(cancel_generation) {
                                if !staged_file_name.is_empty() {
                                    let _ = hm.discard_staged_wav(&staged_file_name);
                                }
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
                            // Save retry entry with empty text ONLY when serialized live policy
                            // under `policy_gate` still allows BOTH history and audio storage;
                            // otherwise discard the staged WAV inside `policy_gate`.
                            if !staged_file_name.is_empty() {
                                let hm_for_save = Arc::clone(&hm);
                                let save_res = tauri::async_runtime::spawn_blocking(move || {
                                    hm_for_save.save_failed_retry_entry_for_snapshot(
                                        policy_snapshot,
                                        staged_file_name,
                                        post_process,
                                    )
                                })
                                .await;
                                match save_res {
                                    Ok(Ok(_)) => {}
                                    Ok(Err(save_err)) => {
                                        debug!("Failed history entry not saved: {}", save_err);
                                    }
                                    Err(join_err) => {
                                        error!("Failed history save task panicked: {}", join_err);
                                    }
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

/// Determines whether a dictation take should persist its transcript text and/or
/// WAV audio file to disk.
///
/// - `never_store_history == true` ("Never"): write neither text nor audio to disk.
/// - `never_store_history == false && keep_dictation_audio == false` (default):
///   write text-only history and never write a WAV file.
/// - `never_store_history == false && keep_dictation_audio == true`: write both.
pub fn dictation_persistence_policy(
    never_store_history: bool,
    keep_dictation_audio: bool,
) -> (bool, bool) {
    if never_store_history {
        (false, false)
    } else {
        (true, keep_dictation_audio)
    }
}

/// Remove a freshly written or partial WAV through the single bounded `unlinkat`
/// delete in `HistoryManager`. Never follows symlinks or deletes outside its folder.
#[cfg(test)]
fn remove_orphan_wav(path: &std::path::Path) {
    let Some(parent) = path.parent() else {
        return;
    };
    let Some(file_name) = path.file_name().and_then(|s| s.to_str()) else {
        return;
    };
    let Ok(canonical_root) = std::fs::canonicalize(parent) else {
        return;
    };
    if let Err(err) = crate::managers::history::HistoryManager::unlink_recording_strict_bounded(
        parent,
        Some(&canonical_root),
        file_name,
    ) {
        error!("Failed to remove orphan WAV file {}: {}", file_name, err);
    }
}

/// Checks `allow_wav()` immediately before writing, writes and verifies the WAV,
/// and removes any newly created partial file on write or verification failure.
/// Never removes a pre-existing file that this call did not create.
#[cfg(test)]
fn write_wav_if_allowed<A, W, V, WE, VE>(
    wav_path: &std::path::Path,
    allow_wav: A,
    write_fn: W,
    verify_fn: V,
) -> bool
where
    A: FnOnce() -> bool,
    W: FnOnce(&std::path::Path) -> std::result::Result<(), WE>,
    V: FnOnce(&std::path::Path) -> std::result::Result<(), VE>,
    WE: std::fmt::Display,
    VE: std::fmt::Display,
{
    if !allow_wav() {
        return false;
    }
    let existed_before = wav_path.symlink_metadata().is_ok();
    match write_fn(wav_path) {
        Ok(()) => match verify_fn(wav_path) {
            Ok(()) => true,
            Err(err) => {
                error!("WAV verification failed: {}", err);
                if !existed_before {
                    remove_orphan_wav(wav_path);
                }
                false
            }
        },
        Err(err) => {
            error!("Failed to save WAV file: {}", err);
            if !existed_before {
                remove_orphan_wav(wav_path);
            }
            false
        }
    }
}

/// Re-evaluates persistence policy RIGHT BEFORE inserting a DB row so an in-flight
/// switch to Never store or audio-off cannot persist a row or leave a WAV on disk.
/// Returns `(persist_text_now, stored_file_name)`.
#[cfg(test)]
fn resolve_in_flight_persistence(
    never_store_now: bool,
    keep_audio_now: bool,
    wav_saved: bool,
    file_name: String,
    wav_path: Option<&std::path::Path>,
) -> (bool, String) {
    let (allow_text_now, allow_wav_now) =
        dictation_persistence_policy(never_store_now, keep_audio_now);
    if !allow_text_now {
        if let Some(path) = wav_path {
            remove_orphan_wav(path);
        }
        return (false, String::new());
    }
    if allow_wav_now && wav_saved && !file_name.is_empty() {
        (true, file_name)
    } else {
        if let Some(path) = wav_path {
            remove_orphan_wav(path);
        }
        (true, String::new())
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
        "hands_free".to_string(),
        Arc::new(TranscribeAction {
            post_process: false,
            correction: false,
        }) as Arc<dyn ShortcutAction>,
    );
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
    #[cfg(feature = "meetings")]
    map.insert(
        "meeting".to_string(),
        Arc::new(MeetingConsentAction) as Arc<dyn ShortcutAction>,
    );
    map
});

#[cfg(test)]
mod tests {
    #[cfg(target_os = "macos")]
    use super::local_llm_error_status;
    use super::{
        build_post_process_messages, complete_unless_cancelled, next_cleanup_preview, non_blank,
        note_cancel_if_generation_moved, provider_configured, refine_base_for_correction,
        should_hold_overlay, should_post_process_take, should_use_streaming_overlay,
        store_refine_buffer, streams_cleanup_progress, SkipReason, CLEANUP_PROGRESS_MIN_GAP_MS,
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
    fn cleanup_preview_emits_on_word_boundaries_at_most_15_hz() {
        let gap = CLEANUP_PROGRESS_MIN_GAP_MS;
        assert!(gap * 15 >= 1000, "gap must keep the rate at or under 15 Hz");
        assert!(gap <= 67, "15 Hz cadence is a gap of at most 67 ms");

        assert_eq!(next_cleanup_preview("Hel", "", None, gap), None);
        assert_eq!(next_cleanup_preview("Hello", "", None, gap), None);
        assert_eq!(next_cleanup_preview("   ", "", None, gap), None);
        // First completed word is not delayed. The partial next word stays hidden.
        assert_eq!(
            next_cleanup_preview("Hello ", "", None, gap),
            Some("Hello ")
        );
        assert_eq!(
            next_cleanup_preview("Hello wor", "", None, gap),
            Some("Hello ")
        );
        assert_eq!(
            next_cleanup_preview("Hello world", "", None, gap),
            Some("Hello ")
        );
        // Already shown, including while the next word is still partial.
        assert_eq!(
            next_cleanup_preview("Hello wor", "Hello ", Some(500), gap),
            None
        );
        // Next word is ready, but inside the gap.
        assert_eq!(
            next_cleanup_preview("Hello world ", "Hello ", Some(gap - 1), gap),
            None
        );
        assert_eq!(
            next_cleanup_preview("Hello world ", "Hello ", Some(0), gap),
            None
        );
        assert_eq!(
            next_cleanup_preview("Hello world ", "Hello ", Some(gap), gap),
            Some("Hello world ")
        );
        // NBSP is a two-byte whitespace char. The prefix must include all of it.
        assert_eq!(
            next_cleanup_preview("Hello\u{00A0}w", "", None, gap),
            Some("Hello\u{00A0}")
        );
    }

    #[test]
    fn only_the_on_device_provider_streams_cleanup_progress() {
        assert!(streams_cleanup_progress(
            crate::settings::LOCAL_LLM_PROVIDER_ID
        ));
        for id in [
            "openai",
            "openrouter",
            "custom",
            crate::settings::APPLE_INTELLIGENCE_PROVIDER_ID,
            "",
        ] {
            assert!(!streams_cleanup_progress(id), "{id}");
        }
    }

    #[test]
    fn cleanup_progress_event_is_text_and_done() {
        let value = serde_json::to_value(super::CleanupProgress {
            text: "Hello ".into(),
            done: true,
        })
        .unwrap();
        assert_eq!(value, serde_json::json!({"text": "Hello ", "done": true}));
        assert_eq!(
            <super::CleanupProgress as tauri_specta::Event>::NAME,
            "cleanup-progress"
        );
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

    #[test]
    fn clean_every_take_runs_post_processing_on_standard_dictation_when_enabled() {
        assert!(should_post_process_take(false, true));
        assert!(!should_post_process_take(false, false));
        assert!(should_post_process_take(true, false));
        assert!(should_post_process_take(true, true));
    }

    #[test]
    fn pipeline_runs_custom_words_then_corrections_then_surface_style_then_saved_blocks() {
        let custom_words = vec!["signoff".to_string()];
        let corrections = vec![crate::settings::CorrectionPair {
            from: "signoff".to_string(),
            to: "team note".to_string(),
        }];
        let blocks = vec![crate::settings::SavedBlock {
            id: "sig-1".to_string(),
            cue: "team note".to_string(),
            content: "Best regards,\nCoco Voice.".to_string(),
        }];

        let step1 = crate::audio_toolkit::apply_custom_words("insert signof", &custom_words, 0.5);
        assert_eq!(step1, "insert signoff");
        let step2 = crate::audio_toolkit::apply_corrections(&step1, &corrections);
        assert_eq!(step2, "insert team note");
        // AC-P30: Casual surface style (lowercase + drop trailing period) runs BEFORE
        // expand_saved_blocks, so the expanded block keeps its exact casing & period.
        let casual = crate::app_profiles::tone_defaults(crate::app_profiles::AppTone::Casual, None);
        let step3 = crate::app_profiles::apply_surface_style(&step2, casual);
        let step4 = crate::audio_toolkit::expand_saved_blocks(&step3, &blocks);
        assert_eq!(step4, "Best regards,\nCoco Voice.");
    }

    #[test]
    fn dictation_persistence_policy_enforces_never_store_and_keep_audio_off_default() {
        use super::{dictation_persistence_policy, remove_orphan_wav};
        // Never store writes neither text nor WAV even if keep_dictation_audio was true.
        assert_eq!(dictation_persistence_policy(true, false), (false, false));
        assert_eq!(dictation_persistence_policy(true, true), (false, false));
        // Default (never_store=false, keep_audio=false) writes text only, zero WAV.
        assert_eq!(dictation_persistence_policy(false, false), (true, false));
        // Opt-in audio writes both text and WAV.
        assert_eq!(dictation_persistence_policy(false, true), (true, true));

        // Orphan WAV cleanup removes a cancelled take's file from disk.
        let dir = tempfile::tempdir().expect("temp dir");
        let orphan = dir.path().join("cancelled-take.wav");
        std::fs::write(&orphan, b"RIFF").unwrap();
        assert!(orphan.exists());
        remove_orphan_wav(&orphan);
        assert!(!orphan.exists());
    }

    #[test]
    fn partial_wav_write_and_verify_failure_leave_zero_orphan_files_on_disk() {
        use super::write_wav_if_allowed;

        let dir = tempfile::tempdir().expect("temp dir");
        let wav_write_err = dir.path().join("partial-write.wav");
        let ok_write_err = write_wav_if_allowed(
            &wav_write_err,
            || true,
            |p| {
                std::fs::write(p, b"RIFF-partial").unwrap();
                Err("disk full")
            },
            |_| Ok::<(), &str>(()),
        );
        assert!(!ok_write_err);
        assert!(
            !wav_write_err.exists(),
            "partial WAV from failed write must be removed"
        );

        let wav_verify_err = dir.path().join("partial-verify.wav");
        let ok_verify_err = write_wav_if_allowed(
            &wav_verify_err,
            || true,
            |p| {
                std::fs::write(p, b"RIFF-corrupt").unwrap();
                Ok::<(), &str>(())
            },
            |_| Err("sample count mismatch"),
        );
        assert!(!ok_verify_err);
        assert!(
            !wav_verify_err.exists(),
            "corrupt WAV from failed verify must be removed"
        );

        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[test]
    fn in_flight_switch_to_never_or_audio_off_writes_zero_wav_and_zero_db_rows() {
        use super::{resolve_in_flight_persistence, write_wav_if_allowed};
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;

        let dir = tempfile::tempdir().expect("temp dir");

        // 1) User flips Never store ON before the WAV writer runs -> 0 WAV written.
        let never_store = Arc::new(AtomicBool::new(true));
        let keep_audio = Arc::new(AtomicBool::new(true));
        let wav_a = dir.path().join("take-a.wav");
        let saved_a = write_wav_if_allowed(
            &wav_a,
            || {
                super::dictation_persistence_policy(
                    never_store.load(Ordering::SeqCst),
                    keep_audio.load(Ordering::SeqCst),
                )
                .1
            },
            |p| {
                std::fs::write(p, b"RIFF").unwrap();
                Ok::<(), &str>(())
            },
            |_| Ok::<(), &str>(()),
        );
        assert!(!saved_a);
        assert!(!wav_a.exists());

        // 2) WAV wrote while keep_audio=true, then user switched to Never before DB insert ->
        //    WAV is deleted and persist_text_now is false (0 DB row, 0 WAV).
        let wav_b = dir.path().join("take-b.wav");
        std::fs::write(&wav_b, b"RIFF-b").unwrap();
        let (persist_text_b, stored_b) =
            resolve_in_flight_persistence(true, true, true, "take-b.wav".into(), Some(&wav_b));
        assert!(!persist_text_b);
        assert!(stored_b.is_empty());
        assert!(!wav_b.exists(), "in-flight WAV must be deleted on Never");

        // 3) WAV wrote while keep_audio=true, then user switched audio OFF before DB insert ->
        //    WAV is deleted and DB row is saved text-only (file_name = "").
        let wav_c = dir.path().join("take-c.wav");
        std::fs::write(&wav_c, b"RIFF-c").unwrap();
        let (persist_text_c, stored_c) =
            resolve_in_flight_persistence(false, false, true, "take-c.wav".into(), Some(&wav_c));
        assert!(persist_text_c);
        assert!(stored_c.is_empty());
        assert!(
            !wav_c.exists(),
            "in-flight WAV must be deleted when audio switched off"
        );

        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[test]
    fn pill_outcome_covers_all_seven_cases_and_truncates_long_app_names() {
        use super::{pill_outcome, truncate_app_name, PillOutcome};
        use crate::app_profiles::ActiveAppInfo;
        use crate::clipboard::PasteFailure;
        use crate::settings::PasteMethod;

        let mail = ActiveAppInfo {
            app_name: "Mail".to_string(),
            bundle_id: Some("com.apple.mail".to_string()),
            process_stem: None,
        };
        let long_app = ActiveAppInfo {
            app_name: "VeryLongApplicationNameExceeding24Chars".to_string(),
            bundle_id: None,
            process_stem: None,
        };

        // 1. Real paste ok, app "Mail" -> Sent("Mail")
        assert_eq!(
            pill_outcome(&Ok(Some(Ok(false))), PasteMethod::CtrlV, Some(&mail)),
            Some(PillOutcome::Sent {
                target_app: Some("Mail".to_string())
            })
        );
        // 2. Real paste ok, no app -> Sent(None)
        assert_eq!(
            pill_outcome(&Ok(Some(Ok(true))), PasteMethod::Direct, None),
            Some(PillOutcome::Sent { target_app: None })
        );
        // 3. Long app name truncated to 24 chars + ellipsis
        assert_eq!(
            truncate_app_name(&long_app.app_name),
            Some("VeryLongApplicationNameE…".to_string())
        );
        // 4. Ok(false) with PasteMethod::None -> Clipboard
        assert_eq!(
            pill_outcome(&Ok(Some(Ok(false))), PasteMethod::None, Some(&mail)),
            Some(PillOutcome::Clipboard)
        );
        // 5. Failure, kept on clipboard -> KeptPressPaste
        assert_eq!(
            pill_outcome(
                &Ok(Some(Err(PasteFailure {
                    error: "blocked".to_string(),
                    kept_on_clipboard: true,
                }))),
                PasteMethod::CtrlV,
                Some(&mail),
            ),
            Some(PillOutcome::KeptPressPaste)
        );
        // 6. Failure, not kept -> None (toast still fires)
        assert_eq!(
            pill_outcome(
                &Ok(Some(Err(PasteFailure {
                    error: "blocked".to_string(),
                    kept_on_clipboard: false,
                }))),
                PasteMethod::CtrlV,
                Some(&mail),
            ),
            None
        );
        // 7. Cancelled -> None
        assert_eq!(
            pill_outcome(&Ok(None), PasteMethod::CtrlV, Some(&mail)),
            None
        );
        // 8. Panicked -> None
        assert_eq!(
            pill_outcome(&Err(()), PasteMethod::CtrlV, Some(&mail)),
            None
        );
    }

    #[test]
    fn pill_outcome_delays_and_accessibility_announcer_match_spec() {
        use super::{announce_with, PillOutcome};
        use std::time::Duration;

        assert_eq!(
            PillOutcome::Sent {
                target_app: Some("Mail".to_string())
            }
            .hide_delay(),
            Duration::from_millis(1500)
        );
        assert_eq!(
            PillOutcome::Clipboard.hide_delay(),
            Duration::from_millis(2500)
        );
        assert_eq!(
            PillOutcome::KeptPressPaste.hide_delay(),
            Duration::from_millis(4000)
        );

        let mut spoken = Vec::<String>::new();
        announce_with("Listening", |t| spoken.push(t.to_string()));
        announce_with(
            &PillOutcome::Sent {
                target_app: Some("Mail".to_string()),
            }
            .announcement_text(true),
            |t| spoken.push(t.to_string()),
        );
        announce_with(
            &PillOutcome::Sent { target_app: None }.announcement_text(true),
            |t| spoken.push(t.to_string()),
        );
        assert_eq!(spoken, vec!["Listening", "Sent to Mail", "Sent"]);
    }

    #[test]
    fn fail_next_paste_is_strictly_gated_to_debug_assertions() {
        let src = include_str!("actions.rs");
        // Slice only non-test code so this test's own strings don't match.
        let prod_src = src.split("#[cfg(test)]").next().unwrap_or(src);
        for symbol in [
            "static FAIL_NEXT_PASTE",
            "pub fn debug_set_fail_next_paste",
            "fn take_fail_next_paste",
            "if take_fail_next_paste()",
        ] {
            let idx = prod_src
                .find(symbol)
                .unwrap_or_else(|| panic!("expected {symbol} in actions.rs"));
            let before = &prod_src[idx.saturating_sub(120)..idx];
            assert!(
                before.contains("#[cfg(debug_assertions)]"),
                "{symbol} must be strictly guarded by #[cfg(debug_assertions)]"
            );
        }
    }

    #[test]
    fn ac_a9b_spoken_retraction_real_path_and_guards() {
        use super::apply_post_process_cleanup_core;
        use crate::audio_toolkit::text::filter_transcription_output_with_policy;

        let forms = [
            "meeting on tuesday, scratch that, wednesday",
            "Meeting on Tuesday. Scratch that, Wednesday.",
            "Meeting on Tuesday. Scratch that. Wednesday.",
            "Meeting on Tuesday, scratch that. Wednesday.",
        ];

        // Real pre-cleanup pass (`managers/transcription.rs:post_process_transcription_text`)
        // followed by `apply_post_process_cleanup_core` (called by `process_transcription_output`).
        let mut failures: Vec<String> = Vec::new();
        for form in forms {
            let pre = filter_transcription_output_with_policy(form, "en", &None, true, true);

            // (a) Clean-up provider unavailable -> model stubbed to return its input unchanged.
            let (out_echo, _) =
                apply_post_process_cleanup_core(&pre, true, |s| Some(s.to_string()));
            // (b) Clean-up model returns the raw un-retracted text (simulates a model miss).
            let (out_model_miss, _) =
                apply_post_process_cleanup_core(&pre, true, |_| Some(form.to_string()));
            // (c) Clean-up model fails / provider unavailable -> raw fallback (`None`).
            let (out_fallback, _) = apply_post_process_cleanup_core(&pre, true, |_| None);

            for (mode, got) in [
                ("echo", &out_echo),
                ("model_miss", &out_model_miss),
                ("fallback", &out_fallback),
            ] {
                let lc = got.to_ascii_lowercase();
                let ok = lc.contains("wednesday")
                    && !lc.contains("tuesday")
                    && !lc.contains("scratch that");
                if !ok {
                    failures.push(format!("form={form:?} mode={mode} got={got:?}"));
                }
            }
        }
        assert!(
            failures.is_empty(),
            "AC-A9b failures ({}):\n{}",
            failures.len(),
            failures.join("\n")
        );

        // Guard cases must stay unchanged across both echo and fallback paths.
        let guards = [
            "I mean it.",
            "We should go. I mean it.",
            "Scratch that off the list.",
            "Don't forget. Scratch that off the list.",
            "Good news. No wait times today.",
        ];
        for guard in guards {
            let pre = filter_transcription_output_with_policy(guard, "en", &None, true, true);
            let (out_echo, _) =
                apply_post_process_cleanup_core(&pre, true, |s| Some(s.to_string()));
            let (out_fallback, _) = apply_post_process_cleanup_core(&pre, true, |_| None);
            assert_eq!(out_echo, guard, "guard altered in echo mode: {guard:?}");
            assert_eq!(
                out_fallback, guard,
                "guard altered in fallback mode: {guard:?}"
            );
        }

        // Cleanup Off leaves spoken retractions untouched.
        let (off_out, off_pp) = apply_post_process_cleanup_core(
            "Meeting on Tuesday. Scratch that, Wednesday.",
            false,
            |_| unreachable!("model must not run when cleanup is Off"),
        );
        assert_eq!(off_out, "Meeting on Tuesday. Scratch that, Wednesday.");
        assert!(off_pp.is_none());
    }

    #[test]
    fn ac_a9b_r1b_spoken_retraction_narrowed_bridge_and_must_not_change() {
        use super::apply_post_process_cleanup_core;
        use crate::audio_toolkit::text::filter_transcription_output_with_policy;

        // Must take back (6 from R1b + 12 from R1c = 18 total under R1d):
        // each final text has the new word and neither the old word nor the cue.
        let r1b_take_back: &[(&str, &str, &str, &str)] = &[
            (
                "meeting on tuesday, scratch that, wednesday",
                "wednesday",
                "tuesday",
                "scratch that",
            ),
            (
                "Meeting on Tuesday. Scratch that, Wednesday.",
                "wednesday",
                "tuesday",
                "scratch that",
            ),
            (
                "Meeting on Tuesday. Scratch that. Wednesday.",
                "wednesday",
                "tuesday",
                "scratch that",
            ),
            (
                "Meeting on Tuesday, scratch that. Wednesday.",
                "wednesday",
                "tuesday",
                "scratch that",
            ),
            (
                "Meeting on Tuesday. I mean Wednesday.",
                "wednesday",
                "tuesday",
                "i mean",
            ),
            (
                "Send it to Priya. Sorry, I mean Jordan.",
                "jordan",
                "priya",
                "i mean",
            ),
        ];

        let r1c_take_back: &[(&str, &str, &str, &str)] = &[
            ("Lunch is at one. Scratch that, two.", "two", "one", "scratch that"),
            ("We met in Paris. I mean Rome.", "rome", "paris", "i mean"),
            ("Ship it on Friday. No wait, Monday.", "monday", "friday", "no wait"),
            ("We'll start at nine. Correction, ten.", "ten", "nine", "correction"),
            ("Book it for Friday. I mean Saturday.", "saturday", "friday", "i mean"),
            ("The report is for Q3. Scratch that, Q4.", "q4", "q3", "scratch that"),
            ("Lunch on Thursday. Scratch that, Friday.", "friday", "thursday", "scratch that"),
            ("The flight is in March. No wait, April.", "april", "march", "no wait"),
            ("Call Mom. No wait, Dad.", "dad", "mom", "no wait"),
            ("Send it to Alex. Scratch that, Jamie.", "jamie", "alex", "scratch that"),
            (
                "meeting on tuesday. scratch that, wednesday.",
                "wednesday",
                "tuesday",
                "scratch that",
            ),
            (
                "Pick it up at four. Sorry, I mean five.",
                "five",
                "four",
                "i mean",
            ),
        ];

        // R1d known misses (tested neither way; moved out of must_take_back under Amendment R1d):
        let _r1d_known_misses: &[&str] = &[
            "Book the room. No wait, the hall.",
            "It goes in the oven. No wait, the fridge.",
            "This is for the team. Scratch that, for everyone.",
            "The deadline is the 3rd. Correction, the 5th.",
        ];
        assert_eq!(_r1d_known_misses.len(), 4);

        let must_take_back: Vec<(&str, &str, &str, &str)> =
            [r1b_take_back, r1c_take_back].into_iter().flatten().copied().collect();
        assert_eq!(must_take_back.len(), 18);

        // Exact output (R1f v11: ex01 is pasted byte-identical as said so no word is lost):
        let exact_output_cases: &[(&str, &str)] = &[(
            "The talk was at noon. Or rather, around noon.",
            "The talk was at noon. Or rather, around noon.",
        )];

        // Must not change (16 from R1b + 44 from R1c + 6 from R1d = 66 total):
        // byte-identical to input in every mode (echo, model_miss, fallback, model_cuts).
        let model_cuts = |s: &str| -> Option<String> {
            const CUT_CUES: &[&str] = &[
                "scratch that",
                "never mind",
                "no wait",
                "or rather",
                "i mean",
                "correction",
                "rephrase",
                "actually",
                "sorry,",
                ", no,",
                "scratch",
                "rather",
                "mean",
            ];
            let lc = s.to_ascii_lowercase();
            let best = CUT_CUES
                .iter()
                .filter_map(|cue| lc.find(cue).map(|pos| (pos, cue.len())))
                .min_by_key(|&(pos, len)| (pos, std::cmp::Reverse(len)));
            let Some((pos, len)) = best else {
                return Some(s.to_string());
            };
            let tail = s[pos + len..]
                .trim_start_matches(|c: char| matches!(c, ',' | ':' | '.' | ';' | ' '));
            if !tail.is_empty() {
                Some(tail.to_string())
            } else {
                let head = s[..pos].trim_end();
                if !head.is_empty() {
                    Some(head.to_string())
                } else {
                    Some(s.to_string())
                }
            }
        };
        let r1b_must_not_change: &[&str] = &[
            "I'll be late. Sorry, traffic is bad.",
            "We shipped it. Actually, the tests were green.",
            "The demo went well. Actually, it went great.",
            "Thanks for the notes. I mean, they really helped.",
            "Send it to Priya. Correction: the deck is final.",
            "Is it ready? Actually, never mind.",
            "Book the room. No wait, I'll do it myself.",
            "Call me later. Sorry, I'm in a meeting.",
            "We shipped it. No wait, not yet.",
            "I mean it.",
            "We should go. I mean it.",
            "Scratch that off the list.",
            "Don't forget. Scratch that off the list.",
            "Good news. No wait times today.",
            "That was fun. Never mind the rain.",
            "Got it. Or rather, we got most of it.",
        ];

        let r1c_must_not_change: &[&str] = &[
            "It was in the box. I mean, obviously.",
            "He works at the bank. I mean, seriously.",
            "I went to the store. I mean, honestly, what else?",
            "Let's go to the park. I mean, why not?",
            "We are on track. I mean, mostly.",
            "Send the file to me. Sorry, my mistake.",
            "It is on the desk. Never mind, found it.",
            "Put it in the drawer. No wait, really?",
            "I was at home. I mean, where else?",
            "That's for the kids. I mean, of course.",
            "Thanks again. I mean, really.",
            "The file is on the server. No wait, hold on.",
            "I'll send it to you. No wait, tomorrow.",
            "Give it to Sam. No wait, okay.",
            "Sounds good to me. No wait, one question.",
            "Put the keys in the bowl. No wait, sorry.",
            "The bus leaves at six. Never mind, it's late.",
            "Dinner is at seven. I mean, roughly.",
            "Ask him about it. No wait, never mind.",
            "It's in the car. Actually, it's in the house.",
            "Talk to you soon. Scratch that, bye.",
            "Send it to the client. No wait, hmm.",
            "I'm at the gym. No wait, okay, fine.",
            "It's for you. Scratch that.",
            "Thanks for coming. No wait, please stay.",
            "We are on it. Correction: done.",
            "Meet me in the lobby. Sorry, I mean it.",
            "It's on GitHub. No wait, OK.",
            "Send it to Priya. Scratch that. Okay.",
            "I live in Boston. Never mind, Thursday works.",
            "See you on Monday. I mean, probably.",
            "We have two options. Actually, three.",
            "Meet at the station. No wait, I'll drive.",
            "That costs ten dollars. I mean, wow.",
            "It's in May. No wait, maybe.",
            "Ask Sarah. Sorry, my bad.",
            "The office is in London. I mean, technically.",
            "Book a table for two. Scratch that, sounds good.",
            "I'll be there at five. No wait, seriously?",
            "We met last Friday. I mean, it was fun.",
            "It was 2019. Or rather, ages ago.",
            "The code is in Rust. No wait, Yes.",
            "Ping me on Slack. Never mind, Zoom is fine.",
            "We sold 40 units. I mean, wow, 40.",
        ];

        let r1d_must_not_change: &[&str] = &[
            "Thanks, Sam. I mean, Thanks.",
            "Text Ana the address. I mean, the new one.",
            "I need to call Sam. I mean, call him later.",
            "Nice work, Ana. I mean, Bravo.",
            "It's due Friday. No wait, Friday is fine.",
            "Meet Leo at five. I mean, Leo.",
        ];

        let must_not_change: Vec<&str> = [
            r1b_must_not_change,
            r1c_must_not_change,
            r1d_must_not_change,
        ]
        .into_iter()
        .flatten()
        .copied()
        .collect();
        assert_eq!(must_not_change.len(), 66);

        let mut failures: Vec<String> = Vec::new();
        for &(input, keep, drop_old, drop_cue) in &must_take_back {
            let pre = filter_transcription_output_with_policy(input, "en", &None, true, true);
            let (out_echo, _) =
                apply_post_process_cleanup_core(&pre, true, |s| Some(s.to_string()));
            let (out_model_miss, _) =
                apply_post_process_cleanup_core(&pre, true, |_| Some(input.to_string()));
            let (out_fallback, _) = apply_post_process_cleanup_core(&pre, true, |_| None);
            let (out_model_cuts, _) = apply_post_process_cleanup_core(&pre, true, model_cuts);

            for (mode, got) in [
                ("echo", &out_echo),
                ("model_miss", &out_model_miss),
                ("fallback", &out_fallback),
                ("model_cuts", &out_model_cuts),
            ] {
                let lc = got.to_ascii_lowercase();
                let ok = lc.contains(keep)
                    && !lc.contains(drop_old)
                    && !lc.contains(drop_cue)
                    && !(input.contains("Sorry, I mean") && lc.contains("sorry"));
                if !ok {
                    failures.push(format!("TAKE_BACK input={input:?} mode={mode} got={got:?}"));
                }
            }
        }

        for &(input, expected) in exact_output_cases {
            let pre = filter_transcription_output_with_policy(input, "en", &None, true, true);
            let (out_echo, _) =
                apply_post_process_cleanup_core(&pre, true, |s| Some(s.to_string()));
            let (out_model_miss, _) =
                apply_post_process_cleanup_core(&pre, true, |_| Some(input.to_string()));
            let (out_fallback, _) = apply_post_process_cleanup_core(&pre, true, |_| None);
            let (out_model_cuts, _) = apply_post_process_cleanup_core(&pre, true, model_cuts);

            for (mode, got) in [
                ("echo", &out_echo),
                ("model_miss", &out_model_miss),
                ("fallback", &out_fallback),
                ("model_cuts", &out_model_cuts),
            ] {
                if got.as_str() != expected {
                    failures.push(format!(
                        "EXACT_OUTPUT input={input:?} mode={mode} expected={expected:?} got={got:?}"
                    ));
                }
            }
        }

        let mut model_cuts_changed_count: usize = 0;
        for &input in &must_not_change {
            // Test all 66 byte-identical through the retraction-only real path (`remove_fillers = false`)
            // across echo / model_miss / fallback / model_cuts, AND also with `remove_fillers = true`
            // for all inputs that do not contain a configured English filler token ("hmm").
            let pre_no_fill = filter_transcription_output_with_policy(input, "en", &None, false, true);
            let (out_echo_nf, _) =
                apply_post_process_cleanup_core(&pre_no_fill, true, |s| Some(s.to_string()));
            let (out_miss_nf, _) =
                apply_post_process_cleanup_core(&pre_no_fill, true, |_| Some(input.to_string()));
            let (out_fb_nf, _) = apply_post_process_cleanup_core(&pre_no_fill, true, |_| None);
            let (out_cuts_nf, _) =
                apply_post_process_cleanup_core(&pre_no_fill, true, model_cuts);
            if out_cuts_nf.as_str() != input {
                model_cuts_changed_count += 1;
            }

            for (mode, got) in [
                ("echo", &out_echo_nf),
                ("model_miss", &out_miss_nf),
                ("fallback", &out_fb_nf),
                ("model_cuts", &out_cuts_nf),
            ] {
                if got.as_str() != input {
                    failures.push(format!(
                        "MUST_NOT_CHANGE input={input:?} mode={mode} got={got:?}"
                    ));
                }
            }

            if !input.to_ascii_lowercase().contains("hmm") {
                let pre = filter_transcription_output_with_policy(input, "en", &None, true, true);
                let (out_echo, _) =
                    apply_post_process_cleanup_core(&pre, true, |s| Some(s.to_string()));
                let (out_model_miss, _) =
                    apply_post_process_cleanup_core(&pre, true, |_| Some(input.to_string()));
                let (out_fallback, _) = apply_post_process_cleanup_core(&pre, true, |_| None);
                let (out_model_cuts, _) =
                    apply_post_process_cleanup_core(&pre, true, model_cuts);

                for (mode, got) in [
                    ("echo+fillers", &out_echo),
                    ("model_miss+fillers", &out_model_miss),
                    ("fallback+fillers", &out_fallback),
                    ("model_cuts+fillers", &out_model_cuts),
                ] {
                    if got.as_str() != input {
                        failures.push(format!(
                            "MUST_NOT_CHANGE input={input:?} mode={mode} got={got:?}"
                        ));
                    }
                }
            }
        }

        // Named regression case: stub drops "No wait," from a sentence the rules kept as said.
        let named_keep_in = "I'll send it to you. No wait, tomorrow.";
        let (named_keep_out, _) = apply_post_process_cleanup_core(named_keep_in, true, |_| {
            Some("I'll send it to you tomorrow.".to_string())
        });
        if named_keep_out != named_keep_in {
            failures.push(format!(
                "NAMED_KEEP input={named_keep_in:?} expected={named_keep_in:?} got={named_keep_out:?}"
            ));
        }

        assert!(
            failures.is_empty(),
            "AC-A9b spoken retraction failures ({}, model_cuts_changed={}/{}):\n{}",
            failures.len(),
            model_cuts_changed_count,
            must_not_change.len(),
            failures.join("\n")
        );
    }

    fn find_first_cut_cue(s: &str) -> Option<(usize, usize)> {
        const CUT_CUES: &[&str] = &[
            "scratch that",
            "never mind",
            "no wait",
            "or rather",
            "i mean",
            "correction",
            "rephrase",
            "actually",
            "sorry,",
            ", no,",
            "scratch",
            "rather",
            "mean",
        ];
        let lc = s.to_ascii_lowercase();
        CUT_CUES
            .iter()
            .filter_map(|cue| lc.find(cue).map(|pos| (pos, cue.len())))
            .min_by_key(|&(pos, len)| (pos, std::cmp::Reverse(len)))
    }

    fn stub_model_cuts_before_cue(s: &str) -> Option<String> {
        let Some((pos, _)) = find_first_cut_cue(s) else {
            return Some(s.to_string());
        };
        Some(s[pos..].to_string())
    }

    fn stub_model_cuts_after_cue(s: &str) -> Option<String> {
        let Some((pos, len)) = find_first_cut_cue(s) else {
            return Some(s.to_string());
        };
        let end = pos + len;
        let rest = &s[end..];
        if !rest.chars().any(char::is_alphanumeric) {
            return Some(s.to_string());
        }
        let prefix = s[..end].trim_end();
        if prefix.ends_with(['.', '?', '!', ',']) {
            Some(prefix.to_string())
        } else {
            Some(format!("{prefix}."))
        }
    }

    fn load_must_not_change_66() -> Vec<(String, String)> {
        let raw = include_str!("../../docs/eval/must-not-change-66.jsonl");
        let rows: Vec<(String, String)> = raw
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|line| {
                let v: serde_json::Value = serde_json::from_str(line).unwrap();
                (
                    v["id"].as_str().unwrap().to_string(),
                    v["input"].as_str().unwrap().to_string(),
                )
            })
            .collect();
        assert_eq!(rows.len(), 66);
        rows
    }

    #[test]
    fn r1f_t1_model_cuts_before_cue_keeps_all_66_and_named_k29() {
        use crate::actions::apply_post_process_cleanup_core;
        use crate::audio_toolkit::text::filter_transcription_output_with_policy;

        let k29 = "I'll send it to you. No wait, tomorrow.";
        assert_eq!(
            stub_model_cuts_before_cue(k29).as_deref(),
            Some("No wait, tomorrow.")
        );
        let (k29_out, _) =
            apply_post_process_cleanup_core(k29, true, stub_model_cuts_before_cue);

        let rows = load_must_not_change_66();
        let mut fail_no_fill: Vec<String> = Vec::new();
        let mut fail_with_fill: Vec<String> = Vec::new();
        for (id, input) in &rows {
            let pre_nf =
                filter_transcription_output_with_policy(input, "en", &None, false, true);
            let (out_nf, _) =
                apply_post_process_cleanup_core(&pre_nf, true, stub_model_cuts_before_cue);
            if out_nf != *input {
                fail_no_fill.push(format!("{id}: got={out_nf:?}"));
            }

            let pre_f =
                filter_transcription_output_with_policy(input, "en", &None, true, true);
            let (out_f, _) =
                apply_post_process_cleanup_core(&pre_f, true, stub_model_cuts_before_cue);
            if out_f != pre_f {
                fail_with_fill.push(format!("{id}: expected={pre_f:?} got={out_f:?}"));
            }
        }

        assert!(
            k29_out == k29 && fail_no_fill.is_empty() && fail_with_fill.is_empty(),
            "T1 model_cuts_before_cue failures: k29_ok={} no_fill={}/66 with_fill={}/66\nno_fill: {:?}\nwith_fill: {:?}",
            k29_out == k29,
            fail_no_fill.len(),
            fail_with_fill.len(),
            fail_no_fill,
            fail_with_fill
        );
    }

    #[test]
    fn r1f_t2_model_cuts_after_cue_keeps_all_66_and_named_k29() {
        use crate::actions::apply_post_process_cleanup_core;
        use crate::audio_toolkit::text::filter_transcription_output_with_policy;

        let k29 = "I'll send it to you. No wait, tomorrow.";
        assert_eq!(
            stub_model_cuts_after_cue(k29).as_deref(),
            Some("I'll send it to you. No wait.")
        );
        let (k29_out, _) =
            apply_post_process_cleanup_core(k29, true, stub_model_cuts_after_cue);

        let rows = load_must_not_change_66();
        let mut fail_no_fill: Vec<String> = Vec::new();
        let mut fail_with_fill: Vec<String> = Vec::new();
        for (id, input) in &rows {
            let pre_nf =
                filter_transcription_output_with_policy(input, "en", &None, false, true);
            let (out_nf, _) =
                apply_post_process_cleanup_core(&pre_nf, true, stub_model_cuts_after_cue);
            // k38 ("Send it to the client. No wait, hmm.") drops only the exempt English filler
            // "hmm" when fillers are off; T4 asserts that drop is allowed while every non-filler
            // word stays intact. All other 65 rows must come back byte-identical to input.
            let expected_nf = if id == "k38" {
                "Send it to the client. No wait."
            } else {
                input.as_str()
            };
            if out_nf != expected_nf {
                fail_no_fill.push(format!("{id}: expected={expected_nf:?} got={out_nf:?}"));
            }

            let pre_f =
                filter_transcription_output_with_policy(input, "en", &None, true, true);
            let (out_f, _) =
                apply_post_process_cleanup_core(&pre_f, true, stub_model_cuts_after_cue);
            if out_f != pre_f {
                fail_with_fill.push(format!("{id}: expected={pre_f:?} got={out_f:?}"));
            }
        }

        assert!(
            k29_out == k29 && fail_no_fill.is_empty() && fail_with_fill.is_empty(),
            "T2 model_cuts_after_cue failures: k29_ok={} no_fill={}/66 with_fill={}/66\nno_fill: {:?}\nwith_fill: {:?}",
            k29_out == k29,
            fail_no_fill.len(),
            fail_with_fill.len(),
            fail_no_fill,
            fail_with_fill
        );
    }

    #[test]
    fn r1f_t3_rules_resolved_takebacks_allow_model_polish_and_ceiling() {
        use crate::actions::apply_post_process_cleanup_core;
        use crate::audio_toolkit::text::filter_transcription_output_with_policy;

        let a9b_raw = include_str!("../testdata/r1f/a9b-cases.jsonl");
        let takebacks: Vec<String> = a9b_raw
            .lines()
            .filter(|l| !l.trim().is_empty())
            .filter_map(|line| {
                let v: serde_json::Value = serde_json::from_str(line).unwrap();
                let id = v["id"].as_str().unwrap();
                if id.starts_with("tb") {
                    Some(v["input"].as_str().unwrap().to_string())
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(takebacks.len(), 18);

        // The R1 sentence + all 18 AC-A9b take-backs: rules remove the cue so a polishing stub
        // is pasted as given, and a stub that drops a word from a cue-free sentence is still taken
        // (rule (a)'s named ceiling).
        for input in &takebacks {
            let pre = filter_transcription_output_with_policy(input, "en", &None, true, true);
            let polished = format!("{pre} Polished.");
            let (out_polish, _) =
                apply_post_process_cleanup_core(&pre, true, |_| Some(polished.clone()));
            assert_eq!(out_polish, polished, "polish rejected for {input:?}");

            let dropped = pre
                .split_whitespace()
                .skip(1)
                .collect::<Vec<_>>()
                .join(" ");
            assert!(!dropped.is_empty(), "expected multi-word pre for {input:?}");
            let (out_drop, _) =
                apply_post_process_cleanup_core(&pre, true, |_| Some(dropped.clone()));
            assert_eq!(out_drop, dropped, "cue-free ceiling changed for {input:?}");
        }

        let r1 = "meeting on tuesday, scratch that, wednesday";
        let pre_r1 = filter_transcription_output_with_policy(r1, "en", &None, true, true);
        let (out_r1, _) =
            apply_post_process_cleanup_core(&pre_r1, true, |_| Some("Meeting on Wednesday.".to_string()));
        assert_eq!(out_r1, "Meeting on Wednesday.");
    }

    #[test]
    fn r1f_t6_and_t7_punctuation_edit_skips_second_pass() {
        use crate::actions::apply_post_process_cleanup_core;

        let t6_raw = include_str!("../testdata/r1f/t6-v7.jsonl");
        let t6_rows: Vec<serde_json::Value> = t6_raw
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        let t6_input = t6_rows[0]["input"].as_str().unwrap();
        let t6_model = t6_rows[1]["input"].as_str().unwrap();
        assert_eq!(t6_input, "Call Sam. Sorry, Ana.");
        assert_eq!(t6_model, "Call Sam, sorry, Ana.");

        let mut failures: Vec<String> = Vec::new();
        let (t6_out, _) =
            apply_post_process_cleanup_core(t6_input, true, |s| Some(s.replace(". Sorry,", ", sorry,")));
        if t6_out != t6_model {
            failures.push(format!("T6 expected={t6_model:?} got={t6_out:?}"));
        }

        // Old T6 keep: rules leave as said; punctuation-only stub output stays verbatim.
        let old_t6 = "Book the big room. Scratch that, we can use my office.";
        let old_t6_expected = "Book the big room, scratch that, we can use my office.";
        let (old_t6_out, _) = apply_post_process_cleanup_core(old_t6, true, |s| {
            Some(s.replace(". Scratch that,", ", scratch that,"))
        });
        if old_t6_out != old_t6_expected {
            failures.push(format!("OLD_T6 expected={old_t6_expected:?} got={old_t6_out:?}"));
        }

        // T7 (c11): punctuation-only stub output stays verbatim.
        let t7_input = "We missed the bus. Never mind, we can walk home.";
        let t7_expected = "We missed the bus, never mind, we can walk home.";
        let (t7_out, _) = apply_post_process_cleanup_core(t7_input, true, |s| {
            Some(s.replace(". Never mind,", ", never mind,"))
        });
        if t7_out != t7_expected {
            failures.push(format!("T7 expected={t7_expected:?} got={t7_out:?}"));
        }

        assert!(
            failures.is_empty(),
            "T6/T7 failures ({}):\n{}",
            failures.len(),
            failures.join("\n")
        );
    }

    #[test]
    fn r1f_t4_cleanup_core_filler_drop_passes_and_non_filler_drop_falls_back() {
        use crate::actions::apply_post_process_cleanup_core;

        let k38 = "Send it to the client. No wait, hmm.";
        let (out_hmm_dropped, _) = apply_post_process_cleanup_core(k38, true, |_| {
            Some("Send it to the client. No wait.".to_string())
        });
        assert_eq!(out_hmm_dropped, "Send it to the client. No wait.");

        let (out_client_dropped, _) = apply_post_process_cleanup_core(k38, true, |_| {
            Some("Send it to the. No wait, hmm.".to_string())
        });
        assert_eq!(out_client_dropped, k38);

        let k29 = "I'll send it to you. No wait, tomorrow.";
        let (out_you_dropped, _) = apply_post_process_cleanup_core(k29, true, |_| {
            Some("I'll send it to. No wait, tomorrow.".to_string())
        });
        assert_eq!(out_you_dropped, k29);
    }

    #[test]
    fn f1b_t8a_app_order_r21_keeps_every_word_under_stock_prompt() {
        use crate::actions::apply_post_process_cleanup_core_with_policy;
        use crate::audio_toolkit::text::filter_transcription_output_with_policy;

        let r21 = "Save the file as final v2, sorry, final v3.";
        let after_manager = filter_transcription_output_with_policy(r21, "en", &None, true, true);
        assert_eq!(after_manager, "Save the file as final v3.");
        assert!(
            !crate::audio_toolkit::text::has_repair_cue_hint(&after_manager),
            "app-order text has no cue left after manager rules"
        );
        let (out, _) = apply_post_process_cleanup_core_with_policy(
            &after_manager,
            true,
            true,
            |_| Some("final v3".to_string()),
        );
        assert_eq!(out, "Save the file as final v3.");
    }

    #[test]
    fn f1b_t8c_plain_dictation_keeps_every_word_under_stock_prompt() {
        use crate::actions::apply_post_process_cleanup_core_with_policy;

        let plain = "Save the file as final v3.";
        let (out, _) = apply_post_process_cleanup_core_with_policy(
            plain,
            true,
            true,
            |_| Some("final v3".to_string()),
        );
        assert_eq!(out, "Save the file as final v3.");
    }

    #[test]
    fn f1b_t3b_k02_word_guard_ceiling_and_polish_stays() {
        use crate::actions::apply_post_process_cleanup_core_with_policy;

        let k02 = "Do you want the red one or the black one?";
        assert!(
            !crate::audio_toolkit::text::has_repair_cue_hint(k02),
            "T3b precondition: backtrack-30 k02 must be cue-free"
        );

        let dropped_first = "you want the red one or the black one?";
        let (out_keep, _) = apply_post_process_cleanup_core_with_policy(
            k02,
            true,
            true,
            |_| Some(dropped_first.to_string()),
        );
        assert_eq!(
            out_keep, k02,
            "keep_every_word=true must fall back to pre when first word is dropped"
        );

        let (out_other_prompt, _) = apply_post_process_cleanup_core_with_policy(
            k02,
            true,
            false,
            |_| Some(dropped_first.to_string()),
        );
        assert_eq!(
            out_other_prompt, dropped_first,
            "keep_every_word=false must keep cue-free ceiling for non-stock prompts"
        );

        // Polish stays: case and punctuation only must still be taken with keep_every_word=true.
        let polished = "do you want the red one, or the black one!";
        let (out_polish, _) = apply_post_process_cleanup_core_with_policy(
            k02,
            true,
            true,
            |_| Some(polished.to_string()),
        );
        assert_eq!(
            out_polish, polished,
            "case and punctuation polish must still be taken with keep_every_word=true"
        );
    }

    #[test]
    fn f1b_local_stock_prompt_detection_covers_stock_edited_custom_and_fallback() {
        use crate::actions::{
            resolve_cleanup, resolve_local_llm_prompt, CleanupDecision, SkipReason,
        };
        use crate::settings::{
            get_default_settings, LLMPrompt, LOCAL_LLM_CLEANUP_PROMPT, LOCAL_LLM_PROVIDER_ID,
        };

        let mut settings = get_default_settings();
        settings.post_process_selected_prompt_id =
            Some("default_improve_transcriptions".to_string());

        // 1. Direct local LLM + unedited stock prompt -> keep_every_word = true.
        let (local_prompt, is_stock) = resolve_local_llm_prompt(&settings, "fallback");
        assert_eq!(local_prompt, LOCAL_LLM_CLEANUP_PROMPT);
        assert!(is_stock);

        // 2. Unavailable cloud provider falls back to local LLM -> provider_id is LOCAL_LLM_PROVIDER_ID,
        // so the local-LLM dispatch site sets keep_every_word = true.
        let decision = resolve_cleanup("openai", |id| {
            if id == LOCAL_LLM_PROVIDER_ID {
                Ok(())
            } else {
                Err(SkipReason::NoApiKey)
            }
        });
        assert!(matches!(
            decision,
            CleanupDecision::Run {
                ref provider_id,
                fallback_from: Some(_)
            } if provider_id == LOCAL_LLM_PROVIDER_ID
        ));
        let (_, is_stock_on_fallback) = resolve_local_llm_prompt(&settings, "fallback");
        assert!(is_stock_on_fallback);

        // 3. Edited stock prompt -> keep_every_word = false.
        let mut edited = settings.clone();
        if let Some(p) = edited
            .post_process_prompts
            .iter_mut()
            .find(|p| p.id == "default_improve_transcriptions")
        {
            p.prompt = "Translate this text into French:\n\n${output}".to_string();
        }
        let (_, is_stock_edited) = resolve_local_llm_prompt(&edited, "fallback");
        assert!(!is_stock_edited);

        // 4. Custom prompt -> keep_every_word = false.
        let mut custom = settings.clone();
        custom.post_process_prompts.push(LLMPrompt {
            id: "custom_formal".to_string(),
            name: "Formal".to_string(),
            prompt: "Make this formal:\n\n${output}".to_string(),
        });
        custom.post_process_selected_prompt_id = Some("custom_formal".to_string());
        let (_, is_stock_custom) = resolve_local_llm_prompt(&custom, "fallback");
        assert!(!is_stock_custom);

        // 5. Available cloud provider does NOT dispatch to LOCAL_LLM_PROVIDER_ID.
        let cloud_decision = resolve_cleanup("openai", |_| Ok(()));
        assert!(matches!(
            cloud_decision,
            CleanupDecision::Run {
                ref provider_id,
                fallback_from: None
            } if provider_id == "openai" && provider_id != LOCAL_LLM_PROVIDER_ID
        ));
    }

    #[test]
    fn f1d_t9a_apple_intelligence_unedited_stock_prompts_use_local_prompt_and_keep_every_word() {
        use crate::actions::resolve_provider_prompt;
        use crate::settings::{
            get_default_settings, APPLE_INTELLIGENCE_PROVIDER_ID, LOCAL_LLM_CLEANUP_PROMPT,
        };

        for stock_id in ["default_improve_transcriptions", "coco_formatted"] {
            let mut settings = get_default_settings();
            settings.post_process_selected_prompt_id = Some(stock_id.to_string());
            let stored = settings
                .post_process_prompts
                .iter()
                .find(|p| p.id == stock_id)
                .expect("stock prompt exists")
                .prompt
                .clone();

            let (resolved, keep_every_word) =
                resolve_provider_prompt(APPLE_INTELLIGENCE_PROVIDER_ID, &settings, &stored);
            assert!(
                keep_every_word,
                "T9a: apple_intelligence with unedited stock id {stock_id} must set keep_every_word=true"
            );
            assert_eq!(
                resolved, LOCAL_LLM_CLEANUP_PROMPT,
                "T9a (F1d-c): apple_intelligence with unedited stock id {stock_id} must resolve to LOCAL_LLM_CLEANUP_PROMPT"
            );
        }
    }

    #[test]
    fn f1d_t9b_apple_intelligence_edited_and_custom_prompts_keep_stored_text_and_flag_false() {
        use crate::actions::resolve_provider_prompt;
        use crate::settings::{get_default_settings, LLMPrompt, APPLE_INTELLIGENCE_PROVIDER_ID};

        // Edited stock prompt -> stored text and flag false.
        let mut edited = get_default_settings();
        edited.post_process_selected_prompt_id = Some("default_improve_transcriptions".to_string());
        let edited_body = "Translate this text into French:\n\n${output}".to_string();
        if let Some(p) = edited
            .post_process_prompts
            .iter_mut()
            .find(|p| p.id == "default_improve_transcriptions")
        {
            p.prompt = edited_body.clone();
        }
        let (resolved_edited, flag_edited) =
            resolve_provider_prompt(APPLE_INTELLIGENCE_PROVIDER_ID, &edited, &edited_body);
        assert!(!flag_edited);
        assert_eq!(resolved_edited, edited_body);

        // Custom prompt -> stored text and flag false.
        let mut custom = get_default_settings();
        let custom_body = "Make this formal:\n\n${output}".to_string();
        custom.post_process_prompts.push(LLMPrompt {
            id: "custom_formal".to_string(),
            name: "Formal".to_string(),
            prompt: custom_body.clone(),
        });
        custom.post_process_selected_prompt_id = Some("custom_formal".to_string());
        let (resolved_custom, flag_custom) =
            resolve_provider_prompt(APPLE_INTELLIGENCE_PROVIDER_ID, &custom, &custom_body);
        assert!(!flag_custom);
        assert_eq!(resolved_custom, custom_body);

        // Cloud provider with unedited stock prompt -> stored text and flag false.
        let mut cloud = get_default_settings();
        cloud.post_process_selected_prompt_id = Some("default_improve_transcriptions".to_string());
        let cloud_stored = cloud
            .post_process_prompts
            .iter()
            .find(|p| p.id == "default_improve_transcriptions")
            .unwrap()
            .prompt
            .clone();
        let (resolved_cloud, flag_cloud) = resolve_provider_prompt("openai", &cloud, &cloud_stored);
        assert!(!flag_cloud);
        assert_eq!(resolved_cloud, cloud_stored);
    }

    #[test]
    fn f1d_t9c_apple_intelligence_t8a_t8c_and_local_parity_keep_every_word_on_r21() {
        use crate::actions::{
            apply_post_process_cleanup_core_with_policy, resolve_provider_prompt,
        };
        use crate::audio_toolkit::text::filter_transcription_output_with_policy;
        use crate::settings::{
            get_default_settings, APPLE_INTELLIGENCE_PROVIDER_ID, LOCAL_LLM_PROVIDER_ID,
        };

        let mut settings = get_default_settings();
        settings.post_process_selected_prompt_id =
            Some("default_improve_transcriptions".to_string());
        let stored = settings
            .post_process_prompts
            .iter()
            .find(|p| p.id == "default_improve_transcriptions")
            .unwrap()
            .prompt
            .clone();

        let (apple_prompt, apple_keep) =
            resolve_provider_prompt(APPLE_INTELLIGENCE_PROVIDER_ID, &settings, &stored);
        let (local_prompt, local_keep) =
            resolve_provider_prompt(LOCAL_LLM_PROVIDER_ID, &settings, &stored);

        // App order (T8a) with Apple decision flag and stub reply "final v3".
        let r21 = "Save the file as final v2, sorry, final v3.";
        let after_manager = filter_transcription_output_with_policy(r21, "en", &None, true, true);
        assert_eq!(after_manager, "Save the file as final v3.");
        let (apple_t8a, _) =
            apply_post_process_cleanup_core_with_policy(&after_manager, true, apple_keep, |_| {
                Some("final v3".to_string())
            });
        assert_eq!(apple_t8a, "Save the file as final v3.");

        // Plain dictation (T8c) with Apple decision flag and stub reply "final v3".
        let plain = "Save the file as final v3.";
        let (apple_t8c, _) =
            apply_post_process_cleanup_core_with_policy(plain, true, apple_keep, |_| {
                Some("final v3".to_string())
            });
        assert_eq!(apple_t8c, "Save the file as final v3.");

        // T8b replacement: Apple decision and local decision give the exact same paste & prompt.
        let (local_t8a, _) =
            apply_post_process_cleanup_core_with_policy(&after_manager, true, local_keep, |_| {
                Some("final v3".to_string())
            });
        assert_eq!(apple_prompt, local_prompt);
        assert_eq!(apple_t8a, local_t8a);
    }
}
