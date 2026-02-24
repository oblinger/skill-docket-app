//! Task executor — manages a fleet of task executions.
//!
//! Tracks execution lifecycle from submission through completion or failure.
//! Enforces concurrency limits and provides query methods for filtering
//! executions by agent, task, or state.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// ExecutionState
// ---------------------------------------------------------------------------

/// The lifecycle state of a single execution.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ExecutionState {
    Queued,
    Preparing,
    Running {
        started_ms: u64,
        pid: Option<u32>,
    },
    Paused {
        reason: String,
    },
    Completed {
        exit_code: i32,
        finished_ms: u64,
    },
    Failed {
        error: String,
        finished_ms: u64,
    },
    Cancelled {
        reason: String,
        cancelled_ms: u64,
    },
    TimedOut {
        deadline_ms: u64,
    },
}

impl ExecutionState {
    /// Whether this state represents a terminal (finished) execution.
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            ExecutionState::Completed { .. }
                | ExecutionState::Failed { .. }
                | ExecutionState::Cancelled { .. }
                | ExecutionState::TimedOut { .. }
        )
    }

    /// Whether this state represents a running execution.
    pub fn is_running(&self) -> bool {
        matches!(self, ExecutionState::Running { .. })
    }

    /// Whether this state represents a queued execution.
    pub fn is_queued(&self) -> bool {
        matches!(self, ExecutionState::Queued)
    }
}

// ---------------------------------------------------------------------------
// Execution
// ---------------------------------------------------------------------------

/// A single task execution with its full context.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Execution {
    pub id: String,
    pub task_id: String,
    pub agent: String,
    pub state: ExecutionState,
    pub command: Vec<String>,
    pub working_dir: String,
    pub env: HashMap<String, String>,
    pub timeout_ms: Option<u64>,
    pub created_ms: u64,
    pub priority: u32,
}

// ---------------------------------------------------------------------------
// ExecutionStats
// ---------------------------------------------------------------------------

/// Aggregate statistics across all executions in the executor.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ExecutionStats {
    pub total: usize,
    pub queued: usize,
    pub running: usize,
    pub completed: usize,
    pub failed: usize,
    pub cancelled: usize,
    pub timed_out: usize,
    pub avg_duration_ms: Option<u64>,
}

// ---------------------------------------------------------------------------
// TaskExecutor
// ---------------------------------------------------------------------------

/// Manages a fleet of task executions with concurrency control.
#[derive(Debug)]
pub struct TaskExecutor {
    executions: HashMap<String, Execution>,
    max_concurrent: usize,
}

impl TaskExecutor {
    /// Create a new executor with the given concurrency limit.
    pub fn new(max_concurrent: usize) -> Self {
        TaskExecutor {
            executions: HashMap::new(),
            max_concurrent,
        }
    }

    /// Submit a new execution. Fails if the ID already exists.
    pub fn submit(&mut self, execution: Execution) -> Result<(), String> {
        if self.executions.contains_key(&execution.id) {
            return Err(format!("execution '{}' already exists", execution.id));
        }
        if !matches!(execution.state, ExecutionState::Queued) {
            return Err("new executions must be in Queued state".into());
        }
        self.executions.insert(execution.id.clone(), execution);
        Ok(())
    }

    /// Transition an execution to Running state.
    pub fn start(&mut self, id: &str, now_ms: u64) -> Result<(), String> {
        // Check state first (immutable borrow).
        match self.executions.get(id) {
            None => return Err(format!("execution '{}' not found", id)),
            Some(exec) => match &exec.state {
                ExecutionState::Queued | ExecutionState::Preparing => {}
                other => {
                    return Err(format!(
                        "cannot start execution in {:?} state",
                        std::mem::discriminant(other)
                    ));
                }
            },
        }

        // Enforce concurrency limit (immutable borrow).
        let running_count = self.running_count();
        if running_count >= self.max_concurrent {
            return Err(format!(
                "concurrency limit reached ({}/{})",
                running_count, self.max_concurrent
            ));
        }

        // Now mutate (mutable borrow).
        let exec = self.executions.get_mut(id).unwrap();
        exec.state = ExecutionState::Running {
            started_ms: now_ms,
            pid: None,
        };
        Ok(())
    }

