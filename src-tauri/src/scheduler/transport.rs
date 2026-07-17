//! The network side of the scheduler: one refresh attempt end to end.
//!
//! [`UsageTransport`] is the seam the polling loop is generic over, so tests
//! drive the loop with fakes and never touch the network. [`LiveTransport`]
//! is the production implementation: session key from the [`SessionStore`],
//! organization discovery (cached after the first success), then the usage
//! endpoint, mapped into a classified [`FetchOutcome`].

use std::sync::{Arc, Mutex, PoisonError};

use jiff::Timestamp;
use meter_api::{ApiError, DEFAULT_BASE_URL, UsageClient};

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
    /// `meter-api`'s base URL, injectable so tests can point a real
    /// `UsageClient` at a local mock server instead of claude.ai — see
    /// [`LiveTransport::with_base_url`].
    base_url: String,
    /// First organization's uuid, cached after discovery so steady-state
    /// polling costs one request, not two. Cleared on 401 so a replacement
    /// key (possibly for another account) rediscovers its organization.
    org_id: Mutex<Option<String>>,
}

impl LiveTransport {
    pub fn new(store: Arc<dyn SessionStore>) -> Self {
        Self::with_base_url(store, DEFAULT_BASE_URL)
    }

    /// As [`LiveTransport::new`], but pointed at `base_url` instead of
    /// claude.ai — the seam integration tests use to drive a real
    /// `UsageClient` against a local mock server with no network access.
    pub fn with_base_url(store: Arc<dyn SessionStore>, base_url: impl Into<String>) -> Self {
        Self {
            store,
            base_url: base_url.into(),
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
        let Ok(client) = UsageClient::with_base_url(&key, &self.base_url) else {
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
    use crate::scheduler::test_support::{USAGE_BODY, mount_org_discovery, store_with_key};
    use crate::store::FakeSessionStore;
    use pretty_assertions::assert_eq;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// A `LiveTransport` pointed at a mock server end to end: real
    /// `UsageClient` requests over loopback, no live claude.ai access — the
    /// scenarios from issue #13, driven through the transport that
    /// production code actually uses (not just `meter-api` in isolation).

    #[tokio::test]
    async fn fetch_against_a_healthy_mock_server_succeeds() {
        let server = MockServer::start().await;
        mount_org_discovery(&server).await;
        Mock::given(method("GET"))
            .and(path("/organizations/org-1/usage"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(USAGE_BODY, "application/json"))
            .mount(&server)
            .await;

        let transport = LiveTransport::with_base_url(store_with_key(), server.uri());
        let outcome = transport.fetch().await;
        assert!(matches!(outcome, FetchOutcome::Success(_)));
    }

    #[tokio::test]
    async fn session_expired_propagates_from_a_real_401_response() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/organizations"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;

        let transport = LiveTransport::with_base_url(store_with_key(), server.uri());
        assert_eq!(transport.fetch().await, FetchOutcome::Unauthorized);
    }

    #[tokio::test]
    async fn blocked_response_after_discovery_is_transient() {
        let server = MockServer::start().await;
        mount_org_discovery(&server).await;
        Mock::given(method("GET"))
            .and(path("/organizations/org-1/usage"))
            .respond_with(ResponseTemplate::new(403))
            .mount(&server)
            .await;

        let transport = LiveTransport::with_base_url(store_with_key(), server.uri());
        assert_eq!(transport.fetch().await, FetchOutcome::Transient);
    }

    #[tokio::test]
    async fn rate_limited_response_is_transient() {
        let server = MockServer::start().await;
        mount_org_discovery(&server).await;
        Mock::given(method("GET"))
            .and(path("/organizations/org-1/usage"))
            .respond_with(ResponseTemplate::new(429))
            .mount(&server)
            .await;

        let transport = LiveTransport::with_base_url(store_with_key(), server.uri());
        assert_eq!(transport.fetch().await, FetchOutcome::Transient);
    }

    #[tokio::test]
    async fn server_error_response_is_transient() {
        let server = MockServer::start().await;
        mount_org_discovery(&server).await;
        Mock::given(method("GET"))
            .and(path("/organizations/org-1/usage"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;

        let transport = LiveTransport::with_base_url(store_with_key(), server.uri());
        assert_eq!(transport.fetch().await, FetchOutcome::Transient);
    }

    #[tokio::test]
    async fn malformed_json_from_a_real_server_is_transient_not_a_panic() {
        let server = MockServer::start().await;
        mount_org_discovery(&server).await;
        Mock::given(method("GET"))
            .and(path("/organizations/org-1/usage"))
            .respond_with(ResponseTemplate::new(200).set_body_raw("{not json", "application/json"))
            .mount(&server)
            .await;

        let transport = LiveTransport::with_base_url(store_with_key(), server.uri());
        assert_eq!(transport.fetch().await, FetchOutcome::Transient);
    }

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
