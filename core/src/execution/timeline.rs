//! Execution timeline — event recording and phase tracking.
//!
//! Each execution has a `Timeline` that records lifecycle events (start,
//! progress updates, phase changes, errors, completion). Provides query
//! methods for duration, current phase, progress, and phase-level durations.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// TimelineEvent
// ---------------------------------------------------------------------------

/// An event recorded on an execution's timeline.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum TimelineEvent {
    Started {
        ms: u64,
    },
    ProgressUpdate {
        ms: u64,
        percent: u32,
        message: String,
    },
    PhaseChange {
        ms: u64,
        from: String,
        to: String,
    },
    OutputEvent {
        ms: u64,
        text: String,
    },
    ErrorOccurred {
        ms: u64,
        error: String,
    },
    Paused {
        ms: u64,
        reason: String,
    },
    Resumed {
        ms: u64,
    },
    Completed {
        ms: u64,
        exit_code: i32,
    },
    Failed {
        ms: u64,
        error: String,
    },
}

impl TimelineEvent {
    /// Extract the timestamp from any event variant.
    pub fn timestamp_ms(&self) -> u64 {
        match self {
            TimelineEvent::Started { ms } => *ms,
            TimelineEvent::ProgressUpdate { ms, .. } => *ms,
            TimelineEvent::PhaseChange { ms, .. } => *ms,
            TimelineEvent::OutputEvent { ms, .. } => *ms,
            TimelineEvent::ErrorOccurred { ms, .. } => *ms,
            TimelineEvent::Paused { ms, .. } => *ms,
            TimelineEvent::Resumed { ms } => *ms,
            TimelineEvent::Completed { ms, .. } => *ms,
            TimelineEvent::Failed { ms, .. } => *ms,
        }
    }

    /// Whether this event is a terminal event (completed or failed).
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            TimelineEvent::Completed { .. } | TimelineEvent::Failed { .. }
        )
    }
}

// ---------------------------------------------------------------------------
// Timeline
// ---------------------------------------------------------------------------

/// An ordered list of events for a single execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Timeline {
    pub execution_id: String,
    pub events: Vec<TimelineEvent>,
}

impl Timeline {
    /// Create a new empty timeline for the given execution.
    pub fn new(execution_id: &str) -> Self {
        Timeline {
            execution_id: execution_id.to_string(),
            events: Vec::new(),
        }
    }

    /// Record a new event on the timeline.
    pub fn record(&mut self, event: TimelineEvent) {
        self.events.push(event);
    }

    /// Total duration from the first Started event to the last terminal event.
    /// Returns None if there is no Started event or no terminal event.
    pub fn duration_ms(&self) -> Option<u64> {
        let start = self.events.iter().find_map(|e| match e {
            TimelineEvent::Started { ms } => Some(*ms),
            _ => None,
        })?;

        let end = self
            .events
            .iter()
            .rev()
            .find_map(|e| match e {
                TimelineEvent::Completed { ms, .. } | TimelineEvent::Failed { ms, .. } => {
                    Some(*ms)
                }
                _ => None,
            })?;

        Some(end.saturating_sub(start))
    }

    /// The current phase, derived from the most recent PhaseChange event.
    /// Returns None if no phase changes have been recorded.
    pub fn current_phase(&self) -> Option<&str> {
        self.events.iter().rev().find_map(|e| match e {
            TimelineEvent::PhaseChange { to, .. } => Some(to.as_str()),
            _ => None,
        })
    }

    /// The latest progress percentage, from the most recent ProgressUpdate.
    pub fn progress_percent(&self) -> Option<u32> {
        self.events.iter().rev().find_map(|e| match e {
            TimelineEvent::ProgressUpdate { percent, .. } => Some(*percent),
            _ => None,
        })
    }

    /// Return all events that occurred at or after the given timestamp.
    pub fn events_since(&self, ms: u64) -> Vec<&TimelineEvent> {
        self.events
            .iter()
            .filter(|e| e.timestamp_ms() >= ms)
            .collect()
    }

    /// The latest error message, from the most recent ErrorOccurred or Failed event.
    pub fn latest_error(&self) -> Option<&str> {
        self.events.iter().rev().find_map(|e| match e {
            TimelineEvent::ErrorOccurred { error, .. } => Some(error.as_str()),
            TimelineEvent::Failed { error, .. } => Some(error.as_str()),
            _ => None,
        })
    }

