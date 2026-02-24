//! Status display that formats agent/task/project data for terminal display.
//!
//! The [`StatusView`] struct controls which sections are rendered and at what
//! width. It consumes core types ([`Agent`], [`TaskNode`], [`FolderEntry`])
//! and produces formatted strings using the [`crate::render`] module.

use skill_docket_core::types::agent::{Agent, AgentStatus, HealthState};
use skill_docket_core::types::config::FolderEntry;
use skill_docket_core::types::task::{TaskNode, TaskStatus};

use crate::render::{
    self, Alignment, Table, TableColumn,
    BOLD, CYAN, DIM, GREEN, RED, RESET, WHITE, YELLOW,
};


/// Controls which sections of the status display are rendered and how.
pub struct StatusView {
    /// Terminal width in characters.
    pub width: usize,
    /// Whether to show the header bar.
    pub show_header: bool,
    /// Whether to show the agents section.
    pub show_agents: bool,
    /// Whether to show the tasks section.
    pub show_tasks: bool,
    /// Whether to show the projects section.
    pub show_projects: bool,
    /// Whether to use compact (single-line-per-item) rendering.
    pub compact: bool,
}


impl StatusView {
    /// Create a new status view with the given width, showing agents only.
    pub fn new(width: usize) -> Self {
        StatusView {
            width,
            show_header: true,
            show_agents: true,
            show_tasks: false,
            show_projects: false,
            compact: false,
        }
    }

    /// Create a full status view showing all sections.
    pub fn full(width: usize) -> Self {
        StatusView {
            width,
            show_header: true,
            show_agents: true,
            show_tasks: true,
            show_projects: true,
            compact: false,
        }
    }

    /// Create a compact status view (all sections, compact mode).
    pub fn compact(width: usize) -> Self {
        StatusView {
            width,
            show_header: true,
            show_agents: true,
            show_tasks: true,
            show_projects: true,
            compact: true,
        }
    }

    /// Render the header bar showing summary counts.
    pub fn render_header(
        &self,
        agent_count: usize,
        task_count: usize,
        project_count: usize,
        msg_count: usize,
    ) -> String {
        if !self.show_header {
            return String::new();
        }
        let summary = system_summary_line(agent_count, task_count, project_count, msg_count);
        let separator: String = std::iter::repeat(render::BOX_H).take(self.width).collect();
        format!("{}\n{}\n", summary, separator)
    }

    /// Render the agents section as a table.
    pub fn render_agents(&self, agents: &[Agent]) -> String {
        if !self.show_agents || agents.is_empty() {
            return String::new();
        }

        if self.compact {
            return self.render_agents_compact(agents);
        }

        let name_w = 14;
        let role_w = 8;
        let status_w = 8;
        let health_w = 10;
        let task_w = 10;
        let notes_w = self
            .width
            .saturating_sub(name_w + role_w + status_w + health_w + task_w + 18); // padding + borders

        let cols = vec![
            TableColumn { header: "Agent".into(), width: name_w, align: Alignment::Left },
            TableColumn { header: "Role".into(), width: role_w, align: Alignment::Left },
            TableColumn { header: "Status".into(), width: status_w, align: Alignment::Left },
            TableColumn { header: "Health".into(), width: health_w, align: Alignment::Left },
            TableColumn { header: "Task".into(), width: task_w, align: Alignment::Left },
            TableColumn { header: "Notes".into(), width: notes_w.max(10), align: Alignment::Left },
        ];

        let mut table = Table::new(cols);
        for agent in agents {
            let status_str = format!(
                "{}{}{}",
                agent_status_color(&agent.status),
                format_agent_status(&agent.status),
                RESET
            );
            let health_str = format!(
                "{}{}{}",
                health_color(&agent.health),
                format_health(&agent.health),
                RESET
            );
            let task_str = agent.task.as_deref().unwrap_or("-").to_string();
            table.add_row(vec![
                agent.name.clone(),
                agent.role.clone(),
                status_str,
                health_str,
                task_str,
                agent.status_notes.clone(),
            ]);
        }

        let mut out = format!("{}  Agents ({}){}\n", BOLD, agents.len(), RESET);
        out.push_str(&table.render_with_color());
        out
    }

