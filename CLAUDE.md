# skill-docket-app

Proprietary Skill Docket application. Task orchestration daemon with agent lifecycle management, monitoring, diagnosis, and remote rig support.

## Structure

- core/ — daemon crate (sys dispatch, data layer, agent management, monitoring)
- cli/ — command-line client (skd binary)
- tui/ — terminal UI (ratatui-based)
- python/ — Python client library
- assets/ — default configuration

## Build

cargo test
cargo build -p skd-cli

## Dependencies

- skill-docket (open source docket parsing and triggers)
- cmx-utils (socket protocol, logging)
