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

use std::collections::HashSet;
use std::sync::{Mutex, MutexGuard, PoisonError};

use jiff::Timestamp;
use meter_render::{IconCache, IconStyle, RenderedIcon, Scale};
use tauri::image::Image;
use tauri::menu::{IsMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{TrayIcon, TrayIconBuilder};
use tauri::{AppHandle, Manager, Runtime};

use crate::scheduler::{MeterState, SchedulerHandle};
use model::{IconOptions, MenuModel, PaceOptions, TrayDiff};

const TRAY_ID: &str = "main";

/// Everything [`init`] seeds from persisted settings at startup, bundled into
/// one value (mirrors `scheduler::PersistPaths`) so the function stays
/// within the workspace's `too_many_arguments` limit — style, monochrome,
/// the scoped-model opt-in set, and the pace options (issue #16) all come
/// from the same [`crate::settings::AppSettings`] snapshot at the one call
/// site anyway.
pub struct TraySeed {
    pub style: IconStyle,
    pub mono: bool,
    pub shown: HashSet<String>,
    pub weekly_pace_days: u8,
    pub pace_first_display: bool,
}

/// Everything the live update path mutates, behind one lock.
///
/// Lock discipline: [`apply_state`] (scheduler thread) and [`set_style`] /
/// [`set_mono`] / [`set_shown_scoped_models`] (Settings commands, issue #6,
/// invoked from a webview IPC thread) all take this lock. What must never
/// take it is the main event loop: no `on_menu_event`/`on_tray_icon_event`
/// handler may lock here, because tray and menu mutations dispatch to the
/// main thread and block, so a main-thread wait on this lock while another
/// thread holds it would deadlock.
struct TrayResources<R: Runtime> {
    cache: IconCache,
    diff: TrayDiff,
    /// The user's current style choice (Settings, issue #9). Read fresh on
    /// every [`apply_state`], so [`set_style`] changing it takes effect on
    /// the very next render — no restart, no tray/menu rebuild.
    style: IconStyle,
    /// The user's current monochrome choice (Settings, issue #6). Defaults
    /// to the platform-appropriate value baked into
    /// `settings::AppSettings::default` (macOS: template/monochrome; Linux:
    /// colour), but is now user-overridable, live, same as `style`.
    mono: bool,
    /// Opt-in set of scoped-model display names to show as usage lines
    /// (Settings, issue #6). Empty means no scoped model renders — see
    /// `model::menu_model`.
    shown: HashSet<String>,
    /// How many days of the week the weekly quota is paced over (issue #16),
    /// 5–7. Feeds `UsageSnapshot::pace_signal`'s weekly basis.
    weekly_pace_days: u8,
    /// Whether the flame/snowflake badge and the pace line are shown (issue
    /// #16). Off by default: quota-first mode never computes a pace signal.
    pace_first_display: bool,
    status_item: MenuItem<R>,
    /// The off-pace tooltip line (Linux has no tray tooltip, so this is its
    /// only home there), present only while `pace_first_display` is set and
    /// a signal exists.
    pace_item: Option<MenuItem<R>>,
    usage_items: Vec<MenuItem<R>>,
}

/// Managed Tauri state wrapping the tray's mutable resources.
pub struct TrayUpdater<R: Runtime>(Mutex<TrayResources<R>>);

fn lock<R: Runtime>(updater: &TrayUpdater<R>) -> MutexGuard<'_, TrayResources<R>> {
    updater.0.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Build the tray icon and its menu, and manage the [`TrayUpdater`] so
/// [`apply_state`] can drive live updates. Must run before the scheduler
/// starts broadcasting, or early states would be dropped. `initial` is the
/// state to render right away — the caller passes the cache-restored state
/// so a restart never flashes an empty gauge. `seed` carries the persisted
/// [`crate::settings::AppSettings`] choices (issues #6, #16) so a restart
/// renders exactly as the user last configured it, not the hardcoded
/// defaults.
pub fn init<R: Runtime>(
    app: &AppHandle<R>,
    initial: &MeterState,
    seed: TraySeed,
) -> tauri::Result<()> {
    let TraySeed {
        style,
        mono,
        shown,
        weekly_pace_days,
        pace_first_display,
    } = seed;
    let pace = PaceOptions {
        weekly_pace_days,
        pace_first_display,
    };
    let now = Timestamp::now();
    let menu_model = model::menu_model(initial, now, &shown, pace);
    let (menu, status_item, usage_items, pace_item) = build_menu(app, &menu_model)?;

    let mut tray = TrayIconBuilder::with_id(TRAY_ID)
        .menu(&menu)
        // macOS reserves left-click for the popover window; everywhere else
        // (and on Linux always, since clicks are never delivered) the menu
        // is the primary surface.
        .show_menu_on_left_click(!cfg!(target_os = "macos"))
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => show_main_window(app),
            // "Settings…" opens the dedicated Settings window (its own
            // top-level window, front-most on macOS). This is the primary way
            // to reach Settings on Linux, where the tray delivers no click
            // events for a popover-style affordance (see the module docs).
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
        tray = tray.on_tray_icon_event(popover::handle_tray_event);
    }

    // A render failure falls back to the bundled app icon rather than
    // aborting startup — and stays uncommitted so the first broadcast
    // retries the real gauge.
    let mut cache = IconCache::new();
    let mut diff = TrayDiff::default();
    let icon = model::icon_state(
        initial,
        now,
        IconOptions {
            style,
            mono,
            scale: Scale::X2,
        },
        pace,
    );
    match cache.get_or_render(icon) {
        Ok(rendered) => {
            tray = tray
                .icon(tray_image(&rendered))
                .icon_as_template(rendered.is_template);
            diff.commit_icon(icon);
        }
        Err(error) => {
            eprintln!("tray icon render failed, using default icon: {error}");
            if let Some(icon) = app.default_window_icon() {
                tray = tray.icon(icon.clone());
            }
        }
    }

    tray.build(app)?;
    // The menu built above is what the tray now shows.
    diff.commit_menu(menu_model);
    app.manage(TrayUpdater(Mutex::new(TrayResources {
        cache,
        diff,
        style,
        mono,
        shown,
        weekly_pace_days,
        pace_first_display,
        status_item,
        pace_item,
        usage_items,
    })));
    Ok(())
}

