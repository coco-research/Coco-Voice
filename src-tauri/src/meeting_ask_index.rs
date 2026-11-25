//! Persistent local SQLite BM25 index (`{app_data_dir}/meetings/ask_index.db`) over meetings.
//! Opens `meetings.db` read-only, supports scoped Q&A, and makes zero network calls.

#![cfg(feature = "meetings")]

// ponytail: ceiling = local SQLite BM25 index (<100k segments); upgrade path = quantized local vector ANN when recall drops.

use anyhow::Result;
use rusqlite::{params, Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use unicode_segmentation::UnicodeSegmentation;

// ponytail: ceiling = the pending ask-index clean flag lives in memory per index
// path while startup runs one sync-and-compact on launch; upgrade path = persist a
// marker file beside ask_index.db if mid-session crash recovery without waiting for
// startup is needed.
fn pending_ask_clean_set() -> &'static Mutex<HashSet<PathBuf>> {
    static PENDING: OnceLock<Mutex<HashSet<PathBuf>>> = OnceLock::new();
    PENDING.get_or_init(|| Mutex::new(HashSet::new()))
}

fn pending_ask_key(data_dir: &Path) -> PathBuf {
    let meetings_dir = data_dir.join("meetings");
    let base = fs::canonicalize(&meetings_dir)
        .or_else(|_| fs::canonicalize(data_dir).map(|p| p.join("meetings")))
        .unwrap_or(meetings_dir);
    base.join("ask_index.db")
}

/// True when any Ask index has a deferred clean waiting to retry.
pub fn any_ask_clean_pending() -> bool {
    let guard = pending_ask_clean_set()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    !guard.is_empty()
}

fn run_ask_store_clean(data_dir: &Path, compact: bool) -> Result<()> {
    let idx = MeetingAskIndex::open(data_dir)?;
    idx.sync()?;
    if compact {
        idx.compact_secure()?;
    }
    Ok(())
}

fn run_todo_store_clean(data_dir: &Path, compact: bool) -> Result<()> {
    let store = crate::meeting_todos::MeetingTodoStore::open(data_dir)?;
    store.sync()?;
    if compact {
        store.compact_secure()?;
    }
    Ok(())
}

/// Sync (and optionally securely compact) the Ask index and to-do store for any
/// database file that already exists under `meetings/`. Missing stores are skipped
/// without creating files. On any error, logs without meeting content and queues
/// a background retry.
pub fn sync_or_defer(data_dir: &Path, compact: bool, context: &str) {
    let meetings_dir = data_dir.join("meetings");
    let ask_exists = meetings_dir.join("ask_index.db").exists();
    let todos_exists = meetings_dir.join("todos.db").exists();
    if !ask_exists && !todos_exists {
        return;
    }
    let key = pending_ask_key(data_dir);
    let mut deferred = false;
    if ask_exists {
        if let Err(err) = run_ask_store_clean(data_dir, compact) {
            log::warn!("ask index clean deferred after {context}: {err}");
            deferred = true;
        }
    }
    if todos_exists {
        if let Err(err) = run_todo_store_clean(data_dir, compact) {
            log::warn!("meetings todos clean deferred after {context}: {err}");
            deferred = true;
        }
    }
    if deferred {
        let mut guard = pending_ask_clean_set()
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        guard.insert(key);
    } else if compact {
        let mut guard = pending_ask_clean_set()
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        guard.remove(&key);
    }
}

/// Retry a deferred Ask index and to-do store sync and secure compact for `data_dir`.
/// Returns `Ok(false)` if nothing was pending, `Ok(true)` if the retry succeeded,
/// or re-queues the pending flag and returns `Err` on failure.
pub fn run_pending_ask_clean(data_dir: &Path) -> Result<bool> {
    let key = pending_ask_key(data_dir);
    let was_pending = {
        let mut guard = pending_ask_clean_set()
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        guard.remove(&key)
    };
    if !was_pending {
        return Ok(false);
    }
    let meetings_dir = data_dir.join("meetings");
    let ask_exists = meetings_dir.join("ask_index.db").exists();
    let todos_exists = meetings_dir.join("todos.db").exists();
    let mut first_err: Option<anyhow::Error> = None;
    if ask_exists {
        if let Err(err) = run_ask_store_clean(data_dir, true) {
            first_err = Some(err);
        }
    }
    if todos_exists {
        if let Err(err) = run_todo_store_clean(data_dir, true) {
            if first_err.is_none() {
                first_err = Some(err);
            } else {
                log::warn!("meetings todos background clean deferred: {err}");
            }
        }
    }
    match first_err {
        None => Ok(true),
        Some(err) => {
            let mut guard = pending_ask_clean_set()
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            guard.insert(key);
            Err(err)
        }
    }
}