    /// Mark an execution as completed with the given exit code.
    pub fn complete(&mut self, id: &str, exit_code: i32, now_ms: u64) -> Result<(), String> {
        let exec = self
            .executions
            .get_mut(id)
            .ok_or_else(|| format!("execution '{}' not found", id))?;

        if !exec.state.is_running() {
            return Err(format!("execution '{}' is not running", id));
        }

        exec.state = ExecutionState::Completed {
            exit_code,
            finished_ms: now_ms,
        };
        Ok(())
    }

    /// Mark an execution as failed with an error message.
    pub fn fail(&mut self, id: &str, error: &str, now_ms: u64) -> Result<(), String> {
        let exec = self
            .executions
            .get_mut(id)
            .ok_or_else(|| format!("execution '{}' not found", id))?;

        if exec.state.is_terminal() {
            return Err(format!("execution '{}' is already terminal", id));
        }

        exec.state = ExecutionState::Failed {
            error: error.to_string(),
            finished_ms: now_ms,
        };
        Ok(())
    }

    /// Cancel an execution with a reason.
    pub fn cancel(&mut self, id: &str, reason: &str, now_ms: u64) -> Result<(), String> {
        let exec = self
            .executions
            .get_mut(id)
            .ok_or_else(|| format!("execution '{}' not found", id))?;

        if exec.state.is_terminal() {
            return Err(format!("execution '{}' is already terminal", id));
        }

        exec.state = ExecutionState::Cancelled {
            reason: reason.to_string(),
            cancelled_ms: now_ms,
        };
        Ok(())
    }

    /// Check all running executions for timeout violations. Returns IDs of
    /// executions that were timed out.
    pub fn timeout_check(&mut self, now_ms: u64) -> Vec<String> {
        let mut timed_out = Vec::new();

        // Collect candidates first to avoid borrow issues.
        let candidates: Vec<(String, u64, u64)> = self
            .executions
            .iter()
            .filter_map(|(id, exec)| {
                if let ExecutionState::Running { started_ms, .. } = &exec.state {
                    exec.timeout_ms.map(|timeout| (id.clone(), *started_ms, timeout))
                } else {
                    None
                }
            })
            .collect();

        for (id, started_ms, timeout) in candidates {
            if now_ms.saturating_sub(started_ms) >= timeout {
                if let Some(exec) = self.executions.get_mut(&id) {
                    exec.state = ExecutionState::TimedOut {
                        deadline_ms: started_ms + timeout,
                    };
                    timed_out.push(id);
                }
            }
        }

        timed_out
    }

    /// Pause a running execution.
    pub fn pause(&mut self, id: &str, reason: &str) -> Result<(), String> {
        let exec = self
            .executions
            .get_mut(id)
            .ok_or_else(|| format!("execution '{}' not found", id))?;

        if !exec.state.is_running() {
            return Err(format!("execution '{}' is not running", id));
        }

        exec.state = ExecutionState::Paused {
            reason: reason.to_string(),
        };
        Ok(())
    }

