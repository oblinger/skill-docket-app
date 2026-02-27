//! Briefing composition — builds the document injected into an agent's session
//! when a task is assigned.

/// Compose a briefing document from skill instructions, task spec, project context,
/// and an optional learnings file path.
///
/// Sections with no content are omitted entirely.
pub fn compose_briefing(
    skill_instructions: Option<&str>,
    task_spec: Option<&str>,
    project_context: Option<&str>,
) -> String {
    compose_briefing_with_learnings(skill_instructions, task_spec, project_context, None)
}

/// Compose a briefing document, optionally including the path to the project's
/// LEARNINGS.md file so agents know where to find and append learnings.
pub fn compose_briefing_with_learnings(
    skill_instructions: Option<&str>,
    task_spec: Option<&str>,
    project_context: Option<&str>,
    learnings_path: Option<&str>,
) -> String {
    let mut parts = Vec::new();

    if let Some(skill) = skill_instructions {
        if !skill.trim().is_empty() {
            parts.push(format!("# Skill Instructions\n\n{}", skill.trim()));
        }
    }

    if let Some(spec) = task_spec {
        if !spec.trim().is_empty() {
            parts.push(format!("# Task Specification\n\n{}", spec.trim()));
        }
    }

    if let Some(ctx) = project_context {
        if !ctx.trim().is_empty() {
            parts.push(format!("# Project Context\n\n{}", ctx.trim()));
        }
    }

    if let Some(lp) = learnings_path {
        if !lp.trim().is_empty() {
            parts.push(format!(
                "# Learnings\n\nRead project learnings before starting: {}\nAppend new discoveries to this file as you work.",
                lp.trim()
            ));
        }
    }

    if parts.is_empty() {
        return String::new();
    }

    parts.join("\n\n")
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn briefing_all_sections() {
        let result = compose_briefing(
            Some("Do the thing."),
            Some("Build module X."),
            Some("Project: Hollow World\nPath: /tmp/hw"),
        );
        assert!(result.contains("# Skill Instructions"));
        assert!(result.contains("Do the thing."));
        assert!(result.contains("# Task Specification"));
        assert!(result.contains("Build module X."));
        assert!(result.contains("# Project Context"));
        assert!(result.contains("Hollow World"));
    }

    #[test]
    fn briefing_skill_only() {
        let result = compose_briefing(Some("Instructions here."), None, None);
        assert!(result.contains("# Skill Instructions"));
        assert!(!result.contains("# Task Specification"));
        assert!(!result.contains("# Project Context"));
    }

    #[test]
    fn briefing_task_only() {
        let result = compose_briefing(None, Some("Build it."), None);
        assert!(!result.contains("# Skill Instructions"));
        assert!(result.contains("# Task Specification"));
    }

    #[test]
    fn briefing_empty_produces_empty() {
        let result = compose_briefing(None, None, None);
        assert!(result.is_empty());
    }

    #[test]
    fn briefing_whitespace_only_skipped() {
        let result = compose_briefing(Some("  \n  "), Some("  "), None);
        assert!(result.is_empty());
    }

    #[test]
    fn briefing_with_learnings_path() {
        let result = compose_briefing_with_learnings(
            Some("Do the thing."),
            None,
            None,
            Some("/projects/hw/LEARNINGS.md"),
        );
        assert!(result.contains("# Skill Instructions"));
        assert!(result.contains("# Learnings"));
        assert!(result.contains("/projects/hw/LEARNINGS.md"));
    }

    #[test]
    fn briefing_with_learnings_path_none() {
        let result = compose_briefing_with_learnings(
            Some("Do the thing."),
            None,
            None,
            None,
        );
        assert!(result.contains("# Skill Instructions"));
        assert!(!result.contains("# Learnings"));
    }
}
