use anyhow::{anyhow, Result};
use chrono::{DateTime, Local, Utc};
use log::{debug, error, info};
use rusqlite::{params, Connection, OptionalExtension};
use rusqlite_migration::{Migrations, M};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::collections::HashSet;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;
use tauri::AppHandle;
use tauri_specta::Event;

static TAKE_WAV_SEQ: AtomicU64 = AtomicU64::new(1);

/// Database migrations for transcription history.
/// Each migration is applied in order. The library tracks which migrations
/// have been applied using SQLite's user_version pragma.
///
/// Note: For users upgrading from tauri-plugin-sql, migrate_from_tauri_plugin_sql()
/// converts the old _sqlx_migrations table tracking to the user_version pragma,
/// ensuring migrations don't re-run on existing databases.
static MIGRATIONS: &[M] = &[
    M::up(
        "CREATE TABLE IF NOT EXISTS transcription_history (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            file_name TEXT NOT NULL,
            timestamp INTEGER NOT NULL,
            saved BOOLEAN NOT NULL DEFAULT 0,
            title TEXT NOT NULL,
            transcription_text TEXT NOT NULL
        );",
    ),
    M::up("ALTER TABLE transcription_history ADD COLUMN post_processed_text TEXT;"),
    M::up("ALTER TABLE transcription_history ADD COLUMN post_process_prompt TEXT;"),
    M::up("ALTER TABLE transcription_history ADD COLUMN post_process_requested BOOLEAN NOT NULL DEFAULT 0;"),
    M::up("ALTER TABLE transcription_history ADD COLUMN app_name TEXT;"),
    M::up("ALTER TABLE transcription_history ADD COLUMN detected_language TEXT;"),
];

/// Monotonic policy generation snapshot taken before an in-flight dictation
/// stages audio or persists a history row (C1 serialization boundary).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HistoryPolicySnapshot {
    pub epoch: u64,
    pub history_purge_epoch: u64,
    pub audio_off_epoch: u64,
}

#[derive(Debug, Default)]
pub struct HistoryPolicyState {
    pub epoch: u64,
    pub history_purge_epoch: u64,
    pub audio_off_epoch: u64,
    // ponytail: ceiling = in-memory set (in-flight means this process; crash empties it
    // so startup sweep treats every remaining entry as abandoned); upgrade path = per-process
    // epoch tag if multi-process writers are ever added.
    pub active_staged_wavs: HashSet<String>,
}

// ponytail: ceiling = single in-process Mutex + monotonic epoch counters serializing
// privacy transitions/purges against final SQLite inserts; upgrade path = SQLite
// policy-version table checked inside an IMMEDIATE write transaction if multi-process
// writers ever share history.db.
/// Explicit serialization and monotonic generation boundary between privacy
/// policy transitions / purges and final SQLite persistence (`I8`).
#[derive(Clone, Debug, Default)]
pub struct HistoryPolicyGate {
    inner: Arc<Mutex<HistoryPolicyState>>,
}

impl HistoryPolicyGate {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn lock(&self) -> MutexGuard<'_, HistoryPolicyState> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    #[cfg(test)]
    pub fn is_active_staged(&self, file_name: &str) -> bool {
        self.lock().active_staged_wavs.contains(file_name)
    }

    #[cfg(test)]
    pub fn active_staged_count(&self) -> usize {
        self.lock().active_staged_wavs.len()
    }

    pub fn snapshot(&self) -> HistoryPolicySnapshot {
        let guard = self.lock();
        HistoryPolicySnapshot {
            epoch: guard.epoch,
            history_purge_epoch: guard.history_purge_epoch,
            audio_off_epoch: guard.audio_off_epoch,
        }
    }

    /// Hold the policy boundary, advance the monotonic generation counters, and
    /// run `f` while serialized against all final SQLite history inserts.
    pub fn run_policy_transition<T, F>(
        &self,
        bump_history_purge: bool,
        bump_audio_off: bool,
        f: F,
    ) -> Result<T>
    where
        F: FnOnce(HistoryPolicySnapshot) -> Result<T>,
    {
        let mut guard = self.lock();
        guard.epoch = guard.epoch.saturating_add(1);
        if bump_history_purge {
            guard.history_purge_epoch = guard.history_purge_epoch.saturating_add(1);
            guard.active_staged_wavs.clear();
        }
        if bump_audio_off {
            guard.audio_off_epoch = guard.audio_off_epoch.saturating_add(1);
            guard.active_staged_wavs.clear();
        }
        let snap = HistoryPolicySnapshot {
            epoch: guard.epoch,
            history_purge_epoch: guard.history_purge_epoch,
            audio_off_epoch: guard.audio_off_epoch,
        };
        f(snap)
    }
}

/// Fixed reason kind reported on uncertain final-save audio inspection.
/// Carries NO folder paths and NO raw OS error text (`AC-C25`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StagedWavInspectionReason {
    RootCheckFailed,
    PermissionDenied,
    NotRegularFile,
    IoError,
}

impl StagedWavInspectionReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RootCheckFailed => "root_check_failed",
            Self::PermissionDenied => "permission_denied",
            Self::NotRegularFile => "not_regular_file",
            Self::IoError => "io_error",
        }
    }
}

impl std::fmt::Display for StagedWavInspectionReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Outcome of a serialized history persistence attempt under `HistoryPolicyGate`.
#[derive(Clone, Debug)]
pub enum SaveOutcome {
    Saved(HistoryEntry),
    AbortedByPolicy {
        wav_cleanup_pending: bool,
    },
    IncompleteAudioInspection {
        /// `Some(entry)` on normal take save (`require_audio == false`): text row was saved
        /// with `file_name == ""`, so caller counts it as in history for the paste notice.
        /// `None` on failed-transcription save (`require_audio == true`): no dead row saved.
        entry: Option<HistoryEntry>,
        file_name: String,
        reason: StagedWavInspectionReason,
    },
}

impl SaveOutcome {
    /// Whether a history row was persisted and counts as in history for the paste notice (`AC-C25`).
    pub fn is_in_history(&self) -> bool {
        matches!(
            self,
            Self::Saved(_) | Self::IncompleteAudioInspection { entry: Some(_), .. }
        )
    }

    /// Name-only + fixed reason-kind message when final-save audio inspection was uncertain.
    pub fn incomplete_error_message(&self) -> Option<String> {
        match self {
            Self::IncompleteAudioInspection {
                file_name, reason, ..
            } => Some(HistoryManager::format_final_save_incomplete_message(
                file_name, *reason,
            )),
            _ => None,
        }
    }
}

/// Result of a normal take save distinguishing full success from text-saved with
/// uncertain audio inspection (which still counts as in history for the paste notice).
#[derive(Clone, Debug)]
pub enum NormalSaveStatus {
    Saved(HistoryEntry),
    SavedWithIncompleteAudio {
        entry: HistoryEntry,
        incomplete_error: String,
    },
}

/// Result of inspecting a staged WAV inside the verified recordings root at final save.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StagedWavInspection {
    /// Confirmed regular non-symlink file inside the verified recordings root.
    Present,
    /// Confirmed absent (`ENOENT` / `NotFound`) inside the verified recordings root.
    ConfirmedAbsent,
    /// Root verification failed, inspection returned an error other than `ENOENT`
    /// (e.g. `EACCES`), or the target is not a regular file (e.g. symlink or directory).
    /// Callers MUST keep `pending_audio_removal` intact and surface an incomplete result.
    Uncertain(StagedWavInspectionReason),
}

#[cfg(unix)]
struct OwnedDirFd(libc::c_int);

#[cfg(unix)]
impl Drop for OwnedDirFd {
    fn drop(&mut self) {
        if self.0 >= 0 {
            // SAFETY: `self.0` is an exclusively owned file descriptor opened by `libc::open`.
            unsafe {
                libc::close(self.0);
            }
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Type)]
pub struct PaginatedHistory {
    pub entries: Vec<HistoryEntry>,
    pub has_more: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, Type, tauri_specta::Event)]
#[serde(tag = "action")]
pub enum HistoryUpdatePayload {
    #[serde(rename = "added")]
    Added { entry: HistoryEntry },
    #[serde(rename = "updated")]
    Updated { entry: HistoryEntry },
    #[serde(rename = "deleted")]
    Deleted { id: i64 },
    #[serde(rename = "toggled")]
    Toggled { id: i64 },
}

#[derive(Clone, Debug, Serialize, Deserialize, Type)]
pub struct HistoryEntry {
    pub id: i64,
    pub file_name: String,
    pub timestamp: i64,
    pub saved: bool,
    pub title: String,
    pub transcription_text: String,
    pub post_processed_text: Option<String>,
    pub post_process_prompt: Option<String>,
    pub post_process_requested: bool,
    #[serde(default)]
    pub app_name: Option<String>,
    #[serde(default)]
    pub detected_language: Option<String>,
}

pub struct HistoryManager {
    app_handle: AppHandle,
    recordings_dir: PathBuf,
    canonical_recordings_dir: PathBuf,
    db_path: PathBuf,
    policy_gate: HistoryPolicyGate,
}

impl HistoryManager {
    pub fn new(app_handle: &AppHandle) -> Result<Self> {
        // Create recordings directory in app data dir
        let app_data_dir = crate::portable::app_data_dir(app_handle)?;
        let recordings_dir = app_data_dir.join("recordings");
        let db_path = app_data_dir.join("history.db");

        // Ensure recordings directory exists
        if !recordings_dir.exists() {
            fs::create_dir_all(&recordings_dir)?;
            debug!("Created recordings directory: {:?}", recordings_dir);
        }

        // Pin the canonicalized recordings directory at start (`I10`) so any later
        // directory rename or symlink substitution is refused before unlinking.
        let canonical_recordings_dir = fs::canonicalize(&recordings_dir)?;

        let manager = Self {
            app_handle: app_handle.clone(),
            recordings_dir,
            canonical_recordings_dir,
            db_path,
            policy_gate: HistoryPolicyGate::new(),
        };

        // Initialize database and run migrations synchronously
        manager.init_database()?;

        // Drain any pending audio removals left behind by earlier I/O failures (`I9`).
        if let Err(err) = manager.drain_pending_audio_removal() {
            error!("Pending audio removal drain on startup incomplete: {}", err);
        }

        Ok(manager)
    }

    /// Generate a unique, collision-free WAV filename for a single dictation take.
    pub fn next_recording_file_name() -> String {
        let ts_ms = Utc::now().timestamp_millis();
        let seq = TAKE_WAV_SEQ.fetch_add(1, Ordering::Relaxed);
        format!("coco-voice-{}-{}-{}.wav", ts_ms, std::process::id(), seq)
    }

    /// Exclusively create a new per-take WAV inside `recordings_dir` using
    /// `OpenOptions::new().write(true).create_new(true)` (with `O_NOFOLLOW` on Unix),
    /// retrying with a unique suffix if a name is already taken and never overwriting (`I10`).
    pub fn create_unique_wav_exclusive_in(
        recordings_dir: &Path,
        base_stem: &str,
        samples: &[f32],
    ) -> Result<String> {
        for attempt in 0..16_u32 {
            let candidate = if attempt == 0 {
                format!("{base_stem}.wav")
            } else {
                let seq = TAKE_WAV_SEQ.fetch_add(1, Ordering::Relaxed);
                format!("{base_stem}-{seq}.wav")
            };
            Self::validate_recording_filename(&candidate)?;
            let path = recordings_dir.join(&candidate);
            match crate::audio_toolkit::save_wav_file_exclusive(&path, samples) {
                Ok(()) => return Ok(candidate),
                Err(err) => {
                    if err.is_already_exists() && attempt + 1 < 16 {
                        continue;
                    }
                    return Err(err.into());
                }
            }
        }
        Err(anyhow!(
            "failed to allocate unique exclusive WAV filename after retries"
        ))
    }

    pub fn policy_snapshot(&self) -> HistoryPolicySnapshot {
        self.policy_gate.snapshot()
    }

    /// Serialize a privacy policy change under `policy_gate` (bumping generation counters)
    /// without running a full table sweep (e.g. toggling `keep_dictation_audio`).
    pub fn run_policy_transition<T, F>(
        &self,
        bump_history_purge: bool,
        bump_audio_off: bool,
        f: F,
    ) -> Result<T>
    where
        F: FnOnce() -> Result<T>,
    {
        self.policy_gate
            .run_policy_transition(bump_history_purge, bump_audio_off, |_| f())
    }

    /// Serialize a privacy policy change AND its immediate retention/purge cleanup
    /// under `policy_gate` so no in-flight save can race with the policy flip or purge.
    pub fn apply_policy_change_and_cleanup<F>(
        &self,
        bump_history_purge: bool,
        bump_audio_off: bool,
        apply_settings: F,
    ) -> Result<()>
    where
        F: FnOnce() -> Result<()>,
    {
        let mut guard = self.policy_gate.lock();
        guard.epoch = guard.epoch.saturating_add(1);
        if bump_history_purge {
            guard.history_purge_epoch = guard.history_purge_epoch.saturating_add(1);
            guard.active_staged_wavs.clear();
        }
        if bump_audio_off {
            guard.audio_off_epoch = guard.audio_off_epoch.saturating_add(1);
            guard.active_staged_wavs.clear();
        }
        apply_settings()?;
        self.cleanup_old_entries_unlocked(&mut guard, bump_history_purge || bump_audio_off)
    }

    /// Turn audio storage OFF inside `policy_gate` and delete every unowned WAV in
    /// `pending_audio_removal` (in-flight takes and prior failed unlinks) BEFORE returning,
    /// while leaving already-saved row audio intact (Rule `I4`, `AC-C20`).
    pub fn apply_audio_off_and_drain<F>(&self, apply_settings: F) -> Result<()>
    where
        F: FnOnce() -> Result<()>,
    {
        let mut guard = self.policy_gate.lock();
        guard.epoch = guard.epoch.saturating_add(1);
        guard.audio_off_epoch = guard.audio_off_epoch.saturating_add(1);
        guard.active_staged_wavs.clear();
        apply_settings()?;
        self.drain_pending_audio_removal_unlocked(&guard, true)
            .map(|_| ())
    }

    fn init_database(&self) -> Result<()> {
        info!("Initializing database at {:?}", self.db_path);

        let mut conn = Connection::open(&self.db_path)?;

        // Handle migration from tauri-plugin-sql to rusqlite_migration
        // tauri-plugin-sql used _sqlx_migrations table, rusqlite_migration uses user_version pragma
        self.migrate_from_tauri_plugin_sql(&conn)?;

        // Create migrations object and run to latest version
        let migrations = Migrations::new(MIGRATIONS.to_vec());

        // Validate migrations in debug builds
        #[cfg(debug_assertions)]
        migrations.validate().expect("Invalid migrations");

        // Get current version before migration
        let version_before: i32 =
            conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
        debug!("Database version before migration: {}", version_before);

        // Apply any pending migrations
        migrations.to_latest(&mut conn)?;

        // Ensure the retry queue table exists idempotently without bumping user_version (`I9`).
        Self::ensure_pending_audio_removal_table(&conn)?;

        // Get version after migration
        let version_after: i32 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;

        if version_after > version_before {
            info!(
                "Database migrated from version {} to {}",
                version_before, version_after
            );
        } else {
            debug!("Database already at latest version {}", version_after);
        }

        Ok(())
    }

    /// Migrate from tauri-plugin-sql's migration tracking to rusqlite_migration's.
    /// tauri-plugin-sql used a _sqlx_migrations table, while rusqlite_migration uses
    /// SQLite's user_version pragma. This function checks if the old system was in use
    /// and sets the user_version accordingly so migrations don't re-run.
    fn migrate_from_tauri_plugin_sql(&self, conn: &Connection) -> Result<()> {
        // Check if the old _sqlx_migrations table exists
        let has_sqlx_migrations: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='_sqlx_migrations'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(false);

        if !has_sqlx_migrations {
            return Ok(());
        }

        // Check current user_version
        let current_version: i32 =
            conn.pragma_query_value(None, "user_version", |row| row.get(0))?;

        if current_version > 0 {
            // Already migrated to rusqlite_migration system
            return Ok(());
        }

        // Get the highest version from the old migrations table
        let old_version: i32 = conn
            .query_row(
                "SELECT COALESCE(MAX(version), 0) FROM _sqlx_migrations WHERE success = 1",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);

        if old_version > 0 {
            info!(
                "Migrating from tauri-plugin-sql (version {}) to rusqlite_migration",
                old_version
            );

            // Set user_version to match the old migration state
            conn.pragma_update(None, "user_version", old_version)?;

            // Optionally drop the old migrations table (keeping it doesn't hurt)
            // conn.execute("DROP TABLE IF EXISTS _sqlx_migrations", [])?;

            info!(
                "Migration tracking converted: user_version set to {}",
                old_version
            );
        }

        Ok(())
    }

    fn get_connection(&self) -> Result<Connection> {
        let conn = Connection::open(&self.db_path)?;
        let _ = conn.busy_timeout(Duration::from_secs(5));
        Ok(conn)
    }

    /// Validates that a recording filename is a single plain file name with no
    /// path separators, traversal segments, leading dots, NUL bytes, or stray whitespace.
    pub fn validate_recording_filename(file_name: &str) -> Result<&str> {
        if file_name.is_empty() || file_name.trim() != file_name {
            return Err(anyhow!("invalid recording filename: empty or padded"));
        }
        if file_name.starts_with('.')
            || file_name.contains('/')
            || file_name.contains('\\')
            || file_name.contains('\0')
            || file_name.contains("..")
        {
            return Err(anyhow!("invalid recording filename: '{}'", file_name));
        }
        let mut comps = Path::new(file_name).components();
        match (comps.next(), comps.next()) {
            (Some(Component::Normal(_)), None) => Ok(file_name),
            _ => Err(anyhow!("invalid recording filename: '{}'", file_name)),
        }
    }

    /// Resolves `file_name` strictly inside canonical `recordings_dir`, refusing
    /// traversal, symlinks, and non-regular files.
    pub fn safe_recording_path_in(recordings_dir: &Path, file_name: &str) -> Result<PathBuf> {
        let clean = Self::validate_recording_filename(file_name)?;
        let canon_dir = fs::canonicalize(recordings_dir).map_err(|e| {
            anyhow!(
                "cannot resolve recordings directory '{}': {}",
                recordings_dir.display(),
                e
            )
        })?;
        if !canon_dir.is_dir() {
            return Err(anyhow!(
                "recordings path is not a directory: {}",
                canon_dir.display()
            ));
        }
        let candidate = canon_dir.join(clean);
        let parent_canon = candidate
            .parent()
            .ok_or_else(|| anyhow!("missing parent directory for recording '{}'", clean))
            .and_then(|p| fs::canonicalize(p).map_err(|e| anyhow!("{}", e)))?;
        if parent_canon != canon_dir || !candidate.starts_with(&canon_dir) {
            return Err(anyhow!(
                "recording path escapes recordings directory: '{}'",
                clean
            ));
        }
        match fs::symlink_metadata(&candidate) {
            Ok(meta) => {
                if meta.file_type().is_symlink() || !meta.is_file() {
                    return Err(anyhow!(
                        "refusing symlink or non-regular recording path: '{}'",
                        clean
                    ));
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                return Err(anyhow!(
                    "failed to inspect recording path '{}': {}",
                    clean,
                    e
                ));
            }
        }
        Ok(candidate)
    }

    pub fn safe_recording_path(&self, file_name: &str) -> Result<PathBuf> {
        Self::safe_recording_path_in(&self.recordings_dir, file_name)
    }

    /// Verify `recordings_dir` is still a real (non-symlink) directory whose canonical
    /// path matches `expected_canonical_root`, and that `file_name` is a single plain
    /// filename whose parent resolves to `expected_canonical_root` (`I10`).
    fn verify_bounded_recordings_root(
        recordings_dir: &Path,
        expected_canonical_root: Option<&Path>,
        file_name: &str,
    ) -> Result<()> {
        let clean = Self::validate_recording_filename(file_name)?;
        let root_meta = fs::symlink_metadata(recordings_dir).map_err(|e| {
            anyhow!(
                "recordings root inaccessible '{}': {}",
                recordings_dir.display(),
                e
            )
        })?;
        if root_meta.file_type().is_symlink() || !root_meta.is_dir() {
            return Err(anyhow!(
                "refusing symlinked or non-directory recordings root '{}'",
                recordings_dir.display()
            ));
        }
        let canon_now = fs::canonicalize(recordings_dir).map_err(|e| {
            anyhow!(
                "cannot canonicalize recordings root '{}': {}",
                recordings_dir.display(),
                e
            )
        })?;
        if let Some(expected) = expected_canonical_root {
            if canon_now != expected {
                return Err(anyhow!(
                    "recordings root '{}' moved or substituted (expected canonical root '{}')",
                    recordings_dir.display(),
                    expected.display()
                ));
            }
        }
        let target = canon_now.join(clean);
        if target.parent() != Some(canon_now.as_path()) {
            return Err(anyhow!(
                "recording target escapes bounded root: '{}'",
                clean
            ));
        }
        Ok(())
    }

    /// Format the path-free, name-only log line for a confirmed-absent staged WAV at final save.
    pub fn format_final_save_absent_log(file_name: &str) -> String {
        format!("final_save_audio_absent file_name={file_name}")
    }

    /// Format the path-free, name-only + fixed reason-kind log/error message for an uncertain
    /// staged WAV inspection at final save (`AC-C25`). Contains no folder paths or raw OS text.
    pub fn format_final_save_incomplete_message(
        file_name: &str,
        reason: StagedWavInspectionReason,
    ) -> String {
        format!("final_save_audio_incomplete file_name={file_name} reason={reason}")
    }

    /// Inspect `file_name` strictly inside the verified recordings root without following
    /// symlinks. Distinguishes `Present` (regular file), `ConfirmedAbsent` (`ENOENT` inside
    /// the verified root), and `Uncertain(reason_kind)` (`root_check_failed`,
    /// `permission_denied`, `not_regular_file`, or `io_error`) with zero paths or raw OS text.
    pub fn inspect_staged_wav_in_verified_root(
        recordings_dir: &Path,
        expected_canonical_root: Option<&Path>,
        file_name: &str,
    ) -> StagedWavInspection {
        let Ok(clean) = Self::validate_recording_filename(file_name) else {
            return StagedWavInspection::Uncertain(StagedWavInspectionReason::RootCheckFailed);
        };

        let root_meta = match fs::symlink_metadata(recordings_dir) {
            Ok(m) => m,
            Err(err) if err.kind() == std::io::ErrorKind::PermissionDenied => {
                return StagedWavInspection::Uncertain(StagedWavInspectionReason::PermissionDenied);
            }
            Err(_) => {
                return StagedWavInspection::Uncertain(StagedWavInspectionReason::RootCheckFailed);
            }
        };
        if root_meta.file_type().is_symlink() || !root_meta.is_dir() {
            return StagedWavInspection::Uncertain(StagedWavInspectionReason::RootCheckFailed);
        }
        let canon_now = match fs::canonicalize(recordings_dir) {
            Ok(c) => c,
            Err(err) if err.kind() == std::io::ErrorKind::PermissionDenied => {
                return StagedWavInspection::Uncertain(StagedWavInspectionReason::PermissionDenied);
            }
            Err(_) => {
                return StagedWavInspection::Uncertain(StagedWavInspectionReason::RootCheckFailed);
            }
        };
        if let Some(expected) = expected_canonical_root {
            if canon_now != expected {
                return StagedWavInspection::Uncertain(StagedWavInspectionReason::RootCheckFailed);
            }
        }
        let target = canon_now.join(clean);
        if target.parent() != Some(canon_now.as_path()) {
            return StagedWavInspection::Uncertain(StagedWavInspectionReason::RootCheckFailed);
        }

        #[cfg(unix)]
        {
            use std::ffi::CString;
            use std::os::unix::ffi::OsStrExt;
            use std::os::unix::fs::MetadataExt;

            let Ok(dir_c) = CString::new(recordings_dir.as_os_str().as_bytes()) else {
                return StagedWavInspection::Uncertain(StagedWavInspectionReason::RootCheckFailed);
            };
            let Ok(name_c) = CString::new(clean.as_bytes()) else {
                return StagedWavInspection::Uncertain(StagedWavInspectionReason::RootCheckFailed);
            };

            // SAFETY: `dir_c` is NUL-terminated; `O_DIRECTORY | O_NOFOLLOW` refuses symlinks.
            let raw_fd = unsafe {
                libc::open(
                    dir_c.as_ptr(),
                    libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW,
                )
            };
            if raw_fd < 0 {
                let raw = std::io::Error::last_os_error().raw_os_error();
                return match raw {
                    Some(libc::EACCES | libc::EPERM) => {
                        StagedWavInspection::Uncertain(StagedWavInspectionReason::PermissionDenied)
                    }
                    Some(libc::ELOOP | libc::ENOTDIR) => {
                        StagedWavInspection::Uncertain(StagedWavInspectionReason::RootCheckFailed)
                    }
                    _ => StagedWavInspection::Uncertain(StagedWavInspectionReason::IoError),
                };
            }
            let dir_fd = OwnedDirFd(raw_fd);

            if let Some(expected) = expected_canonical_root {
                let mut dir_st: libc::stat = unsafe { std::mem::zeroed() };
                // SAFETY: `dir_fd.0` is an open directory fd and `dir_st` is valid writable memory.
                if unsafe { libc::fstat(dir_fd.0, &mut dir_st) } != 0 {
                    let raw = std::io::Error::last_os_error().raw_os_error();
                    return match raw {
                        Some(libc::EACCES | libc::EPERM) => StagedWavInspection::Uncertain(
                            StagedWavInspectionReason::PermissionDenied,
                        ),
                        _ => StagedWavInspection::Uncertain(StagedWavInspectionReason::IoError),
                    };
                }
                let expected_meta = match fs::metadata(expected) {
                    Ok(m) => m,
                    Err(err) if err.kind() == std::io::ErrorKind::PermissionDenied => {
                        return StagedWavInspection::Uncertain(
                            StagedWavInspectionReason::PermissionDenied,
                        );
                    }
                    Err(_) => {
                        return StagedWavInspection::Uncertain(
                            StagedWavInspectionReason::RootCheckFailed,
                        );
                    }
                };
                if dir_st.st_dev as u64 != expected_meta.dev()
                    || dir_st.st_ino as u64 != expected_meta.ino()
                {
                    return StagedWavInspection::Uncertain(
                        StagedWavInspectionReason::RootCheckFailed,
                    );
                }
            }

            let mut st: libc::stat = unsafe { std::mem::zeroed() };
            // SAFETY: `dir_fd.0` is an open verified directory fd, `name_c` is a validated
            // single-segment basename, and `st` is valid writable memory.
            let rc = unsafe {
                libc::fstatat(
                    dir_fd.0,
                    name_c.as_ptr(),
                    &mut st,
                    libc::AT_SYMLINK_NOFOLLOW,
                )
            };
            if rc == 0 {
                if (st.st_mode & libc::S_IFMT) == libc::S_IFREG {
                    StagedWavInspection::Present
                } else {
                    StagedWavInspection::Uncertain(StagedWavInspectionReason::NotRegularFile)
                }
            } else {
                let raw = std::io::Error::last_os_error().raw_os_error();
                match raw {
                    Some(libc::ENOENT) => StagedWavInspection::ConfirmedAbsent,
                    Some(libc::EACCES | libc::EPERM) => {
                        StagedWavInspection::Uncertain(StagedWavInspectionReason::PermissionDenied)
                    }
                    Some(libc::ELOOP) => {
                        StagedWavInspection::Uncertain(StagedWavInspectionReason::NotRegularFile)
                    }
                    _ => StagedWavInspection::Uncertain(StagedWavInspectionReason::IoError),
                }
            }
        }

        #[cfg(not(unix))]
        {
            match fs::symlink_metadata(recordings_dir.join(clean)) {
                Ok(meta) if !meta.file_type().is_symlink() && meta.is_file() => {
                    StagedWavInspection::Present
                }
                Ok(_) => StagedWavInspection::Uncertain(StagedWavInspectionReason::NotRegularFile),
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                    StagedWavInspection::ConfirmedAbsent
                }
                Err(err) if err.kind() == std::io::ErrorKind::PermissionDenied => {
                    StagedWavInspection::Uncertain(StagedWavInspectionReason::PermissionDenied)
                }
                Err(_) => StagedWavInspection::Uncertain(StagedWavInspectionReason::IoError),
            }
        }
    }

