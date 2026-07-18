//! Pure tray view-model: no Tauri types, no I/O, fully unit-testable.
//!
//! Everything the tray shows is computed here from a [`MeterState`] plus a
//! `now` timestamp: the icon state to render, the menu's status line and the
//! live usage lines (one per window — 5-hour, 7-day, each scoped model).
//! [`TrayDiff`] is the debounce gate: it remembers what the tray last
//! successfully applied (the caller commits each part only after the tray
//! call succeeded) and turns a fresh view-model into the minimal
//! [`TrayPlan`], so identical consecutive states touch neither the icon nor
//! the menu (no flicker, no redundant `set_icon` calls).

use std::collections::HashSet;

use jiff::Timestamp;
use meter_core::{LimitWindow, UsageWindow};
use meter_render::{IconState, IconStyle, Scale, round_percent};

use crate::scheduler::{MeterState, Phase, Staleness};

/// Everything the tray menu displays, as plain strings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuModel {
    /// One-line summary of the scheduler phase / data freshness.
    pub status_line: String,
    /// One line per reported window: headline first, then scoped, API order.
    pub usage_lines: Vec<String>,
}

/// The icon to render for a state: the live gauge when a snapshot exists,
/// an empty safe gauge otherwise. `style` is the user's current choice from
/// Settings — passed in rather than hardcoded so switching styles takes
/// effect on the very next state (no restart needed).
pub fn icon_state(
    state: &MeterState,
    now: Timestamp,
    style: IconStyle,
    mono: bool,
    scale: Scale,
) -> IconState {
    state.snapshot.as_ref().map_or(
        IconState {
            style,
            percent: 0,
            secondary_percent: 0,
            status: meter_core::UsageStatus::Safe,
            at_risk: false,
            mono,
            scale,
        },
        |snapshot| IconState::from_snapshot(snapshot, now, style, mono, scale),
    )
}

/// Build the menu view-model for a state at `now`.
///
/// `shown` is the user's opt-in set of scoped-model display names from
/// Settings (issue #6): a scoped limit only becomes a usage line once its
/// name is in this set, even when the API reports it as `is_active`. Empty
/// by default, so a freshly reported model stays out of the tray menu until
/// switched on.
pub fn menu_model(state: &MeterState, now: Timestamp, shown: &HashSet<String>) -> MenuModel {
    let mut usage_lines = Vec::new();
    if let Some(snapshot) = &state.snapshot {
        if let Some(window) = &snapshot.five_hour {
            usage_lines.push(usage_line(window_label(window.window), window, now));
        }
        if let Some(window) = &snapshot.seven_day {
            usage_lines.push(usage_line(window_label(window.window), window, now));
        }
        for limit in &snapshot.scoped {
            if !limit.is_visible(shown) {
                continue;
            }
            let label = format!(
                "{} ({})",
                limit.display_name,
                window_label(limit.usage.window)
            );
            usage_lines.push(usage_line(&label, &limit.usage, now));
        }
    }
    MenuModel {
        status_line: status_line(state, now),
        usage_lines,
    }
}

const fn window_label(window: LimitWindow) -> &'static str {
    match window {
        LimitWindow::FiveHour => "5-hour",
        LimitWindow::SevenDay => "7-day",
    }
}

/// A reset moment this recently in the past still reads "resets soon";
/// beyond it the line says how long ago the window reset — the cue that the
/// numbers come from a stale snapshot, not live data.
const RESET_SOON_GRACE_SECS: i64 = 5 * 60;

/// "5-hour: 42% — resets in 2h 15m"
fn usage_line(label: &str, window: &UsageWindow, now: Timestamp) -> String {
    let percent = round_percent(window.utilization);
    let remaining = window.resets_at.duration_since(now).as_secs();
    if remaining > 0 {
        format!(
            "{label}: {percent}% — resets in {}",
            short_duration(remaining)
        )
    } else if remaining > -RESET_SOON_GRACE_SECS {
        format!("{label}: {percent}% — resets soon")
    } else {
        format!(
            "{label}: {percent}% — reset {} ago",
            short_duration(-remaining)
        )
    }
}

