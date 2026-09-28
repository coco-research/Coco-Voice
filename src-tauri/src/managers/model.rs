use super::model_capabilities::{
    CapabilityProbe, CapabilityProber, Compatibility, GgufHeaderProber,
};
use crate::settings::{get_settings, write_settings};
use anyhow::Result;
use flate2::read::GzDecoder;
use futures_util::StreamExt;
use hf_hub::api::tokio::{ApiBuilder, CancellationToken, Progress};
use hf_hub::{Cache, Repo, RepoType};
use log::{debug, info, warn};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use specta::Type;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tar::Archive;
use tauri::{AppHandle, Emitter, Manager};

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub enum EngineType {
    /// Any GGML/GGUF model loaded through transcribe-cpp (Whisper, Parakeet,
    /// Voxtral, Qwen3-ASR, Nemotron, …). The architecture is auto-detected from
    /// the file, so this one variant covers the whole transcribe-cpp family.
    TranscribeCpp,
    Parakeet,
    Moonshine,
    MoonshineStreaming,
    SenseVoice,
    GigaAM,
    Canary,
    Cohere,
}

/// Where a model comes from and how Coco Voice obtains it.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct RemoteFile {
    pub url: String,
    pub local_name: String,
    pub sha256: String,
    pub size: u64,
}

/// Where a model comes from and how Coco Voice obtains it — the routing discriminant
/// for downloading and on-disk resolution.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub enum ModelSource {
    /// Direct HTTP download from a pinned URL.
    Url {
        url: String,
        /// Expected SHA-256 for integrity verification; `None` skips it.
        sha256: Option<String>,
    },
    /// Several pinned files saved into `filename/` with the old tarball layout.
    Files { files: Vec<RemoteFile> },
    /// A file inside a Hugging Face Hub repo, fetched via hf-hub into the shared
    /// HF cache. The file within the repo is [`ModelInfo::filename`], unless
    /// `path` is set.
    HuggingFace {
        repo_id: String,
        revision: String,
        /// Repo-relative path. Empty means [`ModelInfo::filename`].
        #[serde(default)]
        path: String,
        /// Expected SHA-256. `None` skips verification (cache-discovered copies).
        #[serde(default)]
        sha256: Option<String>,
        /// Previous repo id, so an already-downloaded copy still resolves.
        #[serde(default)]
        legacy_repo_id: Option<String>,
    },
    /// Already present on disk — a user-provided custom model, or one discovered
    /// in a shared cache. Nothing to download.
    Local,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub filename: String,
    pub source: ModelSource,
    pub size_mb: u64,
    pub is_downloaded: bool,
    pub is_downloading: bool,
    pub partial_size: u64,
    pub is_directory: bool,
    pub engine_type: EngineType,
    pub accuracy_score: f32,        // 0.0 to 1.0, higher is more accurate
    pub speed_score: f32,           // 0.0 to 1.0, higher is faster
    pub supports_translation: bool, // Whether the model supports translating to English
    pub is_recommended: bool,       // Whether this is the recommended model for new users
    pub supported_languages: Vec<String>, // Languages this model can transcribe
    pub supports_language_selection: bool, // Whether the user can explicitly pick a language
    pub is_custom: bool,            // Whether this is a user-provided custom model
    pub supports_streaming: bool, // Whether this model supports live streaming preview (transcribe-cpp)
    pub supports_language_detection: bool, // Whether the model can auto-detect language (gates the "Auto" option)
}

const CHINESE_LANGUAGE_CODE: &str = "zh";

fn recognition_language(language: &str) -> &str {
    match language {
        "zh-Hans" | "zh-Hant" => CHINESE_LANGUAGE_CODE,
        other => other,
    }
}

/// The base code Coco Voice matches a language *intent* on: a tag's primary subtag,
/// with any BCP-47 region or script suffix dropped (`en-US` → `en`, `zh-CN` →
/// `zh`, `zh-Hant` → `zh`). Bare and three-letter codes (`haw`) pass through
/// unchanged. Lets a bare intent (`en`) match a model that advertises full
/// locales (`en-US`) without discarding the real code the engine needs.
fn base_language(language: &str) -> &str {
    match language.split_once('-') {
        Some((base, _)) => base,
        None => language,
    }
}

fn canonicalize_supported_languages(languages: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut canonical = Vec::with_capacity(languages.len());

    for language in languages {
        let language = recognition_language(&language).to_string();
        if seen.insert(language.clone()) {
            canonical.push(language);
        }
    }

    canonical
}

/// One downloadable quantization of a model. Mirrors a `files[]` entry in
/// `catalog.json`, so it deserializes straight from the catalog.
#[derive(Debug, Clone, Deserialize)]
pub struct QuantFile {
    pub filename: String,
    pub quant: String,
    pub size_bytes: u64,
}

/// Pick the default quant among `files`: the one whose `quant` matches
/// `default_quant`, else the first file. The single source of the "which file do
/// we surface" rule — shared by [`ModelDescriptor::default_file`] and the
/// catalog's id construction so the two can never drift.
pub(crate) fn default_quant_file<'a>(
    files: &'a [QuantFile],
    default_quant: Option<&str>,
) -> Option<&'a QuantFile> {
    files
        .iter()
        .find(|f| Some(f.quant.as_str()) == default_quant)
        .or_else(|| files.first())
}

/// Live, on-disk status — the half of [`ModelInfo`] that isn't part of the
/// static spec. Kept separate so a descriptor stays purely descriptive and
/// status can be recomputed without rebuilding it.
#[derive(Debug, Clone, Default)]
pub struct DiskStatus {
    pub is_downloaded: bool,
    pub is_downloading: bool,
    pub partial_size: u64,
}

/// The spec of a bundled catalog model: everything in `catalog.json` normalised
/// into one shape, rendered into the frontend-facing [`ModelInfo`] via
/// [`ModelDescriptor::to_model_info`] by combining it with a [`DiskStatus`].
/// (The catalog is the only producer that routes through this; the legacy table
/// and on-disk scans build `ModelInfo` directly.)
#[derive(Debug, Clone)]
pub struct ModelDescriptor {
    pub id: String,
    pub source: ModelSource,
    pub name: String,
    pub description: String,
    pub engine_type: EngineType,
    pub caps: CapabilityProbe,
    pub files: Vec<QuantFile>,
    pub default_quant: Option<String>,
    pub speed_score: f32,
    pub accuracy_score: f32,
    /// Editorial sort priority across the whole catalog (lower = higher). Drives
    /// list ordering; independent of `recommended`.
    pub recommended_rank: Option<u32>,
    /// Whether this is part of the small curated set shown to new users in
    /// onboarding (and badged "Recommended"). A model can be ranked for ordering
    /// without being in this set.
    pub recommended: bool,
}

impl ModelDescriptor {
    /// The quant we surface for download/size: the declared default, else the
    /// first file.
    fn default_file(&self) -> Option<&QuantFile> {
        default_quant_file(&self.files, self.default_quant.as_deref())
    }

    /// Render the frontend-facing [`ModelInfo`] by combining this spec with live
    /// disk `status`.
    pub fn to_model_info(&self, status: &DiskStatus) -> ModelInfo {
        let file = self.default_file();
        let languages =
            canonicalize_supported_languages(self.caps.languages.clone().unwrap_or_default());
        ModelInfo {
            id: self.id.clone(),
            name: self.name.clone(),
            description: self.description.clone(),
            filename: file.map(|f| f.filename.clone()).unwrap_or_default(),
            source: self.source.clone(),
            size_mb: file.map(|f| f.size_bytes / (1024 * 1024)).unwrap_or(0),
            is_downloaded: status.is_downloaded,
            is_downloading: status.is_downloading,
            partial_size: status.partial_size,
            is_directory: false,
            engine_type: self.engine_type.clone(),
            accuracy_score: self.accuracy_score,
            speed_score: self.speed_score,
            supports_translation: self.caps.supports_translation.unwrap_or(false),
            is_recommended: self.recommended,
            supports_language_selection: languages.len() > 1,
            supported_languages: languages,
            // Catalog models are always HF-sourced downloads, never user-dropped
            // custom files (those bypass the descriptor and set this directly).
            is_custom: false,
            supports_streaming: self.caps.supports_streaming.unwrap_or(false),
            supports_language_detection: self.caps.supports_language_detect.unwrap_or(false),
        }
    }
}

/// Resolve the user's persisted language *intent* (`"auto"` or a language code)
/// into the language a given model will actually use.
///
/// The canonical coercion used on every transcription path: computed at the
/// point of use and **never written back** to settings, so the user's last
/// explicit intent survives switching to an incompatible model and back.
///
/// Matching is base-aware ([`base_language`]) and returns the model's own
/// *concrete* code, so a bare intent (`en`) resolves to the exact string the
/// engine's prompt table expects (`en-US`) for models that advertise full
/// BCP-47 locales. Chinese *script* intents (`zh-Hans`/`zh-Hant`) are the sole
/// exception: they pass through unchanged so the downstream Simplified /
/// Traditional output conversion still fires (the engine path collapses them to
/// a plain Chinese code separately).
pub fn effective_language(
    intent: &str,
    supported_languages: &[String],
    supports_language_detection: bool,
) -> String {
    if supported_languages.is_empty() {
        return intent.to_string();
    }

    if intent != "auto" {
        if let Some(code) = supported_languages
            .iter()
            .find(|language| base_language(language) == base_language(intent))
        {
            if intent == "zh-Hans" || intent == "zh-Hant" {
                return intent.to_string();
            }
            return code.clone();
        }
    }

    if supports_language_detection {
        return "auto".to_string();
    }

    // Model can't auto-detect and the intent isn't usable: fall back to a
    // concrete language (prefer English) so we never hand the engine "auto".
    if let Some(en) = supported_languages
        .iter()
        .find(|language| base_language(language) == "en")
    {
        return en.clone();
    }
    recognition_language(&supported_languages[0]).to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct DownloadProgress {
    pub model_id: String,
    pub downloaded: u64,
    pub total: u64,
    pub percentage: f64,
}

/// Shared mirror repo. Deleting one model must not wipe the others.
const MIRROR_REPO: &str = crate::catalog::MIRROR_REPO;

/// Same lookup as hf-hub `CacheRepo::get`: `refs/<revision>` then
/// `snapshots/<commit>/<filename>`. `filename` may contain slashes.
fn cache_file(cache_root: &Path, repo_id: &str, revision: &str, filename: &str) -> Option<PathBuf> {
    if repo_id.is_empty() || filename.is_empty() {
        return None;
    }
    let folder = format!("models--{}", repo_id.replace('/', "--"));
    let repo = cache_root.join(folder);
    let commit = fs::read_to_string(repo.join("refs").join(revision)).ok()?;
    let path = repo.join("snapshots").join(commit.trim()).join(filename);
    path.exists().then_some(path)
}

/// Resolve a mirrored or publisher HF file. An already-downloaded legacy copy
/// counts as present and is not re-hashed: older exports can differ in sha.
fn resolve_hf_file(
    cache_root: &Path,
    repo_id: &str,
    revision: &str,
    repo_path: &str,
    filename: &str,
    legacy_repo_id: Option<&str>,
) -> Option<PathBuf> {
    let primary = if repo_path.is_empty() {
        filename
    } else {
        repo_path
    };
    if let Some(path) = cache_file(cache_root, repo_id, revision, primary) {
        return Some(path);
    }
    let legacy = legacy_repo_id.filter(|id| !id.is_empty() && *id != repo_id)?;
    cache_file(cache_root, legacy, "main", filename)
}

fn resolve_hf_in_env(
    repo_id: &str,
    revision: &str,
    repo_path: &str,
    filename: &str,
    legacy_repo_id: Option<&str>,
) -> Option<PathBuf> {
    resolve_hf_file(
        Cache::from_env().path(),
        repo_id,
        revision,
        repo_path,
        filename,
        legacy_repo_id,
    )
}

fn hf_repo_dir(file: &Path) -> Option<PathBuf> {
    file.ancestors()
        .find(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("models--"))
        })
        .map(|p| p.to_path_buf())
}

/// Drop the snapshot pointer and the blob it points at. A sha mismatch must not
/// leave the blob, or the next hf-hub download reuses it by etag.
fn delete_pointer_and_blob(pointer: &Path) {
    if let Ok(target) = fs::read_link(pointer) {
        let blob = if target.is_absolute() {
            target
        } else {
            pointer.parent().unwrap_or(pointer).join(target)
        };
        let _ = fs::remove_file(pointer);
        let _ = fs::remove_file(blob);
    } else {
        let _ = fs::remove_file(pointer);
    }
}

fn dir_bytes(path: &Path) -> u64 {
    let mut total = 0u64;
    let Ok(entries) = fs::read_dir(path) else {
        return 0;
    };
    for entry in entries.flatten() {
        if entry.path().is_file() {
            total += entry.metadata().map(|m| m.len()).unwrap_or(0);
        }
    }
    total
}

fn apply_license_notice(description: &mut String, repo_id: &str, filename: &str) {
    if let Some(notice) = crate::catalog::license_notice(repo_id, filename) {
        *description = notice;
    }
}

/// Friendly name advertised by GGUF metadata, if present. Empty strings are not
/// useful display names, so callers can keep their filename/repo fallback.
fn probed_display_name(probe: &CapabilityProbe) -> Option<String> {
    probe
        .display_name
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
}

