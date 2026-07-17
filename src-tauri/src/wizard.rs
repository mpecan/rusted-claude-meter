//! First-run setup wizard (issue #11): welcome → session (import or paste) →
//! validate → pick icon style + interval → done.
//!
//! Most of the wizard's steps are just the existing commands
//! (`commands::set_icon_style`, `commands::set_refresh_interval`,
//! `browser_import::import_browser_session`, and — for the paste step —
//! `commands::set_session_key`, which validates a pasted key with the same
//! rollback-on-rejection guarantee browser import gives an imported one)
//! driven from a different screen. This module only adds what those don't
//! already cover: detecting whether the wizard should run at all, the
//! completion marker, and the GNOME `AppIndicator` hint.

use tauri::State;

use crate::settings::SettingsState;

/// Managed Tauri state: whether the wizard should open automatically on
/// startup. Computed once, in `lib.rs::run`, from whether `settings.json`
/// existed *before* this launch loaded (or defaulted) it — the per-issue
/// "detect first run via absence of settings" signal. Re-opening the wizard
/// later from Settings ("Run setup again") does not touch this; it is purely
/// a frontend action.
pub struct FirstRunState(pub bool);

/// Whether the wizard should open automatically on this launch.
#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
pub fn wizard_should_run(state: State<'_, FirstRunState>) -> bool {
    state.0
}

/// Mark the wizard as complete by writing the current settings to disk even
/// if nothing in it changed — the "absence of settings" first-run signal
/// only goes away once something has actually been persisted, and a user who
/// accepts every default without touching a control would otherwise never
/// trip a `settings::save` and see the wizard again on the next launch.
#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
pub fn wizard_complete(settings: State<'_, SettingsState>) {
    settings.update(|_| {});
}

/// Whether this Linux session is GNOME, which hides every
/// `StatusNotifierItem` tray (including this app's) unless the
/// "`AppIndicator` and `KStatusNotifierItem` Support" extension is
/// installed. `false` on every other platform. See
/// `meter_core::desktop_is_gnome` for the pure classification and the
/// crate's `CLAUDE.md` for the "Linux tray reality" background.
#[tauri::command]
pub fn is_gnome_desktop() -> bool {
    std::env::var("XDG_CURRENT_DESKTOP").is_ok_and(|value| meter_core::desktop_is_gnome(&value))
}

#[cfg(test)]
mod tests {
    #[test]
    fn gnome_env_value_is_classified_through_the_pure_helper() {
        // is_gnome_desktop itself reads the real process environment, so it
        // is not asserted on directly here (that would be an I/O-flavoured,
        // environment-dependent test); this just pins that the command
        // delegates to the pure, already-tested classifier rather than
        // reimplementing the matching logic.
        assert!(meter_core::desktop_is_gnome("ubuntu:GNOME"));
        assert!(!meter_core::desktop_is_gnome("KDE"));
    }
}
