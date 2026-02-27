//! Learnings — parse, search, and append entries in LEARNINGS.md files.
//!
//! Each managed project can have a `LEARNINGS.md` file in its root directory.
//! Agents read it on startup and append to it when they make discoveries.
//!
//! # File Format
//!
//! ```markdown
//! # Learnings
//!
//! ## 2026-02-26 — Tests require --no-parallel
//!
//! Body text here.
//!
//! **Source**: worker-3, task M4.2
//! **Tags**: testing, ci
//! ```

use std::path::{Path, PathBuf};

use crate::data::FolderRegistry;


/// A single learning entry parsed from LEARNINGS.md.
#[derive(Debug, Clone, PartialEq)]
pub struct LearningEntry {
    /// ISO date of discovery (e.g. "2026-02-26").
    pub date: String,
    /// Short description title.
    pub title: String,
    /// Explanation body text.
    pub body: String,
    /// Source attribution (e.g. "worker-3, task M4.2").
    pub source: String,
    /// Comma-separated tags.
    pub tags: Vec<String>,
}


/// Parse a LEARNINGS.md file's content into a list of entries.
pub fn parse_learnings(content: &str) -> Vec<LearningEntry> {
    let mut entries = Vec::new();
    let mut current_date = String::new();
    let mut current_title = String::new();
    let mut current_body_lines: Vec<String> = Vec::new();
    let mut current_source = String::new();
    let mut current_tags: Vec<String> = Vec::new();
    let mut in_entry = false;

    for line in content.lines() {
        if line.starts_with("## ") {
            // Flush previous entry if any
            if in_entry {
                let body = current_body_lines.join("\n").trim().to_string();
                entries.push(LearningEntry {
                    date: current_date.clone(),
                    title: current_title.clone(),
                    body,
                    source: current_source.clone(),
                    tags: current_tags.clone(),
                });
            }

            // Parse heading: ## 2026-02-26 — Title text
            let heading = &line[3..];
            if let Some(dash_pos) = heading.find(" — ") {
                current_date = heading[..dash_pos].trim().to_string();
                current_title = heading[dash_pos + " — ".len()..].trim().to_string();
            } else if let Some(dash_pos) = heading.find(" - ") {
                current_date = heading[..dash_pos].trim().to_string();
                current_title = heading[dash_pos + 3..].trim().to_string();
            } else {
                current_date = String::new();
                current_title = heading.trim().to_string();
            }

            current_body_lines = Vec::new();
            current_source = String::new();
            current_tags = Vec::new();
            in_entry = true;
        } else if in_entry {
            let trimmed = line.trim();
            if trimmed.starts_with("**Source**:") || trimmed.starts_with("**Source**:") {
                current_source = trimmed
                    .trim_start_matches("**Source**:")
                    .trim_start_matches("**Source**:")
                    .trim()
                    .to_string();
            } else if trimmed.starts_with("**Tags**:") || trimmed.starts_with("**Tags**:") {
                let tags_str = trimmed
                    .trim_start_matches("**Tags**:")
                    .trim_start_matches("**Tags**:")
                    .trim();
                current_tags = tags_str
                    .split(',')
                    .map(|t| t.trim().to_string())
                    .filter(|t| !t.is_empty())
                    .collect();
            } else {
                current_body_lines.push(line.to_string());
            }
        }
    }

    // Flush last entry
    if in_entry {
        let body = current_body_lines.join("\n").trim().to_string();
        entries.push(LearningEntry {
            date: current_date,
            title: current_title,
            body,
            source: current_source,
            tags: current_tags,
        });
    }

    entries
}


/// Format a new learning entry as markdown text.
fn format_entry(date: &str, title: &str, body: &str, source: &str, tags: &[String]) -> String {
    let mut entry = format!("## {} — {}\n\n{}\n", date, title, body);
    if !source.is_empty() {
        entry.push_str(&format!("\n**Source**: {}", source));
    }
    if !tags.is_empty() {
        entry.push_str(&format!("\n**Tags**: {}", tags.join(", ")));
    }
    entry.push('\n');
    entry
}


