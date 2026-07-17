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
            .filter_map(RawLimit::into_scoped)
            .collect();
        UsageSnapshot {
            five_hour: self
                .five_hour
                .and_then(|w| w.into_window(LimitWindow::FiveHour)),
            seven_day: self
                .seven_day
                .and_then(|w| w.into_window(LimitWindow::SevenDay)),
            scoped,
            fetched_at,
        }
    }
}

impl RawWindow {
    fn into_window(self, window: LimitWindow) -> Option<UsageWindow> {
        Some(UsageWindow {
            utilization: self.utilization,
            resets_at: self.resets_at?,
            window,
        })
    }
}

impl RawLimit {
    fn into_scoped(self) -> Option<ScopedLimit> {
        let model = self.scope?.model?;
        let display_name = model.display_name?;
        Some(ScopedLimit {
            display_name,
            model_id: model.id,
            usage: UsageWindow {
                utilization: self.percent?,
                resets_at: self.resets_at?,
                window: window_for_kind(&self.kind),
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
