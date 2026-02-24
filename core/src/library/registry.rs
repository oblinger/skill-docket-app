use std::collections::HashMap;

use super::source::{LibrarySource, SkillEntry};

// ---------------------------------------------------------------------------
// Registry — ordered source management and conflict resolution
// ---------------------------------------------------------------------------

/// Manages the ordered list of sources and resolves skill name conflicts.
#[derive(Debug)]
pub struct Registry {
    /// All sources in registration order.
    pub(crate) sources: Vec<LibrarySource>,
    /// All discovered entries grouped by skill name.
    pub(crate) skills: HashMap<String, Vec<SkillEntry>>,
    /// After resolution: skill name -> winning entry.
    pub(crate) resolved: HashMap<String, SkillEntry>,
    /// Manual overrides: skill name -> source kind string to prefer.
    pub(crate) overrides: HashMap<String, String>,
}

impl Registry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Registry {
            sources: Vec::new(),
            skills: HashMap::new(),
            resolved: HashMap::new(),
            overrides: HashMap::new(),
        }
    }

    /// Add a source and scan it for skills.
    pub fn add_source(&mut self, source: LibrarySource) {
        let entries = source.scan();
        for entry in entries {
            self.skills
                .entry(entry.name.clone())
                .or_default()
                .push(entry);
        }
        self.sources.push(source);
    }

    /// Set manual overrides (from settings).
    pub fn set_overrides(&mut self, overrides: HashMap<String, String>) {
        self.overrides = overrides;
    }

    /// Resolve conflicts. Must be called after all sources are added.
    ///
    /// Resolution rules:
    /// 1. If an override exists for a skill name, pick the entry from the
    ///    matching source kind (by display string).
    /// 2. Otherwise, highest priority wins.
    /// 3. Returns a list of conflict warnings for skills found in multiple sources.
    pub fn resolve(&mut self) -> Vec<ConflictWarning> {
        let mut warnings = Vec::new();
        self.resolved.clear();

        for (name, entries) in &self.skills {
            if entries.is_empty() {
                continue;
            }

            // Detect conflicts
            if entries.len() > 1 {
                let sources: Vec<String> = entries.iter().map(|e| e.source.to_string()).collect();
                warnings.push(ConflictWarning {
                    skill_name: name.clone(),
                    sources,
                });
            }

            // Try override first
            if let Some(override_source) = self.overrides.get(name) {
                if let Some(entry) = entries
                    .iter()
                    .find(|e| e.source.to_string() == *override_source)
                {
                    self.resolved.insert(name.clone(), entry.clone());
                    continue;
                }
                // Override source not found — fall through to priority
            }

            // Highest priority wins; on tie, last-added wins (stable)
            let winner = entries
                .iter()
                .max_by_key(|e| e.priority)
                .unwrap();
            self.resolved.insert(name.clone(), winner.clone());
        }

        warnings
    }

    /// Get the resolved (winning) entry for a skill name.
    pub fn get(&self, name: &str) -> Option<&SkillEntry> {
        self.resolved.get(name)
    }

    /// List all resolved skill names.
    pub fn list_names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self.resolved.keys().map(|s| s.as_str()).collect();
        names.sort();
        names
    }

    /// List conflicts — skill names appearing in more than one source.
    pub fn conflicts(&self) -> Vec<(&str, Vec<&SkillEntry>)> {
        let mut result = Vec::new();
        for (name, entries) in &self.skills {
            if entries.len() > 1 {
                result.push((name.as_str(), entries.iter().collect()));
            }
        }
        result.sort_by_key(|(name, _)| *name);
        result
    }

    /// Get all sources.
    pub fn sources(&self) -> &[LibrarySource] {
        &self.sources
    }

    /// Full scan: clear all discovered skills and re-scan all sources.
    pub fn rescan(&mut self) {
        self.skills.clear();
        self.resolved.clear();
        for source in &self.sources {
            let entries = source.scan();
            for entry in entries {
                self.skills
                    .entry(entry.name.clone())
                    .or_default()
                    .push(entry);
            }
        }
    }
}

/// Warning about a skill name appearing in multiple sources.
#[derive(Debug, Clone)]
pub struct ConflictWarning {
    pub skill_name: String,
    pub sources: Vec<String>,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::source::{LibrarySource, LibraryType, SourceKind};
    use std::fs;
    use std::path::PathBuf;

