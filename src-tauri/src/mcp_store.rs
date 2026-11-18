//! Read-only local store access, opt-in policy check, and `0600` read audit log for MCP.
//! Opens `meetings.db` and `history.db` read-only, never reads audio files, and uses zero sockets.

// ponytail: ceiling = direct read-only SQLite queries per MCP call; upgrade path = WAL change hook when live polling exceeds 10 req/s.

use chrono::Utc;
use rusqlite::{params, Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::Path;
use std::time::Duration;

pub const MAX_ITEMS: usize = 50;
pub const MAX_SEGMENTS: usize = 200;
const MAX_AUDIT_ROWS: usize = 100;
type PastRow = (i64, i64, Option<i64>, String, String);

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AgentAccessPolicy {
    pub enabled: bool,
    pub live: bool,
    pub past: bool,
    pub custom_words: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentReadEntry {
    pub ts: i64,
    pub client: String,
    pub tool: String,
    pub target: String,
}

fn read_regular_file(path: &Path) -> Option<String> {
    let meta = fs::symlink_metadata(path).ok()?;
    (!meta.file_type().is_symlink() && meta.is_file())
        .then(|| fs::read_to_string(path).ok())
        .flatten()
}

impl AgentAccessPolicy {
    /// Re-read `settings_store.json` on every call so turning Agent access off revokes at once.
    pub fn load(data_dir: &Path) -> Self {
        let Some(raw) = read_regular_file(&data_dir.join("settings_store.json")) else {
            return Self::default();
        };
        let Ok(root) = serde_json::from_str::<Value>(&raw) else {
            return Self::default();
        };
        let obj = root.get("settings").unwrap_or(&root);
        let flag = |key: &str, def: bool| obj.get(key).and_then(Value::as_bool).unwrap_or(def);
        let enabled = flag("agent_access_enabled", false);
        let words = obj["custom_words"].as_array().into_iter().flatten();
        Self {
            enabled,
            live: enabled && flag("agent_access_live", true),
            past: enabled && flag("agent_access_past", true),
            custom_words: words
                .filter_map(Value::as_str)
                .take(MAX_ITEMS)
                .map(str::to_string)
                .collect(),
        }
    }
}

fn open_ro(path: &Path) -> Option<Connection> {
    let meta = fs::symlink_metadata(path).ok()?;
    if meta.file_type().is_symlink() || !meta.is_file() {
        return None;
    }
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let conn = Connection::open_with_flags(path, flags).ok()?;
    let _ = conn.busy_timeout(Duration::from_secs(1));
    let _ = conn.execute_batch("PRAGMA query_only = ON;");
    Some(conn)
}

fn has_table(conn: &Connection, name: &str) -> bool {
    let sql = "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type = 'table' AND name = ?1";
    conn.query_row(sql, params![name], |r| r.get(0))
        .unwrap_or(false)
}

fn active_clause(conn: &Connection) -> &'static str {
    let has_col = conn
        .prepare("PRAGMA table_info(meetings)")
        .ok()
        .and_then(|mut st| {
            let rows = st.query_map([], |r| r.get::<_, String>(1)).ok()?;
            Some(rows.flatten().any(|c| c == "trashed_at"))
        })
        .unwrap_or(false);
    match (has_col, has_table(conn, "meeting_trash")) {
        (true, true) => " AND trashed_at IS NULL AND NOT EXISTS (SELECT 1 FROM meeting_trash t WHERE t.meeting_id = meetings.id)",
        (true, false) => " AND trashed_at IS NULL",
        (false, true) => " AND NOT EXISTS (SELECT 1 FROM meeting_trash t WHERE t.meeting_id = meetings.id)",
        (false, false) => "",
    }
}

pub fn read_live_meeting(data_dir: &Path, after_id: i64) -> Result<(String, Value), &'static str> {
    let Some(conn) = open_ro(&data_dir.join("meetings/meetings.db")) else {
        return Ok(("none".into(), json!({ "recording": false, "segments": [] })));
    };
    let sql = format!(
        "SELECT id, started_at, title FROM meetings WHERE status = 'recording'{} ORDER BY id DESC LIMIT 1",
        active_clause(&conn)
    );
    let Ok((id, started_at, title)): rusqlite::Result<(i64, i64, String)> =
        conn.query_row(&sql, [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
    else {
        return Ok(("none".into(), json!({ "recording": false, "segments": [] })));
    };
    let segs = load_segments(&conn, id, after_id, MAX_ITEMS);
    let next = segs
        .last()
        .and_then(|s| s["id"].as_i64())
        .unwrap_or(after_id);
    let body = json!({
        "recording": true,
        "meeting": { "id": id, "title": &title, "started_at": started_at },
        "segments": segs,
        "next_after_segment_id": next
    });
    Ok((title, body))
}

pub fn read_past_list(data_dir: &Path, query: &str) -> Result<(String, Value), &'static str> {
    let Some(conn) = open_ro(&data_dir.join("meetings/meetings.db")) else {
        return Ok(("meetings".into(), json!({ "meetings": [] })));
    };
    let needle = query.trim().to_lowercase();
    let sql = format!(
        "SELECT id, started_at, ended_at, title, status FROM meetings WHERE status != 'recording'{} ORDER BY started_at DESC, id DESC LIMIT ?1",
        active_clause(&conn)
    );
    let mut st = conn.prepare(&sql).map_err(|_| "refused: store_error")?;
    let rows = st
        .query_map(params![MAX_ITEMS as i64], |r| {
            Ok::<PastRow, _>((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
        })
        .map_err(|_| "refused: store_error")?;
    let mut out = Vec::new();
    for (id, started_at, ended_at, title, status) in rows.flatten() {
        if needle.is_empty() || meeting_matches(&conn, id, &title, &needle) {
            out.push(json!({ "id": id, "title": title, "started_at": started_at, "ended_at": ended_at, "status": status }));
        }
    }
    Ok(("meetings".into(), json!({ "meetings": out })))
}

fn meeting_matches(conn: &Connection, id: i64, title: &str, needle: &str) -> bool {
    if title.to_lowercase().contains(needle) {
        return true;
    }
    let esc = needle
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    let pat = format!("%{esc}%");
    let seg_sql = "SELECT COUNT(*) > 0 FROM meeting_segments WHERE meeting_id = ?1 AND lower(text) LIKE ?2 ESCAPE '\\'";
    let seg_hit = conn
        .query_row(seg_sql, params![id, &pat], |r| r.get(0))
        .unwrap_or(false);
    if seg_hit || !has_table(conn, "meeting_notes") {
        return seg_hit;
    }
    let note_sql = "SELECT COUNT(*) > 0 FROM meeting_notes WHERE meeting_id = ?1 AND lower(text) LIKE ?2 ESCAPE '\\'";
    conn.query_row(note_sql, params![id, &pat], |r| r.get(0))
        .unwrap_or(false)
}

pub fn read_past_meeting(data_dir: &Path, id: i64) -> Result<(String, Value), &'static str> {
    let conn = open_ro(&data_dir.join("meetings/meetings.db")).ok_or("refused: not_found")?;
    let sql = format!(
        "SELECT started_at, ended_at, title, status FROM meetings WHERE id = ?1{}",
        active_clause(&conn)
    );
    let (started_at, ended_at, title, status): (i64, Option<i64>, String, String) = conn
        .query_row(&sql, params![id], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })
        .map_err(|_| "refused: not_found")?;
    if status == "recording" {
        return Err("refused: meeting_is_live");
    }
    let body = json!({
        "meeting": { "id": id, "title": &title, "started_at": started_at, "ended_at": ended_at, "status": status },
        "notes": load_notes(&conn, id),
        "notepad": load_notepad(&conn, id),
        "segments": load_segments(&conn, id, 0, MAX_SEGMENTS)
    });
    Ok((title, body))
}

fn load_segments(conn: &Connection, id: i64, after_id: i64, limit: usize) -> Vec<Value> {
    let sql = "SELECT id, start_ms, end_ms, channel, text FROM meeting_segments WHERE meeting_id = ?1 AND id > ?2 ORDER BY id ASC LIMIT ?3";
    let Ok(mut st) = conn.prepare(sql) else {
        return Vec::new();
    };
    let Ok(rows) = st.query_map(params![id, after_id, limit as i64], |r| {
        Ok(json!({ "id": r.get::<_, i64>(0)?, "start_ms": r.get::<_, i64>(1)?, "end_ms": r.get::<_, i64>(2)?, "channel": r.get::<_, String>(3)?, "text": r.get::<_, String>(4)? }))
    }) else { return Vec::new() };
    rows.flatten().collect()
}

fn load_notes(conn: &Connection, id: i64) -> Vec<Value> {
    if !has_table(conn, "meeting_notes") {
        return Vec::new();
    }
    let Ok(mut st) = conn.prepare("SELECT kind, text, owner, citations FROM meeting_notes WHERE meeting_id = ?1 ORDER BY position ASC") else { return Vec::new() };
    let Ok(rows) = st.query_map(params![id], |r| {
        let cites: Value = serde_json::from_str(&r.get::<_, String>(3)?).unwrap_or_else(|_| json!([]));
        Ok(json!({ "kind": r.get::<_, String>(0)?, "text": r.get::<_, String>(1)?, "owner": r.get::<_, Option<String>>(2)?, "citations": cites }))
    }) else { return Vec::new() };
    rows.flatten().collect()
}

fn load_notepad(conn: &Connection, id: i64) -> Vec<Value> {
    if !has_table(conn, "meeting_notepad") {
        return Vec::new();
    }
    let sql =
        "SELECT at_ms, text FROM meeting_notepad WHERE meeting_id = ?1 ORDER BY at_ms ASC, id ASC";
    let Ok(mut st) = conn.prepare(sql) else {
        return Vec::new();
    };
    let Ok(rows) = st.query_map(params![id], |r| {
        Ok(json!({ "at_ms": r.get::<_, i64>(0)?, "text": r.get::<_, String>(1)? }))
    }) else {
        return Vec::new();
    };
    rows.flatten().collect()
}

pub fn read_dictation_history(data_dir: &Path) -> Result<(String, Value), &'static str> {
    let Some(conn) = open_ro(&data_dir.join("history.db")) else {
        return Ok(("dictation_history".into(), json!({ "entries": [] })));
    };
    if !has_table(&conn, "transcription_history") {
        return Ok(("dictation_history".into(), json!({ "entries": [] })));
    }
    let sql = "SELECT id, timestamp, title, COALESCE(post_processed_text, transcription_text) FROM transcription_history ORDER BY timestamp DESC, id DESC LIMIT ?1";
    let mut st = conn.prepare(sql).map_err(|_| "refused: store_error")?;
    let rows = st
        .query_map(params![MAX_ITEMS as i64], |r| {
            Ok(json!({ "id": r.get::<_, i64>(0)?, "timestamp": r.get::<_, i64>(1)?, "title": r.get::<_, String>(2)?, "text": r.get::<_, String>(3)? }))
        })
        .map_err(|_| "refused: store_error")?;
    Ok((
        "dictation_history".into(),
        json!({ "entries": rows.flatten().collect::<Vec<_>>() }),
    ))
}

