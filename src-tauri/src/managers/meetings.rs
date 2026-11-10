//! Meetings database, separate from dictation `history.db`.
//!
//! File: `{app_data_dir}/meetings/meetings.db`. Its migration list starts at 1.
//! WAL is turned on when the file is created so a later live view can read
//! while rows are written. A public build without the `meetings` feature does
//! not open this file and must not delete it, its `-wal` sidecar, or its WAVs.

#![cfg(feature = "meetings")]

use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use log::info;
use rusqlite::{params, Connection};
use rusqlite_migration::{Migrations, M};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tauri::AppHandle;

use crate::audio_toolkit::audio::AppendWav;

/// Two migrations. `rusqlite_migration` stores them as `user_version` 2.
/// A newer meetings.db is refused (`DatabaseTooFarAhead`); dictation is a
/// different file and is unaffected. The first public meetings binary has to
/// carry every migration already on disk; a later meetings migration takes the
/// next number.
static MIGRATIONS: &[M] = &[
    M::up(
        "CREATE TABLE meetings (
        id INTEGER PRIMARY KEY,
        started_at INTEGER NOT NULL,
        ended_at INTEGER,
        title TEXT NOT NULL,
        status TEXT NOT NULL,
        model_id TEXT NOT NULL,
        saved INTEGER NOT NULL DEFAULT 0
    );
    CREATE TABLE meeting_segments (
        id INTEGER PRIMARY KEY,
        meeting_id INTEGER NOT NULL,
        start_ms INTEGER NOT NULL,
        end_ms INTEGER NOT NULL,
        channel TEXT NOT NULL,
        text TEXT NOT NULL
    );",
    ),
    M::up(
        "CREATE TABLE meeting_notes (
            id INTEGER PRIMARY KEY,
            meeting_id INTEGER NOT NULL,
            kind TEXT NOT NULL,
            text TEXT NOT NULL,
            owner TEXT,
            citations TEXT NOT NULL,
            position INTEGER NOT NULL
        );
        CREATE INDEX meeting_notes_meeting ON meeting_notes(meeting_id, position);
        CREATE TABLE meeting_notepad (
            id INTEGER PRIMARY KEY,
            meeting_id INTEGER NOT NULL,
            at_ms INTEGER NOT NULL,
            text TEXT NOT NULL
        );
        CREATE INDEX meeting_notepad_meeting ON meeting_notepad(meeting_id, at_ms);",
    ),
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Meeting {
    pub id: i64,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub title: String,
    pub status: String,
    pub model_id: String,
    pub saved: i64,
}

/// One cited note line of a meeting.
///
/// `kind` is the note kind, `text` the line itself, `owner` an optional
/// person the note is attributed to, and `citations` the segment ids the
/// line is grounded in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredNote {
    pub kind: String,
    pub text: String,
    pub owner: Option<String>,
    pub citations: Vec<i64>,
}

/// Map one `meetings` row; shared by every `Meeting` query.
fn meeting_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Meeting> {
    Ok(Meeting {
        id: row.get(0)?,
        started_at: row.get(1)?,
        ended_at: row.get(2)?,
        title: row.get(3)?,
        status: row.get(4)?,
        model_id: row.get(5)?,
        saved: row.get(6)?,
    })
}

