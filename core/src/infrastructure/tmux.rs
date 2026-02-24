//! Tmux command builder and output parser.
//!
//! `TmuxBackend` implements `SessionBackend` by building tmux CLI command
//! strings. It never spawns processes — the caller is responsible for executing
//! the commands. This keeps the core crate pure and testable.

use std::collections::HashMap;

use cmx_utils::response::{Action, Direction};
use crate::types::session::{LayoutNode, TmuxPane, TmuxWindow};

use super::SessionBackend;

// ---------------------------------------------------------------------------
// Command builder
// ---------------------------------------------------------------------------

/// Builds tmux CLI command strings without executing them.
pub struct TmuxCommandBuilder;

impl TmuxCommandBuilder {
    pub fn new() -> Self {
        TmuxCommandBuilder
    }

    /// `tmux new-session -d -s <name> -c <cwd>`
    pub fn new_session(&self, name: &str, cwd: &str) -> String {
        format!(
            "tmux new-session -d -s {} -c {}",
            shell_escape(name),
            shell_escape(cwd)
        )
    }

    /// `tmux kill-session -t <name>`
    pub fn kill_session(&self, name: &str) -> String {
        format!("tmux kill-session -t {}", shell_escape(name))
    }

    /// `tmux split-window -t <target> [-h|-v] -p <percent>`
    pub fn split_pane(&self, target: &str, direction: &Direction, percent: u32) -> String {
        let flag = match direction {
            Direction::Horizontal => "-h",
            Direction::Vertical => "-v",
        };
        format!(
            "tmux split-window -t {} {} -p {}",
            shell_escape(target),
            flag,
            percent
        )
    }

    /// `tmux send-keys -t <target> <keys> Enter`
    pub fn send_keys(&self, target: &str, keys: &str) -> String {
        format!(
            "tmux send-keys -t {} {} Enter",
            shell_escape(target),
            shell_escape(keys)
        )
    }

    /// `tmux capture-pane -t <target> -p`
    pub fn capture_pane(&self, target: &str) -> String {
        format!("tmux capture-pane -t {} -p", shell_escape(target))
    }

    /// `tmux resize-pane -t <target> [-L|-R|-U|-D] <amount>`
    pub fn resize_pane(&self, target: &str, direction: &Direction, amount: u32) -> String {
        let flag = match direction {
            Direction::Horizontal => "-R",
            Direction::Vertical => "-D",
        };
        format!(
            "tmux resize-pane -t {} {} {}",
            shell_escape(target),
            flag,
            amount
        )
    }

    /// `tmux list-sessions -F '#{session_name}'`
    pub fn list_sessions(&self) -> String {
        "tmux list-sessions -F '#{session_name}'".to_string()
    }

    /// `tmux list-panes -t <session> -F '#{pane_id}:#{pane_index}:#{pane_width}:#{pane_height}:#{pane_top}:#{pane_left}'`
    pub fn list_panes(&self, session: &str) -> String {
        format!(
            "tmux list-panes -t {} -F '#{{pane_id}}:#{{pane_index}}:#{{pane_width}}:#{{pane_height}}:#{{pane_top}}:#{{pane_left}}'",
            shell_escape(session)
        )
    }

    /// `tmux list-windows -t <session> -F '#{window_index}:#{window_name}:#{window_panes}'`
    pub fn list_windows(&self, session: &str) -> String {
        format!(
            "tmux list-windows -t {} -F '#{{window_index}}:#{{window_name}}:#{{window_panes}}'",
            shell_escape(session)
        )
    }

    /// `tmux select-pane -t <target>`
    pub fn select_pane(&self, target: &str) -> String {
        format!("tmux select-pane -t {}", shell_escape(target))
    }

    /// `tmux select-window -t <target>`
    pub fn select_window(&self, target: &str) -> String {
        format!("tmux select-window -t {}", shell_escape(target))
    }

