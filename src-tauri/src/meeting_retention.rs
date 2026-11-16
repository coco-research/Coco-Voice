//! Meetings retention settings and Delete-all store actions behind `meetings`.

#![cfg(feature = "meetings")]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use chrono::DateTime;
use serde::Serialize;
use specta::Type;
use tauri::AppHandle;

use crate::managers::meetings::{DeleteOutcome, MeetingsStore};
use crate::meetings_list::render_meeting_markdown;
use crate::settings;

static DELETE_ALL_SEQ: AtomicU64 = AtomicU64::new(1);
static PENDING_DELETE_ALL: Mutex<Option<String>> = Mutex::new(None);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Type)]
pub struct MeetingRetentionState {
    pub delete_audio_after_notes: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Type)]
pub struct DeleteAllMeetingsResult {
    pub deleted_count: usize,
    pub audio_left_count: usize,
}

/// Remove `{meetings dir}/<id>` once notes save succeeds and the setting is on.
pub fn maybe_remove_audio_after_notes(
    store: &MeetingsStore,
    meeting_id: i64,
    delete_after_notes: bool,
) {
    if !delete_after_notes {
        return;
    }
    if let Err(err) = store.delete_meeting_audio(meeting_id) {
        log::warn!("meeting {meeting_id}: post-notes audio cleanup failed: {err:#}");
    }
}

/// Delete every meeting through [`MeetingsStore::delete_meeting`].
pub fn delete_all_in_store(store: &MeetingsStore) -> Result<DeleteAllMeetingsResult, String> {
    let rows = store.list_meetings().map_err(|e| e.to_string())?;
    let mut deleted_count = 0usize;
    let mut audio_left_count = 0usize;
    for row in rows {
        match store.delete_meeting(row.id).map_err(|e| e.to_string())? {
            DeleteOutcome::Deleted => {
                deleted_count += 1;
            }
            DeleteOutcome::DeletedAudioLeft { .. } => {
                deleted_count += 1;
                audio_left_count += 1;
            }
        }
    }
    Ok(DeleteAllMeetingsResult {
        deleted_count,
        audio_left_count,
    })
}

#[tauri::command]
#[specta::specta]
pub fn meeting_retention_state(app: AppHandle) -> MeetingRetentionState {
    MeetingRetentionState {
        delete_audio_after_notes: settings::get_settings(&app).delete_audio_after_notes,
    }
}

#[tauri::command]
#[specta::specta]
pub fn change_delete_audio_after_notes_setting(
    app: AppHandle,
    enabled: bool,
) -> Result<MeetingRetentionState, String> {
    settings::update_settings(&app, |s| {
        s.delete_audio_after_notes = enabled;
    });
    Ok(MeetingRetentionState {
        delete_audio_after_notes: enabled,
    })
}

/// Remove audio folders for finished meetings older than `max_age_secs` at `now_ts` (`V-M4`).
pub fn purge_expired_audio_at(
    store: &MeetingsStore,
    max_age_secs: i64,
    now_ts: i64,
) -> Result<usize, String> {
    let meetings_dir = store
        .db_path()
        .parent()
        .ok_or_else(|| "missing meetings dir".to_string())?;
    let mut purged = 0usize;
    for m in store.list_meetings().map_err(|e| e.to_string())? {
        let Some(ended) = m.ended_at else { continue };
        if now_ts.saturating_sub(ended) < max_age_secs.max(0) {
            continue;
        }
        if meetings_dir.join(m.id.to_string()).exists() {
            store
                .delete_meeting_audio(m.id)
                .map_err(|e| e.to_string())?;
            purged += 1;
        }
    }
    Ok(purged)
}

/// Step 1 of two-step Delete all: issue a single-use confirmation token.
pub fn request_delete_all_confirmation(store: &MeetingsStore) -> Result<String, String> {
    let count = store.meeting_count().map_err(|e| e.to_string())?;
    let seq = DELETE_ALL_SEQ.fetch_add(1, Ordering::Relaxed);
    let token = format!("confirm-delete-all-{count}-{seq}");
    *PENDING_DELETE_ALL.lock().unwrap_or_else(|e| e.into_inner()) = Some(token.clone());
    Ok(token)
}

