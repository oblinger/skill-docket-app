//! Periodic state checkpoints — full snapshots taken at intervals to limit
//! journal replay on recovery.
//!
//! `CheckpointManager` decides when to create checkpoints based on a
//! configurable policy (operation count, time, or on-demand) and maintains
//! a bounded history of past checkpoints.

use std::path::Path;

use serde::{Deserialize, Serialize};

use super::state::{SnapshotMetadata, SystemSnapshot};

// ---------------------------------------------------------------------------
// Snapshot persistence
// ---------------------------------------------------------------------------

/// Save a snapshot to a JSON file.
pub fn save_snapshot(snapshot: &SystemSnapshot, path: &Path) -> Result<(), String> {
    let json = serde_json::to_string_pretty(snapshot)
        .map_err(|e| format!("Serialize error: {}", e))?;
    std::fs::write(path, json).map_err(|e| format!("Write error: {}", e))?;
    Ok(())
}

/// Load a snapshot from a JSON file.
pub fn load_snapshot(path: &Path) -> Result<SystemSnapshot, String> {
    let content =
        std::fs::read_to_string(path).map_err(|e| format!("Read error: {}", e))?;
    serde_json::from_str(&content).map_err(|e| format!("Parse error: {}", e))
}

/// Save only if state has changed (compare checksums).
pub fn save_if_changed(
    snapshot: &SystemSnapshot,
    path: &Path,
    last_checksum: &mut String,
) -> Result<bool, String> {
    let new_checksum = snapshot.checksum();
    if new_checksum == *last_checksum {
        return Ok(false);
    }
    save_snapshot(snapshot, path)?;
    *last_checksum = new_checksum;
    Ok(true)
}

// ---------------------------------------------------------------------------
// Checkpoint
// ---------------------------------------------------------------------------

/// A checkpoint combines a full system snapshot with the journal sequence
/// number at which it was taken.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    pub id: String,
    pub snapshot: SystemSnapshot,
    pub journal_sequence: u64,
    pub timestamp_ms: u64,
}

impl Checkpoint {
    /// Generate metadata for the snapshot contained in this checkpoint.
    pub fn metadata(&self) -> SnapshotMetadata {
        self.snapshot.metadata()
    }
}

// ---------------------------------------------------------------------------
// CheckpointPolicy
// ---------------------------------------------------------------------------

/// Policy that determines when a new checkpoint should be created.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CheckpointPolicy {
    /// Checkpoint after every N operations.
    EveryN { operations: u64 },
    /// Checkpoint after a time interval has elapsed.
    TimeBased { interval_ms: u64 },
    /// Only checkpoint when explicitly requested.
    OnDemand,
}

// ---------------------------------------------------------------------------
// CheckpointManager
// ---------------------------------------------------------------------------

/// Manages checkpoint creation, storage, and pruning.
#[derive(Debug, Clone)]
pub struct CheckpointManager {
    checkpoints: Vec<Checkpoint>,
    policy: CheckpointPolicy,
    max_checkpoints: usize,
    operations_since_last: u64,
    last_checkpoint_ms: u64,
}

impl CheckpointManager {
    /// Create a new checkpoint manager with the given policy and capacity.
    pub fn new(policy: CheckpointPolicy, max_checkpoints: usize) -> Self {
        CheckpointManager {
            checkpoints: Vec::new(),
            policy,
            max_checkpoints,
            operations_since_last: 0,
            last_checkpoint_ms: 0,
        }
    }

    /// Record that an operation has occurred (for EveryN policy tracking).
    pub fn record_operation(&mut self) {
        self.operations_since_last += 1;
    }

    /// Determine whether a checkpoint should be created right now.
    pub fn should_checkpoint(&self, now_ms: u64) -> bool {
        match &self.policy {
            CheckpointPolicy::EveryN { operations } => {
                self.operations_since_last >= *operations
            }
            CheckpointPolicy::TimeBased { interval_ms } => {
                if self.last_checkpoint_ms == 0 {
                    // No checkpoint yet — always checkpoint.
                    return true;
                }
                now_ms.saturating_sub(self.last_checkpoint_ms) >= *interval_ms
            }
            CheckpointPolicy::OnDemand => false,
        }
    }

