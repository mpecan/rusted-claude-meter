//! Picking a transport per tick from the user's [`UsageSource`] choice.
//!
//! [`StatuslineTransport`] reads what Claude Code recorded;
//! [`SourcedTransport`] holds it alongside the live claude.ai one and asks the
//! shared [`SourceSelection`] which to use, every tick. Reading the selection
//! per tick rather than choosing a transport once at startup is what lets the
//! Settings picker take effect immediately — the polling loop is generic over
//! one transport and is never rebuilt.
//!
//! **Consent lives on the claude.ai side only.** `LiveTransport::attempt`
//! checks the gate itself, so routing to the status line here bypasses it by
//! construction rather than by an added exception: there is no claude.ai
//! request in that branch to consent to. That direction matters — a wiring
//! mistake sends traffic through the transport that *does* check, not the one
//! that does not.

use std::path::PathBuf;
use std::sync::Arc;

use crate::scheduler::core::FetchOutcome;
use crate::scheduler::transport::{LiveTransport, UsageTransport};
use crate::source::SourceSelection;
use crate::statusline;

/// Reads `~/.claudemeter/statusline.json` — see [`crate::statusline`].
pub struct StatuslineTransport {
    /// `None` when the home directory could not be resolved at startup, which
    /// is indistinguishable to the user from "nothing recorded yet" and is
    /// reported the same way.
    pub path: Option<PathBuf>,
}

impl StatuslineTransport {
    /// Every failure — absent file, corrupt file, no windows in it — reports
    /// [`FetchOutcome::AwaitingStatusline`] rather than `Transient`. Backing
    /// off would be wrong: this is not a flaky dependency, it is a file that
    /// appears once the user has Claude Code running, and the remedy is the
    /// same in every case.
    fn attempt(&self) -> FetchOutcome {
        self.path
            .as_deref()
            .and_then(statusline::read)
            .map_or(FetchOutcome::AwaitingStatusline, FetchOutcome::Success)
    }
}

impl UsageTransport for StatuslineTransport {
    fn fetch(&self) -> impl Future<Output = FetchOutcome> + Send {
        // Read inline rather than through the blocking pool: this is a small
        // local file, not the credential store's daemon round trip.
        std::future::ready(self.attempt())
    }
}

/// The transport the app actually runs: both sources, chosen per tick.
///
/// Built with a struct literal rather than a constructor: three same-shaped
/// arguments are easy to transpose positionally, and the field names carry
/// the meaning.
pub struct SourcedTransport {
    pub live: LiveTransport,
    pub statusline: StatuslineTransport,
    pub selection: Arc<SourceSelection>,
}

impl UsageTransport for SourcedTransport {
    async fn fetch(&self) -> FetchOutcome {
        if self.selection.get() {
            self.statusline.fetch().await
        } else {
            self.live.fetch().await
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use crate::scheduler::test_support::consenting;
    use crate::source::{UsageSource, selection};
    use crate::store::FakeSessionStore;
    use meter_core::{LimitWindow, UsageSnapshot, UsageWindow};
    use pretty_assertions::assert_eq;
    use std::path::Path;

    fn snapshot() -> UsageSnapshot {
        UsageSnapshot {
            five_hour: Some(UsageWindow {
                utilization: 37.4,
                resets_at: "2026-08-02T15:00:00Z".parse().unwrap(),
                window: LimitWindow::FiveHour,
            }),
            seven_day: None,
            scoped: Vec::new(),
            spend: None,
            fetched_at: "2026-08-02T12:00:00Z".parse().unwrap(),
        }
    }

    fn recorded(dir: &tempfile::TempDir) -> PathBuf {
        let path = statusline::statusline_path(dir.path());
        statusline::record(&path, &snapshot()).unwrap();
        path
    }

    #[tokio::test]
    async fn a_recorded_reading_is_reported_as_a_success() {
        let dir = tempfile::tempdir().unwrap();
        let transport = StatuslineTransport {
            path: Some(recorded(&dir)),
        };
        assert_eq!(transport.fetch().await, FetchOutcome::Success(snapshot()));
    }

    #[tokio::test]
    async fn an_absent_file_waits_rather_than_backing_off() {
        let dir = tempfile::tempdir().unwrap();
        let transport = StatuslineTransport {
            path: Some(statusline::statusline_path(dir.path())),
        };
        assert_eq!(transport.fetch().await, FetchOutcome::AwaitingStatusline);
    }

    #[tokio::test]
    async fn an_unresolvable_home_directory_waits_too() {
        assert_eq!(
            StatuslineTransport { path: None }.fetch().await,
            FetchOutcome::AwaitingStatusline
        );
    }

    /// A claude.ai transport whose outcome is unmistakable: the store is
    /// empty, so taking this branch always yields `NoSession` and never a
    /// `Success`. That lets the dispatch tests prove *which* branch ran
    /// rather than merely that the result looked plausible. Its consent gate
    /// is open, so a `NoSession` cannot be mistaken for the gate refusing.
    fn unusable_live() -> LiveTransport {
        consenting(Arc::new(FakeSessionStore::new()), "http://127.0.0.1:1")
    }

    #[tokio::test]
    async fn the_statusline_source_reads_the_file_and_never_reaches_claude_ai() {
        let dir = tempfile::tempdir().unwrap();
        let transport = SourcedTransport {
            live: unusable_live(),
            statusline: StatuslineTransport {
                path: Some(recorded(&dir)),
            },
            selection: Arc::new(selection(UsageSource::ClaudeCodeStatusline)),
        };
        assert_eq!(transport.fetch().await, FetchOutcome::Success(snapshot()));
    }

    #[tokio::test]
    async fn the_claude_ai_source_ignores_a_perfectly_good_recorded_file() {
        let dir = tempfile::tempdir().unwrap();
        let transport = SourcedTransport {
            live: unusable_live(),
            statusline: StatuslineTransport {
                path: Some(recorded(&dir)),
            },
            selection: Arc::new(selection(UsageSource::ClaudeAi)),
        };
        assert_eq!(transport.fetch().await, FetchOutcome::NoSession);
    }

    /// The selection is read per tick, so the Settings picker takes effect on
    /// the next poll without the loop being rebuilt.
    #[tokio::test]
    async fn flipping_the_selection_changes_source_on_the_very_next_tick() {
        let dir = tempfile::tempdir().unwrap();
        let live_selection = Arc::new(selection(UsageSource::ClaudeAi));
        let transport = SourcedTransport {
            live: unusable_live(),
            statusline: StatuslineTransport {
                path: Some(recorded(&dir)),
            },
            selection: Arc::clone(&live_selection),
        };
        assert_eq!(transport.fetch().await, FetchOutcome::NoSession);
        live_selection.set(true);
        assert_eq!(transport.fetch().await, FetchOutcome::Success(snapshot()));
        live_selection.set(false);
        assert_eq!(transport.fetch().await, FetchOutcome::NoSession);
    }

    #[test]
    fn the_transport_reads_the_path_beside_the_usage_export() {
        assert_eq!(
            statusline::statusline_path(Path::new("/home/example")),
            PathBuf::from("/home/example/.claudemeter/statusline.json")
        );
    }
}
