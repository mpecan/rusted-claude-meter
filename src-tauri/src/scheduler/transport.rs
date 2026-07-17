//! The network side of the scheduler: one refresh attempt end to end.
//!
//! [`UsageTransport`] is the seam the polling loop is generic over, so tests
//! drive the loop with fakes and never touch the network. [`LiveTransport`]
//! is the production implementation: session key from the [`SessionStore`],
//! organization discovery (cached after the first success), then the usage
//! endpoint, mapped into a classified [`FetchOutcome`].

use std::sync::{Arc, Mutex, PoisonError};

use jiff::Timestamp;
use meter_api::{ApiError, UsageClient};

use crate::scheduler::core::FetchOutcome;
use crate::store::SessionStore;

/// One refresh attempt. Implementations classify their own failures — the
/// scheduler core never sees transport-specific error types.
pub trait UsageTransport: Send + Sync {
    fn fetch(&self) -> impl Future<Output = FetchOutcome> + Send;
}

/// Production transport talking to claude.ai.
pub struct LiveTransport {
    store: Arc<dyn SessionStore>,
    /// First organization's uuid, cached after discovery so steady-state
    /// polling costs one request, not two. Cleared on 401 so a replacement
    /// key (possibly for another account) rediscovers its organization.
    org_id: Mutex<Option<String>>,
}

impl LiveTransport {
    pub fn new(store: Arc<dyn SessionStore>) -> Self {
        Self {
            store,
            org_id: Mutex::new(None),
        }
    }

    fn cached_org(&self) -> Option<String> {
        self.org_id
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn set_cached_org(&self, org_id: Option<String>) {
        *self.org_id.lock().unwrap_or_else(PoisonError::into_inner) = org_id;
    }

    async fn attempt(&self) -> FetchOutcome {
        let key = match self.store.load() {
            Ok(Some(key)) => key,
            Ok(None) => return FetchOutcome::NoSession,
            // A credential-store hiccup (locked keychain, daemon restart) is
            // retryable; it does not mean the key is gone.
            Err(_) => return FetchOutcome::Transient,
        };
        let Ok(client) = UsageClient::new(&key) else {
            return FetchOutcome::Transient;
        };
        let org_id = match self.cached_org() {
            Some(org_id) => org_id,
            None => match self.discover_org(&client).await {
                Ok(org_id) => org_id,
                Err(outcome) => return outcome,
            },
        };
        match client.usage(&org_id).await {
            Ok(response) => FetchOutcome::Success(response.into_snapshot(Timestamp::now())),
            Err(error) => self.classify_and_reset(&error),
        }
    }

    async fn discover_org(&self, client: &UsageClient) -> Result<String, FetchOutcome> {
        match client.organizations().await {
            Ok(orgs) => orgs.into_iter().next().map_or(
                // A valid session with zero organizations cannot yield usage;
                // treat as transient rather than hard-failing.
                Err(FetchOutcome::Transient),
                |org| {
                    self.set_cached_org(Some(org.uuid.clone()));
                    Ok(org.uuid)
                },
            ),
            Err(error) => Err(self.classify_and_reset(&error)),
        }
    }

    /// Map an API error to an outcome, dropping the cached organization on
    /// 401 (see the field docs on `org_id`).
    fn classify_and_reset(&self, error: &ApiError) -> FetchOutcome {
        let outcome = classify(error);
        if outcome == FetchOutcome::Unauthorized {
            self.set_cached_org(None);
        }
        outcome
    }
}

impl UsageTransport for LiveTransport {
    fn fetch(&self) -> impl Future<Output = FetchOutcome> + Send {
        self.attempt()
    }
}

/// Pure classification of API errors into scheduler outcomes: only a 401
/// pauses polling; everything else is worth retrying with backoff.
const fn classify(error: &ApiError) -> FetchOutcome {
    match error {
        ApiError::Unauthorized => FetchOutcome::Unauthorized,
        ApiError::Blocked
        | ApiError::Status(_)
        | ApiError::InvalidSessionKey
        | ApiError::Network(_)
        | ApiError::Decode(_) => FetchOutcome::Transient,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use crate::store::FakeSessionStore;
    use pretty_assertions::assert_eq;

    #[test]
    fn only_unauthorized_pauses_polling() {
        assert_eq!(
            classify(&ApiError::Unauthorized),
            FetchOutcome::Unauthorized
        );
        assert_eq!(classify(&ApiError::Blocked), FetchOutcome::Transient);
        assert_eq!(classify(&ApiError::Status(500)), FetchOutcome::Transient);
        assert_eq!(classify(&ApiError::Status(429)), FetchOutcome::Transient);
        assert_eq!(
            classify(&ApiError::InvalidSessionKey),
            FetchOutcome::Transient
        );
        let decode = serde_json::from_str::<serde_json::Value>("{").unwrap_err();
        assert_eq!(classify(&ApiError::Decode(decode)), FetchOutcome::Transient);
    }

    #[tokio::test]
    async fn missing_session_key_is_reported_without_touching_the_network() {
        let transport = LiveTransport::new(Arc::new(FakeSessionStore::new()));
        assert_eq!(transport.fetch().await, FetchOutcome::NoSession);
    }

    #[tokio::test]
    async fn unavailable_credential_store_is_transient() {
        let transport = LiveTransport::new(Arc::new(FakeSessionStore::unavailable()));
        assert_eq!(transport.fetch().await, FetchOutcome::Transient);
    }

    #[test]
    fn unauthorized_clears_the_cached_organization() {
        let transport = LiveTransport::new(Arc::new(FakeSessionStore::new()));
        transport.set_cached_org(Some("org-1".to_owned()));
        transport.classify_and_reset(&ApiError::Unauthorized);
        assert_eq!(transport.cached_org(), None);
    }

    #[test]
    fn transient_errors_keep_the_cached_organization() {
        let transport = LiveTransport::new(Arc::new(FakeSessionStore::new()));
        transport.set_cached_org(Some("org-1".to_owned()));
        transport.classify_and_reset(&ApiError::Blocked);
        assert_eq!(transport.cached_org(), Some("org-1".to_owned()));
    }
}