    fn render_agents_compact(&self, agents: &[Agent]) -> String {
        let mut out = format!("{}  Agents ({}){}\n", BOLD, agents.len(), RESET);
        for agent in agents {
            let indicator = render::status_indicator(&format_agent_status(&agent.status));
            let task = agent.task.as_deref().unwrap_or("-");
            out.push_str(&format!(
                " {} {} [{}] {} {}{}{}\n",
                indicator,
                render::pad_right(&agent.name, 12),
                agent.role,
                task,
                DIM,
                render::truncate(&agent.status_notes, 30),
                RESET,
            ));
        }
        out
    }

    /// Render the tasks section as a tree.
    ///
    /// Each tuple is `(task_node, indent_depth)`.
    pub fn render_tasks(&self, tasks: &[(&TaskNode, usize)]) -> String {
        if !self.show_tasks || tasks.is_empty() {
            return String::new();
        }

        if self.compact {
            return self.render_tasks_compact(tasks);
        }

        let id_w = 10;
        let title_w = 25;
        let status_w = 12;
        let agent_w = 12;
        let source_w = 10;

        let cols = vec![
            TableColumn { header: "ID".into(), width: id_w, align: Alignment::Left },
            TableColumn { header: "Title".into(), width: title_w, align: Alignment::Left },
            TableColumn { header: "Status".into(), width: status_w, align: Alignment::Left },
            TableColumn { header: "Agent".into(), width: agent_w, align: Alignment::Left },
            TableColumn { header: "Source".into(), width: source_w, align: Alignment::Left },
        ];

        let mut table = Table::new(cols);
        for (task, depth) in tasks {
            let indent: String = std::iter::repeat("  ").take(*depth).collect();
            let id_display = format!("{}{}", indent, task.id);
            let status_str = format!(
                "{}{}{}",
                task_status_color(&task.status),
                format_task_status(&task.status),
                RESET
            );
            let agent_str = task.agent.as_deref().unwrap_or("-").to_string();
            let source_str = format!("{:?}", task.source);
            table.add_row(vec![
                id_display,
                task.title.clone(),
                status_str,
                agent_str,
                source_str,
            ]);
        }

        let task_count = tasks.len();
        let mut out = format!("{}  Tasks ({}){}\n", BOLD, task_count, RESET);
        out.push_str(&table.render_with_color());
        out
    }

    fn render_tasks_compact(&self, tasks: &[(&TaskNode, usize)]) -> String {
        let mut out = format!("{}  Tasks ({}){}\n", BOLD, tasks.len(), RESET);
        for (task, depth) in tasks {
            let indent: String = std::iter::repeat("  ").take(*depth).collect();
            let indicator = render::status_indicator(&format_task_status(&task.status));
            let agent = task.agent.as_deref().unwrap_or("");
            let agent_part = if agent.is_empty() {
                String::new()
            } else {
                format!(" [{}]", agent)
            };
            out.push_str(&format!(
                " {}{} {} {}{}\n",
                indent,
                indicator,
                render::pad_right(&task.id, 8),
                render::truncate(&task.title, 30),
                agent_part,
            ));
        }
        out
    }

    /// Render the projects section as a table.
    pub fn render_projects(&self, projects: &[FolderEntry]) -> String {
        if !self.show_projects || projects.is_empty() {
            return String::new();
        }

        if self.compact {
            return self.render_projects_compact(projects);
        }

        let name_w = 15;
        let path_w = self.width.saturating_sub(name_w + 8).max(20);

        let cols = vec![
            TableColumn { header: "Project".into(), width: name_w, align: Alignment::Left },
            TableColumn { header: "Path".into(), width: path_w, align: Alignment::Left },
        ];

        let mut table = Table::new(cols);
        for project in projects {
            table.add_row(vec![project.name.clone(), project.path.clone()]);
        }

        let mut out = format!("{}  Projects ({}){}\n", BOLD, projects.len(), RESET);
        out.push_str(&table.render_with_color());
        out
    }