/// Capability fields for a locally-discovered on-disk model, derived from its
/// GGUF header probe. Anything without readable GGUF metadata — a legacy `.bin`
/// file, or a header that simply omits a key — collapses to "no advertised
/// capability"; transcribe-cpp still reconciles the real values at load time.
/// Shared by both local discovery paths (custom models dir + HF cache) so they
/// surface capabilities identically.
struct LocalCaps {
    supports_streaming: bool,
    supports_translation: bool,
    supports_language_selection: bool,
    supports_language_detection: bool,
    supported_languages: Vec<String>,
}

fn local_caps(probe: &CapabilityProbe) -> LocalCaps {
    let languages = canonicalize_supported_languages(probe.languages.clone().unwrap_or_default());
    LocalCaps {
        supports_streaming: probe.supports_streaming.unwrap_or(false),
        supports_translation: probe.supports_translation.unwrap_or(false),
        // Only offer a language picker when there's more than one to choose.
        supports_language_selection: languages.len() > 1,
        supports_language_detection: probe.supports_language_detect.unwrap_or(false),
        supported_languages: languages,
    }
}

/// Bridges hf-hub's async download progress to Coco Voice's `model-download-progress`
/// event. hf-hub clones the reporter, so shared state lives behind an `Arc`.
#[derive(Clone)]
struct HfDownloadProgress {
    app_handle: AppHandle,
    model_id: String,
    state: Arc<Mutex<HfProgressState>>,
}

struct HfProgressState {
    total: u64,
    downloaded: u64,
    last_emit: Instant,
}

impl HfDownloadProgress {
    fn new(app_handle: AppHandle, model_id: String) -> Self {
        Self {
            app_handle,
            model_id,
            state: Arc::new(Mutex::new(HfProgressState {
                total: 0,
                downloaded: 0,
                last_emit: Instant::now(),
            })),
        }
    }

    fn emit(&self, downloaded: u64, total: u64) {
        let percentage = if total > 0 {
            (downloaded as f64 / total as f64) * 100.0
        } else {
            0.0
        };
        let _ = self.app_handle.emit(
            "model-download-progress",
            &DownloadProgress {
                model_id: self.model_id.clone(),
                downloaded,
                total,
                percentage,
            },
        );
    }
}

impl Progress for HfDownloadProgress {
    async fn init(&mut self, size: usize, _filename: &str) {
        {
            let mut st = self.state.lock().unwrap();
            st.total = size as u64;
            st.downloaded = 0;
            st.last_emit = Instant::now();
        }
        self.emit(0, size as u64);
    }

    async fn update(&mut self, size: usize) {
        let (downloaded, total, emit) = {
            let mut st = self.state.lock().unwrap();
            st.downloaded = st.downloaded.saturating_add(size as u64);
            let now = Instant::now();
            // Throttle to ~10 updates/sec, but always emit the final byte.
            let emit = now.duration_since(st.last_emit) >= Duration::from_millis(100)
                || (st.total > 0 && st.downloaded >= st.total);
            if emit {
                st.last_emit = now;
            }
            (st.downloaded, st.total, emit)
        };
        if emit {
            self.emit(downloaded, total);
        }
    }

    async fn finish(&mut self) {
        let total = {
            let st = self.state.lock().unwrap();
            st.total.max(st.downloaded)
        };
        self.emit(total, total);
    }
}

/// RAII guard that clears the `is_rescanning` single-flight flag on drop, so the
/// slot is released on every exit path (including early returns and `?`).
struct RescanGuard {
    flag: Arc<AtomicBool>,
}

impl Drop for RescanGuard {
    fn drop(&mut self) {
        self.flag.store(false, Ordering::SeqCst);
    }
}

/// RAII guard that cleans up download state (`is_downloading` flag and cancel flag)
/// when dropped, unless explicitly disarmed. This ensures consistent cleanup on
/// every error path without requiring manual cleanup at each `?` or `return Err`.
struct DownloadCleanup<'a> {
    available_models: &'a Mutex<HashMap<String, ModelInfo>>,
    cancel_flags: &'a Arc<Mutex<HashMap<String, CancellationToken>>>,
    model_id: String,
    disarmed: bool,
}

impl<'a> Drop for DownloadCleanup<'a> {
    fn drop(&mut self) {
        if self.disarmed {
            return;
        }
        {
            let mut models = self.available_models.lock().unwrap();
            if let Some(model) = models.get_mut(self.model_id.as_str()) {
                model.is_downloading = false;
            }
        }
        self.cancel_flags.lock().unwrap().remove(&self.model_id);
    }
}

pub struct ModelManager {
    app_handle: AppHandle,
    models_dir: PathBuf,
    available_models: Mutex<HashMap<String, ModelInfo>>,
    cancel_flags: Arc<Mutex<HashMap<String, CancellationToken>>>,
    extracting_models: Arc<Mutex<HashSet<String>>>,
    /// Single-flight guard for [`Self::rescan_local_models`] so concurrent
    /// refresh requests coalesce instead of scanning the disk in parallel.
    is_rescanning: Arc<AtomicBool>,
}

