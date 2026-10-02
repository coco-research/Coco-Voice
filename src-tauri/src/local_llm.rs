//! Local LLM post-processing via llama.cpp (llama-cpp-2).
//!
//! Provides on-device text generation for transcription cleanup using GGUF
//! models downloaded through the standard model manager. The GGUF is loaded
//! once and kept resident for the life of the process; each call reuses it.
//! Supports streaming token delivery via mpsc channel for live overlay display.

use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::{AddBos, LlamaModel};
use llama_cpp_2::sampling::LlamaSampler;
use llama_cpp_2::token::LlamaToken;
use log::{debug, warn};
use tokio::sync::mpsc;

/// Context window used for local post-processing. Prompts longer than
/// `N_CTX - max_tokens` are clamped by [`fit_to_context`].
const N_CTX: u32 = 2048;

/// The process-wide llama.cpp backend. Initialized once under the model cache
/// lock; its `Drop` frees global state, so it must outlive every cached model.
static BACKEND: OnceLock<LlamaBackend> = OnceLock::new();

/// One resident GGUF, reused across dictations. `LlamaModel` is `Send + Sync`
/// and does not borrow the backend, so it can live behind a static mutex.
static CACHED_MODEL: OnceLock<Mutex<Option<CachedModel>>> = OnceLock::new();

struct CachedModel {
    path: PathBuf,
    model: LlamaModel,
}

