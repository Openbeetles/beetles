//! Linux programmable reasoning constitution and operator-visible contracts.

mod constitution;
mod lua_runner;
mod memory_query;
mod operator;
mod proposal;
mod runtime;

pub use constitution::{
    programmable_reasoning_capability_taxonomy, programmable_reasoning_runtime_contract,
    ProgrammableReasoningCapabilityContract, ProgrammableReasoningCapabilityKind,
    ProgrammableReasoningExecutionBackend, ProgrammableReasoningRuntimeContract,
    ProgrammableReasoningStage,
};
pub use lua_runner::{execute_lua_query, run_reasoning_runner_stdio};
pub use memory_query::{
    default_lua_memory_query_capabilities, validate_memory_query_result,
    MemoryQueryCandidate, MemoryQueryCandidateKind, MemoryQueryContinuityRecord,
    MemoryQueryContinuityScope, MemoryQueryGroup, MemoryQueryLongTermRecord,
    MemoryQueryResult, MemoryQuerySelection, MemoryQuerySnapshot, MemoryQuerySnapshotCounts,
    MEMORY_QUERY_DEFAULT_CONTINUITY_LIMIT, MEMORY_QUERY_DEFAULT_LONG_TERM_LIMIT,
    MEMORY_QUERY_MAX_CONTINUITY_LIMIT, MEMORY_QUERY_MAX_LONG_TERM_LIMIT,
};
pub use operator::{
    programmable_reasoning_operator_snapshot, programmable_reasoning_system_info_summary,
    ProgrammableReasoningOperatorSnapshot, ProgrammableReasoningSystemInfoSummary,
};
pub use proposal::{
    programmable_reasoning_proposal_kinds, ProgrammableReasoningProposal,
    ProgrammableReasoningProposalKind, ProgrammableReasoningProposalScope,
};
pub use runtime::{
    default_lua_query_capabilities, CurrentExecutableLuaSandboxExecutor,
    DirectLuaSandboxExecutor, LuaQueryBudget, LuaQueryRequest, LuaQueryResponse,
    ReasoningExecutor, SubprocessLuaSandboxExecutor,
};
