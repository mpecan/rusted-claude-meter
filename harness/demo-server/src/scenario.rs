//! Scenario files: load from disk, substitute relative-time tokens, serve.
//!
//! Scenarios are plain JSON in the shape claude.ai's usage endpoint returns —
//! the same shape as `crates/meter-api/tests/fixtures/usage_response.json`,
//! which is the pre-decode payload `meter_api::UsageResponse` parses. (Do not
//! confuse it with `src/demo.ts`, which is the *post-decode* fixture the
//! browser-only frontend demo uses.)
//!
//! ## Why timestamps are templated
//!
//! `meter_core::pacing` derives "how far into this window am I" from
//! `resets_at` minus the window length. A scenario with hardcoded dates
//! therefore reads as 100% elapsed the day after it is written, and every pace
//! ratio collapses to the same value — the pace bands, the flame/snowflake
//! badge and the limit-hit projection all become untestable.
//!
//! So scenarios write `resets_at` as a token relative to request time:
//!
//! ```json
//! { "five_hour": { "utilization": 34.0, "resets_at": "{{now+3h}}" } }
//! ```
//!
//! `{{now}}`, `{{now+90m}}`, `{{now-2h}}`, units `s`/`m`/`h`/`d`. Substitution
//! happens on every request, so a scenario stays "3 hours from now" no matter
//! how long the server has been up.

use std::path::{Path, PathBuf};

use jiff::{SignedDuration, Timestamp};

/// A scenario is rendered per request rather than cached, so editing the JSON
/// in an editor takes effect on the app's very next poll — no server restart,
/// no app restart. That is the whole point of the harness: change the data and
/// watch the tray react.
pub struct Scenarios {
    dir: PathBuf,
}

#[derive(Debug)]
pub enum ScenarioError {
    NotFound(String),
    Unreadable(String),
    /// The file is not valid JSON. Caught at render time and reported as a
    /// 500 with the parse message, because a typo in a hand-edited scenario is
    /// the single most likely failure here and a silent empty body would send
    /// you debugging the app instead of the fixture.
    Invalid(String),
}

impl std::fmt::Display for ScenarioError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound(name) => write!(f, "no scenario named `{name}`"),
            Self::Unreadable(msg) => write!(f, "scenario could not be read: {msg}"),
            Self::Invalid(msg) => write!(f, "scenario is not valid JSON: {msg}"),
        }
    }
}

impl Scenarios {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    /// Scenario names available on disk, sorted. Backs `GET /__admin/scenarios`
    /// so the driver script can list them without duplicating the file layout.
    pub fn available(&self) -> Vec<String> {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        let mut names: Vec<String> = entries
            .filter_map(Result::ok)
            .filter_map(|entry| {
                let path = entry.path();
                if path.extension().is_some_and(|ext| ext == "json") {
                    path.file_stem().map(|stem| stem.to_string_lossy().into_owned())
                } else {
                    None
                }
            })
            .collect();
        names.sort();
        names
    }

    pub fn exists(&self, name: &str) -> bool {
        self.path(name).is_file()
    }

    /// Read `name`, substitute time tokens against `now`, and parse. Returns
    /// the parsed value (not a string) so a malformed scenario fails here
    /// rather than reaching the app as a body it cannot decode.
    pub fn render(&self, name: &str, now: Timestamp) -> Result<serde_json::Value, ScenarioError> {
        let path = self.path(name);
        if !path.is_file() {
            return Err(ScenarioError::NotFound(name.to_owned()));
        }
        let raw = std::fs::read_to_string(&path)
            .map_err(|error| ScenarioError::Unreadable(error.to_string()))?;
        let substituted = substitute_time_tokens(&raw, now);
        serde_json::from_str(&substituted)
            .map_err(|error| ScenarioError::Invalid(error.to_string()))
    }

    fn path(&self, name: &str) -> PathBuf {
        // `name` arrives from the control plane, so keep it to a bare file
        // stem — no traversal out of the scenarios directory.
        let safe: String = name
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
            .collect();
        Path::new(&self.dir).join(format!("{safe}.json"))
    }
}

