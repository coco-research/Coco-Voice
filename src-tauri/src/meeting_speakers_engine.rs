#![allow(dead_code, unused_imports)]
//! On-device post-Stop speaker segmentation and clustering engine (`pyannote` + `WeSpeaker`).

#![cfg(feature = "meetings")]

use anyhow::{anyhow, Result};
use ndarray::Array3;
use ort::inputs;
use ort::session::Session;
use ort::value::TensorRef;
use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use transcribe_rs::features::{compute_mel, MelConfig, WindowType};

use crate::managers::model::ModelManager;

/// Named cosine-distance grouping threshold (`>= 0.34`, not a user setting).
pub const SPEAKER_CLUSTER_THRESHOLD: f32 = 0.34;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpeakerSpan {
    pub start_ms: i64,
    pub end_ms: i64,
    pub speaker_label: String,
}

fn extract_embedding(emb_session: &mut Session, wav_slice: &[f32]) -> Result<Vec<f32>> {
    let mel_cfg = MelConfig {
        sample_rate: 16000,
        num_mels: 80,
        n_fft: 400,
        hop_length: 160,
        window: WindowType::Hamming,
        f_min: 20.0,
        f_max: None,
        pre_emphasis: Some(0.97),
        snip_edges: true,
        normalize_samples: false,
    };
    let mut feats = compute_mel(wav_slice, &mel_cfg);
    let (n_frames, n_mels) = (feats.nrows(), feats.ncols());
    if n_frames > 0 {
        for m in 0..n_mels {
            let mean: f32 = (0..n_frames).map(|f| feats[[f, m]]).sum::<f32>() / (n_frames as f32);
            for f in 0..n_frames {
                feats[[f, m]] -= mean;
            }
        }
    }
    let feat_dyn = feats
        .into_shape_with_order((1, n_frames, n_mels))?
        .into_dyn();
    let t_ref = TensorRef::from_array_view(feat_dyn.view()).map_err(|e| anyhow!("{e}"))?;
    let outputs = emb_session
        .run(inputs!["feats" => t_ref])
        .map_err(|e| anyhow!("{e}"))?;
    let embs = outputs[0]
        .try_extract_array::<f32>()
        .map_err(|e| anyhow!("{e}"))?;
    let norm = embs.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-12);
    Ok(embs.iter().map(|x| *x / norm).collect())
}

pub fn cluster_speaker_embeddings(
    embeddings: &[Vec<f32>],
    pairs: &[(usize, usize)],
    distance_threshold: f32,
) -> (Vec<usize>, Vec<Vec<usize>>) {
    if embeddings.is_empty() {
        return (vec![], vec![]);
    }
    let mut clusters: Vec<Vec<usize>> = (0..embeddings.len()).map(|i| vec![i]).collect();
    while clusters.len() > 1 {
        let (mut best_pair, mut best_dist) = (None, f32::INFINITY);
        for a in 0..clusters.len() {
            for b in (a + 1)..clusters.len() {
                if clusters[a].iter().any(|&ia| {
                    clusters[b]
                        .iter()
                        .any(|&ib| pairs[ia].0 == pairs[ib].0 && pairs[ia].1 != pairs[ib].1)
                }) {
                    continue;
                }
                let mut sum_dist = 0.0f32;
                for &ia in &clusters[a] {
                    for &ib in &clusters[b] {
                        let dot: f32 = embeddings[ia]
                            .iter()
                            .zip(embeddings[ib].iter())
                            .map(|(x, y)| x * y)
                            .sum();
                        sum_dist += 1.0 - dot;
                    }
                }
                let avg = sum_dist / ((clusters[a].len() * clusters[b].len()) as f32);
                if avg < best_dist {
                    best_dist = avg;
                    best_pair = Some((a, b));
                }
            }
        }
        match best_pair {
            Some((a, b)) if best_dist <= distance_threshold.max(SPEAKER_CLUSTER_THRESHOLD) => {
                let mut b_items = clusters.remove(b);
                clusters[a].append(&mut b_items);
            }
            _ => break,
        }
    }
    if clusters.iter().any(|c| c.len() >= 2) {
        let (mut main_clusters, mut singletons) = (Vec::new(), Vec::new());
        for c in clusters {
            if c.len() >= 2 {
                main_clusters.push(c);
            } else {
                singletons.extend(c);
            }
        }
        for s in singletons {
            let (best_c, _) = main_clusters
                .iter()
                .enumerate()
                .map(|(ci, mc)| {
                    let d: f32 = mc
                        .iter()
                        .map(|&m| {
                            1.0 - embeddings[s]
                                .iter()
                                .zip(embeddings[m].iter())
                                .map(|(x, y)| x * y)
                                .sum::<f32>()
                        })
                        .sum::<f32>()
                        / (mc.len() as f32);
                    (ci, d)
                })
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .unwrap();
            main_clusters[best_c].push(s);
        }
        clusters = main_clusters;
    }
    let mut labels = vec![0usize; embeddings.len()];
    for (cid, items) in clusters.iter().enumerate() {
        for &idx in items {
            labels[idx] = cid;
        }
    }
    (labels, clusters)
}
