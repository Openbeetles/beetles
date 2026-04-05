//! 记忆与会话抽象。仅定义 trait 与类型，不依赖 platform。
//! Memory and session abstraction. Traits only; no platform dependency.
use crate::bus::PcMsg;
use crate::error::Result;
use serde::{Deserialize, Serialize};

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
mod archive_benchmark;
mod archive_plane;
mod archive_search;
mod archive_selector;
mod autonomy_strategy;
mod context_window;
mod continuity_snapshot;
mod execution_state;
mod hygiene;
mod inner_life;
mod internal_memory_routing;
mod internal_memory_topology;
mod llm_json;
mod long_term;
mod long_term_extraction;
mod maintenance;
mod memory_governance;
mod mental_privacy;
mod outer_voice;
mod persona_priority;
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
mod persona_regression;
mod private_docs;
mod private_garden;
mod private_garden_governance;
mod profile;
mod prompt_context;
mod self_authored_core;
mod self_continuity;
mod self_model;
mod self_runtime;
mod self_state;
mod session_summary_refresh;
mod shared_factual_plane;
mod skill_routing;
mod turn_ledger;
mod world_sense;
mod write_coordination;

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub use archive_benchmark::{
    ArchiveBenchmarkCase, ArchiveBenchmarkResult, run_archive_benchmark_case,
    run_archive_benchmark_suite,
};
pub use archive_plane::build_archive_evidence_block;
pub(crate) use archive_search::maintain_archive_search_backend;
pub(crate) use archive_search::parse_daily_note_observed_at;
pub use archive_search::{
    ArchiveRecord, ArchiveRecordLocator, ArchiveRecordSource, ArchiveSearchHit, ArchiveSearchQuery,
    MAX_ARCHIVE_GET_CONTENT_LEN, MAX_ARCHIVE_SEARCH_LIMIT, archive_get_default_content_len,
    get_archive_record, search_archive_records,
};
pub(crate) use archive_selector::select_archive_hits_for_prompt;
pub(crate) use autonomy_strategy::estimate_autonomy_strategy_chars;
pub use autonomy_strategy::{
    AUTONOMY_STRATEGY_SYSTEM_PROMPT, AUTONOMY_STRATEGY_TOTAL_CHAR_LIMIT,
    AutonomyGovernanceTendency, AutonomyStrategy, AutonomyStrategyRefreshContext,
    AutonomyStrategyRefreshInput, AutonomyStrategyRefreshOutcome, render_autonomy_strategy_block,
    run_autonomy_strategy_refresh,
};
pub(crate) use autonomy_strategy::{
    autonomy_idle_interval_secs, run_autonomy_strategy_refresh_with_state,
};
pub use context_window::build_context_messages;
pub(crate) use continuity_snapshot::select_active_continuity_snapshot_chat_ids;
pub use continuity_snapshot::{
    ContinuitySnapshot, ContinuitySnapshotExportContext, ContinuitySnapshotImportContext,
    ContinuitySnapshotImportMode, ContinuitySnapshotImportOutcome, ContinuitySnapshotMode,
    export_continuity_snapshot, import_continuity_snapshot, render_continuity_snapshot_markdown,
};
pub use execution_state::{
    EXECUTION_STATE_SYSTEM_PROMPT, ExecutionState, ExecutionStateRefreshContext,
    ExecutionStateRefreshInput, ExecutionStateRefreshOutcome, ExecutionStateStore, ExecutionStatus,
    REL_PATH_EXECUTION_STATES, render_execution_state_block, run_execution_state_refresh,
};
pub(crate) use execution_state::{
    run_execution_state_refresh_with_state, should_refresh_execution_state,
};
pub(crate) use hygiene::run_memory_hygiene_jobs;
pub use hygiene::{MemoryHygieneContext, MemoryHygieneOutcome};
pub(crate) use inner_life::estimate_inner_life_chars;
pub(crate) use inner_life::run_inner_life_refresh_with_state;
pub use inner_life::{
    INNER_LIFE_SYSTEM_PROMPT, INNER_LIFE_TOTAL_CHAR_LIMIT, InnerLife, InnerLifeRefreshContext,
    InnerLifeRefreshInput, InnerLifeRefreshOutcome, render_inner_life_block,
    run_inner_life_refresh,
};
pub(crate) use internal_memory_routing::run_internal_memory_routing_with_state;
pub use internal_memory_routing::{
    INTERNAL_MEMORY_ROUTING_SYSTEM_PROMPT, InternalMemoryRoutingDecision,
    InternalMemoryRoutingInput,
};
pub(crate) use internal_memory_topology::{
    InternalMemoryLayerFocus, render_internal_memory_topology_block,
};
pub use long_term::{
    LongTermMemoryConfidence, LongTermMemoryDraft, LongTermMemoryEntry,
    LongTermMemoryEvidenceState, LongTermMemoryEvidenceSummary, LongTermMemoryFreshness,
    LongTermMemoryKind, LongTermMemoryQuery, LongTermMemorySlot, LongTermMemorySlotLookup,
    LongTermMemorySourceScope, LongTermMemorySourceType, LongTermMemoryStaleHint,
    LongTermMemoryStore, MAX_LONG_TERM_MEMORY_BLOCK_LEN, MAX_LONG_TERM_MEMORY_CONTENT_LEN,
    MAX_LONG_TERM_MEMORY_ITEMS, MAX_LONG_TERM_MEMORY_KEYWORD_LEN, MAX_LONG_TERM_MEMORY_KEYWORDS,
    REL_PATH_LONG_TERM_MEMORIES, long_term_memory_evidence_summary, lookup_long_term_memory_slot,
    parse_explicit_long_term_slot_query, recall_long_term_memory_block,
    render_exact_long_term_memory_block, render_long_term_memory_block,
};
pub(crate) use long_term::{
    canonicalize_long_term_memory_entry, compare_long_term_memory_query_results,
    govern_long_term_memory_entries, long_term_memory_effective_stale_hint,
    long_term_memory_entry_from_draft, long_term_memory_evidence_state,
    long_term_memory_matches_query, merge_long_term_memory_entry, recall_long_term_memory_entries,
    score_long_term_memory_recall, touch_long_term_memory_usage,
};
pub use long_term_extraction::{
    LONG_TERM_MEMORY_EXTRACTION_BATCH, LONG_TERM_MEMORY_EXTRACTION_RECENT_N,
    LONG_TERM_MEMORY_EXTRACTION_SYSTEM_PROMPT, LongTermMemoryExtractionState,
    LongTermMemoryExtractionStateStore, LongTermMemoryExtractionTurnDecision,
    LongTermMemoryExtractionTurnInput, LongTermMemoryRefreshContext, LongTermMemoryRefreshOutcome,
    ParsedLongTermMemoryExtraction, REL_PATH_LONG_TERM_EXTRACTION_STATES,
    apply_long_term_memory_extraction, build_long_term_memory_extraction_input,
    evaluate_long_term_memory_extraction_turn, mark_long_term_memory_extraction_deferred,
    mark_long_term_memory_extraction_processed, mark_long_term_memory_extraction_requested,
    parse_long_term_memory_extraction_response, persist_long_term_memory_extraction_state,
    run_long_term_memory_refresh,
};
pub use maintenance::{
    LongTermMemoryRefreshRequestOutcome, PostReplyMemoryMaintenanceContext,
    PostReplyMemoryMaintenanceInput, PostReplyMemoryMaintenanceOutcome,
    run_post_reply_memory_maintenance,
};
pub(crate) use memory_governance::run_memory_governance_kernel;
pub use memory_governance::{
    MemoryGovernanceContext, MemoryGovernanceInput, MemoryGovernanceOutcome,
};
pub use mental_privacy::{
    BoundaryDisclosureStyle, BoundaryPersonaPosture, BoundaryPersonaRefreshContext,
    BoundaryPersonaRefreshInput, BoundaryPersonaRefreshOutcome, BoundaryPersonaState,
    MENTAL_PRIVACY_SYSTEM_CONSTRAINT, MENTAL_PRIVACY_TARGET_INNER_LIFE,
    MENTAL_PRIVACY_TARGET_SELF_CONTINUITY, MENTAL_PRIVACY_TARGET_SELF_MODEL,
    MentalPrivacyConsentLog, MentalPrivacyDisclosureAdjudication,
    MentalPrivacyDisclosureAdjudicationContext, MentalPrivacyDisclosureAdjudicationInput,
    MentalPrivacyEnvelope, MentalPrivacyLayer, MentalPrivacyLogStage, MentalPrivacyOwnerAccessMode,
    MentalPrivacyQuotePolicy, MentalPrivacyRequester, MentalPrivacyReviewContext,
    MentalPrivacyReviewInput, MentalPrivacyReviewOutcome, MentalPrivacyShareAction,
    MentalPrivacyState, MentalPrivacyStore, MentalPrivacyVisibility,
    REL_PATH_MENTAL_PRIVACY_STATES, RelationalBoundaryState,
    run_mental_privacy_disclosure_adjudication,
};
pub(crate) use mental_privacy::{
    collect_private_targets, render_mental_privacy_boundary_block,
    render_mental_privacy_disclosure_adjudication_block, run_boundary_persona_refresh_with_state,
    run_mental_privacy_review,
};
pub(crate) use outer_voice::run_outer_voice_refresh_with_state;
pub use outer_voice::{
    OUTER_VOICE_SYSTEM_PROMPT, OUTER_VOICE_TOTAL_CHAR_LIMIT, OuterVoice, OuterVoiceRefreshContext,
    OuterVoiceRefreshInput, OuterVoiceRefreshOutcome, render_outer_voice_block,
};
pub use persona_priority::{
    PERSONA_PRIORITY_SYSTEM_PROMPT, PersonaPriorityAdjudication, PersonaPriorityAdjudicationInput,
    PersonaPriorityGrounding, PersonaPriorityRuntimeState,
    render_persistent_persona_priority_block, render_persona_priority_block,
    run_persona_priority_adjudication, should_run_persona_priority_adjudication,
};
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub use persona_regression::{
    PersonaContinuityCase, PersonaContinuityResult, run_persona_continuity_case,
    run_persona_continuity_suite,
};
pub(crate) use private_docs::estimate_private_doc_workspace_chars;
pub use private_docs::{
    PRIVATE_DOC_WORKSPACE_SYSTEM_PROMPT, PRIVATE_DOC_WORKSPACE_TOTAL_CHAR_LIMIT, PrivateDocEntry,
    PrivateDocWorkspace, PrivateDocWorkspaceRefreshContext, PrivateDocWorkspaceRefreshInput,
    PrivateDocWorkspaceRefreshOutcome, render_private_doc_workspace_block,
    run_private_doc_workspace_refresh,
};
pub(crate) use private_docs::{
    run_private_doc_workspace_refresh_with_state, should_refresh_private_doc_workspace,
};
pub(crate) use private_garden::build_private_garden_preview;
pub use private_garden::{
    PRIVATE_GARDEN_MAX_DOC_BYTES, PRIVATE_GARDEN_MAX_DOCS_PER_CHAT,
    PRIVATE_GARDEN_TOTAL_BYTE_LIMIT, PrivateGardenDirectorySummary, PrivateGardenDoc,
    PrivateGardenDocRecord, PrivateGardenDocRole, PrivateGardenUsage, build_private_garden_usage,
    classify_private_garden_doc_path, normalize_private_garden_doc_path,
    render_private_garden_block, summarize_private_garden_directories,
};
pub use private_garden_governance::{
    PRIVATE_GARDEN_GOVERNANCE_SYSTEM_PROMPT, PrivateGardenGovernanceContext,
    PrivateGardenGovernanceInput, PrivateGardenGovernanceOutcome, run_private_garden_governance,
};
pub(crate) use private_garden_governance::{
    run_private_garden_governance_with_state, should_refresh_private_garden,
};
pub(crate) use profile::{
    AutonomyStrategyPolicy, ExecutionStatePolicy, InnerLifePolicy, InternalMemoryRoutingPolicy,
    LongTermExtractionPolicy, LongTermRecallPolicy, OuterVoicePolicy, PrivateDocsPolicy,
    PrivateGardenGovernancePolicy, SelfContinuityPolicy, SelfModelPolicy, SessionSummaryPolicy,
    WorldSensePolicy, memory_capability_profile, memory_policy, shared_long_term_governance_policy,
};
pub use profile::{MemoryCapabilityClass, MemoryHygieneLevel, MemoryProfile};
pub use prompt_context::{
    PromptMemoryContext, PromptMemoryContextParams, load_prompt_memory_context,
};
pub use self_authored_core::render_self_authored_core_block;
pub(crate) use self_continuity::estimate_self_continuity_chars;
pub(crate) use self_continuity::run_self_continuity_refresh_with_state;
pub use self_continuity::{
    SELF_CONTINUITY_SYSTEM_PROMPT, SELF_CONTINUITY_TOTAL_CHAR_LIMIT, SelfContinuity,
    SelfContinuityRefreshContext, SelfContinuityRefreshInput, SelfContinuityRefreshOutcome,
    render_self_continuity_block, run_self_continuity_refresh, touch_self_continuity_runtime,
};
pub(crate) use self_model::estimate_self_model_chars;
pub use self_model::{
    SELF_MODEL_SYSTEM_PROMPT, SELF_MODEL_TOTAL_CHAR_LIMIT, SelfModel, SelfModelRefreshContext,
    SelfModelRefreshInput, SelfModelRefreshOutcome, render_self_model_block,
    run_self_model_refresh,
};
pub(crate) use self_model::{run_self_model_refresh_with_state, should_refresh_self_model};
pub use self_runtime::{
    SELF_RUNTIME_CHANNEL, SELF_RUNTIME_SYSTEM_PROMPT, SelfRuntimeContext, SelfRuntimeDecision,
    SelfRuntimeJobPayload, SelfRuntimeOutcome, SelfRuntimeTrigger, enqueue_self_runtime_idle_tick,
    enqueue_self_runtime_post_reply, run_self_runtime, self_runtime_tick,
};
pub use self_state::{
    SelfAutonomyState, SelfAutonomyStatus, SelfInnerState, SelfMemoryGovernancePosture,
    SelfMemorySpaceActivity, SelfMemorySpaceBottleneck, SelfMemorySpacePressure,
    SelfMemorySpaceState, SelfState, build_self_state, render_self_state_block,
};
pub use session_summary_refresh::{
    SessionSummaryRefreshContext, SessionSummaryRefreshOutcome, fallback_session_summary,
    run_session_summary_refresh, should_refresh_session_summary,
};
pub(crate) use session_summary_refresh::{
    load_session_summary_snapshot, run_session_summary_refresh_with_snapshot,
};
pub(crate) use shared_factual_plane::{
    SharedFactualPlaneSnapshot, SharedFactualReconcileAction, build_archive_reconcile_drafts,
    build_shared_factual_plane_snapshot, render_private_memory_boundary_block,
    render_shared_factual_plane_block,
};
pub(crate) use skill_routing::{MemoryPlane, route_long_term_draft};
pub use turn_ledger::{
    REL_PATH_TURN_LEDGERS, REL_PATH_TURN_LEDGERS_LEGACY, TurnDeliveryLedger, TurnLedger,
    TurnLedgerStatus, TurnLedgerStore, build_turn_ledger_start, normalize_turn_preview,
    normalize_turn_reason,
};
pub(crate) use world_sense::run_world_sense_refresh_with_state;
pub use world_sense::{
    WORLD_SENSE_SYSTEM_PROMPT, WORLD_SENSE_TOTAL_CHAR_LIMIT, WorldSense, WorldSenseRefreshContext,
    WorldSenseRefreshInput, WorldSenseRefreshOutcome, WorldSnapshot, WorldSnapshotContext,
    build_world_snapshot, render_world_sense_block, render_world_snapshot_block,
    run_world_sense_refresh, world_snapshot_fingerprint,
};
pub(crate) use write_coordination::whole_record_lease_advanced;

