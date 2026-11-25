# Installing Coco Voice

Coco Voice ships native installers for **macOS** (Apple Silicon `aarch64` and Intel `x64`), **Windows x64**, and **Linux** (`x64` and `ARM64`) on [GitHub Releases](https://github.com/coco-research/Coco-Voice/releases/latest). Every release publishes `SHA256SUMS`, `latest.json`, minisign `.sig` updater signatures, and GitHub build provenance attestations.

macOS builds are hand-tested; **Windows and Linux builds are built by CI and have not yet been hand-tested.** These builds are not notarized. Verify your download against `SHA256SUMS` or GitHub build provenance before opening it.

---

## 1. macOS (Apple Silicon `aarch64` & Intel `x64`)

### Disk Image (`.dmg`)

1. Download the `0.20.2` disk image for your Mac from the [latest release](https://github.com/coco-research/Coco-Voice/releases/latest):
   - Apple Silicon (M1–M4): `Coco.Voice_0.20.2_aarch64.dmg`
   - Intel: `Coco.Voice_0.20.2_x64.dmg`
   - Also download `SHA256SUMS` to verify the file.
2. Verify the download (see [Verify your download](#verify-your-download) below).
3. Open the `.dmg` and drag **Coco Voice** into **Applications**.
4. **First launch:** Because these builds are not notarized by Apple, right-click **Coco Voice** in `/Applications` → **Open** → **Open** (or choose **Open Anyway** in **System Settings → Privacy & Security**).
5. Grant **Microphone** and **Accessibility** when prompted (or choose **Continue without typing** to use clipboard-only mode). On macOS 14.4+, starting a meeting also prompts once for **System Audio Recording Only** (requested when the app opens the Core Audio process tap, although meetings in `0.20.2` record the microphone only — see [Known issues](README.md#known-issues)), and opting into the **Coming up** calendar card prompts for **Calendars** access.

---

## 2. Windows x64 (built by CI, not yet hand-tested)

> **Status:** Windows `0.20.2` installers are built by CI and have not yet been hand-tested.

### Per-User NSIS Setup (`.exe`) or MSI (`.msi`)

1. Download from the [latest release](https://github.com/coco-research/Coco-Voice/releases/latest):
   - Recommended per-user installer (no Administrator prompt, bundled ONNX Runtime): `Coco.Voice_0.20.2_x64-setup.exe`
   - MSI package: `Coco.Voice_0.20.2_x64_en-US.msi`
   - Checksums: `SHA256SUMS`
2. Verify the SHA-256 digest in **PowerShell**:
   ```powershell
   Get-FileHash .\Coco.Voice_0.20.2_x64-setup.exe -Algorithm SHA256
   ```
   Compare the hash with the matching line in `SHA256SUMS`.
3. Double-click `Coco.Voice_0.20.2_x64-setup.exe` to install.
4. **First launch SmartScreen prompt:** Because Windows builds are not Authenticode-signed, Windows Defender SmartScreen may warn on first run. After verifying the checksum, click **More info** → **Run anyway**.
5. **Updates & Portable Mode:** Standard installs verify minisign signatures on `latest.json` and update in place. Portable Mode installs store data in `Data\` next to the executable and update manually from GitHub Releases.

---

## 3. Linux (`x64` & `ARM64`, built by CI, not yet hand-tested)

> **Status:** Linux `0.20.2` packages are built by CI and have not yet been hand-tested.

All Linux packages bundle `libtranscribe.so` and the `ggml` runtime modules.

1. Download the package for your architecture and distribution from the [latest release](https://github.com/coco-research/Coco-Voice/releases/latest) along with `SHA256SUMS`:
   - **x64 (`amd64` / `x86_64`):** `Coco.Voice_0.20.2_amd64.AppImage`, `Coco.Voice_0.20.2_amd64.deb`, or `Coco.Voice-0.20.2-1.x86_64.rpm`
   - **ARM64 (`aarch64` / `arm64`):** `Coco.Voice_0.20.2_aarch64.AppImage`, `Coco.Voice_0.20.2_arm64.deb`, or `Coco.Voice-0.20.2-1.aarch64.rpm`
2. Verify the checksum:
   ```bash
   sha256sum -c --ignore-missing SHA256SUMS
   ```
3. Install or launch:
   - **AppImage:**
     ```bash
     chmod +x Coco.Voice_0.20.2_amd64.AppImage
     ./Coco.Voice_0.20.2_amd64.AppImage
     ```
     _(On Ubuntu 24.04+, install `libfuse2t64` if FUSE mounting is required.)_
   - **Debian / Ubuntu (`.deb`):**
     ```bash
     sudo apt install ./Coco.Voice_0.20.2_amd64.deb
     ```
   - **Fedora / openSUSE / RHEL (`.rpm`):**
     ```bash
     sudo dnf install ./Coco.Voice-0.20.2-1.x86_64.rpm
     ```
4. **Wayland and overlay notes:** Direct text input on Wayland uses `wtype`, `ydotool`, `dotool`, or `xdotool`. Set `COCO_VOICE_NO_GTK_LAYER_SHELL=1` if your desktop environment does not support `gtk-layer-shell` (the legacy environment variable name continues to work as a fallback).

---

## Verify your download

### 1. Checksum verification (`SHA256SUMS`)

- **macOS:**
  ```bash
  shasum -a 256 -c --ignore-missing SHA256SUMS
  ```
- **Linux:**
  ```bash
  sha256sum -c --ignore-missing SHA256SUMS
  ```
- **Windows (PowerShell):**
  ```powershell
  Get-FileHash .\Coco.Voice_0.20.2_x64-setup.exe -Algorithm SHA256
  ```

### 2. Build provenance attestation (`gh attestation verify`)

Every release signs build provenance for the `SHA256SUMS` subjects using GitHub Artifact Attestations. With the [GitHub CLI](https://cli.github.com/) installed:

```bash
gh attestation verify Coco.Voice_0.20.2_aarch64.dmg --repo coco-research/Coco-Voice
```

Replace the filename with whichever macOS, Windows, or Linux asset you downloaded.

---

## Data paths and uninstalling

### Where Coco Voice stores data

| Platform    | Settings, history & meetings                                                          | Logs                                                                                | Downloaded GGUF models (shared cache)                         |
| ----------- | ------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------- | ------------------------------------------------------------- |
| **macOS**   | `~/Library/Application Support/com.cocoresearch.cocovoice/`                           | `~/Library/Logs/com.cocoresearch.cocovoice/`                                        | `~/.cache/huggingface/hub/` (or `$HF_HOME/hub/`)              |
| **Windows** | `%APPDATA%\com.cocoresearch.cocovoice\` _(or `<install-dir>\Data\` in portable mode)_ | `%LOCALAPPDATA%\com.cocoresearch.cocovoice\logs\` _(or `<install-dir>\Data\logs\`)_ | `%USERPROFILE%\.cache\huggingface\hub\` (or `%HF_HOME%\hub\`) |
| **Linux**   | `~/.local/share/com.cocoresearch.cocovoice/` _(respects `$XDG_DATA_HOME`)_            | `~/.local/share/com.cocoresearch.cocovoice/logs/`                                   | `~/.cache/huggingface/hub/` (or `$HF_HOME/hub/`)              |

### Uninstalling

- **macOS:** Quit **Coco Voice**, move `/Applications/Coco Voice.app` to the Trash, and remove `~/Library/Application Support/com.cocoresearch.cocovoice/` and `~/Library/Logs/com.cocoresearch.cocovoice/` if you want to wipe local settings, history, and meetings.
- **Windows:** Uninstall **Coco Voice** via **Settings → Apps → Installed apps** (or delete the portable folder), then remove `%APPDATA%\com.cocoresearch.cocovoice\` and `%LOCALAPPDATA%\com.cocoresearch.cocovoice\` if desired.
- **Linux:** Delete the `.AppImage` or uninstall via `sudo apt remove coco-voice` / `sudo dnf remove coco-voice`, then remove `~/.local/share/com.cocoresearch.cocovoice/` if desired.
- **Optional model cleanup (all OSes):** Remove downloaded GGUF repos from `~/.cache/huggingface/hub/` if no other local tool uses them. Note: deleting a model inside Coco Voice also removes it from the shared Hugging Face cache.
