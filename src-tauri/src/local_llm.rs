//! Local LLM post-processing via llama.cpp (llama-cpp-2).
//!
//! On-device text generation for transcription cleanup using GGUF models
//! downloaded through the standard model manager. The model is loaded on the
//! first cleanup request and kept resident between takes. After five minutes
//! with no cleanup request the llama context and model are dropped so that
//! memory can be returned. A request that arrives while that drop is in
//! progress waits on the same mutex, sees an empty slot, and loads again.
//!
//! A transcript that does not fit in the prompt budget is refused, and so is any
//! generation that stops without an end-of-sequence token (token cap, full
//! context, receiver gone) or is cancelled. The caller then pastes the raw
//! transcript.

use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::{AddBos, LlamaModel};
use llama_cpp_2::sampling::LlamaSampler;
use log::{debug, warn};
use tokio::sync::mpsc;

#[derive(Debug, PartialEq)]
pub(crate) enum LocalLlmError {
    OverBudget,
    StoppedEarly,
    Cancelled,
    Other(String),
}

impl std::fmt::Display for LocalLlmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OverBudget => write!(
                f,
                "Local LLM prompt exceeds the context budget; pasting the raw transcript"
            ),
            Self::StoppedEarly => write!(
                f,
                "Local LLM stopped without an end-of-sequence token; pasting the raw transcript"
            ),
            Self::Cancelled => write!(f, "Local LLM cleanup was cancelled"),
            Self::Other(s) => write!(f, "{}", s),
        }
    }
}

/// Context window used for local post-processing. A prompt that does not fit
/// in `N_CTX - max_tokens` is refused so the raw transcript is pasted.
const N_CTX: u32 = 2048;

/// How long a loaded model stays resident after the last cleanup request.
const IDLE_TIMEOUT: Duration = Duration::from_secs(5 * 60);

/// The process-wide llama.cpp backend. Initialized once under the model cache
/// lock; its `Drop` frees global state, so it must outlive every cached model.
/// Unload drops the model, not this backend: `LlamaBackend::init` can run once.
static BACKEND: OnceLock<LlamaBackend> = OnceLock::new();

/// One resident GGUF. `LlamaModel` is `Send + Sync` and does not borrow the
/// backend, so it can live behind a static mutex.
static CACHED_MODEL: OnceLock<ModelCache> = OnceLock::new();

/// Started on the first cleanup request, never at app start or when settings open.
static IDLE_WATCHER: OnceLock<()> = OnceLock::new();

struct ModelCache {
    slot: Mutex<Option<CachedModel>>,
    /// Wakes the idle watcher when a request finishes or the slot changes.
    cv: Condvar,
}

struct CachedModel {
    path: PathBuf,
    model: LlamaModel,
    last_used: Instant,
}

fn model_cache() -> &'static ModelCache {
    CACHED_MODEL.get_or_init(|| ModelCache {
        slot: Mutex::new(None),
        cv: Condvar::new(),
    })
}

/// `true` when `last_used` is at least `timeout` before `now`.
///
/// The idle watcher unloads on `true`. A cleanup request holds the cache mutex
/// for its whole run and stamps `last_used` when it finishes, so the watcher
/// cannot observe a stale timestamp while a request is in flight.
fn idle_should_unload(last_used: Instant, now: Instant, timeout: Duration) -> bool {
    now.saturating_duration_since(last_used) >= timeout
}

fn ensure_idle_watcher() {
    // Create the cache before the thread so the watcher never races init.
    let _ = model_cache();
    let _ = IDLE_WATCHER.get_or_init(|| {
        if std::thread::Builder::new()
            .name("local-llm-idle".to_string())
            .spawn(idle_watch_loop)
            .is_err()
        {
            warn!("Failed to start the local LLM idle watcher; the model will stay resident");
        }
    });
}

