//! Monitoring cycle — the integration glue that ties together pane capture,
//! heartbeat parsing, health assessment, message delivery, timeout detection,
//! and trigger evaluation.
//!
//! `MonitorCycle` orchestrates one pass of the monitoring loop for all agents:
//! 1. Capture pane output for each agent via `SessionBackend::capture_pane()`
//! 2. Parse heartbeat from each capture via `heartbeat::parse_capture()`
//! 3. Track output changes (detect stalls by comparing consecutive captures)
//! 4. Build health signals and assess health per agent
//! 5. Evaluate triggers against live agent state
//! 6. Deliver pending messages to agents that are in Ready state
//! 7. Check for message timeouts that need escalation

use std::collections::HashMap;

use crate::data::messages::MessageStore;
use crate::infrastructure::SessionBackend;
use crate::monitor::health;
use crate::monitor::heartbeat::{self, AgentState as HeartbeatAgentState};
use skill_docket::trigger::evaluator::{self, AgentContext, TriggerFired};
use skill_docket::trigger::registry::TriggerRegistry;
use crate::types::agent::Agent;
use crate::types::health::{HealthAssessment, HealthSignal};


// ---------------------------------------------------------------------------
// Output change tracker
// ---------------------------------------------------------------------------

/// Tracks per-agent pane output over time to detect stalls.
///
/// Each call to `check_agent` captures the pane and compares it to the
/// previous capture. If the output is unchanged, the stale count increments
/// and the staleness duration grows. A change resets the counter.
#[derive(Debug, Clone)]
pub struct OutputTracker {
    /// Last captured output per agent.
    last_captures: HashMap<String, String>,
    /// Timestamp of last output change per agent.
    last_change_ms: HashMap<String, u64>,
    /// Number of consecutive identical captures per agent.
    stale_count: HashMap<String, u32>,
}

impl OutputTracker {
    pub fn new() -> Self {
        Self {
            last_captures: HashMap::new(),
            last_change_ms: HashMap::new(),
            stale_count: HashMap::new(),
        }
    }

    /// Record a capture for an agent and determine whether the output changed.
    ///
    /// Returns `OutputCheckResult` with the heartbeat parse result and
    /// change-detection metadata.
    pub fn check_agent(
        &mut self,
        agent: &str,
        backend: &dyn SessionBackend,
        prompt_pattern: &str,
        now_ms: u64,
    ) -> Result<OutputCheckResult, String> {
        let capture = backend.capture_pane(agent)?;
        let heartbeat = heartbeat::parse_capture(&capture, prompt_pattern);

        let changed = match self.last_captures.get(agent) {
            Some(prev) => prev != &capture,
            None => true, // first capture is always "changed"
        };

        if changed {
            self.last_change_ms.insert(agent.to_string(), now_ms);
            self.stale_count.insert(agent.to_string(), 0);
        } else {
            *self.stale_count.entry(agent.to_string()).or_insert(0) += 1;
        }

        self.last_captures.insert(agent.to_string(), capture);

        Ok(OutputCheckResult {
            heartbeat,
            output_changed: changed,
            last_change_ms: *self.last_change_ms.get(agent).unwrap_or(&now_ms),
            stale_count: *self.stale_count.get(agent).unwrap_or(&0),
        })
    }

    /// Get the staleness duration for an agent (ms since last output change).
    pub fn staleness_ms(&self, agent: &str, now_ms: u64) -> u64 {
        now_ms.saturating_sub(
            *self.last_change_ms.get(agent).unwrap_or(&now_ms),
        )
    }

    /// Remove all tracking state for an agent (e.g., on kill/death).
    pub fn remove(&mut self, agent: &str) {
        self.last_captures.remove(agent);
        self.last_change_ms.remove(agent);
        self.stale_count.remove(agent);
    }
}

impl Default for OutputTracker {
    fn default() -> Self {
        Self::new()
    }
}

/// Result of checking an agent's output via `OutputTracker::check_agent`.
#[derive(Debug, Clone)]
pub struct OutputCheckResult {
    /// Parsed heartbeat from the pane capture.
    pub heartbeat: heartbeat::HeartbeatResult,
    /// Whether the captured output differs from the previous capture.
    pub output_changed: bool,
    /// Timestamp (ms) of the most recent output change.
    pub last_change_ms: u64,
    /// Number of consecutive identical captures (0 if output just changed).
    pub stale_count: u32,
}


