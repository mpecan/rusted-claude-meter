use resvg::tiny_skia::{Pixmap, Transform};
use resvg::usvg::{Options, Tree};

use crate::state::{BASE_SIZE, IconState, IconStyle};
use crate::{battery, circular, dual_bar, gauge, minimal, segments};

/// Rendering failure.
///
/// Templates are generated in-crate, so in practice this only fires on a bug
/// (malformed template) — but the workspace denies panics, so it surfaces as
/// an error the shell can log and fall back from.
#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    #[error("icon SVG template failed to parse: {0}")]
    Template(String),
    #[error("could not allocate a {0}x{0} pixmap")]
    Pixmap(u32),
}

/// A rasterized icon: straight-alpha RGBA, row-major, `width * height * 4`
/// bytes — exactly what `tauri::image::Image::new_owned` expects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedIcon {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    /// True for the monochrome variant: on macOS the tray should mark this
    /// image as a template (`icon_as_template`) so the system recolours it
    /// to match the menu bar appearance.
    pub is_template: bool,
}

/// Render one icon state to RGBA. Pure: same state, same bytes.
pub fn render_icon(state: &IconState) -> Result<RenderedIcon, RenderError> {
    let svg = match state.style {
        IconStyle::Battery => battery::svg(*state),
        IconStyle::Circular => circular::svg(*state),
        IconStyle::Minimal => minimal::svg(*state),
        IconStyle::Segments => segments::svg(*state),
        IconStyle::DualBar => dual_bar::svg(*state),
        IconStyle::Gauge => gauge::svg(*state),
    };
    let tree = Tree::from_str(&svg, &Options::default())
        .map_err(|error| RenderError::Template(error.to_string()))?;

    let size = BASE_SIZE * state.scale.factor();
    let mut pixmap = Pixmap::new(size, size).ok_or(RenderError::Pixmap(size))?;
    let factor = state.scale.factor_f32();
    resvg::render(
        &tree,
        Transform::from_scale(factor, factor),
        &mut pixmap.as_mut(),
    );

    // tiny-skia stores premultiplied alpha; tray images want straight alpha.
    let rgba = pixmap
        .pixels()
        .iter()
        .flat_map(|pixel| {
            let color = pixel.demultiply();
            [color.red(), color.green(), color.blue(), color.alpha()]
        })
        .collect();

    Ok(RenderedIcon {
        width: size,
        height: size,
        rgba,
        is_template: state.mono,
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use crate::state::Scale;
    use meter_core::UsageStatus;
    use pretty_assertions::assert_eq;

    const ALL_STYLES: [IconStyle; 6] = [
        IconStyle::Battery,
        IconStyle::Circular,
        IconStyle::Minimal,
        IconStyle::Segments,
        IconStyle::DualBar,
        IconStyle::Gauge,
    ];

    fn state(percent: u8, status: UsageStatus, mono: bool, scale: Scale) -> IconState {
        style_state(IconStyle::Battery, percent, status, mono, scale)
    }

    fn style_state(
        style: IconStyle,
        percent: u8,
        status: UsageStatus,
        mono: bool,
        scale: Scale,
    ) -> IconState {
        IconState {
            style,
            percent,
            secondary_percent: percent,
            status,
            at_risk: false,
            mono,
            scale,
        }
    }

    fn opaque_pixels(icon: &RenderedIcon) -> impl Iterator<Item = &[u8]> {
        icon.rgba.chunks_exact(4).filter(|px| px[3] > 200)
    }

    #[test]
    fn dimensions_match_scale() {
        let x1 = render_icon(&state(50, UsageStatus::Warning, false, Scale::X1)).unwrap();
        assert_eq!((x1.width, x1.height), (22, 22));
        assert_eq!(x1.rgba.len(), 22 * 22 * 4);

        let x2 = render_icon(&state(50, UsageStatus::Warning, false, Scale::X2)).unwrap();
        assert_eq!((x2.width, x2.height), (44, 44));
        assert_eq!(x2.rgba.len(), 44 * 44 * 4);
    }

    #[test]
    fn safe_icon_is_green_where_opaque() {
        let icon = render_icon(&state(80, UsageStatus::Safe, false, Scale::X1)).unwrap();
        let mut seen = 0_usize;
        for px in opaque_pixels(&icon) {
            assert!(px[1] > px[0] && px[1] > px[2], "expected green, got {px:?}");
            seen += 1;
        }
        assert!(seen > 20, "icon should have substantial opaque coverage");
    }

    #[test]
    fn mono_icon_is_black_ink_only() {
        let icon = render_icon(&state(80, UsageStatus::Critical, true, Scale::X1)).unwrap();
        assert!(icon.is_template);
        for px in opaque_pixels(&icon) {
            assert_eq!(&px[..3], [0, 0, 0], "mono artwork must be pure black");
        }
    }

    #[test]
    fn colour_icon_is_not_a_template() {
        let icon = render_icon(&state(10, UsageStatus::Safe, false, Scale::X1)).unwrap();
        assert!(!icon.is_template);
    }

    #[test]
    fn fuller_battery_has_more_ink() {
        let count = |percent| {
            let icon = render_icon(&state(percent, UsageStatus::Safe, false, Scale::X1)).unwrap();
            icon.rgba.chunks_exact(4).filter(|px| px[3] > 0).count()
        };
        assert!(count(100) > count(50));
        assert!(count(50) > count(0));
    }

    // --- every style, not just Battery -------------------------------------
    //
    // Issue #9's acceptance criteria call for every style to stay legible at
    // 22px and in monochrome/template mode. These loop over `ALL_STYLES` so a
    // future seventh style is covered automatically; the perceptual-hash
    // snapshot matrix in `tests/snapshot.rs` covers the visual shape.

    #[test]
    fn every_style_renders_at_both_scales() {
        for style in ALL_STYLES {
            let x1 = render_icon(&style_state(
                style,
                65,
                UsageStatus::Warning,
                false,
                Scale::X1,
            ))
            .unwrap();
            assert_eq!((x1.width, x1.height), (22, 22), "{style:?} at 1x");

            let x2 = render_icon(&style_state(
                style,
                65,
                UsageStatus::Warning,
                false,
                Scale::X2,
            ))
            .unwrap();
            assert_eq!((x2.width, x2.height), (44, 44), "{style:?} at 2x");
        }
    }

    #[test]
    fn every_style_is_pure_black_ink_in_mono() {
        for style in ALL_STYLES {
            let icon = render_icon(&style_state(
                style,
                80,
                UsageStatus::Critical,
                true,
                Scale::X1,
            ))
            .unwrap();
            assert!(icon.is_template, "{style:?} must be a template in mono");
            for px in opaque_pixels(&icon) {
                assert_eq!(&px[..3], [0, 0, 0], "{style:?} mono ink must be black");
            }
        }
    }

    #[test]
    fn every_style_renders_some_ink_at_a_nonzero_percent() {
        for style in ALL_STYLES {
            let icon = render_icon(&style_state(
                style,
                45,
                UsageStatus::Warning,
                false,
                Scale::X1,
            ))
            .unwrap();
            let opaque = icon.rgba.chunks_exact(4).filter(|px| px[3] > 0).count();
            assert!(opaque > 0, "{style:?} produced no visible ink at all");
        }
    }
}
