//! Command — the typed interface for all CMX daemon operations.
//!
//! Every operation that can be dispatched through `Sys::execute()` is a variant
//! of the `Command` enum. This enum serves as both the wire format (JSON over
//! the Unix socket) and the API documentation for the core crate.
//!
//! # Wire Format
//!
//! Commands are serialized as JSON objects with a `"command"` discriminant:
//!
//! ```json
//! {"command": "agent.new", "role": "worker", "name": "w1"}
//! {"command": "status"}
//! {"command": "task.set", "id": "T1", "status": "in_progress"}
//! ```
//!
//! The serde `tag = "command"` attribute handles this automatically.
//!
//! # Command Groups
//!
//! | Group | Commands |
//! |-------|----------|
//! | Top-level | `status`, `view` |
//! | Agent | `agent.new`, `agent.kill`, `agent.restart`, `agent.assign`, `agent.unassign`, `agent.status`, `agent.list` |
//! | Task | `task.list`, `task.get`, `task.set`, `task.check`, `task.uncheck` |
//! | Config | `config.load`, `config.save`, `config.add`, `config.list` |
//! | Project | `project.add`, `project.remove`, `project.list`, `project.scan` |
//! | Pool | `pool.list`, `pool.status`, `pool.set`, `pool.remove` |
//! | Messaging | `tell`, `interrupt` |
//! | Layout | `layout.row`, `layout.column`, `layout.merge`, `layout.place`, `layout.capture`, `layout.session` |
//! | Client | `client.next`, `client.prev` |
//! | Rig | `rig.init`, `rig.push`, `rig.pull`, `rig.status`, `rig.health`, `rig.stop`, `rig.list`, `rig.default` |
//! | Diagnosis | `diagnosis.report`, `diagnosis.reliability`, `diagnosis.effectiveness`, `diagnosis.thresholds`, `diagnosis.events` |
//! | History | `history.list`, `history.show`, `history.diff`, `history.restore`, `history.snapshot`, `history.prune` |
//! | Learnings | `learnings.list`, `learnings.add`, `learnings.search` |
//! | Watch | `watch` |
//! | Daemon | `daemon.run`, `daemon.stop` |

use serde::{Deserialize, Serialize};


/// A typed command sent to the CMX daemon.
///
/// Each variant corresponds to exactly one operation in `Sys::execute()`.
/// Required fields are non-optional; optional fields use `Option<String>`.
/// The `#[serde(tag = "command")]` attribute produces internally-tagged JSON
/// where the `"command"` key selects the variant.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "command")]
pub enum Command {
    // -----------------------------------------------------------------
    // Top-level commands
    // -----------------------------------------------------------------

    /// Return a summary of system state (agent count, task count, etc.).
    #[serde(rename = "status")]
    Status {
        /// Output format: "json" for JSON, omit for one-liner.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        format: Option<String>,
    },

    /// Look up an entity by name — tries agents, then tasks, then projects.
    #[serde(rename = "view")]
    View {
        /// The name to look up.
        name: String,
    },

    // -----------------------------------------------------------------
    // Agent commands
    // -----------------------------------------------------------------

    /// Create a new agent with the given role.
    #[serde(rename = "agent.new")]
    AgentNew {
        /// Role string (e.g. "worker", "pilot", "pm").
        role: String,
        /// Optional agent name. Auto-generated if omitted (e.g. "worker1").
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        /// Working directory. Defaults to project_root from settings.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        path: Option<String>,
        /// Agent type: "claude" (default), "console", or "ssh".
        #[serde(default, skip_serializing_if = "Option::is_none")]
        agent_type: Option<String>,
    },

    /// Kill (remove) an agent by name.
    #[serde(rename = "agent.kill")]
    AgentKill {
        /// Name of the agent to kill.
        name: String,
    },

    /// Restart an agent (kill + re-create with same config).
    #[serde(rename = "agent.restart")]
    AgentRestart {
        /// Name of the agent to restart.
        name: String,
    },

    /// Assign an agent to a task.
    #[serde(rename = "agent.assign")]
    AgentAssign {
        /// Name of the agent.
        name: String,
        /// Task ID to assign.
        task: String,
    },

    /// Remove the current task assignment from an agent.
    #[serde(rename = "agent.unassign")]
    AgentUnassign {
        /// Name of the agent.
        name: String,
    },

    /// Update an agent's status notes.
    #[serde(rename = "agent.status")]
    AgentStatus {
        /// Name of the agent.
        name: String,
        /// Free-text status notes.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        notes: Option<String>,
    },

    /// List all agents. Supports optional JSON output.
    #[serde(rename = "agent.list")]
    AgentList {
        /// Output format: "json" for JSON, omit for tabular.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        format: Option<String>,
    },

    // -----------------------------------------------------------------
    // Task commands
    // -----------------------------------------------------------------

    /// List all tasks, optionally filtered by project.
    #[serde(rename = "task.list")]
    TaskList {
        /// Output format: "json" for JSON, omit for tabular.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        format: Option<String>,
        /// Filter to tasks under this project.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        project: Option<String>,
    },

    /// Get detailed information about a single task.
    #[serde(rename = "task.get")]
    TaskGet {
        /// Task ID.
        id: String,
    },

    /// Update fields on a task (status, title, result, agent).
    #[serde(rename = "task.set")]
    TaskSet {
        /// Task ID.
        id: String,
        /// New status string (e.g. "pending", "in_progress", "completed").
        #[serde(default, skip_serializing_if = "Option::is_none")]
        status: Option<String>,
        /// New title.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        /// Result text.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        result: Option<String>,
        /// Agent name to assign, or "-" to clear.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        agent: Option<String>,
    },

