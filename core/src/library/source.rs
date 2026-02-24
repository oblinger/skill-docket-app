use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Source classification
// ---------------------------------------------------------------------------

/// What kind of library source this is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceKind {
    /// Built-in default library (assets/default-library/).
    System,
    /// Standard Anthropic skill locations (~/.claude/skills/, .claude/skills/).
    AnthropicDefault,
    /// User library (~/.config/cmx/cmx-library/).
    User,
    /// Project-specific library (<project>/.cmx/library/).
    Project(String),
    /// User-registered additional folder.
    Registered(String),
}

impl fmt::Display for SourceKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SourceKind::System => write!(f, "system"),
            SourceKind::AnthropicDefault => write!(f, "anthropic-default"),
            SourceKind::User => write!(f, "user"),
            SourceKind::Project(name) => write!(f, "project:{}", name),
            SourceKind::Registered(name) => write!(f, "registered:{}", name),
        }
    }
}

// ---------------------------------------------------------------------------
// Library type — how to scan the folder
// ---------------------------------------------------------------------------

/// Determines how skills are discovered within a source folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LibraryType {
    /// Contains only SKILL.md files — Anthropic-compatible.
    /// Scan recursively for *.md files; each file is a skill.
    SkillsOnly,
    /// Contains skills/ + automations/ + configs/.
    /// Only scan the skills/ subfolder for *.md files.
    Full,
    /// Anthropic standard layout: subdirectories each containing a SKILL.md.
    /// Skill name is the directory name.
    AnthropicStandard,
}

// ---------------------------------------------------------------------------
// LibrarySource
// ---------------------------------------------------------------------------

/// A single source contributing skills to the library.
#[derive(Debug, Clone)]
pub struct LibrarySource {
    pub kind: SourceKind,
    pub library_type: LibraryType,
    pub path: PathBuf,
    pub priority: u32,
}

/// A discovered skill file (not yet parsed).
#[derive(Debug, Clone)]
pub struct SkillEntry {
    /// Derived from filename without extension (or directory name for Anthropic style).
    pub name: String,
    /// Full path to the skill markdown file.
    pub path: PathBuf,
    /// Which source contributed this entry.
    pub source: SourceKind,
    /// Priority inherited from the source.
    pub priority: u32,
}

// ---------------------------------------------------------------------------
// Folder scanning
// ---------------------------------------------------------------------------

impl LibrarySource {
    /// Scan this source for skill files and return discovered entries.
    /// Returns an empty vec if the path doesn't exist.
    pub fn scan(&self) -> Vec<SkillEntry> {
        if !self.path.exists() {
            return Vec::new();
        }

        match self.library_type {
            LibraryType::SkillsOnly => self.scan_skills_only(&self.path),
            LibraryType::Full => {
                let skills_dir = self.path.join("skills");
                if skills_dir.is_dir() {
                    self.scan_skills_only(&skills_dir)
                } else {
                    Vec::new()
                }
            }
            LibraryType::AnthropicStandard => self.scan_anthropic_standard(),
        }
    }

    /// Recursively scan a directory for *.md files.
    /// Skill name = filename without extension.
    fn scan_skills_only(&self, dir: &Path) -> Vec<SkillEntry> {
        let mut entries = Vec::new();
        self.scan_dir_recursive(dir, &mut entries);
        entries
    }

    fn scan_dir_recursive(&self, dir: &Path, entries: &mut Vec<SkillEntry>) {
        let read_dir = match fs::read_dir(dir) {
            Ok(rd) => rd,
            Err(_) => return,
        };
        for entry in read_dir.flatten() {
            let path = entry.path();
            if path.is_dir() {
                self.scan_dir_recursive(&path, entries);
            } else if path.extension().and_then(|e| e.to_str()) == Some("md") {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    entries.push(SkillEntry {
                        name: stem.to_string(),
                        path,
                        source: self.kind.clone(),
                        priority: self.priority,
                    });
                }
            }
        }
    }

    /// Scan for Anthropic-style skill folders: each subdirectory containing
    /// a SKILL.md file is a skill. The skill name is the directory name.
    fn scan_anthropic_standard(&self) -> Vec<SkillEntry> {
        let mut entries = Vec::new();
        let read_dir = match fs::read_dir(&self.path) {
            Ok(rd) => rd,
            Err(_) => return entries,
        };
        for entry in read_dir.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let skill_file = path.join("SKILL.md");
                if skill_file.is_file() {
                    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                        entries.push(SkillEntry {
                            name: name.to_string(),
                            path: skill_file,
                            source: self.kind.clone(),
                            priority: self.priority,
                        });
                    }
                }
            }
        }
        entries
    }
}

// ---------------------------------------------------------------------------
// Default source construction helpers
// ---------------------------------------------------------------------------

