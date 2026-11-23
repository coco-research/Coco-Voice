//! Offline importer for Markdown and plain-text meeting exports.
//!
//! Reads local `.md`, `.markdown`, and `.txt` files only (never fetching any
//! URL), places imported meetings into the `"Imported"` flat folder by default,
//! and deduplicates by SHA-256 so re-importing the same path adds nothing.

#![cfg(feature = "meetings")]

use std::fs::{self, OpenOptions};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

use anyhow::{anyhow, bail, Result};
use chrono::{NaiveDate, NaiveDateTime, NaiveTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use sha2::{Digest, Sha256};
use specta::Type;

pub use crate::managers::meetings::{Meeting, MeetingSegment, MeetingsStore, StoredNote};

pub const DEFAULT_IMPORT_FOLDER: &str = "Imported";
/// Maximum size per imported file (1 MiB, Q-X5 / AC-X34).
pub const MAX_IMPORT_FILE_BYTES: u64 = 1_048_576;
/// Maximum files imported in one batch (Q-X5 / AC-X34).
pub const MAX_IMPORT_FILES: usize = 500;
/// Maximum directory recursion depth (Q-X5 / AC-X34).
pub const MAX_IMPORT_DEPTH: usize = 3;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Type)]
pub struct ImportFailure {
    pub name: String,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Type)]
pub struct ImportSummary {
    pub folder: String,
    pub imported_count: usize,
    pub skipped_count: usize,
    pub meeting_ids: Vec<i64>,
    pub failed: Vec<ImportFailure>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Type)]
pub struct ImportCountResult {
    pub count: usize,
    pub display_folder: Option<String>,
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

fn collect_note_files(
    canon_root: &Path,
    dir: &Path,
    depth: usize,
    out: &mut Vec<(String, PathBuf)>,
    failed: &mut Vec<ImportFailure>,
) -> Result<()> {
    let mut entries: Vec<_> = fs::read_dir(dir)?.collect::<std::result::Result<Vec<_>, _>>()?;
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let name_str = entry.file_name().to_string_lossy().to_string();
        if name_str.starts_with('.') {
            continue;
        }
        let p = entry.path();
        let meta = match fs::symlink_metadata(&p) {
            Ok(m) => m,
            Err(_) => {
                failed.push(ImportFailure {
                    name: name_str,
                    reason: "unreadable".to_string(),
                });
                continue;
            }
        };
        if meta.file_type().is_symlink() {
            failed.push(ImportFailure {
                name: name_str,
                reason: "symlink not allowed".to_string(),
            });
            continue;
        }
        let Ok(canon_child) = fs::canonicalize(&p) else {
            failed.push(ImportFailure {
                name: name_str,
                reason: "unreadable".to_string(),
            });
            continue;
        };
        if !canon_child.starts_with(canon_root) {
            failed.push(ImportFailure {
                name: name_str,
                reason: "outside chosen folder".to_string(),
            });
            continue;
        }
        if meta.is_dir() {
            if depth < MAX_IMPORT_DEPTH {
                collect_note_files(canon_root, &canon_child, depth + 1, out, failed)?;
            }
            continue;
        }
        if !meta.is_file() {
            continue;
        }
        if !is_supported_note_file(&canon_child) {
            failed.push(ImportFailure {
                name: name_str,
                reason: "not text".to_string(),
            });
            continue;
        }
        if out.len() >= MAX_IMPORT_FILES {
            failed.push(ImportFailure {
                name: name_str,
                reason: "file limit reached".to_string(),
            });
            continue;
        }
        let rel = canon_child
            .strip_prefix(canon_root)
            .unwrap_or(&canon_child)
            .to_string_lossy()
            .replace('\\', "/");
        out.push((rel, canon_child));
    }
    Ok(())
}

/// Path-independent SHA-256 content hash so moving or renaming a file never creates a duplicate.
fn content_hash(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    format!("{:x}", h.finalize())
}

/// Open a regular file without following symlinks and enforce `MAX_IMPORT_FILE_BYTES`
/// both via metadata and bounded `.take(MAX_IMPORT_FILE_BYTES + 1)` reading.
fn read_bounded_nofollow(path: &Path) -> std::result::Result<(Vec<u8>, Option<i64>), &'static str> {
    let link_meta = fs::symlink_metadata(path).map_err(|_| "unreadable")?;
    if link_meta.file_type().is_symlink() {
        return Err("symlink not allowed");
    }
    if !link_meta.is_file() {
        return Err("not a file");
    }
    if link_meta.len() > MAX_IMPORT_FILE_BYTES {
        return Err("too large");
    }
    let mut opts = OpenOptions::new();
    opts.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.custom_flags(libc::O_NOFOLLOW);
    }
    let file = opts.open(path).map_err(|_| "unreadable")?;
    let meta = file.metadata().map_err(|_| "unreadable")?;
    if !meta.is_file() {
        return Err("not a file");
    }
    if meta.len() > MAX_IMPORT_FILE_BYTES {
        return Err("too large");
    }
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64);
    let mut buf = Vec::with_capacity(meta.len().min(MAX_IMPORT_FILE_BYTES) as usize);
    file.take(MAX_IMPORT_FILE_BYTES + 1)
        .read_to_end(&mut buf)
        .map_err(|_| "unreadable")?;
    if buf.len() as u64 > MAX_IMPORT_FILE_BYTES {
        return Err("too large");
    }
    Ok((buf, mtime))
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

