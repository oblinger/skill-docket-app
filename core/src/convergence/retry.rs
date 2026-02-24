//! Retry policies and per-action retry tracking.
//!
//! Provides configurable retry behaviour with multiple backoff strategies
//! (fixed, linear, exponential) and an `ActionRetryTracker` that maintains
//! per-action attempt counts and timing.

use std::collections::HashMap;

use crate::types::config::BackoffStrategy;

// ---------------------------------------------------------------------------
// RetryPolicy
// ---------------------------------------------------------------------------

/// A retry policy that controls how many times an action may be retried and
/// how long to wait between attempts.
#[derive(Debug, Clone)]
pub struct RetryPolicy {
    pub max_retries: u32,
    pub strategy: BackoffStrategy,
    pub base_delay_ms: u64,
}

impl RetryPolicy {
    pub fn new(max_retries: u32, strategy: BackoffStrategy, base_delay_ms: u64) -> Self {
        RetryPolicy {
            max_retries,
            strategy,
            base_delay_ms,
        }
    }

    /// Whether the given attempt number (0-indexed) is within the retry budget.
    pub fn should_retry(&self, attempt: u32) -> bool {
        attempt < self.max_retries
    }

    /// Compute the delay in milliseconds before the given attempt.
    ///
    /// Attempt 0 is the first retry (after the initial failure).
    pub fn delay_ms(&self, attempt: u32) -> u64 {
        match self.strategy {
            BackoffStrategy::Fixed => self.base_delay_ms,
            BackoffStrategy::Linear => self.base_delay_ms * (attempt as u64 + 1),
            BackoffStrategy::Exponential => {
                self.base_delay_ms * 2u64.saturating_pow(attempt)
            }
        }
    }
}

impl Default for RetryPolicy {
    /// Default: 3 retries, exponential backoff, 1000ms base delay.
    fn default() -> Self {
        RetryPolicy {
            max_retries: 3,
            strategy: BackoffStrategy::Exponential,
            base_delay_ms: 1000,
        }
    }
}

// ---------------------------------------------------------------------------
// ActionRetryTracker
// ---------------------------------------------------------------------------

/// Per-action state used by the tracker.
#[derive(Debug, Clone)]
struct ActionRetryState {
    failures: u32,
    succeeded: bool,
}

/// Tracks retry state for individual actions, keyed by a string identifier.
#[derive(Debug, Clone)]
pub struct ActionRetryTracker {
    policy: RetryPolicy,
    states: HashMap<String, ActionRetryState>,
}

impl ActionRetryTracker {
    pub fn new(policy: RetryPolicy) -> Self {
        ActionRetryTracker {
            policy,
            states: HashMap::new(),
        }
    }

    /// Record a failure for the given action key.
    pub fn record_failure(&mut self, action_key: &str) {
        let entry = self
            .states
            .entry(action_key.to_string())
            .or_insert(ActionRetryState {
                failures: 0,
                succeeded: false,
            });
        entry.failures += 1;
    }

    /// Record a success for the given action key. Resets failure count.
    pub fn record_success(&mut self, action_key: &str) {
        let entry = self
            .states
            .entry(action_key.to_string())
            .or_insert(ActionRetryState {
                failures: 0,
                succeeded: false,
            });
        entry.succeeded = true;
        entry.failures = 0;
    }

    /// Whether the action can still be retried given its failure count.
    pub fn can_retry(&self, action_key: &str) -> bool {
        match self.states.get(action_key) {
            None => true, // never attempted
            Some(state) => {
                if state.succeeded {
                    return false; // already succeeded, no retry needed
                }
                self.policy.should_retry(state.failures)
            }
        }
    }

    /// The delay in milliseconds before the next retry for this action.
    /// Returns 0 if no failures recorded yet.
    pub fn next_delay_ms(&self, action_key: &str) -> u64 {
        match self.states.get(action_key) {
            None => 0,
            Some(state) => {
                if state.failures == 0 {
                    return 0;
                }
                self.policy.delay_ms(state.failures - 1)
            }
        }
    }

