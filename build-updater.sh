#!/bin/bash
# Coco Voice — signed updater build (macOS arm64, ad-hoc).
# Produces the DMG (for one-time manual install) plus the updater artifacts
# (.app.tar.gz + .sig) that the Tauri updater downloads and verifies.
#
# The private signing key lives OUTSIDE the repo at ~/.tauri and is loaded by
# PATH at runtime — this script contains no secret material and is safe to commit.
set -euo pipefail

export PATH="$HOME/.cargo/bin:$PATH"
export CMAKE_POLICY_VERSION_MINIMUM=3.5

KEY_PATH="$HOME/.tauri/coco-voice-updater.key"
if [ ! -f "$KEY_PATH" ]; then
  echo "FATAL: signing key not found at $KEY_PATH" >&2
  exit 1
fi
export TAURI_SIGNING_PRIVATE_KEY="$(cat "$KEY_PATH")"
PASS_PATH="$HOME/.tauri/coco-voice-updater.pass"
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD="$(cat "$PASS_PATH")"

cd "$HOME/projects/coco-voice"

echo "=== env ==="
echo "cargo: $(cargo --version 2>&1)"
echo "bun:   $(bun --version 2>&1)"
echo "key loaded: $([ -n "${TAURI_SIGNING_PRIVATE_KEY:-}" ] && echo yes || echo NO)"
echo "date:  $(date)"

echo "=== tauri build (app + dmg, updater artifacts signed) ==="
bun run tauri build --bundles app,dmg 2>&1
echo "TAURI_EXIT=$?"

echo "=== artifacts ==="
find src-tauri/target -name "*.dmg" -o -name "*.app.tar.gz" -o -name "*.app.tar.gz.sig" 2>/dev/null
echo "BUILD_DONE"