// ---------------------------------------------------------------------------
// Message delivery bridge
// ---------------------------------------------------------------------------

/// Result of attempting to deliver a message to an agent.
#[derive(Debug, Clone)]
pub struct DeliveryResult {
    /// The agent that received the message.
    pub agent: String,
    /// The formatted message text (e.g., "[sender] body").
    pub message: String,
    /// Whether the agent was in Ready state when delivery occurred.
    pub was_ready: bool,
}

/// Alert for a message that has exceeded its timeout without delivery.
#[derive(Debug, Clone)]
pub struct TimeoutAlert {
    /// The agent whose message timed out.
    pub agent: String,
    /// How old the message is (ms).
    pub message_age_ms: u64,
    /// The text of the timed-out message.
    pub message_text: String,
}

/// Bridges the `MessageStore` to pane-based ready-state detection for delivery.
///
/// On each cycle, `deliver_pending` checks whether each agent with pending
/// messages is in Ready state (via heartbeat parsing of a pane capture) and
/// delivers the oldest message if so. `check_timeouts` detects messages that
/// have been queued longer than the configured timeout.
#[derive(Debug, Clone)]
pub struct DeliveryBridge {
    /// Maximum age in ms before a queued message triggers an escalation alert.
    message_timeout_ms: u64,
    /// Prompt pattern for ready-state detection.
    prompt_pattern: String,
}

impl DeliveryBridge {
    pub fn new(message_timeout_ms: u64, prompt_pattern: String) -> Self {
        Self {
            message_timeout_ms,
            prompt_pattern,
        }
    }

    /// Attempt to deliver pending messages to agents that are ready.
    ///
    /// For each agent with pending messages, captures the pane, parses the
    /// heartbeat, and delivers the oldest message if the agent is Ready.
    /// Returns the list of successful deliveries.
    pub fn deliver_pending(
        &self,
        store: &mut MessageStore,
        backend: &dyn SessionBackend,
        agents: &[String],
        _now_ms: u64,
    ) -> Vec<DeliveryResult> {
        let mut results = Vec::new();
        for agent in agents {
            let pending = store.pending_for(agent);
            if pending.is_empty() {
                continue;
            }

            // Check if agent is ready (capture pane, parse heartbeat)
            let capture = match backend.capture_pane(agent) {
                Ok(output) => output,
                Err(_) => continue, // can't reach agent, skip
            };
            let heartbeat = heartbeat::parse_capture(&capture, &self.prompt_pattern);

            if heartbeat.state == HeartbeatAgentState::Ready {
                // Deliver oldest pending message
                if let Some(msg) = store.deliver(agent) {
                    let formatted = format!("[{}] {}", msg.sender, msg.text);
                    results.push(DeliveryResult {
                        agent: agent.clone(),
                        message: formatted,
                        was_ready: true,
                    });
                }
            }
        }
        results
    }

    /// Check for timed-out messages that need escalation.
    ///
    /// Returns alerts for agents whose oldest pending message exceeds the
    /// configured timeout threshold.
    pub fn check_timeouts(
        &self,
        store: &MessageStore,
        agents: &[String],
        now_ms: u64,
    ) -> Vec<TimeoutAlert> {
        let mut alerts = Vec::new();
        for agent in agents {
            let pending = store.pending_for(agent);
            if let Some(oldest) = pending.first() {
                let age_ms = now_ms.saturating_sub(oldest.queued_at_ms);
                if age_ms > self.message_timeout_ms {
                    alerts.push(TimeoutAlert {
                        agent: agent.clone(),
                        message_age_ms: age_ms,
                        message_text: oldest.text.clone(),
                    });
                }
            }
        }
        alerts
    }

    /// Send an interrupt: Ctrl-C followed by optional text, regardless of
    /// the agent's ready state.
    pub fn send_interrupt(
        &self,
        backend: &mut dyn SessionBackend,
        agent: &str,
        text: Option<&str>,
    ) -> Result<(), String> {
        use cmx_utils::response::Action;

        // Send Ctrl-C first
        backend.execute_action(&Action::SendKeys {
            target: agent.to_string(),
            keys: "C-c".to_string(),
        })?;

        // Then optional text
        if let Some(t) = text {
            let formatted = format!("{} Enter", t);
            backend.execute_action(&Action::SendKeys {
                target: agent.to_string(),
                keys: formatted,
            })?;
        }
        Ok(())
    }
}


