//! Bind the popover container's height to its rendered content.
//!
//! The frontend measures the height of the popover panel (`.popover`) and calls
//! [`set_popover_height`] so the container hugs the content instead of showing a
//! fixed 420-tall frame with dead space below short views. Split into its own
//! module because it's the one command whose body is macOS-native (`AppKit`).

/// Resize the popover to `height` (logical points); width is fixed.
///
/// macOS: the window is hosted in an `NSPopover` whose `contentSize` the
/// nspopover plugin pins once at startup — this updates it live, and `NSPopover`
/// animates the change. On Linux the main window is a normal window, so this is
/// a no-op; the value is still accepted so the frontend can call it blindly.
#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
pub fn set_popover_height(app: tauri::AppHandle, height: f64) {
    #[cfg(target_os = "macos")]
    resize_popover(&app, height);
    #[cfg(not(target_os = "macos"))]
    let _ = (&app, height);
}

/// Apply a content-fitted height to the live `NSPopover`. Clamped to a floor
/// (about the header plus one usage row, so a near-empty view never collapses)
/// and a ceiling (taller content scrolls inside `#popover-view` via its
/// `overflow-y: auto`). Runs on the main thread as `AppKit` requires.
#[cfg(target_os = "macos")]
fn resize_popover(app: &tauri::AppHandle, height: f64) {
    use tauri_plugin_nspopover::AppExt as _;

    /// Fixed popover width; matches the window frame the plugin seeds from.
    const POPOVER_WIDTH: f64 = 420.0;
    /// Floor: roughly the header plus one usage row.
    const MIN_POPOVER_HEIGHT: f64 = 170.0;
    /// Ceiling so the popover can't outgrow a typical screen.
    const MAX_POPOVER_HEIGHT: f64 = 900.0;

    if !height.is_finite() {
        return;
    }
    let height = height.clamp(MIN_POPOVER_HEIGHT, MAX_POPOVER_HEIGHT);
    let app = app.clone();
    let _ = app.clone().run_on_main_thread(move || {
        use objc2_foundation::NSSize;
        // The popover is created once during `configure_popover_window` at
        // startup, so it always exists by the time the webview measures itself.
        let popover = app.ns_popover();
        popover.setContentSize(NSSize {
            width: POPOVER_WIDTH,
            height,
        });
    });
}
