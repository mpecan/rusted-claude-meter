//! The native tray: Tauri's `TrayIconBuilder`, on every platform.
//!
//! Split out of `tray/mod.rs` to keep that file under the size gate and to
//! separate *what* to show (there) from *how* to push it at the desktop (here).
//! The menu-building, the in-place `set_text` fast path and the
//! rebuild-on-line-count-change rule are unchanged from when they lived there.
//!
//! On macOS left-click is reserved for the `NSPopover` (see [`super::popover`])
//! and the menu answers right-click. On Linux `StatusNotifierItem` delivers no
//! click events through libappindicator, so the menu is the whole surface: it
//! carries the live usage lines, and "Open Rusted Claude Meter" opens the main
//! window. That is deliberate — see the module docs on `tray`.

use meter_render::RenderedIcon;
use tauri::image::Image;
use tauri::menu::{IsMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager, Runtime};

use super::model::MenuModel;
use super::show_main_window;
use crate::scheduler::SchedulerHandle;

const TRAY_ID: &str = "main";

/// Live handles into the built menu, so text can be updated in place without
/// rebuilding (which is what avoids flicker).
pub(super) struct NativeTray<R: Runtime> {
    status_item: MenuItem<R>,
    usage_items: Vec<MenuItem<R>>,
}

impl<R: Runtime> NativeTray<R> {
    pub(super) fn build(
        app: &AppHandle<R>,
        icon: Option<&RenderedIcon>,
        menu: &MenuModel,
    ) -> tauri::Result<Self> {
        let (built, status_item, usage_items) = build_menu(app, menu)?;

        let mut tray = TrayIconBuilder::with_id(TRAY_ID)
            .menu(&built)
            // macOS reserves left-click for the popover window; everywhere
            // else the menu is the primary surface.
            .show_menu_on_left_click(!cfg!(target_os = "macos"))
            .on_menu_event(|app, event| match event.id.as_ref() {
                "open" => show_main_window(app),
                "settings" => crate::settings_window::open(app),
                "refresh" => {
                    if let Some(scheduler) = app.try_state::<SchedulerHandle>() {
                        scheduler.request_refresh();
                    }
                }
                "quit" => app.exit(0),
                _ => {}
            });

        #[cfg(target_os = "macos")]
        {
            tray = tray.on_tray_icon_event(super::popover::handle_tray_event);
        }

        match icon {
            Some(rendered) => {
                tray = tray
                    .icon(tray_image(rendered))
                    .icon_as_template(rendered.is_template);
            }
            // A render failure falls back to the bundled app icon rather than
            // aborting startup.
            None => {
                if let Some(icon) = app.default_window_icon() {
                    tray = tray.icon(icon.clone());
                }
            }
        }

        tray.build(app)?;
        Ok(Self {
            status_item,
            usage_items,
        })
    }
}

/// Replace the tray icon. A free function because it needs no stored state —
/// the tray is looked up by id, exactly as `set_menu` does.
pub(super) fn set_icon<R: Runtime>(app: &AppHandle<R>, icon: &RenderedIcon) -> bool {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return false;
    };
    if tray.set_icon(Some(tray_image(icon))).is_err() {
        return false;
    }
    let _ = tray.set_icon_as_template(icon.is_template);
    true
}

impl<R: Runtime> NativeTray<R> {
    /// Update menu text in place when the line count is unchanged; rebuild the
    /// menu (never the tray icon) when usage lines appeared or vanished — the
    /// fast path can only update a `MenuItem` already built into the menu,
    /// never add or remove one. A window gaining or losing its pace detail line
    /// changes that count, so it takes the rebuild path.
    pub(super) fn set_menu(&mut self, app: &AppHandle<R>, menu: &MenuModel) -> bool {
        let Some(tray) = app.tray_by_id(TRAY_ID) else {
            return false;
        };
        if self.usage_items.len() == menu.usage_lines.len() {
            let mut applied = self.status_item.set_text(&menu.status_line).is_ok();
            for (item, line) in self.usage_items.iter().zip(&menu.usage_lines) {
                applied &= item.set_text(line).is_ok();
            }
            return applied;
        }
        match build_menu(app, menu) {
            Ok((rebuilt, status_item, usage_items)) => {
                if tray.set_menu(Some(rebuilt)).is_ok() {
                    self.status_item = status_item;
                    self.usage_items = usage_items;
                    true
                } else {
                    false
                }
            }
            Err(error) => {
                eprintln!("tray menu rebuild failed, keeping previous menu: {error}");
                false
            }
        }
    }
}

type BuiltMenu<R> = (Menu<R>, MenuItem<R>, Vec<MenuItem<R>>);

/// Build the full tray menu for a [`MenuModel`]: status line, then the live
/// usage lines (informational, disabled), then Open / Settings / Refresh Now /
/// Quit.
fn build_menu<R: Runtime>(app: &AppHandle<R>, menu: &MenuModel) -> tauri::Result<BuiltMenu<R>> {
    let status_item = MenuItem::with_id(app, "status", &menu.status_line, false, None::<&str>)?;
    let usage_items = menu
        .usage_lines
        .iter()
        .enumerate()
        .map(|(index, line)| {
            MenuItem::with_id(app, format!("usage-{index}"), line, false, None::<&str>)
        })
        .collect::<tauri::Result<Vec<_>>>()?;
    let open = MenuItem::with_id(app, "open", "Open Rusted Claude Meter", true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "Settings…", true, None::<&str>)?;
    let refresh = MenuItem::with_id(app, "refresh", "Refresh Now", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let usage_separator = PredefinedMenuItem::separator(app)?;
    let actions_separator = PredefinedMenuItem::separator(app)?;

    let mut items: Vec<&dyn IsMenuItem<R>> = vec![&status_item];
    if !usage_items.is_empty() {
        items.push(&usage_separator);
        for item in &usage_items {
            items.push(item);
        }
    }
    items.push(&actions_separator);
    items.push(&open);
    items.push(&settings);
    items.push(&refresh);
    items.push(&quit);

    let built = Menu::with_items(app, &items)?;
    Ok((built, status_item, usage_items))
}

/// Wrap rendered RGBA bytes in a tray image.
fn tray_image(icon: &RenderedIcon) -> Image<'static> {
    Image::new_owned(icon.rgba.clone(), icon.width, icon.height)
}
