# Coco Voice — Voice Synthesis & Correction Plan (FINAL)

## Goal
Add a Wispr Flow-style layer on top of Coco Voice's existing local transcription: after Whisper produces raw text, a **local** language model cleans and formats it (removes fillers, punctuates, structures into bullets/paragraphs, obeys spoken formatting commands), supports iterative spoken corrections, and applies a persistent personal correction dictionary. Everything on-device — preserves the "nothing leaves your machine" promise.

## Backend Decision
**Local only.** Small instruction-tuned model (Qwen2.5-3B-Instruct recommended) via a llama.cpp Rust binding with **Metal** acceleration on Apple Silicon. whisper.cpp (already bundled as transcribe-cpp) and llama.cpp share ggml, so the runtime infra is familiar.

### Model acquisition (finalized)
Do **not** bundle the model — a 3B Q4_K_M GGUF is ~2 GB and would bloat the app. Use **first-run download** of a pinned GGUF to the app data dir (mirrors how Coco Voice already fetches the Silero VAD / whisper models). Caveat: the model host must be reachable; if HuggingFace is blocked in a given environment, host the GGUF on the same infra used for the other models. Verify reachability before committing to a source.

## Architecture
Current Rust pipeline: audio → VAD → Whisper (transcribe-cpp) → clipboard paste.
New: audio → VAD → Whisper → **synthesis (LLM)** → **dictionary pass (deterministic, runs last)** → clipboard paste, with a settings toggle for raw vs. cleaned.

New components:
- A `synthesis` manager (Rust) parallel to the transcription manager, wrapping llama.cpp (Metal).
- A **refine buffer** holding the last {raw transcript, produced output}; cleared on new dictation or timeout.
- A **correction dictionary** persisted in the settings store.
- Frontend: settings (enable synthesis, style presets concise/bulleted/verbatim, dictionary editor) + a small overlay to view/accept/refine.

## Latency & UX (finalized)
Target < ~2s for a typical utterance. **Paste raw first, then replace with the cleaned version** so the user is never blocked; stream the cleaned output as it generates; **skip synthesis for very short utterances** where it adds nothing. Verbatim mode always available as a fallback; synthesis must never invent content.

## Corrections
- **Inline self-correction** ("Friday, no, Monday") — resolved in the single synthesis pass.
- **Follow-up correction** ("no, make it more formal") — re-runs synthesis with the refine buffer as prior context, editing the previous output.

## Personal correction dictionary (finalized)
Persistent map (e.g. "cocoa"/"coco" → "COCO"). Applied as a **deterministic final pass after synthesis** so exact forms win even if the model re-normalizes; also injected into the synthesis prompt for context.
- First inspect Handy's existing `custom_words` (likely a Whisper vocabulary *hint*, not a replacement map) and decide extend-vs-new.
- **Auto-learn is limited to observable events:** the app cannot see edits the user makes in an external app after paste. Learn only from (a) spoken follow-up corrections and (b) edits made in the accept/refine overlay. State this clearly in the UI.

## Phases
1. Synthesis pass — transcript → local LLM cleanup+punctuation → replace output, behind a toggle. Proves pipeline + latency on-device.
2. Formatting intelligence — bullets/structure + spoken formatting commands.
3. Correction dictionary — deterministic final-pass substitution + settings editor + observable auto-learn.
4. Iterative correction loop — multi-turn refine buffer.
5. Polish — streaming, latency tuning, per-app tone, optional fine-tuned local model.

## Sequencing
Executes after Coco PDF branding is locked (or in parallel if requested). Phase 1 first, and its very first step is recon: inspect the current transcription manager, the settings store, and `custom_words`, plus verify a reachable GGUF source.

## Risks
On-device latency on target hardware (mitigate: smaller/quantized model, streaming, skip-short); model host reachability; guardrail against hallucinated content (verbatim fallback).
