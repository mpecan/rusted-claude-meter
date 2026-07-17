use serde::{Deserialize, Serialize};

/// Utilization below this is [`UsageStatus::Safe`].
const WARNING_THRESHOLD: f64 = 50.0;
/// Utilization at or above this is [`UsageStatus::Critical`].
const CRITICAL_THRESHOLD: f64 = 80.0;

/// Traffic-light classification of a utilization percentage.
///
/// Drives both the tray icon colour and notification urgency. Thresholds
/// mirror the original `ClaudeMeter`: safe below 50%, warning from 50%,
/// critical from 80%.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UsageStatus {
    Safe,
    Warning,
    Critical,
}

impl UsageStatus {
    /// Classify a utilization percentage (0–100 scale; values above 100 are critical).
    pub fn from_utilization(percent: f64) -> Self {
        if percent >= CRITICAL_THRESHOLD {
            Self::Critical
        } else if percent >= WARNING_THRESHOLD {
            Self::Warning
        } else {
            Self::Safe
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn classifies_thresholds() {
        assert_eq!(UsageStatus::from_utilization(0.0), UsageStatus::Safe);
        assert_eq!(UsageStatus::from_utilization(49.9), UsageStatus::Safe);
        assert_eq!(UsageStatus::from_utilization(50.0), UsageStatus::Warning);
        assert_eq!(UsageStatus::from_utilization(79.9), UsageStatus::Warning);
        assert_eq!(UsageStatus::from_utilization(80.0), UsageStatus::Critical);
        assert_eq!(UsageStatus::from_utilization(120.0), UsageStatus::Critical);
    }

    #[test]
    fn orders_by_severity() {
        assert!(UsageStatus::Safe < UsageStatus::Warning);
        assert!(UsageStatus::Warning < UsageStatus::Critical);
    }
}
