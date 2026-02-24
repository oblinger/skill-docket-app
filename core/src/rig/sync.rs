//! File synchronisation via rsync.
//!
//! `SyncManager` maintains a queue of push/pull jobs and tracks their lifecycle.
//! It builds rsync argument vectors from job metadata and `RemoteConfig` but
//! never spawns processes — the caller is responsible for execution.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::config::RemoteConfig;


// ---------------------------------------------------------------------------
// SyncDirection / SyncStatus
// ---------------------------------------------------------------------------

/// Whether a sync job pushes files to or pulls files from a remote.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SyncDirection {
    /// Local -> Remote.
    Push,
    /// Remote -> Local.
    Pull,
}

/// Lifecycle status of a sync job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SyncStatus {
    /// Waiting to be started.
    Queued,
    /// Transfer in progress.
    Running,
    /// Transfer finished successfully.
    Completed,
    /// Transfer failed.
    Failed,
}


// ---------------------------------------------------------------------------
// SyncJob
// ---------------------------------------------------------------------------

/// A single file synchronisation operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncJob {
    /// Unique identifier for this job.
    pub id: String,
    /// Name of the remote host involved.
    pub remote: String,
    /// Push or Pull.
    pub direction: SyncDirection,
    /// Local filesystem path.
    pub local_path: String,
    /// Path on the remote host.
    pub remote_path: String,
    /// Glob patterns to exclude from the transfer.
    pub exclude_patterns: Vec<String>,
    /// Current lifecycle status.
    pub status: SyncStatus,
    /// Epoch-millisecond timestamp when the transfer started.
    pub started_ms: Option<u64>,
    /// Epoch-millisecond timestamp when the transfer completed.
    pub completed_ms: Option<u64>,
    /// Bytes transferred (reported on completion).
    pub bytes_transferred: Option<u64>,
    /// Error message on failure.
    pub error: Option<String>,
}


// ---------------------------------------------------------------------------
// SyncManager
// ---------------------------------------------------------------------------

/// Manages a queue of sync jobs with configurable concurrency limits.
pub struct SyncManager {
    /// Completed and failed jobs (for history).
    history: Vec<SyncJob>,
    /// Jobs in the queue (status == Queued).
    queue: Vec<SyncJob>,
    /// Jobs currently running, keyed by job ID.
    active: HashMap<String, SyncJob>,
    /// Default exclude patterns applied to every job.
    default_excludes: Vec<String>,
    /// Monotonic ID counter.
    next_id: u64,
    /// Maximum number of concurrent sync jobs.
    max_concurrent: usize,
}

impl SyncManager {
    /// Create a new manager with the given concurrency limit.
    pub fn new(max_concurrent: usize) -> Self {
        SyncManager {
            history: Vec::new(),
            queue: Vec::new(),
            active: HashMap::new(),
            default_excludes: vec![
                ".git".to_string(),
                "__pycache__".to_string(),
                "*.pyc".to_string(),
                "target/".to_string(),
                "node_modules/".to_string(),
            ],
            next_id: 1,
            max_concurrent,
        }
    }

    /// Add a pattern to the default exclude list.
    pub fn add_default_exclude(&mut self, pattern: &str) {
        if !self.default_excludes.contains(&pattern.to_string()) {
            self.default_excludes.push(pattern.to_string());
        }
    }

    /// Queue a push (local -> remote) job. Returns the job ID.
    pub fn queue_push(&mut self, remote: &str, local: &str, remote_path: &str) -> String {
        let id = self.allocate_id();
        let job = SyncJob {
            id: id.clone(),
            remote: remote.to_string(),
            direction: SyncDirection::Push,
            local_path: local.to_string(),
            remote_path: remote_path.to_string(),
            exclude_patterns: self.default_excludes.clone(),
            status: SyncStatus::Queued,
            started_ms: None,
            completed_ms: None,
            bytes_transferred: None,
            error: None,
        };
        self.queue.push(job);
        id
    }