/// Prepend a new entry to LEARNINGS.md content, returning the updated content.
pub fn prepend_entry(
    existing_content: &str,
    date: &str,
    title: &str,
    body: &str,
    source: &str,
    tags: &[String],
) -> String {
    let new_entry = format_entry(date, title, body, source, tags);

    // If file is empty or has only the H1 header, start fresh
    let trimmed = existing_content.trim();
    if trimmed.is_empty() {
        return format!("# Learnings\n\n{}", new_entry);
    }

    // Find the position after the H1 header line (and any blank lines after it)
    let mut insert_pos = 0;
    let mut found_h1 = false;
    for line in existing_content.lines() {
        insert_pos += line.len() + 1; // +1 for newline
        if line.starts_with("# ") && !line.starts_with("## ") {
            found_h1 = true;
            // Skip blank lines after the header
            continue;
        }
        if found_h1 {
            if line.trim().is_empty() {
                continue;
            }
            // We've hit the first non-blank line after the header — insert before it
            insert_pos -= line.len() + 1;
            break;
        }
    }

    if !found_h1 {
        // No H1 header found; prepend everything
        return format!("# Learnings\n\n{}\n{}", new_entry, existing_content);
    }

    let before = &existing_content[..insert_pos];
    let after = &existing_content[insert_pos..];
    format!("{}{}\n{}", before, new_entry, after)
}


/// Filter entries by tag (case-insensitive substring match).
pub fn filter_by_tag(entries: &[LearningEntry], tag: &str) -> Vec<LearningEntry> {
    let tag_lower = tag.to_lowercase();
    entries
        .iter()
        .filter(|e| e.tags.iter().any(|t| t.to_lowercase() == tag_lower))
        .cloned()
        .collect()
}


/// Full-text search across entries (case-insensitive).
pub fn search_entries(entries: &[LearningEntry], query: &str) -> Vec<LearningEntry> {
    let query_lower = query.to_lowercase();
    entries
        .iter()
        .filter(|e| {
            e.title.to_lowercase().contains(&query_lower)
                || e.body.to_lowercase().contains(&query_lower)
                || e.source.to_lowercase().contains(&query_lower)
                || e.tags.iter().any(|t| t.to_lowercase().contains(&query_lower))
        })
        .cloned()
        .collect()
}


/// Resolve the LEARNINGS.md path for a project from the folder registry.
pub fn learnings_path_for_project(folders: &FolderRegistry, project: &str) -> Option<PathBuf> {
    folders.get(project).map(|f| PathBuf::from(&f.path).join("LEARNINGS.md"))
}


/// Resolve LEARNINGS.md paths for all registered projects.
pub fn all_learnings_paths(folders: &FolderRegistry) -> Vec<(String, PathBuf)> {
    folders
        .list()
        .iter()
        .map(|f| (f.name.clone(), PathBuf::from(&f.path).join("LEARNINGS.md")))
        .collect()
}


/// Format a learning entry for display output.
pub fn format_entry_display(entry: &LearningEntry, project: Option<&str>) -> String {
    let mut lines = Vec::new();
    let prefix = if let Some(p) = project {
        format!("[{}] ", p)
    } else {
        String::new()
    };
    lines.push(format!("{}{} — {}", prefix, entry.date, entry.title));
    if !entry.body.is_empty() {
        lines.push(format!("  {}", entry.body.replace('\n', "\n  ")));
    }
    if !entry.source.is_empty() {
        lines.push(format!("  Source: {}", entry.source));
    }
    if !entry.tags.is_empty() {
        lines.push(format!("  Tags: {}", entry.tags.join(", ")));
    }
    lines.join("\n")
}


/// Read and parse LEARNINGS.md from a file path, returning entries.
/// Returns an empty vec if the file doesn't exist.
pub fn load_entries(path: &Path) -> Vec<LearningEntry> {
    match std::fs::read_to_string(path) {
        Ok(content) => parse_learnings(&content),
        Err(_) => Vec::new(),
    }
}


/// Get today's date as an ISO date string.
pub fn today_iso() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs();
    // Simple date calculation
    let days = secs / 86400;
    let (year, month, day) = days_to_ymd(days);
    format!("{:04}-{:02}-{:02}", year, month, day)
}


/// Convert days since Unix epoch to (year, month, day).
fn days_to_ymd(days: u64) -> (u64, u64, u64) {
    // Algorithm from http://howardhinnant.github.io/date_algorithms.html
    let z = days + 719468;
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}


// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
# Learnings

## 2026-02-26 — Tests require --no-parallel

The integration tests use a shared SQLite database. Running them in parallel
causes lock contention. Always use `cargo test -- --test-threads=1` for the
integration suite.

**Source**: worker-3, task M4.2
**Tags**: testing, ci

## 2026-02-25 — API rate limit is 100/min not 1000/min

