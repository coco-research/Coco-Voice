use crate::input;
use crate::settings;
use crate::settings::{OverlayPosition, OverlayStyle};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize};

#[cfg(not(target_os = "macos"))]
use log::debug;

#[cfg(not(target_os = "macos"))]
use tauri::WebviewWindowBuilder;

#[cfg(target_os = "macos")]
use tauri::WebviewUrl;

#[cfg(target_os = "macos")]
use tauri_nspanel::{tauri_panel, CollectionBehavior, PanelBuilder, PanelLevel, StyleMask};

#[cfg(target_os = "linux")]
use gtk_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

#[cfg(target_os = "linux")]
use std::env;

#[cfg(target_os = "macos")]
tauri_panel! {
    panel!(RecordingOverlayPanel {
        config: {
            can_become_key_window: false,
            is_floating_panel: true
        }
    })
}

// Native overlay window sizes (logical points). One window is reused for every
// state and resized in `show_overlay_state`; each size need only be at least as
// large as the card it hosts (the `--ov-*` vars in RecordingOverlay.css). The
// card is CSS-anchored flush to the screen edge, so window height doesn't move
// where the card sits — only OVERLAY_TOP_OFFSET / OVERLAY_BOTTOM_OFFSET do. Keep
// these in sync with the CSS card geometry.
//
// Compact overlay (Minimal / transcribing / processing): the 40h pill animates
// width from 172 (--ov-rest-w) to 216 (--ov-work-w) and expands from center, so
// the window must fit the widest state plus a little slack.
const OVERLAY_WIDTH: f64 = 256.0;
const OVERLAY_HEIGHT: f64 = 46.0;

// Actual is 394x118, just a little extra
const OVERLAY_STREAM_WIDTH: f64 = 400.0;
const OVERLAY_STREAM_HEIGHT: f64 = 120.0;

/// Counts overlay shows, so a deferred hide can tell that a newer overlay replaced
/// the one it was meant to hide.
static OVERLAY_SHOW_GENERATION: Mutex<u64> = Mutex::new(0);

/// A dictation hide and a meeting start share this. The hide reads meeting
/// state and hides while holding it; the start commits Recording and shows
/// the pill while holding it. A hide cannot land after a start it did not see.
#[cfg(feature = "meetings")]
static MEETING_PILL_GATE: Mutex<()> = Mutex::new(());

#[cfg(feature = "meetings")]
pub(crate) fn with_meeting_pill_gate<T>(f: impl FnOnce() -> T) -> T {
    let _guard = MEETING_PILL_GATE
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    f()
}

fn current_generation(generation: &Mutex<u64>) -> u64 {
    *generation.lock().unwrap_or_else(|e| e.into_inner())
}

fn bump_generation(generation: &Mutex<u64>) {
    *generation.lock().unwrap_or_else(|e| e.into_inner()) += 1;
}

/// The current show count, to pass to [`hide_recording_overlay_if_current`].
pub fn capture_overlay_generation() -> u64 {
    current_generation(&OVERLAY_SHOW_GENERATION)
}

/// Overlay window size (logical) for a given UI state.
fn overlay_dimensions(state: &str) -> (f64, f64) {
    if state == "streaming" {
        (OVERLAY_STREAM_WIDTH, OVERLAY_STREAM_HEIGHT)
    } else {
        (OVERLAY_WIDTH, OVERLAY_HEIGHT)
    }
}

static LAST_MIC_LEVEL_EMIT: AtomicU64 = AtomicU64::new(0);
const EMIT_THROTTLE_MS: u64 = 33; // ~30 FPS

#[cfg(target_os = "macos")]
const OVERLAY_TOP_OFFSET: f64 = 46.0;
#[cfg(any(target_os = "windows", target_os = "linux"))]
const OVERLAY_TOP_OFFSET: f64 = 4.0;

#[cfg(target_os = "macos")]
const OVERLAY_BOTTOM_OFFSET: f64 = 15.0;

#[cfg(any(target_os = "windows", target_os = "linux"))]
const OVERLAY_BOTTOM_OFFSET: f64 = 40.0;

