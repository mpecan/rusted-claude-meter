use jiff::Timestamp;
use meter_core::{LimitWindow, Money, ScopedLimit, Spend, UsageSnapshot, UsageWindow};
use serde::Deserialize;

/// Kinds already surfaced through the flat headline fields. Entries with
/// these kinds are excluded from the scoped pass so a limit the API reports
/// both ways cannot render twice.
const HEADLINE_KINDS: &[&str] = &["five_hour", "seven_day"];

/// Raw shape of `GET /api/organizations/{org_id}/usage`.
///
/// The flat per-model fields (`seven_day_sonnet`, `seven_day_opus`, …) are
/// legacy and return `null`; model-specific caps appear only as entries in
/// `limits`, which name their own scope. Unknown fields are ignored so new
/// API additions never break decoding.
#[derive(Debug, Deserialize)]
pub struct UsageResponse {
    pub five_hour: Option<RawWindow>,
    pub seven_day: Option<RawWindow>,
    #[serde(default)]
    pub limits: Vec<RawLimit>,
    /// Token/cost-based (Enterprise) usage. Absent on allowance-only accounts,
    /// and present-but-empty (`{"unsurfaced": true}`) on accounts the endpoint
    /// does not surface cost for; both decode to `None` spend.
    #[serde(default)]
    pub spend: Option<RawSpend>,
}

#[derive(Debug, Deserialize)]
pub struct RawWindow {
    pub utilization: f64,
    pub resets_at: Option<Timestamp>,
}

#[derive(Debug, Deserialize)]
pub struct RawLimit {
    pub kind: String,
    pub percent: Option<f64>,
    pub resets_at: Option<Timestamp>,
    #[serde(default)]
    pub is_active: bool,
    pub scope: Option<RawScope>,
}

#[derive(Debug, Deserialize)]
pub struct RawScope {
    pub model: Option<RawModelScope>,
}

#[derive(Debug, Deserialize)]
pub struct RawModelScope {
    pub id: Option<String>,
    pub display_name: Option<String>,
}

/// Raw token/cost usage for the current billing period — the API's `spend`
/// object.
///
/// Every field is optional so a payload that surfaces no cost (the
/// `{"unsurfaced": true}` stub, or an account with the feature merely available)
/// maps to `None` rather than erroring. Extra fields the endpoint also sends —
/// `percent`, `severity`, `disclaimer`, `balance`, `auto_reload` — are ignored;
/// the gauge is recomputed from `used`/`limit` for full precision.
#[derive(Debug, Deserialize)]
pub struct RawSpend {
    pub used: Option<RawMoney>,
    pub limit: Option<RawMoney>,
    pub cap: Option<RawCap>,
    #[serde(default)]
    pub enabled: bool,
}

/// A money amount as the API encodes it: minor units, currency code and the
/// currency's decimal-place count (`{ "amount_minor": 35, "currency": "EUR",
/// "exponent": 2 }` = €0.35).
#[derive(Debug, Deserialize)]
pub struct RawMoney {
    pub amount_minor: Option<i64>,
    pub currency: Option<String>,
    pub exponent: Option<u8>,
}

/// The `spend.cap` wrapper: a hard ceiling expressed either as money or (in a
/// shape not yet observed) as credits. Only the money form is consumed.
#[derive(Debug, Deserialize)]
pub struct RawCap {
    pub money: Option<RawMoney>,
}

impl RawMoney {
    /// Map to a domain [`Money`] only when the full triple is present — a money
    /// object missing any of amount/currency/exponent is not usable and is
    /// dropped rather than guessed at.
    fn into_money(self) -> Option<Money> {
        Some(Money {
            minor: self.amount_minor?,
            currency: self.currency?,
            exponent: self.exponent?,
        })
    }
}

impl UsageResponse {
    /// Map the raw response into the domain snapshot.
    ///
    /// Headline windows come from the flat fields; scoped limits come from
    /// `limits` entries that carry a model scope with a display name and a
    /// complete usage window. Incomplete entries are skipped, not errors —
    /// the API adds kinds over time and decoding must stay forward-compatible.
    pub fn into_snapshot(self, fetched_at: Timestamp) -> UsageSnapshot {
        let scoped = self
            .limits
            .into_iter()
            .filter(|limit| !HEADLINE_KINDS.contains(&limit.kind.as_str()))
            .filter_map(|limit| limit.into_scoped(fetched_at))
            .collect();
        UsageSnapshot {
            five_hour: self
                .five_hour
                .map(|w| w.into_window(LimitWindow::FiveHour, fetched_at)),
            seven_day: self
                .seven_day
                .map(|w| w.into_window(LimitWindow::SevenDay, fetched_at)),
            scoped,
            // Token/cost usage, when the response surfaces any usable figure;
            // the `{"unsurfaced": true}` stub and an absent field both yield
            // `None`. Boxed to keep `UsageSnapshot` cheap to move/clone.
            spend: self.spend.and_then(RawSpend::into_spend).map(Box::new),
            fetched_at,
        }
    }
}

impl RawWindow {
    /// Map a headline window, substituting a fallback reset when the API
    /// omits `resets_at` so the window is never dropped for lack of a reset.
    /// A window with no recent usage has nothing scheduled to reset, so the
    /// API sends `resets_at: null`; dropping the whole window there would
    /// hide, e.g., the 5-hour session card whenever usage is idle. The
    /// fallback (`fetched_at + window`, see [`LimitWindow::fallback_reset`])
    /// mirrors `ClaudeMeter`'s `UsageAPIResponse.toDomain`.
    fn into_window(self, window: LimitWindow, fetched_at: Timestamp) -> UsageWindow {
        UsageWindow {
            utilization: self.utilization,
            resets_at: self
                .resets_at
                .unwrap_or_else(|| window.fallback_reset(fetched_at)),
            window,
        }
    }
}

