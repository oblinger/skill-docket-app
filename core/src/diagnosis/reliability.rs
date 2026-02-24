//! Per-signal reliability tracking and per-(signal, action) effectiveness.
//!
//! Reliability scores measure how often a signal correctly identifies a real
//! problem. Effectiveness scores measure how often a particular intervention
//! succeeds for a given signal type.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::events::{
    InterventionAction, InterventionEvent, InterventionOutcome, SignalType,
};


// ---------------------------------------------------------------------------
// SignalReliability
// ---------------------------------------------------------------------------

/// Accumulated statistics for a single signal type.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignalReliability {
    pub signal: SignalType,
    pub total_fires: u64,
    /// Followed by confirmed failure (Resolved or StillBroken).
    pub true_positives: u64,
    /// Self-resolved — signal fired but there was no real problem.
    pub false_positives: u64,
    /// Timeout or unclear outcome.
    pub unknown: u64,
    /// true_positives / (true_positives + false_positives), 0.0-1.0.
    /// Defaults to 0.5 if no classifiable data.
    pub reliability_score: f64,
    /// Average time to resolve when intervention succeeds.
    pub avg_resolution_ms: u64,
}

impl SignalReliability {
    fn new(signal: SignalType) -> Self {
        SignalReliability {
            signal,
            total_fires: 0,
            true_positives: 0,
            false_positives: 0,
            unknown: 0,
            reliability_score: 0.5,
            avg_resolution_ms: 0,
        }
    }
}


// ---------------------------------------------------------------------------
// ActionEffectiveness
// ---------------------------------------------------------------------------

/// How well a particular intervention works for a given signal type.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionEffectiveness {
    pub signal: SignalType,
    pub action: InterventionAction,
    pub attempts: u64,
    /// outcome == Resolved.
    pub successes: u64,
    /// outcome == StillBroken or DifferentError.
    pub failures: u64,
    /// successes / attempts, 0.0-1.0.
    pub success_rate: f64,
}

impl ActionEffectiveness {
    fn new(signal: SignalType, action: InterventionAction) -> Self {
        ActionEffectiveness {
            signal,
            action,
            attempts: 0,
            successes: 0,
            failures: 0,
            success_rate: 0.0,
        }
    }
}


// ---------------------------------------------------------------------------
// Computation
// ---------------------------------------------------------------------------

/// Recompute all signal reliability stats from a set of events.
/// Returns a map from SignalType to SignalReliability.
pub fn compute_reliability(
    events: &[InterventionEvent],
) -> HashMap<SignalType, SignalReliability> {
    let mut map: HashMap<SignalType, SignalReliability> = HashMap::new();

    for event in events {
        let entry = map
            .entry(event.signal.clone())
            .or_insert_with(|| SignalReliability::new(event.signal.clone()));

        // Skip pending events — they haven't resolved yet.
        if event.outcome == InterventionOutcome::Pending {
            continue;
        }

        entry.total_fires += 1;

        match &event.outcome {
            InterventionOutcome::Resolved => {
                entry.true_positives += 1;
            }
            InterventionOutcome::StillBroken => {
                entry.true_positives += 1;
            }
            InterventionOutcome::SelfResolved => {
                entry.false_positives += 1;
            }
            InterventionOutcome::Timeout | InterventionOutcome::DifferentError => {
                entry.unknown += 1;
            }
            InterventionOutcome::Pending => {
                // Already handled above.
            }
        }
    }

    // Compute reliability scores and average resolution time.
    for entry in map.values_mut() {
        let denom = entry.true_positives + entry.false_positives;
        entry.reliability_score = if denom > 0 {
            entry.true_positives as f64 / denom as f64
        } else {
            0.5
        };

        // Average resolution time for successful interventions.
        let resolved_events: Vec<&InterventionEvent> = events
            .iter()
            .filter(|e| {
                e.signal == entry.signal && e.outcome == InterventionOutcome::Resolved
            })
            .collect();
        if !resolved_events.is_empty() {
            let total_ms: u64 = resolved_events.iter().map(|e| e.duration_ms).sum();
            entry.avg_resolution_ms = total_ms / resolved_events.len() as u64;
        }
    }

    map
}

/// Recompute all action effectiveness stats from a set of events.
/// Returns a map from (SignalType, InterventionAction) to ActionEffectiveness.
pub fn compute_effectiveness(
    events: &[InterventionEvent],
) -> HashMap<(SignalType, InterventionAction), ActionEffectiveness> {
    let mut map: HashMap<(SignalType, InterventionAction), ActionEffectiveness> =
        HashMap::new();

    for event in events {
        // Skip pending events.
        if event.outcome == InterventionOutcome::Pending {
            continue;
        }

        let key = (event.signal.clone(), event.action.clone());
        let entry = map
            .entry(key.clone())
            .or_insert_with(|| ActionEffectiveness::new(key.0, key.1));

        entry.attempts += 1;

        match &event.outcome {
            InterventionOutcome::Resolved => {
                entry.successes += 1;
            }
            InterventionOutcome::StillBroken | InterventionOutcome::DifferentError => {
                entry.failures += 1;
            }
            _ => {
                // Timeout, SelfResolved, Pending don't count as success or failure
                // for action effectiveness, but do count as attempts.
            }
        }
    }

    // Compute success rates.
    for entry in map.values_mut() {
        entry.success_rate = if entry.attempts > 0 {
            entry.successes as f64 / entry.attempts as f64
        } else {
            0.0
        };
    }

    map
}

