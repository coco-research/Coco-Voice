<p align="center">
  <img src="assets/branding/coco-voice-banner.png" alt="Coco Voice" width="820">
</p>

<p align="center">
  <a href="LICENSE"><img alt="License: MIT" src="https://img.shields.io/badge/license-MIT-2251FF.svg?style=for-the-badge"></a>
  <img alt="Platforms" src="https://img.shields.io/badge/macOS%20%C2%B7%20Windows%20%C2%B7%20Linux-2251FF.svg?style=for-the-badge">
  <img alt="Built with Tauri" src="https://img.shields.io/badge/built%20with-Tauri-24C8DB.svg?style=for-the-badge&logo=tauri&logoColor=white">
  <a href="https://discord.com/invite/WVBeWsNXK4"><img alt="Discord" src="https://img.shields.io/badge/Discord-5865F2.svg?style=for-the-badge&logo=discord&logoColor=white"></a>
</p>

<p align="center">
  <b>Speak freely. Your computer listens. Nothing leaves your machine.</b>
</p>

Coco Voice is a free, open-source desktop app that transcribes your speech into any text field — completely offline. No cloud. No subscriptions. No privacy tradeoffs.

Press a keyboard shortcut, talk, and watch your words appear wherever you're typing.

---

## What Makes Coco Voice Different

Most speech-to-text tools send your audio to the cloud. Coco Voice runs **everything locally** — voice detection, speech recognition, and text output all happen on your machine.

- **Zero cloud dependency** — works offline, on airplanes, in secure environments
- **Zero latency** — no round-trip to a server, transcription starts instantly
- **Zero data collection** — your voice never leaves your computer, period
- **Zero cost** — free and open source, forever

---

## How It Works

```
1. Press shortcut → 2. Speak → 3. Release → 4. Text appears
```

The pipeline runs entirely on your hardware:

| Stage | Technology | What it does |
|-------|-----------|-------------|
| Voice detection | Silero VAD | Strips silence, only captures actual speech |
| Speech recognition | Whisper / Parakeet V3 | Converts audio to text using your GPU or CPU |
| Text output | System clipboard / keystroke simulation | Pastes directly into any app you're using |

---

## Model Options

Pick the engine that fits your hardware:

| Model | Best for | Speed | Hardware |
|-------|----------|-------|----------|
| **Parakeet V3** | Everyday use | ~5× real-time | CPU (any modern machine) |
| **Whisper Turbo** | Maximum accuracy | GPU-accelerated | NVIDIA, AMD, Intel GPU |
| **Whisper Large** | Highest quality | GPU required | Dedicated GPU recommended |
| **Whisper Small/Medium** | Balance | Moderate | Most machines |

Parakeet V3 auto-detects your language — no manual switching between English, Spanish, French, German, Japanese, and 90+ others.

---

## Built With Tauri

Coco Voice combines a **React/TypeScript frontend** with a **Rust backend** for native performance:

- **Audio capture** — cross-platform via `cpal`, with automatic mic selection
- **ML inference** — Whisper-family models via `transcribe-cpp` (GGML/GGUF), Parakeet via `transcribe-rs` (ONNX)
- **Global hotkeys** — system-wide keyboard shortcuts via `rdev` and `handy-keys`
- **Text input** — clipboard paste + direct keystroke simulation across macOS, Windows, Linux
- **Binary size** — ~20MB (no Electron bloat)

---

## Quick Start

```bash
# macOS
brew install --cask coco-voice

# Windows
winget install cocoresearch.CocoVoice

# Linux — download from releases
# See BUILD.md for distro-specific instructions
```

1. Launch Coco Voice
2. Grant microphone and accessibility permissions
3. Set your keyboard shortcut (default: `Option+Space` on macOS, `Ctrl+Space` on Windows/Linux)
4. Press the shortcut and start talking

---

## Power User Features

**CLI control** — trigger transcription from scripts, window managers, or automation:

```bash
coco-voice --toggle-transcription    # Remote: start/stop recording
coco-voice --toggle-post-process     # Remote: record with AI post-processing
coco-voice --cancel                  # Remote: cancel current operation
coco-voice --start-hidden --no-tray  # Headless mode for autostart
```

**Push-to-talk mode** — hold the shortcut to record, release to transcribe

**AI post-processing** — pipe transcriptions through OpenAI, Anthropic, or any OpenAI-compatible API to fix grammar, format text, or extract action items

**Debug mode** — `Cmd+Shift+D` (macOS) / `Ctrl+Shift+D` (Windows/Linux) opens live logs, path inspector, and GPU device listing

**Headless transcription** — transcribe WAV files from the terminal:

```bash
coco-voice --transcribe-file recording.wav --model whisper-large-v3-turbo --json
```

---

## Platform Support

| Platform | GPU Acceleration | Status |
|----------|-----------------|--------|
| macOS (Apple Silicon) | Metal | ✅ Fully supported |
| macOS (Intel) | Metal | ✅ Fully supported |
| Windows (x64) | Vulkan, CUDA | ✅ Fully supported |
| Linux (x64) | Vulkan | ✅ Supported (see Linux notes) |

---

## Contributing

Coco Voice is built to be forked. The codebase is designed to be readable and extensible — a well-patterned Tauri app you can learn from, modify, and build upon.

```bash
git clone https://github.com/coco-research/Coco-Voice.git
cd Coco-Voice
bun install
bun run tauri dev
```

See [BUILD.md](BUILD.md) for platform-specific prerequisites and [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.

---

## Architecture

```
┌─────────────────────────────────────┐
│  React Frontend (Settings UI)       │
│  TypeScript + Tailwind CSS          │
├─────────────────────────────────────┤
│  Tauri Bridge (IPC)                 │
├─────────────────────────────────────┤
│  Rust Backend                       │
│  ┌──────────┐ ┌──────────┐         │
│  │ Audio    │ │ ML       │         │
│  │ Capture  │ │ Engine   │         │
│  │ + VAD    │ │ Whisper  │         │
│  └──────────┘ │ Parakeet │         │
│  ┌──────────┐ └──────────┘         │
│  │ Hotkeys  │ ┌──────────┐         │
│  │ + Input  │ │ Text     │         │
│  │ Sim      │ │ Output   │         │
│  └──────────┘ └──────────┘         │
└─────────────────────────────────────┘
```

---

## License

MIT License — see [LICENSE](LICENSE) for details.

The Coco Voice name, logo, icon, and brand assets are not open-source. Unofficial forks and redistributions must use their own branding.

---

## Acknowledgments

- **Whisper** by OpenAI — speech recognition models
- **ggml / whisper.cpp** — cross-platform STT inference and acceleration
- **Silero** — lightweight VAD
- **Tauri** — the Rust-based app framework that makes this possible
