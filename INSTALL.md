# Installing Coco Voice

Coco Voice ships native installers for **macOS** (`aarch64` Apple Silicon and `x64` Intel), **Windows x64**, and **Linux x64** on the [GitHub Releases](https://github.com/coco-research/Coco-Voice/releases/latest) page. Every release includes `SHA256SUMS`, `latest.json`, and detached minisign `.sig` files used by the built-in updater.

---

## 1. macOS (Apple Silicon `aarch64` & Intel `x64`)

### Recommended: Disk Image (`.dmg`)

1. Download `Coco.Voice_<version>_aarch64.dmg` (Apple Silicon M1/M2/M3/M4) or `Coco.Voice_<version>_x64.dmg` (Intel) and `SHA256SUMS` from the [latest release](https://github.com/coco-research/Coco-Voice/releases/latest).
2. _(Optional)_ Verify the download checksum in Terminal:
   ```bash
   shasum -a 256 -c --ignore-missing SHA256SUMS
   ```
3. Open the `.dmg` and drag **Coco Voice** into **Applications**.
4. **First launch only:** Because macOS builds are signed with the fixed self-signed `Coco Research Code Signing` certificate (without paid Apple notarization), right-click **Coco Voice** in `/Applications` → **Open** → **Open** (or choose **Open Anyway** under **System Settings → Privacy & Security**).
5. Grant **Microphone** and **Accessibility** permissions when prompted, then pick a local speech model. Subsequent in-app updates verify minisign signatures automatically, and the fixed signing identity preserves your Accessibility grant across updates.

---

## 2. Windows x64

### Per-User NSIS Setup (`.exe`) or MSI

1. Download `Coco.Voice_<version>_x64-setup.exe` (recommended per-user installer — requires **no Administrator prompt** and bundles the SHA-256-pinned ONNX Runtime) or `Coco.Voice_<version>_x64_en-US.msi`, plus `SHA256SUMS`, from the [latest release](https://github.com/coco-research/Coco-Voice/releases/latest).
2. Verify the installer SHA-256 in **PowerShell** against `SHA256SUMS`:
   ```powershell
   Get-FileHash .\Coco.Voice_*_x64-setup.exe -Algorithm SHA256
   ```
3. Double-click `Coco.Voice_<version>_x64-setup.exe` to install.
4. **First launch SmartScreen note:** Until Windows Authenticode signing is enabled, Windows Defender SmartScreen may display an unrecognized-app banner on the first run of a newly downloaded installer. After confirming the `Get-FileHash` digest matches `SHA256SUMS`, click **More info** → **Run anyway**.
5. **Updates & Portable Mode:** Standard installs check `latest.json` and apply minisign-verified updates in one click. If you select **Portable Mode** in the NSIS wizard, automatic in-place binary replacement is disabled; download the updated installer from GitHub Releases and verify its SHA-256 via `Get-FileHash`.

---

## 3. Linux x64

### AppImage, Debian/Ubuntu (`.deb`), or Fedora/RHEL (`.rpm`)

All Linux x64 bundles ship with `libtranscribe.so` and `libggml*.so` pre-staged inside the package.

1. Download the bundle for your distribution along with `SHA256SUMS` from the [latest release](https://github.com/coco-research/Coco-Voice/releases/latest) and verify its digest:
   ```bash
   sha256sum -c --ignore-missing SHA256SUMS
   ```
2. Install or launch:
   - **AppImage (`amd64`):**
     ```bash
     chmod +x Coco.Voice_*_amd64.AppImage
     ./Coco.Voice_*_amd64.AppImage
     ```
     _(On Ubuntu 24.04+, ensure `libfuse2t64` is installed if AppImage FUSE mounting is required.)_
   - **Debian / Ubuntu (`amd64` `.deb`):**
     ```bash
     sudo apt install ./Coco.Voice_*_amd64.deb
     ```
   - **Fedora / openSUSE / RHEL (`x86_64` `.rpm`):**
     ```bash
     sudo dnf install ./Coco.Voice-*-1.x86_64.rpm
     ```
3. **Wayland / Overlay notes:** On Wayland compositors, direct text paste uses `wtype`, `ydotool`, `dotool`, or `xdotool`. Set `HANDY_NO_GTK_LAYER_SHELL=1` if your desktop environment does not support `gtk-layer-shell`. AppImage and package installs receive minisign-verified updates from `latest.json`.
