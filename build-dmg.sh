#!/bin/bash
# Coco Voice desktop DMG build — cargo on PATH + ggml cmake-policy shim.
export PATH="$HOME/.cargo/bin:$PATH"
export CMAKE_POLICY_VERSION_MINIMUM=3.5
cd /Users/Rijul_Kalra/projects/coco-voice || exit 1
echo "=== env ==="
echo "cargo: $(cargo --version 2>&1)"
echo "bun:   $(bun --version 2>&1)"
echo "date:  $(date)"
echo "=== tauri build (dmg only, no updater artifacts) ==="
bun run tauri build --bundles dmg --config '{"bundle":{"createUpdaterArtifacts":false}}' 2>&1
echo "TAURI_EXIT=$?"
echo "=== resulting DMG(s) ==="
find src-tauri/target -name "*.dmg" 2>/dev/null
echo "BUILD_DONE"
