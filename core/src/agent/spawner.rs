use std::collections::{HashMap, VecDeque};

use serde::{Deserialize, Serialize};

/// A request to spawn a new agent.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SpawnRequest {
    pub name: String,
    pub role: String,
    pub agent_type: String,
    pub path: String,
    pub env: Vec<(String, String)>,
}

/// The result of a spawn attempt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpawnResult {
    pub name: String,
    pub success: bool,
    pub session: Option<String>,
    pub error: Option<String>,
    pub duration_ms: u64,
}

/// Validation errors for spawn requests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpawnValidationError {
    EmptyName,
    EmptyRole,
    EmptyAgentType,
    EmptyPath,
    DuplicateName(String),
}

impl std::fmt::Display for SpawnValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SpawnValidationError::EmptyName => write!(f, "agent name cannot be empty"),
            SpawnValidationError::EmptyRole => write!(f, "agent role cannot be empty"),
            SpawnValidationError::EmptyAgentType => write!(f, "agent type cannot be empty"),
            SpawnValidationError::EmptyPath => write!(f, "agent path cannot be empty"),
            SpawnValidationError::DuplicateName(n) => {
                write!(f, "agent '{}' already in queue", n)
            }
        }
    }
}

impl SpawnRequest {
    /// Validate that required fields are non-empty.
    pub fn validate(&self) -> Result<(), SpawnValidationError> {
        if self.name.trim().is_empty() {
            return Err(SpawnValidationError::EmptyName);
        }
        if self.role.trim().is_empty() {
            return Err(SpawnValidationError::EmptyRole);
        }
        if self.agent_type.trim().is_empty() {
            return Err(SpawnValidationError::EmptyAgentType);
        }
        if self.path.trim().is_empty() {
            return Err(SpawnValidationError::EmptyPath);
        }
        Ok(())
    }
}

/// Manages a queue of agent spawn requests with concurrency limits.
///
/// Spawn requests are enqueued, then started one at a time up to
/// `max_concurrent`. Completed results are collected for inspection.
pub struct SpawnQueue {
    pending: VecDeque<SpawnRequest>,
    in_progress: HashMap<String, SpawnRequest>,
    completed: Vec<SpawnResult>,
    max_concurrent: usize,
}

impl SpawnQueue {
    /// Create a new spawn queue with the given concurrency limit.
    pub fn new(max_concurrent: usize) -> Self {
        Self {
            pending: VecDeque::new(),
            in_progress: HashMap::new(),
            completed: Vec::new(),
            max_concurrent: max_concurrent.max(1),
        }
    }

    /// Enqueue a spawn request. Validates the request and checks for
    /// duplicate names across pending and in-progress.
    pub fn enqueue(&mut self, req: SpawnRequest) -> Result<(), String> {
        req.validate().map_err(|e| e.to_string())?;

        if self.has_name(&req.name) {
            return Err(format!(
                "agent '{}' already in queue or in progress",
                req.name
            ));
        }

        self.pending.push_back(req);
        Ok(())
    }

    /// Start the next pending request if concurrency allows.
    /// Returns a reference to the request that was started, or None if
    /// nothing can start (no pending requests or at capacity).
    pub fn start_next(&mut self) -> Option<&SpawnRequest> {
        if !self.can_start() {
            return None;
        }
        if let Some(req) = self.pending.pop_front() {
            let name = req.name.clone();
            self.in_progress.insert(name.clone(), req);
            self.in_progress.get(&name)
        } else {
            None
        }
    }

    /// Record a spawn result, removing the agent from in_progress.
    pub fn complete(&mut self, result: SpawnResult) -> Result<(), String> {
        if self.in_progress.remove(&result.name).is_none() {
            return Err(format!(
                "agent '{}' not found in progress",
                result.name
            ));
        }
        self.completed.push(result);
        Ok(())
    }

    /// Number of pending requests.
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    /// Number of currently spawning agents.
    pub fn in_progress_count(&self) -> usize {
        self.in_progress.len()
    }

    /// Number of completed spawn results.
    pub fn completed_count(&self) -> usize {
        self.completed.len()
    }

    /// Whether we can start another spawn (not at concurrency limit and
    /// there are pending requests).
    pub fn can_start(&self) -> bool {
        self.in_progress.len() < self.max_concurrent && !self.pending.is_empty()
    }

