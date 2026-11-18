//! Meeting history operations: soft-trash and restore, single-span deletion
//! (segments, WAV range, and citing note lines), optional transcript retention
//! timer, single-transcript search, and concurrent older-meeting reads.

#![cfg(feature = "meetings")]

use std::collections::HashSet;
use std::fs::OpenOptions;
use std::io::{Seek, SeekFrom, Write};
use std::path::Path;
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use rusqlite::{params, Connection};
use serde::Serialize;

use crate::audio_toolkit::audio::AppendWav;
pub use crate::managers::meetings::{Meeting, MeetingSegment, MeetingsStore, StoredNote};

pub const TRASH_RETENTION_DAYS: i64 = 30;
const SECS_PER_DAY: i64 = 86_400;
const WAV_HEADER_BYTES: u64 = 44;
const SAMPLES_PER_MS: u64 = 16;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrashedMeeting {
    pub meeting: Meeting,
    pub trashed_at: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct SpanDeleteOutcome {
    pub removed_segments: usize,
    pub removed_notes: usize,
    pub wiped_audio_files: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MeetingDetail {
    pub meeting: Meeting,
    pub notes: Vec<StoredNote>,
    pub segments: Vec<MeetingSegment>,
    pub notepad: Vec<(i64, String)>,
}

fn open_conn(store: &MeetingsStore) -> Result<Connection> {
    let conn = Connection::open(store.db_path())?;
    conn.busy_timeout(Duration::from_secs(1))?;
    Ok(conn)
}

fn like_pattern(query: &str) -> String {
    let mut out = String::with_capacity(query.len() + 2);
    out.push('%');
    for ch in query.chars() {
        if matches!(ch, '%' | '_' | '\\') {
            out.push('\\');
        }
        out.push(ch);
    }
    out.push('%');
    out
}

/// Move `id` to soft trash at `now_secs`, hiding it from normal list and search.
pub fn trash_meeting_at(store: &MeetingsStore, id: i64, now_secs: i64) -> Result<()> {
    if store.meeting(id)?.status == "recording" {
        bail!("cannot trash an actively recording meeting");
    }
    open_conn(store)?.execute(
        "INSERT INTO meeting_trash (meeting_id, trashed_at)
         VALUES (?1, ?2)
         ON CONFLICT(meeting_id) DO UPDATE SET trashed_at = excluded.trashed_at",
        params![id, now_secs],
    )?;
    Ok(())
}

/// Restore `id` from soft trash back into active meeting history.
pub fn restore_meeting(store: &MeetingsStore, id: i64) -> Result<()> {
    let removed = open_conn(store)?.execute(
        "DELETE FROM meeting_trash WHERE meeting_id = ?1",
        params![id],
    )?;
    if removed == 0 {
        bail!("meeting {id} is not in trash");
    }
    Ok(())
}

/// List meetings currently in soft trash, newest `trashed_at` first.
pub fn list_trashed_meetings(store: &MeetingsStore) -> Result<Vec<TrashedMeeting>> {
    let conn = open_conn(store)?;
    let mut stmt = conn.prepare(
        "SELECT m.id, m.started_at, m.ended_at, m.title, m.status, m.model_id, m.saved, t.trashed_at
         FROM meeting_trash t JOIN meetings m ON m.id = t.meeting_id
         ORDER BY t.trashed_at DESC, m.id DESC",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(TrashedMeeting {
            meeting: Meeting {
                id: r.get(0)?,
                started_at: r.get(1)?,
                ended_at: r.get(2)?,
                title: r.get(3)?,
                status: r.get(4)?,
                model_id: r.get(5)?,
                saved: r.get(6)?,
            },
            trashed_at: r.get(7)?,
        })
    })?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|err| anyhow!(err))
}

/// Permanently delete trashed meetings kept for at least 30 days.
pub fn purge_expired_trash(store: &MeetingsStore, now_secs: i64) -> Result<usize> {
    let cutoff = now_secs - TRASH_RETENTION_DAYS * SECS_PER_DAY;
    let expired: Vec<i64> = {
        let conn = open_conn(store)?;
        let mut stmt =
            conn.prepare("SELECT meeting_id FROM meeting_trash WHERE trashed_at <= ?1")?;
        let rows = stmt.query_map(params![cutoff], |r| r.get(0))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()?
    };
    for id in &expired {
        store.delete_meeting(*id)?;
    }
    Ok(expired.len())
}

