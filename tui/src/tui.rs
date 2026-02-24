//! TUI runner — ratatui event loop with terminal setup and cleanup.
//!
//! The [`Tui`] struct owns the ratatui terminal, the application state machine
//! ([`App`]), and an optional [`MuxClient`] for daemon communication. It runs
//! the main loop: draw frames, poll for keyboard events, handle actions, and
//! periodically refresh data from the daemon.

use std::io;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use ratatui::prelude::*;
use ratatui::widgets::Paragraph;
use ratatui::Terminal;

use crate::agent_view;
use crate::app::{App, AppAction, AppState, Key};
use crate::client::MuxClient;
use crate::dashboard;
use crate::notification::{NotificationCenter, NotificationType};

use skill_docket_core::types::agent::Agent;


/// Snapshot of all state needed for rendering a single frame.
///
/// Extracted from `Tui` so that `terminal.draw()` can borrow its closure
/// argument without conflicting with the `&mut self` borrow on the terminal.
struct RenderState<'a> {
    app: &'a App,
    agents: &'a [Agent],
    agent_output: &'a str,
    agent_scroll: u16,
    notifications: &'a NotificationCenter,
}


/// The main TUI application runner.
///
/// Manages terminal raw mode, the alternate screen, the ratatui terminal
/// backend, the application state machine, daemon client, and cached data.
pub struct Tui {
    terminal: Terminal<CrosstermBackend<io::Stdout>>,
    app: App,
    client: Option<MuxClient>,
    tick_rate: Duration,
    last_refresh: Instant,
    /// Cached agent list from the daemon.
    agents: Vec<Agent>,
    /// Cached captured output for the currently viewed agent.
    agent_output: String,
    /// Scroll offset for agent view.
    agent_scroll: u16,
    /// Notification center for overlay banners.
    notifications: NotificationCenter,
}


impl Tui {
    /// Create a new TUI, entering raw mode and the alternate screen.
    ///
    /// If `socket_path` is provided, a [`MuxClient`] is created and connected.
    /// Connection failures are non-fatal; the TUI will run in disconnected mode.
    pub fn new(socket_path: Option<String>) -> Result<Self, io::Error> {
        terminal::enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        let backend = CrosstermBackend::new(stdout);
        let terminal = Terminal::new(backend)?;

        let client = socket_path.and_then(|path| {
            let mut c = MuxClient::new(PathBuf::from(&path));
            c.connect().ok().map(|_| c)
        });

        Ok(Self {
            terminal,
            app: App::new(),
            client,
            tick_rate: Duration::from_millis(250),
            last_refresh: Instant::now(),
            agents: Vec::new(),
            agent_output: String::new(),
            agent_scroll: 0,
            notifications: NotificationCenter::new(50),
        })
    }

    /// Run the main event loop until quit is requested.
    pub fn run(&mut self) -> Result<(), io::Error> {
        // Transition from Startup to Dashboard on first run.
        self.app.navigate_to(AppState::Dashboard);

        loop {
            // Build a snapshot of render state to avoid borrow conflicts.
            let state = RenderState {
                app: &self.app,
                agents: &self.agents,
                agent_output: &self.agent_output,
                agent_scroll: self.agent_scroll,
                notifications: &self.notifications,
            };
            self.terminal.draw(|frame| render_frame(frame, &state))?;

            // Poll for keyboard events with a timeout.
            let timeout = self
                .tick_rate
                .checked_sub(self.last_refresh.elapsed())
                .unwrap_or(Duration::ZERO);

            if event::poll(timeout)? {
                if let Event::Key(key_event) = event::read()? {
                    // Ctrl-C always quits immediately.
                    if key_event.code == KeyCode::Char('c')
                        && key_event.modifiers.contains(KeyModifiers::CONTROL)
                    {
                        break;
                    }

                    let key = crossterm_to_key(key_event.code, key_event.modifiers);
                    if let Some(action) = self.app.handle_key(key) {
                        if self.handle_action(action) {
                            break;
                        }
                    }
                }
            }

            // Periodic data refresh.
            if self.last_refresh.elapsed() >= self.tick_rate {
                self.refresh_data();
                self.last_refresh = Instant::now();
            }
        }

        self.shutdown()
    }