    /// Create a new checkpoint from the given snapshot and journal sequence.
    ///
    /// The checkpoint is added to the internal list and old checkpoints are
    /// pruned if the list exceeds `max_checkpoints`.
    pub fn create_checkpoint(
        &mut self,
        snapshot: SystemSnapshot,
        journal_seq: u64,
        now_ms: u64,
    ) -> &Checkpoint {
        let id = format!("cp-{}-{}", now_ms, journal_seq);
        let cp = Checkpoint {
            id,
            snapshot,
            journal_sequence: journal_seq,
            timestamp_ms: now_ms,
        };
        self.checkpoints.push(cp);
        self.operations_since_last = 0;
        self.last_checkpoint_ms = now_ms;

        // Prune excess checkpoints.
        if self.checkpoints.len() > self.max_checkpoints {
            let excess = self.checkpoints.len() - self.max_checkpoints;
            self.checkpoints.drain(0..excess);
        }

        self.checkpoints.last().unwrap()
    }

    /// Return the most recent checkpoint, if any.
    pub fn latest(&self) -> Option<&Checkpoint> {
        self.checkpoints.last()
    }

    /// Find the checkpoint closest to (but not exceeding) the given journal
    /// sequence number.
    pub fn at_sequence(&self, seq: u64) -> Option<&Checkpoint> {
        self.checkpoints
            .iter()
            .rev()
            .find(|cp| cp.journal_sequence <= seq)
    }

    /// Remove checkpoints older than the given timestamp.
    pub fn prune_old(&mut self, before_ms: u64) {
        self.checkpoints.retain(|cp| cp.timestamp_ms >= before_ms);
    }

    /// Return metadata for all stored checkpoints, oldest first.
    pub fn list_metadata(&self) -> Vec<SnapshotMetadata> {
        self.checkpoints.iter().map(|cp| cp.metadata()).collect()
    }

    /// The number of stored checkpoints.
    pub fn count(&self) -> usize {
        self.checkpoints.len()
    }

    /// Whether there are no stored checkpoints.
    pub fn is_empty(&self) -> bool {
        self.checkpoints.is_empty()
    }

    /// The current checkpoint policy.
    pub fn policy(&self) -> &CheckpointPolicy {
        &self.policy
    }

