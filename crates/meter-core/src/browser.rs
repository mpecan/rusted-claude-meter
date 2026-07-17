//! Browser session import — the pure, I/O-free core.
//!
//! The claude.ai `sessionKey` cookie authenticates every API call. Rather
//! than force the user to dig it out of `DevTools`, the app can import it
//! straight from an installed browser (issue #10, the `SweetCookieKit`
//! feature). Reading and decrypting a browser's cookie store is platform I/O
//! and lives in the app shell (`src-tauri`); this module owns only the parts
//! that must stay trivially testable:
//!
//! * which [`Browser`]s exist, how they group into a [`BrowserFamily`], and on
//!   which [`Os`] each one's cookie store can be present at all;
//! * the permission story each combination implies (macOS Keychain prompt for
//!   Chromium browsers, Full Disk Access for Safari), as plain copy the UI can
//!   surface before it ever touches the cookie store;
//! * turning a bag of already-read cookies into a validated [`SessionKey`],
//!   keeping only the claude.ai `sessionKey` value and rejecting everything
//!   else.
//!
//! No function here reads a file, spawns a process, or touches the network.

use serde::{Deserialize, Serialize};

use crate::session::{SessionKey, SessionKeyError};

/// The host whose `sessionKey` cookie authenticates the claude.ai API.
pub const CLAUDE_HOST: &str = "claude.ai";
/// The cookie name holding the session token.
pub const SESSION_COOKIE_NAME: &str = "sessionKey";
/// Deep link to the macOS "Full Disk Access" settings pane, so the UI can
/// send the user straight there when Safari import needs the permission.
pub const FULL_DISK_ACCESS_SETTINGS_URL: &str =
    "x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles";

/// The operating systems this app targets.
///
/// `Other` exists so the pure logic stays total on platforms the app is never
/// shipped to (every browser is then reported unavailable) instead of the
/// caller having to guarantee the value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Os {
    MacOs,
    Linux,
    Other,
}

/// How a browser stores its cookies, which decides the permission story.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowserFamily {
    /// Chromium-derived: cookies are AES-encrypted with a "Safe Storage" key
    /// held in the OS keyring (Keychain on macOS; GNOME Keyring / `KWallet` on
    /// Linux), so decryption can trigger a keyring-unlock prompt.
    Chromium,
    /// Firefox-derived: a `cookies.sqlite` the value is read from directly.
    Firefox,
    /// Safari: a `Cookies.binarycookies` file that needs Full Disk Access to
    /// read on modern macOS.
    Safari,
}

/// A browser the app can attempt to import a claude.ai session from.
///
/// Restricted to browsers the underlying `rookie` reader supports on the two
/// platforms this app ships to (macOS + Linux). Serialized as a stable
/// `snake_case` id so the frontend can round-trip a selection back to the
/// import command.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Browser {
    Chrome,
    Chromium,
    Brave,
    Edge,
    Vivaldi,
    Opera,
    OperaGx,
    Arc,
    Firefox,
    Librewolf,
    Zen,
    Safari,
}

impl Browser {
    /// Every browser the app can attempt to import from, in the order the UI
    /// should list them.
    pub const ALL: [Self; 12] = [
        Self::Chrome,
        Self::Chromium,
        Self::Brave,
        Self::Edge,
        Self::Vivaldi,
        Self::Opera,
        Self::OperaGx,
        Self::Arc,
        Self::Firefox,
        Self::Librewolf,
        Self::Zen,
        Self::Safari,
    ];

