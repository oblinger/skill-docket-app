use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::command::Command;
use crate::sys::Sys;
use cmx_utils::response::Response;
use cmx_utils::watch::WatchRegistry;


/// Unix domain socket listener that accepts one connection at a time,
/// reads a length-prefixed JSON command, dispatches it through Sys, and
/// writes back a length-prefixed JSON response.
///
/// Watch commands are intercepted at this layer and routed to a
/// `WatchRegistry` instead of being dispatched through Sys.
pub struct ServiceSocket {
    listener: UnixListener,
    path: PathBuf,
    shutdown_requested: std::cell::Cell<bool>,
}


/// Result of handling a single connection.
enum HandleResult {
    /// A regular command was dispatched through Sys.
    Dispatched { summary: String },
    /// A Watch command was received — the stream was moved to the registry.
    Registered,
    /// A DaemonStop command was received — the response was sent, daemon should shut down.
    Shutdown,
}


impl ServiceSocket {
    /// Bind a new Unix domain socket at the given path.
    /// Removes any stale socket file first.
    pub fn bind(path: &Path) -> Result<ServiceSocket, String> {
        // Clean up stale socket
        if path.exists() {
            std::fs::remove_file(path)
                .map_err(|e| format!("Cannot remove stale socket {}: {}", path.display(), e))?;
        }
        let listener = UnixListener::bind(path)
            .map_err(|e| format!("Cannot bind socket {}: {}", path.display(), e))?;
        Ok(ServiceSocket {
            listener,
            path: path.to_path_buf(),
            shutdown_requested: std::cell::Cell::new(false),
        })
    }

    /// Start the service: bind socket, return ready ServiceSocket.
    /// Called once during daemon initialization.
    pub fn start(config_dir: &Path) -> Result<ServiceSocket, String> {
        let sock_path = config_dir.join("cmx.sock");
        ServiceSocket::bind(&sock_path)
    }

