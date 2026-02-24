//! Remote command execution.
//!
//! `RemoteExecutor` manages a queue of commands to be run on remote hosts via
//! SSH. It tracks lifecycle state, handles timeouts, and builds SSH argument
//! vectors. Like all rig modules, it never spawns processes — the caller
//! executes the commands and reports results back.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::config::RemoteConfig;


// ---------------------------------------------------------------------------
// ExecStatus
// ---------------------------------------------------------------------------

/// Lifecycle status of a remote command execution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExecStatus {
    /// Waiting to be started.
    Queued,
    /// Running on the remote host.
    Running,
    /// Finished successfully (exit code may be non-zero).
    Completed,
    /// Killed because the timeout was exceeded.
    TimedOut,
    /// Failed due to infrastructure error (SSH failure, etc.).
    Failed,
}


// ---------------------------------------------------------------------------
// RemoteExecution
// ---------------------------------------------------------------------------

/// A single remote command execution and its metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteExecution {
    /// Unique identifier for this execution.
    pub id: String,
    /// Name of the remote host.
    pub remote: String,
    /// The shell command to execute remotely.
    pub command: String,
    /// Timeout in milliseconds, if any.
    pub timeout_ms: Option<u64>,
    /// Current lifecycle status.
    pub status: ExecStatus,
    /// Epoch-millisecond timestamp when execution started.
    pub started_ms: Option<u64>,
    /// Epoch-millisecond timestamp when execution completed.
    pub completed_ms: Option<u64>,
    /// Exit code of the remote process.
    pub exit_code: Option<i32>,
    /// Captured standard output.
    pub stdout: Option<String>,
    /// Captured standard error.
    pub stderr: Option<String>,
}


// ---------------------------------------------------------------------------
// RemoteExecutor
// ---------------------------------------------------------------------------

/// Manages queued and active remote command executions.
pub struct RemoteExecutor {
    /// Completed, timed-out, and failed executions (for history).
    history: Vec<RemoteExecution>,
    /// Queued executions waiting to start.
    queue: Vec<RemoteExecution>,
    /// Currently running executions, keyed by ID.
    active: HashMap<String, RemoteExecution>,
    /// Monotonic ID counter.
    next_id: u64,
    /// Default timeout applied when none is specified per-execution.
    default_timeout_ms: u64,
}

impl RemoteExecutor {
    /// Create a new executor with the given default timeout.
    pub fn new(default_timeout_ms: u64) -> Self {
        RemoteExecutor {
            history: Vec::new(),
            queue: Vec::new(),
            active: HashMap::new(),
            next_id: 1,
            default_timeout_ms,
        }
    }

    /// Queue a command for execution on a remote. Returns the execution ID.
    ///
    /// If `timeout_ms` is `None`, the `default_timeout_ms` is used.
    pub fn queue(
        &mut self,
        remote: &str,
        command: &str,
        timeout_ms: Option<u64>,
    ) -> String {
        let id = format!("exec-{}", self.next_id);
        self.next_id += 1;
        let exec = RemoteExecution {
            id: id.clone(),
            remote: remote.to_string(),
            command: command.to_string(),
            timeout_ms: Some(timeout_ms.unwrap_or(self.default_timeout_ms)),
            status: ExecStatus::Queued,
            started_ms: None,
            completed_ms: None,
            exit_code: None,
            stdout: None,
            stderr: None,
        };
        self.queue.push(exec);
        id
    }

    /// Start a queued execution (move it to active). Fails if the ID is not
    /// found in the queue.
    pub fn start(&mut self, exec_id: &str, now_ms: u64) -> Result<(), String> {
        let idx = self
            .queue
            .iter()
            .position(|e| e.id == exec_id)
            .ok_or_else(|| format!("execution '{}' not found in queue", exec_id))?;
        let mut exec = self.queue.remove(idx);
        exec.status = ExecStatus::Running;
        exec.started_ms = Some(now_ms);
        self.active.insert(exec.id.clone(), exec);
        Ok(())
    }

