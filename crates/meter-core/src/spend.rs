use serde::{Deserialize, Serialize};

/// A money amount in minor units, tagged with its currency and how many decimal
/// places that currency uses.
///
/// Mirrors the claude.ai usage response's money shape exactly
/// (`{ "amount_minor": 35, "currency": "EUR", "exponent": 2 }` = €0.35): the
/// value is carried in minor units so no floating-point rounding creeps in, and
/// the currency travels with it because accounts are not necessarily billed in
/// USD. Formatting to `€0.35` is a UI concern (see `format_money` on the shell
/// side and `formatMoney` in the frontend).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Money {
    /// Amount in minor units (e.g. cents for a 2-decimal currency).
    pub minor: i64,
    /// ISO 4217 currency code the API supplied, e.g. `"EUR"` or `"USD"`.
    pub currency: String,
    /// Number of decimal places for the currency (`2` for EUR/USD), i.e. the
    /// power of ten separating minor units from the major amount.
    pub exponent: u8,
}

impl Money {
    /// The amount as a fractional major-unit value (e.g. `0.35` for €0.35).
    /// Used only for ratio math; display formatting stays out of the domain.
    #[must_use]
    #[allow(clippy::cast_precision_loss)]
    pub fn major(&self) -> f64 {
        self.minor as f64 / 10_f64.powi(i32::from(self.exponent))
    }
}

/// Token/cost-based usage ("usage credits" / extra spend) for the current
/// period.
///
/// Present both for token/cost accounts (Enterprise, which report no allowance
/// limits) and alongside allowance limits when an account has opted into paid
/// overage — the API's `spend` object either way. Money stays in minor units;
/// the domain never formats currency. `limit` is the spend budget the gauge
/// measures against and `cap` the hard ceiling (often equal); either may be
/// absent when the account has no cap configured.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Spend {
    /// Spend to date this period, when reported.
    pub used: Option<Money>,
    /// The spend limit/budget the gauge measures against, when configured.
    pub limit: Option<Money>,
    /// The hard spend ceiling (`cap.money`), when configured.
    pub cap: Option<Money>,
    /// Whether paid overage / usage credits are enabled on the account.
    pub enabled: bool,
}

impl Spend {
    /// Spend to date as a fraction (0.0–…) of the spend limit (falling back to
    /// the hard cap), when both a used figure and a positive denominator are
    /// known. `None` otherwise; not clamped, so an overspend can exceed 1.0.
    #[must_use]
    pub fn fraction_used(&self) -> Option<f64> {
        let used = self.used.as_ref()?;
        let denominator = self.limit.as_ref().or(self.cap.as_ref())?;
        if denominator.minor <= 0 {
            return None;
        }
        Some(used.major() / denominator.major())
    }

    /// Whether this spend carries any money figure worth surfacing. A spend
    /// object with no `used`/`limit`/`cap` (e.g. an account with the feature
    /// merely available but unused and uncapped) has nothing to show.
    #[must_use]
    pub const fn has_amounts(&self) -> bool {
        self.used.is_some() || self.limit.is_some() || self.cap.is_some()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use pretty_assertions::assert_eq;

    fn eur(minor: i64) -> Money {
        Money {
            minor,
            currency: "EUR".to_owned(),
            exponent: 2,
        }
    }

    #[test]
    fn major_divides_by_the_exponent() {
        assert!((eur(35).major() - 0.35).abs() < 1e-9);
        assert!((eur(200_000).major() - 2000.0).abs() < 1e-9);
        let whole = Money {
            minor: 5,
            currency: "JPY".to_owned(),
            exponent: 0,
        };
        assert!((whole.major() - 5.0).abs() < 1e-9);
    }

    #[test]
    fn fraction_used_measures_against_the_limit() {
        let spend = Spend {
            used: Some(eur(50_000)),
            limit: Some(eur(200_000)),
            ..Spend::default()
        };
        assert_eq!(spend.fraction_used(), Some(0.25));
    }

    #[test]
    fn fraction_used_falls_back_to_the_cap() {
        let spend = Spend {
            used: Some(eur(100_000)),
            limit: None,
            cap: Some(eur(200_000)),
            ..Spend::default()
        };
        assert_eq!(spend.fraction_used(), Some(0.5));
    }

    #[test]
    fn fraction_used_is_none_without_a_denominator_or_used() {
        assert_eq!(
            Spend {
                used: Some(eur(35)),
                ..Spend::default()
            }
            .fraction_used(),
            None
        );
        assert_eq!(
            Spend {
                limit: Some(eur(200_000)),
                ..Spend::default()
            }
            .fraction_used(),
            None
        );
    }

    #[test]
    fn fraction_used_is_none_for_a_zero_limit() {
        let spend = Spend {
            used: Some(eur(35)),
            limit: Some(eur(0)),
            ..Spend::default()
        };
        assert_eq!(spend.fraction_used(), None);
    }

    #[test]
    fn has_amounts_reflects_presence_of_any_money() {
        assert!(!Spend::default().has_amounts());
        assert!(
            Spend {
                used: Some(eur(35)),
                ..Spend::default()
            }
            .has_amounts()
        );
    }

    #[test]
    fn spend_round_trips_through_serde() {
        let spend = Spend {
            used: Some(eur(35)),
            limit: Some(eur(200_000)),
            cap: Some(eur(200_000)),
            enabled: true,
        };
        let json = serde_json::to_string(&spend).unwrap();
        assert_eq!(serde_json::from_str::<Spend>(&json).unwrap(), spend);
    }
}
