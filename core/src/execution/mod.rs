//! Execution engine — manages task execution lifecycle.
//!
//! This module handles scheduling, output capture, multi-step pipelines, and
//! execution environments. It does NOT spawn processes — it builds command
//! structures and tracks execution state. Actual process spawning is handled
//! by the infrastructure layer.

pub mod engine;
pub mod output;
pub mod timeline;
pub mod sandbox;
pub mod pipeline;
pub mod scheduler;
