pub mod bridge;
pub mod conversation_log;
pub mod copilot_sync;
pub mod lifecycle;
pub mod messenger;
pub mod pool;
pub mod spawner;
pub mod state;
pub mod watcher;

pub use conversation_log::{AgentLogTracker, ConversationLogger, LogConfig, LogError};
pub use copilot_sync::{
    ContextUpdate, CopilotConfig, CopilotSyncManager, CopilotTracker, SyncError,
};
