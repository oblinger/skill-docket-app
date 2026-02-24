//! Timestamped snapshot creation and storage for configuration history.
//!
//! Each snapshot is a copy of `Current Configuration.md` saved with a
//! timestamp-based filename in the `history/` directory. Content hashing
//! prevents duplicate snapshots when the configuration hasn't changed.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// HistoryEntry
// ---------------------------------------------------------------------------

/// A single timestamped snapshot in the history folder.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HistoryEntry {
    pub timestamp_ms: u64,
    pub filename: String,
    pub path: PathBuf,
    pub size_bytes: u64,
}

// ---------------------------------------------------------------------------
// HistoryError
// ---------------------------------------------------------------------------

/// Errors that can occur during history operations.
#[derive(Debug)]
pub enum HistoryError {
    IoError(std::io::Error),
    EntryNotFound(String),
    InvalidTimestamp(String),
    ConfigNotFound(PathBuf),
    RestoreFailed(String),
}

impl std::fmt::Display for HistoryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HistoryError::IoError(e) => write!(f, "I/O error: {}", e),
            HistoryError::EntryNotFound(s) => write!(f, "entry not found: {}", s),
            HistoryError::InvalidTimestamp(s) => write!(f, "invalid timestamp: {}", s),
            HistoryError::ConfigNotFound(p) => {
                write!(f, "config not found: {}", p.display())
            }
            HistoryError::RestoreFailed(s) => write!(f, "restore failed: {}", s),
        }
    }
}

impl From<std::io::Error> for HistoryError {
    fn from(e: std::io::Error) -> Self {
        HistoryError::IoError(e)
    }
}

// ---------------------------------------------------------------------------
// Timestamp helpers
// ---------------------------------------------------------------------------

/// Convert a millisecond timestamp to the filename format `YYYY-MM-DDTHH-MM-SS.md`.
///
/// Uses manual arithmetic to decompose the Unix timestamp into date/time
/// components without external dependencies.
pub fn timestamp_to_filename(ms: u64) -> String {
    let secs = ms / 1000;
    let (year, month, day, hour, minute, second) = decompose_timestamp(secs);
    format!(
        "{:04}-{:02}-{:02}T{:02}-{:02}-{:02}.md",
        year, month, day, hour, minute, second
    )
}

