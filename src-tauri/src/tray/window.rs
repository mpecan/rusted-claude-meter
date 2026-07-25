//! The Linux stand-in for the macOS `NSPopover`.
//!
//! macOS gets a real popover from `tauri-plugin-nspopover`: frameless,
//! anchored under the status item, dismissed when it loses focus, and sized to
//! its content. Linux has no such primitive, so the `main` window is dressed to
//! behave the same way — undecorated, always on top, out of the taskbar, and
//! hidden as soon as focus goes elsewhere.
//!
//! Positioning is the part that cannot be perfect. `Activate(x, y)` is
//! specified as "an hint to the item where to show eventual windows", and
//! GNOME's AppIndicator extension says outright that the coordinates "don't
//! seem to have any effect" — it passes zeroes. So a zero anchor falls back to
//! the pointer, which is where the user just clicked and therefore next to the
//! tray icon in practice.

use tauri::{AppHandle, Manager, PhysicalPosition, PhysicalSize, Runtime, WebviewWindow};

/// Gap between the anchor point and the popover edge, in physical pixels.
const ANCHOR_GAP: i32 = 8;
/// Keep the popover this far from the work-area edges.
const EDGE_MARGIN: i32 = 8;

/// A rectangle in physical pixels — a monitor's usable area, or the popover.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// Dress the `main` window as a popover. Called once at startup, from the
/// Linux branch of the app's window setup.
pub fn configure<R: Runtime>(app: &AppHandle<R>) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let _ = window.set_decorations(false);
    let _ = window.set_always_on_top(true);
    let _ = window.set_skip_taskbar(true);
}

/// Show the popover at `anchor` if hidden, hide it if shown — the same toggle
/// the macOS tray click performs.
pub fn toggle<R: Runtime>(app: &AppHandle<R>, x: i32, y: i32) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    if window.is_visible().unwrap_or(false) {
        let _ = window.hide();
        return;
    }
    // Positioned twice on purpose. Before `show` so the first frame is already
    // in the right place, and again after, because a window manager applies its
    // own placement policy when it maps a window it has not seen before —
    // KWin centres it on screen otherwise, ignoring the pre-show position.
    position(&window, x, y);
    let _ = window.show();
    position(&window, x, y);
    let _ = window.set_focus();
}

/// Hide the popover when it loses focus, mirroring `NSPopover`'s transient
/// behaviour. Wired to the window's focus event at startup.
pub fn hide_on_focus_loss<R: Runtime>(window: &WebviewWindow<R>, focused: bool) {
    if !focused {
        let _ = window.hide();
    }
}

/// Resize the popover to a content-fitted height. Width stays fixed, matching
/// the macOS popover.
pub fn resize<R: Runtime>(app: &AppHandle<R>, width: f64, height: f64) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let _ = window.set_size(tauri::LogicalSize::new(width, height));
}

/// Move `window` so it sits next to the anchor without leaving the screen.
fn position<R: Runtime>(window: &WebviewWindow<R>, x: i32, y: i32) {
    let Ok(size) = window.outer_size() else {
        return;
    };
    let work = work_area(window);
    let anchor = if x == 0 && y == 0 {
        // GNOME passes zeroes; the pointer is where the click happened.
        cursor_position(window).unwrap_or((work.x + work.width / 2, work.y))
    } else {
        (x, y)
    };
    let placed = popover_position(anchor, size_of(size), work);
    let _ = window.set_position(PhysicalPosition::new(placed.0, placed.1));
}

fn size_of(size: PhysicalSize<u32>) -> (i32, i32) {
    (
        i32::try_from(size.width).unwrap_or(0),
        i32::try_from(size.height).unwrap_or(0),
    )
}

/// The monitor's usable area, falling back to its full bounds and then to a
/// conservative default when no monitor can be resolved.
fn work_area<R: Runtime>(window: &WebviewWindow<R>) -> Rect {
    match window.current_monitor() {
        Ok(Some(monitor)) => {
            let position = monitor.position();
            let size = monitor.size();
            Rect {
                x: position.x,
                y: position.y,
                width: i32::try_from(size.width).unwrap_or(0),
                height: i32::try_from(size.height).unwrap_or(0),
            }
        }
        _ => Rect {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        },
    }
}

