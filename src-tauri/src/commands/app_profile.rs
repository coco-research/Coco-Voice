//! Commands for per-application profile support.
//!
//! Provides active window detection so the frontend can match the current
//! frontmost application against user-configured profiles and apply
//! post-processing overrides (prompt, provider, model, corrections).

use serde::Serialize;
use specta::Type;

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

/// Window queries can block on an accessibility prompt, so this stays off the
/// webview thread.
#[specta::specta]
#[tauri::command]
pub async fn get_active_app_info() -> Option<ActiveAppInfo> {
    match tauri::async_runtime::spawn_blocking(read_active_app).await {
        Ok(info) => info,
        Err(err) => {
            log::debug!("Active window detection task failed: {err}");
            None
        }
    }
}
