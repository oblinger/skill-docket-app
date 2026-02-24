use std::path::Path;

use crate::types::config::FolderEntry;


/// In-memory registry of named folder paths.
#[derive(Debug, Clone)]
pub struct FolderRegistry {
    entries: Vec<FolderEntry>,
}


impl FolderRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        FolderRegistry {
            entries: Vec::new(),
        }
    }

    /// Load entries from a simple key:value file (one `name: path` per line).
    pub fn load(path: &Path) -> Result<Self, String> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("cannot read {}: {}", path.display(), e))?;
        let mut reg = FolderRegistry::new();
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some(colon_pos) = line.find(':') {
                let name = line[..colon_pos].trim().to_string();
                let path_val = line[colon_pos + 1..].trim();
                // Strip surrounding quotes
                let path_val = strip_quotes(path_val);
                reg.entries.push(FolderEntry {
                    name,
                    path: path_val.to_string(),
                });
            }
        }
        Ok(reg)
    }

    /// Add a folder entry. Fails if the name already exists.
    pub fn add(&mut self, entry: FolderEntry) -> Result<(), String> {
        if self.entries.iter().any(|e| e.name == entry.name) {
            return Err(format!("folder already registered: {}", entry.name));
        }
        self.entries.push(entry);
        Ok(())
    }

    /// Remove a folder entry by name. Fails if not found.
    pub fn remove(&mut self, name: &str) -> Result<(), String> {
        let pos = self
            .entries
            .iter()
            .position(|e| e.name == name)
            .ok_or_else(|| format!("folder not found: {}", name))?;
        self.entries.remove(pos);
        Ok(())
    }

    /// Look up a folder by name.
    pub fn get(&self, name: &str) -> Option<&FolderEntry> {
        self.entries.iter().find(|e| e.name == name)
    }

    /// Return a slice of all entries.
    pub fn list(&self) -> &[FolderEntry] {
        &self.entries
    }

    /// Save entries to a simple key:value file.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        let mut content = String::new();
        for entry in &self.entries {
            content.push_str(&format!("{}: \"{}\"\n", entry.name, entry.path));
        }
        std::fs::write(path, content)
            .map_err(|e| format!("cannot write {}: {}", path.display(), e))
    }
}


impl Default for FolderRegistry {
    fn default() -> Self {
        Self::new()
    }
}


fn strip_quotes(s: &str) -> &str {
    if (s.starts_with('"') && s.ends_with('"'))
        || (s.starts_with('\'') && s.ends_with('\''))
    {
        &s[1..s.len() - 1]
    } else {
        s
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, path: &str) -> FolderEntry {
        FolderEntry {
            name: name.into(),
            path: path.into(),
        }
    }

    #[test]
    fn new_is_empty() {
        let reg = FolderRegistry::new();
        assert!(reg.list().is_empty());
    }

    #[test]
    fn add_and_get() {
        let mut reg = FolderRegistry::new();
        reg.add(entry("core", "/projects/cmx/core")).unwrap();
        let e = reg.get("core").unwrap();
        assert_eq!(e.path, "/projects/cmx/core");
    }

    #[test]
    fn add_duplicate_fails() {
        let mut reg = FolderRegistry::new();
        reg.add(entry("core", "/a")).unwrap();
        let result = reg.add(entry("core", "/b"));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("already registered"));
    }

    #[test]
    fn remove_existing() {
        let mut reg = FolderRegistry::new();
        reg.add(entry("core", "/a")).unwrap();
        reg.remove("core").unwrap();
        assert!(reg.get("core").is_none());
    }

    #[test]
    fn remove_missing_fails() {
        let mut reg = FolderRegistry::new();
        let result = reg.remove("nope");
        assert!(result.is_err());
    }

    #[test]
    fn list_preserves_order() {
        let mut reg = FolderRegistry::new();
        reg.add(entry("alpha", "/a")).unwrap();
        reg.add(entry("beta", "/b")).unwrap();
        let names: Vec<&str> = reg.list().iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["alpha", "beta"]);
    }

    #[test]
    fn save_and_load_round_trip() {
        let dir = std::env::temp_dir().join("cmx_test_folders");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("folders.yaml");

        let mut reg = FolderRegistry::new();
        reg.add(entry("core", "/projects/core")).unwrap();
        reg.add(entry("ui", "/projects/ui")).unwrap();
        reg.save(&path).unwrap();

        let loaded = FolderRegistry::load(&path).unwrap();
        assert_eq!(loaded.list().len(), 2);
        assert_eq!(loaded.get("core").unwrap().path, "/projects/core");
        assert_eq!(loaded.get("ui").unwrap().path, "/projects/ui");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_with_comments() {
        let dir = std::env::temp_dir().join("cmx_test_folders_comments");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("folders.yaml");

        std::fs::write(&path, "# comment\ncore: /a\n\nui: /b\n").unwrap();
        let loaded = FolderRegistry::load(&path).unwrap();
        assert_eq!(loaded.list().len(), 2);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