impl ModelManager {
    pub fn new(app_handle: &AppHandle) -> Result<Self> {
        // Create models directory in app data
        let models_dir = crate::portable::app_data_dir(app_handle)
            .map_err(|e| anyhow::anyhow!("Failed to get app data dir: {}", e))?
            .join("models");

        if !models_dir.exists() {
            fs::create_dir_all(&models_dir)?;
        }

        let mut available_models = HashMap::new();

        // Whisper supported languages (99 languages from tokenizer)
        let whisper_languages: Vec<String> = vec![
            "en", "zh", "de", "es", "ru", "ko", "fr", "ja", "pt", "tr", "pl", "ca", "nl", "ar",
            "sv", "it", "id", "hi", "fi", "vi", "he", "uk", "el", "ms", "cs", "ro", "da", "hu",
            "ta", "no", "th", "ur", "hr", "bg", "lt", "la", "mi", "ml", "cy", "sk", "te", "fa",
            "lv", "bn", "sr", "az", "sl", "kn", "et", "mk", "br", "eu", "is", "hy", "ne", "mn",
            "bs", "kk", "sq", "sw", "gl", "mr", "pa", "si", "km", "sn", "yo", "so", "af", "oc",
            "ka", "be", "tg", "sd", "gu", "am", "yi", "lo", "uz", "fo", "ht", "ps", "tk", "nn",
            "mt", "sa", "lb", "my", "bo", "tl", "mg", "as", "tt", "haw", "ln", "ha", "ba", "jw",
            "su", "yue",
        ]
        .into_iter()
        .map(String::from)
        .collect();

        available_models.insert(
            "small".to_string(),
            ModelInfo {
                id: "small".to_string(),
                name: "Whisper Small".to_string(),
                description: "Fast and fairly accurate.".to_string(),
                filename: "ggml-small.bin".to_string(),
                source: ModelSource::Url {
                    url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/5359861c739e955e79d9a303bcbc70fb988958b1/ggml-small.bin".to_string(),
                    sha256: Some(
                        "1be3a9b2063867b937e64e2ec7483364a79917e157fa98c5d94b5c1fffea987b"
                            .to_string(),
                    ),
                },
                size_mb: 465,
                is_downloaded: false,
                is_downloading: false,
                partial_size: 0,
                is_directory: false,
                engine_type: EngineType::TranscribeCpp,
                accuracy_score: 0.60,
                speed_score: 0.85,
                supports_translation: true,
                is_recommended: false,
                supported_languages: whisper_languages.clone(),
                supports_language_selection: true,
                is_custom: false,
                supports_streaming: false,
                supports_language_detection: true,
            },
        );

        // Add downloadable models
        available_models.insert(
            "medium".to_string(),
            ModelInfo {
                id: "medium".to_string(),
                name: "Whisper Medium".to_string(),
                description: "Good accuracy, medium speed".to_string(),
                filename: "whisper-medium-q4_1.bin".to_string(),
                source: ModelSource::Url {
                    url: "https://huggingface.co/coco-research/coco-voice-models/resolve/3cb3e6de7c20f58fe5beaaab3d5159384feff3f0/medium/whisper-medium-q4_1.bin".to_string(),
                    sha256: Some(
                        "79283fc1f9fe12ca3248543fbd54b73292164d8df5a16e095e2bceeaaabddf57"
                            .to_string(),
                    ),
                },
                size_mb: 469,
                is_downloaded: false,
                is_downloading: false,
                partial_size: 0,
                is_directory: false,
                engine_type: EngineType::TranscribeCpp,
                accuracy_score: 0.75,
                speed_score: 0.60,
                supports_translation: true,
                is_recommended: false,
                supported_languages: whisper_languages.clone(),
                supports_language_selection: true,
                is_custom: false,
                supports_streaming: false,
                supports_language_detection: true,
            },
        );

        available_models.insert(
            "turbo".to_string(),
            ModelInfo {
                id: "turbo".to_string(),
                name: "Whisper Turbo".to_string(),
                description: "Balanced accuracy and speed.".to_string(),
                filename: "ggml-large-v3-turbo.bin".to_string(),
                source: ModelSource::Url {
                    url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/5359861c739e955e79d9a303bcbc70fb988958b1/ggml-large-v3-turbo.bin".to_string(),
                    sha256: Some(
                        "1fc70f774d38eb169993ac391eea357ef47c88757ef72ee5943879b7e8e2bc69"
                            .to_string(),
                    ),
                },
                size_mb: 1549,
                is_downloaded: false,
                is_downloading: false,
                partial_size: 0,
                is_directory: false,
                engine_type: EngineType::TranscribeCpp,
                accuracy_score: 0.80,
                speed_score: 0.40,
                supports_translation: false, // Turbo doesn't support translation
                is_recommended: false,
                supported_languages: whisper_languages.clone(),
                supports_language_selection: true,
                is_custom: false,
                supports_streaming: false,
                supports_language_detection: true,
            },
        );

        available_models.insert(
            "large".to_string(),
            ModelInfo {
                id: "large".to_string(),
                name: "Whisper Large".to_string(),
                description: "Good accuracy, but slow.".to_string(),
                filename: "ggml-large-v3-q5_0.bin".to_string(),
                source: ModelSource::Url {
                    url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/5359861c739e955e79d9a303bcbc70fb988958b1/ggml-large-v3-q5_0.bin".to_string(),
                    sha256: Some(
                        "d75795ecff3f83b5faa89d1900604ad8c780abd5739fae406de19f23ecd98ad1"
                            .to_string(),
                    ),
                },
                size_mb: 1031,
                is_downloaded: false,
                is_downloading: false,
                partial_size: 0,
                is_directory: false,
                engine_type: EngineType::TranscribeCpp,
                accuracy_score: 0.85,
                speed_score: 0.30,
                supports_translation: true,
                is_recommended: false,
                supported_languages: whisper_languages.clone(),
                supports_language_selection: true,
                is_custom: false,
                supports_streaming: false,
                supports_language_detection: true,
            },
        );

        available_models.insert(
            "breeze-asr".to_string(),
            ModelInfo {
                id: "breeze-asr".to_string(),
                name: "Breeze ASR".to_string(),
                description: "Optimized for Taiwanese Mandarin. Code-switching support."
                    .to_string(),
                filename: "breeze-asr-q5_k.bin".to_string(),
                source: ModelSource::Url {
                    url: "https://huggingface.co/coco-research/coco-voice-models/resolve/3cb3e6de7c20f58fe5beaaab3d5159384feff3f0/breeze-asr/breeze-asr-q5_k.bin".to_string(),
                    sha256: Some(
                        "8efbf0ce8a3f50fe332b7617da787fb81354b358c288b008d3bdef8359df64c6"
                            .to_string(),
                    ),
                },
                size_mb: 1030,
                is_downloaded: false,
                is_downloading: false,
                partial_size: 0,
                is_directory: false,
                engine_type: EngineType::TranscribeCpp,
                accuracy_score: 0.85,
                speed_score: 0.35,
                supports_translation: false,
                is_recommended: false,
                supported_languages: whisper_languages,
                supports_language_selection: true,
                is_custom: false,
                supports_streaming: false,
                supports_language_detection: true,
            },
        );

        // Add NVIDIA Parakeet models (directory-based)
        available_models.insert(
            "parakeet-tdt-0.6b-v2".to_string(),
            ModelInfo {
                id: "parakeet-tdt-0.6b-v2".to_string(),
                name: "Parakeet V2".to_string(),
                description: "English only. The best model for English speakers.".to_string(),
                filename: "parakeet-tdt-0.6b-v2-int8".to_string(), // Directory name
                source: ModelSource::Files {
                    files: super::model_sources::files(super::model_sources::PARAKEET_V2),
                },
                size_mb: super::model_sources::total_mb(super::model_sources::PARAKEET_V2),
                is_downloaded: false,
                is_downloading: false,
                partial_size: 0,
                is_directory: true,
                engine_type: EngineType::Parakeet,
                accuracy_score: 0.85,
                speed_score: 0.85,
                supports_translation: false,
                is_recommended: false,
                supported_languages: vec!["en".to_string()],
                supports_language_selection: false,
                is_custom: false,
                supports_streaming: false,
                supports_language_detection: true,
            },
        );

        // Parakeet V3 supported languages (25 EU languages + Russian/Ukrainian):
        // bg, hr, cs, da, nl, en, et, fi, fr, de, el, hu, it, lv, lt, mt, pl, pt, ro, sk, sl, es, sv, ru, uk
        let parakeet_v3_languages: Vec<String> = vec![
            "bg", "hr", "cs", "da", "nl", "en", "et", "fi", "fr", "de", "el", "hu", "it", "lv",
            "lt", "mt", "pl", "pt", "ro", "sk", "sl", "es", "sv", "ru", "uk",
        ]
        .into_iter()
        .map(String::from)
        .collect();

        available_models.insert(
            "parakeet-tdt-0.6b-v3".to_string(),
            ModelInfo {
                id: "parakeet-tdt-0.6b-v3".to_string(),
                name: "Parakeet V3".to_string(),
                description: "Fast and accurate. Supports 25 European languages.".to_string(),
                filename: "parakeet-tdt-0.6b-v3-int8".to_string(), // Directory name
                source: ModelSource::Files {
                    files: super::model_sources::files(super::model_sources::PARAKEET_V3),
                },
                size_mb: super::model_sources::total_mb(super::model_sources::PARAKEET_V3),
                is_downloaded: false,
                is_downloading: false,
                partial_size: 0,
                is_directory: true,
                engine_type: EngineType::Parakeet,
                accuracy_score: 0.80,
                speed_score: 0.85,
                supports_translation: false,
                is_recommended: true,
                supported_languages: parakeet_v3_languages,
                supports_language_selection: false,
                is_custom: false,
                supports_streaming: false,
                supports_language_detection: true,
            },
        );

        available_models.insert(
            "moonshine-base".to_string(),
            ModelInfo {
                id: "moonshine-base".to_string(),
                name: "Moonshine Base".to_string(),
                description: "Very fast, English only. Handles accents well.".to_string(),
                filename: "moonshine-base".to_string(),
                source: ModelSource::Files {
                    files: super::model_sources::files(super::model_sources::MOONSHINE_BASE),
                },
                size_mb: super::model_sources::total_mb(super::model_sources::MOONSHINE_BASE),
                is_downloaded: false,
                is_downloading: false,
                partial_size: 0,
                is_directory: true,
                engine_type: EngineType::Moonshine,
                accuracy_score: 0.70,
                speed_score: 0.90,
                supports_translation: false,
                is_recommended: false,
                supported_languages: vec!["en".to_string()],
                supports_language_selection: false,
                is_custom: false,
                supports_streaming: false,
                supports_language_detection: true,
            },
        );

        available_models.insert(
            "moonshine-tiny-streaming-en".to_string(),
            ModelInfo {
                id: "moonshine-tiny-streaming-en".to_string(),
                name: "Moonshine V2 Tiny".to_string(),
                description: "Ultra-fast, English only".to_string(),
                filename: "moonshine-tiny-streaming-en".to_string(),
                source: ModelSource::Url {
                    url: "https://huggingface.co/coco-research/coco-voice-models/resolve/3cb3e6de7c20f58fe5beaaab3d5159384feff3f0/moonshine-tiny-streaming-en/moonshine-tiny-streaming-en.tar.gz"
                        .to_string(),
                    sha256: Some(
                        "465addcfca9e86117415677dfdc98b21edc53537210333a3ecdb58509a80abaf"
                            .to_string(),
                    ),
                },
                size_mb: 31,
                is_downloaded: false,
                is_downloading: false,
                partial_size: 0,
                is_directory: true,
                engine_type: EngineType::MoonshineStreaming,
                accuracy_score: 0.55,
                speed_score: 0.95,
                supports_translation: false,
                is_recommended: false,
                supported_languages: vec!["en".to_string()],
                supports_language_selection: false,
                is_custom: false,
                supports_streaming: false,
                supports_language_detection: true,
            },
        );

        available_models.insert(
            "moonshine-small-streaming-en".to_string(),
            ModelInfo {
                id: "moonshine-small-streaming-en".to_string(),
                name: "Moonshine V2 Small".to_string(),
                description: "Fast, English only. Good balance of speed and accuracy.".to_string(),
                filename: "moonshine-small-streaming-en".to_string(),
                source: ModelSource::Url {
                    url: "https://huggingface.co/coco-research/coco-voice-models/resolve/3cb3e6de7c20f58fe5beaaab3d5159384feff3f0/moonshine-small-streaming-en/moonshine-small-streaming-en.tar.gz"
                        .to_string(),
                    sha256: Some(
                        "dbb3e1c1832bd88a4ac712f7449a136cc2c9a18c5fe33a12ed1b7cb1cfe9cdd5"
                            .to_string(),
                    ),
                },
                size_mb: 99,
                is_downloaded: false,
                is_downloading: false,
                partial_size: 0,
                is_directory: true,
                engine_type: EngineType::MoonshineStreaming,
                accuracy_score: 0.65,
                speed_score: 0.90,
                supports_translation: false,
                is_recommended: false,
                supported_languages: vec!["en".to_string()],
                supports_language_selection: false,
                is_custom: false,
                supports_streaming: false,
                supports_language_detection: true,
            },
        );

        available_models.insert(
            "moonshine-medium-streaming-en".to_string(),
            ModelInfo {
                id: "moonshine-medium-streaming-en".to_string(),
                name: "Moonshine V2 Medium".to_string(),
                description: "English only. High quality.".to_string(),
                filename: "moonshine-medium-streaming-en".to_string(),
                source: ModelSource::Url {
                    url: "https://huggingface.co/coco-research/coco-voice-models/resolve/3cb3e6de7c20f58fe5beaaab3d5159384feff3f0/moonshine-medium-streaming-en/moonshine-medium-streaming-en.tar.gz"
                        .to_string(),
                    sha256: Some(
                        "07a66f3bff1c77e75a2f637e5a263928a08baae3c29c4c053fc968a9a9373d13"
                            .to_string(),
                    ),
                },
                size_mb: 192,
                is_downloaded: false,
                is_downloading: false,
                partial_size: 0,
                is_directory: true,
                engine_type: EngineType::MoonshineStreaming,
                accuracy_score: 0.75,
                speed_score: 0.80,
                supports_translation: false,
                is_recommended: false,
                supported_languages: vec!["en".to_string()],
                supports_language_selection: false,
                is_custom: false,
                supports_streaming: false,
                supports_language_detection: true,
            },
        );

        // SenseVoice supported languages
        let sense_voice_languages: Vec<String> = vec!["zh", "en", "yue", "ja", "ko"]
            .into_iter()
            .map(String::from)
            .collect();

        available_models.insert(
            "sense-voice-int8".to_string(),
            ModelInfo {
                id: "sense-voice-int8".to_string(),
                name: "SenseVoice".to_string(),
                description: "Very fast. Chinese, English, Japanese, Korean, Cantonese."
                    .to_string(),
                filename: "sense-voice-int8".to_string(),
                source: ModelSource::Files {
                    files: super::model_sources::files(super::model_sources::SENSE_VOICE),
                },
                size_mb: super::model_sources::total_mb(super::model_sources::SENSE_VOICE),
                is_downloaded: false,
                is_downloading: false,
                partial_size: 0,
                is_directory: true,
                engine_type: EngineType::SenseVoice,
                accuracy_score: 0.65,
                speed_score: 0.95,
                supports_translation: false,
                is_recommended: false,
                supported_languages: sense_voice_languages,
                supports_language_selection: true,
                is_custom: false,
                supports_streaming: false,
                supports_language_detection: true,
            },
        );

        // GigaAM v3 supported languages
        let gigaam_languages: Vec<String> = vec!["ru"].into_iter().map(String::from).collect();

        available_models.insert(
            "gigaam-v3-e2e-ctc".to_string(),
            ModelInfo {
                id: "gigaam-v3-e2e-ctc".to_string(),
                name: "GigaAM v3".to_string(),
                description: "Russian speech recognition. Fast and accurate.".to_string(),
                filename: "giga-am-v3-int8".to_string(),
                source: ModelSource::Files {
                    files: super::model_sources::files(super::model_sources::GIGAAM_V3),
                },
                size_mb: super::model_sources::total_mb(super::model_sources::GIGAAM_V3),
                is_downloaded: false,
                is_downloading: false,
                partial_size: 0,
                is_directory: true,
                engine_type: EngineType::GigaAM,
                accuracy_score: 0.85,
                speed_score: 0.75,
                supports_translation: false,
                is_recommended: false,
                supported_languages: gigaam_languages,
                supports_language_selection: false,
                is_custom: false,
                supports_streaming: false,
                supports_language_detection: true,
            },
        );

        // Canary 180m Flash supported languages (4 languages)
        let canary_flash_languages: Vec<String> = vec!["en", "de", "es", "fr"]
            .into_iter()
            .map(String::from)
            .collect();

        available_models.insert(
            "canary-180m-flash".to_string(),
            ModelInfo {
                id: "canary-180m-flash".to_string(),
                name: "Canary 180M Flash".to_string(),
                description: "Very fast. English, German, Spanish, French. Supports translation."
                    .to_string(),
                filename: "canary-180m-flash".to_string(),
                source: ModelSource::Files {
                    files: super::model_sources::files(super::model_sources::CANARY_180M),
                },
                size_mb: super::model_sources::total_mb(super::model_sources::CANARY_180M),
                is_downloaded: false,
                is_downloading: false,
                partial_size: 0,
                is_directory: true,
                engine_type: EngineType::Canary,
                accuracy_score: 0.75,
                speed_score: 0.85,
                supports_translation: true,
                is_recommended: false,
                supported_languages: canary_flash_languages,
                supports_language_selection: true,
                is_custom: false,
                supports_streaming: false,
                // Canary (NeMo) requires an explicit source language — no auto-detect.
                supports_language_detection: false,
            },
        );

        // Canary 1B v2 supported languages (25 EU languages)
        let canary_1b_languages: Vec<String> = vec![
            "bg", "hr", "cs", "da", "nl", "en", "et", "fi", "fr", "de", "el", "hu", "it", "lv",
            "lt", "mt", "pl", "pt", "ro", "sk", "sl", "es", "sv", "ru", "uk",
        ]
        .into_iter()
        .map(String::from)
        .collect();

        available_models.insert(
            "canary-1b-v2".to_string(),
            ModelInfo {
                id: "canary-1b-v2".to_string(),
                name: "Canary 1B v2".to_string(),
                description: "Accurate multilingual. 25 European languages. Supports translation."
                    .to_string(),
                filename: "canary-1b-v2".to_string(),
                source: ModelSource::Files {
                    files: super::model_sources::files(super::model_sources::CANARY_1B_V2),
                },
                size_mb: super::model_sources::total_mb(super::model_sources::CANARY_1B_V2),
                is_downloaded: false,
                is_downloading: false,
                partial_size: 0,
                is_directory: true,
                engine_type: EngineType::Canary,
                accuracy_score: 0.85,
                speed_score: 0.70,
                supports_translation: true,
                is_recommended: false,
                supported_languages: canary_1b_languages,
                supports_language_selection: true,
                is_custom: false,
                supports_streaming: false,
                // Canary (NeMo) requires an explicit source language — no auto-detect.
                supports_language_detection: false,
            },
        );

        let cohere_languages: Vec<String> = vec![
            "en", "fr", "de", "it", "es", "pt", "el", "nl", "pl", "zh", "ja", "ko", "vi", "ar",
        ]
        .into_iter()
        .map(String::from)
        .collect();

        available_models.insert(
            "cohere-int8".to_string(),
            ModelInfo {
                id: "cohere-int8".to_string(),
                name: "Cohere".to_string(),
                description: "A large, slower, but very accurate multilingual model.".to_string(),
                filename: "cohere-int8".to_string(),
                source: ModelSource::Files {
                    files: super::model_sources::files(super::model_sources::COHERE),
                },
                size_mb: super::model_sources::total_mb(super::model_sources::COHERE),
                is_downloaded: false,
                is_downloading: false,
                partial_size: 0,
                is_directory: true,
                engine_type: EngineType::Cohere,
                accuracy_score: 0.90,
                speed_score: 0.60,
                supports_translation: false,
                is_recommended: false,
                supported_languages: cohere_languages,
                supports_language_selection: true,
                is_custom: false,
                supports_streaming: false,
                supports_language_detection: true,
            },
        );

        // Seed the bundled offline catalog before the on-disk scans, so a model
        // already in the HF cache dedups onto its richer catalog entry (the scans
        // only insert ids not already present) instead of showing as a bare cache
        // find. Additive — see `seed_catalog_models`.
        Self::seed_catalog_models(&mut available_models);

        // Auto-discover custom transcribe-cpp models (.bin / .gguf) in the models directory
        if let Err(e) = Self::discover_custom_transcribe_models(&models_dir, &mut available_models)
        {
            warn!("Failed to discover custom models: {}", e);
        }

        // Auto-discover transcribe-cpp GGUF models already in the shared HF cache.
        Self::discover_hf_cache_models(&mut available_models);

        let manager = Self {
            app_handle: app_handle.clone(),
            models_dir,
            available_models: Mutex::new(available_models),
            cancel_flags: Arc::new(Mutex::new(HashMap::new())),
            extracting_models: Arc::new(Mutex::new(HashSet::new())),
            is_rescanning: Arc::new(AtomicBool::new(false)),
        };

        // Migrate any bundled models to user directory
        manager.migrate_bundled_models()?;

        // Migrate GigaAM from single-file to directory format
        manager.migrate_gigaam_to_directory()?;

        // Check which models are already downloaded
        manager.update_download_status()?;

        // Auto-select a model if none is currently selected
        manager.auto_select_model_if_needed()?;

        Ok(manager)
    }

