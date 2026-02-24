//! Socket client for daemon communication with queuing and reconnection.
//!
//! The [`MuxClient`] connects to the CMX daemon over a Unix socket and
//! sends [`Command`] values, receiving [`Response`] values back. It handles
//! reconnection on transient failures and provides convenience methods for
//! common queries.
//!
//! The [`CommandBatch`] struct allows sending multiple commands in sequence
//! and inspecting results as a group.

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;

use skill_docket_core::command::Command;
use cmx_utils::response::Response;


/// A client that communicates with the CMX daemon over a Unix socket.
pub struct MuxClient {
    socket_path: PathBuf,
    connected: bool,
    stream: Option<UnixStream>,
    reconnect_attempts: u32,
    max_reconnects: u32,
    last_response: Option<Response>,
}


impl MuxClient {
    /// Create a new client targeting the given socket path.
    /// Does not connect immediately; call [`connect`] first.
    pub fn new(socket_path: PathBuf) -> Self {
        MuxClient {
            socket_path,
            connected: false,
            stream: None,
            reconnect_attempts: 0,
            max_reconnects: 5,
            last_response: None,
        }
    }

    /// Attempt to connect to the daemon socket.
    pub fn connect(&mut self) -> Result<(), String> {
        match UnixStream::connect(&self.socket_path) {
            Ok(stream) => {
                stream
                    .set_read_timeout(Some(Duration::from_secs(10)))
                    .map_err(|e| format!("Failed to set read timeout: {}", e))?;
                stream
                    .set_write_timeout(Some(Duration::from_secs(5)))
                    .map_err(|e| format!("Failed to set write timeout: {}", e))?;
                self.stream = Some(stream);
                self.connected = true;
                self.reconnect_attempts = 0;
                Ok(())
            }
            Err(e) => {
                self.connected = false;
                Err(format!(
                    "Failed to connect to {}: {}",
                    self.socket_path.display(),
                    e
                ))
            }
        }
    }

    /// Send a command and wait for the response.
    ///
    /// Uses length-prefixed framing (4-byte BE length + JSON payload),
    /// matching the daemon's `service.rs` protocol.
    pub fn send(&mut self, cmd: &Command) -> Result<Response, String> {
        if !self.connected {
            return Err("Not connected".to_string());
        }

        let json =
            serde_json::to_vec(cmd).map_err(|e| format!("Serialize error: {}", e))?;

        let stream = self
            .stream
            .as_mut()
            .ok_or_else(|| "No stream available".to_string())?;

        // Write length-prefixed frame
        let len = json.len() as u32;
        stream.write_all(&len.to_be_bytes()).map_err(|e| {
            self.connected = false;
            format!("Write error: {}", e)
        })?;
        stream.write_all(&json).map_err(|e| {
            self.connected = false;
            format!("Write error: {}", e)
        })?;
        stream.flush().map_err(|e| {
            self.connected = false;
            format!("Flush error: {}", e)
        })?;

        // Read length-prefixed response
        let mut len_buf = [0u8; 4];
        stream.read_exact(&mut len_buf).map_err(|e| {
            self.connected = false;
            format!("Read error: {}", e)
        })?;
        let resp_len = u32::from_be_bytes(len_buf) as usize;

        if resp_len == 0 {
            return Err("Empty response frame".into());
        }
        if resp_len > 16 * 1024 * 1024 {
            return Err(format!("Response frame too large: {} bytes", resp_len));
        }

        let mut payload = vec![0u8; resp_len];
        stream.read_exact(&mut payload).map_err(|e| {
            self.connected = false;
            format!("Read error: {}", e)
        })?;

        let response: Response = serde_json::from_slice(&payload)
            .map_err(|e| format!("Deserialize error: {}", e))?;

        self.last_response = Some(response.clone());
        Ok(response)
    }

    /// Return whether the client believes it is connected.
    pub fn is_connected(&self) -> bool {
        self.connected
    }

    /// Attempt to reconnect to the daemon.
    ///
    /// Increments the reconnect counter and fails if the maximum
    /// number of attempts has been reached.
    pub fn reconnect(&mut self) -> Result<(), String> {
        if self.reconnect_attempts >= self.max_reconnects {
            return Err(format!(
                "Max reconnect attempts ({}) exceeded",
                self.max_reconnects
            ));
        }
        self.reconnect_attempts += 1;
        self.stream = None;
        self.connected = false;
        self.connect()
    }

    /// Send a `status` command and return the output string.
    pub fn status(&mut self) -> Result<String, String> {
        let resp = self.send(&Command::Status { format: None })?;
        match resp {
            Response::Ok { output } => Ok(output),
            Response::Error { message } => Err(message),
        }
    }

    /// Send an `agent.list` command with JSON format and return the raw JSON.
    pub fn agent_list_json(&mut self) -> Result<String, String> {
        let cmd = Command::AgentList {
            format: Some("json".to_string()),
        };
        let resp = self.send(&cmd)?;
        match resp {
            Response::Ok { output } => Ok(output),
            Response::Error { message } => Err(message),
        }
    }

