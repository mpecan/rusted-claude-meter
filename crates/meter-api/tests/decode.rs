//! Contract tests for the usage-response decoding and snapshot mapping.
//!
//! The fixture mirrors the live payload shape: flat per-model fields are
//! `null`, and model-scoped caps appear only in the `limits` array, each
//! naming its own scope (`scope.model.display_name`).

#![allow(clippy::unwrap_used)]

use jiff::Timestamp;
use meter_api::UsageResponse;
use meter_core::{LimitWindow, UsageMode, UsageStatus};
use pretty_assertions::assert_eq;

const FIXTURE: &str = include_str!("fixtures/usage_response.json");

// The `spend` money shape ({ amount_minor, currency, exponent }, cap nested
// under `cap.money`) is reconciled against a real captured payload. What remains
// provisional is the *no-limits* framing of this fixture: a true Enterprise
// token account (no allowance limits, spend as the primary view) has not been
// observed directly — the real captured account carried spend *alongside*
// allowance limits. The spend object here uses that confirmed shape.
const COST_FIXTURE: &str = include_str!("fixtures/usage_response_cost.json");

fn fetched_at() -> Timestamp {
    "2026-07-17T12:00:00Z".parse().unwrap()
}

fn decode_fixture(fixture: &str) -> meter_core::UsageSnapshot {
    let response: UsageResponse = serde_json::from_str(fixture).unwrap();
    response.into_snapshot(fetched_at())
}

#[test]
fn decodes_headline_windows_from_flat_fields() {
    let snapshot = decode_fixture(FIXTURE);
    let five_hour = snapshot.five_hour.unwrap();
    assert_eq!(five_hour.window, LimitWindow::FiveHour);
    assert!((five_hour.utilization - 34.0).abs() < f64::EPSILON);
    assert_eq!(snapshot.seven_day.unwrap().window, LimitWindow::SevenDay);
}

#[test]
fn maps_one_scoped_limit_per_named_model() {
    let snapshot = decode_fixture(FIXTURE);
    let names: Vec<&str> = snapshot
        .scoped
        .iter()
        .map(|l| l.display_name.as_str())
        .collect();
    assert_eq!(names, vec!["Fable", "Sonnet"]);
}

#[test]
fn scoped_limits_carry_model_id_when_present() {
    let snapshot = decode_fixture(FIXTURE);
    assert_eq!(snapshot.scoped_named("Fable").unwrap().model_id, None);
    assert_eq!(
        snapshot.scoped_named("Sonnet").unwrap().model_id.as_deref(),
        Some("claude-sonnet-5")
    );
}

#[test]
fn headline_kinds_are_excluded_from_the_scoped_pass() {
    // The fixture contains a `seven_day` entry in `limits`; it must not
    // appear as a scoped limit or the same cap would render twice.
    let snapshot = decode_fixture(FIXTURE);
    assert_eq!(snapshot.scoped.len(), 2);
}

#[test]
fn incomplete_entries_are_skipped_not_errors() {
    // Entries without a model display name or without a percent are dropped
    // silently for forward compatibility. (A missing `resets_at` alone is
    // *not* fatal — see `headline_window_with_null_reset_is_kept_*` below.)
    let snapshot = decode_fixture(FIXTURE);
    assert!(snapshot.scoped_named("Incomplete").is_none());
}

#[test]
fn headline_window_with_null_reset_is_kept_with_a_fallback() {
    // The 5-hour window with no recent usage comes back with a null
    // `resets_at`; it must still render (0% + a synthesized reset) rather
    // than vanish — the regression behind "the 5-hour limit isn't shown".
    let json = r#"{
        "five_hour": { "utilization": 0.0, "resets_at": null },
        "seven_day": { "utilization": 56.0, "resets_at": "2026-07-19T11:00:00Z" },
        "limits": []
    }"#;
    let response: UsageResponse = serde_json::from_str(json).unwrap();
    let snapshot = response.into_snapshot(fetched_at());

    let five_hour = snapshot.five_hour.unwrap();
    assert_eq!(five_hour.window, LimitWindow::FiveHour);
    assert!(five_hour.utilization.abs() < f64::EPSILON);
    // Fallback reset = fetched_at + 5h = 2026-07-17T17:00:00Z.
    assert_eq!(
        five_hour.resets_at,
        "2026-07-17T17:00:00Z".parse::<Timestamp>().unwrap()
    );
}

