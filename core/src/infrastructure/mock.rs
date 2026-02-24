//! Mock session backend for testing.
//!
//! Records all actions and provides controllable responses, making it easy
//! to write deterministic tests for higher-level orchestration code.

use std::collections::HashMap;

use cmx_utils::response::Action;

use super::SessionBackend;

/// A test-double that records actions and serves pre-configured pane captures.
pub struct MockBackend {
    /// All actions executed against this backend, in order.
    pub actions: Vec<Action>,
    /// Known session names.
    pub sessions: Vec<String>,
    /// Pre-configured pane capture responses, keyed by target string.
    pub pane_captures: HashMap<String, String>,
}

impl MockBackend {
    pub fn new() -> Self {
        MockBackend {
            actions: Vec::new(),
            sessions: Vec::new(),
            pane_captures: HashMap::new(),
        }
    }

    /// Create a mock with some sessions already present.
    pub fn with_sessions(sessions: Vec<String>) -> Self {
        MockBackend {
            actions: Vec::new(),
            sessions,
            pane_captures: HashMap::new(),
        }
    }

    /// Pre-load a pane capture result.
    pub fn set_capture(&mut self, target: &str, content: &str) {
        self.pane_captures
            .insert(target.to_string(), content.to_string());
    }

    /// Clear all recorded actions.
    pub fn clear_actions(&mut self) {
        self.actions.clear();
    }
}

impl Default for MockBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionBackend for MockBackend {
    fn execute_action(&mut self, action: &Action) -> Result<(), String> {
        // Track session creation and destruction logically.
        match action {
            Action::CreateSession { name, .. } => {
                if !self.sessions.contains(name) {
                    self.sessions.push(name.clone());
                }
            }
            Action::KillSession { name } => {
                self.sessions.retain(|s| s != name);
            }
            _ => {}
        }
        self.actions.push(action.clone());
        Ok(())
    }

    fn session_exists(&self, name: &str) -> bool {
        self.sessions.iter().any(|s| s == name)
    }

    fn list_sessions(&self) -> Vec<String> {
        self.sessions.clone()
    }

    fn capture_pane(&self, target: &str) -> Result<String, String> {
        self.pane_captures
            .get(target)
            .cloned()
            .ok_or_else(|| format!("mock: no capture for '{}'", target))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cmx_utils::response::Direction;

    #[test]
    fn records_actions() {
        let mut mock = MockBackend::new();
        let action = Action::CreateSession {
            name: "test".into(),
            cwd: "/tmp".into(),
        };
        mock.execute_action(&action).unwrap();
        assert_eq!(mock.actions.len(), 1);
    }

    #[test]
    fn tracks_sessions() {
        let mut mock = MockBackend::new();
        assert!(!mock.session_exists("s1"));

        mock.execute_action(&Action::CreateSession {
            name: "s1".into(),
            cwd: "/tmp".into(),
        })
        .unwrap();
        assert!(mock.session_exists("s1"));
        assert_eq!(mock.list_sessions(), vec!["s1"]);

        mock.execute_action(&Action::KillSession {
            name: "s1".into(),
        })
        .unwrap();
        assert!(!mock.session_exists("s1"));
    }

    #[test]
    fn capture_pane_returns_preset() {
        let mut mock = MockBackend::new();
        mock.set_capture("s1:0.0", "$ ready");
        assert_eq!(mock.capture_pane("s1:0.0").unwrap(), "$ ready");
    }

    #[test]
    fn capture_pane_missing_returns_error() {
        let mock = MockBackend::new();
        assert!(mock.capture_pane("missing").is_err());
    }

    #[test]
    fn with_sessions_constructor() {
        let mock = MockBackend::with_sessions(vec!["a".into(), "b".into()]);
        assert!(mock.session_exists("a"));
        assert!(mock.session_exists("b"));
        assert!(!mock.session_exists("c"));
    }

    #[test]
    fn clear_actions() {
        let mut mock = MockBackend::new();
        mock.execute_action(&Action::SplitPane {
            session: "s1".into(),
            direction: Direction::Horizontal,
            percent: 50,
        })
        .unwrap();
        assert_eq!(mock.actions.len(), 1);
        mock.clear_actions();
        assert!(mock.actions.is_empty());
    }
}
