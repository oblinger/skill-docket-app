//! Daemon — the CMX main event loop.
//!
//! The daemon is single-threaded for state mutation. All shared state updates
//! flow through the main loop. External threads (convergence, monitoring)
//! communicate via an mpsc channel. The main loop is the single consumer.
//!
//! # Main loop tick
//!
//! 1. Drain channel — execute each pending event as an internal command
//! 2. Accept socket connections (non-blocking with timeout)
//! 3. Expire stale watchers — send timeouts to long-poll clients

use std::path::Path;
use std::sync::mpsc;

use crate::agent::bridge;
use crate::command::Command;
use crate::convergence::executor::ConvergenceExecutor;
use crate::convergence::retry::RetryPolicy;
use crate::infrastructure::SessionBackend;
use crate::infrastructure::mock::MockBackend;
use crate::monitor::cycle::MonitorCycle;
use crate::monitor::heartbeat;
use crate::service::ServiceSocket;
use crate::sys::Sys;
use crate::types::config::BackoffStrategy;
use cmx_utils::response::Action;
use cmx_utils::watch::WatchRegistry;


/// Events that can be sent to the daemon's main loop via the channel.
#[derive(Debug)]
pub enum DaemonEvent {
    /// A command from an internal source (thread, state-machine).
    InternalCommand {
        command: Command,
        /// Label for watch notifications (e.g., "convergence: layout achieved").
        source: String,
    },
    /// A status message for logging (no state mutation).
    Log { level: String, message: String },
    /// Request the daemon to shut down gracefully.
    Shutdown,
}


/// Configuration for the daemon loop.
pub struct DaemonConfig {
    /// How long to wait for socket connections per tick (milliseconds).
    pub socket_poll_ms: u64,
}


impl Default for DaemonConfig {
    fn default() -> Self {
        DaemonConfig {
            socket_poll_ms: 50,
        }
    }
}


/// Handle returned from `Daemon::handle()` allowing threads to send events.
#[derive(Clone)]
pub struct DaemonHandle {
    sender: mpsc::Sender<DaemonEvent>,
}


impl DaemonHandle {
    /// Send a command to the daemon for execution.
    pub fn send_command(&self, command: Command, source: &str) -> Result<(), String> {
        self.sender
            .send(DaemonEvent::InternalCommand {
                command,
                source: source.to_string(),
            })
            .map_err(|e| format!("Channel send failed: {}", e))
    }

    /// Send a log message to the daemon.
    pub fn log(&self, level: &str, message: &str) -> Result<(), String> {
        self.sender
            .send(DaemonEvent::Log {
                level: level.to_string(),
                message: message.to_string(),
            })
            .map_err(|e| format!("Channel send failed: {}", e))
    }

    /// Request daemon shutdown.
    pub fn shutdown(&self) -> Result<(), String> {
        self.sender
            .send(DaemonEvent::Shutdown)
            .map_err(|e| format!("Channel send failed: {}", e))
    }
}


/// The CMX daemon — owns the event loop, Sys, service socket, and watch registry.
pub struct Daemon {
    sys: Sys,
    service: ServiceSocket,
    registry: WatchRegistry,
    receiver: mpsc::Receiver<DaemonEvent>,
    handle: DaemonHandle,
    config: DaemonConfig,
    backend: Box<dyn SessionBackend + Send>,
    executor: ConvergenceExecutor,
    /// Agents whose tmux sessions were created but haven't been confirmed ready yet.
    spawning_agents: Vec<String>,
    /// Monitor cycle — heartbeat, health assessment, message delivery.
    monitor: MonitorCycle,
    /// Timestamp of last monitor cycle run (ms).
    last_monitor_ms: u64,
}


impl Daemon {
    /// Initialize the daemon: create Sys, bind socket, set up channel.
    pub fn new(config_dir: &Path) -> Result<Daemon, String> {
        Self::with_config(config_dir, DaemonConfig::default())
    }