    /// Unlink `file_name` strictly relative to an `O_DIRECTORY | O_NOFOLLOW` descriptor
    /// for `recordings_dir` on Unix (after verifying canonical root identity if pinned),
    /// verifying via `fstatat(AT_SYMLINK_NOFOLLOW)` that the target is a regular file
    /// (`S_IFREG`) before calling `unlinkat` (`I10`).
    pub fn unlink_recording_strict_bounded(
        recordings_dir: &Path,
        expected_canonical_root: Option<&Path>,
        file_name: &str,
    ) -> Result<()> {
        if file_name.is_empty() {
            return Ok(());
        }
        Self::verify_bounded_recordings_root(recordings_dir, expected_canonical_root, file_name)?;
        Self::unlink_recording_strict(recordings_dir, file_name)
    }

    #[cfg(unix)]
    pub fn unlink_recording_strict(recordings_dir: &Path, file_name: &str) -> Result<()> {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;

        if file_name.is_empty() {
            return Ok(());
        }
        let clean = Self::validate_recording_filename(file_name)?;
        let dir_c = CString::new(recordings_dir.as_os_str().as_bytes())
            .map_err(|_| anyhow!("recordings directory path contains NUL byte"))?;
        let name_c = CString::new(clean.as_bytes())
            .map_err(|_| anyhow!("recording filename contains NUL byte"))?;

        // SAFETY: `dir_c` is a valid NUL-terminated path; `O_DIRECTORY | O_NOFOLLOW` refuses
        // symlinks or non-directories at the recordings root itself.
        let raw_fd = unsafe {
            libc::open(
                dir_c.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW,
            )
        };
        if raw_fd < 0 {
            let err = std::io::Error::last_os_error();
            return Err(anyhow!(
                "refusing symlinked or inaccessible recordings root '{}': {}",
                recordings_dir.display(),
                err
            ));
        }
        let dir_fd = OwnedDirFd(raw_fd);

        let mut st: libc::stat = unsafe { std::mem::zeroed() };
        // SAFETY: `dir_fd.0` is an open directory descriptor, `name_c` is a plain single-segment
        // NUL-terminated filename, and `st` is valid writable storage.
        let stat_rc = unsafe {
            libc::fstatat(
                dir_fd.0,
                name_c.as_ptr(),
                &mut st,
                libc::AT_SYMLINK_NOFOLLOW,
            )
        };
        if stat_rc < 0 {
            let err = std::io::Error::last_os_error();
            if err.raw_os_error() == Some(libc::ENOENT) {
                return Ok(());
            }
            return Err(anyhow!(
                "failed to inspect recording entry '{}': {}",
                clean,
                err
            ));
        }
        if (st.st_mode & libc::S_IFMT) != libc::S_IFREG {
            return Err(anyhow!(
                "refusing symlink or non-regular recording entry: '{}'",
                clean
            ));
        }

        // SAFETY: `dir_fd.0` pins the verified recordings directory root and `name_c` is a
        // validated single filename checked as S_IFREG relative to `dir_fd.0`.
        let unlink_rc = unsafe { libc::unlinkat(dir_fd.0, name_c.as_ptr(), 0) };
        if unlink_rc == 0 {
            debug!("Deleted WAV file via bounded dirfd: {}", clean);
            return Ok(());
        }
        let err = std::io::Error::last_os_error();
        if err.raw_os_error() == Some(libc::ENOENT) {
            return Ok(());
        }
        Err(anyhow!("failed to delete WAV file '{}': {}", clean, err))
    }

    #[cfg(not(unix))]
    pub fn unlink_recording_strict(recordings_dir: &Path, file_name: &str) -> Result<()> {
        if file_name.is_empty() {
            return Ok(());
        }
        let safe_path = Self::safe_recording_path_in(recordings_dir, file_name)?;
        crate::audio_toolkit::unlink_regular_file_non_unix(&safe_path)?;
        debug!("Deleted WAV file: {}", file_name);
        Ok(())
    }

