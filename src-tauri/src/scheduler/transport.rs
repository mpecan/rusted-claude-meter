//! The network side of the scheduler: one refresh attempt end to end.
//!
//! [`UsageTransport`] is the seam the polling loop is generic over, so tests
//! drive the loop with fakes and never touch the network. [`LiveTransport`]
//! is the production implementation: session key from the [`SessionStore`],
//! organization discovery (cached after the first success), then the usage
//! endpoint, mapped into a classified [`FetchOutcome`].

use std::sync::{Arc, Mutex, PoisonError};

use jiff::Timestamp;
use meter_api::{ApiError, UsageClient, UsageResponse};

use crate::consent::ConsentGate;
use crate::debug_log::ResponseLog;
use crate::scheduler::core::FetchOutcome;
use crate::store::{SessionStore, run_store_op};

/// One refresh attempt. Implementations classify their own failures — the
/// scheduler core never sees transport-specific error types.
pub trait UsageTransport: Send + Sync {
    fn fetch(&self) -> impl Future<Output = FetchOutcome> + Send;
}

/// The live, shared handles [`LiveTransport`] is built with. Both are flipped
/// from Settings and read on every poll tick, so neither can be a value copied
/// in at startup — they are `Arc`s of the very objects the commands mutate.
pub struct SharedHandles {
    pub response_log: Arc<ResponseLog>,
    pub consent: Arc<ConsentGate>,
}

/// Production transport talking to claude.ai.
pub struct LiveTransport {
    store: Arc<dyn SessionStore>,
    /// `meter-api`'s base URL, injectable so tests can point a real
    /// `UsageClient` at a local mock server instead of claude.ai, and the
    /// `RCM_API_BASE_URL` override points the shipped binary at the Linux demo
    /// harness — see [`LiveTransport::with_base_url`] and `crate::api_base`.
    base_url: String,
    /// First organization's uuid, cached after discovery so steady-state
    /// polling costs one request, not two. Cleared on 401 so a replacement
    /// key (possibly for another account) rediscovers its organization.
    org_id: Mutex<Option<String>>,
    /// Opt-in raw-response logger. Off (a no-op [`ResponseLog::disabled`]) until
    /// production wiring attaches the real one via [`Self::with_handles`];
    /// the usage body is written through it before it's decoded, so a real
    /// payload can be captured to verify the `spend` shape against more accounts.
    response_log: Arc<ResponseLog>,
    /// The Terms-of-Service consent gate. Shared with the Settings command that flips it,
    /// and checked at the top of every [`Self::attempt`]; a closed gate makes
    /// this transport a no-op that reports [`FetchOutcome::NotAcknowledged`].
    /// Defaults to closed, so a transport built without one polls nothing.
    consent: Arc<ConsentGate>,
}

impl LiveTransport {
    /// Build a transport against `base_url`. Production passes
    /// `crate::api_base::api_base_url()` — claude.ai unless the
    /// `RCM_API_BASE_URL` override is set; integration tests and the Linux
    /// demo harness pass a local mock server, so a real `UsageClient` runs
    /// with no network access.
    /// The consent gate starts **closed**: a transport built this way fetches
    /// nothing until [`Self::with_handles`] attaches the shared gate. Failing
    /// closed is the deliberate direction — a wiring mistake costs a dead
    /// meter, which is loud, rather than un-consented traffic, which is silent.
    pub fn with_base_url(store: Arc<dyn SessionStore>, base_url: impl Into<String>) -> Self {
        Self {
            store,
            base_url: base_url.into(),
            org_id: Mutex::new(None),
            response_log: Arc::new(ResponseLog::disabled()),
            consent: Arc::new(crate::consent::closed()),
        }
    }

