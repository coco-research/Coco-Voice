//! Tauri commands and store helpers for the Meetings list view (list, search,
//! Markdown export, and delete). Compiled only when `meetings` is enabled.

use std::fs;
use std::path::Path;

use serde::Serialize;
use specta::Type;
use tauri::AppHandle;

use crate::managers::meetings::{DeleteOutcome, Meeting, MeetingsStore};
use crate::managers::meetings_export::{meeting_markdown, ExportSegment};

/// Summary row rendered in the Meetings list view.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Type)]
pub struct MeetingListItem {
    pub id: i64,
    pub title: String,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub notes_ready: bool,
    pub attendees: Vec<String>,
    pub system_audio_silent: bool,
}

/// Outcome returned to the Meetings list view after deleting a meeting.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Type)]
pub struct MeetingDeleteResult {
    /// True when database rows were deleted but the meeting's audio folder
    /// could not be removed from disk.
    pub audio_left: bool,
}

// ponytail: ceiling = 1 notes query per listed meeting; upgrade path = single LEFT JOIN EXISTS query past ~500 meetings.
fn to_list_item(store: &MeetingsStore, meeting: Meeting) -> Result<MeetingListItem, String> {
    let has_notes = meeting.status == "notes_ready"
        || !store
            .notes(meeting.id)
            .map_err(|e| e.to_string())?
            .is_empty();
    let attendees = store
        .meeting_attendees(meeting.id)
        .map_err(|e| e.to_string())?;
    let system_audio_silent = store
        .system_audio_silent(meeting.id)
        .map_err(|e| e.to_string())?;
    Ok(MeetingListItem {
        id: meeting.id,
        title: meeting.title,
        started_at: meeting.started_at,
        ended_at: meeting.ended_at,
        notes_ready: has_notes,
        attendees,
        system_audio_silent,
    })
}

fn normalize_export_channel(channel: &str) -> String {
    match channel {
        "me" | "mic" => "mic".to_string(),
        "others" | "system" => "system".to_string(),
        other => other.to_string(),
    }
}

pub fn list_from_store(store: &MeetingsStore) -> Result<Vec<MeetingListItem>, String> {
    let meetings = store.list_meetings().map_err(|e| e.to_string())?;
    meetings
        .into_iter()
        .map(|m| to_list_item(store, m))
        .collect()
}

pub fn search_from_store(
    store: &MeetingsStore,
    query: &str,
) -> Result<Vec<MeetingListItem>, String> {
    let meetings = store.search_meetings(query).map_err(|e| e.to_string())?;
    meetings
        .into_iter()
        .map(|m| to_list_item(store, m))
        .collect()
}

pub fn export_from_store(store: &MeetingsStore, id: i64, dest: &Path) -> Result<(), String> {
    if dest.as_os_str().is_empty() {
        return Err("Export destination path is empty".to_string());
    }
    if dest.is_dir() {
        return Err("Export destination must be a file, not a directory".to_string());
    }
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

    let markdown = meeting_markdown(&meeting, &notes, &segments);
    if let Some(parent) = dest.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
    }
    fs::write(dest, markdown).map_err(|e| e.to_string())
}

pub fn delete_from_store(store: &MeetingsStore, id: i64) -> Result<MeetingDeleteResult, String> {
    match store.delete_meeting(id).map_err(|e| e.to_string())? {
        DeleteOutcome::Deleted => Ok(MeetingDeleteResult { audio_left: false }),
        DeleteOutcome::DeletedAudioLeft { .. } => Ok(MeetingDeleteResult { audio_left: true }),
    }
}

#[tauri::command]
#[specta::specta]
pub fn list_meetings(app: AppHandle) -> Result<Vec<MeetingListItem>, String> {
    let store = MeetingsStore::open_app(&app).map_err(|e| e.to_string())?;
    list_from_store(&store)
}

#[tauri::command]
#[specta::specta]
pub fn search_meetings(app: AppHandle, query: String) -> Result<Vec<MeetingListItem>, String> {
    let store = MeetingsStore::open_app(&app).map_err(|e| e.to_string())?;
    search_from_store(&store, &query)
}

#[tauri::command]
#[specta::specta]
pub fn export_meeting_markdown(app: AppHandle, id: i64, dest_path: String) -> Result<(), String> {
    let store = MeetingsStore::open_app(&app).map_err(|e| e.to_string())?;
    export_from_store(&store, id, Path::new(&dest_path))
}

