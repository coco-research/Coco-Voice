use enigo::{Enigo, Key, Keyboard, Mouse, Settings};
use log::{error, warn};
use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use tauri::{AppHandle, Manager};

/// Thread id of the Tauri UI thread, recorded from the setup hook.
static UI_THREAD: OnceLock<std::thread::ThreadId> = OnceLock::new();

/// Remember the calling thread as the UI thread.
///
/// The Tauri setup hook runs on the main thread and calls this. A later call
/// does not replace the first id.
pub(crate) fn record_ui_thread() {
    UI_THREAD.get_or_init(|| std::thread::current().id());
}

/// Whether `current_id` is the UI thread.
///
/// Uses the recorded id when setup has run. Before that, a thread named
/// `"main"` counts (Rust names the process main thread `"main"`).
fn thread_is_ui(
    current_id: std::thread::ThreadId,
    current_name: Option<&str>,
    recorded_ui: Option<std::thread::ThreadId>,
) -> bool {
    match recorded_ui {
        Some(ui) => current_id == ui,
        None => current_name == Some("main"),
    }
}

/// Wrapper for Enigo to store in Tauri's managed state.
/// Enigo is wrapped in a Mutex since it requires mutable access.
pub struct EnigoState(pub Mutex<Enigo>);

impl EnigoState {
    pub fn new() -> Result<Self, String> {
        let enigo = Enigo::new(&Settings::default())
            .map_err(|e| format!("Failed to initialize Enigo: {}", e))?;
        Ok(Self(Mutex::new(enigo)))
    }
}

/// Get the current mouse cursor position using the managed Enigo instance.
/// Returns None if the state is not available or if getting the location fails.
pub fn get_cursor_position(app_handle: &AppHandle) -> Option<(i32, i32)> {
    let enigo_state = app_handle.try_state::<EnigoState>()?;
    let enigo = enigo_state.0.lock().ok()?;
    enigo.location().ok()
}

/// `Err` when this thread is the UI thread.
///
/// Compares [`std::thread::ThreadId`] against [`UI_THREAD`]. Until
/// [`record_ui_thread`] runs, falls back to the `"main"` thread name so the
/// check is testable without an [`AppHandle`]. Tauri's event loop is that
/// thread.
///
/// Waiting in [`on_main`] on that thread deadlocks: the wait blocks the only
/// thread that can run the queued closure.
fn ui_thread_paste_guard() -> Result<(), String> {
    let current = std::thread::current();
    if thread_is_ui(current.id(), current.name(), UI_THREAD.get().copied()) {
        error!("paste must not run on the UI thread");
        return Err("paste must not run on the UI thread".to_string());
    }
    Ok(())
}

/// Runs `f` on the UI thread and waits until it finishes, or until 5 seconds pass.
///
/// `run_on_main_thread` only queues the closure. The channel is what makes the
/// caller wait. The closure must not sleep: the caller (a background thread)
/// owns every delay. Calling this on the UI thread returns an error instead
/// of queueing: the wait would freeze the app.
///
/// If the wait times out, the caller marks the hop abandoned. The queued
/// closure checks that flag first and returns without running `f`, so a late
/// hop does not touch Enigo. A panic inside `f` is caught and sent back as
/// `Err`, so the UI event loop keeps running.
pub(crate) fn on_main<T, F>(app: &AppHandle, f: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    ui_thread_paste_guard()?;

    let (tx, rx) = std::sync::mpsc::channel();
    let abandoned = Arc::new(AtomicBool::new(false));
    let abandoned_for_hop = Arc::clone(&abandoned);
    app.run_on_main_thread(move || {
        // Abandon wins over the call: a late hop must not touch Enigo.
        let Some(result) = run_unless_abandoned(&abandoned_for_hop, || {
            match std::panic::catch_unwind(AssertUnwindSafe(f)) {
                Ok(value) => value,
                Err(_) => Err("key event panicked".to_string()),
            }
        }) else {
            return;
        };
        let _ = tx.send(result);
    })
    .map_err(|e| format!("Failed to run on main thread: {e:?}"))?;

    // `Ok` carries the closure's Result. Timeout and a dropped sender are ours.
    match rx.recv_timeout(Duration::from_secs(5)) {
        Ok(result) => result,
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            abandoned.store(true, Ordering::SeqCst);
            Err("main thread did not run the key event in time".to_string())
        }
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            Err("Main thread dropped before the key event finished".to_string())
        }
    }
}

/// Runs `f` unless `abandoned` is set.
///
/// [`on_main`] shares the flag with the queued closure. The timeout path sets
/// it; a closure that has not started yet sees it and skips the call.
fn run_unless_abandoned<R>(abandoned: &AtomicBool, f: impl FnOnce() -> R) -> Option<R> {
    if abandoned.load(Ordering::SeqCst) {
        None
    } else {
        Some(f())
    }
}

