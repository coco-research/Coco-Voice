<h1 align="center">Coco Voice</h1>

<p align="center">
  Desktop dictation for people who want speech typed into the app they already
  have open.
</p>

<p align="center">
  <a href="LICENSE"
    ><img
      alt="Licence: MIT"
      src="https://img.shields.io/badge/licence-MIT-5F5F58?style=flat-square"
  /></a>
  <a href="https://github.com/coco-research/Coco-Voice/releases/latest"
    ><img
      alt="Latest release"
      src="https://img.shields.io/github/v/release/coco-research/Coco-Voice?color=5F5F58&style=flat-square"
  /></a>
  <img
    alt="Ships for macOS Apple Silicon"
    src="https://img.shields.io/badge/platform-macOS%20Apple%20Silicon-5F5F58?style=flat-square"
  />
</p>

## Why it exists

Talking is faster than typing, but most dictation tools record you, send the audio to a server, and bill you monthly. Coco Voice runs the speech model on your own computer. Hold a key, speak, and the text is pasted into whatever field you are in: a chat, an editor, an email. No account, no subscription, and recognition works offline once your model is downloaded.

## Quick start

### For users

Release 0.9.6 ships installers for macOS, Windows, and Linux. macOS: `Coco.Voice_0.9.6_aarch64.dmg` (Apple Silicon) and `Coco.Voice_0.9.6_x64.dmg` (Intel). Windows: x64 and arm64 installers (`-setup.exe` and `.msi`). Linux: an AppImage, a `.deb`, and an `.rpm` for x86_64/amd64 and aarch64/arm64. Builds are ad-hoc signed and not notarized, so Gatekeeper warns the first time you open the Mac app. Updater artifacts carry `.sig` files and `latest.json`. There is also a `SHA256SUMS` file. On a Mac, [INSTALL.md](INSTALL.md) covers the one-time quarantine step.

On an Apple Silicon Mac, one command downloads the latest release, copies Coco Voice into Applications, clears the quarantine flag, and launches it:

```bash
curl -fsSL https://raw.githubusercontent.com/coco-research/Coco-Voice/main/install.sh | bash
```

Or by hand on a Mac:

1. Open the `.dmg` and drag **Coco Voice** into **Applications**.
2. Clear quarantine, then launch:

```bash
xattr -dr com.apple.quarantine "/Applications/Coco Voice.app"
open "/Applications/Coco Voice.app"
```

On first launch macOS asks for the microphone and for Accessibility. Accessibility is what lets the global shortcut and the paste keystroke work. After that, the app asks you to download a speech model. Nothing is transcribed until that download finishes. The default shortcut is hold Option+Space (push-to-talk). After an update, Accessibility may need to be granted again.

### Build from source

