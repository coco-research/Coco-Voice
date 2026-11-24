//! Coco-keys based keyboard shortcut implementation
//!
//! This module provides an alternative to Tauri's global-shortcut plugin
//! using the handy-keys library for more control over keyboard events.
//!
//! ## Architecture
//!
//! The implementation uses a dedicated manager thread that owns the `HotkeyManager`:
//!
//! ```text
//! ┌─────────────────┐     commands      ┌──────────────────────┐
//! │   Main Thread   │ ───────────────▶ │   Manager Thread     │
//! │                 │   (via channel)   │                      │
//! │ - register()    │                   │ - owns HotkeyManager │
//! │ - unregister()  │                   │ - polls for events   │
//! └─────────────────┘                   │ - dispatches actions │
//!                                       └──────────────────────┘
//! ```
//!
//! This design ensures thread-safety since `HotkeyManager` is only accessed
//! from a single thread. Commands (register/unregister) are sent via an mpsc
//! channel and responses are synchronously awaited.
//!
//! ## Recording Mode
//!
//! For UI key capture, a separate `KeyboardListener` is created on-demand and
//! polled from a dedicated recording thread. Events are emitted to the frontend
//! via Tauri's event system.

use handy_keys::{Hotkey, HotkeyId, HotkeyManager, HotkeyState, KeyboardListener};
use log::{debug, error, info};
use serde::Serialize;
use specta::Type;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

use crate::settings::{self, get_settings, ShortcutBinding};

use super::handler::handle_shortcut_event;

/// Maximum time to wait for the hotkey manager thread to initialize before
/// reporting a start failure (R1).
pub(crate) const START_DEADLINE_MS: u64 = 5000;

/// Process-wide lock held across manager check, start and store (R4).
static INIT_LOCK: Mutex<()> = Mutex::new(());

/// Tracks whether a manager thread is alive, including one whose `make` call
/// outlived `START_DEADLINE_MS` (R4 / V1). Set only in `start_with` via
/// `compare_exchange` and cleared only by [`ManagerAliveGuard::drop`].
static MANAGER_ALIVE: AtomicBool = AtomicBool::new(false);

struct ManagerAliveGuard;

impl Drop for ManagerAliveGuard {
    fn drop(&mut self) {
        MANAGER_ALIVE.store(false, Ordering::SeqCst);
    }
}

/// Reason `HotkeyManager` failed to start on the manager thread (R1/R3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum StartError {
    AccessibilityNotGranted,
    Other(String),
}

impl StartError {
    fn from_handy_keys(err: handy_keys::Error) -> Self {
        match err {
            handy_keys::Error::AccessibilityNotGranted => Self::AccessibilityNotGranted,
            other => Self::Other(other.to_string()),
        }
    }

    fn cause(&self) -> super::ShortcutFailureCause {
        match self {
            Self::AccessibilityNotGranted => super::ShortcutFailureCause::Permission,
            Self::Other(_) => super::ShortcutFailureCause::Other,
        }
    }

    fn message(&self) -> String {
        match self {
            Self::AccessibilityNotGranted => handy_keys::Error::AccessibilityNotGranted.to_string(),
            Self::Other(msg) => msg.clone(),
        }
    }
}

/// Commands that can be sent to the hotkey manager thread
enum ManagerCommand {
    Register {
        binding_id: String,
        hotkey_string: String,
        response: Sender<Result<(), String>>,
    },
    Unregister {
        binding_id: String,
        response: Sender<Result<(), String>>,
    },
    Shutdown,
}

/// State for the coco-keys shortcut manager
pub struct CocoVoiceKeysState {
    /// Channel to send commands to the manager thread (wrapped in Mutex for Sync)
    command_sender: Mutex<Sender<ManagerCommand>>,
    /// Handle to the manager thread (wrapped in Mutex for Sync, allows proper join on drop)
    thread_handle: Mutex<Option<JoinHandle<()>>>,
    /// Recording listener for UI key capture (only active during recording)
    recording_listener: Mutex<Option<KeyboardListener>>,
    /// Flag indicating if we're in recording mode
    is_recording: AtomicBool,
    /// The binding ID being recorded (if any)
    recording_binding_id: Mutex<Option<String>>,
    /// Generation of the capture loop that may run. Each start claims a new
    /// generation and stop bumps it, so a stale loop that is still inside its
    /// sleep when the next capture starts sees a different value and exits
    /// instead of reading the new capture's listener.
    recording_generation: Arc<AtomicU64>,
}

