//! Pure tray view-model: no Tauri types, no I/O, fully unit-testable.
//!
//! Everything the tray shows is computed here from a [`MeterState`] plus a
//! `now` timestamp: the icon state to render, the menu's status line and the
//! live usage lines (one per window — 5-hour, 7-day, each scoped model).
//! [`TrayDiff`] is the debounce gate: it remembers what the tray last
//! applied and turns a fresh view-model into the minimal [`TrayPlan`], so
//! identical consecutive states touch neither the icon nor the menu (no
//! flicker, no redundant `set_icon` calls).

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
/// an empty safe gauge otherwise.
pub fn icon_state(state: &MeterState, now: Timestamp, mono: bool, scale: Scale) -> IconState {
    state.snapshot.as_ref().map_or(
        IconState {
            style: IconStyle::Battery,
            percent: 0,
            status: meter_core::UsageStatus::Safe,
            at_risk: false,
            mono,
            scale,
        },
        |snapshot| IconState::from_snapshot(snapshot, now, IconStyle::Battery, mono, scale),
    )
}

/// Build the menu view-model for a state at `now`.
pub fn menu_model(state: &MeterState, now: Timestamp) -> MenuModel {
    let mut usage_lines = Vec::new();
    if let Some(snapshot) = &state.snapshot {
        if let Some(window) = &snapshot.five_hour {
            usage_lines.push(usage_line(window_label(window.window), window, now));
        }
        if let Some(window) = &snapshot.seven_day {
            usage_lines.push(usage_line(window_label(window.window), window, now));
        }
        for limit in &snapshot.scoped {
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

/// "5-hour: 42% — resets in 2h 15m"
fn usage_line(label: &str, window: &UsageWindow, now: Timestamp) -> String {
    let percent = round_percent(window.utilization);
    let remaining = window.resets_at.duration_since(now).as_secs();
    if remaining <= 0 {
        format!("{label}: {percent}% — resets soon")
    } else {
        format!(
            "{label}: {percent}% — resets in {}",
            short_duration(remaining)
        )
    }
}

fn status_line(state: &MeterState, now: Timestamp) -> String {
    let age = state
        .snapshot
        .as_ref()
        .map(|snapshot| short_duration(now.duration_since(snapshot.fetched_at).as_secs()));
    match (state.phase, age) {
        (Phase::AwaitingSession, _) => "No session key — choose Open to set one".to_owned(),
        (Phase::SessionExpired, _) => "Session expired — choose Open to update it".to_owned(),
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
    /// Diff a fresh view-model against what the tray currently shows and
    /// record it as applied.
    pub fn plan(&mut self, icon: IconState, menu: MenuModel) -> TrayPlan {
        let icon_changed = self.last_icon != Some(icon);
        let menu_changed = self.last_menu.as_ref() != Some(&menu);
        self.last_icon = Some(icon);
        let plan = TrayPlan {
            icon: icon_changed.then_some(icon),
            menu: menu_changed.then(|| menu.clone()),
        };
        self.last_menu = Some(menu);
        plan
    }
}

/// Tray icon bounds in physical pixels (origin top-left of the screen).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrayRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// Horizontal extent of the screen the tray click landed on, physical px.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScreenBounds {
    pub x: f64,
    pub width: f64,
}

/// Gap between the menu bar and the popover's top edge, physical px.
const POPOVER_GAP: f64 = 8.0;
/// Minimum distance the popover keeps from the screen edges, physical px.
const POPOVER_MARGIN: f64 = 8.0;

/// Top-left corner for the macOS popover window: centred under the tray
/// icon, just below the menu bar, clamped inside the screen when its bounds
/// are known.
pub fn popover_origin(
    tray: TrayRect,
    window_width: f64,
    screen: Option<ScreenBounds>,
) -> (f64, f64) {
    let mut x = tray.width.mul_add(0.5, tray.x) - window_width / 2.0;
    if let Some(screen) = screen {
        let min_x = screen.x + POPOVER_MARGIN;
        let max_x = min_x.max(screen.x + screen.width - window_width - POPOVER_MARGIN);
        x = x.clamp(min_x, max_x);
    }
    (x, tray.y + tray.height + POPOVER_GAP)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    // Popover coordinates are exact float arithmetic on whole numbers.
    #![allow(clippy::float_cmp)]

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

    #[test]
    fn menu_lists_headline_then_scoped_windows_with_percent_and_reset() {
        let model = menu_model(&healthy(), now());
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
    fn menu_has_no_usage_lines_without_a_snapshot() {
        let model = menu_model(&state(Phase::Polling, Staleness::Missing, None), now());
        assert!(model.usage_lines.is_empty());
        assert_eq!(model.status_line, "Waiting for first update…");
    }

    #[test]
    fn reset_in_the_past_reads_as_resets_soon() {
        let mut snap = snapshot();
        snap.five_hour = Some(window(10.0, -5, LimitWindow::FiveHour));
        snap.seven_day = None;
        snap.scoped.clear();
        let model = menu_model(&state(Phase::Polling, Staleness::Fresh, Some(snap)), now());
        assert_eq!(model.usage_lines, vec!["5-hour: 10% — resets soon"]);
    }

    #[test]
    fn status_line_reflects_every_phase() {
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
            let model = menu_model(&state(phase, Staleness::Fresh, Some(snapshot())), now());
            assert_eq!(model.status_line, expected);
        }

        let degraded = state(Phase::Degraded, Staleness::Fresh, Some(snapshot()));
        assert_eq!(
            menu_model(&degraded, now()).status_line,
            "Connection trouble — data from under 1m ago"
        );
        let degraded_empty = state(Phase::Degraded, Staleness::Missing, None);
        assert_eq!(
            menu_model(&degraded_empty, now()).status_line,
            "Connection trouble — retrying"
        );
    }

    #[test]
    fn stale_data_is_called_out_with_its_age() {
        let mut snap = snapshot();
        snap.fetched_at = now() - SignedDuration::from_secs(25 * 60);
        let stale = state(Phase::Polling, Staleness::Stale, Some(snap));
        assert_eq!(
            menu_model(&stale, now()).status_line,
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
        let icon = icon_state(&healthy(), now(), true, Scale::X2);
        assert_eq!(icon.percent, 42);
        assert!(icon.mono);
        // Fable at ~100% drives the worst-window status.
        assert_eq!(icon.status, UsageStatus::Critical);
    }

    #[test]
    fn icon_state_is_an_empty_safe_gauge_without_a_snapshot() {
        let empty = state(Phase::AwaitingSession, Staleness::Missing, None);
        let icon = icon_state(&empty, now(), false, Scale::X1);
        assert_eq!(icon.percent, 0);
        assert_eq!(icon.status, UsageStatus::Safe);
        assert!(!icon.at_risk);
    }

    #[test]
    fn identical_states_debounce_to_a_noop() {
        let mut diff = TrayDiff::default();
        let icon = icon_state(&healthy(), now(), false, Scale::X2);
        let menu = menu_model(&healthy(), now());

        let first = diff.plan(icon, menu.clone());
        assert_eq!(first.icon, Some(icon));
        assert_eq!(first.menu, Some(menu.clone()));

        let second = diff.plan(icon, menu);
        assert_eq!(
            second,
            TrayPlan {
                icon: None,
                menu: None
            }
        );
    }

    #[test]
    fn menu_only_change_leaves_the_icon_untouched() {
        let mut diff = TrayDiff::default();
        let icon = icon_state(&healthy(), now(), false, Scale::X2);
        diff.plan(icon, menu_model(&healthy(), now()));

        // A minute later the icon key is identical but the age text moved.
        let later = now() + SignedDuration::from_secs(60);
        let plan = diff.plan(icon, menu_model(&healthy(), later));
        assert_eq!(plan.icon, None);
        assert_eq!(plan.menu.unwrap().status_line, "Updated 1m ago".to_owned());
    }

    #[test]
    fn icon_change_is_planned_even_when_the_menu_is_identical() {
        let mut diff = TrayDiff::default();
        let menu = menu_model(&healthy(), now());
        let icon = icon_state(&healthy(), now(), false, Scale::X2);
        diff.plan(icon, menu.clone());

        let mut hotter = icon;
        hotter.percent = 43;
        let plan = diff.plan(hotter, menu);
        assert_eq!(plan.icon, Some(hotter));
        assert_eq!(plan.menu, None);
    }

    const TRAY: TrayRect = TrayRect {
        x: 1000.0,
        y: 0.0,
        width: 44.0,
        height: 24.0,
    };

    #[test]
    fn popover_centres_under_the_tray_icon() {
        let (x, y) = popover_origin(TRAY, 420.0, None);
        assert_eq!(x, 1000.0 + 22.0 - 210.0);
        assert_eq!(y, 24.0 + 8.0);
    }

    #[test]
    fn popover_clamps_to_the_right_screen_edge() {
        let tray = TrayRect { x: 1200.0, ..TRAY };
        let screen = ScreenBounds {
            x: 0.0,
            width: 1280.0,
        };
        let (x, _) = popover_origin(tray, 420.0, Some(screen));
        assert_eq!(x, 1280.0 - 420.0 - 8.0);
    }

    #[test]
    fn popover_clamps_to_the_left_screen_edge() {
        let tray = TrayRect { x: 4.0, ..TRAY };
        let screen = ScreenBounds {
            x: 0.0,
            width: 1280.0,
        };
        let (x, _) = popover_origin(tray, 420.0, Some(screen));
        assert_eq!(x, 8.0);
    }

    #[test]
    fn popover_wider_than_the_screen_pins_to_the_left_margin() {
        let screen = ScreenBounds {
            x: 0.0,
            width: 300.0,
        };
        let (x, _) = popover_origin(TRAY, 420.0, Some(screen));
        assert_eq!(x, 8.0);
    }
}