    /// Accept a single connection, read one command, dispatch through Sys,
    /// and send back the response. Blocks until a client connects.
    ///
    /// Watch commands are intercepted and registered in the `WatchRegistry`
    /// instead of being dispatched. After a regular command is dispatched,
    /// all registered watchers are notified of the state change.
    /// Returns `Ok(true)` if a DaemonStop was received and the daemon should shut down.
    pub fn accept_one(&self, sys: &mut Sys, registry: &mut WatchRegistry) -> Result<bool, String> {
        let (stream, _addr) = self
            .listener
            .accept()
            .map_err(|e| format!("Accept failed: {}", e))?;
        match handle_connection(stream, sys, registry)? {
            HandleResult::Dispatched { summary } => {
                let now_ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64;
                registry.notify_all(&summary, now_ms);
            }
            HandleResult::Registered => {
                // Stream moved to registry, nothing more to do.
            }
            HandleResult::Shutdown => {
                self.shutdown_requested.set(true);
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Accept connections in a loop until shutdown is signaled.
    /// Uses non-blocking accept with a poll timeout so the caller
    /// can interleave other work (convergence, monitoring).
    ///
    /// Returns `Ok(true)` if a command was handled, `Ok(false)` if
    /// the timeout elapsed with no incoming connection.
    ///
    /// Also expires stale watchers on each poll iteration.
    pub fn accept_nonblocking(
        &self,
        sys: &mut Sys,
        registry: &mut WatchRegistry,
        timeout_ms: u64,
    ) -> Result<bool, String> {
        self.listener
            .set_nonblocking(true)
            .map_err(|e| format!("Failed to set non-blocking: {}", e))?;

        let deadline = Instant::now() + Duration::from_millis(timeout_ms);
        let poll_interval = Duration::from_millis(10);

        let result = loop {
            match self.listener.accept() {
                Ok((stream, _addr)) => {
                    let _ = self.listener.set_nonblocking(false);
                    match handle_connection(stream, sys, registry)? {
                        HandleResult::Dispatched { summary } => {
                            let now_ms = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_millis() as u64;
                            registry.notify_all(&summary, now_ms);
                        }
                        HandleResult::Registered => {}
                        HandleResult::Shutdown => {
                            let _ = self.listener.set_nonblocking(false);
                            self.shutdown_requested.set(true);
                            return Ok(true);
                        }
                    }
                    break Ok(true);
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    registry.expire_stale();
                    if Instant::now() >= deadline {
                        break Ok(false);
                    }
                    std::thread::sleep(poll_interval);
                }
                Err(e) => {
                    let _ = self.listener.set_nonblocking(false);
                    break Err(format!("Accept failed: {}", e));
                }
            }
        };

        // Always restore blocking mode
        let _ = self.listener.set_nonblocking(false);
        result
    }

    /// Shutdown: cleanup socket file, drop listener.
    pub fn shutdown(self) {
        let _ = std::fs::remove_file(&self.path);
        // listener dropped automatically
    }

    /// Cleanup socket file without consuming self.
    pub fn shutdown_ref(&self) {
        let _ = std::fs::remove_file(&self.path);
    }

    /// Return the path this socket is bound to.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns true if a DaemonStop command has been received.
    pub fn shutdown_requested(&self) -> bool {
        self.shutdown_requested.get()
    }

    /// Remove the socket file from disk (static helper).
    pub fn cleanup(path: &Path) {
        let _ = std::fs::remove_file(path);
    }
}


/// Handle a single connection: read command, dispatch or register.
///
/// If the command is `Watch`, the stream is moved into the registry and
/// `HandleResult::Registered` is returned. Otherwise, the command is
/// dispatched through Sys and the response is written back.
fn handle_connection(
    mut stream: UnixStream,
    sys: &mut Sys,
    registry: &mut WatchRegistry,
) -> Result<HandleResult, String> {
    let cmd = read_frame(&mut stream)?;

    match cmd {
        Command::Watch { since, timeout } => {
            let since_ms = since.and_then(|s| s.parse::<u64>().ok());
            let timeout_ms = timeout
                .and_then(|t| t.parse::<u64>().ok())
                .unwrap_or(30_000);
            registry.register(stream, since_ms, timeout_ms);
            Ok(HandleResult::Registered)
        }
        Command::DaemonStop => {
            let response = sys.execute(cmd);
            write_frame(&mut stream, &response)?;
            Ok(HandleResult::Shutdown)
        }
        _ => {
            let summary = format!("{:?}", cmd);
            // Truncate the debug summary to a reasonable length.
            let summary = if summary.len() > 200 {
                format!("{}...", &summary[..200])
            } else {
                summary
            };
            let response = sys.execute(cmd);
            write_frame(&mut stream, &response)?;
            Ok(HandleResult::Dispatched { summary })
        }
    }
}


/// Read a length-prefixed JSON frame from a stream.
///
/// Wire format: 4 bytes big-endian length, then that many bytes of JSON.
fn read_frame(stream: &mut UnixStream) -> Result<Command, String> {
    // Read 4-byte length prefix
    let mut len_buf = [0u8; 4];
    stream
        .read_exact(&mut len_buf)
        .map_err(|e| format!("Failed to read frame length: {}", e))?;
    let len = u32::from_be_bytes(len_buf) as usize;

    if len == 0 {
        return Err("Empty frame".into());
    }
    if len > 16 * 1024 * 1024 {
        return Err(format!("Frame too large: {} bytes", len));
    }

    // Read payload
    let mut payload = vec![0u8; len];
    stream
        .read_exact(&mut payload)
        .map_err(|e| format!("Failed to read frame payload: {}", e))?;

    // Parse JSON
    serde_json::from_slice(&payload)
        .map_err(|e| format!("Failed to parse command JSON: {}", e))
}


/// Write a length-prefixed JSON frame to a stream.
fn write_frame(stream: &mut UnixStream, response: &Response) -> Result<(), String> {
    let json = serde_json::to_vec(response)
        .map_err(|e| format!("Failed to serialize response: {}", e))?;
    let len = json.len() as u32;
    stream
        .write_all(&len.to_be_bytes())
        .map_err(|e| format!("Failed to write frame length: {}", e))?;
    stream
        .write_all(&json)
        .map_err(|e| format!("Failed to write frame payload: {}", e))?;
    Ok(())
}


// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::Data;
    use std::os::unix::net::UnixStream;

    /// Create a paired (client, server) UnixStream for testing without
    /// needing a filesystem socket.
    fn paired_streams() -> (UnixStream, UnixStream) {
        UnixStream::pair().expect("Failed to create UnixStream pair")
    }

    fn test_sys() -> Sys {
        let data = Data::new(std::path::Path::new("/tmp/cmx-test-nonexistent-svc")).unwrap();
        Sys::from_data(data)
    }

    fn write_cmd_to_stream(stream: &mut UnixStream, cmd: &Command) {
        let json = serde_json::to_vec(cmd).unwrap();
        let len = json.len() as u32;
        stream.write_all(&len.to_be_bytes()).unwrap();
        stream.write_all(&json).unwrap();
    }

    fn read_response_from_stream(stream: &mut UnixStream) -> Response {
        let mut len_buf = [0u8; 4];
        stream.read_exact(&mut len_buf).unwrap();
        let len = u32::from_be_bytes(len_buf) as usize;
        let mut payload = vec![0u8; len];
        stream.read_exact(&mut payload).unwrap();
        serde_json::from_slice(&payload).unwrap()
    }

    #[test]
    fn frame_round_trip() {
        let (mut client, mut server) = paired_streams();
        let cmd = Command::Status { format: None };

        // Write from client side
        write_cmd_to_stream(&mut client, &cmd);

        // Read from server side
        let received = read_frame(&mut server).unwrap();
        assert_eq!(received, Command::Status { format: None });
    }

    #[test]
    fn response_write_and_read() {
        let (mut reader, mut writer) = paired_streams();
        let response = Response::Ok {
            output: "hello".into(),
        };
        write_frame(&mut writer, &response).unwrap();

        let received = read_response_from_stream(&mut reader);
        assert_eq!(
            received,
            Response::Ok {
                output: "hello".into()
            }
        );
    }

    #[test]
    fn full_dispatch_via_streams() {
        let (mut client, server) = paired_streams();

        // Write a status command
        let cmd = Command::Status { format: None };
        write_cmd_to_stream(&mut client, &cmd);

        // Handle on server side
        let mut sys = test_sys();
        let mut registry = WatchRegistry::new();
        let result = handle_connection(server, &mut sys, &mut registry).unwrap();
        assert!(matches!(result, HandleResult::Dispatched { .. }));

        // Read response on client side
        let resp = read_response_from_stream(&mut client);
        match resp {
            Response::Ok { output } => assert!(output.contains("agents: 0")),
            Response::Error { message } => panic!("Unexpected error: {}", message),
        }
    }

    #[test]
    fn dispatch_unknown_command_rejected_at_parse() {
        let (mut client, mut server) = paired_streams();

        // Write raw JSON for a bogus command that the enum cannot deserialize
        let bogus_json = br#"{"command":"bogus.command"}"#;
        let len = bogus_json.len() as u32;
        client.write_all(&len.to_be_bytes()).unwrap();
        client.write_all(bogus_json).unwrap();

        // read_frame should fail because "bogus.command" is not a valid variant
        let result = read_frame(&mut server);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Failed to parse command JSON"));
    }

    #[test]
    fn bind_and_cleanup() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!("cmx-test-socket-{}.sock", std::process::id()));
        ServiceSocket::cleanup(&path);

        let _sock = ServiceSocket::bind(&path).unwrap();
        assert!(path.exists());

        ServiceSocket::cleanup(&path);
        assert!(!path.exists());
    }

