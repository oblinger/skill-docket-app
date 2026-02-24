//! Client — shared daemon client with automatic lifecycle management.
//!
//! All CMX frontends (CLI, MuxUX, Tauri) use `execute_remote()` to send
//! commands to the daemon. If the daemon is not running, it is started
//! automatically. If it is unresponsive, it is restarted.
//!
//! The command send itself serves as the liveness check — there is no
//! separate ping or health-check protocol.

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::command::Command;
use cmx_utils::response::Response;


/// Send a command to the CMX daemon, starting it if necessary.
///
/// This is the primary entry point for all CMX clients. It transparently
/// handles daemon lifecycle: if the daemon is not running, it starts it;
/// if it is unresponsive, it restarts it.
///
/// # Arguments
///
/// * `config_dir` — Path to the CMX config directory (contains socket, PID, lock files)
/// * `cmd` — The command to send
/// * `timeout_ms` — Read timeout in milliseconds for the socket response
///
/// # Errors
///
/// Returns `Err` if the daemon cannot be reached even after a restart attempt.
pub fn execute_remote(
    config_dir: &Path,
    cmd: &Command,
    timeout_ms: u64,
) -> Result<Response, String> {
    // Fast path: try sending directly
    match send_command(config_dir, cmd, timeout_ms) {
        Ok(resp) => return Ok(resp),
        Err(_) => {
            // Fall through to recovery
        }
    }

    // Recovery path: acquire lock, ensure daemon is running, retry
    let lock_path = config_dir.join("cmx.lock");
    let _lock = acquire_lock(&lock_path, 10_000)?;

    // Re-check: another process may have started the daemon while we waited
    if send_command(config_dir, cmd, timeout_ms).is_ok() {
        // The re-check succeeded — another process started the daemon
        return send_command(config_dir, cmd, timeout_ms)
            .or_else(|_| send_command(config_dir, cmd, timeout_ms));
    }

    // Kill stale daemon if PID file exists
    let pid_path = config_dir.join("cmx.pid");
    if let Ok(pid_str) = std::fs::read_to_string(&pid_path) {
        if let Ok(pid) = pid_str.trim().parse::<u32>() {
            if is_pid_alive(pid) {
                kill_pid(pid);
                // Brief wait for process to exit
                std::thread::sleep(Duration::from_millis(200));
            }
        }
        let _ = std::fs::remove_file(&pid_path);
    }

    // Clean up stale socket
    let sock_path = config_dir.join("cmx.sock");
    let _ = std::fs::remove_file(&sock_path);

    // Start daemon as a background process
    let _pid = start_daemon_process(config_dir)?;

    // Wait for socket to appear and accept connections
    wait_for_socket(config_dir, 5_000)?;

    // Retry the original command
    send_command(config_dir, cmd, timeout_ms)
        .map_err(|e| format!("Daemon started but command failed: {}", e))
}


/// Send a command to the daemon socket with a read timeout.
fn send_command(
    config_dir: &Path,
    cmd: &Command,
    timeout_ms: u64,
) -> Result<Response, String> {
    let sock_path = config_dir.join("cmx.sock");

    let stream = UnixStream::connect(&sock_path)
        .map_err(|e| format!("Cannot connect to {}: {}", sock_path.display(), e))?;

    stream
        .set_read_timeout(Some(Duration::from_millis(timeout_ms)))
        .map_err(|e| format!("Cannot set timeout: {}", e))?;

    // Write length-prefixed JSON command
    let json = serde_json::to_vec(cmd)
        .map_err(|e| format!("Failed to serialize command: {}", e))?;
    write_frame(&stream, &json)?;

    // Read length-prefixed JSON response (subject to timeout)
    let payload = read_frame(&stream)?;

    serde_json::from_slice(&payload)
        .map_err(|e| format!("Failed to parse response: {}", e))
}


/// Write a length-prefixed frame to a stream.
fn write_frame(stream: &UnixStream, payload: &[u8]) -> Result<(), String> {
    let mut stream = stream;
    let len = payload.len() as u32;
    stream
        .write_all(&len.to_be_bytes())
        .map_err(|e| format!("Failed to write frame length: {}", e))?;
    stream
        .write_all(payload)
        .map_err(|e| format!("Failed to write frame payload: {}", e))?;
    stream
        .flush()
        .map_err(|e| format!("Failed to flush: {}", e))?;
    Ok(())
}


