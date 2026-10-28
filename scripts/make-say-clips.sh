#!/usr/bin/env bash
# Speak known sentences into 16 kHz clips:
#   scripts/make-say-clips.sh <sentences.txt> <out_dir> [prefix]
# macOS only. One clip per non-empty line: <prefix><NNN>.wav,
# <prefix><NNN>.noisy.wav, and <prefix><NNN>.txt. Default prefix: c.
set -euo pipefail

if [ "$#" -lt 2 ] || [ "$#" -gt 3 ]; then
    echo "Usage: $0 <sentences.txt> <out_dir> [prefix]" >&2
    exit 1
fi

for cmd in say afconvert ffmpeg; do
    if ! command -v "$cmd" >/dev/null 2>&1; then
        echo "error: $cmd is missing (need macOS say, afconvert, and ffmpeg)" >&2
        exit 1
    fi
done

# Resolve against the caller's directory.
abs() { case "$1" in /*) printf '%s\n' "$1" ;; *) printf '%s/%s\n' "$PWD" "$1" ;; esac; }
sentences="$(abs "$1")"
out_dir="$(abs "$2")"
prefix="${3:-c}"

if [ ! -f "$sentences" ]; then
    echo "error: sentences file not found: $sentences" >&2
    exit 1
fi

mkdir -p "$out_dir"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

voices=(Samantha Daniel Karen Rishi)
count=0

while IFS= read -r line || [ -n "$line" ]; do
    if [ -z "${line//[[:space:]]/}" ]; then
        continue
    fi
    count=$((count + 1))
    nnn="$(printf '%03d' "$count")"
    voice="${voices[$(((count - 1) % 4))]}"
    stem="${prefix}${nnn}"
    aiff="${work}/${stem}.aiff"
    clean="${out_dir}/${stem}.wav"
    noisy="${out_dir}/${stem}.noisy.wav"

    say -v "$voice" -o "$aiff" "$line"
    afconvert -f WAVE -d 'LEI16@16000' -c 1 "$aiff" "$clean"
    ffmpeg -y -loglevel error -i "$clean" \
        -filter_complex "anoisesrc=color=white:amplitude=0.02:sample_rate=16000:seed=7[n];[0:a][n]amix=inputs=2:duration=first:normalize=0" \
        -ar 16000 -ac 1 "$noisy"
    printf '%s\n' "$line" > "${out_dir}/${stem}.txt"
done < "$sentences"

echo "wrote ${count} clips to ${out_dir}"
