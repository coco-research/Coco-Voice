//! Meeting language selection and offline model resolution.
//!
//! Meetings keep their own `meeting_language` setting (`"auto"` by default),
//! separate from dictation's `selected_language`. Model resolution inspects
//! only models already downloaded on disk and never initiates a download.

#![cfg(feature = "meetings")]

use serde::Serialize;

use crate::managers::model::{effective_language, EngineType, ModelInfo, ModelSource};

const MULTILINGUAL_PRIORITY: &[&str] = &[
    "turbo",
    "large",
    "medium",
    "small",
    "parakeet-tdt-0.6b-v3",
    "canary-1b-v2",
    "canary-180m-flash",
    "cohere-transcribe",
    "sense-voice-int8",
];

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct MeetingLanguagePlan {
    pub model_id: String,
    pub effective_language: String,
    pub multilingual_capable: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MeetingLanguageRefusal {
    NoLongAudioModel { model_id: String },
    NoInstalledModelForLanguage { language: String, model_id: String },
}

fn is_long_audio_model(model_id: &str) -> bool {
    !model_id.is_empty() && !model_id.starts_with("voxtral")
}

/// Normalize `"auto"` and BCP-47 / POSIX locale tags (`"de_DE"` -> `"de"`, `"es-ES"` -> `"es"`).
pub fn normalize_meeting_language(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("auto") {
        return "auto".to_string();
    }
    let normalized = trimmed.replace('_', "-");
    let lower = normalized.to_ascii_lowercase();
    match lower.as_str() {
        "zh-cn" | "zh-hans" | "zh-sg" => "zh-Hans".to_string(),
        "zh-tw" | "zh-hk" | "zh-hant" => "zh-Hant".to_string(),
        _ => lower.split('-').next().unwrap_or("auto").to_string(),
    }
}

fn model_supports_language(model: &ModelInfo, lang: &str) -> bool {
    let check_cjk = matches!(lang, "zh" | "zh-Hans" | "zh-Hant");
    model.supported_languages.iter().any(|supported| {
        supported.eq_ignore_ascii_case(lang)
            || (check_cjk && matches!(supported.as_str(), "zh" | "zh-Hans" | "zh-Hant"))
    })
}

fn is_multilingual_autodetect(model: &ModelInfo) -> bool {
    model.supports_language_detection && model.supported_languages.len() > 1
}

fn pick_preferred<'a>(
    candidates: &[&'a ModelInfo],
    predicate: impl Fn(&ModelInfo) -> bool,
) -> Option<&'a ModelInfo> {
    for preferred_id in MULTILINGUAL_PRIORITY {
        if let Some(found) = candidates
            .iter()
            .copied()
            .find(|m| m.id == *preferred_id && predicate(m))
        {
            return Some(found);
        }
    }
    candidates.iter().copied().find(|m| predicate(m))
}

/// Resolve the installed model and effective language for a meeting without downloading.
pub fn plan_meeting_language(
    override_language: Option<&str>,
    settings_language: &str,
    selected_model: &str,
    available_models: &[ModelInfo],
) -> Result<MeetingLanguagePlan, MeetingLanguageRefusal> {
    let requested = normalize_meeting_language(override_language.unwrap_or(settings_language));
    let installed: Vec<&ModelInfo> = available_models
        .iter()
        .filter(|m| m.is_downloaded && m.engine_type.is_speech() && is_long_audio_model(&m.id))
        .collect();
    if installed.is_empty() {
        return Err(MeetingLanguageRefusal::NoLongAudioModel {
            model_id: selected_model.to_string(),
        });
    }

    let selected = installed.iter().copied().find(|m| m.id == selected_model);
    if requested == "auto" {
        let chosen = match selected {
            Some(m) if is_multilingual_autodetect(m) => m,
            _ => pick_preferred(&installed, is_multilingual_autodetect)
                .or(selected)
                .unwrap_or(installed[0]),
        };
        let eff = effective_language(
            "auto",
            &chosen.supported_languages,
            chosen.supports_language_detection,
        );
        return Ok(MeetingLanguagePlan {
            model_id: chosen.id.clone(),
            effective_language: eff,
            multilingual_capable: chosen.supported_languages.len() > 1,
        });
    }

    let chosen = match selected {
        Some(m) if model_supports_language(m, &requested) => Some(m),
        _ => pick_preferred(&installed, |m| model_supports_language(m, &requested)),
    }
    .ok_or_else(|| MeetingLanguageRefusal::NoInstalledModelForLanguage {
        language: requested.clone(),
        model_id: selected_model.to_string(),
    })?;

    let eff = effective_language(
        &requested,
        &chosen.supported_languages,
        chosen.supports_language_detection,
    );
    Ok(MeetingLanguagePlan {
        model_id: chosen.id.clone(),
        effective_language: eff,
        multilingual_capable: chosen.supported_languages.len() > 1,
    })
}

fn tokenize_words(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| w.to_lowercase())
        .collect()
}

/// Word error rate (`0.0` = exact match) over normalized alphanumeric tokens.
pub fn word_error_rate(reference: &str, hypothesis: &str) -> f64 {
    let r = tokenize_words(reference);
    let h = tokenize_words(hypothesis);
    if r.is_empty() {
        return if h.is_empty() { 0.0 } else { 1.0 };
    }
    let mut prev: Vec<usize> = (0..=h.len()).collect();
    let mut curr = vec![0usize; h.len() + 1];
    for (i, rw) in r.iter().enumerate() {
        curr[0] = i + 1;
        for (j, hw) in h.iter().enumerate() {
            let cost = usize::from(rw != hw);
            curr[j + 1] = (prev[j + 1] + 1).min(curr[j] + 1).min(prev[j] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[h.len()] as f64 / r.len() as f64
}

#[allow(dead_code)]
pub fn sample_model_info(
    id: &str,
    downloaded: bool,
    languages: &[&str],
    autodetect: bool,
) -> ModelInfo {
    ModelInfo {
        id: id.to_string(),
        name: id.to_string(),
        description: String::new(),
        filename: String::new(),
        source: ModelSource::Local,
        size_mb: 0,
        is_downloaded: downloaded,
        is_downloading: false,
        partial_size: 0,
        is_directory: false,
        engine_type: EngineType::TranscribeCpp,
        accuracy_score: 0.0,
        speed_score: 0.0,
        supports_translation: false,
        is_recommended: false,
        supported_languages: languages.iter().map(|s| (*s).to_string()).collect(),
        supports_language_selection: languages.len() > 1,
        is_custom: false,
        supports_streaming: false,
        supports_language_detection: autodetect,
    }
}