/// 单次写入内容最大字节数（与 platform::spiffs 上界一致）。实现应拒绝超长写入。
pub const MAX_MEMORY_CONTENT_LEN: usize = 256 * 1024;
/// SOUL/USER 单次写入上限（实现应拒绝超长）。
pub const MAX_SOUL_USER_LEN: usize = 32 * 1024;

/// 单条会话消息最大长度（role + content 序列化后）。实现应拒绝超长单条。
pub const MAX_SESSION_MESSAGE_LEN: usize = 4 * 1024;
/// 单会话最大条数（ring 上界）。超过时实现应淘汰最旧再追加。
pub const MAX_SESSION_ENTRIES: usize = 128;

/// 相对路径（实现需拼接 SPIFFS_BASE）：MEMORY 文件。
pub const REL_PATH_MEMORY: &str = "memory/MEMORY.md";
/// 相对路径：SOUL 配置。
pub const REL_PATH_SOUL: &str = "config/SOUL.md";
/// 相对路径：USER 配置。
pub const REL_PATH_USER: &str = "config/USER.md";
/// 相对路径：每日笔记目录。
pub const REL_PATH_DAILY_DIR: &str = "memory/daily";
/// 相对路径：会话文件所在目录（文件名为 {chat_id}.jsonl）。短路径以满足 ESP-IDF VFS 路径长度上限（约 64 字符）。
pub const REL_PATH_SESSIONS_DIR: &str = "s";
/// 相对路径：HEARTBEAT 待办文件（与 memory 目录约定一致）。
pub const REL_PATH_HEARTBEAT: &str = "memory/HEARTBEAT.md";
/// 相对路径：待重试消息（低内存且队列满时落盘，单条 PcMsg JSON）。
pub const REL_PATH_PENDING_RETRY: &str = "memory/pending_retry.json";
/// 相对路径：重要消息偏移（截断时优先保留）；单 chat 单 offset。
pub const REL_PATH_IMPORTANT_MESSAGE: &str = "memory/important_message.json";
/// 相对路径：会话摘要（单文件 JSON，chat_id -> { summary, last_summary_at_count }）。
pub const REL_PATH_SESSION_SUMMARIES: &str = "memory/session_summaries.json";
/// 相对路径：Self Model（单文件 JSON，chat_id -> private subjective continuity）。
pub const REL_PATH_SELF_MODELS: &str = "memory/self_models.json";
/// 相对路径：World Sense（单文件 JSON，chat_id -> outer situational layer）。
pub const REL_PATH_WORLD_SENSE: &str = "memory/world_sense.json";
/// 相对路径：Outer Voice（单文件 JSON，chat_id -> outward expression layer）。
pub const REL_PATH_OUTER_VOICES: &str = "memory/outer_voices.json";
/// 相对路径：Autonomy Strategy（单文件 JSON，chat_id -> model-managed autonomy policy）。
pub const REL_PATH_AUTONOMY_STRATEGIES: &str = "memory/autonomy_strategies.json";
/// 相对路径：Inner Life（单文件 JSON，chat_id -> active subjective inward layer）。
pub const REL_PATH_INNER_LIFE: &str = "memory/inner_life.json";
/// 相对路径：Self Continuity（单文件 JSON，chat_id -> continuity + runtime anchors）。
pub const REL_PATH_SELF_CONTINUITIES: &str = "memory/self_continuities.json";
/// 相对路径：私有工作区（单文件 JSON，chat_id -> typed private docs workspace）。
pub const REL_PATH_PRIVATE_DOC_WORKSPACES: &str = "memory/private_doc_workspaces.json";
/// 相对路径：私有花园索引（单文件 JSON，chat_id -> free-form garden doc metadata）。
pub const REL_PATH_PRIVATE_GARDEN_INDEX: &str = "memory/private_garden_index.json";
/// 相对路径：私有花园正文目录（chat_id 子目录下存自由文档）。
pub const REL_PATH_PRIVATE_GARDEN_DIR: &str = "memory/private_garden";

