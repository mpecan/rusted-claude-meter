//! Shared status colour palette and small SVG fragments reused by every
//! icon style, so the six templates in this crate agree on what "safe",
//! "warning", "critical" and "monochrome" mean.

use std::fmt::Write as _;

use meter_core::UsageStatus;

/// Apple system green / orange / red, matching the original `ClaudeMeter`.
pub const SAFE: &str = "#34C759";
pub const WARNING: &str = "#FF9500";
pub const CRITICAL: &str = "#FF3B30";
/// Apple system yellow — the mid-scale stop of the Battery fill gradient.
pub const YELLOW: &str = "#FFCC00";
/// Monochrome / template artwork is alpha-only black.
pub const MONO: &str = "#000000";
/// Apple system gray, used (at low opacity) for the unfilled track/background
/// of the bar-style icons, matching the reference's `Color.gray.opacity(...)`.
pub const GRAY: &str = "#8E8E93";
/// Secondary-series accent (the 7-day bar in Dual Bar), matching
/// `ClaudeMeter`'s violet weekly bar. Color mode only — mono ignores it.
pub const ACCENT: &str = "#AF52DE";

pub const fn status_color(status: UsageStatus) -> &'static str {
    match status {
        UsageStatus::Safe => SAFE,
        UsageStatus::Warning => WARNING,
        UsageStatus::Critical => CRITICAL,
    }
}

/// The primary ink colour for a style: pure black in monochrome/template
/// mode, otherwise the status colour.
pub const fn ink(mono: bool, status: UsageStatus) -> &'static str {
    if mono { MONO } else { status_color(status) }
}

/// A fill width proportional to `percent` of `max_width`, floored to
/// `min_width` so a nonzero-but-tiny percentage still renders a visible
/// sliver instead of rounding away to nothing. Shared by every style that
/// draws a proportional bar (battery, minimal, dual bar).
pub fn proportional_fill(max_width: f64, min_width: f64, percent: u8) -> f64 {
    (max_width * f64::from(percent) / 100.0).max(min_width)
}

/// The pacing at-risk badge dot, shared geometry across every style: a small
/// filled circle tucked into the very top-right corner of the (style-specific
/// width) canvas, clear of the main artwork. Empty when `at_risk` is false.
pub fn risk_badge(at_risk: bool, mono: bool, canvas_width: f64) -> String {
    if !at_risk {
        return String::new();
    }
    let badge = if mono { MONO } else { CRITICAL };
    let cx = canvas_width - 3.0;
    let mut out = String::with_capacity(48);
    let _ = write!(
        out,
        r#"<circle cx="{cx:.2}" cy="3" r="2.2" fill="{badge}"/>"#
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ink_is_black_only_when_mono() {
        assert_eq!(ink(true, UsageStatus::Critical), MONO);
        assert_eq!(ink(false, UsageStatus::Critical), CRITICAL);
    }

    #[test]
    fn risk_badge_is_empty_unless_at_risk() {
        assert_eq!(risk_badge(false, false, 63.0), "");
        assert!(risk_badge(true, false, 63.0).contains(CRITICAL));
        assert!(risk_badge(true, true, 63.0).contains(MONO));
        // The badge tracks the right edge of the (style-specific) canvas.
        assert!(risk_badge(true, false, 63.0).contains(r#"cx="60.00""#));
    }
}
