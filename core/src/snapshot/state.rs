//! Full system state snapshot — captures agents, tasks, and sessions at a
//! point in time.
//!
//! `SystemSnapshot` is the serializable root type. Builder methods construct
//! snapshots from domain collections, and utility methods provide metadata,
//! checksums, and consistency validation.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// AgentSnapshot
// ---------------------------------------------------------------------------

/// Snapshot of a single agent's state at a point in time.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentSnapshot {
    pub name: String,
    pub role: String,
    pub agent_type: String,
    pub status: String,
    pub task: Option<String>,
    pub path: String,
    pub health: String,
    pub last_heartbeat_ms: Option<u64>,
}

// ---------------------------------------------------------------------------
// TaskSnapshot
// ---------------------------------------------------------------------------

/// Snapshot of a single task node's state.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskSnapshot {
    pub id: String,
    pub title: String,
    pub status: String,
    pub source: String,
    pub agent: Option<String>,
    pub result: Option<String>,
    pub children_ids: Vec<String>,
    pub spec_path: Option<String>,
}

// ---------------------------------------------------------------------------
// SessionSnapshot
// ---------------------------------------------------------------------------

/// Snapshot of a tmux session.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionSnapshot {
    pub name: String,
    pub window_count: usize,
    pub pane_count: usize,
    pub agents_placed: Vec<String>,
}

// ---------------------------------------------------------------------------
// SnapshotMetadata
// ---------------------------------------------------------------------------

/// Lightweight metadata about a snapshot, suitable for listings and indexes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SnapshotMetadata {
    pub id: String,
    pub timestamp_ms: u64,
    pub size_bytes: usize,
    pub agent_count: usize,
    pub task_count: usize,
    pub session_count: usize,
    pub checksum: String,
}

// ---------------------------------------------------------------------------
// SystemSnapshot
// ---------------------------------------------------------------------------

/// Complete snapshot of the CMX system state at a point in time.
///
/// All fields are plain data — no references to runtime objects. This makes
/// the snapshot trivially serializable and suitable for persistence.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SystemSnapshot {
    pub version: String,
    pub timestamp_ms: u64,
    pub agents: Vec<AgentSnapshot>,
    pub tasks: Vec<TaskSnapshot>,
    pub sessions: Vec<SessionSnapshot>,
    pub settings_hash: String,
    pub message_count: usize,
}

impl SystemSnapshot {
    /// Create a new empty snapshot with the given version and timestamp.
    pub fn new(version: &str, timestamp_ms: u64) -> Self {
        SystemSnapshot {
            version: version.to_string(),
            timestamp_ms,
            agents: Vec::new(),
            tasks: Vec::new(),
            sessions: Vec::new(),
            settings_hash: String::new(),
            message_count: 0,
        }
    }

    // -------------------------------------------------------------------
    // Builders
    // -------------------------------------------------------------------

    /// Set the agents list from a vec of `AgentSnapshot`.
    pub fn with_agents(mut self, agents: Vec<AgentSnapshot>) -> Self {
        self.agents = agents;
        self
    }

    /// Set the tasks list from a vec of `TaskSnapshot`.
    pub fn with_tasks(mut self, tasks: Vec<TaskSnapshot>) -> Self {
        self.tasks = tasks;
        self
    }

    /// Set the sessions list from a vec of `SessionSnapshot`.
    pub fn with_sessions(mut self, sessions: Vec<SessionSnapshot>) -> Self {
        self.sessions = sessions;
        self
    }

    /// Set the settings hash.
    pub fn with_settings_hash(mut self, hash: &str) -> Self {
        self.settings_hash = hash.to_string();
        self
    }

    /// Set the message count.
    pub fn with_message_count(mut self, count: usize) -> Self {
        self.message_count = count;
        self
    }

    // -------------------------------------------------------------------
    // Metadata and checksums
    // -------------------------------------------------------------------

    /// Generate metadata for this snapshot.
    pub fn metadata(&self) -> SnapshotMetadata {
        let json = self.to_json();
        let size_bytes = json.len();
        let checksum = self.checksum_sha256();
        SnapshotMetadata {
            id: format!("snap-{}", self.timestamp_ms),
            timestamp_ms: self.timestamp_ms,
            size_bytes,
            agent_count: self.agents.len(),
            task_count: self.tasks.len(),
            session_count: self.sessions.len(),
            checksum,
        }
    }