    /// Mark a task as completed.
    #[serde(rename = "task.check")]
    TaskCheck {
        /// Task ID.
        id: String,
    },

    /// Mark a task as pending (undo check).
    #[serde(rename = "task.uncheck")]
    TaskUncheck {
        /// Task ID.
        id: String,
    },

    // -----------------------------------------------------------------
    // Config commands
    // -----------------------------------------------------------------

    /// Load settings from a YAML file.
    #[serde(rename = "config.load")]
    ConfigLoad {
        /// Path to YAML file. Defaults to `<config_dir>/settings.yaml`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        path: Option<String>,
    },

    /// Save current settings to a YAML file.
    #[serde(rename = "config.save")]
    ConfigSave {
        /// Path to YAML file. Defaults to `<config_dir>/settings.yaml`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        path: Option<String>,
    },

    /// Set a single configuration key to a value.
    #[serde(rename = "config.add")]
    ConfigAdd {
        /// Configuration key (e.g. "max_retries", "project_root").
        key: String,
        /// Value to set.
        value: String,
    },

    /// List all current configuration values.
    #[serde(rename = "config.list")]
    ConfigList,

    // -----------------------------------------------------------------
    // Project commands
    // -----------------------------------------------------------------

    /// Register a project folder.
    #[serde(rename = "project.add")]
    ProjectAdd {
        /// Project name (short identifier).
        name: String,
        /// Filesystem path to the project root.
        path: String,
    },

    /// Remove a registered project.
    #[serde(rename = "project.remove")]
    ProjectRemove {
        /// Project name.
        name: String,
    },

    /// List all registered projects.
    #[serde(rename = "project.list")]
    ProjectList {
        /// Output format: "json" for JSON, omit for tabular.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        format: Option<String>,
    },

    /// Scan a project folder for task subfolders.
    #[serde(rename = "project.scan")]
    ProjectScan {
        /// Project name to scan.
        name: String,
    },

    /// Load tasks from a Roadmap.md file into the task tree.
    #[serde(rename = "roadmap.load")]
    RoadmapLoad {
        /// Path to the Roadmap.md file.
        path: String,
    },

    // -----------------------------------------------------------------
    // Pool commands
    // -----------------------------------------------------------------

    /// List all configured worker pools with current status.
    #[serde(rename = "pool.list")]
    PoolList,

    /// Show detailed status for a specific pool by role.
    #[serde(rename = "pool.status")]
    PoolStatus {
        /// Role name (e.g. "worker").
        role: String,
    },

    /// Create or update a worker pool for a role.
    #[serde(rename = "pool.set")]
    PoolSet {
        /// Role name (e.g. "worker").
        role: String,
        /// Target pool size.
        size: u32,
        /// Working directory for spawned workers. Defaults to project_root.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        path: Option<String>,
    },

    /// Remove a worker pool configuration for a role.
    #[serde(rename = "pool.remove")]
    PoolRemove {
        /// Role name (e.g. "worker").
        role: String,
    },

    // -----------------------------------------------------------------
    // Messaging commands
    // -----------------------------------------------------------------

    /// Send a text message to an agent (queued + SendKeys action).
    #[serde(rename = "tell")]
    Tell {
        /// Target agent name.
        agent: String,
        /// Message text.
        text: String,
    },

    /// Interrupt an agent (sends Ctrl-C, optionally followed by text).
    #[serde(rename = "interrupt")]
    Interrupt {
        /// Target agent name.
        agent: String,
        /// Optional text to send after Ctrl-C.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        text: Option<String>,
    },

    // -----------------------------------------------------------------
    // Layout commands
    // -----------------------------------------------------------------

    /// Split a session with a horizontal divider (row split).
    #[serde(rename = "layout.row")]
    LayoutRow {
        /// Target session name.
        session: String,
        /// Split percentage (default 50).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        percent: Option<String>,
    },

    /// Split a session with a vertical divider (column split).
    #[serde(rename = "layout.column")]
    LayoutColumn {
        /// Target session name.
        session: String,
        /// Split percentage (default 50).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        percent: Option<String>,
    },

    /// Merge all panes in a session into one.
    #[serde(rename = "layout.merge")]
    LayoutMerge {
        /// Target session name.
        session: String,
    },

    /// Place an agent into a specific tmux pane.
    #[serde(rename = "layout.place")]
    LayoutPlace {
        /// Pane identifier (e.g. "%3").
        pane: String,
        /// Agent name to place.
        agent: String,
    },

    /// Capture the current pane contents of a session.
    #[serde(rename = "layout.capture")]
    LayoutCapture {
        /// Target session name.
        session: String,
    },

    /// Create a new tmux session.
    #[serde(rename = "layout.session")]
    LayoutSession {
        /// Session name.
        name: String,
        /// Working directory. Defaults to project_root.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cwd: Option<String>,
    },

    // -----------------------------------------------------------------
    // Client commands
    // -----------------------------------------------------------------

    /// Switch to the next client view.
    #[serde(rename = "client.next")]
    ClientNext,

    /// Switch to the previous client view.
    #[serde(rename = "client.prev")]
    ClientPrev,

    // -----------------------------------------------------------------
    // Rig (remote worker) commands
    // -----------------------------------------------------------------

    /// Initialize a remote host: register and verify SSH connectivity.
    #[serde(rename = "rig.init")]
    RigInit {
        /// Host address (e.g. "user@host:port" or "host").
        host: String,
        /// Optional remote name. Defaults to "default".
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
    },

    /// Push code to a remote via rsync.
    #[serde(rename = "rig.push")]
    RigPush {
        /// Local folder path to push.
        folder: String,
        /// Optional remote name.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        remote: Option<String>,
    },

