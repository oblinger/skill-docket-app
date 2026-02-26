//! Install / bootstrap module for CMX.
//!
//! When the daemon starts and finds no valid settings, `ensure_installed`
//! creates the directory structure, writes default settings with a version
//! number, and seeds the skill library. Existing files are never overwritten.

use std::path::Path;

use crate::data::settings;

/// Result of an installation check.
#[derive(Debug, PartialEq)]
pub enum InstallStatus {
    /// Settings exist with current version — no action needed.
    Current,
    /// Fresh install was performed.
    Installed,
    /// Existing settings were upgraded from an older version.
    Upgraded { from_version: String },
}

/// The current settings version. Bump this when the settings format changes.
pub const SETTINGS_VERSION: &str = "0.1.0";

/// Check if CMX is properly installed, and install if not.
/// This is called automatically by Data::new().
///
/// - If settings.yaml exists with current version: returns Current, does nothing.
/// - If settings.yaml exists with old version: upgrades (preserving user values), returns Upgraded.
/// - If settings.yaml doesn't exist: performs fresh install, returns Installed.
///
/// Never overwrites existing files silently.
pub fn ensure_installed(config_dir: &Path) -> Result<InstallStatus, String> {
    let settings_path = config_dir.join("settings.yaml");

    if settings_path.exists() {
        // Settings file exists — check version
        let existing_version = read_settings_version(config_dir)?;
        match existing_version.as_deref() {
            Some(v) if v == SETTINGS_VERSION => {
                // Current version — ensure directories exist but don't touch files
                create_directories(config_dir)?;
                write_default_skills(config_dir)?;
                Ok(InstallStatus::Current)
            }
            Some(v) => {
                // Old version — upgrade: re-parse with defaults, stamp new version, rewrite
                let from = v.to_string();
                upgrade_settings(config_dir)?;
                create_directories(config_dir)?;
                write_default_skills(config_dir)?;
                Ok(InstallStatus::Upgraded { from_version: from })
            }
            None => {
                // No version field — pre-versioned settings, treat as upgrade from "0.0.0"
                let from = "0.0.0".to_string();
                upgrade_settings(config_dir)?;
                create_directories(config_dir)?;
                write_default_skills(config_dir)?;
                Ok(InstallStatus::Upgraded { from_version: from })
            }
        }
    } else {
        // No settings file — fresh install
        fresh_install(config_dir)?;
        Ok(InstallStatus::Installed)
    }
}

/// Perform a fresh installation.
fn fresh_install(config_dir: &Path) -> Result<(), String> {
    create_directories(config_dir)?;
    write_default_settings(config_dir)?;
    write_default_skills(config_dir)?;
    Ok(())
}

/// Create the standard directory structure.
fn create_directories(config_dir: &Path) -> Result<(), String> {
    let dirs = [
        config_dir.to_path_buf(),
        config_dir.join("agents"),
        config_dir.join("history"),
        config_dir.join("logs"),
        config_dir.join("skills"),
        config_dir.join("skills").join("agent-pm"),
    ];
    for dir in &dirs {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("Failed to create directory {}: {}", dir.display(), e))?;
    }
    Ok(())
}

/// Write default settings.yaml with version field.
fn write_default_settings(config_dir: &Path) -> Result<(), String> {
    let settings_path = config_dir.join("settings.yaml");
    let s = settings::default_settings();
    settings::save(&settings_path, &s)
}

/// Upgrade an existing settings file: reload it (unknown keys preserved via
/// the parse-from-defaults approach), stamp the current version, and rewrite.
fn upgrade_settings(config_dir: &Path) -> Result<(), String> {
    let settings_path = config_dir.join("settings.yaml");
    let mut s = settings::load(&settings_path)?;
    s.version = SETTINGS_VERSION.to_string();
    settings::save(&settings_path, &s)
}

/// Write the default agent-pm skill document.
fn write_default_skills(config_dir: &Path) -> Result<(), String> {
    let pm_dir = config_dir.join("skills").join("agent-pm");
    std::fs::create_dir_all(&pm_dir)
        .map_err(|e| format!("Failed to create agent-pm skill dir: {}", e))?;

    let skill_content = r#"---
name: agent-pm
description: Default CMX project manager orchestrator. Interprets status signals, decides retry vs. redesign vs. escalate, and dispatches commands to CMX.
---

# PM Orchestrator

You are the project manager for a CMX-orchestrated workspace. Your role is to:

1. Monitor agent status signals from CMX
2. Decide on interventions: retry, restart, redesign, or escalate
3. Dispatch work to available workers via `cmx tell`
4. Track task progress and update status via `cmx task.set`

## Decision Framework

- **Infrastructure failure** (SSH down, tmux crashed): retry automatically
- **Agent failure** (stuck, error loop): restart the agent, possibly reassign task
- **Strategic failure** (wrong approach, repeated failures): redesign the approach or escalate to user

## Available Commands

Use `skd help` for the full command reference. Key commands:
- `cmx status` — system overview
- `cmx agent.list` — all agents and their status
- `cmx task.list` — all tasks and progress
- `cmx tell <agent> <message>` — send instructions to an agent
- `cmx agent.assign <agent> <task>` — assign a task
- `cmx diagnosis report` — operational statistics
"#;

    let skill_path = pm_dir.join("SKILL.md");
    if !skill_path.exists() {
        std::fs::write(&skill_path, skill_content)
            .map_err(|e| format!("Failed to write agent-pm skill: {}", e))?;
    }
    Ok(())
}

