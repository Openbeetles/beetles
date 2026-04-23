//! Formal task-execution workspace plane for complex work.
//! 复杂任务执行工作区平面：正式 run / plan / step / artifact / ledger 合同。

mod learning;

use crate::error::{Error, Result};
use crate::util::truncate_content_to_max;
use serde::{de, Deserialize, Deserializer, Serialize, Serializer};
use std::cmp::Reverse;
use std::fmt;

pub(crate) use learning::retrieve_task_learning_hits_with_backend;
pub use learning::{
    build_task_learning_operator_snapshot, build_task_learning_records, build_task_recall_bundle,
    inspect_task_learning, inspect_task_workspace, normalize_task_learning_artifact_ids,
    normalize_task_learning_drafts, render_task_learning_inspection_markdown,
    render_task_learning_operator_text, render_task_workspace_inspection_markdown,
    retrieve_task_learning_hits, run_task_learning_maintenance, TaskLearningCandidateState,
    TaskLearningDraft, TaskLearningHit, TaskLearningInspection, TaskLearningInspectionHit,
    TaskLearningKind, TaskLearningMaintenanceContext, TaskLearningMaintenanceInput,
    TaskLearningMaintenanceOutcome, TaskLearningOperatorRecord, TaskLearningOperatorSnapshot,
    TaskLearningRecallBackend, TaskLearningRecord, TaskLearningRoute, TaskLearningRouteCounts,
    TaskLearningScoreBreakdown, TaskLearningStore, TaskWorkspaceInspection, REL_DIR_TASK_LEARNING,
};

pub const REL_DIR_TASK_RUNS: &str = "memory/task_runs";
pub const REL_DIR_TASK_ARTIFACTS: &str = "memory/task_artifacts";
pub const REL_DIR_TASK_EXECUTION_LEDGER: &str = "memory/task_execution_ledger";

pub const MAX_TASK_RUN_ID_CHARS: usize = 16;
pub const MAX_TASK_ARTIFACT_ID_CHARS: usize = 12;
pub const MAX_TASK_STEP_ID_CHARS: usize = 12;
pub const MAX_TASK_TITLE_CHARS: usize = 96;
pub const MAX_TASK_REASON_CHARS: usize = 240;
pub const MAX_TASK_GOAL_CHARS: usize = 240;
pub const MAX_TASK_COMPLETION_DEFINITION_CHARS: usize = 320;
pub const MAX_TASK_STEP_TITLE_CHARS: usize = 96;
pub const MAX_TASK_STEP_INSTRUCTION_CHARS: usize = 720;
pub const MAX_TASK_STEP_LIST_ITEMS: usize = 6;
pub const MAX_TASK_STEP_RETRY_BUDGET: u8 = 2;
pub const MAX_TASK_STEP_TOOL_BUDGET: u8 = 6;
pub const MAX_TASK_ARTIFACT_SUMMARY_CHARS: usize = 240;
pub const MAX_TASK_ARTIFACT_CONTENT_CHARS: usize = 4 * 1024;
pub const MAX_TASK_PROVENANCE_CHARS: usize = 240;
pub const MAX_TASK_LEDGER_MESSAGE_CHARS: usize = 320;
pub const MAX_TASK_WORKSPACE_RENDER_CHARS: usize = 1_400;
pub const MAX_TASK_OPERATOR_RECENT_RUNS: usize = 5;
pub const MAX_TASK_OPERATOR_STEP_PREVIEW: usize = 4;
pub const MAX_TASK_OPERATOR_ARTIFACT_PREVIEW: usize = 4;
pub const MAX_TASK_CLARIFICATION_FIELDS: usize = 4;
pub const MAX_TASK_CLARIFICATION_OPTIONS: usize = 8;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskRunStatus {
    Planning,
    Running,
    Completed,
    PartialComplete,
    Blocked,
    Failed,
    Aborted,
}

impl TaskRunStatus {
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Completed | Self::PartialComplete | Self::Blocked | Self::Failed | Self::Aborted
        )
    }

    pub fn is_active(self) -> bool {
        matches!(self, Self::Planning | Self::Running | Self::Blocked)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TaskRunKind;

impl TaskRunKind {
    #[allow(non_upper_case_globals)]
    pub const TaskExecution: Self = Self;
}

impl Serialize for TaskRunKind {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str("task_execution")
    }
}

impl<'de> Deserialize<'de> for TaskRunKind {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct TaskRunKindVisitor;

        impl<'de> de::Visitor<'de> for TaskRunKindVisitor {
            type Value = TaskRunKind;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("\"task_execution\"")
            }