    /// Queue a pull (remote -> local) job. Returns the job ID.
    pub fn queue_pull(&mut self, remote: &str, remote_path: &str, local: &str) -> String {
        let id = self.allocate_id();
        let job = SyncJob {
            id: id.clone(),
            remote: remote.to_string(),
            direction: SyncDirection::Pull,
            local_path: local.to_string(),
            remote_path: remote_path.to_string(),
            exclude_patterns: self.default_excludes.clone(),
            status: SyncStatus::Queued,
            started_ms: None,
            completed_ms: None,
            bytes_transferred: None,
            error: None,
        };
        self.queue.push(job);
        id
    }

    /// Start the next queued job if there is capacity. Returns a reference to
    /// the job that was started, or `None` if the queue is empty or
    /// concurrency is at the limit.
    pub fn start_next(&mut self, now_ms: u64) -> Option<&SyncJob> {
        if self.active.len() >= self.max_concurrent {
            return None;
        }
        if self.queue.is_empty() {
            return None;
        }

        let mut job = self.queue.remove(0);
        job.status = SyncStatus::Running;
        job.started_ms = Some(now_ms);
        let id = job.id.clone();
        self.active.insert(id.clone(), job);
        self.active.get(&id)
    }

    /// Mark a running job as completed.
    pub fn complete(&mut self, job_id: &str, bytes: u64, now_ms: u64) -> Result<(), String> {
        let mut job = self
            .active
            .remove(job_id)
            .ok_or_else(|| format!("no active job '{}'", job_id))?;
        job.status = SyncStatus::Completed;
        job.completed_ms = Some(now_ms);
        job.bytes_transferred = Some(bytes);
        self.history.push(job);
        Ok(())
    }

    /// Mark a running job as failed.
    pub fn fail(&mut self, job_id: &str, error: &str, now_ms: u64) -> Result<(), String> {
        let mut job = self
            .active
            .remove(job_id)
            .ok_or_else(|| format!("no active job '{}'", job_id))?;
        job.status = SyncStatus::Failed;
        job.completed_ms = Some(now_ms);
        job.error = Some(error.to_string());
        self.history.push(job);
        Ok(())
    }

    /// Look up a job by ID across queue, active, and history.
    pub fn status(&self, job_id: &str) -> Option<&SyncJob> {
        self.active
            .get(job_id)
            .or_else(|| self.queue.iter().find(|j| j.id == job_id))
            .or_else(|| self.history.iter().find(|j| j.id == job_id))
    }

    /// Number of jobs still waiting to start.
    pub fn pending_count(&self) -> usize {
        self.queue.len()
    }

    /// Number of currently running jobs.
    pub fn active_count(&self) -> usize {
        self.active.len()
    }

    /// Completed and failed jobs.
    pub fn history(&self) -> &[SyncJob] {
        &self.history
    }

    /// Build the rsync argument vector for a job.
    ///
    /// The resulting `Vec<String>` can be passed to `std::process::Command`
    /// with `"rsync"` as the program.
    pub fn build_rsync_args(&self, job: &SyncJob, config: &RemoteConfig) -> Vec<String> {
        let mut args: Vec<String> = Vec::new();

        // Standard flags: archive, compress, verbose, partial for resume.
        args.push("-avz".to_string());
        args.push("--partial".to_string());
        args.push("--progress".to_string());

        // SSH transport with port and optional key.
        let mut ssh_cmd = format!("ssh -p {}", config.port);
        ssh_cmd.push_str(" -o StrictHostKeyChecking=no");
        if let Some(ref key) = config.ssh_key {
            ssh_cmd.push_str(&format!(" -i {}", key));
        }
        args.push("-e".to_string());
        args.push(ssh_cmd);

        // Exclude patterns.
        for pattern in &job.exclude_patterns {
            args.push("--exclude".to_string());
            args.push(pattern.clone());
        }

        // Source and destination depend on direction.
        let remote_spec = format!(
            "{}:{}",
            config.user_at_host(),
            job.remote_path
        );

        match job.direction {
            SyncDirection::Push => {
                args.push(ensure_trailing_slash(&job.local_path));
                args.push(remote_spec);
            }
            SyncDirection::Pull => {
                args.push(remote_spec);
                args.push(ensure_trailing_slash(&job.local_path));
            }
        }

        args
    }