fn collect_candidates_from_root(
    path: &Path,
    out: &mut Vec<(String, PathBuf)>,
    failed: &mut Vec<ImportFailure>,
) -> Result<()> {
    let root_meta = fs::symlink_metadata(path)
        .map_err(|_| anyhow!("import path does not exist: {}", path.display()))?;
    if root_meta.file_type().is_symlink() {
        bail!("refusing symlink import path: {}", path.display());
    }
    let canon_root = fs::canonicalize(path)?;
    if root_meta.is_file() {
        if !is_supported_note_file(&canon_root) {
            bail!("unsupported import file extension: {}", path.display());
        }
        let name = canon_root
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("note.md")
            .to_string();
        if out.len() < MAX_IMPORT_FILES {
            out.push((name, canon_root));
        } else {
            failed.push(ImportFailure {
                name,
                reason: "file limit reached".to_string(),
            });
        }
    } else if root_meta.is_dir() {
        collect_note_files(&canon_root, &canon_root, 0, out, failed)?;
    } else {
        bail!("import path must be a regular file or directory");
    }
    Ok(())
}

/// Dry-run scan counting importable `.md`/`.markdown`/`.txt` files across chosen paths
/// under the same symlink, depth, and file-count caps as [`import_notes_paths`].
pub fn count_importable_notes_paths(paths: &[PathBuf]) -> Result<ImportCountResult> {
    let mut files = Vec::new();
    let mut failed = Vec::new();
    let mut display_folder = None;
    for path in paths {
        if display_folder.is_none() {
            let folder_path = if path.is_dir() {
                Some(path.as_path())
            } else {
                path.parent()
            };
            display_folder = folder_path.map(|p| p.to_string_lossy().to_string());
        }
        let _ = collect_candidates_from_root(path, &mut files, &mut failed);
    }
    Ok(ImportCountResult {
        count: files.len(),
        display_folder,
    })
}

/// Import one `.md`/`.markdown`/`.txt` file or a directory of exports into `target_folder`
/// (`"Imported"` when `None`). Re-running on already-imported files is a no-op.
pub fn import_notes_path(
    store: &MeetingsStore,
    path: &Path,
    target_folder: Option<&str>,
) -> Result<ImportSummary> {
    import_notes_paths(store, &[path.to_path_buf()], target_folder)
}

