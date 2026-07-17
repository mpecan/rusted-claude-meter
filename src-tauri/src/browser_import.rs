//! Browser session import: read the claude.ai `sessionKey` straight out of an
//! installed browser (issue #10), so the user never has to dig it out of
//! `DevTools`.
//!
//! The pure parts — which browsers exist, their permission story, and turning
//! a bag of cookies into a validated [`SessionKey`] — live in
//! [`meter_core::browser`]. This module owns the I/O the domain crate must
//! not: reading and decrypting a browser's cookie store (via the `rookie`
//! crate), validating the recovered key against claude.ai, and persisting it.
//!
//! Everything is built around three seams so the flow is unit-testable with
//! fakes and never touches a real browser, the network, or the OS keyring in
//! tests:
//!
//! * [`BrowserCookieReader`] — reads a browser's claude.ai cookies (see
//!   [`crate::cookie_reader`], which also holds the production
//!   `rookie`-backed [`RookieCookieReader`] and the panic guard that keeps a
//!   locked or undecryptable store from ever crashing the app).
//! * [`SessionValidator`] — confirms a key with claude.ai;
//!   [`LiveSessionValidator`] calls `organizations()`.
//! * [`crate::store::SessionStore`] — persists the key (issue #1).
//!
//! Per the issue's security bar, no cookie value other than the claude.ai
//! `sessionKey` is kept in memory longer than the moment it takes to pick it
//! out.

use std::sync::Arc;

use meter_api::{ApiError, DEFAULT_BASE_URL, UsageClient};
use meter_core::{
    Browser, BrowserFamily, CookieImportError, Os, SessionKey, session_key_from_cookies,
};
use serde::Serialize;
use tauri::State;

use crate::commands::SessionStoreState;
use crate::cookie_reader::{BrowserCookieReader, RookieCookieReader};
use crate::scheduler::SchedulerHandle;
use crate::store::SessionStore;

/// A browser offered to the user as an import source, with the permission
/// story it implies on this platform.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DetectedBrowser {
    /// Stable `snake_case` id passed back to `import_browser_session`.
    pub id: Browser,
    /// Human-facing name.
    pub name: String,
    /// Cookie-store family, so the UI can group or badge sources if it wants.
    pub family: BrowserFamily,
    /// One line of copy warning about the permission prompt to expect, if any.
    pub permission_hint: Option<String>,
    /// A settings deep link to offer alongside the hint (Full Disk Access on
    /// macOS for Safari).
    pub settings_deep_link: Option<String>,
}

/// The result of a successful import, for the UI's confirmation message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ImportSummary {
    /// Which browser the key came from.
    pub browser: String,
    /// Whether claude.ai confirmed the key. `false` means the key is stored
    /// but validation was skipped because claude.ai was unreachable — the
    /// scheduler will validate it on its next poll.
    pub validated: bool,
}

/// Why an import attempt failed, mapped to a discriminated union the frontend
/// can message precisely. Carries only human-readable summaries — never a
/// cookie value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", content = "message")]
pub enum BrowserImportError {
    /// The browser has no cookie store on this platform (e.g. Safari on Linux).
    Unsupported(String),
    /// The cookie store could not be read or decrypted (locked keyring,
    /// missing profile, denied Full Disk Access, ...).
    CookieStore(String),
    /// The store was read, but held no claude.ai session.
    NoSession(String),
    /// A `sessionKey` cookie was present but failed to parse.
    Invalid(String),
    /// The key parsed but claude.ai rejected it (expired/invalid).
    Rejected(String),
    /// The credential store refused to persist the key.
    Store(String),
}

/// Confirms a session key with claude.ai. The seam the import flow is generic
/// over, so tests validate without network access.
pub trait SessionValidator: Send + Sync {
    fn validate<'a>(
        &'a self,
        key: &'a SessionKey,
    ) -> impl Future<Output = Result<(), ValidationError>> + Send + 'a;
}

/// Outcome of validating a key against claude.ai.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidationError {
    /// claude.ai rejected the key (401): it is expired or otherwise invalid.
    Unauthorized,
    /// A transient failure (network down, 5xx): the key might still be good,
    /// so it is kept and the scheduler retries.
    Transient,
}

/// Production validator: a session key is valid iff `GET /organizations`
/// succeeds — the same cheap check the scheduler uses.
pub struct LiveSessionValidator {
    base_url: String,
}

impl LiveSessionValidator {
    pub fn new() -> Self {
        Self {
            base_url: DEFAULT_BASE_URL.to_owned(),
        }
    }

    /// Point the validator at `base_url` instead of claude.ai — the seam the
    /// wiremock test uses to drive a real `UsageClient` over loopback.
    #[cfg(test)]
    pub fn with_base_url(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
        }
    }
}