/// The one-line phase/freshness summary. Whenever a cached snapshot is
/// still shown while polling is paused or failing, its age is surfaced
/// here so the usage lines are never presented as current data.
fn status_line(state: &MeterState, now: Timestamp) -> String {
    let age = state
        .snapshot
        .as_ref()
        .map(|snapshot| short_duration(now.duration_since(snapshot.fetched_at).as_secs()));
    match (state.phase, age) {
        (Phase::AwaitingSession, None) => "No session key — choose Open to set one".to_owned(),
        (Phase::AwaitingSession, Some(age)) => {
            format!("No session key — showing data from {age} ago")
        }
        (Phase::SessionExpired, None) => "Session expired — choose Open to update it".to_owned(),
        (Phase::SessionExpired, Some(age)) => {
            format!("Session expired — showing data from {age} ago")
        }
        (Phase::Degraded, None) => "Connection trouble — retrying".to_owned(),
        (Phase::Degraded, Some(age)) => format!("Connection trouble — data from {age} ago"),
        (Phase::Polling, None) => "Waiting for first update…".to_owned(),
        (Phase::Polling, Some(age)) => {
            if state.staleness == Staleness::Stale {
                format!("Stale — updated {age} ago")
            } else {
                format!("Updated {age} ago")
            }
        }
    }
}

/// Coarse human duration: "3d 4h", "2h 15m", "12m", "under 1m".
fn short_duration(total_secs: i64) -> String {
    let secs = total_secs.max(0);
    let days = secs / 86_400;
    let hours = (secs % 86_400) / 3_600;
    let minutes = (secs % 3_600) / 60;
    if days > 0 {
        format!("{days}d {hours}h")
    } else if hours > 0 {
        format!("{hours}h {minutes}m")
    } else if minutes > 0 {
        format!("{minutes}m")
    } else {
        "under 1m".to_owned()
    }
}

/// What the tray must actually touch for one state change. `None` fields
/// mean "already showing this — do nothing".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrayPlan {
    pub icon: Option<IconState>,
    pub menu: Option<MenuModel>,
}

/// Debounce gate: remembers the last applied icon and menu so repeated
/// identical states produce no tray calls at all.
#[derive(Debug, Default)]
pub struct TrayDiff {
    last_icon: Option<IconState>,
    last_menu: Option<MenuModel>,
}

impl TrayDiff {
    /// Diff a fresh view-model against what the tray last successfully
    /// applied. Nothing is recorded here: the caller confirms each part via
    /// [`Self::commit_icon`] / [`Self::commit_menu`] only after the tray
    /// call actually succeeded, so a failed render or menu rebuild is
    /// retried on the next state instead of silently desyncing the gate.
    pub fn plan(&self, icon: IconState, menu: &MenuModel) -> TrayPlan {
        TrayPlan {
            icon: (self.last_icon != Some(icon)).then_some(icon),
            menu: (self.last_menu.as_ref() != Some(menu)).then(|| menu.clone()),
        }
    }

    /// Record that `icon` is now what the tray shows.
    pub const fn commit_icon(&mut self, icon: IconState) {
        self.last_icon = Some(icon);
    }

    /// Record that `menu` is now what the tray shows.
    pub fn commit_menu(&mut self, menu: MenuModel) {
        self.last_menu = Some(menu);
    }
}

#[cfg(test)]
mod tests {
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