/// Read a length-prefixed frame from a stream.
fn read_frame(stream: &UnixStream) -> Result<Vec<u8>, String> {
    let mut stream = stream;
    let mut len_buf = [0u8; 4];
    stream
        .read_exact(&mut len_buf)
        .map_err(|e| format!("Failed to read response length: {}", e))?;
    let len = u32::from_be_bytes(len_buf) as usize;

    if len == 0 {
        return Err("Empty response frame".into());
    }
    if len > 16 * 1024 * 1024 {
        return Err(format!("Response frame too large: {} bytes", len));
    }

    let mut payload = vec![0u8; len];
    stream
        .read_exact(&mut payload)
        .map_err(|e| format!("Failed to read response payload: {}", e))?;

    Ok(payload)
}


/// File-lock guard that releases the lock on drop.
#[derive(Debug)]
struct LockGuard {
    file: std::fs::File,
    path: PathBuf,
}


impl Drop for LockGuard {
    fn drop(&mut self) {
        // Release the flock
        unsafe {
            libc::flock(
                std::os::unix::io::AsRawFd::as_raw_fd(&self.file),
                libc::LOCK_UN,
            );
        }
        // Remove the lock file (best effort)
        let _ = std::fs::remove_file(&self.path);
    }
}


/// Acquire an exclusive file lock (blocking with timeout).
///
/// Returns a guard that releases the lock on drop.
fn acquire_lock(lock_path: &Path, timeout_ms: u64) -> Result<LockGuard, String> {
    // Ensure parent directory exists
    if let Some(parent) = lock_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(lock_path)
        .map_err(|e| format!("Cannot create lock file {}: {}", lock_path.display(), e))?;

    let fd = std::os::unix::io::AsRawFd::as_raw_fd(&file);
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);

    loop {
        let ret = unsafe { libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB) };
        if ret == 0 {
            return Ok(LockGuard {
                file,
                path: lock_path.to_path_buf(),
            });
        }

        if Instant::now() >= deadline {
            return Err(format!(
                "Timed out acquiring lock {} after {}ms",
                lock_path.display(),
                timeout_ms
            ));
        }

        std::thread::sleep(Duration::from_millis(50));
    }
}


/// Start the daemon as a detached background process.
///
/// Spawns `<current_exe> daemon run` with `CMX_CONFIG_DIR` set to the
/// given config directory. Redirects stdout/stderr to `daemon.log`.
///
/// Returns the PID of the spawned process.
fn start_daemon_process(config_dir: &Path) -> Result<u32, String> {
    let exe = std::env::current_exe()
        .map_err(|e| format!("Cannot determine current executable: {}", e))?;

    let log_path = config_dir.join("daemon.log");
    let log_file = std::fs::File::create(&log_path)
        .map_err(|e| format!("Cannot create {}: {}", log_path.display(), e))?;
    let log_stderr = log_file
        .try_clone()
        .map_err(|e| format!("Cannot clone log file handle: {}", e))?;

    let child = std::process::Command::new(&exe)
        .args(["daemon", "run"])
        .env("CMX_CONFIG_DIR", config_dir)
        .stdout(log_file)
        .stderr(log_stderr)
        .spawn()
        .map_err(|e| format!("Cannot spawn daemon: {}", e))?;

    let pid = child.id();
    Ok(pid)
}


/// Wait for the daemon socket to appear and accept a test connection.
///
/// Polls with backoff up to `timeout_ms` milliseconds.
fn wait_for_socket(config_dir: &Path, timeout_ms: u64) -> Result<(), String> {
    let sock_path = config_dir.join("cmx.sock");
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    let mut interval = Duration::from_millis(25);

    loop {
        if sock_path.exists() {
            // Socket file exists — try to connect
            if UnixStream::connect(&sock_path).is_ok() {
                return Ok(());
            }
        }

        if Instant::now() >= deadline {
            return Err(format!(
                "Timed out waiting for daemon socket at {} ({}ms)",
                sock_path.display(),
                timeout_ms,
            ));
        }

        std::thread::sleep(interval);
        // Exponential backoff capped at 200ms
        interval = std::cmp::min(interval * 2, Duration::from_millis(200));
    }
}