    pub fn get_available_models(&self) -> Vec<ModelInfo> {
        let mut list: Vec<ModelInfo> = {
            let models = self.available_models.lock().unwrap();
            models.values().cloned().collect()
        };
        // Stable, reasonable order: catalog editorial rank first (lower = higher
        // priority), then any other recommended model, then by accuracy, speed,
        // and name. `ModelInfo` doesn't carry rank, so resolve it by id from the
        // catalog here.
        list.sort_by(|a, b| {
            crate::catalog::rank_of(&a.id)
                .cmp(&crate::catalog::rank_of(&b.id))
                .then((!a.is_recommended).cmp(&(!b.is_recommended)))
                .then(b.accuracy_score.total_cmp(&a.accuracy_score))
                .then(b.speed_score.total_cmp(&a.speed_score))
                .then_with(|| a.name.cmp(&b.name))
        });
        list
    }

    /// Seed the bundled catalog ([`crate::catalog::CATALOG`]) into the registry,
    /// inserting each model whose id isn't already present (additive).
    ///
    /// Catalog (`.gguf`, `HuggingFace`) and legacy (`.bin`/ONNX, `Url`) entries
    /// stay SEPARATE — different files, ids, and runtimes. Nothing is merged or
    /// removed; the UI just hides not-on-disk `Url` entries to deprecate legacy
    /// downloads, while already-downloaded ones stay runnable. Runs before the
    /// on-disk scans so a cached model dedups onto its catalog entry.
    fn seed_catalog_models(available_models: &mut HashMap<String, ModelInfo>) {
        use std::collections::hash_map::Entry;
        let mut added = 0usize;
        for desc in crate::catalog::CATALOG.iter() {
            if let Entry::Vacant(slot) = available_models.entry(desc.id.clone()) {
                slot.insert(desc.to_model_info(&DiskStatus::default()));
                added += 1;
            }
        }
        info!("Seeded {} catalog model(s) into the registry", added);
    }

    /// Claim the single rescan slot. Returns a guard that releases it on drop,
    /// or `None` if a rescan is already running (callers should just skip).
    fn try_start_rescan(&self) -> Option<RescanGuard> {
        if self.is_rescanning.swap(true, Ordering::SeqCst) {
            None
        } else {
            Some(RescanGuard {
                flag: self.is_rescanning.clone(),
            })
        }
    }

    /// Re-run the local discovery scans (custom models dir + shared HF cache) so
    /// models dropped in or downloaded outside Coco Voice show up without a restart.
    /// The merge is additive: only new ids are inserted, so existing entries keep
    /// their values — including runtime-probed capabilities from
    /// [`Self::set_runtime_capabilities`]. It then runs [`Self::update_download_status`],
    /// which recomputes disk-derived flags for *every* entry; a rescan racing an
    /// in-flight download can briefly clear its `is_downloading`, but the download
    /// continues and the event-driven UI self-corrects.
    ///
    /// The disk walk and 64 KiB header probes run against a cloned snapshot
    /// *off-lock* so readers never block on I/O; only the brief merge takes the
    /// registry lock. Concurrent calls coalesce via [`Self::try_start_rescan`].
    pub fn rescan_local_models(&self) -> Result<()> {
        let _guard = match self.try_start_rescan() {
            Some(g) => g,
            None => {
                debug!("Model rescan already in progress; skipping");
                return Ok(());
            }
        };

        // Snapshot the current registry and discover against the copy off-lock.
        // The discover_* helpers are purely additive (they skip ids already in
        // the map), so the snapshot ends up as {current} ∪ {newly-found}.
        let mut snapshot = self.available_models.lock().unwrap().clone();
        if let Err(e) = Self::discover_custom_transcribe_models(&self.models_dir, &mut snapshot) {
            warn!("Rescan: failed to discover custom models: {}", e);
        }
        Self::discover_hf_cache_models(&mut snapshot);

        // Merge only the genuinely-new ids back into the live registry. `or_insert`
        // leaves every existing entry exactly as it was.
        let mut added = 0usize;
        {
            let mut live = self.available_models.lock().unwrap();
            for (id, info) in snapshot {
                if let std::collections::hash_map::Entry::Vacant(entry) = live.entry(id) {
                    entry.insert(info);
                    added += 1;
                }
            }
        }

        self.update_download_status()?;
        self.auto_select_model_if_needed()?;
        if added > 0 {
            info!("Model rescan discovered {} new model(s)", added);
        }
        let _ = self.app_handle.emit("models-updated", ());
        Ok(())
    }

    pub fn get_model_info(&self, model_id: &str) -> Option<ModelInfo> {
        let models = self.available_models.lock().unwrap();
        models.get(model_id).cloned()
    }

    /// Reconcile a model's advertised capabilities with the ground truth from the
    /// loaded model (transcribe-cpp's GGUF-derived capabilities), overwriting the
    /// pre-download view (catalog metadata or a header probe — see
    /// [`super::model_capabilities`]).
    ///
    /// This corrects the header probe's gaps. It matters most for **streaming**
    /// (transcribe-cpp infers it at load for parakeet/streaming families, where
    /// the flat GGUF key can be absent, and it gates whether streaming is even
    /// attempted — see `actions.rs`) and for **language detection** / the
    /// **supported-language set**, which feed [`effective_language`]; a mislabeled
    /// header would otherwise coerce an "auto" intent to a forced language for good.
    /// Translate is reconciled too for badge accuracy, though run paths re-read it
    /// live regardless.
    pub fn set_runtime_capabilities(
        &self,
        model_id: &str,
        supports_streaming: bool,
        supports_translation: bool,
        supports_language_detection: bool,
        supported_languages: Vec<String>,
    ) {
        let supported_languages = canonicalize_supported_languages(supported_languages);
        let mut models = self.available_models.lock().unwrap();
        if let Some(model) = models.get_mut(model_id) {
            model.supports_streaming = supports_streaming;
            model.supports_translation = supports_translation;
            model.supports_language_detection = supports_language_detection;
            // An empty set means the model is language-agnostic — but it is also
            // what a failed capability read leaves behind, so keep the probed /
            // catalog list rather than blanking a known one to nothing.
            if !supported_languages.is_empty() {
                model.supports_language_selection = supported_languages.len() > 1;
                model.supported_languages = supported_languages;
            }
        }
    }

    fn migrate_bundled_models(&self) -> Result<()> {
        // Check for bundled models and copy them to user directory
        let bundled_models = ["ggml-small.bin"]; // Add other bundled models here if any

        for filename in &bundled_models {
            let bundled_path = self.app_handle.path().resolve(
                format!("resources/models/{}", filename),
                tauri::path::BaseDirectory::Resource,
            );

            if let Ok(bundled_path) = bundled_path {
                if bundled_path.exists() {
                    let user_path = self.models_dir.join(filename);

                    // Only copy if user doesn't already have the model
                    if !user_path.exists() {
                        info!("Migrating bundled model {} to user directory", filename);
                        fs::copy(&bundled_path, &user_path)?;
                        info!("Successfully migrated {}", filename);
                    }
                }
            }
        }

        Ok(())
    }

    /// Migrate GigaAM from the old single-file format (giga-am-v3.int8.onnx)
    /// to the new directory format (giga-am-v3-int8/model.int8.onnx + vocab.txt).
    /// This was required by the transcribe-rs 0.3.x upgrade.
    fn migrate_gigaam_to_directory(&self) -> Result<()> {
        let old_file = self.models_dir.join("giga-am-v3.int8.onnx");
        let new_dir = self.models_dir.join("giga-am-v3-int8");

        if !old_file.exists() || new_dir.exists() {
            return Ok(());
        }

        info!("Migrating GigaAM from single-file to directory format");

        let vocab_path = self
            .app_handle
            .path()
            .resolve(
                "resources/models/gigaam_vocab.txt",
                tauri::path::BaseDirectory::Resource,
            )
            .map_err(|e| anyhow::anyhow!("Failed to resolve GigaAM vocab path: {}", e))?;

        info!(
            "Resolved vocab path: {:?} (exists: {})",
            vocab_path,
            vocab_path.exists()
        );
        info!("Old file: {:?} (exists: {})", old_file, old_file.exists());
        info!("New dir: {:?} (exists: {})", new_dir, new_dir.exists());

        fs::create_dir_all(&new_dir)?;
        fs::rename(&old_file, new_dir.join("model.int8.onnx"))?;
        fs::copy(&vocab_path, new_dir.join("vocab.txt"))?;

        // Clean up old partial file if it exists
        let old_partial = self.models_dir.join("giga-am-v3.int8.onnx.partial");
        if old_partial.exists() {
            let _ = fs::remove_file(&old_partial);
        }

        info!("GigaAM migration complete");
        Ok(())
    }

    fn update_download_status(&self) -> Result<()> {
        let mut models = self.available_models.lock().unwrap();

        for model in models.values_mut() {
            if let ModelSource::HuggingFace {
                repo_id,
                revision,
                path,
                legacy_repo_id,
                ..
            } = &model.source
            {
                model.is_downloaded = resolve_hf_in_env(
                    repo_id,
                    revision,
                    path,
                    &model.filename,
                    legacy_repo_id.as_deref(),
                )
                .is_some();
                model.is_downloading = false;
                model.partial_size = 0;
                continue;
            }
            if model.is_directory {
                // For directory-based models, check if the directory exists
                let model_path = self.models_dir.join(&model.filename);
                let partial_path = self.models_dir.join(format!("{}.partial", &model.filename));
                let extracting_path = self
                    .models_dir
                    .join(format!("{}.extracting", &model.filename));

                // Clean up any leftover .extracting directories from interrupted extractions
                // But only if this model is NOT currently being extracted
                let is_currently_extracting = {
                    let extracting = self.extracting_models.lock().unwrap();
                    extracting.contains(&model.id)
                };
                if extracting_path.exists() && !is_currently_extracting {
                    warn!("Cleaning up interrupted extraction for model: {}", model.id);
                    let _ = fs::remove_dir_all(&extracting_path);
                }

                model.is_downloaded = model_path.exists() && model_path.is_dir();
                model.is_downloading = false;

                // Get partial file size if it exists (for the .tar.gz being downloaded)
                if partial_path.is_dir() {
                    model.partial_size = dir_bytes(&partial_path);
                } else if partial_path.is_file() {
                    model.partial_size = partial_path.metadata().map(|m| m.len()).unwrap_or(0);
                } else {
                    model.partial_size = 0;
                }
            } else {
                // For file-based models (existing logic)
                let model_path = self.models_dir.join(&model.filename);
                let partial_path = self.models_dir.join(format!("{}.partial", &model.filename));

                model.is_downloaded = model_path.exists();
                model.is_downloading = false;

                // Get partial file size if it exists
                if partial_path.exists() {
                    model.partial_size = partial_path.metadata().map(|m| m.len()).unwrap_or(0);
                } else {
                    model.partial_size = 0;
                }
            }
        }

        Ok(())
    }

    fn auto_select_model_if_needed(&self) -> Result<()> {
        let mut settings = get_settings(&self.app_handle);

        // Clear stale selection: selected model is set but doesn't exist
        // in available_models (e.g. deleted custom model file)
        if !settings.selected_model.is_empty() {
            let models = self.available_models.lock().unwrap();
            let exists = models.contains_key(&settings.selected_model);
            drop(models);

            if !exists {
                info!(
                    "Selected model '{}' not found in available models, clearing selection",
                    settings.selected_model
                );
                settings.selected_model = String::new();
                write_settings(&self.app_handle, settings.clone());
            }
        }

        // If onboarding is still pending, do not auto-select just because a
        // compatible model exists on disk or in the shared HF cache. The
        // onboarding model step should present that choice explicitly.
        if !settings.onboarding_completed {
            debug!("Skipping model auto-selection until onboarding is complete");
            return Ok(());
        }

        // If no model is selected, pick the first downloaded one using the same
        // ranked order the UI receives.
        if settings.selected_model.is_empty() {
            if let Some(available_model) = self
                .get_available_models()
                .into_iter()
                .find(|model| model.is_downloaded)
            {
                info!(
                    "Auto-selecting model: {} ({})",
                    available_model.id, available_model.name
                );

                // Update settings with the selected model
                let mut updated_settings = settings;
                updated_settings.selected_model = available_model.id.clone();
                write_settings(&self.app_handle, updated_settings);

                info!("Successfully auto-selected model: {}", available_model.id);
            }
        }

        Ok(())
    }

