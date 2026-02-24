//! Self-diagnosis module — tracks intervention outcomes and signal
//! reliability, enabling the system to learn which monitoring signals
//! are trustworthy and which interventions succeed.
//!
//! The main entry point is `DiagnosisEngine`, which manages event
//! recording, persistence, reliability computation, adaptive thresholds,
//! and report generation.

pub mod events;
pub mod reliability;
pub mod report;
pub mod thresholds;

use std::collections::HashMap;
use std::path::PathBuf;

pub use events::{
    DiagnosisError, InterventionAction, InterventionEvent, InterventionOutcome,
    SignalType,
};
pub use reliability::{ActionEffectiveness, SignalReliability};
pub use thresholds::AdaptiveThreshold;


// ---------------------------------------------------------------------------
// DiagnosisEngine
// ---------------------------------------------------------------------------

/// The main interface for the self-diagnosis subsystem.
///
/// Records intervention events, computes signal reliability and action
/// effectiveness, adjusts thresholds, and generates diagnostic reports.
pub struct DiagnosisEngine {
    events: Vec<InterventionEvent>,
    reliability: HashMap<SignalType, SignalReliability>,
    effectiveness: HashMap<(SignalType, InterventionAction), ActionEffectiveness>,
    thresholds: HashMap<SignalType, AdaptiveThreshold>,
    events_path: PathBuf,
    next_id: u64,
    max_events: usize,
}

impl DiagnosisEngine {
    /// Default maximum number of events to retain.
    const DEFAULT_MAX_EVENTS: usize = 10_000;

    /// Create a new engine, loading existing events from disk if available.
    ///
    /// `config_dir` is the CMX configuration directory (e.g. `~/.config/cmx/`).
    /// Events are stored at `config_dir/logs/events.jsonl`.
    pub fn new(config_dir: PathBuf) -> Result<Self, DiagnosisError> {
        Self::with_capacity(config_dir, Self::DEFAULT_MAX_EVENTS)
    }

    /// Create with a custom max_events limit.
    pub fn with_capacity(
        config_dir: PathBuf,
        max_events: usize,
    ) -> Result<Self, DiagnosisError> {
        let events_path = config_dir.join("logs").join("events.jsonl");
        Self::load(events_path, max_events)
    }

    /// Load all events from the JSONL file and rebuild statistics.
    pub fn load(
        events_path: PathBuf,
        max_events: usize,
    ) -> Result<Self, DiagnosisError> {
        let mut loaded = events::load_events(&events_path)?;

        // Apply bounded history.
        if loaded.len() > max_events {
            let excess = loaded.len() - max_events;
            loaded.drain(0..excess);
        }

        let next_id = loaded.last().map(|e| e.id + 1).unwrap_or(0);
        let reliability = reliability::compute_reliability(&loaded);
        let effectiveness = reliability::compute_effectiveness(&loaded);

        Ok(DiagnosisEngine {
            events: loaded,
            reliability,
            effectiveness,
            thresholds: HashMap::new(),
            events_path,
            next_id,
            max_events,
        })
    }

    // -------------------------------------------------------------------
    // Event recording
    // -------------------------------------------------------------------

    /// Record a fully-formed intervention event. Updates reliability and
    /// effectiveness stats and persists to disk.
    pub fn record(
        &mut self,
        mut event: InterventionEvent,
    ) -> Result<(), DiagnosisError> {
        event.id = self.next_id;
        self.next_id += 1;

        events::append_event(&self.events_path, &event)?;
        self.events.push(event);
        self.enforce_bounds()?;
        self.recompute_stats();
        Ok(())
    }

    /// Record a signal fire with no outcome yet (pending).
    /// Returns the event ID for later outcome recording via `record_outcome`.
    pub fn record_signal(
        &mut self,
        agent: &str,
        signal: SignalType,
        detail: &str,
        now_ms: u64,
    ) -> Result<u64, DiagnosisError> {
        let id = self.next_id;
        self.next_id += 1;

        let event = InterventionEvent {
            id,
            timestamp_ms: now_ms,
            agent: agent.to_string(),
            signal,
            signal_detail: detail.to_string(),
            action: InterventionAction::Ignore,
            outcome: InterventionOutcome::Pending,
            outcome_detail: String::new(),
            duration_ms: 0,
            failure_mode: "none".to_string(),
        };

        events::append_event(&self.events_path, &event)?;
        self.events.push(event);
        self.enforce_bounds()?;
        // Don't recompute stats here — pending events are skipped.
        Ok(id)
    }

