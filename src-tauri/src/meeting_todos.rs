//! Cross-meeting spoken to-dos extracted from the user's meeting track.
//!
//! Parses first-person spoken commitments and completion phrases from the
//! user's audio channel, formats transcript moment citations, and keeps
//! `{app_data_dir}/meetings/todos.db` synchronized with `meetings.db`.

#![cfg(feature = "meetings")]

use anyhow::{anyhow, Result};
use rusqlite::{params, Connection, OptionalExtension};
use rusqlite_migration::{Migrations, M};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tauri::AppHandle;

static MIGRATIONS: &[M] = &[M::up(
    "CREATE TABLE meeting_todos (
        id INTEGER PRIMARY KEY, dedup_key TEXT NOT NULL UNIQUE, text TEXT NOT NULL,
        status TEXT NOT NULL DEFAULT 'open', origin_meeting_id INTEGER NOT NULL,
        origin_meeting_title TEXT NOT NULL, origin_started_at INTEGER NOT NULL,
        origin_segment_id INTEGER NOT NULL, origin_start_ms INTEGER NOT NULL,
        origin_end_ms INTEGER NOT NULL, completed_meeting_id INTEGER,
        completed_segment_id INTEGER, completed_start_ms INTEGER
    );",
)];

const COMMITMENT_PREFIXES: &[&str] = &[
    "remind me to ",
    "my action item is to ",
    "action item: ",
    "to-do: ",
    "todo: ",
    "i'm going to ",
    "i am going to ",
    "i need to ",
    "i have to ",
    "i will ",
    "i'll ",
    "let me ",
];

const COMPLETION_PREFIXES: &[&str] = &[
    "i already ",
    "i have ",
    "i've ",
    "i sent ",
    "i wrote ",
    "i booked ",
    "i listed ",
    "i finished ",
    "i completed ",
    "i rotated ",
    "i updated ",
    "i shared ",
    "i reviewed ",
    "completed: ",
    "done: ",
];

const STOPWORDS: &str = "a|an|the|to|for|with|on|in|at|by|of|and|or|my|our|this|that|it|up|out|today|tomorrow|week|monday|tuesday|wednesday|thursday|friday";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum TodoStatus {
    Open,
    Done,
}

