//! Weekly pace basis and pace-first display commands (issue #16).
//!
//! Split out of the main `commands` module purely to keep that file under
//! the workspace's file-size gate — these two commands and their `store_*`
//! helpers behave like the other settings commands there (`set_show_reset_time`
//! et al.), except both of them push their new value into the live tray via
//! `tray::set_pace_options` immediately: the tray's own pace ratio, badge and
//! menu line read `weekly_pace_days`/`pace_first_display` directly out of its
//! cached resources (see `tray::apply_state`), and nothing else re-reads
//! `AppSettings` on a scheduler tick to pick up a change on its own.

use tauri::{Emitter, State};

use crate::scheduler::SchedulerHandle;
use crate::settings::{AppSettings, SettingsState};
use crate::tray;

use super::SETTINGS_CHANGED_EVENT;

/// Persist the weekly pace basis. Split from the command so the settings
/// mutation is unit-testable without a Tauri `AppHandle`, mirroring
/// `store_show_reset_time`.
fn store_weekly_pace_days(settings: &SettingsState, days: u8) -> AppSettings {
    settings.update(|s| s.weekly_pace_days = days)
}

/// Persist the pace-first display toggle. Split from the command for the
/// same AppHandle-free testability as `store_weekly_pace_days`.
fn store_pace_first_display(settings: &SettingsState, enabled: bool) -> AppSettings {
    settings.update(|s| s.pace_first_display = enabled)
}

/// Persist the master pace-tracking switch (issue #16). Same AppHandle-free
/// testability as the sibling `store_*` helpers.
fn store_pace_tracking_enabled(settings: &SettingsState, enabled: bool) -> AppSettings {
    settings.update(|s| s.pace_tracking_enabled = enabled)
}

/// Broadcast `settings-changed` (so the popover, a separate window, picks up
/// the resolved snapshot live) and push both pace fields into the live tray
/// immediately — its own pace ratio, badge and menu line read
/// `weekly_pace_days`/`pace_first_display` directly out of `TrayResources`
/// (see `tray::apply_state`), so without this push they would otherwise keep
/// computing against a stale value indefinitely. Shared by
/// [`set_weekly_pace_days`] and [`set_pace_first_display`], the only two
/// settings that change what the tray's pace computation sees.
fn broadcast_and_push_pace(
    app: &tauri::AppHandle,
    scheduler: &SchedulerHandle,
    updated: &AppSettings,
) {
    let _ = app.emit(SETTINGS_CHANGED_EVENT, updated);
    tray::set_pace_options(
        app,
        updated.weekly_pace_days,
        // The tray only shows pace in pace-first mode, so the master
        // `pace_tracking_enabled` switch collapses into the effective
        // pace-first flag it sees — no separate field to thread through.
        updated.pace_tracking_enabled && updated.pace_first_display,
        &scheduler.state_now(),
    );
}

/// Change how many days of the week the weekly quota is paced over (5/6/7,
/// issue #16's working-week option), applying it to the live tray
/// immediately via [`broadcast_and_push_pace`].
#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
pub fn set_weekly_pace_days(
    app: tauri::AppHandle,
    scheduler: State<'_, SchedulerHandle>,
    settings: State<'_, SettingsState>,
    days: u8,
) -> AppSettings {
    let updated = store_weekly_pace_days(&settings, days);
    broadcast_and_push_pace(&app, &scheduler, &updated);
    updated
}

/// Toggle pace-first display mode (issue #16): the tray/popover lead with
/// the pace ratio instead of the raw quota percentage, and the flame/
/// snowflake badge appears. Applies to the live tray immediately via
/// [`broadcast_and_push_pace`].
#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
pub fn set_pace_first_display(
    app: tauri::AppHandle,
    scheduler: State<'_, SchedulerHandle>,
    settings: State<'_, SettingsState>,
    enabled: bool,
) -> AppSettings {
    let updated = store_pace_first_display(&settings, enabled);
    broadcast_and_push_pace(&app, &scheduler, &updated);
    updated
}

/// Master switch for the whole pace-tracking feature (issue #16). When off,
/// the popover drops projections/pace lines and the tray shows no pace ratio
/// or badge, regardless of `pace_first_display`; the sub-settings keep their
/// stored values. Applies to the live tray immediately via
/// [`broadcast_and_push_pace`], whose effective pace-first flag already folds
/// in this switch.
#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
pub fn set_pace_tracking_enabled(
    app: tauri::AppHandle,
    scheduler: State<'_, SchedulerHandle>,
    settings: State<'_, SettingsState>,
    enabled: bool,
) -> AppSettings {
    let updated = store_pace_tracking_enabled(&settings, enabled);
    broadcast_and_push_pace(&app, &scheduler, &updated);
    updated
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn weekly_pace_days_persists() {
        let state = SettingsState::new(None, AppSettings::default());
        // Default is 7 (full week); a working-week choice round-trips too.
        assert_eq!(store_weekly_pace_days(&state, 5).weekly_pace_days, 5);
        assert_eq!(store_weekly_pace_days(&state, 7).weekly_pace_days, 7);
    }

    #[test]
    fn weekly_pace_days_is_clamped_through_the_settings_store() {
        // `store_weekly_pace_days` delegates to `SettingsState::update`,
        // which normalizes on every write — an out-of-range span from a
        // misbehaving caller must still come back clamped to 5..=7.
        let state = SettingsState::new(None, AppSettings::default());
        assert_eq!(store_weekly_pace_days(&state, 1).weekly_pace_days, 5);
        assert_eq!(store_weekly_pace_days(&state, 9).weekly_pace_days, 7);
    }

    #[test]
    fn pace_first_display_toggle_persists_both_ways() {
        let state = SettingsState::new(None, AppSettings::default());
        // Default is off.
        assert!(!store_pace_first_display(&state, false).pace_first_display);
        assert!(store_pace_first_display(&state, true).pace_first_display);
    }

    #[test]
    fn pace_tracking_enabled_toggle_persists_both_ways() {
        let state = SettingsState::new(None, AppSettings::default());
        // Default is on (the feature is visible out of the box).
        assert!(store_pace_tracking_enabled(&state, true).pace_tracking_enabled);
        assert!(!store_pace_tracking_enabled(&state, false).pace_tracking_enabled);
    }
}
