pub mod errors;
pub mod query;
pub mod registry;
pub mod source;

pub use errors::LibraryError;
pub use registry::ConflictWarning;
pub use source::{LibrarySource, LibraryType, SkillEntry, SourceKind};

use std::collections::HashMap;
use std::path::PathBuf;

use crate::skill::types::{SkillDocument, SkillKind};

use registry::Registry;

// ---------------------------------------------------------------------------
// Library configuration
// ---------------------------------------------------------------------------

/// Configuration for constructing a Library.
/// Replaces a full Settings type that doesn't exist yet.
#[derive(Debug, Clone, Default)]
pub struct LibraryConfig {
    /// Optional project directory for project-local skill discovery.
    pub project_dir: Option<PathBuf>,
    /// Additional registered sources from settings.
    pub extra_sources: Vec<ExtraSource>,
    /// Manual overrides: skill name -> source kind display string.
    pub overrides: HashMap<String, String>,
}

/// An extra source to register (from settings.yaml).
#[derive(Debug, Clone)]
pub struct ExtraSource {
    pub path: PathBuf,
    pub library_type: LibraryType,
    pub priority: u32,
    /// A label for this source.
    pub name: String,
}

// ---------------------------------------------------------------------------
// Library — the main public interface
// ---------------------------------------------------------------------------

/// Merged view of an ordered stack of skill sources.
///
/// Skills are discovered eagerly (file paths) but parsed lazily (file
/// contents are only read when `get_parsed()` or `list_by_kind()` is called).
#[derive(Debug)]
pub struct Library {
    registry: Registry,
}

impl Library {
    /// Create a new library from configuration.
    ///
    /// Steps:
    /// 1. Build default sources (system, Anthropic locations, user library).
    /// 2. Add any extra registered sources from config.
    /// 3. Set overrides.
    /// 4. Scan all sources and resolve conflicts.
    pub fn new(config: &LibraryConfig) -> Result<Library, LibraryError> {
        let mut registry = Registry::new();

        // Add default sources
        let defaults = source::default_sources(config.project_dir.as_deref());
        for src in defaults {
            registry.add_source(src);
        }

        // Add extra registered sources
        for extra in &config.extra_sources {
            let source = LibrarySource {
                kind: SourceKind::Registered(extra.name.clone()),
                library_type: extra.library_type.clone(),
                path: extra.path.clone(),
                priority: extra.priority,
            };
            registry.add_source(source);
        }

        // Set overrides and resolve
        registry.set_overrides(config.overrides.clone());
        let _warnings = registry.resolve();

        Ok(Library { registry })
    }

    /// Create an empty library (no default sources).
    /// Useful for testing.
    pub fn empty() -> Library {
        Library {
            registry: Registry::new(),
        }
    }

    /// Get a skill by name. Returns the resolved (winning) entry.
    pub fn get(&self, name: &str) -> Option<&SkillEntry> {
        query::get(&self.registry, name)
    }

    /// Get and parse a skill by name. Parses the file on demand.
    pub fn get_parsed(&self, name: &str) -> Result<SkillDocument, LibraryError> {
        query::get_parsed(&self.registry, name)
    }

    /// List all skill names in the resolved library (sorted).
    pub fn list(&self) -> Vec<&str> {
        query::list(&self.registry)
    }

    /// List skills filtered by kind (requires parsing each skill).
    /// Use sparingly — this parses every resolved skill file.
    pub fn list_by_kind(
        &self,
        kind: SkillKind,
    ) -> Result<Vec<(String, SkillDocument)>, LibraryError> {
        query::list_by_kind(&self.registry, kind)
    }

    /// List all sources in registration order.
    pub fn sources(&self) -> &[LibrarySource] {
        self.registry.sources()
    }

    /// List conflicts — skill names appearing in multiple sources.
    pub fn conflicts(&self) -> Vec<(&str, Vec<&SkillEntry>)> {
        self.registry.conflicts()
    }

    /// Add a new source. Re-resolves conflicts after adding.
    pub fn add_source(&mut self, source: LibrarySource) -> Result<Vec<ConflictWarning>, LibraryError> {
        self.registry.add_source(source);
        let warnings = self.registry.resolve();
        Ok(warnings)
    }

