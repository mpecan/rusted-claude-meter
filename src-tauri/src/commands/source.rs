//! Choosing where usage numbers come from, and helping the user wire up the
//! Claude Code side of it.
//!
//! Split out of the main `commands` module for the file-size gate, and
//! because — like `commands::consent` — this is not an ordinary preference:
//! it flips the live [`SourceSelection`] *and* wakes the scheduler in the same
//! call, so switching source takes effect immediately rather than at some
//! point within the next refresh interval.

use std::sync::Arc;

use tauri::State;

use crate::scheduler::SchedulerHandle;
use crate::settings::{AppSettings, SettingsState};
use crate::source::{SourceSelection, UsageSource};
use crate::statusline;

/// Switch the scheduler between claude.ai and the Claude Code status line.
///
/// Ordering mirrors `commands::consent::set_tos_acknowledged`: the live
/// selection moves *before* the scheduler is woken, so a tick racing this
/// call cannot read the old source and fetch from somewhere the user just
/// left.
///
/// The wake is unconditional. Switching *to* the status line has to re-read a
/// file that may already be there, and switching *to* claude.ai has to leave
/// whatever phase the status-line source parked in — including the case where
/// the next tick immediately re-parks on a withheld consent gate, which costs
/// one tick and no traffic (`LiveTransport` checks consent before anything).
#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
pub fn set_usage_source(
    settings: State<'_, SettingsState>,
    selection: State<'_, Arc<SourceSelection>>,
    scheduler: State<'_, SchedulerHandle>,
    source: UsageSource,
) -> AppSettings {
    selection.set(source.is_statusline());
    scheduler.resume_polling();
    store_usage_source(&settings, source)
}

/// Persist the choice. Split from the command so the settings mutation is
/// unit-testable without a Tauri `AppHandle` or managed state, matching
/// `commands::consent::store_tos_acknowledged`.
fn store_usage_source(settings: &SettingsState, source: UsageSource) -> AppSettings {
    settings.update(|s| s.usage_source = source)
}

/// A ready-to-paste `statusLine.command` for this exact install.
///
/// Generated rather than documented because the path is not guessable: on
/// macOS the binary lives inside the app bundle, at a path containing spaces.
///
/// The shape is deliberately the *component* one — capture stdin once, pipe a
/// copy through the bridge, keep the result in `$meter` — so a user who
/// already has a status line replaces only the final `printf` with their own
/// and drops `$meter` into it. Claude Code hands the same blob to one command
/// and one command only, so composing is the normal case, not the advanced
/// one.
#[must_use]
#[tauri::command]
pub fn statusline_command() -> String {
    // Falling back to the bare name is right where it works — a packaged
    // Linux install puts the binary on `PATH` — and is at least a legible
    // placeholder where it does not.
    let exe = std::env::current_exe().map_or_else(
        |_| FALLBACK_EXE.to_owned(),
        |path| path.display().to_string(),
    );
    format!(
        "input=$(cat); meter=$(printf '%s' \"$input\" | {} {}); printf '%s' \"$meter\"",
        shell_quote(&exe),
        statusline::SUBCOMMAND
    )
}

/// Used only when the running executable's own path cannot be resolved.
const FALLBACK_EXE: &str = "rusted-claude-meter";

/// Single-quote `value` for POSIX `sh`, so a path containing spaces — every
/// macOS install — survives being pasted into a shell command. An embedded
/// single quote is closed, escaped and reopened, the standard idiom.
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r"'\''"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{AppSettings, SettingsState};
    use pretty_assertions::assert_eq;

    fn state() -> SettingsState {
        SettingsState::new(None, AppSettings::default())
    }

    #[test]
    fn the_source_round_trips_through_settings() {
        let settings = state();
        assert_eq!(
            store_usage_source(&settings, UsageSource::ClaudeCodeStatusline).usage_source,
            UsageSource::ClaudeCodeStatusline
        );
        assert_eq!(
            settings.get().usage_source,
            UsageSource::ClaudeCodeStatusline
        );
    }

    #[test]
    fn the_source_can_be_switched_back() {
        let settings = state();
        store_usage_source(&settings, UsageSource::ClaudeCodeStatusline);
        assert_eq!(
            store_usage_source(&settings, UsageSource::ClaudeAi).usage_source,
            UsageSource::ClaudeAi
        );
    }

    /// Switching source must not disturb the Terms-of-Service answer: a user
    /// who tries the status-line source and switches back must not find they
    /// have silently re-accepted the claude.ai risk.
    #[test]
    fn switching_source_leaves_the_consent_answer_alone() {
        let settings = state();
        let before = settings.get();
        let after = store_usage_source(&settings, UsageSource::ClaudeCodeStatusline);
        assert_eq!(after.tos_acknowledged, before.tos_acknowledged);
        assert_eq!(after.refresh_interval, before.refresh_interval);
        assert_eq!(after.shown_scoped_models, before.shown_scoped_models);
    }

    #[test]
    fn the_generated_command_pipes_stdin_through_the_bridge() {
        let command = statusline_command();
        assert!(command.starts_with("input=$(cat);"), "{command}");
        assert!(command.contains("statusline"), "{command}");
        assert!(command.contains("$meter"), "{command}");
    }

    /// Every macOS install has spaces in its path ("Rusted Claude
    /// Meter.app"), so an unquoted path would be a broken paste for the
    /// majority of users.
    #[test]
    fn a_path_with_spaces_is_quoted() {
        assert_eq!(
            shell_quote("/Applications/Rusted Claude Meter.app/Contents/MacOS/rcm"),
            "'/Applications/Rusted Claude Meter.app/Contents/MacOS/rcm'"
        );
    }

    #[test]
    fn an_embedded_single_quote_is_escaped_rather_than_ending_the_string() {
        assert_eq!(shell_quote("/home/o'brien/rcm"), r"'/home/o'\''brien/rcm'");
    }
}