    /// Human-facing name for the UI.
    pub const fn display_name(self) -> &'static str {
        match self {
            Self::Chrome => "Google Chrome",
            Self::Chromium => "Chromium",
            Self::Brave => "Brave",
            Self::Edge => "Microsoft Edge",
            Self::Vivaldi => "Vivaldi",
            Self::Opera => "Opera",
            Self::OperaGx => "Opera GX",
            Self::Arc => "Arc",
            Self::Firefox => "Firefox",
            Self::Librewolf => "LibreWolf",
            Self::Zen => "Zen",
            Self::Safari => "Safari",
        }
    }

    /// Cookie-store family this browser belongs to.
    pub const fn family(self) -> BrowserFamily {
        match self {
            Self::Firefox | Self::Librewolf | Self::Zen => BrowserFamily::Firefox,
            Self::Safari => BrowserFamily::Safari,
            Self::Chrome
            | Self::Chromium
            | Self::Brave
            | Self::Edge
            | Self::Vivaldi
            | Self::Opera
            | Self::OperaGx
            | Self::Arc => BrowserFamily::Chromium,
        }
    }

    /// Whether this browser's cookie store can exist on `os` at all. Safari
    /// is macOS-only, and Arc — though Chromium-family — has never shipped a
    /// Linux build; every other supported browser exists on both macOS and
    /// Linux. On an unsupported platform nothing is available.
    pub const fn available_on(self, os: Os) -> bool {
        match os {
            Os::MacOs => true,
            Os::Linux => !matches!(self, Self::Safari | Self::Arc),
            Os::Other => false,
        }
    }

    /// One line of copy explaining the permission prompt the user should
    /// expect when importing from this browser on `os`, or `None` when import
    /// needs no special permission. Shown *before* the cookie store is touched
    /// so the prompt is never a surprise.
    pub const fn permission_hint(self, os: Os) -> Option<&'static str> {
        match (self.family(), os) {
            (BrowserFamily::Chromium, Os::MacOs) => Some(
                "macOS will ask to unlock the login Keychain (a \"…Safe Storage\" item) so the \
                 cookie can be decrypted — approve the prompt to finish importing.",
            ),
            (BrowserFamily::Safari, Os::MacOs) => Some(
                "Safari's cookies need Full Disk Access. Grant it under System Settings → \
                 Privacy & Security → Full Disk Access, then import again.",
            ),
            (BrowserFamily::Chromium, Os::Linux) => Some(
                "If your desktop keyring (GNOME Keyring / KWallet) is locked, unlock it when \
                 prompted so the cookie can be decrypted.",
            ),
            _ => None,
        }
    }

    /// A settings deep link the UI can offer alongside the permission hint —
    /// currently only the macOS Full Disk Access pane for Safari.
    pub const fn settings_deep_link(self, os: Os) -> Option<&'static str> {
        match (self.family(), os) {
            (BrowserFamily::Safari, Os::MacOs) => Some(FULL_DISK_ACCESS_SETTINGS_URL),
            _ => None,
        }
    }
}

/// A single cookie already read out of a browser's store.
///
/// Deliberately minimal: the app only ever cares about the claude.ai
/// `sessionKey`, so the reader is expected to hand this layer only claude.ai
/// cookies and to drop everything else without it ever reaching here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserCookie {
    /// The cookie's host key, e.g. `claude.ai` or `.claude.ai`.
    pub host: String,
    /// The cookie name, e.g. `sessionKey`.
    pub name: String,
    /// The cookie value. For `sessionKey` this is the raw `sk-ant-…` token.
    pub value: String,
}

impl BrowserCookie {
    /// Convenience constructor, mostly for tests and the reader adapter.
    pub fn new(host: impl Into<String>, name: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            host: host.into(),
            name: name.into(),
            value: value.into(),
        }
    }
}

/// Why turning a browser's cookies into a session key failed.
///
/// Carries no cookie value: a bad token is described, never echoed, so the
/// error is always safe to log or show.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CookieImportError {
    /// No claude.ai `sessionKey` cookie was present in the store.
    #[error("no claude.ai session cookie was found")]
    NoSessionCookie,
    /// A `sessionKey` cookie was found, but its value failed
    /// [`SessionKey::parse`].
    #[error("the browser's session cookie is not valid: {0}")]
    InvalidSessionCookie(#[from] SessionKeyError),
}

/// Pick the claude.ai `sessionKey` out of `cookies` and parse it.
///
/// Only a cookie named exactly [`SESSION_COOKIE_NAME`] on a claude.ai host is
/// considered; any other cookie in the slice is ignored. The first match
/// wins.
pub fn session_key_from_cookies(
    cookies: &[BrowserCookie],
) -> Result<SessionKey, CookieImportError> {
    let cookie = cookies
        .iter()
        .find(|cookie| cookie.name == SESSION_COOKIE_NAME && host_is_claude(&cookie.host))
        .ok_or(CookieImportError::NoSessionCookie)?;
    Ok(SessionKey::parse(&cookie.value)?)
}