    #[test]
    fn empty_frame_rejected() {
        let (mut client, mut server) = paired_streams();

        // Write a zero-length frame
        client.write_all(&0u32.to_be_bytes()).unwrap();

        let result = read_frame(&mut server);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Empty frame"));
    }

    #[test]
    fn agent_creation_via_socket() {
        let (mut client, server) = paired_streams();

        let cmd = Command::AgentNew {
            role: "worker".into(),
            name: Some("w1".into()),
            path: None,
            agent_type: None,
        };
        write_cmd_to_stream(&mut client, &cmd);

        let mut sys = test_sys();
        let mut registry = WatchRegistry::new();
        handle_connection(server, &mut sys, &mut registry).unwrap();

        let resp = read_response_from_stream(&mut client);
        match resp {
            Response::Ok { output } => assert!(output.contains("w1")),
            Response::Error { message } => panic!("Unexpected error: {}", message),
        }
        assert_eq!(sys.data().agents().list().len(), 1);
    }

    #[test]
    fn watch_command_registers_watcher() {
        let (mut client, server) = paired_streams();

        // Send a Watch command.
        let cmd = Command::Watch {
            since: None,
            timeout: Some("5000".into()),
        };
        write_cmd_to_stream(&mut client, &cmd);

        let mut sys = test_sys();
        let mut registry = WatchRegistry::new();
        let result = handle_connection(server, &mut sys, &mut registry).unwrap();

        // The connection should be registered, not dispatched.
        assert!(matches!(result, HandleResult::Registered));
        assert_eq!(registry.watcher_count(), 1);

        // The client stream is still open — no response yet.
        client
            .set_read_timeout(Some(std::time::Duration::from_millis(50)))
            .unwrap();
        let mut buf = [0u8; 1];
        let read_result = client.read(&mut buf);
        // Should get WouldBlock or TimedOut since no data has been sent.
        assert!(read_result.is_err() || read_result.unwrap() == 0);
    }