/// Drop the cached model once it has been idle for [`IDLE_TIMEOUT`].
///
/// Unload and the next cleanup request share `ModelCache::slot`. A request that
/// arrives during the drop blocks on that mutex, then loads because the slot
/// is empty. It does not fail.
fn idle_watch_loop() {
    let cache = model_cache();
    let mut guard = cache.slot.lock().unwrap_or_else(|p| p.into_inner());
    loop {
        if let Some(cached) = guard.as_ref() {
            let now = Instant::now();
            if idle_should_unload(cached.last_used, now, IDLE_TIMEOUT) {
                debug!("Unloading idle local LLM");
                // Drop runs here, while this thread holds the mutex. A cleanup
                // request waits, then reloads.
                *guard = None;
                continue;
            }
            let elapsed = now.saturating_duration_since(cached.last_used);
            let remaining = IDLE_TIMEOUT.saturating_sub(elapsed);
            let (next, _) = cache
                .cv
                .wait_timeout(guard, remaining)
                .unwrap_or_else(|p| p.into_inner());
            guard = next;
        } else {
            guard = cache.cv.wait(guard).unwrap_or_else(|p| p.into_inner());
        }
    }
}

/// Generate text from a prompt using a local GGUF model via llama.cpp.
///
/// If `token_tx` is provided, streams generated tokens as they are produced.
/// Returns the full generated text on success. Returns an error when the
/// prompt does not fit, when generation stops without an end-of-sequence token
/// (token cap, full context, receiver gone), or when it is cancelled; the
/// caller pastes the raw transcript.
pub async fn generate_text(
    model_path: &Path,
    system_prompt: &str,
    user_content: &str,
    max_tokens: i32,
    token_tx: Option<mpsc::Sender<String>>,
    cancel: Option<Arc<AtomicBool>>,
) -> Result<String, LocalLlmError> {
    let model_path = model_path.to_path_buf();
    let system_prompt = system_prompt.to_string();
    let user_content = user_content.to_string();

    // Run llama.cpp inference on a blocking thread to avoid blocking the
    // async runtime. The first call loads the model; later calls reuse it
    // until the idle timeout. Dropping this task does not stop the thread;
    // `cancel` does.
    tokio::task::spawn_blocking(move || {
        generate_text_blocking(
            &model_path,
            &system_prompt,
            &user_content,
            max_tokens,
            token_tx,
            cancel,
        )
    })
    .await
    .map_err(|e| LocalLlmError::Other(format!("Local LLM task join error: {}", e)))?
}

/// Generation length that still leaves half the context for the prompt.
///
/// A caller that passes `max_tokens >= N_CTX` used to collapse the prompt
/// budget to one token via `saturating_sub`.
fn clamped_max_gen(max_tokens: i32) -> usize {
    let requested = if max_tokens > 0 {
        max_tokens as usize
    } else {
        512
    };
    let cap = (N_CTX as usize) / 2;
    if requested > cap {
        warn!(
            "Local LLM max_tokens {requested} would shrink the prompt below half of the {N_CTX} context; clamping to {cap}"
        );
        cap
    } else {
        requested
    }
}

/// Prompt tokens left once `max_gen` (already clamped) is reserved for output.
fn prompt_budget(max_gen: usize) -> usize {
    (N_CTX as usize).saturating_sub(max_gen).max(1)
}

/// `true` when the chat template plus the transcript do not fit in `budget`.
///
/// Equal to the budget is accepted. Over the budget the cleanup is refused
/// and the raw transcript is pasted.
fn prompt_over_budget(template_tokens: usize, transcript_tokens: usize, budget: usize) -> bool {
    template_tokens.saturating_add(transcript_tokens) > budget
}

/// Decide what a finished generation loop may return. Only a run that ended on
/// an end-of-sequence token is complete. Any other stop (token cap, full context,
/// receiver gone) is partial text and is refused, so the raw transcript is pasted.
fn stop_outcome(saw_eos: bool, cancelled: bool) -> Result<(), LocalLlmError> {
    if cancelled {
        Err(LocalLlmError::Cancelled)
    } else if !saw_eos {
        Err(LocalLlmError::StoppedEarly)
    } else {
        Ok(())
    }
}