/// Generate text from a prompt using a local GGUF model via llama.cpp.
///
/// If `token_tx` is provided, streams generated tokens as they are produced.
/// Returns the full generated text on success.
pub async fn generate_text(
    model_path: &Path,
    system_prompt: &str,
    user_content: &str,
    max_tokens: i32,
    token_tx: Option<mpsc::Sender<String>>,
    cancel: Option<Arc<AtomicBool>>,
) -> Result<String, String> {
    let model_path = model_path.to_path_buf();
    let system_prompt = system_prompt.to_string();
    let user_content = user_content.to_string();

    // Run llama.cpp inference on a blocking thread to avoid blocking the
    // async runtime. The first call loads the model; later calls reuse it.
    // Dropping this task does not stop the thread; `cancel` does.
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
    .map_err(|e| format!("Local LLM task join error: {}", e))?
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

#[cfg(test)]
fn prompt_budget(max_tokens: i32) -> usize {
    (N_CTX as usize)
        .saturating_sub(clamped_max_gen(max_tokens))
        .max(1)
}

/// ChatML pieces. Only the middle (the transcript) may be truncated.
fn chat_template_parts(system_prompt: &str, user_content: &str) -> (String, String, String) {
    (
        format!("<|im_start|>system\n{system_prompt}\n<|im_end|>\n<|im_start|>user\n"),
        user_content.to_string(),
        "\n<|im_end|>\n<|im_start|>assistant\n".to_string(),
    )
}

/// Clamp a token list to `budget`, keeping a short head and the newest tail.
///
/// ponytail: naive head+tail clamp with a fixed 25/75 split. Upgrade to a
/// sliding window if transcripts routinely exceed the context.
fn fit_to_context(tokens_list: Vec<LlamaToken>, budget: usize) -> Vec<LlamaToken> {
    if tokens_list.len() <= budget || budget == 0 {
        return if budget == 0 { Vec::new() } else { tokens_list };
    }
    let head = (budget / 4).max(1).min(budget);
    let tail = budget - head;
    let mut kept = tokens_list;
    let tail_tokens = kept.split_off(kept.len() - tail);
    kept.truncate(head);
    kept.extend(tail_tokens);
    kept
}

/// Drop tokens from `transcript` only, then wrap the chat template back around
/// what remains. Prefix and suffix tokens are never cut, even if they alone
/// exceed `budget`.
fn fit_transcript_tokens(
    mut prefix: Vec<LlamaToken>,
    transcript: Vec<LlamaToken>,
    suffix: Vec<LlamaToken>,
    budget: usize,
) -> Vec<LlamaToken> {
    let template = prefix.len().saturating_add(suffix.len());
    let room = budget.saturating_sub(template);
    let transcript = if transcript.len() <= room {
        transcript
    } else {
        fit_to_context(transcript, room)
    };
    prefix.extend(transcript);
    prefix.extend(suffix);
    prefix
}

fn generate_text_blocking(
    model_path: &Path,
    system_prompt: &str,
    user_content: &str,
    max_tokens: i32,
    token_tx: Option<mpsc::Sender<String>>,
    cancel: Option<Arc<AtomicBool>>,
) -> Result<String, String> {
    let max_gen = clamped_max_gen(max_tokens);
    // One clamp per call. `prompt_budget()` would clamp (and warn) again.
    let prompt_budget = (N_CTX as usize).saturating_sub(max_gen).max(1);
    let cancelled = || {
        cancel
            .as_ref()
            .is_some_and(|flag| flag.load(Ordering::Relaxed))
    };

    // All model access (backend init, load, generation) is serialized here.
    // Besides keeping one resident GGUF, this prevents two concurrent
    // dictations from racing `LlamaBackend::init()` (it can only run once).
    let cache = CACHED_MODEL.get_or_init(|| Mutex::new(None));
    let mut cache_guard = cache
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let backend: &'static LlamaBackend = match BACKEND.get() {
        Some(backend) => backend,
        None => {
            let backend = LlamaBackend::init()
                .map_err(|e| format!("Failed to initialize llama backend: {}", e))?;
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
            .map_err(|e| format!("Failed to load GGUF model: {}", e))?;
        *cache_guard = Some(CachedModel {
            path: model_path.to_path_buf(),
            model,
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
        .map_err(|e| format!("Failed to create llama context: {}", e))?;

    // Tokenize the chat template and the transcript apart so a long transcript
    // cannot cut through `<|im_start|>` turn markers. BOS belongs on the prefix
    // only; a second BOS would land in the middle of the prompt.
    let (prefix, transcript, suffix) = chat_template_parts(system_prompt, user_content);
    let prefix_tokens = model
        .str_to_token(&prefix, AddBos::Always)
        .map_err(|e| format!("Failed to tokenize prompt: {}", e))?;
    let transcript_tokens = model
        .str_to_token(&transcript, AddBos::Never)
        .map_err(|e| format!("Failed to tokenize prompt: {}", e))?;
    let suffix_tokens = model
        .str_to_token(&suffix, AddBos::Never)
        .map_err(|e| format!("Failed to tokenize prompt: {}", e))?;
    let tokens_list = fit_transcript_tokens(
        prefix_tokens,
        transcript_tokens,
        suffix_tokens,
        prompt_budget,
    );
    if tokens_list.is_empty() {
        return Err("Local LLM prompt produced no tokens".to_string());
    }

    let mut batch = LlamaBatch::new(N_CTX as usize, 1);
    let last_index = (tokens_list.len() - 1) as i32;
    for (i, token) in tokens_list.iter().enumerate() {
        let is_last = i as i32 == last_index;
        batch
            .add(*token, i as i32, &[0], is_last)
            .map_err(|e| format!("Failed to add token to batch: {}", e))?;
    }

    ctx.decode(&mut batch)
        .map_err(|e| format!("Failed to decode prompt batch: {}", e))?;
    let mut n_past = tokens_list.len() as i32;

    let mut generated = String::new();
    let mut tokens_generated = 0usize;

    // One decoder for the whole generation. A fresh decoder per token drops a
    // UTF-8 sequence that llama.cpp splits across token boundaries.
    let mut decoder = encoding_rs::UTF_8.new_decoder();

    // Create greedy sampler for deterministic output
    let mut sampler = LlamaSampler::greedy();

    while tokens_generated < max_gen && n_past < N_CTX as i32 - 1 {
        if cancelled() {
            break;
        }

        // Sample next token using greedy strategy
        let new_token = sampler.sample(&ctx, -1);

        // Check for end-of-generation token
        if model.is_eog_token(new_token) {
            break;
        }

        let token_str = model
            .token_to_piece(new_token, &mut decoder, false, None)
            .map_err(|e| format!("Failed to convert token to string: {}", e))?;

        tokens_generated += 1;

        // A cancel between sampling and send must not reach the overlay.
        // A closed channel means the receiver stopped; leave the mutex.
        if cancelled() {
            break;
        }
        if !token_str.is_empty() {
            generated.push_str(&token_str);
            if let Some(ref tx) = token_tx {
                if tx.blocking_send(token_str).is_err() {
                    break;
                }
            }
        }

        // Accept token in sampler (updates internal state for repetition etc.)
        sampler.accept(new_token);

        // Prepare next batch with the single generated token, advancing the
        // position so the model attends to real context instead of rewriting
        // every token at position 0.
        batch.clear();
        batch
            .add(new_token, n_past, &[0], true)
            .map_err(|e| format!("Failed to add generated token to batch: {}", e))?;

        ctx.decode(&mut batch)
            .map_err(|e| format!("Failed to decode generation batch: {}", e))?;
        n_past += 1;
    }

    if !cancelled() {
        let mut tail = String::with_capacity(8);
        let _ = decoder.decode_to_string(&[], &mut tail, true);
        if !tail.is_empty() {
            generated.push_str(&tail);
            if let Some(ref tx) = token_tx {
                let _ = tx.blocking_send(tail);
            }
        }
    }

    debug!(
        "Local LLM generation complete: {} tokens, {} chars",
        tokens_generated,
        generated.len()
    );

    Ok(generated)
}

#[cfg(test)]
mod tests {
    use super::{clamped_max_gen, fit_to_context, fit_transcript_tokens, prompt_budget, N_CTX};
    use llama_cpp_2::token::LlamaToken;

    fn tokens(n: i32) -> Vec<LlamaToken> {
        (0..n).map(LlamaToken).collect()
    }

    #[test]
    fn keeps_prompts_within_budget_unchanged() {
        assert_eq!(fit_to_context(tokens(10), 16), tokens(10));
    }

    #[test]
    fn clamps_long_prompts_keeping_head_and_tail() {
        let fitted = fit_to_context(tokens(100), 20);
        assert_eq!(fitted.len(), 20);
        // Head keeps the first 5 tokens (budget/4), tail keeps the last 15.
        assert_eq!(&fitted[..5], &tokens(5)[..]);
        assert_eq!(fitted[5], LlamaToken(85));
        assert_eq!(fitted[19], LlamaToken(99));
    }

    #[test]
    fn clamps_requested_generation_to_half_the_context() {
        assert_eq!(clamped_max_gen(N_CTX as i32), (N_CTX as usize) / 2);
        assert_eq!(prompt_budget(512), N_CTX as usize - 512);
        assert_eq!(clamped_max_gen(0), 512);
    }

    #[test]
    fn truncation_keeps_the_chat_template() {
        let prefix: Vec<_> = (0..4).map(|i| LlamaToken(500 + i)).collect();
        let suffix = vec![LlamaToken(1000), LlamaToken(1001)];
        let fitted = fit_transcript_tokens(prefix.clone(), tokens(100), suffix.clone(), 20);
        assert_eq!(fitted.len(), 20);
        assert_eq!(&fitted[..4], &prefix[..]);
        assert_eq!(&fitted[fitted.len() - 2..], &suffix[..]);

        // Template larger than the budget stays intact; the transcript is dropped.
        let fitted = fit_transcript_tokens(tokens(8), tokens(5), suffix.clone(), 4);
        assert_eq!(fitted.len(), 10);
        assert_eq!(&fitted[..8], &tokens(8)[..]);
        assert_eq!(&fitted[8..], &suffix[..]);
    }
}
