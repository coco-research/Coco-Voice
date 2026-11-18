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

pub fn diarize_others_samples(
    audio: &[f32],
    seg_model_path: &Path,
    emb_model_path: &Path,
) -> Result<Vec<SpeakerSpan>> {
    ModelManager::verify_speaker_model_file(
        seg_model_path,
        ModelManager::PYANNOTE_SEGMENTATION_ONNX_SHA256,
        ModelManager::PYANNOTE_SEGMENTATION_MODEL_ID,
    )?;
    ModelManager::verify_speaker_model_file(
        emb_model_path,
        ModelManager::WESPEAKER_EMBEDDING_SHA256,
        ModelManager::WESPEAKER_EMBEDDING_MODEL_ID,
    )?;
    if audio.is_empty() {
        return Ok(vec![]);
    }
    let mut seg_session = Session::builder()
        .map_err(|e| anyhow!("{e}"))?
        .with_intra_threads(4)
        .map_err(|e| anyhow!("{e}"))?
        .commit_from_file(seg_model_path)
        .map_err(|e| anyhow!("{e}"))?;
    let mut emb_session = Session::builder()
        .map_err(|e| anyhow!("{e}"))?
        .with_intra_threads(4)
        .map_err(|e| anyhow!("{e}"))?
        .commit_from_file(emb_model_path)
        .map_err(|e| anyhow!("{e}"))?;

    let (window_size, window_shift, rf_size, rf_shift, num_local_spk) =
        (160_000usize, 16_000usize, 991usize, 270usize, 3usize);
    let mapping: [[u8; 3]; 7] = [
        [0, 0, 0],
        [1, 0, 0],
        [0, 1, 0],
        [0, 0, 1],
        [1, 1, 0],
        [1, 0, 1],
        [0, 1, 1],
    ];
    let (mut chunks, mut pos) = (Vec::new(), 0usize);
    while pos + window_size <= audio.len() {
        chunks.push(audio[pos..pos + window_size].to_vec());
        pos += window_shift;
    }
    let has_last_chunk =
        audio.len() < window_size || (audio.len() - window_size) % window_shift > 0;
    if has_last_chunk {
        let (start, mut last) = (chunks.len() * window_shift, vec![0.0f32; window_size]);
        last[..audio.len() - start].copy_from_slice(&audio[start..]);
        chunks.push(last);
    }
    let num_chunks = chunks.len();
    let (mut labels, mut num_chunk_frames) = (Vec::with_capacity(num_chunks), 589usize);
    for b_start in (0..num_chunks).step_by(32) {
        let b_end = (b_start + 32).min(num_chunks);
        let b_len = b_end - b_start;
        let batch_flat: Vec<f32> = chunks[b_start..b_end].iter().flatten().copied().collect();
        let x_dyn = Array3::from_shape_vec((b_len, 1, window_size), batch_flat)?.into_dyn();
        let t_ref = TensorRef::from_array_view(x_dyn.view()).map_err(|e| anyhow!("{e}"))?;
        let outputs = seg_session
            .run(inputs!["x" => t_ref])
            .map_err(|e| anyhow!("{e}"))?;
        let y = outputs[0]
            .try_extract_array::<f32>()
            .map_err(|e| anyhow!("{e}"))?;
        let n_classes = y.shape()[2];
        num_chunk_frames = y.shape()[1];
        for bi in 0..b_len {
            labels.push(
                (0..num_chunk_frames)
                    .map(|f| {
                        mapping[(0..n_classes)
                            .max_by(|&a, &b| y[[bi, f, a]].total_cmp(&y[[bi, f, b]]))
                            .unwrap_or(0)]
                    })
                    .collect::<Vec<[u8; 3]>>(),
            );
        }
    }
    let total_frames = ((window_size + (num_chunks - 1) * window_shift) / rf_shift) + 1;
    let (mut spk_sum, mut spk_cnt) = (vec![0.0f32; total_frames], vec![0.0f32; total_frames]);
    for (i, chunk_lbl) in labels.iter().enumerate() {
        let start = ((i * window_shift) as f32 / rf_shift as f32 + 0.5) as usize;
        for (f, lbl) in chunk_lbl.iter().enumerate() {
            if start + f < total_frames {
                spk_sum[start + f] += (lbl[0] + lbl[1] + lbl[2]) as f32;
                spk_cnt[start + f] += 1.0;
            }
        }
    }
    let speakers_per_frame: Vec<usize> = spk_sum
        .iter()
        .zip(spk_cnt.iter())
        .map(|(&s, &c)| ((s / c.max(1e-12)) + 0.5) as usize)
        .collect();
    let (mut pairs, mut embeddings) = (Vec::new(), Vec::new());
    for i in 0..num_chunks {
        let sample_offset = i * window_shift;
        for j in 0..num_local_spk {
            let clean_active = (0..num_chunk_frames)
                .filter(|&f| {
                    labels[i][f][j] == 1
                        && (labels[i][f][0] + labels[i][f][1] + labels[i][f][2]) == 1
                })
                .count();
            if (0..num_chunk_frames)
                .filter(|&f| labels[i][f][j] == 1)
                .count()
                < 10
            {
                continue;
            }
            let (exclude_overlap, mut buf, mut start_f) =
                (clean_active >= 12, Vec::with_capacity(window_size), None);
            for k in 0..=num_chunk_frames {
                let active = k < num_chunk_frames
                    && if exclude_overlap {
                        labels[i][k][j] == 1
                            && (labels[i][k][0] + labels[i][k][1] + labels[i][k][2]) == 1
                    } else {
                        labels[i][k][j] == 1
                    };
                match (active, start_f) {
                    (true, None) => start_f = Some(k),
                    (false, Some(s)) => {
                        let (s_samp, e_samp) = (
                            (s * window_size / num_chunk_frames) + sample_offset,
                            (k * window_size / num_chunk_frames) + sample_offset,
                        );
                        if s_samp < audio.len() {
                            buf.extend_from_slice(&audio[s_samp..e_samp.min(audio.len())]);
                        }
                        start_f = None;
                    }
                    _ => {}
                }
            }
            if buf.len() >= 3200 {
                embeddings.push(extract_embedding(&mut emb_session, &buf)?);
                pairs.push((i, j));
            }
        }
    }
    if embeddings.is_empty() {
        return Err(anyhow!("no speaker embeddings produced"));
    }
    let (_, clusters) = cluster_speaker_embeddings(&embeddings, &pairs, SPEAKER_CLUSTER_THRESHOLD);
    for emb in &mut embeddings {
        emb.fill(0.0);
    }
    drop(embeddings);

    let num_clusters = clusters.len();
    let mut pair_to_cluster: HashMap<(usize, usize), usize> = HashMap::new();
    for (cid, items) in clusters.iter().enumerate() {
        for &idx in items {
            pair_to_cluster.insert(pairs[idx], cid);
        }
    }
    let mut count = vec![vec![0.0f32; num_clusters]; total_frames];
    for i in 0..num_chunks {
        let start = ((i * window_shift) as f32 / rf_shift as f32 + 0.5) as usize;
        for j in 0..num_chunk_frames {
            if start + j < total_frames {
                for k in 0..num_local_spk {
                    if labels[i][j][k] == 1 {
                        if let Some(&cid) = pair_to_cluster.get(&(i, k)) {
                            count[start + j][cid] += 1.0;
                        }
                    }
                }
            }
        }
    }
    let stop_frame = if has_last_chunk {
        (audio.len() / rf_shift).min(total_frames)
    } else {
        total_frames
    };
    let mut final_act = vec![vec![0u8; stop_frame]; num_clusters];
    for f in 0..stop_frame {
        let c = speakers_per_frame[f];
        if c == 0 {
            continue;
        }
        let mut order: Vec<usize> = (0..num_clusters).collect();
        order.sort_by(|&a, &b| count[f][b].total_cmp(&count[f][a]));
        for &cid in order.iter().take(c.min(num_clusters)) {
            if count[f][cid] > 0.0 {
                final_act[cid][f] = 1;
            }
        }
    }
    let (scale, scale_offset) = (rf_shift as f64 / 16000.0, rf_size as f64 / 16000.0 * 0.5);
    let mut raw_spans: Vec<(i64, i64, usize)> = Vec::new();
    for (kk, frames) in final_act.iter().enumerate() {
        let (mut raw_segs, mut start_opt) = (Vec::new(), None);
        for i in 0..=frames.len() {
            match (i < frames.len() && frames[i] > 0, start_opt) {
                (true, None) => start_opt = Some(i),
                (false, Some(s)) => {
                    let end_idx = if i == frames.len() {
                        frames.len().saturating_sub(1)
                    } else {
                        i
                    };
                    raw_segs.push((
                        s as f64 * scale + scale_offset,
                        end_idx as f64 * scale + scale_offset,
                    ));
                    start_opt = None;
                }
                _ => {}
            }
        }
        let mut merged: Vec<(f64, f64)> = Vec::new();
        for (s, e) in raw_segs {
            if let Some(last) = merged.last_mut() {
                if s - last.1 <= 0.5 {
                    last.1 = last.1.max(e);
                    continue;
                }
            }
            merged.push((s, e));
        }
        for (s, e) in merged {
            if e - s > 0.3 {
                raw_spans.push(((s * 1000.0).round() as i64, (e * 1000.0).round() as i64, kk));
            }
        }
    }
    raw_spans.sort_by_key(|x| (x.0, x.1, x.2));
    let (mut order_map, mut next_num) = (BTreeMap::new(), 1usize);
    Ok(raw_spans
        .into_iter()
        .map(|(s_ms, e_ms, cid)| {
            let num = *order_map.entry(cid).or_insert_with(|| {
                let n = next_num;
                next_num += 1;
                n
            });
            SpeakerSpan {
                start_ms: s_ms,
                end_ms: e_ms,
                speaker_label: format!("Speaker {num}"),
            }
        })
        .collect())
}
