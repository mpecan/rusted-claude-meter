//! Gauge SVG template: a semicircular radial dial with a needle, standing in
//! for `ClaudeMeter`'s SF Symbol `gauge.with.dots.needle.*percent` glyphs
//! (not portable to a headless rasterizer with no guaranteed font/symbol
//! set). The track and progress arc reuse the same full-circle
//! `stroke-dasharray` trick as [`crate::circular`], rotated 180° so the dash
//! starts at 9 o'clock and sweeps clockwise through 12 to 3 o'clock — i.e.
//! exactly the top half. The needle is the one piece of real trigonometry in
//! this crate: it points at the same angle the arc sweeps to.
use std::fmt::Write as _;

use crate::palette::{ink, risk_badge};
use crate::state::IconState;
use crate::svg::svg_document;

const CENTER_X: f64 = 13.0;
const CENTER_Y: f64 = 15.0;
const RADIUS: f64 = 7.0;
const NEEDLE_LENGTH: f64 = 6.0;
const HUB_RADIUS: f64 = 1.3;
const STROKE: f64 = 2.5;
const CIRCUMFERENCE: f64 = 2.0 * std::f64::consts::PI * RADIUS;
const HALF_CIRCUMFERENCE: f64 = std::f64::consts::PI * RADIUS;
/// The thinnest visible progress arc, so 1–3% does not round away to nothing.
const MIN_ARC: f64 = 1.2;

pub fn svg(state: IconState) -> String {
    let (width, height) = state.style.logical_size();
    let canvas_w = f64::from(width);
    let ink = ink(state.mono, state.status);

    svg_document(width, height, 768, |out| {
        // Background track: the top half of the circle, faint.
        let _ = write!(
            out,
            r#"<circle cx="{CENTER_X}" cy="{CENTER_Y}" r="{RADIUS}" fill="none" stroke="{ink}" stroke-opacity="0.25" stroke-width="{STROKE}" stroke-dasharray="{HALF_CIRCUMFERENCE:.2} {HALF_CIRCUMFERENCE:.2}" transform="rotate(-180 {CENTER_X} {CENTER_Y})"/>"#
        );
        if state.percent > 0 {
            let filled = (HALF_CIRCUMFERENCE * f64::from(state.percent) / 100.0).max(MIN_ARC);
            let remainder = (CIRCUMFERENCE - filled).max(0.0);
            let _ = write!(
                out,
                r#"<circle cx="{CENTER_X}" cy="{CENTER_Y}" r="{RADIUS}" fill="none" stroke="{ink}" stroke-width="{STROKE}" stroke-linecap="round" stroke-dasharray="{filled:.2} {remainder:.2}" transform="rotate(-180 {CENTER_X} {CENTER_Y})"/>"#
            );
        }
        let (needle_x, needle_y) = needle_tip(state.percent);
        let _ = write!(
            out,
            r#"<line x1="{CENTER_X}" y1="{CENTER_Y}" x2="{needle_x:.2}" y2="{needle_y:.2}" stroke="{ink}" stroke-width="1.4" stroke-linecap="round"/><circle cx="{CENTER_X}" cy="{CENTER_Y}" r="{HUB_RADIUS}" fill="{ink}"/>"#
        );
        out.push_str(&risk_badge(state.at_risk, state.mono, canvas_w));
    })
}

/// The needle tip for `percent`: 0% points at 9 o'clock, 100% at 3 o'clock,
/// sweeping clockwise through 12 o'clock in between — the same direction the
/// dash-array arc above traces.
fn needle_tip(percent: u8) -> (f64, f64) {
    let theta = (f64::from(percent) / 100.0)
        .mul_add(-180.0, 180.0)
        .to_radians();
    let x = NEEDLE_LENGTH.mul_add(theta.cos(), CENTER_X);
    let y = NEEDLE_LENGTH.mul_add(-theta.sin(), CENTER_Y);
    (x, y)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::palette::{CRITICAL, MONO, SAFE};
    use crate::state::{IconStyle, Scale};
    use meter_core::UsageStatus;

    fn state(percent: u8, status: UsageStatus, at_risk: bool, mono: bool) -> IconState {
        IconState {
            style: IconStyle::Gauge,
            percent,
            secondary_percent: 0,
            status,
            at_risk,
            mono,
            scale: Scale::X1,
        }
    }

    #[test]
    fn needle_sweeps_left_to_top_to_right() {
        let (x0, y0) = needle_tip(0);
        assert!(
            (x0 - (CENTER_X - NEEDLE_LENGTH)).abs() < 1e-9,
            "0% points left: {x0}"
        );
        assert!((y0 - CENTER_Y).abs() < 1e-9);

        let (x50, y50) = needle_tip(50);
        assert!(
            (x50 - CENTER_X).abs() < 1e-9,
            "50% points straight up: {x50}"
        );
        assert!((y50 - (CENTER_Y - NEEDLE_LENGTH)).abs() < 1e-9);

        let (x100, y100) = needle_tip(100);
        assert!(
            (x100 - (CENTER_X + NEEDLE_LENGTH)).abs() < 1e-9,
            "100% points right: {x100}"
        );
        assert!((y100 - CENTER_Y).abs() < 1e-9);
    }

    #[test]
    fn zero_percent_has_no_progress_arc() {
        let svg = svg(state(0, UsageStatus::Safe, false, false));
        // Track + needle line + hub = 3 shapes; no second (progress) circle.
        assert_eq!(svg.matches("<circle").count(), 2);
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
        let plain = svg(state(70, UsageStatus::Warning, false, false));
        let flagged = svg(state(70, UsageStatus::Warning, true, false));
        assert_eq!(
            plain.matches("<circle").count() + 1,
            flagged.matches("<circle").count()
        );
    }
}
