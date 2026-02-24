//! Rig orchestrator — connects command builders to `ShellRunner`.
//!
//! `RigOrchestrator` is the integration layer that wires the pure state machines
//! (ConnectionTracker, SyncManager, RemoteExecutor, WorkerPool) to real command
//! execution via a `CommandRunner`. It is the only rig component that causes
//! side effects (through the injected runner).

use crate::infrastructure::runner::CommandRunner;
use crate::rig::config::RigRegistry;
use crate::rig::connection::ConnectionTracker;
use crate::rig::remote::RemoteExecutor;
use crate::rig::sync::SyncManager;
use crate::rig::worker::WorkerPool;

use std::fmt;


/// Orchestrates the rig lifecycle: connect, sync, execute, collect, decommission.
///
/// All actual execution goes through the injected `CommandRunner`
/// (`ShellRunner` in production, `MockRunner` in tests).
pub struct RigOrchestrator {
    pub registry: RigRegistry,
    pub connections: ConnectionTracker,
    pub sync_manager: SyncManager,
    pub executor: RemoteExecutor,
    pub workers: WorkerPool,
    runner: Box<dyn CommandRunner>,
}

impl fmt::Debug for RigOrchestrator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RigOrchestrator")
            .field("registry", &self.registry)
            .finish()
    }
}

impl RigOrchestrator {
    /// Create a new orchestrator with the given registry and command runner.
    pub fn new(registry: RigRegistry, runner: Box<dyn CommandRunner>) -> Self {
        Self {
            registry,
            connections: ConnectionTracker::new(3, 1000),
            sync_manager: SyncManager::new(2),
            executor: RemoteExecutor::new(300_000),
            workers: WorkerPool::new(4),
            runner,
        }
    }

    /// Initialize a remote: verify SSH connectivity, register in tracker.
    pub fn init_remote(&mut self, name: &str) -> Result<String, String> {
        let config = self
            .registry
            .get(name)
            .ok_or_else(|| format!("Remote '{}' not found", name))?
            .clone();

        let now = now_ms();
        self.connections.register(name);
        self.connections.start_connecting(name, now)?;

        // Test SSH connectivity
        let health_cmd = format!("ssh {} echo ok", config.ssh_base_args().join(" "));
        match self.runner.run(&health_cmd) {
            Ok(output) if output.trim() == "ok" => {
                let done = now_ms();
                let latency = done.saturating_sub(now);
                self.connections.mark_connected(name, done, latency)?;
                Ok(format!(
                    "Remote '{}' connected ({})",
                    name,
                    config.user_at_host()
                ))
            }
            Ok(output) => {
                self.connections
                    .mark_failed(name, "unexpected response", now_ms())?;
                Err(format!(
                    "Unexpected response from {}: {}",
                    name,
                    output.trim()
                ))
            }
            Err(e) => {
                self.connections
                    .mark_failed(name, &e, now_ms())?;
                Err(format!("SSH connection to '{}' failed: {}", name, e))
            }
        }
    }

    /// Push code to a remote via rsync.
    pub fn push(&mut self, name: &str, local_path: &str) -> Result<String, String> {
        let config = self
            .registry
            .get(name)
            .ok_or_else(|| format!("Remote '{}' not found", name))?
            .clone();

        let job_id = self
            .sync_manager
            .queue_push(name, local_path, &config.workspace_dir);
        let now = now_ms();
        let job = self
            .sync_manager
            .start_next(now)
            .ok_or_else(|| "Failed to start sync job".to_string())?
            .clone();

        let args = self.sync_manager.build_rsync_args(&job, &config);
        let cmd = format!("rsync {}", args.join(" "));

        match self.runner.run(&cmd) {
            Ok(output) => {
                self.sync_manager.complete(&job_id, 0, now_ms())?;
                Ok(format!("Push to '{}' complete\n{}", name, output))
            }
            Err(e) => {
                self.sync_manager.fail(&job_id, &e, now_ms())?;
                Err(format!("Push to '{}' failed: {}", name, e))
            }
        }
    }