fn cursor_position<R: Runtime>(window: &WebviewWindow<R>) -> Option<(i32, i32)> {
    let position = window.cursor_position().ok()?;
    // Physical pixel coordinates; the fractional part is meaningless here and
    // an out-of-range value would mean a screen wider than 2^31 px.
    Some((round_to_i32(position.x), round_to_i32(position.y)))
}

// The cast is guarded by the range check immediately above it, so the lint's
// truncation case cannot be reached; there is no cast-free f64 -> i32 in std.
#[allow(clippy::cast_possible_truncation)]
fn round_to_i32(value: f64) -> i32 {
    let rounded = value.round();
    if rounded.is_finite() && (f64::from(i32::MIN)..=f64::from(i32::MAX)).contains(&rounded) {
        rounded as i32
    } else {
        0
    }
}

/// Where the popover's top-left corner should go.
///
/// Pure, so the placement rules are testable without a display: centre on the
/// anchor horizontally, drop below it when the anchor is in the top half of
/// the screen (a top panel) and lift above it otherwise (a bottom panel), then
/// clamp inside the work area so it can never land off-screen.
fn popover_position(anchor: (i32, i32), size: (i32, i32), work: Rect) -> (i32, i32) {
    let (anchor_x, anchor_y) = anchor;
    let (width, height) = size;

    let x = anchor_x - width / 2;
    let anchored_at_top = anchor_y < work.y + work.height / 2;
    let y = if anchored_at_top {
        anchor_y + ANCHOR_GAP
    } else {
        anchor_y - height - ANCHOR_GAP
    };

    (
        clamp(x, work.x, work.x + work.width - width),
        clamp(y, work.y, work.y + work.height - height),
    )
}

/// `i32::clamp` panics when `min > max`, which happens whenever the popover is
/// larger than the work area. Prefer the top-left corner in that case.
fn clamp(value: i32, min: i32, max: i32) -> i32 {
    if max <= min {
        return min;
    }
    value.clamp(min + EDGE_MARGIN, max - EDGE_MARGIN).max(min)
}

#[cfg(test)]
mod tests {
    use super::{Rect, popover_position};
    use pretty_assertions::assert_eq;

    const SCREEN: Rect = Rect {
        x: 0,
        y: 0,
        width: 1920,
        height: 1080,
    };
    const SIZE: (i32, i32) = (420, 500);

    #[test]
    fn a_top_panel_anchor_drops_the_popover_below_it() {
        let (x, y) = popover_position((960, 10), SIZE, SCREEN);
        assert_eq!(x, 960 - 210);
        assert!(y > 10, "expected the popover below the anchor, got y={y}");
    }

    #[test]
    fn a_bottom_panel_anchor_lifts_the_popover_above_it() {
        let (_, y) = popover_position((960, 1070), SIZE, SCREEN);
        assert!(
            y + SIZE.1 < 1070,
            "expected the popover to clear the anchor, got y={y}"
        );
    }

    #[test]
    fn an_anchor_at_the_right_edge_stays_on_screen() {
        let (x, _) = popover_position((1915, 10), SIZE, SCREEN);
        assert!(
            x + SIZE.0 <= SCREEN.width,
            "popover ran off the right edge: x={x}"
        );
    }

    #[test]
    fn an_anchor_at_the_left_edge_stays_on_screen() {
        let (x, _) = popover_position((2, 10), SIZE, SCREEN);
        assert!(x >= 0, "popover ran off the left edge: x={x}");
    }

    #[test]
    fn a_second_monitor_offset_is_respected() {
        // A monitor to the right of the primary: the popover must land inside
        // it, not at the primary's coordinates.
        let right = Rect {
            x: 1920,
            y: 0,
            width: 1920,
            height: 1080,
        };
        let (x, _) = popover_position((2880, 10), SIZE, right);
        assert!(
            (1920..=1920 + 1920 - SIZE.0).contains(&x),
            "popover landed outside the second monitor: x={x}"
        );
    }

    #[test]
    fn a_popover_larger_than_the_screen_pins_to_the_corner() {
        let tiny = Rect {
            x: 0,
            y: 0,
            width: 300,
            height: 300,
        };
        assert_eq!(popover_position((150, 10), SIZE, tiny), (0, 0));
    }
}
