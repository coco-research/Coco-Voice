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
    conn.execute("INSERT INTO meeting_segments (meeting_id, start_ms, end_ms, channel, speaker_label, text) VALUES (?1, ?2, ?3, ?4, ?5, ?6)", params![meeting_id, start_ms, end_ms, channel, slot, text])?;
    Ok(conn.last_insert_rowid())
}

fn ensure_notes_table(conn: &rusqlite::Connection) -> Result<()> {
    conn.execute_batch("CREATE TABLE IF NOT EXISTS meeting_notes (id INTEGER PRIMARY KEY, meeting_id INTEGER NOT NULL, kind TEXT NOT NULL DEFAULT '', text TEXT NOT NULL, owner TEXT, citations TEXT NOT NULL DEFAULT '[]', position INTEGER NOT NULL DEFAULT 0);")?;
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
    conn.execute("INSERT INTO meeting_notes (meeting_id, kind, text, owner, citations, position) VALUES (?1, ?2, ?3, ?4, '[]', 0)", params![meeting_id, section, text, speaker_label])?;
    Ok(conn.last_insert_rowid())
}

pub fn apply_speaker_spans_after_stop(
    store: &MeetingsStore,
    meeting_id: i64,
    spans: &[SpeakerSpan],
) -> Result<()> {
    let conn = store.connection()?;
    let mut stmt = conn.prepare("SELECT id, start_ms, end_ms, channel FROM meeting_segments WHERE meeting_id = ?1 ORDER BY start_ms, id")?;
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
    conn.execute("UPDATE meeting_notes SET owner = 'Me' WHERE meeting_id = ?1 AND LOWER(COALESCE(owner, '')) IN ('me', 'mic')", params![meeting_id])?;
    Ok(())
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
        conn.execute("INSERT INTO meeting_speaker_names (meeting_id, speaker_label, custom_name) VALUES (?1, ?2, ?3) ON CONFLICT(meeting_id, speaker_label) DO UPDATE SET custom_name = excluded.custom_name", params![meeting_id, slot, clean])?;
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

pub fn meeting_segments(store: &MeetingsStore, meeting_id: i64) -> Result<Vec<MeetingSegmentRow>> {
    let (map, conn) = (speaker_name_map(store, meeting_id)?, store.connection()?);
    let mut stmt = conn.prepare("SELECT id, start_ms, end_ms, channel, speaker_label, text FROM meeting_segments WHERE meeting_id = ?1 ORDER BY start_ms, id")?;
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
    let mut stmt = conn.prepare("SELECT id, kind, text, COALESCE(owner, '') FROM meeting_notes WHERE meeting_id = ?1 ORDER BY position, id")?;
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
    let mut md = format!("# {}\n\n", store.meeting(meeting_id)?.title);
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