#[cfg(target_os = "linux")]
fn update_gtk_layer_shell_anchors(overlay_window: &tauri::webview::WebviewWindow) {
    let window_clone = overlay_window.clone();
    let _ = overlay_window.run_on_main_thread(move || {
        // Try to get the GTK window from the Tauri webview
        if let Ok(gtk_window) = window_clone.gtk_window() {
            let settings = settings::get_settings(window_clone.app_handle());
            match settings.overlay_position {
                OverlayPosition::Top => {
                    gtk_window.set_anchor(Edge::Top, true);
                    gtk_window.set_anchor(Edge::Bottom, false);
                }
                OverlayPosition::Bottom => {
                    gtk_window.set_anchor(Edge::Bottom, true);
                    gtk_window.set_anchor(Edge::Top, false);
                }
            }
        }
    });
}

/// Returns true when an environment variable value is truthy
/// (e.g. "1", "true", "yes", "on").
/// "0", "false", "no", "off" and empty string are treated as falsy (case-insensitive).
#[cfg(any(target_os = "linux", test))]
fn is_truthy_env_value(value: &str) -> bool {
    !matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "" | "0" | "false" | "no" | "off"
    )
}

/// Resolves the GTK layer-shell disable flag: `COCO_VOICE_NO_GTK_LAYER_SHELL`
/// takes precedence whenever set; otherwise falls back silently to the legacy
/// variable so existing Linux setups keep working.
#[cfg(any(target_os = "linux", test))]
fn gtk_layer_shell_disabled_from(primary: Option<&str>, legacy: Option<&str>) -> bool {
    match primary {
        Some(v) => is_truthy_env_value(v),
        None => legacy.is_some_and(is_truthy_env_value),
    }
}

#[cfg(target_os = "linux")]
fn gtk_layer_shell_disabled() -> bool {
    let primary = env::var("COCO_VOICE_NO_GTK_LAYER_SHELL").ok();
    let legacy = if primary.is_none() {
        env::var("HANDY_NO_GTK_LAYER_SHELL").ok()
    } else {
        None
    };
    gtk_layer_shell_disabled_from(primary.as_deref(), legacy.as_deref())
}

/// Initializes GTK layer shell for Linux overlay window
/// Returns true if layer shell was successfully initialized, false otherwise
#[cfg(target_os = "linux")]
fn init_gtk_layer_shell(overlay_window: &tauri::webview::WebviewWindow) -> bool {
    if gtk_layer_shell_disabled() {
        debug!("Skipping GTK layer shell init (COCO_VOICE_NO_GTK_LAYER_SHELL is enabled)");
        return false;
    }

    if !gtk_layer_shell::is_supported() {
        return false;
    }

    // Try to get the GTK window from the Tauri webview
    if let Ok(gtk_window) = overlay_window.gtk_window() {
        // Initialize layer shell
        gtk_window.init_layer_shell();
        gtk_window.set_layer(Layer::Overlay);
        gtk_window.set_keyboard_mode(KeyboardMode::None);
        gtk_window.set_exclusive_zone(0);

        update_gtk_layer_shell_anchors(overlay_window);

        return true;
    }
    false
}

/// Forces a window to be topmost using Win32 API (Windows only)
/// This is more reliable than Tauri's set_always_on_top which can be overridden
#[cfg(target_os = "windows")]
fn force_overlay_topmost(overlay_window: &tauri::webview::WebviewWindow) {
    use windows::Win32::UI::WindowsAndMessaging::{
        SetWindowPos, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW,
    };

    // Clone because run_on_main_thread takes 'static
    let overlay_clone = overlay_window.clone();

    // Make sure the Win32 call happens on the UI thread
    let _ = overlay_clone.clone().run_on_main_thread(move || {
        if let Ok(hwnd) = overlay_clone.hwnd() {
            unsafe {
                // Force Z-order: make this window topmost without changing size/pos or stealing focus
                let _ = SetWindowPos(
                    hwnd,
                    Some(HWND_TOPMOST),
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
                );
            }
        }
    });
}

