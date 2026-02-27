pub mod agent;
pub mod config_doc;
pub mod folders;
pub mod learnings;
pub mod messages;
pub mod roadmap;
pub mod settings;
pub mod task_tree;

// M2 modules
pub mod config;
pub mod merge;
pub mod project_config;
pub mod scanner;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::types::config::Settings;

pub use agent::AgentRegistry;
pub use config_doc::{AgentEntry, ConfigDoc};
pub use folders::FolderRegistry;
pub use messages::MessageStore;
pub use task_tree::TaskTree;

// M2 re-exports
pub use config::layout_expr::{parse_layout_expr, serialize_layout_expr};
pub use config::tiles::TileRegistry;
pub use merge::merge_task_trees;
pub use scanner::scan_tasks;


/// Central data store owning all persistent CMX state.
pub struct Data {
    settings: Settings,
    agents: AgentRegistry,
    tasks: TaskTree,
    folders: FolderRegistry,
    messages: MessageStore,
    config_dir: PathBuf,
    /// In-memory layout expressions keyed by session name.
    /// Populated by layout capture; persisted to ConfigDoc on save.
    layouts: HashMap<String, String>,
    /// Roadmap file paths loaded via `roadmap.load`, used for write-back.
    roadmap_paths: Vec<PathBuf>,
}


impl Data {
    /// Create a new Data instance, loading settings from `config_dir/settings.yaml`.
    /// If the settings file does not exist, the install module creates the
    /// directory structure and writes defaults before loading proceeds.
    /// Also loads folders from `config_dir/folders.yaml` if present.
    pub fn new(config_dir: &Path) -> Result<Data, String> {
        // Ensure CMX is installed (creates dirs, writes defaults if needed)
        crate::install::ensure_installed(config_dir)?;

        // Now proceed with loading — settings.yaml is guaranteed to exist
        let settings_path = config_dir.join("settings.yaml");
        let settings = settings::load(&settings_path)?;

        let folders_path = config_dir.join("folders.yaml");
        let folders = if folders_path.exists() {
            FolderRegistry::load(&folders_path)?
        } else {
            FolderRegistry::new()
        };

        Ok(Data {
            settings,
            agents: AgentRegistry::new(),
            tasks: TaskTree::new(),
            folders,
            messages: MessageStore::new(),
            config_dir: config_dir.to_path_buf(),
            layouts: HashMap::new(),
            roadmap_paths: Vec::new(),
        })
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    pub fn agents(&self) -> &AgentRegistry {
        &self.agents
    }

    pub fn agents_mut(&mut self) -> &mut AgentRegistry {
        &mut self.agents
    }

    pub fn tasks(&self) -> &TaskTree {
        &self.tasks
    }

    pub fn tasks_mut(&mut self) -> &mut TaskTree {
        &mut self.tasks
    }

    pub fn folders(&self) -> &FolderRegistry {
        &self.folders
    }

    pub fn folders_mut(&mut self) -> &mut FolderRegistry {
        &mut self.folders
    }

    pub fn messages(&self) -> &MessageStore {
        &self.messages
    }

    pub fn messages_mut(&mut self) -> &mut MessageStore {
        &mut self.messages
    }

    pub fn config_dir(&self) -> &Path {
        &self.config_dir
    }

    /// Return the stored layout expressions.
    pub fn layouts(&self) -> &HashMap<String, String> {
        &self.layouts
    }

    /// Look up the layout expression for a specific session.
    pub fn layout_for(&self, session: &str) -> Option<&str> {
        self.layouts.get(session).map(|s| s.as_str())
    }

    /// Update (or insert) the layout expression for a session.
    pub fn update_layout(&mut self, session: &str, layout_expr: &str) {
        self.layouts
            .insert(session.to_string(), layout_expr.to_string());
    }

    /// Return the loaded roadmap file paths.
    pub fn roadmap_paths(&self) -> &[PathBuf] {
        &self.roadmap_paths
    }

    /// Register a roadmap file path for write-back.
    pub fn add_roadmap_path(&mut self, path: PathBuf) {
        if !self.roadmap_paths.contains(&path) {
            self.roadmap_paths.push(path);
        }
    }
}


/// Convenience wrapper for parsing/serializing Roadmap.md files into TaskNode trees.
pub struct RoadmapParser;

impl RoadmapParser {
    pub fn parse(content: &str) -> Result<Vec<crate::types::task::TaskNode>, String> {
        roadmap::parse(content)
    }

    pub fn serialize(tasks: &[crate::types::task::TaskNode]) -> String {
        roadmap::serialize(tasks)
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_new_with_missing_dir_uses_defaults() {
        let data = Data::new(Path::new("/tmp/cmx_nonexistent_test_dir_12345"));
        assert!(data.is_ok());
        let data = data.unwrap();
        assert_eq!(data.settings().health_check_interval, 5000);
        assert!(data.agents().list().is_empty());
        assert!(data.tasks().roots().is_empty());
    }

    #[test]
    fn data_new_with_settings_file() {
        let dir = std::env::temp_dir().join("cmx_test_data_new");
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(
            dir.join("settings.yaml"),
            "health_check_interval: 9999\nmax_retries: 11\n",
        )
        .unwrap();

        let data = Data::new(&dir).unwrap();
        assert_eq!(data.settings().health_check_interval, 9999);
        assert_eq!(data.settings().max_retries, 11);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn data_getters_work() {
        let data = Data::new(Path::new("/tmp/cmx_nonexistent_test_dir_67890")).unwrap();
        let _ = data.settings();
        let _ = data.agents();
        let _ = data.tasks();
        let _ = data.folders();
        let _ = data.messages();
        let _ = data.config_dir();
    }

    #[test]
    fn data_mut_getters_work() {
        let mut data = Data::new(Path::new("/tmp/cmx_nonexistent_test_dir_abcde")).unwrap();
        let _ = data.agents_mut();
        let _ = data.tasks_mut();
        let _ = data.folders_mut();
        let _ = data.messages_mut();
    }
}
