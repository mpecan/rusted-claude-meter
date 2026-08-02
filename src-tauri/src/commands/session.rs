//! Session-key commands: store a pasted key, report whether one is held, and
//! clear it.
//!
//! Split out of `commands::mod` for the file-size gate, and because these are
//! the commands that can *reach claude.ai* — storing a key validates it — so
//! they are the ones subject to `crate::signin`'s two gates. Everything here
//! is a thin adapter over a directly unit-testable function taking a
//! `dyn SessionStore`, so error mapping is covered without a Tauri runtime.
//!
//! Two invariants every session command honors:
//!
//! * A pasted key is **validated against claude.ai** before it is allowed to
//!   stick, with rollback on rejection (`signin::store_and_validate` — the
//!   same guarantee browser import and the wizard give).
//! * Credential-store I/O never runs on the UI thread: the commands are
//!   `async` and route Keychain / Secret-Service calls through
//!   [`run_store_op`]'s blocking pool, so a slow or stuck credential daemon
//!   can never freeze tray or window redraws.

use std::sync::Arc;

use meter_core::{SessionKey, SessionKeyError};
use serde::Serialize;
use tauri::State;

use crate::browser_import::{LiveSessionValidator, SessionValidator};
use crate::commands::{SessionStoreState, consent};
use crate::consent::ConsentGate;
use crate::scheduler::{FetchOutcome, SchedulerHandle};
use crate::signin::{SessionSink, StoreAndValidateError, store_and_validate};
use crate::source::{SourceSelection, WRONG_SOURCE_MESSAGE};
use crate::store::{SessionStore, StoreError, run_store_op};

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
    /// The key parsed but claude.ai rejected it (expired/invalid). The
    /// previously stored key, if any, has already been restored.
    Rejected(String),
    /// The credential-store backend failed.
    Store(String),
    /// Terms-of-Service consent is withheld, so validating the key against claude.ai —
    /// which is a request to claude.ai — was refused (`crate::consent`).
    NotAcknowledged(String),
    /// The Claude Code status line is the selected source, so no key is needed
    /// and none was stored — validating one would be a claude.ai request that
    /// source promises not to make (`crate::source`).
    WrongSource(String),
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

/// Outcome of a validated session-key submission. Shared by every entry
/// point that accepts a pasted key — the popover's inline field, the
/// Settings panel field, and the wizard's paste step all go through
/// [`set_session_key`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct SessionSubmission {
    /// Whether claude.ai confirmed the key. `false` means the key is stored
    /// but validation was skipped because claude.ai was unreachable — the
    /// scheduler validates it on its next poll (mirrors
    /// `browser_import::ImportSummary::validated`).
    pub validated: bool,
}

/// Parse, persist and validate a pasted session key via
/// [`store_and_validate`], so a pasted key gets the exact same
/// rollback-on-rejection guarantee an imported one does (issues #10/#11): a
/// key claude.ai rejects never clobbers a previously working one.
async fn submit_session_key_impl<V: SessionValidator>(
    sink: &SessionSink<'_, V>,
    input: &str,
) -> Result<SessionSubmission, SessionCommandError> {
    let key = SessionKey::parse(input)?;
    let validated = store_and_validate(sink, &key)
        .await
        .map_err(|error| match error {
            StoreAndValidateError::Store(message) => SessionCommandError::Store(message),
            StoreAndValidateError::NotAcknowledged => {
                SessionCommandError::NotAcknowledged(consent::WITHHELD_MESSAGE.to_owned())
            }
            StoreAndValidateError::WrongSource => {
                SessionCommandError::WrongSource(WRONG_SOURCE_MESSAGE.to_owned())
            }
            StoreAndValidateError::Rejected => SessionCommandError::Rejected(
                "claude.ai rejected that session key — it may be expired. Sign in to claude.ai \
             again, copy a fresh key, and try again."
                    .to_owned(),
            ),
        })?;
    Ok(SessionSubmission { validated })
}

fn session_status_impl(store: &dyn SessionStore) -> Result<SessionStatus, StoreError> {
    Ok(if store.load()?.is_some() {
        SessionStatus::Present
    } else {
        SessionStatus::Absent
    })
}

