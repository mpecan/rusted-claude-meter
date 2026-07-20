use serde::{Deserialize, Serialize};

use crate::snapshot::UsageSnapshot;

/// How the tray/popover should present usage.
///
/// `Auto` follows the account: allowance accounts (Pro/Team, which report
/// usage limits) show the percentage-of-limit view; token/cost accounts
/// (Enterprise, no limits) show the spend view. `Allowance` and `Cost` pin the
/// view regardless of what the account reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UsageMode {
    #[default]
    Auto,
    Allowance,
    Cost,
}

impl UsageMode {
    /// Resolve `Auto` against a snapshot to a concrete view; a pinned
    /// `Allowance`/`Cost` is returned unchanged.
    #[must_use]
    pub const fn effective(self, snapshot: &UsageSnapshot) -> Self {
        match self {
            Self::Auto => snapshot.suggested_mode(),
            other => other,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::Timestamp;
    use pretty_assertions::assert_eq;

    fn at(ts: &str) -> Timestamp {
        ts.parse().unwrap_or(Timestamp::UNIX_EPOCH)
    }

    fn empty_snapshot() -> UsageSnapshot {
        UsageSnapshot {
            five_hour: None,
            seven_day: None,
            scoped: vec![],
            spend: None,
            fetched_at: at("2026-07-17T12:00:00Z"),
        }
    }

    fn cost_snapshot() -> UsageSnapshot {
        UsageSnapshot {
            spend: Some(Box::new(crate::spend::Spend {
                used: Some(crate::spend::Money {
                    minor: 12_500,
                    currency: "USD".to_owned(),
                    exponent: 2,
                }),
                ..crate::spend::Spend::default()
            })),
            ..empty_snapshot()
        }
    }

    #[test]
    fn auto_follows_the_suggestion_for_a_cost_account() {
        let snapshot = cost_snapshot();
        assert_eq!(UsageMode::Auto.effective(&snapshot), UsageMode::Cost);
    }

    #[test]
    fn auto_stays_allowance_when_no_limits_and_no_spend() {
        let snapshot = empty_snapshot();
        assert_eq!(UsageMode::Auto.effective(&snapshot), UsageMode::Allowance);
    }

    #[test]
    fn pinned_modes_are_returned_unchanged() {
        let cost_account = cost_snapshot();
        assert_eq!(
            UsageMode::Allowance.effective(&cost_account),
            UsageMode::Allowance
        );
        assert_eq!(UsageMode::Cost.effective(&cost_account), UsageMode::Cost);
    }

    #[test]
    fn default_is_auto() {
        assert_eq!(UsageMode::default(), UsageMode::Auto);
    }
}
