//! Task scheduling — priority queues, scheduling policies, and metrics.
//!
//! Provides a `Scheduler` that orders pending executions according to
//! configurable policies (FIFO, priority, round-robin, affinity) and
//! tracks scheduling metrics.

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// SchedulePolicy
// ---------------------------------------------------------------------------

/// Policy that determines how entries are ordered in the queue.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "policy", rename_all = "snake_case")]
pub enum SchedulePolicy {
    Fifo,
    Priority,
    RoundRobin { agents: Vec<String> },
    Affinity { preferred_agent: String },
}

// ---------------------------------------------------------------------------
// ScheduleEntry
// ---------------------------------------------------------------------------

/// An entry in the scheduling queue.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduleEntry {
    pub execution_id: String,
    pub task_id: String,
    pub priority: u32,
    pub submitted_ms: u64,
    pub agent_affinity: Option<String>,
    pub estimated_duration_ms: Option<u64>,
}

// ---------------------------------------------------------------------------
// ScheduleMetrics
// ---------------------------------------------------------------------------

/// Metrics about scheduling performance.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ScheduleMetrics {
    pub avg_wait_time_ms: u64,
    pub max_wait_time_ms: u64,
    pub throughput: f64,
    pub utilization_percent: f64,
}

// ---------------------------------------------------------------------------
// Scheduler
// ---------------------------------------------------------------------------

/// A scheduling queue that orders entries according to a policy.
#[derive(Debug)]
pub struct Scheduler {
    entries: Vec<ScheduleEntry>,
    policy: SchedulePolicy,
    /// Tracks dequeue timestamps for metrics.
    dequeue_log: Vec<DequeuedRecord>,
    /// Round-robin state: index of next agent.
    rr_index: usize,
}

/// Internal record of a dequeued entry for metrics.
#[derive(Debug, Clone)]
struct DequeuedRecord {
    submitted_ms: u64,
    dequeued_ms: u64,
}

impl Scheduler {
    /// Create a new scheduler with the given policy.
    pub fn new(policy: SchedulePolicy) -> Self {
        Scheduler {
            entries: Vec::new(),
            policy,
            dequeue_log: Vec::new(),
            rr_index: 0,
        }
    }

    /// Add an entry to the queue.
    pub fn enqueue(&mut self, entry: ScheduleEntry) {
        self.entries.push(entry);
        self.sort_entries();
    }

    /// Remove and return the next entry according to the scheduling policy.
    pub fn dequeue(&mut self, now_ms: u64) -> Option<ScheduleEntry> {
        if self.entries.is_empty() {
            return None;
        }

        let index = self.pick_index(None);
        let entry = self.entries.remove(index);

        self.dequeue_log.push(DequeuedRecord {
            submitted_ms: entry.submitted_ms,
            dequeued_ms: now_ms,
        });

        Some(entry)
    }

    /// Remove and return the next entry appropriate for the given agent.
    pub fn dequeue_for_agent(&mut self, agent: &str, now_ms: u64) -> Option<ScheduleEntry> {
        if self.entries.is_empty() {
            return None;
        }

        // Find the best entry for this agent.
        let index = self.pick_index_for_agent(agent)?;
        let entry = self.entries.remove(index);

        self.dequeue_log.push(DequeuedRecord {
            submitted_ms: entry.submitted_ms,
            dequeued_ms: now_ms,
        });

        Some(entry)
    }

    /// Peek at the next entry without removing it.
    pub fn peek(&self) -> Option<&ScheduleEntry> {
        if self.entries.is_empty() {
            return None;
        }
        let index = self.peek_index();
        Some(&self.entries[index])
    }

    /// Change the scheduling policy and re-sort.
    pub fn reorder(&mut self, policy: SchedulePolicy) {
        self.policy = policy;
        self.rr_index = 0;
        self.sort_entries();
    }

    /// Remove an entry by execution ID.
    pub fn remove(&mut self, id: &str) -> bool {
        let len_before = self.entries.len();
        self.entries.retain(|e| e.execution_id != id);
        self.entries.len() < len_before
    }

    /// Number of entries in the queue.
    pub fn size(&self) -> usize {
        self.entries.len()
    }

