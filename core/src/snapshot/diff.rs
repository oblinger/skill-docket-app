//! State diff computation — compare two `SystemSnapshot` instances and
//! produce a structured description of what changed.
//!
//! The diff is computed at the entity level (agents, tasks, sessions) and
//! at the field level within changed entities. This is used for audit
//! logging, UI updates, and convergence detection.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::state::SystemSnapshot;

// ---------------------------------------------------------------------------
// FieldChange
// ---------------------------------------------------------------------------

/// A change to a single field within an entity.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FieldChange {
    pub field: String,
    pub old_value: String,
    pub new_value: String,
}

// ---------------------------------------------------------------------------
// AgentDiff
// ---------------------------------------------------------------------------

/// Changes detected in a specific agent between two snapshots.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentDiff {
    pub name: String,
    pub changes: Vec<FieldChange>,
}

// ---------------------------------------------------------------------------
// TaskDiff
// ---------------------------------------------------------------------------

/// Changes detected in a specific task between two snapshots.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskDiff {
    pub id: String,
    pub changes: Vec<FieldChange>,
}

// ---------------------------------------------------------------------------
// SnapshotDiff
// ---------------------------------------------------------------------------

/// The complete diff between two `SystemSnapshot` instances.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SnapshotDiff {
    pub agents_added: Vec<String>,
    pub agents_removed: Vec<String>,
    pub agents_changed: Vec<AgentDiff>,
    pub tasks_added: Vec<String>,
    pub tasks_removed: Vec<String>,
    pub tasks_changed: Vec<TaskDiff>,
    pub sessions_added: Vec<String>,
    pub sessions_removed: Vec<String>,
}

impl SnapshotDiff {
    /// Compute the diff between two snapshots.
    pub fn compute(old: &SystemSnapshot, new: &SystemSnapshot) -> Self {
        let (agents_added, agents_removed, agents_changed) = diff_agents(old, new);
        let (tasks_added, tasks_removed, tasks_changed) = diff_tasks(old, new);
        let (sessions_added, sessions_removed) = diff_sessions(old, new);

        SnapshotDiff {
            agents_added,
            agents_removed,
            agents_changed,
            tasks_added,
            tasks_removed,
            tasks_changed,
            sessions_added,
            sessions_removed,
        }
    }

    /// Whether the diff contains no changes at all.
    pub fn is_empty(&self) -> bool {
        self.agents_added.is_empty()
            && self.agents_removed.is_empty()
            && self.agents_changed.is_empty()
            && self.tasks_added.is_empty()
            && self.tasks_removed.is_empty()
            && self.tasks_changed.is_empty()
            && self.sessions_added.is_empty()
            && self.sessions_removed.is_empty()
    }

    /// Total number of individual changes across all categories.
    pub fn change_count(&self) -> usize {
        self.agents_added.len()
            + self.agents_removed.len()
            + self.agents_changed.iter().map(|d| d.changes.len()).sum::<usize>()
            + self.tasks_added.len()
            + self.tasks_removed.len()
            + self.tasks_changed.iter().map(|d| d.changes.len()).sum::<usize>()
            + self.sessions_added.len()
            + self.sessions_removed.len()
    }

    /// Produce a human-readable summary of the diff.
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();

        if !self.agents_added.is_empty() {
            parts.push(format!("{} agent(s) added", self.agents_added.len()));
        }
        if !self.agents_removed.is_empty() {
            parts.push(format!("{} agent(s) removed", self.agents_removed.len()));
        }
        if !self.agents_changed.is_empty() {
            let field_count: usize = self.agents_changed.iter().map(|d| d.changes.len()).sum();
            parts.push(format!(
                "{} agent(s) changed ({} field(s))",
                self.agents_changed.len(),
                field_count,
            ));
        }
        if !self.tasks_added.is_empty() {
            parts.push(format!("{} task(s) added", self.tasks_added.len()));
        }
        if !self.tasks_removed.is_empty() {
            parts.push(format!("{} task(s) removed", self.tasks_removed.len()));
        }
        if !self.tasks_changed.is_empty() {
            let field_count: usize = self.tasks_changed.iter().map(|d| d.changes.len()).sum();
            parts.push(format!(
                "{} task(s) changed ({} field(s))",
                self.tasks_changed.len(),
                field_count,
            ));
        }
        if !self.sessions_added.is_empty() {
            parts.push(format!("{} session(s) added", self.sessions_added.len()));
        }
        if !self.sessions_removed.is_empty() {
            parts.push(format!("{} session(s) removed", self.sessions_removed.len()));
        }

