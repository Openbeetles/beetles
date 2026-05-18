//! 记忆与会话抽象。仅定义 trait 与类型，不依赖 platform。
//! Memory and session abstraction. Traits only; no platform dependency.
use crate::bus::PcMsg;
use crate::error::Result;
use serde::{Deserialize, Serialize};

#[cfg(all(test, not(any(target_arch = "xtensa", target_arch = "riscv32"))))]
mod archive_benchmark;
mod archive_plane;
mod archive_search;
mod archive_selector;
mod autonomy_strategy;
mod context_window;
mod continuity_capsule;
mod continuity_snapshot;
mod core_revision_ledger;
mod execution_state;
mod felt_significance;
#[cfg(all(test, not(any(target_arch = "xtensa", target_arch = "riscv32"))))]
mod harness;
mod hygiene;
mod inner_conflict;
mod inner_life;
mod intelligence_replay;
mod internal_memory_topology;
mod llm_json;
mod long_term;
mod long_term_extraction;
mod maintenance;
mod memory_governance;
mod mental_privacy;
mod outer_voice;
#[cfg(all(test, not(any(target_arch = "xtensa", target_arch = "riscv32"))))]
mod persona_governance_benchmark;
mod persona_priority;
#[cfg(all(test, not(any(target_arch = "xtensa", target_arch = "riscv32"))))]
mod persona_regression;
mod personality_closure;
mod private_docs;
mod private_garden;
mod private_garden_governance;
mod profile;
mod prompt_context;
mod prompt_context_stages;
mod prompt_sanitizer;
#[cfg(all(test, not(any(target_arch = "xtensa", target_arch = "riscv32"))))]
mod recall_benchmark;
mod recall_contract;
mod recall_inspection;
mod recall_rerank;
mod recall_router;
mod recent_persona_evidence;
mod relationship_constitution;
mod relationship_portfolio;
mod relationship_topology;
mod self_authored_core;
mod self_continuity;
mod self_model;
mod self_runtime;
mod self_scope;
mod self_state;
mod session_summary_refresh;
mod shared_factual_plane;
mod shared_memory_governance;
mod skill_routing;
mod subject_shell;
mod temperament_continuity;
mod turn_continuity_evidence;
mod turn_ledger;
mod work_continuity;
mod world_sense;
mod write_coordination;

