//! View rendering for each application state.
//!
//! Each view struct takes domain data and a width/height and produces a
//! `Vec<String>` of lines suitable for display. Views do not own data or
//! perform I/O — they are pure rendering functions.

use skill_docket_core::types::agent::{Agent, AgentStatus, HealthState};
use skill_docket_core::types::config::FolderEntry;
use skill_docket_core::types::message::Message;
use skill_docket_core::types::task::{TaskNode, TaskStatus};

use crate::render::{
    self, Alignment, Panel, Table, TableColumn,
    BOLD, CYAN, DIM, GREEN, RED, RESET, WHITE, YELLOW,
};
use crate::theme::Theme;


// ---------------------------------------------------------------------------
// DashboardView
// ---------------------------------------------------------------------------

/// Renders the main dashboard: summary header + agent table + task tree +
/// project list.
pub struct DashboardView;

impl DashboardView {
    /// Render the full dashboard.
    ///
    /// `agents` — all registered agents.
    /// `tasks` — flattened task tree as `(task, depth)` pairs.
    /// `projects` — registered project folders.
    /// `theme` — color theme.
    /// `width` — terminal width.
    /// `height` — terminal height.
    pub fn render(
        agents: &[Agent],
        tasks: &[(&TaskNode, usize)],
        projects: &[FolderEntry],
        _theme: &Theme,
        width: usize,
        _height: usize,
    ) -> Vec<String> {
        let mut lines = Vec::new();

        // --- Header ---
        let separator: String = std::iter::repeat(render::BOX_H).take(width).collect();
        lines.push(format!(
            "{}{}CMX Dashboard{} | agents: {} | tasks: {} | projects: {}",
            BOLD, CYAN, RESET, agents.len(), tasks.len(), projects.len()
        ));
        lines.push(separator);
        lines.push(String::new());

        // --- Agents section ---
        if !agents.is_empty() {
            lines.push(format!("{}  Agents ({}){}", BOLD, agents.len(), RESET));
            let name_w = 14;
            let role_w = 8;
            let status_w = 10;
            let health_w = 10;
            let task_w = 10;
            let cols = vec![
                TableColumn { header: "Name".into(), width: name_w, align: Alignment::Left },
                TableColumn { header: "Role".into(), width: role_w, align: Alignment::Left },
                TableColumn { header: "Status".into(), width: status_w, align: Alignment::Left },
                TableColumn { header: "Health".into(), width: health_w, align: Alignment::Left },
                TableColumn { header: "Task".into(), width: task_w, align: Alignment::Left },
            ];
            let mut table = Table::new(cols);
            for agent in agents {
                let status_str = format_agent_status(&agent.status);
                let health_str = format_health(&agent.health);
                let task_str = agent.task.as_deref().unwrap_or("-").to_string();
                table.add_row(vec![
                    agent.name.clone(),
                    agent.role.clone(),
                    status_str,
                    health_str,
                    task_str,
                ]);
            }
            for line in table.render().lines() {
                lines.push(line.to_string());
            }
            lines.push(String::new());
        }

        // --- Tasks section ---
        if !tasks.is_empty() {
            lines.push(format!("{}  Tasks ({}){}", BOLD, tasks.len(), RESET));
            for (task, depth) in tasks {
                let indent: String = std::iter::repeat("  ").take(*depth).collect();
                let status_str = format_task_status(&task.status);
                let agent_part = match &task.agent {
                    Some(a) => format!(" [{}]", a),
                    None => String::new(),
                };
                lines.push(format!(
                    "  {}{} {} {}{}",
                    indent,
                    render::status_indicator(&status_str),
                    render::pad_right(&task.id, 10),
                    render::truncate(&task.title, 30),
                    agent_part,
                ));
            }
            lines.push(String::new());
        }

        // --- Projects section ---
        if !projects.is_empty() {
            lines.push(format!("{}  Projects ({}){}", BOLD, projects.len(), RESET));
            for project in projects {
                lines.push(format!(
                    "  {} {}{}{}",
                    render::pad_right(&project.name, 14),
                    DIM,
                    render::truncate(&project.path, 50),
                    RESET,
                ));
            }
            lines.push(String::new());
        }

        // --- Empty state ---
        if agents.is_empty() && tasks.is_empty() && projects.is_empty() {
            lines.push("  No agents, tasks, or projects registered.".into());
            lines.push("  Use ':' to enter a command.".into());
        }

        lines
    }
}


