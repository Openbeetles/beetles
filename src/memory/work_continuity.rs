//! Deterministic foreground work continuity projection.
//! 前台工作连续性投影：从现有正式资产汇总“做到哪、为什么停、下一步是什么”。

use crate::task_execution::{current_or_next_step, TaskRunRecord, TaskRunStatus, TaskStepStatus};
use crate::util::truncate_content_to_max;
use serde::{Deserialize, Serialize};

use super::{ExecutionState, ExecutionStatus};

pub const MAX_WORK_CONTINUITY_BLOCK_LEN: usize = 560;

const MAX_WORK_CONTINUITY_FOCUS_CHARS: usize = 120;
const MAX_WORK_CONTINUITY_FIELD_CHARS: usize = 200;
const MAX_WORK_CONTINUITY_ARTIFACT_REF_CHARS: usize = 96;
const MAX_WORK_CONTINUITY_ARTIFACT_REFS: usize = 4;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkContinuityRecord {
    #[serde(default)]
    pub focus: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub progress_summary: String,
    #[serde(default)]
    pub blocker: String,
    #[serde(default)]
    pub next_action: String,
    #[serde(default)]
    pub recent_outcome: String,
    #[serde(default)]
    pub active_artifact_refs: Vec<String>,
    #[serde(default)]
    pub updated_at: u64,
}

impl WorkContinuityRecord {
    pub fn is_meaningful(&self) -> bool {
        !self.focus.trim().is_empty()
            && (!self.progress_summary.trim().is_empty()
                || !self.blocker.trim().is_empty()
                || !self.next_action.trim().is_empty()
                || !self.recent_outcome.trim().is_empty()
                || !self.active_artifact_refs.is_empty())
    }
}

pub fn build_work_continuity_record(
    active_task_run: Option<&TaskRunRecord>,
    execution_state: Option<&ExecutionState>,
    summary_text: Option<&str>,
) -> Option<WorkContinuityRecord> {
    let mut record = active_task_run
        .map(work_continuity_from_task_run)
        .or_else(|| execution_state.map(work_continuity_from_execution_state))
        .unwrap_or_default();

    if let Some(state) = execution_state {
        merge_execution_state_into_work_continuity(&mut record, state);
    }
    if record.progress_summary.trim().is_empty() {
        record.progress_summary = normalize_field(summary_text, MAX_WORK_CONTINUITY_FIELD_CHARS);
    }
    if record.updated_at == 0 {
        record.updated_at = execution_state.map(|state| state.updated_at).unwrap_or(0);
    }

    record.is_meaningful().then_some(record)
}

pub fn render_work_continuity_block(
    record: &WorkContinuityRecord,
    max_len: usize,
) -> Option<String> {
    if !record.is_meaningful() {
        return None;
    }
    let mut out = String::with_capacity(max_len.min(MAX_WORK_CONTINUITY_BLOCK_LEN));
    out.push_str("## Work Continuity\n");
    out.push_str("Focus: ");
    out.push_str(record.focus.trim());
    out.push('\n');
    if !record.status.trim().is_empty() {
        out.push_str("Status: ");
        out.push_str(record.status.trim());
        out.push('\n');
    }
    if !record.progress_summary.trim().is_empty() {
        out.push_str("Progress: ");
        out.push_str(record.progress_summary.trim());
        out.push('\n');
    }
    if !record.blocker.trim().is_empty() {
        out.push_str("Blocker: ");
        out.push_str(record.blocker.trim());
        out.push('\n');
    }
    if !record.next_action.trim().is_empty() {
        out.push_str("Next: ");
        out.push_str(record.next_action.trim());
        out.push('\n');
    }
    if !record.recent_outcome.trim().is_empty() {
        out.push_str("Recent outcome: ");
        out.push_str(record.recent_outcome.trim());
        out.push('\n');
    }
    if !record.active_artifact_refs.is_empty() {
        out.push_str("Artifacts: ");
        out.push_str(&record.active_artifact_refs.join(" | "));
        out.push('\n');
    }
    let rendered = truncate_content_to_max(out.trim_end(), max_len).into_owned();
    (!rendered.trim().is_empty()).then_some(rendered)
}

