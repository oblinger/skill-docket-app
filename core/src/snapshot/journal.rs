//! Write-ahead journal for crash recovery.
//!
//! Each state mutation is recorded as a `JournalEntry` before being applied.
//! On crash, the journal is replayed from the last checkpoint to reconstruct
//! the current state. Entries are serialized as JSON lines (one entry per line).

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// JournalOp
// ---------------------------------------------------------------------------

/// An operation recorded in the journal.
///
/// Each variant captures the minimal data needed to replay the operation
/// during recovery.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum JournalOp {
    AgentCreated { name: String, role: String },
    AgentKilled { name: String },
    AgentAssigned { name: String, task: String },
    AgentUnassigned { name: String },
    TaskCreated { id: String, title: String },
    TaskStatusChanged { id: String, from: String, to: String },
    TaskCompleted { id: String, result: String },
    MessageSent { from: String, to: String, text: String },
    ConfigChanged { key: String, old: String, new_val: String },
    SessionCreated { name: String },
    SessionKilled { name: String },
}

// ---------------------------------------------------------------------------
// JournalEntry
// ---------------------------------------------------------------------------

/// A single journal entry with sequence number and checksum.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct JournalEntry {
    pub sequence: u64,
    pub timestamp_ms: u64,
    pub operation: JournalOp,
    pub checksum: String,
}

impl JournalEntry {
    /// Compute a checksum for this entry based on its content.
    fn compute_checksum(sequence: u64, timestamp_ms: u64, op: &JournalOp) -> String {
        let op_json = serde_json::to_string(op).unwrap_or_default();
        let data = format!("{}:{}:{}", sequence, timestamp_ms, op_json);
        let hash = fnv1a_hash(data.as_bytes());
        format!("{:016x}", hash)
    }

    /// Verify that the stored checksum matches the computed checksum.
    pub fn verify_checksum(&self) -> bool {
        let expected = Self::compute_checksum(self.sequence, self.timestamp_ms, &self.operation);
        self.checksum == expected
    }
}

// ---------------------------------------------------------------------------
// Journal
// ---------------------------------------------------------------------------

/// Write-ahead journal that records state mutations as an ordered sequence
/// of entries.
#[derive(Debug, Clone)]
pub struct Journal {
    entries: Vec<JournalEntry>,
    next_sequence: u64,
    max_entries: usize,
}

impl Journal {
    /// Create a new empty journal with the given maximum entry capacity.
    pub fn new(max_entries: usize) -> Self {
        Journal {
            entries: Vec::new(),
            next_sequence: 0,
            max_entries,
        }
    }

    /// Append a new operation to the journal.
    ///
    /// Returns the sequence number assigned to this entry.
    pub fn append(&mut self, op: JournalOp, now_ms: u64) -> u64 {
        let seq = self.next_sequence;
        let checksum = JournalEntry::compute_checksum(seq, now_ms, &op);
        let entry = JournalEntry {
            sequence: seq,
            timestamp_ms: now_ms,
            operation: op,
            checksum,
        };
        self.entries.push(entry);
        self.next_sequence += 1;

        // Auto-truncate if over capacity.
        if self.entries.len() > self.max_entries {
            let excess = self.entries.len() - self.max_entries;
            self.entries.drain(0..excess);
        }

        seq
    }

    /// Return all entries with a sequence number >= the given value.
    pub fn entries_since(&self, sequence: u64) -> Vec<&JournalEntry> {
        self.entries
            .iter()
            .filter(|e| e.sequence >= sequence)
            .collect()
    }

    /// Return all entries with a timestamp > the given value.
    pub fn entries_after(&self, ms: u64) -> Vec<&JournalEntry> {
        self.entries
            .iter()
            .filter(|e| e.timestamp_ms > ms)
            .collect()
    }

    /// Return the last `n` entries, ordered by sequence.
    pub fn latest(&self, n: usize) -> Vec<&JournalEntry> {
        let start = if self.entries.len() > n {
            self.entries.len() - n
        } else {
            0
        };
        self.entries[start..].iter().collect()
    }

    /// Remove all entries with a sequence number < the given value.
    pub fn truncate_before(&mut self, sequence: u64) {
        self.entries.retain(|e| e.sequence >= sequence);
    }