    /// Resume a paused execution.
    pub fn resume(&mut self, id: &str, now_ms: u64) -> Result<(), String> {
        // Check state first (immutable borrow).
        match self.executions.get(id) {
            None => return Err(format!("execution '{}' not found", id)),
            Some(exec) => {
                if !matches!(exec.state, ExecutionState::Paused { .. }) {
                    return Err(format!("execution '{}' is not paused", id));
                }
            }
        }

        // Check concurrency before resuming (immutable borrow).
        let running_count = self.running_count();
        if running_count >= self.max_concurrent {
            return Err(format!(
                "concurrency limit reached ({}/{})",
                running_count, self.max_concurrent
            ));
        }

        // Now mutate (mutable borrow).
        let exec = self.executions.get_mut(id).unwrap();
        exec.state = ExecutionState::Running {
            started_ms: now_ms,
            pid: None,
        };
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Query methods
    // -----------------------------------------------------------------------

    /// Return all currently running executions.
    pub fn running(&self) -> Vec<&Execution> {
        self.executions
            .values()
            .filter(|e| e.state.is_running())
            .collect()
    }

    /// Return all queued executions, sorted by priority (highest first).
    pub fn queued(&self) -> Vec<&Execution> {
        let mut q: Vec<&Execution> = self
            .executions
            .values()
            .filter(|e| e.state.is_queued())
            .collect();
        q.sort_by(|a, b| b.priority.cmp(&a.priority));
        q
    }

    /// Return all executions assigned to the given agent.
    pub fn by_agent(&self, name: &str) -> Vec<&Execution> {
        self.executions
            .values()
            .filter(|e| e.agent == name)
            .collect()
    }

    /// Return all executions for the given task ID.
    pub fn by_task(&self, task_id: &str) -> Vec<&Execution> {
        self.executions
            .values()
            .filter(|e| e.task_id == task_id)
            .collect()
    }

    /// Return the execution history for a task (all terminal executions),
    /// sorted by creation time.
    pub fn history_for(&self, task_id: &str) -> Vec<&Execution> {
        let mut history: Vec<&Execution> = self
            .executions
            .values()
            .filter(|e| e.task_id == task_id && e.state.is_terminal())
            .collect();
        history.sort_by_key(|e| e.created_ms);
        history
    }

    /// Get a single execution by ID.
    pub fn get(&self, id: &str) -> Option<&Execution> {
        self.executions.get(id)
    }

    /// Aggregate statistics across all executions.
    pub fn stats(&self) -> ExecutionStats {
        let mut stats = ExecutionStats {
            total: self.executions.len(),
            queued: 0,
            running: 0,
            completed: 0,
            failed: 0,
            cancelled: 0,
            timed_out: 0,
            avg_duration_ms: None,
        };

        let mut total_duration: u64 = 0;
        let mut duration_count: u64 = 0;

        for exec in self.executions.values() {
            match &exec.state {
                ExecutionState::Queued => stats.queued += 1,
                ExecutionState::Preparing => stats.queued += 1,
                ExecutionState::Running { .. } => stats.running += 1,
                ExecutionState::Paused { .. } => stats.running += 1,
                ExecutionState::Completed { finished_ms, .. } => {
                    stats.completed += 1;
                    total_duration += finished_ms.saturating_sub(exec.created_ms);
                    duration_count += 1;
                }
                ExecutionState::Failed { finished_ms, .. } => {
                    stats.failed += 1;
                    total_duration += finished_ms.saturating_sub(exec.created_ms);
                    duration_count += 1;
                }
                ExecutionState::Cancelled { .. } => stats.cancelled += 1,
                ExecutionState::TimedOut { .. } => stats.timed_out += 1,
            }
        }

        if duration_count > 0 {
            stats.avg_duration_ms = Some(total_duration / duration_count);
        }

        stats
    }

    /// Count of currently running executions.
    fn running_count(&self) -> usize {
        self.executions
            .values()
            .filter(|e| e.state.is_running())
            .count()
    }
}

// ---------------------------------------------------------------------------
// Helper: create a test execution
// ---------------------------------------------------------------------------

#[cfg(test)]
fn make_execution(id: &str, task_id: &str, agent: &str, priority: u32) -> Execution {
    Execution {
        id: id.into(),
        task_id: task_id.into(),
        agent: agent.into(),
        state: ExecutionState::Queued,
        command: vec!["cargo".into(), "test".into()],
        working_dir: "/tmp".into(),
        env: HashMap::new(),
        timeout_ms: None,
        created_ms: 1000,
        priority,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn submit_and_get() {
        let mut ex = TaskExecutor::new(4);
        let e = make_execution("e1", "T1", "worker-1", 5);
        assert!(ex.submit(e).is_ok());
        assert!(ex.get("e1").is_some());
        assert_eq!(ex.get("e1").unwrap().task_id, "T1");
    }

    #[test]
    fn submit_duplicate_fails() {
        let mut ex = TaskExecutor::new(4);
        ex.submit(make_execution("e1", "T1", "w1", 1)).unwrap();
        let result = ex.submit(make_execution("e1", "T2", "w2", 1));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("already exists"));
    }

    #[test]
    fn submit_non_queued_fails() {
        let mut ex = TaskExecutor::new(4);
        let mut e = make_execution("e1", "T1", "w1", 1);
        e.state = ExecutionState::Running {
            started_ms: 100,
            pid: None,
        };
        assert!(ex.submit(e).is_err());
    }

    #[test]
    fn start_transitions_to_running() {
        let mut ex = TaskExecutor::new(4);
        ex.submit(make_execution("e1", "T1", "w1", 1)).unwrap();
        assert!(ex.start("e1", 2000).is_ok());

        let e = ex.get("e1").unwrap();
        assert!(matches!(
            e.state,
            ExecutionState::Running { started_ms: 2000, .. }
        ));
    }

    #[test]
    fn start_nonexistent_fails() {
        let mut ex = TaskExecutor::new(4);
        assert!(ex.start("nope", 1000).is_err());
    }

    #[test]
    fn start_already_running_fails() {
        let mut ex = TaskExecutor::new(4);
        ex.submit(make_execution("e1", "T1", "w1", 1)).unwrap();
        ex.start("e1", 1000).unwrap();
        assert!(ex.start("e1", 2000).is_err());
    }

    #[test]
    fn complete_from_running() {
        let mut ex = TaskExecutor::new(4);
        ex.submit(make_execution("e1", "T1", "w1", 1)).unwrap();
        ex.start("e1", 1000).unwrap();
        assert!(ex.complete("e1", 0, 5000).is_ok());

        let e = ex.get("e1").unwrap();
        assert!(matches!(
            e.state,
            ExecutionState::Completed {
                exit_code: 0,
                finished_ms: 5000
            }
        ));
    }

    #[test]
    fn complete_not_running_fails() {
        let mut ex = TaskExecutor::new(4);
        ex.submit(make_execution("e1", "T1", "w1", 1)).unwrap();
        assert!(ex.complete("e1", 0, 5000).is_err());
    }

    #[test]
    fn fail_from_running() {
        let mut ex = TaskExecutor::new(4);
        ex.submit(make_execution("e1", "T1", "w1", 1)).unwrap();
        ex.start("e1", 1000).unwrap();
        assert!(ex.fail("e1", "segfault", 3000).is_ok());

        let e = ex.get("e1").unwrap();
        match &e.state {
            ExecutionState::Failed { error, finished_ms } => {
                assert_eq!(error, "segfault");
                assert_eq!(*finished_ms, 3000);
            }
            other => panic!("expected Failed, got {:?}", other),
        }
    }

    #[test]
    fn fail_from_queued() {
        let mut ex = TaskExecutor::new(4);
        ex.submit(make_execution("e1", "T1", "w1", 1)).unwrap();
        assert!(ex.fail("e1", "bad config", 2000).is_ok());
    }

    #[test]
    fn fail_terminal_fails() {
        let mut ex = TaskExecutor::new(4);
        ex.submit(make_execution("e1", "T1", "w1", 1)).unwrap();
        ex.start("e1", 1000).unwrap();
        ex.complete("e1", 0, 2000).unwrap();
        assert!(ex.fail("e1", "too late", 3000).is_err());
    }

    #[test]
    fn cancel_from_queued() {
        let mut ex = TaskExecutor::new(4);
        ex.submit(make_execution("e1", "T1", "w1", 1)).unwrap();
        assert!(ex.cancel("e1", "user requested", 2000).is_ok());

        let e = ex.get("e1").unwrap();
        assert!(matches!(e.state, ExecutionState::Cancelled { .. }));
    }

    #[test]
    fn cancel_from_running() {
        let mut ex = TaskExecutor::new(4);
        ex.submit(make_execution("e1", "T1", "w1", 1)).unwrap();
        ex.start("e1", 1000).unwrap();
        assert!(ex.cancel("e1", "priority change", 3000).is_ok());
    }

    #[test]
    fn cancel_terminal_fails() {
        let mut ex = TaskExecutor::new(4);
        ex.submit(make_execution("e1", "T1", "w1", 1)).unwrap();
        ex.start("e1", 1000).unwrap();
        ex.fail("e1", "boom", 2000).unwrap();
        assert!(ex.cancel("e1", "too late", 3000).is_err());
    }

    #[test]
    fn concurrency_limit_enforced() {
        let mut ex = TaskExecutor::new(2);
        ex.submit(make_execution("e1", "T1", "w1", 1)).unwrap();
        ex.submit(make_execution("e2", "T2", "w2", 1)).unwrap();
        ex.submit(make_execution("e3", "T3", "w3", 1)).unwrap();

        ex.start("e1", 1000).unwrap();
        ex.start("e2", 1000).unwrap();
        let result = ex.start("e3", 1000);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("concurrency limit"));
    }