/// Step 2 of two-step Delete all: fails unless `confirmation_token` matches the active token.
pub fn delete_all_with_token(
    store: &MeetingsStore,
    confirmation_token: Option<&str>,
) -> Result<DeleteAllMeetingsResult, String> {
    let supplied = confirmation_token.map(str::trim).filter(|t| !t.is_empty());
    let mut pending = PENDING_DELETE_ALL.lock().unwrap_or_else(|e| e.into_inner());
    let valid = supplied.is_some() && pending.as_deref() == supplied;
    if !valid {
        return Err("Second confirmation token is required to delete all meetings".to_string());
    }
    *pending = None;
    drop(pending);
    delete_all_in_store(store)
}

/// Local-only path inspection for cloud sync folders (`V-M9` / `C11c`: no IO, no network).
pub fn detect_sync_service(folder: &Path) -> Option<&'static str> {
    let s = folder.to_string_lossy();
    if s.contains("Mobile Documents") || s.contains("iCloud") {
        Some("iCloud Drive")
    } else if s.contains("Dropbox") {
        Some("Dropbox")
    } else if s.contains("OneDrive") {
        Some("OneDrive")
    } else if s.contains("Google Drive") || s.contains("GoogleDrive") {
        Some("Google Drive")
    } else {
        None
    }
}

fn slugify_title(title: &str) -> String {
    let mut out = String::new();
    for ch in title.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    let trimmed = out.trim_matches('-');
    if trimmed.is_empty() {
        "meeting".to_string()
    } else {
        trimmed.to_string()
    }
}

/// Write one Markdown file per meeting in `folder` using the shared `#247` renderer (`V-M9`).
pub fn sync_meeting_markdown_file(
    store: &MeetingsStore,
    meeting_id: i64,
    enabled: bool,
    folder: Option<&Path>,
) -> Result<Option<PathBuf>, String> {
    let Some(dir) = folder.filter(|d| enabled && !d.as_os_str().is_empty()) else {
        return Ok(None);
    };
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let meeting = store.meeting(meeting_id).map_err(|e| e.to_string())?;
    let date = DateTime::from_timestamp(meeting.started_at, 0)
        .map(|dt| dt.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| "meeting".to_string());
    let stem = format!("{date}-{}", slugify_title(&meeting.title));
    let owned = store
        .meeting_markdown_file(meeting_id)
        .map_err(|e| e.to_string())?;

    let chosen_name = if let Some(existing) = owned.filter(|n| n.starts_with(&stem)) {
        existing
    } else {
        if let Some(old_name) = store
            .meeting_markdown_file(meeting_id)
            .map_err(|e| e.to_string())?
        {
            let _ = fs::remove_file(dir.join(old_name));
        }
        let mut candidate = format!("{stem}.md");
        let mut idx = 2u32;
        while dir.join(&candidate).exists() {
            candidate = format!("{stem}-{idx}.md");
            idx += 1;
        }
        store
            .set_meeting_markdown_file(meeting_id, Some(&candidate))
            .map_err(|e| e.to_string())?;
        candidate
    };

    let dest = dir.join(&chosen_name);
    let markdown = render_meeting_markdown(store, meeting_id)?;
    fs::write(&dest, markdown).map_err(|e| e.to_string())?;
    Ok(Some(dest))
}

#[tauri::command]
#[specta::specta]
pub fn request_delete_all_meetings(app: AppHandle) -> Result<String, String> {
    let store = MeetingsStore::open_app(&app).map_err(|e| e.to_string())?;
    request_delete_all_confirmation(&store)
}