    /// Compact the journal by removing consecutive duplicate operations.
    ///
    /// Keeps the latest entry for each run of identical operations (by op
    /// variant and key fields). This is a simple compaction — it does not
    /// merge operations semantically.
    pub fn compact(&mut self) {
        if self.entries.len() <= 1 {
            return;
        }

        let mut compacted: Vec<JournalEntry> = Vec::new();
        for entry in &self.entries {
            let dominated = if let Some(last) = compacted.last() {
                ops_are_redundant(&last.operation, &entry.operation)
            } else {
                false
            };
            if dominated {
                // Replace the last entry with this newer one.
                compacted.pop();
            }
            compacted.push(entry.clone());
        }
        self.entries = compacted;
    }

    /// Extract all operations from the journal in sequence order.
    pub fn replay_ops(&self) -> Vec<JournalOp> {
        self.entries.iter().map(|e| e.operation.clone()).collect()
    }

    /// The number of entries currently in the journal.
    pub fn size(&self) -> usize {
        self.entries.len()
    }

    /// The next sequence number that will be assigned.
    pub fn next_seq(&self) -> u64 {
        self.next_sequence
    }

    /// Whether the journal has no entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Get a reference to all entries.
    pub fn all_entries(&self) -> &[JournalEntry] {
        &self.entries
    }

    // -------------------------------------------------------------------
    // JSON lines serialization
    // -------------------------------------------------------------------

    /// Serialize the journal to JSON lines format (one JSON object per line).
    pub fn to_json_lines(&self) -> String {
        self.entries
            .iter()
            .map(|e| serde_json::to_string(e).unwrap_or_default())
            .collect::<Vec<String>>()
            .join("\n")
    }

    /// Deserialize a journal from JSON lines format.
    ///
    /// Blank lines are skipped. Invalid lines produce an error.
    pub fn from_json_lines(data: &str, max_entries: usize) -> Result<Self, String> {
        let mut entries: Vec<JournalEntry> = Vec::new();
        for (i, line) in data.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let entry: JournalEntry = serde_json::from_str(trimmed)
                .map_err(|e| format!("journal line {} parse error: {}", i + 1, e))?;
            entries.push(entry);
        }

