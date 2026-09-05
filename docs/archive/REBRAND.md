# Coco Voice — Rebrand Plan & Record

This document records the rebrand of this repository from **Handy** (its upstream origin) to **Coco Voice**, including the analysis that guided it, a summary of the code knowledge graph, the classification of every residual `handy` reference, and the concrete changes that were applied.

## Overview

Coco Voice is a fork of the open-source Handy speech-to-text application. It is a cross-platform desktop dictation tool built on Tauri 2 (a Rust backend under `src-tauri/` and a React + TypeScript frontend under `src/`), using `bun` as the package manager. Local transcription is performed by whisper.cpp (via the `transcribe-cpp` crate) and ONNX models (via `transcribe-rs`), with Silero voice-activity detection, global keyboard shortcuts, and a menu-bar/tray presence.

When this work began, the fork had already been renamed at the identity level: the package names, the Tauri product name and bundle identifier, the window title, the user-facing interface strings, the in-app SVG logo components, and the color palette all referred to Coco Voice. What remained were the **visual brand assets**, which were still entirely the Handy hand-mascot artwork, and the **color theme**, which was still the inherited pink.

## Code Knowledge Graph (Graphfy)

The repository was indexed with GitNexus to support structural and impact analysis during the rebrand. The resulting graph contains **3,189 nodes, 7,548 edges, 173 clusters, and 267 execution flows**. The large Silero VAD model file was correctly excluded from indexing as a vendored binary.

The graph confirmed the architecture described in `AGENTS.md`: a manager-based Rust backend (audio capture, model management, transcription pipeline, history) exposed to the frontend through Tauri commands and events, with the tray icon state machine (`tray.rs`) loading icon images from `src-tauri/resources/` at runtime and the settings store driving reactive UI updates. This runtime icon loading is why swapping the tray PNG files takes effect without recompiling the icon paths.

## Classification of Residual `handy` References

A central finding is that most remaining occurrences of the string `handy` are **load-bearing** and must not be renamed, because they refer to upstream infrastructure the application still depends on.

| Category | Examples | Action |
|----------|----------|--------|
| Load-bearing (must keep) | `handy-computer/*` Hugging Face repository IDs in `catalog.json`; `https://blob.handy.computer/*` model-download URLs in `model.rs`; the `handy-keys` crate; the `cjpais/tauri` fork branch `handy-2.10.2` in `Cargo.toml` | Preserved unchanged |
| Cosmetic / internal | `THEME_STORAGE_KEY = "handy.theme"`; the "Handy Portable Mode" marker string; `HANDY_*` environment-variable names; the generated Tauri command names `start`/`stop_handy_keys_recording` | Left in place (safe to revisit later) |
| False positives | `custom_words: ["handy"]` test data; the comment "still handy for composing" | Ignored (ordinary English) |

Renaming any load-bearing reference would break model downloads or the build, so the rebrand deliberately leaves them intact. The application was verified to still download and load a `handy-computer` model successfully after the rebrand.

## Changes Applied

### Visual identity

The Handy hand-mascot was replaced with an original **Coco parrot** mascot — a cockatoo drawn in the same bold-outline cartoon family, with a crest, hooked beak, and friendly face — chosen because a talking parrot ties together the brand name, the playful personality, and the app's speech function. The mascot artwork is authored as vector source in `assets/branding/`:

- `coco-parrot-1024.svg` — the full app icon (mascot on the brand background).
- `coco-parrot-glyph.svg` — the transparent mascot used for the colored tray icon.
- `coco-parrot-mono.svg` — a single-color silhouette used for the monochrome menu-bar glyphs.
- `coco-voice-banner.png` — the README hero banner (mascot, wordmark, and tagline).

The full operating-system icon set under `src-tauri/icons/` (PNG sizes, `icon.icns`, `icon.ico`, and the Android, iOS, and Windows tile variants) was regenerated from the master artwork with `bun run tauri icon`. The tray and menu-bar assets under `src-tauri/resources/` were regenerated as well: the idle mascot (`coco-voice.png`, `tray_idle.png`, `tray_idle_dark.png`), and the recording ("ear") and transcribing ("brain") state glyphs, which were recolored to the new brand color while preserving their metaphors.

### Color theme

The theme was changed from the inherited pink to **electric blue (`#2251FF`) and white**, with a deep-navy dark background (`#051C2C`). Because the palette is centralized in `src/styles/theme.css` as a single source of truth, the change was surgical:

- `theme.css` tokens for the logo primary, logo stroke, dark background, and UI accent were repointed to the electric-blue and navy values.
- Hardcoded pink literals (`#FAA2CA`) in `AudioPlayer.tsx`, `CancelIcon.tsx`, `TranscriptionIcon.tsx`, and `MicrophoneIcon.tsx` were updated to electric blue.
- The `CocoVoiceTextLogo` wordmark gradient was recolored from pink to blue.

### Documentation

The README received a hero banner and a badge row (license, platforms, Tauri, Discord) and retained its existing marketing-oriented structure. Factual references to upstream components (for example the `handy-keys` crate) were kept accurate.

## How to Run and Verify Live

The application runs in development mode with hot reload:

```bash
bun install
mkdir -p src-tauri/resources/models
curl -o src-tauri/resources/models/silero_vad_v4.onnx https://blob.handy.computer/silero_vad_v4.onnx
CMAKE_POLICY_VERSION_MINIMUM=3.5 bun run tauri dev
```

Frontend changes — the theme colors, the in-app wordmark, and interface strings — hot-reload instantly. The operating-system application icon (Dock, app switcher, installer) is embedded at compile time, so it updates on the next build rather than through hot reload. Tray icons are read from disk at runtime and refresh when the app is relaunched.

## Optional Follow-ups

The cosmetic internal references listed above (the theme storage key, the portable-mode marker, the `HANDY_*` environment variables, and the generated key-recording command names) can be renamed in a later pass if fully internal consistency is desired. None of them are user-facing, and renaming them requires coordinated changes across the Rust backend and the generated TypeScript bindings.
