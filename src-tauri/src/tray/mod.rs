//! Tray icon and its live menu.
//!
//! Interaction model differs by platform. On macOS the menu serves
//! right-click while left-click toggles a native `NSPopover` hosting the
//! webview. On Linux `StatusNotifierItem` delivers no click events at all
//! through libappindicator, so the menu is the entire surface: it carries a
//! live line per usage window, and "Open Rusted Claude Meter" opens the main
//! window. There is no Linux popover — a client cannot position its own
//! toplevel on Wayland, so an anchored pop-down is not something an
//! application can implement there.
//!
//! All display strings and the debounce logic live in the pure [`model`]
//! module; this file owns the shared state and decides *what* to show, while
//! [`native`] owns the Tauri handles and does the pushing. [`apply_state`]
//! is the live path: the scheduler calls it with every broadcast state, and
//! the [`model::TrayDiff`] gate turns repeats into no-ops. The tray icon
//! itself is never recreated — that is what avoids flicker.

mod model;
mod native;

#[cfg(target_os = "macos")]
mod popover;

use std::collections::HashSet;
use std::sync::Mutex;

use jiff::Timestamp;
use meter_core::UsageMode;
use meter_render::{IconCache, IconStyle, Scale};
use tauri::{AppHandle, Manager, Runtime};

use crate::scheduler::MeterState;
use crate::sync::lock;
use model::{IconOptions, PaceOptions, TrayDiff};
use native::NativeTray;

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
    pub usage_mode: UsageMode,
}

/// Everything the live update path mutates, behind one lock.
///
/// Lock discipline: [`apply_state`] (scheduler thread) and [`set_style`] /
/// [`set_mono`] / [`set_shown_scoped_models`] (Settings commands, issue #6,
/// invoked from a webview IPC thread) all take this lock. What must never
/// take it is the main event loop: no menu or tray-click handler may lock
/// here, because tray and menu mutations dispatch to the main thread and
/// block, so a main-thread wait on this lock while another thread holds it
/// would deadlock.
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
    /// How usage is presented — allowance (percentage) vs cost (spend), or
    /// `Auto` to follow the account. Read fresh on every [`apply_state`], so
    /// [`set_usage_mode`] takes effect on the next render with no rebuild.
    usage_mode: UsageMode,
    /// The native tray resources.
    tray: NativeTray<R>,
}

/// Managed Tauri state wrapping the tray's mutable resources.
pub struct TrayUpdater<R: Runtime>(Mutex<TrayResources<R>>);

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
        usage_mode,
    } = seed;
    let pace = PaceOptions {
        weekly_pace_days,
        pace_first_display,
    };
    let now = Timestamp::now();
    let menu_model = model::menu_model(initial, now, &shown, pace, usage_mode);

    // A render failure hands the backend `None` rather than aborting startup,
    // and stays uncommitted so the first broadcast retries the real gauge.
    let mut cache = IconCache::new();
    let mut diff = TrayDiff::default();
    let icon_options = IconOptions {
        style,
        mono,
        scale: Scale::X2,
    };
    let icon = model::icon_state(initial, now, icon_options, pace, usage_mode);
    let rendered = match cache.get_or_render(icon) {
        Ok(rendered) => Some(rendered),
        Err(error) => {
            eprintln!("tray icon render failed, using default icon: {error}");
            None
        }
    };

    let tray = NativeTray::build(app, rendered.as_deref(), &menu_model)?;
    if rendered.is_some() {
        diff.commit_icon(icon);
    }
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
        usage_mode,
        tray,
    })));
    Ok(())
}

