use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::state::{AgentState, Transition};

/// A recorded lifecycle event: a state transition for a named agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LifecycleEvent {
    pub agent: String,
    pub from: AgentState,
    pub to: AgentState,
    pub transition: Transition,
    pub timestamp_ms: u64,
}

/// Summary counts of agents in each lifecycle state.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct LifecycleSummary {
    pub total: usize,
    pub spawning: usize,
    pub ready: usize,
    pub busy: usize,
    pub idle: usize,
    pub stalled: usize,
    pub recovering: usize,
    pub stopping: usize,
    pub dead: usize,
}

/// Manages agent lifecycle states, enforces transitions, records history,
/// and provides queries over the fleet of tracked agents.
pub struct LifecycleManager {
    states: HashMap<String, AgentState>,
    history: Vec<LifecycleEvent>,
    max_recovery_attempts: u32,
    stall_timeout_ms: u64,
}

impl LifecycleManager {
    /// Create a new manager.
    ///
    /// * `max_recovery` — maximum recovery attempts before declaring dead
    /// * `stall_timeout` — milliseconds of silence before auto-stalling
    pub fn new(max_recovery: u32, stall_timeout: u64) -> Self {
        Self {
            states: HashMap::new(),
            history: Vec::new(),
            max_recovery_attempts: max_recovery,
            stall_timeout_ms: stall_timeout,
        }
    }

    /// Register a new agent in Spawning state.
    pub fn register(&mut self, name: &str) -> Result<(), String> {
        if self.states.contains_key(name) {
            return Err(format!("agent '{}' already registered", name));
        }
        self.states.insert(name.to_string(), AgentState::Spawning);
        Ok(())
    }

    /// Apply a transition to an agent's state, recording the event.
    pub fn transition(
        &mut self,
        name: &str,
        t: Transition,
        now_ms: u64,
    ) -> Result<&AgentState, String> {
        let current = self
            .states
            .get(name)
            .ok_or_else(|| format!("agent '{}' not found", name))?
            .clone();

        let next = current.apply(t.clone())?;

        self.history.push(LifecycleEvent {
            agent: name.to_string(),
            from: current,
            to: next.clone(),
            transition: t,
            timestamp_ms: now_ms,
        });

        self.states.insert(name.to_string(), next);
        Ok(self.states.get(name).unwrap())
    }

    /// Get the current state of an agent.
    pub fn state(&self, name: &str) -> Option<&AgentState> {
        self.states.get(name)
    }

    /// Remove an agent from tracking. Returns error if agent is not registered.
    pub fn remove(&mut self, name: &str) -> Result<(), String> {
        if self.states.remove(name).is_none() {
            return Err(format!("agent '{}' not found", name));
        }
        Ok(())
    }

    /// List agents currently in a Stalled state.
    pub fn stalled_agents(&self) -> Vec<&str> {
        self.states
            .iter()
            .filter(|(_, s)| matches!(s, AgentState::Stalled { .. }))
            .map(|(name, _)| name.as_str())
            .collect()
    }

    /// List agents that can accept tasks (Ready or Idle).
    pub fn available_agents(&self) -> Vec<&str> {
        self.states
            .iter()
            .filter(|(_, s)| s.is_available())
            .map(|(name, _)| name.as_str())
            .collect()
    }

    /// List agents currently in Dead state.
    pub fn dead_agents(&self) -> Vec<&str> {
        self.states
            .iter()
            .filter(|(_, s)| s.is_terminal())
            .map(|(name, _)| name.as_str())
            .collect()
    }

    /// List agents currently Busy with a task.
    pub fn busy_agents(&self) -> Vec<&str> {
        self.states
            .iter()
            .filter(|(_, s)| matches!(s, AgentState::Busy { .. }))
            .map(|(name, _)| name.as_str())
            .collect()
    }

    /// Return lifecycle history for a specific agent.
    pub fn history_for(&self, name: &str) -> Vec<&LifecycleEvent> {
        self.history
            .iter()
            .filter(|e| e.agent == name)
            .collect()
    }

    /// Return the full event history.
    pub fn history(&self) -> &[LifecycleEvent] {
        &self.history
    }