    /// Allocate the next monotonic job ID.
    fn allocate_id(&mut self) -> String {
        let id = format!("sync-{}", self.next_id);
        self.next_id += 1;
        id
    }
}


/// Ensure a path ends with `/` (rsync convention for syncing directory contents).
fn ensure_trailing_slash(path: &str) -> String {
    if path.ends_with('/') {
        path.to_string()
    } else {
        format!("{}/", path)
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
            ssh_key: Some("/keys/gpu.pem".to_string()),
            workspace_dir: "/data/work".to_string(),
            gpu_count: Some(4),
            labels: vec!["a100".to_string()],
        }
    }

    // -- Queue and lifecycle --

    #[test]
    fn queue_push_returns_unique_ids() {
        let mut mgr = SyncManager::new(2);
        let id1 = mgr.queue_push("r1", "/local/a", "/remote/a");
        let id2 = mgr.queue_push("r1", "/local/b", "/remote/b");
        assert_ne!(id1, id2);
        assert_eq!(mgr.pending_count(), 2);
        assert_eq!(mgr.active_count(), 0);
    }

    #[test]
    fn queue_pull_returns_unique_ids() {
        let mut mgr = SyncManager::new(2);
        let id1 = mgr.queue_pull("r1", "/remote/a", "/local/a");
        let id2 = mgr.queue_pull("r1", "/remote/b", "/local/b");
        assert_ne!(id1, id2);
    }

    #[test]
    fn start_next_moves_to_active() {
        let mut mgr = SyncManager::new(2);
        let id = mgr.queue_push("r1", "/local/a", "/remote/a");
        let started = mgr.start_next(1000).unwrap();
        assert_eq!(started.id, id);
        assert_eq!(started.status, SyncStatus::Running);
        assert_eq!(started.started_ms, Some(1000));
        assert_eq!(mgr.pending_count(), 0);
        assert_eq!(mgr.active_count(), 1);
    }

    #[test]
    fn start_next_respects_concurrency_limit() {
        let mut mgr = SyncManager::new(1);
        mgr.queue_push("r1", "/local/a", "/remote/a");
        mgr.queue_push("r1", "/local/b", "/remote/b");

        mgr.start_next(1000); // starts first job
        assert_eq!(mgr.active_count(), 1);

        // Should not start another — at limit.
        assert!(mgr.start_next(2000).is_none());
        assert_eq!(mgr.pending_count(), 1);
    }

    #[test]
    fn start_next_empty_queue_returns_none() {
        let mut mgr = SyncManager::new(2);
        assert!(mgr.start_next(1000).is_none());
    }

    #[test]
    fn complete_moves_to_history() {
        let mut mgr = SyncManager::new(2);
        let id = mgr.queue_push("r1", "/local/a", "/remote/a");
        mgr.start_next(1000);
        mgr.complete(&id, 12345, 2000).unwrap();

        assert_eq!(mgr.active_count(), 0);
        assert_eq!(mgr.history().len(), 1);
        assert_eq!(mgr.history()[0].status, SyncStatus::Completed);
        assert_eq!(mgr.history()[0].bytes_transferred, Some(12345));
        assert_eq!(mgr.history()[0].completed_ms, Some(2000));
    }

    #[test]
    fn fail_moves_to_history() {
        let mut mgr = SyncManager::new(2);
        let id = mgr.queue_push("r1", "/local/a", "/remote/a");
        mgr.start_next(1000);
        mgr.fail(&id, "permission denied", 2000).unwrap();

        assert_eq!(mgr.active_count(), 0);
        assert_eq!(mgr.history().len(), 1);
        assert_eq!(mgr.history()[0].status, SyncStatus::Failed);
        assert_eq!(
            mgr.history()[0].error,
            Some("permission denied".to_string())
        );
    }

    #[test]
    fn complete_missing_job_fails() {
        let mut mgr = SyncManager::new(2);
        assert!(mgr.complete("nope", 0, 0).is_err());
    }

    #[test]
    fn fail_missing_job_fails() {
        let mut mgr = SyncManager::new(2);
        assert!(mgr.fail("nope", "err", 0).is_err());
    }

    // -- Status lookup --

    #[test]
    fn status_finds_queued() {
        let mut mgr = SyncManager::new(2);
        let id = mgr.queue_push("r1", "/local", "/remote");
        let job = mgr.status(&id).unwrap();
        assert_eq!(job.status, SyncStatus::Queued);
    }

    #[test]
    fn status_finds_active() {
        let mut mgr = SyncManager::new(2);
        let id = mgr.queue_push("r1", "/local", "/remote");
        mgr.start_next(1000);
        let job = mgr.status(&id).unwrap();
        assert_eq!(job.status, SyncStatus::Running);
    }

    #[test]
    fn status_finds_completed() {
        let mut mgr = SyncManager::new(2);
        let id = mgr.queue_push("r1", "/local", "/remote");
        mgr.start_next(1000);
        mgr.complete(&id, 100, 2000).unwrap();
        let job = mgr.status(&id).unwrap();
        assert_eq!(job.status, SyncStatus::Completed);
    }

    #[test]
    fn status_returns_none_for_unknown() {
        let mgr = SyncManager::new(2);
        assert!(mgr.status("ghost").is_none());
    }

    // -- Default excludes --

    #[test]
    fn default_excludes_applied() {
        let mut mgr = SyncManager::new(2);
        let id = mgr.queue_push("r1", "/local", "/remote");
        let job = mgr.status(&id).unwrap();
        assert!(job.exclude_patterns.contains(&".git".to_string()));
        assert!(job.exclude_patterns.contains(&"__pycache__".to_string()));
        assert!(job.exclude_patterns.contains(&"target/".to_string()));
    }

    #[test]
    fn add_default_exclude_deduplicates() {
        let mut mgr = SyncManager::new(2);
        let before = mgr.default_excludes.len();
        mgr.add_default_exclude(".git"); // already present
        assert_eq!(mgr.default_excludes.len(), before);

        mgr.add_default_exclude(".mypy_cache");
        assert_eq!(mgr.default_excludes.len(), before + 1);
    }

    // -- rsync arg building --

    #[test]
    fn rsync_args_push() {
        let mgr = SyncManager::new(2);
        let job = SyncJob {
            id: "sync-1".to_string(),
            remote: "r1".to_string(),
            direction: SyncDirection::Push,
            local_path: "/local/project".to_string(),
            remote_path: "/remote/project".to_string(),
            exclude_patterns: vec![".git".to_string()],
            status: SyncStatus::Running,
            started_ms: Some(1000),
            completed_ms: None,
            bytes_transferred: None,
            error: None,
        };
        let config = test_config();
        let args = mgr.build_rsync_args(&job, &config);

        assert!(args.contains(&"-avz".to_string()));
        assert!(args.contains(&"--partial".to_string()));
        assert!(args.contains(&"--progress".to_string()));
        assert!(args.contains(&"--exclude".to_string()));
        assert!(args.contains(&".git".to_string()));

        // -e with ssh command
        let e_idx = args.iter().position(|a| a == "-e").unwrap();
        let ssh_cmd = &args[e_idx + 1];
        assert!(ssh_cmd.contains("ssh -p 22"));

        // Source should be local with trailing slash, dest is remote spec.
        let last = args.last().unwrap();
        assert!(last.contains("ubuntu@10.0.0.1:/remote/project"));
        let second_last = &args[args.len() - 2];
        assert!(second_last.ends_with('/'));
        assert!(second_last.contains("/local/project"));
    }

    #[test]
    fn rsync_args_pull() {
        let mgr = SyncManager::new(2);
        let job = SyncJob {
            id: "sync-2".to_string(),
            remote: "r1".to_string(),
            direction: SyncDirection::Pull,
            local_path: "/local/results".to_string(),
            remote_path: "/remote/results".to_string(),
            exclude_patterns: Vec::new(),
            status: SyncStatus::Running,
            started_ms: Some(1000),
            completed_ms: None,
            bytes_transferred: None,
            error: None,
        };
        let config = test_config();
        let args = mgr.build_rsync_args(&job, &config);

        // Source should be remote spec, dest is local with trailing slash.
        let last = args.last().unwrap();
        assert!(last.ends_with('/'));
        assert!(last.contains("/local/results"));
        let second_last = &args[args.len() - 2];
        assert!(second_last.contains("ubuntu@10.0.0.1:/remote/results"));
    }

    #[test]
    fn rsync_args_with_ssh_key() {
        let mgr = SyncManager::new(2);
        let job = SyncJob {
            id: "sync-3".to_string(),
            remote: "r1".to_string(),
            direction: SyncDirection::Push,
            local_path: "/local/a".to_string(),
            remote_path: "/remote/a".to_string(),
            exclude_patterns: Vec::new(),
            status: SyncStatus::Running,
            started_ms: Some(1000),
            completed_ms: None,
            bytes_transferred: None,
            error: None,
        };
        let config = test_config_with_key();
        let args = mgr.build_rsync_args(&job, &config);

        let e_idx = args.iter().position(|a| a == "-e").unwrap();
        let ssh_cmd = &args[e_idx + 1];
        assert!(ssh_cmd.contains("-p 2222"));
        assert!(ssh_cmd.contains("-i /keys/gpu.pem"));
    }

    #[test]
    fn rsync_args_multiple_excludes() {
        let mgr = SyncManager::new(2);
        let job = SyncJob {
            id: "sync-4".to_string(),
            remote: "r1".to_string(),
            direction: SyncDirection::Push,
            local_path: "/local".to_string(),
            remote_path: "/remote".to_string(),
            exclude_patterns: vec![
                ".git".to_string(),
                "*.pyc".to_string(),
                "target/".to_string(),
            ],
            status: SyncStatus::Running,
            started_ms: Some(1000),
            completed_ms: None,
            bytes_transferred: None,
            error: None,
        };
        let config = test_config();
        let args = mgr.build_rsync_args(&job, &config);

        // Count --exclude flags.
        let exclude_count = args.iter().filter(|a| *a == "--exclude").count();
        assert_eq!(exclude_count, 3);
    }

    // -- Trailing slash helper --

    #[test]
    fn ensure_trailing_slash_adds_when_missing() {
        assert_eq!(ensure_trailing_slash("/path/to/dir"), "/path/to/dir/");
    }

    #[test]
    fn ensure_trailing_slash_noop_when_present() {
        assert_eq!(ensure_trailing_slash("/path/to/dir/"), "/path/to/dir/");
    }

    // -- Full lifecycle sequence --

    #[test]
    fn full_lifecycle_multiple_jobs() {
        let mut mgr = SyncManager::new(2);

        let id1 = mgr.queue_push("r1", "/a", "/b");
        let id2 = mgr.queue_pull("r2", "/c", "/d");
        let id3 = mgr.queue_push("r1", "/e", "/f");
        assert_eq!(mgr.pending_count(), 3);

        // Start two (the limit).
        mgr.start_next(100).unwrap();
        mgr.start_next(100).unwrap();
        assert!(mgr.start_next(100).is_none()); // at limit
        assert_eq!(mgr.active_count(), 2);
        assert_eq!(mgr.pending_count(), 1);

        // Complete first.
        mgr.complete(&id1, 1000, 200).unwrap();
        assert_eq!(mgr.active_count(), 1);

        // Now we can start the third.
        let started = mgr.start_next(300).unwrap();
        assert_eq!(started.id, id3);
        assert_eq!(mgr.active_count(), 2);
        assert_eq!(mgr.pending_count(), 0);

        // Fail second, complete third.
        mgr.fail(&id2, "disk full", 400).unwrap();
        mgr.complete(&id3, 500, 500).unwrap();

        assert_eq!(mgr.active_count(), 0);
        assert_eq!(mgr.history().len(), 3);
    }
}
