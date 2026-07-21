//! Session-key storage, backed by the platform credential store.
//!
//! macOS uses Keychain Services; Linux uses the Secret Service D-Bus API
//! (gnome-keyring / `KWallet`), both driven directly through `keyring-core`
//! rather than the `keyring` crate's `v1` convenience shim. The shim caches
//! backend-construction success/failure in a process-wide `AtomicBool` that
//! is set *before* the platform store is actually built: if the Secret
//! Service daemon isn't reachable yet on the first call (a real race on
//! login/autostart or after a sleep/wake D-Bus renegotiation), the shim
//! never retries for the rest of the process's life, even once the daemon
//! comes up. Driving `keyring-core` ourselves and re-attempting
//! [`keyring_core::set_default_store`] whenever no default store is set lets
//! a later retry recover once the daemon becomes reachable. There is
//! deliberately no plaintext-file fallback for a headless Linux session with
//! no Secret Service daemon running: [`StoreError::Unavailable`] surfaces
//! that case as an explicit, typed error instead of silently degrading
//! security. See the decision record on
//! <https://github.com/mpecan/rusted-claude-meter/issues/1>.

use keyring_core::Entry;
use meter_core::SessionKey;
use std::sync::Arc;
use zeroize::Zeroize;

// Keychain/Secret-Service item key. Must match the app's bundle identifier so
// silent, no-re-prompt access works (the item's ACL trusts the designated
// requirement anchored to Team ID + bundle ID). The `browser-import` feature
// is the marker for which variant this is: the full build ships as
// `com.mpecan.rusted-claude-meter`, the lite build (no browser import) as
// `com.mpecan.rusted-claude-meter-lite` — so each stores its own key and the
// two never collide on a machine that has both.
#[cfg(feature = "browser-import")]
const SERVICE: &str = "com.mpecan.rusted-claude-meter";
#[cfg(not(feature = "browser-import"))]
const SERVICE: &str = "com.mpecan.rusted-claude-meter-lite";
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

impl From<keyring_core::Error> for StoreError {
    fn from(error: keyring_core::Error) -> Self {
        Self::Unavailable(error.to_string())
    }
}

/// Builds this platform's `keyring-core` credential store backend.
///
/// macOS uses Keychain Services via `apple-native-keyring-store`.
#[cfg(target_os = "macos")]
fn build_platform_store() -> keyring_core::Result<Arc<keyring_core::CredentialStore>> {
    apple_native_keyring_store::keychain::Store::new()
        .map(|store| store as Arc<keyring_core::CredentialStore>)
}

/// Builds this platform's `keyring-core` credential store backend.
///
/// Non-macOS Unix (i.e. Linux, per this app's supported platforms) uses the
/// Secret Service D-Bus API via `zbus-secret-service-keyring-store`.
#[cfg(all(unix, not(target_os = "macos")))]
fn build_platform_store() -> keyring_core::Result<Arc<keyring_core::CredentialStore>> {
    zbus_secret_service_keyring_store::Store::new()
        .map(|store| store as Arc<keyring_core::CredentialStore>)
}

/// Ensures `keyring_core`'s default store is set, (re-)constructing the
/// platform backend if it isn't.
///
/// Unlike the `keyring` crate's `v1::Entry::new`, this retries backend
/// construction on every call that finds no default store installed —
/// including after a previous attempt failed — rather than caching failure
/// forever in a process-wide flag. That matters for a long-lived tray app:
/// a transient failure (Secret Service daemon not yet up at login/autostart,
/// or a D-Bus session renegotiation after sleep/wake) must not permanently
/// poison every later `save`/`load`/`clear` call.
fn ensure_credential_store() -> Result<(), StoreError> {
    if keyring_core::get_default_store().is_some() {
        return Ok(());
    }
    let store = build_platform_store()?;
    keyring_core::set_default_store(store);
    Ok(())
}

