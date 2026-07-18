//! Circular (donut) gauge icon: a gray ring track, a status-coloured progress
//! arc drawn with `stroke-dasharray` (rotated to start at 12 o'clock), and the
//! percentage number baked into the centre — mirroring `ClaudeMeter`'s
//! `CircularGaugeIcon`. Monochrome/template mode draws every part in black.

use std::fmt::Write as _;

use crate::palette::{GRAY, MONO, badge, draw_label, ink, primary_label};
use crate::state::IconState;
use crate::svg::svg_document;

const CENTER_X: f64 = 13.0;
const CENTER_Y: f64 = 11.0;
const RADIUS: f64 = 9.0;
const STROKE: f64 = 3.5;
/// `2 * PI * RADIUS`, precomputed so the template has no runtime trig.
const CIRCUMFERENCE: f64 = 2.0 * std::f64::consts::PI * RADIUS;
/// The thinnest visible progress arc, so 1–3% does not round away to nothing.
const MIN_ARC: f64 = 1.5;
/// The centre number is small so it fits inside the ring.
const NUMBER_FS: f64 = 8.0;
/// A narrower size for the three-digit `100` case: at `NUMBER_FS` the monospaced
/// `100` (three 0.6em glyphs ≈ 12.6px) is wider than the ring's inner chord and
/// its outer strokes collide with the donut, so 3-digit labels shrink to clear
/// it. Two-digit labels keep the larger size.
const NUMBER_FS_WIDE: f64 = 6.5;

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
        // The centre number (no `%`, like the reference), or the compact pace
        // ratio in pace-first display. Long labels — `100`, or a pace ratio like
        // `12.5` — shrink so their outer strokes clear the surrounding ring; the
        // arc keeps the quota status colour while the number takes the pace band.
        let label = primary_label(state);
        let font_size = if label.len() > 2 {
            NUMBER_FS_WIDE
        } else {
            NUMBER_FS
        };
        draw_label(out, (CENTER_X, CENTER_Y), font_size, state, &label);

        badge(out, state, canvas_w);
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::palette::{BLUE, CRITICAL, MONO, SAFE};
    use crate::state::{IconStyle, Scale};
    use meter_core::{PaceKind, UsageStatus};

    fn state(percent: u8, status: UsageStatus, at_risk: bool, mono: bool) -> IconState {
        IconState {
            style: IconStyle::Circular,
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
    fn three_digit_label_shrinks_to_clear_the_ring() {
        // `100` is the only three-digit case; it must render at the narrower
        // size so its outer strokes do not collide with the surrounding ring,
        // while two-digit labels keep the larger size.
        let full = svg(state(100, UsageStatus::Critical, false, false));
        assert!(full.contains(">100<"));
        assert!(
            full.contains(&format!(r#"font-size="{NUMBER_FS_WIDE}""#)),
            "100 must use the narrow font size: {full}"
        );
        let two_digit = svg(state(92, UsageStatus::Critical, false, false));
        assert!(
            two_digit.contains(&format!(r#"font-size="{NUMBER_FS}""#)),
            "two-digit labels keep the larger size: {two_digit}"
        );
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
    fn pace_first_recolours_only_the_centre_number_not_the_arc() {
        // Circular is the one style whose pace-first split is asymmetric (per
        // CircularGaugeIcon.swift): the progress arc keeps the quota-status
        // colour while only the centre number takes the pace-band colour. A
        // Safe window pacing cold (underuse → blue) must leave the arc SAFE
        // green while the number turns blue.
        let cold =
            state(30, UsageStatus::Safe, false, false).with_pace(Some(0.3), Some(PaceKind::Cold));
        let svg = svg(cold);
        // The arc keeps the SAFE status colour — its stroke is the only SAFE
        // element, so if it wrongly took the pace band this assertion fails.
        assert!(
            svg.contains(&format!(r#"stroke="{SAFE}""#)),
            "arc must keep the quota-status colour, not the pace band: {svg}"
        );
        // The centre number is recoloured to the underuse band: its digits are
        // filled blue (the snowflake badge is *stroked* blue, fill="none", so a
        // blue `fill=` uniquely identifies the recoloured number).
        assert!(
            svg.contains(&format!(r#"fill="{BLUE}""#)),
            "centre number must take the pace-band colour: {svg}"
        );
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
