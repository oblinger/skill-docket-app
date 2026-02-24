//! Action planner — diffs desired vs actual state and emits a minimal action set.
//!
//! The planner is stateless: it takes snapshots of the current and desired
//! worlds and returns the actions needed to converge them. It never executes
//! anything itself.

use crate::types::agent::Agent;
use cmx_utils::response::Action;

/// A lightweight description of a desired agent, used as input to the planner.
#[derive(Debug, Clone, PartialEq)]
pub struct AgentEntry {
    pub name: String,
    pub role: String,
    pub task: Option<String>,
    pub path: String,
}

/// Compute the minimum set of `Action`s to move from the current state to the
/// desired state.
///
/// # Arguments
///
/// * `current_agents` — agents that currently exist.
/// * `desired_agents` — the agents we want to exist after convergence.
/// * `current_sessions` — session names that currently exist.
/// * `desired_sessions` — `(name, cwd)` pairs for sessions that should exist.
///
/// # Returns
///
/// A vec of `Action`s in recommended execution order:
/// 1. Create missing sessions
/// 2. Kill surplus sessions
/// 3. Create missing agents
/// 4. Kill surplus agents
/// 5. Update assignments for existing agents whose task changed
pub fn plan(
    current_agents: &[Agent],
    desired_agents: &[AgentEntry],
    current_sessions: &[String],
    desired_sessions: &[(String, String)],
) -> Vec<Action> {
    let mut actions = Vec::new();

    // --- Sessions ---

    // Sessions to create: desired but not in current.
    for (name, cwd) in desired_sessions {
        if !current_sessions.iter().any(|s| s == name) {
            actions.push(Action::CreateSession {
                name: name.clone(),
                cwd: cwd.clone(),
            });
        }
    }

    // Sessions to kill: current but not in desired.
    let desired_session_names: Vec<&String> = desired_sessions.iter().map(|(n, _)| n).collect();
    for name in current_sessions {
        if !desired_session_names.contains(&name) {
            actions.push(Action::KillSession { name: name.clone() });
        }
    }

    // --- Agents ---

    let current_names: Vec<&str> = current_agents.iter().map(|a| a.name.as_str()).collect();
    let desired_names: Vec<&str> = desired_agents.iter().map(|a| a.name.as_str()).collect();

    // Agents to create: desired but not in current.
    for entry in desired_agents {
        if !current_names.contains(&entry.name.as_str()) {
            if entry.role == "remote" {
                // Remote agents need SSH connectivity instead of local creation.
                // The path field doubles as the host address for remote agents.
                actions.push(Action::ConnectSsh {
                    agent: entry.name.clone(),
                    host: entry.path.clone(),
                    port: 22,
                });
            } else {
                actions.push(Action::CreateAgent {
                    name: entry.name.clone(),
                    role: entry.role.clone(),
                    path: entry.path.clone(),
                });
            }
        }
    }

    // Agents to kill: current but not in desired.
    for agent in current_agents {
        if !desired_names.contains(&agent.name.as_str()) {
            actions.push(Action::KillAgent {
                name: agent.name.clone(),
            });
        }
    }

    // --- Assignment updates ---
    // For agents that exist in both current and desired, check if the task changed.
    for entry in desired_agents {
        if let Some(current) = current_agents.iter().find(|a| a.name == entry.name) {
            if current.task != entry.task {
                actions.push(Action::UpdateAssignment {
                    agent: entry.name.clone(),
                    task: entry.task.clone(),
                });
            }
        }
    }

    actions
}