/// Qwen3 template tail for `enable_thinking=false`: an empty think block the
/// model treats as already finished, so it answers without reasoning.
const QWEN3_NO_THINK: &str = "<think>\n\n</think>\n\n";

/// Whether `architecture` (GGUF `general.architecture`) or, when the metadata is
/// missing, the file name says this is a Qwen3 chat model. The `asr` guard keeps
/// the speech models (`qwen3_asr`) out; they are never loaded here anyway.
fn is_qwen3(architecture: Option<&str>, file_name: &str) -> bool {
    match architecture {
        Some(arch) => arch == "qwen3",
        None => {
            let name = file_name.to_ascii_lowercase();
            name.contains("qwen3") && !name.contains("asr")
        }
    }
}

/// ChatML pieces. The transcript is never truncated; an over-budget prompt is refused.
fn chat_template_parts(
    system_prompt: &str,
    user_content: &str,
    qwen3: bool,
) -> (String, String, String) {
    let think = if qwen3 { QWEN3_NO_THINK } else { "" };
    (
        format!("<|im_start|>system\n{system_prompt}\n<|im_end|>\n<|im_start|>user\n"),
        user_content.to_string(),
        format!("\n<|im_end|>\n<|im_start|>assistant\n{think}"),
    )
}

/// Remove `<think>...</think>` blocks a Qwen3 model may still emit. An unclosed
/// `<think>` (reasoning cut off by the token limit) drops the rest; a stray
/// `</think>` (the opening tag was part of the prompt) drops everything before it.
fn strip_think_blocks(text: &str) -> String {
    if !text.contains("<think>") && !text.contains("</think>") {
        return text.to_string();
    }
    let mut rest = text;
    let mut out = String::with_capacity(text.len());
    while let Some(open) = rest.find("<think>") {
        // A close tag before the next open tag has no matching open.
        if let Some(close) = rest[..open].find("</think>") {
            out.clear();
            rest = &rest[close + "</think>".len()..];
            continue;
        }
        out.push_str(&rest[..open]);
        match rest[open..].find("</think>") {
            Some(close) => rest = &rest[open + close + "</think>".len()..],
            None => return out.trim().to_string(),
        }
    }
    if let Some(close) = rest.find("</think>") {
        out.clear();
        rest = &rest[close + "</think>".len()..];
    }
    out.push_str(rest);
    out.trim().to_string()
}

fn generate_text_blocking(
    model_path: &Path,
    system_prompt: &str,
    user_content: &str,
    max_tokens: i32,
    token_tx: Option<mpsc::Sender<String>>,
    cancel: Option<Arc<AtomicBool>>,
) -> Result<String, LocalLlmError> {
    ensure_idle_watcher();
    let cache = model_cache();
    let mut guard = cache
        .slot
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let result = generate_with_model(
        &mut guard,
        model_path,
        system_prompt,
        user_content,
        max_tokens,
        token_tx,
        cancel,
    );

    // Stamp after the request, including a refused one that already loaded the
    // weights, so the idle clock starts when the request finishes.
    if let Some(cached) = guard.as_mut() {
        cached.last_used = Instant::now();
    }
    cache.cv.notify_all();
    result
}