/// Claims a fresh generation for a new capture loop, invalidating any older one.
fn begin_capture(generation: &AtomicU64) -> u64 {
    generation.fetch_add(1, Ordering::SeqCst) + 1
}

/// Invalidates whichever capture loop is running.
fn end_capture(generation: &AtomicU64) {
    generation.fetch_add(1, Ordering::SeqCst);
}

/// True while `mine` is still the live capture generation.
fn capture_is_current(generation: &AtomicU64, mine: u64) -> bool {
    generation.load(Ordering::SeqCst) == mine
}

/// Key event sent to frontend during recording mode
#[derive(Debug, Clone, Serialize, Type)]
pub struct FrontendKeyEvent {
    /// Currently pressed modifier keys
    pub modifiers: Vec<String>,
    /// The key that was pressed (if any)
    pub key: Option<String>,
    /// Whether this is a key down event
    pub is_key_down: bool,
    /// The full hotkey string (e.g., "option+space")
    pub hotkey_string: String,
}

impl CocoVoiceKeysState {
    /// Start a new `CocoVoiceKeysState`, waiting up to `deadline` for the
    /// manager thread to construct `HotkeyManager` (R1/R4).
    pub(crate) fn start<F>(app: AppHandle, make: F, deadline: Duration) -> Result<Self, StartError>
    where
        F: FnOnce() -> handy_keys::Result<HotkeyManager> + Send + 'static,
    {
        Self::start_with(
            move |binding_id, hotkey_string, is_pressed| {
                handle_shortcut_event(&app, binding_id, hotkey_string, is_pressed);
            },
            make,
            deadline,
        )
    }

    fn start_with<F, H>(on_event: H, make: F, deadline: Duration) -> Result<Self, StartError>
    where
        F: FnOnce() -> handy_keys::Result<HotkeyManager> + Send + 'static,
        H: Fn(&str, &str, bool) + Send + 'static,
    {
        if MANAGER_ALIVE
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Err(StartError::Other(
                "previous shortcut engine is still stopping".to_string(),
            ));
        }

        // Guard is created before spawning and moved into the manager thread so
        // every exit path — including spawn failure, start error, late success,
        // Disconnected or panic — clears `MANAGER_ALIVE`.
        let alive_guard = ManagerAliveGuard;
        let (cmd_tx, cmd_rx) = mpsc::channel::<ManagerCommand>();
        let (started_tx, started_rx) = mpsc::channel::<Result<(), StartError>>();

        let thread_handle = thread::spawn(move || {
            Self::manager_thread(cmd_rx, on_event, make, started_tx, alive_guard);
        });