pub fn record_read(data_dir: &Path, client: &str, tool: &str, target: &str) -> io::Result<()> {
    let _ = crate::portable::make_private(data_dir);
    let path = data_dir.join("agent_reads.jsonl");
    if let Ok(meta) = fs::symlink_metadata(&path) {
        if meta.file_type().is_symlink() || !meta.is_file() {
            return Err(io::Error::from(io::ErrorKind::InvalidInput));
        }
    }
    let mut rows = recent_reads(data_dir);
    rows.insert(
        0,
        AgentReadEntry {
            ts: Utc::now().timestamp(),
            client: client.to_string(),
            tool: tool.to_string(),
            target: target.chars().take(120).collect(),
        },
    );
    rows.truncate(MAX_AUDIT_ROWS);
    let mut opts = OpenOptions::new();
    opts.create(true).write(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
        #[cfg(target_os = "macos")]
        opts.custom_flags(0x0100); // O_NOFOLLOW
        #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
        opts.custom_flags(0o100000); // O_NOFOLLOW
        #[cfg(all(target_os = "linux", not(target_arch = "aarch64")))]
        opts.custom_flags(0o400000); // O_NOFOLLOW
    }
    let mut fh = opts.open(&path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fh.set_permissions(fs::Permissions::from_mode(0o600));
    }
    for row in rows.iter().rev() {
        writeln!(fh, "{}", serde_json::to_string(row).unwrap_or_default())?;
    }
    Ok(())
}

