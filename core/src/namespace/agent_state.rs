//! Agent state persistence (M10.4).
//!
//! Manages per-agent runtime state stored on disk at
//! `~/.config/cmx/agents/<role>/<name>/state.json`.
//! Supports atomic writes (write-to-temp then rename), agent listing,
//! and cleanup on agent kill.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use serde_json::Value;


/// Manages agent state files on disk.
#[derive(Debug, Clone)]
pub struct AgentStateManager {
    /// Root directory for agent state, e.g. `~/.config/cmx/agents/`.
    base_dir: PathBuf,
}

impl AgentStateManager {
    /// Create a new manager rooted at `config_dir/agents/`.
    ///
    /// Creates the directory if it doesn't exist.
    pub fn new(config_dir: &Path) -> Result<Self, String> {
        let base_dir = config_dir.join("agents");
        fs::create_dir_all(&base_dir)
            .map_err(|e| format!("failed to create agents dir: {}", e))?;
        Ok(AgentStateManager { base_dir })
    }

    /// Get the state file path for an agent.
    ///
    /// Returns `<base_dir>/<role>/<name>/state.json`.
    pub fn state_path(&self, role: &str, name: &str) -> PathBuf {
        self.base_dir.join(role).join(name).join("state.json")
    }

    /// Read agent state from disk.
    ///
    /// Returns an empty map if the state file doesn't exist.
    pub fn read_state(
        &self,
        role: &str,
        name: &str,
    ) -> Result<HashMap<String, Value>, String> {
        let path = self.state_path(role, name);
        if !path.exists() {
            return Ok(HashMap::new());
        }
        let contents = fs::read_to_string(&path)
            .map_err(|e| format!("failed to read {}: {}", path.display(), e))?;
        let map: HashMap<String, Value> = serde_json::from_str(&contents)
            .map_err(|e| format!("failed to parse {}: {}", path.display(), e))?;
        Ok(map)
    }

    /// Write agent state to disk.
    ///
    /// Uses atomic write: writes to a temporary file in the same directory,
    /// then renames to the final path. This prevents partial-write corruption.
    pub fn write_state(
        &self,
        role: &str,
        name: &str,
        state: &HashMap<String, Value>,
    ) -> Result<(), String> {
        let path = self.state_path(role, name);
        let parent = path
            .parent()
            .ok_or_else(|| "invalid state path".to_string())?;
        fs::create_dir_all(parent)
            .map_err(|e| format!("failed to create dir {}: {}", parent.display(), e))?;

        let json = serde_json::to_string_pretty(state)
            .map_err(|e| format!("failed to serialize state: {}", e))?;

        // Write to temp file, then rename for atomicity.
        let tmp_path = parent.join(".state.json.tmp");
        fs::write(&tmp_path, &json)
            .map_err(|e| format!("failed to write {}: {}", tmp_path.display(), e))?;
        fs::rename(&tmp_path, &path)
            .map_err(|e| format!("failed to rename {} to {}: {}", tmp_path.display(), path.display(), e))?;

        Ok(())
    }

    /// Delete agent state (cleanup on agent kill).
    ///
    /// Removes the state file and cleans up empty parent directories
    /// (the agent's name dir and role dir, if empty).
    pub fn delete_state(&self, role: &str, name: &str) -> Result<(), String> {
        let path = self.state_path(role, name);
        if path.exists() {
            fs::remove_file(&path)
                .map_err(|e| format!("failed to remove {}: {}", path.display(), e))?;
        }

        // Clean up empty directories: name dir, then role dir.
        let name_dir = self.base_dir.join(role).join(name);
        if name_dir.exists() && dir_is_empty(&name_dir) {
            let _ = fs::remove_dir(&name_dir);
        }
        let role_dir = self.base_dir.join(role);
        if role_dir.exists() && dir_is_empty(&role_dir) {
            let _ = fs::remove_dir(&role_dir);
        }

        Ok(())
    }

    /// List all agents with state files.
    ///
    /// Returns `(role, name)` pairs for every agent that has a state.json.
    pub fn list_agents(&self) -> Result<Vec<(String, String)>, String> {
        let mut agents = Vec::new();

        if !self.base_dir.exists() {
            return Ok(agents);
        }

        let role_entries = fs::read_dir(&self.base_dir)
            .map_err(|e| format!("failed to read agents dir: {}", e))?;

        for role_entry in role_entries {
            let role_entry = role_entry
                .map_err(|e| format!("failed to read dir entry: {}", e))?;
            let role_path = role_entry.path();
            if !role_path.is_dir() {
                continue;
            }
            let role_name = role_entry
                .file_name()
                .to_string_lossy()
                .to_string();

            let name_entries = fs::read_dir(&role_path)
                .map_err(|e| format!("failed to read role dir: {}", e))?;

            for name_entry in name_entries {
                let name_entry = name_entry
                    .map_err(|e| format!("failed to read dir entry: {}", e))?;
                let name_path = name_entry.path();
                if !name_path.is_dir() {
                    continue;
                }
                let agent_name = name_entry
                    .file_name()
                    .to_string_lossy()
                    .to_string();

                let state_file = name_path.join("state.json");
                if state_file.exists() {
                    agents.push((role_name.clone(), agent_name));
                }
            }
        }

        agents.sort();
        Ok(agents)
    }