    fn render_projects_compact(&self, projects: &[FolderEntry]) -> String {
        let mut out = format!("{}  Projects ({}){}\n", BOLD, projects.len(), RESET);
        for project in projects {
            out.push_str(&format!(
                "  {} {}{}{}\n",
                render::pad_right(&project.name, 12),
                DIM,
                render::truncate(&project.path, 50),
                RESET,
            ));
        }
        out
    }

    /// Render all enabled sections.
    pub fn render_all(
        &self,
        agents: &[Agent],
        tasks: &[(&TaskNode, usize)],
        projects: &[FolderEntry],
        msg_count: usize,
    ) -> String {
        let mut out = String::new();

        out.push_str(&self.render_header(agents.len(), tasks.len(), projects.len(), msg_count));

        if self.show_agents {
            let agent_section = self.render_agents(agents);
            if !agent_section.is_empty() {
                out.push_str(&agent_section);
                out.push('\n');
            }
        }

        if self.show_tasks {
            let task_section = self.render_tasks(tasks);
            if !task_section.is_empty() {
                out.push_str(&task_section);
                out.push('\n');
            }
        }

        if self.show_projects {
            let project_section = self.render_projects(projects);
            if !project_section.is_empty() {
                out.push_str(&project_section);
                out.push('\n');
            }
        }

        out
    }
}


// ---------------------------------------------------------------------------
// Color helpers
// ---------------------------------------------------------------------------

/// Return the ANSI color code for an agent status.
fn agent_status_color(status: &AgentStatus) -> &'static str {
    match status {
        AgentStatus::Idle => WHITE,
        AgentStatus::Busy => GREEN,
        AgentStatus::Stalled => YELLOW,
        AgentStatus::Error => RED,
        AgentStatus::Dead => RED,
    }
}

/// Return the ANSI color code for a health state.
fn health_color(health: &HealthState) -> &'static str {
    match health {
        HealthState::Healthy => GREEN,
        HealthState::Degraded => YELLOW,
        HealthState::Unhealthy => RED,
        HealthState::Unknown => DIM,
    }
}

/// Return the ANSI color code for a task status.
fn task_status_color(status: &TaskStatus) -> &'static str {
    match status {
        TaskStatus::Pending => YELLOW,
        TaskStatus::InProgress => CYAN,
        TaskStatus::Completed => GREEN,
        TaskStatus::Failed => RED,
        TaskStatus::Paused => YELLOW,
        TaskStatus::Cancelled => DIM,
    }
}

/// Format an agent status enum as a display string.
fn format_agent_status(status: &AgentStatus) -> String {
    match status {
        AgentStatus::Idle => "idle".to_string(),
        AgentStatus::Busy => "busy".to_string(),
        AgentStatus::Stalled => "stalled".to_string(),
        AgentStatus::Error => "error".to_string(),
        AgentStatus::Dead => "dead".to_string(),
    }
}

/// Format a health state enum as a display string.
fn format_health(health: &HealthState) -> String {
    match health {
        HealthState::Healthy => "healthy".to_string(),
        HealthState::Degraded => "degraded".to_string(),
        HealthState::Unhealthy => "unhealthy".to_string(),
        HealthState::Unknown => "unknown".to_string(),
    }
}

/// Format a task status enum as a display string.
fn format_task_status(status: &TaskStatus) -> String {
    match status {
        TaskStatus::Pending => "pending".to_string(),
        TaskStatus::InProgress => "in_progress".to_string(),
        TaskStatus::Completed => "completed".to_string(),
        TaskStatus::Failed => "failed".to_string(),
        TaskStatus::Paused => "paused".to_string(),
        TaskStatus::Cancelled => "cancelled".to_string(),
    }
}


/// Produce a single summary line showing counts.
pub fn system_summary_line(
    agents: usize,
    tasks: usize,
    projects: usize,
    messages: usize,
) -> String {
    format!(
        "{}{} CMX {} agents:{} {} tasks:{} {} projects:{} {} msgs:{} {}{}",
        BOLD,
        CYAN,
        RESET,
        BOLD,
        agents,
        RESET,
        tasks,
        RESET,
        projects,
        RESET,
        messages,
        RESET,
    )
}


// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::Panel;
    use skill_docket_core::types::agent::AgentType;
    use skill_docket_core::types::task::TaskSource;

    fn sample_agents() -> Vec<Agent> {
        vec![
            Agent {
                name: "pilot".into(),
                role: "pilot".into(),
                agent_type: AgentType::Claude,
                task: Some("CMX1".into()),
                path: "/projects/cmx".into(),
                status: AgentStatus::Busy,
                status_notes: "coordinating workers".into(),
                health: HealthState::Healthy,
                last_heartbeat_ms: Some(1700000000000),
                session: Some("cmx-main".into()),
            },
            Agent {
                name: "worker-1".into(),
                role: "worker".into(),
                agent_type: AgentType::Claude,
                task: Some("CMX1A".into()),
                path: "/projects/cmx".into(),
                status: AgentStatus::Busy,
                status_notes: "running tests".into(),
                health: HealthState::Healthy,
                last_heartbeat_ms: Some(1700000000000),
                session: Some("cmx-main".into()),
            },
            Agent {
                name: "worker-2".into(),
                role: "worker".into(),
                agent_type: AgentType::Ssh,
                task: None,
                path: "/projects/cmx".into(),
                status: AgentStatus::Idle,
                status_notes: String::new(),
                health: HealthState::Unknown,
                last_heartbeat_ms: None,
                session: None,
            },
            Agent {
                name: "worker-3".into(),
                role: "worker".into(),
                agent_type: AgentType::Claude,
                task: Some("CMX2".into()),
                path: "/projects/cmx".into(),
                status: AgentStatus::Error,
                status_notes: "compile failed".into(),
                health: HealthState::Unhealthy,
                last_heartbeat_ms: Some(1699999990000),
                session: Some("cmx-main".into()),
            },
        ]
    }

    fn sample_tasks() -> Vec<TaskNode> {
        vec![
            TaskNode {
                id: "CMX1".into(),
                title: "Core daemon".into(),
                source: TaskSource::Roadmap,
                status: TaskStatus::InProgress,
                result: None,
                agent: Some("pilot".into()),
                children: vec![
                    TaskNode {
                        id: "CMX1A".into(),
                        title: "Socket protocol".into(),
                        source: TaskSource::Filesystem,
                        status: TaskStatus::InProgress,
                        result: None,
                        agent: Some("worker-1".into()),
                        children: vec![],
                        spec_path: Some("/tasks/CMX1A/CMX1A.md".into()),
                    },
                    TaskNode {
                        id: "CMX1B".into(),
                        title: "Data layer".into(),
                        source: TaskSource::Filesystem,
                        status: TaskStatus::Pending,
                        result: None,
                        agent: None,
                        children: vec![],
                        spec_path: Some("/tasks/CMX1B/CMX1B.md".into()),
                    },
                ],
                spec_path: Some("/tasks/CMX1/CMX1.md".into()),
            },
            TaskNode {
                id: "CMX2".into(),
                title: "CLI interface".into(),
                source: TaskSource::Roadmap,
                status: TaskStatus::Failed,
                result: Some("compile error".into()),
                agent: Some("worker-3".into()),
                children: vec![],
                spec_path: None,
            },
        ]
    }

    fn sample_projects() -> Vec<FolderEntry> {
        vec![
            FolderEntry {
                name: "cmx-core".into(),
                path: "/projects/cmx/core".into(),
            },
            FolderEntry {
                name: "cmx-cli".into(),
                path: "/projects/cmx/cli".into(),
            },
        ]
    }

    fn flatten_tasks(tasks: &[TaskNode]) -> Vec<(&TaskNode, usize)> {
        let mut result = Vec::new();
        fn walk<'a>(task: &'a TaskNode, depth: usize, out: &mut Vec<(&'a TaskNode, usize)>) {
            out.push((task, depth));
            for child in &task.children {
                walk(child, depth + 1, out);
            }
        }
        for task in tasks {
            walk(task, 0, &mut result);
        }
        result
    }

    // --- StatusView construction ---

    #[test]
    fn status_view_new_defaults() {
        let v = StatusView::new(80);
        assert_eq!(v.width, 80);
        assert!(v.show_header);
        assert!(v.show_agents);
        assert!(!v.show_tasks);
        assert!(!v.show_projects);
        assert!(!v.compact);
    }

    #[test]
    fn status_view_full() {
        let v = StatusView::full(120);
        assert_eq!(v.width, 120);
        assert!(v.show_header);
        assert!(v.show_agents);
        assert!(v.show_tasks);
        assert!(v.show_projects);
        assert!(!v.compact);
    }

    #[test]
    fn status_view_compact() {
        let v = StatusView::compact(100);
        assert!(v.compact);
        assert!(v.show_agents);
        assert!(v.show_tasks);
        assert!(v.show_projects);
    }

    // --- render_header ---

    #[test]
    fn render_header_contains_counts() {
        let v = StatusView::new(80);
        let header = v.render_header(3, 5, 2, 10);
        assert!(header.contains("3"));
        assert!(header.contains("5"));
        assert!(header.contains("2"));
        assert!(header.contains("10"));
        assert!(header.contains("CMX"));
    }

    #[test]
    fn render_header_disabled() {
        let mut v = StatusView::new(80);
        v.show_header = false;
        let header = v.render_header(3, 5, 2, 10);
        assert!(header.is_empty());
    }

    // --- render_agents ---

    #[test]
    fn render_agents_table() {
        let v = StatusView::new(100);
        let agents = sample_agents();
        let output = v.render_agents(&agents);

        assert!(output.contains("Agents (4)"));
        assert!(output.contains("pilot"));
        assert!(output.contains("worker-1"));
        assert!(output.contains("worker-2"));
        assert!(output.contains("worker-3"));
    }

    #[test]
    fn render_agents_compact() {
        let v = StatusView::compact(100);
        let agents = sample_agents();
        let output = v.render_agents(&agents);

        assert!(output.contains("Agents (4)"));
        assert!(output.contains("pilot"));
        assert!(output.contains("worker-1"));
        // Compact mode should not have table borders
        assert!(!output.contains(&render::BOX_TL.to_string()));
    }

    #[test]
    fn render_agents_empty() {
        let v = StatusView::new(80);
        let output = v.render_agents(&[]);
        assert!(output.is_empty());
    }

    #[test]
    fn render_agents_disabled() {
        let mut v = StatusView::new(80);
        v.show_agents = false;
        let agents = sample_agents();
        let output = v.render_agents(&agents);
        assert!(output.is_empty());
    }

    #[test]
    fn render_agents_shows_task_assignment() {
        let v = StatusView::new(100);
        let agents = sample_agents();
        let output = v.render_agents(&agents);
        assert!(output.contains("CMX1"));
        assert!(output.contains("CMX1A"));
    }

    #[test]
    fn render_agents_shows_status_notes() {
        let v = StatusView::new(100);
        let agents = sample_agents();
        let output = v.render_agents(&agents);
        assert!(output.contains("coordinating workers"));
        assert!(output.contains("running tests"));
        assert!(output.contains("compile failed"));
    }

    // --- render_tasks ---

    #[test]
    fn render_tasks_table() {
        let v = StatusView::full(120);
        let tasks = sample_tasks();
        let flat = flatten_tasks(&tasks);
        let output = v.render_tasks(&flat);

        assert!(output.contains("Tasks (4)"));
        assert!(output.contains("CMX1"));
        assert!(output.contains("CMX1A"));
        assert!(output.contains("CMX1B"));
        assert!(output.contains("CMX2"));
        assert!(output.contains("Core daemon"));
        assert!(output.contains("Socket protocol"));
    }

    #[test]
    fn render_tasks_compact() {
        let v = StatusView::compact(100);
        let tasks = sample_tasks();
        let flat = flatten_tasks(&tasks);
        let output = v.render_tasks(&flat);

        assert!(output.contains("Tasks (4)"));
        assert!(output.contains("CMX1"));
        // No table borders in compact
        assert!(!output.contains(&render::BOX_TL.to_string()));
    }

    #[test]
    fn render_tasks_empty() {
        let v = StatusView::full(80);
        let output = v.render_tasks(&[]);
        assert!(output.is_empty());
    }

    #[test]
    fn render_tasks_disabled() {
        let mut v = StatusView::full(80);
        v.show_tasks = false;
        let tasks = sample_tasks();
        let flat = flatten_tasks(&tasks);
        let output = v.render_tasks(&flat);
        assert!(output.is_empty());
    }

    // --- render_projects ---

    #[test]
    fn render_projects_table() {
        let v = StatusView::full(100);
        let projects = sample_projects();
        let output = v.render_projects(&projects);

        assert!(output.contains("Projects (2)"));
        assert!(output.contains("cmx-core"));
        assert!(output.contains("cmx-cli"));
        assert!(output.contains("/projects/cmx/core"));
    }

    #[test]
    fn render_projects_compact() {
        let v = StatusView::compact(100);
        let projects = sample_projects();
        let output = v.render_projects(&projects);

        assert!(output.contains("Projects (2)"));
        assert!(output.contains("cmx-core"));
    }

    #[test]
    fn render_projects_empty() {
        let v = StatusView::full(80);
        let output = v.render_projects(&[]);
        assert!(output.is_empty());
    }

    #[test]
    fn render_projects_disabled() {
        let mut v = StatusView::full(80);
        v.show_projects = false;
        let projects = sample_projects();
        let output = v.render_projects(&projects);
        assert!(output.is_empty());
    }

    // --- render_all ---

    #[test]
    fn render_all_full() {
        let v = StatusView::full(120);
        let agents = sample_agents();
        let tasks = sample_tasks();
        let flat = flatten_tasks(&tasks);
        let projects = sample_projects();

        let output = v.render_all(&agents, &flat, &projects, 7);

        // Should contain all sections
        assert!(output.contains("CMX"));
        assert!(output.contains("Agents"));
        assert!(output.contains("Tasks"));
        assert!(output.contains("Projects"));
    }

    #[test]
    fn render_all_compact() {
        let v = StatusView::compact(100);
        let agents = sample_agents();
        let tasks = sample_tasks();
        let flat = flatten_tasks(&tasks);
        let projects = sample_projects();

        let output = v.render_all(&agents, &flat, &projects, 3);

        assert!(output.contains("Agents"));
        assert!(output.contains("Tasks"));
        assert!(output.contains("Projects"));
    }

    #[test]
    fn render_all_empty_data() {
        let v = StatusView::full(80);
        let output = v.render_all(&[], &[], &[], 0);

        // Should still have header
        assert!(output.contains("CMX"));
        // No section titles for empty data
        assert!(!output.contains("Agents"));
        assert!(!output.contains("Tasks"));
        assert!(!output.contains("Projects"));
    }

    // --- Color helpers ---

    #[test]
    fn agent_status_color_mapping() {
        assert_eq!(agent_status_color(&AgentStatus::Idle), WHITE);
        assert_eq!(agent_status_color(&AgentStatus::Busy), GREEN);
        assert_eq!(agent_status_color(&AgentStatus::Stalled), YELLOW);
        assert_eq!(agent_status_color(&AgentStatus::Error), RED);
        assert_eq!(agent_status_color(&AgentStatus::Dead), RED);
    }

    #[test]
    fn health_color_mapping() {
        assert_eq!(health_color(&HealthState::Healthy), GREEN);
        assert_eq!(health_color(&HealthState::Degraded), YELLOW);
        assert_eq!(health_color(&HealthState::Unhealthy), RED);
        assert_eq!(health_color(&HealthState::Unknown), DIM);
    }

    #[test]
    fn task_status_color_mapping() {
        assert_eq!(task_status_color(&TaskStatus::Pending), YELLOW);
        assert_eq!(task_status_color(&TaskStatus::InProgress), CYAN);
        assert_eq!(task_status_color(&TaskStatus::Completed), GREEN);
        assert_eq!(task_status_color(&TaskStatus::Failed), RED);
        assert_eq!(task_status_color(&TaskStatus::Paused), YELLOW);
        assert_eq!(task_status_color(&TaskStatus::Cancelled), DIM);
    }

    // --- Format helpers ---

    #[test]
    fn format_agent_status_strings() {
        assert_eq!(format_agent_status(&AgentStatus::Idle), "idle");
        assert_eq!(format_agent_status(&AgentStatus::Busy), "busy");
        assert_eq!(format_agent_status(&AgentStatus::Stalled), "stalled");
        assert_eq!(format_agent_status(&AgentStatus::Error), "error");
        assert_eq!(format_agent_status(&AgentStatus::Dead), "dead");
    }

    #[test]
    fn format_health_strings() {
        assert_eq!(format_health(&HealthState::Healthy), "healthy");
        assert_eq!(format_health(&HealthState::Degraded), "degraded");
        assert_eq!(format_health(&HealthState::Unhealthy), "unhealthy");
        assert_eq!(format_health(&HealthState::Unknown), "unknown");
    }

    #[test]
    fn format_task_status_strings() {
        assert_eq!(format_task_status(&TaskStatus::Pending), "pending");
        assert_eq!(format_task_status(&TaskStatus::InProgress), "in_progress");
        assert_eq!(format_task_status(&TaskStatus::Completed), "completed");
        assert_eq!(format_task_status(&TaskStatus::Failed), "failed");
        assert_eq!(format_task_status(&TaskStatus::Paused), "paused");
        assert_eq!(format_task_status(&TaskStatus::Cancelled), "cancelled");
    }

    // --- system_summary_line ---

    #[test]
    fn system_summary_line_content() {
        let line = system_summary_line(3, 7, 2, 15);
        assert!(line.contains("CMX"));
        assert!(line.contains("3"));
        assert!(line.contains("7"));
        assert!(line.contains("2"));
        assert!(line.contains("15"));
    }

    #[test]
    fn system_summary_line_zeros() {
        let line = system_summary_line(0, 0, 0, 0);
        assert!(line.contains("0"));
    }

    // --- Panel integration ---

    #[test]
    fn agent_detail_panel() {
        let agent = &sample_agents()[0];
        let mut panel = Panel::new(&agent.name, 50);
        panel.add_kv("Role", &agent.role);
        panel.add_kv("Status", &format_agent_status(&agent.status));
        panel.add_kv("Health", &format_health(&agent.health));
        panel.add_kv("Task", agent.task.as_deref().unwrap_or("-"));
        panel.add_kv("Notes", &agent.status_notes);
        let output = panel.render();

        assert!(output.contains("pilot"));
        assert!(output.contains("Role: pilot"));
        assert!(output.contains("Status: busy"));
        assert!(output.contains("Health: healthy"));
        assert!(output.contains("Task: CMX1"));
    }

    // --- Flatten tasks helper ---

    #[test]
    fn flatten_tasks_depth() {
        let tasks = sample_tasks();
        let flat = flatten_tasks(&tasks);

        // CMX1 (0), CMX1A (1), CMX1B (1), CMX2 (0)
        assert_eq!(flat.len(), 4);
        assert_eq!(flat[0].0.id, "CMX1");
        assert_eq!(flat[0].1, 0);
        assert_eq!(flat[1].0.id, "CMX1A");
        assert_eq!(flat[1].1, 1);
        assert_eq!(flat[2].0.id, "CMX1B");
        assert_eq!(flat[2].1, 1);
        assert_eq!(flat[3].0.id, "CMX2");
        assert_eq!(flat[3].1, 0);
    }

    // --- Narrow width handling ---

    #[test]
    fn render_agents_narrow_width() {
        let v = StatusView::new(40);
        let agents = sample_agents();
        let output = v.render_agents(&agents);
        // Should render without panicking
        assert!(output.contains("Agents"));
    }

    #[test]
    fn render_tasks_narrow_width() {
        let v = StatusView::full(40);
        let tasks = sample_tasks();
        let flat = flatten_tasks(&tasks);
        let output = v.render_tasks(&flat);
        assert!(output.contains("Tasks"));
    }

    #[test]
    fn render_projects_narrow_width() {
        let v = StatusView::full(40);
        let projects = sample_projects();
        let output = v.render_projects(&projects);
        assert!(output.contains("Projects"));
    }
}