/// Escape `%`, `_` and `\` so the query is a plain substring, not a pattern.
fn like_pattern(query: &str) -> String {
    let mut out = String::with_capacity(query.len() + 2);
    out.push('%');
    for c in query.chars() {
        if c == '%' || c == '_' || c == '\\' {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('%');
    out
}

pub struct MeetingsStore {
    db_path: PathBuf,
}

impl MeetingsStore {
    /// Open `{app_data_dir}/meetings/meetings.db`, migrate, enable WAL, and
    /// recover a row left `recording` with a null `ended_at`.
    pub fn open(app_data_dir: &Path) -> Result<Self> {
        let meetings_dir = app_data_dir.join("meetings");
        fs::create_dir_all(&meetings_dir)?;
        let store = Self {
            db_path: meetings_dir.join("meetings.db"),
        };
        store.init()?;
        Ok(store)
    }

    /// Same as [`open`](Self::open), with the portable-aware app data dir.
    /// The meeting controller calls this; this PR only builds the store.
    #[allow(dead_code)]
    pub fn open_app(app: &AppHandle) -> Result<Self> {
        Self::open(&crate::portable::app_data_dir(app)?)
    }

    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    pub fn start_meeting(&self, title: &str, model_id: &str) -> Result<i64> {
        let conn = self.connection()?;
        let started_at = Utc::now().timestamp();
        conn.execute(
            "INSERT INTO meetings (started_at, title, status, model_id)
             VALUES (?1, ?2, 'recording', ?3)",
            params![started_at, title, model_id],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn meeting(&self, id: i64) -> Result<Meeting> {
        let conn = self.connection()?;
        conn.query_row(
            "SELECT id, started_at, ended_at, title, status, model_id, saved
             FROM meetings WHERE id = ?1",
            params![id],
            meeting_from_row,
        )
        .map_err(|e| anyhow!("meeting {id}: {e}"))
    }

    /// Every meeting, newest first.
    pub fn list_meetings(&self) -> Result<Vec<Meeting>> {
        let conn = self.connection()?;
        let mut stmt = conn.prepare(
            "SELECT id, started_at, ended_at, title, status, model_id, saved
             FROM meetings ORDER BY started_at DESC, id DESC",
        )?;
        let rows = stmt.query_map([], meeting_from_row)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// Meetings with `query` in the title, any segment, or any note text or owner; blank lists all.
    pub fn search_meetings(&self, query: &str) -> Result<Vec<Meeting>> {
        let trimmed = query.trim();
        if trimmed.is_empty() {
            return self.list_meetings();
        }
        let conn = self.connection()?;
        let mut stmt = conn.prepare(
            "SELECT m.id, m.started_at, m.ended_at, m.title, m.status, m.model_id, m.saved
             FROM meetings m
             WHERE m.title LIKE ?1 ESCAPE '\\'
                OR EXISTS (SELECT 1 FROM meeting_segments s
                           WHERE s.meeting_id = m.id AND s.text LIKE ?1 ESCAPE '\\')
                OR EXISTS (SELECT 1 FROM meeting_notes n
                           WHERE n.meeting_id = m.id
                             AND (n.text LIKE ?1 ESCAPE '\\'
                                  OR n.owner LIKE ?1 ESCAPE '\\'))
             ORDER BY m.started_at DESC, m.id DESC",
        )?;
        let rows = stmt.query_map(params![like_pattern(trimmed)], meeting_from_row)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// Replace every note of `meeting_id` with `notes`, in one transaction.
    ///
    /// `position` is the index of the note in `notes`. `citations` is stored
    /// as a JSON array of segment ids. On error nothing is written.
    pub fn save_notes(&self, meeting_id: i64, notes: &[StoredNote]) -> Result<()> {
        let mut conn = self.connection()?;
        let tx = conn.transaction()?;
        tx.execute(
            "DELETE FROM meeting_notes WHERE meeting_id = ?1",
            params![meeting_id],
        )?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO meeting_notes
                 (meeting_id, kind, text, owner, citations, position)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )?;
            for (position, note) in notes.iter().enumerate() {
                let citations = serde_json::to_string(&note.citations)?;
                stmt.execute(params![
                    meeting_id,
                    &note.kind,
                    &note.text,
                    note.owner.as_deref(),
                    citations,
                    position as i64
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Every note of `meeting_id`, ordered by `position` ascending.
    pub fn notes(&self, meeting_id: i64) -> Result<Vec<StoredNote>> {
        let conn = self.connection()?;
        let mut stmt = conn.prepare(
            "SELECT kind, text, owner, citations FROM meeting_notes
             WHERE meeting_id = ?1 ORDER BY position",
        )?;
        let rows = stmt.query_map(params![meeting_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (kind, text, owner, citations) = row?;
            let citations = serde_json::from_str(&citations)
                .with_context(|| format!("meeting {meeting_id}: invalid citations JSON"))?;
            out.push(StoredNote {
                kind,
                text,
                owner,
                citations,
            });
        }
        Ok(out)
    }

    /// Append one notepad point at `at_ms` and return the new row id.
    pub fn add_notepad_point(&self, meeting_id: i64, at_ms: i64, text: &str) -> Result<i64> {
        let conn = self.connection()?;
        conn.execute(
            "INSERT INTO meeting_notepad (meeting_id, at_ms, text) VALUES (?1, ?2, ?3)",
            params![meeting_id, at_ms, text],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// Notepad points of `meeting_id` as `(at_ms, text)`, ordered by `at_ms`
    /// ascending and then by row id ascending.
    pub fn notepad(&self, meeting_id: i64) -> Result<Vec<(i64, String)>> {
        let conn = self.connection()?;
        let mut stmt = conn.prepare(
            "SELECT at_ms, text FROM meeting_notepad
             WHERE meeting_id = ?1 ORDER BY at_ms, id",
        )?;
        let rows = stmt.query_map(params![meeting_id], |row| Ok((row.get(0)?, row.get(1)?)))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// Append PCM. On a write error the writer is stopped, this meeting is
    /// finalised with what was already written, and the error is returned.
    pub fn append_pcm(&self, meeting_id: i64, wav: &mut AppendWav, samples: &[i16]) -> Result<()> {
        if let Err(e) = wav.append(samples) {
            if let Err(fin) = self.finalise_recording(meeting_id) {
                return Err(anyhow!("{e:#}; failed to finalise the meeting: {fin:#}"));
            }
            return Err(e);
        }
        Ok(())
    }

    fn init(&self) -> Result<()> {
        let mut conn = Connection::open(&self.db_path)?;
        conn.busy_timeout(Duration::from_secs(1))?;
        let migrations = Migrations::new(MIGRATIONS.to_vec());
        #[cfg(debug_assertions)]
        migrations.validate().expect("invalid meetings migrations");
        migrations.to_latest(&mut conn)?;
        // WAL is persistent in the file. Set it on create (and on every open,
        // which is a no-op once it is wal). history.db is a different file.
        let mode: String =
            conn.pragma_update_and_check(None, "journal_mode", "WAL", |row| row.get(0))?;
        if !mode.eq_ignore_ascii_case("wal") {
            anyhow::bail!("meetings.db journal mode is {mode}, expected wal");
        }
        self.recover_unfinished(&conn)?;
        Ok(())
    }

    fn connection(&self) -> Result<Connection> {
        let conn = Connection::open(&self.db_path)?;
        conn.busy_timeout(Duration::from_secs(1))?;
        Ok(conn)
    }

    /// A crash leaves the row `recording` with `ended_at` null. Keep the files.
    /// Do not invent the missing audio tail.
    fn recover_unfinished(&self, conn: &Connection) -> Result<()> {
        let now = Utc::now().timestamp();
        let n = conn.execute(
            "UPDATE meetings
             SET ended_at = ?1, status = 'transcript_ready'
             WHERE status = 'recording' AND ended_at IS NULL",
            params![now],
        )?;
        if n > 0 {
            info!("recovered {n} unfinished meeting row(s); audio files kept");
        }
        Ok(())
    }

    fn finalise_recording(&self, meeting_id: i64) -> Result<()> {
        let conn = self.connection()?;
        let now = Utc::now().timestamp();
        conn.execute(
            "UPDATE meetings
             SET ended_at = ?1, status = 'transcript_ready'
             WHERE id = ?2 AND status = 'recording' AND ended_at IS NULL",
            params![now, meeting_id],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio_toolkit::read_wav_samples;
    use rusqlite::Connection;
    use rusqlite_migration::{Migrations, M};
    use std::fs::File;
    use std::io::{Seek, SeekFrom, Write};

    fn pcm(n: usize) -> Vec<i16> {
        (0..n).map(|i| ((i * 17) % 2001) as i16 - 1000).collect()
    }

    fn assert_roundtrip(got: &[f32], samples: &[i16]) {
        assert_eq!(got.len(), samples.len());
        for (got, sample) in got.iter().zip(samples) {
            let expect = *sample as f32 / i16::MAX as f32;
            assert!((got - expect).abs() < 1e-6, "{got} vs {expect}");
        }
    }

    #[test]
    fn append_wav_readable_after_drop() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mic.wav");
        let samples = pcm(16_000 * 3);
        {
            let mut wav = AppendWav::create(&path).unwrap();
            wav.append(&samples).unwrap();
            wav.flush().unwrap();
            // Drop without a hound-style finalize. The flush is the durable point.
        }
        let got = read_wav_samples(&path).unwrap();
        assert_roundtrip(&got, &samples);
    }

    #[test]
    fn torn_header_is_repaired_on_open() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mic.wav");
        let samples = pcm(1234);
        {
            let mut wav = AppendWav::create(&path).unwrap();
            wav.append(&samples).unwrap();
            wav.flush().unwrap();
        }
        // Crash after the PCM hit disk and before the size patch: sizes say empty.
        {
            let mut file = File::options().write(true).open(&path).unwrap();
            file.seek(SeekFrom::Start(4)).unwrap();
            file.write_all(&36u32.to_le_bytes()).unwrap();
            file.seek(SeekFrom::Start(40)).unwrap();
            file.write_all(&0u32.to_le_bytes()).unwrap();
        }
        let stale = read_wav_samples(&path).unwrap();
        assert!(stale.is_empty(), "stale header must not expose the tail");

        {
            let _wav = AppendWav::open(&path).unwrap();
        }
        let got = read_wav_samples(&path).unwrap();
        assert_roundtrip(&got, &samples);

        let bytes = fs::read(&path).unwrap();
        let data_size = u32::from_le_bytes(bytes[40..44].try_into().unwrap());
        let riff_size = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
        assert_eq!(data_size as usize, samples.len() * 2);
        assert_eq!(riff_size, 36 + data_size);
        assert_eq!(bytes.len() as u64, 44 + u64::from(data_size));
    }

    /// `M::up("...")` string literals from history.rs. HistoryManager::new needs
    /// an AppHandle, and history.rs stays untouched, so the test applies that
    /// same SQL through rusqlite_migration.
    fn history_migration_sql(src: &'static str) -> Vec<&'static str> {
        let mut out = Vec::new();
        let mut rest = src;
        while let Some(i) = rest.find("M::up(") {
            let trimmed = rest[i + "M::up(".len()..].trim_start();
            assert!(
                trimmed.starts_with('"'),
                "history migration is not a string literal"
            );
            let body = &trimmed[1..];
            let bytes = body.as_bytes();
            let mut end = 0;
            while end < bytes.len() {
                if bytes[end] == b'\\' {
                    end += 2;
                    continue;
                }
                if bytes[end] == b'"' {
                    break;
                }
                end += 1;
            }
            out.push(&body[..end]);
            rest = &body[end..];
        }
        out
    }

    #[test]
    fn history_db_version_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let history_path = dir.path().join("history.db");
        {
            let mut conn = Connection::open(&history_path).unwrap();
            let sqls = history_migration_sql(include_str!("history.rs"));
            assert_eq!(sqls.len(), 4, "history.rs migration list changed");
            assert!(sqls[0].contains("transcription_history"));
            let migrations = Migrations::new(sqls.into_iter().map(M::up).collect());
            migrations.to_latest(&mut conn).unwrap();
            let version: i32 = conn
                .pragma_query_value(None, "user_version", |row| row.get(0))
                .unwrap();
            assert_eq!(version, 4);
        }
        let before = fs::read(&history_path).unwrap();

        let store = MeetingsStore::open(dir.path()).unwrap();
        assert_eq!(
            store.db_path(),
            dir.path().join("meetings").join("meetings.db")
        );

        assert_eq!(fs::read(&history_path).unwrap(), before);
        let history = Connection::open(&history_path).unwrap();
        let version: i32 = history
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, 4);
        let meetings_in_history: i64 = history
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'meetings'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(meetings_in_history, 0);

        let meetings = Connection::open(store.db_path()).unwrap();
        let meetings_version: i32 = meetings
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(meetings_version, 2);
        let mode: String = meetings
            .pragma_query_value(None, "journal_mode", |row| row.get(0))
            .unwrap();
        assert_eq!(mode.to_ascii_lowercase(), "wal");
    }

    #[test]
    fn write_error_finalises_not_succeeds() {
        let dir = tempfile::tempdir().unwrap();
        let store = MeetingsStore::open(dir.path()).unwrap();
        let id = store.start_meeting("Standup", "model").unwrap();
        let path = dir.path().join("meetings").join("mic.wav");
        {
            let mut wav = AppendWav::create(&path).unwrap();
            wav.flush().unwrap();
        }
        // Read-only fd: the header is already consistent, so open does not
        // write, and the next append fails.
        let mut wav = AppendWav::from_file(File::open(&path).unwrap()).unwrap();
        let err = store.append_pcm(id, &mut wav, &[1, -2, 3, 4]);
        assert!(err.is_err(), "write error reported as success: {err:?}");
        assert!(wav.is_stopped());
        let row = store.meeting(id).unwrap();
        assert_eq!(row.status, "transcript_ready");
        assert!(row.ended_at.is_some());
        assert!(path.exists());

        let err2 = store.append_pcm(id, &mut wav, &[5]);
        let message = err2
            .expect_err("stopped writer reported success")
            .to_string();
        assert!(
            message.contains("stopped"),
            "later append was not refused: {message}"
        );
        let row = store.meeting(id).unwrap();
        assert_eq!(row.status, "transcript_ready");
    }

    #[test]
    fn unfinished_meeting_row_recovers_on_open() {
        let dir = tempfile::tempdir().unwrap();
        let id = {
            let store = MeetingsStore::open(dir.path()).unwrap();
            let id = store.start_meeting("Standup", "model").unwrap();
            let row = store.meeting(id).unwrap();
            assert_eq!(row.status, "recording");
            assert!(row.ended_at.is_none());
            let meeting_dir = dir.path().join("meetings").join(id.to_string());
            fs::create_dir_all(&meeting_dir).unwrap();
            fs::write(meeting_dir.join("mic.wav"), b"mic-audio").unwrap();
            fs::write(meeting_dir.join("system.wav"), b"system-audio").unwrap();
            id
        };

        let store = MeetingsStore::open(dir.path()).unwrap();
        let row = store.meeting(id).unwrap();
        assert_eq!(row.status, "transcript_ready");
        assert!(row.ended_at.is_some());
        assert_eq!(row.title, "Standup");
        assert_eq!(row.model_id, "model");
        assert_eq!(row.saved, 0);
        let meeting_dir = dir.path().join("meetings").join(id.to_string());
        assert_eq!(fs::read(meeting_dir.join("mic.wav")).unwrap(), b"mic-audio");
        assert_eq!(
            fs::read(meeting_dir.join("system.wav")).unwrap(),
            b"system-audio"
        );

        let ended = row.ended_at;
        drop(store);
        let store = MeetingsStore::open(dir.path()).unwrap();
        let again = store.meeting(id).unwrap();
        assert_eq!(again.status, "transcript_ready");
        assert_eq!(again.ended_at, ended);
    }

    fn note(kind: &str, text: &str, owner: Option<&str>, citations: Vec<i64>) -> StoredNote {
        StoredNote {
            kind: kind.to_string(),
            text: text.to_string(),
            owner: owner.map(str::to_string),
            citations,
        }
    }

    #[test]
    fn notes_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let store = MeetingsStore::open(dir.path()).unwrap();
        let id = store.start_meeting("Planning", "model").unwrap();
        let first = note("decision", "Ship on Friday", Some("Priya"), vec![1, 3]);
        let second = note("action", "Book the room", None, vec![2]);
        store
            .save_notes(id, &[first.clone(), second.clone()])
            .unwrap();
        assert_eq!(store.notes(id).unwrap(), vec![first, second]);
    }

    #[test]
    fn save_notes_replaces_and_is_per_meeting() {
        let dir = tempfile::tempdir().unwrap();
        let store = MeetingsStore::open(dir.path()).unwrap();
        let a = store.start_meeting("A", "model").unwrap();
        let b = store.start_meeting("B", "model").unwrap();
        let a1 = note("decision", "A one", None, vec![1]);
        let a2 = note("action", "A two", None, vec![2]);
        let b1 = note("decision", "B one", None, vec![3]);
        store.save_notes(a, &[a1, a2]).unwrap();
        store.save_notes(b, &[b1.clone()]).unwrap();

        let replacement = note("risk", "A only", Some("Sam"), vec![4]);
        store.save_notes(a, &[replacement.clone()]).unwrap();
        assert_eq!(store.notes(a).unwrap(), vec![replacement]);
        assert_eq!(store.notes(b).unwrap(), vec![b1]);
    }

    #[test]
    fn notepad_points_ordered() {
        let dir = tempfile::tempdir().unwrap();
        let store = MeetingsStore::open(dir.path()).unwrap();
        let id = store.start_meeting("Standup", "model").unwrap();
        store.add_notepad_point(id, 5000, "b").unwrap();
        store.add_notepad_point(id, 1000, "a").unwrap();
        store.add_notepad_point(id, 5000, "c").unwrap();
        assert_eq!(
            store.notepad(id).unwrap(),
            vec![
                (1000, "a".to_string()),
                (5000, "b".to_string()),
                (5000, "c".to_string()),
            ]
        );
    }

    #[test]
    fn version_1_db_upgrades_to_2() {
        let dir = tempfile::tempdir().unwrap();
        let meetings_dir = dir.path().join("meetings");
        fs::create_dir_all(&meetings_dir).unwrap();
        let path = meetings_dir.join("meetings.db");
        {
            let mut conn = Connection::open(&path).unwrap();
            let migrations = Migrations::new(vec![MIGRATIONS[0].clone()]);
            migrations.to_latest(&mut conn).unwrap();
            conn.execute(
                "INSERT INTO meetings
                 (id, started_at, ended_at, title, status, model_id, saved)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    1i64,
                    1_700_000_000i64,
                    1_700_000_100i64,
                    "Old",
                    "transcript_ready",
                    "m",
                    0
                ],
            )
            .unwrap();
        }

        let store = MeetingsStore::open(dir.path()).unwrap();
        let conn = Connection::open(store.db_path()).unwrap();
        let version: i32 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, 2);
        let row = store.meeting(1).unwrap();
        assert_eq!(row.title, "Old");
    }

    #[test]
    fn newer_db_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let meetings_dir = dir.path().join("meetings");
        fs::create_dir_all(&meetings_dir).unwrap();
        let path = meetings_dir.join("meetings.db");
        {
            let conn = Connection::open(&path).unwrap();
            conn.pragma_update(None, "user_version", 3).unwrap();
        }

        let result = MeetingsStore::open(dir.path());
        assert!(result.is_err(), "a newer meetings.db was accepted");
    }

    fn segment(store: &MeetingsStore, meeting_id: i64, text: &str) {
        store
            .connection()
            .unwrap()
            .execute(
                "INSERT INTO meeting_segments (meeting_id, start_ms, end_ms, channel, text)
                 VALUES (?1, 0, 0, 'mic', ?2)",
                params![meeting_id, text],
            )
            .unwrap();
    }

    fn ids(meetings: &[Meeting]) -> Vec<i64> {
        meetings.iter().map(|m| m.id).collect()
    }

    #[test]
    fn search_matches_transcript_segment() {
        let dir = tempfile::tempdir().unwrap();
        let store = MeetingsStore::open(dir.path()).unwrap();
        let a = store.start_meeting("Standup", "model").unwrap();
        let b = store.start_meeting("Planning", "model").unwrap();
        segment(&store, a, "a zebra crossed the car park");
        segment(&store, b, "nothing unusual happened");
        assert_eq!(ids(&store.search_meetings("zebra").unwrap()), vec![a]);
    }

    #[test]
    fn search_matches_action_item_note() {
        let dir = tempfile::tempdir().unwrap();
        let store = MeetingsStore::open(dir.path()).unwrap();
        let _ = store.start_meeting("Standup", "model").unwrap();
        let b = store.start_meeting("Planning", "model").unwrap();
        store
            .save_notes(b, &[note("action", "Email the zebra report", None, vec![])])
            .unwrap();
        assert_eq!(ids(&store.search_meetings("zebra").unwrap()), vec![b]);
    }

    #[test]
    fn blank_query_lists_all_newest_first() {
        let dir = tempfile::tempdir().unwrap();
        let store = MeetingsStore::open(dir.path()).unwrap();
        let a = store.start_meeting("Older", "model").unwrap();
        let b = store.start_meeting("Newer", "model").unwrap();
        assert_eq!(ids(&store.list_meetings().unwrap()), vec![b, a]);
        assert_eq!(ids(&store.search_meetings("").unwrap()), vec![b, a]);
        assert_eq!(ids(&store.search_meetings("   ").unwrap()), vec![b, a]);
    }

    #[test]
    fn search_is_case_insensitive() {
        let dir = tempfile::tempdir().unwrap();
        let store = MeetingsStore::open(dir.path()).unwrap();
        let a = store.start_meeting("Standup", "model").unwrap();
        segment(&store, a, "Zebra crossing closed");
        assert_eq!(ids(&store.search_meetings("ZEBRA").unwrap()), vec![a]);
    }

    #[test]
    fn wildcards_are_literal() {
        let dir = tempfile::tempdir().unwrap();
        let store = MeetingsStore::open(dir.path()).unwrap();
        let sale = store.start_meeting("50% off", "model").unwrap();
        let plain = store.start_meeting("5000", "model").unwrap();
        assert_eq!(ids(&store.search_meetings("50%").unwrap()), vec![sale]);
        assert_eq!(ids(&store.search_meetings("5000").unwrap()), vec![plain]);

        let snake = store.start_meeting("snake_case plan", "model").unwrap();
        let camel = store.start_meeting("snakeXcase plan", "model").unwrap();
        assert_eq!(
            ids(&store.search_meetings("snake_case").unwrap()),
            vec![snake]
        );
        assert_eq!(
            ids(&store.search_meetings("snakeXcase").unwrap()),
            vec![camel]
        );
    }

    #[test]
    fn meeting_matching_twice_appears_once() {
        let dir = tempfile::tempdir().unwrap();
        let store = MeetingsStore::open(dir.path()).unwrap();
        let first = store.start_meeting("Zebra review", "model").unwrap();
        let second = store.start_meeting("Standup", "model").unwrap();
        segment(&store, first, "the zebra is back");
        segment(&store, first, "another zebra line");
        segment(&store, second, "zebra again");
        assert_eq!(
            ids(&store.search_meetings("zebra").unwrap()),
            vec![second, first]
        );
    }
}