    #[test]
    fn menu_lists_headline_then_scoped_windows_with_percent_and_reset() {
        let model = menu_model(&healthy(), now(), &all_shown());
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
        let model = menu_model(&healthy(), now(), &HashSet::new());
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
        let model = menu_model(&healthy(), now(), &shown);
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
        let model = menu_model(
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
        let model = menu_model(
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
        let model = menu_model(
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
        let model = menu_model(
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
            let model = menu_model(&state(phase, Staleness::Missing, None), now(), &all_shown());
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
            let model = menu_model(
                &state(phase, Staleness::Stale, Some(old.clone())),
                now(),
                &all_shown(),
            );
            assert_eq!(model.status_line, expected);
        }

        let degraded = state(Phase::Degraded, Staleness::Fresh, Some(snapshot()));
        assert_eq!(
            menu_model(&degraded, now(), &all_shown()).status_line,
            "Connection trouble — data from under 1m ago"
        );
        let degraded_empty = state(Phase::Degraded, Staleness::Missing, None);
        assert_eq!(
            menu_model(&degraded_empty, now(), &all_shown()).status_line,
            "Connection trouble — retrying"
        );
    }

    #[test]
    fn stale_data_is_called_out_with_its_age() {
        let mut snap = snapshot();
        snap.fetched_at = now() - SignedDuration::from_secs(25 * 60);
        let stale = state(Phase::Polling, Staleness::Stale, Some(snap));
        assert_eq!(
            menu_model(&stale, now(), &all_shown()).status_line,
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
        let icon = icon_state(&healthy(), now(), IconStyle::Battery, true, Scale::X2);
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
        let icon = icon_state(&empty, now(), IconStyle::Battery, false, Scale::X1);
        assert_eq!(icon.percent, 0);
        assert_eq!(icon.status, UsageStatus::Safe);
        assert!(!icon.at_risk);
    }

    #[test]
    fn icon_state_carries_the_requested_style_through_snapshot_and_empty_paths() {
        // A style switch (Settings, issue #9) must show up in the very next
        // `icon_state`, with or without a live snapshot — that is what lets
        // the tray apply it live without a restart.
        let icon = icon_state(&healthy(), now(), IconStyle::Gauge, false, Scale::X2);
        assert_eq!(icon.style, IconStyle::Gauge);

        let empty = state(Phase::AwaitingSession, Staleness::Missing, None);
        let icon = icon_state(&empty, now(), IconStyle::Segments, false, Scale::X1);
        assert_eq!(icon.style, IconStyle::Segments);
    }

    #[test]
    fn identical_states_debounce_to_a_noop_once_committed() {
        let mut diff = TrayDiff::default();
        let icon = icon_state(&healthy(), now(), IconStyle::Battery, false, Scale::X2);
        let menu = menu_model(&healthy(), now(), &all_shown());

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
        let icon = icon_state(&healthy(), now(), IconStyle::Battery, false, Scale::X2);
        let menu = menu_model(&healthy(), now(), &all_shown());

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
        let icon = icon_state(&healthy(), now(), IconStyle::Battery, false, Scale::X2);
        let menu = menu_model(&healthy(), now(), &all_shown());
        diff.commit_icon(icon);
        diff.commit_menu(menu);

        // A minute later the icon key is identical but the age text moved.
        let later = now() + SignedDuration::from_secs(60);
        let plan = diff.plan(icon, &menu_model(&healthy(), later, &all_shown()));
        assert_eq!(plan.icon, None);
        assert_eq!(plan.menu.unwrap().status_line, "Updated 1m ago".to_owned());
    }

    #[test]
    fn icon_change_is_planned_even_when_the_menu_is_identical() {
        let mut diff = TrayDiff::default();
        let menu = menu_model(&healthy(), now(), &all_shown());
        let icon = icon_state(&healthy(), now(), IconStyle::Battery, false, Scale::X2);
        diff.commit_icon(icon);
        diff.commit_menu(menu.clone());

        let mut hotter = icon;
        hotter.percent = 43;
        let plan = diff.plan(hotter, &menu);
        assert_eq!(plan.icon, Some(hotter));
        assert_eq!(plan.menu, None);
    }
}
