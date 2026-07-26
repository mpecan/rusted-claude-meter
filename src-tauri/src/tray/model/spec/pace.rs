//! Specs for everything pace-related in the tray (issue #16): which of the
//! two settings gates the icon, which gates the menu, and the menu's
//! per-window pace/projection detail lines.
//!
//! Split out of `spec.rs` for the same reason `cost.rs` was — that file is
//! already over the 500-line soft gate.
//!
//! Every case pins an exact string. The detail line is the only tray text that
//! renders a wall-clock time, so `menu_with` fixes the zone to UTC; without
//! that these assertions would read differently on every developer's machine.

#![allow(clippy::unwrap_used)]

use super::*;
// Explicit, because the glob above re-exports it and the std prelude also
// defines one — ambiguous otherwise.
use pretty_assertions::assert_eq;

/// The usage lines a state produces, so a spec can assert on the pair
/// (usage line, detail line) without restating the whole `MenuModel`.
fn lines(state: &MeterState, pace: PaceOptions) -> Vec<String> {
    menu_with(state, now(), &all_shown(), pace, UsageMode::Auto).usage_lines
}

/// A state carrying exactly one headline window, so the assertions are about
/// that window and nothing else.
fn one_window(window: UsageWindow) -> MeterState {
    let (five_hour, seven_day) = match window.window {
        LimitWindow::FiveHour => (Some(window), None),
        LimitWindow::SevenDay => (None, Some(window)),
    };
    state(
        Phase::Polling,
        Staleness::Fresh,
        Some(UsageSnapshot {
            five_hour,
            seven_day,
            scoped: vec![],
            spend: None,
            fetched_at: now(),
        }),
    )
}

#[test]
fn overuse_projects_the_limit_hit_as_a_wall_clock_time() {
    // 50% used one hour into a five-hour window: expected 20%, ratio 2.5, and
    // at that rate the remaining 50% is gone an hour from now (13:00Z).
    let lines = lines(
        &one_window(window(50.0, 4 * 3600, LimitWindow::FiveHour)),
        pace_default(),
    );
    assert_eq!(
        lines,
        vec![
            "5-hour: 50% — resets in 4h 0m".to_owned(),
            "    2.5× pace · 20% expected · hits limit ~1:00 PM".to_owned(),
        ]
    );
}

#[test]
fn a_hit_on_a_later_day_keeps_the_date() {
    // 40% used two days into a seven-day window hits the limit on day five —
    // a bare "12:00 PM" there would read as today, so the date stays.
    let lines = lines(
        &one_window(window(40.0, 5 * 86_400, LimitWindow::SevenDay)),
        pace_default(),
    );
    assert_eq!(
        lines,
        vec![
            "7-day: 40% — resets in 5d 0h".to_owned(),
            "    1.4× pace · 29% expected · hits limit ~Jul 20, 12:00 PM".to_owned(),
        ]
    );
}

#[test]
fn a_maxed_window_says_the_limit_is_reached() {
    // `projected_limit_date` returns None at 100% by design, so without an
    // explicit first case this would fall through to "on pace to end at ~100%"
    // — which is what the popover calls "Limit reached". Same data, so the two
    // surfaces have to agree.
    let lines = lines(
        &one_window(window(100.0, 3600, LimitWindow::FiveHour)),
        pace_default(),
    );
    assert_eq!(
        lines,
        vec![
            "5-hour: 100% — resets in 1h 0m".to_owned(),
            "    1.2× pace · 80% expected · limit reached".to_owned(),
        ]
    );
}

#[test]
fn underuse_projects_where_the_window_ends_instead() {
    // 24% used four hours into a five-hour window: nowhere near the limit, so
    // the useful projection is the end percentage, not a hit time.
    let lines = lines(
        &one_window(window(24.0, 3600, LimitWindow::FiveHour)),
        pace_default(),
    );
    assert_eq!(
        lines,
        vec![
            "5-hour: 24% — resets in 1h 0m".to_owned(),
            "    0.3× pace · 80% expected · on pace to end at ~30%".to_owned(),
        ]
    );
}

#[test]
fn no_detail_line_while_there_is_nothing_to_say_yet() {
    // Five minutes into a five-hour window *and* barely any usage: under both
    // the elapsed grace and the usage floor, so there is no meaningful ratio
    // and the menu stays one line per window rather than padding it with noise.
    let lines = lines(
        &one_window(window(1.0, 4 * 3600 + 55 * 60, LimitWindow::FiveHour)),
        pace_default(),
    );
    assert_eq!(lines, vec!["5-hour: 1% — resets in 4h 55m".to_owned()]);
}