    /// Record the outcome of a previously recorded signal.
    pub fn record_outcome(
        &mut self,
        event_id: u64,
        action: InterventionAction,
        outcome: InterventionOutcome,
        detail: &str,
        now_ms: u64,
    ) -> Result<(), DiagnosisError> {
        let event = self
            .events
            .iter_mut()
            .find(|e| e.id == event_id)
            .ok_or(DiagnosisError::EventNotFound(event_id))?;

        if event.outcome != InterventionOutcome::Pending {
            return Err(DiagnosisError::InvalidOutcome(format!(
                "event {} already has outcome {:?}",
                event_id, event.outcome
            )));
        }

        event.action = action;
        event.outcome = outcome;
        event.outcome_detail = detail.to_string();
        event.duration_ms = now_ms.saturating_sub(event.timestamp_ms);

        // Classify failure mode based on outcome.
        event.failure_mode = match &event.outcome {
            InterventionOutcome::Resolved => "none".to_string(),
            InterventionOutcome::StillBroken => "agent".to_string(),
            InterventionOutcome::DifferentError => "strategic".to_string(),
            InterventionOutcome::SelfResolved => "none".to_string(),
            InterventionOutcome::Timeout => "unknown".to_string(),
            InterventionOutcome::Pending => "none".to_string(),
        };

        // Full rewrite since we modified an existing event.
        self.save()?;
        self.recompute_stats();
        Ok(())
    }

    // -------------------------------------------------------------------
    // Persistence
    // -------------------------------------------------------------------

    /// Save all events (full rewrite). Used after compaction or outcome updates.
    pub fn save(&self) -> Result<(), DiagnosisError> {
        events::save_all_events(&self.events_path, &self.events)
    }

    /// Enforce bounded history. If events exceed max_events, prune the
    /// oldest and do a full rewrite.
    fn enforce_bounds(&mut self) -> Result<(), DiagnosisError> {
        if self.events.len() > self.max_events {
            let excess = self.events.len() - self.max_events;
            self.events.drain(0..excess);
            self.save()?;
        }
        Ok(())
    }

    // -------------------------------------------------------------------
    // Statistics
    // -------------------------------------------------------------------

    /// Recompute all reliability and effectiveness scores from event history.
    fn recompute_stats(&mut self) {
        self.reliability = reliability::compute_reliability(&self.events);
        self.effectiveness = reliability::compute_effectiveness(&self.events);
    }

    /// Get reliability for a specific signal type.
    pub fn signal_reliability(
        &self,
        signal: &SignalType,
    ) -> Option<&SignalReliability> {
        self.reliability.get(signal)
    }