fn get_monitor_with_cursor(app_handle: &AppHandle) -> Option<tauri::Monitor> {
    if let Some(mouse_location) = input::get_cursor_position(app_handle) {
        if let Ok(monitors) = app_handle.available_monitors() {
            for monitor in monitors {
                // Tauri's monitor position/size are physical pixels, but enigo
                // may return logical coordinates (confirmed on macOS via
                // NSEvent::mouseLocation; on Windows, GetCursorPos behavior
                // depends on the process DPI-awareness context). Dividing by
                // scale_factor normalizes to logical, which is safe regardless:
                // if enigo returns logical it matches directly, and if it returns
                // physical on a scale=1 monitor the division is a no-op.
                let scale = monitor.scale_factor();
                let pos = PhysicalPosition::new(
                    (monitor.position().x as f64 / scale) as i32,
                    (monitor.position().y as f64 / scale) as i32,
                );
                let size = PhysicalSize::new(
                    (monitor.size().width as f64 / scale) as u32,
                    (monitor.size().height as f64 / scale) as u32,
                );
                if is_mouse_within_monitor(mouse_location, &pos, &size) {
                    return Some(monitor);
                }
            }
        }
    }

    app_handle.primary_monitor().ok().flatten()
}

#[cfg(feature = "meetings")]
pub(crate) fn cursor_monitor_logical_rect(app_handle: &AppHandle) -> Option<(f64, f64, f64, f64)> {
    let monitor = get_monitor_with_cursor(app_handle)?;
    let scale = monitor.scale_factor();
    Some((
        monitor.position().x as f64 / scale,
        monitor.position().y as f64 / scale,
        monitor.size().width as f64 / scale,
        monitor.size().height as f64 / scale,
    ))
}

fn is_mouse_within_monitor(
    mouse_pos: (i32, i32),
    monitor_pos: &PhysicalPosition<i32>,
    monitor_size: &PhysicalSize<u32>,
) -> bool {
    let (mouse_x, mouse_y) = mouse_pos;
    let PhysicalPosition {
        x: monitor_x,
        y: monitor_y,
    } = *monitor_pos;
    let PhysicalSize {
        width: monitor_width,
        height: monitor_height,
    } = *monitor_size;

    mouse_x >= monitor_x
        && mouse_x < (monitor_x + monitor_width as i32)
        && mouse_y >= monitor_y
        && mouse_y < (monitor_y + monitor_height as i32)
}

/// Returns overlay position in logical coordinates (points on macOS).
///
/// Uses monitor position/size directly rather than work_area(), which can
/// return incorrect coordinates on macOS for monitors with negative positions.
/// The per-platform OVERLAY_TOP_OFFSET / OVERLAY_BOTTOM_OFFSET constants
/// already account for system chrome (menu bar, taskbar).
///
/// We must use LogicalPosition (not PhysicalPosition) because Tauri/tao
/// converts PhysicalPosition using the scale factor of the monitor the window
/// is *currently* on, which is wrong when moving cross-monitor.
/// Computes the top offset for the recording pill (`D-E6`):
/// on notched macOS displays (`safe_area_top > 0.0`), sit 6 pt below the safe area;
/// otherwise fall back to `OVERLAY_TOP_OFFSET` (46 pt on non-notched Macs).
pub(crate) fn top_offset_for_safe_area(safe_area_top: f64) -> f64 {
    if safe_area_top > 0.0 {
        safe_area_top + 6.0
    } else {
        OVERLAY_TOP_OFFSET
    }
}

/// Pure coordinate math for the recording overlay pill with an explicit safe-area
/// top inset (in logical points).
pub(crate) fn compute_overlay_xy_with_safe_area(
    monitor_x: f64,
    monitor_y: f64,
    monitor_width: f64,
    monitor_height: f64,
    width: f64,
    height: f64,
    position: OverlayPosition,
    safe_area_top: f64,
) -> (f64, f64) {
    let x = monitor_x + (monitor_width - width) / 2.0;
    let y = match position {
        OverlayPosition::Top => monitor_y + top_offset_for_safe_area(safe_area_top),
        OverlayPosition::Bottom => monitor_y + monitor_height - height - OVERLAY_BOTTOM_OFFSET,
    };
    (x, y)
}

/// Pure coordinate math for the recording overlay pill so Top vs Bottom
/// placement can be tested without a window server.
pub(crate) fn compute_overlay_xy(
    monitor_x: f64,
    monitor_y: f64,
    monitor_width: f64,
    monitor_height: f64,
    width: f64,
    height: f64,
    position: OverlayPosition,
) -> (f64, f64) {
    compute_overlay_xy_with_safe_area(
        monitor_x,
        monitor_y,
        monitor_width,
        monitor_height,
        width,
        height,
        position,
        0.0,
    )
}