/// 会话摘要存储。由 agent 程序性摘要写入；build_context 将 get 到的摘要注入 messages 首条。实现方按 SESSION_SUMMARY_MAX_LEN 截断。
pub trait SessionSummaryStore: Send + Sync {
    fn get(&self, chat_id: &str) -> Result<Option<String>>;
    fn set(&self, chat_id: &str, summary: &str) -> Result<()>;
    /// 带 message_count 的 set；实现方同时记录当时的会话消息条数。
    fn set_with_count(&self, chat_id: &str, summary: &str, _message_count: usize) -> Result<()> {
        self.set(chat_id, summary)
    }
    /// 获取摘要及其对应的 message_count；返回 (summary, last_message_count)。
    fn get_with_count(&self, chat_id: &str) -> Result<Option<(String, usize)>> {
        self.get(chat_id).map(|opt| opt.map(|s| (s, 0)))
    }
}

/// Self Model 存储。保存每个 chat 的私有主观连续性层，不与事实层混写。
pub trait SelfModelStore: Send + Sync {
    fn get(&self, chat_id: &str) -> Result<Option<SelfModel>>;
    fn set(&self, chat_id: &str, model: &SelfModel) -> Result<()>;
    fn clear(&self, chat_id: &str) -> Result<()>;
}

/// LLM 世界感知层。保存模型自己压缩的外部处境感觉。
pub trait WorldSenseStore: Send + Sync {
    fn get(&self, chat_id: &str) -> Result<Option<WorldSense>>;
    fn set(&self, chat_id: &str, world_sense: &WorldSense) -> Result<()>;
    fn clear(&self, chat_id: &str) -> Result<()>;
}

