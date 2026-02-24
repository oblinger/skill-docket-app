//! Configuration history — timestamped snapshots of `Current Configuration.md`.
//!
//! This module maintains a `history/` folder alongside the live configuration
//! file, containing timestamped copies. A retention policy prunes old
//! snapshots automatically: hourly resolution for the last 24 hours, daily
//! for one week, weekly beyond that.
//!
//! # Usage
//!
//! ```ignore
//! let mgr = HistoryManager::with_defaults(config_dir)?;
//! if let Some(entry) = mgr.maybe_snapshot(now_ms)? {
//!     mgr.prune(now_ms)?;
//! }
//! ```

pub mod browse;
pub mod retention;
pub mod snapshot;

pub use browse::HistoryDiff;
pub use retention::RetentionPolicy;
pub use snapshot::{HistoryEntry, HistoryError};

use std::path::PathBuf;

// ---------------------------------------------------------------------------
// HistoryManager
// ---------------------------------------------------------------------------

/// Main interface for configuration history operations.
///
/// Manages timestamped copies of `Current Configuration.md` in a `history/`
/// subdirectory. Snapshots are deduplicated by content hash, and old entries
/// are pruned according to a configurable retention policy.
pub struct HistoryManager {
    history_dir: PathBuf,
    config_path: PathBuf,
    policy: RetentionPolicy,
}

impl HistoryManager {
    /// Create a new HistoryManager.
    ///
    /// `config_dir` is the CMX configuration directory (e.g., `~/.config/cmx/`).
    /// Creates the `history/` subdirectory if it doesn't exist.
    pub fn new(
        config_dir: PathBuf,
        policy: RetentionPolicy,
    ) -> Result<HistoryManager, HistoryError> {
        let history_dir = config_dir.join("history");
        let config_path = config_dir.join("Current Configuration.md");
        std::fs::create_dir_all(&history_dir)?;

        Ok(HistoryManager {
            history_dir,
            config_path,
            policy,
        })
    }

    /// Create with default retention policy.
    pub fn with_defaults(config_dir: PathBuf) -> Result<HistoryManager, HistoryError> {
        Self::new(config_dir, RetentionPolicy::default())
    }

    /// Create a history snapshot if the config has changed since the last one.
    ///
    /// Returns `Some(entry)` if a new snapshot was created, `None` if the
    /// configuration is unchanged (or missing).
    pub fn maybe_snapshot(&self, now_ms: u64) -> Result<Option<HistoryEntry>, HistoryError> {
        // Read current config.
        if !self.config_path.exists() {
            return Ok(None);
        }

        let current_content = std::fs::read_to_string(&self.config_path)?;
        let current_hash = snapshot::content_hash(&current_content);

        // Compare with the most recent history entry.
        if let Some(latest) = snapshot::latest_entry(&self.history_dir)? {
            let latest_content = snapshot::read_snapshot(&latest)?;
            let latest_hash = snapshot::content_hash(&latest_content);
            if current_hash == latest_hash {
                return Ok(None); // No change.
            }
        }

        // Content changed — create a new snapshot.
        let entry = snapshot::create_snapshot(&self.history_dir, &current_content, now_ms)?;
        Ok(Some(entry))
    }

    /// Prune old snapshots according to the retention policy.
    ///
    /// Returns the number of entries deleted.
    pub fn prune(&self, now_ms: u64) -> Result<usize, HistoryError> {
        let entries = snapshot::list_entries(&self.history_dir)?;
        retention::prune_entries(&entries, now_ms, &self.policy)
    }

    /// List all history entries, newest first.
    pub fn list(&self) -> Result<Vec<HistoryEntry>, HistoryError> {
        snapshot::list_entries(&self.history_dir)
    }

    /// List entries within a time range.
    pub fn list_range(
        &self,
        from_ms: u64,
        to_ms: u64,
    ) -> Result<Vec<HistoryEntry>, HistoryError> {
        let entries = snapshot::list_entries(&self.history_dir)?;
        Ok(browse::filter_range(&entries, from_ms, to_ms))
    }

