//! Agent：上下文构建与 ReAct 循环。仅依赖 trait，不依赖 platform/channels。
//! Agent: context build and ReAct loop; trait-only, no platform.

mod context;
mod deliberation;
mod delivery;
mod final_reply;
mod r#loop;
mod reply_surface;
mod request_plan;
mod request_semantics;
mod strategy;
mod subject_state;
mod tool_outcome;

pub use context::{
    build_context, ContextParams, DEFAULT_MESSAGES_MAX_LEN, DEFAULT_SYSTEM_MAX_LEN,
    SESSION_RECENT_N,
};
pub use delivery::StreamEditor;
pub use r#loop::{run_agent_loop, AgentLoopConfig, TypingNotifier};
pub use strategy::AgentRunStrategy;
