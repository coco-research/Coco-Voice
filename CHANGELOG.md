# Changelog

All notable changes to Coco Voice are listed here, newest first. From 0.9.5 on,
[release-please](https://github.com/googleapis/release-please) writes each entry
from the conventional commit titles when a release is cut. The user-facing notes
for each version are in [src/content/release-notes/](src/content/release-notes/)
and on the [GitHub Release](https://github.com/coco-research/Coco-Voice/releases).

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
