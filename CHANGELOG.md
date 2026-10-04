# Changelog

All notable user-facing changes to Coco Voice are listed here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/).

## [Unreleased]

## [0.9.5] - 2026-10-03

### Changed

- New update signing key. Copies on 0.9.4 cannot update to 0.9.5 automatically: download 0.9.5 once by hand. Later updates install through "Check for updates" again. ([#38](https://github.com/coco-research/Coco-Voice/pull/38))
- New macOS signing certificate. After installing 0.9.5, grant Accessibility to Coco Voice once more. ([#38](https://github.com/coco-research/Coco-Voice/pull/38))

### Fixed

- Mute while recording no longer leaves your speakers muted after a take, keeps an existing mute as it was, and cannot deadlock the recording. ([#24](https://github.com/coco-research/Coco-Voice/pull/24))
- If changing the shortcut fails, the old shortcut keeps working. ([#25](https://github.com/coco-research/Coco-Voice/pull/25))
- A crash while loading a model no longer leaves the app stuck on "loading". ([#29](https://github.com/coco-research/Coco-Voice/pull/29))
- Dictation recovers by itself after an internal crash instead of stopping until restart. ([#30](https://github.com/coco-research/Coco-Voice/pull/30))
- A streaming transcription that times out no longer blocks the next dictation. ([#31](https://github.com/coco-research/Coco-Voice/pull/31))
- Spoken corrections replace the right characters, including emoji and the trailing space. ([#34](https://github.com/coco-research/Coco-Voice/pull/34))
- Recording a new shortcut no longer fights a previous, unfinished capture. ([#36](https://github.com/coco-research/Coco-Voice/pull/36))

### Security

- Build scripts no longer depend on a personal path. ([#23](https://github.com/coco-research/Coco-Voice/pull/23))

## [0.9.4] - 2026-07-19

Earlier versions are described in [GitHub Releases](https://github.com/coco-research/Coco-Voice/releases).

[Unreleased]: https://github.com/coco-research/Coco-Voice/compare/v0.9.5...HEAD
[0.9.5]: https://github.com/coco-research/Coco-Voice/compare/v0.9.4...v0.9.5
[0.9.4]: https://github.com/coco-research/Coco-Voice/releases/tag/v0.9.4
