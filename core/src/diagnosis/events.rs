//! Intervention event recording and JSONL persistence.
//!
//! Each intervention is recorded as an `InterventionEvent` — the core unit
//! of diagnostic data. Events are persisted in JSON Lines format (one JSON
//! object per line) and loaded on startup to rebuild in-memory statistics.

use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};


// ---------------------------------------------------------------------------
// SignalType
// ---------------------------------------------------------------------------

/// The monitoring signal that triggered an intervention.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum SignalType {
    HeartbeatStale,
    ErrorPattern,
    OutputStall,
    SshDisconnected,
    ExplicitError,
    TriggerFired(String),
    ManualEscalation,
}

impl fmt::Display for SignalType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SignalType::HeartbeatStale => write!(f, "heartbeat_stale"),
            SignalType::ErrorPattern => write!(f, "error_pattern"),
            SignalType::OutputStall => write!(f, "output_stall"),
            SignalType::SshDisconnected => write!(f, "ssh_disconnected"),
            SignalType::ExplicitError => write!(f, "explicit_error"),
            SignalType::TriggerFired(name) => write!(f, "trigger_fired({})", name),
            SignalType::ManualEscalation => write!(f, "manual_escalation"),
        }
    }
}


// ---------------------------------------------------------------------------
// InterventionAction
// ---------------------------------------------------------------------------

/// What the system (or PM agent) did in response to a signal.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum InterventionAction {
    Retry,
    Restart,
    Escalate,
    Redesign,
    Ignore,
    Manual(String),
}

impl fmt::Display for InterventionAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InterventionAction::Retry => write!(f, "retry"),
            InterventionAction::Restart => write!(f, "restart"),
            InterventionAction::Escalate => write!(f, "escalate"),
            InterventionAction::Redesign => write!(f, "redesign"),
            InterventionAction::Ignore => write!(f, "ignore"),
            InterventionAction::Manual(desc) => write!(f, "manual({})", desc),
        }
    }
}


// ---------------------------------------------------------------------------
// InterventionOutcome
// ---------------------------------------------------------------------------

/// What happened after an intervention was applied.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InterventionOutcome {
    /// Problem fixed.
    Resolved,
    /// Same problem persists.
    StillBroken,
    /// New/different problem emerged.
    DifferentError,
    /// Problem went away on its own (signal was noise).
    SelfResolved,
    /// No clear outcome within monitoring window.
    Timeout,
    /// Outcome not yet recorded (signal fired, awaiting result).
    Pending,
}


// ---------------------------------------------------------------------------
// InterventionEvent
// ---------------------------------------------------------------------------

/// A single recorded intervention — the core unit of diagnostic data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterventionEvent {
    pub id: u64,
    pub timestamp_ms: u64,
    pub agent: String,
    pub signal: SignalType,
    pub signal_detail: String,
    pub action: InterventionAction,
    pub outcome: InterventionOutcome,
    pub outcome_detail: String,
    pub duration_ms: u64,
    pub failure_mode: String,
}


// ---------------------------------------------------------------------------
// DiagnosisError
// ---------------------------------------------------------------------------

/// Errors that can occur in the diagnosis module.
#[derive(Debug)]
pub enum DiagnosisError {
    IoError(std::io::Error),
    ParseError(String),
    EventNotFound(u64),
    InvalidOutcome(String),
}

impl fmt::Display for DiagnosisError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DiagnosisError::IoError(e) => write!(f, "IO error: {}", e),
            DiagnosisError::ParseError(msg) => write!(f, "parse error: {}", msg),
            DiagnosisError::EventNotFound(id) => write!(f, "event not found: {}", id),
            DiagnosisError::InvalidOutcome(msg) => write!(f, "invalid outcome: {}", msg),
        }
    }
}

impl From<std::io::Error> for DiagnosisError {
    fn from(e: std::io::Error) -> Self {
        DiagnosisError::IoError(e)
    }
}

impl From<serde_json::Error> for DiagnosisError {
    fn from(e: serde_json::Error) -> Self {
        DiagnosisError::ParseError(e.to_string())
    }
}


// ---------------------------------------------------------------------------
// JSONL persistence helpers
// ---------------------------------------------------------------------------

/// Append a single event as a JSON line to the given file path.
/// Creates parent directories if they don't exist.
pub fn append_event(path: &PathBuf, event: &InterventionEvent) -> Result<(), DiagnosisError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    let line = serde_json::to_string(event)?;
    writeln!(file, "{}", line)?;
    Ok(())
}

/// Load all events from a JSONL file. Skips blank and malformed lines
/// (printing a warning to stderr for malformed ones). Returns an empty
/// vec if the file doesn't exist.
pub fn load_events(path: &PathBuf) -> Result<Vec<InterventionEvent>, DiagnosisError> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let data = fs::read_to_string(path)?;
    let mut events = Vec::new();
    for (i, line) in data.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        match serde_json::from_str::<InterventionEvent>(trimmed) {
            Ok(event) => events.push(event),
            Err(e) => {
                eprintln!(
                    "warning: skipping malformed JSONL line {} in {}: {}",
                    i + 1,
                    path.display(),
                    e
                );
            }
        }
    }
    Ok(events)
}

/// Write all events to the file, replacing its contents.
/// Used after compaction.
pub fn save_all_events(
    path: &PathBuf,
    events: &[InterventionEvent],
) -> Result<(), DiagnosisError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(path)?;
    for event in events {
        let line = serde_json::to_string(event)?;
        writeln!(file, "{}", line)?;
    }
    Ok(())
}


// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

#[cfg(test)]
pub fn test_dir(name: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let id = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir()
        .join("cmx_diag_test")
        .join(format!("{}_{}", name, id));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}


// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    pub fn sample_event(id: u64) -> InterventionEvent {
        InterventionEvent {
            id,
            timestamp_ms: 1000 + id * 100,
            agent: "worker-1".to_string(),
            signal: SignalType::HeartbeatStale,
            signal_detail: "no heartbeat for 120s".to_string(),
            action: InterventionAction::Retry,
            outcome: InterventionOutcome::Resolved,
            outcome_detail: "agent resumed".to_string(),
            duration_ms: 5000,
            failure_mode: "infrastructure".to_string(),
        }
    }

    #[test]
    fn signal_type_serde_round_trip() {
        let signals = vec![
            SignalType::HeartbeatStale,
            SignalType::ErrorPattern,
            SignalType::OutputStall,
            SignalType::SshDisconnected,
            SignalType::ExplicitError,
            SignalType::TriggerFired("my_trigger".into()),
            SignalType::ManualEscalation,
        ];
        for signal in &signals {
            let json = serde_json::to_string(signal).unwrap();
            let back: SignalType = serde_json::from_str(&json).unwrap();
            assert_eq!(&back, signal);
        }
    }

    #[test]
    fn intervention_action_serde_round_trip() {
        let actions = vec![
            InterventionAction::Retry,
            InterventionAction::Restart,
            InterventionAction::Escalate,
            InterventionAction::Redesign,
            InterventionAction::Ignore,
            InterventionAction::Manual("rebooted host".into()),
        ];
        for action in &actions {
            let json = serde_json::to_string(action).unwrap();
            let back: InterventionAction = serde_json::from_str(&json).unwrap();
            assert_eq!(&back, action);
        }
    }

    #[test]
    fn intervention_outcome_serde_round_trip() {
        let outcomes = vec![
            InterventionOutcome::Resolved,
            InterventionOutcome::StillBroken,
            InterventionOutcome::DifferentError,
            InterventionOutcome::SelfResolved,
            InterventionOutcome::Timeout,
            InterventionOutcome::Pending,
        ];
        for outcome in &outcomes {
            let json = serde_json::to_string(outcome).unwrap();
            let back: InterventionOutcome = serde_json::from_str(&json).unwrap();
            assert_eq!(&back, outcome);
        }
    }

    #[test]
    fn event_serde_round_trip() {
        let event = sample_event(42);
        let json = serde_json::to_string(&event).unwrap();
        let back: InterventionEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, 42);
        assert_eq!(back.agent, "worker-1");
        assert_eq!(back.signal, SignalType::HeartbeatStale);
        assert_eq!(back.outcome, InterventionOutcome::Resolved);
    }

    #[test]
    fn append_and_load_events() {
        let dir = test_dir("append_load");
        let path = dir.join("logs/events.jsonl");

        append_event(&path, &sample_event(0)).unwrap();
        append_event(&path, &sample_event(1)).unwrap();
        append_event(&path, &sample_event(2)).unwrap();

        let loaded = load_events(&path).unwrap();
        assert_eq!(loaded.len(), 3);
        assert_eq!(loaded[0].id, 0);
        assert_eq!(loaded[2].id, 2);
    }

    #[test]
    fn load_events_missing_file() {
        let dir = test_dir("missing");
        let path = dir.join("nonexistent.jsonl");
        let loaded = load_events(&path).unwrap();
        assert!(loaded.is_empty());
    }

    #[test]
    fn load_events_skips_malformed_lines() {
        let dir = test_dir("malformed");
        let path = dir.join("events.jsonl");

        let event = sample_event(0);
        let good_line = serde_json::to_string(&event).unwrap();

        let content = format!("{}\nthis is not json\n{}\n", good_line, good_line);
        fs::write(&path, content).unwrap();

        let loaded = load_events(&path).unwrap();
        assert_eq!(loaded.len(), 2);
    }

    #[test]
    fn load_events_skips_blank_lines() {
        let dir = test_dir("blank");
        let path = dir.join("events.jsonl");

        let event = sample_event(0);
        let good_line = serde_json::to_string(&event).unwrap();

        let content = format!("\n  \n{}\n\n", good_line);
        fs::write(&path, content).unwrap();

        let loaded = load_events(&path).unwrap();
        assert_eq!(loaded.len(), 1);
    }

    #[test]
    fn save_all_overwrites() {
        let dir = test_dir("save_all");
        let path = dir.join("events.jsonl");

        let events: Vec<InterventionEvent> = (0..3).map(sample_event).collect();
        save_all_events(&path, &events).unwrap();

        let events2 = vec![sample_event(99)];
        save_all_events(&path, &events2).unwrap();

        let loaded = load_events(&path).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].id, 99);
    }

    #[test]
    fn signal_type_display() {
        assert_eq!(SignalType::HeartbeatStale.to_string(), "heartbeat_stale");
        assert_eq!(
            SignalType::TriggerFired("foo".into()).to_string(),
            "trigger_fired(foo)"
        );
    }

    #[test]
    fn creates_parent_directories() {
        let dir = test_dir("mkdir");
        let path = dir.join("a/b/c/events.jsonl");
        assert!(!path.exists());

        append_event(&path, &sample_event(0)).unwrap();
        assert!(path.exists());
    }
}