/// Locks [`EnigoState`] on the UI thread, runs `f`, and drops the lock before
/// returning. Keyboard events stay on the main thread (macOS TIS/TSM). Do not
/// call this while already holding the Enigo mutex, and do not sleep inside `f`.
pub(crate) fn with_enigo_on_main<T, F>(app: &AppHandle, f: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce(&mut Enigo) -> Result<T, String> + Send + 'static,
{
    let app_for_keys = app.clone();
    on_main(app, move || {
        let state = app_for_keys
            .try_state::<EnigoState>()
            .ok_or_else(|| "Enigo state not initialized".to_string())?;
        let mut enigo = state
            .0
            .lock()
            .map_err(|e| format!("Failed to lock Enigo: {}", e))?;
        f(&mut enigo)
    })
}

/// How long the paste modifier stays down after the V or Insert click.
/// Slept on the caller, never on the UI thread and never while Enigo is locked.
fn sleep_modifier_hold() {
    std::thread::sleep(Duration::from_millis(100));
}

/// Release `key` while already handling another key error. The failure is
/// logged; the caller's error is the one that matters.
fn warn_release(enigo: &mut Enigo, key: Key, name: &str) {
    if let Err(error) = enigo.key(key, enigo::Direction::Release) {
        warn!("Failed to release {name} during paste: {error}");
    }
}

/// Runs `release` on the main thread. If that hop fails, logs and tries the
/// same release once more. The press hop has already succeeded, so a stuck
/// modifier is worse than a second release of a key that is already up.
fn release_hop_with_retry<F>(app: &AppHandle, release: F) -> Result<(), String>
where
    F: Fn(&mut Enigo) -> Result<(), String> + Send + Sync + 'static,
{
    let release = std::sync::Arc::new(release);
    let first = std::sync::Arc::clone(&release);
    if let Err(error) = with_enigo_on_main(app, move |enigo| first(enigo)) {
        warn!("Paste release hop failed, retrying once: {error}");
        with_enigo_on_main(app, move |enigo| release(enigo))
    } else {
        Ok(())
    }
}

/// Sends a Ctrl+V or Cmd+V paste command using platform-specific virtual key codes.
/// This ensures the paste works regardless of keyboard layout (e.g., Russian, AZERTY, DVORAK).
/// Note: On Wayland, this may not work - callers should check for Wayland and use alternative methods.
///
/// The caller must not be the UI thread: the 100ms modifier hold sleeps here.
/// The clipboard paste lock serializes pastes, because the Enigo lock is
/// released during the hold.
pub fn send_paste_ctrl_v(app: &AppHandle) -> Result<(), String> {
    // Platform-specific key definitions
    #[cfg(target_os = "macos")]
    let (modifier_key, v_key_code) = (Key::Meta, Key::Other(9));
    #[cfg(target_os = "windows")]
    let (modifier_key, v_key_code) = (Key::Control, Key::Other(0x56)); // VK_V
    #[cfg(target_os = "linux")]
    let (modifier_key, v_key_code) = (Key::Control, Key::Unicode('v'));

    // Press modifier + V on the main thread, then release after the hold.
    // A failed click still releases the modifier: `?` would skip the release hop
    // and leave Cmd/Ctrl held system-wide.
    with_enigo_on_main(app, move |enigo| {
        enigo
            .key(modifier_key, enigo::Direction::Press)
            .map_err(|e| format!("Failed to press modifier key: {}", e))?;
        if let Err(e) = enigo.key(v_key_code, enigo::Direction::Click) {
            warn_release(enigo, modifier_key, "modifier");
            return Err(format!("Failed to click V key: {}", e));
        }
        Ok(())
    })?;

    sleep_modifier_hold();

    release_hop_with_retry(app, move |enigo| {
        enigo
            .key(modifier_key, enigo::Direction::Release)
            .map_err(|e| format!("Failed to release modifier key: {}", e))?;
        Ok(())
    })?;

    Ok(())
}

/// Sends a Ctrl+Shift+V paste command.
/// This is commonly used in terminal applications on Linux to paste without formatting.
/// Note: On Wayland, this may not work - callers should check for Wayland and use alternative methods.
///
/// The caller must not be the UI thread: the 100ms modifier hold sleeps here.
/// The clipboard paste lock serializes pastes, because the Enigo lock is
/// released during the hold.
pub fn send_paste_ctrl_shift_v(app: &AppHandle) -> Result<(), String> {
    // Platform-specific key definitions
    #[cfg(target_os = "macos")]
    let (modifier_key, v_key_code) = (Key::Meta, Key::Other(9)); // Cmd+Shift+V on macOS
    #[cfg(target_os = "windows")]
    let (modifier_key, v_key_code) = (Key::Control, Key::Other(0x56)); // VK_V
    #[cfg(target_os = "linux")]
    let (modifier_key, v_key_code) = (Key::Control, Key::Unicode('v'));

    // Press Ctrl/Cmd + Shift + V on the main thread, then release after the hold.
    // A failed Shift press or V click still releases whatever was pressed: `?`
    // would skip the release hop and leave the modifiers held system-wide.
    with_enigo_on_main(app, move |enigo| {
        enigo
            .key(modifier_key, enigo::Direction::Press)
            .map_err(|e| format!("Failed to press modifier key: {}", e))?;
        if let Err(e) = enigo.key(Key::Shift, enigo::Direction::Press) {
            warn_release(enigo, modifier_key, "modifier");
            return Err(format!("Failed to press Shift key: {}", e));
        }
        if let Err(e) = enigo.key(v_key_code, enigo::Direction::Click) {
            warn_release(enigo, Key::Shift, "Shift");
            warn_release(enigo, modifier_key, "modifier");
            return Err(format!("Failed to click V key: {}", e));
        }
        Ok(())
    })?;

    sleep_modifier_hold();

    // Release both even if one fails, so neither key stays held; report the first error.
    release_hop_with_retry(app, move |enigo| {
        let shift = enigo
            .key(Key::Shift, enigo::Direction::Release)
            .map_err(|e| format!("Failed to release Shift key: {}", e));
        let modifier = enigo
            .key(modifier_key, enigo::Direction::Release)
            .map_err(|e| format!("Failed to release modifier key: {}", e));
        shift.and(modifier)
    })?;

    Ok(())
}

/// Sends a Shift+Insert paste command (Windows and Linux only).
/// This is more universal for terminal applications and legacy software.
/// Note: On Wayland, this may not work - callers should check for Wayland and use alternative methods.
///
/// The caller must not be the UI thread: the 100ms modifier hold sleeps here.
/// The clipboard paste lock serializes pastes, because the Enigo lock is
/// released during the hold.
pub fn send_paste_shift_insert(app: &AppHandle) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    let insert_key_code = Key::Other(0x2D); // VK_INSERT
    #[cfg(not(target_os = "windows"))]
    let insert_key_code = Key::Other(0x76); // XK_Insert (keycode 118 / 0x76, also used as fallback)

    // Press Shift + Insert on the main thread, then release Shift after the hold.
    // A failed Insert click still releases Shift: `?` would skip the release hop
    // and leave Shift held system-wide.
    with_enigo_on_main(app, move |enigo| {
        enigo
            .key(Key::Shift, enigo::Direction::Press)
            .map_err(|e| format!("Failed to press Shift key: {}", e))?;
        if let Err(e) = enigo.key(insert_key_code, enigo::Direction::Click) {
            warn_release(enigo, Key::Shift, "Shift");
            return Err(format!("Failed to click Insert key: {}", e));
        }
        Ok(())
    })?;

    sleep_modifier_hold();

    release_hop_with_retry(app, move |enigo| {
        enigo
            .key(Key::Shift, enigo::Direction::Release)
            .map_err(|e| format!("Failed to release Shift key: {}", e))?;
        Ok(())
    })?;

    Ok(())
}

