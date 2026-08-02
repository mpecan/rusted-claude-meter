//! Where usage numbers come from — the user's choice between polling
//! claude.ai and reading what Claude Code reports.
//!
//! The two sources are genuinely different products, not a fallback chain,
//! which is why this is an explicit setting rather than "try one, then the
//! other". Blending them would make the tray untrustworthy: you could never
//! tell whether a number was seconds old from Claude Code or minutes old from
//! claude.ai, and the two do not even cover the same limits.
//!
//! | | [`UsageSource::ClaudeAi`] | [`UsageSource::ClaudeCodeStatusline`] |
//! |---|---|---|
//! | Terms of Service | needs [`crate::consent`] | nothing to consent to |
//! | Credential | claude.ai session cookie | none |
//! | Model-scoped limits | yes | **no** |
//! | Spend / cost view | yes | no |
//! | Updates | every refresh interval | only while Claude Code runs |
//!
//! The second column is why this exists: a user who declines the
//! Terms-of-Service risk used to get a permanently dead meter. Now they get a
//! working one, with less in it.

use serde::{Deserialize, Serialize};

use crate::sync::AtomicFlag;

/// Which source the scheduler fetches from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UsageSource {
    /// Poll claude.ai directly. Complete, and the only source for scoped
    /// models and spend — but gated on the Terms-of-Service acknowledgement.
    #[default]
    ClaudeAi,
    /// Read `~/.claudemeter/statusline.json`, written by Claude Code through
    /// the `rusted-claude-meter statusline` bridge. See [`crate::statusline`].
    ClaudeCodeStatusline,
}

impl UsageSource {
    /// Whether this source reads the recorded status-line file rather than
    /// making a request. Also the answer to "may this source run without a
    /// Terms-of-Service acknowledgement?" — the two coincide because the file
    /// is the only source that originates no claude.ai traffic.
    #[must_use]
    pub const fn is_statusline(self) -> bool {
        matches!(self, Self::ClaudeCodeStatusline)
    }
}

/// What every sign-in path returns while the Claude Code status line is the
/// source. Defined once so the pasted-key path and the browser-import path
/// cannot drift into telling the user two different stories — the same
/// discipline as `commands::consent::WITHHELD_MESSAGE`.
pub const WRONG_SOURCE_MESSAGE: &str = "Rusted Claude Meter is reading usage from Claude Code, so it needs no session key and \
     contacts claude.ai on no path at all. Switch Usage source to \"Poll claude.ai\" in \
     Settings first if you want to poll directly.";

/// Runtime mirror of [`UsageSource`], shared between the Settings command that
/// flips it and the transport that reads it on every tick — the same
/// arrangement as [`crate::consent::ConsentGate`].
///
/// A bool because there are exactly two sources and [`AtomicFlag`] already
/// exists; set means [`UsageSource::ClaudeCodeStatusline`]. A third source
/// must widen this rather than add a second flag beside it, or the two could
/// disagree about which one is live.
pub type SourceSelection = AtomicFlag;

/// Seed a selection from the persisted setting.
#[must_use]
pub const fn selection(source: UsageSource) -> SourceSelection {
    SourceSelection::new(source.is_statusline())
}

/// Read a selection back as the enum it mirrors. Test-only: production reads
/// the source from persisted settings, and the flag exists purely so the
/// transport can branch without locking anything.
#[cfg(test)]
#[must_use]
pub fn selected(selection: &SourceSelection) -> UsageSource {
    if selection.get() {
        UsageSource::ClaudeCodeStatusline
    } else {
        UsageSource::ClaudeAi
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use pretty_assertions::assert_eq;

    /// A fresh install polls claude.ai, which is what every install did
    /// before this setting existed — the new source is opt-in.
    #[test]
    fn claude_ai_is_the_default_source() {
        assert_eq!(UsageSource::default(), UsageSource::ClaudeAi);
    }

    #[test]
    fn only_the_statusline_source_skips_the_consent_gate() {
        assert!(!UsageSource::ClaudeAi.is_statusline());
        assert!(UsageSource::ClaudeCodeStatusline.is_statusline());
    }

    #[test]
    fn a_selection_round_trips_through_the_shared_flag() {
        for source in [UsageSource::ClaudeAi, UsageSource::ClaudeCodeStatusline] {
            assert_eq!(selected(&selection(source)), source);
        }
    }

    #[test]
    fn a_selection_can_be_flipped_after_it_is_built() {
        let live = selection(UsageSource::ClaudeAi);
        live.set(true);
        assert_eq!(selected(&live), UsageSource::ClaudeCodeStatusline);
    }

    /// The persisted spelling is part of the settings file contract.
    #[test]
    fn the_persisted_spelling_is_snake_case() {
        assert_eq!(
            serde_json::to_value(UsageSource::ClaudeCodeStatusline).unwrap(),
            serde_json::json!("claude_code_statusline")
        );
    }
}