    /// Check all Busy, Ready, and Idle agents for stalls based on last heartbeat.
    ///
    /// For each agent whose last heartbeat event is older than `stall_timeout_ms`,
    /// automatically transition them to Stalled. Returns the names of newly
    /// stalled agents.
    pub fn check_stalls(&mut self, now_ms: u64) -> Vec<String> {
        // Collect agents that need stalling: those in Busy, Ready, or Idle
        // whose last transition timestamp is older than stall_timeout_ms.
        let candidates: Vec<(String, u64)> = self
            .states
            .iter()
            .filter(|(_, s)| {
                matches!(
                    s,
                    AgentState::Busy { .. } | AgentState::Ready | AgentState::Idle
                )
            })
            .map(|(name, _)| {
                let last_event_ts = self
                    .history
                    .iter()
                    .rev()
                    .find(|e| e.agent == *name)
                    .map(|e| e.timestamp_ms)
                    .unwrap_or(0);
                (name.clone(), last_event_ts)
            })
            .filter(|(_, last_ts)| {
                now_ms.saturating_sub(*last_ts) >= self.stall_timeout_ms
            })
            .collect();

        let mut stalled = Vec::new();
        for (name, last_ts) in candidates {
            let age = now_ms.saturating_sub(last_ts);
            let transition = Transition::HeartbeatTimeout { age_ms: age };
            if self.transition(&name, transition, now_ms).is_ok() {
                stalled.push(name);
            }
        }
        stalled
    }

    /// Attempt recovery for a stalled agent. Respects max_recovery_attempts
    /// by checking the current recovery attempt count.
    pub fn attempt_recovery(&mut self, name: &str, now_ms: u64) -> Result<&AgentState, String> {
        let current = self
            .states
            .get(name)
            .ok_or_else(|| format!("agent '{}' not found", name))?;

        match current {
            AgentState::Stalled { .. } => {
                self.transition(name, Transition::RecoveryStarted, now_ms)
            }
            AgentState::Recovering { attempt } => {
                if *attempt >= self.max_recovery_attempts {
                    self.transition(
                        name,
                        Transition::RecoveryFailed {
                            message: format!(
                                "exceeded max recovery attempts ({})",
                                self.max_recovery_attempts
                            ),
                        },
                        now_ms,
                    )
                } else {
                    self.transition(name, Transition::RecoveryStarted, now_ms)
                }
            }
            other => Err(format!(
                "cannot attempt recovery from state {}",
                other.label()
            )),
        }
    }

    /// Produce a summary of agent counts by state.
    pub fn summary(&self) -> LifecycleSummary {
        let mut s = LifecycleSummary {
            total: self.states.len(),
            ..Default::default()
        };
        for state in self.states.values() {
            match state {
                AgentState::Spawning => s.spawning += 1,
                AgentState::Ready => s.ready += 1,
                AgentState::Busy { .. } => s.busy += 1,
                AgentState::Idle => s.idle += 1,
                AgentState::Stalled { .. } => s.stalled += 1,
                AgentState::Recovering { .. } => s.recovering += 1,
                AgentState::Stopping => s.stopping += 1,
                AgentState::Dead { .. } => s.dead += 1,
            }
        }
        s
    }

    /// The configured max recovery attempts.
    pub fn max_recovery_attempts(&self) -> u32 {
        self.max_recovery_attempts
    }

    /// The configured stall timeout in milliseconds.
    pub fn stall_timeout_ms(&self) -> u64 {
        self.stall_timeout_ms
    }

    /// Number of agents currently tracked.
    pub fn agent_count(&self) -> usize {
        self.states.len()
    }

