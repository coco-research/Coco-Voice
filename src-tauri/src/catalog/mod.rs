//! The bundled, offline model catalog.
//!
//! `catalog.json` is pinned to `coco-research/coco-voice-models` at commit
//! `3cb3e6de7c20f58fe5beaaab3d5159384feff3f0`. Each entry keeps its old
//! `handy-computer/<slug>-gguf` id so a saved selection still matches, and
//! downloads the mirrored default quant only.
//!
//! Each entry is normalised into a [`ModelDescriptor`].

use std::collections::{HashMap, HashSet};

use once_cell::sync::Lazy;
use serde::Deserialize;

use crate::managers::model::{
    default_quant_file, EngineType, ModelDescriptor, ModelSource, QuantFile,
};
use crate::managers::model_capabilities::{CapabilityProbe, Compatibility};

pub const MIRROR_REPO: &str = "coco-research/coco-voice-models";
#[cfg_attr(not(test), allow(dead_code))]
pub const MIRROR_REVISION: &str = "3cb3e6de7c20f58fe5beaaab3d5159384feff3f0";

#[derive(Deserialize)]
struct CatalogRoot {
    models: Vec<CatalogModel>,
}

/// One model as written in `catalog.json`. Only the fields the descriptor needs
/// are declared; serde ignores the rest (slug, family, license, …).
#[derive(Deserialize)]
struct CatalogModel {
    /// Legacy repo id, e.g. `handy-computer/whisper-small-gguf`. Kept as the
    /// stable id prefix.
    id: String,
    #[serde(default)]
    repo_id: Option<String>,
    #[serde(default)]
    revision: Option<String>,
    /// Path inside the download repo (`<slug>/<file>`). Empty means `filename`.
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    sha256: Option<String>,
    #[serde(default)]
    legacy_repo_id: Option<String>,
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
    #[serde(default)]
    recommended: bool,
}

#[derive(Deserialize)]
struct CatalogCaps {
    streaming: bool,
    translate: bool,
    lang_detect: bool,
}