fn generate_with_model(
    cache_guard: &mut Option<CachedModel>,
    model_path: &Path,
    system_prompt: &str,
    user_content: &str,
    max_tokens: i32,
    token_tx: Option<mpsc::Sender<String>>,
    cancel: Option<Arc<AtomicBool>>,
) -> Result<String, LocalLlmError> {
    let max_gen = clamped_max_gen(max_tokens);
    let prompt_budget = prompt_budget(max_gen);
    let cancelled = || {
        cancel
            .as_ref()
            .is_some_and(|flag| flag.load(Ordering::Acquire))
    };

    // All model access (backend init, load, generation) is serialized on the
    // cache mutex. Besides keeping one resident GGUF, this prevents two
    // concurrent dictations from racing `LlamaBackend::init()` (it can only
    // run once) and from observing a model the idle watcher is dropping.
    let backend: &'static LlamaBackend = match BACKEND.get() {
        Some(backend) => backend,
        None => {
            let backend = LlamaBackend::init().map_err(|e| {
                LocalLlmError::Other(format!("Failed to initialize llama backend: {}", e))
            })?;
            let _ = BACKEND.set(backend);
            BACKEND.get().expect("backend was just initialized")
        }
    };

    let needs_load = cache_guard
        .as_ref()
        .map(|cached| cached.path != model_path)
        .unwrap_or(true);
    if needs_load {
        debug!("Loading local LLM model from: {}", model_path.display());
        // Windows on ARM has no Metal/Vulkan backend in this crate build, so it
        // stays on the CPU default (0 GPU layers). Every other target offloads.
        // This module is macOS-only; the cfg stays so the load path matches the
        // crate's GPU rule if it is ever compiled elsewhere.
        let params = {
            #[cfg(not(all(target_os = "windows", target_arch = "aarch64")))]
            {
                LlamaModelParams::default().with_n_gpu_layers(999)
            }
            #[cfg(all(target_os = "windows", target_arch = "aarch64"))]
            {
                LlamaModelParams::default()
            }
        };
        let model = LlamaModel::load_from_file(backend, model_path, &params)
            .map_err(|e| LocalLlmError::Other(format!("Failed to load GGUF model: {}", e)))?;
        *cache_guard = Some(CachedModel {
            path: model_path.to_path_buf(),
            model,
            last_used: Instant::now(),
        });
    } else {
        debug!("Reusing cached local LLM model: {}", model_path.display());
    }
    let model = &cache_guard
        .as_ref()
        .expect("model is cached after the load branch")
        .model;

    let ctx_params = LlamaContextParams::default().with_n_ctx(NonZeroU32::new(N_CTX));
    let mut ctx = model
        .new_context(backend, ctx_params)
        .map_err(|e| LocalLlmError::Other(format!("Failed to create llama context: {}", e)))?;

    // Tokenize the chat template and the transcript apart so a long transcript
    // cannot cut through `<|im_start|>` turn markers. BOS belongs on the prefix
    // only; a second BOS would land in the middle of the prompt.
    let architecture = model.meta_val_str("general.architecture").ok();
    let file_name = model_path
        .file_name()
        .map(|n| n.to_string_lossy())
        .unwrap_or_default();
    let qwen3 = is_qwen3(architecture.as_deref(), &file_name);
    let (prefix, transcript, suffix) = chat_template_parts(system_prompt, user_content, qwen3);
    let prefix_tokens = model
        .str_to_token(&prefix, AddBos::Always)
        .map_err(|e| LocalLlmError::Other(format!("Failed to tokenize prompt: {}", e)))?;
    let transcript_tokens = model
        .str_to_token(&transcript, AddBos::Never)
        .map_err(|e| LocalLlmError::Other(format!("Failed to tokenize prompt: {}", e)))?;
    let suffix_tokens = model
        .str_to_token(&suffix, AddBos::Never)
        .map_err(|e| LocalLlmError::Other(format!("Failed to tokenize prompt: {}", e)))?;
    if prompt_over_budget(
        prefix_tokens.len().saturating_add(suffix_tokens.len()),
        transcript_tokens.len(),
        prompt_budget,
    ) {
        return Err(LocalLlmError::OverBudget);
    }
    let mut tokens_list = prefix_tokens;
    tokens_list.extend(transcript_tokens);
    tokens_list.extend(suffix_tokens);
    if tokens_list.is_empty() {
        return Err(LocalLlmError::Other(
            "Local LLM prompt produced no tokens".to_string(),
        ));
    }

    let mut batch = LlamaBatch::new(N_CTX as usize, 1);
    let last_index = (tokens_list.len() - 1) as i32;
    for (i, token) in tokens_list.iter().enumerate() {
        let is_last = i as i32 == last_index;
        batch
            .add(*token, i as i32, &[0], is_last)
            .map_err(|e| LocalLlmError::Other(format!("Failed to add token to batch: {}", e)))?;
    }

    ctx.decode(&mut batch)
        .map_err(|e| LocalLlmError::Other(format!("Failed to decode prompt batch: {}", e)))?;
    let mut n_past = tokens_list.len() as i32;

    let mut generated = String::new();
    let mut tokens_generated = 0usize;
    let mut saw_eos = false;

    // One decoder for the whole generation. A fresh decoder per token drops a
    // UTF-8 sequence that llama.cpp splits across token boundaries.
    let mut decoder = encoding_rs::UTF_8.new_decoder();

    // Create greedy sampler for deterministic output
    let mut sampler = LlamaSampler::greedy();
    let mut stream = token_tx.as_ref();

    while tokens_generated < max_gen && n_past < N_CTX as i32 - 1 {
        if cancelled() {
            break;
        }

        // Sample next token using greedy strategy
        let new_token = sampler.sample(&ctx, -1);

        // Check for end-of-generation token
        if model.is_eog_token(new_token) {
            saw_eos = true;
            break;
        }

        let token_str = model
            .token_to_piece(new_token, &mut decoder, false, None)
            .map_err(|e| {
                LocalLlmError::Other(format!("Failed to convert token to string: {}", e))
            })?;

        tokens_generated += 1;

        // A cancel between sampling and send must not reach the overlay.
        // A closed channel means the receiver stopped; leave the mutex.
        if cancelled() {
            break;
        }
        if !token_str.is_empty() {
            generated.push_str(&token_str);
            // Never block while holding the model mutex: a full channel only
            // stops streaming, a closed one stops generating.
            if let Some(tx) = stream {
                match tx.try_send(token_str) {
                    Ok(()) => {}
                    Err(mpsc::error::TrySendError::Full(_)) => {
                        warn!("Local LLM token stream is full; live preview stops, the result is unchanged");
                        stream = None;
                    }
                    Err(mpsc::error::TrySendError::Closed(_)) => break,
                }
            }
        }

        // Accept token in sampler (updates internal state for repetition etc.)
        sampler.accept(new_token);

        // Prepare next batch with the single generated token, advancing the
        // position so the model attends to real context instead of rewriting
        // every token at position 0.
        batch.clear();
        batch.add(new_token, n_past, &[0], true).map_err(|e| {
            LocalLlmError::Other(format!("Failed to add generated token to batch: {}", e))
        })?;

        ctx.decode(&mut batch).map_err(|e| {
            LocalLlmError::Other(format!("Failed to decode generation batch: {}", e))
        })?;
        n_past += 1;
    }

    // Context (`ctx`) drops at the end of this function. The model stays until
    // the idle watcher unloads it.
    stop_outcome(saw_eos, cancelled())?;

    let mut tail = String::with_capacity(8);
    let _ = decoder.decode_to_string(&[], &mut tail, true);
    if !tail.is_empty() {
        generated.push_str(&tail);
        if let Some(tx) = stream {
            let _ = tx.try_send(tail);
        }
    }

    debug!(
        "Local LLM generation complete: {} tokens, {} chars",
        tokens_generated,
        generated.len()
    );

    // Only Qwen3 emits think blocks; other models keep their output verbatim.
    Ok(if qwen3 {
        strip_think_blocks(&generated)
    } else {
        generated
    })
}