/// Pastes text directly using the enigo text method.
/// This tries to use system input methods if possible, otherwise simulates keystrokes one by one.
pub fn paste_text_direct(enigo: &mut Enigo, text: &str) -> Result<(), String> {
    enigo
        .text(text)
        .map_err(|e| format!("Failed to send text directly: {}", e))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{run_unless_abandoned, thread_is_ui, ui_thread_paste_guard};
    use std::sync::atomic::{AtomicBool, Ordering};

    #[test]
    fn ui_thread_match_uses_recorded_id_else_name() {
        let ui = std::thread::current().id();
        let other = std::thread::spawn(|| std::thread::current().id())
            .join()
            .unwrap();

        assert!(thread_is_ui(ui, Some("worker"), Some(ui)));
        // Recorded id wins over the old name check.
        assert!(!thread_is_ui(other, Some("main"), Some(ui)));
        assert!(thread_is_ui(other, Some("main"), None));
        assert!(!thread_is_ui(other, Some("paste-worker"), None));
        assert!(!thread_is_ui(other, None, None));
    }

    #[test]
    fn paste_guard_rejects_thread_named_main() {
        let rejected = std::thread::Builder::new()
            .name("main".into())
            .spawn(ui_thread_paste_guard)
            .unwrap()
            .join()
            .unwrap();
        assert_eq!(rejected.unwrap_err(), "paste must not run on the UI thread");

        let allowed = std::thread::Builder::new()
            .name("paste-worker".into())
            .spawn(ui_thread_paste_guard)
            .unwrap()
            .join()
            .unwrap();
        assert!(allowed.is_ok());
    }

    #[test]
    fn abandoned_hop_does_not_run() {
        let flag = AtomicBool::new(false);
        assert_eq!(run_unless_abandoned(&flag, || 7), Some(7));

        flag.store(true, Ordering::SeqCst);
        let mut ran = false;
        assert!(run_unless_abandoned(&flag, || {
            ran = true;
            7
        })
        .is_none());
        assert!(!ran);
    }
}
