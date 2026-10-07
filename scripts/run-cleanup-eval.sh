#!/usr/bin/env bash
set -euo pipefail

# Run from the repo root so the relative paths below resolve.
cd "$(dirname "$0")/.."

if [ "$#" -lt 1 ]; then
    echo "Usage: $0 <set.jsonl> [report.md]"
    exit 1
fi

EVAL_SET="$(cd "$OLDPWD" && python3 -c 'import os,sys; print(os.path.abspath(sys.argv[1]))' "$1")"
REPORT_PATH="${2:-}"

if [ -z "$REPORT_PATH" ]; then
    REPORT_PATH="./cleanup-eval.md"
fi
REPORT_PATH="$(cd "$OLDPWD" && python3 -c 'import os,sys; print(os.path.abspath(sys.argv[1]))' "$REPORT_PATH")"

REVISION=$(python3 -c '
import json
with open("src-tauri/src/catalog/catalog.json") as f:
    data = json.load(f)
for m in data.get("models", []):
    if m.get("id") == "Qwen/Qwen3-4B-GGUF":
        print(m.get("revision", ""))
')

if [ -z "$REVISION" ]; then
    echo "Error: Could not find revision for Qwen/Qwen3-4B-GGUF in catalog.json"
    exit 1
fi

HF_CACHE="${HF_HOME:-$HOME/.cache/huggingface}"
MODEL_PATH="$HF_CACHE/hub/models--Qwen--Qwen3-4B-GGUF/snapshots/$REVISION/Qwen3-4B-Q4_K_M.gguf"

if [ ! -f "$MODEL_PATH" ]; then
    echo "Download Qwen3-4B in Settings > Post-processing first"
    exit 2
fi

TARGET_DIR="${CARGO_TARGET_DIR:-src-tauri/target}"
BIN_PATH="$TARGET_DIR/release/coco-voice"

echo "Building release binary..."
(cd src-tauri && cargo build --release)

exec "$BIN_PATH" \
    --cleanup-eval "$EVAL_SET" \
    --eval-model "$MODEL_PATH" \
    --eval-out "$REPORT_PATH"