    fn make_temp_dir(suffix: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "cmx_registry_{}_{}", std::process::id(), suffix
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn higher_priority_wins() {
        let low_dir = make_temp_dir("low");
        let high_dir = make_temp_dir("high");

        fs::write(low_dir.join("deploy.md"), "# Low priority deploy").unwrap();
        fs::write(high_dir.join("deploy.md"), "# High priority deploy").unwrap();

        let mut reg = Registry::new();
        reg.add_source(LibrarySource {
            kind: SourceKind::System,
            library_type: LibraryType::SkillsOnly,
            path: low_dir.clone(),
            priority: 0,
        });
        reg.add_source(LibrarySource {
            kind: SourceKind::User,
            library_type: LibraryType::SkillsOnly,
            path: high_dir.clone(),
            priority: 20,
        });

        let warnings = reg.resolve();
        assert_eq!(warnings.len(), 1); // one conflict
        assert_eq!(warnings[0].skill_name, "deploy");

        let entry = reg.get("deploy").unwrap();
        assert_eq!(entry.source, SourceKind::User);
        assert_eq!(entry.priority, 20);

        let _ = fs::remove_dir_all(&low_dir);
        let _ = fs::remove_dir_all(&high_dir);
    }

    #[test]
    fn override_beats_priority() {
        let low_dir = make_temp_dir("ovr_low");
        let high_dir = make_temp_dir("ovr_high");

        fs::write(low_dir.join("deploy.md"), "# Low").unwrap();
        fs::write(high_dir.join("deploy.md"), "# High").unwrap();

        let mut reg = Registry::new();
        reg.add_source(LibrarySource {
            kind: SourceKind::System,
            library_type: LibraryType::SkillsOnly,
            path: low_dir.clone(),
            priority: 0,
        });
        reg.add_source(LibrarySource {
            kind: SourceKind::User,
            library_type: LibraryType::SkillsOnly,
            path: high_dir.clone(),
            priority: 20,
        });

        let mut overrides = HashMap::new();
        overrides.insert("deploy".to_string(), "system".to_string());
        reg.set_overrides(overrides);

        reg.resolve();

        let entry = reg.get("deploy").unwrap();
        assert_eq!(entry.source, SourceKind::System);

        let _ = fs::remove_dir_all(&low_dir);
        let _ = fs::remove_dir_all(&high_dir);
    }

    #[test]
    fn list_names_sorted() {
        let dir = make_temp_dir("list");
        fs::write(dir.join("zebra.md"), "# Z").unwrap();
        fs::write(dir.join("alpha.md"), "# A").unwrap();
        fs::write(dir.join("middle.md"), "# M").unwrap();

        let mut reg = Registry::new();
        reg.add_source(LibrarySource {
            kind: SourceKind::User,
            library_type: LibraryType::SkillsOnly,
            path: dir.clone(),
            priority: 10,
        });
        reg.resolve();

        assert_eq!(reg.list_names(), vec!["alpha", "middle", "zebra"]);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn no_conflict_for_single_source() {
        let dir = make_temp_dir("no_conflict");
        fs::write(dir.join("solo.md"), "# Solo").unwrap();

        let mut reg = Registry::new();
        reg.add_source(LibrarySource {
            kind: SourceKind::User,
            library_type: LibraryType::SkillsOnly,
            path: dir.clone(),
            priority: 10,
        });

        let warnings = reg.resolve();
        assert!(warnings.is_empty());
        assert!(reg.conflicts().is_empty());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn rescan_picks_up_changes() {
        let dir = make_temp_dir("rescan");
        fs::write(dir.join("original.md"), "# Original").unwrap();

        let mut reg = Registry::new();
        reg.add_source(LibrarySource {
            kind: SourceKind::User,
            library_type: LibraryType::SkillsOnly,
            path: dir.clone(),
            priority: 10,
        });
        reg.resolve();
        assert_eq!(reg.list_names(), vec!["original"]);

        // Add a new file on disk
        fs::write(dir.join("added.md"), "# Added").unwrap();

        reg.rescan();
        reg.resolve();

        let names = reg.list_names();
        assert!(names.contains(&"original"));
        assert!(names.contains(&"added"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn add_source_dynamically() {
        let dir1 = make_temp_dir("dyn1");
        let dir2 = make_temp_dir("dyn2");
        fs::write(dir1.join("a.md"), "# A").unwrap();
        fs::write(dir2.join("b.md"), "# B").unwrap();

        let mut reg = Registry::new();
        reg.add_source(LibrarySource {
            kind: SourceKind::System,
            library_type: LibraryType::SkillsOnly,
            path: dir1.clone(),
            priority: 0,
        });
        reg.resolve();
        assert_eq!(reg.list_names(), vec!["a"]);

        // Dynamically add second source
        reg.add_source(LibrarySource {
            kind: SourceKind::Registered("extra".into()),
            library_type: LibraryType::SkillsOnly,
            path: dir2.clone(),
            priority: 50,
        });
        reg.resolve();

        let names = reg.list_names();
        assert!(names.contains(&"a"));
        assert!(names.contains(&"b"));

        let _ = fs::remove_dir_all(&dir1);
        let _ = fs::remove_dir_all(&dir2);
    }
}