    /// Pull results from a remote via rsync.
    #[serde(rename = "rig.pull")]
    RigPull {
        /// Local folder path to pull into.
        folder: String,
        /// Optional remote name.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        remote: Option<String>,
    },

    /// Show status for a remote.
    #[serde(rename = "rig.status")]
    RigStatus {
        /// Optional remote name. Shows default if omitted.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        remote: Option<String>,
    },

    /// Run a health check on a remote (SSH connectivity test).
    #[serde(rename = "rig.health")]
    RigHealth {
        /// Optional remote name.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        remote: Option<String>,
    },

    /// Stop running operations on a remote.
    #[serde(rename = "rig.stop")]
    RigStop {
        /// Optional remote name.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        remote: Option<String>,
    },

    /// List all configured remotes.
    #[serde(rename = "rig.list")]
    RigList,

    /// Show or set the default remote.
    #[serde(rename = "rig.default")]
    RigDefault {
        /// Remote name to set as default, or omit to show current default.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
    },

    // -----------------------------------------------------------------
    // Diagnosis commands
    // -----------------------------------------------------------------

    /// Generate a self-diagnosis report (markdown).
    #[serde(rename = "diagnosis.report")]
    DiagnosisReport,

    /// Show signal reliability statistics.
    #[serde(rename = "diagnosis.reliability")]
    DiagnosisReliability {
        /// Optional signal type to filter (e.g. "heartbeat_stale"). Shows all if omitted.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        signal: Option<String>,
        /// Output format: "json" for JSON, omit for tabular.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        format: Option<String>,
    },

    /// Show intervention effectiveness by (signal, action) pairs.
    #[serde(rename = "diagnosis.effectiveness")]
    DiagnosisEffectiveness {
        /// Optional signal type to filter.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        signal: Option<String>,
        /// Output format: "json" for JSON, omit for tabular.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        format: Option<String>,
    },

    /// Show current adaptive thresholds for all signal types.
    #[serde(rename = "diagnosis.thresholds")]
    DiagnosisThresholds {
        /// Output format: "json" for JSON, omit for tabular.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        format: Option<String>,
    },

    /// List recent intervention events.
    #[serde(rename = "diagnosis.events")]
    DiagnosisEvents {
        /// Number of recent events to show. Default: 20.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        limit: Option<String>,
        /// Output format: "json" for JSON, omit for tabular.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        format: Option<String>,
    },

    // -----------------------------------------------------------------
    // History commands
    // -----------------------------------------------------------------

    /// List configuration history snapshots.
    #[serde(rename = "history.list")]
    HistoryList {
        /// Maximum number of entries to show. Default: 20.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        limit: Option<String>,
        /// Output format: "json" for JSON, omit for tabular.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        format: Option<String>,
    },

    /// Show the content of a history snapshot.
    #[serde(rename = "history.show")]
    HistoryShow {
        /// Snapshot filename (e.g. "2026-02-22T10-00-00.md") or index (e.g. "0" for latest).
        id: String,
    },

    /// Show diff between two history snapshots.
    #[serde(rename = "history.diff")]
    HistoryDiff {
        /// From snapshot (filename or index).
        from: String,
        /// To snapshot (filename or index). Default: current config.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        to: Option<String>,
    },

    /// Restore a history snapshot as the current configuration.
    #[serde(rename = "history.restore")]
    HistoryRestore {
        /// Snapshot filename or index to restore.
        id: String,
    },

    /// Take a snapshot of the current configuration now.
    #[serde(rename = "history.snapshot")]
    HistorySnapshot,

    /// Prune old history snapshots per retention policy.
    #[serde(rename = "history.prune")]
    HistoryPrune,

    // -----------------------------------------------------------------
    // Watch commands
    // -----------------------------------------------------------------

    /// Register this connection as a watcher. The response is deferred
    /// until a state change occurs or the timeout elapses.
    /// This command is handled at the service layer, not dispatched to Sys.
    #[serde(rename = "watch")]
    Watch {
        /// Optional: only report changes since this timestamp (epoch ms).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        since: Option<String>,
        /// Timeout in milliseconds. Default: 30000 (30 seconds).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        timeout: Option<String>,
    },


    // -----------------------------------------------------------------
    // Daemon lifecycle commands
    // -----------------------------------------------------------------

    /// Start the daemon in the foreground (blocking). Used internally
    /// by execute_remote to spawn a daemon process.
    #[serde(rename = "daemon.run")]
    DaemonRun,

    /// Request a running daemon to shut down gracefully.
    #[serde(rename = "daemon.stop")]
    DaemonStop,

    /// Launch the terminal UI. Handled by the CLI binary, not the daemon.
    #[serde(rename = "tui")]
    Tui,

    // -----------------------------------------------------------------
    // Learnings commands
    // -----------------------------------------------------------------

    /// List learning entries, optionally filtered by project or tag.
    #[serde(rename = "learnings.list")]
    LearningsList {
        /// Filter to entries from this project.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        project: Option<String>,
        /// Filter to entries with this tag.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tag: Option<String>,
    },

    /// Add a new learning entry to a project's LEARNINGS.md.
    #[serde(rename = "learnings.add")]
    LearningsAdd {
        /// Project name (must be registered in folder registry).
        project: String,
        /// Short title for the entry.
        title: String,
        /// Body text explaining the learning.
        body: String,
    },

    /// Full-text search across all projects' LEARNINGS.md files.
    #[serde(rename = "learnings.search")]
    LearningsSearch {
        /// Search query (case-insensitive substring match).
        query: String,
    },

    // -----------------------------------------------------------------
    // Help
    // -----------------------------------------------------------------

