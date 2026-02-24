use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BackoffStrategy {
    Exponential,
    Linear,
    Fixed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PoolConfigYaml {
    pub size: u32,
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_size: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Settings {
    #[serde(default)]
    pub version: String,
    pub health_check_interval: u64,
    pub heartbeat_timeout: u64,
    pub message_timeout: u64,
    pub snapshot_interval: u64,
    pub project_root: String,
    pub ready_prompt_pattern: String,
    pub max_retries: u32,
    pub backoff_strategy: BackoffStrategy,
    pub ssh_retries: u32,
    pub ssh_backoff: Vec<u64>,
    pub alert_targets: Vec<String>,
    pub escalation_timeout: u64,
    pub pool_configs: HashMap<String, PoolConfigYaml>,
    pub pool_auto_expand: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FolderEntry {
    pub name: String,
    pub path: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip() {
        let settings = Settings {
            version: "0.1.0".into(),
            health_check_interval: 5000,
            heartbeat_timeout: 30000,
            message_timeout: 10000,
            snapshot_interval: 60000,
            project_root: "/projects/cmx".into(),
            ready_prompt_pattern: r"\$\s*$".into(),
            max_retries: 3,
            backoff_strategy: BackoffStrategy::Exponential,
            ssh_retries: 5,
            ssh_backoff: vec![1000, 2000, 4000, 8000, 16000],
            alert_targets: vec!["pm".into()],
            escalation_timeout: 300000,
            pool_configs: HashMap::new(),
            pool_auto_expand: false,
        };
        let json = serde_json::to_string(&settings).unwrap();
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(back, settings);
    }

    #[test]
    fn backoff_strategy_serde() {
        let json = serde_json::to_string(&BackoffStrategy::Exponential).unwrap();
        assert_eq!(json, "\"exponential\"");
    }

    #[test]
    fn folder_entry_round_trip() {
        let entry = FolderEntry {
            name: "cmx-core".into(),
            path: "/projects/cmx/core".into(),
        };
        let json = serde_json::to_string(&entry).unwrap();
        let back: FolderEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(back, entry);
    }
}