    /// Get all signal reliability stats, sorted by reliability score (lowest first).
    pub fn all_reliability(&self) -> Vec<&SignalReliability> {
        let mut entries: Vec<&SignalReliability> =
            self.reliability.values().collect();
        entries.sort_by(|a, b| {
            a.reliability_score
                .partial_cmp(&b.reliability_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        entries
    }

    /// Get effectiveness of a specific action for a specific signal.
    pub fn action_effectiveness(
        &self,
        signal: &SignalType,
        action: &InterventionAction,
    ) -> Option<&ActionEffectiveness> {
        self.effectiveness.get(&(signal.clone(), action.clone()))
    }

    /// Get the best action for a signal (highest success rate with at least
    /// `min_attempts` attempts).
    pub fn best_action(
        &self,
        signal: &SignalType,
        min_attempts: u64,
    ) -> Option<&ActionEffectiveness> {
        reliability::best_action_for_signal(&self.effectiveness, signal, min_attempts)
    }

    // -------------------------------------------------------------------
    // Thresholds
    // -------------------------------------------------------------------

    /// Recompute adjusted thresholds based on reliability scores.
    pub fn recompute_thresholds(
        &mut self,
        base_thresholds: &HashMap<SignalType, u64>,
    ) {
        self.thresholds =
            thresholds::compute_thresholds(base_thresholds, &self.reliability);
    }

    /// Get the adjusted timeout for a signal type.
    pub fn adjusted_timeout(&self, signal: &SignalType) -> Option<u64> {
        self.thresholds
            .get(signal)
            .map(|t| t.adjusted_timeout_ms)
    }

    /// Get all current thresholds.
    pub fn all_thresholds(&self) -> &HashMap<SignalType, AdaptiveThreshold> {
        &self.thresholds
    }

    // -------------------------------------------------------------------
    // Report
    // -------------------------------------------------------------------

    /// Generate a markdown report summarizing operational statistics.
    pub fn generate_report(&self) -> String {
        report::generate_report(
            &self.events,
            &self.reliability,
            &self.effectiveness,
            &self.thresholds,
        )
    }

    // -------------------------------------------------------------------
    // Accessors
    // -------------------------------------------------------------------

    /// All recorded events.
    pub fn events(&self) -> &[InterventionEvent] {
        &self.events
    }

    /// The last `n` events, in chronological order.
    pub fn recent_events(&self, n: usize) -> &[InterventionEvent] {
        let start = if self.events.len() > n {
            self.events.len() - n
        } else {
            0
        };
        &self.events[start..]
    }

    /// Number of events.
    pub fn event_count(&self) -> usize {
        self.events.len()
    }
}


// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn test_engine(name: &str) -> DiagnosisEngine {
        let dir = events::test_dir(name);
        DiagnosisEngine::with_capacity(dir, 100).unwrap()
    }

    fn test_engine_with_cap(name: &str, cap: usize) -> DiagnosisEngine {
        let dir = events::test_dir(name);
        DiagnosisEngine::with_capacity(dir, cap).unwrap()
    }

    // --- Test 1: Record events and verify reliability ---

    #[test]
    fn record_events_and_verify_reliability() {
        let mut engine = test_engine("t01_reliability");

        // 8 true positives (Resolved)
        for i in 0..8 {
            engine
                .record(InterventionEvent {
                    id: 0,
                    timestamp_ms: 1000 + i * 100,
                    agent: "w1".into(),
                    signal: SignalType::HeartbeatStale,
                    signal_detail: "stale".into(),
                    action: InterventionAction::Retry,
                    outcome: InterventionOutcome::Resolved,
                    outcome_detail: "fixed".into(),
                    duration_ms: 500,
                    failure_mode: "none".into(),
                })
                .unwrap();
        }
        // 2 false positives (SelfResolved)
        for i in 8..10 {
            engine
                .record(InterventionEvent {
                    id: 0,
                    timestamp_ms: 1000 + i * 100,
                    agent: "w1".into(),
                    signal: SignalType::HeartbeatStale,
                    signal_detail: "stale".into(),
                    action: InterventionAction::Ignore,
                    outcome: InterventionOutcome::SelfResolved,
                    outcome_detail: "went away".into(),
                    duration_ms: 200,
                    failure_mode: "none".into(),
                })
                .unwrap();
        }

        let rel = engine.signal_reliability(&SignalType::HeartbeatStale).unwrap();
        assert_eq!(rel.total_fires, 10);
        assert_eq!(rel.true_positives, 8);
        assert_eq!(rel.false_positives, 2);
        assert!((rel.reliability_score - 0.8).abs() < 0.001);
    }

    // --- Test 2: 8 TP / 2 FP reliability = 0.8 ---

    #[test]
    fn reliability_score_0_8() {
        let mut engine = test_engine("t02_rel08");

        for _ in 0..8 {
            engine
                .record(InterventionEvent {
                    id: 0,
                    timestamp_ms: 1000,
                    agent: "w1".into(),
                    signal: SignalType::ErrorPattern,
                    signal_detail: "err".into(),
                    action: InterventionAction::Restart,
                    outcome: InterventionOutcome::StillBroken,
                    outcome_detail: "still broken".into(),
                    duration_ms: 1000,
                    failure_mode: "agent".into(),
                })
                .unwrap();
        }
        for _ in 0..2 {
            engine
                .record(InterventionEvent {
                    id: 0,
                    timestamp_ms: 1000,
                    agent: "w1".into(),
                    signal: SignalType::ErrorPattern,
                    signal_detail: "err".into(),
                    action: InterventionAction::Ignore,
                    outcome: InterventionOutcome::SelfResolved,
                    outcome_detail: "ok".into(),
                    duration_ms: 500,
                    failure_mode: "none".into(),
                })
                .unwrap();
        }

        let rel = engine.signal_reliability(&SignalType::ErrorPattern).unwrap();
        assert!((rel.reliability_score - 0.8).abs() < 0.001);
    }

    // --- Test 3: All self-resolved = reliability 0.0 ---

    #[test]
    fn all_self_resolved_reliability_zero() {
        let mut engine = test_engine("t03_rel0");

        for _ in 0..5 {
            engine
                .record(InterventionEvent {
                    id: 0,
                    timestamp_ms: 1000,
                    agent: "w1".into(),
                    signal: SignalType::OutputStall,
                    signal_detail: "stall".into(),
                    action: InterventionAction::Ignore,
                    outcome: InterventionOutcome::SelfResolved,
                    outcome_detail: "ok".into(),
                    duration_ms: 100,
                    failure_mode: "none".into(),
                })
                .unwrap();
        }

        let rel = engine.signal_reliability(&SignalType::OutputStall).unwrap();
        assert!((rel.reliability_score - 0.0).abs() < 0.001);
    }

    // --- Test 4: best_action picks highest success rate ---

    #[test]
    fn best_action_for_signal() {
        let mut engine = test_engine("t04_best");

        // Retry: 1 success, 2 failures => 33%
        for outcome in &[
            InterventionOutcome::Resolved,
            InterventionOutcome::StillBroken,
            InterventionOutcome::StillBroken,
        ] {
            engine
                .record(InterventionEvent {
                    id: 0,
                    timestamp_ms: 1000,
                    agent: "w1".into(),
                    signal: SignalType::HeartbeatStale,
                    signal_detail: "stale".into(),
                    action: InterventionAction::Retry,
                    outcome: outcome.clone(),
                    outcome_detail: "test".into(),
                    duration_ms: 1000,
                    failure_mode: "none".into(),
                })
                .unwrap();
        }

        // Restart: 2 successes, 1 failure => 67%
        for outcome in &[
            InterventionOutcome::Resolved,
            InterventionOutcome::Resolved,
            InterventionOutcome::StillBroken,
        ] {
            engine
                .record(InterventionEvent {
                    id: 0,
                    timestamp_ms: 1000,
                    agent: "w1".into(),
                    signal: SignalType::HeartbeatStale,
                    signal_detail: "stale".into(),
                    action: InterventionAction::Restart,
                    outcome: outcome.clone(),
                    outcome_detail: "test".into(),
                    duration_ms: 1000,
                    failure_mode: "none".into(),
                })
                .unwrap();
        }

        let best = engine
            .best_action(&SignalType::HeartbeatStale, 2)
            .unwrap();
        assert_eq!(best.action, InterventionAction::Restart);
    }

    // --- Test 5: Adaptive thresholds ---

    #[test]
    fn adaptive_thresholds_direction() {
        let mut engine = test_engine("t05_thresh");

        // High reliability signal
        for _ in 0..10 {
            engine
                .record(InterventionEvent {
                    id: 0,
                    timestamp_ms: 1000,
                    agent: "w1".into(),
                    signal: SignalType::HeartbeatStale,
                    signal_detail: "stale".into(),
                    action: InterventionAction::Retry,
                    outcome: InterventionOutcome::Resolved,
                    outcome_detail: "ok".into(),
                    duration_ms: 500,
                    failure_mode: "none".into(),
                })
                .unwrap();
        }

        // Low reliability signal
        for _ in 0..10 {
            engine
                .record(InterventionEvent {
                    id: 0,
                    timestamp_ms: 1000,
                    agent: "w1".into(),
                    signal: SignalType::OutputStall,
                    signal_detail: "stall".into(),
                    action: InterventionAction::Ignore,
                    outcome: InterventionOutcome::SelfResolved,
                    outcome_detail: "ok".into(),
                    duration_ms: 100,
                    failure_mode: "none".into(),
                })
                .unwrap();
        }

        let mut base = HashMap::new();
        base.insert(SignalType::HeartbeatStale, 60_000_u64);
        base.insert(SignalType::OutputStall, 60_000_u64);
        engine.recompute_thresholds(&base);

        let hb = engine.adjusted_timeout(&SignalType::HeartbeatStale).unwrap();
        let os = engine.adjusted_timeout(&SignalType::OutputStall).unwrap();

        // High reliability => shorter timeout
        assert!(hb < 60_000);
        // Low reliability => longer timeout
        assert!(os > 60_000);
    }

    // --- Test 6: JSONL persistence round-trip ---

    #[test]
    fn persistence_round_trip() {
        let dir = events::test_dir("t06_persist");

        // Create engine and record events.
        {
            let mut engine =
                DiagnosisEngine::with_capacity(dir.clone(), 100).unwrap();
            for i in 0..5 {
                engine
                    .record(InterventionEvent {
                        id: 0,
                        timestamp_ms: 1000 + i * 100,
                        agent: "w1".into(),
                        signal: SignalType::HeartbeatStale,
                        signal_detail: "stale".into(),
                        action: InterventionAction::Retry,
                        outcome: InterventionOutcome::Resolved,
                        outcome_detail: "ok".into(),
                        duration_ms: 500,
                        failure_mode: "none".into(),
                    })
                    .unwrap();
            }
            assert_eq!(engine.event_count(), 5);
        }

        // Reload from disk.
        let engine2 =
            DiagnosisEngine::with_capacity(dir, 100).unwrap();
        assert_eq!(engine2.event_count(), 5);
        let rel = engine2
            .signal_reliability(&SignalType::HeartbeatStale)
            .unwrap();
        assert!((rel.reliability_score - 1.0).abs() < 0.001);
    }

    // --- Test 7: Bounded history ---

    #[test]
    fn bounded_history() {
        let mut engine = test_engine_with_cap("t07_bounded", 50);

        for i in 0..150 {
            engine
                .record(InterventionEvent {
                    id: 0,
                    timestamp_ms: i * 100,
                    agent: "w1".into(),
                    signal: SignalType::HeartbeatStale,
                    signal_detail: "stale".into(),
                    action: InterventionAction::Retry,
                    outcome: InterventionOutcome::Resolved,
                    outcome_detail: "ok".into(),
                    duration_ms: 500,
                    failure_mode: "none".into(),
                })
                .unwrap();
        }

        assert!(engine.event_count() <= 50);
    }

    // --- Test 8: Two-step API ---

    #[test]
    fn two_step_api() {
        let mut engine = test_engine("t08_twostep");

        let id = engine
            .record_signal("w1", SignalType::SshDisconnected, "ssh down", 1000)
            .unwrap();

        // Verify it's pending.
        let event = engine.events().iter().find(|e| e.id == id).unwrap();
        assert_eq!(event.outcome, InterventionOutcome::Pending);

        // Record outcome.
        engine
            .record_outcome(
                id,
                InterventionAction::Restart,
                InterventionOutcome::Resolved,
                "restarted agent",
                2000,
            )
            .unwrap();

        let event = engine.events().iter().find(|e| e.id == id).unwrap();
        assert_eq!(event.outcome, InterventionOutcome::Resolved);
        assert_eq!(event.action, InterventionAction::Restart);
        assert_eq!(event.duration_ms, 1000); // 2000 - 1000
    }

    // --- Test 9: record_outcome with invalid ID ---

    #[test]
    fn record_outcome_invalid_id() {
        let mut engine = test_engine("t09_invalid_id");

        let result = engine.record_outcome(
            9999,
            InterventionAction::Retry,
            InterventionOutcome::Resolved,
            "ok",
            1000,
        );
        assert!(matches!(result, Err(DiagnosisError::EventNotFound(9999))));
    }

    // --- Test 10: Report generation ---

    #[test]
    fn report_generation() {
        let mut engine = test_engine("t10_report");

        engine
            .record(InterventionEvent {
                id: 0,
                timestamp_ms: 1000,
                agent: "w1".into(),
                signal: SignalType::HeartbeatStale,
                signal_detail: "stale".into(),
                action: InterventionAction::Retry,
                outcome: InterventionOutcome::Resolved,
                outcome_detail: "ok".into(),
                duration_ms: 500,
                failure_mode: "none".into(),
            })
            .unwrap();

        let report = engine.generate_report();
        assert!(report.contains("# Diagnosis Report"));
        assert!(report.contains("## Signal Reliability"));
        assert!(report.contains("## Recommendations"));
    }

    // --- Test 11: Empty engine ---

    #[test]
    fn empty_engine() {
        let engine = test_engine("t11_empty");

        assert_eq!(engine.event_count(), 0);
        assert!(engine.signal_reliability(&SignalType::HeartbeatStale).is_none());
        assert!(engine.all_reliability().is_empty());
        assert!(engine
            .action_effectiveness(&SignalType::HeartbeatStale, &InterventionAction::Retry)
            .is_none());
        assert!(engine.best_action(&SignalType::HeartbeatStale, 1).is_none());
        assert!(engine.adjusted_timeout(&SignalType::HeartbeatStale).is_none());

        // Report should still generate without errors.
        let report = engine.generate_report();
        assert!(report.contains("No intervention events recorded"));
    }

    // --- Test 12: Compaction recomputes stats ---

    #[test]
    fn compaction_recomputes_stats() {
        let mut engine = test_engine_with_cap("t12_compact", 5);

        // Record 3 false positives first.
        for _ in 0..3 {
            engine
                .record(InterventionEvent {
                    id: 0,
                    timestamp_ms: 1000,
                    agent: "w1".into(),
                    signal: SignalType::HeartbeatStale,
                    signal_detail: "stale".into(),
                    action: InterventionAction::Ignore,
                    outcome: InterventionOutcome::SelfResolved,
                    outcome_detail: "ok".into(),
                    duration_ms: 100,
                    failure_mode: "none".into(),
                })
                .unwrap();
        }

        // Now record 5 true positives — pushes the false positives out.
        for _ in 0..5 {
            engine
                .record(InterventionEvent {
                    id: 0,
                    timestamp_ms: 2000,
                    agent: "w1".into(),
                    signal: SignalType::HeartbeatStale,
                    signal_detail: "stale".into(),
                    action: InterventionAction::Retry,
                    outcome: InterventionOutcome::Resolved,
                    outcome_detail: "ok".into(),
                    duration_ms: 500,
                    failure_mode: "none".into(),
                })
                .unwrap();
        }

        // After compaction, only 5 events remain — all true positives.
        assert_eq!(engine.event_count(), 5);
        let rel = engine.signal_reliability(&SignalType::HeartbeatStale).unwrap();
        assert!((rel.reliability_score - 1.0).abs() < 0.001);
    }

    // --- Test: record_outcome on already-completed event ---

    #[test]
    fn record_outcome_already_completed() {
        let mut engine = test_engine("t13_double_outcome");

        let id = engine
            .record_signal("w1", SignalType::HeartbeatStale, "stale", 1000)
            .unwrap();

        engine
            .record_outcome(
                id,
                InterventionAction::Retry,
                InterventionOutcome::Resolved,
                "fixed",
                2000,
            )
            .unwrap();

        // Trying to record outcome again should fail.
        let result = engine.record_outcome(
            id,
            InterventionAction::Restart,
            InterventionOutcome::StillBroken,
            "nope",
            3000,
        );
        assert!(matches!(result, Err(DiagnosisError::InvalidOutcome(_))));
    }

    // --- Test: recent_events ---

    #[test]
    fn recent_events() {
        let mut engine = test_engine("t14_recent");

        for i in 0..10 {
            engine
                .record(InterventionEvent {
                    id: 0,
                    timestamp_ms: i * 100,
                    agent: "w1".into(),
                    signal: SignalType::HeartbeatStale,
                    signal_detail: "stale".into(),
                    action: InterventionAction::Retry,
                    outcome: InterventionOutcome::Resolved,
                    outcome_detail: "ok".into(),
                    duration_ms: 500,
                    failure_mode: "none".into(),
                })
                .unwrap();
        }

        let last3 = engine.recent_events(3);
        assert_eq!(last3.len(), 3);
        assert_eq!(last3[0].id, 7);
        assert_eq!(last3[2].id, 9);

        let all = engine.recent_events(100);
        assert_eq!(all.len(), 10);
    }
}