/// Like `plan()`, but first filters out sessions that already exist in the
/// backend, preventing duplicate `CreateSession` actions (session adoption).
pub fn plan_with_adoption(
    current_agents: &[Agent],
    desired_agents: &[AgentEntry],
    current_sessions: &[String],
    desired_sessions: &[(String, String)],
    existing_sessions: &[String],
) -> Vec<Action> {
    // Merge existing sessions into current so the planner knows they exist
    // (avoids creating them again).
    let mut all_current = current_sessions.to_vec();
    for name in existing_sessions {
        if !all_current.contains(name) {
            all_current.push(name.clone());
        }
    }

    // Keep desired_sessions as-is so the planner won't kill adopted sessions.
    // The planner will skip creating them because they're now in all_current.
    plan(current_agents, desired_agents, &all_current, desired_sessions)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::agent::{AgentStatus, AgentType, HealthState};

    fn make_agent(name: &str, task: Option<&str>) -> Agent {
        Agent {
            name: name.into(),
            role: "worker".into(),
            agent_type: AgentType::Claude,
            task: task.map(|t| t.into()),
            path: "/tmp".into(),
            status: AgentStatus::Idle,
            status_notes: String::new(),
            health: HealthState::Healthy,
            last_heartbeat_ms: None,
            session: None,
        }
    }

    fn make_entry(name: &str, task: Option<&str>) -> AgentEntry {
        AgentEntry {
            name: name.into(),
            role: "worker".into(),
            task: task.map(|t| t.into()),
            path: "/tmp".into(),
        }
    }

    #[test]
    fn no_changes_produces_empty() {
        let agents = vec![make_agent("w1", None)];
        let entries = vec![make_entry("w1", None)];
        let sessions = vec!["s1".to_string()];
        let desired_sessions = vec![("s1".to_string(), "/tmp".to_string())];
        let actions = plan(&agents, &entries, &sessions, &desired_sessions);
        assert!(actions.is_empty());
    }

    #[test]
    fn create_missing_session() {
        let actions = plan(
            &[],
            &[],
            &[],
            &[("work".into(), "/home".into())],
        );
        assert_eq!(actions.len(), 1);
        matches!(&actions[0], Action::CreateSession { name, .. } if name == "work");
    }

    #[test]
    fn kill_surplus_session() {
        let actions = plan(&[], &[], &["old".into()], &[]);
        assert_eq!(actions.len(), 1);
        matches!(&actions[0], Action::KillSession { name } if name == "old");
    }

    #[test]
    fn create_missing_agent() {
        let actions = plan(
            &[],
            &[make_entry("w1", None)],
            &[],
            &[],
        );
        assert_eq!(actions.len(), 1);
        match &actions[0] {
            Action::CreateAgent { name, .. } => assert_eq!(name, "w1"),
            other => panic!("unexpected action: {:?}", other),
        }
    }

    #[test]
    fn kill_surplus_agent() {
        let actions = plan(&[make_agent("w1", None)], &[], &[], &[]);
        assert_eq!(actions.len(), 1);
        match &actions[0] {
            Action::KillAgent { name } => assert_eq!(name, "w1"),
            other => panic!("unexpected action: {:?}", other),
        }
    }

    #[test]
    fn update_task_assignment() {
        let current = vec![make_agent("w1", Some("CMX1"))];
        let desired = vec![make_entry("w1", Some("CMX2"))];
        let actions = plan(&current, &desired, &[], &[]);
        assert_eq!(actions.len(), 1);
        match &actions[0] {
            Action::UpdateAssignment { agent, task } => {
                assert_eq!(agent, "w1");
                assert_eq!(task.as_deref(), Some("CMX2"));
            }
            other => panic!("unexpected action: {:?}", other),
        }
    }

    #[test]
    fn no_update_when_task_unchanged() {
        let current = vec![make_agent("w1", Some("CMX1"))];
        let desired = vec![make_entry("w1", Some("CMX1"))];
        let actions = plan(&current, &desired, &[], &[]);
        assert!(actions.is_empty());
    }

    #[test]
    fn mixed_agent_changes() {
        let current = vec![make_agent("keep", None), make_agent("remove", None)];
        let desired = vec![make_entry("keep", None), make_entry("add", None)];
        let actions = plan(&current, &desired, &[], &[]);
        assert_eq!(actions.len(), 2);
        let names: Vec<String> = actions
            .iter()
            .map(|a| match a {
                Action::CreateAgent { name, .. } => format!("create:{}", name),
                Action::KillAgent { name } => format!("kill:{}", name),
                _ => "other".into(),
            })
            .collect();
        assert!(names.contains(&"create:add".to_string()));
        assert!(names.contains(&"kill:remove".to_string()));
    }

    #[test]
    fn mixed_session_changes() {
        let current = vec!["keep".into(), "old".into()];
        let desired = vec![
            ("keep".into(), "/tmp".into()),
            ("new".into(), "/home".into()),
        ];
        let actions = plan(&[], &[], &current, &desired);
        assert_eq!(actions.len(), 2);
    }

    #[test]
    fn full_convergence_scenario() {
        let current_agents = vec![make_agent("w1", Some("CMX1"))];
        let desired_agents = vec![
            make_entry("w1", Some("CMX2")),
            make_entry("w2", None),
        ];
        let current_sessions = vec!["s1".into()];
        let desired_sessions = vec![
            ("s1".into(), "/tmp".into()),
            ("s2".into(), "/home".into()),
        ];
        let actions = plan(
            &current_agents,
            &desired_agents,
            &current_sessions,
            &desired_sessions,
        );
        // Expect: create session s2, create agent w2, update w1 assignment
        assert_eq!(actions.len(), 3);
    }

    #[test]
    fn assign_none_from_some() {
        let current = vec![make_agent("w1", Some("CMX1"))];
        let desired = vec![make_entry("w1", None)];
        let actions = plan(&current, &desired, &[], &[]);
        assert_eq!(actions.len(), 1);
        match &actions[0] {
            Action::UpdateAssignment { agent, task } => {
                assert_eq!(agent, "w1");
                assert!(task.is_none());
            }
            other => panic!("unexpected action: {:?}", other),
        }
    }

    #[test]
    fn assign_some_from_none() {
        let current = vec![make_agent("w1", None)];
        let desired = vec![make_entry("w1", Some("CMX1"))];
        let actions = plan(&current, &desired, &[], &[]);
        assert_eq!(actions.len(), 1);
        match &actions[0] {
            Action::UpdateAssignment { agent, task } => {
                assert_eq!(agent, "w1");
                assert_eq!(task.as_deref(), Some("CMX1"));
            }
            other => panic!("unexpected action: {:?}", other),
        }
    }

    #[test]
    fn adoption_skips_existing_session() {
        let desired = vec![("work".into(), "/tmp".into())];
        let existing = vec!["work".into()];
        let actions = plan_with_adoption(&[], &[], &[], &desired, &existing);
        assert!(actions.is_empty());
    }

    #[test]
    fn adoption_creates_only_missing_sessions() {
        let desired = vec![
            ("work".into(), "/tmp".into()),
            ("dev".into(), "/home".into()),
        ];
        let existing = vec!["work".into()];
        let actions = plan_with_adoption(&[], &[], &[], &desired, &existing);
        assert_eq!(actions.len(), 1);
        match &actions[0] {
            Action::CreateSession { name, .. } => assert_eq!(name, "dev"),
            other => panic!("expected CreateSession, got {:?}", other),
        }
    }

    // --- Remote agent (ConnectSsh) tests ---

    fn make_remote_entry(name: &str, host: &str) -> AgentEntry {
        AgentEntry {
            name: name.into(),
            role: "remote".into(),
            task: None,
            path: host.into(),
        }
    }

    #[test]
    fn remote_agent_produces_connect_ssh() {
        let desired = vec![make_remote_entry("gpu1", "10.0.0.1")];
        let actions = plan(&[], &desired, &[], &[]);
        assert_eq!(actions.len(), 1);
        match &actions[0] {
            Action::ConnectSsh { agent, host, port } => {
                assert_eq!(agent, "gpu1");
                assert_eq!(host, "10.0.0.1");
                assert_eq!(*port, 22);
            }
            other => panic!("expected ConnectSsh, got {:?}", other),
        }
    }

    #[test]
    fn mixed_local_and_remote_agents() {
        let desired = vec![
            make_entry("w1", None),
            make_remote_entry("gpu1", "10.0.0.1"),
        ];
        let actions = plan(&[], &desired, &[], &[]);
        assert_eq!(actions.len(), 2);
        // First should be CreateAgent for local worker
        match &actions[0] {
            Action::CreateAgent { name, .. } => assert_eq!(name, "w1"),
            other => panic!("expected CreateAgent, got {:?}", other),
        }
        // Second should be ConnectSsh for remote agent
        match &actions[1] {
            Action::ConnectSsh { agent, .. } => assert_eq!(agent, "gpu1"),
            other => panic!("expected ConnectSsh, got {:?}", other),
        }
    }

    #[test]
    fn existing_remote_agent_not_recreated() {
        let current = vec![make_agent("gpu1", None)];
        // Even though desired has remote role, the agent already exists — no action needed
        let desired = vec![AgentEntry {
            name: "gpu1".into(),
            role: "remote".into(),
            task: None,
            path: "10.0.0.1".into(),
        }];
        let actions = plan(&current, &desired, &[], &[]);
        assert!(actions.is_empty());
    }
}