    /// The number of failures recorded for the given action key.
    pub fn failure_count(&self, action_key: &str) -> u32 {
        self.states
            .get(action_key)
            .map(|s| s.failures)
            .unwrap_or(0)
    }

    /// Remove tracking state for the given action key.
    pub fn clear(&mut self, action_key: &str) {
        self.states.remove(action_key);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_policy() {
        let p = RetryPolicy::default();
        assert_eq!(p.max_retries, 3);
        assert_eq!(p.base_delay_ms, 1000);
        assert!(matches!(p.strategy, BackoffStrategy::Exponential));
    }

    #[test]
    fn should_retry_within_budget() {
        let p = RetryPolicy::new(3, BackoffStrategy::Fixed, 100);
        assert!(p.should_retry(0));
        assert!(p.should_retry(1));
        assert!(p.should_retry(2));
        assert!(!p.should_retry(3));
        assert!(!p.should_retry(4));
    }

    #[test]
    fn fixed_delay() {
        let p = RetryPolicy::new(3, BackoffStrategy::Fixed, 500);
        assert_eq!(p.delay_ms(0), 500);
        assert_eq!(p.delay_ms(1), 500);
        assert_eq!(p.delay_ms(5), 500);
    }

    #[test]
    fn linear_delay() {
        let p = RetryPolicy::new(5, BackoffStrategy::Linear, 1000);
        assert_eq!(p.delay_ms(0), 1000);
        assert_eq!(p.delay_ms(1), 2000);
        assert_eq!(p.delay_ms(2), 3000);
    }

    #[test]
    fn exponential_delay() {
        let p = RetryPolicy::new(5, BackoffStrategy::Exponential, 1000);
        assert_eq!(p.delay_ms(0), 1000);
        assert_eq!(p.delay_ms(1), 2000);
        assert_eq!(p.delay_ms(2), 4000);
        assert_eq!(p.delay_ms(3), 8000);
    }

    #[test]
    fn tracker_can_retry_fresh() {
        let tracker = ActionRetryTracker::new(RetryPolicy::default());
        assert!(tracker.can_retry("action1"));
    }

    #[test]
    fn tracker_records_failures() {
        let mut tracker = ActionRetryTracker::new(RetryPolicy::new(2, BackoffStrategy::Fixed, 100));
        tracker.record_failure("a1");
        assert!(tracker.can_retry("a1"));
        assert_eq!(tracker.failure_count("a1"), 1);

        tracker.record_failure("a1");
        assert!(!tracker.can_retry("a1"));
        assert_eq!(tracker.failure_count("a1"), 2);
    }

    #[test]
    fn tracker_success_resets() {
        let mut tracker = ActionRetryTracker::new(RetryPolicy::new(2, BackoffStrategy::Fixed, 100));
        tracker.record_failure("a1");
        tracker.record_success("a1");
        assert!(!tracker.can_retry("a1")); // succeeded, no retry needed
        assert_eq!(tracker.failure_count("a1"), 0);
    }

    #[test]
    fn tracker_delay_ms() {
        let mut tracker =
            ActionRetryTracker::new(RetryPolicy::new(5, BackoffStrategy::Exponential, 1000));
        assert_eq!(tracker.next_delay_ms("a1"), 0); // never failed
        tracker.record_failure("a1");
        assert_eq!(tracker.next_delay_ms("a1"), 1000); // first failure -> delay_ms(0)
        tracker.record_failure("a1");
        assert_eq!(tracker.next_delay_ms("a1"), 2000); // second failure -> delay_ms(1)
    }

    #[test]
    fn tracker_clear() {
        let mut tracker = ActionRetryTracker::new(RetryPolicy::default());
        tracker.record_failure("a1");
        tracker.clear("a1");
        assert!(tracker.can_retry("a1"));
        assert_eq!(tracker.failure_count("a1"), 0);
    }

    #[test]
    fn tracker_independent_keys() {
        let mut tracker = ActionRetryTracker::new(RetryPolicy::new(1, BackoffStrategy::Fixed, 100));
        tracker.record_failure("a1");
        assert!(!tracker.can_retry("a1"));
        assert!(tracker.can_retry("a2"));
    }
}
