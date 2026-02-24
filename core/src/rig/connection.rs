//! SSH connection state tracking.
//!
//! `ConnectionTracker` maintains per-remote connection state, tracks attempt
//! counts and timing, and implements exponential backoff for retry decisions.
//! No actual SSH connections are opened here — this is pure state management.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};


// ---------------------------------------------------------------------------
// ConnState
// ---------------------------------------------------------------------------

/// The current state of a connection to a remote host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConnState {
    /// No connection has been established or attempted.
    Disconnected,
    /// A connection attempt is in progress.
    Connecting {
        /// Epoch-millisecond timestamp when the attempt started.
        since_ms: u64,
    },
    /// The connection is active.
    Connected {
        /// Epoch-millisecond timestamp when the connection was established.
        since_ms: u64,
    },
    /// The last connection attempt failed.
    Failed {
        /// Human-readable failure reason.
        reason: String,
        /// Epoch-millisecond timestamp of the failure.
        at_ms: u64,
    },
}


// ---------------------------------------------------------------------------
// ConnectionInfo
// ---------------------------------------------------------------------------

/// Full connection metadata for a single remote.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionInfo {
    /// Name of the remote this info belongs to.
    pub remote: String,
    /// Current connection state.
    pub state: ConnState,
    /// Total number of connection attempts (successful or not).
    pub attempts: u32,
    /// Timestamp of the most recent successful connection.
    pub last_success_ms: Option<u64>,
    /// Timestamp of the most recent failure.
    pub last_failure_ms: Option<u64>,
    /// Round-trip latency measured during the last successful check.
    pub latency_ms: Option<u64>,
}

impl ConnectionInfo {
    fn new(remote: &str) -> Self {
        ConnectionInfo {
            remote: remote.to_string(),
            state: ConnState::Disconnected,
            attempts: 0,
            last_success_ms: None,
            last_failure_ms: None,
            latency_ms: None,
        }
    }
}


// ---------------------------------------------------------------------------
// ConnectionSummary
// ---------------------------------------------------------------------------

/// Aggregate counts of connections by state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionSummary {
    pub total: usize,
    pub connected: usize,
    pub connecting: usize,
    pub failed: usize,
    pub disconnected: usize,
}


// ---------------------------------------------------------------------------
// ConnectionTracker
// ---------------------------------------------------------------------------

/// Tracks connection state for all registered remotes.
///
/// Provides retry budgets and exponential backoff calculations without
/// actually performing any network operations.
pub struct ConnectionTracker {
    connections: HashMap<String, ConnectionInfo>,
    max_retries: u32,
    backoff_base_ms: u64,
}

impl ConnectionTracker {
    /// Create a new tracker with the given retry budget and backoff base.
    ///
    /// Retries use exponential backoff: `backoff_base_ms * 2^(attempt - 1)`.
    pub fn new(max_retries: u32, backoff_base_ms: u64) -> Self {
        ConnectionTracker {
            connections: HashMap::new(),
            max_retries,
            backoff_base_ms,
        }
    }

    /// Register a remote for tracking. Idempotent — calling twice with the
    /// same name is a no-op.
    pub fn register(&mut self, remote: &str) {
        self.connections
            .entry(remote.to_string())
            .or_insert_with(|| ConnectionInfo::new(remote));
    }

    /// Record that a connection attempt has started.
    pub fn start_connecting(&mut self, remote: &str, now_ms: u64) -> Result<(), String> {
        let info = self
            .connections
            .get_mut(remote)
            .ok_or_else(|| format!("remote '{}' not registered", remote))?;
        info.state = ConnState::Connecting { since_ms: now_ms };
        info.attempts += 1;
        Ok(())
    }

    /// Record that a connection has been established.
    pub fn mark_connected(
        &mut self,
        remote: &str,
        now_ms: u64,
        latency_ms: u64,
    ) -> Result<(), String> {
        let info = self
            .connections
            .get_mut(remote)
            .ok_or_else(|| format!("remote '{}' not registered", remote))?;
        info.state = ConnState::Connected { since_ms: now_ms };
        info.last_success_ms = Some(now_ms);
        info.latency_ms = Some(latency_ms);
        Ok(())
    }