You need Rust (stable), [Bun](https://bun.sh/), and cmake.

On macOS, install full Xcode and point the active developer directory at it (`sudo xcode-select -s /Applications/Xcode.app`). With only Command Line Tools, the Apple Intelligence bridge compiles as a stub. The published 0.9.5 disk image was built that way, so Apple Intelligence is unavailable in that download.

Windows also needs the Microsoft C++ build tools and the Vulkan SDK. Linux needs the libraries listed in [BUILD.md](BUILD.md), including cmake. Intel Macs have no prebuilt ONNX Runtime in this repo. [BUILD.md](BUILD.md) has the Homebrew link flags.

```bash
bun install

mkdir -p src-tauri/resources/models
curl -o src-tauri/resources/models/silero_vad_v4.onnx https://blob.handy.computer/silero_vad_v4.onnx

bun run tauri dev
```

If cmake stops on a policy error:

```bash
CMAKE_POLICY_VERSION_MINIMUM=3.5 bun run tauri dev
```

## What it does

- Hold a key to dictate, and the text lands in the focused app. Push-to-talk is the default (Option+Space on macOS, Ctrl+Space on Windows and Linux). Turn it off to toggle, or drive a running copy with `--toggle-transcription`.
- You pick a local model: Whisper, Parakeet, Moonshine, and other architectures, through transcribe-cpp, with older ONNX models through transcribe-rs.
- transcribe.cpp uses the GPU when one is there: Metal on macOS, Vulkan on Windows x64 and Linux, and the CPU otherwise. A Windows on ARM build of the speech engine is CPU only.
- You can clean a transcript afterwards through an OpenAI-compatible API, or through Apple Intelligence on Apple Silicon when that provider was compiled in. The 0.9.5 download does not include Apple Intelligence because the provider is a stub.
- Names and fixes stay local: custom words, whole-word corrections, and a transcript history stored in a database on disk.
- The settings window is translated into 22 languages.

## How it works

The shortcut opens the microphone through cpal. Silero voice activity detection drops silence. A local model turns the speech into text: transcribe-cpp for the GGUF catalog, transcribe-rs for an older ONNX model. Cleanup is separate, and it runs only when that step is enabled and the chosen provider is actually available. The text is pasted into whichever app is focused.

## Status and roadmap

This is a beta. The latest release is 0.9.6. See [CHANGELOG.md](CHANGELOG.md) for what changed. Today, on an Apple Silicon Mac, you can hold the shortcut, speak, and paste a local transcript, with history, custom words, and corrections. The app includes an updater that reads the latest GitHub release. The Mac build is ad-hoc signed and not notarized. Apple Intelligence is not in the 0.9.5 download.

Next:

1. A Coco-hosted model mirror, with sha256 checks on each download. [Pull request #2](https://github.com/coco-research/Coco-Voice/pull/2) is open.
2. Per-app profiles, plus cleanup that stays on the device with a local model. [Pull request #3](https://github.com/coco-research/Coco-Voice/pull/3) is open (still a draft).
3. Meeting mode: long-form capture and summaries on the device. Planned. There is no pull request yet.

## Models and licences

The application is [MIT](LICENSE). Each speech model keeps its own licence, and Coco Voice only offers models whose licences allow it. These were removed in 0.9.5 and won't load even if you already downloaded them. If you used one, pick another model in Settings > Models.

- **Held for licence review (custom licences)**: Parakeet Unified EN 0.6B, Nemotron Streaming 3.5, Nemotron Speech Streaming EN, MedASR, SenseVoice Small.
- **Non-commercial licences**: Canary 1B, Moonshine Tiny (Vietnamese), Moonshine Tiny (Ukrainian), Moonshine Tiny (Korean), Moonshine Tiny (Chinese), Moonshine Tiny (Arabic), Moonshine Tiny (Japanese), Moonshine Base (Vietnamese), Moonshine Base (Ukrainian), Moonshine Base (Korean), Moonshine Base (Chinese), Moonshine Base (Arabic), Moonshine Base (Japanese).
- **Built-in model removed**: SenseVoice (`sense-voice-int8`).

Licence details: [NOTICE](NOTICE).

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) and [CONTRIBUTING_TRANSLATIONS.md](CONTRIBUTING_TRANSLATIONS.md).

Coco Voice is in a feature freeze. A pull request for a new feature the community has not asked for is rejected. If the community has asked for it, or you have gathered support for it, it may still be considered. Bug fixes come first. The freeze is spelled out in CONTRIBUTING.md.

## Licence

[MIT](LICENSE).

## Credits

Coco Voice is a fork of [Handy](https://github.com/cjpais/Handy) by CJ Pais, which is also MIT. Handy is the speech pipeline this app still runs: recording, local models, shortcuts, and paste. Thank you to CJ Pais and the Handy contributors.

On top of that fork, Coco Voice changed the name, the parrot mark, and the blue theme. It added the macOS install helper and an updater signed for this repository. On macOS, after updating, Accessibility may need to be granted again. It also added Apple Intelligence as an on-device cleanup provider when the app is built with full Xcode, and a speak-a-correction shortcut that replaces the last dictation, including corrections learned from edits you make in history.

Key libraries: [transcribe-cpp](https://github.com/handy-computer/transcribe.cpp), transcribe-rs, Silero VAD, and [Tauri](https://tauri.app/).

## More

### Platforms

| Where you run it | What the code does                                                                                                                            |
| ---------------- | --------------------------------------------------------------------------------------------------------------------------------------------- |
| macOS            | Apple Silicon and Intel disk images. transcribe.cpp uses Metal. First launch asks for the microphone and Accessibility.                       |
| Windows x64      | An x64 `-setup.exe` and `.msi`. transcribe.cpp uses Vulkan.                                                                                   |
| Windows on ARM   | An arm64 `-setup.exe` and `.msi`. The speech engine is CPU-only.                                                                              |
| Linux            | An AppImage, a `.deb`, and an `.rpm` for x86_64/amd64 and aarch64/arm64. transcribe.cpp uses Vulkan. The recording overlay is off by default. |

On Linux, `HANDY_NO_GTK_LAYER_SHELL=1` skips the GTK layer-shell overlay. Wayland paste depends on a typing tool such as `wtype`, `ydotool`, `dotool`, or `xdotool`. Install notes and the long build troubleshooting (Windows path length, AppImage, Intel Mac ONNX Runtime) are in [BUILD.md](BUILD.md).

### Debug

`Cmd+Shift+D` on macOS, or `Ctrl+Shift+D` on Windows and Linux, toggles debug mode.

### Command line

These flags apply to this launch only. They do not change saved settings. If the app is already running, `--toggle-transcription`, `--toggle-post-process`, and `--cancel` are forwarded to it and the new process exits. Any other second launch brings the settings window forward.

```bash
coco-voice --toggle-transcription
coco-voice --toggle-post-process
coco-voice --cancel
coco-voice --start-hidden
coco-voice --no-tray
coco-voice --debug
```

`--transcribe-file` transcribes a 16 kHz mono WAV with a model that is already installed. It does not open the microphone, run voice activity detection, or download anything. `--json` prints that result as JSON. `--list-models` and `--list-devices` print what this build can load.
