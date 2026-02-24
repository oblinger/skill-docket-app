use std::collections::HashMap;
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};


/// Configuration for a copilot agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CopilotConfig {
    /// The copilot's agent name.
    pub name: String,
    /// Which agent this copilot shadows (usually "pilot").
    pub shadows: String,
    /// Whether this copilot is active.
    pub active: bool,
}

/// Tracks sync state for a single copilot.
#[derive(Debug, Clone)]
pub struct CopilotTracker {
    /// The copilot's name.
    pub name: String,
    /// The agent being shadowed.
    pub shadows: String,
    /// Last byte offset delivered to this copilot.
    pub last_delivered_offset: usize,
    /// Number of syncs performed.
    pub sync_count: u64,
}

/// A prepared context update ready to be sent to a copilot.
#[derive(Debug, Clone)]
pub struct ContextUpdate {
    /// The copilot that should receive this.
    pub copilot_name: String,
    /// The new content to deliver.
    pub content: String,
    /// The framing message wrapping the content.
    pub framed_message: String,
    /// The new byte offset after this delivery.
    pub new_offset: usize,
}

/// Error types for copilot sync.
#[derive(Debug)]
pub enum SyncError {
    Io(std::io::Error),
    LogError(String),
    CopilotNotRegistered(String),
}

impl std::fmt::Display for SyncError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SyncError::Io(e) => write!(f, "I/O error: {}", e),
            SyncError::LogError(msg) => write!(f, "log error: {}", msg),
            SyncError::CopilotNotRegistered(name) => {
                write!(f, "copilot '{}' not registered", name)
            }
        }
    }
}

impl std::error::Error for SyncError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            SyncError::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for SyncError {
    fn from(e: std::io::Error) -> Self {
        SyncError::Io(e)
    }
}

/// Serializable form of tracker state for persistence.
#[derive(Debug, Serialize, Deserialize)]
struct TrackerState {
    shadows: String,
    last_delivered_offset: usize,
    sync_count: u64,
}

/// Manages context synchronization for all copilots.
pub struct CopilotSyncManager {
    /// Per-copilot tracking state.
    trackers: HashMap<String, CopilotTracker>,
    /// The log directory (same as ConversationLogger's .pilot-log/).
    log_dir: PathBuf,
}

impl CopilotSyncManager {
    /// Create a new sync manager pointing at the same log directory as ConversationLogger.
    pub fn new(log_dir: PathBuf) -> Self {
        CopilotSyncManager {
            trackers: HashMap::new(),
            log_dir,
        }
    }

    /// Register a copilot for context synchronization.
    pub fn register_copilot(&mut self, config: CopilotConfig) -> Result<(), SyncError> {
        let tracker = CopilotTracker {
            name: config.name.clone(),
            shadows: config.shadows,
            last_delivered_offset: 0,
            sync_count: 0,
        };
        self.trackers.insert(config.name, tracker);
        Ok(())
    }

    /// Unregister a copilot.
    pub fn unregister_copilot(&mut self, name: &str) {
        self.trackers.remove(name);
    }

    /// Check if a copilot has pending updates.
    /// Reads the shadowed agent's log file and compares against the copilot's
    /// last_delivered_offset.
    pub fn has_pending(&self, copilot_name: &str) -> Result<bool, SyncError> {
        let tracker = self
            .trackers
            .get(copilot_name)
            .ok_or_else(|| SyncError::CopilotNotRegistered(copilot_name.to_string()))?;

        let file_size = self.current_log_size(&tracker.shadows)?;
        Ok(file_size > tracker.last_delivered_offset)
    }

    /// Prepare a context update for a copilot.
    /// Reads new content from the shadowed agent's log since last_delivered_offset.
    /// Returns None if there's no new content.
    pub fn prepare_update(
        &self,
        copilot_name: &str,
        date: &str,
    ) -> Result<Option<ContextUpdate>, SyncError> {
        let tracker = self
            .trackers
            .get(copilot_name)
            .ok_or_else(|| SyncError::CopilotNotRegistered(copilot_name.to_string()))?;

        let log_path = self.log_file_path(&tracker.shadows, date);
        if !log_path.exists() {
            return Ok(None);
        }

        let content = self.read_from_offset(&log_path, tracker.last_delivered_offset)?;
        if content.is_empty() {
            return Ok(None);
        }

        let file_size = fs::metadata(&log_path)?.len() as usize;

        let framed_message = format!(
            "--- Context Update ---\n\
             For your reference — recent {} conversation history. No action required.\n\n\
             {}\n\
             --- End Context Update ---",
            tracker.shadows, content
        );

        Ok(Some(ContextUpdate {
            copilot_name: copilot_name.to_string(),
            content,
            framed_message,
            new_offset: file_size,
        }))
    }

