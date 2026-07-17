//! Domain model for Claude plan usage.
//!
//! This crate is platform-neutral and UI-free: it knows about usage windows,
//! model-scoped limits, status thresholds and pacing risk, and nothing about
//! HTTP, trays or webviews. Everything here must stay trivially testable.

mod browser;
pub mod notify;
mod pacing;
mod session;
mod snapshot;
mod status;
mod window;

pub use browser::{
    Browser, BrowserCookie, BrowserFamily, CLAUDE_HOST, CookieImportError,
    FULL_DISK_ACCESS_SETTINGS_URL, Os, SESSION_COOKIE_NAME, session_key_from_cookies,
};
pub use pacing::{PacingAssessment, RISK_THRESHOLD};
pub use session::{SessionKey, SessionKeyError};
pub use snapshot::{ScopedLimit, UsageSnapshot};
pub use status::UsageStatus;
pub use window::{LimitWindow, UsageWindow};