fn work_continuity_from_task_run(record: &TaskRunRecord) -> WorkContinuityRecord {
    let step = current_or_next_step(record);
    let focus = first_non_empty([
        Some(record.run.title.as_str()),
        Some(record.plan.goal.as_str()),
        Some(record.run.user_request.as_str()),
    ]);
    let progress_summary = first_non_empty([
        step.map(|step| step.last_result_summary.as_str()),
        step.and_then(task_step_progress_fallback),
    ]);
    let blocker = first_non_empty([
        Some(record.run.failure_reason.as_str()),
        step.and_then(task_step_blocker_fallback),
    ]);
    let next_action = first_non_empty([
        step.map(|step| step.instruction.as_str()),
        step.map(|step| step.title.as_str()),
    ]);
    let recent_outcome = first_non_empty([
        Some(record.run.final_summary.as_str()),
        step.and_then(task_step_recent_outcome_fallback),
    ]);
    let active_artifact_refs = step
        .map(|step| normalize_artifact_refs(&step.expected_artifacts))
        .unwrap_or_default();

    WorkContinuityRecord {
        focus: normalize_field(focus.as_deref(), MAX_WORK_CONTINUITY_FOCUS_CHARS),
        status: task_run_status_label(record.run.status).to_string(),
        progress_summary: normalize_field(
            progress_summary.as_deref(),
            MAX_WORK_CONTINUITY_FIELD_CHARS,
        ),
        blocker: normalize_field(blocker.as_deref(), MAX_WORK_CONTINUITY_FIELD_CHARS),
        next_action: normalize_field(next_action.as_deref(), MAX_WORK_CONTINUITY_FIELD_CHARS),
        recent_outcome: normalize_field(recent_outcome.as_deref(), MAX_WORK_CONTINUITY_FIELD_CHARS),
        active_artifact_refs,
        updated_at: record.run.updated_at,
    }
}

fn work_continuity_from_execution_state(state: &ExecutionState) -> WorkContinuityRecord {
    WorkContinuityRecord {
        focus: normalize_field(Some(state.goal.as_str()), MAX_WORK_CONTINUITY_FOCUS_CHARS),
        status: execution_status_label(state.status).to_string(),
        progress_summary: normalize_field(
            Some(state.progress.as_str()),
            MAX_WORK_CONTINUITY_FIELD_CHARS,
        ),
        blocker: normalize_field(
            Some(state.blocker.as_str()),
            MAX_WORK_CONTINUITY_FIELD_CHARS,
        ),
        next_action: normalize_field(
            Some(state.next_action.as_str()),
            MAX_WORK_CONTINUITY_FIELD_CHARS,
        ),
        recent_outcome: normalize_field(
            Some(state.last_output.as_str()),
            MAX_WORK_CONTINUITY_FIELD_CHARS,
        ),
        active_artifact_refs: Vec::new(),
        updated_at: state.updated_at,
    }
}

fn merge_execution_state_into_work_continuity(
    record: &mut WorkContinuityRecord,
    state: &ExecutionState,
) {
    fill_if_empty(
        &mut record.focus,
        Some(state.goal.as_str()),
        MAX_WORK_CONTINUITY_FOCUS_CHARS,
    );
    if record.status.trim().is_empty() {
        record.status = execution_status_label(state.status).to_string();
    }
    fill_if_empty(
        &mut record.progress_summary,
        Some(state.progress.as_str()),
        MAX_WORK_CONTINUITY_FIELD_CHARS,
    );
    fill_if_empty(
        &mut record.blocker,
        Some(state.blocker.as_str()),
        MAX_WORK_CONTINUITY_FIELD_CHARS,
    );
    let live_next_action = normalize_field(
        Some(state.next_action.as_str()),
        MAX_WORK_CONTINUITY_FIELD_CHARS,
    );
    if !live_next_action.is_empty() {
        record.next_action = live_next_action;
    }
    let live_recent_outcome = normalize_field(
        Some(state.last_output.as_str()),
        MAX_WORK_CONTINUITY_FIELD_CHARS,
    );
    if !live_recent_outcome.is_empty() {
        record.recent_outcome = live_recent_outcome;
    }
    record.updated_at = record.updated_at.max(state.updated_at);
}

fn task_step_progress_fallback(step: &crate::task_execution::TaskStep) -> Option<&str> {
    if !step.last_review_summary.trim().is_empty() {
        Some(step.last_review_summary.as_str())
    } else {
        None
    }
}

fn task_step_blocker_fallback(step: &crate::task_execution::TaskStep) -> Option<&str> {
    if step.status == TaskStepStatus::Blocked {
        if !step.last_review_summary.trim().is_empty() {
            Some(step.last_review_summary.as_str())
        } else if !step.title.trim().is_empty() {
            Some(step.title.as_str())
        } else {
            None
        }
    } else {
        None
    }
}

fn task_step_recent_outcome_fallback(step: &crate::task_execution::TaskStep) -> Option<&str> {
    if !step.last_result_summary.trim().is_empty() {
        Some(step.last_result_summary.as_str())
    } else if step.status.is_terminal() && !step.last_review_summary.trim().is_empty() {
        Some(step.last_review_summary.as_str())
    } else {
        None
    }
}

