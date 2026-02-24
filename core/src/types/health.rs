use serde::{Deserialize, Serialize};

use super::agent::HealthState;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HealthSignal {
    InfrastructureOk,
    InfrastructureFailed { reason: String },
    HeartbeatRecent { age_secs: u64 },
    HeartbeatStale { age_secs: u64 },
    ErrorPatternDetected { pattern: String },
    ExplicitError { message: String },
    SshConnected,
    SshDisconnected,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthAssessment {
    pub agent: String,
    pub overall: HealthState,
    pub signals: Vec<HealthSignal>,
    pub reason: String,
    pub timestamp_ms: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_signal_tagged() {
        let sig = HealthSignal::HeartbeatStale { age_secs: 120 };
        let json = serde_json::to_string(&sig).unwrap();
        assert!(json.contains("\"type\":\"heartbeat_stale\""));
        assert!(json.contains("\"age_secs\":120"));
        let back: HealthSignal = serde_json::from_str(&json).unwrap();
        assert_eq!(back, sig);
    }

    #[test]
    fn health_assessment_round_trip() {
        let assessment = HealthAssessment {
            agent: "worker-1".into(),
            overall: HealthState::Degraded,
            signals: vec![
                HealthSignal::HeartbeatStale { age_secs: 90 },
                HealthSignal::SshConnected,
            ],
            reason: "heartbeat stale but ssh ok".into(),
            timestamp_ms: 1700000000000,
        };
        let json = serde_json::to_string(&assessment).unwrap();
        let back: HealthAssessment = serde_json::from_str(&json).unwrap();
        assert_eq!(back.agent, "worker-1");
        assert_eq!(back.overall, HealthState::Degraded);
        assert_eq!(back.signals.len(), 2);
    }
}
