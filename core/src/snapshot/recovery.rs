//! State recovery from snapshots and journals.
//!
//! After a crash, the recovery engine locates the most recent valid checkpoint,
//! gathers subsequent journal entries, and produces a recovery plan. The plan
//! describes what needs to be replayed to reconstruct the pre-crash state.

use serde::{Deserialize, Serialize};

use super::checkpoint::Checkpoint;
use super::journal::{Journal, JournalEntry};

// ---------------------------------------------------------------------------
// RecoveryPlan
// ---------------------------------------------------------------------------

/// A plan describing how to recover system state from a checkpoint plus
/// journal entries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryPlan {
    /// The checkpoint to start from, if any.
    pub checkpoint_id: Option<String>,
    /// The journal sequence the checkpoint corresponds to.
    pub checkpoint_sequence: Option<u64>,
    /// The journal entries that need to be replayed after the checkpoint.
    pub journal_entries: Vec<JournalEntry>,
    /// The number of operations to replay.
    pub operations_to_replay: usize,
    /// Estimated time to complete recovery, in milliseconds.
    pub estimated_recovery_ms: u64,
}

impl RecoveryPlan {
    /// Whether this plan requires any work (has entries to replay or a
    /// checkpoint to restore).
    pub fn is_empty(&self) -> bool {
        self.checkpoint_id.is_none() && self.journal_entries.is_empty()
    }

