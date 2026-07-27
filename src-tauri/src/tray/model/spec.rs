#![allow(clippy::unwrap_used)]

use super::*;
use jiff::SignedDuration;
use meter_core::{Money, ScopedLimit, Spend, UsageMode, UsageSnapshot, UsageStatus};
use pretty_assertions::assert_eq;

fn now() -> Timestamp {
    "2026-07-17T12:00:00Z".parse().unwrap()
}

fn window(utilization: f64, resets_in_secs: i64, kind: LimitWindow) -> UsageWindow {
    UsageWindow {
        utilization,
        resets_at: now() + SignedDuration::from_secs(resets_in_secs),
        window: kind,
    }
}

fn snapshot() -> UsageSnapshot {
    UsageSnapshot {
        five_hour: Some(window(41.5, 2 * 3600 + 15 * 60, LimitWindow::FiveHour)),
        seven_day: Some(window(63.0, 3 * 86_400 + 4 * 3600, LimitWindow::SevenDay)),
        scoped: vec![
            ScopedLimit {
                display_name: "Sonnet".to_owned(),
                model_id: None,
                usage: window(12.0, 3 * 86_400, LimitWindow::SevenDay),
                is_active: true,
            },
            ScopedLimit {
                display_name: "Fable".to_owned(),
                model_id: None,
                usage: window(99.6, 45, LimitWindow::SevenDay),
                is_active: true,
            },
        ],
        spend: None,
        fetched_at: now() - SignedDuration::from_secs(30),
    }
}

fn state(phase: Phase, staleness: Staleness, snapshot: Option<UsageSnapshot>) -> MeterState {
    MeterState {
        snapshot,
        staleness,
        phase,
    }
}

fn healthy() -> MeterState {
    state(Phase::Polling, Staleness::Fresh, Some(snapshot()))
}

/// Every scoped model in `snapshot()` opted in — the pre-issue-#6
/// behaviour most existing tests still assert.
fn all_shown() -> HashSet<String> {
    ["Sonnet", "Fable"].into_iter().map(String::from).collect()
}

/// Pace tracking switched off entirely: no pace signal, no pace line, and no
/// per-window detail lines. The configuration most of the older specs assert
/// against, because it is the one that renders bare usage lines.
fn pace_off() -> PaceOptions {
    PaceOptions {
        pace_tracking_enabled: false,
        ..pace_default()
    }
}

/// The product default: tracking on, pace-first display off. The menu shows
/// pace; the icon still shows a percentage.
fn pace_default() -> PaceOptions {
    PaceOptions {
        weekly_pace_days: 7,
        pace_tracking_enabled: true,
        pace_first_display: false,
    }
}

/// Tracking on *and* pace-first display on: the icon switches to a ratio too.
fn pace_first() -> PaceOptions {
    PaceOptions {
        pace_first_display: true,
        ..pace_default()
    }
}

/// `menu_model` with the fixture zone. Every projected limit-hit time in the
/// specs is asserted in UTC, so a developer's local zone cannot change what
/// the assertions read.
fn menu_with(
    state: &MeterState,
    now: Timestamp,
    shown: &HashSet<String>,
    pace: PaceOptions,
    usage_mode: UsageMode,
) -> MenuModel {
    menu_model(
        state,
        now,
        MenuOptions {
            shown,
            pace,
            usage_mode,
            tz: &TimeZone::UTC,
        },
    )
}

/// `icon_state` with quota-first pace options, for the many tests that
/// only care about the base gauge.
fn icon_of(
    state: &MeterState,
    now: Timestamp,
    style: IconStyle,
    mono: bool,
    scale: Scale,
) -> IconState {
    icon_state(
        state,
        now,
        IconOptions { style, mono, scale },
        pace_off(),
        UsageMode::Auto,
    )
}

