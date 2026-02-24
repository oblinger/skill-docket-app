use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Configuration for conversation logging.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogConfig {
    /// Whether conversation logging is enabled.
    pub enabled: bool,
    /// Whether to capture agent responses (true = full transcript, false = user messages only).
    /// Note: in practice this is hard to distinguish from tmux capture, so start with capturing everything.
    pub capture_responses: bool,
    /// Retention period in days. Log files older than this are auto-deleted. Default: 7.
    pub retention_days: u32,
    /// Capture interval in seconds. How often to poll tmux panes. Default: 5.
    pub capture_interval_secs: u32,
}

impl Default for LogConfig {
    fn default() -> Self {
        LogConfig {
            enabled: true,
            capture_responses: true,
            retention_days: 7,
            capture_interval_secs: 5,
        }
    }
}

/// Error type for conversation logging operations.
#[derive(Debug)]
pub enum LogError {
    Io(std::io::Error),
    AgentNotRegistered(String),
    InvalidDate(String),
}

impl std::fmt::Display for LogError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LogError::Io(e) => write!(f, "I/O error: {}", e),
            LogError::AgentNotRegistered(name) => {
                write!(f, "agent '{}' not registered for logging", name)
            }
            LogError::InvalidDate(date) => write!(f, "invalid date: {}", date),
        }
    }
}

impl std::error::Error for LogError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            LogError::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for LogError {
    fn from(e: std::io::Error) -> Self {
        LogError::Io(e)
    }
}

/// Tracks the capture state for a single agent's tmux pane.
#[derive(Debug, Clone)]
pub struct AgentLogTracker {
    /// Agent name (used in filename).
    pub agent_name: String,
    /// Last captured byte offset in the tmux pane buffer.
    pub last_offset: usize,
    /// Path to the current day's log file.
    pub current_log_path: PathBuf,
    /// The date of the current log file (YYYY-MM-DD).
    pub current_date: String,
}

/// Manages conversation logging for all agents.
pub struct ConversationLogger {
    /// Base directory for log files (.pilot-log/).
    log_dir: PathBuf,
    /// Per-agent tracking state.
    trackers: HashMap<String, AgentLogTracker>,
    /// Configuration.
    config: LogConfig,
}

impl ConversationLogger {
    /// Create a new logger for a project directory.
    ///
    /// Creates the `.pilot-log/` directory inside `project_dir` if it does not exist.
    pub fn new(project_dir: &Path, config: LogConfig) -> Result<Self, LogError> {
        let log_dir = project_dir.join(".pilot-log");
        fs::create_dir_all(&log_dir)?;
        Ok(ConversationLogger {
            log_dir,
            trackers: HashMap::new(),
            config,
        })
    }

    /// Register an agent for conversation logging.
    pub fn register_agent(&mut self, agent_name: &str) -> Result<(), LogError> {
        let tracker = AgentLogTracker {
            agent_name: agent_name.to_string(),
            last_offset: 0,
            current_log_path: self.log_file_path(agent_name, "0000-00-00"),
            current_date: String::new(),
        };
        self.trackers.insert(agent_name.to_string(), tracker);
        Ok(())
    }

    /// Unregister an agent (stops logging for it).
    pub fn unregister_agent(&mut self, agent_name: &str) {
        self.trackers.remove(agent_name);
    }

    /// Process new captured text from an agent's tmux pane.
    ///
    /// This is called with the FULL pane content; the tracker uses byte offsets
    /// to determine what's new and appends only the new content.
    /// Returns the number of new bytes written.
    pub fn process_capture(
        &mut self,
        agent_name: &str,
        pane_content: &str,
        date: &str,
    ) -> Result<usize, LogError> {
        if !self.config.enabled {
            return Ok(0);
        }

        if !self.trackers.contains_key(agent_name) {
            return Err(LogError::AgentNotRegistered(agent_name.to_string()));
        }

        if !is_valid_date(date) {
            return Err(LogError::InvalidDate(date.to_string()));
        }

        // Compute the new path before borrowing tracker mutably.
        let new_path = self.log_file_path(agent_name, date);

        let tracker = self.trackers.get_mut(agent_name).unwrap();

        // Handle day rollover: switch log file but keep byte offset.
        if tracker.current_date != date {
            tracker.current_date = date.to_string();
            tracker.current_log_path = new_path;
        }

        // Determine new content based on byte offset.
        let content_len = pane_content.len();
        if content_len <= tracker.last_offset {
            return Ok(0);
        }

        let new_content = &pane_content[tracker.last_offset..];
        if new_content.is_empty() {
            return Ok(0);
        }

        // Append new content to the log file.
        use std::io::Write;
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&tracker.current_log_path)?;
        file.write_all(new_content.as_bytes())?;