        match started_rx.recv_timeout(deadline) {
            Ok(Ok(())) => Ok(Self {
                command_sender: Mutex::new(cmd_tx),
                thread_handle: Mutex::new(Some(thread_handle)),
                recording_listener: Mutex::new(None),
                is_recording: AtomicBool::new(false),
                recording_binding_id: Mutex::new(None),
                recording_generation: Arc::new(AtomicU64::new(0)),
            }),
            Ok(Err(err)) => {
                drop(cmd_tx);
                let _ = thread_handle.join();
                Err(err)
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                let msg = format!(
                    "hotkey manager did not start within {} ms",
                    deadline.as_millis()
                );
                error!("Failed to create HotkeyManager: {}", msg);
                drop(cmd_tx);
                Err(StartError::Other(msg))
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                drop(cmd_tx);
                let _ = thread_handle.join();
                Err(StartError::Other("hotkey manager thread ended".to_string()))
            }
        }
    }

    /// The main manager thread - owns the HotkeyManager and processes commands
    fn manager_thread<F, H>(
        cmd_rx: Receiver<ManagerCommand>,
        on_event: H,
        make: F,
        started: Sender<Result<(), StartError>>,
        _alive_guard: ManagerAliveGuard,
    ) where
        F: FnOnce() -> handy_keys::Result<HotkeyManager>,
        H: Fn(&str, &str, bool),
    {
        info!("coco-keys manager thread started");

        // Create the HotkeyManager in this thread
        let manager = match make() {
            Ok(m) => m,
            Err(e) => {
                error!("Failed to create HotkeyManager: {}", e);
                let _ = started.send(Err(StartError::from_handy_keys(e)));
                return;
            }
        };

        if started.send(Ok(())).is_err() {
            // `start` timed out and dropped the receiver while `make` was still
            // running. Drop the manager at once so no second event tap lives.
            drop(manager);
            info!("coco-keys manager thread exited after late start");
            return;
        }

        // Maps binding IDs to HotkeyIds and hotkey strings
        let mut binding_to_hotkey: HashMap<String, HotkeyId> = HashMap::new();
        let mut hotkey_to_binding: HashMap<HotkeyId, (String, String)> = HashMap::new(); // (binding_id, hotkey_string)

        loop {
            // Check for hotkey events (non-blocking)
            while let Some(event) = manager.try_recv() {
                if let Some((binding_id, hotkey_string)) = hotkey_to_binding.get(&event.id) {
                    debug!(
                        "coco-keys event: binding={}, hotkey={}, state={:?}",
                        binding_id, hotkey_string, event.state
                    );
                    let is_pressed = event.state == HotkeyState::Pressed;
                    on_event(binding_id, hotkey_string, is_pressed);
                }
            }

            // Check for commands (non-blocking with timeout)
            match cmd_rx.recv_timeout(std::time::Duration::from_millis(10)) {
                Ok(cmd) => match cmd {
                    ManagerCommand::Register {
                        binding_id,
                        hotkey_string,
                        response,
                    } => {
                        let result = Self::do_register(
                            &manager,
                            &mut binding_to_hotkey,
                            &mut hotkey_to_binding,
                            &binding_id,
                            &hotkey_string,
                        );
                        let _ = response.send(result);
                    }
                    ManagerCommand::Unregister {
                        binding_id,
                        response,
                    } => {
                        let result = Self::do_unregister(
                            &manager,
                            &mut binding_to_hotkey,
                            &mut hotkey_to_binding,
                            &binding_id,
                        );
                        let _ = response.send(result);
                    }
                    ManagerCommand::Shutdown => {
                        info!("coco-keys manager thread shutting down");
                        break;
                    }
                },
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    // No command, continue
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    info!("Command channel disconnected, shutting down");
                    break;
                }
            }
        }

        info!("coco-keys manager thread stopped");
    }

    /// Register a hotkey
    fn do_register(
        manager: &HotkeyManager,
        binding_to_hotkey: &mut HashMap<String, HotkeyId>,
        hotkey_to_binding: &mut HashMap<HotkeyId, (String, String)>,
        binding_id: &str,
        hotkey_string: &str,
    ) -> Result<(), String> {
        let hotkey: Hotkey = hotkey_string
            .parse()
            .map_err(|e| format!("Failed to parse hotkey '{}': {}", hotkey_string, e))?;

        let id = manager
            .register(hotkey)
            .map_err(|e| format!("Failed to register hotkey: {}", e))?;

        binding_to_hotkey.insert(binding_id.to_string(), id);
        hotkey_to_binding.insert(id, (binding_id.to_string(), hotkey_string.to_string()));

        debug!(
            "Registered coco-keys shortcut: {} -> {:?}",
            binding_id, hotkey
        );
        Ok(())
    }

    /// Unregister a hotkey
    fn do_unregister(
        manager: &HotkeyManager,
        binding_to_hotkey: &mut HashMap<String, HotkeyId>,
        hotkey_to_binding: &mut HashMap<HotkeyId, (String, String)>,
        binding_id: &str,
    ) -> Result<(), String> {
        if let Some(id) = binding_to_hotkey.remove(binding_id) {
            manager
                .unregister(id)
                .map_err(|e| format!("Failed to unregister hotkey: {}", e))?;
            hotkey_to_binding.remove(&id);
            debug!("Unregistered coco-keys shortcut: {}", binding_id);
        }
        Ok(())
    }

    /// Register a shortcut binding
    pub fn register(&self, binding: &ShortcutBinding) -> Result<(), String> {
        let (tx, rx) = mpsc::channel();
        self.command_sender
            .lock()
            .map_err(|_| "Failed to lock command_sender")?
            .send(ManagerCommand::Register {
                binding_id: binding.id.clone(),
                hotkey_string: binding.current_binding.clone(),
                response: tx,
            })
            .map_err(|_| "Failed to send register command")?;

        rx.recv()
            .map_err(|_| "Failed to receive register response")?
    }

    /// Unregister a shortcut binding
    pub fn unregister(&self, binding: &ShortcutBinding) -> Result<(), String> {
        let (tx, rx) = mpsc::channel();
        self.command_sender
            .lock()
            .map_err(|_| "Failed to lock command_sender")?
            .send(ManagerCommand::Unregister {
                binding_id: binding.id.clone(),
                response: tx,
            })
            .map_err(|_| "Failed to send unregister command")?;

        rx.recv()
            .map_err(|_| "Failed to receive unregister response")?
    }

    /// Start recording mode for a specific binding
    pub fn start_recording(&self, app: &AppHandle, binding_id: String) -> Result<(), String> {
        if self.is_recording.load(Ordering::SeqCst) {
            return Err("Already recording".into());
        }

        // Create a new keyboard listener for recording
        let listener = KeyboardListener::new()
            .map_err(|e| format!("Failed to create keyboard listener: {}", e))?;

        {
            let mut recording = self
                .recording_listener
                .lock()
                .map_err(|_| "Failed to lock recording_listener")?;
            *recording = Some(listener);
        }
        {
            let mut binding = self
                .recording_binding_id
                .lock()
                .map_err(|_| "Failed to lock recording_binding_id")?;
            *binding = Some(binding_id);
        }

        self.is_recording.store(true, Ordering::SeqCst);
        let generation = begin_capture(&self.recording_generation);

        // Start a thread to emit key events to the frontend
        let app_clone = app.clone();
        let recording_generation = Arc::clone(&self.recording_generation);
        thread::spawn(move || {
            Self::recording_loop(app_clone, recording_generation, generation);
        });

        debug!("Started coco-keys recording mode");
        Ok(())
    }

    /// Recording loop - emits key events to frontend during recording
    fn recording_loop(app: AppHandle, current: Arc<AtomicU64>, generation: u64) {
        while capture_is_current(&current, generation) {
            let event = {
                let state = match app.try_state::<CocoVoiceKeysState>() {
                    Some(s) => s,
                    None => break,
                };
                let listener = state.recording_listener.lock().ok();
                // Re-check under the lock: stop bumped the generation before it
                // swapped the listener, so a stale loop must not read the next
                // capture's listener.
                if !capture_is_current(&current, generation) {
                    break;
                }
                listener.as_ref().and_then(|l| l.as_ref()?.try_recv())
            };

            if let Some(key_event) = event {
                // Convert to frontend-friendly format
                let frontend_event = FrontendKeyEvent {
                    modifiers: modifiers_to_strings(key_event.modifiers),
                    key: key_event.key.map(|k| k.to_string().to_lowercase()),
                    is_key_down: key_event.is_key_down,
                    hotkey_string: key_event
                        .as_hotkey()
                        .map(|h| h.to_handy_string())
                        .unwrap_or_default(),
                };

                // Emit to frontend
                if let Err(e) = app.emit("coco-keys-event", &frontend_event) {
                    error!("Failed to emit key event: {}", e);
                }
            } else {
                thread::sleep(std::time::Duration::from_millis(10));
            }
        }

        debug!("Recording loop ended");
    }

    /// Stop recording mode
    pub fn stop_recording(&self) -> Result<(), String> {
        self.is_recording.store(false, Ordering::SeqCst);
        end_capture(&self.recording_generation);

        {
            let mut recording = self
                .recording_listener
                .lock()
                .map_err(|_| "Failed to lock recording_listener")?;
            *recording = None;
        }
        {
            let mut binding = self
                .recording_binding_id
                .lock()
                .map_err(|_| "Failed to lock recording_binding_id")?;
            *binding = None;
        }

        debug!("Stopped coco-keys recording mode");
        Ok(())
    }
}