    #[test]
    fn concurrency_freed_after_complete() {
        let mut ex = TaskExecutor::new(1);
        ex.submit(make_execution("e1", "T1", "w1", 1)).unwrap();
        ex.submit(make_execution("e2", "T2", "w2", 1)).unwrap();

        ex.start("e1", 1000).unwrap();
        assert!(ex.start("e2", 1000).is_err());

        ex.complete("e1", 0, 2000).unwrap();
        assert!(ex.start("e2", 3000).is_ok());
    }

    #[test]
    fn timeout_check_marks_overdue() {
        let mut ex = TaskExecutor::new(4);
        let mut e = make_execution("e1", "T1", "w1", 1);
        e.timeout_ms = Some(5000);
        ex.submit(e).unwrap();
        ex.start("e1", 1000).unwrap();

        // Not timed out yet.
        let timed = ex.timeout_check(5000);
        assert!(timed.is_empty());

        // Now timed out.
        let timed = ex.timeout_check(6001);
        assert_eq!(timed, vec!["e1"]);

        let e = ex.get("e1").unwrap();
        assert!(matches!(e.state, ExecutionState::TimedOut { .. }));
    }

    #[test]
    fn timeout_ignores_no_timeout() {
        let mut ex = TaskExecutor::new(4);
        let e = make_execution("e1", "T1", "w1", 1);
        // timeout_ms is None
        ex.submit(e).unwrap();
        ex.start("e1", 1000).unwrap();

        let timed = ex.timeout_check(999999);
        assert!(timed.is_empty());
    }