    /// Drain all completed results, returning them.
    pub fn drain_completed(&mut self) -> Vec<SpawnResult> {
        std::mem::take(&mut self.completed)
    }

    /// Peek at the next pending request without removing it.
    pub fn peek_next(&self) -> Option<&SpawnRequest> {
        self.pending.front()
    }

    /// Get a reference to an in-progress request by name.
    pub fn in_progress_request(&self, name: &str) -> Option<&SpawnRequest> {
        self.in_progress.get(name)
    }

    /// Cancel a pending request by name. Returns true if found and removed.
    pub fn cancel_pending(&mut self, name: &str) -> bool {
        if let Some(pos) = self.pending.iter().position(|r| r.name == name) {
            self.pending.remove(pos);
            true
        } else {
            false
        }
    }

    /// List all names across pending and in-progress.
    pub fn all_active_names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self.pending.iter().map(|r| r.name.as_str()).collect();
        names.extend(self.in_progress.keys().map(|s| s.as_str()));
        names
    }

    /// The max concurrency setting.
    pub fn max_concurrent(&self) -> usize {
        self.max_concurrent
    }

    /// Check if a name exists in pending or in_progress.
    fn has_name(&self, name: &str) -> bool {
        self.pending.iter().any(|r| r.name == name)
            || self.in_progress.contains_key(name)
    }
}

/// Batch-build spawn requests for a fleet of agents with the same settings.
pub struct SpawnPlan {
    pub requests: Vec<SpawnRequest>,
}

impl SpawnPlan {
    /// Create a plan with a set of worker agents.
    ///
    /// * `prefix` — name prefix (e.g., "worker"), agents named "{prefix}-1", "{prefix}-2", ...
    /// * `count` — number of agents
    /// * `role` — role string
    /// * `agent_type` — agent type string
    /// * `path` — working directory
    /// * `env` — shared environment variables
    pub fn workers(
        prefix: &str,
        count: usize,
        role: &str,
        agent_type: &str,
        path: &str,
        env: Vec<(String, String)>,
    ) -> Self {
        let requests = (1..=count)
            .map(|i| SpawnRequest {
                name: format!("{}-{}", prefix, i),
                role: role.to_string(),
                agent_type: agent_type.to_string(),
                path: path.to_string(),
                env: env.clone(),
            })
            .collect();
        Self { requests }
    }

    /// Enqueue all requests in this plan into a spawn queue.
    pub fn enqueue_all(&self, queue: &mut SpawnQueue) -> Result<usize, String> {
        let mut count = 0;
        for req in &self.requests {
            queue.enqueue(req.clone())?;
            count += 1;
        }
        Ok(count)
    }

    /// Number of agents in this plan.
    pub fn len(&self) -> usize {
        self.requests.len()
    }