impl Drop for CocoVoiceKeysState {
    fn drop(&mut self) {
        // Signal recording to stop
        end_capture(&self.recording_generation);
        self.is_recording.store(false, Ordering::SeqCst);

        // Send shutdown command
        if let Ok(sender) = self.command_sender.lock() {
            let _ = sender.send(ManagerCommand::Shutdown);
        }

        // Wait for the manager thread to finish
        if let Ok(mut handle) = self.thread_handle.lock() {
            if let Some(h) = handle.take() {
                let _ = h.join();
            }
        }
    }
}

/// Convert handy-keys Modifiers to a list of strings
fn modifiers_to_strings(modifiers: handy_keys::Modifiers) -> Vec<String> {
    let mut result = Vec::new();

    if modifiers.contains(handy_keys::Modifiers::CTRL) {
        result.push("ctrl".to_string());
    }
    if modifiers.contains(handy_keys::Modifiers::OPT) {
        #[cfg(target_os = "macos")]
        result.push("option".to_string());
        #[cfg(not(target_os = "macos"))]
        result.push("alt".to_string());
    }
    if modifiers.contains(handy_keys::Modifiers::SHIFT) {
        result.push("shift".to_string());
    }
    if modifiers.contains(handy_keys::Modifiers::CMD) {
        #[cfg(target_os = "macos")]
        result.push("command".to_string());
        #[cfg(not(target_os = "macos"))]
        result.push("super".to_string());
    }
    if modifiers.contains(handy_keys::Modifiers::FN) {
        result.push("fn".to_string());
    }

    result
}

