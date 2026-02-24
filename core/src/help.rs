//! Help system — generates usage text for all CMX commands.
//!
//! The help module provides structured help text for the CLI. It supports
//! three levels of detail:
//!
//! 1. **Overview** (`cmx help`) — lists all command groups with summaries
//! 2. **Group help** (`cmx help agent`) — lists commands within a group
//! 3. **Command help** (`cmx help agent.new`) — detailed usage for one command


/// Generate help text for a given topic.
///
/// - `None` → overview of all command groups
/// - `Some("agent")` → list of agent commands
/// - `Some("agent.new")` → detailed help for agent.new
pub fn help_text(topic: Option<&str>) -> String {
    match topic {
        None => overview(),
        Some(t) => {
            // Try exact command match first
            if let Some(text) = command_help(t) {
                return text;
            }
            // Try group match
            if let Some(text) = group_help(t) {
                return text;
            }
            format!("Unknown help topic: '{}'. Run 'cmx help' for a list of commands.", t)
        }
    }
}


/// Top-level overview of all commands.
fn overview() -> String {
    "\
cmx — ClaudiMux command-line interface

Usage: cmx <command> [args...]

Commands:
  status [--json]             Show system summary (agents, tasks, projects)
  view <name>                Look up an agent, task, or project by name
  help [topic]               Show help (this message, or help on a topic)

Agent commands:
  agent new <role> [flags]   Create a new agent
  agent kill <name>          Remove an agent
  agent restart <name>       Restart an agent (kill + re-create)
  agent assign <name> <task> Assign an agent to a task
  agent unassign <name>      Remove task assignment from an agent
  agent status <name> [note] Update an agent's status notes
  agent list [--json]        List all agents

Task commands:
  task list [project] [--json]  List tasks, optionally filtered by project
  task get <id>                 Show detailed task information
  task set <id> key=value ...   Update task fields (status, title, result, agent)
  task check <id>               Mark a task as completed
  task uncheck <id>             Mark a task as pending

Config commands:
  config load [path]         Load settings from YAML file
  config save [path]         Save settings to YAML file
  config add <key> <value>   Set a configuration value
  config list                Show all configuration values

Project commands:
  project add <name> <path>  Register a project folder
  project remove <name>      Remove a registered project
  project list [--json]      List all registered projects
  project scan <name>        Scan a project for task subfolders

Messaging commands:
  tell <agent> <text...>     Send a message to an agent
  interrupt <agent> [text]   Send Ctrl-C to an agent, optionally followed by text

Layout commands:
  layout row <session> [--percent <n>]     Split session horizontally
  layout column <session> [--percent <n>]  Split session vertically
  layout merge <session>                   Merge all panes into one
  layout place <pane> <agent>              Place an agent in a pane
  layout capture <session>                 Capture pane contents
  layout session <name> [--cwd <path>]     Create a new tmux session

Client commands:
  client next                Switch to next client view
  client prev                Switch to previous client view

Rig commands (remote workers):
  rig init <host> [--name <n>]     Initialize a remote host
  rig push <folder> [-r <remote>]  Push code to remote
  rig pull <folder> [-r <remote>]  Pull results from remote
  rig status [-r <remote>]         Show remote status
  rig health [-r <remote>]         Health check remote SSH
  rig stop [-r <remote>]           Stop remote operations
  rig list                         List all configured remotes
  rig default [<name>]             Show or set default remote

Diagnosis commands:
  diagnosis report                 Generate self-diagnosis report
  diagnosis reliability [signal]   Signal reliability statistics
  diagnosis effectiveness [signal] Intervention effectiveness
  diagnosis thresholds             Show adaptive thresholds
  diagnosis events [--limit <n>]   List recent intervention events

History commands:
  history list [--limit <n>]       List configuration snapshots
  history show <id>                Show a snapshot
  history diff <from> [<to>]       Diff two snapshots
  history restore <id>             Restore a snapshot
  history snapshot                 Take a snapshot now
  history prune                    Prune old snapshots

Watch command:
  watch [--since <ms>] [--timeout <ms>]  Stream state changes

Daemon commands:
  daemon run                       Start daemon in foreground
  daemon stop                      Stop running daemon

Pool commands:
  pool list                        List all worker pools
  pool status <role>               Show pool status for a role
  pool set <role> <size> [--path]  Create or update a pool
  pool remove <role>               Remove a pool

Run 'cmx help <command>' for detailed help on a specific command.
Run 'cmx help <group>' for help on a command group (agent, task, config, etc.)."
        .into()
}


