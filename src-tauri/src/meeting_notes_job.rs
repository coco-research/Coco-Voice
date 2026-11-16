//! Background notes runner triggered after Stop, plus Tauri commands for the
//! post-stop notes view, notepad persistence, and Retry.

#![cfg(feature = "meetings")]

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
use serde::Serialize;
use specta::Type;
use tauri::{AppHandle, Emitter, Manager};

use crate::managers::meetings::{MeetingsStore, StoredNote};
use crate::managers::model::ModelManager;
use crate::meeting_controller::NotesTrigger;
use crate::meeting_notes_runner::{
    generate_meeting_notes, NotesGenerator, NotesProgress, NOTEPAD_KIND,
};

pub const NOTES_STATE_EVENT: &str = "meeting-notes-state";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum NotesStateKind {
    Idle,
    Running,
    Ready,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Type)]
pub struct MeetingNoteLineView {
    pub kind: String,
    pub text: String,
    pub owner: Option<String>,
    pub start_ms: Option<i64>,
    pub from_notepad: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Type)]
pub struct MeetingNotesSnapshot {
    pub meeting_id: i64,
    pub state: NotesStateKind,
    pub progress_label: Option<String>,
    pub elapsed_secs: u64,
    pub notepad_text: String,
    pub lines: Vec<MeetingNoteLineView>,
    pub oversized_count: usize,
}

type StoreFn = Arc<dyn Fn() -> Result<MeetingsStore> + Send + Sync>;
type GenFn = Arc<dyn Fn() -> Result<Box<dyn NotesGenerator + Send>> + Send + Sync>;
type EmitFn = Arc<dyn Fn(MeetingNotesSnapshot) + Send + Sync>;
type SavedHook = Arc<dyn Fn(&MeetingsStore, i64) + Send + Sync>;

struct BoxedGen(Box<dyn NotesGenerator + Send>);

impl NotesGenerator for BoxedGen {
    fn generate(&mut self, prompt: &str) -> Result<String> {
        self.0.generate(prompt)
    }
}

fn poison<T>(err: PoisonError<T>) -> T {
    err.into_inner()
}