#[test]
fn a_front_loaded_burst_gets_its_detail_line_before_the_elapsed_grace() {
    // Same five minutes in, but 10% already spent — past the usage floor, so
    // the ratio surfaces immediately instead of waiting out the grace (#48).
    // The menu is the whole Linux surface, so this is where that warning lands.
    let lines = lines(
        &one_window(window(10.0, 4 * 3600 + 55 * 60, LimitWindow::FiveHour)),
        pace_default(),
    );
    assert_eq!(
        lines,
        vec![
            "5-hour: 10% — resets in 4h 55m".to_owned(),
            "    6.0× pace · 2% expected · hits limit ~12:45 PM".to_owned(),
        ]
    );
}

#[test]
fn weekly_pace_days_changes_the_weekly_detail_line() {
    // The same window paced over five working days instead of seven: the
    // expected-by-now figure rises, so the ratio falls and the projected end
    // percentage rises with the shorter horizon.
    let state = one_window(window(50.0, 302_400, LimitWindow::SevenDay));

    assert_eq!(
        lines(&state, pace_default())[1],
        "    1.0× pace · 50% expected · on pace to end at ~100%"
    );

    let five_day = PaceOptions {
        weekly_pace_days: 5,
        ..pace_default()
    };
    assert_eq!(
        lines(&state, five_day)[1],
        "    0.7× pace · 70% expected · on pace to end at ~71%"
    );
}

#[test]
fn tracking_off_suppresses_every_detail_line() {
    let state = one_window(window(50.0, 4 * 3600, LimitWindow::FiveHour));
    assert_eq!(
        lines(&state, pace_off()),
        vec!["5-hour: 50% — resets in 4h 0m".to_owned()],
        "the master switch must remove the detail line, not just the pace line"
    );
}

#[test]
fn pace_first_display_does_not_change_the_detail_lines() {
    // The decoupling, from the menu's side: an icon preference must not add or
    // remove menu content.
    let state = one_window(window(50.0, 4 * 3600, LimitWindow::FiveHour));
    assert_eq!(lines(&state, pace_default()), lines(&state, pace_first()));
}

#[test]
fn every_window_gets_its_own_detail_line_in_order() {
    // Headline windows first, then the opted-in scoped limits, each detail
    // line directly under the window it belongs to.
    let menu = menu_with(
        &healthy(),
        now(),
        &all_shown(),
        pace_default(),
        UsageMode::Auto,
    );
    assert_eq!(
        menu.usage_lines,
        vec![
            "5-hour: 42% — resets in 2h 15m".to_owned(),
            "    0.8× pace · 55% expected · on pace to end at ~75%".to_owned(),
            "7-day: 63% — resets in 3d 4h".to_owned(),
            "    1.2× pace · 55% expected · hits limit ~Jul 19, 6:01 PM".to_owned(),
            "Sonnet (7-day): 12% — resets in 3d 0h".to_owned(),
            "    0.2× pace · 57% expected · on pace to end at ~21%".to_owned(),
            "Fable (7-day): 100% — resets in under 1m".to_owned(),
            "    1.0× pace · 100% expected · on pace to end at ~100%".to_owned(),
        ]
    );
}

#[test]
fn scoped_limits_left_switched_off_contribute_no_lines_at_all() {
    // The opt-in gate applies to the detail line too — a hidden model must not
    // leak into the menu through its pace line.
    let menu = menu_with(
        &healthy(),
        now(),
        &HashSet::new(),
        pace_default(),
        UsageMode::Auto,
    );
    assert!(
        menu.usage_lines.iter().all(|line| !line.contains("Sonnet")),
        "hidden scoped model leaked: {:?}",
        menu.usage_lines
    );
    assert_eq!(
        menu.usage_lines.len(),
        4,
        "two headline windows, two details"
    );
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
            spend: None,
            fetched_at: now(),
        }),
    )
}