/// Validate a shortcut string for the CocoVoiceKeys implementation.
/// CocoVoiceKeys is more permissive: allows modifier-only combos and the fn key.
pub fn validate_shortcut(raw: &str) -> Result<(), String> {
    if raw.trim().is_empty() {
        return Err("Shortcut cannot be empty".into());
    }
    // CocoVoiceKeys accepts modifier-only, key-only, and modifier+key combos
    // Just verify the string is parseable
    raw.parse::<Hotkey>()
        .map(|_| ())
        .map_err(|e| format!("Invalid shortcut for CocoVoiceKeys: {}", e))
}

fn init_shortcuts_core<R, StartFn, RegFn>(
    app: &AppHandle<R>,
    start_fn: StartFn,
    register_active: RegFn,
) -> Result<Vec<super::ShortcutFailure>, String>
where
    R: tauri::Runtime,
    StartFn: FnOnce() -> Result<CocoVoiceKeysState, StartError>,
    RegFn: FnOnce(&AppHandle<R>) -> Vec<super::ShortcutFailure>,
{
    let _guard = INIT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    if app.try_state::<CocoVoiceKeysState>().is_none() {
        match start_fn() {
            Ok(state) => {
                app.manage(state);
            }
            Err(err) => {
                let failures = super::start_failures(app, err.cause(), &err.message());
                for failure in &failures {
                    error!(
                        "Failed to start coco-keys for shortcut {} during init: {}",
                        failure.id, failure.error
                    );
                }
                return Ok(failures);
            }
        }
    }

    let failures = register_active(app);
    for failure in &failures {
        error!(
            "Failed to register coco-keys shortcut {} during init: {}",
            failure.id, failure.error
        );
    }
    info!("coco-keys shortcuts initialized");
    Ok(failures)
}

/// Initialize coco-keys shortcuts using a custom `HotkeyManager` factory (test seam).
pub(crate) fn init_shortcuts_with<F>(
    app: &AppHandle,
    make: F,
) -> Result<Vec<super::ShortcutFailure>, String>
where
    F: FnOnce() -> handy_keys::Result<HotkeyManager> + Send + 'static,
{
    let app_for_start = app.clone();
    init_shortcuts_core(
        app,
        move || {
            CocoVoiceKeysState::start(
                app_for_start,
                make,
                Duration::from_millis(START_DEADLINE_MS),
            )
        },
        super::register_active_shortcuts,
    )
}