impl RawLimit {
    /// A scoped limit is skipped only when the essentials are missing — model
    /// scope, display name, or percent. A missing `resets_at` is filled from
    /// [`LimitWindow::fallback_reset`] rather than dropping the limit, matching
    /// the headline-window behaviour above.
    fn into_scoped(self, fetched_at: Timestamp) -> Option<ScopedLimit> {
        let window = window_for_kind(&self.kind);
        let percent = self.percent?;
        let resets_at = self
            .resets_at
            .unwrap_or_else(|| window.fallback_reset(fetched_at));
        let model = self.scope?.model?;
        let display_name = model.display_name?;
        Some(ScopedLimit {
            display_name,
            model_id: model.id,
            usage: UsageWindow {
                utilization: percent,
                resets_at,
                window,
            },
            is_active: self.is_active,
        })
    }
}

fn window_for_kind(kind: &str) -> LimitWindow {
    if kind.starts_with("five_hour") {
        LimitWindow::FiveHour
    } else {
        LimitWindow::SevenDay
    }
}

impl RawSpend {
    /// Map raw spend into the domain [`Spend`], or `None` when it carries no
    /// money at all — the `{"unsurfaced": true}` stub, and an account with the
    /// feature merely available but no used/limit/cap, both leave nothing to
    /// show and must not force an empty cost card on.
    fn into_spend(self) -> Option<Spend> {
        let spend = Spend {
            used: self.used.and_then(RawMoney::into_money),
            limit: self.limit.and_then(RawMoney::into_money),
            cap: self
                .cap
                .and_then(|cap| cap.money)
                .and_then(RawMoney::into_money),
            enabled: self.enabled,
        };
        spend.has_amounts().then_some(spend)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn unsurfaced_stub_yields_no_spend() {
        let raw: RawSpend = serde_json::from_str(r#"{ "unsurfaced": true }"#).unwrap();
        assert!(raw.into_spend().is_none());
    }

    #[test]
    fn real_spend_object_decodes_used_limit_and_cap() {
        // The exact `spend` object claude.ai returns (captured via debug
        // logging): money is a { amount_minor, currency, exponent } triple, the
        // cap is nested under `cap.money`, and the extra `percent`/`severity`/
        // `disclaimer` fields are ignored.
        let raw: RawSpend = serde_json::from_str(
            r#"{
                "used":  { "amount_minor": 35,     "currency": "EUR", "exponent": 2 },
                "limit": { "amount_minor": 200000, "currency": "EUR", "exponent": 2 },
                "percent": 0,
                "severity": "normal",
                "enabled": true,
                "cap": { "money": { "amount_minor": 200000, "currency": "EUR", "exponent": 2 }, "credits": null },
                "balance": null,
                "disclaimer": "Usage credits cover you when you hit your plan limits."
            }"#,
        )
        .unwrap();
        let spend = raw.into_spend().unwrap();
        assert_eq!(
            spend.used,
            Some(Money {
                minor: 35,
                currency: "EUR".to_owned(),
                exponent: 2
            })
        );
        assert_eq!(spend.limit.as_ref().map(|m| m.minor), Some(200_000));
        assert_eq!(spend.cap.as_ref().map(|m| m.minor), Some(200_000));
        assert!(spend.enabled);
        // €0.35 of a €2000 budget.
        assert!((spend.fraction_used().unwrap() - 0.000_175).abs() < 1e-9);
    }

    #[test]
    fn spend_with_no_money_yields_no_spend() {
        // The feature is enabled but nothing is used and no cap is set: there is
        // nothing to display, so it must not force an empty cost card on.
        let raw: RawSpend = serde_json::from_str(r#"{ "enabled": true }"#).unwrap();
        assert!(raw.into_spend().is_none());
    }

    #[test]
    fn a_money_object_missing_part_of_its_triple_is_dropped() {
        // amount/currency/exponent must all be present; a partial money object
        // is not usable and is dropped rather than guessed at.
        let raw: RawSpend =
            serde_json::from_str(r#"{ "used": { "amount_minor": 35, "currency": "EUR" } }"#)
                .unwrap();
        assert!(raw.into_spend().is_none());
    }

    #[test]
    fn spend_alongside_an_allowance_response_keeps_the_allowance() {
        // A real Pro/Team account carries both allowance limits and a spend
        // object (paid overage). Decoding the spend must never wipe the limits.
        let json = r#"{
            "five_hour": { "utilization": 5.0, "resets_at": null },
            "seven_day": { "utilization": 4.0, "resets_at": null },
            "limits": [],
            "spend": {
                "used":  { "amount_minor": 35,     "currency": "EUR", "exponent": 2 },
                "limit": { "amount_minor": 200000, "currency": "EUR", "exponent": 2 },
                "enabled": true
            }
        }"#;
        let snapshot = serde_json::from_str::<UsageResponse>(json)
            .unwrap()
            .into_snapshot("2026-07-19T20:33:00Z".parse().unwrap());
        assert!(snapshot.five_hour.is_some());
        assert!(snapshot.seven_day.is_some());
        // Both allowance and cost present -> the auto-detected view stays on the
        // allowance side (with the cost surfaced alongside).
        assert!(snapshot.has_limits());
        assert_eq!(snapshot.suggested_mode(), meter_core::UsageMode::Allowance);
        assert_eq!(
            snapshot
                .spend
                .as_ref()
                .and_then(|s| s.used.as_ref())
                .map(|m| m.minor),
            Some(35)
        );
    }
}
