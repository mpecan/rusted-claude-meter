use std::hash::{Hash, Hasher};

use jiff::Timestamp;
use meter_core::{PaceBand, PaceKind, UsageSnapshot, UsageStatus};
use serde::{Deserialize, Serialize};

/// Logical icon height in CSS pixels: the common menu-bar/tray glyph height on
/// both macOS (22pt menu bar) and Linux (22/24px trays scale it well).
///
/// Width is style-dependent — the number-bearing styles are wider than tall,
/// like `ClaudeMeter`'s horizontal glyph-plus-percentage layout — so it lives
/// on [`IconStyle::logical_size`] rather than here.
pub const BASE_HEIGHT: u32 = 22;

/// Tray icon visual style — the six styles `ClaudeMeter` offers, selectable
/// live in Settings (issue #9). `Serialize`/`Deserialize` let the Tauri
/// command surface accept and persist a plain string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IconStyle {
    Battery,
    Circular,
    Minimal,
    Segments,
    DualBar,
    Gauge,
}

impl IconStyle {
    /// Logical `(width, height)` of this style's canvas, in viewBox units.
    ///
    /// Height is always [`BASE_HEIGHT`]. Width varies: the styles that bake in
    /// a monospaced percentage number (Battery, Minimal, Dual Bar) are wide to
    /// fit their glyph plus the number; the glyph-only styles (Circular,
    /// Segments, Gauge) stay near-square. Mirrors the reference app, whose
    /// menu-bar icons are a horizontal `HStack` and so wider than tall.
    pub const fn logical_size(self) -> (u32, u32) {
        let width = match self {
            Self::Battery => 66,
            Self::Minimal => 44,
            Self::Segments => 34,
            Self::DualBar => 70,
            // The glyph-only styles stay near-square around the 22px height.
            Self::Circular | Self::Gauge => 26,
        };
        (width, BASE_HEIGHT)
    }
}

/// Raster scale factor applied uniformly to a style's logical size: 1x for
/// standard density, 2x for `HiDPI` (Retina menu bars, scaled Linux trays).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Scale {
    X1,
    X2,
}

impl Scale {
    pub const fn factor(self) -> u32 {
        match self {
            Self::X1 => 1,
            Self::X2 => 2,
        }
    }

    pub(crate) const fn factor_f32(self) -> f32 {
        match self {
            Self::X1 => 1.0,
            Self::X2 => 2.0,
        }
    }
}

/// Everything the renderer needs to draw one icon.
///
/// This is also the icon-cache key, mirroring `ClaudeMeter`'s `IconCache`:
/// the percentage is pre-rounded to a whole number so consecutive fetches
/// that move utilization by a fraction of a percent hit the cache instead of
/// re-rasterizing.
///
/// [`PartialEq`]/[`Eq`]/[`Hash`] are hand-written rather than derived because
/// [`Self::pace_ratio`] is an `f64` (no total `Eq`/`Hash`): it is keyed by its
/// one-decimal rendered form ("1.8"), the exact string the override text shows,
/// so consecutive fetches that nudge the ratio a hair still hit the cache. The
/// colour [`Self::pace_band`] is keyed *separately* — two ratios that round to
/// the same text but straddle a band boundary (1.16 vs 1.24 → both "1.2", but
/// sustainable vs overuse) must not collapse to one entry and render the wrong
/// colour.
#[derive(Debug, Clone, Copy)]
pub struct IconState {
    pub style: IconStyle,
    /// Displayed percentage, rounded to the nearest whole number and clamped
    /// to `0..=100` (the API can report utilization above 100).
    pub percent: u8,
    /// A second percentage for styles that show two windows at once (Dual
    /// Bar: 5-hour on top, 7-day on the bottom). Ignored by every other
    /// style, but still part of the cache key so it never causes a stale
    /// render to be reused. `0` when the snapshot has no 7-day window.
    pub secondary_percent: u8,
    pub status: UsageStatus,
    /// Pacing at-risk indicator (the badge dot), from [`PacingAssessment`].
    /// Superseded by the flame/snowflake badge when [`Self::pace_kind`] is set.
    pub at_risk: bool,
    /// Off-pace badge to draw in place of the at-risk dot: a flame (hot) or a
    /// snowflake (cold). `None` outside pace-first display.
    pub pace_kind: Option<PaceKind>,
    /// Colour band the pace ratio falls into — the source of truth for the
    /// override colour, and a cache key in its own right (see the type doc).
    /// `None` outside pace-first display.
    pub pace_band: Option<PaceBand>,
    /// Pace ratio driving the pace-first override text ("1.8×"). `None` outside
    /// pace-first display. Cached by its one-decimal rendered form, not raw bits.
    pub pace_ratio: Option<f64>,
    /// Monochrome variant: alpha-only black artwork. On macOS this becomes a
    /// template image so the system recolours it for the menu bar appearance.
    pub mono: bool,
    pub scale: Scale,
}

