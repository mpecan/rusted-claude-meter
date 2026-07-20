#![allow(clippy::unwrap_used)]

//! Cost/spend tray view-model tests. Split out of `spec.rs` to keep both files
//! under the 700-line hard gate; shares that module's fixtures and helpers via
//! `use super::*`.

use super::*;
use pretty_assertions::assert_eq;

/// `minor`-unit money in a 2-decimal currency (EUR/USD in these tests).
fn money(minor: i64, currency: &str) -> Money {
    Money {
        minor,
        currency: currency.to_owned(),
        exponent: 2,
    }
}

/// A token/cost account: no allowance windows, a spend object instead. `Auto`
/// resolves this to the cost view (no limits reported).
fn cost_state(spend: Spend) -> MeterState {
    state(
        Phase::Polling,
        Staleness::Fresh,
        Some(UsageSnapshot {
            five_hour: None,
            seven_day: None,
            scoped: vec![],
            spend: Some(Box::new(spend)),
            fetched_at: now() - SignedDuration::from_secs(30),
        }),
    )
}

/// €500.00 used of a €2000.00 budget (with an equal hard cap) — the real
/// captured spend shape, scaled to a legible 25%.
fn spend_with_cap() -> Spend {
    Spend {
        used: Some(money(50_000, "EUR")),
        limit: Some(money(200_000, "EUR")),
        cap: Some(money(200_000, "EUR")),
        enabled: true,
    }
}

fn cost_icon_of(state: &MeterState, mode: UsageMode) -> IconState {
    icon_state(
        state,
        now(),
        IconOptions {
            style: IconStyle::Battery,
            mono: false,
            scale: Scale::X2,
        },
        pace_off(),
        mode,
    )
}

#[test]
fn format_money_is_currency_and_exponent_aware() {
    assert_eq!(format_money(&money(0, "USD")), "$0.00");
    assert_eq!(format_money(&money(5, "USD")), "$0.05");
    assert_eq!(format_money(&money(50_000, "USD")), "$500.00");
    assert_eq!(format_money(&money(35, "EUR")), "€0.35");
    assert_eq!(format_money(&money(200_000, "EUR")), "€2000.00");
    // A negative figure should not normally occur, but must stay legible.
    assert_eq!(format_money(&money(-1_234, "USD")), "-$12.34");
    // A currency with no known glyph falls back to the ISO code suffix.
    assert_eq!(
        format_money(&Money {
            minor: 100_000,
            currency: "SEK".to_owned(),
            exponent: 2,
        }),
        "1000.00 SEK"
    );
    // A zero-decimal currency shows no fractional part.
    assert_eq!(
        format_money(&Money {
            minor: 500,
            currency: "JPY".to_owned(),
            exponent: 0,
        }),
        "¥500"
    );
}

#[test]
fn cost_menu_shows_spend_with_its_share_of_the_budget() {
    // Auto on a no-limits account resolves to the cost view: the spend to date
    // and its share of the spend limit, in the account's own currency.
    let menu = menu_model(
        &cost_state(spend_with_cap()),
        now(),
        &all_shown(),
        pace_off(),
        UsageMode::Auto,
    );
    assert_eq!(
        menu.usage_lines,
        vec!["Spend €500.00 this period · 25% of €2000.00".to_owned()]
    );
    // Freshness still drives the status line; cost mode never emits a pace line.
    assert_eq!(menu.status_line, "Updated under 1m ago");
    assert_eq!(menu.pace_line, None);
}

#[test]
fn cost_menu_without_a_budget_shows_bare_spend() {
    let spend = Spend {
        used: Some(money(4_200, "USD")),
        ..Spend::default()
    };
    let menu = menu_model(
        &cost_state(spend),
        now(),
        &all_shown(),
        pace_off(),
        UsageMode::Auto,
    );
    assert_eq!(
        menu.usage_lines,
        vec!["Spend $42.00 this period".to_owned()]
    );
}

#[test]
fn cost_menu_falls_back_to_the_cap_when_there_is_no_limit() {
    // A spend with only a hard cap (no separate limit) still gauges against the
    // cap — the `.or(cap)` fallback in both the gauge and the menu annotation.
    let spend = Spend {
        used: Some(money(50_000, "USD")),
        limit: None,
        cap: Some(money(200_000, "USD")),
        enabled: true,
    };
    let menu = menu_model(
        &cost_state(spend),
        now(),
        &all_shown(),
        pace_off(),
        UsageMode::Auto,
    );
    assert_eq!(
        menu.usage_lines,
        vec!["Spend $500.00 this period · 25% of $2000.00".to_owned()]
    );
}