impl Default for LiveSessionValidator {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionValidator for LiveSessionValidator {
    async fn validate<'a>(&'a self, key: &'a SessionKey) -> Result<(), ValidationError> {
        let client = UsageClient::with_base_url(key, &self.base_url)
            .map_err(|_| ValidationError::Transient)?;
        match client.organizations().await {
            Ok(_) => Ok(()),
            Err(ApiError::Unauthorized) => Err(ValidationError::Unauthorized),
            Err(_) => Err(ValidationError::Transient),
        }
    }
}

/// The importable browsers for `os`, with per-browser permission copy. Pure,
/// so it is exercised without a Tauri runtime.
fn detected_browsers(os: Os) -> Vec<DetectedBrowser> {
    Browser::ALL
        .into_iter()
        .filter(|browser| browser.available_on(os))
        .map(|browser| DetectedBrowser {
            id: browser,
            name: browser.display_name().to_owned(),
            family: browser.family(),
            permission_hint: browser.permission_hint(os).map(str::to_owned),
            settings_deep_link: browser.settings_deep_link(os).map(str::to_owned),
        })
        .collect()
}

/// The OS this binary is running on, in the domain crate's terms.
const fn current_os() -> Os {
    #[cfg(target_os = "macos")]
    {
        Os::MacOs
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        Os::Linux
    }
    #[cfg(not(unix))]
    {
        Os::Other
    }
}

/// The whole import flow, isolated from Tauri so every branch is unit-tested:
/// availability → read → extract → persist → validate. A key rejected by
/// claude.ai is rolled back: the previously stored key is restored (or the
/// store cleared if there was none), so a failed import never destroys a
/// working session and nothing invalid lingers.
async fn import_impl(
    reader: &dyn BrowserCookieReader,
    validator: &impl SessionValidator,
    store: &dyn SessionStore,
    os: Os,
    browser: Browser,
) -> Result<ImportSummary, BrowserImportError> {
    if !browser.available_on(os) {
        return Err(BrowserImportError::Unsupported(format!(
            "{} isn't available on this platform.",
            browser.display_name()
        )));
    }

    let cookies = reader
        .read_claude_cookies(browser)
        .map_err(|error| BrowserImportError::CookieStore(error.into_message()))?;
    let key = session_key_from_cookies(&cookies).map_err(|error| match error {
        CookieImportError::NoSessionCookie => BrowserImportError::NoSession(format!(
            "No claude.ai session was found in {}. Sign in to claude.ai there first.",
            browser.display_name()
        )),
        CookieImportError::InvalidSessionCookie(inner) => {
            BrowserImportError::Invalid(inner.to_string())
        }
    })?;
    // Drop every other claude.ai cookie the moment the key is in hand.
    drop(cookies);

    // Hold on to whatever key was stored before, so a rejected import can put
    // it back instead of destroying a working session. Best-effort: if the
    // store can't be read, there is nothing to restore.
    let previous = store.load().unwrap_or_default();

    store
        .save(&key)
        .map_err(|error| BrowserImportError::Store(error.to_string()))?;

    let display_name = browser.display_name().to_owned();
    match validator.validate(&key).await {
        Ok(()) => Ok(ImportSummary {
            browser: display_name,
            validated: true,
        }),
        Err(ValidationError::Unauthorized) => {
            // Best-effort: don't leave a rejected key behind — restore the
            // key that was working before, or clear if there was none.
            let _ = previous
                .as_ref()
                .map_or_else(|| store.clear(), |previous_key| store.save(previous_key));
            Err(BrowserImportError::Rejected(format!(
                "claude.ai rejected the session imported from {display_name} — it may have \
                 expired. Sign in again there and retry."
            )))
        }
        // Network hiccup: keep the key; the scheduler validates on its next
        // poll once it can reach claude.ai.
        Err(ValidationError::Transient) => Ok(ImportSummary {
            browser: display_name,
            validated: false,
        }),
    }
}

/// List the browsers the user can import a session from on this platform,
/// with the permission story each implies.
#[tauri::command]
pub fn list_browser_sessions() -> Vec<DetectedBrowser> {
    detected_browsers(current_os())
}

