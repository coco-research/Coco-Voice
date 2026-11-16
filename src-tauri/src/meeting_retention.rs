//! Meetings retention settings and Delete-all store actions behind `meetings`.

#![cfg(feature = "meetings")]

use serde::Serialize;
use specta::Type;
use tauri::AppHandle;

use crate::managers::meetings::{DeleteOutcome, MeetingsStore};
use crate::settings;

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

#[tauri::command]
#[specta::specta]
pub fn delete_all_meetings(app: AppHandle) -> Result<DeleteAllMeetingsResult, String> {
    let store = MeetingsStore::open_app(&app).map_err(|e| e.to_string())?;
    delete_all_in_store(&store)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::atomic::{AtomicBool, Ordering};
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
}