/// Apply `mutate` to the tray's live resources — a no-op if the tray has not
/// been initialized yet — then re-render `state` under the new resources.
///
/// The shared body behind every live Settings-driven switch (`set_style`,
/// `set_mono`, `set_shown_scoped_models`, `set_pace_options`,
/// `set_usage_mode`): each is a one-line closure over the field it owns, so
/// none of them repeat the `try_state` / `lock` / `apply_state` dance (which,
/// duplicated per setter, would grow the `just dupes` ceiling).
fn mutate_and_apply<R: Runtime>(
    app: &AppHandle<R>,
    state: &MeterState,
    mutate: impl FnOnce(&mut TrayResources<R>),
) {
    if let Some(updater) = app.try_state::<TrayUpdater<R>>() {
        let mut resources = lock(&updater.0);
        mutate(&mut resources);
    }
    apply_state(app, state);
}

/// Change the tray's icon style and re-render the current state under it
/// immediately — the live-switch path Settings drives (issue #9). A no-op if
/// the tray has not been initialized yet.
pub fn set_style<R: Runtime>(app: &AppHandle<R>, style: IconStyle, state: &MeterState) {
    mutate_and_apply(app, state, |resources| resources.style = style);
}

/// Change the tray's monochrome/colour choice and re-render immediately —
/// the live-switch path Settings drives (issue #6). A no-op if the tray has
/// not been initialized yet.
pub fn set_mono<R: Runtime>(app: &AppHandle<R>, mono: bool, state: &MeterState) {
    mutate_and_apply(app, state, |resources| resources.mono = mono);
}

/// Change which scoped models render as usage lines and re-render
/// immediately — the live-switch path Settings drives (issue #6). A no-op if
/// the tray has not been initialized yet.
pub fn set_shown_scoped_models<R: Runtime>(
    app: &AppHandle<R>,
    shown: HashSet<String>,
    state: &MeterState,
) {
    mutate_and_apply(app, state, |resources| resources.shown = shown);
}

/// Change the weekly pace basis (5/6/7 days) and pace-first display mode
/// together and re-render immediately — the live-switch path Settings
/// drives (issue #16). Both are set in one call because they always change
/// together from a single settings command's resolved snapshot. A no-op if
/// the tray has not been initialized yet.
pub fn set_pace_options<R: Runtime>(
    app: &AppHandle<R>,
    weekly_pace_days: u8,
    pace_first_display: bool,
    state: &MeterState,
) {
    mutate_and_apply(app, state, |resources| {
        resources.weekly_pace_days = weekly_pace_days;
        resources.pace_first_display = pace_first_display;
    });
}

/// Change how usage is presented — allowance (percentage) vs cost (spend), or
/// `Auto` to follow the account — and re-render immediately, the live-switch
/// path the Usage-mode setting drives. A no-op if the tray has not been
/// initialized yet.
pub fn set_usage_mode<R: Runtime>(app: &AppHandle<R>, usage_mode: UsageMode, state: &MeterState) {
    mutate_and_apply(app, state, |resources| resources.usage_mode = usage_mode);
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

    let now = Timestamp::now();

    let mut resources = lock(&updater.0);
    let pace = PaceOptions {
        weekly_pace_days: resources.weekly_pace_days,
        pace_first_display: resources.pace_first_display,
    };
    let usage_mode = resources.usage_mode;
    let menu = model::menu_model(state, now, &resources.shown, pace, usage_mode);
    let icon = model::icon_state(
        state,
        now,
        IconOptions {
            style: resources.style,
            mono: resources.mono,
            scale: Scale::X2,
        },
        pace,
        usage_mode,
    );
    let plan = resources.diff.plan(icon, &menu);
    if let Some(icon) = plan.icon {
        match resources.cache.get_or_render(icon) {
            // Only a successful set is recorded, so a failure here is retried
            // on the next state instead of being debounced away.
            Ok(rendered) => {
                if native::set_icon(app, &rendered) {
                    resources.diff.commit_icon(icon);
                }
            }
            Err(error) => eprintln!("tray icon render failed, keeping previous icon: {error}"),
        }
    }
    if let Some(menu) = plan.menu
        && resources.tray.set_menu(app, &menu)
    {
        resources.diff.commit_menu(menu);
    }
}

/// Show the app's main surface — the "Open" menu item, and the popover
/// toggle's show half. Private to `tray`, which its backend submodules can
/// still reach as descendants.
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