/// Whether `host` is claude.ai or a subdomain of it, tolerating the leading
/// dot browsers store on domain cookies (`.claude.ai`).
fn host_is_claude(host: &str) -> bool {
    let host = host.trim_start_matches('.');
    host == CLAUDE_HOST || host.ends_with(".claude.ai")
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use pretty_assertions::assert_eq;

    const VALID: &str = "sk-ant-sid01-abcDEF123456_-xyz789";

    fn cookie(host: &str, name: &str, value: &str) -> BrowserCookie {
        BrowserCookie::new(host, name, value)
    }

    #[test]
    fn all_lists_every_variant_once() {
        let mut seen = Browser::ALL.to_vec();
        seen.sort_by_key(|browser| browser.display_name());
        seen.dedup();
        assert_eq!(seen.len(), Browser::ALL.len());
    }

    #[test]
    fn families_group_as_expected() {
        assert_eq!(Browser::Chrome.family(), BrowserFamily::Chromium);
        assert_eq!(Browser::Arc.family(), BrowserFamily::Chromium);
        assert_eq!(Browser::Firefox.family(), BrowserFamily::Firefox);
        assert_eq!(Browser::Zen.family(), BrowserFamily::Firefox);
        assert_eq!(Browser::Safari.family(), BrowserFamily::Safari);
    }

    #[test]
    fn safari_is_macos_only() {
        assert!(Browser::Safari.available_on(Os::MacOs));
        assert!(!Browser::Safari.available_on(Os::Linux));
        assert!(!Browser::Safari.available_on(Os::Other));
    }

    #[test]
    fn arc_is_macos_only_despite_being_chromium_family() {
        // Arc has no Linux build, so offering it as a Linux import source
        // could only ever fail with a confusing cookie-store error.
        assert_eq!(Browser::Arc.family(), BrowserFamily::Chromium);
        assert!(Browser::Arc.available_on(Os::MacOs));
        assert!(!Browser::Arc.available_on(Os::Linux));
        assert!(!Browser::Arc.available_on(Os::Other));
    }

    #[test]
    fn chromium_and_firefox_available_on_both_desktop_platforms() {
        for browser in [Browser::Chrome, Browser::Brave, Browser::Firefox] {
            assert!(browser.available_on(Os::MacOs));
            assert!(browser.available_on(Os::Linux));
            assert!(!browser.available_on(Os::Other));
        }
    }

    #[test]
    fn chromium_on_macos_warns_about_the_keychain_prompt() {
        let hint = Browser::Chrome.permission_hint(Os::MacOs).unwrap();
        assert!(hint.contains("Keychain"));
        assert!(Browser::Chrome.settings_deep_link(Os::MacOs).is_none());
    }

    #[test]
    fn safari_on_macos_points_at_full_disk_access() {
        let hint = Browser::Safari.permission_hint(Os::MacOs).unwrap();
        assert!(hint.contains("Full Disk Access"));
        assert_eq!(
            Browser::Safari.settings_deep_link(Os::MacOs),
            Some(FULL_DISK_ACCESS_SETTINGS_URL)
        );
    }

    #[test]
    fn chromium_on_linux_warns_about_the_keyring() {
        let hint = Browser::Chrome.permission_hint(Os::Linux).unwrap();
        assert!(hint.contains("keyring"));
    }

    #[test]
    fn firefox_needs_no_special_permission() {
        assert!(Browser::Firefox.permission_hint(Os::MacOs).is_none());
        assert!(Browser::Firefox.permission_hint(Os::Linux).is_none());
        assert!(Browser::Firefox.settings_deep_link(Os::MacOs).is_none());
    }

    #[test]
    fn browser_id_round_trips_through_serde() {
        let json = serde_json::to_string(&Browser::OperaGx).unwrap();
        assert_eq!(json, "\"opera_gx\"");
        let back: Browser = serde_json::from_str(&json).unwrap();
        assert_eq!(back, Browser::OperaGx);
    }

    #[test]
    fn extracts_the_session_key_ignoring_other_cookies() {
        let cookies = vec![
            cookie(".claude.ai", "ajs_anonymous_id", "should-be-ignored"),
            cookie(".claude.ai", "sessionKey", VALID),
            cookie(".claude.ai", "lastActiveOrg", "org-uuid"),
        ];
        assert_eq!(session_key_from_cookies(&cookies).unwrap().expose(), VALID);
    }

    #[test]
    fn matches_the_bare_host_and_subdomains() {
        for host in ["claude.ai", ".claude.ai", "www.claude.ai"] {
            let cookies = vec![cookie(host, "sessionKey", VALID)];
            assert_eq!(
                session_key_from_cookies(&cookies).unwrap().expose(),
                VALID,
                "host {host} should match"
            );
        }
    }

    #[test]
    fn ignores_a_session_cookie_from_another_host() {
        let cookies = vec![cookie("evil.example", "sessionKey", VALID)];
        assert_eq!(
            session_key_from_cookies(&cookies),
            Err(CookieImportError::NoSessionCookie)
        );
    }

    #[test]
    fn ignores_a_non_prefixed_claude_host() {
        // `notclaude.ai` must not match `claude.ai`.
        let cookies = vec![cookie("notclaude.ai", "sessionKey", VALID)];
        assert_eq!(
            session_key_from_cookies(&cookies),
            Err(CookieImportError::NoSessionCookie)
        );
    }

    #[test]
    fn empty_store_reports_no_session_cookie() {
        assert_eq!(
            session_key_from_cookies(&[]),
            Err(CookieImportError::NoSessionCookie)
        );
    }

    #[test]
    fn a_malformed_session_cookie_is_surfaced_not_echoed() {
        let cookies = vec![cookie(".claude.ai", "sessionKey", "sk-ant-sid01-bad value")];
        let error = session_key_from_cookies(&cookies).unwrap_err();
        assert!(matches!(
            error,
            CookieImportError::InvalidSessionCookie(SessionKeyError::InvalidCharacters)
        ));
        // The offending raw value must never round-trip through the error.
        assert!(!format!("{error}").contains("bad value"));
        assert!(!format!("{error:?}").contains("bad value"));
    }
}