impl PartialEq for IconState {
    fn eq(&self, other: &Self) -> bool {
        self.style == other.style
            && self.percent == other.percent
            && self.secondary_percent == other.secondary_percent
            && self.status == other.status
            && self.effective_at_risk() == other.effective_at_risk()
            && self.pace_kind == other.pace_kind
            && self.pace_band == other.pace_band
            && self.pace_ratio_key() == other.pace_ratio_key()
            && self.mono == other.mono
            && self.scale == other.scale
    }
}

impl Eq for IconState {}

impl Hash for IconState {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.style.hash(state);
        self.percent.hash(state);
        self.secondary_percent.hash(state);
        self.status.hash(state);
        self.effective_at_risk().hash(state);
        self.pace_kind.hash(state);
        self.pace_band.hash(state);
        self.pace_ratio_key().hash(state);
        self.mono.hash(state);
        self.scale.hash(state);
    }
}

impl IconState {
    /// The pace ratio in the exact one-decimal form the override text renders
    /// (`palette::primary_label` uses `format!("{ratio:.1}")`), so 1.81 and 1.84
    /// (both "1.8×") share a cache entry while a change in the displayed digit
    /// does not. Keyed off that same formatting — not a separate
    /// multiply-round-cast, which disagrees with `{:.1}` at `.x5` boundaries
    /// (e.g. 0.85 renders "0.8" but `(0.85*10.0).round()` is 9, colliding with
    /// 0.851's "0.9") and would show a stale ratio.
    fn pace_ratio_key(&self) -> Option<String> {
        self.pace_ratio.map(|ratio| format!("{ratio:.1}"))
    }

    /// Whether the at-risk dot actually affects the pixels: `palette::badge`
    /// draws it only when no pace kind is set (a flame/snowflake supersedes it),
    /// so a differing `at_risk` behind a `pace_kind` must not split cache
    /// entries that render byte-identical SVG.
    const fn effective_at_risk(&self) -> bool {
        self.at_risk && self.pace_kind.is_none()
    }

    /// Attach a pace-first display overlay: the flame/snowflake badge (`kind`)
    /// and the ratio driving the override text/colour. The colour band is
    /// derived from the ratio so it can never drift from the number shown.
    #[must_use]
    pub fn with_pace(mut self, ratio: Option<f64>, kind: Option<PaceKind>) -> Self {
        self.pace_ratio = ratio;
        self.pace_band = ratio.map(PaceBand::from_ratio);
        self.pace_kind = kind;
        self
    }

    /// Derive the icon state from a usage snapshot.
    ///
    /// The displayed percentage is the five-hour headline window (the number
    /// the original `ClaudeMeter` gauges), falling back to seven-day and then
    /// to the busiest scoped limit. `secondary_percent` is specifically the
    /// seven-day window (0 when the snapshot has none), for Dual Bar's second
    /// row. Status is the worst across every window, and the at-risk badge
    /// lights up when any window is pacing at risk.
    pub fn from_snapshot(
        snapshot: &UsageSnapshot,
        now: Timestamp,
        style: IconStyle,
        mono: bool,
        scale: Scale,
    ) -> Self {
        let primary = snapshot
            .five_hour
            .as_ref()
            .or(snapshot.seven_day.as_ref())
            .map(|window| window.utilization)
            .or_else(|| {
                snapshot
                    .scoped
                    .iter()
                    .map(|limit| limit.usage.utilization)
                    .max_by(f64::total_cmp)
            })
            .unwrap_or(0.0);
        let secondary_percent = snapshot
            .seven_day
            .as_ref()
            .map_or(0.0, |window| window.utilization);
        Self {
            style,
            percent: round_percent(primary),
            secondary_percent: round_percent(secondary_percent),
            // Colour follows the primary (session) window shown as the number,
            // matching `ClaudeMeter`, whose menu-bar icon is always driven by
            // the 5-hour session status — not the worst window overall (that
            // still drives the popover cards and the tray status line).
            status: UsageStatus::from_utilization(primary),
            at_risk: snapshot.at_risk(now),
            // Pace-first overlay is a display-mode choice computed by the app
            // shell (headline-only, gated behind `pace_first_display`); the
            // base icon carries none. Callers layer it on via [`Self::with_pace`].
            pace_kind: None,
            pace_band: None,
            pace_ratio: None,
            mono,
            scale,
        }
    }
}

