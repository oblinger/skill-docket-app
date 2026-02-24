//! Remote worker lifecycle management.
//!
//! A `RemoteWorker` represents an agent instance running on a remote host.
//! Workers move through a defined lifecycle: provisioning, syncing code,
//! becoming ready, executing tasks, collecting results, and eventually being
//! decommissioned. `WorkerPool` manages a collection of workers with per-remote
//! capacity limits.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};


// ---------------------------------------------------------------------------
// WorkerState
// ---------------------------------------------------------------------------

/// The lifecycle state of a remote worker.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorkerState {
    /// Worker is being set up on the remote (tmux session, environment).
    Provisioning,
    /// Code/data is being synced to the worker.
    Syncing,
    /// Worker is ready to accept a task.
    Ready,
    /// Worker is executing a command (with the associated command ID).
    Executing {
        /// ID of the command being executed.
        command_id: String,
    },
    /// Worker is pulling results back from the remote.
    CollectingResults,
    /// Worker has completed its task and is available for reassignment.
    Idle,
    /// Worker encountered an error.
    Error {
        /// Human-readable error description.
        message: String,
    },
    /// Worker has been shut down and removed from service.
    Decommissioned,
}

impl WorkerState {
    /// Whether this state represents an available (assignable) worker.
    pub fn is_available(&self) -> bool {
        matches!(self, WorkerState::Ready | WorkerState::Idle)
    }

    /// Whether this state is a terminal state.
    pub fn is_terminal(&self) -> bool {
        matches!(self, WorkerState::Decommissioned)
    }
}


// ---------------------------------------------------------------------------
// RemoteWorker
// ---------------------------------------------------------------------------

/// A single remote worker instance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteWorker {
    /// Unique name for this worker (e.g. "r1-worker-1").
    pub name: String,
    /// Name of the remote host this worker runs on.
    pub remote: String,
    /// Name of the agent assigned to this worker, if any.
    pub agent: Option<String>,
    /// Current lifecycle state.
    pub state: WorkerState,
    /// ID of the task currently assigned, if any.
    pub task: Option<String>,
    /// Workspace directory on the remote.
    pub workspace: String,
    /// Epoch-millisecond timestamp when the worker was created.
    pub created_ms: u64,
    /// Epoch-millisecond timestamp of the most recent activity.
    pub last_activity_ms: u64,
}


// ---------------------------------------------------------------------------
// WorkerSummary
// ---------------------------------------------------------------------------

/// Aggregate counts of workers by state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerSummary {
    pub total: usize,
    pub provisioning: usize,
    pub syncing: usize,
    pub ready: usize,
    pub executing: usize,
    pub collecting: usize,
    pub idle: usize,
    pub error: usize,
    pub decommissioned: usize,
}


// ---------------------------------------------------------------------------
// WorkerPool
// ---------------------------------------------------------------------------

/// Manages a pool of remote workers with per-remote capacity limits.
pub struct WorkerPool {
    workers: HashMap<String, RemoteWorker>,
    max_workers_per_remote: usize,
}

impl WorkerPool {
    /// Create a new pool with the given per-remote capacity limit.
    pub fn new(max_per_remote: usize) -> Self {
        WorkerPool {
            workers: HashMap::new(),
            max_workers_per_remote: max_per_remote,
        }
    }

    /// Create a new worker. Fails if the name is already taken or the
    /// per-remote limit has been reached.
    pub fn create(
        &mut self,
        name: &str,
        remote: &str,
        workspace: &str,
        now_ms: u64,
    ) -> Result<(), String> {
        if self.workers.contains_key(name) {
            return Err(format!("worker '{}' already exists", name));
        }
        if !self.can_add_to(remote) {
            return Err(format!(
                "remote '{}' already has {} workers (limit: {})",
                remote,
                self.count_for_remote(remote),
                self.max_workers_per_remote
            ));
        }
        let worker = RemoteWorker {
            name: name.to_string(),
            remote: remote.to_string(),
            agent: None,
            state: WorkerState::Provisioning,
            task: None,
            workspace: workspace.to_string(),
            created_ms: now_ms,
            last_activity_ms: now_ms,
        };
        self.workers.insert(name.to_string(), worker);
        Ok(())
    }

