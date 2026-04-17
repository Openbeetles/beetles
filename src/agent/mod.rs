//! Agent：上下文构建与 ReAct 循环。仅依赖 trait，不依赖 platform/channels。
//! Agent: context build and ReAct loop; trait-only, no platform.

mod active_work;
mod context;
mod deliberation;
mod delivery;
mod final_reply;
mod r#loop;
mod reasoning_intent;
mod reply_surface;
mod request_plan;
mod request_semantics;
mod soul_feedback;
mod strategy;
mod subject_state;
mod tool_outcome;

pub use active_work::{
    classify_background_job_disposition, idle_self_runtime_scheduler_block_reason_with_live_state,
    live_foreground_state_for_chat, ActiveWorkKind, ActiveWorkRecord, ActiveWorkStore,
    BackgroundDisposition, DetachedJobKind, DetachedWorkKey, DetachedWorkRecord, DetachedWorkState,
    DetachedWorkStore, DetachedWorkUpsertOutcome, DetachedWorkWake, LiveForegroundState,
    REL_PATH_ACTIVE_WORKS, REL_PATH_DETACHED_WORKS,
};
pub(crate) use active_work::{
    current_unix_ms, due_detached_work_records, load_active_work_for_chat,
    sync_active_work_after_turn, upsert_detached_work_job, ActiveWorkSyncInput,
};
pub use context::{
    build_context, ContextParams, DEFAULT_MESSAGES_MAX_LEN, DEFAULT_SYSTEM_MAX_LEN,
    SESSION_RECENT_N,
};
pub use delivery::StreamEditor;
pub use r#loop::{run_agent_loop, AgentLoopConfig, TypingNotifier};
pub use strategy::AgentRunStrategy;
