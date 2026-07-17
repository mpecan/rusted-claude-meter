//! Battery-style SVG template.
//!
//! Geometry lives in a 22x22 viewBox ([`crate::BASE_SIZE`]); the rasterizer
//! scales it, so 1x and 2x share one template. The body outline is stroked,
//! the charge fill width is proportional to the displayed percent, and the
//! pacing at-risk badge is a dot in the free space above the battery cap.

use std::fmt::Write as _;

use meter_core::UsageStatus;

use crate::state::IconState;

/// Apple system green / orange / red, matching the original `ClaudeMeter`.
const SAFE: &str = "#34C759";
const WARNING: &str = "#FF9500";
const CRITICAL: &str = "#FF3B30";
/// Monochrome / template artwork is alpha-only black.
const MONO: &str = "#000000";

/// Charge fill geometry: inset inside the stroked body.
const FILL_X: f64 = 3.0;
const FILL_MAX_WIDTH: f64 = 14.0;
/// The thinnest visible sliver, so 1–3% does not round to nothing.
const FILL_MIN_WIDTH: f64 = 1.0;

pub fn svg(state: IconState) -> String {
    let ink = if state.mono {
        MONO
    } else {
        status_color(state.status)
    };
    let badge = if state.mono { MONO } else { CRITICAL };

    let mut out = String::with_capacity(640);
    out.push_str(r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 22 22">"#);
    // Body outline and terminal cap.
    let _ = write!(
        out,
        r#"<rect x="1.25" y="6.25" width="17.5" height="9.5" rx="2.75" fill="none" stroke="{ink}" stroke-width="1.5"/><rect x="19.75" y="9.25" width="1.5" height="3.5" rx="0.75" fill="{ink}"/>"#
    );
    // Charge fill, proportional to percent.
    if state.percent > 0 {
        let width = (FILL_MAX_WIDTH * f64::from(state.percent) / 100.0).max(FILL_MIN_WIDTH);
        let _ = write!(
            out,
            r#"<rect x="{FILL_X}" y="8" width="{width:.2}" height="6" rx="1.2" fill="{ink}"/>"#
        );
    }
    // Pacing at-risk badge.
    if state.at_risk {
        let _ = write!(
            out,
            r#"<circle cx="17.75" cy="3.4" r="2.2" fill="{badge}"/>"#
        );
    }
    out.push_str("</svg>");
    out
}

const fn status_color(status: UsageStatus) -> &'static str {
    match status {
        UsageStatus::Safe => SAFE,
        UsageStatus::Warning => WARNING,
        UsageStatus::Critical => CRITICAL,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{IconStyle, Scale};

    fn state(percent: u8, status: UsageStatus, at_risk: bool, mono: bool) -> IconState {
        IconState {
            style: IconStyle::Battery,
            percent,
            status,
            at_risk,
            mono,
            scale: Scale::X1,
        }
    }

    #[test]
    fn empty_battery_has_no_fill_rect() {
        let svg = svg(state(0, UsageStatus::Safe, false, false));
        assert_eq!(svg.matches("<rect").count(), 2, "outline and cap only");
        assert!(!svg.contains("circle"));
    }

    #[test]
    fn fill_width_is_proportional_and_floored() {
        let half = svg(state(50, UsageStatus::Warning, false, false));
        assert!(half.contains(r#"width="7.00""#), "50% of 14: {half}");
        let sliver = svg(state(2, UsageStatus::Safe, false, false));
        assert!(sliver.contains(r#"width="1.00""#), "min sliver: {sliver}");
    }

    #[test]
    fn colours_follow_status_and_mono_wins() {
        assert!(svg(state(10, UsageStatus::Safe, false, false)).contains(SAFE));
        assert!(svg(state(60, UsageStatus::Warning, false, false)).contains(WARNING));
        assert!(svg(state(90, UsageStatus::Critical, false, false)).contains(CRITICAL));
        let mono = svg(state(90, UsageStatus::Critical, true, true));
        assert!(!mono.contains(CRITICAL));
        assert!(mono.contains(MONO));
    }

    #[test]
    fn at_risk_adds_the_badge_dot() {
        assert!(svg(state(70, UsageStatus::Warning, true, false)).contains("<circle"));
    }
}
