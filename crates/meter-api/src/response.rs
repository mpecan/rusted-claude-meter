use jiff::Timestamp;
use meter_core::{LimitWindow, ScopedLimit, UsageSnapshot, UsageWindow};
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
            fetched_at,
        }
    }
}

/// The reset instant to use when the API reports the window but omits
/// `resets_at`. A window with no recent usage has nothing scheduled to reset,
/// so the API sends `resets_at: null`; dropping the whole window there would
/// hide, e.g., the 5-hour session card whenever usage is idle. Mirrors
/// `ClaudeMeter`'s `UsageAPIResponse.toDomain` fallback of `now + window`.
fn fallback_reset(window: LimitWindow, fetched_at: Timestamp) -> Timestamp {
    fetched_at
        .checked_add(window.duration())
        .unwrap_or(fetched_at)
}

impl RawWindow {
    /// Map a headline window, substituting a fallback reset when the API
    /// omits `resets_at` so the window is never dropped for lack of a reset.
    fn into_window(self, window: LimitWindow, fetched_at: Timestamp) -> UsageWindow {
        UsageWindow {
            utilization: self.utilization,
            resets_at: self
                .resets_at
                .unwrap_or_else(|| fallback_reset(window, fetched_at)),
            window,
        }
    }
}

impl RawLimit {
    /// A scoped limit is skipped only when the essentials are missing — model
    /// scope, display name, or percent. A missing `resets_at` is filled from
    /// [`fallback_reset`] rather than dropping the limit, matching the
    /// headline-window behaviour above.
    fn into_scoped(self, fetched_at: Timestamp) -> Option<ScopedLimit> {
        let window = window_for_kind(&self.kind);
        let percent = self.percent?;
        let resets_at = self
            .resets_at
            .unwrap_or_else(|| fallback_reset(window, fetched_at));
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
