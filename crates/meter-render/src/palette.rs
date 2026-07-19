//! Shared status colour palette and small SVG fragments reused by every
//! icon style, so the six templates in this crate agree on what "safe",
//! "warning", "critical" and "monochrome" mean.

use std::fmt::Write as _;

use meter_core::{PaceBand, PaceKind, UsageStatus};

use crate::font::{centered_text, pace_label};
use crate::state::{IconState, IconStyle};

/// Apple system green / orange / red, matching the original `ClaudeMeter`.
pub const SAFE: &str = "#34C759";
pub const WARNING: &str = "#FF9500";
pub const CRITICAL: &str = "#FF3B30";
/// Apple system blue — the underuse (idle) end of the pace scale. The pace
/// palette's other three bands reuse [`SAFE`]/[`WARNING`]/[`CRITICAL`], exactly
/// like `PacePalette.swift`'s `.blue`/`.green`/`.orange`/`.red`.
pub const BLUE: &str = "#007AFF";
/// Apple system yellow — the mid-scale stop of the Battery fill gradient.
pub const YELLOW: &str = "#FFCC00";
/// Monochrome / template artwork is alpha-only black.
pub const MONO: &str = "#000000";
/// Apple system gray, used (at low opacity) for the unfilled track/background
/// of the bar-style icons, matching the reference's `Color.gray.opacity(...)`.
pub const GRAY: &str = "#8E8E93";
/// Secondary-series accent (the 7-day bar in Dual Bar), matching
/// `ClaudeMeter`'s violet weekly bar. Color mode only — mono ignores it.
pub const ACCENT: &str = "#AF52DE";

pub const fn status_color(status: UsageStatus) -> &'static str {
    match status {
        UsageStatus::Safe => SAFE,
        UsageStatus::Warning => WARNING,
        UsageStatus::Critical => CRITICAL,
    }
}

/// The primary ink colour for a style: pure black in monochrome/template
/// mode, otherwise the status colour.
pub const fn ink(mono: bool, status: UsageStatus) -> &'static str {
    if mono { MONO } else { status_color(status) }
}

/// A fill width proportional to `percent` of `max_width`, floored to
/// `min_width` so a nonzero-but-tiny percentage still renders a visible
/// sliver instead of rounding away to nothing. Shared by every style that
/// draws a proportional bar (battery, minimal, dual bar).
pub fn proportional_fill(max_width: f64, min_width: f64, percent: u8) -> f64 {
    (max_width * f64::from(percent) / 100.0).max(min_width)
}

/// The colour for a pace band: blue underuse, green sustainable, orange
/// overuse, red heavy overuse — the shared pace scale from `PacePalette.swift`.
pub const fn pace_band_color(band: PaceBand) -> &'static str {
    match band {
        PaceBand::Underuse => BLUE,
        PaceBand::Sustainable => SAFE,
        PaceBand::Overuse => WARNING,
        PaceBand::HeavyOveruse => CRITICAL,
    }
}

// --- pace-first override (shared across every style) -----------------------
//
// In pace-first display the primary metric becomes the pace ratio: the number
// text is replaced by the ratio and recoloured by the pace band, and the
// at-risk dot is replaced by a flame/snowflake. The decision logic lives here
// once; the six style modules only choose *which* element it applies to.

/// The primary label a style prints: the pace ratio ("1.8×", or compact "1.8"
/// for the space-starved circular gauge) in pace-first display, else the quota
/// percentage — with a `%` for every style except the circular gauge, which
/// prints the bare number like `ClaudeMeter`'s `CircularGaugeIcon`.
pub fn primary_label(state: IconState) -> String {
    if let Some(ratio) = state.pace_ratio {
        let number = format!("{ratio:.1}");
        return if state.style == IconStyle::Circular {
            number
        } else {
            format!("{number}\u{00D7}")
        };
    }
    match state.style {
        IconStyle::Circular => state.percent.to_string(),
        _ => format!("{}%", state.percent),
    }
}

/// The ink for the primary label / overridable glyph: pure black in
/// monochrome/template mode, else the pace-band colour in pace-first display,
/// else the quota status colour.
pub const fn label_ink(state: IconState) -> &'static str {
    match state.pace_band {
        Some(band) if !state.mono => pace_band_color(band),
        _ => ink(state.mono, state.status),
    }
}

/// The single pace colour that overrides a style's own multi-hue fill (the
/// segmented bar's positional gradient), or `None` when not in pace-first
/// display. Black in monochrome so template artwork stays pure ink.
pub fn override_color(state: IconState) -> Option<&'static str> {
    state.pace_band.map(|band| {
        if state.mono {
            MONO
        } else {
            pace_band_color(band)
        }
    })
}