    /// Mark an update as delivered (advance the offset).
    /// Call this AFTER successfully sending the update to the copilot's tmux pane.
    pub fn mark_delivered(
        &mut self,
        copilot_name: &str,
        new_offset: usize,
    ) -> Result<(), SyncError> {
        let tracker = self
            .trackers
            .get_mut(copilot_name)
            .ok_or_else(|| SyncError::CopilotNotRegistered(copilot_name.to_string()))?;
        tracker.last_delivered_offset = new_offset;
        tracker.sync_count += 1;
        Ok(())
    }

    /// Prepare updates for ALL copilots that have pending content.
    /// Returns a list of ContextUpdates ready to send.
    pub fn prepare_all_updates(&self, date: &str) -> Result<Vec<ContextUpdate>, SyncError> {
        let mut updates = Vec::new();
        let names: Vec<String> = self.trackers.keys().cloned().collect();
        for name in names {
            if let Some(update) = self.prepare_update(&name, date)? {
                updates.push(update);
            }
        }
        Ok(updates)
    }

    /// Get the sync state for a copilot.
    pub fn tracker(&self, copilot_name: &str) -> Option<&CopilotTracker> {
        self.trackers.get(copilot_name)
    }

    /// Get all registered copilot names.
    pub fn copilot_names(&self) -> Vec<&str> {
        self.trackers.keys().map(|s| s.as_str()).collect()
    }

    /// Number of registered copilots.
    pub fn len(&self) -> usize {
        self.trackers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.trackers.is_empty()
    }

    /// Reset a copilot's offset (e.g., if logs were rotated).
    pub fn reset_offset(&mut self, copilot_name: &str) -> Result<(), SyncError> {
        let tracker = self
            .trackers
            .get_mut(copilot_name)
            .ok_or_else(|| SyncError::CopilotNotRegistered(copilot_name.to_string()))?;
        tracker.last_delivered_offset = 0;
        Ok(())
    }

    /// Save sync state to a JSON file (for persistence across restarts).
    pub fn save_state(&self, path: &Path) -> Result<(), SyncError> {
        let state: HashMap<String, TrackerState> = self
            .trackers
            .iter()
            .map(|(name, tracker)| {
                (
                    name.clone(),
                    TrackerState {
                        shadows: tracker.shadows.clone(),
                        last_delivered_offset: tracker.last_delivered_offset,
                        sync_count: tracker.sync_count,
                    },
                )
            })
            .collect();

        let json = serde_json::to_string_pretty(&state)
            .map_err(|e| SyncError::LogError(format!("JSON serialization failed: {}", e)))?;
        fs::write(path, json)?;
        Ok(())
    }

    /// Load sync state from a JSON file.
    pub fn load_state(&mut self, path: &Path) -> Result<(), SyncError> {
        let json = fs::read_to_string(path)?;
        let state: HashMap<String, TrackerState> = serde_json::from_str(&json)
            .map_err(|e| SyncError::LogError(format!("JSON deserialization failed: {}", e)))?;

        for (name, ts) in state {
            if let Some(tracker) = self.trackers.get_mut(&name) {
                tracker.last_delivered_offset = ts.last_delivered_offset;
                tracker.sync_count = ts.sync_count;
            } else {
                // Re-create tracker from persisted state.
                self.trackers.insert(
                    name.clone(),
                    CopilotTracker {
                        name: name.clone(),
                        shadows: ts.shadows,
                        last_delivered_offset: ts.last_delivered_offset,
                        sync_count: ts.sync_count,
                    },
                );
            }
        }
        Ok(())
    }

