use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentType {
    Claude,
    Console,
    Ssh,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentStatus {
    Idle,
    Busy,
    Stalled,
    Error,
    Dead,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HealthState {
    Healthy,
    Degraded,
    Unhealthy,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Agent {
    pub name: String,
    pub role: String,
    pub agent_type: AgentType,
    pub task: Option<String>,
    pub path: String,
    pub status: AgentStatus,
    pub status_notes: String,
    pub health: HealthState,
    pub last_heartbeat_ms: Option<u64>,
    pub session: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_round_trip() {
        let agent = Agent {
            name: "worker-1".into(),
            role: "worker".into(),
            agent_type: AgentType::Claude,
            task: Some("CMX1".into()),
            path: "/tmp/work".into(),
            status: AgentStatus::Busy,
            status_notes: "running tests".into(),
            health: HealthState::Healthy,
            last_heartbeat_ms: Some(1700000000000),
            session: Some("cmx-main".into()),
        };
        let json = serde_json::to_string(&agent).unwrap();
        let back: Agent = serde_json::from_str(&json).unwrap();
        assert_eq!(back.name, "worker-1");
        assert_eq!(back.role, "worker");
        assert_eq!(back.status, AgentStatus::Busy);
        assert_eq!(back.health, HealthState::Healthy);
    }

    #[test]
    fn health_state_serde() {
        let json = serde_json::to_string(&HealthState::Degraded).unwrap();
        assert_eq!(json, "\"degraded\"");
    }
}