        if parts.is_empty() {
            "no changes".to_string()
        } else {
            parts.join(", ")
        }
    }
}

// ---------------------------------------------------------------------------
// Agent diffing
// ---------------------------------------------------------------------------

fn diff_agents(
    old: &SystemSnapshot,
    new: &SystemSnapshot,
) -> (Vec<String>, Vec<String>, Vec<AgentDiff>) {
    let old_map: HashMap<&str, usize> = old
        .agents
        .iter()
        .enumerate()
        .map(|(i, a)| (a.name.as_str(), i))
        .collect();
    let new_map: HashMap<&str, usize> = new
        .agents
        .iter()
        .enumerate()
        .map(|(i, a)| (a.name.as_str(), i))
        .collect();

    let mut added = Vec::new();
    let mut removed = Vec::new();
    let mut changed = Vec::new();

    // Find removed and changed.
    for (name, &old_idx) in &old_map {
        match new_map.get(name) {
            None => removed.push(name.to_string()),
            Some(&new_idx) => {
                let changes = diff_agent_fields(&old.agents[old_idx], &new.agents[new_idx]);
                if !changes.is_empty() {
                    changed.push(AgentDiff {
                        name: name.to_string(),
                        changes,
                    });
                }
            }
        }
    }

    // Find added.
    for name in new_map.keys() {
        if !old_map.contains_key(name) {
            added.push(name.to_string());
        }
    }

    added.sort();
    removed.sort();
    changed.sort_by(|a, b| a.name.cmp(&b.name));

    (added, removed, changed)
}

fn diff_agent_fields(
    old: &super::state::AgentSnapshot,
    new: &super::state::AgentSnapshot,
) -> Vec<FieldChange> {
    let mut changes = Vec::new();

    if old.role != new.role {
        changes.push(FieldChange {
            field: "role".into(),
            old_value: old.role.clone(),
            new_value: new.role.clone(),
        });
    }
    if old.agent_type != new.agent_type {
        changes.push(FieldChange {
            field: "agent_type".into(),
            old_value: old.agent_type.clone(),
            new_value: new.agent_type.clone(),
        });
    }
    if old.status != new.status {
        changes.push(FieldChange {
            field: "status".into(),
            old_value: old.status.clone(),
            new_value: new.status.clone(),
        });
    }
    if old.task != new.task {
        changes.push(FieldChange {
            field: "task".into(),
            old_value: old.task.clone().unwrap_or_default(),
            new_value: new.task.clone().unwrap_or_default(),
        });
    }
    if old.path != new.path {
        changes.push(FieldChange {
            field: "path".into(),
            old_value: old.path.clone(),
            new_value: new.path.clone(),
        });
    }
    if old.health != new.health {
        changes.push(FieldChange {
            field: "health".into(),
            old_value: old.health.clone(),
            new_value: new.health.clone(),
        });
    }
    if old.last_heartbeat_ms != new.last_heartbeat_ms {
        changes.push(FieldChange {
            field: "last_heartbeat_ms".into(),
            old_value: old
                .last_heartbeat_ms
                .map(|v| v.to_string())
                .unwrap_or_default(),
            new_value: new
                .last_heartbeat_ms
                .map(|v| v.to_string())
                .unwrap_or_default(),
        });
    }

    changes
}

// ---------------------------------------------------------------------------
// Task diffing
// ---------------------------------------------------------------------------