#[cfg(test)]
mod tests {
    use super::{
        chat_template_parts, clamped_max_gen, idle_should_unload, is_qwen3, prompt_budget,
        prompt_over_budget, stop_outcome, strip_think_blocks, LocalLlmError, N_CTX,
    };
    use std::time::{Duration, Instant};

    #[test]
    fn clamps_requested_generation_to_half_the_context() {
        assert_eq!(clamped_max_gen(N_CTX as i32), (N_CTX as usize) / 2);
        assert_eq!(prompt_budget(clamped_max_gen(512)), N_CTX as usize - 512);
        assert_eq!(clamped_max_gen(0), 512);
    }

    #[test]
    fn over_budget_refuses_and_exact_budget_fits() {
        assert!(prompt_over_budget(100, 50, 149));
        assert!(!prompt_over_budget(100, 49, 149));
        assert!(!prompt_over_budget(10, 0, 10));
    }

    #[test]
    fn only_a_run_that_ended_on_eos_is_kept() {
        // Token cap or full context: no end-of-sequence token, so refuse.
        assert_eq!(stop_outcome(false, false), Err(LocalLlmError::StoppedEarly));
        assert!(stop_outcome(true, false).is_ok());
        // A cancel is never pasted, finished or not.
        assert_eq!(stop_outcome(true, true), Err(LocalLlmError::Cancelled));
        assert_eq!(stop_outcome(false, true), Err(LocalLlmError::Cancelled));
    }

