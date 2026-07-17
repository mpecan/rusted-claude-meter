/// Errors from talking to the claude.ai API.
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    /// HTTP 401: the session key is invalid or expired — the user must
    /// re-import or re-paste their session.
    #[error("session key is invalid or expired")]
    Unauthorized,
    /// HTTP 403: likely a Cloudflare challenge; the spoofed headers were not
    /// accepted.
    #[error("request was blocked (HTTP 403), possibly by a Cloudflare challenge")]
    Blocked,
    #[error("unexpected HTTP status {0}")]
    Status(u16),
    /// The session key survived parsing but cannot be encoded as a header —
    /// practically unreachable because [`meter_core::SessionKey`] validates
    /// its charset.
    #[error("session key cannot be encoded into a request header")]
    InvalidSessionKey,
    #[error("network error: {0}")]
    Network(#[from] reqwest::Error),
    #[error("failed to decode API response: {0}")]
    Decode(#[from] serde_json::Error),
}