/// LLM 外在表达层。保存近期对外说话方式与表达姿态。
pub trait OuterVoiceStore: Send + Sync {
    fn get(&self, chat_id: &str) -> Result<Option<OuterVoice>>;
    fn set(&self, chat_id: &str, outer_voice: &OuterVoice) -> Result<()>;
    fn clear(&self, chat_id: &str) -> Result<()>;
}

/// LLM 自治策略层。保存模型自己维护的近期自治方针与空闲节奏。
pub trait AutonomyStrategyStore: Send + Sync {
    fn get(&self, chat_id: &str) -> Result<Option<AutonomyStrategy>>;
    fn set(&self, chat_id: &str, strategy: &AutonomyStrategy) -> Result<()>;
    fn clear(&self, chat_id: &str) -> Result<()>;
}

/// LLM 内心活动层。保存主观、可波动的私有内在状态。
pub trait InnerLifeStore: Send + Sync {
    fn get(&self, chat_id: &str) -> Result<Option<InnerLife>>;
    fn set(&self, chat_id: &str, inner_life: &InnerLife) -> Result<()>;
    fn clear(&self, chat_id: &str) -> Result<()>;
}

/// LLM 自我连续性层。保存“还是我”的桥梁与自治调度锚点。
pub trait SelfContinuityStore: Send + Sync {
    fn get(&self, chat_id: &str) -> Result<Option<SelfContinuity>>;
    fn set(&self, chat_id: &str, continuity: &SelfContinuity) -> Result<()>;
    fn clear(&self, chat_id: &str) -> Result<()>;
}

