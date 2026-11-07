//! Meetings database, separate from dictation `history.db`.
//!
//! File: `{app_data_dir}/meetings/meetings.db`. Its migration list starts at 1.
//! WAL is turned on when the file is created so a later live view can read
//! while rows are written. A public build without the `meetings` feature does
//! not open this file and must not delete it, its `-wal` sidecar, or its WAVs.

#![cfg(feature = "meetings")]

use anyhow::{anyhow, Result};
use chrono::Utc;
use log::info;
use rusqlite::{params, Connection};
use rusqlite_migration::{Migrations, M};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tauri::AppHandle;

/// One meeting at a time. Mute and the meeting controller read this.
static MEETING_ACTIVE: AtomicBool = AtomicBool::new(false);

/// Whether a meeting is recording. Dictation mute checks this and returns.
pub fn meeting_is_active() -> bool {
    MEETING_ACTIVE.load(Ordering::Acquire)
}

pub fn set_meeting_active(active: bool) {
    MEETING_ACTIVE.store(active, Ordering::Release);
}

#[cfg(test)]
pub(crate) fn test_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|err| err.into_inner())
}

use crate::audio_toolkit::audio::AppendWav;

/// One migration. `rusqlite_migration` stores it as `user_version` 1.
/// A newer meetings.db is refused (`DatabaseTooFarAhead`); dictation is a
/// different file and is unaffected. The first public meetings binary has to
/// carry every migration already on disk.
static MIGRATIONS: &[M] = &[M::up(
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
)];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MeetingSegment {
    pub id: i64,
    pub meeting_id: i64,
    pub start_ms: i64,
    pub end_ms: i64,
    pub channel: String,
    pub text: String,
}

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

    pub fn meeting_count(&self) -> Result<i64> {
        let conn = self.connection()?;
        Ok(conn.query_row("SELECT COUNT(*) FROM meetings", [], |row| row.get(0))?)
    }

    /// One finished chunk, one transaction. Times are milliseconds from the
    /// meeting start. `channel` is `me` or `others`.
    pub fn insert_segment(
        &self,
        meeting_id: i64,
        start_ms: i64,
        end_ms: i64,
        channel: &str,
        text: &str,
    ) -> Result<i64> {
        let mut conn = self.connection()?;
        let tx = conn.transaction()?;
        tx.execute(
            "INSERT INTO meeting_segments (meeting_id, start_ms, end_ms, channel, text)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![meeting_id, start_ms, end_ms, channel, text],
        )?;
        let id = tx.last_insert_rowid();
        tx.commit()?;
        Ok(id)
    }

    pub fn segments(&self, meeting_id: i64) -> Result<Vec<MeetingSegment>> {
        let conn = self.connection()?;
        let mut stmt = conn.prepare(
            "SELECT id, meeting_id, start_ms, end_ms, channel, text
             FROM meeting_segments
             WHERE meeting_id = ?1
             ORDER BY id",
        )?;
        let rows = stmt.query_map(params![meeting_id], |row| {
            Ok(MeetingSegment {
                id: row.get(0)?,
                meeting_id: row.get(1)?,
                start_ms: row.get(2)?,
                end_ms: row.get(3)?,
                channel: row.get(4)?,
                text: row.get(5)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|err| anyhow!(err))
    }

    pub fn meeting(&self, id: i64) -> Result<Meeting> {
        let conn = self.connection()?;
        conn.query_row(
            "SELECT id, started_at, ended_at, title, status, model_id, saved
             FROM meetings WHERE id = ?1",
            params![id],
            |row| {
                Ok(Meeting {
                    id: row.get(0)?,
                    started_at: row.get(1)?,
                    ended_at: row.get(2)?,
                    title: row.get(3)?,
                    status: row.get(4)?,
                    model_id: row.get(5)?,
                    saved: row.get(6)?,
                })
            },
        )
        .map_err(|e| anyhow!("meeting {id}: {e}"))
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
        assert_eq!(meetings_version, 1);
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
}
