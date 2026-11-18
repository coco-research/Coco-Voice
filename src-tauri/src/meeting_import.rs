#![allow(dead_code)]
//! Offline importer for Markdown and plain-text meeting exports.
//!
//! Reads local `.md`, `.markdown`, and `.txt` files only (never fetching any
//! URL), places imported meetings into the `"Imported"` flat folder by default,
//! and deduplicates by SHA-256 so re-importing the same path adds nothing.

#![cfg(feature = "meetings")]

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::{NaiveDate, NaiveDateTime, NaiveTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use sha2::{Digest, Sha256};

pub use crate::managers::meetings::{Meeting, MeetingSegment, MeetingsStore, StoredNote};

pub const DEFAULT_IMPORT_FOLDER: &str = "Imported";

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ImportSummary {
    pub folder: String,
    pub imported_count: usize,
    pub skipped_count: usize,
    pub meeting_ids: Vec<i64>,
}

fn open_conn(store: &MeetingsStore) -> Result<Connection> {
    let conn = Connection::open(store.db_path())?;
    conn.busy_timeout(Duration::from_secs(1))?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS meeting_imports (
            source_hash TEXT PRIMARY KEY,
            meeting_id INTEGER NOT NULL,
            folder TEXT NOT NULL,
            source_relpath TEXT NOT NULL,
            raw_text TEXT NOT NULL,
            imported_at INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS meeting_imports_folder ON meeting_imports(folder, meeting_id);",
    )?;
    Ok(conn)
}

fn has_v_m8_folders_schema(conn: &Connection) -> Result<bool> {
    let tables: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'meeting_folders'",
        [],
        |r| r.get(0),
    )?;
    if tables == 0 {
        return Ok(false);
    }
    let has_col: i64 = conn.query_row(
        "SELECT COUNT(*) FROM pragma_table_info('meetings') WHERE name = 'folder_id'",
        [],
        |r| r.get(0),
    )?;
    Ok(has_col > 0)
}

fn ensure_v_m8_folder_id(tx: &rusqlite::Transaction<'_>, folder: &str, now: i64) -> Result<i64> {
    if let Some(id) = tx
        .query_row(
            "SELECT id FROM meeting_folders WHERE name = ?1",
            params![folder],
            |r| r.get(0),
        )
        .optional()?
    {
        return Ok(id);
    }
    tx.execute(
        "INSERT INTO meeting_folders (name, created_at) VALUES (?1, ?2)",
        params![folder, now],
    )?;
    Ok(tx.last_insert_rowid())
}

fn is_supported_note_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "md" | "markdown" | "txt"))
}

fn collect_note_files(root: &Path, dir: &Path, out: &mut Vec<(String, PathBuf)>) -> Result<()> {
    let mut entries: Vec<_> = fs::read_dir(dir)?.collect::<std::result::Result<Vec<_>, _>>()?;
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        if entry.file_name().to_string_lossy().starts_with('.') || entry.file_type()?.is_symlink() {
            continue;
        }
        let p = entry.path();
        if p.is_dir() {
            collect_note_files(root, &p, out)?;
        } else if p.is_file() && is_supported_note_file(&p) {
            let rel = p
                .strip_prefix(root)
                .unwrap_or(&p)
                .to_string_lossy()
                .replace('\\', "/");
            out.push((rel, p));
        }
    }
    Ok(())
}

fn source_hash(rel_key: &str, bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(rel_key.as_bytes());
    h.update([0u8]);
    h.update(bytes);
    format!("{:x}", h.finalize())
}