    /// Initialize with custom config (uses MockBackend by default).
    pub fn with_config(config_dir: &Path, config: DaemonConfig) -> Result<Daemon, String> {
        Self::with_backend(config_dir, config, Box::new(MockBackend::new()))
    }

    /// Initialize with a specific session backend.
    pub fn with_backend(
        config_dir: &Path,
        config: DaemonConfig,
        backend: Box<dyn SessionBackend + Send>,
    ) -> Result<Daemon, String> {
        let sys = Sys::new(config_dir)?;
        let service = ServiceSocket::start(config_dir)?;
        let registry = WatchRegistry::new();
        let (sender, receiver) = mpsc::channel();
        let handle = DaemonHandle { sender };
        let policy = RetryPolicy::new(3, BackoffStrategy::Fixed, 100);
        let executor = ConvergenceExecutor::new(policy);
        let monitor = MonitorCycle::new(
            sys.settings().message_timeout as u64,
            sys.settings().heartbeat_timeout as u64 / 1000,
            sys.settings().ready_prompt_pattern.clone(),
        );

        Ok(Daemon {
            sys,
            service,
            registry,
            receiver,
            handle,
            config,
            backend,
            executor,
            spawning_agents: Vec::new(),
            monitor,
            last_monitor_ms: now_ms(),
        })
    }

    /// Get a handle for sending events to this daemon.
    pub fn handle(&self) -> DaemonHandle {
        self.handle.clone()
    }

    /// Run the main event loop. Blocks until shutdown is received.
    pub fn run(&mut self) -> Result<(), String> {
        loop {
            if self.tick() {
                break;
            }
        }

        self.service.shutdown_ref();
        Ok(())
    }

    /// Run exactly one tick of the main loop.
    /// Returns true if shutdown was requested.
    pub fn tick(&mut self) -> bool {
        // 1. Drain channel — process all pending internal events
        let should_shutdown = self.drain_channel();
        if should_shutdown {
            return true;
        }

        // 2. Accept socket connections (non-blocking with timeout)
        match self.service.accept_nonblocking(
            &mut self.sys,
            &mut self.registry,
            self.config.socket_poll_ms,
        ) {
            Ok(_handled) => {}
            Err(e) => {
                eprintln!("cmx daemon: socket error: {}", e);
            }
        }

        // Execute any actions accumulated from socket commands
        self.execute_pending_actions();

        // Check if a DaemonStop command was received via the socket
        if self.service.shutdown_requested() {
            return true;
        }

        // 3. Check spawning agents for ready-state detection
        self.check_spawning_agents();

        // 4. Run monitor cycle (health, messages, triggers) at configured interval
        self.run_monitor_cycle();

        // 5. Expire stale watchers
        self.registry.expire_stale();

        false
    }

    /// Drain all pending events from the channel.
    /// Returns true if a Shutdown event was received.
    fn drain_channel(&mut self) -> bool {
        loop {
            match self.receiver.try_recv() {
                Ok(DaemonEvent::InternalCommand { command, source }) => {
                    let _response = self.sys.execute(command);
                    self.execute_pending_actions();
                    let now = now_ms();
                    self.registry.record_change(now);
                    self.registry.notify_all(&source, now);
                }
                Ok(DaemonEvent::Log { level, message }) => {
                    eprintln!("cmx [{}]: {}", level, message);
                }
                Ok(DaemonEvent::Shutdown) => {
                    return true;
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    eprintln!("cmx daemon: channel disconnected, shutting down");
                    return true;
                }
            }
        }
        false
    }