            fn visit_unit<E>(self) -> std::result::Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(TaskRunKind::TaskExecution)
            }

            fn visit_str<E>(self, value: &str) -> std::result::Result<Self::Value, E>
            where
                E: de::Error,
            {
                match value {
                    "task_execution" => Ok(TaskRunKind::TaskExecution),
                    other => Err(E::unknown_variant(other, &["task_execution"])),
                }
            }
        }

        deserializer.deserialize_any(TaskRunKindVisitor)
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskStepStatus {
    Pending,
    Running,
    Passed,
    Retrying,
    RevisedOut,
    Skipped,
    Failed,
    Blocked,
}

impl TaskStepStatus {
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Passed | Self::RevisedOut | Self::Skipped | Self::Failed | Self::Blocked
        )
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskArtifactKind {
    Plan,
    StepResult,
    Review,
    FinalReply,
    Evidence,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskLedgerKind {
    RunCreated,
    PlanAccepted,
    PlanRevised,
    StepStarted,
    StepResultRecorded,
    StepReviewRecorded,
    RunFinished,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskReviewDecision {
    Pass,
    RetryStep,
    RevisePlan,
    AbortRun,
    PartialComplete,
    NeedsUserFacts,
    NeedsUserChoice,
    NeedsConfirmation,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskExecutionRoute {
    DirectReply,
    StartRun,
    ResumeRun,
    NeedsUserFacts,
    NeedsUserChoice,
    NeedsConfirmation,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskClarificationOption {
    pub value: String,
    pub label: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskClarificationField {
    pub key: String,
    pub label: String,
    pub description: String,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub secret: bool,
    #[serde(default)]
    pub multiple: bool,
    #[serde(default)]
    pub options: Vec<TaskClarificationOption>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskStep {
    pub step_id: String,
    pub title: String,
    pub instruction: String,
    pub status: TaskStepStatus,
    #[serde(default)]
    pub tool_budget: u8,
    #[serde(default)]
    pub retry_budget: u8,
    #[serde(default)]
    pub expected_artifacts: Vec<String>,
    #[serde(default)]
    pub review_criteria: Vec<String>,
    #[serde(default)]
    pub attempt_count: u8,
    #[serde(default)]
    pub last_result_summary: String,
    #[serde(default)]
    pub last_review_summary: String,
    #[serde(default)]
    pub started_at: u64,
    #[serde(default)]
    pub finished_at: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskPlan {
    pub goal: String,
    pub completion_definition: String,
    #[serde(default)]
    pub risk_notes: Vec<String>,
    #[serde(default)]
    pub ordered_steps: Vec<TaskStep>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskRun {
    pub run_id: String,
    #[serde(default)]
    pub kind: TaskRunKind,
    pub source_channel: String,
    pub source_chat_id: String,
    pub user_request: String,
    pub title: String,
    pub status: TaskRunStatus,
    #[serde(default)]
    pub current_step_id: String,
    #[serde(default)]
    pub planner_reason: String,
    #[serde(default)]
    pub final_summary: String,
    #[serde(default)]
    pub failure_reason: String,
    #[serde(default)]
    pub plan_revision: u32,
    #[serde(default)]
    pub created_at: u64,
    #[serde(default)]
    pub updated_at: u64,
    #[serde(default)]
    pub finished_at: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskRunRecord {
    pub run: TaskRun,
    pub plan: TaskPlan,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskArtifact {
    pub artifact_id: String,
    pub run_id: String,
    #[serde(default)]
    pub step_id: String,
    pub kind: TaskArtifactKind,
    pub summary: String,
    pub content_ref: String,
    #[serde(default)]
    pub provenance: String,
    #[serde(default)]
    pub created_at: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskArtifactRecord {
    pub artifact: TaskArtifact,
    #[serde(default)]
    pub content: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskExecutionLedgerEntry {
    pub sequence: u32,
    pub run_id: String,
    #[serde(default)]
    pub step_id: String,
    pub kind: TaskLedgerKind,
    pub run_status: TaskRunStatus,
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub recorded_at: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskPlannerStepDraft {
    pub title: String,
    pub instruction: String,
    #[serde(default)]
    pub tool_budget: u8,
    #[serde(default)]
    pub retry_budget: u8,
    #[serde(default)]
    pub expected_artifacts: Vec<String>,
    #[serde(default)]
    pub review_criteria: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskPlannerDecision {
    pub route: TaskExecutionRoute,
    #[serde(default)]
    pub reason: String,
    #[serde(default)]
    pub blocker_summary: String,
    #[serde(default)]
    pub missing_fields: Vec<String>,
    #[serde(default)]
    pub clarification_fields: Vec<TaskClarificationField>,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub goal: String,
    #[serde(default)]
    pub completion_definition: String,
    #[serde(default)]
    pub risk_notes: Vec<String>,
    #[serde(default)]
    pub steps: Vec<TaskPlannerStepDraft>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskReviewOutcome {
    pub decision: TaskReviewDecision,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub blocker_summary: String,
    #[serde(default)]
    pub missing_fields: Vec<String>,
    #[serde(default)]
    pub clarification_fields: Vec<TaskClarificationField>,
    #[serde(default)]
    pub artifact_summary: String,
    #[serde(default)]
    pub revised_steps: Vec<TaskPlannerStepDraft>,
    #[serde(default)]
    pub durable_facts: Vec<TaskLearningDraft>,
    #[serde(default)]
    pub reusable_procedures: Vec<TaskLearningDraft>,
    #[serde(default)]
    pub evidence_only: Vec<TaskLearningDraft>,
    #[serde(default)]
    pub transient_artifact_ids: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskExecutionOperatorRun {
    pub run_id: String,
    pub channel: String,
    pub chat_id: String,
    pub title: String,
    pub status: TaskRunStatus,
    pub active_step: String,
    pub completed_steps: usize,
    pub total_steps: usize,
    pub artifact_count: usize,
    pub updated_at: u64,
    #[serde(default)]
    pub failure_reason: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct TaskExecutionOperatorSnapshot {
    #[serde(default)]
    pub recent_runs: Vec<TaskExecutionOperatorRun>,
    #[serde(default)]
    pub learning: TaskLearningOperatorSnapshot,
}

pub trait TaskRunStore: Send + Sync {
    fn get(&self, run_id: &str) -> Result<Option<TaskRunRecord>>;
    fn upsert(&self, record: &TaskRunRecord) -> Result<()>;
    fn list_recent(&self, limit: usize) -> Result<Vec<TaskRunRecord>>;
    fn list_active_for_chat(
        &self,
        channel: &str,
        chat_id: &str,
        limit: usize,
    ) -> Result<Vec<TaskRunRecord>>;
}

pub trait TaskArtifactStore: Send + Sync {
    fn put(&self, record: &TaskArtifactRecord) -> Result<()>;
    fn list_for_run(&self, run_id: &str, limit: usize) -> Result<Vec<TaskArtifactRecord>>;
    fn delete(&self, _run_id: &str, _artifact_id: &str) -> Result<bool> {
        Ok(false)
    }
}

pub trait TaskExecutionLedgerStore: Send + Sync {
    fn append(&self, run_id: &str, entry: &TaskExecutionLedgerEntry) -> Result<()>;
    fn list(&self, run_id: &str, limit: usize) -> Result<Vec<TaskExecutionLedgerEntry>>;
}

pub fn normalize_task_planner_decision(
    mut decision: TaskPlannerDecision,
) -> Result<TaskPlannerDecision> {
    decision.reason = normalize_inline(&decision.reason, MAX_TASK_REASON_CHARS);
    decision.blocker_summary =
        normalize_multiline(&decision.blocker_summary, MAX_TASK_REASON_CHARS);
    normalize_string_list(
        &mut decision.missing_fields,
        MAX_TASK_CLARIFICATION_FIELDS,
        MAX_TASK_STEP_TITLE_CHARS,
    );
    normalize_task_clarification_fields(
        &mut decision.clarification_fields,
        "task_planner_clarification",
    )?;
    decision.title = normalize_inline(&decision.title, MAX_TASK_TITLE_CHARS);
    decision.goal = normalize_inline(&decision.goal, MAX_TASK_GOAL_CHARS);
    decision.completion_definition = normalize_multiline(
        &decision.completion_definition,
        MAX_TASK_COMPLETION_DEFINITION_CHARS,
    );
    normalize_string_list(
        &mut decision.risk_notes,
        MAX_TASK_STEP_LIST_ITEMS,
        MAX_TASK_REASON_CHARS,
    );
    decision.steps = decision
        .steps
        .into_iter()
        .take(MAX_TASK_STEP_LIST_ITEMS)
        .enumerate()
        .map(|(index, step)| normalize_task_planner_step(step, index))
        .collect::<Result<Vec<_>>>()?;
    match decision.route {
        TaskExecutionRoute::DirectReply => Ok(decision),
        TaskExecutionRoute::NeedsUserFacts
        | TaskExecutionRoute::NeedsUserChoice
        | TaskExecutionRoute::NeedsConfirmation => {
            if decision.blocker_summary.is_empty()
                && decision.missing_fields.is_empty()
                && decision.clarification_fields.is_empty()
            {
                return Err(Error::config(
                    "task_planner",
                    "blocker_summary, missing_fields, or clarification_fields must describe workflow blockers",
                ));
            }
            decision.title.clear();
            decision.goal.clear();
            decision.completion_definition.clear();
            decision.risk_notes.clear();
            decision.steps.clear();
            Ok(decision)
        }
        TaskExecutionRoute::StartRun | TaskExecutionRoute::ResumeRun => {
            if decision.goal.is_empty() {
                return Err(Error::config("task_planner", "goal must not be empty"));
            }
            if decision.completion_definition.is_empty() {
                return Err(Error::config(
                    "task_planner",
                    "completion_definition must not be empty",
                ));
            }
            if decision.steps.is_empty() {
                return Err(Error::config("task_planner", "steps must not be empty"));
            }
            if decision.title.is_empty() {
                decision.title =
                    truncate_content_to_max(&decision.goal, MAX_TASK_TITLE_CHARS).into_owned();
            }
            Ok(decision)
        }
    }
}

pub fn normalize_task_review_outcome(mut outcome: TaskReviewOutcome) -> Result<TaskReviewOutcome> {
    outcome.summary = normalize_multiline(&outcome.summary, MAX_TASK_REASON_CHARS);
    outcome.blocker_summary = normalize_multiline(&outcome.blocker_summary, MAX_TASK_REASON_CHARS);
    normalize_string_list(
        &mut outcome.missing_fields,
        MAX_TASK_CLARIFICATION_FIELDS,
        MAX_TASK_STEP_TITLE_CHARS,
    );
    normalize_task_clarification_fields(
        &mut outcome.clarification_fields,
        "task_review_clarification",
    )?;
    outcome.artifact_summary =
        normalize_multiline(&outcome.artifact_summary, MAX_TASK_ARTIFACT_SUMMARY_CHARS);
    outcome.revised_steps = outcome
        .revised_steps
        .into_iter()
        .take(MAX_TASK_STEP_LIST_ITEMS)
        .enumerate()
        .map(|(index, step)| normalize_task_planner_step(step, index))
        .collect::<Result<Vec<_>>>()?;
    normalize_task_learning_drafts(&mut outcome.durable_facts, "task_review")?;
    normalize_task_learning_drafts(&mut outcome.reusable_procedures, "task_review")?;
    normalize_task_learning_drafts(&mut outcome.evidence_only, "task_review")?;
    normalize_task_learning_artifact_ids(&mut outcome.transient_artifact_ids);
    if matches!(outcome.decision, TaskReviewDecision::RevisePlan)
        && outcome.revised_steps.is_empty()
    {
        return Err(Error::config(
            "task_review",
            "revised_steps must not be empty when decision=revise_plan",
        ));
    }
    if matches!(
        outcome.decision,
        TaskReviewDecision::NeedsUserFacts
            | TaskReviewDecision::NeedsUserChoice
            | TaskReviewDecision::NeedsConfirmation
    ) && outcome.blocker_summary.is_empty()
        && outcome.missing_fields.is_empty()
        && outcome.clarification_fields.is_empty()
    {
        return Err(Error::config(
            "task_review",
            "blocker_summary, missing_fields, or clarification_fields must describe workflow blockers",
        ));
    }
    Ok(outcome)
}

pub fn build_task_run_record(
    run_id: &str,
    source_channel: &str,
    source_chat_id: &str,
    user_request: &str,
    decision: &TaskPlannerDecision,
    now_secs: u64,
) -> Result<TaskRunRecord> {
    let plan = TaskPlan {
        goal: decision.goal.clone(),
        completion_definition: decision.completion_definition.clone(),
        risk_notes: decision.risk_notes.clone(),
        ordered_steps: decision
            .steps
            .iter()
            .enumerate()
            .map(|(index, step)| TaskStep {
                step_id: format!("s{:02}", index + 1),
                title: step.title.clone(),
                instruction: step.instruction.clone(),
                status: TaskStepStatus::Pending,
                tool_budget: step.tool_budget,
                retry_budget: step.retry_budget,
                expected_artifacts: step.expected_artifacts.clone(),
                review_criteria: step.review_criteria.clone(),
                attempt_count: 0,
                last_result_summary: String::new(),
                last_review_summary: String::new(),
                started_at: 0,
                finished_at: 0,
            })
            .collect(),
    };
    let current_step_id = plan
        .ordered_steps
        .first()
        .map(|step| step.step_id.clone())
        .unwrap_or_default();
    Ok(TaskRunRecord {
        run: TaskRun {
            run_id: normalize_fixed_id(run_id, MAX_TASK_RUN_ID_CHARS, "task_run_id")?,
            kind: TaskRunKind::TaskExecution,
            source_channel: normalize_inline(source_channel, 32),
            source_chat_id: normalize_inline(source_chat_id, 160),
            user_request: normalize_multiline(user_request, MAX_TASK_STEP_INSTRUCTION_CHARS),
            title: decision.title.clone(),
            status: TaskRunStatus::Planning,
            current_step_id,
            planner_reason: decision.reason.clone(),
            final_summary: String::new(),
            failure_reason: String::new(),
            plan_revision: 1,
            created_at: now_secs,
            updated_at: now_secs,
            finished_at: 0,
        },
        plan,
    })
}

pub fn finalize_foreground_task_run(
    task_run_store: &dyn TaskRunStore,
    record: &TaskRunRecord,
    status: TaskRunStatus,
    final_summary: &str,
    failure_reason: &str,
    now_secs: u64,
) -> Result<TaskRunRecord> {
    let mut next = record.clone();
    next.run.status = status;
    next.run.updated_at = now_secs;
    next.run.finished_at = now_secs;
    next.run.current_step_id.clear();
    next.run.final_summary =
        truncate_content_to_max(final_summary.trim(), MAX_TASK_ARTIFACT_CONTENT_CHARS).into_owned();
    next.run.failure_reason =
        truncate_content_to_max(failure_reason.trim(), MAX_TASK_REASON_CHARS).into_owned();
    let target_step_index = next
        .plan
        .ordered_steps
        .iter()
        .position(|step| step.step_id == record.run.current_step_id)
        .or_else(|| {
            next.plan
                .ordered_steps
                .iter()
                .position(|step| !step.status.is_terminal())
        });
    if let Some(step_index) = target_step_index {
        let step = &mut next.plan.ordered_steps[step_index];
        step.finished_at = now_secs;
        step.last_result_summary = if next.run.final_summary.is_empty() {
            step.last_result_summary.clone()
        } else {
            truncate_content_to_max(
                next.run.final_summary.trim(),
                MAX_TASK_ARTIFACT_SUMMARY_CHARS,
            )
            .into_owned()
        };
        step.last_review_summary = next.run.failure_reason.clone();
        step.status = match status {
            TaskRunStatus::Completed => TaskStepStatus::Passed,
            TaskRunStatus::Aborted | TaskRunStatus::Failed => TaskStepStatus::Failed,
            TaskRunStatus::Blocked => TaskStepStatus::Blocked,
            TaskRunStatus::PartialComplete => TaskStepStatus::Blocked,
            TaskRunStatus::Planning | TaskRunStatus::Running => step.status,
        };
    }
    task_run_store.upsert(&next)?;
    Ok(next)
}

pub fn apply_revised_remaining_steps(
    record: &mut TaskRunRecord,
    revised_steps: &[TaskPlannerStepDraft],
) -> Result<()> {
    let current_index = record
        .plan
        .ordered_steps
        .iter()
        .position(|step| step.step_id == record.run.current_step_id)
        .map(|index| index.saturating_add(1))
        .unwrap_or(record.plan.ordered_steps.len());
    record.plan.ordered_steps.truncate(current_index);
    let base_index = record.plan.ordered_steps.len();
    for (offset, step) in revised_steps.iter().enumerate() {
        record.plan.ordered_steps.push(TaskStep {
            step_id: format!("s{:02}", base_index + offset + 1),
            title: step.title.clone(),
            instruction: step.instruction.clone(),
            status: TaskStepStatus::Pending,
            tool_budget: step.tool_budget,
            retry_budget: step.retry_budget,
            expected_artifacts: step.expected_artifacts.clone(),
            review_criteria: step.review_criteria.clone(),
            attempt_count: 0,
            last_result_summary: String::new(),
            last_review_summary: String::new(),
            started_at: 0,
            finished_at: 0,
        });
    }
    record.run.plan_revision = record.run.plan_revision.saturating_add(1);
    Ok(())
}

pub fn active_task_run_for_chat(
    task_run_store: &dyn TaskRunStore,
    channel: &str,
    chat_id: &str,
) -> Result<Option<TaskRunRecord>> {
    Ok(task_run_store
        .list_active_for_chat(channel, chat_id, 1)?
        .into_iter()
        .next())
}

pub fn current_or_next_step(record: &TaskRunRecord) -> Option<&TaskStep> {
    if let Some(step) = record
        .plan
        .ordered_steps
        .iter()
        .find(|step| step.step_id == record.run.current_step_id)
    {
        return Some(step);
    }
    record
        .plan
        .ordered_steps
        .iter()
        .find(|step| !step.status.is_terminal())
}

pub fn render_task_workspace_block(
    record: &TaskRunRecord,
    artifacts: &[TaskArtifactRecord],
    max_len: usize,
) -> Option<String> {
    let mut out = String::new();
    out.push_str("## Task Workspace\n");
    out.push_str("An active task run exists in the task workspace plane.\n");
    out.push_str(&format!(
        "Run: {} | status={:?} | revision={}\n",
        record.run.run_id, record.run.status, record.run.plan_revision
    ));
    out.push_str(&format!("Title: {}\n", record.run.title));
    out.push_str(&format!("Goal: {}\n", record.plan.goal));
    out.push_str(&format!(
        "Completion definition: {}\n",
        record.plan.completion_definition
    ));
    if let Some(step) = current_or_next_step(record) {
        out.push_str(&format!(
            "Current step: {} [{}] | retries={}/{} | tool_budget={}\n",
            step.title, step.step_id, step.attempt_count, step.retry_budget, step.tool_budget
        ));
        if !step.last_review_summary.is_empty() {
            out.push_str(&format!(
                "Current review note: {}\n",
                step.last_review_summary
            ));
        }
    }
    out.push_str("Steps:\n");
    for step in record
        .plan
        .ordered_steps
        .iter()
        .take(MAX_TASK_OPERATOR_STEP_PREVIEW)
    {
        out.push_str(&format!(
            "- [{}] {:?}: {}\n",
            step.step_id, step.status, step.title
        ));
    }
    if !artifacts.is_empty() {
        out.push_str("Recent artifacts:\n");
        for artifact in artifacts.iter().take(MAX_TASK_OPERATOR_ARTIFACT_PREVIEW) {
            out.push_str(&format!(
                "- {:?} {}: {}\n",
                artifact.artifact.kind, artifact.artifact.artifact_id, artifact.artifact.summary
            ));
        }
    }
    if !record.run.failure_reason.is_empty() {
        out.push_str(&format!(
            "Failure or blocker: {}\n",
            record.run.failure_reason
        ));
    }
    let rendered = truncate_content_to_max(out.trim(), max_len).into_owned();
    (!rendered.trim().is_empty()).then_some(rendered)
}

pub fn build_task_execution_operator_snapshot(
    task_run_store: &dyn TaskRunStore,
    task_artifact_store: &dyn TaskArtifactStore,
    task_learning_store: &dyn TaskLearningStore,
) -> Result<TaskExecutionOperatorSnapshot> {
    let mut recent_runs = Vec::new();
    for record in task_run_store
        .list_recent(MAX_TASK_OPERATOR_RECENT_RUNS.saturating_mul(2))?
        .into_iter()
        .take(MAX_TASK_OPERATOR_RECENT_RUNS)
    {
        let artifact_count = task_artifact_store
            .list_for_run(&record.run.run_id, usize::MAX)
            .map(|items| items.len())
            .unwrap_or(0);
        let completed_steps = record
            .plan
            .ordered_steps
            .iter()
            .filter(|step| matches!(step.status, TaskStepStatus::Passed))
            .count();
        let active_step = current_or_next_step(&record)
            .map(|step| step.title.clone())
            .unwrap_or_default();
        recent_runs.push(TaskExecutionOperatorRun {
            run_id: record.run.run_id.clone(),
            channel: record.run.source_channel.clone(),
            chat_id: record.run.source_chat_id.clone(),
            title: record.run.title.clone(),
            status: record.run.status,
            active_step,
            completed_steps,
            total_steps: record.plan.ordered_steps.len(),
            artifact_count,
            updated_at: record.run.updated_at,
            failure_reason: record.run.failure_reason.clone(),
        });
    }
    recent_runs.sort_by_key(|run| Reverse(run.updated_at));
    Ok(TaskExecutionOperatorSnapshot {
        recent_runs,
        learning: build_task_learning_operator_snapshot(task_learning_store)?,
    })
}

pub fn render_task_execution_operator_text(snapshot: &TaskExecutionOperatorSnapshot) -> String {
    let mut out = String::from("task_execution:\n");
    if snapshot.recent_runs.is_empty() {
        out.push_str("  recent_runs: none\n");
    } else {
        for run in &snapshot.recent_runs {
            out.push_str(&format!(
                "  - {} | status={:?} | {} -> {} | steps={}/{} | artifacts={} | active_step={} | failure={}\n",
                run.run_id,
                run.status,
                run.channel,
                run.chat_id,
                run.completed_steps,
                run.total_steps,
                run.artifact_count,
                if run.active_step.is_empty() {
                    "-"
                } else {
                    run.active_step.as_str()
                },
                if run.failure_reason.is_empty() {
                    "-"
                } else {
                    run.failure_reason.as_str()
                }
            ));
        }
    }
    out.push_str(&render_task_learning_operator_text(&snapshot.learning));
    out
}

pub fn next_ledger_sequence(entries: &[TaskExecutionLedgerEntry]) -> u32 {
    entries
        .iter()
        .map(|entry| entry.sequence)
        .max()
        .unwrap_or(0)
        .saturating_add(1)
}

pub fn summarize_task_artifact_content(content: &str) -> String {
    truncate_content_to_max(content.trim(), MAX_TASK_ARTIFACT_SUMMARY_CHARS).into_owned()
}

fn normalize_task_planner_step(
    mut step: TaskPlannerStepDraft,
    index: usize,
) -> Result<TaskPlannerStepDraft> {
    step.title = normalize_inline(&step.title, MAX_TASK_STEP_TITLE_CHARS);
    if step.title.is_empty() {
        return Err(Error::config(
            "task_planner_step",
            format!("step {} title must not be empty", index + 1),
        ));
    }
    step.instruction = normalize_multiline(&step.instruction, MAX_TASK_STEP_INSTRUCTION_CHARS);
    if step.instruction.is_empty() {
        return Err(Error::config(
            "task_planner_step",
            format!("step {} instruction must not be empty", index + 1),
        ));
    }
    step.tool_budget = step.tool_budget.clamp(1, MAX_TASK_STEP_TOOL_BUDGET);
    step.retry_budget = step.retry_budget.min(MAX_TASK_STEP_RETRY_BUDGET);
    normalize_string_list(
        &mut step.expected_artifacts,
        MAX_TASK_STEP_LIST_ITEMS,
        MAX_TASK_ARTIFACT_SUMMARY_CHARS,
    );
    normalize_string_list(
        &mut step.review_criteria,
        MAX_TASK_STEP_LIST_ITEMS,
        MAX_TASK_REASON_CHARS,
    );
    Ok(step)
}

fn normalize_string_list(values: &mut Vec<String>, max_items: usize, max_chars: usize) {
    *values = values
        .drain(..)
        .filter_map(|value| {
            let normalized = normalize_multiline(&value, max_chars);
            (!normalized.is_empty()).then_some(normalized)
        })
        .take(max_items)
        .collect();
}

fn normalize_task_clarification_fields(
    fields: &mut Vec<TaskClarificationField>,
    stage: &'static str,
) -> Result<()> {
    *fields = fields
        .drain(..)
        .filter_map(|mut field| {
            field.key = normalize_inline(&field.key, MAX_TASK_STEP_ID_CHARS);
            field.label = normalize_inline(&field.label, MAX_TASK_STEP_TITLE_CHARS);
            field.description = normalize_multiline(&field.description, MAX_TASK_REASON_CHARS);
            field.options = field
                .options
                .drain(..)
                .filter_map(|mut option| {
                    option.value = normalize_inline(&option.value, MAX_TASK_STEP_TITLE_CHARS);
                    option.label = normalize_inline(&option.label, MAX_TASK_STEP_TITLE_CHARS);
                    (!option.value.is_empty() && !option.label.is_empty()).then_some(option)
                })
                .take(MAX_TASK_CLARIFICATION_OPTIONS)
                .collect();
            if field.key.is_empty() || field.label.is_empty() || field.description.is_empty() {
                None
            } else {
                Some(field)
            }
        })
        .take(MAX_TASK_CLARIFICATION_FIELDS)
        .collect();
    if fields.iter().any(|field| field.key.is_empty()) {
        return Err(Error::config(
            stage,
            "clarification field key must not be empty",
        ));
    }
    Ok(())
}

fn normalize_multiline(value: &str, max_chars: usize) -> String {
    truncate_content_to_max(value.trim(), max_chars)
        .trim()
        .to_string()
}

fn normalize_inline(value: &str, max_chars: usize) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string()
        .chars()
        .take(max_chars)
        .collect()
}

fn normalize_fixed_id(value: &str, max_chars: usize, stage: &'static str) -> Result<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(Error::config(stage, "id must not be empty"));
    }
    if trimmed.chars().count() > max_chars {
        return Err(Error::config(stage, format!("id exceeds {}", max_chars)));
    }
    if !trimmed
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-'))
    {
        return Err(Error::config(stage, "id contains invalid chars"));
    }
    Ok(trimmed.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn planner_decision_requires_steps_for_task_runs() {
        let err = normalize_task_planner_decision(TaskPlannerDecision {
            route: TaskExecutionRoute::StartRun,
            reason: "complex".to_string(),
            blocker_summary: String::new(),
            missing_fields: Vec::new(),
            clarification_fields: Vec::new(),
            title: "Fix bug".to_string(),
            goal: "Fix the regression".to_string(),
            completion_definition: "Build should pass".to_string(),
            risk_notes: vec![],
            steps: vec![],
        })
        .unwrap_err();
        assert_eq!(err.stage(), "task_planner");
    }

    #[test]
    fn review_revise_requires_revised_steps() {
        let err = normalize_task_review_outcome(TaskReviewOutcome {
            decision: TaskReviewDecision::RevisePlan,
            summary: "need a new plan".to_string(),
            blocker_summary: String::new(),
            missing_fields: Vec::new(),
            clarification_fields: Vec::new(),
            artifact_summary: String::new(),
            revised_steps: vec![],
            durable_facts: vec![],
            reusable_procedures: vec![],
            evidence_only: vec![],
            transient_artifact_ids: vec![],
        })
        .unwrap_err();
        assert_eq!(err.stage(), "task_review");
    }

    #[test]
    fn workspace_block_mentions_active_step_and_artifacts() {
        let record = build_task_run_record(
            "tr0001abcd0001",
            "qq",
            "chat-1",
            "fix the crash",
            &normalize_task_planner_decision(TaskPlannerDecision {
                route: TaskExecutionRoute::StartRun,
                reason: "multi step".to_string(),
                blocker_summary: String::new(),
                missing_fields: Vec::new(),
                clarification_fields: Vec::new(),
                title: "Crash fix".to_string(),
                goal: "Fix the crash".to_string(),
                completion_definition: "Root cause fixed".to_string(),
                risk_notes: vec![],
                steps: vec![TaskPlannerStepDraft {
                    title: "Inspect".to_string(),
                    instruction: "Inspect the code path".to_string(),
                    tool_budget: 2,
                    retry_budget: 1,
                    expected_artifacts: vec!["notes".to_string()],
                    review_criteria: vec!["cause found".to_string()],
                }],
            })
            .unwrap(),
            10,
        )
        .unwrap();
        let rendered = render_task_workspace_block(
            &record,
            &[TaskArtifactRecord {
                artifact: TaskArtifact {
                    artifact_id: "a01".to_string(),
                    run_id: "tr0001abcd0001".to_string(),
                    step_id: "s01".to_string(),
                    kind: TaskArtifactKind::StepResult,
                    summary: "Found the root cause".to_string(),
                    content_ref: "memory/task_artifacts/tr0001abcd0001/a01.json".to_string(),
                    provenance: "executor".to_string(),
                    created_at: 11,
                },
                content: "details".to_string(),
            }],
            512,
        )
        .unwrap();
        assert!(rendered.contains("Current step"));
        assert!(rendered.contains("Recent artifacts"));
    }

    #[test]
    fn planner_decision_accepts_user_choice_blocker_without_plan_payload() {
        let decision = normalize_task_planner_decision(TaskPlannerDecision {
            route: TaskExecutionRoute::NeedsUserChoice,
            reason: "need account choice".to_string(),
            blocker_summary: "Need the user to choose which account to use.".to_string(),
            missing_fields: vec!["account_key".to_string()],
            clarification_fields: vec![TaskClarificationField {
                key: "account_key".to_string(),
                label: "Account".to_string(),
                description: "Choose one account.".to_string(),
                required: true,
                secret: false,
                multiple: false,
                options: vec![TaskClarificationOption {
                    value: "work".to_string(),
                    label: "Work".to_string(),
                }],
            }],
            title: "ignored".to_string(),
            goal: "ignored".to_string(),
            completion_definition: "ignored".to_string(),
            risk_notes: vec!["ignored".to_string()],
            steps: vec![TaskPlannerStepDraft {
                title: "ignored".to_string(),
                instruction: "ignored".to_string(),
                tool_budget: 1,
                retry_budget: 0,
                expected_artifacts: Vec::new(),
                review_criteria: Vec::new(),
            }],
        })
        .expect("planner blocker decision");
        assert!(decision.goal.is_empty());
        assert!(decision.steps.is_empty());
        assert_eq!(decision.clarification_fields.len(), 1);
    }

    #[test]
    fn review_outcome_accepts_confirmation_blocker_without_revised_steps() {
        let outcome = normalize_task_review_outcome(TaskReviewOutcome {
            decision: TaskReviewDecision::NeedsConfirmation,
            summary: "Need explicit approval before continuing.".to_string(),
            blocker_summary: "Need explicit approval before continuing.".to_string(),
            missing_fields: vec!["confirm".to_string()],
            clarification_fields: vec![TaskClarificationField {
                key: "confirm".to_string(),
                label: "Confirm".to_string(),
                description: "Confirm whether to continue.".to_string(),
                required: true,
                secret: false,
                multiple: false,
                options: vec![
                    TaskClarificationOption {
                        value: "true".to_string(),
                        label: "Continue".to_string(),
                    },
                    TaskClarificationOption {
                        value: "false".to_string(),
                        label: "Stop".to_string(),
                    },
                ],
            }],
            artifact_summary: String::new(),
            revised_steps: vec![],
            durable_facts: vec![],
            reusable_procedures: vec![],
            evidence_only: vec![],
            transient_artifact_ids: vec![],
        })
        .expect("review blocker decision");
        assert_eq!(outcome.clarification_fields.len(), 1);
    }
}