/// Change the tray's icon style and re-render the current state under it
/// immediately — the live-switch path Settings drives (issue #9). A no-op if
/// the tray has not been initialized yet.
pub fn set_style<R: Runtime>(app: &AppHandle<R>, style: IconStyle, state: &MeterState) {
    if let Some(updater) = app.try_state::<TrayUpdater<R>>() {
        lock(&updater).style = style;
    }
    apply_state(app, state);
}

/// Change the tray's monochrome/colour choice and re-render immediately —
/// the live-switch path Settings drives (issue #6). A no-op if the tray has
/// not been initialized yet.
pub fn set_mono<R: Runtime>(app: &AppHandle<R>, mono: bool, state: &MeterState) {
    if let Some(updater) = app.try_state::<TrayUpdater<R>>() {
        lock(&updater).mono = mono;
    }
    apply_state(app, state);
}

/// Change which scoped models render as usage lines and re-render
/// immediately — the live-switch path Settings drives (issue #6). A no-op if
/// the tray has not been initialized yet.
pub fn set_shown_scoped_models<R: Runtime>(
    app: &AppHandle<R>,
    shown: HashSet<String>,
    state: &MeterState,
) {
    if let Some(updater) = app.try_state::<TrayUpdater<R>>() {
        lock(&updater).shown = shown;
    }
    apply_state(app, state);
}

