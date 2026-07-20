//! Integration tests: `UsageClient` against a local mock of the claude.ai
//! API (wiremock), not the real network — CI can run these with no
//! outbound access at all.
//!
//! Covers the scenarios from issue #13: success, the auth/blocked/rate-limit
//! status codes, a generic 5xx, malformed JSON, and a slow response; plus
//! that the spoofed browser headers (UA, Origin, sensitive Cookie) are
//! actually put on the wire, not just constructed in `headers.rs`.

#![allow(clippy::unwrap_used)]

use std::time::Duration;

use meter_api::{ApiError, UsageClient};
use meter_core::SessionKey;
use wiremock::matchers::{body_string_contains, header, header_exists, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const FIXTURE: &str = include_str!("fixtures/usage_response.json");
// PROVISIONAL cost/token payload — see the note in `decode.rs`.
const COST_FIXTURE: &str = include_str!("fixtures/usage_response_cost.json");
const RAW_KEY: &str = "sk-ant-sid01-abcDEF123456_-xyz789";

fn session_key() -> SessionKey {
    SessionKey::parse(RAW_KEY).unwrap()
}

fn client(server: &MockServer) -> UsageClient {
    UsageClient::with_base_url(&session_key(), &server.uri()).unwrap()
}

#[tokio::test]
async fn organizations_decodes_a_successful_response() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/organizations"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
            { "uuid": "org-1", "name": "Acme" }
        ])))
        .mount(&server)
        .await;

    let orgs = client(&server).organizations().await.unwrap();
    assert_eq!(orgs.len(), 1);
    assert_eq!(orgs[0].uuid, "org-1");
}

#[tokio::test]
async fn usage_decodes_the_fixture_corpus_over_the_wire() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/organizations/org-1/usage"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(FIXTURE, "application/json"))
        .mount(&server)
        .await;

    let usage = client(&server).usage("org-1").await.unwrap();
    assert_eq!(usage.limits.len(), 5);
    assert!((usage.five_hour.unwrap().utilization - 34.0).abs() < f64::EPSILON);
}

#[tokio::test]
async fn usage_decodes_a_token_cost_response_over_the_wire() {
    // A no-limits Enterprise payload: `limits` is empty and the account is
    // token/cost based, so the decoded response carries a `spend` object.
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/organizations/org-1/usage"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(COST_FIXTURE, "application/json"))
        .mount(&server)
        .await;

    let usage = client(&server).usage("org-1").await.unwrap();
    assert!(usage.limits.is_empty());
    assert!(usage.five_hour.is_none());

    let fetched_at: jiff::Timestamp = "2026-07-19T12:00:00Z".parse().unwrap();
    let snapshot = usage.into_snapshot(fetched_at);
    assert!(!snapshot.has_limits());
    assert_eq!(snapshot.suggested_mode(), meter_core::UsageMode::Cost);
    let spend = snapshot.spend.unwrap();
    assert_eq!(spend.used.as_ref().map(|m| m.minor), Some(12_500));
    assert_eq!(spend.limit.as_ref().map(|m| m.minor), Some(500_000));
    assert_eq!(
        spend.used.as_ref().map(|m| m.currency.as_str()),
        Some("USD")
    );
}

#[tokio::test]
async fn requests_present_the_spoofed_browser_headers() {
    let server = MockServer::start().await;
    // The mock only matches (and therefore only returns 200) when the
    // headers `browser_headers` builds are actually present on the wire —
    // a request missing any of them falls through to wiremock's default 404
    // and the client call below fails, proving the header spoofing survives
    // the trip through `reqwest`, not just `headers.rs`'s own unit tests.
    Mock::given(method("GET"))
        .and(path("/organizations"))
        .and(header_exists("user-agent"))
        .and(header("origin", "https://claude.ai"))
        .and(header("referer", "https://claude.ai/"))
        .and(header("cookie", format!("sessionKey={RAW_KEY}").as_str()))
        .respond_with(ResponseTemplate::new(200).set_body_json(Vec::<serde_json::Value>::new()))
        .mount(&server)
        .await;

    let result = client(&server).organizations().await;
    assert!(
        result.is_ok(),
        "headers were not sent as expected: {result:?}"
    );

    // A distinct assertion for the User-Agent's *value*, not just presence.
    let request = server
        .received_requests()
        .await
        .unwrap()
        .into_iter()
        .next()
        .unwrap();
    let ua = request.headers.get("user-agent").unwrap().to_str().unwrap();
    assert!(ua.contains("Chrome"), "unexpected UA: {ua}");
}

#[tokio::test]
async fn auth_failures_map_to_their_api_error() {
    // The status→error mapping lives in `UsageClient::get`, shared by every
    // endpoint: a 401 during org discovery is a session-expired signal, and a
    // 403 on the usage endpoint is claude.ai's bot block. Both distinct
    // mappings are exercised on the endpoint each actually occurs on.
    let unauthorized = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/organizations"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&unauthorized)
        .await;
    assert!(matches!(
        client(&unauthorized).organizations().await.unwrap_err(),
        ApiError::Unauthorized
    ));

    let blocked = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/organizations/org-1/usage"))
        .respond_with(ResponseTemplate::new(403))
        .mount(&blocked)
        .await;
    assert!(matches!(
        client(&blocked).usage("org-1").await.unwrap_err(),
        ApiError::Blocked
    ));
}

#[tokio::test]
async fn other_http_errors_map_to_a_status_error() {
    // 429 (rate limit) and 5xx both surface as ApiError::Status carrying the
    // code — the caller decides whether to back off.
    for code in [429_u16, 503] {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/organizations/org-1/usage"))
            .respond_with(ResponseTemplate::new(code))
            .mount(&server)
            .await;

        let error = client(&server).usage("org-1").await.unwrap_err();
        assert!(
            matches!(error, ApiError::Status(got) if got == code),
            "{code} should map to ApiError::Status({code}), got {error:?}"
        );
    }
}

#[tokio::test]
async fn malformed_json_is_a_decode_error_not_a_panic() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/organizations/org-1/usage"))
        .respond_with(ResponseTemplate::new(200).set_body_raw("{not json", "application/json"))
        .mount(&server)
        .await;

    let error = client(&server).usage("org-1").await.unwrap_err();
    assert!(matches!(error, ApiError::Decode(_)));
}

#[tokio::test]
async fn slow_responses_still_resolve_successfully() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/organizations/org-1/usage"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw(FIXTURE, "application/json")
                .set_delay(Duration::from_millis(300)),
        )
        .mount(&server)
        .await;

    let usage = client(&server).usage("org-1").await.unwrap();
    assert_eq!(usage.limits.len(), 5);
}

#[tokio::test]
async fn a_request_body_matcher_never_matches_get_requests() {
    // Sanity check that GETs used throughout this module carry no body, so
    // the header-presence assertions above cannot be accidentally satisfied
    // by a permissive matcher.
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/organizations"))
        .and(body_string_contains("never-present"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;

    let error = client(&server).organizations().await.unwrap_err();
    // No mock matches (the GET carries no body), so wiremock falls through
    // to its default 404 response.
    assert!(matches!(error, ApiError::Status(404)));
}