/// Initialize coco-keys shortcuts.
///
/// Start failures store nothing and return `Ok(failures)` (one per active
/// binding) so a later call can retry once Accessibility or the OS allows it
/// without switching `keyboard_implementation`.
pub fn init_shortcuts(app: &AppHandle) -> Result<Vec<super::ShortcutFailure>, String> {
    init_shortcuts_with(app, HotkeyManager::new_with_blocking)
}

/// Register the cancel shortcut (called when recording starts)
pub fn register_cancel_shortcut(app: &AppHandle) {
    // Disabled on Linux due to instability
    #[cfg(target_os = "linux")]
    {
        let _ = app;
        return;
    }

    #[cfg(not(target_os = "linux"))]
    {
        let app_clone = app.clone();
        super::run_ordered(move || {
            if let Some(cancel_binding) = get_settings(&app_clone).bindings.get("cancel").cloned() {
                if let Some(state) = app_clone.try_state::<CocoVoiceKeysState>() {
                    if let Err(e) = state.register(&cancel_binding) {
                        error!("Failed to register cancel shortcut: {}", e);
                    }
                }
            }
        });
    }
}

/// Unregister the cancel shortcut (called when recording stops)
pub fn unregister_cancel_shortcut(app: &AppHandle) {
    #[cfg(target_os = "linux")]
    {
        let _ = app;
        return;
    }

    #[cfg(not(target_os = "linux"))]
    {
        let app_clone = app.clone();
        super::run_ordered(move || {
            if let Some(cancel_binding) = get_settings(&app_clone).bindings.get("cancel").cloned() {
                if let Some(state) = app_clone.try_state::<CocoVoiceKeysState>() {
                    let _ = state.unregister(&cancel_binding);
                }
            }
        });
    }
}

/// Register a shortcut
pub fn register_shortcut(app: &AppHandle, binding: ShortcutBinding) -> Result<(), String> {
    let state = app
        .try_state::<CocoVoiceKeysState>()
        .ok_or("CocoVoiceKeysState not initialized")?;
    state.register(&binding)
}

/// Unregister a shortcut
pub fn unregister_shortcut(app: &AppHandle, binding: ShortcutBinding) -> Result<(), String> {
    let state = app
        .try_state::<CocoVoiceKeysState>()
        .ok_or("CocoVoiceKeysState not initialized")?;
    state.unregister(&binding)
}

/// Start key recording mode
#[tauri::command]
#[specta::specta]
pub fn start_handy_keys_recording(app: AppHandle, binding_id: String) -> Result<(), String> {
    let settings = get_settings(&app);
    if settings.keyboard_implementation != settings::KeyboardImplementation::CocoVoiceKeys {
        return Err("coco-keys is not the active keyboard implementation".into());
    }

    let state = app
        .try_state::<CocoVoiceKeysState>()
        .ok_or("CocoVoiceKeysState not initialized")?;
    state.start_recording(&app, binding_id)
}