/// `menu_model` with quota-first pace options, for the many tests that
/// predate issue #16 and don't exercise the pace line. Uses `Auto`, which
/// resolves to the allowance view for the limits-bearing fixtures here.
fn menu_of(state: &MeterState, now: Timestamp, shown: &HashSet<String>) -> MenuModel {
    menu_with(state, now, shown, pace_off(), UsageMode::Auto)
}

#[test]
fn menu_lists_headline_then_scoped_windows_with_percent_and_reset() {
    let model = menu_of(&healthy(), now(), &all_shown());
    assert_eq!(
        model.usage_lines,
        vec![
            "5-hour: 42% — resets in 2h 15m",
            "7-day: 63% — resets in 3d 4h",
            "Sonnet (7-day): 12% — resets in 3d 0h",
            "Fable (7-day): 100% — resets in under 1m",
        ]
    );
    assert_eq!(model.status_line, "Updated under 1m ago");
}

#[test]
fn scoped_models_are_opt_in_and_hidden_by_default() {
    // An empty `shown` set (the default, matching `AppSettings`) hides
    // every scoped line, even though both are `is_active` in the API
    // response — only the headline windows survive.
    let model = menu_of(&healthy(), now(), &HashSet::new());
    assert_eq!(
        model.usage_lines,
        vec![
            "5-hour: 42% — resets in 2h 15m",
            "7-day: 63% — resets in 3d 4h",
        ]
    );
}

#[test]
fn toggling_one_model_on_shows_only_that_one() {
    let shown: HashSet<String> = std::iter::once("Fable".to_owned()).collect();
    let model = menu_of(&healthy(), now(), &shown);
    assert_eq!(
        model.usage_lines,
        vec![
            "5-hour: 42% — resets in 2h 15m",
            "7-day: 63% — resets in 3d 4h",
            "Fable (7-day): 100% — resets in under 1m",
        ]
    );
}

#[test]
fn opt_in_is_the_only_gate_is_active_does_not_suppress_a_line() {
    // Mirrors src/view-model.test.ts's "opt-in is the only gate" case. Live
    // payloads report `is_active: false` on every weekly window (see
    // `ScopedLimit::is_visible`), so a scoped model the user switched on must
    // still produce a usage line — while one they did not stays out.
    let mut snap = snapshot();
    snap.five_hour = None;
    snap.seven_day = None;
    snap.scoped = vec![
        ScopedLimit {
            display_name: "Sonnet".to_owned(),
            model_id: None,
            usage: window(12.0, 3 * 86_400, LimitWindow::SevenDay),
            is_active: false,
        },
        ScopedLimit {
            display_name: "Fable".to_owned(),
            model_id: None,
            usage: window(50.0, 3 * 86_400, LimitWindow::SevenDay),
            is_active: false,
        },
    ];
    let shown: HashSet<String> = std::iter::once("Sonnet".to_owned()).collect();
    let model = menu_of(
        &state(Phase::Polling, Staleness::Fresh, Some(snap)),
        now(),
        &shown,
    );
    assert_eq!(
        model.usage_lines,
        vec!["Sonnet (7-day): 12% — resets in 3d 0h"]
    );
}

#[test]
fn menu_has_no_usage_lines_without_a_snapshot() {
    let model = menu_of(
        &state(Phase::Polling, Staleness::Missing, None),
        now(),
        &all_shown(),
    );
    assert!(model.usage_lines.is_empty());
    assert_eq!(model.status_line, "Waiting for first update…");
}

#[test]
fn reset_just_in_the_past_reads_as_resets_soon() {
    let mut snap = snapshot();
    snap.five_hour = Some(window(10.0, -5, LimitWindow::FiveHour));
    snap.seven_day = None;
    snap.scoped.clear();
    let model = menu_of(
        &state(Phase::Polling, Staleness::Fresh, Some(snap)),
        now(),
        &all_shown(),
    );
    assert_eq!(model.usage_lines, vec!["5-hour: 10% — resets soon"]);
}

