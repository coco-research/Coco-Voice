//! The bundled, offline model catalog.
//!
//! `catalog.json` is generated at build time by `scripts/gen_catalog.py` from the
//! `handy-computer` Hugging Face org (card `transcribe_cpp` capabilities +
//! benchmarks, a GGUF header probe for name/params, and local curation for the
//! recommended set). It is compiled into the binary so Coco Voice ships a complete
//! model list with zero network access.
//!
//! Each entry is normalised into a [`ModelDescriptor`] — the same source-agnostic
//! shape every other producer (HF discovery, on-disk scans, the legacy table)
//! yields — so the catalog is "just another producer". Its explicit `capabilities`
//! map becomes a [`CapabilityProbe`] with confident `Some(..)` values; the runtime
//! `GgufHeaderProber` is the same shape with `None` where a header omits a key,
//! which is why the two are interchangeable (the catalog is a baked probe).

use std::collections::HashMap;

use once_cell::sync::Lazy;
use serde::Deserialize;

use crate::managers::model::{
    default_quant_file, EngineType, ModelDescriptor, ModelSource, QuantFile,
};
use crate::managers::model_capabilities::{CapabilityProbe, Compatibility};

#[derive(Deserialize)]
struct CatalogRoot {
    models: Vec<CatalogModel>,
}

/// One model as written in `catalog.json`. Only the fields the descriptor needs
/// are declared; serde ignores the rest (slug, family, license, …).
#[derive(Deserialize)]
struct CatalogModel {
    /// HF repo id, e.g. `handy-computer/whisper-small-gguf`.
    id: String,
    name: String,
    description: String,
    architecture: Option<String>,
    languages: Vec<String>,
    capabilities: CatalogCaps,
    speed_score: Option<f32>,
    accuracy_score: Option<f32>,
    files: Vec<QuantFile>,
    default_quant: Option<String>,
    recommended_rank: Option<u32>,
    /// Part of the small curated onboarding set (badged "Recommended"). Distinct
    /// from `recommended_rank`, which only orders the full list.
    #[serde(default)]
    recommended: bool,
    /// Pinned HF commit for publisher repos. Absent means the `main` branch.
    #[serde(default)]
    revision: Option<String>,
}

/// Architectures served by llama.cpp for transcript cleanup.
fn is_local_llm_arch(architecture: Option<&str>) -> bool {
    matches!(architecture, Some("qwen2" | "qwen3"))
}

#[derive(Deserialize)]
struct CatalogCaps {
    streaming: bool,
    translate: bool,
    lang_detect: bool,
    // `timestamps` (a string enum) is present in the catalog but has no
    // `CapabilityProbe` field yet — wire it through when the probe gains one.
}

impl From<CatalogModel> for ModelDescriptor {
    fn from(m: CatalogModel) -> Self {
        // The default download file. Its name is folded into the id so a catalog
        // entry collides (dedups) with the very same file later discovered in
        // the HF cache — both compute `"{repo_id}/{filename}"`.
        let default_filename = default_quant_file(&m.files, m.default_quant.as_deref())
            .map(|f| f.filename.clone())
            .unwrap_or_default();

        ModelDescriptor {
            id: format!("{}/{}", m.id, default_filename),
            source: ModelSource::HuggingFace {
                repo_id: m.id,
                revision: m
                    .revision
                    .map(|r| r.trim().to_string())
                    .filter(|r| !r.is_empty())
                    .unwrap_or_else(|| "main".to_string()),
            },
            name: m.name,
            description: m.description,
            // qwen2 and qwen3 are the local post-processing GGUFs, not
            // transcribe-cpp ASR archs (those are `qwen3_asr`, `canary_qwen`).
            engine_type: if is_local_llm_arch(m.architecture.as_deref()) {
                EngineType::LlamaCpp
            } else {
                EngineType::TranscribeCpp
            },
            caps: CapabilityProbe {
                verdict: Compatibility::Compatible, // curated org models we ship support for
                display_name: None,
                architecture: m.architecture,
                variant: None,
                languages: Some(m.languages),
                supports_streaming: Some(m.capabilities.streaming),
                supports_translation: Some(m.capabilities.translate),
                supports_language_detect: Some(m.capabilities.lang_detect),
            },
            files: m.files,
            default_quant: m.default_quant,
            // catalog scores are 0–100; ModelInfo / the UI bars use 0.0–1.0.
            speed_score: m.speed_score.unwrap_or(0.0) / 100.0,
            accuracy_score: m.accuracy_score.unwrap_or(0.0) / 100.0,
            recommended_rank: m.recommended_rank,
            recommended: m.recommended,
        }
    }
}

/// The bundled catalog, parsed once and normalised into descriptors.
pub static CATALOG: Lazy<Vec<ModelDescriptor>> = Lazy::new(|| {
    let root: CatalogRoot = serde_json::from_str(include_str!("catalog.json"))
        .expect("bundled catalog.json is valid JSON matching the catalog schema");
    root.models.into_iter().map(ModelDescriptor::from).collect()
});

