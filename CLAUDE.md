# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What This Is

Skill Docket App (SKD) — the task orchestration daemon for ClaudiMux. Monitors concurrent AI agent projects, detects failures/stalls, manages agent lifecycle. Does NOT replace AI agents — it orchestrates them.

## Build & Test

```bash
cargo test                              # All tests (~1,763 tests, ~0.5s)
cargo test -p skill-docket-core         # Core crate only
cargo test -p skd-cli                   # CLI crate only
cargo test -p skd-tui                   # TUI crate only
cargo test --lib hw_scenario            # Hollow World E2E scenarios only
cargo test name_of_test                 # Single test by name
cargo check                             # Fast compile check (no tests)
cargo build --release -p skd-cli        # Release binary
```

Pre-commit gate: `cargo test --lib -p skill-docket-core -- --quiet`

Install binary: `just install` (symlinks `target/release/skd` → `~/bin/skd`)

Run daemon: `cargo run --release -p skd-cli -- daemon run`

## Workspace Layout

Three crates in a Cargo workspace:

- **core/** (`skill-docket-core`) — All daemon logic. This is where nearly all development happens.
- **cli/** (`skd-cli`) — Thin CLI binary. Parses args, connects to daemon socket, falls back to local execution.
- **tui/** (`skd-tui`) — Terminal UI built on ratatui/crossterm.
- **python/** — Python client library (cmx module).

External path dependencies (sibling repos):
- `skill-docket` (../../skill-docket) — open-source docket parsing
- `cmx-utils` (../../cmx-utils) — shared socket protocol, response types, logging

## Architecture

### Central Dispatch

All operations route through `Sys::execute(cmd: Command) -> Response` in `core/src/sys.rs`. The `Sys` struct owns all mutable state: `Data` (persistent store), `Settings` (runtime overrides), `PoolManager`, `Library`, and optionally a `RigOrchestrator`.

`Command` is a flat enum with 49+ variants (`core/src/command.rs`), parsed from CLI args by `core/src/cli/parse.rs`.

### Daemon vs Local Mode

- **Daemon mode** (`daemon run`): `Daemon` in `core/src/daemon.rs` runs an event loop — accepts socket connections, runs monitor cycles, executes convergence. State persists across commands.
- **Local mode**: CLI falls back to creating a fresh `Sys` per command when daemon is unavailable. Good for one-shot queries, no monitoring.

### Socket Protocol

Unix domain socket at `~/.config/skill-docket/cmx.sock`. Length-prefixed JSON. `ServiceSocket` (core/src/service.rs) listens; `client.rs` sends. Watch registry supports long-poll subscribers.

### Two-Component Pattern (SKD + PM Agent)

1. **SKD daemon** — heartbeat checks, stall detection, state model, tmux session management. Never makes judgment calls.
2. **PM Agent** — AI decision-maker (external Claude Code instance). Interprets status signals, decides retry/redesign/escalate, issues commands through SKD.

Agents NEVER touch infrastructure directly. All commands go through SKD.

### Core Modules (core/src/)

| Module | Purpose |
|--------|---------|
| `sys.rs` | Central dispatch — `Sys::execute()` |
| `command.rs` | Command enum (49+ variants) |
| `daemon.rs` | Event loop, socket accept, monitor scheduling |
| `service.rs` | Unix socket listener + watch registry |
| `data/` | Persistent state: agents, tasks, folders, settings, roadmap, config |
| `types/` | Domain types: Agent, Task, Config, Health, Message, Session |
| `agent/` | Agent lifecycle, pool management, bridge, briefing, watcher |
| `monitor/` | Heartbeat parsing, health assessment, monitor cycle |
| `execution/` | Task execution engine, pipelines, scheduler, sandbox |
| `diagnosis/` | Self-diagnosis: events, signal reliability, adaptive thresholds |
| `infrastructure/` | Session backends (tmux, mock), shell runner |
| `rig/` | Remote worker orchestration (SSH + rsync) |
| `convergence/` | Layout convergence executor, retry policies |
| `history/` | Config history snapshots, retention, browsing |
| `library/` | Skill library registry, sources, conflict resolution |
| `namespace/` | Parameter store with path-based access |
| `rules/` | Rule engine, expression language |
| `skill/` | Project-local skill parsing |
| `snapshot/` | System state snapshots, checkpoints, journaling |

### Config Directory

`~/.config/skill-docket/` (override with `SKD_CONFIG_DIR` env var):
- `settings.yaml` — global settings
- `folders.yaml` — registered project folders
- `cmx.sock` — Unix socket (runtime)
- `skd.pid` — PID file (runtime)
- `logs/events.jsonl` — diagnosis events

### Testing Patterns

- Unit tests: `#[cfg(test)] mod tests` blocks throughout modules
- Hollow World: E2E scenario in `tests/hollow-world/` simulating a multi-agent project lifecycle with 5 task folders
- `MockBackend` (`infrastructure/mock.rs`) for testing without tmux side effects
- All tests run in-process with no external dependencies

## Key Documentation

- `SKD Docs/SKD User/SKD Pilot Role.md` — defines the `next` command protocol. **Re-read after every /compact.**
- `SKD Docs/SKD Dev/` — module deep-dives (Core, Agent, Config, Health, Execution)
- `SKD Docs/SKD Plan/Specs/` — active implementation specs
- `SKD Docs/SKD Plan/SKD Backlog.md` — backlog
- `SESSION-START.md` — current session handoff state (if present)
