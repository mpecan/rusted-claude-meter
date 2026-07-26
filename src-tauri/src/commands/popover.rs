//! Bind the main window's height to its rendered content.
//!
//! The frontend measures the height of the popover panel (`.popover`) and calls
//! [`set_popover_height`] so the container hugs the content instead of showing a
//! fixed frame with dead space below short views. Split into its own module
//! because its macOS body is native (`AppKit`).
//!
//! The two platforms honour it differently, and deliberately. On macOS the
//! window lives in an `NSPopover` that is *only* ever content-sized, so every
//! measurement applies. On Linux it is an ordinary window the user can resize,
//! so only the first measurement after each show applies — enough to fit the
//! content on open, without the next poll yanking a window the user has just
//! dragged to a size they wanted. See [`ContentFit`].

/// Whether the next measured height should resize the window.
///
/// Managed state on Linux only. `tray::show_main_window` stores `true`
/// immediately before showing the window, and the first [`set_popover_height`]
/// that follows swaps it back — so exactly one measurement per show resizes.
/// Without that gate a resizable window would fight its user: the frontend
/// re-measures on every state broadcast, so any manual resize would be undone
/// within a minute.
///
/// A bare field rather than `arm`/`take` methods: there are exactly two call
/// sites, and at that size the atomic ordering is better read at each of them
/// than hidden behind a name.
#[cfg(target_os = "linux")]
#[derive(Debug, Default)]
pub struct ContentFit(pub std::sync::atomic::AtomicBool);

/// Resize the window to `height` (logical points); width is left alone.
///
/// The frontend calls this blindly on every layout change, with no platform
/// check — what varies is how much of it each platform honours (see the module
/// docs).
#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
pub fn set_popover_height(app: tauri::AppHandle, height: f64) {
    // Clamped once here rather than in each platform body, so the bounds and
    // the non-finite guard are stated once and neither platform can drift.
    let Some(height) = clamped_height(height) else {
        return;
    };
    #[cfg(target_os = "macos")]
    resize_popover(&app, height);
    #[cfg(target_os = "linux")]
    fit_window(&app, height);
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    let _ = (&app, height);
}

/// Floor: roughly the header plus one usage row, so a near-empty view never
/// collapses.
const MIN_POPOVER_HEIGHT: f64 = 170.0;
/// Ceiling so the window can't outgrow a typical screen; taller content
/// scrolls inside `#popover-view` via its `overflow-y: auto`.
const MAX_POPOVER_HEIGHT: f64 = 900.0;

/// Clamp a measured height into the usable range, rejecting the non-finite
/// values a mid-layout measurement can produce.
fn clamped_height(height: f64) -> Option<f64> {
    height
        .is_finite()
        .then(|| height.clamp(MIN_POPOVER_HEIGHT, MAX_POPOVER_HEIGHT))
}

/// Fixed popover width; matches the window frame the macOS plugin seeds from.
#[cfg(target_os = "macos")]
const POPOVER_WIDTH: f64 = 420.0;

/// Apply a content-fitted height to the live `NSPopover`. Runs on the main
/// thread as `AppKit` requires.
#[cfg(target_os = "macos")]
fn resize_popover(app: &tauri::AppHandle, height: f64) {
    use tauri_plugin_nspopover::AppExt as _;

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

/// Fit the main window to its content, once per show. Keeps the current width
/// — only the height is content-driven, and the user owns both thereafter.
#[cfg(target_os = "linux")]
fn fit_window(app: &tauri::AppHandle, height: f64) {
    use tauri::Manager as _;

    // No managed `ContentFit` means the window was never shown through
    // `show_main_window` (nothing to fit), so this is a no-op rather than a
    // surprise resize.
    if !app
        .try_state::<ContentFit>()
        .is_some_and(|fit| fit.0.swap(false, std::sync::atomic::Ordering::Relaxed))
    {
        return;
    }
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let Ok(size) = window.inner_size() else {
        return;
    };
    let Ok(scale) = window.scale_factor() else {
        return;
    };
    let _ = window.set_size(tauri::LogicalSize::new(
        f64::from(size.width) / scale,
        height,
    ));
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn clamped_height_rejects_non_finite_measurements() {
        assert_eq!(clamped_height(f64::NAN), None);
        assert_eq!(clamped_height(f64::INFINITY), None);
    }

    #[test]
    fn clamped_height_holds_the_floor_and_ceiling() {
        assert_eq!(clamped_height(10.0), Some(MIN_POPOVER_HEIGHT));
        assert_eq!(clamped_height(10_000.0), Some(MAX_POPOVER_HEIGHT));
        assert_eq!(clamped_height(400.0), Some(400.0));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn content_fit_lets_exactly_one_measurement_through_per_show() {
        use std::sync::atomic::Ordering::Relaxed;
        let fit = ContentFit::default();
        let take = || fit.0.swap(false, Relaxed);
        assert!(!take(), "unarmed by default: never resize unasked");
        fit.0.store(true, Relaxed);
        assert!(take(), "the measurement right after a show applies");
        assert!(!take(), "later ones must not fight a manual resize");
        fit.0.store(true, Relaxed);
        assert!(take(), "the next show re-arms it");
    }
}