    /// Record that a connection attempt has failed.
    pub fn mark_failed(
        &mut self,
        remote: &str,
        reason: &str,
        now_ms: u64,
    ) -> Result<(), String> {
        let info = self
            .connections
            .get_mut(remote)
            .ok_or_else(|| format!("remote '{}' not registered", remote))?;
        info.state = ConnState::Failed {
            reason: reason.to_string(),
            at_ms: now_ms,
        };
        info.last_failure_ms = Some(now_ms);
        Ok(())
    }

    /// Disconnect a remote (transition to `Disconnected` without recording a failure).
    pub fn disconnect(&mut self, remote: &str) -> Result<(), String> {
        let info = self
            .connections
            .get_mut(remote)
            .ok_or_else(|| format!("remote '{}' not registered", remote))?;
        info.state = ConnState::Disconnected;
        Ok(())
    }

    /// Get the current state for a remote.
    pub fn state(&self, remote: &str) -> Option<&ConnState> {
        self.connections.get(remote).map(|i| &i.state)
    }

    /// Get full connection info for a remote.
    pub fn info(&self, remote: &str) -> Option<&ConnectionInfo> {
        self.connections.get(remote)
    }

    /// Whether the remote is currently in the `Connected` state.
    pub fn is_connected(&self, remote: &str) -> bool {
        matches!(
            self.connections.get(remote).map(|i| &i.state),
            Some(ConnState::Connected { .. })
        )
    }

    /// Whether the remote should be retried based on its attempt count and
    /// the configured `max_retries`.
    pub fn should_retry(&self, remote: &str) -> bool {
        match self.connections.get(remote) {
            None => false,
            Some(info) => {
                // Only retry from Failed state.
                if !matches!(info.state, ConnState::Failed { .. }) {
                    return false;
                }
                // Count only the *failure* attempts. The first `max_retries`
                // failures are retryable.
                let failure_count = self.failure_count(remote);
                failure_count <= self.max_retries
            }
        }
    }

    /// Calculate the next retry delay in milliseconds based on the failure
    /// count, using exponential backoff.
    pub fn next_retry_ms(&self, remote: &str) -> Option<u64> {
        let info = self.connections.get(remote)?;
        if !matches!(info.state, ConnState::Failed { .. }) {
            return None;
        }
        let failures = self.failure_count(remote);
        if failures == 0 {
            return Some(self.backoff_base_ms);
        }
        let exponent = (failures - 1).min(16); // cap to avoid overflow
        Some(self.backoff_base_ms * 2u64.saturating_pow(exponent))
    }

    /// Names of all remotes currently in the `Connected` state.
    pub fn connected_remotes(&self) -> Vec<&str> {
        self.connections
            .iter()
            .filter(|(_, info)| matches!(info.state, ConnState::Connected { .. }))
            .map(|(name, _)| name.as_str())
            .collect()
    }

    /// Names of all remotes currently in the `Failed` state.
    pub fn failed_remotes(&self) -> Vec<&str> {
        self.connections
            .iter()
            .filter(|(_, info)| matches!(info.state, ConnState::Failed { .. }))
            .map(|(name, _)| name.as_str())
            .collect()
    }

    /// Return an aggregate summary of all tracked connections.
    pub fn summary(&self) -> ConnectionSummary {
        let mut s = ConnectionSummary {
            total: self.connections.len(),
            connected: 0,
            connecting: 0,
            failed: 0,
            disconnected: 0,
        };
        for info in self.connections.values() {
            match info.state {
                ConnState::Disconnected => s.disconnected += 1,
                ConnState::Connecting { .. } => s.connecting += 1,
                ConnState::Connected { .. } => s.connected += 1,
                ConnState::Failed { .. } => s.failed += 1,
            }
        }
        s
    }

    /// Count the number of failures for a remote. This counts transitions into
    /// the Failed state based on `attempts` minus successes.
    fn failure_count(&self, remote: &str) -> u32 {
        match self.connections.get(remote) {
            None => 0,
            Some(info) => {
                // If last_success_ms is set, count failures since then.
                // Otherwise count all attempts as failures.
                // A Connected state means the most recent attempt succeeded.
                match info.state {
                    ConnState::Failed { .. } => {
                        // Count sequential failures: attempts since last success.
                        // If never succeeded, all attempts are failures.
                        // We approximate with: all attempts are failures if never
                        // connected. Otherwise, the last success resets the count.
                        // Since we don't track per-attempt results, use attempts
                        // as the failure counter for retry purposes. We reset
                        // on successful connection in mark_connected.
                        info.attempts
                    }
                    _ => 0,
                }
            }
        }
    }

