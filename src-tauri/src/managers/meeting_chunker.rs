//! Per-channel meeting chunks. Silence uses one Silero session per channel
//! (threshold 0.3, 480-sample frames, 15-frame hangover). A chunk also closes
//! at 30 seconds. Each finished chunk is one `meeting_segments` row.

#![cfg(feature = "meetings")]

use crate::audio_toolkit::vad::{SileroVad, VoiceActivityDetector};
use crate::catalog::find_model;
use crate::managers::meetings::{set_meeting_active, MeetingsStore};
use crate::managers::model::EngineType;
use crate::managers::transcription::TranscriptionManager;
use anyhow::{anyhow, Result};
use log::warn;
use std::path::Path;

const FRAME_SAMPLES: usize = 480;
const HANGOVER_FRAMES: usize = 15;
const MAX_CHUNK_SAMPLES: usize = 16_000 * 30;
const SAMPLE_RATE: u64 = 16_000;
/// Speech if a frame's peak is at least this. Tests use it in place of Silero.
#[cfg(test)]
const ENERGY_THRESHOLD: f32 = 0.2;

pub const CHANNEL_ME: &str = "me";
pub const CHANNEL_OTHERS: &str = "others";

pub fn channel_label(channel: &str) -> Option<&'static str> {
    match channel {
        CHANNEL_ME => Some("Me"),
        CHANNEL_OTHERS => Some("Others"),
        _ => None,
    }
}

fn ms(samples: u64) -> i64 {
    (samples * 1000 / SAMPLE_RATE) as i64
}

pub trait FrameSpeech: Send {
    fn is_speech_frame(&mut self, frame: &[f32]) -> bool;
}

/// One Silero session. The dictation recorder keeps its own.
#[allow(dead_code)]
pub struct SileroSpeech {
    vad: SileroVad,
    warned: bool,
}

impl SileroSpeech {
    pub fn new(model_path: &Path) -> Result<Self> {
        Ok(Self {
            vad: SileroVad::new(model_path, 0.3)?,
            warned: false,
        })
    }
}

impl FrameSpeech for SileroSpeech {
    fn is_speech_frame(&mut self, frame: &[f32]) -> bool {
        match self.vad.push_frame(frame) {
            Ok(frame) => frame.is_speech(),
            Err(err) => {
                if !self.warned {
                    self.warned = true;
                    warn!("silero push_frame failed, treating this frame as silence: {err}");
                }
                false
            }
        }
    }
}

/// Peak-energy stand-in for Silero. Meeting tests have no VAD weights.
#[cfg(test)]
struct EnergySpeech;

#[cfg(test)]
impl FrameSpeech for EnergySpeech {
    fn is_speech_frame(&mut self, frame: &[f32]) -> bool {
        frame.iter().any(|sample| sample.abs() >= ENERGY_THRESHOLD)
    }
}

pub struct FinishedChunk {
    pub channel: &'static str,
    pub start_ms: i64,
    pub end_ms: i64,
    pub samples: Vec<f32>,
}

pub struct ChannelChunker {
    channel: &'static str,
    vad: Box<dyn FrameSpeech>,
    pending: Vec<f32>,
    seen: u64,
    chunk: Vec<f32>,
    chunk_start: u64,
    in_speech: bool,
    hangover: usize,
}

impl ChannelChunker {
    pub fn new(channel: &'static str, vad: Box<dyn FrameSpeech>) -> Self {
        Self {
            channel,
            vad,
            pending: Vec::new(),
            seen: 0,
            chunk: Vec::new(),
            chunk_start: 0,
            in_speech: false,
            hangover: 0,
        }
    }

    /// Own Silero session for this channel. Capture feeds it 16 kHz mono.
    #[allow(dead_code)]
    pub fn silero(channel: &'static str, model_path: &Path) -> Result<Self> {
        Ok(Self::new(channel, Box::new(SileroSpeech::new(model_path)?)))
    }