fn wipe_wav_span(path: &Path, start_ms: i64, end_ms: i64) -> Result<bool> {
    if !path.exists() {
        return Ok(false);
    }
    AppendWav::open(path)?.flush()?;
    let mut file = OpenOptions::new().read(true).write(true).open(path)?;
    let file_len = file.metadata()?.len();
    let start_byte = WAV_HEADER_BYTES + (start_ms.max(0) as u64) * SAMPLES_PER_MS * 2;
    let end_byte = (WAV_HEADER_BYTES + (end_ms.max(0) as u64) * SAMPLES_PER_MS * 2).min(file_len);
    if file_len <= WAV_HEADER_BYTES || start_byte >= end_byte {
        return Ok(false);
    }
    file.seek(SeekFrom::Start(start_byte))?;
    let mut remaining = (end_byte - start_byte) as usize;
    let zeros = [0u8; 8192];
    while remaining > 0 {
        let step = remaining.min(zeros.len());
        file.write_all(&zeros[..step])?;
        remaining -= step;
    }
    file.sync_all()?;
    Ok(true)
}

/// Delete overlapping transcript segments in `[start_ms, end_ms)`, wipe their
/// WAV audio range, and remove any note lines citing deleted segments.
pub fn delete_transcript_span(
    store: &MeetingsStore,
    meeting_id: i64,
    start_ms: i64,
    end_ms: i64,
) -> Result<SpanDeleteOutcome> {
    if start_ms < 0 || end_ms <= start_ms {
        bail!("invalid transcript span [{start_ms}, {end_ms})");
    }
    if store.meeting(meeting_id)?.status == "recording" {
        bail!("cannot delete a span from an actively recording meeting");
    }
    let mut conn = open_conn(store)?;
    let tx = conn.transaction()?;
    let deleted_ids: HashSet<i64> = {
        let mut stmt = tx.prepare(
            "SELECT id FROM meeting_segments WHERE meeting_id = ?1 AND end_ms > ?2 AND start_ms < ?3",
        )?;
        let rows = stmt.query_map(params![meeting_id, start_ms, end_ms], |r| r.get(0))?;
        rows.collect::<std::result::Result<HashSet<_>, _>>()?
    };
    let removed_segments = tx.execute(
        "DELETE FROM meeting_segments WHERE meeting_id = ?1 AND end_ms > ?2 AND start_ms < ?3",
        params![meeting_id, start_ms, end_ms],
    )?;

    let mut removed_notes = 0usize;
    if !deleted_ids.is_empty() {
        let mut kept: Vec<(String, String, Option<String>, String)> = Vec::new();
        {
            let mut stmt = tx.prepare(
                "SELECT kind, text, owner, citations FROM meeting_notes WHERE meeting_id = ?1 ORDER BY position",
            )?;
            let rows = stmt.query_map(params![meeting_id], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get::<_, String>(3)?))
            })?;
            for row in rows {
                let (kind, text, owner, raw_cites) = row?;
                let cites: Vec<i64> = serde_json::from_str(&raw_cites)
                    .with_context(|| format!("meeting {meeting_id}: invalid citations JSON"))?;
                if cites.iter().any(|cid| deleted_ids.contains(cid)) {
                    removed_notes += 1;
                } else {
                    kept.push((kind, text, owner, raw_cites));
                }
            }
        }
        tx.execute(
            "DELETE FROM meeting_notes WHERE meeting_id = ?1",
            params![meeting_id],
        )?;
        for (pos, (kind, text, owner, cites)) in kept.iter().enumerate() {
            tx.execute(
                "INSERT INTO meeting_notes (meeting_id, kind, text, owner, citations, position)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![meeting_id, kind, text, owner.as_deref(), cites, pos as i64],
            )?;
        }
    }
    tx.commit()?;

    let mut wiped_audio_files = 0usize;
    if let Some(meetings_dir) = store.db_path().parent() {
        let dir = meetings_dir.join(meeting_id.to_string());
        for name in ["mic.wav", "system.wav"] {
            if wipe_wav_span(&dir.join(name), start_ms, end_ms)? {
                wiped_audio_files += 1;
            }
        }
    }
    Ok(SpanDeleteOutcome {
        removed_segments,
        removed_notes,
        wiped_audio_files,
    })
}

