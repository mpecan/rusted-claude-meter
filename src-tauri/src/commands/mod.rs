//! Tauri commands for session-key management.
//!
//! Each `#[tauri::command]` is a thin adapter over a directly unit-testable
//! function that talks to a `dyn SessionStore` — no `tauri::State`
//! construction required in tests — so parse-error and store-error mapping
//! is covered without spinning up the Tauri runtime.
//!
//! Two invariants every session command honors:
//!
//! * A pasted key is **validated against claude.ai** before it is allowed to
//!   stick, with rollback on rejection (`browser_import::store_and_validate`
//!   — the same guarantee browser import and the wizard give).
//! * Credential-store I/O never runs on the UI thread: the commands are
//!   `async` and route Keychain / Secret-Service calls through
//!   [`run_store_op`]'s blocking pool, so a slow or stuck credential daemon
//!   can never freeze tray or window redraws.

pub mod browser;
pub mod consent;
pub mod debug;
pub mod pace;
pub mod popover;
pub mod session;
pub mod source;

use std::collections::HashSet;
use std::sync::Arc;

use meter_core::{UsageMode, UsageStatus};
use meter_render::{IconState, IconStyle, Scale, render_icon};
use serde::Serialize;
use tauri::{Emitter, State};

use crate::scheduler::{MeterState, RefreshInterval, SchedulerHandle};
use crate::settings::{AppSettings, PopoverLayout, SettingsState};
use crate::store::SessionStore;
use crate::tray;

/// Managed Tauri state wrapping the active [`SessionStore`].
pub struct SessionStoreState(pub Arc<dyn SessionStore>);

/// Broadcast to every window whenever settings that another window renders
/// change. Now that Settings lives in its own window (see
/// `crate::settings_window`), the popover — a separate window — can no longer
/// see a model-visibility toggle by sharing the same `settings` object, so it
/// subscribes to this to re-filter its cards live. Carries the full
/// [`AppSettings`] so any future cross-window setting can piggyback on it.
pub const SETTINGS_CHANGED_EVENT: &str = "settings-changed";

/// Current scheduler state, for the initial render before the first
/// `usage-state` event arrives.
#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
pub fn usage_state(scheduler: State<'_, SchedulerHandle>) -> MeterState {
    scheduler.state_now()
}

/// Ask for a refresh now. TTL-guarded: a snapshot younger than ~55s is
/// served from memory instead of re-hitting the API.
#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
pub fn refresh_usage(scheduler: State<'_, SchedulerHandle>) {
    scheduler.request_refresh();
}

/// Change the polling cadence (60 / 300 / 600 seconds) and persist the
/// choice (Settings, issue #6) so it survives a restart.
#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
pub fn set_refresh_interval(
    scheduler: State<'_, SchedulerHandle>,
    settings: State<'_, SettingsState>,
    interval: RefreshInterval,
) {
    scheduler.update(|core| core.interval = interval);
    settings.update(|s| s.refresh_interval = interval);
}

/// Change the tray icon style (Settings, issue #9) and apply it
/// immediately, so switching styles never needs a restart. Persisted
/// (Settings, issue #6) so it survives a restart too.
#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
pub fn set_icon_style(
    app: tauri::AppHandle,
    scheduler: State<'_, SchedulerHandle>,
    settings: State<'_, SettingsState>,
    style: IconStyle,
) {
    settings.update(|s| s.icon_style = style);
    tray::set_style(&app, style, &scheduler.state_now());
}