    // -------------------------------------------------------------------
    // Action handling
    // -------------------------------------------------------------------

    /// Handle an `AppAction` returned by the state machine.
    ///
    /// Returns `true` if the application should quit.
    fn handle_action(&mut self, action: AppAction) -> bool {
        match action {
            AppAction::Quit => return true,
            AppAction::SendCommand(cmd_text) => {
                let now_ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64;

                let parsed = parse_command_text(&cmd_text);
                if let Some(client) = &mut self.client {
                    match client.send(&parsed) {
                        Ok(resp) => {
                            let body = match resp {
                                cmx_utils::response::Response::Ok {
                                    output,
                                } => output,
                                cmx_utils::response::Response::Error {
                                    message,
                                } => format!("Error: {}", message),
                            };
                            self.notifications.push(
                                NotificationType::Info,
                                &body,
                                None,
                                now_ms,
                                Some(5000),
                            );
                        }
                        Err(e) => {
                            self.notifications.push(
                                NotificationType::Error,
                                &format!("Send failed: {}", e),
                                None,
                                now_ms,
                                Some(5000),
                            );
                        }
                    }
                } else {
                    self.notifications.push(
                        NotificationType::Warning,
                        &format!("Not connected. Command: {}", cmd_text),
                        None,
                        now_ms,
                        Some(5000),
                    );
                }
                // Return to dashboard after command execution.
                self.app.navigate_to(AppState::Dashboard);
            }
            AppAction::Navigate(state) => {
                self.app.transition(state);
            }
            AppAction::Refresh => {
                self.refresh_data();
            }
            AppAction::ScrollUp => {
                self.app.scroll_up();
                if matches!(self.app.state, AppState::AgentDetail { .. }) {
                    self.agent_scroll = self.agent_scroll.saturating_sub(1);
                }
            }
            AppAction::ScrollDown => {
                self.app.scroll_down(1000);
                if matches!(self.app.state, AppState::AgentDetail { .. }) {
                    self.agent_scroll = self.agent_scroll.saturating_add(1);
                }
            }
            AppAction::SelectNext => {
                let max = if self.agents.is_empty() {
                    0
                } else {
                    self.agents.len() - 1
                };
                self.app.select_next(max);
            }
            AppAction::SelectPrev => {
                self.app.select_prev();
            }
            AppAction::Confirm => {
                // The pending action would be executed here. For now, go back.
                self.app.back();
            }
            AppAction::Cancel => {
                self.app.back();
            }
        }
        false
    }

    // -------------------------------------------------------------------
    // Data refresh
    // -------------------------------------------------------------------

    /// Poll the daemon for fresh agent data.
    fn refresh_data(&mut self) {
        if let Some(client) = &mut self.client {
            if let Ok(json) = client.agent_list_json() {
                if let Ok(agents) = serde_json::from_str::<Vec<Agent>>(&json) {
                    self.agents = agents;
                }
            }
        }

        // Prune expired notifications.
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        self.notifications.prune(now_ms);
    }

    // -------------------------------------------------------------------
    // Shutdown
    // -------------------------------------------------------------------

    /// Restore the terminal to its normal state.
    fn shutdown(&mut self) -> Result<(), io::Error> {
        terminal::disable_raw_mode()?;
        execute!(self.terminal.backend_mut(), LeaveAlternateScreen)?;
        self.terminal.show_cursor()?;
        Ok(())
    }
}


impl Drop for Tui {
    fn drop(&mut self) {
        let _ = terminal::disable_raw_mode();
        let _ = execute!(self.terminal.backend_mut(), LeaveAlternateScreen);
    }
}


// ---------------------------------------------------------------------------
// Rendering (free functions to avoid borrow conflicts)
// ---------------------------------------------------------------------------

/// Render the full screen layout: menu bar, main content, input bar.
fn render_frame(frame: &mut Frame, state: &RenderState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // menu bar
            Constraint::Min(5),   // main content
            Constraint::Length(1), // input / status bar
        ])
        .split(frame.area());

    render_menu_bar(frame, chunks[0]);
    render_main(frame, chunks[1], state);
    render_input_bar(frame, chunks[2], state.app);

    // Notification overlay on top of the main area.
    render_notifications(frame, chunks[1], state.notifications);
}