/// Import one or more files/folders with strict symlink, canonicalization, size (<= 1 MB),
/// depth (<= 3), file count (<= 500), and path-independent content-hash deduplication.
pub fn import_notes_paths(
    store: &MeetingsStore,
    paths: &[PathBuf],
    target_folder: Option<&str>,
) -> Result<ImportSummary> {
    if paths.is_empty() {
        bail!("no import paths selected");
    }
    let folder = target_folder
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(DEFAULT_IMPORT_FOLDER)
        .replace(['/', '\\'], " ");
    let mut files: Vec<(String, PathBuf)> = Vec::new();
    let mut failed: Vec<ImportFailure> = Vec::new();

    for path in paths {
        collect_candidates_from_root(path, &mut files, &mut failed)?;
    }

    let mut conn = open_conn(store)?;
    let has_v_m8 = has_v_m8_folders_schema(&conn)?;
    let now = Utc::now().timestamp();
    let (mut imported_count, mut skipped_count, mut meeting_ids) = (0usize, 0usize, Vec::new());

    for (rel_key, file_path) in files {
        let display_name = file_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(&rel_key)
            .to_string();
        let (bytes, mtime_secs) = match read_bounded_nofollow(&file_path) {
            Ok(v) => v,
            Err(reason) => {
                failed.push(ImportFailure {
                    name: display_name,
                    reason: reason.to_string(),
                });
                continue;
            }
        };
        let Ok(raw_text) = String::from_utf8(bytes.clone()) else {
            failed.push(ImportFailure {
                name: display_name,
                reason: "not text".to_string(),
            });
            continue;
        };
        let hash = content_hash(&bytes);
        let exists: Option<i64> = conn
            .query_row(
                "SELECT meeting_id FROM meeting_imports WHERE source_hash = ?1",
                params![&hash],
                |r| r.get(0),
            )
            .optional()?;
        if exists.is_some() {
            skipped_count += 1;
            continue;
        }

        let fallback = file_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Imported note");
        let parsed = parse_export(&raw_text, fallback, mtime_secs.unwrap_or(now));
        let status = "imported";

        let tx = conn.transaction()?;
        tx.execute(
            "INSERT INTO meetings (started_at, ended_at, title, status, model_id, saved)
             VALUES (?1, ?2, ?3, ?4, 'imported', 1)",
            params![
                parsed.started_at,
                parsed.ended_at.or(Some(parsed.started_at)),
                &parsed.title,
                status
            ],
        )?;
        let id = tx.last_insert_rowid();
        if has_v_m8 {
            let folder_id = ensure_v_m8_folder_id(&tx, &folder, now)?;
            tx.execute(
                "UPDATE meetings SET folder_id = ?1 WHERE id = ?2",
                params![folder_id, id],
            )?;
        }
        for (pos, n) in parsed.notes.iter().enumerate() {
            tx.execute(
                "INSERT INTO meeting_notes (meeting_id, kind, text, owner, citations, position)
                 VALUES (?1, ?2, ?3, ?4, '[]', ?5)",
                params![id, &n.kind, &n.text, n.owner.as_deref(), pos as i64],
            )?;
        }
        for (start_ms, ch, text) in &parsed.segments {
            tx.execute(
                "INSERT INTO meeting_segments (meeting_id, start_ms, end_ms, channel, text)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![id, start_ms, start_ms + 1000, ch, text],
            )?;
        }
        tx.execute(
            "INSERT INTO meeting_imports (source_hash, meeting_id, folder, source_relpath, raw_text, imported_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![&hash, id, &folder, &rel_key, &raw_text, now],
        )?;
        tx.commit()?;
        imported_count += 1;
        meeting_ids.push(id);
    }

    Ok(ImportSummary {
        folder,
        imported_count,
        skipped_count,
        meeting_ids,
        failed,
    })
}

/// Return imported meetings assigned to `folder`, newest first.
pub fn list_meetings_in_folder(store: &MeetingsStore, folder: &str) -> Result<Vec<Meeting>> {
    let conn = open_conn(store)?;
    let mut stmt = conn.prepare(
        "SELECT m.id, m.started_at, m.ended_at, m.title, m.status, m.model_id, m.saved
         FROM meeting_imports i JOIN meetings m ON m.id = i.meeting_id
         WHERE i.folder = ?1 ORDER BY m.started_at DESC, m.id DESC",
    )?;
    let rows = stmt.query_map(params![folder], |r| {
        Ok(Meeting {
            id: r.get(0)?,
            started_at: r.get(1)?,
            ended_at: r.get(2)?,
            title: r.get(3)?,
            status: r.get(4)?,
            model_id: r.get(5)?,
            saved: r.get(6)?,
        })
    })?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|err| anyhow!(err))
}

