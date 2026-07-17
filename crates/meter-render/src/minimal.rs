//! Minimal SVG template: the plainest style — a single unadorned fill bar,
//! no outline, no cap, no track markings. Mirrors `ClaudeMeter`'s
//! `MinimalIcon` intent (percentage and nothing else) without depending on a
//! system font being installed to rasterize text.

use std::fmt::Write as _;

use crate::palette::{ink, proportional_fill, risk_badge};
use crate::state::IconState;
use crate::svg::svg_document;

const TRACK_X: f64 = 2.0;
const TRACK_WIDTH: f64 = 18.0;
const TRACK_Y: f64 = 9.0;
const TRACK_HEIGHT: f64 = 4.0;
/// The thinnest visible sliver, so 1–3% does not round away to nothing.
const MIN_FILL_WIDTH: f64 = 1.0;

pub fn svg(state: IconState) -> String {
    let ink = ink(state.mono, state.status);

    svg_document(384, |out| {
        let _ = write!(
            out,
            r#"<rect x="{TRACK_X}" y="{TRACK_Y}" width="{TRACK_WIDTH}" height="{TRACK_HEIGHT}" rx="2" fill="{ink}" fill-opacity="0.2"/>"#
        );
        if state.percent > 0 {
            let width = proportional_fill(TRACK_WIDTH, MIN_FILL_WIDTH, state.percent);
            let _ = write!(
                out,
                r#"<rect x="{TRACK_X}" y="{TRACK_Y}" width="{width:.2}" height="{TRACK_HEIGHT}" rx="2" fill="{ink}"/>"#
            );
        }
        out.push_str(&risk_badge(state.at_risk, state.mono));
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::palette::{CRITICAL, MONO, SAFE};
    use crate::state::{IconStyle, Scale};
    use meter_core::UsageStatus;

    fn state(percent: u8, status: UsageStatus, at_risk: bool, mono: bool) -> IconState {
        IconState {
            style: IconStyle::Minimal,
            percent,
            secondary_percent: 0,
            status,
            at_risk,
            mono,
            scale: Scale::X1,
        }
    }

    #[test]
    fn empty_bar_has_only_the_track() {
        let svg = svg(state(0, UsageStatus::Safe, false, false));
        assert_eq!(svg.matches("<rect").count(), 1);
    }

    #[test]
    fn fill_width_is_proportional_and_floored() {
        let half = svg(state(50, UsageStatus::Warning, false, false));
        assert!(half.contains(r#"width="9.00""#), "50% of 18: {half}");
        let sliver = svg(state(2, UsageStatus::Safe, false, false));
        assert!(sliver.contains(r#"width="1.00""#), "min sliver: {sliver}");
    }

    #[test]
    fn colours_follow_status_and_mono_wins() {
        assert!(svg(state(10, UsageStatus::Safe, false, false)).contains(SAFE));
        assert!(svg(state(90, UsageStatus::Critical, false, false)).contains(CRITICAL));
        let mono = svg(state(90, UsageStatus::Critical, false, true));
        assert!(!mono.contains(CRITICAL));
        assert!(mono.contains(MONO));
    }

    #[test]
    fn at_risk_adds_the_badge_dot() {
        assert!(svg(state(70, UsageStatus::Warning, true, false)).contains("<circle"));
    }
}
