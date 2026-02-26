use crate::command::Command;


/// Parse CLI arguments into a typed Command enum.
///
/// The first argument is expected to be the subcommand (e.g., "status",
/// "agent", "task"). Multi-word subcommands map to enum variants
/// (e.g., "agent new" becomes `Command::AgentNew`).
///
/// Arguments are expected WITHOUT the program name (i.e., `args` should
/// be `["status"]`, not `["cmx", "status"]`).
pub fn parse_args(args: &[&str]) -> Result<Command, String> {
    if args.is_empty() {
        return Err("No command specified. Run 'skd help' for usage.".into());
    }

    match args[0] {
        "status" => parse_status(args),
        "view" => parse_view(args),
        "help" => parse_help(args),
        "agent" => parse_agent(args),
        "task" => parse_task(args),
        "config" => parse_config(args),
        "project" => parse_project(args),
        "roadmap" => parse_roadmap(args),
        "pool" => parse_pool(args),
        "tell" => parse_tell(args),
        "interrupt" => parse_interrupt(args),
        "layout" => parse_layout(args),
        "client" => parse_client(args),
        "rig" => parse_rig(args),
        "diagnosis" => parse_diagnosis(args),
        "history" => parse_history(args),
        "daemon" => parse_daemon(args),
        "watch" => parse_watch(args),
        "tui" => Ok(Command::Tui),
        _ => Err(format!("Unknown command: '{}'", args[0])),
    }
}


// ---------------------------------------------------------------------------
// Sub-parsers
// ---------------------------------------------------------------------------

/// `cmx view <name>`
fn parse_view(args: &[&str]) -> Result<Command, String> {
    if args.len() < 2 {
        return Err("Usage: cmx view <name>".into());
    }
    Ok(Command::View {
        name: args[1].into(),
    })
}

/// `cmx help [topic]`
fn parse_help(args: &[&str]) -> Result<Command, String> {
    let topic = if args.len() > 1 {
        Some(args[1..].join(" "))
    } else {
        None
    };
    Ok(Command::Help { topic })
}

/// `cmx agent <subcommand> ...`
fn parse_agent(args: &[&str]) -> Result<Command, String> {
    if args.len() < 2 {
        return Err("Usage: cmx agent <new|kill|restart|assign|unassign|status|list>".into());
    }
    match args[1] {
        "new" => parse_agent_new(args),
        "kill" => parse_agent_kill(args),
        "restart" => parse_agent_restart(args),
        "assign" => parse_agent_assign(args),
        "unassign" => parse_agent_unassign(args),
        "status" => parse_agent_status(args),
        "list" => parse_agent_list(args),
        _ => Err(format!("Unknown agent subcommand: '{}'", args[1])),
    }
}

/// `cmx agent new <role> [--path <path>] [--name <name>] [--type <type>]`
fn parse_agent_new(args: &[&str]) -> Result<Command, String> {
    if args.len() < 3 {
        return Err("Usage: cmx agent new <role> [--path <p>] [--name <n>] [--type <t>]".into());
    }
    let role = args[2].to_string();
    let mut name = None;
    let mut path = None;
    let mut agent_type = None;

    let rest = &args[3..];
    let mut i = 0;
    while i < rest.len() {
        match rest[i] {
            "--path" => {
                i += 1;
                path = Some(take_arg(rest, i, "--path")?);
            }
            "--name" => {
                i += 1;
                name = Some(take_arg(rest, i, "--name")?);
            }
            "--type" => {
                i += 1;
                agent_type = Some(take_arg(rest, i, "--type")?);
            }
            other => return Err(format!("Unknown flag for agent new: '{}'", other)),
        }
        i += 1;
    }
    Ok(Command::AgentNew { role, name, path, agent_type })
}

/// `cmx agent kill <name>`
fn parse_agent_kill(args: &[&str]) -> Result<Command, String> {
    if args.len() < 3 {
        return Err("Usage: cmx agent kill <name>".into());
    }
    Ok(Command::AgentKill {
        name: args[2].into(),
    })
}

/// `cmx agent restart <name>`
fn parse_agent_restart(args: &[&str]) -> Result<Command, String> {
    if args.len() < 3 {
        return Err("Usage: cmx agent restart <name>".into());
    }
    Ok(Command::AgentRestart {
        name: args[2].into(),
    })
}

/// `cmx agent assign <name> <task>`
fn parse_agent_assign(args: &[&str]) -> Result<Command, String> {
    if args.len() < 4 {
        return Err("Usage: cmx agent assign <name> <task>".into());
    }
    Ok(Command::AgentAssign {
        name: args[2].into(),
        task: args[3].into(),
    })
}

/// `cmx agent unassign <name>`
fn parse_agent_unassign(args: &[&str]) -> Result<Command, String> {
    if args.len() < 3 {
        return Err("Usage: cmx agent unassign <name>".into());
    }
    Ok(Command::AgentUnassign {
        name: args[2].into(),
    })
}

/// `cmx agent status <name> [<notes...>]`
fn parse_agent_status(args: &[&str]) -> Result<Command, String> {
    if args.len() < 3 {
        return Err("Usage: cmx agent status <name> [<notes...>]".into());
    }
    let name = args[2].to_string();
    let notes = if args.len() > 3 {
        Some(args[3..].join(" "))
    } else {
        None
    };
    Ok(Command::AgentStatus { name, notes })
}

/// `cmx agent list [--json]`
fn parse_agent_list(args: &[&str]) -> Result<Command, String> {
    let format = if args.contains(&"--json") {
        Some("json".into())
    } else {
        None
    };
    Ok(Command::AgentList { format })
}

/// `cmx task <subcommand> ...`
fn parse_task(args: &[&str]) -> Result<Command, String> {
    if args.len() < 2 {
        return Err("Usage: cmx task <list|get|set|check|uncheck>".into());
    }
    match args[1] {
        "list" => parse_task_list(args),
        "get" => parse_task_get(args),
        "set" => parse_task_set(args),
        "check" => parse_task_check(args),
        "uncheck" => parse_task_uncheck(args),
        _ => Err(format!("Unknown task subcommand: '{}'", args[1])),
    }
}

/// `cmx task list [<project>] [--json]`
fn parse_task_list(args: &[&str]) -> Result<Command, String> {
    let mut format = None;
    let mut project = None;
    let mut i = 2;
    while i < args.len() {
        match args[i] {
            "--json" => {
                format = Some("json".into());
            }
            other if !other.starts_with("--") => {
                project = Some(other.into());
            }
            other => return Err(format!("Unknown flag for task list: '{}'", other)),
        }
        i += 1;
    }
    Ok(Command::TaskList { format, project })
}

