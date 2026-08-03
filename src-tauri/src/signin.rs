//! The shared sign-in primitive: persist a session key, confirm it with
//! claude.ai, and roll back if claude.ai rejects it.
//!
//! Lives here rather than in `browser_import` because it is not browser
//! specific — both user-driven sign-in paths go through it, the browser import
//! (`commands::browser`) and a pasted key (`commands::set_session_key`) — and
//! because that makes it the one chokepoint where *both* gates on claude.ai
//! traffic can be enforced for both paths at once (see [`SessionSink`]): the
//! Terms-of-Service consent gate (`crate::consent`) and the selected usage
//! source (`crate::source`). Validating a key is a claude.ai request, so it is
//! subject to exactly the same rules as a poll.

use std::sync::Arc;

use meter_core::SessionKey;

use crate::browser_import::{SessionValidator, ValidationError};
use crate::consent::ConsentGate;
use crate::source::{self, SourceSelection};
use crate::store::{SessionStore, run_store_op};

/// What every sign-in path needs: where the key is kept, what confirms it
/// with claude.ai, and the two independent answers to "may we contact
/// claude.ai at all?" — has the user accepted the risk, and are they even
/// polling claude.ai.
///
/// Bundled rather than passed separately so callers stay under the
/// workspace's `too_many_arguments` limit (same move as
/// `scheduler::PersistPaths`), and — the reason it is worth having — so both
/// gates are a *mandatory* part of constructing a sign-in rather than checks
/// each caller has to remember to write.
pub struct SessionSink<'a, V: SessionValidator> {
    pub store: &'a Arc<dyn SessionStore>,
    pub validator: &'a V,
    pub consent: &'a ConsentGate,
    /// Set when the Claude Code status line is the source. A key is neither
    /// needed nor usable then, and validating one would be the very claude.ai
    /// request that source promises not to make.
    pub source: &'a SourceSelection,
}

impl<'a, V: SessionValidator> SessionSink<'a, V> {
    #[must_use]
    pub const fn new(
        store: &'a Arc<dyn SessionStore>,
        validator: &'a V,
        consent: &'a ConsentGate,
        source: &'a SourceSelection,
    ) -> Self {
        Self {
            store,
            validator,
            consent,
            source,
        }
    }
}

/// Outcome of [`store_and_validate`] when the key could not be kept.
///
/// Deliberately generic over *why* the caller wanted the key stored (a
/// browser import here, a pasted key in the setup wizard — issue #11) so
/// both paths share one rollback implementation instead of duplicating it.
pub enum StoreAndValidateError {
    /// The credential store refused to persist the key.
    Store(String),
    /// Terms-of-Service consent is withheld, so nothing was read, stored or
    /// contacted (`crate::consent`).
    NotAcknowledged,
    /// The Claude Code status line is the selected source, so a session key is
    /// neither needed nor usable and nothing was read, stored or contacted
    /// (`crate::source`).
    WrongSource,
    /// claude.ai rejected the key (401): it is expired or otherwise invalid.
    /// The previously stored key (if any) has already been restored by the
    /// time this is returned.
    Rejected,
}

