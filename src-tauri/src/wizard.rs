//! First-run setup wizard (issue #11): welcome → session (import or paste) →
//! validate → pick icon style + interval → done.
//!
//! Most of the wizard's steps are just the existing Settings commands
//! (`commands::set_icon_style`, `commands::set_refresh_interval`,
//! `browser_import::import_browser_session`) driven from a different screen —
//! this module only adds what those don't already cover: detecting whether
//! the wizard should run at all, validating a *pasted* key the same way
//! browser import validates an imported one, and the GNOME `AppIndicator`
//! hint. Reusing `browser_import::store_and_validate` means a pasted key
//! gets the exact same rollback-on-rejection guarantee an imported one does
//! (see that module's docs), which is what keeps a cancelled wizard from
//! ever leaving a half-saved session behind.

use meter_core::SessionKey;
use serde::Serialize;
use tauri::State;

use crate::browser_import::{LiveSessionValidator, StoreAndValidateError, store_and_validate};
use crate::commands::SessionStoreState;
use crate::scheduler::SchedulerHandle;
use crate::settings::SettingsState;

/// Managed Tauri state: whether the wizard should open automatically on
/// startup. Computed once, in `lib.rs::run`, from whether `settings.json`
/// existed *before* this launch loaded (or defaulted) it — the per-issue
/// "detect first run via absence of settings" signal. Re-opening the wizard
/// later from Settings ("Run setup again") does not touch this; it is purely
/// a frontend action.
pub struct FirstRunState(pub bool);

/// Outcome of validating a pasted session key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct WizardSessionResult {
    /// Whether claude.ai confirmed the key. `false` means the key is stored
    /// but validation was skipped because claude.ai was unreachable — the
    /// scheduler validates it on its next poll (mirrors
    /// `browser_import::ImportSummary::validated`).
    pub validated: bool,
}

/// Why validating a pasted session key failed, mapped to a discriminated
/// union the frontend can message precisely. Carries only human-readable
/// summaries — never the pasted value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", content = "message")]
pub enum WizardSessionError {
    /// The pasted input failed [`SessionKey::parse`].
    Validation(String),
    /// The key parsed but claude.ai rejected it (expired/invalid).
    Rejected(String),
    /// The credential store refused to persist the key.
    Store(String),
}

async fn submit_session_key_impl(
    store: &dyn crate::store::SessionStore,
    validator: &impl crate::browser_import::SessionValidator,
    input: &str,
) -> Result<WizardSessionResult, WizardSessionError> {
    let key = SessionKey::parse(input)
        .map_err(|error| WizardSessionError::Validation(error.to_string()))?;
    let validated =
        store_and_validate(store, validator, &key)
            .await
            .map_err(|error| {
                match error {
            StoreAndValidateError::Store(message) => WizardSessionError::Store(message),
            StoreAndValidateError::Rejected => WizardSessionError::Rejected(
                "claude.ai rejected that session key — it may be expired. Sign in to claude.ai \
                 again, copy a fresh key, and try again."
                    .to_owned(),
            ),
        }
            })?;
    Ok(WizardSessionResult { validated })
}

/// Whether the wizard should open automatically on this launch.
#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
pub fn wizard_should_run(state: State<'_, FirstRunState>) -> bool {
    state.0
}

/// Parse, store and validate a pasted session key exactly like
/// `browser_import::import_browser_session` does for an imported one
/// (rollback on rejection, keep-and-retry-later on a network hiccup), then
/// wake the polling loop so a scheduler parked on "session expired" retries
/// with the new key.
#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
pub async fn wizard_submit_session_key(
    state: State<'_, SessionStoreState>,
    scheduler: State<'_, SchedulerHandle>,
    input: String,
) -> Result<WizardSessionResult, WizardSessionError> {
    // Extract owned handles before the first `await`: the `State` guards are
    // not `Send`, so holding them across the await would make the command's
    // future non-`Send`, which Tauri requires (mirrors `import_browser_session`).
    let store = std::sync::Arc::clone(&state.0);
    let scheduler = (*scheduler).clone();

    let result =
        submit_session_key_impl(store.as_ref(), &LiveSessionValidator::new(), &input).await?;
    scheduler.resume_polling();
    Ok(result)
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
    #![allow(clippy::unwrap_used)]

    use super::*;
    use crate::browser_import::{SessionValidator, ValidationError};
    use crate::store::{FakeSessionStore, SessionStore};
    use pretty_assertions::assert_eq;

    const VALID: &str = "sk-ant-sid01-abcDEF123456_-xyz789";

    struct FakeValidator(Result<(), ValidationError>);

    impl SessionValidator for FakeValidator {
        async fn validate<'a>(&'a self, _key: &'a SessionKey) -> Result<(), ValidationError> {
            self.0
        }
    }

    #[tokio::test]
    async fn rejects_input_that_fails_to_parse_without_touching_the_store() {
        let store = FakeSessionStore::new();
        let error = submit_session_key_impl(&store, &FakeValidator(Ok(())), "not-a-key")
            .await
            .unwrap_err();
        assert!(matches!(error, WizardSessionError::Validation(_)));
        assert_eq!(store.load().unwrap(), None);
    }

    #[tokio::test]
    async fn a_valid_confirmed_key_is_stored_and_reported_validated() {
        let store = FakeSessionStore::new();
        let result = submit_session_key_impl(&store, &FakeValidator(Ok(())), VALID)
            .await
            .unwrap();
        assert!(result.validated);
        assert_eq!(store.load().unwrap().unwrap().expose(), VALID);
    }

    #[tokio::test]
    async fn a_transient_failure_keeps_the_key_but_reports_unvalidated() {
        let store = FakeSessionStore::new();
        let result = submit_session_key_impl(
            &store,
            &FakeValidator(Err(ValidationError::Transient)),
            VALID,
        )
        .await
        .unwrap();
        assert!(!result.validated);
        assert_eq!(store.load().unwrap().unwrap().expose(), VALID);
    }

    #[tokio::test]
    async fn a_rejected_key_is_rolled_back_and_never_lingers() {
        let store = FakeSessionStore::new();
        let error = submit_session_key_impl(
            &store,
            &FakeValidator(Err(ValidationError::Unauthorized)),
            VALID,
        )
        .await
        .unwrap_err();
        assert!(matches!(error, WizardSessionError::Rejected(_)));
        assert_eq!(store.load().unwrap(), None);
    }

    #[tokio::test]
    async fn a_rejected_key_restores_whatever_was_previously_stored() {
        let previous = SessionKey::parse("sk-ant-sid01-previousKEY_123-456789").unwrap();
        let store = FakeSessionStore::with_key(previous.clone());
        let error = submit_session_key_impl(
            &store,
            &FakeValidator(Err(ValidationError::Unauthorized)),
            VALID,
        )
        .await
        .unwrap_err();
        assert!(matches!(error, WizardSessionError::Rejected(_)));
        assert_eq!(store.load().unwrap(), Some(previous));
    }

    #[tokio::test]
    async fn a_store_failure_surfaces_as_a_store_error() {
        let store = FakeSessionStore::unavailable();
        let error = submit_session_key_impl(&store, &FakeValidator(Ok(())), VALID)
            .await
            .unwrap_err();
        assert!(matches!(error, WizardSessionError::Store(_)));
    }

    #[test]
    fn wizard_session_error_serializes_with_a_discriminant_tag() {
        let error = WizardSessionError::Validation("session key is empty".to_owned());
        let json = serde_json::to_value(&error).unwrap();
        assert_eq!(json["kind"], "Validation");
        assert_eq!(json["message"], "session key is empty");
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