// ---------------------------------------------------------------------------
// AgentDetailView
// ---------------------------------------------------------------------------

/// Renders a detailed view of a single agent.
pub struct AgentDetailView;

impl AgentDetailView {
    /// Render agent detail card with optional message history and timeline.
    pub fn render(
        agent: &Agent,
        messages: &[Message],
        _timeline_events: &[(u64, String)],
        _theme: &Theme,
        width: usize,
    ) -> Vec<String> {
        let mut lines = Vec::new();

        // Agent info panel
        let mut panel = Panel::new(&agent.name, width.min(60));
        panel.add_kv("Role", &agent.role);
        panel.add_kv("Type", &format!("{:?}", agent.agent_type));
        panel.add_kv("Status", &format_agent_status(&agent.status));
        panel.add_kv("Health", &format_health(&agent.health));
        panel.add_kv("Task", agent.task.as_deref().unwrap_or("-"));
        panel.add_kv("Path", &agent.path);
        panel.add_kv("Notes", &agent.status_notes);
        if let Some(session) = &agent.session {
            panel.add_kv("Session", session);
        }
        if let Some(hb) = agent.last_heartbeat_ms {
            panel.add_kv("Last Heartbeat", &format!("{}ms", hb));
        }
        for line in panel.render().lines() {
            lines.push(line.to_string());
        }
        lines.push(String::new());

        // Messages
        if !messages.is_empty() {
            lines.push(format!("{}  Messages ({}){}", BOLD, messages.len(), RESET));
            for msg in messages.iter().rev().take(20) {
                let delivered = if msg.delivered_at_ms.is_some() {
                    format!("{} delivered{}", GREEN, RESET)
                } else {
                    format!("{} pending{}", YELLOW, RESET)
                };
                lines.push(format!(
                    "  {} -> {}: {} [{}]",
                    msg.sender,
                    msg.recipient,
                    render::truncate(&msg.text, 40),
                    delivered,
                ));
            }
        }

        lines
    }
}


// ---------------------------------------------------------------------------
// TaskDetailView
// ---------------------------------------------------------------------------

/// Renders a detailed view of a single task.
pub struct TaskDetailView;

impl TaskDetailView {
    /// Render task detail with children, assignment, and result.
    pub fn render(
        task: &TaskNode,
        agent_name: Option<&str>,
        _theme: &Theme,
        width: usize,
    ) -> Vec<String> {
        let mut lines = Vec::new();

        let mut panel = Panel::new(&task.id, width.min(60));
        panel.add_kv("Title", &task.title);
        panel.add_kv("Status", &format_task_status(&task.status));
        panel.add_kv("Source", &format!("{:?}", task.source));
        panel.add_kv("Agent", agent_name.unwrap_or("-"));
        if let Some(ref result) = task.result {
            panel.add_kv("Result", result);
        }
        if let Some(ref spec) = task.spec_path {
            panel.add_kv("Spec", spec);
        }
        for line in panel.render().lines() {
            lines.push(line.to_string());
        }

        // Children
        if !task.children.is_empty() {
            lines.push(String::new());
            lines.push(format!("{}  Children ({}){}", BOLD, task.children.len(), RESET));
            for child in &task.children {
                let indicator = render::status_indicator(&format_task_status(&child.status));
                let agent_part = match &child.agent {
                    Some(a) => format!(" [{}]", a),
                    None => String::new(),
                };
                lines.push(format!(
                    "  {} {} {}{}",
                    indicator,
                    render::pad_right(&child.id, 10),
                    child.title,
                    agent_part,
                ));
            }
        }

        lines
    }
}