    pub fn push(&mut self, samples: &[f32]) -> Vec<FinishedChunk> {
        self.pending.extend_from_slice(samples);
        let mut done = Vec::new();
        let mut frame = [0.0f32; FRAME_SAMPLES];
        while self.pending.len() >= FRAME_SAMPLES {
            frame.copy_from_slice(&self.pending[..FRAME_SAMPLES]);
            self.pending.drain(..FRAME_SAMPLES);
            if let Some(chunk) = self.take_frame(&frame) {
                done.push(chunk);
            }
        }
        done
    }

    /// Close an open chunk at stop, without waiting out the hangover.
    pub fn finish(&mut self) -> Option<FinishedChunk> {
        if self.in_speech && !self.pending.is_empty() {
            let tail = std::mem::take(&mut self.pending);
            self.seen += tail.len() as u64;
            self.chunk.extend(tail);
        } else {
            self.pending.clear();
        }
        if !self.in_speech || self.chunk.is_empty() {
            return None;
        }
        let done = self.emit();
        self.in_speech = false;
        self.hangover = 0;
        done
    }

    fn take_frame(&mut self, frame: &[f32]) -> Option<FinishedChunk> {
        let speech = self.vad.is_speech_frame(frame);
        if !self.in_speech {
            if !speech {
                self.seen += frame.len() as u64;
                return None;
            }
            self.in_speech = true;
            self.hangover = HANGOVER_FRAMES;
            self.chunk_start = self.seen;
            self.chunk.clear();
        }

        self.chunk.extend_from_slice(frame);
        self.seen += frame.len() as u64;
        if speech {
            self.hangover = HANGOVER_FRAMES;
        } else {
            self.hangover = self.hangover.saturating_sub(1);
        }

        let hit_silence = !speech && self.hangover == 0;
        // ponytail: the cut lands on the frame that reaches 30 s, so a chunk
        // can run one 30 ms frame past the cap. A sample-accurate trim is the
        // upgrade if a later check cares.
        let hit_cap = self.chunk.len() >= MAX_CHUNK_SAMPLES;
        if !hit_silence && !hit_cap {
            return None;
        }

        let done = self.emit();
        self.in_speech = false;
        self.hangover = 0;
        done
    }

    fn emit(&mut self) -> Option<FinishedChunk> {
        if self.chunk.is_empty() {
            return None;
        }
        let samples = std::mem::take(&mut self.chunk);
        let end = self.chunk_start + samples.len() as u64;
        Some(FinishedChunk {
            channel: self.channel,
            start_ms: ms(self.chunk_start),
            end_ms: ms(end),
            samples,
        })
    }
}

/// Transcribe one chunk under the meeting lock and store the raw text.
/// Meeting text does not go through `post_process_transcription`.
/// The segment is stored even when the engine check afterwards fails.
pub fn commit_chunk<R: tauri::Runtime>(
    tm: &TranscriptionManager<R>,
    store: &MeetingsStore,
    meeting_id: i64,
    chunk: FinishedChunk,
) -> Result<String> {
    let text = tm.transcribe_meeting_chunk(chunk.samples)?;
    store.insert_segment(
        meeting_id,
        chunk.start_ms,
        chunk.end_ms,
        chunk.channel,
        &text,
    )?;
    if !tm.is_engine_available() {
        return Err(anyhow!("engine still out after a meeting chunk"));
    }
    Ok(text)
}

/// Every downloaded speech model is Voxtral, so Start names that model and
/// records nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MeetingStartRefusal {
    OnlyVoxtral { model_id: String },
}

impl std::fmt::Display for MeetingStartRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OnlyVoxtral { model_id } => write!(
                f,
                "no long-audio speech model is downloaded (only {model_id})"
            ),
        }
    }
}

#[derive(Debug)]
pub enum BeginMeetingError {
    Refused(MeetingStartRefusal),
    Store(anyhow::Error),
}

