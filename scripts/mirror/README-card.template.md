---
license: { spdx_id }
base_model: { upstream_repo }
tags:
  - gguf
  - automatic-speech-recognition
  - speech-to-text
---

# {model} (GGUF)

Unofficial mirror of {model} by {publisher}, converted to GGUF by handy-computer. Not affiliated with or endorsed by {publisher}.

The publisher name is used only to identify the model this file was converted from.

## Files

This repository holds one file: `{mirrored_filename}`, quantization `{quant}`. That is the only quant mirrored here. The other quants handy-computer publishes for this model are not copied.

| File                  | Quant   | SHA-256    |
| --------------------- | ------- | ---------- |
| `{mirrored_filename}` | {quant} | `{sha256}` |

## Where it comes from

- Original model: {model}
- Publisher: {publisher}
- Upstream repository: {upstream_repo_url}
- Upstream commit: `{upstream_commit}`
- Converter: handy-computer. Converted to GGUF at commit `{converted_at_commit}` for transcribe.cpp, https://github.com/handy-computer/transcribe.cpp
- Licence: {licence_name}, {licence_url}

## Licence and notices

`LICENSE` is the licence text, copied unchanged from handy-computer's repository. `NOTICE` records the conversion and states that the weights were not retrained or fine-tuned. `ATTRIBUTION.md` names the publisher, the upstream commit, the licence and the converter.

The GGUF bytes here are identical to the file handy-computer published at the commit above. Nothing was re-quantized, and nothing was written inside the GGUF.

## Variant: Qwen3-4B (Qwen/Qwen3-4B-GGUF)

`Qwen/Qwen3-4B-GGUF` is not a handy-computer conversion. For that model only:

- Use this sentence instead of the one above: "Unofficial mirror of Qwen3-4B-GGUF by Qwen, as published by Qwen. Not affiliated with or endorsed by Qwen."
- Drop the converter line and the conversion sentence in "Licence and notices". There is no handy-computer NOTICE for this model, so do not add one.
- Front matter stays `license: apache-2.0`, `base_model: Qwen/Qwen3-4B-GGUF`, tags unchanged.
- Upstream commit and file hash come from Qwen's own repository at the pin in `model_licences.json`.

## Template notes, delete before publishing

- `{model}` is the model name as the publisher writes it, without the organisation prefix. Example: `whisper-large-v3-turbo`.
- `{publisher}` is the plain publisher name, the same one used in the "not affiliated with or endorsed by" sentence. Example: `OpenAI`, `NVIDIA`, `Qwen`, `IBM`, `Mistral`, `MediaTek`, `GigaChat`, `Useful Sensors`.
- `{spdx_id}` is the SPDX id in Hugging Face front matter form, lowercase: `mit`, `apache-2.0`, or `cc-by-4.0`.
- `{upstream_repo}` is the publisher's repository id, for example `openai/whisper-large-v3-turbo`. `{upstream_repo_url}` is its full https URL.
- `{converted_at_commit}` is the commit handy-computer's NOTICE names as the one it converted from. `{upstream_commit}` is the same commit for every model whose NOTICE and `model_licences.json` pin agree. Fill both separately, and check them against the NOTICE.
- `{sha256}` is the hash of the mirrored file. Write the lowercase hex, no spaces.
- `{mirrored_filename}` and `{quant}` are the one default-quant file for the model, the entry whose `quant` matches `default_quant` in `catalog.json`.
- If a value cannot be read from the source files, write `{UNKNOWN: what is missing}` and leave it visible. Do not guess.
- Never add a publisher logo, the word "official", the phrase "Coco models", a donate link, or a sponsor link. Do not gate the repository and do not add DRM.