impl TodoStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Done => "done",
        }
    }

    pub fn from_db(raw: &str) -> Self {
        if raw.eq_ignore_ascii_case("done") {
            Self::Done
        } else {
            Self::Open
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct TodoMoment {
    pub meeting_id: i64,
    pub meeting_title: String,
    pub meeting_started_at: i64,
    pub segment_id: i64,
    pub start_ms: i64,
    pub end_ms: i64,
    pub timestamp: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct MeetingTodo {
    pub id: i64,
    pub text: String,
    pub status: TodoStatus,
    pub origin: TodoMoment,
    #[serde(default)]
    pub said_by_me: bool,
    pub completed_meeting_id: Option<i64>,
    pub completed_segment_id: Option<i64>,
    pub completed_start_ms: Option<i64>,
}

/// Default lookback window for "Your to-dos" (14 days, AC-X37).
pub const TODOS_LOOKBACK_DAYS: i64 = 14;

pub fn format_timestamp_ms(ms: i64) -> String {
    let total_secs = (ms.max(0)) / 1000;
    let hours = total_secs / 3600;
    let mins = (total_secs % 3600) / 60;
    let secs = total_secs % 60;
    if hours > 0 {
        format!("{hours:02}:{mins:02}:{secs:02}")
    } else {
        format!("{mins:02}:{secs:02}")
    }
}

pub fn is_user_channel(channel: &str) -> bool {
    let c = channel.trim();
    c.eq_ignore_ascii_case("me") || c.eq_ignore_ascii_case("mic")
}

fn content_tokens(text: &str) -> Vec<String> {
    let stops: HashSet<&str> = STOPWORDS.split('|').collect();
    text.split(|c: char| !c.is_alphanumeric())
        .map(|w| w.to_ascii_lowercase())
        .filter(|w| w.len() >= 2 && !stops.contains(w.as_str()))
        .collect()
}

pub fn extract_commitments(text: &str) -> Vec<(String, String)> {
    let mut found = Vec::new();
    for raw_clause in text.split(['.', '!', '?', ';', '\n']) {
        let mut clause = raw_clause.trim();
        for lead in [
            "okay,", "okay ", "alright,", "alright ", "yes,", "yeah,", "sure,", "so ",
        ] {
            if clause.len() > lead.len() && clause[..lead.len()].eq_ignore_ascii_case(lead) {
                clause = clause[lead.len()..].trim_start();
            }
        }
        let lower = clause.to_ascii_lowercase();
        if lower.starts_with("i'll be ")
            && !["i'll be sending ", "i'll be writing ", "i'll be updating "]
                .iter()
                .any(|p| lower.starts_with(p))
        {
            continue;
        }
        let Some(prefix) = COMMITMENT_PREFIXES.iter().find(|p| lower.starts_with(**p)) else {
            continue;
        };
        let body = clause[prefix.len()..]
            .trim()
            .trim_end_matches([',', ':', ';', '.'])
            .trim();
        if body.split_whitespace().count() < 2 || body.chars().count() > 160 {
            continue;
        }
        let tokens = content_tokens(body);
        if tokens.is_empty() {
            continue;
        }
        let mut chars = body.chars();
        let display = match chars.next() {
            Some(first) => format!("{}{}", first.to_uppercase(), chars.as_str()),
            None => continue,
        };
        found.push((tokens.join(" "), display));
    }
    found
}

pub fn extract_completion_token_sets(text: &str) -> Vec<HashSet<String>> {
    let mut sets = Vec::new();
    for raw_clause in text.split(['.', '!', '?', ';', '\n']) {
        let clause = raw_clause.trim();
        let lower = clause.to_ascii_lowercase();
        if COMPLETION_PREFIXES.iter().any(|p| lower.starts_with(p)) {
            let toks: HashSet<String> = content_tokens(clause).into_iter().collect();
            if toks.len() >= 2 {
                sets.push(toks);
            }
        }
    }
    sets
}

pub fn completion_matches(dedup_key: &str, completion_tokens: &HashSet<String>) -> bool {
    let mut iter = dedup_key.split_whitespace();
    let _verb = iter.next();
    let targets: Vec<&str> = iter.collect();
    targets.len() >= 2 && targets.iter().all(|t| completion_tokens.contains(*t))
}

type SegRow = (i64, String, i64, i64, i64, i64, String, String);

pub struct MeetingTodoStore {
    meetings_db_path: PathBuf,
    todos_db_path: PathBuf,
}

impl MeetingTodoStore {
    pub fn open(app_data_dir: &Path) -> Result<Self> {
        let dir = app_data_dir.join("meetings");
        fs::create_dir_all(&dir)?;
        let store = Self {
            meetings_db_path: dir.join("meetings.db"),
            todos_db_path: dir.join("todos.db"),
        };
        store.init()?;
        Ok(store)
    }

    #[allow(dead_code)]
    pub fn open_app(app: &AppHandle) -> Result<Self> {
        Self::open(&crate::portable::app_data_dir(app)?)
    }

    fn init(&self) -> Result<()> {
        let mut conn = Connection::open(&self.todos_db_path)?;
        conn.busy_timeout(Duration::from_secs(1))?;
        conn.execute_batch("PRAGMA secure_delete = ON;")?;
        let migrations = Migrations::new(MIGRATIONS.to_vec());
        #[cfg(debug_assertions)]
        migrations
            .validate()
            .expect("invalid meeting_todos migrations");
        migrations.to_latest(&mut conn)?;
        let mode: String =
            conn.pragma_update_and_check(None, "journal_mode", "WAL", |r| r.get(0))?;
        if !mode.eq_ignore_ascii_case("wal") {
            anyhow::bail!("todos.db journal mode is {mode}, expected wal");
        }
        Ok(())
    }

    fn todos_conn(&self) -> Result<Connection> {
        let conn = Connection::open(&self.todos_db_path)?;
        conn.busy_timeout(Duration::from_secs(1))?;
        conn.execute_batch("PRAGMA secure_delete = ON;")?;
        Ok(conn)
    }

    /// Securely zero freed pages and truncate the WAL on `todos.db`.
    pub fn compact_secure(&self) -> Result<()> {
        let conn = self.todos_conn()?;
        conn.execute_batch("PRAGMA secure_delete = ON;")?;
        crate::managers::meetings::wal_checkpoint_truncate(&conn)?;
        conn.execute_batch("VACUUM;")?;
        crate::managers::meetings::wal_checkpoint_truncate(&conn)?;
        Ok(())
    }

    fn read_segments(&self) -> Result<Vec<SegRow>> {
        if !self.meetings_db_path.exists() {
            return Ok(Vec::new());
        }
        let conn = Connection::open(&self.meetings_db_path)?;
        conn.busy_timeout(Duration::from_secs(1))?;
        if conn
            .query_row(
                "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'meetings'",
                [],
                |r| r.get::<_, i64>(0),
            )
            .optional()?
            .is_none()
        {
            return Ok(Vec::new());
        }
        let has_trash: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type = 'table' AND name = 'meeting_trash'",
                [],
                |r| r.get(0),
            )
            .unwrap_or(false);
        let trash_filter = if has_trash {
            " AND NOT EXISTS (SELECT 1 FROM meeting_trash t WHERE t.meeting_id = m.id)"
        } else {
            ""
        };
        let mut st = conn.prepare(&format!(
            "SELECT m.id, m.title, m.started_at, s.id, s.start_ms, s.end_ms, s.channel, s.text
             FROM meetings m JOIN meeting_segments s ON s.meeting_id = m.id
             WHERE m.status != 'trashed'{trash_filter}
             ORDER BY m.started_at ASC, m.id ASC, s.start_ms ASC, s.id ASC"
        ))?;
        let rows = st
            .query_map([], |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                    r.get(7)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    fn read_me_action_notes(&self) -> Result<Vec<(String, String, SegRow)>> {
        if !self.meetings_db_path.exists() {
            return Ok(Vec::new());
        }
        let conn = Connection::open(&self.meetings_db_path)?;
        conn.busy_timeout(Duration::from_secs(1))?;
        let has_notes: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type = 'table' AND name = 'meeting_notes'",
                [],
                |r| r.get(0),
            )
            .unwrap_or(false);
        if !has_notes {
            return Ok(Vec::new());
        }
        let has_trash: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type = 'table' AND name = 'meeting_trash'",
                [],
                |r| r.get(0),
            )
            .unwrap_or(false);
        let trash_filter = if has_trash {
            " AND NOT EXISTS (SELECT 1 FROM meeting_trash t WHERE t.meeting_id = m.id)"
        } else {
            ""
        };
        let mut st = conn.prepare(&format!(
            "SELECT m.id, m.title, m.started_at, n.text, n.citations
             FROM meetings m JOIN meeting_notes n ON n.meeting_id = m.id
             WHERE m.status != 'trashed'{trash_filter}
               AND LOWER(n.kind) IN ('action', 'actions')
               AND LOWER(COALESCE(n.owner, '')) IN ('me', 'mic')
             ORDER BY m.started_at ASC, m.id ASC, n.position ASC"
        ))?;
        let rows: Vec<(i64, String, i64, String, String)> = st
            .query_map([], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut out = Vec::new();
        for (mid, title, started_at, text, raw_cites) in rows {
            let clean = text.trim();
            if clean.is_empty() {
                continue;
            }
            let cids = serde_json::from_str::<Vec<i64>>(&raw_cites).unwrap_or_default();
            let first_cid = cids.first().copied().unwrap_or(0);
            let seg_info: Option<(i64, i64, i64, String, String)> = if first_cid > 0 {
                conn.query_row(
                    "SELECT id, start_ms, end_ms, channel, text FROM meeting_segments WHERE id = ?1 AND meeting_id = ?2",
                    params![first_cid, mid],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
                )
                .optional()?
            } else {
                None
            };
            let (seg_id, start_ms, end_ms, ch, seg_text) =
                seg_info.unwrap_or((first_cid, 0, 0, "me".to_string(), clean.to_string()));
            let tokens = content_tokens(clean);
            if tokens.is_empty() {
                continue;
            }
            let key = tokens.join(" ");
            out.push((
                key,
                clean.to_string(),
                (
                    mid, title, started_at, seg_id, start_ms, end_ms, ch, seg_text,
                ),
            ));
        }
        Ok(out)
    }

    pub fn sync(&self) -> Result<Vec<MeetingTodo>> {
        let segments = self.read_segments()?;
        let action_notes = self.read_me_action_notes()?;
        let (mut earliest, mut completions) = (HashMap::new(), HashMap::new());
        let mut ordered_keys: Vec<String> = Vec::new();
        let mut meetings_with_action_notes: HashSet<i64> = HashSet::new();

        for (key, display, origin_seg) in action_notes {
            meetings_with_action_notes.insert(origin_seg.0);
            let k_copy = key.clone();
            earliest.entry(key).or_insert_with(|| {
                ordered_keys.push(k_copy);
                (display, origin_seg)
            });
        }

        for seg in &segments {
            if !is_user_channel(&seg.6) {
                continue;
            }
            if !meetings_with_action_notes.contains(&seg.0) {
                for (key, display) in extract_commitments(&seg.7) {
                    let k_copy = key.clone();
                    earliest.entry(key).or_insert_with(|| {
                        ordered_keys.push(k_copy);
                        (display, seg.clone())
                    });
                }
            }
            for comp_set in extract_completion_token_sets(&seg.7) {
                for key in &ordered_keys {
                    let (_, o) = &earliest[key];
                    let after = (seg.2, seg.0, seg.4, seg.3) > (o.2, o.0, o.4, o.3);
                    if after && completion_matches(key, &comp_set) {
                        completions
                            .entry(key.clone())
                            .or_insert_with(|| seg.clone());
                    }
                }
            }
        }

        let mut conn = self.todos_conn()?;
        let tx = conn.transaction()?;
        {
            let existing: Vec<String> = tx
                .prepare("SELECT dedup_key FROM meeting_todos")?
                .query_map([], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?;
            for k in existing {
                if !earliest.contains_key(&k) {
                    tx.execute("DELETE FROM meeting_todos WHERE dedup_key = ?1", params![k])?;
                }
            }
            for key in &ordered_keys {
                let (display, o) = &earliest[key];
                tx.execute(
                    "INSERT INTO meeting_todos (dedup_key, text, status, origin_meeting_id, origin_meeting_title, origin_started_at, origin_segment_id, origin_start_ms, origin_end_ms)
                     VALUES (?1, ?2, 'open', ?3, ?4, ?5, ?6, ?7, ?8)
                     ON CONFLICT(dedup_key) DO UPDATE SET origin_meeting_id = excluded.origin_meeting_id, origin_meeting_title = excluded.origin_meeting_title,
                     origin_started_at = excluded.origin_started_at, origin_segment_id = excluded.origin_segment_id, origin_start_ms = excluded.origin_start_ms, origin_end_ms = excluded.origin_end_ms",
                    params![key, display, o.0, o.1, o.2, o.3, o.4, o.5],
                )?;
                if let Some(d) = completions.get(key) {
                    tx.execute(
                        "UPDATE meeting_todos SET status = 'done', completed_meeting_id = ?1, completed_segment_id = ?2, completed_start_ms = ?3 WHERE dedup_key = ?4 AND status = 'open'",
                        params![d.0, d.3, d.4, key],
                    )?;
                }
            }
        }
        tx.commit()?;
        self.list(true, None)
    }

    pub fn list(
        &self,
        include_done: bool,
        since_started_at: Option<i64>,
    ) -> Result<Vec<MeetingTodo>> {
        let conn = self.todos_conn()?;
        let mut st = conn.prepare(
            "SELECT id, text, status, origin_meeting_id, origin_meeting_title,
                    origin_started_at, origin_segment_id, origin_start_ms, origin_end_ms,
                    completed_meeting_id, completed_segment_id, completed_start_ms
             FROM meeting_todos
             WHERE (?1 != 0 OR status = 'open') AND origin_started_at >= ?2
             ORDER BY origin_started_at DESC, origin_start_ms ASC, id ASC",
        )?;
        let out = st
            .query_map(
                params![
                    i32::from(include_done),
                    since_started_at.unwrap_or(i64::MIN)
                ],
                |r| {
                    let start_ms: i64 = r.get(7)?;
                    Ok(MeetingTodo {
                        id: r.get(0)?,
                        text: r.get(1)?,
                        status: TodoStatus::from_db(&r.get::<_, String>(2)?),
                        origin: TodoMoment {
                            meeting_id: r.get(3)?,
                            meeting_title: r.get(4)?,
                            meeting_started_at: r.get(5)?,
                            segment_id: r.get(6)?,
                            start_ms,
                            end_ms: r.get(8)?,
                            timestamp: format_timestamp_ms(start_ms),
                        },
                        said_by_me: true,
                        completed_meeting_id: r.get(9)?,
                        completed_segment_id: r.get(10)?,
                        completed_start_ms: r.get(11)?,
                    })
                },
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(out)
    }

    /// List open to-dos from the last 14 days (`AC-X37`).
    pub fn list_last_14_days(&self, now_secs: i64) -> Result<Vec<MeetingTodo>> {
        let cutoff = now_secs.saturating_sub(TODOS_LOOKBACK_DAYS * 86_400);
        self.list(false, Some(cutoff))
    }

    /// Sync from `meetings.db` and return open to-dos from the last 14 days,
    /// failing closed if the sync cannot finish.
    pub fn sync_then_list_last_14_days(&self, now_secs: i64) -> Result<Vec<MeetingTodo>> {
        self.sync()?;
        self.list_last_14_days(now_secs)
    }

    pub fn set_status(&self, todo_id: i64, status: TodoStatus) -> Result<()> {
        let conn = self.todos_conn()?;
        if conn.execute(
            "UPDATE meeting_todos SET status = ?1 WHERE id = ?2",
            params![status.as_str(), todo_id],
        )? == 0
        {
            return Err(anyhow!("meeting todo {todo_id} not found"));
        }
        Ok(())
    }

    pub fn resolve_moment(&self, moment: &TodoMoment) -> Result<bool> {
        if !self.meetings_db_path.exists() {
            return Ok(false);
        }
        let conn = Connection::open(&self.meetings_db_path)?;
        Ok(conn
            .query_row(
                "SELECT s.id FROM meetings m JOIN meeting_segments s ON s.meeting_id = m.id
                 WHERE m.id = ?1 AND s.id = ?2 AND s.start_ms = ?3 AND m.status != 'trashed'",
                params![moment.meeting_id, moment.segment_id, moment.start_ms],
                |r| r.get::<_, i64>(0),
            )
            .optional()?
            .is_some())
    }
}

// ponytail: ceiling = deterministic local SQLite + spoken cues (0 sockets); upgrade path = on-device LLM pass when precision <90%.
pub fn run_todos_json(app_data_dir: &Path, include_done: bool) -> Result<String> {
    let store = MeetingTodoStore::open(app_data_dir)?;
    let _ = store.sync()?;
    Ok(serde_json::to_string_pretty(
        &store.list(include_done, None)?,
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::managers::meetings::MeetingsStore;

    fn seed(dir: &Path, started: i64, title: &str, segs: &[(i64, i64, &str, &str)]) -> i64 {
        let ms = MeetingsStore::open(dir).unwrap();
        let conn = Connection::open(ms.db_path()).unwrap();
        conn.execute(
            "INSERT INTO meetings (started_at, ended_at, title, status, model_id, saved)
             VALUES (?1, ?2, ?3, 'transcript_ready', 'parakeet', 1)",
            params![started, started + 900, title],
        )
        .unwrap();
        let mid = conn.last_insert_rowid();
        for (s, e, ch, text) in segs {
            conn.execute(
                "INSERT INTO meeting_segments (meeting_id, start_ms, end_ms, channel, text)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![mid, s, e, ch, text],
            )
            .unwrap();
        }
        mid
    }

    #[test]
    fn commitment_extractor_normalizes_clauses_and_matches_completions() {
        assert!(is_user_channel("Me") && is_user_channel("mic"));
        assert!(!is_user_channel("Others"));
        assert_eq!(format_timestamp_ms(588_000), "09:48");
        assert_eq!(format_timestamp_ms(3_723_000), "01:02:03");

        let extracted = extract_commitments(
            "Okay, I'll send the empty-state copy. I'll be ruthless about the agenda.",
        );
        assert_eq!(extracted.len(), 1);
        assert_eq!(extracted[0].1, "Send the empty-state copy");

        let repeat = extract_commitments("Let me send the empty-state copy");
        assert_eq!(repeat[0].0, extracted[0].0);

        let done_sets = extract_completion_token_sets("I sent the empty-state copy this morning.");
        assert_eq!(done_sets.len(), 1);
        assert!(completion_matches(&extracted[0].0, &done_sets[0]));
    }

    #[test]
    fn todos_across_four_meetings_resolve_deduplicate_and_keep_done() {
        let dir = tempfile::tempdir().unwrap();
        let m1 = seed(
            dir.path(),
            1_700_000_000,
            "Product sync",
            &[
                (588_000, 594_000, "me", "I'll send the empty-state copy."),
                (610_000, 615_000, "others", "I'll update the pricing slide."),
                (760_000, 766_000, "me", "Let me list the open questions."),
            ],
        );
        let m2 = seed(
            dir.path(),
            1_700_086_400,
            "Design critique",
            &[(
                1_865_000,
                1_871_000,
                "me",
                "Remind me to book the design review.",
            )],
        );
        let m3 = seed(
            dir.path(),
            1_700_172_800,
            "Friday check-in",
            &[
                (42_000, 47_000, "me", "I sent the empty-state copy."),
                (
                    95_000,
                    100_000,
                    "me",
                    "I need to rotate the staging webhook key.",
                ),
            ],
        );

        let store = MeetingTodoStore::open(dir.path()).unwrap();
        let after_three = store.sync().unwrap();
        assert_eq!(after_three.len(), 4);
        assert!(after_three
            .iter()
            .all(|t| store.resolve_moment(&t.origin).unwrap()));

        let copy_item = after_three
            .iter()
            .find(|t| t.text == "Send the empty-state copy")
            .unwrap();
        assert_eq!(
            (copy_item.status, copy_item.origin.meeting_id),
            (TodoStatus::Done, m1)
        );
        assert_eq!(copy_item.origin.timestamp, "09:48");
        assert_eq!(copy_item.completed_meeting_id, Some(m3));

        let book = after_three
            .iter()
            .find(|t| t.text == "Book the design review")
            .unwrap();
        assert_eq!(
            (book.origin.meeting_id, book.origin.timestamp.as_str()),
            (m2, "31:05")
        );
        store.set_status(book.id, TodoStatus::Done).unwrap();

        let m4 = seed(
            dir.path(),
            1_700_259_200,
            "Monday standup",
            &[
                (15_000, 20_000, "me", "Let me book the design review."),
                (
                    30_000,
                    36_000,
                    "me",
                    "I will share the pilot onboarding checklist.",
                ),
            ],
        );
        let after_four = store.sync().unwrap();
        assert_eq!(after_four.len(), 5);
        let book_after = after_four.iter().find(|t| t.id == book.id).unwrap();
        assert_eq!(
            (book_after.status, book_after.origin.meeting_id),
            (TodoStatus::Done, m2)
        );
        let added = after_four
            .iter()
            .find(|t| t.origin.meeting_id == m4)
            .unwrap();
        assert!(store.resolve_moment(&added.origin).unwrap());

        let open_only: Vec<MeetingTodo> =
            serde_json::from_str(&run_todos_json(dir.path(), false).unwrap()).unwrap();
        assert_eq!(open_only.len(), 3);
    }

    #[test]
    fn todo_store_connection_enables_secure_delete() {
        let dir = tempfile::tempdir().unwrap();
        let store = MeetingTodoStore::open(dir.path()).unwrap();
        let conn = store.todos_conn().unwrap();
        let sd: i64 = conn
            .query_row("PRAGMA secure_delete;", [], |r| r.get(0))
            .unwrap();
        assert_eq!(sd, 1, "PRAGMA secure_delete must be enabled on todos.db");
    }

    #[test]
    fn deleted_meeting_todos_are_gone_after_sync_or_defer() {
        let dir = tempfile::tempdir().unwrap();
        let m1 = seed(
            dir.path(),
            1_700_000_000,
            "Product sync",
            &[(10_000, 15_000, "me", "I'll send the rollout summary.")],
        );
        let store = MeetingTodoStore::open(dir.path()).unwrap();
        assert_eq!(store.sync().unwrap().len(), 1);

        let mc = Connection::open(dir.path().join("meetings/meetings.db")).unwrap();
        mc.execute(
            "DELETE FROM meeting_segments WHERE meeting_id = ?1",
            params![m1],
        )
        .unwrap();
        mc.execute("DELETE FROM meetings WHERE id = ?1", params![m1])
            .unwrap();
        drop(mc);

        crate::meeting_ask_index::sync_or_defer(dir.path(), true, "delete_meeting");
        let remaining = store.list(true, None).unwrap();
        assert!(
            remaining.is_empty(),
            "deleted meeting to-dos must be removed from todos.db"
        );
        let wal = dir.path().join("meetings/todos.db-wal");
        let wal_len = fs::metadata(&wal).map(|m| m.len()).unwrap_or(0);
        assert_eq!(wal_len, 0, "todos.db-wal must truncate to 0 bytes");
    }

    #[test]
    fn sync_then_list_last_14_days_errors_while_todos_db_is_locked() {
        let dir = tempfile::tempdir().unwrap();
        let now = 1_700_100_000;
        let _m1 = seed(
            dir.path(),
            now - 3_600,
            "Product sync",
            &[(10_000, 15_000, "me", "I'll send the rollout summary.")],
        );
        let store = MeetingTodoStore::open(dir.path()).unwrap();
        assert_eq!(store.sync_then_list_last_14_days(now).unwrap().len(), 1);

        let locker = Connection::open(dir.path().join("meetings/todos.db")).unwrap();
        locker.execute_batch("BEGIN IMMEDIATE;").unwrap();

        let res = store.sync_then_list_last_14_days(now);
        assert!(
            res.is_err(),
            "sync_then_list_last_14_days must return Err while todos.db write lock is held"
        );

        locker.execute_batch("COMMIT;").unwrap();
    }

    #[test]
    fn deferred_todo_clean_finishes_on_retry() {
        let _serial = crate::meeting_ask_index::tests::test_pending_lock()
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        let m1 = seed(
            dir.path(),
            1_700_000_000,
            "Product sync",
            &[(10_000, 15_000, "me", "I'll send the rollout summary.")],
        );
        let store = MeetingTodoStore::open(dir.path()).unwrap();
        assert_eq!(store.sync().unwrap().len(), 1);

        let mc = Connection::open(dir.path().join("meetings/meetings.db")).unwrap();
        mc.execute(
            "DELETE FROM meeting_segments WHERE meeting_id = ?1",
            params![m1],
        )
        .unwrap();
        mc.execute("DELETE FROM meetings WHERE id = ?1", params![m1])
            .unwrap();
        drop(mc);

        let locker = Connection::open(dir.path().join("meetings/todos.db")).unwrap();
        locker.execute_batch("BEGIN IMMEDIATE;").unwrap();

        crate::meeting_ask_index::sync_or_defer(dir.path(), true, "delete_meeting");
        assert!(crate::meeting_ask_index::any_ask_clean_pending());
        let rows_before: i64 = locker
            .query_row("SELECT COUNT(*) FROM meeting_todos", [], |r| r.get(0))
            .unwrap();
        assert_eq!(rows_before, 1);

        locker.execute_batch("COMMIT;").unwrap();
        drop(locker);

        assert!(crate::meeting_ask_index::run_pending_ask_clean(dir.path()).unwrap());
        assert!(store.list(true, None).unwrap().is_empty());
        assert!(!crate::meeting_ask_index::run_pending_ask_clean(dir.path()).unwrap());
        assert!(!crate::meeting_ask_index::any_ask_clean_pending());
    }
}