    /// Discover custom Whisper-family models in the models directory: legacy
    /// GGML `.bin` files and `.gguf` files (both load through transcribe-cpp).
    /// Skips files that match predefined model filenames.
    fn discover_custom_transcribe_models(
        models_dir: &Path,
        available_models: &mut HashMap<String, ModelInfo>,
    ) -> Result<()> {
        if !models_dir.exists() {
            return Ok(());
        }

        // Collect filenames of predefined transcribe-cpp file-based models to skip
        let predefined_filenames: HashSet<String> = available_models
            .values()
            .filter(|m| matches!(m.engine_type, EngineType::TranscribeCpp) && !m.is_directory)
            .map(|m| m.filename.clone())
            .collect();

        // Scan models directory for .bin / .gguf files
        for entry in fs::read_dir(models_dir)? {
            let entry = match entry {
                Ok(e) => e,
                Err(e) => {
                    warn!("Failed to read directory entry: {}", e);
                    continue;
                }
            };

            let path = entry.path();

            // Skip directories; the .bin / .gguf extension filter is below.
            if !path.is_file() {
                continue;
            }

            let filename = match path.file_name().and_then(|s| s.to_str()) {
                Some(name) => name.to_string(),
                None => continue,
            };

            // Skip hidden files
            if filename.starts_with('.') {
                continue;
            }

            // Only process Whisper-family model files: legacy GGML `.bin` or
            // GGUF `.gguf` (both load through transcribe-cpp). Anything else —
            // including `.partial` downloads like "model.bin.partial" — is
            // skipped, since it ends in neither extension. The model ID is the
            // filename with its extension removed.
            let (model_id, is_gguf) = if let Some(stem) = filename.strip_suffix(".bin") {
                (stem.to_string(), false)
            } else if let Some(stem) = filename.strip_suffix(".gguf") {
                (stem.to_string(), true)
            } else {
                continue;
            };

            // Skip predefined model files
            if predefined_filenames.contains(&filename) {
                continue;
            }

            // Skip if model ID already exists (shouldn't happen, but be safe)
            if available_models.contains_key(&model_id) {
                continue;
            }

            // Generate display name: replace - and _ with space, capitalize words
            let fallback_display_name = model_id
                .replace(['-', '_'], " ")
                .split_whitespace()
                .map(|word| {
                    let mut chars = word.chars();
                    match chars.next() {
                        None => String::new(),
                        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                    }
                })
                .collect::<Vec<_>>()
                .join(" ");

            // Get file size in MB
            let size_mb = match path.metadata() {
                Ok(meta) => meta.len() / (1024 * 1024),
                Err(e) => {
                    warn!("Failed to get metadata for {}: {}", filename, e);
                    0
                }
            };

            // Probe GGUF headers for advertised capabilities so a dropped-in
            // model surfaces streaming / translation / languages just like a
            // Coco Voice-downloaded one. Legacy `.bin` files have no GGUF header, so
            // they stay "unknown" until transcribe-cpp reconciles them at load.
            let probe = if is_gguf {
                GgufHeaderProber.probe_file(&path)
            } else {
                CapabilityProbe::default()
            };
            let caps = local_caps(&probe);
            let display_name = probed_display_name(&probe).unwrap_or(fallback_display_name);

            info!(
                "Discovered custom transcribe-cpp model: {} ({}, {} MB, streaming={})",
                model_id, filename, size_mb, caps.supports_streaming
            );

            let mut description = "Not officially supported".to_string();
            apply_license_notice(&mut description, "", &filename);
            available_models.insert(
                model_id.clone(),
                ModelInfo {
                    id: model_id,
                    name: display_name,
                    description,
                    filename,
                    source: ModelSource::Local, // already on disk; nothing to download
                    size_mb,
                    is_downloaded: true, // Already present on disk
                    is_downloading: false,
                    partial_size: 0,
                    is_directory: false,
                    engine_type: EngineType::TranscribeCpp,
                    accuracy_score: 0.0, // Sentinel: UI hides score bars when both are 0
                    speed_score: 0.0,
                    supports_translation: caps.supports_translation,
                    is_recommended: false,
                    supported_languages: caps.supported_languages,
                    supports_language_selection: caps.supports_language_selection,
                    is_custom: true,
                    supports_streaming: caps.supports_streaming,
                    supports_language_detection: caps.supports_language_detection,
                },
            );
        }

        Ok(())
    }

    /// Discover transcribe-cpp-compatible GGUF models already present in the
    /// shared Hugging Face cache, so models downloaded by Coco Voice (or any other
    /// tool) appear in "Your Models" without re-downloading. Only architectures
    /// transcribe-cpp recognises are surfaced; arbitrary (e.g. LLM) GGUFs that
    /// share the cache are ignored.
    fn discover_hf_cache_models(available_models: &mut HashMap<String, ModelInfo>) {
        Self::discover_hf_cache_models_in(Cache::from_env().path(), available_models);
    }

    /// Scan a Hugging Face cache root (`<cache>/models--*`) for GGUF snapshots.
    /// Split from [`Self::discover_hf_cache_models`] so it can be tested against
    /// a synthetic cache directory.
    fn discover_hf_cache_models_in(
        cache_root: &Path,
        available_models: &mut HashMap<String, ModelInfo>,
    ) {
        if !cache_root.is_dir() {
            return;
        }

        // Repo+file pairs already represented, including the pre-mirror
        // handy-computer cache, so the same file is not listed twice.
        let mut known_hf: HashSet<(String, String)> = HashSet::new();
        for m in available_models.values() {
            if let ModelSource::HuggingFace {
                repo_id,
                legacy_repo_id,
                ..
            } = &m.source
            {
                known_hf.insert((repo_id.clone(), m.filename.clone()));
                if let Some(legacy) = legacy_repo_id {
                    known_hf.insert((legacy.clone(), m.filename.clone()));
                }
            }
        }

        let prober = GgufHeaderProber;

        let entries = match fs::read_dir(cache_root) {
            Ok(entries) => entries,
            Err(_) => return,
        };

        for entry in entries.flatten() {
            let folder = entry.file_name();
            let folder = folder.to_string_lossy();
            let Some(rest) = folder.strip_prefix("models--") else {
                continue;
            };
            // Reverse hf-hub's `org/name` -> `models--org--name` folder naming.
            let repo_id = rest.replace("--", "/");

            let refs_dir = entry.path().join("refs");
            let Some(revision) = Self::pick_hf_revision(&refs_dir) else {
                continue;
            };
            let Ok(commit) = fs::read_to_string(refs_dir.join(&revision)) else {
                continue;
            };
            let snapshot = entry.path().join("snapshots").join(commit.trim());
            let Ok(files) = fs::read_dir(&snapshot) else {
                continue;
            };

            for file in files.flatten() {
                let fname = file.file_name().to_string_lossy().to_string();
                if !fname.ends_with(".gguf") {
                    continue;
                }
                if known_hf.contains(&(repo_id.clone(), fname.clone())) {
                    continue;
                }
                let model_id = format!("{}/{}", repo_id, fname);
                if available_models.contains_key(&model_id) {
                    continue;
                }

                let path = snapshot.join(&fname);
                let probe = prober.probe_file(&path);
                // Only surface models transcribe-cpp recognises.
                if probe.verdict != Compatibility::Compatible {
                    continue;
                }
                let caps = local_caps(&probe);

                let size_mb = path
                    .metadata()
                    .map(|m| m.len() / (1024 * 1024))
                    .unwrap_or(0);
                let display = probed_display_name(&probe)
                    .unwrap_or_else(|| fname.trim_end_matches(".gguf").to_string());

                let mut description = format!("From Hugging Face cache: {}", repo_id);
                apply_license_notice(&mut description, &repo_id, &fname);
                info!("Discovered HF cache model: {} ({})", model_id, repo_id);
                available_models.insert(
                    model_id.clone(),
                    ModelInfo {
                        id: model_id,
                        name: display,
                        description,
                        filename: fname,
                        source: ModelSource::HuggingFace {
                            repo_id: repo_id.clone(),
                            revision: revision.clone(),
                            path: String::new(),
                            sha256: None,
                            legacy_repo_id: None,
                        },
                        size_mb,
                        is_downloaded: true,
                        is_downloading: false,
                        partial_size: 0,
                        is_directory: false,
                        engine_type: EngineType::TranscribeCpp,
                        accuracy_score: 0.0,
                        speed_score: 0.0,
                        supports_translation: caps.supports_translation,
                        is_recommended: false,
                        supported_languages: caps.supported_languages,
                        supports_language_selection: caps.supports_language_selection,
                        is_custom: false,
                        supports_streaming: caps.supports_streaming,
                        supports_language_detection: caps.supports_language_detection,
                    },
                );
            }
        }
    }

    /// Pick a cache ref to resolve a snapshot from, preferring `main`.
    fn pick_hf_revision(refs_dir: &Path) -> Option<String> {
        if refs_dir.join("main").is_file() {
            return Some("main".to_string());
        }
        fs::read_dir(refs_dir).ok()?.flatten().find_map(|e| {
            if e.path().is_file() {
                e.file_name().to_str().map(str::to_string)
            } else {
                None
            }
        })
    }

    /// Verifies the SHA256 of `path` against `expected_sha256` (if provided).
    /// On mismatch or read error the partial file is deleted and an error is returned,
    /// so the next download attempt always starts from a clean state.
    /// When `expected_sha256` is `None` (custom user models) verification is skipped.
    fn verify_sha256(path: &Path, expected_sha256: Option<&str>, model_id: &str) -> Result<()> {
        let Some(expected) = expected_sha256 else {
            return Ok(());
        };
        match Self::compute_sha256(path) {
            Ok(actual) if actual == expected => {
                info!("SHA256 verified for model {}", model_id);
                Ok(())
            }
            Ok(actual) => {
                warn!(
                    "SHA256 mismatch for model {}: expected {}, got {}",
                    model_id, expected, actual
                );
                let _ = fs::remove_file(path);
                Err(anyhow::anyhow!(
                    "Download verification failed for model {}: file is corrupt. Please retry.",
                    model_id
                ))
            }
            Err(e) => {
                let _ = fs::remove_file(path);
                Err(anyhow::anyhow!(
                    "Failed to verify download for model {}: {}. Please retry.",
                    model_id,
                    e
                ))
            }
        }
    }

    /// Computes the SHA256 hex digest of a file, reading in 64KB chunks to handle large models.
    fn compute_sha256(path: &Path) -> Result<String> {
        let mut file = File::open(path)?;
        let mut hasher = Sha256::new();
        let mut buffer = [0u8; 65536];
        loop {
            let n = file.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            hasher.update(&buffer[..n]);
        }
        Ok(format!("{:x}", hasher.finalize()))
    }

    /// Download a Hugging Face-sourced model into the shared HF cache via
    /// hf-hub, reporting progress through the same `model-download-progress`
    /// event the URL path uses. Relies on hf-hub's stock token + cache (no
    /// custom environment wiring).
    async fn download_hf_model(&self, model_info: &ModelInfo) -> Result<()> {
        let ModelSource::HuggingFace {
            repo_id,
            revision,
            path,
            sha256,
            legacy_repo_id,
        } = &model_info.source
        else {
            return Err(anyhow::anyhow!("Model is not a Hugging Face download"));
        };
        let model_id = model_info.id.clone();
        let filename = model_info.filename.clone();
        let file_path = if path.is_empty() {
            filename.clone()
        } else {
            path.clone()
        };

        // Already present, including a pre-mirror handy-computer copy. Do not
        // re-download and do not sha-check the legacy file.
        if resolve_hf_in_env(
            repo_id,
            revision,
            path,
            &filename,
            legacy_repo_id.as_deref(),
        )
        .is_some()
        {
            self.update_download_status()?;
            let _ = self.app_handle.emit("model-download-complete", &model_id);
            return Ok(());
        }

        {
            let mut models = self.available_models.lock().unwrap();
            if let Some(model) = models.get_mut(&model_id) {
                model.is_downloading = true;
            }
        }

        let cancel_token = CancellationToken::new();
        {
            let mut flags = self.cancel_flags.lock().unwrap();
            flags.insert(model_id.clone(), cancel_token.clone());
        }

        let mut cleanup = DownloadCleanup {
            available_models: &self.available_models,
            cancel_flags: &self.cancel_flags,
            model_id: model_id.clone(),
            disarmed: false,
        };

        info!(
            "Downloading HF model {} from {}@{} ({})",
            model_id, repo_id, revision, file_path
        );

        let api = ApiBuilder::from_env()
            .with_progress(false)
            .with_max_files(8)
            .build()
            .map_err(|e| anyhow::anyhow!("Failed to init Hugging Face API: {}", e))?;
        let repo = api.repo(Repo::with_revision(
            repo_id.clone(),
            RepoType::Model,
            revision.clone(),
        ));
        let progress = HfDownloadProgress::new(self.app_handle.clone(), model_id.clone());
        let pointer = match repo
            .download_with_progress_cancellable(&file_path, progress, cancel_token)
            .await
        {
            Ok(pointer) => pointer,
            Err(hf_hub::api::tokio::ApiError::Cancelled) => {
                info!("HF download cancelled for: {}", model_id);
                return Ok(());
            }
            Err(e) => {
                return Err(anyhow::anyhow!("Hugging Face download failed: {}", e));
            }
        };

        if let Some(expected) = sha256.clone() {
            let _ = self
                .app_handle
                .emit("model-verification-started", &model_id);
            let verify_path = pointer.clone();
            let verify_id = model_id.clone();
            let verify_result = tokio::task::spawn_blocking(move || {
                match ModelManager::compute_sha256(&verify_path) {
                    Ok(actual) if actual == expected => Ok(()),
                    Ok(actual) => {
                        delete_pointer_and_blob(&verify_path);
                        Err(anyhow::anyhow!(
                            "Download verification failed for model {}: file is corrupt (got {}). Please retry.",
                            verify_id,
                            actual
                        ))
                    }
                    Err(e) => {
                        delete_pointer_and_blob(&verify_path);
                        Err(anyhow::anyhow!(
                            "Failed to verify download for model {}: {}. Please retry.",
                            verify_id,
                            e
                        ))
                    }
                }
            })
            .await
            .map_err(|e| anyhow::anyhow!("SHA256 task panicked: {}", e))?;
            verify_result?;
            let _ = self
                .app_handle
                .emit("model-verification-completed", &model_id);
        }

        cleanup.disarmed = true;
        self.update_download_status()?;
        self.cancel_flags.lock().unwrap().remove(&model_id);
        let _ = self.app_handle.emit("model-download-complete", &model_id);
        info!("HF model {} downloaded", model_id);
        Ok(())
    }

