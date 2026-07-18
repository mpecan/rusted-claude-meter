use jiff::{SignedDuration, Timestamp};
use serde::{Deserialize, Serialize};

use crate::window::UsageWindow;

/// Burning usage more than 20% faster than a sustainable, even pace counts
/// as at-risk (mirrors `ClaudeMeter`'s `Constants.Pacing.riskThreshold`).
pub const RISK_THRESHOLD: f64 = 1.2;

/// Below this ratio the weekly quota is likely to go unused before reset
/// (mirrors `Constants.Pacing.underuseThreshold`).
pub const UNDERUSE_THRESHOLD: f64 = 0.8;

/// Above this ratio overuse is shown as heavy — red rather than orange
/// (mirrors `Constants.Pacing.heavyOveruseThreshold`).
pub const HEAVY_OVERUSE_THRESHOLD: f64 = 2.5;

/// Minimum utilization before a limit-hit is projected.
///
/// Below this an early front-loaded burst is treated as noise; at or above it
/// a lockout warning surfaces immediately, without waiting out
/// [`MIN_ELAPSED_FRACTION`] (mirrors `Constants.Pacing.minimumUsageForProjection`).
pub const MIN_USAGE_FOR_PROJECTION: f64 = 5.0;

/// Ignore pacing until this fraction of the window has elapsed; ratios
/// against a nearly-empty denominator are noise, not signal.
const MIN_ELAPSED_FRACTION: f64 = 0.05;

/// The span a weekly quota is expected to be consumed over.
///
/// Given a pace-days setting (5–7), returns it as a [`SignedDuration`].
/// Mirrors `Constants.Pacing.weeklyPacingDuration(days:)`.
#[must_use]
pub fn weekly_pacing_duration(days: u8) -> SignedDuration {
    SignedDuration::from_hours(i64::from(days) * 24)
}

/// The discrete pace band a ratio falls into.
///
/// Single source of truth for the ratio thresholds so colours (in
/// `meter-render`), cache keys and callers cannot drift apart. Mirrors
/// `ClaudeMeter`'s `PacePalette.Band`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaceBand {
    /// `< 0.8×` — quota likely left unused (blue).
    Underuse,
    /// `0.8..=1.2×` — sustainable (green).
    Sustainable,
    /// `1.2..=2.5×` — overuse (orange).
    Overuse,
    /// `> 2.5×` — heavy overuse (red).
    HeavyOveruse,
}

impl PaceBand {
    /// Classify a pace ratio. Blue underuse (`<0.8×`), green sustainable
    /// (`0.8–1.2×`), orange overuse (`1.2–2.5×`), red heavy overuse (`>2.5×`).
    #[must_use]
    pub fn from_ratio(ratio: f64) -> Self {
        if ratio < UNDERUSE_THRESHOLD {
            Self::Underuse
        } else if ratio <= RISK_THRESHOLD {
            Self::Sustainable
        } else if ratio <= HEAVY_OVERUSE_THRESHOLD {
            Self::Overuse
        } else {
            Self::HeavyOveruse
        }
    }
}

impl UsageWindow {
    /// Utilization percentage the pace plan expects by now (0–100): the elapsed
    /// fraction of the *pacing span*, ×100, capped at 100%.
    ///
    /// `pacing_duration` is the span the quota is expected to be consumed over;
    /// `None` uses the full window. A shorter span (e.g. 5 working days of a
    /// 7-day window) expects a faster burn. Elapsed time is capped at the
    /// pacing duration, so past it the expected usage is 100% and the pace
    /// ratio equals the raw usage fraction.
    ///
    /// Returns `None` when the window has already reset, the pacing span is
    /// non-positive, or less than [`MIN_ELAPSED_FRACTION`] of the pacing span
    /// has elapsed (suppresses post-reset ratio noise).
    #[must_use]
    pub fn expected_usage_percent(
        &self,
        now: Timestamp,
        pacing_duration: Option<SignedDuration>,
    ) -> Option<f64> {
        let window_secs = self.window.duration().as_secs_f64();
        let pacing = pacing_duration
            .unwrap_or_else(|| self.window.duration())
            .as_secs_f64();
        if self.resets_at <= now || pacing <= 0.0 {
            return None;
        }
        // windowStart = resets_at - window; elapsed = now - windowStart.
        let remaining = self.resets_at.duration_since(now).as_secs_f64();
        let elapsed = window_secs - remaining;
        let elapsed_fraction = (elapsed / pacing).min(1.0);
        if elapsed_fraction < MIN_ELAPSED_FRACTION {
            return None;
        }
        Some(elapsed_fraction * 100.0)
    }

