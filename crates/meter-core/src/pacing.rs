use jiff::Timestamp;
use serde::{Deserialize, Serialize};

use crate::window::UsageWindow;

/// Burning usage more than 20% faster than a sustainable, even pace counts
/// as at-risk (mirrors `ClaudeMeter`'s `Constants.Pacing.riskThreshold`).
pub const RISK_THRESHOLD: f64 = 1.2;

/// Ignore pacing until this fraction of the window has elapsed; ratios
/// against a nearly-empty denominator are noise, not signal.
const MIN_ELAPSED_FRACTION: f64 = 0.05;

/// Result of comparing consumption rate against an even burn-down.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PacingAssessment {
    /// `used_fraction / elapsed_fraction`; 1.0 is exactly sustainable pace.
    pub ratio: f64,
    /// True when the ratio exceeds [`RISK_THRESHOLD`].
    pub at_risk: bool,
}

impl PacingAssessment {
    /// Assess a window's pacing at `now`.
    ///
    /// Returns `None` while less than [`MIN_ELAPSED_FRACTION`] of the window
    /// has elapsed, when the window has already reset, or when nothing has
    /// been used yet.
    pub fn for_window(window: &UsageWindow, now: Timestamp) -> Option<Self> {
        let elapsed = window.elapsed_fraction(now);
        if !(MIN_ELAPSED_FRACTION..1.0).contains(&elapsed) || window.utilization <= 0.0 {
            return None;
        }
        let ratio = (window.utilization / 100.0) / elapsed;
        Some(Self {
            ratio,
            at_risk: ratio > RISK_THRESHOLD,
        })
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use crate::window::LimitWindow;
    use jiff::SignedDuration;

    fn now() -> Timestamp {
        "2026-07-17T12:00:00Z".parse().unwrap()
    }

    fn five_hour_window(utilization: f64, resets_in_minutes: i64) -> UsageWindow {
        UsageWindow {
            utilization,
            resets_at: now() + SignedDuration::from_mins(resets_in_minutes),
            window: LimitWindow::FiveHour,
        }
    }

    #[test]
    fn sustainable_pace_is_not_at_risk() {
        // Half the window gone, half the budget used.
        let assessment = PacingAssessment::for_window(&five_hour_window(50.0, 150), now()).unwrap();
        assert!((assessment.ratio - 1.0).abs() < 1e-9);
        assert!(!assessment.at_risk);
    }

    #[test]
    fn burning_fast_is_at_risk() {
        // Half the window gone, 80% used → ratio 1.6.
        let assessment = PacingAssessment::for_window(&five_hour_window(80.0, 150), now()).unwrap();
        assert!(assessment.at_risk);
    }

    #[test]
    fn fresh_window_yields_none() {
        // Only ~1% elapsed: below MIN_ELAPSED_FRACTION.
        let assessment = PacingAssessment::for_window(&five_hour_window(5.0, 297), now());
        assert!(assessment.is_none());
    }

    #[test]
    fn expired_window_yields_none() {
        let assessment = PacingAssessment::for_window(&five_hour_window(90.0, -10), now());
        assert!(assessment.is_none());
    }

    #[test]
    fn unused_window_yields_none() {
        let assessment = PacingAssessment::for_window(&five_hour_window(0.0, 150), now());
        assert!(assessment.is_none());
    }
}