/// Render the top menu bar with tab labels.
fn render_menu_bar(frame: &mut Frame, area: Rect) {
    let items = vec![
        Span::raw("[+]"),
        Span::raw("  "),
        Span::raw("[>]"),
        Span::raw("  "),
        Span::raw("Config"),
        Span::raw("  "),
        Span::raw("Agents"),
        Span::raw("  "),
        Span::raw("Layout"),
        Span::raw("  "),
        Span::raw("Help"),
    ];
    let menu =
        Paragraph::new(Line::from(items)).style(Style::default().bg(Color::DarkGray));
    frame.render_widget(menu, area);
}

/// Dispatch main-area rendering based on the current app state.
fn render_main(frame: &mut Frame, area: Rect, state: &RenderState) {
    match &state.app.state {
        AppState::Dashboard | AppState::Startup => {
            dashboard::render_dashboard(
                frame,
                area,
                state.agents,
                state.app.selected_index,
            );
        }
        AppState::AgentDetail { name } => {
            agent_view::render_agent_view(
                frame,
                area,
                name,
                state.agent_output,
                state.agent_scroll,
            );
        }
        AppState::HelpView { .. } => {
            let help_text = concat!(
                "ClaudiMux TUI Help\n",
                "\n",
                "  q       Quit\n",
                "  ?       Show this help\n",
                "  /  :    Enter command mode\n",
                "  j/k     Select next/prev agent\n",
                "  Enter   View agent detail\n",
                "  Escape  Go back\n",
                "  r       Refresh data\n",
                "  Ctrl-C  Force quit\n",
            );
            let paragraph = Paragraph::new(help_text)
                .block(
                    ratatui::widgets::Block::default()
                        .borders(ratatui::widgets::Borders::ALL)
                        .title("Help"),
                )
                .wrap(ratatui::widgets::Wrap { trim: false });
            frame.render_widget(paragraph, area);
        }
        AppState::Confirm { prompt, .. } => {
            let text = format!(
                "\n  {}\n\n  [y] Yes   [n] No   [Enter] Confirm   [Esc] Cancel\n",
                prompt,
            );
            let paragraph = Paragraph::new(text).block(
                ratatui::widgets::Block::default()
                    .borders(ratatui::widgets::Borders::ALL)
                    .title("Confirm"),
            );
            frame.render_widget(paragraph, area);
        }
        other => {
            let placeholder = format!("View: {}", other.label());
            frame.render_widget(Paragraph::new(placeholder), area);
        }
    }
}