    /// Ratio of usage fraction to elapsed-time fraction of the pacing span:
    /// `min(utilization, 100) / expected_usage_percent`. 1.0 is exactly
    /// sustainable, `>1` burning faster, `<1` underusing. `None` under the same
    /// conditions as [`Self::expected_usage_percent`].
    #[must_use]
    pub fn pace_ratio(
        &self,
        now: Timestamp,
        pacing_duration: Option<SignedDuration>,
    ) -> Option<f64> {
        let expected = self.expected_usage_percent(now, pacing_duration)?;
        Some(self.utilization.min(100.0) / expected)
    }

    /// Projected utilization percentage at the pacing deadline if the current
    /// average rate holds. Extrapolates to the pacing horizon (default: the
    /// full window) so it shares a time basis with [`Self::pace_ratio`] — a card
    /// cannot then read "underusing" and "hits limit" at once. `None` when the
    /// window has reset or less than [`MIN_ELAPSED_FRACTION`] of the *window*
    /// has elapsed.
    #[must_use]
    pub fn projected_end_percent(
        &self,
        now: Timestamp,
        pacing_duration: Option<SignedDuration>,
    ) -> Option<f64> {
        let window_dur = self.window.duration();
        let window_secs = window_dur.as_secs_f64();
        if self.resets_at <= now {
            return None;
        }
        let remaining = self.resets_at.duration_since(now).as_secs_f64();
        let elapsed = window_secs - remaining;
        if elapsed < window_secs * MIN_ELAPSED_FRACTION {
            return None;
        }
        // Never project a horizon shorter than what has already elapsed.
        let horizon = pacing_duration
            .unwrap_or(window_dur)
            .as_secs_f64()
            .max(elapsed);
        Some(self.utilization * (horizon / elapsed))
    }

    /// When the limit will be hit at the current average rate, if that lands on
    /// or before the pacing deadline. Returns `None` if usage will not reach
    /// 100% in time (or already has). Unlike [`Self::projected_end_percent`],
    /// this fires as soon as utilization clears [`MIN_USAGE_FOR_PROJECTION`] —
    /// a genuine front-loaded burst warns immediately, bypassing the
    /// elapsed-time grace but honouring the utilization floor.
    #[must_use]
    pub fn projected_limit_date(
        &self,
        now: Timestamp,
        pacing_duration: Option<SignedDuration>,
    ) -> Option<Timestamp> {
        if self.utilization >= 100.0
            || self.utilization < MIN_USAGE_FOR_PROJECTION
            || self.resets_at <= now
        {
            return None;
        }
        let window_dur = self.window.duration();
        let window_secs = window_dur.as_secs_f64();
        let remaining = self.resets_at.duration_since(now).as_secs_f64();
        let elapsed = window_secs - remaining;
        if elapsed <= 0.0 {
            return None;
        }
        // Offset from the window start at which usage reaches 100%.
        let hit_offset = elapsed * 100.0 / self.utilization;
        let deadline_offset = pacing_duration
            .unwrap_or(window_dur)
            .as_secs_f64()
            .max(elapsed);
        if hit_offset >= deadline_offset {
            return None;
        }
        // hitDate = windowStart + hit_offset = resets_at + (hit_offset - window).
        let from_reset = SignedDuration::try_from_secs_f64(hit_offset - window_secs).ok()?;
        self.resets_at.checked_add(from_reset).ok()
    }
}

/// Result of comparing consumption rate against an even burn-down.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PacingAssessment {
    /// `min(utilization, 100) / expected_usage_percent`; 1.0 is exactly
    /// sustainable pace.
    pub ratio: f64,
    /// True when the ratio exceeds [`RISK_THRESHOLD`].
    pub at_risk: bool,
}

