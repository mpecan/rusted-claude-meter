//! Shared mock-server fixtures for the scheduler's `LiveTransport` tests.
//!
//! Both `transport.rs`'s unit tests and `mock_integration.rs`'s end-to-end
//! `run_loop` tests drive a real `meter_api::UsageClient` against a local
//! `wiremock` server, so they share one session key, one usage-response
//! body, and one org-discovery mock rather than keeping byte-for-byte
//! copies in sync by hand as the fixture corpus grows.

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use meter_core::SessionKey;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::consent::ConsentGate;
use crate::debug_log::ResponseLog;
use crate::scheduler::transport::{LiveTransport, SharedHandles};
use crate::store::{FakeSessionStore, SessionStore};

pub(super) const RAW_KEY: &str = "sk-ant-sid01-abcDEF123456_-xyz789";
pub(super) const USAGE_BODY: &str = r#"{"five_hour":{"utilization":10.0,"resets_at":"2026-07-17T15:00:00Z"},"seven_day":null,"limits":[]}"#;

pub(super) fn store_with_key() -> Arc<FakeSessionStore> {
    Arc::new(FakeSessionStore::with_key(
        SessionKey::parse(RAW_KEY).unwrap(),
    ))
}

/// A transport with the Terms-of-Service consent gate **open**, pointed at
/// `base_url`. Almost every scheduler test is about what the stack does once
/// the user has agreed; the gate's own behaviour (no traffic at all while
/// closed) is covered directly in `transport.rs` and `core.rs`.
///
/// Written as a helper so the gate is stated at every construction rather than
/// defaulted — a test that silently fetched nothing would pass vacuously.
pub(super) fn consenting(
    store: Arc<dyn SessionStore>,
    base_url: impl Into<String>,
) -> LiveTransport {
    LiveTransport::with_base_url(store, base_url).with_handles(SharedHandles {
        response_log: Arc::new(ResponseLog::disabled()),
        consent: Arc::new(ConsentGate::new(true)),
    })
}

pub(super) async fn mount_org_discovery(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/organizations"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!([{ "uuid": "org-1", "name": "Acme" }])),
        )
        .mount(server)
        .await;
}
