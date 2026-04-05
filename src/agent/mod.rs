//! Agent：上下文构建与 ReAct 循环。仅依赖 trait，不依赖 platform/channels。
//! Agent: context build and ReAct loop; trait-only, no platform.

mod context;
mod delivery;
mod final_reply;
mod r#loop;
mod request_plan;
mod strategy;
mod tool_guidance;
mod tool_outcome;

pub use context::{
    ContextParams, DEFAULT_MESSAGES_MAX_LEN, DEFAULT_SYSTEM_MAX_LEN, SESSION_RECENT_N,
    build_context,
};
pub use delivery::StreamEditor;
pub use r#loop::{AgentLoopConfig, TypingNotifier, run_agent_loop};
pub use strategy::AgentRunStrategy;