    /// The base directory for agent state.
    pub fn base_dir(&self) -> &Path {
        &self.base_dir
    }
}


/// Check if a directory is empty.
fn dir_is_empty(path: &Path) -> bool {
    match fs::read_dir(path) {
        Ok(mut entries) => entries.next().is_none(),
        Err(_) => false,
    }
}


// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn test_config_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join("cmx_agent_state_tests")
            .join(name);
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn create_and_read_back() {
        let dir = test_config_dir("create_read");
        let mgr = AgentStateManager::new(&dir).unwrap();

        let mut state = HashMap::new();
        state.insert("status".into(), json!("running"));
        state.insert("task_id".into(), json!("AUTH1"));
        state.insert("progress".into(), json!(0.5));

        mgr.write_state("worker", "worker1", &state).unwrap();
        let read = mgr.read_state("worker", "worker1").unwrap();

        assert_eq!(read.get("status").unwrap(), &json!("running"));
        assert_eq!(read.get("task_id").unwrap(), &json!("AUTH1"));
        assert_eq!(read.get("progress").unwrap(), &json!(0.5));
    }

    #[test]
    fn read_nonexistent_returns_empty() {
        let dir = test_config_dir("read_nonexistent");
        let mgr = AgentStateManager::new(&dir).unwrap();

        let state = mgr.read_state("worker", "ghost").unwrap();
        assert!(state.is_empty());
    }

    #[test]
    fn state_path_format() {
        let dir = test_config_dir("path_fmt");
        let mgr = AgentStateManager::new(&dir).unwrap();

        let path = mgr.state_path("worker", "worker1");
        assert!(path.ends_with("agents/worker/worker1/state.json"));
    }

    #[test]
    fn write_various_value_types() {
        let dir = test_config_dir("value_types");
        let mgr = AgentStateManager::new(&dir).unwrap();

        let mut state = HashMap::new();
        state.insert("string_val".into(), json!("hello"));
        state.insert("int_val".into(), json!(42));
        state.insert("float_val".into(), json!(3.14));
        state.insert("bool_val".into(), json!(true));
        state.insert("null_val".into(), json!(null));
        state.insert("array_val".into(), json!([1, 2, 3]));
        state.insert(
            "object_val".into(),
            json!({"nested": "value", "count": 5}),
        );

        mgr.write_state("pm", "pm1", &state).unwrap();
        let read = mgr.read_state("pm", "pm1").unwrap();

        assert_eq!(read.get("string_val").unwrap(), &json!("hello"));
        assert_eq!(read.get("int_val").unwrap(), &json!(42));
        assert_eq!(read.get("float_val").unwrap(), &json!(3.14));
        assert_eq!(read.get("bool_val").unwrap(), &json!(true));
        assert!(read.get("null_val").unwrap().is_null());
        assert_eq!(read.get("array_val").unwrap(), &json!([1, 2, 3]));
        assert_eq!(
            read.get("object_val").unwrap(),
            &json!({"nested": "value", "count": 5})
        );
    }

    #[test]
    fn overwrite_state() {
        let dir = test_config_dir("overwrite");
        let mgr = AgentStateManager::new(&dir).unwrap();

        let mut state1 = HashMap::new();
        state1.insert("status".into(), json!("running"));
        mgr.write_state("worker", "w1", &state1).unwrap();

        let mut state2 = HashMap::new();
        state2.insert("status".into(), json!("complete"));
        state2.insert("result".into(), json!("success"));
        mgr.write_state("worker", "w1", &state2).unwrap();

        let read = mgr.read_state("worker", "w1").unwrap();
        assert_eq!(read.get("status").unwrap(), &json!("complete"));
        assert_eq!(read.get("result").unwrap(), &json!("success"));
        // Old keys not in new state should be gone.
        assert_eq!(read.len(), 2);
    }

    #[test]
    fn delete_state_removes_file() {
        let dir = test_config_dir("delete");
        let mgr = AgentStateManager::new(&dir).unwrap();

        let mut state = HashMap::new();
        state.insert("x".into(), json!(1));
        mgr.write_state("worker", "w1", &state).unwrap();
        assert!(mgr.state_path("worker", "w1").exists());

        mgr.delete_state("worker", "w1").unwrap();
        assert!(!mgr.state_path("worker", "w1").exists());
    }

    #[test]
    fn delete_cleans_empty_dirs() {
        let dir = test_config_dir("delete_dirs");
        let mgr = AgentStateManager::new(&dir).unwrap();

        let mut state = HashMap::new();
        state.insert("x".into(), json!(1));
        mgr.write_state("worker", "w1", &state).unwrap();

        let name_dir = dir.join("agents").join("worker").join("w1");
        let role_dir = dir.join("agents").join("worker");
        assert!(name_dir.exists());

        mgr.delete_state("worker", "w1").unwrap();

        // Both name dir and role dir should be cleaned up (empty).
        assert!(!name_dir.exists());
        assert!(!role_dir.exists());
    }

    #[test]
    fn delete_preserves_nonempty_dirs() {
        let dir = test_config_dir("delete_preserve");
        let mgr = AgentStateManager::new(&dir).unwrap();

        let mut state = HashMap::new();
        state.insert("x".into(), json!(1));
        mgr.write_state("worker", "w1", &state).unwrap();
        mgr.write_state("worker", "w2", &state).unwrap();

        mgr.delete_state("worker", "w1").unwrap();

        // w1 dir gone, but role dir (worker) still has w2.
        let name_dir = dir.join("agents").join("worker").join("w1");
        let role_dir = dir.join("agents").join("worker");
        assert!(!name_dir.exists());
        assert!(role_dir.exists());
    }

    #[test]
    fn delete_nonexistent_is_ok() {
        let dir = test_config_dir("delete_noop");
        let mgr = AgentStateManager::new(&dir).unwrap();
        // Should not error.
        mgr.delete_state("worker", "ghost").unwrap();
    }

    #[test]
    fn list_agents_finds_created() {
        let dir = test_config_dir("list");
        let mgr = AgentStateManager::new(&dir).unwrap();

        let state = HashMap::new();
        mgr.write_state("worker", "w1", &state).unwrap();
        mgr.write_state("worker", "w2", &state).unwrap();
        mgr.write_state("pm", "pm1", &state).unwrap();

        let mut agents = mgr.list_agents().unwrap();
        agents.sort();

        assert_eq!(agents.len(), 3);
        assert_eq!(agents[0], ("pm".into(), "pm1".into()));
        assert_eq!(agents[1], ("worker".into(), "w1".into()));
        assert_eq!(agents[2], ("worker".into(), "w2".into()));
    }

    #[test]
    fn list_agents_empty() {
        let dir = test_config_dir("list_empty");
        let mgr = AgentStateManager::new(&dir).unwrap();
        let agents = mgr.list_agents().unwrap();
        assert!(agents.is_empty());
    }

    #[test]
    fn list_agents_after_delete() {
        let dir = test_config_dir("list_after_del");
        let mgr = AgentStateManager::new(&dir).unwrap();

        let state = HashMap::new();
        mgr.write_state("worker", "w1", &state).unwrap();
        mgr.write_state("worker", "w2", &state).unwrap();
        mgr.delete_state("worker", "w1").unwrap();

        let agents = mgr.list_agents().unwrap();
        assert_eq!(agents.len(), 1);
        assert_eq!(agents[0], ("worker".into(), "w2".into()));
    }

    #[test]
    fn atomic_write_no_partial() {
        let dir = test_config_dir("atomic");
        let mgr = AgentStateManager::new(&dir).unwrap();

        // Write initial state.
        let mut state1 = HashMap::new();
        state1.insert("version".into(), json!(1));
        mgr.write_state("worker", "w1", &state1).unwrap();

        // Write new state (overwrites atomically).
        let mut state2 = HashMap::new();
        state2.insert("version".into(), json!(2));
        state2.insert("big_field".into(), json!("x".repeat(10000)));
        mgr.write_state("worker", "w1", &state2).unwrap();

        // Read should get version 2 with all fields.
        let read = mgr.read_state("worker", "w1").unwrap();
        assert_eq!(read.get("version").unwrap(), &json!(2));
        assert!(read.contains_key("big_field"));

        // No temp file should linger.
        let tmp = dir.join("agents").join("worker").join("w1").join(".state.json.tmp");
        assert!(!tmp.exists());
    }

    #[test]
    fn base_dir_accessor() {
        let dir = test_config_dir("base_dir");
        let mgr = AgentStateManager::new(&dir).unwrap();
        assert_eq!(mgr.base_dir(), dir.join("agents"));
    }
}
