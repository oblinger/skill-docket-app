//! Parser for `.skilldocket` project configuration files.
//!
//! A `.skilldocket` file at a project root defines agents, skills,
//! and the roadmap path — enabling one-command project setup via `project.add`.

use serde::{Deserialize, Serialize};
use std::path::Path;


/// Top-level project configuration from a `.skilldocket` file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectConfig {
    /// Project display name.
    pub project: String,

    /// Path to the Roadmap.md file (relative to project root).
    #[serde(default)]
    pub roadmap: Option<String>,

    /// Path to the skills directory (relative to project root). Defaults to "skills".
    #[serde(default = "default_skills_dir")]
    pub skills_dir: String,

    /// Agent definitions to create on project setup.
    #[serde(default)]
    pub agents: Vec<AgentDef>,

    /// If true, immediately spawn agents into tmux after creation.
    #[serde(default)]
    pub auto_start: bool,
}


/// Agent definition within a project config.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentDef {
    /// Agent name.
    pub name: String,

    /// Agent role (e.g., "pm", "worker", "checker").
    pub role: String,

    /// Skill name to assign. Resolved from the project's skills directory.
    #[serde(default)]
    pub skill: Option<String>,
}


fn default_skills_dir() -> String {
    "skills".into()
}


/// Load a `.skilldocket` config from a YAML file.
pub fn load(path: &Path) -> Result<ProjectConfig, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read '{}': {}", path.display(), e))?;
    parse(&content)
}


/// Parse a `.skilldocket` config from a YAML string.
pub fn parse(content: &str) -> Result<ProjectConfig, String> {
    serde_yaml::from_str(content)
        .map_err(|e| format!("invalid .skilldocket config: {}", e))
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_full_config() {
        let yaml = r#"
project: "MyProject"
roadmap: "docs/Roadmap.md"
skills_dir: "skills"
auto_start: true
agents:
  - name: pm1
    role: pm
    skill: agent-pm
  - name: w1
    role: worker
    skill: builder
  - name: w2
    role: worker
"#;
        let cfg = parse(yaml).unwrap();
        assert_eq!(cfg.project, "MyProject");
        assert_eq!(cfg.roadmap.as_deref(), Some("docs/Roadmap.md"));
        assert_eq!(cfg.skills_dir, "skills");
        assert!(cfg.auto_start);
        assert_eq!(cfg.agents.len(), 3);
        assert_eq!(cfg.agents[0].name, "pm1");
        assert_eq!(cfg.agents[0].role, "pm");
        assert_eq!(cfg.agents[0].skill.as_deref(), Some("agent-pm"));
        assert_eq!(cfg.agents[2].skill, None);
    }

    #[test]
    fn parse_minimal_config() {
        let yaml = "project: Minimal\n";
        let cfg = parse(yaml).unwrap();
        assert_eq!(cfg.project, "Minimal");
        assert_eq!(cfg.roadmap, None);
        assert_eq!(cfg.skills_dir, "skills");
        assert!(!cfg.auto_start);
        assert!(cfg.agents.is_empty());
    }

    #[test]
    fn parse_missing_project_fails() {
        let yaml = "agents:\n  - name: w1\n    role: worker\n";
        let result = parse(yaml);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("invalid"));
    }

    #[test]
    fn parse_agent_missing_role_fails() {
        let yaml = "project: Test\nagents:\n  - name: w1\n";
        let result = parse(yaml);
        assert!(result.is_err());
    }

    #[test]
    fn parse_agent_missing_name_fails() {
        let yaml = "project: Test\nagents:\n  - role: worker\n";
        let result = parse(yaml);
        assert!(result.is_err());
    }

    #[test]
    fn parse_empty_agents_list() {
        let yaml = "project: Test\nagents: []\n";
        let cfg = parse(yaml).unwrap();
        assert!(cfg.agents.is_empty());
    }

    #[test]
    fn parse_default_skills_dir() {
        let yaml = "project: Test\n";
        let cfg = parse(yaml).unwrap();
        assert_eq!(cfg.skills_dir, "skills");
    }

    #[test]
    fn parse_custom_skills_dir() {
        let yaml = "project: Test\nskills_dir: my-skills\n";
        let cfg = parse(yaml).unwrap();
        assert_eq!(cfg.skills_dir, "my-skills");
    }

    #[test]
    fn load_from_file() {
        let dir = std::env::temp_dir().join("cmx_test_project_config");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join(".skilldocket");
        std::fs::write(&path, "project: FileTest\nroadmap: Roadmap.md\n").unwrap();

        let cfg = load(&path).unwrap();
        assert_eq!(cfg.project, "FileTest");
        assert_eq!(cfg.roadmap.as_deref(), Some("Roadmap.md"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_missing_file_errors() {
        let result = load(Path::new("/tmp/nonexistent_skilldocket_xyz"));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("cannot read"));
    }
}
