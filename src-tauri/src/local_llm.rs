//! Local LLM post-processing via llama.cpp (llama-cpp-2).
//!
//! Provides on-device text generation for transcription cleanup using GGUF
//! models downloaded through the standard model manager. Supports streaming
//! token delivery via mpsc channel for live overlay display.

use std::num::NonZeroU32;
use std::path::Path;

use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::{AddBos, LlamaModel, Special};
use llama_cpp_2::sampling::LlamaSampler;
use log::debug;
use tokio::sync::mpsc;

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
    // async runtime. The model load + generation can take several seconds.
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

fn generate_text_blocking(
    model_path: &Path,
    system_prompt: &str,
    user_content: &str,
    max_tokens: i32,
    token_tx: Option<mpsc::Sender<String>>,
) -> Result<String, String> {
    debug!(
        "Loading local LLM model from: {}",
        model_path.display()
    );

    let backend = LlamaBackend::init()
        .map_err(|e| format!("Failed to initialize llama backend: {}", e))?;

    let model_params = LlamaModelParams::default();
    let model = LlamaModel::load_from_file(&backend, model_path, &model_params)
        .map_err(|e| format!("Failed to load GGUF model: {}", e))?;

    let ctx_params = LlamaContextParams::default()
        .with_n_ctx(NonZeroU32::new(2048));
    let mut ctx = model
        .new_context(&backend, ctx_params)
        .map_err(|e| format!("Failed to create llama context: {}", e))?;

    // Build chat template prompt (Qwen2.5 format)
    let prompt = format!(
        "<|im_start|>system\n{}\n<|im_end|>\n<|im_start|>user\n{}\n<|im_end|>\n<|im_start|>assistant\n",
        system_prompt, user_content
    );

    let tokens_list = model
        .str_to_token(&prompt, AddBos::Always)
        .map_err(|e| format!("Failed to tokenize prompt: {}", e))?;

    let mut batch = LlamaBatch::new(512, 1);
    let last_index = (tokens_list.len() - 1) as i32;
    for (i, token) in tokens_list.into_iter().enumerate() {
        let is_last = i as i32 == last_index;
        batch
            .add(token, i as i32, &[0], is_last)
            .map_err(|e| format!("Failed to add token to batch: {}", e))?;
    }

    ctx.decode(&mut batch)
        .map_err(|e| format!("Failed to decode prompt batch: {}", e))?;

    let mut generated = String::new();
    let mut tokens_generated = 0i32;
    let max_gen = if max_tokens > 0 { max_tokens } else { 512 };

    // Create greedy sampler for deterministic output
    let mut sampler = LlamaSampler::greedy();

    loop {
        if tokens_generated >= max_gen {
            break;
        }

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

        // Prepare next batch with single token
        batch.clear();
        batch
            .add(new_token, 0, &[0], true)
            .map_err(|e| format!("Failed to add generated token to batch: {}", e))?;

        ctx.decode(&mut batch)
            .map_err(|e| format!("Failed to decode generation batch: {}", e))?;
    }

    debug!(
        "Local LLM generation complete: {} tokens, {} chars",
        tokens_generated,
        generated.len()
    );

    Ok(generated)
}