#[test]
fn cost_menu_is_empty_for_an_unsurfaced_spend_stub() {
    // The `{"unsurfaced": true}` stub decodes to an all-None spend: no usable
    // figure, so the menu simply shows no usage lines (never a bogus $0.00).
    let menu = menu_model(
        &cost_state(Spend::default()),
        now(),
        &all_shown(),
        pace_off(),
        UsageMode::Auto,
    );
    assert!(menu.usage_lines.is_empty());
}

#[test]
fn cost_icon_gauges_the_spend_budget_fraction() {
    // A budget turns spend-to-date into a percentage gauge: 500/2000 = 25%, safe.
    let icon = cost_icon_of(&cost_state(spend_with_cap()), UsageMode::Auto);
    assert_eq!(icon.percent, 25);
    assert_eq!(icon.status, UsageStatus::Safe);
    assert_eq!(icon.pace_ratio, None);
}

#[test]
fn cost_icon_without_a_budget_stays_an_empty_gauge() {
    let spend = Spend {
        used: Some(money(4_200, "USD")),
        ..Spend::default()
    };
    let icon = cost_icon_of(&cost_state(spend), UsageMode::Auto);
    // No denominator -> the empty gauge; the "$" figure lives in the menu.
    assert_eq!(icon.percent, 0);
    assert_eq!(icon.status, UsageStatus::Safe);
}

#[test]
fn pinning_allowance_on_a_cost_account_hides_spend() {
    // A cost account pinned to the allowance view shows the (absent) percentage
    // windows, not the spend lines — the user's override wins over detection.
    let menu = menu_model(
        &cost_state(spend_with_cap()),
        now(),
        &all_shown(),
        pace_off(),
        UsageMode::Allowance,
    );
    assert!(menu.usage_lines.is_empty());
    let icon = cost_icon_of(&cost_state(spend_with_cap()), UsageMode::Allowance);
    assert_eq!(icon.percent, 0);
}

#[test]
fn pinning_cost_on_an_allowance_account_without_spend_shows_no_lines() {
    // Pinning Cost on a limits-bearing snapshot that carries no spend object
    // yields no spend lines (rather than the percentage windows) — the mode is
    // honoured, there is just nothing to show. Crucially the icon must NOT leak
    // the allowance percentage gauge either: it stays the empty safe gauge.
    let menu = menu_model(&healthy(), now(), &all_shown(), pace_off(), UsageMode::Cost);
    assert!(menu.usage_lines.is_empty());
    let icon = cost_icon_of(&healthy(), UsageMode::Cost);
    assert_eq!(icon.percent, 0);
    assert_eq!(icon.status, UsageStatus::Safe);
    assert_eq!(icon.pace_ratio, None);
}

#[test]
fn pinning_cost_on_an_allowance_account_with_uncapped_spend_gauges_nothing() {
    // An allowance account (windows present) also carrying a spend object with
    // no budget, pinned to Cost: the menu shows the bare spend figure and the
    // icon stays the empty gauge — never the allowance percentage (42% here).
    let mut snap = snapshot();
    snap.spend = Some(Box::new(Spend {
        used: Some(money(4_200, "USD")),
        ..Spend::default()
    }));
    let st = state(Phase::Polling, Staleness::Fresh, Some(snap));
    let menu = menu_model(&st, now(), &all_shown(), pace_off(), UsageMode::Cost);
    assert_eq!(
        menu.usage_lines,
        vec!["Spend $42.00 this period".to_owned()]
    );
    let icon = cost_icon_of(&st, UsageMode::Cost);
    assert_eq!(icon.percent, 0);
    assert_eq!(icon.status, UsageStatus::Safe);
}

#[test]
fn allowance_account_under_auto_is_unaffected_by_cost_support() {
    // The stock limits-bearing fixture under Auto still renders the percentage
    // windows exactly as before — cost support must not disturb it.
    let menu = menu_model(&healthy(), now(), &all_shown(), pace_off(), UsageMode::Auto);
    assert_eq!(
        menu.usage_lines,
        vec![
            "5-hour: 42% — resets in 2h 15m",
            "7-day: 63% — resets in 3d 4h",
            "Sonnet (7-day): 12% — resets in 3d 0h",
            "Fable (7-day): 100% — resets in under 1m",
        ]
    );
    let icon = cost_icon_of(&healthy(), UsageMode::Auto);
    assert_eq!(icon.percent, 42);
}