#[test]
fn scoped_limit_with_null_reset_is_kept_with_a_fallback() {
    // Same rule for a model-scoped weekly cap: a null reset is filled in,
    // not a reason to drop the model.
    let json = r#"{
        "five_hour": { "utilization": 3.0, "resets_at": "2026-07-17T15:00:00Z" },
        "seven_day": { "utilization": 56.0, "resets_at": "2026-07-19T11:00:00Z" },
        "limits": [
            {
                "kind": "weekly_scoped",
                "percent": 64,
                "resets_at": null,
                "is_active": true,
                "scope": { "model": { "id": null, "display_name": "Fable" } }
            }
        ]
    }"#;
    let response: UsageResponse = serde_json::from_str(json).unwrap();
    let snapshot = response.into_snapshot(fetched_at());

    let fable = snapshot.scoped_named("Fable").unwrap();
    // Fallback reset = fetched_at + 7d = 2026-07-24T12:00:00Z.
    assert_eq!(
        fable.usage.resets_at,
        "2026-07-24T12:00:00Z".parse::<Timestamp>().unwrap()
    );
}

#[test]
fn overall_status_reflects_the_worst_scoped_limit() {
    // Sonnet at 82.5% is the worst window in the fixture.
    assert_eq!(
        decode_fixture(FIXTURE).overall_status(),
        UsageStatus::Critical
    );
}

#[test]
fn unknown_fields_do_not_break_decoding() {
    // The fixture includes `spend` and `surface` fields the app ignores.
    let response: Result<UsageResponse, _> = serde_json::from_str(FIXTURE);
    assert!(response.is_ok());
}

#[test]
fn unsurfaced_spend_stub_maps_to_no_spend() {
    // The allowance fixture carries `"spend": { "unsurfaced": true }` — a
    // payload with no usable cost numbers. It must decode to `spend == None`,
    // leaving the account on the allowance view.
    let snapshot = decode_fixture(FIXTURE);
    assert!(snapshot.spend.is_none());
    assert!(snapshot.has_limits());
    assert_eq!(snapshot.suggested_mode(), UsageMode::Allowance);
}

#[test]
fn cost_response_has_no_limits_and_suggests_the_cost_mode() {
    let snapshot = decode_fixture(COST_FIXTURE);
    assert!(!snapshot.has_limits());
    assert!(snapshot.five_hour.is_none());
    assert!(snapshot.seven_day.is_none());
    assert!(snapshot.scoped.is_empty());
    assert_eq!(snapshot.suggested_mode(), UsageMode::Cost);
}

#[test]
fn cost_response_decodes_the_spend_money_triple() {
    let snapshot = decode_fixture(COST_FIXTURE);
    let spend = snapshot.spend.unwrap();
    // Money is minor units + currency + exponent; the cap is read from the
    // nested `cap.money` wrapper. $125.00 used of a $5000.00 budget.
    let used = spend.used.as_ref().unwrap();
    assert_eq!(used.minor, 12_500);
    assert_eq!(used.currency, "USD");
    assert_eq!(used.exponent, 2);
    assert!((used.major() - 125.0).abs() < 1e-9);
    assert_eq!(spend.limit.as_ref().map(|m| m.minor), Some(500_000));
    assert_eq!(spend.cap.as_ref().map(|m| m.minor), Some(500_000));
    assert!(spend.enabled);
    // Gauge recomputed from used/limit rather than the coarse `percent`.
    assert!((spend.fraction_used().unwrap() - 0.025).abs() < 1e-9);
}
