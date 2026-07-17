use std::fmt;

use serde::{Deserialize, Serialize};

const KEY_PREFIX: &str = "sk-ant-";
const COOKIE_NAME: &str = "sessionKey=";
const MIN_KEY_LENGTH: usize = 20;

/// A claude.ai `sessionKey` cookie value.
///
/// Accepts either the raw `sk-ant-...` value or a full cookie header
/// containing `sessionKey=...`. `Debug` and `Display` redact the value so the
/// key can never leak into logs.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SessionKey(String);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SessionKeyError {
    #[error("session key is empty")]
    Empty,
    #[error("session key must start with `sk-ant-` or contain `sessionKey=`")]
    MissingPrefix,
    #[error("session key is too short to be valid")]
    TooShort,
    #[error("session key contains characters outside [A-Za-z0-9_-]")]
    InvalidCharacters,
}

impl SessionKey {
    /// Parse user-supplied input into a session key.
    pub fn parse(input: &str) -> Result<Self, SessionKeyError> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Err(SessionKeyError::Empty);
        }
        let candidate = Self::extract_candidate(trimmed)?;
        if candidate.len() < MIN_KEY_LENGTH {
            return Err(SessionKeyError::TooShort);
        }
        let body = &candidate[KEY_PREFIX.len()..];
        if !body
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            return Err(SessionKeyError::InvalidCharacters);
        }
        Ok(Self(candidate.to_owned()))
    }

    /// The raw value, for building the `Cookie` request header.
    pub fn expose(&self) -> &str {
        &self.0
    }

    fn extract_candidate(input: &str) -> Result<&str, SessionKeyError> {
        if input.starts_with(KEY_PREFIX) {
            return Ok(input);
        }
        let Some(start) = input.find(COOKIE_NAME) else {
            return Err(SessionKeyError::MissingPrefix);
        };
        let value = &input[start + COOKIE_NAME.len()..];
        let end = value.find(';').unwrap_or(value.len());
        let value = value[..end].trim();
        if value.starts_with(KEY_PREFIX) {
            Ok(value)
        } else {
            Err(SessionKeyError::MissingPrefix)
        }
    }
}

impl fmt::Debug for SessionKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SessionKey(sk-ant-…redacted)")
    }
}

impl fmt::Display for SessionKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("sk-ant-…redacted")
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use pretty_assertions::assert_eq;

    const VALID: &str = "sk-ant-sid01-abcDEF123456_-xyz789";

    #[test]
    fn parses_raw_key() {
        assert_eq!(SessionKey::parse(VALID).unwrap().expose(), VALID);
    }

    #[test]
    fn parses_key_out_of_cookie_header() {
        let header = format!("ajs_id=123; sessionKey={VALID}; other=value");
        assert_eq!(SessionKey::parse(&header).unwrap().expose(), VALID);
    }

    #[test]
    fn parses_cookie_value_at_end_of_header() {
        let header = format!("sessionKey={VALID}");
        assert_eq!(SessionKey::parse(&header).unwrap().expose(), VALID);
    }

    #[test]
    fn trims_surrounding_whitespace() {
        let padded = format!("  {VALID}\n");
        assert_eq!(SessionKey::parse(&padded).unwrap().expose(), VALID);
    }

    #[test]
    fn rejects_bad_input() {
        assert_eq!(SessionKey::parse(""), Err(SessionKeyError::Empty));
        assert_eq!(
            SessionKey::parse("not-a-key"),
            Err(SessionKeyError::MissingPrefix)
        );
        assert_eq!(
            SessionKey::parse("sk-ant-short"),
            Err(SessionKeyError::TooShort)
        );
        assert_eq!(
            SessionKey::parse("sk-ant-sid01-abc DEF123456789"),
            Err(SessionKeyError::InvalidCharacters)
        );
    }

    #[test]
    fn debug_and_display_redact() {
        let key = SessionKey::parse(VALID).unwrap();
        assert!(!format!("{key:?}").contains("abcDEF"));
        assert!(!format!("{key}").contains("abcDEF"));
    }
}
