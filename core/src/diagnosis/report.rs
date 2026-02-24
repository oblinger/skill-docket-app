//! Markdown report generation for operational statistics.
//!
//! Produces a human-readable summary of signal reliability, intervention
//! effectiveness, threshold adjustments, and actionable recommendations.

use std::collections::HashMap;

use super::events::{InterventionAction, InterventionEvent, SignalType};
use super::reliability::{ActionEffectiveness, SignalReliability};
use super::thresholds::AdaptiveThreshold;


/// Generate a complete markdown diagnostic report.
pub fn generate_report(
    events: &[InterventionEvent],
    reliability: &HashMap<SignalType, SignalReliability>,
    effectiveness: &HashMap<(SignalType, InterventionAction), ActionEffectiveness>,
    thresholds: &HashMap<SignalType, AdaptiveThreshold>,
) -> String {
    let mut out = String::new();

    // --- Summary ---
    out.push_str("# Diagnosis Report\n\n");
    out.push_str("## Summary\n\n");

    if events.is_empty() {
        out.push_str("No intervention events recorded.\n\n");
        return out;
    }

    let total = events.len();
    let min_ts = events.iter().map(|e| e.timestamp_ms).min().unwrap_or(0);
    let max_ts = events.iter().map(|e| e.timestamp_ms).max().unwrap_or(0);

    let resolved = events
        .iter()
        .filter(|e| {
            e.outcome == super::events::InterventionOutcome::Resolved
        })
        .count();
    let success_rate = if total > 0 {
        resolved as f64 / total as f64
    } else {
        0.0
    };

    out.push_str(&format!("- **Total events:** {}\n", total));
    out.push_str(&format!(
        "- **Time range:** {} ms to {} ms\n",
        min_ts, max_ts
    ));
    out.push_str(&format!(
        "- **Overall success rate:** {:.1}%\n",
        success_rate * 100.0
    ));
    out.push('\n');

    // --- Signal Reliability Table ---
    out.push_str("## Signal Reliability\n\n");

    let mut rel_entries: Vec<&SignalReliability> = reliability.values().collect();
    rel_entries.sort_by(|a, b| {
        a.reliability_score
            .partial_cmp(&b.reliability_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    if rel_entries.is_empty() {
        out.push_str("No signal reliability data.\n\n");
    } else {
        out.push_str(
            "| Signal | Fires | True+ | False+ | Unknown | Reliability | Avg Resolution |\n",
        );
        out.push_str(
            "|--------|-------|-------|--------|---------|-------------|----------------|\n",
        );
        for r in &rel_entries {
            out.push_str(&format!(
                "| {} | {} | {} | {} | {} | {:.2} | {}ms |\n",
                r.signal,
                r.total_fires,
                r.true_positives,
                r.false_positives,
                r.unknown,
                r.reliability_score,
                r.avg_resolution_ms,
            ));
        }
        out.push('\n');
    }

    // --- Intervention Effectiveness Table ---
    out.push_str("## Intervention Effectiveness\n\n");

    let mut eff_entries: Vec<&ActionEffectiveness> = effectiveness.values().collect();
    eff_entries.sort_by(|a, b| {
        b.success_rate
            .partial_cmp(&a.success_rate)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    if eff_entries.is_empty() {
        out.push_str("No intervention effectiveness data.\n\n");
    } else {
        out.push_str(
            "| Signal | Action | Attempts | Successes | Failures | Success Rate |\n",
        );
        out.push_str(
            "|--------|--------|----------|-----------|----------|-------------|\n",
        );
        for e in &eff_entries {
            out.push_str(&format!(
                "| {} | {} | {} | {} | {} | {:.1}% |\n",
                e.signal,
                e.action,
                e.attempts,
                e.successes,
                e.failures,
                e.success_rate * 100.0,
            ));
        }
        out.push('\n');
    }

    // --- Threshold Adjustments ---
    out.push_str("## Threshold Adjustments\n\n");

    if thresholds.is_empty() {
        out.push_str("No threshold adjustments configured.\n\n");
    } else {
        out.push_str(
            "| Signal | Base | Adjusted | Reliability | Reason |\n",
        );
        out.push_str(
            "|--------|------|----------|-------------|--------|\n",
        );
        let mut thresh_entries: Vec<&AdaptiveThreshold> = thresholds.values().collect();
        thresh_entries.sort_by(|a, b| {
            a.reliability_score
                .partial_cmp(&b.reliability_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        for t in &thresh_entries {
            out.push_str(&format!(
                "| {} | {}ms | {}ms | {:.2} | {} |\n",
                t.signal,
                t.base_timeout_ms,
                t.adjusted_timeout_ms,
                t.reliability_score,
                t.adjustment_reason,
            ));
        }
        out.push('\n');
    }

    // --- Recommendations ---
    out.push_str("## Recommendations\n\n");

    let mut has_recommendations = false;

    // Signals with reliability < 0.2
    for r in &rel_entries {
        if r.reliability_score < 0.2 && r.total_fires > 0 {
            out.push_str(&format!(
                "- **{}** — reliability {:.2}, consider removing or increasing threshold\n",
                r.signal, r.reliability_score,
            ));
            has_recommendations = true;
        }
    }

    // Actions with success rate < 0.1
    for e in &eff_entries {
        if e.success_rate < 0.1 && e.attempts > 0 {
            out.push_str(&format!(
                "- **{} + {}** — success rate {:.1}%, this intervention rarely works for this signal\n",
                e.signal, e.action, e.success_rate * 100.0,
            ));
            has_recommendations = true;
        }
    }

    // Signals with high fire count and high reliability
    for r in &rel_entries {
        if r.reliability_score >= 0.8 && r.total_fires >= 10 {
            out.push_str(&format!(
                "- **{}** — reliable indicator ({:.2}, {} fires), consider faster response\n",
                r.signal, r.reliability_score, r.total_fires,
            ));
            has_recommendations = true;
        }
    }

    if !has_recommendations {
        out.push_str("No actionable recommendations at this time.\n");
    }

    out.push('\n');
    out
}


// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnosis::events::{
        InterventionAction, InterventionEvent, InterventionOutcome, SignalType,
    };

    fn make_event(
        id: u64,
        signal: SignalType,
        action: InterventionAction,
        outcome: InterventionOutcome,
    ) -> InterventionEvent {
        InterventionEvent {
            id,
            timestamp_ms: 1000 + id * 100,
            agent: "w1".to_string(),
            signal,
            signal_detail: "test".to_string(),
            action,
            outcome,
            outcome_detail: "test".to_string(),
            duration_ms: 1000,
            failure_mode: "none".to_string(),
        }
    }

    #[test]
    fn report_empty_events() {
        let report = generate_report(
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
        );
        assert!(report.contains("# Diagnosis Report"));
        assert!(report.contains("No intervention events recorded"));
    }

    #[test]
    fn report_contains_signal_reliability_table() {
        let events = vec![
            make_event(0, SignalType::HeartbeatStale, InterventionAction::Retry, InterventionOutcome::Resolved),
        ];

        let mut rel = HashMap::new();
        rel.insert(
            SignalType::HeartbeatStale,
            crate::diagnosis::reliability::SignalReliability {
                signal: SignalType::HeartbeatStale,
                total_fires: 1,
                true_positives: 1,
                false_positives: 0,
                unknown: 0,
                reliability_score: 1.0,
                avg_resolution_ms: 1000,
            },
        );

        let report = generate_report(&events, &rel, &HashMap::new(), &HashMap::new());
        assert!(report.contains("## Signal Reliability"));
        assert!(report.contains("heartbeat_stale"));
        assert!(report.contains("1.00"));
    }

    #[test]
    fn report_contains_effectiveness_table() {
        let events = vec![
            make_event(0, SignalType::HeartbeatStale, InterventionAction::Retry, InterventionOutcome::Resolved),
        ];

        let mut eff = HashMap::new();
        eff.insert(
            (SignalType::HeartbeatStale, InterventionAction::Retry),
            ActionEffectiveness {
                signal: SignalType::HeartbeatStale,
                action: InterventionAction::Retry,
                attempts: 1,
                successes: 1,
                failures: 0,
                success_rate: 1.0,
            },
        );

        let report = generate_report(&events, &HashMap::new(), &eff, &HashMap::new());
        assert!(report.contains("## Intervention Effectiveness"));
        assert!(report.contains("retry"));
        assert!(report.contains("100.0%"));
    }

    #[test]
    fn report_contains_recommendations_for_unreliable_signal() {
        let events = vec![
            make_event(0, SignalType::OutputStall, InterventionAction::Ignore, InterventionOutcome::SelfResolved),
        ];

        let mut rel = HashMap::new();
        rel.insert(
            SignalType::OutputStall,
            crate::diagnosis::reliability::SignalReliability {
                signal: SignalType::OutputStall,
                total_fires: 5,
                true_positives: 0,
                false_positives: 5,
                unknown: 0,
                reliability_score: 0.0,
                avg_resolution_ms: 0,
            },
        );

        let report = generate_report(&events, &rel, &HashMap::new(), &HashMap::new());
        assert!(report.contains("## Recommendations"));
        assert!(report.contains("consider removing or increasing threshold"));
    }

    #[test]
    fn report_contains_recommendations_for_reliable_signal() {
        let events = vec![
            make_event(0, SignalType::HeartbeatStale, InterventionAction::Retry, InterventionOutcome::Resolved),
        ];

        let mut rel = HashMap::new();
        rel.insert(
            SignalType::HeartbeatStale,
            crate::diagnosis::reliability::SignalReliability {
                signal: SignalType::HeartbeatStale,
                total_fires: 15,
                true_positives: 14,
                false_positives: 1,
                unknown: 0,
                reliability_score: 0.93,
                avg_resolution_ms: 2000,
            },
        );

        let report = generate_report(&events, &rel, &HashMap::new(), &HashMap::new());
        assert!(report.contains("reliable indicator"));
        assert!(report.contains("consider faster response"));
    }

    #[test]
    fn report_contains_threshold_adjustments() {
        let events = vec![
            make_event(0, SignalType::HeartbeatStale, InterventionAction::Retry, InterventionOutcome::Resolved),
        ];

        let mut thresholds = HashMap::new();
        thresholds.insert(
            SignalType::HeartbeatStale,
            crate::diagnosis::thresholds::AdaptiveThreshold {
                signal: SignalType::HeartbeatStale,
                base_timeout_ms: 60_000,
                adjusted_timeout_ms: 30_000,
                reliability_score: 0.9,
                adjustment_reason: "high reliability (0.90) — intervene quickly".to_string(),
            },
        );

        let report = generate_report(&events, &HashMap::new(), &HashMap::new(), &thresholds);
        assert!(report.contains("## Threshold Adjustments"));
        assert!(report.contains("60000ms"));
        assert!(report.contains("30000ms"));
    }

    #[test]
    fn report_summary_counts() {
        let events = vec![
            make_event(0, SignalType::HeartbeatStale, InterventionAction::Retry, InterventionOutcome::Resolved),
            make_event(1, SignalType::HeartbeatStale, InterventionAction::Retry, InterventionOutcome::StillBroken),
            make_event(2, SignalType::OutputStall, InterventionAction::Ignore, InterventionOutcome::SelfResolved),
        ];

        let report = generate_report(&events, &HashMap::new(), &HashMap::new(), &HashMap::new());
        assert!(report.contains("**Total events:** 3"));
        assert!(report.contains("33.3%")); // 1/3 resolved
    }

    #[test]
    fn report_recommendation_for_low_success_action() {
        let events = vec![
            make_event(0, SignalType::HeartbeatStale, InterventionAction::Retry, InterventionOutcome::StillBroken),
        ];

        let mut eff = HashMap::new();
        eff.insert(
            (SignalType::HeartbeatStale, InterventionAction::Retry),
            ActionEffectiveness {
                signal: SignalType::HeartbeatStale,
                action: InterventionAction::Retry,
                attempts: 10,
                successes: 0,
                failures: 10,
                success_rate: 0.0,
            },
        );

        let report = generate_report(&events, &HashMap::new(), &eff, &HashMap::new());
        assert!(report.contains("this intervention rarely works"));
    }
}
