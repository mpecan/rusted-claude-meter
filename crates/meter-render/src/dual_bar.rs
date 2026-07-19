//! Dual-bar icon: the 5-hour session window (top, status colour) and the
//! 7-day weekly window (bottom, violet accent) shown as two stacked
//! proportional bars, followed by the session percentage number — mirroring
//! `ClaudeMeter`'s `DualBarIcon`. The weekly bar always uses the accent colour
//! so the two rows read as distinct series. Monochrome collapses everything to
//! pure black.

use std::fmt::Write as _;

use crate::palette::{
    ACCENT, GRAY, MONO, badge, draw_label, ink, primary_label, proportional_fill,
};
use crate::state::IconState;
use crate::svg::svg_document;

const BAR_X: f64 = 3.0;
const BAR_WIDTH: f64 = 32.0;
const BAR_HEIGHT: f64 = 7.0;
const TOP_Y: f64 = 3.0;
const GAP: f64 = 2.0;
const BOTTOM_Y: f64 = TOP_Y + BAR_HEIGHT + GAP;
/// The thinnest visible sliver, so 1–3% does not round away to nothing.
const MIN_FILL_WIDTH: f64 = 1.5;
const NUMBER_CX: f64 = 53.0;
const NUMBER_CY: f64 = 11.0;
const NUMBER_FS: f64 = 13.0;

pub fn svg(state: IconState) -> String {
    let (width, height) = state.style.logical_size();
    let canvas_w = f64::from(width);
    let primary_ink = ink(state.mono, state.status);
    let secondary_ink = if state.mono { MONO } else { ACCENT };
    let track = if state.mono { MONO } else { GRAY };

    svg_document(width, height, 1024, |out| {
        write_bar(out, TOP_Y, state.percent, primary_ink, track);
        write_bar(out, BOTTOM_Y, state.secondary_percent, secondary_ink, track);

        // Session number (the primary metric), status-coloured — or the pace
        // ratio in pace band colour in pace-first display. The bars keep their
        // own session/weekly colours regardless.
        let label = primary_label(state);
        draw_label(out, (NUMBER_CX, NUMBER_CY), NUMBER_FS, state, &label);

        badge(out, state, canvas_w);
    })
}

fn write_bar(out: &mut String, y: f64, percent: u8, fill: &str, track: &str) {
    let _ = write!(
        out,
        r#"<rect x="{BAR_X}" y="{y}" width="{BAR_WIDTH}" height="{BAR_HEIGHT}" rx="1.5" fill="{track}" fill-opacity="0.3"/>"#
    );
    if percent > 0 {
        let bar_w = proportional_fill(BAR_WIDTH, MIN_FILL_WIDTH, percent);
        let _ = write!(
            out,
            r#"<rect x="{BAR_X}" y="{y}" width="{bar_w:.2}" height="{BAR_HEIGHT}" rx="1.5" fill="{fill}"/>"#
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::palette::{CRITICAL, MONO, SAFE};
    use crate::state::{IconStyle, Scale};
    use meter_core::UsageStatus;

    fn state(
        percent: u8,
        secondary_percent: u8,
        status: UsageStatus,
        at_risk: bool,
        mono: bool,
    ) -> IconState {
        IconState {
            style: IconStyle::DualBar,
            percent,
            secondary_percent,
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
    fn draws_the_session_percentage_number() {
        let svg = svg(state(65, 40, UsageStatus::Warning, false, false));
        assert!(svg.contains("<text"));
        assert!(svg.contains(">65%<"), "session number: {svg}");
    }

    #[test]
    fn both_bars_are_tracks_only_when_both_windows_are_empty() {
        let svg = svg(state(0, 0, UsageStatus::Safe, false, false));
        // Two gray tracks, no fills; the number is a <text>, not a <rect>.
        assert_eq!(svg.matches("<rect").count(), 2, "two tracks, no fills");
    }

    #[test]
    fn each_bar_fills_proportionally_and_independently() {
        let svg = svg(state(50, 25, UsageStatus::Warning, false, false));
        assert!(svg.contains(r#"width="16.00""#), "top 50% of 32: {svg}");
        assert!(svg.contains(r#"width="8.00""#), "bottom 25% of 32: {svg}");
    }

    #[test]
    fn bottom_bar_uses_the_accent_colour_in_colour_mode() {
        let svg = svg(state(10, 90, UsageStatus::Critical, false, false));
        assert!(svg.contains(CRITICAL));
        assert!(svg.contains(ACCENT));
    }

    #[test]
    fn mono_collapses_both_bars_to_pure_ink() {
        let svg = svg(state(10, 90, UsageStatus::Critical, false, true));
        assert!(!svg.contains(CRITICAL));
        assert!(!svg.contains(ACCENT));
        assert!(!svg.contains(SAFE));
        assert!(!svg.contains(GRAY));
        assert!(svg.contains(MONO));
    }

    #[test]
    fn at_risk_adds_the_badge_dot() {
        assert!(svg(state(30, 30, UsageStatus::Safe, true, false)).contains("<circle"));
    }
}