/// Import the claude.ai session from `browser`: read it, persist it, validate
/// it, and wake the polling loop so the new key takes effect immediately.
#[tauri::command]
pub async fn import_browser_session(
    state: State<'_, SessionStoreState>,
    scheduler: State<'_, SchedulerHandle>,
    browser: Browser,
) -> Result<ImportSummary, BrowserImportError> {
    // Extract owned handles before the first `await`: the `State` guards are
    // not `Send`, so holding them across the await would make the command's
    // future non-`Send`, which Tauri requires.
    let store = Arc::clone(&state.0);
    let scheduler = (*scheduler).clone();

    let summary = import_impl(
        &RookieCookieReader,
        &LiveSessionValidator::new(),
        store.as_ref(),
        current_os(),
        browser,
    )
    .await?;

    scheduler.resume_polling();
    Ok(summary)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use crate::cookie_reader::CookieStoreError;
    use crate::store::FakeSessionStore;
    use meter_core::BrowserCookie;
    use pretty_assertions::assert_eq;

    const VALID: &str = "sk-ant-sid01-abcDEF123456_-xyz789";

    /// A reader returning a canned result regardless of which browser is asked.
    struct FakeReader(Result<Vec<BrowserCookie>, CookieStoreError>);

    impl FakeReader {
        fn with_session() -> Self {
            Self(Ok(vec![
                BrowserCookie::new(".claude.ai", "ajs_anonymous_id", "noise"),
                BrowserCookie::new(".claude.ai", "sessionKey", VALID),
            ]))
        }

        fn empty() -> Self {
            Self(Ok(vec![BrowserCookie::new(".claude.ai", "other", "noise")]))
        }

        fn locked() -> Self {
            Self(Err(CookieStoreError::new("keyring is locked")))
        }
    }

    impl BrowserCookieReader for FakeReader {
        fn read_claude_cookies(
            &self,
            _browser: Browser,
        ) -> Result<Vec<BrowserCookie>, CookieStoreError> {
            self.0.clone()
        }
    }

    /// A validator returning a fixed verdict without any network.
    struct FakeValidator(Result<(), ValidationError>);

    impl SessionValidator for FakeValidator {
        async fn validate<'a>(&'a self, _key: &'a SessionKey) -> Result<(), ValidationError> {
            self.0
        }
    }

    fn ok_validator() -> FakeValidator {
        FakeValidator(Ok(()))
    }

    #[tokio::test]
    async fn successful_import_stores_and_reports_validated() {
        let store = FakeSessionStore::new();
        let summary = import_impl(
            &FakeReader::with_session(),
            &ok_validator(),
            &store,
            Os::MacOs,
            Browser::Chrome,
        )
        .await
        .unwrap();

        assert_eq!(
            summary,
            ImportSummary {
                browser: "Google Chrome".to_owned(),
                validated: true,
            }
        );
        assert_eq!(store.load().unwrap().unwrap().expose(), VALID);
    }

    #[tokio::test]
    async fn unsupported_browser_never_touches_the_store() {
        let store = FakeSessionStore::new();
        let error = import_impl(
            &FakeReader::with_session(),
            &ok_validator(),
            &store,
            Os::Linux,
            Browser::Safari,
        )
        .await
        .unwrap_err();

        assert!(matches!(error, BrowserImportError::Unsupported(_)));
        assert_eq!(store.load().unwrap(), None);
    }

    #[tokio::test]
    async fn a_locked_store_degrades_to_a_cookie_store_error() {
        let store = FakeSessionStore::new();
        let error = import_impl(
            &FakeReader::locked(),
            &ok_validator(),
            &store,
            Os::MacOs,
            Browser::Brave,
        )
        .await
        .unwrap_err();

        assert!(matches!(error, BrowserImportError::CookieStore(_)));
        assert_eq!(store.load().unwrap(), None);
    }

    #[tokio::test]
    async fn no_session_cookie_reports_no_session() {
        let store = FakeSessionStore::new();
        let error = import_impl(
            &FakeReader::empty(),
            &ok_validator(),
            &store,
            Os::MacOs,
            Browser::Firefox,
        )
        .await
        .unwrap_err();

        assert!(matches!(error, BrowserImportError::NoSession(_)));
        assert_eq!(store.load().unwrap(), None);
    }

    #[tokio::test]
    async fn a_malformed_cookie_reports_invalid_without_echoing_it() {
        let reader = FakeReader(Ok(vec![BrowserCookie::new(
            ".claude.ai",
            "sessionKey",
            "sk-ant-sid01-bad value",
        )]));
        let store = FakeSessionStore::new();
        let error = import_impl(&reader, &ok_validator(), &store, Os::MacOs, Browser::Chrome)
            .await
            .unwrap_err();

        assert!(matches!(error, BrowserImportError::Invalid(_)));
        assert!(!format!("{error:?}").contains("bad value"));
        assert_eq!(store.load().unwrap(), None);
    }

    #[tokio::test]
    async fn a_rejected_key_with_no_previous_key_clears_the_store() {
        let store = FakeSessionStore::new();
        let error = import_impl(
            &FakeReader::with_session(),
            &FakeValidator(Err(ValidationError::Unauthorized)),
            &store,
            Os::MacOs,
            Browser::Chrome,
        )
        .await
        .unwrap_err();

        assert!(matches!(error, BrowserImportError::Rejected(_)));
        // The rejected key must not linger.
        assert_eq!(store.load().unwrap(), None);
    }

    #[tokio::test]
    async fn a_rejected_key_restores_the_previously_stored_key() {
        let previous = SessionKey::parse("sk-ant-sid01-previousKEY_123-456789").unwrap();
        let store = FakeSessionStore::with_key(previous.clone());
        let error = import_impl(
            &FakeReader::with_session(),
            &FakeValidator(Err(ValidationError::Unauthorized)),
            &store,
            Os::MacOs,
            Browser::Chrome,
        )
        .await
        .unwrap_err();

        assert!(matches!(error, BrowserImportError::Rejected(_)));
        // The working key from before the import must survive the rejection —
        // the failed import must not destroy it.
        assert_eq!(store.load().unwrap(), Some(previous));
    }

    #[tokio::test]
    async fn a_transient_validation_failure_keeps_the_key_unvalidated() {
        let store = FakeSessionStore::new();
        let summary = import_impl(
            &FakeReader::with_session(),
            &FakeValidator(Err(ValidationError::Transient)),
            &store,
            Os::MacOs,
            Browser::Chrome,
        )
        .await
        .unwrap();

        assert!(!summary.validated);
        // Kept, because the failure might be a network blip, not a bad key.
        assert_eq!(store.load().unwrap().unwrap().expose(), VALID);
    }

    #[tokio::test]
    async fn a_store_failure_surfaces_before_validation() {
        let store = FakeSessionStore::unavailable();
        let error = import_impl(
            &FakeReader::with_session(),
            &ok_validator(),
            &store,
            Os::MacOs,
            Browser::Chrome,
        )
        .await
        .unwrap_err();

        assert!(matches!(error, BrowserImportError::Store(_)));
    }

    #[test]
    fn linux_omits_safari_and_arc_but_keeps_the_rest() {
        let linux = detected_browsers(Os::Linux);
        assert!(!linux.iter().any(|browser| browser.id == Browser::Safari));
        // Arc has never shipped a Linux build, so offering it could only fail.
        assert!(!linux.iter().any(|browser| browser.id == Browser::Arc));
        assert!(linux.iter().any(|browser| browser.id == Browser::Chrome));
        assert!(linux.iter().any(|browser| browser.id == Browser::Firefox));
    }

    #[test]
    fn macos_includes_safari_with_a_full_disk_access_deep_link() {
        let macos = detected_browsers(Os::MacOs);
        let safari = macos
            .iter()
            .find(|browser| browser.id == Browser::Safari)
            .unwrap();
        assert_eq!(
            safari.settings_deep_link.as_deref(),
            Some(meter_core::FULL_DISK_ACCESS_SETTINGS_URL)
        );
        let chrome = macos
            .iter()
            .find(|browser| browser.id == Browser::Chrome)
            .unwrap();
        assert!(chrome.permission_hint.is_some());
    }

    #[test]
    fn detected_browser_serializes_id_as_a_snake_case_string() {
        let detected = detected_browsers(Os::MacOs);
        let json = serde_json::to_value(&detected[0]).unwrap();
        assert_eq!(json["id"], "chrome");
        assert_eq!(json["name"], "Google Chrome");
    }

    #[test]
    fn import_error_serializes_with_a_discriminant_tag() {
        let error = BrowserImportError::NoSession("nothing here".to_owned());
        let json = serde_json::to_value(&error).unwrap();
        assert_eq!(json["kind"], "NoSession");
        assert_eq!(json["message"], "nothing here");
    }
}