pub const TOP_K: usize = 5;
const STOP: &str = " a about an and are at be by did do does for from had has have how i in is it \
    me my of on or our that the their this to was we were what when where which who why with you your ";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AskScope {
    OneMeeting(i64),
    AllMeetings,
    Folder(String),
    PickedSet(Vec<i64>),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AskCitation {
    pub meeting_id: i64,
    pub meeting_title: String,
    #[serde(default)]
    pub started_at: i64,
    pub segment_id: i64,
    pub start_ms: i64,
    pub end_ms: i64,
    pub timestamp: String,
    pub channel: String,
    pub quote: String,
}

impl AskCitation {
    pub fn resolves_in(&self, data_dir: &Path) -> bool {
        let Some(c) = open_ro(&data_dir.join("meetings/meetings.db")) else {
            return false;
        };
        if has_table(&c, "meeting_trash") {
            let trashed: bool = c
                .query_row(
                    "SELECT COUNT(*) > 0 FROM meeting_trash WHERE meeting_id = ?1",
                    params![self.meeting_id],
                    |r| r.get(0),
                )
                .unwrap_or(false);
            if trashed {
                return false;
            }
        }
        if self.segment_id < 0 {
            let note_id = -self.segment_id;
            let sql = "SELECT text FROM meeting_notes WHERE id = ?1 AND meeting_id = ?2";
            return c
                .query_row(sql, params![note_id, self.meeting_id], |r| {
                    Ok(!r.get::<_, String>(0)?.trim().is_empty())
                })
                .unwrap_or(false);
        }
        let sql = "SELECT start_ms, text FROM meeting_segments WHERE id = ?1 AND meeting_id = ?2";
        c.query_row(sql, params![self.segment_id, self.meeting_id], |r| {
            Ok(r.get::<_, i64>(0)? == self.start_ms && !r.get::<_, String>(1)?.is_empty())
        })
        .unwrap_or(false)
    }
}

pub struct MeetingAskIndex {
    pub(crate) data_dir: PathBuf,
    index_path: PathBuf,
}

impl MeetingAskIndex {
    pub fn open(data_dir: &Path) -> Result<Self> {
        let mdir = data_dir.join("meetings");
        fs::create_dir_all(&mdir)?;
        let _ = crate::portable::make_private(&mdir);
        let idx = Self {
            data_dir: data_dir.to_path_buf(),
            index_path: mdir.join("ask_index.db"),
        };
        idx.conn()?.execute_batch(
            "PRAGMA secure_delete = ON;
             PRAGMA journal_mode = WAL;
             CREATE TABLE IF NOT EXISTS ask_folders (meeting_id INTEGER PRIMARY KEY, folder TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS indexed_meetings (meeting_id INTEGER PRIMARY KEY, title TEXT NOT NULL, folder TEXT, started_at INTEGER NOT NULL, fingerprint TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS ask_docs (doc_id INTEGER PRIMARY KEY, meeting_id INTEGER NOT NULL, segment_id INTEGER NOT NULL, start_ms INTEGER NOT NULL, end_ms INTEGER NOT NULL, channel TEXT NOT NULL, text TEXT NOT NULL, token_len INTEGER NOT NULL);
             CREATE TABLE IF NOT EXISTS ask_terms (term TEXT NOT NULL, doc_id INTEGER NOT NULL, tf INTEGER NOT NULL, PRIMARY KEY (term, doc_id));",
        )?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&idx.index_path, fs::Permissions::from_mode(0o600));
        }
        Ok(idx)
    }

    /// Securely zero freed pages and truncate WAL on `ask_index.db`.
    pub fn compact_secure(&self) -> Result<()> {
        let c = self.conn()?;
        c.execute_batch("PRAGMA secure_delete = ON;")?;
        crate::managers::meetings::wal_checkpoint_truncate(&c)?;
        c.execute_batch("VACUUM;")?;
        crate::managers::meetings::wal_checkpoint_truncate(&c)?;
        Ok(())
    }

    pub fn set_folder(&self, id: i64, folder: Option<&str>) -> Result<()> {
        let c = self.conn()?;
        let up = "INSERT INTO ask_folders VALUES (?1, ?2) ON CONFLICT(meeting_id) DO UPDATE SET folder = excluded.folder";
        let rm = "DELETE FROM ask_folders WHERE meeting_id = ?1";
        match folder.map(str::trim).filter(|s| !s.is_empty()) {
            Some(name) => c.execute(up, params![id, name])?,
            None => c.execute(rm, [id])?,
        };
        c.execute("DELETE FROM indexed_meetings WHERE meeting_id = ?1", [id])?;
        Ok(())
    }

    pub fn sync(&self) -> Result<usize> {
        let mut ic = self.conn()?;
        let wipe = "DELETE FROM ask_terms; DELETE FROM ask_docs; DELETE FROM indexed_meetings;";
        let Some(mc) = open_ro(&self.data_dir.join("meetings/meetings.db")) else {
            ic.execute_batch(wipe)?;
            return Ok(0);
        };
        let overrides = kv_map(&ic, "SELECT meeting_id, folder FROM ask_folders");
        let existing = kv_map(&ic, "SELECT meeting_id, fingerprint FROM indexed_meetings");
        let cols = table_cols(&mc, "meetings");
        let mut where_clauses: Vec<&str> = Vec::new();
        if has_table(&mc, "meeting_trash") {
            where_clauses
                .push("NOT EXISTS (SELECT 1 FROM meeting_trash t WHERE t.meeting_id = m.id)");
        }
        if cols.iter().any(|c| c == "trashed_at") {
            where_clauses.push("m.trashed_at IS NULL");
        }
        let trash = if where_clauses.is_empty() {
            String::new()
        } else {
            format!(" WHERE {}", where_clauses.join(" AND "))
        };
        let src = if cols.iter().any(|c| c == "folder_id") && has_table(&mc, "meeting_folders") {
            "f.name FROM meetings m LEFT JOIN meeting_folders f ON f.id = m.folder_id"
        } else if cols.iter().any(|c| c == "folder_id") && has_table(&mc, "folders") {
            "f.name FROM meetings m LEFT JOIN folders f ON f.id = m.folder_id"
        } else {
            "NULL FROM meetings m"
        };
        let mut mst = mc.prepare(&format!(
            "SELECT m.id, m.started_at, m.title, {src}{trash} ORDER BY m.id ASC"
        ))?;
        let mrows: Vec<(i64, i64, String, Option<String>)> = mst
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
            .collect::<rusqlite::Result<_>>()?;
        let seen: HashSet<i64> = mrows.iter().map(|r| r.0).collect();
        let has_notes = has_table(&mc, "meeting_notes");
        let tx = ic.transaction()?;
        for old_id in existing.keys().copied().filter(|id| !seen.contains(id)) {
            purge(&tx, old_id)?;
        }
        let mut updated = 0usize;
        for (id, started_at, title, db_folder) in mrows {
            let folder = overrides.get(&id).cloned().or(db_folder);
            let q_s = "SELECT id, start_ms, end_ms, channel, text FROM meeting_segments WHERE meeting_id = ?1 ORDER BY id ASC";
            let mut sst = mc.prepare(q_s)?;
            let segs: Vec<(i64, i64, i64, String, String)> = sst
                .query_map([id], |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
                })?
                .collect::<rusqlite::Result<_>>()?;
            let mut note_by_seg = HashMap::<i64, String>::new();
            let mut standalone_notes: Vec<(i64, String)> = Vec::new();
            let mut n_cnt = 0usize;
            if has_notes {
                let q_n = "SELECT id, text, citations FROM meeting_notes WHERE meeting_id = ?1 ORDER BY position ASC, id ASC";
                let mut nst = mc.prepare(q_n)?;
                let nrows = nst.query_map([id], |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                    ))
                })?;
                for (nid, txt, raw_c) in nrows.flatten() {
                    n_cnt += 1;
                    let cids = serde_json::from_str::<Vec<i64>>(&raw_c).unwrap_or_default();
                    if cids.is_empty() && !txt.trim().is_empty() {
                        standalone_notes.push((nid, txt.clone()));
                    }
                    for cid in cids {
                        let buf = note_by_seg.entry(cid).or_default();
                        buf.push(' ');
                        buf.push_str(&txt);
                    }
                }
            }
            let bytes: usize = segs.iter().map(|s| s.4.len()).sum::<usize>()
                + standalone_notes.iter().map(|(_, t)| t.len()).sum::<usize>();
            let last = segs
                .last()
                .map(|s| s.0)
                .or_else(|| standalone_notes.last().map(|(nid, _)| -*nid))
                .unwrap_or(0);
            let fp = format!("{title}|{folder:?}|{}|{last}|{bytes}|{n_cnt}", segs.len());
            if existing.get(&id).map(String::as_str) == Some(fp.as_str()) {
                continue;
            }
            purge(&tx, id)?;
            let ins_m = "INSERT INTO indexed_meetings VALUES (?1, ?2, ?3, ?4, ?5)";
            tx.execute(ins_m, params![id, title, folder, started_at, fp])?;
            if !segs.is_empty() {
                for (sid, s_ms, e_ms, ch, txt) in segs {
                    let extra = note_by_seg.get(&sid).map(String::as_str).unwrap_or("");
                    let toks = tokenize(&format!("{title} {txt} {extra}"));
                    let ins_d = "INSERT INTO ask_docs (meeting_id, segment_id, start_ms, end_ms, channel, text, token_len) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)";
                    tx.execute(
                        ins_d,
                        params![id, sid, s_ms, e_ms, ch, txt, toks.len() as i64],
                    )?;
                    let doc_id = tx.last_insert_rowid();
                    let mut tf_map = HashMap::<String, i64>::new();
                    for t in toks {
                        *tf_map.entry(t).or_insert(0) += 1;
                    }
                    for (term, tf) in tf_map {
                        tx.execute(
                            "INSERT INTO ask_terms VALUES (?1, ?2, ?3)",
                            params![term, doc_id, tf],
                        )?;
                    }
                }
            } else {
                // Imported notes-only meetings (0 transcript segments) remain searchable via Ask.
                for (nid, txt) in standalone_notes {
                    let toks = tokenize(&format!("{title} {txt}"));
                    let ins_d = "INSERT INTO ask_docs (meeting_id, segment_id, start_ms, end_ms, channel, text, token_len) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)";
                    tx.execute(
                        ins_d,
                        params![id, -nid, 0i64, 0i64, "notes", txt, toks.len() as i64],
                    )?;
                    let doc_id = tx.last_insert_rowid();
                    let mut tf_map = HashMap::<String, i64>::new();
                    for t in toks {
                        *tf_map.entry(t).or_insert(0) += 1;
                    }
                    for (term, tf) in tf_map {
                        tx.execute(
                            "INSERT INTO ask_terms VALUES (?1, ?2, ?3)",
                            params![term, doc_id, tf],
                        )?;
                    }
                }
            }
            updated += 1;
        }
        tx.commit()?;
        Ok(updated)
    }

    pub fn retrieve(&self, scope: &AskScope, question: &str) -> Result<Vec<AskCitation>> {
        let ic = self.conn()?;
        let docs = load_scoped_docs(&ic, scope)?;
        if docs.is_empty() {
            return Ok(Vec::new());
        }
        let q_terms = tokenize(question);
        if question.to_lowercase().contains("what did i miss") || q_terms.is_empty() {
            let skip = docs.len().saturating_sub(TOP_K);
            return Ok(docs.into_iter().skip(skip).map(|(_, c, _)| c).collect());
        }
        let n_docs = docs.len() as f64;
        let avg_len = (docs.iter().map(|(_, _, l)| *l as f64).sum::<f64>() / n_docs).max(1.0);
        let doc_map: HashMap<i64, (&AskCitation, i64)> =
            docs.iter().map(|(id, c, l)| (*id, (c, *l))).collect();
        let mut scores = HashMap::<i64, f64>::new();
        let mut st = ic.prepare("SELECT doc_id, tf FROM ask_terms WHERE term = ?1")?;
        for term in &q_terms {
            let posts: Vec<(i64, i64)> = st
                .query_map(params![term], |r| Ok((r.get(0)?, r.get(1)?)))?
                .flatten()
                .filter(|(did, _)| doc_map.contains_key(did))
                .collect();
            if posts.is_empty() {
                continue;
            }
            let df = posts.len() as f64;
            let idf = ((n_docs - df + 0.5) / (df + 0.5) + 1.0).ln();
            for (did, tf) in posts {
                let (dl, tf_f) = (doc_map[&did].1 as f64, tf as f64);
                let norm = (tf_f * 2.2) / (tf_f + 1.2 * (0.25 + 0.75 * (dl / avg_len)));
                *scores.entry(did).or_insert(0.0) += idf * norm;
            }
        }
        let mut hits: Vec<(f64, AskCitation)> = scores
            .into_iter()
            .filter_map(|(did, s)| doc_map.get(&did).map(|(c, _)| (s, (*c).clone())))
            .collect();
        hits.sort_by(|a, b| {
            b.0.total_cmp(&a.0).then_with(|| {
                (a.1.meeting_id, a.1.segment_id).cmp(&(b.1.meeting_id, b.1.segment_id))
            })
        });
        Ok(hits.into_iter().take(TOP_K).map(|(_, c)| c).collect())
    }

    /// Return every indexed segment in `scope` in chronological order.
    pub(crate) fn scope_segments(&self, scope: &AskScope) -> Result<Vec<AskCitation>> {
        let ic = self.conn()?;
        Ok(load_scoped_docs(&ic, scope)?
            .into_iter()
            .map(|(_, c, _)| c)
            .collect())
    }

    /// Return up to `limit` most recent segments in `scope`, ordered chronologically.
    pub(crate) fn recent_scope_segments(
        &self,
        scope: &AskScope,
        limit: usize,
    ) -> Result<Vec<AskCitation>> {
        let ic = self.conn()?;
        let mut cites: Vec<AskCitation> = load_scoped_docs(&ic, scope)?
            .into_iter()
            .map(|(_, c, _)| c)
            .collect();
        cites.sort_by(|a, b| {
            (b.started_at, b.meeting_id, b.start_ms, b.segment_id).cmp(&(
                a.started_at,
                a.meeting_id,
                a.start_ms,
                a.segment_id,
            ))
        });
        cites.truncate(limit);
        cites.reverse();
        Ok(cites)
    }

    fn conn(&self) -> Result<Connection> {
        let c = Connection::open(&self.index_path)?;
        c.busy_timeout(Duration::from_secs(1))?;
        c.execute_batch("PRAGMA secure_delete = ON;")?;
        Ok(c)
    }
}