/// Expire transcripts older than `delete_after_days` while keeping meeting rows and notes.
pub fn apply_transcript_retention(
    store: &MeetingsStore,
    now_secs: i64,
    delete_after_days: Option<u32>,
) -> Result<usize> {
    let Some(days) = delete_after_days.filter(|d| *d > 0) else {
        return Ok(0);
    };
    let cutoff = now_secs - (days as i64) * SECS_PER_DAY;
    let mut conn = open_conn(store)?;
    let tx = conn.transaction()?;
    let ids: Vec<i64> = {
        let mut stmt = tx.prepare(
            "SELECT DISTINCT m.id FROM meetings m
             JOIN meeting_segments s ON s.meeting_id = m.id
             WHERE m.status != 'recording' AND COALESCE(m.ended_at, m.started_at) <= ?1",
        )?;
        let rows = stmt.query_map(params![cutoff], |r| r.get(0))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()?
    };
    for id in &ids {
        tx.execute(
            "DELETE FROM meeting_segments WHERE meeting_id = ?1",
            params![id],
        )?;
        tx.execute(
            "UPDATE meeting_notes SET citations = '[]' WHERE meeting_id = ?1",
            params![id],
        )?;
    }
    tx.commit()?;
    Ok(ids.len())
}

/// Escaped substring search scoped strictly to a single meeting's transcript.
pub fn search_meeting_transcript(
    store: &MeetingsStore,
    meeting_id: i64,
    query: &str,
) -> Result<Vec<MeetingSegment>> {
    store.meeting(meeting_id)?;
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }
    let conn = open_conn(store)?;
    let mut stmt = conn.prepare(
        "SELECT id, meeting_id, start_ms, end_ms, channel, text
         FROM meeting_segments WHERE meeting_id = ?1 AND text LIKE ?2 ESCAPE '\\'
         ORDER BY start_ms, id",
    )?;
    let rows = stmt.query_map(params![meeting_id, like_pattern(trimmed)], |r| {
        Ok(MeetingSegment {
            id: r.get(0)?,
            meeting_id: r.get(1)?,
            start_ms: r.get(2)?,
            end_ms: r.get(3)?,
            channel: r.get(4)?,
            text: r.get(5)?,
        })
    })?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|err| anyhow!(err))
}