pub fn recent_reads(data_dir: &Path) -> Vec<AgentReadEntry> {
    let Some(raw) = read_regular_file(&data_dir.join("agent_reads.jsonl")) else {
        return Vec::new();
    };
    let mut rows: Vec<AgentReadEntry> = raw
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    rows.reverse();
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_defaults_off_and_audit_log_is_private_without_transcript_text() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        assert_eq!(AgentAccessPolicy::load(p), AgentAccessPolicy::default());
        let sf = p.join("settings_store.json");
        fs::write(&sf, r#"{"settings":{"agent_access_enabled":false,"agent_access_live":true,"agent_access_past":true}}"#).unwrap();
        let off = AgentAccessPolicy::load(p);
        assert!(!off.enabled && !off.live && !off.past);

        fs::write(&sf, r#"{"settings":{"agent_access_enabled":true,"agent_access_live":false,"custom_words":["Parakeet"]}}"#).unwrap();
        let sc = AgentAccessPolicy::load(p);
        assert!(sc.enabled && !sc.live && sc.past && sc.custom_words == vec!["Parakeet"]);

        record_read(p, "Local Editor", "get_meeting", "Product sync").unwrap();
        let got = recent_reads(p);
        assert_eq!(got.len(), 1);
        assert_eq!(
            (&*got[0].client, &*got[0].target),
            ("Local Editor", "Product sync")
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let md = fs::metadata(p.join("agent_reads.jsonl")).unwrap();
            assert_eq!(md.permissions().mode() & 0o777, 0o600);
            let victim = p.join("victim.txt");
            fs::write(&victim, "keep-intact").unwrap();
            fs::remove_file(p.join("agent_reads.jsonl")).unwrap();
            std::os::unix::fs::symlink(&victim, p.join("agent_reads.jsonl")).unwrap();
            assert!(record_read(p, "Local Editor", "get_meeting", "Product sync").is_err());
            assert_eq!(fs::read_to_string(&victim).unwrap(), "keep-intact");
            assert!(recent_reads(p).is_empty());
        }
        fs::create_dir_all(p.join("meetings")).unwrap();
        let mc = Connection::open(p.join("meetings/meetings.db")).unwrap();
        mc.execute_batch(
            "CREATE TABLE meetings (id INTEGER PRIMARY KEY, started_at INTEGER NOT NULL, ended_at INTEGER, title TEXT NOT NULL, status TEXT NOT NULL, model_id TEXT NOT NULL, saved INTEGER NOT NULL DEFAULT 0);
             CREATE TABLE meeting_trash (meeting_id INTEGER PRIMARY KEY, trashed_at INTEGER NOT NULL);
             INSERT INTO meetings VALUES (1, 100, 200, 'Kept sync', 'notes_ready', 'local', 0), (2, 300, 400, 'Trashed sync', 'notes_ready', 'local', 0);
             INSERT INTO meeting_trash VALUES (2, 500);",
        ).unwrap();
        let listed = read_past_list(p, "").unwrap().1;
        assert_eq!(listed["meetings"].as_array().unwrap().len(), 1);
        assert_eq!(listed["meetings"][0]["id"], json!(1));
        assert_eq!(read_past_meeting(p, 2), Err("refused: not_found"));
    }
}