// ---------------------------------------------------------------------------
// MonitorCycle — the main orchestrator
// ---------------------------------------------------------------------------

/// Orchestrates one complete monitoring cycle for all agents.
///
/// Ties together output tracking (capture + change detection), health
/// assessment (signals + staleness), trigger evaluation, and message
/// delivery (ready-state gating + timeout escalation).
#[derive(Debug, Clone)]
pub struct MonitorCycle {
    /// Tracks output changes across cycles.
    pub tracker: OutputTracker,
    /// Bridges message store to pane-based delivery.
    pub delivery: DeliveryBridge,
    /// Prompt pattern for heartbeat parsing.
    pub prompt_pattern: String,
    /// Heartbeat timeout in seconds (used for health assessment).
    pub heartbeat_timeout_secs: u64,
    /// Registry of active triggers (global + task-scoped).
    pub trigger_registry: TriggerRegistry,
    /// Per-agent timers for heartbeat-type trigger conditions.
    pub heartbeat_timers: HashMap<String, u64>,
}

impl MonitorCycle {
    pub fn new(
        message_timeout_ms: u64,
        heartbeat_timeout_secs: u64,
        prompt_pattern: String,
    ) -> Self {
        Self {
            tracker: OutputTracker::new(),
            delivery: DeliveryBridge::new(message_timeout_ms, prompt_pattern.clone()),
            prompt_pattern,
            heartbeat_timeout_secs,
            trigger_registry: TriggerRegistry::new(),
            heartbeat_timers: HashMap::new(),
        }
    }

    /// Run one monitoring cycle.
    ///
    /// # Phases
    ///
    /// 1. **Capture + parse** — For each agent, capture pane output and parse
    ///    the heartbeat. Track output changes for stall detection.
    /// 2. **Assess health** — Build health signals from the capture results
    ///    and run them through the health assessor.
    /// 3. **Deliver messages** — Attempt to deliver pending messages to agents
    ///    that are in Ready state.
    /// 4. **Check timeouts** — Flag messages that have been pending too long.
    pub fn run_cycle(
        &mut self,
        agents: &[Agent],
        backend: &dyn SessionBackend,
        messages: &mut MessageStore,
        now_ms: u64,
    ) -> CycleResult {
        let mut health_updates = Vec::new();
        let agent_names: Vec<String> = agents.iter().map(|a| a.name.clone()).collect();

        // Phase 1 + 2: Capture, parse, and assess health per agent
        for agent in agents {
            let signals = match self.tracker.check_agent(
                &agent.name,
                backend,
                &self.prompt_pattern,
                now_ms,
            ) {
                Ok(check) => {
                    let mut sigs = vec![HealthSignal::InfrastructureOk];
                    let staleness_secs =
                        self.tracker.staleness_ms(&agent.name, now_ms) / 1000;
                    if staleness_secs > self.heartbeat_timeout_secs {
                        sigs.push(HealthSignal::HeartbeatStale {
                            age_secs: staleness_secs,
                        });
                    } else {
                        sigs.push(HealthSignal::HeartbeatRecent {
                            age_secs: staleness_secs,
                        });
                    }
                    if let HeartbeatAgentState::Error = check.heartbeat.state {
                        sigs.push(HealthSignal::ErrorPatternDetected {
                            pattern: check.heartbeat.last_line.clone(),
                        });
                    }
                    sigs
                }
                Err(_) => {
                    vec![HealthSignal::InfrastructureFailed {
                        reason: "capture failed".into(),
                    }]
                }
            };

            let assessment = health::assess(
                agent,
                &signals,
                self.heartbeat_timeout_secs,
                now_ms,
            );
            health_updates.push(assessment);
        }

        // Phase 3: Evaluate triggers against live agent state
        let mut trigger_fires = Vec::new();
        for agent in agents {
            let ctx = AgentContext {
                name: agent.name.clone(),
                last_output: self.tracker.last_captures
                    .get(&agent.name)
                    .cloned()
                    .unwrap_or_default(),
                health_state: health_updates
                    .iter()
                    .find(|h| h.agent == agent.name)
                    .map(|h| format!("{:?}", h.overall).to_lowercase())
                    .unwrap_or_else(|| "unknown".into()),
                idle_secs: self.tracker.staleness_ms(&agent.name, now_ms) / 1000,
                context_percent: None, // populated from heartbeat if available
                task: agent.task.clone(),
            };
            for block in self.trigger_registry.active_triggers(agent.task.as_deref()) {
                if let Some(fired) = evaluator::evaluate_block(
                    block,
                    &ctx,
                    now_ms,
                    &mut self.heartbeat_timers,
                ) {
                    trigger_fires.push(fired);
                }
            }
        }

        // Phase 4: Deliver pending messages to ready agents
        let deliveries = self.delivery.deliver_pending(
            messages,
            backend,
            &agent_names,
            now_ms,
        );

        // Phase 5: Check for message timeouts
        let timeouts = self.delivery.check_timeouts(
            messages,
            &agent_names,
            now_ms,
        );

        CycleResult {
            health_updates,
            trigger_fires,
            deliveries,
            timeouts,
        }
    }
}