/// Change the weekly pace basis (5/6/7 days) and pace-first display mode
/// together and re-render immediately — the live-switch path Settings
/// drives (issue #16). Both are set in one call because they always change
/// together from a single settings command's resolved snapshot, and because
/// a single two-field setter here is one function, not two near-identical
/// one-field ones that would otherwise mirror [`set_style`]/[`set_mono`]
/// closely enough to grow the duplication ceiling (`just dupes`). A no-op if
/// the tray has not been initialized yet.
pub fn set_pace_options<R: Runtime>(
    app: &AppHandle<R>,
    weekly_pace_days: u8,
    pace_first_display: bool,
    state: &MeterState,
) {
    if let Some(updater) = app.try_state::<TrayUpdater<R>>() {
        let mut resources = lock(&updater);
        resources.weekly_pace_days = weekly_pace_days;
        resources.pace_first_display = pace_first_display;
    }
    apply_state(app, state);
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

    let mut resources = lock(&updater);
    let pace = PaceOptions {
        weekly_pace_days: resources.weekly_pace_days,
        pace_first_display: resources.pace_first_display,
    };
    let menu = model::menu_model(state, now, &resources.shown, pace);
    let icon = model::icon_state(
        state,
        now,
        IconOptions {
            style: resources.style,
            mono: resources.mono,
            scale: Scale::X2,
        },
        pace,
    );
    let plan = resources.diff.plan(icon, &menu);
    if let Some(icon) = plan.icon {
        match resources.cache.get_or_render(icon) {
            Ok(rendered) => {
                if tray.set_icon(Some(tray_image(&rendered))).is_ok() {
                    let _ = tray.set_icon_as_template(rendered.is_template);
                    // Only a successful set is recorded, so a failure here
                    // is retried on the next state instead of debounced.
                    resources.diff.commit_icon(icon);
                }
            }
            Err(error) => eprintln!("tray icon render failed, keeping previous icon: {error}"),
        }
    }
    if let Some(menu) = plan.menu
        && apply_menu(app, &tray, &mut resources, &menu)
    {
        resources.diff.commit_menu(menu);
    }
}

/// Update menu text in place when the line count and pace-line presence are
/// both unchanged; rebuild the menu (never the tray icon) when usage lines
/// appeared/vanished, or when the pace line (issue #16) appeared/vanished —
/// the fast path can only update a `MenuItem` already built into the menu,
/// never add or remove one. Returns whether the menu now fully matches
/// `menu`, so the caller only commits the debounce gate on success.
fn apply_menu<R: Runtime>(
    app: &AppHandle<R>,
    tray: &TrayIcon<R>,
    resources: &mut TrayResources<R>,
    menu: &MenuModel,
) -> bool {
    let lines_match = resources.usage_items.len() == menu.usage_lines.len();
    let pace_presence_matches = resources.pace_item.is_some() == menu.pace_line.is_some();
    if lines_match && pace_presence_matches {
        let mut applied = resources.status_item.set_text(&menu.status_line).is_ok();
        for (item, line) in resources.usage_items.iter().zip(&menu.usage_lines) {
            applied &= item.set_text(line).is_ok();
        }
        if let (Some(item), Some(line)) = (&resources.pace_item, &menu.pace_line) {
            applied &= item.set_text(line).is_ok();
        }
        return applied;
    }
    match build_menu(app, menu) {
        Ok((rebuilt, status_item, usage_items, pace_item)) => {
            if tray.set_menu(Some(rebuilt)).is_ok() {
                resources.status_item = status_item;
                resources.usage_items = usage_items;
                resources.pace_item = pace_item;
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

type BuiltMenu<R> = (Menu<R>, MenuItem<R>, Vec<MenuItem<R>>, Option<MenuItem<R>>);

/// Build the full tray menu for a [`MenuModel`]: status line, the pace line
/// when pace-first display has a signal (issue #16 — the only place that
/// text is visible on Linux, which has no tray tooltip), live usage lines
/// (informational, disabled), then Open / Settings / Refresh Now / Quit.
/// "Settings…" is the primary way to reach Settings on Linux, where the tray
/// delivers no click events for a popover-style affordance.
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

fn show_main_window<R: Runtime>(app: &AppHandle<R>) {
    // On macOS the main window is an NSPopover, so "Open" shows the popover
    // (a bare `window.show()` would surface a naked, unanchored webview).
    #[cfg(target_os = "macos")]
    {
        use tauri_plugin_nspopover::AppExt as _;
        app.show_popover();
    }
    #[cfg(not(target_os = "macos"))]
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }
}