    fn ensure_pending_audio_removal_table(conn: &Connection) -> Result<()> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS pending_audio_removal (
                file_name TEXT PRIMARY KEY,
                first_failed_at INTEGER NOT NULL
            );",
        )?;
        Ok(())
    }

    /// Record a candidate WAV filename in `pending_audio_removal` BEFORE `create_new`
    /// runs so every unowned WAV is tracked from before it exists on disk.
    /// Fails with a SQLite UNIQUE constraint error if the name is already tracked.
    pub fn insert_pending_audio_removal_fresh_in(conn: &Connection, file_name: &str) -> Result<()> {
        let clean = Self::validate_recording_filename(file_name)?;
        Self::ensure_pending_audio_removal_table(conn)?;
        conn.execute(
            "INSERT INTO pending_audio_removal (file_name, first_failed_at)
             VALUES (?1, ?2)",
            params![clean, Utc::now().timestamp()],
        )?;
        Ok(())
    }

    /// Idempotently record a WAV filename in `pending_audio_removal` (`I9`).
    /// Log lines name ONLY `file_name`, never take text.
    pub fn record_pending_audio_removal_in(conn: &Connection, file_name: &str) -> Result<()> {
        if file_name.is_empty() {
            return Ok(());
        }
        let clean = Self::validate_recording_filename(file_name)?;
        Self::ensure_pending_audio_removal_table(conn)?;
        conn.execute(
            "INSERT OR IGNORE INTO pending_audio_removal (file_name, first_failed_at)
             VALUES (?1, ?2)",
            params![clean, Utc::now().timestamp()],
        )?;
        Ok(())
    }

    pub fn delete_pending_audio_removal_in(conn: &Connection, file_name: &str) -> Result<()> {
        if file_name.is_empty() {
            return Ok(());
        }
        Self::ensure_pending_audio_removal_table(conn)?;
        conn.execute(
            "DELETE FROM pending_audio_removal WHERE file_name = ?1",
            params![file_name],
        )?;
        Ok(())
    }

    pub fn pending_audio_removal_count_in(conn: &Connection) -> Result<usize> {
        Self::ensure_pending_audio_removal_table(conn)?;
        let count: i64 =
            conn.query_row("SELECT COUNT(*) FROM pending_audio_removal", [], |row| {
                row.get(0)
            })?;
        Ok(count.max(0) as usize)
    }

    #[cfg(test)]
    pub fn pending_audio_removal_names_in(conn: &Connection) -> Result<Vec<String>> {
        Self::ensure_pending_audio_removal_table(conn)?;
        let mut stmt = conn.prepare(
            "SELECT file_name FROM pending_audio_removal ORDER BY first_failed_at ASC, file_name ASC",
        )?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn unlink_or_record_pending_with_hook_in<R>(
        conn: &Connection,
        recordings_dir: &Path,
        expected_canonical_root: Option<&Path>,
        file_name: &str,
        remove_fn: R,
    ) -> Result<()>
    where
        R: FnOnce(&Path, Option<&Path>, &str) -> Result<()>,
    {
        if file_name.is_empty() {
            return Ok(());
        }
        match remove_fn(recordings_dir, expected_canonical_root, file_name) {
            Ok(()) => {
                let _ = Self::delete_pending_audio_removal_in(conn, file_name);
                Ok(())
            }
            Err(err) => {
                let _ = Self::record_pending_audio_removal_in(conn, file_name);
                Err(err)
            }
        }
    }

    pub fn unlink_or_record_pending_in(
        conn: &Connection,
        recordings_dir: &Path,
        expected_canonical_root: Option<&Path>,
        file_name: &str,
    ) -> Result<()> {
        Self::unlink_or_record_pending_with_hook_in(
            conn,
            recordings_dir,
            expected_canonical_root,
            file_name,
            Self::unlink_recording_strict_bounded,
        )
    }

    /// Drain `pending_audio_removal` inside `policy_gate`.
    /// - When `include_active_staged == false` (routine retention / hourly sweep),
    ///   skips any `file_name` currently present in `active_staged_wavs` so live
    ///   in-flight takes keep their staged WAV intact.
    /// - Startup sweep starts with an empty `active_staged_wavs` so crash-abandoned
    ///   intents drain immediately (`AC-C17`).
    /// - Explicit privacy transitions (`Never` or audio-off) clear `active_staged_wavs`
    ///   and pass `include_active_staged == true` so affected live staged WAVs drain
    ///   before the command returns (`AC-C20`).
    /// - Unlinks a file ONLY IF no row in `transcription_history` references its `file_name`.
    ///   If any row references `file_name`, drops the pending entry and keeps the file intact.
    pub fn drain_pending_audio_removal_with_hook_and_active_in<R>(
        conn: &Connection,
        recordings_dir: &Path,
        expected_canonical_root: Option<&Path>,
        active_staged_wavs: &HashSet<String>,
        include_active_staged: bool,
        mut remove_fn: R,
    ) -> Result<usize>
    where
        R: FnMut(&Path, Option<&Path>, &str) -> Result<()>,
    {
        Self::ensure_pending_audio_removal_table(conn)?;
        let pending: Vec<String> = {
            let mut stmt = conn.prepare(
                "SELECT file_name
                 FROM pending_audio_removal
                 ORDER BY first_failed_at ASC, file_name ASC",
            )?;
            let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
            let mut out = Vec::new();
            for r in rows {
                out.push(r?);
            }
            out
        };

        if pending.is_empty() {
            return Ok(0);
        }

        let mut drained = 0usize;
        let mut failures: Vec<String> = Vec::new();

        for file_name in pending {
            if !include_active_staged && active_staged_wavs.contains(&file_name) {
                continue;
            }

            let referenced: i64 = conn.query_row(
                "SELECT COUNT(*) FROM transcription_history WHERE file_name = ?1",
                params![&file_name],
                |r| r.get(0),
            )?;
            if referenced > 0 {
                conn.execute(
                    "DELETE FROM pending_audio_removal WHERE file_name = ?1",
                    params![&file_name],
                )?;
                debug!(
                    "Dropped pending removal for referenced WAV '{}' (file preserved)",
                    file_name
                );
                continue;
            }

            match remove_fn(recordings_dir, expected_canonical_root, &file_name) {
                Ok(()) => {
                    conn.execute(
                        "DELETE FROM pending_audio_removal WHERE file_name = ?1",
                        params![&file_name],
                    )?;
                    drained += 1;
                }
                Err(err) => {
                    error!(
                        "Pending audio removal retry failed for '{}': {}",
                        file_name, err
                    );
                    failures.push(format!("{}: {}", file_name, err));
                }
            }
        }

        if !failures.is_empty() {
            return Err(anyhow!(
                "incomplete pending audio removal ({} drained, {} remaining): {}",
                drained,
                failures.len(),
                failures.join("; ")
            ));
        }

        Ok(drained)
    }

    pub fn drain_pending_audio_removal_with_hook_in<R>(
        conn: &Connection,
        recordings_dir: &Path,
        expected_canonical_root: Option<&Path>,
        remove_fn: R,
    ) -> Result<usize>
    where
        R: FnMut(&Path, Option<&Path>, &str) -> Result<()>,
    {
        let empty = HashSet::new();
        Self::drain_pending_audio_removal_with_hook_and_active_in(
            conn,
            recordings_dir,
            expected_canonical_root,
            &empty,
            true,
            remove_fn,
        )
    }

    pub fn drain_pending_audio_removal_in(
        conn: &Connection,
        recordings_dir: &Path,
        expected_canonical_root: Option<&Path>,
    ) -> Result<usize> {
        Self::drain_pending_audio_removal_with_hook_in(
            conn,
            recordings_dir,
            expected_canonical_root,
            Self::unlink_recording_strict_bounded,
        )
    }

    /// Discard a staged take WAV inside `policy_gate` (e.g. on cancel before save).
    /// Removes from `active_staged_wavs`, unlinks via bounded `unlinkat`, and removes its
    /// name from `pending_audio_removal` only if the unlink succeeds; if unlink fails,
    /// the name stays queued for retry.
    pub fn discard_staged_wav(&self, file_name: &str) -> Result<()> {
        if file_name.is_empty() {
            return Ok(());
        }
        let mut guard = self.policy_gate.lock();
        guard.active_staged_wavs.remove(file_name);
        let conn = self.get_connection()?;
        Self::unlink_or_record_pending_in(
            &conn,
            &self.recordings_dir,
            Some(&self.canonical_recordings_dir),
            file_name,
        )
    }

    fn drain_pending_audio_removal_unlocked(
        &self,
        state: &HistoryPolicyState,
        include_active_staged: bool,
    ) -> Result<usize> {
        let conn = self.get_connection()?;
        Self::drain_pending_audio_removal_with_hook_and_active_in(
            &conn,
            &self.recordings_dir,
            Some(&self.canonical_recordings_dir),
            &state.active_staged_wavs,
            include_active_staged,
            Self::unlink_recording_strict_bounded,
        )
    }

    pub fn drain_pending_audio_removal(&self) -> Result<usize> {
        let guard = self.policy_gate.lock();
        self.drain_pending_audio_removal_unlocked(&guard, false)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn pending_audio_removal_count(&self) -> Result<usize> {
        let conn = self.get_connection()?;
        Self::pending_audio_removal_count_in(&conn)
    }

    fn map_history_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<HistoryEntry> {
        Ok(HistoryEntry {
            id: row.get("id")?,
            file_name: row.get("file_name")?,
            timestamp: row.get("timestamp")?,
            saved: row.get("saved")?,
            title: row.get("title")?,
            transcription_text: row.get("transcription_text")?,
            post_processed_text: row.get("post_processed_text")?,
            post_process_prompt: row.get("post_process_prompt")?,
            post_process_requested: row.get("post_process_requested")?,
            app_name: row.get("app_name").ok().flatten(),
            detected_language: row.get("detected_language").ok().flatten(),
        })
    }

    pub fn recordings_dir(&self) -> &std::path::Path {
        &self.recordings_dir
    }

    /// Save a new history entry to the database.
    /// The WAV file should already have been written to the recordings directory.
    pub fn save_entry(
        &self,
        file_name: String,
        transcription_text: String,
        post_process_requested: bool,
        post_processed_text: Option<String>,
        post_process_prompt: Option<String>,
    ) -> Result<HistoryEntry> {
        self.save_entry_with_meta(
            file_name,
            transcription_text,
            post_process_requested,
            post_processed_text,
            post_process_prompt,
            None,
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn save_entry_with_meta(
        &self,
        file_name: String,
        transcription_text: String,
        post_process_requested: bool,
        post_processed_text: Option<String>,
        post_process_prompt: Option<String>,
        app_name: Option<String>,
        detected_language: Option<String>,
    ) -> Result<HistoryEntry> {
        let snapshot = self.policy_snapshot();
        match self.save_entry_with_meta_for_snapshot(
            snapshot,
            file_name,
            transcription_text,
            post_process_requested,
            post_processed_text,
            post_process_prompt,
            app_name,
            detected_language,
        )? {
            NormalSaveStatus::Saved(entry)
            | NormalSaveStatus::SavedWithIncompleteAudio { entry, .. } => Ok(entry),
        }
    }

    /// Stage a take's WAV file INSIDE `policy_gate` as one intent-first step:
    /// 1. Re-read `never_store_history` and `keep_dictation_audio` and compare epochs.
    /// 2. Insert the candidate `file_name` into `pending_audio_removal` BEFORE `create_new`.
    ///    If that DB insert fails, write NO WAV (`Ok(None)` — take continues text-only).
    /// 3. `create_new(true)` (`O_NOFOLLOW` on Unix), write, verify, close.
    ///    - `OpenFailed` (e.g. `AlreadyExists` foreign file): remove ONLY our own name from
    ///      `pending_audio_removal` — NEVER unlink the foreign file — and try the next name.
    ///    - `WriteFailedAfterCreate` or verify failure: this process OWNS the partial file;
    ///      attempt removal through `remove_fn` (root-bounded `unlinkat`) and delete from
    ///      `pending_audio_removal` ONLY when removal returns `Ok(())` — otherwise KEEP the
    ///      durable pending entry for retry (and NEVER mark as active-staged).
    /// 4. On verified success, record `candidate` in `active_staged_wavs` so routine
    ///    retention sweeps preserve this live take's audio while transcription runs.
    /// 5. Release `policy_gate`.
    // ponytail: ceiling = gate held during one WAV write (~10 MB max for a 5-min take);
    // upgrade path = per-take slot if a measure ever shows contention.
    #[allow(clippy::too_many_arguments)]
    pub fn stage_take_wav_under_policy_gate_with_hooks_in<F, I, W, V, R>(
        conn: &Connection,
        recordings_dir: &Path,
        expected_canonical_root: Option<&Path>,
        policy_gate: &HistoryPolicyGate,
        snapshot: HistoryPolicySnapshot,
        read_live_policy: F,
        base_stem: &str,
        samples: &[f32],
        mut insert_pending_fn: I,
        mut write_fn: W,
        mut verify_fn: V,
        mut remove_fn: R,
    ) -> Result<Option<String>>
    where
        F: FnOnce() -> (bool, bool),
        I: FnMut(&Connection, &str) -> Result<()>,
        W: FnMut(&Path, &[f32]) -> std::result::Result<(), crate::audio_toolkit::ExclusiveWavError>,
        V: FnMut(&Path, usize) -> Result<()>,
        R: FnMut(&Path, Option<&Path>, &str) -> Result<()>,
    {
        let mut guard = policy_gate.lock();
        let (never_store_now, keep_audio_now) = read_live_policy();
        if never_store_now
            || guard.history_purge_epoch != snapshot.history_purge_epoch
            || !keep_audio_now
            || guard.audio_off_epoch != snapshot.audio_off_epoch
        {
            return Ok(None);
        }
        if samples.is_empty() {
            return Ok(None);
        }

        Self::verify_bounded_recordings_root(
            recordings_dir,
            expected_canonical_root,
            &format!("{base_stem}.wav"),
        )?;

        for attempt in 0..16_u32 {
            let candidate = if attempt == 0 {
                format!("{base_stem}.wav")
            } else {
                let seq = TAKE_WAV_SEQ.fetch_add(1, Ordering::Relaxed);
                format!("{base_stem}-{seq}.wav")
            };
            Self::validate_recording_filename(&candidate)?;

            if let Err(db_err) = insert_pending_fn(conn, &candidate) {
                let is_unique = db_err
                    .downcast_ref::<rusqlite::Error>()
                    .and_then(|e| e.sqlite_error_code())
                    .is_some_and(|code| code == rusqlite::ErrorCode::ConstraintViolation);
                if is_unique && attempt + 1 < 16 {
                    continue;
                }
                error!(
                    "Skipping WAV write because pending_audio_removal insert failed for '{}': {}",
                    candidate, db_err
                );
                return Ok(None);
            }

            // Active set entry joins BEFORE `create_new` (when recorded into `pending_audio_removal`).
            guard.active_staged_wavs.insert(candidate.clone());

            let path = recordings_dir.join(&candidate);
            match write_fn(&path, samples) {
                Ok(()) => {
                    // Ownership begins ONLY after our own `create_new(true)` succeeded.
                    match verify_fn(&path, samples.len()) {
                        Ok(()) => return Ok(Some(candidate)),
                        Err(verify_err) => {
                            guard.active_staged_wavs.remove(&candidate);
                            error!(
                                "WAV verification failed for '{}': {}",
                                candidate, verify_err
                            );
                            // Keep durable pending entry unless root-bounded removal succeeds;
                            // report delete failure to caller instead of swallowing it.
                            match remove_fn(recordings_dir, expected_canonical_root, &candidate) {
                                Ok(()) => {
                                    let _ = Self::delete_pending_audio_removal_in(conn, &candidate);
                                    return Ok(None);
                                }
                                Err(delete_err) => {
                                    return Err(anyhow!(
                                        "WAV verification failed for '{}' ({}) and bounded delete failed ({})",
                                        candidate,
                                        verify_err,
                                        delete_err
                                    ));
                                }
                            }
                        }
                    }
                }
                Err(crate::audio_toolkit::ExclusiveWavError::OpenFailed(open_err)) => {
                    // Open failed BEFORE this process created or owned the file:
                    // drop active-set entry and remove ONLY our own candidate name from
                    // `pending_audio_removal`; NEVER unlink the pre-existing foreign path (`AC-C18`).
                    guard.active_staged_wavs.remove(&candidate);
                    let _ = Self::delete_pending_audio_removal_in(conn, &candidate);
                    let is_exists = open_err
                        .downcast_ref::<std::io::Error>()
                        .is_some_and(|io_e| io_e.kind() == std::io::ErrorKind::AlreadyExists);
                    if is_exists && attempt + 1 < 16 {
                        continue;
                    }
                    error!("Failed to open exclusive WAV '{}': {}", candidate, open_err);
                    return Ok(None);
                }
                Err(crate::audio_toolkit::ExclusiveWavError::WriteFailedAfterCreate(write_err)) => {
                    // This process OWNS the partial file and its name is ALREADY durably
                    // in `pending_audio_removal`. Remove from `active_staged_wavs` so if
                    // deletion fails it becomes abandoned and sweeps retry it; report any
                    // bounded delete failure to the caller instead of swallowing it.
                    guard.active_staged_wavs.remove(&candidate);
                    error!(
                        "Failed to write exclusive WAV '{}' after create: {}",
                        candidate, write_err
                    );
                    match remove_fn(recordings_dir, expected_canonical_root, &candidate) {
                        Ok(()) => {
                            let _ = Self::delete_pending_audio_removal_in(conn, &candidate);
                            return Ok(None);
                        }
                        Err(delete_err) => {
                            return Err(anyhow!(
                                "WAV write failed after create for '{}' ({}) and bounded delete failed ({})",
                                candidate,
                                write_err,
                                delete_err
                            ));
                        }
                    }
                }
            }
        }

        error!("Failed to allocate unique staged WAV filename after retries");
        Ok(None)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn stage_take_wav_under_policy_gate_in<F>(
        conn: &Connection,
        recordings_dir: &Path,
        expected_canonical_root: Option<&Path>,
        policy_gate: &HistoryPolicyGate,
        snapshot: HistoryPolicySnapshot,
        read_live_policy: F,
        base_stem: &str,
        samples: &[f32],
    ) -> Result<Option<String>>
    where
        F: FnOnce() -> (bool, bool),
    {
        Self::stage_take_wav_under_policy_gate_with_hooks_in(
            conn,
            recordings_dir,
            expected_canonical_root,
            policy_gate,
            snapshot,
            read_live_policy,
            base_stem,
            samples,
            Self::insert_pending_audio_removal_fresh_in,
            |p, s| crate::audio_toolkit::save_wav_file_exclusive(p, s),
            |p, n| crate::audio_toolkit::verify_wav_file(p, n),
            Self::unlink_recording_strict_bounded,
        )
    }

    pub fn stage_take_wav_for_snapshot(
        &self,
        snapshot: HistoryPolicySnapshot,
        samples: &[f32],
    ) -> Result<Option<String>> {
        let conn = self.get_connection()?;
        let app_handle = &self.app_handle;
        let base_stem = format!(
            "coco-voice-{}-{}-{}",
            Utc::now().timestamp_millis(),
            std::process::id(),
            TAKE_WAV_SEQ.fetch_add(1, Ordering::Relaxed)
        );
        Self::stage_take_wav_under_policy_gate_in(
            &conn,
            &self.recordings_dir,
            Some(&self.canonical_recordings_dir),
            &self.policy_gate,
            snapshot,
            || {
                let s = crate::settings::get_settings(app_handle);
                (s.never_store_history, s.keep_dictation_audio)
            },
            &base_stem,
            samples,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn save_entry_under_policy_gate_with_hook_in<F, R>(
        conn: &mut Connection,
        recordings_dir: &Path,
        expected_canonical_root: Option<&Path>,
        policy_gate: &HistoryPolicyGate,
        snapshot: HistoryPolicySnapshot,
        read_live_policy: F,
        before_lock_hook: Option<&dyn Fn()>,
        remove_fn: R,
        file_name: String,
        transcription_text: String,
        post_process_requested: bool,
        post_processed_text: Option<String>,
        post_process_prompt: Option<String>,
        app_name: Option<String>,
        detected_language: Option<String>,
    ) -> Result<SaveOutcome>
    where
        F: FnOnce() -> (bool, bool),
        R: FnOnce(&Path, Option<&Path>, &str) -> Result<()>,
    {
        Self::save_entry_under_policy_gate_core_in(
            conn,
            recordings_dir,
            expected_canonical_root,
            policy_gate,
            snapshot,
            read_live_policy,
            before_lock_hook,
            remove_fn,
            false,
            file_name,
            transcription_text,
            post_process_requested,
            post_processed_text,
            post_process_prompt,
            app_name,
            detected_language,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn save_entry_under_policy_gate_core_in<F, R>(
        conn: &mut Connection,
        recordings_dir: &Path,
        expected_canonical_root: Option<&Path>,
        policy_gate: &HistoryPolicyGate,
        snapshot: HistoryPolicySnapshot,
        read_live_policy: F,
        before_lock_hook: Option<&dyn Fn()>,
        remove_fn: R,
        require_audio: bool,
        file_name: String,
        transcription_text: String,
        post_process_requested: bool,
        post_processed_text: Option<String>,
        post_process_prompt: Option<String>,
        app_name: Option<String>,
        detected_language: Option<String>,
    ) -> Result<SaveOutcome>
    where
        F: FnOnce() -> (bool, bool),
        R: FnOnce(&Path, Option<&Path>, &str) -> Result<()>,
    {
        Self::save_entry_under_policy_gate_with_inspect_hook_in(
            conn,
            recordings_dir,
            expected_canonical_root,
            policy_gate,
            snapshot,
            read_live_policy,
            before_lock_hook,
            remove_fn,
            Self::inspect_staged_wav_in_verified_root,
            require_audio,
            file_name,
            transcription_text,
            post_process_requested,
            post_processed_text,
            post_process_prompt,
            app_name,
            detected_language,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn save_entry_under_policy_gate_with_inspect_hook_in<F, R, I>(
        conn: &mut Connection,
        recordings_dir: &Path,
        expected_canonical_root: Option<&Path>,
        policy_gate: &HistoryPolicyGate,
        snapshot: HistoryPolicySnapshot,
        read_live_policy: F,
        before_lock_hook: Option<&dyn Fn()>,
        remove_fn: R,
        inspect_fn: I,
        require_audio: bool,
        file_name: String,
        transcription_text: String,
        post_process_requested: bool,
        post_processed_text: Option<String>,
        post_process_prompt: Option<String>,
        app_name: Option<String>,
        detected_language: Option<String>,
    ) -> Result<SaveOutcome>
    where
        F: FnOnce() -> (bool, bool),
        R: FnOnce(&Path, Option<&Path>, &str) -> Result<()>,
        I: FnOnce(&Path, Option<&Path>, &str) -> StagedWavInspection,
    {
        if let Some(hook) = before_lock_hook {
            hook();
        }

        // Hold `policy_gate` across the live policy + epoch check AND the single SQLite
        // transaction (`INSERT INTO transcription_history` + `DELETE FROM pending_audio_removal`)
        // so no concurrent switch to Never, audio-off, or purge can interleave with persistence.
        let mut guard = policy_gate.lock();
        if !file_name.is_empty() {
            guard.active_staged_wavs.remove(&file_name);
        }
        let (never_store_now, keep_audio_now) = read_live_policy();

        if never_store_now || guard.history_purge_epoch != snapshot.history_purge_epoch {
            let wav_cleanup_pending = if !file_name.is_empty() {
                Self::unlink_or_record_pending_with_hook_in(
                    conn,
                    recordings_dir,
                    expected_canonical_root,
                    &file_name,
                    remove_fn,
                )
                .is_err()
            } else {
                false
            };
            return Ok(SaveOutcome::AbortedByPolicy {
                wav_cleanup_pending,
            });
        }

        let audio_allowed = keep_audio_now && guard.audio_off_epoch == snapshot.audio_off_epoch;
        if require_audio && (!audio_allowed || file_name.is_empty()) {
            let wav_cleanup_pending = if !file_name.is_empty() {
                Self::unlink_or_record_pending_with_hook_in(
                    conn,
                    recordings_dir,
                    expected_canonical_root,
                    &file_name,
                    remove_fn,
                )
                .is_err()
            } else {
                false
            };
            return Ok(SaveOutcome::AbortedByPolicy {
                wav_cleanup_pending,
            });
        }

        let mut effective_file_name = if !file_name.is_empty() && !audio_allowed {
            let _ = Self::unlink_or_record_pending_with_hook_in(
                conn,
                recordings_dir,
                expected_canonical_root,
                &file_name,
                remove_fn,
            );
            String::new()
        } else {
            file_name
        };

        // Final save inspection inside `policy_gate` (AC-C24 / AC-C25):
        // - `Present`: verified root + regular file -> save with `file_name`, delete pending in same tx.
        // - `ConfirmedAbsent`: verified root + `ENOENT` -> log name only, delete pending entry;
        //   if `require_audio` (failed-transcription retry row) save NO dead row and return AbortedByPolicy;
        //   if normal take save (`!require_audio`) save text row with `file_name = ""` in same tx.
        // - `Uncertain(reason_kind)`: log ONLY file name + fixed reason kind (no paths, no raw OS text),
        //   KEEP `pending_audio_removal` (name already left `active_staged_wavs`);
        //   if `require_audio` save NO dead row and return `IncompleteAudioInspection { entry: None, .. }`;
        //   if normal take save (`!require_audio`) save text row with `file_name = ""` and return
        //   `IncompleteAudioInspection { entry: Some(entry), .. }` (counts as in history for paste notice!).
        let mut stale_confirmed_absent_name: Option<String> = None;
        let mut uncertain_inspection: Option<(String, StagedWavInspectionReason)> = None;
        if !effective_file_name.is_empty() {
            match inspect_fn(
                recordings_dir,
                expected_canonical_root,
                &effective_file_name,
            ) {
                StagedWavInspection::Present => {}
                StagedWavInspection::ConfirmedAbsent => {
                    error!(
                        "{}",
                        Self::format_final_save_absent_log(&effective_file_name)
                    );
                    stale_confirmed_absent_name = Some(effective_file_name);
                    effective_file_name = String::new();
                    if require_audio {
                        if let Some(missing) = stale_confirmed_absent_name.as_deref() {
                            let _ = Self::delete_pending_audio_removal_in(conn, missing);
                        }
                        return Ok(SaveOutcome::AbortedByPolicy {
                            wav_cleanup_pending: false,
                        });
                    }
                }
                StagedWavInspection::Uncertain(reason) => {
                    let inspected_name = effective_file_name;
                    error!(
                        "{}",
                        Self::format_final_save_incomplete_message(&inspected_name, reason)
                    );
                    effective_file_name = String::new();
                    if require_audio {
                        return Ok(SaveOutcome::IncompleteAudioInspection {
                            entry: None,
                            file_name: inspected_name,
                            reason,
                        });
                    }
                    uncertain_inspection = Some((inspected_name, reason));
                }
            }
        }

        Self::ensure_pending_audio_removal_table(conn)?;
        let timestamp = Utc::now().timestamp();
        let title = Self::format_timestamp_title_static(timestamp);

        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        tx.execute(
            "INSERT INTO transcription_history (
                file_name,
                timestamp,
                saved,
                title,
                transcription_text,
                post_processed_text,
                post_process_prompt,
                post_process_requested,
                app_name,
                detected_language
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                &effective_file_name,
                timestamp,
                false,
                &title,
                &transcription_text,
                &post_processed_text,
                &post_process_prompt,
                post_process_requested,
                &app_name,
                &detected_language,
            ],
        )?;
        let entry_id = tx.last_insert_rowid();
        if !effective_file_name.is_empty() {
            tx.execute(
                "DELETE FROM pending_audio_removal WHERE file_name = ?1",
                params![&effective_file_name],
            )?;
        }
        if let Some(missing) = stale_confirmed_absent_name.as_deref() {
            tx.execute(
                "DELETE FROM pending_audio_removal WHERE file_name = ?1",
                params![missing],
            )?;
        }
        tx.commit()?;

        let entry = HistoryEntry {
            id: entry_id,
            file_name: effective_file_name,
            timestamp,
            saved: false,
            title,
            transcription_text,
            post_processed_text,
            post_process_prompt,
            post_process_requested,
            app_name,
            detected_language,
        };

        if let Some((inspected_name, reason)) = uncertain_inspection {
            return Ok(SaveOutcome::IncompleteAudioInspection {
                entry: Some(entry),
                file_name: inspected_name,
                reason,
            });
        }

        Ok(SaveOutcome::Saved(entry))
    }

    #[allow(clippy::too_many_arguments)]
    pub fn save_entry_under_policy_gate_in<F>(
        conn: &mut Connection,
        recordings_dir: &Path,
        policy_gate: &HistoryPolicyGate,
        snapshot: HistoryPolicySnapshot,
        read_live_policy: F,
        before_lock_hook: Option<&dyn Fn()>,
        file_name: String,
        transcription_text: String,
        post_process_requested: bool,
        post_processed_text: Option<String>,
        post_process_prompt: Option<String>,
        app_name: Option<String>,
        detected_language: Option<String>,
    ) -> Result<SaveOutcome>
    where
        F: FnOnce() -> (bool, bool),
    {
        let canon = fs::canonicalize(recordings_dir).ok();
        Self::save_entry_under_policy_gate_with_hook_in(
            conn,
            recordings_dir,
            canon.as_deref(),
            policy_gate,
            snapshot,
            read_live_policy,
            before_lock_hook,
            Self::unlink_recording_strict_bounded,
            file_name,
            transcription_text,
            post_process_requested,
            post_processed_text,
            post_process_prompt,
            app_name,
            detected_language,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn save_entry_with_meta_for_snapshot(
        &self,
        snapshot: HistoryPolicySnapshot,
        file_name: String,
        transcription_text: String,
        post_process_requested: bool,
        post_processed_text: Option<String>,
        post_process_prompt: Option<String>,
        app_name: Option<String>,
        detected_language: Option<String>,
    ) -> Result<NormalSaveStatus> {
        let mut conn = self.get_connection()?;
        let app_handle = &self.app_handle;
        let outcome = Self::save_entry_under_policy_gate_with_hook_in(
            &mut conn,
            &self.recordings_dir,
            Some(&self.canonical_recordings_dir),
            &self.policy_gate,
            snapshot,
            || {
                let s = crate::settings::get_settings(app_handle);
                (s.never_store_history, s.keep_dictation_audio)
            },
            None,
            Self::unlink_recording_strict_bounded,
            file_name,
            transcription_text,
            post_process_requested,
            post_processed_text,
            post_process_prompt,
            app_name,
            detected_language,
        )?;

        match outcome {
            SaveOutcome::Saved(entry) => {
                debug!("Saved history entry with id {}", entry.id);
                self.cleanup_old_entries()?;
                if let Err(e) = (HistoryUpdatePayload::Added {
                    entry: entry.clone(),
                })
                .emit(&self.app_handle)
                {
                    error!("Failed to emit history-updated event: {}", e);
                }
                Ok(NormalSaveStatus::Saved(entry))
            }
            SaveOutcome::IncompleteAudioInspection {
                entry: Some(entry),
                file_name,
                reason,
            } => {
                // Text row is saved (counts as in history for the paste notice); audio
                // verification was uncertain so `pending_audio_removal` remains queued.
                debug!(
                    "Saved text-only history entry {} with incomplete audio inspection",
                    entry.id
                );
                if let Err(e) = (HistoryUpdatePayload::Added {
                    entry: entry.clone(),
                })
                .emit(&self.app_handle)
                {
                    error!("Failed to emit history-updated event: {}", e);
                }
                let incomplete_error =
                    Self::format_final_save_incomplete_message(&file_name, reason);
                Ok(NormalSaveStatus::SavedWithIncompleteAudio {
                    entry,
                    incomplete_error,
                })
            }
            SaveOutcome::IncompleteAudioInspection {
                entry: None,
                file_name,
                reason,
            } => Err(anyhow!(
                "{}",
                Self::format_final_save_incomplete_message(&file_name, reason)
            )),
            SaveOutcome::AbortedByPolicy { .. } => Err(anyhow!(
                "refused to save history entry while Never store history or a purge epoch is active"
            )),
        }
    }

    /// Save an empty-text retry row for a failed transcription ONLY if live policy
    /// inside `policy_gate` still permits BOTH history and audio AND the audio file is
    /// confirmed present; otherwise save NO dead row (deleting pending on ConfirmedAbsent,
    /// keeping pending and returning name-only + reason-kind Err on Uncertain).
    pub fn save_failed_retry_entry_for_snapshot(
        &self,
        snapshot: HistoryPolicySnapshot,
        file_name: String,
        post_process_requested: bool,
    ) -> Result<Option<HistoryEntry>> {
        let mut conn = self.get_connection()?;
        let app_handle = &self.app_handle;
        let outcome = Self::save_entry_under_policy_gate_core_in(
            &mut conn,
            &self.recordings_dir,
            Some(&self.canonical_recordings_dir),
            &self.policy_gate,
            snapshot,
            || {
                let s = crate::settings::get_settings(app_handle);
                (s.never_store_history, s.keep_dictation_audio)
            },
            None,
            Self::unlink_recording_strict_bounded,
            true,
            file_name,
            String::new(),
            post_process_requested,
            None,
            None,
            None,
            None,
        )?;

        let entry = match outcome {
            SaveOutcome::Saved(entry) => entry,
            SaveOutcome::AbortedByPolicy { .. } => return Ok(None),
            SaveOutcome::IncompleteAudioInspection {
                file_name, reason, ..
            } => {
                return Err(anyhow!(
                    "{}",
                    Self::format_final_save_incomplete_message(&file_name, reason)
                ));
            }
        };

        debug!("Saved failed retry history entry with id {}", entry.id);
        self.cleanup_old_entries()?;

        if let Err(e) = (HistoryUpdatePayload::Added {
            entry: entry.clone(),
        })
        .emit(&self.app_handle)
        {
            error!("Failed to emit history-updated event: {}", e);
        }

        Ok(Some(entry))
    }

    /// Update an existing history entry with new transcription results (used by retry).
    pub fn update_transcription(
        &self,
        id: i64,
        transcription_text: String,
        post_processed_text: Option<String>,
        post_process_prompt: Option<String>,
    ) -> Result<HistoryEntry> {
        let conn = self.get_connection()?;
        let updated = conn.execute(
            "UPDATE transcription_history
             SET transcription_text = ?1,
                 post_processed_text = ?2,
                 post_process_prompt = ?3
             WHERE id = ?4",
            params![
                transcription_text,
                post_processed_text,
                post_process_prompt,
                id
            ],
        )?;

        if updated == 0 {
            return Err(anyhow!("History entry {} not found", id));
        }

        let entry = conn
            .query_row(
                "SELECT id, file_name, timestamp, saved, title, transcription_text, post_processed_text, post_process_prompt, post_process_requested, app_name, detected_language
                 FROM transcription_history WHERE id = ?1",
                params![id],
                Self::map_history_entry,
            )?;

        debug!("Updated transcription for history entry {}", id);

        if let Err(e) = (HistoryUpdatePayload::Updated {
            entry: entry.clone(),
        })
        .emit(&self.app_handle)
        {
            error!("Failed to emit history-updated event: {}", e);
        }

        Ok(entry)
    }

    pub fn cleanup_old_entries(&self) -> Result<()> {
        let mut guard = self.policy_gate.lock();
        self.cleanup_old_entries_unlocked(&mut guard, false)
    }

    fn cleanup_old_entries_unlocked(
        &self,
        state: &mut HistoryPolicyState,
        drain_active_staged: bool,
    ) -> Result<()> {
        let never_store = crate::settings::get_never_store_history(&self.app_handle);
        let include_active = drain_active_staged || never_store;
        if include_active {
            state.active_staged_wavs.clear();
        }
        let pending_res = self.drain_pending_audio_removal_unlocked(state, include_active);
        let retention_res = if never_store {
            self.cleanup_all_unsaved()
        } else {
            let retention_period =
                crate::settings::get_recording_retention_period(&self.app_handle);

            match retention_period {
                crate::settings::RecordingRetentionPeriod::Never => {
                    // Don't delete anything
                    Ok(())
                }
                crate::settings::RecordingRetentionPeriod::PreserveLimit => {
                    // Use the old count-based logic with history_limit
                    let limit = crate::settings::get_history_limit(&self.app_handle);
                    self.cleanup_by_count(limit)
                }
                _ => {
                    // Use time-based logic
                    self.cleanup_by_time(retention_period)
                }
            }
        };

        retention_res?;
        pending_res.map(|_| ())
    }

    /// Runs retention cleanup once at startup and then hourly on a background task
    /// so expired dictations are removed even when the user takes no new dictations.
    pub fn start_sweeper(self: std::sync::Arc<Self>) {
        if let Err(err) = self.cleanup_old_entries() {
            error!("Initial history retention sweep failed: {}", err);
        }
        tauri::async_runtime::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(3600));
            // Skip immediate first tick since we already ran cleanup_old_entries() above.
            interval.tick().await;
            loop {
                interval.tick().await;
                if let Err(err) = self.cleanup_old_entries() {
                    error!("Hourly history retention sweep failed: {}", err);
                }
            }
        });
    }

    fn cleanup_all_unsaved(&self) -> Result<()> {
        let mut conn = self.get_connection()?;
        let deleted_count = Self::cleanup_all_unsaved_bounded_in(
            &mut conn,
            &self.recordings_dir,
            Some(&self.canonical_recordings_dir),
        )?;
        if deleted_count > 0 {
            debug!(
                "Cleared {} unstarred history entries for Never store mode",
                deleted_count
            );
        }
        Ok(())
    }

    fn cleanup_all_unsaved_bounded_in(
        conn: &mut Connection,
        recordings_dir: &Path,
        expected_canonical_root: Option<&Path>,
    ) -> Result<usize> {
        let entries: Vec<(i64, String)> = {
            let mut stmt =
                conn.prepare("SELECT id, file_name FROM transcription_history WHERE saved = 0")?;
            let rows = stmt.query_map([], |row| {
                Ok((row.get::<_, i64>("id")?, row.get::<_, String>("file_name")?))
            })?;
            let mut out = Vec::new();
            for row in rows {
                out.push(row?);
            }
            out
        };
        Self::delete_entries_and_files_with_hook_in(
            conn,
            recordings_dir,
            expected_canonical_root,
            &entries,
            Self::unlink_recording_strict_bounded,
        )
    }

    #[cfg(test)]
    fn cleanup_all_unsaved_in(conn: &mut Connection, recordings_dir: &Path) -> Result<usize> {
        let canon = fs::canonicalize(recordings_dir).ok();
        Self::cleanup_all_unsaved_bounded_in(conn, recordings_dir, canon.as_deref())
    }

    fn purge_single_unstarred_row_with_hook_in<R>(
        conn: &mut Connection,
        recordings_dir: &Path,
        expected_canonical_root: Option<&Path>,
        id: i64,
        remove_fn: R,
    ) -> Result<bool>
    where
        R: FnOnce(&Path, Option<&Path>, &str) -> Result<()>,
    {
        Self::ensure_pending_audio_removal_table(conn)?;

        // Step 1: ONE SQLite IMMEDIATE transaction verifies `saved = 0`, deletes the row,
        // and records its `file_name` in `pending_audio_removal` BEFORE unlinking (`AC-C19`).
        // A failed unlink therefore never leaves text behind under Never and the row cannot
        // be starred afterwards.
        let file_name = {
            let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            let row_opt: Option<(bool, String)> = tx
                .query_row(
                    "SELECT saved, file_name FROM transcription_history WHERE id = ?1",
                    params![id],
                    |row| {
                        Ok((
                            row.get::<_, bool>("saved")?,
                            row.get::<_, String>("file_name")?,
                        ))
                    },
                )
                .optional()?;

            let Some((saved, file_name)) = row_opt else {
                tx.commit()?;
                return Ok(false);
            };

            if saved {
                // Row was starred after candidate selection — preserve both WAV and DB row.
                tx.commit()?;
                return Ok(false);
            }

            if !file_name.is_empty() {
                // Refuse traversal/malformed paths before touching row or pending table.
                Self::validate_recording_filename(&file_name)?;
            }

            let removed = tx.execute(
                "DELETE FROM transcription_history WHERE id = ?1 AND saved = 0",
                params![id],
            )?;
            if removed == 0 {
                tx.commit()?;
                return Ok(false);
            }

            if !file_name.is_empty() {
                tx.execute(
                    "INSERT OR IGNORE INTO pending_audio_removal (file_name, first_failed_at)
                     VALUES (?1, ?2)",
                    params![&file_name, Utc::now().timestamp()],
                )?;
            }

            tx.commit()?;
            file_name
        };

        if file_name.is_empty() {
            return Ok(true);
        }

        // Step 2 & 3: Unlink via bounded `unlinkat`; delete pending entry ONLY on `Ok(())`.
        // On failure, `file_name` is already durably recorded in `pending_audio_removal`.
        remove_fn(recordings_dir, expected_canonical_root, &file_name)?;
        let _ = Self::delete_pending_audio_removal_in(conn, &file_name);
        Ok(true)
    }

    fn delete_entries_and_files_with_hook_in<R>(
        conn: &mut Connection,
        recordings_dir: &Path,
        expected_canonical_root: Option<&Path>,
        entries: &[(i64, String)],
        mut remove_fn: R,
    ) -> Result<usize>
    where
        R: FnMut(&Path, Option<&Path>, &str) -> Result<()>,
    {
        if entries.is_empty() {
            return Ok(0);
        }

        let mut deleted_count = 0usize;
        let mut failures: Vec<String> = Vec::new();

        for (id, _) in entries {
            match Self::purge_single_unstarred_row_with_hook_in(
                conn,
                recordings_dir,
                expected_canonical_root,
                *id,
                &mut remove_fn,
            ) {
                Ok(true) => deleted_count += 1,
                Ok(false) => {}
                Err(e) => {
                    error!("Incomplete purge for history entry {}: {}", id, e);
                    failures.push(format!("entry {}: {}", id, e));
                }
            }
        }

        if !failures.is_empty() {
            return Err(anyhow!(
                "incomplete history purge ({} deleted, {} failed): {}",
                deleted_count,
                failures.len(),
                failures.join("; ")
            ));
        }

        Ok(deleted_count)
    }

    #[cfg(test)]
    fn delete_entries_and_files_in(
        conn: &mut Connection,
        recordings_dir: &Path,
        entries: &[(i64, String)],
    ) -> Result<usize> {
        let canon = fs::canonicalize(recordings_dir).ok();
        Self::delete_entries_and_files_with_hook_in(
            conn,
            recordings_dir,
            canon.as_deref(),
            entries,
            Self::unlink_recording_strict_bounded,
        )
    }

    fn cleanup_by_count(&self, limit: usize) -> Result<()> {
        let mut conn = self.get_connection()?;

        // Get all entries that are not saved, ordered by timestamp desc
        let entries: Vec<(i64, String)> = {
            let mut stmt = conn.prepare(
                "SELECT id, file_name FROM transcription_history WHERE saved = 0 ORDER BY timestamp DESC",
            )?;

            let rows = stmt.query_map([], |row| {
                Ok((row.get::<_, i64>("id")?, row.get::<_, String>("file_name")?))
            })?;

            let mut out = Vec::new();
            for row in rows {
                out.push(row?);
            }
            out
        };

        if entries.len() > limit {
            let entries_to_delete = &entries[limit..];
            let deleted_count = Self::delete_entries_and_files_with_hook_in(
                &mut conn,
                &self.recordings_dir,
                Some(&self.canonical_recordings_dir),
                entries_to_delete,
                Self::unlink_recording_strict_bounded,
            )?;

            if deleted_count > 0 {
                debug!("Cleaned up {} old history entries by count", deleted_count);
            }
        }

        Ok(())
    }

    fn cleanup_by_time(
        &self,
        retention_period: crate::settings::RecordingRetentionPeriod,
    ) -> Result<()> {
        let mut conn = self.get_connection()?;
        let now = Utc::now().timestamp();
        let deleted_count = Self::cleanup_by_time_bounded_in(
            &mut conn,
            &self.recordings_dir,
            Some(&self.canonical_recordings_dir),
            retention_period,
            now,
        )?;

        if deleted_count > 0 {
            debug!(
                "Cleaned up {} old history entries based on retention period",
                deleted_count
            );
        }

        Ok(())
    }

    fn cleanup_by_time_bounded_in(
        conn: &mut Connection,
        recordings_dir: &Path,
        expected_canonical_root: Option<&Path>,
        retention_period: crate::settings::RecordingRetentionPeriod,
        now: i64,
    ) -> Result<usize> {
        let cutoff_timestamp = match retention_period {
            crate::settings::RecordingRetentionPeriod::Hours24 => now - (24 * 60 * 60),
            crate::settings::RecordingRetentionPeriod::Days3 => now - (3 * 24 * 60 * 60),
            crate::settings::RecordingRetentionPeriod::Weeks2 => now - (2 * 7 * 24 * 60 * 60),
            crate::settings::RecordingRetentionPeriod::Days30 => now - (30 * 24 * 60 * 60),
            crate::settings::RecordingRetentionPeriod::Months3 => now - (3 * 30 * 24 * 60 * 60),
            crate::settings::RecordingRetentionPeriod::Never
            | crate::settings::RecordingRetentionPeriod::PreserveLimit => return Ok(0),
        };

        // Get all unsaved entries older than the cutoff timestamp
        let entries_to_delete: Vec<(i64, String)> = {
            let mut stmt = conn.prepare(
                "SELECT id, file_name FROM transcription_history WHERE saved = 0 AND timestamp < ?1",
            )?;

            let rows = stmt.query_map(params![cutoff_timestamp], |row| {
                Ok((row.get::<_, i64>("id")?, row.get::<_, String>("file_name")?))
            })?;

            let mut out = Vec::new();
            for row in rows {
                out.push(row?);
            }
            out
        };

        Self::delete_entries_and_files_with_hook_in(
            conn,
            recordings_dir,
            expected_canonical_root,
            &entries_to_delete,
            Self::unlink_recording_strict_bounded,
        )
    }

    #[cfg(test)]
    fn cleanup_by_time_in(
        conn: &mut Connection,
        recordings_dir: &Path,
        retention_period: crate::settings::RecordingRetentionPeriod,
        now: i64,
    ) -> Result<usize> {
        let canon = fs::canonicalize(recordings_dir).ok();
        Self::cleanup_by_time_bounded_in(
            conn,
            recordings_dir,
            canon.as_deref(),
            retention_period,
            now,
        )
    }

    pub async fn get_history_entries(
        &self,
        cursor: Option<i64>,
        limit: Option<usize>,
    ) -> Result<PaginatedHistory> {
        let conn = self.get_connection()?;
        let limit = limit.map(|l| l.min(100));

        let mut entries: Vec<HistoryEntry> = match (cursor, limit) {
            (Some(cursor_id), Some(lim)) => {
                let fetch_count = (lim + 1) as i64;
                let mut stmt = conn.prepare(
                    "SELECT id, file_name, timestamp, saved, title, transcription_text, post_processed_text, post_process_prompt, post_process_requested, app_name, detected_language
                     FROM transcription_history
                     WHERE id < ?1
                     ORDER BY id DESC
                     LIMIT ?2",
                )?;
                let result = stmt
                    .query_map(params![cursor_id, fetch_count], Self::map_history_entry)?
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                result
            }
            (None, Some(lim)) => {
                let fetch_count = (lim + 1) as i64;
                let mut stmt = conn.prepare(
                    "SELECT id, file_name, timestamp, saved, title, transcription_text, post_processed_text, post_process_prompt, post_process_requested, app_name, detected_language
                     FROM transcription_history
                     ORDER BY id DESC
                     LIMIT ?1",
                )?;
                let result = stmt
                    .query_map(params![fetch_count], Self::map_history_entry)?
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                result
            }
            (_, None) => {
                let mut stmt = conn.prepare(
                    "SELECT id, file_name, timestamp, saved, title, transcription_text, post_processed_text, post_process_prompt, post_process_requested, app_name, detected_language
                     FROM transcription_history
                     ORDER BY id DESC",
                )?;
                let result = stmt
                    .query_map([], Self::map_history_entry)?
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                result
            }
        };

        let has_more = limit.is_some_and(|lim| entries.len() > lim);
        if has_more {
            entries.pop();
        }

        Ok(PaginatedHistory { entries, has_more })
    }

    #[cfg(test)]
    fn get_latest_entry_with_conn(conn: &Connection) -> Result<Option<HistoryEntry>> {
        let mut stmt = conn.prepare(
            "SELECT
                id,
                file_name,
                timestamp,
                saved,
                title,
                transcription_text,
                post_processed_text,
                post_process_prompt,
                post_process_requested,
                app_name,
                detected_language
             FROM transcription_history
             ORDER BY timestamp DESC
             LIMIT 1",
        )?;

        let entry = stmt.query_row([], Self::map_history_entry).optional()?;
        Ok(entry)
    }

    /// Get the latest entry with non-empty transcription text.
    pub fn get_latest_completed_entry(&self) -> Result<Option<HistoryEntry>> {
        let conn = self.get_connection()?;
        Self::get_latest_completed_entry_with_conn(&conn)
    }

    fn get_latest_completed_entry_with_conn(conn: &Connection) -> Result<Option<HistoryEntry>> {
        let mut stmt = conn.prepare(
            "SELECT
                id,
                file_name,
                timestamp,
                saved,
                title,
                transcription_text,
                post_processed_text,
                post_process_prompt,
                post_process_requested,
                app_name,
                detected_language
             FROM transcription_history
             WHERE transcription_text != ''
             ORDER BY timestamp DESC
             LIMIT 1",
        )?;

        let entry = stmt.query_row([], Self::map_history_entry).optional()?;
        Ok(entry)
    }

    pub async fn toggle_saved_status(&self, id: i64) -> Result<()> {
        {
            // Serialize starring under `policy_gate` so it cannot interleave with
            // deferred purge drains rechecking `saved = 1` (Item 1).
            let _guard = self.policy_gate.lock();
            let conn = self.get_connection()?;

            let updated = conn.execute(
                "UPDATE transcription_history SET saved = NOT saved WHERE id = ?1",
                params![id],
            )?;
            if updated == 0 {
                return Err(anyhow!("History entry {} not found", id));
            }
        }

        debug!("Toggled saved status for entry {}", id);

        // Emit history updated event
        if let Err(e) = (HistoryUpdatePayload::Toggled { id }).emit(&self.app_handle) {
            error!("Failed to emit history-updated event: {}", e);
        }

        Ok(())
    }

    pub fn get_audio_file_path(&self, file_name: &str) -> PathBuf {
        self.recordings_dir.join(file_name)
    }

    pub async fn get_entry_by_id(&self, id: i64) -> Result<Option<HistoryEntry>> {
        let conn = self.get_connection()?;
        let mut stmt = conn.prepare(
            "SELECT
                id,
                file_name,
                timestamp,
                saved,
                title,
                transcription_text,
                post_processed_text,
                post_process_prompt,
                post_process_requested,
                app_name,
                detected_language
             FROM transcription_history
             WHERE id = ?1",
        )?;

        let entry = stmt.query_row([id], Self::map_history_entry).optional()?;

        Ok(entry)
    }

    fn delete_entry_bounded_in(
        conn: &mut Connection,
        recordings_dir: &Path,
        expected_canonical_root: Option<&Path>,
        id: i64,
    ) -> Result<bool> {
        Self::ensure_pending_audio_removal_table(conn)?;
        let file_name = {
            let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            let file_name_opt: Option<String> = tx
                .query_row(
                    "SELECT file_name FROM transcription_history WHERE id = ?1",
                    params![id],
                    |row| row.get::<_, String>("file_name"),
                )
                .optional()?;

            let Some(file_name) = file_name_opt else {
                tx.commit()?;
                return Ok(false);
            };

            if !file_name.is_empty() {
                Self::validate_recording_filename(&file_name)?;
            }

            let removed = tx.execute(
                "DELETE FROM transcription_history WHERE id = ?1",
                params![id],
            )?;
            if removed == 0 {
                tx.commit()?;
                return Ok(false);
            }

            if !file_name.is_empty() {
                tx.execute(
                    "INSERT OR IGNORE INTO pending_audio_removal (file_name, first_failed_at)
                     VALUES (?1, ?2)",
                    params![&file_name, Utc::now().timestamp()],
                )?;
            }

            tx.commit()?;
            file_name
        };

        if file_name.is_empty() {
            return Ok(true);
        }

        // Unlink WAV strictly against pinned canonical root; delete pending record on `Ok(())`.
        Self::unlink_recording_strict_bounded(recordings_dir, expected_canonical_root, &file_name)?;
        let _ = Self::delete_pending_audio_removal_in(conn, &file_name);
        Ok(true)
    }

    #[cfg(test)]
    fn delete_entry_in(conn: &mut Connection, recordings_dir: &Path, id: i64) -> Result<bool> {
        let canon = fs::canonicalize(recordings_dir).ok();
        Self::delete_entry_bounded_in(conn, recordings_dir, canon.as_deref(), id)
    }

    pub async fn delete_entry(&self, id: i64) -> Result<()> {
        {
            let _guard = self.policy_gate.lock();
            let mut conn = self.get_connection()?;
            Self::delete_entry_bounded_in(
                &mut conn,
                &self.recordings_dir,
                Some(&self.canonical_recordings_dir),
                id,
            )?;
        }

        debug!("Deleted history entry with id: {}", id);

        // Emit history updated event
        if let Err(e) = (HistoryUpdatePayload::Deleted { id }).emit(&self.app_handle) {
            error!("Failed to emit history-updated event: {}", e);
        }

        Ok(())
    }

    fn format_timestamp_title_static(timestamp: i64) -> String {
        if let Some(utc_datetime) = DateTime::from_timestamp(timestamp, 0) {
            // Convert UTC to local timezone
            let local_datetime = utc_datetime.with_timezone(&Local);
            local_datetime.format("%B %e, %Y - %l:%M%p").to_string()
        } else {
            format!("Recording {}", timestamp)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::{params, Connection};

    fn setup_conn() -> Connection {
        let conn = Connection::open_in_memory().expect("open in-memory db");
        conn.execute_batch(
            "CREATE TABLE transcription_history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                file_name TEXT NOT NULL,
                timestamp INTEGER NOT NULL,
                saved BOOLEAN NOT NULL DEFAULT 0,
                title TEXT NOT NULL,
                transcription_text TEXT NOT NULL,
                post_processed_text TEXT,
                post_process_prompt TEXT,
                post_process_requested BOOLEAN NOT NULL DEFAULT 0,
                app_name TEXT,
                detected_language TEXT
            );",
        )
        .expect("create transcription_history table");
        conn
    }

    fn insert_entry(conn: &Connection, timestamp: i64, text: &str, post_processed: Option<&str>) {
        conn.execute(
            "INSERT INTO transcription_history (
                file_name,
                timestamp,
                saved,
                title,
                transcription_text,
                post_processed_text,
                post_process_prompt,
                post_process_requested
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                format!("coco-voice-{}.wav", timestamp),
                timestamp,
                false,
                format!("Recording {}", timestamp),
                text,
                post_processed,
                Option::<String>::None,
                false,
            ],
        )
        .expect("insert history entry");
    }

    #[test]
    fn get_latest_entry_returns_none_when_empty() {
        let conn = setup_conn();
        let entry = HistoryManager::get_latest_entry_with_conn(&conn).expect("fetch latest entry");
        assert!(entry.is_none());
    }

    #[test]
    fn get_latest_entry_returns_newest_entry() {
        let conn = setup_conn();
        insert_entry(&conn, 100, "first", None);
        insert_entry(&conn, 200, "second", Some("processed"));

        let entry = HistoryManager::get_latest_entry_with_conn(&conn)
            .expect("fetch latest entry")
            .expect("entry exists");

        assert_eq!(entry.timestamp, 200);
        assert_eq!(entry.transcription_text, "second");
        assert_eq!(entry.post_processed_text.as_deref(), Some("processed"));
    }

    #[test]
    fn get_latest_completed_entry_skips_empty_entries() {
        let conn = setup_conn();
        insert_entry(&conn, 100, "completed", None);
        insert_entry(&conn, 200, "", None);

        let entry = HistoryManager::get_latest_completed_entry_with_conn(&conn)
            .expect("fetch latest completed entry")
            .expect("completed entry exists");

        assert_eq!(entry.timestamp, 100);
        assert_eq!(entry.transcription_text, "completed");
    }

    #[test]
    fn transcription_history_survives_app_restart() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let db_path = dir.path().join("history.db");

        {
            let mut conn = Connection::open(&db_path).expect("open history db");
            let migrations = super::Migrations::new(super::MIGRATIONS.to_vec());
            migrations.validate().expect("Invalid migrations");
            migrations.to_latest(&mut conn).expect("apply migrations");
            insert_entry(&conn, 100, "first", None);
            insert_entry(&conn, 200, "second", None);
        }

        let conn = Connection::open(&db_path).expect("reopen history db");
        let mut stmt = conn
            .prepare(
                "SELECT id, file_name, timestamp, saved, title, transcription_text, post_processed_text, post_process_prompt, post_process_requested, app_name, detected_language
                 FROM transcription_history
                 ORDER BY id DESC",
            )
            .expect("prepare history query");
        let entries = stmt
            .query_map([], HistoryManager::map_history_entry)
            .expect("query history")
            .collect::<std::result::Result<Vec<_>, _>>()
            .expect("read history rows");

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].timestamp, 200);
        assert_eq!(entries[0].transcription_text, "second");
        assert_eq!(entries[1].timestamp, 100);
        assert_eq!(entries[1].transcription_text, "first");

        let latest = HistoryManager::get_latest_entry_with_conn(&conn)
            .expect("fetch latest entry")
            .expect("entry exists");
        assert_eq!(latest.timestamp, 200);
        assert_eq!(latest.transcription_text, "second");
    }

    #[test]
    fn time_retention_supports_24h_30d_forever_and_text_only_entries_without_touching_recordings_dir(
    ) {
        use crate::settings::RecordingRetentionPeriod;

        let dir = tempfile::tempdir().expect("create temp dir");
        let recordings_dir = dir.path().join("recordings");
        fs::create_dir_all(&recordings_dir).expect("create recordings dir");

        let mut conn = setup_conn();
        let now: i64 = 1_760_000_000;
        // 1) Old text-only entry (35 days old, empty file_name -> no WAV on disk)
        conn.execute(
            "INSERT INTO transcription_history (file_name, timestamp, saved, title, transcription_text, post_process_requested)
             VALUES ('', ?1, 0, 'Old text', 'old 35d', 0)",
            params![now - 35 * 86_400],
        )
        .unwrap();
        // 2) 3-day-old entry with WAV on disk
        let wav_3d = recordings_dir.join("take-3d.wav");
        fs::write(&wav_3d, b"RIFF").unwrap();
        conn.execute(
            "INSERT INTO transcription_history (file_name, timestamp, saved, title, transcription_text, post_process_requested)
             VALUES ('take-3d.wav', ?1, 0, '3d', 'mid 3d', 0)",
            params![now - 3 * 86_400],
        )
        .unwrap();
        // 3) Fresh text-only entry (1 hour old)
        conn.execute(
            "INSERT INTO transcription_history (file_name, timestamp, saved, title, transcription_text, post_process_requested)
             VALUES ('', ?1, 0, 'Fresh', 'fresh 1h', 0)",
            params![now - 3_600],
        )
        .unwrap();

        // Forever (Never) deletes nothing
        let deleted_forever = HistoryManager::cleanup_by_time_in(
            &mut conn,
            &recordings_dir,
            RecordingRetentionPeriod::Never,
            now,
        )
        .unwrap();
        assert_eq!(deleted_forever, 0);

        // 30 days deletes only the 35-day text-only row and keeps recordings_dir intact
        let deleted_30d = HistoryManager::cleanup_by_time_in(
            &mut conn,
            &recordings_dir,
            RecordingRetentionPeriod::Days30,
            now,
        )
        .unwrap();
        assert_eq!(deleted_30d, 1);
        assert!(recordings_dir.is_dir());
        assert!(wav_3d.exists());

        // 24 hours deletes the 3-day row + its WAV, keeping the 1-hour text-only row
        let deleted_24h = HistoryManager::cleanup_by_time_in(
            &mut conn,
            &recordings_dir,
            RecordingRetentionPeriod::Hours24,
            now,
        )
        .unwrap();
        assert_eq!(deleted_24h, 1);
        assert!(!wav_3d.exists());
        assert!(recordings_dir.is_dir());

        let remaining: i64 = conn
            .query_row("SELECT COUNT(*) FROM transcription_history", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(remaining, 1);
    }

    #[test]
    fn never_mode_purges_all_unstarred_rows_and_wavs_while_preserving_starred_and_sweeps_3000_rows_fast(
    ) {
        use crate::settings::RecordingRetentionPeriod;

        let dir = tempfile::tempdir().expect("create temp dir");
        let recordings_dir = dir.path().join("recordings");
        fs::create_dir_all(&recordings_dir).expect("create recordings dir");

        let mut conn = setup_conn();
        let now: i64 = 1_760_000_000;

        // Seed AC-C6 matrix: 2h, 2d, 40d, 400d + 1 starred row with WAVs
        for (idx, (age_secs, saved)) in [
            (2 * 3_600, false),
            (2 * 86_400, false),
            (40 * 86_400, false),
            (400 * 86_400, false),
            (400 * 86_400, true),
        ]
        .into_iter()
        .enumerate()
        {
            let fname = format!("take-{idx}.wav");
            fs::write(recordings_dir.join(&fname), b"RIFF").unwrap();
            conn.execute(
                "INSERT INTO transcription_history (file_name, timestamp, saved, title, transcription_text, post_process_requested)
                 VALUES (?1, ?2, ?3, 'T', 'hello', 0)",
                params![fname, now - age_secs, saved],
            )
            .unwrap();
        }

        let deleted = HistoryManager::cleanup_all_unsaved_in(&mut conn, &recordings_dir).unwrap();
        assert_eq!(deleted, 4);
        let remaining: i64 = conn
            .query_row("SELECT COUNT(*) FROM transcription_history", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(remaining, 1);
        assert!(!recordings_dir.join("take-0.wav").exists());
        assert!(!recordings_dir.join("take-3.wav").exists());
        assert!(recordings_dir.join("take-4.wav").exists());

        // Performance check (Plan C1 Part 5): sweeping 3,000 rows finishes well under 2 s.
        let mut perf_conn = setup_conn();
        perf_conn.execute_batch("BEGIN").unwrap();
        for i in 0..3_000_i64 {
            perf_conn
                .execute(
                    "INSERT INTO transcription_history (file_name, timestamp, saved, title, transcription_text, post_process_requested)
                     VALUES ('', ?1, 0, 'Row', 'bulk row', 0)",
                    params![now - (48 * 3_600) - i],
                )
                .unwrap();
        }
        perf_conn.execute_batch("COMMIT").unwrap();

        let started = std::time::Instant::now();
        let purged = HistoryManager::cleanup_by_time_in(
            &mut perf_conn,
            &recordings_dir,
            RecordingRetentionPeriod::Hours24,
            now,
        )
        .unwrap();
        assert_eq!(purged, 3_000);
        assert!(
            started.elapsed() < std::time::Duration::from_secs(2),
            "3,000-row cleanup took {:?}, exceeding 2 s target",
            started.elapsed()
        );
    }

    #[cfg(unix)]
    #[test]
    fn purge_deletes_row_and_records_pending_when_wav_unlink_fails_then_drains_on_retry() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().expect("create temp dir");
        let recordings_dir = dir.path().join("recordings");
        fs::create_dir_all(&recordings_dir).expect("create recordings dir");
        let canon_root = fs::canonicalize(&recordings_dir).unwrap();

        let locked_wav = recordings_dir.join("locked.wav");
        fs::write(&locked_wav, b"RIFF").unwrap();

        let mut conn = setup_conn();
        conn.execute(
            "INSERT INTO transcription_history (file_name, timestamp, saved, title, transcription_text, post_process_requested)
             VALUES ('locked.wav', 1000, 0, 'Locked', 'locked take', 0)",
            [],
        )
        .unwrap();

        // Make recordings_dir read-only so unlinking locked.wav fails with EACCES.
        fs::set_permissions(&recordings_dir, fs::Permissions::from_mode(0o555)).unwrap();

        let purge_res = HistoryManager::cleanup_all_unsaved_in(&mut conn, &recordings_dir);
        assert!(
            purge_res.is_err(),
            "purge must report incomplete failure when WAV unlink fails"
        );

        // Under Amendment 3 (AC-C19), the DB row is deleted BEFORE unlink in the same
        // transaction that inserts `locked.wav` into `pending_audio_removal`.
        let remaining: i64 = conn
            .query_row("SELECT COUNT(*) FROM transcription_history", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(remaining, 0);
        assert_eq!(
            HistoryManager::pending_audio_removal_count_in(&conn).unwrap(),
            1
        );
        assert!(locked_wav.exists());

        // Restore write permissions and drain pending_audio_removal -> WAV removed and table cleared.
        fs::set_permissions(&recordings_dir, fs::Permissions::from_mode(0o755)).unwrap();
        let drained = HistoryManager::drain_pending_audio_removal_in(
            &conn,
            &recordings_dir,
            Some(&canon_root),
        )
        .unwrap();
        assert_eq!(drained, 1);
        assert!(!locked_wav.exists());
        assert_eq!(
            HistoryManager::pending_audio_removal_count_in(&conn).unwrap(),
            0
        );

        // Manual delete_entry_in follows the same row-first + pending_audio_removal contract.
        let manual_wav = recordings_dir.join("manual-locked.wav");
        fs::write(&manual_wav, b"RIFF").unwrap();
        conn.execute(
            "INSERT INTO transcription_history (id, file_name, timestamp, saved, title, transcription_text, post_process_requested)
             VALUES (2, 'manual-locked.wav', 1001, 0, 'Manual', 'manual take', 0)",
            [],
        )
        .unwrap();
        assert!(HistoryManager::delete_entry_in(&mut conn, &recordings_dir, 2).unwrap());
        assert!(!manual_wav.exists());
        assert_eq!(
            HistoryManager::pending_audio_removal_count_in(&conn).unwrap(),
            0
        );
    }

    #[test]
    fn safe_recording_path_and_purge_refuse_traversal_and_symlinks() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let recordings_dir = dir.path().join("recordings");
        fs::create_dir_all(&recordings_dir).expect("create recordings dir");

        let outside = dir.path().join("outside-secret.wav");
        fs::write(&outside, b"DO-NOT-DELETE").unwrap();

        for bad in [
            "../outside-secret.wav",
            "/tmp/abs.wav",
            "sub/nested.wav",
            "sub\\nested.wav",
            ".hidden.wav",
            "..",
            ".",
            "   ",
            " leading.wav",
            "bad\0name.wav",
        ] {
            assert!(
                HistoryManager::safe_recording_path_in(&recordings_dir, bad).is_err(),
                "expected safe_recording_path_in to reject {bad:?}"
            );
        }

        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&outside, recordings_dir.join("evil-link.wav")).unwrap();
            assert!(
                HistoryManager::safe_recording_path_in(&recordings_dir, "evil-link.wav").is_err(),
                "symlink inside recordings_dir must be refused"
            );
        }

        let mut conn = setup_conn();
        conn.execute(
            "INSERT INTO transcription_history (file_name, timestamp, saved, title, transcription_text, post_process_requested)
             VALUES ('../outside-secret.wav', 1000, 0, 'Bad', 'traversal', 0)",
            [],
        )
        .unwrap();
        #[cfg(unix)]
        conn.execute(
            "INSERT INTO transcription_history (file_name, timestamp, saved, title, transcription_text, post_process_requested)
             VALUES ('evil-link.wav', 1001, 0, 'Link', 'symlink', 0)",
            [],
        )
        .unwrap();

        let res = HistoryManager::cleanup_all_unsaved_in(&mut conn, &recordings_dir);
        assert!(res.is_err());
        assert_eq!(fs::read(&outside).unwrap(), b"DO-NOT-DELETE");
        let remaining: i64 = conn
            .query_row("SELECT COUNT(*) FROM transcription_history", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert!(remaining >= 1);
    }

    #[test]
    fn row_starred_after_stale_selection_and_concurrently_during_purge_survives_intact() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let recordings_dir = dir.path().join("recordings");
        fs::create_dir_all(&recordings_dir).expect("create recordings dir");

        let wav1 = recordings_dir.join("take-1.wav");
        let wav2 = recordings_dir.join("take-2.wav");
        fs::write(&wav1, b"RIFF-1").unwrap();
        fs::write(&wav2, b"RIFF-2").unwrap();

        let mut conn = setup_conn();
        conn.execute(
            "INSERT INTO transcription_history (id, file_name, timestamp, saved, title, transcription_text, post_process_requested)
             VALUES (1, 'take-1.wav', 1000, 0, 'One', 'first', 0),
                    (2, 'take-2.wav', 1001, 0, 'Two', 'second', 0)",
            [],
        )
        .unwrap();

        // Simulate stale candidate selection captured while both rows had saved = 0.
        let stale_candidates = vec![
            (1_i64, "take-1.wav".to_string()),
            (2_i64, "take-2.wav".to_string()),
        ];

        // User stars row 2 AFTER candidate selection and BEFORE purge executes on row 2.
        conn.execute(
            "UPDATE transcription_history SET saved = 1 WHERE id = 2",
            [],
        )
        .unwrap();

        let deleted = HistoryManager::delete_entries_and_files_in(
            &mut conn,
            &recordings_dir,
            &stale_candidates,
        )
        .unwrap();
        assert_eq!(deleted, 1);
        assert!(!wav1.exists(), "unstarred take-1.wav should be deleted");
        assert!(wav2.exists(), "starred take-2.wav must survive purge");

        let (saved_2, file_2): (bool, String) = conn
            .query_row(
                "SELECT saved, file_name FROM transcription_history WHERE id = 2",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert!(saved_2);
        assert_eq!(file_2, "take-2.wav");

        // Multi-threaded SQLite file concurrency test: concurrent starring vs purge never
        // deletes a starred row or leaves a surviving row with a missing WAV.
        let db_path = dir.path().join("concurrent.db");
        {
            let setup = Connection::open(&db_path).unwrap();
            setup
                .execute_batch(
                    "CREATE TABLE transcription_history (
                        id INTEGER PRIMARY KEY AUTOINCREMENT,
                        file_name TEXT NOT NULL,
                        timestamp INTEGER NOT NULL,
                        saved BOOLEAN NOT NULL DEFAULT 0,
                        title TEXT NOT NULL,
                        transcription_text TEXT NOT NULL,
                        post_processed_text TEXT,
                        post_process_prompt TEXT,
                        post_process_requested BOOLEAN NOT NULL DEFAULT 0,
                        app_name TEXT,
                        detected_language TEXT
                    );",
                )
                .unwrap();
            for i in 1..=20_i64 {
                let fname = format!("race-{i}.wav");
                fs::write(recordings_dir.join(&fname), b"RIFF").unwrap();
                setup
                    .execute(
                        "INSERT INTO transcription_history (id, file_name, timestamp, saved, title, transcription_text, post_process_requested)
                         VALUES (?1, ?2, ?3, 0, 'R', 'race', 0)",
                        params![i, fname, 2000 + i],
                    )
                    .unwrap();
            }
        }

        let db_star = db_path.clone();
        let db_purge = db_path.clone();
        let rec_purge = recordings_dir.clone();

        let star_thread = std::thread::spawn(move || {
            let c = Connection::open(&db_star).unwrap();
            let _ = c.busy_timeout(Duration::from_secs(5));
            for i in (2..=20_i64).step_by(2) {
                let _ = c.execute(
                    "UPDATE transcription_history SET saved = 1 WHERE id = ?1",
                    params![i],
                );
            }
        });
        let purge_thread = std::thread::spawn(move || {
            let mut c = Connection::open(&db_purge).unwrap();
            let _ = c.busy_timeout(Duration::from_secs(5));
            HistoryManager::cleanup_all_unsaved_in(&mut c, &rec_purge).unwrap();
        });
        star_thread.join().unwrap();
        purge_thread.join().unwrap();

        let verify = Connection::open(&db_path).unwrap();
        let mut st = verify
            .prepare("SELECT id, saved, file_name FROM transcription_history")
            .unwrap();
        let surviving: Vec<(i64, bool, String)> = st
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();
        for (id, saved, fname) in surviving {
            assert!(
                saved,
                "surviving row {id} after Never purge must be starred"
            );
            assert!(
                recordings_dir.join(&fname).exists(),
                "surviving starred row {id} must still have its WAV {fname} on disk"
            );
        }
    }

    fn setup_file_db(db_path: &Path) {
        let mut conn = Connection::open(db_path).expect("open file db");
        let migrations = Migrations::new(MIGRATIONS.to_vec());
        migrations.to_latest(&mut conn).expect("apply migrations");
        HistoryManager::ensure_pending_audio_removal_table(&conn)
            .expect("create pending_audio_removal table");
    }

    #[test]
    fn ac_c14_serialized_policy_boundary_both_orders_50_runs_never_and_audio_off() {
        use std::sync::atomic::AtomicBool;
        use std::sync::Barrier;

        // 1) Never mode — Order A (50 runs): take stages WAV and hits barrier before save ->
        //    Never + purge runs -> take releases -> 0 rows & 0 WAVs on every single run.
        for round in 0..50_usize {
            let dir = tempfile::tempdir().expect("tempdir");
            let recordings_dir = dir.path().join("recordings");
            fs::create_dir_all(&recordings_dir).expect("recordings_dir");
            let db_path = dir.path().join("history.db");
            setup_file_db(&db_path);

            let policy_gate = HistoryPolicyGate::new();
            let never_store = Arc::new(AtomicBool::new(false));
            let keep_audio = Arc::new(AtomicBool::new(true));

            let snapshot = policy_gate.snapshot();
            let in_flight_name = HistoryManager::next_recording_file_name();
            let in_flight_wav = recordings_dir.join(&in_flight_name);
            crate::audio_toolkit::save_wav_file_exclusive(&in_flight_wav, &[0.2_f32; 32]).unwrap();

            let reached = Arc::new(Barrier::new(2));
            let release = Arc::new(Barrier::new(2));

            let gate_a = policy_gate.clone();
            let db_a = db_path.clone();
            let rec_a = recordings_dir.clone();
            let never_a = Arc::clone(&never_store);
            let audio_a = Arc::clone(&keep_audio);
            let r_a = Arc::clone(&reached);
            let rel_a = Arc::clone(&release);
            let fname_a = in_flight_name.clone();

            let thread_a = std::thread::spawn(move || {
                let mut conn = Connection::open(&db_a).unwrap();
                let _ = conn.busy_timeout(Duration::from_secs(5));
                let hook = || {
                    r_a.wait();
                    rel_a.wait();
                };
                HistoryManager::save_entry_under_policy_gate_in(
                    &mut conn,
                    &rec_a,
                    &gate_a,
                    snapshot,
                    || {
                        (
                            never_a.load(Ordering::SeqCst),
                            audio_a.load(Ordering::SeqCst),
                        )
                    },
                    Some(&hook),
                    fname_a,
                    format!("in-flight never order A round {round}"),
                    false,
                    None,
                    None,
                    None,
                    None,
                )
                .unwrap()
            });

            let gate_b = policy_gate.clone();
            let db_b = db_path.clone();
            let rec_b = recordings_dir.clone();
            let never_b = Arc::clone(&never_store);
            let r_b = Arc::clone(&reached);
            let rel_b = Arc::clone(&release);

            let thread_b = std::thread::spawn(move || {
                r_b.wait();
                gate_b
                    .run_policy_transition(true, false, |_| {
                        never_b.store(true, Ordering::SeqCst);
                        let mut conn = Connection::open(&db_b)?;
                        let _ = conn.busy_timeout(Duration::from_secs(5));
                        HistoryManager::cleanup_all_unsaved_in(&mut conn, &rec_b)?;
                        Ok(())
                    })
                    .unwrap();
                rel_b.wait();
            });

            let outcome = thread_a.join().unwrap();
            thread_b.join().unwrap();

            assert!(
                matches!(
                    outcome,
                    SaveOutcome::AbortedByPolicy {
                        wav_cleanup_pending: false
                    }
                ),
                "Never order A round {round} failed: {outcome:?}"
            );
            let verify = Connection::open(&db_path).unwrap();
            let row_count: i64 = verify
                .query_row("SELECT COUNT(*) FROM transcription_history", [], |r| {
                    r.get(0)
                })
                .unwrap();
            assert_eq!(row_count, 0, "Never order A round {round}: expected 0 rows");
            assert_eq!(
                fs::read_dir(&recordings_dir).unwrap().count(),
                0,
                "Never order A round {round}: expected 0 WAVs"
            );
        }

        // 2) Never mode — Order B (50 runs): take saves first -> Never + purge runs -> 0 rows & 0 WAVs.
        for round in 0..50_usize {
            let dir = tempfile::tempdir().expect("tempdir");
            let recordings_dir = dir.path().join("recordings");
            fs::create_dir_all(&recordings_dir).expect("recordings_dir");
            let db_path = dir.path().join("history.db");
            setup_file_db(&db_path);

            let policy_gate = HistoryPolicyGate::new();
            let snapshot = policy_gate.snapshot();
            let fname = HistoryManager::next_recording_file_name();
            crate::audio_toolkit::save_wav_file_exclusive(
                &recordings_dir.join(&fname),
                &[0.2_f32; 32],
            )
            .unwrap();

            let mut conn = Connection::open(&db_path).unwrap();
            let saved = HistoryManager::save_entry_under_policy_gate_in(
                &mut conn,
                &recordings_dir,
                &policy_gate,
                snapshot,
                || (false, true),
                None,
                fname,
                format!("in-flight never order B round {round}"),
                false,
                None,
                None,
                None,
                None,
            )
            .unwrap();
            assert!(matches!(saved, SaveOutcome::Saved(_)));

            policy_gate
                .run_policy_transition(true, false, |_| {
                    let mut c = Connection::open(&db_path)?;
                    HistoryManager::cleanup_all_unsaved_in(&mut c, &recordings_dir)?;
                    Ok(())
                })
                .unwrap();

            let row_count: i64 = conn
                .query_row("SELECT COUNT(*) FROM transcription_history", [], |r| {
                    r.get(0)
                })
                .unwrap();
            assert_eq!(row_count, 0, "Never order B round {round}: expected 0 rows");
            assert_eq!(
                fs::read_dir(&recordings_dir).unwrap().count(),
                0,
                "Never order B round {round}: expected 0 WAVs"
            );
        }

        // 3) Keep the audio OFF — Order A (50 runs): take stages WAV, hits barrier -> audio turns OFF ->
        //    take releases -> row stays with file_name == "" and 0 WAVs on disk.
        for round in 0..50_usize {
            let dir = tempfile::tempdir().expect("tempdir");
            let recordings_dir = dir.path().join("recordings");
            fs::create_dir_all(&recordings_dir).expect("recordings_dir");
            let db_path = dir.path().join("history.db");
            setup_file_db(&db_path);

            let policy_gate = HistoryPolicyGate::new();
            let keep_audio = Arc::new(AtomicBool::new(true));
            let snapshot = policy_gate.snapshot();

            let fname = HistoryManager::next_recording_file_name();
            let wav_path = recordings_dir.join(&fname);
            crate::audio_toolkit::save_wav_file_exclusive(&wav_path, &[0.2_f32; 32]).unwrap();

            let reached = Arc::new(Barrier::new(2));
            let release = Arc::new(Barrier::new(2));

            let gate_a = policy_gate.clone();
            let db_a = db_path.clone();
            let rec_a = recordings_dir.clone();
            let audio_a = Arc::clone(&keep_audio);
            let r_a = Arc::clone(&reached);
            let rel_a = Arc::clone(&release);
            let fname_a = fname.clone();

            let thread_a = std::thread::spawn(move || {
                let mut conn = Connection::open(&db_a).unwrap();
                let hook = || {
                    r_a.wait();
                    rel_a.wait();
                };
                HistoryManager::save_entry_under_policy_gate_in(
                    &mut conn,
                    &rec_a,
                    &gate_a,
                    snapshot,
                    || (false, audio_a.load(Ordering::SeqCst)),
                    Some(&hook),
                    fname_a,
                    format!("audio-off order A round {round}"),
                    false,
                    None,
                    None,
                    None,
                    None,
                )
                .unwrap()
            });

            let gate_b = policy_gate.clone();
            let audio_b = Arc::clone(&keep_audio);
            let r_b = Arc::clone(&reached);
            let rel_b = Arc::clone(&release);
            let thread_b = std::thread::spawn(move || {
                r_b.wait();
                gate_b
                    .run_policy_transition(false, true, |_| {
                        audio_b.store(false, Ordering::SeqCst);
                        Ok(())
                    })
                    .unwrap();
                rel_b.wait();
            });

            let outcome = thread_a.join().unwrap();
            thread_b.join().unwrap();

            match outcome {
                SaveOutcome::Saved(entry) => {
                    assert!(
                        entry.file_name.is_empty(),
                        "audio-off order A round {round}: expected empty file_name"
                    );
                }
                other => panic!("audio-off order A round {round}: unexpected {other:?}"),
            }
            assert_eq!(
                fs::read_dir(&recordings_dir).unwrap().count(),
                0,
                "audio-off order A round {round}: expected 0 WAVs"
            );
        }

        // 4) Keep the audio OFF — Order B (50 runs): audio turns OFF first, then take runs with
        //    pre-flip snapshot -> row stays with file_name == "" and 0 WAVs on disk.
        for round in 0..50_usize {
            let dir = tempfile::tempdir().expect("tempdir");
            let recordings_dir = dir.path().join("recordings");
            fs::create_dir_all(&recordings_dir).expect("recordings_dir");
            let db_path = dir.path().join("history.db");
            setup_file_db(&db_path);

            let policy_gate = HistoryPolicyGate::new();
            let snapshot = policy_gate.snapshot();
            let fname = HistoryManager::next_recording_file_name();
            crate::audio_toolkit::save_wav_file_exclusive(
                &recordings_dir.join(&fname),
                &[0.2_f32; 32],
            )
            .unwrap();

            policy_gate
                .run_policy_transition(false, true, |_| Ok(()))
                .unwrap();

            let mut conn = Connection::open(&db_path).unwrap();
            let outcome = HistoryManager::save_entry_under_policy_gate_in(
                &mut conn,
                &recordings_dir,
                &policy_gate,
                snapshot,
                || (false, false),
                None,
                fname,
                format!("audio-off order B round {round}"),
                false,
                None,
                None,
                None,
                None,
            )
            .unwrap();
            match outcome {
                SaveOutcome::Saved(entry) => assert!(entry.file_name.is_empty()),
                other => panic!("audio-off order B round {round}: unexpected {other:?}"),
            }
            assert_eq!(fs::read_dir(&recordings_dir).unwrap().count(), 0);
        }
        println!("AC_C14_PROOF: never_order_a=50 never_order_b=50 audio_off_order_a=50 audio_off_order_b=50 all_passed=true");
    }

    #[test]
    fn ac_c15_pending_audio_removal_survives_restart_and_drains_when_fault_cleared() {
        use std::sync::atomic::AtomicBool;

        let dir = tempfile::tempdir().expect("tempdir");
        let recordings_dir = dir.path().join("recordings");
        fs::create_dir_all(&recordings_dir).unwrap();
        let canon_root = fs::canonicalize(&recordings_dir).unwrap();
        let db_path = dir.path().join("history.db");
        setup_file_db(&db_path);

        let fail_remove = Arc::new(AtomicBool::new(true));
        let make_hook = |flag: Arc<AtomicBool>| {
            move |dir: &Path, expected: Option<&Path>, fname: &str| -> Result<()> {
                if flag.load(Ordering::SeqCst) {
                    return Err(anyhow!("injected remove failure for '{fname}'"));
                }
                HistoryManager::unlink_recording_strict_bounded(dir, expected, fname)
            }
        };

        let f_inflight_never = "ac15-inflight-never.wav".to_string();
        let f_audio_off = "ac15-audio-off.wav".to_string();
        let f_never_purge = "ac15-never-purge.wav".to_string();
        let f_time_cleanup = "ac15-time-cleanup.wav".to_string();
        for f in [
            &f_inflight_never,
            &f_audio_off,
            &f_never_purge,
            &f_time_cleanup,
        ] {
            crate::audio_toolkit::save_wav_file_exclusive(&recordings_dir.join(f), &[0.1_f32; 32])
                .unwrap();
        }

        // Session 1: trigger all 4 failure paths while fault hook is active.
        {
            let mut conn = Connection::open(&db_path).unwrap();
            let policy_gate = HistoryPolicyGate::new();

            // (1) In-flight Never switch abort
            let snap1 = policy_gate.snapshot();
            policy_gate
                .run_policy_transition(true, false, |_| Ok(()))
                .unwrap();
            let out1 = HistoryManager::save_entry_under_policy_gate_with_hook_in(
                &mut conn,
                &recordings_dir,
                Some(&canon_root),
                &policy_gate,
                snap1,
                || (true, false),
                None,
                make_hook(Arc::clone(&fail_remove)),
                f_inflight_never.clone(),
                "inflight never".into(),
                false,
                None,
                None,
                None,
                None,
            )
            .unwrap();
            assert!(matches!(
                out1,
                SaveOutcome::AbortedByPolicy {
                    wav_cleanup_pending: true
                }
            ));

            // (2) In-flight Keep the audio OFF switch
            let snap2 = policy_gate.snapshot();
            policy_gate
                .run_policy_transition(false, true, |_| Ok(()))
                .unwrap();
            let out2 = HistoryManager::save_entry_under_policy_gate_with_hook_in(
                &mut conn,
                &recordings_dir,
                Some(&canon_root),
                &policy_gate,
                snap2,
                || (false, false),
                None,
                make_hook(Arc::clone(&fail_remove)),
                f_audio_off.clone(),
                "inflight audio off".into(),
                false,
                None,
                None,
                None,
                None,
            )
            .unwrap();
            assert!(matches!(out2, SaveOutcome::Saved(ref e) if e.file_name.is_empty()));

            // (3) Never purge on existing unstarred row + (4) Time retention cleanup row
            conn.execute(
                "INSERT INTO transcription_history (id, file_name, timestamp, saved, title, transcription_text, post_process_requested)
                 VALUES (10, ?1, 1000, 0, 'Purge', 'purge', 0),
                        (11, ?2, 500, 0, 'Old', 'old', 0)",
                params![&f_never_purge, &f_time_cleanup],
            )
            .unwrap();
            let purge_err = HistoryManager::delete_entries_and_files_with_hook_in(
                &mut conn,
                &recordings_dir,
                Some(&canon_root),
                &[(10, f_never_purge.clone()), (11, f_time_cleanup.clone())],
                make_hook(Arc::clone(&fail_remove)),
            );
            assert!(purge_err.is_err());

            assert_eq!(
                HistoryManager::pending_audio_removal_count_in(&conn).unwrap(),
                4
            );
        }

        // Session 2 (simulated HistoryManager restart by reopening DB from disk):
        // all 4 entries are STILL present in `pending_audio_removal` and files still exist.
        {
            let conn = Connection::open(&db_path).unwrap();
            assert_eq!(
                HistoryManager::pending_audio_removal_count_in(&conn).unwrap(),
                4,
                "pending_audio_removal entries must survive restart"
            );
            for f in [
                &f_inflight_never,
                &f_audio_off,
                &f_never_purge,
                &f_time_cleanup,
            ] {
                assert!(recordings_dir.join(f).exists());
            }

            // Clear the fault and run the sweep -> all 4 files deleted & pending table cleared.
            fail_remove.store(false, Ordering::SeqCst);
            let drained = HistoryManager::drain_pending_audio_removal_in(
                &conn,
                &recordings_dir,
                Some(&canon_root),
            )
            .unwrap();
            assert_eq!(drained, 4);
            assert_eq!(
                HistoryManager::pending_audio_removal_count_in(&conn).unwrap(),
                0
            );
            for f in [
                &f_inflight_never,
                &f_audio_off,
                &f_never_purge,
                &f_time_cleanup,
            ] {
                assert!(!recordings_dir.join(f).exists());
            }
        }
        println!("AC_C15_PROOF: all_4_paths_recorded=true survived_restart=true drained_after_fault_clear=4");
    }

    #[test]
    fn ac_c16_exclusive_per_take_wav_same_second_and_bounded_root_substitution_refusal() {
        let dir = tempfile::tempdir().expect("tempdir");
        let recordings_dir = dir.path().join("recordings");
        let outside_dir = dir.path().join("outside");
        fs::create_dir_all(&recordings_dir).unwrap();
        fs::create_dir_all(&outside_dir).unwrap();
        let pinned_root = fs::canonicalize(&recordings_dir).unwrap();

        // 1) Two takes finishing in the same second (same base stem) get two distinct WAV names
        //    via `create_new(true)` + suffix retry, and both files remain intact without overwrite.
        let same_second_stem = "coco-voice-1760000000";
        let f1 = HistoryManager::create_unique_wav_exclusive_in(
            &recordings_dir,
            same_second_stem,
            &[0.1_f32; 64],
        )
        .unwrap();
        let f2 = HistoryManager::create_unique_wav_exclusive_in(
            &recordings_dir,
            same_second_stem,
            &[0.2_f32; 64],
        )
        .unwrap();
        assert_ne!(f1, f2, "same-second takes must get distinct WAV names");
        assert!(recordings_dir.join(&f1).exists());
        assert!(recordings_dir.join(&f2).exists());
        assert!(
            crate::audio_toolkit::save_wav_file_exclusive(
                &recordings_dir.join(&f1),
                &[0.3_f32; 64]
            )
            .is_err(),
            "create_new(true) must never overwrite an existing take WAV"
        );

        let outside_file = outside_dir.join("victim.wav");
        fs::write(&outside_file, b"DO-NOT-TOUCH").unwrap();

        // 2) Delete refuses `../` traversal name.
        assert!(HistoryManager::unlink_recording_strict_bounded(
            &recordings_dir,
            Some(&pinned_root),
            "../outside/victim.wav"
        )
        .is_err());

        #[cfg(unix)]
        {
            // 3) Delete refuses symlink inside recordings folder (target untouched).
            std::os::unix::fs::symlink(&outside_file, recordings_dir.join("evil-link.wav"))
                .unwrap();
            assert!(HistoryManager::unlink_recording_strict_bounded(
                &recordings_dir,
                Some(&pinned_root),
                "evil-link.wav"
            )
            .is_err());
            assert_eq!(fs::read(&outside_file).unwrap(), b"DO-NOT-TOUCH");

            // 4) Delete refuses when recordings folder is replaced by a symlink to another folder after start.
            let backup_dir = dir.path().join("recordings-moved");
            fs::rename(&recordings_dir, &backup_dir).unwrap();
            std::os::unix::fs::symlink(&outside_dir, &recordings_dir).unwrap();
            assert!(
                HistoryManager::unlink_recording_strict_bounded(
                    &recordings_dir,
                    Some(&pinned_root),
                    "victim.wav"
                )
                .is_err(),
                "bounded delete must refuse recordings_dir replaced with symlink after start"
            );
            assert_eq!(fs::read(&outside_file).unwrap(), b"DO-NOT-TOUCH");
        }
        println!("AC_C16_PROOF: same_second_distinct=true create_new_no_overwrite=true bounded_root_refusals=3");
    }

    #[test]
    fn ac_c17_crash_orphan_drains_on_restart_and_db_insert_failure_writes_zero_wav() {
        let dir = tempfile::tempdir().expect("tempdir");
        let recordings_dir = dir.path().join("recordings");
        fs::create_dir_all(&recordings_dir).unwrap();
        let canon_root = fs::canonicalize(&recordings_dir).unwrap();
        let db_path = dir.path().join("history.db");
        setup_file_db(&db_path);

        let samples = vec![0.15_f32; 64];

        // 1) Simulated crash: take records its name and creates its WAV inside policy_gate,
        //    then the connection/manager is dropped before any history row is written.
        let staged_name = {
            let conn = Connection::open(&db_path).unwrap();
            let policy_gate = HistoryPolicyGate::new();
            let snapshot = policy_gate.snapshot();
            let staged = HistoryManager::stage_take_wav_under_policy_gate_in(
                &conn,
                &recordings_dir,
                Some(&canon_root),
                &policy_gate,
                snapshot,
                || (false, true),
                "ac17-crash-take",
                &samples,
            )
            .unwrap()
            .expect("staged WAV name");
            assert!(recordings_dir.join(&staged).exists());
            assert_eq!(
                HistoryManager::pending_audio_removal_names_in(&conn).unwrap(),
                vec![staged.clone()]
            );
            staged
        };

        // On next startup sweep (reopened DB), the unowned crash orphan is deleted and table is empty.
        {
            let conn = Connection::open(&db_path).unwrap();
            let drained = HistoryManager::drain_pending_audio_removal_in(
                &conn,
                &recordings_dir,
                Some(&canon_root),
            )
            .unwrap();
            assert_eq!(drained, 1);
            assert!(!recordings_dir.join(&staged_name).exists());
            assert_eq!(
                HistoryManager::pending_audio_removal_count_in(&conn).unwrap(),
                0
            );
        }

        // 2) Forcing the pending_audio_removal insert to fail: ZERO WAV is created on disk,
        //    and saving the take (30 days, audio on) writes a text-only row with file_name == "".
        {
            let mut conn = Connection::open(&db_path).unwrap();
            let policy_gate = HistoryPolicyGate::new();
            let snapshot = policy_gate.snapshot();
            let staged = HistoryManager::stage_take_wav_under_policy_gate_with_hooks_in(
                &conn,
                &recordings_dir,
                Some(&canon_root),
                &policy_gate,
                snapshot,
                || (false, true),
                "ac17-insert-fail",
                &samples,
                |_conn, fname| Err(anyhow!("injected pending insert failure for '{fname}'")),
                |p, s| crate::audio_toolkit::save_wav_file_exclusive(p, s),
                |p, n| crate::audio_toolkit::verify_wav_file(p, n),
                HistoryManager::unlink_recording_strict_bounded,
            )
            .unwrap();
            assert!(staged.is_none(), "DB insert failure must write NO WAV");
            assert_eq!(fs::read_dir(&recordings_dir).unwrap().count(), 0);

            let saved = HistoryManager::save_entry_under_policy_gate_in(
                &mut conn,
                &recordings_dir,
                &policy_gate,
                snapshot,
                || (false, true),
                None,
                staged.unwrap_or_default(),
                "take continues without audio".into(),
                false,
                None,
                None,
                None,
                None,
            )
            .unwrap();
            match saved {
                SaveOutcome::Saved(entry) => {
                    assert!(
                        entry.file_name.is_empty(),
                        "row must be saved with empty file_name when WAV staging failed"
                    );
                }
                other => panic!("expected Saved text-only row, got {other:?}"),
            }
            assert_eq!(fs::read_dir(&recordings_dir).unwrap().count(), 0);
        }
        println!(
            "AC_C17_PROOF: crash_orphan_drained=1 db_insert_fail_wavs=0 text_only_row_saved=true"
        );
    }

    #[test]
    fn ac_c18_foreign_file_collision_untouched_and_table_holds_only_owned_names() {
        let dir = tempfile::tempdir().expect("tempdir");
        let recordings_dir = dir.path().join("recordings");
        fs::create_dir_all(&recordings_dir).unwrap();
        let canon_root = fs::canonicalize(&recordings_dir).unwrap();
        let db_path = dir.path().join("history.db");
        setup_file_db(&db_path);

        let foreign_name = "ac18-collide.wav";
        let foreign_path = recordings_dir.join(foreign_name);
        let sentinel = b"FOREIGN_UNOWNED_FILE_SENTINEL";
        fs::write(&foreign_path, sentinel).unwrap();

        let conn = Connection::open(&db_path).unwrap();
        let policy_gate = HistoryPolicyGate::new();
        let snapshot = policy_gate.snapshot();

        let owned_name = HistoryManager::stage_take_wav_under_policy_gate_in(
            &conn,
            &recordings_dir,
            Some(&canon_root),
            &policy_gate,
            snapshot,
            || (false, true),
            "ac18-collide",
            &[0.2_f32; 64],
        )
        .unwrap()
        .expect("take must allocate next available WAV name");

        assert_ne!(owned_name, foreign_name);
        assert!(
            foreign_path.exists(),
            "foreign file must never be unlinked on AlreadyExists"
        );
        assert_eq!(
            fs::read(&foreign_path).unwrap(),
            sentinel,
            "foreign file must remain byte-for-byte untouched"
        );
        assert!(recordings_dir.join(&owned_name).exists());
        assert_eq!(
            HistoryManager::pending_audio_removal_names_in(&conn).unwrap(),
            vec![owned_name.clone()],
            "pending_audio_removal must hold ONLY names this take owns"
        );
        println!(
            "AC_C18_PROOF: foreign_untouched=true owned_name={owned_name} table_only_owned=true"
        );
    }

    #[test]
    fn ac_c19_purge_deletes_row_before_unlink_and_retry_protects_referenced_name() {
        let dir = tempfile::tempdir().expect("tempdir");
        let recordings_dir = dir.path().join("recordings");
        fs::create_dir_all(&recordings_dir).unwrap();
        let canon_root = fs::canonicalize(&recordings_dir).unwrap();
        let db_path = dir.path().join("history.db");
        setup_file_db(&db_path);

        let wav_name = "ac19-purge-fail.wav";
        let wav_path = recordings_dir.join(wav_name);
        crate::audio_toolkit::save_wav_file_exclusive(&wav_path, &[0.25_f32; 48]).unwrap();

        let mut conn = Connection::open(&db_path).unwrap();
        conn.execute(
            "INSERT INTO transcription_history (id, file_name, timestamp, saved, title, transcription_text, post_process_requested)
             VALUES (77, ?1, 1000, 0, 'T', 'purged row', 0)",
            params![wav_name],
        )
        .unwrap();

        // 1) Purge with unlink forced to fail: ONE commit deletes row AND records name in pending table.
        let purge_err = HistoryManager::delete_entries_and_files_with_hook_in(
            &mut conn,
            &recordings_dir,
            Some(&canon_root),
            &[(77, wav_name.to_string())],
            |_dir, _expected, fname| Err(anyhow!("injected unlink failure for '{fname}'")),
        );
        assert!(purge_err.is_err());

        // Row is already gone, pending table holds wav_name, and starring id 77 updates 0 rows (fails).
        let row_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM transcription_history WHERE id = 77",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(row_count, 0, "row must be deleted before unlink");
        assert_eq!(
            HistoryManager::pending_audio_removal_names_in(&conn).unwrap(),
            vec![wav_name.to_string()]
        );
        let star_updated = conn
            .execute(
                "UPDATE transcription_history SET saved = 1 WHERE id = 77",
                [],
            )
            .unwrap();
        assert_eq!(
            star_updated, 0,
            "starring purged id must fail because row is already gone"
        );
        assert!(wav_path.exists());

        // 2) Insert a row referencing that same file_name directly -> retry keeps the file and drops entry.
        conn.execute(
            "INSERT INTO transcription_history (id, file_name, timestamp, saved, title, transcription_text, post_process_requested)
             VALUES (88, ?1, 1001, 1, 'Referenced', 'referenced take', 0)",
            params![wav_name],
        )
        .unwrap();

        let drained = HistoryManager::drain_pending_audio_removal_in(
            &conn,
            &recordings_dir,
            Some(&canon_root),
        )
        .unwrap();
        assert_eq!(drained, 0, "referenced WAV must NOT be unlinked by retry");
        assert!(
            wav_path.exists(),
            "referenced WAV must stay intact on disk after retry"
        );
        assert_eq!(
            HistoryManager::pending_audio_removal_count_in(&conn).unwrap(),
            0,
            "pending entry for referenced WAV must be dropped"
        );
        println!("AC_C19_PROOF: row_deleted_before_unlink=true star_after_purge_rows=0 referenced_wav_kept=true");
    }

    #[test]
    fn ac_c20_inflight_wav_deleted_before_audio_off_or_never_returns_50_runs_each() {
        use std::sync::atomic::AtomicBool;
        use std::sync::Barrier;

        let samples: Vec<f32> = vec![0.1_f32; 64];

        // 1) Audio OFF (50 runs): take stages WAV inside gate, then holds. Thread B turns audio OFF
        //    and drains unowned pending WAVs before returning. At that moment 0 in-flight WAVs exist
        //    (while pre-existing saved row's WAV stays per I4). Released, take saves with file_name == "".
        for round in 0..50_usize {
            let dir = tempfile::tempdir().expect("tempdir");
            let recordings_dir = dir.path().join("recordings");
            fs::create_dir_all(&recordings_dir).unwrap();
            let canon_root = fs::canonicalize(&recordings_dir).unwrap();
            let db_path = dir.path().join("history.db");
            setup_file_db(&db_path);

            // Seed one already-committed row with WAV (must survive audio-off per Rule I4).
            let committed_wav = "committed-row.wav";
            crate::audio_toolkit::save_wav_file_exclusive(
                &recordings_dir.join(committed_wav),
                &samples,
            )
            .unwrap();
            {
                let conn = Connection::open(&db_path).unwrap();
                conn.execute(
                    "INSERT INTO transcription_history (file_name, timestamp, saved, title, transcription_text, post_process_requested)
                     VALUES (?1, 900, 0, 'Prior', 'prior take', 0)",
                    params![committed_wav],
                )
                .unwrap();
            }

            let policy_gate = HistoryPolicyGate::new();
            let keep_audio = Arc::new(AtomicBool::new(true));
            let snapshot = policy_gate.snapshot();
            let staged_ready = Arc::new(Barrier::new(2));
            let release_save = Arc::new(Barrier::new(2));

            let gate_a = policy_gate.clone();
            let rec_a = recordings_dir.clone();
            let canon_a = canon_root.clone();
            let db_a = db_path.clone();
            let audio_a = Arc::clone(&keep_audio);
            let ready_a = Arc::clone(&staged_ready);
            let rel_a = Arc::clone(&release_save);
            let samples_a = samples.clone();
            let thread_a = std::thread::spawn(move || {
                let mut conn = Connection::open(&db_a).unwrap();
                let _ = conn.busy_timeout(Duration::from_secs(5));
                let staged = HistoryManager::stage_take_wav_under_policy_gate_in(
                    &conn,
                    &rec_a,
                    Some(&canon_a),
                    &gate_a,
                    snapshot,
                    || (false, audio_a.load(Ordering::SeqCst)),
                    &format!("ac20-audio-{round}"),
                    &samples_a,
                )
                .unwrap()
                .expect("in-flight take stages WAV");
                // Hold after WAV write while Thread B flips audio off and returns.
                ready_a.wait();
                rel_a.wait();
                let outcome = HistoryManager::save_entry_under_policy_gate_in(
                    &mut conn,
                    &rec_a,
                    &gate_a,
                    snapshot,
                    || (false, audio_a.load(Ordering::SeqCst)),
                    None,
                    staged.clone(),
                    format!("audio off round {round}"),
                    false,
                    None,
                    None,
                    None,
                    None,
                )
                .unwrap();
                (staged, outcome)
            });

            let gate_b = policy_gate.clone();
            let rec_b = recordings_dir.clone();
            let canon_b = canon_root.clone();
            let db_b = db_path.clone();
            let audio_b = Arc::clone(&keep_audio);
            let ready_b = Arc::clone(&staged_ready);
            let rel_b = Arc::clone(&release_save);
            let thread_b = std::thread::spawn(move || {
                ready_b.wait();
                gate_b
                    .run_policy_transition(false, true, |_| {
                        audio_b.store(false, Ordering::SeqCst);
                        let conn = Connection::open(&db_b)?;
                        let _ = conn.busy_timeout(Duration::from_secs(5));
                        HistoryManager::drain_pending_audio_removal_in(
                            &conn,
                            &rec_b,
                            Some(&canon_b),
                        )?;
                        Ok(())
                    })
                    .unwrap();
                // AT THE MOMENT audio-off returns (before releasing take):
                // 0 files from in-flight takes; only the prior committed row WAV remains (I4).
                let files: Vec<String> = fs::read_dir(&rec_b)
                    .unwrap()
                    .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
                    .collect();
                assert_eq!(
                    files,
                    vec![committed_wav.to_string()],
                    "audio-off round {round}: expected 0 in-flight WAVs when command returns"
                );
                rel_b.wait();
            });

            let (staged, outcome) = thread_a.join().unwrap();
            thread_b.join().unwrap();
            match outcome {
                SaveOutcome::Saved(entry) => {
                    assert!(
                        entry.file_name.is_empty(),
                        "audio-off round {round}: released take must save with empty file_name"
                    );
                }
                other => panic!("audio-off round {round}: unexpected {other:?}"),
            }
            assert!(!recordings_dir.join(&staged).exists());
            assert!(recordings_dir.join(committed_wav).exists());
        }

        // 2) Never ON (50 runs): take stages WAV inside gate, then holds. Thread B turns Never ON,
        //    drains pending and purges unstarred rows before returning -> 0 rows and 0 files at return,
        //    and still 0 rows and 0 files after take releases.
        for round in 0..50_usize {
            let dir = tempfile::tempdir().expect("tempdir");
            let recordings_dir = dir.path().join("recordings");
            fs::create_dir_all(&recordings_dir).unwrap();
            let canon_root = fs::canonicalize(&recordings_dir).unwrap();
            let db_path = dir.path().join("history.db");
            setup_file_db(&db_path);

            let policy_gate = HistoryPolicyGate::new();
            let never_store = Arc::new(AtomicBool::new(false));
            let snapshot = policy_gate.snapshot();
            let staged_ready = Arc::new(Barrier::new(2));
            let release_save = Arc::new(Barrier::new(2));

            let gate_a = policy_gate.clone();
            let rec_a = recordings_dir.clone();
            let canon_a = canon_root.clone();
            let db_a = db_path.clone();
            let never_a = Arc::clone(&never_store);
            let ready_a = Arc::clone(&staged_ready);
            let rel_a = Arc::clone(&release_save);
            let samples_a = samples.clone();
            let thread_a = std::thread::spawn(move || {
                let mut conn = Connection::open(&db_a).unwrap();
                let _ = conn.busy_timeout(Duration::from_secs(5));
                let staged = HistoryManager::stage_take_wav_under_policy_gate_in(
                    &conn,
                    &rec_a,
                    Some(&canon_a),
                    &gate_a,
                    snapshot,
                    || (never_a.load(Ordering::SeqCst), true),
                    &format!("ac20-never-{round}"),
                    &samples_a,
                )
                .unwrap()
                .expect("in-flight take stages WAV");
                ready_a.wait();
                rel_a.wait();
                HistoryManager::save_entry_under_policy_gate_in(
                    &mut conn,
                    &rec_a,
                    &gate_a,
                    snapshot,
                    || (never_a.load(Ordering::SeqCst), true),
                    None,
                    staged,
                    format!("never round {round}"),
                    false,
                    None,
                    None,
                    None,
                    None,
                )
                .unwrap()
            });

            let gate_b = policy_gate.clone();
            let rec_b = recordings_dir.clone();
            let canon_b = canon_root.clone();
            let db_b = db_path.clone();
            let never_b = Arc::clone(&never_store);
            let ready_b = Arc::clone(&staged_ready);
            let rel_b = Arc::clone(&release_save);
            let thread_b = std::thread::spawn(move || {
                ready_b.wait();
                gate_b
                    .run_policy_transition(true, false, |_| {
                        never_b.store(true, Ordering::SeqCst);
                        let mut conn = Connection::open(&db_b)?;
                        let _ = conn.busy_timeout(Duration::from_secs(5));
                        HistoryManager::drain_pending_audio_removal_in(
                            &conn,
                            &rec_b,
                            Some(&canon_b),
                        )?;
                        HistoryManager::cleanup_all_unsaved_bounded_in(
                            &mut conn,
                            &rec_b,
                            Some(&canon_b),
                        )?;
                        Ok(())
                    })
                    .unwrap();
                // AT THE MOMENT Never returns (before releasing take): 0 rows and 0 files!
                assert_eq!(
                    fs::read_dir(&rec_b).unwrap().count(),
                    0,
                    "never round {round}: expected 0 files when Never returns"
                );
                let conn = Connection::open(&db_b).unwrap();
                let rows: i64 = conn
                    .query_row("SELECT COUNT(*) FROM transcription_history", [], |r| {
                        r.get(0)
                    })
                    .unwrap();
                assert_eq!(rows, 0);
                rel_b.wait();
            });

            let outcome = thread_a.join().unwrap();
            thread_b.join().unwrap();
            assert!(matches!(
                outcome,
                SaveOutcome::AbortedByPolicy {
                    wav_cleanup_pending: false
                }
            ));
            let conn = Connection::open(&db_path).unwrap();
            let rows: i64 = conn
                .query_row("SELECT COUNT(*) FROM transcription_history", [], |r| {
                    r.get(0)
                })
                .unwrap();
            assert_eq!(rows, 0);
            assert_eq!(fs::read_dir(&recordings_dir).unwrap().count(), 0);
        }
        println!(
            "AC_C20_PROOF: audio_off_runs=50 never_runs=50 inflight_deleted_before_return=true"
        );
    }

    #[test]
    fn ac_c21_symlinked_recordings_root_refused_everywhere_and_zero_fs_remove_file_in_source() {
        use crate::settings::RecordingRetentionPeriod;

        #[cfg(unix)]
        {
            let dir = tempfile::tempdir().expect("tempdir");
            let recordings_dir = dir.path().join("recordings");
            let outside_dir = dir.path().join("outside");
            fs::create_dir_all(&recordings_dir).unwrap();
            fs::create_dir_all(&outside_dir).unwrap();
            let pinned_root = fs::canonicalize(&recordings_dir).unwrap();

            let victim_never = outside_dir.join("victim-never.wav");
            let victim_time = outside_dir.join("victim-time.wav");
            let victim_retry = outside_dir.join("victim-retry.wav");
            fs::write(&victim_never, b"OUTSIDE_NEVER").unwrap();
            fs::write(&victim_time, b"OUTSIDE_TIME").unwrap();
            fs::write(&victim_retry, b"OUTSIDE_RETRY").unwrap();

            let mut conn = setup_conn();
            let now: i64 = 1_760_000_000;
            // Seed 1 row for time purge (48h old), 1 row for Never purge (1h old), 1 retry name.
            conn.execute(
                "INSERT INTO transcription_history (id, file_name, timestamp, saved, title, transcription_text, post_process_requested)
                 VALUES (1, 'victim-time.wav', ?1, 0, 'Old', 'old', 0),
                        (2, 'victim-never.wav', ?2, 0, 'Recent', 'recent', 0)",
                params![now - 48 * 3_600, now - 3_600],
            )
            .unwrap();
            HistoryManager::record_pending_audio_removal_in(&conn, "victim-retry.wav").unwrap();

            // Replace recordings_dir with a symlink to outside_dir after start.
            let moved_dir = dir.path().join("recordings-moved");
            fs::rename(&recordings_dir, &moved_dir).unwrap();
            std::os::unix::fs::symlink(&outside_dir, &recordings_dir).unwrap();

            // 1) Time purge refuses symlinked root; outside file untouched; name stays in pending table.
            let time_res = HistoryManager::cleanup_by_time_bounded_in(
                &mut conn,
                &recordings_dir,
                Some(&pinned_root),
                RecordingRetentionPeriod::Hours24,
                now,
            );
            assert!(time_res.is_err());
            assert_eq!(fs::read(&victim_time).unwrap(), b"OUTSIDE_TIME");

            // 2) Bulk Never purge refuses symlinked root; outside file untouched; name stays in pending table.
            let never_res = HistoryManager::cleanup_all_unsaved_bounded_in(
                &mut conn,
                &recordings_dir,
                Some(&pinned_root),
            );
            assert!(never_res.is_err());
            assert_eq!(fs::read(&victim_never).unwrap(), b"OUTSIDE_NEVER");

            // 3) Retry sweep refuses symlinked root; outside file untouched; all 3 failed names stay in table.
            let retry_res = HistoryManager::drain_pending_audio_removal_in(
                &conn,
                &recordings_dir,
                Some(&pinned_root),
            );
            assert!(retry_res.is_err());
            assert_eq!(fs::read(&victim_retry).unwrap(), b"OUTSIDE_RETRY");

            let pending = HistoryManager::pending_audio_removal_names_in(&conn).unwrap();
            assert!(pending.contains(&"victim-time.wav".to_string()));
            assert!(pending.contains(&"victim-never.wav".to_string()));
            assert!(pending.contains(&"victim-retry.wav".to_string()));
        }

        // Source test: assert direct stdlib file deletion appears on NO recordings path
        // in actions.rs or managers/history.rs.
        let forbidden = concat!("fs::", "remove_file");
        let actions_src = include_str!("../actions.rs");
        let history_src = include_str!("history.rs");
        assert!(
            !actions_src.contains(forbidden),
            "actions.rs must not call {forbidden} on any recordings path"
        );
        assert!(
            !history_src.contains(forbidden),
            "managers/history.rs must not call {forbidden} on any recordings path"
        );
        println!("AC_C21_PROOF: symlinked_root_refused_never_time_retry=true zero_fs_remove_file_in_source=true");
    }

    #[test]
    fn ac_c22_active_staged_preserved_by_sweeps_drained_on_crash_and_never_missing_file_saves_empty(
    ) {
        use crate::settings::RecordingRetentionPeriod;

        let samples = vec![0.15_f32; 64];

        // 1) Isolated temp dir: hold take after staging -> run hourly retry AND 24-hour retention
        //    cleanup with no policy change -> staged file & pending entry untouched -> release take
        //    -> row saves with file_name and file exists on disk.
        {
            let dir = tempfile::tempdir().expect("tempdir");
            let recordings_dir = dir.path().join("recordings");
            fs::create_dir_all(&recordings_dir).unwrap();
            let canon_root = fs::canonicalize(&recordings_dir).unwrap();
            let db_path = dir.path().join("history.db");
            setup_file_db(&db_path);

            let policy_gate = HistoryPolicyGate::new();
            let snapshot = policy_gate.snapshot();
            let mut conn = Connection::open(&db_path).unwrap();

            // Seed an expired 48h-old row so 24-hour retention cleanup actually runs real work.
            let now: i64 = 1_760_000_000;
            let old_wav = "expired-48h.wav";
            crate::audio_toolkit::save_wav_file_exclusive(&recordings_dir.join(old_wav), &samples)
                .unwrap();
            conn.execute(
                "INSERT INTO transcription_history (file_name, timestamp, saved, title, transcription_text, post_process_requested)
                 VALUES (?1, ?2, 0, 'Old', 'expired row', 0)",
                params![old_wav, now - 48 * 3_600],
            )
            .unwrap();

            let live_wav = HistoryManager::stage_take_wav_under_policy_gate_in(
                &conn,
                &recordings_dir,
                Some(&canon_root),
                &policy_gate,
                snapshot,
                || (false, true),
                "ac22-live-take",
                &samples,
            )
            .unwrap()
            .expect("live take must stage WAV");
            assert!(recordings_dir.join(&live_wav).exists());
            assert!(policy_gate.is_active_staged(&live_wav));
            assert_eq!(
                HistoryManager::pending_audio_removal_names_in(&conn).unwrap(),
                vec![live_wav.clone()]
            );

            // Hourly retry (include_active_staged = false) skips active staged file.
            let hourly_drained = {
                let guard = policy_gate.lock();
                HistoryManager::drain_pending_audio_removal_with_hook_and_active_in(
                    &conn,
                    &recordings_dir,
                    Some(&canon_root),
                    &guard.active_staged_wavs,
                    false,
                    HistoryManager::unlink_recording_strict_bounded,
                )
                .unwrap()
            };
            assert_eq!(hourly_drained, 0);

            // 24-hour retention cleanup deletes expired row + its WAV, leaving active staged WAV untouched.
            let retained_deleted = HistoryManager::cleanup_by_time_bounded_in(
                &mut conn,
                &recordings_dir,
                Some(&canon_root),
                RecordingRetentionPeriod::Hours24,
                now,
            )
            .unwrap();
            assert_eq!(retained_deleted, 1);
            assert!(!recordings_dir.join(old_wav).exists());
            assert!(recordings_dir.join(&live_wav).exists());
            assert_eq!(
                HistoryManager::pending_audio_removal_names_in(&conn).unwrap(),
                vec![live_wav.clone()]
            );

            // Release take -> saves row with file_name, WAV exists, pending & active sets empty.
            let outcome = HistoryManager::save_entry_under_policy_gate_in(
                &mut conn,
                &recordings_dir,
                &policy_gate,
                snapshot,
                || (false, true),
                None,
                live_wav.clone(),
                "live take after sweeps".into(),
                false,
                None,
                None,
                None,
                None,
            )
            .unwrap();
            match outcome {
                SaveOutcome::Saved(entry) => assert_eq!(entry.file_name, live_wav),
                other => panic!("expected Saved with intact WAV, got {other:?}"),
            }
            assert!(recordings_dir.join(&live_wav).exists());
            assert_eq!(
                HistoryManager::pending_audio_removal_count_in(&conn).unwrap(),
                0
            );
            assert_eq!(policy_gate.active_staged_count(), 0);
        }

        // 2) Isolated temp dir: simulated crash -> fresh process (empty active set) drains abandoned WAV.
        {
            let dir = tempfile::tempdir().expect("tempdir");
            let recordings_dir = dir.path().join("recordings");
            fs::create_dir_all(&recordings_dir).unwrap();
            let canon_root = fs::canonicalize(&recordings_dir).unwrap();
            let db_path = dir.path().join("history.db");
            setup_file_db(&db_path);

            let crashed_wav = {
                let conn = Connection::open(&db_path).unwrap();
                let gate = HistoryPolicyGate::new();
                let snap = gate.snapshot();
                HistoryManager::stage_take_wav_under_policy_gate_in(
                    &conn,
                    &recordings_dir,
                    Some(&canon_root),
                    &gate,
                    snap,
                    || (false, true),
                    "ac22-crash-take",
                    &samples,
                )
                .unwrap()
                .expect("staged before crash")
            };
            assert!(recordings_dir.join(&crashed_wav).exists());

            let conn = Connection::open(&db_path).unwrap();
            let restarted_gate = HistoryPolicyGate::new();
            let drained = {
                let guard = restarted_gate.lock();
                HistoryManager::drain_pending_audio_removal_with_hook_and_active_in(
                    &conn,
                    &recordings_dir,
                    Some(&canon_root),
                    &guard.active_staged_wavs,
                    false,
                    HistoryManager::unlink_recording_strict_bounded,
                )
                .unwrap()
            };
            assert_eq!(drained, 1);
            assert!(!recordings_dir.join(&crashed_wav).exists());
            assert_eq!(
                HistoryManager::pending_audio_removal_count_in(&conn).unwrap(),
                0
            );
        }

        // 3) Isolated temp dir: Never switch during hold deletes staged file before command returns.
        {
            let dir = tempfile::tempdir().expect("tempdir");
            let recordings_dir = dir.path().join("recordings");
            fs::create_dir_all(&recordings_dir).unwrap();
            let canon_root = fs::canonicalize(&recordings_dir).unwrap();
            let db_path = dir.path().join("history.db");
            setup_file_db(&db_path);

            let conn = Connection::open(&db_path).unwrap();
            let gate = HistoryPolicyGate::new();
            let snap = gate.snapshot();
            let staged = HistoryManager::stage_take_wav_under_policy_gate_in(
                &conn,
                &recordings_dir,
                Some(&canon_root),
                &gate,
                snap,
                || (false, true),
                "ac22-never-take",
                &samples,
            )
            .unwrap()
            .expect("staged before Never");
            assert!(gate.is_active_staged(&staged));

            let drained = gate
                .run_policy_transition(true, false, |_| {
                    HistoryManager::drain_pending_audio_removal_with_hook_and_active_in(
                        &conn,
                        &recordings_dir,
                        Some(&canon_root),
                        &HashSet::new(),
                        true,
                        HistoryManager::unlink_recording_strict_bounded,
                    )
                })
                .unwrap();
            assert_eq!(drained, 1);
            assert_eq!(gate.active_staged_count(), 0);
            assert!(!recordings_dir.join(&staged).exists());
            assert_eq!(
                HistoryManager::pending_audio_removal_count_in(&conn).unwrap(),
                0
            );
        }

        // 4) Isolated temp dir: staged file removed by hand during hold -> final save stores
        //    row with `file_name == ""` (never stores a dangling reference to a missing file).
        {
            let dir = tempfile::tempdir().expect("tempdir");
            let recordings_dir = dir.path().join("recordings");
            fs::create_dir_all(&recordings_dir).unwrap();
            let canon_root = fs::canonicalize(&recordings_dir).unwrap();
            let db_path = dir.path().join("history.db");
            setup_file_db(&db_path);

            let mut conn = Connection::open(&db_path).unwrap();
            let gate = HistoryPolicyGate::new();
            let snap = gate.snapshot();
            let hand_removed = HistoryManager::stage_take_wav_under_policy_gate_in(
                &conn,
                &recordings_dir,
                Some(&canon_root),
                &gate,
                snap,
                || (false, true),
                "ac22-hand-removed",
                &samples,
            )
            .unwrap()
            .expect("staged before manual removal");

            // Simulate user/external removal of the staged file while take is held.
            HistoryManager::unlink_recording_strict_bounded(
                &recordings_dir,
                Some(&canon_root),
                &hand_removed,
            )
            .unwrap();
            assert!(!recordings_dir.join(&hand_removed).exists());

            let outcome = HistoryManager::save_entry_under_policy_gate_in(
                &mut conn,
                &recordings_dir,
                &gate,
                snap,
                || (false, true),
                None,
                hand_removed,
                "saved without missing wav".into(),
                false,
                None,
                None,
                None,
                None,
            )
            .unwrap();
            match outcome {
                SaveOutcome::Saved(entry) => {
                    assert!(
                        entry.file_name.is_empty(),
                        "missing staged WAV must save with empty file_name"
                    );
                }
                other => panic!("expected Saved with empty file_name, got {other:?}"),
            }
            assert_eq!(
                HistoryManager::pending_audio_removal_count_in(&conn).unwrap(),
                0
            );
            assert_eq!(gate.active_staged_count(), 0);
        }
        println!("AC_C22_PROOF: sweeps_preserved=true crash_drained=1 never_drained=1 missing_file_saved_empty=true");
    }

    #[test]
    fn ac_c23_write_failure_after_create_and_open_failures() {
        let samples = vec![0.1_f32; 48];

        // 1) Write fails after `create_new`, and bounded delete is ALSO forced to fail:
        //    entry stays, name is out of active set, and caller sees the failure (`Err`).
        //    With the fault cleared, next sweep deletes the file and the entry.
        {
            let dir = tempfile::tempdir().expect("tempdir");
            let recordings_dir = dir.path().join("recordings");
            fs::create_dir_all(&recordings_dir).unwrap();
            let canon_root = fs::canonicalize(&recordings_dir).unwrap();
            let db_path = dir.path().join("history.db");
            setup_file_db(&db_path);

            let conn = Connection::open(&db_path).unwrap();
            let gate = HistoryPolicyGate::new();
            let snap = gate.snapshot();

            let res = HistoryManager::stage_take_wav_under_policy_gate_with_hooks_in(
                &conn,
                &recordings_dir,
                Some(&canon_root),
                &gate,
                snap,
                || (false, true),
                "ac23-write-and-del-fail",
                &samples,
                HistoryManager::insert_pending_audio_removal_fresh_in,
                |path, _samples| {
                    let mut f = std::fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(path)
                        .map_err(|e| {
                            crate::audio_toolkit::ExclusiveWavError::OpenFailed(e.into())
                        })?;
                    use std::io::Write;
                    let _ = f.write_all(b"RIFF-PARTIAL");
                    Err(
                        crate::audio_toolkit::ExclusiveWavError::WriteFailedAfterCreate(anyhow!(
                            "injected ENOSPC after create_new"
                        )),
                    )
                },
                |p, n| crate::audio_toolkit::verify_wav_file(p, n),
                |_dir, _expected, fname| Err(anyhow!("injected unlink EACCES for '{fname}'")),
            );
            assert!(
                res.is_err(),
                "caller MUST see the failure when bounded delete fails after write failure"
            );
            let partial_name = "ac23-write-and-del-fail.wav";
            let partial_path = recordings_dir.join(partial_name);
            assert!(partial_path.exists());
            assert!(!gate.is_active_staged(partial_name));
            assert_eq!(
                HistoryManager::pending_audio_removal_names_in(&conn).unwrap(),
                vec![partial_name.to_string()]
            );

            // Clear fault -> next routine sweep deletes file and clears entry.
            let drained = {
                let guard = gate.lock();
                HistoryManager::drain_pending_audio_removal_with_hook_and_active_in(
                    &conn,
                    &recordings_dir,
                    Some(&canon_root),
                    &guard.active_staged_wavs,
                    false,
                    HistoryManager::unlink_recording_strict_bounded,
                )
                .unwrap()
            };
            assert_eq!(drained, 1);
            assert!(!partial_path.exists());
            assert_eq!(
                HistoryManager::pending_audio_removal_count_in(&conn).unwrap(),
                0
            );
        }

        // 2) Write fails after `create_new`, bounded delete succeeds -> file AND entry both gone.
        {
            let dir = tempfile::tempdir().expect("tempdir");
            let recordings_dir = dir.path().join("recordings");
            fs::create_dir_all(&recordings_dir).unwrap();
            let canon_root = fs::canonicalize(&recordings_dir).unwrap();
            let db_path = dir.path().join("history.db");
            setup_file_db(&db_path);

            let conn = Connection::open(&db_path).unwrap();
            let gate = HistoryPolicyGate::new();
            let snap = gate.snapshot();

            let res = HistoryManager::stage_take_wav_under_policy_gate_with_hooks_in(
                &conn,
                &recordings_dir,
                Some(&canon_root),
                &gate,
                snap,
                || (false, true),
                "ac23-write-fail-del-ok",
                &samples,
                HistoryManager::insert_pending_audio_removal_fresh_in,
                |path, _samples| {
                    let mut f = std::fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(path)
                        .map_err(|e| {
                            crate::audio_toolkit::ExclusiveWavError::OpenFailed(e.into())
                        })?;
                    use std::io::Write;
                    let _ = f.write_all(b"RIFF-PARTIAL");
                    Err(
                        crate::audio_toolkit::ExclusiveWavError::WriteFailedAfterCreate(anyhow!(
                            "injected write failure after create_new"
                        )),
                    )
                },
                |p, n| crate::audio_toolkit::verify_wav_file(p, n),
                HistoryManager::unlink_recording_strict_bounded,
            )
            .unwrap();
            assert!(res.is_none());
            assert!(!recordings_dir.join("ac23-write-fail-del-ok.wav").exists());
            assert_eq!(
                HistoryManager::pending_audio_removal_count_in(&conn).unwrap(),
                0
            );
            assert_eq!(gate.active_staged_count(), 0);
        }

        // 3) `create_new` fails with `AlreadyExists` AND AGAIN with `PermissionDenied`:
        //    entry is dropped from `pending_audio_removal` and foreign file is byte-for-byte unchanged.
        {
            let dir = tempfile::tempdir().expect("tempdir");
            let recordings_dir = dir.path().join("recordings");
            fs::create_dir_all(&recordings_dir).unwrap();
            let canon_root = fs::canonicalize(&recordings_dir).unwrap();
            let db_path = dir.path().join("history.db");
            setup_file_db(&db_path);

            let conn = Connection::open(&db_path).unwrap();
            let gate = HistoryPolicyGate::new();
            let snap = gate.snapshot();

            // 3a) AlreadyExists on foreign file -> foreign file byte-for-byte unchanged, entry dropped.
            let foreign_name = "ac23-foreign.wav";
            let foreign_path = recordings_dir.join(foreign_name);
            let sentinel = b"FOREIGN_UNOWNED_BYTES";
            fs::write(&foreign_path, sentinel).unwrap();

            let owned = HistoryManager::stage_take_wav_under_policy_gate_in(
                &conn,
                &recordings_dir,
                Some(&canon_root),
                &gate,
                snap,
                || (false, true),
                "ac23-foreign",
                &samples,
            )
            .unwrap()
            .expect("next candidate name allocated on AlreadyExists");
            assert_ne!(owned, foreign_name);
            assert_eq!(fs::read(&foreign_path).unwrap(), sentinel);
            assert_eq!(
                HistoryManager::pending_audio_removal_names_in(&conn).unwrap(),
                vec![owned.clone()]
            );
            assert!(!gate.is_active_staged(foreign_name));

            // 3b) PermissionDenied on read-only temp dir -> entry dropped, foreign file unchanged.
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let ro_dir = tempfile::tempdir().expect("tempdir");
                let ro_recordings = ro_dir.path().join("recordings");
                fs::create_dir_all(&ro_recordings).unwrap();
                let ro_canon = fs::canonicalize(&ro_recordings).unwrap();
                let ro_db = ro_dir.path().join("history.db");
                setup_file_db(&ro_db);
                let ro_conn = Connection::open(&ro_db).unwrap();
                let ro_gate = HistoryPolicyGate::new();
                let ro_snap = ro_gate.snapshot();

                let ro_foreign = ro_recordings.join("ac23-ro-foreign.wav");
                fs::write(&ro_foreign, sentinel).unwrap();
                fs::set_permissions(&ro_recordings, fs::Permissions::from_mode(0o500)).unwrap();

                let ro_res = HistoryManager::stage_take_wav_under_policy_gate_in(
                    &ro_conn,
                    &ro_recordings,
                    Some(&ro_canon),
                    &ro_gate,
                    ro_snap,
                    || (false, true),
                    "ac23-ro-new",
                    &samples,
                )
                .unwrap();

                // Restore directory permissions before assertions so tempdir cleanup always succeeds.
                fs::set_permissions(&ro_recordings, fs::Permissions::from_mode(0o700)).unwrap();

                assert!(
                    ro_res.is_none(),
                    "PermissionDenied on create_new must return None"
                );
                assert_eq!(
                    HistoryManager::pending_audio_removal_count_in(&ro_conn).unwrap(),
                    0,
                    "PermissionDenied must drop candidate name from pending_audio_removal"
                );
                assert_eq!(ro_gate.active_staged_count(), 0);
                assert_eq!(
                    fs::read(&ro_foreign).unwrap(),
                    sentinel,
                    "foreign file in read-only dir must remain byte-for-byte unchanged"
                );
            }
        }
        println!("AC_C23_PROOF: write_fail_del_fail_err_and_kept=true write_fail_del_ok_cleared=true already_exists_and_eperm_dropped=true");
    }

    #[test]
    fn ac_c24_final_save_three_outcomes_on_both_save_paths() {
        let samples = vec![0.15_f32; 48];

        // Run all 3 outcomes on BOTH save paths:
        //   require_audio == false (normal dictation save)
        //   require_audio == true  (failed-transcription save)
        for require_audio in [false, true] {
            let text = if require_audio {
                String::new()
            } else {
                "normal dictation text".to_string()
            };

            // 1) PRESENT: verified root + regular file -> row has file_name, pending empty,
            //    name leaves active set, file exists, result Saved (`ok`).
            {
                let dir = tempfile::tempdir().expect("tempdir");
                let recordings_dir = dir.path().join("recordings");
                fs::create_dir_all(&recordings_dir).unwrap();
                let canon_root = fs::canonicalize(&recordings_dir).unwrap();
                let db_path = dir.path().join("history.db");
                setup_file_db(&db_path);
                let mut conn = Connection::open(&db_path).unwrap();
                let gate = HistoryPolicyGate::new();
                let snap = gate.snapshot();

                let staged = HistoryManager::stage_take_wav_under_policy_gate_in(
                    &conn,
                    &recordings_dir,
                    Some(&canon_root),
                    &gate,
                    snap,
                    || (false, true),
                    "ac24-present",
                    &samples,
                )
                .unwrap()
                .expect("staged present");
                assert_eq!(
                    HistoryManager::inspect_staged_wav_in_verified_root(
                        &recordings_dir,
                        Some(&canon_root),
                        &staged,
                    ),
                    StagedWavInspection::Present
                );

                let out = HistoryManager::save_entry_under_policy_gate_core_in(
                    &mut conn,
                    &recordings_dir,
                    Some(&canon_root),
                    &gate,
                    snap,
                    || (false, true),
                    None,
                    HistoryManager::unlink_recording_strict_bounded,
                    require_audio,
                    staged.clone(),
                    text.clone(),
                    false,
                    None,
                    None,
                    None,
                    None,
                )
                .unwrap();
                assert!(matches!(out, SaveOutcome::Saved(ref e) if e.file_name == staged));
                assert!(recordings_dir.join(&staged).exists());
                assert!(!gate.is_active_staged(&staged));
                assert_eq!(
                    HistoryManager::pending_audio_removal_count_in(&conn).unwrap(),
                    0
                );
            }

            // 2) ABSENT (file deleted by hand while held): verified root + ENOENT ->
            //    normal save stores row with file_name == ""; failed-transcription save stores 0 rows;
            //    both clear pending_audio_removal and active_staged_wavs.
            {
                let dir = tempfile::tempdir().expect("tempdir");
                let recordings_dir = dir.path().join("recordings");
                fs::create_dir_all(&recordings_dir).unwrap();
                let canon_root = fs::canonicalize(&recordings_dir).unwrap();
                let db_path = dir.path().join("history.db");
                setup_file_db(&db_path);
                let mut conn = Connection::open(&db_path).unwrap();
                let gate = HistoryPolicyGate::new();
                let snap = gate.snapshot();

                let staged = HistoryManager::stage_take_wav_under_policy_gate_in(
                    &conn,
                    &recordings_dir,
                    Some(&canon_root),
                    &gate,
                    snap,
                    || (false, true),
                    "ac24-absent",
                    &samples,
                )
                .unwrap()
                .expect("staged absent");
                HistoryManager::unlink_recording_strict_bounded(
                    &recordings_dir,
                    Some(&canon_root),
                    &staged,
                )
                .unwrap();
                assert_eq!(
                    HistoryManager::inspect_staged_wav_in_verified_root(
                        &recordings_dir,
                        Some(&canon_root),
                        &staged,
                    ),
                    StagedWavInspection::ConfirmedAbsent
                );

                let out = HistoryManager::save_entry_under_policy_gate_core_in(
                    &mut conn,
                    &recordings_dir,
                    Some(&canon_root),
                    &gate,
                    snap,
                    || (false, true),
                    None,
                    HistoryManager::unlink_recording_strict_bounded,
                    require_audio,
                    staged.clone(),
                    text.clone(),
                    false,
                    None,
                    None,
                    None,
                    None,
                )
                .unwrap();
                if require_audio {
                    assert!(matches!(
                        out,
                        SaveOutcome::AbortedByPolicy {
                            wav_cleanup_pending: false
                        }
                    ));
                    let rows: i64 = conn
                        .query_row("SELECT COUNT(*) FROM transcription_history", [], |r| {
                            r.get(0)
                        })
                        .unwrap();
                    assert_eq!(rows, 0);
                } else {
                    assert!(matches!(out, SaveOutcome::Saved(ref e) if e.file_name.is_empty()));
                }
                assert!(!gate.is_active_staged(&staged));
                assert_eq!(
                    HistoryManager::pending_audio_removal_count_in(&conn).unwrap(),
                    0
                );
            }

            // 3a) CAN'T TELL — Way 1: lookup hook returns PermissionDenied ->
            //     normal save has row with file_name == "" (in_history=true), failed save has 0 rows;
            //     both KEEP pending_audio_removal, leave active set, return Incomplete;
            //     with fault cleared, next sweep deletes the regular file and clears its entry.
            {
                let dir = tempfile::tempdir().expect("tempdir");
                let recordings_dir = dir.path().join("recordings");
                fs::create_dir_all(&recordings_dir).unwrap();
                let canon_root = fs::canonicalize(&recordings_dir).unwrap();
                let db_path = dir.path().join("history.db");
                setup_file_db(&db_path);
                let mut conn = Connection::open(&db_path).unwrap();
                let gate = HistoryPolicyGate::new();
                let snap = gate.snapshot();

                let staged = HistoryManager::stage_take_wav_under_policy_gate_in(
                    &conn,
                    &recordings_dir,
                    Some(&canon_root),
                    &gate,
                    snap,
                    || (false, true),
                    "ac24-hook-eperm",
                    &samples,
                )
                .unwrap()
                .expect("staged hook eperm");

                let out = HistoryManager::save_entry_under_policy_gate_with_inspect_hook_in(
                    &mut conn,
                    &recordings_dir,
                    Some(&canon_root),
                    &gate,
                    snap,
                    || (false, true),
                    None,
                    HistoryManager::unlink_recording_strict_bounded,
                    |_dir, _root, _fname| {
                        StagedWavInspection::Uncertain(StagedWavInspectionReason::PermissionDenied)
                    },
                    require_audio,
                    staged.clone(),
                    text.clone(),
                    false,
                    None,
                    None,
                    None,
                    None,
                )
                .unwrap();
                if require_audio {
                    assert!(matches!(
                        out,
                        SaveOutcome::IncompleteAudioInspection {
                            entry: None,
                            reason: StagedWavInspectionReason::PermissionDenied,
                            ..
                        }
                    ));
                    let rows: i64 = conn
                        .query_row("SELECT COUNT(*) FROM transcription_history", [], |r| {
                            r.get(0)
                        })
                        .unwrap();
                    assert_eq!(rows, 0);
                } else {
                    assert!(matches!(
                        out,
                        SaveOutcome::IncompleteAudioInspection {
                            entry: Some(ref e),
                            reason: StagedWavInspectionReason::PermissionDenied,
                            ..
                        } if e.file_name.is_empty()
                    ));
                    assert!(out.is_in_history());
                }
                assert!(!gate.is_active_staged(&staged));
                assert_eq!(
                    HistoryManager::pending_audio_removal_names_in(&conn).unwrap(),
                    vec![staged.clone()]
                );

                // Clear fault -> next routine sweep deletes file and clears pending entry.
                let drained = {
                    let guard = gate.lock();
                    HistoryManager::drain_pending_audio_removal_with_hook_and_active_in(
                        &conn,
                        &recordings_dir,
                        Some(&canon_root),
                        &guard.active_staged_wavs,
                        false,
                        HistoryManager::unlink_recording_strict_bounded,
                    )
                    .unwrap()
                };
                assert_eq!(drained, 1);
                assert!(!recordings_dir.join(&staged).exists());
                assert_eq!(
                    HistoryManager::pending_audio_removal_count_in(&conn).unwrap(),
                    0
                );
            }

            // 3b) CAN'T TELL — Way 2 (Unix): recordings root swapped for a symlink to another folder ->
            //     normal save has row with file_name == "", failed save has 0 rows;
            //     both KEEP pending_audio_removal, leave active set, return Incomplete;
            //     with real root restored, next sweep deletes the regular file and clears its entry.
            #[cfg(unix)]
            {
                let dir = tempfile::tempdir().expect("tempdir");
                let recordings_dir = dir.path().join("recordings");
                let outside_dir = dir.path().join("outside");
                fs::create_dir_all(&recordings_dir).unwrap();
                fs::create_dir_all(&outside_dir).unwrap();
                let pinned_root = fs::canonicalize(&recordings_dir).unwrap();
                let db_path = dir.path().join("history.db");
                setup_file_db(&db_path);
                let mut conn = Connection::open(&db_path).unwrap();
                let gate = HistoryPolicyGate::new();
                let snap = gate.snapshot();

                let staged = HistoryManager::stage_take_wav_under_policy_gate_in(
                    &conn,
                    &recordings_dir,
                    Some(&pinned_root),
                    &gate,
                    snap,
                    || (false, true),
                    "ac24-root-symlink",
                    &samples,
                )
                .unwrap()
                .expect("staged root symlink");

                let moved_dir = dir.path().join("recordings-real");
                fs::rename(&recordings_dir, &moved_dir).unwrap();
                std::os::unix::fs::symlink(&outside_dir, &recordings_dir).unwrap();

                let out = HistoryManager::save_entry_under_policy_gate_core_in(
                    &mut conn,
                    &recordings_dir,
                    Some(&pinned_root),
                    &gate,
                    snap,
                    || (false, true),
                    None,
                    HistoryManager::unlink_recording_strict_bounded,
                    require_audio,
                    staged.clone(),
                    text.clone(),
                    false,
                    None,
                    None,
                    None,
                    None,
                )
                .unwrap();
                if require_audio {
                    assert!(matches!(
                        out,
                        SaveOutcome::IncompleteAudioInspection {
                            entry: None,
                            reason: StagedWavInspectionReason::RootCheckFailed,
                            ..
                        }
                    ));
                    let rows: i64 = conn
                        .query_row("SELECT COUNT(*) FROM transcription_history", [], |r| {
                            r.get(0)
                        })
                        .unwrap();
                    assert_eq!(rows, 0);
                } else {
                    assert!(matches!(
                        out,
                        SaveOutcome::IncompleteAudioInspection {
                            entry: Some(ref e),
                            reason: StagedWavInspectionReason::RootCheckFailed,
                            ..
                        } if e.file_name.is_empty()
                    ));
                    assert!(out.is_in_history());
                }
                assert!(!gate.is_active_staged(&staged));
                assert_eq!(
                    HistoryManager::pending_audio_removal_names_in(&conn).unwrap(),
                    vec![staged.clone()]
                );

                // Restore real root via rename -> sweep deletes file and clears pending entry.
                fs::rename(&recordings_dir, dir.path().join("discarded-root-link")).unwrap();
                fs::rename(&moved_dir, &recordings_dir).unwrap();
                let drained = {
                    let guard = gate.lock();
                    HistoryManager::drain_pending_audio_removal_with_hook_and_active_in(
                        &conn,
                        &recordings_dir,
                        Some(&pinned_root),
                        &guard.active_staged_wavs,
                        false,
                        HistoryManager::unlink_recording_strict_bounded,
                    )
                    .unwrap()
                };
                assert_eq!(drained, 1);
                assert!(!recordings_dir.join(&staged).exists());
                assert_eq!(
                    HistoryManager::pending_audio_removal_count_in(&conn).unwrap(),
                    0
                );
            }

            // 3c) CAN'T TELL — Way 3: target replaced by a directory ->
            //     normal save has row with file_name == "", failed save has 0 rows;
            //     both KEEP pending_audio_removal, leave active set, return Incomplete;
            //     once regular file is restored (fault cleared), next sweep deletes it and clears entry.
            {
                let dir = tempfile::tempdir().expect("tempdir");
                let recordings_dir = dir.path().join("recordings");
                fs::create_dir_all(&recordings_dir).unwrap();
                let canon_root = fs::canonicalize(&recordings_dir).unwrap();
                let db_path = dir.path().join("history.db");
                setup_file_db(&db_path);
                let mut conn = Connection::open(&db_path).unwrap();
                let gate = HistoryPolicyGate::new();
                let snap = gate.snapshot();

                let staged = HistoryManager::stage_take_wav_under_policy_gate_in(
                    &conn,
                    &recordings_dir,
                    Some(&canon_root),
                    &gate,
                    snap,
                    || (false, true),
                    "ac24-dir-target",
                    &samples,
                )
                .unwrap()
                .expect("staged dir target");

                let saved_file_backup = dir.path().join("staged-backup.wav");
                fs::rename(recordings_dir.join(&staged), &saved_file_backup).unwrap();
                fs::create_dir(recordings_dir.join(&staged)).unwrap();

                assert_eq!(
                    HistoryManager::inspect_staged_wav_in_verified_root(
                        &recordings_dir,
                        Some(&canon_root),
                        &staged,
                    ),
                    StagedWavInspection::Uncertain(StagedWavInspectionReason::NotRegularFile)
                );

                let out = HistoryManager::save_entry_under_policy_gate_core_in(
                    &mut conn,
                    &recordings_dir,
                    Some(&canon_root),
                    &gate,
                    snap,
                    || (false, true),
                    None,
                    HistoryManager::unlink_recording_strict_bounded,
                    require_audio,
                    staged.clone(),
                    text.clone(),
                    false,
                    None,
                    None,
                    None,
                    None,
                )
                .unwrap();
                if require_audio {
                    assert!(matches!(
                        out,
                        SaveOutcome::IncompleteAudioInspection {
                            entry: None,
                            reason: StagedWavInspectionReason::NotRegularFile,
                            ..
                        }
                    ));
                    let rows: i64 = conn
                        .query_row("SELECT COUNT(*) FROM transcription_history", [], |r| {
                            r.get(0)
                        })
                        .unwrap();
                    assert_eq!(rows, 0);
                } else {
                    assert!(matches!(
                        out,
                        SaveOutcome::IncompleteAudioInspection {
                            entry: Some(ref e),
                            reason: StagedWavInspectionReason::NotRegularFile,
                            ..
                        } if e.file_name.is_empty()
                    ));
                    assert!(out.is_in_history());
                }
                assert!(!gate.is_active_staged(&staged));
                assert_eq!(
                    HistoryManager::pending_audio_removal_names_in(&conn).unwrap(),
                    vec![staged.clone()]
                );

                // Clear fault (remove directory, restore regular WAV) -> sweep deletes file and clears entry.
                fs::remove_dir(recordings_dir.join(&staged)).unwrap();
                fs::rename(&saved_file_backup, recordings_dir.join(&staged)).unwrap();
                let drained = {
                    let guard = gate.lock();
                    HistoryManager::drain_pending_audio_removal_with_hook_and_active_in(
                        &conn,
                        &recordings_dir,
                        Some(&canon_root),
                        &guard.active_staged_wavs,
                        false,
                        HistoryManager::unlink_recording_strict_bounded,
                    )
                    .unwrap()
                };
                assert_eq!(drained, 1);
                assert!(!recordings_dir.join(&staged).exists());
                assert_eq!(
                    HistoryManager::pending_audio_removal_count_in(&conn).unwrap(),
                    0
                );
            }

            // 3d) CAN'T TELL — Real OS EACCES (`chmod 0o000` on recordings_dir, restored to 0o700).
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;

                let dir = tempfile::tempdir().expect("tempdir");
                let recordings_dir = dir.path().join("recordings");
                fs::create_dir_all(&recordings_dir).unwrap();
                let pinned_root = fs::canonicalize(&recordings_dir).unwrap();
                let db_path = dir.path().join("history.db");
                setup_file_db(&db_path);
                let mut conn = Connection::open(&db_path).unwrap();
                let gate = HistoryPolicyGate::new();
                let snap = gate.snapshot();

                let staged = HistoryManager::stage_take_wav_under_policy_gate_in(
                    &conn,
                    &recordings_dir,
                    Some(&pinned_root),
                    &gate,
                    snap,
                    || (false, true),
                    "ac24-os-eperm",
                    &samples,
                )
                .unwrap()
                .expect("staged os eperm");

                fs::set_permissions(&recordings_dir, fs::Permissions::from_mode(0o000)).unwrap();
                let insp = HistoryManager::inspect_staged_wav_in_verified_root(
                    &recordings_dir,
                    Some(&pinned_root),
                    &staged,
                );
                let out = HistoryManager::save_entry_under_policy_gate_core_in(
                    &mut conn,
                    &recordings_dir,
                    Some(&pinned_root),
                    &gate,
                    snap,
                    || (false, true),
                    None,
                    HistoryManager::unlink_recording_strict_bounded,
                    require_audio,
                    staged.clone(),
                    text.clone(),
                    false,
                    None,
                    None,
                    None,
                    None,
                )
                .unwrap();
                fs::set_permissions(&recordings_dir, fs::Permissions::from_mode(0o700)).unwrap();

                assert_eq!(
                    insp,
                    StagedWavInspection::Uncertain(StagedWavInspectionReason::PermissionDenied)
                );
                if require_audio {
                    assert!(matches!(
                        out,
                        SaveOutcome::IncompleteAudioInspection {
                            entry: None,
                            reason: StagedWavInspectionReason::PermissionDenied,
                            ..
                        }
                    ));
                    let rows: i64 = conn
                        .query_row("SELECT COUNT(*) FROM transcription_history", [], |r| {
                            r.get(0)
                        })
                        .unwrap();
                    assert_eq!(rows, 0);
                } else {
                    assert!(matches!(
                        out,
                        SaveOutcome::IncompleteAudioInspection {
                            entry: Some(ref e),
                            reason: StagedWavInspectionReason::PermissionDenied,
                            ..
                        } if e.file_name.is_empty()
                    ));
                    assert!(out.is_in_history());
                }
                assert!(!gate.is_active_staged(&staged));
                assert_eq!(
                    HistoryManager::pending_audio_removal_names_in(&conn).unwrap(),
                    vec![staged.clone()]
                );

                let drained = {
                    let guard = gate.lock();
                    HistoryManager::drain_pending_audio_removal_with_hook_and_active_in(
                        &conn,
                        &recordings_dir,
                        Some(&pinned_root),
                        &guard.active_staged_wavs,
                        false,
                        HistoryManager::unlink_recording_strict_bounded,
                    )
                    .unwrap()
                };
                assert_eq!(drained, 1);
                assert!(!recordings_dir.join(&staged).exists());
                assert_eq!(
                    HistoryManager::pending_audio_removal_count_in(&conn).unwrap(),
                    0
                );
            }
        }
        println!("AC_C24_PROOF: both_save_paths=true present_ok=true absent_cleared_and_failed_0_rows=true cant_tell_hook_symlink_dir_eperm_kept_and_drained=true");
    }

    #[test]
    fn ac_c25_failed_save_no_dead_row_paste_notice_in_history_and_path_free_logs() {
        use crate::settings::{ClipboardHandling, RecordingRetentionPeriod};

        let samples = vec![0.15_f32; 48];

        let assert_path_free_message =
            |msg: &str, file_name: &str, reason: StagedWavInspectionReason, temp_root: &Path| {
                let temp_str = temp_root.to_string_lossy();
                assert!(
                    msg.contains(file_name),
                    "message '{msg}' must contain file_name '{file_name}'"
                );
                assert!(
                    msg.contains(reason.as_str()),
                    "message '{msg}' must contain fixed reason '{}'",
                    reason.as_str()
                );
                assert!(
                    !msg.contains(temp_str.as_ref()),
                    "message '{msg}' must NOT contain temp folder path '{temp_str}'"
                );
                assert!(
                    !msg.contains('/') && !msg.contains('\\'),
                    "message '{msg}' must NOT contain any folder path separators"
                );
                assert!(
                    !msg.contains("os error")
                        && !msg.contains("Permission denied")
                        && !msg.contains("No such file"),
                    "message '{msg}' must NOT contain raw OS error text"
                );
            };

        // 1) Failed-transcription save (`require_audio = true`), Absent -> 0 rows and entry gone.
        {
            let dir = tempfile::tempdir().expect("tempdir");
            let recordings_dir = dir.path().join("recordings");
            fs::create_dir_all(&recordings_dir).unwrap();
            let canon_root = fs::canonicalize(&recordings_dir).unwrap();
            let db_path = dir.path().join("history.db");
            setup_file_db(&db_path);
            let mut conn = Connection::open(&db_path).unwrap();
            let gate = HistoryPolicyGate::new();
            let snap = gate.snapshot();

            let staged = HistoryManager::stage_take_wav_under_policy_gate_in(
                &conn,
                &recordings_dir,
                Some(&canon_root),
                &gate,
                snap,
                || (false, true),
                "ac25-failed-absent",
                &samples,
            )
            .unwrap()
            .expect("staged");
            HistoryManager::unlink_recording_strict_bounded(
                &recordings_dir,
                Some(&canon_root),
                &staged,
            )
            .unwrap();

            let out = HistoryManager::save_entry_under_policy_gate_core_in(
                &mut conn,
                &recordings_dir,
                Some(&canon_root),
                &gate,
                snap,
                || (false, true),
                None,
                HistoryManager::unlink_recording_strict_bounded,
                true,
                staged.clone(),
                String::new(),
                false,
                None,
                None,
                None,
                None,
            )
            .unwrap();
            assert!(matches!(
                out,
                SaveOutcome::AbortedByPolicy {
                    wav_cleanup_pending: false
                }
            ));
            assert!(!out.is_in_history());
            let rows: i64 = conn
                .query_row("SELECT COUNT(*) FROM transcription_history", [], |r| {
                    r.get(0)
                })
                .unwrap();
            assert_eq!(rows, 0, "failed-transcription Absent must store 0 rows");
            assert_eq!(
                HistoryManager::pending_audio_removal_count_in(&conn).unwrap(),
                0
            );
            assert!(!gate.is_active_staged(&staged));

            let absent_log = HistoryManager::format_final_save_absent_log(&staged);
            assert!(absent_log.contains(&staged));
            assert!(!absent_log.contains(dir.path().to_string_lossy().as_ref()));
            assert!(!absent_log.contains('/'));
        }

        // 2) Failed-transcription save (`require_audio = true`), Can't tell under all 4 reason kinds
        //    (PermissionDenied hook, symlinked root, directory target, IoError hook):
        //    0 rows, entry kept, not active, incomplete returned with path-free message,
        //    and after fault cleared next sweep removes file and entry.
        for reason_kind in [
            StagedWavInspectionReason::PermissionDenied,
            StagedWavInspectionReason::RootCheckFailed,
            StagedWavInspectionReason::NotRegularFile,
            StagedWavInspectionReason::IoError,
        ] {
            let dir = tempfile::tempdir().expect("tempdir");
            let recordings_dir = dir.path().join("recordings");
            fs::create_dir_all(&recordings_dir).unwrap();
            let canon_root = fs::canonicalize(&recordings_dir).unwrap();
            let db_path = dir.path().join("history.db");
            setup_file_db(&db_path);
            let mut conn = Connection::open(&db_path).unwrap();
            let gate = HistoryPolicyGate::new();
            let snap = gate.snapshot();

            let stem = format!("ac25-failed-{}", reason_kind.as_str());
            let staged = HistoryManager::stage_take_wav_under_policy_gate_in(
                &conn,
                &recordings_dir,
                Some(&canon_root),
                &gate,
                snap,
                || (false, true),
                &stem,
                &samples,
            )
            .unwrap()
            .expect("staged");

            let out = match reason_kind {
                StagedWavInspectionReason::PermissionDenied
                | StagedWavInspectionReason::IoError => {
                    HistoryManager::save_entry_under_policy_gate_with_inspect_hook_in(
                        &mut conn,
                        &recordings_dir,
                        Some(&canon_root),
                        &gate,
                        snap,
                        || (false, true),
                        None,
                        HistoryManager::unlink_recording_strict_bounded,
                        |_d, _r, _f| StagedWavInspection::Uncertain(reason_kind),
                        true,
                        staged.clone(),
                        String::new(),
                        false,
                        None,
                        None,
                        None,
                        None,
                    )
                    .unwrap()
                }
                StagedWavInspectionReason::RootCheckFailed => {
                    #[cfg(unix)]
                    {
                        let outside_dir = dir.path().join("outside");
                        fs::create_dir_all(&outside_dir).unwrap();
                        let moved_dir = dir.path().join("recordings-real");
                        fs::rename(&recordings_dir, &moved_dir).unwrap();
                        std::os::unix::fs::symlink(&outside_dir, &recordings_dir).unwrap();
                        let res = HistoryManager::save_entry_under_policy_gate_core_in(
                            &mut conn,
                            &recordings_dir,
                            Some(&canon_root),
                            &gate,
                            snap,
                            || (false, true),
                            None,
                            HistoryManager::unlink_recording_strict_bounded,
                            true,
                            staged.clone(),
                            String::new(),
                            false,
                            None,
                            None,
                            None,
                            None,
                        )
                        .unwrap();
                        fs::rename(&recordings_dir, dir.path().join("discarded-link")).unwrap();
                        fs::rename(&moved_dir, &recordings_dir).unwrap();
                        res
                    }
                    #[cfg(not(unix))]
                    {
                        HistoryManager::save_entry_under_policy_gate_with_inspect_hook_in(
                            &mut conn,
                            &recordings_dir,
                            Some(&canon_root),
                            &gate,
                            snap,
                            || (false, true),
                            None,
                            HistoryManager::unlink_recording_strict_bounded,
                            |_d, _r, _f| StagedWavInspection::Uncertain(reason_kind),
                            true,
                            staged.clone(),
                            String::new(),
                            false,
                            None,
                            None,
                            None,
                            None,
                        )
                        .unwrap()
                    }
                }
                StagedWavInspectionReason::NotRegularFile => {
                    let backup = dir.path().join("backup.wav");
                    fs::rename(recordings_dir.join(&staged), &backup).unwrap();
                    fs::create_dir(recordings_dir.join(&staged)).unwrap();
                    let res = HistoryManager::save_entry_under_policy_gate_core_in(
                        &mut conn,
                        &recordings_dir,
                        Some(&canon_root),
                        &gate,
                        snap,
                        || (false, true),
                        None,
                        HistoryManager::unlink_recording_strict_bounded,
                        true,
                        staged.clone(),
                        String::new(),
                        false,
                        None,
                        None,
                        None,
                        None,
                    )
                    .unwrap();
                    fs::remove_dir(recordings_dir.join(&staged)).unwrap();
                    fs::rename(&backup, recordings_dir.join(&staged)).unwrap();
                    res
                }
            };

            assert!(matches!(
                out,
                SaveOutcome::IncompleteAudioInspection {
                    entry: None,
                    reason,
                    ..
                } if reason == reason_kind
            ));
            assert!(!out.is_in_history());
            let rows: i64 = conn
                .query_row("SELECT COUNT(*) FROM transcription_history", [], |r| {
                    r.get(0)
                })
                .unwrap();
            assert_eq!(rows, 0, "failed-transcription Can't tell must store 0 rows");
            assert!(!gate.is_active_staged(&staged));
            assert_eq!(
                HistoryManager::pending_audio_removal_names_in(&conn).unwrap(),
                vec![staged.clone()]
            );

            // Verify both formatted log line AND returned error are path-free + exact reason kind.
            let log_line =
                HistoryManager::format_final_save_incomplete_message(&staged, reason_kind);
            let err_msg = out
                .incomplete_error_message()
                .expect("incomplete outcome must produce error message");
            assert_path_free_message(&log_line, &staged, reason_kind, dir.path());
            assert_path_free_message(&err_msg, &staged, reason_kind, dir.path());

            // With fault cleared, next sweep deletes the regular file and clears pending entry.
            let drained = {
                let guard = gate.lock();
                HistoryManager::drain_pending_audio_removal_with_hook_and_active_in(
                    &conn,
                    &recordings_dir,
                    Some(&canon_root),
                    &guard.active_staged_wavs,
                    false,
                    HistoryManager::unlink_recording_strict_bounded,
                )
                .unwrap()
            };
            assert_eq!(drained, 1);
            assert!(!recordings_dir.join(&staged).exists());
            assert_eq!(
                HistoryManager::pending_audio_removal_count_in(&conn).unwrap(),
                0
            );
        }

        // 3) Normal save (`require_audio = false`), Can't tell -> row saved with file_name == "",
        //    counts as IN HISTORY for the paste notice, pending KEPT, and log/error are path-free.
        {
            let dir = tempfile::tempdir().expect("tempdir");
            let recordings_dir = dir.path().join("recordings");
            fs::create_dir_all(&recordings_dir).unwrap();
            let canon_root = fs::canonicalize(&recordings_dir).unwrap();
            let db_path = dir.path().join("history.db");
            setup_file_db(&db_path);
            let mut conn = Connection::open(&db_path).unwrap();
            let gate = HistoryPolicyGate::new();
            let snap = gate.snapshot();

            let staged = HistoryManager::stage_take_wav_under_policy_gate_in(
                &conn,
                &recordings_dir,
                Some(&canon_root),
                &gate,
                snap,
                || (false, true),
                "ac25-normal-cant-tell",
                &samples,
            )
            .unwrap()
            .expect("staged");

            let out = HistoryManager::save_entry_under_policy_gate_with_inspect_hook_in(
                &mut conn,
                &recordings_dir,
                Some(&canon_root),
                &gate,
                snap,
                || (false, true),
                None,
                HistoryManager::unlink_recording_strict_bounded,
                |_d, _r, _f| {
                    StagedWavInspection::Uncertain(StagedWavInspectionReason::PermissionDenied)
                },
                false,
                staged.clone(),
                "important dictated text".into(),
                false,
                None,
                None,
                None,
                None,
            )
            .unwrap();
            assert!(matches!(
                out,
                SaveOutcome::IncompleteAudioInspection {
                    entry: Some(ref e),
                    reason: StagedWavInspectionReason::PermissionDenied,
                    ..
                } if e.file_name.is_empty() && e.transcription_text == "important dictated text"
            ));
            assert!(
                out.is_in_history(),
                "normal save with uncertain audio MUST count as in history for the paste notice"
            );

            // Verify paste notice logic sees `in_history == true`.
            let (on_clipboard, in_history) = crate::clipboard::paste_word_locations_with_privacy(
                ClipboardHandling::DontModify,
                true,
                out.is_in_history(),
                10,
                RecordingRetentionPeriod::Days30,
                false,
            );
            assert!(on_clipboard);
            assert!(
                in_history,
                "paste_word_locations_with_privacy must report in_history=true"
            );

            let err_msg = out
                .incomplete_error_message()
                .expect("incomplete message present");
            assert_path_free_message(
                &err_msg,
                &staged,
                StagedWavInspectionReason::PermissionDenied,
                dir.path(),
            );
            assert_eq!(
                HistoryManager::pending_audio_removal_names_in(&conn).unwrap(),
                vec![staged.clone()]
            );
        }

        println!("AC_C25_PROOF: failed_absent_zero_rows=true failed_cant_tell_zero_rows_kept_and_drained=true normal_cant_tell_in_history_paste_notice=true path_free_reason_kinds=4");
    }
}
