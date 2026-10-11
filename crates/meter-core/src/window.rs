use std::cmp::Ordering;

use jiff::{SignedDuration, Timestamp};
use serde::{Deserialize, Serialize};

use crate::status::UsageStatus;

/// How far two readings' `resets_at` may differ and still name the same
/// cycle of a window.
///
/// The API stamps a real `resets_at` with sub-second jitter that can even
/// straddle the whole second (observed live: the same 17:00 boundary arriving
/// as `16:59:59.601` on one poll and `17:00:00.306` on the next), and Claude
/// Code rounds it to whole seconds before passing it on. Exact equality would
/// read every reading as a fresh cycle. A genuine reset moves the boundary by
/// at least a full window (≥5 h) — the next cycle starts no earlier than the
/// last one ends — so a minute of slack cleanly separates a real rollover
/// from that jitter.
const RESET_JITTER: SignedDuration = SignedDuration::from_secs(60);

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

    /// The span this window's quota is expected to be consumed over — the
    /// denominator every pacing calculation divides by.
    ///
    /// For a session window that is just the window length; for a weekly one it
    /// is the user's `weekly_pace_days` setting, because a week's quota spread
    /// over five working days paces differently from one spread over seven.
    /// Naming the rule once keeps per-window callers (the tray menu's pace
    /// lines) from re-deriving what [`crate::UsageSnapshot::pace_signal`]
    /// already does internally.
    #[must_use]
    pub fn pacing_duration(self, weekly_days: u8) -> SignedDuration {
        match self {
            Self::FiveHour => self.duration(),
            Self::SevenDay => crate::pacing::weekly_pacing_duration(weekly_days),
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

    /// Whether a window resetting at `other_reset` is this same cycle, within
    /// [`RESET_JITTER`]. The one definition of a rollover, shared by
    /// notification cycle tracking and [`Self::progress_cmp`].
    pub fn shares_cycle_with(&self, other_reset: Timestamp) -> bool {
        self.resets_at.duration_since(other_reset).abs() <= RESET_JITTER
    }

    /// Which of two readings of the same limit is further along, using
    /// nothing but the readings themselves: a later cycle is ahead whatever
    /// its utilization, and within one cycle utilization never falls, so the
    /// higher reading is the later one. `Equal` when neither is ahead.
    ///
    /// This is what lets readings from sources with no shared clock — several
    /// Claude Code sessions, each holding the numbers from its own last API
    /// response — be reconciled without trusting when each was taken.
    pub fn progress_cmp(&self, other: &Self) -> Ordering {
        if self.shares_cycle_with(other.resets_at) {
            self.utilization.total_cmp(&other.utilization)
        } else {
            self.resets_at.cmp(&other.resets_at)
        }
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
    fn pacing_duration_is_the_window_for_a_session_and_the_setting_for_a_week() {
        // The session window paces over itself — the weekly pace-days setting
        // must not leak into it.
        assert_eq!(
            LimitWindow::FiveHour.pacing_duration(5),
            SignedDuration::from_hours(5)
        );
        assert_eq!(
            LimitWindow::FiveHour.pacing_duration(7),
            SignedDuration::from_hours(5)
        );
        // A week's quota spread over five working days paces faster than the
        // same quota spread over seven.
        assert_eq!(
            LimitWindow::SevenDay.pacing_duration(5),
            SignedDuration::from_hours(5 * 24)
        );
        assert_eq!(
            LimitWindow::SevenDay.pacing_duration(7),
            LimitWindow::SevenDay.duration()
        );
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

    #[test]
    fn further_into_the_same_cycle_is_ahead() {
        let earlier = window(40.0, 3, LimitWindow::FiveHour);
        let later = window(41.0, 3, LimitWindow::FiveHour);
        assert_eq!(later.progress_cmp(&earlier), Ordering::Greater);
        assert_eq!(earlier.progress_cmp(&later), Ordering::Less);
        assert_eq!(earlier.progress_cmp(&earlier.clone()), Ordering::Equal);
    }

    /// A reset starts the count again from a low number; the later cycle is
    /// still the newer reading.
    #[test]
    fn a_later_cycle_is_ahead_despite_a_lower_reading() {
        let previous = window(97.0, 1, LimitWindow::FiveHour);
        let current = window(3.0, 6, LimitWindow::FiveHour);
        assert_eq!(current.progress_cmp(&previous), Ordering::Greater);
        assert_eq!(previous.progress_cmp(&current), Ordering::Less);
    }

    /// Reset jitter must not make a lower reading of the same cycle look
    /// like a newer cycle.
    #[test]
    fn reset_jitter_is_the_same_cycle() {
        let recorded = window(40.0, 3, LimitWindow::FiveHour);
        let mut jittered = window(30.0, 3, LimitWindow::FiveHour);
        jittered.resets_at += SignedDuration::from_secs(59);
        assert!(jittered.shares_cycle_with(recorded.resets_at));
        assert_eq!(jittered.progress_cmp(&recorded), Ordering::Less);
        jittered.resets_at += SignedDuration::from_secs(2);
        assert!(!jittered.shares_cycle_with(recorded.resets_at));
    }
}
