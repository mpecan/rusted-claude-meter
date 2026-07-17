//! Minimal icon: just the big monospaced percentage number in the status
//! colour (black in monochrome/template mode) — no track, no bar, nothing
//! else. Mirrors `ClaudeMeter`'s `MinimalIcon`, which is a single semibold
//! monospaced `Text("N%")`.

use crate::font::centered_text;
use crate::palette::{ink, risk_badge};
use crate::state::IconState;
use crate::svg::svg_document;

const NUMBER_CY: f64 = 11.0;
const NUMBER_FS: f64 = 14.0;

pub fn svg(state: IconState) -> String {
    let (width, height) = state.style.logical_size();
    let canvas_w = f64::from(width);
    let number_ink = ink(state.mono, state.status);

    svg_document(width, height, 384, |out| {
        let label = format!("{}%", state.percent);
        centered_text(
            out,
            (canvas_w / 2.0, NUMBER_CY),
            NUMBER_FS,
            number_ink,
            &label,
        );
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
    fn is_just_the_number_no_bars() {
        let svg = svg(state(65, UsageStatus::Warning, false, false));
        assert!(svg.contains("<text"), "minimal is a single text element");
        assert!(svg.contains(">65%<"));
        assert!(!svg.contains("<rect"), "no track or bar");
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
