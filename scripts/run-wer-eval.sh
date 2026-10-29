#!/usr/bin/env bash
# Transcribe say-clips and score word error rate:
#   scripts/run-wer-eval.sh <clips_dir> <out.jsonl> <model id> [<model id> ...] [--totals-only]
# Does not build or download. The release binary must already exist, and each
# model must already be installed in the app.
set -euo pipefail

if [ "$#" -lt 3 ]; then
    echo "Usage: $0 <clips_dir> <out.jsonl> <model id> [<model id> ...] [--totals-only]" >&2
    exit 1
fi

# Resolve against the caller's directory before moving to the repo root.
abs() { case "$1" in /*) printf '%s\n' "$1" ;; *) printf '%s/%s\n' "$PWD" "$1" ;; esac; }
clips_dir="$(abs "$1")"
out_jsonl="$(abs "$2")"
shift 2

totals_only=0
models=()
for arg in "$@"; do
    if [ "$arg" = "--totals-only" ]; then
        totals_only=1
    else
        models+=("$arg")
    fi
done

if [ "${#models[@]}" -eq 0 ]; then
    echo "Usage: $0 <clips_dir> <out.jsonl> <model id> [<model id> ...] [--totals-only]" >&2
    exit 1
fi

if ! command -v jq >/dev/null 2>&1; then
    echo "error: jq is missing (needed to read transcription JSON and write results)" >&2
    exit 1
fi

cd "$(dirname "$0")/.." || exit 1

if [ -n "${CARGO_TARGET_DIR:-}" ]; then
    bin="${CARGO_TARGET_DIR}/release/coco-voice"
else
    bin="src-tauri/target/release/coco-voice"
fi

if [ ! -f "$bin" ]; then
    echo "error: release binary not found at ${bin}" >&2
    echo "cargo build --release --manifest-path src-tauri/Cargo.toml" >&2
    exit 2
fi

if [ ! -d "$clips_dir" ]; then
    echo "error: clips directory not found: ${clips_dir}" >&2
    exit 1
fi

if ! list_json="$("$bin" --list-models --json)"; then
    echo "error: coco-voice --list-models --json failed" >&2
    exit 1
fi

for model in "${models[@]}"; do
    downloaded="$(printf '%s\n' "$list_json" | jq -r --arg id "$model" '[.[] | select(.id == $id) | .is_downloaded] | first // false')"
    if [ "$downloaded" != "true" ]; then
        echo "model ${model} is not installed; install it in the app first" >&2
        exit 3
    fi
done

: > "$out_jsonl"

wavs=()
while IFS= read -r wav; do
    if [ -n "$wav" ]; then
        wavs+=("$wav")
    fi
done < <(find "$clips_dir" -maxdepth 1 -type f -name '*.wav' | LC_ALL=C sort)

if [ "${#wavs[@]}" -gt 0 ]; then
    for model in "${models[@]}"; do
        for wav in "${wavs[@]}"; do
            base="$(basename "$wav")"
            case "$base" in
                *.noisy.wav)
                    variant="noisy"
                    clip="${base%.noisy.wav}"
                    ;;
                *.wav)
                    variant="clean"
                    clip="${base%.wav}"
                    ;;
                *)
                    continue
                    ;;
            esac
            txt="${clips_dir}/${clip}.txt"
            if [ ! -f "$txt" ]; then
                continue
            fi
            ref="$(cat "$txt")"
            ok=true
            hyp=""
            if stdout="$("$bin" --transcribe-file "$wav" --model "$model" --json)"; then
                if ! hyp="$(printf '%s\n' "$stdout" | jq -r .text)"; then
                    echo "error: could not read .text from transcription JSON for ${wav}" >&2
                    exit 1
                fi
            else
                ok=false
                hyp=""
            fi
            jq -nc \
                --arg clip "$clip" \
                --arg variant "$variant" \
                --arg model "$model" \
                --arg ref "$ref" \
                --arg hyp "$hyp" \
                --argjson ok "$ok" \
                '{clip:$clip,variant:$variant,model:$model,ref:$ref,hyp:$hyp,ok:$ok}' >> "$out_jsonl"
        done
    done
fi

if [ "$totals_only" -eq 1 ]; then
    bun scripts/wer.ts "$out_jsonl" --totals-only
else
    bun scripts/wer.ts "$out_jsonl"
fi
