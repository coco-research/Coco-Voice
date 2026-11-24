//! Per-meeting speaker attribution, manual speaker rename, Markdown export, and
//! zero-voiceprint persistence guards.

#![cfg(feature = "meetings")]

use anyhow::{anyhow, bail, Result};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::audio_toolkit::read_wav_samples;
use crate::managers::meetings::MeetingsStore;
use crate::meeting_speakers_engine::{diarize_others_samples, SpeakerSpan};

/// Compile-time flag for optional speaker diarization (`AC-X41` — OFF by default).
pub const SPEAKER_SPLIT_ENABLED: bool = cfg!(feature = "speaker-split");

/// Hard wall-clock cap (10 minutes) for post-Stop speaker splitting (`AC-X22`).
pub const SPEAKER_SPLIT_TIMEOUT_SECS: u64 = 600;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct MeetingSegmentRow {
    pub id: i64,
    pub start_ms: i64,
    pub end_ms: i64,
    pub channel: String,
    pub speaker: String,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct MeetingNoteRow {
    pub id: i64,
    pub section: String,
    pub speaker: String,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct MeetingSpeakerSlot {
    pub slot: String,
    pub custom_name: String,
}

fn is_me_channel(ch: &str) -> bool {
    ch.trim().eq_ignore_ascii_case("me") || ch.trim().eq_ignore_ascii_case("mic")
}

pub fn insert_meeting_segment(
    store: &MeetingsStore,
    meeting_id: i64,
    start_ms: i64,
    end_ms: i64,
    channel: &str,
    text: &str,
) -> Result<i64> {
    let conn = store.connection()?;
    let slot = if is_me_channel(channel) {
        "Me"
    } else {
        "Others"
    };
    conn.execute(
        "INSERT INTO meeting_segments (meeting_id, start_ms, end_ms, channel, speaker_label, text) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![meeting_id, start_ms, end_ms, channel, slot, text],
    )?;
    Ok(conn.last_insert_rowid())
}

fn ensure_notes_table(conn: &rusqlite::Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS meeting_notes (id INTEGER PRIMARY KEY, meeting_id INTEGER NOT NULL, kind TEXT NOT NULL DEFAULT '', text TEXT NOT NULL, owner TEXT, citations TEXT NOT NULL DEFAULT '[]', position INTEGER NOT NULL DEFAULT 0);",
    )?;
    Ok(())
}

pub fn insert_meeting_note(
    store: &MeetingsStore,
    meeting_id: i64,
    section: &str,
    text: &str,
    speaker_label: &str,
) -> Result<i64> {
    let conn = store.connection()?;
    ensure_notes_table(&conn)?;
    conn.execute(
        "INSERT INTO meeting_notes (meeting_id, kind, text, owner, citations, position) VALUES (?1, ?2, ?3, ?4, '[]', 0)",
        params![meeting_id, section, text, speaker_label],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn apply_speaker_spans_after_stop(
    store: &MeetingsStore,
    meeting_id: i64,
    spans: &[SpeakerSpan],
) -> Result<()> {
    let conn = store.connection()?;
    let mut stmt = conn.prepare(
        "SELECT id, start_ms, end_ms, channel FROM meeting_segments WHERE meeting_id = ?1 ORDER BY start_ms, id",
    )?;
    let rows: Vec<(i64, i64, i64, String)> = stmt
        .query_map(params![meeting_id], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })?
        .collect::<std::result::Result<_, _>>()?;
    for (seg_id, s_ms, e_ms, ch) in rows {
        let label = if is_me_channel(&ch) {
            "Me".to_string()
        } else {
            let mut ov_map: BTreeMap<&str, i64> = BTreeMap::new();
            for sp in spans {
                let ov = (e_ms.min(sp.end_ms) - s_ms.max(sp.start_ms)).max(0);
                if ov > 0 {
                    *ov_map.entry(sp.speaker_label.as_str()).or_insert(0) += ov;
                }
            }
            ov_map
                .into_iter()
                .max_by_key(|&(_, ov)| ov)
                .map(|(k, _)| k.to_string())
                .or_else(|| spans.first().map(|s| s.speaker_label.clone()))
                .unwrap_or_else(|| "Speaker 1".to_string())
        };
        conn.execute(
            "UPDATE meeting_segments SET speaker_label = ?1 WHERE id = ?2",
            params![label, seg_id],
        )?;
    }
    ensure_notes_table(&conn)?;
    conn.execute(
        "UPDATE meeting_notes SET owner = 'Me' WHERE meeting_id = ?1 AND LOWER(COALESCE(owner, '')) IN ('me', 'mic')",
        params![meeting_id],
    )?;

    // Fill "Others" action owners from the first cited segment's resolved speaker slot (I6).
    let mut seg_speaker: BTreeMap<i64, String> = BTreeMap::new();
    {
        let mut s_stmt =
            conn.prepare("SELECT id, speaker_label FROM meeting_segments WHERE meeting_id = ?1")?;
        for row in s_stmt.query_map(params![meeting_id], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
        })? {
            let (sid, lbl) = row?;
            if !lbl.is_empty() {
                seg_speaker.insert(sid, lbl);
            }
        }
    }
    let mut n_stmt = conn.prepare(
        "SELECT id, citations FROM meeting_notes
         WHERE meeting_id = ?1
           AND LOWER(kind) IN ('action', 'actions')
           AND LOWER(COALESCE(owner, '')) IN ('others', 'other', '')",
    )?;
    let pending_notes: Vec<(i64, String)> = n_stmt
        .query_map(params![meeting_id], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<std::result::Result<_, _>>()?;
    for (note_id, citations_json) in pending_notes {
        let cites: Vec<i64> = serde_json::from_str(&citations_json).unwrap_or_default();
        if let Some(resolved) = cites
            .iter()
            .find_map(|cid| seg_speaker.get(cid))
            .filter(|lbl| lbl.starts_with("Speaker "))
        {
            conn.execute(
                "UPDATE meeting_notes SET owner = ?1 WHERE id = ?2",
                params![resolved, note_id],
            )?;
        }
    }
    Ok(())
}

fn dedup_clean_names(names: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for raw in names {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        if !out.iter().any(|n| n.eq_ignore_ascii_case(trimmed)) {
            out.push(trimmed.to_string());
        }
    }
    out
}

/// Format the participants line under a meeting's title (`I7` / `AC-X23`):
/// - When calendar attendees exist (stored with EventKit's current user first):
///   `"<first> with <rest>"` (e.g. `"Sam with Dana, Raj, Mei"`), or the single name if only one.
/// - Else when typed speaker names exist: `"Me with <names>"` (e.g. `"Me with Jordan"`).
/// - Else: `"Me and Others"`.
pub fn format_with_line(attendees: &[String], typed_speakers: &[String]) -> String {
    let clean_attendees = dedup_clean_names(attendees);
    if !clean_attendees.is_empty() {
        if clean_attendees.len() == 1 {
            return clean_attendees[0].clone();
        }
        return format!(
            "{} with {}",
            clean_attendees[0],
            clean_attendees[1..].join(", ")
        );
    }
    let clean_typed = dedup_clean_names(typed_speakers);
    if !clean_typed.is_empty() {
        return format!("Me with {}", clean_typed.join(", "));
    }
    "Me and Others".to_string()
}

pub fn typed_speaker_names(store: &MeetingsStore, meeting_id: i64) -> Result<Vec<String>> {
    let slots = speaker_slots(store, meeting_id)?;
    Ok(slots
        .into_iter()
        .map(|s| s.custom_name.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect())
}

/// Run speaker diarization on `system.wav` only, verifying both pinned model SHAs,
/// enforcing the 10-minute wall-clock cap, and guaranteeing zero voiceprint files (`AC-X22`, `AC-X41`).
#[cfg(feature = "speaker-split")]
pub fn run_speaker_split_with_cap(
    app_data_dir: &Path,
    meeting_id: i64,
    seg_model_path: &Path,
    emb_model_path: &Path,
) -> Result<Vec<SpeakerSpan>> {
    use crate::managers::model::ModelManager;
    use std::sync::mpsc;
    use std::time::Duration;

    let others_wav = app_data_dir
        .join("meetings")
        .join(meeting_id.to_string())
        .join("system.wav");
    if !others_wav.exists() {
        return Ok(Vec::new());
    }
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

    let before = collect_dir_files(app_data_dir)?;
    let dir_owned = app_data_dir.to_path_buf();
    let seg_owned = seg_model_path.to_path_buf();
    let emb_owned = emb_model_path.to_path_buf();
    let wav_owned = others_wav.clone();
    let (tx, rx) = mpsc::channel();

    std::thread::spawn(move || {
        let res = (|| -> Result<Vec<SpeakerSpan>> {
            let store = MeetingsStore::open(&dir_owned)?;
            let spans =
                diarize_meeting_after_stop(&store, meeting_id, &wav_owned, &seg_owned, &emb_owned)?;
            verify_no_voiceprint_persisted(&dir_owned, &store, &before)?;
            Ok(spans)
        })();
        let _ = tx.send(res);
    });

    rx.recv_timeout(Duration::from_secs(SPEAKER_SPLIT_TIMEOUT_SECS))
        .map_err(|_| anyhow!("speaker split exceeded {SPEAKER_SPLIT_TIMEOUT_SECS}s cap"))?
}

pub fn diarize_meeting_after_stop(
    store: &MeetingsStore,
    meeting_id: i64,
    others_wav_path: &Path,
    seg_model_path: &Path,
    emb_model_path: &Path,
) -> Result<Vec<SpeakerSpan>> {
    let spans = diarize_others_samples(
        &read_wav_samples(others_wav_path)?,
        seg_model_path,
        emb_model_path,
    )?;
    apply_speaker_spans_after_stop(store, meeting_id, &spans)?;
    Ok(spans)
}

pub fn rename_meeting_speaker(
    store: &MeetingsStore,
    meeting_id: i64,
    speaker_label: &str,
    custom_name: &str,
) -> Result<()> {
    let slot = speaker_label.trim();
    if !slot.starts_with("Speaker ") {
        bail!("only per-meeting Speaker slots can be renamed, got {slot:?}");
    }
    let (clean, conn) = (custom_name.trim(), store.connection()?);
    if clean.is_empty() {
        conn.execute(
            "DELETE FROM meeting_speaker_names WHERE meeting_id = ?1 AND speaker_label = ?2",
            params![meeting_id, slot],
        )?;
    } else {
        conn.execute(
            "INSERT INTO meeting_speaker_names (meeting_id, speaker_label, custom_name) VALUES (?1, ?2, ?3) ON CONFLICT(meeting_id, speaker_label) DO UPDATE SET custom_name = excluded.custom_name",
            params![meeting_id, slot, clean],
        )?;
    }
    Ok(())
}

fn speaker_name_map(store: &MeetingsStore, meeting_id: i64) -> Result<BTreeMap<String, String>> {
    let conn = store.connection()?;
    let mut stmt = conn.prepare(
        "SELECT speaker_label, custom_name FROM meeting_speaker_names WHERE meeting_id = ?1",
    )?;
    let mut map = BTreeMap::new();
    for row in stmt.query_map(params![meeting_id], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
    })? {
        let (k, v) = row?;
        if !v.trim().is_empty() {
            map.insert(k, v);
        }
    }
    Ok(map)
}

pub fn speaker_slots(store: &MeetingsStore, meeting_id: i64) -> Result<Vec<MeetingSpeakerSlot>> {
    let (map, conn) = (speaker_name_map(store, meeting_id)?, store.connection()?);
    let mut slots: BTreeSet<String> = map.keys().cloned().collect();
    let mut stmt = conn.prepare(
        "SELECT DISTINCT speaker_label FROM meeting_segments WHERE meeting_id = ?1 AND speaker_label LIKE 'Speaker %' ORDER BY speaker_label",
    )?;
    for row in stmt.query_map(params![meeting_id], |r| r.get::<_, String>(0))? {
        let slot = row?;
        if slot.starts_with("Speaker ") {
            slots.insert(slot);
        }
    }
    Ok(slots
        .into_iter()
        .map(|slot| {
            let custom_name = map.get(&slot).cloned().unwrap_or_default();
            MeetingSpeakerSlot { slot, custom_name }
        })
        .collect())
}

#[cfg(feature = "speaker-split")]
#[tauri::command]
#[specta::specta]
pub fn list_meeting_speakers(
    app: tauri::AppHandle,
    meeting_id: i64,
) -> std::result::Result<Vec<MeetingSpeakerSlot>, String> {
    let dir = crate::portable::app_data_dir(&app).map_err(|e| e.to_string())?;
    let store = MeetingsStore::open(&dir).map_err(|e| e.to_string())?;
    speaker_slots(&store, meeting_id).map_err(|e| e.to_string())
}

#[cfg(feature = "speaker-split")]
#[tauri::command]
#[specta::specta]
pub fn rename_meeting_speaker_slot(
    app: tauri::AppHandle,
    meeting_id: i64,
    speaker_label: String,
    custom_name: String,
) -> std::result::Result<Vec<MeetingSpeakerSlot>, String> {
    let dir = crate::portable::app_data_dir(&app).map_err(|e| e.to_string())?;
    let store = MeetingsStore::open(&dir).map_err(|e| e.to_string())?;
    rename_meeting_speaker(&store, meeting_id, &speaker_label, &custom_name)
        .map_err(|e| e.to_string())?;
    speaker_slots(&store, meeting_id).map_err(|e| e.to_string())
}

pub fn meeting_segments(store: &MeetingsStore, meeting_id: i64) -> Result<Vec<MeetingSegmentRow>> {
    let (map, conn) = (speaker_name_map(store, meeting_id)?, store.connection()?);
    let mut stmt = conn.prepare(
        "SELECT id, start_ms, end_ms, channel, speaker_label, text FROM meeting_segments WHERE meeting_id = ?1 ORDER BY start_ms, id",
    )?;
    let rows = stmt
        .query_map(params![meeting_id], |r| {
            let (ch, raw_slot): (String, String) = (r.get(3)?, r.get(4)?);
            let slot = if is_me_channel(&ch) {
                "Me".into()
            } else if raw_slot.is_empty() {
                "Others".into()
            } else {
                raw_slot
            };
            let speaker = if is_me_channel(&slot) {
                "Me".into()
            } else {
                map.get(&slot).cloned().unwrap_or(slot)
            };
            Ok(MeetingSegmentRow {
                id: r.get(0)?,
                start_ms: r.get(1)?,
                end_ms: r.get(2)?,
                channel: ch,
                speaker,
                text: r.get(5)?,
            })
        })?
        .collect::<std::result::Result<_, _>>()?;
    Ok(rows)
}

pub fn meeting_notes(store: &MeetingsStore, meeting_id: i64) -> Result<Vec<MeetingNoteRow>> {
    let (map, conn) = (speaker_name_map(store, meeting_id)?, store.connection()?);
    ensure_notes_table(&conn)?;
    let mut stmt = conn.prepare(
        "SELECT id, kind, text, COALESCE(owner, '') FROM meeting_notes WHERE meeting_id = ?1 ORDER BY position, id",
    )?;
    let rows = stmt
        .query_map(params![meeting_id], |r| {
            let (mut text, slot): (String, String) = (r.get(2)?, r.get(3)?);
            let speaker = if is_me_channel(&slot) {
                "Me".into()
            } else {
                map.get(&slot).cloned().unwrap_or(slot)
            };
            for (k, v) in &map {
                text = text.replace(k, v);
            }
            Ok(MeetingNoteRow {
                id: r.get(0)?,
                section: r.get(1)?,
                speaker,
                text,
            })
        })?
        .collect::<std::result::Result<_, _>>()?;
    Ok(rows)
}

pub fn export_meeting_markdown(store: &MeetingsStore, meeting_id: i64) -> Result<String> {
    let attendees = store.meeting_attendees(meeting_id).unwrap_or_default();
    let typed = typed_speaker_names(store, meeting_id).unwrap_or_default();
    let with_line = format_with_line(&attendees, &typed);
    let mut md = format!("# {}\n{}\n\n", store.meeting(meeting_id)?.title, with_line);
    let notes = meeting_notes(store, meeting_id)?;
    if !notes.is_empty() {
        md.push_str("## Notes\n\n");
        for n in &notes {
            md.push_str(&format!("- {} ({})\n", n.text, n.speaker));
        }
        md.push('\n');
    }
    md.push_str("## Transcript\n\n");
    for s in &meeting_segments(store, meeting_id)? {
        let secs = (s.start_ms.max(0)) / 1000;
        md.push_str(&format!(
            "[{:02}:{:02}] {}: {}\n",
            secs / 60,
            secs % 60,
            s.speaker,
            s.text
        ));
    }
    Ok(md)
}

pub fn collect_dir_files(root: &Path) -> Result<BTreeSet<PathBuf>> {
    let mut out = BTreeSet::new();
    if root.exists() {
        for entry in fs::read_dir(root)? {
            let entry = entry?;
            let p = entry.path();
            if entry.file_type()?.is_dir() {
                out.extend(collect_dir_files(&p)?);
            } else {
                out.insert(p);
            }
        }
    }
    Ok(out)
}

pub fn verify_no_voiceprint_persisted(
    app_data_dir: &Path,
    store: &MeetingsStore,
    allowed_before: &BTreeSet<PathBuf>,
) -> Result<()> {
    for path in collect_dir_files(app_data_dir)? {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if !allowed_before.contains(&path)
            && !matches!(name, "meetings.db" | "meetings.db-wal" | "meetings.db-shm")
        {
            return Err(anyhow!(
                "unexpected file persisted after diarization: {}",
                path.display()
            ));
        }
    }
    let conn = store.connection()?;
    let mut stmt = conn.prepare("SELECT name, sql FROM sqlite_master WHERE type = 'table'")?;
    let rows: Vec<(String, String)> = stmt
        .query_map([], |r| {
            Ok((
                r.get(0)?,
                r.get::<_, Option<String>>(1)?.unwrap_or_default(),
            ))
        })?
        .collect::<std::result::Result<_, _>>()?;
    for (tname, sql) in rows {
        let lower = sql.to_ascii_lowercase();
        for tok in ["embed", "voiceprint", "centroid", "biometric", "blob"] {
            if lower.contains(tok) {
                bail!("forbidden voiceprint/embedding schema token {tok:?} in {tname}");
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(feature = "speaker-split")]
    use crate::managers::model::{ModelManager, ModelSource};
    use crate::meeting_speakers_engine::{cluster_speaker_embeddings, SPEAKER_CLUSTER_THRESHOLD};
    #[cfg(feature = "speaker-split")]
    use std::collections::HashMap;
    use std::process::Command;

    #[cfg(not(feature = "speaker-split"))]
    #[test]
    fn speaker_split_is_disabled_by_default_in_meetings_builds() {
        assert!(!SPEAKER_SPLIT_ENABLED);
    }

    #[test]
    fn with_line_follows_i7_attendees_typed_speakers_and_fallback() {
        assert_eq!(
            format_with_line(
                &[
                    "Sam".to_string(),
                    "Dana".to_string(),
                    "Raj".to_string(),
                    "Mei".to_string(),
                ],
                &["Jordan".to_string()],
            ),
            "Sam with Dana, Raj, Mei"
        );
        assert_eq!(
            format_with_line(&[], &["Jordan".to_string()]),
            "Me with Jordan"
        );
        assert_eq!(
            format_with_line(&[], &["Jordan".to_string(), "Alex".to_string()]),
            "Me with Jordan, Alex"
        );
        assert_eq!(format_with_line(&[], &[]), "Me and Others");
    }

    #[cfg(feature = "speaker-split")]
    #[test]
    fn speaker_model_entries_match_pinned_sha256_and_refuse_wrong_hash() {
        let mut models = HashMap::new();
        ModelManager::seed_speaker_models(&mut models);
        let seg = models
            .get(ModelManager::PYANNOTE_SEGMENTATION_MODEL_ID)
            .unwrap();
        let emb = models
            .get(ModelManager::WESPEAKER_EMBEDDING_MODEL_ID)
            .unwrap();
        assert!(!seg.is_downloaded && !emb.is_downloaded);
        assert!(!seg.engine_type.is_speech() && !emb.engine_type.is_speech());
        assert!(matches!(
            &seg.source,
            ModelSource::Url { url, sha256: Some(s) }
                if url == ModelManager::PYANNOTE_SEGMENTATION_URL
                    && s == "24615ee884c897d9d2ba09bb4d30da6bb1b15e685065962db5b02e76e4996488"
        ));
        assert_eq!(
            ModelManager::PYANNOTE_SEGMENTATION_ONNX_SHA256,
            "220ad67ca923bef2fa91f2390c786097bf305bceb5e261d4af67b38e938e1079"
        );
        assert!(matches!(
            &emb.source,
            ModelSource::Url { url, sha256: Some(s) }
                if url == ModelManager::WESPEAKER_EMBEDDING_URL
                    && s == "5ef208a9da1453335308a6b6f4e6dfbd7e183a38b604de0a57664f45d257fe94"
        ));
        let tmp = tempfile::tempdir().unwrap();
        let bad = tmp.path().join("corrupt.onnx");
        fs::write(&bad, b"wrong model bytes").unwrap();
        let err = ModelManager::verify_speaker_model_file(
            &bad,
            ModelManager::WESPEAKER_EMBEDDING_SHA256,
            ModelManager::WESPEAKER_EMBEDDING_MODEL_ID,
        )
        .unwrap_err();
        assert!(err.to_string().contains("Download verification failed") && !bad.exists());
        println!(
            "SPEAKER_MODEL_PROOF: opt_in_unbundled=true pyannote_sha256={} wespeaker_sha256={} mismatch_refused_and_deleted=true",
            ModelManager::PYANNOTE_SEGMENTATION_ARCHIVE_SHA256,
            ModelManager::WESPEAKER_EMBEDDING_SHA256
        );
    }

    #[test]
    fn post_stop_splits_others_into_speakers_while_keeping_me_and_persists_rename() {
        assert!(SPEAKER_CLUSTER_THRESHOLD >= 0.34);
        let embs = vec![
            vec![1.0, 0.0, 0.0],
            vec![0.99, 0.14, 0.0],
            vec![0.0, 1.0, 0.0],
            vec![0.0, 0.99, 0.14],
            vec![0.0, 0.0, 1.0],
            vec![0.14, 0.0, 0.99],
        ];
        let (labels, centroids) = cluster_speaker_embeddings(
            &embs,
            &[(0, 0), (1, 0), (0, 1), (1, 1), (2, 0), (3, 0)],
            SPEAKER_CLUSTER_THRESHOLD,
        );
        assert!(
            centroids.len() == 3
                && labels[0] == labels[1]
                && labels[2] == labels[3]
                && labels[4] == labels[5]
        );

        let dir = tempfile::tempdir().unwrap();
        let mid = {
            let store = MeetingsStore::open(dir.path()).unwrap();
            let mid = store
                .start_meeting("Design critique — voice v0.2", "parakeet")
                .unwrap();
            let other = store.start_meeting("Other meeting", "parakeet").unwrap();
            let mdir = dir.path().join("meetings").join(mid.to_string());
            fs::create_dir_all(&mdir).unwrap();
            fs::write(mdir.join("mic.wav"), b"mic").unwrap();
            fs::write(mdir.join("system.wav"), b"sys").unwrap();
            let before = collect_dir_files(dir.path()).unwrap();
            insert_meeting_segment(
                &store,
                mid,
                41_000,
                48_000,
                "Others",
                "Are we keeping the pill?",
            )
            .unwrap();
            insert_meeting_segment(
                &store,
                mid,
                262_000,
                270_000,
                "Others",
                "Only while a take is running.",
            )
            .unwrap();
            insert_meeting_segment(&store, mid, 300_000, 305_000, "Me", "Sounds good to me.")
                .unwrap();
            insert_meeting_segment(&store, other, 10_000, 20_000, "Others", "Unrelated turn.")
                .unwrap();
            apply_speaker_spans_after_stop(
                &store,
                mid,
                &[
                    SpeakerSpan {
                        start_ms: 40_000,
                        end_ms: 49_000,
                        speaker_label: "Speaker 1".into(),
                    },
                    SpeakerSpan {
                        start_ms: 260_000,
                        end_ms: 271_000,
                        speaker_label: "Speaker 2".into(),
                    },
                ],
            )
            .unwrap();
            apply_speaker_spans_after_stop(
                &store,
                other,
                &[SpeakerSpan {
                    start_ms: 9_000,
                    end_ms: 21_000,
                    speaker_label: "Speaker 2".into(),
                }],
            )
            .unwrap();
            insert_meeting_note(
                &store,
                mid,
                "action",
                "Speaker 2 to send the empty-state copy",
                "Speaker 2",
            )
            .unwrap();
            assert!(rename_meeting_speaker(&store, mid, "Me", "Host").is_err());
            rename_meeting_speaker(&store, mid, "Speaker 2", "Dana").unwrap();
            assert_eq!(
                speaker_slots(&store, mid).unwrap(),
                vec![
                    MeetingSpeakerSlot {
                        slot: "Speaker 1".into(),
                        custom_name: String::new(),
                    },
                    MeetingSpeakerSlot {
                        slot: "Speaker 2".into(),
                        custom_name: "Dana".into(),
                    },
                ]
            );
            assert_eq!(
                meeting_segments(&store, other).unwrap()[0].speaker,
                "Speaker 2"
            );
            assert_eq!(
                format_with_line(&[], &typed_speaker_names(&store, mid).unwrap()),
                "Me with Dana"
            );
            assert_eq!(
                format_with_line(&[], &typed_speaker_names(&store, other).unwrap()),
                "Me and Others"
            );
            verify_no_voiceprint_persisted(dir.path(), &store, &before).unwrap();
            let stray = mdir.join("speaker.voiceprint");
            fs::write(&stray, b"vec").unwrap();
            assert!(verify_no_voiceprint_persisted(dir.path(), &store, &before).is_err());
            fs::remove_file(&stray).unwrap();
            println!(
                "SPEAKER_RAM_PROOF: embeddings_in_ram_only=true persisted_voiceprint_files=0 sqlite_embedding_blobs=0"
            );
            mid
        };
        let reopened = MeetingsStore::open(dir.path()).unwrap();
        let segs = meeting_segments(&reopened, mid).unwrap();
        assert_eq!(
            (
                segs[0].speaker.as_str(),
                segs[1].speaker.as_str(),
                segs[2].speaker.as_str()
            ),
            ("Speaker 1", "Dana", "Me")
        );
        let notes = meeting_notes(&reopened, mid).unwrap();
        assert_eq!(
            (notes[0].speaker.as_str(), notes[0].text.as_str()),
            ("Dana", "Dana to send the empty-state copy")
        );
        let md = export_meeting_markdown(&reopened, mid).unwrap();
        assert!(
            md.contains("- Dana to send the empty-state copy (Dana)")
                && md.contains("[04:22] Dana: Only while a take is running.")
                && md.contains("[05:00] Me: Sounds good to me.")
        );
        let _ = Command::new("true");
    }
}
