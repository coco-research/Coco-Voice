#!/bin/bash
# Coco Voice installer for macOS (Apple Silicon).
#
# Downloads the latest signed release, installs it to /Applications, clears the
# Gatekeeper quarantine flag, and launches it. You only need this once — after
# the first install, Coco Voice updates itself silently.
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/coco-research/Coco-Voice/main/install.sh | bash
# or, if you already downloaded this file:
#   bash install.sh
set -euo pipefail

APP_NAME="Coco Voice"
REPO="coco-research/Coco-Voice"
APP_PATH="/Applications/${APP_NAME}.app"

echo "==> Finding the latest Coco Voice release…"
DMG_URL=$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" \
  | grep -o '"browser_download_url": *"[^"]*\.dmg"' \
  | head -1 \
  | sed 's/.*"browser_download_url": *"//; s/"$//')

if [ -z "${DMG_URL}" ]; then
  echo "ERROR: could not find a .dmg in the latest release of ${REPO}." >&2
  exit 1
fi
echo "    ${DMG_URL}"

WORK_DIR="$(mktemp -d)"
TMP_DMG="${WORK_DIR}/CocoVoice.dmg"
MOUNT_DIR="${WORK_DIR}/mnt"
mkdir -p "${MOUNT_DIR}"

cleanup() {
  hdiutil detach "${MOUNT_DIR}" -quiet 2>/dev/null || true
  rm -rf "${WORK_DIR}" 2>/dev/null || true
}
trap cleanup EXIT

echo "==> Downloading…"
curl -fL# "${DMG_URL}" -o "${TMP_DMG}"

echo "==> Mounting disk image…"
hdiutil attach "${TMP_DMG}" -nobrowse -quiet -mountpoint "${MOUNT_DIR}"

echo "==> Installing to /Applications…"
# Quit any running copy so the bundle can be replaced cleanly.
osascript -e "quit app \"${APP_NAME}\"" 2>/dev/null || true
rm -rf "${APP_PATH}"
cp -R "${MOUNT_DIR}/${APP_NAME}.app" "/Applications/"

echo "==> Clearing the Gatekeeper quarantine flag…"
xattr -dr com.apple.quarantine "${APP_PATH}" 2>/dev/null || true

echo "==> Launching Coco Voice…"
open "${APP_PATH}"

echo ""
echo "Done. Coco Voice is installed in /Applications."
echo "Future releases will update automatically from inside the app."