fn diff_tasks(
    old: &SystemSnapshot,
    new: &SystemSnapshot,
) -> (Vec<String>, Vec<String>, Vec<TaskDiff>) {
    let old_map: HashMap<&str, usize> = old
        .tasks
        .iter()
        .enumerate()
        .map(|(i, t)| (t.id.as_str(), i))
        .collect();
    let new_map: HashMap<&str, usize> = new
        .tasks
        .iter()
        .enumerate()
        .map(|(i, t)| (t.id.as_str(), i))
        .collect();

    let mut added = Vec::new();
    let mut removed = Vec::new();
    let mut changed = Vec::new();

    for (id, &old_idx) in &old_map {
        match new_map.get(id) {
            None => removed.push(id.to_string()),
            Some(&new_idx) => {
                let changes = diff_task_fields(&old.tasks[old_idx], &new.tasks[new_idx]);
                if !changes.is_empty() {
                    changed.push(TaskDiff {
                        id: id.to_string(),
                        changes,
                    });
                }
            }
        }
    }

    for id in new_map.keys() {
        if !old_map.contains_key(id) {
            added.push(id.to_string());
        }
    }

    added.sort();
    removed.sort();
    changed.sort_by(|a, b| a.id.cmp(&b.id));

    (added, removed, changed)
}

fn diff_task_fields(
    old: &super::state::TaskSnapshot,
    new: &super::state::TaskSnapshot,
) -> Vec<FieldChange> {
    let mut changes = Vec::new();

    if old.title != new.title {
        changes.push(FieldChange {
            field: "title".into(),
            old_value: old.title.clone(),
            new_value: new.title.clone(),
        });
    }
    if old.status != new.status {
        changes.push(FieldChange {
            field: "status".into(),
            old_value: old.status.clone(),
            new_value: new.status.clone(),
        });
    }
    if old.source != new.source {
        changes.push(FieldChange {
            field: "source".into(),
            old_value: old.source.clone(),
            new_value: new.source.clone(),
        });
    }
    if old.agent != new.agent {
        changes.push(FieldChange {
            field: "agent".into(),
            old_value: old.agent.clone().unwrap_or_default(),
            new_value: new.agent.clone().unwrap_or_default(),
        });
    }
    if old.result != new.result {
        changes.push(FieldChange {
            field: "result".into(),
            old_value: old.result.clone().unwrap_or_default(),
            new_value: new.result.clone().unwrap_or_default(),
        });
    }
    if old.children_ids != new.children_ids {
        changes.push(FieldChange {
            field: "children_ids".into(),
            old_value: old.children_ids.join(","),
            new_value: new.children_ids.join(","),
        });
    }
    if old.spec_path != new.spec_path {
        changes.push(FieldChange {
            field: "spec_path".into(),
            old_value: old.spec_path.clone().unwrap_or_default(),
            new_value: new.spec_path.clone().unwrap_or_default(),
        });
    }

    changes
}

// ---------------------------------------------------------------------------
// Session diffing
// ---------------------------------------------------------------------------

