//! Which speech models earn a "Recommended" tag: only those measured best in
//! the benchmark records shipped in `measured_runs.json`.
//!
//! A record is one headless run of one model over our clip set (see
//! `scripts/run-wer-eval.sh`): words wrong per 100 reference words, speed as a
//! multiple of real time, and peak memory. A model is tagged only when every
//! one of those three numbers was measured in the same run AND it beat every
//! other measured model. A model with no record, an incomplete record, or a
//! better-measured rival gets no tag. Nothing here reads the network or the disk
//! at run time; the records are compiled into the binary.

use std::cmp::Ordering;
use std::collections::HashMap;

use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::managers::model::ModelInfo;

/// The only clip set whose runs count. Runs on any other set are ignored.
pub const CLIP_SET: &str = "wer-dev-40";

#[derive(Debug, Clone, Deserialize)]
pub struct MeasuredRun {
    pub model_id: String,
    pub clip_set: String,
    #[serde(default)]
    pub words_wrong_per_100: Option<f64>,
    #[serde(default)]
    pub speed_x_realtime: Option<f64>,
    #[serde(default)]
    pub peak_memory_mb: Option<f64>,
}

#[derive(Deserialize)]
struct MeasuredFile {
    runs: Vec<MeasuredRun>,
}

/// Why a model is tagged: the numbers from its measured run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct MeasuredReason {
    pub clip_set: String,
    pub words_wrong_per_100: f32,
    pub speed_x_realtime: f32,
    pub peak_memory_mb: u32,
    /// How many models were measured on the clip set (always at least 2).
    pub models_compared: u32,
}

/// Pick the measured winner among `runs`, keyed by model id. Empty when there
/// is no clear winner.
pub fn measured_best(runs: &[MeasuredRun]) -> HashMap<String, MeasuredReason> {
    // One complete record per model on our clip set; a later record replaces an
    // earlier one for the same model.
    let mut latest: HashMap<&str, Complete> = HashMap::new();
    for run in runs.iter().filter(|r| r.clip_set == CLIP_SET) {
        match Complete::from_run(run) {
            Some(done) => {
                latest.insert(run.model_id.as_str(), done);
            }
            None => {
                latest.remove(run.model_id.as_str());
            }
        }
    }
    if latest.len() < 2 {
        return HashMap::new();
    }

    let mut ranked: Vec<(&str, Complete)> = latest.into_iter().collect();
    ranked.sort_by(|a, b| a.1.rank(&b.1));
    let compared = ranked.len() as u32;
    let (winner_id, winner) = ranked[0];
    if ranked[0].1.rank(&ranked[1].1) == Ordering::Equal {
        return HashMap::new();
    }

    HashMap::from([(
        winner_id.to_string(),
        MeasuredReason {
            clip_set: CLIP_SET.to_string(),
            words_wrong_per_100: winner.wer as f32,
            speed_x_realtime: winner.speed as f32,
            peak_memory_mb: winner.memory.round() as u32,
            models_compared: compared,
        },
    )])
}

/// Set `measured_best` on the speech models in `models` that won in `runs`, and
/// clear it on every other one, so only a measured winner can carry the tag.
pub fn tag_measured_best(models: &mut [ModelInfo], runs: &[MeasuredRun]) {
    let best = measured_best(runs);
    for model in models.iter_mut() {
        model.measured_best = best.get(&model.id).cloned();
    }
}

/// A run with all three measurements present and sensible.
#[derive(Clone, Copy)]
struct Complete {
    wer: f64,
    speed: f64,
    memory: f64,
}

impl Complete {
    fn from_run(run: &MeasuredRun) -> Option<Self> {
        let wer = run
            .words_wrong_per_100
            .filter(|v| v.is_finite() && *v >= 0.0)?;
        let speed = run.speed_x_realtime.filter(|v| v.is_finite() && *v > 0.0)?;
        let memory = run.peak_memory_mb.filter(|v| v.is_finite() && *v > 0.0)?;
        Some(Self { wer, speed, memory })
    }

    /// Better sorts first: fewer words wrong, then faster, then less memory.
    fn rank(&self, other: &Self) -> Ordering {
        self.wer
            .total_cmp(&other.wer)
            .then(other.speed.total_cmp(&self.speed))
            .then(self.memory.total_cmp(&other.memory))
    }
}

/// The bundled records, parsed once.
pub static BUNDLED_RUNS: Lazy<Vec<MeasuredRun>> = Lazy::new(|| {
    let file: MeasuredFile = serde_json::from_str(include_str!("measured_runs.json"))
        .expect("bundled measured_runs.json is valid JSON matching the schema");
    file.runs
});

#[cfg(test)]
mod tests {
    use super::*;

    fn run(id: &str, wer: f64, speed: f64, mem: f64) -> MeasuredRun {
        MeasuredRun {
            model_id: id.to_string(),
            clip_set: CLIP_SET.to_string(),
            words_wrong_per_100: Some(wer),
            speed_x_realtime: Some(speed),
            peak_memory_mb: Some(mem),
        }
    }

    #[test]
    fn no_records_means_no_tags() {
        assert!(measured_best(&[]).is_empty());
    }