    /// Show help text. With no topic, shows the command overview.
    /// With a topic, shows detailed help for that command or group.
    #[serde(rename = "help")]
    Help {
        /// Optional topic: a command name (e.g. "agent.new") or group (e.g. "agent").
        #[serde(default, skip_serializing_if = "Option::is_none")]
        topic: Option<String>,
    },
}


// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // --- Serialization round-trips ---

    #[test]
    fn status_round_trip() {
        let cmd = Command::Status { format: None };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("\"command\":\"status\""));
        let back: Command = serde_json::from_str(&json).unwrap();
        assert_eq!(back, cmd);
    }

    #[test]
    fn view_round_trip() {
        let cmd = Command::View { name: "w1".into() };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("\"command\":\"view\""));
        assert!(json.contains("\"name\":\"w1\""));
        let back: Command = serde_json::from_str(&json).unwrap();
        assert_eq!(back, cmd);
    }

    #[test]
    fn agent_new_full_round_trip() {
        let cmd = Command::AgentNew {
            role: "worker".into(),
            name: Some("w1".into()),
            path: Some("/tmp".into()),
            agent_type: Some("ssh".into()),
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("\"command\":\"agent.new\""));
        assert!(json.contains("\"role\":\"worker\""));
        assert!(json.contains("\"name\":\"w1\""));
        assert!(json.contains("\"agent_type\":\"ssh\""));
        let back: Command = serde_json::from_str(&json).unwrap();
        assert_eq!(back, cmd);
    }

    #[test]
    fn agent_new_minimal_round_trip() {
        let cmd = Command::AgentNew {
            role: "worker".into(),
            name: None,
            path: None,
            agent_type: None,
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("\"command\":\"agent.new\""));
        assert!(json.contains("\"role\":\"worker\""));
        // Optional fields should be omitted
        assert!(!json.contains("\"name\""));
        assert!(!json.contains("\"path\""));
        assert!(!json.contains("\"agent_type\""));
        let back: Command = serde_json::from_str(&json).unwrap();
        assert_eq!(back, cmd);
    }

    #[test]
    fn agent_new_from_minimal_json() {
        // Deserialize from JSON that omits optional fields
        let json = r#"{"command":"agent.new","role":"pilot"}"#;
        let cmd: Command = serde_json::from_str(json).unwrap();
        assert_eq!(
            cmd,
            Command::AgentNew {
                role: "pilot".into(),
                name: None,
                path: None,
                agent_type: None,
            }
        );
    }

    #[test]
    fn agent_kill_round_trip() {
        let cmd = Command::AgentKill { name: "w1".into() };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("\"command\":\"agent.kill\""));
        let back: Command = serde_json::from_str(&json).unwrap();
        assert_eq!(back, cmd);
    }

    #[test]
    fn agent_restart_round_trip() {
        let cmd = Command::AgentRestart { name: "w1".into() };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("\"command\":\"agent.restart\""));
        let back: Command = serde_json::from_str(&json).unwrap();
        assert_eq!(back, cmd);
    }

    #[test]
    fn agent_assign_round_trip() {
        let cmd = Command::AgentAssign {
            name: "w1".into(),
            task: "T1".into(),
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("\"command\":\"agent.assign\""));
        let back: Command = serde_json::from_str(&json).unwrap();
        assert_eq!(back, cmd);
    }

    #[test]
    fn agent_unassign_round_trip() {
        let cmd = Command::AgentUnassign { name: "w1".into() };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("\"command\":\"agent.unassign\""));
        let back: Command = serde_json::from_str(&json).unwrap();
        assert_eq!(back, cmd);
    }

    #[test]
    fn agent_status_with_notes() {
        let cmd = Command::AgentStatus {
            name: "w1".into(),
            notes: Some("compiling".into()),
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("\"command\":\"agent.status\""));
        assert!(json.contains("\"notes\":\"compiling\""));
        let back: Command = serde_json::from_str(&json).unwrap();
        assert_eq!(back, cmd);
    }

    #[test]
    fn agent_status_no_notes() {
        let json = r#"{"command":"agent.status","name":"w1"}"#;
        let cmd: Command = serde_json::from_str(json).unwrap();
        assert_eq!(
            cmd,
            Command::AgentStatus {
                name: "w1".into(),
                notes: None,
            }
        );
    }

    #[test]
    fn agent_list_plain() {
        let json = r#"{"command":"agent.list"}"#;
        let cmd: Command = serde_json::from_str(json).unwrap();
        assert_eq!(cmd, Command::AgentList { format: None });
    }

    #[test]
    fn agent_list_json_format() {
        let cmd = Command::AgentList {
            format: Some("json".into()),
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("\"format\":\"json\""));
        let back: Command = serde_json::from_str(&json).unwrap();
        assert_eq!(back, cmd);
    }

    #[test]
    fn task_list_round_trip() {
        let cmd = Command::TaskList {
            format: Some("json".into()),
            project: Some("CMX".into()),
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("\"command\":\"task.list\""));
        let back: Command = serde_json::from_str(&json).unwrap();
        assert_eq!(back, cmd);
    }

    #[test]
    fn task_get_round_trip() {
        let cmd = Command::TaskGet { id: "T1".into() };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("\"command\":\"task.get\""));
        let back: Command = serde_json::from_str(&json).unwrap();
        assert_eq!(back, cmd);
    }

    #[test]
    fn task_set_round_trip() {
        let cmd = Command::TaskSet {
            id: "T1".into(),
            status: Some("in_progress".into()),
            title: Some("New Title".into()),
            result: None,
            agent: None,
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("\"command\":\"task.set\""));
        assert!(json.contains("\"status\":\"in_progress\""));
        assert!(!json.contains("\"result\""));
        let back: Command = serde_json::from_str(&json).unwrap();
        assert_eq!(back, cmd);
    }

    #[test]
    fn task_set_minimal_json() {
        let json = r#"{"command":"task.set","id":"X1"}"#;
        let cmd: Command = serde_json::from_str(json).unwrap();
        assert_eq!(
            cmd,
            Command::TaskSet {
                id: "X1".into(),
                status: None,
                title: None,
                result: None,
                agent: None,
            }
        );
    }

    #[test]
    fn task_check_round_trip() {
        let cmd = Command::TaskCheck { id: "T1".into() };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("\"command\":\"task.check\""));
        let back: Command = serde_json::from_str(&json).unwrap();
        assert_eq!(back, cmd);
    }

    #[test]
    fn task_uncheck_round_trip() {
        let cmd = Command::TaskUncheck { id: "T1".into() };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("\"command\":\"task.uncheck\""));
        let back: Command = serde_json::from_str(&json).unwrap();
        assert_eq!(back, cmd);
    }

    #[test]
    fn config_load_with_path() {
        let cmd = Command::ConfigLoad {
            path: Some("/etc/cmx.yaml".into()),
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("\"command\":\"config.load\""));
        let back: Command = serde_json::from_str(&json).unwrap();
        assert_eq!(back, cmd);
    }

    #[test]
    fn config_load_default() {
        let json = r#"{"command":"config.load"}"#;
        let cmd: Command = serde_json::from_str(json).unwrap();
        assert_eq!(cmd, Command::ConfigLoad { path: None });
    }

    #[test]
    fn config_save_round_trip() {
        let cmd = Command::ConfigSave { path: None };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("\"command\":\"config.save\""));
        let back: Command = serde_json::from_str(&json).unwrap();
        assert_eq!(back, cmd);
    }

    #[test]
    fn config_add_round_trip() {
        let cmd = Command::ConfigAdd {
            key: "max_retries".into(),
            value: "10".into(),
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("\"command\":\"config.add\""));
        let back: Command = serde_json::from_str(&json).unwrap();
        assert_eq!(back, cmd);
    }

    #[test]
    fn config_list_round_trip() {
        let cmd = Command::ConfigList;
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("\"command\":\"config.list\""));
        let back: Command = serde_json::from_str(&json).unwrap();
        assert_eq!(back, cmd);
    }

    #[test]
    fn project_add_round_trip() {
        let cmd = Command::ProjectAdd {
            name: "myproj".into(),
            path: "/projects/my".into(),
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("\"command\":\"project.add\""));
        let back: Command = serde_json::from_str(&json).unwrap();
        assert_eq!(back, cmd);
    }

    #[test]
    fn project_remove_round_trip() {
        let cmd = Command::ProjectRemove {
            name: "myproj".into(),
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("\"command\":\"project.remove\""));
        let back: Command = serde_json::from_str(&json).unwrap();
        assert_eq!(back, cmd);
    }

    #[test]
    fn project_list_round_trip() {
        let cmd = Command::ProjectList { format: None };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("\"command\":\"project.list\""));
        let back: Command = serde_json::from_str(&json).unwrap();
        assert_eq!(back, cmd);
    }

    #[test]
    fn project_scan_round_trip() {
        let cmd = Command::ProjectScan {
            name: "myproj".into(),
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("\"command\":\"project.scan\""));
        let back: Command = serde_json::from_str(&json).unwrap();
        assert_eq!(back, cmd);
    }

    #[test]
    fn tell_round_trip() {
        let cmd = Command::Tell {
            agent: "w1".into(),
            text: "start task".into(),
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("\"command\":\"tell\""));
        let back: Command = serde_json::from_str(&json).unwrap();
        assert_eq!(back, cmd);
    }

    #[test]
    fn interrupt_with_text_round_trip() {
        let cmd = Command::Interrupt {
            agent: "w1".into(),
            text: Some("stop".into()),
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("\"command\":\"interrupt\""));
        let back: Command = serde_json::from_str(&json).unwrap();
        assert_eq!(back, cmd);
    }

    #[test]
    fn interrupt_no_text() {
        let json = r#"{"command":"interrupt","agent":"w1"}"#;
        let cmd: Command = serde_json::from_str(json).unwrap();
        assert_eq!(
            cmd,
            Command::Interrupt {
                agent: "w1".into(),
                text: None,
            }
        );
    }

    #[test]
    fn layout_row_round_trip() {
        let cmd = Command::LayoutRow {
            session: "main".into(),
            percent: Some("30".into()),
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("\"command\":\"layout.row\""));
        let back: Command = serde_json::from_str(&json).unwrap();
        assert_eq!(back, cmd);
    }

    #[test]
    fn layout_column_round_trip() {
        let cmd = Command::LayoutColumn {
            session: "main".into(),
            percent: None,
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("\"command\":\"layout.column\""));
        let back: Command = serde_json::from_str(&json).unwrap();
        assert_eq!(back, cmd);
    }

    #[test]
    fn layout_merge_round_trip() {
        let cmd = Command::LayoutMerge {
            session: "main".into(),
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("\"command\":\"layout.merge\""));
        let back: Command = serde_json::from_str(&json).unwrap();
        assert_eq!(back, cmd);
    }

    #[test]
    fn layout_place_round_trip() {
        let cmd = Command::LayoutPlace {
            pane: "%3".into(),
            agent: "w1".into(),
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("\"command\":\"layout.place\""));
        let back: Command = serde_json::from_str(&json).unwrap();
        assert_eq!(back, cmd);
    }

    #[test]
    fn layout_capture_round_trip() {
        let cmd = Command::LayoutCapture {
            session: "main".into(),
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("\"command\":\"layout.capture\""));
        let back: Command = serde_json::from_str(&json).unwrap();
        assert_eq!(back, cmd);
    }

    #[test]
    fn layout_session_round_trip() {
        let cmd = Command::LayoutSession {
            name: "work".into(),
            cwd: Some("/projects".into()),
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("\"command\":\"layout.session\""));
        let back: Command = serde_json::from_str(&json).unwrap();
        assert_eq!(back, cmd);
    }

    #[test]
    fn layout_session_no_cwd() {
        let json = r#"{"command":"layout.session","name":"dev"}"#;
        let cmd: Command = serde_json::from_str(json).unwrap();
        assert_eq!(
            cmd,
            Command::LayoutSession {
                name: "dev".into(),
                cwd: None,
            }
        );
    }

    #[test]
    fn client_next_round_trip() {
        let cmd = Command::ClientNext;
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("\"command\":\"client.next\""));
        let back: Command = serde_json::from_str(&json).unwrap();
        assert_eq!(back, cmd);
    }

    #[test]
    fn client_prev_round_trip() {
        let cmd = Command::ClientPrev;
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("\"command\":\"client.prev\""));
        let back: Command = serde_json::from_str(&json).unwrap();
        assert_eq!(back, cmd);
    }

    // --- Pool command round-trips ---

    #[test]
    fn pool_list_round_trip() {
        let cmd = Command::PoolList;
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("\"command\":\"pool.list\""));
        let back: Command = serde_json::from_str(&json).unwrap();
        assert_eq!(back, cmd);
    }

    #[test]
    fn pool_status_round_trip() {
        let cmd = Command::PoolStatus { role: "worker".into() };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("\"command\":\"pool.status\""));
        assert!(json.contains("\"role\":\"worker\""));
        let back: Command = serde_json::from_str(&json).unwrap();
        assert_eq!(back, cmd);
    }

    #[test]
    fn pool_set_round_trip() {
        let cmd = Command::PoolSet {
            role: "worker".into(),
            size: 3,
            path: Some("/tmp/work".into()),
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("\"command\":\"pool.set\""));
        assert!(json.contains("\"size\":3"));
        let back: Command = serde_json::from_str(&json).unwrap();
        assert_eq!(back, cmd);
    }

    #[test]
    fn pool_set_no_path() {
        let json = r#"{"command":"pool.set","role":"worker","size":2}"#;
        let cmd: Command = serde_json::from_str(json).unwrap();
        assert_eq!(cmd, Command::PoolSet {
            role: "worker".into(),
            size: 2,
            path: None,
        });
    }

    #[test]
    fn pool_remove_round_trip() {
        let cmd = Command::PoolRemove { role: "worker".into() };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("\"command\":\"pool.remove\""));
        let back: Command = serde_json::from_str(&json).unwrap();
        assert_eq!(back, cmd);
    }

    // --- Error cases ---

    #[test]
    fn unknown_command_rejected() {
        let json = r#"{"command":"bogus.command"}"#;
        let result = serde_json::from_str::<Command>(json);
        assert!(result.is_err());
    }

    #[test]
    fn missing_command_key_rejected() {
        let json = r#"{"foo":"bar"}"#;
        let result = serde_json::from_str::<Command>(json);
        assert!(result.is_err());
    }

    #[test]
    fn missing_required_field_rejected() {
        // agent.new requires "role"
        let json = r#"{"command":"agent.new"}"#;
        let result = serde_json::from_str::<Command>(json);
        assert!(result.is_err());
    }

    #[test]
    fn agent_assign_missing_task_rejected() {
        let json = r#"{"command":"agent.assign","name":"w1"}"#;
        let result = serde_json::from_str::<Command>(json);
        assert!(result.is_err());
    }

    // --- Wire compatibility ---

    #[test]
    fn wire_format_matches_old_hashmap() {
        // Verify that the new enum produces JSON that matches what the old
        // HashMap<String, String> format would have produced.
        let cmd = Command::AgentNew {
            role: "worker".into(),
            name: Some("w1".into()),
            path: None,
            agent_type: None,
        };
        let json = serde_json::to_string(&cmd).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["command"], "agent.new");
        assert_eq!(parsed["role"], "worker");
        assert_eq!(parsed["name"], "w1");
        // path and agent_type should not appear (skip_serializing_if)
        assert!(parsed.get("path").is_none());
        assert!(parsed.get("agent_type").is_none());
    }

    #[test]
    fn help_no_topic() {
        let json = r#"{"command":"help"}"#;
        let cmd: Command = serde_json::from_str(json).unwrap();
        assert_eq!(cmd, Command::Help { topic: None });
    }

    #[test]
    fn help_with_topic() {
        let cmd = Command::Help {
            topic: Some("agent".into()),
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("\"command\":\"help\""));
        assert!(json.contains("\"topic\":\"agent\""));
        let back: Command = serde_json::from_str(&json).unwrap();
        assert_eq!(back, cmd);
    }

    // --- Diagnosis command round-trips ---

    #[test]
    fn diagnosis_report_round_trip() {
        let cmd = Command::DiagnosisReport;
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains(r#""command":"diagnosis.report""#));
        let back: Command = serde_json::from_str(&json).unwrap();
        assert_eq!(back, cmd);
    }

    #[test]
    fn diagnosis_reliability_round_trip() {
        let cmd = Command::DiagnosisReliability {
            signal: Some("heartbeat_stale".into()),
            format: Some("json".into()),
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains(r#""command":"diagnosis.reliability""#));
        assert!(json.contains(r#""signal":"heartbeat_stale""#));
        let back: Command = serde_json::from_str(&json).unwrap();
        assert_eq!(back, cmd);
    }

    #[test]
    fn diagnosis_reliability_no_args() {
        let json = r#"{"command":"diagnosis.reliability"}"#;
        let cmd: Command = serde_json::from_str(json).unwrap();
        assert_eq!(cmd, Command::DiagnosisReliability { signal: None, format: None });
    }

    #[test]
    fn diagnosis_effectiveness_round_trip() {
        let cmd = Command::DiagnosisEffectiveness {
            signal: Some("error_pattern".into()),
            format: None,
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains(r#""command":"diagnosis.effectiveness""#));
        assert!(!json.contains(r#""format""#));
        let back: Command = serde_json::from_str(&json).unwrap();
        assert_eq!(back, cmd);
    }

    #[test]
    fn diagnosis_thresholds_round_trip() {
        let cmd = Command::DiagnosisThresholds {
            format: Some("json".into()),
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains(r#""command":"diagnosis.thresholds""#));
        let back: Command = serde_json::from_str(&json).unwrap();
        assert_eq!(back, cmd);
    }

    #[test]
    fn diagnosis_events_round_trip() {
        let cmd = Command::DiagnosisEvents {
            limit: Some("50".into()),
            format: Some("json".into()),
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains(r#""command":"diagnosis.events""#));
        assert!(json.contains(r#""limit":"50""#));
        let back: Command = serde_json::from_str(&json).unwrap();
        assert_eq!(back, cmd);
    }

    #[test]
    fn diagnosis_events_no_args() {
        let json = r#"{"command":"diagnosis.events"}"#;
        let cmd: Command = serde_json::from_str(json).unwrap();
        assert_eq!(cmd, Command::DiagnosisEvents { limit: None, format: None });
    }

    // --- History command round-trips ---

    #[test]
    fn history_list_round_trip() {
        let cmd = Command::HistoryList {
            limit: Some("10".into()),
            format: Some("json".into()),
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains(r#""command":"history.list""#));
        assert!(json.contains(r#""limit":"10""#));
        let back: Command = serde_json::from_str(&json).unwrap();
        assert_eq!(back, cmd);
    }

    #[test]
    fn history_list_no_args() {
        let json = r#"{"command":"history.list"}"#;
        let cmd: Command = serde_json::from_str(json).unwrap();
        assert_eq!(cmd, Command::HistoryList { limit: None, format: None });
    }

    #[test]
    fn history_show_round_trip() {
        let cmd = Command::HistoryShow { id: "2026-02-22T10-00-00.md".into() };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains(r#""command":"history.show""#));
        assert!(json.contains(r#""id":"2026-02-22T10-00-00.md""#));
        let back: Command = serde_json::from_str(&json).unwrap();
        assert_eq!(back, cmd);
    }

    #[test]
    fn history_diff_round_trip() {
        let cmd = Command::HistoryDiff {
            from: "0".into(),
            to: Some("1".into()),
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains(r#""command":"history.diff""#));
        assert!(json.contains(r#""from":"0""#));
        let back: Command = serde_json::from_str(&json).unwrap();
        assert_eq!(back, cmd);
    }

    #[test]
    fn history_diff_no_to() {
        let json = r#"{"command":"history.diff","from":"0"}"#;
        let cmd: Command = serde_json::from_str(json).unwrap();
        assert_eq!(cmd, Command::HistoryDiff { from: "0".into(), to: None });
    }

    #[test]
    fn history_restore_round_trip() {
        let cmd = Command::HistoryRestore { id: "0".into() };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains(r#""command":"history.restore""#));
        let back: Command = serde_json::from_str(&json).unwrap();
        assert_eq!(back, cmd);
    }

    #[test]
    fn history_snapshot_round_trip() {
        let cmd = Command::HistorySnapshot;
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains(r#""command":"history.snapshot""#));
        let back: Command = serde_json::from_str(&json).unwrap();
        assert_eq!(back, cmd);
    }

    #[test]
    fn history_prune_round_trip() {
        let cmd = Command::HistoryPrune;
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains(r#""command":"history.prune""#));
        let back: Command = serde_json::from_str(&json).unwrap();
        assert_eq!(back, cmd);
    }

    #[test]
    fn all_variants_deserialize() {
        // Smoke-test that every variant can deserialize from minimal JSON.
        let cases = vec![
            r#"{"command":"status"}"#,
            r#"{"command":"view","name":"x"}"#,
            r#"{"command":"agent.new","role":"worker"}"#,
            r#"{"command":"agent.kill","name":"x"}"#,
            r#"{"command":"agent.restart","name":"x"}"#,
            r#"{"command":"agent.assign","name":"x","task":"t"}"#,
            r#"{"command":"agent.unassign","name":"x"}"#,
            r#"{"command":"agent.status","name":"x"}"#,
            r#"{"command":"agent.list"}"#,
            r#"{"command":"task.list"}"#,
            r#"{"command":"task.get","id":"x"}"#,
            r#"{"command":"task.set","id":"x"}"#,
            r#"{"command":"task.check","id":"x"}"#,
            r#"{"command":"task.uncheck","id":"x"}"#,
            r#"{"command":"config.load"}"#,
            r#"{"command":"config.save"}"#,
            r#"{"command":"config.add","key":"k","value":"v"}"#,
            r#"{"command":"config.list"}"#,
            r#"{"command":"project.add","name":"p","path":"/x"}"#,
            r#"{"command":"project.remove","name":"p"}"#,
            r#"{"command":"project.list"}"#,
            r#"{"command":"project.scan","name":"p"}"#,
            r#"{"command":"pool.list"}"#,
            r#"{"command":"pool.status","role":"worker"}"#,
            r#"{"command":"pool.set","role":"worker","size":3}"#,
            r#"{"command":"pool.remove","role":"worker"}"#,
            r#"{"command":"tell","agent":"a","text":"t"}"#,
            r#"{"command":"interrupt","agent":"a"}"#,
            r#"{"command":"layout.row","session":"s"}"#,
            r#"{"command":"layout.column","session":"s"}"#,
            r#"{"command":"layout.merge","session":"s"}"#,
            r#"{"command":"layout.place","pane":"%1","agent":"a"}"#,
            r#"{"command":"layout.capture","session":"s"}"#,
            r#"{"command":"layout.session","name":"n"}"#,
            r#"{"command":"client.next"}"#,
            r#"{"command":"client.prev"}"#,
            r#"{"command":"rig.init","host":"user@host:22"}"#,
            r#"{"command":"rig.push","folder":"/local"}"#,
            r#"{"command":"rig.pull","folder":"/local"}"#,
            r#"{"command":"rig.status"}"#,
            r#"{"command":"rig.health"}"#,
            r#"{"command":"rig.stop"}"#,
            r#"{"command":"rig.list"}"#,
            r#"{"command":"rig.default"}"#,
            r#"{"command":"diagnosis.report"}"#,
            r#"{"command":"diagnosis.reliability"}"#,
            r#"{"command":"diagnosis.effectiveness"}"#,
            r#"{"command":"diagnosis.thresholds"}"#,
            r#"{"command":"diagnosis.events"}"#,
            r#"{"command":"history.list"}"#,
            r#"{"command":"history.show","id":"0"}"#,
            r#"{"command":"history.diff","from":"0"}"#,
            r#"{"command":"history.restore","id":"0"}"#,
            r#"{"command":"history.snapshot"}"#,
            r#"{"command":"history.prune"}"#,
            r#"{"command":"learnings.list"}"#,
            r#"{"command":"learnings.add","project":"p","title":"t","body":"b"}"#,
            r#"{"command":"learnings.search","query":"q"}"#,
            r#"{"command":"watch"}"#,
            r#"{"command":"help"}"#,
            r#"{"command":"daemon.run"}"#,
            r#"{"command":"daemon.stop"}"#,
        ];
        for (i, json) in cases.iter().enumerate() {
            let result = serde_json::from_str::<Command>(json);
            assert!(
                result.is_ok(),
                "Variant {} failed to deserialize from: {}. Error: {}",
                i,
                json,
                result.unwrap_err()
            );
        }
    }

    // --- Watch command round-trips ---

    #[test]
    fn watch_round_trip() {
        let cmd = Command::Watch {
            since: Some("1708700000000".into()),
            timeout: Some("5000".into()),
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains(r#""command":"watch""#));
        assert!(json.contains(r#""since":"1708700000000""#));
        assert!(json.contains(r#""timeout":"5000""#));
        let back: Command = serde_json::from_str(&json).unwrap();
        assert_eq!(back, cmd);
    }

    #[test]
    fn watch_no_args() {
        let json = r#"{"command":"watch"}"#;
        let cmd: Command = serde_json::from_str(json).unwrap();
        assert_eq!(cmd, Command::Watch { since: None, timeout: None });
    }

    #[test]
    fn watch_with_since_only() {
        let cmd = Command::Watch {
            since: Some("1708700000000".into()),
            timeout: None,
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(!json.contains("timeout"));
        let back: Command = serde_json::from_str(&json).unwrap();
        assert_eq!(back, cmd);
    }

    // --- Daemon command round-trips ---

    #[test]
    fn daemon_run_round_trip() {
        let cmd = Command::DaemonRun;
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains(r#""command":"daemon.run""#));
        let back: Command = serde_json::from_str(&json).unwrap();
        assert_eq!(back, cmd);
    }

    #[test]
    fn daemon_stop_round_trip() {
        let cmd = Command::DaemonStop;
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains(r#""command":"daemon.stop""#));
        let back: Command = serde_json::from_str(&json).unwrap();
        assert_eq!(back, cmd);
    }

    // --- Learnings command round-trips ---

    #[test]
    fn learnings_list_round_trip() {
        let cmd = Command::LearningsList {
            project: Some("myproj".into()),
            tag: Some("testing".into()),
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains(r#""command":"learnings.list""#));
        assert!(json.contains(r#""project":"myproj""#));
        assert!(json.contains(r#""tag":"testing""#));
        let back: Command = serde_json::from_str(&json).unwrap();
        assert_eq!(back, cmd);
    }

    #[test]
    fn learnings_list_no_args() {
        let json = r#"{"command":"learnings.list"}"#;
        let cmd: Command = serde_json::from_str(json).unwrap();
        assert_eq!(cmd, Command::LearningsList { project: None, tag: None });
    }

    #[test]
    fn learnings_add_round_trip() {
        let cmd = Command::LearningsAdd {
            project: "myproj".into(),
            title: "Tests need flag".into(),
            body: "Use --no-parallel.".into(),
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains(r#""command":"learnings.add""#));
        assert!(json.contains(r#""project":"myproj""#));
        assert!(json.contains(r#""title":"Tests need flag""#));
        let back: Command = serde_json::from_str(&json).unwrap();
        assert_eq!(back, cmd);
    }

    #[test]
    fn learnings_search_round_trip() {
        let cmd = Command::LearningsSearch {
            query: "rate limit".into(),
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains(r#""command":"learnings.search""#));
        assert!(json.contains(r#""query":"rate limit""#));
        let back: Command = serde_json::from_str(&json).unwrap();
        assert_eq!(back, cmd);
    }
}
