//! Browser session-import commands: list the importable browsers, and import
//! the claude.ai session from one of them.
//!
//! Split out of `crate::browser_import` (which keeps the cookie-reading and
//! validation logic they call) for the same reason `commands::debug` was split
//! out of `commands`: the workspace file-size gate. Putting them here also
//! groups every `#[tauri::command]` that can reach claude.ai in one place, and
//! that is the set the Terms-of-Service consent gate has to cover — see `crate::consent`.

use std::sync::Arc;

use meter_core::Browser;
use tauri::State;

use crate::browser_import::{BrowserImportError, DetectedBrowser, ImportSummary};
use crate::commands::SessionStoreState;
use crate::consent::ConsentGate;
use crate::scheduler::SchedulerHandle;

#[cfg(feature = "browser-import")]
use crate::browser_import::{LiveSessionValidator, current_os, detected_browsers, import_impl};
#[cfg(feature = "browser-import")]
use crate::cookie_reader::RookieCookieReader;
#[cfg(feature = "browser-import")]
use crate::signin::SessionSink;

/// List the browsers the user can import a session from on this platform,
/// with the permission story each implies. Empty in the "lite" build, where
/// automated import is compiled out and only manual session-key paste remains.
///
/// Deliberately *not* consent-gated: this reads nothing and contacts nobody,
/// it only describes what the platform offers, so the Settings and wizard
/// screens can render the (disabled) import list alongside the warning.
#[tauri::command]
#[cfg_attr(not(feature = "browser-import"), allow(clippy::missing_const_for_fn))]
pub fn list_browser_sessions() -> Vec<DetectedBrowser> {
    #[cfg(feature = "browser-import")]
    {
        detected_browsers(current_os())
    }
    #[cfg(not(feature = "browser-import"))]
    {
        Vec::new()
    }
}

/// Import the claude.ai session from `browser`: read it, persist it, validate
/// it, and wake the polling loop so the new key takes effect immediately.
/// Reports "unavailable" in the "lite" build (no cookie-store access).
///
/// Refused outright while Terms-of-Service consent is withheld — validation is a request to
/// claude.ai, and the refusal happens before the cookie store is read, so a
/// user who has not accepted the risk gets neither claude.ai traffic nor a
/// keychain prompt out of this app.
#[tauri::command]
#[cfg_attr(not(feature = "browser-import"), allow(clippy::unused_async))]
pub async fn import_browser_session(
    state: State<'_, SessionStoreState>,
    scheduler: State<'_, SchedulerHandle>,
    consent: State<'_, Arc<ConsentGate>>,
    browser: Browser,
) -> Result<ImportSummary, BrowserImportError> {
    #[cfg(feature = "browser-import")]
    {
        // Extract owned handles before the first `await`: the `State` guards
        // are not `Send`, so holding them across the await would make the
        // command's future non-`Send`, which Tauri requires.
        let store = Arc::clone(&state.0);
        let scheduler = (*scheduler).clone();
        let consent = Arc::clone(&consent);

        let summary = import_impl(
            RookieCookieReader,
            &SessionSink::new(&store, &LiveSessionValidator::new(), &consent),
            current_os(),
            browser,
        )
        .await?;

        scheduler.resume_polling();
        Ok(summary)
    }
    #[cfg(not(feature = "browser-import"))]
    {
        // Browser import is compiled out in this build; manual session-key
        // paste is the way in.
        let _ = (state, scheduler, consent, browser);
        Err(BrowserImportError::Unsupported(
            "Browser import isn't available in this build — paste your session key instead."
                .to_owned(),
        ))
    }
}
