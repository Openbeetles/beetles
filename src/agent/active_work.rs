//! Formal foreground active-work contract for the current chat.
//! 对当前会话前台主工作面的正式合同。

use crate::error::Result;
use crate::memory::{
    render_execution_state_block, should_resume_active_execution_state, ExecutionState,
    ExecutionStatus,
};
use crate::task_execution::{current_or_next_step, TaskRunRecord, TaskRunStatus};
use crate::util::truncate_content_to_max;
use serde::{Deserialize, Serialize};

pub const REL_PATH_ACTIVE_WORKS: &str = "memory/active_works.json";

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ActiveWorkKind {
    InteractiveAction,
    TaskExecution,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ActiveWorkRecord {
    pub kind: ActiveWorkKind,
    #[serde(default)]
    pub title: String,
    pub state: ExecutionState,
}

impl ActiveWorkRecord {
    pub fn is_meaningful(&self) -> bool {
        self.state.status != ExecutionStatus::Done && self.state.is_meaningful()
    }

    pub(crate) fn should_resume(&self, user_content: &str) -> bool {
        self.state.status != ExecutionStatus::Done
            && should_resume_active_execution_state(&self.state, user_content)
    }

    pub(crate) fn from_task_run(record: &TaskRunRecord) -> Option<Self> {
        let mut state = ExecutionState {
            status: task_run_status_to_execution_status(record.run.status),
            goal: record.plan.goal.clone(),
            updated_at: record.run.updated_at,
            ..ExecutionState::default()
        };
        if let Some(step) = current_or_next_step(record) {
            if !step.last_result_summary.trim().is_empty() {
                state.progress = step.last_result_summary.clone();
            } else if !step.title.trim().is_empty() {
                state.progress = format!("Current step: {}", step.title);
            }
            if !step.instruction.trim().is_empty() {
                state.next_action = step.instruction.clone();
            } else if !step.title.trim().is_empty() {
                state.next_action = step.title.clone();
            }
            if !step.last_review_summary.trim().is_empty() {
                state
                    .latest_observations
                    .push(step.last_review_summary.clone());
            }
            if !step.title.trim().is_empty() {
                state.next_best_actions.push(step.title.clone());
            }
        }
        if !record.run.final_summary.trim().is_empty() {
            state.last_output = record.run.final_summary.clone();
        }
        if !record.run.failure_reason.trim().is_empty() {
            state.blocker = record.run.failure_reason.clone();
        }
        if state.goal.trim().is_empty() {
            state.goal = record.run.user_request.clone();
        }
        let title = if record.run.title.trim().is_empty() {
            state.goal.clone()
        } else {
            record.run.title.clone()
        };
        let candidate = Self {
            kind: ActiveWorkKind::TaskExecution,
            title,
            state,
        };
        candidate.is_meaningful().then_some(candidate)
    }

    pub(crate) fn from_execution_state(
        kind: ActiveWorkKind,
        state: &ExecutionState,
    ) -> Option<Self> {
        if state.status == ExecutionStatus::Done || !state.is_meaningful() {
            return None;
        }
        let title = if !state.goal.trim().is_empty() {
            state.goal.clone()
        } else {
            state.next_action.clone()
        };
        Some(Self {
            kind,
            title,
            state: state.clone(),
        })
    }
}

pub trait ActiveWorkStore: Send + Sync {
    fn get(&self, chat_id: &str) -> Result<Option<ActiveWorkRecord>>;
    fn set(&self, chat_id: &str, record: &ActiveWorkRecord) -> Result<()>;
    fn clear(&self, chat_id: &str) -> Result<()>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ActiveWorkSyncInput<'a> {
    pub(crate) chat_id: &'a str,
    pub(crate) request_semantics: crate::agent::request_semantics::RequestSemantics,
    pub(crate) reply_surface: crate::agent::reply_surface::ReplySurface,
    pub(crate) active_task_run: Option<&'a TaskRunRecord>,
    pub(crate) execution_state: Option<&'a ExecutionState>,
}

pub(crate) fn load_active_work_for_chat(
    store: &dyn ActiveWorkStore,
    active_task_run: Option<&TaskRunRecord>,
    chat_id: &str,
) -> Result<Option<ActiveWorkRecord>> {
    if let Some(record) = store.get(chat_id)? {
        if record.is_meaningful() {
            return Ok(Some(record));
        }
    }
    Ok(active_task_run.and_then(ActiveWorkRecord::from_task_run))
}

pub(crate) fn render_active_work_block(
    record: &ActiveWorkRecord,
    max_len: usize,
) -> Option<String> {
    let kind = match record.kind {
        ActiveWorkKind::InteractiveAction => "interactive_action",
        ActiveWorkKind::TaskExecution => "task_execution",
    };
    let mut out = String::from("## Active Work\n");
    out.push_str(&format!("Kind: {}\n", kind));
    if !record.title.trim().is_empty() {
        out.push_str(&format!("Title: {}\n", record.title.trim()));
    }
    if let Some(state_block) = render_execution_state_block(&record.state, max_len) {
        out.push_str(&state_block);
    }
    let rendered = truncate_content_to_max(out.trim(), max_len).into_owned();
    (!rendered.trim().is_empty()).then_some(rendered)
}

pub(crate) fn sync_active_work_after_turn(
    store: &dyn ActiveWorkStore,
    input: ActiveWorkSyncInput<'_>,
) -> Result<()> {
    use crate::agent::reply_surface::ReplySurface;
    use crate::agent::request_semantics::ResumeRelation;

    if matches!(
        input.request_semantics.resume_relation,
        ResumeRelation::DenyOrCancelActiveAction | ResumeRelation::SwitchToNewRequest
    ) {
        return store.clear(input.chat_id);
    }
    let next = if input.reply_surface == ReplySurface::TaskExecution {
        input
            .active_task_run
            .and_then(ActiveWorkRecord::from_task_run)
            .or_else(|| {
                input.execution_state.and_then(|state| {
                    ActiveWorkRecord::from_execution_state(ActiveWorkKind::TaskExecution, state)
                })
            })
    } else if should_keep_interactive_action_work(input.request_semantics) {
        input.execution_state.and_then(|state| {
            ActiveWorkRecord::from_execution_state(ActiveWorkKind::InteractiveAction, state)
        })
    } else {
        None
    };
    if let Some(record) = next {
        store.set(input.chat_id, &record)
    } else {
        store.clear(input.chat_id)
    }
}

fn should_keep_interactive_action_work(
    semantics: crate::agent::request_semantics::RequestSemantics,
) -> bool {
    use crate::agent::request_semantics::{ActionFamily, ResumeRelation};

    matches!(
        semantics.action_family,
        ActionFamily::ActionRequest | ActionFamily::ActiveAction
    ) || matches!(
        semantics.resume_relation,
        ResumeRelation::ConfirmActiveAction
            | ResumeRelation::SupplyActiveActionInput
            | ResumeRelation::ResumeActiveAction
    )
}

fn task_run_status_to_execution_status(status: TaskRunStatus) -> ExecutionStatus {
    match status {
        TaskRunStatus::Planning | TaskRunStatus::Running => ExecutionStatus::Active,
        TaskRunStatus::Blocked | TaskRunStatus::Failed => ExecutionStatus::Blocked,
        TaskRunStatus::Completed | TaskRunStatus::PartialComplete | TaskRunStatus::Aborted => {
            ExecutionStatus::Done
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::reply_surface::ReplySurface;
    use crate::agent::request_semantics::{
        ActionFamily, DisclosureSurface, EvidenceNeed, ExecutionPreference, RequestKind,
        RequestSemantics, ResumeRelation,
    };
    use crate::task_execution::{TaskPlan, TaskRun, TaskRunStatus, TaskStep, TaskStepStatus};
    use std::collections::HashMap;
    use std::sync::Mutex;

    #[derive(Default)]
    struct MemoryActiveWorkStore {
        inner: Mutex<HashMap<String, ActiveWorkRecord>>,
    }

    impl ActiveWorkStore for MemoryActiveWorkStore {
        fn get(&self, chat_id: &str) -> Result<Option<ActiveWorkRecord>> {
            Ok(self
                .inner
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(chat_id)
                .cloned())
        }

        fn set(&self, chat_id: &str, record: &ActiveWorkRecord) -> Result<()> {
            self.inner
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(chat_id.to_string(), record.clone());
            Ok(())
        }

        fn clear(&self, chat_id: &str) -> Result<()> {
            self.inner
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(chat_id);
            Ok(())
        }
    }

    fn semantics(action_family: ActionFamily, resume_relation: ResumeRelation) -> RequestSemantics {
        RequestSemantics {
            request_kind: RequestKind::General,
            evidence_need: EvidenceNeed::HostTool,
            disclosure_surface: DisclosureSurface::Governed,
            execution_preference: ExecutionPreference::ToolFirst,
            action_family,
            resume_relation,
            confidence: 100,
        }
    }

    fn sample_task_run(status: TaskRunStatus) -> TaskRunRecord {
        TaskRunRecord {
            run: TaskRun {
                run_id: "run-1".to_string(),
                source_channel: "qq_channel".to_string(),
                source_chat_id: "chat-1".to_string(),
                user_request: "配置 QQ 邮箱".to_string(),
                title: "QQ 邮箱配置".to_string(),
                status,
                current_step_id: "s01".to_string(),
                planner_reason: String::new(),
                final_summary: "账户草案已创建".to_string(),
                failure_reason: String::new(),
                plan_revision: 1,
                created_at: 1,
                updated_at: 9,
                finished_at: 0,
            },
            plan: TaskPlan {
                goal: "配置 QQ 邮箱账户".to_string(),
                completion_definition: "账户已保存并通过校验".to_string(),
                risk_notes: Vec::new(),
                ordered_steps: vec![TaskStep {
                    step_id: "s01".to_string(),
                    title: "补认证信息".to_string(),
                    instruction: "写入 provider_kind 并补认证凭据".to_string(),
                    status: TaskStepStatus::Running,
                    tool_budget: 1,
                    retry_budget: 1,
                    expected_artifacts: Vec::new(),
                    review_criteria: Vec::new(),
                    attempt_count: 0,
                    last_result_summary: "账户草案已创建".to_string(),
                    last_review_summary: String::new(),
                    started_at: 0,
                    finished_at: 0,
                }],
            },
        }
    }

    #[test]
    fn loads_active_work_from_store_before_task_run_fallback() {
        let store = MemoryActiveWorkStore::default();
        let stored = ActiveWorkRecord {
            kind: ActiveWorkKind::InteractiveAction,
            title: "查看系统状态".to_string(),
            state: ExecutionState {
                goal: "查看当前系统状态".to_string(),
                next_action: "执行 office_status".to_string(),
                updated_at: 5,
                ..ExecutionState::default()
            },
        };
        store.set("chat-1", &stored).expect("store");

        let loaded = load_active_work_for_chat(
            &store,
            Some(&sample_task_run(TaskRunStatus::Running)),
            "chat-1",
        )
        .expect("load")
        .expect("active work");

        assert_eq!(loaded.kind, ActiveWorkKind::InteractiveAction);
        assert_eq!(loaded.title, "查看系统状态");
    }

    #[test]
    fn loads_active_work_from_task_run_when_store_missing() {
        let store = MemoryActiveWorkStore::default();
        let loaded = load_active_work_for_chat(
            &store,
            Some(&sample_task_run(TaskRunStatus::Running)),
            "chat-1",
        )
        .expect("load")
        .expect("active work");

        assert_eq!(loaded.kind, ActiveWorkKind::TaskExecution);
        assert_eq!(loaded.title, "QQ 邮箱配置");
        assert!(loaded.should_resume("继续"));
    }

    #[test]
    fn sync_clears_active_work_on_switch_request() {
        let store = MemoryActiveWorkStore::default();
        let record = ActiveWorkRecord {
            kind: ActiveWorkKind::InteractiveAction,
            title: "QQ 邮箱配置".to_string(),
            state: ExecutionState {
                goal: "配置 QQ 邮箱账户".to_string(),
                next_action: "补认证信息".to_string(),
                updated_at: 1,
                ..ExecutionState::default()
            },
        };
        store.set("chat-1", &record).expect("store");

        sync_active_work_after_turn(
            &store,
            ActiveWorkSyncInput {
                chat_id: "chat-1",
                request_semantics: semantics(
                    ActionFamily::ActionRequest,
                    ResumeRelation::SwitchToNewRequest,
                ),
                reply_surface: ReplySurface::GovernedConversation,
                active_task_run: None,
                execution_state: None,
            },
        )
        .expect("sync");

        assert!(store.get("chat-1").expect("get").is_none());
    }

    #[test]
    fn sync_promotes_action_execution_state_but_not_plain_conversation() {
        let store = MemoryActiveWorkStore::default();
        let state = ExecutionState {
            goal: "配置 QQ 邮箱账户".to_string(),
            next_action: "补认证信息".to_string(),
            updated_at: 7,
            ..ExecutionState::default()
        };

        sync_active_work_after_turn(
            &store,
            ActiveWorkSyncInput {
                chat_id: "chat-1",
                request_semantics: semantics(
                    ActionFamily::ActionRequest,
                    ResumeRelation::IndependentTurn,
                ),
                reply_surface: ReplySurface::GovernedConversation,
                active_task_run: None,
                execution_state: Some(&state),
            },
        )
        .expect("sync");
        assert_eq!(
            store.get("chat-1").expect("get").expect("record").kind,
            ActiveWorkKind::InteractiveAction
        );

        sync_active_work_after_turn(
            &store,
            ActiveWorkSyncInput {
                chat_id: "chat-1",
                request_semantics: semantics(
                    ActionFamily::Conversation,
                    ResumeRelation::IndependentTurn,
                ),
                reply_surface: ReplySurface::PublicRuntime,
                active_task_run: None,
                execution_state: Some(&state),
            },
        )
        .expect("sync");
        assert!(store.get("chat-1").expect("get").is_none());
    }
}