/// Stop key recording mode
#[tauri::command]
#[specta::specta]
pub fn stop_handy_keys_recording(app: AppHandle) -> Result<(), String> {
    let settings = get_settings(&app);
    if settings.keyboard_implementation != settings::KeyboardImplementation::CocoVoiceKeys {
        return Err("coco-keys is not the active keyboard implementation".into());
    }

    let state = app
        .try_state::<CocoVoiceKeysState>()
        .ok_or("CocoVoiceKeysState not initialized")?;
    state.stop_recording()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_capture_loop_exits_when_a_new_capture_starts() {
        // Cancel then immediately start another binding's capture (#19). With one
        // shared flag the old loop saw `true` again and kept running.
        let generation = AtomicU64::new(0);
        let first = begin_capture(&generation);
        assert!(capture_is_current(&generation, first));

        end_capture(&generation);
        let second = begin_capture(&generation);

        assert!(!capture_is_current(&generation, first));
        assert!(capture_is_current(&generation, second));

        end_capture(&generation);
        assert!(!capture_is_current(&generation, second));
    }

    static TEST_SERIAL: Mutex<()> = Mutex::new(());

    fn wait_for_manager_exit() {
        let deadline = std::time::Instant::now() + Duration::from_secs(1);
        while MANAGER_ALIVE.load(Ordering::SeqCst) {
            assert!(
                std::time::Instant::now() < deadline,
                "MANAGER_ALIVE did not clear within 1 s"
            );
            thread::sleep(Duration::from_millis(5));
        }
    }

    fn init_shortcuts_for_test<R, F>(
        app: &AppHandle<R>,
        make: F,
        deadline: Duration,
    ) -> Result<Vec<super::super::ShortcutFailure>, String>
    where
        R: tauri::Runtime,
        F: FnOnce() -> handy_keys::Result<HotkeyManager> + Send + 'static,
    {
        init_shortcuts_core(
            app,
            move || CocoVoiceKeysState::start_with(|_, _, _| {}, make, deadline),
            |_| Vec::new(),
        )
    }

    fn mock_app() -> tauri::App<tauri::test::MockRuntime> {
        tauri::test::mock_builder()
            .plugin(tauri_plugin_store::Builder::default().build())
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock app")
    }

    #[test]
    fn t_g1_accessibility_not_granted_returns_permission_failures_and_stores_nothing() {
        let _serial = TEST_SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        wait_for_manager_exit();
        let app = mock_app();

        let failures = init_shortcuts_for_test(
            app.handle(),
            || Err(handy_keys::Error::AccessibilityNotGranted),
            Duration::from_millis(500),
        )
        .expect("start failure must return Ok(failures), not Err");

        assert!(
            !failures.is_empty(),
            "expected one failure per active binding"
        );
        assert!(
            failures
                .iter()
                .all(|f| f.cause == super::super::ShortcutFailureCause::Permission),
            "all failures must carry cause Permission: {failures:?}"
        );
        assert!(app.try_state::<CocoVoiceKeysState>().is_none());
        wait_for_manager_exit();
    }

    #[test]
    fn t_g2_retry_after_failed_start_invokes_make_again() {
        use std::sync::atomic::AtomicUsize;

        let _serial = TEST_SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        wait_for_manager_exit();
        let app = mock_app();
        let calls = Arc::new(AtomicUsize::new(0));

        for expected in 1..=2 {
            let calls_clone = Arc::clone(&calls);
            let failures = init_shortcuts_for_test(
                app.handle(),
                move || {
                    calls_clone.fetch_add(1, Ordering::SeqCst);
                    Err(handy_keys::Error::AccessibilityNotGranted)
                },
                Duration::from_millis(500),
            )
            .expect("start failure returns Ok(failures)");
            assert!(!failures.is_empty());
            assert_eq!(calls.load(Ordering::SeqCst), expected);
            assert!(app.try_state::<CocoVoiceKeysState>().is_none());
        }
        wait_for_manager_exit();
    }

    #[test]
    fn t_g3_event_tap_creation_failed_returns_other_stores_nothing_and_retries() {
        use std::sync::atomic::AtomicUsize;

        let _serial = TEST_SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        wait_for_manager_exit();
        let app = mock_app();
        let calls = Arc::new(AtomicUsize::new(0));

        for expected in 1..=2 {
            let calls_clone = Arc::clone(&calls);
            let failures = init_shortcuts_for_test(
                app.handle(),
                move || {
                    calls_clone.fetch_add(1, Ordering::SeqCst);
                    Err(handy_keys::Error::EventTapCreationFailed("x".to_string()))
                },
                Duration::from_millis(500),
            )
            .expect("EventTapCreationFailed must return Ok(failures), not Err");
            assert!(!failures.is_empty());
            assert!(
                failures
                    .iter()
                    .all(|f| f.cause == super::super::ShortcutFailureCause::Other),
                "expected Other cause: {failures:?}"
            );
            assert!(app.try_state::<CocoVoiceKeysState>().is_none());
            assert_eq!(calls.load(Ordering::SeqCst), expected);
        }
        wait_for_manager_exit();
    }

    #[test]
    fn t_g6_start_timeout_returns_other_stores_nothing_and_retries() {
        use std::sync::atomic::AtomicUsize;

        let _serial = TEST_SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        wait_for_manager_exit();
        let app = mock_app();
        let calls = Arc::new(AtomicUsize::new(0));
        let (release_tx, release_rx) = mpsc::channel::<()>();

        let c1 = Arc::clone(&calls);
        let failures = init_shortcuts_for_test(
            app.handle(),
            move || {
                c1.fetch_add(1, Ordering::SeqCst);
                let _ = release_rx.recv_timeout(Duration::from_secs(2));
                Err(handy_keys::Error::EventTapCreationFailed(
                    "late".to_string(),
                ))
            },
            Duration::from_millis(50),
        )
        .expect("timeout returns Ok(failures)");

        assert!(!failures.is_empty());
        assert!(failures.iter().all(|f| {
            f.cause == super::super::ShortcutFailureCause::Other
                && f.error.contains("did not start within")
        }));
        assert!(app.try_state::<CocoVoiceKeysState>().is_none());

        let _ = release_tx.send(());
        wait_for_manager_exit();

        let c2 = Arc::clone(&calls);
        let retry_failures = init_shortcuts_for_test(
            app.handle(),
            move || {
                c2.fetch_add(1, Ordering::SeqCst);
                Err(handy_keys::Error::AccessibilityNotGranted)
            },
            Duration::from_millis(500),
        )
        .expect("retry after timeout returns Ok(failures)");
        assert!(!retry_failures.is_empty());
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        wait_for_manager_exit();
    }

    #[test]
    fn t_g9_slow_make_blocks_concurrent_retry_until_old_thread_exits() {
        use std::sync::atomic::AtomicUsize;

        let _serial = TEST_SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        wait_for_manager_exit();
        let app = mock_app();
        let calls = Arc::new(AtomicUsize::new(0));
        let in_flight = Arc::new(AtomicUsize::new(0));
        let max_in_flight = Arc::new(AtomicUsize::new(0));

        let track_make = |sleep_ms: u64,
                          calls: Arc<AtomicUsize>,
                          in_flight: Arc<AtomicUsize>,
                          max_in_flight: Arc<AtomicUsize>| {
            move || {
                calls.fetch_add(1, Ordering::SeqCst);
                let cur = in_flight.fetch_add(1, Ordering::SeqCst) + 1;
                max_in_flight.fetch_max(cur, Ordering::SeqCst);
                if sleep_ms > 0 {
                    thread::sleep(Duration::from_millis(sleep_ms));
                }
                in_flight.fetch_sub(1, Ordering::SeqCst);
                Err(handy_keys::Error::EventTapCreationFailed(
                    "slow".to_string(),
                ))
            }
        };

        // 1. Slow make (200 ms) times out at 50 ms.
        let first = init_shortcuts_for_test(
            app.handle(),
            track_make(
                200,
                Arc::clone(&calls),
                Arc::clone(&in_flight),
                Arc::clone(&max_in_flight),
            ),
            Duration::from_millis(50),
        )
        .expect("first call returns Ok(failures)");
        assert!(!first.is_empty());
        assert!(first.iter().all(|f| {
            f.cause == super::super::ShortcutFailureCause::Other
                && f.error.contains("did not start within")
        }));
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        // 2. Immediate retry while old thread is still alive is refused without calling make.
        let second = init_shortcuts_for_test(
            app.handle(),
            track_make(
                0,
                Arc::clone(&calls),
                Arc::clone(&in_flight),
                Arc::clone(&max_in_flight),
            ),
            Duration::from_millis(50),
        )
        .expect("immediate retry returns Ok(failures)");
        assert!(!second.is_empty());
        assert!(second.iter().all(|f| {
            f.cause == super::super::ShortcutFailureCause::Other
                && f.error
                    .contains("previous shortcut engine is still stopping")
        }));
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        // 3. Once the old thread exits (within 1 s), a retry calls make (count 2).
        wait_for_manager_exit();
        let third = init_shortcuts_for_test(
            app.handle(),
            track_make(
                0,
                Arc::clone(&calls),
                Arc::clone(&in_flight),
                Arc::clone(&max_in_flight),
            ),
            Duration::from_millis(500),
        )
        .expect("retry after exit returns Ok(failures)");
        assert!(!third.is_empty());
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert_eq!(max_in_flight.load(Ordering::SeqCst), 1);
        wait_for_manager_exit();
    }
}