/// Parse, store and **validate** a pasted session key (raw `sk-ant-...`
/// value or a full `Cookie` header containing `sessionKey=...`) against
/// claude.ai, rolling back to the previously stored key if claude.ai
/// rejects it, then wake the polling loop so a scheduler parked on "session
/// expired" retries with the new key.
///
/// `State` and `String` are required by value here: they are Tauri's
/// command-extractor types, not a choice this function makes.
#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
pub async fn set_session_key(
    state: State<'_, SessionStoreState>,
    scheduler: State<'_, SchedulerHandle>,
    consent: State<'_, Arc<ConsentGate>>,
    selection: State<'_, Arc<SourceSelection>>,
    input: String,
) -> Result<SessionSubmission, SessionCommandError> {
    // Owned handles, so nothing borrowed from the `State` guards is held
    // across the await (mirrors `import_browser_session`).
    let store = Arc::clone(&state.0);
    let scheduler = (*scheduler).clone();
    let consent = Arc::clone(&consent);
    let selection = Arc::clone(&selection);

    // Both gates on claude.ai traffic are enforced inside `store_and_validate`,
    // which this goes through — not re-checked here (see `crate::signin`).
    let validator = LiveSessionValidator::new();
    let sink = SessionSink::new(&store, &validator, &consent, &selection);
    let submission = submit_session_key_impl(&sink, &input).await?;
    scheduler.resume_polling();
    Ok(submission)
}

/// Report whether a session key is currently stored. Async and routed
/// through the blocking pool: the credential-store round trip must never
/// run on the UI thread (see [`run_store_op`]).
#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
pub async fn session_status(
    state: State<'_, SessionStoreState>,
) -> Result<SessionStatus, SessionCommandError> {
    Ok(run_store_op(&state.0, session_status_impl).await?)
}