    /// All tracked agent names.
    pub fn agent_names(&self) -> Vec<&str> {
        self.states.keys().map(|s| s.as_str()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_manager() -> LifecycleManager {
        LifecycleManager::new(3, 30000)
    }

    // ---- Registration ----

    #[test]
    fn register_new_agent() {
        let mut mgr = make_manager();
        assert!(mgr.register("w1").is_ok());
        assert_eq!(mgr.state("w1"), Some(&AgentState::Spawning));
    }

    #[test]
    fn register_duplicate_fails() {
        let mut mgr = make_manager();
        mgr.register("w1").unwrap();
        let result = mgr.register("w1");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("already registered"));
    }

    #[test]
    fn register_multiple_agents() {
        let mut mgr = make_manager();
        mgr.register("w1").unwrap();
        mgr.register("w2").unwrap();
        mgr.register("pm").unwrap();
        assert_eq!(mgr.agent_count(), 3);
    }

    // ---- Transition ----

    #[test]
    fn transition_spawning_to_ready() {
        let mut mgr = make_manager();
        mgr.register("w1").unwrap();
        let state = mgr.transition("w1", Transition::SpawnComplete, 1000).unwrap();
        assert_eq!(*state, AgentState::Ready);
    }

    #[test]
    fn transition_records_history() {
        let mut mgr = make_manager();
        mgr.register("w1").unwrap();
        mgr.transition("w1", Transition::SpawnComplete, 1000).unwrap();
        mgr.transition(
            "w1",
            Transition::TaskAssigned {
                task_id: "T1".into(),
            },
            2000,
        )
        .unwrap();

        let hist = mgr.history_for("w1");
        assert_eq!(hist.len(), 2);
        assert_eq!(hist[0].timestamp_ms, 1000);
        assert_eq!(hist[1].timestamp_ms, 2000);
        assert_eq!(hist[0].to, AgentState::Ready);
    }

    #[test]
    fn transition_unknown_agent_fails() {
        let mut mgr = make_manager();
        let result = mgr.transition("ghost", Transition::SpawnComplete, 1000);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not found"));
    }

    #[test]
    fn transition_invalid_fails() {
        let mut mgr = make_manager();
        mgr.register("w1").unwrap();
        let result = mgr.transition("w1", Transition::TaskCompleted, 1000);
        assert!(result.is_err());
    }

    // ---- Remove ----

    #[test]
    fn remove_agent() {
        let mut mgr = make_manager();
        mgr.register("w1").unwrap();
        assert!(mgr.remove("w1").is_ok());
        assert_eq!(mgr.state("w1"), None);
        assert_eq!(mgr.agent_count(), 0);
    }

    #[test]
    fn remove_unknown_agent_fails() {
        let mut mgr = make_manager();
        let result = mgr.remove("ghost");
        assert!(result.is_err());
    }

    #[test]
    fn remove_preserves_history() {
        let mut mgr = make_manager();
        mgr.register("w1").unwrap();
        mgr.transition("w1", Transition::SpawnComplete, 1000).unwrap();
        mgr.remove("w1").unwrap();

        // History events remain for auditing
        let hist = mgr.history_for("w1");
        assert_eq!(hist.len(), 1);
    }

    // ---- Queries ----

    #[test]
    fn stalled_agents_empty_initially() {
        let mgr = make_manager();
        assert!(mgr.stalled_agents().is_empty());
    }

    #[test]
    fn stalled_agents_returns_stalled() {
        let mut mgr = make_manager();
        mgr.register("w1").unwrap();
        mgr.transition("w1", Transition::SpawnComplete, 1000).unwrap();
        mgr.transition(
            "w1",
            Transition::HeartbeatTimeout { age_ms: 60000 },
            61000,
        )
        .unwrap();

        let stalled = mgr.stalled_agents();
        assert_eq!(stalled.len(), 1);
        assert_eq!(stalled[0], "w1");
    }

    #[test]
    fn available_agents_returns_ready_and_idle() {
        let mut mgr = make_manager();
        mgr.register("w1").unwrap();
        mgr.register("w2").unwrap();
        mgr.transition("w1", Transition::SpawnComplete, 1000).unwrap();
        mgr.transition("w2", Transition::SpawnComplete, 1000).unwrap();
        mgr.transition(
            "w2",
            Transition::TaskAssigned {
                task_id: "T1".into(),
            },
            2000,
        )
        .unwrap();
        mgr.transition("w2", Transition::TaskCompleted, 3000).unwrap();

        let mut available = mgr.available_agents();
        available.sort();
        assert_eq!(available.len(), 2);
        // w1 is Ready, w2 is Idle
    }

    #[test]
    fn dead_agents_query() {
        let mut mgr = make_manager();
        mgr.register("w1").unwrap();
        mgr.transition("w1", Transition::SpawnComplete, 1000).unwrap();
        mgr.transition("w1", Transition::Killed, 2000).unwrap();

        let dead = mgr.dead_agents();
        assert_eq!(dead.len(), 1);
        assert_eq!(dead[0], "w1");
    }

    #[test]
    fn busy_agents_query() {
        let mut mgr = make_manager();
        mgr.register("w1").unwrap();
        mgr.register("w2").unwrap();
        mgr.transition("w1", Transition::SpawnComplete, 1000).unwrap();
        mgr.transition("w2", Transition::SpawnComplete, 1000).unwrap();
        mgr.transition(
            "w1",
            Transition::TaskAssigned {
                task_id: "T1".into(),
            },
            2000,
        )
        .unwrap();

        let busy = mgr.busy_agents();
        assert_eq!(busy.len(), 1);
        assert_eq!(busy[0], "w1");
    }

    // ---- check_stalls ----

    #[test]
    fn check_stalls_detects_stale_agents() {
        let mut mgr = LifecycleManager::new(3, 10000);
        mgr.register("w1").unwrap();
        mgr.transition("w1", Transition::SpawnComplete, 1000).unwrap();
        mgr.transition(
            "w1",
            Transition::TaskAssigned {
                task_id: "T1".into(),
            },
            2000,
        )
        .unwrap();

        // At 5000ms, not stale yet (3000ms since last event, threshold 10000)
        let stalled = mgr.check_stalls(5000);
        assert!(stalled.is_empty());

        // At 15000ms, stale (13000ms since last event, threshold 10000)
        let stalled = mgr.check_stalls(15000);
        assert_eq!(stalled.len(), 1);
        assert_eq!(stalled[0], "w1");
        assert!(matches!(mgr.state("w1"), Some(AgentState::Stalled { .. })));
    }

    #[test]
    fn check_stalls_does_not_stall_already_stalled() {
        let mut mgr = LifecycleManager::new(3, 10000);
        mgr.register("w1").unwrap();
        mgr.transition("w1", Transition::SpawnComplete, 1000).unwrap();

        // Stall once
        let stalled = mgr.check_stalls(20000);
        assert_eq!(stalled.len(), 1);

        // Second check should not re-stall (agent is already Stalled, not Busy/Ready/Idle)
        let stalled = mgr.check_stalls(30000);
        assert!(stalled.is_empty());
    }

    #[test]
    fn check_stalls_ignores_spawning_and_dead() {
        let mut mgr = LifecycleManager::new(3, 5000);
        mgr.register("spawning-agent").unwrap();
        mgr.register("dead-agent").unwrap();
        mgr.transition("dead-agent", Transition::SpawnComplete, 1000)
            .unwrap();
        mgr.transition("dead-agent", Transition::Killed, 2000)
            .unwrap();

        // Neither spawning nor dead agents should be stalled
        let stalled = mgr.check_stalls(100000);
        assert!(stalled.is_empty());
    }

    // ---- attempt_recovery ----

    #[test]
    fn attempt_recovery_from_stalled() {
        let mut mgr = make_manager();
        mgr.register("w1").unwrap();
        mgr.transition("w1", Transition::SpawnComplete, 1000).unwrap();
        mgr.transition(
            "w1",
            Transition::HeartbeatTimeout { age_ms: 60000 },
            61000,
        )
        .unwrap();

        let state = mgr.attempt_recovery("w1", 62000).unwrap();
        assert_eq!(*state, AgentState::Recovering { attempt: 1 });
    }

    #[test]
    fn attempt_recovery_increments() {
        let mut mgr = make_manager();
        mgr.register("w1").unwrap();
        mgr.transition("w1", Transition::SpawnComplete, 1000).unwrap();
        mgr.transition(
            "w1",
            Transition::HeartbeatTimeout { age_ms: 60000 },
            61000,
        )
        .unwrap();
        mgr.attempt_recovery("w1", 62000).unwrap(); // attempt 1
        mgr.attempt_recovery("w1", 63000).unwrap(); // attempt 2
        let state = mgr.state("w1").unwrap();
        assert_eq!(*state, AgentState::Recovering { attempt: 2 });
    }

    #[test]
    fn attempt_recovery_exceeds_max() {
        let mut mgr = LifecycleManager::new(2, 30000);
        mgr.register("w1").unwrap();
        mgr.transition("w1", Transition::SpawnComplete, 1000).unwrap();
        mgr.transition(
            "w1",
            Transition::HeartbeatTimeout { age_ms: 60000 },
            61000,
        )
        .unwrap();
        mgr.attempt_recovery("w1", 62000).unwrap(); // attempt 1
        mgr.attempt_recovery("w1", 63000).unwrap(); // attempt 2

        // attempt 3 exceeds max of 2 -> Dead
        let state = mgr.attempt_recovery("w1", 64000).unwrap();
        assert!(state.is_terminal());
    }

    #[test]
    fn attempt_recovery_from_ready_fails() {
        let mut mgr = make_manager();
        mgr.register("w1").unwrap();
        mgr.transition("w1", Transition::SpawnComplete, 1000).unwrap();

        let result = mgr.attempt_recovery("w1", 2000);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("cannot attempt recovery"));
    }