    #[test]
    fn lowest_words_wrong_wins_and_carries_its_numbers() {
        let best = measured_best(&[run("a", 6.0, 20.0, 900.0), run("b", 4.5, 8.0, 1500.0)]);
        assert_eq!(best.len(), 1);
        let reason = &best["b"];
        assert_eq!(reason.clip_set, CLIP_SET);
        assert_eq!(reason.words_wrong_per_100, 4.5);
        assert_eq!(reason.speed_x_realtime, 8.0);
        assert_eq!(reason.peak_memory_mb, 1500);
        assert_eq!(reason.models_compared, 2);
    }

    #[test]
    fn a_single_measured_model_has_nothing_to_beat() {
        assert!(measured_best(&[run("a", 1.0, 50.0, 100.0)]).is_empty());
    }

    #[test]
    fn a_model_without_a_record_is_never_tagged() {
        let best = measured_best(&[run("a", 6.0, 20.0, 900.0), run("b", 4.5, 8.0, 1500.0)]);
        assert!(!best.contains_key("untested"));
        assert!(!best.contains_key("a"));
    }

    #[test]
    fn a_record_missing_any_measurement_is_not_counted() {
        for broken in [
            MeasuredRun {
                words_wrong_per_100: None,
                ..run("x", 0.1, 99.0, 10.0)
            },
            MeasuredRun {
                speed_x_realtime: None,
                ..run("x", 0.1, 99.0, 10.0)
            },
            MeasuredRun {
                peak_memory_mb: None,
                ..run("x", 0.1, 99.0, 10.0)
            },
            run("x", f64::NAN, 99.0, 10.0),
            run("x", 0.1, 0.0, 10.0),
            run("x", 0.1, 99.0, -5.0),
        ] {
            let best = measured_best(&[
                run("a", 6.0, 20.0, 900.0),
                run("b", 4.5, 8.0, 1500.0),
                broken,
            ]);
            assert!(!best.contains_key("x"), "incomplete record was tagged");
            assert!(best.contains_key("b"), "complete runs still decide");
            assert_eq!(best["b"].models_compared, 2);
        }
    }

    #[test]
    fn runs_on_other_clip_sets_are_ignored() {
        let other = MeasuredRun {
            clip_set: "other".into(),
            ..run("x", 0.1, 99.0, 10.0)
        };
        let best = measured_best(&[
            run("a", 6.0, 20.0, 900.0),
            run("b", 4.5, 8.0, 1500.0),
            other,
        ]);
        assert!(!best.contains_key("x"));
        assert!(best.contains_key("b"));
    }

    #[test]
    fn equal_words_wrong_falls_to_speed_then_memory() {
        let best = measured_best(&[run("a", 5.0, 10.0, 900.0), run("b", 5.0, 12.0, 1500.0)]);
        assert!(best.contains_key("b"));
        let best = measured_best(&[run("a", 5.0, 10.0, 900.0), run("b", 5.0, 10.0, 800.0)]);
        assert!(best.contains_key("b"));
    }

    #[test]
    fn a_full_tie_tags_nobody() {
        assert!(
            measured_best(&[run("a", 5.0, 10.0, 900.0), run("b", 5.0, 10.0, 900.0)]).is_empty()
        );
    }

    #[test]
    fn the_latest_record_for_a_model_replaces_earlier_ones() {
        let best = measured_best(&[
            run("a", 1.0, 20.0, 900.0),
            run("b", 4.5, 8.0, 1500.0),
            run("a", 9.0, 20.0, 900.0),
        ]);
        assert!(best.contains_key("b"));
        assert!(!best.contains_key("a"));
    }

    fn speech_infos(n: usize) -> Vec<ModelInfo> {
        crate::catalog::CATALOG
            .iter()
            .filter(|d| d.engine_type.is_speech())
            .take(n)
            .map(|d| d.to_model_info(&Default::default()))
            .collect()
    }

    #[test]
    fn only_the_measured_winner_in_the_model_list_is_tagged() {
        let mut models = speech_infos(4);
        assert_eq!(models.len(), 4, "catalog has at least four speech models");
        for m in models.iter_mut() {
            // A catalog flag or a stale tag must not survive without a measurement.
            m.is_recommended = true;
            m.measured_best = Some(MeasuredReason {
                clip_set: CLIP_SET.into(),
                words_wrong_per_100: 0.0,
                speed_x_realtime: 1.0,
                peak_memory_mb: 1,
                models_compared: 2,
            });
        }
        let runs = [
            run(&models[0].id, 6.0, 20.0, 900.0),
            run(&models[1].id, 4.5, 8.0, 1500.0),
        ];
        tag_measured_best(&mut models, &runs);
        assert!(models[0].measured_best.is_none(), "beaten");
        assert_eq!(
            models[1]
                .measured_best
                .as_ref()
                .unwrap()
                .words_wrong_per_100,
            4.5
        );
        assert!(models[2].measured_best.is_none(), "untested");
        assert!(models[3].measured_best.is_none(), "untested");
    }

    #[test]
    fn bundled_records_only_name_catalog_speech_models() {
        let speech: std::collections::HashSet<String> = crate::catalog::CATALOG
            .iter()
            .filter(|d| d.engine_type.is_speech())
            .map(|d| d.id.clone())
            .collect();
        for r in BUNDLED_RUNS.iter() {
            assert!(
                speech.contains(&r.model_id),
                "{} is not a catalog speech model",
                r.model_id
            );
        }
    }
}