        let bytes_written = new_content.len();
        tracker.last_offset = content_len;

        Ok(bytes_written)
    }

    /// Get the current log file path for an agent.
    pub fn log_path(&self, agent_name: &str) -> Option<&Path> {
        self.trackers
            .get(agent_name)
            .map(|t| t.current_log_path.as_path())
    }

    /// Read the full log for an agent on a given date (YYYY-MM-DD).
    pub fn read_log(&self, agent_name: &str, date: &str) -> Result<String, LogError> {
        if !is_valid_date(date) {
            return Err(LogError::InvalidDate(date.to_string()));
        }
        let path = self.log_file_path(agent_name, date);
        let content = fs::read_to_string(&path)?;
        Ok(content)
    }

    /// List all available log dates for an agent.
    pub fn list_dates(&self, agent_name: &str) -> Result<Vec<String>, LogError> {
        let suffix = format!("-{}.md", agent_name);
        let mut dates = Vec::new();

        let entries = match fs::read_dir(&self.log_dir) {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(dates),
            Err(e) => return Err(LogError::Io(e)),
        };

        for entry in entries {
            let entry = entry?;
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            // Files are named YYYY-MM-DD-{agent_name}.md
            if name_str.ends_with(&suffix) && name_str.len() >= 10 {
                let date_part = &name_str[..10];
                if is_valid_date(date_part) {
                    dates.push(date_part.to_string());
                }
            }
        }

        dates.sort();
        Ok(dates)
    }

    /// List all agents that have logs.
    pub fn list_agents(&self) -> Result<Vec<String>, LogError> {
        let mut agents: std::collections::HashSet<String> = std::collections::HashSet::new();

        let entries = match fs::read_dir(&self.log_dir) {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Vec::new());
            }
            Err(e) => return Err(LogError::Io(e)),
        };

        for entry in entries {
            let entry = entry?;
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            // Files are named YYYY-MM-DD-{agent_name}.md
            if name_str.ends_with(".md") && name_str.len() > 14 {
                let date_part = &name_str[..10];
                if is_valid_date(date_part) {
                    // After "YYYY-MM-DD-" (11 chars) and before ".md" (3 chars).
                    let agent_part = &name_str[11..name_str.len() - 3];
                    if !agent_part.is_empty() {
                        agents.insert(agent_part.to_string());
                    }
                }
            }
        }

        let mut result: Vec<String> = agents.into_iter().collect();
        result.sort();
        Ok(result)
    }

    /// Run retention cleanup — delete log files older than retention_days.
    /// Returns the number of files deleted.
    pub fn cleanup(&self, today: &str) -> Result<usize, LogError> {
        if !is_valid_date(today) {
            return Err(LogError::InvalidDate(today.to_string()));
        }

        let today_days =
            date_to_days(today).ok_or_else(|| LogError::InvalidDate(today.to_string()))?;

        let mut deleted = 0;

        let entries = match fs::read_dir(&self.log_dir) {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
            Err(e) => return Err(LogError::Io(e)),
        };

        for entry in entries {
            let entry = entry?;
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if name_str.ends_with(".md") && name_str.len() > 14 {
                let date_part = &name_str[..10];
                if is_valid_date(date_part) {
                    if let Some(file_days) = date_to_days(date_part) {
                        let age = today_days.saturating_sub(file_days);
                        if age > self.config.retention_days as i64 {
                            fs::remove_file(entry.path())?;
                            deleted += 1;
                        }
                    }
                }
            }
        }

        Ok(deleted)
    }

    /// Get the byte offset for an agent (for copilot sync).
    pub fn agent_offset(&self, agent_name: &str) -> Option<usize> {
        self.trackers.get(agent_name).map(|t| t.last_offset)
    }

    /// Read new content since a given byte offset.
    /// Used by copilot sync to get only unseen content.
    ///
    /// Returns a tuple of (new_content, current_offset).
    pub fn read_since(
        &self,
        agent_name: &str,
        offset: usize,
    ) -> Result<(String, usize), LogError> {
        let tracker = self
            .trackers
            .get(agent_name)
            .ok_or_else(|| LogError::AgentNotRegistered(agent_name.to_string()))?;

        let path = &tracker.current_log_path;
        if !path.exists() {
            return Ok((String::new(), offset));
        }

        let content = fs::read_to_string(path)?;
        let content_len = content.len();

        if offset >= content_len {
            return Ok((String::new(), content_len));
        }

        let new_content = content[offset..].to_string();
        Ok((new_content, content_len))
    }

    /// Build the log file path for an agent on a given date.
    fn log_file_path(&self, agent_name: &str, date: &str) -> PathBuf {
        self.log_dir.join(format!("{}-{}.md", date, agent_name))
    }
}

