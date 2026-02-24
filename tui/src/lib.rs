//! MuxUX — Terminal UI rendering for ClaudiMux.
//!
//! This crate provides string-based rendering utilities for displaying CMX
//! system state in a terminal. It does not perform any I/O directly; all
//! output is rendered to `String` values that callers can print or pipe
//! into tmux panes.
//!
//! # Modules
//!
//! - [`client`] — Socket client for daemon communication
//! - [`completion`] — Tab completion for CMX commands
//! - [`input`] — Command line editing and history
//! - [`render`] — ANSI formatting, tables, panels, progress bars
//! - [`status`] — Status display formatting for agents, tasks, projects
//! - [`theme`] — Color theme configuration

pub mod agent_view;
pub mod app;
pub mod client;
pub mod completion;
pub mod dashboard;
pub mod input;
pub mod keybindings;
pub mod notification;
pub mod render;
pub mod search;
pub mod status;
pub mod theme;
pub mod tui;
pub mod views;
