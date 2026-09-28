//! Local LLM post-processing via llama.cpp (llama-cpp-2).
//!
//! Provides on-device text generation for transcription cleanup using GGUF
//! models downloaded through the standard model manager. The GGUF is loaded
//! once and kept resident for the life of the process; each call reuses it.
//! Supports streaming token delivery via mpsc channel for live overlay display.

use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::{AddBos, LlamaModel, Special};
use llama_cpp_2::sampling::LlamaSampler;
use llama_cpp_2::token::LlamaToken;
use log::debug;
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
) -> Result<String, String> {
    let model_path = model_path.to_path_buf();
    let system_prompt = system_prompt.to_string();
    let user_content = user_content.to_string();

    // Run llama.cpp inference on a blocking thread to avoid blocking the
    // async runtime. The first call loads the model; later calls reuse it.
    tokio::task::spawn_blocking(move || {
        generate_text_blocking(
            &model_path,
            &system_prompt,
            &user_content,
            max_tokens,
            token_tx,
        )
    })
    .await
    .map_err(|e| format!("Local LLM task join error: {}", e))?
}

/// Clamp a prompt to the context budget, keeping the system-prompt head and the
/// newest transcription tail.
///
/// ponytail: naive head+tail clamp with a fixed 25/75 split. Upgrade to a
/// sliding window if inputs routinely exceed `N_CTX`.
fn fit_to_context(tokens_list: Vec<LlamaToken>, budget: usize) -> Vec<LlamaToken> {
    if tokens_list.len() <= budget {
        return tokens_list;
    }
    let head = (budget / 4).max(1);
    let tail = budget - head;
    let mut kept = tokens_list;
    let tail_tokens = kept.split_off(kept.len() - tail);
    kept.truncate(head);
    kept.extend(tail_tokens);
    kept
}

fn generate_text_blocking(
    model_path: &Path,
    system_prompt: &str,
    user_content: &str,
    max_tokens: i32,
    token_tx: Option<mpsc::Sender<String>>,
) -> Result<String, String> {
    let max_gen = if max_tokens > 0 {
        max_tokens as usize
    } else {
        512
    };
    let prompt_budget = (N_CTX as usize).saturating_sub(max_gen).max(1);

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
        let model = LlamaModel::load_from_file(backend, model_path, &LlamaModelParams::default())
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

    // Build chat template prompt (Qwen2.5 format)
    let prompt = format!(
        "<|im_start|>system\n{}\n<|im_end|>\n<|im_start|>user\n{}\n<|im_end|>\n<|im_start|>assistant\n",
        system_prompt, user_content
    );

    let tokens_list = model
        .str_to_token(&prompt, AddBos::Always)
        .map_err(|e| format!("Failed to tokenize prompt: {}", e))?;
    let tokens_list = fit_to_context(tokens_list, prompt_budget);

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

    // Create greedy sampler for deterministic output
    let mut sampler = LlamaSampler::greedy();

    while tokens_generated < max_gen && n_past < N_CTX as i32 - 1 {
        // Sample next token using greedy strategy
        let new_token = sampler.sample(&ctx, -1);

        // Check for end-of-generation token
        if model.is_eog_token(new_token) {
            break;
        }

        #[allow(deprecated)]
        let token_str = model
            .token_to_str(new_token, Special::Plaintext)
            .map_err(|e| format!("Failed to convert token to string: {}", e))?;

        generated.push_str(&token_str);
        tokens_generated += 1;

        // Stream token if sender is available
        if let Some(ref tx) = token_tx {
            let _ = tx.blocking_send(token_str.to_string());
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

    debug!(
        "Local LLM generation complete: {} tokens, {} chars",
        tokens_generated,
        generated.len()
    );

    Ok(generated)
}

#[cfg(test)]
mod tests {
    use super::fit_to_context;
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
}