fn calculate_overlay_position(
    app_handle: &AppHandle,
    width: f64,
    height: f64,
) -> Option<(f64, f64)> {
    let monitor = get_monitor_with_cursor(app_handle)?;
    let scale = monitor.scale_factor();
    let monitor_x = monitor.position().x as f64 / scale;
    let monitor_y = monitor.position().y as f64 / scale;
    let monitor_width = monitor.size().width as f64 / scale;
    let monitor_height = monitor.size().height as f64 / scale;
    #[cfg(target_os = "macos")]
    let safe_area_top = {
        let inset = (monitor.work_area().position.y - monitor.position().y) as f64 / scale;
        if inset > 32.0 && inset < 120.0 {
            inset
        } else {
            0.0
        }
    };
    #[cfg(not(target_os = "macos"))]
    let safe_area_top = 0.0;

    let settings = settings::get_settings(app_handle);

    Some(compute_overlay_xy_with_safe_area(
        monitor_x,
        monitor_y,
        monitor_width,
        monitor_height,
        width,
        height,
        settings.overlay_position,
        safe_area_top,
    ))
}

/// Current overlay window size in logical units (points), for repositioning
/// without assuming a fixed size (compact vs. streaming).
fn current_overlay_logical_size(window: &tauri::webview::WebviewWindow) -> Option<(f64, f64)> {
    let size = window.inner_size().ok()?;
    let scale = window.scale_factor().ok()?;
    Some((size.width as f64 / scale, size.height as f64 / scale))
}

/// Creates the recording overlay window and keeps it hidden by default
#[cfg(not(target_os = "macos"))]
pub fn create_recording_overlay(app_handle: &AppHandle) {
    // On Linux (Wayland), monitor detection often fails, but we don't need exact coordinates
    // for Layer Shell as we use anchors. On other platforms, we require a monitor.
    #[cfg(not(target_os = "linux"))]
    {
        let position = calculate_overlay_position(app_handle, OVERLAY_WIDTH, OVERLAY_HEIGHT);
        if position.is_none() {
            debug!("Failed to determine overlay position, not creating overlay window");
            return;
        }
    }

    // Position starts unset — update_overlay_position() sets the correct
    // LogicalPosition before the overlay is shown.
    let mut builder = WebviewWindowBuilder::new(
        app_handle,
        "recording_overlay",
        tauri::WebviewUrl::App("src/overlay/index.html".into()),
    )
    .title("Recording")
    .resizable(false)
    .inner_size(OVERLAY_WIDTH, OVERLAY_HEIGHT)
    .shadow(false)
    .maximizable(false)
    .minimizable(false)
    .closable(false)
    .accept_first_mouse(true)
    .decorations(false)
    .always_on_top(true)
    .skip_taskbar(true)
    .transparent(true)
    .focusable(false)
    .focused(false)
    .visible(false);

    if let Some(data_dir) = crate::portable::data_dir() {
        builder = builder.data_directory(data_dir.join("webview"));
    }

    #[allow(unused_variables)]
    match builder.build() {
        Ok(window) => {
            #[cfg(target_os = "linux")]
            {
                // Try to initialize GTK layer shell, ignore errors if compositor doesn't support it
                if init_gtk_layer_shell(&window) {
                    debug!("GTK layer shell initialized for overlay window");
                } else {
                    debug!("GTK layer shell not available, falling back to regular window");
                }
            }

            debug!("Recording overlay window created successfully (hidden)");
        }
        Err(e) => {
            debug!("Failed to create recording overlay window: {}", e);
        }
    }
}

/// Creates the recording overlay panel and keeps it hidden by default (macOS)
#[cfg(target_os = "macos")]
pub fn create_recording_overlay(app_handle: &AppHandle) {
    if let Some((x, y)) = calculate_overlay_position(app_handle, OVERLAY_WIDTH, OVERLAY_HEIGHT) {
        // PanelBuilder creates a Tauri window then converts it to NSPanel.
        // The window remains registered, so get_webview_window() still works.
        match PanelBuilder::<_, RecordingOverlayPanel>::new(app_handle, "recording_overlay")
            .url(WebviewUrl::App("src/overlay/index.html".into()))
            .title("Recording")
            .position(tauri::Position::Logical(tauri::LogicalPosition { x, y }))
            .level(PanelLevel::Status)
            .size(tauri::Size::Logical(tauri::LogicalSize {
                width: OVERLAY_WIDTH,
                height: OVERLAY_HEIGHT,
            }))
            .has_shadow(false)
            .transparent(true)
            .no_activate(true)
            .corner_radius(0.0)
            .style_mask(StyleMask::empty().borderless().nonactivating_panel())
            .with_window(|w| w.decorations(false).transparent(true).focusable(false))
            .collection_behavior(
                CollectionBehavior::new()
                    .can_join_all_spaces()
                    .full_screen_auxiliary(),
            )
            .build()
        {
            Ok(panel) => {
                panel.hide();
            }
            Err(e) => {
                log::error!("Failed to create recording overlay panel: {}", e);
            }
        }
    }
}