/// Build the default sources that are always present.
/// Non-existent paths are included — they will produce empty scans.
pub fn default_sources(project_dir: Option<&Path>) -> Vec<LibrarySource> {
    let mut sources = Vec::new();

    // 1. System library — assets/default-library/ (priority 0)
    //    In practice this would resolve relative to the binary or a known install path.
    //    For now we use a compile-time conventional path.
    let system_path = PathBuf::from("assets/default-library");
    sources.push(LibrarySource {
        kind: SourceKind::System,
        library_type: LibraryType::Full,
        path: system_path,
        priority: 0,
    });

    // 2. Anthropic standard locations (priority 10)
    if let Some(home) = home_dir() {
        let claude_skills = home.join(".claude").join("skills");
        sources.push(LibrarySource {
            kind: SourceKind::AnthropicDefault,
            library_type: LibraryType::AnthropicStandard,
            path: claude_skills,
            priority: 10,
        });
    }

    if let Some(proj) = project_dir {
        let project_skills = proj.join(".claude").join("skills");
        sources.push(LibrarySource {
            kind: SourceKind::AnthropicDefault,
            library_type: LibraryType::AnthropicStandard,
            path: project_skills,
            priority: 10,
        });
    }

    // 3. User library (priority 20)
    if let Some(home) = home_dir() {
        let user_lib = home.join(".config").join("cmx").join("cmx-library");
        sources.push(LibrarySource {
            kind: SourceKind::User,
            library_type: LibraryType::Full,
            path: user_lib,
            priority: 20,
        });
    }

    sources
}

fn home_dir() -> Option<PathBuf> {
    std::env::var("HOME").ok().map(PathBuf::from)
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
            "cmx_source_{}_{}", std::process::id(), suffix
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn scan_skills_only_finds_md_files() {
        let tmp = make_temp_dir("skills_only");
        fs::create_dir_all(&tmp).unwrap();
        fs::write(tmp.join("code-review.md"), "# Code Review").unwrap();
        fs::write(tmp.join("testing.md"), "# Testing").unwrap();
        fs::write(tmp.join("readme.txt"), "not a skill").unwrap();

        let source = LibrarySource {
            kind: SourceKind::Registered("test".into()),
            library_type: LibraryType::SkillsOnly,
            path: tmp.clone(),
            priority: 50,
        };

        let entries = source.scan();
        assert_eq!(entries.len(), 2);

        let mut names: Vec<_> = entries.iter().map(|e| e.name.as_str()).collect();
        names.sort();
        assert_eq!(names, vec!["code-review", "testing"]);

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn scan_skills_only_recursive() {
        let tmp = make_temp_dir("skills_recursive");
        let sub = tmp.join("sub");
        fs::create_dir_all(&sub).unwrap();
        fs::write(tmp.join("top.md"), "# Top").unwrap();
        fs::write(sub.join("nested.md"), "# Nested").unwrap();

        let source = LibrarySource {
            kind: SourceKind::User,
            library_type: LibraryType::SkillsOnly,
            path: tmp.clone(),
            priority: 20,
        };

        let entries = source.scan();
        assert_eq!(entries.len(), 2);

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn scan_full_library_only_scans_skills_subfolder() {
        let tmp = make_temp_dir("full_lib");
        let skills = tmp.join("skills");
        let automations = tmp.join("automations");
        fs::create_dir_all(&skills).unwrap();
        fs::create_dir_all(&automations).unwrap();
        fs::write(skills.join("deploy.md"), "# Deploy").unwrap();
        fs::write(automations.join("ci.md"), "# CI").unwrap();

        let source = LibrarySource {
            kind: SourceKind::User,
            library_type: LibraryType::Full,
            path: tmp.clone(),
            priority: 20,
        };

        let entries = source.scan();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "deploy");

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn scan_anthropic_standard() {
        let tmp = make_temp_dir("anthropic");
        let skill_a = tmp.join("code-review");
        let skill_b = tmp.join("testing");
        let not_skill = tmp.join("random-dir");
        fs::create_dir_all(&skill_a).unwrap();
        fs::create_dir_all(&skill_b).unwrap();
        fs::create_dir_all(&not_skill).unwrap();
        fs::write(skill_a.join("SKILL.md"), "# Code Review").unwrap();
        fs::write(skill_b.join("SKILL.md"), "# Testing").unwrap();
        // not_skill has no SKILL.md — should be ignored

        let source = LibrarySource {
            kind: SourceKind::AnthropicDefault,
            library_type: LibraryType::AnthropicStandard,
            path: tmp.clone(),
            priority: 10,
        };

        let entries = source.scan();
        assert_eq!(entries.len(), 2);
        let mut names: Vec<_> = entries.iter().map(|e| e.name.as_str()).collect();
        names.sort();
        assert_eq!(names, vec!["code-review", "testing"]);

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn scan_missing_path_returns_empty() {
        let source = LibrarySource {
            kind: SourceKind::System,
            library_type: LibraryType::Full,
            path: PathBuf::from("/nonexistent/path/cmx_test"),
            priority: 0,
        };
        assert!(source.scan().is_empty());
    }

    #[test]
    fn non_skill_files_ignored_in_full_library() {
        let tmp = make_temp_dir("full_nonskill");
        let skills = tmp.join("skills");
        fs::create_dir_all(&skills).unwrap();
        fs::write(skills.join("deploy.md"), "# Deploy").unwrap();
        fs::write(skills.join("helper.py"), "print('hi')").unwrap();
        fs::write(skills.join("config.yaml"), "key: val").unwrap();

        let source = LibrarySource {
            kind: SourceKind::User,
            library_type: LibraryType::Full,
            path: tmp.clone(),
            priority: 20,
        };

        let entries = source.scan();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "deploy");

        let _ = fs::remove_dir_all(&tmp);
    }
}