/// `cmx task get <id>`
fn parse_task_get(args: &[&str]) -> Result<Command, String> {
    if args.len() < 3 {
        return Err("Usage: cmx task get <id>".into());
    }
    Ok(Command::TaskGet {
        id: args[2].into(),
    })
}

/// `cmx task set <id> key=value ...`
fn parse_task_set(args: &[&str]) -> Result<Command, String> {
    if args.len() < 4 {
        return Err("Usage: cmx task set <id> key=value ...".into());
    }
    let id = args[2].to_string();
    let mut status = None;
    let mut title = None;
    let mut result = None;
    let mut agent = None;

    for kv in &args[3..] {
        if let Some(eq_pos) = kv.find('=') {
            let key = &kv[..eq_pos];
            let value = kv[eq_pos + 1..].to_string();
            match key {
                "status" => status = Some(value),
                "title" => title = Some(value),
                "result" => result = Some(value),
                "agent" => agent = Some(value),
                _ => return Err(format!("Unknown task field: '{}'", key)),
            }
        } else {
            return Err(format!("Expected key=value, got: '{}'", kv));
        }
    }
    Ok(Command::TaskSet { id, status, title, result, agent })
}

/// `cmx task check <id>`
fn parse_task_check(args: &[&str]) -> Result<Command, String> {
    if args.len() < 3 {
        return Err("Usage: cmx task check <id>".into());
    }
    Ok(Command::TaskCheck {
        id: args[2].into(),
    })
}

/// `cmx task uncheck <id>`
fn parse_task_uncheck(args: &[&str]) -> Result<Command, String> {
    if args.len() < 3 {
        return Err("Usage: cmx task uncheck <id>".into());
    }
    Ok(Command::TaskUncheck {
        id: args[2].into(),
    })
}

/// `cmx config <load|save|add|list>`
fn parse_config(args: &[&str]) -> Result<Command, String> {
    if args.len() < 2 {
        return Err("Usage: cmx config <load|save|add|list>".into());
    }
    match args[1] {
        "load" => {
            let path = if args.len() > 2 {
                Some(args[2].into())
            } else {
                None
            };
            Ok(Command::ConfigLoad { path })
        }
        "save" => {
            let path = if args.len() > 2 {
                Some(args[2].into())
            } else {
                None
            };
            Ok(Command::ConfigSave { path })
        }
        "add" => {
            if args.len() < 4 {
                return Err("Usage: cmx config add <key> <value>".into());
            }
            Ok(Command::ConfigAdd {
                key: args[2].into(),
                value: args[3..].join(" "),
            })
        }
        "list" => Ok(Command::ConfigList),
        _ => Err(format!("Unknown config subcommand: '{}'", args[1])),
    }
}

/// `cmx project <add|remove|list|scan>`
fn parse_project(args: &[&str]) -> Result<Command, String> {
    if args.len() < 2 {
        return Err("Usage: cmx project <add|remove|list|scan>".into());
    }
    match args[1] {
        "add" => {
            if args.len() < 4 {
                return Err("Usage: cmx project add <name> <path>".into());
            }
            Ok(Command::ProjectAdd {
                name: args[2].into(),
                path: args[3].into(),
            })
        }
        "remove" => {
            if args.len() < 3 {
                return Err("Usage: cmx project remove <name>".into());
            }
            Ok(Command::ProjectRemove {
                name: args[2].into(),
            })
        }
        "list" => {
            let format = if args.contains(&"--json") {
                Some("json".into())
            } else {
                None
            };
            Ok(Command::ProjectList { format })
        }
        "scan" => {
            if args.len() < 3 {
                return Err("Usage: cmx project scan <name>".into());
            }
            Ok(Command::ProjectScan {
                name: args[2].into(),
            })
        }
        _ => Err(format!("Unknown project subcommand: '{}'", args[1])),
    }
}

/// `cmx roadmap <load>`
fn parse_roadmap(args: &[&str]) -> Result<Command, String> {
    if args.len() < 2 {
        return Err("Usage: cmx roadmap <load> <path>".into());
    }
    match args[1] {
        "load" => {
            if args.len() < 3 {
                return Err("Usage: cmx roadmap load <path>".into());
            }
            Ok(Command::RoadmapLoad {
                path: args[2].into(),
            })
        }
        _ => Err(format!("Unknown roadmap subcommand: '{}'", args[1])),
    }
}

/// `cmx pool <list|status|set|remove>`
fn parse_pool(args: &[&str]) -> Result<Command, String> {
    if args.len() < 2 {
        return Err("Usage: cmx pool <list|status|set|remove>".into());
    }
    match args[1] {
        "list" => Ok(Command::PoolList),
        "status" => {
            if args.len() < 3 {
                return Err("Usage: cmx pool status <role>".into());
            }
            Ok(Command::PoolStatus {
                role: args[2].into(),
            })
        }
        "set" => {
            if args.len() < 4 {
                return Err("Usage: cmx pool set <role> <size> [--path <path>]".into());
            }
            let role = args[2].to_string();
            let size: u32 = args[3]
                .parse()
                .map_err(|_| format!("Invalid pool size: '{}'", args[3]))?;
            let mut path = None;
            let rest = &args[4..];
            let mut i = 0;
            while i < rest.len() {
                if rest[i] == "--path" {
                    i += 1;
                    path = Some(take_arg(rest, i, "--path")?);
                }
                i += 1;
            }
            Ok(Command::PoolSet { role, size, path })
        }
        "remove" => {
            if args.len() < 3 {
                return Err("Usage: cmx pool remove <role>".into());
            }
            Ok(Command::PoolRemove {
                role: args[2].into(),
            })
        }
        _ => Err(format!("Unknown pool subcommand: '{}'", args[1])),
    }
}

/// `cmx tell <agent> <text...>`
fn parse_tell(args: &[&str]) -> Result<Command, String> {
    if args.len() < 3 {
        return Err("Usage: cmx tell <agent> <text...>".into());
    }
    Ok(Command::Tell {
        agent: args[1].into(),
        text: args[2..].join(" "),
    })
}

/// `cmx interrupt <agent> [<text...>]`
fn parse_interrupt(args: &[&str]) -> Result<Command, String> {
    if args.len() < 2 {
        return Err("Usage: cmx interrupt <agent> [<text...>]".into());
    }
    let agent = args[1].to_string();
    let text = if args.len() > 2 {
        Some(args[2..].join(" "))
    } else {
        None
    };
    Ok(Command::Interrupt { agent, text })
}

