use crate::managers::audio::AudioRecordingManager;
use crate::managers::transcription::TranscriptionManager;
use crate::shortcut;
use crate::TranscriptionCoordinator;
use log::info;
use std::sync::Arc;
use tauri::{AppHandle, Manager};

// Re-export all utility modules for easy access
// pub use crate::audio_feedback::*;
pub use crate::clipboard::*;
pub use crate::overlay::*;
pub use crate::tray::*;

trait CancelApp {
    fn try_state<T: Send + Sync + 'static>(&self) -> Option<&T>;

    #[cfg(test)]
    #[allow(dead_code)]
    fn state<T: Send + Sync + 'static>(&self) -> &T {
        self.try_state::<T>()
            .expect("state() called before manage()")
    }

    fn unregister_cancel_shortcut(&self);
    fn set_tray_idle(&self);
    fn hide_overlay(&self);
}

impl CancelApp for AppHandle {
    fn try_state<T: Send + Sync + 'static>(&self) -> Option<&T> {
        Manager::try_state::<T>(self).map(|state| state.inner())
    }

    fn unregister_cancel_shortcut(&self) {
        shortcut::unregister_cancel_shortcut(self);
    }

    fn set_tray_idle(&self) {
        change_tray_icon(self, crate::tray::TrayIconState::Idle);
    }

    fn hide_overlay(&self) {
        hide_recording_overlay(self);
    }
}

fn update_idle_ui<A: CancelApp>(app: &A) {
    let (tray, tray_state) = (
        app.try_state::<tauri::tray::TrayIcon>(),
        app.try_state::<crate::tray::CurrentTrayIconState>(),
    );
    if tray.is_some() && tray_state.is_some() {
        app.set_tray_idle();
    }
    app.hide_overlay();
}

fn cancel_current_operation_with<A: CancelApp>(app: &A) {
    info!("Initiating operation cancellation...");

    let (audio_manager, tm) = (
        app.try_state::<Arc<AudioRecordingManager>>(),
        app.try_state::<Arc<TranscriptionManager>>(),
    );
    let (Some(audio_manager), Some(tm)) = (audio_manager, tm) else {
        info!("Cancellation requested before managers exist; skipping");
        return;
    };

    // Unregister the cancel shortcut asynchronously
    app.unregister_cancel_shortcut();

    // Cancel any ongoing recording
    let recording_was_active = audio_manager.is_recording();
    audio_manager.cancel_recording();

    // Abandon any live streaming transcription
    tm.cancel_stream();

    // Update tray icon and hide overlay
    update_idle_ui(app);

    // Unload model if immediate unload is enabled
    tm.maybe_unload_immediately("cancellation");

    // Notify coordinator so it can keep lifecycle state coherent.
    if let Some(coordinator) = app.try_state::<TranscriptionCoordinator>() {
        coordinator.notify_cancel(recording_was_active);
    }

    info!("Operation cancellation completed - returned to idle state");
}

/// Centralized cancellation function that can be called from anywhere in the app.
/// Handles cancelling both recording and transcription operations and updates UI state.
pub fn cancel_current_operation(app: &AppHandle) {
    cancel_current_operation_with(app);
}

/// Check if using the Wayland display server protocol
#[cfg(target_os = "linux")]
pub fn is_wayland() -> bool {
    std::env::var("WAYLAND_DISPLAY").is_ok()
        || std::env::var("XDG_SESSION_TYPE")
            .map(|v| v.to_lowercase() == "wayland")
            .unwrap_or(false)
}

/// Check if running on KDE Plasma desktop environment
#[cfg(target_os = "linux")]
pub fn is_kde_plasma() -> bool {
    std::env::var("XDG_CURRENT_DESKTOP")
        .map(|v| v.to_uppercase().contains("KDE"))
        .unwrap_or(false)
        || std::env::var("KDE_SESSION_VERSION").is_ok()
}

/// Check if running on KDE Plasma with Wayland
#[cfg(target_os = "linux")]
pub fn is_kde_wayland() -> bool {
    is_wayland() && is_kde_plasma()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Default)]
    struct StartupApp {
        shortcut_unregistered: AtomicUsize,
        tray_updated: AtomicUsize,
        overlay_hidden: AtomicUsize,
    }

    impl CancelApp for StartupApp {
        fn try_state<T: Send + Sync + 'static>(&self) -> Option<&T> {
            None
        }

        fn unregister_cancel_shortcut(&self) {
            self.shortcut_unregistered.fetch_add(1, Ordering::SeqCst);
        }

        fn set_tray_idle(&self) {
            self.tray_updated.fetch_add(1, Ordering::SeqCst);
        }

        fn hide_overlay(&self) {
            self.overlay_hidden.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[test]
    fn cancel_before_managers_exist_returns_without_panicking() {
        let app = StartupApp::default();
        cancel_current_operation_with(&app);
        assert_eq!(app.shortcut_unregistered.load(Ordering::SeqCst), 0);
        assert_eq!(app.tray_updated.load(Ordering::SeqCst), 0);
        assert_eq!(app.overlay_hidden.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn idle_ui_update_before_tray_exists_skips_tray_without_panicking() {
        let app = StartupApp::default();
        update_idle_ui(&app);
        assert_eq!(app.tray_updated.load(Ordering::SeqCst), 0);
        assert_eq!(app.overlay_hidden.load(Ordering::SeqCst), 1);
    }
}
