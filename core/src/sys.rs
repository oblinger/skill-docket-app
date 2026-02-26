use std::path::{Path, PathBuf};

use crate::agent::pool::{PoolConfig, PoolManager};
use crate::command::Command;
use crate::data::Data;
use crate::infrastructure::runner::ShellRunner;
use crate::library::{Library, LibrarySource, LibraryType, SourceKind};
use crate::library::LibraryConfig;
use crate::rig::config::{RemoteConfig, RigRegistry};
use crate::rig::orchestrator::RigOrchestrator;
use crate::types::agent::{Agent, AgentStatus, AgentType, HealthState};
use crate::types::config::{FolderEntry, Settings};
use crate::types::message::Message;
use cmx_utils::response::{Action, Response};
use crate::types::task::{TaskNode, TaskSource, TaskStatus};
use crate::diagnosis::{DiagnosisEngine, SignalType};
use crate::history::{HistoryManager, HistoryEntry};


/// Central runtime for the CMX daemon. Owns all state and dispatches commands.
///
/// `Sys` wraps a `Data` store plus a mutable copy of `Settings` for runtime
/// overrides. The separate settings copy exists because `Data.settings()` is
/// immutable (loaded from disk); runtime changes go through `Sys.settings`.
pub struct Sys {
    data: Data,
    settings: Settings,
    actions: Vec<Action>,
    rig: Option<RigOrchestrator>,
    pool: PoolManager,
    library: Library,
}


/// Build a LibraryConfig from the current folder registry, adding project
/// skill sources for any registered project that has a `skills/` subfolder.
fn build_library_config(data: &Data) -> LibraryConfig {
    let mut config = LibraryConfig::default();
    for folder in data.folders().list() {
        let skills_dir = PathBuf::from(&folder.path).join("skills");
        if skills_dir.is_dir() {
            config.extra_sources.push(crate::library::ExtraSource {
                path: skills_dir,
                library_type: LibraryType::SkillsOnly,
                priority: 25,
                name: format!("project:{}", folder.name),
            });
        }
    }
    config
}

/// Build a PoolManager from the current settings.
fn build_pool_manager(settings: &Settings) -> PoolManager {
    let mut pool = PoolManager::new();
    for (role, cfg) in &settings.pool_configs {
        pool.set_pool(role, PoolConfig {
            target_size: cfg.size,
            auto_expand: settings.pool_auto_expand,
            max_size: cfg.max_size.unwrap_or(cfg.size * 2),
            path: cfg.path.clone(),
        });
    }
    pool
}


impl Sys {
    /// Create a new Sys from a config directory, loading settings from disk.
    pub fn new(config_dir: &Path) -> Result<Sys, String> {
        let data = Data::new(config_dir)?;
        let settings = data.settings().clone();
        let rig = Some(RigOrchestrator::new(
            RigRegistry::new(),
            Box::new(ShellRunner),
        ));
        let pool = build_pool_manager(&settings);
        let lib_config = build_library_config(&data);
        let library = Library::new(&lib_config).unwrap_or_else(|_| Library::empty());
        Ok(Sys {
            data,
            settings,
            actions: Vec::new(),
            rig,
            pool,
            library,
        })
    }

    /// Create a Sys from a pre-built Data. Useful for testing.
    pub fn from_data(data: Data) -> Sys {
        let settings = data.settings().clone();
        let pool = build_pool_manager(&settings);
        let lib_config = build_library_config(&data);
        let library = Library::new(&lib_config).unwrap_or_else(|_| Library::empty());
        Sys {
            data,
            settings,
            actions: Vec::new(),
            rig: None,
            pool,
            library,
        }
    }

    /// Create a Sys from a pre-built Data and a RigOrchestrator. Useful for testing.
    pub fn from_data_with_rig(data: Data, rig: RigOrchestrator) -> Sys {
        let settings = data.settings().clone();
        let pool = build_pool_manager(&settings);
        let lib_config = build_library_config(&data);
        let library = Library::new(&lib_config).unwrap_or_else(|_| Library::empty());
        Sys {
            data,
            settings,
            actions: Vec::new(),
            rig: Some(rig),
            pool,
            library,
        }
    }

    /// The single dispatch method. Every command enters here.
    pub fn execute(&mut self, cmd: Command) -> Response {
        self.actions.clear();
        match cmd {
            Command::Status { format } => self.cmd_status(format),
            Command::View { name } => self.cmd_view(name),
            Command::AgentNew { role, name, path, agent_type } => {
                self.cmd_agent_new(role, name, path, agent_type)
            }
            Command::AgentKill { name } => self.cmd_agent_kill(name),
            Command::AgentRestart { name } => self.cmd_agent_restart(name),
            Command::AgentAssign { name, task } => self.cmd_agent_assign(name, task),
            Command::AgentUnassign { name } => self.cmd_agent_unassign(name),
            Command::AgentStatus { name, notes } => self.cmd_agent_status(name, notes),
            Command::AgentList { format } => self.cmd_agent_list(format),
            Command::TaskList { format, project } => self.cmd_task_list(format, project),
            Command::TaskGet { id } => self.cmd_task_get(id),
            Command::TaskSet { id, status, title, result, agent } => {
                self.cmd_task_set(id, status, title, result, agent)
            }
            Command::TaskCheck { id } => self.cmd_task_check(id),
            Command::TaskUncheck { id } => self.cmd_task_uncheck(id),
            Command::ConfigLoad { path } => self.cmd_config_load(path),
            Command::ConfigSave { path } => self.cmd_config_save(path),
            Command::ConfigAdd { key, value } => self.cmd_config_add(key, value),
            Command::ConfigList => self.cmd_config_list(),
            Command::ProjectAdd { name, path } => self.cmd_project_add(name, path),
            Command::ProjectRemove { name } => self.cmd_project_remove(name),
            Command::ProjectList { format } => self.cmd_project_list(format),
            Command::ProjectScan { name } => self.cmd_project_scan(name),
            Command::RoadmapLoad { path } => self.cmd_roadmap_load(path),
            Command::PoolList => self.cmd_pool_list(),
            Command::PoolStatus { role } => self.cmd_pool_status(role),
            Command::PoolSet { role, size, path } => self.cmd_pool_set(role, size, path),
            Command::PoolRemove { role } => self.cmd_pool_remove(role),
            Command::Tell { agent, text } => self.cmd_tell(agent, text),
            Command::Interrupt { agent, text } => self.cmd_interrupt(agent, text),
            // Layout and Client commands are handled by MuxUX, not the docket app.
            Command::LayoutRow { .. }
            | Command::LayoutColumn { .. }
            | Command::LayoutMerge { .. }
            | Command::LayoutPlace { .. }
            | Command::LayoutCapture { .. }
            | Command::LayoutSession { .. }
            | Command::ClientNext
            | Command::ClientPrev => Response::Error {
                message: "Layout/Client commands are handled by MuxUX".into(),
            },
            Command::RigInit { host, name } => self.cmd_rig_init(host, name),
            Command::RigPush { folder, remote } => self.cmd_rig_push(folder, remote),
            Command::RigPull { folder, remote } => self.cmd_rig_pull(folder, remote),
            Command::RigStatus { remote } => self.cmd_rig_status(remote),
            Command::RigHealth { remote } => self.cmd_rig_health(remote),
            Command::RigStop { remote } => self.cmd_rig_stop(remote),
            Command::RigList => self.cmd_rig_list(),
            Command::RigDefault { name } => self.cmd_rig_default(name),
            Command::DiagnosisReport => self.cmd_diagnosis_report(),
            Command::DiagnosisReliability { signal, format } => {
                self.cmd_diagnosis_reliability(signal, format)
            }
            Command::DiagnosisEffectiveness { signal, format } => {
                self.cmd_diagnosis_effectiveness(signal, format)
            }
            Command::DiagnosisThresholds { format } => self.cmd_diagnosis_thresholds(format),
            Command::DiagnosisEvents { limit, format } => {
                self.cmd_diagnosis_events(limit, format)
            }
            Command::HistoryList { limit, format } => self.cmd_history_list(limit, format),
            Command::HistoryShow { id } => self.cmd_history_show(id),
            Command::HistoryDiff { from, to } => self.cmd_history_diff(from, to),
            Command::HistoryRestore { id } => self.cmd_history_restore(id),
            Command::HistorySnapshot => self.cmd_history_snapshot(),
            Command::HistoryPrune => self.cmd_history_prune(),
            Command::Watch { .. } => Response::Error {
                message: "Watch commands are handled at the service layer, not via Sys::execute()".into(),
            },
            Command::DaemonRun => Response::Error {
                message: "DaemonRun must be handled by the binary, not dispatched to Sys".into(),
            },
            Command::Tui => Response::Error {
                message: "Tui must be handled by the binary, not dispatched to Sys".into(),
            },
            Command::DaemonStop => Response::Ok {
                output: "Daemon shutting down".into(),
            },
            Command::Help { topic } => self.cmd_help(topic),
        }
    }

    /// Actions emitted during the last execute() call.
    pub fn pending_actions(&self) -> &[Action] {
        &self.actions
    }

    /// Take and clear accumulated actions.
    pub fn drain_actions(&mut self) -> Vec<Action> {
        std::mem::take(&mut self.actions)
    }

    /// Record that an agent's tmux session has been created.
    pub fn notify_session_created(&mut self, agent_name: &str, session_name: &str) -> Result<(), String> {
        let agent = self.data.agents_mut().get_mut(agent_name)
            .ok_or_else(|| format!("agent '{}' not found", agent_name))?;
        agent.session = Some(session_name.to_string());
        Ok(())
    }

    /// Record that a spawning agent's process is confirmed ready.
    /// Sets health to Healthy and status to Idle.
    pub fn notify_agent_ready(&mut self, agent_name: &str) -> Result<(), String> {
        let agent = self.data.agents_mut().get_mut(agent_name)
            .ok_or_else(|| format!("agent '{}' not found", agent_name))?;
        agent.health = HealthState::Healthy;
        agent.status = AgentStatus::Idle;
        Ok(())
    }

    /// Borrow the data layer (for inspection in tests / external code).
    pub fn data(&self) -> &Data {
        &self.data
    }

    /// Borrow the runtime settings.
    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// Borrow the skill library.
    pub fn library(&self) -> &Library {
        &self.library
    }

    /// Mutable access to the message store (for monitor cycle delivery).
    pub fn messages_mut(&mut self) -> &mut crate::data::messages::MessageStore {
        self.data.messages_mut()
    }

    /// Apply a health assessment from the monitor cycle to an agent's state.
    pub fn apply_health_update(&mut self, assessment: &crate::types::health::HealthAssessment) {
        if let Some(agent) = self.data.agents_mut().get_mut(&assessment.agent) {
            agent.health = assessment.overall.clone();
            agent.last_heartbeat_ms = Some(assessment.timestamp_ms);
            // Update status based on health
            match &assessment.overall {
                HealthState::Unhealthy => {
                    if agent.status != AgentStatus::Dead {
                        agent.status = AgentStatus::Stalled;
                        agent.status_notes = assessment.reason.clone();
                    }
                }
                HealthState::Degraded => {
                    if agent.status == AgentStatus::Idle || agent.status == AgentStatus::Busy {
                        agent.status_notes = assessment.reason.clone();
                    }
                }
                _ => {}
            }
        }
    }

    /// Build a `SystemSnapshot` capturing the current system state.
    pub fn build_snapshot(&self) -> crate::snapshot::state::SystemSnapshot {
        use crate::snapshot::state::{AgentSnapshot, SystemSnapshot, TaskSnapshot};

        let agents: Vec<AgentSnapshot> = self
            .data
            .agents()
            .list()
            .iter()
            .map(|a| AgentSnapshot {
                name: a.name.clone(),
                role: a.role.clone(),
                agent_type: format!("{:?}", a.agent_type).to_lowercase(),
                status: format!("{:?}", a.status).to_lowercase(),
                task: a.task.clone(),
                path: a.path.clone(),
                health: format!("{:?}", a.health).to_lowercase(),
                last_heartbeat_ms: a.last_heartbeat_ms,
            })
            .collect();

        let tasks: Vec<TaskSnapshot> = self
            .data
            .tasks()
            .flat_list()
            .iter()
            .map(|(t, _depth)| TaskSnapshot {
                id: t.id.clone(),
                title: t.title.clone(),
                status: format!("{:?}", t.status).to_lowercase(),
                source: format!("{:?}", t.source).to_lowercase(),
                agent: t.agent.clone(),
                result: t.result.clone(),
                children_ids: t.children.iter().map(|c| c.id.clone()).collect(),
                spec_path: t.spec_path.clone(),
            })
            .collect();

        let now = now_ms();
        SystemSnapshot::new("0.1.0", now)
            .with_agents(agents)
            .with_tasks(tasks)
            .with_message_count(self.data.messages().all_pending().len())
    }

    /// Persist the current system state to `current_state.json` in the config directory.
    pub fn save_current_state(&self) -> Result<(), String> {
        let snapshot = self.build_snapshot();
        let path = self.data.config_dir().join("current_state.json");
        crate::snapshot::checkpoint::save_snapshot(&snapshot, &path)
    }

    // -----------------------------------------------------------------------
    // Command handlers
    // -----------------------------------------------------------------------

    fn cmd_status(&self, format: Option<String>) -> Response {
        let agent_count = self.data.agents().list().len();
        let task_count = self.data.tasks().flat_list().len();
        let pending_msgs = self.data.messages().all_pending().len();
        let project_count = self.data.folders().list().len();
        if format.as_deref() == Some("json") {
            let agents: Vec<&str> = self.data.agents().list().iter().map(|a| a.name.as_str()).collect();
            let obj = serde_json::json!({
                "agents": agents,
                "agent_count": agent_count,
                "task_count": task_count,
                "project_count": project_count,
                "pending_messages": pending_msgs,
            });
            Response::Ok {
                output: serde_json::to_string_pretty(&obj).unwrap_or_else(|_| "{}".into()),
            }
        } else {
            Response::Ok {
                output: format!(
                    "agents: {}, tasks: {}, projects: {}, pending messages: {}",
                    agent_count, task_count, project_count, pending_msgs
                ),
            }
        }
    }

    fn cmd_view(&self, name: String) -> Response {
        // Try agent first
        if let Some(agent) = self.data.agents().get(&name) {
            let json = serde_json::to_string_pretty(agent).unwrap_or_else(|_| "{}".into());
            return Response::Ok { output: json };
        }
        // Try task
        if let Some(task) = self.data.tasks().get(&name) {
            let json = serde_json::to_string_pretty(task).unwrap_or_else(|_| "{}".into());
            return Response::Ok { output: json };
        }
        // Try folder/project
        if let Some(folder) = self.data.folders().get(&name) {
            let json = serde_json::to_string_pretty(folder).unwrap_or_else(|_| "{}".into());
            return Response::Ok { output: json };
        }
        Response::Error {
            message: format!("Nothing found named '{}'", name),
        }
    }