    /// `tmux rename-session -t <old> <new>`
    pub fn rename_session(&self, old: &str, new: &str) -> String {
        format!(
            "tmux rename-session -t {} {}",
            shell_escape(old),
            shell_escape(new)
        )
    }

    /// `tmux rename-window -t <target> <name>`
    pub fn rename_window(&self, target: &str, name: &str) -> String {
        format!(
            "tmux rename-window -t {} {}",
            shell_escape(target),
            shell_escape(name)
        )
    }
}

impl Default for TmuxCommandBuilder {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Output parsers
// ---------------------------------------------------------------------------

/// Parse the output of `list_panes` into `TmuxPane` structs.
///
/// Expected line format: `%id:index:width:height:top:left`
pub fn parse_list_panes(output: &str) -> Vec<TmuxPane> {
    let mut panes = Vec::new();
    for line in output.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.splitn(6, ':').collect();
        if parts.len() < 6 {
            continue;
        }
        let id = parts[0].to_string();
        let index = parts[1].parse::<u32>().unwrap_or(0);
        let width = parts[2].parse::<u32>().unwrap_or(0);
        let height = parts[3].parse::<u32>().unwrap_or(0);
        let top = parts[4].parse::<u32>().unwrap_or(0);
        let left = parts[5].parse::<u32>().unwrap_or(0);
        panes.push(TmuxPane {
            id,
            index,
            width,
            height,
            top,
            left,
            agent: None,
        });
    }
    panes
}

/// Parse the output of `list_windows` into `TmuxWindow` structs.
///
/// Expected line format: `index:name:pane_count`
pub fn parse_list_windows(output: &str) -> Vec<TmuxWindow> {
    let mut windows = Vec::new();
    for line in output.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.splitn(3, ':').collect();
        if parts.len() < 3 {
            continue;
        }
        let index = parts[0].parse::<u32>().unwrap_or(0);
        let name = parts[1].to_string();
        // pane_count is informational; panes are filled separately.
        windows.push(TmuxWindow {
            index,
            name,
            panes: Vec::new(),
        });
    }
    windows
}

/// Parse the output of `list_sessions` into session name strings.
///
/// Each line is a session name.
pub fn parse_list_sessions(output: &str) -> Vec<String> {
    output
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect()
}

// ---------------------------------------------------------------------------
// Layout realization
// ---------------------------------------------------------------------------

/// Convert a `LayoutNode` tree into a sequence of tmux commands that will
/// build the described layout inside the given session.
///
/// The first pane is assumed to already exist (it is the default pane created
/// with the session). Subsequent panes are created via `split-window`.
///
/// Returns a vec of command strings in execution order.
pub fn realize_layout(session: &str, layout: &LayoutNode) -> Vec<String> {
    let builder = TmuxCommandBuilder::new();
    let mut commands = Vec::new();
    let target_base = format!("{}:0", session);
    realize_node(&builder, &target_base, layout, &mut commands, true);
    commands
}

