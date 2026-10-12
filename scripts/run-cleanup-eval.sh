#!/usr/bin/env bash
# Score the on-device cleanup model: scripts/run-cleanup-eval.sh <set.jsonl> [report.md]
# Paths resolve from the caller's directory; the default report is
# ./cleanup-eval-<timestamp>.md. macOS only, and Qwen3-4B must be downloaded in the app.
set -euo pipefail

if [ "$#" -lt 1 ]; then
    echo "Usage: $0 <set.jsonl> [report.md]" >&2
    exit 1
fi

# Resolve against the caller's directory before moving to the repo root.
abs() { case "$1" in /*) printf '%s\n' "$1" ;; *) printf '%s/%s\n' "$PWD" "$1" ;; esac; }
EVAL_SET="$(abs "$1")"
REPORT_PATH="$(abs "${2:-cleanup-eval-$(date +%Y-%m-%d-%H%M%S).md}")"

# Qwen/Qwen3-4B-GGUF at its pinned catalog revision, in the app's HF cache.
MODEL_PATH="${HF_HOME:-$HOME/.cache/huggingface}/hub/models--Qwen--Qwen3-4B-GGUF/snapshots/bc640142c66e1fdd12af0bd68f40445458f3869b/Qwen3-4B-Q4_K_M.gguf"
if [ ! -f "$MODEL_PATH" ]; then
    echo "Download Qwen3-4B in Settings > Post-processing first" >&2
    exit 2
fi

cd "$(dirname "$0")/.."

# Always rebuild so the eval never scores a stale binary.
cargo build --release --manifest-path src-tauri/Cargo.toml

exec "${CARGO_TARGET_DIR:-src-tauri/target}/release/coco-voice" \
    --cleanup-eval "$EVAL_SET" \
    --eval-model "$MODEL_PATH" \
    --eval-out "$REPORT_PATH"