/// Draw a style's primary `label` centered on `center`: the ordinary
/// monospaced text in quota mode, or the vector-composed pace label (which can
/// render the `.`/`×` the subset font lacks) in pace-first display. Colour is
/// resolved by [`label_ink`].
pub fn draw_label(
    out: &mut String,
    center: (f64, f64),
    font_size: f64,
    state: IconState,
    label: &str,
) {
    let ink = label_ink(state);
    if state.pace_ratio.is_some() {
        pace_label(out, center, font_size, ink, label);
    } else {
        centered_text(out, center, font_size, ink, label);
    }
}

// --- top-right corner badge ------------------------------------------------

/// Draw the top-right corner badge for `state`: the flame (hot) / snowflake
/// (cold) pace glyph when a pace kind is set, otherwise the plain at-risk dot.
/// Shared geometry across every style so the six templates agree on placement.
pub fn badge(out: &mut String, state: IconState, canvas_width: f64) {
    match state.pace_kind {
        Some(kind) => pace_badge(out, kind, badge_color(state, kind), canvas_width),
        None => risk_badge(out, state.at_risk, state.mono, canvas_width),
    }
}

/// The flame/snowflake colour: pure black in mono, else the pace-band colour
/// (so a heavy overuse reads red, matching the popover), falling back to a
/// fixed hot-orange / cold-blue when no band is available.
fn badge_color(state: IconState, kind: PaceKind) -> &'static str {
    if state.mono {
        return MONO;
    }
    state.pace_band.map_or(
        match kind {
            PaceKind::Hot => WARNING,
            PaceKind::Cold => BLUE,
        },
        pace_band_color,
    )
}

/// The pacing at-risk badge dot: a small filled circle tucked into the very
/// top-right corner of the (style-specific width) canvas, clear of the main
/// artwork. A no-op when `at_risk` is false.
pub fn risk_badge(out: &mut String, at_risk: bool, mono: bool, canvas_width: f64) {
    if !at_risk {
        return;
    }
    let badge = if mono { MONO } else { CRITICAL };
    let cx = canvas_width - 3.0;
    let _ = write!(
        out,
        r#"<circle cx="{cx:.2}" cy="3" r="2.2" fill="{badge}"/>"#
    );
}

/// Append the flame/snowflake pace glyph in the top-right corner.
fn pace_badge(out: &mut String, kind: PaceKind, color: &str, canvas_width: f64) {
    let (cx, cy) = (canvas_width - 4.0, 4.5);
    match kind {
        PaceKind::Hot => flame(out, cx, cy, color),
        PaceKind::Cold => snowflake(out, cx, cy, color),
    }
}

/// A small upward flame (a pointed-top teardrop), filled with `color`.
fn flame(out: &mut String, cx: f64, cy: f64, color: &str) {
    let _ = write!(
        out,
        r#"<path d="M{cx:.2},{top:.2}C{rx:.2},{cy:.2} {rx2:.2},{bot:.2} {cx:.2},{bot:.2}C{lx2:.2},{bot:.2} {lx:.2},{cy:.2} {cx:.2},{top:.2}Z" fill="{color}"/>"#,
        top = cy - 3.0,
        bot = cy + 2.6,
        rx = cx + 2.3,
        rx2 = cx + 2.0,
        lx2 = cx - 2.0,
        lx = cx - 2.3,
    );
}

/// A small six-spoke snowflake with branch ticks, stroked in `color`.
fn snowflake(out: &mut String, cx: f64, cy: f64, color: &str) {
    let reach: f64 = 2.8;
    let branch: f64 = 1.0;
    let mut d = String::new();
    for spoke in 0..6 {
        let angle = std::f64::consts::FRAC_PI_3 * f64::from(spoke);
        let (ux, uy) = (angle.cos(), angle.sin());
        let (tip_x, tip_y) = (reach.mul_add(ux, cx), reach.mul_add(uy, cy));
        let _ = write!(d, "M{cx:.2},{cy:.2}L{tip_x:.2},{tip_y:.2}");
        // Two branch ticks two-thirds of the way out, angled off the spoke.
        let (base_x, base_y) = (
            0.6_f64.mul_add(reach * ux, cx),
            0.6_f64.mul_add(reach * uy, cy),
        );
        for side in [1.0_f64, -1.0] {
            let ba = side.mul_add(std::f64::consts::FRAC_PI_3, angle);
            let (ex, ey) = (
                branch.mul_add(ba.cos(), base_x),
                branch.mul_add(ba.sin(), base_y),
            );
            let _ = write!(d, "M{base_x:.2},{base_y:.2}L{ex:.2},{ey:.2}");
        }
    }
    let _ = write!(
        out,
        r#"<path d="{d}" stroke="{color}" stroke-width="0.7" stroke-linecap="round" fill="none"/>"#
    );
}

#[cfg(test)]
mod tests {
    use meter_core::PaceBand;

    use super::*;
    use crate::state::{IconStyle, Scale};