    #[test]
    fn attempt_recovery_unknown_agent() {
        let mut mgr = make_manager();
        let result = mgr.attempt_recovery("ghost", 1000);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not found"));
    }

    // ---- Summary ----

    #[test]
    fn summary_counts_all_states() {
        let mut mgr = make_manager();
        // spawning
        mgr.register("s1").unwrap();
        // ready
        mgr.register("r1").unwrap();
        mgr.transition("r1", Transition::SpawnComplete, 100).unwrap();
        // busy
        mgr.register("b1").unwrap();
        mgr.transition("b1", Transition::SpawnComplete, 100).unwrap();
        mgr.transition(
            "b1",
            Transition::TaskAssigned {
                task_id: "T1".into(),
            },
            200,
        )
        .unwrap();
        // idle
        mgr.register("i1").unwrap();
        mgr.transition("i1", Transition::SpawnComplete, 100).unwrap();
        mgr.transition(
            "i1",
            Transition::TaskAssigned {
                task_id: "T2".into(),
            },
            200,
        )
        .unwrap();
        mgr.transition("i1", Transition::TaskCompleted, 300).unwrap();
        // stalled
        mgr.register("st1").unwrap();
        mgr.transition("st1", Transition::SpawnComplete, 100).unwrap();
        mgr.transition(
            "st1",
            Transition::HeartbeatTimeout { age_ms: 60000 },
            60100,
        )
        .unwrap();
        // recovering
        mgr.register("rec1").unwrap();
        mgr.transition("rec1", Transition::SpawnComplete, 100).unwrap();
        mgr.transition(
            "rec1",
            Transition::HeartbeatTimeout { age_ms: 60000 },
            60100,
        )
        .unwrap();
        mgr.transition("rec1", Transition::RecoveryStarted, 60200)
            .unwrap();
        // dead
        mgr.register("d1").unwrap();
        mgr.transition("d1", Transition::SpawnComplete, 100).unwrap();
        mgr.transition("d1", Transition::Killed, 200).unwrap();
        // stopping
        mgr.register("stop1").unwrap();
        mgr.transition("stop1", Transition::SpawnComplete, 100).unwrap();
        mgr.transition("stop1", Transition::StopRequested, 200).unwrap();

        let s = mgr.summary();
        assert_eq!(s.total, 8);
        assert_eq!(s.spawning, 1);
        assert_eq!(s.ready, 1);
        assert_eq!(s.busy, 1);
        assert_eq!(s.idle, 1);
        assert_eq!(s.stalled, 1);
        assert_eq!(s.recovering, 1);
        assert_eq!(s.dead, 1);
        assert_eq!(s.stopping, 1);
    }

