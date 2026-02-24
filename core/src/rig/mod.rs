//! Remote GPU/worker management — the "rig" subsystem.
//!
//! This module provides everything CMX needs to manage remote compute resources:
//! host configuration, SSH connectivity tracking, file synchronisation via rsync,
//! remote command execution, and worker lifecycle management.
//!
//! The design mirrors the patterns established by the `exp` bash tool (~1800 lines)
//! but expressed as typed Rust structs with no process-spawning side effects — all
//! external commands are built as argument vectors and returned for the caller to
//! execute.

pub mod config;
pub mod connection;
pub mod orchestrator;
pub mod remote;
pub mod sync;
pub mod worker;