/// Help for a command group.
fn group_help(group: &str) -> Option<String> {
    let text = match group {
        "agent" => "\
Agent commands — manage AI agent lifecycle

  agent new <role> [--name <n>] [--path <p>] [--type <t>]
    Create a new agent with the given role. Role is a free-form string
    (common values: worker, pilot, pm, curator). If --name is omitted,
    a name is auto-generated (e.g. worker1, worker2). --type can be
    claude (default), console, or ssh.

  agent kill <name>
    Remove an agent. Emits a KillAgent action for infrastructure cleanup.

  agent restart <name>
    Kill and re-create an agent with the same configuration.
    Resets status to idle and health to unknown.

  agent assign <name> <task>
    Assign an agent to a task. Updates both the agent's task field and
    the task's agent field, and sets task status to in_progress.

  agent unassign <name>
    Remove the agent's current task assignment. Clears the task's agent
    field as well.

  agent status <name> [notes...]
    Update the agent's free-text status notes (e.g. 'compiling', 'running tests').

  agent list [--json]
    List all agents in tabular format. Use --json for JSON output.",

        "task" => "\
Task commands — manage the task tree

  task list [<project>] [--json]
    List all tasks. Optionally filter by project name prefix.
    Use --json for JSON array output.

  task get <id>
    Show detailed JSON for a single task, including status, agent,
    result, and children.

  task set <id> key=value [key=value ...]
    Update one or more fields on a task. Supported fields:
      status   — pending, in_progress, completed, failed, paused, cancelled
      title    — task title text
      result   — result/output text
      agent    — agent name, or '-' to clear

  task check <id>
    Mark a task as completed (shorthand for task set <id> status=completed).

  task uncheck <id>
    Mark a task as pending (shorthand for task set <id> status=pending).",

        "config" => "\
Config commands — manage runtime settings

  config load [<path>]
    Load settings from a YAML file. Defaults to <config_dir>/settings.yaml.

  config save [<path>]
    Save current settings to a YAML file.

  config add <key> <value>
    Set a configuration value. Supported keys:
      project_root          — default working directory for new agents
      max_retries           — maximum retry count (u32)
      health_check_interval — health check interval in ms (u64)
      heartbeat_timeout     — heartbeat timeout in ms (u64)
      message_timeout       — message delivery timeout in ms (u64)
      escalation_timeout    — escalation timeout in ms (u64)

  config list
    Display all current configuration values in YAML format.",

        "project" => "\
Project commands — manage registered project folders

  project add <name> <path>
    Register a project folder. Also creates a root task node for
    the project in the task tree.

  project remove <name>
    Remove a registered project. Does not delete files on disk.

  project list [--json]
    List all registered projects with their paths.

  project scan <name>
    Scan a project folder for task subfolders. Queues discovery
    of spec files and execution state.",

        "layout" => "\
Layout commands — manage tmux sessions and pane layout

  layout row <session> [--percent <n>]
    Split the session with a horizontal divider. Default 50%.

  layout column <session> [--percent <n>]
    Split the session with a vertical divider. Default 50%.

  layout merge <session>
    Merge all panes in a session into a single pane.

  layout place <pane> <agent>
    Place an agent into a specific tmux pane (e.g. %3).

  layout capture <session>
    Capture the current content of all panes in a session.

  layout session <name> [--cwd <path>]
    Create a new tmux session. Uses project_root as default cwd.",

        "client" => "\
Client commands — navigate between client views

  client next
    Switch to the next client view.

  client prev
    Switch to the previous client view.",

        "tell" | "messaging" => "\
Messaging commands — communicate with agents

  tell <agent> <text...>
    Send a text message to an agent. The message is queued in the
    message store and a SendKeys action is emitted to deliver it
    via tmux.

  interrupt <agent> [text...]
    Send Ctrl-C to an agent. If text is provided, it is sent after
    the interrupt signal. Useful for cancelling long-running operations
    and issuing new instructions.",


        "rig" => "\
Rig commands — manage remote worker hosts

  rig init <host> [--name <n>]
    Initialize a remote host for use as a worker rig. Verifies SSH
    connectivity and sets up the remote environment. If --name is omitted,
    a name is derived from the host.

  rig push <folder> [-r <remote>]
    Push a local folder to the remote via rsync. Uses the default remote
    unless -r is specified.

  rig pull <folder> [-r <remote>]
    Pull results from the remote folder back to local via rsync.

  rig status [-r <remote>]
    Show the current status of the remote (running tasks, load, etc.).

  rig health [-r <remote>]
    Perform an SSH health check on the remote.

  rig stop [-r <remote>]
    Stop all running operations on the remote.

  rig list
    List all configured remote hosts with their status.

  rig default [<name>]
    Show the current default remote, or set it to <name>.",

        "diagnosis" => "\
Diagnosis commands — self-diagnosis and monitoring analytics

  diagnosis report
    Generate a comprehensive self-diagnosis report covering signal
    reliability, intervention effectiveness, and threshold health.

  diagnosis reliability [<signal>]
    Show reliability statistics for heartbeat signals. Optionally
    filter by a specific signal name.

  diagnosis effectiveness [<signal>]
    Show intervention effectiveness metrics. Optionally filter by
    a specific signal or intervention type.

  diagnosis thresholds
    Display current adaptive threshold values and their adjustment
    history.

  diagnosis events [--limit <n>]
    List recent intervention events. Defaults to the last 20 events.
    Use --limit to control how many are shown.",

        "history" => "\
History commands — configuration snapshot management

  history list [--limit <n>]
    List available configuration snapshots. Defaults to showing the
    most recent entries. Use --limit to control the count.

  history show <id>
    Display the contents of a specific snapshot.

  history diff <from> [<to>]
    Show the differences between two snapshots. If <to> is omitted,
    diffs against the current configuration.

  history restore <id>
    Restore configuration from a previous snapshot.

  history snapshot
    Take a snapshot of the current configuration immediately.

  history prune
    Remove old snapshots according to the retention policy.",

        "watch" => "\
Watch command — stream state changes

  watch [--since <ms>] [--timeout <ms>]
    Stream state change events to stdout as they occur. Use --since
    to replay events from a given epoch-ms timestamp. Use --timeout
    to limit how long the stream stays open (default: indefinite).",

        "daemon" => "\
Daemon commands — manage the CMX daemon process

  daemon run
    Start the CMX daemon in the foreground. Opens the Unix socket,
    begins accepting commands, and runs the convergence loop.

  daemon stop
    Send a stop signal to the running daemon. The daemon will finish
    in-flight commands and shut down gracefully.",

        "pool" => "\
Pool commands — manage worker agent pools

  pool list
    List all configured worker pools with their roles and sizes.

  pool status <role>
    Show detailed status for the pool with the given role, including
    current agent count, assigned tasks, and health.

  pool set <role> <size> [--path <p>]
    Create or update a worker pool. Sets the target size (number of
    agents). Use --path to specify the working directory for agents
    in the pool.

  pool remove <role>
    Remove a worker pool. Kills all agents in the pool.",

        _ => return None,
    };
    Some(text.into())
}


