//! macOS-only popover behaviour for the tray icon.
//!
//! Left-click on the tray toggles the native `NSPopover` that `lib.rs` set up
//! (via `tauri-plugin-nspopover`) to host the `main` webview. The popover
//! anchors itself under the status item and gives the native pop-down
//! animation and arrow; positioning is no longer computed here. Right-click
//! falls through to the tray menu. Linux never reaches this code —
//! `StatusNotifierItem` delivers no click events at all, which is why the
//! menu is the primary surface there.

use tauri::Runtime;
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconEvent};
use tauri_plugin_nspopover::AppExt as _;

/// Tray click handler: left-click (on release) toggles the popover.
///
/// `event` is by value because that is the `on_tray_icon_event` callback
/// signature, not a choice this function makes.
#[allow(clippy::needless_pass_by_value)]
pub fn handle_tray_event<R: Runtime>(tray: &TrayIcon<R>, event: TrayIconEvent) {
    if let TrayIconEvent::Click {
        button: MouseButton::Left,
        button_state: MouseButtonState::Up,
        ..
    } = event
    {
        toggle_popover(tray.app_handle());
    }
}

/// Show the popover if hidden, hide it if shown. The plugin owns the shown
/// state and self-anchors to the status item, so there is nothing to position
/// or focus here. Both calls are no-ops until `to_popover` has run.
fn toggle_popover<R: Runtime>(app: &tauri::AppHandle<R>) {
    if app.is_popover_shown() {
        app.hide_popover();
    } else {
        app.show_popover();
    }
}
