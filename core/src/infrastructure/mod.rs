//! Infrastructure backends for session management.
//!
//! Provides the `SessionBackend` trait and implementations for tmux (production)
//! and mock (testing). The tmux backend builds command strings without executing
//! them, keeping this crate free of process-spawning side effects.

pub mod mock;
pub mod runner;
pub mod tmux;

use cmx_utils::response::Action;

/// Trait for session management backends. Implementations translate abstract
/// actions into backend-specific operations.
pub trait SessionBackend {
    /// Execute a single action against the backend.
    fn execute_action(&mut self, action: &Action) -> Result<(), String>;

    /// Check whether a session with the given name exists.
    fn session_exists(&self, name: &str) -> bool;

    /// Return the names of all known sessions.
    fn list_sessions(&self) -> Vec<String>;

    /// Capture the current content of a pane, identified by a target string
    /// (e.g. `"session:window.pane"`).
    fn capture_pane(&self, target: &str) -> Result<String, String>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::mock::MockBackend;

    #[test]
    fn mock_implements_session_backend() {
        let backend = MockBackend::new();
        // Ensure the trait object can be constructed.
        let _: &dyn SessionBackend = &backend;
    }
}