/// Detailed help for a specific command.
fn command_help(command: &str) -> Option<String> {
    let text = match command {
        "status" => "\
cmx status — show system summary

Usage: cmx status [--json]

Displays a one-line summary of the system state:
  agents: N, tasks: N, projects: N, pending messages: N

Use --json for machine-readable JSON output.
No other arguments required.",

        "view" => "\
cmx view — look up an entity by name

Usage: cmx view <name>

Searches for the given name across agents, tasks, and projects
(in that order). Returns the first match as pretty-printed JSON.

Examples:
  cmx view worker1     # show agent details
  cmx view CMX         # show task details
  cmx view myproject   # show project details",

        "help" => "\
cmx help — show help information

Usage: cmx help [topic]

With no topic, shows an overview of all available commands.
With a topic, shows detailed help:

  cmx help              # overview
  cmx help agent        # all agent commands
  cmx help agent.new    # detailed help for agent.new
  cmx help task         # all task commands
  cmx help config       # all config commands",

        "agent.new" => "\
cmx agent new — create a new agent

Usage: cmx agent new <role> [--name <n>] [--path <p>] [--type <t>]

Arguments:
  <role>       Role string (e.g. worker, pilot, pm, curator)

Flags:
  --name <n>   Agent name. Auto-generated if omitted (worker1, worker2, etc.)
  --path <p>   Working directory. Defaults to project_root from settings.
  --type <t>   Agent type: claude (default), console, or ssh.

Examples:
  cmx agent new worker
  cmx agent new pilot --name my-pilot
  cmx agent new worker --name w1 --path /projects/cmx --type ssh

Side effects:
  Emits a CreateAgent action for infrastructure to spawn the agent.",

        "agent.kill" => "\
cmx agent kill — remove an agent

Usage: cmx agent kill <name>

Removes the named agent from the registry and emits a KillAgent action.
Fails if the agent does not exist.",

        "agent.restart" => "\
cmx agent restart — restart an agent

Usage: cmx agent restart <name>

Kills and re-creates the agent with the same role, name, and path.
Resets status to idle and health to unknown. Emits KillAgent + CreateAgent.",

        "agent.assign" => "\
cmx agent assign — assign an agent to a task

Usage: cmx agent assign <name> <task>

Sets the agent's task field and the task's agent field. Also marks
the task as in_progress. Emits an UpdateAssignment action.",

        "agent.unassign" => "\
cmx agent unassign — remove task assignment

Usage: cmx agent unassign <name>

Clears the agent's task field. Also clears the task's agent field
if one was assigned. Emits an UpdateAssignment action with task=null.",

        "agent.status" => "\
cmx agent status — update status notes

Usage: cmx agent status <name> [notes...]

Sets the agent's free-text status notes. Multiple words are joined.

Examples:
  cmx agent status w1 compiling
  cmx agent status w1 running cargo test",

        "agent.list" => "\
cmx agent list — list all agents

Usage: cmx agent list [--json]

Displays agents in a table with columns:
  NAME  ROLE  STATUS  HEALTH  TASK

Use --json for JSON array output.",

        "task.list" => "\
cmx task list — list all tasks

Usage: cmx task list [<project>] [--json]

Lists all tasks in the task tree with indentation for depth.
Optionally filter by project name prefix.

Columns: ID  TITLE  STATUS  AGENT

Use --json for JSON array output.",

        "task.get" => "\
cmx task get — show task details

Usage: cmx task get <id>

Returns the task as pretty-printed JSON, including all fields:
id, title, source, status, result, agent, children, spec_path.",

        "task.set" => "\
cmx task set — update task fields

Usage: cmx task set <id> key=value [key=value ...]

Update one or more fields on a task.

Supported fields:
  status   — pending, in_progress, completed, failed, paused, cancelled
  title    — task title text
  result   — result/output text
  agent    — agent name, or '-' to clear

Examples:
  cmx task set T1 status=in_progress
  cmx task set T1 status=completed title=Done result='all tests passed'",

        "task.check" => "\
cmx task check — mark task completed

Usage: cmx task check <id>

Shorthand for: cmx task set <id> status=completed",

        "task.uncheck" => "\
cmx task uncheck — mark task pending

Usage: cmx task uncheck <id>

Shorthand for: cmx task set <id> status=pending",

        "config.load" => "\
cmx config load — load settings from file

Usage: cmx config load [<path>]

Loads settings from a YAML file. If no path is given, defaults to
<config_dir>/settings.yaml.",

        "config.save" => "\
cmx config save — save settings to file

Usage: cmx config save [<path>]

Saves current runtime settings to a YAML file.",

        "config.add" => "\
cmx config add — set a configuration value

Usage: cmx config add <key> <value>

Supported keys: project_root, max_retries, health_check_interval,
heartbeat_timeout, message_timeout, escalation_timeout.

Numeric keys are validated on parse.",

        "config.list" => "\
cmx config list — show all settings

Usage: cmx config list

Displays all configuration values in YAML format.",

        "project.add" => "\
cmx project add — register a project

Usage: cmx project add <name> <path>

Registers a project folder and creates a root task node in the task tree.",

        "project.remove" => "\
cmx project remove — remove a project

Usage: cmx project remove <name>

Removes the project from the folder registry. Does not delete files.",

        "project.list" => "\
cmx project list — list projects

Usage: cmx project list [--json]

Displays registered projects with their paths.",

        "project.scan" => "\
cmx project scan — scan project folder

Usage: cmx project scan <name>

Scans the project folder for task subfolders.",

        "tell" => "\
cmx tell — send a message to an agent

Usage: cmx tell <agent> <text...>

Queues a message for the agent and emits a SendKeys action to deliver it.
The agent must exist.",

        "interrupt" => "\
cmx interrupt — interrupt an agent

Usage: cmx interrupt <agent> [text...]

Sends Ctrl-C to the agent. If text is provided, sends it after the interrupt.

Examples:
  cmx interrupt w1              # just Ctrl-C
  cmx interrupt w1 stop now     # Ctrl-C then 'stop now'",

        "layout.row" => "\
cmx layout row — horizontal split

Usage: cmx layout row <session> [--percent <n>]

Splits the session with a horizontal divider. Default split is 50%.",

        "layout.column" => "\
cmx layout column — vertical split

Usage: cmx layout column <session> [--percent <n>]

Splits the session with a vertical divider. Default split is 50%.",

        "layout.merge" => "\
cmx layout merge — merge panes

Usage: cmx layout merge <session>

Merges all panes in the session into a single pane.",

        "layout.place" => "\
cmx layout place — place agent in pane

Usage: cmx layout place <pane> <agent>

Places an agent into a specific tmux pane ID (e.g. %3).",

        "layout.capture" => "\
cmx layout capture — capture pane contents

Usage: cmx layout capture <session>

Captures the current content of all panes in the session.",

        "layout.session" => "\
cmx layout session — create tmux session

Usage: cmx layout session <name> [--cwd <path>]

Creates a new tmux session with the given name.",

        "client.next" => "\
cmx client next — switch to next view

Usage: cmx client next",

        "client.prev" => "\
cmx client prev — switch to previous view

Usage: cmx client prev",


        // --- Rig commands ---

        "rig.init" => "\
cmx rig init — initialize a remote host

Usage: cmx rig init <host> [--name <n>]

Arguments:
  <host>       SSH host string (e.g. user@host:port or IP address)

Flags:
  --name <n>   Name for this remote. Derived from host if omitted.

Verifies SSH connectivity and sets up the remote environment for
use as a worker rig.",

        "rig.push" => "\
cmx rig push — push code to remote

Usage: cmx rig push <folder> [-r <remote>]

Pushes a local folder to the remote host via rsync. Uses the default
remote unless -r is specified.

Examples:
  cmx rig push ./src
  cmx rig push ./project -r gpu1",

        "rig.pull" => "\
cmx rig pull — pull results from remote

Usage: cmx rig pull <folder> [-r <remote>]

Pulls a remote folder back to local via rsync. Uses the default
remote unless -r is specified.",

        "rig.status" => "\
cmx rig status — show remote status

Usage: cmx rig status [-r <remote>]

Displays the current status of the remote: running tasks, load,
disk usage, and connectivity state.",

        "rig.health" => "\
cmx rig health — health check remote

Usage: cmx rig health [-r <remote>]

Performs an SSH connectivity check on the remote. Reports latency
and connection status.",

        "rig.stop" => "\
cmx rig stop — stop remote operations

Usage: cmx rig stop [-r <remote>]

Stops all running operations on the remote host. Sends termination
signals to active processes.",

        "rig.list" => "\
cmx rig list — list configured remotes

Usage: cmx rig list

Displays all configured remote hosts with their names, addresses,
and current status.",

        "rig.default" => "\
cmx rig default — show or set default remote

Usage: cmx rig default [<name>]

With no argument, shows the current default remote name.
With a name, sets that remote as the default for -r flags.",

        // --- Diagnosis commands ---

        "diagnosis.report" => "\
cmx diagnosis report — generate self-diagnosis report

Usage: cmx diagnosis report

Generates a comprehensive report covering signal reliability,
intervention effectiveness, adaptive threshold health, and
recent events.",

        "diagnosis.reliability" => "\
cmx diagnosis reliability — signal reliability statistics

Usage: cmx diagnosis reliability [<signal>]

Shows reliability metrics for heartbeat signals: hit rate, miss rate,
false-positive rate. Optionally filter to a single signal name.",

        "diagnosis.effectiveness" => "\
cmx diagnosis effectiveness — intervention effectiveness

Usage: cmx diagnosis effectiveness [<signal>]

Shows how effective past interventions have been: success rate,
average recovery time, repeat failure rate. Optionally filter
by signal or intervention type.",

        "diagnosis.thresholds" => "\
cmx diagnosis thresholds — show adaptive thresholds

Usage: cmx diagnosis thresholds

Displays all adaptive threshold values, their current settings,
and recent adjustment history.",

        "diagnosis.events" => "\
cmx diagnosis events — list recent intervention events

Usage: cmx diagnosis events [--limit <n>]

Lists recent intervention events with timestamps, signal names,
actions taken, and outcomes. Defaults to the last 20 events.",

        // --- History commands ---

        "history.list" => "\
cmx history list — list configuration snapshots

Usage: cmx history list [--limit <n>]

Lists available configuration snapshots with IDs and timestamps.
Use --limit to control how many are shown.",

        "history.show" => "\
cmx history show — show a snapshot

Usage: cmx history show <id>

Displays the full contents of the specified configuration snapshot.",

        "history.diff" => "\
cmx history diff — diff two snapshots

Usage: cmx history diff <from> [<to>]

Shows differences between two snapshots. If <to> is omitted,
diffs the snapshot against the current live configuration.",

        "history.restore" => "\
cmx history restore — restore a snapshot

Usage: cmx history restore <id>

Restores configuration from a previous snapshot. The current
configuration is automatically snapshotted before restoration.",

        "history.snapshot" => "\
cmx history snapshot — take a snapshot now

Usage: cmx history snapshot

Takes an immediate snapshot of the current configuration state.",

        "history.prune" => "\
cmx history prune — prune old snapshots

Usage: cmx history prune

Removes old snapshots according to the configured retention policy.",

        // --- Watch command ---

        "watch" => "\
cmx watch — stream state changes

Usage: cmx watch [--since <ms>] [--timeout <ms>]

Streams state change events to stdout as newline-delimited JSON.

Flags:
  --since <ms>    Replay events from this epoch-ms timestamp
  --timeout <ms>  Close the stream after this many milliseconds

Without --since, only new events are streamed. Without --timeout,
the stream stays open until interrupted.",

        // --- Daemon commands ---

        "daemon.run" => "\
cmx daemon run — start daemon in foreground

Usage: cmx daemon run

Starts the CMX daemon in the foreground. Opens the Unix socket
at the configured path, begins accepting client connections, and
runs the convergence loop. Logs to stdout.",

        "daemon.stop" => "\
cmx daemon stop — stop running daemon

Usage: cmx daemon stop

Sends a stop command to the running CMX daemon via the Unix socket.
The daemon finishes in-flight commands and shuts down gracefully.",

        // --- Pool commands ---

        "pool.list" => "\
cmx pool list — list all worker pools

Usage: cmx pool list

Displays all configured worker pools with their roles, target sizes,
and current agent counts.",

        "pool.status" => "\
cmx pool status — show pool status for a role

Usage: cmx pool status <role>

Shows detailed status for the pool with the given role, including
individual agent states, assigned tasks, and health.",

        "pool.set" => "\
cmx pool set — create or update a pool

Usage: cmx pool set <role> <size> [--path <p>]

Creates a new worker pool or updates an existing one. Sets the target
number of agents for the given role.

Flags:
  --path <p>   Working directory for agents in the pool.

Examples:
  cmx pool set worker 4
  cmx pool set builder 2 --path /projects/build",

        "pool.remove" => "\
cmx pool remove — remove a pool

Usage: cmx pool remove <role>

Removes the worker pool for the given role. All agents in the pool
are killed.",

        _ => return None,
    };
    Some(text.into())
}


// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overview_contains_all_groups() {
        let text = help_text(None);
        assert!(text.contains("Agent commands:"));
        assert!(text.contains("Task commands:"));
        assert!(text.contains("Config commands:"));
        assert!(text.contains("Project commands:"));
        assert!(text.contains("Layout commands:"));
        assert!(text.contains("Client commands:"));
        assert!(text.contains("Messaging commands:"));
        assert!(text.contains("Rig commands"));
        assert!(text.contains("Diagnosis commands:"));
        assert!(text.contains("History commands:"));
        assert!(text.contains("Watch command:"));
        assert!(text.contains("Daemon commands:"));
        assert!(text.contains("Pool commands:"));
    }

    #[test]
    fn overview_lists_status() {
        let text = help_text(None);
        assert!(text.contains("status"));
        assert!(text.contains("Show system summary"));
    }

    #[test]
    fn group_help_agent() {
        let text = help_text(Some("agent"));
        assert!(text.contains("agent new"));
        assert!(text.contains("agent kill"));
        assert!(text.contains("agent restart"));
        assert!(text.contains("agent assign"));
        assert!(text.contains("agent unassign"));
        assert!(text.contains("agent status"));
        assert!(text.contains("agent list"));
    }

    #[test]
    fn group_help_task() {
        let text = help_text(Some("task"));
        assert!(text.contains("task list"));
        assert!(text.contains("task get"));
        assert!(text.contains("task set"));
        assert!(text.contains("task check"));
        assert!(text.contains("task uncheck"));
    }

    #[test]
    fn group_help_config() {
        let text = help_text(Some("config"));
        assert!(text.contains("config load"));
        assert!(text.contains("config save"));
        assert!(text.contains("config add"));
        assert!(text.contains("config list"));
        assert!(text.contains("max_retries"));
    }

    #[test]
    fn group_help_project() {
        let text = help_text(Some("project"));
        assert!(text.contains("project add"));
        assert!(text.contains("project remove"));
        assert!(text.contains("project list"));
        assert!(text.contains("project scan"));
    }

    #[test]
    fn group_help_layout() {
        let text = help_text(Some("layout"));
        assert!(text.contains("layout row"));
        assert!(text.contains("layout column"));
        assert!(text.contains("layout merge"));
        assert!(text.contains("layout place"));
        assert!(text.contains("layout capture"));
        assert!(text.contains("layout session"));
    }

    #[test]
    fn group_help_client() {
        let text = help_text(Some("client"));
        assert!(text.contains("client next"));
        assert!(text.contains("client prev"));
    }

    #[test]
    fn group_help_messaging() {
        // "tell" matches as a command first, so use "messaging" for the group
        let text = help_text(Some("messaging"));
        assert!(text.contains("tell"));
        assert!(text.contains("interrupt"));
    }

    #[test]
    fn command_help_agent_new() {
        let text = help_text(Some("agent.new"));
        assert!(text.contains("Usage:"));
        assert!(text.contains("--name"));
        assert!(text.contains("--path"));
        assert!(text.contains("--type"));
        assert!(text.contains("CreateAgent"));
    }

    #[test]
    fn command_help_task_set() {
        let text = help_text(Some("task.set"));
        assert!(text.contains("Usage:"));
        assert!(text.contains("key=value"));
        assert!(text.contains("status"));
        assert!(text.contains("in_progress"));
    }

    #[test]
    fn command_help_tell() {
        let _text = help_text(Some("tell"));
        // "tell" matches as group first (messaging group)
        let text = command_help("tell").unwrap();
        assert!(text.contains("Usage:"));
        assert!(text.contains("SendKeys"));
    }

    #[test]
    fn command_help_status() {
        let text = help_text(Some("status"));
        assert!(text.contains("Usage: cmx status"));
        assert!(text.contains("--json"));
    }

    #[test]
    fn command_help_view() {
        let text = help_text(Some("view"));
        assert!(text.contains("Usage: cmx view"));
    }

    #[test]
    fn command_help_help() {
        let text = help_text(Some("help"));
        assert!(text.contains("Usage: cmx help"));
    }

    #[test]
    fn command_help_all_commands_covered() {
        let commands = vec![
            "status", "view", "help",
            "agent.new", "agent.kill", "agent.restart",
            "agent.assign", "agent.unassign", "agent.status", "agent.list",
            "task.list", "task.get", "task.set", "task.check", "task.uncheck",
            "config.load", "config.save", "config.add", "config.list",
            "project.add", "project.remove", "project.list", "project.scan",
            "tell", "interrupt",
            "layout.row", "layout.column", "layout.merge",
            "layout.place", "layout.capture", "layout.session",
            "client.next", "client.prev",
            "rig.init", "rig.push", "rig.pull", "rig.status",
            "rig.health", "rig.stop", "rig.list", "rig.default",
            "diagnosis.report", "diagnosis.reliability", "diagnosis.effectiveness",
            "diagnosis.thresholds", "diagnosis.events",
            "history.list", "history.show", "history.diff",
            "history.restore", "history.snapshot", "history.prune",
            "watch",
            "daemon.run", "daemon.stop",
            "pool.list", "pool.status", "pool.set", "pool.remove",
        ];
        for cmd in commands {
            assert!(
                command_help(cmd).is_some(),
                "Missing command help for: {}",
                cmd
            );
        }
    }


    #[test]
    fn group_help_rig() {
        let text = help_text(Some("rig"));
        assert!(text.contains("rig init"));
        assert!(text.contains("rig push"));
        assert!(text.contains("rig pull"));
        assert!(text.contains("rig status"));
        assert!(text.contains("rig health"));
        assert!(text.contains("rig stop"));
        assert!(text.contains("rig list"));
        assert!(text.contains("rig default"));
    }

    #[test]
    fn group_help_diagnosis() {
        let text = help_text(Some("diagnosis"));
        assert!(text.contains("diagnosis report"));
        assert!(text.contains("diagnosis reliability"));
        assert!(text.contains("diagnosis effectiveness"));
        assert!(text.contains("diagnosis thresholds"));
        assert!(text.contains("diagnosis events"));
    }

    #[test]
    fn group_help_history() {
        let text = help_text(Some("history"));
        assert!(text.contains("history list"));
        assert!(text.contains("history show"));
        assert!(text.contains("history diff"));
        assert!(text.contains("history restore"));
        assert!(text.contains("history snapshot"));
        assert!(text.contains("history prune"));
    }

    #[test]
    fn group_help_watch() {
        let text = help_text(Some("watch"));
        assert!(text.contains("watch"));
        assert!(text.contains("--since"));
        assert!(text.contains("--timeout"));
    }

    #[test]
    fn group_help_daemon() {
        let text = help_text(Some("daemon"));
        assert!(text.contains("daemon run"));
        assert!(text.contains("daemon stop"));
    }

    #[test]
    fn group_help_pool() {
        let text = help_text(Some("pool"));
        assert!(text.contains("pool list"));
        assert!(text.contains("pool status"));
        assert!(text.contains("pool set"));
        assert!(text.contains("pool remove"));
    }

    #[test]
    fn unknown_topic() {
        let text = help_text(Some("bogus"));
        assert!(text.contains("Unknown help topic"));
    }

    #[test]
    fn group_help_unknown() {
        assert!(group_help("nonexistent").is_none());
    }
}
