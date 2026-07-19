//! Segmented-bar icon: a signal-strength-style row of five quantized blocks,
//! mirroring `ClaudeMeter`'s `SegmentedBarIcon`. Each segment covers a
//! 20-point band and lights once `percent` reaches its band's floor. The
//! reference shows no number for this style, so neither do we. Lit segments
//! shade green → orange → red by position; unlit ones are a faint gray track.

use std::fmt::Write as _;

use meter_core::UsageStatus;

use crate::palette::{GRAY, MONO, badge, ink, override_color, status_color};
use crate::state::IconState;
use crate::svg::svg_document;

const COUNT: u32 = 5;
const SEG_WIDTH: f64 = 4.0;
const GAP: f64 = 2.0;
const START_X: f64 = 3.0;
const BASE_SEG_HEIGHT: f64 = 8.0;
const HEIGHT_STEP: f64 = 2.4;
/// Segments sit on this baseline, growing upward.
const BASELINE: f64 = 19.5;

pub fn svg(state: IconState) -> String {
    let (width, height) = state.style.logical_size();
    let canvas_w = f64::from(width);
    let ink = ink(state.mono, state.status);
    let track = if state.mono { MONO } else { GRAY };
    // Pace-first display recolours every lit segment with the single pace band
    // colour instead of the positional green→orange→red gradient.
    let pace_fill = override_color(state);
    // How many of the five segments are lit — matching the original's
    // `percentage >= index * 20` quantization exactly, which always lights the
    // first segment (`percentage >= 0` is trivially true), even at 0%.
    let lit = (1 + u32::from(state.percent) / 20).min(COUNT);

    svg_document(width, height, 768, |out| {
        for index in 0..COUNT {
            let x = f64::from(index).mul_add(SEG_WIDTH + GAP, START_X);
            let seg_height = HEIGHT_STEP.mul_add(f64::from(index), BASE_SEG_HEIGHT);
            let y = BASELINE - seg_height;
            if index < lit {
                let fill = pace_fill.unwrap_or_else(|| segment_color(state.mono, index, ink));
                let _ = write!(
                    out,
                    r#"<rect x="{x:.2}" y="{y:.2}" width="{SEG_WIDTH}" height="{seg_height:.2}" rx="1" fill="{fill}"/>"#
                );
            } else {
                let _ = write!(
                    out,
                    r#"<rect x="{x:.2}" y="{y:.2}" width="{SEG_WIDTH}" height="{seg_height:.2}" rx="1" fill="{track}" fill-opacity="0.3"/>"#
                );
            }
        }
        badge(out, state, canvas_w);
    })
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
            pace_kind: None,
            pace_band: None,
            pace_ratio: None,
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
    fn always_renders_five_segments_and_no_number() {
        let svg = svg(state(37, UsageStatus::Safe, false, false));
        assert_eq!(svg.matches("<rect").count(), 5);
        assert!(!svg.contains("<text"), "reference segments has no number");
    }

    #[test]
    fn mono_wins_over_the_position_gradient() {
        let mono = svg(state(100, UsageStatus::Critical, false, true));
        assert!(!mono.contains(crate::palette::SAFE));
        assert!(!mono.contains(GRAY));
        assert!(mono.contains(MONO));
    }

    #[test]
    fn at_risk_adds_the_badge_dot() {
        assert!(svg(state(70, UsageStatus::Warning, true, false)).contains("<circle"));
    }

    #[test]
    fn pace_first_paints_active_segments_one_pace_colour_and_a_snowflake() {
        use crate::palette::{BLUE, SAFE};
        // Underuse (cold): every active segment turns blue, replacing the
        // positional green→orange→red gradient, and a snowflake badge appears.
        let s = state(65, UsageStatus::Warning, false, false)
            .with_pace(Some(0.3), Some(meter_core::PaceKind::Cold));
        let svg = svg(s);
        assert!(
            svg.contains(BLUE),
            "active segments are the pace band colour"
        );
        assert!(
            !svg.contains(SAFE),
            "the positional gradient is gone in pace-first: {svg}"
        );
        assert!(
            svg.contains(r#"fill="none""#),
            "snowflake is a stroked path"
        );
    }
}