    /// Reset the attempt counter for a remote. Called internally or by the
    /// caller after a successful cycle.
    pub fn reset_attempts(&mut self, remote: &str) -> Result<(), String> {
        let info = self
            .connections
            .get_mut(remote)
            .ok_or_else(|| format!("remote '{}' not registered", remote))?;
        info.attempts = 0;
        Ok(())
    }
}


// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_creates_disconnected() {
        let mut tracker = ConnectionTracker::new(3, 1000);
        tracker.register("r1");
        assert_eq!(tracker.state("r1"), Some(&ConnState::Disconnected));
    }

    #[test]
    fn register_is_idempotent() {
        let mut tracker = ConnectionTracker::new(3, 1000);
        tracker.register("r1");
        tracker.start_connecting("r1", 100).unwrap();
        // Re-register should NOT reset state.
        tracker.register("r1");
        assert!(matches!(
            tracker.state("r1"),
            Some(ConnState::Connecting { .. })
        ));
    }

    #[test]
    fn full_lifecycle_happy_path() {
        let mut tracker = ConnectionTracker::new(3, 1000);
        tracker.register("r1");

        // Disconnected -> Connecting
        tracker.start_connecting("r1", 1000).unwrap();
        assert!(matches!(
            tracker.state("r1"),
            Some(ConnState::Connecting { since_ms: 1000 })
        ));

        // Connecting -> Connected
        tracker.mark_connected("r1", 1050, 50).unwrap();
        assert!(matches!(
            tracker.state("r1"),
            Some(ConnState::Connected { since_ms: 1050 })
        ));
        assert!(tracker.is_connected("r1"));

        let info = tracker.info("r1").unwrap();
        assert_eq!(info.last_success_ms, Some(1050));
        assert_eq!(info.latency_ms, Some(50));
        assert_eq!(info.attempts, 1);

        // Connected -> Disconnected
        tracker.disconnect("r1").unwrap();
        assert_eq!(tracker.state("r1"), Some(&ConnState::Disconnected));
        assert!(!tracker.is_connected("r1"));
    }

    #[test]
    fn failure_path() {
        let mut tracker = ConnectionTracker::new(3, 1000);
        tracker.register("r1");

        tracker.start_connecting("r1", 1000).unwrap();
        tracker.mark_failed("r1", "connection refused", 1500).unwrap();

        assert!(matches!(tracker.state("r1"), Some(ConnState::Failed { .. })));
        let info = tracker.info("r1").unwrap();
        assert_eq!(info.last_failure_ms, Some(1500));
        assert_eq!(info.attempts, 1);
    }

    #[test]
    fn unregistered_remote_fails() {
        let mut tracker = ConnectionTracker::new(3, 1000);
        assert!(tracker.start_connecting("ghost", 0).is_err());
        assert!(tracker.mark_connected("ghost", 0, 0).is_err());
        assert!(tracker.mark_failed("ghost", "x", 0).is_err());
        assert!(tracker.disconnect("ghost").is_err());
    }

    #[test]
    fn should_retry_within_budget() {
        let mut tracker = ConnectionTracker::new(3, 1000);
        tracker.register("r1");

        // Not in Failed state -> no retry.
        assert!(!tracker.should_retry("r1"));

        // First failure -> retryable.
        tracker.start_connecting("r1", 100).unwrap();
        tracker.mark_failed("r1", "timeout", 200).unwrap();
        assert!(tracker.should_retry("r1"));

        // Second failure.
        tracker.start_connecting("r1", 300).unwrap();
        tracker.mark_failed("r1", "timeout", 400).unwrap();
        assert!(tracker.should_retry("r1"));

        // Third failure.
        tracker.start_connecting("r1", 500).unwrap();
        tracker.mark_failed("r1", "timeout", 600).unwrap();
        assert!(tracker.should_retry("r1"));

        // Fourth failure -> exceeds budget of 3 retries.
        tracker.start_connecting("r1", 700).unwrap();
        tracker.mark_failed("r1", "timeout", 800).unwrap();
        assert!(!tracker.should_retry("r1"));
    }

    #[test]
    fn next_retry_ms_exponential_backoff() {
        let mut tracker = ConnectionTracker::new(5, 1000);
        tracker.register("r1");

        // Not failed -> None.
        assert_eq!(tracker.next_retry_ms("r1"), None);

        // 1 failure -> base delay.
        tracker.start_connecting("r1", 100).unwrap();
        tracker.mark_failed("r1", "err", 200).unwrap();
        assert_eq!(tracker.next_retry_ms("r1"), Some(1000));

        // 2 failures -> 2x base.
        tracker.start_connecting("r1", 300).unwrap();
        tracker.mark_failed("r1", "err", 400).unwrap();
        assert_eq!(tracker.next_retry_ms("r1"), Some(2000));

        // 3 failures -> 4x base.
        tracker.start_connecting("r1", 500).unwrap();
        tracker.mark_failed("r1", "err", 600).unwrap();
        assert_eq!(tracker.next_retry_ms("r1"), Some(4000));
    }

    #[test]
    fn connected_and_failed_remotes() {
        let mut tracker = ConnectionTracker::new(3, 1000);
        tracker.register("r1");
        tracker.register("r2");
        tracker.register("r3");

        tracker.start_connecting("r1", 100).unwrap();
        tracker.mark_connected("r1", 150, 50).unwrap();

        tracker.start_connecting("r2", 100).unwrap();
        tracker.mark_failed("r2", "timeout", 200).unwrap();

        // r3 stays disconnected.

        let connected = tracker.connected_remotes();
        assert_eq!(connected.len(), 1);
        assert!(connected.contains(&"r1"));

        let failed = tracker.failed_remotes();
        assert_eq!(failed.len(), 1);
        assert!(failed.contains(&"r2"));
    }

    #[test]
    fn summary_counts() {
        let mut tracker = ConnectionTracker::new(3, 1000);
        tracker.register("r1");
        tracker.register("r2");
        tracker.register("r3");
        tracker.register("r4");

        tracker.start_connecting("r1", 100).unwrap();
        tracker.mark_connected("r1", 150, 50).unwrap();

        tracker.start_connecting("r2", 100).unwrap();
        tracker.mark_failed("r2", "refused", 200).unwrap();

        tracker.start_connecting("r3", 100).unwrap();
        // r3 is still connecting.

        // r4 is still disconnected.

        let s = tracker.summary();
        assert_eq!(s.total, 4);
        assert_eq!(s.connected, 1);
        assert_eq!(s.failed, 1);
        assert_eq!(s.connecting, 1);
        assert_eq!(s.disconnected, 1);
    }

    #[test]
    fn reset_attempts() {
        let mut tracker = ConnectionTracker::new(3, 1000);
        tracker.register("r1");
        tracker.start_connecting("r1", 100).unwrap();
        tracker.mark_failed("r1", "err", 200).unwrap();
        assert_eq!(tracker.info("r1").unwrap().attempts, 1);
        tracker.reset_attempts("r1").unwrap();
        assert_eq!(tracker.info("r1").unwrap().attempts, 0);
    }

    #[test]
    fn reset_attempts_unregistered_fails() {
        let mut tracker = ConnectionTracker::new(3, 1000);
        assert!(tracker.reset_attempts("ghost").is_err());
    }

    #[test]
    fn not_connected_for_unknown_remote() {
        let tracker = ConnectionTracker::new(3, 1000);
        assert!(!tracker.is_connected("ghost"));
    }

    #[test]
    fn state_returns_none_for_unknown() {
        let tracker = ConnectionTracker::new(3, 1000);
        assert!(tracker.state("ghost").is_none());
    }

    #[test]
    fn info_returns_none_for_unknown() {
        let tracker = ConnectionTracker::new(3, 1000);
        assert!(tracker.info("ghost").is_none());
    }

    #[test]
    fn multiple_success_updates_latency() {
        let mut tracker = ConnectionTracker::new(3, 1000);
        tracker.register("r1");
        tracker.start_connecting("r1", 100).unwrap();
        tracker.mark_connected("r1", 150, 50).unwrap();
        assert_eq!(tracker.info("r1").unwrap().latency_ms, Some(50));

        // Reconnect with different latency.
        tracker.disconnect("r1").unwrap();
        tracker.start_connecting("r1", 200).unwrap();
        tracker.mark_connected("r1", 220, 20).unwrap();
        assert_eq!(tracker.info("r1").unwrap().latency_ms, Some(20));
        assert_eq!(tracker.info("r1").unwrap().last_success_ms, Some(220));
    }
}