    #[test]
    fn pause_and_resume() {
        let mut ex = TaskExecutor::new(4);
        ex.submit(make_execution("e1", "T1", "w1", 1)).unwrap();
        ex.start("e1", 1000).unwrap();

        assert!(ex.pause("e1", "user break").is_ok());
        assert!(matches!(
            ex.get("e1").unwrap().state,
            ExecutionState::Paused { .. }
        ));

        assert!(ex.resume("e1", 5000).is_ok());
        assert!(ex.get("e1").unwrap().state.is_running());
    }

    #[test]
    fn pause_non_running_fails() {
        let mut ex = TaskExecutor::new(4);
        ex.submit(make_execution("e1", "T1", "w1", 1)).unwrap();
        assert!(ex.pause("e1", "why").is_err());
    }

    #[test]
    fn resume_non_paused_fails() {
        let mut ex = TaskExecutor::new(4);
        ex.submit(make_execution("e1", "T1", "w1", 1)).unwrap();
        assert!(ex.resume("e1", 5000).is_err());
    }

    #[test]
    fn resume_respects_concurrency() {
        let mut ex = TaskExecutor::new(1);
        ex.submit(make_execution("e1", "T1", "w1", 1)).unwrap();
        ex.submit(make_execution("e2", "T2", "w2", 1)).unwrap();

        ex.start("e1", 1000).unwrap();
        ex.pause("e1", "break").unwrap();
        ex.start("e2", 2000).unwrap();

        // e2 is running, so e1 can't resume.
        assert!(ex.resume("e1", 3000).is_err());
    }

