//! macOS-only popover behaviour for the tray icon.
//!
//! Left-click on the tray toggles the main window as a popover: positioned
//! just below the click's tray-icon rect (centred, clamped to the screen),
//! frameless and always on top (configured at startup in `lib.rs`), hidden
//! again on focus loss. Right-click falls through to the tray menu. Linux
//! never reaches this code — `StatusNotifierItem` delivers no click events
//! at all, which is why the menu is the primary surface there.

use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconEvent};
use tauri::{Manager, PhysicalPosition, Runtime};

use super::geometry::{ScreenBounds, TrayRect, popover_origin};

/// Tray click handler: left-click (on release) toggles the popover window.
///
/// `event` is by value because that is the `on_tray_icon_event` callback
/// signature, not a choice this function makes.
#[allow(clippy::needless_pass_by_value)]
pub fn handle_tray_event<R: Runtime>(tray: &TrayIcon<R>, event: TrayIconEvent) {
    if let TrayIconEvent::Click {
        button: MouseButton::Left,
        button_state: MouseButtonState::Up,
        rect,
        ..
    } = event
    {
        toggle_popover(tray.app_handle(), rect);
    }
}

fn toggle_popover<R: Runtime>(app: &tauri::AppHandle<R>, rect: tauri::Rect) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    if window.is_visible().unwrap_or(false) {
        let _ = window.hide();
        return;
    }

    let tray = to_physical_rect(rect, window.scale_factor().unwrap_or(1.0));
    // The monitor under the click, so multi-display setups anchor to the
    // menu bar that was clicked. Position is still best-effort when the
    // monitor cannot be resolved.
    let screen = app
        .monitor_from_point(tray.x, tray.y)
        .ok()
        .flatten()
        .map(|monitor| ScreenBounds {
            x: f64::from(monitor.position().x),
            width: f64::from(monitor.size().width),
        });
    let width = window
        .outer_size()
        .map_or(0.0, |size| f64::from(size.width));
    let (x, y) = popover_origin(tray, width, screen);
    let _ = window.set_position(PhysicalPosition::new(x, y));
    let _ = window.show();
    let _ = window.set_focus();
}

/// Flatten Tauri's logical-or-physical rect into physical pixels.
fn to_physical_rect(rect: tauri::Rect, scale: f64) -> TrayRect {
    let position = rect.position.to_physical::<f64>(scale);
    let size = rect.size.to_physical::<f64>(scale);
    TrayRect {
        x: position.x,
        y: position.y,
        width: size.width,
        height: size.height,
    }
}
