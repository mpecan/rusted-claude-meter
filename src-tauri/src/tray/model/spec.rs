#![allow(clippy::unwrap_used)]

use super::*;
use jiff::SignedDuration;
use meter_core::{ScopedLimit, UsageSnapshot, UsageStatus};
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

/// Quota-first (pre-issue-#16) defaults: no pace signal computed at all.
fn pace_off() -> PaceOptions {
    PaceOptions {
        weekly_pace_days: 7,
        pace_first_display: false,
    }
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
    icon_state(state, now, IconOptions { style, mono, scale }, pace_off())
}

/// `menu_model` with quota-first pace options, for the many tests that
/// predate issue #16 and don't exercise the pace line.
fn menu_of(state: &MeterState, now: Timestamp, shown: &HashSet<String>) -> MenuModel {
    menu_model(state, now, shown, pace_off())
}

/// A session window burning fast enough to produce a hot `PaceSignal`:
/// 60% used at a quarter of the 5-hour window elapsed (expected 25%,
/// ratio 60/25 = 2.4).
fn hot_session_state() -> MeterState {
    state(
        Phase::Polling,
        Staleness::Fresh,
        Some(UsageSnapshot {
            five_hour: Some(window(60.0, 225 * 60, LimitWindow::FiveHour)),
            seven_day: None,
            scoped: vec![],
            fetched_at: now(),
        }),
    )
}

#[test]
fn pace_first_display_off_never_computes_a_signal_even_when_burning_hot() {
    // Gating (issue #16): quota-first mode must not overlay a pace badge
    // or tooltip, no matter how off-pace the underlying window is.
    let pace = pace_off();
    let icon = icon_state(
        &hot_session_state(),
        now(),
        IconOptions {
            style: IconStyle::Battery,
            mono: false,
            scale: Scale::X2,
        },
        pace,
    );
    assert_eq!(icon.pace_kind, None);
    assert_eq!(icon.pace_ratio, None);

    let menu = menu_model(&hot_session_state(), now(), &HashSet::new(), pace);
    assert_eq!(menu.pace_line, None);
}

#[test]
fn pace_first_display_on_shows_the_ratio_even_when_no_window_is_off_pace() {
    // The stock `snapshot()` fixture is on-pace on both headline windows
    // (session ratio 41.5/55 ≈ 0.75, weekly ≈ 1.15 — neither hot nor cold),
    // so the hybrid `PaceSignal` is `None` and no flame/snowflake badge or
    // tooltip appears. But pace-first display still swaps the primary metric
    // to the ratio: upstream's `paceSignal?.ratio ?? session.paceRatio ??
    // weekly.paceRatio` falls back to the plain session ratio, in its band
    // colour, so the icon shows a ratio even when nothing is off pace.
    let pace = PaceOptions {
        weekly_pace_days: 7,
        pace_first_display: true,
    };
    let icon = icon_state(
        &healthy(),
        now(),
        IconOptions {
            style: IconStyle::Battery,
            mono: false,
            scale: Scale::X2,
        },
        pace,
    );
    // No hybrid signal -> no badge, but the fallback session ratio drives
    // the primary metric and its band colour.
    assert_eq!(icon.pace_kind, None);
    let ratio = icon.pace_ratio.unwrap();
    assert!(
        (ratio - 41.5 / 55.0).abs() < 1e-9,
        "expected the session pace ratio, got {ratio}"
    );
    assert_eq!(icon.pace_band, Some(meter_core::PaceBand::Underuse));

    // The tooltip/pace line stays hybrid-signal-only (upstream `toolTip =
    // paceSignal?.tooltip`), so an on-pace snapshot produces no pace line.
    let menu = menu_model(&healthy(), now(), &all_shown(), pace);
    assert_eq!(menu.pace_line, None);
}

#[test]
fn pace_first_display_on_overlays_the_icon_badge_and_the_menu_pace_line() {
    let pace = PaceOptions {
        weekly_pace_days: 7,
        pace_first_display: true,
    };
    let icon = icon_state(
        &hot_session_state(),
        now(),
        IconOptions {
            style: IconStyle::Battery,
            mono: false,
            scale: Scale::X2,
        },
        pace,
    );
    assert_eq!(icon.pace_kind, Some(meter_core::PaceKind::Hot));
    assert_eq!(icon.pace_ratio, Some(2.4));

    let menu = menu_model(&hot_session_state(), now(), &HashSet::new(), pace);
    assert!(
        menu.pace_line.is_some(),
        "hot session must produce a pace line"
    );
    let pace_line = menu.pace_line.unwrap();
    assert!(
        pace_line.starts_with("Used 60% vs 25% expected by now"),
        "unexpected pace line: {pace_line}"
    );
    assert!(pace_line.contains("5-hour window"), "{pace_line}");
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
fn requires_both_is_active_and_opt_in_either_gate_alone_is_not_enough() {
    // Mirrors src/view-model.test.ts's "requires both is_active and
    // opt-in" case: both models are opted in, but "CodeOnly" is not
    // `is_active`, so it must not produce a usage line even though it's
    // in `shown`.
    let mut snap = snapshot();
    snap.five_hour = None;
    snap.seven_day = None;
    snap.scoped = vec![
        ScopedLimit {
            display_name: "Sonnet".to_owned(),
            model_id: None,
            usage: window(12.0, 3 * 86_400, LimitWindow::SevenDay),
            is_active: true,
        },
        ScopedLimit {
            display_name: "CodeOnly".to_owned(),
            model_id: None,
            usage: window(50.0, 3 * 86_400, LimitWindow::SevenDay),
            is_active: false,
        },
    ];
    let shown: HashSet<String> = ["Sonnet", "CodeOnly"]
        .into_iter()
        .map(String::from)
        .collect();
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
