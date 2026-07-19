use std::collections::HashSet;

use jiff::{SignedDuration, Timestamp};
use serde::{Deserialize, Serialize};

use crate::pace_signal::{PaceKind, PaceSignal};
use crate::pacing::{PacingAssessment, RISK_THRESHOLD, UNDERUSE_THRESHOLD, weekly_pacing_duration};
use crate::status::UsageStatus;
use crate::window::UsageWindow;

/// A limit scoped to a specific model, taken from the API's `limits` array.
///
/// The API supplies the display name (`scope.model.display_name`, e.g.
/// "Sonnet" or "Fable"), so a model released after this build needs no code
/// change. `model_id` is carried when present but is currently `null` in the
/// API, so identity is keyed on `display_name`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScopedLimit {
    pub display_name: String,
    pub model_id: Option<String>,
    pub usage: UsageWindow,
    pub is_active: bool,
}

impl ScopedLimit {
    /// Whether this scoped limit belongs in the tray/popover/notifier: the
    /// API reports it active *and* the user opted into showing it (issue
    /// #6's `shown_scoped_models`, empty/opt-in by default). Single source
    /// of truth for that gate — `tray::model::menu_model` and
    /// `notifier::tracked_windows` both call this so their notion of a
    /// visible scoped model cannot drift apart.
    pub fn is_visible(&self, shown: &HashSet<String>) -> bool {
        self.is_active && shown.contains(&self.display_name)
    }
}

/// Everything `ClaudeMeter` knows about current usage after one fetch.
///
/// `five_hour` and `seven_day` are the headline windows; `scoped` holds one
/// entry per model-scoped limit. Headline kinds are excluded from the scoped
/// list at decode time so an entry the API later scopes cannot render twice.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UsageSnapshot {
    pub five_hour: Option<UsageWindow>,
    pub seven_day: Option<UsageWindow>,
    pub scoped: Vec<ScopedLimit>,
    pub fetched_at: Timestamp,
}

impl UsageSnapshot {
    /// Worst status across every window, headline and scoped alike.
    /// Drives the tray icon colour.
    pub fn overall_status(&self) -> UsageStatus {
        self.windows()
            .map(UsageWindow::status)
            .max()
            .unwrap_or(UsageStatus::Safe)
    }

    /// True when any window — headline or scoped — is pacing at risk at
    /// `now`. Drives the tray icon's at-risk badge.
    pub fn at_risk(&self, now: Timestamp) -> bool {
        self.windows()
            .any(|window| PacingAssessment::for_window(window, now).is_some_and(|p| p.at_risk))
    }

    /// Off-pace signal for the tray/popover badge, computed from the headline
    /// windows only (session + weekly) — scoped per-model limits do not
    /// participate, matching upstream's `UsageData.paceSignal`. (The broader
    /// [`Self::at_risk`] gate does include scoped windows; the two are
    /// deliberately separate.)
    ///
    /// Hot when either window burns faster than sustainable (highest ratio
    /// wins, since an imminent lockout matters more than long-term underuse);
    /// cold only when the weekly window is underused — idle time within the
    /// short session window is not a meaningful underuse signal.
    ///
    /// `weekly_pace_days` (5–7) is the number of days per week the weekly quota
    /// is expected to be consumed over; sustainable weekly pace is measured
    /// against this span.
    #[must_use]
    pub fn pace_signal(&self, now: Timestamp, weekly_pace_days: u8) -> Option<PaceSignal> {
        let weekly_pacing = weekly_pacing_duration(weekly_pace_days);
        let session_basis = WindowBasis {
            name: "5-hour",
            pacing: None,
            pace_days: None,
        };
        let weekly_basis = WindowBasis {
            name: "7-day",
            pacing: Some(weekly_pacing),
            pace_days: Some(weekly_pace_days),
        };

        let session = self.five_hour.as_ref();
        let weekly = self.seven_day.as_ref();
        let session_ratio = session.and_then(|w| w.pace_ratio(now, None));
        let weekly_ratio = weekly.and_then(|w| w.pace_ratio(now, Some(weekly_pacing)));

        let session_hot = session
            .zip(session_ratio)
            .filter(|&(_, ratio)| ratio > RISK_THRESHOLD)
            .map(|(w, ratio)| make_signal(PaceKind::Hot, w, ratio, now, &session_basis));
        let weekly_hot = weekly
            .zip(weekly_ratio)
            .filter(|&(_, ratio)| ratio > RISK_THRESHOLD)
            .map(|(w, ratio)| make_signal(PaceKind::Hot, w, ratio, now, &weekly_basis));

        // Highest ratio wins; on an exact tie the session signal wins. Upstream
        // builds `hotSignals` session-first then calls `max(by: { $0.ratio <
        // $1.ratio })`, and Swift's `max(by:)` only replaces its running result
        // when the current element is strictly greater, so it keeps the first
        // maximal element — the session signal here.
        let hottest = match (session_hot, weekly_hot) {
            (Some(s), Some(w)) => Some(if w.ratio > s.ratio { w } else { s }),
            (s, w) => s.or(w),
        };
        if let Some(signal) = hottest {
            return Some(signal);
        }

        // Underuse is a weekly-only signal: idle time within the short session
        // window is not meaningful waste.
        weekly
            .zip(weekly_ratio)
            .filter(|&(_, ratio)| ratio < UNDERUSE_THRESHOLD)
            .map(|(w, ratio)| make_signal(PaceKind::Cold, w, ratio, now, &weekly_basis))
    }