    /// Record that an execution completed (with its output and exit code).
    pub fn complete(
        &mut self,
        exec_id: &str,
        exit_code: i32,
        stdout: &str,
        stderr: &str,
        now_ms: u64,
    ) -> Result<(), String> {
        let mut exec = self
            .active
            .remove(exec_id)
            .ok_or_else(|| format!("no active execution '{}'", exec_id))?;
        exec.status = ExecStatus::Completed;
        exec.completed_ms = Some(now_ms);
        exec.exit_code = Some(exit_code);
        exec.stdout = Some(stdout.to_string());
        exec.stderr = Some(stderr.to_string());
        self.history.push(exec);
        Ok(())
    }

    /// Record that an execution timed out.
    pub fn timeout(&mut self, exec_id: &str, now_ms: u64) -> Result<(), String> {
        let mut exec = self
            .active
            .remove(exec_id)
            .ok_or_else(|| format!("no active execution '{}'", exec_id))?;
        exec.status = ExecStatus::TimedOut;
        exec.completed_ms = Some(now_ms);
        self.history.push(exec);
        Ok(())
    }

    /// Record that an execution failed due to an infrastructure error.
    pub fn fail(&mut self, exec_id: &str, error: &str, now_ms: u64) -> Result<(), String> {
        let mut exec = self
            .active
            .remove(exec_id)
            .ok_or_else(|| format!("no active execution '{}'", exec_id))?;
        exec.status = ExecStatus::Failed;
        exec.completed_ms = Some(now_ms);
        exec.stderr = Some(error.to_string());
        self.history.push(exec);
        Ok(())
    }

    /// Look up an execution by ID across queue, active, and history.
    pub fn get(&self, exec_id: &str) -> Option<&RemoteExecution> {
        self.active
            .get(exec_id)
            .or_else(|| self.queue.iter().find(|e| e.id == exec_id))
            .or_else(|| self.history.iter().find(|e| e.id == exec_id))
    }

    /// Return all active executions targeting a specific remote.
    pub fn active_for(&self, remote: &str) -> Vec<&RemoteExecution> {
        self.active
            .values()
            .filter(|e| e.remote == remote)
            .collect()
    }

    /// Return all historical (completed/failed/timed-out) executions for a
    /// specific remote.
    pub fn history_for(&self, remote: &str) -> Vec<&RemoteExecution> {
        self.history
            .iter()
            .filter(|e| e.remote == remote)
            .collect()
    }

    /// Check all active executions for timeouts. Returns the IDs of
    /// executions that have exceeded their timeout as of `now_ms`.
    ///
    /// The caller should then call `timeout()` on each returned ID.
    pub fn check_timeouts(&self, now_ms: u64) -> Vec<String> {
        let mut timed_out = Vec::new();
        for exec in self.active.values() {
            if let (Some(started), Some(timeout)) = (exec.started_ms, exec.timeout_ms) {
                if now_ms.saturating_sub(started) >= timeout {
                    timed_out.push(exec.id.clone());
                }
            }
        }
        timed_out
    }

    /// Build the SSH command argument vector for an execution.
    ///
    /// The resulting `Vec<String>` can be passed to `std::process::Command`
    /// with `"ssh"` as the program.
    pub fn build_ssh_command(
        &self,
        exec: &RemoteExecution,
        config: &RemoteConfig,
    ) -> Vec<String> {
        let mut args = config.ssh_base_args();
        // Wrap the remote command in a single string argument so that the
        // remote shell handles pipes, redirects, etc.
        args.push(exec.command.clone());
        args
    }

    /// Number of queued executions.
    pub fn queued_count(&self) -> usize {
        self.queue.len()
    }

    /// Number of active executions.
    pub fn active_count(&self) -> usize {
        self.active.len()
    }

    /// All completed/failed/timed-out executions.
    pub fn history(&self) -> &[RemoteExecution] {
        &self.history
    }
}


// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> RemoteConfig {
        RemoteConfig {
            name: "r1".to_string(),
            host: "10.0.0.1".to_string(),
            port: 22,
            user: "ubuntu".to_string(),
            ssh_key: None,
            workspace_dir: "/home/ubuntu/work".to_string(),
            gpu_count: None,
            labels: Vec::new(),
        }
    }

    fn test_config_with_key() -> RemoteConfig {
        RemoteConfig {
            name: "r1".to_string(),
            host: "10.0.0.1".to_string(),
            port: 2222,
            user: "deploy".to_string(),
            ssh_key: Some("/keys/id_rsa".to_string()),
            workspace_dir: "/data/work".to_string(),
            gpu_count: Some(8),
            labels: Vec::new(),
        }
    }

    // -- Queue --

    #[test]
    fn queue_returns_unique_ids() {
        let mut executor = RemoteExecutor::new(60_000);
        let id1 = executor.queue("r1", "echo hello", None);
        let id2 = executor.queue("r1", "echo world", None);
        assert_ne!(id1, id2);
        assert_eq!(executor.queued_count(), 2);
    }

    #[test]
    fn queue_uses_default_timeout() {
        let mut executor = RemoteExecutor::new(30_000);
        let id = executor.queue("r1", "ls", None);
        let exec = executor.get(&id).unwrap();
        assert_eq!(exec.timeout_ms, Some(30_000));
    }

    #[test]
    fn queue_uses_custom_timeout() {
        let mut executor = RemoteExecutor::new(30_000);
        let id = executor.queue("r1", "ls", Some(5_000));
        let exec = executor.get(&id).unwrap();
        assert_eq!(exec.timeout_ms, Some(5_000));
    }

    // -- Start --

    #[test]
    fn start_moves_to_active() {
        let mut executor = RemoteExecutor::new(60_000);
        let id = executor.queue("r1", "echo hi", None);
        executor.start(&id, 1000).unwrap();
        assert_eq!(executor.queued_count(), 0);
        assert_eq!(executor.active_count(), 1);
        let exec = executor.get(&id).unwrap();
        assert_eq!(exec.status, ExecStatus::Running);
        assert_eq!(exec.started_ms, Some(1000));
    }

    #[test]
    fn start_missing_id_fails() {
        let mut executor = RemoteExecutor::new(60_000);
        assert!(executor.start("nope", 0).is_err());
    }

    // -- Complete --

    #[test]
    fn complete_records_output() {
        let mut executor = RemoteExecutor::new(60_000);
        let id = executor.queue("r1", "echo hi", None);
        executor.start(&id, 1000).unwrap();
        executor
            .complete(&id, 0, "hi\n", "", 2000)
            .unwrap();

        let exec = executor.get(&id).unwrap();
        assert_eq!(exec.status, ExecStatus::Completed);
        assert_eq!(exec.exit_code, Some(0));
        assert_eq!(exec.stdout, Some("hi\n".to_string()));
        assert_eq!(exec.stderr, Some("".to_string()));
        assert_eq!(exec.completed_ms, Some(2000));

        assert_eq!(executor.active_count(), 0);
        assert_eq!(executor.history().len(), 1);
    }

    #[test]
    fn complete_non_zero_exit() {
        let mut executor = RemoteExecutor::new(60_000);
        let id = executor.queue("r1", "false", None);
        executor.start(&id, 1000).unwrap();
        executor
            .complete(&id, 1, "", "error\n", 2000)
            .unwrap();

        let exec = executor.get(&id).unwrap();
        assert_eq!(exec.status, ExecStatus::Completed);
        assert_eq!(exec.exit_code, Some(1));
    }

    #[test]
    fn complete_missing_fails() {
        let mut executor = RemoteExecutor::new(60_000);
        assert!(executor.complete("nope", 0, "", "", 0).is_err());
    }

    // -- Timeout --

    #[test]
    fn timeout_records_correctly() {
        let mut executor = RemoteExecutor::new(60_000);
        let id = executor.queue("r1", "sleep 999", None);
        executor.start(&id, 1000).unwrap();
        executor.timeout(&id, 61_000).unwrap();

        let exec = executor.get(&id).unwrap();
        assert_eq!(exec.status, ExecStatus::TimedOut);
        assert_eq!(exec.completed_ms, Some(61_000));
        assert_eq!(executor.active_count(), 0);
    }

    #[test]
    fn timeout_missing_fails() {
        let mut executor = RemoteExecutor::new(60_000);
        assert!(executor.timeout("nope", 0).is_err());
    }

    // -- Fail --

    #[test]
    fn fail_records_error() {
        let mut executor = RemoteExecutor::new(60_000);
        let id = executor.queue("r1", "cmd", None);
        executor.start(&id, 1000).unwrap();
        executor.fail(&id, "SSH connection lost", 1500).unwrap();

        let exec = executor.get(&id).unwrap();
        assert_eq!(exec.status, ExecStatus::Failed);
        assert_eq!(exec.stderr, Some("SSH connection lost".to_string()));
    }

    #[test]
    fn fail_missing_fails() {
        let mut executor = RemoteExecutor::new(60_000);
        assert!(executor.fail("nope", "err", 0).is_err());
    }

    // -- Lookup --

    #[test]
    fn get_finds_in_queue() {
        let mut executor = RemoteExecutor::new(60_000);
        let id = executor.queue("r1", "cmd", None);
        assert!(executor.get(&id).is_some());
        assert_eq!(executor.get(&id).unwrap().status, ExecStatus::Queued);
    }

    #[test]
    fn get_finds_in_active() {
        let mut executor = RemoteExecutor::new(60_000);
        let id = executor.queue("r1", "cmd", None);
        executor.start(&id, 100).unwrap();
        assert_eq!(executor.get(&id).unwrap().status, ExecStatus::Running);
    }

    #[test]
    fn get_finds_in_history() {
        let mut executor = RemoteExecutor::new(60_000);
        let id = executor.queue("r1", "cmd", None);
        executor.start(&id, 100).unwrap();
        executor.complete(&id, 0, "ok", "", 200).unwrap();
        assert_eq!(executor.get(&id).unwrap().status, ExecStatus::Completed);
    }

    #[test]
    fn get_returns_none_for_unknown() {
        let executor = RemoteExecutor::new(60_000);
        assert!(executor.get("ghost").is_none());
    }

    // -- Filtering by remote --

    #[test]
    fn active_for_filters() {
        let mut executor = RemoteExecutor::new(60_000);
        let id1 = executor.queue("r1", "cmd1", None);
        let id2 = executor.queue("r2", "cmd2", None);
        let _id3 = executor.queue("r1", "cmd3", None);
        executor.start(&id1, 100).unwrap();
        executor.start(&id2, 100).unwrap();

        let r1_active = executor.active_for("r1");
        assert_eq!(r1_active.len(), 1);
        assert_eq!(r1_active[0].command, "cmd1");

        let r2_active = executor.active_for("r2");
        assert_eq!(r2_active.len(), 1);
        assert_eq!(r2_active[0].command, "cmd2");
    }

    #[test]
    fn history_for_filters() {
        let mut executor = RemoteExecutor::new(60_000);
        let id1 = executor.queue("r1", "cmd1", None);
        let id2 = executor.queue("r2", "cmd2", None);
        executor.start(&id1, 100).unwrap();
        executor.start(&id2, 100).unwrap();
        executor.complete(&id1, 0, "", "", 200).unwrap();
        executor.complete(&id2, 0, "", "", 200).unwrap();

        assert_eq!(executor.history_for("r1").len(), 1);
        assert_eq!(executor.history_for("r2").len(), 1);
        assert_eq!(executor.history_for("r3").len(), 0);
    }

    // -- Timeout checking --

    #[test]
    fn check_timeouts_detects_expired() {
        let mut executor = RemoteExecutor::new(1000);
        let id1 = executor.queue("r1", "slow", None); // 1000ms timeout
        let id2 = executor.queue("r1", "fast", Some(5000)); // 5000ms timeout
        executor.start(&id1, 100).unwrap();
        executor.start(&id2, 100).unwrap();

        // At time 1200: id1 has been running for 1100ms > 1000ms timeout.
        let expired = executor.check_timeouts(1200);
        assert_eq!(expired.len(), 1);
        assert!(expired.contains(&id1));

        // At time 6000: both should be expired.
        let expired = executor.check_timeouts(6000);
        assert_eq!(expired.len(), 2);
    }

    #[test]
    fn check_timeouts_no_false_positives() {
        let mut executor = RemoteExecutor::new(10_000);
        let id = executor.queue("r1", "cmd", None);
        executor.start(&id, 1000).unwrap();

        // At time 5000: only 4000ms elapsed, well under 10000ms timeout.
        let expired = executor.check_timeouts(5000);
        assert!(expired.is_empty());
    }

    // -- SSH command building --

    #[test]
    fn build_ssh_command_basic() {
        let executor = RemoteExecutor::new(60_000);
        let exec = RemoteExecution {
            id: "exec-1".to_string(),
            remote: "r1".to_string(),
            command: "nvidia-smi".to_string(),
            timeout_ms: Some(60_000),
            status: ExecStatus::Running,
            started_ms: Some(1000),
            completed_ms: None,
            exit_code: None,
            stdout: None,
            stderr: None,
        };
        let config = test_config();
        let args = executor.build_ssh_command(&exec, &config);

        assert!(args.contains(&"-p".to_string()));
        assert!(args.contains(&"22".to_string()));
        assert!(args.contains(&"ubuntu@10.0.0.1".to_string()));
        assert_eq!(args.last().unwrap(), "nvidia-smi");
    }

    #[test]
    fn build_ssh_command_with_key() {
        let executor = RemoteExecutor::new(60_000);
        let exec = RemoteExecution {
            id: "exec-1".to_string(),
            remote: "r1".to_string(),
            command: "ls -la /data".to_string(),
            timeout_ms: Some(5_000),
            status: ExecStatus::Running,
            started_ms: Some(1000),
            completed_ms: None,
            exit_code: None,
            stdout: None,
            stderr: None,
        };
        let config = test_config_with_key();
        let args = executor.build_ssh_command(&exec, &config);

        assert!(args.contains(&"-i".to_string()));
        assert!(args.contains(&"/keys/id_rsa".to_string()));
        assert!(args.contains(&"-p".to_string()));
        assert!(args.contains(&"2222".to_string()));
        assert!(args.contains(&"deploy@10.0.0.1".to_string()));
        assert_eq!(args.last().unwrap(), "ls -la /data");
    }

    #[test]
    fn build_ssh_command_complex_remote_command() {
        let executor = RemoteExecutor::new(60_000);
        let exec = RemoteExecution {
            id: "exec-1".to_string(),
            remote: "r1".to_string(),
            command: "cd /data/project && python train.py --epochs 10 2>&1 | tee log.txt"
                .to_string(),
            timeout_ms: Some(600_000),
            status: ExecStatus::Running,
            started_ms: Some(1000),
            completed_ms: None,
            exit_code: None,
            stdout: None,
            stderr: None,
        };
        let config = test_config();
        let args = executor.build_ssh_command(&exec, &config);

        // The complex command should be the last argument, passed as a single string.
        assert_eq!(
            args.last().unwrap(),
            "cd /data/project && python train.py --epochs 10 2>&1 | tee log.txt"
        );
    }

    // -- Full lifecycle --

    #[test]
    fn full_lifecycle_sequence() {
        let mut executor = RemoteExecutor::new(60_000);

        // Queue several.
        let id1 = executor.queue("r1", "echo 1", None);
        let id2 = executor.queue("r1", "echo 2", None);
        let id3 = executor.queue("r2", "echo 3", None);
        assert_eq!(executor.queued_count(), 3);

        // Start all.
        executor.start(&id1, 100).unwrap();
        executor.start(&id2, 100).unwrap();
        executor.start(&id3, 200).unwrap();
        assert_eq!(executor.queued_count(), 0);
        assert_eq!(executor.active_count(), 3);

        // Complete one, timeout another, fail the third.
        executor.complete(&id1, 0, "1\n", "", 500).unwrap();
        executor.timeout(&id2, 60_100).unwrap();
        executor.fail(&id3, "connection reset", 300).unwrap();

        assert_eq!(executor.active_count(), 0);
        assert_eq!(executor.history().len(), 3);

        // Verify individual states.
        assert_eq!(executor.get(&id1).unwrap().status, ExecStatus::Completed);
        assert_eq!(executor.get(&id2).unwrap().status, ExecStatus::TimedOut);
        assert_eq!(executor.get(&id3).unwrap().status, ExecStatus::Failed);
    }
}