/// One rendered preview for the Settings/wizard icon-style picker: straight
/// RGBA the frontend paints into a `<canvas>` so the buttons show the actual
/// tray artwork (issue #9's visual picker, mirroring `ClaudeMeter`'s
/// `IconStylePicker`).
#[derive(Serialize)]
pub struct IconPreview {
    pub style: IconStyle,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Render every icon style at one representative sample state (65% session /
/// 45% weekly, warning) for the picker. Always rendered in colour (a
/// template/monochrome preview would be black-on-dark and invisible in the
/// picker) — the picker communicates *shape*, mirroring `ClaudeMeter`'s
/// coloured `IconStylePicker`. Styles that fail to render are omitted rather
/// than erroring the whole picker.
#[tauri::command]
pub fn icon_style_previews() -> Vec<IconPreview> {
    const SAMPLE_PRIMARY: u8 = 65;
    const SAMPLE_SECONDARY: u8 = 45;
    const STYLES: [IconStyle; 6] = [
        IconStyle::Battery,
        IconStyle::Circular,
        IconStyle::Minimal,
        IconStyle::Segments,
        IconStyle::DualBar,
        IconStyle::Gauge,
    ];
    STYLES
        .into_iter()
        .filter_map(|style| {
            let state = IconState {
                style,
                percent: SAMPLE_PRIMARY,
                secondary_percent: SAMPLE_SECONDARY,
                status: UsageStatus::Warning,
                at_risk: false,
                pace_kind: None,
                pace_band: None,
                pace_ratio: None,
                mono: false,
                scale: Scale::X2,
            };
            let icon = render_icon(&state).ok()?;
            Some(IconPreview {
                style,
                width: icon.width,
                height: icon.height,
                rgba: icon.rgba,
            })
        })
        .collect()
}

/// The current settings, for the Settings panel's initial render.
#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
pub fn get_settings(settings: State<'_, SettingsState>) -> AppSettings {
    settings.get()
}

/// Toggle the tray/popover between template (monochrome) and full-colour
/// icon artwork, and apply it immediately (Settings, issue #6). Persisted so
/// it survives a restart.
#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
pub fn set_monochrome(
    app: tauri::AppHandle,
    scheduler: State<'_, SchedulerHandle>,
    settings: State<'_, SettingsState>,
    monochrome: bool,
) {
    settings.update(|s| s.monochrome = monochrome);
    tray::set_mono(&app, monochrome, &scheduler.state_now());
}

/// Replace the opt-in set of scoped-model display names the popover and
/// Linux tray menu are allowed to show (Settings, issue #6), and apply it
/// immediately. `models` need not be deduplicated — the tray only ever reads
/// it as a set.
#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
pub fn set_shown_scoped_models(
    app: tauri::AppHandle,
    scheduler: State<'_, SchedulerHandle>,
    settings: State<'_, SettingsState>,
    models: Vec<String>,
) {
    let updated = settings.update(|s| s.shown_scoped_models = models);
    // Tell the popover window (which filters its cards by this set) before we
    // consume `updated` for the tray update below.
    let _ = app.emit(SETTINGS_CHANGED_EVENT, &updated);
    let shown: HashSet<String> = updated.shown_scoped_models.into_iter().collect();
    tray::set_shown_scoped_models(&app, shown, &scheduler.state_now());
}

/// Update the warning/critical notification thresholds (Settings, issue #6;
/// consumed by notifications, issue #7). Both are clamped to `0..=100` by
/// the settings store; the resolved values are returned so the frontend's
/// sliders can reflect the clamp.
#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
pub fn set_thresholds(
    settings: State<'_, SettingsState>,
    warning: f64,
    critical: f64,
) -> AppSettings {
    settings.update(|s| {
        s.warning_threshold = warning;
        s.critical_threshold = critical;
    })
}

/// Toggle the extra "limit reset" notification (issue #7) on or off.
/// Threshold-crossing notifications are always on; this only gates the
/// noisier reset notice.
#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
pub fn set_notify_on_reset(settings: State<'_, SettingsState>, enabled: bool) -> AppSettings {
    store_notify_on_reset(&settings, enabled)
}

/// Persist the notify-on-reset toggle. Split from the command so the settings
/// mutation is unit-testable without a Tauri runtime.
fn store_notify_on_reset(settings: &SettingsState, enabled: bool) -> AppSettings {
    settings.update(|s| s.notify_on_reset = enabled)
}

/// Toggle whether popover cards append the exact reset wall-clock time
/// (`ClaudeMeter` PR #26). Emits `settings-changed` so the popover window
/// re-renders its cards immediately, since the toggle lives in the separate
/// Settings window.
#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
pub fn set_show_reset_time(
    app: tauri::AppHandle,
    settings: State<'_, SettingsState>,
    enabled: bool,
) -> AppSettings {
    let updated = store_show_reset_time(&settings, enabled);
    let _ = app.emit(SETTINGS_CHANGED_EVENT, &updated);
    updated
}

/// Persist the show-reset-time toggle. Split from the command so the settings
/// mutation is unit-testable without a Tauri `AppHandle`.
fn store_show_reset_time(settings: &SettingsState, enabled: bool) -> AppSettings {
    settings.update(|s| s.show_reset_time = enabled)
}

/// Switch the popover layout (redesign 1a Rows / 1c Cards). Emits
/// `settings-changed` so the popover window re-renders immediately, since the
/// control lives in the separate Settings window.
#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
pub fn set_popover_layout(
    app: tauri::AppHandle,
    settings: State<'_, SettingsState>,
    layout: PopoverLayout,
) -> AppSettings {
    let updated = store_popover_layout(&settings, layout);
    let _ = app.emit(SETTINGS_CHANGED_EVENT, &updated);
    updated
}

/// Persist the popover layout. Split from the command so the settings
/// mutation is unit-testable without a Tauri `AppHandle`.
fn store_popover_layout(settings: &SettingsState, layout: PopoverLayout) -> AppSettings {
    settings.update(|s| s.popover_layout = layout)
}

/// Switch how usage is presented — Auto (follow the account), Allowance
/// (percentage-of-limit) or Cost (spend). Emits `settings-changed` so the
/// popover, a separate window, re-renders in the new mode, and pushes the
/// resolved mode into the live tray so its icon and menu switch between the
/// percentage and spend views immediately — via `pace::broadcast_and_push`,
/// the shared settings-command broadcast/push helper, rather than a new
/// near-duplicate emit-and-push block.
#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
pub fn set_usage_mode(
    app: tauri::AppHandle,
    scheduler: State<'_, SchedulerHandle>,
    settings: State<'_, SettingsState>,
    mode: UsageMode,
) -> AppSettings {
    let updated = store_usage_mode(&settings, mode);
    pace::broadcast_and_push(&app, &scheduler, &updated);
    updated
}

/// Persist the usage mode. Split from the command so the settings mutation is
/// unit-testable without a Tauri `AppHandle`, mirroring `store_popover_layout`.
fn store_usage_mode(settings: &SettingsState, mode: UsageMode) -> AppSettings {
    settings.update(|s| s.usage_mode = mode)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn show_reset_time_toggle_persists_both_ways() {
        let state = SettingsState::new(None, AppSettings::default());
        // Default is on; toggling off then on round-trips through the store.
        assert!(!store_show_reset_time(&state, false).show_reset_time);
        assert!(store_show_reset_time(&state, true).show_reset_time);
    }

    #[test]
    fn notify_on_reset_toggle_persists_both_ways() {
        let state = SettingsState::new(None, AppSettings::default());
        assert!(store_notify_on_reset(&state, true).notify_on_reset);
        assert!(!store_notify_on_reset(&state, false).notify_on_reset);
    }

    #[test]
    fn popover_layout_persists() {
        let state = SettingsState::new(None, AppSettings::default());
        // Default is Rows; switching to Cards and back round-trips.
        assert_eq!(
            store_popover_layout(&state, PopoverLayout::Cards).popover_layout,
            PopoverLayout::Cards
        );
        assert_eq!(
            store_popover_layout(&state, PopoverLayout::Rows).popover_layout,
            PopoverLayout::Rows
        );
    }

    #[test]
    fn usage_mode_persists() {
        let state = SettingsState::new(None, AppSettings::default());
        // Default is Auto; pinning Cost and Allowance both round-trip.
        assert_eq!(
            store_usage_mode(&state, UsageMode::Auto).usage_mode,
            UsageMode::Auto
        );
        assert_eq!(
            store_usage_mode(&state, UsageMode::Cost).usage_mode,
            UsageMode::Cost
        );
        assert_eq!(
            store_usage_mode(&state, UsageMode::Allowance).usage_mode,
            UsageMode::Allowance
        );
    }

    #[test]
    fn icon_style_previews_renders_every_style_with_pixels() {
        let previews = icon_style_previews();
        // Every one of the six styles renders — none dropped.
        assert_eq!(previews.len(), 6);
        for preview in &previews {
            assert_eq!(
                preview.rgba.len(),
                (preview.width * preview.height * 4) as usize,
                "{:?} rgba length must match its dimensions",
                preview.style
            );
            assert!(
                preview.rgba.iter().any(|&b| b != 0),
                "{:?} preview must contain visible pixels",
                preview.style
            );
            // Sample state renders at 2x (44px tall) and wider than tall for
            // the text styles, so the picker shows real artwork.
            assert_eq!(preview.height, 44);
        }
    }
}