/// Validate that a date string has YYYY-MM-DD format with plausible values.
fn is_valid_date(date: &str) -> bool {
    if date.len() != 10 {
        return false;
    }
    let parts: Vec<&str> = date.split('-').collect();
    if parts.len() != 3 {
        return false;
    }
    let year: u32 = match parts[0].parse() {
        Ok(y) => y,
        Err(_) => return false,
    };
    let month: u32 = match parts[1].parse() {
        Ok(m) => m,
        Err(_) => return false,
    };
    let day: u32 = match parts[2].parse() {
        Ok(d) => d,
        Err(_) => return false,
    };
    year >= 2000 && year <= 9999 && month >= 1 && month <= 12 && day >= 1 && day <= 31
}

/// Convert a YYYY-MM-DD date to an approximate day count for age comparison.
/// This is a simple approximation — exact calendar math is not needed for retention.
fn date_to_days(date: &str) -> Option<i64> {
    let parts: Vec<&str> = date.split('-').collect();
    if parts.len() != 3 {
        return None;
    }
    let year: i64 = parts[0].parse().ok()?;
    let month: i64 = parts[1].parse().ok()?;
    let day: i64 = parts[2].parse().ok()?;
    // Approximate: 365 days/year, 30 days/month. Good enough for retention.
    Some(year * 365 + month * 30 + day)
}


