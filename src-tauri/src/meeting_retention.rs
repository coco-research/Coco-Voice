//! Meetings retention settings, two-step Delete all, and per-meeting Markdown folder sync.

#![cfg(feature = "meetings")]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use chrono::DateTime;
use serde::Serialize;
use specta::Type;
use tauri::AppHandle;

use crate::managers::meetings::{DeleteOutcome, MeetingsStore, StoredNote};
use crate::managers::meetings_export::{meeting_markdown, ExportSegment};
use crate::settings;

static DELETE_ALL_SEQ: AtomicU64 = AtomicU64::new(1);
static PENDING_DELETE_ALL: Mutex<Option<String>> = Mutex::new(None);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Type)]
pub struct MeetingRetentionState {
    pub delete_audio_after_notes: bool,
    pub meeting_audio_retention_secs: Option<i64>,
    pub save_notes_as_markdown: bool,
    pub notes_markdown_folder: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Type)]
pub struct DeleteAllMeetingsResult {
    pub deleted_count: usize,
    pub audio_left_count: usize,
}

fn normalize_export_channel(channel: &str) -> String {
    match channel {
        "me" => "mic".to_string(),
        "others" => "system".to_string(),
        other => other.to_string(),
    }
}

/// Render `id` using the shared Markdown formatter so app export and folder sync stay identical.
pub fn render_meeting_markdown(store: &MeetingsStore, id: i64) -> Result<String, String> {
    let meeting = store.meeting(id).map_err(|e| e.to_string())?;
    let notes = store.notes(id).map_err(|e| e.to_string())?;
    let segments: Vec<ExportSegment> = store
        .segments(id)
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|seg| ExportSegment {
            id: seg.id,
            start_ms: seg.start_ms,
            channel: normalize_export_channel(&seg.channel),
            text: seg.text,
        })
        .collect();
    Ok(meeting_markdown(&meeting, &notes, &segments))
}