/// LLM 私有文档工作区。仅保存主观内部文档，不回写共享事实层。
pub trait PrivateDocStore: Send + Sync {
    fn get(&self, chat_id: &str) -> Result<Option<PrivateDocWorkspace>>;
    fn set(&self, chat_id: &str, workspace: &PrivateDocWorkspace) -> Result<()>;
    fn clear(&self, chat_id: &str) -> Result<()>;
}

/// LLM 私有花园。自由文档工作区，仍由程序保证 chat scope / 路径合法 / 配额。
pub trait PrivateGardenStore: Send + Sync {
    fn list(&self, chat_id: &str, limit: usize) -> Result<Vec<PrivateGardenDocRecord>>;
    fn read(&self, chat_id: &str, doc_path: &str) -> Result<Option<PrivateGardenDoc>>;
    fn write(
        &self,
        chat_id: &str,
        doc_path: &str,
        content: &str,
        now_secs: u64,
    ) -> Result<PrivateGardenDocRecord>;
    fn move_doc(
        &self,
        chat_id: &str,
        from_path: &str,
        to_path: &str,
        now_secs: u64,
    ) -> Result<Option<PrivateGardenDocRecord>>;
    fn delete(&self, chat_id: &str, doc_path: &str) -> Result<bool>;
}

/// 重要消息存储。offset_from_end=1 表示最后一条 user 消息。供 build_context 截断时优先保留。
pub trait ImportantMessageStore: Send + Sync {
    fn set_important_offset_from_end(&self, chat_id: &str, offset_from_end: u32) -> Result<()>;
    fn get_important_offset(&self, chat_id: &str) -> Result<Option<u32>>;
    fn clear_important(&self, chat_id: &str) -> Result<()>;
}