    /// The number of operations since the last checkpoint.
    pub fn ops_since_last(&self) -> u64 {
        self.operations_since_last
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_snapshot(ts: u64) -> SystemSnapshot {
        SystemSnapshot::new("0.1.0", ts)
    }

    fn make_snapshot_with_agents(ts: u64, count: usize) -> SystemSnapshot {
        use super::super::state::AgentSnapshot;
        let agents: Vec<AgentSnapshot> = (0..count)
            .map(|i| AgentSnapshot {
                name: format!("agent-{}", i),
                role: "worker".into(),
                agent_type: "claude".into(),
                status: "idle".into(),
                task: None,
                path: "/tmp".into(),
                health: "healthy".into(),
                last_heartbeat_ms: Some(ts),
            })
            .collect();
        SystemSnapshot::new("0.1.0", ts).with_agents(agents)
    }

    // --- Policy evaluation ---

    #[test]
    fn every_n_should_checkpoint_when_threshold_reached() {
        let mut mgr = CheckpointManager::new(CheckpointPolicy::EveryN { operations: 5 }, 10);
        assert!(!mgr.should_checkpoint(0));
        for _ in 0..4 {
            mgr.record_operation();
        }
        assert!(!mgr.should_checkpoint(0));
        mgr.record_operation();
        assert!(mgr.should_checkpoint(0));
    }

    #[test]
    fn every_n_resets_after_checkpoint() {
        let mut mgr = CheckpointManager::new(CheckpointPolicy::EveryN { operations: 3 }, 10);
        for _ in 0..3 {
            mgr.record_operation();
        }
        assert!(mgr.should_checkpoint(0));
        mgr.create_checkpoint(make_snapshot(1000), 3, 1000);
        assert!(!mgr.should_checkpoint(0));
        assert_eq!(mgr.ops_since_last(), 0);
    }

    #[test]
    fn time_based_should_checkpoint_when_interval_elapsed() {
        let mut mgr =
            CheckpointManager::new(CheckpointPolicy::TimeBased { interval_ms: 5000 }, 10);
        // No checkpoint yet — should always checkpoint.
        assert!(mgr.should_checkpoint(1000));
        mgr.create_checkpoint(make_snapshot(1000), 0, 1000);
        assert!(!mgr.should_checkpoint(2000)); // only 1s elapsed
        assert!(!mgr.should_checkpoint(5999)); // 4.999s elapsed
        assert!(mgr.should_checkpoint(6000)); // 5s elapsed
    }

    #[test]
    fn time_based_first_checkpoint_always_triggers() {
        let mgr = CheckpointManager::new(CheckpointPolicy::TimeBased { interval_ms: 60000 }, 10);
        assert!(mgr.should_checkpoint(100));
    }

    #[test]
    fn on_demand_never_triggers() {
        let mut mgr = CheckpointManager::new(CheckpointPolicy::OnDemand, 10);
        for _ in 0..100 {
            mgr.record_operation();
        }
        assert!(!mgr.should_checkpoint(0));
        assert!(!mgr.should_checkpoint(999999));
    }

    // --- Checkpoint creation ---

    #[test]
    fn create_checkpoint_stores_it() {
        let mut mgr = CheckpointManager::new(CheckpointPolicy::OnDemand, 10);
        assert!(mgr.is_empty());
        mgr.create_checkpoint(make_snapshot(1000), 5, 1000);
        assert_eq!(mgr.count(), 1);
        assert!(!mgr.is_empty());
    }

    #[test]
    fn create_checkpoint_assigns_id() {
        let mut mgr = CheckpointManager::new(CheckpointPolicy::OnDemand, 10);
        mgr.create_checkpoint(make_snapshot(1000), 5, 1000);
        let cp = mgr.latest().unwrap();
        assert_eq!(cp.id, "cp-1000-5");
    }

    #[test]
    fn create_checkpoint_stores_snapshot() {
        let mut mgr = CheckpointManager::new(CheckpointPolicy::OnDemand, 10);
        let snap = make_snapshot_with_agents(1000, 3);
        mgr.create_checkpoint(snap, 10, 1000);
        let cp = mgr.latest().unwrap();
        assert_eq!(cp.snapshot.agents.len(), 3);
        assert_eq!(cp.journal_sequence, 10);
        assert_eq!(cp.timestamp_ms, 1000);
    }

    #[test]
    fn create_multiple_checkpoints() {
        let mut mgr = CheckpointManager::new(CheckpointPolicy::OnDemand, 10);
        mgr.create_checkpoint(make_snapshot(1000), 5, 1000);
        mgr.create_checkpoint(make_snapshot(2000), 10, 2000);
        mgr.create_checkpoint(make_snapshot(3000), 15, 3000);
        assert_eq!(mgr.count(), 3);
    }

    // --- latest ---

    #[test]
    fn latest_returns_most_recent() {
        let mut mgr = CheckpointManager::new(CheckpointPolicy::OnDemand, 10);
        mgr.create_checkpoint(make_snapshot(1000), 5, 1000);
        mgr.create_checkpoint(make_snapshot(2000), 10, 2000);
        let cp = mgr.latest().unwrap();
        assert_eq!(cp.timestamp_ms, 2000);
        assert_eq!(cp.journal_sequence, 10);
    }

    #[test]
    fn latest_none_when_empty() {
        let mgr = CheckpointManager::new(CheckpointPolicy::OnDemand, 10);
        assert!(mgr.latest().is_none());
    }

    // --- at_sequence ---

    #[test]
    fn at_sequence_finds_matching() {
        let mut mgr = CheckpointManager::new(CheckpointPolicy::OnDemand, 10);
        mgr.create_checkpoint(make_snapshot(1000), 5, 1000);
        mgr.create_checkpoint(make_snapshot(2000), 10, 2000);
        mgr.create_checkpoint(make_snapshot(3000), 15, 3000);

        let cp = mgr.at_sequence(12).unwrap();
        assert_eq!(cp.journal_sequence, 10); // closest <= 12
    }

    #[test]
    fn at_sequence_exact_match() {
        let mut mgr = CheckpointManager::new(CheckpointPolicy::OnDemand, 10);
        mgr.create_checkpoint(make_snapshot(1000), 5, 1000);
        mgr.create_checkpoint(make_snapshot(2000), 10, 2000);

        let cp = mgr.at_sequence(10).unwrap();
        assert_eq!(cp.journal_sequence, 10);
    }

    #[test]
    fn at_sequence_before_first_returns_first() {
        let mut mgr = CheckpointManager::new(CheckpointPolicy::OnDemand, 10);
        mgr.create_checkpoint(make_snapshot(1000), 5, 1000);

        let cp = mgr.at_sequence(5).unwrap();
        assert_eq!(cp.journal_sequence, 5);
    }

    #[test]
    fn at_sequence_before_any_returns_none() {
        let mut mgr = CheckpointManager::new(CheckpointPolicy::OnDemand, 10);
        mgr.create_checkpoint(make_snapshot(1000), 5, 1000);

        assert!(mgr.at_sequence(3).is_none());
    }

    #[test]
    fn at_sequence_empty_returns_none() {
        let mgr = CheckpointManager::new(CheckpointPolicy::OnDemand, 10);
        assert!(mgr.at_sequence(0).is_none());
    }

    // --- Pruning ---

    #[test]
    fn auto_prune_on_create() {
        let mut mgr = CheckpointManager::new(CheckpointPolicy::OnDemand, 3);
        mgr.create_checkpoint(make_snapshot(1000), 5, 1000);
        mgr.create_checkpoint(make_snapshot(2000), 10, 2000);
        mgr.create_checkpoint(make_snapshot(3000), 15, 3000);
        mgr.create_checkpoint(make_snapshot(4000), 20, 4000);
        assert_eq!(mgr.count(), 3);
        // Oldest (1000) should be gone.
        let oldest = &mgr.list_metadata()[0];
        assert_eq!(oldest.timestamp_ms, 2000);
    }

    #[test]
    fn prune_old_removes_before_timestamp() {
        let mut mgr = CheckpointManager::new(CheckpointPolicy::OnDemand, 10);
        mgr.create_checkpoint(make_snapshot(1000), 5, 1000);
        mgr.create_checkpoint(make_snapshot(2000), 10, 2000);
        mgr.create_checkpoint(make_snapshot(3000), 15, 3000);

        mgr.prune_old(2000);
        assert_eq!(mgr.count(), 2);
    }

    #[test]
    fn prune_old_keeps_matching_timestamp() {
        let mut mgr = CheckpointManager::new(CheckpointPolicy::OnDemand, 10);
        mgr.create_checkpoint(make_snapshot(1000), 5, 1000);
        mgr.create_checkpoint(make_snapshot(2000), 10, 2000);

        mgr.prune_old(2000);
        assert_eq!(mgr.count(), 1);
        assert_eq!(mgr.latest().unwrap().timestamp_ms, 2000);
    }

    #[test]
    fn prune_old_future_removes_all() {
        let mut mgr = CheckpointManager::new(CheckpointPolicy::OnDemand, 10);
        mgr.create_checkpoint(make_snapshot(1000), 5, 1000);
        mgr.prune_old(99999);
        assert!(mgr.is_empty());
    }

    // --- Metadata listing ---

    #[test]
    fn list_metadata_returns_all() {
        let mut mgr = CheckpointManager::new(CheckpointPolicy::OnDemand, 10);
        mgr.create_checkpoint(make_snapshot_with_agents(1000, 2), 5, 1000);
        mgr.create_checkpoint(make_snapshot_with_agents(2000, 3), 10, 2000);

        let metas = mgr.list_metadata();
        assert_eq!(metas.len(), 2);
        assert_eq!(metas[0].agent_count, 2);
        assert_eq!(metas[1].agent_count, 3);
    }

    #[test]
    fn list_metadata_empty() {
        let mgr = CheckpointManager::new(CheckpointPolicy::OnDemand, 10);
        assert!(mgr.list_metadata().is_empty());
    }

    // --- Checkpoint metadata ---

    #[test]
    fn checkpoint_metadata_delegates_to_snapshot() {
        let mut mgr = CheckpointManager::new(CheckpointPolicy::OnDemand, 10);
        mgr.create_checkpoint(make_snapshot_with_agents(1000, 5), 10, 1000);
        let cp = mgr.latest().unwrap();
        let meta = cp.metadata();
        assert_eq!(meta.agent_count, 5);
    }

    // --- Policy serde ---

    #[test]
    fn checkpoint_policy_serde_every_n() {
        let policy = CheckpointPolicy::EveryN { operations: 100 };
        let json = serde_json::to_string(&policy).unwrap();
        assert!(json.contains("\"type\":\"every_n\""));
        let back: CheckpointPolicy = serde_json::from_str(&json).unwrap();
        assert_eq!(back, policy);
    }

    #[test]
    fn checkpoint_policy_serde_time_based() {
        let policy = CheckpointPolicy::TimeBased { interval_ms: 60000 };
        let json = serde_json::to_string(&policy).unwrap();
        let back: CheckpointPolicy = serde_json::from_str(&json).unwrap();
        assert_eq!(back, policy);
    }

    #[test]
    fn checkpoint_policy_serde_on_demand() {
        let policy = CheckpointPolicy::OnDemand;
        let json = serde_json::to_string(&policy).unwrap();
        let back: CheckpointPolicy = serde_json::from_str(&json).unwrap();
        assert_eq!(back, policy);
    }

    // --- Policy accessor ---

    #[test]
    fn policy_accessor_returns_current() {
        let mgr = CheckpointManager::new(CheckpointPolicy::EveryN { operations: 42 }, 10);
        assert_eq!(*mgr.policy(), CheckpointPolicy::EveryN { operations: 42 });
    }

    #[test]
    fn checkpoint_save_and_load_roundtrip() {
        let snapshot = make_snapshot_with_agents(1000, 3);
        let dir = std::env::temp_dir().join("cmx_checkpoint_roundtrip");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("state.json");
        save_snapshot(&snapshot, &path).unwrap();
        let loaded = load_snapshot(&path).unwrap();
        assert_eq!(snapshot.agents.len(), loaded.agents.len());
        assert_eq!(snapshot, loaded);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_if_changed_skips_when_unchanged() {
        let snapshot = make_snapshot(5000);
        let dir = std::env::temp_dir().join("cmx_save_if_changed");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("state.json");
        let mut checksum = String::new();
        assert!(save_if_changed(&snapshot, &path, &mut checksum).unwrap());
        assert!(!save_if_changed(&snapshot, &path, &mut checksum).unwrap());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_if_changed_writes_when_content_changes() {
        let snap1 = make_snapshot(1000);
        let snap2 = make_snapshot_with_agents(2000, 5);
        let dir = std::env::temp_dir().join("cmx_save_if_changed_2");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("state.json");
        let mut checksum = String::new();
        assert!(save_if_changed(&snap1, &path, &mut checksum).unwrap());
        assert!(save_if_changed(&snap2, &path, &mut checksum).unwrap());
        let loaded = load_snapshot(&path).unwrap();
        assert_eq!(loaded.agents.len(), 5);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_snapshot_error_on_missing_file() {
        let result = load_snapshot(Path::new("/tmp/cmx_nonexistent_file_12345.json"));
        assert!(result.is_err());
    }
}