#[cfg(test)]
mod validator_tests {
    //! The production [`LiveSessionValidator`] driven against a local wiremock
    //! server — real `UsageClient` requests over loopback, no live claude.ai.
    #![allow(clippy::unwrap_used)]

    use super::*;
    use pretty_assertions::assert_eq;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn key() -> SessionKey {
        SessionKey::parse("sk-ant-sid01-abcDEF123456_-xyz789").unwrap()
    }

    #[tokio::test]
    async fn a_200_from_organizations_validates_the_key() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/organizations"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!([{ "uuid": "org-1", "name": "Acme" }])),
            )
            .mount(&server)
            .await;

        let validator = LiveSessionValidator::with_base_url(server.uri());
        assert_eq!(validator.validate(&key()).await, Ok(()));
    }

    #[tokio::test]
    async fn a_401_is_reported_as_unauthorized() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/organizations"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;

        let validator = LiveSessionValidator::with_base_url(server.uri());
        assert_eq!(
            validator.validate(&key()).await,
            Err(ValidationError::Unauthorized)
        );
    }

    #[tokio::test]
    async fn a_server_error_is_transient() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/organizations"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;

        let validator = LiveSessionValidator::with_base_url(server.uri());
        assert_eq!(
            validator.validate(&key()).await,
            Err(ValidationError::Transient)
        );
    }
}