/// Summary of one monitoring cycle's results.
#[derive(Debug, Clone)]
pub struct CycleResult {
    /// Health assessments produced for each agent.
    pub health_updates: Vec<HealthAssessment>,
    /// Triggers that fired during this cycle.
    pub trigger_fires: Vec<TriggerFired>,
    /// Messages that were successfully delivered this cycle.
    pub deliveries: Vec<DeliveryResult>,
    /// Messages that have exceeded their timeout threshold.
    pub timeouts: Vec<TimeoutAlert>,
}


// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::mock::MockBackend;
    use crate::types::agent::{AgentStatus, AgentType, HealthState};
    use crate::types::message::Message;

    fn make_agent(name: &str) -> Agent {
        Agent {
            name: name.into(),
            role: "worker".into(),
            agent_type: AgentType::Claude,
            task: None,
            path: "/tmp".into(),
            status: AgentStatus::Idle,
            status_notes: String::new(),
            health: HealthState::Healthy,
            last_heartbeat_ms: None,
            session: None,
        }
    }

    fn make_msg(sender: &str, recipient: &str, text: &str, queued_at_ms: u64) -> Message {
        Message {
            sender: sender.into(),
            recipient: recipient.into(),
            text: text.into(),
            queued_at_ms,
            delivered_at_ms: None,
        }
    }

    // ---- OutputTracker tests ----

    #[test]
    fn first_capture_is_always_changed() {
        let mut tracker = OutputTracker::new();
        let mut mock = MockBackend::new();
        mock.set_capture("w1", "some output\n$ ");

        let result = tracker.check_agent("w1", &mock, "$ ", 1000).unwrap();
        assert!(result.output_changed);
        assert_eq!(result.stale_count, 0);
    }

    #[test]
    fn identical_captures_increment_stale_count() {
        let mut tracker = OutputTracker::new();
        let mut mock = MockBackend::new();
        mock.set_capture("w1", "same output\n$ ");

        tracker.check_agent("w1", &mock, "$ ", 1000).unwrap();
        let r2 = tracker.check_agent("w1", &mock, "$ ", 2000).unwrap();
        assert!(!r2.output_changed);
        assert_eq!(r2.stale_count, 1);

        let r3 = tracker.check_agent("w1", &mock, "$ ", 3000).unwrap();
        assert!(!r3.output_changed);
        assert_eq!(r3.stale_count, 2);
    }

    #[test]
    fn changed_output_resets_stale_count() {
        let mut tracker = OutputTracker::new();
        let mut mock = MockBackend::new();
        mock.set_capture("w1", "output A");

        tracker.check_agent("w1", &mock, "$ ", 1000).unwrap();
        // Same output -> stale
        tracker.check_agent("w1", &mock, "$ ", 2000).unwrap();

        // Change output
        mock.set_capture("w1", "output B");
        let result = tracker.check_agent("w1", &mock, "$ ", 3000).unwrap();
        assert!(result.output_changed);
        assert_eq!(result.stale_count, 0);
    }

    #[test]
    fn staleness_ms_tracks_time_since_last_change() {
        let mut tracker = OutputTracker::new();
        let mut mock = MockBackend::new();
        mock.set_capture("w1", "output");

        tracker.check_agent("w1", &mock, "$ ", 1000).unwrap();
        // Same output at 5000 — last change was at 1000
        tracker.check_agent("w1", &mock, "$ ", 5000).unwrap();

        assert_eq!(tracker.staleness_ms("w1", 7000), 6000);
    }

    #[test]
    fn remove_clears_agent_tracking() {
        let mut tracker = OutputTracker::new();
        let mut mock = MockBackend::new();
        mock.set_capture("w1", "output");

        tracker.check_agent("w1", &mock, "$ ", 1000).unwrap();
        tracker.remove("w1");

        // After removal, staleness_ms returns 0 (now - now)
        assert_eq!(tracker.staleness_ms("w1", 5000), 0);
    }

    #[test]
    fn check_agent_returns_error_for_missing_capture() {
        let tracker_result = {
            let mut tracker = OutputTracker::new();
            let mock = MockBackend::new();
            tracker.check_agent("missing", &mock, "$ ", 1000)
        };
        assert!(tracker_result.is_err());
    }

    #[test]
    fn heartbeat_state_propagated_through_check() {
        let mut tracker = OutputTracker::new();
        let mut mock = MockBackend::new();

        // Ready state (prompt visible)
        mock.set_capture("w1", "done\n$ ");
        let result = tracker.check_agent("w1", &mock, "$ ", 1000).unwrap();
        assert_eq!(result.heartbeat.state, HeartbeatAgentState::Ready);

        // Busy state (no prompt)
        mock.set_capture("w2", "compiling...\nrunning tests");
        let result = tracker.check_agent("w2", &mock, "$ ", 1000).unwrap();
        assert_eq!(result.heartbeat.state, HeartbeatAgentState::Busy);

        // Error state
        mock.set_capture("w3", "Error: something broke");
        let result = tracker.check_agent("w3", &mock, "$ ", 1000).unwrap();
        assert_eq!(result.heartbeat.state, HeartbeatAgentState::Error);
    }

    // ---- DeliveryBridge tests ----

    #[test]
    fn deliver_pending_delivers_when_ready() {
        let bridge = DeliveryBridge::new(60000, "$ ".into());
        let mut store = MessageStore::new();
        store.enqueue(make_msg("pm", "w1", "do task X", 1000));

        let mut mock = MockBackend::new();
        mock.set_capture("w1", "idle\n$ ");

        let agents = vec!["w1".to_string()];
        let results = bridge.deliver_pending(&mut store, &mock, &agents, 2000);

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].agent, "w1");
        assert_eq!(results[0].message, "[pm] do task X");
        assert!(results[0].was_ready);
        // Message should be marked delivered in store
        assert!(store.pending_for("w1").is_empty());
    }

    #[test]
    fn deliver_pending_skips_busy_agent() {
        let bridge = DeliveryBridge::new(60000, "$ ".into());
        let mut store = MessageStore::new();
        store.enqueue(make_msg("pm", "w1", "do task X", 1000));

        let mut mock = MockBackend::new();
        mock.set_capture("w1", "compiling...\nrunning tests");

        let agents = vec!["w1".to_string()];
        let results = bridge.deliver_pending(&mut store, &mock, &agents, 2000);

        assert!(results.is_empty());
        // Message should still be pending
        assert_eq!(store.pending_for("w1").len(), 1);
    }

    #[test]
    fn deliver_pending_fifo_order() {
        let bridge = DeliveryBridge::new(60000, "$ ".into());
        let mut store = MessageStore::new();
        store.enqueue(make_msg("pm", "w1", "first", 1000));
        store.enqueue(make_msg("pm", "w1", "second", 2000));
        store.enqueue(make_msg("pm", "w1", "third", 3000));

        let mut mock = MockBackend::new();
        mock.set_capture("w1", "ready\n$ ");

        let agents = vec!["w1".to_string()];

        // First delivery → oldest
        let r1 = bridge.deliver_pending(&mut store, &mock, &agents, 4000);
        assert_eq!(r1.len(), 1);
        assert_eq!(r1[0].message, "[pm] first");

        // Second delivery → next oldest
        let r2 = bridge.deliver_pending(&mut store, &mock, &agents, 5000);
        assert_eq!(r2.len(), 1);
        assert_eq!(r2[0].message, "[pm] second");

        // Third delivery → last
        let r3 = bridge.deliver_pending(&mut store, &mock, &agents, 6000);
        assert_eq!(r3.len(), 1);
        assert_eq!(r3[0].message, "[pm] third");

        // No more
        let r4 = bridge.deliver_pending(&mut store, &mock, &agents, 7000);
        assert!(r4.is_empty());
    }

    #[test]
    fn deliver_pending_skips_unreachable_agent() {
        let bridge = DeliveryBridge::new(60000, "$ ".into());
        let mut store = MessageStore::new();
        store.enqueue(make_msg("pm", "w1", "hello", 1000));

        let mock = MockBackend::new(); // no captures set → will return Err

        let agents = vec!["w1".to_string()];
        let results = bridge.deliver_pending(&mut store, &mock, &agents, 2000);
        assert!(results.is_empty());
        // Message still pending
        assert_eq!(store.pending_for("w1").len(), 1);
    }

    #[test]
    fn deliver_pending_skips_agent_with_no_messages() {
        let bridge = DeliveryBridge::new(60000, "$ ".into());
        let mut store = MessageStore::new();

        let mut mock = MockBackend::new();
        mock.set_capture("w1", "ready\n$ ");

        let agents = vec!["w1".to_string()];
        let results = bridge.deliver_pending(&mut store, &mock, &agents, 2000);
        assert!(results.is_empty());
    }

    #[test]
    fn check_timeouts_detects_stale_message() {
        let bridge = DeliveryBridge::new(5000, "$ ".into());
        let store = {
            let mut s = MessageStore::new();
            s.enqueue(make_msg("pm", "w1", "old message", 1000));
            s
        };

        let agents = vec!["w1".to_string()];
        let alerts = bridge.check_timeouts(&store, &agents, 7000);

        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].agent, "w1");
        assert_eq!(alerts[0].message_age_ms, 6000);
        assert_eq!(alerts[0].message_text, "old message");
    }

    #[test]
    fn check_timeouts_ignores_fresh_message() {
        let bridge = DeliveryBridge::new(5000, "$ ".into());
        let store = {
            let mut s = MessageStore::new();
            s.enqueue(make_msg("pm", "w1", "fresh message", 1000));
            s
        };

        let agents = vec!["w1".to_string()];
        let alerts = bridge.check_timeouts(&store, &agents, 3000);

        assert!(alerts.is_empty());
    }

    #[test]
    fn check_timeouts_multiple_agents() {
        let bridge = DeliveryBridge::new(5000, "$ ".into());
        let store = {
            let mut s = MessageStore::new();
            s.enqueue(make_msg("pm", "w1", "old", 1000));
            s.enqueue(make_msg("pm", "w2", "fresh", 8000));
            s
        };

        let agents = vec!["w1".to_string(), "w2".to_string()];
        let alerts = bridge.check_timeouts(&store, &agents, 10000);

        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].agent, "w1");
    }

    #[test]
    fn interrupt_sends_ctrl_c_then_text() {
        let bridge = DeliveryBridge::new(60000, "$ ".into());
        let mut mock = MockBackend::new();

        bridge
            .send_interrupt(&mut mock, "w1", Some("new instructions"))
            .unwrap();

        assert_eq!(mock.actions.len(), 2);
        match &mock.actions[0] {
            cmx_utils::response::Action::SendKeys { target, keys } => {
                assert_eq!(target, "w1");
                assert_eq!(keys, "C-c");
            }
            other => panic!("expected SendKeys, got {:?}", other),
        }
        match &mock.actions[1] {
            cmx_utils::response::Action::SendKeys { target, keys } => {
                assert_eq!(target, "w1");
                assert_eq!(keys, "new instructions Enter");
            }
            other => panic!("expected SendKeys, got {:?}", other),
        }
    }

    #[test]
    fn interrupt_sends_only_ctrl_c_when_no_text() {
        let bridge = DeliveryBridge::new(60000, "$ ".into());
        let mut mock = MockBackend::new();

        bridge.send_interrupt(&mut mock, "w1", None).unwrap();

        assert_eq!(mock.actions.len(), 1);
        match &mock.actions[0] {
            cmx_utils::response::Action::SendKeys { target, keys } => {
                assert_eq!(target, "w1");
                assert_eq!(keys, "C-c");
            }
            other => panic!("expected SendKeys, got {:?}", other),
        }
    }

    // ---- MonitorCycle tests ----

    #[test]
    fn monitoring_cycle_detects_healthy_agent() {
        let mut cycle = MonitorCycle::new(60000, 60, "$ ".into());
        let mut mock = MockBackend::new();
        mock.set_capture("w1", "working...\n$ ");

        let agents = vec![make_agent("w1")];
        let mut messages = MessageStore::new();

        let result = cycle.run_cycle(&agents, &mock, &mut messages, 1000);

        assert_eq!(result.health_updates.len(), 1);
        assert_eq!(result.health_updates[0].agent, "w1");
        assert_eq!(result.health_updates[0].overall, HealthState::Healthy);
    }

    #[test]
    fn monitoring_cycle_detects_stale_agent() {
        let mut cycle = MonitorCycle::new(60000, 10, "$ ".into());
        let mut mock = MockBackend::new();
        mock.set_capture("w1", "stuck output");

        let agents = vec![make_agent("w1")];
        let mut messages = MessageStore::new();

        // First cycle at t=0 — output is "new" (first capture)
        cycle.run_cycle(&agents, &mock, &mut messages, 0);

        // Second cycle at t=20000 — same output, 20s stale, timeout is 10s
        let result = cycle.run_cycle(&agents, &mock, &mut messages, 20000);

        assert_eq!(result.health_updates.len(), 1);
        assert_eq!(result.health_updates[0].overall, HealthState::Unhealthy);
    }

    #[test]
    fn monitoring_cycle_delivers_message_to_ready_agent() {
        let mut cycle = MonitorCycle::new(60000, 60, "$ ".into());
        let mut mock = MockBackend::new();
        mock.set_capture("w1", "ready\n$ ");

        let agents = vec![make_agent("w1")];
        let mut messages = MessageStore::new();
        messages.enqueue(make_msg("pm", "w1", "start task", 500));

        let result = cycle.run_cycle(&agents, &mock, &mut messages, 1000);

        assert_eq!(result.deliveries.len(), 1);
        assert_eq!(result.deliveries[0].agent, "w1");
        assert_eq!(result.deliveries[0].message, "[pm] start task");
    }

    #[test]
    fn monitoring_cycle_does_not_deliver_to_busy_agent() {
        let mut cycle = MonitorCycle::new(60000, 60, "$ ".into());
        let mut mock = MockBackend::new();
        mock.set_capture("w1", "compiling...\nrunning tests");

        let agents = vec![make_agent("w1")];
        let mut messages = MessageStore::new();
        messages.enqueue(make_msg("pm", "w1", "start task", 500));

        let result = cycle.run_cycle(&agents, &mock, &mut messages, 1000);

        assert!(result.deliveries.is_empty());
    }

    #[test]
    fn monitoring_cycle_escalates_timed_out_message() {
        let mut cycle = MonitorCycle::new(5000, 60, "$ ".into());
        let mut mock = MockBackend::new();
        mock.set_capture("w1", "busy working");

        let agents = vec![make_agent("w1")];
        let mut messages = MessageStore::new();
        messages.enqueue(make_msg("pm", "w1", "urgent task", 1000));

        // Run at t=8000 — message is 7000ms old, timeout is 5000ms
        let result = cycle.run_cycle(&agents, &mock, &mut messages, 8000);

        assert_eq!(result.timeouts.len(), 1);
        assert_eq!(result.timeouts[0].agent, "w1");
        assert_eq!(result.timeouts[0].message_age_ms, 7000);
    }

    #[test]
    fn monitoring_cycle_handles_capture_failure() {
        let mut cycle = MonitorCycle::new(60000, 60, "$ ".into());
        let mock = MockBackend::new(); // no captures configured → Err

        let agents = vec![make_agent("w1")];
        let mut messages = MessageStore::new();

        let result = cycle.run_cycle(&agents, &mock, &mut messages, 1000);

        assert_eq!(result.health_updates.len(), 1);
        assert_eq!(result.health_updates[0].overall, HealthState::Unhealthy);
        assert!(result.health_updates[0].reason.contains("infrastructure"));
    }

    #[test]
    fn monitoring_cycle_multiple_agents() {
        let mut cycle = MonitorCycle::new(60000, 60, "$ ".into());
        let mut mock = MockBackend::new();
        mock.set_capture("w1", "ready\n$ ");
        mock.set_capture("w2", "compiling...");
        mock.set_capture("w3", "Error: something broke");

        let agents = vec![make_agent("w1"), make_agent("w2"), make_agent("w3")];
        let mut messages = MessageStore::new();

        let result = cycle.run_cycle(&agents, &mock, &mut messages, 1000);

        assert_eq!(result.health_updates.len(), 3);
        // w1: healthy (ready prompt)
        assert_eq!(result.health_updates[0].overall, HealthState::Healthy);
        // w2: healthy (busy but not stale)
        assert_eq!(result.health_updates[1].overall, HealthState::Healthy);
        // w3: degraded (error pattern detected)
        assert_eq!(result.health_updates[2].overall, HealthState::Degraded);
    }

    #[test]
    fn monitoring_cycle_combines_delivery_and_timeout() {
        let mut cycle = MonitorCycle::new(5000, 60, "$ ".into());
        let mut mock = MockBackend::new();
        mock.set_capture("w1", "ready\n$ ");
        mock.set_capture("w2", "busy working");

        let agents = vec![make_agent("w1"), make_agent("w2")];
        let mut messages = MessageStore::new();
        messages.enqueue(make_msg("pm", "w1", "new task", 7000));
        messages.enqueue(make_msg("pm", "w2", "old task", 1000));

        // At t=8000: w1's message is 1000ms old (fresh, delivers because ready),
        // w2's message is 7000ms old (timed out, not delivered because busy)
        let result = cycle.run_cycle(&agents, &mock, &mut messages, 8000);

        assert_eq!(result.deliveries.len(), 1);
        assert_eq!(result.deliveries[0].agent, "w1");

        assert_eq!(result.timeouts.len(), 1);
        assert_eq!(result.timeouts[0].agent, "w2");
    }

    #[test]
    fn output_tracker_default_impl() {
        let tracker = OutputTracker::default();
        assert_eq!(tracker.staleness_ms("any", 1000), 0);
    }

    // ---- Trigger integration tests ----

    #[test]
    fn cycle_with_no_triggers_empty_fires() {
        let mut cycle = MonitorCycle::new(60000, 60, "$ ".into());
        let mut mock = MockBackend::new();
        mock.set_capture("w1", "working...\n$ ");

        let agents = vec![make_agent("w1")];
        let mut messages = MessageStore::new();

        let result = cycle.run_cycle(&agents, &mock, &mut messages, 1000);
        assert!(result.trigger_fires.is_empty());
    }

    #[test]
    fn cycle_with_global_trigger_matching_fires() {
        use skill_docket::trigger::parser::*;

        let mut cycle = MonitorCycle::new(60000, 60, "$ ".into());
        let mut mock = MockBackend::new();
        mock.set_capture("w1", "something went wrong: error detected\n");

        // Register a global trigger that fires on "error"
        let block = TriggerBlock {
            name: Some("error-watcher".into()),
            clauses: vec![TriggerClause {
                condition: Condition::Contains {
                    agent: "{agent}".into(),
                    pattern: "error".into(),
                },
                action: TriggerAction {
                    command_template: "cmx tell pm \"{agent} has error\"".into(),
                },
                is_else: false,
            }],
        };
        cycle.trigger_registry.set_global(vec![block]);

        let agents = vec![make_agent("w1")];
        let mut messages = MessageStore::new();

        let result = cycle.run_cycle(&agents, &mock, &mut messages, 1000);
        assert_eq!(result.trigger_fires.len(), 1);
        assert_eq!(result.trigger_fires[0].agent, "w1");
        assert_eq!(
            result.trigger_fires[0].action_command,
            "cmx tell pm \"w1 has error\""
        );
    }

    #[test]
    fn cycle_with_task_trigger_fires_only_for_that_agent() {
        use skill_docket::trigger::parser::*;

        let mut cycle = MonitorCycle::new(60000, 60, "$ ".into());
        let mut mock = MockBackend::new();
        mock.set_capture("w1", "working on task\n$ ");
        mock.set_capture("w2", "working on task\n$ ");

        // Register a task-scoped trigger for task "T1"
        let block = TriggerBlock {
            name: Some("task-watcher".into()),
            clauses: vec![TriggerClause {
                condition: Condition::Contains {
                    agent: "{agent}".into(),
                    pattern: "working".into(),
                },
                action: TriggerAction {
                    command_template: "cmx tell pm \"{agent} working\"".into(),
                },
                is_else: false,
            }],
        };
        cycle.trigger_registry.set_task_triggers("T1", vec![block]);

        // w1 is assigned to T1, w2 is not
        let mut w1 = make_agent("w1");
        w1.task = Some("T1".into());
        let w2 = make_agent("w2"); // no task

        let agents = vec![w1, w2];
        let mut messages = MessageStore::new();

        let result = cycle.run_cycle(&agents, &mock, &mut messages, 1000);

        // Only w1 should fire (it has task T1 matching the task trigger)
        assert_eq!(result.trigger_fires.len(), 1);
        assert_eq!(result.trigger_fires[0].agent, "w1");
    }
}
