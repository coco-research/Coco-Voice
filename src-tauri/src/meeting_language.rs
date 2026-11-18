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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_meeting_language_is_auto_and_independent_of_dictation() {
        let mut settings = crate::settings::get_default_settings();
        assert_eq!(settings.meeting_language, "auto");
        settings.selected_language = "fr".to_string();
        assert_eq!(settings.meeting_language, "auto");

        let app = tauri::test::mock_builder()
            .plugin(tauri_plugin_store::Builder::default().build())
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();
        let tm = crate::managers::transcription::TranscriptionManager::new_for_test(
            app.handle().clone(),
        );
        assert_eq!(tm.resolved_meeting_language(), "auto");
        tm.set_meeting_language_override(Some("de_DE"));
        assert_eq!(tm.resolved_meeting_language(), "de");
        tm.clear_meeting_pin();
        assert_eq!(tm.resolved_meeting_language(), "auto");
    }

    #[test]
    fn auto_prefers_downloaded_multilingual_turbo_and_resolves_21_plus_languages() {
        let twenty_two_langs = [
            "en", "de", "es", "fr", "it", "pt", "nl", "pl", "ru", "uk", "cs", "bg", "sv", "tr",
            "ar", "he", "hi", "ja", "ko", "vi", "zh-Hans", "zh-Hant",
        ];
        assert!(twenty_two_langs.len() >= 21);
        let models = vec![
            sample_model_info("moonshine-base", true, &["en"], false),
            sample_model_info("turbo", true, &twenty_two_langs, true),
            sample_model_info("large", false, &twenty_two_langs, true),
        ];
        let plan = plan_meeting_language(None, "auto", "moonshine-base", &models).unwrap();
        assert_eq!(plan.model_id, "turbo");
        assert_eq!(plan.effective_language, "auto");
        assert!(plan.multilingual_capable);

        for lang in twenty_two_langs {
            let resolved =
                plan_meeting_language(Some(lang), "auto", "moonshine-base", &models).unwrap();
            assert_eq!(resolved.model_id, "turbo");
            assert_eq!(resolved.effective_language, lang);
        }
    }

    #[test]
    fn explicit_german_and_spanish_resolve_installed_model_and_refuse_missing_without_download() {
        let multilingual = vec![
            sample_model_info("moonshine-base", true, &["en"], false),
            sample_model_info("parakeet-tdt-0.6b-v3", true, &["en", "de", "es"], true),
        ];
        let de =
            plan_meeting_language(Some("de_DE"), "auto", "moonshine-base", &multilingual).unwrap();
        assert_eq!(de.model_id, "parakeet-tdt-0.6b-v3");
        assert_eq!(de.effective_language, "de");

        let es =
            plan_meeting_language(Some("es-ES"), "auto", "moonshine-base", &multilingual).unwrap();
        assert_eq!(es.model_id, "parakeet-tdt-0.6b-v3");
        assert_eq!(es.effective_language, "es");

        let english_only = vec![
            sample_model_info("moonshine-base", true, &["en"], false),
            sample_model_info("turbo", false, &["en", "de", "es"], true),
        ];
        let err =
            plan_meeting_language(Some("de"), "auto", "moonshine-base", &english_only).unwrap_err();
        assert_eq!(
            err,
            MeetingLanguageRefusal::NoInstalledModelForLanguage {
                language: "de".to_string(),
                model_id: "moonshine-base".to_string(),
            }
        );
    }

    #[test]
    fn word_error_rate_scores_german_and_spanish_hypotheses() {
        assert_eq!(
            word_error_rate("Guten Morgen zusammen!", "guten morgen zusammen"),
            0.0
        );
        let wer = word_error_rate(
            "Buenos dias equipo hoy lanzamos",
            "Buenos dias equipo lanzamos",
        );
        assert!((wer - 0.2).abs() < 1e-6, "wer={wer}");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_say_generates_offline_german_anna_and_spanish_eddy_16khz_wavs() {
        let dir = tempfile::tempdir().unwrap();
        for (voice, phrase, name) in [
            ("Anna", "Guten Morgen zusammen wir beginnen", "de.wav"),
            (
                "Eddy (Spanish (Spain))",
                "Buenos dias equipo empezamos ahora",
                "es.wav",
            ),
        ] {
            let out = dir.path().join(name);
            let status = std::process::Command::new("say")
                .args([
                    "-v",
                    voice,
                    "-o",
                    out.to_str().unwrap(),
                    "--data-format=LEI16@16000",
                    phrase,
                ])
                .status()
                .unwrap();
            assert!(status.success(), "say failed for {voice}");
            let samples = crate::audio_toolkit::read_wav_samples(&out).unwrap();
            assert!(
                samples.len() > 8_000,
                "expected >0.5s 16 kHz PCM for {voice}, got {}",
                samples.len()
            );
        }
    }
}
