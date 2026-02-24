//! Batch flush and file sync (M10.3).
//!
//! Manages periodic persistence of store state to disk. Tracks dirty files,
//! detects external modifications via mtime, and resolves conflicts where
//! a file is both locally dirty and externally modified.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::SystemTime;


/// Manages dirty tracking, mtime-based external edit detection, and
/// conflict resolution for the parameter store's backing files.
#[derive(Debug, Clone)]
pub struct FlushManager {
    /// Files that need writing.
    dirty_files: HashSet<PathBuf>,
    /// Last known mtime per file (recorded after we write).
    file_mtimes: HashMap<PathBuf, SystemTime>,
    /// Mapping from state paths (dotted) to their backing file paths.
    path_to_file: HashMap<String, PathBuf>,
}

impl FlushManager {
    /// Create a new flush manager with no registered paths.
    pub fn new() -> Self {
        FlushManager {
            dirty_files: HashSet::new(),
            file_mtimes: HashMap::new(),
            path_to_file: HashMap::new(),
        }
    }

    /// Register a mapping from a state path to its backing file.
    ///
    /// Multiple state paths may map to the same file (e.g. all fields
    /// of a task live in one markdown file).
    pub fn register_path(&mut self, state_path: &str, file_path: PathBuf) {
        self.path_to_file.insert(state_path.to_string(), file_path);
    }

    /// Mark a file as needing a write.
    pub fn mark_dirty(&mut self, file_path: &Path) {
        self.dirty_files.insert(file_path.to_path_buf());
    }

    /// Mark dirty by state path — looks up the file mapping and marks
    /// the backing file as dirty.
    pub fn mark_dirty_by_path(&mut self, state_path: &str) {
        if let Some(file_path) = self.path_to_file.get(state_path) {
            self.dirty_files.insert(file_path.clone());
        }
    }

    /// Check for external modifications by comparing current file mtime
    /// against our last recorded mtime.
    ///
    /// Returns paths whose on-disk mtime is newer than what we recorded.
    pub fn check_external_modifications(&self) -> Vec<PathBuf> {
        let mut modified = Vec::new();
        for (path, recorded_mtime) in &self.file_mtimes {
            if let Ok(metadata) = std::fs::metadata(path) {
                if let Ok(current_mtime) = metadata.modified() {
                    if current_mtime > *recorded_mtime {
                        modified.push(path.clone());
                    }
                }
            }
        }
        modified
    }

    /// Get all files that need flushing.
    pub fn dirty_files(&self) -> &HashSet<PathBuf> {
        &self.dirty_files
    }

    /// Record that a file was successfully written.
    ///
    /// Updates the mtime tracking so future external-modification checks
    /// use this write's mtime as the baseline.
    pub fn record_write(&mut self, file_path: &Path) {
        self.dirty_files.remove(file_path);
        if let Ok(metadata) = std::fs::metadata(file_path) {
            if let Ok(mtime) = metadata.modified() {
                self.file_mtimes.insert(file_path.to_path_buf(), mtime);
            }
        }
    }

    /// Clear all dirty state.
    pub fn clear(&mut self) {
        self.dirty_files.clear();
    }

    /// Resolve conflicts: if a file is both dirty (pending local write)
    /// AND externally modified, external edit wins.
    ///
    /// Returns the files whose pending writes should be discarded.
    /// Those files are removed from the dirty set.
    pub fn resolve_conflicts(&mut self) -> Vec<PathBuf> {
        let externally_modified: HashSet<PathBuf> =
            self.check_external_modifications().into_iter().collect();

        let conflicts: Vec<PathBuf> = self
            .dirty_files
            .intersection(&externally_modified)
            .cloned()
            .collect();

        for path in &conflicts {
            self.dirty_files.remove(path);
            // Update our mtime record to the external edit's mtime.
            if let Ok(metadata) = std::fs::metadata(path) {
                if let Ok(mtime) = metadata.modified() {
                    self.file_mtimes.insert(path.clone(), mtime);
                }
            }
        }

        conflicts
    }

    /// Get the file path for a state path, if registered.
    pub fn file_for_path(&self, state_path: &str) -> Option<&PathBuf> {
        self.path_to_file.get(state_path)
    }

    /// Number of dirty files.
    pub fn dirty_count(&self) -> usize {
        self.dirty_files.len()
    }

    /// Number of registered path-to-file mappings.
    pub fn registered_count(&self) -> usize {
        self.path_to_file.len()
    }
}

impl Default for FlushManager {
    fn default() -> Self {
        Self::new()
    }
}


// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;
    use std::thread;
    use std::time::Duration;

    fn test_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join("cmx_flush_tests")
            .join(name);
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn mark_dirty_appears_in_dirty_files() {
        let mut fm = FlushManager::new();
        let path = PathBuf::from("/tmp/cmx_test_a.json");
        fm.mark_dirty(&path);
        assert!(fm.dirty_files().contains(&path));
        assert_eq!(fm.dirty_count(), 1);
    }

    #[test]
    fn mark_dirty_idempotent() {
        let mut fm = FlushManager::new();
        let path = PathBuf::from("/tmp/cmx_test_b.json");
        fm.mark_dirty(&path);
        fm.mark_dirty(&path);
        assert_eq!(fm.dirty_count(), 1);
    }

    #[test]
    fn register_path_and_mark_dirty_by_path() {
        let mut fm = FlushManager::new();
        let file = PathBuf::from("/tmp/cmx_test_c.json");
        fm.register_path("task.AUTH1.status", file.clone());
        fm.mark_dirty_by_path("task.AUTH1.status");
        assert!(fm.dirty_files().contains(&file));
    }

    #[test]
    fn mark_dirty_by_unregistered_path_is_noop() {
        let mut fm = FlushManager::new();
        fm.mark_dirty_by_path("task.NOPE.status");
        assert!(fm.dirty_files().is_empty());
    }

    #[test]
    fn record_write_updates_mtime() {
        let dir = test_dir("record_write");
        let file = dir.join("state.json");
        fs::write(&file, "{}").unwrap();

        let mut fm = FlushManager::new();
        fm.mark_dirty(&file);
        assert_eq!(fm.dirty_count(), 1);

        fm.record_write(&file);
        assert_eq!(fm.dirty_count(), 0);
        assert!(fm.file_mtimes.contains_key(&file));
    }

    #[test]
    fn external_modification_detected() {
        let dir = test_dir("ext_mod");
        let file = dir.join("data.json");
        fs::write(&file, r#"{"v":1}"#).unwrap();

        let mut fm = FlushManager::new();
        fm.record_write(&file);

        // Simulate external edit after a small delay so mtime differs.
        thread::sleep(Duration::from_millis(50));
        {
            let mut f = fs::OpenOptions::new()
                .write(true)
                .truncate(true)
                .open(&file)
                .unwrap();
            f.write_all(b"{\"v\":2}").unwrap();
        }

        let modified = fm.check_external_modifications();
        assert_eq!(modified.len(), 1);
        assert_eq!(modified[0], file);
    }

    #[test]
    fn no_external_modification_when_unchanged() {
        let dir = test_dir("no_ext_mod");
        let file = dir.join("stable.json");
        fs::write(&file, "{}").unwrap();

        let mut fm = FlushManager::new();
        fm.record_write(&file);

        let modified = fm.check_external_modifications();
        assert!(modified.is_empty());
    }

    #[test]
    fn resolve_conflicts_dirty_and_externally_modified() {
        let dir = test_dir("conflict");
        let file = dir.join("conflict.json");
        fs::write(&file, "{}").unwrap();

        let mut fm = FlushManager::new();
        fm.record_write(&file);
        fm.mark_dirty(&file);

        // External edit.
        thread::sleep(Duration::from_millis(50));
        fs::write(&file, "{\"ext\":true}").unwrap();

        let discarded = fm.resolve_conflicts();
        assert_eq!(discarded.len(), 1);
        assert_eq!(discarded[0], file);
        // File should no longer be dirty.
        assert!(fm.dirty_files().is_empty());
    }

    #[test]
    fn resolve_conflicts_dirty_but_not_externally_modified() {
        let dir = test_dir("no_conflict");
        let file = dir.join("local.json");
        fs::write(&file, "{}").unwrap();

        let mut fm = FlushManager::new();
        fm.record_write(&file);
        fm.mark_dirty(&file);

        // No external edit.
        let discarded = fm.resolve_conflicts();
        assert!(discarded.is_empty());
        // File should still be dirty.
        assert!(fm.dirty_files().contains(&file));
    }

    #[test]
    fn clear_removes_all_dirty() {
        let mut fm = FlushManager::new();
        fm.mark_dirty(Path::new("/tmp/a.json"));
        fm.mark_dirty(Path::new("/tmp/b.json"));
        assert_eq!(fm.dirty_count(), 2);
        fm.clear();
        assert_eq!(fm.dirty_count(), 0);
    }

    #[test]
    fn file_for_path_lookup() {
        let mut fm = FlushManager::new();
        let file = PathBuf::from("/projects/tasks.md");
        fm.register_path("task.AUTH1.status", file.clone());
        assert_eq!(fm.file_for_path("task.AUTH1.status"), Some(&file));
        assert_eq!(fm.file_for_path("task.NOPE"), None);
    }

    #[test]
    fn multiple_paths_same_file() {
        let mut fm = FlushManager::new();
        let file = PathBuf::from("/projects/tasks.md");
        fm.register_path("task.AUTH1.status", file.clone());
        fm.register_path("task.AUTH1.assignee", file.clone());
        assert_eq!(fm.registered_count(), 2);

        fm.mark_dirty_by_path("task.AUTH1.status");
        fm.mark_dirty_by_path("task.AUTH1.assignee");
        // Both map to the same file, so only one dirty entry.
        assert_eq!(fm.dirty_count(), 1);
    }

    #[test]
    fn check_external_on_missing_file() {
        let mut fm = FlushManager::new();
        let path = PathBuf::from("/nonexistent/file.json");
        fm.file_mtimes.insert(path, SystemTime::now());
        // File doesn't exist — should not crash, just return empty.
        let modified = fm.check_external_modifications();
        assert!(modified.is_empty());
    }

    #[test]
    fn default_trait() {
        let fm = FlushManager::default();
        assert_eq!(fm.dirty_count(), 0);
        assert_eq!(fm.registered_count(), 0);
    }
}