#[cfg(all(test, not(any(target_arch = "xtensa", target_arch = "riscv32"))))]
pub use archive_benchmark::{
    run_archive_benchmark_case, run_archive_benchmark_suite, ArchiveBenchmarkCase,
    ArchiveBenchmarkResult,
};
pub use archive_plane::build_archive_evidence_block;
pub(crate) use archive_search::maintain_archive_search_backend;
pub(crate) use archive_search::parse_daily_note_observed_at;
pub use archive_search::{
    archive_get_default_content_len, get_archive_record, search_archive_records,
    search_archive_records_detailed, ArchiveRecord, ArchiveRecordLocator, ArchiveRecordSource,
    ArchiveSearchHit, ArchiveSearchQuery, ArchiveSearchQueryReport, ArchiveSearchResult,
    ArchiveSearchSourceStats, MAX_ARCHIVE_GET_CONTENT_LEN, MAX_ARCHIVE_SEARCH_LIMIT,
};
pub(crate) use archive_selector::select_archive_hits_for_prompt_with_report;
pub use archive_selector::{
    ArchivePromptSelectionReport, ArchivePromptSelectionResult, ArchivePromptSelectionSourceStats,
};
pub(crate) use autonomy_strategy::estimate_autonomy_strategy_chars;
pub(crate) use autonomy_strategy::{
    autonomy_idle_interval_secs, run_autonomy_strategy_refresh_with_state,
};
pub use autonomy_strategy::{
    render_autonomy_strategy_block, run_autonomy_strategy_refresh, AutonomyGovernanceTendency,
    AutonomyStrategy, AutonomyStrategyRefreshContext, AutonomyStrategyRefreshInput,
    AutonomyStrategyRefreshOutcome, AUTONOMY_STRATEGY_SYSTEM_PROMPT,
    AUTONOMY_STRATEGY_TOTAL_CHAR_LIMIT,
};
pub use context_window::build_context_messages;
pub(crate) use continuity_capsule::{
    apply_continuity_capsule_drafts, build_post_reply_continuity_drafts,
    canonicalize_continuity_capsule,
};
pub use continuity_capsule::{
    build_continuity_capsule_operator_summary, inspect_continuity_capsule_recall,
    render_continuity_capsule_block, ContinuityCapsule, ContinuityCapsuleDraft,
    ContinuityCapsuleKind, ContinuityCapsuleOperatorSummary,
    ContinuityCapsuleRecallInspectionInput, ContinuityCapsuleScopeKind, ContinuityCapsuleSource,
    ContinuityCapsuleStatus, ContinuityCapsuleStore, ContinuityCapsuleWriteOutcome,
    MAX_CONTINUITY_CAPSULES, MAX_CONTINUITY_CAPSULES_PER_SCOPE, REL_PATH_CONTINUITY_CAPSULES,
};
pub(crate) use continuity_snapshot::select_active_continuity_snapshot_chat_ids;
pub use continuity_snapshot::{
    export_continuity_snapshot, import_continuity_snapshot, render_continuity_snapshot_markdown,
    select_personality_governance_targets, ContinuitySnapshot, ContinuitySnapshotExportContext,
    ContinuitySnapshotImportContext, ContinuitySnapshotImportDecision,
    ContinuitySnapshotImportMode, ContinuitySnapshotImportOutcome, ContinuitySnapshotKindCount,
    ContinuitySnapshotManifest, ContinuitySnapshotMode,
};
pub use core_revision_ledger::{
    append_core_revision_record, build_core_revision_timeline,
    compute_core_revision_governance_digest, core_revision_observation_due_at, correction_pressure,
    has_recent_matching_adopted_change, has_recent_matching_rejected_change,
    recent_adopted_revision, recent_rejected_direction_count,
    render_core_revision_governance_block, render_core_revision_ledger_block,
    CoreRevisionActionKind, CoreRevisionConflictClass, CoreRevisionCorrectionKind,
    CoreRevisionGovernanceDigest, CoreRevisionLedger, CoreRevisionOutcome, CoreRevisionRecord,
    CoreRevisionRecordChange, CoreRevisionTimelineEntry,
};
pub use execution_state::{
    render_execution_state_block, run_execution_state_refresh, ExecutionState,
    ExecutionStateRefreshContext, ExecutionStateRefreshInput, ExecutionStateRefreshOutcome,
    ExecutionStateStore, ExecutionStatus, EXECUTION_STATE_SYSTEM_PROMPT, REL_PATH_EXECUTION_STATES,
};
pub(crate) use execution_state::{
    run_execution_state_refresh_with_state, seed_execution_state_from_turn,
    should_refresh_execution_state, ProvisionalExecutionStateInput,
};
pub(crate) use felt_significance::{
    build_felt_significance_refresh_input, run_felt_significance_refresh_with_state,
    FeltSignificanceRefreshCandidate,
};
pub use felt_significance::{
    render_felt_significance_block, FeltSignificance, FeltSignificanceRefreshOutcome,
    FELT_SIGNIFICANCE_SYSTEM_CONTRACT, FELT_SIGNIFICANCE_TOTAL_CHAR_LIMIT,
};
pub(crate) use hygiene::run_memory_hygiene_jobs;
pub use hygiene::{
    inspect_memory_hygiene, render_memory_hygiene_inspection_markdown, MemoryHygieneContext,
    MemoryHygieneInspection, MemoryHygieneOutcome,
};
pub(crate) use inner_conflict::{
    build_inner_conflict_refresh_input, run_inner_conflict_refresh_with_state,
    InnerConflictRefreshCandidate,
};
pub use inner_conflict::{
    render_inner_conflict_block, InnerConflict, InnerConflictRefreshOutcome,
    INNER_CONFLICT_MAX_REVIEW_AFTER_SECS, INNER_CONFLICT_SYSTEM_CONTRACT,
    INNER_CONFLICT_TOTAL_CHAR_LIMIT,
};
pub(crate) use inner_life::estimate_inner_life_chars;
pub(crate) use inner_life::run_inner_life_refresh_with_state;
pub use inner_life::{
    render_inner_life_block, run_inner_life_refresh, InnerLife, InnerLifeRefreshContext,
    InnerLifeRefreshInput, InnerLifeRefreshOutcome, INNER_LIFE_SYSTEM_PROMPT,
    INNER_LIFE_TOTAL_CHAR_LIMIT,
};
pub use intelligence_replay::{
    inspect_intelligence_replay, IntelligenceReplayAlert, IntelligenceReplayAlertCode,
    IntelligenceReplayInspection, IntelligenceReplayTurnDigest,
};
pub(crate) use internal_memory_topology::{
    render_internal_memory_topology_block, InternalMemoryLayerFocus,
};
pub(crate) use long_term::{
    canonicalize_long_term_memory_entry, compare_long_term_memory_query_results,
    govern_long_term_memory_entries, inspect_long_term_memory_merge_guard,
    long_term_memory_effective_stale_hint, long_term_memory_entry_from_draft,
    long_term_memory_evidence_state, long_term_memory_matches_query, merge_long_term_memory_entry,
    recall_long_term_memory_entries, score_long_term_memory_recall_breakdown,
    select_long_term_recall_entries, LongTermMemoryMergeGuardDecision,
};
pub use long_term::{
    long_term_memory_evidence_summary, lookup_long_term_memory_slot,
    parse_explicit_long_term_slot_query, recall_long_term_memory_block,
    render_exact_long_term_memory_block, render_long_term_memory_block, LongTermMemoryConfidence,
    LongTermMemoryDraft, LongTermMemoryEntry, LongTermMemoryEvidenceState,
    LongTermMemoryEvidenceSummary, LongTermMemoryFreshness, LongTermMemoryKind,
    LongTermMemoryQuery, LongTermMemorySlot, LongTermMemorySlotLookup, LongTermMemorySourceScope,
    LongTermMemorySourceType, LongTermMemoryStaleHint, LongTermMemoryStore,
    MAX_LONG_TERM_MEMORY_BLOCK_LEN, MAX_LONG_TERM_MEMORY_CONTENT_LEN, MAX_LONG_TERM_MEMORY_ITEMS,
    MAX_LONG_TERM_MEMORY_KEYWORDS, MAX_LONG_TERM_MEMORY_KEYWORD_LEN, REL_PATH_LONG_TERM_MEMORIES,
};
pub use long_term_extraction::{
    apply_long_term_memory_extraction, build_long_term_memory_extraction_input,
    evaluate_long_term_memory_extraction_turn, mark_long_term_memory_extraction_deferred,
    mark_long_term_memory_extraction_processed, mark_long_term_memory_extraction_requested,
    parse_long_term_memory_extraction_response, persist_long_term_memory_extraction_state,
    run_long_term_memory_refresh, LongTermMemoryExtractionState,
    LongTermMemoryExtractionStateStore, LongTermMemoryExtractionTurnDecision,
    LongTermMemoryExtractionTurnInput, LongTermMemoryRefreshContext, LongTermMemoryRefreshOutcome,
    ParsedLongTermMemoryExtraction, LONG_TERM_MEMORY_EXTRACTION_BATCH,
    LONG_TERM_MEMORY_EXTRACTION_RECENT_N, LONG_TERM_MEMORY_EXTRACTION_SYSTEM_PROMPT,
    REL_PATH_LONG_TERM_EXTRACTION_STATES,
};
pub use maintenance::{
    run_post_reply_memory_maintenance, ContinuityCapsuleMaintenanceOutcome,
    LongTermMemoryRefreshRequestOutcome, PostReplyMemoryMaintenanceContext,
    PostReplyMemoryMaintenanceInput, PostReplyMemoryMaintenanceOutcome,
};
pub(crate) use memory_governance::run_memory_governance_kernel;
pub use memory_governance::{
    MemoryGovernanceContext, MemoryGovernanceInput, MemoryGovernanceOutcome,
};
pub(crate) use mental_privacy::{
    collect_private_targets, render_mental_privacy_boundary_block,
    render_mental_privacy_disclosure_adjudication_block,
    render_mental_privacy_governance_fallback_block, run_boundary_persona_refresh_with_state,
    run_mental_privacy_review,
};
pub use mental_privacy::{
    mental_privacy_adjudication_failure_fallback, run_mental_privacy_disclosure_adjudication,
    BoundaryDisclosureStyle, BoundaryPersonaPosture, BoundaryPersonaRefreshContext,
    BoundaryPersonaRefreshInput, BoundaryPersonaRefreshOutcome, BoundaryPersonaState,
    MentalPrivacyConsentLog, MentalPrivacyDisclosureAdjudication,
    MentalPrivacyDisclosureAdjudicationContext, MentalPrivacyDisclosureAdjudicationInput,
    MentalPrivacyEnvelope, MentalPrivacyLayer, MentalPrivacyLogStage, MentalPrivacyOwnerAccessMode,
    MentalPrivacyQuotePolicy, MentalPrivacyRequester, MentalPrivacyReviewContext,
    MentalPrivacyReviewInput, MentalPrivacyReviewOutcome, MentalPrivacyShareAction,
    MentalPrivacyState, MentalPrivacyStore, MentalPrivacyVisibility, RelationalBoundaryState,
    MENTAL_PRIVACY_SYSTEM_CONSTRAINT, MENTAL_PRIVACY_TARGET_INNER_LIFE,
    MENTAL_PRIVACY_TARGET_SELF_CONTINUITY, MENTAL_PRIVACY_TARGET_SELF_MODEL,
    REL_PATH_MENTAL_PRIVACY_STATES,
};
pub(crate) use outer_voice::run_outer_voice_refresh_with_state;
pub use outer_voice::{
    render_outer_voice_block, OuterVoice, OuterVoiceRefreshContext, OuterVoiceRefreshInput,
    OuterVoiceRefreshOutcome, OUTER_VOICE_SYSTEM_PROMPT, OUTER_VOICE_TOTAL_CHAR_LIMIT,
};
#[cfg(all(test, not(any(target_arch = "xtensa", target_arch = "riscv32"))))]
pub use persona_governance_benchmark::{
    run_persona_governance_replay_case, run_persona_governance_replay_suite,
    PersonaGovernanceReplayCase, PersonaGovernanceReplayResult,
};
pub use persona_priority::{
    build_persistent_persona_priority_adjudication, render_persistent_persona_priority_block,
    render_persona_priority_block, run_persona_priority_adjudication,
    should_run_persona_priority_adjudication, PersonaPriorityAdjudication,
    PersonaPriorityAdjudicationInput, PersonaPriorityGrounding, PersonaPriorityRuntimeState,
    PERSONA_PRIORITY_SYSTEM_PROMPT,
};
#[cfg(all(test, not(any(target_arch = "xtensa", target_arch = "riscv32"))))]
pub use persona_regression::{
    run_persona_continuity_case, run_persona_continuity_suite, PersonaContinuityCase,
    PersonaContinuityResult,
};
pub use personality_closure::{
    derive_personality_governance_repair_plan, derive_personality_runtime_governance_gate,
    derive_personality_runtime_governance_gate_from_inspection, inspect_personality_governance,
    render_personality_governance_inspection_markdown,
    render_personality_runtime_governance_gate_block, PersonalityClosureReport,
    PersonalityGovernanceEvent, PersonalityGovernanceInspection,
    PersonalityGovernanceInspectionInput, PersonalityGovernanceRepairAction,
    PersonalityGovernanceRepairPlan, PersonalityRuntimeGovernanceGate,
};
pub(crate) use private_docs::estimate_private_doc_workspace_chars;
pub(crate) use private_docs::run_private_doc_workspace_refresh_with_state;
pub use private_docs::{
    render_private_doc_workspace_block, run_private_doc_workspace_refresh, PrivateDocEntry,
    PrivateDocWorkspace, PrivateDocWorkspaceRefreshContext, PrivateDocWorkspaceRefreshInput,
    PrivateDocWorkspaceRefreshOutcome, PRIVATE_DOC_WORKSPACE_SYSTEM_PROMPT,
    PRIVATE_DOC_WORKSPACE_TOTAL_CHAR_LIMIT,
};
pub(crate) use private_garden::build_private_garden_preview;
pub use private_garden::{
    build_private_garden_usage, classify_private_garden_doc_path,
    normalize_private_garden_doc_path, render_private_garden_block,
    summarize_private_garden_directories, PrivateGardenDirectorySummary, PrivateGardenDoc,
    PrivateGardenDocRecord, PrivateGardenDocRole, PrivateGardenUsage,
    PRIVATE_GARDEN_MAX_DOCS_PER_CHAT, PRIVATE_GARDEN_MAX_DOC_BYTES,
    PRIVATE_GARDEN_TOTAL_BYTE_LIMIT,
};
pub(crate) use private_garden_governance::run_private_garden_governance_with_state;
pub use private_garden_governance::{
    run_private_garden_governance, PrivateGardenGovernanceContext, PrivateGardenGovernanceInput,
    PrivateGardenGovernanceOutcome, PRIVATE_GARDEN_GOVERNANCE_SYSTEM_PROMPT,
};
pub(crate) use profile::{
    decide_prompt_assembly, decide_self_runtime_authority, memory_capability_profile,
    memory_policy, prompt_context_normalization_budget, prompt_participation_policy,
    shared_long_term_governance_policy, AutonomyStrategyPolicy, ExecutionStatePolicy,
    InnerLifePolicy, LongTermExtractionPolicy, LongTermRecallPolicy, OuterVoicePolicy,
    PrivateDocsPolicy, PrivateGardenGovernancePolicy, SelfContinuityPolicy, SelfModelPolicy,
    SelfRuntimeAuthorityPlan, SessionSummaryPolicy, WorldSensePolicy,
};
pub use profile::{
    MemoryCapabilityClass, MemoryHygieneLevel, MemoryProfile, MemorySystemKind,
    PromptParticipationPlan,
};
pub use prompt_context::{
    load_prompt_memory_context, PromptMemoryContext, PromptMemoryContextParams, PromptRuntimeCarry,
};
pub(crate) use prompt_sanitizer::{scrub_memory_prompt_block, scrub_private_source_echoes};
#[cfg(all(test, not(any(target_arch = "xtensa", target_arch = "riscv32"))))]
pub use recall_benchmark::{
    compute_recall_benchmark_metrics, run_recall_benchmark_case, run_recall_benchmark_suite,
    RecallBenchmarkCase, RecallBenchmarkMetrics, RecallBenchmarkResult,
};
pub use recall_contract::{
    inspect_archive_recall, inspect_runtime_skill_recall, inspect_shared_factual_recall,
    inspect_task_recall, RecallCandidate, RecallPlane, RecallQuery, RecallScoreBreakdown,
    RecallSelectionReport,
};
pub use recall_inspection::{
    inspect_working_recall, render_working_recall_inspection_markdown, WorkingRecallInspection,
    WorkingRecallInspectionInput,
};
pub(crate) use recall_rerank::{
    build_cross_plane_rerank_result, plane_signal_score, CrossPlaneRerankInput,
    CrossPlaneRerankResult,
};
#[cfg(test)]
pub(crate) use recall_rerank::{CrossPlanePlaneSignal, CrossPlaneRerankCandidate};
pub(crate) use recall_router::{
    build_continuity_recall_query, decide_prompt_recall_route, PromptRecallIntent,
    PromptRecallRouterDecision,
};
pub use recent_persona_evidence::{
    derive_recent_persona_evidence, derive_recent_persona_evidence_from_continuity_evidence,
    load_recent_persona_evidence, render_recent_persona_evidence_block, RecentPersonaEvidence,
    RECENT_PERSONA_EVIDENCE_HISTORY_LOOKBACK, RECENT_PERSONA_EVIDENCE_MEANINGFUL_TURNS,
};
pub use relationship_constitution::{
    audit_relationship_constitution, clamp_boundary_persona_to_constitution,
    derive_relationship_constitution, enforce_relationship_constitution_share_action,
    render_relationship_constitution_block, sync_relationship_constitution,
    RelationshipBoundaryShift, RelationshipConstitution, RelationshipConstitutionAlignment,
    RelationshipConstitutionAudit, RelationshipConstitutionOverride,
    RelationshipConstitutionOverrideDomain, RelationshipConstitutionStore,
    RelationshipConstitutionSyncInput, RelationshipDisclosureAllowance,
    RelationshipOuterVoiceShift, RelationshipTaskScopeCeiling, REL_PATH_RELATIONSHIP_CONSTITUTIONS,
};
pub use relationship_portfolio::{
    render_relationship_portfolio_block, select_relationship_portfolio_targets,
    sync_relationship_portfolio, touch_relationship_portfolio_selection,
    RelationshipGovernanceState, RelationshipInheritanceMode, RelationshipPortfolio,
    RelationshipPortfolioEntry, RelationshipPortfolioSelectorInput, RelationshipPortfolioStore,
    RelationshipPortfolioSyncOutcome, REL_PATH_RELATIONSHIP_PORTFOLIOS,
};
pub use relationship_topology::{
    render_relationship_topology_block, select_relationship_topology_targets,
    upsert_relationship_topology_entry, RelationshipSelectionTarget, RelationshipSelectorInput,
    RelationshipTopology, RelationshipTopologyEntry, RelationshipTopologyRefreshOutcome,
    RelationshipTopologyStore, RelationshipTopologyUpsertInput, REL_PATH_RELATIONSHIP_TOPOLOGIES,
};
pub(crate) use self_authored_core::run_self_authored_core_refresh_with_state;
pub use self_authored_core::{
    render_persistent_self_authored_core_block, render_self_authored_core_block, SelfAuthoredCore,
    SelfAuthoredCoreRefreshContext, SelfAuthoredCoreRefreshInput, SelfAuthoredCoreRefreshOutcome,
    SELF_AUTHORED_CORE_SYSTEM_PROMPT, SELF_AUTHORED_CORE_TOTAL_CHAR_LIMIT,
};
pub(crate) use self_continuity::estimate_self_continuity_chars;
pub(crate) use self_continuity::run_self_continuity_refresh_with_state;
pub use self_continuity::{
    render_self_continuity_block, run_self_continuity_refresh, touch_self_continuity_runtime,
    SelfContinuity, SelfContinuityRefreshContext, SelfContinuityRefreshInput,
    SelfContinuityRefreshOutcome, SELF_CONTINUITY_SYSTEM_PROMPT, SELF_CONTINUITY_TOTAL_CHAR_LIMIT,
};
pub(crate) use self_model::estimate_self_model_chars;
pub(crate) use self_model::run_self_model_refresh_with_state;
pub use self_model::{
    render_self_model_block, run_self_model_refresh, SelfModel, SelfModelRefreshContext,
    SelfModelRefreshInput, SelfModelRefreshOutcome, SELF_MODEL_SYSTEM_PROMPT,
    SELF_MODEL_TOTAL_CHAR_LIMIT,
};
pub use self_runtime::{
    enqueue_self_runtime_idle_tick, enqueue_self_runtime_operator_request,
    enqueue_self_runtime_post_reply, run_self_runtime, self_runtime_tick, SelfRuntimeContext,
    SelfRuntimeDecision, SelfRuntimeJobPayload, SelfRuntimeOutcome, SelfRuntimeTrigger,
    SELF_RUNTIME_CHANNEL, SELF_RUNTIME_SYSTEM_PROMPT,
};
pub use self_scope::{
    board_subject_scope_id, private_garden_scope_id, relationship_scope_id, BOARD_SUBJECT_SCOPE_ID,
    PRIVATE_GARDEN_SCOPE_ID,
};
pub use self_state::{
    build_self_state, render_self_state_block, SelfAutonomyState, SelfAutonomyStatus,
    SelfInnerState, SelfMemoryGovernancePosture, SelfMemorySpaceActivity,
    SelfMemorySpaceBottleneck, SelfMemorySpacePressure, SelfMemorySpaceState, SelfState,
};
pub use session_summary_refresh::{
    fallback_session_summary, run_session_summary_refresh, should_refresh_session_summary,
    SessionSummaryRefreshContext, SessionSummaryRefreshOutcome,
};
pub(crate) use session_summary_refresh::{
    load_session_summary_snapshot, run_session_summary_refresh_with_snapshot,
};
pub(crate) use shared_factual_plane::{
    build_archive_reconcile_drafts, build_shared_factual_plane_snapshot,
    render_private_memory_boundary_block, render_shared_factual_plane_block,
    SharedFactualPlaneSnapshot, SharedFactualReconcileAction,
};
pub use shared_memory_governance::{
    write_governed_shared_memory, SharedMemoryWriteAction, SharedMemoryWriteItemReport,
    SharedMemoryWriteOutcome, SharedMemoryWriteReason, SharedMemoryWriteSource,
};
pub(crate) use skill_routing::{route_long_term_draft, MemoryPlane};
pub(crate) use subject_shell::{compile_subject_shell, SubjectShell, SubjectShellCompileInput};
pub(crate) use temperament_continuity::{
    build_temperament_continuity_refresh_input, run_temperament_continuity_refresh_with_state,
    TemperamentContinuityRefreshCandidate,
};
pub use temperament_continuity::{
    render_temperament_continuity_block, TemperamentContinuity,
    TemperamentContinuityRefreshOutcome, TEMPERAMENT_CONTINUITY_SYSTEM_CONTRACT,
    TEMPERAMENT_CONTINUITY_TOTAL_CHAR_LIMIT,
};
pub use turn_continuity_evidence::{
    TurnContinuityEvidence, TurnContinuityEvidenceStore, REL_PATH_TURN_CONTINUITY_EVIDENCE,
    TURN_CONTINUITY_EVIDENCE_HISTORY_MAX_ITEMS,
};
pub use turn_ledger::{
    build_turn_ledger_start, build_turn_persona_disclosure_ledger,
    build_turn_persona_priority_ledger, normalize_turn_observation_text,
    normalize_turn_persona_scope, normalize_turn_persona_targets, normalize_turn_preview,
    normalize_turn_reason, normalize_turn_subject_state_summary, normalize_turn_subject_state_text,
    render_turn_adversarial_arena_ledger_block, render_turn_counterfactual_ledger_block,
    render_turn_observation_ledger_block, render_turn_persona_ledger_block,
    render_turn_reasoning_intent_ledger_block, turn_ledger_observed_at_ms,
    TurnAdversarialArenaClaimLedger, TurnAdversarialArenaLedger, TurnBlockerLedger,
    TurnCounterfactualBranchLedger, TurnCounterfactualLedger, TurnCounterfactualSnapshotLedger,
    TurnDeliberationClass, TurnDeliveryLedger, TurnExecutionClass, TurnLedger, TurnLedgerStatus,
    TurnLedgerStore, TurnModeSnapshotLedger, TurnObservationLedger, TurnPersonaDisclosureLedger,
    TurnPersonaLedger, TurnPersonaPressureLevel, TurnPersonaPriorityLedger,
    TurnPersonaReviewLedger, TurnReasoningIntentLedger, TurnSoulFeedbackLedger,
    TurnSoulInitiativeLedger, TurnSoulReplyLedger, TurnSoulStrategyLedger, TurnSubjectStateLedger,
    TurnToolPathLedger, VolatileTurnLedgerStore, REL_PATH_TURN_LEDGERS,
    REL_PATH_TURN_LEDGER_HISTORY, TURN_LEDGER_HISTORY_MAX_ITEMS,
};
pub use work_continuity::{
    build_work_continuity_record, render_work_continuity_block, WorkContinuityRecord,
    MAX_WORK_CONTINUITY_BLOCK_LEN,
};
pub(crate) use world_sense::run_world_sense_refresh_with_state;
pub use world_sense::{
    build_world_snapshot, render_world_sense_block, render_world_snapshot_block,
    run_world_sense_refresh, world_snapshot_fingerprint, WorldSense, WorldSenseRefreshContext,
    WorldSenseRefreshInput, WorldSenseRefreshOutcome, WorldSnapshot, WorldSnapshotContext,
    WORLD_SENSE_SYSTEM_PROMPT, WORLD_SENSE_TOTAL_CHAR_LIMIT,
};
pub(crate) use world_sense::{
    build_world_snapshot_from_commitments, load_world_snapshot_reminders, load_world_snapshot_tasks,
};
pub(crate) use write_coordination::whole_record_lease_advanced;

