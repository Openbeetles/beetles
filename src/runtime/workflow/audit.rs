use super::{WorkflowAuditRecord, WorkflowDisposition};
use serde::Serialize;
use std::collections::VecDeque;
use std::sync::{Mutex, OnceLock};

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
const WORKFLOW_AUDIT_CAPACITY: usize = 32;
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
const WORKFLOW_AUDIT_CAPACITY: usize = 128;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct WorkflowAuditSummary {
    pub total_retained: usize,
    pub executed: usize,
    pub deferred: usize,
    pub suppressed: usize,
    pub canceled: usize,
    pub no_trigger: usize,
    pub failed: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct WorkflowAuditSnapshot {
    pub summary: WorkflowAuditSummary,
    pub recent_records: Vec<WorkflowAuditRecord>,
}

fn workflow_audit_state() -> &'static Mutex<VecDeque<WorkflowAuditRecord>> {
    static STATE: OnceLock<Mutex<VecDeque<WorkflowAuditRecord>>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(VecDeque::with_capacity(WORKFLOW_AUDIT_CAPACITY)))
}

pub fn append_workflow_audit(record: WorkflowAuditRecord) {
    let mut state = workflow_audit_state()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if state.len() >= WORKFLOW_AUDIT_CAPACITY {
        state.pop_front();
    }
    state.push_back(record);
}

pub fn recent_workflow_audits(limit: usize) -> Vec<WorkflowAuditRecord> {
    let state = workflow_audit_state()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    state
        .iter()
        .rev()
        .take(limit)
        .cloned()
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect()
}

pub fn workflow_audit_snapshot(limit: usize) -> WorkflowAuditSnapshot {
    let recent_records = recent_workflow_audits(limit);
    let mut summary = WorkflowAuditSummary {
        total_retained: recent_records.len(),
        ..WorkflowAuditSummary::default()
    };
    for record in &recent_records {
        match record.disposition {
            WorkflowDisposition::ExecuteNow => summary.executed += 1,
            WorkflowDisposition::DeferUntil => summary.deferred += 1,
            WorkflowDisposition::Suppress => summary.suppressed += 1,
            WorkflowDisposition::Cancel => summary.canceled += 1,
            WorkflowDisposition::NoTrigger => summary.no_trigger += 1,
            WorkflowDisposition::ExecuteFailed => summary.failed += 1,
        }
    }
    WorkflowAuditSnapshot {
        summary,
        recent_records,
    }
}

#[cfg(test)]
pub fn reset_workflow_audit_for_tests() {
    workflow_audit_state()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clear();
}

#[cfg(test)]
pub fn workflow_audit_test_guard() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::workflow::{
        WorkflowEffect, WorkflowKind, WorkflowRecoveryPolicy, WorkflowTrigger,
    };

    #[test]
    fn workflow_audit_snapshot_counts_dispositions() {
        let _guard = workflow_audit_test_guard();
        reset_workflow_audit_for_tests();
        append_workflow_audit(WorkflowAuditRecord::new(
            WorkflowKind::UpcomingReminderNudge,
            WorkflowTrigger::CronTick,
            WorkflowDisposition::ExecuteNow,
            WorkflowEffect::EnqueueSystemJob,
            WorkflowRecoveryPolicy::DropOnModeExit,
            "executed",
            100,
        ));
        append_workflow_audit(WorkflowAuditRecord::new(
            WorkflowKind::InitiativeTick,
            WorkflowTrigger::CronTick,
            WorkflowDisposition::Suppress,
            WorkflowEffect::Noop,
            WorkflowRecoveryPolicy::DropOnModeExit,
            "suppressed",
            101,
        ));
        append_workflow_audit(WorkflowAuditRecord::new(
            WorkflowKind::InitiativeTick,
            WorkflowTrigger::CronTick,
            WorkflowDisposition::NoTrigger,
            WorkflowEffect::Noop,
            WorkflowRecoveryPolicy::DropOnModeExit,
            "no_trigger",
            102,
        ));

        let snapshot = workflow_audit_snapshot(8);
        assert_eq!(snapshot.summary.total_retained, 3);
        assert_eq!(snapshot.summary.executed, 1);
        assert_eq!(snapshot.summary.suppressed, 1);
        assert_eq!(snapshot.summary.no_trigger, 1);
    }

    #[test]
    fn workflow_audit_keeps_recent_records_bounded() {
        let _guard = workflow_audit_test_guard();
        reset_workflow_audit_for_tests();
        for idx in 0..(WORKFLOW_AUDIT_CAPACITY + 4) {
            append_workflow_audit(WorkflowAuditRecord::new(
                WorkflowKind::InitiativeTick,
                WorkflowTrigger::CronTick,
                WorkflowDisposition::NoTrigger,
                WorkflowEffect::Noop,
                WorkflowRecoveryPolicy::DropOnModeExit,
                format!("record-{idx}"),
                idx as u64,
            ));
        }

        let records = recent_workflow_audits(WORKFLOW_AUDIT_CAPACITY + 8);
        assert_eq!(records.len(), WORKFLOW_AUDIT_CAPACITY);
        assert_eq!(records.first().map(|item| item.happened_at), Some(4));
        let expected = format!("record-{}", WORKFLOW_AUDIT_CAPACITY + 3);
        assert_eq!(
            records.last().map(|item| item.rationale.as_str()),
            Some(expected.as_str())
        );
    }
}