/// `cmx layout <row|column|merge|place|capture|session>`
fn parse_layout(args: &[&str]) -> Result<Command, String> {
    if args.len() < 2 {
        return Err("Usage: cmx layout <row|column|merge|place|capture|session>".into());
    }
    match args[1] {
        "row" => parse_layout_row(args),
        "column" => parse_layout_column(args),
        "merge" => {
            if args.len() < 3 {
                return Err("Usage: cmx layout merge <session>".into());
            }
            Ok(Command::LayoutMerge {
                session: args[2].into(),
            })
        }
        "place" => {
            if args.len() < 4 {
                return Err("Usage: cmx layout place <pane> <agent>".into());
            }
            Ok(Command::LayoutPlace {
                pane: args[2].into(),
                agent: args[3].into(),
            })
        }
        "capture" => {
            if args.len() < 3 {
                return Err("Usage: cmx layout capture <session>".into());
            }
            Ok(Command::LayoutCapture {
                session: args[2].into(),
            })
        }
        "session" => {
            if args.len() < 3 {
                return Err("Usage: cmx layout session <name> [--cwd <path>]".into());
            }
            let name = args[2].to_string();
            let mut cwd = None;
            let rest = &args[3..];
            let mut i = 0;
            while i < rest.len() {
                if rest[i] == "--cwd" {
                    i += 1;
                    cwd = Some(take_arg(rest, i, "--cwd")?);
                }
                i += 1;
            }
            Ok(Command::LayoutSession { name, cwd })
        }
        _ => Err(format!("Unknown layout subcommand: '{}'", args[1])),
    }
}

/// `cmx layout row <session> [--percent <n>]`
fn parse_layout_row(args: &[&str]) -> Result<Command, String> {
    if args.len() < 3 {
        return Err(format!("Usage: cmx {} <session> [--percent <n>]", args[1]));
    }
    let session = args[2].to_string();
    let mut percent = None;
    let rest = &args[3..];
    let mut i = 0;
    while i < rest.len() {
        if rest[i] == "--percent" {
            i += 1;
            percent = Some(take_arg(rest, i, "--percent")?);
        }
        i += 1;
    }
    Ok(Command::LayoutRow { session, percent })
}

/// `cmx layout column <session> [--percent <n>]`
fn parse_layout_column(args: &[&str]) -> Result<Command, String> {
    if args.len() < 3 {
        return Err(format!("Usage: cmx {} <session> [--percent <n>]", args[1]));
    }
    let session = args[2].to_string();
    let mut percent = None;
    let rest = &args[3..];
    let mut i = 0;
    while i < rest.len() {
        if rest[i] == "--percent" {
            i += 1;
            percent = Some(take_arg(rest, i, "--percent")?);
        }
        i += 1;
    }
    Ok(Command::LayoutColumn { session, percent })
}

/// `cmx client <next|prev>`
fn parse_client(args: &[&str]) -> Result<Command, String> {
    if args.len() < 2 {
        return Err("Usage: cmx client <next|prev>".into());
    }
    match args[1] {
        "next" => Ok(Command::ClientNext),
        "prev" => Ok(Command::ClientPrev),
        _ => Err(format!("Unknown client subcommand: '{}'", args[1])),
    }
}


/// `cmx status [--json]`
fn parse_status(args: &[&str]) -> Result<Command, String> {
    let format = if args.contains(&"--json") {
        Some("json".into())
    } else {
        None
    };
    Ok(Command::Status { format })
}

/// `cmx rig <subcommand>`
fn parse_rig(args: &[&str]) -> Result<Command, String> {
    if args.len() < 2 {
        return Err("Usage: cmx rig <init|push|pull|status|health|stop|list|default>".into());
    }
    match args[1] {
        "init" => parse_rig_init(args),
        "push" => parse_rig_push(args),
        "pull" => parse_rig_pull(args),
        "status" => parse_rig_status(args),
        "health" => parse_rig_health(args),
        "stop" => parse_rig_stop(args),
        "list" => Ok(Command::RigList),
        "default" => {
            let name = if args.len() > 2 {
                Some(args[2].into())
            } else {
                None
            };
            Ok(Command::RigDefault { name })
        }
        _ => Err(format!("Unknown rig subcommand: '{}'", args[1])),
    }
}

/// `cmx rig init <host> [--name <name>]`
fn parse_rig_init(args: &[&str]) -> Result<Command, String> {
    if args.len() < 3 {
        return Err("Usage: cmx rig init <host> [--name <name>]".into());
    }
    let host = args[2].to_string();
    let mut name = None;
    let rest = &args[3..];
    let mut i = 0;
    while i < rest.len() {
        if rest[i] == "--name" {
            i += 1;
            name = Some(take_arg(rest, i, "--name")?);
        }
        i += 1;
    }
    Ok(Command::RigInit { host, name })
}

/// `cmx rig push <folder> [--remote <name>]`
fn parse_rig_push(args: &[&str]) -> Result<Command, String> {
    if args.len() < 3 {
        return Err("Usage: cmx rig push <folder> [--remote <name>]".into());
    }
    let folder = args[2].to_string();
    let mut remote = None;
    let rest = &args[3..];
    let mut i = 0;
    while i < rest.len() {
        if rest[i] == "--remote" {
            i += 1;
            remote = Some(take_arg(rest, i, "--remote")?);
        }
        i += 1;
    }
    Ok(Command::RigPush { folder, remote })
}

/// `cmx rig pull <folder> [--remote <name>]`
fn parse_rig_pull(args: &[&str]) -> Result<Command, String> {
    if args.len() < 3 {
        return Err("Usage: cmx rig pull <folder> [--remote <name>]".into());
    }
    let folder = args[2].to_string();
    let mut remote = None;
    let rest = &args[3..];
    let mut i = 0;
    while i < rest.len() {
        if rest[i] == "--remote" {
            i += 1;
            remote = Some(take_arg(rest, i, "--remote")?);
        }
        i += 1;
    }
    Ok(Command::RigPull { folder, remote })
}

/// `cmx rig status [--remote <name>]`
fn parse_rig_status(args: &[&str]) -> Result<Command, String> {
    let mut remote = None;
    let rest = &args[2..];
    let mut i = 0;
    while i < rest.len() {
        if rest[i] == "--remote" {
            i += 1;
            remote = Some(take_arg(rest, i, "--remote")?);
        }
        i += 1;
    }
    Ok(Command::RigStatus { remote })
}