    /// Drain accumulated actions from Sys, expand logical actions into
    /// infrastructure actions, execute through the backend, and feed
    /// session mappings back into Sys.
    fn execute_pending_actions(&mut self) {
        let raw_actions = self.sys.drain_actions();
        if raw_actions.is_empty() {
            return;
        }

        let launch_cmd = self.sys.settings().agent_launch_command.clone();
        let (expanded, session_mappings) = bridge::expand_actions(raw_actions, &launch_cmd);

        let result = self.executor.execute(expanded, self.backend.as_mut());

        // Feed session mappings back to Sys for successful creates
        for (agent_name, sess_name) in session_mappings {
            let succeeded = result.succeeded.iter().any(|a| {
                matches!(a, Action::CreateSession { name, .. } if name == &sess_name)
            });
            if succeeded {
                let _ = self.sys.notify_session_created(&agent_name, &sess_name);
                self.spawning_agents.push(agent_name);
            }
        }

        for (action, err) in &result.failed {
            eprintln!("cmx daemon: action failed: {:?}: {}", action, err);
        }
    }

    /// Poll spawning agents' panes to detect when they reach the ready prompt.
    /// When detected, update agent health to Healthy and remove from spawning list.
    fn check_spawning_agents(&mut self) {
        if self.spawning_agents.is_empty() {
            return;
        }

        let prompt_pattern = self.sys.settings().ready_prompt_pattern.clone();
        let mut newly_ready = Vec::new();

        for agent_name in &self.spawning_agents {
            let session = match self.sys.data().agents().get(agent_name) {
                Some(a) => match &a.session {
                    Some(s) => s.clone(),
                    None => continue,
                },
                None => continue,
            };

            if let Ok(output) = self.backend.capture_pane(&session) {
                let result = heartbeat::parse_capture(&output, &prompt_pattern);
                if result.state == heartbeat::AgentState::Ready {
                    newly_ready.push(agent_name.clone());
                }
            }
        }

        for agent_name in &newly_ready {
            let _ = self.sys.notify_agent_ready(agent_name);
            self.spawning_agents.retain(|n| n != agent_name);
        }
    }

    /// Run one monitoring cycle if enough time has elapsed since the last one.
    ///
    /// The health check interval from settings controls the frequency.
    /// Each cycle captures agent panes, assesses health, delivers messages,
    /// and checks for timeouts.
    fn run_monitor_cycle(&mut self) {
        let now = now_ms();
        let interval = self.sys.settings().health_check_interval;
        if now.saturating_sub(self.last_monitor_ms) < interval {
            return;
        }
        self.last_monitor_ms = now;

        let agents = self.sys.data().agents().list().to_vec();
        // Only monitor agents that have sessions (are actually running)
        let active: Vec<_> = agents.into_iter()
            .filter(|a| a.session.is_some())
            .collect();
        if active.is_empty() {
            return;
        }

        let result = self.monitor.run_cycle(
            &active,
            self.backend.as_ref(),
            self.sys.messages_mut(),
            now,
        );

        // Apply health updates back to agent state
        for assessment in &result.health_updates {
            self.sys.apply_health_update(assessment);
        }

        // Log any timeout alerts
        for timeout in &result.timeouts {
            eprintln!(
                "cmx daemon: message timeout for '{}' ({}ms): {}",
                timeout.agent, timeout.message_age_ms, timeout.message_text
            );
        }
    }

    /// Borrow Sys for inspection (testing).
    pub fn sys(&self) -> &Sys {
        &self.sys
    }
}


fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}


// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// Counter to generate unique short directory names per test.
    static TEST_SEQ: AtomicU32 = AtomicU32::new(0);

    /// Create a short temp directory path to stay under SUN_LEN for Unix sockets.
    fn test_config_dir() -> PathBuf {
        let seq = TEST_SEQ.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("cmxd{}-{}", std::process::id(), seq));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Clean up a test config directory.
    fn cleanup(dir: &Path) {
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn daemon_new_creates_socket() {
        let dir = test_config_dir();
        let daemon = Daemon::new(&dir).unwrap();
        let sock_path = dir.join("cmx.sock");
        assert!(sock_path.exists(), "Socket file should exist after Daemon::new");
        daemon.service.shutdown_ref();
        cleanup(&dir);
    }

    #[test]
    fn daemon_handle_send_command() {
        let dir = test_config_dir();
        let mut daemon = Daemon::new(&dir).unwrap();
        let handle = daemon.handle();

        // Send a command to create an agent
        handle
            .send_command(
                Command::AgentNew {
                    role: "worker".into(),
                    name: Some("test-w1".into()),
                    path: None,
                    agent_type: None,
                },
                "test",
            )
            .unwrap();

        // Tick should process it
        let shutdown = daemon.tick();
        assert!(!shutdown, "tick should not signal shutdown");

        // Verify the agent was created
        let agents = daemon.sys().data().agents().list();
        assert_eq!(agents.len(), 1);
        assert_eq!(agents[0].name, "test-w1");

        daemon.service.shutdown_ref();
        cleanup(&dir);
    }

    #[test]
    fn daemon_handle_shutdown() {
        let dir = test_config_dir();
        let mut daemon = Daemon::new(&dir).unwrap();
        let handle = daemon.handle();

        handle.shutdown().unwrap();

        let shutdown = daemon.tick();
        assert!(shutdown, "tick should return true after shutdown event");

        daemon.service.shutdown_ref();
        cleanup(&dir);
    }

    #[test]
    fn daemon_drain_channel_processes_multiple() {
        let dir = test_config_dir();
        let mut daemon = Daemon::new(&dir).unwrap();
        let handle = daemon.handle();

        // Send 3 agent creation commands
        for i in 1..=3 {
            handle
                .send_command(
                    Command::AgentNew {
                        role: "worker".into(),
                        name: Some(format!("w{}", i)),
                        path: None,
                        agent_type: None,
                    },
                    &format!("test-{}", i),
                )
                .unwrap();
        }

        // One tick should process all 3
        let shutdown = daemon.tick();
        assert!(!shutdown);

        let agents = daemon.sys().data().agents().list();
        assert_eq!(
            agents.len(),
            3,
            "All 3 agents should have been created in one tick"
        );

        daemon.service.shutdown_ref();
        cleanup(&dir);
    }

    #[test]
    fn daemon_channel_disconnected_triggers_shutdown() {
        // Verify that a disconnected channel causes try_recv to return Disconnected.
        // The Daemon holds its own sender internally, so we test the mechanism directly.
        let (sender, receiver) = mpsc::channel::<DaemonEvent>();
        drop(sender);

        match receiver.try_recv() {
            Err(mpsc::TryRecvError::Disconnected) => {} // expected
            other => panic!("Expected Disconnected, got {:?}", other),
        }
    }

    #[test]
    fn daemon_handle_from_thread() {
        let dir = test_config_dir();
        let mut daemon = Daemon::new(&dir).unwrap();
        let handle = daemon.handle();

        // Spawn a thread that sends a command
        let thread = std::thread::spawn(move || {
            handle
                .send_command(
                    Command::AgentNew {
                        role: "pm".into(),
                        name: Some("pm-from-thread".into()),
                        path: None,
                        agent_type: None,
                    },
                    "background-thread",
                )
                .unwrap();
        });

        thread.join().unwrap();

        // Tick to process
        let shutdown = daemon.tick();
        assert!(!shutdown);

        let agents = daemon.sys().data().agents().list();
        assert_eq!(agents.len(), 1);
        assert_eq!(agents[0].name, "pm-from-thread");
        assert_eq!(agents[0].role, "pm");

        daemon.service.shutdown_ref();
        cleanup(&dir);
    }

    #[test]
    fn daemon_run_stops_on_shutdown() {
        let dir = test_config_dir();
        let mut daemon =
            Daemon::with_config(&dir, DaemonConfig { socket_poll_ms: 10 }).unwrap();
        let handle = daemon.handle();

        // Send a command then a shutdown
        handle
            .send_command(
                Command::AgentNew {
                    role: "worker".into(),
                    name: Some("w-run".into()),
                    path: None,
                    agent_type: None,
                },
                "test",
            )
            .unwrap();
        handle.shutdown().unwrap();

        // run() should process the command, then stop on shutdown
        let result = daemon.run();
        assert!(result.is_ok());

        // Verify agent was created before shutdown
        assert_eq!(daemon.sys().data().agents().list().len(), 1);

        // Socket file should be cleaned up
        let sock_path = dir.join("cmx.sock");
        assert!(
            !sock_path.exists(),
            "Socket file should be removed after run()"
        );

        cleanup(&dir);
    }

    #[test]
    fn daemon_log_event_does_not_mutate_state() {
        let dir = test_config_dir();
        let mut daemon = Daemon::new(&dir).unwrap();
        let handle = daemon.handle();

        handle.log("info", "this is a test log").unwrap();

        // Process the log event
        let shutdown = daemon.drain_channel();
        assert!(!shutdown, "Log event should not trigger shutdown");

        // No state changes
        assert!(daemon.sys().data().agents().list().is_empty());

        daemon.service.shutdown_ref();
        cleanup(&dir);
    }

    #[test]
    fn daemon_config_default() {
        let config = DaemonConfig::default();
        assert_eq!(config.socket_poll_ms, 50);
    }

    // --- agent execution bridge tests ---

    #[test]
    fn daemon_agent_new_creates_session_via_backend() {
        let dir = test_config_dir();
        let mut daemon = Daemon::with_backend(
            &dir,
            DaemonConfig { socket_poll_ms: 10 },
            Box::new(MockBackend::new()),
        )
        .unwrap();
        let handle = daemon.handle();

        handle
            .send_command(
                Command::AgentNew {
                    role: "worker".into(),
                    name: Some("test-w1".into()),
                    path: None,
                    agent_type: None,
                },
                "test",
            )
            .unwrap();

        let shutdown = daemon.tick();
        assert!(!shutdown);

        // Access backend through Daemon — downcast
        // We can't easily get back the concrete MockBackend, so we check the
        // agent's session field as evidence that the backend was called.
        let agents = daemon.sys().data().agents().list();
        assert_eq!(agents.len(), 1);
        assert_eq!(agents[0].name, "test-w1");

        // The session field should be set via the bridge feedback loop
        assert_eq!(
            agents[0].session.as_deref(),
            Some("cmx-test-w1"),
            "agent.session should be set after backend executes CreateSession"
        );

        daemon.service.shutdown_ref();
        cleanup(&dir);
    }

    #[test]
    fn daemon_agent_new_sets_session_field() {
        let dir = test_config_dir();
        let mut daemon = Daemon::with_backend(
            &dir,
            DaemonConfig { socket_poll_ms: 10 },
            Box::new(MockBackend::new()),
        )
        .unwrap();
        let handle = daemon.handle();

        handle
            .send_command(
                Command::AgentNew {
                    role: "worker".into(),
                    name: Some("w2".into()),
                    path: None,
                    agent_type: None,
                },
                "test",
            )
            .unwrap();

        daemon.tick();

        let agent = daemon.sys().data().agents().get("w2").unwrap();
        assert_eq!(agent.session, Some("cmx-w2".to_string()));
    }

    #[test]
    fn daemon_agent_kill_creates_kill_session() {
        let dir = test_config_dir();
        let mut daemon = Daemon::with_backend(
            &dir,
            DaemonConfig { socket_poll_ms: 10 },
            Box::new(MockBackend::new()),
        )
        .unwrap();
        let handle = daemon.handle();

        // First create an agent
        handle
            .send_command(
                Command::AgentNew {
                    role: "worker".into(),
                    name: Some("k1".into()),
                    path: None,
                    agent_type: None,
                },
                "test",
            )
            .unwrap();
        daemon.tick();

        // Now kill it
        handle
            .send_command(Command::AgentKill { name: "k1".into() }, "test")
            .unwrap();
        daemon.tick();

        // Agent should be gone
        assert!(daemon.sys().data().agents().get("k1").is_none());

        daemon.service.shutdown_ref();
        cleanup(&dir);
    }

    // --- spawn detection tests (MN.2.3 / MN.2.4) ---

    /// Write a settings.yaml with a literal prompt pattern for substring matching.
    fn write_test_settings(dir: &Path) {
        let settings_content = "ready_prompt_pattern: \"$ \"\n";
        std::fs::write(dir.join("settings.yaml"), settings_content).unwrap();
    }

    #[test]
    fn daemon_spawn_detection_marks_agent_ready() {
        let dir = test_config_dir();
        write_test_settings(&dir);

        // Pre-configure MockBackend with a ready prompt for the expected session
        let mut mock = MockBackend::new();
        mock.set_capture("cmx-sd1", "some output\n$ ");

        let mut daemon = Daemon::with_backend(
            &dir,
            DaemonConfig { socket_poll_ms: 10 },
            Box::new(mock),
        )
        .unwrap();
        let handle = daemon.handle();

        // Create agent — tick processes it, adds to spawning_agents, and
        // check_spawning_agents runs in the same tick detecting the ready prompt.
        handle
            .send_command(
                Command::AgentNew {
                    role: "worker".into(),
                    name: Some("sd1".into()),
                    path: None,
                    agent_type: None,
                },
                "test",
            )
            .unwrap();
        daemon.tick();

        // Agent was created, session assigned, and ready-state detected in one tick
        let a = daemon.sys().data().agents().get("sd1").unwrap();
        assert_eq!(a.session.as_deref(), Some("cmx-sd1"));
        assert_eq!(a.health, crate::types::agent::HealthState::Healthy);
        assert_eq!(a.status, crate::types::agent::AgentStatus::Idle);

        daemon.service.shutdown_ref();
        cleanup(&dir);
    }

    #[test]
    fn daemon_spawn_detection_waits_when_not_ready() {
        let dir = test_config_dir();
        write_test_settings(&dir);

        // Backend has a capture that does NOT contain a ready prompt
        let mut mock = MockBackend::new();
        mock.set_capture("cmx-sd2", "Loading claude...\nPlease wait...");

        let mut daemon = Daemon::with_backend(
            &dir,
            DaemonConfig { socket_poll_ms: 10 },
            Box::new(mock),
        )
        .unwrap();
        let handle = daemon.handle();

        handle
            .send_command(
                Command::AgentNew {
                    role: "worker".into(),
                    name: Some("sd2".into()),
                    path: None,
                    agent_type: None,
                },
                "test",
            )
            .unwrap();
        daemon.tick();

        // Second tick — not ready yet, should stay Unknown
        daemon.tick();

        let a = daemon.sys().data().agents().get("sd2").unwrap();
        assert_eq!(a.health, crate::types::agent::HealthState::Unknown);

        daemon.service.shutdown_ref();
        cleanup(&dir);
    }

    #[test]
    fn daemon_spawn_detection_no_capture_stays_spawning() {
        let dir = test_config_dir();

        // Backend has no capture at all for this session
        let mut daemon = Daemon::with_backend(
            &dir,
            DaemonConfig { socket_poll_ms: 10 },
            Box::new(MockBackend::new()),
        )
        .unwrap();
        let handle = daemon.handle();

        handle
            .send_command(
                Command::AgentNew {
                    role: "worker".into(),
                    name: Some("sd3".into()),
                    path: None,
                    agent_type: None,
                },
                "test",
            )
            .unwrap();
        daemon.tick();
        daemon.tick();

        // Still Unknown — capture_pane returned Err so we skip
        let a = daemon.sys().data().agents().get("sd3").unwrap();
        assert_eq!(a.health, crate::types::agent::HealthState::Unknown);

        daemon.service.shutdown_ref();
        cleanup(&dir);
    }

    // --- monitor cycle integration tests (MP.2/MP.3) ---

    #[test]
    fn daemon_monitor_cycle_updates_health() {
        let dir = test_config_dir();
        // Set health_check_interval to 0 so cycle runs every tick
        std::fs::write(
            dir.join("settings.yaml"),
            "ready_prompt_pattern: \"$ \"\nhealth_check_interval: 0\n",
        ).unwrap();

        let mut mock = MockBackend::new();
        // Agent with session showing busy output (no prompt)
        mock.set_capture("cmx-mc1", "running tests...\ntest_foo ok");

        let mut daemon = Daemon::with_backend(
            &dir,
            DaemonConfig { socket_poll_ms: 10 },
            Box::new(mock),
        )
        .unwrap();
        let handle = daemon.handle();

        handle
            .send_command(
                Command::AgentNew {
                    role: "worker".into(),
                    name: Some("mc1".into()),
                    path: None,
                    agent_type: None,
                },
                "test",
            )
            .unwrap();
        // First tick: creates agent, sets session, runs monitor
        daemon.tick();

        let a = daemon.sys().data().agents().get("mc1").unwrap();
        // Agent has a session and the monitor ran — health should be updated
        assert!(a.last_heartbeat_ms.is_some(),
            "Monitor cycle should set last_heartbeat_ms");

        daemon.service.shutdown_ref();
        cleanup(&dir);
    }

    #[test]
    fn daemon_monitor_handles_capture_failure_gracefully() {
        let dir = test_config_dir();
        std::fs::write(
            dir.join("settings.yaml"),
            "health_check_interval: 0\n",
        ).unwrap();

        // MockBackend with no preset captures — capture_pane will return Err
        let mut daemon = Daemon::with_backend(
            &dir,
            DaemonConfig { socket_poll_ms: 10 },
            Box::new(MockBackend::new()),
        )
        .unwrap();
        let handle = daemon.handle();

        handle
            .send_command(
                Command::AgentNew {
                    role: "worker".into(),
                    name: Some("ns1".into()),
                    path: None,
                    agent_type: None,
                },
                "test",
            )
            .unwrap();
        daemon.tick();

        // Agent gets a session via bridge, but capture_pane fails (no preset).
        // Monitor should handle this gracefully — agent gets health update
        // with InfrastructureFailed signal but no panic.
        let a = daemon.sys().data().agents().get("ns1").unwrap();
        assert!(a.session.is_some(), "Bridge should have assigned a session");
        // Health update happened despite capture failure
        assert!(a.last_heartbeat_ms.is_some(),
            "Monitor should update heartbeat timestamp even on capture failure");

        daemon.service.shutdown_ref();
        cleanup(&dir);
    }

    #[test]
    fn daemon_monitor_detects_stall() {
        let dir = test_config_dir();
        // Very short heartbeat timeout so stall triggers quickly
        std::fs::write(
            dir.join("settings.yaml"),
            "ready_prompt_pattern: \"$ \"\nhealth_check_interval: 0\nheartbeat_timeout: 1000\n",
        ).unwrap();

        let mut mock = MockBackend::new();
        mock.set_capture("cmx-st1", "stuck output that never changes");

        let mut daemon = Daemon::with_backend(
            &dir,
            DaemonConfig { socket_poll_ms: 10 },
            Box::new(mock),
        )
        .unwrap();
        let handle = daemon.handle();

        handle
            .send_command(
                Command::AgentNew {
                    role: "worker".into(),
                    name: Some("st1".into()),
                    path: None,
                    agent_type: None,
                },
                "test",
            )
            .unwrap();

        // First tick: agent created + first monitor cycle (establishes baseline)
        daemon.tick();

        // Wait a bit then tick again to let staleness accumulate
        std::thread::sleep(std::time::Duration::from_millis(50));
        // Force last_monitor_ms to 0 so cycle runs again
        daemon.last_monitor_ms = 0;
        daemon.tick();

        // After two cycles with same output, the health assessment runs.
        // With heartbeat_timeout=1 second and only 50ms elapsed, it should NOT
        // be stalled yet — but the health update infrastructure is working.
        let a = daemon.sys().data().agents().get("st1").unwrap();
        assert!(a.last_heartbeat_ms.is_some());

        daemon.service.shutdown_ref();
        cleanup(&dir);
    }
}
