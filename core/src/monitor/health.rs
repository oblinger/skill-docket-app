//! Health assessor — combines signals into per-agent health assessments.
//!
//! Takes an agent, a collection of health signals, and timing information,
//! then produces a `HealthAssessment` that summarizes the agent's health.
//! Also classifies the failure mode (infrastructure, agent, or strategic)
//! for use by the PM agent's decision logic.

use crate::types::agent::{Agent, HealthState};
use crate::types::health::{HealthAssessment, HealthSignal};

// ---------------------------------------------------------------------------
// Failure mode classification
// ---------------------------------------------------------------------------

/// Classification of a health problem, used by the PM agent to decide
/// the appropriate response (retry, redesign, or escalate).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FailureMode {
    /// Infrastructure issue — SSH down, tmux crashed, etc. Retry is appropriate.
    Infrastructure,
    /// Agent-level issue — the agent itself is producing errors. Redesign task.
    Agent,
    /// Strategic issue — persistent failure despite retries. Escalate to user.
    Strategic,
    /// No failure detected.
    None,
}

// ---------------------------------------------------------------------------
// Health assessment
// ---------------------------------------------------------------------------

/// Assess the health of a single agent given its current signals.
///
/// # Logic
///
/// The "worst signal wins" principle:
/// - `InfrastructureFailed` or `SshDisconnected` -> Unhealthy
/// - `HeartbeatStale` with `age_secs > heartbeat_timeout` -> Unhealthy
/// - `HeartbeatStale` with `age_secs > heartbeat_timeout / 2` -> Degraded
/// - `ErrorPatternDetected` or `ExplicitError` -> Degraded
/// - All signals positive -> Healthy
/// - No signals at all -> Unknown
pub fn assess(
    agent: &Agent,
    signals: &[HealthSignal],
    heartbeat_timeout_secs: u64,
    now_ms: u64,
) -> HealthAssessment {
    if signals.is_empty() {
        return HealthAssessment {
            agent: agent.name.clone(),
            overall: HealthState::Unknown,
            signals: Vec::new(),
            reason: "no signals available".to_string(),
            timestamp_ms: now_ms,
        };
    }

    let mut worst = HealthState::Healthy;
    let mut reason = String::new();

    for signal in signals {
        match signal {
            HealthSignal::InfrastructureFailed { reason: r } => {
                worst = worst_of(worst.clone(), HealthState::Unhealthy);
                reason = format!("infrastructure failed: {}", r);
            }
            HealthSignal::SshDisconnected => {
                worst = worst_of(worst.clone(), HealthState::Unhealthy);
                if reason.is_empty() {
                    reason = "SSH disconnected".to_string();
                }
            }
            HealthSignal::HeartbeatStale { age_secs } => {
                if *age_secs > heartbeat_timeout_secs {
                    worst = worst_of(worst.clone(), HealthState::Unhealthy);
                    reason = format!(
                        "heartbeat stale ({}s > {}s timeout)",
                        age_secs, heartbeat_timeout_secs
                    );
                } else if *age_secs > heartbeat_timeout_secs / 2 {
                    worst = worst_of(worst.clone(), HealthState::Degraded);
                    if reason.is_empty() {
                        reason = format!(
                            "heartbeat aging ({}s > {}s warning threshold)",
                            age_secs,
                            heartbeat_timeout_secs / 2
                        );
                    }
                }
            }
            HealthSignal::ErrorPatternDetected { pattern } => {
                worst = worst_of(worst.clone(), HealthState::Degraded);
                if reason.is_empty() {
                    reason = format!("error pattern detected: {}", pattern);
                }
            }
            HealthSignal::ExplicitError { message } => {
                worst = worst_of(worst.clone(), HealthState::Degraded);
                if reason.is_empty() {
                    reason = format!("explicit error: {}", message);
                }
            }
            HealthSignal::InfrastructureOk
            | HealthSignal::HeartbeatRecent { .. }
            | HealthSignal::SshConnected => {
                // Positive signals don't change worst state.
            }
        }
    }

    if reason.is_empty() {
        reason = "all signals healthy".to_string();
    }

    HealthAssessment {
        agent: agent.name.clone(),
        overall: worst,
        signals: signals.to_vec(),
        reason,
        timestamp_ms: now_ms,
    }
}

