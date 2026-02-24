//! Monitoring subsystem — heartbeat parsing, health assessment, and cycle orchestration.
//!
//! The `heartbeat` module extracts agent state from raw tmux pane captures.
//! The `health` module combines multiple signals into per-agent health
//! assessments and classifies failure modes.
//! The `cycle` module orchestrates one monitoring pass: capture → parse →
//! assess → deliver messages → check timeouts.

pub mod cycle;
pub mod health;
pub mod heartbeat;
