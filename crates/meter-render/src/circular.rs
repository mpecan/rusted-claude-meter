//! Circular (donut) gauge icon: a gray ring track, a status-coloured progress
//! arc drawn with `stroke-dasharray` (rotated to start at 12 o'clock), and the
//! percentage number baked into the centre — mirroring `ClaudeMeter`'s
//! `CircularGaugeIcon`. Monochrome/template mode draws every part in black.

use std::fmt::Write as _;

use crate::font::centered_text;
use crate::palette::{GRAY, MONO, ink, risk_badge};
use crate::state::IconState;
use crate::svg::svg_document;

const CENTER_X: f64 = 13.0;
const CENTER_Y: f64 = 11.0;
const RADIUS: f64 = 8.0;
const STROKE: f64 = 3.0;
/// `2 * PI * RADIUS`, precomputed so the template has no runtime trig.
const CIRCUMFERENCE: f64 = 2.0 * std::f64::consts::PI * RADIUS;
/// The thinnest visible progress arc, so 1–3% does not round away to nothing.
const MIN_ARC: f64 = 1.5;
/// The centre number is small so it fits inside the ring.
const NUMBER_FS: f64 = 7.0;

pub fn svg(state: IconState) -> String {
    let (width, height) = state.style.logical_size();
    let canvas_w = f64::from(width);
    let track = if state.mono { MONO } else { GRAY };
    let arc_ink = ink(state.mono, state.status);

    svg_document(width, height, 768, |out| {
        // Background track.
        let _ = write!(
            out,
            r#"<circle cx="{CENTER_X}" cy="{CENTER_Y}" r="{RADIUS}" fill="none" stroke="{track}" stroke-opacity="0.3" stroke-width="{STROKE}"/>"#
        );
        if state.percent > 0 {
            let filled = (CIRCUMFERENCE * f64::from(state.percent) / 100.0).max(MIN_ARC);
            let remainder = (CIRCUMFERENCE - filled).max(0.0);
            let _ = write!(
                out,
                r#"<circle cx="{CENTER_X}" cy="{CENTER_Y}" r="{RADIUS}" fill="none" stroke="{arc_ink}" stroke-width="{STROKE}" stroke-linecap="round" stroke-dasharray="{filled:.2} {remainder:.2}" transform="rotate(-90 {CENTER_X} {CENTER_Y})"/>"#
            );
        }
        // The percentage number in the centre (no `%`, like the reference).
        let label = state.percent.to_string();
        centered_text(out, (CENTER_X, CENTER_Y), NUMBER_FS, arc_ink, &label);

        out.push_str(&risk_badge(state.at_risk, state.mono, canvas_w));
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
            style: IconStyle::Circular,
            percent,
            secondary_percent: 0,
            status,
            at_risk,
            mono,
            scale: Scale::X1,
        }
    }

    #[test]
    fn draws_the_centre_number() {
        let svg = svg(state(65, UsageStatus::Warning, false, false));
        assert!(svg.contains("<text"));
        assert!(
            svg.contains(">65<"),
            "centre number, no percent sign: {svg}"
        );
    }

    #[test]
    fn empty_ring_has_no_progress_arc() {
        let svg = svg(state(0, UsageStatus::Safe, false, false));
        assert_eq!(svg.matches("<circle").count(), 1, "background track only");
    }

    #[test]
    fn full_ring_draws_the_whole_circumference() {
        let svg = svg(state(100, UsageStatus::Critical, false, false));
        assert!(svg.contains(&format!("{CIRCUMFERENCE:.2} 0.00")));
    }

    #[test]
    fn colours_follow_status_and_mono_wins() {
        assert!(svg(state(10, UsageStatus::Safe, false, false)).contains(SAFE));
        assert!(svg(state(90, UsageStatus::Critical, false, false)).contains(CRITICAL));
        let mono = svg(state(90, UsageStatus::Critical, false, true));
        assert!(!mono.contains(CRITICAL));
        assert!(!mono.contains(GRAY));
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
