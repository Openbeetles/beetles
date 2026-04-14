//! Linux programmable reasoning constitution and operator-visible contracts.

mod constitution;
mod engineering_distillation;
mod experience_crystal;
mod idle_forge;
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
mod lua_runner;
mod memory_attack;
mod memory_query;
mod operator;
mod proposal;
mod protocol_frame;
mod register_table;
mod runtime;
mod state_machine;
mod tool_request;

pub use constitution::{
    programmable_reasoning_capability_taxonomy, programmable_reasoning_runtime_contract,
    ProgrammableReasoningCapabilityContract, ProgrammableReasoningCapabilityKind,
    ProgrammableReasoningExecutionBackend, ProgrammableReasoningRuntimeContract,
    ProgrammableReasoningStage,
};
pub use engineering_distillation::{
    validate_engineering_distillation_result, EngineeringDistillationAssetCandidate,
    EngineeringDistillationAssetKind, EngineeringDistillationResult,
};
pub use experience_crystal::{
    build_experience_crystal_operator_summary, promote_skill_crystal_candidates,
    skill_crystal_candidate_to_runtime_skill_write, validate_skill_crystal_result,
    ExperienceCrystalOperatorSummary, SkillCrystalCandidate, SkillCrystalResult,
};
pub(crate) use idle_forge::{enqueue_idle_memory_forge_tick, run_idle_memory_forge_background_job};
pub use idle_forge::{
    idle_memory_forge_job_contracts, load_idle_memory_forge_operator_summary,
    persist_idle_memory_forge_run, should_run_idle_memory_forge, IdleMemoryForgeAdjudicationState,
    IdleMemoryForgeAdmissionSnapshot, IdleMemoryForgeAttackBatch, IdleMemoryForgeJobContract,
    IdleMemoryForgeJobKind, IdleMemoryForgeJobReport, IdleMemoryForgeJobStatus,
    IdleMemoryForgeOperatorSummary, IdleMemoryForgeProposalBatch, IdleMemoryForgeRunLedger,
    IdleMemoryForgeTrigger,
};
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub use lua_runner::{execute_lua_query, run_reasoning_runner_stdio};
pub use memory_attack::{
    memory_attack_job_contracts, validate_memory_attack_result, MemoryAttackFinding,
    MemoryAttackFindingKind, MemoryAttackJobContract, MemoryAttackJobKind, MemoryAttackJobReport,
    MemoryAttackJobStatus, MemoryAttackResult, MemoryDistillationCandidate,
};
pub use memory_query::{
    build_memory_query_snapshot_from_stores, default_lua_memory_query_capabilities,
    validate_memory_query_result, MemoryQueryCandidate, MemoryQueryCandidateKind,
    MemoryQueryContinuityRecord, MemoryQueryContinuityScope, MemoryQueryGroup,
    MemoryQueryLongTermRecord, MemoryQueryResult, MemoryQuerySelection, MemoryQuerySnapshot,
    MemoryQuerySnapshotCounts, MEMORY_QUERY_DEFAULT_CONTINUITY_LIMIT,
    MEMORY_QUERY_DEFAULT_LONG_TERM_LIMIT, MEMORY_QUERY_MAX_CONTINUITY_LIMIT,
    MEMORY_QUERY_MAX_LONG_TERM_LIMIT,
};
pub use operator::{
    programmable_reasoning_operator_snapshot, programmable_reasoning_system_info_summary,
    ProgrammableReasoningMaintenanceDigest, ProgrammableReasoningOperatorSnapshot,
    ProgrammableReasoningSystemInfoSummary, ProgrammableReasoningTimeline,
    ProgrammableReasoningTimelineEvent, ProgrammableReasoningToolUsageSummary,
    ProgrammableReasoningUsageAnalytics,
};
pub use proposal::{
    programmable_reasoning_proposal_kinds, ProgrammableReasoningProposal,
    ProgrammableReasoningProposalKind, ProgrammableReasoningProposalScope,
};
pub use protocol_frame::{
    validate_protocol_frame_result, ProtocolFieldEncoding, ProtocolFrameByteRange,
    ProtocolFrameDirection, ProtocolFrameEntry, ProtocolFrameField, ProtocolFrameResult,
};
pub use register_table::{
    validate_register_table_result, RegisterFieldAccess, RegisterTableBitRange, RegisterTableEntry,
    RegisterTableField, RegisterTableResult,
};
pub use runtime::{
    default_lua_query_capabilities, CurrentExecutableLuaSandboxExecutor, DirectLuaSandboxExecutor,
    LuaQueryBudget, LuaQueryRequest, LuaQueryResponse, ReasoningExecutor,
    SubprocessLuaSandboxExecutor,
};
pub use state_machine::{
    validate_state_machine_result, StateMachineFinding, StateMachineFindingKind, StateMachineModel,
    StateMachineResult, StateMachineState, StateMachineTransition, StateNodeRole,
};
pub use tool_request::{validate_tool_request_result, ToolRequestProposal, ToolRequestResult};

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
pub fn execute_lua_query(request: &LuaQueryRequest) -> crate::Result<LuaQueryResponse> {
    Ok(LuaQueryResponse::failure(
        "unsupported_platform",
        "programmable reasoning lua sandbox is unavailable on esp targets",
        request.budget.clone(),
    ))
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
pub fn run_reasoning_runner_stdio() -> crate::Result<()> {
    Err(crate::Error::config(
        "reasoning_runner",
        "programmable reasoning runner is unavailable on esp targets",
    ))
}