    /// Whether the plan is empty.
    pub fn is_empty(&self) -> bool {
        self.requests.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_request(name: &str) -> SpawnRequest {
        SpawnRequest {
            name: name.into(),
            role: "worker".into(),
            agent_type: "claude".into(),
            path: "/tmp/work".into(),
            env: vec![("KEY".into(), "VAL".into())],
        }
    }

    // ---- SpawnRequest validation ----

    #[test]
    fn valid_request() {
        let req = make_request("w1");
        assert!(req.validate().is_ok());
    }

    #[test]
    fn empty_name_invalid() {
        let req = SpawnRequest {
            name: "".into(),
            ..make_request("x")
        };
        assert_eq!(req.validate().unwrap_err(), SpawnValidationError::EmptyName);
    }

    #[test]
    fn whitespace_name_invalid() {
        let req = SpawnRequest {
            name: "   ".into(),
            ..make_request("x")
        };
        assert_eq!(req.validate().unwrap_err(), SpawnValidationError::EmptyName);
    }

    #[test]
    fn empty_role_invalid() {
        let req = SpawnRequest {
            role: "".into(),
            ..make_request("w1")
        };
        assert_eq!(req.validate().unwrap_err(), SpawnValidationError::EmptyRole);
    }

    #[test]
    fn empty_agent_type_invalid() {
        let req = SpawnRequest {
            agent_type: "".into(),
            ..make_request("w1")
        };
        assert_eq!(
            req.validate().unwrap_err(),
            SpawnValidationError::EmptyAgentType
        );
    }

    #[test]
    fn empty_path_invalid() {
        let req = SpawnRequest {
            path: "".into(),
            ..make_request("w1")
        };
        assert_eq!(req.validate().unwrap_err(), SpawnValidationError::EmptyPath);
    }

    // ---- SpawnRequest serde ----

    #[test]
    fn request_serde_round_trip() {
        let req = make_request("w1");
        let json = serde_json::to_string(&req).unwrap();
        let back: SpawnRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(back, req);
    }

    #[test]
    fn result_serde_round_trip() {
        let result = SpawnResult {
            name: "w1".into(),
            success: true,
            session: Some("cmx-main".into()),
            error: None,
            duration_ms: 250,
        };
        let json = serde_json::to_string(&result).unwrap();
        let back: SpawnResult = serde_json::from_str(&json).unwrap();
        assert_eq!(back.name, "w1");
        assert!(back.success);
        assert_eq!(back.session, Some("cmx-main".into()));
    }

    #[test]
    fn result_serde_failure() {
        let result = SpawnResult {
            name: "w2".into(),
            success: false,
            session: None,
            error: Some("tmux not found".into()),
            duration_ms: 50,
        };
        let json = serde_json::to_string(&result).unwrap();
        let back: SpawnResult = serde_json::from_str(&json).unwrap();
        assert!(!back.success);
        assert_eq!(back.error, Some("tmux not found".into()));
    }

    // ---- SpawnQueue basic operations ----

    #[test]
    fn new_queue_is_empty() {
        let q = SpawnQueue::new(2);
        assert_eq!(q.pending_count(), 0);
        assert_eq!(q.in_progress_count(), 0);
        assert_eq!(q.completed_count(), 0);
        assert!(!q.can_start());
    }

    #[test]
    fn max_concurrent_minimum_is_one() {
        let q = SpawnQueue::new(0);
        assert_eq!(q.max_concurrent(), 1);
    }

    #[test]
    fn enqueue_adds_to_pending() {
        let mut q = SpawnQueue::new(2);
        q.enqueue(make_request("w1")).unwrap();
        assert_eq!(q.pending_count(), 1);
        assert!(q.can_start());
    }

    #[test]
    fn enqueue_duplicate_name_fails() {
        let mut q = SpawnQueue::new(2);
        q.enqueue(make_request("w1")).unwrap();
        let result = q.enqueue(make_request("w1"));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("already in queue"));
    }