/// 单次写入内容最大字节数（与 platform::storage 上界一致）。实现应拒绝超长写入。
pub const MAX_MEMORY_CONTENT_LEN: usize = 256 * 1024;

/// 单条会话消息最大长度（role + content 序列化后）。实现应拒绝超长单条。
pub const MAX_SESSION_MESSAGE_LEN: usize = 4 * 1024;
/// 单会话最大条数（ring 上界）。超过时实现应淘汰最旧再追加。
pub const MAX_SESSION_ENTRIES: usize = 128;

/// 相对路径（实现需拼接 storage_BASE）：MEMORY 文件。
pub const REL_PATH_MEMORY: &str = "memory/MEMORY.md";
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
/// 相对路径：Felt Significance（单文件 JSON，scope/chat scoped key -> subjective weight）。
pub const REL_PATH_FELT_SIGNIFICANCES: &str = "memory/felt_significances.json";
/// 相对路径：Temperament Continuity（单文件 JSON，scope/chat scoped key -> durable inertia）。
pub const REL_PATH_TEMPERAMENT_CONTINUITIES: &str = "memory/temperament_continuities.json";
/// 相对路径：Inner Conflict（单文件 JSON，scope/chat scoped key -> bounded unresolved tension）。
pub const REL_PATH_INNER_CONFLICTS: &str = "memory/inner_conflicts.json";
/// 相对路径：Self-Authored Core（单文件 JSON，scope_id -> persistent board-level self core）。
pub const REL_PATH_SELF_AUTHORED_CORES: &str = "memory/self_authored_cores.json";
/// 相对路径：Self-Authored Core 修订账本（单文件 JSON，scope_id -> versioned revision ledger）。
pub const REL_PATH_CORE_REVISION_LEDGERS: &str = "memory/core_revision_ledgers.json";
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

