//! Command runner abstraction for executing shell commands.
//!
//! `CommandRunner` is the trait that backends use to execute system commands.
//! `ShellRunner` is the production implementation that spawns `sh -c`.
//! `MockRunner` is the test double that records calls and returns preset responses.

use std::cell::RefCell;
use std::process::Command;

/// Trait for executing shell command strings.
pub trait CommandRunner: Send {
    fn run(&self, cmd: &str) -> Result<String, String>;
}

/// Production runner that spawns `sh -c <cmd>`.
pub struct ShellRunner;

impl CommandRunner for ShellRunner {
    fn run(&self, cmd: &str) -> Result<String, String> {
        let output = Command::new("sh")
            .arg("-c")
            .arg(cmd)
            .output()
            .map_err(|e| format!("Failed to execute: {}", e))?;
        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).to_string())
        } else {
            Err(String::from_utf8_lossy(&output.stderr).to_string())
        }
    }
}

/// Test-double runner that records commands and returns pre-configured responses.
pub struct MockRunner {
    responses: RefCell<Vec<Result<String, String>>>,
    commands: RefCell<Vec<String>>,
}

unsafe impl Send for MockRunner {}

impl MockRunner {
    pub fn with_responses(responses: Vec<Result<String, String>>) -> Self {
        let mut reversed = responses;
        reversed.reverse();
        MockRunner {
            responses: RefCell::new(reversed),
            commands: RefCell::new(Vec::new()),
        }
    }

    pub fn new() -> Self {
        MockRunner {
            responses: RefCell::new(Vec::new()),
            commands: RefCell::new(Vec::new()),
        }
    }

    pub fn executed_commands(&self) -> Vec<String> {
        self.commands.borrow().clone()
    }
}

impl Default for MockRunner {
    fn default() -> Self {
        Self::new()
    }
}

impl CommandRunner for MockRunner {
    fn run(&self, cmd: &str) -> Result<String, String> {
        self.commands.borrow_mut().push(cmd.to_string());
        let mut responses = self.responses.borrow_mut();
        if let Some(response) = responses.pop() {
            response
        } else {
            Ok(String::new())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mock_runner_records_commands() {
        let runner = MockRunner::with_responses(vec![Ok("ok".into()), Ok("ok2".into())]);
        let r1 = runner.run("echo hello");
        assert!(r1.is_ok());
        let r2 = runner.run("echo world");
        assert!(r2.is_ok());
        let cmds = runner.executed_commands();
        assert_eq!(cmds.len(), 2);
        assert_eq!(cmds[0], "echo hello");
        assert_eq!(cmds[1], "echo world");
    }

    #[test]
    fn mock_runner_returns_responses_in_order() {
        let runner = MockRunner::with_responses(vec![
            Ok("first".into()),
            Err("fail".into()),
            Ok("third".into()),
        ]);
        assert_eq!(runner.run("cmd1").unwrap(), "first");
        assert_eq!(runner.run("cmd2").unwrap_err(), "fail");
        assert_eq!(runner.run("cmd3").unwrap(), "third");
    }

    #[test]
    fn mock_runner_defaults_to_empty_ok() {
        let runner = MockRunner::new();
        let result = runner.run("anything");
        assert_eq!(result.unwrap(), "");
    }

    #[test]
    fn mock_runner_propagates_errors() {
        let runner = MockRunner::with_responses(vec![Err("tmux: session not found".into())]);
        let result = runner.run("tmux kill-session -t nope");
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "tmux: session not found");
    }
}