fn show_overlay_state(app_handle: &AppHandle, state: &str) {
    // Whether the overlay shows at all is governed by overlay_style; position
    // only chooses Top vs Bottom placement. The meeting pill is the one state
    // that survives OverlayStyle::None: turning the dictation pill off must not
    // take away the only visible Stop.
    let settings = settings::get_settings(app_handle);
    #[cfg(feature = "meetings")]
    let style_allows = overlay_show_allowed(state, settings.overlay_style);
    #[cfg(not(feature = "meetings"))]
    let style_allows = settings.overlay_style != OverlayStyle::None;
    if !style_allows {
        return;
    }

    // Size the overlay for this state (compact vs. streaming), then position it.
    let (width, height) = overlay_dimensions(state);
    if let Some(overlay_window) = app_handle.get_webview_window("recording_overlay") {
        #[cfg(target_os = "linux")]
        update_gtk_layer_shell_anchors(&overlay_window);

        let size_started = std::time::Instant::now();
        let _ = overlay_window.set_size(tauri::Size::Logical(tauri::LogicalSize { width, height }));
        let size_elapsed = size_started.elapsed();

        let pos_started = std::time::Instant::now();
        let mut set_pos_elapsed = std::time::Duration::ZERO;
        if let Some((x, y)) = calculate_overlay_position(app_handle, width, height) {
            let set_pos_started = std::time::Instant::now();
            let _ = overlay_window
                .set_position(tauri::Position::Logical(tauri::LogicalPosition { x, y }));
            set_pos_elapsed = set_pos_started.elapsed();
        }
        let pos_calc_elapsed = pos_started.elapsed() - set_pos_elapsed;

        bump_generation(&OVERLAY_SHOW_GENERATION);

        let show_started = std::time::Instant::now();
        let _ = overlay_window.show();
        let show_elapsed = show_started.elapsed();

        // On Windows, aggressively re-assert "topmost" in the native Z-order after showing
        #[cfg(target_os = "windows")]
        force_overlay_topmost(&overlay_window);

        let _ = overlay_window.emit("show-overlay", state);
        log::debug!(
            "overlay '{}': set_size={:?} pos_calc={:?} set_pos={:?} show={:?}",
            state,
            size_elapsed,
            pos_calc_elapsed,
            set_pos_elapsed,
            show_elapsed
        );
    }
}

/// Shows the recording overlay window with fade-in animation
pub fn show_recording_overlay(app_handle: &AppHandle) {
    show_overlay_state(app_handle, "recording");
}

/// Shows the meeting pill: the same 256 x 46 window, a mark, Recording, the
/// elapsed time and Stop. It ignores `overlay_style = none`.
#[cfg(feature = "meetings")]
pub fn show_meeting_overlay(app_handle: &AppHandle) {
    show_overlay_state(app_handle, crate::meeting_controller::PILL_OVERLAY_STATE);
}

/// Whether `overlay_style` lets this state show. `none` hides the dictation
/// pill only; the meeting pill always shows.
#[cfg(feature = "meetings")]
fn overlay_show_allowed(state: &str, style: OverlayStyle) -> bool {
    state == crate::meeting_controller::PILL_OVERLAY_STATE || style != OverlayStyle::None
}

/// Whether a hide request must be dropped. Only the meeting's own Stop ends the
/// recording state; a dictation finishing, or the tray returning to Idle, must
/// not take the pill away.
#[cfg(feature = "meetings")]
fn overlay_hide_blocked(state: crate::meeting_controller::MeetingState) -> bool {
    state.pill_visible()
}

