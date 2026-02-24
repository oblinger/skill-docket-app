//! Pruning policy for configuration history snapshots.
//!
//! Implements a tiered retention strategy: hourly resolution for recent
//! snapshots, daily for the medium term, and weekly for long-term history.
//! An optional hard cap limits total snapshot count.

use serde::{Deserialize, Serialize};

use super::snapshot::{HistoryEntry, HistoryError};
use std::collections::HashMap;
use std::fs;

// ---------------------------------------------------------------------------
// RetentionPolicy
// ---------------------------------------------------------------------------

/// Defines how aggressively old snapshots are pruned.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RetentionPolicy {
    /// Keep hourly snapshots for this many hours (default: 24).
    pub hourly_window_hours: u32,
    /// Keep daily snapshots for this many days (default: 7).
    pub daily_window_days: u32,
    /// Keep weekly snapshots beyond the daily window (default: true).
    pub weekly_beyond: bool,
    /// Hard cap on total snapshots (default: None).
    pub max_total: Option<usize>,
}

impl Default for RetentionPolicy {
    fn default() -> Self {
        RetentionPolicy {
            hourly_window_hours: 24,
            daily_window_days: 7,
            weekly_beyond: true,
            max_total: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Time slot helpers
// ---------------------------------------------------------------------------

const MS_PER_HOUR: u64 = 3_600_000;
const MS_PER_DAY: u64 = 86_400_000;
const MS_PER_WEEK: u64 = 604_800_000;

/// Truncate a millisecond timestamp to the start of its hour.
fn hourly_slot(ms: u64) -> u64 {
    (ms / MS_PER_HOUR) * MS_PER_HOUR
}

/// Truncate a millisecond timestamp to midnight UTC of its day.
fn daily_slot(ms: u64) -> u64 {
    (ms / MS_PER_DAY) * MS_PER_DAY
}

/// Truncate a millisecond timestamp to Monday midnight UTC of its week.
///
/// The Unix epoch (1970-01-01) was a Thursday. Monday of that week is
/// 1969-12-29, which is -3 days from epoch. We offset by 3 days (in ms)
/// before truncating to a 7-day boundary, then subtract the offset back.
fn weekly_slot(ms: u64) -> u64 {
    // Thursday offset: epoch is day 4 of the week (Mon=0). Shift by 3 days.
    let offset = 3 * MS_PER_DAY;
    let shifted = ms + offset;
    let truncated = (shifted / MS_PER_WEEK) * MS_PER_WEEK;
    truncated.saturating_sub(offset)
}

// ---------------------------------------------------------------------------
// Pruning
// ---------------------------------------------------------------------------

/// Determine which entries to keep and which to delete based on the policy.
///
/// `entries` must be sorted newest-first. `now_ms` is the current timestamp.
/// Returns the list of entries to delete.
pub fn entries_to_prune(
    entries: &[HistoryEntry],
    now_ms: u64,
    policy: &RetentionPolicy,
) -> Vec<HistoryEntry> {
    let hourly_cutoff = now_ms.saturating_sub(policy.hourly_window_hours as u64 * MS_PER_HOUR);
    let daily_cutoff = now_ms.saturating_sub(policy.daily_window_days as u64 * MS_PER_DAY);

    // Track which time slots we've already kept an entry for.
    // Key: slot timestamp, Value: true if we've kept one for this slot.
    let mut hourly_slots: HashMap<u64, bool> = HashMap::new();
    let mut daily_slots: HashMap<u64, bool> = HashMap::new();
    let mut weekly_slots: HashMap<u64, bool> = HashMap::new();

    let mut keep_indices = Vec::new();

    // Walk newest to oldest (entries are already sorted newest-first).
    for (i, entry) in entries.iter().enumerate() {
        let ts = entry.timestamp_ms;

        if ts >= hourly_cutoff {
            // Within hourly window — keep one per hour slot.
            let slot = hourly_slot(ts);
            if !hourly_slots.contains_key(&slot) {
                hourly_slots.insert(slot, true);
                keep_indices.push(i);
            }
        } else if ts >= daily_cutoff {
            // Within daily window — keep one per day slot.
            let slot = daily_slot(ts);
            if !daily_slots.contains_key(&slot) {
                daily_slots.insert(slot, true);
                keep_indices.push(i);
            }
        } else if policy.weekly_beyond {
            // Beyond daily window — keep one per week slot.
            let slot = weekly_slot(ts);
            if !weekly_slots.contains_key(&slot) {
                weekly_slots.insert(slot, true);
                keep_indices.push(i);
            }
        }
        // If !weekly_beyond, entries beyond the daily window are not kept.
    }

    // Apply max_total cap: keep the newest `max_total` entries from keep_indices.
    if let Some(max) = policy.max_total {
        keep_indices.truncate(max);
    }

    // Build the delete list: everything not in keep_indices.
    let keep_set: std::collections::HashSet<usize> = keep_indices.into_iter().collect();
    entries
        .iter()
        .enumerate()
        .filter(|(i, _)| !keep_set.contains(i))
        .map(|(_, e)| e.clone())
        .collect()
}

/// Execute pruning: delete files for entries that should be removed.
///
/// Returns the number of entries deleted.
pub fn prune_entries(
    entries: &[HistoryEntry],
    now_ms: u64,
    policy: &RetentionPolicy,
) -> Result<usize, HistoryError> {
    let to_delete = entries_to_prune(entries, now_ms, policy);
    let mut deleted = 0;

    for entry in &to_delete {
        if entry.path.exists() {
            fs::remove_file(&entry.path)?;
            deleted += 1;
        }
    }

    Ok(deleted)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::snapshot::compose_timestamp;
    use std::path::PathBuf;

    fn make_entry(ts_ms: u64) -> HistoryEntry {
        use super::super::snapshot::timestamp_to_filename;
        let filename = timestamp_to_filename(ts_ms);
        HistoryEntry {
            timestamp_ms: ts_ms,
            filename: filename.clone(),
            path: PathBuf::from(format!("/tmp/history/{}", filename)),
            size_bytes: 100,
        }
    }

    #[test]
    fn default_policy() {
        let p = RetentionPolicy::default();
        assert_eq!(p.hourly_window_hours, 24);
        assert_eq!(p.daily_window_days, 7);
        assert!(p.weekly_beyond);
        assert!(p.max_total.is_none());
    }

    #[test]
    fn hourly_slot_truncation() {
        // 14:35:22 should truncate to 14:00:00
        let ts = compose_timestamp(2026, 2, 22, 14, 35, 22) * 1000;
        let slot = hourly_slot(ts);
        let expected = compose_timestamp(2026, 2, 22, 14, 0, 0) * 1000;
        assert_eq!(slot, expected);
    }

    #[test]
    fn daily_slot_truncation() {
        let ts = compose_timestamp(2026, 2, 22, 14, 35, 22) * 1000;
        let slot = daily_slot(ts);
        let expected = compose_timestamp(2026, 2, 22, 0, 0, 0) * 1000;
        assert_eq!(slot, expected);
    }

    #[test]
    fn weekly_slot_truncation() {
        // 2026-02-22 is a Sunday. The Monday of that week is 2026-02-16.
        let ts = compose_timestamp(2026, 2, 22, 14, 0, 0) * 1000;
        let slot = weekly_slot(ts);
        let expected = compose_timestamp(2026, 2, 16, 0, 0, 0) * 1000;
        assert_eq!(slot, expected);
    }

    #[test]
    fn prune_keeps_one_per_hour_in_hourly_window() {
        let now = compose_timestamp(2026, 2, 22, 12, 0, 0) * 1000;

        // Two entries in the same hour (11:00-11:59).
        let e1 = make_entry(compose_timestamp(2026, 2, 22, 11, 30, 0) * 1000);
        let e2 = make_entry(compose_timestamp(2026, 2, 22, 11, 15, 0) * 1000);
        // One entry in a different hour.
        let e3 = make_entry(compose_timestamp(2026, 2, 22, 10, 45, 0) * 1000);

        let entries = vec![e1.clone(), e2.clone(), e3.clone()]; // newest first
        let policy = RetentionPolicy::default();
        let to_delete = entries_to_prune(&entries, now, &policy);

        // Should keep e1 (newest in 11:xx slot) and e3 (only in 10:xx slot).
        // Should delete e2.
        assert_eq!(to_delete.len(), 1);
        assert_eq!(to_delete[0].timestamp_ms, e2.timestamp_ms);
    }

    #[test]
    fn prune_keeps_one_per_day_in_daily_window() {
        let now = compose_timestamp(2026, 2, 22, 12, 0, 0) * 1000;

        // Two entries 2 days ago (within 7-day window, outside 24h window).
        let e1 = make_entry(compose_timestamp(2026, 2, 20, 15, 0, 0) * 1000);
        let e2 = make_entry(compose_timestamp(2026, 2, 20, 10, 0, 0) * 1000);

        let entries = vec![e1.clone(), e2.clone()];
        let policy = RetentionPolicy::default();
        let to_delete = entries_to_prune(&entries, now, &policy);

        // Same day → keep only the newest.
        assert_eq!(to_delete.len(), 1);
        assert_eq!(to_delete[0].timestamp_ms, e2.timestamp_ms);
    }

    #[test]
    fn prune_keeps_one_per_week_beyond_daily() {
        let now = compose_timestamp(2026, 2, 22, 12, 0, 0) * 1000;

        // Two entries 10 days ago (beyond 7-day window, same week).
        let e1 = make_entry(compose_timestamp(2026, 2, 12, 15, 0, 0) * 1000);
        let e2 = make_entry(compose_timestamp(2026, 2, 11, 10, 0, 0) * 1000);

        let entries = vec![e1.clone(), e2.clone()];
        let policy = RetentionPolicy::default();
        let to_delete = entries_to_prune(&entries, now, &policy);

        // Same week → keep only the newest.
        assert_eq!(to_delete.len(), 1);
        assert_eq!(to_delete[0].timestamp_ms, e2.timestamp_ms);
    }

    #[test]
    fn prune_weekly_disabled_deletes_old_entries() {
        let now = compose_timestamp(2026, 2, 22, 12, 0, 0) * 1000;

        let e1 = make_entry(compose_timestamp(2026, 2, 10, 12, 0, 0) * 1000);

        let entries = vec![e1.clone()];
        let policy = RetentionPolicy {
            weekly_beyond: false,
            ..Default::default()
        };
        let to_delete = entries_to_prune(&entries, now, &policy);

        assert_eq!(to_delete.len(), 1);
    }

    #[test]
    fn prune_max_total_cap() {
        let now = compose_timestamp(2026, 2, 22, 12, 0, 0) * 1000;

        // 5 entries, one per hour in the last 5 hours.
        let entries: Vec<HistoryEntry> = (0..5)
            .map(|i| make_entry(compose_timestamp(2026, 2, 22, 11 - i, 0, 0) * 1000))
            .collect();

        let policy = RetentionPolicy {
            max_total: Some(3),
            ..Default::default()
        };
        let to_delete = entries_to_prune(&entries, now, &policy);

        // Should keep 3 newest, delete 2 oldest.
        assert_eq!(to_delete.len(), 2);
    }

    #[test]
    fn prune_empty_entries() {
        let now = compose_timestamp(2026, 2, 22, 12, 0, 0) * 1000;
        let entries: Vec<HistoryEntry> = vec![];
        let policy = RetentionPolicy::default();
        let to_delete = entries_to_prune(&entries, now, &policy);
        assert!(to_delete.is_empty());
    }

    #[test]
    fn prune_spanning_three_days() {
        // Simulate 50 entries spanning 3 days with varying density.
        let base = compose_timestamp(2026, 2, 22, 12, 0, 0) * 1000;
        let now = base;

        let mut entries = Vec::new();
        // 24 entries in the last 24 hours (one per hour).
        for h in 0..24 {
            let ts = base - (h as u64) * MS_PER_HOUR - 1800_000; // offset by 30 min
            entries.push(make_entry(ts));
        }
        // 26 entries in the two days before that (13 per day).
        for d in 1..=2 {
            for h in 0..13 {
                let ts = base - 24 * MS_PER_HOUR - (d as u64) * MS_PER_DAY
                    + (h as u64) * MS_PER_HOUR;
                entries.push(make_entry(ts));
            }
        }

        // Sort newest first.
        entries.sort_by(|a, b| b.timestamp_ms.cmp(&a.timestamp_ms));

        let policy = RetentionPolicy::default();
        let to_delete = entries_to_prune(&entries, now, &policy);

        // All 50 entries minus the kept ones should be deleted.
        // Hourly window: up to 24 entries kept (one per distinct hour).
        // Daily window: up to 2 entries kept (one per distinct day).
        let kept = entries.len() - to_delete.len();
        assert!(kept <= 28, "kept {} entries, expected <= 28", kept);
        assert!(kept >= 2, "kept {} entries, expected >= 2", kept);
    }

    #[test]
    fn prune_entries_deletes_files() {
        let dir = std::env::temp_dir().join("cmx_hist_test_prune_files");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let now = compose_timestamp(2026, 2, 22, 12, 0, 0) * 1000;

        // Create two entries in the same hour.
        let ts1 = compose_timestamp(2026, 2, 22, 11, 50, 0) * 1000;
        let ts2 = compose_timestamp(2026, 2, 22, 11, 20, 0) * 1000;

        let e1 = super::super::snapshot::create_snapshot(&dir, "newer", ts1).unwrap();
        let e2 = super::super::snapshot::create_snapshot(&dir, "older", ts2).unwrap();

        let entries = vec![e1.clone(), e2.clone()];
        let policy = RetentionPolicy::default();
        let deleted = prune_entries(&entries, now, &policy).unwrap();

        assert_eq!(deleted, 1);
        assert!(e1.path.exists(), "newer entry should be kept");
        assert!(!e2.path.exists(), "older entry should be deleted");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn max_total_with_many_entries() {
        let now = compose_timestamp(2026, 2, 22, 12, 0, 0) * 1000;

        // 20 entries, one per hour.
        let entries: Vec<HistoryEntry> = (0..20)
            .map(|i| make_entry(now - (i as u64) * MS_PER_HOUR - 60_000))
            .collect();

        let policy = RetentionPolicy {
            max_total: Some(5),
            ..Default::default()
        };
        let to_delete = entries_to_prune(&entries, now, &policy);

        let kept = entries.len() - to_delete.len();
        assert_eq!(kept, 5);
    }
}
