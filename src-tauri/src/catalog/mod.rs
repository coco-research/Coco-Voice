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
    #[serde(default)]
    license: Option<String>,
    /// Pinned HF commit for publisher repos. Absent means the `main` branch.
    #[serde(default)]
    revision: Option<String>,
}

/// Architectures served by llama.cpp for transcript cleanup (`qwen3_asr` and
/// `canary_qwen` are transcribe-cpp speech archs and do not match).
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

/// Models held from the catalog for compliance reasons.
pub const LEGAL_HOLD_IDS: &[&str] = &[
    // held for legal review (custom licences)
    "handy-computer/parakeet-unified-en-0.6b-gguf",
    "handy-computer/nemotron-3.5-asr-streaming-0.6b-gguf",
    "handy-computer/nemotron-speech-streaming-en-0.6b-gguf",
    "handy-computer/medasr-gguf",
    "handy-computer/SenseVoiceSmall-gguf",
    // non-commercial
    "handy-computer/canary-1b-gguf",
    "handy-computer/moonshine-tiny-vi-gguf",
    "handy-computer/moonshine-tiny-uk-gguf",
    "handy-computer/moonshine-tiny-ko-gguf",
    "handy-computer/moonshine-tiny-zh-gguf",
    "handy-computer/moonshine-tiny-ar-gguf",
    "handy-computer/moonshine-tiny-ja-gguf",
    "handy-computer/moonshine-base-ar-gguf",
    "handy-computer/moonshine-base-ko-gguf",
    "handy-computer/moonshine-base-uk-gguf",
    "handy-computer/moonshine-base-ja-gguf",
    "handy-computer/moonshine-base-vi-gguf",
    "handy-computer/moonshine-base-zh-gguf",
];

/// catalog says "other", upstream FunAudioLLM releases these under Apache-2.0
/// (verified 2026-09-27 in the model-source audit).
pub const CLEARED_OTHER_IDS: &[&str] = &[
    "handy-computer/Fun-ASR-MLT-Nano-2512-gguf",
    "handy-computer/Fun-ASR-Nano-2512-gguf",
];

/// Checks if a given repo id is on the legal hold list (case-insensitive).
pub(crate) fn repo_on_legal_hold(repo_id: &str) -> bool {
    LEGAL_HOLD_IDS
        .iter()
        .any(|held_id| held_id.eq_ignore_ascii_case(repo_id))
}

/// A catalog entry is offered only if its id is NOT on legal hold, AND its license
/// is allowlisted (or its id is explicitly cleared).
fn is_allowed_catalog_entry(id: &str, license: Option<&str>) -> bool {
    if repo_on_legal_hold(id) {
        return false;
    }

    let l_lower = license.map(|l| l.trim().to_lowercase());

    if let Some(ref l) = l_lower {
        if l == "mit" || l == "apache-2.0" || l == "cc-by-4.0" {
            return true;
        }
    }

    if CLEARED_OTHER_IDS
        .iter()
        .any(|cleared| cleared.eq_ignore_ascii_case(id))
    {
        if let Some(ref l) = l_lower {
            if l == "other" {
                return true;
            }
        }
    }

    false
}

static WITHHELD_REPOS: Lazy<std::collections::HashSet<String>> = Lazy::new(|| {
    let root: CatalogRoot = serde_json::from_str(include_str!("catalog.json"))
        .expect("bundled catalog.json is valid JSON matching the catalog schema");
    root.models
        .into_iter()
        .filter(|m| !is_allowed_catalog_entry(&m.id, m.license.as_deref()))
        .map(|m| m.id.to_lowercase())
        .collect()
});

pub(crate) fn repo_withheld(repo_id: &str) -> bool {
    if repo_on_legal_hold(repo_id) {
        return true;
    }
    WITHHELD_REPOS.contains(&repo_id.to_lowercase())
}

