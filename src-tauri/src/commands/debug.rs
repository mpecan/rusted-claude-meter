//! Debug logging commands: the "Log API responses" toggle and the log-file
//! path/reveal helpers (see `crate::debug_log`).
//!
//! Split out of the main `commands` module purely to keep that file under the
//! workspace's file-size gate. `set_debug_logging` behaves like the other
//! settings commands there, plus it flips the live `ResponseLog` sink so the
//! change takes effect on the very next poll — nothing re-reads `AppSettings`
//! on a scheduler tick to notice it otherwise.

use std::sync::Arc;

use tauri::State;

use crate::debug_log::ResponseLog;
use crate::settings::{AppSettings, SettingsState};

/// Turn raw-API-response logging on or off (Settings' "Log API responses").
/// Flips the live [`ResponseLog`] sink so the change takes effect on the very
/// next poll, then persists it so it survives a restart. Captured payloads are
/// how the token/cost `spend` shape was pinned down, and the way to verify it
/// against account types not yet observed; the response body holds only usage
/// data, never the session key.
#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
pub fn set_debug_logging(
    settings: State<'_, SettingsState>,
    response_log: State<'_, Arc<ResponseLog>>,
    enabled: bool,
) -> AppSettings {
    response_log.set_enabled(enabled);
    store_debug_logging(&settings, enabled)
}

/// Persist the debug-logging toggle. Split from the command so the settings
/// mutation is unit-testable without a Tauri `AppHandle` or managed state.
fn store_debug_logging(settings: &SettingsState, enabled: bool) -> AppSettings {
    settings.update(|s| s.debug_logging = enabled)
}

/// The absolute path of the API-response log, shown in Settings so the user can
/// find the captured payloads. `None` when no log directory was resolvable at
/// startup (logging is then a no-op).
#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
pub fn debug_log_path(response_log: State<'_, Arc<ResponseLog>>) -> Option<String> {
    response_log.path().map(|path| path.display().to_string())
}

/// Reveal the API-response log in the OS file manager (Finder / Files /
/// Explorer). Reveals the file when it exists, otherwise opens the folder it
/// will appear in (nothing has been logged yet). A no-op when no path is known;
/// an opener failure surfaces as a string the Settings page can show.
#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
pub fn reveal_debug_log(
    app: tauri::AppHandle,
    response_log: State<'_, Arc<ResponseLog>>,
) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt as _;
    let Some(path) = response_log.path() else {
        return Ok(());
    };
    let opener = app.opener();
    let result = if path.exists() {
        opener.reveal_item_in_dir(path)
    } else if let Some(parent) = path.parent() {
        opener.open_path(parent.to_string_lossy(), None::<&str>)
    } else {
        return Ok(());
    };
    result.map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn debug_logging_toggle_persists_both_ways() {
        let state = SettingsState::new(None, AppSettings::default());
        // Default is off; toggling on then off round-trips through the store.
        // (The command also flips the live `ResponseLog` sink, whose enable/
        // disable behaviour is covered in `crate::debug_log`.)
        assert!(store_debug_logging(&state, true).debug_logging);
        assert!(!store_debug_logging(&state, false).debug_logging);
    }
}