impl PacingAssessment {
    /// Assess a window's pacing at `now` against the full window span.
    ///
    /// Returns `None` while less than [`MIN_ELAPSED_FRACTION`] of the window
    /// has elapsed, when the window has already reset, or when nothing has
    /// been used yet. Shares its ratio math with [`UsageWindow::pace_ratio`].
    #[must_use]
    pub fn for_window(window: &UsageWindow, now: Timestamp) -> Option<Self> {
        if window.utilization <= 0.0 {
            return None;
        }
        let ratio = window.pace_ratio(now, None)?;
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

    fn now() -> Timestamp {
        "2026-07-17T12:00:00Z".parse().unwrap()
    }

    const SESSION: SignedDuration = SignedDuration::from_hours(5);

    /// A window with `utilization` used and `elapsed_fraction` of `window`
    /// elapsed at [`now`], mirroring upstream's `limit(...)` test helper.
    fn limit(utilization: f64, elapsed_fraction: f64, window: LimitWindow) -> UsageWindow {
        let full = window.duration().as_secs_f64();
        let remaining = full * (1.0 - elapsed_fraction);
        UsageWindow {
            utilization,
            resets_at: now() + SignedDuration::try_from_secs_f64(remaining).unwrap(),
            window,
        }
    }

    fn five_days() -> SignedDuration {
        weekly_pacing_duration(5)
    }

    // --- pace_ratio ------------------------------------------------------

    #[test]
    fn pace_ratio_at_sustainable_pace_is_one() {
        let w = limit(50.0, 0.5, LimitWindow::FiveHour);
        assert!((w.pace_ratio(now(), None).unwrap() - 1.0).abs() < 0.01);
    }

    #[test]
    fn pace_ratio_burning_fast_is_above_one() {
        // 50% used at 25% elapsed = 2.0.
        let w = limit(50.0, 0.25, LimitWindow::FiveHour);
        assert!((w.pace_ratio(now(), None).unwrap() - 2.0).abs() < 0.01);
    }

    #[test]
    fn pace_ratio_underusing_is_below_one() {
        // 20% used at 50% elapsed = 0.4.
        let w = limit(20.0, 0.5, LimitWindow::SevenDay);
        assert!((w.pace_ratio(now(), None).unwrap() - 0.4).abs() < 0.01);
    }

    #[test]
    fn pace_ratio_within_grace_period_is_none() {
        // Only 2% of the window elapsed (< MIN_ELAPSED_FRACTION of 5%).
        let w = limit(10.0, 0.02, LimitWindow::FiveHour);
        assert!(w.pace_ratio(now(), None).is_none());
    }

    #[test]
    fn pace_ratio_past_reset_is_none() {
        let w = UsageWindow {
            utilization: 50.0,
            resets_at: now() - SignedDuration::from_secs(60),
            window: LimitWindow::FiveHour,
        };
        assert!(w.pace_ratio(now(), None).is_none());
    }

    // --- weekly pace basis (5/6/7 days) ---------------------------------

    #[test]
    fn pace_ratio_five_day_pacing_expects_faster_burn() {
        // 40% used at 2 of 7 days elapsed: on a 5-day basis expected is 40% -> 1.0.
        let w = limit(40.0, 2.0 / 7.0, LimitWindow::SevenDay);
        assert!((w.pace_ratio(now(), Some(five_days())).unwrap() - 1.0).abs() < 0.01);
    }

    #[test]
    fn pace_ratio_past_pacing_duration_caps_elapsed_at_full() {
        // Day 6 of 7 on a 5-day basis: expected is 100%, so ratio == usage fraction.
        let w = limit(70.0, 6.0 / 7.0, LimitWindow::SevenDay);
        assert!((w.pace_ratio(now(), Some(five_days())).unwrap() - 0.7).abs() < 0.01);
    }

    // --- projection ------------------------------------------------------

    #[test]
    fn projected_end_percent_extrapolates_current_rate() {
        // 40% used at 50% elapsed -> 80% at window end.
        let w = limit(40.0, 0.5, LimitWindow::FiveHour);
        assert!((w.projected_end_percent(now(), None).unwrap() - 80.0).abs() < 0.5);
    }

    #[test]
    fn projected_limit_date_when_burning_fast_is_before_reset() {
        // 60% used at 50% elapsed -> hits 100% at 5/6 of the window, before reset.
        let w = limit(60.0, 0.5, LimitWindow::FiveHour);
        let hit = w.projected_limit_date(now(), None).unwrap();
        assert!(hit < w.resets_at);

        let window_start = w.resets_at - SESSION;
        let hit_fraction = hit.duration_since(window_start).as_secs_f64() / SESSION.as_secs_f64();
        assert!((hit_fraction - 5.0 / 6.0).abs() < 0.01);
    }

    #[test]
    fn projected_limit_date_when_on_sustainable_pace_is_none() {
        let w = limit(50.0, 0.5, LimitWindow::FiveHour);
        assert!(w.projected_limit_date(now(), None).is_none());
    }

    #[test]
    fn projected_limit_date_when_already_exceeded_is_none() {
        let w = limit(105.0, 0.5, LimitWindow::FiveHour);
        assert!(w.projected_limit_date(now(), None).is_none());
    }

    #[test]
    fn projected_limit_date_front_loaded_burst_within_grace_still_warns() {
        // 60% burned in the first 2% of the window: below the pace grace, but a
        // lockout is unambiguous so the projection still fires.
        let w = limit(60.0, 0.02, LimitWindow::FiveHour);
        assert!(
            w.pace_ratio(now(), None).is_none(),
            "grace still suppresses the ratio"
        );
        let hit = w.projected_limit_date(now(), None).unwrap();
        assert!(hit < w.resets_at);
    }

    #[test]
    fn projected_limit_date_trivial_early_usage_is_none() {
        // 2% used moments after reset is noise — below the usage floor.
        let w = limit(2.0, 0.01, LimitWindow::FiveHour);
        assert!(w.projected_limit_date(now(), None).is_none());
    }

    #[test]
    fn projection_respects_pacing_basis_no_contradiction() {
        // 60% used at 4/7 of the week, paced over 5 days: under-pace (<0.8), so
        // the projection must agree — end below 100%, no limit-hit warning.
        let w = limit(60.0, 4.0 / 7.0, LimitWindow::SevenDay);
        let ratio = w.pace_ratio(now(), Some(five_days())).unwrap();
        assert!(ratio < UNDERUSE_THRESHOLD);

        assert!(w.projected_limit_date(now(), Some(five_days())).is_none());
        let end = w.projected_end_percent(now(), Some(five_days())).unwrap();
        assert!(end < 100.0);
    }

    // --- PaceBand --------------------------------------------------------

    #[test]
    fn pace_band_classifies_each_tier() {
        assert_eq!(PaceBand::from_ratio(0.4), PaceBand::Underuse);
        assert_eq!(PaceBand::from_ratio(0.8), PaceBand::Sustainable);
        assert_eq!(PaceBand::from_ratio(1.2), PaceBand::Sustainable);
        assert_eq!(PaceBand::from_ratio(1.8), PaceBand::Overuse);
        assert_eq!(PaceBand::from_ratio(2.5), PaceBand::Overuse);
        assert_eq!(PaceBand::from_ratio(3.0), PaceBand::HeavyOveruse);
    }

    // --- PacingAssessment (precursor, full-window basis) ----------------

    #[test]
    fn sustainable_pace_is_not_at_risk() {
        let a =
            PacingAssessment::for_window(&limit(50.0, 0.5, LimitWindow::FiveHour), now()).unwrap();
        assert!((a.ratio - 1.0).abs() < 1e-9);
        assert!(!a.at_risk);
    }

    #[test]
    fn burning_fast_is_at_risk() {
        let a =
            PacingAssessment::for_window(&limit(80.0, 0.5, LimitWindow::FiveHour), now()).unwrap();
        assert!(a.at_risk);
    }

    #[test]
    fn fresh_window_yields_none() {
        assert!(
            PacingAssessment::for_window(&limit(5.0, 0.01, LimitWindow::FiveHour), now()).is_none()
        );
    }

    #[test]
    fn expired_window_yields_none() {
        let w = UsageWindow {
            utilization: 90.0,
            resets_at: now() - SignedDuration::from_mins(10),
            window: LimitWindow::FiveHour,
        };
        assert!(PacingAssessment::for_window(&w, now()).is_none());
    }

    #[test]
    fn unused_window_yields_none() {
        assert!(
            PacingAssessment::for_window(&limit(0.0, 0.5, LimitWindow::FiveHour), now()).is_none()
        );
    }
}
