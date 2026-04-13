//! Static runtime workflow contract.
//! 轻量 runtime workflow 合同：统一类型、审计与后续 admission/runner 基座。

mod audit;
mod types;

#[cfg(test)]
pub use audit::reset_workflow_audit_for_tests;
#[cfg(test)]
pub use audit::workflow_audit_test_guard;
pub use audit::{
    append_workflow_audit, recent_workflow_audits, workflow_audit_snapshot, WorkflowAuditSnapshot,
    WorkflowAuditSummary,
};
pub use types::{
    WorkflowAdmissionSnapshot, WorkflowAuditRecord, WorkflowDisposition, WorkflowEffect,
    WorkflowKind, WorkflowRecoveryPolicy, WorkflowTrigger,
};