    async fn staged_file_ok(path: &Path, sha: &str, size: u64) -> bool {
        if path.metadata().map(|m| m.len()).unwrap_or(0) != size {
            return false;
        }
        let path = path.to_path_buf();
        let sha = sha.to_string();
        tokio::task::spawn_blocking(move || {
            ModelManager::compute_sha256(&path)
                .ok()
                .is_some_and(|actual| actual == sha)
        })
        .await
        .unwrap_or(false)
    }

    /// Download one file with Range resume. Returns true if the user cancelled.
    async fn fetch_resumable(
        &self,
        client: &reqwest::Client,
        url: &str,
        partial_path: &Path,
        model_id: &str,
        cancel_token: &CancellationToken,
        done_before: u64,
        total: u64,
    ) -> Result<bool> {
        let mut resume_from = if partial_path.is_file() {
            partial_path.metadata()?.len()
        } else {
            0
        };
        let mut request = client.get(url);
        if resume_from > 0 {
            request = request.header("Range", format!("bytes={}-", resume_from));
        }
        let mut response = request.send().await?;
        if resume_from > 0 && response.status() == reqwest::StatusCode::OK {
            drop(response);
            let _ = fs::remove_file(partial_path);
            resume_from = 0;
            response = client.get(url).send().await?;
        }
        if resume_from > 0 && response.status() == reqwest::StatusCode::RANGE_NOT_SATISFIABLE {
            // Partial already holds the full body (process died after the last
            // byte, before rename). A Range past EOF is 416 and would stick
            // forever if treated as a hard error. Caller checks size and sha256.
            let downloaded = done_before + resume_from;
            let _ = self.app_handle.emit(
                "model-download-progress",
                &DownloadProgress {
                    model_id: model_id.to_string(),
                    downloaded,
                    total,
                    percentage: if total > 0 {
                        (downloaded as f64 / total as f64) * 100.0
                    } else {
                        100.0
                    },
                },
            );
            return Ok(false);
        }
        if !response.status().is_success()
            && response.status() != reqwest::StatusCode::PARTIAL_CONTENT
        {
            return Err(anyhow::anyhow!(
                "Failed to download model: HTTP {}",
                response.status()
            ));
        }
        let mut downloaded = done_before + resume_from;
        let mut stream = response.bytes_stream();
        let mut file = if resume_from > 0 {
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(partial_path)?
        } else {
            if let Some(parent) = partial_path.parent() {
                fs::create_dir_all(parent)?;
            }
            std::fs::File::create(partial_path)?
        };
        let mut last_emit = Instant::now();
        while let Some(chunk) = stream.next().await {
            if cancel_token.is_cancelled() {
                file.flush()?;
                return Ok(true);
            }
            let chunk = chunk?;
            file.write_all(&chunk)?;
            downloaded += chunk.len() as u64;
            if last_emit.elapsed() >= Duration::from_millis(100) {
                let _ = self.app_handle.emit(
                    "model-download-progress",
                    &DownloadProgress {
                        model_id: model_id.to_string(),
                        downloaded,
                        total,
                        percentage: if total > 0 {
                            (downloaded as f64 / total as f64) * 100.0
                        } else {
                            0.0
                        },
                    },
                );
                last_emit = Instant::now();
            }
        }
        file.flush()?;
        let _ = self.app_handle.emit(
            "model-download-progress",
            &DownloadProgress {
                model_id: model_id.to_string(),
                downloaded,
                total,
                percentage: if total > 0 {
                    (downloaded as f64 / total as f64) * 100.0
                } else {
                    100.0
                },
            },
        );
        Ok(false)
    }

    async fn download_files(&self, model_info: &ModelInfo, files: &[RemoteFile]) -> Result<()> {
        let model_id = model_info.id.clone();
        let final_dir = self.models_dir.join(&model_info.filename);
        if final_dir.is_dir() {
            self.update_download_status()?;
            return Ok(());
        }
        let staging = self
            .models_dir
            .join(format!("{}.partial", model_info.filename));
        if staging.is_file() {
            warn!("Replacing leftover archive partial for {}", model_id);
            let _ = fs::remove_file(&staging);
        }
        fs::create_dir_all(&staging)?;

        {
            let mut models = self.available_models.lock().unwrap();
            if let Some(model) = models.get_mut(&model_id) {
                model.is_downloading = true;
            }
        }
        let cancel_token = CancellationToken::new();
        {
            let mut flags = self.cancel_flags.lock().unwrap();
            flags.insert(model_id.clone(), cancel_token.clone());
        }
        let mut cleanup = DownloadCleanup {
            available_models: &self.available_models,
            cancel_flags: &self.cancel_flags,
            model_id: model_id.clone(),
            disarmed: false,
        };

        let total: u64 = files.iter().map(|f| f.size).sum();
        let mut done = 0u64;
        let client = reqwest::Client::new();
        for file in files {
            let dest = staging.join(&file.local_name);
            let partial = staging.join(format!("{}.partial", file.local_name));
            if dest.is_file() && Self::staged_file_ok(&dest, &file.sha256, file.size).await {
                done += file.size;
                continue;
            }
            let _ = fs::remove_file(&dest);
            if self
                .fetch_resumable(
                    &client,
                    &file.url,
                    &partial,
                    &model_id,
                    &cancel_token,
                    done,
                    total,
                )
                .await?
            {
                info!("Download cancelled for: {}", model_id);
                return Ok(());
            }
            let actual = partial.metadata().map(|m| m.len()).unwrap_or(0);
            if file.size > 0 && actual != file.size {
                let _ = fs::remove_file(&partial);
                return Err(anyhow::anyhow!(
                    "Download incomplete for {}: expected {} bytes, got {}",
                    file.local_name,
                    file.size,
                    actual
                ));
            }
            let _ = self
                .app_handle
                .emit("model-verification-started", &model_id);
            let verify_path = partial.clone();
            let expected = file.sha256.clone();
            let verify_id = model_id.clone();
            let verified = tokio::task::spawn_blocking(move || {
                ModelManager::verify_sha256(&verify_path, Some(&expected), &verify_id)
            })
            .await
            .map_err(|e| anyhow::anyhow!("SHA256 task panicked: {}", e))?;
            verified?;
            let _ = self
                .app_handle
                .emit("model-verification-completed", &model_id);
            fs::rename(&partial, &dest)?;
            done += file.size;
        }

        if final_dir.exists() {
            fs::remove_dir_all(&final_dir)?;
        }
        fs::rename(&staging, &final_dir)?;
        cleanup.disarmed = true;
        {
            let mut models = self.available_models.lock().unwrap();
            if let Some(model) = models.get_mut(&model_id) {
                model.is_downloading = false;
                model.is_downloaded = true;
                model.partial_size = 0;
            }
        }
        self.cancel_flags.lock().unwrap().remove(&model_id);
        let _ = self.app_handle.emit("model-download-complete", &model_id);
        Ok(())
    }

    pub async fn download_model(&self, model_id: &str) -> Result<()> {
        let model_info = {
            let models = self.available_models.lock().unwrap();
            models.get(model_id).cloned()
        };

        let model_info =
            model_info.ok_or_else(|| anyhow::anyhow!("Model not found: {}", model_id))?;

        let (url, expected_sha256) = match &model_info.source {
            ModelSource::Files { files } => {
                return self.download_files(&model_info, files).await;
            }
            ModelSource::Url { url, sha256 } => (url.clone(), sha256.clone()),
            ModelSource::HuggingFace { .. } => {
                return self.download_hf_model(&model_info).await;
            }
            ModelSource::Local => {
                return Err(anyhow::anyhow!("No download source for model"));
            }
        };
        let model_path = self.models_dir.join(&model_info.filename);
        let partial_path = self
            .models_dir
            .join(format!("{}.partial", &model_info.filename));

        // Don't download if complete version already exists
        if model_path.exists() {
            // Clean up any partial file that might exist
            if partial_path.exists() {
                let _ = fs::remove_file(&partial_path);
            }
            self.update_download_status()?;
            return Ok(());
        }

        // Check if we have a partial download to resume
        let mut resume_from = if partial_path.exists() {
            let size = partial_path.metadata()?.len();
            info!("Resuming download of model {} from byte {}", model_id, size);
            size
        } else {
            info!("Starting fresh download of model {} from {}", model_id, url);
            0
        };

        // Mark as downloading
        {
            let mut models = self.available_models.lock().unwrap();
            if let Some(model) = models.get_mut(model_id) {
                model.is_downloading = true;
            }
        }

        // Create cancellation token for this download
        let cancel_token = CancellationToken::new();
        {
            let mut flags = self.cancel_flags.lock().unwrap();
            flags.insert(model_id.to_string(), cancel_token.clone());
        }

        // Guard ensures is_downloading and cancel_flags are cleaned up on every
        // error path. Disarmed only on success (which sets is_downloaded = true).
        let mut cleanup = DownloadCleanup {
            available_models: &self.available_models,
            cancel_flags: &self.cancel_flags,
            model_id: model_id.to_string(),
            disarmed: false,
        };

        // Create HTTP client with range request for resuming
        let client = reqwest::Client::new();
        let mut request = client.get(&url);

        if resume_from > 0 {
            request = request.header("Range", format!("bytes={}-", resume_from));
        }

        let mut response = request.send().await?;

        // If we tried to resume but server returned 200 (not 206 Partial Content),
        // the server doesn't support range requests. Delete partial file and restart
        // fresh to avoid file corruption (appending full file to partial).
        if resume_from > 0 && response.status() == reqwest::StatusCode::OK {
            warn!(
                "Server doesn't support range requests for model {}, restarting download",
                model_id
            );
            drop(response);
            let _ = fs::remove_file(&partial_path);

            // Reset resume_from since we're starting fresh
            resume_from = 0;

            // Restart download without range header
            response = client.get(&url).send().await?;
        }

        // Check for success or partial content status
        if !response.status().is_success()
            && response.status() != reqwest::StatusCode::PARTIAL_CONTENT
        {
            return Err(anyhow::anyhow!(
                "Failed to download model: HTTP {}",
                response.status()
            ));
        }

        let total_size = if resume_from > 0 {
            // For resumed downloads, add the resume point to content length
            resume_from + response.content_length().unwrap_or(0)
        } else {
            response.content_length().unwrap_or(0)
        };

        let mut downloaded = resume_from;
        let mut stream = response.bytes_stream();

        // Open file for appending if resuming, or create new if starting fresh
        let mut file = if resume_from > 0 {
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&partial_path)?
        } else {
            std::fs::File::create(&partial_path)?
        };

        // Emit initial progress
        let initial_progress = DownloadProgress {
            model_id: model_id.to_string(),
            downloaded,
            total: total_size,
            percentage: if total_size > 0 {
                (downloaded as f64 / total_size as f64) * 100.0
            } else {
                0.0
            },
        };
        let _ = self
            .app_handle
            .emit("model-download-progress", &initial_progress);

        // Throttle progress events to max 10/sec (100ms intervals)
        let mut last_emit = Instant::now();
        let throttle_duration = Duration::from_millis(100);

        // Download with progress
        while let Some(chunk) = stream.next().await {
            // Check if download was cancelled
            if cancel_token.is_cancelled() {
                drop(file);
                info!("Download cancelled for: {}", model_id);
                // Keep partial file for resume functionality.
                // Guard handles is_downloading + cancel_flags cleanup on drop.
                return Ok(());
            }

            let chunk = chunk?;

            file.write_all(&chunk)?;
            downloaded += chunk.len() as u64;

            let percentage = if total_size > 0 {
                (downloaded as f64 / total_size as f64) * 100.0
            } else {
                0.0
            };

            // Emit progress event (throttled to avoid UI freeze)
            if last_emit.elapsed() >= throttle_duration {
                let progress = DownloadProgress {
                    model_id: model_id.to_string(),
                    downloaded,
                    total: total_size,
                    percentage,
                };
                let _ = self.app_handle.emit("model-download-progress", &progress);
                last_emit = Instant::now();
            }
        }

        // Emit final progress to ensure 100% is shown
        let final_progress = DownloadProgress {
            model_id: model_id.to_string(),
            downloaded,
            total: total_size,
            percentage: if total_size > 0 {
                (downloaded as f64 / total_size as f64) * 100.0
            } else {
                100.0
            },
        };
        let _ = self
            .app_handle
            .emit("model-download-progress", &final_progress);

        file.flush()?;
        drop(file); // Ensure file is closed before moving

        // Verify downloaded file size matches expected size
        if total_size > 0 {
            let actual_size = partial_path.metadata()?.len();
            if actual_size != total_size {
                // Download is incomplete/corrupted - delete partial and return error
                let _ = fs::remove_file(&partial_path);
                return Err(anyhow::anyhow!(
                    "Download incomplete: expected {} bytes, got {} bytes",
                    total_size,
                    actual_size
                ));
            }
        }