/// Run one blocking [`SessionStore`] operation on the async runtime's
/// dedicated blocking pool.
///
/// Keychain / Secret-Service calls are synchronous OS round trips — and per
/// the module docs, the Secret Service daemon may not even be up yet at
/// login/autostart, so a call can genuinely stall. Routing every store call
/// through `spawn_blocking` keeps that latency off both the UI thread (where
/// non-async commands run) and the shared async worker threads (where the
/// scheduler loop and other command futures run).
///
/// A join failure (the blocking task panicking or being cancelled) is mapped
/// to [`StoreError::Unavailable`] — same retryable classification as any
/// other backend failure.
pub async fn run_store_op<T: Send + 'static>(
    store: &Arc<dyn SessionStore>,
    op: impl FnOnce(&dyn SessionStore) -> Result<T, StoreError> + Send + 'static,
) -> Result<T, StoreError> {
    let store = Arc::clone(store);
    tokio::task::spawn_blocking(move || op(store.as_ref()))
        .await
        .unwrap_or_else(|error| {
            Err(StoreError::Unavailable(format!(
                "credential-store task failed: {error}"
            )))
        })
}

/// Maps a raw `get_password` result to a parsed session key.
///
/// Pulled out of [`KeyringSessionStore::load`] so it can be unit-tested
/// without a real OS credential-store backend: `keyring_core::Error`'s
/// variants (`NoEntry`, `NoDefaultStore`, ...) are plain, publicly
/// constructible data with no OS dependency.
fn map_loaded(result: keyring_core::Result<String>) -> Result<Option<SessionKey>, StoreError> {
    match result {
        Ok(mut raw) => {
            let parsed = SessionKey::parse(&raw)
                .map(Some)
                .map_err(|_| StoreError::Corrupt);
            // The transient copy of the credential must not linger in freed
            // heap memory (`SessionKey` itself zeroizes on drop; this is the
            // one owned intermediate on the load path).
            raw.zeroize();
            parsed
        }
        Err(keyring_core::Error::NoEntry) => Ok(None),
        Err(other) => Err(StoreError::from(other)),
    }
}

/// [`SessionStore`] backed by the platform credential store via
/// `keyring-core`.
#[derive(Debug, Clone, Copy, Default)]
pub struct KeyringSessionStore;

impl KeyringSessionStore {
    /// Not an instance method: the entry identity (service/username) is
    /// fixed, not per-instance state.
    fn entry() -> Result<Entry, StoreError> {
        ensure_credential_store()?;
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
        map_loaded(Self::entry()?.get_password())
    }

    fn clear(&self) -> Result<(), StoreError> {
        match Self::entry()?.delete_credential() {
            Ok(()) | Err(keyring_core::Error::NoEntry) => Ok(()),
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

    #[tokio::test]
    async fn run_store_op_round_trips_through_the_blocking_pool() {
        let store: Arc<dyn SessionStore> = Arc::new(FakeSessionStore::new());
        let key = SessionKey::parse(VALID).unwrap();
        run_store_op(&store, move |s| s.save(&key)).await.unwrap();
        let loaded = run_store_op(&store, |s| s.load()).await.unwrap();
        assert_eq!(loaded.unwrap().expose(), VALID);
    }

    #[tokio::test]
    async fn run_store_op_propagates_backend_errors() {
        let store: Arc<dyn SessionStore> = Arc::new(FakeSessionStore::unavailable());
        let result = run_store_op(&store, |s| s.load()).await;
        assert!(matches!(result, Err(StoreError::Unavailable(_))));
    }

    // `map_loaded` is the free-function core of `KeyringSessionStore::load`,
    // pulled out specifically so these four cases can be exercised without a
    // real OS credential-store backend (see its doc comment).

    #[test]
    fn map_loaded_no_entry_is_none() {
        assert_eq!(map_loaded(Err(keyring_core::Error::NoEntry)), Ok(None));
    }

    #[test]
    fn map_loaded_unparseable_value_is_corrupt() {
        assert_eq!(
            map_loaded(Ok("not-a-valid-key".to_owned())),
            Err(StoreError::Corrupt)
        );
    }

    #[test]
    fn map_loaded_valid_value_is_some() {
        let key = SessionKey::parse(VALID).unwrap();
        assert_eq!(map_loaded(Ok(VALID.to_owned())), Ok(Some(key)));
    }

    #[test]
    fn map_loaded_other_backend_error_is_unavailable() {
        let result = map_loaded(Err(keyring_core::Error::NoDefaultStore));
        assert!(matches!(result, Err(StoreError::Unavailable(_))));
    }
}