pub(crate) fn open_ro(path: &Path) -> Option<Connection> {
    let m = fs::symlink_metadata(path).ok()?;
    if m.file_type().is_symlink() || !m.is_file() {
        return None;
    }
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let c = Connection::open_with_flags(path, flags).ok()?;
    let _ = c.busy_timeout(Duration::from_secs(1));
    let _ = c.execute_batch("PRAGMA query_only = ON;");
    Some(c)
}

fn has_table(c: &Connection, name: &str) -> bool {
    let q = "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type = 'table' AND name = ?1";
    c.query_row(q, [name], |r| r.get(0)).unwrap_or(false)
}

fn table_cols(c: &Connection, table: &str) -> Vec<String> {
    let Ok(mut st) = c.prepare(&format!("PRAGMA table_info({table})")) else {
        return Vec::new();
    };
    let rows = st.query_map([], |r| r.get::<_, String>(1)).ok();
    rows.into_iter().flatten().flatten().collect()
}

fn kv_map(ic: &Connection, sql: &str) -> HashMap<i64, String> {
    let Ok(mut st) = ic.prepare(sql) else {
        return HashMap::new();
    };
    let rows = st
        .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))
        .ok();
    rows.into_iter().flatten().flatten().collect()
}

fn purge(tx: &rusqlite::Transaction<'_>, id: i64) -> Result<()> {
    let q_t =
        "DELETE FROM ask_terms WHERE doc_id IN (SELECT doc_id FROM ask_docs WHERE meeting_id = ?1)";
    tx.execute(q_t, [id])?;
    tx.execute("DELETE FROM ask_docs WHERE meeting_id = ?1", [id])?;
    tx.execute("DELETE FROM indexed_meetings WHERE meeting_id = ?1", [id])?;
    Ok(())
}

