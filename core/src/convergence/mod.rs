//! Convergence engine — diffs desired vs actual state and produces actions.
//!
//! The `planner` module computes the minimal set of actions to move the system
//! from its current state to the desired state. The `retry` module provides
//! configurable retry policies with backoff for failed actions.

pub mod executor;
pub mod planner;
pub mod retry;