    fn cmd_agent_new(
        &mut self,
        role: String,
        name: Option<String>,
        path: Option<String>,
        agent_type: Option<String>,
    ) -> Response {
        let name = name.unwrap_or_else(|| self.data.agents().next_name(&role));
        let path = path.unwrap_or_else(|| self.settings.project_root.clone());
        let agent_type_val = match agent_type.as_deref() {
            Some("console") => AgentType::Console,
            Some("ssh") => AgentType::Ssh,
            _ => AgentType::Claude,
        };
        let agent = Agent {
            name: name.clone(),
            role: role.clone(),
            agent_type: agent_type_val,
            task: None,
            path: path.clone(),
            status: AgentStatus::Idle,
            status_notes: String::new(),
            health: HealthState::Unknown,
            last_heartbeat_ms: None,
            session: None,
        };
        if let Err(e) = self.data.agents_mut().add(agent) {
            return Response::Error { message: e };
        }
        self.actions.push(Action::CreateAgent {
            name: name.clone(),
            role,
            path,
        });
        Response::Ok {
            output: format!("Agent '{}' created", name),
        }
    }

    fn cmd_agent_kill(&mut self, name: String) -> Response {
        if let Err(e) = self.data.agents_mut().remove(&name) {
            return Response::Error { message: e };
        }
        self.actions.push(Action::KillAgent { name: name.clone() });
        Response::Ok {
            output: format!("Agent '{}' killed", name),
        }
    }

    fn cmd_agent_restart(&mut self, name: String) -> Response {
        let agent = match self.data.agents().get(&name) {
            Some(a) => a.clone(),
            None => {
                return Response::Error {
                    message: format!("Agent '{}' not found", name),
                }
            }
        };
        // Kill then re-create
        self.actions.push(Action::KillAgent { name: name.clone() });
        self.actions.push(Action::CreateAgent {
            name: agent.name.clone(),
            role: agent.role.clone(),
            path: agent.path.clone(),
        });
        // Reset status in registry
        if let Some(a) = self.data.agents_mut().get_mut(&name) {
            a.status = AgentStatus::Idle;
            a.health = HealthState::Unknown;
            a.status_notes = String::new();
        }
        Response::Ok {
            output: format!("Agent '{}' restarting", name),
        }
    }

    fn cmd_agent_assign(&mut self, name: String, task: String) -> Response {
        if let Err(e) = self.data.agents_mut().assign(&name, &task) {
            return Response::Error { message: e };
        }
        // Also mark the task as assigned in the task tree
        let _ = self.data.tasks_mut().assign(&task, &name);
        self.actions.push(Action::UpdateAssignment {
            agent: name.clone(),
            task: Some(task.clone()),
        });

        // Resolve role skill and inject briefing into agent's session
        if let Some(agent) = self.data.agents().get(&name) {
            let skill_text = self.library.get_parsed(&agent.role)
                .ok()
                .map(|doc| doc.instructions.clone());
            let task_spec = self.data.tasks().get(&task)
                .and_then(|t| t.spec_path.as_ref())
                .and_then(|p| std::fs::read_to_string(p).ok());
            let project_ctx = self.data.folders().list().first()
                .map(|f| format!("Project: {}\nPath: {}", f.name, f.path));

            let briefing = crate::agent::briefing::compose_briefing(
                skill_text.as_deref(),
                task_spec.as_deref(),
                project_ctx.as_deref(),
            );

            if !briefing.is_empty() {
                if let Some(ref session) = agent.session {
                    self.actions.push(Action::SendKeys {
                        target: session.clone(),
                        keys: briefing,
                    });
                }
            }
        }

        Response::Ok {
            output: format!("Agent '{}' assigned to task '{}'", name, task),
        }
    }

    fn cmd_agent_unassign(&mut self, name: String) -> Response {
        let old_task = match self.data.agents_mut().unassign(&name) {
            Ok(t) => t,
            Err(e) => return Response::Error { message: e },
        };
        if let Some(ref task_id) = old_task {
            let _ = self.data.tasks_mut().unassign(task_id);
        }
        self.actions.push(Action::UpdateAssignment {
            agent: name.clone(),
            task: None,
        });
        Response::Ok {
            output: format!("Agent '{}' unassigned", name),
        }
    }

    fn cmd_agent_status(&mut self, name: String, notes: Option<String>) -> Response {
        let notes = notes.unwrap_or_default();
        if let Err(e) = self.data.agents_mut().update_status(&name, &notes) {
            return Response::Error { message: e };
        }
        Response::Ok {
            output: format!("Agent '{}' status updated", name),
        }
    }

    fn cmd_agent_list(&self, format: Option<String>) -> Response {
        let agents = self.data.agents().list();
        if format.as_deref() == Some("json") {
            let json = serde_json::to_string_pretty(agents).unwrap_or_else(|_| "[]".into());
            return Response::Ok { output: json };
        }
        if agents.is_empty() {
            return Response::Ok {
                output: "No agents".into(),
            };
        }
        let mut lines = Vec::new();
        for a in agents {
            let task_str = a.task.as_deref().unwrap_or("-");
            lines.push(format!(
                "{:<16} {:<10} {:<10} {:<12} {}",
                a.name,
                a.role,
                format!("{:?}", a.status).to_lowercase(),
                format!("{:?}", a.health).to_lowercase(),
                task_str
            ));
        }
        Response::Ok {
            output: lines.join("\n"),
        }
    }

    fn cmd_task_list(&self, format: Option<String>, project: Option<String>) -> Response {
        let all_tasks = self.data.tasks().flat_list();
        let tasks: Vec<&(&TaskNode, usize)> = if let Some(ref proj) = project {
            all_tasks
                .iter()
                .filter(|(t, _depth)| t.id.starts_with(proj.as_str()))
                .collect()
        } else {
            all_tasks.iter().collect()
        };
        if format.as_deref() == Some("json") {
            let nodes: Vec<&TaskNode> = tasks.iter().map(|(t, _)| *t).collect();
            let json = serde_json::to_string_pretty(&nodes).unwrap_or_else(|_| "[]".into());
            return Response::Ok { output: json };
        }
        if tasks.is_empty() {
            return Response::Ok {
                output: "No tasks".into(),
            };
        }
        let mut lines = Vec::new();
        for (t, depth) in &tasks {
            let indent = "  ".repeat(*depth);
            let agent_str = t.agent.as_deref().unwrap_or("-");
            lines.push(format!(
                "{}{:<12} {:<30} {:<12} {}",
                indent,
                t.id,
                t.title,
                format!("{:?}", t.status).to_lowercase(),
                agent_str
            ));
        }
        Response::Ok {
            output: lines.join("\n"),
        }
    }

    fn cmd_task_get(&self, id: String) -> Response {
        match self.data.tasks().get(&id) {
            Some(task) => {
                let json = serde_json::to_string_pretty(task).unwrap_or_else(|_| "{}".into());
                Response::Ok { output: json }
            }
            None => Response::Error {
                message: format!("Task '{}' not found", id),
            },
        }
    }

    fn cmd_task_set(
        &mut self,
        id: String,
        status: Option<String>,
        title: Option<String>,
        result: Option<String>,
        agent: Option<String>,
    ) -> Response {
        let task = match self.data.tasks_mut().get_mut(&id) {
            Some(t) => t,
            None => {
                return Response::Error {
                    message: format!("Task '{}' not found", id),
                }
            }
        };
        // Apply any provided fields
        let mut new_status: Option<TaskStatus> = None;
        if let Some(status_str) = status {
            let parsed = match parse_task_status(&status_str) {
                Ok(s) => s,
                Err(e) => return Response::Error { message: e },
            };
            task.status = parsed.clone();
            new_status = Some(parsed);
        }
        if let Some(title) = title {
            task.title = title;
        }
        if let Some(result) = result {
            task.result = Some(result);
        }
        if let Some(agent) = agent {
            task.agent = if agent.is_empty() || agent == "-" {
                None
            } else {
                Some(agent)
            };
        }
        if let Some(ref s) = new_status {
            self.roadmap_write_back(&id, s);
        }
        Response::Ok {
            output: format!("Task '{}' updated", id),
        }
    }

    fn cmd_task_check(&mut self, id: String) -> Response {
        if let Err(e) = self.data.tasks_mut().set_status(&id, TaskStatus::Completed) {
            return Response::Error { message: e };
        }
        self.roadmap_write_back(&id, &TaskStatus::Completed);
        Response::Ok {
            output: format!("Task '{}' marked completed", id),
        }
    }

    fn cmd_task_uncheck(&mut self, id: String) -> Response {
        if let Err(e) = self.data.tasks_mut().set_status(&id, TaskStatus::Pending) {
            return Response::Error { message: e };
        }
        self.roadmap_write_back(&id, &TaskStatus::Pending);
        Response::Ok {
            output: format!("Task '{}' marked pending", id),
        }
    }