    fn base(style: IconStyle, mono: bool) -> IconState {
        IconState {
            style,
            percent: 50,
            secondary_percent: 0,
            status: UsageStatus::Warning,
            at_risk: false,
            pace_kind: None,
            pace_band: None,
            pace_ratio: None,
            mono,
            scale: Scale::X1,
        }
    }

    #[test]
    fn ink_is_black_only_when_mono() {
        assert_eq!(ink(true, UsageStatus::Critical), MONO);
        assert_eq!(ink(false, UsageStatus::Critical), CRITICAL);
    }

    #[test]
    fn pace_bands_map_to_the_blue_green_orange_red_scale() {
        assert_eq!(pace_band_color(PaceBand::Underuse), BLUE);
        assert_eq!(pace_band_color(PaceBand::Sustainable), SAFE);
        assert_eq!(pace_band_color(PaceBand::Overuse), WARNING);
        assert_eq!(pace_band_color(PaceBand::HeavyOveruse), CRITICAL);
    }

    #[test]
    fn primary_label_is_quota_percent_without_pace_and_ratio_with_it() {
        assert_eq!(primary_label(base(IconStyle::Battery, false)), "50%");
        // Circular prints the bare number, like the reference.
        assert_eq!(primary_label(base(IconStyle::Circular, false)), "50");

        let paced = base(IconStyle::Battery, false).with_pace(Some(1.8), None);
        assert_eq!(primary_label(paced), "1.8\u{00D7}");
        // The circular gauge uses the compact form without the multiply sign.
        let paced_circular = base(IconStyle::Circular, false).with_pace(Some(1.8), None);
        assert_eq!(primary_label(paced_circular), "1.8");
    }

    #[test]
    fn label_ink_prefers_pace_band_then_status_and_mono_wins() {
        // Underuse pace → blue, overriding the warning status colour.
        let cold = base(IconStyle::Battery, false).with_pace(Some(0.3), None);
        assert_eq!(label_ink(cold), BLUE);
        // No pace → status colour.
        assert_eq!(label_ink(base(IconStyle::Battery, false)), WARNING);
        // Mono beats everything, pace or not.
        assert_eq!(
            label_ink(base(IconStyle::Battery, true).with_pace(Some(3.0), None)),
            MONO
        );
    }

    #[test]
    fn override_color_is_some_only_in_pace_first_and_black_in_mono() {
        assert!(override_color(base(IconStyle::Segments, false)).is_none());
        assert_eq!(
            override_color(base(IconStyle::Segments, false).with_pace(Some(3.0), None)),
            Some(CRITICAL)
        );
        assert_eq!(
            override_color(base(IconStyle::Segments, true).with_pace(Some(3.0), None)),
            Some(MONO)
        );
    }

    #[test]
    fn risk_badge_is_empty_unless_at_risk() {
        let badge = |at_risk, mono| {
            let mut out = String::new();
            risk_badge(&mut out, at_risk, mono, 63.0);
            out
        };
        assert_eq!(badge(false, false), "");
        assert!(badge(true, false).contains(CRITICAL));
        assert!(badge(true, true).contains(MONO));
        // The badge tracks the right edge of the (style-specific) canvas.
        assert!(badge(true, false).contains(r#"cx="60.00""#));
    }

    #[test]
    fn badge_draws_flame_when_hot_and_snowflake_when_cold() {
        let draw = |kind, ratio| {
            let mut out = String::new();
            let state = base(IconStyle::Battery, false).with_pace(Some(ratio), Some(kind));
            badge(&mut out, state, 63.0);
            out
        };
        // The flame is a filled teardrop path; the snowflake is a stroked path.
        let hot = draw(PaceKind::Hot, 3.0);
        assert!(hot.contains("<path"));
        assert!(hot.contains("fill="), "flame is filled");
        assert!(hot.contains(CRITICAL), "heavy overuse flame is red");

        let cold = draw(PaceKind::Cold, 0.3);
        assert!(cold.contains("stroke="), "snowflake is stroked");
        assert!(cold.contains(BLUE), "underuse snowflake is blue");
        assert!(cold.contains(r#"fill="none""#));
    }

    #[test]
    fn pace_badge_is_pure_black_in_mono() {
        let mut out = String::new();
        let state = base(IconStyle::Battery, true).with_pace(Some(3.0), Some(PaceKind::Hot));
        badge(&mut out, state, 63.0);
        assert!(out.contains(MONO));
        assert!(!out.contains(CRITICAL));
    }

    #[test]
    fn badge_falls_back_to_at_risk_dot_without_a_pace_kind() {
        let mut out = String::new();
        let mut state = base(IconStyle::Battery, false);
        state.at_risk = true;
        badge(&mut out, state, 63.0);
        assert!(out.contains("<circle"), "the plain at-risk dot");
    }
}