/// Replace every `{{now...}}` token with an RFC 3339 UTC timestamp.
///
/// Unknown or malformed tokens are left verbatim rather than silently
/// resolving to `now`: the resulting JSON then fails to parse as a timestamp
/// and you see the bad token, instead of quietly getting the wrong window.
fn substitute_time_tokens(raw: &str, now: Timestamp) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(start) = rest.find("{{now") {
        let (before, from_token) = rest.split_at(start);
        out.push_str(before);
        let Some(end) = from_token.find("}}") else {
            // Unterminated token: nothing sensible to do but emit the rest.
            out.push_str(from_token);
            return out;
        };
        let token = &from_token[2..end];
        match offset_of(token) {
            Some(offset) => out.push_str(&format_timestamp(now + offset)),
            None => out.push_str(&from_token[..end + 2]),
        }
        rest = &from_token[end + 2..];
    }
    out.push_str(rest);
    out
}

/// Parse the body of a `{{now±<n><unit>}}` token into a duration offset.
/// `{{now}}` is zero. Returns `None` for anything unrecognised.
fn offset_of(token: &str) -> Option<SignedDuration> {
    let arg = token.strip_prefix("now")?.trim();
    if arg.is_empty() {
        return Some(SignedDuration::ZERO);
    }
    let (sign, rest) = match arg.split_at_checked(1)? {
        ("+", rest) => (1, rest),
        ("-", rest) => (-1, rest),
        _ => return None,
    };
    let (digits, unit) = rest.split_at_checked(rest.len().checked_sub(1)?)?;
    let amount: i64 = digits.parse().ok()?;
    let seconds = match unit {
        "s" => 1,
        "m" => 60,
        "h" => 60 * 60,
        "d" => 24 * 60 * 60,
        _ => return None,
    };
    Some(SignedDuration::from_secs(
        sign * amount.checked_mul(seconds)?,
    ))
}

/// RFC 3339 with a `Z` suffix and whole seconds — the format claude.ai uses
/// and `jiff`'s `Deserialize` on the app side accepts.
fn format_timestamp(at: Timestamp) -> String {
    at.round(jiff::Unit::Second)
        .unwrap_or(at)
        .strftime("%Y-%m-%dT%H:%M:%SZ")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::{format_timestamp, offset_of, substitute_time_tokens};
    use jiff::{SignedDuration, Timestamp};

    fn epoch() -> Timestamp {
        "2026-07-25T12:00:00Z".parse().expect("valid timestamp")
    }

    #[test]
    fn bare_now_is_the_current_instant() {
        assert_eq!(offset_of("now"), Some(SignedDuration::ZERO));
    }

    #[test]
    fn each_unit_is_understood() {
        assert_eq!(offset_of("now+30s"), Some(SignedDuration::from_secs(30)));
        assert_eq!(offset_of("now+90m"), Some(SignedDuration::from_secs(5400)));
        assert_eq!(offset_of("now+3h"), Some(SignedDuration::from_secs(10800)));
        assert_eq!(offset_of("now+2d"), Some(SignedDuration::from_secs(172_800)));
    }

    #[test]
    fn negative_offsets_go_backwards() {
        assert_eq!(offset_of("now-2h"), Some(SignedDuration::from_secs(-7200)));
    }

    #[test]
    fn unrecognised_tokens_are_rejected_rather_than_defaulting_to_now() {
        assert_eq!(offset_of("now+3w"), None);
        assert_eq!(offset_of("now*3h"), None);
        assert_eq!(offset_of("now+h"), None);
        assert_eq!(offset_of("nonsense"), None);
    }

    #[test]
    fn tokens_are_replaced_in_place() {
        let rendered = substitute_time_tokens(
            r#"{"a":"{{now}}","b":"{{now+3h}}","c":"{{now-1d}}"}"#,
            epoch(),
        );
        assert_eq!(
            rendered,
            r#"{"a":"2026-07-25T12:00:00Z","b":"2026-07-25T15:00:00Z","c":"2026-07-24T12:00:00Z"}"#
        );
    }

    #[test]
    fn a_bad_token_survives_verbatim_so_the_typo_is_visible() {
        let rendered = substitute_time_tokens(r#"{"a":"{{now+3w}}"}"#, epoch());
        assert_eq!(rendered, r#"{"a":"{{now+3w}}"}"#);
    }

    #[test]
    fn text_without_tokens_is_untouched() {
        let raw = r#"{"five_hour":{"utilization":34.0}}"#;
        assert_eq!(substitute_time_tokens(raw, epoch()), raw);
    }

    #[test]
    fn an_unterminated_token_does_not_lose_the_rest_of_the_file() {
        let rendered = substitute_time_tokens(r#"{"a":"{{now+3h"#, epoch());
        assert_eq!(rendered, r#"{"a":"{{now+3h"#);
    }

    #[test]
    fn timestamps_render_as_whole_second_utc() {
        assert_eq!(format_timestamp(epoch()), "2026-07-25T12:00:00Z");
    }
}