    #[test]
    fn local_llm_error_display_matches_old_messages() {
        assert_eq!(
            LocalLlmError::OverBudget.to_string(),
            "Local LLM prompt exceeds the context budget; pasting the raw transcript"
        );
        assert_eq!(
            LocalLlmError::StoppedEarly.to_string(),
            "Local LLM stopped without an end-of-sequence token; pasting the raw transcript"
        );
        assert_eq!(
            LocalLlmError::Cancelled.to_string(),
            "Local LLM cleanup was cancelled"
        );
        assert_eq!(
            LocalLlmError::Other("custom failure".to_string()).to_string(),
            "custom failure"
        );
    }

    #[test]
    fn idle_just_under_five_minutes_keeps_and_at_or_over_unloads() {
        let last_used = Instant::now();
        let timeout = Duration::from_secs(5 * 60);
        let just_under = last_used + timeout - Duration::from_secs(1);
        assert!(!idle_should_unload(last_used, just_under, timeout));
        assert!(idle_should_unload(last_used, last_used + timeout, timeout));
        assert!(idle_should_unload(
            last_used,
            last_used + timeout + Duration::from_secs(1),
            timeout
        ));
    }

    #[test]
    fn qwen3_prompt_ends_with_an_empty_think_block() {
        let (prefix, transcript, suffix) = chat_template_parts("sys", "hello", true);
        assert_eq!(
            prefix,
            "<|im_start|>system\nsys\n<|im_end|>\n<|im_start|>user\n"
        );
        assert_eq!(transcript, "hello");
        assert_eq!(
            suffix,
            "\n<|im_end|>\n<|im_start|>assistant\n<think>\n\n</think>\n\n"
        );
        // Other models keep the plain ChatML tail.
        let (_, _, plain) = chat_template_parts("sys", "hello", false);
        assert_eq!(plain, "\n<|im_end|>\n<|im_start|>assistant\n");
    }

    #[test]
    fn qwen3_is_detected_from_metadata_then_file_name() {
        assert!(is_qwen3(Some("qwen3"), "anything.gguf"));
        assert!(!is_qwen3(Some("qwen2"), "Qwen3-4B-Q4_K_M.gguf"));
        assert!(!is_qwen3(Some("qwen3_asr"), "Qwen3-ASR-1.7B-Q5_K_M.gguf"));
        assert!(is_qwen3(None, "Qwen3-4B-Q4_K_M.gguf"));
        assert!(!is_qwen3(None, "qwen2.5-3b-instruct-q4_k_m.gguf"));
        assert!(!is_qwen3(None, "Qwen3-ASR-0.6B-Q8_0.gguf"));
    }

    #[test]
    fn think_blocks_are_stripped_from_output() {
        assert_eq!(strip_think_blocks("Hello there."), "Hello there.");
        assert_eq!(
            strip_think_blocks("<think>\n\n</think>\n\nHello."),
            "Hello."
        );
        assert_eq!(
            strip_think_blocks("a <think>x</think>b<think>y</think> c"),
            "a b c"
        );
        // Reasoning cut off by the token limit leaves nothing after it.
        assert_eq!(strip_think_blocks("Hi <think>still thinking"), "Hi");
        // Opening tag lived in the prompt.
        assert_eq!(
            strip_think_blocks("reasoning</think>\n\nAnswer."),
            "Answer."
        );
    }
}
