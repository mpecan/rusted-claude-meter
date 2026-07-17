//! Dual Bar SVG template: the 5-hour headline (top) and 7-day window
//! (bottom) shown as two stacked fill bars simultaneously, mirroring
//! `ClaudeMeter`'s `DualBarIcon`. The bottom bar always uses the accent
//! colour in colour mode (matching the original's violet weekly bar) so the
//! two rows read as distinct series even when both are calm.

use std::fmt::Write as _;

use crate::palette::{ACCENT, ink, proportional_fill, risk_badge};
use crate::state::IconState;

const BAR_X: f64 = 2.0;
const BAR_WIDTH: f64 = 18.0;
const BAR_HEIGHT: f64 = 5.0;
const TOP_Y: f64 = 5.0;
const GAP: f64 = 2.0;
const BOTTOM_Y: f64 = TOP_Y + BAR_HEIGHT + GAP;
/// The thinnest visible sliver, so 1–3% does not round away to nothing.
const MIN_FILL_WIDTH: f64 = 1.0;

pub fn svg(state: IconState) -> String {
    let primary_ink = ink(state.mono, state.status);
    let secondary_ink = if state.mono { primary_ink } else { ACCENT };

    let mut out = String::with_capacity(896);
    out.push_str(r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 22 22">"#);
    write_bar(&mut out, TOP_Y, state.percent, primary_ink);
    write_bar(&mut out, BOTTOM_Y, state.secondary_percent, secondary_ink);
    out.push_str(&risk_badge(state.at_risk, state.mono));
    out.push_str("</svg>");
    out
}

fn write_bar(out: &mut String, y: f64, percent: u8, fill: &str) {
    let _ = write!(
        out,
        r#"<rect x="{BAR_X}" y="{y}" width="{BAR_WIDTH}" height="{BAR_HEIGHT}" rx="1.5" fill="{fill}" fill-opacity="0.2"/>"#
    );
    if percent > 0 {
        let width = proportional_fill(BAR_WIDTH, MIN_FILL_WIDTH, percent);
        let _ = write!(
            out,
            r#"<rect x="{BAR_X}" y="{y}" width="{width:.2}" height="{BAR_HEIGHT}" rx="1.5" fill="{fill}"/>"#
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
            mono,
            scale: Scale::X1,
        }
    }

    #[test]
    fn both_bars_are_tracks_only_when_both_windows_are_empty() {
        let svg = svg(state(0, 0, UsageStatus::Safe, false, false));
        assert_eq!(svg.matches("<rect").count(), 2, "two tracks, no fills");
    }

    #[test]
    fn each_bar_fills_proportionally_and_independently() {
        let svg = svg(state(50, 25, UsageStatus::Warning, false, false));
        assert!(svg.contains(r#"width="9.00""#), "top 50% of 18: {svg}");
        assert!(svg.contains(r#"width="4.50""#), "bottom 25% of 18: {svg}");
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
        assert!(svg.contains(MONO));
    }

    #[test]
    fn at_risk_adds_the_badge_dot() {
        assert!(svg(state(30, 30, UsageStatus::Safe, true, false)).contains("<circle"));
    }
}