/// Read an existing meeting's notes, transcript, and notepad without disturbing
/// any active recording session.
pub fn read_meeting_detail(store: &MeetingsStore, meeting_id: i64) -> Result<MeetingDetail> {
    Ok(MeetingDetail {
        meeting: store.meeting(meeting_id)?,
        notes: store.notes(meeting_id)?,
        segments: store.segments(meeting_id)?,
        notepad: store.notepad(meeting_id)?,
    })
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::audio_toolkit::read_wav_samples;
    use crate::managers::meetings::{meeting_is_active, set_meeting_active, test_lock};

    fn seed_finalised_meeting(root: &Path, title: &str, ended_at: i64) -> (MeetingsStore, i64) {
        let store = MeetingsStore::open(root).unwrap();
        let id = store.start_meeting(title, "turbo").unwrap();
        open_conn(&store)
            .unwrap()
            .execute(
                "UPDATE meetings SET started_at = ?1, ended_at = ?2, status = 'transcript_ready' WHERE id = ?3",
                params![ended_at - 600, ended_at, id],
            )
            .unwrap();
        (store, id)
    }

    #[test]
    fn trash_hides_and_restore_brings_back_and_purges_after_30_days() {
        let dir = tempfile::tempdir().unwrap();
        let now = 1_800_000_000;
        let (store, id_a) = seed_finalised_meeting(dir.path(), "Monday standup", now);
        let (_, id_b) = seed_finalised_meeting(dir.path(), "Product sync", now + 10);

        trash_meeting_at(&store, id_a, now).unwrap();
        let visible: Vec<i64> = store
            .list_meetings()
            .unwrap()
            .into_iter()
            .map(|m| m.id)
            .collect();
        assert_eq!(visible, vec![id_b]);
        assert!(store.search_meetings("Monday").unwrap().is_empty());
        assert_eq!(list_trashed_meetings(&store).unwrap().len(), 1);

        restore_meeting(&store, id_a).unwrap();
        assert_eq!(store.list_meetings().unwrap().len(), 2);
        assert_eq!(store.search_meetings("Monday").unwrap().len(), 1);

        trash_meeting_at(&store, id_a, now).unwrap();
        assert_eq!(
            purge_expired_trash(&store, now + 29 * SECS_PER_DAY).unwrap(),
            0
        );
        assert_eq!(
            purge_expired_trash(&store, now + 30 * SECS_PER_DAY).unwrap(),
            1
        );
        assert!(store.meeting(id_a).is_err());
    }

    #[test]
    fn delete_span_removes_segments_wipes_wav_range_and_leaves_zero_dangling_citations() {
        let dir = tempfile::tempdir().unwrap();
        let (store, id) = seed_finalised_meeting(dir.path(), "Retro", 1_800_000_000);
        let s1 = store
            .insert_segment(id, 0, 1_000, "me", "Keep intro")
            .unwrap();
        let s2 = store
            .insert_segment(id, 1_000, 2_000, "others", "Private span")
            .unwrap();
        let s3 = store
            .insert_segment(id, 2_000, 3_000, "me", "Keep wrap-up")
            .unwrap();
        store
            .save_notes(
                id,
                &[
                    StoredNote {
                        kind: "summary".to_string(),
                        text: "Intro summary".to_string(),
                        owner: None,
                        citations: vec![s1],
                    },
                    StoredNote {
                        kind: "decision".to_string(),
                        text: "Cited private span".to_string(),
                        owner: None,
                        citations: vec![s2],
                    },
                    StoredNote {
                        kind: "action".to_string(),
                        text: "Wrap-up action".to_string(),
                        owner: Some("Me".to_string()),
                        citations: vec![s3],
                    },
                ],
            )
            .unwrap();

        let audio_dir = dir.path().join("meetings").join(id.to_string());
        fs::create_dir_all(&audio_dir).unwrap();
        let mic_path = audio_dir.join("mic.wav");
        let mut wav = AppendWav::create(&mic_path).unwrap();
        wav.append(&vec![8_000i16; 3 * 16_000]).unwrap();
        wav.flush().unwrap();
        drop(wav);

        let out = delete_transcript_span(&store, id, 1_000, 2_000).unwrap();
        assert_eq!(out.removed_segments, 1);
        assert_eq!(out.removed_notes, 1);
        assert_eq!(out.wiped_audio_files, 1);

        let seg_ids: Vec<i64> = store
            .segments(id)
            .unwrap()
            .into_iter()
            .map(|s| s.id)
            .collect();
        assert_eq!(seg_ids, vec![s1, s3]);
        for note in store.notes(id).unwrap() {
            assert!(
                note.citations.iter().all(|cid| seg_ids.contains(cid)),
                "dangling citation in {note:?}"
            );
        }

        let samples = read_wav_samples(&mic_path).unwrap();
        assert!(samples[..16_000].iter().all(|s| s.abs() > 0.1));
        assert!(samples[16_000..32_000].iter().all(|s| *s == 0.0));
        assert!(samples[32_000..48_000].iter().all(|s| s.abs() > 0.1));
    }

    #[test]
    fn transcript_timer_defaults_off_has_no_training_switch_and_expires_old_transcripts() {
        let defaults = crate::settings::get_default_settings();
        assert_eq!(defaults.transcript_delete_after_days, None);
        let raw = serde_json::to_value(&defaults).unwrap();
        let obj = raw.as_object().unwrap();
        assert!(
            obj.keys()
                .all(|k| !k.to_ascii_lowercase().contains("train")),
            "unexpected training key in settings: {:?}",
            obj.keys().collect::<Vec<_>>()
        );

        let dir = tempfile::tempdir().unwrap();
        let now = 1_800_000_000;
        let (store, old_id) =
            seed_finalised_meeting(dir.path(), "Old sync", now - 10 * SECS_PER_DAY);
        let (_, recent_id) =
            seed_finalised_meeting(dir.path(), "Recent sync", now - 2 * SECS_PER_DAY);
        let s_old = store
            .insert_segment(old_id, 0, 1_000, "me", "Old words")
            .unwrap();
        store
            .insert_segment(recent_id, 0, 1_000, "me", "New words")
            .unwrap();
        store
            .save_notes(
                old_id,
                &[StoredNote {
                    kind: "summary".to_string(),
                    text: "Kept summary".to_string(),
                    owner: None,
                    citations: vec![s_old],
                }],
            )
            .unwrap();

        assert_eq!(apply_transcript_retention(&store, now, None).unwrap(), 0);
        assert_eq!(store.segments(old_id).unwrap().len(), 1);
        assert_eq!(apply_transcript_retention(&store, now, Some(7)).unwrap(), 1);
        assert!(store.segments(old_id).unwrap().is_empty());
        assert_eq!(store.notes(old_id).unwrap().len(), 1);
        assert!(store.notes(old_id).unwrap()[0].citations.is_empty());
        assert_eq!(store.segments(recent_id).unwrap().len(), 1);
    }

    #[test]
    fn search_one_transcript_scopes_to_single_meeting_and_escapes_wildcards() {
        let dir = tempfile::tempdir().unwrap();
        let (store, id_a) = seed_finalised_meeting(dir.path(), "A", 1_800_000_000);
        let (_, id_b) = seed_finalised_meeting(dir.path(), "B", 1_800_000_100);
        store
            .insert_segment(id_a, 0, 1_000, "me", "100% ready for launch")
            .unwrap();
        store
            .insert_segment(id_a, 1_000, 2_000, "others", "1000 items queued")
            .unwrap();
        store
            .insert_segment(id_b, 0, 1_000, "me", "100% in another meeting")
            .unwrap();

        let hits = search_meeting_transcript(&store, id_a, "100%").unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].meeting_id, id_a);
        assert!(hits[0].text.contains("100% ready"));
    }

    #[test]
    fn reading_older_meeting_during_active_recording_leaves_sample_count_rising() {
        let _guard = test_lock();
        let dir = tempfile::tempdir().unwrap();
        let (store, older_id) =
            seed_finalised_meeting(dir.path(), "Design critique", 1_800_000_000);
        let seg = store
            .insert_segment(older_id, 0, 1_000, "others", "Dana owns export")
            .unwrap();
        store
            .save_notes(
                older_id,
                &[StoredNote {
                    kind: "action".to_string(),
                    text: "Dana owns the export flow".to_string(),
                    owner: Some("Dana".to_string()),
                    citations: vec![seg],
                }],
            )
            .unwrap();

        let active_id = store.start_meeting("Live standup", "turbo").unwrap();
        set_meeting_active(true);
        let active_dir = dir.path().join("meetings").join(active_id.to_string());
        fs::create_dir_all(&active_dir).unwrap();
        let mic_wav = active_dir.join("mic.wav");
        let mut wav = AppendWav::create(&mic_wav).unwrap();
        let frame = vec![4_000i16; 16_000];
        store.append_pcm(active_id, &mut wav, &frame).unwrap();
        wav.flush().unwrap();
        let count_before = read_wav_samples(&mic_wav).unwrap().len();

        let older = read_meeting_detail(&store, older_id).unwrap();
        assert_eq!(older.meeting.title, "Design critique");
        assert_eq!(older.notes[0].text, "Dana owns the export flow");
        assert!(meeting_is_active());

        store.append_pcm(active_id, &mut wav, &frame).unwrap();
        wav.flush().unwrap();
        set_meeting_active(false);
        let count_after = read_wav_samples(&mic_wav).unwrap().len();
        assert!(
            count_after > count_before,
            "{count_after} <= {count_before}"
        );
    }
}