    /// Compute how long each phase lasted, based on PhaseChange events.
    ///
    /// A phase's duration is measured from the PhaseChange event that entered it
    /// to the next PhaseChange event (or the terminal event if it is the last phase).
    pub fn phase_durations(&self) -> HashMap<String, u64> {
        let mut durations: HashMap<String, u64> = HashMap::new();

        let phase_changes: Vec<(u64, &str)> = self
            .events
            .iter()
            .filter_map(|e| match e {
                TimelineEvent::PhaseChange { ms, to, .. } => Some((*ms, to.as_str())),
                _ => None,
            })
            .collect();

        if phase_changes.is_empty() {
            return durations;
        }

        // Find the end timestamp (terminal event or last event).
        let end_ms = self
            .events
            .last()
            .map(|e| e.timestamp_ms())
            .unwrap_or(0);

        for (i, (start_ms, phase_name)) in phase_changes.iter().enumerate() {
            let next_ms = if i + 1 < phase_changes.len() {
                phase_changes[i + 1].0
            } else {
                end_ms
            };

            let duration = next_ms.saturating_sub(*start_ms);
            *durations.entry(phase_name.to_string()).or_insert(0) += duration;
        }

        durations
    }

    /// Number of events recorded.
    pub fn event_count(&self) -> usize {
        self.events.len()
    }

    /// Whether the timeline has a terminal event.
    pub fn is_finished(&self) -> bool {
        self.events.iter().any(|e| e.is_terminal())
    }
}

// ---------------------------------------------------------------------------
// TimelineView
// ---------------------------------------------------------------------------

/// Generates summary strings from a timeline for display purposes.
#[derive(Debug)]
pub struct TimelineView<'a> {
    timeline: &'a Timeline,
}

impl<'a> TimelineView<'a> {
    /// Create a new view over the given timeline.
    pub fn new(timeline: &'a Timeline) -> Self {
        TimelineView { timeline }
    }

    /// One-line summary of the execution.
    pub fn summary(&self) -> String {
        let phase = self
            .timeline
            .current_phase()
            .unwrap_or("unknown");
        let progress = self
            .timeline
            .progress_percent()
            .map(|p| format!(" ({}%)", p))
            .unwrap_or_default();
        let duration = self
            .timeline
            .duration_ms()
            .map(|d| format!(" [{}ms]", d))
            .unwrap_or_default();
        let status = if self.timeline.is_finished() {
            "finished"
        } else {
            "active"
        };

        format!(
            "{}: {} — {}{}{}",
            self.timeline.execution_id, status, phase, progress, duration
        )
    }