    /// Reload: re-scan all source folders from disk and re-resolve conflicts.
    pub fn reload(&mut self) -> Result<Vec<ConflictWarning>, LibraryError> {
        self.registry.rescan();
        let warnings = self.registry.resolve();
        Ok(warnings)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn make_temp_dir(suffix: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "cmx_library_{}_{}", std::process::id(), suffix
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn simple_skill(name: &str) -> String {
        format!(
            "---\nname: {}\ndescription: A test skill\n---\n\nDo the thing.\n",
            name
        )
    }

    fn structured_skill(name: &str) -> String {
        format!(
            r#"---
name: {}
description: A structured skill
---

| Fields | Type | Description |
|--------|------|-------------|
| input | string | The input |

Do the structured thing.
"#,
            name
        )
    }

    fn orchestration_skill(name: &str) -> String {
        format!(
            r#"---
name: {}
description: An orchestration skill
---

| Nodes | Role | Description |
|-------|------|-------------|
| planner | pm | Plans the work |

| Edges | To | Condition |
|-------|-----|-----------|
| START | planner | always |

Orchestrate the thing.
"#,
            name
        )
    }

    #[test]
    fn empty_library_has_no_skills() {
        let lib = Library::empty();
        assert!(lib.list().is_empty());
        assert!(lib.get("anything").is_none());
    }

    #[test]
    fn stacking_order_respected() {
        let low = make_temp_dir("stack_low");
        let high = make_temp_dir("stack_high");
        fs::write(low.join("deploy.md"), simple_skill("deploy-low")).unwrap();
        fs::write(high.join("deploy.md"), simple_skill("deploy-high")).unwrap();

        let config = LibraryConfig {
            project_dir: None,
            extra_sources: vec![
                ExtraSource {
                    path: low.clone(),
                    library_type: LibraryType::SkillsOnly,
                    priority: 5,
                    name: "low".into(),
                },
                ExtraSource {
                    path: high.clone(),
                    library_type: LibraryType::SkillsOnly,
                    priority: 50,
                    name: "high".into(),
                },
            ],
            overrides: HashMap::new(),
        };

        let lib = Library::new(&config).unwrap();
        let entry = lib.get("deploy").unwrap();
        assert_eq!(entry.priority, 50);

        // Verify the conflict is reported
        assert_eq!(lib.conflicts().len(), 1);

        let _ = fs::remove_dir_all(&low);
        let _ = fs::remove_dir_all(&high);
    }

    #[test]
    fn overrides_from_config() {
        let low = make_temp_dir("ovr_cfg_low");
        let high = make_temp_dir("ovr_cfg_high");
        fs::write(low.join("deploy.md"), simple_skill("deploy-low")).unwrap();
        fs::write(high.join("deploy.md"), simple_skill("deploy-high")).unwrap();

        let mut overrides = HashMap::new();
        overrides.insert("deploy".into(), "registered:low".into());

        let config = LibraryConfig {
            project_dir: None,
            extra_sources: vec![
                ExtraSource {
                    path: low.clone(),
                    library_type: LibraryType::SkillsOnly,
                    priority: 5,
                    name: "low".into(),
                },
                ExtraSource {
                    path: high.clone(),
                    library_type: LibraryType::SkillsOnly,
                    priority: 50,
                    name: "high".into(),
                },
            ],
            overrides,
        };

        let lib = Library::new(&config).unwrap();
        let entry = lib.get("deploy").unwrap();
        // Override picked low-priority source
        assert_eq!(entry.priority, 5);

        let _ = fs::remove_dir_all(&low);
        let _ = fs::remove_dir_all(&high);
    }

    #[test]
    fn anthropic_style_discovery() {
        let dir = make_temp_dir("anthropic_lib");
        let skill_a = dir.join("code-review");
        let skill_b = dir.join("testing");
        fs::create_dir_all(&skill_a).unwrap();
        fs::create_dir_all(&skill_b).unwrap();
        fs::write(
            skill_a.join("SKILL.md"),
            simple_skill("code-review"),
        ).unwrap();
        fs::write(skill_b.join("SKILL.md"), simple_skill("testing")).unwrap();

        let mut lib = Library::empty();
        lib.add_source(LibrarySource {
            kind: SourceKind::AnthropicDefault,
            library_type: LibraryType::AnthropicStandard,
            path: dir.clone(),
            priority: 10,
        }).unwrap();

        let names = lib.list();
        assert!(names.contains(&"code-review"));
        assert!(names.contains(&"testing"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn flat_skills_discovery() {
        let dir = make_temp_dir("flat_lib");
        fs::write(dir.join("deploy.md"), simple_skill("deploy")).unwrap();
        fs::write(dir.join("rollback.md"), simple_skill("rollback")).unwrap();

        let mut lib = Library::empty();
        lib.add_source(LibrarySource {
            kind: SourceKind::User,
            library_type: LibraryType::SkillsOnly,
            path: dir.clone(),
            priority: 20,
        }).unwrap();

        let names = lib.list();
        assert_eq!(names.len(), 2);
        assert!(names.contains(&"deploy"));
        assert!(names.contains(&"rollback"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn list_by_kind_classifies() {
        let dir = make_temp_dir("classify");
        fs::write(dir.join("simple.md"), simple_skill("simple")).unwrap();
        fs::write(dir.join("structured.md"), structured_skill("structured")).unwrap();
        fs::write(dir.join("orch.md"), orchestration_skill("orch")).unwrap();

        let mut lib = Library::empty();
        lib.add_source(LibrarySource {
            kind: SourceKind::User,
            library_type: LibraryType::SkillsOnly,
            path: dir.clone(),
            priority: 10,
        }).unwrap();

        let simple = lib.list_by_kind(SkillKind::Simple).unwrap();
        assert_eq!(simple.len(), 1);

        let structured = lib.list_by_kind(SkillKind::Structured).unwrap();
        assert_eq!(structured.len(), 1);

        let orch = lib.list_by_kind(SkillKind::Orchestration).unwrap();
        assert_eq!(orch.len(), 1);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn add_source_dynamically() {
        let dir1 = make_temp_dir("dyn_add1");
        let dir2 = make_temp_dir("dyn_add2");
        fs::write(dir1.join("a.md"), simple_skill("a")).unwrap();
        fs::write(dir2.join("b.md"), simple_skill("b")).unwrap();

        let mut lib = Library::empty();
        lib.add_source(LibrarySource {
            kind: SourceKind::User,
            library_type: LibraryType::SkillsOnly,
            path: dir1.clone(),
            priority: 10,
        }).unwrap();
        assert_eq!(lib.list(), vec!["a"]);

        lib.add_source(LibrarySource {
            kind: SourceKind::Registered("extra".into()),
            library_type: LibraryType::SkillsOnly,
            path: dir2.clone(),
            priority: 50,
        }).unwrap();

        let names = lib.list();
        assert!(names.contains(&"a"));
        assert!(names.contains(&"b"));

        let _ = fs::remove_dir_all(&dir1);
        let _ = fs::remove_dir_all(&dir2);
    }

    #[test]
    fn reload_picks_up_disk_changes() {
        let dir = make_temp_dir("reload");
        fs::write(dir.join("original.md"), simple_skill("original")).unwrap();

        let mut lib = Library::empty();
        lib.add_source(LibrarySource {
            kind: SourceKind::User,
            library_type: LibraryType::SkillsOnly,
            path: dir.clone(),
            priority: 10,
        }).unwrap();
        assert_eq!(lib.list(), vec!["original"]);

        // Add a new file on disk
        fs::write(dir.join("added.md"), simple_skill("added")).unwrap();

        lib.reload().unwrap();
        let names = lib.list();
        assert!(names.contains(&"original"));
        assert!(names.contains(&"added"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_source_path_skipped_gracefully() {
        let config = LibraryConfig {
            project_dir: None,
            extra_sources: vec![ExtraSource {
                path: PathBuf::from("/does/not/exist/cmx_test"),
                library_type: LibraryType::SkillsOnly,
                priority: 99,
                name: "missing".into(),
            }],
            overrides: HashMap::new(),
        };

        let lib = Library::new(&config).unwrap();
        // Should not error, just have no skills from that source
        // (may have skills from default sources if those folders exist)
        assert!(lib.get("anything").is_none());
    }

    #[test]
    fn get_parsed_lazy() {
        let dir = make_temp_dir("lazy_parse");
        fs::write(
            dir.join("my-skill.md"),
            simple_skill("my-skill"),
        ).unwrap();

        let mut lib = Library::empty();
        lib.add_source(LibrarySource {
            kind: SourceKind::User,
            library_type: LibraryType::SkillsOnly,
            path: dir.clone(),
            priority: 10,
        }).unwrap();

        // get() returns entry without parsing
        let entry = lib.get("my-skill").unwrap();
        assert_eq!(entry.name, "my-skill");

        // get_parsed() actually reads and parses the file
        let doc = lib.get_parsed("my-skill").unwrap();
        assert_eq!(doc.frontmatter.name.as_deref(), Some("my-skill"));

        let _ = fs::remove_dir_all(&dir);
    }
}