/// Persist `key`, then confirm it with claude.ai. A rejection (401) is
/// rolled back: the key that was stored before this call is restored (or the
/// store cleared if there was none), so a failed validation never destroys a
/// working session and an invalid key never lingers. A network hiccup keeps
/// the key — it might still be good, and the scheduler validates it on its
/// next poll — and resolves to `Ok(false)` ("stored, not yet confirmed").
///
/// Every store touch goes through [`run_store_op`]'s blocking pool: the
/// credential store is a synchronous OS round trip that must not occupy an
/// async worker thread.
///
/// Both gates live here rather than in each caller: this is the single point
/// both user-driven sign-in paths (pasted key, browser import) pass through
/// before contacting claude.ai, so checking them here makes them unskippable
/// by construction instead of a convention every future caller has to
/// remember. Together with `SourcedTransport`'s dispatch, that accounts for
/// every line in this app that can reach claude.ai.
pub async fn store_and_validate<V: SessionValidator>(
    sink: &SessionSink<'_, V>,
    key: &SessionKey,
) -> Result<bool, StoreAndValidateError> {
    let SessionSink {
        store,
        validator,
        consent,
        source,
    } = sink;
    // Both before any store I/O, so a refusal costs neither claude.ai traffic
    // nor a keychain prompt.
    //
    // Source first: to someone reading from Claude Code, "you do not need a
    // key" is the useful answer, and whether they ever accepted the
    // claude.ai risk is beside the point.
    if !source::selected(source).reaches_claude_ai() {
        return Err(StoreAndValidateError::WrongSource);
    }
    if !consent.get() {
        return Err(StoreAndValidateError::NotAcknowledged);
    }
    // Hold on to whatever key was stored before, so a rejection can put it
    // back instead of destroying a working session. Best-effort: if the
    // store can't be read, there is nothing to restore.
    let previous = run_store_op(store, |s| s.load()).await.unwrap_or_default();

    let to_save = key.clone();
    run_store_op(store, move |s| s.save(&to_save))
        .await
        .map_err(|error| StoreAndValidateError::Store(error.to_string()))?;

    match validator.validate(key).await {
        Ok(()) => Ok(true),
        Err(ValidationError::Unauthorized) => {
            // Best-effort: don't leave a rejected key behind.
            let _ = run_store_op(store, move |s| {
                previous
                    .as_ref()
                    .map_or_else(|| s.clear(), |previous_key| s.save(previous_key))
            })
            .await;
            Err(StoreAndValidateError::Rejected)
        }
        Err(ValidationError::Transient) => Ok(false),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::{SessionSink, StoreAndValidateError, store_and_validate};
    use crate::browser_import::{SessionValidator, ValidationError};
    use crate::consent::ConsentGate;
    use crate::store::{FakeSessionStore, SessionStore};
    use meter_core::SessionKey;
    use std::sync::Arc;

    const VALID: &str = "sk-ant-sid01-abcDEF123456_-xyz789";

    struct FakeValidator(Result<(), ValidationError>);

    impl SessionValidator for FakeValidator {
        async fn validate<'a>(&'a self, _key: &'a SessionKey) -> Result<(), ValidationError> {
            self.0
        }
    }

    static OPEN_GATE: ConsentGate = ConsentGate::new(true);
    static CLOSED_GATE: ConsentGate = ConsentGate::new(false);
    use crate::source::{SourceSelection, UsageSource, selection};

    static POLLING_CLAUDE_AI: SourceSelection = selection(UsageSource::ClaudeAi);
    static READING_CLAUDE_CODE: SourceSelection = selection(UsageSource::ClaudeCodeStatusline);

    fn key() -> SessionKey {
        SessionKey::parse(VALID).unwrap()
    }

    #[tokio::test]
    async fn withheld_consent_refuses_before_any_store_touch() {
        // The property the whole consent gate rests on, asserted at the one
        // place both sign-in paths share: nothing is persisted and the
        // validator — the thing that would call claude.ai — never runs.
        let store: Arc<dyn SessionStore> = Arc::new(FakeSessionStore::new());
        let validator = FakeValidator(Ok(()));
        let error = store_and_validate(
            &SessionSink {
                store: &store,
                validator: &validator,
                consent: &CLOSED_GATE,
                source: &POLLING_CLAUDE_AI,
            },
            &key(),
        )
        .await
        .err();

        assert!(matches!(
            error,
            Some(StoreAndValidateError::NotAcknowledged)
        ));
        assert_eq!(
            store.load().unwrap(),
            None,
            "a refused sign-in must not leave a key behind"
        );
    }

    /// The same property for the other gate: reading from Claude Code means
    /// no key is needed, so nothing is persisted and the validator — the
    /// thing that would call claude.ai — never runs.
    #[tokio::test]
    async fn the_claude_code_source_refuses_before_any_store_touch() {
        let store: Arc<dyn SessionStore> = Arc::new(FakeSessionStore::new());
        let validator = FakeValidator(Ok(()));
        let error = store_and_validate(
            &SessionSink {
                store: &store,
                validator: &validator,
                consent: &OPEN_GATE,
                source: &READING_CLAUDE_CODE,
            },
            &key(),
        )
        .await
        .err();

        assert!(matches!(error, Some(StoreAndValidateError::WrongSource)));
        assert_eq!(
            store.load().unwrap(),
            None,
            "a refused sign-in must not leave a key behind"
        );
    }

    /// Source is checked first: to someone reading from Claude Code, "you do
    /// not need a key" is the useful answer, and whether they ever accepted
    /// the claude.ai risk is beside the point.
    #[tokio::test]
    async fn the_source_refusal_takes_precedence_over_the_consent_one() {
        let store: Arc<dyn SessionStore> = Arc::new(FakeSessionStore::new());
        let validator = FakeValidator(Ok(()));
        let error = store_and_validate(
            &SessionSink {
                store: &store,
                validator: &validator,
                consent: &CLOSED_GATE,
                source: &READING_CLAUDE_CODE,
            },
            &key(),
        )
        .await
        .err();

        assert!(matches!(error, Some(StoreAndValidateError::WrongSource)));
    }

    #[tokio::test]
    async fn an_open_gate_stores_and_confirms() {
        let store: Arc<dyn SessionStore> = Arc::new(FakeSessionStore::new());
        let validator = FakeValidator(Ok(()));
        let validated = store_and_validate(
            &SessionSink {
                store: &store,
                validator: &validator,
                consent: &OPEN_GATE,
                source: &POLLING_CLAUDE_AI,
            },
            &key(),
        )
        .await
        .ok();

        assert_eq!(validated, Some(true));
        assert_eq!(store.load().unwrap().unwrap().expose(), VALID);
    }
}
