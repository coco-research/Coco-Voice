# Model card: Qwen3 4B (Local LLM)

<!-- TODO(Bhavna): the "This is a model" UI line goes here once its wording is approved. -->

| Field         | Value                                                                                          |
| ------------- | ---------------------------------------------------------------------------------------------- |
| Model ID      | Qwen/Qwen3-4B-GGUF                                                                             |
| Source        | https://huggingface.co/Qwen/Qwen3-4B-GGUF at revision bc640142c66e1fdd12af0bd68f40445458f3869b |
| Licence       | apache-2.0                                                                                     |
| Architecture  | qwen3                                                                                          |
| File          | Qwen3-4B-Q4_K_M.gguf                                                                           |
| Quantization  | Q4_K_M                                                                                         |
| Size          | 2497280256 bytes (about 2.5 GB)                                                                |
| SHA-256       | 7485fe6f11af29433bc51cab58009521f205840f5b4ae3a32fa7f92e8534fdf5                               |
| Languages     | en, zh, es, fr, de, ja, ko, pt, ru, ar                                                         |
| Where it runs | runs locally (llama.cpp, on the user's computer)                                               |

## What Coco Voice uses it for

Coco Voice uses it for optional transcript cleanup: when the user corrects themselves while speaking ("scratch that", "I mean"), it keeps only what they meant. It does not remove filler words, fix spelling, or format lists. It is downloaded only when the user chooses it in Settings, and the download is checked against the SHA-256 above.

## Source of these values

Generated from src-tauri/src/catalog/catalog.json; if the catalog row changes, update this card.
