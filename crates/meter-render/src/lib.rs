//! Tray gauge renderer: turns a [`meter_core::UsageSnapshot`]-derived
//! [`IconState`] into raw RGBA pixels for the tray icon.
//!
//! The pipeline is a pure function: state → parameterized SVG template →
//! `resvg` rasterization → straight-alpha RGBA. No platform code lives here;
//! `src-tauri` wraps the bytes in a tray image and applies macOS template
//! semantics using [`RenderedIcon::is_template`].
//!
//! All six `ClaudeMeter` styles are implemented: Battery, Circular, Minimal,
//! Segments, Dual Bar and Gauge (issue #9).

mod battery;
mod cache;
mod circular;
mod dual_bar;
mod font;
mod gauge;
mod minimal;
mod palette;
mod render;
mod segments;
mod state;
mod svg;

pub use cache::IconCache;
pub use render::{RenderError, RenderedIcon, render_icon};
pub use state::{BASE_HEIGHT, IconState, IconStyle, Scale, round_percent};