// ---------------------------------------------------------------------------
// ConfigView
// ---------------------------------------------------------------------------

/// Renders a settings table with current values.
pub struct ConfigViewRenderer;

impl ConfigViewRenderer {
    /// Render a list of settings as key-value pairs.
    pub fn render(
        settings: &[(String, String)],
        _theme: &Theme,
        width: usize,
    ) -> Vec<String> {
        let mut lines = Vec::new();
        lines.push(format!("{}  Configuration{}", BOLD, RESET));
        lines.push(String::new());

        if settings.is_empty() {
            lines.push("  No configuration values set.".into());
            return lines;
        }

        let key_w = 25;
        let val_w = width.saturating_sub(key_w + 8).max(20);

        let cols = vec![
            TableColumn { header: "Key".into(), width: key_w, align: Alignment::Left },
            TableColumn { header: "Value".into(), width: val_w, align: Alignment::Left },
        ];
        let mut table = Table::new(cols);
        for (key, value) in settings {
            table.add_row(vec![key.clone(), value.clone()]);
        }
        for line in table.render().lines() {
            lines.push(line.to_string());
        }

        lines
    }
}


// ---------------------------------------------------------------------------
// LogView
// ---------------------------------------------------------------------------

/// Renders a scrollable log display.
pub struct LogViewRenderer;

impl LogViewRenderer {
    /// Render log entries with scroll offset.
    pub fn render(
        entries: &[(u64, String, String)], // (timestamp_ms, level, message)
        scroll_offset: usize,
        _theme: &Theme,
        width: usize,
        height: usize,
    ) -> Vec<String> {
        let mut lines = Vec::new();
        let total = entries.len();
        lines.push(format!(
            "{}  Log ({} entries){} [showing {}-{}]",
            BOLD,
            total,
            RESET,
            scroll_offset + 1,
            (scroll_offset + height).min(total),
        ));
        lines.push(String::new());

        if entries.is_empty() {
            lines.push("  No log entries.".into());
            return lines;
        }

        let visible = entries
            .iter()
            .skip(scroll_offset)
            .take(height.saturating_sub(3));

        for (ts, level, message) in visible {
            let color = match level.as_str() {
                "ERROR" => RED,
                "WARN" => YELLOW,
                "INFO" => CYAN,
                "DEBUG" => DIM,
                _ => WHITE,
            };
            lines.push(format!(
                "  {} {}{:<5}{} {}",
                ts,
                color,
                render::truncate(level, 5),
                RESET,
                render::truncate(message, width.saturating_sub(20)),
            ));
        }

        lines
    }
}


// ---------------------------------------------------------------------------
// HelpView
// ---------------------------------------------------------------------------

/// Renders formatted help text.
pub struct HelpViewRenderer;

impl HelpViewRenderer {
    /// Render help text, optionally word-wrapped to width.
    pub fn render(
        help_text: &str,
        _theme: &Theme,
        width: usize,
    ) -> Vec<String> {
        let mut lines = Vec::new();
        lines.push(format!("{}  Help{}", BOLD, RESET));
        lines.push(String::new());

        for line in help_text.lines() {
            if line.len() > width {
                // Simple word-wrap
                let mut current = String::new();
                for word in line.split_whitespace() {
                    if current.len() + word.len() + 1 > width {
                        lines.push(format!("  {}", current));
                        current = word.to_string();
                    } else {
                        if !current.is_empty() {
                            current.push(' ');
                        }
                        current.push_str(word);
                    }
                }
                if !current.is_empty() {
                    lines.push(format!("  {}", current));
                }
            } else {
                lines.push(format!("  {}", line));
            }
        }

        lines
    }
}


// ---------------------------------------------------------------------------
// ConfirmView
// ---------------------------------------------------------------------------

/// Renders a confirmation dialog.
pub struct ConfirmViewRenderer;