/// Speech model to pin. Does not write `selected_model`.
/// `qwen3` rows are not speech models (`is_local_llm_arch` -> not a speech engine).
///
/// `other` answers for a downloaded id the catalog does not know (the ONNX
/// engines `ModelManager::new` registers); production passes
/// `|id| model_manager.get_model_info(id).map(|m| m.engine_type)`.
///
/// Pick order, deterministic: `selected` when it takes long audio, else the
/// first parakeet (catalog GGUF ids before non-catalog ones, ties by
/// `downloaded` order), else the first long-audio id.
pub fn plan_meeting_model(
    selected: &str,
    downloaded: &[String],
    other: &dyn Fn(&str) -> Option<EngineType>,
) -> Result<String, MeetingStartRefusal> {
    let mut speech = Vec::new();
    let mut long_audio = Vec::new();
    let mut catalog_parakeet: Option<String> = None;
    let mut other_parakeet: Option<String> = None;
    for id in downloaded {
        // Catalog rows keep today's rules: every family but `voxtral` is safe
        // for 30 s chunks, and `parakeet` is the preferred family.
        let (is_speech, takes_long_audio, is_parakeet, in_catalog) = match find_model(id) {
            Some(desc) => (
                desc.engine_type.is_speech(),
                desc.family.as_deref() != Some("voxtral"),
                desc.family.as_deref() == Some("parakeet"),
                true,
            ),
            // ponytail: only Parakeet is cleared for 30 s chunks off-catalog.
            // Every other ONNX engine counts as speech but not long-audio until
            // it gets its own long-audio check; add it to this arm after that
            // check exists. `None` from the lookup means not downloaded: skip.
            None => match other(id) {
                Some(engine_type) => {
                    let is_parakeet = matches!(engine_type, EngineType::Parakeet);
                    (engine_type.is_speech(), is_parakeet, is_parakeet, false)
                }
                None => continue,
            },
        };
        if !is_speech {
            continue;
        }
        speech.push(id.clone());
        if !takes_long_audio {
            continue;
        }
        long_audio.push(id.clone());
        if is_parakeet && in_catalog && catalog_parakeet.is_none() {
            catalog_parakeet = Some(id.clone());
        } else if is_parakeet && !in_catalog && other_parakeet.is_none() {
            other_parakeet = Some(id.clone());
        }
    }
    if long_audio.is_empty() {
        let model_id = speech
            .into_iter()
            .next()
            .unwrap_or_else(|| selected.to_string());
        return Err(MeetingStartRefusal::OnlyVoxtral { model_id });
    }
    if long_audio.iter().any(|id| id == selected) {
        return Ok(selected.to_string());
    }
    if let Some(id) = catalog_parakeet.or(other_parakeet) {
        return Ok(id);
    }
    Ok(long_audio.remove(0))
}