/// Find the best action for a given signal type (highest success rate
/// with at least `min_attempts` attempts).
pub fn best_action_for_signal<'a>(
    effectiveness: &'a HashMap<(SignalType, InterventionAction), ActionEffectiveness>,
    signal: &SignalType,
    min_attempts: u64,
) -> Option<&'a ActionEffectiveness> {
    effectiveness
        .values()
        .filter(|e| &e.signal == signal && e.attempts >= min_attempts)
        .max_by(|a, b| {
            a.success_rate
                .partial_cmp(&b.success_rate)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
}


// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnosis::events::InterventionEvent;

    fn make_event(
        id: u64,
        signal: SignalType,
        action: InterventionAction,
        outcome: InterventionOutcome,
        duration_ms: u64,
    ) -> InterventionEvent {
        InterventionEvent {
            id,
            timestamp_ms: 1000 + id * 100,
            agent: "worker-1".to_string(),
            signal,
            signal_detail: "test detail".to_string(),
            action,
            outcome,
            outcome_detail: "test outcome".to_string(),
            duration_ms,
            failure_mode: "none".to_string(),
        }
    }

    #[test]
    fn reliability_true_and_false_positives() {
        // 8 true positives, 2 false positives => reliability 0.8
        let mut events = Vec::new();
        for i in 0..8 {
            events.push(make_event(
                i,
                SignalType::HeartbeatStale,
                InterventionAction::Retry,
                InterventionOutcome::Resolved,
                1000,
            ));
        }
        for i in 8..10 {
            events.push(make_event(
                i,
                SignalType::HeartbeatStale,
                InterventionAction::Ignore,
                InterventionOutcome::SelfResolved,
                500,
            ));
        }

        let rel = compute_reliability(&events);
        let hb = rel.get(&SignalType::HeartbeatStale).unwrap();
        assert_eq!(hb.total_fires, 10);
        assert_eq!(hb.true_positives, 8);
        assert_eq!(hb.false_positives, 2);
        assert!((hb.reliability_score - 0.8).abs() < 0.001);
    }

    #[test]
    fn reliability_all_self_resolved() {
        // All self-resolved => reliability 0.0
        let events: Vec<InterventionEvent> = (0..5)
            .map(|i| {
                make_event(
                    i,
                    SignalType::OutputStall,
                    InterventionAction::Ignore,
                    InterventionOutcome::SelfResolved,
                    200,
                )
            })
            .collect();

        let rel = compute_reliability(&events);
        let os = rel.get(&SignalType::OutputStall).unwrap();
        assert_eq!(os.true_positives, 0);
        assert_eq!(os.false_positives, 5);
        assert!((os.reliability_score - 0.0).abs() < 0.001);
    }

    #[test]
    fn reliability_no_classifiable_data() {
        // All timeouts => unknown, score defaults to 0.5
        let events: Vec<InterventionEvent> = (0..3)
            .map(|i| {
                make_event(
                    i,
                    SignalType::ErrorPattern,
                    InterventionAction::Retry,
                    InterventionOutcome::Timeout,
                    3000,
                )
            })
            .collect();

        let rel = compute_reliability(&events);
        let ep = rel.get(&SignalType::ErrorPattern).unwrap();
        assert_eq!(ep.true_positives, 0);
        assert_eq!(ep.false_positives, 0);
        assert_eq!(ep.unknown, 3);
        assert!((ep.reliability_score - 0.5).abs() < 0.001);
    }

    #[test]
    fn reliability_avg_resolution_ms() {
        let events = vec![
            make_event(
                0,
                SignalType::HeartbeatStale,
                InterventionAction::Retry,
                InterventionOutcome::Resolved,
                1000,
            ),
            make_event(
                1,
                SignalType::HeartbeatStale,
                InterventionAction::Retry,
                InterventionOutcome::Resolved,
                3000,
            ),
        ];

        let rel = compute_reliability(&events);
        let hb = rel.get(&SignalType::HeartbeatStale).unwrap();
        assert_eq!(hb.avg_resolution_ms, 2000);
    }

    #[test]
    fn reliability_skips_pending_events() {
        let events = vec![
            make_event(
                0,
                SignalType::HeartbeatStale,
                InterventionAction::Ignore,
                InterventionOutcome::Pending,
                0,
            ),
        ];

        let rel = compute_reliability(&events);
        let hb = rel.get(&SignalType::HeartbeatStale).unwrap();
        assert_eq!(hb.total_fires, 0);
        assert!((hb.reliability_score - 0.5).abs() < 0.001);
    }

    #[test]
    fn effectiveness_basic() {
        let events = vec![
            make_event(0, SignalType::HeartbeatStale, InterventionAction::Retry, InterventionOutcome::Resolved, 1000),
            make_event(1, SignalType::HeartbeatStale, InterventionAction::Retry, InterventionOutcome::Resolved, 1000),
            make_event(2, SignalType::HeartbeatStale, InterventionAction::Retry, InterventionOutcome::StillBroken, 1000),
            make_event(3, SignalType::HeartbeatStale, InterventionAction::Restart, InterventionOutcome::Resolved, 2000),
        ];

        let eff = compute_effectiveness(&events);

        let retry_key = (SignalType::HeartbeatStale, InterventionAction::Retry);
        let retry_eff = eff.get(&retry_key).unwrap();
        assert_eq!(retry_eff.attempts, 3);
        assert_eq!(retry_eff.successes, 2);
        assert_eq!(retry_eff.failures, 1);
        assert!((retry_eff.success_rate - 2.0 / 3.0).abs() < 0.001);

        let restart_key = (SignalType::HeartbeatStale, InterventionAction::Restart);
        let restart_eff = eff.get(&restart_key).unwrap();
        assert_eq!(restart_eff.attempts, 1);
        assert_eq!(restart_eff.successes, 1);
        assert!((restart_eff.success_rate - 1.0).abs() < 0.001);
    }

    #[test]
    fn best_action_picks_highest_success_rate() {
        let events = vec![
            // Retry: 1/3 success
            make_event(0, SignalType::HeartbeatStale, InterventionAction::Retry, InterventionOutcome::Resolved, 1000),
            make_event(1, SignalType::HeartbeatStale, InterventionAction::Retry, InterventionOutcome::StillBroken, 1000),
            make_event(2, SignalType::HeartbeatStale, InterventionAction::Retry, InterventionOutcome::StillBroken, 1000),
            // Restart: 2/3 success
            make_event(3, SignalType::HeartbeatStale, InterventionAction::Restart, InterventionOutcome::Resolved, 2000),
            make_event(4, SignalType::HeartbeatStale, InterventionAction::Restart, InterventionOutcome::Resolved, 2000),
            make_event(5, SignalType::HeartbeatStale, InterventionAction::Restart, InterventionOutcome::StillBroken, 2000),
        ];

        let eff = compute_effectiveness(&events);
        let best = best_action_for_signal(&eff, &SignalType::HeartbeatStale, 2).unwrap();
        assert_eq!(best.action, InterventionAction::Restart);
    }

    #[test]
    fn best_action_respects_min_attempts() {
        let events = vec![
            // Restart: 1/1 success (but only 1 attempt)
            make_event(0, SignalType::HeartbeatStale, InterventionAction::Restart, InterventionOutcome::Resolved, 2000),
            // Retry: 2/3 success (3 attempts)
            make_event(1, SignalType::HeartbeatStale, InterventionAction::Retry, InterventionOutcome::Resolved, 1000),
            make_event(2, SignalType::HeartbeatStale, InterventionAction::Retry, InterventionOutcome::Resolved, 1000),
            make_event(3, SignalType::HeartbeatStale, InterventionAction::Retry, InterventionOutcome::StillBroken, 1000),
        ];

        let eff = compute_effectiveness(&events);

        // With min_attempts=2, Restart is excluded
        let best = best_action_for_signal(&eff, &SignalType::HeartbeatStale, 2).unwrap();
        assert_eq!(best.action, InterventionAction::Retry);
    }

    #[test]
    fn best_action_returns_none_for_unknown_signal() {
        let eff: HashMap<(SignalType, InterventionAction), ActionEffectiveness> =
            HashMap::new();
        let result = best_action_for_signal(&eff, &SignalType::OutputStall, 1);
        assert!(result.is_none());
    }

    #[test]
    fn effectiveness_skips_pending() {
        let events = vec![
            make_event(0, SignalType::HeartbeatStale, InterventionAction::Retry, InterventionOutcome::Pending, 0),
        ];

        let eff = compute_effectiveness(&events);
        assert!(eff.is_empty());
    }

    #[test]
    fn still_broken_is_true_positive() {
        // StillBroken means the signal was right about a problem existing,
        // even though the intervention didn't fix it.
        let events = vec![
            make_event(0, SignalType::SshDisconnected, InterventionAction::Retry, InterventionOutcome::StillBroken, 5000),
        ];

        let rel = compute_reliability(&events);
        let ssh = rel.get(&SignalType::SshDisconnected).unwrap();
        assert_eq!(ssh.true_positives, 1);
        assert_eq!(ssh.false_positives, 0);
        assert!((ssh.reliability_score - 1.0).abs() < 0.001);
    }

    #[test]
    fn empty_events_returns_empty_maps() {
        let rel = compute_reliability(&[]);
        let eff = compute_effectiveness(&[]);
        assert!(rel.is_empty());
        assert!(eff.is_empty());
    }
}
