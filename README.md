<div align="center">

# Coco Voice

Private dictation and meeting notes that run on your computer. By [Coco Research](https://cocoresearch.org/).

[**Download latest release (v0.20.2)**](https://github.com/coco-research/Coco-Voice/releases/latest)

</div>

<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="assets/branding/banner-dark.png" />
    <source media="(prefers-color-scheme: light)" srcset="assets/branding/banner-light.png" />
    <img
      src="assets/branding/banner-dark.png"
      width="100%"
      alt="Coco Voice v0.20.2 by Coco Research — private dictation on macOS, Windows and Linux, and meeting notes on macOS."
    />
  </picture>
</p>

<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/readme/0.20/H1-dark.png" />
    <source media="(prefers-color-scheme: light)" srcset="docs/readme/0.20/H1-light.png" />
    <img
      src="docs/readme/0.20/H1-dark.png"
      width="100%"
      alt="Coco Voice 0.20.2 Home screen with the floating recording pill dictating into a plain text editor."
    />
  </picture>
</p>

<p align="center">
  <a href="https://github.com/coco-research/Coco-Voice/releases/latest"
    ><img
      alt="Release"
      src="https://img.shields.io/github/v/release/coco-research/Coco-Voice?style=flat-square&label=release&labelColor=0B0B0B&color=5b8dff"
  /></a>
  <img
    alt="Platforms: macOS, Windows x64, Linux x64"
    src="https://img.shields.io/badge/platforms-macOS%20%C2%B7%20Windows%20x64%20%C2%B7%20Linux%20x64-5F5F58?style=flat-square&labelColor=0B0B0B"
  />
  <a href="LICENSE"
    ><img
      alt="License: MIT"
      src="https://img.shields.io/badge/license-MIT-5F5F58?style=flat-square&labelColor=0B0B0B"
  /></a>
  <img
    alt="Runs on-device"
    src="https://img.shields.io/badge/runs-on--device-5b8dff?style=flat-square&labelColor=0B0B0B"
  />
</p>

## Why it exists

Speaking is easier than typing, yet most dictation tools send your voice to a remote server and lock your words behind an account. Coco Voice runs speech recognition locally on your own machine and pastes straight into whichever app is in focus. On macOS, it also records meetings with your explicit consent and builds searchable notes, citations, and to-dos without leaving your Mac.

## See it

### Dictate anywhere

Hold `Option+Space` on macOS (`Ctrl+Space` on Windows and Linux), speak, and release to type into any focused text field. A compact floating pill tracks listening and displays a "Still working" state while transcription finishes so a second press never drops your take.

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/readme/0.20/V1-dark.png" />
  <img src="docs/readme/0.20/V1-dark.png" width="100%" alt="Coco Voice recording pill showing listening and Still working states while dictating into a text field." />
</picture>

### Dictation history and search

Every dictation take can be kept locally on disk in SQLite with instant full-text search and one-click copy. Choose how long to keep dictations and whether to keep take audio, or turn history storage off completely.

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/readme/0.20/V2-dark.png" />
  <img src="docs/readme/0.20/V2-dark.png" width="100%" alt="Dictation history screen with search filtering matching takes." />
</picture>

### Your words, your fixes

Save custom names, technical terms, and whole-word replacements so spellings stay consistent across takes. Spoken corrections like "scratch that" or "no wait" can be removed automatically when clean-up is enabled.

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/readme/0.20/V3-dark.png" />
  <img src="docs/readme/0.20/V3-dark.png" width="100%" alt="Words and clean-up screen showing custom vocabulary entries and whole-word corrections." />
</picture>

### Per-app profiles

Set per-app rules for tone, capitalization, punctuation, and clean-up level depending on the frontmost app. Add sensitive apps or browser domains to **Never dictate in** so the dictation shortcut does nothing there.

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/readme/0.20/V4-dark.png" />
  <img src="docs/readme/0.20/V4-dark.png" width="100%" alt="Per-app profiles screen showing tailored dictation rules for TextEdit and Terminal alongside Never dictate in." />
</picture>

### Choose your model, on your machine

Pick from Whisper, Parakeet, Moonshine, Canary, Cohere, and other local models with Metal GPU acceleration on macOS and Vulkan on Windows x64 and Linux. Every catalog download is pinned to a commit, verified by SHA-256, and surfaces clear failure causes with **Try again**.

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/readme/0.20/V5-dark.png" />
  <img src="docs/readme/0.20/V5-dark.png" width="100%" alt="Models screen listing an installed local speech model and downloadable catalog models with sizes and licences." />
</picture>

### Optional clean-up

Keep raw dictation or enable optional post-processing using Apple Intelligence (Apple Silicon builds compiled with full Xcode), a local GGUF model, or an OpenAI-compatible API you configure yourself. Clean-up only runs where you choose and never silently switches to a cloud provider.

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/readme/0.20/V6-dark.png" />
  <img src="docs/readme/0.20/V6-dark.png" width="100%" alt="Clean-up settings showing on-device and optional OpenAI-compatible provider choices with no API key required." />
</picture>

### Agents can ask, opt-in

Off by default: enable **Agents can ask you** to expose a local stdio `ask_user` bridge (`coco-voice --mcp-ask-user`) over a user-private Unix socket. Local coding agents can display a short question in the recording pill and receive your spoken answer directly without touching your clipboard.

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/readme/0.20/V7-dark.png" />
  <img src="docs/readme/0.20/V7-dark.png" width="100%" alt="Agents can ask you card in Settings showing the opt-in toggle and local stdio command." />
</picture>

### Meetings (macOS): record with consent

On macOS release builds (macOS 14.4+), Coco Voice captures your microphone and system audio locally without joining calls as a bot. Recording never starts automatically: every meeting starts only after you confirm the consent dialog, and Google Meet offers appear only on active meeting-code tabs.

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/readme/0.20/M1-dark.png" />
  <img src="docs/readme/0.20/M1-dark.png" width="100%" alt="Consent reminder dialog shown before starting a local meeting recording on macOS." />
</picture>

### Meetings (macOS): notes with citations

Take live notepad bullets while recording, then generate structured notes with clickable citations back to transcript segments, in one shot or across pieces for longer meetings. Full-text search spans all saved meetings, and deleting a meeting removes its notes and audio while reporting any file cleanup failure.

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/readme/0.20/M2-dark.png" />
  <source media="(prefers-color-scheme: light)" srcset="docs/readme/0.20/M2-light.png" />
  <img src="docs/readme/0.20/M2-dark.png" width="100%" alt="Meeting notes view on macOS showing structured summary bullets linked to transcript segment citations." />
</picture>

### Meetings (macOS): ask your meetings

Ask questions across a single meeting or your entire local meeting archive using a local SQLite BM25 index and on-device model. Type a question or ask by voice, and jump straight to the cited transcript lines.

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/readme/0.20/M3-dark.png" />
  <img src="docs/readme/0.20/M3-dark.png" width="100%" alt="Ask your meetings view displaying a question and a cited local answer." />
</picture>

### Meetings (macOS): to-dos across meetings

Spoken follow-ups and action items are extracted locally and synced into a single cross-meeting to-dos list. Check off completed items and trace each to-do back to the meeting where it came up.

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/readme/0.20/M4-dark.png" />
  <img src="docs/readme/0.20/M4-dark.png" width="100%" alt="Cross-meeting to-dos list displaying action items extracted from local meeting notes." />
</picture>

### Meetings (macOS): coming up

Opt into reading chosen calendars on your Mac to see upcoming meetings in a Coming up card and launch consent-gated notes with one click. Calendar reads stay strictly local and read-only, and calendars marked as work are never queried.

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/readme/0.20/M5-dark.png" />
  <img src="docs/readme/0.20/M5-dark.png" width="100%" alt="Coming up meeting card showing an upcoming test calendar event ready to start notes." />
</picture>

### Trust: What goes online

Open **Settings → About → What goes online** to inspect every host the app can contact, when it contacts each host, and what leaves your machine. The interface is available in 22 languages and checks stale Accessibility entries automatically on macOS.

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/readme/0.20/T1-dark.png" />
  <img src="docs/readme/0.20/T1-dark.png" width="100%" alt="Settings About What goes online disclosure listing update checks, model downloads, and opt-in cleanup hosts." />
</picture>

## Install

Download installers for `0.20.2` from the [latest release](https://github.com/coco-research/Coco-Voice/releases/latest). These builds are not notarized. Full steps, uninstall paths, and checksum verification are in [INSTALL.md](INSTALL.md).

| Platform                          | 0.20.2 assets                                                                                                                                               | First launch                                                                                                                                                               |
| --------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **macOS** (Apple Silicon & Intel) | `Coco.Voice_0.20.2_aarch64.dmg`, `Coco.Voice_0.20.2_x64.dmg`                                                                                                | Drag **Coco Voice** to Applications, then right-click → **Open** → **Open** once. Grant Microphone and Accessibility when asked.                                           |
| **Windows x64**                   | `Coco.Voice_0.20.2_x64-setup.exe` (per-user NSIS), `Coco.Voice_0.20.2_x64_en-US.msi`                                                                        | Run the installer. If Windows SmartScreen appears, verify `SHA256SUMS` then click **More info** → **Run anyway**.                                                          |
| **Linux x64** (and ARM64)         | `Coco.Voice_0.20.2_amd64.AppImage`, `Coco.Voice_0.20.2_amd64.deb`, `Coco.Voice-0.20.2-1.x86_64.rpm` (ARM64: `aarch64.AppImage`, `arm64.deb`, `aarch64.rpm`) | `chmod +x` the AppImage or install with `apt` / `dnf`. Set `COCO_VOICE_NO_GTK_LAYER_SHELL=1` if your compositor lacks `gtk-layer-shell` (the legacy env name still works). |

### Verify your download

Every release publishes `SHA256SUMS`, minisign `.sig` files for the built-in updater, and GitHub build provenance attestations:

```bash
# macOS / Linux checksum check
shasum -a 256 -c --ignore-missing SHA256SUMS   # macOS
sha256sum -c --ignore-missing SHA256SUMS       # Linux

# Build provenance check (requires GitHub CLI)
gh attestation verify Coco.Voice_0.20.2_aarch64.dmg --repo coco-research/Coco-Voice
```

## Privacy

- **On-device by default:** Speech recognition, custom words, history, and macOS meeting transcripts/notes run locally on your computer.
- **No account, no telemetry:** Coco Voice requires no sign-in, ships zero analytics SDKs, and collects no usage telemetry or word counts (enforced by automated privacy guards in the test suite).
- **Transparent network hosts:** The only outbound requests are model downloads you trigger, optional GitHub release update checks (toggleable in Settings), and an optional cloud clean-up provider if you explicitly enter your own API key. See **Settings → About → What goes online** inside the app.

## FAQ

**Which model should I pick?**  
Onboarding downloads the default local speech model (`Parakeet Ultra`, ~706 MB) automatically. You can switch anytime in **Models** — try smaller Moonshine or Canary models for speed, or Whisper Medium / Cohere Transcribe for broader language coverage.

**Does it work offline?**  
Yes. Once your speech model (and optional local clean-up model) is downloaded, dictation and macOS meeting notes work completely offline.

**Why does macOS ask for Accessibility?**  
Accessibility lets Coco Voice listen for your global push-to-talk shortcut and send the paste keystroke into the focused app. You can also skip Accessibility during onboarding and use clipboard-only mode (**Don't type**). If a macOS update leaves a stale Accessibility entry, Coco Voice detects it and shows the exact reset steps.

**Meetings on Windows or Linux?**  
Not yet. Meetings currently ship in macOS release builds only (macOS 14.4+ for Core Audio process tap system-audio capture).

**How do updates work?**  
When **Check for updates** is enabled, Coco Voice checks GitHub Releases `latest.json` and verifies update archives with minisign before installing. Portable Windows installs check manually via GitHub Releases.

**Where is my data stored?**

- **macOS:** `~/Library/Application Support/com.cocoresearch.cocovoice/` (logs in `~/Library/Logs/com.cocoresearch.cocovoice/`)
- **Windows:** `%APPDATA%\com.cocoresearch.cocovoice\` (or `<install-dir>\Data\` in portable mode)
- **Linux:** `~/.local/share/com.cocoresearch.cocovoice/` (respecting `$XDG_DATA_HOME`)
- **Downloaded GGUF models (all OSes):** Shared Hugging Face cache (`$HF_HOME/hub`, default `~/.cache/huggingface/hub/`).

**How do I uninstall?**  
Remove the app binary/package and delete the data folder above (and optionally cached models under `~/.cache/huggingface/hub/`). Step-by-step instructions per OS are in [INSTALL.md](INSTALL.md#uninstalling).

## Build from source

You need Rust (stable), [Bun](https://bun.sh/), and `cmake`. Platform prerequisites, the required [Silero VAD model download step](BUILD.md#3-download-required-models), and Xcode/Vulkan/Linux packaging flags live in [BUILD.md](BUILD.md).

```bash
bun install
# Download the Silero VAD model as documented in BUILD.md, then run:
bun run tauri dev
```

<details>
<summary>Command line flags</summary>

Flags apply to the current launch only and do not overwrite saved settings. When Coco Voice is already running, `--toggle-transcription`, `--toggle-post-process`, and `--cancel` forward to the active instance and exit.

```bash
coco-voice --toggle-transcription
coco-voice --toggle-post-process
coco-voice --cancel
coco-voice --start-hidden
coco-voice --no-tray
coco-voice --debug
coco-voice --list-models --json
coco-voice --list-devices
coco-voice --transcribe-file clip.wav --model <model-id> --json
coco-voice --mcp-ask-user
coco-voice mcp   # macOS meetings builds only (opt-in stdio MCP)
```

`Cmd+Shift+D` on macOS (`Ctrl+Shift+D` on Windows and Linux) toggles the in-app **Debug** panel.

</details>

## Models and licences

Coco Voice itself is MIT-licensed. Downloaded speech and clean-up models are not bundled with the app and keep their own upstream licences; the catalog offers only models under MIT, Apache-2.0, or CC-BY-4.0. See [NOTICE](NOTICE) and [sbom/model-licences.md](sbom/model-licences.md) for the complete model attribution table and licences.

## Contributing

Bug reports, fixes, and translations are welcome. See [CONTRIBUTING.md](CONTRIBUTING.md), [CONTRIBUTING_TRANSLATIONS.md](CONTRIBUTING_TRANSLATIONS.md), or open an issue on [GitHub Issues](https://github.com/coco-research/Coco-Voice/issues).

## Security

To report a security vulnerability privately, see [SECURITY.md](SECURITY.md).

## Licence

[MIT](LICENSE).

## Credits

Coco Voice began from [Handy](https://github.com/cjpais/Handy) by CJ Pais (MIT); thank you to its contributors.

Built with transcribe.cpp, transcribe-rs, Silero VAD and [Tauri](https://tauri.app/).