    #[test]
    fn running_query() {
        let mut ex = TaskExecutor::new(4);
        ex.submit(make_execution("e1", "T1", "w1", 1)).unwrap();
        ex.submit(make_execution("e2", "T2", "w2", 1)).unwrap();
        ex.submit(make_execution("e3", "T3", "w3", 1)).unwrap();

        ex.start("e1", 1000).unwrap();
        ex.start("e2", 1000).unwrap();

        let running = ex.running();
        assert_eq!(running.len(), 2);
    }

    #[test]
    fn queued_sorted_by_priority() {
        let mut ex = TaskExecutor::new(4);
        ex.submit(make_execution("e1", "T1", "w1", 1)).unwrap();
        ex.submit(make_execution("e2", "T2", "w2", 10)).unwrap();
        ex.submit(make_execution("e3", "T3", "w3", 5)).unwrap();

        let queued = ex.queued();
        assert_eq!(queued.len(), 3);
        assert_eq!(queued[0].id, "e2"); // priority 10
        assert_eq!(queued[1].id, "e3"); // priority 5
        assert_eq!(queued[2].id, "e1"); // priority 1
    }

    #[test]
    fn by_agent_filters() {
        let mut ex = TaskExecutor::new(4);
        ex.submit(make_execution("e1", "T1", "w1", 1)).unwrap();
        ex.submit(make_execution("e2", "T2", "w1", 1)).unwrap();
        ex.submit(make_execution("e3", "T3", "w2", 1)).unwrap();

        assert_eq!(ex.by_agent("w1").len(), 2);
        assert_eq!(ex.by_agent("w2").len(), 1);
        assert_eq!(ex.by_agent("w3").len(), 0);
    }

    #[test]
    fn by_task_filters() {
        let mut ex = TaskExecutor::new(4);
        ex.submit(make_execution("e1", "T1", "w1", 1)).unwrap();
        ex.submit(make_execution("e2", "T1", "w2", 1)).unwrap();
        ex.submit(make_execution("e3", "T2", "w3", 1)).unwrap();

        assert_eq!(ex.by_task("T1").len(), 2);
        assert_eq!(ex.by_task("T2").len(), 1);
    }

    #[test]
    fn history_for_returns_terminal_only() {
        let mut ex = TaskExecutor::new(4);
        ex.submit(make_execution("e1", "T1", "w1", 1)).unwrap();
        ex.submit(make_execution("e2", "T1", "w2", 1)).unwrap();
        ex.submit(make_execution("e3", "T1", "w3", 1)).unwrap();

        ex.start("e1", 1000).unwrap();
        ex.complete("e1", 0, 2000).unwrap();
        ex.start("e2", 1000).unwrap();
        ex.fail("e2", "oom", 3000).unwrap();
        // e3 stays queued

        let history = ex.history_for("T1");
        assert_eq!(history.len(), 2);
    }

    #[test]
    fn stats_empty() {
        let ex = TaskExecutor::new(4);
        let s = ex.stats();
        assert_eq!(s.total, 0);
        assert!(s.avg_duration_ms.is_none());
    }

    #[test]
    fn stats_mixed() {
        let mut ex = TaskExecutor::new(4);
        let mut e1 = make_execution("e1", "T1", "w1", 1);
        e1.created_ms = 1000;
        let mut e2 = make_execution("e2", "T2", "w2", 1);
        e2.created_ms = 1000;
        let e3 = make_execution("e3", "T3", "w3", 1);

        ex.submit(e1).unwrap();
        ex.submit(e2).unwrap();
        ex.submit(e3).unwrap();

        ex.start("e1", 1000).unwrap();
        ex.complete("e1", 0, 5000).unwrap(); // duration: 5000-1000 = 4000
        ex.start("e2", 1000).unwrap();
        ex.fail("e2", "err", 3000).unwrap(); // duration: 3000-1000 = 2000

        let s = ex.stats();
        assert_eq!(s.total, 3);
        assert_eq!(s.completed, 1);
        assert_eq!(s.failed, 1);
        assert_eq!(s.queued, 1);
        assert_eq!(s.running, 0);
        assert_eq!(s.avg_duration_ms, Some(3000)); // (4000+2000)/2
    }

