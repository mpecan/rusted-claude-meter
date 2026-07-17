//! Circular (donut) gauge SVG template.
//!
//! A stroked ring track plus a progress arc drawn with `stroke-dasharray`,
//! rotated so it starts at 12 o'clock like `ClaudeMeter`'s `CircularGaugeIcon`
//! — no trigonometry needed since the arc is always a full circle traced
//! partway round.

use std::fmt::Write as _;

use crate::palette::{ink, risk_badge};
use crate::state::IconState;
use crate::svg::svg_document;

const CENTER: f64 = 11.0;
const RADIUS: f64 = 8.0;
const STROKE: f64 = 3.0;
/// `2 * PI * RADIUS`, precomputed so the template has no runtime trig.
const CIRCUMFERENCE: f64 = 2.0 * std::f64::consts::PI * RADIUS;
/// The thinnest visible progress arc, so 1–3% does not round away to nothing.
const MIN_ARC: f64 = 1.5;

pub fn svg(state: IconState) -> String {
    let ink = ink(state.mono, state.status);

    svg_document(640, |out| {
        // Background track, faint so it reads at 22px without competing with
        // the progress arc.
        let _ = write!(
            out,
            r#"<circle cx="{CENTER}" cy="{CENTER}" r="{RADIUS}" fill="none" stroke="{ink}" stroke-opacity="0.25" stroke-width="{STROKE}"/>"#
        );
        if state.percent > 0 {
            let filled = (CIRCUMFERENCE * f64::from(state.percent) / 100.0).max(MIN_ARC);
            let remainder = (CIRCUMFERENCE - filled).max(0.0);
            let _ = write!(
                out,
                r#"<circle cx="{CENTER}" cy="{CENTER}" r="{RADIUS}" fill="none" stroke="{ink}" stroke-width="{STROKE}" stroke-linecap="round" stroke-dasharray="{filled:.2} {remainder:.2}" transform="rotate(-90 {CENTER} {CENTER})"/>"#
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