#[tauri::command]
#[specta::specta]
pub fn delete_meeting(app: AppHandle, id: i64) -> Result<MeetingDeleteResult, String> {
    let store = MeetingsStore::open_app(&app).map_err(|e| e.to_string())?;
    delete_from_store(&store, id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::managers::meetings::{MeetingChannel, StoredNote};
    use rusqlite::params;

    fn seed_meeting(
        store: &MeetingsStore,
        title: &str,
        started_at: i64,
        ended_at: Option<i64>,
    ) -> i64 {
        let id = store.start_meeting(title, "whisper-small").unwrap();
        let conn = rusqlite::Connection::open(store.db_path()).unwrap();
        conn.execute(
            "UPDATE meetings SET started_at = ?1, ended_at = ?2, status = 'transcript_ready' WHERE id = ?3",
            params![started_at, ended_at, id],
        )
        .unwrap();
        id
    }

    #[test]
    fn list_orders_newest_first_and_marks_notes_ready() {
        let tmp = tempfile::tempdir().unwrap();
        let store = MeetingsStore::open(tmp.path()).unwrap();

        let older = seed_meeting(&store, "Monday standup", 1_700_000_000, Some(1_700_000_900));
        let newer = seed_meeting(&store, "Product sync", 1_700_003_600, Some(1_700_005_400));
        store
            .save_notes(
                newer,
                &[StoredNote {
                    kind: "summary".to_string(),
                    text: "Locked the homepage scope.".to_string(),
                    owner: None,
                    citations: vec![],
                }],
            )
            .unwrap();

        let rows = list_from_store(&store).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].id, newer);
        assert_eq!(rows[0].title, "Product sync");
        assert!(rows[0].notes_ready);
        assert!(rows[0].attendees.is_empty());
        assert_eq!(rows[1].id, older);
        assert!(!rows[1].notes_ready);

        store
            .set_meeting_attendees(
                newer,
                &[
                    "Sam".to_string(),
                    "Dana".to_string(),
                    "Raj".to_string(),
                    "Mei".to_string(),
                ],
            )
            .unwrap();
        let with_attendees = list_from_store(&store).unwrap();
        assert_eq!(
            with_attendees[0].attendees,
            vec!["Sam", "Dana", "Raj", "Mei"]
        );
        assert!(with_attendees[1].attendees.is_empty());
    }

    #[test]
    fn search_hits_transcript_and_action_items_and_misses_cleanly() {
        let tmp = tempfile::tempdir().unwrap();
        let store = MeetingsStore::open(tmp.path()).unwrap();

        let m1 = seed_meeting(&store, "Standup", 1_700_000_000, Some(1_700_000_600));
        let m2 = seed_meeting(&store, "Planning", 1_700_001_000, Some(1_700_002_800));
        store
            .insert_segment(
                m1,
                0,
                4_000,
                MeetingChannel::Me,
                "Check the lighthouse latency numbers",
            )
            .unwrap();
        store
            .save_notes(
                m2,
                &[StoredNote {
                    kind: "action".to_string(),
                    text: "Draft onboarding checklist".to_string(),
                    owner: Some("Me".to_string()),
                    citations: vec![],
                }],
            )
            .unwrap();

        let hit_seg = search_from_store(&store, "lighthouse").unwrap();
        assert_eq!(hit_seg.len(), 1);
        assert_eq!(hit_seg[0].id, m1);

        let hit_note = search_from_store(&store, "onboarding").unwrap();
        assert_eq!(hit_note.len(), 1);
        assert_eq!(hit_note[0].id, m2);

        let miss = search_from_store(&store, "nonexistent-keyword").unwrap();
        assert!(miss.is_empty());
    }

    #[test]
    fn export_writes_markdown_with_me_and_others_labels_and_delete_clears_all_rows() {
        let tmp = tempfile::tempdir().unwrap();
        let store = MeetingsStore::open(tmp.path()).unwrap();

        let id = seed_meeting(&store, "Product sync", 1_700_000_000, Some(1_700_001_800));
        store
            .insert_segment(
                id,
                5_000,
                9_000,
                MeetingChannel::Me,
                "Ship the homepage this week",
            )
            .unwrap();
        store
            .insert_segment(id, 12_000, 16_000, MeetingChannel::Others, "Sounds good")
            .unwrap();
        store
            .save_notes(
                id,
                &[
                    StoredNote {
                        kind: "summary".to_string(),
                        text: "Homepage scope locked.".to_string(),
                        owner: None,
                        citations: vec![],
                    },
                    StoredNote {
                        kind: "action".to_string(),
                        text: "Review copy on Friday".to_string(),
                        owner: Some("Me".to_string()),
                        citations: vec![],
                    },
                ],
            )
            .unwrap();
        store.add_notepad_point(id, 3_000, "homepage").unwrap();

        let audio_dir = tmp.path().join("meetings").join(id.to_string());
        fs::create_dir_all(&audio_dir).unwrap();
        fs::write(audio_dir.join("mic.wav"), b"stub").unwrap();

        let out_md = tmp.path().join("exported").join("product-sync.md");
        export_from_store(&store, id, &out_md).unwrap();
        let text = fs::read_to_string(&out_md).unwrap();
        assert!(text.contains("# Product sync"), "{text}");
        assert!(text.contains("Homepage scope locked."), "{text}");
        assert!(
            text.contains("- Review copy on Friday (owner: Me)"),
            "{text}"
        );
        assert!(
            text.contains("[00:05] Me: Ship the homepage this week"),
            "{text}"
        );
        assert!(text.contains("[00:12] Others: Sounds good"), "{text}");

        let del = delete_from_store(&store, id).unwrap();
        assert!(!del.audio_left);
        assert!(list_from_store(&store).unwrap().is_empty());
        assert!(!audio_dir.exists());
    }

    #[cfg(unix)]
    #[test]
    fn delete_reports_audio_left_when_audio_folder_cannot_be_removed() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = tempfile::tempdir().unwrap();
        let store = MeetingsStore::open(tmp.path()).unwrap();
        let id = seed_meeting(&store, "Locked audio", 1_700_000_000, Some(1_700_000_600));
        let meetings_dir = tmp.path().join("meetings");
        let audio_dir = meetings_dir.join(id.to_string());
        fs::create_dir_all(&audio_dir).unwrap();
        fs::write(audio_dir.join("mic.wav"), b"stub").unwrap();
        fs::set_permissions(&audio_dir, fs::Permissions::from_mode(0o555)).unwrap();

        let del = delete_from_store(&store, id);
        fs::set_permissions(&audio_dir, fs::Permissions::from_mode(0o755)).unwrap();

        let del = del.unwrap();
        assert!(del.audio_left);
        assert!(list_from_store(&store).unwrap().is_empty());
        assert!(audio_dir.exists());
    }
}