/// `cmx rig health [--remote <name>]`
fn parse_rig_health(args: &[&str]) -> Result<Command, String> {
    let mut remote = None;
    let rest = &args[2..];
    let mut i = 0;
    while i < rest.len() {
        if rest[i] == "--remote" {
            i += 1;
            remote = Some(take_arg(rest, i, "--remote")?);
        }
        i += 1;
    }
    Ok(Command::RigHealth { remote })
}

/// `cmx rig stop [--remote <name>]`
fn parse_rig_stop(args: &[&str]) -> Result<Command, String> {
    let mut remote = None;
    let rest = &args[2..];
    let mut i = 0;
    while i < rest.len() {
        if rest[i] == "--remote" {
            i += 1;
            remote = Some(take_arg(rest, i, "--remote")?);
        }
        i += 1;
    }
    Ok(Command::RigStop { remote })
}

/// `cmx diagnosis <subcommand>`
fn parse_diagnosis(args: &[&str]) -> Result<Command, String> {
    if args.len() < 2 {
        return Err("Usage: cmx diagnosis <report|reliability|effectiveness|thresholds|events>".into());
    }
    match args[1] {
        "report" => Ok(Command::DiagnosisReport),
        "reliability" => parse_diagnosis_reliability(args),
        "effectiveness" => parse_diagnosis_effectiveness(args),
        "thresholds" => parse_diagnosis_thresholds(args),
        "events" => parse_diagnosis_events(args),
        _ => Err(format!("Unknown diagnosis subcommand: '{}'", args[1])),
    }
}

/// `cmx diagnosis reliability [--signal <name>] [--json]`
fn parse_diagnosis_reliability(args: &[&str]) -> Result<Command, String> {
    let mut signal = None;
    let mut format = None;
    let rest = &args[2..];
    let mut i = 0;
    while i < rest.len() {
        match rest[i] {
            "--signal" => {
                i += 1;
                signal = Some(take_arg(rest, i, "--signal")?);
            }
            "--json" => {
                format = Some("json".into());
            }
            other => return Err(format!("Unknown flag for diagnosis reliability: '{}'", other)),
        }
        i += 1;
    }
    Ok(Command::DiagnosisReliability { signal, format })
}

/// `cmx diagnosis effectiveness [--signal <name>] [--json]`
fn parse_diagnosis_effectiveness(args: &[&str]) -> Result<Command, String> {
    let mut signal = None;
    let mut format = None;
    let rest = &args[2..];
    let mut i = 0;
    while i < rest.len() {
        match rest[i] {
            "--signal" => {
                i += 1;
                signal = Some(take_arg(rest, i, "--signal")?);
            }
            "--json" => {
                format = Some("json".into());
            }
            other => return Err(format!("Unknown flag for diagnosis effectiveness: '{}'", other)),
        }
        i += 1;
    }
    Ok(Command::DiagnosisEffectiveness { signal, format })
}

/// `cmx diagnosis thresholds [--json]`
fn parse_diagnosis_thresholds(args: &[&str]) -> Result<Command, String> {
    let format = if args.contains(&"--json") {
        Some("json".into())
    } else {
        None
    };
    Ok(Command::DiagnosisThresholds { format })
}

/// `cmx diagnosis events [--limit <n>] [--json]`
fn parse_diagnosis_events(args: &[&str]) -> Result<Command, String> {
    let mut limit = None;
    let mut format = None;
    let rest = &args[2..];
    let mut i = 0;
    while i < rest.len() {
        match rest[i] {
            "--limit" => {
                i += 1;
                limit = Some(take_arg(rest, i, "--limit")?);
            }
            "--json" => {
                format = Some("json".into());
            }
            other => return Err(format!("Unknown flag for diagnosis events: '{}'", other)),
        }
        i += 1;
    }
    Ok(Command::DiagnosisEvents { limit, format })
}

/// `cmx history <subcommand>`
fn parse_history(args: &[&str]) -> Result<Command, String> {
    if args.len() < 2 {
        return Err("Usage: cmx history <list|show|diff|restore|snapshot|prune>".into());
    }
    match args[1] {
        "list" => parse_history_list(args),
        "show" => parse_history_show(args),
        "diff" => parse_history_diff(args),
        "restore" => parse_history_restore(args),
        "snapshot" => Ok(Command::HistorySnapshot),
        "prune" => Ok(Command::HistoryPrune),
        _ => Err(format!("Unknown history subcommand: '{}'", args[1])),
    }
}

/// `cmx history list [--limit <n>] [--json]`
fn parse_history_list(args: &[&str]) -> Result<Command, String> {
    let mut limit = None;
    let mut format = None;
    let rest = &args[2..];
    let mut i = 0;
    while i < rest.len() {
        match rest[i] {
            "--limit" => {
                i += 1;
                limit = Some(take_arg(rest, i, "--limit")?);
            }
            "--json" => {
                format = Some("json".into());
            }
            other => return Err(format!("Unknown flag for history list: '{}'", other)),
        }
        i += 1;
    }
    Ok(Command::HistoryList { limit, format })
}

/// `cmx history show <entry>`
fn parse_history_show(args: &[&str]) -> Result<Command, String> {
    if args.len() < 3 {
        return Err("Usage: cmx history show <entry>".into());
    }
    Ok(Command::HistoryShow { id: args[2].into() })
}

/// `cmx history diff <entry> [<entry2>]`
fn parse_history_diff(args: &[&str]) -> Result<Command, String> {
    if args.len() < 3 {
        return Err("Usage: cmx history diff <entry> [<entry2>]".into());
    }
    let from = args[2].to_string();
    let to = if args.len() > 3 {
        Some(args[3].into())
    } else {
        None
    };
    Ok(Command::HistoryDiff { from, to })
}

/// `cmx history restore <entry>`
fn parse_history_restore(args: &[&str]) -> Result<Command, String> {
    if args.len() < 3 {
        return Err("Usage: cmx history restore <entry>".into());
    }
    Ok(Command::HistoryRestore { id: args[2].into() })
}

/// `cmx watch [--since <ms>] [--timeout <ms>]`
fn parse_watch(args: &[&str]) -> Result<Command, String> {
    let mut since = None;
    let mut timeout = None;
    let rest = &args[1..];
    let mut i = 0;
    while i < rest.len() {
        match rest[i] {
            "--since" => {
                i += 1;
                since = Some(take_arg(rest, i, "--since")?);
            }
            "--timeout" => {
                i += 1;
                timeout = Some(take_arg(rest, i, "--timeout")?);
            }
            other => return Err(format!("Unknown flag for watch: '{}'", other)),
        }
        i += 1;
    }
    Ok(Command::Watch { since, timeout })
}