    #[test]
    fn enqueue_invalid_request_fails() {
        let mut q = SpawnQueue::new(2);
        let req = SpawnRequest {
            name: "".into(),
            ..make_request("x")
        };
        let result = q.enqueue(req);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("empty"));
    }

    // ---- start_next ----

    #[test]
    fn start_next_moves_to_in_progress() {
        let mut q = SpawnQueue::new(2);
        q.enqueue(make_request("w1")).unwrap();
        let started = q.start_next();
        assert!(started.is_some());
        assert_eq!(started.unwrap().name, "w1");
        assert_eq!(q.pending_count(), 0);
        assert_eq!(q.in_progress_count(), 1);
    }

    #[test]
    fn start_next_respects_concurrency() {
        let mut q = SpawnQueue::new(1);
        q.enqueue(make_request("w1")).unwrap();
        q.enqueue(make_request("w2")).unwrap();

        q.start_next(); // w1 starts
        assert_eq!(q.in_progress_count(), 1);
        assert!(!q.can_start()); // at capacity

        let next = q.start_next();
        assert!(next.is_none()); // blocked by concurrency
    }

    #[test]
    fn start_next_empty_queue() {
        let mut q = SpawnQueue::new(2);
        let next = q.start_next();
        assert!(next.is_none());
    }

    #[test]
    fn start_next_fifo_order() {
        let mut q = SpawnQueue::new(3);
        q.enqueue(make_request("w1")).unwrap();
        q.enqueue(make_request("w2")).unwrap();
        q.enqueue(make_request("w3")).unwrap();

        assert_eq!(q.start_next().unwrap().name, "w1");
        assert_eq!(q.start_next().unwrap().name, "w2");
        assert_eq!(q.start_next().unwrap().name, "w3");
    }

    // ---- complete ----

    #[test]
    fn complete_moves_to_completed() {
        let mut q = SpawnQueue::new(2);
        q.enqueue(make_request("w1")).unwrap();
        q.start_next();

        let result = SpawnResult {
            name: "w1".into(),
            success: true,
            session: Some("sess".into()),
            error: None,
            duration_ms: 100,
        };
        q.complete(result).unwrap();
        assert_eq!(q.in_progress_count(), 0);
        assert_eq!(q.completed_count(), 1);
    }

    #[test]
    fn complete_unknown_agent_fails() {
        let mut q = SpawnQueue::new(2);
        let result = SpawnResult {
            name: "ghost".into(),
            success: false,
            session: None,
            error: Some("nope".into()),
            duration_ms: 0,
        };
        let err = q.complete(result);
        assert!(err.is_err());
        assert!(err.unwrap_err().contains("not found in progress"));
    }

    #[test]
    fn complete_frees_concurrency_slot() {
        let mut q = SpawnQueue::new(1);
        q.enqueue(make_request("w1")).unwrap();
        q.enqueue(make_request("w2")).unwrap();

        q.start_next(); // w1 starts
        assert!(!q.can_start());

        q.complete(SpawnResult {
            name: "w1".into(),
            success: true,
            session: None,
            error: None,
            duration_ms: 100,
        })
        .unwrap();

        assert!(q.can_start()); // slot freed
        let next = q.start_next();
        assert_eq!(next.unwrap().name, "w2");
    }

    // ---- drain_completed ----

    #[test]
    fn drain_completed_returns_and_clears() {
        let mut q = SpawnQueue::new(2);
        q.enqueue(make_request("w1")).unwrap();
        q.enqueue(make_request("w2")).unwrap();
        q.start_next();
        q.start_next();

        q.complete(SpawnResult {
            name: "w1".into(),
            success: true,
            session: None,
            error: None,
            duration_ms: 100,
        })
        .unwrap();
        q.complete(SpawnResult {
            name: "w2".into(),
            success: false,
            session: None,
            error: Some("fail".into()),
            duration_ms: 50,
        })
        .unwrap();

        let results = q.drain_completed();
        assert_eq!(results.len(), 2);
        assert_eq!(q.completed_count(), 0);
    }

    #[test]
    fn drain_completed_empty() {
        let mut q = SpawnQueue::new(2);
        let results = q.drain_completed();
        assert!(results.is_empty());
    }

    // ---- Peek and cancel ----

    #[test]
    fn peek_next_without_removing() {
        let mut q = SpawnQueue::new(2);
        q.enqueue(make_request("w1")).unwrap();
        q.enqueue(make_request("w2")).unwrap();

        assert_eq!(q.peek_next().unwrap().name, "w1");
        assert_eq!(q.pending_count(), 2); // still there
    }

    #[test]
    fn cancel_pending_removes_request() {
        let mut q = SpawnQueue::new(2);
        q.enqueue(make_request("w1")).unwrap();
        q.enqueue(make_request("w2")).unwrap();
        q.enqueue(make_request("w3")).unwrap();

        assert!(q.cancel_pending("w2"));
        assert_eq!(q.pending_count(), 2);
        // w2 is gone; w1 and w3 remain
        assert_eq!(q.start_next().unwrap().name, "w1");
        assert_eq!(q.start_next().unwrap().name, "w3");
    }

    #[test]
    fn cancel_pending_nonexistent() {
        let mut q = SpawnQueue::new(2);
        assert!(!q.cancel_pending("ghost"));
    }

    #[test]
    fn cancel_pending_does_not_affect_in_progress() {
        let mut q = SpawnQueue::new(2);
        q.enqueue(make_request("w1")).unwrap();
        q.start_next(); // moves to in_progress
        assert!(!q.cancel_pending("w1")); // not in pending anymore
    }

    // ---- in_progress_request ----

    #[test]
    fn in_progress_request_found() {
        let mut q = SpawnQueue::new(2);
        q.enqueue(make_request("w1")).unwrap();
        q.start_next();
        assert!(q.in_progress_request("w1").is_some());
        assert_eq!(q.in_progress_request("w1").unwrap().role, "worker");
    }

    #[test]
    fn in_progress_request_not_found() {
        let q = SpawnQueue::new(2);
        assert!(q.in_progress_request("ghost").is_none());
    }

    // ---- all_active_names ----

    #[test]
    fn all_active_names_includes_pending_and_in_progress() {
        let mut q = SpawnQueue::new(2);
        q.enqueue(make_request("w1")).unwrap();
        q.enqueue(make_request("w2")).unwrap();
        q.enqueue(make_request("w3")).unwrap();
        q.start_next(); // w1 -> in_progress

        let mut names = q.all_active_names();
        names.sort();
        assert_eq!(names, vec!["w1", "w2", "w3"]);
    }

    // ---- Duplicate detection across pending + in_progress ----

    #[test]
    fn enqueue_duplicate_in_progress_fails() {
        let mut q = SpawnQueue::new(2);
        q.enqueue(make_request("w1")).unwrap();
        q.start_next(); // w1 in progress

        let result = q.enqueue(make_request("w1"));
        assert!(result.is_err());
    }

    // ---- SpawnPlan ----

    #[test]
    fn spawn_plan_workers() {
        let plan = SpawnPlan::workers(
            "worker",
            3,
            "worker",
            "claude",
            "/tmp/work",
            vec![("ENV".into(), "prod".into())],
        );
        assert_eq!(plan.len(), 3);
        assert!(!plan.is_empty());
        assert_eq!(plan.requests[0].name, "worker-1");
        assert_eq!(plan.requests[1].name, "worker-2");
        assert_eq!(plan.requests[2].name, "worker-3");
        assert_eq!(plan.requests[0].role, "worker");
        assert_eq!(plan.requests[0].agent_type, "claude");
        assert_eq!(plan.requests[0].path, "/tmp/work");
        assert_eq!(plan.requests[0].env, vec![("ENV".into(), "prod".into())]);
    }

    #[test]
    fn spawn_plan_empty() {
        let plan = SpawnPlan::workers("w", 0, "worker", "claude", "/tmp", vec![]);
        assert!(plan.is_empty());
        assert_eq!(plan.len(), 0);
    }

    #[test]
    fn spawn_plan_enqueue_all() {
        let plan = SpawnPlan::workers("w", 3, "worker", "claude", "/tmp/work", vec![]);
        let mut q = SpawnQueue::new(2);
        let count = plan.enqueue_all(&mut q).unwrap();
        assert_eq!(count, 3);
        assert_eq!(q.pending_count(), 3);
    }

    #[test]
    fn spawn_plan_enqueue_all_detects_duplicates() {
        let plan = SpawnPlan::workers("w", 2, "worker", "claude", "/tmp", vec![]);
        let mut q = SpawnQueue::new(2);
        q.enqueue(make_request("w-1")).unwrap(); // conflict with plan's "w-1"
        let result = plan.enqueue_all(&mut q);
        assert!(result.is_err());
    }

    // ---- Full workflow ----

    #[test]
    fn full_spawn_queue_workflow() {
        let mut q = SpawnQueue::new(2);

        // Enqueue 4 workers
        for i in 1..=4 {
            q.enqueue(make_request(&format!("w{}", i))).unwrap();
        }
        assert_eq!(q.pending_count(), 4);

        // Start first batch (up to 2 concurrent)
        let r1 = q.start_next().unwrap().name.to_string();
        let r2 = q.start_next().unwrap().name.to_string();
        assert_eq!(r1, "w1");
        assert_eq!(r2, "w2");
        assert_eq!(q.in_progress_count(), 2);
        assert!(!q.can_start());

        // w1 succeeds
        q.complete(SpawnResult {
            name: "w1".into(),
            success: true,
            session: Some("s1".into()),
            error: None,
            duration_ms: 200,
        })
        .unwrap();

        // Now we can start w3
        assert!(q.can_start());
        let r3 = q.start_next().unwrap().name.to_string();
        assert_eq!(r3, "w3");

        // w2 fails
        q.complete(SpawnResult {
            name: "w2".into(),
            success: false,
            session: None,
            error: Some("timeout".into()),
            duration_ms: 5000,
        })
        .unwrap();

        // Start w4
        let r4 = q.start_next().unwrap().name.to_string();
        assert_eq!(r4, "w4");

        // Complete remaining
        q.complete(SpawnResult {
            name: "w3".into(),
            success: true,
            session: Some("s3".into()),
            error: None,
            duration_ms: 150,
        })
        .unwrap();
        q.complete(SpawnResult {
            name: "w4".into(),
            success: true,
            session: Some("s4".into()),
            error: None,
            duration_ms: 180,
        })
        .unwrap();

        // Drain and verify
        let results = q.drain_completed();
        assert_eq!(results.len(), 4);
        let successes: Vec<_> = results.iter().filter(|r| r.success).collect();
        let failures: Vec<_> = results.iter().filter(|r| !r.success).collect();
        assert_eq!(successes.len(), 3);
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].name, "w2");
    }
}