        // Verify SHA256 checksum. Runs in a blocking thread so the async executor is not
        // stalled while hashing large model files (up to 1.6 GB). On failure the partial
        // is deleted inside verify_sha256 so the next attempt always starts fresh.
        let _ = self.app_handle.emit("model-verification-started", model_id);
        info!("Verifying SHA256 for model {}...", model_id);
        let verify_path = partial_path.clone();
        let verify_expected = expected_sha256.clone();
        let verify_model_id = model_id.to_string();
        let verify_result = tokio::task::spawn_blocking(move || {
            Self::verify_sha256(&verify_path, verify_expected.as_deref(), &verify_model_id)
        })
        .await
        .map_err(|e| anyhow::anyhow!("SHA256 task panicked: {}", e))?;
        verify_result?;
        let _ = self
            .app_handle
            .emit("model-verification-completed", model_id);

        // Handle directory-based models (extract tar.gz) vs file-based models
        if model_info.is_directory {
            // Track that this model is being extracted
            {
                let mut extracting = self.extracting_models.lock().unwrap();
                extracting.insert(model_id.to_string());
            }

            // Emit extraction started event
            let _ = self.app_handle.emit("model-extraction-started", model_id);
            info!("Extracting archive for directory-based model: {}", model_id);

            // Use a temporary extraction directory to ensure atomic operations
            let temp_extract_dir = self
                .models_dir
                .join(format!("{}.extracting", &model_info.filename));
            let final_model_dir = self.models_dir.join(&model_info.filename);

            // Clean up any previous incomplete extraction
            if temp_extract_dir.exists() {
                let _ = fs::remove_dir_all(&temp_extract_dir);
            }

            // Create temporary extraction directory
            fs::create_dir_all(&temp_extract_dir)?;

            // Open the downloaded tar.gz file
            let tar_gz = File::open(&partial_path)?;
            let tar = GzDecoder::new(tar_gz);
            let mut archive = Archive::new(tar);

            // Extract to the temporary directory first
            archive.unpack(&temp_extract_dir).map_err(|e| {
                let error_msg = format!("Failed to extract archive: {}", e);
                // Clean up failed extraction
                let _ = fs::remove_dir_all(&temp_extract_dir);
                // Delete the corrupt partial file so the next download attempt starts fresh
                // instead of resuming from a broken archive (issue #858).
                let _ = fs::remove_file(&partial_path);
                // Remove from extracting set
                {
                    let mut extracting = self.extracting_models.lock().unwrap();
                    extracting.remove(model_id);
                }
                let _ = self.app_handle.emit(
                    "model-extraction-failed",
                    &serde_json::json!({
                        "model_id": model_id,
                        "error": error_msg
                    }),
                );
                anyhow::anyhow!(error_msg)
            })?;

            // Find the actual extracted directory (archive might have a nested structure)
            let extracted_dirs: Vec<_> = fs::read_dir(&temp_extract_dir)?
                .filter_map(|entry| entry.ok())
                .filter(|entry| entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false))
                .collect();

            if extracted_dirs.len() == 1 {
                // Single directory extracted, move it to the final location
                let source_dir = extracted_dirs[0].path();
                if final_model_dir.exists() {
                    fs::remove_dir_all(&final_model_dir)?;
                }
                fs::rename(&source_dir, &final_model_dir)?;
                // Clean up temp directory
                let _ = fs::remove_dir_all(&temp_extract_dir);
            } else {
                // Multiple items or no directories, rename the temp directory itself
                if final_model_dir.exists() {
                    fs::remove_dir_all(&final_model_dir)?;
                }
                fs::rename(&temp_extract_dir, &final_model_dir)?;
            }

            info!("Successfully extracted archive for model: {}", model_id);
            // Remove from extracting set
            {
                let mut extracting = self.extracting_models.lock().unwrap();
                extracting.remove(model_id);
            }
            // Emit extraction completed event
            let _ = self.app_handle.emit("model-extraction-completed", model_id);