struct ParsedExport {
    title: String,
    started_at: i64,
    ended_at: Option<i64>,
    notes: Vec<StoredNote>,
    segments: Vec<(i64, &'static str, String)>,
}

fn parse_clock_ms(token: &str) -> Option<i64> {
    let parts: Vec<i64> = token
        .trim()
        .split(':')
        .map(|p| p.parse::<i64>().ok())
        .collect::<Option<Vec<_>>>()?;
    match parts.as_slice() {
        [m, s] => Some((m * 60 + s) * 1000),
        [h, m, s] => Some((h * 3600 + m * 60 + s) * 1000),
        _ => None,
    }
}

fn parse_utc_range(line: &str) -> Option<(i64, Option<i64>)> {
    let (start_part, rest) = line.trim().split_once(" UTC")?;
    let start_dt = NaiveDateTime::parse_from_str(start_part.trim(), "%Y-%m-%d %H:%M").ok()?;
    let end_ts = rest
        .trim()
        .strip_prefix("to ")
        .and_then(|s| s.strip_suffix(" UTC"))
        .and_then(|hm| NaiveTime::parse_from_str(hm.trim(), "%H:%M").ok())
        .map(|t| {
            let mut dt = NaiveDateTime::new(start_dt.date(), t);
            if dt < start_dt {
                dt += chrono::Duration::days(1);
            }
            dt.and_utc().timestamp()
        });
    Some((start_dt.and_utc().timestamp(), end_ts))
}

fn parse_date_value(raw: &str) -> Option<i64> {
    let c = raw.trim().trim_matches(['"', '\'']);
    chrono::DateTime::parse_from_rfc3339(c)
        .ok()
        .map(|dt| dt.timestamp())
        .or_else(|| {
            NaiveDateTime::parse_from_str(c, "%Y-%m-%d %H:%M")
                .ok()
                .map(|dt| dt.and_utc().timestamp())
        })
        .or_else(|| {
            NaiveDate::parse_from_str(c, "%Y-%m-%d")
                .ok()
                .and_then(|d| d.and_hms_opt(0, 0, 0))
                .map(|dt| dt.and_utc().timestamp())
        })
}

fn strip_frontmatter(text: &str) -> (&str, Option<String>, Option<i64>) {
    let Some(rest) = text
        .strip_prefix("---\n")
        .or_else(|| text.strip_prefix("---\r\n"))
    else {
        return (text, None, None);
    };
    let Some(idx) = rest.find("\n---\n").or_else(|| rest.find("\r\n---\r\n")) else {
        return (text, None, None);
    };
    let (fm, after) = rest.split_at(idx);
    let body = after
        .strip_prefix("\n---\n")
        .or_else(|| after.strip_prefix("\r\n---\r\n"))
        .unwrap_or(after);
    let (mut title, mut started) = (None, None);
    for (k, v) in fm.lines().filter_map(|l| l.split_once(':')) {
        match k.trim().to_ascii_lowercase().as_str() {
            "title" => {
                title =
                    Some(v.trim().trim_matches(['"', '\'']).to_string()).filter(|s| !s.is_empty())
            }
            "date" | "started_at" | "created" => started = started.or_else(|| parse_date_value(v)),
            _ => {}
        }
    }
    (body, title, started)
}

fn section_kind(heading: &str) -> &'static str {
    match heading.trim().to_ascii_lowercase().as_str() {
        "summary" | "overview" | "tl;dr" | "key takeaways" => "summary",
        "decisions" | "decision" => "decision",
        "action items" | "actions" | "next steps" | "tasks" | "to-dos" | "todos" => "action",
        "questions" | "open questions" => "question",
        "transcript" => "transcript",
        _ => "other",
    }
}

fn parse_action_owner(line: &str) -> (String, Option<String>) {
    if let Some((body, owner)) = line
        .strip_suffix(')')
        .and_then(|p| p.rsplit_once(" (owner: "))
    {
        if !owner.trim().is_empty() {
            return (body.trim().to_string(), Some(owner.trim().to_string()));
        }
    }
    (line.to_string(), None)
}

fn parse_transcript_line(line: &str) -> Option<(i64, &'static str, String)> {
    let (clock_str, after) = line.trim().strip_prefix('[')?.split_once(']')?;
    let start_ms = parse_clock_ms(clock_str)?;
    let (speaker, spoken) = after.trim().split_once(':')?;
    let ch = if speaker.trim().eq_ignore_ascii_case("me") {
        "me"
    } else {
        "others"
    };
    Some((start_ms, ch, spoken.trim().to_string()))
}

fn parse_export(raw_text: &str, fallback_title: &str, fallback_ts: i64) -> ParsedExport {
    let (body, mut title, mut started_at) = strip_frontmatter(raw_text);
    let (mut ended_at, mut kind, mut saw_heading) = (None, "summary", false);
    let (mut notes, mut segments) = (Vec::new(), Vec::new());

    for trimmed in body.lines().map(str::trim).filter(|l| !l.is_empty()) {
        if let Some(h1) = trimmed.strip_prefix("# ") {
            if title.is_none() {
                title = Some(h1.trim().to_string());
                continue;
            }
        }
        if let Some(h2) = trimmed
            .strip_prefix("## ")
            .or_else(|| trimmed.strip_prefix("### "))
        {
            kind = section_kind(h2);
            saw_heading = true;
            continue;
        }
        if !saw_heading && started_at.is_none() {
            if let Some((s, e)) = parse_utc_range(trimmed) {
                (started_at, ended_at) = (Some(s), e);
                continue;
            }
            if let Some(s) = trimmed.strip_prefix("Date:").and_then(parse_date_value) {
                started_at = Some(s);
                continue;
            }
        }
        if kind == "transcript" {
            if let Some(seg) = parse_transcript_line(trimmed) {
                segments.push(seg);
                continue;
            }
        }
        let clean = ["- [ ] ", "- [x] ", "- ", "* "]
            .iter()
            .find_map(|p| trimmed.strip_prefix(p))
            .unwrap_or(trimmed)
            .trim();
        if !clean.is_empty() {
            let (text, owner) = if kind == "action" {
                parse_action_owner(clean)
            } else {
                (clean.to_string(), None)
            };
            notes.push(StoredNote {
                kind: kind.to_string(),
                text,
                owner,
                citations: Vec::new(),
            });
        }
    }

    ParsedExport {
        title: title
            .filter(|t| !t.trim().is_empty())
            .unwrap_or_else(|| fallback_title.to_string()),
        started_at: started_at.unwrap_or(fallback_ts),
        ended_at,
        notes,
        segments,
    }
}
