//! Tauri commands for session-key management.
//!
//! Each `#[tauri::command]` is a thin adapter over a pure, directly
//! unit-testable function that talks to a `&dyn SessionStore` — no
//! `tauri::State` construction required in tests — so parse-error and
//! store-error mapping is covered without spinning up the Tauri runtime.

use std::sync::Arc;

use meter_core::{SessionKey, SessionKeyError};
use serde::Serialize;
use tauri::State;

use crate::scheduler::{MeterState, RefreshInterval, SchedulerHandle};
use crate::store::{SessionStore, StoreError};

/// Managed Tauri state wrapping the active [`SessionStore`].
pub struct SessionStoreState(pub Arc<dyn SessionStore>);

/// Errors returned to the frontend by session commands.
///
/// Carries only human-readable summaries — never the session key — and
/// distinguishes input-validation failures (fixable by the user re-typing)
/// from backend failures (fixable by, e.g., unlocking the Keychain).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", content = "message")]
pub enum SessionCommandError {
    /// The pasted input failed [`SessionKey::parse`].
    Validation(String),
    /// The credential-store backend failed.
    Store(String),
}

impl From<SessionKeyError> for SessionCommandError {
    fn from(error: SessionKeyError) -> Self {
        Self::Validation(error.to_string())
    }
}

impl From<StoreError> for SessionCommandError {
    fn from(error: StoreError) -> Self {
        Self::Store(error.to_string())
    }
}

/// Whether a session key is currently stored, without exposing it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    Present,
    Absent,
}

fn set_session_key_impl(store: &dyn SessionStore, input: &str) -> Result<(), SessionCommandError> {
    let key = SessionKey::parse(input)?;
    store.save(&key)?;
    Ok(())
}

fn session_status_impl(store: &dyn SessionStore) -> Result<SessionStatus, SessionCommandError> {
    Ok(if store.load()?.is_some() {
        SessionStatus::Present
    } else {
        SessionStatus::Absent
    })
}

fn clear_session_key_impl(store: &dyn SessionStore) -> Result<(), SessionCommandError> {
    store.clear()?;
    Ok(())
}

/// Parse and store a pasted session key (raw `sk-ant-...` value or a full
/// `Cookie` header containing `sessionKey=...`), then wake the polling loop
/// so a scheduler parked on "session expired" retries with the new key.
///
/// `State` and `String` are required by value here: they are Tauri's
/// command-extractor types, not a choice this function makes.
#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
pub fn set_session_key(
    state: State<'_, SessionStoreState>,
    scheduler: State<'_, SchedulerHandle>,
    input: String,
) -> Result<(), SessionCommandError> {
    set_session_key_impl(state.0.as_ref(), &input)?;
    scheduler.resume_polling();
    Ok(())
}

/// Report whether a session key is currently stored.
#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
pub fn session_status(
    state: State<'_, SessionStoreState>,
) -> Result<SessionStatus, SessionCommandError> {
    session_status_impl(state.0.as_ref())
}

/// Remove the stored session key and tell the scheduler directly, so the
/// broadcast state flips to "awaiting session" immediately instead of on
/// the next scheduled tick.
#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
pub fn clear_session_key(
    state: State<'_, SessionStoreState>,
    scheduler: State<'_, SchedulerHandle>,
) -> Result<(), SessionCommandError> {
    clear_session_key_impl(state.0.as_ref())?;
    scheduler.mark_no_session();
    Ok(())
}

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

/// Change the polling cadence (60 / 300 / 600 seconds).
#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
pub fn set_refresh_interval(scheduler: State<'_, SchedulerHandle>, interval: RefreshInterval) {
    scheduler.set_interval(interval);
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use crate::store::FakeSessionStore;
    use pretty_assertions::assert_eq;

    const VALID: &str = "sk-ant-sid01-abcDEF123456_-xyz789";

    #[test]
    fn set_session_key_rejects_invalid_input() {
        let store = FakeSessionStore::new();
        let result = set_session_key_impl(&store, "not-a-key");
        assert_eq!(
            result,
            Err(SessionCommandError::Validation(
                SessionKeyError::MissingPrefix.to_string()
            ))
        );
    }

    #[test]
    fn set_session_key_maps_every_parse_error_variant() {
        let store = FakeSessionStore::new();
        assert_eq!(
            set_session_key_impl(&store, ""),
            Err(SessionCommandError::Validation(
                SessionKeyError::Empty.to_string()
            ))
        );
        assert_eq!(
            set_session_key_impl(&store, "sk-ant-short"),
            Err(SessionCommandError::Validation(
                SessionKeyError::TooShort.to_string()
            ))
        );
        assert_eq!(
            set_session_key_impl(&store, "sk-ant-sid01-abc DEF123456789"),
            Err(SessionCommandError::Validation(
                SessionKeyError::InvalidCharacters.to_string()
            ))
        );
    }

    #[test]
    fn set_session_key_stores_a_valid_key() {
        let store = FakeSessionStore::new();
        set_session_key_impl(&store, VALID).unwrap();
        assert_eq!(store.load().unwrap().unwrap().expose(), VALID);
    }

    #[test]
    fn set_session_key_never_leaks_the_raw_value_in_its_error() {
        // A valid-looking prefix but invalid body triggers a validation
        // error; assert the offending raw text never round-trips through it.
        let bad = "sk-ant-sid01-abc DEF123456789";
        let error = set_session_key_impl(&FakeSessionStore::new(), bad).unwrap_err();
        assert!(!format!("{error:?}").contains("abc DEF"));
    }

    #[test]
    fn set_session_key_surfaces_store_errors() {
        let store = FakeSessionStore::unavailable();
        let result = set_session_key_impl(&store, VALID);
        assert!(matches!(result, Err(SessionCommandError::Store(_))));
    }

    #[test]
    fn session_status_reports_absent_then_present() {
        let store = FakeSessionStore::new();
        assert_eq!(session_status_impl(&store).unwrap(), SessionStatus::Absent);
        set_session_key_impl(&store, VALID).unwrap();
        assert_eq!(session_status_impl(&store).unwrap(), SessionStatus::Present);
    }

    #[test]
    fn session_status_surfaces_store_errors() {
        let store = FakeSessionStore::unavailable();
        assert!(matches!(
            session_status_impl(&store),
            Err(SessionCommandError::Store(_))
        ));
    }

    #[test]
    fn clear_session_key_removes_a_stored_key() {
        let key = SessionKey::parse(VALID).unwrap();
        let store = FakeSessionStore::with_key(key);
        clear_session_key_impl(&store).unwrap();
        assert_eq!(session_status_impl(&store).unwrap(), SessionStatus::Absent);
    }

    #[test]
    fn clear_session_key_is_idempotent_when_nothing_is_stored() {
        let store = FakeSessionStore::new();
        clear_session_key_impl(&store).unwrap();
        clear_session_key_impl(&store).unwrap();
    }

    #[test]
    fn clear_session_key_surfaces_store_errors() {
        let store = FakeSessionStore::unavailable();
        assert!(matches!(
            clear_session_key_impl(&store),
            Err(SessionCommandError::Store(_))
        ));
    }

    #[test]
    fn command_error_serializes_with_a_discriminant_tag() {
        let error = SessionCommandError::Validation("session key is empty".to_owned());
        let json = serde_json::to_value(&error).unwrap();
        assert_eq!(json["kind"], "Validation");
        assert_eq!(json["message"], "session key is empty");
    }
}