    #[test]
    fn stats_cancelled_and_timed_out() {
        let mut ex = TaskExecutor::new(4);
        ex.submit(make_execution("e1", "T1", "w1", 1)).unwrap();
        ex.submit(make_execution("e2", "T2", "w2", 1)).unwrap();

        ex.cancel("e1", "nope", 2000).unwrap();
        let mut e2m = make_execution("e2x", "T2", "w2", 1);
        e2m.timeout_ms = Some(1000);
        ex.submit(e2m).unwrap();
        ex.start("e2x", 1000).unwrap();
        ex.timeout_check(3000);

        let s = ex.stats();
        assert_eq!(s.cancelled, 1);
        assert_eq!(s.timed_out, 1);
    }

    #[test]
    fn execution_state_is_terminal() {
        assert!(!ExecutionState::Queued.is_terminal());
        assert!(!ExecutionState::Preparing.is_terminal());
        assert!(!(ExecutionState::Running {
            started_ms: 0,
            pid: None,
        })
        .is_terminal());
        assert!(!(ExecutionState::Paused {
            reason: "x".into(),
        })
        .is_terminal());
        assert!((ExecutionState::Completed {
            exit_code: 0,
            finished_ms: 0,
        })
        .is_terminal());
        assert!((ExecutionState::Failed {
            error: "e".into(),
            finished_ms: 0,
        })
        .is_terminal());
        assert!((ExecutionState::Cancelled {
            reason: "r".into(),
            cancelled_ms: 0,
        })
        .is_terminal());
        assert!((ExecutionState::TimedOut { deadline_ms: 0 }).is_terminal());
    }

