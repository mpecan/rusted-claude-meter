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

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::sync::AtomicFlag;

/// How often the recorded status-line file is re-read.
///
/// Fixed, and deliberately not the user's `RefreshInterval`: that setting
/// paces *requests to claude.ai*, and there are none here — this is a
/// sub-kilobyte local file the bridge rewrites on every Claude Code render.
/// Pacing a file read at five minutes would leave the tray five minutes
/// behind data that is seconds old.
const STATUSLINE_POLL: Duration = Duration::from_secs(15);

/// Which source the scheduler fetches from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UsageSource {
    /// Poll claude.ai directly. Complete, and the only source for scoped
    /// models and spend — but gated on the Terms-of-Service acknowledgement.
    #[default]
    ClaudeAi,
    /// Read `~/.claudemeter/statusline.json`, written by Claude Code through
    /// the `rusted-claude-meter-statusline` bridge. See [`meter_files::statusline`].
    ClaudeCodeStatusline,
}

impl UsageSource {
    /// Which variant this is, as the bool the runtime mirror stores.
    ///
    /// **An encoding detail, not a policy question.** Every "what does this
    /// source imply?" decision below is its own exhaustive `match`, so adding
    /// a third source is a compile error at each one rather than a silent
    /// inheritance of claude.ai's answers. That distinction is the whole
    /// reason these methods exist instead of `if is_statusline()`.
    const fn is_statusline(self) -> bool {
        matches!(self, Self::ClaudeCodeStatusline)
    }

    /// The cadence this source is polled at, or `None` to use the user's
    /// `RefreshInterval`.
    ///
    /// A source that costs a network request honours the user's setting; one
    /// that costs a local file read has no reason to, and every reason not to
    /// — the interval exists to be polite to claude.ai.
    #[must_use]
    pub const fn fixed_poll_interval(self) -> Option<Duration> {
        match self {
            Self::ClaudeAi => None,
            Self::ClaudeCodeStatusline => Some(STATUSLINE_POLL),
        }
    }

    /// Whether using this source means making requests to claude.ai.
    ///
    /// The single fact three rules follow from, so they cannot drift apart:
    /// the Terms-of-Service gate exists to permit those requests
    /// (`crate::consent`), a session key exists to authenticate them
    /// (`crate::signin`), and the forced-refresh memory TTL exists to stop a
    /// burst of them. A source that makes none is exempt from all three for
    /// the same reason, and a fourth rule about claude.ai traffic should ask
    /// this rather than grow its own predicate.
    ///
    /// Cadence is deliberately *not* derived from this — see
    /// [`Self::fixed_poll_interval`] — because "how expensive is a fetch" and
    /// "does it leave this machine" are genuinely different questions, and a
    /// third source could easily answer them differently.
    #[must_use]
    pub const fn reaches_claude_ai(self) -> bool {
        match self {
            Self::ClaudeAi => true,
            Self::ClaudeCodeStatusline => false,
        }
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
/// Backed by a bool because there are exactly two sources and [`AtomicFlag`]
/// already exists. That is an implementation detail of these three functions
/// and nothing else: callers construct with [`selection`], read with
/// [`selected`] and write with [`select`], all in terms of [`UsageSource`], so
/// widening to a third source changes three function bodies rather than every
/// call site.
pub type SourceSelection = AtomicFlag;

/// Seed a selection from the persisted setting.
#[must_use]
pub const fn selection(source: UsageSource) -> SourceSelection {
    SourceSelection::new(source.is_statusline())
}

/// The source currently selected.
#[must_use]
pub fn selected(selection: &SourceSelection) -> UsageSource {
    if selection.get() {
        UsageSource::ClaudeCodeStatusline
    } else {
        UsageSource::ClaudeAi
    }
}

/// Switch the live selection. The only runtime write of "which source" —
/// everything that needs to know reads this one value.
pub fn select(selection: &SourceSelection, source: UsageSource) {
    selection.set(source.is_statusline());
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

    /// Each policy question is asked separately, so a third source cannot
    /// inherit claude.ai's answers by accident.
    #[test]
    fn only_the_statusline_source_is_exempt_from_the_claude_ai_rules() {
        assert!(UsageSource::ClaudeAi.reaches_claude_ai());
        assert_eq!(UsageSource::ClaudeAi.fixed_poll_interval(), None);

        assert!(!UsageSource::ClaudeCodeStatusline.reaches_claude_ai());
        assert_eq!(
            UsageSource::ClaudeCodeStatusline.fixed_poll_interval(),
            Some(Duration::from_secs(15))
        );
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
        select(&live, UsageSource::ClaudeCodeStatusline);
        assert_eq!(selected(&live), UsageSource::ClaudeCodeStatusline);
        select(&live, UsageSource::ClaudeAi);
        assert_eq!(selected(&live), UsageSource::ClaudeAi);
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