    /// A brief summary of the recovery plan.
    pub fn summary(&self) -> String {
        match &self.checkpoint_id {
            Some(id) => format!(
                "restore checkpoint {} then replay {} operations (est. {}ms)",
                id, self.operations_to_replay, self.estimated_recovery_ms,
            ),
            None => {
                if self.operations_to_replay == 0 && self.journal_entries.is_empty() {
                    "nothing to recover".to_string()
                } else {
                    format!(
                        "replay {} operations from journal (no checkpoint, est. {}ms)",
                        self.operations_to_replay, self.estimated_recovery_ms,
                    )
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// RecoveryResult
// ---------------------------------------------------------------------------

/// The outcome of executing a recovery plan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryResult {
    pub success: bool,
    pub agents_recovered: usize,
    pub tasks_recovered: usize,
    pub operations_replayed: usize,
    pub errors: Vec<String>,
    pub duration_ms: u64,
}

impl RecoveryResult {
    /// Create a successful result.
    pub fn ok(agents: usize, tasks: usize, ops: usize, duration_ms: u64) -> Self {
        RecoveryResult {
            success: true,
            agents_recovered: agents,
            tasks_recovered: tasks,
            operations_replayed: ops,
            errors: Vec::new(),
            duration_ms,
        }
    }

    /// Create a failed result.
    pub fn failed(errors: Vec<String>, duration_ms: u64) -> Self {
        RecoveryResult {
            success: false,
            agents_recovered: 0,
            tasks_recovered: 0,
            operations_replayed: 0,
            errors,
            duration_ms,
        }
    }

    /// Whether any errors occurred during recovery.
    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }
}

// ---------------------------------------------------------------------------
// RecoveryEngine
// ---------------------------------------------------------------------------

/// Estimated milliseconds per journal operation replay.
const MS_PER_OPERATION: u64 = 1;

/// Estimated milliseconds for checkpoint restoration.
const MS_PER_CHECKPOINT_RESTORE: u64 = 50;

/// Engine that produces recovery plans from checkpoints and journals.
#[derive(Debug, Clone)]
pub struct RecoveryEngine;

impl RecoveryEngine {
    pub fn new() -> Self {
        RecoveryEngine
    }

    /// Build a recovery plan from a list of checkpoints and a journal.
    ///
    /// Strategy:
    /// 1. Find the latest valid checkpoint.
    /// 2. Gather all journal entries after that checkpoint's sequence.
    /// 3. If no checkpoint, plan replays the entire journal.
    pub fn plan(
        &self,
        checkpoints: &[Checkpoint],
        journal: &Journal,
    ) -> RecoveryPlan {
        // Find the latest checkpoint with a valid (consistent) snapshot.
        let valid_cp = checkpoints
            .iter()
            .rev()
            .find(|cp| cp.snapshot.is_consistent());

        match valid_cp {
            Some(cp) => {
                let entries: Vec<JournalEntry> = journal
                    .entries_since(cp.journal_sequence + 1)
                    .into_iter()
                    .cloned()
                    .collect();
                let ops = entries.len();
                let est = MS_PER_CHECKPOINT_RESTORE + (ops as u64 * MS_PER_OPERATION);
                RecoveryPlan {
                    checkpoint_id: Some(cp.id.clone()),
                    checkpoint_sequence: Some(cp.journal_sequence),
                    journal_entries: entries,
                    operations_to_replay: ops,
                    estimated_recovery_ms: est,
                }
            }
            None => {
                let entries: Vec<JournalEntry> = journal
                    .entries_since(0)
                    .into_iter()
                    .cloned()
                    .collect();
                let ops = entries.len();
                let est = ops as u64 * MS_PER_OPERATION;
                RecoveryPlan {
                    checkpoint_id: None,
                    checkpoint_sequence: None,
                    journal_entries: entries,
                    operations_to_replay: ops,
                    estimated_recovery_ms: est,
                }
            }
        }
    }

    /// Re-estimate the recovery time for a plan (useful after modifying it).
    pub fn estimate_plan(&self, plan: &RecoveryPlan) -> u64 {
        let base = if plan.checkpoint_id.is_some() {
            MS_PER_CHECKPOINT_RESTORE
        } else {
            0
        };
        base + (plan.operations_to_replay as u64 * MS_PER_OPERATION)
    }

    /// Validate a checkpoint's snapshot for consistency issues.
    ///
    /// Returns a list of error messages. An empty list means the checkpoint
    /// is valid.
    pub fn validate_checkpoint(&self, cp: &Checkpoint) -> Vec<String> {
        let mut errors = Vec::new();

        if cp.snapshot.version.is_empty() {
            errors.push("snapshot version is empty".into());
        }

        // Check for duplicate agent names.
        let names = cp.snapshot.agent_names();
        let mut seen = std::collections::HashMap::new();
        for name in &names {
            let entry = seen.entry(*name).or_insert(0u32);
            *entry += 1;
            if *entry > 1 {
                errors.push(format!("duplicate agent name: {}", name));
            }
        }

        // Check for duplicate task IDs.
        let ids = cp.snapshot.task_ids();
        let mut seen_ids = std::collections::HashMap::new();
        for id in &ids {
            let entry = seen_ids.entry(*id).or_insert(0u32);
            *entry += 1;
            if *entry > 1 {
                errors.push(format!("duplicate task id: {}", id));
            }
        }

        // Check for duplicate session names.
        let sess = cp.snapshot.session_names();
        let mut seen_sess = std::collections::HashMap::new();
        for s in &sess {
            let entry = seen_sess.entry(*s).or_insert(0u32);
            *entry += 1;
            if *entry > 1 {
                errors.push(format!("duplicate session name: {}", s));
            }
        }

        // Check cross-references.
        if !cp.snapshot.is_consistent() && errors.is_empty() {
            errors.push("snapshot has invalid cross-references".into());
        }

        errors
    }

    /// Validate journal entries for checksum integrity.
    ///
    /// Returns indices of entries with invalid checksums.
    pub fn validate_journal_entries(&self, entries: &[JournalEntry]) -> Vec<usize> {
        entries
            .iter()
            .enumerate()
            .filter(|(_, e)| !e.verify_checksum())
            .map(|(i, _)| i)
            .collect()
    }
}

impl Default for RecoveryEngine {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot::journal::JournalOp;
    use crate::snapshot::state::{AgentSnapshot, SystemSnapshot, TaskSnapshot};

    fn make_snapshot(ts: u64) -> SystemSnapshot {
        SystemSnapshot::new("0.1.0", ts)
    }

    fn make_consistent_snapshot(ts: u64) -> SystemSnapshot {
        SystemSnapshot::new("0.1.0", ts)
            .with_agents(vec![AgentSnapshot {
                name: "w1".into(),
                role: "worker".into(),
                agent_type: "claude".into(),
                status: "idle".into(),
                task: Some("T1".into()),
                path: "/tmp".into(),
                health: "healthy".into(),
                last_heartbeat_ms: Some(ts),
            }])
            .with_tasks(vec![TaskSnapshot {
                id: "T1".into(),
                title: "Task one".into(),
                status: "pending".into(),
                source: "roadmap".into(),
                agent: Some("w1".into()),
                result: None,
                children_ids: Vec::new(),
                spec_path: None,
            }])
    }

    fn make_checkpoint(id: &str, ts: u64, seq: u64, snap: SystemSnapshot) -> Checkpoint {
        Checkpoint {
            id: id.into(),
            snapshot: snap,
            journal_sequence: seq,
            timestamp_ms: ts,
        }
    }

    fn make_journal_with_entries(ops: &[(&str, u64)]) -> Journal {
        let mut j = Journal::new(1000);
        for (name, ts) in ops {
            j.append(
                JournalOp::AgentCreated {
                    name: (*name).into(),
                    role: "worker".into(),
                },
                *ts,
            );
        }
        j
    }

    // --- Plan generation ---

    #[test]
    fn plan_with_checkpoint_and_journal() {
        let snap = make_consistent_snapshot(1000);
        let cp = make_checkpoint("cp-1", 1000, 5, snap);
        let journal = make_journal_with_entries(&[
            ("a0", 100),
            ("a1", 200),
            ("a2", 300),
            ("a3", 400),
            ("a4", 500),
            ("a5", 600), // seq 5
            ("a6", 700), // seq 6
            ("a7", 800), // seq 7
        ]);

        let engine = RecoveryEngine::new();
        let plan = engine.plan(&[cp], &journal);

        assert!(plan.checkpoint_id.is_some());
        assert_eq!(plan.checkpoint_id.as_deref(), Some("cp-1"));
        assert_eq!(plan.checkpoint_sequence, Some(5));
        assert_eq!(plan.operations_to_replay, 2); // entries 6 and 7
        assert!(!plan.is_empty());
    }

    #[test]
    fn plan_without_checkpoint() {
        let journal = make_journal_with_entries(&[
            ("a0", 100),
            ("a1", 200),
            ("a2", 300),
        ]);

        let engine = RecoveryEngine::new();
        let plan = engine.plan(&[], &journal);

        assert!(plan.checkpoint_id.is_none());
        assert_eq!(plan.operations_to_replay, 3);
    }

    #[test]
    fn plan_empty_journal_with_checkpoint() {
        let snap = make_consistent_snapshot(1000);
        let cp = make_checkpoint("cp-1", 1000, 0, snap);
        let journal = Journal::new(100);

        let engine = RecoveryEngine::new();
        let plan = engine.plan(&[cp], &journal);

        assert!(plan.checkpoint_id.is_some());
        assert_eq!(plan.operations_to_replay, 0);
    }

    #[test]
    fn plan_empty_everything() {
        let engine = RecoveryEngine::new();
        let journal = Journal::new(100);
        let plan = engine.plan(&[], &journal);

        assert!(plan.is_empty());
        assert_eq!(plan.operations_to_replay, 0);
    }

    #[test]
    fn plan_uses_latest_valid_checkpoint() {
        let snap1 = make_consistent_snapshot(1000);
        let snap2 = make_consistent_snapshot(2000);
        let cp1 = make_checkpoint("cp-1", 1000, 3, snap1);
        let cp2 = make_checkpoint("cp-2", 2000, 7, snap2);

        let journal = make_journal_with_entries(&[
            ("a0", 100),
            ("a1", 200),
            ("a2", 300),
            ("a3", 400),
            ("a4", 500),
            ("a5", 600),
            ("a6", 700),
            ("a7", 800), // seq 7
            ("a8", 900), // seq 8
        ]);

        let engine = RecoveryEngine::new();
        let plan = engine.plan(&[cp1, cp2], &journal);

        assert_eq!(plan.checkpoint_id.as_deref(), Some("cp-2"));
        assert_eq!(plan.operations_to_replay, 1); // only seq 8
    }

    #[test]
    fn plan_skips_inconsistent_checkpoint() {
        // Create an inconsistent snapshot (agent references missing task).
        let mut bad_snap = make_snapshot(2000);
        bad_snap.agents.push(AgentSnapshot {
            name: "w1".into(),
            role: "worker".into(),
            agent_type: "claude".into(),
            status: "idle".into(),
            task: Some("NONEXISTENT".into()),
            path: "/tmp".into(),
            health: "healthy".into(),
            last_heartbeat_ms: None,
        });

        let good_snap = make_consistent_snapshot(1000);
        let cp1 = make_checkpoint("cp-good", 1000, 3, good_snap);
        let cp2 = make_checkpoint("cp-bad", 2000, 7, bad_snap);

        let journal = make_journal_with_entries(&[
            ("a0", 100),
            ("a1", 200),
            ("a2", 300),
            ("a3", 400),
            ("a4", 500),
            ("a5", 600),
            ("a6", 700),
            ("a7", 800),
        ]);

        let engine = RecoveryEngine::new();
        let plan = engine.plan(&[cp1, cp2], &journal);

        // Should fall back to cp-good since cp-bad is inconsistent.
        assert_eq!(plan.checkpoint_id.as_deref(), Some("cp-good"));
    }

    // --- Estimation ---

    #[test]
    fn estimate_plan_with_checkpoint() {
        let engine = RecoveryEngine::new();
        let plan = RecoveryPlan {
            checkpoint_id: Some("cp-1".into()),
            checkpoint_sequence: Some(5),
            journal_entries: Vec::new(),
            operations_to_replay: 10,
            estimated_recovery_ms: 0,
        };
        let est = engine.estimate_plan(&plan);
        assert_eq!(est, 50 + 10); // 50ms restore + 10 * 1ms
    }

    #[test]
    fn estimate_plan_without_checkpoint() {
        let engine = RecoveryEngine::new();
        let plan = RecoveryPlan {
            checkpoint_id: None,
            checkpoint_sequence: None,
            journal_entries: Vec::new(),
            operations_to_replay: 100,
            estimated_recovery_ms: 0,
        };
        let est = engine.estimate_plan(&plan);
        assert_eq!(est, 100); // 100 * 1ms
    }

    #[test]
    fn estimate_plan_zero_operations() {
        let engine = RecoveryEngine::new();
        let plan = RecoveryPlan {
            checkpoint_id: Some("cp-1".into()),
            checkpoint_sequence: Some(0),
            journal_entries: Vec::new(),
            operations_to_replay: 0,
            estimated_recovery_ms: 0,
        };
        let est = engine.estimate_plan(&plan);
        assert_eq!(est, 50); // just restore
    }

    // --- Validation ---

    #[test]
    fn validate_consistent_checkpoint() {
        let snap = make_consistent_snapshot(1000);
        let cp = make_checkpoint("cp-1", 1000, 5, snap);
        let engine = RecoveryEngine::new();
        let errors = engine.validate_checkpoint(&cp);
        assert!(errors.is_empty());
    }

    #[test]
    fn validate_empty_version() {
        let mut snap = make_snapshot(1000);
        snap.version = String::new();
        let cp = make_checkpoint("cp-1", 1000, 5, snap);
        let engine = RecoveryEngine::new();
        let errors = engine.validate_checkpoint(&cp);
        assert!(errors.iter().any(|e| e.contains("version")));
    }

    #[test]
    fn validate_duplicate_agents() {
        let snap = SystemSnapshot::new("0.1.0", 1000).with_agents(vec![
            AgentSnapshot {
                name: "dupe".into(),
                role: "worker".into(),
                agent_type: "claude".into(),
                status: "idle".into(),
                task: None,
                path: "/tmp".into(),
                health: "healthy".into(),
                last_heartbeat_ms: None,
            },
            AgentSnapshot {
                name: "dupe".into(),
                role: "pilot".into(),
                agent_type: "claude".into(),
                status: "idle".into(),
                task: None,
                path: "/tmp".into(),
                health: "healthy".into(),
                last_heartbeat_ms: None,
            },
        ]);
        let cp = make_checkpoint("cp-1", 1000, 5, snap);
        let engine = RecoveryEngine::new();
        let errors = engine.validate_checkpoint(&cp);
        assert!(errors.iter().any(|e| e.contains("duplicate agent")));
    }

    #[test]
    fn validate_journal_entries_all_valid() {
        let mut j = Journal::new(100);
        j.append(
            JournalOp::AgentCreated {
                name: "w1".into(),
                role: "worker".into(),
            },
            1000,
        );
        let engine = RecoveryEngine::new();
        let bad = engine.validate_journal_entries(j.all_entries());
        assert!(bad.is_empty());
    }

    #[test]
    fn validate_journal_entries_with_tampered() {
        let mut j = Journal::new(100);
        j.append(
            JournalOp::AgentCreated {
                name: "w1".into(),
                role: "worker".into(),
            },
            1000,
        );
        // Tamper with the entry.
        let mut entries: Vec<JournalEntry> = j.all_entries().to_vec();
        entries[0].timestamp_ms = 9999;
        let engine = RecoveryEngine::new();
        let bad = engine.validate_journal_entries(&entries);
        assert_eq!(bad, vec![0]);
    }

    // --- RecoveryPlan ---

    #[test]
    fn plan_summary_with_checkpoint() {
        let plan = RecoveryPlan {
            checkpoint_id: Some("cp-1".into()),
            checkpoint_sequence: Some(5),
            journal_entries: Vec::new(),
            operations_to_replay: 10,
            estimated_recovery_ms: 60,
        };
        let summary = plan.summary();
        assert!(summary.contains("cp-1"));
        assert!(summary.contains("10 operations"));
    }

    #[test]
    fn plan_summary_without_checkpoint() {
        let plan = RecoveryPlan {
            checkpoint_id: None,
            checkpoint_sequence: None,
            journal_entries: vec![],
            operations_to_replay: 5,
            estimated_recovery_ms: 5,
        };
        let summary = plan.summary();
        assert!(summary.contains("no checkpoint"));
    }

    #[test]
    fn plan_summary_empty() {
        let plan = RecoveryPlan {
            checkpoint_id: None,
            checkpoint_sequence: None,
            journal_entries: Vec::new(),
            operations_to_replay: 0,
            estimated_recovery_ms: 0,
        };
        assert_eq!(plan.summary(), "nothing to recover");
    }

    // --- RecoveryResult ---

    #[test]
    fn recovery_result_ok() {
        let r = RecoveryResult::ok(5, 10, 20, 100);
        assert!(r.success);
        assert_eq!(r.agents_recovered, 5);
        assert_eq!(r.tasks_recovered, 10);
        assert_eq!(r.operations_replayed, 20);
        assert!(!r.has_errors());
    }

    #[test]
    fn recovery_result_failed() {
        let r = RecoveryResult::failed(vec!["bad checksum".into()], 50);
        assert!(!r.success);
        assert!(r.has_errors());
        assert_eq!(r.errors[0], "bad checksum");
    }

    #[test]
    fn recovery_result_serde() {
        let r = RecoveryResult::ok(3, 7, 15, 200);
        let json = serde_json::to_string(&r).unwrap();
        let back: RecoveryResult = serde_json::from_str(&json).unwrap();
        assert!(back.success);
        assert_eq!(back.agents_recovered, 3);
    }

    // --- Default ---

    #[test]
    fn recovery_engine_default() {
        let _engine = RecoveryEngine::default();
        // Just ensure it compiles and works.
    }
}