/// `cmx daemon <run|stop>`
fn parse_daemon(args: &[&str]) -> Result<Command, String> {
    if args.len() < 2 {
        return Err("Usage: cmx daemon <run|stop>".into());
    }
    match args[1] {
        "run" => Ok(Command::DaemonRun),
        "stop" => Ok(Command::DaemonStop),
        _ => Err(format!("Unknown daemon subcommand: '{}'", args[1])),
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Safely take an argument value after a flag.
fn take_arg(args: &[&str], index: usize, flag: &str) -> Result<String, String> {
    if index >= args.len() {
        return Err(format!("{} requires a value", flag));
    }
    Ok(args[index].into())
}


// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_args() {
        assert!(parse_args(&[]).is_err());
    }

    #[test]
    fn unknown_command() {
        assert!(parse_args(&["bogus"]).is_err());
    }

    #[test]
    fn status() {
        let cmd = parse_args(&["status"]).unwrap();
        assert_eq!(cmd, Command::Status { format: None });
    }

    #[test]
    fn status_json() {
        let cmd = parse_args(&["status", "--json"]).unwrap();
        assert_eq!(cmd, Command::Status { format: Some("json".into()) });
    }

    #[test]
    fn view() {
        let cmd = parse_args(&["view", "worker-1"]).unwrap();
        assert_eq!(cmd, Command::View { name: "worker-1".into() });
    }

    #[test]
    fn view_missing_name() {
        assert!(parse_args(&["view"]).is_err());
    }

    #[test]
    fn agent_new_minimal() {
        let cmd = parse_args(&["agent", "new", "worker"]).unwrap();
        match cmd {
            Command::AgentNew { role, name, path, agent_type } => {
                assert_eq!(role, "worker");
                assert!(name.is_none());
                assert!(path.is_none());
                assert!(agent_type.is_none());
            }
            _ => panic!("Expected AgentNew"),
        }
    }

    #[test]
    fn agent_new_with_flags() {
        let cmd = parse_args(&[
            "agent", "new", "worker", "--name", "w1", "--path", "/tmp", "--type", "ssh",
        ])
        .unwrap();
        match cmd {
            Command::AgentNew { role, name, path, agent_type } => {
                assert_eq!(role, "worker");
                assert_eq!(name.unwrap(), "w1");
                assert_eq!(path.unwrap(), "/tmp");
                assert_eq!(agent_type.unwrap(), "ssh");
            }
            _ => panic!("Expected AgentNew"),
        }
    }

    #[test]
    fn agent_new_missing_role() {
        assert!(parse_args(&["agent", "new"]).is_err());
    }

    #[test]
    fn agent_kill() {
        let cmd = parse_args(&["agent", "kill", "w1"]).unwrap();
        assert_eq!(cmd, Command::AgentKill { name: "w1".into() });
    }

    #[test]
    fn agent_restart() {
        let cmd = parse_args(&["agent", "restart", "w1"]).unwrap();
        assert_eq!(cmd, Command::AgentRestart { name: "w1".into() });
    }

    #[test]
    fn agent_assign() {
        let cmd = parse_args(&["agent", "assign", "w1", "TASK1"]).unwrap();
        assert_eq!(cmd, Command::AgentAssign {
            name: "w1".into(),
            task: "TASK1".into(),
        });
    }

    #[test]
    fn agent_unassign() {
        let cmd = parse_args(&["agent", "unassign", "w1"]).unwrap();
        assert_eq!(cmd, Command::AgentUnassign { name: "w1".into() });
    }

    #[test]
    fn agent_status_with_notes() {
        let cmd = parse_args(&["agent", "status", "w1", "compiling", "tests"]).unwrap();
        assert_eq!(cmd, Command::AgentStatus {
            name: "w1".into(),
            notes: Some("compiling tests".into()),
        });
    }

    #[test]
    fn agent_list_plain() {
        let cmd = parse_args(&["agent", "list"]).unwrap();
        assert_eq!(cmd, Command::AgentList { format: None });
    }

    #[test]
    fn agent_list_json() {
        let cmd = parse_args(&["agent", "list", "--json"]).unwrap();
        assert_eq!(cmd, Command::AgentList { format: Some("json".into()) });
    }

    #[test]
    fn task_list_plain() {
        let cmd = parse_args(&["task", "list"]).unwrap();
        assert_eq!(cmd, Command::TaskList { format: None, project: None });
    }

    #[test]
    fn task_list_with_project() {
        let cmd = parse_args(&["task", "list", "CMX"]).unwrap();
        assert_eq!(cmd, Command::TaskList {
            format: None,
            project: Some("CMX".into()),
        });
    }

    #[test]
    fn task_list_json() {
        let cmd = parse_args(&["task", "list", "--json"]).unwrap();
        assert_eq!(cmd, Command::TaskList {
            format: Some("json".into()),
            project: None,
        });
    }

    #[test]
    fn task_get() {
        let cmd = parse_args(&["task", "get", "CMX1"]).unwrap();
        assert_eq!(cmd, Command::TaskGet { id: "CMX1".into() });
    }

    #[test]
    fn task_set() {
        let cmd = parse_args(&["task", "set", "CMX1", "status=completed", "title=Done"]).unwrap();
        assert_eq!(cmd, Command::TaskSet {
            id: "CMX1".into(),
            status: Some("completed".into()),
            title: Some("Done".into()),
            result: None,
            agent: None,
        });
    }

    #[test]
    fn task_set_bad_kv() {
        assert!(parse_args(&["task", "set", "CMX1", "noequalssign"]).is_err());
    }

    #[test]
    fn task_set_unknown_field() {
        assert!(parse_args(&["task", "set", "CMX1", "bogus=value"]).is_err());
    }

    #[test]
    fn task_check() {
        let cmd = parse_args(&["task", "check", "T1"]).unwrap();
        assert_eq!(cmd, Command::TaskCheck { id: "T1".into() });
    }

    #[test]
    fn task_uncheck() {
        let cmd = parse_args(&["task", "uncheck", "T1"]).unwrap();
        assert_eq!(cmd, Command::TaskUncheck { id: "T1".into() });
    }

    #[test]
    fn tell() {
        let cmd = parse_args(&["tell", "w1", "start", "task", "CMX1"]).unwrap();
        assert_eq!(cmd, Command::Tell {
            agent: "w1".into(),
            text: "start task CMX1".into(),
        });
    }

    #[test]
    fn interrupt_no_text() {
        let cmd = parse_args(&["interrupt", "w1"]).unwrap();
        assert_eq!(cmd, Command::Interrupt {
            agent: "w1".into(),
            text: None,
        });
    }

    #[test]
    fn interrupt_with_text() {
        let cmd = parse_args(&["interrupt", "w1", "stop", "now"]).unwrap();
        assert_eq!(cmd, Command::Interrupt {
            agent: "w1".into(),
            text: Some("stop now".into()),
        });
    }

    #[test]
    fn config_list() {
        let cmd = parse_args(&["config", "list"]).unwrap();
        assert_eq!(cmd, Command::ConfigList);
    }

    #[test]
    fn config_add() {
        let cmd = parse_args(&["config", "add", "max_retries", "5"]).unwrap();
        assert_eq!(cmd, Command::ConfigAdd {
            key: "max_retries".into(),
            value: "5".into(),
        });
    }

    #[test]
    fn config_load_with_path() {
        let cmd = parse_args(&["config", "load", "/etc/cmx.yaml"]).unwrap();
        assert_eq!(cmd, Command::ConfigLoad {
            path: Some("/etc/cmx.yaml".into()),
        });
    }

    #[test]
    fn config_load_no_path() {
        let cmd = parse_args(&["config", "load"]).unwrap();
        assert_eq!(cmd, Command::ConfigLoad { path: None });
    }

    #[test]
    fn config_save_with_path() {
        let cmd = parse_args(&["config", "save", "/tmp/out.yaml"]).unwrap();
        assert_eq!(cmd, Command::ConfigSave {
            path: Some("/tmp/out.yaml".into()),
        });
    }

    #[test]
    fn config_save_no_path() {
        let cmd = parse_args(&["config", "save"]).unwrap();
        assert_eq!(cmd, Command::ConfigSave { path: None });
    }

    #[test]
    fn project_add() {
        let cmd = parse_args(&["project", "add", "myproj", "/home/user/proj"]).unwrap();
        assert_eq!(cmd, Command::ProjectAdd {
            name: "myproj".into(),
            path: "/home/user/proj".into(),
        });
    }

    #[test]
    fn project_remove() {
        let cmd = parse_args(&["project", "remove", "myproj"]).unwrap();
        assert_eq!(cmd, Command::ProjectRemove { name: "myproj".into() });
    }

    #[test]
    fn project_list() {
        let cmd = parse_args(&["project", "list"]).unwrap();
        assert_eq!(cmd, Command::ProjectList { format: None });
    }

    #[test]
    fn project_list_json() {
        let cmd = parse_args(&["project", "list", "--json"]).unwrap();
        assert_eq!(cmd, Command::ProjectList { format: Some("json".into()) });
    }

    #[test]
    fn project_scan() {
        let cmd = parse_args(&["project", "scan", "myproj"]).unwrap();
        assert_eq!(cmd, Command::ProjectScan { name: "myproj".into() });
    }

    // --- pool CLI tests ---

    #[test]
    fn pool_list() {
        let cmd = parse_args(&["pool", "list"]).unwrap();
        assert_eq!(cmd, Command::PoolList);
    }

    #[test]
    fn pool_status() {
        let cmd = parse_args(&["pool", "status", "worker"]).unwrap();
        assert_eq!(cmd, Command::PoolStatus { role: "worker".into() });
    }

    #[test]
    fn pool_set_minimal() {
        let cmd = parse_args(&["pool", "set", "worker", "3"]).unwrap();
        assert_eq!(cmd, Command::PoolSet {
            role: "worker".into(),
            size: 3,
            path: None,
        });
    }

    #[test]
    fn pool_set_with_path() {
        let cmd = parse_args(&["pool", "set", "worker", "3", "--path", "/tmp/work"]).unwrap();
        assert_eq!(cmd, Command::PoolSet {
            role: "worker".into(),
            size: 3,
            path: Some("/tmp/work".into()),
        });
    }

    #[test]
    fn pool_set_invalid_size() {
        assert!(parse_args(&["pool", "set", "worker", "abc"]).is_err());
    }

    #[test]
    fn pool_remove() {
        let cmd = parse_args(&["pool", "remove", "worker"]).unwrap();
        assert_eq!(cmd, Command::PoolRemove { role: "worker".into() });
    }

    #[test]
    fn pool_missing_subcommand() {
        assert!(parse_args(&["pool"]).is_err());
    }

    #[test]
    fn pool_unknown_subcommand() {
        assert!(parse_args(&["pool", "bogus"]).is_err());
    }

    #[test]
    fn layout_row() {
        let cmd = parse_args(&["layout", "row", "main", "--percent", "30"]).unwrap();
        assert_eq!(cmd, Command::LayoutRow {
            session: "main".into(),
            percent: Some("30".into()),
        });
    }

    #[test]
    fn layout_row_no_percent() {
        let cmd = parse_args(&["layout", "row", "main"]).unwrap();
        assert_eq!(cmd, Command::LayoutRow {
            session: "main".into(),
            percent: None,
        });
    }

    #[test]
    fn layout_column() {
        let cmd = parse_args(&["layout", "column", "main"]).unwrap();
        assert_eq!(cmd, Command::LayoutColumn {
            session: "main".into(),
            percent: None,
        });
    }

    #[test]
    fn layout_place() {
        let cmd = parse_args(&["layout", "place", "%3", "worker-1"]).unwrap();
        assert_eq!(cmd, Command::LayoutPlace {
            pane: "%3".into(),
            agent: "worker-1".into(),
        });
    }

    #[test]
    fn layout_capture() {
        let cmd = parse_args(&["layout", "capture", "main"]).unwrap();
        assert_eq!(cmd, Command::LayoutCapture { session: "main".into() });
    }

    #[test]
    fn layout_merge() {
        let cmd = parse_args(&["layout", "merge", "main"]).unwrap();
        assert_eq!(cmd, Command::LayoutMerge { session: "main".into() });
    }

    #[test]
    fn layout_session_with_cwd() {
        let cmd = parse_args(&["layout", "session", "work", "--cwd", "/projects"]).unwrap();
        assert_eq!(cmd, Command::LayoutSession {
            name: "work".into(),
            cwd: Some("/projects".into()),
        });
    }

    #[test]
    fn layout_session_no_cwd() {
        let cmd = parse_args(&["layout", "session", "work"]).unwrap();
        assert_eq!(cmd, Command::LayoutSession {
            name: "work".into(),
            cwd: None,
        });
    }

    #[test]
    fn client_next() {
        let cmd = parse_args(&["client", "next"]).unwrap();
        assert_eq!(cmd, Command::ClientNext);
    }

    #[test]
    fn client_prev() {
        let cmd = parse_args(&["client", "prev"]).unwrap();
        assert_eq!(cmd, Command::ClientPrev);
    }

    // --- status --json ---

    // --- rig CLI tests ---

    #[test]
    fn rig_missing_subcommand() {
        assert!(parse_args(&["rig"]).is_err());
    }

    #[test]
    fn rig_unknown_subcommand() {
        assert!(parse_args(&["rig", "bogus"]).is_err());
    }

    #[test]
    fn rig_init_minimal() {
        let cmd = parse_args(&["rig", "init", "user@host:22"]).unwrap();
        assert_eq!(cmd, Command::RigInit {
            host: "user@host:22".into(),
            name: None,
        });
    }

    #[test]
    fn rig_init_with_name() {
        let cmd = parse_args(&["rig", "init", "user@host:22", "--name", "gpu1"]).unwrap();
        assert_eq!(cmd, Command::RigInit {
            host: "user@host:22".into(),
            name: Some("gpu1".into()),
        });
    }

    #[test]
    fn rig_init_missing_host() {
        assert!(parse_args(&["rig", "init"]).is_err());
    }

    #[test]
    fn rig_push_minimal() {
        let cmd = parse_args(&["rig", "push", "/local/folder"]).unwrap();
        assert_eq!(cmd, Command::RigPush {
            folder: "/local/folder".into(),
            remote: None,
        });
    }

    #[test]
    fn rig_push_with_remote() {
        let cmd = parse_args(&["rig", "push", "/local/folder", "--remote", "gpu1"]).unwrap();
        assert_eq!(cmd, Command::RigPush {
            folder: "/local/folder".into(),
            remote: Some("gpu1".into()),
        });
    }

    #[test]
    fn rig_push_missing_folder() {
        assert!(parse_args(&["rig", "push"]).is_err());
    }

    #[test]
    fn rig_pull_minimal() {
        let cmd = parse_args(&["rig", "pull", "/local/folder"]).unwrap();
        assert_eq!(cmd, Command::RigPull {
            folder: "/local/folder".into(),
            remote: None,
        });
    }

    #[test]
    fn rig_pull_with_remote() {
        let cmd = parse_args(&["rig", "pull", "/local/folder", "--remote", "gpu1"]).unwrap();
        assert_eq!(cmd, Command::RigPull {
            folder: "/local/folder".into(),
            remote: Some("gpu1".into()),
        });
    }

    #[test]
    fn rig_status_no_remote() {
        let cmd = parse_args(&["rig", "status"]).unwrap();
        assert_eq!(cmd, Command::RigStatus { remote: None });
    }

    #[test]
    fn rig_status_with_remote() {
        let cmd = parse_args(&["rig", "status", "--remote", "gpu1"]).unwrap();
        assert_eq!(cmd, Command::RigStatus { remote: Some("gpu1".into()) });
    }

    #[test]
    fn rig_health_no_remote() {
        let cmd = parse_args(&["rig", "health"]).unwrap();
        assert_eq!(cmd, Command::RigHealth { remote: None });
    }

    #[test]
    fn rig_health_with_remote() {
        let cmd = parse_args(&["rig", "health", "--remote", "gpu1"]).unwrap();
        assert_eq!(cmd, Command::RigHealth { remote: Some("gpu1".into()) });
    }

    #[test]
    fn rig_stop_no_remote() {
        let cmd = parse_args(&["rig", "stop"]).unwrap();
        assert_eq!(cmd, Command::RigStop { remote: None });
    }

    #[test]
    fn rig_stop_with_remote() {
        let cmd = parse_args(&["rig", "stop", "--remote", "gpu1"]).unwrap();
        assert_eq!(cmd, Command::RigStop { remote: Some("gpu1".into()) });
    }

    #[test]
    fn rig_list() {
        let cmd = parse_args(&["rig", "list"]).unwrap();
        assert_eq!(cmd, Command::RigList);
    }

    #[test]
    fn rig_default_show() {
        let cmd = parse_args(&["rig", "default"]).unwrap();
        assert_eq!(cmd, Command::RigDefault { name: None });
    }

    #[test]
    fn rig_default_set() {
        let cmd = parse_args(&["rig", "default", "gpu1"]).unwrap();
        assert_eq!(cmd, Command::RigDefault { name: Some("gpu1".into()) });
    }

    // --- diagnosis CLI tests ---

    #[test]
    fn diagnosis_missing_subcommand() {
        assert!(parse_args(&["diagnosis"]).is_err());
    }

    #[test]
    fn diagnosis_unknown_subcommand() {
        assert!(parse_args(&["diagnosis", "bogus"]).is_err());
    }

    #[test]
    fn diagnosis_report() {
        let cmd = parse_args(&["diagnosis", "report"]).unwrap();
        assert_eq!(cmd, Command::DiagnosisReport);
    }

    #[test]
    fn diagnosis_reliability_no_args() {
        let cmd = parse_args(&["diagnosis", "reliability"]).unwrap();
        assert_eq!(cmd, Command::DiagnosisReliability { signal: None, format: None });
    }

    #[test]
    fn diagnosis_reliability_with_signal() {
        let cmd = parse_args(&["diagnosis", "reliability", "--signal", "heartbeat_stale"]).unwrap();
        assert_eq!(cmd, Command::DiagnosisReliability {
            signal: Some("heartbeat_stale".into()),
            format: None,
        });
    }

    #[test]
    fn diagnosis_reliability_json() {
        let cmd = parse_args(&["diagnosis", "reliability", "--json"]).unwrap();
        assert_eq!(cmd, Command::DiagnosisReliability {
            signal: None,
            format: Some("json".into()),
        });
    }

    #[test]
    fn diagnosis_reliability_all_flags() {
        let cmd = parse_args(&["diagnosis", "reliability", "--signal", "error_pattern", "--json"]).unwrap();
        assert_eq!(cmd, Command::DiagnosisReliability {
            signal: Some("error_pattern".into()),
            format: Some("json".into()),
        });
    }

    #[test]
    fn diagnosis_effectiveness_no_args() {
        let cmd = parse_args(&["diagnosis", "effectiveness"]).unwrap();
        assert_eq!(cmd, Command::DiagnosisEffectiveness { signal: None, format: None });
    }

    #[test]
    fn diagnosis_effectiveness_with_signal() {
        let cmd = parse_args(&["diagnosis", "effectiveness", "--signal", "error_pattern"]).unwrap();
        assert_eq!(cmd, Command::DiagnosisEffectiveness {
            signal: Some("error_pattern".into()),
            format: None,
        });
    }

    #[test]
    fn diagnosis_thresholds_plain() {
        let cmd = parse_args(&["diagnosis", "thresholds"]).unwrap();
        assert_eq!(cmd, Command::DiagnosisThresholds { format: None });
    }

    #[test]
    fn diagnosis_thresholds_json() {
        let cmd = parse_args(&["diagnosis", "thresholds", "--json"]).unwrap();
        assert_eq!(cmd, Command::DiagnosisThresholds { format: Some("json".into()) });
    }

    #[test]
    fn diagnosis_events_no_args() {
        let cmd = parse_args(&["diagnosis", "events"]).unwrap();
        assert_eq!(cmd, Command::DiagnosisEvents { limit: None, format: None });
    }

    #[test]
    fn diagnosis_events_with_limit() {
        let cmd = parse_args(&["diagnosis", "events", "--limit", "50"]).unwrap();
        assert_eq!(cmd, Command::DiagnosisEvents {
            limit: Some("50".into()),
            format: None,
        });
    }

    #[test]
    fn diagnosis_events_json() {
        let cmd = parse_args(&["diagnosis", "events", "--json"]).unwrap();
        assert_eq!(cmd, Command::DiagnosisEvents {
            limit: None,
            format: Some("json".into()),
        });
    }

    #[test]
    fn diagnosis_events_all_flags() {
        let cmd = parse_args(&["diagnosis", "events", "--limit", "10", "--json"]).unwrap();
        assert_eq!(cmd, Command::DiagnosisEvents {
            limit: Some("10".into()),
            format: Some("json".into()),
        });
    }

    // --- history CLI tests ---

    #[test]
    fn history_missing_subcommand() {
        assert!(parse_args(&["history"]).is_err());
    }

    #[test]
    fn history_unknown_subcommand() {
        assert!(parse_args(&["history", "bogus"]).is_err());
    }

    #[test]
    fn history_list_no_args() {
        let cmd = parse_args(&["history", "list"]).unwrap();
        assert_eq!(cmd, Command::HistoryList { limit: None, format: None });
    }

    #[test]
    fn history_list_with_limit() {
        let cmd = parse_args(&["history", "list", "--limit", "10"]).unwrap();
        assert_eq!(cmd, Command::HistoryList {
            limit: Some("10".into()),
            format: None,
        });
    }

    #[test]
    fn history_list_json() {
        let cmd = parse_args(&["history", "list", "--json"]).unwrap();
        assert_eq!(cmd, Command::HistoryList {
            limit: None,
            format: Some("json".into()),
        });
    }

    #[test]
    fn history_list_all_flags() {
        let cmd = parse_args(&["history", "list", "--limit", "5", "--json"]).unwrap();
        assert_eq!(cmd, Command::HistoryList {
            limit: Some("5".into()),
            format: Some("json".into()),
        });
    }

    #[test]
    fn history_show() {
        let cmd = parse_args(&["history", "show", "2026-02-22T10-00-00.md"]).unwrap();
        assert_eq!(cmd, Command::HistoryShow { id: "2026-02-22T10-00-00.md".into() });
    }

    #[test]
    fn history_show_by_index() {
        let cmd = parse_args(&["history", "show", "0"]).unwrap();
        assert_eq!(cmd, Command::HistoryShow { id: "0".into() });
    }

    #[test]
    fn history_show_missing_entry() {
        assert!(parse_args(&["history", "show"]).is_err());
    }

    #[test]
    fn history_diff_one_entry() {
        let cmd = parse_args(&["history", "diff", "0"]).unwrap();
        assert_eq!(cmd, Command::HistoryDiff { from: "0".into(), to: None });
    }

    #[test]
    fn history_diff_two_entries() {
        let cmd = parse_args(&["history", "diff", "0", "1"]).unwrap();
        assert_eq!(cmd, Command::HistoryDiff {
            from: "0".into(),
            to: Some("1".into()),
        });
    }

    #[test]
    fn history_diff_missing_entry() {
        assert!(parse_args(&["history", "diff"]).is_err());
    }

    #[test]
    fn history_restore() {
        let cmd = parse_args(&["history", "restore", "0"]).unwrap();
        assert_eq!(cmd, Command::HistoryRestore { id: "0".into() });
    }

    #[test]
    fn history_restore_missing_entry() {
        assert!(parse_args(&["history", "restore"]).is_err());
    }

    #[test]
    fn history_snapshot() {
        let cmd = parse_args(&["history", "snapshot"]).unwrap();
        assert_eq!(cmd, Command::HistorySnapshot);
    }

    #[test]
    fn history_prune() {
        let cmd = parse_args(&["history", "prune"]).unwrap();
        assert_eq!(cmd, Command::HistoryPrune);
    }

    // --- watch CLI tests ---

    #[test]
    fn watch_no_args() {
        let cmd = parse_args(&["watch"]).unwrap();
        assert_eq!(cmd, Command::Watch { since: None, timeout: None });
    }

    #[test]
    fn watch_with_since() {
        let cmd = parse_args(&["watch", "--since", "1708700000000"]).unwrap();
        assert_eq!(cmd, Command::Watch {
            since: Some("1708700000000".into()),
            timeout: None,
        });
    }

    #[test]
    fn watch_with_timeout() {
        let cmd = parse_args(&["watch", "--timeout", "5000"]).unwrap();
        assert_eq!(cmd, Command::Watch {
            since: None,
            timeout: Some("5000".into()),
        });
    }

    #[test]
    fn watch_all_flags() {
        let cmd = parse_args(&["watch", "--since", "1000", "--timeout", "5000"]).unwrap();
        assert_eq!(cmd, Command::Watch {
            since: Some("1000".into()),
            timeout: Some("5000".into()),
        });
    }

    // --- Daemon subcommand tests ---

    #[test]
    fn daemon_run() {
        let cmd = parse_args(&["daemon", "run"]).unwrap();
        assert_eq!(cmd, Command::DaemonRun);
    }

    #[test]
    fn daemon_stop() {
        let cmd = parse_args(&["daemon", "stop"]).unwrap();
        assert_eq!(cmd, Command::DaemonStop);
    }

    #[test]
    fn daemon_no_subcommand() {
        let result = parse_args(&["daemon"]);
        assert!(result.is_err());
    }

    #[test]
    fn daemon_unknown_subcommand() {
        let result = parse_args(&["daemon", "restart"]);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Unknown daemon subcommand"));
    }
}
