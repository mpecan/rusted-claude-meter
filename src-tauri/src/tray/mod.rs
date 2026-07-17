//! Tray icon, its live menu, and the macOS popover behaviour.
//!
//! Interaction model differs by platform: on Linux (`StatusNotifierItem` /
//! `AppIndicator`) the tray delivers no click events and no tooltip, so the
//! menu is the primary surface — it carries one live line per usage window.
//! On macOS the same menu serves right-click, while left-click toggles a
//! frameless always-on-top window anchored under the tray icon (popover
//! feel).
//!
//! All display strings and the debounce logic live in the pure [`model`]
//! module; this file only owns Tauri resources. [`apply_state`] is the live
//! path: the scheduler calls it with every broadcast state, and the
//! [`model::TrayDiff`] gate turns repeats into no-ops. Menu text is updated
//! in place via `MenuItem::set_text`; the menu is only rebuilt when the
//! number of usage lines changes (a new scoped model appeared or vanished),
//! and the tray icon itself is never recreated — that is what avoids
//! flicker.

mod model;

#[cfg(target_os = "macos")]
mod popover;

use std::sync::{Mutex, MutexGuard, PoisonError};

use jiff::Timestamp;
use meter_render::{IconCache, RenderedIcon, Scale};
use tauri::image::Image;
use tauri::menu::{IsMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{TrayIcon, TrayIconBuilder};
use tauri::{AppHandle, Manager, Runtime};

use crate::scheduler::{MeterState, SchedulerHandle};
use model::{MenuModel, TrayDiff};

const TRAY_ID: &str = "main";

/// macOS menu-bar icons should be templates so they adapt to light/dark
/// appearance; Linux trays have no template concept, so colour carries state.
const MONO: bool = cfg!(target_os = "macos");

/// Everything the live update path mutates, behind one lock.
///
/// Lock discipline: only [`apply_state`] (scheduler thread) takes this lock.
/// Menu/tray event handlers run on the main thread and must never take it —
/// tray and menu mutations dispatch to the main thread and block, so a
/// main-thread wait on this lock while the scheduler holds it would
/// deadlock.
struct TrayResources<R: Runtime> {
    cache: IconCache,
    diff: TrayDiff,
    status_item: MenuItem<R>,
    usage_items: Vec<MenuItem<R>>,
}

/// Managed Tauri state wrapping the tray's mutable resources.
pub struct TrayUpdater<R: Runtime>(Mutex<TrayResources<R>>);

fn lock<R: Runtime>(updater: &TrayUpdater<R>) -> MutexGuard<'_, TrayResources<R>> {
    updater.0.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Build the tray icon and its menu, and manage the [`TrayUpdater`] so
/// [`apply_state`] can drive live updates. Must run before the scheduler
/// starts broadcasting, or early states would be dropped.
pub fn init<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let now = Timestamp::now();
    let initial = MeterState::empty();
    let menu_model = model::menu_model(&initial, now);
    let (menu, status_item, usage_items) = build_menu(app, &menu_model)?;

    let mut tray = TrayIconBuilder::with_id(TRAY_ID)
        .menu(&menu)
        // macOS reserves left-click for the popover window; everywhere else
        // (and on Linux always, since clicks are never delivered) the menu
        // is the primary surface.
        .show_menu_on_left_click(!cfg!(target_os = "macos"))
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => show_main_window(app),
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
        tray = tray.on_tray_icon_event(popover::handle_tray_event);
    }

    // Empty gauge until the first snapshot arrives. A render failure falls
    // back to the bundled app icon rather than aborting startup.
    let mut cache = IconCache::new();
    let mut diff = TrayDiff::default();
    let plan = diff.plan(
        model::icon_state(&initial, now, MONO, Scale::X2),
        menu_model,
    );
    if let Some(icon) = plan.icon {
        match cache.get_or_render(icon) {
            Ok(rendered) => {
                tray = tray
                    .icon(tray_image(&rendered))
                    .icon_as_template(rendered.is_template);
            }
            Err(error) => {
                eprintln!("tray icon render failed, using default icon: {error}");
                if let Some(icon) = app.default_window_icon() {
                    tray = tray.icon(icon.clone());
                }
            }
        }
    }

    tray.build(app)?;
    app.manage(TrayUpdater(Mutex::new(TrayResources {
        cache,
        diff,
        status_item,
        usage_items,
    })));
    Ok(())
}

/// Live update path: fold one broadcast [`MeterState`] into the tray.
///
/// Safe to call from any thread (tray/menu mutations proxy to the main
/// thread internally). Repeated identical states are debounced to no-ops by
/// [`model::TrayDiff`], so the icon is only re-set when its rendered content
/// actually changes and menu text only when a line changed.
pub fn apply_state<R: Runtime>(app: &AppHandle<R>, state: &MeterState) {
    let Some(updater) = app.try_state::<TrayUpdater<R>>() else {
        return;
    };
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };

    let now = Timestamp::now();
    let icon = model::icon_state(state, now, MONO, Scale::X2);
    let menu = model::menu_model(state, now);

    let mut resources = lock(&updater);
    let plan = resources.diff.plan(icon, menu);
    if let Some(icon) = plan.icon {
        match resources.cache.get_or_render(icon) {
            Ok(rendered) => {
                let _ = tray.set_icon(Some(tray_image(&rendered)));
                let _ = tray.set_icon_as_template(rendered.is_template);
            }
            Err(error) => eprintln!("tray icon render failed, keeping previous icon: {error}"),
        }
    }
    if let Some(menu) = plan.menu {
        apply_menu(app, &tray, &mut resources, &menu);
    }
}

/// Update menu text in place when the line count is unchanged; rebuild the
/// menu (never the tray icon) only when usage lines appeared or vanished.
fn apply_menu<R: Runtime>(
    app: &AppHandle<R>,
    tray: &TrayIcon<R>,
    resources: &mut TrayResources<R>,
    menu: &MenuModel,
) {
    if resources.usage_items.len() == menu.usage_lines.len() {
        let _ = resources.status_item.set_text(&menu.status_line);
        for (item, line) in resources.usage_items.iter().zip(&menu.usage_lines) {
            let _ = item.set_text(line);
        }
        return;
    }
    match build_menu(app, menu) {
        Ok((rebuilt, status_item, usage_items)) => {
            if tray.set_menu(Some(rebuilt)).is_ok() {
                resources.status_item = status_item;
                resources.usage_items = usage_items;
            }
        }
        Err(error) => eprintln!("tray menu rebuild failed, keeping previous menu: {error}"),
    }
}

type BuiltMenu<R> = (Menu<R>, MenuItem<R>, Vec<MenuItem<R>>);

/// Build the full tray menu for a [`MenuModel`]: status line, live usage
/// lines (informational, disabled), then Open / Refresh Now / Quit.
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
    items.push(&refresh);
    items.push(&quit);

    let built = Menu::with_items(app, &items)?;
    Ok((built, status_item, usage_items))
}

/// Wrap rendered RGBA bytes in a tray image.
fn tray_image(icon: &RenderedIcon) -> Image<'static> {
    Image::new_owned(icon.rgba.clone(), icon.width, icon.height)
}

fn show_main_window<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }
}
