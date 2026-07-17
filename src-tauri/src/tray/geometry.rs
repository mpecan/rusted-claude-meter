//! macOS-only popover positioning math: pure geometry, no Tauri types.
//!
//! Computes where the popover window goes for a tray-icon click — centred
//! under the icon, just below the menu bar, clamped inside the screen. Kept
//! separate from the cross-platform view-model in [`super::model`] because
//! only [`super::popover`] (and therefore only macOS) ever uses it.

/// Tray icon bounds in physical pixels (origin top-left of the screen).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrayRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// Horizontal extent of the screen the tray click landed on, physical px.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScreenBounds {
    pub x: f64,
    pub width: f64,
}

/// Gap between the menu bar and the popover's top edge, physical px.
const POPOVER_GAP: f64 = 8.0;
/// Minimum distance the popover keeps from the screen edges, physical px.
const POPOVER_MARGIN: f64 = 8.0;

/// Top-left corner for the macOS popover window: centred under the tray
/// icon, just below the menu bar, clamped inside the screen when its bounds
/// are known.
pub fn popover_origin(
    tray: TrayRect,
    window_width: f64,
    screen: Option<ScreenBounds>,
) -> (f64, f64) {
    let mut x = tray.width.mul_add(0.5, tray.x) - window_width / 2.0;
    if let Some(screen) = screen {
        let min_x = screen.x + POPOVER_MARGIN;
        let max_x = min_x.max(screen.x + screen.width - window_width - POPOVER_MARGIN);
        x = x.clamp(min_x, max_x);
    }
    (x, tray.y + tray.height + POPOVER_GAP)
}

#[cfg(test)]
mod tests {
    // Popover coordinates are exact float arithmetic on whole numbers.
    #![allow(clippy::float_cmp)]

    use super::*;
    use pretty_assertions::assert_eq;

    const TRAY: TrayRect = TrayRect {
        x: 1000.0,
        y: 0.0,
        width: 44.0,
        height: 24.0,
    };

    #[test]
    fn popover_centres_under_the_tray_icon() {
        let (x, y) = popover_origin(TRAY, 420.0, None);
        assert_eq!(x, 1000.0 + 22.0 - 210.0);
        assert_eq!(y, 24.0 + 8.0);
    }

    #[test]
    fn popover_clamps_to_the_right_screen_edge() {
        let tray = TrayRect { x: 1200.0, ..TRAY };
        let screen = ScreenBounds {
            x: 0.0,
            width: 1280.0,
        };
        let (x, _) = popover_origin(tray, 420.0, Some(screen));
        assert_eq!(x, 1280.0 - 420.0 - 8.0);
    }

    #[test]
    fn popover_clamps_to_the_left_screen_edge() {
        let tray = TrayRect { x: 4.0, ..TRAY };
        let screen = ScreenBounds {
            x: 0.0,
            width: 1280.0,
        };
        let (x, _) = popover_origin(tray, 420.0, Some(screen));
        assert_eq!(x, 8.0);
    }

    #[test]
    fn popover_wider_than_the_screen_pins_to_the_left_margin() {
        let screen = ScreenBounds {
            x: 0.0,
            width: 300.0,
        };
        let (x, _) = popover_origin(TRAY, 420.0, Some(screen));
        assert_eq!(x, 8.0);
    }
}