    /// Send a `task.list` command with JSON format and return the raw JSON.
    pub fn task_list_json(&mut self) -> Result<String, String> {
        let cmd = Command::TaskList {
            format: Some("json".to_string()),
            project: None,
        };
        let resp = self.send(&cmd)?;
        match resp {
            Response::Ok { output } => Ok(output),
            Response::Error { message } => Err(message),
        }
    }

    /// Send a `project.list` command with JSON format and return the raw JSON.
    pub fn project_list_json(&mut self) -> Result<String, String> {
        let cmd = Command::ProjectList {
            format: Some("json".to_string()),
        };
        let resp = self.send(&cmd)?;
        match resp {
            Response::Ok { output } => Ok(output),
            Response::Error { message } => Err(message),
        }
    }

    /// Send a `help` command, optionally for a specific topic.
    pub fn help(&mut self, topic: Option<&str>) -> Result<String, String> {
        let cmd = Command::Help {
            topic: topic.map(|s| s.to_string()),
        };
        let resp = self.send(&cmd)?;
        match resp {
            Response::Ok { output } => Ok(output),
            Response::Error { message } => Err(message),
        }
    }

    /// Return the last response received, if any.
    pub fn last_response(&self) -> Option<&Response> {
        self.last_response.as_ref()
    }

    /// Return the socket path this client is configured to connect to.
    pub fn socket_path(&self) -> &PathBuf {
        &self.socket_path
    }
}


// ---------------------------------------------------------------------------
// CommandBatch
// ---------------------------------------------------------------------------

/// A batch of commands to be sent sequentially to a [`MuxClient`].
///
/// Useful for operations that require multiple daemon calls. Results
/// are stored and can be inspected after execution.
pub struct CommandBatch {
    commands: Vec<Command>,
    results: Vec<Result<Response, String>>,
}


impl CommandBatch {
    /// Create a new empty batch.
    pub fn new() -> Self {
        CommandBatch {
            commands: Vec::new(),
            results: Vec::new(),
        }
    }

    /// Add a command to the batch.
    pub fn add(&mut self, cmd: Command) {
        self.commands.push(cmd);
    }

    /// Execute all commands in sequence using the given client.
    /// Returns a slice of the results.
    pub fn execute(&mut self, client: &mut MuxClient) -> &[Result<Response, String>] {
        self.results.clear();
        for cmd in &self.commands {
            self.results.push(client.send(cmd));
        }
        &self.results
    }

    /// Return true if all commands succeeded (returned `Response::Ok`).
    pub fn all_ok(&self) -> bool {
        self.results.iter().all(|r| match r {
            Ok(Response::Ok { .. }) => true,
            _ => false,
        })
    }

    /// Return error messages from failed commands.
    pub fn errors(&self) -> Vec<String> {
        self.results
            .iter()
            .filter_map(|r| match r {
                Ok(Response::Error { message }) => Some(message.clone()),
                Err(e) => Some(e.clone()),
                _ => None,
            })
            .collect()
    }

    /// Return the number of commands in the batch.
    pub fn len(&self) -> usize {
        self.commands.len()
    }

    /// Return whether the batch is empty.
    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }

    /// Return the number of results (after execution).
    pub fn result_count(&self) -> usize {
        self.results.len()
    }
}


impl Default for CommandBatch {
    fn default() -> Self {
        Self::new()
    }
}


// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // We cannot connect to a real socket in unit tests, but we can test
    // the client's state management and batch logic.

    #[test]
    fn client_new_not_connected() {
        let client = MuxClient::new(PathBuf::from("/tmp/cmx-test.sock"));
        assert!(!client.is_connected());
        assert!(client.last_response().is_none());
    }

    #[test]
    fn client_socket_path() {
        let path = PathBuf::from("/tmp/cmx.sock");
        let client = MuxClient::new(path.clone());
        assert_eq!(client.socket_path(), &path);
    }

    #[test]
    fn client_send_when_not_connected() {
        let mut client = MuxClient::new(PathBuf::from("/tmp/nonexistent.sock"));
        let result = client.send(&Command::Status { format: None });
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Not connected");
    }

    #[test]
    fn client_connect_to_nonexistent_socket() {
        let mut client = MuxClient::new(PathBuf::from("/tmp/definitely-not-a-socket-12345.sock"));
        let result = client.connect();
        assert!(result.is_err());
        assert!(!client.is_connected());
    }

    #[test]
    fn client_reconnect_limit() {
        let mut client = MuxClient::new(PathBuf::from("/tmp/nonexistent.sock"));
        client.max_reconnects = 2;

        // First two attempts should fail but not hit the limit error
        let r1 = client.reconnect();
        assert!(r1.is_err());
        assert!(r1.unwrap_err().contains("Failed to connect"));

        let r2 = client.reconnect();
        assert!(r2.is_err());
        assert!(r2.unwrap_err().contains("Failed to connect"));

        // Third attempt should hit the limit
        let r3 = client.reconnect();
        assert!(r3.is_err());
        assert!(r3.unwrap_err().contains("Max reconnect attempts"));
    }

    #[test]
    fn client_status_not_connected() {
        let mut client = MuxClient::new(PathBuf::from("/tmp/no.sock"));
        let result = client.status();
        assert!(result.is_err());
    }

    #[test]
    fn client_agent_list_not_connected() {
        let mut client = MuxClient::new(PathBuf::from("/tmp/no.sock"));
        let result = client.agent_list_json();
        assert!(result.is_err());
    }

    #[test]
    fn client_task_list_not_connected() {
        let mut client = MuxClient::new(PathBuf::from("/tmp/no.sock"));
        let result = client.task_list_json();
        assert!(result.is_err());
    }

    #[test]
    fn client_project_list_not_connected() {
        let mut client = MuxClient::new(PathBuf::from("/tmp/no.sock"));
        let result = client.project_list_json();
        assert!(result.is_err());
    }

    #[test]
    fn client_help_not_connected() {
        let mut client = MuxClient::new(PathBuf::from("/tmp/no.sock"));
        let result = client.help(None);
        assert!(result.is_err());
        let result2 = client.help(Some("agent"));
        assert!(result2.is_err());
    }

    // --- CommandBatch ---

    #[test]
    fn batch_new_is_empty() {
        let batch = CommandBatch::new();
        assert!(batch.is_empty());
        assert_eq!(batch.len(), 0);
        assert_eq!(batch.result_count(), 0);
    }

    #[test]
    fn batch_add_commands() {
        let mut batch = CommandBatch::new();
        batch.add(Command::Status { format: None });
        batch.add(Command::AgentList { format: None });
        assert_eq!(batch.len(), 2);
        assert!(!batch.is_empty());
    }

    #[test]
    fn batch_execute_not_connected() {
        let mut client = MuxClient::new(PathBuf::from("/tmp/no.sock"));
        let mut batch = CommandBatch::new();
        batch.add(Command::Status { format: None });
        batch.add(Command::AgentList { format: None });

        let results = batch.execute(&mut client);
        assert_eq!(results.len(), 2);
        // All should be errors since client is not connected
        for r in results {
            assert!(r.is_err());
        }
        assert!(!batch.all_ok());
    }

    #[test]
    fn batch_errors_not_connected() {
        let mut client = MuxClient::new(PathBuf::from("/tmp/no.sock"));
        let mut batch = CommandBatch::new();
        batch.add(Command::Status { format: None });
        batch.execute(&mut client);

        let errors = batch.errors();
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("Not connected"));
    }

    #[test]
    fn batch_all_ok_empty() {
        let batch = CommandBatch::new();
        // No results means vacuously true
        assert!(batch.all_ok());
    }

    #[test]
    fn batch_default() {
        let batch = CommandBatch::default();
        assert!(batch.is_empty());
    }

    // --- Simulated response tests ---

    #[test]
    fn response_ok_output() {
        let resp = Response::Ok {
            output: "3 agents, 5 tasks".into(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        let back: Response = serde_json::from_str(&json).unwrap();
        assert_eq!(back, resp);
    }

    #[test]
    fn response_error_message() {
        let resp = Response::Error {
            message: "agent not found".into(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        let back: Response = serde_json::from_str(&json).unwrap();
        assert_eq!(back, resp);
    }

    #[test]
    fn command_serialization_for_send() {
        // Verify the commands we build serialize correctly
        let cmd = Command::AgentList {
            format: Some("json".into()),
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("\"command\":\"agent.list\""));
        assert!(json.contains("\"format\":\"json\""));
    }

    #[test]
    fn command_status_serialization() {
        let cmd = Command::Status { format: None };
        let json = serde_json::to_string(&cmd).unwrap();
        assert_eq!(json, "{\"command\":\"status\"}");
    }

    #[test]
    fn command_help_serialization() {
        let cmd = Command::Help {
            topic: Some("agent".into()),
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("\"command\":\"help\""));
        assert!(json.contains("\"topic\":\"agent\""));
    }

    #[test]
    fn command_help_no_topic_serialization() {
        let cmd = Command::Help { topic: None };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("\"command\":\"help\""));
        assert!(!json.contains("topic"));
    }

    #[test]
    fn batch_result_count_after_execute() {
        let mut client = MuxClient::new(PathBuf::from("/tmp/no.sock"));
        let mut batch = CommandBatch::new();
        batch.add(Command::Status { format: None });
        batch.add(Command::Status { format: None });
        batch.add(Command::Status { format: None });
        batch.execute(&mut client);
        assert_eq!(batch.result_count(), 3);
    }

    #[test]
    fn batch_errors_include_transport_and_response_errors() {
        // We can only test transport errors here since we have no daemon
        let mut client = MuxClient::new(PathBuf::from("/tmp/no.sock"));
        let mut batch = CommandBatch::new();
        batch.add(Command::Status { format: None });
        batch.execute(&mut client);

        let errors = batch.errors();
        assert!(!errors.is_empty());
    }
}