/// Self-Authored Core 存储。保存板级主体可跨 chat 继承的稳定自我核心。
pub trait SelfAuthoredCoreStore: Send + Sync {
    fn get(&self, scope_id: &str) -> Result<Option<SelfAuthoredCore>>;
    fn set(&self, scope_id: &str, core: &SelfAuthoredCore) -> Result<()>;
    fn clear(&self, scope_id: &str) -> Result<()>;
}

/// Self-Authored Core 修订账本存储。保存板级主体的修订候选、裁决结果与版本晋升轨迹。
pub trait CoreRevisionLedgerStore: Send + Sync {
    fn get(&self, scope_id: &str) -> Result<Option<CoreRevisionLedger>>;
    fn set(&self, scope_id: &str, ledger: &CoreRevisionLedger) -> Result<()>;
    fn clear(&self, scope_id: &str) -> Result<()>;
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

/// Felt Significance 存储。保存 scope/chat scoped key 的当前主观重量层。
pub trait FeltSignificanceStore: Send + Sync {
    fn get(&self, scope_id: &str) -> Result<Option<FeltSignificance>>;
    fn set(&self, scope_id: &str, significance: &FeltSignificance) -> Result<()>;
    fn clear(&self, scope_id: &str) -> Result<()>;
}

/// Temperament Continuity 存储。保存 scope/chat scoped key 的长期行为惯性层。
pub trait TemperamentContinuityStore: Send + Sync {
    fn get(&self, scope_id: &str) -> Result<Option<TemperamentContinuity>>;
    fn set(&self, scope_id: &str, continuity: &TemperamentContinuity) -> Result<()>;
    fn clear(&self, scope_id: &str) -> Result<()>;
}

/// Inner Conflict 存储。保存 scope/chat scoped key 的有界未决内在拉扯。
pub trait InnerConflictStore: Send + Sync {
    fn get(&self, scope_id: &str) -> Result<Option<InnerConflict>>;
    fn set(&self, scope_id: &str, conflict: &InnerConflict) -> Result<()>;
    fn clear(&self, scope_id: &str) -> Result<()>;
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

/// 到点提醒存储。持久条目带稳定 id 与可选 calendar link；到期投递必须先非破坏性 list，成功入队后再按快照 delete。
/// 条目数/context 长度上界见 constants::REMIND_AT_*，字段规范见 `crate::reminder::ReminderItem`。
pub trait RemindAtStore: Send + Sync {
    fn get(
        &self,
        channel: &str,
        chat_id: &str,
        id: &str,
    ) -> Result<Option<crate::reminder::ReminderItem>>;
    fn upsert(&self, reminder: &crate::reminder::ReminderItem) -> Result<()>;
    fn delete(&self, channel: &str, chat_id: &str, id: &str) -> Result<bool>;
    /// 非破坏性列出最早到期提醒；用于成功投递 system inbound 后再显式删除。
    fn list_due(
        &self,
        now_unix_secs: u64,
        limit: usize,
    ) -> Result<Vec<crate::reminder::ReminderItem>>;
    /// 删除仍与投递快照一致的 due reminder；若用户已更新同 id 条目则不得删除。
    fn delete_due(&self, reminder: &crate::reminder::ReminderItem) -> Result<bool>;
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
    ) -> Result<Vec<crate::reminder::ReminderItem>>;
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

/// 待重试消息存储。实现由 platform 注入（如 StoragePendingRetryStore）。低内存且入队满时落盘，启动或循环前取回重试。
pub trait PendingRetryStore: Send + Sync {
    fn save_pending_retry(&self, msg: &PcMsg) -> Result<()>;
    fn load_pending_retry(&self) -> Result<Option<PcMsg>>;
    fn clear_pending_retry(&self) -> Result<()>;
}

/// 长期记忆与每日笔记存储。实现由 platform 注入（如 StorageMemoryStore）。
pub trait MemoryStore: Send + Sync {
    fn get_memory(&self) -> Result<String>;
    fn set_memory(&self, content: &str) -> Result<()>;
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

/// 带稳定 message_id 的会话消息记录。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionMessageRecord {
    pub message_id: String,
    pub role: String,
    pub content: String,
}

impl SessionMessageRecord {
    pub fn into_message(self) -> SessionMessage {
        SessionMessage {
            role: self.role,
            content: self.content,
        }
    }