    #[test]
    fn summary_empty() {
        let mgr = make_manager();
        let s = mgr.summary();
        assert_eq!(s.total, 0);
        assert_eq!(s.spawning, 0);
    }

    // ---- Agent names ----

    #[test]
    fn agent_names_lists_all() {
        let mut mgr = make_manager();
        mgr.register("alpha").unwrap();
        mgr.register("beta").unwrap();
        let mut names = mgr.agent_names();
        names.sort();
        assert_eq!(names, vec!["alpha", "beta"]);
    }

    // ---- History ----

    #[test]
    fn history_returns_all_events() {
        let mut mgr = make_manager();
        mgr.register("w1").unwrap();
        mgr.register("w2").unwrap();
        mgr.transition("w1", Transition::SpawnComplete, 1000).unwrap();
        mgr.transition("w2", Transition::SpawnComplete, 1000).unwrap();
        mgr.transition(
            "w1",
            Transition::TaskAssigned {
                task_id: "T1".into(),
            },
            2000,
        )
        .unwrap();

        assert_eq!(mgr.history().len(), 3);
    }

    #[test]
    fn history_for_filters_by_agent() {
        let mut mgr = make_manager();
        mgr.register("w1").unwrap();
        mgr.register("w2").unwrap();
        mgr.transition("w1", Transition::SpawnComplete, 1000).unwrap();
        mgr.transition("w2", Transition::SpawnComplete, 1000).unwrap();
        mgr.transition(
            "w1",
            Transition::TaskAssigned {
                task_id: "T1".into(),
            },
            2000,
        )
        .unwrap();

        assert_eq!(mgr.history_for("w1").len(), 2);
        assert_eq!(mgr.history_for("w2").len(), 1);
        assert_eq!(mgr.history_for("ghost").len(), 0);
    }