#[test]
fn reset_long_in_the_past_reads_as_reset_ago_not_resets_soon() {
    // A stale cached snapshot whose window elapsed days ago must not
    // read like a window about to reset within seconds.
    let mut snap = snapshot();
    snap.five_hour = Some(window(
        10.0,
        -(2 * 86_400 + 3 * 3600),
        LimitWindow::FiveHour,
    ));
    snap.seven_day = None;
    snap.scoped.clear();
    let model = menu_of(
        &state(Phase::Polling, Staleness::Stale, Some(snap)),
        now(),
        &all_shown(),
    );
    assert_eq!(model.usage_lines, vec!["5-hour: 10% — reset 2d 3h ago"]);
}

#[test]
fn status_line_reflects_every_phase() {
    // Without a snapshot the paused phases only point at the fix…
    let cases = [
        (
            Phase::AwaitingSession,
            "No session key — choose Open to set one",
        ),
        (
            Phase::SessionExpired,
            "Session expired — choose Open to update it",
        ),
    ];
    for (phase, expected) in cases {
        let model = menu_of(&state(phase, Staleness::Missing, None), now(), &all_shown());
        assert_eq!(model.status_line, expected);
    }

    // …but with a cached snapshot still on display, its age is surfaced
    // so the usage lines are never mistaken for live data.
    let mut old = snapshot();
    old.fetched_at = now() - SignedDuration::from_secs(2 * 86_400 + 3600);
    let aged_cases = [
        (
            Phase::AwaitingSession,
            "No session key — showing data from 2d 1h ago",
        ),
        (
            Phase::SessionExpired,
            "Session expired — showing data from 2d 1h ago",
        ),
    ];
    for (phase, expected) in aged_cases {
        let model = menu_of(
            &state(phase, Staleness::Stale, Some(old.clone())),
            now(),
            &all_shown(),
        );
        assert_eq!(model.status_line, expected);
    }

    let degraded = state(Phase::Degraded, Staleness::Fresh, Some(snapshot()));
    assert_eq!(
        menu_of(&degraded, now(), &all_shown()).status_line,
        "Connection trouble — data from under 1m ago"
    );
    let degraded_empty = state(Phase::Degraded, Staleness::Missing, None);
    assert_eq!(
        menu_of(&degraded_empty, now(), &all_shown()).status_line,
        "Connection trouble — retrying"
    );
}

#[test]
fn stale_data_is_called_out_with_its_age() {
    let mut snap = snapshot();
    snap.fetched_at = now() - SignedDuration::from_secs(25 * 60);
    let stale = state(Phase::Polling, Staleness::Stale, Some(snap));
    assert_eq!(
        menu_of(&stale, now(), &all_shown()).status_line,
        "Stale — updated 25m ago"
    );
}

#[test]
fn short_durations_cover_all_magnitudes() {
    assert_eq!(short_duration(30), "under 1m");
    assert_eq!(short_duration(12 * 60), "12m");
    assert_eq!(short_duration(2 * 3600 + 15 * 60), "2h 15m");
    assert_eq!(short_duration(3 * 86_400 + 4 * 3600), "3d 4h");
    assert_eq!(short_duration(-10), "under 1m");
}

#[test]
fn icon_state_uses_the_snapshot_when_present() {
    let icon = icon_of(&healthy(), now(), IconStyle::Battery, true, Scale::X2);
    assert_eq!(icon.percent, 42);
    assert!(icon.mono);
    // The icon colour follows the session (5-hour) window shown as the
    // number — 42% is safe — matching ClaudeMeter's session-driven menu
    // bar, even though Fable is pacing near 100%. (The popover cards and
    // the status line still surface that worst window.)
    assert_eq!(icon.status, UsageStatus::Safe);
}