/// Classify the failure mode based on a completed health assessment.
///
/// - Unhealthy with infrastructure/SSH signals -> Infrastructure
/// - Degraded with error patterns -> Agent
/// - Unhealthy with stale heartbeat (no infra signal) -> Agent (stalled agent)
/// - Healthy -> None
pub fn classify_failure(assessment: &HealthAssessment) -> FailureMode {
    match &assessment.overall {
        HealthState::Healthy | HealthState::Unknown => FailureMode::None,
        HealthState::Unhealthy => {
            // Check if it's infrastructure-related.
            let has_infra_failure = assessment.signals.iter().any(|s| {
                matches!(
                    s,
                    HealthSignal::InfrastructureFailed { .. } | HealthSignal::SshDisconnected
                )
            });
            if has_infra_failure {
                FailureMode::Infrastructure
            } else {
                // Stale heartbeat without infra issues means the agent itself is stuck.
                FailureMode::Agent
            }
        }
        HealthState::Degraded => {
            let has_error = assessment.signals.iter().any(|s| {
                matches!(
                    s,
                    HealthSignal::ErrorPatternDetected { .. } | HealthSignal::ExplicitError { .. }
                )
            });
            if has_error {
                FailureMode::Agent
            } else {
                FailureMode::None
            }
        }
    }
}