    /// Whether the queue is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Compute scheduling metrics from dequeue history.
    ///
    /// `window_ms` defines the time window for throughput calculation.
    /// `total_agents` is used for utilization approximation.
    pub fn metrics(&self, now_ms: u64, window_ms: u64, total_agents: usize) -> ScheduleMetrics {
        let mut total_wait: u64 = 0;
        let mut max_wait: u64 = 0;

        for record in &self.dequeue_log {
            let wait = record.dequeued_ms.saturating_sub(record.submitted_ms);
            total_wait += wait;
            if wait > max_wait {
                max_wait = wait;
            }
        }

        let avg_wait = if self.dequeue_log.is_empty() {
            0
        } else {
            total_wait / self.dequeue_log.len() as u64
        };

        // Throughput: completions in the recent window.
        let window_start = now_ms.saturating_sub(window_ms);
        let recent_count = self
            .dequeue_log
            .iter()
            .filter(|r| r.dequeued_ms >= window_start)
            .count();

        let throughput = if window_ms > 0 {
            (recent_count as f64) / (window_ms as f64 / 1000.0)
        } else {
            0.0
        };

        // Utilization: rough estimate based on queue depth vs agent count.
        let utilization = if total_agents > 0 {
            let pending_ratio = self.entries.len() as f64 / total_agents as f64;
            (pending_ratio * 100.0).min(100.0)
        } else {
            0.0
        };

        ScheduleMetrics {
            avg_wait_time_ms: avg_wait,
            max_wait_time_ms: max_wait,
            throughput,
            utilization_percent: utilization,
        }
    }

    // -----------------------------------------------------------------------
    // Internal sorting / picking
    // -----------------------------------------------------------------------

    /// Sort entries according to the current policy.
    fn sort_entries(&mut self) {
        match &self.policy {
            SchedulePolicy::Fifo => {
                self.entries.sort_by_key(|e| e.submitted_ms);
            }
            SchedulePolicy::Priority => {
                // Higher priority first, then earlier submission.
                self.entries
                    .sort_by(|a, b| b.priority.cmp(&a.priority).then(a.submitted_ms.cmp(&b.submitted_ms)));
            }
            SchedulePolicy::RoundRobin { .. } => {
                // Round-robin doesn't re-sort; pick_index handles ordering.
                self.entries.sort_by_key(|e| e.submitted_ms);
            }
            SchedulePolicy::Affinity { preferred_agent } => {
                // Entries with matching affinity come first, then by priority.
                let pref = preferred_agent.clone();
                self.entries.sort_by(|a, b| {
                    let a_match = a.agent_affinity.as_deref() == Some(&pref);
                    let b_match = b.agent_affinity.as_deref() == Some(&pref);
                    b_match
                        .cmp(&a_match)
                        .then(b.priority.cmp(&a.priority))
                        .then(a.submitted_ms.cmp(&b.submitted_ms))
                });
            }
        }
    }

    /// Pick the index of the next entry to dequeue.
    fn pick_index(&mut self, _agent: Option<&str>) -> usize {
        match &self.policy {
            SchedulePolicy::RoundRobin { agents } if !agents.is_empty() => {
                let target_agent = &agents[self.rr_index % agents.len()];
                self.rr_index += 1;

                // Find an entry with affinity for the target agent.
                self.entries
                    .iter()
                    .position(|e| e.agent_affinity.as_deref() == Some(target_agent.as_str()))
                    .unwrap_or(0)
            }
            _ => 0, // Already sorted; take the first.
        }
    }

    /// Peek index (non-mutating version).
    fn peek_index(&self) -> usize {
        match &self.policy {
            SchedulePolicy::RoundRobin { agents } if !agents.is_empty() => {
                let target_agent = &agents[self.rr_index % agents.len()];
                self.entries
                    .iter()
                    .position(|e| e.agent_affinity.as_deref() == Some(target_agent.as_str()))
                    .unwrap_or(0)
            }
            _ => 0,
        }
    }

