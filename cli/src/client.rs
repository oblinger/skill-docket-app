#![allow(dead_code)]
//! DaemonClient — socket communication with the Skill Docket daemon.

use std::path::Path;

use skill_docket_core::command::Command;
use cmx_utils::response::Response;


/// Send a command to the Skill Docket daemon via Unix socket.
pub fn send_command(config_dir: &Path, cmd: &Command, timeout_ms: u64) -> Result<Response, String> {
    let sock_path = config_dir.join("skd.sock");
    cmx_utils::client::send_and_receive(&sock_path, cmd, timeout_ms)
}