fn diff_sessions(old: &SystemSnapshot, new: &SystemSnapshot) -> (Vec<String>, Vec<String>) {
    let old_names: HashMap<&str, ()> = old
        .sessions
        .iter()
        .map(|s| (s.name.as_str(), ()))
        .collect();
    let new_names: HashMap<&str, ()> = new
        .sessions
        .iter()
        .map(|s| (s.name.as_str(), ()))
        .collect();

    let mut added: Vec<String> = new_names
        .keys()
        .filter(|n| !old_names.contains_key(*n))
        .map(|n| n.to_string())
        .collect();
    let mut removed: Vec<String> = old_names
        .keys()
        .filter(|n| !new_names.contains_key(*n))
        .map(|n| n.to_string())
        .collect();

    added.sort();
    removed.sort();

    (added, removed)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot::state::{AgentSnapshot, SessionSnapshot, TaskSnapshot};

    fn make_agent(name: &str, role: &str, status: &str, task: Option<&str>) -> AgentSnapshot {
        AgentSnapshot {
            name: name.into(),
            role: role.into(),
            agent_type: "claude".into(),
            status: status.into(),
            task: task.map(|t| t.into()),
            path: "/tmp".into(),
            health: "healthy".into(),
            last_heartbeat_ms: Some(1000),
        }
    }

    fn make_task(id: &str, title: &str, status: &str, agent: Option<&str>) -> TaskSnapshot {
        TaskSnapshot {
            id: id.into(),
            title: title.into(),
            status: status.into(),
            source: "roadmap".into(),
            agent: agent.map(|a| a.into()),
            result: None,
            children_ids: Vec::new(),
            spec_path: None,
        }
    }

    fn make_session(name: &str) -> SessionSnapshot {
        SessionSnapshot {
            name: name.into(),
            window_count: 1,
            pane_count: 1,
            agents_placed: Vec::new(),
        }
    }

    fn empty_snap() -> SystemSnapshot {
        SystemSnapshot::new("0.1.0", 1000)
    }

    // --- Identical snapshots ---

    #[test]
    fn identical_snapshots_produce_empty_diff() {
        let snap = empty_snap()
            .with_agents(vec![make_agent("w1", "worker", "idle", None)])
            .with_tasks(vec![make_task("T1", "Task", "pending", None)]);
        let diff = SnapshotDiff::compute(&snap, &snap);
        assert!(diff.is_empty());
        assert_eq!(diff.change_count(), 0);
    }

    #[test]
    fn empty_snapshots_produce_empty_diff() {
        let diff = SnapshotDiff::compute(&empty_snap(), &empty_snap());
        assert!(diff.is_empty());
    }

    // --- Agent additions ---

    #[test]
    fn agent_added() {
        let old = empty_snap();
        let new = empty_snap().with_agents(vec![make_agent("w1", "worker", "idle", None)]);
        let diff = SnapshotDiff::compute(&old, &new);
        assert_eq!(diff.agents_added, vec!["w1"]);
        assert!(diff.agents_removed.is_empty());
    }

    #[test]
    fn multiple_agents_added() {
        let old = empty_snap();
        let new = empty_snap().with_agents(vec![
            make_agent("a", "worker", "idle", None),
            make_agent("b", "worker", "idle", None),
            make_agent("c", "worker", "idle", None),
        ]);
        let diff = SnapshotDiff::compute(&old, &new);
        assert_eq!(diff.agents_added.len(), 3);
    }

    // --- Agent removals ---

    #[test]
    fn agent_removed() {
        let old = empty_snap().with_agents(vec![make_agent("w1", "worker", "idle", None)]);
        let new = empty_snap();
        let diff = SnapshotDiff::compute(&old, &new);
        assert!(diff.agents_added.is_empty());
        assert_eq!(diff.agents_removed, vec!["w1"]);
    }

    // --- Agent field changes ---

    #[test]
    fn agent_status_changed() {
        let old = empty_snap().with_agents(vec![make_agent("w1", "worker", "idle", None)]);
        let new = empty_snap().with_agents(vec![make_agent("w1", "worker", "busy", None)]);
        let diff = SnapshotDiff::compute(&old, &new);
        assert_eq!(diff.agents_changed.len(), 1);
        assert_eq!(diff.agents_changed[0].name, "w1");
        assert!(diff.agents_changed[0]
            .changes
            .iter()
            .any(|c| c.field == "status" && c.old_value == "idle" && c.new_value == "busy"));
    }

    #[test]
    fn agent_role_changed() {
        let old = empty_snap().with_agents(vec![make_agent("w1", "worker", "idle", None)]);
        let new = empty_snap().with_agents(vec![make_agent("w1", "pilot", "idle", None)]);
        let diff = SnapshotDiff::compute(&old, &new);
        assert_eq!(diff.agents_changed.len(), 1);
        assert!(diff.agents_changed[0]
            .changes
            .iter()
            .any(|c| c.field == "role"));
    }

    #[test]
    fn agent_task_assigned() {
        let old = empty_snap().with_agents(vec![make_agent("w1", "worker", "idle", None)]);
        let new = empty_snap().with_agents(vec![make_agent("w1", "worker", "busy", Some("T1"))]);
        let diff = SnapshotDiff::compute(&old, &new);
        assert_eq!(diff.agents_changed.len(), 1);
        let changes = &diff.agents_changed[0].changes;
        assert!(changes.iter().any(|c| c.field == "task"));
        assert!(changes.iter().any(|c| c.field == "status"));
    }

    #[test]
    fn agent_multiple_fields_changed() {
        let mut old_agent = make_agent("w1", "worker", "idle", None);
        old_agent.health = "healthy".into();
        old_agent.path = "/old".into();

        let mut new_agent = make_agent("w1", "worker", "busy", Some("T1"));
        new_agent.health = "degraded".into();
        new_agent.path = "/new".into();

        let old = empty_snap().with_agents(vec![old_agent]);
        let new = empty_snap().with_agents(vec![new_agent]);
        let diff = SnapshotDiff::compute(&old, &new);

        assert_eq!(diff.agents_changed.len(), 1);
        assert!(diff.agents_changed[0].changes.len() >= 3); // status, task, health, path
    }

    #[test]
    fn agent_heartbeat_changed() {
        let mut old_agent = make_agent("w1", "worker", "idle", None);
        old_agent.last_heartbeat_ms = Some(1000);
        let mut new_agent = make_agent("w1", "worker", "idle", None);
        new_agent.last_heartbeat_ms = Some(2000);

        let old = empty_snap().with_agents(vec![old_agent]);
        let new = empty_snap().with_agents(vec![new_agent]);
        let diff = SnapshotDiff::compute(&old, &new);

        assert_eq!(diff.agents_changed.len(), 1);
        assert!(diff.agents_changed[0]
            .changes
            .iter()
            .any(|c| c.field == "last_heartbeat_ms"));
    }

    // --- Task additions ---

    #[test]
    fn task_added() {
        let old = empty_snap();
        let new = empty_snap().with_tasks(vec![make_task("T1", "Task", "pending", None)]);
        let diff = SnapshotDiff::compute(&old, &new);
        assert_eq!(diff.tasks_added, vec!["T1"]);
    }

    // --- Task removals ---

    #[test]
    fn task_removed() {
        let old = empty_snap().with_tasks(vec![make_task("T1", "Task", "pending", None)]);
        let new = empty_snap();
        let diff = SnapshotDiff::compute(&old, &new);
        assert_eq!(diff.tasks_removed, vec!["T1"]);
    }

    // --- Task field changes ---

    #[test]
    fn task_status_changed() {
        let old = empty_snap().with_tasks(vec![make_task("T1", "Task", "pending", None)]);
        let new = empty_snap().with_tasks(vec![make_task("T1", "Task", "in_progress", None)]);
        let diff = SnapshotDiff::compute(&old, &new);
        assert_eq!(diff.tasks_changed.len(), 1);
        assert!(diff.tasks_changed[0]
            .changes
            .iter()
            .any(|c| c.field == "status"));
    }

    #[test]
    fn task_title_changed() {
        let old = empty_snap().with_tasks(vec![make_task("T1", "Old title", "pending", None)]);
        let new = empty_snap().with_tasks(vec![make_task("T1", "New title", "pending", None)]);
        let diff = SnapshotDiff::compute(&old, &new);
        assert_eq!(diff.tasks_changed.len(), 1);
        assert!(diff.tasks_changed[0]
            .changes
            .iter()
            .any(|c| c.field == "title" && c.old_value == "Old title"));
    }

    #[test]
    fn task_agent_assigned() {
        let old = empty_snap().with_tasks(vec![make_task("T1", "Task", "pending", None)]);
        let new = empty_snap().with_tasks(vec![make_task("T1", "Task", "pending", Some("w1"))]);
        let diff = SnapshotDiff::compute(&old, &new);
        assert!(diff.tasks_changed[0]
            .changes
            .iter()
            .any(|c| c.field == "agent"));
    }

    #[test]
    fn task_children_changed() {
        let mut old_task = make_task("T1", "Parent", "pending", None);
        old_task.children_ids = vec!["T2".into()];
        let mut new_task = make_task("T1", "Parent", "pending", None);
        new_task.children_ids = vec!["T2".into(), "T3".into()];

        let old = empty_snap().with_tasks(vec![old_task]);
        let new = empty_snap().with_tasks(vec![new_task]);
        let diff = SnapshotDiff::compute(&old, &new);

        assert!(diff.tasks_changed[0]
            .changes
            .iter()
            .any(|c| c.field == "children_ids"));
    }

    #[test]
    fn task_result_changed() {
        let mut old_task = make_task("T1", "Task", "completed", None);
        old_task.result = None;
        let mut new_task = make_task("T1", "Task", "completed", None);
        new_task.result = Some("success".into());

        let old = empty_snap().with_tasks(vec![old_task]);
        let new = empty_snap().with_tasks(vec![new_task]);
        let diff = SnapshotDiff::compute(&old, &new);

        assert!(diff.tasks_changed[0]
            .changes
            .iter()
            .any(|c| c.field == "result"));
    }

    #[test]
    fn task_spec_path_changed() {
        let mut old_task = make_task("T1", "Task", "pending", None);
        old_task.spec_path = Some("/old/path.md".into());
        let mut new_task = make_task("T1", "Task", "pending", None);
        new_task.spec_path = Some("/new/path.md".into());

        let old = empty_snap().with_tasks(vec![old_task]);
        let new = empty_snap().with_tasks(vec![new_task]);
        let diff = SnapshotDiff::compute(&old, &new);

        assert!(diff.tasks_changed[0]
            .changes
            .iter()
            .any(|c| c.field == "spec_path"));
    }

    // --- Session additions/removals ---

    #[test]
    fn session_added() {
        let old = empty_snap();
        let new = empty_snap().with_sessions(vec![make_session("main")]);
        let diff = SnapshotDiff::compute(&old, &new);
        assert_eq!(diff.sessions_added, vec!["main"]);
    }

    #[test]
    fn session_removed() {
        let old = empty_snap().with_sessions(vec![make_session("main")]);
        let new = empty_snap();
        let diff = SnapshotDiff::compute(&old, &new);
        assert_eq!(diff.sessions_removed, vec!["main"]);
    }

    #[test]
    fn session_added_and_removed() {
        let old = empty_snap().with_sessions(vec![make_session("old-session")]);
        let new = empty_snap().with_sessions(vec![make_session("new-session")]);
        let diff = SnapshotDiff::compute(&old, &new);
        assert_eq!(diff.sessions_added, vec!["new-session"]);
        assert_eq!(diff.sessions_removed, vec!["old-session"]);
    }

    // --- change_count ---

    #[test]
    fn change_count_sums_all() {
        let old = empty_snap()
            .with_agents(vec![
                make_agent("w1", "worker", "idle", None),
                make_agent("w2", "worker", "idle", None),
            ])
            .with_tasks(vec![make_task("T1", "Task", "pending", None)])
            .with_sessions(vec![make_session("main")]);
        let new = empty_snap()
            .with_agents(vec![
                make_agent("w1", "worker", "busy", None), // changed
                make_agent("w3", "worker", "idle", None),  // added, w2 removed
            ])
            .with_tasks(vec![
                make_task("T1", "Task", "in_progress", None), // changed
                make_task("T2", "Task 2", "pending", None),   // added
            ]);
        let diff = SnapshotDiff::compute(&old, &new);

        // w3 added (1) + w2 removed (1) + w1 status changed (1 field) +
        // T2 added (1) + T1 status changed (1 field) + main removed (1) = 6
        assert_eq!(diff.change_count(), 6);
    }

    #[test]
    fn change_count_zero_for_identical() {
        let snap = empty_snap();
        let diff = SnapshotDiff::compute(&snap, &snap);
        assert_eq!(diff.change_count(), 0);
    }

    // --- summary ---

    #[test]
    fn summary_no_changes() {
        let diff = SnapshotDiff::compute(&empty_snap(), &empty_snap());
        assert_eq!(diff.summary(), "no changes");
    }

    #[test]
    fn summary_agent_added() {
        let old = empty_snap();
        let new = empty_snap().with_agents(vec![make_agent("w1", "worker", "idle", None)]);
        let diff = SnapshotDiff::compute(&old, &new);
        assert!(diff.summary().contains("1 agent(s) added"));
    }

    #[test]
    fn summary_multiple_changes() {
        let old = empty_snap()
            .with_agents(vec![make_agent("w1", "worker", "idle", None)])
            .with_sessions(vec![make_session("main")]);
        let new = empty_snap()
            .with_agents(vec![make_agent("w1", "worker", "busy", None)])
            .with_tasks(vec![make_task("T1", "Task", "pending", None)]);
        let diff = SnapshotDiff::compute(&old, &new);
        let summary = diff.summary();
        assert!(summary.contains("agent(s) changed"));
        assert!(summary.contains("task(s) added"));
        assert!(summary.contains("session(s) removed"));
    }

    // --- Serde round-trip ---

    #[test]
    fn diff_serde_round_trip() {
        let old = empty_snap().with_agents(vec![make_agent("w1", "worker", "idle", None)]);
        let new = empty_snap().with_agents(vec![make_agent("w1", "worker", "busy", None)]);
        let diff = SnapshotDiff::compute(&old, &new);
        let json = serde_json::to_string(&diff).unwrap();
        let back: SnapshotDiff = serde_json::from_str(&json).unwrap();
        assert_eq!(back, diff);
    }

    #[test]
    fn field_change_serde() {
        let fc = FieldChange {
            field: "status".into(),
            old_value: "idle".into(),
            new_value: "busy".into(),
        };
        let json = serde_json::to_string(&fc).unwrap();
        let back: FieldChange = serde_json::from_str(&json).unwrap();
        assert_eq!(back, fc);
    }

    #[test]
    fn agent_diff_serde() {
        let ad = AgentDiff {
            name: "w1".into(),
            changes: vec![FieldChange {
                field: "status".into(),
                old_value: "idle".into(),
                new_value: "busy".into(),
            }],
        };
        let json = serde_json::to_string(&ad).unwrap();
        let back: AgentDiff = serde_json::from_str(&json).unwrap();
        assert_eq!(back, ad);
    }

    #[test]
    fn task_diff_serde() {
        let td = TaskDiff {
            id: "T1".into(),
            changes: vec![FieldChange {
                field: "title".into(),
                old_value: "Old".into(),
                new_value: "New".into(),
            }],
        };
        let json = serde_json::to_string(&td).unwrap();
        let back: TaskDiff = serde_json::from_str(&json).unwrap();
        assert_eq!(back, td);
    }

    // --- Combined changes ---

    #[test]
    fn mixed_additions_removals_changes() {
        let old = empty_snap()
            .with_agents(vec![
                make_agent("keep", "worker", "idle", None),
                make_agent("remove-me", "worker", "idle", None),
            ])
            .with_tasks(vec![make_task("T1", "Existing", "pending", None)])
            .with_sessions(vec![make_session("old-sess")]);

        let new = empty_snap()
            .with_agents(vec![
                make_agent("keep", "worker", "busy", None),
                make_agent("new-agent", "pilot", "idle", None),
            ])
            .with_tasks(vec![
                make_task("T1", "Existing", "completed", None),
                make_task("T2", "New task", "pending", None),
            ])
            .with_sessions(vec![make_session("new-sess")]);

        let diff = SnapshotDiff::compute(&old, &new);

        assert_eq!(diff.agents_added, vec!["new-agent"]);
        assert_eq!(diff.agents_removed, vec!["remove-me"]);
        assert_eq!(diff.agents_changed.len(), 1);
        assert_eq!(diff.agents_changed[0].name, "keep");

        assert_eq!(diff.tasks_added, vec!["T2"]);
        assert!(diff.tasks_removed.is_empty());
        assert_eq!(diff.tasks_changed.len(), 1);
        assert_eq!(diff.tasks_changed[0].id, "T1");

        assert_eq!(diff.sessions_added, vec!["new-sess"]);
        assert_eq!(diff.sessions_removed, vec!["old-sess"]);

        assert!(!diff.is_empty());
        assert!(diff.change_count() > 0);
    }

    // --- is_empty ---

    #[test]
    fn is_empty_true_when_no_changes() {
        let diff = SnapshotDiff {
            agents_added: vec![],
            agents_removed: vec![],
            agents_changed: vec![],
            tasks_added: vec![],
            tasks_removed: vec![],
            tasks_changed: vec![],
            sessions_added: vec![],
            sessions_removed: vec![],
        };
        assert!(diff.is_empty());
    }

    #[test]
    fn is_empty_false_when_agent_added() {
        let diff = SnapshotDiff {
            agents_added: vec!["w1".into()],
            agents_removed: vec![],
            agents_changed: vec![],
            tasks_added: vec![],
            tasks_removed: vec![],
            tasks_changed: vec![],
            sessions_added: vec![],
            sessions_removed: vec![],
        };
        assert!(!diff.is_empty());
    }
}