/// Shows the larger streaming overlay that displays live transcription text
pub fn show_streaming_overlay(app_handle: &AppHandle) {
    show_overlay_state(app_handle, "streaming");
}

/// Shows the transcribing overlay window
pub fn show_transcribing_overlay(app_handle: &AppHandle) {
    show_overlay_state(app_handle, "transcribing");
}

/// Shows the processing overlay window
pub fn show_processing_overlay(app_handle: &AppHandle) {
    show_overlay_state(app_handle, "processing");
}

/// Updates the overlay window position based on current settings
pub fn update_overlay_position(app_handle: &AppHandle) {
    if let Some(overlay_window) = app_handle.get_webview_window("recording_overlay") {
        #[cfg(target_os = "linux")]
        {
            update_gtk_layer_shell_anchors(&overlay_window);
        }

        // Use the window's current size so centering stays correct whether the
        // overlay is in compact or streaming layout.
        let (width, height) = current_overlay_logical_size(&overlay_window)
            .unwrap_or((OVERLAY_WIDTH, OVERLAY_HEIGHT));
        if let Some((x, y)) = calculate_overlay_position(app_handle, width, height) {
            let _ = overlay_window
                .set_position(tauri::Position::Logical(tauri::LogicalPosition { x, y }));
        }
    }
}

/// Emit the fade, then hide the window. When `captured` is set, a show that
/// landed after that generation keeps the window. A meeting build re-checks
/// the meeting state under the pill gate immediately before the hide.
fn schedule_overlay_hide(overlay_window: tauri::webview::WebviewWindow, captured: Option<u64>) {
    let _ = overlay_window.emit("hide-overlay", ());
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(300));
        #[cfg(feature = "meetings")]
        {
            with_meeting_pill_gate(|| {
                if let Some(captured) = captured {
                    if current_generation(&OVERLAY_SHOW_GENERATION) != captured {
                        return;
                    }
                }
                if let Some(state) = crate::meeting_controller::state(overlay_window.app_handle()) {
                    if overlay_hide_blocked(state) {
                        return;
                    }
                }
                let _ = overlay_window.hide();
            });
        }
        #[cfg(not(feature = "meetings"))]
        {
            if let Some(captured) = captured {
                if current_generation(&OVERLAY_SHOW_GENERATION) != captured {
                    return;
                }
            }
            let _ = overlay_window.hide();
        }
    });
}

/// Hides the recording overlay window with fade-out animation.
///
/// In a meeting build the state read and the hide share the pill gate with
/// the start that shows the meeting pill, so this cannot hide a pill it did
/// not see.
pub fn hide_recording_overlay(app_handle: &AppHandle) {
    #[cfg(feature = "meetings")]
    {
        with_meeting_pill_gate(|| {
            if let Some(state) = crate::meeting_controller::state(app_handle) {
                if overlay_hide_blocked(state) {
                    // Keep the pill and its Stop up. Re-assert it too: a dictation
                    // take just drew its own row over the shared overlay window.
                    show_overlay_state(app_handle, crate::meeting_controller::PILL_OVERLAY_STATE);
                    return;
                }
            }
            if let Some(overlay_window) = app_handle.get_webview_window("recording_overlay") {
                schedule_overlay_hide(overlay_window, None);
            }
        });
        return;
    }

    #[cfg(not(feature = "meetings"))]
    if let Some(overlay_window) = app_handle.get_webview_window("recording_overlay") {
        schedule_overlay_hide(overlay_window, None);
    }
}

/// Like [`hide_recording_overlay`], but does nothing if an overlay was shown after
/// `captured` was taken. The check runs again right before the window is hidden, so a
/// take that starts during the fade-out keeps its overlay. Returns whether it hid.
/// A meeting start holds the same gate as this read and the hide, so it cannot
/// lose its pill to a hide that sampled an earlier state.
pub fn hide_recording_overlay_if_current(app_handle: &AppHandle, captured: u64) -> bool {
    #[cfg(feature = "meetings")]
    {
        return with_meeting_pill_gate(|| {
            if let Some(state) = crate::meeting_controller::state(app_handle) {
                if overlay_hide_blocked(state) {
                    show_overlay_state(app_handle, crate::meeting_controller::PILL_OVERLAY_STATE);
                    return false;
                }
            }
            hide_if_generation_current(app_handle, captured)
        });
    }
    #[cfg(not(feature = "meetings"))]
    hide_if_generation_current(app_handle, captured)
}

