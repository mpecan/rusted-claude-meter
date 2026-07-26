//! The API base URL the app talks to, and the one supported way to redirect it.
//!
//! Production always points at claude.ai ([`meter_api::DEFAULT_BASE_URL`]).
//! Setting [`OVERRIDE_ENV`] repoints every request at another host instead —
//! the seam the Linux desktop harness in `harness/` uses to drive a real,
//! unmodified build against a local demo server with no session key and no
//! network access to claude.ai (see `harness/README.md`).
//!
//! Two deliberate properties:
//!
//! * **The override ships in release builds.** A demo-only cargo feature would
//!   mean the VMs validate a binary that isn't the one users install; this way
//!   the `.deb`/`AppImage` under test is bit-for-bit the released artifact.
//! * **A bad value degrades, it never breaks.** Anything that isn't a plain
//!   `http`/`https` URL with a host is ignored and claude.ai is used, so a typo
//!   in a shell profile cannot brick the app.
//!
//! Because a redirected build looks identical to a real one, the override is
//! never silent: it is logged at startup and surfaced in Settings via
//! `crate::commands::debug::api_base_override`.

use std::sync::OnceLock;

use meter_api::DEFAULT_BASE_URL;

/// Environment variable that repoints the app at a non-claude.ai API base.
pub const OVERRIDE_ENV: &str = "RCM_API_BASE_URL";

/// Resolved once, on first use. The consumers run at very different times —
/// the startup log immediately, the Settings banner whenever that window is
/// opened, the session validator whenever a key is submitted — and the whole
/// point of the override is that a redirected build cannot mislead. Reading
/// the environment separately at each of those moments would let the banner
/// claim one endpoint while requests went to another; resolving once means
/// every consumer is describing the same value by construction.
static OVERRIDE: OnceLock<Option<String>> = OnceLock::new();

/// Validate a candidate override value. Pure, so the accept/reject rules are
/// unit-tested without touching the process environment — the same split as
/// `crate::wizard::linux_desktop` over `meter_core::LinuxDesktop`.
///
/// Accepts only `http://` or `https://` followed by a non-empty authority, so
/// neither an empty variable (the common "unset it" idiom of `FOO=`) nor a
/// scheme that `reqwest` cannot drive can take effect.
fn resolve_base_url(raw: Option<&str>) -> Option<String> {
    let trimmed = raw?.trim();
    let authority = trimmed
        .strip_prefix("http://")
        .or_else(|| trimmed.strip_prefix("https://"))?;
    // Reject `http://` alone and `http:///path`: there must be a host before
    // the first `/`, `?` or `#`.
    if authority
        .split(['/', '?', '#'])
        .next()
        .unwrap_or("")
        .is_empty()
    {
        return None;
    }
    Some(trimmed.trim_end_matches('/').to_owned())
}

/// The active override, if one is set and valid. `None` means the app is
/// talking to the real claude.ai — the state Settings must show as normal.
pub fn api_base_override() -> Option<&'static str> {
    OVERRIDE
        .get_or_init(|| resolve_base_url(std::env::var(OVERRIDE_ENV).ok().as_deref()))
        .as_deref()
}

/// The base URL every `UsageClient` in the app is built with.
pub fn api_base_url() -> &'static str {
    api_base_override().unwrap_or(DEFAULT_BASE_URL)
}

/// Log the override once at startup so a redirected build announces itself in
/// the terminal even before any window is open. No-op when unset.
pub fn log_override() {
    if let Some(base) = api_base_override() {
        eprintln!("{OVERRIDE_ENV} is set: talking to {base} instead of claude.ai");
    }
}

#[cfg(test)]
mod tests {
    use super::{DEFAULT_BASE_URL, api_base_url, resolve_base_url};

    #[test]
    fn an_unset_variable_means_claude_ai() {
        assert_eq!(resolve_base_url(None), None);
    }

    #[test]
    fn a_plain_http_url_is_accepted() {
        assert_eq!(
            resolve_base_url(Some("http://host.lima.internal:8787")),
            Some("http://host.lima.internal:8787".to_owned())
        );
    }

    #[test]
    fn https_is_accepted_too() {
        assert_eq!(
            resolve_base_url(Some("https://staging.example.com/api")),
            Some("https://staging.example.com/api".to_owned())
        );
    }

    #[test]
    fn surrounding_whitespace_is_trimmed() {
        assert_eq!(
            resolve_base_url(Some("  http://localhost:8787\n")),
            Some("http://localhost:8787".to_owned())
        );
    }

    #[test]
    fn a_trailing_slash_is_dropped_so_paths_join_cleanly() {
        // `UsageClient::with_base_url` trims trailing slashes too; doing it
        // here as well keeps the value shown in Settings identical to the one
        // actually requested.
        assert_eq!(
            resolve_base_url(Some("http://localhost:8787/")),
            Some("http://localhost:8787".to_owned())
        );
    }

    #[test]
    fn an_empty_variable_is_ignored_rather_than_treated_as_a_url() {
        assert_eq!(resolve_base_url(Some("")), None);
        assert_eq!(resolve_base_url(Some("   ")), None);
    }

    #[test]
    fn a_scheme_reqwest_cannot_drive_is_rejected() {
        assert_eq!(resolve_base_url(Some("file:///tmp/usage.json")), None);
        assert_eq!(resolve_base_url(Some("ftp://example.com")), None);
    }

    #[test]
    fn a_bare_host_without_a_scheme_is_rejected() {
        assert_eq!(resolve_base_url(Some("localhost:8787")), None);
        assert_eq!(resolve_base_url(Some("claude.ai/api")), None);
    }

    #[test]
    fn a_scheme_with_no_host_is_rejected() {
        assert_eq!(resolve_base_url(Some("http://")), None);
        assert_eq!(resolve_base_url(Some("http:///organizations")), None);
        assert_eq!(resolve_base_url(Some("https://?q=1")), None);
    }

    #[test]
    fn api_base_url_falls_back_to_claude_ai() {
        // Reads the real environment: in the test process the variable is
        // unset, which is exactly the production default this pins.
        assert_eq!(api_base_url(), DEFAULT_BASE_URL);
    }
}
