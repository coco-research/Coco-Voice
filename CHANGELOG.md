# Changelog

All notable changes to Coco Voice are listed here, newest first. From 0.9.5 on,
[release-please](https://github.com/googleapis/release-please) writes each entry
from the conventional commit titles when a release is cut. The user-facing notes
for each version are in [src/content/release-notes/](src/content/release-notes/)
and on the [GitHub Release](https://github.com/coco-research/Coco-Voice/releases).

## [0.20.4](https://github.com/coco-research/Coco-Voice/compare/v0.20.3...v0.20.4) (2026-11-26)


### Fixed

* **ci:** retry release uploads and clear leftover partial assets ([#364](https://github.com/coco-research/Coco-Voice/issues/364)) ([1e1b941](https://github.com/coco-research/Coco-Voice/commit/1e1b9417504ddc78f20e07de607aa69861509fb9))

## [0.20.3](https://github.com/coco-research/Coco-Voice/compare/v0.20.2...v0.20.3) (2026-11-25)


### Fixed

* write meeting notes for longer meetings and follow custom template headings ([#355](https://github.com/coco-research/Coco-Voice/issues/355)) ([e9b9794](https://github.com/coco-research/Coco-Voice/commit/e9b979456acb6213acf577380e7a81ad773eb0e7))

## [0.20.2](https://github.com/coco-research/Coco-Voice/compare/v0.11.0...v0.20.2) (2026-11-25)


### Added

* choose one-shot or pieces for meeting notes ([#244](https://github.com/coco-research/Coco-Voice/issues/244)) ([8bdcede](https://github.com/coco-research/Coco-Voice/commit/8bdcede7c47421cb523b4d9821b9551f7ae87f77))
* **dictation:** add opt-in local stdio ask_user pill bridge ([#275](https://github.com/coco-research/Coco-Voice/issues/275)) ([244c447](https://github.com/coco-research/Coco-Voice/commit/244c4475cb45d9d9404aec5cdef96bbc21fc0beb))
* **meetings:** add local SQLite BM25 index, cited meeting Q&A, and spoken WAV queries ([#269](https://github.com/coco-research/Coco-Voice/issues/269)) ([2ea16ab](https://github.com/coco-research/Coco-Voice/commit/2ea16ab571b278c860785feefa602946dbe8327b))
* **meetings:** delete a meeting with its notes and audio ([#252](https://github.com/coco-research/Coco-Voice/issues/252)) ([eae0234](https://github.com/coco-research/Coco-Voice/commit/eae0234f2b9f3999339215b48faceea067f007e5))
* **meetings:** enable meetings in macOS release builds only ([#264](https://github.com/coco-research/Coco-Voice/issues/264)) ([dbd5778](https://github.com/coco-research/Coco-Voice/commit/dbd577891822c3d922fc879effa90aeb9efa6f9c))
* **meetings:** extract and sync cross-meeting spoken to-dos ([#274](https://github.com/coco-research/Coco-Voice/issues/274)) ([cda3eb4](https://github.com/coco-research/Coco-Voice/commit/cda3eb4e7634fa95e013988ae8e46c3aed969168))
* **meetings:** list meetings and search them by any text ([#250](https://github.com/coco-research/Coco-Voice/issues/250)) ([64491f6](https://github.com/coco-research/Coco-Voice/commit/64491f63e924f6bec4bab135f5f4aeb16858a0ac))
* merge meeting note lines across transcript pieces ([#242](https://github.com/coco-research/Coco-Voice/issues/242)) ([a4e7534](https://github.com/coco-research/Coco-Voice/commit/a4e7534a32c3950a3141221a19f06609486a09be))
* **settings:** add What goes online network disclosure section to About ([#265](https://github.com/coco-research/Coco-Voice/issues/265)) ([dbd9355](https://github.com/coco-research/Coco-Voice/commit/dbd9355c4f92d2899dc9b4c64b7d7f073fb1e850))
* **shell:** add grouped Home, Meetings, and Voice navigation shell ([#288](https://github.com/coco-research/Coco-Voice/issues/288)) ([e5dabd2](https://github.com/coco-research/Coco-Voice/commit/e5dabd23d0d37883f032586b16097b390a58312e))


### Fixed

* **ci:** create the folders ort-sys copies DLLs into on Windows ([#259](https://github.com/coco-research/Coco-Voice/issues/259)) ([413c1da](https://github.com/coco-research/Coco-Voice/commit/413c1da0f90f3e432a0bf8598413085964289a82))
* **ci:** fetch Windows ONNX Runtime from NuGet, pinned by SHA-256 ([#263](https://github.com/coco-research/Coco-Voice/issues/263)) ([874054d](https://github.com/coco-research/Coco-Voice/commit/874054d186a8e141e14f7c5fff5b5f38f1c2df19))
* keep a live meeting recording when the meeting store reopens ([#346](https://github.com/coco-research/Coco-Voice/issues/346)) ([0535b4f](https://github.com/coco-research/Coco-Voice/commit/0535b4faea2b862232997d91cd423396fbe5e4a0))
* keep meeting Stop and Pause off the main thread and pause recording ([#351](https://github.com/coco-research/Coco-Voice/issues/351)) ([126f4e7](https://github.com/coco-research/Coco-Voice/commit/126f4e73a72ff5faec4f4d7e9753539cedb6db7d))
* let a new meeting start after the last one finishes ([#342](https://github.com/coco-research/Coco-Voice/issues/342)) ([623509b](https://github.com/coco-research/Coco-Voice/commit/623509b98bae87c9acfc6ed4cf710b33b86ee509))
* let Ask answer questions asked in different words ([#348](https://github.com/coco-research/Coco-Voice/issues/348)) ([5536d5f](https://github.com/coco-research/Coco-Voice/commit/5536d5fceeaba9fe3638701a8435161a72af47fe))
* make the clean-up card download the clean-up model ([#344](https://github.com/coco-research/Coco-Voice/issues/344)) ([632b81a](https://github.com/coco-research/Coco-Voice/commit/632b81aa5b85c288ecdce96507fa9af6cf2d00af))
* **meetings:** drop the second MeetingSegment definition ([#262](https://github.com/coco-research/Coco-Voice/issues/262)) ([413c5f1](https://github.com/coco-research/Coco-Voice/commit/413c5f17dfc03c6ddf6a16ef30fd4c9e83918e87))
* **meetings:** keep a meeting chunk waiting out a dictation stream ([#238](https://github.com/coco-research/Coco-Voice/issues/238)) ([4fa7726](https://github.com/coco-research/Coco-Voice/commit/4fa7726717ecdc1b165bc1c9132ee23315d2f68e))
* **meetings:** keep the tap IO thread off the pool after a stop ([#236](https://github.com/coco-research/Coco-Voice/issues/236)) ([696ac48](https://github.com/coco-research/Coco-Voice/commit/696ac48553ba23adaeb8a301c2ad0a1ca05520fc))
* **meetings:** let the meeting planner pin the installed ONNX Parakeet ([#225](https://github.com/coco-research/Coco-Voice/issues/225)) ([f717536](https://github.com/coco-research/Coco-Voice/commit/f7175361474441191899ad2817dad1013cbf5c60))
* **meetings:** make history re-transcribe take the meeting lock ([#239](https://github.com/coco-research/Coco-Voice/issues/239)) ([da42606](https://github.com/coco-research/Coco-Voice/commit/da42606d1fb237d14aba03ff8bd1acd36e9c858b))
* **models:** surface typed model download failure cause and Try again ([#306](https://github.com/coco-research/Coco-Voice/issues/306)) ([6efab75](https://github.com/coco-research/Coco-Voice/commit/6efab75ac4b888cb50bc815997a778842f4e73f7))
* open the cited meeting when you click a source in Ask ([#331](https://github.com/coco-research/Coco-Voice/issues/331)) ([9129cb7](https://github.com/coco-research/Coco-Voice/commit/9129cb7036d76dc49ecd4d7ac853707b8f1aea66))
* report failed meeting deletes and never show sample lines as a transcript ([#327](https://github.com/coco-research/Coco-Voice/issues/327)) ([4ee81b4](https://github.com/coco-research/Coco-Voice/commit/4ee81b4257f5d033a28e3d78a9926d55e89ee2ad))
* say "Still working" on a busy press and lock the model picker while recording ([#253](https://github.com/coco-research/Coco-Voice/issues/253)) ([409caa5](https://github.com/coco-research/Coco-Voice/commit/409caa5732a5d4e7f82e040d4bf75f0fbc0b1742)), closes [#218](https://github.com/coco-research/Coco-Voice/issues/218)
* say Google Meet is in a call only when the tab shows a meeting code ([#326](https://github.com/coco-research/Coco-Voice/issues/326)) ([43f1153](https://github.com/coco-research/Coco-Voice/commit/43f1153f2cf6be13b3b41b31e689794cdf1c69be))
* settings labels, the coming-up meeting status, and the frontend suite in CI ([#328](https://github.com/coco-research/Coco-Voice/issues/328)) ([8ba12cd](https://github.com/coco-research/Coco-Voice/commit/8ba12cdce6e62791c28eb268b585e4f5a0e28a61))
* show the first try-it take once in onboarding ([#338](https://github.com/coco-research/Coco-Voice/issues/338)) ([8b37487](https://github.com/coco-research/Coco-Voice/commit/8b374873a7c8c015d1ff05626c8098f1584308ed))
* spot a stale Accessibility entry on the permission screen ([#303](https://github.com/coco-research/Coco-Voice/issues/303)) ([97a73b3](https://github.com/coco-research/Coco-Voice/commit/97a73b34013aec1c7ee02fe08de6a1ef09f5dd40))
* treat a permission check that can't complete as not granted ([#248](https://github.com/coco-research/Coco-Voice/issues/248)) ([3082746](https://github.com/coco-research/Coco-Voice/commit/3082746a1253fa30bddc57dce8d4e669576c7f80)), closes [#217](https://github.com/coco-research/Coco-Voice/issues/217)
* use the chosen note template when writing meeting notes ([#333](https://github.com/coco-research/Coco-Voice/issues/333)) ([47389f5](https://github.com/coco-research/Coco-Voice/commit/47389f561be9e1bf5be5621ce4462dc5e53f11a7))


### Miscellaneous

* bring the 0.20 line to main ([fd9ea8f](https://github.com/coco-research/Coco-Voice/commit/fd9ea8f111661e6586e5a6f015034513045d6a42))

## [0.11.0](https://github.com/coco-research/Coco-Voice/compare/v0.10.1...v0.11.0) (2026-11-17)


### Added

* meeting notes core with segment citations ([#241](https://github.com/coco-research/Coco-Voice/issues/241)) ([f0ad288](https://github.com/coco-research/Coco-Voice/commit/f0ad288e60ee0836f14ae45501f263da3db653ca))
* score WER with number words and digits treated as equal ([#221](https://github.com/coco-research/Coco-Voice/issues/221)) ([9214389](https://github.com/coco-research/Coco-Voice/commit/921438940762a600971124ebd0e56111bf1633de))
* store meeting notes and notepad points ([#243](https://github.com/coco-research/Coco-Voice/issues/243)) ([006a766](https://github.com/coco-research/Coco-Voice/commit/006a76629f3340811e489dcaca428088646e82af))


### Fixed

* hold transcription toggles until startup has managed the managers ([#245](https://github.com/coco-research/Coco-Voice/issues/245)) ([a748aa6](https://github.com/coco-research/Coco-Voice/commit/a748aa6203905069810ee3f679884fd105daa448)), closes [#212](https://github.com/coco-research/Coco-Voice/issues/212)
* **llm:** build against llama-cpp-2 0.1.161 ([#249](https://github.com/coco-research/Coco-Voice/issues/249)) ([77052f5](https://github.com/coco-research/Coco-Voice/commit/77052f580c4d07349e6cf025e608fe7df354db17)), closes [#233](https://github.com/coco-research/Coco-Voice/issues/233)
* **meetings:** drop a misaligned tap buffer instead of reading it ([#234](https://github.com/coco-research/Coco-Voice/issues/234)) ([53e71be](https://github.com/coco-research/Coco-Voice/commit/53e71be21e64f058d316c10b6cb1738dc95b4efc))
* **meetings:** parse Info.plist for the system-audio usage string ([#235](https://github.com/coco-research/Coco-Voice/issues/235)) ([09d415a](https://github.com/coco-research/Coco-Voice/commit/09d415a7f4db9524e555d6b0f17d4c234aa4f77f))
* use the backend key recorder for coco_voice_keys ([#246](https://github.com/coco-research/Coco-Voice/issues/246)) ([4f55528](https://github.com/coco-research/Coco-Voice/commit/4f5552899dc9884606cfeef4995060ec820dd0b7)), closes [#219](https://github.com/coco-research/Coco-Voice/issues/219)

## [0.10.1](https://github.com/coco-research/Coco-Voice/compare/v0.10.0...v0.10.1) (2026-11-16)


### Fixed

* apply the legal hold to every model source, not only Hugging Face ([#173](https://github.com/coco-research/Coco-Voice/issues/173)) ([5ce6a28](https://github.com/coco-research/Coco-Voice/commit/5ce6a284f397aac084dd87c6c8b78c3befce387a))
* cancelling a model download stops only that download ([#135](https://github.com/coco-research/Coco-Voice/issues/135)) ([b306eb3](https://github.com/coco-research/Coco-Voice/commit/b306eb344cbed41e1949494316c992a618b88c6c))
* check every catalog model's download against its SHA-256 ([#180](https://github.com/coco-research/Coco-Voice/issues/180)) ([9a7e453](https://github.com/coco-research/Coco-Voice/commit/9a7e4535896f4e9c8bb70c70acf821b040c9ba0f))
* format scripts/wer.ts so main's code-quality check passes again ([#183](https://github.com/coco-research/Coco-Voice/issues/183)) ([113e3bc](https://github.com/coco-research/Coco-Voice/commit/113e3bc2439a6fd482abef512b442670ef0974c1))
* keep models people already downloaded when the catalog pins, hashes or moves them ([#193](https://github.com/coco-research/Coco-Voice/issues/193)) ([89b8bae](https://github.com/coco-research/Coco-Voice/commit/89b8baecdb8482f2809abbd166c7109ff0205415))
* label Whisper large-v3-turbo MIT and pin licences to the converted source ([#181](https://github.com/coco-research/Coco-Voice/issues/181)) ([0ea3e4c](https://github.com/coco-research/Coco-Voice/commit/0ea3e4cd3ffcf0a052996adb0994ca9fcf53511b))
* pin every catalog model to a commit instead of main ([#179](https://github.com/coco-research/Coco-Voice/issues/179)) ([d0a279f](https://github.com/coco-research/Coco-Voice/commit/d0a279f9e9d8e9f041b6787836a800f67c405355))
* reuse a model already in the shared cache instead of downloading it again ([#136](https://github.com/coco-research/Coco-Voice/issues/136)) ([df4b703](https://github.com/coco-research/Coco-Voice/commit/df4b70334ce9f347940c01d95d66fc1ccb220e9b))
* say how to re-add Coco Voice in Accessibility after the update ([4fff70e](https://github.com/coco-research/Coco-Voice/commit/4fff70e10cbf6432d4614d16cec8356ab00f64a7))
* say why a shortcut can't be registered, and offer Try again ([#149](https://github.com/coco-research/Coco-Voice/issues/149)) ([7512e17](https://github.com/coco-research/Coco-Voice/commit/7512e179ca73b50b8e4f8452a263ea8e4bdae3e9))
* ship the system-audio permission text only in meetings builds ([#254](https://github.com/coco-research/Coco-Voice/issues/254)) ([2b60771](https://github.com/coco-research/Coco-Voice/commit/2b60771f6af16a4e95f58a6ca587dd9a3dd02a1d))

## [0.10.0](https://github.com/coco-research/Coco-Voice/compare/v0.9.6...v0.10.0) (2026-10-28)


### Added

* add an on-device cleanup eval (30 takes, one command) ([#101](https://github.com/coco-research/Coco-Voice/issues/101)) ([36d202a](https://github.com/coco-research/Coco-Voice/commit/36d202ab5d5985ad4069defbbcc85d5df5305533))
* add the on-device llama.cpp cleanup module (macOS, not wired yet) ([#89](https://github.com/coco-research/Coco-Voice/issues/89)) ([00c234e](https://github.com/coco-research/Coco-Voice/commit/00c234e9b48813cebc6c4c7892458306d07196dd))
* drop spoken self-corrections in the default cleanup prompts ([#90](https://github.com/coco-research/Coco-Voice/issues/90)) ([634dde2](https://github.com/coco-research/Coco-Voice/commit/634dde244e003dfc849d9c1c1a7a0afb43259ce1))
* keep dictated text on the clipboard by default on macOS ([#140](https://github.com/coco-research/Coco-Voice/issues/140)) ([7e4bc59](https://github.com/coco-research/Coco-Voice/commit/7e4bc5941a4db65a1bc220bed8499bc996987547))
* list Qwen3-4B for on-device cleanup and add its settings row ([#103](https://github.com/coco-research/Coco-Voice/issues/103)) ([a1b95d0](https://github.com/coco-research/Coco-Voice/commit/a1b95d04daacb07189650c11282cdcbfc4cded54))
* run cleanup on the on-device model and resolve the provider ([#104](https://github.com/coco-research/Coco-Voice/issues/104)) ([043bc7d](https://github.com/coco-research/Coco-Voice/commit/043bc7d9c6972a226d4fc41487071d31b3763121))
* show a "not cleaned" row in the overlay when cleanup did not run ([#112](https://github.com/coco-research/Coco-Voice/issues/112)) ([449d9c6](https://github.com/coco-research/Coco-Voice/commit/449d9c6e1883996ddc8a0e0522b0de30cf1464d7))
* show each downloadable model's licence, linked to its publisher ([#132](https://github.com/coco-research/Coco-Voice/issues/132)) ([80f5195](https://github.com/coco-research/Coco-Voice/commit/80f51958dc89a26e94b3186ab65cb5c924cf4408))
* show on-device cleanup progress in the overlay ([#131](https://github.com/coco-research/Coco-Voice/issues/131)) ([4336a06](https://github.com/coco-research/Coco-Voice/commit/4336a06405d98417b1d19eba42cfdbffe26d2f0a))
* show one settings banner with one fix when cleanup can't run ([#119](https://github.com/coco-research/Coco-Voice/issues/119)) ([884358c](https://github.com/coco-research/Coco-Voice/commit/884358c0079527ee6aea40c6bda7eb2d5447d08b))
* tell the UI why cleanup did not run, and hold the overlay ([#107](https://github.com/coco-research/Coco-Voice/issues/107)) ([35f63b5](https://github.com/coco-research/Coco-Voice/commit/35f63b509b14bed3a18584a4579803c74ec64b80))
* use the measured on-device prompt for unedited stock prompts ([#122](https://github.com/coco-research/Coco-Voice/issues/122)) ([3ed7837](https://github.com/coco-research/Coco-Voice/commit/3ed783707e858a4e368f6670d0843587a7b412ea))


### Fixed

* bind verified models to the exact file that was hashed ([#100](https://github.com/coco-research/Coco-Voice/issues/100)) ([a6dbeb1](https://github.com/coco-research/Coco-Voice/commit/a6dbeb1fd4756f59c6b96384907b8643bce1b9e1))
* **catalog:** pause two Fun-ASR models pending licence review ([#133](https://github.com/coco-research/Coco-Voice/issues/133)) ([123989b](https://github.com/coco-research/Coco-Voice/commit/123989b0c6a57c2ac8ce01e4b05bc81edabf91f0))
* **ci:** keep a separate Rust cache per runner image ([#106](https://github.com/coco-research/Coco-Voice/issues/106)) ([b0b2e5d](https://github.com/coco-research/Coco-Voice/commit/b0b2e5d4d6c012fb0fcd4e6c1f607a3d8d2225c6))
* **ci:** skip llama.cpp's OpenSSL lookup so macOS builds work again ([#105](https://github.com/coco-research/Coco-Voice/issues/105)) ([8a42400](https://github.com/coco-research/Coco-Voice/commit/8a42400e048523de94506db52df9a22144250593))
* drop held and non-commercial models from the catalog ([#110](https://github.com/coco-research/Coco-Voice/issues/110)) ([50f632e](https://github.com/coco-research/Coco-Voice/commit/50f632ee3dc08f499e1d7c81fce1227d5b3984a4))
* drop the unused store permission from the webviews ([#127](https://github.com/coco-research/Coco-Voice/issues/127)) ([5d4dd95](https://github.com/coco-research/Coco-Voice/commit/5d4dd954a43fb19ab9bebb6f977c523831961f98))
* keep the app's data and log folders private to the user ([#129](https://github.com/coco-research/Coco-Voice/issues/129)) ([bb3f894](https://github.com/coco-research/Coco-Voice/commit/bb3f8943a2785512cee8b2be461da41c0016fd65))
* keep the transcript on the clipboard when a paste fails ([#134](https://github.com/coco-research/Coco-Voice/issues/134)) ([4fee6f7](https://github.com/coco-research/Coco-Voice/commit/4fee6f79df86ddd088da16bfaf78759b4c562201))
* mention Universal Clipboard in the clipboard privacy hint ([#158](https://github.com/coco-research/Coco-Voice/issues/158)) ([36e455e](https://github.com/coco-research/Coco-Voice/commit/36e455e3ca413d39ce14dc6eb2e84ef3235424e7))
* never write dictated text to the log file ([#108](https://github.com/coco-research/Coco-Voice/issues/108)) ([61be943](https://github.com/coco-research/Coco-Voice/commit/61be9438e1d604f7cafd2144e462f6fd0d9ccf78))
* paste without freezing the Coco Voice window ([#102](https://github.com/coco-research/Coco-Voice/issues/102)) ([13d3df9](https://github.com/coco-research/Coco-Voice/commit/13d3df987bdb2552d3cbff3c8ba53e5491d085e4))
* remove the donate button ([#138](https://github.com/coco-research/Coco-Voice/issues/138)) ([fbc061f](https://github.com/coco-research/Coco-Voice/commit/fbc061f001339b204f5428ec2a2190d4c34361a2))
* save every settings change under the settings lock ([#93](https://github.com/coco-research/Coco-Voice/issues/93)) ([398a154](https://github.com/coco-research/Coco-Voice/commit/398a154a77c872b31074be09659838476362641e))
* skip the paste when macOS secure input is on ([#137](https://github.com/coco-research/Coco-Voice/issues/137)) ([5f99645](https://github.com/coco-research/Coco-Voice/commit/5f9964580d7f0845b7e9aa763164a1cbdade1950))
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
