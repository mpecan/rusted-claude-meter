//! The one place the tray's platform differences live.
//!
//! Everything above this seam is shared and stays in [`super`]: which icon to
//! render, which menu lines to show, and the [`super::model::TrayDiff`] gate
//! that decides whether anything changed at all. Only *pushing* the result at
//! the desktop differs, so the trait is deliberately two methods wide.
//!
//! Both pushes return whether they actually took. The caller commits the
//! debounce gate only on `true`, so a failed push is retried on the next state
//! instead of being silently swallowed — the behaviour the pre-split
//! `apply_state` had inline.

use meter_render::RenderedIcon;
use tauri::{AppHandle, Runtime};

use super::model::MenuModel;

// Linux speaks StatusNotifierItem directly; everywhere else Tauri's own tray
// is the right tool. The two are mutually exclusive on purpose — building both
// on Linux would put two icons in the panel.
#[cfg(target_os = "linux")]
mod icon_file;
#[cfg(target_os = "linux")]
mod ksni_tray;
#[cfg(not(target_os = "linux"))]
mod tauri_tray;

#[cfg(target_os = "linux")]
pub(super) use ksni_tray::KsniTray as PlatformTray;
#[cfg(not(target_os = "linux"))]
pub(super) use tauri_tray::TauriTray as PlatformTray;

/// One platform's tray. Implementors own only the native handles; all state
/// that decides *what* to show lives in [`super::TrayResources`].
pub(super) trait TrayBackend<R: Runtime>: Sized {
    /// Create the tray, showing `menu` and `icon`.
    ///
    /// `icon` is `None` when the initial render failed. That is not fatal —
    /// the platform substitutes whatever fallback it has and startup
    /// continues, because the next broadcast will retry the real gauge.
    fn build(
        app: &AppHandle<R>,
        icon: Option<&RenderedIcon>,
        menu: &MenuModel,
    ) -> tauri::Result<Self>;

    /// Replace the tray icon. Called only when the rendered content actually
    /// changed.
    fn set_icon(&mut self, app: &AppHandle<R>, icon: &RenderedIcon) -> bool;

    /// Replace the tray menu. Called only when a menu line actually changed.
    fn set_menu(&mut self, app: &AppHandle<R>, menu: &MenuModel) -> bool;
}
