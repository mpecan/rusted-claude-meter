//! Session-key storage, backed by the platform credential store.
//!
//! macOS uses Keychain Services; Linux uses the Secret Service D-Bus API
//! (gnome-keyring / `KWallet`), both via the `keyring` crate's default `v1`
//! backend set. There is deliberately no plaintext-file fallback for a
//! headless Linux session with no Secret Service daemon running:
//! [`StoreError::Unavailable`] surfaces that case as an explicit, typed
//! error instead of silently degrading security. See the decision record
//! on <https://github.com/mpecan/rusted-claude-meter/issues/1>.

use keyring::Entry;
use meter_core::SessionKey;

const SERVICE: &str = "com.mpecan.rusted-claude-meter";
const USERNAME: &str = "session-key";

/// Persists the claude.ai session key outside the process.
///
/// Implementations must never write the key to a plaintext file or log it;
/// [`SessionKey`]'s own `Debug`/`Display` redaction is the last line of
/// defense if an implementation does log a value in an error path.
pub trait SessionStore: Send + Sync {
    /// Store `key`, replacing any previously stored value.
    fn save(&self, key: &SessionKey) -> Result<(), StoreError>;
    /// Load the stored key, or `Ok(None)` if nothing has been stored yet.
    fn load(&self) -> Result<Option<SessionKey>, StoreError>;
    /// Remove the stored key. Idempotent: clearing an empty store is not an
    /// error.
    fn clear(&self) -> Result<(), StoreError>;
}

/// Errors from the credential-store backend.
///
/// Carries only human-readable summaries produced by the platform layer —
/// never the session key itself — so it is always safe to log or display.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum StoreError {
    /// A value is stored under this entry but does not parse as a session
    /// key (e.g. the service/username pair was reused by something else).
    #[error("stored value is not a valid session key")]
    Corrupt,
    /// The platform credential-store backend could not be reached at all.
    /// On a headless Linux session with no Secret Service daemon
    /// (gnome-keyring / `KWallet`) running, this is the expected failure mode
    /// — there is no automatic fallback to storing the key in plaintext.
    #[error("the OS credential store is unavailable: {0}")]
    Unavailable(String),
}

impl From<keyring::Error> for StoreError {
    fn from(error: keyring::Error) -> Self {
        Self::Unavailable(error.to_string())
    }
}

/// [`SessionStore`] backed by the platform credential store via `keyring`.
#[derive(Debug, Clone, Copy, Default)]
pub struct KeyringSessionStore;

impl KeyringSessionStore {
    /// Not an instance method: the entry identity (service/username) is
    /// fixed, not per-instance state.
    fn entry() -> Result<Entry, StoreError> {
        Entry::new(SERVICE, USERNAME).map_err(StoreError::from)
    }
}

impl SessionStore for KeyringSessionStore {
    fn save(&self, key: &SessionKey) -> Result<(), StoreError> {
        Self::entry()?
            .set_password(key.expose())
            .map_err(StoreError::from)
    }

    fn load(&self) -> Result<Option<SessionKey>, StoreError> {
        match Self::entry()?.get_password() {
            Ok(raw) => SessionKey::parse(&raw)
                .map(Some)
                .map_err(|_| StoreError::Corrupt),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(other) => Err(StoreError::from(other)),
        }
    }

    fn clear(&self) -> Result<(), StoreError> {
        match Self::entry()?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(other) => Err(StoreError::from(other)),
        }
    }
}

/// In-memory [`SessionStore`] for tests: no OS credential store involved.
///
/// `#[cfg(test)]`-only: this is test scaffolding, not part of the app's
/// runtime surface.
#[cfg(test)]
#[derive(Debug, Default)]
pub struct FakeSessionStore {
    slot: std::sync::Mutex<Option<SessionKey>>,
}

#[cfg(test)]
impl FakeSessionStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Build a fake that already has `key` stored, for exercising `load`
    /// without going through `save` first.
    pub const fn with_key(key: SessionKey) -> Self {
        Self {
            slot: std::sync::Mutex::new(Some(key)),
        }
    }

    /// Force the next call into `Err(StoreError::Unavailable(..))`, to
    /// simulate a missing Secret Service daemon or a locked Keychain.
    pub const fn unavailable() -> UnavailableSessionStore {
        UnavailableSessionStore
    }
}

#[cfg(test)]
impl SessionStore for FakeSessionStore {
    fn save(&self, key: &SessionKey) -> Result<(), StoreError> {
        *self
            .slot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(key.clone());
        Ok(())
    }

    fn load(&self) -> Result<Option<SessionKey>, StoreError> {
        Ok(self
            .slot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone())
    }

    fn clear(&self) -> Result<(), StoreError> {
        *self
            .slot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
        Ok(())
    }
}

/// [`SessionStore`] that always fails, simulating a backend with no daemon
/// reachable (e.g. headless Linux with no Secret Service).
#[cfg(test)]
#[derive(Debug, Clone, Copy, Default)]
pub struct UnavailableSessionStore;

#[cfg(test)]
impl SessionStore for UnavailableSessionStore {
    fn save(&self, _key: &SessionKey) -> Result<(), StoreError> {
        Err(StoreError::Unavailable("no backend for tests".to_owned()))
    }

    fn load(&self) -> Result<Option<SessionKey>, StoreError> {
        Err(StoreError::Unavailable("no backend for tests".to_owned()))
    }

    fn clear(&self) -> Result<(), StoreError> {
        Err(StoreError::Unavailable("no backend for tests".to_owned()))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use pretty_assertions::assert_eq;

    const VALID: &str = "sk-ant-sid01-abcDEF123456_-xyz789";

    #[test]
    fn fake_round_trips_save_load_clear() {
        let store = FakeSessionStore::new();
        assert_eq!(store.load().unwrap(), None);

        let key = SessionKey::parse(VALID).unwrap();
        store.save(&key).unwrap();
        assert_eq!(store.load().unwrap(), Some(key));

        store.clear().unwrap();
        assert_eq!(store.load().unwrap(), None);
    }

    #[test]
    fn fake_clear_is_idempotent() {
        let store = FakeSessionStore::new();
        store.clear().unwrap();
        store.clear().unwrap();
        assert_eq!(store.load().unwrap(), None);
    }

    #[test]
    fn fake_with_key_preloads_state() {
        let key = SessionKey::parse(VALID).unwrap();
        let store = FakeSessionStore::with_key(key.clone());
        assert_eq!(store.load().unwrap(), Some(key));
    }

    #[test]
    fn unavailable_store_surfaces_backend_error() {
        let store = FakeSessionStore::unavailable();
        assert_eq!(
            store.load(),
            Err(StoreError::Unavailable("no backend for tests".to_owned()))
        );
    }

    #[test]
    fn store_error_never_contains_the_raw_key() {
        let error = StoreError::Unavailable("no Secret Service daemon".to_owned());
        assert!(!error.to_string().contains(VALID));
        assert!(!format!("{error:?}").contains(VALID));
    }
}
