//! Battery-style icon: a gray track capsule whose fill width is proportional
//! to the percent, followed by the monospaced `N%` number — the reference
//! app's default style. In colour mode the fill is a green → yellow → orange →
//! red gradient (so the filled portion shows where the current level sits on
//! that scale) and the number is drawn in the status colour; in monochrome
//! (template) mode every part collapses to pure black.

use std::fmt::Write as _;

use crate::font::centered_text;
use crate::palette::{
    CRITICAL, GRAY, MONO, SAFE, WARNING, YELLOW, ink, proportional_fill, risk_badge,
};
use crate::state::IconState;
use crate::svg::svg_document;

const TRACK_X: f64 = 3.0;
const TRACK_Y: f64 = 6.0;
const TRACK_W: f64 = 27.0;
const TRACK_H: f64 = 10.0;
const TRACK_RX: f64 = 5.0;
/// The thinnest visible charge sliver, so 1–3% does not round to nothing.
const FILL_MIN_WIDTH: f64 = 1.5;
const NUMBER_CX: f64 = 47.0;
const NUMBER_CY: f64 = 11.0;
const NUMBER_FS: f64 = 11.0;

pub fn svg(state: IconState) -> String {
    let (width, height) = state.style.logical_size();
    let canvas_w = f64::from(width);
    let track_gray = if state.mono { MONO } else { GRAY };
    let number_ink = ink(state.mono, state.status);

    svg_document(width, height, 1024, |out| {
        // Clip so the proportional fill keeps the capsule's rounded ends.
        let _ = write!(
            out,
            r#"<defs><clipPath id="bc"><rect x="{TRACK_X}" y="{TRACK_Y}" width="{TRACK_W}" height="{TRACK_H}" rx="{TRACK_RX}"/></clipPath>"#
        );
        if !state.mono {
            let _ = write!(
                out,
                r#"<linearGradient id="bg" gradientUnits="userSpaceOnUse" x1="{TRACK_X}" y1="0" x2="{right}" y2="0"><stop offset="0" stop-color="{SAFE}"/><stop offset="0.33" stop-color="{YELLOW}"/><stop offset="0.67" stop-color="{WARNING}"/><stop offset="1" stop-color="{CRITICAL}"/></linearGradient>"#,
                right = TRACK_X + TRACK_W,
            );
        }
        out.push_str("</defs>");

        // Gray track capsule.
        let _ = write!(
            out,
            r#"<rect x="{TRACK_X}" y="{TRACK_Y}" width="{TRACK_W}" height="{TRACK_H}" rx="{TRACK_RX}" fill="{track_gray}" fill-opacity="0.3"/>"#
        );

        // Proportional charge fill (gradient in colour, solid black in mono).
        if state.percent > 0 {
            let fill_w = proportional_fill(TRACK_W, FILL_MIN_WIDTH, state.percent);
            let fill: &str = if state.mono { MONO } else { "url(#bg)" };
            let _ = write!(
                out,
                r#"<rect x="{TRACK_X}" y="{TRACK_Y}" width="{fill_w:.2}" height="{TRACK_H}" clip-path="url(#bc)" fill="{fill}"/>"#
            );
        }

        // The percentage number, in the status colour (black in mono).
        let label = format!("{}%", state.percent);
        centered_text(out, (NUMBER_CX, NUMBER_CY), NUMBER_FS, number_ink, &label);

        risk_badge(out, state.at_risk, state.mono, canvas_w);
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
    fn draws_the_percentage_number() {
        let svg = svg(state(42, UsageStatus::Warning, false, false));
        assert!(svg.contains("<text"), "battery must bake in the number");
        assert!(svg.contains(">42%<"), "number text: {svg}");
    }

    #[test]
    fn empty_battery_has_no_fill_rect_but_still_shows_zero() {
        let svg = svg(state(0, UsageStatus::Safe, false, false));
        assert!(
            !svg.contains(r#"clip-path="url(#bc)""#),
            "no charge fill at 0%"
        );
        assert!(svg.contains(">0%<"));
    }

    #[test]
    fn colour_fill_is_a_gradient_and_number_follows_status() {
        let svg = svg(state(60, UsageStatus::Warning, false, false));
        assert!(
            svg.contains("linearGradient"),
            "gradient fill in colour mode"
        );
        assert!(svg.contains("url(#bg)"));
        assert!(svg.contains(WARNING), "number in the status colour");
        assert!(
            svg.contains(SAFE) && svg.contains(CRITICAL),
            "gradient stops"
        );
    }

    #[test]
    fn fill_width_is_proportional_and_floored() {
        let half = svg(state(50, UsageStatus::Warning, false, false));
        assert!(half.contains(r#"width="13.50""#), "50% of 27: {half}");
        let sliver = svg(state(2, UsageStatus::Safe, false, false));
        assert!(sliver.contains(r#"width="1.50""#), "min sliver: {sliver}");
    }

    #[test]
    fn mono_is_pure_black_with_no_gradient() {
        let mono = svg(state(90, UsageStatus::Critical, false, true));
        assert!(
            !mono.contains("linearGradient"),
            "no gradient in template mode"
        );
        assert!(!mono.contains(CRITICAL));
        assert!(!mono.contains(GRAY));
        assert!(mono.contains(MONO));
    }

    #[test]
    fn at_risk_adds_the_badge_dot() {
        assert!(svg(state(70, UsageStatus::Warning, true, false)).contains("<circle"));
    }
}
