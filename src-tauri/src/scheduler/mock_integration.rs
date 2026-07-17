//! Scheduler + client integration tests (issue #13): a real `LiveTransport`
//! — a real `meter_api::UsageClient` making real HTTP requests over loopback
//! — wired against a local `wiremock` server. No live claude.ai access.
//!
//! `core.rs` already proves the backoff *formula* in isolation with a fake
//! clock and `transport.rs` already proves HTTP-status classification with
//! fakes; this module is the one place that proves the two are wired
//! together correctly end to end using a real `UsageClient` over real
//! (loopback) HTTP: the backoff sequence computed from real 5xx responses,
//! session-expiry propagating from a real 401 all the way through
//! `SchedulerCore`, and (for the two scenarios that need no artificial
//! delay — an immediate success and a 401 that parks the loop) the actual
//! async `run_loop` driver rather than just its pieces.
//!
//! Deliberately real (unpaused) time throughout: `tokio::time::pause`
//! combined with genuine socket I/O against a background `wiremock` server
//! is unreliable — the paused runtime's auto-advance can starve a spawned
//! task's timer indefinitely once a real connection is involved, so mixing
//! the two is avoided. Every test here either performs no waiting at all
//! (the backoff sequence is computed, never slept) or waits only on
//! near-instant real I/O, which keeps the whole module comfortably under
//! the "runs in nextest under 5s" acceptance criterion.

#![allow(clippy::unwrap_used)]

use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::Notify;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::core::{FetchOutcome, Phase};
use super::test_support::{USAGE_BODY, mount_org_discovery, store_with_key};
use super::transport::{LiveTransport, UsageTransport};
use super::{PersistPaths, RefreshInterval, SchedulerCore, SchedulerHandle, SystemClock, run_loop};

fn phase_of(core: &Mutex<SchedulerCore>) -> Phase {
    core.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .state(jiff::Timestamp::now())
        .phase
}

fn snapshot_present(core: &Mutex<SchedulerCore>) -> bool {
    core.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .state(jiff::Timestamp::now())
        .snapshot
        .is_some()
}