#[test]
fn icon_state_is_an_empty_safe_gauge_without_a_snapshot() {
    let empty = state(Phase::AwaitingSession, Staleness::Missing, None);
    let icon = icon_of(&empty, now(), IconStyle::Battery, false, Scale::X1);
    assert_eq!(icon.percent, 0);
    assert_eq!(icon.status, UsageStatus::Safe);
    assert!(!icon.at_risk);
}

#[test]
fn icon_state_carries_the_requested_style_through_snapshot_and_empty_paths() {
    // A style switch (Settings, issue #9) must show up in the very next
    // `icon_state`, with or without a live snapshot — that is what lets
    // the tray apply it live without a restart.
    let icon = icon_of(&healthy(), now(), IconStyle::Gauge, false, Scale::X2);
    assert_eq!(icon.style, IconStyle::Gauge);

    let empty = state(Phase::AwaitingSession, Staleness::Missing, None);
    let icon = icon_of(&empty, now(), IconStyle::Segments, false, Scale::X1);
    assert_eq!(icon.style, IconStyle::Segments);
}

#[test]
fn identical_states_debounce_to_a_noop_once_committed() {
    let mut diff = TrayDiff::default();
    let icon = icon_of(&healthy(), now(), IconStyle::Battery, false, Scale::X2);
    let menu = menu_of(&healthy(), now(), &all_shown());

    let first = diff.plan(icon, &menu);
    assert_eq!(first.icon, Some(icon));
    assert_eq!(first.menu, Some(menu.clone()));
    diff.commit_icon(icon);
    diff.commit_menu(menu.clone());

    let second = diff.plan(icon, &menu);
    assert_eq!(
        second,
        TrayPlan {
            icon: None,
            menu: None
        }
    );
}

#[test]
fn uncommitted_plan_is_replanned_so_failed_applies_are_retried() {
    let mut diff = TrayDiff::default();
    let icon = icon_of(&healthy(), now(), IconStyle::Battery, false, Scale::X2);
    let menu = menu_of(&healthy(), now(), &all_shown());

    // The caller failed to apply (render/rebuild error) and committed
    // nothing — the same state must be planned again, not swallowed.
    let first = diff.plan(icon, &menu);
    assert_eq!(first.icon, Some(icon));
    let second = diff.plan(icon, &menu);
    assert_eq!(second.icon, Some(icon));
    assert_eq!(second.menu, Some(menu.clone()));

    // Committing only the icon leaves the menu pending, and vice versa.
    diff.commit_icon(icon);
    let third = diff.plan(icon, &menu);
    assert_eq!(third.icon, None);
    assert_eq!(third.menu, Some(menu));
}

#[test]
fn menu_only_change_leaves_the_icon_untouched() {
    let mut diff = TrayDiff::default();
    let icon = icon_of(&healthy(), now(), IconStyle::Battery, false, Scale::X2);
    let menu = menu_of(&healthy(), now(), &all_shown());
    diff.commit_icon(icon);
    diff.commit_menu(menu);

    // A minute later the icon key is identical but the age text moved.
    let later = now() + SignedDuration::from_secs(60);
    let plan = diff.plan(icon, &menu_of(&healthy(), later, &all_shown()));
    assert_eq!(plan.icon, None);
    assert_eq!(plan.menu.unwrap().status_line, "Updated 1m ago".to_owned());
}

#[test]
fn icon_change_is_planned_even_when_the_menu_is_identical() {
    let mut diff = TrayDiff::default();
    let menu = menu_of(&healthy(), now(), &all_shown());
    let icon = icon_of(&healthy(), now(), IconStyle::Battery, false, Scale::X2);
    diff.commit_icon(icon);
    diff.commit_menu(menu.clone());

    let mut hotter = icon;
    hotter.percent = 43;
    let plan = diff.plan(hotter, &menu);
    assert_eq!(plan.icon, Some(hotter));
    assert_eq!(plan.menu, None);
}

// The cost/spend (Enterprise) view-model tests live in a sibling submodule to
// keep this file under the 700-line hard gate.
mod cost;
mod pace;
