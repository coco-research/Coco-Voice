//! Floating non-activating desktop panel for the meeting call offer (`Frame 1`).
//!
//! Mirrors `recording_overlay` (`src-tauri/src/overlay.rs`) so the call offer
//! surfaces at the top-right of the active monitor without stealing focus or
//! requiring the main Coco Voice window to be visible.

use tauri::{AppHandle, Manager};

#[cfg(not(target_os = "macos"))]
use tauri::WebviewWindowBuilder;

#[cfg(target_os = "macos")]
use tauri::WebviewUrl;

#[cfg(target_os = "macos")]
use tauri_nspanel::{tauri_panel, CollectionBehavior, PanelBuilder, PanelLevel, StyleMask};

pub const MEETING_OFFER_WINDOW_LABEL: &str = "meeting_offer";
pub const OFFER_WINDOW_WIDTH: f64 = 388.0;
pub const OFFER_WINDOW_HEIGHT: f64 = 276.0;
const OFFER_RIGHT_MARGIN: f64 = 24.0;
const OFFER_TOP_OFFSET: f64 = 96.0;

#[cfg(target_os = "macos")]
tauri_panel! {
    panel!(MeetingOfferPanel {
        config: {
            can_become_key_window: false,
            is_floating_panel: true
        }
    })
}

/// Computes top-right logical coordinates `(x, y)` on a monitor so the 340px
/// card (plus shadow margin) sits beside the call window without clipping.
pub fn offer_panel_position(
    monitor_x: f64,
    monitor_y: f64,
    monitor_width: f64,
    window_width: f64,
) -> (f64, f64) {
    let x = monitor_x + (monitor_width - window_width - OFFER_RIGHT_MARGIN).max(16.0);
    let y = monitor_y + OFFER_TOP_OFFSET;
    (x, y)
}

fn calculate_offer_position(app_handle: &AppHandle) -> Option<(f64, f64)> {
    let (monitor_x, monitor_y, monitor_width, _monitor_height) =
        crate::overlay::cursor_monitor_logical_rect(app_handle)?;
    Some(offer_panel_position(
        monitor_x,
        monitor_y,
        monitor_width,
        OFFER_WINDOW_WIDTH,
    ))
}

#[cfg(target_os = "macos")]
pub fn create_meeting_offer_panel(app_handle: &AppHandle) {
    if app_handle
        .get_webview_window(MEETING_OFFER_WINDOW_LABEL)
        .is_some()
    {
        return;
    }
    let (x, y) = calculate_offer_position(app_handle).unwrap_or((980.0, OFFER_TOP_OFFSET));
    match PanelBuilder::<_, MeetingOfferPanel>::new(app_handle, MEETING_OFFER_WINDOW_LABEL)
        .url(WebviewUrl::App("src/offer/index.html".into()))
        .title("Coco Voice")
        .position(tauri::Position::Logical(tauri::LogicalPosition { x, y }))
        .level(PanelLevel::Status)
        .size(tauri::Size::Logical(tauri::LogicalSize {
            width: OFFER_WINDOW_WIDTH,
            height: OFFER_WINDOW_HEIGHT,
        }))
        .has_shadow(false)
        .transparent(true)
        .no_activate(true)
        .corner_radius(0.0)
        .style_mask(StyleMask::empty().borderless().nonactivating_panel())
        .with_window(|w| {
            w.decorations(false)
                .transparent(true)
                .focusable(false)
                .accept_first_mouse(true)
        })
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
        Err(err) => {
            log::error!("Failed to create meeting offer panel: {}", err);
        }
    }
}

#[cfg(not(target_os = "macos"))]
pub fn create_meeting_offer_panel(app_handle: &AppHandle) {
    if app_handle
        .get_webview_window(MEETING_OFFER_WINDOW_LABEL)
        .is_some()
    {
        return;
    }
    let mut builder = WebviewWindowBuilder::new(
        app_handle,
        MEETING_OFFER_WINDOW_LABEL,
        tauri::WebviewUrl::App("src/offer/index.html".into()),
    )
    .title("Coco Voice")
    .resizable(false)
    .inner_size(OFFER_WINDOW_WIDTH, OFFER_WINDOW_HEIGHT)
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

    if let Err(err) = builder.build() {
        log::debug!("Failed to create meeting offer window: {}", err);
    }
}

pub fn hide_meeting_offer(app_handle: &AppHandle) {
    if let Some(window) = app_handle.get_webview_window(MEETING_OFFER_WINDOW_LABEL) {
        let _ = window.hide();
    }
}

#[tauri::command]
#[specta::specta]
pub fn set_meeting_offer_visible(app: AppHandle, visible: bool) -> Result<(), String> {
    if !visible {
        hide_meeting_offer(&app);
        return Ok(());
    }
    if app.get_webview_window(MEETING_OFFER_WINDOW_LABEL).is_none() {
        create_meeting_offer_panel(&app);
    }
    if let Some(window) = app.get_webview_window(MEETING_OFFER_WINDOW_LABEL) {
        if let Some((x, y)) = calculate_offer_position(&app) {
            let _ = window.set_position(tauri::Position::Logical(tauri::LogicalPosition { x, y }));
        }
        window.show().map_err(|err| err.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{offer_panel_position, OFFER_WINDOW_WIDTH};

    #[test]
    fn positions_offer_panel_at_top_right_of_active_monitor() {
        let (x, y) = offer_panel_position(0.0, 0.0, 1440.0, OFFER_WINDOW_WIDTH);
        assert_eq!(x, 1440.0 - OFFER_WINDOW_WIDTH - 24.0);
        assert_eq!(y, 96.0);

        let (secondary_x, secondary_y) =
            offer_panel_position(-1920.0, 40.0, 1920.0, OFFER_WINDOW_WIDTH);
        assert_eq!(secondary_x, -1920.0 + (1920.0 - OFFER_WINDOW_WIDTH - 24.0));
        assert_eq!(secondary_y, 136.0);
    }
}