    /// Multi-line event log.
    pub fn event_log(&self) -> String {
        let mut lines = Vec::new();
        for event in &self.timeline.events {
            let ts = event.timestamp_ms();
            let desc = match event {
                TimelineEvent::Started { .. } => "started".to_string(),
                TimelineEvent::ProgressUpdate {
                    percent, message, ..
                } => format!("progress: {}% — {}", percent, message),
                TimelineEvent::PhaseChange { from, to, .. } => {
                    format!("phase: {} -> {}", from, to)
                }
                TimelineEvent::OutputEvent { text, .. } => format!("output: {}", text),
                TimelineEvent::ErrorOccurred { error, .. } => format!("error: {}", error),
                TimelineEvent::Paused { reason, .. } => format!("paused: {}", reason),
                TimelineEvent::Resumed { .. } => "resumed".to_string(),
                TimelineEvent::Completed { exit_code, .. } => {
                    format!("completed (exit {})", exit_code)
                }
                TimelineEvent::Failed { error, .. } => format!("failed: {}", error),
            };
            lines.push(format!("[{}ms] {}", ts, desc));
        }
        lines.join("\n")
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_timeline() -> Timeline {
        let mut t = Timeline::new("exec-1");
        t.record(TimelineEvent::Started { ms: 1000 });
        t.record(TimelineEvent::PhaseChange {
            ms: 1000,
            from: "init".into(),
            to: "build".into(),
        });
        t.record(TimelineEvent::ProgressUpdate {
            ms: 2000,
            percent: 25,
            message: "compiling".into(),
        });
        t.record(TimelineEvent::PhaseChange {
            ms: 3000,
            from: "build".into(),
            to: "test".into(),
        });
        t.record(TimelineEvent::ProgressUpdate {
            ms: 4000,
            percent: 75,
            message: "running tests".into(),
        });
        t.record(TimelineEvent::Completed {
            ms: 5000,
            exit_code: 0,
        });
        t
    }

    #[test]
    fn new_timeline_empty() {
        let t = Timeline::new("x");
        assert_eq!(t.execution_id, "x");
        assert!(t.events.is_empty());
        assert_eq!(t.event_count(), 0);
    }

    #[test]
    fn record_and_count() {
        let mut t = Timeline::new("x");
        t.record(TimelineEvent::Started { ms: 100 });
        assert_eq!(t.event_count(), 1);
    }

    #[test]
    fn duration_start_to_complete() {
        let t = sample_timeline();
        assert_eq!(t.duration_ms(), Some(4000)); // 5000 - 1000
    }

    #[test]
    fn duration_start_to_failed() {
        let mut t = Timeline::new("x");
        t.record(TimelineEvent::Started { ms: 100 });
        t.record(TimelineEvent::Failed {
            ms: 600,
            error: "boom".into(),
        });
        assert_eq!(t.duration_ms(), Some(500));
    }

    #[test]
    fn duration_no_start() {
        let mut t = Timeline::new("x");
        t.record(TimelineEvent::Completed {
            ms: 500,
            exit_code: 0,
        });
        assert!(t.duration_ms().is_none());
    }

    #[test]
    fn duration_no_terminal() {
        let mut t = Timeline::new("x");
        t.record(TimelineEvent::Started { ms: 100 });
        t.record(TimelineEvent::ProgressUpdate {
            ms: 200,
            percent: 50,
            message: "half".into(),
        });
        assert!(t.duration_ms().is_none());
    }

    #[test]
    fn current_phase() {
        let t = sample_timeline();
        assert_eq!(t.current_phase(), Some("test"));
    }

    #[test]
    fn current_phase_none() {
        let t = Timeline::new("x");
        assert!(t.current_phase().is_none());
    }

    #[test]
    fn progress_percent_latest() {
        let t = sample_timeline();
        assert_eq!(t.progress_percent(), Some(75));
    }

    #[test]
    fn progress_percent_none() {
        let t = Timeline::new("x");
        assert!(t.progress_percent().is_none());
    }

    #[test]
    fn events_since_filters() {
        let t = sample_timeline();
        let recent = t.events_since(3000);
        assert_eq!(recent.len(), 3); // PhaseChange@3000, Progress@4000, Completed@5000
    }

    #[test]
    fn events_since_all() {
        let t = sample_timeline();
        let all = t.events_since(0);
        assert_eq!(all.len(), t.event_count());
    }

    #[test]
    fn events_since_none() {
        let t = sample_timeline();
        let none = t.events_since(99999);
        assert!(none.is_empty());
    }

    #[test]
    fn latest_error_from_error_occurred() {
        let mut t = Timeline::new("x");
        t.record(TimelineEvent::Started { ms: 100 });
        t.record(TimelineEvent::ErrorOccurred {
            ms: 200,
            error: "disk full".into(),
        });
        assert_eq!(t.latest_error(), Some("disk full"));
    }

    #[test]
    fn latest_error_from_failed() {
        let mut t = Timeline::new("x");
        t.record(TimelineEvent::Started { ms: 100 });
        t.record(TimelineEvent::Failed {
            ms: 200,
            error: "oom".into(),
        });
        assert_eq!(t.latest_error(), Some("oom"));
    }

    #[test]
    fn latest_error_prefers_most_recent() {
        let mut t = Timeline::new("x");
        t.record(TimelineEvent::ErrorOccurred {
            ms: 100,
            error: "first".into(),
        });
        t.record(TimelineEvent::ErrorOccurred {
            ms: 200,
            error: "second".into(),
        });
        assert_eq!(t.latest_error(), Some("second"));
    }

    #[test]
    fn latest_error_none() {
        let mut t = Timeline::new("x");
        t.record(TimelineEvent::Started { ms: 100 });
        assert!(t.latest_error().is_none());
    }

    #[test]
    fn phase_durations_basic() {
        let t = sample_timeline();
        let durations = t.phase_durations();

        assert_eq!(*durations.get("build").unwrap(), 2000); // 3000 - 1000
        assert_eq!(*durations.get("test").unwrap(), 2000); // 5000 - 3000
    }

    #[test]
    fn phase_durations_empty() {
        let t = Timeline::new("x");
        assert!(t.phase_durations().is_empty());
    }

    #[test]
    fn phase_durations_single_phase() {
        let mut t = Timeline::new("x");
        t.record(TimelineEvent::PhaseChange {
            ms: 100,
            from: "none".into(),
            to: "build".into(),
        });
        t.record(TimelineEvent::Completed {
            ms: 500,
            exit_code: 0,
        });

        let durations = t.phase_durations();
        assert_eq!(*durations.get("build").unwrap(), 400);
    }

    #[test]
    fn is_finished_true() {
        let t = sample_timeline();
        assert!(t.is_finished());
    }

    #[test]
    fn is_finished_false() {
        let mut t = Timeline::new("x");
        t.record(TimelineEvent::Started { ms: 100 });
        assert!(!t.is_finished());
    }

    #[test]
    fn event_timestamp_extraction() {
        let events = vec![
            TimelineEvent::Started { ms: 100 },
            TimelineEvent::ProgressUpdate {
                ms: 200,
                percent: 50,
                message: "half".into(),
            },
            TimelineEvent::Paused {
                ms: 300,
                reason: "break".into(),
            },
            TimelineEvent::Resumed { ms: 400 },
        ];

        assert_eq!(events[0].timestamp_ms(), 100);
        assert_eq!(events[1].timestamp_ms(), 200);
        assert_eq!(events[2].timestamp_ms(), 300);
        assert_eq!(events[3].timestamp_ms(), 400);
    }

    #[test]
    fn timeline_serde_round_trip() {
        let t = sample_timeline();
        let json = serde_json::to_string(&t).unwrap();
        let back: Timeline = serde_json::from_str(&json).unwrap();
        assert_eq!(back.execution_id, "exec-1");
        assert_eq!(back.events.len(), t.events.len());
    }

    #[test]
    fn timeline_event_serde_all_variants() {
        let events = vec![
            TimelineEvent::Started { ms: 1 },
            TimelineEvent::ProgressUpdate {
                ms: 2,
                percent: 50,
                message: "half".into(),
            },
            TimelineEvent::PhaseChange {
                ms: 3,
                from: "a".into(),
                to: "b".into(),
            },
            TimelineEvent::OutputEvent {
                ms: 4,
                text: "line".into(),
            },
            TimelineEvent::ErrorOccurred {
                ms: 5,
                error: "err".into(),
            },
            TimelineEvent::Paused {
                ms: 6,
                reason: "pause".into(),
            },
            TimelineEvent::Resumed { ms: 7 },
            TimelineEvent::Completed {
                ms: 8,
                exit_code: 0,
            },
            TimelineEvent::Failed {
                ms: 9,
                error: "fail".into(),
            },
        ];

        for event in &events {
            let json = serde_json::to_string(event).unwrap();
            let back: TimelineEvent = serde_json::from_str(&json).unwrap();
            assert_eq!(back, *event);
        }
    }

    // -- TimelineView tests --

    #[test]
    fn view_summary_finished() {
        let t = sample_timeline();
        let view = TimelineView::new(&t);
        let summary = view.summary();
        assert!(summary.contains("exec-1"));
        assert!(summary.contains("finished"));
        assert!(summary.contains("test"));
        assert!(summary.contains("75%"));
        assert!(summary.contains("4000ms"));
    }

    #[test]
    fn view_summary_active() {
        let mut t = Timeline::new("e2");
        t.record(TimelineEvent::Started { ms: 100 });
        t.record(TimelineEvent::PhaseChange {
            ms: 100,
            from: "none".into(),
            to: "build".into(),
        });

        let view = TimelineView::new(&t);
        let summary = view.summary();
        assert!(summary.contains("active"));
        assert!(summary.contains("build"));
    }

    #[test]
    fn view_event_log() {
        let t = sample_timeline();
        let view = TimelineView::new(&t);
        let log = view.event_log();

        assert!(log.contains("[1000ms] started"));
        assert!(log.contains("phase: build -> test"));
        assert!(log.contains("completed (exit 0)"));
    }

    #[test]
    fn view_event_log_empty() {
        let t = Timeline::new("x");
        let view = TimelineView::new(&t);
        assert!(view.event_log().is_empty());
    }
}