/// Remove the stored session key and tell the scheduler directly, so the
/// broadcast state flips to "awaiting session" immediately instead of on
/// the next scheduled tick.
#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
pub async fn clear_session_key(
    state: State<'_, SessionStoreState>,
    scheduler: State<'_, SchedulerHandle>,
) -> Result<(), SessionCommandError> {
    run_store_op(&state.0, |store| store.clear()).await?;
    scheduler.update(|core| core.record(FetchOutcome::NoSession));
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use crate::browser_import::ValidationError;
    use crate::source::SourceSelection;
    use crate::store::FakeSessionStore;
    use pretty_assertions::assert_eq;

    const VALID: &str = "sk-ant-sid01-abcDEF123456_-xyz789";

    static OPEN_GATE: ConsentGate = ConsentGate::new(true);

    /// The claude.ai source. Named rather than a bare `false` so every sink
    /// in these tests states which source it is standing in for.
    static POLLING_CLAUDE_AI: SourceSelection = SourceSelection::new(false);
    static READING_CLAUDE_CODE: SourceSelection = SourceSelection::new(true);

    struct FakeValidator(Result<(), ValidationError>);

    impl SessionValidator for FakeValidator {
        async fn validate<'a>(&'a self, _key: &'a SessionKey) -> Result<(), ValidationError> {
            self.0
        }
    }

    fn ok_validator() -> FakeValidator {
        FakeValidator(Ok(()))
    }

    fn empty_store() -> Arc<dyn SessionStore> {
        Arc::new(FakeSessionStore::new())
    }

    async fn submit(
        store: &Arc<dyn SessionStore>,
        validator: &FakeValidator,
        input: &str,
    ) -> Result<SessionSubmission, SessionCommandError> {
        submit_session_key_impl(
            &SessionSink::new(store, validator, &OPEN_GATE, &POLLING_CLAUDE_AI),
            input,
        )
        .await
    }

    /// The pasted-key path's half of the guarantee, and the one a user is
    /// most likely to trip: the Session field is still on screen, so pasting
    /// into it must refuse rather than quietly make the claude.ai request the
    /// Claude Code source promises not to make.
    #[tokio::test]
    async fn a_pasted_key_is_refused_while_reading_from_claude_code() {
        let store: Arc<dyn SessionStore> = Arc::new(FakeSessionStore::new());
        let validator = ok_validator();
        let error = submit_session_key_impl(
            &SessionSink::new(&store, &validator, &OPEN_GATE, &READING_CLAUDE_CODE),
            VALID,
        )
        .await
        .err();

        assert!(matches!(error, Some(SessionCommandError::WrongSource(_))));
        assert_eq!(
            store.load().unwrap(),
            None,
            "a refused paste must not leave a key behind"
        );
    }

    #[tokio::test]
    async fn set_session_key_rejects_invalid_input_without_touching_the_store() {
        let store = empty_store();
        let result = submit(&store, &ok_validator(), "not-a-key").await;
        assert_eq!(
            result,
            Err(SessionCommandError::Validation(
                SessionKeyError::MissingPrefix.to_string()
            ))
        );
        assert_eq!(store.load().unwrap(), None);
    }

    #[tokio::test]
    async fn set_session_key_maps_every_parse_error_variant() {
        let store = empty_store();
        assert_eq!(
            submit(&store, &ok_validator(), "").await,
            Err(SessionCommandError::Validation(
                SessionKeyError::Empty.to_string()
            ))
        );
        assert_eq!(
            submit(&store, &ok_validator(), "sk-ant-short").await,
            Err(SessionCommandError::Validation(
                SessionKeyError::TooShort.to_string()
            ))
        );
        assert_eq!(
            submit(&store, &ok_validator(), "sk-ant-sid01-abc DEF123456789").await,
            Err(SessionCommandError::Validation(
                SessionKeyError::InvalidCharacters.to_string()
            ))
        );
    }

    #[tokio::test]
    async fn set_session_key_stores_a_confirmed_key_and_reports_validated() {
        let store = empty_store();
        let submission = submit(&store, &ok_validator(), VALID).await.unwrap();
        assert!(submission.validated);
        assert_eq!(store.load().unwrap().unwrap().expose(), VALID);
    }

    #[tokio::test]
    async fn set_session_key_keeps_an_unconfirmed_key_on_a_transient_failure() {
        let store = empty_store();
        let submission = submit(
            &store,
            &FakeValidator(Err(ValidationError::Transient)),
            VALID,
        )
        .await
        .unwrap();
        assert!(!submission.validated);
        // Kept, because the failure might be a network blip, not a bad key.
        assert_eq!(store.load().unwrap().unwrap().expose(), VALID);
    }

    #[tokio::test]
    async fn set_session_key_rolls_back_a_rejected_key() {
        let store = empty_store();
        let error = submit(
            &store,
            &FakeValidator(Err(ValidationError::Unauthorized)),
            VALID,
        )
        .await
        .unwrap_err();
        assert!(matches!(error, SessionCommandError::Rejected(_)));
        // The rejected key must not linger.
        assert_eq!(store.load().unwrap(), None);
    }

    #[tokio::test]
    async fn set_session_key_restores_the_previously_stored_key_on_rejection() {
        let previous = SessionKey::parse("sk-ant-sid01-previousKEY_123-456789").unwrap();
        let store: Arc<dyn SessionStore> = Arc::new(FakeSessionStore::with_key(previous.clone()));
        let error = submit(
            &store,
            &FakeValidator(Err(ValidationError::Unauthorized)),
            VALID,
        )
        .await
        .unwrap_err();
        assert!(matches!(error, SessionCommandError::Rejected(_)));
        // A bad paste must never clobber a working key.
        assert_eq!(store.load().unwrap(), Some(previous));
    }

    #[tokio::test]
    async fn set_session_key_never_leaks_the_raw_value_in_its_error() {
        // A valid-looking prefix but invalid body triggers a validation
        // error; assert the offending raw text never round-trips through it.
        let bad = "sk-ant-sid01-abc DEF123456789";
        let error = submit(&empty_store(), &ok_validator(), bad)
            .await
            .unwrap_err();
        assert!(!format!("{error:?}").contains("abc DEF"));
    }

    #[tokio::test]
    async fn set_session_key_surfaces_store_errors() {
        let store: Arc<dyn SessionStore> = Arc::new(FakeSessionStore::unavailable());
        let result = submit(&store, &ok_validator(), VALID).await;
        assert!(matches!(result, Err(SessionCommandError::Store(_))));
    }

    #[test]
    fn session_status_reports_absent_then_present() {
        let store = FakeSessionStore::new();
        assert_eq!(session_status_impl(&store).unwrap(), SessionStatus::Absent);
        store.save(&SessionKey::parse(VALID).unwrap()).unwrap();
        assert_eq!(session_status_impl(&store).unwrap(), SessionStatus::Present);
    }

    #[test]
    fn session_status_surfaces_store_errors() {
        let store = FakeSessionStore::unavailable();
        assert!(matches!(
            session_status_impl(&store),
            Err(StoreError::Unavailable(_))
        ));
    }

    // `clear_session_key` delegates straight to `SessionStore::clear` via
    // `run_store_op`; clearing behaviour (including idempotence and backend
    // errors) is pinned by `store.rs`'s own tests.

    #[test]
    fn command_error_serializes_with_a_discriminant_tag() {
        let error = SessionCommandError::Validation("session key is empty".to_owned());
        let json = serde_json::to_value(&error).unwrap();
        assert_eq!(json["kind"], "Validation");
        assert_eq!(json["message"], "session key is empty");

        let rejected = SessionCommandError::Rejected("rejected".to_owned());
        let json = serde_json::to_value(&rejected).unwrap();
        assert_eq!(json["kind"], "Rejected");
    }

    #[test]
    fn session_submission_serializes_the_validated_flag() {
        let json = serde_json::to_value(SessionSubmission { validated: false }).unwrap();
        assert_eq!(json["validated"], false);
    }
}
