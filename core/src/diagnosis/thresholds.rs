//! Adaptive threshold adjustment based on signal reliability.
//!
//! High-reliability signals get shorter timeouts (intervene sooner).
//! Low-reliability signals get longer timeouts (wait longer, might self-resolve).

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::events::SignalType;
use super::reliability::SignalReliability;


// ---------------------------------------------------------------------------
// AdaptiveThreshold
// ---------------------------------------------------------------------------

/// A dynamically adjusted threshold for a specific signal.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdaptiveThreshold {
    pub signal: SignalType,
    pub base_timeout_ms: u64,
    pub adjusted_timeout_ms: u64,
    pub reliability_score: f64,
    pub adjustment_reason: String,
}


// ---------------------------------------------------------------------------
// Threshold computation
// ---------------------------------------------------------------------------

/// Compute adaptive thresholds for all signals that have both a base
/// threshold and reliability data.
///
/// Adjustment formula:
/// - reliability >= 0.8 => base * 0.5 (high confidence, intervene quickly)
/// - reliability >= 0.5 => base * 1.0 (moderate, use default)
/// - reliability >= 0.2 => base * 2.0 (low confidence, wait longer)
/// - reliability <  0.2 => base * 3.0 (very unreliable, wait much longer)
pub fn compute_thresholds(
    base_thresholds: &HashMap<SignalType, u64>,
    reliability: &HashMap<SignalType, SignalReliability>,
) -> HashMap<SignalType, AdaptiveThreshold> {
    let mut result = HashMap::new();

    for (signal, &base_ms) in base_thresholds {
        let rel_score = reliability
            .get(signal)
            .map(|r| r.reliability_score)
            .unwrap_or(0.5);

        let (multiplier, reason) = if rel_score >= 0.8 {
            (0.5, format!("high reliability ({:.2}) — intervene quickly", rel_score))
        } else if rel_score >= 0.5 {
            (1.0, format!("moderate reliability ({:.2}) — use default", rel_score))
        } else if rel_score >= 0.2 {
            (2.0, format!("low reliability ({:.2}) — wait longer", rel_score))
        } else {
            (3.0, format!("very low reliability ({:.2}) — wait much longer", rel_score))
        };

        let adjusted_ms = (base_ms as f64 * multiplier) as u64;

        result.insert(
            signal.clone(),
            AdaptiveThreshold {
                signal: signal.clone(),
                base_timeout_ms: base_ms,
                adjusted_timeout_ms: adjusted_ms,
                reliability_score: rel_score,
                adjustment_reason: reason,
            },
        );
    }

    result
}


// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_reliability(signal: SignalType, score: f64) -> SignalReliability {
        SignalReliability {
            signal,
            total_fires: 10,
            true_positives: 0,
            false_positives: 0,
            unknown: 0,
            reliability_score: score,
            avg_resolution_ms: 1000,
        }
    }

    #[test]
    fn high_reliability_shortens_timeout() {
        let mut base = HashMap::new();
        base.insert(SignalType::HeartbeatStale, 60_000);

        let mut rel = HashMap::new();
        rel.insert(
            SignalType::HeartbeatStale,
            make_reliability(SignalType::HeartbeatStale, 0.9),
        );

        let thresholds = compute_thresholds(&base, &rel);
        let t = thresholds.get(&SignalType::HeartbeatStale).unwrap();
        assert_eq!(t.adjusted_timeout_ms, 30_000); // 60000 * 0.5
        assert!(t.adjustment_reason.contains("high reliability"));
    }

    #[test]
    fn moderate_reliability_keeps_default() {
        let mut base = HashMap::new();
        base.insert(SignalType::ErrorPattern, 60_000);

        let mut rel = HashMap::new();
        rel.insert(
            SignalType::ErrorPattern,
            make_reliability(SignalType::ErrorPattern, 0.6),
        );

        let thresholds = compute_thresholds(&base, &rel);
        let t = thresholds.get(&SignalType::ErrorPattern).unwrap();
        assert_eq!(t.adjusted_timeout_ms, 60_000); // 60000 * 1.0
    }

    #[test]
    fn low_reliability_doubles_timeout() {
        let mut base = HashMap::new();
        base.insert(SignalType::OutputStall, 30_000);

        let mut rel = HashMap::new();
        rel.insert(
            SignalType::OutputStall,
            make_reliability(SignalType::OutputStall, 0.3),
        );

        let thresholds = compute_thresholds(&base, &rel);
        let t = thresholds.get(&SignalType::OutputStall).unwrap();
        assert_eq!(t.adjusted_timeout_ms, 60_000); // 30000 * 2.0
    }

    #[test]
    fn very_low_reliability_triples_timeout() {
        let mut base = HashMap::new();
        base.insert(SignalType::SshDisconnected, 10_000);

        let mut rel = HashMap::new();
        rel.insert(
            SignalType::SshDisconnected,
            make_reliability(SignalType::SshDisconnected, 0.1),
        );

        let thresholds = compute_thresholds(&base, &rel);
        let t = thresholds.get(&SignalType::SshDisconnected).unwrap();
        assert_eq!(t.adjusted_timeout_ms, 30_000); // 10000 * 3.0
    }

    #[test]
    fn missing_reliability_defaults_to_moderate() {
        let mut base = HashMap::new();
        base.insert(SignalType::ExplicitError, 20_000);

        let rel = HashMap::new(); // No reliability data

        let thresholds = compute_thresholds(&base, &rel);
        let t = thresholds.get(&SignalType::ExplicitError).unwrap();
        // Default reliability is 0.5, which is >= 0.5, so multiplier = 1.0
        assert_eq!(t.adjusted_timeout_ms, 20_000);
        assert!((t.reliability_score - 0.5).abs() < 0.001);
    }

    #[test]
    fn boundary_at_exactly_0_8() {
        let mut base = HashMap::new();
        base.insert(SignalType::HeartbeatStale, 100_000);

        let mut rel = HashMap::new();
        rel.insert(
            SignalType::HeartbeatStale,
            make_reliability(SignalType::HeartbeatStale, 0.8),
        );

        let thresholds = compute_thresholds(&base, &rel);
        let t = thresholds.get(&SignalType::HeartbeatStale).unwrap();
        assert_eq!(t.adjusted_timeout_ms, 50_000); // 0.8 >= 0.8, so * 0.5
    }

    #[test]
    fn boundary_at_exactly_0_5() {
        let mut base = HashMap::new();
        base.insert(SignalType::HeartbeatStale, 100_000);

        let mut rel = HashMap::new();
        rel.insert(
            SignalType::HeartbeatStale,
            make_reliability(SignalType::HeartbeatStale, 0.5),
        );

        let thresholds = compute_thresholds(&base, &rel);
        let t = thresholds.get(&SignalType::HeartbeatStale).unwrap();
        assert_eq!(t.adjusted_timeout_ms, 100_000); // 0.5 >= 0.5, so * 1.0
    }

    #[test]
    fn boundary_at_exactly_0_2() {
        let mut base = HashMap::new();
        base.insert(SignalType::HeartbeatStale, 100_000);

        let mut rel = HashMap::new();
        rel.insert(
            SignalType::HeartbeatStale,
            make_reliability(SignalType::HeartbeatStale, 0.2),
        );

        let thresholds = compute_thresholds(&base, &rel);
        let t = thresholds.get(&SignalType::HeartbeatStale).unwrap();
        assert_eq!(t.adjusted_timeout_ms, 200_000); // 0.2 >= 0.2, so * 2.0
    }

    #[test]
    fn empty_base_thresholds() {
        let base = HashMap::new();
        let rel = HashMap::new();
        let thresholds = compute_thresholds(&base, &rel);
        assert!(thresholds.is_empty());
    }

    #[test]
    fn multiple_signals() {
        let mut base = HashMap::new();
        base.insert(SignalType::HeartbeatStale, 60_000);
        base.insert(SignalType::OutputStall, 30_000);

        let mut rel = HashMap::new();
        rel.insert(
            SignalType::HeartbeatStale,
            make_reliability(SignalType::HeartbeatStale, 0.9),
        );
        rel.insert(
            SignalType::OutputStall,
            make_reliability(SignalType::OutputStall, 0.1),
        );

        let thresholds = compute_thresholds(&base, &rel);
        assert_eq!(thresholds.len(), 2);
        assert_eq!(
            thresholds.get(&SignalType::HeartbeatStale).unwrap().adjusted_timeout_ms,
            30_000
        );
        assert_eq!(
            thresholds.get(&SignalType::OutputStall).unwrap().adjusted_timeout_ms,
            90_000
        );
    }
}
