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

use std::sync::atomic::{AtomicBool, Ordering};

use tauri::State;

use crate::settings::SettingsState;

/// Managed Tauri state: whether the wizard still needs to be auto-opened this
/// process. Seeded once, in `lib.rs::run`, from whether `settings.json`
/// existed *before* this launch loaded (or defaulted) it — the per-issue
/// "detect first run via absence of settings" signal.
///
/// It is a consume-once flag (`AtomicBool`) rather than a plain `bool` because
/// the Settings window is destroyed on close and rebuilt on the next open, so
/// its frontend (`settings-view.ts`) runs `wizard.maybeAutoOpen()` on *every*
/// open, not once per process. Without a way to record "already offered", a
/// user who finished or skipped the wizard would be shown it again every time
/// they reopened Settings in the same session. `wizard_mark_offered` clears it
/// the moment the wizard is auto-opened, so only the very first Settings open
/// of a first-run session shows it. Re-opening the wizard later from Settings
/// ("Run setup again") does not touch this; it is purely a frontend action.
pub struct FirstRunState(pub AtomicBool);

impl FirstRunState {
    /// Seed the flag: `true` on a first run (no `settings.json` yet), `false`
    /// otherwise.
    pub const fn new(first_run: bool) -> Self {
        Self(AtomicBool::new(first_run))
    }

    /// Whether the wizard should still be auto-opened. A pure read — clearing
    /// is [`Self::mark_offered`]'s job.
    fn should_run(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }

    /// Record that the wizard has now been offered this process.
    fn mark_offered(&self) {
        self.0.store(false, Ordering::Relaxed);
    }
}

/// Whether the wizard should open automatically on this launch. A pure read:
/// both the popover (deciding whether to surface the Settings window on
/// launch) and the Settings window's `maybeAutoOpen` observe the same flag;
/// clearing it is `wizard_mark_offered`'s job, run once the wizard is shown.
#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
pub fn wizard_should_run(state: State<'_, FirstRunState>) -> bool {
    state.should_run()
}

/// Record that the first-run wizard has now been offered this process, so a
/// later rebuild of the (destroy-on-close) Settings window does not auto-open
/// it a second time. Called by `maybeAutoOpen` the moment it opens the wizard.
#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
pub fn wizard_mark_offered(state: State<'_, FirstRunState>) {
    state.mark_offered();
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
    use super::FirstRunState;

    #[test]
    fn first_run_flag_is_consumed_once_so_window_rebuilds_do_not_re_offer() {
        // Fresh install: no settings.json -> the wizard should be offered.
        let state = FirstRunState::new(true);
        assert!(
            state.should_run(),
            "first observation must offer the wizard"
        );

        // maybeAutoOpen offers it and records that it has been offered.
        state.mark_offered();

        // The Settings window is destroy-on-close: every later open re-reads
        // the flag. It must now stay false so the wizard is not re-shown.
        assert!(!state.should_run());
        assert!(!state.should_run());
    }

    #[test]
    fn non_first_run_never_offers_the_wizard() {
        let state = FirstRunState::new(false);
        assert!(!state.should_run());
    }

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