    /// Attach the shared runtime handles — the consent gate and the debug
    /// response log. One builder rather than one per handle: they are always
    /// attached together, and two `fn with_x(mut self, x) -> Self { self.x = x;
    /// self }` methods are the same code twice.
    #[must_use]
    pub fn with_handles(mut self, handles: SharedHandles) -> Self {
        self.consent = handles.consent;
        self.response_log = handles.response_log;
        self
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
        // Consent first, before anything else — deliberately ahead of the
        // credential-store read, so a user who has not accepted the ToS risk
        // gets no claude.ai traffic *and* no keychain prompt out of this app.
        if !self.consent.get() {
            return FetchOutcome::NotAcknowledged;
        }
        // The credential store is a synchronous OS round trip; every poll
        // tick routes it through the blocking pool so a slow Keychain /
        // Secret-Service daemon can't tie up an async worker thread.
        let key = match run_store_op(&self.store, |store| store.load()).await {
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
        // Fetch the raw body so debug logging can capture it verbatim, then
        // decode. A decode failure routes through `classify_and_reset` as an
        // `ApiError::Decode`, so it follows the same classification as a decode
        // failure inside `UsageClient::usage` rather than hardcoding the outcome.
        match client.usage_raw(&org_id).await {
            Ok(body) => {
                self.response_log.record("usage", &body);
                serde_json::from_str::<UsageResponse>(&body).map_or_else(
                    |error| self.classify_and_reset(&ApiError::Decode(error)),
                    |response| FetchOutcome::Success(response.into_snapshot(Timestamp::now())),
                )
            }
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
    use meter_api::DEFAULT_BASE_URL;
    use pretty_assertions::assert_eq;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    // A `LiveTransport` pointed at a mock server end to end: real
    // `UsageClient` requests over loopback, no live claude.ai access — the
    // scenarios from issue #13, driven through the transport that
    // production code actually uses (not just `meter-api` in isolation).

    /// A transport with the Terms-of-Service consent gate **open**, which is
    /// what every test below other than the consent tests themselves is
    /// about: they exercise what happens once the user has agreed. Written as
    /// a helper so the gate is stated at every construction rather than
    /// defaulted — a test that silently fetched nothing would pass vacuously.
    fn consenting(store: Arc<dyn SessionStore>, base_url: impl Into<String>) -> LiveTransport {
        LiveTransport::with_base_url(store, base_url).with_handles(SharedHandles {
            response_log: Arc::new(ResponseLog::disabled()),
            consent: Arc::new(ConsentGate::new(true)),
        })
    }

    #[tokio::test]
    async fn a_closed_consent_gate_makes_no_request_at_all() {
        // The load-bearing test for the whole feature. The mock server is
        // fully healthy and a valid key is stored, so the *only* reason for
        // not fetching is the gate — and `received_requests` proves nothing
        // went over the wire, rather than merely that the outcome was mapped.
        let server = MockServer::start().await;
        mount_org_discovery(&server).await;
        Mock::given(method("GET"))
            .and(path("/organizations/org-1/usage"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(USAGE_BODY, "application/json"))
            .mount(&server)
            .await;

        let transport = LiveTransport::with_base_url(store_with_key(), server.uri());
        assert_eq!(transport.fetch().await, FetchOutcome::NotAcknowledged);
        assert!(
            server
                .received_requests()
                .await
                .is_some_and(|r| r.is_empty()),
            "a transport without consent must not touch claude.ai"
        );
    }

    #[tokio::test]
    async fn a_closed_gate_is_checked_before_the_credential_store() {
        // No session key is stored either, so both refusals apply; consent
        // must win, because the app should never read the user's keychain on
        // behalf of a request it is not allowed to make.
        let transport =
            LiveTransport::with_base_url(Arc::new(FakeSessionStore::new()), DEFAULT_BASE_URL);
        assert_eq!(transport.fetch().await, FetchOutcome::NotAcknowledged);
    }

    #[tokio::test]
    async fn withdrawing_consent_stops_a_transport_that_was_fetching() {
        let server = MockServer::start().await;
        mount_org_discovery(&server).await;
        Mock::given(method("GET"))
            .and(path("/organizations/org-1/usage"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(USAGE_BODY, "application/json"))
            .mount(&server)
            .await;

        let gate = Arc::new(ConsentGate::new(true));
        let transport = LiveTransport::with_base_url(store_with_key(), server.uri()).with_handles(
            SharedHandles {
                response_log: Arc::new(ResponseLog::disabled()),
                consent: Arc::clone(&gate),
            },
        );
        assert!(matches!(transport.fetch().await, FetchOutcome::Success(_)));

        // Flipping the shared gate reaches the live transport without
        // rebuilding it — un-ticking the box stops polling immediately.
        gate.set(false);
        assert_eq!(transport.fetch().await, FetchOutcome::NotAcknowledged);
    }

    #[tokio::test]
    async fn fetch_against_a_healthy_mock_server_succeeds() {
        let server = MockServer::start().await;
        mount_org_discovery(&server).await;
        Mock::given(method("GET"))
            .and(path("/organizations/org-1/usage"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(USAGE_BODY, "application/json"))
            .mount(&server)
            .await;

        let transport = consenting(store_with_key(), server.uri());
        let outcome = transport.fetch().await;
        assert!(matches!(outcome, FetchOutcome::Success(_)));
    }

    #[tokio::test]
    async fn enabled_response_log_captures_the_raw_usage_body() {
        let server = MockServer::start().await;
        mount_org_discovery(&server).await;
        Mock::given(method("GET"))
            .and(path("/organizations/org-1/usage"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(USAGE_BODY, "application/json"))
            .mount(&server)
            .await;

        let dir = tempfile::tempdir().unwrap();
        let log_path = dir.path().join(crate::debug_log::LOG_FILE);
        let log = Arc::new(ResponseLog::new(Some(log_path.clone()), true));
        let transport = LiveTransport::with_base_url(store_with_key(), server.uri()).with_handles(
            SharedHandles {
                response_log: Arc::clone(&log),
                consent: Arc::new(ConsentGate::new(true)),
            },
        );

        let outcome = transport.fetch().await;
        assert!(matches!(outcome, FetchOutcome::Success(_)));
        // The exact wire body was written verbatim (the whole point: capturing
        // real payloads to reconcile the `spend` shape).
        let logged = std::fs::read_to_string(&log_path).unwrap();
        assert!(logged.contains(USAGE_BODY.trim_end()));
        assert!(logged.contains("usage ====="));
    }

    #[tokio::test]
    async fn disabled_response_log_writes_nothing_on_success() {
        let server = MockServer::start().await;
        mount_org_discovery(&server).await;
        Mock::given(method("GET"))
            .and(path("/organizations/org-1/usage"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(USAGE_BODY, "application/json"))
            .mount(&server)
            .await;

        let dir = tempfile::tempdir().unwrap();
        let log_path = dir.path().join(crate::debug_log::LOG_FILE);
        let log = Arc::new(ResponseLog::new(Some(log_path.clone()), false));
        let transport = LiveTransport::with_base_url(store_with_key(), server.uri()).with_handles(
            SharedHandles {
                response_log: Arc::clone(&log),
                consent: Arc::new(ConsentGate::new(true)),
            },
        );

        assert!(matches!(transport.fetch().await, FetchOutcome::Success(_)));
        assert!(!log_path.exists(), "a disabled log must not be written");
    }

    #[tokio::test]
    async fn session_expired_propagates_from_a_real_401_response() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/organizations"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;

        let transport = consenting(store_with_key(), server.uri());
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

        let transport = consenting(store_with_key(), server.uri());
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

        let transport = consenting(store_with_key(), server.uri());
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

        let transport = consenting(store_with_key(), server.uri());
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

        let transport = consenting(store_with_key(), server.uri());
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
        let transport = consenting(Arc::new(FakeSessionStore::new()), DEFAULT_BASE_URL);
        assert_eq!(transport.fetch().await, FetchOutcome::NoSession);
    }

    #[tokio::test]
    async fn unavailable_credential_store_is_transient() {
        let transport = consenting(Arc::new(FakeSessionStore::unavailable()), DEFAULT_BASE_URL);
        assert_eq!(transport.fetch().await, FetchOutcome::Transient);
    }

    #[test]
    fn unauthorized_clears_the_cached_organization() {
        let transport = consenting(Arc::new(FakeSessionStore::new()), DEFAULT_BASE_URL);
        transport.set_cached_org(Some("org-1".to_owned()));
        transport.classify_and_reset(&ApiError::Unauthorized);
        assert_eq!(transport.cached_org(), None);
    }

    #[test]
    fn transient_errors_keep_the_cached_organization() {
        let transport = consenting(Arc::new(FakeSessionStore::new()), DEFAULT_BASE_URL);
        transport.set_cached_org(Some("org-1".to_owned()));
        transport.classify_and_reset(&ApiError::Blocked);
        assert_eq!(transport.cached_org(), Some("org-1".to_owned()));
    }
}