/// Start. Refusal happens before any audio and before a meeting row.
pub fn begin_meeting(
    store: &MeetingsStore,
    selected: &str,
    downloaded: &[String],
    title: &str,
    other: &dyn Fn(&str) -> Option<EngineType>,
) -> Result<i64, BeginMeetingError> {
    let model_id =
        plan_meeting_model(selected, downloaded, other).map_err(BeginMeetingError::Refused)?;
    let id = store
        .start_meeting(title, &model_id)
        .map_err(BeginMeetingError::Store)?;
    set_meeting_active(true);
    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actions::POST_PROCESS_CALLS;
    use crate::managers::meetings::test_lock;
    use crate::managers::model::{builtin_models, EngineType};
    use crate::managers::transcription::DICTATION_LOAD_KICKOFFS;
    use crate::settings::{get_settings, update_settings, ModelUnloadTimeout};
    use std::sync::atomic::Ordering;
    use std::sync::{Arc, Barrier};
    use std::thread;

    const VOXTRAL: &str = "handy-computer/Voxtral-Mini-4B-Realtime-2602-gguf";
    const PARAKEET: &str = "handy-computer/parakeet-tdt-0.6b-v3-gguf";
    const QWEN: &str = "Qwen/Qwen3-4B-GGUF";
    const CANARY: &str = "handy-computer/canary-180m-flash-gguf";
    const CANARY_FILE: &str = "canary-180m-flash-Q8_0.gguf";
    const PARAKEET_V3: &str = "parakeet-tdt-0.6b-v3";
    const PARAKEET_V3_DIR: &str = "parakeet-tdt-0.6b-v3-int8";

    /// What `test_app()` produces, so the live fixture helper can hand it to a
    /// closure without repeating the runtime type at every call site.
    type TestManager = TranscriptionManager<tauri::test::MockRuntime>;

    struct ResetMeeting;
    impl Drop for ResetMeeting {
        fn drop(&mut self) {
            set_meeting_active(false);
        }
    }

    fn test_app() -> tauri::App<tauri::test::MockRuntime> {
        tauri::test::mock_builder()
            .plugin(tauri_plugin_store::Builder::default().build())
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock app")
    }

    fn frame(speech: bool) -> Vec<f32> {
        if speech {
            vec![0.5; FRAME_SAMPLES]
        } else {
            vec![0.0; FRAME_SAMPLES]
        }
    }

    /// Speech, a silence longer than the hangover, speech, then silence that
    /// closes the second chunk.
    fn speech_silence_speech() -> Vec<f32> {
        let mut samples = Vec::new();
        for _ in 0..20 {
            samples.extend(frame(true));
        }
        for _ in 0..40 {
            samples.extend(frame(false));
        }
        for _ in 0..20 {
            samples.extend(frame(true));
        }
        for _ in 0..20 {
            samples.extend(frame(false));
        }
        samples
    }

    fn collect(chunker: &mut ChannelChunker, samples: &[f32]) -> Vec<FinishedChunk> {
        let mut chunks = chunker.push(samples);
        if let Some(tail) = chunker.finish() {
            chunks.push(tail);
        }
        chunks
    }

    /// `other` for the planner: the built-in non-catalog table, by id.
    fn builtin_lookup() -> impl Fn(&str) -> Option<EngineType> {
        let models = builtin_models();
        move |id| {
            models
                .iter()
                .find(|model| model.id == id)
                .map(|model| model.engine_type.clone())
        }
    }

    #[test]
    fn speech_silence_speech_yields_two_segments() {
        let _lock = test_lock();
        let _reset = ResetMeeting;
        let app = test_app();
        let tm = TranscriptionManager::new_for_test(app.handle().clone());
        tm.install_fake_engine(PARAKEET);
        let dir = tempfile::tempdir().unwrap();
        let store = MeetingsStore::open(dir.path()).unwrap();
        let meeting_id = store.start_meeting("Standup", PARAKEET).unwrap();
        let audio = speech_silence_speech();

        let mut me = ChannelChunker::new(CHANNEL_ME, Box::new(EnergySpeech));
        let me_chunks = collect(&mut me, &audio);
        assert_eq!(me_chunks.len(), 2, "me channel did not cut on silence");
        assert_eq!(me_chunks[0].start_ms, 0);
        assert_eq!(me_chunks[0].end_ms, 1050);
        assert_eq!(me_chunks[1].start_ms, 1800);
        assert!(me_chunks[1].end_ms > me_chunks[1].start_ms);
        for chunk in me_chunks {
            assert_eq!(channel_label(chunk.channel), Some("Me"));
            let text = commit_chunk(&tm, &store, meeting_id, chunk).unwrap();
            assert!(!text.is_empty());
            assert!(tm.is_engine_available());
        }

        let mut others = ChannelChunker::new(CHANNEL_OTHERS, Box::new(EnergySpeech));
        let others_chunks = collect(&mut others, &audio);
        assert_eq!(
            others_chunks.len(),
            2,
            "others channel did not cut on silence"
        );
        for chunk in others_chunks {
            assert_eq!(channel_label(chunk.channel), Some("Others"));
            commit_chunk(&tm, &store, meeting_id, chunk).unwrap();
            assert!(tm.is_engine_available());
        }

        let rows = store.segments(meeting_id).unwrap();
        assert_eq!(rows.len(), 4);
        assert_eq!(rows.iter().filter(|row| row.channel == "me").count(), 2);
        assert_eq!(rows.iter().filter(|row| row.channel == "others").count(), 2);
        assert!(rows.iter().all(|row| row.end_ms > row.start_ms));
        assert!(rows.iter().all(|row| !row.text.is_empty()));
    }

    /// A cut on the cap frame must not leave a chunk holding only hangover
    /// silence. The next speech frame opens the next chunk on its own.
    #[test]
    fn cap_cut_on_last_speech_frame_leaves_no_silence_chunk() {
        let mut audio = vec![0.5_f32; MAX_CHUNK_SAMPLES];
        audio.resize(audio.len() + 2 * SAMPLE_RATE as usize, 0.0);
        let mut chunker = ChannelChunker::new(CHANNEL_ME, Box::new(EnergySpeech));
        let chunks = collect(&mut chunker, &audio);
        assert_eq!(
            chunks.len(),
            1,
            "a silence-only chunk came out after the cap"
        );
        assert!(chunks[0].samples.len() >= MAX_CHUNK_SAMPLES);
    }

    /// Live fixture shared by the ignored live tests: `say` -> `afconvert` ->
    /// the Silero chunker -> the engine `install` loads -> one `meeting_lock`
    /// pass. `start` opens the meeting row, so the planner is what pins the
    /// model. Returns the model id that row was started with.
    fn run_live_fixture(
        install: impl FnOnce(&TestManager) -> anyhow::Result<()>,
        start: impl FnOnce(&MeetingsStore) -> i64,
    ) -> String {
        let _lock = test_lock();
        let _reset = ResetMeeting;
        let app = test_app();

        let dir = std::env::temp_dir().join(format!(
            "coco-live-chunker-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system clock is before the epoch")
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).expect("create the fixture dir");
        struct FixtureDir(std::path::PathBuf);
        impl Drop for FixtureDir {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let _fixture = FixtureDir(dir.clone());

        let aiff = dir.join("fixture.aiff");
        let wav = dir.join("fixture.wav");
        let said = std::process::Command::new("say")
            .arg("-o")
            .arg(&aiff)
            .arg("The quarterly budget review starts on Monday morning")
            .status()
            .expect("run say");
        assert!(said.success(), "say failed: {said}");
        let converted = std::process::Command::new("afconvert")
            .args(["-f", "WAVE", "-d", "LEI16@16000", "-c", "1"])
            .arg(&aiff)
            .arg(&wav)
            .status()
            .expect("run afconvert");
        assert!(converted.success(), "afconvert failed: {converted}");

        let speech =
            crate::audio_toolkit::read_wav_samples(&wav).expect("read the 16 kHz fixture wav");
        let mut audio = vec![0.0_f32; SAMPLE_RATE as usize];
        audio.extend(speech);
        audio.resize(audio.len() + 2 * SAMPLE_RATE as usize, 0.0);

        let silero = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("resources/models/silero_vad_v4.onnx");
        let mut chunker =
            ChannelChunker::silero(CHANNEL_ME, &silero).expect("build the silero chunker");
        let mut chunks = Vec::new();
        for slice in audio.chunks(FRAME_SAMPLES) {
            chunks.extend(chunker.push(slice));
        }
        if let Some(tail) = chunker.finish() {
            chunks.push(tail);
        }
        assert!(!chunks.is_empty(), "the chunker produced no finished chunk");

        let tm = TranscriptionManager::new_for_test(app.handle().clone());
        install(&tm).expect("load the engine at the given path");

        // The meeting row is where the planned model lands, so it is what
        // "pinned" means for a live run.
        let store_dir = tempfile::tempdir().expect("create the meeting store dir");
        let store = MeetingsStore::open(store_dir.path()).expect("open the meeting store");
        let meeting_id = start(&store);
        let pinned = store
            .meeting(meeting_id)
            .expect("read the meeting")
            .model_id;

        let mut texts = Vec::new();
        for chunk in chunks {
            assert_eq!(chunk.channel, CHANNEL_ME);
            texts.push(
                tm.transcribe_meeting_chunk(chunk.samples)
                    .expect("transcribe a meeting chunk"),
            );
        }
        let joined = texts.join(" ").to_lowercase();
        for word in ["budget", "review", "monday"] {
            assert!(joined.contains(word), "missing '{word}' in: {joined}");
        }
        assert!(
            tm.is_engine_available(),
            "the meeting lock did not release the engine"
        );
        pinned
    }

    #[test]
    #[ignore = "live: needs macOS say/afconvert and an installed speech model"]
    fn live_chunker_transcribes_fixture() {
        let snapshots = std::env::var("HOME")
            .map(std::path::PathBuf::from)
            .expect("HOME is set")
            .join(".cache/huggingface/hub")
            .join(format!("models--{}/snapshots", CANARY.replace('/', "--")));
        let model_path = std::fs::read_dir(&snapshots)
            .ok()
            .and_then(|entries| {
                entries
                    .filter_map(|entry| entry.ok())
                    .map(|entry| entry.path().join(CANARY_FILE))
                    .find(|path| path.is_file())
            })
            .unwrap_or_else(|| {
                panic!(
                    "missing {CANARY_FILE} under {}; download the model first",
                    snapshots.display()
                )
            });

        run_live_fixture(
            |tm| tm.install_engine_from_path_for_test(CANARY, &model_path),
            |store| {
                store
                    .start_meeting("Live fixture", CANARY)
                    .expect("start the meeting")
            },
        );
    }

    /// The ONNX Parakeet V3 engine, read straight from its installed directory.
    /// The model dir is only read; nothing in it is written, moved or deleted.
    #[test]
    #[ignore = "live: needs macOS say/afconvert and the installed Parakeet V3 model"]
    fn live_chunker_transcribes_fixture_parakeet_v3() {
        let dir = std::env::var("COCO_TEST_PARAKEET_V3_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| {
                let home = std::env::var("HOME").expect("HOME is set");
                std::path::PathBuf::from(home)
                    .join("Library/Application Support/com.cocoresearch.cocovoice/models")
                    .join(PARAKEET_V3_DIR)
            });
        if !dir.is_dir() {
            panic!(
                "missing the Parakeet V3 model under {}; install it first or set \
                 COCO_TEST_PARAKEET_V3_DIR",
                dir.display()
            );
        }

        let lookup = builtin_lookup();
        let pinned = run_live_fixture(
            |tm| tm.install_engine_with_type_for_test(PARAKEET_V3, EngineType::Parakeet, &dir),
            |store| {
                let downloaded = [PARAKEET_V3.to_string()];
                begin_meeting(store, "", &downloaded, "Live fixture", &lookup)
                    .expect("start the meeting through the planner")
            },
        );
        assert_eq!(pinned, PARAKEET_V3, "the meeting pinned a different model");
    }

    #[test]
    fn planner_pins_onnx_parakeet_v3_when_it_is_the_only_download() {
        let lookup = builtin_lookup();
        let downloaded = vec![PARAKEET_V3.to_string()];
        let pinned = plan_meeting_model("", &downloaded, &lookup).unwrap();
        assert_eq!(pinned, PARAKEET_V3);
    }

    #[test]
    fn planner_prefers_gguf_parakeet_over_onnx_when_both_downloaded() {
        let lookup = builtin_lookup();
        // The ONNX id comes first, so picking the catalog id proves the order is
        // not simply `downloaded` order.
        let downloaded = vec![PARAKEET_V3.to_string(), PARAKEET.to_string()];
        let pinned = plan_meeting_model(VOXTRAL, &downloaded, &lookup).unwrap();
        assert_eq!(pinned, PARAKEET);

        let pinned = plan_meeting_model(PARAKEET_V3, &downloaded, &lookup).unwrap();
        assert_eq!(pinned, PARAKEET_V3);
    }

    #[test]
    fn planner_refuses_when_nothing_is_downloaded() {
        let lookup = builtin_lookup();
        let downloaded: Vec<String> = Vec::new();
        let err = plan_meeting_model(VOXTRAL, &downloaded, &lookup).unwrap_err();
        assert_eq!(
            err,
            MeetingStartRefusal::OnlyVoxtral {
                model_id: VOXTRAL.to_string()
            }
        );
    }

    /// Reads this Mac's installed models. Read only: nothing in that directory
    /// is written, moved or deleted. Catalog GGUFs live in the Hugging Face
    /// cache, not this directory, so they are not counted here.
    #[test]
    #[ignore = "live: reads this Mac's installed models, read only"]
    fn planner_picks_on_this_macs_installed_models() {
        let dir = std::env::var("COCO_TEST_MODELS_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| {
                let home = std::env::var("HOME").expect("HOME is set");
                std::path::PathBuf::from(home)
                    .join("Library/Application Support/com.cocoresearch.cocovoice/models")
            });
        let downloaded: Vec<String> = builtin_models()
            .into_iter()
            .filter(|model| dir.join(model.filename.as_str()).exists())
            .map(|model| model.id)
            .collect();
        let lookup = builtin_lookup();
        let picked = match plan_meeting_model("", &downloaded, &lookup) {
            Ok(id) => id,
            Err(refusal) => format!("refused: {refusal}"),
        };
        println!("downloaded={} picked={}", downloaded.join(", "), picked);
        if dir.join(PARAKEET_V3_DIR).exists() {
            assert_eq!(
                picked, PARAKEET_V3,
                "the planner did not pick the installed ONNX Parakeet"
            );
        }
    }

    #[test]
    fn meeting_pin_does_not_change_selected_model() {
        let _lock = test_lock();
        let _reset = ResetMeeting;
        let app = test_app();
        let selected = VOXTRAL.to_string();
        update_settings(app.handle(), |settings| {
            settings.selected_model = selected.clone();
        });
        let downloaded = vec![VOXTRAL.to_string(), PARAKEET.to_string(), QWEN.to_string()];
        let pinned = plan_meeting_model(&selected, &downloaded, &|_| None).unwrap();
        assert_eq!(pinned, PARAKEET);
        assert_eq!(get_settings(app.handle()).selected_model, selected);

        let tm = TranscriptionManager::new_for_test(app.handle().clone());
        tm.install_fake_engine(&pinned);
        let current = tm.get_current_model();
        assert_eq!(current.as_deref(), Some(PARAKEET));
        tm.pin_meeting_model(&pinned);
        DICTATION_LOAD_KICKOFFS.store(0, Ordering::SeqCst);
        tm.initiate_model_load();
        assert_eq!(tm.get_current_model(), current);
        assert_eq!(get_settings(app.handle()).selected_model, selected);
        assert_eq!(DICTATION_LOAD_KICKOFFS.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn meeting_lock_serialises_engine_use() {
        let _lock = test_lock();
        let _reset = ResetMeeting;
        let app = test_app();
        let tm = Arc::new(TranscriptionManager::new_for_test(app.handle().clone()));
        tm.install_fake_engine(PARAKEET);
        let barrier = Arc::new(Barrier::new(3));
        let mut handles = Vec::new();
        for worker in 0..3 {
            let tm = Arc::clone(&tm);
            let barrier = Arc::clone(&barrier);
            handles.push(thread::spawn(move || {
                barrier.wait();
                let audio = vec![0.2_f32; 1600];
                if worker == 2 {
                    tm.transcribe_with_meeting_lock(audio)
                } else {
                    tm.transcribe_meeting_chunk(audio)
                }
            }));
        }
        for handle in handles {
            let result = handle.join().expect("worker panicked");
            match result {
                Ok(text) => assert!(text.starts_with("chunk-"), "{text}"),
                Err(err) => {
                    let message = err.to_string();
                    assert!(
                        !message.contains("Model is not loaded"),
                        "dictation or a chunk saw the engine out: {message}"
                    );
                    assert!(
                        !message.contains("still busy"),
                        "dictation or a chunk saw the engine busy: {message}"
                    );
                    panic!("transcription failed: {message}");
                }
            }
        }
        assert!(tm.is_engine_available());
    }

    #[test]
    fn pinned_model_not_unloaded_during_meeting() {
        let _lock = test_lock();
        let _reset = ResetMeeting;
        let app = test_app();
        let tm = TranscriptionManager::new_for_test(app.handle().clone());
        tm.install_fake_engine(PARAKEET);
        tm.set_test_unload_timeout(Some(ModelUnloadTimeout::Immediately));
        tm.pin_meeting_model(PARAKEET);
        tm.maybe_unload_immediately("meeting");
        tm.poll_idle_unload();
        assert!(tm.is_model_loaded());
        assert!(tm.is_engine_available());

        tm.clear_meeting_pin();
        tm.maybe_unload_immediately("meeting ended");
        assert!(!tm.is_model_loaded());

        tm.install_fake_engine(PARAKEET);
        tm.set_test_unload_timeout(Some(ModelUnloadTimeout::Min5));
        tm.set_last_activity_for_test(0);
        tm.pin_meeting_model(PARAKEET);
        tm.poll_idle_unload();
        assert!(
            tm.is_model_loaded(),
            "idle watcher unloaded the pinned model"
        );
        tm.clear_meeting_pin();
        tm.poll_idle_unload();
        assert!(
            !tm.is_model_loaded(),
            "idle watcher left the model loaded after the meeting"
        );
    }

    #[test]
    fn meeting_text_never_post_processed() {
        let _lock = test_lock();
        let _reset = ResetMeeting;
        let app = test_app();
        let tm = TranscriptionManager::new_for_test(app.handle().clone());
        tm.install_fake_engine(PARAKEET);
        let dir = tempfile::tempdir().unwrap();
        let store = MeetingsStore::open(dir.path()).unwrap();
        let meeting_id = store.start_meeting("Standup", PARAKEET).unwrap();
        let mut me = ChannelChunker::new(CHANNEL_ME, Box::new(EnergySpeech));
        let mut chunks = collect(&mut me, &speech_silence_speech());
        let chunk = chunks.pop().unwrap();
        let before = POST_PROCESS_CALLS.load(Ordering::SeqCst);
        let text = commit_chunk(&tm, &store, meeting_id, chunk).unwrap();
        assert_eq!(POST_PROCESS_CALLS.load(Ordering::SeqCst), before);
        let rows = store.segments(meeting_id).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].text, text);
        assert_eq!(rows[0].channel, CHANNEL_ME);
    }

    #[test]
    fn voxtral_only_refuses_start() {
        let _lock = test_lock();
        let _reset = ResetMeeting;
        let dir = tempfile::tempdir().unwrap();
        let store = MeetingsStore::open(dir.path()).unwrap();
        let downloaded = vec![VOXTRAL.to_string(), QWEN.to_string()];
        let err = begin_meeting(&store, VOXTRAL, &downloaded, "Standup", &|_| None).unwrap_err();
        match err {
            BeginMeetingError::Refused(MeetingStartRefusal::OnlyVoxtral { model_id }) => {
                assert!(
                    model_id.contains("Voxtral-Mini-4B-Realtime-2602"),
                    "{model_id}"
                );
                assert!(err_display_names_model(&model_id));
            }
            BeginMeetingError::Store(message) => panic!("store error: {message}"),
        }
        assert_eq!(store.meeting_count().unwrap(), 0);
        assert!(!crate::managers::meetings::meeting_is_active());
    }

    fn err_display_names_model(model_id: &str) -> bool {
        MeetingStartRefusal::OnlyVoxtral {
            model_id: model_id.to_string(),
        }
        .to_string()
        .contains(model_id)
    }
}
