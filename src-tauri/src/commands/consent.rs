//! The Terms-of-Service consent command — the one switch that decides whether
//! this app may talk to claude.ai at all (see `crate::consent` and
//! `docs/terms-of-service.md`).
//!
//! Split out of the main `commands` module to keep that file under the
//! workspace's file-size gate, and because this is not an ordinary preference:
//! it flips the live [`ConsentGate`] *and* moves the scheduler in the same
//! call, so ticking the box starts polling immediately and un-ticking it stops
//! polling immediately rather than at some point within the next refresh
//! interval. A user who withdraws consent has withdrawn it now.

use std::sync::Arc;

use tauri::State;

use crate::consent::ConsentGate;
use crate::scheduler::{FetchOutcome, SchedulerHandle};
use crate::settings::{AppSettings, SettingsState};

/// What every command that would reach claude.ai returns while consent is
/// withheld. Defined once so the session-paste path and the browser-import
/// path cannot drift into telling the user two different stories.
pub const WITHHELD_MESSAGE: &str = "Rusted Claude Meter is paused: it can't contact claude.ai \
     until you've read the Terms-of-Service warning and accepted the risk in Settings.";

/// Accept or withdraw the Terms-of-Service risk acknowledgement.
///
/// Ordering matters and is deliberate: the live gate moves *before* the
/// scheduler is told to resume, so a poll woken by `resume_polling` can never
/// observe a stale closed gate and immediately re-park; and on withdrawal the
/// gate closes before the loop is parked, so an in-flight tick that checks the
/// gate after this point also refuses.
#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
pub fn set_tos_acknowledged(
    settings: State<'_, SettingsState>,
    consent: State<'_, Arc<ConsentGate>>,
    scheduler: State<'_, SchedulerHandle>,
    acknowledged: bool,
) -> AppSettings {
    consent.set(acknowledged);
    if acknowledged {
        scheduler.resume_polling();
    } else {
        scheduler.update(|core| core.record(FetchOutcome::NotAcknowledged));
    }
    store_tos_acknowledged(&settings, acknowledged)
}

/// Persist the acknowledgement. Split from the command so the settings
/// mutation is unit-testable without a Tauri `AppHandle` or managed state,
/// matching `commands::debug::store_debug_logging`.
fn store_tos_acknowledged(settings: &SettingsState, acknowledged: bool) -> AppSettings {
    settings.update(|s| s.tos_acknowledged = acknowledged)
}

#[cfg(test)]
mod tests {
    use super::store_tos_acknowledged;
    use crate::settings::{AppSettings, SettingsState};

    fn state() -> SettingsState {
        SettingsState::new(None, AppSettings::default())
    }

    #[test]
    fn a_fresh_install_has_not_acknowledged_anything() {
        assert!(
            !AppSettings::default().tos_acknowledged,
            "consent must never default to given"
        );
    }

    #[test]
    fn acknowledgement_round_trips_through_settings() {
        let settings = state();
        assert!(store_tos_acknowledged(&settings, true).tos_acknowledged);
        assert!(settings.get().tos_acknowledged);
    }

    #[test]
    fn acknowledgement_can_be_withdrawn() {
        let settings = state();
        store_tos_acknowledged(&settings, true);
        assert!(!store_tos_acknowledged(&settings, false).tos_acknowledged);
        assert!(!settings.get().tos_acknowledged);
    }

    #[test]
    fn acknowledgement_does_not_disturb_other_settings() {
        let settings = state();
        let before = settings.get();
        let after = store_tos_acknowledged(&settings, true);
        assert_eq!(after.refresh_interval, before.refresh_interval);
        assert_eq!(after.icon_style, before.icon_style);
        assert_eq!(after.shown_scoped_models, before.shown_scoped_models);
    }
}
