//! Browsing, diffing, and restoring configuration history snapshots.
//!
//! Provides line-based diffing between snapshots and safe restore
//! operations that preserve the pre-restore state in history.

use std::collections::HashSet;

use super::snapshot::{HistoryEntry, HistoryError};

// ---------------------------------------------------------------------------
// HistoryDiff
// ---------------------------------------------------------------------------

/// A simple text-level diff between two configuration snapshots.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryDiff {
    pub from: HistoryEntry,
    pub to: HistoryEntry,
    pub added_lines: Vec<String>,
    pub removed_lines: Vec<String>,
    pub summary: String,
}

// ---------------------------------------------------------------------------
// Diffing
// ---------------------------------------------------------------------------

/// Compute a line-based diff between two strings.
///
/// Returns `(added_lines, removed_lines)` where `added_lines` are lines
/// present in `to_content` but not in `from_content`, and `removed_lines`
/// are lines present in `from_content` but not in `to_content`.
pub fn line_diff(from_content: &str, to_content: &str) -> (Vec<String>, Vec<String>) {
    let from_lines: Vec<&str> = from_content.lines().collect();
    let to_lines: Vec<&str> = to_content.lines().collect();

    let from_set: HashSet<&str> = from_lines.iter().copied().collect();
    let to_set: HashSet<&str> = to_lines.iter().copied().collect();

    let added: Vec<String> = to_lines
        .iter()
        .filter(|line| !from_set.contains(**line))
        .map(|line| line.to_string())
        .collect();

    let removed: Vec<String> = from_lines
        .iter()
        .filter(|line| !to_set.contains(**line))
        .map(|line| line.to_string())
        .collect();

    (added, removed)
}

/// Generate a human-readable summary of configuration changes.
///
/// Scans the added and removed lines for known configuration patterns
/// (agent lines, session headers, layout expressions) and produces a
/// concise description.
pub fn summarize_changes(added: &[String], removed: &[String]) -> String {
    let mut parts = Vec::new();

    // Count agent-related changes.
    let agents_added = added
        .iter()
        .filter(|l| looks_like_agent_line(l))
        .count();
    let agents_removed = removed
        .iter()
        .filter(|l| looks_like_agent_line(l))
        .count();

    if agents_added > 0 {
        parts.push(format!(
            "{} agent{} added",
            agents_added,
            if agents_added == 1 { "" } else { "s" }
        ));
    }
    if agents_removed > 0 {
        parts.push(format!(
            "{} agent{} removed",
            agents_removed,
            if agents_removed == 1 { "" } else { "s" }
        ));
    }

    // Count session-related changes.
    let sessions_added = added
        .iter()
        .filter(|l| looks_like_session_line(l))
        .count();
    let sessions_removed = removed
        .iter()
        .filter(|l| looks_like_session_line(l))
        .count();

    if sessions_added > 0 {
        parts.push(format!(
            "{} session{} added",
            sessions_added,
            if sessions_added == 1 { "" } else { "s" }
        ));
    }
    if sessions_removed > 0 {
        parts.push(format!(
            "{} session{} removed",
            sessions_removed,
            if sessions_removed == 1 { "" } else { "s" }
        ));
    }

    // Count layout-related changes.
    let layouts_changed = added
        .iter()
        .chain(removed.iter())
        .filter(|l| looks_like_layout_line(l))
        .count();

    if layouts_changed > 0 {
        parts.push(format!(
            "{} layout{} changed",
            layouts_changed,
            if layouts_changed == 1 { "" } else { "s" }
        ));
    }

    if parts.is_empty() {
        let total = added.len() + removed.len();
        format!(
            "{} line{} changed",
            total,
            if total == 1 { "" } else { "s" }
        )
    } else {
        parts.join(", ")
    }
}

// ---------------------------------------------------------------------------
// Pattern detection helpers
// ---------------------------------------------------------------------------

/// Heuristic: does this line look like an agent definition?
fn looks_like_agent_line(line: &str) -> bool {
    let trimmed = line.trim().to_lowercase();
    trimmed.starts_with("- agent:")
        || trimmed.starts_with("agent:")
        || (trimmed.starts_with("- ") && trimmed.contains("role:"))
}

/// Heuristic: does this line look like a session header?
fn looks_like_session_line(line: &str) -> bool {
    let trimmed = line.trim().to_lowercase();
    trimmed.starts_with("## session")
        || trimmed.starts_with("session:")
        || (trimmed.starts_with("- ") && trimmed.contains("session:"))
}

/// Heuristic: does this line look like a layout expression?
fn looks_like_layout_line(line: &str) -> bool {
    let trimmed = line.trim().to_lowercase();
    trimmed.starts_with("layout:")
        || trimmed.contains("layout-expr:")
        || trimmed.contains("layout_expr:")
}

