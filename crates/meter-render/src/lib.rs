//! Tray gauge renderer: turns a [`meter_core::UsageSnapshot`]-derived
//! [`IconState`] into raw RGBA pixels for the tray icon.
//!
//! The pipeline is a pure function: state → parameterized SVG template →
//! `resvg` rasterization → straight-alpha RGBA. No platform code lives here;
//! `src-tauri` wraps the bytes in a tray image and applies macOS template
//! semantics using [`RenderedIcon::is_template`].
//!
//! Battery is the only style for now; the remaining five styles are issue #9.

mod battery;
mod cache;
mod render;
mod state;

pub use cache::IconCache;
pub use render::{RenderError, RenderedIcon, render_icon};
pub use state::{BASE_SIZE, IconState, IconStyle, Scale, round_percent};