    fn cmd_config_load(&mut self, path: Option<String>) -> Response {
        let path = path
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| self.data.config_dir().join("settings.yaml"));
        match crate::data::settings::load(&path) {
            Ok(loaded) => {
                self.settings = loaded;
                Response::Ok {
                    output: format!("Settings loaded from {}", path.display()),
                }
            }
            Err(e) => Response::Error { message: e },
        }
    }

    fn cmd_config_save(&self, path: Option<String>) -> Response {
        let path = path
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| self.data.config_dir().join("settings.yaml"));
        match crate::data::settings::save(&path, &self.settings) {
            Ok(()) => Response::Ok {
                output: format!("Settings saved to {}", path.display()),
            },
            Err(e) => Response::Error { message: e },
        }
    }

    fn cmd_config_add(&mut self, key: String, value: String) -> Response {
        match key.as_str() {
            "project_root" => self.settings.project_root = value.clone(),
            "max_retries" => match value.parse::<u32>() {
                Ok(n) => self.settings.max_retries = n,
                Err(_) => {
                    return Response::Error {
                        message: format!("Invalid u32 for max_retries: {}", value),
                    }
                }
            },
            "health_check_interval" => match value.parse::<u64>() {
                Ok(n) => self.settings.health_check_interval = n,
                Err(_) => {
                    return Response::Error {
                        message: format!("Invalid u64: {}", value),
                    }
                }
            },
            "heartbeat_timeout" => match value.parse::<u64>() {
                Ok(n) => self.settings.heartbeat_timeout = n,
                Err(_) => {
                    return Response::Error {
                        message: format!("Invalid u64: {}", value),
                    }
                }
            },
            "message_timeout" => match value.parse::<u64>() {
                Ok(n) => self.settings.message_timeout = n,
                Err(_) => {
                    return Response::Error {
                        message: format!("Invalid u64: {}", value),
                    }
                }
            },
            "escalation_timeout" => match value.parse::<u64>() {
                Ok(n) => self.settings.escalation_timeout = n,
                Err(_) => {
                    return Response::Error {
                        message: format!("Invalid u64: {}", value),
                    }
                }
            },
            _ => {
                return Response::Error {
                    message: format!("Unknown config key: {}", key),
                }
            }
        }
        Response::Ok {
            output: format!("Config '{}' set to '{}'", key, value),
        }
    }

    fn cmd_config_list(&self) -> Response {
        let text = crate::data::settings::serialize(&self.settings);
        Response::Ok { output: text }
    }

    fn cmd_project_add(&mut self, name: String, path: String) -> Response {
        let entry = FolderEntry {
            name: name.clone(),
            path: path.clone(),
        };
        if let Err(e) = self.data.folders_mut().add(entry) {
            return Response::Error { message: e };
        }
        // Also add a root task node for this project
        let task = TaskNode {
            id: name.clone(),
            title: name.clone(),
            source: TaskSource::Filesystem,
            status: TaskStatus::Pending,
            result: None,
            agent: None,
            children: vec![],
            spec_path: Some(path.clone()),
        };
        let _ = self.data.tasks_mut().add_root(task);

        // Register project skill source if skills/ subfolder exists
        let skills_dir = PathBuf::from(&path).join("skills");
        if skills_dir.is_dir() {
            let source = LibrarySource {
                kind: SourceKind::Project(name.clone()),
                library_type: LibraryType::SkillsOnly,
                path: skills_dir,
                priority: 25,
            };
            let _ = self.library.add_source(source);
        }

        Response::Ok {
            output: format!("Project '{}' added", name),
        }
    }

    fn cmd_project_remove(&mut self, name: String) -> Response {
        if let Err(e) = self.data.folders_mut().remove(&name) {
            return Response::Error { message: e };
        }
        // Rebuild library without the removed project's skills
        let lib_config = build_library_config(&self.data);
        self.library = Library::new(&lib_config).unwrap_or_else(|_| Library::empty());
        Response::Ok {
            output: format!("Project '{}' removed", name),
        }
    }

    fn cmd_project_list(&self, format: Option<String>) -> Response {
        let folders = self.data.folders().list();
        if format.as_deref() == Some("json") {
            let json = serde_json::to_string_pretty(folders).unwrap_or_else(|_| "[]".into());
            return Response::Ok { output: json };
        }
        if folders.is_empty() {
            return Response::Ok {
                output: "No projects".into(),
            };
        }
        let lines: Vec<String> = folders
            .iter()
            .map(|f| format!("{:<20} {}", f.name, f.path))
            .collect();
        Response::Ok {
            output: lines.join("\n"),
        }
    }

    fn cmd_project_scan(&mut self, name: String) -> Response {
        let folder = match self.data.folders().get(&name) {
            Some(f) => f.clone(),
            None => {
                return Response::Error {
                    message: format!("Project '{}' not found", name),
                }
            }
        };
        let path = std::path::Path::new(&folder.path);
        match crate::data::scanner::scan_tasks(path) {
            Ok(tasks) => {
                let count = tasks.len();
                for t in tasks {
                    let _ = self.data.tasks_mut().add_root(t);
                }
                Response::Ok {
                    output: format!("Scanned project '{}': {} tasks found", name, count),
                }
            }
            Err(e) => Response::Error { message: e },
        }
    }

    fn cmd_roadmap_load(&mut self, path: String) -> Response {
        let file_path = std::path::PathBuf::from(&path);
        let content = match std::fs::read_to_string(&file_path) {
            Ok(c) => c,
            Err(e) => {
                return Response::Error {
                    message: format!("cannot read '{}': {}", path, e),
                }
            }
        };
        match crate::data::roadmap::parse(&content) {
            Ok(tasks) => {
                let count = tasks.len();
                for t in tasks {
                    let _ = self.data.tasks_mut().add_root(t);
                }
                self.data.add_roadmap_path(file_path);
                Response::Ok {
                    output: format!("Loaded roadmap '{}': {} top-level tasks", path, count),
                }
            }
            Err(e) => Response::Error { message: e },
        }
    }

    /// Write back a task's status change to all loaded roadmap files.
    fn roadmap_write_back(&self, task_id: &str, status: &TaskStatus) {
        for roadmap_path in self.data.roadmap_paths() {
            let content = match std::fs::read_to_string(roadmap_path) {
                Ok(c) => c,
                Err(_) => continue,
            };
            match crate::data::roadmap::update_status_in_place(&content, task_id, status) {
                Ok(updated) => {
                    let _ = std::fs::write(roadmap_path, updated);
                }
                Err(_) => continue, // Task not in this roadmap file
            }
        }
    }

    // -----------------------------------------------------------------------
    // Pool command handlers
    // -----------------------------------------------------------------------

    fn cmd_pool_list(&self) -> Response {
        let configs = self.pool.list_configs();
        if configs.is_empty() {
            return Response::Ok {
                output: "No pools configured".into(),
            };
        }
        let mut lines = Vec::new();
        for (role, cfg) in &configs {
            let state = self.pool.pool_state(role, self.data.agents());
            let (idle, busy, total) = match state {
                Some(s) => (s.idle_count, s.busy_count, s.total),
                None => (0, 0, 0),
            };
            lines.push(format!(
                "{}: {}/{} idle, {}/{} busy (target: {}, max: {})",
                role, idle, total, busy, total, cfg.target_size, cfg.max_size
            ));
        }
        Response::Ok {
            output: lines.join("\n"),
        }
    }

    fn cmd_pool_status(&self, role: String) -> Response {
        match self.pool.pool_state(&role, self.data.agents()) {
            Some(state) => Response::Ok {
                output: format!(
                    "Pool '{}': {} idle, {} busy, {} spawning, {} total (target: {})",
                    role, state.idle_count, state.busy_count,
                    state.spawning_count, state.total, state.config.target_size
                ),
            },
            None => Response::Error {
                message: format!("No pool configured for role '{}'", role),
            },
        }
    }

    fn cmd_pool_set(&mut self, role: String, size: u32, path: Option<String>) -> Response {
        let path = path.unwrap_or_else(|| self.settings.project_root.clone());
        self.pool.set_pool(&role, PoolConfig {
            target_size: size,
            auto_expand: self.settings.pool_auto_expand,
            max_size: size * 2,
            path: path.clone(),
        });
        // Compute deficit and create agents one at a time so next_name() sees
        // previously added agents and generates unique sequential names.
        let deficit = self.pool.deficit(&role, self.data.agents());
        let mut spawned = 0u32;
        for _ in 0..deficit {
            let name = self.data.agents().next_name(&role);
            let agent = Agent {
                name,
                role: role.clone(),
                agent_type: AgentType::Claude,
                task: None,
                path: path.clone(),
                status: AgentStatus::Idle,
                status_notes: "pool member".into(),
                health: HealthState::Unknown,
                last_heartbeat_ms: None,
                session: None,
            };
            if self.data.agents_mut().add(agent).is_ok() {
                spawned += 1;
            }
        }
        Response::Ok {
            output: format!("Pool '{}' set to {} (spawned {} new workers)", role, size, spawned),
        }
    }

    fn cmd_pool_remove(&mut self, role: String) -> Response {
        if self.pool.remove_pool(&role) {
            Response::Ok {
                output: format!("Pool '{}' removed", role),
            }
        } else {
            Response::Error {
                message: format!("No pool configured for role '{}'", role),
            }
        }
    }

    // -----------------------------------------------------------------------
    // Messaging command handlers
    // -----------------------------------------------------------------------

    fn cmd_tell(&mut self, agent: String, text: String) -> Response {
        // Verify agent exists
        if self.data.agents().get(&agent).is_none() {
            return Response::Error {
                message: format!("Agent '{}' not found", agent),
            };
        }
        let msg = Message {
            sender: "user".into(),
            recipient: agent.clone(),
            text: text.clone(),
            queued_at_ms: now_ms(),
            delivered_at_ms: None,
        };
        self.data.messages_mut().enqueue(msg);
        self.actions.push(Action::SendKeys {
            target: agent.clone(),
            keys: text,
        });
        Response::Ok {
            output: format!("Message queued for '{}'", agent),
        }
    }

    fn cmd_interrupt(&mut self, agent: String, text: Option<String>) -> Response {
        if self.data.agents().get(&agent).is_none() {
            return Response::Error {
                message: format!("Agent '{}' not found", agent),
            };
        }
        let text = text.unwrap_or_default();
        // Send Ctrl-C followed by the text
        self.actions.push(Action::SendKeys {
            target: agent.clone(),
            keys: "C-c".into(),
        });
        if !text.is_empty() {
            self.actions.push(Action::SendKeys {
                target: agent.clone(),
                keys: text,
            });
        }
        Response::Ok {
            output: format!("Interrupt sent to '{}'", agent),
        }
    }

    // Layout and Client methods removed — handled by MuxUX.

    // -----------------------------------------------------------------------
    // Diagnosis command handlers
    // -----------------------------------------------------------------------

    fn cmd_diagnosis_report(&self) -> Response {
        match DiagnosisEngine::new(self.data.config_dir().to_path_buf()) {
            Ok(engine) => Response::Ok {
                output: engine.generate_report(),
            },
            Err(e) => Response::Error {
                message: format!("Failed to load diagnosis data: {}", e),
            },
        }
    }

    fn cmd_diagnosis_reliability(
        &self,
        signal: Option<String>,
        format: Option<String>,
    ) -> Response {
        let engine = match DiagnosisEngine::new(self.data.config_dir().to_path_buf()) {
            Ok(e) => e,
            Err(e) => {
                return Response::Error {
                    message: format!("Failed to load diagnosis data: {}", e),
                }
            }
        };

        if let Some(signal_str) = signal {
            let signal_type = match parse_signal_type(&signal_str) {
                Ok(s) => s,
                Err(e) => return Response::Error { message: e },
            };
            match engine.signal_reliability(&signal_type) {
                Some(rel) => {
                    if format.as_deref() == Some("json") {
                        let json = serde_json::to_string_pretty(rel)
                            .unwrap_or_else(|_| "{}".into());
                        Response::Ok { output: json }
                    } else {
                        Response::Ok {
                            output: format_reliability_table(&[rel]),
                        }
                    }
                }
                None => Response::Ok {
                    output: format!("No reliability data for signal '{}'", signal_str),
                },
            }
        } else {
            let all = engine.all_reliability();
            if all.is_empty() {
                return Response::Ok {
                    output: "No reliability data recorded yet.".into(),
                };
            }
            if format.as_deref() == Some("json") {
                let json = serde_json::to_string_pretty(&all)
                    .unwrap_or_else(|_| "[]".into());
                Response::Ok { output: json }
            } else {
                Response::Ok {
                    output: format_reliability_table(&all),
                }
            }
        }
    }

    fn cmd_diagnosis_effectiveness(
        &self,
        signal: Option<String>,
        format: Option<String>,
    ) -> Response {
        let engine = match DiagnosisEngine::new(self.data.config_dir().to_path_buf()) {
            Ok(e) => e,
            Err(e) => {
                return Response::Error {
                    message: format!("Failed to load diagnosis data: {}", e),
                }
            }
        };

        let all_rel = engine.all_reliability();
        let mut entries = Vec::new();

        let target_signal = if let Some(ref s) = signal {
            match parse_signal_type(s) {
                Ok(st) => Some(st),
                Err(e) => return Response::Error { message: e },
            }
        } else {
            None
        };

        // Collect all effectiveness entries for each signal
        let actions = [
            crate::diagnosis::InterventionAction::Retry,
            crate::diagnosis::InterventionAction::Restart,
            crate::diagnosis::InterventionAction::Escalate,
            crate::diagnosis::InterventionAction::Redesign,
            crate::diagnosis::InterventionAction::Ignore,
        ];

        for rel in &all_rel {
            if let Some(ref target) = target_signal {
                if rel.signal != *target {
                    continue;
                }
            }
            for action in &actions {
                if let Some(eff) = engine.action_effectiveness(&rel.signal, action) {
                    entries.push(eff.clone());
                }
            }
        }

        if entries.is_empty() {
            return Response::Ok {
                output: "No effectiveness data recorded yet.".into(),
            };
        }

        if format.as_deref() == Some("json") {
            let json = serde_json::to_string_pretty(&entries)
                .unwrap_or_else(|_| "[]".into());
            Response::Ok { output: json }
        } else {
            Response::Ok {
                output: format_effectiveness_table(&entries),
            }
        }
    }

    fn cmd_diagnosis_thresholds(&self, format: Option<String>) -> Response {
        let engine = match DiagnosisEngine::new(self.data.config_dir().to_path_buf()) {
            Ok(e) => e,
            Err(e) => {
                return Response::Error {
                    message: format!("Failed to load diagnosis data: {}", e),
                }
            }
        };

        let thresholds = engine.all_thresholds();
        if thresholds.is_empty() {
            return Response::Ok {
                output: "No thresholds computed yet. Thresholds are computed during monitoring cycles.".into(),
            };
        }

        let mut entries: Vec<_> = thresholds.values().collect();
        entries.sort_by(|a, b| a.signal.to_string().cmp(&b.signal.to_string()));

        if format.as_deref() == Some("json") {
            let json = serde_json::to_string_pretty(&entries)
                .unwrap_or_else(|_| "[]".into());
            Response::Ok { output: json }
        } else {
            let mut lines = Vec::new();
            lines.push(format!(
                "{:<24} {:>12} {:>12} {:>8} {}",
                "Signal", "Base (ms)", "Adjusted (ms)", "Score", "Reason"
            ));
            lines.push("-".repeat(80));
            for t in &entries {
                lines.push(format!(
                    "{:<24} {:>12} {:>12} {:>8.2} {}",
                    t.signal.to_string(),
                    t.base_timeout_ms,
                    t.adjusted_timeout_ms,
                    t.reliability_score,
                    t.adjustment_reason
                ));
            }
            Response::Ok {
                output: lines.join("\n"),
            }
        }
    }

    fn cmd_diagnosis_events(
        &self,
        limit: Option<String>,
        format: Option<String>,
    ) -> Response {
        let engine = match DiagnosisEngine::new(self.data.config_dir().to_path_buf()) {
            Ok(e) => e,
            Err(e) => {
                return Response::Error {
                    message: format!("Failed to load diagnosis data: {}", e),
                }
            }
        };

        let n = limit
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(20);
        let events = engine.recent_events(n);

        if events.is_empty() {
            return Response::Ok {
                output: "No intervention events recorded.".into(),
            };
        }

        if format.as_deref() == Some("json") {
            let json = serde_json::to_string_pretty(events)
                .unwrap_or_else(|_| "[]".into());
            Response::Ok { output: json }
        } else {
            let mut lines = Vec::new();
            lines.push(format!(
                "{:<6} {:>14} {:<12} {:<24} {:<10} {:<14} {:>10}",
                "ID", "Time", "Agent", "Signal", "Action", "Outcome", "Duration"
            ));
            lines.push("-".repeat(96));
            for e in events {
                lines.push(format!(
                    "{:<6} {:>14} {:<12} {:<24} {:<10} {:<14} {:>10}",
                    e.id,
                    e.timestamp_ms,
                    e.agent,
                    e.signal.to_string(),
                    e.action.to_string(),
                    format!("{:?}", e.outcome).to_lowercase(),
                    format!("{}ms", e.duration_ms)
                ));
            }
            Response::Ok {
                output: lines.join("\n"),
            }
        }
    }

    fn cmd_help(&self, topic: Option<String>) -> Response {
        let text = crate::help::help_text(topic.as_deref());
        Response::Ok { output: text }
    }

    // -----------------------------------------------------------------------
    // Rig command handlers
    // -----------------------------------------------------------------------

    fn cmd_rig_init(&mut self, host: String, name: Option<String>) -> Response {
        let remote_name = name.unwrap_or_else(|| "default".into());
        let config = parse_host_string(&host, &remote_name);
        if let Some(rig) = &mut self.rig {
            let _ = rig.registry.add(config);
            match rig.init_remote(&remote_name) {
                Ok(msg) => Response::Ok { output: msg },
                Err(e) => Response::Error { message: e },
            }
        } else {
            Response::Error { message: "Rig not initialized".into() }
        }
    }

    fn cmd_rig_push(&mut self, folder: String, remote: Option<String>) -> Response {
        if let Some(rig) = &mut self.rig {
            let name = match remote {
                Some(n) => n,
                None => match rig.registry.default_name() {
                    Some(d) => d.to_string(),
                    None => return Response::Error { message: "No remote specified and no default set".into() },
                },
            };
            match rig.push(&name, &folder) {
                Ok(msg) => Response::Ok { output: msg },
                Err(e) => Response::Error { message: e },
            }
        } else {
            Response::Error { message: "Rig not initialized".into() }
        }
    }

    fn cmd_rig_pull(&mut self, folder: String, remote: Option<String>) -> Response {
        if let Some(rig) = &mut self.rig {
            let name = match remote {
                Some(n) => n,
                None => match rig.registry.default_name() {
                    Some(d) => d.to_string(),
                    None => return Response::Error { message: "No remote specified and no default set".into() },
                },
            };
            match rig.pull(&name, &folder) {
                Ok(msg) => Response::Ok { output: msg },
                Err(e) => Response::Error { message: e },
            }
        } else {
            Response::Error { message: "Rig not initialized".into() }
        }
    }

    fn cmd_rig_status(&self, remote: Option<String>) -> Response {
        if let Some(rig) = &self.rig {
            let name = match remote {
                Some(n) => n,
                None => match rig.registry.default_name() {
                    Some(d) => d.to_string(),
                    None => return Response::Error { message: "No remote specified and no default set".into() },
                },
            };
            match rig.status(&name) {
                Ok(msg) => Response::Ok { output: msg },
                Err(e) => Response::Error { message: e },
            }
        } else {
            Response::Error { message: "Rig not initialized".into() }
        }
    }

    fn cmd_rig_health(&mut self, remote: Option<String>) -> Response {
        if let Some(rig) = &mut self.rig {
            let name = match remote {
                Some(n) => n,
                None => match rig.registry.default_name() {
                    Some(d) => d.to_string(),
                    None => return Response::Error { message: "No remote specified and no default set".into() },
                },
            };
            match rig.health_check(&name) {
                Ok(msg) => Response::Ok { output: msg },
                Err(e) => Response::Error { message: e },
            }
        } else {
            Response::Error { message: "Rig not initialized".into() }
        }
    }

    fn cmd_rig_stop(&mut self, remote: Option<String>) -> Response {
        if let Some(rig) = &mut self.rig {
            let name = match remote {
                Some(n) => n,
                None => match rig.registry.default_name() {
                    Some(d) => d.to_string(),
                    None => return Response::Error { message: "No remote specified and no default set".into() },
                },
            };
            match rig.stop(&name) {
                Ok(msg) => Response::Ok { output: msg },
                Err(e) => Response::Error { message: e },
            }
        } else {
            Response::Error { message: "Rig not initialized".into() }
        }
    }

    fn cmd_rig_list(&self) -> Response {
        if let Some(rig) = &self.rig {
            let remotes = rig.registry.list();
            if remotes.is_empty() {
                return Response::Ok { output: "No remotes configured".into() };
            }
            let default_name = rig.registry.default_name();
            let mut lines = Vec::new();
            for r in remotes {
                let marker = if default_name == Some(&r.name) { " *" } else { "" };
                lines.push(format!("{}{:<16} {}:{} ({})", marker, r.name, r.host, r.port, r.user_at_host()));
            }
            Response::Ok { output: lines.join("\n") }
        } else {
            Response::Error { message: "Rig not initialized".into() }
        }
    }

    fn cmd_rig_default(&mut self, name: Option<String>) -> Response {
        if let Some(rig) = &mut self.rig {
            match name {
                Some(n) => match rig.registry.set_default(&n) {
                    Ok(()) => Response::Ok { output: format!("Default remote set to '{}'", n) },
                    Err(e) => Response::Error { message: e },
                },
                None => match rig.registry.default_name() {
                    Some(d) => Response::Ok { output: format!("Default remote: {}", d) },
                    None => Response::Ok { output: "No default remote set".into() },
                },
            }
        } else {
            Response::Error { message: "Rig not initialized".into() }
        }
    }

    // -----------------------------------------------------------------------
    // History command handlers
    // -----------------------------------------------------------------------

    fn cmd_history_list(&self, limit: Option<String>, format: Option<String>) -> Response {
        let mgr = match HistoryManager::with_defaults(self.data.config_dir().to_path_buf()) {
            Ok(m) => m,
            Err(e) => return Response::Error { message: format!("Failed to init history: {}", e) },
        };
        let entries = match mgr.list() {
            Ok(e) => e,
            Err(e) => return Response::Error { message: format!("Failed to list history: {}", e) },
        };
        let max = limit
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(20);
        let entries: Vec<_> = entries.into_iter().take(max).collect();

        if format.as_deref() == Some("json") {
            match serde_json::to_string_pretty(&entries) {
                Ok(json) => Response::Ok { output: json },
                Err(e) => Response::Error { message: format!("JSON serialization failed: {}", e) },
            }
        } else {
            if entries.is_empty() {
                return Response::Ok { output: "No history snapshots found".into() };
            }
            let mut lines = Vec::new();
            lines.push(format!("{:<6} {:<28} {:>12} {:>10}", "Index", "Filename", "Timestamp", "Size"));
            lines.push("-".repeat(60));
            for (i, e) in entries.iter().enumerate() {
                lines.push(format!(
                    "{:<6} {:<28} {:>12} {:>8}B",
                    i, e.filename, e.timestamp_ms, e.size_bytes
                ));
            }
            Response::Ok { output: lines.join("\n") }
        }
    }

    fn cmd_history_show(&self, id: String) -> Response {
        let mgr = match HistoryManager::with_defaults(self.data.config_dir().to_path_buf()) {
            Ok(m) => m,
            Err(e) => return Response::Error { message: format!("Failed to init history: {}", e) },
        };
        let entries = match mgr.list() {
            Ok(e) => e,
            Err(e) => return Response::Error { message: format!("Failed to list history: {}", e) },
        };
        let entry = match resolve_history_entry(&entries, &id) {
            Ok(e) => e,
            Err(msg) => return Response::Error { message: msg },
        };
        match mgr.read(&entry) {
            Ok(content) => Response::Ok { output: content },
            Err(e) => Response::Error { message: format!("Failed to read snapshot: {}", e) },
        }
    }

    fn cmd_history_diff(&self, from: String, to: Option<String>) -> Response {
        let mgr = match HistoryManager::with_defaults(self.data.config_dir().to_path_buf()) {
            Ok(m) => m,
            Err(e) => return Response::Error { message: format!("Failed to init history: {}", e) },
        };
        let entries = match mgr.list() {
            Ok(e) => e,
            Err(e) => return Response::Error { message: format!("Failed to list history: {}", e) },
        };
        let from_entry = match resolve_history_entry(&entries, &from) {
            Ok(e) => e,
            Err(msg) => return Response::Error { message: msg },
        };

        let to_entry = if let Some(to_id) = to {
            match resolve_history_entry(&entries, &to_id) {
                Ok(e) => e,
                Err(msg) => return Response::Error { message: msg },
            }
        } else {
            // Diff against current config: take a temporary snapshot.
            let now = now_ms();
            match mgr.maybe_snapshot(now) {
                Ok(Some(e)) => e,
                Ok(None) => {
                    // Config unchanged from latest — use the latest entry.
                    match entries.first() {
                        Some(e) => e.clone(),
                        None => return Response::Error { message: "No history entries to diff against".into() },
                    }
                }
                Err(e) => return Response::Error { message: format!("Failed to snapshot current config: {}", e) },
            }
        };

        match mgr.diff(&from_entry, &to_entry) {
            Ok(diff) => {
                let mut lines = Vec::new();
                lines.push(format!("From: {} -> To: {}", diff.from.filename, diff.to.filename));
                lines.push(format!("Summary: {}", diff.summary));
                if !diff.added_lines.is_empty() {
                    lines.push(String::new());
                    lines.push("Added:".into());
                    for line in &diff.added_lines {
                        lines.push(format!("+ {}", line));
                    }
                }
                if !diff.removed_lines.is_empty() {
                    lines.push(String::new());
                    lines.push("Removed:".into());
                    for line in &diff.removed_lines {
                        lines.push(format!("- {}", line));
                    }
                }
                Response::Ok { output: lines.join("\n") }
            }
            Err(e) => Response::Error { message: format!("Failed to compute diff: {}", e) },
        }
    }

    fn cmd_history_restore(&mut self, id: String) -> Response {
        let mgr = match HistoryManager::with_defaults(self.data.config_dir().to_path_buf()) {
            Ok(m) => m,
            Err(e) => return Response::Error { message: format!("Failed to init history: {}", e) },
        };
        let entries = match mgr.list() {
            Ok(e) => e,
            Err(e) => return Response::Error { message: format!("Failed to list history: {}", e) },
        };
        let entry = match resolve_history_entry(&entries, &id) {
            Ok(e) => e,
            Err(msg) => return Response::Error { message: msg },
        };
        let now = now_ms();
        match mgr.restore(&entry, now) {
            Ok(()) => Response::Ok {
                output: format!("Restored configuration from {}", entry.filename),
            },
            Err(e) => Response::Error { message: format!("Restore failed: {}", e) },
        }
    }

    fn cmd_history_snapshot(&self) -> Response {
        let mgr = match HistoryManager::with_defaults(self.data.config_dir().to_path_buf()) {
            Ok(m) => m,
            Err(e) => return Response::Error { message: format!("Failed to init history: {}", e) },
        };
        let now = now_ms();
        match mgr.maybe_snapshot(now) {
            Ok(Some(entry)) => Response::Ok {
                output: format!("Snapshot created: {}", entry.filename),
            },
            Ok(None) => Response::Ok {
                output: "No changes to snapshot (configuration unchanged)".into(),
            },
            Err(e) => Response::Error { message: format!("Snapshot failed: {}", e) },
        }
    }

    fn cmd_history_prune(&self) -> Response {
        let mgr = match HistoryManager::with_defaults(self.data.config_dir().to_path_buf()) {
            Ok(m) => m,
            Err(e) => return Response::Error { message: format!("Failed to init history: {}", e) },
        };
        let now = now_ms();
        match mgr.prune(now) {
            Ok(count) => Response::Ok {
                output: format!("Pruned {} history entries", count),
            },
            Err(e) => Response::Error { message: format!("Prune failed: {}", e) },
        }
    }
}


// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Parse a string into a TaskStatus.
fn parse_task_status(s: &str) -> Result<TaskStatus, String> {
    match s.to_lowercase().as_str() {
        "pending" => Ok(TaskStatus::Pending),
        "in_progress" | "inprogress" | "in-progress" => Ok(TaskStatus::InProgress),
        "completed" | "done" => Ok(TaskStatus::Completed),
        "failed" | "fail" => Ok(TaskStatus::Failed),
        "paused" => Ok(TaskStatus::Paused),
        "cancelled" | "canceled" => Ok(TaskStatus::Cancelled),
        _ => Err(format!("Unknown task status: '{}'", s)),
    }
}

/// Parse a host string like "user@host:port" or "host" into a RemoteConfig.
fn parse_host_string(host_str: &str, name: &str) -> RemoteConfig {
    let (user, rest) = if host_str.contains('@') {
        let parts: Vec<&str> = host_str.splitn(2, '@').collect();
        (parts[0].to_string(), parts[1].to_string())
    } else {
        ("root".to_string(), host_str.to_string())
    };
    let (host, port) = if rest.contains(':') {
        let parts: Vec<&str> = rest.splitn(2, ':').collect();
        let port = parts[1].parse::<u16>().unwrap_or(22);
        (parts[0].to_string(), port)
    } else {
        (rest, 22)
    };
    RemoteConfig {
        name: name.to_string(),
        host,
        port,
        user,
        ssh_key: None,
        workspace_dir: "/home/ubuntu/work".to_string(),
        gpu_count: None,
        labels: Vec::new(),
    }
}

/// Simple wall-clock milliseconds.
fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Resolve a history ID (index or filename) to a HistoryEntry.
fn resolve_history_entry(entries: &[HistoryEntry], id: &str) -> Result<HistoryEntry, String> {
    if let Ok(idx) = id.parse::<usize>() {
        entries.get(idx).cloned().ok_or_else(|| {
            format!("History index {} out of range (have {} entries)", idx, entries.len())
        })
    } else {
        entries.iter().find(|e| e.filename == id).cloned()
            .ok_or_else(|| format!("History entry '{}' not found", id))
    }
}


// ---------------------------------------------------------------------------
// Diagnosis formatting helpers
// ---------------------------------------------------------------------------

fn format_reliability_table(entries: &[&crate::diagnosis::SignalReliability]) -> String {
    let mut lines = Vec::new();
    lines.push(format!(
        "{:<24} {:>6} {:>6} {:>6} {:>8} {:>8} {:>14}",
        "Signal", "Fires", "TP", "FP", "Unknown", "Score", "Avg Resolution"
    ));
    lines.push("-".repeat(80));
    for r in entries {
        lines.push(format!(
            "{:<24} {:>6} {:>6} {:>6} {:>8} {:>8.2} {:>12}ms",
            r.signal.to_string(),
            r.total_fires,
            r.true_positives,
            r.false_positives,
            r.unknown,
            r.reliability_score,
            r.avg_resolution_ms
        ));
    }
    lines.join("\n")
}

fn format_effectiveness_table(entries: &[crate::diagnosis::ActionEffectiveness]) -> String {
    let mut lines = Vec::new();
    lines.push(format!(
        "{:<24} {:<10} {:>10} {:>10} {:>10} {:>8}",
        "Signal", "Action", "Attempts", "Successes", "Failures", "Rate"
    ));
    lines.push("-".repeat(78));
    for e in entries {
        lines.push(format!(
            "{:<24} {:<10} {:>10} {:>10} {:>10} {:>7.1}%",
            e.signal.to_string(),
            e.action.to_string(),
            e.attempts,
            e.successes,
            e.failures,
            e.success_rate * 100.0
        ));
    }
    lines.join("\n")
}

/// Parse a signal type string into a `SignalType` enum variant.
fn parse_signal_type(s: &str) -> Result<SignalType, String> {
    match s.to_lowercase().as_str() {
        "heartbeat_stale" => Ok(SignalType::HeartbeatStale),
        "error_pattern" => Ok(SignalType::ErrorPattern),
        "output_stall" => Ok(SignalType::OutputStall),
        "ssh_disconnected" => Ok(SignalType::SshDisconnected),
        "explicit_error" => Ok(SignalType::ExplicitError),
        "manual_escalation" => Ok(SignalType::ManualEscalation),
        other => {
            if other.starts_with("trigger_fired") {
                let name = other
                    .strip_prefix("trigger_fired")
                    .unwrap_or("")
                    .trim_start_matches(|c: char| c == '(' || c == ' ')
                    .trim_end_matches(|c: char| c == ')' || c == ' ');
                Ok(SignalType::TriggerFired(name.to_string()))
            } else {
                Err(format!(
                    "Unknown signal type: '{}'. Valid types: heartbeat_stale, error_pattern, \
                     output_stall, ssh_disconnected, explicit_error, trigger_fired(<name>), \
                     manual_escalation",
                    s
                ))
            }
        }
    }
}


// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn test_sys() -> Sys {
        let data = Data::new(Path::new("/tmp/cmx-test-nonexistent-999")).unwrap();
        Sys::from_data(data)
    }

    fn is_ok(r: &Response) -> bool {
        matches!(r, Response::Ok { .. })
    }

    fn is_err(r: &Response) -> bool {
        matches!(r, Response::Error { .. })
    }

    fn output(r: &Response) -> &str {
        match r {
            Response::Ok { output } => output,
            Response::Error { message } => message,
        }
    }

    // --- status ---

    #[test]
    fn status_empty() {
        let mut sys = test_sys();
        let r = sys.execute(Command::Status { format: None });
        assert!(is_ok(&r));
        assert!(output(&r).contains("agents: 0"));
    }

    #[test]
    fn status_with_agents() {
        let mut sys = test_sys();
        sys.execute(Command::AgentNew {
            role: "worker".into(),
            name: None,
            path: None,
            agent_type: None,
        });
        let r = sys.execute(Command::Status { format: None });
        assert!(output(&r).contains("agents: 1"));
    }

    // --- agent lifecycle ---

    #[test]
    fn agent_new_default_name() {
        let mut sys = test_sys();
        let r = sys.execute(Command::AgentNew {
            role: "worker".into(),
            name: None,
            path: None,
            agent_type: None,
        });
        assert!(is_ok(&r));
        assert!(output(&r).contains("worker1"));
        assert_eq!(sys.data.agents().list().len(), 1);
    }

    #[test]
    fn agent_new_custom_name() {
        let mut sys = test_sys();
        let r = sys.execute(Command::AgentNew {
            role: "pilot".into(),
            name: Some("my-pilot".into()),
            path: None,
            agent_type: None,
        });
        assert!(is_ok(&r));
        assert!(output(&r).contains("my-pilot"));
    }

    #[test]
    fn agent_new_emits_action() {
        let mut sys = test_sys();
        sys.execute(Command::AgentNew {
            role: "worker".into(),
            name: Some("w1".into()),
            path: None,
            agent_type: None,
        });
        assert_eq!(sys.pending_actions().len(), 1);
        match &sys.pending_actions()[0] {
            Action::CreateAgent { name, role, .. } => {
                assert_eq!(name, "w1");
                assert_eq!(role, "worker");
            }
            _ => panic!("Expected CreateAgent action"),
        }
    }

    #[test]
    fn agent_new_duplicate_fails() {
        let mut sys = test_sys();
        sys.execute(Command::AgentNew {
            role: "worker".into(),
            name: Some("w1".into()),
            path: None,
            agent_type: None,
        });
        let r = sys.execute(Command::AgentNew {
            role: "worker".into(),
            name: Some("w1".into()),
            path: None,
            agent_type: None,
        });
        assert!(is_err(&r));
    }

    #[test]
    fn agent_new_with_type() {
        let mut sys = test_sys();
        let r = sys.execute(Command::AgentNew {
            role: "worker".into(),
            name: Some("ssh1".into()),
            path: None,
            agent_type: Some("ssh".into()),
        });
        assert!(is_ok(&r));
        assert_eq!(
            sys.data.agents().get("ssh1").unwrap().agent_type,
            AgentType::Ssh
        );
    }

    #[test]
    fn agent_kill() {
        let mut sys = test_sys();
        sys.execute(Command::AgentNew {
            role: "worker".into(),
            name: Some("w1".into()),
            path: None,
            agent_type: None,
        });
        let r = sys.execute(Command::AgentKill { name: "w1".into() });
        assert!(is_ok(&r));
        assert!(sys.data.agents().list().is_empty());
    }

    #[test]
    fn agent_kill_nonexistent() {
        let mut sys = test_sys();
        let r = sys.execute(Command::AgentKill { name: "ghost".into() });
        assert!(is_err(&r));
    }

    #[test]
    fn agent_restart() {
        let mut sys = test_sys();
        sys.execute(Command::AgentNew {
            role: "worker".into(),
            name: Some("w1".into()),
            path: None,
            agent_type: None,
        });
        let r = sys.execute(Command::AgentRestart { name: "w1".into() });
        assert!(is_ok(&r));
        assert_eq!(sys.pending_actions().len(), 2); // KillAgent + CreateAgent
    }

    #[test]
    fn agent_assign_and_unassign() {
        let mut sys = test_sys();
        sys.execute(Command::AgentNew {
            role: "worker".into(),
            name: Some("w1".into()),
            path: None,
            agent_type: None,
        });
        let r = sys.execute(Command::AgentAssign {
            name: "w1".into(),
            task: "T1".into(),
        });
        assert!(is_ok(&r));
        assert_eq!(
            sys.data.agents().get("w1").unwrap().task.as_deref(),
            Some("T1")
        );

        let r = sys.execute(Command::AgentUnassign { name: "w1".into() });
        assert!(is_ok(&r));
        assert_eq!(sys.data.agents().get("w1").unwrap().task, None);
    }

    #[test]
    fn agent_status_update() {
        let mut sys = test_sys();
        sys.execute(Command::AgentNew {
            role: "worker".into(),
            name: Some("w1".into()),
            path: None,
            agent_type: None,
        });
        let r = sys.execute(Command::AgentStatus {
            name: "w1".into(),
            notes: Some("compiling".into()),
        });
        assert!(is_ok(&r));
        assert_eq!(
            sys.data.agents().get("w1").unwrap().status_notes,
            "compiling"
        );
    }

    #[test]
    fn agent_list_empty() {
        let mut sys = test_sys();
        let r = sys.execute(Command::AgentList { format: None });
        assert!(is_ok(&r));
        assert!(output(&r).contains("No agents"));
    }

    #[test]
    fn agent_list_json() {
        let mut sys = test_sys();
        sys.execute(Command::AgentNew {
            role: "worker".into(),
            name: Some("w1".into()),
            path: None,
            agent_type: None,
        });
        let r = sys.execute(Command::AgentList {
            format: Some("json".into()),
        });
        assert!(is_ok(&r));
        assert!(output(&r).contains("\"name\": \"w1\""));
    }

    // --- task lifecycle ---

    #[test]
    fn task_list_empty() {
        let mut sys = test_sys();
        let r = sys.execute(Command::TaskList {
            format: None,
            project: None,
        });
        assert!(is_ok(&r));
        assert!(output(&r).contains("No tasks"));
    }

    #[test]
    fn task_get_not_found() {
        let mut sys = test_sys();
        let r = sys.execute(Command::TaskGet { id: "NOPE".into() });
        assert!(is_err(&r));
    }

    #[test]
    fn task_check_and_uncheck() {
        let mut sys = test_sys();
        // Add a project which creates a root task
        sys.execute(Command::ProjectAdd {
            name: "PRJ".into(),
            path: "/tmp/prj".into(),
        });
        let r = sys.execute(Command::TaskCheck { id: "PRJ".into() });
        assert!(is_ok(&r));
        assert_eq!(
            sys.data.tasks().get("PRJ").unwrap().status,
            TaskStatus::Completed
        );

        let r = sys.execute(Command::TaskUncheck { id: "PRJ".into() });
        assert!(is_ok(&r));
        assert_eq!(
            sys.data.tasks().get("PRJ").unwrap().status,
            TaskStatus::Pending
        );
    }

    #[test]
    fn task_set_updates_fields() {
        let mut sys = test_sys();
        sys.execute(Command::ProjectAdd {
            name: "T1".into(),
            path: "/tmp/t1".into(),
        });
        let r = sys.execute(Command::TaskSet {
            id: "T1".into(),
            status: Some("in_progress".into()),
            title: Some("New Title".into()),
            result: None,
            agent: None,
        });
        assert!(is_ok(&r));
        let t = sys.data.tasks().get("T1").unwrap();
        assert_eq!(t.status, TaskStatus::InProgress);
        assert_eq!(t.title, "New Title");
    }

    #[test]
    fn task_set_invalid_status() {
        let mut sys = test_sys();
        sys.execute(Command::ProjectAdd {
            name: "T1".into(),
            path: "/tmp".into(),
        });
        let r = sys.execute(Command::TaskSet {
            id: "T1".into(),
            status: Some("bogus".into()),
            title: None,
            result: None,
            agent: None,
        });
        assert!(is_err(&r));
    }

    // --- view ---

    #[test]
    fn view_agent() {
        let mut sys = test_sys();
        sys.execute(Command::AgentNew {
            role: "pilot".into(),
            name: Some("p1".into()),
            path: None,
            agent_type: None,
        });
        let r = sys.execute(Command::View { name: "p1".into() });
        assert!(is_ok(&r));
        assert!(output(&r).contains("pilot"));
    }

    #[test]
    fn view_task() {
        let mut sys = test_sys();
        sys.execute(Command::ProjectAdd {
            name: "PRJ".into(),
            path: "/tmp".into(),
        });
        let r = sys.execute(Command::View { name: "PRJ".into() });
        assert!(is_ok(&r));
        assert!(output(&r).contains("PRJ"));
    }

    #[test]
    fn view_not_found() {
        let mut sys = test_sys();
        let r = sys.execute(Command::View { name: "ghost".into() });
        assert!(is_err(&r));
    }

    // --- messaging ---

    #[test]
    fn tell_queues_message() {
        let mut sys = test_sys();
        sys.execute(Command::AgentNew {
            role: "worker".into(),
            name: Some("w1".into()),
            path: None,
            agent_type: None,
        });
        let r = sys.execute(Command::Tell {
            agent: "w1".into(),
            text: "start task".into(),
        });
        assert!(is_ok(&r));
        assert_eq!(sys.data.messages().pending_for("w1").len(), 1);
        assert_eq!(sys.pending_actions().len(), 1);
    }

    #[test]
    fn tell_nonexistent_agent() {
        let mut sys = test_sys();
        let r = sys.execute(Command::Tell {
            agent: "ghost".into(),
            text: "hello".into(),
        });
        assert!(is_err(&r));
    }

    #[test]
    fn interrupt_sends_ctrl_c() {
        let mut sys = test_sys();
        sys.execute(Command::AgentNew {
            role: "worker".into(),
            name: Some("w1".into()),
            path: None,
            agent_type: None,
        });
        let r = sys.execute(Command::Interrupt {
            agent: "w1".into(),
            text: None,
        });
        assert!(is_ok(&r));
        assert!(sys.pending_actions().len() >= 1);
        match &sys.pending_actions()[0] {
            Action::SendKeys { keys, .. } => assert_eq!(keys, "C-c"),
            _ => panic!("Expected SendKeys"),
        }
    }

    #[test]
    fn interrupt_with_text() {
        let mut sys = test_sys();
        sys.execute(Command::AgentNew {
            role: "worker".into(),
            name: Some("w1".into()),
            path: None,
            agent_type: None,
        });
        let r = sys.execute(Command::Interrupt {
            agent: "w1".into(),
            text: Some("stop now".into()),
        });
        assert!(is_ok(&r));
        assert_eq!(sys.pending_actions().len(), 2);
    }

    // --- project lifecycle ---

    #[test]
    fn project_add_and_list() {
        let mut sys = test_sys();
        let r = sys.execute(Command::ProjectAdd {
            name: "myproj".into(),
            path: "/projects/my".into(),
        });
        assert!(is_ok(&r));
        let r = sys.execute(Command::ProjectList { format: None });
        assert!(is_ok(&r));
        assert!(output(&r).contains("myproj"));
    }

    #[test]
    fn project_remove() {
        let mut sys = test_sys();
        sys.execute(Command::ProjectAdd {
            name: "myproj".into(),
            path: "/tmp".into(),
        });
        let r = sys.execute(Command::ProjectRemove {
            name: "myproj".into(),
        });
        assert!(is_ok(&r));
        let r = sys.execute(Command::ProjectList { format: None });
        assert!(output(&r).contains("No projects"));
    }

    #[test]
    fn project_scan_found() {
        let mut sys = test_sys();
        sys.execute(Command::ProjectAdd {
            name: "myproj".into(),
            path: "/tmp".into(),
        });
        let r = sys.execute(Command::ProjectScan {
            name: "myproj".into(),
        });
        assert!(is_ok(&r));
        assert!(output(&r).contains("Scanned"));
    }

    #[test]
    fn project_scan_not_found() {
        let mut sys = test_sys();
        let r = sys.execute(Command::ProjectScan {
            name: "ghost".into(),
        });
        assert!(is_err(&r));
    }

    // --- config ---

    #[test]
    fn config_add_and_list() {
        let mut sys = test_sys();
        let r = sys.execute(Command::ConfigAdd {
            key: "max_retries".into(),
            value: "10".into(),
        });
        assert!(is_ok(&r));
        assert_eq!(sys.settings.max_retries, 10);
        let r = sys.execute(Command::ConfigList);
        assert!(is_ok(&r));
        assert!(output(&r).contains("max_retries: 10"));
    }

    #[test]
    fn config_add_unknown_key() {
        let mut sys = test_sys();
        let r = sys.execute(Command::ConfigAdd {
            key: "bogus_key".into(),
            value: "x".into(),
        });
        assert!(is_err(&r));
    }

    #[test]
    fn config_add_invalid_number() {
        let mut sys = test_sys();
        let r = sys.execute(Command::ConfigAdd {
            key: "max_retries".into(),
            value: "notanumber".into(),
        });
        assert!(is_err(&r));
    }

    // Layout tests removed — handled by MuxUX.

    // --- drain_actions ---

    #[test]
    fn drain_actions_clears() {
        let mut sys = test_sys();
        sys.execute(Command::AgentNew {
            role: "worker".into(),
            name: Some("w1".into()),
            path: None,
            agent_type: None,
        });
        assert!(!sys.pending_actions().is_empty());
        let drained = sys.drain_actions();
        assert!(!drained.is_empty());
        assert!(sys.pending_actions().is_empty());
    }

    #[test]
    fn actions_cleared_between_executes() {
        let mut sys = test_sys();
        sys.execute(Command::AgentNew {
            role: "worker".into(),
            name: Some("w1".into()),
            path: None,
            agent_type: None,
        });
        assert_eq!(sys.pending_actions().len(), 1);
        // Next execute clears previous actions
        sys.execute(Command::Status { format: None });
        assert!(sys.pending_actions().is_empty());
    }

    // --- parse_task_status ---

    #[test]
    fn parse_status_variants() {
        assert_eq!(parse_task_status("pending").unwrap(), TaskStatus::Pending);
        assert_eq!(
            parse_task_status("in_progress").unwrap(),
            TaskStatus::InProgress
        );
        assert_eq!(
            parse_task_status("in-progress").unwrap(),
            TaskStatus::InProgress
        );
        assert_eq!(
            parse_task_status("completed").unwrap(),
            TaskStatus::Completed
        );
        assert_eq!(parse_task_status("done").unwrap(), TaskStatus::Completed);
        assert_eq!(parse_task_status("failed").unwrap(), TaskStatus::Failed);
        assert_eq!(parse_task_status("paused").unwrap(), TaskStatus::Paused);
        assert_eq!(
            parse_task_status("cancelled").unwrap(),
            TaskStatus::Cancelled
        );
        assert_eq!(
            parse_task_status("canceled").unwrap(),
            TaskStatus::Cancelled
        );
        assert!(parse_task_status("bogus").is_err());
    }

    // --- parse_host_string ---

    #[test]
    fn parse_host_string_full() {
        let config = parse_host_string("ubuntu@10.0.0.1:2222", "r1");
        assert_eq!(config.name, "r1");
        assert_eq!(config.host, "10.0.0.1");
        assert_eq!(config.port, 2222);
        assert_eq!(config.user, "ubuntu");
    }

    #[test]
    fn parse_host_string_no_port() {
        let config = parse_host_string("deploy@myhost", "r2");
        assert_eq!(config.host, "myhost");
        assert_eq!(config.port, 22);
        assert_eq!(config.user, "deploy");
    }

    #[test]
    fn parse_host_string_host_only() {
        let config = parse_host_string("192.168.1.10", "r3");
        assert_eq!(config.host, "192.168.1.10");
        assert_eq!(config.port, 22);
        assert_eq!(config.user, "root");
    }

    // --- rig commands without rig ---

    #[test]
    fn rig_commands_without_rig() {
        let mut sys = test_sys();
        let r = sys.execute(Command::RigList);
        assert!(is_err(&r));
        assert!(output(&r).contains("not initialized"));
    }

    // Client tests removed — handled by MuxUX.

    #[test]
    fn build_snapshot_captures_agents() {
        let mut sys = test_sys();
        sys.execute(Command::AgentNew {
            role: "worker".into(),
            name: Some("w1".into()),
            path: None,
            agent_type: None,
        });
        let snap = sys.build_snapshot();
        assert_eq!(snap.agents.len(), 1);
        assert_eq!(snap.agents[0].name, "w1");
        assert_eq!(snap.agents[0].role, "worker");
    }

    #[test]
    fn save_current_state_creates_file() {
        let dir = std::env::temp_dir().join("cmx_sys_save_state_test");
        let _ = std::fs::create_dir_all(&dir);
        let data = Data::new(&dir).unwrap();
        let mut sys = Sys::from_data(data);
        sys.execute(Command::AgentNew {
            role: "worker".into(),
            name: Some("snap-agent".into()),
            path: None,
            agent_type: None,
        });
        let result = sys.save_current_state();
        assert!(result.is_ok());
        let state_path = dir.join("current_state.json");
        assert!(state_path.exists());
        let loaded = crate::snapshot::checkpoint::load_snapshot(&state_path).unwrap();
        assert_eq!(loaded.agents.len(), 1);
        assert_eq!(loaded.agents[0].name, "snap-agent");
        let _ = std::fs::remove_dir_all(&dir);
    }

    // --- pool commands ---

    #[test]
    fn pool_list_no_pools() {
        let mut sys = test_sys();
        let r = sys.execute(Command::PoolList);
        assert!(is_ok(&r));
        assert!(output(&r).contains("No pools configured"));
    }

    #[test]
    fn pool_set_creates_pool_and_spawns() {
        let mut sys = test_sys();
        let r = sys.execute(Command::PoolSet {
            role: "worker".into(),
            size: 3,
            path: Some("/tmp/work".into()),
        });
        assert!(is_ok(&r));
        assert!(output(&r).contains("Pool 'worker' set to 3"));
        assert!(output(&r).contains("spawned 3"));
        assert_eq!(sys.data.agents().list().len(), 3);
    }

    #[test]
    fn pool_status_shows_counts() {
        let mut sys = test_sys();
        sys.execute(Command::PoolSet {
            role: "worker".into(),
            size: 2,
            path: Some("/tmp".into()),
        });
        let r = sys.execute(Command::PoolStatus { role: "worker".into() });
        assert!(is_ok(&r));
        assert!(output(&r).contains("2 idle"));
        assert!(output(&r).contains("target: 2"));
    }

    #[test]
    fn pool_list_after_set() {
        let mut sys = test_sys();
        sys.execute(Command::PoolSet {
            role: "worker".into(),
            size: 2,
            path: Some("/tmp".into()),
        });
        let r = sys.execute(Command::PoolList);
        assert!(is_ok(&r));
        assert!(output(&r).contains("worker"));
        assert!(output(&r).contains("target: 2"));
    }

    #[test]
    fn pool_remove_removes_pool() {
        let mut sys = test_sys();
        sys.execute(Command::PoolSet {
            role: "worker".into(),
            size: 2,
            path: Some("/tmp".into()),
        });
        let r = sys.execute(Command::PoolRemove { role: "worker".into() });
        assert!(is_ok(&r));
        assert!(output(&r).contains("removed"));
        // Pool should be gone now
        let r = sys.execute(Command::PoolList);
        assert!(output(&r).contains("No pools configured"));
    }

    #[test]
    fn pool_status_unknown_role() {
        let mut sys = test_sys();
        let r = sys.execute(Command::PoolStatus { role: "ghost".into() });
        assert!(is_err(&r));
        assert!(output(&r).contains("No pool configured"));
    }

    // --- diagnosis commands ---

    #[test]
    fn diagnosis_report_empty() {
        let dir = std::env::temp_dir().join("cmx_sys_diag_report_test");
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::create_dir_all(&dir);
        let data = Data::new(&dir).unwrap();
        let mut sys = Sys::from_data(data);
        let r = sys.execute(Command::DiagnosisReport);
        assert!(is_ok(&r));
        assert!(output(&r).contains("No intervention events recorded"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn diagnosis_report_with_events() {
        use crate::diagnosis::{
            DiagnosisEngine, InterventionAction, InterventionEvent,
            InterventionOutcome, SignalType,
        };
        let dir = std::env::temp_dir().join("cmx_sys_diag_report_events");
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::create_dir_all(&dir);

        // Seed events via DiagnosisEngine
        {
            let mut engine = DiagnosisEngine::new(dir.clone()).unwrap();
            engine
                .record(InterventionEvent {
                    id: 0,
                    timestamp_ms: 1000,
                    agent: "w1".into(),
                    signal: SignalType::HeartbeatStale,
                    signal_detail: "stale".into(),
                    action: InterventionAction::Retry,
                    outcome: InterventionOutcome::Resolved,
                    outcome_detail: "ok".into(),
                    duration_ms: 500,
                    failure_mode: "none".into(),
                })
                .unwrap();
        }

        let data = Data::new(&dir).unwrap();
        let mut sys = Sys::from_data(data);
        let r = sys.execute(Command::DiagnosisReport);
        assert!(is_ok(&r));
        assert!(output(&r).contains("# Diagnosis Report"));
        assert!(output(&r).contains("Signal Reliability"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn diagnosis_reliability_all() {
        use crate::diagnosis::{
            DiagnosisEngine, InterventionAction, InterventionEvent,
            InterventionOutcome, SignalType,
        };
        let dir = std::env::temp_dir().join("cmx_sys_diag_rel_all");
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::create_dir_all(&dir);

        {
            let mut engine = DiagnosisEngine::new(dir.clone()).unwrap();
            for i in 0..5 {
                engine
                    .record(InterventionEvent {
                        id: 0,
                        timestamp_ms: 1000 + i * 100,
                        agent: "w1".into(),
                        signal: SignalType::HeartbeatStale,
                        signal_detail: "stale".into(),
                        action: InterventionAction::Retry,
                        outcome: InterventionOutcome::Resolved,
                        outcome_detail: "ok".into(),
                        duration_ms: 500,
                        failure_mode: "none".into(),
                    })
                    .unwrap();
            }
        }

        let data = Data::new(&dir).unwrap();
        let mut sys = Sys::from_data(data);

        // Tabular format
        let r = sys.execute(Command::DiagnosisReliability {
            signal: None,
            format: None,
        });
        assert!(is_ok(&r));
        assert!(output(&r).contains("heartbeat_stale"));
        assert!(output(&r).contains("Fires"));

        // JSON format
        let r = sys.execute(Command::DiagnosisReliability {
            signal: None,
            format: Some("json".into()),
        });
        assert!(is_ok(&r));
        assert!(output(&r).contains("\"total_fires\""));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn diagnosis_reliability_single_signal() {
        use crate::diagnosis::{
            DiagnosisEngine, InterventionAction, InterventionEvent,
            InterventionOutcome, SignalType,
        };
        let dir = std::env::temp_dir().join("cmx_sys_diag_rel_single");
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::create_dir_all(&dir);

        {
            let mut engine = DiagnosisEngine::new(dir.clone()).unwrap();
            engine
                .record(InterventionEvent {
                    id: 0,
                    timestamp_ms: 1000,
                    agent: "w1".into(),
                    signal: SignalType::ErrorPattern,
                    signal_detail: "err".into(),
                    action: InterventionAction::Restart,
                    outcome: InterventionOutcome::Resolved,
                    outcome_detail: "ok".into(),
                    duration_ms: 1000,
                    failure_mode: "none".into(),
                })
                .unwrap();
        }

        let data = Data::new(&dir).unwrap();
        let mut sys = Sys::from_data(data);
        let r = sys.execute(Command::DiagnosisReliability {
            signal: Some("error_pattern".into()),
            format: None,
        });
        assert!(is_ok(&r));
        assert!(output(&r).contains("error_pattern"));

        // Unknown signal
        let r = sys.execute(Command::DiagnosisReliability {
            signal: Some("bogus_signal".into()),
            format: None,
        });
        assert!(is_err(&r));
        assert!(output(&r).contains("Unknown signal type"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn diagnosis_effectiveness_tabular() {
        use crate::diagnosis::{
            DiagnosisEngine, InterventionAction, InterventionEvent,
            InterventionOutcome, SignalType,
        };
        let dir = std::env::temp_dir().join("cmx_sys_diag_eff");
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::create_dir_all(&dir);

        {
            let mut engine = DiagnosisEngine::new(dir.clone()).unwrap();
            engine
                .record(InterventionEvent {
                    id: 0,
                    timestamp_ms: 1000,
                    agent: "w1".into(),
                    signal: SignalType::HeartbeatStale,
                    signal_detail: "stale".into(),
                    action: InterventionAction::Retry,
                    outcome: InterventionOutcome::Resolved,
                    outcome_detail: "ok".into(),
                    duration_ms: 500,
                    failure_mode: "none".into(),
                })
                .unwrap();
            engine
                .record(InterventionEvent {
                    id: 0,
                    timestamp_ms: 2000,
                    agent: "w1".into(),
                    signal: SignalType::HeartbeatStale,
                    signal_detail: "stale".into(),
                    action: InterventionAction::Retry,
                    outcome: InterventionOutcome::StillBroken,
                    outcome_detail: "nope".into(),
                    duration_ms: 1000,
                    failure_mode: "agent".into(),
                })
                .unwrap();
        }

        let data = Data::new(&dir).unwrap();
        let mut sys = Sys::from_data(data);
        let r = sys.execute(Command::DiagnosisEffectiveness {
            signal: None,
            format: None,
        });
        assert!(is_ok(&r));
        assert!(output(&r).contains("retry"));
        assert!(output(&r).contains("Attempts"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn diagnosis_thresholds_empty() {
        let dir = std::env::temp_dir().join("cmx_sys_diag_thresh_empty");
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::create_dir_all(&dir);
        let data = Data::new(&dir).unwrap();
        let mut sys = Sys::from_data(data);
        let r = sys.execute(Command::DiagnosisThresholds { format: None });
        assert!(is_ok(&r));
        assert!(output(&r).contains("No thresholds computed yet"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn diagnosis_events_empty() {
        let dir = std::env::temp_dir().join("cmx_sys_diag_events_empty");
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::create_dir_all(&dir);
        let data = Data::new(&dir).unwrap();
        let mut sys = Sys::from_data(data);
        let r = sys.execute(Command::DiagnosisEvents {
            limit: None,
            format: None,
        });
        assert!(is_ok(&r));
        assert!(output(&r).contains("No intervention events recorded"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn diagnosis_events_with_limit() {
        use crate::diagnosis::{
            DiagnosisEngine, InterventionAction, InterventionEvent,
            InterventionOutcome, SignalType,
        };
        let dir = std::env::temp_dir().join("cmx_sys_diag_events_limit");
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::create_dir_all(&dir);

        {
            let mut engine = DiagnosisEngine::new(dir.clone()).unwrap();
            for i in 0..10 {
                engine
                    .record(InterventionEvent {
                        id: 0,
                        timestamp_ms: i * 100,
                        agent: "w1".into(),
                        signal: SignalType::HeartbeatStale,
                        signal_detail: "stale".into(),
                        action: InterventionAction::Retry,
                        outcome: InterventionOutcome::Resolved,
                        outcome_detail: "ok".into(),
                        duration_ms: 500,
                        failure_mode: "none".into(),
                    })
                    .unwrap();
            }
        }

        let data = Data::new(&dir).unwrap();
        let mut sys = Sys::from_data(data);

        // Limit to 3 events
        let r = sys.execute(Command::DiagnosisEvents {
            limit: Some("3".into()),
            format: None,
        });
        assert!(is_ok(&r));
        let text = output(&r);
        // Should have header + separator + 3 data lines = 5 lines
        let line_count = text.lines().count();
        assert_eq!(line_count, 5);

        // JSON format
        let r = sys.execute(Command::DiagnosisEvents {
            limit: Some("3".into()),
            format: Some("json".into()),
        });
        assert!(is_ok(&r));
        assert!(output(&r).contains("\"id\""));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn parse_signal_type_valid() {
        assert_eq!(
            parse_signal_type("heartbeat_stale").unwrap(),
            crate::diagnosis::SignalType::HeartbeatStale
        );
        assert_eq!(
            parse_signal_type("error_pattern").unwrap(),
            crate::diagnosis::SignalType::ErrorPattern
        );
        assert_eq!(
            parse_signal_type("output_stall").unwrap(),
            crate::diagnosis::SignalType::OutputStall
        );
        assert_eq!(
            parse_signal_type("ssh_disconnected").unwrap(),
            crate::diagnosis::SignalType::SshDisconnected
        );
        assert_eq!(
            parse_signal_type("explicit_error").unwrap(),
            crate::diagnosis::SignalType::ExplicitError
        );
        assert_eq!(
            parse_signal_type("manual_escalation").unwrap(),
            crate::diagnosis::SignalType::ManualEscalation
        );
        assert_eq!(
            parse_signal_type("trigger_fired(my_trigger)").unwrap(),
            crate::diagnosis::SignalType::TriggerFired("my_trigger".into())
        );
    }

    #[test]
    fn parse_signal_type_invalid() {
        assert!(parse_signal_type("bogus").is_err());
    }


    // --- history commands ---

    #[test]
    fn history_list_empty() {
        let dir = std::env::temp_dir().join("cmx_sys_hist_list_empty");
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::create_dir_all(&dir);
        let data = Data::new(&dir).unwrap();
        let mut sys = Sys::from_data(data);
        let r = sys.execute(Command::HistoryList { limit: None, format: None });
        assert!(is_ok(&r));
        assert!(output(&r).contains("No history snapshots found"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn history_snapshot_creates_entry() {
        let dir = std::env::temp_dir().join("cmx_sys_hist_snap");
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(dir.join("Current Configuration.md"), "# Config v1\n").unwrap();
        let data = Data::new(&dir).unwrap();
        let mut sys = Sys::from_data(data);
        let r = sys.execute(Command::HistorySnapshot);
        assert!(is_ok(&r));
        assert!(output(&r).contains("Snapshot created"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn history_snapshot_no_change() {
        let dir = std::env::temp_dir().join("cmx_sys_hist_snap_nochange");
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(dir.join("Current Configuration.md"), "# Config\n").unwrap();
        let data = Data::new(&dir).unwrap();
        let mut sys = Sys::from_data(data);
        // First snapshot.
        sys.execute(Command::HistorySnapshot);
        // Second snapshot should detect no change.
        let r = sys.execute(Command::HistorySnapshot);
        assert!(is_ok(&r));
        assert!(output(&r).contains("No changes to snapshot"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn history_list_shows_entries() {
        let dir = std::env::temp_dir().join("cmx_sys_hist_list");
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(dir.join("Current Configuration.md"), "# Config v1\n").unwrap();
        let data = Data::new(&dir).unwrap();
        let mut sys = Sys::from_data(data);
        sys.execute(Command::HistorySnapshot);
        let r = sys.execute(Command::HistoryList { limit: None, format: None });
        assert!(is_ok(&r));
        assert!(output(&r).contains("Filename"));
        assert!(output(&r).contains(".md"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn history_list_json_format() {
        let dir = std::env::temp_dir().join("cmx_sys_hist_list_json");
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(dir.join("Current Configuration.md"), "# Config v1\n").unwrap();
        let data = Data::new(&dir).unwrap();
        let mut sys = Sys::from_data(data);
        sys.execute(Command::HistorySnapshot);
        let r = sys.execute(Command::HistoryList { limit: None, format: Some("json".into()) });
        assert!(is_ok(&r));
        assert!(output(&r).contains("timestamp_ms"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn history_list_with_limit() {
        use crate::history::snapshot::{create_snapshot, compose_timestamp};
        let dir = std::env::temp_dir().join("cmx_sys_hist_list_limit");
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::create_dir_all(&dir);
        let history_dir = dir.join("history");
        let ts1 = compose_timestamp(2026, 2, 22, 10, 0, 0) * 1000;
        let ts2 = compose_timestamp(2026, 2, 22, 11, 0, 0) * 1000;
        let ts3 = compose_timestamp(2026, 2, 22, 12, 0, 0) * 1000;
        create_snapshot(&history_dir, "v1\n", ts1).unwrap();
        create_snapshot(&history_dir, "v2\n", ts2).unwrap();
        create_snapshot(&history_dir, "v3\n", ts3).unwrap();
        let data = Data::new(&dir).unwrap();
        let mut sys = Sys::from_data(data);
        let r = sys.execute(Command::HistoryList { limit: Some("1".into()), format: None });
        assert!(is_ok(&r));
        // Header + separator + 1 data line = 3 lines.
        let line_count = output(&r).lines().count();
        assert_eq!(line_count, 3);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn history_show_by_index() {
        let dir = std::env::temp_dir().join("cmx_sys_hist_show_idx");
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(dir.join("Current Configuration.md"), "# My Config\n").unwrap();
        let data = Data::new(&dir).unwrap();
        let mut sys = Sys::from_data(data);
        sys.execute(Command::HistorySnapshot);
        let r = sys.execute(Command::HistoryShow { id: "0".into() });
        assert!(is_ok(&r));
        assert!(output(&r).contains("# My Config"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn history_show_invalid_index() {
        let dir = std::env::temp_dir().join("cmx_sys_hist_show_bad_idx");
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::create_dir_all(&dir);
        let data = Data::new(&dir).unwrap();
        let mut sys = Sys::from_data(data);
        let r = sys.execute(Command::HistoryShow { id: "99".into() });
        assert!(is_err(&r));
        assert!(output(&r).contains("out of range"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn history_restore_by_index() {
        use crate::history::snapshot::{create_snapshot, compose_timestamp};
        let dir = std::env::temp_dir().join("cmx_sys_hist_restore_idx");
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::create_dir_all(&dir);
        let config = dir.join("Current Configuration.md");
        let history_dir = dir.join("history");
        // Create two snapshots with explicit timestamps.
        let ts1 = compose_timestamp(2026, 2, 22, 10, 0, 0) * 1000;
        let ts2 = compose_timestamp(2026, 2, 22, 11, 0, 0) * 1000;
        create_snapshot(&history_dir, "original\n", ts1).unwrap();
        create_snapshot(&history_dir, "modified\n", ts2).unwrap();
        // Write current config as something different.
        std::fs::write(&config, "current\n").unwrap();
        let data = Data::new(&dir).unwrap();
        let mut sys = Sys::from_data(data);
        // Restore oldest (index 1 since newest is 0).
        let r = sys.execute(Command::HistoryRestore { id: "1".into() });
        assert!(is_ok(&r));
        assert!(output(&r).contains("Restored"));
        let restored = std::fs::read_to_string(&config).unwrap();
        assert_eq!(restored, "original\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn history_prune_empty() {
        let dir = std::env::temp_dir().join("cmx_sys_hist_prune_empty");
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::create_dir_all(&dir);
        let data = Data::new(&dir).unwrap();
        let mut sys = Sys::from_data(data);
        let r = sys.execute(Command::HistoryPrune);
        assert!(is_ok(&r));
        assert!(output(&r).contains("Pruned 0"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn history_diff_between_entries() {
        use crate::history::snapshot::{create_snapshot, compose_timestamp};
        let dir = std::env::temp_dir().join("cmx_sys_hist_diff");
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::create_dir_all(&dir);
        let history_dir = dir.join("history");
        // Create two snapshots with explicit timestamps.
        let ts1 = compose_timestamp(2026, 2, 22, 10, 0, 0) * 1000;
        let ts2 = compose_timestamp(2026, 2, 22, 11, 0, 0) * 1000;
        create_snapshot(&history_dir, "alpha\nbeta\n", ts1).unwrap();
        create_snapshot(&history_dir, "alpha\ngamma\n", ts2).unwrap();
        let data = Data::new(&dir).unwrap();
        let mut sys = Sys::from_data(data);
        // Diff oldest (index 1) vs newest (index 0).
        let r = sys.execute(Command::HistoryDiff { from: "1".into(), to: Some("0".into()) });
        assert!(is_ok(&r));
        assert!(output(&r).contains("gamma"));
        assert!(output(&r).contains("beta"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn resolve_history_entry_by_index_and_name() {
        use crate::history::snapshot::HistoryEntry;
        use std::path::PathBuf;
        let entries = vec![
            HistoryEntry {
                timestamp_ms: 2000,
                filename: "2026-02-22T10-00-00.md".into(),
                path: PathBuf::from("/tmp/test"),
                size_bytes: 100,
            },
            HistoryEntry {
                timestamp_ms: 1000,
                filename: "2026-02-22T09-00-00.md".into(),
                path: PathBuf::from("/tmp/test2"),
                size_bytes: 50,
            },
        ];
        // By index.
        let e = resolve_history_entry(&entries, "0").unwrap();
        assert_eq!(e.timestamp_ms, 2000);
        let e = resolve_history_entry(&entries, "1").unwrap();
        assert_eq!(e.timestamp_ms, 1000);
        // By filename.
        let e = resolve_history_entry(&entries, "2026-02-22T09-00-00.md").unwrap();
        assert_eq!(e.timestamp_ms, 1000);
        // Out of range.
        assert!(resolve_history_entry(&entries, "5").is_err());
        // Not found.
        assert!(resolve_history_entry(&entries, "nonexistent.md").is_err());
    }

    // --- hollow world (Milestone M) ---
    //
    // Synthetic E2E tests exercising the full orchestration pipeline:
    // agent lifecycle, task assignment, inter-agent messaging, state
    // transitions, interrupts, and role-specific behavior.
    //
    // Roles: hw_pilot, hw_pm, hw_builder, hw_checker
    // Agents: hwp, hwpm, hwb1, hwb2, hwc1

    /// Helper: create the 5 hollow-world agents and return the Sys.
    fn hw_sys() -> Sys {
        let mut sys = test_sys();
        for (role, name) in [
            ("hw_pilot", "hwp"),
            ("hw_pm", "hwpm"),
            ("hw_builder", "hwb1"),
            ("hw_builder", "hwb2"),
            ("hw_checker", "hwc1"),
        ] {
            let r = sys.execute(Command::AgentNew {
                role: role.into(),
                name: Some(name.into()),
                path: None,
                agent_type: None,
            });
            assert!(is_ok(&r), "Failed to create {}: {}", name, output(&r));
        }
        // Drain setup actions so tests start clean
        sys.drain_actions();
        sys
    }

    #[test]
    fn hw_scenario_1_full_orchestration_chain() {
        let mut sys = hw_sys();

        // 1. Pilot tells PM: "build widget-alpha"
        let r = sys.execute(Command::Tell {
            agent: "hwpm".into(),
            text: "build widget-alpha".into(),
        });
        assert!(is_ok(&r));
        assert_eq!(sys.data.messages().pending_for("hwpm").len(), 1);

        // 2. PM assigns task T1 to builder hwb1
        let r = sys.execute(Command::AgentAssign {
            name: "hwb1".into(),
            task: "T1".into(),
        });
        assert!(is_ok(&r));
        assert_eq!(sys.data.agents().get("hwb1").unwrap().task.as_deref(), Some("T1"));

        // 3. Builder updates status: "building"
        let r = sys.execute(Command::AgentStatus {
            name: "hwb1".into(),
            notes: Some("building".into()),
        });
        assert!(is_ok(&r));
        assert_eq!(sys.data.agents().get("hwb1").unwrap().status_notes, "building");

        // 4. Builder completes — unassign and update status
        sys.execute(Command::AgentUnassign { name: "hwb1".into() });
        sys.execute(Command::AgentStatus {
            name: "hwb1".into(),
            notes: Some("done".into()),
        });
        assert_eq!(sys.data.agents().get("hwb1").unwrap().task, None);

        // 5. PM assigns check to checker hwc1
        let r = sys.execute(Command::AgentAssign {
            name: "hwc1".into(),
            task: "T1-check".into(),
        });
        assert!(is_ok(&r));
        assert_eq!(sys.data.agents().get("hwc1").unwrap().task.as_deref(), Some("T1-check"));

        // 6. Checker completes
        sys.execute(Command::AgentUnassign { name: "hwc1".into() });
        sys.execute(Command::AgentStatus {
            name: "hwc1".into(),
            notes: Some("verified".into()),
        });
        assert_eq!(sys.data.agents().get("hwc1").unwrap().status_notes, "verified");

        // 7. PM tells pilot: "widget-alpha complete"
        let r = sys.execute(Command::Tell {
            agent: "hwp".into(),
            text: "widget-alpha complete".into(),
        });
        assert!(is_ok(&r));
        assert_eq!(sys.data.messages().pending_for("hwp").len(), 1);
    }

    #[test]
    fn hw_scenario_2_parallel_workers() {
        let mut sys = hw_sys();

        // Assign tasks to two builders simultaneously
        sys.execute(Command::AgentAssign { name: "hwb1".into(), task: "T2".into() });
        sys.execute(Command::AgentAssign { name: "hwb2".into(), task: "T3".into() });

        assert_eq!(sys.data.agents().get("hwb1").unwrap().task.as_deref(), Some("T2"));
        assert_eq!(sys.data.agents().get("hwb2").unwrap().task.as_deref(), Some("T3"));

        // Both update status
        sys.execute(Command::AgentStatus { name: "hwb1".into(), notes: Some("building".into()) });
        sys.execute(Command::AgentStatus { name: "hwb2".into(), notes: Some("building".into()) });

        // hwb1 completes first
        sys.execute(Command::AgentUnassign { name: "hwb1".into() });
        sys.execute(Command::AgentStatus { name: "hwb1".into(), notes: Some("done".into()) });
        assert_eq!(sys.data.agents().get("hwb1").unwrap().task, None);
        // hwb2 still busy
        assert_eq!(sys.data.agents().get("hwb2").unwrap().task.as_deref(), Some("T3"));

        // hwb2 completes
        sys.execute(Command::AgentUnassign { name: "hwb2".into() });
        sys.execute(Command::AgentStatus { name: "hwb2".into(), notes: Some("done".into()) });
        assert_eq!(sys.data.agents().get("hwb2").unwrap().task, None);

        // Verify both builders listed
        let r = sys.execute(Command::AgentList { format: None });
        assert!(output(&r).contains("hwb1"));
        assert!(output(&r).contains("hwb2"));
    }

    #[test]
    fn hw_scenario_3_interrupt_and_recovery() {
        let mut sys = hw_sys();

        // Assign task to builder
        sys.execute(Command::AgentAssign { name: "hwb1".into(), task: "T4".into() });
        sys.execute(Command::AgentStatus { name: "hwb1".into(), notes: Some("building".into()) });
        sys.drain_actions();

        // Interrupt the builder
        let r = sys.execute(Command::Interrupt {
            agent: "hwb1".into(),
            text: Some("priority change".into()),
        });
        assert!(is_ok(&r));
        // Should generate SendKeys (C-c) + SendKeys (message) actions
        assert!(sys.pending_actions().len() >= 1);
        let has_ctrl_c = sys.pending_actions().iter().any(|a| {
            matches!(a, Action::SendKeys { keys, .. } if keys == "C-c")
        });
        assert!(has_ctrl_c, "Interrupt should generate C-c SendKeys action");

        // Builder acknowledges interrupt
        sys.execute(Command::AgentStatus { name: "hwb1".into(), notes: Some("interrupted".into()) });
        assert_eq!(sys.data.agents().get("hwb1").unwrap().status_notes, "interrupted");

        // Reassign to new task
        sys.execute(Command::AgentUnassign { name: "hwb1".into() });
        sys.execute(Command::AgentAssign { name: "hwb1".into(), task: "T5".into() });
        sys.execute(Command::AgentStatus { name: "hwb1".into(), notes: Some("building".into()) });

        assert_eq!(sys.data.agents().get("hwb1").unwrap().task.as_deref(), Some("T5"));
        assert_eq!(sys.data.agents().get("hwb1").unwrap().status_notes, "building");
    }

    #[test]
    fn hw_scenario_4_agent_kill_and_restart() {
        let mut sys = hw_sys();

        // Kill hwb2
        let r = sys.execute(Command::AgentKill { name: "hwb2".into() });
        assert!(is_ok(&r));
        assert!(sys.data.agents().get("hwb2").is_none());

        // Agent list should have 4 agents now
        assert_eq!(sys.data.agents().list().len(), 4);

        // Recreate hwb2
        let r = sys.execute(Command::AgentNew {
            role: "hw_builder".into(),
            name: Some("hwb2".into()),
            path: None,
            agent_type: None,
        });
        assert!(is_ok(&r));
        assert_eq!(sys.data.agents().list().len(), 5);

        // Assign task to the new hwb2 — works normally
        let r = sys.execute(Command::AgentAssign { name: "hwb2".into(), task: "T6".into() });
        assert!(is_ok(&r));
        assert_eq!(sys.data.agents().get("hwb2").unwrap().task.as_deref(), Some("T6"));
    }

    #[test]
    fn hw_scenario_5_cross_role_messaging() {
        let mut sys = hw_sys();

        // Builder tells checker
        sys.execute(Command::Tell {
            agent: "hwc1".into(),
            text: "build artifact ready at /out/widget".into(),
        });

        // Checker tells PM
        sys.execute(Command::Tell {
            agent: "hwpm".into(),
            text: "check passed for /out/widget".into(),
        });

        // PM tells pilot
        sys.execute(Command::Tell {
            agent: "hwp".into(),
            text: "widget verified".into(),
        });

        // Verify message queues
        assert_eq!(sys.data.messages().pending_for("hwc1").len(), 1);
        assert_eq!(sys.data.messages().pending_for("hwpm").len(), 1);
        assert_eq!(sys.data.messages().pending_for("hwp").len(), 1);

        // Verify message content
        let hwc1_msg = &sys.data.messages().pending_for("hwc1")[0];
        assert!(hwc1_msg.text.contains("artifact ready"));

        let hwpm_msg = &sys.data.messages().pending_for("hwpm")[0];
        assert!(hwpm_msg.text.contains("check passed"));

        let hwp_msg = &sys.data.messages().pending_for("hwp")[0];
        assert!(hwp_msg.text.contains("widget verified"));

        // Verify total pending
        assert_eq!(sys.data.messages().all_pending().len(), 3);
    }

    #[test]
    fn hw_scenario_6_priority_messages() {
        let mut sys = hw_sys();

        // Send 3 normal messages
        sys.execute(Command::Tell { agent: "hwb1".into(), text: "normal msg 1".into() });
        sys.execute(Command::Tell { agent: "hwb1".into(), text: "normal msg 2".into() });
        sys.execute(Command::Tell { agent: "hwb1".into(), text: "normal msg 3".into() });

        // Verify 3 pending
        assert_eq!(sys.data.messages().pending_for("hwb1").len(), 3);

        // Verify FIFO ordering — first pending message is "normal msg 1"
        let pending = sys.data.messages().pending_for("hwb1");
        assert_eq!(pending[0].text, "normal msg 1");
        assert_eq!(pending[1].text, "normal msg 2");
        assert_eq!(pending[2].text, "normal msg 3");

        assert_eq!(sys.data.messages().all_pending().len(), 3);
    }

    #[test]
    fn hw_scenario_7_state_machine_walkthrough() {
        use crate::agent::state::{AgentState, Transition};
        use crate::agent::lifecycle::LifecycleManager;

        let mut mgr = LifecycleManager::new(3, 30000);
        let mut t = 1000u64; // simulated clock

        // Register all hollow-world agents
        for name in ["hwp", "hwpm", "hwb1", "hwb2", "hwc1"] {
            mgr.register(name).unwrap();
        }
        assert_eq!(mgr.available_agents().len(), 0); // All Spawning
        assert_eq!(mgr.summary().spawning, 5);

        // Spawn-complete all agents
        for name in ["hwp", "hwpm", "hwb1", "hwb2", "hwc1"] {
            t += 100;
            mgr.transition(name, Transition::SpawnComplete, t).unwrap();
        }
        assert_eq!(mgr.available_agents().len(), 5); // All Ready

        // Drive hwb1 through full state machine path
        t += 100;
        mgr.transition("hwb1", Transition::TaskAssigned { task_id: "T1".into() }, t).unwrap();
        assert_eq!(mgr.busy_agents().len(), 1);
        assert!(matches!(mgr.state("hwb1"), Some(AgentState::Busy { .. })));

        // Heartbeat timeout → Stalled
        t += 100;
        mgr.transition("hwb1", Transition::HeartbeatTimeout { age_ms: 60000 }, t).unwrap();
        assert_eq!(mgr.stalled_agents().len(), 1);
        assert!(matches!(mgr.state("hwb1"), Some(AgentState::Stalled { .. })));

        // Recovery started → Recovering
        t += 100;
        mgr.transition("hwb1", Transition::RecoveryStarted, t).unwrap();
        assert!(matches!(mgr.state("hwb1"), Some(AgentState::Recovering { attempt: 1 })));

        // Recovery succeeded → Ready
        t += 100;
        mgr.transition("hwb1", Transition::RecoverySucceeded, t).unwrap();
        assert!(matches!(mgr.state("hwb1"), Some(AgentState::Ready)));
        assert!(mgr.available_agents().contains(&"hwb1"));

        // Verify full history recorded
        let history = mgr.history_for("hwb1");
        // Spawning→Ready, Ready→Busy, Busy→Stalled, Stalled→Recovering, Recovering→Ready = 5 transitions
        assert_eq!(history.len(), 5);

        // Verify other agents unaffected
        assert!(matches!(mgr.state("hwp"), Some(AgentState::Ready)));
        assert!(matches!(mgr.state("hwpm"), Some(AgentState::Ready)));
        assert!(matches!(mgr.state("hwb2"), Some(AgentState::Ready)));
        assert!(matches!(mgr.state("hwc1"), Some(AgentState::Ready)));
    }

    // --- notify_session_created ---

    #[test]
    fn notify_session_created_sets_field() {
        let mut sys = test_sys();
        sys.execute(Command::AgentNew {
            role: "worker".into(),
            name: Some("w1".into()),
            path: None,
            agent_type: None,
        });
        assert!(sys.data().agents().get("w1").unwrap().session.is_none());

        sys.notify_session_created("w1", "cmx-w1").unwrap();
        assert_eq!(
            sys.data().agents().get("w1").unwrap().session.as_deref(),
            Some("cmx-w1")
        );
    }

    #[test]
    fn notify_session_created_unknown_agent_errors() {
        let mut sys = test_sys();
        let result = sys.notify_session_created("nonexistent", "cmx-nope");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not found"));
    }

    // --- notify_agent_ready ---

    #[test]
    fn notify_agent_ready_sets_health_and_status() {
        let mut sys = test_sys();
        sys.execute(Command::AgentNew {
            role: "worker".into(),
            name: Some("w1".into()),
            path: None,
            agent_type: None,
        });
        // Agent starts with Unknown health
        let a = sys.data().agents().get("w1").unwrap();
        assert_eq!(a.health, HealthState::Unknown);

        sys.notify_agent_ready("w1").unwrap();
        let a = sys.data().agents().get("w1").unwrap();
        assert_eq!(a.health, HealthState::Healthy);
        assert_eq!(a.status, AgentStatus::Idle);
    }

    #[test]
    fn notify_agent_ready_unknown_agent_errors() {
        let mut sys = test_sys();
        let result = sys.notify_agent_ready("nonexistent");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not found"));
    }

    // --- library integration (MO.1) ---

    #[test]
    fn sys_has_library() {
        let sys = test_sys();
        // Library exists and is accessible (may be empty in test context)
        let _ = sys.library().list();
    }

    #[test]
    fn project_add_with_skills_registers_source() {
        let mut sys = test_sys();
        // Use the hollow-world fixture which has a skills/ dir
        let hw_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent().unwrap()
            .join("tests/hollow-world");
        let r = sys.execute(Command::ProjectAdd {
            name: "hw".into(),
            path: hw_path.to_string_lossy().into(),
        });
        assert!(is_ok(&r));

        // Library should now contain hw-builder, hw-checker, hw-pilot, hw-pm
        let skills = sys.library().list();
        assert!(skills.contains(&"hw-builder"), "expected hw-builder in {:?}", skills);
        assert!(skills.contains(&"hw-pm"), "expected hw-pm in {:?}", skills);
    }

    #[test]
    fn project_add_without_skills_no_error() {
        let mut sys = test_sys();
        // Use a temp dir with no skills/ subfolder
        let dir = std::env::temp_dir().join("cmx-test-no-skills");
        let _ = std::fs::create_dir_all(&dir);
        let r = sys.execute(Command::ProjectAdd {
            name: "bare".into(),
            path: dir.to_string_lossy().into(),
        });
        assert!(is_ok(&r));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn project_remove_drops_skills() {
        let mut sys = test_sys();
        let hw_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent().unwrap()
            .join("tests/hollow-world");
        sys.execute(Command::ProjectAdd {
            name: "hw".into(),
            path: hw_path.to_string_lossy().into(),
        });
        assert!(sys.library().list().contains(&"hw-builder"));

        sys.execute(Command::ProjectRemove { name: "hw".into() });
        // Skills from the removed project should be gone
        assert!(!sys.library().list().contains(&"hw-builder"),
            "hw-builder should be gone after project remove");
    }

    // --- agent assign with briefing (MO.2) ---

    #[test]
    fn agent_assign_resolves_role_skill() {
        let mut sys = test_sys();
        let hw_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent().unwrap()
            .join("tests/hollow-world");
        sys.execute(Command::ProjectAdd {
            name: "hw".into(),
            path: hw_path.to_string_lossy().into(),
        });

        // Create agent with role matching a skill name
        sys.execute(Command::AgentNew {
            role: "hw-builder".into(),
            name: Some("b1".into()),
            path: None,
            agent_type: None,
        });
        // Give the agent a session so briefing can be delivered
        sys.notify_session_created("b1", "cmx-b1").unwrap();

        // Create a task to assign
        sys.execute(Command::TaskSet {
            id: "T1".into(),
            title: None,
            status: None,
            result: None,
            agent: None,
        });

        sys.drain_actions(); // clear prior actions
        sys.execute(Command::AgentAssign {
            name: "b1".into(),
            task: "T1".into(),
        });

        let actions = sys.drain_actions();
        // Should have UpdateAssignment + SendKeys with briefing
        let send_keys = actions.iter().find(|a| matches!(a, Action::SendKeys { .. }));
        assert!(send_keys.is_some(), "Expected SendKeys with briefing, got {:?}", actions);
        if let Some(Action::SendKeys { target, keys }) = send_keys {
            assert_eq!(target, "cmx-b1");
            assert!(keys.contains("Skill Instructions"), "Briefing should contain skill instructions");
            assert!(keys.contains("Builder Instructions"), "Briefing should contain builder content");
        }
    }

    #[test]
    fn agent_assign_no_skill_still_succeeds() {
        let mut sys = test_sys();
        sys.execute(Command::AgentNew {
            role: "unknown-role".into(),
            name: Some("u1".into()),
            path: None,
            agent_type: None,
        });
        sys.execute(Command::TaskSet {
            id: "T2".into(),
            title: None,
            status: None,
            result: None,
            agent: None,
        });

        let r = sys.execute(Command::AgentAssign {
            name: "u1".into(),
            task: "T2".into(),
        });
        assert!(is_ok(&r), "Assign should succeed even without matching skill");
    }

    #[test]
    fn agent_assign_no_session_skips_sendkeys() {
        let mut sys = test_sys();
        let hw_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent().unwrap()
            .join("tests/hollow-world");
        sys.execute(Command::ProjectAdd {
            name: "hw".into(),
            path: hw_path.to_string_lossy().into(),
        });
        sys.execute(Command::AgentNew {
            role: "hw-builder".into(),
            name: Some("b2".into()),
            path: None,
            agent_type: None,
        });
        // Do NOT set a session — no SendKeys should be emitted
        sys.execute(Command::TaskSet {
            id: "T3".into(),
            title: None,
            status: None,
            result: None,
            agent: None,
        });

        sys.drain_actions();
        sys.execute(Command::AgentAssign {
            name: "b2".into(),
            task: "T3".into(),
        });

        let actions = sys.drain_actions();
        let send_keys = actions.iter().find(|a| matches!(a, Action::SendKeys { .. }));
        assert!(send_keys.is_none(), "No SendKeys expected without session, got {:?}", actions);
    }

    // -----------------------------------------------------------------------
    // Roadmap load + write-back tests
    // -----------------------------------------------------------------------

    fn roadmap_test_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("cmx_roadmap_test_{}", name));
        let _ = std::fs::create_dir_all(&dir);
        dir
    }

    #[test]
    fn roadmap_load_populates_tasks() {
        let dir = roadmap_test_dir("load_populates");
        let roadmap = dir.join("Roadmap.md");
        std::fs::write(&roadmap, "\
# \u{25EF} M1 \u{2014} Core
## \u{25EF} M1.1 \u{2014} Socket
## \u{25EF} M1.2 \u{2014} Health
").unwrap();

        let mut sys = test_sys();
        let resp = sys.execute(Command::RoadmapLoad {
            path: roadmap.to_str().unwrap().into(),
        });
        assert!(matches!(resp, Response::Ok { .. }));
        assert!(sys.data().tasks().get("M1").is_some());
        assert!(sys.data().tasks().get("M1.1").is_some());
        assert!(sys.data().tasks().get("M1.2").is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn roadmap_load_registers_path_for_writeback() {
        let dir = roadmap_test_dir("load_registers");
        let roadmap = dir.join("Roadmap.md");
        std::fs::write(&roadmap, "# \u{25EF} M1 \u{2014} Core\n").unwrap();

        let mut sys = test_sys();
        sys.execute(Command::RoadmapLoad {
            path: roadmap.to_str().unwrap().into(),
        });
        assert_eq!(sys.data().roadmap_paths().len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn task_check_writes_back_to_roadmap_file() {
        let dir = roadmap_test_dir("check_writeback");
        let roadmap = dir.join("Roadmap.md");
        std::fs::write(&roadmap, "\
# \u{25EF} M1 \u{2014} Core
## \u{25EF} M1.1 \u{2014} Socket
## \u{25EF} M1.2 \u{2014} Health
").unwrap();

        let mut sys = test_sys();
        sys.execute(Command::RoadmapLoad {
            path: roadmap.to_str().unwrap().into(),
        });

        sys.execute(Command::TaskCheck { id: "M1.1".into() });

        // Verify in-memory
        let task = sys.data().tasks().get("M1.1").unwrap();
        assert_eq!(task.status, TaskStatus::Completed);

        // Verify on disk
        let content = std::fs::read_to_string(&roadmap).unwrap();
        assert!(content.contains("\u{2B24} M1.1"), "Expected completed marker in file, got:\n{}", content);
        assert!(content.contains("\u{25EF} M1 \u{2014} Core"));
        assert!(content.contains("\u{25EF} M1.2 \u{2014} Health"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn task_uncheck_writes_back_to_roadmap_file() {
        let dir = roadmap_test_dir("uncheck_writeback");
        let roadmap = dir.join("Roadmap.md");
        std::fs::write(&roadmap, "\
# \u{2B24} M1 \u{2014} Core
## \u{2B24} M1.1 \u{2014} Socket
").unwrap();

        let mut sys = test_sys();
        sys.execute(Command::RoadmapLoad {
            path: roadmap.to_str().unwrap().into(),
        });

        sys.execute(Command::TaskUncheck { id: "M1.1".into() });

        let content = std::fs::read_to_string(&roadmap).unwrap();
        assert!(content.contains("\u{25EF} M1.1"), "Expected pending marker in file");
        assert!(content.contains("\u{2B24} M1 \u{2014} Core"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn task_set_status_writes_back_to_roadmap_file() {
        let dir = roadmap_test_dir("set_writeback");
        let roadmap = dir.join("Roadmap.md");
        std::fs::write(&roadmap, "# \u{25EF} M1 \u{2014} Core\n").unwrap();

        let mut sys = test_sys();
        sys.execute(Command::RoadmapLoad {
            path: roadmap.to_str().unwrap().into(),
        });

        sys.execute(Command::TaskSet {
            id: "M1".into(),
            status: Some("completed".into()),
            title: None,
            result: None,
            agent: None,
        });

        let content = std::fs::read_to_string(&roadmap).unwrap();
        assert!(content.contains("\u{2B24} M1 \u{2014} Core"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn task_set_without_status_does_not_write_back() {
        let dir = roadmap_test_dir("set_no_writeback");
        let roadmap = dir.join("Roadmap.md");
        std::fs::write(&roadmap, "# \u{25EF} M1 \u{2014} Core\n").unwrap();

        let mut sys = test_sys();
        sys.execute(Command::RoadmapLoad {
            path: roadmap.to_str().unwrap().into(),
        });

        sys.execute(Command::TaskSet {
            id: "M1".into(),
            status: None,
            title: Some("Updated Core".into()),
            result: None,
            agent: None,
        });

        let content = std::fs::read_to_string(&roadmap).unwrap();
        assert!(content.contains("\u{25EF} M1 \u{2014} Core"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn roadmap_load_missing_file_returns_error() {
        let mut sys = test_sys();
        let resp = sys.execute(Command::RoadmapLoad {
            path: "/tmp/nonexistent_roadmap_xyz.md".into(),
        });
        assert!(matches!(resp, Response::Error { .. }));
    }

    #[test]
    fn roadmap_cli_parse() {
        let cmd = crate::cli::parse::parse_args(&["roadmap", "load", "/tmp/Roadmap.md"]).unwrap();
        assert!(matches!(cmd, Command::RoadmapLoad { path } if path == "/tmp/Roadmap.md"));
    }
}