impl From<CatalogModel> for ModelDescriptor {
    fn from(m: CatalogModel) -> Self {
        let default_filename = default_quant_file(&m.files, m.default_quant.as_deref())
            .map(|f| f.filename.clone())
            .unwrap_or_default();
        let repo_id = m.repo_id.unwrap_or_else(|| m.id.clone());
        let legacy = m.legacy_repo_id.or_else(|| {
            if repo_id != m.id {
                Some(m.id.clone())
            } else {
                None
            }
        });

        ModelDescriptor {
            id: format!("{}/{}", m.id, default_filename),
            source: ModelSource::HuggingFace {
                repo_id,
                revision: m.revision.unwrap_or_else(|| "main".to_string()),
                path: m.path.unwrap_or_default(),
                sha256: m.sha256,
                legacy_repo_id: legacy,
            },
            name: m.name,
            description: m.description,
            engine_type: EngineType::TranscribeCpp,
            caps: CapabilityProbe {
                verdict: Compatibility::Compatible,
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

/// Recommended rank for a model id (lower = higher priority). Returns
/// `u32::MAX` for unranked/unknown ids so they sort last in an ascending sort.
pub fn rank_of(model_id: &str) -> u32 {
    RANK_BY_ID.get(model_id).copied().unwrap_or(u32::MAX)
}

#[derive(Deserialize)]
struct RestrictedFile {
    notices: RestrictedNotices,
    review: Vec<String>,
    noncommercial: Vec<String>,
}

#[derive(Deserialize)]
struct RestrictedNotices {
    review: String,
    noncommercial: String,
}

struct RestrictedIndex {
    review: HashSet<String>,
    noncommercial: HashSet<String>,
    review_notice: String,
    noncommercial_notice: String,
}

static RESTRICTED: Lazy<RestrictedIndex> = Lazy::new(|| {
    let raw: RestrictedFile = serde_json::from_str(include_str!(
        "../../../src/lib/constants/restricted-models.json"
    ))
    .expect("restricted-models.json");
    RestrictedIndex {
        review: raw.review.into_iter().collect(),
        noncommercial: raw.noncommercial.into_iter().collect(),
        review_notice: raw.notices.review,
        noncommercial_notice: raw.notices.noncommercial,
    }
});

fn is_quant_token(token: &str) -> bool {
    let token = token.strip_suffix(".gguf").unwrap_or(token);
    if matches!(token, "F16" | "F32" | "BF16") {
        return true;
    }
    let rest = token
        .strip_prefix("IQ")
        .or_else(|| token.strip_prefix('Q'))
        .unwrap_or("");
    rest.chars().next().is_some_and(|c| c.is_ascii_digit())
}

fn matches_restricted_slug(repo_id: &str, filename: &str, slug: &str) -> bool {
    let legacy = format!("handy-computer/{slug}-gguf");
    if repo_id == legacy {
        return true;
    }
    let base = filename.rsplit('/').next().unwrap_or(filename);
    let stem = base.strip_suffix(".gguf").unwrap_or(base);
    if stem == slug {
        return true;
    }
    stem.strip_prefix(&format!("{slug}-"))
        .is_some_and(is_quant_token)
}

/// License label for an already-downloaded model that is no longer offered.
/// `None` for everything we still distribute. The slug list is the JSON file
/// so the five review entries can move later without a code change.
pub fn license_notice(repo_id: &str, filename: &str) -> Option<String> {
    let index = &*RESTRICTED;
    if index
        .review
        .iter()
        .any(|slug| matches_restricted_slug(repo_id, filename, slug))
    {
        return Some(index.review_notice.clone());
    }
    if index
        .noncommercial
        .iter()
        .any(|slug| matches_restricted_slug(repo_id, filename, slug))
    {
        return Some(index.noncommercial_notice.clone());
    }
    None
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
        let missing: BTreeSet<&str> = CATALOG
            .iter()
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
    fn catalog_is_pinned_mirror_default_quant_only() {
        assert_eq!(CATALOG.len(), 47);
        for d in CATALOG.iter() {
            match &d.source {
                ModelSource::HuggingFace {
                    repo_id,
                    revision,
                    path,
                    sha256,
                    legacy_repo_id,
                } => {
                    assert_eq!(repo_id, MIRROR_REPO, "{}", d.id);
                    assert_eq!(revision, MIRROR_REVISION, "{}", d.id);
                    assert!(path.contains('/'), "{}", d.id);
                    assert_eq!(sha256.as_ref().map(|s| s.len()), Some(64), "{}", d.id);
                    assert!(d.id.starts_with("handy-computer/"), "{}", d.id);
                    assert_eq!(
                        legacy_repo_id.as_deref(),
                        Some(d.id.rsplit_once('/').unwrap().0)
                    );
                }
                other => panic!("unexpected source for {}: {other:?}", d.id),
            }
            assert_eq!(d.files.len(), 1, "{}", d.id);
            assert!(
                license_notice("", &d.files[0].filename).is_none(),
                "{}",
                d.id
            );
        }
    }

    #[test]
    fn restricted_notice_matches_local_copies_only() {
        assert_eq!(
            license_notice("handy-computer/canary-1b-gguf", "canary-1b-Q5_K_M.gguf").as_deref(),
            Some("Non-commercial license, personal use only")
        );
        assert_eq!(
            license_notice("handy-computer/medasr-gguf", "medasr-Q8_0.gguf").as_deref(),
            Some("License under review")
        );
        assert!(license_notice(
            "handy-computer/canary-1b-v2-gguf",
            "canary-1b-v2-Q5_K_M.gguf"
        )
        .is_none());
        assert!(license_notice(
            "handy-computer/moonshine-base-gguf",
            "moonshine-base-Q8_0.gguf"
        )
        .is_none());
        assert_eq!(
            license_notice("", "moonshine-base-ar-Q8_0.gguf").as_deref(),
            Some("Non-commercial license, personal use only")
        );
    }
}