    /// Build the log file path for an agent on a given date.
    /// Matches ConversationLogger's naming: `{date}-{agent_name}.md`
    fn log_file_path(&self, agent_name: &str, date: &str) -> PathBuf {
        self.log_dir.join(format!("{}-{}.md", date, agent_name))
    }

    /// Read content from a log file starting at a byte offset.
    fn read_from_offset(&self, path: &Path, offset: usize) -> Result<String, SyncError> {
        let mut file = fs::File::open(path)?;
        let metadata = file.metadata()?;
        let file_size = metadata.len() as usize;

        if offset >= file_size {
            return Ok(String::new());
        }

        file.seek(SeekFrom::Start(offset as u64))?;
        let mut buf = Vec::with_capacity(file_size - offset);
        file.read_to_end(&mut buf)?;

        String::from_utf8(buf).map_err(|e| {
            SyncError::LogError(format!("invalid UTF-8 in log at offset {}: {}", offset, e))
        })
    }

    /// Get the current log size for the latest date file of an agent.
    fn current_log_size(&self, agent_name: &str) -> Result<usize, SyncError> {
        let suffix = format!("-{}.md", agent_name);
        let mut latest_file: Option<(String, PathBuf)> = None;

        let entries = match fs::read_dir(&self.log_dir) {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
            Err(e) => return Err(SyncError::Io(e)),
        };

        for entry in entries {
            let entry = entry?;
            let name = entry.file_name();
            let name_str = name.to_string_lossy().to_string();
            if name_str.ends_with(&suffix) && name_str.len() >= 10 {
                let date_part = name_str[..10].to_string();
                match &latest_file {
                    Some((existing_date, _)) if &date_part > existing_date => {
                        latest_file = Some((date_part, entry.path()));
                    }
                    None => {
                        latest_file = Some((date_part, entry.path()));
                    }
                    _ => {}
                }
            }
        }

        match latest_file {
            Some((_, path)) => {
                let metadata = fs::metadata(&path)?;
                Ok(metadata.len() as usize)
            }
            None => Ok(0),
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_log_dir() -> PathBuf {
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "cmx-test-copilot-sync-{}-{:?}-{}",
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

    fn make_config(name: &str, shadows: &str) -> CopilotConfig {
        CopilotConfig {
            name: name.to_string(),
            shadows: shadows.to_string(),
            active: true,
        }
    }

    /// Write a log file matching ConversationLogger's naming convention.
    fn write_log(log_dir: &Path, agent: &str, date: &str, content: &str) {
        let path = log_dir.join(format!("{}-{}.md", date, agent));
        fs::write(&path, content).unwrap();
    }

    // 1. Create sync manager: New manager, empty state
    #[test]
    fn create_sync_manager_empty() {
        let dir = temp_log_dir();
        let mgr = CopilotSyncManager::new(dir.clone());
        assert!(mgr.is_empty());
        assert_eq!(mgr.len(), 0);
        assert!(mgr.copilot_names().is_empty());
        fs::remove_dir_all(&dir).ok();
    }

    // 2. Register copilot: Register copilot-1 shadowing pilot, verify tracker created
    #[test]
    fn register_copilot_creates_tracker() {
        let dir = temp_log_dir();
        let mut mgr = CopilotSyncManager::new(dir.clone());
        mgr.register_copilot(make_config("copilot-1", "pilot"))
            .unwrap();

        assert_eq!(mgr.len(), 1);
        let tracker = mgr.tracker("copilot-1").unwrap();
        assert_eq!(tracker.name, "copilot-1");
        assert_eq!(tracker.shadows, "pilot");
        assert_eq!(tracker.last_delivered_offset, 0);
        assert_eq!(tracker.sync_count, 0);
        fs::remove_dir_all(&dir).ok();
    }

    // 3. No pending when no log file: has_pending returns false when log doesn't exist
    #[test]
    fn no_pending_when_no_log_file() {
        let dir = temp_log_dir();
        let mut mgr = CopilotSyncManager::new(dir.clone());
        mgr.register_copilot(make_config("copilot-1", "pilot"))
            .unwrap();

        assert!(!mgr.has_pending("copilot-1").unwrap());
        fs::remove_dir_all(&dir).ok();
    }

    // 4. No pending when offset caught up: Write log content, set offset to end,
    //    has_pending returns false
    #[test]
    fn no_pending_when_offset_caught_up() {
        let dir = temp_log_dir();
        let content = "Some log content here.\n";
        write_log(&dir, "pilot", "2026-02-23", content);

        let mut mgr = CopilotSyncManager::new(dir.clone());
        mgr.register_copilot(make_config("copilot-1", "pilot"))
            .unwrap();
        mgr.mark_delivered("copilot-1", content.len()).unwrap();

        assert!(!mgr.has_pending("copilot-1").unwrap());
        fs::remove_dir_all(&dir).ok();
    }

    // 5. Has pending when new content: Write content, offset at 0, has_pending returns true
    #[test]
    fn has_pending_when_new_content() {
        let dir = temp_log_dir();
        write_log(&dir, "pilot", "2026-02-23", "New conversation data.\n");

        let mut mgr = CopilotSyncManager::new(dir.clone());
        mgr.register_copilot(make_config("copilot-1", "pilot"))
            .unwrap();

        assert!(mgr.has_pending("copilot-1").unwrap());
        fs::remove_dir_all(&dir).ok();
    }

    // 6. Prepare update gets new content: Write content, prepare_update returns
    //    the content with framing
    #[test]
    fn prepare_update_gets_new_content() {
        let dir = temp_log_dir();
        let content = "User: Hello\nAssistant: Hi there\n";
        write_log(&dir, "pilot", "2026-02-23", content);

        let mut mgr = CopilotSyncManager::new(dir.clone());
        mgr.register_copilot(make_config("copilot-1", "pilot"))
            .unwrap();

        let update = mgr
            .prepare_update("copilot-1", "2026-02-23")
            .unwrap()
            .unwrap();
        assert_eq!(update.copilot_name, "copilot-1");
        assert_eq!(update.content, content);
        assert_eq!(update.new_offset, content.len());
        assert!(update.framed_message.contains("--- Context Update ---"));
        assert!(update.framed_message.contains(content));
        assert!(update.framed_message.contains("--- End Context Update ---"));
        fs::remove_dir_all(&dir).ok();
    }

    // 7. Mark delivered advances offset: Prepare, mark delivered, verify offset advanced,
    //    no more pending
    #[test]
    fn mark_delivered_advances_offset() {
        let dir = temp_log_dir();
        let content = "Some conversation.\n";
        write_log(&dir, "pilot", "2026-02-23", content);

        let mut mgr = CopilotSyncManager::new(dir.clone());
        mgr.register_copilot(make_config("copilot-1", "pilot"))
            .unwrap();

        let update = mgr
            .prepare_update("copilot-1", "2026-02-23")
            .unwrap()
            .unwrap();
        mgr.mark_delivered("copilot-1", update.new_offset).unwrap();

        let tracker = mgr.tracker("copilot-1").unwrap();
        assert_eq!(tracker.last_delivered_offset, content.len());
        assert_eq!(tracker.sync_count, 1);

        // No more pending.
        let update2 = mgr.prepare_update("copilot-1", "2026-02-23").unwrap();
        assert!(update2.is_none());
        fs::remove_dir_all(&dir).ok();
    }

    // 8. Multiple copilots: Two copilots shadowing same agent, each gets independent updates
    #[test]
    fn multiple_copilots_independent() {
        let dir = temp_log_dir();
        let content = "Shared conversation.\n";
        write_log(&dir, "pilot", "2026-02-23", content);

        let mut mgr = CopilotSyncManager::new(dir.clone());
        mgr.register_copilot(make_config("copilot-1", "pilot"))
            .unwrap();
        mgr.register_copilot(make_config("copilot-2", "pilot"))
            .unwrap();

        // Both have pending.
        assert!(mgr.has_pending("copilot-1").unwrap());
        assert!(mgr.has_pending("copilot-2").unwrap());

        // Deliver to copilot-1 only.
        let update1 = mgr
            .prepare_update("copilot-1", "2026-02-23")
            .unwrap()
            .unwrap();
        mgr.mark_delivered("copilot-1", update1.new_offset)
            .unwrap();

        // copilot-1 caught up, copilot-2 still pending.
        assert!(!mgr.has_pending("copilot-1").unwrap());
        assert!(mgr.has_pending("copilot-2").unwrap());
        fs::remove_dir_all(&dir).ok();
    }

    // 9. Prepare all updates: Register 3 copilots, 2 with pending, prepare_all returns 2 updates
    #[test]
    fn prepare_all_updates_filters_pending() {
        let dir = temp_log_dir();
        let content = "Log content here.\n";
        write_log(&dir, "pilot", "2026-02-23", content);

        let mut mgr = CopilotSyncManager::new(dir.clone());
        mgr.register_copilot(make_config("copilot-1", "pilot"))
            .unwrap();
        mgr.register_copilot(make_config("copilot-2", "pilot"))
            .unwrap();
        mgr.register_copilot(make_config("copilot-3", "pilot"))
            .unwrap();

        // Mark copilot-3 as caught up.
        mgr.mark_delivered("copilot-3", content.len()).unwrap();

        let updates = mgr.prepare_all_updates("2026-02-23").unwrap();
        assert_eq!(updates.len(), 2);

        let names: Vec<&str> = updates.iter().map(|u| u.copilot_name.as_str()).collect();
        assert!(names.contains(&"copilot-1"));
        assert!(names.contains(&"copilot-2"));
        fs::remove_dir_all(&dir).ok();
    }

    // 10. Framing message format: Verify the framed_message includes the header and
    //     footer markers
    #[test]
    fn framing_message_format() {
        let dir = temp_log_dir();
        let content = "User asked a question.\nAssistant answered.\n";
        write_log(&dir, "pilot", "2026-02-23", content);

        let mut mgr = CopilotSyncManager::new(dir.clone());
        mgr.register_copilot(make_config("copilot-1", "pilot"))
            .unwrap();

        let update = mgr
            .prepare_update("copilot-1", "2026-02-23")
            .unwrap()
            .unwrap();

        let msg = &update.framed_message;
        assert!(msg.starts_with("--- Context Update ---\n"));
        assert!(msg.contains(
            "For your reference \u{2014} recent pilot conversation history. No action required.\n"
        ));
        assert!(msg.contains(content));
        assert!(msg.ends_with("\n--- End Context Update ---"));
        fs::remove_dir_all(&dir).ok();
    }

    // 11. Save and load state: Save state, create new manager, load state, verify offsets
    //     preserved
    #[test]
    fn save_and_load_state() {
        let dir = temp_log_dir();
        let state_file = dir.join("sync_state.json");

        // Set up manager with state.
        let mut mgr = CopilotSyncManager::new(dir.clone());
        mgr.register_copilot(make_config("copilot-1", "pilot"))
            .unwrap();
        mgr.register_copilot(make_config("copilot-2", "pilot"))
            .unwrap();
        mgr.mark_delivered("copilot-1", 4523).unwrap();
        mgr.mark_delivered("copilot-2", 1200).unwrap();

        // Save.
        mgr.save_state(&state_file).unwrap();
        assert!(state_file.exists());

        // Load into a fresh manager.
        let mut mgr2 = CopilotSyncManager::new(dir.clone());
        mgr2.load_state(&state_file).unwrap();

        let t1 = mgr2.tracker("copilot-1").unwrap();
        assert_eq!(t1.last_delivered_offset, 4523);
        assert_eq!(t1.sync_count, 1);
        assert_eq!(t1.shadows, "pilot");

        let t2 = mgr2.tracker("copilot-2").unwrap();
        assert_eq!(t2.last_delivered_offset, 1200);
        assert_eq!(t2.sync_count, 1);
        fs::remove_dir_all(&dir).ok();
    }

    // 12. Unregister copilot: Register, unregister, verify prepare_update fails
    #[test]
    fn unregister_copilot_removes_tracker() {
        let dir = temp_log_dir();
        write_log(&dir, "pilot", "2026-02-23", "content\n");

        let mut mgr = CopilotSyncManager::new(dir.clone());
        mgr.register_copilot(make_config("copilot-1", "pilot"))
            .unwrap();
        assert_eq!(mgr.len(), 1);

        mgr.unregister_copilot("copilot-1");
        assert_eq!(mgr.len(), 0);

        let result = mgr.prepare_update("copilot-1", "2026-02-23");
        assert!(result.is_err());
        match result.unwrap_err() {
            SyncError::CopilotNotRegistered(name) => assert_eq!(name, "copilot-1"),
            other => panic!("expected CopilotNotRegistered, got: {:?}", other),
        }
        fs::remove_dir_all(&dir).ok();
    }

    // 13. Reset offset: Set offset to 100, reset, verify offset is 0
    #[test]
    fn reset_offset_to_zero() {
        let dir = temp_log_dir();
        let mut mgr = CopilotSyncManager::new(dir.clone());
        mgr.register_copilot(make_config("copilot-1", "pilot"))
            .unwrap();
        mgr.mark_delivered("copilot-1", 100).unwrap();

        assert_eq!(mgr.tracker("copilot-1").unwrap().last_delivered_offset, 100);

        mgr.reset_offset("copilot-1").unwrap();
        assert_eq!(mgr.tracker("copilot-1").unwrap().last_delivered_offset, 0);
        fs::remove_dir_all(&dir).ok();
    }

    // 14. Copilot shadows different agent: Copilot shadowing "worker1" reads worker1's logs,
    //     not pilot's
    #[test]
    fn copilot_shadows_different_agent() {
        let dir = temp_log_dir();
        write_log(&dir, "pilot", "2026-02-23", "Pilot conversation.\n");
        write_log(&dir, "worker1", "2026-02-23", "Worker1 conversation.\n");

        let mut mgr = CopilotSyncManager::new(dir.clone());
        mgr.register_copilot(make_config("copilot-w", "worker1"))
            .unwrap();

        let update = mgr
            .prepare_update("copilot-w", "2026-02-23")
            .unwrap()
            .unwrap();

        // Should contain worker1's content, not pilot's.
        assert_eq!(update.content, "Worker1 conversation.\n");
        assert!(update
            .framed_message
            .contains("recent worker1 conversation history"));
        assert!(!update.content.contains("Pilot"));
        fs::remove_dir_all(&dir).ok();
    }

    // Additional: prepare_update returns None when no log exists for that date
    #[test]
    fn prepare_update_returns_none_when_no_log() {
        let dir = temp_log_dir();
        let mut mgr = CopilotSyncManager::new(dir.clone());
        mgr.register_copilot(make_config("copilot-1", "pilot"))
            .unwrap();

        let result = mgr.prepare_update("copilot-1", "2026-02-23").unwrap();
        assert!(result.is_none());
        fs::remove_dir_all(&dir).ok();
    }

    // Additional: has_pending errors for unregistered copilot
    #[test]
    fn has_pending_errors_for_unregistered() {
        let dir = temp_log_dir();
        let mgr = CopilotSyncManager::new(dir.clone());

        let result = mgr.has_pending("nonexistent");
        assert!(result.is_err());
        match result.unwrap_err() {
            SyncError::CopilotNotRegistered(name) => assert_eq!(name, "nonexistent"),
            other => panic!("expected CopilotNotRegistered, got: {:?}", other),
        }
        fs::remove_dir_all(&dir).ok();
    }

    // Additional: incremental delivery — partial content
    #[test]
    fn incremental_delivery() {
        let dir = temp_log_dir();
        let content1 = "First line.\n";
        write_log(&dir, "pilot", "2026-02-23", content1);

        let mut mgr = CopilotSyncManager::new(dir.clone());
        mgr.register_copilot(make_config("copilot-1", "pilot"))
            .unwrap();

        // First delivery.
        let update1 = mgr
            .prepare_update("copilot-1", "2026-02-23")
            .unwrap()
            .unwrap();
        assert_eq!(update1.content, "First line.\n");
        mgr.mark_delivered("copilot-1", update1.new_offset)
            .unwrap();

        // Append more content.
        let content2 = "First line.\nSecond line.\n";
        write_log(&dir, "pilot", "2026-02-23", content2);

        // Second delivery — only new content.
        let update2 = mgr
            .prepare_update("copilot-1", "2026-02-23")
            .unwrap()
            .unwrap();
        assert_eq!(update2.content, "Second line.\n");
        fs::remove_dir_all(&dir).ok();
    }
}