    /// Transition a worker to a new state. Validates the transition is
    /// reasonable (not from a terminal state, etc.).
    pub fn transition(&mut self, name: &str, state: WorkerState) -> Result<(), String> {
        let worker = self
            .workers
            .get_mut(name)
            .ok_or_else(|| format!("worker '{}' not found", name))?;

        // Cannot transition from a terminal state.
        if worker.state.is_terminal() {
            return Err(format!(
                "worker '{}' is decommissioned, cannot transition",
                name
            ));
        }

        worker.state = state;
        Ok(())
    }

    /// Assign a task to an available worker. The worker must be in Ready or
    /// Idle state.
    pub fn assign_task(&mut self, name: &str, task: &str) -> Result<(), String> {
        let worker = self
            .workers
            .get_mut(name)
            .ok_or_else(|| format!("worker '{}' not found", name))?;

        if !worker.state.is_available() {
            return Err(format!(
                "worker '{}' is not available (state: {:?})",
                name, worker.state
            ));
        }

        worker.task = Some(task.to_string());
        Ok(())
    }

    /// Complete the current task on a worker. Returns the task ID that was
    /// completed. Transitions the worker to `Idle`.
    pub fn complete_task(&mut self, name: &str) -> Result<Option<String>, String> {
        let worker = self
            .workers
            .get_mut(name)
            .ok_or_else(|| format!("worker '{}' not found", name))?;

        let task = worker.task.take();
        worker.state = WorkerState::Idle;
        Ok(task)
    }

    /// Assign an agent to a worker.
    pub fn assign_agent(&mut self, name: &str, agent: &str) -> Result<(), String> {
        let worker = self
            .workers
            .get_mut(name)
            .ok_or_else(|| format!("worker '{}' not found", name))?;
        worker.agent = Some(agent.to_string());
        Ok(())
    }

    /// Remove a worker from the pool entirely. Returns the removed worker.
    pub fn remove(&mut self, name: &str) -> Result<RemoteWorker, String> {
        self.workers
            .remove(name)
            .ok_or_else(|| format!("worker '{}' not found", name))
    }

    /// Look up a worker by name.
    pub fn get(&self, name: &str) -> Option<&RemoteWorker> {
        self.workers.get(name)
    }

    /// List all workers (no particular order).
    pub fn list(&self) -> Vec<&RemoteWorker> {
        self.workers.values().collect()
    }

    /// List workers that are available for task assignment (Ready or Idle).
    pub fn available(&self) -> Vec<&RemoteWorker> {
        self.workers
            .values()
            .filter(|w| w.state.is_available())
            .collect()
    }

    /// List all workers on a specific remote.
    pub fn by_remote(&self, remote: &str) -> Vec<&RemoteWorker> {
        self.workers
            .values()
            .filter(|w| w.remote == remote)
            .collect()
    }

    /// Whether another worker can be added to the given remote.
    pub fn can_add_to(&self, remote: &str) -> bool {
        self.count_for_remote(remote) < self.max_workers_per_remote
    }

    /// Aggregate summary of worker states.
    pub fn summary(&self) -> WorkerSummary {
        let mut s = WorkerSummary {
            total: self.workers.len(),
            provisioning: 0,
            syncing: 0,
            ready: 0,
            executing: 0,
            collecting: 0,
            idle: 0,
            error: 0,
            decommissioned: 0,
        };
        for w in self.workers.values() {
            match w.state {
                WorkerState::Provisioning => s.provisioning += 1,
                WorkerState::Syncing => s.syncing += 1,
                WorkerState::Ready => s.ready += 1,
                WorkerState::Executing { .. } => s.executing += 1,
                WorkerState::CollectingResults => s.collecting += 1,
                WorkerState::Idle => s.idle += 1,
                WorkerState::Error { .. } => s.error += 1,
                WorkerState::Decommissioned => s.decommissioned += 1,
            }
        }
        s
    }

    /// Update the `last_activity_ms` timestamp for a worker.
    pub fn touch(&mut self, name: &str, now_ms: u64) -> Result<(), String> {
        let worker = self
            .workers
            .get_mut(name)
            .ok_or_else(|| format!("worker '{}' not found", name))?;
        worker.last_activity_ms = now_ms;
        Ok(())
    }

    /// Count active (non-decommissioned) workers on a remote.
    fn count_for_remote(&self, remote: &str) -> usize {
        self.workers
            .values()
            .filter(|w| w.remote == remote && !w.state.is_terminal())
            .count()
    }
}


// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- WorkerState --

    #[test]
    fn state_available() {
        assert!(WorkerState::Ready.is_available());
        assert!(WorkerState::Idle.is_available());
        assert!(!WorkerState::Provisioning.is_available());
        assert!(!WorkerState::Syncing.is_available());
        assert!(!WorkerState::Executing {
            command_id: "x".to_string()
        }
        .is_available());
        assert!(!WorkerState::CollectingResults.is_available());
        assert!(!WorkerState::Error {
            message: "x".to_string()
        }
        .is_available());
        assert!(!WorkerState::Decommissioned.is_available());
    }

    #[test]
    fn state_terminal() {
        assert!(WorkerState::Decommissioned.is_terminal());
        assert!(!WorkerState::Ready.is_terminal());
        assert!(!WorkerState::Error {
            message: "x".to_string()
        }
        .is_terminal());
    }

    // -- WorkerPool create --

    #[test]
    fn create_worker() {
        let mut pool = WorkerPool::new(4);
        pool.create("w1", "r1", "/work", 1000).unwrap();
        let w = pool.get("w1").unwrap();
        assert_eq!(w.name, "w1");
        assert_eq!(w.remote, "r1");
        assert_eq!(w.workspace, "/work");
        assert_eq!(w.state, WorkerState::Provisioning);
        assert_eq!(w.created_ms, 1000);
        assert_eq!(w.last_activity_ms, 1000);
        assert!(w.agent.is_none());
        assert!(w.task.is_none());
    }

    #[test]
    fn create_duplicate_fails() {
        let mut pool = WorkerPool::new(4);
        pool.create("w1", "r1", "/work", 1000).unwrap();
        assert!(pool.create("w1", "r1", "/work", 2000).is_err());
    }

    #[test]
    fn create_respects_per_remote_limit() {
        let mut pool = WorkerPool::new(2);
        pool.create("w1", "r1", "/work", 1000).unwrap();
        pool.create("w2", "r1", "/work", 1000).unwrap();
        assert!(pool.create("w3", "r1", "/work", 1000).is_err());

        // Different remote should still work.
        pool.create("w3", "r2", "/work", 1000).unwrap();
    }

    #[test]
    fn decommissioned_workers_dont_count_toward_limit() {
        let mut pool = WorkerPool::new(2);
        pool.create("w1", "r1", "/work", 1000).unwrap();
        pool.create("w2", "r1", "/work", 1000).unwrap();

        // At limit.
        assert!(!pool.can_add_to("r1"));

        // Decommission one.
        pool.transition("w1", WorkerState::Decommissioned).unwrap();

        // Now we can add.
        assert!(pool.can_add_to("r1"));
        pool.create("w3", "r1", "/work", 2000).unwrap();
    }

    // -- Transition --

    #[test]
    fn transition_lifecycle() {
        let mut pool = WorkerPool::new(4);
        pool.create("w1", "r1", "/work", 1000).unwrap();

        pool.transition("w1", WorkerState::Syncing).unwrap();
        assert_eq!(pool.get("w1").unwrap().state, WorkerState::Syncing);

        pool.transition("w1", WorkerState::Ready).unwrap();
        assert_eq!(pool.get("w1").unwrap().state, WorkerState::Ready);

        pool.transition(
            "w1",
            WorkerState::Executing {
                command_id: "exec-1".to_string(),
            },
        )
        .unwrap();
        assert!(matches!(
            pool.get("w1").unwrap().state,
            WorkerState::Executing { .. }
        ));

        pool.transition("w1", WorkerState::CollectingResults)
            .unwrap();
        pool.transition("w1", WorkerState::Idle).unwrap();
        pool.transition("w1", WorkerState::Decommissioned).unwrap();
    }

    #[test]
    fn transition_from_decommissioned_fails() {
        let mut pool = WorkerPool::new(4);
        pool.create("w1", "r1", "/work", 1000).unwrap();
        pool.transition("w1", WorkerState::Decommissioned).unwrap();
        assert!(pool.transition("w1", WorkerState::Ready).is_err());
    }

    #[test]
    fn transition_unknown_worker_fails() {
        let mut pool = WorkerPool::new(4);
        assert!(pool.transition("nope", WorkerState::Ready).is_err());
    }

    // -- Task assignment --

    #[test]
    fn assign_task_ready_worker() {
        let mut pool = WorkerPool::new(4);
        pool.create("w1", "r1", "/work", 1000).unwrap();
        pool.transition("w1", WorkerState::Ready).unwrap();
        pool.assign_task("w1", "task-42").unwrap();
        assert_eq!(pool.get("w1").unwrap().task, Some("task-42".to_string()));
    }

    #[test]
    fn assign_task_idle_worker() {
        let mut pool = WorkerPool::new(4);
        pool.create("w1", "r1", "/work", 1000).unwrap();
        pool.transition("w1", WorkerState::Idle).unwrap();
        pool.assign_task("w1", "task-99").unwrap();
    }

    #[test]
    fn assign_task_provisioning_fails() {
        let mut pool = WorkerPool::new(4);
        pool.create("w1", "r1", "/work", 1000).unwrap();
        assert!(pool.assign_task("w1", "task-1").is_err());
    }

    #[test]
    fn assign_task_unknown_worker_fails() {
        let mut pool = WorkerPool::new(4);
        assert!(pool.assign_task("nope", "task-1").is_err());
    }

    // -- Complete task --

    #[test]
    fn complete_task_returns_task_id() {
        let mut pool = WorkerPool::new(4);
        pool.create("w1", "r1", "/work", 1000).unwrap();
        pool.transition("w1", WorkerState::Ready).unwrap();
        pool.assign_task("w1", "task-42").unwrap();
        pool.transition(
            "w1",
            WorkerState::Executing {
                command_id: "exec-1".to_string(),
            },
        )
        .unwrap();

        let task = pool.complete_task("w1").unwrap();
        assert_eq!(task, Some("task-42".to_string()));
        assert_eq!(pool.get("w1").unwrap().state, WorkerState::Idle);
        assert!(pool.get("w1").unwrap().task.is_none());
    }

    #[test]
    fn complete_task_no_task() {
        let mut pool = WorkerPool::new(4);
        pool.create("w1", "r1", "/work", 1000).unwrap();
        pool.transition("w1", WorkerState::Ready).unwrap();
        let task = pool.complete_task("w1").unwrap();
        assert!(task.is_none());
    }

    #[test]
    fn complete_task_unknown_fails() {
        let mut pool = WorkerPool::new(4);
        assert!(pool.complete_task("nope").is_err());
    }

    // -- Agent assignment --

    #[test]
    fn assign_agent() {
        let mut pool = WorkerPool::new(4);
        pool.create("w1", "r1", "/work", 1000).unwrap();
        pool.assign_agent("w1", "agent-pilot").unwrap();
        assert_eq!(
            pool.get("w1").unwrap().agent,
            Some("agent-pilot".to_string())
        );
    }

    #[test]
    fn assign_agent_unknown_fails() {
        let mut pool = WorkerPool::new(4);
        assert!(pool.assign_agent("nope", "agent").is_err());
    }

    // -- Remove --

    #[test]
    fn remove_worker() {
        let mut pool = WorkerPool::new(4);
        pool.create("w1", "r1", "/work", 1000).unwrap();
        let removed = pool.remove("w1").unwrap();
        assert_eq!(removed.name, "w1");
        assert!(pool.get("w1").is_none());
    }

    #[test]
    fn remove_unknown_fails() {
        let mut pool = WorkerPool::new(4);
        assert!(pool.remove("nope").is_err());
    }

    // -- List and filter --

    #[test]
    fn list_all() {
        let mut pool = WorkerPool::new(4);
        pool.create("w1", "r1", "/work", 1000).unwrap();
        pool.create("w2", "r2", "/work", 1000).unwrap();
        assert_eq!(pool.list().len(), 2);
    }

    #[test]
    fn available_workers() {
        let mut pool = WorkerPool::new(4);
        pool.create("w1", "r1", "/work", 1000).unwrap();
        pool.create("w2", "r1", "/work", 1000).unwrap();
        pool.create("w3", "r1", "/work", 1000).unwrap();

        // w1: Provisioning (not available)
        // w2: Ready
        pool.transition("w2", WorkerState::Ready).unwrap();
        // w3: Idle
        pool.transition("w3", WorkerState::Idle).unwrap();

        let avail = pool.available();
        assert_eq!(avail.len(), 2);
    }

    #[test]
    fn by_remote_filters() {
        let mut pool = WorkerPool::new(4);
        pool.create("w1", "r1", "/work", 1000).unwrap();
        pool.create("w2", "r2", "/work", 1000).unwrap();
        pool.create("w3", "r1", "/work", 1000).unwrap();

        assert_eq!(pool.by_remote("r1").len(), 2);
        assert_eq!(pool.by_remote("r2").len(), 1);
        assert_eq!(pool.by_remote("r3").len(), 0);
    }

    // -- can_add_to --

    #[test]
    fn can_add_to_under_limit() {
        let mut pool = WorkerPool::new(3);
        pool.create("w1", "r1", "/work", 1000).unwrap();
        assert!(pool.can_add_to("r1"));
    }

    #[test]
    fn can_add_to_at_limit() {
        let mut pool = WorkerPool::new(1);
        pool.create("w1", "r1", "/work", 1000).unwrap();
        assert!(!pool.can_add_to("r1"));
    }

    #[test]
    fn can_add_to_empty_remote() {
        let pool = WorkerPool::new(2);
        assert!(pool.can_add_to("r1"));
    }

    // -- Summary --

    #[test]
    fn summary_counts() {
        let mut pool = WorkerPool::new(10);
        pool.create("w1", "r1", "/work", 1000).unwrap(); // Provisioning
        pool.create("w2", "r1", "/work", 1000).unwrap();
        pool.transition("w2", WorkerState::Syncing).unwrap();
        pool.create("w3", "r1", "/work", 1000).unwrap();
        pool.transition("w3", WorkerState::Ready).unwrap();
        pool.create("w4", "r1", "/work", 1000).unwrap();
        pool.transition(
            "w4",
            WorkerState::Executing {
                command_id: "x".to_string(),
            },
        )
        .unwrap();
        pool.create("w5", "r1", "/work", 1000).unwrap();
        pool.transition("w5", WorkerState::CollectingResults)
            .unwrap();
        pool.create("w6", "r1", "/work", 1000).unwrap();
        pool.transition("w6", WorkerState::Idle).unwrap();
        pool.create("w7", "r1", "/work", 1000).unwrap();
        pool.transition(
            "w7",
            WorkerState::Error {
                message: "oops".to_string(),
            },
        )
        .unwrap();
        pool.create("w8", "r1", "/work", 1000).unwrap();
        pool.transition("w8", WorkerState::Decommissioned).unwrap();

        let s = pool.summary();
        assert_eq!(s.total, 8);
        assert_eq!(s.provisioning, 1);
        assert_eq!(s.syncing, 1);
        assert_eq!(s.ready, 1);
        assert_eq!(s.executing, 1);
        assert_eq!(s.collecting, 1);
        assert_eq!(s.idle, 1);
        assert_eq!(s.error, 1);
        assert_eq!(s.decommissioned, 1);
    }

    // -- Touch --

    #[test]
    fn touch_updates_timestamp() {
        let mut pool = WorkerPool::new(4);
        pool.create("w1", "r1", "/work", 1000).unwrap();
        assert_eq!(pool.get("w1").unwrap().last_activity_ms, 1000);
        pool.touch("w1", 5000).unwrap();
        assert_eq!(pool.get("w1").unwrap().last_activity_ms, 5000);
    }

    #[test]
    fn touch_unknown_fails() {
        let mut pool = WorkerPool::new(4);
        assert!(pool.touch("nope", 0).is_err());
    }

    // -- Full lifecycle --

    #[test]
    fn full_worker_lifecycle() {
        let mut pool = WorkerPool::new(4);

        // Create and provision.
        pool.create("w1", "r1", "/data/work", 1000).unwrap();
        assert_eq!(pool.get("w1").unwrap().state, WorkerState::Provisioning);

        // Sync code.
        pool.transition("w1", WorkerState::Syncing).unwrap();

        // Become ready.
        pool.transition("w1", WorkerState::Ready).unwrap();
        assert!(pool.available().iter().any(|w| w.name == "w1"));

        // Assign agent and task.
        pool.assign_agent("w1", "pm-agent").unwrap();
        pool.assign_task("w1", "BUILD-1").unwrap();

        // Execute.
        pool.transition(
            "w1",
            WorkerState::Executing {
                command_id: "exec-5".to_string(),
            },
        )
        .unwrap();
        assert!(pool.available().is_empty());

        // Collect results.
        pool.transition("w1", WorkerState::CollectingResults)
            .unwrap();

        // Complete task.
        let task = pool.complete_task("w1").unwrap();
        assert_eq!(task, Some("BUILD-1".to_string()));
        assert_eq!(pool.get("w1").unwrap().state, WorkerState::Idle);

        // Decommission.
        pool.transition("w1", WorkerState::Decommissioned).unwrap();
        assert!(pool.transition("w1", WorkerState::Ready).is_err());
    }
}