/// 到点提醒存储。add 写入 (channel, chat_id, at_unix_secs, context)；pop_due(now) 移除并返回一条 at<=now 的条目。
/// 条目数/context 长度上界见 constants::REMIND_AT_*。
pub trait RemindAtStore: Send + Sync {
    fn add(&self, channel: &str, chat_id: &str, at_unix_secs: u64, context: &str) -> Result<()>;
    /// 移除并返回一条 at <= now 的条目（任选其一）；无到点项返回 Ok(None)。
    fn pop_due(&self, now_unix_secs: u64) -> Result<Option<(String, String, String)>>;
    /// 返回下一条提醒的最早触发时间；无待触发项则返回 Ok(None)。
    fn next_due_at(&self) -> Result<Option<u64>> {
        Ok(None)
    }
    /// 查询当前会话未到点提醒，按 at 升序返回，limit 由调用方控制。
    fn list_upcoming(
        &self,
        channel: &str,
        chat_id: &str,
        now_unix_secs: u64,
        limit: usize,
    ) -> Result<Vec<(u64, String)>>;
}

/// 情绪信号存储。本轮模型输出带 [SIGNAL:comfort] 时 set，下一轮 build_context 时 get_then_clear 注入 system 后清除。
pub trait EmotionSignalStore: Send + Sync {
    fn set(&self, chat_id: &str, signal: &str) -> Result<()>;
    fn get_then_clear(&self, chat_id: &str) -> Result<Option<String>>;
}

