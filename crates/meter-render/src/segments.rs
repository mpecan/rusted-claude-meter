//! Segments SVG template: a signal-strength-style row of quantized blocks,
//! mirroring `ClaudeMeter`'s `SegmentedBarIcon`. Five segments, each covering
//! a 20-point band; a segment lights up once `percent` reaches its band's
//! floor, so the display always quantizes to whole segments rather than a
//! continuous fill.

use std::fmt::Write as _;

use meter_core::UsageStatus;

use crate::palette::{ink, risk_badge, status_color};
use crate::state::IconState;

const COUNT: u32 = 5;
const WIDTH: f64 = 2.6;
const GAP: f64 = 1.0;
const START_X: f64 = 2.5;
const BASE_HEIGHT: f64 = 6.0;
const HEIGHT_STEP: f64 = 2.0;
/// Segments sit on this baseline, growing upward.
const BASELINE: f64 = 18.0;

pub fn svg(state: IconState) -> String {
    let ink = ink(state.mono, state.status);
    // How many of the five segments are lit — matching the original's
    // `percentage >= index * 20` quantization exactly, which always lights
    // the first segment (`percentage >= 0 * 20` is trivially true), even at
    // 0%.
    let lit = (1 + u32::from(state.percent) / 20).min(COUNT);

    let mut out = String::with_capacity(768);
    out.push_str(r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 22 22">"#);
    for index in 0..COUNT {
        let x = f64::from(index).mul_add(WIDTH + GAP, START_X);
        let height = HEIGHT_STEP.mul_add(f64::from(index), BASE_HEIGHT);
        let y = BASELINE - height;
        if index < lit {
            let fill = segment_color(state.mono, index, ink);
            let _ = write!(
                out,
                r#"<rect x="{x:.2}" y="{y:.2}" width="{WIDTH}" height="{height:.2}" rx="1" fill="{fill}"/>"#
            );
        } else {
            let _ = write!(
                out,
                r#"<rect x="{x:.2}" y="{y:.2}" width="{WIDTH}" height="{height:.2}" rx="1" fill="{ink}" fill-opacity="0.2"/>"#
            );
        }
    }
    out.push_str(&risk_badge(state.at_risk, state.mono));
    out.push_str("</svg>");
    out
}

/// Lit segments shade green → orange → red by position (a gradient effect,
/// independent of the overall status), same as the original `SwiftUI` icon.
/// Monochrome ignores the gradient — every lit segment is pure ink.
fn segment_color(mono: bool, index: u32, mono_ink: &'static str) -> &'static str {
    if mono {
        return mono_ink;
    }
    let band_percent = f64::from(index + 1) / f64::from(COUNT) * 100.0;
    status_color(UsageStatus::from_utilization(band_percent))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::palette::MONO;
    use crate::state::{IconStyle, Scale};

    fn state(percent: u8, status: UsageStatus, at_risk: bool, mono: bool) -> IconState {
        IconState {
            style: IconStyle::Segments,
            percent,
            secondary_percent: 0,
            status,
            at_risk,
            mono,
            scale: Scale::X1,
        }
    }

    fn lit_count(svg: &str) -> usize {
        // Lit segments never carry `fill-opacity`; dim ones always do.
        svg.matches("<rect").count() - svg.matches("fill-opacity").count()
    }

    #[test]
    fn zero_percent_still_lights_the_first_segment() {
        // Matches the original's `percentage >= index * 20`: for index 0
        // that's `percentage >= 0`, always true.
        assert_eq!(
            lit_count(&svg(state(0, UsageStatus::Safe, false, false))),
            1
        );
    }

    #[test]
    fn quantizes_into_five_bands() {
        assert_eq!(
            lit_count(&svg(state(1, UsageStatus::Safe, false, false))),
            1
        );
        assert_eq!(
            lit_count(&svg(state(19, UsageStatus::Safe, false, false))),
            1
        );
        assert_eq!(
            lit_count(&svg(state(20, UsageStatus::Safe, false, false))),
            2
        );
        assert_eq!(
            lit_count(&svg(state(79, UsageStatus::Warning, false, false))),
            4
        );
        assert_eq!(
            lit_count(&svg(state(100, UsageStatus::Critical, false, false))),
            5
        );
    }

    #[test]
    fn always_renders_five_segments() {
        let svg = svg(state(37, UsageStatus::Safe, false, false));
        assert_eq!(svg.matches("<rect").count(), 5);
    }

    #[test]
    fn mono_wins_over_the_position_gradient() {
        let mono = svg(state(100, UsageStatus::Critical, false, true));
        assert!(!mono.contains(crate::palette::SAFE));
        assert!(mono.contains(MONO));
    }

    #[test]
    fn at_risk_adds_the_badge_dot() {
        assert!(svg(state(70, UsageStatus::Warning, true, false)).contains("<circle"));
    }
}