#[tauri::command]
#[specta::specta]
pub fn delete_all_meetings(
    app: AppHandle,
    confirmation_token: Option<String>,
) -> Result<DeleteAllMeetingsResult, String> {
    let store = MeetingsStore::open_app(&app).map_err(|e| e.to_string())?;
    delete_all_with_token(&store, confirmation_token.as_deref())
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicBool;
    use std::sync::Arc;

    use anyhow::{anyhow, Result};

    use super::*;
    use crate::managers::meetings::{MeetingChannel, StoredNote};
    use crate::meeting_notes_job::{NotesJobRunner, NotesStateKind};
    use crate::meeting_notes_runner::NotesGenerator;

    struct ToggleFake {
        fail: Arc<AtomicBool>,
        seg_id: i64,
    }

    impl NotesGenerator for ToggleFake {
        fn generate(&mut self, _prompt: &str) -> Result<String> {
            if self.fail.load(Ordering::SeqCst) {
                return Err(anyhow!("notes model failed"));
            }
            Ok(format!("SUMMARY: Ready [S{}]", self.seg_id))
        }
    }

    fn seed_meeting_with_audio(
        store: &MeetingsStore,
        root: &std::path::Path,
        title: &str,
    ) -> (i64, i64) {
        let id = store.start_meeting(title, "model").unwrap();
        let seg_id = store
            .insert_segment(id, 0, 1000, MeetingChannel::Me, "We ship Friday")
            .unwrap();
        store.add_notepad_point(id, 500, "ship Friday").unwrap();
        let audio = root.join("meetings").join(id.to_string());
        fs::create_dir_all(&audio).unwrap();
        fs::write(audio.join("mic.wav"), b"mic-bytes").unwrap();
        fs::write(audio.join("system.wav"), b"sys-bytes").unwrap();
        (id, seg_id)
    }

    #[test]
    fn default_setting_is_on() {
        assert!(crate::settings::default_delete_audio_after_notes());
        assert!(crate::settings::get_default_settings().delete_audio_after_notes);
    }

    #[test]
    fn audio_kept_on_notes_failure_and_removed_after_success_when_enabled() {
        let dir = tempfile::tempdir().unwrap();
        let store = MeetingsStore::open(dir.path()).unwrap();
        let (id, seg_id) = seed_meeting_with_audio(&store, dir.path(), "Sync");
        let audio_dir = dir.path().join("meetings").join(id.to_string());

        let fail = Arc::new(AtomicBool::new(true));
        let retain_on = Arc::new(AtomicBool::new(true));
        let root = dir.path().to_path_buf();
        let gen_fail = Arc::clone(&fail);
        let hook_on = Arc::clone(&retain_on);

        let runner = NotesJobRunner::new(
            Arc::new(move || MeetingsStore::open(&root)),
            Arc::new(move || {
                Ok(Box::new(ToggleFake {
                    fail: Arc::clone(&gen_fail),
                    seg_id,
                }))
            }),
            Arc::new(|_| {}),
            Arc::new(move |s, mid| {
                maybe_remove_audio_after_notes(s, mid, hook_on.load(Ordering::SeqCst));
            }),
        );

        runner.trigger(id);
        runner.wait_idle(id);
        assert_eq!(runner.snapshot(id).unwrap().state, NotesStateKind::Failed);
        assert!(
            audio_dir.join("mic.wav").exists(),
            "audio must be kept on notes failure so Retry works"
        );

        fail.store(false, Ordering::SeqCst);
        runner.trigger(id);
        runner.wait_idle(id);
        assert_eq!(runner.snapshot(id).unwrap().state, NotesStateKind::Ready);
        assert!(
            !audio_dir.exists(),
            "audio dir must be removed once notes save succeeds with setting on"
        );
    }

    #[test]
    fn audio_kept_after_notes_success_when_setting_is_off() {
        let dir = tempfile::tempdir().unwrap();
        let store = MeetingsStore::open(dir.path()).unwrap();
        let (id, seg_id) = seed_meeting_with_audio(&store, dir.path(), "Keep audio");
        let audio_dir = dir.path().join("meetings").join(id.to_string());

        let root = dir.path().to_path_buf();
        let runner = NotesJobRunner::new(
            Arc::new(move || MeetingsStore::open(&root)),
            Arc::new(move || {
                Ok(Box::new(ToggleFake {
                    fail: Arc::new(AtomicBool::new(false)),
                    seg_id,
                }))
            }),
            Arc::new(|_| {}),
            Arc::new(|s, mid| maybe_remove_audio_after_notes(s, mid, false)),
        );

        runner.trigger(id);
        runner.wait_idle(id);
        assert_eq!(runner.snapshot(id).unwrap().state, NotesStateKind::Ready);
        assert!(audio_dir.join("mic.wav").exists());
        assert!(audio_dir.join("system.wav").exists());
    }

    #[test]
    fn delete_all_leaves_zero_rows_and_zero_audio_folders() {
        let dir = tempfile::tempdir().unwrap();
        let store = MeetingsStore::open(dir.path()).unwrap();
        let (a, seg_a) = seed_meeting_with_audio(&store, dir.path(), "One");
        let (b, seg_b) = seed_meeting_with_audio(&store, dir.path(), "Two");
        store
            .save_notes(
                a,
                &[StoredNote {
                    kind: "summary".to_string(),
                    text: "One".to_string(),
                    owner: None,
                    citations: vec![seg_a],
                }],
            )
            .unwrap();
        store
            .save_notes(
                b,
                &[StoredNote {
                    kind: "decision".to_string(),
                    text: "Two".to_string(),
                    owner: None,
                    citations: vec![seg_b],
                }],
            )
            .unwrap();

        let res = delete_all_in_store(&store).unwrap();
        assert_eq!(res.deleted_count, 2);
        assert_eq!(res.audio_left_count, 0);
        assert_eq!(store.meeting_count().unwrap(), 0);
        assert!(store.segments(a).unwrap().is_empty());
        assert!(store.notes(a).unwrap().is_empty());
        assert!(store.notepad(a).unwrap().is_empty());
        assert!(store.segments(b).unwrap().is_empty());
        assert!(store.notes(b).unwrap().is_empty());
        assert!(store.notepad(b).unwrap().is_empty());

        let entries: Vec<_> = fs::read_dir(dir.path().join("meetings"))
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_dir())
            .collect();
        assert!(entries.is_empty(), "leftover audio folders: {entries:?}");
    }

    #[test]
    fn fake_clock_retention_two_confirm_delete_all_and_markdown_sync() {
        let dir = tempfile::tempdir().unwrap();
        let store = MeetingsStore::open(dir.path()).unwrap();
        let (old_m, _) = seed_meeting_with_audio(&store, dir.path(), "Old sync");
        let (new_m, seg_new) = seed_meeting_with_audio(&store, dir.path(), "Product sync");
        let conn = rusqlite::Connection::open(store.db_path()).unwrap();
        conn.execute("UPDATE meetings SET started_at = 1700000000, ended_at = 1700001000, status = 'notes_ready' WHERE id = ?1", [old_m]).unwrap();
        conn.execute("UPDATE meetings SET started_at = 1700008000, ended_at = 1700009000, status = 'notes_ready' WHERE id = ?1", [new_m]).unwrap();
        drop(conn);

        assert_eq!(
            purge_expired_audio_at(&store, 3_600, 1700010000).unwrap(),
            1
        );
        assert!(!dir.path().join("meetings").join(old_m.to_string()).exists());
        assert!(dir.path().join("meetings").join(new_m.to_string()).exists());
        assert_eq!(store.segments(old_m).unwrap().len(), 1);

        assert!(delete_all_with_token(&store, None).is_err());
        assert!(delete_all_with_token(&store, Some("forged")).is_err());
        assert_eq!(store.meeting_count().unwrap(), 2);

        store
            .save_notes(
                new_m,
                &[StoredNote {
                    kind: "summary".into(),
                    text: "Ship Friday.".into(),
                    owner: None,
                    citations: vec![seg_new],
                }],
            )
            .unwrap();
        let notes_dir = dir.path().join("notes-folder");
        assert!(
            sync_meeting_markdown_file(&store, new_m, false, Some(&notes_dir))
                .unwrap()
                .is_none()
        );
        fs::create_dir_all(&notes_dir).unwrap();
        let foreign = notes_dir.join("2023-11-15-product-sync.md");
        fs::write(&foreign, "foreign text").unwrap();

        let written = sync_meeting_markdown_file(&store, new_m, true, Some(&notes_dir))
            .unwrap()
            .unwrap();
        assert_eq!(
            written.file_name().unwrap().to_str(),
            Some("2023-11-15-product-sync-2.md")
        );
        assert_eq!(fs::read_to_string(&foreign).unwrap(), "foreign text");
        let export_path = dir.path().join("export.md");
        crate::meetings_list::export_from_store(&store, new_m, &export_path).unwrap();
        assert_eq!(fs::read(&written).unwrap(), fs::read(&export_path).unwrap());
        assert_eq!(
            detect_sync_service(Path::new("/Users/u/Dropbox/Notes")),
            Some("Dropbox")
        );
        assert_eq!(detect_sync_service(&notes_dir), None);

        store.delete_meeting(new_m).unwrap();
        assert!(written.exists());

        let tok = request_delete_all_confirmation(&store).unwrap();
        assert_eq!(
            delete_all_with_token(&store, Some(&tok))
                .unwrap()
                .deleted_count,
            1
        );
        assert!(delete_all_with_token(&store, Some(&tok)).is_err());
    }
}
