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
//! completion marker, and the Linux desktop hints (GNOME's `AppIndicator`
//! requirement, Plasma's square tray cell).

use crate::sync::AtomicFlag;

use tauri::State;

use crate::settings::SettingsState;

/// Managed Tauri state: whether the wizard still needs to be auto-opened this
/// process. Seeded once, in `lib.rs::run`, from whether `settings.json`
/// existed *before* this launch loaded (or defaulted) it — the per-issue
/// "detect first run via absence of settings" signal.
///
/// It is a consume-once flag ([`AtomicFlag`]) rather than a plain `bool` because
/// the Settings window is destroyed on close and rebuilt on the next open, so
/// its frontend (`settings-view.ts`) runs `wizard.maybeAutoOpen()` on *every*
/// open, not once per process. Without a way to record "already offered", a
/// user who finished or skipped the wizard would be shown it again every time
/// they reopened Settings in the same session. `wizard_mark_offered` clears it
/// the moment the wizard is auto-opened, so only the very first Settings open
/// of a first-run session shows it. Re-opening the wizard later from Settings
/// ("Run setup again") does not touch this; it is purely a frontend action.
pub struct FirstRunState(pub AtomicFlag);

impl FirstRunState {
    /// Whether the wizard should still be auto-opened. A pure read — clearing
    /// is [`Self::mark_offered`]'s job.
    fn should_run(&self) -> bool {
        self.0.get()
    }

    /// Record that the wizard has now been offered this process.
    fn mark_offered(&self) {
        self.0.set(false);
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

/// Which desktop this session is, so the UI can warn about the two that
/// constrain the tray: GNOME hides every `StatusNotifierItem` without the
/// "`AppIndicator` and `KStatusNotifierItem` Support" extension, and Plasma
/// renders tray icons into a square cell so a wide icon draws small.
///
/// `Other` on every other desktop and on macOS, where `XDG_CURRENT_DESKTOP` is
/// unset. One command rather than a boolean per desktop: the reading of the
/// environment is the same either way, and a second `is_…_desktop` would be a
/// near-copy of the first. See `meter_core::LinuxDesktop` for the pure
/// classification and the crate's `CLAUDE.md` for the "Linux surface"
/// background.
#[tauri::command]
#[must_use]
pub fn linux_desktop() -> meter_core::LinuxDesktop {
    std::env::var("XDG_CURRENT_DESKTOP").map_or(meter_core::LinuxDesktop::Other, |value| {
        meter_core::LinuxDesktop::classify(&value)
    })
}

#[cfg(test)]
mod tests {
    use super::{AtomicFlag, FirstRunState};

    #[test]
    fn first_run_flag_is_consumed_once_so_window_rebuilds_do_not_re_offer() {
        // Fresh install: no settings.json -> the wizard should be offered.
        let state = FirstRunState(AtomicFlag::new(true));
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
        let state = FirstRunState(AtomicFlag::new(false));
        assert!(!state.should_run());
    }

    #[test]
    fn desktop_env_value_is_classified_through_the_pure_helper() {
        // linux_desktop itself reads the real process environment, so it is
        // not asserted on directly here (that would be an I/O-flavoured,
        // environment-dependent test); this just pins that the command
        // delegates to the pure, already-tested classifier rather than
        // reimplementing the matching logic.
        use meter_core::LinuxDesktop;
        assert_eq!(LinuxDesktop::classify("ubuntu:GNOME"), LinuxDesktop::Gnome);
        assert_eq!(LinuxDesktop::classify("KDE"), LinuxDesktop::Kde);
        assert_eq!(LinuxDesktop::classify("XFCE"), LinuxDesktop::Other);
    }
}
