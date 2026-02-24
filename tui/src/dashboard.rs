//! Dashboard view — renders the agent table and summary using ratatui widgets.
//!
//! This module bridges the existing string-based renderers in [`crate::status`]
//! and [`crate::views`] with ratatui's widget system. It takes domain data
//! (agents, tasks) and renders them into a ratatui `Frame`.

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph, Row, Cell, Table};

use skill_docket_core::types::agent::{Agent, AgentStatus, HealthState};


/// Render the dashboard view: agent table + summary line.
pub fn render_dashboard(
    frame: &mut Frame,
    area: Rect,
    agents: &[Agent],
    selected_row: usize,
) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(5),    // agent table
            Constraint::Length(1), // summary
        ])
        .split(area);

    render_agent_table(frame, chunks[0], agents, selected_row);
    render_summary(frame, chunks[1], agents);
}


/// Render the agent table with a highlighted selection row.
fn render_agent_table(
    frame: &mut Frame,
    area: Rect,
    agents: &[Agent],
    selected: usize,
) {
    let header = Row::new(vec!["Time", "St", "Name", "Task", "Notes"])
        .style(Style::default().bold());

    let rows: Vec<Row> = agents
        .iter()
        .enumerate()
        .map(|(i, agent)| {
            let style = if i == selected {
                Style::default().bg(Color::DarkGray)
            } else {
                agent_style(agent)
            };
            Row::new(vec![
                Cell::from(format_age(agent.last_heartbeat_ms)),
                Cell::from(status_symbol(&agent.status)),
                Cell::from(agent.name.clone()),
                Cell::from(agent.task.clone().unwrap_or_default()),
                Cell::from(agent.status_notes.clone()),
            ])
            .style(style)
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Length(6),  // Time
            Constraint::Length(3),  // Status
            Constraint::Length(12), // Name
            Constraint::Length(15), // Task
            Constraint::Fill(1),   // Notes
        ],
    )
    .header(header)
    .block(Block::default().borders(Borders::ALL).title("Agents"));

    frame.render_widget(table, area);
}


/// Return a ratatui `Style` based on the agent's health state.
fn agent_style(agent: &Agent) -> Style {
    match agent.health {
        HealthState::Unhealthy => Style::default().fg(Color::Red),
        HealthState::Degraded => Style::default().fg(Color::Yellow),
        _ => Style::default(),
    }
}


/// Return a Unicode symbol representing the agent's status.
fn status_symbol(status: &AgentStatus) -> &'static str {
    match status {
        AgentStatus::Idle => "\u{25cf}",    // ●
        AgentStatus::Busy => "\u{25c9}",    // ◉
        AgentStatus::Stalled => "\u{25b2}", // ▲
        AgentStatus::Error => "\u{2716}",   // ✖
        AgentStatus::Dead => "\u{25cb}",    // ○
    }
}


/// Render a one-line summary of agent counts.
fn render_summary(frame: &mut Frame, area: Rect, agents: &[Agent]) {
    let healthy = agents
        .iter()
        .filter(|a| a.health == HealthState::Healthy)
        .count();
    let stalled = agents
        .iter()
        .filter(|a| a.status == AgentStatus::Stalled)
        .count();
    let text = format!(
        "\u{25cf} {} healthy  \u{25b2} {} stalled  {} total",
        healthy,
        stalled,
        agents.len()
    );
    frame.render_widget(Paragraph::new(text), area);
}


/// Format a heartbeat timestamp as a human-readable age string.
fn format_age(ms: Option<u64>) -> String {
    match ms {
        Some(ts) => {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            let age_secs = now.saturating_sub(ts) / 1000;
            if age_secs < 60 {
                format!("{}s", age_secs)
            } else if age_secs < 3600 {
                format!("{}m", age_secs / 60)
            } else {
                format!("{}h", age_secs / 3600)
            }
        }
        None => "--".into(),
    }
}


// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use skill_docket_core::types::agent::AgentType;

    fn make_agent(
        name: &str,
        status: AgentStatus,
        health: HealthState,
    ) -> Agent {
        Agent {
            name: name.into(),
            role: "worker".into(),
            agent_type: AgentType::Claude,
            task: Some("T1".into()),
            path: "/tmp".into(),
            status,
            status_notes: "ok".into(),
            health,
            last_heartbeat_ms: Some(1000),
            session: Some("main".into()),
        }
    }

    #[test]
    fn status_symbol_mapping() {
        assert_eq!(status_symbol(&AgentStatus::Idle), "\u{25cf}");
        assert_eq!(status_symbol(&AgentStatus::Busy), "\u{25c9}");
        assert_eq!(status_symbol(&AgentStatus::Stalled), "\u{25b2}");
        assert_eq!(status_symbol(&AgentStatus::Error), "\u{2716}");
        assert_eq!(status_symbol(&AgentStatus::Dead), "\u{25cb}");
    }

    #[test]
    fn format_age_none_returns_dashes() {
        assert_eq!(format_age(None), "--");
    }

    #[test]
    fn format_age_recent_returns_seconds() {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let result = format_age(Some(now_ms));
        assert_eq!(result, "0s");
    }

    #[test]
    fn format_age_minutes() {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        // 120 seconds ago
        let result = format_age(Some(now_ms - 120_000));
        assert_eq!(result, "2m");
    }

    #[test]
    fn format_age_hours() {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        // 2 hours ago
        let result = format_age(Some(now_ms - 7_200_000));
        assert_eq!(result, "2h");
    }

    #[test]
    fn agent_style_unhealthy_is_red() {
        let agent = make_agent("w1", AgentStatus::Idle, HealthState::Unhealthy);
        let style = agent_style(&agent);
        assert_eq!(style.fg, Some(Color::Red));
    }

    #[test]
    fn agent_style_degraded_is_yellow() {
        let agent = make_agent("w1", AgentStatus::Idle, HealthState::Degraded);
        let style = agent_style(&agent);
        assert_eq!(style.fg, Some(Color::Yellow));
    }

    #[test]
    fn agent_style_healthy_is_default() {
        let agent = make_agent("w1", AgentStatus::Idle, HealthState::Healthy);
        let style = agent_style(&agent);
        assert_eq!(style.fg, None);
    }

    #[test]
    fn agent_style_unknown_is_default() {
        let agent = make_agent("w1", AgentStatus::Idle, HealthState::Unknown);
        let style = agent_style(&agent);
        assert_eq!(style.fg, None);
    }
}
