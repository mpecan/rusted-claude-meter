use jiff::Timestamp;
use meter_core::{UsageSnapshot, UsageStatus};
use serde::{Deserialize, Serialize};

/// Logical icon size in CSS pixels: the common menu-bar/tray glyph size on
/// both macOS (22pt menu bar) and Linux (22/24px trays scale it well).
pub const BASE_SIZE: u32 = 22;

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

/// Raster scale factor over [`BASE_SIZE`]: 1x for standard density, 2x for
/// `HiDPI` (Retina menu bars, scaled Linux trays).
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
    pub at_risk: bool,
    /// Monochrome variant: alpha-only black artwork. On macOS this becomes a
    /// template image so the system recolours it for the menu bar appearance.
    pub mono: bool,
    pub scale: Scale,
}

impl IconState {
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
        let percent = snapshot
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
            percent: round_percent(percent),
            secondary_percent: round_percent(secondary_percent),
            status: snapshot.overall_status(),
            at_risk: snapshot.at_risk(now),
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
            fetched_at: now(),
        };
        let s = state(&snapshot);
        assert_eq!(s.percent, 30);
        // ...but status is still the worst window.
        assert_eq!(s.status, UsageStatus::Critical);
    }

    #[test]
    fn secondary_percent_is_the_seven_day_window_independent_of_the_headline() {
        let snapshot = UsageSnapshot {
            five_hour: Some(window(30.0, 150, LimitWindow::FiveHour)),
            seven_day: Some(window(64.0, 5000, LimitWindow::SevenDay)),
            scoped: vec![],
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
            fetched_at: now(),
        };
        assert!(state(&snapshot).at_risk);

        // Sustainable pace: not at risk.
        let calm = UsageSnapshot {
            five_hour: Some(window(50.0, 150, LimitWindow::FiveHour)),
            seven_day: None,
            scoped: vec![],
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
            fetched_at: now(),
        };
        assert!(state(&hot_scoped).at_risk);
    }

    #[test]
    fn empty_snapshot_renders_as_zero_and_safe() {
        let empty = UsageSnapshot {
            five_hour: None,
            seven_day: None,
            scoped: vec![],
            fetched_at: now(),
        };
        let s = state(&empty);
        assert_eq!(s.percent, 0);
        assert_eq!(s.status, UsageStatus::Safe);
        assert!(!s.at_risk);
    }
}
