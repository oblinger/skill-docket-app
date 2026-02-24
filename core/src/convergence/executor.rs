//! Convergence executor — runs planned actions through a backend with retry.

use crate::convergence::retry::{ActionRetryTracker, RetryPolicy};
use crate::infrastructure::SessionBackend;
use cmx_utils::response::Action;

/// Outcome of a convergence execution pass.
#[derive(Debug, Clone)]
pub struct ConvergenceResult {
    pub succeeded: Vec<Action>,
    pub failed: Vec<(Action, String)>,
    pub retries_used: u32,
}

/// Executes a batch of actions through a `SessionBackend`, retrying failures
/// according to the configured policy.
pub struct ConvergenceExecutor {
    retry_tracker: ActionRetryTracker,
}

impl ConvergenceExecutor {
    pub fn new(policy: RetryPolicy) -> Self {
        ConvergenceExecutor {
            retry_tracker: ActionRetryTracker::new(policy),
        }
    }

    pub fn execute(
        &mut self,
        actions: Vec<Action>,
        backend: &mut dyn SessionBackend,
    ) -> ConvergenceResult {
        let mut succeeded = Vec::new();
        let mut failed = Vec::new();
        let mut retries_used: u32 = 0;
        let mut pending = actions;

        loop {
            let mut still_failing = Vec::new();
            let mut last_errors: Vec<(Action, String)> = Vec::new();

            for action in pending {
                let key = action_key(&action);
                match backend.execute_action(&action) {
                    Ok(()) => {
                        self.retry_tracker.record_success(&key);
                        succeeded.push(action);
                    }
                    Err(e) => {
                        self.retry_tracker.record_failure(&key);
                        if self.retry_tracker.can_retry(&key) {
                            retries_used += 1;
                            still_failing.push(action);
                        } else {
                            last_errors.push((action, e));
                        }
                    }
                }
            }

            failed.extend(last_errors);

            if still_failing.is_empty() {
                break;
            }
            pending = still_failing;
        }

        ConvergenceResult {
            succeeded,
            failed,
            retries_used,
        }
    }
}