    pub fn as_message(&self) -> SessionMessage {
        SessionMessage {
            role: self.role.clone(),
            content: self.content.clone(),
        }
    }
}

const SESSION_MESSAGE_ID_FNV_OFFSET: u64 = 0xcbf29ce484222325;
const SESSION_MESSAGE_ID_FNV_PRIME: u64 = 0x100000001b3;

fn session_message_id_hash_update(hash: &mut u64, bytes: &[u8]) {
    for byte in bytes {
        *hash ^= u64::from(*byte);
        *hash = hash.wrapping_mul(SESSION_MESSAGE_ID_FNV_PRIME);
    }
}

pub(crate) fn synthesize_session_message_id(
    chat_id: &str,
    role: &str,
    content: &str,
    occurrence: u32,
) -> String {
    let mut hash = SESSION_MESSAGE_ID_FNV_OFFSET;
    session_message_id_hash_update(&mut hash, chat_id.as_bytes());
    session_message_id_hash_update(&mut hash, &[0]);
    session_message_id_hash_update(&mut hash, role.as_bytes());
    session_message_id_hash_update(&mut hash, &[0]);
    session_message_id_hash_update(&mut hash, content.as_bytes());
    session_message_id_hash_update(&mut hash, &[0]);
    session_message_id_hash_update(&mut hash, &occurrence.to_le_bytes());
    format!("legacy_{hash:016x}")
}

pub(crate) fn synthesize_session_message_records(
    chat_id: &str,
    messages: Vec<SessionMessage>,
) -> Vec<SessionMessageRecord> {
    let mut seen = std::collections::HashMap::<(String, String), u32>::new();
    messages
        .into_iter()
        .map(|message| {
            let key = (message.role.clone(), message.content.clone());
            let occurrence = seen
                .entry(key)
                .and_modify(|count| *count = count.saturating_add(1))
                .or_insert(1);
            SessionMessageRecord {
                message_id: synthesize_session_message_id(
                    chat_id,
                    message.role.as_str(),
                    message.content.as_str(),
                    *occurrence,
                ),
                role: message.role,
                content: message.content,
            }
        })
        .collect()
}

/// 按 chat_id 的会话存储。实现由 platform 注入（如 StorageSessionStore）。
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
    /// 返回最近 N 条消息及其稳定 message_id。
    /// 默认实现会从 `load_recent` 合成 legacy 稳定 id；持久化实现应覆写为真正持久主键。
    fn load_recent_records(&self, chat_id: &str, n: usize) -> Result<Vec<SessionMessageRecord>> {
        self.load_recent(chat_id, n)
            .map(|messages| synthesize_session_message_records(chat_id, messages))
    }
    /// 返回当前会话消息条数（不含可选头注释）。默认实现回退到 `load_recent(MAX_SESSION_ENTRIES)`。
    /// Implementations should override with an O(file-scan) fast path when possible.
    fn message_count(&self, chat_id: &str) -> Result<usize> {
        self.load_recent(chat_id, MAX_SESSION_ENTRIES)
            .map(|v| v.len())
    }
    fn clear(&self, chat_id: &str) -> Result<()>;
    /// 列举所有会话的 chat_id（如 sessions 目录下 *.jsonl 文件名去掉后缀）。用于 GET /api/sessions。
    fn list_chat_ids(&self) -> Result<Vec<String>>;
    /// 删除指定 chat_id 的会话文件。默认调用 clear。
    fn delete(&self, chat_id: &str) -> Result<()> {
        self.clear(chat_id)
    }
}

/// 系统提示聚合：MEMORY + 近期每日笔记，总长度不超过 max_len。
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

pub(crate) fn append_system_prompt_base(out: &mut String, memory: &str, max_len: usize) {
    out.clear();
    let base_hint = memory.len();
    let reserve_hint = base_hint.min(max_len);
    if out.capacity() < reserve_hint {
        out.reserve(reserve_hint - out.capacity());
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

pub fn build_system_prompt(memory: &str, daily_notes: &[String], max_len: usize) -> String {
    let mut out = String::with_capacity(max_len.min(memory.len() + 512));
    append_system_prompt_base(&mut out, memory, max_len);
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
    mut cleanup: impl FnMut(&crate::reminder::ReminderItem) -> Result<()>,
    inbound_tx: &crate::bus::SystemInboundTx,
    resolve_locale: &std::sync::Arc<dyn Fn() -> crate::i18n::Locale + Send + Sync>,
) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let limit = crate::constants::DUE_REMINDER_SWEEP_BATCH_MAX.min(inbound_tx.remaining_capacity());
    if limit == 0 {
        return;
    }
    let due = match remind_store.list_due(now, limit) {
        Ok(due) => due,
        Err(error) => {
            log::warn!("[memory::remind_tick] failed to list due reminders: {error}");
            return;
        }
    };
    for reminder in due {
        let loc = resolve_locale();
        let prefix = crate::i18n::tr(crate::i18n::Message::RemindPrefix, loc);
        let content = format!("{}{}", prefix, reminder.context);
        let Ok(msg) = PcMsg::new_inbound_with_ingress(
            reminder.channel.as_str(),
            reminder.chat_id.as_str(),
            content,
            false,
            crate::bus::IngressKind::System,
        ) else {
            log::warn!(
                "[memory::remind_tick] failed to build system inbound for reminder {}",
                reminder.id
            );
            continue;
        };
        match inbound_tx.try_send(msg) {
            Ok(()) => match remind_store.delete_due(&reminder) {
                Ok(true) => {
                    if let Err(error) = cleanup(&reminder) {
                        log::warn!(
                                "[memory::remind_tick] linked calendar cleanup failed for reminder {}: {}",
                                reminder.id,
                                error
                            );
                    }
                }
                Ok(false) => {}
                Err(error) => {
                    log::warn!(
                        "[memory::remind_tick] failed to delete delivered reminder {}: {}",
                        reminder.id,
                        error
                    );
                }
            },
            Err(std::sync::mpsc::TrySendError::Full(_))
            | Err(std::sync::mpsc::TrySendError::Disconnected(_)) => {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::build_system_prompt;
    use crate::bus::new_inbound_channel;
    use crate::error::Result;
    use crate::memory::RemindAtStore;
    use crate::reminder::ReminderItem;
    use std::sync::Mutex;

    #[test]
    fn build_system_prompt_respects_max_len() {
        let memory = "Memory";
        let notes = vec!["Note1".to_string(), "Note2".to_string()];
        let out = build_system_prompt(memory, &notes, 20);
        assert!(out.len() <= 20);
    }

    #[test]
    fn build_system_prompt_order() {
        let out = build_system_prompt("Memory", &["Note".to_string()], 100);
        assert!(out.starts_with("Memory"));
        assert!(out.contains("Note"));
    }

    #[derive(Default)]
    struct StubRemindAtStore {
        items: Mutex<Vec<ReminderItem>>,
    }

    impl RemindAtStore for StubRemindAtStore {
        fn get(&self, _channel: &str, _chat_id: &str, _id: &str) -> Result<Option<ReminderItem>> {
            Ok(None)
        }

        fn upsert(&self, reminder: &ReminderItem) -> Result<()> {
            let mut items = self.items.lock().expect("items lock");
            items.push(reminder.clone());
            items.sort_by(|left, right| {
                left.at_unix_secs
                    .cmp(&right.at_unix_secs)
                    .then_with(|| left.id.cmp(&right.id))
            });
            Ok(())
        }

        fn delete(&self, channel: &str, chat_id: &str, id: &str) -> Result<bool> {
            let mut items = self.items.lock().expect("items lock");
            let Some(idx) = items.iter().position(|item| {
                item.channel == channel && item.chat_id == chat_id && item.id == id
            }) else {
                return Ok(false);
            };
            items.remove(idx);
            Ok(true)
        }

        fn list_due(&self, now_unix_secs: u64, limit: usize) -> Result<Vec<ReminderItem>> {
            Ok(self
                .items
                .lock()
                .expect("items lock")
                .iter()
                .filter(|item| item.at_unix_secs <= now_unix_secs)
                .take(limit)
                .cloned()
                .collect())
        }

        fn delete_due(&self, reminder: &ReminderItem) -> Result<bool> {
            let mut items = self.items.lock().expect("items lock");
            let Some(idx) = items.iter().position(|item| item == reminder) else {
                return Ok(false);
            };
            items.remove(idx);
            Ok(true)
        }

        fn list_upcoming(
            &self,
            _channel: &str,
            _chat_id: &str,
            _now_unix_secs: u64,
            _limit: usize,
        ) -> Result<Vec<ReminderItem>> {
            Ok(Vec::new())
        }
    }

    impl StubRemindAtStore {
        fn new(items: Vec<ReminderItem>) -> Self {
            Self {
                items: Mutex::new(items),
            }
        }

        fn ids(&self) -> Vec<String> {
            self.items
                .lock()
                .expect("items lock")
                .iter()
                .map(|item| item.id.clone())
                .collect()
        }
    }

    fn reminder(id: &str) -> ReminderItem {
        ReminderItem {
            id: id.to_string(),
            channel: "qq_channel".to_string(),
            chat_id: "chat-1".to_string(),
            at_unix_secs: 1,
            context: format!("reminder-{id}"),
            ..ReminderItem::default()
        }
    }

    #[test]
    fn remind_tick_runs_cleanup_then_injects_message() {
        let store = StubRemindAtStore::new(vec![ReminderItem {
            id: "rem-1".to_string(),
            channel: "qq_channel".to_string(),
            chat_id: "chat-1".to_string(),
            at_unix_secs: 1,
            context: "喝水".to_string(),
            ..ReminderItem::default()
        }]);
        let cleaned = Mutex::new(Vec::new());
        let (tx, rx, _) = new_inbound_channel(4);
        let resolve_locale: std::sync::Arc<dyn Fn() -> crate::i18n::Locale + Send + Sync> =
            std::sync::Arc::new(|| crate::i18n::Locale::Zh);

        super::remind_tick(
            &store,
            |reminder| {
                cleaned
                    .lock()
                    .expect("cleaned lock")
                    .push(reminder.id.clone());
                Ok(())
            },
            &tx,
            &resolve_locale,
        );

        assert_eq!(
            cleaned.lock().expect("cleaned lock").as_slice(),
            &["rem-1".to_string()]
        );
        let msg = rx.recv().expect("message");
        assert!(msg.content.contains("喝水"));
        assert_eq!(msg.ingress, crate::bus::IngressKind::System);
    }

    #[test]
    fn remind_tick_processes_at_most_four_due_items_per_tick() {
        let store =
            StubRemindAtStore::new((0..5).map(|idx| reminder(&format!("rem-{idx}"))).collect());
        let cleaned = Mutex::new(Vec::new());
        let (tx, rx, _) = new_inbound_channel(16);
        let resolve_locale: std::sync::Arc<dyn Fn() -> crate::i18n::Locale + Send + Sync> =
            std::sync::Arc::new(|| crate::i18n::Locale::Zh);

        super::remind_tick(
            &store,
            |reminder| {
                cleaned
                    .lock()
                    .expect("cleaned lock")
                    .push(reminder.id.clone());
                Ok(())
            },
            &tx,
            &resolve_locale,
        );

        let delivered = std::iter::from_fn(|| rx.try_recv().ok()).collect::<Vec<_>>();
        assert_eq!(delivered.len(), 4);
        assert_eq!(
            cleaned.lock().expect("cleaned lock").as_slice(),
            &[
                "rem-0".to_string(),
                "rem-1".to_string(),
                "rem-2".to_string(),
                "rem-3".to_string()
            ]
        );
        assert_eq!(store.ids(), vec!["rem-4".to_string()]);
    }

    #[test]
    fn remind_tick_requeues_when_system_inbound_is_disconnected() {
        let store = StubRemindAtStore::new(vec![reminder("rem-1")]);
        let cleaned = Mutex::new(Vec::new());
        let (tx, rx, _) = new_inbound_channel(1);
        drop(rx);
        let resolve_locale: std::sync::Arc<dyn Fn() -> crate::i18n::Locale + Send + Sync> =
            std::sync::Arc::new(|| crate::i18n::Locale::Zh);

        super::remind_tick(
            &store,
            |reminder| {
                cleaned
                    .lock()
                    .expect("cleaned lock")
                    .push(reminder.id.clone());
                Ok(())
            },
            &tx,
            &resolve_locale,
        );

        assert!(cleaned.lock().expect("cleaned lock").is_empty());
        assert_eq!(store.ids(), vec!["rem-1".to_string()]);
    }
}
