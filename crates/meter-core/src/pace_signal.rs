use serde::{Deserialize, Serialize};

/// Direction of an off-pace burn rate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaceKind {
    /// Burning faster than sustainable — likely to hit the limit before reset.
    Hot,
    /// Underusing — quota likely to go unused before reset.
    Cold,
}

/// An off-pace signal for the tray/popover badge, naming the window that
/// produced it. Mirrors `ClaudeMeter`'s `PaceSignal`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PaceSignal {
    pub kind: PaceKind,
    /// Usage fraction divided by elapsed-time fraction (1.0 = sustainable pace).
    pub ratio: f64,
    /// Human-readable window name (e.g. "5-hour", "7-day").
    pub window_name: String,
    /// Actual utilization percentage.
    pub used_percent: f64,
    /// Utilization percentage the pace plan expected by now.
    pub expected_percent: f64,
    /// Weekly pace basis in days, when the quota is paced over fewer days than
    /// the window.
    pub pace_days: Option<u8>,
}

impl PaceSignal {
    /// Tooltip text explaining the signal and which window drives it. The exact
    /// wording (including the `×` and `-` punctuation) is pinned by upstream's
    /// `PaceSignalTests`.
    #[must_use]
    pub fn tooltip(&self) -> String {
        // `f64::round` rounds half away from zero, matching Swift's `rounded()`;
        // formatting the already-rounded value with `{:.0}` avoids a lossy cast.
        let used = format!("{:.0}", self.used_percent.round());
        let expected = format!("{:.0}", self.expected_percent.round());
        let ratio = format!("{:.1}", self.ratio);
        let window = match self.pace_days {
            Some(days) if days != 7 => format!("{days}/7-day window"),
            _ => format!("{} window", self.window_name),
        };

        match self.kind {
            PaceKind::Hot => format!(
                "Used {used}% vs {expected}% expected by now - burning {ratio}× sustainable pace ({window})"
            ),
            PaceKind::Cold => format!(
                "Used {used}% vs {expected}% expected by now - {ratio}× pace, quota may go unused ({window})"
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn tooltip_cold_shows_used_vs_expected_and_pace_basis() {
        let signal = PaceSignal {
            kind: PaceKind::Cold,
            ratio: 0.29,
            window_name: "7-day".to_owned(),
            used_percent: 11.4,
            expected_percent: 40.0,
            pace_days: Some(5),
        };

        assert_eq!(
            signal.tooltip(),
            "Used 11% vs 40% expected by now - 0.3× pace, quota may go unused (5/7-day window)"
        );
    }

    #[test]
    fn tooltip_hot_omits_pace_basis_for_session_window() {
        let signal = PaceSignal {
            kind: PaceKind::Hot,
            ratio: 1.8,
            window_name: "5-hour".to_owned(),
            used_percent: 72.0,
            expected_percent: 40.0,
            pace_days: None,
        };

        assert_eq!(
            signal.tooltip(),
            "Used 72% vs 40% expected by now - burning 1.8× sustainable pace (5-hour window)"
        );
    }

    #[test]
    fn tooltip_full_week_basis_uses_window_name() {
        // `paceDays == 7` is the full week, so the label falls back to the
        // window name rather than "7/7-day window".
        let signal = PaceSignal {
            kind: PaceKind::Hot,
            ratio: 1.4,
            window_name: "7-day".to_owned(),
            used_percent: 40.0,
            expected_percent: 28.0,
            pace_days: Some(7),
        };

        assert!(signal.tooltip().ends_with("(7-day window)"));
    }
}