/// Render the bottom input bar or status line.
fn render_input_bar(frame: &mut Frame, area: Rect, app: &App) {
    let is_command = app.state == AppState::CommandEntry;
    let text = if is_command {
        format!("> {}", app.input.text())
    } else {
        format!(" {} | Press / to enter command", app.state.label())
    };
    let style = if is_command {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    frame.render_widget(Paragraph::new(text).style(style), area);

    // Position cursor when in command entry mode.
    if is_command {
        let cursor_x = area.x + 2 + app.input.cursor_pos() as u16;
        let cursor_y = area.y;
        frame.set_cursor_position((cursor_x, cursor_y));
    }
}

/// Render notification banners as an overlay at the top of the main area.
fn render_notifications(
    frame: &mut Frame,
    area: Rect,
    notifications: &NotificationCenter,
) {
    if let Some(notif) = notifications.latest_unread() {
        let color = match notif.notification_type {
            NotificationType::Error => Color::Red,
            NotificationType::Warning => Color::Yellow,
            NotificationType::Success => Color::Green,
            _ => Color::Cyan,
        };
        let block = ratatui::widgets::Block::default()
            .borders(ratatui::widgets::Borders::ALL)
            .border_style(Style::default().fg(color));
        let text = Paragraph::new(notif.body.clone())
            .block(block)
            .style(Style::default().fg(color));
        let notif_area = Rect {
            x: area.x,
            y: area.y,
            width: area.width,
            height: 3.min(area.height),
        };
        frame.render_widget(text, notif_area);
    }
}


// ---------------------------------------------------------------------------
// Command text parsing
// ---------------------------------------------------------------------------

/// Parse a user-typed command string into a `Command` enum variant.
///
/// This provides basic mapping from typed text to the core command type.
/// Unrecognized commands fall back to `Command::Status { format: None }`.
fn parse_command_text(text: &str) -> skill_docket_core::command::Command {
    use skill_docket_core::command::Command;

    let parts: Vec<&str> = text.trim().splitn(3, ' ').collect();
    let cmd = parts.first().copied().unwrap_or("");

    match cmd {
        "status" => Command::Status { format: None },
        "help" => Command::Help {
            topic: parts.get(1).map(|s| s.to_string()),
        },
        "agent.list" => Command::AgentList {
            format: parts.get(1).map(|s| s.to_string()),
        },
        "agent.new" => Command::AgentNew {
            role: parts.get(1).unwrap_or(&"worker").to_string(),
            name: parts.get(2).map(|s| s.to_string()),
            path: None,
            agent_type: None,
        },
        "agent.kill" => {
            if let Some(name) = parts.get(1) {
                Command::AgentKill {
                    name: name.to_string(),
                }
            } else {
                Command::Status { format: None }
            }
        }
        "task.list" => Command::TaskList {
            format: parts.get(1).map(|s| s.to_string()),
            project: None,
        },
        "project.list" => Command::ProjectList {
            format: parts.get(1).map(|s| s.to_string()),
        },
        "config.list" => Command::ConfigList,
        _ => {
            // Try as a View lookup if it's a single word, otherwise Status.
            if parts.len() == 1 && !cmd.is_empty() {
                Command::View {
                    name: cmd.to_string(),
                }
            } else {
                Command::Status { format: None }
            }
        }
    }
}


// ---------------------------------------------------------------------------
// Key conversion
// ---------------------------------------------------------------------------

/// Convert a crossterm `KeyCode` + `KeyModifiers` into our domain `Key` type.
pub fn crossterm_to_key(code: KeyCode, modifiers: KeyModifiers) -> Key {
    if modifiers.contains(KeyModifiers::CONTROL) {
        if let KeyCode::Char(ch) = code {
            return Key::Ctrl(ch);
        }
    }
    if modifiers.contains(KeyModifiers::ALT) {
        if let KeyCode::Char(ch) = code {
            return Key::Alt(ch);
        }
    }
    match code {
        KeyCode::Char(ch) => Key::Char(ch),
        KeyCode::Enter => Key::Enter,
        KeyCode::Tab => Key::Tab,
        KeyCode::Esc => Key::Escape,
        KeyCode::Backspace => Key::Backspace,
        KeyCode::Delete => Key::Delete,
        KeyCode::Up => Key::Up,
        KeyCode::Down => Key::Down,
        KeyCode::Left => Key::Left,
        KeyCode::Right => Key::Right,
        KeyCode::Home => Key::Home,
        KeyCode::End => Key::End,
        KeyCode::PageUp => Key::PageUp,
        KeyCode::PageDown => Key::PageDown,
        KeyCode::F(n) => Key::F(n),
        _ => Key::Char('\0'), // unmapped keys produce a null char
    }
}


// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crossterm_char_to_key() {
        let key = crossterm_to_key(KeyCode::Char('a'), KeyModifiers::NONE);
        assert_eq!(key, Key::Char('a'));
    }

    #[test]
    fn crossterm_ctrl_to_key() {
        let key = crossterm_to_key(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(key, Key::Ctrl('c'));
    }

    #[test]
    fn crossterm_alt_to_key() {
        let key = crossterm_to_key(KeyCode::Char('x'), KeyModifiers::ALT);
        assert_eq!(key, Key::Alt('x'));
    }

    #[test]
    fn crossterm_enter_to_key() {
        let key = crossterm_to_key(KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(key, Key::Enter);
    }

    #[test]
    fn crossterm_tab_to_key() {
        let key = crossterm_to_key(KeyCode::Tab, KeyModifiers::NONE);
        assert_eq!(key, Key::Tab);
    }

    #[test]
    fn crossterm_escape_to_key() {
        let key = crossterm_to_key(KeyCode::Esc, KeyModifiers::NONE);
        assert_eq!(key, Key::Escape);
    }

    #[test]
    fn crossterm_arrows_to_key() {
        assert_eq!(
            crossterm_to_key(KeyCode::Up, KeyModifiers::NONE),
            Key::Up
        );
        assert_eq!(
            crossterm_to_key(KeyCode::Down, KeyModifiers::NONE),
            Key::Down
        );
        assert_eq!(
            crossterm_to_key(KeyCode::Left, KeyModifiers::NONE),
            Key::Left
        );
        assert_eq!(
            crossterm_to_key(KeyCode::Right, KeyModifiers::NONE),
            Key::Right
        );
    }

    #[test]
    fn crossterm_function_key_to_key() {
        let key = crossterm_to_key(KeyCode::F(5), KeyModifiers::NONE);
        assert_eq!(key, Key::F(5));
    }

    #[test]
    fn crossterm_page_keys_to_key() {
        assert_eq!(
            crossterm_to_key(KeyCode::PageUp, KeyModifiers::NONE),
            Key::PageUp
        );
        assert_eq!(
            crossterm_to_key(KeyCode::PageDown, KeyModifiers::NONE),
            Key::PageDown
        );
    }

    #[test]
    fn crossterm_home_end_to_key() {
        assert_eq!(
            crossterm_to_key(KeyCode::Home, KeyModifiers::NONE),
            Key::Home
        );
        assert_eq!(
            crossterm_to_key(KeyCode::End, KeyModifiers::NONE),
            Key::End
        );
    }

    #[test]
    fn crossterm_backspace_delete_to_key() {
        assert_eq!(
            crossterm_to_key(KeyCode::Backspace, KeyModifiers::NONE),
            Key::Backspace
        );
        assert_eq!(
            crossterm_to_key(KeyCode::Delete, KeyModifiers::NONE),
            Key::Delete
        );
    }

    #[test]
    fn app_quit_from_dashboard() {
        let mut app = App::new();
        app.navigate_to(AppState::Dashboard);
        let action = app.handle_key(Key::Char('q'));
        assert_eq!(action, Some(AppAction::Quit));
    }

    #[test]
    fn app_slash_enters_command_mode() {
        let mut app = App::new();
        app.navigate_to(AppState::Dashboard);
        app.handle_key(Key::Char('/'));
        assert_eq!(app.state, AppState::CommandEntry);
    }

    #[test]
    fn parse_status_command() {
        let cmd = parse_command_text("status");
        assert_eq!(cmd, skill_docket_core::command::Command::Status { format: None });
    }

    #[test]
    fn parse_help_command() {
        let cmd = parse_command_text("help agent");
        assert_eq!(
            cmd,
            skill_docket_core::command::Command::Help {
                topic: Some("agent".into()),
            }
        );
    }

    #[test]
    fn parse_help_no_topic() {
        let cmd = parse_command_text("help");
        assert_eq!(
            cmd,
            skill_docket_core::command::Command::Help { topic: None }
        );
    }

    #[test]
    fn parse_agent_list() {
        let cmd = parse_command_text("agent.list json");
        assert_eq!(
            cmd,
            skill_docket_core::command::Command::AgentList {
                format: Some("json".into()),
            }
        );
    }

    #[test]
    fn parse_unknown_single_word_is_view() {
        let cmd = parse_command_text("w1");
        assert_eq!(
            cmd,
            skill_docket_core::command::Command::View {
                name: "w1".into(),
            }
        );
    }

    #[test]
    fn parse_unknown_multi_word_is_status() {
        let cmd = parse_command_text("foo bar baz");
        assert_eq!(cmd, skill_docket_core::command::Command::Status { format: None });
    }

    #[test]
    fn parse_config_list() {
        let cmd = parse_command_text("config.list");
        assert_eq!(cmd, skill_docket_core::command::Command::ConfigList);
    }
}