/// Recursive helper that walks the layout tree. `is_first` tracks whether
/// the current node can reuse an existing pane or must split.
fn realize_node(
    builder: &TmuxCommandBuilder,
    target: &str,
    node: &LayoutNode,
    commands: &mut Vec<String>,
    is_first: bool,
) {
    match node {
        LayoutNode::Pane { agent } => {
            if !is_first {
                // We have already split to create this pane; nothing else to do.
            }
            // Annotate: send a comment so pane can be identified.
            let _ = agent; // agent binding used for documentation only in command gen
        }
        LayoutNode::Row { children } => {
            for (i, entry) in children.iter().enumerate() {
                if i == 0 {
                    realize_node(builder, target, &entry.node, commands, is_first);
                } else {
                    let pct = entry.percent.unwrap_or(50);
                    commands.push(builder.split_pane(target, &Direction::Horizontal, pct));
                    realize_node(builder, target, &entry.node, commands, false);
                }
            }
        }
        LayoutNode::Col { children } => {
            for (i, entry) in children.iter().enumerate() {
                if i == 0 {
                    realize_node(builder, target, &entry.node, commands, is_first);
                } else {
                    let pct = entry.percent.unwrap_or(50);
                    commands.push(builder.split_pane(target, &Direction::Vertical, pct));
                    realize_node(builder, target, &entry.node, commands, false);
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Shell escaping
// ---------------------------------------------------------------------------

/// Escape a string for safe use in a shell command.
///
/// Wraps the value in single quotes and escapes any embedded single quotes
/// using the `'\''` idiom.
pub fn shell_escape(s: &str) -> String {
    if s.is_empty() {
        return "''".to_string();
    }
    // If the string contains no special characters, return it bare.
    if s.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' || c == '/' || c == '%' || c == ':')
    {
        return s.to_string();
    }
    // Otherwise, wrap in single quotes.
    let escaped = s.replace('\'', "'\\''");
    format!("'{}'", escaped)
}

// ---------------------------------------------------------------------------
// TmuxBackend (SessionBackend implementation)
// ---------------------------------------------------------------------------

/// A `SessionBackend` that records the commands it would execute and tracks
/// logical session state. Used as the production adapter whose command
/// output is later fed to the system shell.
pub struct TmuxBackend {
    builder: TmuxCommandBuilder,
    /// Commands generated by `execute_action`, in order.
    pub commands: Vec<String>,
    /// Logical set of sessions (names) tracked by the backend.
    sessions: Vec<String>,
    /// Simulated pane captures, keyed by target string.
    pane_captures: HashMap<String, String>,
}

impl TmuxBackend {
    pub fn new() -> Self {
        TmuxBackend {
            builder: TmuxCommandBuilder::new(),
            commands: Vec::new(),
            sessions: Vec::new(),
            pane_captures: HashMap::new(),
        }
    }

    /// Pre-populate known sessions (e.g. after parsing `list-sessions` output).
    pub fn set_sessions(&mut self, sessions: Vec<String>) {
        self.sessions = sessions;
    }

    /// Pre-populate a pane capture (e.g. after running `capture-pane`).
    pub fn set_pane_capture(&mut self, target: &str, content: &str) {
        self.pane_captures.insert(target.to_string(), content.to_string());
    }

    /// Return all generated commands and clear the buffer.
    pub fn drain_commands(&mut self) -> Vec<String> {
        std::mem::take(&mut self.commands)
    }
}

impl Default for TmuxBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionBackend for TmuxBackend {
    fn execute_action(&mut self, action: &Action) -> Result<(), String> {
        match action {
            Action::CreateSession { name, cwd } => {
                self.commands.push(self.builder.new_session(name, cwd));
                if !self.sessions.contains(name) {
                    self.sessions.push(name.clone());
                }
            }
            Action::KillSession { name } => {
                self.commands.push(self.builder.kill_session(name));
                self.sessions.retain(|s| s != name);
            }
            Action::SplitPane {
                session,
                direction,
                percent,
            } => {
                self.commands
                    .push(self.builder.split_pane(session, direction, *percent));
            }
            Action::SendKeys { target, keys } => {
                self.commands.push(self.builder.send_keys(target, keys));
            }
            Action::CreateAgent { name, role, path } => {
                // Agent creation is a logical operation; no tmux command needed.
                let _ = (name, role, path);
            }
            Action::KillAgent { name } => {
                let _ = name;
            }
            Action::PlaceAgent { pane_id, agent } => {
                let _ = (pane_id, agent);
            }
            Action::ConnectSsh { agent, host, port } => {
                // Build an ssh command to send into the agent's pane.
                let ssh_cmd = format!("ssh -p {} {}", port, host);
                self.commands.push(self.builder.send_keys(agent, &ssh_cmd));
            }
            Action::UpdateAssignment { agent, task } => {
                let _ = (agent, task);
            }
        }
        Ok(())
    }

    fn session_exists(&self, name: &str) -> bool {
        self.sessions.iter().any(|s| s == name)
    }

    fn list_sessions(&self) -> Vec<String> {
        self.sessions.clone()
    }

    fn capture_pane(&self, target: &str) -> Result<String, String> {
        self.pane_captures
            .get(target)
            .cloned()
            .ok_or_else(|| format!("no capture for target '{}'", target))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use cmx_utils::response::Direction;
    use crate::types::session::LayoutEntry;

    #[test]
    fn cmd_new_session() {
        let b = TmuxCommandBuilder::new();
        let cmd = b.new_session("work", "/tmp/proj");
        assert_eq!(cmd, "tmux new-session -d -s work -c /tmp/proj");
    }

    #[test]
    fn cmd_new_session_with_spaces() {
        let b = TmuxCommandBuilder::new();
        let cmd = b.new_session("my session", "/tmp/my proj");
        assert!(cmd.contains("'my session'"));
        assert!(cmd.contains("'/tmp/my proj'"));
    }

    #[test]
    fn cmd_kill_session() {
        let b = TmuxCommandBuilder::new();
        assert_eq!(b.kill_session("work"), "tmux kill-session -t work");
    }

    #[test]
    fn cmd_split_horizontal() {
        let b = TmuxCommandBuilder::new();
        let cmd = b.split_pane("work:0", &Direction::Horizontal, 50);
        assert_eq!(cmd, "tmux split-window -t work:0 -h -p 50");
    }

    #[test]
    fn cmd_split_vertical() {
        let b = TmuxCommandBuilder::new();
        let cmd = b.split_pane("work:0", &Direction::Vertical, 30);
        assert_eq!(cmd, "tmux split-window -t work:0 -v -p 30");
    }

    #[test]
    fn cmd_send_keys() {
        let b = TmuxCommandBuilder::new();
        let cmd = b.send_keys("work:0.1", "ls -la");
        assert!(cmd.starts_with("tmux send-keys"));
        assert!(cmd.contains("Enter"));
    }

    #[test]
    fn cmd_capture_pane() {
        let b = TmuxCommandBuilder::new();
        let cmd = b.capture_pane("work:0.1");
        assert_eq!(cmd, "tmux capture-pane -t work:0.1 -p");
    }

    #[test]
    fn cmd_resize_pane() {
        let b = TmuxCommandBuilder::new();
        let cmd = b.resize_pane("work:0.1", &Direction::Horizontal, 10);
        assert_eq!(cmd, "tmux resize-pane -t work:0.1 -R 10");
    }

    #[test]
    fn cmd_resize_pane_vertical() {
        let b = TmuxCommandBuilder::new();
        let cmd = b.resize_pane("work:0.1", &Direction::Vertical, 5);
        assert_eq!(cmd, "tmux resize-pane -t work:0.1 -D 5");
    }

    #[test]
    fn cmd_list_sessions() {
        let b = TmuxCommandBuilder::new();
        let cmd = b.list_sessions();
        assert!(cmd.contains("list-sessions"));
        assert!(cmd.contains("session_name"));
    }

    #[test]
    fn cmd_list_panes() {
        let b = TmuxCommandBuilder::new();
        let cmd = b.list_panes("work");
        assert!(cmd.contains("list-panes -t work"));
        assert!(cmd.contains("pane_id"));
    }

    #[test]
    fn cmd_list_windows() {
        let b = TmuxCommandBuilder::new();
        let cmd = b.list_windows("work");
        assert!(cmd.contains("list-windows -t work"));
        assert!(cmd.contains("window_index"));
    }

    #[test]
    fn cmd_select_pane() {
        let b = TmuxCommandBuilder::new();
        assert_eq!(b.select_pane("work:0.1"), "tmux select-pane -t work:0.1");
    }

    #[test]
    fn cmd_select_window() {
        let b = TmuxCommandBuilder::new();
        assert_eq!(
            b.select_window("work:1"),
            "tmux select-window -t work:1"
        );
    }

    #[test]
    fn cmd_rename_session() {
        let b = TmuxCommandBuilder::new();
        assert_eq!(
            b.rename_session("old", "new"),
            "tmux rename-session -t old new"
        );
    }

    #[test]
    fn cmd_rename_window() {
        let b = TmuxCommandBuilder::new();
        assert_eq!(
            b.rename_window("work:0", "editor"),
            "tmux rename-window -t work:0 editor"
        );
    }

    // -- Parser tests --

    #[test]
    fn parse_panes_basic() {
        let output = "%0:0:120:40:0:0\n%1:1:60:40:0:60\n";
        let panes = parse_list_panes(output);
        assert_eq!(panes.len(), 2);
        assert_eq!(panes[0].id, "%0");
        assert_eq!(panes[0].width, 120);
        assert_eq!(panes[1].id, "%1");
        assert_eq!(panes[1].left, 60);
    }

    #[test]
    fn parse_panes_empty() {
        assert!(parse_list_panes("").is_empty());
        assert!(parse_list_panes("   \n  \n").is_empty());
    }

    #[test]
    fn parse_panes_malformed_line() {
        let output = "%0:0:120\nbadline\n%1:1:60:40:0:60\n";
        let panes = parse_list_panes(output);
        assert_eq!(panes.len(), 1);
        assert_eq!(panes[0].id, "%1");
    }

    #[test]
    fn parse_windows_basic() {
        let output = "0:main:2\n1:editor:1\n";
        let windows = parse_list_windows(output);
        assert_eq!(windows.len(), 2);
        assert_eq!(windows[0].index, 0);
        assert_eq!(windows[0].name, "main");
        assert_eq!(windows[1].index, 1);
        assert_eq!(windows[1].name, "editor");
    }

    #[test]
    fn parse_windows_empty() {
        assert!(parse_list_windows("").is_empty());
    }

    #[test]
    fn parse_sessions_basic() {
        let output = "cmx-main\nwork\ntest\n";
        let sessions = parse_list_sessions(output);
        assert_eq!(sessions, vec!["cmx-main", "work", "test"]);
    }

    #[test]
    fn parse_sessions_empty() {
        assert!(parse_list_sessions("").is_empty());
    }

    // -- Shell escape tests --

    #[test]
    fn escape_simple() {
        assert_eq!(shell_escape("hello"), "hello");
    }

    #[test]
    fn escape_with_space() {
        assert_eq!(shell_escape("hello world"), "'hello world'");
    }

    #[test]
    fn escape_with_single_quote() {
        assert_eq!(shell_escape("it's"), "'it'\\''s'");
    }

    #[test]
    fn escape_empty() {
        assert_eq!(shell_escape(""), "''");
    }

    #[test]
    fn escape_path_no_quoting() {
        assert_eq!(shell_escape("/tmp/proj-1/src"), "/tmp/proj-1/src");
    }

    // -- Layout realization tests --

    #[test]
    fn realize_single_pane() {
        let layout = LayoutNode::Pane {
            agent: "pilot".into(),
        };
        let cmds = realize_layout("work", &layout);
        assert!(cmds.is_empty()); // first pane exists, no splits needed
    }

    #[test]
    fn realize_two_column_row() {
        let layout = LayoutNode::Row {
            children: vec![
                LayoutEntry {
                    node: LayoutNode::Pane {
                        agent: "pilot".into(),
                    },
                    percent: Some(30),
                },
                LayoutEntry {
                    node: LayoutNode::Pane {
                        agent: "worker".into(),
                    },
                    percent: Some(70),
                },
            ],
        };
        let cmds = realize_layout("work", &layout);
        assert_eq!(cmds.len(), 1);
        assert!(cmds[0].contains("split-window"));
        assert!(cmds[0].contains("-h"));
        assert!(cmds[0].contains("-p 70"));
    }

    #[test]
    fn realize_nested_layout() {
        let layout = LayoutNode::Row {
            children: vec![
                LayoutEntry {
                    node: LayoutNode::Pane {
                        agent: "pilot".into(),
                    },
                    percent: Some(30),
                },
                LayoutEntry {
                    node: LayoutNode::Col {
                        children: vec![
                            LayoutEntry {
                                node: LayoutNode::Pane {
                                    agent: "w1".into(),
                                },
                                percent: Some(50),
                            },
                            LayoutEntry {
                                node: LayoutNode::Pane {
                                    agent: "w2".into(),
                                },
                                percent: Some(50),
                            },
                        ],
                    },
                    percent: Some(70),
                },
            ],
        };
        let cmds = realize_layout("work", &layout);
        // 1 horizontal split (row child 2), then 1 vertical split (col child 2)
        assert_eq!(cmds.len(), 2);
        assert!(cmds[0].contains("-h"));
        assert!(cmds[1].contains("-v"));
    }

    // -- Backend trait tests --

    #[test]
    fn backend_create_and_kill_session() {
        let mut backend = TmuxBackend::new();
        let create = Action::CreateSession {
            name: "work".into(),
            cwd: "/tmp".into(),
        };
        backend.execute_action(&create).unwrap();
        assert!(backend.session_exists("work"));
        assert_eq!(backend.list_sessions(), vec!["work"]);
        assert_eq!(backend.commands.len(), 1);

        let kill = Action::KillSession {
            name: "work".into(),
        };
        backend.execute_action(&kill).unwrap();
        assert!(!backend.session_exists("work"));
        assert_eq!(backend.commands.len(), 2);
    }

    #[test]
    fn backend_split_pane() {
        let mut backend = TmuxBackend::new();
        let split = Action::SplitPane {
            session: "work".into(),
            direction: Direction::Horizontal,
            percent: 50,
        };
        backend.execute_action(&split).unwrap();
        assert_eq!(backend.commands.len(), 1);
        assert!(backend.commands[0].contains("split-window"));
    }

    #[test]
    fn backend_send_keys() {
        let mut backend = TmuxBackend::new();
        let action = Action::SendKeys {
            target: "work:0.1".into(),
            keys: "echo hello".into(),
        };
        backend.execute_action(&action).unwrap();
        assert!(backend.commands[0].contains("send-keys"));
    }

    #[test]
    fn backend_capture_pane() {
        let mut backend = TmuxBackend::new();
        backend.set_pane_capture("work:0.1", "$ prompt here");
        let capture = backend.capture_pane("work:0.1");
        assert_eq!(capture.unwrap(), "$ prompt here");
    }

    #[test]
    fn backend_capture_pane_missing() {
        let backend = TmuxBackend::new();
        assert!(backend.capture_pane("nonexistent").is_err());
    }

    #[test]
    fn backend_connect_ssh() {
        let mut backend = TmuxBackend::new();
        let action = Action::ConnectSsh {
            agent: "worker-1".into(),
            host: "gpu.example.com".into(),
            port: 2222,
        };
        backend.execute_action(&action).unwrap();
        assert_eq!(backend.commands.len(), 1);
        assert!(backend.commands[0].contains("ssh -p 2222"));
    }

    #[test]
    fn backend_drain_commands() {
        let mut backend = TmuxBackend::new();
        let action = Action::CreateSession {
            name: "s1".into(),
            cwd: "/tmp".into(),
        };
        backend.execute_action(&action).unwrap();
        let drained = backend.drain_commands();
        assert_eq!(drained.len(), 1);
        assert!(backend.commands.is_empty());
    }

    #[test]
    fn backend_duplicate_session_no_duplicate() {
        let mut backend = TmuxBackend::new();
        let action = Action::CreateSession {
            name: "dup".into(),
            cwd: "/tmp".into(),
        };
        backend.execute_action(&action).unwrap();
        backend.execute_action(&action).unwrap();
        assert_eq!(backend.list_sessions().len(), 1);
    }
}
