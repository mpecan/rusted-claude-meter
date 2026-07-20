//! claude.ai usage API client.
//!
//! Owns everything HTTP: the spoofed browser headers the endpoint expects,
//! the raw response shapes, and the mapping into [`meter_core`] domain types.
//! The mapping implements the model-scoped limits contract: the `limits`
//! array is the source of truth for per-model caps (each entry names its own
//! scope via `scope.model.display_name`), while the flat `five_hour` /
//! `seven_day` fields remain as the headline windows.

mod client;
mod error;
mod headers;
mod response;

pub use client::{DEFAULT_BASE_URL, Organization, UsageClient};
pub use error::ApiError;
pub use response::{RawLimit, RawSpend, RawWindow, UsageResponse};