impl ConfirmViewRenderer {
    /// Render a confirmation prompt centered on screen.
    pub fn render(
        prompt: &str,
        _theme: &Theme,
        width: usize,
    ) -> Vec<String> {
        let mut lines = Vec::new();
        lines.push(String::new());

        let mut panel = Panel::new("Confirm", width.min(50));
        panel.add_line("");
        panel.add_line(&format!("  {}", prompt));
        panel.add_line("");
        panel.add_line("  [y] Yes   [n] No   [Enter] Confirm   [Esc] Cancel");
        panel.add_line("");

        for line in panel.render().lines() {
            lines.push(line.to_string());
        }

        lines
    }
}


// ---------------------------------------------------------------------------
// Format helpers
// ---------------------------------------------------------------------------

fn format_agent_status(status: &AgentStatus) -> String {
    match status {
        AgentStatus::Idle => "idle".to_string(),
        AgentStatus::Busy => "busy".to_string(),
        AgentStatus::Stalled => "stalled".to_string(),
        AgentStatus::Error => "error".to_string(),
        AgentStatus::Dead => "dead".to_string(),
    }
}

fn format_health(health: &HealthState) -> String {
    match health {
        HealthState::Healthy => "healthy".to_string(),
        HealthState::Degraded => "degraded".to_string(),
        HealthState::Unhealthy => "unhealthy".to_string(),
        HealthState::Unknown => "unknown".to_string(),
    }
}

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


// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use skill_docket_core::types::agent::AgentType;
    use skill_docket_core::types::task::TaskSource;

    fn make_agent(name: &str, role: &str, status: AgentStatus) -> Agent {
        Agent {
            name: name.into(),
            role: role.into(),
            agent_type: AgentType::Claude,
            task: Some("T1".into()),
            path: "/tmp".into(),
            status,
            status_notes: "notes".into(),
            health: HealthState::Healthy,
            last_heartbeat_ms: Some(1000),
            session: Some("main".into()),
        }
    }

    fn make_task(id: &str, title: &str, status: TaskStatus) -> TaskNode {
        TaskNode {
            id: id.into(),
            title: title.into(),
            source: TaskSource::Roadmap,
            status,
            result: None,
            agent: None,
            children: vec![],
            spec_path: None,
        }
    }

    fn make_project(name: &str, path: &str) -> FolderEntry {
        FolderEntry {
            name: name.into(),
            path: path.into(),
        }
    }

    fn default_theme() -> Theme {
        Theme::default_dark()
    }

    // --- DashboardView ---

    #[test]
    fn dashboard_empty_shows_message() {
        let lines = DashboardView::render(&[], &[], &[], &default_theme(), 80, 24);
        let text = lines.join("\n");
        assert!(text.contains("No agents"));
    }

    #[test]
    fn dashboard_shows_header() {
        let agents = vec![make_agent("w1", "worker", AgentStatus::Busy)];
        let lines = DashboardView::render(&agents, &[], &[], &default_theme(), 80, 24);
        let text = lines.join("\n");
        assert!(text.contains("CMX Dashboard"));
        assert!(text.contains("agents: 1"));
    }

    #[test]
    fn dashboard_shows_agents() {
        let agents = vec![
            make_agent("w1", "worker", AgentStatus::Busy),
            make_agent("p1", "pilot", AgentStatus::Idle),
        ];
        let lines = DashboardView::render(&agents, &[], &[], &default_theme(), 100, 24);
        let text = lines.join("\n");
        assert!(text.contains("Agents (2)"));
        assert!(text.contains("w1"));
        assert!(text.contains("p1"));
    }

    #[test]
    fn dashboard_shows_tasks() {
        let task = make_task("T1", "Core daemon", TaskStatus::InProgress);
        let tasks: Vec<(&TaskNode, usize)> = vec![(&task, 0)];
        let lines = DashboardView::render(&[], &tasks, &[], &default_theme(), 80, 24);
        let text = lines.join("\n");
        assert!(text.contains("Tasks (1)"));
        assert!(text.contains("T1"));
        assert!(text.contains("Core daemon"));
    }

    #[test]
    fn dashboard_shows_projects() {
        let projects = vec![make_project("cmx", "/projects/cmx")];
        let lines = DashboardView::render(&[], &[], &projects, &default_theme(), 80, 24);
        let text = lines.join("\n");
        assert!(text.contains("Projects (1)"));
        assert!(text.contains("cmx"));
    }

    #[test]
    fn dashboard_full_view() {
        let agents = vec![make_agent("w1", "worker", AgentStatus::Busy)];
        let task = make_task("T1", "Task 1", TaskStatus::Pending);
        let tasks: Vec<(&TaskNode, usize)> = vec![(&task, 0)];
        let projects = vec![make_project("proj", "/proj")];
        let lines = DashboardView::render(&agents, &tasks, &projects, &default_theme(), 100, 24);
        let text = lines.join("\n");
        assert!(text.contains("Agents"));
        assert!(text.contains("Tasks"));
        assert!(text.contains("Projects"));
    }

    #[test]
    fn dashboard_width_constraint() {
        let agents = vec![make_agent("w1", "worker", AgentStatus::Busy)];
        let lines = DashboardView::render(&agents, &[], &[], &default_theme(), 40, 24);
        // Should render without panic at narrow width
        assert!(!lines.is_empty());
    }

    // --- AgentDetailView ---

    #[test]
    fn agent_detail_shows_info() {
        let agent = make_agent("w1", "worker", AgentStatus::Busy);
        let lines = AgentDetailView::render(&agent, &[], &[], &default_theme(), 60);
        let text = lines.join("\n");
        assert!(text.contains("w1"));
        assert!(text.contains("worker"));
        assert!(text.contains("busy"));
    }

    #[test]
    fn agent_detail_shows_messages() {
        let agent = make_agent("w1", "worker", AgentStatus::Idle);
        let msgs = vec![Message {
            sender: "pm".into(),
            recipient: "w1".into(),
            text: "start task".into(),
            queued_at_ms: 1000,
            delivered_at_ms: None,
        }];
        let lines = AgentDetailView::render(&agent, &msgs, &[], &default_theme(), 60);
        let text = lines.join("\n");
        assert!(text.contains("Messages (1)"));
        assert!(text.contains("start task"));
    }

    #[test]
    fn agent_detail_no_messages() {
        let agent = make_agent("w1", "worker", AgentStatus::Idle);
        let lines = AgentDetailView::render(&agent, &[], &[], &default_theme(), 60);
        let text = lines.join("\n");
        assert!(!text.contains("Messages"));
    }

    #[test]
    fn agent_detail_shows_session() {
        let agent = make_agent("w1", "worker", AgentStatus::Idle);
        let lines = AgentDetailView::render(&agent, &[], &[], &default_theme(), 60);
        let text = lines.join("\n");
        assert!(text.contains("Session: main"));
    }

    #[test]
    fn agent_detail_shows_heartbeat() {
        let agent = make_agent("w1", "worker", AgentStatus::Idle);
        let lines = AgentDetailView::render(&agent, &[], &[], &default_theme(), 60);
        let text = lines.join("\n");
        assert!(text.contains("Last Heartbeat"));
    }

    // --- TaskDetailView ---

    #[test]
    fn task_detail_shows_info() {
        let task = make_task("T1", "Core daemon", TaskStatus::InProgress);
        let lines = TaskDetailView::render(&task, Some("w1"), &default_theme(), 60);
        let text = lines.join("\n");
        assert!(text.contains("T1"));
        assert!(text.contains("Core daemon"));
        assert!(text.contains("in_progress"));
        assert!(text.contains("w1"));
    }

    #[test]
    fn task_detail_with_children() {
        let mut task = make_task("T1", "Parent", TaskStatus::InProgress);
        task.children = vec![
            make_task("T1A", "Child A", TaskStatus::Pending),
            make_task("T1B", "Child B", TaskStatus::Completed),
        ];
        let lines = TaskDetailView::render(&task, None, &default_theme(), 60);
        let text = lines.join("\n");
        assert!(text.contains("Children (2)"));
        assert!(text.contains("T1A"));
        assert!(text.contains("T1B"));
    }

    #[test]
    fn task_detail_no_children() {
        let task = make_task("T1", "Leaf task", TaskStatus::Pending);
        let lines = TaskDetailView::render(&task, None, &default_theme(), 60);
        let text = lines.join("\n");
        assert!(!text.contains("Children"));
    }

    #[test]
    fn task_detail_with_result() {
        let mut task = make_task("T1", "Done", TaskStatus::Completed);
        task.result = Some("all tests pass".into());
        let lines = TaskDetailView::render(&task, None, &default_theme(), 60);
        let text = lines.join("\n");
        assert!(text.contains("all tests pass"));
    }

    #[test]
    fn task_detail_with_spec_path() {
        let mut task = make_task("T1", "Task", TaskStatus::Pending);
        task.spec_path = Some("/tasks/T1/T1.md".into());
        let lines = TaskDetailView::render(&task, None, &default_theme(), 60);
        let text = lines.join("\n");
        assert!(text.contains("/tasks/T1/T1.md"));
    }

    // --- ConfigView ---

    #[test]
    fn config_view_shows_settings() {
        let settings = vec![
            ("max_retries".into(), "3".into()),
            ("project_root".into(), "/tmp".into()),
        ];
        let lines = ConfigViewRenderer::render(&settings, &default_theme(), 80);
        let text = lines.join("\n");
        assert!(text.contains("Configuration"));
        assert!(text.contains("max_retries"));
        assert!(text.contains("/tmp"));
    }

    #[test]
    fn config_view_empty() {
        let lines = ConfigViewRenderer::render(&[], &default_theme(), 80);
        let text = lines.join("\n");
        assert!(text.contains("No configuration"));
    }

    // --- LogView ---

    #[test]
    fn log_view_shows_entries() {
        let entries = vec![
            (1000u64, "INFO".into(), "Started daemon".into()),
            (2000u64, "WARN".into(), "Agent stalled".into()),
        ];
        let lines = LogViewRenderer::render(&entries, 0, &default_theme(), 80, 24);
        let text = lines.join("\n");
        assert!(text.contains("Log (2 entries)"));
        assert!(text.contains("Started daemon"));
        assert!(text.contains("Agent stalled"));
    }

    #[test]
    fn log_view_empty() {
        let lines = LogViewRenderer::render(&[], 0, &default_theme(), 80, 24);
        let text = lines.join("\n");
        assert!(text.contains("No log entries"));
    }

    #[test]
    fn log_view_scroll_offset() {
        let entries: Vec<(u64, String, String)> = (0..100)
            .map(|i| (i as u64, "INFO".into(), format!("Entry {}", i)))
            .collect();
        let lines = LogViewRenderer::render(&entries, 50, &default_theme(), 80, 10);
        let text = lines.join("\n");
        assert!(text.contains("Entry 50"));
        assert!(!text.contains("Entry 49"));
    }

    // --- HelpView ---

    #[test]
    fn help_view_renders_text() {
        let lines = HelpViewRenderer::render("This is help text.\nLine two.", &default_theme(), 80);
        let text = lines.join("\n");
        assert!(text.contains("Help"));
        assert!(text.contains("This is help text."));
        assert!(text.contains("Line two."));
    }

    #[test]
    fn help_view_wraps_long_lines() {
        let long = "word ".repeat(50); // very long line
        let lines = HelpViewRenderer::render(&long, &default_theme(), 40);
        // Should have more lines than 1 due to wrapping
        assert!(lines.len() > 3);
    }

    // --- ConfirmView ---

    #[test]
    fn confirm_view_shows_prompt() {
        let lines = ConfirmViewRenderer::render("Are you sure?", &default_theme(), 60);
        let text = lines.join("\n");
        assert!(text.contains("Confirm"));
        assert!(text.contains("Are you sure?"));
        assert!(text.contains("[y] Yes"));
        assert!(text.contains("[n] No"));
    }

    #[test]
    fn confirm_view_narrow_width() {
        let lines = ConfirmViewRenderer::render("Delete it?", &default_theme(), 30);
        // Should render without panic
        assert!(!lines.is_empty());
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
}
