//! Tauri's own tray (`TrayIconBuilder`), used on macOS.
//!
//! Moved here verbatim when the tray grew a second backend; the menu-building,
//! the in-place `set_text` fast path and the rebuild-on-line-count-change rule
//! are unchanged from when they lived in `tray/mod.rs`.
//!
//! On macOS left-click is reserved for the `NSPopover` (see [`super::super::popover`])
//! and the menu answers right-click. Linux does not use this backend — see
//! `tray/backend/mod.rs`.

use meter_render::RenderedIcon;
use tauri::image::Image;
use tauri::menu::{IsMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager, Runtime};

use super::TrayBackend;
use crate::scheduler::SchedulerHandle;
use crate::tray::model::MenuModel;
use crate::tray::show_main_window;

const TRAY_ID: &str = "main";

/// Live handles into the built menu, so text can be updated in place without
/// rebuilding (which is what avoids flicker).
pub(in crate::tray) struct TauriTray<R: Runtime> {
    status_item: MenuItem<R>,
    /// The off-pace pace line, present only while pace-first display is set
    /// and a signal exists.
    pace_item: Option<MenuItem<R>>,
    usage_items: Vec<MenuItem<R>>,
}

impl<R: Runtime> TrayBackend<R> for TauriTray<R> {
    fn build(
        app: &AppHandle<R>,
        icon: Option<&RenderedIcon>,
        menu: &MenuModel,
    ) -> tauri::Result<Self> {
        let (built, status_item, usage_items, pace_item) = build_menu(app, menu)?;

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
            tray = tray.on_tray_icon_event(crate::tray::popover::handle_tray_event);
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
            pace_item,
            usage_items,
        })
    }

    fn set_icon(&mut self, app: &AppHandle<R>, icon: &RenderedIcon) -> bool {
        let Some(tray) = app.tray_by_id(TRAY_ID) else {
            return false;
        };
        if tray.set_icon(Some(tray_image(icon))).is_err() {
            return false;
        }
        let _ = tray.set_icon_as_template(icon.is_template);
        true
    }

    /// Update menu text in place when the line count and pace-line presence
    /// are both unchanged; rebuild the menu (never the tray icon) when usage
    /// lines appeared/vanished, or when the pace line appeared/vanished — the
    /// fast path can only update a `MenuItem` already built into the menu,
    /// never add or remove one.
    fn set_menu(&mut self, app: &AppHandle<R>, menu: &MenuModel) -> bool {
        let Some(tray) = app.tray_by_id(TRAY_ID) else {
            return false;
        };
        let lines_match = self.usage_items.len() == menu.usage_lines.len();
        let pace_presence_matches = self.pace_item.is_some() == menu.pace_line.is_some();
        if lines_match && pace_presence_matches {
            let mut applied = self.status_item.set_text(&menu.status_line).is_ok();
            for (item, line) in self.usage_items.iter().zip(&menu.usage_lines) {
                applied &= item.set_text(line).is_ok();
            }
            if let (Some(item), Some(line)) = (&self.pace_item, &menu.pace_line) {
                applied &= item.set_text(line).is_ok();
            }
            return applied;
        }
        match build_menu(app, menu) {
            Ok((rebuilt, status_item, usage_items, pace_item)) => {
                if tray.set_menu(Some(rebuilt)).is_ok() {
                    self.status_item = status_item;
                    self.usage_items = usage_items;
                    self.pace_item = pace_item;
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

type BuiltMenu<R> = (Menu<R>, MenuItem<R>, Vec<MenuItem<R>>, Option<MenuItem<R>>);

/// Build the full tray menu for a [`MenuModel`]: status line, the pace line
/// when pace-first display has a signal, live usage lines (informational,
/// disabled), then Open / Settings / Refresh Now / Quit.
fn build_menu<R: Runtime>(app: &AppHandle<R>, menu: &MenuModel) -> tauri::Result<BuiltMenu<R>> {
    let status_item = MenuItem::with_id(app, "status", &menu.status_line, false, None::<&str>)?;
    let pace_item = menu
        .pace_line
        .as_ref()
        .map(|line| MenuItem::with_id(app, "pace", line, false, None::<&str>))
        .transpose()?;
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
    if let Some(item) = &pace_item {
        items.push(item);
    }
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
    Ok((built, status_item, usage_items, pace_item))
}

/// Wrap rendered RGBA bytes in a tray image.
fn tray_image(icon: &RenderedIcon) -> Image<'static> {
    Image::new_owned(icon.rgba.clone(), icon.width, icon.height)
}