#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_dir() -> PathBuf {
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "cmx-test-convlog-{}-{:?}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            std::thread::current().id(),
            id,
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    // 1. Create logger: New logger creates `.pilot-log/` directory
    #[test]
    fn create_logger_creates_directory() {
        let dir = temp_dir();
        let _logger = ConversationLogger::new(&dir, LogConfig::default()).unwrap();
        assert!(dir.join(".pilot-log").is_dir());
        fs::remove_dir_all(&dir).ok();
    }

    // 2. Register agent: Register "pilot", verify tracker created
    #[test]
    fn register_agent_creates_tracker() {
        let dir = temp_dir();
        let mut logger = ConversationLogger::new(&dir, LogConfig::default()).unwrap();
        logger.register_agent("pilot").unwrap();
        assert!(logger.agent_offset("pilot").is_some());
        assert_eq!(logger.agent_offset("pilot"), Some(0));
        fs::remove_dir_all(&dir).ok();
    }

    // 3. First capture: Process first capture, verify content written to dated log file
    #[test]
    fn first_capture_writes_to_log_file() {
        let dir = temp_dir();
        let mut logger = ConversationLogger::new(&dir, LogConfig::default()).unwrap();
        logger.register_agent("pilot").unwrap();

        let content = "Hello, this is a test capture.\n";
        let written = logger.process_capture("pilot", content, "2026-02-17").unwrap();
        assert_eq!(written, content.len());

        let log_file = dir.join(".pilot-log/2026-02-17-pilot.md");
        assert!(log_file.exists());
        let file_content = fs::read_to_string(&log_file).unwrap();
        assert_eq!(file_content, content);
        fs::remove_dir_all(&dir).ok();
    }

    // 4. Incremental capture: Two captures with growing content, verify only new content appended
    #[test]
    fn incremental_capture_appends_only_new() {
        let dir = temp_dir();
        let mut logger = ConversationLogger::new(&dir, LogConfig::default()).unwrap();
        logger.register_agent("pilot").unwrap();

        let content1 = "Line 1\n";
        let written1 = logger.process_capture("pilot", content1, "2026-02-17").unwrap();
        assert_eq!(written1, content1.len());

        // Second capture includes the first content plus new content.
        let content2 = "Line 1\nLine 2\n";
        let written2 = logger.process_capture("pilot", content2, "2026-02-17").unwrap();
        assert_eq!(written2, "Line 2\n".len());

        let log_file = dir.join(".pilot-log/2026-02-17-pilot.md");
        let file_content = fs::read_to_string(&log_file).unwrap();
        assert_eq!(file_content, "Line 1\nLine 2\n");
        fs::remove_dir_all(&dir).ok();
    }

    // 5. Multiple agents: Register pilot + worker1, capture for both, verify separate log files
    #[test]
    fn multiple_agents_separate_files() {
        let dir = temp_dir();
        let mut logger = ConversationLogger::new(&dir, LogConfig::default()).unwrap();
        logger.register_agent("pilot").unwrap();
        logger.register_agent("worker1").unwrap();

        logger
            .process_capture("pilot", "Pilot says hello\n", "2026-02-17")
            .unwrap();
        logger
            .process_capture("worker1", "Worker says hello\n", "2026-02-17")
            .unwrap();

        let pilot_content =
            fs::read_to_string(dir.join(".pilot-log/2026-02-17-pilot.md")).unwrap();
        let worker_content =
            fs::read_to_string(dir.join(".pilot-log/2026-02-17-worker1.md")).unwrap();

        assert_eq!(pilot_content, "Pilot says hello\n");
        assert_eq!(worker_content, "Worker says hello\n");
        fs::remove_dir_all(&dir).ok();
    }

    // 6. Day rollover: Capture on date "2026-02-17", then "2026-02-18", verify two files created
    #[test]
    fn day_rollover_creates_new_file() {
        let dir = temp_dir();
        let mut logger = ConversationLogger::new(&dir, LogConfig::default()).unwrap();
        logger.register_agent("pilot").unwrap();

        let content_day1 = "Day 1 content\n";
        logger
            .process_capture("pilot", content_day1, "2026-02-17")
            .unwrap();

        // Day 2: pane content has grown (continuous buffer).
        let content_day2 = "Day 1 content\nDay 2 content\n";
        logger
            .process_capture("pilot", content_day2, "2026-02-18")
            .unwrap();

        let file1 = dir.join(".pilot-log/2026-02-17-pilot.md");
        let file2 = dir.join(".pilot-log/2026-02-18-pilot.md");
        assert!(file1.exists());
        assert!(file2.exists());

        let content1 = fs::read_to_string(&file1).unwrap();
        let content2 = fs::read_to_string(&file2).unwrap();
        assert_eq!(content1, "Day 1 content\n");
        assert_eq!(content2, "Day 2 content\n");
        fs::remove_dir_all(&dir).ok();
    }

    // 7. Read log: Write content, then read it back by date
    #[test]
    fn read_log_returns_content() {
        let dir = temp_dir();
        let mut logger = ConversationLogger::new(&dir, LogConfig::default()).unwrap();
        logger.register_agent("pilot").unwrap();

        let content = "Some log content\n";
        logger
            .process_capture("pilot", content, "2026-02-17")
            .unwrap();

        let read_back = logger.read_log("pilot", "2026-02-17").unwrap();
        assert_eq!(read_back, content);
        fs::remove_dir_all(&dir).ok();
    }

    // 8. List dates: Capture on three dates, verify list_dates returns all three sorted
    #[test]
    fn list_dates_returns_sorted() {
        let dir = temp_dir();
        let _logger = ConversationLogger::new(&dir, LogConfig::default()).unwrap();

        // Create log files for three dates directly.
        let log_dir = dir.join(".pilot-log");
        fs::write(log_dir.join("2026-02-19-pilot.md"), "c").unwrap();
        fs::write(log_dir.join("2026-02-17-pilot.md"), "a").unwrap();
        fs::write(log_dir.join("2026-02-18-pilot.md"), "b").unwrap();

        let dates = _logger.list_dates("pilot").unwrap();
        assert_eq!(dates, vec!["2026-02-17", "2026-02-18", "2026-02-19"]);
        fs::remove_dir_all(&dir).ok();
    }

    // 9. List agents: Register three agents, capture for each, verify list_agents finds all
    #[test]
    fn list_agents_finds_all() {
        let dir = temp_dir();
        let mut logger = ConversationLogger::new(&dir, LogConfig::default()).unwrap();
        logger.register_agent("pilot").unwrap();
        logger.register_agent("worker1").unwrap();
        logger.register_agent("worker2").unwrap();

        logger
            .process_capture("pilot", "p", "2026-02-17")
            .unwrap();
        logger
            .process_capture("worker1", "w1", "2026-02-17")
            .unwrap();
        logger
            .process_capture("worker2", "w2", "2026-02-17")
            .unwrap();

        let agents = logger.list_agents().unwrap();
        assert_eq!(agents, vec!["pilot", "worker1", "worker2"]);
        fs::remove_dir_all(&dir).ok();
    }

    // 10. Cleanup retention: Create logs outside retention window, verify cleanup deletes them
    #[test]
    fn cleanup_deletes_old_files() {
        let dir = temp_dir();
        let config = LogConfig {
            retention_days: 3,
            ..LogConfig::default()
        };
        let logger = ConversationLogger::new(&dir, config).unwrap();

        let log_dir = dir.join(".pilot-log");
        // Today is 2026-02-20, retention is 3 days.
        // 2026-02-15 (5 days old) and 2026-02-16 (4 days old) should be deleted.
        // 2026-02-17 (3 days old) should NOT be deleted (age == retention, not >).
        fs::write(log_dir.join("2026-02-15-pilot.md"), "old1").unwrap();
        fs::write(log_dir.join("2026-02-16-pilot.md"), "old2").unwrap();
        fs::write(log_dir.join("2026-02-17-pilot.md"), "keep1").unwrap();
        fs::write(log_dir.join("2026-02-19-pilot.md"), "keep2").unwrap();
        fs::write(log_dir.join("2026-02-20-pilot.md"), "keep3").unwrap();

        let deleted = logger.cleanup("2026-02-20").unwrap();
        assert_eq!(deleted, 2);

        assert!(!log_dir.join("2026-02-15-pilot.md").exists());
        assert!(!log_dir.join("2026-02-16-pilot.md").exists());
        assert!(log_dir.join("2026-02-17-pilot.md").exists());
        assert!(log_dir.join("2026-02-19-pilot.md").exists());
        assert!(log_dir.join("2026-02-20-pilot.md").exists());
        fs::remove_dir_all(&dir).ok();
    }

    // 11. Read since offset: Write content, read_since(0) gets all, read_since(N) gets only new
    #[test]
    fn read_since_returns_new_content() {
        let dir = temp_dir();
        let mut logger = ConversationLogger::new(&dir, LogConfig::default()).unwrap();
        logger.register_agent("pilot").unwrap();

        let content = "Hello world\nSecond line\n";
        logger
            .process_capture("pilot", content, "2026-02-17")
            .unwrap();

        // Read everything from offset 0.
        let (text, new_offset) = logger.read_since("pilot", 0).unwrap();
        assert_eq!(text, content);
        assert_eq!(new_offset, content.len());

        // Read from middle offset.
        let mid = "Hello world\n".len();
        let (text2, new_offset2) = logger.read_since("pilot", mid).unwrap();
        assert_eq!(text2, "Second line\n");
        assert_eq!(new_offset2, content.len());

        // Read from end — nothing new.
        let (text3, new_offset3) = logger.read_since("pilot", content.len()).unwrap();
        assert_eq!(text3, "");
        assert_eq!(new_offset3, content.len());
        fs::remove_dir_all(&dir).ok();
    }

    // 12. Unregister stops tracking: Unregister agent, verify process_capture returns error
    #[test]
    fn unregister_stops_tracking() {
        let dir = temp_dir();
        let mut logger = ConversationLogger::new(&dir, LogConfig::default()).unwrap();
        logger.register_agent("pilot").unwrap();
        logger.unregister_agent("pilot");

        let result = logger.process_capture("pilot", "test", "2026-02-17");
        assert!(result.is_err());
        match result.unwrap_err() {
            LogError::AgentNotRegistered(name) => assert_eq!(name, "pilot"),
            other => panic!("expected AgentNotRegistered, got: {:?}", other),
        }
        fs::remove_dir_all(&dir).ok();
    }

    // 13. Disabled config: Logger with `enabled: false`, verify process_capture is a no-op
    #[test]
    fn disabled_config_is_noop() {
        let dir = temp_dir();
        let config = LogConfig {
            enabled: false,
            ..LogConfig::default()
        };
        let mut logger = ConversationLogger::new(&dir, config).unwrap();
        logger.register_agent("pilot").unwrap();

        let written = logger
            .process_capture("pilot", "should not be written", "2026-02-17")
            .unwrap();
        assert_eq!(written, 0);

        let log_file = dir.join(".pilot-log/2026-02-17-pilot.md");
        assert!(!log_file.exists());
        fs::remove_dir_all(&dir).ok();
    }

    // 14. Empty capture: Process empty pane content, verify no file growth
    #[test]
    fn empty_capture_no_file_growth() {
        let dir = temp_dir();
        let mut logger = ConversationLogger::new(&dir, LogConfig::default()).unwrap();
        logger.register_agent("pilot").unwrap();

        let written = logger
            .process_capture("pilot", "", "2026-02-17")
            .unwrap();
        assert_eq!(written, 0);

        let log_file = dir.join(".pilot-log/2026-02-17-pilot.md");
        assert!(!log_file.exists());
        fs::remove_dir_all(&dir).ok();
    }

    // ---- Additional edge case tests ----

    #[test]
    fn log_path_returns_correct_path() {
        let dir = temp_dir();
        let mut logger = ConversationLogger::new(&dir, LogConfig::default()).unwrap();
        logger.register_agent("pilot").unwrap();
        logger
            .process_capture("pilot", "test", "2026-02-17")
            .unwrap();

        let path = logger.log_path("pilot").unwrap();
        assert!(path.ends_with("2026-02-17-pilot.md"));

        assert!(logger.log_path("nonexistent").is_none());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn invalid_date_rejected() {
        let dir = temp_dir();
        let mut logger = ConversationLogger::new(&dir, LogConfig::default()).unwrap();
        logger.register_agent("pilot").unwrap();

        let result = logger.process_capture("pilot", "test", "not-a-date");
        assert!(matches!(result, Err(LogError::InvalidDate(_))));

        let result = logger.read_log("pilot", "bad");
        assert!(matches!(result, Err(LogError::InvalidDate(_))));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn agent_offset_tracks_correctly() {
        let dir = temp_dir();
        let mut logger = ConversationLogger::new(&dir, LogConfig::default()).unwrap();
        logger.register_agent("pilot").unwrap();

        assert_eq!(logger.agent_offset("pilot"), Some(0));

        logger
            .process_capture("pilot", "12345", "2026-02-17")
            .unwrap();
        assert_eq!(logger.agent_offset("pilot"), Some(5));

        logger
            .process_capture("pilot", "1234567890", "2026-02-17")
            .unwrap();
        assert_eq!(logger.agent_offset("pilot"), Some(10));

        assert_eq!(logger.agent_offset("nonexistent"), None);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn date_validation() {
        assert!(is_valid_date("2026-02-17"));
        assert!(is_valid_date("2000-01-01"));
        assert!(!is_valid_date(""));
        assert!(!is_valid_date("not-a-date"));
        assert!(!is_valid_date("2026-13-01"));
        assert!(!is_valid_date("2026-00-01"));
        assert!(!is_valid_date("2026-01-32"));
        assert!(!is_valid_date("2026-01-00"));
        assert!(!is_valid_date("1999-01-01"));
    }

    #[test]
    fn default_config_values() {
        let config = LogConfig::default();
        assert!(config.enabled);
        assert!(config.capture_responses);
        assert_eq!(config.retention_days, 7);
        assert_eq!(config.capture_interval_secs, 5);
    }

    #[test]
    fn log_error_display() {
        let io_err = LogError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "file missing",
        ));
        assert!(format!("{}", io_err).contains("I/O error"));

        let agent_err = LogError::AgentNotRegistered("pilot".into());
        assert!(format!("{}", agent_err).contains("pilot"));

        let date_err = LogError::InvalidDate("bad".into());
        assert!(format!("{}", date_err).contains("bad"));
    }
}