// ---------------------------------------------------------------------------
// Range filtering
// ---------------------------------------------------------------------------

/// Filter entries to those within a time range (inclusive).
pub fn filter_range(
    entries: &[HistoryEntry],
    from_ms: u64,
    to_ms: u64,
) -> Vec<HistoryEntry> {
    entries
        .iter()
        .filter(|e| e.timestamp_ms >= from_ms && e.timestamp_ms <= to_ms)
        .cloned()
        .collect()
}

/// Compute a `HistoryDiff` by reading both entries and comparing their content.
pub fn compute_diff(
    from: &HistoryEntry,
    to: &HistoryEntry,
    from_content: &str,
    to_content: &str,
) -> HistoryDiff {
    let (added, removed) = line_diff(from_content, to_content);
    let summary = summarize_changes(&added, &removed);

    HistoryDiff {
        from: from.clone(),
        to: to.clone(),
        added_lines: added,
        removed_lines: removed,
        summary,
    }
}

/// Perform a restore operation: snapshot current state, then overwrite config.
///
/// This is the core logic; the `HistoryManager` method wraps this with
/// path resolution.
pub fn restore_config(
    config_path: &std::path::Path,
    history_dir: &std::path::Path,
    entry: &HistoryEntry,
    now_ms: u64,
) -> Result<(), HistoryError> {
    // 1. Read the historical content.
    let historical_content = super::snapshot::read_snapshot(entry)?;

    // 2. Snapshot the current state (if the config file exists).
    if config_path.exists() {
        let current_content = std::fs::read_to_string(config_path)?;
        let current_hash = super::snapshot::content_hash(&current_content);

        // Only snapshot if it differs from the most recent history entry.
        let should_snapshot = match super::snapshot::latest_entry(history_dir)? {
            Some(latest) => {
                let latest_content = super::snapshot::read_snapshot(&latest)?;
                let latest_hash = super::snapshot::content_hash(&latest_content);
                current_hash != latest_hash
            }
            None => true,
        };

        if should_snapshot {
            super::snapshot::create_snapshot(history_dir, &current_content, now_ms)?;
        }
    }

    // 3. Overwrite the config file with historical content.
    std::fs::write(config_path, &historical_content).map_err(|e| {
        HistoryError::RestoreFailed(format!(
            "failed to write {}: {}",
            config_path.display(),
            e
        ))
    })?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::snapshot::{compose_timestamp, create_snapshot};
    use std::path::PathBuf;

    fn make_entry_at(ts_ms: u64) -> HistoryEntry {
        use super::super::snapshot::timestamp_to_filename;
        let filename = timestamp_to_filename(ts_ms);
        HistoryEntry {
            timestamp_ms: ts_ms,
            filename,
            path: PathBuf::from("/tmp/test"),
            size_bytes: 0,
        }
    }

    #[test]
    fn line_diff_added_lines() {
        let from = "line1\nline2\n";
        let to = "line1\nline2\nline3\n";
        let (added, removed) = line_diff(from, to);
        assert_eq!(added, vec!["line3"]);
        assert!(removed.is_empty());
    }

    #[test]
    fn line_diff_removed_lines() {
        let from = "line1\nline2\nline3\n";
        let to = "line1\nline2\n";
        let (added, removed) = line_diff(from, to);
        assert!(added.is_empty());
        assert_eq!(removed, vec!["line3"]);
    }

    #[test]
    fn line_diff_both_added_and_removed() {
        let from = "alpha\nbeta\n";
        let to = "alpha\ngamma\n";
        let (added, removed) = line_diff(from, to);
        assert_eq!(added, vec!["gamma"]);
        assert_eq!(removed, vec!["beta"]);
    }

    #[test]
    fn line_diff_identical() {
        let content = "same\ncontent\n";
        let (added, removed) = line_diff(content, content);
        assert!(added.is_empty());
        assert!(removed.is_empty());
    }

    #[test]
    fn line_diff_empty_strings() {
        let (added, removed) = line_diff("", "");
        assert!(added.is_empty());
        assert!(removed.is_empty());
    }

    #[test]
    fn summarize_agent_changes() {
        let added = vec!["- agent: worker-2, role: worker".to_string()];
        let removed = vec![];
        let summary = summarize_changes(&added, &removed);
        assert!(summary.contains("1 agent added"), "got: {}", summary);
    }

    #[test]
    fn summarize_session_changes() {
        let added = vec!["## Session: dev-session".to_string()];
        let removed = vec!["## Session: old-session".to_string()];
        let summary = summarize_changes(&added, &removed);
        assert!(summary.contains("session"), "got: {}", summary);
    }

    #[test]
    fn summarize_layout_changes() {
        let added = vec!["layout: tiled".to_string()];
        let removed = vec![];
        let summary = summarize_changes(&added, &removed);
        assert!(summary.contains("layout"), "got: {}", summary);
    }

    #[test]
    fn summarize_generic_changes() {
        let added = vec!["some random line".to_string()];
        let removed = vec!["another random line".to_string()];
        let summary = summarize_changes(&added, &removed);
        assert!(summary.contains("changed"), "got: {}", summary);
    }

    #[test]
    fn filter_range_inclusive() {
        let entries = vec![
            make_entry_at(1000),
            make_entry_at(2000),
            make_entry_at(3000),
            make_entry_at(4000),
        ];
        let filtered = filter_range(&entries, 2000, 3000);
        assert_eq!(filtered.len(), 2);
        assert_eq!(filtered[0].timestamp_ms, 2000);
        assert_eq!(filtered[1].timestamp_ms, 3000);
    }

    #[test]
    fn filter_range_empty() {
        let entries = vec![make_entry_at(1000), make_entry_at(5000)];
        let filtered = filter_range(&entries, 2000, 4000);
        assert!(filtered.is_empty());
    }

    #[test]
    fn compute_diff_produces_correct_output() {
        let from = make_entry_at(1000);
        let to = make_entry_at(2000);
        let diff = compute_diff(&from, &to, "line1\nline2\n", "line1\nline3\n");
        assert_eq!(diff.added_lines, vec!["line3"]);
        assert_eq!(diff.removed_lines, vec!["line2"]);
        assert_eq!(diff.from.timestamp_ms, 1000);
        assert_eq!(diff.to.timestamp_ms, 2000);
    }

    #[test]
    fn restore_snapshots_current_then_overwrites() {
        let dir = std::env::temp_dir().join("cmx_hist_test_restore");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let config_path = dir.join("Current Configuration.md");
        let history_dir = dir.join("history");

        // Write initial config.
        std::fs::write(&config_path, "current state\n").unwrap();

        // Create a historical snapshot to restore.
        let old_ts = compose_timestamp(2026, 2, 20, 10, 0, 0) * 1000;
        let old_entry = create_snapshot(&history_dir, "historical state\n", old_ts).unwrap();

        // Restore.
        let now = compose_timestamp(2026, 2, 22, 12, 0, 0) * 1000;
        restore_config(&config_path, &history_dir, &old_entry, now).unwrap();

        // Config should now contain the historical content.
        let restored = std::fs::read_to_string(&config_path).unwrap();
        assert_eq!(restored, "historical state\n");

        // History should contain a snapshot of the pre-restore state.
        let entries = super::super::snapshot::list_entries(&history_dir).unwrap();
        assert!(entries.len() >= 2, "expected at least 2 entries, got {}", entries.len());

        // Find the pre-restore snapshot.
        let pre_restore = entries.iter().find(|e| e.timestamp_ms == now).unwrap();
        let pre_content = super::super::snapshot::read_snapshot(pre_restore).unwrap();
        assert_eq!(pre_content, "current state\n");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn restore_with_missing_config_file() {
        let dir = std::env::temp_dir().join("cmx_hist_test_restore_missing");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let config_path = dir.join("Current Configuration.md");
        let history_dir = dir.join("history");

        // Create a historical snapshot.
        let old_ts = compose_timestamp(2026, 2, 20, 10, 0, 0) * 1000;
        let old_entry = create_snapshot(&history_dir, "restored content\n", old_ts).unwrap();

        // Restore without existing config file.
        let now = compose_timestamp(2026, 2, 22, 12, 0, 0) * 1000;
        restore_config(&config_path, &history_dir, &old_entry, now).unwrap();

        let restored = std::fs::read_to_string(&config_path).unwrap();
        assert_eq!(restored, "restored content\n");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn restore_content_matches_historical() {
        let dir = std::env::temp_dir().join("cmx_hist_test_restore_match");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let config_path = dir.join("Current Configuration.md");
        let history_dir = dir.join("history");

        std::fs::write(&config_path, "before\n").unwrap();

        let historical = "# CMX Configuration\n\n## Agents\n- pilot: active\n";
        let old_ts = compose_timestamp(2026, 1, 15, 8, 0, 0) * 1000;
        let old_entry = create_snapshot(&history_dir, historical, old_ts).unwrap();

        let now = compose_timestamp(2026, 2, 22, 12, 0, 0) * 1000;
        restore_config(&config_path, &history_dir, &old_entry, now).unwrap();

        let result = std::fs::read_to_string(&config_path).unwrap();
        assert_eq!(result, historical);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
