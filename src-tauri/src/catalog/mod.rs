//! The bundled, offline model catalog.
//!
//! `catalog.json` is maintained by hand. `scripts/gen_catalog.py` lists the
//! `coco-research` Hugging Face org and must not be rerun over it: a rerun would
//! drop the handy-computer rows and Qwen3-4B. The file is compiled into the binary
//! so Coco Voice ships a complete model list with zero network access.
//!
//! Each entry is normalised into a [`ModelDescriptor`] — the same source-agnostic
//! shape every other producer (HF discovery, on-disk scans, the legacy table)
//! yields — so the catalog is "just another producer". Its explicit `capabilities`
//! map becomes a [`CapabilityProbe`] with confident `Some(..)` values; the runtime
//! `GgufHeaderProber` is the same shape with `None` where a header omits a key,
//! which is why the two are interchangeable (the catalog is a baked probe).

use std::collections::HashMap;

use log::error;
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
#[derive(Clone, Deserialize)]
struct CatalogModel {
    /// Catalog id, e.g. `handy-computer/whisper-small-gguf`.
    id: String,
    /// Hugging Face repo to download. Empty or absent uses `id`.
    #[serde(default)]
    source_repo: Option<String>,
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
    /// SHA-256 of the default quant file. Absent skips integrity verification.
    #[serde(default)]
    sha256: Option<String>,
}

/// Architectures served by llama.cpp for transcript cleanup (`qwen3_asr` and
/// `canary_qwen` are transcribe-cpp speech archs and do not match).
fn is_local_llm_arch(architecture: Option<&str>) -> bool {
    matches!(architecture, Some("qwen2" | "qwen3"))
}

#[derive(Clone, Deserialize)]
struct CatalogCaps {
    streaming: bool,
    translate: bool,
    lang_detect: bool,
    // `timestamps` (a string enum) is present in the catalog but has no
    // `CapabilityProbe` field yet — wire it through when the probe gains one.
}

/// Normalizes an optional raw SHA-256 string: trims leading and trailing whitespace,
/// converts to lowercase, and accepts it only if it is exactly 64 ASCII hex characters.
fn normalize_sha256(raw: Option<&str>) -> Option<String> {
    let s = raw?.trim();
    if s.len() == 64 && s.chars().all(|c| c.is_ascii_hexdigit()) {
        Some(s.to_ascii_lowercase())
    } else {
        None
    }
}

/// Validates an optional SHA-256 pin:
/// - `None` -> `Ok(None)` (not pinned yet)
/// - valid 64 ASCII hex chars -> `Ok(Some(lowercase))`
/// - invalid or blank-but-present -> `Err(())`
fn pinned_sha256(raw: Option<&str>) -> Result<Option<String>, ()> {
    match raw {
        None => Ok(None),
        Some(s) => normalize_sha256(Some(s)).map(Some).ok_or(()),
    }
}

impl TryFrom<CatalogModel> for ModelDescriptor {
    type Error = ();