/// Read the version from an existing settings.yaml.
fn read_settings_version(config_dir: &Path) -> Result<Option<String>, String> {
    let settings_path = config_dir.join("settings.yaml");
    let content = std::fs::read_to_string(&settings_path)
        .map_err(|e| format!("cannot read {}: {}", settings_path.display(), e))?;

    // Look for a "version:" line in the YAML content
    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("version:") {
            let val = rest.trim();
            if val.is_empty() {
                return Ok(None);
            }
            // Strip surrounding quotes if present
            let unquoted = if (val.starts_with('"') && val.ends_with('"'))
                || (val.starts_with('\'') && val.ends_with('\''))
            {
                &val[1..val.len() - 1]
            } else {
                val
            };
            return Ok(Some(unquoted.to_string()));
        }
    }
    Ok(None)
}


#[cfg(test)]
mod tests {
    use super::*;

    fn test_dir(suffix: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "cmx-install-test-{}-{}",
            suffix,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        // Clean up any leftover from previous runs
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn fresh_install_creates_structure() {
        let dir = test_dir("structure");
        ensure_installed(&dir).unwrap();

        assert!(dir.join("agents").is_dir());
        assert!(dir.join("history").is_dir());
        assert!(dir.join("logs").is_dir());
        assert!(dir.join("skills").is_dir());
        assert!(dir.join("skills").join("agent-pm").is_dir());
        assert!(dir.join("settings.yaml").is_file());
        assert!(dir.join("skills").join("agent-pm").join("SKILL.md").is_file());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn fresh_install_returns_installed() {
        let dir = test_dir("returns-installed");
        let status = ensure_installed(&dir).unwrap();
        assert_eq!(status, InstallStatus::Installed);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn current_version_returns_current() {
        let dir = test_dir("returns-current");
        // First install
        ensure_installed(&dir).unwrap();
        // Second call — should detect current
        let status = ensure_installed(&dir).unwrap();
        assert_eq!(status, InstallStatus::Current);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_version_triggers_upgrade() {
        let dir = test_dir("upgrade-missing-version");
        std::fs::create_dir_all(&dir).unwrap();
        // Write settings without a version field
        std::fs::write(
            dir.join("settings.yaml"),
            "health_check_interval: 8000\nmax_retries: 5\n",
        )
        .unwrap();

        let status = ensure_installed(&dir).unwrap();
        assert_eq!(
            status,
            InstallStatus::Upgraded {
                from_version: "0.0.0".into()
            }
        );

        // Verify settings now has a version and preserved the custom values
        let s = settings::load(&dir.join("settings.yaml")).unwrap();
        assert_eq!(s.version, SETTINGS_VERSION);
        assert_eq!(s.health_check_interval, 8000);
        assert_eq!(s.max_retries, 5);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn does_not_overwrite_existing_skills() {
        let dir = test_dir("preserve-skills");
        // First install to create structure
        ensure_installed(&dir).unwrap();

        // Write custom content to the skill file
        let skill_path = dir.join("skills").join("agent-pm").join("SKILL.md");
        std::fs::write(&skill_path, "# My Custom PM Skill\nCustomized content.").unwrap();

        // Run install again
        ensure_installed(&dir).unwrap();

        // Verify custom content is preserved
        let content = std::fs::read_to_string(&skill_path).unwrap();
        assert!(content.contains("My Custom PM Skill"));
        assert!(content.contains("Customized content."));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn does_not_overwrite_existing_settings() {
        let dir = test_dir("preserve-settings");
        // First install
        ensure_installed(&dir).unwrap();

        // Modify settings — change a value but keep current version
        let settings_path = dir.join("settings.yaml");
        let mut s = settings::load(&settings_path).unwrap();
        s.max_retries = 99;
        settings::save(&settings_path, &s).unwrap();

        // Run install again — should return Current and not touch settings
        let status = ensure_installed(&dir).unwrap();
        assert_eq!(status, InstallStatus::Current);

        // Verify custom value preserved
        let reloaded = settings::load(&settings_path).unwrap();
        assert_eq!(reloaded.max_retries, 99);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn creates_agent_pm_skill() {
        let dir = test_dir("agent-pm-skill");
        ensure_installed(&dir).unwrap();

        let skill_path = dir.join("skills").join("agent-pm").join("SKILL.md");
        assert!(skill_path.exists());

        let content = std::fs::read_to_string(&skill_path).unwrap();
        assert!(content.contains("PM Orchestrator"));
        assert!(content.contains("agent-pm"));
        assert!(content.contains("Decision Framework"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn settings_has_version_after_install() {
        let dir = test_dir("version-present");
        ensure_installed(&dir).unwrap();

        let s = settings::load(&dir.join("settings.yaml")).unwrap();
        assert_eq!(s.version, SETTINGS_VERSION);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