fn hide_if_generation_current(app_handle: &AppHandle, captured: u64) -> bool {
    if current_generation(&OVERLAY_SHOW_GENERATION) != captured {
        return false;
    }
    if let Some(overlay_window) = app_handle.get_webview_window("recording_overlay") {
        schedule_overlay_hide(overlay_window, Some(captured));
    }
    true
}

// Cached "overlay is enabled" flag, kept in sync with overlay_style. Avoids
// reading the Tauri store on every audio callback (~24 Hz during recording).
// Defaults to false so the audio path doesn't emit until lib.rs::setup
// populates the cache from initial settings.
static OVERLAY_ENABLED: AtomicBool = AtomicBool::new(false);

/// Update the cached overlay-enabled flag. Called from `lib.rs` at
/// startup after settings load, and from `change_overlay_style_setting`
/// whenever the user changes whether the overlay is shown.
pub fn update_overlay_enabled_cache(enabled: bool) {
    OVERLAY_ENABLED.store(enabled, Ordering::Relaxed);
}

pub fn emit_levels(app_handle: &AppHandle, levels: &[f32]) {
    // Skip emission when the overlay is disabled. The recording_overlay
    // window is created at boot regardless of overlay_style, so without this
    // guard a hidden overlay's WebKit subprocess still
    // processes every event. Each event drives some kind of WebKit
    // C++ allocation that accumulates without bound (mechanism not
    // directly characterized; see issue #1279 for the investigation).
    // For users with `overlay_style: none` (the Linux default) this skip
    // eliminates the upstream driver of that accumulation.
    if !OVERLAY_ENABLED.load(Ordering::Relaxed) {
        return;
    }

    // Throttle to ~30 FPS. Even with the overlay enabled, the raw audio
    // callback fires far faster than the UI needs; capping emission rate
    // cuts the per-frame `eval_script`/IPC volume that drives the wry
    // memory growth in issue #1279 (upstream tauri-apps/wry#1489).
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    let last = LAST_MIC_LEVEL_EMIT.load(Ordering::Relaxed);
    if now.saturating_sub(last) < EMIT_THROTTLE_MS {
        return;
    }
    LAST_MIC_LEVEL_EMIT.store(now, Ordering::Relaxed);

    // Target only the overlay window. In Tauri 2 both `AppHandle::emit`
    // and `WebviewWindow::emit` broadcast to all webviews; Tauri's
    // listener filter then skips webviews with no registered listener
    // for the event, so the settings webview never received `mic-level`.
    // But the previous dual-call pattern still produced two `eval_script`
    // calls to the overlay per audio callback (one from each .emit()).
    // `emit_to` with the overlay's window label produces a single
    // eval_script call per callback, cutting the per-callback WebKit
    // dispatch work in half.
    let _ = app_handle.emit_to("recording_overlay", "mic-level", levels);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_show_after_the_capture_makes_the_capture_stale() {
        let generation = Mutex::new(0);

        let captured = current_generation(&generation);
        assert_eq!(current_generation(&generation), captured);

        bump_generation(&generation);
        assert_ne!(current_generation(&generation), captured);

        // Only a capture taken after the last show is current.
        let captured = current_generation(&generation);
        bump_generation(&generation);
        bump_generation(&generation);
        assert_ne!(current_generation(&generation), captured);
    }

    #[cfg(feature = "meetings")]
    #[test]
    fn overlay_hide_keeps_meeting_pill() {
        use crate::meeting_controller::{MeetingState, PILL_OVERLAY_STATE};

        // Dictation finishing, or the tray going Idle, is a hide request. While
        // a meeting records that request is dropped, so Stop stays visible.
        assert!(overlay_hide_blocked(MeetingState::Recording));
        assert!(overlay_hide_blocked(MeetingState::Stopping));
        assert!(!overlay_hide_blocked(MeetingState::Idle));
        assert!(!overlay_hide_blocked(MeetingState::ConsentOpen));

        // The meeting's own Stop leaves Idle, and that hide goes through.
        assert!(!overlay_hide_blocked(MeetingState::Idle));

        // OverlayStyle::None hides only the dictation pill.
        assert!(!overlay_show_allowed("recording", OverlayStyle::None));
        assert!(!overlay_show_allowed("streaming", OverlayStyle::None));
        assert!(overlay_show_allowed(PILL_OVERLAY_STATE, OverlayStyle::None));
        assert!(overlay_show_allowed("recording", OverlayStyle::Minimal));
    }

    #[cfg(feature = "meetings")]
    #[test]
    fn hide_cannot_land_after_a_meeting_start() {
        use crate::meeting_controller::MeetingState;

        struct Pill {
            state: MeetingState,
            hidden: bool,
        }

        // Same gate the window hide and the meeting start use. No window.
        fn start(pill: &Mutex<Pill>) {
            with_meeting_pill_gate(|| {
                let mut pill = pill.lock().unwrap_or_else(|e| e.into_inner());
                pill.state = MeetingState::Recording;
                pill.hidden = false;
            });
        }

        fn hide(pill: &Mutex<Pill>) -> bool {
            with_meeting_pill_gate(|| {
                let mut pill = pill.lock().unwrap_or_else(|e| e.into_inner());
                if overlay_hide_blocked(pill.state) {
                    pill.hidden = false;
                    return false;
                }
                pill.hidden = true;
                true
            })
        }

        // Start, then hide. The hide reads Recording under the gate.
        let pill = Mutex::new(Pill {
            state: MeetingState::Idle,
            hidden: false,
        });
        start(&pill);
        assert!(!hide(&pill));
        {
            let pill = pill.lock().unwrap_or_else(|e| e.into_inner());
            assert!(pill.state.pill_visible());
            assert!(!pill.hidden);
        }

        // Hide sampled Idle, then a start commits before the hide acts.
        // The act re-reads under the gate, so the stale Idle does not land.
        let pill = Mutex::new(Pill {
            state: MeetingState::Idle,
            hidden: false,
        });
        let sampled = pill.lock().unwrap_or_else(|e| e.into_inner()).state;
        assert!(!overlay_hide_blocked(sampled));
        start(&pill);
        assert!(!hide(&pill));
        assert!(!pill.lock().unwrap_or_else(|e| e.into_inner()).hidden);

        // A hide that finishes before the start may hide. The start shows the pill.
        let pill = Mutex::new(Pill {
            state: MeetingState::Idle,
            hidden: false,
        });
        assert!(hide(&pill));
        assert!(pill.lock().unwrap_or_else(|e| e.into_inner()).hidden);
        start(&pill);
        {
            let pill = pill.lock().unwrap_or_else(|e| e.into_inner());
            assert_eq!(pill.state, MeetingState::Recording);
            assert!(!pill.hidden);
        }

        // Stopping is the same guard: the hide does not land.
        {
            let mut pill = pill.lock().unwrap_or_else(|e| e.into_inner());
            pill.state = MeetingState::Stopping;
        }
        assert!(!hide(&pill));
        assert!(overlay_hide_blocked(MeetingState::Stopping));
    }

    #[test]
    fn overlay_position_top_places_under_camera_and_bottom_near_bottom_edge() {
        let (top_x, top_y) = compute_overlay_xy(
            0.0,
            0.0,
            1440.0,
            900.0,
            OVERLAY_WIDTH,
            OVERLAY_HEIGHT,
            OverlayPosition::Top,
        );
        let (bottom_x, bottom_y) = compute_overlay_xy(
            0.0,
            0.0,
            1440.0,
            900.0,
            OVERLAY_WIDTH,
            OVERLAY_HEIGHT,
            OverlayPosition::Bottom,
        );
        assert_eq!(top_x, bottom_x);
        assert_eq!(top_y, OVERLAY_TOP_OFFSET);
        assert_eq!(bottom_y, 900.0 - OVERLAY_HEIGHT - OVERLAY_BOTTOM_OFFSET);
        assert!(top_y < bottom_y);
    }

    #[test]
    fn gtk_layer_shell_flag_prefers_primary_and_falls_back_to_legacy() {
        assert!(!gtk_layer_shell_disabled_from(None, None));
        assert!(gtk_layer_shell_disabled_from(Some("1"), None));
        assert!(gtk_layer_shell_disabled_from(None, Some("1")));
        assert!(!gtk_layer_shell_disabled_from(Some("0"), Some("1")));
        assert!(gtk_layer_shell_disabled_from(Some("true"), Some("0")));
        assert!(!gtk_layer_shell_disabled_from(None, Some("false")));
    }
}
