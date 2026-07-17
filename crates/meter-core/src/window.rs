use jiff::{SignedDuration, Timestamp};
use serde::{Deserialize, Serialize};

use crate::status::UsageStatus;

/// The rolling window a limit applies to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LimitWindow {
    FiveHour,
    SevenDay,
}

impl LimitWindow {
    /// Full length of the rolling window.
    pub const fn duration(self) -> SignedDuration {
        match self {
            Self::FiveHour => SignedDuration::from_hours(5),
            Self::SevenDay => SignedDuration::from_hours(7 * 24),
        }
    }
}

/// One usage window as reported by the API: how much of the limit is used
/// and when it resets.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UsageWindow {
    /// Utilization percentage on a 0–100 scale. May exceed 100.
    pub utilization: f64,
    pub resets_at: Timestamp,
    pub window: LimitWindow,
}

impl UsageWindow {
    pub fn status(&self) -> UsageStatus {
        UsageStatus::from_utilization(self.utilization)
    }

    /// Fraction of the window that has elapsed at `now`, clamped to `0.0..=1.0`.
    ///
    /// The window start is derived from `resets_at` minus the window length,
    /// which is how the original app computes pacing.
    pub fn elapsed_fraction(&self, now: Timestamp) -> f64 {
        let duration = self.window.duration().as_secs_f64();
        let remaining = self.resets_at.duration_since(now).as_secs_f64();
        (1.0 - remaining / duration).clamp(0.0, 1.0)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    // Exact 0.0/1.0 are produced by `clamp`, so equality is intentional here.
    #![allow(clippy::float_cmp)]

    use super::*;
    use pretty_assertions::assert_eq;

    fn window(utilization: f64, resets_in_hours: i64, window: LimitWindow) -> UsageWindow {
        let now: Timestamp = "2026-07-17T12:00:00Z".parse().unwrap();
        UsageWindow {
            utilization,
            resets_at: now + SignedDuration::from_hours(resets_in_hours),
            window,
        }
    }

    fn now() -> Timestamp {
        "2026-07-17T12:00:00Z".parse().unwrap()
    }

    #[test]
    fn elapsed_fraction_midway() {
        let w = window(40.0, 1, LimitWindow::FiveHour);
        let elapsed = w.elapsed_fraction(now());
        assert!((elapsed - 0.8).abs() < 1e-9, "got {elapsed}");
    }

    #[test]
    fn elapsed_fraction_clamps_before_window_start() {
        // resets_at further away than the window length (clock skew / fresh window)
        let w = window(0.0, 10, LimitWindow::FiveHour);
        assert_eq!(w.elapsed_fraction(now()), 0.0);
    }

    #[test]
    fn elapsed_fraction_clamps_after_reset() {
        let w = window(90.0, -1, LimitWindow::FiveHour);
        assert_eq!(w.elapsed_fraction(now()), 1.0);
    }

    #[test]
    fn status_delegates_to_thresholds() {
        assert_eq!(
            window(85.0, 2, LimitWindow::SevenDay).status(),
            UsageStatus::Critical
        );
    }
}
