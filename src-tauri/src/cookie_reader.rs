//! Reading a browser's claude.ai cookies off disk — the I/O half of browser
//! session import (issue #10), split out of [`crate::browser_import`] which
//! owns the import flow itself.
//!
//! [`BrowserCookieReader`] is the seam the flow is generic over, so tests
//! drive it with a fake and never touch a real browser or the OS keyring.
//! [`RookieCookieReader`] is the production `rookie`-backed implementation.
//!
//! Per the issue's security bar: a locked or undecryptable cookie store
//! degrades to a per-browser [`CookieStoreError`] and can never crash the app
//! — the `rookie` call is both `Result`-checked and panic-guarded (see
//! [`read_guarded`]).

use meter_core::{Browser, BrowserCookie};

/// A per-browser failure reading the cookie store. The message is always safe
/// to surface: it describes the failure, never a cookie value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CookieStoreError(String);

impl CookieStoreError {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }

    pub(crate) fn into_message(self) -> String {
        self.0
    }
}

/// Reads a browser's claude.ai cookies. The seam the import flow is generic
/// over, so tests drive it with a fake and never touch a real browser.
pub trait BrowserCookieReader: Send + Sync {
    /// Read only the claude.ai cookies from `browser`'s store. Failures are
    /// per-browser and must never propagate as a panic.
    fn read_claude_cookies(&self, browser: Browser)
    -> Result<Vec<BrowserCookie>, CookieStoreError>;
}

/// Production reader backed by the `rookie` crate.
#[derive(Debug, Clone, Copy, Default)]
pub struct RookieCookieReader;

impl BrowserCookieReader for RookieCookieReader {
    fn read_claude_cookies(
        &self,
        browser: Browser,
    ) -> Result<Vec<BrowserCookie>, CookieStoreError> {
        // Scope the query to claude.ai so no unrelated cookie value is ever
        // read into memory. `rookie` filters with `host_key LIKE '%claude.ai%'`,
        // so the exact-host check in `session_key_from_cookies` still matters.
        let domains = Some(vec![meter_core::CLAUDE_HOST.to_owned()]);
        let raw = read_guarded(browser, move || read_rookie_cookies(browser, domains))?;
        Ok(raw
            .into_iter()
            .map(|cookie| BrowserCookie::new(cookie.domain, cookie.name, cookie.value))
            .collect())
    }
}

/// Run `read` with both failure modes mapped to [`CookieStoreError`].
///
/// A locked or corrupt store can make `rookie` panic (e.g. an empty Firefox
/// `installs` list), not just return `Err`. Guard both so a bad store
/// degrades to a per-browser error and never takes the app down — the
/// issue's explicit "never a crash" acceptance criterion.
fn read_guarded<F>(
    browser: Browser,
    read: F,
) -> Result<Vec<rookie::enums::Cookie>, CookieStoreError>
where
    F: FnOnce() -> rookie::Result<Vec<rookie::enums::Cookie>> + std::panic::UnwindSafe,
{
    std::panic::catch_unwind(read)
        .map_err(|_| {
            CookieStoreError::new(format!(
                "Reading {}'s cookie store failed unexpectedly.",
                browser.display_name()
            ))
        })?
        .map_err(|error| {
            CookieStoreError::new(format!(
                "Could not read cookies from {}: {error}",
                browser.display_name()
            ))
        })
}

/// Dispatch a [`Browser`] to its `rookie` reader function. Safari is macOS
/// only; on Linux it is rejected before this is reached.
fn read_rookie_cookies(
    browser: Browser,
    domains: Option<Vec<String>>,
) -> rookie::Result<Vec<rookie::enums::Cookie>> {
    match browser {
        Browser::Chrome => rookie::chrome(domains),
        Browser::Chromium => rookie::chromium(domains),
        Browser::Brave => rookie::brave(domains),
        Browser::Edge => rookie::edge(domains),
        Browser::Vivaldi => rookie::vivaldi(domains),
        Browser::Opera => rookie::opera(domains),
        Browser::OperaGx => rookie::opera_gx(domains),
        Browser::Arc => rookie::arc(domains),
        Browser::Firefox => rookie::firefox(domains),
        Browser::Librewolf => rookie::librewolf(domains),
        Browser::Zen => rookie::zen(domains),
        Browser::Safari => safari_cookies(domains),
    }
}

/// Safari's reader only exists on macOS; elsewhere the store cannot be present.
#[cfg(target_os = "macos")]
fn safari_cookies(domains: Option<Vec<String>>) -> rookie::Result<Vec<rookie::enums::Cookie>> {
    rookie::safari(domains)
}

#[cfg(not(target_os = "macos"))]
fn safari_cookies(_domains: Option<Vec<String>>) -> rookie::Result<Vec<rookie::enums::Cookie>> {
    // `rookie::Result` is `eyre::Result`; a `std::io::Error` converts into its
    // report through `eyre`'s blanket `From`, so no direct `eyre` dependency
    // is needed. In practice this is unreachable: `import_impl` rejects Safari
    // on non-macOS via `available_on` before the reader is called.
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "Safari is only available on macOS",
    )
    .into())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    // A deliberate panic is the whole point here: it stands in for the
    // unwind a locked or corrupt store can start inside `rookie`, proving
    // the guard turns it into a per-browser error instead of a crash.
    #[allow(clippy::panic)]
    fn a_panicking_read_degrades_to_a_cookie_store_error() {
        let error = read_guarded(Browser::Firefox, || std::panic::panic_any("boom"))
            .unwrap_err()
            .into_message();
        assert_eq!(error, "Reading Firefox's cookie store failed unexpectedly.");
        assert!(!error.contains("boom"));
    }

    #[test]
    fn a_read_error_is_wrapped_with_the_browser_name() {
        let error = read_guarded(Browser::Brave, || {
            Err(std::io::Error::other("keyring is locked").into())
        })
        .unwrap_err()
        .into_message();
        assert!(error.contains("Brave"));
        assert!(error.contains("keyring is locked"));
    }

    #[test]
    fn a_successful_read_passes_the_cookies_through() {
        let cookies = read_guarded(Browser::Chrome, || Ok(Vec::new())).unwrap();
        assert_eq!(cookies.len(), 0);
    }
}
