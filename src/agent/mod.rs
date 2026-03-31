//! Agent：上下文构建与 ReAct 循环。仅依赖 trait，不依赖 platform/channels。
//! Agent: context build and ReAct loop; trait-only, no platform.

mod context;
mod r#loop;
mod parse_tools;
mod request_plan;
mod strategy;

pub use context::{
    build_context, ContextParams, DEFAULT_MESSAGES_MAX_LEN, DEFAULT_SYSTEM_MAX_LEN,
    SESSION_RECENT_N,
};
pub use r#loop::{
    run_system_agent_loop, run_user_agent_loop, AgentLoopConfig, StreamEditor, TypingNotifier,
};
pub use strategy::AgentRunStrategy;
