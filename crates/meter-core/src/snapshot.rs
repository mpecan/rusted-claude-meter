use std::collections::HashSet;

use jiff::Timestamp;
use serde::{Deserialize, Serialize};

use crate::pacing::PacingAssessment;
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
}