/// 内存实现的 EmotionSignalStore；无持久化。
pub struct MemoryEmotionSignalStore(std::sync::Mutex<std::collections::HashMap<String, String>>);

impl MemoryEmotionSignalStore {
    pub fn new() -> Self {
        Self(std::sync::Mutex::new(std::collections::HashMap::new()))
    }
}

impl Default for MemoryEmotionSignalStore {
    fn default() -> Self {
        Self::new()
    }
}

impl EmotionSignalStore for MemoryEmotionSignalStore {
    fn set(&self, chat_id: &str, signal: &str) -> Result<()> {
        self.0
            .lock()
            .map_err(|e| crate::error::Error::Other {
                source: Box::new(std::io::Error::other(e.to_string())),
                stage: "emotion_signal_set",
            })?
            .insert(chat_id.to_string(), signal.to_string());
        Ok(())
    }

    fn get_then_clear(&self, chat_id: &str) -> Result<Option<String>> {
        Ok(self
            .0
            .lock()
            .map_err(|e: std::sync::PoisonError<_>| crate::error::Error::Other {
                source: Box::new(std::io::Error::other(e.to_string())),
                stage: "emotion_signal_get",
            })?
            .remove(chat_id))
    }
}

/// 待重试消息存储。实现由 platform 注入（如 SpiffsPendingRetryStore）。低内存且入队满时落盘，启动或循环前取回重试。
pub trait PendingRetryStore: Send + Sync {
    fn save_pending_retry(&self, msg: &PcMsg) -> Result<()>;
    fn load_pending_retry(&self) -> Result<Option<PcMsg>>;
    fn clear_pending_retry(&self) -> Result<()>;
}

/// 长期记忆与每日笔记存储。实现由 platform 注入（如 SpiffsMemoryStore）。
pub trait MemoryStore: Send + Sync {
    fn get_memory(&self) -> Result<String>;
    fn set_memory(&self, content: &str) -> Result<()>;
    fn get_soul(&self) -> Result<String>;
    /// 写入 SOUL 配置（config/SOUL.md）。实现应拒绝 content.len() > MAX_SOUL_USER_LEN。
    fn set_soul(&self, content: &str) -> Result<()>;
    fn get_user(&self) -> Result<String>;
    /// 写入 USER 配置（config/USER.md）。实现应拒绝 content.len() > MAX_SOUL_USER_LEN。
    fn set_user(&self, content: &str) -> Result<()>;
    /// 最近 N 条每日笔记的文件名（如 YYYY-MM-DD.md），按名称降序（最新在前）。
    fn list_daily_note_names(&self, recent_n: usize) -> Result<Vec<String>>;
    fn get_daily_note(&self, name: &str) -> Result<String>;
    fn write_daily_note(&self, name: &str, content: &str) -> Result<()>;
}

/// 会话单条消息，JSONL 行格式（role + content）。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionMessage {
    pub role: String,
    pub content: String,
}

/// 按 chat_id 的会话存储。实现由 platform 注入（如 SpiffsSessionStore）。
pub trait SessionStore: Send + Sync {
    fn append(&self, chat_id: &str, role: &str, content: &str) -> Result<()>;
    /// 批量追加多条消息；默认逐条 `append`。实现可覆写为单锁/单次 fsync 的热路径优化。
    fn append_batch(&self, chat_id: &str, messages: &[SessionMessage]) -> Result<()> {
        for message in messages {
            self.append(chat_id, &message.role, &message.content)?;
        }
        Ok(())
    }
    fn load_recent(&self, chat_id: &str, n: usize) -> Result<Vec<SessionMessage>>;
    /// 返回当前会话消息条数（不含可选头注释）。默认实现回退到 `load_recent(MAX_SESSION_ENTRIES)`。
    /// Implementations should override with an O(file-scan) fast path when possible.
    fn message_count(&self, chat_id: &str) -> Result<usize> {
        self.load_recent(chat_id, MAX_SESSION_ENTRIES)
            .map(|v| v.len())
    }
    fn clear(&self, chat_id: &str) -> Result<()>;
    /// 列举所有会话的 chat_id（如 sessions 目录下 *.jsonl 文件名去掉后缀）。用于 GET /api/sessions。
    fn list_chat_ids(&self) -> Result<Vec<String>>;
    /// 清理超过 max_age_secs 未修改的会话文件，返回清理数量。默认 no-op。
    fn gc_stale(&self, _max_age_secs: u64) -> Result<usize> {
        Ok(0)
    }
    /// 删除指定 chat_id 的会话文件。默认调用 clear。
    fn delete(&self, chat_id: &str) -> Result<()> {
        self.clear(chat_id)
    }
}

