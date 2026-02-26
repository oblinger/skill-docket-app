//! Action expander — converts logical agent actions into infrastructure actions.
//!
//! `CreateAgent` and `KillAgent` are logical; the backend only understands
//! `CreateSession`, `SendKeys`, and `KillSession`.  This module bridges the gap.

use cmx_utils::response::Action;

/// Derive the tmux session name for an agent.
pub fn session_name(agent_name: &str) -> String {
    format!("cmx-{}", agent_name)
}

/// Expand logical actions into infrastructure actions.
///
/// Returns `(expanded_actions, session_mappings)` where each mapping is
/// `(agent_name, session_name)` for feedback into Sys.
pub fn expand_actions(
    actions: Vec<Action>,
    launch_command: &str,
) -> (Vec<Action>, Vec<(String, String)>) {
    let mut expanded = Vec::new();
    let mut mappings = Vec::new();

    for action in actions {
        match action {
            Action::CreateAgent { ref name, ref path, .. } => {
                let sess = session_name(name);
                expanded.push(Action::CreateSession {
                    name: sess.clone(),
                    cwd: path.clone(),
                });
                expanded.push(Action::SendKeys {
                    target: sess.clone(),
                    keys: launch_command.to_string(),
                });
                mappings.push((name.clone(), sess));
            }
            Action::KillAgent { ref name } => {
                let sess = session_name(name);
                expanded.push(Action::KillSession { name: sess });
            }
            other => {
                expanded.push(other);
            }
        }
    }

    (expanded, mappings)
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_name_convention() {
        assert_eq!(session_name("w1"), "cmx-w1");
        assert_eq!(session_name("pm-main"), "cmx-pm-main");
    }

    #[test]
    fn expand_create_agent() {
        let actions = vec![Action::CreateAgent {
            name: "w1".into(),
            role: "worker".into(),
            path: "/projects/foo".into(),
        }];
        let (expanded, mappings) = expand_actions(actions, "claude");

        assert_eq!(expanded.len(), 2);
        assert_eq!(
            expanded[0],
            Action::CreateSession {
                name: "cmx-w1".into(),
                cwd: "/projects/foo".into(),
            }
        );
        assert_eq!(
            expanded[1],
            Action::SendKeys {
                target: "cmx-w1".into(),
                keys: "claude".into(),
            }
        );
        assert_eq!(mappings, vec![("w1".into(), "cmx-w1".into())]);
    }

    #[test]
    fn expand_kill_agent() {
        let actions = vec![Action::KillAgent { name: "w1".into() }];
        let (expanded, mappings) = expand_actions(actions, "claude");

        assert_eq!(expanded.len(), 1);
        assert_eq!(
            expanded[0],
            Action::KillSession { name: "cmx-w1".into() }
        );
        assert!(mappings.is_empty());
    }

    #[test]
    fn expand_passthrough() {
        let actions = vec![
            Action::CreateSession {
                name: "manual".into(),
                cwd: "/tmp".into(),
            },
            Action::SendKeys {
                target: "manual".into(),
                keys: "echo hi".into(),
            },
        ];
        let (expanded, mappings) = expand_actions(actions.clone(), "claude");

        assert_eq!(expanded.len(), 2);
        assert_eq!(expanded, actions);
        assert!(mappings.is_empty());
    }

    #[test]
    fn expand_empty() {
        let (expanded, mappings) = expand_actions(vec![], "claude");
        assert!(expanded.is_empty());
        assert!(mappings.is_empty());
    }
}