fn load_scoped_docs(ic: &Connection, scope: &AskScope) -> Result<Vec<(i64, AskCitation, i64)>> {
    let mut st = ic.prepare(
        "SELECT d.doc_id, d.meeting_id, m.title, m.folder, m.started_at, d.segment_id, d.start_ms, d.end_ms, d.channel, d.text, d.token_len
         FROM ask_docs d JOIN indexed_meetings m ON m.meeting_id = d.meeting_id ORDER BY d.meeting_id, d.segment_id",
    )?;
    let mut rows = st.query([])?;
    let mut out = Vec::new();
    while let Some(r) = rows.next()? {
        let (mid, folder): (i64, Option<String>) = (r.get(1)?, r.get(3)?);
        let keep = match scope {
            AskScope::OneMeeting(t) => mid == *t,
            AskScope::AllMeetings => true,
            AskScope::Folder(w) => folder
                .as_deref()
                .is_some_and(|f| f.eq_ignore_ascii_case(w.trim())),
            AskScope::PickedSet(ids) => ids.contains(&mid),
        };
        if keep {
            let s_ms: i64 = r.get(6)?;
            let cite = AskCitation {
                meeting_id: mid,
                meeting_title: r.get(2)?,
                started_at: r.get(4)?,
                segment_id: r.get(5)?,
                start_ms: s_ms,
                end_ms: r.get(7)?,
                timestamp: format_mm_ss(s_ms),
                channel: r.get(8)?,
                quote: r.get(9)?,
            };
            out.push((r.get(0)?, cite, r.get(10)?));
        }
    }
    Ok(out)
}