    /// Compute a simple checksum of the snapshot content.
    ///
    /// Uses a deterministic hash of the JSON representation. This is not
    /// cryptographic SHA-256 — we avoid external crate dependencies. Instead
    /// we use a simple FNV-1a hash formatted as hex.
    pub fn checksum_sha256(&self) -> String {
        let json = serde_json::to_string(self).unwrap_or_default();
        let hash = fnv1a_hash(json.as_bytes());
        format!("{:016x}", hash)
    }

    /// Convenience alias for `checksum_sha256()`.
    pub fn checksum(&self) -> String {
        self.checksum_sha256()
    }

    // -------------------------------------------------------------------
    // Serialization
    // -------------------------------------------------------------------

    /// Serialize this snapshot to a JSON string.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    /// Serialize this snapshot to a pretty-printed JSON string.
    pub fn to_json_pretty(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default()
    }

    /// Deserialize a snapshot from a JSON string.
    pub fn from_json(json: &str) -> Result<Self, String> {
        serde_json::from_str(json).map_err(|e| format!("snapshot parse error: {}", e))
    }

    // -------------------------------------------------------------------
    // Validation
    // -------------------------------------------------------------------

    /// Check whether this snapshot is internally consistent.
    ///
    /// Validates:
    /// - No duplicate agent names
    /// - No duplicate task IDs
    /// - No duplicate session names
    /// - Agent task references point to existing tasks
    /// - Task agent references point to existing agents
    /// - Session agent references point to existing agents
    /// - Children IDs reference existing tasks
    pub fn is_consistent(&self) -> bool {
        // Check for duplicate agent names.
        let agent_names: Vec<&str> = self.agents.iter().map(|a| a.name.as_str()).collect();
        if has_duplicates(&agent_names) {
            return false;
        }

        // Check for duplicate task IDs.
        let task_ids: Vec<&str> = self.tasks.iter().map(|t| t.id.as_str()).collect();
        if has_duplicates(&task_ids) {
            return false;
        }

        // Check for duplicate session names.
        let session_names: Vec<&str> = self.sessions.iter().map(|s| s.name.as_str()).collect();
        if has_duplicates(&session_names) {
            return false;
        }

        // Build lookup sets.
        let agent_set: HashMap<&str, ()> =
            agent_names.iter().map(|n| (*n, ())).collect();
        let task_set: HashMap<&str, ()> =
            task_ids.iter().map(|id| (*id, ())).collect();

        // Agent task references must exist.
        for agent in &self.agents {
            if let Some(ref task_id) = agent.task {
                if !task_set.contains_key(task_id.as_str()) {
                    return false;
                }
            }
        }

        // Task agent references must exist.
        for task in &self.tasks {
            if let Some(ref agent_name) = task.agent {
                if !agent_set.contains_key(agent_name.as_str()) {
                    return false;
                }
            }
        }

        // Task children_ids must reference existing tasks.
        for task in &self.tasks {
            for child_id in &task.children_ids {
                if !task_set.contains_key(child_id.as_str()) {
                    return false;
                }
            }
        }

        // Session agent references must exist.
        for session in &self.sessions {
            for agent_name in &session.agents_placed {
                if !agent_set.contains_key(agent_name.as_str()) {
                    return false;
                }
            }
        }

        true
    }

    /// Return a list of all agent names in this snapshot.
    pub fn agent_names(&self) -> Vec<&str> {
        self.agents.iter().map(|a| a.name.as_str()).collect()
    }

    /// Return a list of all task IDs in this snapshot.
    pub fn task_ids(&self) -> Vec<&str> {
        self.tasks.iter().map(|t| t.id.as_str()).collect()
    }

    /// Return a list of all session names in this snapshot.
    pub fn session_names(&self) -> Vec<&str> {
        self.sessions.iter().map(|s| s.name.as_str()).collect()
    }

    /// Find an agent snapshot by name.
    pub fn find_agent(&self, name: &str) -> Option<&AgentSnapshot> {
        self.agents.iter().find(|a| a.name == name)
    }

    /// Find a task snapshot by ID.
    pub fn find_task(&self, id: &str) -> Option<&TaskSnapshot> {
        self.tasks.iter().find(|t| t.id == id)
    }