    #[test]
    fn regular_command_triggers_notification() {
        // Step 1: Register a watcher.
        let (mut watcher_client, watcher_server) = paired_streams();
        watcher_client
            .set_read_timeout(Some(std::time::Duration::from_secs(2)))
            .unwrap();

        let mut sys = test_sys();
        let mut registry = WatchRegistry::new();

        let watch_cmd = Command::Watch {
            since: None,
            timeout: Some("30000".into()),
        };
        write_cmd_to_stream(&mut watcher_client.try_clone().unwrap(), &watch_cmd);
        // Handle the watch — should register, not dispatch.
        handle_connection(watcher_server, &mut sys, &mut registry).unwrap();
        assert_eq!(registry.watcher_count(), 1);

        // Step 2: Send a regular command on a separate connection.
        let (mut cmd_client, cmd_server) = paired_streams();
        let cmd = Command::AgentNew {
            role: "worker".into(),
            name: Some("w1".into()),
            path: None,
            agent_type: None,
        };
        write_cmd_to_stream(&mut cmd_client, &cmd);

        let result = handle_connection(cmd_server, &mut sys, &mut registry).unwrap();
        // Manually trigger notification (normally done by accept_one).
        if let HandleResult::Dispatched { summary } = result {
            registry.notify_all(&summary, 1708700000000);
        }

        // The command client should get its normal response.
        let cmd_resp = read_response_from_stream(&mut cmd_client);
        match cmd_resp {
            Response::Ok { output } => assert!(output.contains("w1")),
            Response::Error { message } => panic!("Unexpected error: {}", message),
        }

        // The watcher should have been notified.
        assert_eq!(registry.watcher_count(), 0);
        let watcher_resp = read_response_from_stream(&mut watcher_client);
        match watcher_resp {
            Response::Ok { output } => {
                assert!(output.contains("state_changed"));
                assert!(output.contains("AgentNew"));
            }
            Response::Error { message } => panic!("Unexpected error: {}", message),
        }
    }
}