    /// Read the content of a history entry.
    pub fn read(&self, entry: &HistoryEntry) -> Result<String, HistoryError> {
        snapshot::read_snapshot(entry)
    }

    /// Compute a line-based diff between two entries.
    pub fn diff(
        &self,
        from: &HistoryEntry,
        to: &HistoryEntry,
    ) -> Result<HistoryDiff, HistoryError> {
        let from_content = snapshot::read_snapshot(from)?;
        let to_content = snapshot::read_snapshot(to)?;
        Ok(browse::compute_diff(from, to, &from_content, &to_content))
    }

    /// Restore a history entry as the current configuration.
    ///
    /// Takes a snapshot of the current state first (so the pre-restore state
    /// is preserved), then overwrites `Current Configuration.md` with the
    /// historical content.
    pub fn restore(
        &self,
        entry: &HistoryEntry,
        now_ms: u64,
    ) -> Result<(), HistoryError> {
        browse::restore_config(&self.config_path, &self.history_dir, entry, now_ms)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use snapshot::compose_timestamp;

    /// Create a temp directory with a unique suffix for test isolation.
    fn test_dir(suffix: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("cmx_hist_mgr_{}", suffix));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn new_creates_history_directory() {
        let dir = test_dir("new_creates_dir");
        let mgr = HistoryManager::with_defaults(dir.clone()).unwrap();
        assert!(mgr.history_dir.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn maybe_snapshot_creates_entry_on_change() {
        let dir = test_dir("snap_creates");
        let config = dir.join("Current Configuration.md");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&config, "# Config v1\nagent: pilot\n").unwrap();

        let mgr = HistoryManager::with_defaults(dir.clone()).unwrap();
        let now = compose_timestamp(2026, 2, 22, 10, 0, 0) * 1000;
        let result = mgr.maybe_snapshot(now).unwrap();

        assert!(result.is_some());
        let entry = result.unwrap();
        assert_eq!(entry.filename, "2026-02-22T10-00-00.md");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn maybe_snapshot_skips_when_unchanged() {
        let dir = test_dir("snap_skips");
        let config = dir.join("Current Configuration.md");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&config, "# Config\n").unwrap();

        let mgr = HistoryManager::with_defaults(dir.clone()).unwrap();
        let ts1 = compose_timestamp(2026, 2, 22, 10, 0, 0) * 1000;
        let ts2 = compose_timestamp(2026, 2, 22, 10, 1, 0) * 1000;

        let r1 = mgr.maybe_snapshot(ts1).unwrap();
        assert!(r1.is_some());

        // Same content — should skip.
        let r2 = mgr.maybe_snapshot(ts2).unwrap();
        assert!(r2.is_none());

        let entries = mgr.list().unwrap();
        assert_eq!(entries.len(), 1);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn maybe_snapshot_creates_on_content_change() {
        let dir = test_dir("snap_change");
        let config = dir.join("Current Configuration.md");
        std::fs::create_dir_all(&dir).unwrap();

        std::fs::write(&config, "version 1\n").unwrap();
        let mgr = HistoryManager::with_defaults(dir.clone()).unwrap();

        let ts1 = compose_timestamp(2026, 2, 22, 10, 0, 0) * 1000;
        mgr.maybe_snapshot(ts1).unwrap();

        // Change content.
        std::fs::write(&config, "version 2\n").unwrap();
        let ts2 = compose_timestamp(2026, 2, 22, 10, 5, 0) * 1000;
        let r2 = mgr.maybe_snapshot(ts2).unwrap();
        assert!(r2.is_some());

        let entries = mgr.list().unwrap();
        assert_eq!(entries.len(), 2);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn maybe_snapshot_returns_none_when_config_missing() {
        let dir = test_dir("snap_no_config");
        let mgr = HistoryManager::with_defaults(dir.clone()).unwrap();
        let now = compose_timestamp(2026, 2, 22, 10, 0, 0) * 1000;
        let result = mgr.maybe_snapshot(now).unwrap();
        assert!(result.is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn list_returns_newest_first() {
        let dir = test_dir("list_order");
        let config = dir.join("Current Configuration.md");
        std::fs::create_dir_all(&dir).unwrap();

        let mgr = HistoryManager::with_defaults(dir.clone()).unwrap();

        // Create snapshots with different content.
        let ts1 = compose_timestamp(2026, 2, 22, 8, 0, 0) * 1000;
        let ts2 = compose_timestamp(2026, 2, 22, 9, 0, 0) * 1000;
        let ts3 = compose_timestamp(2026, 2, 22, 10, 0, 0) * 1000;

        std::fs::write(&config, "v1\n").unwrap();
        mgr.maybe_snapshot(ts1).unwrap();

        std::fs::write(&config, "v2\n").unwrap();
        mgr.maybe_snapshot(ts2).unwrap();

        std::fs::write(&config, "v3\n").unwrap();
        mgr.maybe_snapshot(ts3).unwrap();

        let entries = mgr.list().unwrap();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].timestamp_ms, ts3);
        assert_eq!(entries[1].timestamp_ms, ts2);
        assert_eq!(entries[2].timestamp_ms, ts1);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn list_range_filters_correctly() {
        let dir = test_dir("list_range");
        let config = dir.join("Current Configuration.md");
        std::fs::create_dir_all(&dir).unwrap();

        let mgr = HistoryManager::with_defaults(dir.clone()).unwrap();

        let ts1 = compose_timestamp(2026, 2, 20, 10, 0, 0) * 1000;
        let ts2 = compose_timestamp(2026, 2, 21, 10, 0, 0) * 1000;
        let ts3 = compose_timestamp(2026, 2, 22, 10, 0, 0) * 1000;

        std::fs::write(&config, "a\n").unwrap();
        mgr.maybe_snapshot(ts1).unwrap();
        std::fs::write(&config, "b\n").unwrap();
        mgr.maybe_snapshot(ts2).unwrap();
        std::fs::write(&config, "c\n").unwrap();
        mgr.maybe_snapshot(ts3).unwrap();

        let range = mgr.list_range(ts1, ts2).unwrap();
        assert_eq!(range.len(), 2);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn diff_produces_correct_added_removed() {
        let dir = test_dir("diff_test");
        let history_dir = dir.join("history");
        std::fs::create_dir_all(&dir).unwrap();

        let mgr = HistoryManager::with_defaults(dir.clone()).unwrap();

        let ts1 = compose_timestamp(2026, 2, 22, 10, 0, 0) * 1000;
        let ts2 = compose_timestamp(2026, 2, 22, 11, 0, 0) * 1000;

        let e1 = snapshot::create_snapshot(&history_dir, "alpha\nbeta\n", ts1).unwrap();
        let e2 = snapshot::create_snapshot(&history_dir, "alpha\ngamma\n", ts2).unwrap();

        let diff = mgr.diff(&e1, &e2).unwrap();
        assert_eq!(diff.added_lines, vec!["gamma"]);
        assert_eq!(diff.removed_lines, vec!["beta"]);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn restore_preserves_pre_restore_state() {
        let dir = test_dir("restore_preserve");
        let config = dir.join("Current Configuration.md");
        std::fs::create_dir_all(&dir).unwrap();

        let mgr = HistoryManager::with_defaults(dir.clone()).unwrap();

        // Write initial config and snapshot it.
        std::fs::write(&config, "current state\n").unwrap();
        let ts1 = compose_timestamp(2026, 2, 22, 10, 0, 0) * 1000;
        mgr.maybe_snapshot(ts1).unwrap();

        // Change config and snapshot.
        std::fs::write(&config, "updated state\n").unwrap();
        let ts2 = compose_timestamp(2026, 2, 22, 11, 0, 0) * 1000;
        mgr.maybe_snapshot(ts2).unwrap();

        // Change config again WITHOUT snapshotting, so restore will
        // detect a difference and preserve this pre-restore state.
        std::fs::write(&config, "latest unsaved state\n").unwrap();

        // Restore the first snapshot.
        let entries = mgr.list().unwrap();
        let oldest = entries.last().unwrap();
        let restore_ts = compose_timestamp(2026, 2, 22, 12, 0, 0) * 1000;
        mgr.restore(oldest, restore_ts).unwrap();

        // Config should be restored to the oldest content.
        let result = std::fs::read_to_string(&config).unwrap();
        assert_eq!(result, "current state\n");

        // History should have 3 entries: ts1, ts2, and the pre-restore snapshot
        // of "latest unsaved state".
        let all = mgr.list().unwrap();
        assert!(all.len() >= 3, "expected >= 3 entries, got {}", all.len());

        // The newest entry (pre-restore snapshot) should contain the unsaved state.
        let newest = &all[0];
        let newest_content = mgr.read(newest).unwrap();
        assert_eq!(newest_content, "latest unsaved state\n");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn restore_content_matches_historical() {
        let dir = test_dir("restore_match");
        let config = dir.join("Current Configuration.md");
        let history_dir = dir.join("history");
        std::fs::create_dir_all(&dir).unwrap();

        let mgr = HistoryManager::with_defaults(dir.clone()).unwrap();

        // Create a historical snapshot directly.
        let historical = "# Configuration\n\n## Agents\n- pilot: active\n- worker-1: idle\n";
        let old_ts = compose_timestamp(2026, 1, 15, 8, 0, 0) * 1000;
        let old_entry = snapshot::create_snapshot(&history_dir, historical, old_ts).unwrap();

        // Write different current config.
        std::fs::write(&config, "different content\n").unwrap();

        let now = compose_timestamp(2026, 2, 22, 12, 0, 0) * 1000;
        mgr.restore(&old_entry, now).unwrap();

        let result = std::fs::read_to_string(&config).unwrap();
        assert_eq!(result, historical);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn prune_after_snapshot() {
        let dir = test_dir("prune_test");
        let config = dir.join("Current Configuration.md");
        std::fs::create_dir_all(&dir).unwrap();

        let policy = RetentionPolicy {
            max_total: Some(3),
            ..Default::default()
        };
        let mgr = HistoryManager::new(dir.clone(), policy).unwrap();

        // Create 5 snapshots with different content, each in a different hour.
        for i in 0..5u64 {
            let content = format!("version {}\n", i);
            std::fs::write(&config, &content).unwrap();
            let ts = compose_timestamp(2026, 2, 22, 8 + i, 0, 0) * 1000;
            mgr.maybe_snapshot(ts).unwrap();
        }

        let before = mgr.list().unwrap();
        assert_eq!(before.len(), 5);

        let now = compose_timestamp(2026, 2, 22, 13, 0, 0) * 1000;
        let deleted = mgr.prune(now).unwrap();

        assert_eq!(deleted, 2);
        let after = mgr.list().unwrap();
        assert_eq!(after.len(), 3);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn with_defaults_uses_default_policy() {
        let dir = test_dir("with_defaults");
        let mgr = HistoryManager::with_defaults(dir.clone()).unwrap();
        assert_eq!(mgr.policy.hourly_window_hours, 24);
        assert_eq!(mgr.policy.daily_window_days, 7);
        assert!(mgr.policy.weekly_beyond);
        assert!(mgr.policy.max_total.is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn read_entry_content() {
        let dir = test_dir("read_entry");
        let history_dir = dir.join("history");
        std::fs::create_dir_all(&dir).unwrap();

        let mgr = HistoryManager::with_defaults(dir.clone()).unwrap();

        let content = "# Test Content\nline 2\n";
        let ts = compose_timestamp(2026, 2, 22, 10, 0, 0) * 1000;
        let entry = snapshot::create_snapshot(&history_dir, content, ts).unwrap();

        let read_back = mgr.read(&entry).unwrap();
        assert_eq!(read_back, content);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