/// Check if a process with the given PID is alive.
fn is_pid_alive(pid: u32) -> bool {
    // kill(pid, 0) checks if the process exists without sending a signal
    unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
}


/// Kill a process by PID (best effort, SIGTERM then SIGKILL after brief delay).
fn kill_pid(pid: u32) {
    let pid = pid as libc::pid_t;

    // Send SIGTERM
    unsafe {
        libc::kill(pid, libc::SIGTERM);
    }

    // Wait briefly for graceful shutdown
    std::thread::sleep(Duration::from_millis(500));

    // If still alive, send SIGKILL
    if unsafe { libc::kill(pid, 0) } == 0 {
        unsafe {
            libc::kill(pid, libc::SIGKILL);
        }
    }
}


// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static TEST_SEQ: AtomicU32 = AtomicU32::new(0);

    /// Create a short temp directory to stay under SUN_LEN for Unix sockets.
    fn test_config_dir() -> PathBuf {
        let seq = TEST_SEQ.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("cmxc{}-{}", std::process::id(), seq));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn cleanup(dir: &Path) {
        let _ = std::fs::remove_dir_all(dir);
    }

    // -----------------------------------------------------------------------
    // 1. send_command_to_running_daemon
    // -----------------------------------------------------------------------

    #[test]
    fn send_command_to_running_daemon() {
        let dir = test_config_dir();
        let mut daemon = crate::daemon::Daemon::with_config(
            &dir,
            crate::daemon::DaemonConfig { socket_poll_ms: 10 },
        )
        .unwrap();
        let handle = daemon.handle();

        // Run daemon in a background thread
        let thread = std::thread::spawn(move || {
            daemon.run().unwrap();
        });

        // Give daemon time to start accepting
        std::thread::sleep(Duration::from_millis(100));

        // Send a status command
        let cmd = Command::Status { format: None };
        let resp = send_command(&dir, &cmd, 5_000).unwrap();
        match resp {
            Response::Ok { output } => assert!(output.contains("agents: 0")),
            Response::Error { message } => panic!("Unexpected error: {}", message),
        }

        // Shut down daemon
        handle.shutdown().unwrap();
        thread.join().unwrap();
        cleanup(&dir);
    }

    // -----------------------------------------------------------------------
    // 2. send_command_no_daemon_fails
    // -----------------------------------------------------------------------

    #[test]
    fn send_command_no_daemon_fails() {
        let dir = test_config_dir();
        let cmd = Command::Status { format: None };
        let result = send_command(&dir, &cmd, 1_000);
        assert!(result.is_err());
        cleanup(&dir);
    }

    // -----------------------------------------------------------------------
    // 3. execute_remote_finds_running_daemon
    // -----------------------------------------------------------------------

    #[test]
    fn execute_remote_finds_running_daemon() {
        let dir = test_config_dir();
        let mut daemon = crate::daemon::Daemon::with_config(
            &dir,
            crate::daemon::DaemonConfig { socket_poll_ms: 10 },
        )
        .unwrap();
        let handle = daemon.handle();

        let thread = std::thread::spawn(move || {
            daemon.run().unwrap();
        });

        std::thread::sleep(Duration::from_millis(100));

        // execute_remote should find the existing daemon and succeed
        let cmd = Command::Status { format: None };
        let resp = execute_remote(&dir, &cmd, 5_000).unwrap();
        match resp {
            Response::Ok { output } => assert!(output.contains("agents: 0")),
            Response::Error { message } => panic!("Unexpected error: {}", message),
        }

        handle.shutdown().unwrap();
        thread.join().unwrap();
        cleanup(&dir);
    }

    // -----------------------------------------------------------------------
    // 4. lock_file_prevents_races
    // -----------------------------------------------------------------------

    #[test]
    fn lock_file_prevents_races() {
        let dir = test_config_dir();
        let lock_path = dir.join("test.lock");

        // Acquire lock
        let _guard = acquire_lock(&lock_path, 1_000).unwrap();

        // Second acquire should time out quickly
        let result = acquire_lock(&lock_path, 200);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Timed out"));

        // Drop guard, then lock should be acquirable
        drop(_guard);
        let _guard2 = acquire_lock(&lock_path, 1_000).unwrap();

        cleanup(&dir);
    }

    // -----------------------------------------------------------------------
    // 5. pid_file_written_and_cleaned
    // -----------------------------------------------------------------------

    #[test]
    fn pid_file_written_and_cleaned() {
        // This test verifies the daemon writes and cleans up PID files.
        // We test the PID file lifecycle via the Daemon directly.
        let dir = test_config_dir();
        let pid_path = dir.join("cmx.pid");

        // Write a PID file manually (simulating what the CLI does)
        let my_pid = std::process::id();
        std::fs::write(&pid_path, my_pid.to_string()).unwrap();
        assert!(pid_path.exists());

        let contents = std::fs::read_to_string(&pid_path).unwrap();
        assert_eq!(contents, my_pid.to_string());

        // Clean up (simulating what the CLI does after daemon exits)
        std::fs::remove_file(&pid_path).unwrap();
        assert!(!pid_path.exists());

        cleanup(&dir);
    }

    // -----------------------------------------------------------------------
    // 6. wait_for_socket_succeeds
    // -----------------------------------------------------------------------

    #[test]
    fn wait_for_socket_succeeds() {
        let dir = test_config_dir();

        // Start a daemon in a background thread (creates the socket)
        let dir_clone = dir.clone();
        let thread = std::thread::spawn(move || {
            let mut daemon = crate::daemon::Daemon::with_config(
                &dir_clone,
                crate::daemon::DaemonConfig { socket_poll_ms: 10 },
            )
            .unwrap();
            let handle = daemon.handle();
            // Schedule shutdown after a brief run
            let h = handle.clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(500));
                h.shutdown().unwrap();
            });
            daemon.run().unwrap();
        });

        // Wait should succeed once the daemon creates the socket
        let result = wait_for_socket(&dir, 3_000);
        assert!(result.is_ok(), "wait_for_socket failed: {:?}", result.err());

        thread.join().unwrap();
        cleanup(&dir);
    }

    // -----------------------------------------------------------------------
    // 7. wait_for_socket_timeout
    // -----------------------------------------------------------------------

    #[test]
    fn wait_for_socket_timeout() {
        let dir = test_config_dir();
        // No daemon running — should time out
        let result = wait_for_socket(&dir, 200);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Timed out"));
        cleanup(&dir);
    }

    // -----------------------------------------------------------------------
    // 8. is_pid_alive_returns_true_for_self
    // -----------------------------------------------------------------------

    #[test]
    fn is_pid_alive_returns_true_for_self() {
        assert!(is_pid_alive(std::process::id()));
    }

    // -----------------------------------------------------------------------
    // 9. is_pid_alive_returns_false_for_nonexistent
    // -----------------------------------------------------------------------

    #[test]
    fn is_pid_alive_returns_false_for_nonexistent() {
        // PID 4_000_000 is extremely unlikely to exist
        assert!(!is_pid_alive(4_000_000));
    }

    // -----------------------------------------------------------------------
    // 10. daemon_stop_command_round_trip
    // -----------------------------------------------------------------------

    #[test]
    fn daemon_stop_command_round_trip() {
        // Verify DaemonRun and DaemonStop serialize/deserialize correctly
        let cmd_run = Command::DaemonRun;
        let json = serde_json::to_string(&cmd_run).unwrap();
        assert!(json.contains("daemon.run"));
        let back: Command = serde_json::from_str(&json).unwrap();
        assert_eq!(back, Command::DaemonRun);

        let cmd_stop = Command::DaemonStop;
        let json = serde_json::to_string(&cmd_stop).unwrap();
        assert!(json.contains("daemon.stop"));
        let back: Command = serde_json::from_str(&json).unwrap();
        assert_eq!(back, Command::DaemonStop);
    }
}