pub fn format_mm_ss(ms: i64) -> String {
    let s = (ms.max(0) / 1000) as u64;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, (s % 3600) / 60, s % 60)
    } else {
        format!("{:02}:{:02}", s / 60, s % 60)
    }
}

pub(crate) fn tokenize(text: &str) -> Vec<String> {
    text.unicode_words()
        .map(str::to_lowercase)
        .filter(|w| w.len() > 1 && !STOP.split_ascii_whitespace().any(|s| s == w))
        .collect()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn seed_three_meetings(dir: &Path) {
        fs::create_dir_all(dir.join("meetings")).unwrap();
        let mc = Connection::open(dir.join("meetings/meetings.db")).unwrap();
        mc.execute_batch(
            "CREATE TABLE meetings (id INTEGER PRIMARY KEY, started_at INTEGER NOT NULL, ended_at INTEGER, title TEXT NOT NULL, status TEXT NOT NULL, model_id TEXT NOT NULL, saved INTEGER NOT NULL DEFAULT 0);
             CREATE TABLE meeting_segments (id INTEGER PRIMARY KEY, meeting_id INTEGER NOT NULL, start_ms INTEGER NOT NULL, end_ms INTEGER NOT NULL, channel TEXT NOT NULL, text TEXT NOT NULL);
             INSERT INTO meetings VALUES (1, 1700000100, 1700001900, 'Weekly product sync', 'notes_ready', 'whisper-small', 0);
             INSERT INTO meeting_segments VALUES (101, 1, 724000, 730000, 'me', 'Thursday is the homepage launch deadline and Friday is the review');
             INSERT INTO meetings VALUES (2, 1700003600, 1700005400, 'Design critique', 'notes_ready', 'whisper-small', 0);
             INSERT INTO meeting_segments VALUES (202, 2, 980000, 988000, 'others', 'Design critique kept the Thursday deadline unchanged and approved the dark sidebar');
             INSERT INTO meetings VALUES (3, 1700007200, 1700009000, 'Infra retro', 'notes_ready', 'whisper-small', 0);
             INSERT INTO meeting_segments VALUES (303, 3, 255000, 262000, 'me', 'SQLite WAL checkpoint latency dropped under two milliseconds');",
        )
        .unwrap();
    }

    #[test]
    fn index_retrieves_across_all_folder_and_set_and_prunes_deleted_meetings() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        seed_three_meetings(p);
        let idx = MeetingAskIndex::open(p).unwrap();
        idx.set_folder(1, Some("Product")).unwrap();
        idx.set_folder(2, Some("Product")).unwrap();
        idx.set_folder(3, Some("Engineering")).unwrap();
        assert_eq!((idx.sync().unwrap(), idx.sync().unwrap()), (3, 0));
        let q = |s: AskScope, w: &str| idx.retrieve(&s, w).unwrap();
        let all = q(AskScope::AllMeetings, "What is the Thursday deadline?");
        assert!(all.len() == 2 && all.iter().all(|c| c.resolves_in(p)));
        assert_eq!([&*all[0].timestamp, &*all[1].timestamp], ["12:04", "16:20"]);
        let eng = q(AskScope::Folder("engineering".into()), "Thursday WAL");
        assert!(eng.len() == 1 && eng[0].meeting_id == 3);
        let set = q(AskScope::PickedSet(vec![1, 3]), "Thursday deadline");
        assert!(set.len() == 1 && set[0].meeting_id == 1);
        let db = Connection::open(p.join("meetings/meetings.db")).unwrap();
        db.execute("DELETE FROM meetings WHERE id = 2", []).unwrap();
        idx.sync().unwrap();
        assert!(q(AskScope::AllMeetings, "dark sidebar").is_empty());
    }

    #[test]
    fn sync_joins_meeting_folders_skips_trashed_and_indexes_imported_notes_only() {
        let dir = tempfile::tempdir().unwrap();
        let store = crate::managers::meetings::MeetingsStore::open(dir.path()).unwrap();
        let fid = store.create_folder("Customers").unwrap();
        let kept = store.start_meeting("Client kickoff", "turbo").unwrap();
        store
            .insert_segment(
                kept,
                10_000,
                15_000,
                crate::managers::meetings::MeetingChannel::Me,
                "Migration milestone locked for November",
            )
            .unwrap();
        store.finalise(kept, false).unwrap();
        store.move_meetings_to_folder(&[kept], Some(fid)).unwrap();

        let trashed = store.start_meeting("Secret trash", "turbo").unwrap();
        store
            .insert_segment(
                trashed,
                5_000,
                9_000,
                crate::managers::meetings::MeetingChannel::Others,
                "Confidential trashed flamingo phrase",
            )
            .unwrap();
        store.finalise(trashed, false).unwrap();
        crate::meeting_history_ops::trash_meeting_at(&store, trashed, 1_800_000_000).unwrap();

        let imported = store.start_meeting("Imported deck", "imported").unwrap();
        store.set_status(imported, "imported").unwrap();
        store
            .save_notes(
                imported,
                &[crate::managers::meetings::StoredNote {
                    kind: "summary".to_string(),
                    text: "Quarterly telemetry baseline documented offline".to_string(),
                    owner: None,
                    citations: vec![],
                }],
            )
            .unwrap();

        let idx = MeetingAskIndex::open(dir.path()).unwrap();
        idx.sync().unwrap();

        // Folder join via `meeting_folders` works without manual `set_folder`.
        let folder_hits = idx
            .retrieve(&AskScope::Folder("Customers".into()), "November milestone")
            .unwrap();
        assert_eq!(folder_hits.len(), 1);
        assert_eq!(folder_hits[0].meeting_id, kept);

        // Trashed meeting is excluded.
        assert!(idx
            .retrieve(&AskScope::AllMeetings, "flamingo phrase")
            .unwrap()
            .is_empty());

        // Imported notes-only meeting (0 segments) is indexed and resolves cleanly.
        let imp_hits = idx
            .retrieve(&AskScope::AllMeetings, "telemetry baseline")
            .unwrap();
        assert_eq!(imp_hits.len(), 1);
        assert_eq!(imp_hits[0].meeting_id, imported);
        assert!(imp_hits[0].resolves_in(dir.path()));
    }

    #[test]
    fn ask_index_compact_errs_while_a_reader_blocks_the_checkpoint() {
        let dir = tempfile::tempdir().unwrap();
        seed_three_meetings(dir.path());
        let idx = MeetingAskIndex::open(dir.path()).unwrap();
        idx.sync().unwrap();

        let reader = Connection::open(dir.path().join("meetings/ask_index.db")).unwrap();
        reader.execute_batch("BEGIN DEFERRED;").unwrap();
        let _: i64 = reader
            .query_row("SELECT COUNT(*) FROM ask_docs", [], |r| r.get(0))
            .unwrap();

        let mc = Connection::open(dir.path().join("meetings/meetings.db")).unwrap();
        mc.execute("DELETE FROM meeting_segments WHERE meeting_id = 2", [])
            .unwrap();
        mc.execute("DELETE FROM meetings WHERE id = 2", []).unwrap();
        drop(mc);
        idx.sync().unwrap();

        assert!(
            idx.compact_secure().is_err(),
            "compact_secure must report Err while a reader holds the WAL"
        );

        reader.execute_batch("COMMIT;").unwrap();
        drop(reader);

        idx.compact_secure().unwrap();
        let wal = dir.path().join("meetings/ask_index.db-wal");
        let wal_len = fs::metadata(&wal).map(|m| m.len()).unwrap_or(0);
        assert_eq!(wal_len, 0, "ask_index.db-wal must truncate to 0 bytes");
    }

    pub(crate) fn test_pending_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    #[test]
    fn deferred_ask_clean_purges_a_deleted_meeting_on_retry() {
        let _serial = test_pending_lock()
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        seed_three_meetings(dir.path());
        let idx = MeetingAskIndex::open(dir.path()).unwrap();
        idx.sync().unwrap();

        let mc = Connection::open(dir.path().join("meetings/meetings.db")).unwrap();
        mc.execute("DELETE FROM meeting_segments WHERE meeting_id = 2", [])
            .unwrap();
        mc.execute("DELETE FROM meetings WHERE id = 2", []).unwrap();
        drop(mc);

        let locker = Connection::open(dir.path().join("meetings/ask_index.db")).unwrap();
        locker.execute_batch("BEGIN IMMEDIATE;").unwrap();

        sync_or_defer(dir.path(), true, "delete_meeting");
        assert!(any_ask_clean_pending(), "pending flag must be set on lock");
        let rows_before: i64 = locker
            .query_row(
                "SELECT COUNT(*) FROM ask_docs WHERE meeting_id = 2",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(
            rows_before > 0,
            "deleted meeting rows remain until deferred retry runs"
        );

        locker.execute_batch("COMMIT;").unwrap();
        drop(locker);

        assert!(run_pending_ask_clean(dir.path()).unwrap());
        let check = Connection::open(dir.path().join("meetings/ask_index.db")).unwrap();
        let rows_after: i64 = check
            .query_row(
                "SELECT COUNT(*) FROM ask_docs WHERE meeting_id = 2",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(rows_after, 0, "retry must purge deleted meeting rows");
        assert!(!run_pending_ask_clean(dir.path()).unwrap());
        assert!(!any_ask_clean_pending());
    }

    #[test]
    fn sync_or_defer_skips_a_missing_index() {
        let _serial = test_pending_lock()
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        sync_or_defer(dir.path(), true, "delete_meeting");
        assert!(!dir.path().join("meetings/ask_index.db").exists());
        assert!(!any_ask_clean_pending());
    }
}