            // Remove the downloaded tar.gz file
            let _ = fs::remove_file(&partial_path);
        } else {
            // Move partial file to final location for file-based models
            fs::rename(&partial_path, &model_path)?;
        }

        // Disarm the guard — success path does its own cleanup because it
        // additionally sets is_downloaded = true.
        cleanup.disarmed = true;
        {
            let mut models = self.available_models.lock().unwrap();
            if let Some(model) = models.get_mut(model_id) {
                model.is_downloading = false;
                model.is_downloaded = true;
                model.partial_size = 0;
            }
        }
        self.cancel_flags.lock().unwrap().remove(model_id);

        // Emit completion event
        let _ = self.app_handle.emit("model-download-complete", model_id);

        info!(
            "Successfully downloaded model {} to {:?}",
            model_id, model_path
        );

        Ok(())
    }

    pub fn delete_model(&self, model_id: &str) -> Result<()> {
        debug!("ModelManager: delete_model called for: {}", model_id);

        let model_info = {
            let models = self.available_models.lock().unwrap();
            models.get(model_id).cloned()
        };

        let model_info =
            model_info.ok_or_else(|| anyhow::anyhow!("Model not found: {}", model_id))?;

        debug!("ModelManager: Found model info: {:?}", model_info);

        if let ModelSource::HuggingFace {
            repo_id,
            revision,
            path,
            legacy_repo_id,
            ..
        } = &model_info.source
        {
            let cache = Cache::from_env().path().to_path_buf();
            let primary_name = if path.is_empty() {
                model_info.filename.as_str()
            } else {
                path.as_str()
            };
            let mut deleted = false;
            if let Some(file) = cache_file(&cache, repo_id, revision, primary_name) {
                if repo_id == MIRROR_REPO {
                    info!("Deleting mirrored file at: {:?}", file);
                    delete_pointer_and_blob(&file);
                } else if let Some(repo_dir) = hf_repo_dir(&file) {
                    info!("Deleting HF cache repo at: {:?}", repo_dir);
                    fs::remove_dir_all(repo_dir)?;
                }
                deleted = true;
            }
            if let Some(legacy) = legacy_repo_id {
                if let Some(file) = cache_file(&cache, legacy, "main", &model_info.filename) {
                    if let Some(repo_dir) = hf_repo_dir(&file) {
                        info!("Deleting legacy HF cache repo at: {:?}", repo_dir);
                        fs::remove_dir_all(repo_dir)?;
                        deleted = true;
                    }
                }
            }
            if !deleted {
                return Err(anyhow::anyhow!("No model files found to delete"));
            }
            self.update_download_status()?;
            let _ = self.app_handle.emit("model-deleted", model_id);
            return Ok(());
        }

        let model_path = self.models_dir.join(&model_info.filename);
        let partial_path = self
            .models_dir
            .join(format!("{}.partial", &model_info.filename));
        debug!("ModelManager: Model path: {:?}", model_path);
        debug!("ModelManager: Partial path: {:?}", partial_path);

        let mut deleted_something = false;

        if model_info.is_directory {
            // Delete complete model directory if it exists
            if model_path.exists() && model_path.is_dir() {
                info!("Deleting model directory at: {:?}", model_path);
                fs::remove_dir_all(&model_path)?;
                info!("Model directory deleted successfully");
                deleted_something = true;
            }
        } else {
            // Delete complete model file if it exists
            if model_path.exists() {
                info!("Deleting model file at: {:?}", model_path);
                fs::remove_file(&model_path)?;
                info!("Model file deleted successfully");
                deleted_something = true;
            }
        }

        if partial_path.is_dir() {
            info!("Deleting partial directory at: {:?}", partial_path);
            fs::remove_dir_all(&partial_path)?;
            deleted_something = true;
        } else if partial_path.is_file() {
            info!("Deleting partial file at: {:?}", partial_path);
            fs::remove_file(&partial_path)?;
            deleted_something = true;
        }

        if !deleted_something {
            return Err(anyhow::anyhow!("No model files found to delete"));
        }

        // Custom models should be removed from the list entirely since they
        // have no download URL and can't be re-downloaded
        if model_info.is_custom {
            let mut models = self.available_models.lock().unwrap();
            models.remove(model_id);
            debug!("ModelManager: removed custom model from available models");
        } else {
            // Update download status (marks predefined models as not downloaded)
            self.update_download_status()?;
            debug!("ModelManager: download status updated");
        }

        // Emit event to notify UI
        let _ = self.app_handle.emit("model-deleted", model_id);

        Ok(())
    }

    pub fn get_model_path(&self, model_id: &str) -> Result<PathBuf> {
        let model_info = self
            .get_model_info(model_id)
            .ok_or_else(|| anyhow::anyhow!("Model not found: {}", model_id))?;

        if !model_info.is_downloaded {
            return Err(anyhow::anyhow!("Model not available: {}", model_id));
        }

        // Ensure we don't return partial files/directories
        if model_info.is_downloading {
            return Err(anyhow::anyhow!(
                "Model is currently downloading: {}",
                model_id
            ));
        }

        if let ModelSource::HuggingFace {
            repo_id,
            revision,
            path,
            legacy_repo_id,
            ..
        } = &model_info.source
        {
            return resolve_hf_in_env(
                repo_id,
                revision,
                path,
                &model_info.filename,
                legacy_repo_id.as_deref(),
            )
            .ok_or_else(|| {
                anyhow::anyhow!("Complete model file not found in HF cache: {}", model_id)
            });
        }

        let model_path = self.models_dir.join(&model_info.filename);
        let partial_path = self
            .models_dir
            .join(format!("{}.partial", &model_info.filename));

        if model_info.is_directory {
            // For directory-based models, ensure the directory exists and is complete
            if model_path.exists() && model_path.is_dir() && !partial_path.exists() {
                Ok(model_path)
            } else {
                Err(anyhow::anyhow!(
                    "Complete model directory not found: {}",
                    model_id
                ))
            }
        } else {
            // For file-based models (existing logic)
            if model_path.exists() && !partial_path.exists() {
                Ok(model_path)
            } else {
                Err(anyhow::anyhow!(
                    "Complete model file not found: {}",
                    model_id
                ))
            }
        }
    }

    pub fn cancel_download(&self, model_id: &str) -> Result<()> {
        debug!("ModelManager: cancel_download called for: {}", model_id);

        // Trigger the cancellation token to stop the download. The HF path
        // aborts its in-flight chunk tasks and unwinds promptly; the URL path
        // observes it on the next chunk of its stream loop.
        {
            let flags = self.cancel_flags.lock().unwrap();
            if let Some(token) = flags.get(model_id) {
                token.cancel();
                info!("Cancellation token triggered for: {}", model_id);
            } else {
                warn!("No active download found for: {}", model_id);
            }
        }

        // Update state immediately for UI responsiveness
        {
            let mut models = self.available_models.lock().unwrap();
            if let Some(model) = models.get_mut(model_id) {
                model.is_downloading = false;
            }
        }

        // Update download status to reflect current state
        self.update_download_status()?;

        // Emit cancellation event so all UI components can clear their state
        let _ = self.app_handle.emit("model-download-cancelled", model_id);

        info!("Download cancellation initiated for: {}", model_id);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    #[test]
    fn test_effective_language_accepts_chinese_script_intent_for_zh_capability() {
        let languages = vec!["zh".to_string()];

        assert_eq!(effective_language("zh-Hans", &languages, false), "zh-Hans");
        assert_eq!(effective_language("zh-Hant", &languages, false), "zh-Hant");
    }

    #[test]
    fn test_effective_language_falls_back_to_canonical_chinese() {
        let languages = vec!["zh-Hant".to_string()];

        assert_eq!(effective_language("auto", &languages, false), "zh");
    }

    #[test]
    fn test_effective_language_resolves_bare_intent_to_concrete_locale() {
        // A model advertising full BCP-47 locales (e.g. Nemotron Streaming):
        // a bare intent must resolve to the exact code the engine expects, not
        // be handed back as the bare form the prompt table may not contain.
        let languages = vec![
            "en-US".to_string(),
            "en-GB".to_string(),
            "es-ES".to_string(),
            "zh-CN".to_string(),
            "ja-JP".to_string(),
        ];

        assert_eq!(effective_language("en", &languages, true), "en-US");
        assert_eq!(effective_language("es", &languages, true), "es-ES");
        // `zh`/`ja` have no bare entry in this model's table; resolve to locale.
        assert_eq!(effective_language("zh", &languages, true), "zh-CN");
        assert_eq!(effective_language("ja", &languages, true), "ja-JP");
        // An unsupported intent still auto-detects when the model can.
        assert_eq!(effective_language("fr", &languages, true), "auto");
    }

    #[test]
    fn test_effective_language_preserves_chinese_script_intent_for_locale_model() {
        // Script intents survive so Simplified/Traditional output conversion
        // still fires, even when the model advertises a regioned Chinese code.
        let languages = vec!["en-US".to_string(), "zh-CN".to_string()];

        assert_eq!(effective_language("zh-Hans", &languages, true), "zh-Hans");
        assert_eq!(effective_language("zh-Hant", &languages, true), "zh-Hant");
    }

    #[test]
    fn test_canonicalize_supported_languages_collapses_chinese_scripts() {
        let languages = canonicalize_supported_languages(
            vec!["en", "zh", "zh-Hans", "zh-Hant", "yue"]
                .into_iter()
                .map(String::from)
                .collect(),
        );

        assert_eq!(languages, vec!["en", "zh", "yue"]);
    }

    fn build_test_gguf_string_metadata(kvs: &[(&str, &str)]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(b"GGUF");
        out.extend_from_slice(&3u32.to_le_bytes());
        out.extend_from_slice(&0u64.to_le_bytes());
        out.extend_from_slice(&(kvs.len() as u64).to_le_bytes());
        for (key, value) in kvs {
            push_gguf_str(&mut out, key);
            out.extend_from_slice(&8u32.to_le_bytes()); // GGUF string value type.
            push_gguf_str(&mut out, value);
        }
        out
    }

    #[test]
    fn test_discover_custom_transcribe_models() {
        let temp_dir = TempDir::new().unwrap();
        let models_dir = temp_dir.path().to_path_buf();

        // Create test .bin files
        let mut custom_file = File::create(models_dir.join("my-custom-model.bin")).unwrap();
        custom_file.write_all(b"fake model data").unwrap();

        let mut another_file = File::create(models_dir.join("whisper_medical_v2.bin")).unwrap();
        another_file.write_all(b"another fake model").unwrap();

        // Custom GGUF model (also supported by transcribe-cpp)
        let mut gguf_file = File::create(models_dir.join("my-gguf-model.gguf")).unwrap();
        gguf_file
            .write_all(&build_test_gguf_string_metadata(&[(
                "general.name",
                "Friendly GGUF Name",
            )]))
            .unwrap();

        // Create files that should be ignored
        File::create(models_dir.join(".hidden-model.bin")).unwrap(); // Hidden file
        File::create(models_dir.join("readme.txt")).unwrap(); // Non-model file
        File::create(models_dir.join("ggml-small.bin")).unwrap(); // Predefined filename
        File::create(models_dir.join("download.bin.partial")).unwrap(); // Partial download
        fs::create_dir(models_dir.join("some-directory.bin")).unwrap(); // Directory

        // Set up available_models with a predefined Whisper model
        let mut models = HashMap::new();
        models.insert(
            "small".to_string(),
            ModelInfo {
                id: "small".to_string(),
                name: "Whisper Small".to_string(),
                description: "Test".to_string(),
                filename: "ggml-small.bin".to_string(),
                source: ModelSource::Url {
                    url: "https://example.com".to_string(),
                    sha256: None,
                },
                size_mb: 100,
                is_downloaded: false,
                is_downloading: false,
                partial_size: 0,
                is_directory: false,
                engine_type: EngineType::TranscribeCpp,
                accuracy_score: 0.5,
                speed_score: 0.5,
                supports_translation: true,
                is_recommended: false,
                supported_languages: vec!["en".to_string()],
                supports_language_selection: true,
                is_custom: false,
                supports_streaming: false,
                // Legacy entry: preserve the historical "Auto offered" behavior.
                // (Catalog GGUFs and on-disk probes derive this from metadata.)
                supports_language_detection: true,
            },
        );

        // Discover custom models
        ModelManager::discover_custom_transcribe_models(&models_dir, &mut models).unwrap();

        // Should have discovered 2 custom models (my-custom-model and whisper_medical_v2)
        assert!(models.contains_key("my-custom-model"));
        assert!(models.contains_key("whisper_medical_v2"));

        // Verify custom model properties
        let custom = models.get("my-custom-model").unwrap();
        assert_eq!(custom.name, "My Custom Model");
        assert_eq!(custom.filename, "my-custom-model.bin");
        assert!(matches!(custom.source, ModelSource::Local)); // Custom models have no remote source
        assert!(custom.is_downloaded);
        assert!(custom.is_custom);
        assert_eq!(custom.accuracy_score, 0.0);
        assert_eq!(custom.speed_score, 0.0);
        assert!(custom.supported_languages.is_empty());

        // Verify underscore handling
        let medical = models.get("whisper_medical_v2").unwrap();
        assert_eq!(medical.name, "Whisper Medical V2");

        // Verify .gguf models are discovered too (extension stripped for the id)
        assert!(models.contains_key("my-gguf-model"));
        let gguf = models.get("my-gguf-model").unwrap();
        assert_eq!(gguf.filename, "my-gguf-model.gguf");
        assert_eq!(gguf.name, "Friendly GGUF Name");
        assert!(gguf.is_custom);
        assert!(matches!(gguf.engine_type, EngineType::TranscribeCpp));

        // Should NOT have discovered hidden, non-model, predefined, partial, or directories
        assert!(!models.contains_key(".hidden-model"));
        assert!(!models.contains_key("readme"));
        assert!(!models.contains_key("download.bin"));
        assert!(!models.contains_key("some-directory"));
    }

    #[test]
    fn test_discover_custom_models_empty_dir() {
        let temp_dir = TempDir::new().unwrap();
        let models_dir = temp_dir.path().to_path_buf();

        let mut models = HashMap::new();
        let count_before = models.len();

        ModelManager::discover_custom_transcribe_models(&models_dir, &mut models).unwrap();

        // No new models should be added
        assert_eq!(models.len(), count_before);
    }

    #[test]
    fn test_discover_custom_models_nonexistent_dir() {
        let models_dir = PathBuf::from("/nonexistent/path/that/does/not/exist");

        let mut models = HashMap::new();
        let count_before = models.len();

        // Should not error, just return Ok
        let result = ModelManager::discover_custom_transcribe_models(&models_dir, &mut models);
        assert!(result.is_ok());
        assert_eq!(models.len(), count_before);
    }

    // ── SHA256 verification tests ─────────────────────────────────────────────

    /// Helper: write `data` to a temp file and return (TempDir, path).
    /// TempDir must be kept alive for the duration of the test.
    fn write_temp_file(data: &[u8]) -> (TempDir, std::path::PathBuf) {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("model.partial");
        let mut f = File::create(&path).unwrap();
        f.write_all(data).unwrap();
        (dir, path)
    }

    #[test]
    fn test_verify_sha256_skipped_when_none() {
        // Custom models have no expected hash — verification must be a no-op.
        let (_dir, path) = write_temp_file(b"anything");
        assert!(ModelManager::verify_sha256(&path, None, "custom").is_ok());
        assert!(
            path.exists(),
            "file must be untouched when verification is skipped"
        );
    }

    #[test]
    fn test_verify_sha256_passes_on_correct_hash() {
        // Compute the real hash so the test is self-consistent.
        let (_dir, path) = write_temp_file(b"hello world");
        let actual = ModelManager::compute_sha256(&path).unwrap();
        assert!(
            ModelManager::verify_sha256(&path, Some(&actual), "test_model").is_ok(),
            "should pass when hash matches"
        );
        assert!(
            path.exists(),
            "file must be kept on successful verification"
        );
    }

    #[test]
    fn test_verify_sha256_fails_and_deletes_partial_on_mismatch() {
        let (_dir, path) = write_temp_file(b"this is not the real model");
        let wrong_hash = "0000000000000000000000000000000000000000000000000000000000000000";

        let result = ModelManager::verify_sha256(&path, Some(wrong_hash), "bad_model");

        assert!(result.is_err(), "mismatch must return an error");
        assert!(
            result.unwrap_err().to_string().contains("corrupt"),
            "error message should mention corruption"
        );
        assert!(
            !path.exists(),
            "partial file must be deleted after hash mismatch"
        );
    }

    #[test]
    fn test_verify_sha256_fails_and_deletes_partial_when_file_missing() {
        // Simulate a partial file that was already removed (e.g. disk full mid-download).
        let dir = TempDir::new().unwrap();
        let missing_path = dir.path().join("gone.partial");
        // Don't create the file — it should not exist.

        let result =
            ModelManager::verify_sha256(&missing_path, Some("anyexpectedhash"), "missing_model");

        assert!(result.is_err(), "missing file must return an error");
    }

    fn push_gguf_str(out: &mut Vec<u8>, val: &str) {
        out.extend_from_slice(&(val.len() as u64).to_le_bytes());
        out.extend_from_slice(val.as_bytes());
    }

    fn write_synthetic_gguf(path: &Path, arch: &str, languages: &[&str]) {
        let mut out: Vec<u8> = Vec::new();
        out.extend_from_slice(&0x4655_4747u32.to_le_bytes()); // magic "GGUF"
        out.extend_from_slice(&3u32.to_le_bytes()); // version
        out.extend_from_slice(&0u64.to_le_bytes()); // tensor_count
        out.extend_from_slice(&2u64.to_le_bytes()); // kv_count
                                                    // general.architecture : string
        push_gguf_str(&mut out, "general.architecture");
        out.extend_from_slice(&8u32.to_le_bytes()); // STRING
        push_gguf_str(&mut out, arch);
        // general.languages : array<string>
        push_gguf_str(&mut out, "general.languages");
        out.extend_from_slice(&9u32.to_le_bytes()); // ARRAY
        out.extend_from_slice(&8u32.to_le_bytes()); // elem STRING
        out.extend_from_slice(&(languages.len() as u64).to_le_bytes());
        for l in languages {
            push_gguf_str(&mut out, l);
        }
        fs::write(path, out).unwrap();
    }

    #[test]
    fn test_discover_hf_cache_models_filters_by_arch() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();

        // ASR repo: a whisper gguf -> should be discovered.
        let repo = root.join("models--handy-computer--whisper-test");
        fs::create_dir_all(repo.join("snapshots").join("abc123")).unwrap();
        fs::create_dir_all(repo.join("refs")).unwrap();
        fs::write(repo.join("refs").join("main"), "abc123").unwrap();
        write_synthetic_gguf(
            &repo
                .join("snapshots")
                .join("abc123")
                .join("whisper-q8.gguf"),
            "whisper",
            &["en", "de"],
        );

        // Non-ASR (llama) gguf -> must be ignored.
        let repo2 = root.join("models--someone--llama-7b");
        fs::create_dir_all(repo2.join("snapshots").join("def456")).unwrap();
        fs::create_dir_all(repo2.join("refs")).unwrap();
        fs::write(repo2.join("refs").join("main"), "def456").unwrap();
        write_synthetic_gguf(
            &repo2.join("snapshots").join("def456").join("llama-q8.gguf"),
            "llama",
            &[],
        );

        let mut models = HashMap::new();
        ModelManager::discover_hf_cache_models_in(root, &mut models);

        let id = "handy-computer/whisper-test/whisper-q8.gguf";
        let m = models.get(id).expect("whisper gguf should be discovered");
        assert!(m.is_downloaded);
        assert!(
            matches!(&m.source, ModelSource::HuggingFace { repo_id, revision, .. }
            if repo_id == "handy-computer/whisper-test" && revision == "main")
        );
        assert_eq!(
            m.supported_languages,
            vec!["en".to_string(), "de".to_string()]
        );
        assert!(
            !models.contains_key("someone/llama-7b/llama-q8.gguf"),
            "non-ASR gguf must be ignored"
        );
    }

    #[test]
    fn test_resolve_hf_file_falls_back_to_legacy_cache() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let repo = root.join("models--handy-computer--whisper-small-gguf");
        fs::create_dir_all(repo.join("snapshots").join("abc")).unwrap();
        fs::create_dir_all(repo.join("refs")).unwrap();
        fs::write(repo.join("refs").join("main"), "abc\n").unwrap();
        let file = repo
            .join("snapshots")
            .join("abc")
            .join("whisper-small-Q8_0.gguf");
        fs::write(&file, b"already-downloaded").unwrap();

        let found = resolve_hf_file(
            root,
            "coco-research/coco-voice-models",
            "3cb3e6de7c20f58fe5beaaab3d5159384feff3f0",
            "whisper-small/whisper-small-Q8_0.gguf",
            "whisper-small-Q8_0.gguf",
            Some("handy-computer/whisper-small-gguf"),
        );
        assert_eq!(found.as_deref(), Some(file.as_path()));
    }

    #[test]
    fn test_resolve_hf_file_prefers_pinned_mirror() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let legacy = root.join("models--handy-computer--whisper-small-gguf");
        fs::create_dir_all(legacy.join("snapshots").join("old")).unwrap();
        fs::create_dir_all(legacy.join("refs")).unwrap();
        fs::write(legacy.join("refs").join("main"), "old").unwrap();
        fs::write(
            legacy
                .join("snapshots")
                .join("old")
                .join("whisper-small-Q8_0.gguf"),
            b"old",
        )
        .unwrap();

        let mirror = root.join("models--coco-research--coco-voice-models");
        let rev = "3cb3e6de7c20f58fe5beaaab3d5159384feff3f0";
        fs::create_dir_all(mirror.join("snapshots").join(rev).join("whisper-small")).unwrap();
        fs::create_dir_all(mirror.join("refs")).unwrap();
        fs::write(mirror.join("refs").join(rev), rev).unwrap();
        let file = mirror
            .join("snapshots")
            .join(rev)
            .join("whisper-small/whisper-small-Q8_0.gguf");
        fs::write(&file, b"new").unwrap();

        let found = resolve_hf_file(
            root,
            "coco-research/coco-voice-models",
            rev,
            "whisper-small/whisper-small-Q8_0.gguf",
            "whisper-small-Q8_0.gguf",
            Some("handy-computer/whisper-small-gguf"),
        );
        assert_eq!(found.as_deref(), Some(file.as_path()));
    }

    #[test]
    fn gigaam_files_keep_extracted_names() {
        let files = crate::managers::model_sources::GIGAAM_V3;
        assert_eq!(files[0].local_name, "model.int8.onnx");
        assert!(files[0].url.ends_with("/v3_e2e_ctc.int8.onnx"));
        assert!(files.iter().all(|f| {
            f.url.contains("/resolve/")
                && !f.url.contains("blob.handy.computer")
                && !f.url.contains("/main/")
        }));
    }
}