        let next_sequence = entries.last().map(|e| e.sequence + 1).unwrap_or(0);
        Ok(Journal {
            entries,
            next_sequence,
            max_entries,
        })
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Check if two operations are redundant (same type and same key).
///
/// Consecutive status changes to the same entity can be compacted — only
/// the latest matters.
fn ops_are_redundant(prev: &JournalOp, next: &JournalOp) -> bool {
    match (prev, next) {
        (
            JournalOp::TaskStatusChanged { id: id1, .. },
            JournalOp::TaskStatusChanged { id: id2, .. },
        ) => id1 == id2,
        (
            JournalOp::ConfigChanged { key: k1, .. },
            JournalOp::ConfigChanged { key: k2, .. },
        ) => k1 == k2,
        _ => false,
    }
}

/// FNV-1a 64-bit hash.
fn fnv1a_hash(data: &[u8]) -> u64 {
    const FNV_OFFSET: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;
    let mut hash = FNV_OFFSET;
    for &byte in data {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn agent_created(name: &str, role: &str) -> JournalOp {
        JournalOp::AgentCreated {
            name: name.into(),
            role: role.into(),
        }
    }

    fn agent_killed(name: &str) -> JournalOp {
        JournalOp::AgentKilled { name: name.into() }
    }

    fn agent_assigned(name: &str, task: &str) -> JournalOp {
        JournalOp::AgentAssigned {
            name: name.into(),
            task: task.into(),
        }
    }

    fn task_created(id: &str, title: &str) -> JournalOp {
        JournalOp::TaskCreated {
            id: id.into(),
            title: title.into(),
        }
    }

    fn task_status_changed(id: &str, from: &str, to: &str) -> JournalOp {
        JournalOp::TaskStatusChanged {
            id: id.into(),
            from: from.into(),
            to: to.into(),
        }
    }

    fn task_completed(id: &str, result: &str) -> JournalOp {
        JournalOp::TaskCompleted {
            id: id.into(),
            result: result.into(),
        }
    }

    fn message_sent(from: &str, to: &str, text: &str) -> JournalOp {
        JournalOp::MessageSent {
            from: from.into(),
            to: to.into(),
            text: text.into(),
        }
    }

    fn config_changed(key: &str, old: &str, new_val: &str) -> JournalOp {
        JournalOp::ConfigChanged {
            key: key.into(),
            old: old.into(),
            new_val: new_val.into(),
        }
    }

    fn session_created(name: &str) -> JournalOp {
        JournalOp::SessionCreated { name: name.into() }
    }

    fn session_killed(name: &str) -> JournalOp {
        JournalOp::SessionKilled { name: name.into() }
    }

    // --- Basic append/size ---

    #[test]
    fn new_journal_is_empty() {
        let j = Journal::new(100);
        assert!(j.is_empty());
        assert_eq!(j.size(), 0);
        assert_eq!(j.next_seq(), 0);
    }

    #[test]
    fn append_increments_sequence() {
        let mut j = Journal::new(100);
        let s0 = j.append(agent_created("w1", "worker"), 1000);
        let s1 = j.append(agent_created("w2", "worker"), 2000);
        assert_eq!(s0, 0);
        assert_eq!(s1, 1);
        assert_eq!(j.size(), 2);
        assert_eq!(j.next_seq(), 2);
    }

    #[test]
    fn append_auto_truncates() {
        let mut j = Journal::new(3);
        j.append(agent_created("a", "r"), 100);
        j.append(agent_created("b", "r"), 200);
        j.append(agent_created("c", "r"), 300);
        j.append(agent_created("d", "r"), 400);
        assert_eq!(j.size(), 3);
        // Oldest entry (a) should be gone.
        assert!(j.entries_since(0).iter().all(|e| {
            if let JournalOp::AgentCreated { name, .. } = &e.operation {
                name != "a"
            } else {
                true
            }
        }));
    }

    // --- entries_since ---

    #[test]
    fn entries_since_returns_matching() {
        let mut j = Journal::new(100);
        j.append(agent_created("a", "r"), 100);
        j.append(agent_created("b", "r"), 200);
        j.append(agent_created("c", "r"), 300);

        let since1 = j.entries_since(1);
        assert_eq!(since1.len(), 2);
        assert_eq!(since1[0].sequence, 1);
        assert_eq!(since1[1].sequence, 2);
    }

    #[test]
    fn entries_since_zero_returns_all() {
        let mut j = Journal::new(100);
        j.append(agent_created("a", "r"), 100);
        j.append(agent_created("b", "r"), 200);
        assert_eq!(j.entries_since(0).len(), 2);
    }

    #[test]
    fn entries_since_future_returns_empty() {
        let mut j = Journal::new(100);
        j.append(agent_created("a", "r"), 100);
        assert!(j.entries_since(999).is_empty());
    }

    // --- entries_after ---

    #[test]
    fn entries_after_returns_matching() {
        let mut j = Journal::new(100);
        j.append(agent_created("a", "r"), 100);
        j.append(agent_created("b", "r"), 200);
        j.append(agent_created("c", "r"), 300);

        let after = j.entries_after(150);
        assert_eq!(after.len(), 2);
    }

    #[test]
    fn entries_after_exclusive() {
        let mut j = Journal::new(100);
        j.append(agent_created("a", "r"), 100);
        j.append(agent_created("b", "r"), 200);
        let after = j.entries_after(100);
        assert_eq!(after.len(), 1);
        assert_eq!(after[0].timestamp_ms, 200);
    }

    // --- latest ---

    #[test]
    fn latest_returns_last_n() {
        let mut j = Journal::new(100);
        j.append(agent_created("a", "r"), 100);
        j.append(agent_created("b", "r"), 200);
        j.append(agent_created("c", "r"), 300);

        let last2 = j.latest(2);
        assert_eq!(last2.len(), 2);
        assert_eq!(last2[0].sequence, 1);
        assert_eq!(last2[1].sequence, 2);
    }

    #[test]
    fn latest_more_than_size_returns_all() {
        let mut j = Journal::new(100);
        j.append(agent_created("a", "r"), 100);
        assert_eq!(j.latest(10).len(), 1);
    }

    #[test]
    fn latest_zero_returns_empty() {
        let mut j = Journal::new(100);
        j.append(agent_created("a", "r"), 100);
        assert!(j.latest(0).is_empty());
    }

    // --- truncate_before ---

    #[test]
    fn truncate_before_removes_old() {
        let mut j = Journal::new(100);
        j.append(agent_created("a", "r"), 100);
        j.append(agent_created("b", "r"), 200);
        j.append(agent_created("c", "r"), 300);

        j.truncate_before(2);
        assert_eq!(j.size(), 1);
        assert_eq!(j.all_entries()[0].sequence, 2);
    }

    #[test]
    fn truncate_before_zero_keeps_all() {
        let mut j = Journal::new(100);
        j.append(agent_created("a", "r"), 100);
        j.truncate_before(0);
        assert_eq!(j.size(), 1);
    }

    #[test]
    fn truncate_before_future_removes_all() {
        let mut j = Journal::new(100);
        j.append(agent_created("a", "r"), 100);
        j.truncate_before(999);
        assert!(j.is_empty());
    }

    // --- compact ---

    #[test]
    fn compact_removes_redundant_status_changes() {
        let mut j = Journal::new(100);
        j.append(task_status_changed("T1", "pending", "in_progress"), 100);
        j.append(task_status_changed("T1", "in_progress", "completed"), 200);
        j.append(task_completed("T1", "done"), 300);

        j.compact();
        assert_eq!(j.size(), 2); // second status change + completed
    }

    #[test]
    fn compact_keeps_different_task_status_changes() {
        let mut j = Journal::new(100);
        j.append(task_status_changed("T1", "pending", "in_progress"), 100);
        j.append(task_status_changed("T2", "pending", "in_progress"), 200);

        j.compact();
        assert_eq!(j.size(), 2); // different tasks, both kept
    }

    #[test]
    fn compact_removes_redundant_config_changes() {
        let mut j = Journal::new(100);
        j.append(config_changed("timeout", "5000", "10000"), 100);
        j.append(config_changed("timeout", "10000", "15000"), 200);

        j.compact();
        assert_eq!(j.size(), 1);
    }

    #[test]
    fn compact_keeps_different_config_keys() {
        let mut j = Journal::new(100);
        j.append(config_changed("timeout", "5000", "10000"), 100);
        j.append(config_changed("retries", "3", "5"), 200);

        j.compact();
        assert_eq!(j.size(), 2);
    }

    #[test]
    fn compact_noop_on_empty() {
        let mut j = Journal::new(100);
        j.compact();
        assert!(j.is_empty());
    }

    #[test]
    fn compact_noop_on_single_entry() {
        let mut j = Journal::new(100);
        j.append(agent_created("a", "r"), 100);
        j.compact();
        assert_eq!(j.size(), 1);
    }

    // --- replay_ops ---

    #[test]
    fn replay_ops_returns_all_operations() {
        let mut j = Journal::new(100);
        j.append(agent_created("w1", "worker"), 100);
        j.append(agent_assigned("w1", "T1"), 200);
        j.append(task_created("T1", "Task one"), 300);

        let ops = j.replay_ops();
        assert_eq!(ops.len(), 3);
        assert_eq!(
            ops[0],
            JournalOp::AgentCreated {
                name: "w1".into(),
                role: "worker".into()
            }
        );
    }

    #[test]
    fn replay_ops_empty_journal() {
        let j = Journal::new(100);
        assert!(j.replay_ops().is_empty());
    }

    // --- JSON lines round-trip ---

    #[test]
    fn json_lines_round_trip() {
        let mut j = Journal::new(100);
        j.append(agent_created("w1", "worker"), 100);
        j.append(task_created("T1", "Task"), 200);
        j.append(message_sent("pilot", "w1", "do the thing"), 300);

        let lines = j.to_json_lines();
        let back = Journal::from_json_lines(&lines, 100).unwrap();
        assert_eq!(back.size(), 3);
        assert_eq!(back.next_seq(), 3);
    }

    #[test]
    fn json_lines_with_all_op_types() {
        let mut j = Journal::new(100);
        j.append(agent_created("w1", "worker"), 100);
        j.append(agent_killed("w1"), 200);
        j.append(agent_assigned("w2", "T1"), 300);
        j.append(JournalOp::AgentUnassigned { name: "w2".into() }, 400);
        j.append(task_created("T1", "Task"), 500);
        j.append(task_status_changed("T1", "pending", "done"), 600);
        j.append(task_completed("T1", "success"), 700);
        j.append(message_sent("a", "b", "hi"), 800);
        j.append(config_changed("k", "v1", "v2"), 900);
        j.append(session_created("main"), 1000);
        j.append(session_killed("main"), 1100);

        let lines = j.to_json_lines();
        let back = Journal::from_json_lines(&lines, 100).unwrap();
        assert_eq!(back.size(), 11);
    }

    #[test]
    fn json_lines_skips_blank_lines() {
        let data = "\n  \n";
        let j = Journal::from_json_lines(data, 100).unwrap();
        assert!(j.is_empty());
    }

    #[test]
    fn json_lines_error_on_invalid() {
        let result = Journal::from_json_lines("not valid json", 100);
        assert!(result.is_err());
    }

    #[test]
    fn json_lines_preserves_sequence_order() {
        let mut j = Journal::new(100);
        j.append(agent_created("a", "r"), 100);
        j.append(agent_created("b", "r"), 200);
        j.append(agent_created("c", "r"), 300);

        let lines = j.to_json_lines();
        let back = Journal::from_json_lines(&lines, 100).unwrap();
        let entries = back.all_entries();
        assert_eq!(entries[0].sequence, 0);
        assert_eq!(entries[1].sequence, 1);
        assert_eq!(entries[2].sequence, 2);
    }

    // --- Checksum verification ---

    #[test]
    fn entry_checksum_verifies() {
        let mut j = Journal::new(100);
        j.append(agent_created("w1", "worker"), 1000);
        let entry = &j.all_entries()[0];
        assert!(entry.verify_checksum());
    }

    #[test]
    fn tampered_entry_fails_checksum() {
        let mut j = Journal::new(100);
        j.append(agent_created("w1", "worker"), 1000);
        let mut entry = j.all_entries()[0].clone();
        entry.timestamp_ms = 9999;
        assert!(!entry.verify_checksum());
    }

    // --- Op serde ---

    #[test]
    fn journal_op_tagged_serde() {
        let op = agent_created("w1", "worker");
        let json = serde_json::to_string(&op).unwrap();
        assert!(json.contains("\"op\":\"agent_created\""));
        let back: JournalOp = serde_json::from_str(&json).unwrap();
        assert_eq!(back, op);
    }

    #[test]
    fn all_ops_round_trip_serde() {
        let ops = vec![
            agent_created("w1", "worker"),
            agent_killed("w1"),
            agent_assigned("w2", "T1"),
            JournalOp::AgentUnassigned { name: "w2".into() },
            task_created("T1", "Task"),
            task_status_changed("T1", "pending", "done"),
            task_completed("T1", "ok"),
            message_sent("a", "b", "hi"),
            config_changed("k", "v1", "v2"),
            session_created("main"),
            session_killed("main"),
        ];
        for op in ops {
            let json = serde_json::to_string(&op).unwrap();
            let back: JournalOp = serde_json::from_str(&json).unwrap();
            assert_eq!(back, op);
        }
    }

    // --- Edge cases ---

    #[test]
    fn append_many_entries() {
        let mut j = Journal::new(10000);
        for i in 0..1000 {
            j.append(agent_created(&format!("a{}", i), "r"), i * 100);
        }
        assert_eq!(j.size(), 1000);
        assert_eq!(j.next_seq(), 1000);
    }

    #[test]
    fn entries_since_after_truncate() {
        let mut j = Journal::new(100);
        j.append(agent_created("a", "r"), 100);
        j.append(agent_created("b", "r"), 200);
        j.append(agent_created("c", "r"), 300);
        j.truncate_before(1);
        assert_eq!(j.entries_since(0).len(), 2); // entries 1 and 2
    }

    #[test]
    fn compact_mixed_operations() {
        let mut j = Journal::new(100);
        j.append(agent_created("w1", "worker"), 100);
        j.append(task_status_changed("T1", "pending", "in_progress"), 200);
        j.append(agent_assigned("w1", "T1"), 300);
        j.append(task_status_changed("T1", "in_progress", "completed"), 400);
        j.append(agent_killed("w1"), 500);

        j.compact();
        // Status changes to T1 are consecutive entries 1 and 3, but entry 2
        // (agent_assigned) breaks the run, so no compaction occurs.
        assert_eq!(j.size(), 5);
    }

    #[test]
    fn compact_consecutive_same_task_status() {
        let mut j = Journal::new(100);
        j.append(task_status_changed("T1", "pending", "in_progress"), 100);
        j.append(task_status_changed("T1", "in_progress", "completed"), 200);

        j.compact();
        assert_eq!(j.size(), 1);
        if let JournalOp::TaskStatusChanged { to, .. } = &j.all_entries()[0].operation {
            assert_eq!(to, "completed");
        } else {
            panic!("expected TaskStatusChanged");
        }
    }
}