/// Parse a history filename back to a millisecond timestamp.
///
/// Expected format: `YYYY-MM-DDTHH-MM-SS.md`.
pub fn filename_to_timestamp(filename: &str) -> Result<u64, HistoryError> {
    let stem = filename.strip_suffix(".md").ok_or_else(|| {
        HistoryError::InvalidTimestamp(format!("missing .md extension: {}", filename))
    })?;

    let parts: Vec<&str> = stem.split('T').collect();
    if parts.len() != 2 {
        return Err(HistoryError::InvalidTimestamp(format!(
            "expected YYYY-MM-DDTHH-MM-SS: {}",
            filename
        )));
    }

    let date_parts: Vec<u64> = parts[0]
        .split('-')
        .map(|s| {
            s.parse::<u64>().map_err(|_| {
                HistoryError::InvalidTimestamp(format!("bad date component: {}", s))
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    let time_parts: Vec<u64> = parts[1]
        .split('-')
        .map(|s| {
            s.parse::<u64>().map_err(|_| {
                HistoryError::InvalidTimestamp(format!("bad time component: {}", s))
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    if date_parts.len() != 3 || time_parts.len() != 3 {
        return Err(HistoryError::InvalidTimestamp(format!(
            "wrong number of components: {}",
            filename
        )));
    }

    let year = date_parts[0];
    let month = date_parts[1];
    let day = date_parts[2];
    let hour = time_parts[0];
    let minute = time_parts[1];
    let second = time_parts[2];

    let secs = compose_timestamp(year, month, day, hour, minute, second);
    Ok(secs * 1000)
}

// ---------------------------------------------------------------------------
// Snapshot I/O
// ---------------------------------------------------------------------------

/// Create a new history snapshot file.
///
/// Writes the given content to `history_dir/YYYY-MM-DDTHH-MM-SS.md`.
/// Returns the resulting `HistoryEntry`.
pub fn create_snapshot(
    history_dir: &Path,
    content: &str,
    now_ms: u64,
) -> Result<HistoryEntry, HistoryError> {
    fs::create_dir_all(history_dir)?;

    let filename = timestamp_to_filename(now_ms);
    let path = history_dir.join(&filename);
    fs::write(&path, content)?;

    Ok(HistoryEntry {
        timestamp_ms: now_ms,
        filename,
        path,
        size_bytes: content.len() as u64,
    })
}

/// Read the content of a history entry.
pub fn read_snapshot(entry: &HistoryEntry) -> Result<String, HistoryError> {
    if !entry.path.exists() {
        return Err(HistoryError::EntryNotFound(format!(
            "file does not exist: {}",
            entry.path.display()
        )));
    }
    Ok(fs::read_to_string(&entry.path)?)
}

/// List all history entries in a directory, sorted newest first.
pub fn list_entries(history_dir: &Path) -> Result<Vec<HistoryEntry>, HistoryError> {
    if !history_dir.exists() {
        return Ok(Vec::new());
    }

    let mut entries = Vec::new();

    for dir_entry in fs::read_dir(history_dir)? {
        let dir_entry = dir_entry?;
        let filename = match dir_entry.file_name().into_string() {
            Ok(s) => s,
            Err(_) => continue,
        };

        if !filename.ends_with(".md") {
            continue;
        }

        let timestamp_ms = match filename_to_timestamp(&filename) {
            Ok(ts) => ts,
            Err(_) => continue, // skip non-conforming files
        };

        let metadata = dir_entry.metadata()?;
        entries.push(HistoryEntry {
            timestamp_ms,
            filename,
            path: dir_entry.path(),
            size_bytes: metadata.len(),
        });
    }

    // Sort newest first.
    entries.sort_by(|a, b| b.timestamp_ms.cmp(&a.timestamp_ms));
    Ok(entries)
}

/// Read the most recent history entry, if any.
pub fn latest_entry(history_dir: &Path) -> Result<Option<HistoryEntry>, HistoryError> {
    let entries = list_entries(history_dir)?;
    Ok(entries.into_iter().next())
}

// ---------------------------------------------------------------------------
// Content hashing
// ---------------------------------------------------------------------------

/// FNV-1a 64-bit hash — a fast, non-cryptographic hash function.
///
/// Matches the implementation in `snapshot/state.rs`.
pub fn fnv1a_hash(data: &[u8]) -> u64 {
    const FNV_OFFSET: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;
    let mut hash = FNV_OFFSET;
    for &byte in data {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

/// Compute a hex-formatted content hash for deduplication.
pub fn content_hash(content: &str) -> String {
    format!("{:016x}", fnv1a_hash(content.as_bytes()))
}

// ---------------------------------------------------------------------------
// Date/time decomposition (no external deps)
// ---------------------------------------------------------------------------

/// Decompose a Unix timestamp (seconds) into (year, month, day, hour, minute, second).
fn decompose_timestamp(secs: u64) -> (u64, u64, u64, u64, u64, u64) {
    let second = secs % 60;
    let total_minutes = secs / 60;
    let minute = total_minutes % 60;
    let total_hours = total_minutes / 60;
    let hour = total_hours % 24;
    let mut days = total_hours / 24;

    // Compute year from days since epoch (1970-01-01).
    let mut year = 1970u64;
    loop {
        let days_in_year = if is_leap_year(year) { 366 } else { 365 };
        if days < days_in_year {
            break;
        }
        days -= days_in_year;
        year += 1;
    }

    // Compute month and day.
    let days_in_months = if is_leap_year(year) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };

    let mut month = 1u64;
    for &dim in &days_in_months {
        if days < dim {
            break;
        }
        days -= dim;
        month += 1;
    }

    let day = days + 1; // 1-indexed

    (year, month, day, hour, minute, second)
}

/// Compose date/time components into a Unix timestamp (seconds since epoch).
pub fn compose_timestamp(year: u64, month: u64, day: u64, hour: u64, minute: u64, second: u64) -> u64 {
    // Days from epoch to start of year.
    let mut days: u64 = 0;
    for y in 1970..year {
        days += if is_leap_year(y) { 366 } else { 365 };
    }

    // Days from start of year to start of month.
    let days_in_months = if is_leap_year(year) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };
    for m in 1..month {
        days += days_in_months[(m - 1) as usize];
    }

    // Day of month (1-indexed).
    days += day - 1;

    days * 86400 + hour * 3600 + minute * 60 + second
}

/// Check if a year is a leap year.
fn is_leap_year(year: u64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamp_to_filename_basic() {
        let ms = compose_timestamp(2026, 2, 22, 14, 30, 0) * 1000;
        let filename = timestamp_to_filename(ms);
        assert_eq!(filename, "2026-02-22T14-30-00.md");
    }

    #[test]
    fn filename_to_timestamp_basic() {
        let ms = filename_to_timestamp("2026-02-22T14-30-00.md").unwrap();
        let expected = compose_timestamp(2026, 2, 22, 14, 30, 0) * 1000;
        assert_eq!(ms, expected);
    }

    #[test]
    fn filename_round_trip() {
        let original_ms = compose_timestamp(2025, 12, 31, 23, 59, 59) * 1000;
        let filename = timestamp_to_filename(original_ms);
        let parsed_ms = filename_to_timestamp(&filename).unwrap();
        assert_eq!(original_ms, parsed_ms);
    }

    #[test]
    fn filename_round_trip_epoch() {
        let ms = compose_timestamp(1970, 1, 1, 0, 0, 0) * 1000;
        let filename = timestamp_to_filename(ms);
        assert_eq!(filename, "1970-01-01T00-00-00.md");
        let parsed = filename_to_timestamp(&filename).unwrap();
        assert_eq!(parsed, ms);
    }

    #[test]
    fn filename_to_timestamp_invalid_extension() {
        let result = filename_to_timestamp("2026-02-22T14-30-00.txt");
        assert!(result.is_err());
    }

    #[test]
    fn filename_to_timestamp_invalid_format() {
        let result = filename_to_timestamp("not-a-timestamp.md");
        assert!(result.is_err());
    }

    #[test]
    fn fnv1a_hash_deterministic() {
        let h1 = fnv1a_hash(b"hello world");
        let h2 = fnv1a_hash(b"hello world");
        assert_eq!(h1, h2);
    }

    #[test]
    fn fnv1a_hash_differs() {
        let h1 = fnv1a_hash(b"content A");
        let h2 = fnv1a_hash(b"content B");
        assert_ne!(h1, h2);
    }

    #[test]
    fn content_hash_hex_format() {
        let hash = content_hash("test content");
        assert_eq!(hash.len(), 16);
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn create_and_read_snapshot() {
        let dir = std::env::temp_dir().join("cmx_hist_test_create_read");
        let _ = fs::remove_dir_all(&dir);

        let content = "# Configuration\nagent: pilot\n";
        let now_ms = compose_timestamp(2026, 2, 22, 10, 0, 0) * 1000;
        let entry = create_snapshot(&dir, content, now_ms).unwrap();

        assert_eq!(entry.filename, "2026-02-22T10-00-00.md");
        assert_eq!(entry.size_bytes, content.len() as u64);

        let read_back = read_snapshot(&entry).unwrap();
        assert_eq!(read_back, content);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn list_entries_empty_dir() {
        let dir = std::env::temp_dir().join("cmx_hist_test_list_empty");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let entries = list_entries(&dir).unwrap();
        assert!(entries.is_empty());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn list_entries_nonexistent_dir() {
        let dir = std::env::temp_dir().join("cmx_hist_test_nonexistent_dir_xyz");
        let _ = fs::remove_dir_all(&dir);

        let entries = list_entries(&dir).unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn list_entries_sorted_newest_first() {
        let dir = std::env::temp_dir().join("cmx_hist_test_list_sorted");
        let _ = fs::remove_dir_all(&dir);

        let ts1 = compose_timestamp(2026, 1, 1, 0, 0, 0) * 1000;
        let ts2 = compose_timestamp(2026, 1, 1, 1, 0, 0) * 1000;
        let ts3 = compose_timestamp(2026, 1, 1, 2, 0, 0) * 1000;

        // Create in non-chronological order.
        create_snapshot(&dir, "c2", ts2).unwrap();
        create_snapshot(&dir, "c1", ts1).unwrap();
        create_snapshot(&dir, "c3", ts3).unwrap();

        let entries = list_entries(&dir).unwrap();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].timestamp_ms, ts3);
        assert_eq!(entries[1].timestamp_ms, ts2);
        assert_eq!(entries[2].timestamp_ms, ts1);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn list_entries_ignores_non_md_files() {
        let dir = std::env::temp_dir().join("cmx_hist_test_ignore_non_md");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        fs::write(dir.join("notes.txt"), "not a snapshot").unwrap();
        fs::write(dir.join("README.md"), "not a snapshot either").unwrap();

        let ts = compose_timestamp(2026, 1, 1, 0, 0, 0) * 1000;
        create_snapshot(&dir, "valid", ts).unwrap();

        let entries = list_entries(&dir).unwrap();
        assert_eq!(entries.len(), 1);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn latest_entry_returns_newest() {
        let dir = std::env::temp_dir().join("cmx_hist_test_latest");
        let _ = fs::remove_dir_all(&dir);

        let ts1 = compose_timestamp(2026, 1, 1, 0, 0, 0) * 1000;
        let ts2 = compose_timestamp(2026, 1, 2, 0, 0, 0) * 1000;

        create_snapshot(&dir, "old", ts1).unwrap();
        create_snapshot(&dir, "new", ts2).unwrap();

        let latest = latest_entry(&dir).unwrap().unwrap();
        assert_eq!(latest.timestamp_ms, ts2);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn latest_entry_none_when_empty() {
        let dir = std::env::temp_dir().join("cmx_hist_test_latest_empty");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let latest = latest_entry(&dir).unwrap();
        assert!(latest.is_none());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn read_snapshot_missing_file() {
        let entry = HistoryEntry {
            timestamp_ms: 0,
            filename: "nonexistent.md".into(),
            path: PathBuf::from("/tmp/cmx_hist_does_not_exist/nonexistent.md"),
            size_bytes: 0,
        };
        let result = read_snapshot(&entry);
        assert!(result.is_err());
    }

    #[test]
    fn decompose_and_compose_round_trip() {
        let cases = [
            (1970, 1, 1, 0, 0, 0),
            (2000, 2, 29, 12, 0, 0), // leap year
            (2026, 2, 22, 14, 30, 45),
            (2024, 12, 31, 23, 59, 59),
        ];
        for (y, mo, d, h, mi, s) in cases {
            let secs = compose_timestamp(y, mo, d, h, mi, s);
            let (y2, mo2, d2, h2, mi2, s2) = decompose_timestamp(secs);
            assert_eq!((y, mo, d, h, mi, s), (y2, mo2, d2, h2, mi2, s2),
                "round-trip failed for {:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
                y, mo, d, h, mi, s);
        }
    }

    #[test]
    fn create_snapshot_creates_directory() {
        let dir = std::env::temp_dir().join("cmx_hist_test_auto_mkdir");
        let _ = fs::remove_dir_all(&dir);

        let nested = dir.join("sub").join("dir");
        let ts = compose_timestamp(2026, 3, 1, 0, 0, 0) * 1000;
        let entry = create_snapshot(&nested, "content", ts).unwrap();
        assert!(entry.path.exists());

        let _ = fs::remove_dir_all(&dir);
    }
}