/// Poll `predicate` on a short real-time cadence until it holds, bounded so
/// a broken wiring fails the test fast instead of hanging the suite. Real
/// (not virtual) time — see the module docs for why.
async fn wait_until(predicate: impl Fn() -> bool) {
    for _ in 0..200 {
        if predicate() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(
        predicate(),
        "condition not reached within the real-time budget"
    );
}

/// The scheduler's own backoff formula (10s, 20s, 40s, … — see `core.rs`),
/// driven by classifications of *real* HTTP 5xx/429 responses rather than a
/// scripted fake, then a real success reply once the mock's failure budget
/// is exhausted. No time is slept: `SchedulerCore::next_delay` is a pure
/// function of the recorded failure count, so the sequence is asserted
/// directly against real fetch outcomes.
#[tokio::test]
async fn real_http_backoff_sequence_matches_the_core_formula() {
    let server = MockServer::start().await;
    mount_org_discovery(&server).await;
    // The first three usage requests are retryable errors (5xx, then a rate
    // limit); wiremock exhausts this mock's budget and falls through to the
    // lower-priority, unlimited success mock from the fourth request on.
    Mock::given(method("GET"))
        .and(path("/organizations/org-1/usage"))
        .respond_with(ResponseTemplate::new(500))
        .up_to_n_times(2)
        .with_priority(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/organizations/org-1/usage"))
        .respond_with(ResponseTemplate::new(429))
        .up_to_n_times(1)
        .with_priority(2)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/organizations/org-1/usage"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(USAGE_BODY, "application/json"))
        .mount(&server)
        .await;

    let transport = LiveTransport::with_base_url(store_with_key(), server.uri());
    let mut core = SchedulerCore::new(RefreshInterval::OneMinute, None);
    let mut delays = Vec::new();

    for _ in 0..3 {
        let outcome = transport.fetch().await;
        assert_eq!(outcome, FetchOutcome::Transient);
        core.record(outcome);
        assert_eq!(core.state(jiff::Timestamp::now()).phase, Phase::Degraded);
        delays.push(core.next_delay(1.0).unwrap().as_secs());
    }
    assert_eq!(
        delays,
        vec![10, 20, 40],
        "real 5xx/429 responses must drive the documented backoff sequence"
    );

    let outcome = transport.fetch().await;
    assert!(matches!(outcome, FetchOutcome::Success(_)));
    core.record(outcome);
    assert_eq!(core.state(jiff::Timestamp::now()).phase, Phase::Polling);
    assert_eq!(
        core.next_delay(1.0),
        Some(RefreshInterval::OneMinute.duration()),
        "recovery must reset the backoff sequence back to the normal cadence"
    );
}

/// A real 401 from the mock server classifies as `Unauthorized` and must
/// carry all the way to `Phase::SessionExpired` with polling paused (no
/// retry storm against a dead key) — driven through the real async
/// `run_loop`, not just `SchedulerCore::record` in isolation.
#[tokio::test]
async fn real_http_401_propagates_to_session_expired_and_recovers_on_resume() {
    let server = MockServer::start().await;
    // Only the first organizations request is unauthorized; once resumed,
    // discovery and the usage fetch both succeed.
    Mock::given(method("GET"))
        .and(path("/organizations"))
        .respond_with(ResponseTemplate::new(401))
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&server)
        .await;
    mount_org_discovery(&server).await;
    Mock::given(method("GET"))
        .and(path("/organizations/org-1/usage"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(USAGE_BODY, "application/json"))
        .mount(&server)
        .await;

    let core = Arc::new(Mutex::new(SchedulerCore::new(
        RefreshInterval::OneMinute,
        None,
    )));
    let notify = Arc::new(Notify::new());
    let handle = SchedulerHandle::new(Arc::clone(&core), Arc::clone(&notify));
    let task = tokio::spawn(run_loop(
        LiveTransport::with_base_url(store_with_key(), server.uri()),
        SystemClock::default(),
        SchedulerHandle::new(Arc::clone(&core), notify),
        PersistPaths::default(),
        |_| {},
    ));

    wait_until(|| phase_of(&core) == Phase::SessionExpired).await;
    let parked_requests = server.received_requests().await.unwrap().len();
    // Parked: the loop must not keep hitting the API on its own — only an
    // explicit resume (as issuing a new session key does) wakes it.
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(
        server.received_requests().await.unwrap().len(),
        parked_requests,
        "a parked loop must not keep hitting the API on an expired session"
    );

    handle.resume_polling();
    // `resume()` itself flips the phase back to `Polling` synchronously
    // (before any new fetch runs), so wait on the snapshot too — the only
    // signal that the *new* fetch actually completed.
    wait_until(|| snapshot_present(&core) && phase_of(&core) == Phase::Polling).await;
    task.abort();
}

/// End-to-end happy path through the real async driver: a session key from
/// the store, a real `UsageClient` request for org discovery and usage
/// against the mock server, mapped into a snapshot and broadcast as
/// `Phase::Polling` — the wiring `run_loop` + `LiveTransport` + a real HTTP
/// response actually production code exercises.
#[tokio::test]
async fn run_loop_completes_a_real_fetch_through_the_full_stack() {
    let server = MockServer::start().await;
    mount_org_discovery(&server).await;
    Mock::given(method("GET"))
        .and(path("/organizations/org-1/usage"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(USAGE_BODY, "application/json"))
        .mount(&server)
        .await;

    let core = Arc::new(Mutex::new(SchedulerCore::new(
        RefreshInterval::OneMinute,
        None,
    )));
    let notify = Arc::new(Notify::new());
    let task = tokio::spawn(run_loop(
        LiveTransport::with_base_url(store_with_key(), server.uri()),
        SystemClock::default(),
        SchedulerHandle::new(Arc::clone(&core), notify),
        PersistPaths::default(),
        |_| {},
    ));

    wait_until(|| phase_of(&core) == Phase::Polling && snapshot_present(&core)).await;
    task.abort();
}
