//! Linux programmable reasoning constitution and operator-visible contracts.

mod constitution;
mod lua_runner;
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
