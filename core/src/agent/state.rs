use serde::{Deserialize, Serialize};

/// Agent lifecycle state.
///
/// The state machine enforces valid transitions:
///
/// ```text
/// Spawning -> Ready -> Busy -> Idle -> (Busy | Stopping)
///                       |
///                    Stalled -> Recovering -> (Ready | Dead)
///                       |
///                      Dead
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum AgentState {
    Spawning,
    Ready,
    Busy { task_id: String },
    Idle,
    Stalled { since_ms: u64, reason: String },
    Recovering { attempt: u32 },
    Stopping,
    Dead { reason: String },
}

/// Events that trigger state transitions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "transition", rename_all = "snake_case")]
pub enum Transition {
    SpawnComplete,
    TaskAssigned { task_id: String },
    TaskCompleted,
    HeartbeatTimeout { age_ms: u64 },
    ErrorDetected { message: String },
    RecoveryStarted,
    RecoverySucceeded,
    RecoveryFailed { message: String },
    StopRequested,
    Killed,
}

impl AgentState {
    /// Apply a transition, returning the new state or an error if the
    /// transition is not valid from the current state.
    pub fn apply(&self, transition: Transition) -> Result<AgentState, String> {
        match (self, &transition) {
            // --- Spawning ---
            (AgentState::Spawning, Transition::SpawnComplete) => Ok(AgentState::Ready),
            (AgentState::Spawning, Transition::ErrorDetected { message }) => {
                Ok(AgentState::Dead {
                    reason: format!("spawn failed: {}", message),
                })
            }
            (AgentState::Spawning, Transition::Killed) => Ok(AgentState::Dead {
                reason: "killed during spawn".into(),
            }),

            // --- Ready ---
            (AgentState::Ready, Transition::TaskAssigned { task_id }) => {
                Ok(AgentState::Busy {
                    task_id: task_id.clone(),
                })
            }
            (AgentState::Ready, Transition::StopRequested) => Ok(AgentState::Stopping),
            (AgentState::Ready, Transition::Killed) => Ok(AgentState::Dead {
                reason: "killed while ready".into(),
            }),
            (AgentState::Ready, Transition::HeartbeatTimeout { age_ms }) => {
                Ok(AgentState::Stalled {
                    since_ms: *age_ms,
                    reason: format!("heartbeat timeout after {}ms", age_ms),
                })
            }

            // --- Busy ---
            (AgentState::Busy { .. }, Transition::TaskCompleted) => Ok(AgentState::Idle),
            (AgentState::Busy { .. }, Transition::HeartbeatTimeout { age_ms }) => {
                Ok(AgentState::Stalled {
                    since_ms: *age_ms,
                    reason: format!("heartbeat timeout after {}ms while busy", age_ms),
                })
            }
            (AgentState::Busy { .. }, Transition::ErrorDetected { message }) => {
                Ok(AgentState::Stalled {
                    since_ms: 0,
                    reason: format!("error detected: {}", message),
                })
            }
            (AgentState::Busy { .. }, Transition::StopRequested) => Ok(AgentState::Stopping),
            (AgentState::Busy { .. }, Transition::Killed) => Ok(AgentState::Dead {
                reason: "killed while busy".into(),
            }),

            // --- Idle ---
            (AgentState::Idle, Transition::TaskAssigned { task_id }) => {
                Ok(AgentState::Busy {
                    task_id: task_id.clone(),
                })
            }
            (AgentState::Idle, Transition::StopRequested) => Ok(AgentState::Stopping),
            (AgentState::Idle, Transition::Killed) => Ok(AgentState::Dead {
                reason: "killed while idle".into(),
            }),
            (AgentState::Idle, Transition::HeartbeatTimeout { age_ms }) => {
                Ok(AgentState::Stalled {
                    since_ms: *age_ms,
                    reason: format!("heartbeat timeout after {}ms while idle", age_ms),
                })
            }

            // --- Stalled ---
            (AgentState::Stalled { .. }, Transition::RecoveryStarted) => {
                Ok(AgentState::Recovering { attempt: 1 })
            }
            (AgentState::Stalled { .. }, Transition::Killed) => Ok(AgentState::Dead {
                reason: "killed while stalled".into(),
            }),
            (AgentState::Stalled { reason, .. }, Transition::StopRequested) => {
                Ok(AgentState::Dead {
                    reason: format!("stopped while stalled: {}", reason),
                })
            }

            // --- Recovering ---
            (AgentState::Recovering { .. }, Transition::RecoverySucceeded) => {
                Ok(AgentState::Ready)
            }
            (AgentState::Recovering { attempt, .. }, Transition::RecoveryFailed { message }) => {
                Ok(AgentState::Dead {
                    reason: format!(
                        "recovery failed after {} attempt(s): {}",
                        attempt, message
                    ),
                })
            }
            (AgentState::Recovering { attempt }, Transition::RecoveryStarted) => {
                Ok(AgentState::Recovering {
                    attempt: attempt + 1,
                })
            }
            (AgentState::Recovering { .. }, Transition::Killed) => Ok(AgentState::Dead {
                reason: "killed during recovery".into(),
            }),

            // --- Stopping ---
            (AgentState::Stopping, Transition::Killed) => Ok(AgentState::Dead {
                reason: "killed after stop requested".into(),
            }),
            (AgentState::Stopping, Transition::StopRequested) => {
                // Idempotent: already stopping
                Ok(AgentState::Stopping)
            }

            // --- Dead ---
            (AgentState::Dead { .. }, _) => Err(format!(
                "agent is dead; cannot apply transition {:?}",
                transition
            )),

            // --- Catch-all for invalid transitions ---
            _ => Err(format!(
                "invalid transition {:?} from state {:?}",
                transition, self
            )),
        }
    }