    /// Pull results from a remote via rsync.
    pub fn pull(&mut self, name: &str, local_path: &str) -> Result<String, String> {
        let config = self
            .registry
            .get(name)
            .ok_or_else(|| format!("Remote '{}' not found", name))?
            .clone();

        let job_id = self
            .sync_manager
            .queue_pull(name, &config.workspace_dir, local_path);
        let now = now_ms();
        let job = self
            .sync_manager
            .start_next(now)
            .ok_or_else(|| "Failed to start sync job".to_string())?
            .clone();

        let args = self.sync_manager.build_rsync_args(&job, &config);
        let cmd = format!("rsync {}", args.join(" "));

        match self.runner.run(&cmd) {
            Ok(output) => {
                self.sync_manager.complete(&job_id, 0, now_ms())?;
                Ok(format!("Pull from '{}' complete\n{}", name, output))
            }
            Err(e) => {
                self.sync_manager.fail(&job_id, &e, now_ms())?;
                Err(format!("Pull from '{}' failed: {}", name, e))
            }
        }
    }

    /// Execute a command on a remote host via SSH.
    pub fn execute_remote(
        &mut self,
        name: &str,
        command: &str,
        timeout_ms: Option<u64>,
    ) -> Result<String, String> {
        let config = self
            .registry
            .get(name)
            .ok_or_else(|| format!("Remote '{}' not found", name))?
            .clone();

        let exec_id = self.executor.queue(name, command, timeout_ms);
        let now = now_ms();
        self.executor.start(&exec_id, now)?;

        let exec = self
            .executor
            .get(&exec_id)
            .ok_or_else(|| "Execution not found after start".to_string())?
            .clone();
        let args = self.executor.build_ssh_command(&exec, &config);
        let cmd = format!("ssh {}", args.join(" "));

        match self.runner.run(&cmd) {
            Ok(output) => {
                self.executor
                    .complete(&exec_id, 0, &output, "", now_ms())?;
                Ok(output)
            }
            Err(e) => {
                self.executor.fail(&exec_id, &e, now_ms())?;
                Err(e)
            }
        }
    }

    /// Check SSH health for a remote.
    pub fn health_check(&mut self, name: &str) -> Result<String, String> {
        let config = self
            .registry
            .get(name)
            .ok_or_else(|| format!("Remote '{}' not found", name))?
            .clone();

        // Ensure the remote is registered with the connection tracker.
        self.connections.register(name);

        let now = now_ms();
        let health_cmd = format!("ssh {} echo ok", config.ssh_base_args().join(" "));
        match self.runner.run(&health_cmd) {
            Ok(_) => {
                let done = now_ms();
                let latency = done.saturating_sub(now);
                // Transition through Connecting before Connected
                let _ = self.connections.start_connecting(name, now);
                let _ = self.connections.mark_connected(name, done, latency);
                Ok(format!("Remote '{}': healthy", name))
            }
            Err(e) => {
                let _ = self.connections.start_connecting(name, now);
                let _ = self.connections.mark_failed(name, &e, now_ms());
                Err(format!("Remote '{}': unreachable ({})", name, e))
            }
        }
    }

    /// Get status summary for a remote.
    pub fn status(&self, name: &str) -> Result<String, String> {
        let config = self
            .registry
            .get(name)
            .ok_or_else(|| format!("Remote '{}' not found", name))?;

        let conn_state = self
            .connections
            .state(name)
            .map(|s| format!("{:?}", s))
            .unwrap_or_else(|| "untracked".into());

        let workers = self.workers.by_remote(name);
        let worker_info = if workers.is_empty() {
            "no workers".into()
        } else {
            workers
                .iter()
                .map(|w| format!("{} ({:?})", w.name, w.state))
                .collect::<Vec<_>>()
                .join(", ")
        };

        Ok(format!(
            "Remote '{}'\n  Host: {}\n  Connection: {}\n  Workers: {}",
            name,
            config.user_at_host(),
            conn_state,
            worker_info
        ))
    }

