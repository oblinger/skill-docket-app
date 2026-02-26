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

        Ok(Daemon {
            sys,
            service,
            registry,
            receiver,
            handle,
            config,
            backend,
            executor,
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

        // 3. Expire stale watchers
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
            }
        }

        for (action, err) in &result.failed {
            eprintln!("cmx daemon: action failed: {:?}: {}", action, err);
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
}