fn norm(text: &str) -> String {
    text.to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn build_lines(
    store: &MeetingsStore,
    meeting_id: i64,
    notes: &[StoredNote],
    notepad_points: &[String],
) -> Vec<MeetingNoteLineView> {
    let seg_times: HashMap<i64, i64> = store
        .segments(meeting_id)
        .unwrap_or_default()
        .into_iter()
        .map(|s| (s.id, s.start_ms))
        .collect();
    let needles: Vec<String> = notepad_points
        .iter()
        .map(|p| norm(p))
        .filter(|n| !n.is_empty())
        .collect();
    notes
        .iter()
        .map(|n| {
            let norm_text = norm(&n.text);
            let from_notepad =
                n.kind == NOTEPAD_KIND || needles.iter().any(|needle| norm_text.contains(needle));
            let start_ms = n
                .citations
                .first()
                .and_then(|seg_id| seg_times.get(seg_id).copied());
            MeetingNoteLineView {
                kind: n.kind.clone(),
                text: n.text.clone(),
                owner: n.owner.clone(),
                start_ms,
                from_notepad,
            }
        })
        .collect()
}

fn read_notepad(store: &MeetingsStore, meeting_id: i64) -> (Vec<String>, String) {
    let points: Vec<String> = store
        .notepad(meeting_id)
        .unwrap_or_default()
        .into_iter()
        .map(|(_, t)| t)
        .collect();
    let joined = points.join("\n");
    (points, joined)
}

// ponytail: ceiling = 1 background worker thread per stopped meeting; upgrade path = bounded work queue if batch runs land.
pub struct NotesJobRunner {
    store_fn: StoreFn,
    gen_fn: GenFn,
    emit_fn: EmitFn,
    on_saved: SavedHook,
    in_flight: (Mutex<HashSet<i64>>, Condvar),
    snapshots: Mutex<HashMap<i64, MeetingNotesSnapshot>>,
}

impl NotesJobRunner {
    pub fn new(
        store_fn: StoreFn,
        gen_fn: GenFn,
        emit_fn: EmitFn,
        on_saved: SavedHook,
    ) -> Arc<Self> {
        Arc::new(Self {
            store_fn,
            gen_fn,
            emit_fn,
            on_saved,
            in_flight: (Mutex::new(HashSet::new()), Condvar::new()),
            snapshots: Mutex::new(HashMap::new()),
        })
    }

    pub fn for_app(app: AppHandle) -> Arc<Self> {
        let store_app = app.clone();
        let gen_app = app.clone();
        let emit_app = app.clone();
        let retain_app = app;
        Self::new(
            Arc::new(move || MeetingsStore::open_app(&store_app)),
            Arc::new(move || app_notes_generator(&gen_app)),
            Arc::new(move |snap| {
                let _ = emit_app.emit(NOTES_STATE_EVENT, snap);
            }),
            Arc::new(move |store, meeting_id| {
                let cfg = crate::settings::get_settings(&retain_app);
                crate::meeting_retention::maybe_remove_audio_after_notes(
                    store,
                    meeting_id,
                    cfg.delete_audio_after_notes,
                );
                let folder = cfg
                    .notes_markdown_folder
                    .as_deref()
                    .map(std::path::Path::new);
                let _ = crate::meeting_retention::sync_meeting_markdown_file(
                    store,
                    meeting_id,
                    cfg.save_notes_as_markdown,
                    folder,
                );
            }),
        )
    }

    pub fn trigger(self: &Arc<Self>, meeting_id: i64) {
        {
            let mut active = self.in_flight.0.lock().unwrap_or_else(poison);
            if !active.insert(meeting_id) {
                return;
            }
        }
        let runner = Arc::clone(self);
        std::thread::spawn(move || {
            runner.execute(meeting_id);
            let mut active = runner.in_flight.0.lock().unwrap_or_else(poison);
            active.remove(&meeting_id);
            runner.in_flight.1.notify_all();
        });
    }

    #[cfg(test)]
    pub fn wait_idle(&self, meeting_id: i64) {
        let mut active = self.in_flight.0.lock().unwrap_or_else(poison);
        while active.contains(&meeting_id) {
            active = self.in_flight.1.wait(active).unwrap_or_else(poison);
        }
    }

    pub fn snapshot(&self, meeting_id: i64) -> Result<MeetingNotesSnapshot> {
        if let Some(cached) = self
            .snapshots
            .lock()
            .unwrap_or_else(poison)
            .get(&meeting_id)
            .cloned()
        {
            return Ok(cached);
        }
        let store = (self.store_fn)()?;
        let (points, notepad_text) = read_notepad(&store, meeting_id);
        let notes = store.notes(meeting_id)?;
        let state = if notes.is_empty() {
            NotesStateKind::Idle
        } else {
            NotesStateKind::Ready
        };
        let lines = build_lines(&store, meeting_id, &notes, &points);
        Ok(MeetingNotesSnapshot {
            meeting_id,
            state,
            progress_label: None,
            elapsed_secs: 0,
            notepad_text,
            lines,
            oversized_count: 0,
        })
    }

    fn publish(&self, snap: MeetingNotesSnapshot) {
        self.snapshots
            .lock()
            .unwrap_or_else(poison)
            .insert(snap.meeting_id, snap.clone());
        (self.emit_fn)(snap);
    }

    fn execute(self: &Arc<Self>, meeting_id: i64) {
        let store = match (self.store_fn)() {
            Ok(store) => store,
            Err(err) => {
                log::warn!("meeting {meeting_id} notes store open failed: {err:#}");
                self.publish(MeetingNotesSnapshot {
                    meeting_id,
                    state: NotesStateKind::Failed,
                    progress_label: None,
                    elapsed_secs: 0,
                    notepad_text: String::new(),
                    lines: Vec::new(),
                    oversized_count: 0,
                });
                return;
            }
        };
        let (points, notepad_text) = read_notepad(&store, meeting_id);
        let started = Instant::now();
        let initial = NotesProgress {
            calls_done: 0,
            calls_total: 1,
            elapsed_secs: 0,
        };
        self.publish(MeetingNotesSnapshot {
            meeting_id,
            state: NotesStateKind::Running,
            progress_label: Some(initial.label()),
            elapsed_secs: 0,
            notepad_text: notepad_text.clone(),
            lines: Vec::new(),
            oversized_count: 0,
        });

        let done_flag = Arc::new(AtomicBool::new(false));
        let tick_done = Arc::clone(&done_flag);
        let tick_runner = Arc::clone(self);
        let tick_pad = notepad_text.clone();
        let ticker = std::thread::spawn(move || {
            while !tick_done.load(Ordering::Acquire) {
                std::thread::sleep(Duration::from_millis(250));
                if tick_done.load(Ordering::Acquire) {
                    break;
                }
                let secs = started.elapsed().as_secs();
                let prog = NotesProgress {
                    calls_done: 0,
                    calls_total: 1,
                    elapsed_secs: secs,
                };
                tick_runner.publish(MeetingNotesSnapshot {
                    meeting_id,
                    state: NotesStateKind::Running,
                    progress_label: Some(prog.label()),
                    elapsed_secs: secs,
                    notepad_text: tick_pad.clone(),
                    lines: Vec::new(),
                    oversized_count: 0,
                });
            }
        });

        let outcome = (self.gen_fn)().and_then(|inner| {
            let mut boxed = BoxedGen(inner);
            generate_meeting_notes(&store, meeting_id, &mut boxed, &mut |prog| {
                self.publish(MeetingNotesSnapshot {
                    meeting_id,
                    state: NotesStateKind::Running,
                    progress_label: Some(prog.label()),
                    elapsed_secs: prog.elapsed_secs,
                    notepad_text: notepad_text.clone(),
                    lines: Vec::new(),
                    oversized_count: 0,
                });
            })
        });

        done_flag.store(true, Ordering::Release);
        let _ = ticker.join();

        match outcome {
            Ok(out) => {
                if let Err(err) = store.mark_notes_ready(meeting_id) {
                    log::warn!("meeting {meeting_id} status update failed: {err:#}");
                }
                (self.on_saved)(&store, meeting_id);
                let lines = build_lines(&store, meeting_id, &out.notes, &points);
                self.publish(MeetingNotesSnapshot {
                    meeting_id,
                    state: NotesStateKind::Ready,
                    progress_label: None,
                    elapsed_secs: started.elapsed().as_secs(),
                    notepad_text,
                    lines,
                    oversized_count: out.oversized_segments.len(),
                });
            }
            Err(err) => {
                log::warn!("meeting {meeting_id} notes generation failed: {err:#}");
                self.publish(MeetingNotesSnapshot {
                    meeting_id,
                    state: NotesStateKind::Failed,
                    progress_label: None,
                    elapsed_secs: started.elapsed().as_secs(),
                    notepad_text,
                    lines: Vec::new(),
                    oversized_count: 0,
                });
            }
        }
    }
}

impl NotesTrigger for NotesJobRunner {
    fn run_after_stop(&self, _meeting_id: i64) {
        // Arc-backed trigger wrapper below handles spawned lifetime.
    }
}

pub struct SharedNotesTrigger(pub Arc<NotesJobRunner>);

impl NotesTrigger for SharedNotesTrigger {
    fn run_after_stop(&self, meeting_id: i64) {
        self.0.trigger(meeting_id);
    }
}

fn app_notes_generator(app: &AppHandle) -> Result<Box<dyn NotesGenerator + Send>> {
    #[cfg(target_os = "macos")]
    {
        let settings = crate::settings::get_settings(app);
        let model_id = settings
            .post_process_models
            .get(crate::settings::LOCAL_LLM_PROVIDER_ID)
            .filter(|id| !id.trim().is_empty())
            .cloned()
            .unwrap_or_else(|| crate::settings::LOCAL_LLM_DEFAULT_MODEL_ID.to_string());
        let models = app
            .try_state::<Arc<ModelManager>>()
            .ok_or_else(|| anyhow!("model manager unavailable"))?;
        let model_path = models.get_model_path(&model_id)?;
        Ok(Box::new(crate::meeting_notes_runner::LocalLlmNotes {
            model_path,
        }))
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = app;
        Err(anyhow!("local notes model unavailable on this platform"))
    }
}

fn runner(app: &AppHandle) -> Arc<NotesJobRunner> {
    Arc::clone(app.state::<Arc<NotesJobRunner>>().inner())
}

#[tauri::command]
#[specta::specta]
pub fn meeting_notes_snapshot(
    app: AppHandle,
    meeting_id: i64,
) -> Result<MeetingNotesSnapshot, String> {
    runner(&app).snapshot(meeting_id).map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub fn save_meeting_notepad(app: AppHandle, meeting_id: i64, text: String) -> Result<(), String> {
    let store = MeetingsStore::open_app(&app).map_err(|e| e.to_string())?;
    let lines: Vec<String> = text.lines().map(str::to_string).collect();
    store
        .replace_notepad(meeting_id, &lines)
        .map_err(|e| e.to_string())?;
    let state = runner(&app);
    let mut cache = state.snapshots.lock().unwrap_or_else(poison);
    if let Some(entry) = cache.get_mut(&meeting_id) {
        entry.notepad_text = lines
            .into_iter()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
    }
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn retry_meeting_notes(
    app: AppHandle,
    meeting_id: i64,
) -> Result<MeetingNotesSnapshot, String> {
    let r = runner(&app);
    r.trigger(meeting_id);
    r.snapshot(meeting_id).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicUsize;

    use super::*;
    use crate::managers::meetings::MeetingChannel;
    use crate::meeting_controller::{
        MeetingController, MeetingEnd, MeetingRecorders, MeetingRefusal, MeetingWarning,
        StartedMeeting,
    };
    use crate::meeting_notes_pieces::piece_budget;

    struct OneMeetingRecorder(i64);

    impl MeetingRecorders for OneMeetingRecorder {
        fn start(&self) -> Result<StartedMeeting, MeetingRefusal> {
            Ok(StartedMeeting {
                id: self.0,
                mic_name: None,
                warnings: Vec::new(),
            })
        }
        fn stop(&self, _id: i64, _reason: MeetingEnd) -> Vec<MeetingWarning> {
            Vec::new()
        }
    }

    struct CountingFake {
        calls: Arc<AtomicUsize>,
        threads: Arc<Mutex<Vec<std::thread::ThreadId>>>,
        fail: Arc<AtomicBool>,
        seg_id: i64,
    }

    impl NotesGenerator for CountingFake {
        fn generate(&mut self, _prompt: &str) -> Result<String> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.threads
                .lock()
                .unwrap()
                .push(std::thread::current().id());
            if self.fail.load(Ordering::SeqCst) {
                return Err(anyhow!("generator failure"));
            }
            Ok(format!(
                "SUMMARY: Locked homepage [S{id}]\nDECISION: Ship homepage this week [S{id}]\nACTION: Me | Review copy on Friday [S{id}]",
                id = self.seg_id
            ))
        }
    }

    #[test]
    fn stop_calls_notes_runner_once_off_ui_thread_and_preserves_data_on_failure() {
        let dir = tempfile::tempdir().unwrap();
        let store = MeetingsStore::open(dir.path()).unwrap();
        let id = store.start_meeting("Product sync", "model").unwrap();
        let seg_id = store
            .insert_segment(
                id,
                435_000,
                440_000,
                MeetingChannel::Me,
                "Me: we ship homepage this week and review copy on Friday",
            )
            .unwrap();
        let huge = format!("HUGE {}", "x".repeat(piece_budget(0) * 8));
        store
            .insert_segment(id, 500_000, 510_000, MeetingChannel::Others, &huge)
            .unwrap();
        store
            .replace_notepad(id, &["homepage this week".to_string()])
            .unwrap();

        let calls = Arc::new(AtomicUsize::new(0));
        let threads = Arc::new(Mutex::new(Vec::new()));
        let fail = Arc::new(AtomicBool::new(true));
        let emitted = Arc::new(Mutex::new(Vec::new()));

        let db_dir = dir.path().to_path_buf();
        let c_calls = Arc::clone(&calls);
        let c_threads = Arc::clone(&threads);
        let c_fail = Arc::clone(&fail);
        let c_emitted = Arc::clone(&emitted);

        let runner = NotesJobRunner::new(
            Arc::new(move || MeetingsStore::open(&db_dir)),
            Arc::new(move || {
                Ok(Box::new(CountingFake {
                    calls: Arc::clone(&c_calls),
                    threads: Arc::clone(&c_threads),
                    fail: Arc::clone(&c_fail),
                    seg_id,
                }))
            }),
            Arc::new(move |snap| c_emitted.lock().unwrap().push(snap)),
            Arc::new(|_, _| {}),
        );

        let controller = MeetingController::with_notes_trigger(
            Box::new(OneMeetingRecorder(id)),
            Arc::new(SharedNotesTrigger(Arc::clone(&runner))),
        );
        let segs_before = store.segments(id).unwrap();
        let pad_before = store.notepad(id).unwrap();

        controller.open_consent();
        controller.confirm_start().unwrap();
        controller.stop();
        controller.stop();
        runner.wait_idle(id);

        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "Stop must call runner once"
        );
        assert_ne!(
            threads.lock().unwrap()[0],
            std::thread::current().id(),
            "notes runner must run off the caller thread"
        );
        assert_eq!(store.segments(id).unwrap(), segs_before);
        assert_eq!(store.notepad(id).unwrap(), pad_before);
        assert_eq!(runner.snapshot(id).unwrap().state, NotesStateKind::Failed);

        fail.store(false, Ordering::SeqCst);
        runner.trigger(id);
        runner.wait_idle(id);

        assert_eq!(calls.load(Ordering::SeqCst), 2);
        let ready = runner.snapshot(id).unwrap();
        assert_eq!(ready.state, NotesStateKind::Ready);
        assert_eq!(ready.oversized_count, 1);
        assert!(ready.lines.iter().any(|l| l.from_notepad));
        assert!(ready.lines.iter().all(|l| l.start_ms == Some(435_000)));
        assert!(emitted
            .lock()
            .unwrap()
            .iter()
            .any(|s| s.progress_label.as_deref() == Some("Finishing notes: 0 s")));
    }
}