    /// Returns true if the agent is in a terminal state (Dead).
    pub fn is_terminal(&self) -> bool {
        matches!(self, AgentState::Dead { .. })
    }

    /// Returns true if the agent can potentially do work (Ready or Idle).
    pub fn is_available(&self) -> bool {
        matches!(self, AgentState::Ready | AgentState::Idle)
    }

    /// Returns true if the agent can accept a new task assignment right now.
    pub fn can_accept_task(&self) -> bool {
        matches!(self, AgentState::Ready | AgentState::Idle)
    }

    /// Human-readable label for the current state.
    pub fn label(&self) -> &str {
        match self {
            AgentState::Spawning => "spawning",
            AgentState::Ready => "ready",
            AgentState::Busy { .. } => "busy",
            AgentState::Idle => "idle",
            AgentState::Stalled { .. } => "stalled",
            AgentState::Recovering { .. } => "recovering",
            AgentState::Stopping => "stopping",
            AgentState::Dead { .. } => "dead",
        }
    }
}

impl std::fmt::Display for AgentState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AgentState::Spawning => write!(f, "Spawning"),
            AgentState::Ready => write!(f, "Ready"),
            AgentState::Busy { task_id } => write!(f, "Busy({})", task_id),
            AgentState::Idle => write!(f, "Idle"),
            AgentState::Stalled { since_ms, reason } => {
                write!(f, "Stalled({}ms: {})", since_ms, reason)
            }
            AgentState::Recovering { attempt } => write!(f, "Recovering(attempt={})", attempt),
            AgentState::Stopping => write!(f, "Stopping"),
            AgentState::Dead { reason } => write!(f, "Dead({})", reason),
        }
    }
}