/// Save `notes` and remove `{meetings dir}/<id>` only once notes save succeeds.
pub fn save_notes_with_retention(
    store: &MeetingsStore,
    meeting_id: i64,
    notes: &[StoredNote],
    delete_after_notes: bool,
) -> Result<(), String> {
    store
        .save_notes(meeting_id, notes)
        .map_err(|e| e.to_string())?;
    maybe_remove_audio_after_notes(store, meeting_id, delete_after_notes);
    Ok(())
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

/// Remove audio folders for finished meetings older than `max_age_secs` at `now_ts`.
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

/// Local-only path inspection for cloud sync folders (no IO, no network).
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

const MAX_MARKDOWN_FILENAME_CHARS: usize = 80;

fn slugify_title(title: &str) -> String {
    let mut out = String::new();
    for ch in title.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    let trimmed = out.trim_matches(['-', '.']);
    if trimmed.is_empty() {
        "meeting".to_string()
    } else {
        trimmed.to_string()
    }
}

/// Sanitize a meeting date + title into a safe single filename stem (no `/`, `\`, `:`,
/// leading `.`, or `..`) capped so `<stem>.md` stays at most 80 characters.
pub fn safe_markdown_stem(date: &str, title: &str) -> String {
    let clean_date: String = date
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect();
    let date_prefix = if clean_date.is_empty() {
        "meeting".to_string()
    } else {
        clean_date
    };
    let slug = slugify_title(title);
    // Reserve room for `-{idx}.md` suffix (up to 10 chars) while keeping filename <= 80 chars.
    let max_stem = MAX_MARKDOWN_FILENAME_CHARS.saturating_sub(10);
    let combined = format!("{date_prefix}-{slug}");
    let truncated: String = combined
        .chars()
        .take(max_stem)
        .collect::<String>()
        .trim_matches(['-', '.'])
        .to_string();
    if truncated.is_empty() {
        "meeting".to_string()
    } else {
        truncated
    }
}

fn ensure_safe_file_name(name: &str) -> Result<(), String> {
    if name.is_empty()
        || name.starts_with('.')
        || name.contains('/')
        || name.contains('\\')
        || name.contains(':')
        || name.contains("..")
        || name.chars().count() > MAX_MARKDOWN_FILENAME_CHARS
    {
        return Err("Refusing unsafe Markdown filename".to_string());
    }
    Ok(())
}

/// Resolve `dir` canonically and verify `target` stays strictly inside it,
/// refusing any symlink escape or path traversal.
fn verified_destination_inside_folder(
    canon_dir: &Path,
    file_name: &str,
) -> Result<PathBuf, String> {
    ensure_safe_file_name(file_name)?;
    let dest = canon_dir.join(file_name);
    let parent = dest
        .parent()
        .ok_or_else(|| "Missing Markdown destination parent".to_string())?;
    let canon_parent = fs::canonicalize(parent).map_err(|e| e.to_string())?;
    if canon_parent != canon_dir {
        return Err("Refusing to write meeting notes outside the chosen folder".to_string());
    }
    if let Ok(meta) = fs::symlink_metadata(&dest) {
        if meta.file_type().is_symlink() {
            return Err("Refusing to follow symlink in meeting notes folder".to_string());
        }
        let canon_dest = fs::canonicalize(&dest).map_err(|e| e.to_string())?;
        if !canon_dest.starts_with(canon_dir) {
            return Err("Refusing to write meeting notes outside the chosen folder".to_string());
        }
    }
    Ok(dest)
}

/// Write one Markdown file per meeting in `folder` using the shared Markdown renderer.
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
    let canon_dir = fs::canonicalize(dir).map_err(|e| e.to_string())?;
    if !canon_dir.is_dir() {
        return Err("Meeting notes destination is not a directory".to_string());
    }

    let meeting = store.meeting(meeting_id).map_err(|e| e.to_string())?;
    let date = DateTime::from_timestamp(meeting.started_at, 0)
        .map(|dt| dt.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| "meeting".to_string());
    let stem = safe_markdown_stem(&date, &meeting.title);
    let owned = store
        .meeting_markdown_file(meeting_id)
        .map_err(|e| e.to_string())?;

    let chosen_name = if let Some(existing) = owned
        .clone()
        .filter(|n| n.starts_with(&stem) && ensure_safe_file_name(n).is_ok())
    {
        existing
    } else {
        if let Some(old_name) = owned {
            if let Ok(old_dest) = verified_destination_inside_folder(&canon_dir, &old_name) {
                let _ = fs::remove_file(old_dest);
            }
        }
        let mut candidate = format!("{stem}.md");
        let mut idx = 2u32;
        while canon_dir.join(&candidate).exists()
            || fs::symlink_metadata(canon_dir.join(&candidate)).is_ok()
        {
            candidate = format!("{stem}-{idx}.md");
            idx += 1;
        }
        ensure_safe_file_name(&candidate)?;
        store
            .set_meeting_markdown_file(meeting_id, Some(&candidate))
            .map_err(|e| e.to_string())?;
        candidate
    };

    let dest = verified_destination_inside_folder(&canon_dir, &chosen_name)?;
    let markdown = render_meeting_markdown(store, meeting_id)?;
    fs::write(&dest, markdown).map_err(|e| e.to_string())?;
    let canon_written = fs::canonicalize(&dest).map_err(|e| e.to_string())?;
    if !canon_written.starts_with(&canon_dir) {
        let _ = fs::remove_file(&dest);
        return Err("Refusing to write meeting notes outside the chosen folder".to_string());
    }
    Ok(Some(canon_written))
}

fn state_from_settings(s: settings::AppSettings) -> MeetingRetentionState {
    MeetingRetentionState {
        delete_audio_after_notes: s.delete_audio_after_notes,
        meeting_audio_retention_secs: s.meeting_audio_retention_secs,
        save_notes_as_markdown: s.save_notes_as_markdown,
        notes_markdown_folder: s.notes_markdown_folder,
    }
}

#[tauri::command]
#[specta::specta]
pub fn meeting_retention_state(app: AppHandle) -> MeetingRetentionState {
    state_from_settings(settings::get_settings(&app))
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
    Ok(state_from_settings(settings::get_settings(&app)))
}

/// Validates a user-selected Markdown notes folder before persisting it.
/// Rejects NUL bytes, non-directory existing paths, and dangling symlinks.
pub(crate) fn validate_markdown_folder_path(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("Choose a folder first.".to_string());
    }
    if trimmed.contains('\0') {
        return Err("Folder path contains an invalid character.".to_string());
    }
    let path = Path::new(trimmed);
    if let Ok(meta) = fs::symlink_metadata(path) {
        if meta.file_type().is_symlink() {
            match fs::metadata(path) {
                Ok(target_meta) if target_meta.is_dir() => {}
                _ => return Err("Selected symlink does not point to a valid folder.".to_string()),
            }
        } else if !meta.is_dir() {
            return Err("Selected path is a file, not a folder.".to_string());
        }
    }
    Ok(trimmed.to_string())
}