    /// Find a session snapshot by name.
    pub fn find_session(&self, name: &str) -> Option<&SessionSnapshot> {
        self.sessions.iter().find(|s| s.name == name)
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Check if a slice contains duplicate values.
fn has_duplicates(items: &[&str]) -> bool {
    let mut seen: HashMap<&str, ()> = HashMap::new();
    for item in items {
        if seen.contains_key(item) {
            return true;
        }
        seen.insert(item, ());
    }
    false
}

/// FNV-1a 64-bit hash — a fast, non-cryptographic hash function.
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

    // --- Helpers ---

    fn make_agent(name: &str, role: &str, task: Option<&str>) -> AgentSnapshot {
        AgentSnapshot {
            name: name.into(),
            role: role.into(),
            agent_type: "claude".into(),
            status: "idle".into(),
            task: task.map(|t| t.into()),
            path: "/tmp".into(),
            health: "healthy".into(),
            last_heartbeat_ms: Some(1700000000000),
        }
    }

    fn make_task(id: &str, title: &str, agent: Option<&str>) -> TaskSnapshot {
        TaskSnapshot {
            id: id.into(),
            title: title.into(),
            status: "pending".into(),
            source: "roadmap".into(),
            agent: agent.map(|a| a.into()),
            result: None,
            children_ids: Vec::new(),
            spec_path: None,
        }
    }

    fn make_session(name: &str, agents: Vec<&str>) -> SessionSnapshot {
        SessionSnapshot {
            name: name.into(),
            window_count: 1,
            pane_count: agents.len(),
            agents_placed: agents.into_iter().map(|a| a.into()).collect(),
        }
    }

    fn make_snapshot() -> SystemSnapshot {
        SystemSnapshot::new("0.1.0", 1700000000000)
            .with_agents(vec![
                make_agent("pilot", "pilot", Some("CMX1")),
                make_agent("worker-1", "worker", Some("CMX2")),
            ])
            .with_tasks(vec![
                make_task("CMX1", "Core daemon", Some("pilot")),
                make_task("CMX2", "Socket protocol", Some("worker-1")),
            ])
            .with_sessions(vec![make_session("cmx-main", vec!["pilot", "worker-1"])])
            .with_settings_hash("abc123")
            .with_message_count(42)
    }

    // --- Construction ---

    #[test]
    fn new_creates_empty_snapshot() {
        let snap = SystemSnapshot::new("0.1.0", 1000);
        assert_eq!(snap.version, "0.1.0");
        assert_eq!(snap.timestamp_ms, 1000);
        assert!(snap.agents.is_empty());
        assert!(snap.tasks.is_empty());
        assert!(snap.sessions.is_empty());
        assert!(snap.settings_hash.is_empty());
        assert_eq!(snap.message_count, 0);
    }

    #[test]
    fn builder_methods_set_fields() {
        let snap = make_snapshot();
        assert_eq!(snap.agents.len(), 2);
        assert_eq!(snap.tasks.len(), 2);
        assert_eq!(snap.sessions.len(), 1);
        assert_eq!(snap.settings_hash, "abc123");
        assert_eq!(snap.message_count, 42);
    }

    // --- Serialization round-trips ---

    #[test]
    fn snapshot_json_round_trip() {
        let snap = make_snapshot();
        let json = snap.to_json();
        let back = SystemSnapshot::from_json(&json).unwrap();
        assert_eq!(back, snap);
    }

    #[test]
    fn snapshot_pretty_json_round_trip() {
        let snap = make_snapshot();
        let json = snap.to_json_pretty();
        let back = SystemSnapshot::from_json(&json).unwrap();
        assert_eq!(back, snap);
    }

    #[test]
    fn empty_snapshot_round_trip() {
        let snap = SystemSnapshot::new("0.1.0", 0);
        let json = snap.to_json();
        let back = SystemSnapshot::from_json(&json).unwrap();
        assert_eq!(back, snap);
    }

    #[test]
    fn from_json_error_on_invalid() {
        let result = SystemSnapshot::from_json("not json");
        assert!(result.is_err());
    }

    #[test]
    fn from_json_error_on_empty() {
        let result = SystemSnapshot::from_json("");
        assert!(result.is_err());
    }

    #[test]
    fn agent_snapshot_round_trip() {
        let agent = make_agent("w1", "worker", Some("T1"));
        let json = serde_json::to_string(&agent).unwrap();
        let back: AgentSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(back, agent);
    }

    #[test]
    fn task_snapshot_round_trip() {
        let task = make_task("T1", "Task one", Some("w1"));
        let json = serde_json::to_string(&task).unwrap();
        let back: TaskSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(back, task);
    }

    #[test]
    fn session_snapshot_round_trip() {
        let session = make_session("main", vec!["pilot"]);
        let json = serde_json::to_string(&session).unwrap();
        let back: SessionSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(back, session);
    }

    // --- Metadata ---

    #[test]
    fn metadata_reflects_counts() {
        let snap = make_snapshot();
        let meta = snap.metadata();
        assert_eq!(meta.agent_count, 2);
        assert_eq!(meta.task_count, 2);
        assert_eq!(meta.session_count, 1);
        assert_eq!(meta.timestamp_ms, 1700000000000);
        assert!(meta.size_bytes > 0);
        assert!(!meta.checksum.is_empty());
    }

    #[test]
    fn metadata_id_contains_timestamp() {
        let snap = SystemSnapshot::new("0.1.0", 5555);
        let meta = snap.metadata();
        assert_eq!(meta.id, "snap-5555");
    }

    #[test]
    fn metadata_empty_snapshot() {
        let snap = SystemSnapshot::new("0.1.0", 0);
        let meta = snap.metadata();
        assert_eq!(meta.agent_count, 0);
        assert_eq!(meta.task_count, 0);
        assert_eq!(meta.session_count, 0);
    }

    #[test]
    fn metadata_round_trip() {
        let snap = make_snapshot();
        let meta = snap.metadata();
        let json = serde_json::to_string(&meta).unwrap();
        let back: SnapshotMetadata = serde_json::from_str(&json).unwrap();
        assert_eq!(back, meta);
    }

    // --- Checksums ---

    #[test]
    fn checksum_deterministic() {
        let snap = make_snapshot();
        let c1 = snap.checksum_sha256();
        let c2 = snap.checksum_sha256();
        assert_eq!(c1, c2);
    }

    #[test]
    fn checksum_changes_with_content() {
        let snap1 = make_snapshot();
        let snap2 = SystemSnapshot::new("0.1.0", 1700000000000)
            .with_agents(vec![make_agent("different", "worker", None)]);
        assert_ne!(snap1.checksum_sha256(), snap2.checksum_sha256());
    }

    #[test]
    fn checksum_hex_format() {
        let snap = make_snapshot();
        let checksum = snap.checksum_sha256();
        assert_eq!(checksum.len(), 16);
        assert!(checksum.chars().all(|c| c.is_ascii_hexdigit()));
    }

    // --- Consistency ---

    #[test]
    fn consistent_snapshot_passes() {
        let snap = make_snapshot();
        assert!(snap.is_consistent());
    }

    #[test]
    fn empty_snapshot_is_consistent() {
        let snap = SystemSnapshot::new("0.1.0", 0);
        assert!(snap.is_consistent());
    }

    #[test]
    fn duplicate_agent_names_inconsistent() {
        let snap = SystemSnapshot::new("0.1.0", 0).with_agents(vec![
            make_agent("dupe", "worker", None),
            make_agent("dupe", "pilot", None),
        ]);
        assert!(!snap.is_consistent());
    }

    #[test]
    fn duplicate_task_ids_inconsistent() {
        let snap = SystemSnapshot::new("0.1.0", 0).with_tasks(vec![
            make_task("T1", "First", None),
            make_task("T1", "Second", None),
        ]);
        assert!(!snap.is_consistent());
    }

    #[test]
    fn duplicate_session_names_inconsistent() {
        let snap = SystemSnapshot::new("0.1.0", 0).with_sessions(vec![
            make_session("main", vec![]),
            make_session("main", vec![]),
        ]);
        assert!(!snap.is_consistent());
    }

    #[test]
    fn agent_references_missing_task_inconsistent() {
        let snap = SystemSnapshot::new("0.1.0", 0)
            .with_agents(vec![make_agent("w1", "worker", Some("NONEXISTENT"))]);
        assert!(!snap.is_consistent());
    }

    #[test]
    fn task_references_missing_agent_inconsistent() {
        let snap = SystemSnapshot::new("0.1.0", 0)
            .with_tasks(vec![make_task("T1", "Task", Some("NONEXISTENT"))]);
        assert!(!snap.is_consistent());
    }

    #[test]
    fn session_references_missing_agent_inconsistent() {
        let snap = SystemSnapshot::new("0.1.0", 0)
            .with_sessions(vec![make_session("main", vec!["NONEXISTENT"])]);
        assert!(!snap.is_consistent());
    }

    #[test]
    fn children_ids_reference_missing_task_inconsistent() {
        let mut task = make_task("T1", "Parent", None);
        task.children_ids = vec!["NONEXISTENT".into()];
        let snap = SystemSnapshot::new("0.1.0", 0).with_tasks(vec![task]);
        assert!(!snap.is_consistent());
    }

    #[test]
    fn children_ids_reference_existing_task_consistent() {
        let mut parent = make_task("T1", "Parent", None);
        parent.children_ids = vec!["T2".into()];
        let child = make_task("T2", "Child", None);
        let snap = SystemSnapshot::new("0.1.0", 0).with_tasks(vec![parent, child]);
        assert!(snap.is_consistent());
    }

    #[test]
    fn none_references_are_consistent() {
        let snap = SystemSnapshot::new("0.1.0", 0)
            .with_agents(vec![make_agent("w1", "worker", None)])
            .with_tasks(vec![make_task("T1", "Task", None)])
            .with_sessions(vec![make_session("main", vec!["w1"])]);
        assert!(snap.is_consistent());
    }

    // --- Lookup methods ---

    #[test]
    fn find_agent_by_name() {
        let snap = make_snapshot();
        let agent = snap.find_agent("pilot").unwrap();
        assert_eq!(agent.role, "pilot");
    }

    #[test]
    fn find_agent_missing() {
        let snap = make_snapshot();
        assert!(snap.find_agent("nonexistent").is_none());
    }

    #[test]
    fn find_task_by_id() {
        let snap = make_snapshot();
        let task = snap.find_task("CMX1").unwrap();
        assert_eq!(task.title, "Core daemon");
    }

    #[test]
    fn find_task_missing() {
        let snap = make_snapshot();
        assert!(snap.find_task("NOPE").is_none());
    }

    #[test]
    fn find_session_by_name() {
        let snap = make_snapshot();
        let session = snap.find_session("cmx-main").unwrap();
        assert_eq!(session.window_count, 1);
    }

    #[test]
    fn find_session_missing() {
        let snap = make_snapshot();
        assert!(snap.find_session("nope").is_none());
    }

    // --- Name/ID listing ---

    #[test]
    fn agent_names_returns_all() {
        let snap = make_snapshot();
        let names = snap.agent_names();
        assert_eq!(names.len(), 2);
        assert!(names.contains(&"pilot"));
        assert!(names.contains(&"worker-1"));
    }

    #[test]
    fn task_ids_returns_all() {
        let snap = make_snapshot();
        let ids = snap.task_ids();
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&"CMX1"));
        assert!(ids.contains(&"CMX2"));
    }

    #[test]
    fn session_names_returns_all() {
        let snap = make_snapshot();
        let names = snap.session_names();
        assert_eq!(names, vec!["cmx-main"]);
    }

    // --- Large snapshot ---

    #[test]
    fn large_snapshot_round_trip() {
        let agents: Vec<AgentSnapshot> = (0..100)
            .map(|i| make_agent(&format!("agent-{}", i), "worker", None))
            .collect();
        let tasks: Vec<TaskSnapshot> = (0..200)
            .map(|i| make_task(&format!("T{}", i), &format!("Task {}", i), None))
            .collect();
        let snap = SystemSnapshot::new("0.1.0", 999)
            .with_agents(agents)
            .with_tasks(tasks);
        let json = snap.to_json();
        let back = SystemSnapshot::from_json(&json).unwrap();
        assert_eq!(back.agents.len(), 100);
        assert_eq!(back.tasks.len(), 200);
        assert!(back.is_consistent());
    }

    #[test]
    fn large_snapshot_metadata() {
        let agents: Vec<AgentSnapshot> = (0..50)
            .map(|i| make_agent(&format!("a-{}", i), "worker", None))
            .collect();
        let snap = SystemSnapshot::new("0.1.0", 999).with_agents(agents);
        let meta = snap.metadata();
        assert_eq!(meta.agent_count, 50);
    }

    // --- FNV hash ---

    #[test]
    fn fnv_hash_deterministic() {
        let h1 = fnv1a_hash(b"hello world");
        let h2 = fnv1a_hash(b"hello world");
        assert_eq!(h1, h2);
    }

    #[test]
    fn fnv_hash_differs_for_different_input() {
        let h1 = fnv1a_hash(b"hello");
        let h2 = fnv1a_hash(b"world");
        assert_ne!(h1, h2);
    }

    #[test]
    fn fnv_hash_empty_input() {
        let h = fnv1a_hash(b"");
        // Should return the FNV offset basis.
        assert_eq!(h, 0xcbf29ce484222325);
    }
}
