//! Domain model for Claude plan usage.
//!
//! This crate is platform-neutral and UI-free: it knows about usage windows,
//! model-scoped limits, status thresholds and pacing risk, and nothing about
//! HTTP, trays or webviews. Everything here must stay trivially testable.

mod pacing;
mod session;
mod snapshot;
mod status;
mod window;

pub use pacing::{PacingAssessment, RISK_THRESHOLD};
pub use session::{SessionKey, SessionKeyError};
pub use snapshot::{ScopedLimit, UsageSnapshot};
pub use status::UsageStatus;
pub use window::{LimitWindow, UsageWindow};