    /// Stop any running operations on a remote (kill remote tmux session).
    pub fn stop(&mut self, name: &str) -> Result<String, String> {
        let config = self
            .registry
            .get(name)
            .ok_or_else(|| format!("Remote '{}' not found", name))?
            .clone();

        let kill_cmd = format!(
            "ssh {} tmux kill-session -t cmx 2>/dev/null; echo done",
            config.ssh_base_args().join(" ")
        );

        self.runner
            .run(&kill_cmd)
            .map(|_| format!("Stopped remote '{}'", name))
            .map_err(|e| format!("Failed to stop '{}': {}", name, e))
    }

    /// Execute a command with nonce-based completion detection.
    pub fn execute_with_nonce(
        &mut self,
        name: &str,
        command: &str,
    ) -> Result<String, String> {
        let nonce = uuid_v4_simple();
        let wrapped = format!(
            "{} && echo {} > /tmp/_cmx_done_{}",
            command, nonce, nonce
        );
        self.execute_remote(name, &wrapped, None)
    }
}


/// Generate a simple nonce from the current timestamp.
fn uuid_v4_simple() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format!("cmx-{}-{}", ts.as_secs(), ts.subsec_nanos())
}

/// Simple wall-clock milliseconds.
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
    use crate::infrastructure::runner::MockRunner;
    use crate::rig::config::RemoteConfig;

    fn make_config(name: &str) -> RemoteConfig {
        RemoteConfig {
            name: name.to_string(),
            host: "10.0.0.1".to_string(),
            port: 22,
            user: "ubuntu".to_string(),
            ssh_key: None,
            workspace_dir: "/home/ubuntu/work".to_string(),
            gpu_count: None,
            labels: Vec::new(),
        }
    }

    fn make_registry(name: &str) -> RigRegistry {
        let mut reg = RigRegistry::new();
        reg.add(make_config(name)).unwrap();
        reg
    }

    #[test]
    fn init_remote_success() {
        let registry = make_registry("r1");
        let runner = MockRunner::with_responses(vec![Ok("ok\n".into())]);
        let mut rig = RigOrchestrator::new(registry, Box::new(runner));
        let result = rig.init_remote("r1");
        assert!(result.is_ok());
        let msg = result.unwrap();
        assert!(msg.contains("connected"));
        assert!(msg.contains("r1"));
        assert!(rig.connections.is_connected("r1"));
    }

    #[test]
    fn init_remote_ssh_failure() {
        let registry = make_registry("r1");
        let runner = MockRunner::with_responses(vec![Err("Connection refused".into())]);
        let mut rig = RigOrchestrator::new(registry, Box::new(runner));
        let result = rig.init_remote("r1");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("failed"));
        assert!(!rig.connections.is_connected("r1"));
    }

    #[test]
    fn init_remote_unexpected_response() {
        let registry = make_registry("r1");
        let runner = MockRunner::with_responses(vec![Ok("not ok\n".into())]);
        let mut rig = RigOrchestrator::new(registry, Box::new(runner));
        let result = rig.init_remote("r1");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Unexpected response"));
    }

    #[test]
    fn init_remote_unknown_remote() {
        let registry = RigRegistry::new();
        let runner = MockRunner::new();
        let mut rig = RigOrchestrator::new(registry, Box::new(runner));
        let result = rig.init_remote("nonexistent");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not found"));
    }

    #[test]
    fn push_success() {
        let registry = make_registry("r1");
        let runner =
            MockRunner::with_responses(vec![Ok("sending incremental file list\n".into())]);
        let mut rig = RigOrchestrator::new(registry, Box::new(runner));
        let result = rig.push("r1", "/local/project");
        assert!(result.is_ok());
        assert!(result.unwrap().contains("Push to 'r1' complete"));
    }

    #[test]
    fn push_failure() {
        let registry = make_registry("r1");
        let runner = MockRunner::with_responses(vec![Err("rsync: connection unexpectedly closed".into())]);
        let mut rig = RigOrchestrator::new(registry, Box::new(runner));
        let result = rig.push("r1", "/local/project");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("failed"));
    }

    #[test]
    fn pull_success() {
        let registry = make_registry("r1");
        let runner = MockRunner::with_responses(vec![Ok("receiving file list\n".into())]);
        let mut rig = RigOrchestrator::new(registry, Box::new(runner));
        let result = rig.pull("r1", "/local/results");
        assert!(result.is_ok());
        assert!(result.unwrap().contains("Pull from 'r1' complete"));
    }

    #[test]
    fn execute_remote_success() {
        let registry = make_registry("r1");
        let runner = MockRunner::with_responses(vec![Ok("result output\n".into())]);
        let mut rig = RigOrchestrator::new(registry, Box::new(runner));
        let result = rig.execute_remote("r1", "nvidia-smi", None);
        assert!(result.is_ok());
        assert!(result.unwrap().contains("result output"));
    }

    #[test]
    fn execute_remote_failure() {
        let registry = make_registry("r1");
        let runner = MockRunner::with_responses(vec![Err("command not found".into())]);
        let mut rig = RigOrchestrator::new(registry, Box::new(runner));
        let result = rig.execute_remote("r1", "bad-cmd", None);
        assert!(result.is_err());
    }

    #[test]
    fn health_check_success() {
        let registry = make_registry("r1");
        let runner = MockRunner::with_responses(vec![Ok("ok\n".into())]);
        let mut rig = RigOrchestrator::new(registry, Box::new(runner));
        let result = rig.health_check("r1");
        assert!(result.is_ok());
        assert!(result.unwrap().contains("healthy"));
        assert!(rig.connections.is_connected("r1"));
    }

    #[test]
    fn health_check_failure() {
        let registry = make_registry("r1");
        let runner = MockRunner::with_responses(vec![Err("timeout".into())]);
        let mut rig = RigOrchestrator::new(registry, Box::new(runner));
        let result = rig.health_check("r1");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("unreachable"));
    }

    #[test]
    fn status_shows_info() {
        let registry = make_registry("r1");
        let runner = MockRunner::new();
        let rig = RigOrchestrator::new(registry, Box::new(runner));
        let result = rig.status("r1");
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.contains("ubuntu@10.0.0.1"));
        assert!(output.contains("no workers"));
    }

    #[test]
    fn status_unknown_remote() {
        let registry = RigRegistry::new();
        let runner = MockRunner::new();
        let rig = RigOrchestrator::new(registry, Box::new(runner));
        let result = rig.status("nonexistent");
        assert!(result.is_err());
    }

    #[test]
    fn stop_success() {
        let registry = make_registry("r1");
        let runner = MockRunner::with_responses(vec![Ok("done\n".into())]);
        let mut rig = RigOrchestrator::new(registry, Box::new(runner));
        let result = rig.stop("r1");
        assert!(result.is_ok());
        assert!(result.unwrap().contains("Stopped"));
    }

    #[test]
    fn stop_unknown_remote() {
        let registry = RigRegistry::new();
        let runner = MockRunner::new();
        let mut rig = RigOrchestrator::new(registry, Box::new(runner));
        let result = rig.stop("nonexistent");
        assert!(result.is_err());
    }

    #[test]
    fn execute_with_nonce() {
        let registry = make_registry("r1");
        let runner = MockRunner::with_responses(vec![Ok("done\n".into())]);
        let mut rig = RigOrchestrator::new(registry, Box::new(runner));
        let result = rig.execute_with_nonce("r1", "python train.py");
        assert!(result.is_ok());
    }

    #[test]
    fn unknown_remote_errors() {
        let registry = RigRegistry::new();
        let runner = MockRunner::new();
        let mut rig = RigOrchestrator::new(registry, Box::new(runner));

        assert!(rig.init_remote("ghost").is_err());
        assert!(rig.push("ghost", "/path").is_err());
        assert!(rig.pull("ghost", "/path").is_err());
        assert!(rig.execute_remote("ghost", "cmd", None).is_err());
        assert!(rig.status("ghost").is_err());
        assert!(rig.stop("ghost").is_err());
    }
}