fn normalize_artifact_refs(items: &[String]) -> Vec<String> {
    items
        .iter()
        .map(|item| normalize_field(Some(item.as_str()), MAX_WORK_CONTINUITY_ARTIFACT_REF_CHARS))
        .filter(|item| !item.is_empty())
        .take(MAX_WORK_CONTINUITY_ARTIFACT_REFS)
        .collect()
}

fn fill_if_empty(target: &mut String, candidate: Option<&str>, max_len: usize) {
    if target.trim().is_empty() {
        *target = normalize_field(candidate, max_len);
    }
}

fn first_non_empty<const N: usize>(values: [Option<&str>; N]) -> Option<String> {
    values
        .into_iter()
        .flatten()
        .map(str::trim)
        .find(|value| !value.is_empty())
        .map(str::to_string)
}

fn normalize_field(value: Option<&str>, max_len: usize) -> String {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| truncate_content_to_max(value, max_len).into_owned())
        .unwrap_or_default()
}

fn task_run_status_label(status: TaskRunStatus) -> &'static str {
    match status {
        TaskRunStatus::Planning | TaskRunStatus::Running => "active",
        TaskRunStatus::Blocked => "blocked",
        TaskRunStatus::Completed | TaskRunStatus::PartialComplete => "done",
        TaskRunStatus::Failed | TaskRunStatus::Aborted => "failed",
    }
}

fn execution_status_label(status: ExecutionStatus) -> &'static str {
    match status {
        ExecutionStatus::Active => "active",
        ExecutionStatus::Blocked => "blocked",
        ExecutionStatus::Done => "done",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task_execution::{
        TaskPlan, TaskRun, TaskRunKind, TaskRunRecord, TaskRunStatus, TaskStep, TaskStepStatus,
    };

    fn sample_task_run_record() -> TaskRunRecord {
        TaskRunRecord {
            run: TaskRun {
                run_id: "run1".to_string(),
                kind: TaskRunKind::InteractiveAction,
                source_channel: "qq_channel".to_string(),
                source_chat_id: "chat-1".to_string(),
                user_request: "继续配置邮箱".to_string(),
                title: "配置 QQ 邮箱".to_string(),
                status: TaskRunStatus::Running,
                current_step_id: "s01".to_string(),
                planner_reason: String::new(),
                final_summary: String::new(),
                failure_reason: String::new(),
                plan_revision: 1,
                created_at: 1,
                updated_at: 8,
                finished_at: 0,
            },
            plan: TaskPlan {
                goal: "配置 QQ 邮箱".to_string(),
                completion_definition: "邮箱可正常收发".to_string(),
                risk_notes: Vec::new(),
                ordered_steps: vec![TaskStep {
                    step_id: "s01".to_string(),
                    title: "补齐当前账号配置".to_string(),
                    instruction: "继续写入缺失的邮箱参数".to_string(),
                    status: TaskStepStatus::Running,
                    tool_budget: 1,
                    retry_budget: 0,
                    expected_artifacts: vec!["office.account.qq".to_string()],
                    review_criteria: Vec::new(),
                    attempt_count: 1,
                    last_result_summary: "已创建账户草案".to_string(),
                    last_review_summary: String::new(),
                    started_at: 2,
                    finished_at: 0,
                }],
            },
        }
    }

    #[test]
    fn work_continuity_prefers_formal_task_state_then_fills_from_execution_state() {
        let state = ExecutionState {
            status: ExecutionStatus::Blocked,
            goal: "配置 QQ 邮箱".to_string(),
            progress: "等待用户确认授权码".to_string(),
            blocker: "缺少授权码".to_string(),
            next_action: "让用户发送授权码".to_string(),
            last_output: "账户草案已创建".to_string(),
            updated_at: 12,
            ..ExecutionState::default()
        };
        let record = build_work_continuity_record(
            Some(&sample_task_run_record()),
            Some(&state),
            Some("会话摘要不应覆盖已有工作态"),
        )
        .expect("work continuity");

        assert_eq!(record.focus, "配置 QQ 邮箱");
        assert_eq!(record.status, "active");
        assert_eq!(record.progress_summary, "已创建账户草案");
        assert_eq!(record.blocker, "缺少授权码");
        assert_eq!(record.next_action, "让用户发送授权码");
        assert_eq!(record.recent_outcome, "账户草案已创建");
        assert_eq!(record.active_artifact_refs, vec!["office.account.qq"]);
        assert_eq!(record.updated_at, 12);
    }

    #[test]
    fn summary_only_does_not_create_fake_work_continuity() {
        let record = build_work_continuity_record(
            None,
            None,
            Some("这里只是会话摘要，不能被冒充成当前工作态"),
        );

        assert!(record.is_none());
    }
}