#[test]
fn pace_tracking_off_never_computes_a_signal_even_when_burning_hot() {
    // Gating (issue #16): with the master switch off, nothing pace-related is
    // computed — no badge, no ratio, no tooltip — however off-pace the
    // underlying window is.
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
        UsageMode::Auto,
    );
    assert_eq!(icon.pace_kind, None);
    assert_eq!(icon.pace_ratio, None);

    let menu = menu_with(
        &hot_session_state(),
        now(),
        &HashSet::new(),
        pace,
        UsageMode::Auto,
    );
    assert_eq!(
        menu.usage_lines,
        vec!["5-hour: 60% — resets in 3h 45m".to_owned()],
        "tracking off leaves the bare usage line and nothing else"
    );
}

#[test]
fn pace_first_display_gates_the_icon_but_not_the_menu() {
    // The decoupling. `pace_first_display` is an *icon* preference: with it off
    // but tracking on, the icon stays a percentage gauge with no badge, while
    // the menu — the whole surface on Linux — still gets the pace line.
    let pace = pace_default();
    let icon = icon_state(
        &hot_session_state(),
        now(),
        IconOptions {
            style: IconStyle::Battery,
            mono: false,
            scale: Scale::X2,
        },
        pace,
        UsageMode::Auto,
    );
    assert_eq!(icon.pace_kind, None, "quota-first icon shows no badge");
    assert_eq!(icon.pace_ratio, None, "quota-first icon shows no ratio");

    let menu = menu_with(
        &hot_session_state(),
        now(),
        &HashSet::new(),
        pace,
        UsageMode::Auto,
    );
    assert_eq!(
        menu.usage_lines,
        vec![
            "5-hour: 60% — resets in 3h 45m".to_owned(),
            "    2.4× pace · 25% expected · hits limit ~12:50 PM".to_owned(),
        ],
        "tracking on must give the menu its pace detail regardless of pace-first"
    );
}

#[test]
fn pace_first_display_on_shows_the_ratio_even_when_no_window_is_off_pace() {
    // Both headline windows pace on the sustainable side (ratio 0.9), so
    // neither is hot (>1.0×) nor cold (<0.8×): the hybrid `PaceSignal` is
    // `None` and no flame/snowflake badge or tooltip appears. But pace-first
    // display still swaps the primary metric to the ratio: upstream's
    // `paceSignal?.ratio ?? session.paceRatio ?? weekly.paceRatio` falls back
    // to the plain session ratio, in its band colour.
    let on_pace = state(
        Phase::Polling,
        Staleness::Fresh,
        Some(UsageSnapshot {
            // 45% used at 50% elapsed -> ratio 0.9.
            five_hour: Some(window(45.0, 150 * 60, LimitWindow::FiveHour)),
            // 45% used at 50% elapsed -> ratio 0.9 (neither hot nor cold).
            seven_day: Some(window(45.0, 3 * 86_400 + 12 * 3600, LimitWindow::SevenDay)),
            scoped: vec![],
            spend: None,
            fetched_at: now(),
        }),
    );
    let pace = pace_first();
    let icon = icon_state(
        &on_pace,
        now(),
        IconOptions {
            style: IconStyle::Battery,
            mono: false,
            scale: Scale::X2,
        },
        pace,
        UsageMode::Auto,
    );
    // No hybrid signal -> no badge, but the fallback session ratio drives
    // the primary metric and its band colour.
    assert_eq!(icon.pace_kind, None);
    let ratio = icon.pace_ratio.unwrap();
    assert!(
        (ratio - 45.0 / 50.0).abs() < 1e-9,
        "expected the session pace ratio, got {ratio}"
    );
    assert_eq!(icon.pace_band, Some(meter_core::PaceBand::Sustainable));

    // The menu is unaffected either way — it has no headline pace line to
    // gate, only the per-window detail lines, which pace-first does not touch.
    let menu = menu_with(&on_pace, now(), &all_shown(), pace, UsageMode::Auto);
    assert_eq!(
        menu.usage_lines,
        menu_with(
            &on_pace,
            now(),
            &all_shown(),
            pace_default(),
            UsageMode::Auto
        )
        .usage_lines
    );
}

#[test]
fn pace_first_display_on_overlays_the_icon_badge() {
    let pace = pace_first();
    let icon = icon_state(
        &hot_session_state(),
        now(),
        IconOptions {
            style: IconStyle::Battery,
            mono: false,
            scale: Scale::X2,
        },
        pace,
        UsageMode::Auto,
    );
    assert_eq!(icon.pace_kind, Some(meter_core::PaceKind::Hot));
    assert_eq!(icon.pace_ratio, Some(2.4));
}
