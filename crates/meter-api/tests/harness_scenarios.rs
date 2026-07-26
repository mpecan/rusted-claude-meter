//! Every Linux-harness scenario must still decode through the real parser.
//!
//! `harness/demo-server/scenarios/*.json` are the payloads the demo API serves
//! to a real app build running in the Lima desktop VMs (see `harness/README.md`).
//! They are hand-edited fixtures living outside this workspace, so nothing
//! otherwise stops one from drifting out of shape — a renamed field or a typo
//! would surface as an unexplained "no data" in a VM, which is a slow and
//! confusing place to debug a JSON error.
//!
//! This test is the cheap guard: it decodes each scenario exactly as
//! `LiveTransport` does and asserts the result is usable. It runs on every
//! `just check`, so the harness cannot rot silently between uses.

// Test-only: a failed assertion here should abort loudly with the offending
// scenario named, which is exactly what `expect`/`panic!` give us.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use jiff::Timestamp;
use meter_api::UsageResponse;

fn scenarios_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../harness/demo-server/scenarios")
        .canonicalize()
        .expect("the harness scenarios directory should exist")
}

/// Scenarios write `resets_at` as `{{now+3h}}` tokens that the demo server
/// resolves per request (and must, or every window would read as fully elapsed
/// — see `harness/demo-server/src/scenario.rs`). Structure is what this test
/// checks, so any token is replaced with one fixed valid timestamp rather than
/// reimplementing the offset arithmetic.
fn resolve_tokens(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after = &rest[start..];
        let Some(end) = after.find("}}") else {
            out.push_str(after);
            return out;
        };
        out.push_str("2026-07-25T12:00:00Z");
        rest = &after[end + 2..];
    }
    out.push_str(rest);
    out
}

#[test]
fn every_scenario_decodes_into_a_usable_snapshot() {
    let dir = scenarios_dir();
    let mut checked = Vec::new();
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|ext| ext != "json") {
            continue;
        }
        let name = path.file_stem().unwrap().to_string_lossy().into_owned();
        let raw = resolve_tokens(&std::fs::read_to_string(&path).unwrap());
        let response: UsageResponse = serde_json::from_str(&raw)
            .unwrap_or_else(|error| panic!("scenario `{name}` does not decode: {error}"));
        let snapshot = response.into_snapshot(Timestamp::now());

        // A scenario that decodes but carries nothing renders as an empty
        // popover, which in a VM looks exactly like a failed fetch. Require
        // each one to actually say something.
        assert!(
            snapshot.five_hour.is_some() || snapshot.seven_day.is_some(),
            "scenario `{name}` has no headline window, so the tray would show nothing"
        );
        checked.push(name);
    }
    checked.sort();
    assert!(
        checked.len() >= 8,
        "expected the full scenario set, found {checked:?}"
    );
}

#[test]
fn the_spend_scenario_surfaces_a_cost_account() {
    // `spend` is the field that decides whether the app can render Cost mode at
    // all; the other scenarios deliberately mark it unsurfaced. If this one
    // stopped producing a spend object, the cost view would be untestable in
    // the VMs and nothing else here would notice.
    let raw = resolve_tokens(&std::fs::read_to_string(scenarios_dir().join("spend.json")).unwrap());
    let response: UsageResponse = serde_json::from_str(&raw).unwrap();
    let spend = response
        .into_snapshot(Timestamp::now())
        .spend
        .expect("the `spend` scenario should decode a spend object");
    assert!(spend.enabled);
    assert_eq!(spend.used.as_ref().map(|money| money.minor), Some(640));
    assert_eq!(spend.limit.as_ref().map(|money| money.minor), Some(200_000));
}

#[test]
fn the_scoped_models_scenario_covers_the_named_model_contract() {
    // Mirrors the fixture in `usage_response.json`: named models only, the
    // `seven_day` headline kind excluded, and the incomplete future-kind entry
    // skipped rather than treated as an error.
    let raw = resolve_tokens(
        &std::fs::read_to_string(scenarios_dir().join("scoped-models.json")).unwrap(),
    );
    let response: UsageResponse = serde_json::from_str(&raw).unwrap();
    let snapshot = response.into_snapshot(Timestamp::now());
    let names: Vec<&str> = snapshot
        .scoped
        .iter()
        .map(|limit| limit.display_name.as_str())
        .collect();
    assert_eq!(names, ["Fable", "Sonnet", "Opus"]);
}
