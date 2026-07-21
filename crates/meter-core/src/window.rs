use jiff::{SignedDuration, Timestamp};
use serde::{Deserialize, Serialize};

use crate::status::UsageStatus;

/// The rolling window a limit applies to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
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

    /// The reset instant to assume when the API omits `resets_at` for a
    /// window of this length (`resets_at: null` — an idle window with nothing
    /// scheduled to reset): the fetch time plus the window length. The single
    /// source of truth for that fallback, shared by the API mapping (which
    /// fills it in so an idle window still renders) and the notifier's cycle
    /// tracking (which recognizes it via [`UsageWindow::reset_is_estimated`],
    /// so a value that advances with every poll is never mistaken for a real
    /// reset).
    pub fn fallback_reset(self, fetched_at: Timestamp) -> Timestamp {
        fetched_at
            .checked_add(self.duration())
            .unwrap_or(fetched_at)
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

    /// Whether this window's `resets_at` was synthesized by
    /// [`LimitWindow::fallback_reset`] rather than reported by the API — i.e.
    /// the API sent `resets_at: null` and the mapping filled in
    /// `fetched_at + window`. That synthesized instant advances with every
    /// poll, so cycle-tracking callers (notifications) must not treat its
    /// movement as a real reset. `fetched_at` must be the snapshot's fetch
    /// time — the exact value the mapping used — so this is a precise
    /// recomputation, not a heuristic. (A genuine API reset would have to land
    /// nanosecond-exactly on `fetched_at + window` to be misclassified, which
    /// a clock-aligned boundary never does.)
    pub fn reset_is_estimated(&self, fetched_at: Timestamp) -> bool {
        self.resets_at == self.window.fallback_reset(fetched_at)
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
    fn reset_is_estimated_recognizes_the_fallback_and_rejects_a_real_reset() {
        let fetched: Timestamp = "2026-07-20T12:34:56.789Z".parse().unwrap();
        // A window whose reset is exactly the fallback the mapping would fill.
        let synthesized = UsageWindow {
            utilization: 0.0,
            resets_at: LimitWindow::FiveHour.fallback_reset(fetched),
            window: LimitWindow::FiveHour,
        };
        assert!(synthesized.reset_is_estimated(fetched));
        // A real, clock-aligned boundary reported by the API is not the fallback.
        let real = UsageWindow {
            utilization: 0.0,
            resets_at: "2026-07-20T17:00:00Z".parse().unwrap(),
            window: LimitWindow::FiveHour,
        };
        assert!(!real.reset_is_estimated(fetched));
    }

    #[test]
    fn fallback_reset_is_fetch_time_plus_window_length() {
        let fetched: Timestamp = "2026-07-20T12:00:00Z".parse().unwrap();
        assert_eq!(
            LimitWindow::FiveHour.fallback_reset(fetched),
            "2026-07-20T17:00:00Z".parse::<Timestamp>().unwrap()
        );
        assert_eq!(
            LimitWindow::SevenDay.fallback_reset(fetched),
            "2026-07-27T12:00:00Z".parse::<Timestamp>().unwrap()
        );
    }

    #[test]
    fn status_delegates_to_thresholds() {
        assert_eq!(
            window(85.0, 2, LimitWindow::SevenDay).status(),
            UsageStatus::Critical
        );
    }
}
