//! Namespace and parameter store — unified state model for all CMX system state.
//!
//! Provides dotted-path addressing (e.g. `task.AUTH1.status`), a typed
//! in-memory store backed by `serde_json::Value`, batch flush with dirty
//! tracking, and per-agent state persistence.

pub mod path;
pub mod store;
pub mod flush;
pub mod agent_state;

pub use path::{NamespacePath, Namespace, PathSegment, resolve_namespace};
pub use store::{ParameterStore, StoreValue, GetResult};
pub use flush::FlushManager;
pub use agent_state::AgentStateManager;