    /// Pick the best entry index for a specific agent.
    fn pick_index_for_agent(&self, agent: &str) -> Option<usize> {
        // First try to find an entry with affinity for this agent.
        let affinity_match = self
            .entries
            .iter()
            .position(|e| e.agent_affinity.as_deref() == Some(agent));

        if affinity_match.is_some() {
            return affinity_match;
        }

        // Fall back to entries with no affinity.
        let no_affinity = self
            .entries
            .iter()
            .position(|e| e.agent_affinity.is_none());

        if no_affinity.is_some() {
            return no_affinity;
        }

        // Last resort: take the first entry regardless of affinity.
        if !self.entries.is_empty() {
            Some(0)
        } else {
            None
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_entry(id: &str, priority: u32, submitted_ms: u64) -> ScheduleEntry {
        ScheduleEntry {
            execution_id: id.into(),
            task_id: format!("T-{}", id),
            priority,
            submitted_ms,
            agent_affinity: None,
            estimated_duration_ms: None,
        }
    }

    fn make_entry_with_affinity(
        id: &str,
        priority: u32,
        submitted_ms: u64,
        agent: &str,
    ) -> ScheduleEntry {
        ScheduleEntry {
            execution_id: id.into(),
            task_id: format!("T-{}", id),
            priority,
            submitted_ms,
            agent_affinity: Some(agent.into()),
            estimated_duration_ms: None,
        }
    }

    // -- FIFO tests --

    #[test]
    fn fifo_ordering() {
        let mut s = Scheduler::new(SchedulePolicy::Fifo);
        s.enqueue(make_entry("e3", 1, 3000));
        s.enqueue(make_entry("e1", 1, 1000));
        s.enqueue(make_entry("e2", 1, 2000));

        assert_eq!(s.dequeue(4000).unwrap().execution_id, "e1");
        assert_eq!(s.dequeue(4000).unwrap().execution_id, "e2");
        assert_eq!(s.dequeue(4000).unwrap().execution_id, "e3");
    }

    #[test]
    fn fifo_ignores_priority() {
        let mut s = Scheduler::new(SchedulePolicy::Fifo);
        s.enqueue(make_entry("e1", 1, 1000));
        s.enqueue(make_entry("e2", 100, 2000));

        assert_eq!(s.dequeue(3000).unwrap().execution_id, "e1");
    }

    // -- Priority tests --

    #[test]
    fn priority_ordering() {
        let mut s = Scheduler::new(SchedulePolicy::Priority);
        s.enqueue(make_entry("e1", 1, 1000));
        s.enqueue(make_entry("e2", 10, 2000));
        s.enqueue(make_entry("e3", 5, 3000));

        assert_eq!(s.dequeue(4000).unwrap().execution_id, "e2"); // priority 10
        assert_eq!(s.dequeue(4000).unwrap().execution_id, "e3"); // priority 5
        assert_eq!(s.dequeue(4000).unwrap().execution_id, "e1"); // priority 1
    }

    #[test]
    fn priority_same_priority_uses_fifo() {
        let mut s = Scheduler::new(SchedulePolicy::Priority);
        s.enqueue(make_entry("e2", 5, 2000));
        s.enqueue(make_entry("e1", 5, 1000));

        assert_eq!(s.dequeue(3000).unwrap().execution_id, "e1"); // earlier
    }

    // -- RoundRobin tests --

    #[test]
    fn round_robin_cycles() {
        let mut s = Scheduler::new(SchedulePolicy::RoundRobin {
            agents: vec!["w1".into(), "w2".into()],
        });

        s.enqueue(make_entry_with_affinity("e1", 1, 1000, "w1"));
        s.enqueue(make_entry_with_affinity("e2", 1, 2000, "w2"));
        s.enqueue(make_entry_with_affinity("e3", 1, 3000, "w1"));
        s.enqueue(make_entry_with_affinity("e4", 1, 4000, "w2"));

        // RR: w1 -> w2 -> w1 -> w2
        assert_eq!(s.dequeue(5000).unwrap().execution_id, "e1"); // w1
        assert_eq!(s.dequeue(5000).unwrap().execution_id, "e2"); // w2
        assert_eq!(s.dequeue(5000).unwrap().execution_id, "e3"); // w1
        assert_eq!(s.dequeue(5000).unwrap().execution_id, "e4"); // w2
    }

    // -- Affinity tests --

    #[test]
    fn affinity_preferred_first() {
        let mut s = Scheduler::new(SchedulePolicy::Affinity {
            preferred_agent: "w1".into(),
        });

        s.enqueue(make_entry_with_affinity("e1", 5, 1000, "w2"));
        s.enqueue(make_entry_with_affinity("e2", 5, 2000, "w1"));
        s.enqueue(make_entry("e3", 10, 500)); // no affinity

        // e2 has w1 affinity, comes first regardless of priority.
        assert_eq!(s.dequeue(3000).unwrap().execution_id, "e2");
    }

    // -- dequeue_for_agent tests --

    #[test]
    fn dequeue_for_agent_affinity_match() {
        let mut s = Scheduler::new(SchedulePolicy::Fifo);
        s.enqueue(make_entry_with_affinity("e1", 1, 1000, "w1"));
        s.enqueue(make_entry_with_affinity("e2", 1, 2000, "w2"));

        let entry = s.dequeue_for_agent("w2", 3000).unwrap();
        assert_eq!(entry.execution_id, "e2");
    }

    #[test]
    fn dequeue_for_agent_no_affinity_fallback() {
        let mut s = Scheduler::new(SchedulePolicy::Fifo);
        s.enqueue(make_entry("e1", 1, 1000)); // no affinity
        s.enqueue(make_entry_with_affinity("e2", 1, 2000, "w1"));

        let entry = s.dequeue_for_agent("w2", 3000).unwrap();
        assert_eq!(entry.execution_id, "e1"); // no affinity matches anyone
    }

    #[test]
    fn dequeue_for_agent_empty() {
        let mut s = Scheduler::new(SchedulePolicy::Fifo);
        assert!(s.dequeue_for_agent("w1", 1000).is_none());
    }

    // -- General queue operations --

    #[test]
    fn peek_returns_first() {
        let mut s = Scheduler::new(SchedulePolicy::Priority);
        s.enqueue(make_entry("e1", 1, 1000));
        s.enqueue(make_entry("e2", 10, 2000));

        assert_eq!(s.peek().unwrap().execution_id, "e2");
        assert_eq!(s.size(), 2); // peek doesn't remove
    }

    #[test]
    fn peek_empty() {
        let s = Scheduler::new(SchedulePolicy::Fifo);
        assert!(s.peek().is_none());
    }

    #[test]
    fn remove_by_id() {
        let mut s = Scheduler::new(SchedulePolicy::Fifo);
        s.enqueue(make_entry("e1", 1, 1000));
        s.enqueue(make_entry("e2", 1, 2000));

        assert!(s.remove("e1"));
        assert_eq!(s.size(), 1);
        assert!(!s.remove("e1")); // already removed
    }

    #[test]
    fn remove_nonexistent() {
        let mut s = Scheduler::new(SchedulePolicy::Fifo);
        assert!(!s.remove("nope"));
    }

    #[test]
    fn size_and_empty() {
        let mut s = Scheduler::new(SchedulePolicy::Fifo);
        assert!(s.is_empty());
        assert_eq!(s.size(), 0);

        s.enqueue(make_entry("e1", 1, 1000));
        assert!(!s.is_empty());
        assert_eq!(s.size(), 1);
    }

    #[test]
    fn dequeue_empty() {
        let mut s = Scheduler::new(SchedulePolicy::Fifo);
        assert!(s.dequeue(1000).is_none());
    }

    // -- Reorder tests --

    #[test]
    fn reorder_fifo_to_priority() {
        let mut s = Scheduler::new(SchedulePolicy::Fifo);
        s.enqueue(make_entry("e1", 1, 1000));
        s.enqueue(make_entry("e2", 10, 2000));

        // In FIFO, e1 would be first.
        assert_eq!(s.peek().unwrap().execution_id, "e1");

        s.reorder(SchedulePolicy::Priority);
        assert_eq!(s.peek().unwrap().execution_id, "e2");
    }

    #[test]
    fn reorder_priority_to_fifo() {
        let mut s = Scheduler::new(SchedulePolicy::Priority);
        s.enqueue(make_entry("e1", 1, 1000));
        s.enqueue(make_entry("e2", 10, 2000));

        assert_eq!(s.peek().unwrap().execution_id, "e2"); // priority

        s.reorder(SchedulePolicy::Fifo);
        assert_eq!(s.peek().unwrap().execution_id, "e1"); // fifo
    }

    // -- Metrics tests --

    #[test]
    fn metrics_empty() {
        let s = Scheduler::new(SchedulePolicy::Fifo);
        let m = s.metrics(1000, 10000, 2);
        assert_eq!(m.avg_wait_time_ms, 0);
        assert_eq!(m.max_wait_time_ms, 0);
        assert_eq!(m.throughput, 0.0);
    }

    #[test]
    fn metrics_basic() {
        let mut s = Scheduler::new(SchedulePolicy::Fifo);
        s.enqueue(make_entry("e1", 1, 1000));
        s.enqueue(make_entry("e2", 1, 2000));

        s.dequeue(3000); // wait: 3000 - 1000 = 2000
        s.dequeue(5000); // wait: 5000 - 2000 = 3000

        let m = s.metrics(6000, 10000, 2);
        assert_eq!(m.avg_wait_time_ms, 2500); // (2000+3000)/2
        assert_eq!(m.max_wait_time_ms, 3000);
    }

    #[test]
    fn metrics_throughput() {
        let mut s = Scheduler::new(SchedulePolicy::Fifo);

        for i in 0..10 {
            s.enqueue(make_entry(&format!("e{}", i), 1, i as u64 * 100));
        }

        for i in 0..10 {
            s.dequeue(1000 + i as u64 * 100);
        }

        // 10 completions in a 10s window = 1.0/sec
        let m = s.metrics(2000, 10000, 2);
        assert!(m.throughput > 0.0);
    }

    #[test]
    fn metrics_utilization() {
        let mut s = Scheduler::new(SchedulePolicy::Fifo);
        s.enqueue(make_entry("e1", 1, 1000));
        s.enqueue(make_entry("e2", 1, 2000));

        // 2 entries, 4 agents -> 50% utilization
        let m = s.metrics(3000, 10000, 4);
        assert_eq!(m.utilization_percent, 50.0);
    }

    #[test]
    fn metrics_utilization_capped() {
        let mut s = Scheduler::new(SchedulePolicy::Fifo);
        for i in 0..20 {
            s.enqueue(make_entry(&format!("e{}", i), 1, 1000));
        }

        // 20 entries, 2 agents -> would be 1000% but capped at 100%.
        let m = s.metrics(2000, 10000, 2);
        assert_eq!(m.utilization_percent, 100.0);
    }

    // -- Serde tests --

    #[test]
    fn schedule_policy_serde() {
        let policies = vec![
            SchedulePolicy::Fifo,
            SchedulePolicy::Priority,
            SchedulePolicy::RoundRobin {
                agents: vec!["w1".into(), "w2".into()],
            },
            SchedulePolicy::Affinity {
                preferred_agent: "w1".into(),
            },
        ];

        for policy in &policies {
            let json = serde_json::to_string(policy).unwrap();
            let back: SchedulePolicy = serde_json::from_str(&json).unwrap();
            assert_eq!(back, *policy);
        }
    }

    #[test]
    fn schedule_entry_serde() {
        let entry = make_entry_with_affinity("e1", 5, 1000, "w1");
        let json = serde_json::to_string(&entry).unwrap();
        let back: ScheduleEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(back.execution_id, "e1");
        assert_eq!(back.priority, 5);
        assert_eq!(back.agent_affinity, Some("w1".into()));
    }

    #[test]
    fn schedule_metrics_serde() {
        let m = ScheduleMetrics {
            avg_wait_time_ms: 1000,
            max_wait_time_ms: 5000,
            throughput: 2.5,
            utilization_percent: 75.0,
        };

        let json = serde_json::to_string(&m).unwrap();
        let back: ScheduleMetrics = serde_json::from_str(&json).unwrap();
        assert_eq!(back, m);
    }

    #[test]
    fn entry_with_estimated_duration() {
        let mut entry = make_entry("e1", 1, 1000);
        entry.estimated_duration_ms = Some(30000);

        let json = serde_json::to_string(&entry).unwrap();
        let back: ScheduleEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(back.estimated_duration_ms, Some(30000));
    }

    #[test]
    fn fifo_many_entries() {
        let mut s = Scheduler::new(SchedulePolicy::Fifo);
        for i in 0..100 {
            s.enqueue(make_entry(&format!("e{}", i), 1, i as u64));
        }
        assert_eq!(s.size(), 100);

        for i in 0..100 {
            let entry = s.dequeue(1000).unwrap();
            assert_eq!(entry.execution_id, format!("e{}", i));
        }
        assert!(s.is_empty());
    }

    #[test]
    fn priority_with_many_priorities() {
        let mut s = Scheduler::new(SchedulePolicy::Priority);
        for i in 0..10 {
            s.enqueue(make_entry(&format!("e{}", i), i, 1000));
        }

        // Should come out 9, 8, 7, ... 0
        for i in (0..10).rev() {
            let entry = s.dequeue(2000).unwrap();
            assert_eq!(entry.execution_id, format!("e{}", i));
        }
    }

    #[test]
    fn dequeue_for_agent_last_resort() {
        let mut s = Scheduler::new(SchedulePolicy::Fifo);
        s.enqueue(make_entry_with_affinity("e1", 1, 1000, "w1"));

        // w2 wants work, but only w1-affinity entry exists — still gets it.
        let entry = s.dequeue_for_agent("w2", 2000).unwrap();
        assert_eq!(entry.execution_id, "e1");
    }
}