The staging environment has a lower rate limit than documented. Batch operations
need 15ms delays between calls.

**Source**: worker-1, task M3.1
**Tags**: api, staging
";

    #[test]
    fn parse_two_entries() {
        let entries = parse_learnings(SAMPLE);
        assert_eq!(entries.len(), 2);

        assert_eq!(entries[0].date, "2026-02-26");
        assert_eq!(entries[0].title, "Tests require --no-parallel");
        assert!(entries[0].body.contains("shared SQLite database"));
        assert_eq!(entries[0].source, "worker-3, task M4.2");
        assert_eq!(entries[0].tags, vec!["testing", "ci"]);

        assert_eq!(entries[1].date, "2026-02-25");
        assert_eq!(entries[1].title, "API rate limit is 100/min not 1000/min");
        assert!(entries[1].body.contains("15ms delays"));
        assert_eq!(entries[1].source, "worker-1, task M3.1");
        assert_eq!(entries[1].tags, vec!["api", "staging"]);
    }

    #[test]
    fn parse_empty() {
        let entries = parse_learnings("");
        assert!(entries.is_empty());
    }

    #[test]
    fn parse_header_only() {
        let entries = parse_learnings("# Learnings\n");
        assert!(entries.is_empty());
    }

    #[test]
    fn prepend_to_existing() {
        let updated = prepend_entry(
            SAMPLE,
            "2026-02-27",
            "New discovery",
            "Some body text.",
            "worker-5, task M5.1",
            &["build".into()],
        );
        let entries = parse_learnings(&updated);
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].date, "2026-02-27");
        assert_eq!(entries[0].title, "New discovery");
        // Original entries still present
        assert_eq!(entries[1].date, "2026-02-26");
        assert_eq!(entries[2].date, "2026-02-25");
    }

    #[test]
    fn prepend_to_empty() {
        let updated = prepend_entry(
            "",
            "2026-02-27",
            "First learning",
            "Body.",
            "w1",
            &["tag1".into()],
        );
        assert!(updated.starts_with("# Learnings"));
        let entries = parse_learnings(&updated);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].title, "First learning");
    }

    #[test]
    fn filter_by_tag_matches() {
        let entries = parse_learnings(SAMPLE);
        let filtered = filter_by_tag(&entries, "testing");
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].title, "Tests require --no-parallel");
    }

    #[test]
    fn filter_by_tag_case_insensitive() {
        let entries = parse_learnings(SAMPLE);
        let filtered = filter_by_tag(&entries, "CI");
        assert_eq!(filtered.len(), 1);
    }

    #[test]
    fn filter_by_tag_no_match() {
        let entries = parse_learnings(SAMPLE);
        let filtered = filter_by_tag(&entries, "nonexistent");
        assert!(filtered.is_empty());
    }

    #[test]
    fn search_finds_in_title() {
        let entries = parse_learnings(SAMPLE);
        let found = search_entries(&entries, "rate limit");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].date, "2026-02-25");
    }

    #[test]
    fn search_finds_in_body() {
        let entries = parse_learnings(SAMPLE);
        let found = search_entries(&entries, "SQLite");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].date, "2026-02-26");
    }

    #[test]
    fn search_case_insensitive() {
        let entries = parse_learnings(SAMPLE);
        let found = search_entries(&entries, "sqlite");
        assert_eq!(found.len(), 1);
    }

    #[test]
    fn search_no_match() {
        let entries = parse_learnings(SAMPLE);
        let found = search_entries(&entries, "zzzz_nonexistent");
        assert!(found.is_empty());
    }

    #[test]
    fn search_finds_in_tags() {
        let entries = parse_learnings(SAMPLE);
        let found = search_entries(&entries, "staging");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].date, "2026-02-25");
    }

    #[test]
    fn format_entry_display_with_project() {
        let entry = LearningEntry {
            date: "2026-02-26".into(),
            title: "Test".into(),
            body: "Body.".into(),
            source: "w1".into(),
            tags: vec!["t1".into()],
        };
        let output = format_entry_display(&entry, Some("myproj"));
        assert!(output.contains("[myproj]"));
        assert!(output.contains("2026-02-26"));
        assert!(output.contains("Test"));
    }

    #[test]
    fn today_iso_format() {
        let date = today_iso();
        assert_eq!(date.len(), 10);
        assert!(date.contains('-'));
        // Should be a reasonable year
        let year: u32 = date[..4].parse().unwrap();
        assert!(year >= 2024 && year <= 2100);
    }
}
