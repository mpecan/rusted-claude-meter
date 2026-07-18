//! Weekly pace basis and pace-first display commands (issue #16).
//!
//! Split out of the main `commands` module purely to keep that file under
//! the workspace's file-size gate — these two commands and their `store_*`
//! helpers behave exactly like the other single-field settings commands
//! there (`set_show_reset_time` et al.); see this module's doc comments for
//! the one deliberate asymmetry (only [`set_pace_first_display`] forces an
//! immediate tray redraw).

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

/// Change how many days of the week the weekly quota is paced over (5/6/7,
/// issue #16's working-week option). Broadcasts `settings-changed` so the
/// popover (a separate window) picks it up live; the tray icon's badge picks
/// it up on its next redraw (every scheduler tick), same as the toggle
/// below when nothing else forces an earlier one.
#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
pub fn set_weekly_pace_days(
    app: tauri::AppHandle,
    settings: State<'_, SettingsState>,
    days: u8,
) -> AppSettings {
    let updated = store_weekly_pace_days(&settings, days);
    let _ = app.emit(SETTINGS_CHANGED_EVENT, &updated);
    updated
}

/// Toggle pace-first display mode (issue #16): the tray/popover lead with
/// the pace ratio instead of the raw quota percentage, and the flame/
/// snowflake badge appears. Unlike [`set_weekly_pace_days`] above, this one
/// pushes into the live tray immediately — flipping the primary display mode
/// is the dramatic, binary change (the badge appearing/disappearing
/// altogether), so it shouldn't wait out the next scheduled tick the way a
/// pacing-span tweak reasonably can.
#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
pub fn set_pace_first_display(
    app: tauri::AppHandle,
    scheduler: State<'_, SchedulerHandle>,
    settings: State<'_, SettingsState>,
    enabled: bool,
) -> AppSettings {
    let updated = store_pace_first_display(&settings, enabled);
    let _ = app.emit(SETTINGS_CHANGED_EVENT, &updated);
    tray::set_pace_options(
        &app,
        updated.weekly_pace_days,
        updated.pace_first_display,
        &scheduler.state_now(),
    );
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
}
