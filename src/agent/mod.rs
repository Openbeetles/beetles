//! Agent：上下文构建与 ReAct 循环。仅依赖 trait，不依赖 platform/channels。
//! Agent: context build and ReAct loop; trait-only, no platform.

mod active_work;
mod context;
mod deliberation;
mod delivery;
mod final_reply;
mod r#loop;
mod reply_surface;
mod request_plan;
mod request_semantics;
mod soul_feedback;
mod strategy;
mod subject_state;
mod tool_outcome;

pub(crate) use active_work::{
    load_active_work_for_chat, sync_active_work_after_turn, ActiveWorkSyncInput,
};
pub use active_work::{ActiveWorkKind, ActiveWorkRecord, ActiveWorkStore, REL_PATH_ACTIVE_WORKS};
pub use context::{
    build_context, ContextParams, DEFAULT_MESSAGES_MAX_LEN, DEFAULT_SYSTEM_MAX_LEN,
    SESSION_RECENT_N,
};
pub use delivery::StreamEditor;
pub use r#loop::{run_agent_loop, AgentLoopConfig, TypingNotifier};
pub use strategy::AgentRunStrategy;