/// Editorial recommended rank keyed by descriptor id (the same id the model
/// registry uses). Built once from the catalog.
static RANK_BY_ID: Lazy<HashMap<String, u32>> = Lazy::new(|| {
    CATALOG
        .iter()
        .filter_map(|d| d.recommended_rank.map(|r| (d.id.clone(), r)))
        .collect()
});

/// Catalog ids of the on-device post-processing (LlamaCpp) models.
pub fn local_llm_model_ids() -> Vec<String> {
    CATALOG
        .iter()
        .filter(|d| is_local_llm_arch(d.caps.architecture.as_deref()))
        .map(|d| d.id.clone())
        .collect()
}

/// Recommended rank for a model id (lower = higher priority). Returns
/// `u32::MAX` for unranked/unknown ids so they sort last in an ascending sort.
pub fn rank_of(model_id: &str) -> u32 {
    RANK_BY_ID.get(model_id).copied().unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::managers::model::{EngineType, ModelSource};
    use crate::managers::model_capabilities::KNOWN_ARCHES;
    use std::collections::BTreeSet;

    #[test]
    fn catalog_parses_and_is_nonempty() {
        assert!(!CATALOG.is_empty(), "bundled catalog should contain models");
    }

    #[test]
    fn ids_are_unique() {
        let mut ids: Vec<&str> = CATALOG.iter().map(|d| d.id.as_str()).collect();
        ids.sort_unstable();
        let before = ids.len();
        ids.dedup();
        assert_eq!(before, ids.len(), "catalog descriptor ids must be unique");
    }

    #[test]
    fn scores_are_normalised_0_to_1() {
        for d in CATALOG.iter() {
            assert!((0.0..=1.0).contains(&d.speed_score), "{} speed", d.id);
            assert!((0.0..=1.0).contains(&d.accuracy_score), "{} acc", d.id);
        }
    }

    #[test]
    fn catalog_architectures_are_known_to_capability_probe() {
        // KNOWN_ARCHES is the transcribe-cpp set. Local LLM entries (LlamaCpp)
        // are not ASR models and are not offered as transcription engines.
        let missing: BTreeSet<&str> = CATALOG
            .iter()
            .filter(|d| !matches!(d.engine_type, EngineType::LlamaCpp))
            .filter_map(|d| d.caps.architecture.as_deref())
            .filter(|arch| !KNOWN_ARCHES.contains(arch))
            .collect();

        assert!(
            missing.is_empty(),
            "catalog architecture(s) missing from KNOWN_ARCHES: {:?}",
            missing
        );
    }

    #[test]
    fn qwen_cleanup_entries_are_local_llms_not_transcription_models() {
        let llms: Vec<_> = CATALOG
            .iter()
            .filter(|d| is_local_llm_arch(d.caps.architecture.as_deref()))
            .collect();
        // Qwen3-4B (default) and Qwen2.5-1.5B (opt-in).
        assert_eq!(llms.len(), 2);
        for llm in llms {
            assert!(
                matches!(llm.engine_type, EngineType::LlamaCpp),
                "{}",
                llm.id
            );
            assert!(
                (0.5..=1.0).contains(&llm.speed_score) && (0.5..=1.0).contains(&llm.accuracy_score),
                "catalog scores are 0-100 before normalisation, got {}/{} for {}",
                llm.speed_score,
                llm.accuracy_score,
                llm.id
            );
        }
        // The ASR arch that merely contains "qwen3" must stay a transcription engine.
        let asr = CATALOG
            .iter()
            .find(|d| d.caps.architecture.as_deref() == Some("qwen3_asr"))
            .expect("qwen3_asr catalog entry");
        assert!(matches!(asr.engine_type, EngineType::TranscribeCpp));
    }

    #[test]
    fn default_local_llm_is_the_apache_qwen3_entry() {
        let id = crate::settings::LOCAL_LLM_DEFAULT_MODEL_ID;
        let d = CATALOG
            .iter()
            .find(|d| d.id == id)
            .expect("default in catalog");
        assert_eq!(d.caps.architecture.as_deref(), Some("qwen3"));
        assert!(d.recommended);
        assert!(matches!(
            &d.source,
            ModelSource::HuggingFace { repo_id, revision }
                if repo_id == "Qwen/Qwen3-4B-GGUF"
                    && revision == "bc640142c66e1fdd12af0bd68f40445458f3869b"
        ));
    }

    #[test]
    fn only_the_default_local_llm_is_recommended() {
        for d in CATALOG
            .iter()
            .filter(|d| matches!(d.engine_type, EngineType::LlamaCpp))
        {
            let default = d.id == crate::settings::LOCAL_LLM_DEFAULT_MODEL_ID;
            assert_eq!(d.recommended, default, "{}", d.id);
        }
    }
}
