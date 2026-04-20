//! Live tmux backend — executes tmux commands via shell for production use.

use std::process::Command;

use ob_utils::response::Action;
use super::SessionBackend;
use super::tmux::TmuxCommandBuilder;


/// Production backend that executes tmux commands via `sh -c`.
pub struct LiveTmuxBackend {
    builder: TmuxCommandBuilder,
}

impl LiveTmuxBackend {
    pub fn new() -> Self {
        LiveTmuxBackend {
            builder: TmuxCommandBuilder::new(),
        }
    }

    /// Execute a shell command and return stdout.
    fn shell_run(&self, cmd: &str) -> Result<String, String> {
        let output = Command::new("sh")
            .arg("-c")
            .arg(cmd)
            .output()
            .map_err(|e| format!("Failed to execute: {}", e))?;
        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).to_string())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            Err(if stderr.is_empty() {
                format!("Command failed with status {}", output.status)
            } else {
                stderr
            })
        }
    }
}

impl Default for LiveTmuxBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionBackend for LiveTmuxBackend {
    fn execute_action(&mut self, action: &Action) -> Result<(), String> {
        let cmd = match action {
            Action::CreateSession { name, cwd } => {
                self.builder.new_session(name, cwd)
            }
            Action::KillSession { name } => {
                self.builder.kill_session(name)
            }
            Action::SplitPane { session, direction, percent } => {
                self.builder.split_pane(session, direction, *percent)
            }
            Action::SendKeys { target, keys } => {
                self.builder.send_keys(target, keys)
            }
            Action::ConnectSsh { agent, host, port } => {
                let ssh_cmd = format!("ssh -p {} {}", port, host);
                self.builder.send_keys(agent, &ssh_cmd)
            }
            // Logical-only actions — no tmux command needed
            Action::CreateAgent { .. }
            | Action::KillAgent { .. }
            | Action::PlaceAgent { .. }
            | Action::UpdateAssignment { .. } => {
                return Ok(());
            }
            Action::ParkPane { .. } => {
                return Err("ParkPane is not supported by LiveTmuxBackend (MuxUX-only action)".into());
            }
        };
        self.shell_run(&cmd).map(|_| ())
    }

    fn session_exists(&self, name: &str) -> bool {
        self.shell_run(&format!("tmux has-session -t {}", shell_escape(name)))
            .is_ok()
    }

    fn list_sessions(&self) -> Vec<String> {
        match self.shell_run("tmux list-sessions -F '#{session_name}'") {
            Ok(output) => output.lines().map(|s| s.to_string()).collect(),
            Err(_) => Vec::new(),
        }
    }

    fn capture_pane(&self, target: &str) -> Result<String, String> {
        self.shell_run(&format!("tmux capture-pane -t {} -p", shell_escape(target)))
    }
}


/// Escape a string for safe use in shell commands.
fn shell_escape(s: &str) -> String {
    if s.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '.' || c == '%' || c == ':') {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_escape_simple() {
        assert_eq!(shell_escape("Demo1"), "Demo1");
        assert_eq!(shell_escape("%105"), "%105");
        assert_eq!(shell_escape("skd-w1"), "skd-w1");
    }

    #[test]
    fn shell_escape_special() {
        assert_eq!(shell_escape("has space"), "'has space'");
    }

    #[test]
    fn live_backend_implements_session_backend() {
        let backend = LiveTmuxBackend::new();
        let _: &dyn SessionBackend = &backend;
    }
}
