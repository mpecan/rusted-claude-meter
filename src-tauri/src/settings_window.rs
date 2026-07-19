//! The dedicated Settings window — an ordinary, resizable top-level window,
//! not the in-popover overlay it used to be.
//!
//! Why a real window needs special handling on macOS: Rusted Claude Meter runs
//! under the `Accessory` activation policy so it lives in the menu bar with no
//! Dock icon (see `lib.rs::run`). A side effect of `Accessory` is that the app
//! is not allowed to activate itself, so a freshly built window opens *behind*
//! whatever app is frontmost — useless for Settings. So, mirroring the Swift
//! `ClaudeMeter` (whose Settings is a normal front-most window), opening
//! Settings temporarily promotes the app to `Regular`; closing it drops back to
//! `Accessory` (see `lib.rs::handle_window_event`), returning the app to
//! menu-bar-only.
//!
//! The window is created on demand and destroyed on close (rebuilt on the next
//! open), so every open starts from a clean render — no stale session status or
//! model list left over from a previous visit — and there is never a second
//! copy: an open request while it already exists just focuses it.

use tauri::{AppHandle, Manager, Runtime, WebviewUrl, WebviewWindowBuilder};

/// Label of the runtime-created Settings window. Distinct from the config's
/// `main` popover window so both coexist and are addressed independently.
pub const SETTINGS_WINDOW_LABEL: &str = "settings";

/// The window the popover lives in (defined in `tauri.conf.json`). Only the
/// macOS focus-loss handler (and the unit test) reference it; on Linux the
/// popover has no blur behaviour, so gate it to avoid a dead-code error there.
#[cfg(any(target_os = "macos", test))]
const MAIN_WINDOW_LABEL: &str = "main";

/// Open the Settings window, or focus it if it is already open — never a second
/// copy. Both the popover's "Settings" button (via the `open_settings_window`
/// command) and the tray's "Settings…" item route here. On macOS the app is
/// raised to a foreground app first so the window actually comes to the front.
pub fn open<R: Runtime>(app: &AppHandle<R>) {
    set_app_foreground(app, true);
    if let Some(window) = app.get_webview_window(SETTINGS_WINDOW_LABEL) {
        let _ = window.show();
        let _ = window.set_focus();
        return;
    }
    match WebviewWindowBuilder::new(
        app,
        SETTINGS_WINDOW_LABEL,
        WebviewUrl::App("index.html".into()),
    )
    .title("Settings")
    .inner_size(480.0, 720.0)
    .min_inner_size(420.0, 540.0)
    .resizable(true)
    .build()
    {
        Ok(window) => {
            let _ = window.set_focus();
        }
        // A build failure is logged, not fatal: the tray and popover keep
        // working, and the next open request retries.
        Err(error) => eprintln!("failed to open settings window: {error}"),
    }
}

/// macOS activation-policy dance for the Settings window. Set `foreground` when
/// opening Settings to promote the app to a `Regular` foreground app so the
/// window can become key and come to the front; clear it when Settings closes
/// to drop back to `Accessory` (menu-bar-only, no Dock icon). No-op elsewhere,
/// where activation policy is not a concept.
#[cfg(target_os = "macos")]
pub fn set_app_foreground<R: Runtime>(app: &AppHandle<R>, foreground: bool) {
    let policy = if foreground {
        tauri::ActivationPolicy::Regular
    } else {
        tauri::ActivationPolicy::Accessory
    };
    let _ = app.set_activation_policy(policy);
}

#[cfg(not(target_os = "macos"))]
pub const fn set_app_foreground<R: Runtime>(_app: &AppHandle<R>, _foreground: bool) {}

/// Whether `label` is the Settings window's label — the one window that is a
/// real, closable window rather than the always-alive popover.
pub fn is_settings_label(label: &str) -> bool {
    label == SETTINGS_WINDOW_LABEL
}

/// Whether `label` is the popover/`main` window's label. Only used by the
/// macOS focus-loss handler (popover auto-hide) and the unit test — gated so
/// Linux, which has no such handler, doesn't flag it as dead code.
#[cfg(any(target_os = "macos", test))]
pub fn is_main_label(label: &str) -> bool {
    label == MAIN_WINDOW_LABEL
}

/// Open (or focus) the Settings window — the popover's "Settings" button.
#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
pub fn open_settings_window(app: AppHandle) {
    open(&app);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn label_predicates_match_the_two_window_labels() {
        assert!(is_settings_label(SETTINGS_WINDOW_LABEL));
        assert!(!is_settings_label(MAIN_WINDOW_LABEL));
        assert!(is_main_label(MAIN_WINDOW_LABEL));
        assert!(!is_main_label(SETTINGS_WINDOW_LABEL));
    }
}