    #[test]
    fn execution_serde_round_trip() {
        let e = make_execution("e1", "T1", "w1", 5);
        let json = serde_json::to_string(&e).unwrap();
        let back: Execution = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, "e1");
        assert_eq!(back.priority, 5);
    }

    #[test]
    fn execution_state_serde_variants() {
        let states = vec![
            ExecutionState::Queued,
            ExecutionState::Preparing,
            ExecutionState::Running {
                started_ms: 100,
                pid: Some(42),
            },
            ExecutionState::Paused {
                reason: "lunch".into(),
            },
            ExecutionState::Completed {
                exit_code: 0,
                finished_ms: 200,
            },
            ExecutionState::Failed {
                error: "oom".into(),
                finished_ms: 300,
            },
            ExecutionState::Cancelled {
                reason: "user".into(),
                cancelled_ms: 400,
            },
            ExecutionState::TimedOut { deadline_ms: 500 },
        ];
        for state in states {
            let json = serde_json::to_string(&state).unwrap();
            let back: ExecutionState = serde_json::from_str(&json).unwrap();
            assert_eq!(back, state);
        }
    }

    #[test]
    fn stats_serde_round_trip() {
        let s = ExecutionStats {
            total: 10,
            queued: 2,
            running: 3,
            completed: 4,
            failed: 1,
            cancelled: 0,
            timed_out: 0,
            avg_duration_ms: Some(5000),
        };
        let json = serde_json::to_string(&s).unwrap();
        let back: ExecutionStats = serde_json::from_str(&json).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn multiple_timeouts_at_once() {
        let mut ex = TaskExecutor::new(10);
        for i in 0..5 {
            let mut e = make_execution(&format!("e{}", i), "T1", "w1", 1);
            e.timeout_ms = Some(1000);
            ex.submit(e).unwrap();
            ex.start(&format!("e{}", i), 1000).unwrap();
        }

        let timed = ex.timeout_check(3000);
        assert_eq!(timed.len(), 5);
    }

    #[test]
    fn get_nonexistent_returns_none() {
        let ex = TaskExecutor::new(4);
        assert!(ex.get("nope").is_none());
    }

    #[test]
    fn fail_nonexistent_returns_error() {
        let mut ex = TaskExecutor::new(4);
        assert!(ex.fail("nope", "err", 1000).is_err());
    }

    #[test]
    fn cancel_nonexistent_returns_error() {
        let mut ex = TaskExecutor::new(4);
        assert!(ex.cancel("nope", "r", 1000).is_err());
    }

    #[test]
    fn complete_nonexistent_returns_error() {
        let mut ex = TaskExecutor::new(4);
        assert!(ex.complete("nope", 0, 1000).is_err());
    }

    #[test]
    fn pause_nonexistent_returns_error() {
        let mut ex = TaskExecutor::new(4);
        assert!(ex.pause("nope", "r").is_err());
    }

    #[test]
    fn resume_nonexistent_returns_error() {
        let mut ex = TaskExecutor::new(4);
        assert!(ex.resume("nope", 1000).is_err());
    }

    #[test]
    fn preparing_can_start() {
        let mut ex = TaskExecutor::new(4);
        let e = make_execution("e1", "T1", "w1", 1);
        // Manually set to Preparing via internal access for test.
        ex.submit(e).unwrap();
        // Simulate prepare step by directly modifying.
        ex.executions.get_mut("e1").unwrap().state = ExecutionState::Preparing;
        assert!(ex.start("e1", 2000).is_ok());
    }

    #[test]
    fn concurrency_limit_one() {
        let mut ex = TaskExecutor::new(1);
        ex.submit(make_execution("e1", "T1", "w1", 1)).unwrap();
        ex.submit(make_execution("e2", "T2", "w2", 1)).unwrap();

        ex.start("e1", 1000).unwrap();
        assert!(ex.start("e2", 1000).is_err());
    }

    #[test]
    fn history_sorted_by_created_ms() {
        let mut ex = TaskExecutor::new(4);
        let mut e1 = make_execution("e1", "T1", "w1", 1);
        e1.created_ms = 3000;
        let mut e2 = make_execution("e2", "T1", "w2", 1);
        e2.created_ms = 1000;
        let mut e3 = make_execution("e3", "T1", "w3", 1);
        e3.created_ms = 2000;

        ex.submit(e1).unwrap();
        ex.submit(e2).unwrap();
        ex.submit(e3).unwrap();

        ex.start("e1", 3000).unwrap();
        ex.complete("e1", 0, 4000).unwrap();
        ex.start("e2", 1000).unwrap();
        ex.complete("e2", 0, 2000).unwrap();
        ex.start("e3", 2000).unwrap();
        ex.fail("e3", "err", 2500).unwrap();

        let history = ex.history_for("T1");
        assert_eq!(history.len(), 3);
        assert_eq!(history[0].id, "e2"); // created_ms 1000
        assert_eq!(history[1].id, "e3"); // created_ms 2000
        assert_eq!(history[2].id, "e1"); // created_ms 3000
    }

    #[test]
    fn timeout_boundary_exact() {
        let mut ex = TaskExecutor::new(4);
        let mut e = make_execution("e1", "T1", "w1", 1);
        e.timeout_ms = Some(5000);
        ex.submit(e).unwrap();
        ex.start("e1", 1000).unwrap();

        // Exactly at deadline: 1000 + 5000 = 6000
        let timed = ex.timeout_check(6000);
        assert_eq!(timed.len(), 1);
    }

    #[test]
    fn execution_with_env() {
        let e = {
            let mut ex = make_execution("e1", "T1", "w1", 1);
            ex.env.insert("RUST_LOG".into(), "debug".into());
            ex.env.insert("HOME".into(), "/home/test".into());
            ex
        };

        let json = serde_json::to_string(&e).unwrap();
        let back: Execution = serde_json::from_str(&json).unwrap();
        assert_eq!(back.env.get("RUST_LOG").unwrap(), "debug");
    }
}
