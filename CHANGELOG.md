# Changelog

All notable changes to Coco Voice are listed here, newest first. From 0.9.5 on,
[release-please](https://github.com/googleapis/release-please) writes each entry
from the conventional commit titles when a release is cut. The user-facing notes
for each version are in [src/content/release-notes/](src/content/release-notes/)
and on the [GitHub Release](https://github.com/coco-research/Coco-Voice/releases).

## [0.10.0](https://github.com/coco-research/Coco-Voice/compare/v0.9.6...v0.10.0) (2026-10-17)


### Added

* add an on-device cleanup eval (30 takes, one command) ([#101](https://github.com/coco-research/Coco-Voice/issues/101)) ([36d202a](https://github.com/coco-research/Coco-Voice/commit/36d202ab5d5985ad4069defbbcc85d5df5305533))
* add the on-device llama.cpp cleanup module (macOS, not wired yet) ([#89](https://github.com/coco-research/Coco-Voice/issues/89)) ([00c234e](https://github.com/coco-research/Coco-Voice/commit/00c234e9b48813cebc6c4c7892458306d07196dd))
* drop spoken self-corrections in the default cleanup prompts ([#90](https://github.com/coco-research/Coco-Voice/issues/90)) ([634dde2](https://github.com/coco-research/Coco-Voice/commit/634dde244e003dfc849d9c1c1a7a0afb43259ce1))
* list Qwen3-4B for on-device cleanup and add its settings row ([#103](https://github.com/coco-research/Coco-Voice/issues/103)) ([a1b95d0](https://github.com/coco-research/Coco-Voice/commit/a1b95d04daacb07189650c11282cdcbfc4cded54))
* run cleanup on the on-device model and resolve the provider ([#104](https://github.com/coco-research/Coco-Voice/issues/104)) ([043bc7d](https://github.com/coco-research/Coco-Voice/commit/043bc7d9c6972a226d4fc41487071d31b3763121))
* show a "not cleaned" row in the overlay when cleanup did not run ([#112](https://github.com/coco-research/Coco-Voice/issues/112)) ([449d9c6](https://github.com/coco-research/Coco-Voice/commit/449d9c6e1883996ddc8a0e0522b0de30cf1464d7))
* tell the UI why cleanup did not run, and hold the overlay ([#107](https://github.com/coco-research/Coco-Voice/issues/107)) ([35f63b5](https://github.com/coco-research/Coco-Voice/commit/35f63b509b14bed3a18584a4579803c74ec64b80))


### Fixed

* bind verified models to the exact file that was hashed ([#100](https://github.com/coco-research/Coco-Voice/issues/100)) ([a6dbeb1](https://github.com/coco-research/Coco-Voice/commit/a6dbeb1fd4756f59c6b96384907b8643bce1b9e1))
* **ci:** keep a separate Rust cache per runner image ([#106](https://github.com/coco-research/Coco-Voice/issues/106)) ([b0b2e5d](https://github.com/coco-research/Coco-Voice/commit/b0b2e5d4d6c012fb0fcd4e6c1f607a3d8d2225c6))
* **ci:** skip llama.cpp's OpenSSL lookup so macOS builds work again ([#105](https://github.com/coco-research/Coco-Voice/issues/105)) ([8a42400](https://github.com/coco-research/Coco-Voice/commit/8a42400e048523de94506db52df9a22144250593))
* drop held and non-commercial models from the catalog ([#110](https://github.com/coco-research/Coco-Voice/issues/110)) ([50f632e](https://github.com/coco-research/Coco-Voice/commit/50f632ee3dc08f499e1d7c81fce1227d5b3984a4))
* never write dictated text to the log file ([#108](https://github.com/coco-research/Coco-Voice/issues/108)) ([61be943](https://github.com/coco-research/Coco-Voice/commit/61be9438e1d604f7cafd2144e462f6fd0d9ccf78))
* paste without freezing the Coco Voice window ([#102](https://github.com/coco-research/Coco-Voice/issues/102)) ([13d3df9](https://github.com/coco-research/Coco-Voice/commit/13d3df987bdb2552d3cbff3c8ba53e5491d085e4))
* save every settings change under the settings lock ([#93](https://github.com/coco-research/Coco-Voice/issues/93)) ([398a154](https://github.com/coco-research/Coco-Voice/commit/398a154a77c872b31074be09659838476362641e))
* verify the SHA-256 of Hugging Face model downloads ([#92](https://github.com/coco-research/Coco-Voice/issues/92)) ([a6606d2](https://github.com/coco-research/Coco-Voice/commit/a6606d292546172fe8dea0c746491eb2a55c6a86))

## [0.9.6](https://github.com/coco-research/Coco-Voice/compare/v0.9.5...v0.9.6) (2026-10-08)


### Fixed

* add a release finalize script that works on drafts and checks the draft first ([#80](https://github.com/coco-research/Coco-Voice/issues/80)) ([c29c075](https://github.com/coco-research/Coco-Voice/commit/c29c0752647458b344400329ec068249b0122408))

## [0.9.5](https://github.com/coco-research/Coco-Voice/compare/v0.9.4...v0.9.5) (2026-10-07)


### Fixed

* build PR code without repository secrets or a write token ([2c3cdd3](https://github.com/coco-research/Coco-Voice/commit/2c3cdd3834ea98c99a451b578d7ed468c3da557b))
* build PR code without repository secrets or a write token ([b5459a1](https://github.com/coco-research/Coco-Voice/commit/b5459a11d4171c8afb1c174d7725fe5095588300))
* build PR code without repository secrets or a write token ([#22](https://github.com/coco-research/Coco-Voice/issues/22)) ([b5459a1](https://github.com/coco-research/Coco-Voice/commit/b5459a11d4171c8afb1c174d7725fe5095588300))
* **ci:** keep PR test builds out of the Rust cache ([b60cc6e](https://github.com/coco-research/Coco-Voice/commit/b60cc6e77c746c4c6d934d8b90f9fe7ae1dfb1d3))
* **ci:** keep PR test builds out of the Rust cache ([#56](https://github.com/coco-research/Coco-Voice/issues/56)) ([b60cc6e](https://github.com/coco-research/Coco-Voice/commit/b60cc6e77c746c4c6d934d8b90f9fe7ae1dfb1d3))
* clear model loading if the load thread panics ([#29](https://github.com/coco-research/Coco-Voice/issues/29)) ([475c32e](https://github.com/coco-research/Coco-Voice/commit/475c32e40a93e6b5cedb0809233c473c091e9abe)), closes [#10](https://github.com/coco-research/Coco-Voice/issues/10)
* delete graphemes, including the trailing space, when a correction replaces text ([#34](https://github.com/coco-research/Coco-Voice/issues/34)) ([88f8c04](https://github.com/coco-research/Coco-Voice/commit/88f8c0412e38dd1dd51c787c4adeb4560488d7ba)), closes [#16](https://github.com/coco-research/Coco-Voice/issues/16)
* do not panic when the settings store fails ([2cf64fd](https://github.com/coco-research/Coco-Voice/commit/2cf64fddf108738a9f9fd962ed9a917b555a7040)), closes [#18](https://github.com/coco-research/Coco-Voice/issues/18)
* do not wipe a non-text clipboard when pasting ([28c4187](https://github.com/coco-research/Coco-Voice/commit/28c418738d6f46b1c8bdf4ad3b65a0bf3cb2b18d)), closes [#9](https://github.com/coco-research/Coco-Voice/issues/9)
* do not wipe a non-text clipboard when pasting ([#32](https://github.com/coco-research/Coco-Voice/issues/32)) ([28c4187](https://github.com/coco-research/Coco-Voice/commit/28c418738d6f46b1c8bdf4ad3b65a0bf3cb2b18d))
* fall back to batch only after a timed-out stream returns the engine ([#31](https://github.com/coco-research/Coco-Voice/issues/31)) ([a96f30f](https://github.com/coco-research/Coco-Voice/commit/a96f30fbc12f6086ef7e427d134ea75444b39a5a)), closes [#11](https://github.com/coco-research/Coco-Voice/issues/11)
* keep apostrophes and quotes in corrections ([c350285](https://github.com/coco-research/Coco-Voice/commit/c350285078d3971b6cf4b0c8ed74d3f4852f44fc))
* keep apostrophes and quotes in corrections ([#55](https://github.com/coco-research/Coco-Voice/issues/55)) ([c350285](https://github.com/coco-research/Coco-Voice/commit/c350285078d3971b6cf4b0c8ed74d3f4852f44fc))
* let build.yml inherit permissions from its caller ([2c3cdd3](https://github.com/coco-research/Coco-Voice/commit/2c3cdd3834ea98c99a451b578d7ed468c3da557b))
* let build.yml inherit permissions from its caller ([b5459a1](https://github.com/coco-research/Coco-Voice/commit/b5459a11d4171c8afb1c174d7725fe5095588300))
* make main CI green (translations, lint, format, nix) ([#20](https://github.com/coco-research/Coco-Voice/issues/20)) ([f7f4c9b](https://github.com/coco-research/Coco-Voice/commit/f7f4c9ba1d2f59f09645c48e398d6e11ed1f9ede))
* make mute-while-recording ordered, tracked and restored ([#24](https://github.com/coco-research/Coco-Voice/issues/24)) ([f6cc024](https://github.com/coco-research/Coco-Voice/commit/f6cc0248c5ec76a63e570c59414277b0119c4a4e)), closes [#4](https://github.com/coco-research/Coco-Voice/issues/4) [#5](https://github.com/coco-research/Coco-Voice/issues/5) [#6](https://github.com/coco-research/Coco-Voice/issues/6)
* make unsigned builds pass on macOS and the Linux package check ([2c3cdd3](https://github.com/coco-research/Coco-Voice/commit/2c3cdd3834ea98c99a451b578d7ed468c3da557b))
* make unsigned builds pass on macOS and the Linux package check ([b5459a1](https://github.com/coco-research/Coco-Voice/commit/b5459a11d4171c8afb1c174d7725fe5095588300))
* match the npm dialog and updater plugins to their Rust crates ([#72](https://github.com/coco-research/Coco-Voice/issues/72)) ([864bca7](https://github.com/coco-research/Coco-Voice/commit/864bca7ea1d74a700ca8b69e93fd3e7da576968f))
* move existing installs from the old 60 ms restore delay to 300 ms ([212a631](https://github.com/coco-research/Coco-Voice/commit/212a6318ae4c1f442dae9b19bcae045ec86006c8))
* never paste an empty cleanup result ([#66](https://github.com/coco-research/Coco-Voice/issues/66)) ([cc459ed](https://github.com/coco-research/Coco-Voice/commit/cc459ed8337056683c54dd2ba51468b9fcd4c404))
* never save defaults over settings after a failed read ([2cf64fd](https://github.com/coco-research/Coco-Voice/commit/2cf64fddf108738a9f9fd962ed9a917b555a7040))
* never save defaults over settings after a failed read ([#27](https://github.com/coco-research/Coco-Voice/issues/27)) ([2cf64fd](https://github.com/coco-research/Coco-Voice/commit/2cf64fddf108738a9f9fd962ed9a917b555a7040))
* point the Windows package check at coco-voice.exe ([2c3cdd3](https://github.com/coco-research/Coco-Voice/commit/2c3cdd3834ea98c99a451b578d7ed468c3da557b))
* point the Windows package check at coco-voice.exe ([b5459a1](https://github.com/coco-research/Coco-Voice/commit/b5459a11d4171c8afb1c174d7725fe5095588300))
* put the old hotkey back if rebinding fails ([#25](https://github.com/coco-research/Coco-Voice/issues/25)) ([2582832](https://github.com/coco-research/Coco-Voice/commit/25828327034973c0bc1072574b286f1025e591af)), closes [#7](https://github.com/coco-research/Coco-Voice/issues/7)
* refuse a microphone change while a take is in progress ([1203be9](https://github.com/coco-research/Coco-Voice/commit/1203be912db462734801a183da92a4120f904276))
* refuse a microphone change while a take is in progress ([#35](https://github.com/coco-research/Coco-Voice/issues/35)) ([1203be9](https://github.com/coco-research/Coco-Voice/commit/1203be912db462734801a183da92a4120f904276))
* register and unregister the cancel shortcut in order ([#26](https://github.com/coco-research/Coco-Voice/issues/26)) ([fa78499](https://github.com/coco-research/Coco-Voice/commit/fa784999ee25126b52d793ab9fd6d29c0ea8e120)), closes [#8](https://github.com/coco-research/Coco-Voice/issues/8)
* restart the dictation coordinator after a panic ([#30](https://github.com/coco-research/Coco-Voice/issues/30)) ([0bfb087](https://github.com/coco-research/Coco-Voice/commit/0bfb08737a8f5498ba8067c1681c695518f4e871)), closes [#12](https://github.com/coco-research/Coco-Voice/issues/12)
* run the macOS build scripts from the repo, not a personal path ([#23](https://github.com/coco-research/Coco-Voice/issues/23)) ([c2f6bb3](https://github.com/coco-research/Coco-Voice/commit/c2f6bb319235e71a9ef659ddd3d5038c6bac69d5))
* serialize settings writes and roll back only the failed key ([#28](https://github.com/coco-research/Coco-Voice/issues/28)) ([892bc79](https://github.com/coco-research/Coco-Voice/commit/892bc79cc9ad78619abd103802074d919bcc9687))
* stop a stale shortcut-capture loop when a new capture starts ([#36](https://github.com/coco-research/Coco-Voice/issues/36)) ([e6937cc](https://github.com/coco-research/Coco-Voice/commit/e6937cc939672fa0ecd83e4154bc760d8dc8a3c6)), closes [#19](https://github.com/coco-research/Coco-Voice/issues/19)
* translate the download labels in the progress bar ([197e769](https://github.com/coco-research/Coco-Voice/commit/197e7696e511493a012e10cc25b3b11d9730bc09))
* wait 300ms after the paste key before restoring the clipboard ([212a631](https://github.com/coco-research/Coco-Voice/commit/212a6318ae4c1f442dae9b19bcae045ec86006c8)), closes [#13](https://github.com/coco-research/Coco-Voice/issues/13)
* wait 300ms after the paste key before restoring the clipboard ([#33](https://github.com/coco-research/Coco-Voice/issues/33)) ([212a631](https://github.com/coco-research/Coco-Voice/commit/212a6318ae4c1f442dae9b19bcae045ec86006c8))
* withhold non-commercial and uncleared models, keep upstream attribution ([#63](https://github.com/coco-research/Coco-Voice/issues/63)) ([f8488f5](https://github.com/coco-research/Coco-Voice/commit/f8488f5bc53aac990197dcc1e110bd9a226512e1))

## [0.9.4](https://github.com/coco-research/Coco-Voice/releases/tag/v0.9.4) (2026-07-19)

Earlier versions are described in [GitHub Releases](https://github.com/coco-research/Coco-Voice/releases).