/// The bundled catalog, parsed once and normalised into descriptors.
pub static CATALOG: Lazy<Vec<ModelDescriptor>> = Lazy::new(|| {
    let root: CatalogRoot = serde_json::from_str(include_str!("catalog.json"))
        .expect("bundled catalog.json is valid JSON matching the catalog schema");
    root.models
        .into_iter()
        .filter(|m| is_allowed_catalog_entry(&m.id, m.license.as_deref()))
        .map(ModelDescriptor::from)
        .collect()
});

/// Editorial recommended rank keyed by descriptor id (the same id the model
/// registry uses). Built once from the catalog.
static RANK_BY_ID: Lazy<HashMap<String, u32>> = Lazy::new(|| {
    CATALOG
        .iter()
        .filter_map(|d| d.recommended_rank.map(|r| (d.id.clone(), r)))
        .collect()
});

/// Recommended rank for a model id (lower = higher priority). Returns
/// `u32::MAX` for unranked/unknown ids so they sort last in an ascending sort.
pub fn rank_of(model_id: &str) -> u32 {
    RANK_BY_ID.get(model_id).copied().unwrap_or(u32::MAX)
}

/// Catalog ids of the on-device post-processing (llama.cpp) models.
pub fn local_llm_model_ids() -> Vec<String> {
    CATALOG
        .iter()
        .filter(|d| !d.engine_type.is_speech())
        .map(|d| d.id.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
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
        // KNOWN_ARCHES is the transcribe-cpp set; local cleanup LLMs are not in it.
        let missing: BTreeSet<&str> = CATALOG
            .iter()
            .filter(|d| d.engine_type.is_speech())
            .filter_map(|d| d.caps.architecture.as_deref())
            .filter(|arch| !KNOWN_ARCHES.contains(arch))
            .collect();

        assert!(
            missing.is_empty(),
            "catalog architecture(s) missing from KNOWN_ARCHES: {:?}",
            missing
        );
    }

    const QWEN3_REPO: &str = "Qwen/Qwen3-4B-GGUF";
    const QWEN25_REPO: &str = "Qwen/Qwen2.5-1.5B-Instruct-GGUF";

    #[test]
    fn qwen_cleanup_entries_pass_the_allowlist_and_are_not_speech_models() {
        let root: CatalogRoot = serde_json::from_str(include_str!("catalog.json")).unwrap();
        for repo in [QWEN3_REPO, QWEN25_REPO] {
            // Allowed by the licence allowlist, not on legal hold.
            let entry = root.models.iter().find(|m| m.id == repo).expect(repo);
            assert_eq!(entry.license.as_deref(), Some("apache-2.0"), "{repo}");
            assert!(
                is_allowed_catalog_entry(&entry.id, entry.license.as_deref()),
                "{repo}"
            );
            assert!(!repo_withheld(repo), "{repo}");

            // Offered as a local LLM, downloaded only from Qwen's own repo at a pinned commit.
            let d = CATALOG
                .iter()
                .find(|d| d.id.starts_with(&format!("{repo}/")))
                .expect(repo);
            assert!(!d.engine_type.is_speech(), "{repo}");
            assert!(
                matches!(&d.source, ModelSource::HuggingFace { repo_id, revision }
                    if repo_id == repo
                        && revision.len() == 40
                        && revision.bytes().all(|b| b.is_ascii_hexdigit())),
                "{repo}: {:?}",
                d.source
            );
        }

        // Only those two: the ASR archs that merely contain "qwen" stay speech models.
        let llms = CATALOG
            .iter()
            .filter(|d| !d.engine_type.is_speech())
            .count();
        assert_eq!(llms, 2);
        let asr = CATALOG
            .iter()
            .find(|d| d.caps.architecture.as_deref() == Some("qwen3_asr"))
            .expect("qwen3_asr catalog entry");
        assert!(asr.engine_type.is_speech());
    }

    #[test]
    fn local_llm_model_ids_are_exactly_the_non_speech_entries() {
        let ids = local_llm_model_ids();
        assert!(ids.contains(&crate::settings::LOCAL_LLM_DEFAULT_MODEL_ID.to_string()));
        for d in CATALOG.iter() {
            assert_eq!(ids.contains(&d.id), !d.engine_type.is_speech(), "{}", d.id);
        }
    }

    #[test]
    fn default_local_llm_is_the_recommended_qwen3_entry() {
        let id = crate::settings::LOCAL_LLM_DEFAULT_MODEL_ID;
        let d = CATALOG
            .iter()
            .find(|d| d.id == id)
            .expect("default in catalog");
        assert_eq!(d.caps.architecture.as_deref(), Some("qwen3"));
        assert!(matches!(
            &d.source,
            ModelSource::HuggingFace { repo_id, revision }
                if repo_id == QWEN3_REPO
                    && revision == "bc640142c66e1fdd12af0bd68f40445458f3869b"
        ));
        // Among the local LLMs only the default is recommended.
        for llm in CATALOG.iter().filter(|d| !d.engine_type.is_speech()) {
            assert_eq!(llm.recommended, llm.id == id, "{}", llm.id);
        }
    }

    #[test]
    fn compliance_gated_models_are_dropped() {
        let root: CatalogRoot = serde_json::from_str(include_str!("catalog.json")).unwrap();
        // Every held id must exist in catalog.json, so a typo fails here.
        for id in LEGAL_HOLD_IDS {
            assert!(
                root.models.iter().any(|m| m.id == *id),
                "LEGAL_HOLD_IDS entry {id} is not in catalog.json"
            );
            assert!(
                !CATALOG.iter().any(|d| d.id.starts_with(&format!("{id}/"))),
                "catalog must not offer gated id {id}"
            );
        }

        // A new non-allowlisted model in a regenerated catalog must fail here and be reviewed.
        assert_eq!(
            root.models.len() - CATALOG.len(),
            18,
            "a new non-allowlisted model in a regenerated catalog must fail here and be reviewed"
        );

        assert!(!is_allowed_catalog_entry("any/model", Some("other")));
        assert!(!is_allowed_catalog_entry("any/model", Some("cc-by-nc-4.0")));
        assert!(!is_allowed_catalog_entry(
            "any/model",
            Some("qwen-research")
        ));
        assert!(!is_allowed_catalog_entry("any/model", None));
        assert!(is_allowed_catalog_entry("any/model", Some(" MIT ")));
        assert!(is_allowed_catalog_entry("any/model", Some("Apache-2.0")));

        // Test CLEARED_OTHER_IDS
        let cleared_id = CLEARED_OTHER_IDS[0];
        assert!(is_allowed_catalog_entry(cleared_id, Some("other")));
        assert!(!is_allowed_catalog_entry(cleared_id, Some("cc-by-nc-4.0")));
        assert!(!is_allowed_catalog_entry(cleared_id, None));
    }

    #[test]
    fn repo_on_legal_hold_is_case_insensitive() {
        assert!(repo_on_legal_hold("handy-computer/SenseVoiceSmall-gguf"));
        assert!(repo_on_legal_hold("HANDY-COMPUTER/SENSEVOICESMALL-GGUF"));
        assert!(!repo_on_legal_hold("handy-computer/whisper-small-gguf"));
    }

    #[test]
    fn repo_withheld_behavior() {
        // True for a held ID in any case
        assert!(repo_withheld("handy-computer/SenseVoiceSmall-gguf"));
        assert!(repo_withheld("HANDY-COMPUTER/SENSEVOICESMALL-GGUF"));

        // Withheld exactly when the allowlist rejects the catalog entry.
        assert!(!LEGAL_HOLD_IDS.is_empty() && !CLEARED_OTHER_IDS.is_empty());
        let root: CatalogRoot = serde_json::from_str(include_str!("catalog.json")).unwrap();
        for m in &root.models {
            assert_eq!(
                repo_withheld(&m.id),
                !is_allowed_catalog_entry(&m.id, m.license.as_deref()),
                "{}",
                m.id
            );
        }

        // False for an allowlisted catalog repo
        assert!(!repo_withheld("handy-computer/whisper-small-gguf"));

        // False for an unknown repo
        assert!(!repo_withheld("some/unknown-repo"));
    }
}