#[tauri::command]
#[specta::specta]
pub fn change_notes_markdown_folder_setting(
    app: AppHandle,
    enabled: bool,
    folder: Option<String>,
) -> Result<MeetingRetentionState, String> {
    let cleaned = match folder
        .map(|f| f.trim().to_string())
        .filter(|f| !f.is_empty())
    {
        Some(path) if enabled => Some(validate_markdown_folder_path(&path)?),
        other => other,
    };
    settings::update_settings(&app, |s| {
        s.save_notes_as_markdown = enabled && cleaned.is_some();
        s.notes_markdown_folder = cleaned;
    });
    Ok(state_from_settings(settings::get_settings(&app)))
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
    use super::*;
    use crate::managers::meetings::StoredNote;

    fn seed_meeting_with_audio(
        store: &MeetingsStore,
        root: &Path,
        title: &str,
    ) -> (i64, i64, PathBuf) {
        let id = store.start_meeting(title, "model").unwrap();
        let seg_id = store
            .insert_segment(
                id,
                5_000,
                9_000,
                crate::managers::meetings::MeetingChannel::Me,
                "Ship the homepage on Friday",
            )
            .unwrap();
        store.add_notepad_point(id, 5_500, "ship Friday").unwrap();
        let audio = root.join("meetings").join(id.to_string());
        fs::create_dir_all(&audio).unwrap();
        fs::write(audio.join("mic.wav"), b"mic-bytes").unwrap();
        fs::write(audio.join("system.wav"), b"sys-bytes").unwrap();
        (id, seg_id, audio)
    }

    #[test]
    fn default_retention_and_markdown_folder_settings_are_safe() {
        let defaults = crate::settings::get_default_settings();
        assert!(crate::settings::default_delete_audio_after_notes());
        assert!(defaults.delete_audio_after_notes);
        assert!(!defaults.save_notes_as_markdown);
        assert_eq!(defaults.notes_markdown_folder, None);
        assert_eq!(defaults.meeting_audio_retention_secs, None);
    }

    #[test]
    fn audio_kept_when_disabled_and_removed_only_after_notes_save_when_enabled() {
        let dir = tempfile::tempdir().unwrap();
        let store = MeetingsStore::open(dir.path()).unwrap();
        let (id, seg_id, audio_dir) = seed_meeting_with_audio(&store, dir.path(), "Sync");

        // When delete_after_notes is false, saving notes keeps the audio directory.
        save_notes_with_retention(
            &store,
            id,
            &[StoredNote {
                kind: "summary".to_string(),
                text: "Draft ready".to_string(),
                owner: None,
                citations: vec![seg_id],
            }],
            false,
        )
        .unwrap();
        assert!(audio_dir.join("mic.wav").exists());
        assert!(audio_dir.join("system.wav").exists());

        // When delete_after_notes is true, saving notes removes the audio directory while DB rows stay.
        save_notes_with_retention(
            &store,
            id,
            &[StoredNote {
                kind: "summary".to_string(),
                text: "Final ready".to_string(),
                owner: None,
                citations: vec![seg_id],
            }],
            true,
        )
        .unwrap();
        assert!(!audio_dir.exists());
        assert_eq!(store.notes(id).unwrap().len(), 1);
        assert_eq!(store.segments(id).unwrap().len(), 1);
    }

    #[test]
    fn fake_clock_retention_two_step_delete_all_and_markdown_folder_sync() {
        let _lock = crate::managers::meetings::test_lock();
        let dir = tempfile::tempdir().unwrap();
        let store = MeetingsStore::open(dir.path()).unwrap();
        let (old_m, _, old_audio) = seed_meeting_with_audio(&store, dir.path(), "Old sync");
        let (new_m, seg_new, new_audio) =
            seed_meeting_with_audio(&store, dir.path(), "Product sync");

        let conn = rusqlite::Connection::open(store.db_path()).unwrap();
        conn.execute(
            "UPDATE meetings SET started_at = 1700000000, ended_at = 1700001000, status = 'notes_ready' WHERE id = ?1",
            [old_m],
        )
        .unwrap();
        conn.execute(
            "UPDATE meetings SET started_at = 1700008000, ended_at = 1700009000, status = 'notes_ready' WHERE id = ?1",
            [new_m],
        )
        .unwrap();
        drop(conn);

        assert_eq!(
            purge_expired_audio_at(&store, 3_600, 1_700_010_000).unwrap(),
            1
        );
        assert!(!old_audio.exists());
        assert!(new_audio.exists());
        assert_eq!(store.segments(old_m).unwrap().len(), 1);

        // Delete all fails without a valid second confirmation token.
        assert!(delete_all_with_token(&store, None).is_err());
        assert!(delete_all_with_token(&store, Some("forged")).is_err());
        assert_eq!(store.meeting_count().unwrap(), 2);

        store
            .save_notes(
                new_m,
                &[StoredNote {
                    kind: "summary".to_string(),
                    text: "Ship Friday.".to_string(),
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
        assert_eq!(
            fs::read(&written).unwrap(),
            render_meeting_markdown(&store, new_m).unwrap().as_bytes()
        );
        assert_eq!(
            detect_sync_service(Path::new("/srv/Dropbox/Notes")),
            Some("Dropbox")
        );
        assert_eq!(
            detect_sync_service(Path::new("/srv/Library/Mobile Documents/Notes")),
            Some("iCloud Drive")
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
        assert_eq!(store.meeting_count().unwrap(), 0);
        assert!(delete_all_with_token(&store, Some(&tok)).is_err());
    }

    #[test]
    fn validate_markdown_folder_path_rejects_empty_nul_and_non_directory() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("not-a-folder.md");
        fs::write(&file, "x").unwrap();

        assert!(validate_markdown_folder_path("").is_err());
        assert!(validate_markdown_folder_path("   ").is_err());
        assert!(validate_markdown_folder_path("/tmp/bad\0path").is_err());
        assert!(validate_markdown_folder_path(file.to_str().unwrap()).is_err());
        assert_eq!(
            validate_markdown_folder_path(dir.path().to_str().unwrap()).unwrap(),
            dir.path().to_str().unwrap()
        );
    }

    #[test]
    fn markdown_filename_sanitizes_untrusted_titles_and_blocks_symlink_escape() {
        let dir = tempfile::tempdir().unwrap();
        let store = MeetingsStore::open(dir.path()).unwrap();
        let notes_dir = dir.path().join("notes-folder");
        fs::create_dir_all(&notes_dir).unwrap();
        let canon_notes_dir = fs::canonicalize(&notes_dir).unwrap();

        let long_title = "a".repeat(300);
        for bad_title in [
            "../../etc/passwd",
            "a/b\\c:d",
            ".hidden",
            long_title.as_str(),
        ] {
            let (mid, seg_id, _) = seed_meeting_with_audio(&store, dir.path(), bad_title);
            store
                .save_notes(
                    mid,
                    &[StoredNote {
                        kind: "summary".to_string(),
                        text: "Keep file inside folder.".to_string(),
                        owner: None,
                        citations: vec![seg_id],
                    }],
                )
                .unwrap();
            let written = sync_meeting_markdown_file(&store, mid, true, Some(&notes_dir))
                .unwrap()
                .unwrap();
            assert!(written.starts_with(&canon_notes_dir));
            let fname = written.file_name().unwrap().to_str().unwrap();
            assert!(fname.chars().count() <= MAX_MARKDOWN_FILENAME_CHARS);
            assert!(!fname.starts_with('.'));
            assert!(!fname.contains('/') && !fname.contains('\\') && !fname.contains(':'));
            let content = fs::read_to_string(&written).unwrap();
            assert!(content.contains("Keep file inside folder."));
            assert!(!content.contains("mic-bytes") && !content.contains("sys-bytes"));
        }

        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            let outside = dir.path().join("outside.md");
            fs::write(&outside, "do-not-overwrite").unwrap();
            let (mid, seg_id, _) = seed_meeting_with_audio(&store, dir.path(), "Escape check");
            let conn = rusqlite::Connection::open(store.db_path()).unwrap();
            conn.execute(
                "UPDATE meetings SET started_at = 1700008000 WHERE id = ?1",
                [mid],
            )
            .unwrap();
            drop(conn);
            store
                .save_notes(
                    mid,
                    &[StoredNote {
                        kind: "summary".to_string(),
                        text: "Blocked write.".to_string(),
                        owner: None,
                        citations: vec![seg_id],
                    }],
                )
                .unwrap();
            let link_name = "2023-11-15-escape-check.md";
            symlink(&outside, notes_dir.join(link_name)).unwrap();
            store
                .set_meeting_markdown_file(mid, Some(link_name))
                .unwrap();
            let err = sync_meeting_markdown_file(&store, mid, true, Some(&notes_dir)).unwrap_err();
            assert!(err.contains("symlink") || err.contains("outside"));
            assert_eq!(fs::read_to_string(&outside).unwrap(), "do-not-overwrite");
        }
    }
}
