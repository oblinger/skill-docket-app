//! SKD CLI — the command-line entry point for Skill Docket.
//!
//! # Usage
//!
//! ```text
//! skd status
//! skd agent new worker --name w1
//! skd task list
//! skd daemon run
//! skd daemon stop
//! ```

mod client;

use std::path::{Path, PathBuf};
use std::process;

use skill_docket_core::cli::parse_args;
use skill_docket_core::command::Command;
use skill_docket_core::sys::Sys;
use cmx_utils::response::Response;


fn main() {
    let args: Vec<String> = std::env::args().collect();
    let arg_refs: Vec<&str> = args[1..].iter().map(|s| s.as_str()).collect();

    let cmd = match parse_args(&arg_refs) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("skd: {}", e);
            process::exit(1);
        }
    };

    let config_dir = resolve_config_dir();

    // Tui is handled directly — launch the terminal UI.
    if matches!(cmd, Command::Tui) {
        let socket_path = config_dir.join("cmx.sock");
        match skd_tui::tui::Tui::new(Some(socket_path.to_string_lossy().into_owned())) {
            Ok(mut tui) => {
                if let Err(e) = tui.run() {
                    eprintln!("skd tui: {}", e);
                    process::exit(1);
                }
            }
            Err(e) => {
                eprintln!("skd tui: failed to start: {}", e);
                process::exit(1);
            }
        }
        return;
    }

    // DaemonRun is handled directly — run the daemon in this process.
    if matches!(cmd, Command::DaemonRun) {
        let pid_path = config_dir.join("skd.pid");
        let _ = std::fs::write(&pid_path, std::process::id().to_string());

        match skill_docket_core::daemon::Daemon::new(&config_dir) {
            Ok(mut daemon) => {
                if let Err(e) = daemon.run() {
                    eprintln!("skd daemon: {}", e);
                    let _ = std::fs::remove_file(&pid_path);
                    process::exit(1);
                }
                let _ = std::fs::remove_file(&pid_path);
            }
            Err(e) => {
                eprintln!("skd daemon: failed to start: {}", e);
                let _ = std::fs::remove_file(&pid_path);
                process::exit(1);
            }
        }
        return;
    }

    // All other commands: use execute_remote (handles daemon lifecycle).
    let response = match skill_docket_core::client::execute_remote(&config_dir, &cmd, 10_000) {
        Ok(resp) => resp,
        Err(e) => {
            eprintln!("skd: daemon unavailable ({}), using local mode", e);
            execute_local(&config_dir, cmd)
        }
    };

    match response {
        Response::Ok { output } => {
            if !output.is_empty() {
                println!("{}", output);
            }
        }
        Response::Error { message } => {
            eprintln!("skd error: {}", message);
            process::exit(1);
        }
    }
}


fn resolve_config_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("SKD_CONFIG_DIR") {
        return PathBuf::from(dir);
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    PathBuf::from(home).join(".config").join("skill-docket")
}


fn execute_local(config_dir: &Path, cmd: Command) -> Response {
    match Sys::new(config_dir) {
        Ok(mut sys) => sys.execute(cmd),
        Err(e) => Response::Error {
            message: format!("Failed to initialize: {}", e),
        },
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_config_dir_default() {
        let old = std::env::var("SKD_CONFIG_DIR").ok();
        std::env::remove_var("SKD_CONFIG_DIR");
        let dir = resolve_config_dir();
        assert!(dir.to_string_lossy().contains(".config/skill-docket"));
        if let Some(v) = old {
            std::env::set_var("SKD_CONFIG_DIR", v);
        }
    }

    #[test]
    fn resolve_config_dir_from_env() {
        std::env::set_var("SKD_CONFIG_DIR", "/tmp/test-skd-config");
        let dir = resolve_config_dir();
        assert_eq!(dir, PathBuf::from("/tmp/test-skd-config"));
        std::env::remove_var("SKD_CONFIG_DIR");
    }

    #[test]
    fn execute_local_status() {
        let dir = std::env::temp_dir().join("skd-cli-test-local");
        let _ = std::fs::create_dir_all(&dir);
        let cmd = Command::Status { format: None };
        let resp = execute_local(&dir, cmd);
        match resp {
            Response::Ok { output } => assert!(output.contains("agents: 0")),
            Response::Error { message } => panic!("Unexpected error: {}", message),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