/// Return the worse of two health states.
///
/// Ordering: Healthy < Degraded < Unhealthy (Unknown treated as Healthy for comparison).
fn worst_of(a: HealthState, b: HealthState) -> HealthState {
    let rank = |s: &HealthState| -> u8 {
        match s {
            HealthState::Healthy => 0,
            HealthState::Unknown => 0,
            HealthState::Degraded => 1,
            HealthState::Unhealthy => 2,
        }
    };
    if rank(&a) >= rank(&b) {
        a
    } else {
        b
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::agent::{AgentStatus, AgentType};

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

    #[test]
    fn healthy_all_ok() {
        let agent = make_agent("w1");
        let signals = vec![
            HealthSignal::InfrastructureOk,
            HealthSignal::HeartbeatRecent { age_secs: 5 },
            HealthSignal::SshConnected,
        ];
        let result = assess(&agent, &signals, 60, 1000);
        assert_eq!(result.overall, HealthState::Healthy);
        assert_eq!(result.agent, "w1");
        assert!(result.reason.contains("healthy"));
    }

    #[test]
    fn no_signals_unknown() {
        let agent = make_agent("w1");
        let result = assess(&agent, &[], 60, 1000);
        assert_eq!(result.overall, HealthState::Unknown);
    }

    #[test]
    fn infrastructure_failed_unhealthy() {
        let agent = make_agent("w1");
        let signals = vec![HealthSignal::InfrastructureFailed {
            reason: "tmux crashed".into(),
        }];
        let result = assess(&agent, &signals, 60, 1000);
        assert_eq!(result.overall, HealthState::Unhealthy);
        assert!(result.reason.contains("infrastructure"));
    }

    #[test]
    fn ssh_disconnected_unhealthy() {
        let agent = make_agent("w1");
        let signals = vec![HealthSignal::SshDisconnected];
        let result = assess(&agent, &signals, 60, 1000);
        assert_eq!(result.overall, HealthState::Unhealthy);
        assert!(result.reason.contains("SSH"));
    }

    #[test]
    fn heartbeat_stale_over_timeout_unhealthy() {
        let agent = make_agent("w1");
        let signals = vec![HealthSignal::HeartbeatStale { age_secs: 120 }];
        let result = assess(&agent, &signals, 60, 1000);
        assert_eq!(result.overall, HealthState::Unhealthy);
        assert!(result.reason.contains("stale"));
    }

    #[test]
    fn heartbeat_stale_over_half_timeout_degraded() {
        let agent = make_agent("w1");
        let signals = vec![HealthSignal::HeartbeatStale { age_secs: 35 }];
        let result = assess(&agent, &signals, 60, 1000);
        assert_eq!(result.overall, HealthState::Degraded);
        assert!(result.reason.contains("aging"));
    }

    #[test]
    fn heartbeat_stale_under_half_timeout_healthy() {
        let agent = make_agent("w1");
        let signals = vec![HealthSignal::HeartbeatStale { age_secs: 20 }];
        let result = assess(&agent, &signals, 60, 1000);
        assert_eq!(result.overall, HealthState::Healthy);
    }

    #[test]
    fn error_pattern_degraded() {
        let agent = make_agent("w1");
        let signals = vec![HealthSignal::ErrorPatternDetected {
            pattern: "Traceback".into(),
        }];
        let result = assess(&agent, &signals, 60, 1000);
        assert_eq!(result.overall, HealthState::Degraded);
    }

    #[test]
    fn explicit_error_degraded() {
        let agent = make_agent("w1");
        let signals = vec![HealthSignal::ExplicitError {
            message: "task failed".into(),
        }];
        let result = assess(&agent, &signals, 60, 1000);
        assert_eq!(result.overall, HealthState::Degraded);
    }

    #[test]
    fn worst_signal_wins() {
        let agent = make_agent("w1");
        let signals = vec![
            HealthSignal::HeartbeatRecent { age_secs: 5 },
            HealthSignal::InfrastructureFailed {
                reason: "disk full".into(),
            },
            HealthSignal::ErrorPatternDetected {
                pattern: "Error:".into(),
            },
        ];
        let result = assess(&agent, &signals, 60, 1000);
        assert_eq!(result.overall, HealthState::Unhealthy);
    }

    // -- classify_failure tests --

    #[test]
    fn classify_healthy_is_none() {
        let assessment = HealthAssessment {
            agent: "w1".into(),
            overall: HealthState::Healthy,
            signals: vec![HealthSignal::InfrastructureOk],
            reason: "all good".into(),
            timestamp_ms: 1000,
        };
        assert_eq!(classify_failure(&assessment), FailureMode::None);
    }

    #[test]
    fn classify_infra_failure() {
        let assessment = HealthAssessment {
            agent: "w1".into(),
            overall: HealthState::Unhealthy,
            signals: vec![HealthSignal::SshDisconnected],
            reason: "SSH disconnected".into(),
            timestamp_ms: 1000,
        };
        assert_eq!(classify_failure(&assessment), FailureMode::Infrastructure);
    }

    #[test]
    fn classify_agent_failure_stale() {
        let assessment = HealthAssessment {
            agent: "w1".into(),
            overall: HealthState::Unhealthy,
            signals: vec![HealthSignal::HeartbeatStale { age_secs: 120 }],
            reason: "stale".into(),
            timestamp_ms: 1000,
        };
        assert_eq!(classify_failure(&assessment), FailureMode::Agent);
    }

    #[test]
    fn classify_agent_failure_error_pattern() {
        let assessment = HealthAssessment {
            agent: "w1".into(),
            overall: HealthState::Degraded,
            signals: vec![HealthSignal::ErrorPatternDetected {
                pattern: "Traceback".into(),
            }],
            reason: "error".into(),
            timestamp_ms: 1000,
        };
        assert_eq!(classify_failure(&assessment), FailureMode::Agent);
    }

    #[test]
    fn classify_degraded_no_error_is_none() {
        let assessment = HealthAssessment {
            agent: "w1".into(),
            overall: HealthState::Degraded,
            signals: vec![HealthSignal::HeartbeatStale { age_secs: 35 }],
            reason: "aging".into(),
            timestamp_ms: 1000,
        };
        assert_eq!(classify_failure(&assessment), FailureMode::None);
    }
}