    fn try_from(m: CatalogModel) -> Result<Self, Self::Error> {
        // The default download file. Its name is folded into the descriptor id so
        // a catalog entry collides (dedups) with the same file later discovered in
        // the HF cache — both compute `"{catalog id}/{filename}"`. `source_repo`
        // can point the download at another repo; the id stays the catalog id.
        let default_filename = default_quant_file(&m.files, m.default_quant.as_deref())
            .map(|f| f.filename.clone())
            .unwrap_or_default();
        // Owned copy so the fallback can read `m.id` after this borrow ends.
        let source_repo = m
            .source_repo
            .as_deref()
            .map(str::trim)
            .filter(|repo| !repo.is_empty())
            .map(str::to_string);
        let repo_id = source_repo.unwrap_or_else(|| m.id.clone());

        let sha256 = pinned_sha256(m.sha256.as_deref()).map_err(|()| {
            error!(
                "Invalid sha256 for catalog model '{}': expected 64 ASCII hex characters",
                m.id
            );
        })?;

        Ok(ModelDescriptor {
            id: format!("{}/{}", m.id, default_filename),
            source: ModelSource::HuggingFace {
                repo_id,
                revision: m
                    .revision
                    .map(|r| r.trim().to_string())
                    .filter(|r| !r.is_empty())
                    .unwrap_or_else(|| "main".to_string()),
                sha256,
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
        })
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
    // The downloadable re-hosts carry the FunASR Model License v1.1, like SenseVoiceSmall.
    "handy-computer/Fun-ASR-MLT-Nano-2512-gguf",
    "handy-computer/Fun-ASR-Nano-2512-gguf",
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

/// Repos whose catalog licence is "other" but whose licence has been reviewed and
/// cleared. None today.
pub const CLEARED_OTHER_IDS: &[&str] = &[];

/// True when `id` matches an entry in `hold_ids` (ASCII case-insensitive).
/// The match is the model id itself, so the same list covers every host.
pub(crate) fn id_on_hold(id: &str, hold_ids: &[&str]) -> bool {
    hold_ids
        .iter()
        .any(|held_id| held_id.eq_ignore_ascii_case(id))
}

/// Production hold check: [`id_on_hold`] against [`LEGAL_HOLD_IDS`].
pub(crate) fn model_on_legal_hold(id: &str) -> bool {
    id_on_hold(id, LEGAL_HOLD_IDS)
}

/// A catalog entry is offered only if its id is NOT on legal hold, AND its license
/// is allowlisted (or its id is explicitly cleared).
fn is_allowed_catalog_entry(id: &str, license: Option<&str>) -> bool {
    if model_on_legal_hold(id) {
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
    if model_on_legal_hold(repo_id) {
        return true;
    }
    WITHHELD_REPOS.contains(&repo_id.to_lowercase())
}

/// Repo ids whose catalog row was dropped because the SHA-256 pin was present
/// but not 64 hex characters. Allowlisted repos with no pin are not included.
fn pin_rejected_repo_ids(
    models: impl IntoIterator<Item = CatalogModel>,
) -> std::collections::HashSet<String> {
    models
        .into_iter()
        .filter(|m| {
            is_allowed_catalog_entry(&m.id, m.license.as_deref())
                && pinned_sha256(m.sha256.as_deref()).is_err()
        })
        .map(|m| m.id.to_lowercase())
        .collect()
}

static PIN_REJECTED_REPOS: Lazy<std::collections::HashSet<String>> = Lazy::new(|| {
    let root: CatalogRoot = serde_json::from_str(include_str!("catalog.json"))
        .expect("bundled catalog.json is valid JSON matching the catalog schema");
    pin_rejected_repo_ids(root.models)
});

/// True when this repo's catalog entry was dropped for a malformed SHA-256 pin.
pub(crate) fn repo_pin_rejected(repo_id: &str) -> bool {
    PIN_REJECTED_REPOS.contains(&repo_id.to_lowercase())
}

/// Builds a catalog of descriptors from an iterator of [`CatalogModel`] entries,
/// filtering out entries on legal hold, disallowed licenses, or with malformed SHA-256 pins.
fn build_catalog(models: impl IntoIterator<Item = CatalogModel>) -> Vec<ModelDescriptor> {
    models
        .into_iter()
        .filter(|m| is_allowed_catalog_entry(&m.id, m.license.as_deref()))
        .filter_map(|m| ModelDescriptor::try_from(m).ok())
        .collect()
}

/// Convenience builder taking a slice of [`CatalogModel`] entries.
#[cfg(test)]
fn build_catalog_from_slice(models: &[CatalogModel]) -> Vec<ModelDescriptor> {
    build_catalog(models.iter().cloned())
}

/// The bundled catalog, parsed once and normalised into descriptors.
pub static CATALOG: Lazy<Vec<ModelDescriptor>> = Lazy::new(|| {
    let root: CatalogRoot = serde_json::from_str(include_str!("catalog.json"))
        .expect("bundled catalog.json is valid JSON matching the catalog schema");
    build_catalog(root.models)
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

    #[test]
    fn qwen_cleanup_entries_pass_the_allowlist_and_are_not_speech_models() {
        let root: CatalogRoot = serde_json::from_str(include_str!("catalog.json")).unwrap();
        for repo in [QWEN3_REPO] {
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
                matches!(&d.source, ModelSource::HuggingFace { repo_id, revision, .. }
                    if repo_id == repo
                        && revision.len() == 40
                        && revision.bytes().all(|b| b.is_ascii_hexdigit())),
                "{repo}: {:?}",
                d.source
            );
        }

        // Only Qwen3-4B (the 1.5B is not offered in 0.10.0: unmeasured on the on-device
        // prompt). The ASR archs that merely contain "qwen" stay speech models.
        let llms = CATALOG
            .iter()
            .filter(|d| !d.engine_type.is_speech())
            .count();
        assert_eq!(llms, 1);
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
            ModelSource::HuggingFace { repo_id, revision, .. }
                if repo_id == QWEN3_REPO
                    && revision == "bc640142c66e1fdd12af0bd68f40445458f3869b"
        ));
        // Among the local LLMs only the default is recommended.
        for llm in CATALOG.iter().filter(|d| !d.engine_type.is_speech()) {
            assert_eq!(llm.recommended, llm.id == id, "{}", llm.id);
        }
    }

    #[test]
    fn catalog_sha256_reaches_hugging_face_source() {
        // Round-trip a real entry so the test follows the catalog schema.
        let root: serde_json::Value = serde_json::from_str(include_str!("catalog.json")).unwrap();
        let source_sha256 = |pinned: Option<&str>| {
            let mut entry = root["models"][0].clone();
            // Every bundled row carries a hash now, so drop it to test a row without one.
            match pinned {
                Some(hash) => entry["sha256"] = serde_json::json!(hash),
                None => {
                    entry.as_object_mut().unwrap().remove("sha256");
                }
            }
            let model: CatalogModel = serde_json::from_value(entry).unwrap();
            ModelDescriptor::try_from(model).map(|d| match d.source {
                ModelSource::HuggingFace { sha256, .. } => sha256,
                other => panic!("expected a Hugging Face source, got {other:?}"),
            })
        };

        let hash = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        assert_eq!(source_sha256(Some(hash)), Ok(Some(hash.to_string())));
        let upper = "0123456789ABCDEF0123456789ABCDEF0123456789ABCDEF0123456789ABCDEF";
        assert_eq!(source_sha256(Some(upper)), Ok(Some(hash.to_string())));
        // An entry that does not declare a hash does not grow one.
        assert_eq!(source_sha256(None), Ok(None));
        // Malformed hashes fail closed.
        assert_eq!(source_sha256(Some("  ")), Err(()));
        assert_eq!(source_sha256(Some("not-a-valid-sha256")), Err(()));
    }

    #[test]
    fn normalize_sha256_cases() {
        let valid_lower = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let valid_upper = "0123456789ABCDEF0123456789ABCDEF0123456789ABCDEF0123456789ABCDEF";

        // valid lowercase
        assert_eq!(
            normalize_sha256(Some(valid_lower)),
            Some(valid_lower.to_string())
        );

        // valid uppercase -> lowercased
        assert_eq!(
            normalize_sha256(Some(valid_upper)),
            Some(valid_lower.to_string())
        );

        // padded with spaces -> trimmed
        let padded = format!("   {valid_lower}   ");
        assert_eq!(
            normalize_sha256(Some(&padded)),
            Some(valid_lower.to_string())
        );
        let padded_upper = format!("  {valid_upper}  ");
        assert_eq!(
            normalize_sha256(Some(&padded_upper)),
            Some(valid_lower.to_string())
        );

        // 63 chars -> None
        assert_eq!(normalize_sha256(Some(&valid_lower[..63])), None);

        // non-hex -> None
        let non_hex = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdeg";
        assert_eq!(normalize_sha256(Some(non_hex)), None);

        // blank -> None
        assert_eq!(normalize_sha256(Some("")), None);
        assert_eq!(normalize_sha256(Some("   ")), None);
        assert_eq!(normalize_sha256(Some("\t\n ")), None);

        // None -> None
        assert_eq!(normalize_sha256(None), None);
    }

    #[test]
    fn pinned_sha256_cases() {
        let valid_lower = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let valid_upper = "0123456789ABCDEF0123456789ABCDEF0123456789ABCDEF0123456789ABCDEF";

        // missing -> Ok(None)
        assert_eq!(pinned_sha256(None), Ok(None));

        // valid upper -> Ok(Some(lower))
        assert_eq!(
            pinned_sha256(Some(valid_upper)),
            Ok(Some(valid_lower.to_string()))
        );

        // blank present -> Err
        assert_eq!(pinned_sha256(Some("")), Err(()));
        assert_eq!(pinned_sha256(Some("   ")), Err(()));
        assert_eq!(pinned_sha256(Some("\t\n ")), Err(()));

        // 63 chars -> Err
        assert_eq!(pinned_sha256(Some(&valid_lower[..63])), Err(()));

        // non-hex -> Err
        let non_hex = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdeg";
        assert_eq!(pinned_sha256(Some(non_hex)), Err(()));
    }

    #[test]
    fn catalog_builder_drops_malformed_sha256() {
        let root: serde_json::Value = serde_json::from_str(include_str!("catalog.json")).unwrap();
        let mut valid_entry = root["models"][0].clone();
        valid_entry["id"] = serde_json::json!("test/valid-model");
        valid_entry["license"] = serde_json::json!("mit");
        valid_entry["sha256"] =
            serde_json::json!("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef");

        let mut malformed_entry = root["models"][0].clone();
        malformed_entry["id"] = serde_json::json!("test/malformed-model");
        malformed_entry["license"] = serde_json::json!("mit");
        malformed_entry["sha256"] = serde_json::json!("not-a-valid-sha");

        let valid: CatalogModel = serde_json::from_value(valid_entry).unwrap();
        let malformed: CatalogModel = serde_json::from_value(malformed_entry).unwrap();

        let entries = [valid.clone(), malformed.clone()];
        let built = build_catalog_from_slice(&entries);

        assert_eq!(built.len(), 1);
        assert!(built[0].id.starts_with("test/valid-model/"));

        let mut unpinned_entry = root["models"][0].clone();
        unpinned_entry["id"] = serde_json::json!("test/unpinned-model");
        unpinned_entry["license"] = serde_json::json!("mit");
        unpinned_entry["sha256"] = serde_json::Value::Null;
        let unpinned: CatalogModel = serde_json::from_value(unpinned_entry).unwrap();

        let mut held_entry = root["models"][0].clone();
        held_entry["id"] = serde_json::json!("handy-computer/SenseVoiceSmall-gguf");
        held_entry["license"] = serde_json::json!("mit");
        held_entry["sha256"] = serde_json::json!("not-a-valid-sha");
        let held: CatalogModel = serde_json::from_value(held_entry).unwrap();

        let rejected = pin_rejected_repo_ids([valid, malformed, unpinned, held]);
        assert!(rejected.contains("test/malformed-model"));
        assert!(!rejected.contains("test/valid-model"));
        assert!(!rejected.contains("test/unpinned-model"));
        // Withheld repos are skipped by repo_withheld, not by the bad-pin set.
        assert!(!rejected.contains("handy-computer/sensevoicesmall-gguf"));
    }

    #[test]
    fn bundled_catalog_sha256_pins_are_valid() {
        let root: CatalogRoot = serde_json::from_str(include_str!("catalog.json"))
            .expect("bundled catalog.json is valid JSON matching the catalog schema");
        for model in &root.models {
            if let Some(ref raw) = model.sha256 {
                let normalized = normalize_sha256(Some(raw));
                assert!(
                    normalized.is_some(),
                    "bundled catalog model {} has malformed sha256 pin: {:?}",
                    model.id,
                    raw
                );
                let hash = normalized.unwrap();
                assert_eq!(hash.len(), 64);
                assert!(hash
                    .chars()
                    .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
            }
        }

        for descriptor in CATALOG.iter() {
            if let ModelSource::HuggingFace {
                sha256: Some(ref hash),
                ..
            } = descriptor.source
            {
                let normalized = normalize_sha256(Some(hash));
                assert!(
                    normalized.is_some(),
                    "CATALOG descriptor {} has malformed sha256: {:?}",
                    descriptor.id,
                    hash
                );
                let h = normalized.unwrap();
                assert_eq!(h.len(), 64);
                assert_eq!(&h, hash);
            }
        }
    }

    #[test]
    fn every_bundled_row_sha256_is_on_its_descriptor() {
        let root: CatalogRoot = serde_json::from_str(include_str!("catalog.json"))
            .expect("bundled catalog.json is valid JSON matching the catalog schema");
        assert_eq!(root.models.len(), CATALOG.len());
        for model in &root.models {
            let raw = model
                .sha256
                .as_deref()
                .unwrap_or_else(|| panic!("{} has no sha256", model.id));
            let normalized = normalize_sha256(Some(raw))
                .unwrap_or_else(|| panic!("{} sha256 is not 64 lowercase hex: {raw}", model.id));
            assert_eq!(normalized.len(), 64, "{}", model.id);
            assert_eq!(
                normalized, raw,
                "{} sha256 must be stored lowercase",
                model.id
            );

            let prefix = format!("{}/", model.id);
            let matched: Vec<_> = CATALOG
                .iter()
                .filter(|d| d.id.starts_with(&prefix))
                .collect();
            assert_eq!(matched.len(), 1, "{}", model.id);
            match &matched[0].source {
                ModelSource::HuggingFace {
                    sha256: Some(hash), ..
                } => assert_eq!(hash, &normalized, "{}", model.id),
                other => panic!("{} source does not carry the pin: {other:?}", model.id),
            }
        }
    }

    #[test]
    fn compliance_gated_models_are_dropped() {
        let root: CatalogRoot = serde_json::from_str(include_str!("catalog.json")).unwrap();
        // Held rows are cut from catalog.json; the list stays so copies already in
        // the HF cache are still withheld.
        for id in LEGAL_HOLD_IDS {
            assert!(
                !root.models.iter().any(|m| m.id == *id),
                "held id {id} is back in catalog.json"
            );
            assert!(
                !CATALOG.iter().any(|d| d.id.starts_with(&format!("{id}/"))),
                "catalog must not offer gated id {id}"
            );
        }

        // A new non-allowlisted model in a regenerated catalog must fail here and be reviewed.
        assert_eq!(
            root.models.len() - CATALOG.len(),
            0,
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

        // Nothing is cleared, so "other" is never offered.
        assert!(CLEARED_OTHER_IDS.is_empty());
        assert!(!is_allowed_catalog_entry(
            "handy-computer/Fun-ASR-Nano-2512-gguf",
            Some("other")
        ));
    }

    #[test]
    fn legal_hold_ids_match_the_generator_held_list() {
        // Two independent lists: a typo in either fails here, so a held copy in the
        // HF cache can't slip past model_on_legal_hold.
        let gen = include_str!("../../../scripts/gen_catalog.py");
        let block = gen
            .split("HELD = {")
            .nth(1)
            .and_then(|s| s.split('}').next())
            .expect("HELD set in scripts/gen_catalog.py");
        let held: Vec<&str> = block.split('"').skip(1).step_by(2).collect();
        assert_eq!(held.len(), LEGAL_HOLD_IDS.len());
        for id in LEGAL_HOLD_IDS {
            let slug = id
                .strip_prefix("handy-computer/")
                .and_then(|s| s.strip_suffix("-gguf"))
                .unwrap_or_else(|| panic!("held id {id} is not handy-computer/<slug>-gguf"));
            assert!(held.contains(&slug), "{id} is not in gen_catalog.py HELD");
        }
    }

    #[test]
    fn model_on_legal_hold_is_case_insensitive() {
        assert!(model_on_legal_hold("handy-computer/SenseVoiceSmall-gguf"));
        assert!(model_on_legal_hold("HANDY-COMPUTER/SENSEVOICESMALL-GGUF"));
        assert!(!model_on_legal_hold("handy-computer/whisper-small-gguf"));
        // A caller can pass another list without editing LEGAL_HOLD_IDS.
        assert!(id_on_hold("small", &["SMALL"]));
        assert!(!id_on_hold("small", LEGAL_HOLD_IDS));
    }

    #[test]
    fn repo_withheld_behavior() {
        // True for a held ID in any case
        assert!(repo_withheld("handy-computer/SenseVoiceSmall-gguf"));
        assert!(repo_withheld("HANDY-COMPUTER/SENSEVOICESMALL-GGUF"));

        // Withheld exactly when the allowlist rejects the catalog entry.
        assert!(!LEGAL_HOLD_IDS.is_empty());
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

    #[derive(Deserialize)]
    struct LicenceFile {
        models: HashMap<String, LicenceEntry>,
    }

    #[derive(Deserialize)]
    struct LicenceEntry {
        licence: String,
        revision: String,
        url: String,
    }

    /// Built-in ids `ModelManager::new` inserts before the catalog seed.
    /// ponytail: scans those string-literal inserts instead of booting Tauri.
    /// If a builtin stops being `"id".to_string()` on the next line, parse that
    /// shape here.
    fn builtin_legacy_model_ids() -> Vec<&'static str> {
        let src = include_str!("../managers/model.rs");
        let start = src
            .find("let mut available_models = HashMap::new();")
            .expect("builtin model registry");
        let end = src
            .find("Self::seed_catalog_models")
            .expect("catalog seed follows builtin models");
        let lines: Vec<&str> = src[start..end].lines().collect();
        let mut ids = Vec::new();
        for (i, line) in lines.iter().enumerate() {
            if line.trim() == "available_models.insert(" {
                let key = lines[i + 1].trim();
                let id = key
                    .strip_prefix('"')
                    .and_then(|s| s.strip_suffix("\".to_string(),"))
                    .unwrap_or_else(|| panic!("builtin insert key is not a string literal: {key}"));
                ids.push(id);
            }
        }
        ids
    }

    #[test]
    fn every_downloadable_model_has_a_licence_entry() {
        let file: LicenceFile =
            serde_json::from_str(include_str!("model_licences.json")).expect("model_licences.json");
        const ALLOWED: &[&str] = &["MIT", "Apache-2.0", "CC-BY-4.0"];
        for (id, entry) in &file.models {
            assert!(
                ALLOWED.contains(&entry.licence.as_str()),
                "{id} licence {:?} is not MIT, Apache-2.0, or CC-BY-4.0",
                entry.licence
            );
            assert!(
                entry.revision.len() == 40 && entry.revision.bytes().all(|b| b.is_ascii_hexdigit()),
                "{id} revision {:?} is not 40 hex",
                entry.revision
            );
            assert!(
                entry.url.contains(&entry.revision),
                "{id} url does not contain its revision"
            );
        }

        for id in LEGAL_HOLD_IDS {
            assert!(
                !file.models.contains_key(*id),
                "held model {id} must not have a licence entry"
            );
        }

        let root: CatalogRoot = serde_json::from_str(include_str!("catalog.json")).unwrap();
        let mut required: Vec<String> = root
            .models
            .iter()
            .filter(|m| is_allowed_catalog_entry(&m.id, m.license.as_deref()))
            .map(|m| m.id.clone())
            .collect();
        assert!(!required.is_empty(), "catalog offered no models");
        let builtins = builtin_legacy_model_ids();
        assert!(builtins.len() >= 15, "builtin id scan found {builtins:?}");
        required.extend(builtins.iter().map(|id| (*id).to_string()));
        for id in &required {
            assert!(
                file.models.contains_key(id),
                "downloadable model {id} has no licence entry"
            );
        }
    }

    /// PR 5: the turbo label matches its MIT notice, and every moved pin names the
    /// commit handy-computer's NOTICE records for that conversion.
    #[test]
    fn licence_pins_follow_the_handy_notice() {
        let root: CatalogRoot = serde_json::from_str(include_str!("catalog.json")).unwrap();
        let turbo = root
            .models
            .iter()
            .find(|m| m.id == "handy-computer/whisper-large-v3-turbo-gguf")
            .expect("turbo row in catalog.json");
        assert_eq!(turbo.license.as_deref(), Some("mit"));

        // The 13 checked NOTICE commits, plus the 4 slug keys that stored the
        // same old revision as their handy-computer sibling.
        let pins: [(&str, &str); 17] = [
            (
                "handy-computer/parakeet-tdt-0.6b-v3-gguf",
                "6d590f77001d318fb17a0b5bf7ee329a91b52598",
            ),
            (
                "handy-computer/parakeet-tdt-0.6b-v2-gguf",
                "1b149a3589351c96ddb101709fe7dd9c7069572f",
            ),
            (
                "handy-computer/canary-1b-flash-gguf",
                "a9a55e0295e7dd50d0c8c2a19491900a0daf24f3",
            ),
            (
                "handy-computer/canary-1b-v2-gguf",
                "87bc52657add533cd0156b3fc1aef027280754bf",
            ),
            (
                "handy-computer/gigaam-v3-ctc-gguf",
                "15ef3b5a88da78f93134b3cb7f015c70aefa8946",
            ),
            (
                "handy-computer/gigaam-v3-e2e-ctc-gguf",
                "cec030b4c4f35d928e4a9044a3bdb29ebd499fac",
            ),
            (
                "handy-computer/gigaam-v3-rnnt-gguf",
                "c7f128b8accdd9624df905e5c2d7b7a48c27c0d8",
            ),
            (
                "handy-computer/granite-speech-4.1-2b-nar-gguf",
                "99a4df9007ac5682f9daa093fb7008ff606e9a5d",
            ),
            (
                "handy-computer/granite-speech-4.1-2b-gguf",
                "8f4bb5f31ae98971bd218169f00065a041d20058",
            ),
            (
                "handy-computer/granite-speech-4.1-2b-plus-gguf",
                "edd3bf54fbb06d8e263aa0c1939321d67b073f86",
            ),
            (
                "handy-computer/parakeet-rnnt-0.6b-gguf",
                "c0c1f09fdc3f18b0b2ddbeafd5d6684f1b38078f",
            ),
            (
                "handy-computer/parakeet-ctc-1.1b-gguf",
                "a707e818195cb97c8f7da2fc36b221a29f69a5db",
            ),
            (
                "handy-computer/parakeet-rnnt-1.1b-gguf",
                "a07b19e98a26c1873a3f2622c446a4a1ca6316cb",
            ),
            ("canary-1b-v2", "87bc52657add533cd0156b3fc1aef027280754bf"),
            (
                "gigaam-v3-e2e-ctc",
                "cec030b4c4f35d928e4a9044a3bdb29ebd499fac",
            ),
            (
                "parakeet-tdt-0.6b-v2",
                "1b149a3589351c96ddb101709fe7dd9c7069572f",
            ),
            (
                "parakeet-tdt-0.6b-v3",
                "6d590f77001d318fb17a0b5bf7ee329a91b52598",
            ),
        ];

        let file: LicenceFile = serde_json::from_str(include_str!("model_licences.json")).unwrap();
        for (id, pin) in pins {
            let entry = file
                .models
                .get(id)
                .unwrap_or_else(|| panic!("{id} has no licence entry"));
            assert_eq!(entry.revision, pin, "{id} revision");
            assert!(
                entry.url.contains(pin),
                "{id} url does not contain {pin}: {}",
                entry.url
            );
        }

        // Cohere stays put: the licence at the NOTICE commit could not be read.
        let cohere = file
            .models
            .get("handy-computer/cohere-transcribe-03-2026-gguf")
            .expect("cohere licence entry");
        assert_eq!(cohere.revision, "b1eacc2686a3d08ceaae5f24a88b1d519620bc09");
    }

    fn is_lowercase_40_hex(revision: &str) -> bool {
        revision.len() == 40
            && revision
                .bytes()
                .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    }

    fn descriptor_for(catalog_id: &str) -> &ModelDescriptor {
        CATALOG
            .iter()
            .find(|d| d.id.starts_with(&format!("{catalog_id}/")))
            .unwrap_or_else(|| panic!("{catalog_id} is missing from CATALOG"))
    }

    #[test]
    fn every_bundled_row_has_a_40_hex_lowercase_revision() {
        let root: CatalogRoot = serde_json::from_str(include_str!("catalog.json")).unwrap();
        assert!(!root.models.is_empty());
        for model in &root.models {
            let pinned = model
                .revision
                .as_deref()
                .unwrap_or_else(|| panic!("{} has no revision", model.id));
            assert!(
                is_lowercase_40_hex(pinned),
                "{} revision {pinned:?} is not 40 lowercase hex",
                model.id
            );
            match &descriptor_for(&model.id).source {
                ModelSource::HuggingFace { revision, .. } => {
                    assert_eq!(revision, pinned, "{}", model.id);
                    assert_ne!(revision, "main", "{}", model.id);
                }
                other => panic!("{} resolved to {other:?}", model.id),
            }
        }
    }

    #[test]
    fn resolved_download_sources_equal_catalog_ids() {
        let root: CatalogRoot = serde_json::from_str(include_str!("catalog.json")).unwrap();
        for model in &root.models {
            match &descriptor_for(&model.id).source {
                ModelSource::HuggingFace { repo_id, .. } => {
                    assert_eq!(repo_id, &model.id, "{}", model.id);
                    assert!(
                        !repo_id.contains("coco-research"),
                        "{repo_id} names coco-research"
                    );
                }
                other => panic!("{} resolved to {other:?}", model.id),
            }
        }
    }

    #[test]
    fn stay_rows_resolve_to_handy_computer() {
        for id in [
            "handy-computer/canary-qwen-2.5b-gguf",
            "handy-computer/cohere-transcribe-03-2026-gguf",
        ] {
            match &descriptor_for(id).source {
                ModelSource::HuggingFace { repo_id, .. } => {
                    assert_eq!(repo_id, id);
                    assert!(
                        repo_id.starts_with("handy-computer/"),
                        "{id} resolved to {repo_id}"
                    );
                }
                other => panic!("{id} resolved to {other:?}"),
            }
        }
    }

    #[test]
    fn qwen_row_resolves_to_qwen_at_the_copy_pin() {
        match &descriptor_for(QWEN3_REPO).source {
            ModelSource::HuggingFace {
                repo_id, revision, ..
            } => {
                assert_eq!(repo_id, QWEN3_REPO);
                assert_eq!(revision, "bc640142c66e1fdd12af0bd68f40445458f3869b");
            }
            other => panic!("qwen resolved to {other:?}"),
        }
    }

    #[test]
    fn source_repo_sets_the_download_repo_and_keeps_the_catalog_id() {
        let row = |source_repo: serde_json::Value| {
            serde_json::from_value::<CatalogModel>(serde_json::json!({
                "id": "handy-computer/example-gguf",
                "source_repo": source_repo,
                "name": "Example",
                "description": "",
                "architecture": null,
                "languages": [],
                "capabilities": {
                    "streaming": false,
                    "translate": false,
                    "lang_detect": false
                },
                "speed_score": null,
                "accuracy_score": null,
                "files": [{
                    "filename": "example.gguf",
                    "quant": "Q8_0",
                    "size_bytes": 1
                }],
                "default_quant": null,
                "recommended_rank": null,
                "license": "mit"
            }))
            .expect("inline catalog row")
        };

        let pinned =
            ModelDescriptor::try_from(row(serde_json::json!("coco-research/example-gguf")))
                .expect("descriptor");
        assert!(pinned.id.starts_with("handy-computer/example-gguf/"));
        match pinned.source {
            ModelSource::HuggingFace { repo_id, .. } => {
                assert_eq!(repo_id, "coco-research/example-gguf");
            }
            other => panic!("inline row resolved to {other:?}"),
        }

        // Empty and whitespace-only source_repo keep the catalog id.
        for blank in ["", "  "] {
            let d = ModelDescriptor::try_from(row(serde_json::json!(blank))).expect("descriptor");
            assert!(d.id.starts_with("handy-computer/example-gguf/"));
            match d.source {
                ModelSource::HuggingFace { repo_id, .. } => {
                    assert_eq!(repo_id, "handy-computer/example-gguf");
                }
                other => panic!("blank source_repo resolved to {other:?}"),
            }
        }
    }
}
