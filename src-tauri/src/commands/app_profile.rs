//! Commands for per-application profile support.
//!
//! Provides active window detection so the frontend can match the current
//! frontmost application against user-configured profiles and apply
//! post-processing overrides (prompt, provider, model, corrections).

use serde::Serialize;
use specta::Type;
use std::time::Duration;

/// Window queries can block on an accessibility prompt. Every caller waits at
/// most this long, then carries on without an active app.
const ACTIVE_WINDOW_QUERY_TIMEOUT: Duration = Duration::from_millis(500);

/// Information about the currently active/frontmost application.
/// Returned by `get_active_app_info` for profile matching in the frontend.
#[derive(Debug, Clone, Serialize, Type)]
pub struct ActiveAppInfo {
    /// Human-readable application name (e.g. "Xcode", "Visual Studio Code").
    pub app_name: String,
    /// Full path to the application executable or bundle.
    /// On macOS this is the `.app` bundle path; on Windows/Linux the exe path.
    pub process_path: String,
    /// Window title of the active window.
    pub title: String,
}

/// Get information about the currently active/frontmost application.
///
/// Returns `None` if the active window cannot be detected (e.g. no GUI session,
/// permissions denied on macOS, or platform API failure). The frontend uses
/// this to match against configured `AppProfile` entries and apply overrides.
fn read_active_app() -> Option<ActiveAppInfo> {
    match active_win_pos_rs::get_active_window() {
        Ok(window) => Some(ActiveAppInfo {
            app_name: window.app_name,
            process_path: window.process_path.to_string_lossy().to_string(),
            title: window.title,
        }),
        // active-win-pos-rs 0.11 returns Err(()) with no platform message.
        Err(err) => {
            log::debug!("Failed to detect active window for profile matching: {err:?}");
            None
        }
    }
}

/// Reads the active window off the async worker, giving up after
/// [`ACTIVE_WINDOW_QUERY_TIMEOUT`]. Dropping the join handle does not cancel
/// the blocking thread. `Ok(None)` means the platform reported no active
/// window; `Err` means the query timed out or its task failed.
pub(crate) async fn query_active_app() -> Result<Option<ActiveAppInfo>, String> {
    let query = tauri::async_runtime::spawn_blocking(read_active_app);
    match tokio::time::timeout(ACTIVE_WINDOW_QUERY_TIMEOUT, query).await {
        Ok(Ok(info)) => Ok(info),
        Ok(Err(err)) => Err(format!("Active window detection task failed: {err}")),
        Err(_) => Err(format!(
            "Active window detection timed out after {} ms",
            ACTIVE_WINDOW_QUERY_TIMEOUT.as_millis()
        )),
    }
}

/// Window queries can block on an accessibility prompt, so this stays off the
/// webview thread and gives up after [`ACTIVE_WINDOW_QUERY_TIMEOUT`] with an
/// error instead of leaving the caller waiting.
#[specta::specta]
#[tauri::command]
pub async fn get_active_app_info() -> Result<Option<ActiveAppInfo>, String> {
    query_active_app().await
}
