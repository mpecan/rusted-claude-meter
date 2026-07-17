//! Battery-style SVG template.
//!
//! Geometry lives in a 22x22 viewBox ([`crate::BASE_SIZE`]); the rasterizer
//! scales it, so 1x and 2x share one template. The body outline is stroked,
//! the charge fill width is proportional to the displayed percent, and the
//! pacing at-risk badge is a dot in the free space above the battery cap.

use std::fmt::Write as _;

use crate::palette::{ink, proportional_fill, risk_badge};
use crate::state::IconState;
use crate::svg::svg_document;

/// Charge fill geometry: inset inside the stroked body.
const FILL_X: f64 = 3.0;
const FILL_MAX_WIDTH: f64 = 14.0;
/// The thinnest visible sliver, so 1–3% does not round to nothing.
const FILL_MIN_WIDTH: f64 = 1.0;

pub fn svg(state: IconState) -> String {
    let ink = ink(state.mono, state.status);

    svg_document(640, |out| {
        // Body outline and terminal cap.
        let _ = write!(
            out,
            r#"<rect x="1.25" y="6.25" width="17.5" height="9.5" rx="2.75" fill="none" stroke="{ink}" stroke-width="1.5"/><rect x="19.75" y="9.25" width="1.5" height="3.5" rx="0.75" fill="{ink}"/>"#
        );
        // Charge fill, proportional to percent.
        if state.percent > 0 {
            let width = proportional_fill(FILL_MAX_WIDTH, FILL_MIN_WIDTH, state.percent);
            let _ = write!(
                out,
                r#"<rect x="{FILL_X}" y="8" width="{width:.2}" height="6" rx="1.2" fill="{ink}"/>"#
            );
        }
        out.push_str(&risk_badge(state.at_risk, state.mono));
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::palette::{CRITICAL, MONO, SAFE, WARNING};
    use crate::state::{IconStyle, Scale};
    use meter_core::UsageStatus;

    fn state(percent: u8, status: UsageStatus, at_risk: bool, mono: bool) -> IconState {
        IconState {
            style: IconStyle::Battery,
            percent,
            secondary_percent: 0,
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