impl std::fmt::Display for Transition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Transition::SpawnComplete => write!(f, "SpawnComplete"),
            Transition::TaskAssigned { task_id } => write!(f, "TaskAssigned({})", task_id),
            Transition::TaskCompleted => write!(f, "TaskCompleted"),
            Transition::HeartbeatTimeout { age_ms } => {
                write!(f, "HeartbeatTimeout({}ms)", age_ms)
            }
            Transition::ErrorDetected { message } => write!(f, "ErrorDetected({})", message),
            Transition::RecoveryStarted => write!(f, "RecoveryStarted"),
            Transition::RecoverySucceeded => write!(f, "RecoverySucceeded"),
            Transition::RecoveryFailed { message } => write!(f, "RecoveryFailed({})", message),
            Transition::StopRequested => write!(f, "StopRequested"),
            Transition::Killed => write!(f, "Killed"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- Spawning transitions ----

    #[test]
    fn spawning_to_ready() {
        let state = AgentState::Spawning;
        let next = state.apply(Transition::SpawnComplete).unwrap();
        assert_eq!(next, AgentState::Ready);
    }

    #[test]
    fn spawning_error_to_dead() {
        let state = AgentState::Spawning;
        let next = state
            .apply(Transition::ErrorDetected {
                message: "no tmux".into(),
            })
            .unwrap();
        assert!(next.is_terminal());
        if let AgentState::Dead { reason } = &next {
            assert!(reason.contains("spawn failed"));
            assert!(reason.contains("no tmux"));
        } else {
            panic!("expected Dead");
        }
    }

    #[test]
    fn spawning_killed_to_dead() {
        let state = AgentState::Spawning;
        let next = state.apply(Transition::Killed).unwrap();
        assert!(next.is_terminal());
    }

    #[test]
    fn spawning_rejects_task_assigned() {
        let state = AgentState::Spawning;
        let result = state.apply(Transition::TaskAssigned {
            task_id: "T1".into(),
        });
        assert!(result.is_err());
    }

    #[test]
    fn spawning_rejects_task_completed() {
        let state = AgentState::Spawning;
        let result = state.apply(Transition::TaskCompleted);
        assert!(result.is_err());
    }

    // ---- Ready transitions ----

    #[test]
    fn ready_to_busy() {
        let state = AgentState::Ready;
        let next = state
            .apply(Transition::TaskAssigned {
                task_id: "CMX1".into(),
            })
            .unwrap();
        assert_eq!(
            next,
            AgentState::Busy {
                task_id: "CMX1".into()
            }
        );
    }

    #[test]
    fn ready_to_stopping() {
        let state = AgentState::Ready;
        let next = state.apply(Transition::StopRequested).unwrap();
        assert_eq!(next, AgentState::Stopping);
    }

    #[test]
    fn ready_to_dead_on_kill() {
        let state = AgentState::Ready;
        let next = state.apply(Transition::Killed).unwrap();
        assert!(next.is_terminal());
    }

    #[test]
    fn ready_to_stalled_on_heartbeat_timeout() {
        let state = AgentState::Ready;
        let next = state
            .apply(Transition::HeartbeatTimeout { age_ms: 60000 })
            .unwrap();
        if let AgentState::Stalled { since_ms, reason } = &next {
            assert_eq!(*since_ms, 60000);
            assert!(reason.contains("heartbeat timeout"));
        } else {
            panic!("expected Stalled, got {:?}", next);
        }
    }

    #[test]
    fn ready_rejects_spawn_complete() {
        let state = AgentState::Ready;
        let result = state.apply(Transition::SpawnComplete);
        assert!(result.is_err());
    }

    #[test]
    fn ready_rejects_task_completed() {
        let state = AgentState::Ready;
        let result = state.apply(Transition::TaskCompleted);
        assert!(result.is_err());
    }

    // ---- Busy transitions ----

    #[test]
    fn busy_to_idle() {
        let state = AgentState::Busy {
            task_id: "CMX1".into(),
        };
        let next = state.apply(Transition::TaskCompleted).unwrap();
        assert_eq!(next, AgentState::Idle);
    }

    #[test]
    fn busy_to_stalled_on_timeout() {
        let state = AgentState::Busy {
            task_id: "CMX1".into(),
        };
        let next = state
            .apply(Transition::HeartbeatTimeout { age_ms: 45000 })
            .unwrap();
        if let AgentState::Stalled { since_ms, .. } = &next {
            assert_eq!(*since_ms, 45000);
        } else {
            panic!("expected Stalled");
        }
    }

    #[test]
    fn busy_to_stalled_on_error() {
        let state = AgentState::Busy {
            task_id: "CMX1".into(),
        };
        let next = state
            .apply(Transition::ErrorDetected {
                message: "segfault".into(),
            })
            .unwrap();
        if let AgentState::Stalled { reason, .. } = &next {
            assert!(reason.contains("error detected"));
            assert!(reason.contains("segfault"));
        } else {
            panic!("expected Stalled");
        }
    }

    #[test]
    fn busy_to_stopping() {
        let state = AgentState::Busy {
            task_id: "CMX1".into(),
        };
        let next = state.apply(Transition::StopRequested).unwrap();
        assert_eq!(next, AgentState::Stopping);
    }

    #[test]
    fn busy_killed_to_dead() {
        let state = AgentState::Busy {
            task_id: "CMX1".into(),
        };
        let next = state.apply(Transition::Killed).unwrap();
        assert!(next.is_terminal());
    }

    #[test]
    fn busy_rejects_task_assigned() {
        let state = AgentState::Busy {
            task_id: "CMX1".into(),
        };
        let result = state.apply(Transition::TaskAssigned {
            task_id: "CMX2".into(),
        });
        assert!(result.is_err());
    }

    // ---- Idle transitions ----

    #[test]
    fn idle_to_busy() {
        let state = AgentState::Idle;
        let next = state
            .apply(Transition::TaskAssigned {
                task_id: "CMX2".into(),
            })
            .unwrap();
        assert_eq!(
            next,
            AgentState::Busy {
                task_id: "CMX2".into()
            }
        );
    }

    #[test]
    fn idle_to_stopping() {
        let state = AgentState::Idle;
        let next = state.apply(Transition::StopRequested).unwrap();
        assert_eq!(next, AgentState::Stopping);
    }

    #[test]
    fn idle_killed_to_dead() {
        let state = AgentState::Idle;
        let next = state.apply(Transition::Killed).unwrap();
        assert!(next.is_terminal());
    }

    #[test]
    fn idle_to_stalled_on_heartbeat_timeout() {
        let state = AgentState::Idle;
        let next = state
            .apply(Transition::HeartbeatTimeout { age_ms: 90000 })
            .unwrap();
        if let AgentState::Stalled { since_ms, .. } = &next {
            assert_eq!(*since_ms, 90000);
        } else {
            panic!("expected Stalled");
        }
    }

    // ---- Stalled transitions ----

    #[test]
    fn stalled_to_recovering() {
        let state = AgentState::Stalled {
            since_ms: 30000,
            reason: "heartbeat lost".into(),
        };
        let next = state.apply(Transition::RecoveryStarted).unwrap();
        assert_eq!(next, AgentState::Recovering { attempt: 1 });
    }

    #[test]
    fn stalled_killed_to_dead() {
        let state = AgentState::Stalled {
            since_ms: 30000,
            reason: "heartbeat lost".into(),
        };
        let next = state.apply(Transition::Killed).unwrap();
        assert!(next.is_terminal());
    }

    #[test]
    fn stalled_stop_to_dead() {
        let state = AgentState::Stalled {
            since_ms: 30000,
            reason: "heartbeat lost".into(),
        };
        let next = state.apply(Transition::StopRequested).unwrap();
        assert!(next.is_terminal());
    }

    #[test]
    fn stalled_rejects_task_assigned() {
        let state = AgentState::Stalled {
            since_ms: 30000,
            reason: "heartbeat lost".into(),
        };
        let result = state.apply(Transition::TaskAssigned {
            task_id: "T1".into(),
        });
        assert!(result.is_err());
    }

    // ---- Recovering transitions ----

    #[test]
    fn recovering_to_ready() {
        let state = AgentState::Recovering { attempt: 1 };
        let next = state.apply(Transition::RecoverySucceeded).unwrap();
        assert_eq!(next, AgentState::Ready);
    }

    #[test]
    fn recovering_to_dead_on_failure() {
        let state = AgentState::Recovering { attempt: 3 };
        let next = state
            .apply(Transition::RecoveryFailed {
                message: "tmux session gone".into(),
            })
            .unwrap();
        assert!(next.is_terminal());
        if let AgentState::Dead { reason } = &next {
            assert!(reason.contains("3 attempt(s)"));
            assert!(reason.contains("tmux session gone"));
        } else {
            panic!("expected Dead");
        }
    }

    #[test]
    fn recovering_retry_increments_attempt() {
        let state = AgentState::Recovering { attempt: 1 };
        let next = state.apply(Transition::RecoveryStarted).unwrap();
        assert_eq!(next, AgentState::Recovering { attempt: 2 });
    }

    #[test]
    fn recovering_killed_to_dead() {
        let state = AgentState::Recovering { attempt: 2 };
        let next = state.apply(Transition::Killed).unwrap();
        assert!(next.is_terminal());
    }

    // ---- Stopping transitions ----

    #[test]
    fn stopping_killed_to_dead() {
        let state = AgentState::Stopping;
        let next = state.apply(Transition::Killed).unwrap();
        assert!(next.is_terminal());
    }

    #[test]
    fn stopping_stop_is_idempotent() {
        let state = AgentState::Stopping;
        let next = state.apply(Transition::StopRequested).unwrap();
        assert_eq!(next, AgentState::Stopping);
    }

    #[test]
    fn stopping_rejects_task_assigned() {
        let state = AgentState::Stopping;
        let result = state.apply(Transition::TaskAssigned {
            task_id: "T1".into(),
        });
        assert!(result.is_err());
    }

    // ---- Dead transitions ----

    #[test]
    fn dead_rejects_all_transitions() {
        let state = AgentState::Dead {
            reason: "test".into(),
        };
        assert!(state.apply(Transition::SpawnComplete).is_err());
        assert!(state
            .apply(Transition::TaskAssigned {
                task_id: "T1".into()
            })
            .is_err());
        assert!(state.apply(Transition::TaskCompleted).is_err());
        assert!(state
            .apply(Transition::HeartbeatTimeout { age_ms: 1000 })
            .is_err());
        assert!(state.apply(Transition::RecoveryStarted).is_err());
        assert!(state.apply(Transition::RecoverySucceeded).is_err());
        assert!(state
            .apply(Transition::RecoveryFailed {
                message: "x".into()
            })
            .is_err());
        assert!(state.apply(Transition::StopRequested).is_err());
        assert!(state.apply(Transition::Killed).is_err());
    }

    // ---- Property queries ----

    #[test]
    fn is_terminal_only_for_dead() {
        assert!(!AgentState::Spawning.is_terminal());
        assert!(!AgentState::Ready.is_terminal());
        assert!(!AgentState::Busy {
            task_id: "T1".into()
        }
        .is_terminal());
        assert!(!AgentState::Idle.is_terminal());
        assert!(!AgentState::Stalled {
            since_ms: 0,
            reason: "x".into()
        }
        .is_terminal());
        assert!(!AgentState::Recovering { attempt: 1 }.is_terminal());
        assert!(!AgentState::Stopping.is_terminal());
        assert!(AgentState::Dead {
            reason: "test".into()
        }
        .is_terminal());
    }

    #[test]
    fn is_available_for_ready_and_idle() {
        assert!(!AgentState::Spawning.is_available());
        assert!(AgentState::Ready.is_available());
        assert!(!AgentState::Busy {
            task_id: "T1".into()
        }
        .is_available());
        assert!(AgentState::Idle.is_available());
        assert!(!AgentState::Stalled {
            since_ms: 0,
            reason: "x".into()
        }
        .is_available());
        assert!(!AgentState::Recovering { attempt: 1 }.is_available());
        assert!(!AgentState::Stopping.is_available());
        assert!(!AgentState::Dead {
            reason: "x".into()
        }
        .is_available());
    }

    #[test]
    fn can_accept_task_matches_available() {
        assert!(AgentState::Ready.can_accept_task());
        assert!(AgentState::Idle.can_accept_task());
        assert!(!AgentState::Spawning.can_accept_task());
        assert!(!AgentState::Busy {
            task_id: "T1".into()
        }
        .can_accept_task());
        assert!(!AgentState::Stopping.can_accept_task());
    }

    #[test]
    fn label_values() {
        assert_eq!(AgentState::Spawning.label(), "spawning");
        assert_eq!(AgentState::Ready.label(), "ready");
        assert_eq!(
            AgentState::Busy {
                task_id: "T1".into()
            }
            .label(),
            "busy"
        );
        assert_eq!(AgentState::Idle.label(), "idle");
        assert_eq!(
            AgentState::Stalled {
                since_ms: 0,
                reason: "x".into()
            }
            .label(),
            "stalled"
        );
        assert_eq!(AgentState::Recovering { attempt: 1 }.label(), "recovering");
        assert_eq!(AgentState::Stopping.label(), "stopping");
        assert_eq!(
            AgentState::Dead {
                reason: "x".into()
            }
            .label(),
            "dead"
        );
    }

    // ---- Serialization ----

    #[test]
    fn state_serde_round_trip_spawning() {
        let state = AgentState::Spawning;
        let json = serde_json::to_string(&state).unwrap();
        let back: AgentState = serde_json::from_str(&json).unwrap();
        assert_eq!(back, state);
    }

    #[test]
    fn state_serde_round_trip_busy() {
        let state = AgentState::Busy {
            task_id: "CMX1".into(),
        };
        let json = serde_json::to_string(&state).unwrap();
        assert!(json.contains("\"state\":\"busy\""));
        assert!(json.contains("\"task_id\":\"CMX1\""));
        let back: AgentState = serde_json::from_str(&json).unwrap();
        assert_eq!(back, state);
    }

    #[test]
    fn state_serde_round_trip_stalled() {
        let state = AgentState::Stalled {
            since_ms: 45000,
            reason: "heartbeat lost".into(),
        };
        let json = serde_json::to_string(&state).unwrap();
        let back: AgentState = serde_json::from_str(&json).unwrap();
        assert_eq!(back, state);
    }

    #[test]
    fn state_serde_round_trip_dead() {
        let state = AgentState::Dead {
            reason: "killed".into(),
        };
        let json = serde_json::to_string(&state).unwrap();
        let back: AgentState = serde_json::from_str(&json).unwrap();
        assert_eq!(back, state);
    }

    #[test]
    fn transition_serde_round_trip() {
        let transitions = vec![
            Transition::SpawnComplete,
            Transition::TaskAssigned {
                task_id: "T1".into(),
            },
            Transition::TaskCompleted,
            Transition::HeartbeatTimeout { age_ms: 30000 },
            Transition::ErrorDetected {
                message: "crash".into(),
            },
            Transition::RecoveryStarted,
            Transition::RecoverySucceeded,
            Transition::RecoveryFailed {
                message: "gone".into(),
            },
            Transition::StopRequested,
            Transition::Killed,
        ];
        for t in transitions {
            let json = serde_json::to_string(&t).unwrap();
            let back: Transition = serde_json::from_str(&json).unwrap();
            assert_eq!(back, t);
        }
    }

    // ---- Display ----

    #[test]
    fn display_formats() {
        assert_eq!(format!("{}", AgentState::Spawning), "Spawning");
        assert_eq!(format!("{}", AgentState::Ready), "Ready");
        assert_eq!(
            format!(
                "{}",
                AgentState::Busy {
                    task_id: "T1".into()
                }
            ),
            "Busy(T1)"
        );
        assert_eq!(format!("{}", AgentState::Idle), "Idle");
        assert!(format!(
            "{}",
            AgentState::Stalled {
                since_ms: 5000,
                reason: "lost".into()
            }
        )
        .contains("5000ms"));
        assert_eq!(
            format!("{}", AgentState::Recovering { attempt: 2 }),
            "Recovering(attempt=2)"
        );
        assert_eq!(format!("{}", AgentState::Stopping), "Stopping");
        assert_eq!(
            format!(
                "{}",
                AgentState::Dead {
                    reason: "gone".into()
                }
            ),
            "Dead(gone)"
        );
        assert_eq!(format!("{}", Transition::SpawnComplete), "SpawnComplete");
        assert_eq!(
            format!(
                "{}",
                Transition::TaskAssigned {
                    task_id: "T1".into()
                }
            ),
            "TaskAssigned(T1)"
        );
    }

    // ---- Full lifecycle walkthrough ----

    #[test]
    fn full_happy_path_lifecycle() {
        let mut state = AgentState::Spawning;
        state = state.apply(Transition::SpawnComplete).unwrap();
        assert_eq!(state, AgentState::Ready);

        state = state
            .apply(Transition::TaskAssigned {
                task_id: "CMX1".into(),
            })
            .unwrap();
        assert_eq!(
            state,
            AgentState::Busy {
                task_id: "CMX1".into()
            }
        );

        state = state.apply(Transition::TaskCompleted).unwrap();
        assert_eq!(state, AgentState::Idle);

        state = state
            .apply(Transition::TaskAssigned {
                task_id: "CMX2".into(),
            })
            .unwrap();
        assert_eq!(
            state,
            AgentState::Busy {
                task_id: "CMX2".into()
            }
        );

        state = state.apply(Transition::TaskCompleted).unwrap();
        assert_eq!(state, AgentState::Idle);

        state = state.apply(Transition::StopRequested).unwrap();
        assert_eq!(state, AgentState::Stopping);

        state = state.apply(Transition::Killed).unwrap();
        assert!(state.is_terminal());
    }

    #[test]
    fn stall_and_recovery_lifecycle() {
        let mut state = AgentState::Spawning;
        state = state.apply(Transition::SpawnComplete).unwrap();

        state = state
            .apply(Transition::TaskAssigned {
                task_id: "CMX1".into(),
            })
            .unwrap();

        // Heartbeat times out
        state = state
            .apply(Transition::HeartbeatTimeout { age_ms: 60000 })
            .unwrap();
        assert!(matches!(state, AgentState::Stalled { .. }));

        // Recovery starts
        state = state.apply(Transition::RecoveryStarted).unwrap();
        assert_eq!(state, AgentState::Recovering { attempt: 1 });

        // Recovery succeeds
        state = state.apply(Transition::RecoverySucceeded).unwrap();
        assert_eq!(state, AgentState::Ready);
    }

    #[test]
    fn stall_recovery_failure_lifecycle() {
        let mut state = AgentState::Busy {
            task_id: "T1".into(),
        };
        state = state
            .apply(Transition::HeartbeatTimeout { age_ms: 60000 })
            .unwrap();
        state = state.apply(Transition::RecoveryStarted).unwrap();
        assert_eq!(state, AgentState::Recovering { attempt: 1 });

        // Retry recovery (attempt increments)
        state = state.apply(Transition::RecoveryStarted).unwrap();
        assert_eq!(state, AgentState::Recovering { attempt: 2 });

        // Final failure
        state = state
            .apply(Transition::RecoveryFailed {
                message: "session destroyed".into(),
            })
            .unwrap();
        assert!(state.is_terminal());
    }
}