fn action_key(action: &Action) -> String {
    match action {
        Action::CreateSession { name, .. } => format!("create_session:{}", name),
        Action::KillSession { name } => format!("kill_session:{}", name),
        Action::SplitPane { session, .. } => format!("split_pane:{}", session),
        Action::PlaceAgent { pane_id, agent } => format!("place_agent:{}:{}", pane_id, agent),
        Action::CreateAgent { name, .. } => format!("create_agent:{}", name),
        Action::KillAgent { name } => format!("kill_agent:{}", name),
        Action::ConnectSsh { agent, host, .. } => format!("connect_ssh:{}:{}", agent, host),
        Action::UpdateAssignment { agent, .. } => format!("update_assignment:{}", agent),
        Action::SendKeys { target, .. } => format!("send_keys:{}", target),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::mock::MockBackend;
    use crate::types::config::BackoffStrategy;

    struct FailNBackend {
        fail_count: u32,
        calls: u32,
        sessions: Vec<String>,
    }

    impl FailNBackend {
        fn new(fail_count: u32) -> Self {
            FailNBackend { fail_count, calls: 0, sessions: Vec::new() }
        }
    }

    impl SessionBackend for FailNBackend {
        fn execute_action(&mut self, action: &Action) -> Result<(), String> {
            self.calls += 1;
            if self.calls <= self.fail_count {
                return Err(format!("Simulated failure #{}", self.calls));
            }
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
            Ok(())
        }
        fn session_exists(&self, name: &str) -> bool {
            self.sessions.iter().any(|s| s == name)
        }
        fn list_sessions(&self) -> Vec<String> {
            self.sessions.clone()
        }
        fn capture_pane(&self, _target: &str) -> Result<String, String> {
            Err("not supported".into())
        }
    }

    struct AlwaysFailBackend;

    impl SessionBackend for AlwaysFailBackend {
        fn execute_action(&mut self, _action: &Action) -> Result<(), String> {
            Err("permanent failure".into())
        }
        fn session_exists(&self, _name: &str) -> bool { false }
        fn list_sessions(&self) -> Vec<String> { Vec::new() }
        fn capture_pane(&self, _target: &str) -> Result<String, String> {
            Err("not supported".into())
        }
    }

    #[test]
    fn executor_runs_all_actions_on_success() {
        let policy = RetryPolicy::new(3, BackoffStrategy::Fixed, 100);
        let mut executor = ConvergenceExecutor::new(policy);
        let mut backend = MockBackend::new();
        let actions = vec![
            Action::CreateSession { name: "s1".into(), cwd: "/tmp".into() },
            Action::CreateAgent { name: "w1".into(), role: "worker".into(), path: "/tmp".into() },
        ];
        let result = executor.execute(actions, &mut backend);
        assert_eq!(result.succeeded.len(), 2);
        assert!(result.failed.is_empty());
        assert_eq!(result.retries_used, 0);
    }

    #[test]
    fn executor_retries_failed_action_and_succeeds() {
        let policy = RetryPolicy::new(3, BackoffStrategy::Fixed, 100);
        let mut executor = ConvergenceExecutor::new(policy);
        let mut backend = FailNBackend::new(1);
        let actions = vec![Action::CreateSession { name: "retry-me".into(), cwd: "/tmp".into() }];
        let result = executor.execute(actions, &mut backend);
        assert_eq!(result.succeeded.len(), 1);
        assert!(result.failed.is_empty());
        assert_eq!(result.retries_used, 1);
    }

    #[test]
    fn executor_gives_up_after_max_retries() {
        let policy = RetryPolicy::new(3, BackoffStrategy::Fixed, 100);
        let mut executor = ConvergenceExecutor::new(policy);
        let mut backend = AlwaysFailBackend;
        let actions = vec![Action::KillSession { name: "doomed".into() }];
        let result = executor.execute(actions, &mut backend);
        assert!(result.succeeded.is_empty());
        assert_eq!(result.failed.len(), 1);
        assert_eq!(result.failed[0].0, Action::KillSession { name: "doomed".into() });
        assert_eq!(result.retries_used, 2);
    }

    #[test]
    fn executor_partial_failure() {
        let policy = RetryPolicy::new(0, BackoffStrategy::Fixed, 100);
        let mut executor = ConvergenceExecutor::new(policy);

        struct PartialBackend;
        impl SessionBackend for PartialBackend {
            fn execute_action(&mut self, action: &Action) -> Result<(), String> {
                match action {
                    Action::CreateSession { .. } => Ok(()),
                    _ => Err("not supported".into()),
                }
            }
            fn session_exists(&self, _name: &str) -> bool { false }
            fn list_sessions(&self) -> Vec<String> { Vec::new() }
            fn capture_pane(&self, _target: &str) -> Result<String, String> { Err("nope".into()) }
        }

        let mut backend = PartialBackend;
        let actions = vec![
            Action::CreateSession { name: "good".into(), cwd: "/tmp".into() },
            Action::KillAgent { name: "bad".into() },
        ];
        let result = executor.execute(actions, &mut backend);
        assert_eq!(result.succeeded.len(), 1);
        assert_eq!(result.failed.len(), 1);
    }

    #[test]
    fn executor_action_key_uniqueness() {
        let a1 = Action::CreateSession { name: "s1".into(), cwd: "/tmp".into() };
        let a2 = Action::KillSession { name: "s1".into() };
        let a3 = Action::CreateAgent { name: "s1".into(), role: "w".into(), path: "/".into() };
        assert_ne!(action_key(&a1), action_key(&a2));
        assert_ne!(action_key(&a1), action_key(&a3));
        assert_ne!(action_key(&a2), action_key(&a3));
    }
}