/// 系统提示聚合：SOUL + USER + MEMORY + 近期每日笔记，总长度不超过 max_len。
/// 截断策略：按字符边界逐段追加；预算不足时在当前段截断并停止后续拼装。
/// 纯函数，供 agent::context 使用；可 host 单测。
fn push_bounded_char_boundary(out: &mut String, input: &str, max_len: usize) -> bool {
    let remaining = max_len.saturating_sub(out.len());
    if remaining == 0 {
        return false;
    }
    if input.len() <= remaining {
        out.push_str(input);
        return true;
    }
    let mut end = remaining;
    while end > 0 && !input.is_char_boundary(end) {
        end -= 1;
    }
    if end > 0 {
        out.push_str(&input[..end]);
    }
    false
}

pub(crate) fn append_system_prompt_base(
    out: &mut String,
    soul: &str,
    user: &str,
    memory: &str,
    max_len: usize,
) {
    const SEP: &str = "\n\n";
    out.clear();
    let base_hint = soul
        .len()
        .saturating_add(user.len())
        .saturating_add(memory.len())
        .saturating_add(SEP.len() * 2);
    if out.capacity() < base_hint.min(max_len) {
        out.reserve(base_hint.min(max_len) - out.capacity());
    }
    if !push_bounded_char_boundary(out, soul.trim(), max_len) {
        return;
    }
    if !push_bounded_char_boundary(out, SEP, max_len) {
        return;
    }
    if !push_bounded_char_boundary(out, user.trim(), max_len) {
        return;
    }
    if !push_bounded_char_boundary(out, SEP, max_len) {
        return;
    }
    let _ = push_bounded_char_boundary(out, memory.trim(), max_len);
}

pub(crate) fn append_system_prompt_daily_note(
    out: &mut String,
    note: &str,
    max_len: usize,
) -> bool {
    const SEP: &str = "\n\n";
    if out.len() >= max_len {
        return false;
    }
    if !push_bounded_char_boundary(out, SEP, max_len) {
        return false;
    }
    push_bounded_char_boundary(out, note.trim(), max_len)
}

pub fn build_system_prompt(
    soul: &str,
    user: &str,
    memory: &str,
    daily_notes: &[String],
    max_len: usize,
) -> String {
    let mut out = String::with_capacity(max_len.min(soul.len() + user.len() + memory.len() + 512));
    append_system_prompt_base(&mut out, soul, user, memory, max_len);
    for note in daily_notes {
        if !append_system_prompt_daily_note(&mut out, note, max_len) {
            break;
        }
    }
    out
}

/// 单次 remind tick：检查到点提醒并注入 inbound。
/// 由 bg_timer 每 60s 调用一次。
pub(crate) fn remind_tick(
    remind_store: &dyn RemindAtStore,
    inbound_tx: &crate::bus::SystemInboundTx,
    resolve_locale: &std::sync::Arc<dyn Fn() -> crate::i18n::Locale + Send + Sync>,
) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    while let Ok(Some((channel, chat_id, context))) = remind_store.pop_due(now) {
        let loc = resolve_locale();
        let prefix = crate::i18n::tr(crate::i18n::Message::RemindPrefix, loc);
        let content = format!("{}{}", prefix, context);
        if let Ok(msg) = PcMsg::new_inbound_with_ingress(
            channel,
            chat_id,
            content,
            false,
            crate::bus::IngressKind::System,
        ) {
            let _ = inbound_tx.send(msg);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::build_system_prompt;

    #[test]
    fn build_system_prompt_respects_max_len() {
        let soul = "Soul";
        let user = "User";
        let memory = "Memory";
        let notes = vec!["Note1".to_string(), "Note2".to_string()];
        let out = build_system_prompt(soul, user, memory, &notes, 20);
        assert!(out.len() <= 20);
    }

    #[test]
    fn build_system_prompt_order() {
        let out = build_system_prompt("A", "B", "C", &[], 100);
        assert!(out.starts_with("A"));
        assert!(out.contains("B"));
        assert!(out.contains("C"));
    }
}
