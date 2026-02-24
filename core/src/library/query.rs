use std::fs;

use crate::skill::parse::parse_skill;
use crate::skill::types::{SkillDocument, SkillKind};

use super::errors::LibraryError;
use super::registry::Registry;
use super::source::SkillEntry;

// ---------------------------------------------------------------------------
// Query operations on the registry
// ---------------------------------------------------------------------------

/// Get a skill entry by name from the resolved registry.
pub fn get<'a>(registry: &'a Registry, name: &str) -> Option<&'a SkillEntry> {
    registry.get(name)
}

/// Get and parse a skill by name. Reads the file from disk and parses on demand.
pub fn get_parsed(registry: &Registry, name: &str) -> Result<SkillDocument, LibraryError> {
    let entry = registry.get(name).ok_or_else(|| {
        LibraryError::SkillNotFound(name.to_string())
    })?;

    let content = fs::read_to_string(&entry.path).map_err(LibraryError::IoError)?;
    parse_skill(&content).map_err(|e| LibraryError::ParseError {
        skill: name.to_string(),
        error: e,
    })
}

/// List all resolved skill names (sorted).
pub fn list(registry: &Registry) -> Vec<&str> {
    registry.list_names()
}

/// List skills filtered by kind. Parses every resolved skill to classify.
/// Use sparingly.
pub fn list_by_kind(
    registry: &Registry,
    kind: SkillKind,
) -> Result<Vec<(String, SkillDocument)>, LibraryError> {
    let mut results = Vec::new();

    for name in registry.list_names() {
        let doc = get_parsed(registry, name)?;
        if doc.kind() == kind {
            results.push((name.to_string(), doc));
        }
    }

    Ok(results)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::source::{LibrarySource, LibraryType, SourceKind};
    use std::path::PathBuf;

    fn make_temp_dir(suffix: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "cmx_query_{}_{}", std::process::id(), suffix
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn simple_skill_content() -> &'static str {
        "---\nname: test-skill\ndescription: A test skill\n---\n\nDo the thing.\n"
    }

    fn structured_skill_content() -> &'static str {
        r#"---
name: structured-skill
description: A structured skill
---

| Fields | Type | Description |
|--------|------|-------------|
| input | string | The input |

Do the structured thing.
"#
    }

    fn orchestration_skill_content() -> &'static str {
        r#"---
name: orchestration-skill
description: An orchestration skill
---

| Nodes | Role | Description |
|-------|------|-------------|
| planner | pm | Plans the work |

| Edges | To | Condition |
|-------|-----|-----------|
| START | planner | always |

Orchestrate the thing.
"#
    }

    #[test]
    fn get_parsed_returns_document() {
        let dir = make_temp_dir("get_parsed");
        fs::write(dir.join("test-skill.md"), simple_skill_content()).unwrap();

        let mut reg = Registry::new();
        reg.add_source(LibrarySource {
            kind: SourceKind::User,
            library_type: LibraryType::SkillsOnly,
            path: dir.clone(),
            priority: 10,
        });
        reg.resolve();

        let doc = get_parsed(&reg, "test-skill").unwrap();
        assert_eq!(doc.frontmatter.name.as_deref(), Some("test-skill"));
        assert_eq!(doc.kind(), SkillKind::Simple);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn get_parsed_skill_not_found() {
        let reg = Registry::new();
        let result = get_parsed(&reg, "nonexistent");
        assert!(matches!(result, Err(LibraryError::SkillNotFound(_))));
    }

    #[test]
    fn list_by_kind_filters_correctly() {
        let dir = make_temp_dir("by_kind");
        fs::write(dir.join("simple.md"), simple_skill_content()).unwrap();
        fs::write(dir.join("structured.md"), structured_skill_content()).unwrap();
        fs::write(dir.join("orch.md"), orchestration_skill_content()).unwrap();

        let mut reg = Registry::new();
        reg.add_source(LibrarySource {
            kind: SourceKind::User,
            library_type: LibraryType::SkillsOnly,
            path: dir.clone(),
            priority: 10,
        });
        reg.resolve();

        let simple = list_by_kind(&reg, SkillKind::Simple).unwrap();
        assert_eq!(simple.len(), 1);
        assert_eq!(simple[0].0, "simple");

        let structured = list_by_kind(&reg, SkillKind::Structured).unwrap();
        assert_eq!(structured.len(), 1);
        assert_eq!(structured[0].0, "structured");

        let orch = list_by_kind(&reg, SkillKind::Orchestration).unwrap();
        assert_eq!(orch.len(), 1);
        assert_eq!(orch[0].0, "orch");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn list_returns_sorted_names() {
        let dir = make_temp_dir("list_sorted");
        fs::write(dir.join("zebra.md"), simple_skill_content()).unwrap();
        fs::write(dir.join("alpha.md"), simple_skill_content()).unwrap();

        let mut reg = Registry::new();
        reg.add_source(LibrarySource {
            kind: SourceKind::User,
            library_type: LibraryType::SkillsOnly,
            path: dir.clone(),
            priority: 10,
        });
        reg.resolve();

        assert_eq!(list(&reg), vec!["alpha", "zebra"]);

        let _ = fs::remove_dir_all(&dir);
    }
}