    // ---- Full workflow test ----

    #[test]
    fn full_fleet_lifecycle() {
        let mut mgr = LifecycleManager::new(2, 10000);

        // Register three workers
        mgr.register("w1").unwrap();
        mgr.register("w2").unwrap();
        mgr.register("w3").unwrap();

        // All spawn successfully
        for name in &["w1", "w2", "w3"] {
            mgr.transition(name, Transition::SpawnComplete, 1000).unwrap();
        }
        assert_eq!(mgr.available_agents().len(), 3);

        // Assign tasks to w1 and w2
        mgr.transition(
            "w1",
            Transition::TaskAssigned {
                task_id: "T1".into(),
            },
            2000,
        )
        .unwrap();
        mgr.transition(
            "w2",
            Transition::TaskAssigned {
                task_id: "T2".into(),
            },
            2000,
        )
        .unwrap();
        assert_eq!(mgr.busy_agents().len(), 2);
        assert_eq!(mgr.available_agents().len(), 1);

        // w1 completes its task
        mgr.transition("w1", Transition::TaskCompleted, 5000).unwrap();
        assert_eq!(mgr.available_agents().len(), 2);

        // w2 stalls
        let stalled = mgr.check_stalls(15000);
        // w3 was Ready since 1000ms, that's 14000ms ago > 10000ms threshold
        // w2 was assigned at 2000ms, that's 13000ms ago > 10000ms threshold
        // w1 completed at 5000ms, that's 10000ms ago = 10000ms threshold (should stall)
        assert!(stalled.len() >= 1);

        let s = mgr.summary();
        assert!(s.stalled >= 1);
    }

    // ---- Config getters ----

    #[test]
    fn config_getters() {
        let mgr = LifecycleManager::new(5, 45000);
        assert_eq!(mgr.max_recovery_attempts(), 5);
        assert_eq!(mgr.stall_timeout_ms(), 45000);
    }

    // ---- Lifecycle event serialization ----

    #[test]
    fn lifecycle_event_serde() {
        let event = LifecycleEvent {
            agent: "w1".into(),
            from: AgentState::Spawning,
            to: AgentState::Ready,
            transition: Transition::SpawnComplete,
            timestamp_ms: 1000,
        };
        let json = serde_json::to_string(&event).unwrap();
        let back: LifecycleEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(back.agent, "w1");
        assert_eq!(back.from, AgentState::Spawning);
        assert_eq!(back.to, AgentState::Ready);
        assert_eq!(back.timestamp_ms, 1000);
    }

    #[test]
    fn lifecycle_summary_serde() {
        let summary = LifecycleSummary {
            total: 5,
            spawning: 1,
            ready: 1,
            busy: 1,
            idle: 1,
            stalled: 0,
            recovering: 0,
            stopping: 0,
            dead: 1,
        };
        let json = serde_json::to_string(&summary).unwrap();
        let back: LifecycleSummary = serde_json::from_str(&json).unwrap();
        assert_eq!(back, summary);
    }
}