/// Round a raw utilization percentage to the whole number the icon displays,
/// clamped to `0..=100` (the API can report utilization above 100).
pub const fn round_percent(percent: f64) -> u8 {
    // Clamped to 0..=100 first, so the cast can neither truncate nor wrap.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    {
        percent.clamp(0.0, 100.0).round() as u8
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use jiff::SignedDuration;
    use meter_core::{LimitWindow, ScopedLimit, UsageWindow};
    use pretty_assertions::assert_eq;

    fn now() -> Timestamp {
        "2026-07-17T12:00:00Z".parse().unwrap()
    }

    fn window(utilization: f64, resets_in_minutes: i64, kind: LimitWindow) -> UsageWindow {
        UsageWindow {
            utilization,
            resets_at: now() + SignedDuration::from_mins(resets_in_minutes),
            window: kind,
        }
    }

    fn state(snapshot: &UsageSnapshot) -> IconState {
        IconState::from_snapshot(snapshot, now(), IconStyle::Battery, false, Scale::X1)
    }

    #[test]
    fn percent_rounds_and_clamps() {
        assert_eq!(round_percent(41.5), 42);
        assert_eq!(round_percent(130.0), 100);
        assert_eq!(round_percent(-3.0), 0);
    }

    #[test]
    fn from_snapshot_prefers_the_five_hour_headline() {
        let snapshot = UsageSnapshot {
            five_hour: Some(window(30.0, 150, LimitWindow::FiveHour)),
            seven_day: Some(window(90.0, 5000, LimitWindow::SevenDay)),
            scoped: vec![],
            spend: None,
            fetched_at: now(),
        };
        let s = state(&snapshot);
        assert_eq!(s.percent, 30);
        // ...and the icon colour follows that same session window (30% = safe),
        // matching ClaudeMeter's session-driven menu-bar colour, even though a
        // seven-day window is critical.
        assert_eq!(s.status, UsageStatus::Safe);
    }

    #[test]
    fn secondary_percent_is_the_seven_day_window_independent_of_the_headline() {
        let snapshot = UsageSnapshot {
            five_hour: Some(window(30.0, 150, LimitWindow::FiveHour)),
            seven_day: Some(window(64.0, 5000, LimitWindow::SevenDay)),
            scoped: vec![],
            spend: None,
            fetched_at: now(),
        };
        let s = state(&snapshot);
        assert_eq!(s.percent, 30);
        assert_eq!(s.secondary_percent, 64);
    }

    #[test]
    fn secondary_percent_is_zero_without_a_seven_day_window() {
        let snapshot = UsageSnapshot {
            five_hour: Some(window(30.0, 150, LimitWindow::FiveHour)),
            seven_day: None,
            scoped: vec![],
            spend: None,
            fetched_at: now(),
        };
        assert_eq!(state(&snapshot).secondary_percent, 0);
    }

    #[test]
    fn from_snapshot_falls_back_to_busiest_scoped_limit() {
        let snapshot = UsageSnapshot {
            five_hour: None,
            seven_day: None,
            scoped: vec![
                ScopedLimit {
                    display_name: "Sonnet".to_owned(),
                    model_id: None,
                    usage: window(12.0, 5000, LimitWindow::SevenDay),
                    is_active: true,
                },
                ScopedLimit {
                    display_name: "Fable".to_owned(),
                    model_id: None,
                    usage: window(47.0, 5000, LimitWindow::SevenDay),
                    is_active: true,
                },
            ],
            spend: None,
            fetched_at: now(),
        };
        assert_eq!(state(&snapshot).percent, 47);
    }

    #[test]
    fn from_snapshot_flags_pacing_risk_from_any_window() {
        // Half the five-hour window gone, 90% used → ratio 1.8, at risk.
        let snapshot = UsageSnapshot {
            five_hour: Some(window(90.0, 150, LimitWindow::FiveHour)),
            seven_day: None,
            scoped: vec![],
            spend: None,
            fetched_at: now(),
        };
        assert!(state(&snapshot).at_risk);

        // Sustainable pace: not at risk.
        let calm = UsageSnapshot {
            five_hour: Some(window(50.0, 150, LimitWindow::FiveHour)),
            seven_day: None,
            scoped: vec![],
            spend: None,
            fetched_at: now(),
        };
        assert!(!state(&calm).at_risk);

        // A scoped limit pacing hot (90% used, 4 of 7 days left → ratio 2.1)
        // lights the badge even when the five-hour headline is calm.
        let hot_scoped = UsageSnapshot {
            five_hour: Some(window(50.0, 150, LimitWindow::FiveHour)),
            seven_day: None,
            scoped: vec![ScopedLimit {
                display_name: "Fable".to_owned(),
                model_id: None,
                usage: window(90.0, 4 * 24 * 60, LimitWindow::SevenDay),
                is_active: true,
            }],
            spend: None,
            fetched_at: now(),
        };
        assert!(state(&hot_scoped).at_risk);
    }

    fn hash_of(state: &IconState) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut hasher = DefaultHasher::new();
        state.hash(&mut hasher);
        hasher.finish()
    }

    fn base() -> IconState {
        IconState {
            style: IconStyle::Battery,
            percent: 50,
            secondary_percent: 0,
            status: UsageStatus::Warning,
            at_risk: false,
            pace_kind: None,
            pace_band: None,
            pace_ratio: None,
            mono: false,
            scale: Scale::X1,
        }
    }

    #[test]
    fn with_pace_derives_the_band_from_the_ratio() {
        let hot = base().with_pace(Some(3.0), Some(PaceKind::Hot));
        assert_eq!(hot.pace_band, Some(meter_core::PaceBand::HeavyOveruse));
        assert_eq!(hot.pace_kind, Some(PaceKind::Hot));
        assert_eq!(hot.pace_ratio, Some(3.0));
    }

    #[test]
    fn ratios_that_render_the_same_share_a_cache_key() {
        // 1.81 and 1.84 both render "1.8×" and both sit in the overuse band, so
        // they must be equal and hash equal — one cache entry, not two.
        let a = base().with_pace(Some(1.81), Some(PaceKind::Hot));
        let b = base().with_pace(Some(1.84), Some(PaceKind::Hot));
        assert_eq!(a, b);
        assert_eq!(hash_of(&a), hash_of(&b));
    }

    #[test]
    fn ratios_that_round_equal_but_straddle_a_band_stay_distinct() {
        // Both round to "1.2", but 1.16 is sustainable (green) and 1.24 is
        // overuse (orange). Keying on the band as well as the rounded ratio keeps
        // them separate cache entries so neither borrows the other's colour.
        let sustainable = base().with_pace(Some(1.16), None);
        let overuse = base().with_pace(Some(1.24), None);
        assert_eq!(sustainable.pace_ratio_key(), overuse.pace_ratio_key());
        assert_ne!(sustainable.pace_band, overuse.pace_band);
        assert_ne!(sustainable, overuse);
    }

    #[test]
    fn ratios_that_render_different_text_never_share_a_cache_key() {
        // At a `.x5` boundary the displayed one-decimal form and a naive
        // `(ratio*10.0).round()` disagree: 1.15 renders "1.1", 1.19 renders
        // "1.2" (both sustainable), yet both scale-and-round to 12 — a key the
        // cache would collide, showing one ratio's icon for the other. Keying
        // the cache off the exact rendered string forbids that.
        let low = base().with_pace(Some(1.15), Some(PaceKind::Hot));
        let high = base().with_pace(Some(1.19), Some(PaceKind::Hot));
        // The keys are exactly the strings the override text renders.
        assert_eq!(low.pace_ratio_key().as_deref(), Some("1.1"));
        assert_eq!(high.pace_ratio_key().as_deref(), Some("1.2"));
        assert_eq!(low.pace_band, high.pace_band, "both sustainable");
        assert_ne!(low, high, "different displayed text must not compare equal");
        assert_ne!(
            hash_of(&low),
            hash_of(&high),
            "different displayed text must not hash equal"
        );
    }

    #[test]
    fn at_risk_is_ignored_by_the_cache_key_once_a_pace_kind_is_set() {
        // `palette::badge` draws the at-risk dot only when `pace_kind` is None;
        // under a flame/snowflake the dot is never drawn, so two states that
        // differ only in `at_risk` render byte-identical SVG and must share one
        // cache entry.
        let mut with = base().with_pace(Some(1.8), Some(PaceKind::Hot));
        with.at_risk = true;
        let mut without = base().with_pace(Some(1.8), Some(PaceKind::Hot));
        without.at_risk = false;
        assert_eq!(with, without);
        assert_eq!(hash_of(&with), hash_of(&without));

        // But with no pace kind the dot *is* drawn, so at_risk still splits them.
        let mut dot = base();
        dot.at_risk = true;
        let mut no_dot = base();
        no_dot.at_risk = false;
        assert_ne!(dot, no_dot);
    }

    #[test]
    fn empty_snapshot_renders_as_zero_and_safe() {
        let empty = UsageSnapshot {
            five_hour: None,
            seven_day: None,
            scoped: vec![],
            spend: None,
            fetched_at: now(),
        };
        let s = state(&empty);
        assert_eq!(s.percent, 0);
        assert_eq!(s.status, UsageStatus::Safe);
        assert!(!s.at_risk);
    }
}