/// Exact original source text stored for an imported meeting.
pub fn imported_raw_text(store: &MeetingsStore, meeting_id: i64) -> Result<Option<String>> {
    Ok(open_conn(store)?
        .query_row(
            "SELECT raw_text FROM meeting_imports WHERE meeting_id = ?1",
            params![meeting_id],
            |r| r.get(0),
        )
        .optional()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::managers::meetings_export::{meeting_markdown, ExportSegment};

    #[test]
    fn import_folder_places_exports_in_imported_matches_text_and_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let store = MeetingsStore::open(dir.path()).unwrap();
        let exports = dir.path().join("sample-exports");
        fs::create_dir_all(&exports).unwrap();

        let md_export = [
            "# Weekly sync",
            "",
            "2023-11-14 22:13 UTC to 22:43 UTC",
            "",
            "## Summary",
            "Look at the draft before Friday.",
            "",
            "## Decisions",
            "- Ship on Friday",
            "",
            "## Action items",
            "- Send the deck (owner: Priya)",
            "",
            "## Transcript",
            "[00:05] Me: I will send the deck",
            "[01:05] Others: Thanks",
            "",
        ]
        .join("\n");
        let third_party_md = [
            "---",
            "title: Customer onboarding",
            "date: 2024-03-10",
            "---",
            "## Key takeaways",
            "- Smooth setup flow; see https://example.invalid/spec (not fetched)",
            "## Next steps",
            "- [ ] Share checklist",
        ]
        .join("\n");
        let plain_txt = "Retrospective notes\n- Keep builds fast\n- Zero network calls\n";

        fs::write(exports.join("01-weekly.md"), &md_export).unwrap();
        fs::write(exports.join("02-onboarding.markdown"), &third_party_md).unwrap();
        fs::write(exports.join("03-retro.txt"), plain_txt).unwrap();
        fs::write(exports.join("ignore.png"), b"\x89PNG\r\n").unwrap();

        let first = import_notes_path(&store, &exports, None).unwrap();
        assert_eq!(first.folder, "Imported");
        assert_eq!((first.imported_count, first.skipped_count), (3, 0));
        assert_eq!(
            list_meetings_in_folder(&store, "Imported").unwrap().len(),
            3
        );

        let id0 = first.meeting_ids[0];
        assert_eq!(
            imported_raw_text(&store, id0).unwrap().as_deref(),
            Some(md_export.as_str())
        );
        assert_eq!(
            imported_raw_text(&store, first.meeting_ids[1])
                .unwrap()
                .as_deref(),
            Some(third_party_md.as_str())
        );
        assert_eq!(
            imported_raw_text(&store, first.meeting_ids[2])
                .unwrap()
                .as_deref(),
            Some(plain_txt)
        );

        let m0 = store.meeting(id0).unwrap();
        let n0 = store.notes(id0).unwrap();
        let s0: Vec<ExportSegment> = store
            .segments(id0)
            .unwrap()
            .into_iter()
            .map(|s| ExportSegment {
                id: s.id,
                start_ms: s.start_ms,
                channel: if s.channel == "me" {
                    "mic".into()
                } else {
                    "system".into()
                },
                text: s.text,
            })
            .collect();
        assert_eq!(meeting_markdown(&m0, &n0, &s0), md_export);
        assert_eq!(store.search_meetings("Priya").unwrap().len(), 1);

        let second = import_notes_path(&store, &exports, None).unwrap();
        assert_eq!((second.imported_count, second.skipped_count), (0, 3));
        assert_eq!(store.meeting_count().unwrap(), 3);
    }

    #[test]
    fn import_refuses_missing_path_and_unsupported_extension() {
        let dir = tempfile::tempdir().unwrap();
        let store = MeetingsStore::open(dir.path()).unwrap();
        assert!(import_notes_path(&store, &dir.path().join("missing.md"), None).is_err());
        let bad = dir.path().join("archive.zip");
        fs::write(&bad, b"PK\x03\x04").unwrap();
        assert!(import_notes_path(&store, &bad, None).is_err());
        assert_eq!(store.meeting_count().unwrap(), 0);
    }

    #[test]
    fn import_enforces_caps_rejects_symlinks_dedupes_by_content_and_sets_imported_status() {
        let dir = tempfile::tempdir().unwrap();
        let store = MeetingsStore::open(dir.path()).unwrap();
        let root = dir.path().join("batch");
        fs::create_dir_all(&root).unwrap();

        let good_body = "# Launch Prep\n## Decisions\n- Ship offline\n";
        fs::write(root.join("a.md"), good_body).unwrap();
        // Same content under a different filename -> path-independent content-hash dedup skips it.
        fs::write(root.join("b-copy.md"), good_body).unwrap();
        // Oversized (> 1 MiB) file -> reported as "too large".
        fs::write(
            root.join("huge.md"),
            vec![b'x'; (MAX_IMPORT_FILE_BYTES + 64) as usize],
        )
        .unwrap();
        // Unsupported PNG -> reported as "not text".
        fs::write(root.join("photo.png"), b"\x89PNG\r\n").unwrap();
        // Depth 4 nested file -> ignored beyond MAX_IMPORT_DEPTH (3).
        let deep = root.join("d1").join("d2").join("d3").join("d4");
        fs::create_dir_all(&deep).unwrap();
        fs::write(deep.join("too-deep.md"), "# Too deep\n").unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            let outside = dir.path().join("secret.md");
            fs::write(&outside, "# Outside secret\n").unwrap();
            symlink(&outside, root.join("escape.md")).unwrap();
            // Symlink root itself must be refused outright.
            let link_root = dir.path().join("link-root");
            symlink(&root, &link_root).unwrap();
            assert!(import_notes_path(&store, &link_root, None).is_err());
        }

        let summary = import_notes_path(&store, &root, Some("Imported")).unwrap();
        assert_eq!(summary.imported_count, 1);
        assert_eq!(summary.skipped_count, 1);
        assert!(summary
            .failed
            .iter()
            .any(|f| f.name == "huge.md" && f.reason == "too large"));
        assert!(summary
            .failed
            .iter()
            .any(|f| f.name == "photo.png" && f.reason == "not text"));
        #[cfg(unix)]
        assert!(summary
            .failed
            .iter()
            .any(|f| f.name == "escape.md" && f.reason.contains("symlink")));

        let imported = store.meeting(summary.meeting_ids[0]).unwrap();
        assert_eq!(imported.status, "imported");
    }
}