    /// The scoped limit for a given API display name, if reported.
    pub fn scoped_named(&self, display_name: &str) -> Option<&ScopedLimit> {
        self.scoped
            .iter()
            .find(|limit| limit.display_name == display_name)
    }

    /// Every reported window: headline first, then scoped, in API order.
    fn windows(&self) -> impl Iterator<Item = &UsageWindow> {
        self.five_hour
            .iter()
            .chain(self.seven_day.iter())
            .chain(self.scoped.iter().map(|limit| &limit.usage))
    }
}

/// The pacing basis of a headline window: its display name plus the span and
/// day count the quota is paced over (both `None` for the full-window session).
struct WindowBasis {
    name: &'static str,
    pacing: Option<SignedDuration>,
    pace_days: Option<u8>,
}

/// Build a [`PaceSignal`] from a window, resolving the expected-usage figure on
/// the same pacing basis as its ratio. Mirrors `UsageData.makeSignal`.
fn make_signal(
    kind: PaceKind,
    window: &UsageWindow,
    ratio: f64,
    now: Timestamp,
    basis: &WindowBasis,
) -> PaceSignal {
    PaceSignal {
        kind,
        ratio,
        window_name: basis.name.to_owned(),
        used_percent: window.utilization,
        expected_percent: window
            .expected_usage_percent(now, basis.pacing)
            .unwrap_or(0.0),
        pace_days: basis.pace_days,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use crate::window::LimitWindow;
    use pretty_assertions::assert_eq;

    fn window(utilization: f64) -> UsageWindow {
        UsageWindow {
            utilization,
            resets_at: "2026-07-18T12:00:00Z".parse().unwrap(),
            window: LimitWindow::SevenDay,
        }
    }

    fn snapshot(five_hour: f64, seven_day: f64, scoped: f64) -> UsageSnapshot {
        UsageSnapshot {
            five_hour: Some(window(five_hour)),
            seven_day: Some(window(seven_day)),
            scoped: vec![ScopedLimit {
                display_name: "Fable".to_owned(),
                model_id: None,
                usage: window(scoped),
                is_active: true,
            }],
            fetched_at: "2026-07-17T12:00:00Z".parse().unwrap(),
        }
    }

    #[test]
    fn overall_status_takes_the_worst_window() {
        assert_eq!(
            snapshot(10.0, 20.0, 85.0).overall_status(),
            UsageStatus::Critical
        );
        assert_eq!(
            snapshot(10.0, 55.0, 20.0).overall_status(),
            UsageStatus::Warning
        );
    }

    #[test]
    fn overall_status_of_empty_snapshot_is_safe() {
        let empty = UsageSnapshot {
            five_hour: None,
            seven_day: None,
            scoped: vec![],
            fetched_at: "2026-07-17T12:00:00Z".parse().unwrap(),
        };
        assert_eq!(empty.overall_status(), UsageStatus::Safe);
    }

    #[test]
    fn at_risk_when_any_window_is_pacing_hot() {
        let now: Timestamp = "2026-07-17T12:00:00Z".parse().unwrap();
        // The helper's seven-day windows reset 24h out (~86% elapsed), so
        // even 100% used stays under the 1.2 risk ratio: calm all round.
        assert!(!snapshot(10.0, 20.0, 100.0).at_risk(now));

        // A single scoped window burning hot (90% used, 4 of 7 days left →
        // ratio 2.1) flips the snapshot even with calm headline windows.
        let mut hot = snapshot(10.0, 20.0, 0.0);
        hot.scoped[0].usage = UsageWindow {
            utilization: 90.0,
            resets_at: "2026-07-21T12:00:00Z".parse().unwrap(),
            window: LimitWindow::SevenDay,
        };
        assert!(hot.at_risk(now));
    }

    #[test]
    fn scoped_lookup_is_keyed_on_display_name() {
        let snapshot = snapshot(0.0, 0.0, 42.0);
        assert!(snapshot.scoped_named("Fable").is_some());
        assert!(snapshot.scoped_named("Sonnet").is_none());
    }

    // --- pace_signal (hybrid rule) --------------------------------------

    fn pace_now() -> Timestamp {
        "2026-07-17T12:00:00Z".parse().unwrap()
    }

    /// A window with `utilization` used and `elapsed_fraction` of `window`
    /// elapsed at [`pace_now`], mirroring upstream's `limit(...)` test helper.
    fn paced(utilization: f64, elapsed_fraction: f64, window: LimitWindow) -> UsageWindow {
        let full = window.duration().as_secs_f64();
        let remaining = full * (1.0 - elapsed_fraction);
        UsageWindow {
            utilization,
            resets_at: pace_now() + SignedDuration::try_from_secs_f64(remaining).unwrap(),
            window,
        }
    }

    fn paced_data(session: UsageWindow, weekly: UsageWindow) -> UsageSnapshot {
        UsageSnapshot {
            five_hour: Some(session),
            seven_day: Some(weekly),
            scoped: vec![],
            fetched_at: pace_now(),
        }
    }

    #[test]
    fn pace_signal_session_burning_fast_is_hot_from_session_window() {
        let data = paced_data(
            paced(50.0, 0.25, LimitWindow::FiveHour),
            paced(50.0, 0.5, LimitWindow::SevenDay),
        );
        let signal = data.pace_signal(pace_now(), 7).unwrap();
        assert_eq!(signal.kind, PaceKind::Hot);
        assert_eq!(signal.window_name, "5-hour");
    }

    #[test]
    fn pace_signal_both_hot_picks_higher_ratio() {
        let data = paced_data(
            paced(60.0, 0.4, LimitWindow::FiveHour), // 1.5
            paced(90.0, 0.3, LimitWindow::SevenDay), // 3.0
        );
        let signal = data.pace_signal(pace_now(), 7).unwrap();
        assert_eq!(signal.kind, PaceKind::Hot);
        assert_eq!(signal.window_name, "7-day");
    }

    #[test]
    fn pace_signal_both_hot_equal_ratio_prefers_session() {
        // Session and weekly both hot at exactly the same ratio (2.0). Upstream
        // builds hotSignals session-first and max(by:) keeps the first maximal
        // element, so the 5-hour signal must win the tie.
        let data = paced_data(
            paced(50.0, 0.25, LimitWindow::FiveHour), // 2.0
            paced(50.0, 0.25, LimitWindow::SevenDay), // 2.0 (full-week basis)
        );
        let signal = data.pace_signal(pace_now(), 7).unwrap();
        assert_eq!(signal.kind, PaceKind::Hot);
        assert_eq!(signal.window_name, "5-hour");
    }

    #[test]
    fn pace_signal_weekly_underused_is_cold() {
        let data = paced_data(
            paced(50.0, 0.5, LimitWindow::FiveHour),
            paced(20.0, 0.5, LimitWindow::SevenDay), // 0.4
        );
        let signal = data.pace_signal(pace_now(), 7).unwrap();
        assert_eq!(signal.kind, PaceKind::Cold);
        assert_eq!(signal.window_name, "7-day");
    }

    #[test]
    fn pace_signal_session_idle_but_weekly_on_pace_is_none() {
        // Session underuse alone must not produce a cold signal.
        let data = paced_data(
            paced(10.0, 0.8, LimitWindow::FiveHour), // 0.125
            paced(50.0, 0.5, LimitWindow::SevenDay), // 1.0
        );
        assert!(data.pace_signal(pace_now(), 7).is_none());
    }

    #[test]
    fn pace_signal_session_hot_wins_over_weekly_cold() {
        // Imminent session lockout beats long-term weekly underuse.
        let data = paced_data(
            paced(50.0, 0.25, LimitWindow::FiveHour), // 2.0
            paced(20.0, 0.5, LimitWindow::SevenDay),  // 0.4
        );
        assert_eq!(data.pace_signal(pace_now(), 7).unwrap().kind, PaceKind::Hot);
    }

    #[test]
    fn pace_signal_on_pace_everywhere_is_none() {
        let data = paced_data(
            paced(50.0, 0.5, LimitWindow::FiveHour),
            paced(50.0, 0.5, LimitWindow::SevenDay),
        );
        assert!(data.pace_signal(pace_now(), 7).is_none());
    }

    #[test]
    fn pace_signal_zero_utilization_has_zero_ratio_and_finite_expected() {
        let data = paced_data(
            paced(50.0, 0.5, LimitWindow::FiveHour),
            paced(0.0, 0.5, LimitWindow::SevenDay),
        );
        let signal = data.pace_signal(pace_now(), 7).unwrap();
        assert_eq!(signal.kind, PaceKind::Cold);
        assert!(signal.ratio.abs() < 0.001);
        assert!((signal.expected_percent - 50.0).abs() < 1.0);
    }

    #[test]
    fn pace_signal_hot_on_seven_day_basis_not_hot_on_five_day_basis() {
        // Weekly 40% used at 2 of 7 days: 7-day ratio 1.4 (hot), 5-day 1.0 (on pace).
        let data = paced_data(
            paced(50.0, 0.5, LimitWindow::FiveHour),
            paced(40.0, 2.0 / 7.0, LimitWindow::SevenDay),
        );
        assert_eq!(data.pace_signal(pace_now(), 7).unwrap().kind, PaceKind::Hot);
        assert!(data.pace_signal(pace_now(), 5).is_none());
    }
}
