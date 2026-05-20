//! Agent ReAct 循环：入站一条 → context → chat（含 tool_use 多轮）→ 会话持久化 → 出站一条。
//! 仅依赖 trait；HTTP/Tool 由 main 注入同一实现（如 EspHttpClient）。
#![allow(clippy::too_many_arguments)]

mod background_jobs;
mod delivery_handoff;
mod driver;
mod ingress_admission;
mod reply_finalize;
mod task_execution;
mod task_execution_support;
mod tool_round;
mod turn_execution;
mod turn_finalize;
mod turn_prepare;
mod worker_context_stages;
mod worker_error;
mod worker_governance;

use super::delivery::{DeliveryReport, DeliverySession, ToolIntentDelivery};
use super::reasoning_intent::ProgrammableReasoningIntent;
use super::reply_surface::ReplySurface;
use super::request_plan::AgentRequestPlan;
use super::request_semantics::RequestSemantics;
use super::soul_feedback::{build_turn_soul_feedback_ledger, SoulFeedbackProjection};
use super::strategy::AgentRunStrategy;
use super::subject_state::{
    build_turn_subject_state_ledger, compile_subject_state, render_subject_state_block,
    SubjectState, SubjectStateCompileInput,
};
use super::tool_outcome::{
    classify_tool_error, denied_tool_assessment, unavailable_tool_assessment,
};
use super::StreamEditor;
use crate::agent::context::{
    build_context, estimate_post_memory_system_tail_len, PostMemoryTailParams, RuntimeContext,
};
use crate::bus::{
    InboundRx, IngressKind, OutboundKind, OutboundTx, PcMsg, SystemInboundTx, UserInboundRx,
    UserInboundTx, MAX_CONTENT_LEN,
};
use crate::constants::{
    AGENT_MARKER_MARK_IMPORTANT, AGENT_MARKER_SIGNAL_COMFORT, AGENT_RETRY_BASE_MS,
    AGENT_RETRY_MAX_MS, INBOUND_RECV_TIMEOUT_SECS, MAX_DEFER_RETRIES,
    MAX_TOOL_RESULTS_USER_MESSAGE_LEN,
};
use crate::error::Result;
use crate::i18n::{tr, Locale as UiLocale, Message as UiMessage};
use crate::llm::{LlmClient, Message, StopReason, ToolChoicePolicy};
use crate::memory::{
    board_subject_scope_id, build_turn_ledger_start, build_turn_persona_disclosure_ledger,
    build_turn_persona_priority_ledger, compute_core_revision_governance_digest,
    load_prompt_memory_context, load_recent_persona_evidence, memory_policy,
    normalize_turn_observation_text, normalize_turn_persona_scope, normalize_turn_persona_targets,
    normalize_turn_preview, normalize_turn_reason, recall_long_term_memory_block,
    render_core_revision_governance_block, render_recent_persona_evidence_block,
    run_long_term_memory_refresh, run_mental_privacy_disclosure_adjudication,
    run_mental_privacy_review, run_post_reply_memory_maintenance, run_self_runtime,
    LongTermMemoryRefreshContext, LongTermMemoryRefreshOutcome,
    LongTermMemoryRefreshRequestOutcome, MentalPrivacyDisclosureAdjudicationContext,
    MentalPrivacyDisclosureAdjudicationInput, MentalPrivacyReviewContext, MentalPrivacyReviewInput,
    MentalPrivacyReviewOutcome, PersonaPriorityAdjudication, PersonaPriorityAdjudicationInput,
    PersonaPriorityGrounding, PersonaPriorityRuntimeState, PostReplyMemoryMaintenanceContext,
    PostReplyMemoryMaintenanceInput, PromptMemoryContext, PromptMemoryContextParams,
    PromptRuntimeCarry, SelfRuntimeContext, SessionMessage, SessionSummaryRefreshOutcome,
    TurnBlockerLedger, TurnDeliveryLedger, TurnExecutionClass, TurnLedger, TurnLedgerStatus,
    TurnLedgerStore, TurnModeSnapshotLedger, TurnObservationLedger, TurnPersonaLedger,
    TurnPersonaReviewLedger, TurnToolPathLedger,
};
use crate::metrics;
use crate::orchestrator::admission::{LlmDecision, ToolDecision};
use crate::runtime::system_work::{
    classify_system_work, CHANNEL_CRON, CHANNEL_DETACHED_WORK_WAKE, CHANNEL_IDLE_MEMORY_FORGE,
    CHANNEL_LONG_TERM_MEMORY_REFRESH, CHANNEL_OPERATOR_MAINTENANCE, CHANNEL_POST_REPLY_MAINTENANCE,
    CHANNEL_SELF_RUNTIME,
};
use crate::state;
use crate::task_execution::{
    active_task_run_for_chat, apply_revised_remaining_steps, build_task_learning_records,
    build_task_run_record, next_ledger_sequence, normalize_task_planner_decision,
    normalize_task_review_outcome, summarize_task_artifact_content, TaskArtifact, TaskArtifactKind,
    TaskArtifactRecord, TaskExecutionLedgerEntry, TaskExecutionRoute, TaskLedgerKind,
    TaskPlannerDecision, TaskReviewDecision, TaskReviewOutcome, TaskRunRecord, TaskRunStatus,
    TaskStep, TaskStepStatus,
};
use crate::task_execution::{TaskArtifactStore, TaskExecutionLedgerStore, TaskRunStore};
use crate::tools::http_bridge::HttpClientToolContext;
use crate::tools::{ToolOutboundDeliveryKind, ToolOutboundIntent, ToolOutboundTarget};
use crate::util::{
    push_json_string_escaped, remove_substrings_all_trim, truncate_content_to_max,
    usize_to_decimal_buf,
};
use crate::PlatformHttpClient;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::fmt::Write as _;
use std::hash::{Hash, Hasher};
#[cfg(test)]
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::RecvTimeoutError;
use std::sync::Arc;
use std::time::{Duration, Instant};

use self::background_jobs::run_background_job_with_accounting;
use self::delivery_handoff::deliver_turn;
use self::driver::{prepare_system_with_suffix, recv_next_agent_msg};
use self::ingress_admission::admit_turn;
use self::reply_finalize::{complete_turn_boxed, finalize_turn_boxed};
use self::task_execution::try_run_task_execution;
use self::tool_round::execute_tool_use_round;
use self::turn_execution::execute_turn_boxed;
use self::turn_finalize::{persist_turn_continuity_evidence, persist_turn_ledger};
use self::worker_error::handle_worker_path_error;
use super::deliberation::{
    compile_turn_deliberation_gate, render_turn_deliberation_gate_block, TurnDeliberationGate,
    TurnDeliberationInput,
};

type CapabilityPackageTextProvider = Arc<dyn Fn(&str, usize) -> Option<String> + Send + Sync>;

/// 最大 ReAct 轮数（含首轮 chat），防止无限 tool 循环。
const MAX_REACT_ROUNDS: usize = 10;
const MAX_TOOL_PROTOCOL_REPAIR_ATTEMPTS: u8 = 1;

/// 工具结果 user 消息前缀；与 `compact_early_tool_rounds` / 摘要逻辑一致。
const TOOL_RESULTS_PREFIX: &str = "Tool results:\n";
const MEMORY_GROUNDING_SUMMARY_PREVIEW_CHARS: usize = 160;
const MEMORY_GROUNDING_LONG_TERM_PREVIEW_CHARS: usize = 240;

/// ReAct 轮间保留完整内容的最近轮数（每轮 assistant + user 各 1 条 = 4 条）。
const REACT_FULL_ROUNDS_KEPT: usize = 2;
const REACT_FULL_MSGS_KEPT: usize = REACT_FULL_ROUNDS_KEPT * 2;

/// 早期轮次工具结果摘要：每条结果保留的首行预览字符数（UTF-8 安全截断）。
const TOOL_RESULT_PREVIEW_CHARS: usize = 80;
const TOOL_RESULT_TAIL_CHARS: usize = 24;
const TOOL_EVIDENCE_PREVIEW_CHARS: usize = 120;
const TOOL_EVIDENCE_TAIL_CHARS: usize = 32;
const MAX_TOOL_EVIDENCE_ITEMS: usize = 4;
const TOOL_RESULT_RAW_MIN_BYTES: usize = 1536;
const TOOL_EVIDENCE_RESERVED_BYTES: usize = 768;
const TOOL_MEMORY_GROUNDING_MAX_BYTES: usize = 384;
const TOOL_RESULTS_TRUNCATED_MARKER: &str = "\n[truncated]";
const ASSISTANT_COMPACT_PREVIEW_CHARS: usize = 160;
const ASSISTANT_COMPACT_TAIL_CHARS: usize = 48;
const POST_REPLY_MAINTENANCE_USER_PREVIEW_CHARS: usize = 512;
const POST_REPLY_MAINTENANCE_REPLY_PREVIEW_CHARS: usize = 768;
const POST_REPLY_MAINTENANCE_DELAY_MS: u64 = 1_500;
const TASK_EXECUTION_PLANNER_SYSTEM_SUFFIX: &str = "\n\n## Task Execution Planner\nDecide whether the latest user request should stay on the normal reply path, enter the formal task-execution path, or stop first for a structured workflow blocker. Return JSON only with fields: route, reason, blocker_summary, missing_fields, clarification_fields, title, goal, completion_definition, risk_notes, steps. route must be one of direct_reply, start_run, resume_run, needs_user_facts, needs_user_choice, needs_confirmation. When route is direct_reply, leave blocker_summary/missing_fields/clarification_fields/goal/completion_definition/steps empty. When route is a blocker route, explain the blocker in blocker_summary, fill missing_fields and clarification_fields as needed, and leave title/goal/completion_definition/steps empty. When route starts or resumes a run, steps must be an ordered array of 1-6 objects with title, instruction, tool_budget, retry_budget, expected_artifacts, review_criteria. clarification_fields is an array of objects with key, label, description, required, secret, multiple, options; each option has value and label. Do not answer the user. Do not call tools in this planner step.";
const TASK_EXECUTION_REVIEW_SYSTEM_SUFFIX: &str = "\n\n## Task Step Reviewer\nReview the just-finished task step and decide whether the run should pass the step, retry the same step, revise the remaining plan, abort the run, finish partially, or stop for a structured workflow blocker. Return JSON only with fields: decision, summary, blocker_summary, missing_fields, clarification_fields, artifact_summary, revised_steps, durable_facts, reusable_procedures, evidence_only, transient_artifact_ids. decision must be one of pass, retry_step, revise_plan, abort_run, partial_complete, needs_user_facts, needs_user_choice, needs_confirmation. Only provide revised_steps when decision is revise_plan. For blocker decisions, fill blocker_summary, missing_fields, and clarification_fields, and leave revised_steps empty. clarification_fields is an array of objects with key, label, description, required, secret, multiple, options; each option has value and label. durable_facts / reusable_procedures / evidence_only are arrays of objects with topic, summary, content, and optional memory_kind for durable_facts. durable_facts are only for canonical long-term facts that deserve governed shared memory. reusable_procedures are only for methods that might become runtime skills after repeated success. evidence_only is for supporting evidence that should enter archive but not canonical memory. transient_artifact_ids lists workspace artifact ids that should be pruned after review because they are low-value scratch output. Do not call tools in this review step.";
const TASK_EXECUTION_FINISHER_SYSTEM_SUFFIX: &str = "\n\n## Task Run Finisher\nUsing only the governed task workspace, completed step outputs, and current conclusions, write the final user-facing reply for this task run. Do not call tools. Do not output execution transcripts, internal step ids, or future-plan boilerplate. If the run is partial or blocked, say exactly what was completed and what remains blocked.";
const TASK_EXECUTION_MIN_CHARS: usize = 96;
const TASK_EXECUTION_MIN_LINES: usize = 3;
const TASK_EXECUTION_MIN_SEPARATORS: usize = 2;
const TASK_EXECUTION_ARTIFACT_PREVIEW_LIMIT: usize = 4;
const INGRESS_VISIBILITY_ACK_SETTLE_MS: u64 = 5;

/// 程序性会话摘要：单次轻量 LLM 调用的 system 提示。
/// 同一 chat_id 的 "low memory, defer" 日志最少间隔，避免刷屏。
const LOW_MEM_DEFER_LOG_INTERVAL: Duration = Duration::from_secs(60);

fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis().min(u64::MAX as u128) as u64)
        .unwrap_or(0)
}

#[allow(clippy::result_large_err)]
pub(super) fn try_send_inbound_msg(
    msg: PcMsg,
    user_inbound_tx: &UserInboundTx,
    system_inbound_tx: &SystemInboundTx,
) -> std::result::Result<(), std::sync::mpsc::TrySendError<PcMsg>> {
    match msg.ingress {
        IngressKind::User => {
            let source = msg
                .runtime_foreground_source()
                .unwrap_or(crate::runtime::RuntimeForegroundSource::ExternalUserMessage);
            user_inbound_tx.try_submit_user(msg, source)
        }
        IngressKind::System => system_inbound_tx.try_send(msg),
    }
}

fn is_long_term_memory_refresh_job(msg: &PcMsg) -> bool {
    msg.ingress == IngressKind::System && msg.channel.as_ref() == CHANNEL_LONG_TERM_MEMORY_REFRESH
}

fn is_post_reply_maintenance_job(msg: &PcMsg) -> bool {
    msg.ingress == IngressKind::System && msg.channel.as_ref() == CHANNEL_POST_REPLY_MAINTENANCE
}

fn is_idle_memory_forge_job(msg: &PcMsg) -> bool {
    msg.ingress == IngressKind::System && msg.channel.as_ref() == CHANNEL_IDLE_MEMORY_FORGE
}

fn is_self_runtime_job(msg: &PcMsg) -> bool {
    msg.ingress == IngressKind::System && msg.channel.as_ref() == CHANNEL_SELF_RUNTIME
}

fn is_operator_maintenance_job(msg: &PcMsg) -> bool {
    msg.ingress == IngressKind::System && msg.channel.as_ref() == CHANNEL_OPERATOR_MAINTENANCE
}

fn is_lane_background_job(msg: &PcMsg) -> bool {
    is_long_term_memory_refresh_job(msg)
        || is_post_reply_maintenance_job(msg)
        || is_idle_memory_forge_job(msg)
        || is_self_runtime_job(msg)
        || is_operator_maintenance_job(msg)
        || is_detached_work_wake(msg)
}

fn should_persist_background_job_as_detached(
    msg: &PcMsg,
    profile: crate::memory::MemoryProfile,
) -> bool {
    if matches!(profile, crate::memory::MemoryProfile::Embedded)
        && is_lane_background_job(msg)
        && !is_detached_work_wake(msg)
    {
        return false;
    }
    true
}

const BACKGROUND_DEFER_DELAY_MS: u64 = 1_000;
const DETACHED_WAKE_RETRY_DELAY_MS: u64 = 250;
const IDLE_SELF_RUNTIME_RETRY_DELAY_MS: u64 = 5_000;

fn append_background_defer_workflow_audit(
    channel: &str,
    chat_id: &str,
    trigger: crate::runtime::WorkflowTrigger,
    workflow: crate::runtime::WorkflowKind,
    rationale: &str,
) {
    crate::runtime::append_workflow_audit(
        crate::runtime::WorkflowAuditRecord::new(
            workflow,
            trigger,
            crate::runtime::WorkflowDisposition::DeferUntil,
            crate::runtime::WorkflowEffect::EnqueueSystemJob,
            crate::runtime::WorkflowRecoveryPolicy::DropOnModeExit,
            rationale,
            crate::util::current_unix_secs(),
        )
        .with_target(None, Some(channel), Some(chat_id)),
    );
}

fn post_reply_quiet_delay_ms() -> Option<u64> {
    crate::runtime::system_work::post_reply_quiet_window_remaining_ms(
        crate::util::current_unix_secs(),
        crate::metrics::snapshot().last_active_epoch_secs,
    )
}

fn is_detached_work_wake(msg: &PcMsg) -> bool {
    msg.ingress == IngressKind::System && msg.channel.as_ref() == CHANNEL_DETACHED_WORK_WAKE
}

fn detached_work_key_for_msg(msg: &PcMsg) -> Option<crate::agent::DetachedWorkKey> {
    let chat_id = msg.chat_id.as_ref();
    match msg.channel.as_ref() {
        CHANNEL_LONG_TERM_MEMORY_REFRESH => Some(crate::agent::DetachedWorkKey::new(
            "memory_refresh",
            chat_id,
            crate::agent::DetachedJobKind::LongTermMemoryRefresh,
        )),
        CHANNEL_POST_REPLY_MAINTENANCE => {
            let payload: PostReplyMaintenanceJobPayload =
                serde_json::from_str(&msg.content).ok()?;
            let owner_channel = if payload.source_channel.trim().is_empty() {
                "post_reply_maintenance".to_string()
            } else {
                payload.source_channel
            };
            Some(crate::agent::DetachedWorkKey::new(
                owner_channel,
                chat_id,
                crate::agent::DetachedJobKind::PostReplyMaintenance,
            ))
        }
        CHANNEL_IDLE_MEMORY_FORGE => {
            let owner_channel = serde_json::from_str::<serde_json::Value>(&msg.content)
                .ok()
                .and_then(|value| {
                    value
                        .get("source_channel")
                        .and_then(|field| field.as_str())
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .map(ToOwned::to_owned)
                })
                .unwrap_or_else(|| "idle_memory_forge".to_string());
            Some(crate::agent::DetachedWorkKey::new(
                owner_channel,
                chat_id,
                crate::agent::DetachedJobKind::IdleMemoryForge,
            ))
        }
        CHANNEL_SELF_RUNTIME => {
            let payload: crate::memory::SelfRuntimeJobPayload =
                serde_json::from_str(&msg.content).ok()?;
            let owner_channel = if payload.source_channel.trim().is_empty() {
                format!("self_runtime:{chat_id}")
            } else {
                payload.source_channel
            };
            Some(crate::agent::DetachedWorkKey::new(
                owner_channel,
                chat_id,
                match payload.trigger {
                    crate::memory::SelfRuntimeTrigger::PostReply => {
                        crate::agent::DetachedJobKind::SelfRuntimePostReply
                    }
                    crate::memory::SelfRuntimeTrigger::IdleTick => {
                        crate::agent::DetachedJobKind::SelfRuntimeIdleTick
                    }
                    crate::memory::SelfRuntimeTrigger::OperatorRequested => {
                        crate::agent::DetachedJobKind::OperatorMaintenance
                    }
                },
            ))
        }
        CHANNEL_OPERATOR_MAINTENANCE => {
            let request: crate::runtime::OperatorMaintenanceRequest =
                serde_json::from_str(&msg.content).ok()?;
            let owner_channel = request
                .channel
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or("operator_maintenance");
            Some(crate::agent::DetachedWorkKey::new(
                owner_channel,
                request.queue_chat_id(),
                crate::agent::DetachedJobKind::OperatorMaintenance,
            ))
        }
        _ => None,
    }
}

fn adopt_background_job_as_detached(
    store: &dyn crate::agent::DetachedWorkStore,
    msg: &PcMsg,
    reason: &str,
) -> Result<bool> {
    let Some(key) = detached_work_key_for_msg(msg) else {
        return Ok(false);
    };
    crate::agent::upsert_detached_work_job(store, key, msg, 0, reason).map(|_| true)
}

fn wake_due_detached_background_work(
    store: &dyn crate::agent::DetachedWorkStore,
    system_inbound_tx: &SystemInboundTx,
    limit: usize,
) {
    let now_ms = crate::agent::current_unix_ms();
    let due_records = match crate::agent::due_detached_work_records(store, now_ms, limit) {
        Ok(records) => records,
        Err(error) => {
            log::warn!("[agent] detached wake scan failed: {}", error);
            return;
        }
    };
    for record in due_records {
        match store.mark_queued(&record.key, record.revision) {
            Ok(true) => {}
            Ok(false) => continue,
            Err(error) => {
                log::warn!(
                    "[agent] detached wake queue mark failed chat_id={} kind={:?}: {}",
                    record.key.owner_chat_id,
                    record.key.kind,
                    error
                );
                continue;
            }
        }
        let wake = crate::agent::DetachedWorkWake {
            key: record.key.clone(),
            revision: record.revision,
        };
        let body = match serde_json::to_string(&wake) {
            Ok(body) => body,
            Err(error) => {
                log::warn!("[agent] detached wake serialize failed: {}", error);
                let _ = store.reschedule(
                    &record.key,
                    record.revision,
                    now_ms.saturating_add(DETACHED_WAKE_RETRY_DELAY_MS),
                    "detached_wake_serialize_failed",
                );
                continue;
            }
        };
        let wake_msg =
            match PcMsg::new_system(CHANNEL_DETACHED_WORK_WAKE, &record.key.owner_chat_id, body) {
                Ok(msg) => msg,
                Err(error) => {
                    log::warn!("[agent] detached wake build failed: {}", error);
                    let _ = store.reschedule(
                        &record.key,
                        record.revision,
                        now_ms.saturating_add(DETACHED_WAKE_RETRY_DELAY_MS),
                        "detached_wake_build_failed",
                    );
                    continue;
                }
            };
        if system_inbound_tx.try_send(wake_msg).is_err() {
            let _ = store.reschedule(
                &record.key,
                record.revision,
                now_ms.saturating_add(DETACHED_WAKE_RETRY_DELAY_MS),
                "detached_wake_enqueue_failed",
            );
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct PostReplyMaintenanceJobPayload {
    ingress: IngressKind,
    source_channel: String,
    user_content: String,
    reply_content: String,
    tool_calls: u32,
    #[serde(default)]
    external_content_used: bool,
    #[serde(default)]
    prompt_recall_intent: crate::memory::PromptRecallIntent,
    #[serde(default)]
    runtime_skill_selected_ids: Vec<String>,
    #[serde(default)]
    task_learning_selected_ids: Vec<String>,
    #[serde(default)]
    reuse_outcome: crate::skills::RuntimeSkillReuseOutcome,
    #[serde(default)]
    reuse_outcome_note: String,
    #[serde(default)]
    first_deferred_at_ms: u64,
    now_secs: u64,
}

impl PostReplyMaintenanceJobPayload {
    fn from_turn(
        msg: &PcMsg,
        reply_content: &str,
        tool_calls: u32,
        external_content_used: bool,
        prompt_recall_intent: crate::memory::PromptRecallIntent,
        runtime_skill_selected_ids: &[String],
        task_learning_selected_ids: &[String],
        reuse_outcome: crate::skills::RuntimeSkillReuseOutcome,
        reuse_outcome_note: &str,
    ) -> Self {
        Self {
            ingress: msg.ingress,
            source_channel: msg.channel.to_string(),
            user_content: truncate_content_to_max(
                &msg.content,
                POST_REPLY_MAINTENANCE_USER_PREVIEW_CHARS,
            )
            .into_owned(),
            reply_content: truncate_content_to_max(
                reply_content,
                POST_REPLY_MAINTENANCE_REPLY_PREVIEW_CHARS,
            )
            .into_owned(),
            tool_calls,
            external_content_used,
            prompt_recall_intent,
            runtime_skill_selected_ids: runtime_skill_selected_ids.to_vec(),
            task_learning_selected_ids: task_learning_selected_ids.to_vec(),
            reuse_outcome,
            reuse_outcome_note: truncate_content_to_max(reuse_outcome_note, 120).into_owned(),
            first_deferred_at_ms: now_unix_ms(),
            now_secs: crate::util::current_unix_secs(),
        }
    }
}
const AGENT_LOOP_TAG: &str = "main";

#[derive(Default)]
struct WorkerLatency {
    context_ms: u128,
    request_semantics_ms: u128,
    mental_privacy_review_ms: u128,
    llm_round_total_ms: u128,
    tool_exec_ms: u128,
    session_write_ms: u128,
    ttft_ms: Option<u128>,
    react_rounds: u32,
    tool_calls: u32,
}

struct WorkerRunTelemetry {
    streamed: bool,
    latency: WorkerLatency,
    delivery: DeliveryReport,
    artifact_bundle: Option<crate::agent::final_reply::ReplyArtifactBundle>,
    any_tool_round_executed: bool,
    any_tool_used: bool,
    tool_round_completion: ToolRoundCompletionTelemetry,
    external_content_used: bool,
    task_execution_used: bool,
    foreground_work_context_present: bool,
    pressure: crate::orchestrator::PressureLevel,
    runtime_mode: crate::runtime::RuntimeModeSnapshot,
    deliberation_class: crate::memory::TurnDeliberationClass,
    reply_surface: ReplySurface,
    prompt_recall_intent: crate::memory::PromptRecallIntent,
    runtime_skill_selected_ids: Vec<String>,
    task_learning_selected_ids: Vec<String>,
    programmable_reasoning_intent: Option<ProgrammableReasoningIntent>,
    counterfactual_analysis: Option<crate::agent::counterfactual::CounterfactualAnalysis>,
    adversarial_arena_adjudication: Option<crate::reasoning::AdversarialArenaAdjudication>,
    subject_state: Option<SubjectState>,
    soul_feedback_projection: Option<SoulFeedbackProjection>,
    mental_privacy_adjudication: Option<crate::memory::MentalPrivacyDisclosureAdjudication>,
    persona_priority_adjudication: Option<PersonaPriorityAdjudication>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct ToolRoundCompletionTelemetry {
    had_mutating_effects: bool,
    had_visible_outbound_side_effects: bool,
    blocker: Option<crate::agent::WorkflowBlocker>,
}

fn build_turn_observation_ledger(
    final_outcome: &str,
    is_interrupt: bool,
    telemetry: &WorkerRunTelemetry,
) -> Option<TurnObservationLedger> {
    let execution_class = if is_interrupt {
        TurnExecutionClass::Interrupted
    } else if telemetry.task_execution_used {
        TurnExecutionClass::TaskExecution
    } else if telemetry.any_tool_round_executed {
        TurnExecutionClass::ToolAssisted
    } else {
        TurnExecutionClass::DirectReply
    };
    let tool_path = if telemetry.task_execution_used {
        "task_execution"
    } else if telemetry.any_tool_round_executed {
        if telemetry.delivery.current_primary_delivered {
            "tool_primary_delivery"
        } else {
            "tool_reply"
        }
    } else if is_interrupt {
        "interrupt"
    } else {
        ""
    };
    let observation = TurnObservationLedger {
        execution_class,
        deliberation_class: telemetry.deliberation_class,
        final_outcome: normalize_turn_observation_text(final_outcome),
        pressure: telemetry.pressure.into(),
        mode: TurnModeSnapshotLedger {
            current_mode: telemetry.runtime_mode.current_mode.as_str().to_string(),
            allow_non_voice_outbound: telemetry
                .runtime_mode
                .action_budget
                .allow_non_voice_outbound,
            allow_idle_self_runtime: telemetry.runtime_mode.action_budget.allow_idle_self_runtime,
        },
        tool_path: TurnToolPathLedger {
            path: tool_path.to_string(),
            tool_calls: telemetry.latency.tool_calls,
            react_rounds: telemetry.latency.react_rounds,
            current_primary_delivered: telemetry.delivery.current_primary_delivered,
        },
        blocker: telemetry
            .tool_round_completion
            .blocker
            .as_ref()
            .map(|blocker| TurnBlockerLedger {
                kind: blocker.outcome_kind().as_str().to_string(),
                failed_calls: 1,
                total_calls: telemetry.latency.tool_calls.max(1),
            }),
    };
    observation.is_meaningful().then_some(observation)
}

struct PreparedWorkerConversation {
    runtime_carry: Box<PromptRuntimeCarry>,
    subject_state: Option<Box<SubjectState>>,
    soul_feedback_projection: Option<Box<SoulFeedbackProjection>>,
    system: String,
    messages: Vec<Message>,
    system_scratch: String,
    deliberation_gate: TurnDeliberationGate,
    interactive_fast_path: bool,
    allow_tool_round_recall_refill: bool,
    prompt_memory_system_budget: usize,
    pressure: crate::orchestrator::PressureLevel,
    request_semantics: RequestSemantics,
    active_task_context_present: bool,
    governed_memory_evidence_present: bool,
    mental_privacy_adjudication: Option<Box<crate::memory::MentalPrivacyDisclosureAdjudication>>,
    persona_priority_adjudication: Option<Box<PersonaPriorityAdjudication>>,
}

struct ToolCallExecutionResult {
    result_owned: String,
    failure_kind: Option<super::tool_outcome::ToolFailureKind>,
    blocker: Option<crate::agent::WorkflowBlocker>,
    call_succeeded: bool,
    protocol_violation: bool,
    had_mutating_effects: bool,
    had_visible_outbound_side_effects: bool,
    current_chat_primary_artifact: Option<crate::agent::final_reply::ReplyArtifactBundle>,
}

struct ToolUseRoundExecutionOutput {
    truncated: bool,
    round_tool_success: bool,
    used_external_content: bool,
    had_mutating_effects: bool,
    had_visible_outbound_side_effects: bool,
    protocol_repair_exhausted: bool,
    artifact_bundle: Option<crate::agent::final_reply::ReplyArtifactBundle>,
    omitted_evidence_count: usize,
    successful_tool_names: Vec<String>,
    blocker: Option<crate::agent::WorkflowBlocker>,
}

fn merge_reply_artifact_bundle(
    slot: &mut Option<crate::agent::final_reply::ReplyArtifactBundle>,
    next: crate::agent::final_reply::ReplyArtifactBundle,
) -> Result<()> {
    match slot {
        None => {
            *slot = Some(next);
            Ok(())
        }
        Some(existing) if *existing == next => Ok(()),
        Some(_) => Err(crate::error::Error::config(
            "reply_artifact_bundle_conflict",
            "multiple distinct current-chat primary artifacts were declared in one turn",
        )),
    }
}

fn mark_ttft_if_visible(latency: &mut WorkerLatency, worker_start: Instant, content: &str) {
    if latency.ttft_ms.is_none() && !content.trim().is_empty() {
        latency.ttft_ms = Some(worker_start.elapsed().as_millis());
    }
}

fn build_json_error_object(message: &str) -> String {
    let mut out = String::with_capacity(message.len().saturating_add(16));
    out.push_str("{\"error\":");
    push_json_string_escaped(&mut out, message);
    out.push('}');
    out
}

fn generate_task_run_id(msg: &PcMsg) -> String {
    let ts = now_unix_ms() & 0x00ff_ffff_ffff;
    let mut hasher = DefaultHasher::new();
    msg.channel.hash(&mut hasher);
    msg.chat_id.hash(&mut hasher);
    msg.content.hash(&mut hasher);
    let short = hasher.finish() & 0xffff;
    format!("tr{ts:010x}{short:04x}")
}

fn build_task_step_request(
    record: &TaskRunRecord,
    step: &TaskStep,
    artifacts: &[TaskArtifactRecord],
) -> String {
    let mut out = String::new();
    out.push_str("Internal task execution step.\n");
    out.push_str("Do not answer the user directly. Complete only the current step.\n");
    out.push_str(&format!("Run id: {}\n", record.run.run_id));
    out.push_str(&format!("Task title: {}\n", record.run.title));
    out.push_str(&format!("Goal: {}\n", record.plan.goal));
    out.push_str(&format!(
        "Completion definition: {}\n",
        record.plan.completion_definition
    ));
    out.push_str(&format!(
        "Current step: {} [{}]\nInstruction: {}\n",
        step.title, step.step_id, step.instruction
    ));
    out.push_str(&format!(
        "Tool budget: {} | Retry budget: {} | Attempt: {}\n",
        step.tool_budget, step.retry_budget, step.attempt_count
    ));
    if !step.expected_artifacts.is_empty() {
        out.push_str("Expected artifacts:\n");
        for item in &step.expected_artifacts {
            out.push_str("- ");
            out.push_str(item);
            out.push('\n');
        }
    }
    if !step.review_criteria.is_empty() {
        out.push_str("Review criteria:\n");
        for item in &step.review_criteria {
            out.push_str("- ");
            out.push_str(item);
            out.push('\n');
        }
    }
    if !artifacts.is_empty() {
        out.push_str("Recent task artifacts:\n");
        for artifact in artifacts.iter().take(TASK_EXECUTION_ARTIFACT_PREVIEW_LIMIT) {
            out.push_str(&format!(
                "- {:?} {}: {}\n",
                artifact.artifact.kind, artifact.artifact.artifact_id, artifact.artifact.summary
            ));
        }
    }
    out.push_str(
        "Return only the step result, blocker, or concrete evidence gathered in this step.",
    );
    out
}

fn build_task_review_request(
    record: &TaskRunRecord,
    step: &TaskStep,
    step_result_artifact: &TaskArtifactRecord,
    existing_artifacts: &[TaskArtifactRecord],
) -> String {
    let mut out = String::new();
    out.push_str("Review the latest task step.\n");
    out.push_str(&format!("Run id: {}\n", record.run.run_id));
    out.push_str(&format!("Task title: {}\n", record.run.title));
    out.push_str(&format!("Goal: {}\n", record.plan.goal));
    out.push_str(&format!(
        "Completion definition: {}\n",
        record.plan.completion_definition
    ));
    out.push_str(&format!(
        "Current step: {} [{}]\nInstruction: {}\n",
        step.title, step.step_id, step.instruction
    ));
    if !step.review_criteria.is_empty() {
        out.push_str("Review criteria:\n");
        for criterion in &step.review_criteria {
            out.push_str("- ");
            out.push_str(criterion);
            out.push('\n');
        }
    }
    out.push_str("Current step result artifact:\n");
    out.push_str(&format!(
        "- {} | {}\n",
        step_result_artifact.artifact.artifact_id, step_result_artifact.artifact.summary
    ));
    out.push_str(step_result_artifact.content.trim());
    out.push('\n');
    if !existing_artifacts.is_empty() {
        out.push_str("Existing task workspace artifacts you may reference by artifact id:\n");
        for artifact in existing_artifacts
            .iter()
            .take(TASK_EXECUTION_ARTIFACT_PREVIEW_LIMIT)
        {
            out.push_str(&format!(
                "- {} | {:?} | {}\n",
                artifact.artifact.artifact_id, artifact.artifact.kind, artifact.artifact.summary
            ));
        }
    }
    out.push_str(
        "Decide whether the step passed, needs retry, requires plan revision, should abort, or should finish partially. Also classify durable facts, reusable procedures, evidence-only material, and transient artifact ids for routing.",
    );
    out
}

fn build_task_finisher_request(record: &TaskRunRecord, artifacts: &[TaskArtifactRecord]) -> String {
    let mut out = String::new();
    out.push_str("Finalize the task run for the user.\n");
    out.push_str(&format!("Run id: {}\n", record.run.run_id));
    out.push_str(&format!("Status: {:?}\n", record.run.status));
    out.push_str(&format!("Task title: {}\n", record.run.title));
    out.push_str(&format!("Goal: {}\n", record.plan.goal));
    out.push_str(&format!(
        "Completion definition: {}\n",
        record.plan.completion_definition
    ));
    if !record.run.final_summary.is_empty() {
        out.push_str(&format!(
            "Execution summary: {}\n",
            record.run.final_summary
        ));
    }
    if !record.run.failure_reason.is_empty() {
        out.push_str(&format!(
            "Failure or blocker: {}\n",
            record.run.failure_reason
        ));
    }
    out.push_str("Step status:\n");
    for step in &record.plan.ordered_steps {
        out.push_str(&format!(
            "- [{}] {:?}: {}",
            step.step_id, step.status, step.title
        ));
        if !step.last_review_summary.is_empty() {
            out.push_str(" | ");
            out.push_str(step.last_review_summary.trim());
        }
        out.push('\n');
    }
    if !artifacts.is_empty() {
        out.push_str("Task artifacts:\n");
        for artifact in artifacts.iter().take(TASK_EXECUTION_ARTIFACT_PREVIEW_LIMIT) {
            out.push_str(&format!(
                "- {:?}: {}\n",
                artifact.artifact.kind, artifact.content
            ));
        }
    }
    out.push_str("Write the final user-facing reply now.");
    out
}

fn build_task_artifact_record(
    run_id: &str,
    step_id: &str,
    kind: TaskArtifactKind,
    content: &str,
    provenance: &str,
    sequence: usize,
    now_secs: u64,
) -> TaskArtifactRecord {
    let artifact_id = format!("a{sequence:02}");
    let content_ref = if cfg!(any(target_arch = "xtensa", target_arch = "riscv32")) {
        format!("x/a/{}_{}.j", run_id, artifact_id)
    } else {
        format!("memory/task_artifacts/{run_id}/{artifact_id}.json")
    };
    TaskArtifactRecord {
        artifact: TaskArtifact {
            artifact_id,
            run_id: run_id.to_string(),
            step_id: step_id.to_string(),
            kind,
            summary: summarize_task_artifact_content(content),
            content_ref,
            provenance: truncate_content_to_max(provenance, 240).into_owned(),
            created_at: now_secs,
        },
        content: truncate_content_to_max(content.trim(), 4 * 1024).into_owned(),
    }
}

fn build_task_ledger_entry(
    run_id: &str,
    step_id: &str,
    kind: TaskLedgerKind,
    run_status: TaskRunStatus,
    message: &str,
    sequence: u32,
    now_secs: u64,
) -> TaskExecutionLedgerEntry {
    TaskExecutionLedgerEntry {
        sequence,
        run_id: run_id.to_string(),
        step_id: step_id.to_string(),
        kind,
        run_status,
        message: truncate_content_to_max(message.trim(), 320).into_owned(),
        recorded_at: now_secs,
    }
}

/// 将文本按 UTF-8 边界追加到 dst，确保总字节不超过 max_bytes。
/// 返回 true 表示本次发生截断（达到上限）。
fn push_bounded_utf8(dst: &mut String, text: &str, max_bytes: usize) -> bool {
    if dst.len() >= max_bytes {
        return true;
    }
    let remain = max_bytes - dst.len();
    if text.len() <= remain {
        dst.push_str(text);
        return false;
    }
    let mut end = 0usize;
    for (i, ch) in text.char_indices() {
        let next = i + ch.len_utf8();
        if next > remain {
            break;
        }
        end = next;
    }
    if end > 0 {
        dst.push_str(&text[..end]);
    }
    true
}

/// 对 (tool_name, args) 做稳定哈希，用于重复工具调用检测。
fn hash_tool_call(name: &str, args: &str) -> u64 {
    let mut h = DefaultHasher::new();
    name.hash(&mut h);
    0x9e37_79b9_7f4a_7c15u64.hash(&mut h);
    args.hash(&mut h);
    h.finish()
}

fn tool_result_status_attr(call_failed: bool) -> &'static str {
    if call_failed {
        "error"
    } else {
        "ok"
    }
}

fn failure_kind_attr(
    failure_kind: Option<super::tool_outcome::ToolFailureKind>,
) -> Option<&'static str> {
    failure_kind.map(|kind| super::workflow_outcome_kind_from_tool_failure_kind(kind).as_str())
}

struct ToolResultBlock<'a> {
    call_id: &'a str,
    tool_name: &'a str,
    status: &'a str,
    failure: Option<&'a str>,
    repeat_count: usize,
    content: &'a str,
}

fn append_tool_result_block(
    dst: &mut String,
    block: ToolResultBlock<'_>,
    max_bytes: usize,
) -> bool {
    if push_bounded_utf8(dst, "<tool_result id=\"", max_bytes)
        || push_bounded_utf8(dst, block.call_id, max_bytes)
        || push_bounded_utf8(dst, "\" tool=\"", max_bytes)
        || push_bounded_utf8(dst, block.tool_name, max_bytes)
        || push_bounded_utf8(dst, "\" status=\"", max_bytes)
        || push_bounded_utf8(dst, block.status, max_bytes)
    {
        return true;
    }
    if let Some(failure) = block.failure {
        if push_bounded_utf8(dst, "\" failure=\"", max_bytes)
            || push_bounded_utf8(dst, failure, max_bytes)
        {
            return true;
        }
    }
    if block.repeat_count > 1 {
        let repeat_attr = block.repeat_count.to_string();
        if push_bounded_utf8(dst, "\" repeat_count=\"", max_bytes)
            || push_bounded_utf8(dst, &repeat_attr, max_bytes)
        {
            return true;
        }
    }
    push_bounded_utf8(dst, "\">\n", max_bytes)
        || push_bounded_utf8(dst, block.content, max_bytes)
        || push_bounded_utf8(dst, "\n</tool_result>", max_bytes)
}

fn append_surface_evidence_block(
    dst: &mut String,
    reply_surface: ReplySurface,
    evidence_lines: &[String],
    omitted_count: usize,
    max_bytes: usize,
) -> bool {
    if evidence_lines.is_empty() {
        return false;
    }
    let Some(authority) = reply_surface.evidence_authority() else {
        return false;
    };
    let mut open_tag = String::with_capacity(96);
    let _ = writeln!(
        &mut open_tag,
        "<surface_evidence surface=\"{}\" authority=\"{}\">",
        reply_surface.as_str(),
        authority
    );
    if push_bounded_utf8(dst, open_tag.as_str(), max_bytes) {
        return true;
    }
    for line in evidence_lines {
        if push_bounded_utf8(dst, line, max_bytes) || push_bounded_utf8(dst, "\n", max_bytes) {
            return true;
        }
    }
    if omitted_count > 0 {
        let mut more = String::with_capacity(56);
        let mut count_buf = [0u8; 20];
        more.push_str("- [more] ");
        more.push_str(usize_to_decimal_buf(&mut count_buf, omitted_count));
        more.push_str(" additional successful tool result(s)");
        if push_bounded_utf8(dst, &more, max_bytes) || push_bounded_utf8(dst, "\n", max_bytes) {
            return true;
        }
    }
    push_bounded_utf8(dst, "</surface_evidence>", max_bytes)
}

fn render_surface_evidence_block(
    reply_surface: ReplySurface,
    evidence_lines: &[String],
    omitted_count: usize,
) -> String {
    let estimated = evidence_lines.iter().map(String::len).sum::<usize>() + 96;
    let mut out = String::with_capacity(estimated);
    let _ = append_surface_evidence_block(
        &mut out,
        reply_surface,
        evidence_lines,
        omitted_count,
        usize::MAX,
    );
    out
}

fn append_memory_grounding_block(dst: &mut String, grounding: &str, max_bytes: usize) -> bool {
    push_bounded_utf8(dst, "<memory_grounding>\n", max_bytes)
        || push_bounded_utf8(dst, grounding, max_bytes)
        || push_bounded_utf8(dst, "\n</memory_grounding>", max_bytes)
}

fn render_memory_grounding_block(grounding: &str) -> String {
    let mut out = String::with_capacity(grounding.len().saturating_add(48));
    let _ = append_memory_grounding_block(&mut out, grounding, usize::MAX);
    out
}

fn clone_bounded_utf8(input: &str, max_bytes: usize) -> (String, bool) {
    let mut out = String::with_capacity(input.len().min(max_bytes));
    let truncated = push_bounded_utf8(&mut out, input, max_bytes);
    (out, truncated)
}

fn append_rendered_tool_section(dst: &mut String, section: &str, max_bytes: usize) -> bool {
    if section.is_empty() {
        return false;
    }
    let mut truncated = false;
    if !dst.ends_with('\n') {
        truncated |= push_bounded_utf8(dst, "\n", max_bytes);
    }
    truncated || push_bounded_utf8(dst, section, max_bytes)
}

fn assemble_tool_round_user_message(
    raw_results: &str,
    raw_results_truncated: bool,
    evidence_block: Option<&str>,
    memory_block: Option<&str>,
    max_bytes: usize,
) -> (String, bool) {
    let evidence_reserve = evidence_block
        .map(|block| block.len().min(TOOL_EVIDENCE_RESERVED_BYTES))
        .unwrap_or(0);
    let (mut memory, mut memory_truncated) = memory_block
        .map(|block| clone_bounded_utf8(block, TOOL_MEMORY_GROUNDING_MAX_BYTES))
        .unwrap_or_else(|| (String::new(), false));

    let mut raw_budget = max_bytes.saturating_sub(
        evidence_reserve
            .saturating_add(memory.len())
            .saturating_add(TOOL_RESULTS_TRUNCATED_MARKER.len()),
    );
    if raw_budget < TOOL_RESULT_RAW_MIN_BYTES {
        if !memory.is_empty() {
            let shrink = (TOOL_RESULT_RAW_MIN_BYTES - raw_budget).min(memory.len());
            let target = memory.len().saturating_sub(shrink);
            let (bounded, truncated) = clone_bounded_utf8(memory.as_str(), target);
            memory = bounded;
            memory_truncated |= truncated || shrink > 0;
        }
        raw_budget = max_bytes.saturating_sub(
            evidence_reserve
                .saturating_add(memory.len())
                .saturating_add(TOOL_RESULTS_TRUNCATED_MARKER.len()),
        );
    }
    raw_budget = raw_budget.max(TOOL_RESULTS_PREFIX.len());

    let (mut out, raw_truncated) = clone_bounded_utf8(raw_results, raw_budget);
    let mut truncated = raw_results_truncated || raw_truncated || memory_truncated;

    if let Some(block) = evidence_block {
        truncated |= append_rendered_tool_section(&mut out, block, max_bytes);
    }
    if !memory.is_empty() {
        truncated |= append_rendered_tool_section(&mut out, memory.as_str(), max_bytes);
    }
    if truncated && out.len() < max_bytes {
        let _ = push_bounded_utf8(&mut out, TOOL_RESULTS_TRUNCATED_MARKER, max_bytes);
    }
    (out, truncated)
}

fn build_memory_grounding_text(
    summary_text: Option<&str>,
    long_term_memory_text: Option<&str>,
) -> Option<String> {
    let mut out = String::new();
    if let Some(summary) = summary_text
        .map(str::trim)
        .filter(|summary| !summary.is_empty())
    {
        let preview = truncate_content_to_max(summary, MEMORY_GROUNDING_SUMMARY_PREVIEW_CHARS);
        let _ = writeln!(out, "[summary] {}", preview.as_ref());
    }
    if let Some(long_term) = long_term_memory_text {
        let bullets = extract_long_term_memory_bullets(long_term);
        if !bullets.is_empty() {
            let joined = bullets.join(" | ");
            let preview =
                truncate_content_to_max(&joined, MEMORY_GROUNDING_LONG_TERM_PREVIEW_CHARS);
            let _ = writeln!(out, "[long_term] {}", preview.as_ref());
        }
    }
    let trimmed = out.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn extract_long_term_memory_bullets(block: &str) -> Vec<String> {
    let mut bullets = Vec::new();
    for line in block.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with("- ") {
            continue;
        }
        bullets.push(trimmed.to_string());
        if bullets.len() >= 3 {
            break;
        }
    }
    bullets
}

fn extract_tag_attr<'a>(line: &'a str, attr: &str) -> Option<&'a str> {
    let mut search_from = 0usize;
    let bytes = line.as_bytes();
    while let Some(pos) = line[search_from..].find(attr) {
        let start = search_from + pos;
        let after = start + attr.len();
        if bytes.get(after) == Some(&b'=') && bytes.get(after + 1) == Some(&b'"') {
            let value_start = after + 2;
            let remain = &line[value_start..];
            let end = remain.find('"')?;
            return Some(&remain[..end]);
        }
        search_from = after;
    }
    None
}

fn append_tool_result_summary_header(
    out: &mut String,
    call_id: &str,
    tool_name: &str,
    status: &str,
    failure: Option<&str>,
    repeat_count: Option<&str>,
) {
    out.push('[');
    out.push_str(call_id);
    out.push_str("] ");
    out.push_str(tool_name);
    out.push_str(" status=");
    out.push_str(status);
    if let Some(failure) = failure {
        out.push_str(" failure=");
        out.push_str(failure);
    }
    if let Some(repeat_count) = repeat_count {
        out.push_str(" repeat=");
        out.push_str(repeat_count);
    }
    out.push_str(": ");
}

/// 将早期轮次的工具结果压缩为「预览 + 总字节数」摘要，保留语义锚点、不丢轮次结构。
fn summarize_tool_results(content: &str) -> String {
    let body = content.strip_prefix(TOOL_RESULTS_PREFIX).unwrap_or(content);
    let mut out = String::with_capacity(512);
    out.push_str("Tool results (prior round):\n");
    let mut wrote_any = false;
    let mut lines = body.lines().peekable();
    while let Some(line) = lines.next() {
        if line.starts_with("<tool_result ") {
            let call_id = extract_tag_attr(line, "id").unwrap_or("unknown");
            let tool_name = extract_tag_attr(line, "tool").unwrap_or("unknown");
            let status = extract_tag_attr(line, "status").unwrap_or("unknown");
            let failure = extract_tag_attr(line, "failure");
            let repeat_count = extract_tag_attr(line, "repeat_count");
            let mut block_body = String::new();
            for next in lines.by_ref() {
                if next == "</tool_result>" {
                    break;
                }
                if !block_body.is_empty() {
                    block_body.push('\n');
                }
                block_body.push_str(next);
            }
            let preview = build_tool_result_preview(&block_body);
            let mut header = String::with_capacity(
                call_id
                    .len()
                    .saturating_add(tool_name.len())
                    .saturating_add(status.len())
                    .saturating_add(failure.map_or(0, str::len))
                    .saturating_add(repeat_count.map_or(0, str::len))
                    .saturating_add(32),
            );
            append_tool_result_summary_header(
                &mut header,
                call_id,
                tool_name,
                status,
                failure,
                repeat_count,
            );
            if block_body.chars().count() <= TOOL_RESULT_PREVIEW_CHARS {
                let _ = writeln!(out, "{header}{preview}");
            } else {
                let _ = writeln!(out, "{header}{}[{} bytes total]", preview, block_body.len());
            }
            wrote_any = true;
            continue;
        }
        if line == "<tool_round_guidance>" {
            for next in lines.by_ref() {
                if next == "</tool_round_guidance>" {
                    break;
                }
            }
            continue;
        }
        if line.starts_with("<surface_evidence ") {
            let surface = extract_tag_attr(line, "surface").unwrap_or("unknown");
            let authority = extract_tag_attr(line, "authority").unwrap_or("unknown");
            let _ = writeln!(out, "[surface={} authority={}]", surface, authority);
            for next in lines.by_ref() {
                if next == "</surface_evidence>" {
                    break;
                }
                let trimmed = next.trim();
                if trimmed.is_empty() {
                    continue;
                }
                let _ = writeln!(
                    out,
                    "[evidence] {}",
                    build_tool_evidence_preview(trimmed).unwrap_or_default()
                );
            }
            wrote_any = true;
            continue;
        }
        if line == "<tool_evidence_summary>" {
            for next in lines.by_ref() {
                if next == "</tool_evidence_summary>" {
                    break;
                }
                let trimmed = next.trim();
                if trimmed.is_empty() {
                    continue;
                }
                let _ = writeln!(
                    out,
                    "[evidence] {}",
                    build_tool_evidence_preview(trimmed).unwrap_or_default()
                );
                wrote_any = true;
            }
            continue;
        }
        if line == "<memory_grounding>" {
            let mut memory = String::new();
            for next in lines.by_ref() {
                if next == "</memory_grounding>" {
                    break;
                }
                let trimmed = next.trim();
                if trimmed.is_empty() {
                    continue;
                }
                if !memory.is_empty() {
                    memory.push(' ');
                }
                memory.push_str(trimmed);
            }
            let _ = writeln!(
                out,
                "[memory] {}",
                truncate_content_to_max(&memory, 180).as_ref()
            );
            wrote_any = true;
            continue;
        }
        if let Some(idx) = line.find("]: ") {
            let id_part = &line[..idx + 3];
            let first_val = &line[idx + 3..];
            let mut block_body = String::new();
            block_body.push_str(first_val);
            let mut total_bytes = first_val.len();
            while let Some(next) = lines.peek().copied() {
                if next.contains("]: ") && next.starts_with('[') {
                    break;
                }
                total_bytes = total_bytes.saturating_add(1).saturating_add(next.len());
                block_body.push('\n');
                block_body.push_str(next);
                let _ = lines.next();
            }
            let preview = build_tool_result_preview(&block_body);
            if block_body.chars().count() > TOOL_RESULT_PREVIEW_CHARS {
                let _ = writeln!(out, "{}{}[{} bytes total]", id_part, preview, total_bytes);
            } else {
                let _ = writeln!(out, "{}{}", id_part, preview);
            }
            wrote_any = true;
        }
    }
    if !wrote_any {
        let _ = writeln!(out, "[{} bytes total, format not parsed]", body.len());
    }
    out
}

fn take_suffix_chars(text: &str, max_chars: usize) -> &str {
    if max_chars == 0 {
        return "";
    }
    let total_chars = text.chars().count();
    if total_chars <= max_chars {
        return text;
    }
    let keep_from = total_chars.saturating_sub(max_chars);
    match text.char_indices().nth(keep_from) {
        Some((idx, _)) => &text[idx..],
        None => text,
    }
}

fn collapse_inline_whitespace(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    let mut normalized = String::with_capacity(trimmed.len().min(256));
    let mut pending_space = false;
    for ch in trimmed.chars() {
        if ch.is_whitespace() {
            pending_space = !normalized.is_empty();
            continue;
        }
        if pending_space {
            normalized.push(' ');
            pending_space = false;
        }
        normalized.push(ch);
    }
    if normalized.is_empty() {
        return None;
    }
    Some(normalized)
}

fn build_head_tail_preview(text: &str, max_chars: usize, tail_chars: usize) -> Option<String> {
    let normalized = collapse_inline_whitespace(text)?;
    let total_chars = normalized.chars().count();
    if total_chars <= max_chars {
        return Some(normalized);
    }
    let head_chars = max_chars
        .saturating_sub(tail_chars)
        .saturating_sub(5)
        .max(1);
    let head = truncate_content_to_max(&normalized, head_chars).into_owned();
    let tail = take_suffix_chars(&normalized, tail_chars);
    Some(format!("{head} ... {tail}"))
}

fn build_tool_result_preview(text: &str) -> String {
    build_head_tail_preview(text, TOOL_RESULT_PREVIEW_CHARS, TOOL_RESULT_TAIL_CHARS)
        .unwrap_or_default()
}

fn build_tool_evidence_preview(text: &str) -> Option<String> {
    build_head_tail_preview(text, TOOL_EVIDENCE_PREVIEW_CHARS, TOOL_EVIDENCE_TAIL_CHARS)
}

fn build_assistant_compact_preview(text: &str) -> Option<String> {
    build_head_tail_preview(
        text,
        ASSISTANT_COMPACT_PREVIEW_CHARS,
        ASSISTANT_COMPACT_TAIL_CHARS,
    )
}

fn build_tool_evidence_line(call_id: &str, tool_name: &str, content: &str) -> Option<String> {
    let preview = build_tool_evidence_preview(content)?;
    Some(format!("- [{call_id}] {tool_name}: {preview}"))
}

/// 滑动窗口：保留最近 `REACT_FULL_MSGS_KEPT` 条 ReAct 追加消息完整，更早的 assistant / tool 结果做机械摘要。
fn compact_early_tool_rounds(messages: &mut [Message], initial_count: usize) {
    let react_start = initial_count;
    let react_end = messages.len();
    let react_count = react_end.saturating_sub(react_start);
    if react_count <= REACT_FULL_MSGS_KEPT {
        return;
    }
    let compact_end = react_end - REACT_FULL_MSGS_KEPT;
    for msg in messages[react_start..compact_end].iter_mut() {
        if msg.content.starts_with(TOOL_RESULTS_PREFIX) && msg.content.len() > 128 {
            let s = summarize_tool_results(&msg.content);
            msg.content = s;
        } else if msg.role.as_ref() == "assistant" && msg.content.len() > 200 {
            if let Some(preview) = build_assistant_compact_preview(&msg.content) {
                if preview.chars().count() < msg.content.chars().count() {
                    msg.content = preview;
                    msg.content.push_str(" [compressed]");
                }
            }
        }
    }
}

fn outbound_provenance_value(value: &str) -> &str {
    if value.trim().is_empty() {
        "-"
    } else {
        value
    }
}

const PRIMARY_OUTBOUND_ENQUEUE_RETRY_DELAY_MS: u64 = 50;
const PRIMARY_OUTBOUND_ENQUEUE_LOG_EVERY: u32 = 20;

fn try_send_outbound(outbound_tx: &OutboundTx, msg: PcMsg, log_prefix: &str) -> bool {
    let req_id = msg.req_id.clone().unwrap_or_default();
    let channel = msg.channel.clone();
    let chat_id = msg.chat_id.clone();
    let source_transport = msg.source_transport;
    let platform_message_id = msg.platform_message_id.clone();
    let platform_event_id = msg.platform_event_id.clone();
    let inbound_dedup_key = msg.inbound_dedup_key.clone();
    let outbound_kind = msg.outbound_kind;
    let mut pending = msg;
    let mut full_attempts = 0u32;
    loop {
        match outbound_tx.try_send(pending) {
            Ok(()) => {
                metrics::record_message_out();
                if outbound_kind == OutboundKind::Primary {
                    log::info!(
                        "[agent] primary_delivery event=outbound_enqueued delivered=true req_id={} channel={} chat_id={}",
                        req_id,
                        channel,
                        chat_id
                    );
                } else if outbound_kind == OutboundKind::Visibility {
                    log::info!(
                        "[agent] foreground_ack event=visibility_enqueued before_llm=true req_id={} channel={} chat_id={}",
                        req_id,
                        channel,
                        chat_id
                    );
                }
                log::info!(
                    "[agent] {} outbound enqueued req_id={} channel={} chat_id={} transport={} platform_message_id={} platform_event_id={} dedup_key={}",
                    log_prefix,
                    req_id,
                    channel,
                    chat_id,
                    source_transport.as_str(),
                    outbound_provenance_value(&platform_message_id),
                    outbound_provenance_value(&platform_event_id),
                    outbound_provenance_value(&inbound_dedup_key)
                );
                return true;
            }
            Err(std::sync::mpsc::TrySendError::Full(msg))
                if !msg.outbound_kind.is_best_effort_delivery() =>
            {
                full_attempts = full_attempts.saturating_add(1);
                if full_attempts == 1
                    || full_attempts.is_multiple_of(PRIMARY_OUTBOUND_ENQUEUE_LOG_EVERY)
                {
                    log::warn!(
                        "[agent] {} outbound queue full for reliable delivery req_id={} channel={} chat_id={} outbound_kind={}, applying backpressure",
                        log_prefix,
                        req_id,
                        channel,
                        chat_id,
                        msg.outbound_kind.as_str()
                    );
                }
                crate::platform::task_wdt::feed_current_task();
                std::thread::sleep(std::time::Duration::from_millis(
                    PRIMARY_OUTBOUND_ENQUEUE_RETRY_DELAY_MS,
                ));
                crate::platform::task_wdt::feed_current_task();
                pending = msg;
            }
            Err(e) => {
                metrics::record_outbound_enqueue_fail();
                log::error!(
                    "[agent] {} outbound enqueue failed req_id={} channel={} chat_id={} transport={} platform_message_id={} platform_event_id={} dedup_key={}: {}",
                    log_prefix,
                    req_id,
                    channel,
                    chat_id,
                    source_transport.as_str(),
                    outbound_provenance_value(&platform_message_id),
                    outbound_provenance_value(&platform_event_id),
                    outbound_provenance_value(&inbound_dedup_key),
                    e
                );
                return false;
            }
        }
    }
}

enum GateResult {
    Proceed(Box<PcMsg>),
    Skipped,
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
pub(super) fn log_user_turn_memory_checkpoint(stage: &'static str, msg: &PcMsg) {
    log_user_turn_memory_checkpoint_parts(stage, msg.ingress, &msg.channel, &msg.chat_id);
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
pub(super) fn log_user_turn_memory_checkpoint_parts(
    stage: &'static str,
    ingress: IngressKind,
    channel: &str,
    chat_id: &str,
) {
    if ingress != IngressKind::User {
        return;
    }
    let snap = crate::orchestrator::memory_snapshot_live();
    crate::orchestrator::apply_memory_snapshot(snap);
    let pressure = crate::orchestrator::current_pressure();
    let tls_fragmentation = crate::orchestrator::current_tls_fragmentation_risk();
    log::info!(
        "[agent] turn memory checkpoint stage={} channel={} chat_id={} internal_free={} internal_min={} largest_block={} spiram_free={} spiram_min={} spiram_largest={} pressure={:?} tls_fragmentation={:?}",
        stage,
        channel,
        chat_id,
        snap.heap_free_internal,
        snap.heap_min_free_internal,
        snap.heap_largest_block,
        snap.heap_free_spiram,
        snap.heap_min_free_spiram,
        snap.heap_largest_block_spiram,
        pressure,
        tls_fragmentation
    );
}

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub(super) fn log_user_turn_memory_checkpoint(_stage: &'static str, _msg: &PcMsg) {}

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub(super) fn log_user_turn_memory_checkpoint_parts(
    _stage: &'static str,
    _ingress: IngressKind,
    _channel: &str,
    _chat_id: &str,
) {
}

#[allow(clippy::too_many_arguments)]
fn handle_llm_gate(
    mut msg: PcMsg,
    loc: UiLocale,
    msg_key: u64,
    user_inbound_tx: &UserInboundTx,
    system_inbound_tx: &SystemInboundTx,
    outbound_tx: &OutboundTx,
    config: &AgentLoopConfig,
    defer_tracker: &mut HashMap<u64, (u8, Instant)>,
    low_mem_defer_log: &mut Option<(Arc<str>, Instant)>,
) -> GateResult {
    crate::orchestrator::refresh_heap_if_stale();
    match crate::orchestrator::can_call_llm_for_channel_pub(&msg.channel) {
        LlmDecision::Proceed => GateResult::Proceed(Box::new(msg)),
        LlmDecision::RetryLater { delay_ms } => {
            background_jobs::handle_admission_defer(
                delay_ms,
                msg,
                msg_key,
                AdmissionDeferContext {
                    source: "llm-retry-later",
                    loc,
                    user_inbound_tx,
                    system_inbound_tx,
                    outbound_tx,
                    config,
                    defer_tracker,
                    low_mem_defer_log,
                },
            );
            GateResult::Skipped
        }
        LlmDecision::Degrade { reason } => {
            if msg.ingress == IngressKind::System {
                log::info!("[agent] system task degraded, retry later: {}", reason);
                msg.enqueue_ts_ms = now_unix_ms();
                if let Err(std::sync::mpsc::TrySendError::Full(m)) =
                    try_send_inbound_msg(msg, user_inbound_tx, system_inbound_tx)
                {
                    if let Err(error) = config.runtime.pending_retry_store.save_pending_retry(&m) {
                        metrics::record_error_by_stage(error.metrics_stage());
                        log::error!(
                            "[agent] system degrade pending_retry save failed chat_id={}: {}",
                            m.chat_id,
                            error
                        );
                    }
                }
            } else {
                log::info!("[agent] LLM degraded: {}", reason);
                match PcMsg::new_outbound_reply_to(&msg, tr(UiMessage::LowMemoryUserDefer, loc)) {
                    Ok(out) => {
                        let _ = try_send_outbound(outbound_tx, out, "llm-degrade");
                    }
                    Err(error) => {
                        metrics::record_error_by_stage(error.metrics_stage());
                        log::error!(
                            "[agent] failed to build llm-degrade reply channel={} chat_id={}: {}",
                            msg.channel,
                            msg.chat_id,
                            error
                        );
                    }
                }
            }
            GateResult::Skipped
        }
    }
}

#[cold]
#[inline(never)]
#[allow(clippy::too_many_arguments)]
fn log_agent_latency_summary(
    worker_lane_tag: &str,
    req_id: &str,
    channel: &str,
    chat_id: &str,
    queue_wait_ms: u128,
    admission_ms: u128,
    worker_prepare_ms: u128,
    worker_latency: &WorkerLatency,
    llm_ms: u128,
    outbound_enqueue_ms: u128,
    reply_handoff_ms: u128,
    post_reply_ms: u128,
    total_ms: u128,
    streamed: bool,
    delivered: bool,
    latency_warn_ms: u128,
) {
    if total_ms >= latency_warn_ms {
        log::warn!(
            "[latency][agent:{}] req_id={} channel={} chat_id={} queue_wait_ms={} admission_ms={} worker_prepare_ms={} context_ms={} request_semantics_ms={} mental_privacy_review_ms={} llm_round_total_ms={} tool_exec_ms={} session_write_ms={} llm_ms={} outbound_enqueue_ms={} reply_handoff_ms={} post_reply_ms={} total_ms={} react_rounds={} tool_calls={} ttft_ms={} streamed={} delivered={} level=slow",
            worker_lane_tag,
            req_id,
            channel,
            chat_id,
            queue_wait_ms,
            admission_ms,
            worker_prepare_ms,
            worker_latency.context_ms,
            worker_latency.request_semantics_ms,
            worker_latency.mental_privacy_review_ms,
            worker_latency.llm_round_total_ms,
            worker_latency.tool_exec_ms,
            worker_latency.session_write_ms,
            llm_ms,
            outbound_enqueue_ms,
            reply_handoff_ms,
            post_reply_ms,
            total_ms,
            worker_latency.react_rounds,
            worker_latency.tool_calls,
            worker_latency.ttft_ms.unwrap_or(0),
            streamed,
            delivered
        );
    } else {
        log::info!(
            "[latency][agent:{}] req_id={} channel={} chat_id={} queue_wait_ms={} admission_ms={} worker_prepare_ms={} context_ms={} request_semantics_ms={} mental_privacy_review_ms={} llm_round_total_ms={} tool_exec_ms={} session_write_ms={} llm_ms={} outbound_enqueue_ms={} reply_handoff_ms={} post_reply_ms={} total_ms={} react_rounds={} tool_calls={} ttft_ms={} streamed={} delivered={}",
            worker_lane_tag,
            req_id,
            channel,
            chat_id,
            queue_wait_ms,
            admission_ms,
            worker_prepare_ms,
            worker_latency.context_ms,
            worker_latency.request_semantics_ms,
            worker_latency.mental_privacy_review_ms,
            worker_latency.llm_round_total_ms,
            worker_latency.tool_exec_ms,
            worker_latency.session_write_ms,
            llm_ms,
            outbound_enqueue_ms,
            reply_handoff_ms,
            post_reply_ms,
            total_ms,
            worker_latency.react_rounds,
            worker_latency.tool_calls,
            worker_latency.ttft_ms.unwrap_or(0),
            streamed,
            delivered
        );
    }
}

struct LaneTurnFinalizeContext<'a> {
    worker_lane_tag: &'a str,
    config: &'a AgentLoopConfig,
    system_inbound_tx: &'a SystemInboundTx,
    msg: Box<PcMsg>,
    msg_start: Instant,
    queue_wait_ms: u128,
    admission_ms: u128,
    worker_prepare_ms: u128,
    msg_key: u64,
    turn_ledger: Box<TurnLedger>,
    latency_warn_ms: u128,
}

struct AdmissionDeferContext<'a> {
    source: &'static str,
    loc: UiLocale,
    user_inbound_tx: &'a UserInboundTx,
    system_inbound_tx: &'a SystemInboundTx,
    outbound_tx: &'a OutboundTx,
    config: &'a AgentLoopConfig,
    defer_tracker: &'a mut HashMap<u64, (u8, Instant)>,
    low_mem_defer_log: &'a mut Option<(Arc<str>, Instant)>,
}

/// run_worker_path 返回：当前轮 canonical final reply text。
pub enum WorkerOutcome {
    Content(String),
}

/// Agent 循环的存储与运行参数，由 main 构建并传入 run_agent_loop，减少参数数量。
pub struct AgentLoopConfig {
    pub runtime: crate::RuntimeServices,
    pub get_skill_descriptions: Arc<dyn Fn() -> String + Send + Sync>,
    pub get_capability_package_text: CapabilityPackageTextProvider,
    pub tg_group_activation: Arc<str>,
    pub channel_capability_registry: Arc<crate::ChannelCapabilityRegistry>,
    pub strategy: AgentRunStrategy,
    /// 流式编辑器；仅当前通道支持 stream-edit 时由 main 传入。
    pub stream_editor: Option<Arc<dyn StreamEditor + Send + Sync>>,
    /// 流式编辑器对应的通道名；仅当前消息来自该通道时才允许流式编辑。
    pub stream_editor_channel: Option<Arc<str>>,
    /// Configure UI 本地 SSE broker；HTTP 负责写流，agent 只上报 progress/final。
    pub chat_streams: Arc<crate::chat_stream::ChatStreamBroker>,
    /// 当前 NVS 语言；工具与降级文案按此本地化。
    pub resolve_locale: std::sync::Arc<dyn Fn() -> UiLocale + Send + Sync>,
}

pub trait TypingNotifier: Send {
    fn notify(&mut self, channel: &str, chat_id: &str, http: &mut dyn PlatformHttpClient);
}

/// 单一 agent 执行面：同时消费 user/system 两条入站队列。
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn run_agent_loop(
    http: &mut dyn PlatformHttpClient,
    worker_llm: &(dyn LlmClient + Send + Sync),
    registry: &crate::tools::ToolRegistry,
    config: &AgentLoopConfig,
    user_inbound_tx: UserInboundTx,
    user_inbound_rx: UserInboundRx,
    system_inbound_tx: SystemInboundTx,
    system_inbound_rx: InboundRx,
    outbound_tx: OutboundTx,
    typing_notifier: Option<Box<dyn TypingNotifier>>,
) -> Result<()> {
    run_agent_loop_main(
        http,
        worker_llm,
        registry,
        config,
        user_inbound_tx,
        user_inbound_rx,
        system_inbound_tx,
        system_inbound_rx,
        outbound_tx,
        typing_notifier,
    )
}
enum AgentRecvStatus {
    Message(Box<PcMsg>),
    Timeout,
    Disconnected,
}

const INBOUND_POLL_SLICE_MS: u64 = 200;
const MAX_CONSECUTIVE_USER_MSGS: u8 = 4;

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn run_agent_loop_main(
    http: &mut dyn PlatformHttpClient,
    worker_llm: &(dyn LlmClient + Send + Sync),
    registry: &crate::tools::ToolRegistry,
    config: &AgentLoopConfig,
    user_inbound_tx: UserInboundTx,
    user_inbound_rx: UserInboundRx,
    system_inbound_tx: SystemInboundTx,
    system_inbound_rx: InboundRx,
    outbound_tx: OutboundTx,
    mut typing_notifier: Option<Box<dyn TypingNotifier>>,
) -> Result<()> {
    // Track repeated LLM failure for same request body, avoid infinite retry.
    // Key: u64 hash of (channel, chat_id, content) — avoids per-message format! String alloc.
    // Value: (failure count, last failure time) — entries expire after 5 minutes.
    let mut llm_failure_count: HashMap<u64, (u8, Instant)> = HashMap::new();
    // Reuse per-request tool repeat map to reduce heap churn.
    let mut tool_call_repeat_buf: HashMap<u64, u8> = HashMap::with_capacity(16);
    // Track consecutive defer count per message key to break infinite defer loops.
    let mut defer_tracker: HashMap<u64, (u8, Instant)> = HashMap::new();
    // Throttle "low memory, defer" log per chat_id to avoid log spam.
    let mut low_mem_defer_log: Option<(Arc<str>, Instant)> = None;
    // Periodic GC for llm_failure_count + defer_tracker: evict expired entries every N messages.
    let mut msg_since_gc: u16 = 0;
    const GC_INTERVAL_MSGS: u16 = 50;
    const FAILURE_EXPIRY: Duration = Duration::from_secs(300);
    const DEFER_EXPIRY: Duration = Duration::from_secs(300);
    const LATENCY_WARN_MS: u128 = 3000;
    let mut consecutive_user_msgs = 0u8;

    let recv_timeout = Duration::from_secs(INBOUND_RECV_TIMEOUT_SECS);
    loop {
        if !matches!(
            config.runtime.memory_system_kind.memory_profile(),
            crate::memory::MemoryProfile::Embedded
        ) {
            wake_due_detached_background_work(
                config.runtime.detached_work_store.as_ref(),
                &system_inbound_tx,
                2,
            );
        }
        let prefer_system_once = consecutive_user_msgs >= MAX_CONSECUTIVE_USER_MSGS;
        let mut before_poll = || {
            crate::runtime::service_write_back_tasks();
            #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
            {
                if let Err(error) = crate::runtime::drain_persisted_operator_maintenance_requests(
                    &system_inbound_tx,
                    4,
                ) {
                    log::warn!(
                        "[operator_maintenance] failed to drain persisted requests: {}",
                        error
                    );
                }
            }
        };
        let mut msg = match recv_next_agent_msg(
            &user_inbound_rx,
            &system_inbound_rx,
            recv_timeout,
            prefer_system_once,
            &mut before_poll,
        ) {
            AgentRecvStatus::Message(m) => *m,
            AgentRecvStatus::Timeout => {
                crate::platform::task_wdt::feed_current_task();
                continue;
            }
            AgentRecvStatus::Disconnected => break,
        };
        if msg.ingress == IngressKind::System
            && is_lane_background_job(&msg)
            && !is_detached_work_wake(&msg)
            && should_persist_background_job_as_detached(
                &msg,
                config.runtime.memory_system_kind.memory_profile(),
            )
        {
            match adopt_background_job_as_detached(
                config.runtime.detached_work_store.as_ref(),
                &msg,
                "background_job_adopted",
            ) {
                Ok(true) => continue,
                Ok(false) => {}
                Err(error) => {
                    log::warn!(
                        "[agent] failed to adopt background job channel={} chat_id={}: {}",
                        msg.channel,
                        msg.chat_id,
                        error
                    );
                    continue;
                }
            }
        }
        if msg.ingress == IngressKind::System {
            consecutive_user_msgs = 0;
        } else {
            consecutive_user_msgs = consecutive_user_msgs.saturating_add(1);
        }
        if msg.ingress == IngressKind::User {
            metrics::record_user_message_in();
            metrics::record_user_activity();
        } else {
            metrics::record_system_message_in();
        }
        crate::platform::task_wdt::feed_current_task();
        let loc = (config.resolve_locale)();
        let msg_start = Instant::now();
        msg.ensure_req_id();
        msg.settle_ingress_visibility_ack_claim(Duration::from_millis(
            INGRESS_VISIBILITY_ACK_SETTLE_MS,
        ));
        // Periodic GC: evict expired failure/defer entries to prevent unbounded growth.
        msg_since_gc += 1;
        if msg_since_gc >= GC_INTERVAL_MSGS
            || llm_failure_count.len() > 64
            || defer_tracker.len() > 64
        {
            msg_since_gc = 0;
            let now_gc = Instant::now();
            llm_failure_count.retain(|_, (_, ts)| now_gc.duration_since(*ts) < FAILURE_EXPIRY);
            defer_tracker.retain(|_, (_, ts)| now_gc.duration_since(*ts) < DEFER_EXPIRY);
        }

        let msg_key = {
            let mut hasher = DefaultHasher::new();
            msg.channel.hash(&mut hasher);
            msg.chat_id.hash(&mut hasher);
            msg.content.hash(&mut hasher);
            hasher.finish()
        };
        let now_for_key = Instant::now();
        if llm_failure_count
            .get(&msg_key)
            .map(|(count, ts)| *count >= 3 && now_for_key.duration_since(*ts) < FAILURE_EXPIRY)
            .unwrap_or(false)
        {
            match PcMsg::new_outbound_reply_to(&msg, tr(UiMessage::NodeMaintenance, loc)) {
                Ok(out) => {
                    let _ = try_send_outbound(&outbound_tx, out, "maintenance");
                }
                Err(error) => {
                    metrics::record_error_by_stage(error.metrics_stage());
                    log::error!(
                        "[agent] failed to build maintenance reply channel={} chat_id={}: {}",
                        msg.channel,
                        msg.chat_id,
                        error
                    );
                }
            }
            continue;
        }

        let work_class = classify_system_work(msg.channel.as_ref(), msg.ingress);

        if work_class.is_background_job() && is_lane_background_job(&msg) {
            run_background_job_with_accounting(
                http,
                worker_llm,
                config,
                &user_inbound_tx,
                &system_inbound_tx,
                &outbound_tx,
                loc,
                msg,
            );
            continue;
        }

        let admitted = match admit_turn(
            msg,
            msg_start,
            loc,
            &user_inbound_tx,
            &system_inbound_tx,
            &outbound_tx,
            config,
            &mut defer_tracker,
            &mut low_mem_defer_log,
        ) {
            Some(admitted) => admitted,
            None => continue,
        };
        let ingress_admission::AdmittedTurn {
            mut msg,
            msg_key,
            queue_wait_ms,
            admission_ms,
            _agent_task_guard: mut turn_guard,
        } = admitted;
        log_user_turn_memory_checkpoint("agent_turn_admitted", &msg);
        let turn_started_at_ms = now_unix_ms();
        let mut turn_ledger = build_turn_ledger_start(&msg, turn_started_at_ms);
        persist_turn_ledger(
            config.runtime.turn_ledger_store.as_ref(),
            &crate::memory::relationship_scope_id(&msg.channel, &msg.chat_id),
            &turn_ledger,
            "start",
        );
        if let Some(ref mut notifier) = typing_notifier {
            notifier.notify(&msg.channel, &msg.chat_id, http);
        }
        let worker_prepare_ms = msg_start.elapsed().as_millis().saturating_sub(admission_ms);
        if worker_prepare_ms >= 1000 {
            log::warn!(
                "[latency][agent:{}] req_id={} channel={} chat_id={} queue_wait_ms={} admission_ms={} worker_prepare_ms={} status=pre_worker_slow",
                AGENT_LOOP_TAG,
                msg.req_id.as_deref().unwrap_or_default(),
                msg.channel,
                msg.chat_id,
                queue_wait_ms,
                admission_ms,
                worker_prepare_ms
            );
        }
        let executed = execute_turn_boxed(
            http,
            worker_llm,
            &msg,
            &outbound_tx,
            msg.req_id.as_deref().unwrap_or_default(),
            registry,
            config,
            &mut tool_call_repeat_buf,
            loc,
        );
        log_user_turn_memory_checkpoint("agent_turn_after_execute", &msg);

        let executed = match executed {
            Ok(ok) => ok,
            Err(e) => {
                handle_worker_path_error(
                    e,
                    AGENT_LOOP_TAG,
                    &mut msg,
                    loc,
                    msg_start,
                    queue_wait_ms,
                    admission_ms,
                    worker_prepare_ms,
                    msg_key,
                    &mut llm_failure_count,
                    &user_inbound_tx,
                    &system_inbound_tx,
                    &outbound_tx,
                    config,
                    &mut turn_ledger,
                );
                continue;
            }
        };
        let finalized =
            match finalize_turn_boxed(http, worker_llm, config, &msg, loc, msg_start, executed) {
                Ok(finalized) => finalized,
                Err(e) => {
                    handle_worker_path_error(
                        e,
                        AGENT_LOOP_TAG,
                        &mut msg,
                        loc,
                        msg_start,
                        queue_wait_ms,
                        admission_ms,
                        worker_prepare_ms,
                        msg_key,
                        &mut llm_failure_count,
                        &user_inbound_tx,
                        &system_inbound_tx,
                        &outbound_tx,
                        config,
                        &mut turn_ledger,
                    );
                    continue;
                }
            };
        log_user_turn_memory_checkpoint("agent_turn_after_finalize", &msg);
        let handoff = deliver_turn(&outbound_tx, &msg, finalized.as_ref(), config);
        if handoff.delivered {
            turn_guard.finish_user_visible_delivery_window();
        }
        let checkpoint_ingress = msg.ingress;
        let checkpoint_channel = Arc::clone(&msg.channel);
        let checkpoint_chat_id = Arc::clone(&msg.chat_id);
        complete_turn_boxed(
            Box::new(LaneTurnFinalizeContext {
                worker_lane_tag: AGENT_LOOP_TAG,
                config,
                system_inbound_tx: &system_inbound_tx,
                msg: Box::new(msg),
                msg_start,
                queue_wait_ms,
                admission_ms,
                worker_prepare_ms,
                msg_key,
                turn_ledger: Box::new(turn_ledger),
                latency_warn_ms: LATENCY_WARN_MS,
            }),
            &mut llm_failure_count,
            &mut defer_tracker,
            finalized,
            handoff,
        );
        log_user_turn_memory_checkpoint_parts(
            "agent_turn_after_complete",
            checkpoint_ingress,
            checkpoint_channel.as_ref(),
            checkpoint_chat_id.as_ref(),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::DetachedWorkStore;
    use crate::error::Result;
    use crate::llm::{LlmHttpClient, LlmModelCompat, LlmResponse, StopReason, ToolChoicePolicy};
    use crate::memory::{
        AutonomyStrategyStore, ExecutionState, ExecutionStateStore, FeltSignificanceStore,
        ImportantMessageStore, InnerConflictStore, InnerLifeStore, LongTermMemoryDraft,
        LongTermMemoryEntry, LongTermMemoryExtractionState, LongTermMemoryExtractionStateStore,
        LongTermMemorySlot, LongTermMemoryStore, MemoryStore, MentalPrivacyState,
        MentalPrivacyStore, OuterVoiceStore, PendingRetryStore, PrivateDocStore, PrivateGardenDoc,
        PrivateGardenDocRecord, PrivateGardenStore, RelationshipTopologyStore, SelfContinuityStore,
        SelfModelStore, SessionMessage, SessionStore, SessionSummaryStore,
        TemperamentContinuityStore, TurnBlockerLedger, TurnContinuityEvidence,
        TurnContinuityEvidenceStore, TurnDeliberationClass, TurnLedger, TurnLedgerStore,
        TurnPersonaPressureLevel, WorldSenseStore,
    };
    use crate::platform::{PlatformHttpClient, ResponseBody};
    use crate::tools::{
        build_default_tool_protocol_authority, ToolCatalogAuthority, ToolLlmVisibility,
        ToolProtocolContract,
    };
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    #[test]
    fn pre_llm_visibility_window_waits_for_os_outbound_worker_to_exit() {
        assert!(
            !turn_execution::pre_llm_visibility_window_is_settled(0, 0, 1, false, true, true),
            "queue depth alone is not enough; the visibility worker stack must be released before LLM"
        );
        assert!(turn_execution::pre_llm_visibility_window_is_settled(
            0, 0, 0, false, true, false
        ));
        assert!(
            !turn_execution::pre_llm_visibility_window_is_settled(0, 1, 0, true, true, true),
            "active HTTP must still keep the pre-LLM window open"
        );
    }

    fn synthetic_catalog(entries: &[(&str, ToolLlmVisibility)]) -> Arc<ToolCatalogAuthority> {
        let mut authority = ToolCatalogAuthority::default();
        for (name, visibility) in entries {
            authority.insert(name, *visibility);
        }
        Arc::new(authority)
    }

    fn test_registry(entries: &[(&str, ToolLlmVisibility)]) -> crate::tools::ToolRegistry {
        test_registry_with_protocols(entries, &[])
    }

    fn test_registry_with_protocols(
        entries: &[(&str, ToolLlmVisibility)],
        protocols: &[(&str, ToolProtocolContract)],
    ) -> crate::tools::ToolRegistry {
        let mut protocol_authority = build_default_tool_protocol_authority();
        for (name, contract) in protocols {
            protocol_authority.insert(name, *contract);
        }
        crate::tools::ToolRegistry::new()
            .with_llm_catalog_authority(synthetic_catalog(entries))
            .with_tool_protocol_authority(Arc::new(protocol_authority))
    }

    #[test]
    fn try_send_outbound_waits_for_primary_reply_queue_space() {
        let (bus, _inbound_rx, outbound_rx) = crate::bus::MessageBus::new(1);
        bus.outbound_tx
            .try_send(PcMsg::new("qq_channel", "chat-1", "queued").expect("queued"))
            .expect("fill outbound queue");
        let tx = bus.outbound_tx.clone();
        let receiver = std::thread::spawn(move || {
            let first = outbound_rx.recv().expect("first queued message");
            let second = outbound_rx.recv().expect("primary reply");
            (first.content, second.content)
        });
        let reply = PcMsg::new("qq_channel", "chat-1", "reply").expect("reply");

        assert!(try_send_outbound(&tx, reply, "reply"));

        let (first, second) = receiver.join().expect("receiver joins");
        assert_eq!(first, "queued");
        assert_eq!(second, "reply");
    }

    #[test]
    fn try_send_outbound_waits_for_visibility_queue_space() {
        let (bus, _inbound_rx, outbound_rx) = crate::bus::MessageBus::new(1);
        bus.outbound_tx
            .try_send(PcMsg::new("qq_channel", "chat-1", "queued").expect("queued"))
            .expect("fill outbound queue");
        let tx = bus.outbound_tx.clone();
        let receiver = std::thread::spawn(move || {
            let first = outbound_rx.recv().expect("first queued message");
            let second = outbound_rx.recv().expect("visibility ack");
            (first.content, second.content, second.outbound_kind)
        });
        let mut visibility = PcMsg::new("qq_channel", "chat-1", "ack").expect("visibility");
        visibility.outbound_kind = OutboundKind::Visibility;

        assert!(try_send_outbound(&tx, visibility, "visibility"));

        let (first, second, kind) = receiver.join().expect("receiver joins");
        assert_eq!(first, "queued");
        assert_eq!(second, "ack");
        assert_eq!(kind, OutboundKind::Visibility);
    }

    #[test]
    fn post_reply_payload_defaults_external_content_flag_for_older_jobs() {
        let raw = serde_json::json!({
            "ingress": IngressKind::User,
            "source_channel": "qq_channel",
            "user_content": "hi",
            "reply_content": "hello",
            "tool_calls": 1,
            "now_secs": 42
        })
        .to_string();

        let payload: PostReplyMaintenanceJobPayload =
            serde_json::from_str(&raw).expect("deserialize legacy payload");
        assert!(!payload.external_content_used);
    }

    #[test]
    fn post_reply_maintenance_jobs_adopt_into_detached_work_queue() {
        let store = StubDetachedWorkStore::default();
        let payload = serde_json::json!({
            "ingress": IngressKind::User,
            "source_channel": "qq_channel",
            "user_content": "查看系统状态",
            "reply_content": "正在检查",
            "tool_calls": 0,
            "external_content_used": false,
            "now_secs": 42
        })
        .to_string();
        let msg = PcMsg::new_system(CHANNEL_POST_REPLY_MAINTENANCE, "chat-1", payload)
            .expect("build maintenance message");

        assert!(
            adopt_background_job_as_detached(&store, &msg, "background_job_adopted")
                .expect("adopt")
        );

        let key = crate::agent::DetachedWorkKey::new(
            "qq_channel",
            "chat-1",
            crate::agent::DetachedJobKind::PostReplyMaintenance,
        );
        let stored = store
            .get(&key)
            .expect("load detached work")
            .expect("stored record");
        assert_eq!(stored.state, crate::agent::DetachedWorkState::Pending);

        let (system_inbound_tx, system_inbound_rx, _) = crate::bus::new_system_inbound_channel(4);
        wake_due_detached_background_work(&store, &system_inbound_tx, 1);

        let wake_msg = system_inbound_rx.try_recv().expect("wake enqueued");
        assert_eq!(wake_msg.channel.as_ref(), CHANNEL_DETACHED_WORK_WAKE);
        let wake: crate::agent::DetachedWorkWake =
            serde_json::from_str(&wake_msg.content).expect("decode wake");
        assert_eq!(wake.key, key);
        assert_eq!(
            store
                .get(&key)
                .expect("reload detached work")
                .expect("queued record")
                .state,
            crate::agent::DetachedWorkState::Queued
        );
    }

    #[test]
    fn detached_work_wake_messages_are_lane_background_jobs() {
        let wake = crate::agent::DetachedWorkWake {
            key: crate::agent::DetachedWorkKey::new(
                "qq_channel",
                "chat-1",
                crate::agent::DetachedJobKind::PostReplyMaintenance,
            ),
            revision: 1,
        };
        let msg = PcMsg::new_system(
            CHANNEL_DETACHED_WORK_WAKE,
            "chat-1",
            serde_json::to_string(&wake).expect("serialize wake"),
        )
        .expect("build detached work wake");

        assert!(is_detached_work_wake(&msg));
        assert!(is_lane_background_job(&msg));
        assert_eq!(
            classify_system_work(msg.channel.as_ref(), msg.ingress),
            crate::runtime::system_work::SystemWorkClass::Maintenance
        );
    }

    #[derive(Default)]
    struct DummyPlatformHttp;

    impl PlatformHttpClient for DummyPlatformHttp {
        fn get(&mut self, _url: &str, _headers: &[(&str, &str)]) -> Result<(u16, ResponseBody)> {
            Ok((200, ResponseBody::Heap(Vec::new())))
        }

        fn post(
            &mut self,
            _url: &str,
            _headers: &[(&str, &str)],
            _body: &[u8],
        ) -> Result<(u16, ResponseBody)> {
            Ok((200, ResponseBody::Heap(Vec::new())))
        }
    }

    #[derive(Default)]
    struct EmptyMemoryStore;

    impl MemoryStore for EmptyMemoryStore {
        fn get_memory(&self) -> Result<String> {
            Ok(String::new())
        }
        fn set_memory(&self, _content: &str) -> Result<()> {
            Ok(())
        }
        fn list_daily_note_names(&self, _recent_n: usize) -> Result<Vec<String>> {
            Ok(Vec::new())
        }
        fn get_daily_note(&self, _name: &str) -> Result<String> {
            Ok(String::new())
        }
        fn write_daily_note(&self, _name: &str, _content: &str) -> Result<()> {
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubSessionStore {
        entries: Mutex<HashMap<String, Vec<SessionMessage>>>,
    }

    impl SessionStore for StubSessionStore {
        fn append(&self, chat_id: &str, role: &str, content: &str) -> Result<()> {
            self.entries
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .entry(chat_id.to_string())
                .or_default()
                .push(SessionMessage {
                    role: role.to_string(),
                    content: content.to_string(),
                });
            Ok(())
        }

        fn load_recent(&self, chat_id: &str, n: usize) -> Result<Vec<SessionMessage>> {
            let entries = self.entries.lock().unwrap_or_else(|e| e.into_inner());
            let values = entries.get(chat_id).cloned().unwrap_or_default();
            let start = values.len().saturating_sub(n);
            Ok(values[start..].to_vec())
        }

        fn clear(&self, chat_id: &str) -> Result<()> {
            self.entries
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(chat_id);
            Ok(())
        }

        fn list_chat_ids(&self) -> Result<Vec<String>> {
            Ok(self
                .entries
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .keys()
                .cloned()
                .collect())
        }
    }

    #[derive(Default)]
    struct StubSessionSummaryStore;

    impl SessionSummaryStore for StubSessionSummaryStore {
        fn get(&self, _chat_id: &str) -> Result<Option<String>> {
            Ok(None)
        }
        fn set(&self, _chat_id: &str, _summary: &str) -> Result<()> {
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubExecutionStateStore {
        entries: Mutex<HashMap<String, ExecutionState>>,
    }

    impl ExecutionStateStore for StubExecutionStateStore {
        fn get(&self, chat_id: &str) -> Result<Option<ExecutionState>> {
            Ok(self
                .entries
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(chat_id)
                .cloned())
        }
        fn set(&self, chat_id: &str, state: &ExecutionState) -> Result<()> {
            self.entries
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(chat_id.to_string(), state.clone());
            Ok(())
        }
        fn clear(&self, chat_id: &str) -> Result<()> {
            self.entries
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(chat_id);
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubActiveWorkStore {
        entries: Mutex<HashMap<String, crate::agent::ActiveWorkRecord>>,
    }

    impl crate::agent::ActiveWorkStore for StubActiveWorkStore {
        fn get(&self, chat_id: &str) -> Result<Option<crate::agent::ActiveWorkRecord>> {
            Ok(self
                .entries
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(chat_id)
                .cloned())
        }

        fn set(&self, chat_id: &str, record: &crate::agent::ActiveWorkRecord) -> Result<()> {
            self.entries
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(chat_id.to_string(), record.clone());
            Ok(())
        }

        fn clear(&self, chat_id: &str) -> Result<()> {
            self.entries
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(chat_id);
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubDetachedWorkStore {
        entries: Mutex<HashMap<String, crate::agent::DetachedWorkRecord>>,
    }

    impl crate::agent::DetachedWorkStore for StubDetachedWorkStore {
        fn get(
            &self,
            key: &crate::agent::DetachedWorkKey,
        ) -> Result<Option<crate::agent::DetachedWorkRecord>> {
            Ok(self
                .entries
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(&key.storage_key())
                .cloned())
        }

        fn list(&self) -> Result<Vec<crate::agent::DetachedWorkRecord>> {
            Ok(self
                .entries
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .values()
                .cloned()
                .collect())
        }

        fn upsert(
            &self,
            key: &crate::agent::DetachedWorkKey,
            job: &PcMsg,
            wake_at_ms: u64,
            reason: &str,
        ) -> Result<crate::agent::DetachedWorkUpsertOutcome> {
            let mut entries = self.entries.lock().unwrap_or_else(|e| e.into_inner());
            let storage_key = key.storage_key();
            let next = match entries.get(&storage_key) {
                Some(current)
                    if current.job == *job
                        && current.wake_at_ms == wake_at_ms
                        && current.last_reason == reason
                        && current.state == crate::agent::DetachedWorkState::Pending =>
                {
                    return Ok(crate::agent::DetachedWorkUpsertOutcome {
                        changed: false,
                        record: current.clone(),
                    });
                }
                Some(current) => crate::agent::DetachedWorkRecord {
                    key: key.clone(),
                    job: job.clone(),
                    state: crate::agent::DetachedWorkState::Pending,
                    wake_at_ms,
                    revision: current.revision.saturating_add(1),
                    last_reason: reason.to_string(),
                    updated_at_ms: 1,
                },
                None => crate::agent::DetachedWorkRecord {
                    key: key.clone(),
                    job: job.clone(),
                    state: crate::agent::DetachedWorkState::Pending,
                    wake_at_ms,
                    revision: 1,
                    last_reason: reason.to_string(),
                    updated_at_ms: 1,
                },
            };
            entries.insert(storage_key, next.clone());
            Ok(crate::agent::DetachedWorkUpsertOutcome {
                changed: true,
                record: next,
            })
        }

        fn mark_queued(&self, key: &crate::agent::DetachedWorkKey, revision: u64) -> Result<bool> {
            let mut entries = self.entries.lock().unwrap_or_else(|e| e.into_inner());
            let Some(record) = entries.get_mut(&key.storage_key()) else {
                return Ok(false);
            };
            if record.revision != revision
                || record.state != crate::agent::DetachedWorkState::Pending
            {
                return Ok(false);
            }
            record.state = crate::agent::DetachedWorkState::Queued;
            Ok(true)
        }

        fn claim_running(
            &self,
            key: &crate::agent::DetachedWorkKey,
            revision: u64,
        ) -> Result<Option<crate::agent::DetachedWorkRecord>> {
            let mut entries = self.entries.lock().unwrap_or_else(|e| e.into_inner());
            let Some(record) = entries.get_mut(&key.storage_key()) else {
                return Ok(None);
            };
            if record.revision != revision
                || !matches!(
                    record.state,
                    crate::agent::DetachedWorkState::Pending
                        | crate::agent::DetachedWorkState::Queued
                )
            {
                return Ok(None);
            }
            record.state = crate::agent::DetachedWorkState::Running;
            Ok(Some(record.clone()))
        }

        fn reschedule(
            &self,
            key: &crate::agent::DetachedWorkKey,
            revision: u64,
            wake_at_ms: u64,
            reason: &str,
        ) -> Result<Option<crate::agent::DetachedWorkRecord>> {
            let mut entries = self.entries.lock().unwrap_or_else(|e| e.into_inner());
            let Some(record) = entries.get_mut(&key.storage_key()) else {
                return Ok(None);
            };
            if record.revision != revision {
                return Ok(None);
            }
            record.revision = record.revision.saturating_add(1);
            record.state = crate::agent::DetachedWorkState::Pending;
            record.wake_at_ms = wake_at_ms;
            record.last_reason = reason.to_string();
            Ok(Some(record.clone()))
        }

        fn finish(&self, key: &crate::agent::DetachedWorkKey, revision: u64) -> Result<()> {
            let mut entries = self.entries.lock().unwrap_or_else(|e| e.into_inner());
            if entries
                .get(&key.storage_key())
                .is_some_and(|record| record.revision == revision)
            {
                entries.remove(&key.storage_key());
            }
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubSelfModelStore;

    impl SelfModelStore for StubSelfModelStore {
        fn get(&self, _chat_id: &str) -> Result<Option<crate::memory::SelfModel>> {
            Ok(None)
        }

        fn set(&self, _chat_id: &str, _model: &crate::memory::SelfModel) -> Result<()> {
            Ok(())
        }

        fn clear(&self, _chat_id: &str) -> Result<()> {
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubSelfAuthoredCoreStore;

    impl crate::memory::SelfAuthoredCoreStore for StubSelfAuthoredCoreStore {
        fn get(&self, _scope_id: &str) -> Result<Option<crate::memory::SelfAuthoredCore>> {
            Ok(None)
        }

        fn set(&self, _scope_id: &str, _core: &crate::memory::SelfAuthoredCore) -> Result<()> {
            Ok(())
        }

        fn clear(&self, _scope_id: &str) -> Result<()> {
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubCoreRevisionLedgerStore;

    impl crate::memory::CoreRevisionLedgerStore for StubCoreRevisionLedgerStore {
        fn get(&self, _scope_id: &str) -> Result<Option<crate::memory::CoreRevisionLedger>> {
            Ok(None)
        }

        fn set(&self, _scope_id: &str, _ledger: &crate::memory::CoreRevisionLedger) -> Result<()> {
            Ok(())
        }

        fn clear(&self, _scope_id: &str) -> Result<()> {
            Ok(())
        }
    }

    struct StubRelationshipConstitutionStore;

    impl crate::memory::RelationshipConstitutionStore for StubRelationshipConstitutionStore {
        fn get(&self, _scope_id: &str) -> Result<Option<crate::memory::RelationshipConstitution>> {
            Ok(None)
        }

        fn set(
            &self,
            _scope_id: &str,
            _constitution: &crate::memory::RelationshipConstitution,
        ) -> Result<()> {
            Ok(())
        }

        fn clear(&self, _scope_id: &str) -> Result<()> {
            Ok(())
        }
    }

    #[derive(Default)]
    struct LoadedSelfAuthoredCoreStore {
        value: crate::memory::SelfAuthoredCore,
    }

    impl crate::memory::SelfAuthoredCoreStore for LoadedSelfAuthoredCoreStore {
        fn get(&self, _scope_id: &str) -> Result<Option<crate::memory::SelfAuthoredCore>> {
            Ok(Some(self.value.clone()))
        }

        fn set(&self, _scope_id: &str, _core: &crate::memory::SelfAuthoredCore) -> Result<()> {
            Ok(())
        }

        fn clear(&self, _scope_id: &str) -> Result<()> {
            Ok(())
        }
    }

    #[derive(Default)]
    struct TrackingRelationshipConstitutionStore {
        value: Mutex<Option<crate::memory::RelationshipConstitution>>,
        set_count: AtomicU32,
        clear_count: AtomicU32,
    }

    impl TrackingRelationshipConstitutionStore {
        fn set_count(&self) -> u32 {
            self.set_count.load(Ordering::Relaxed)
        }

        fn clear_count(&self) -> u32 {
            self.clear_count.load(Ordering::Relaxed)
        }
    }

    impl crate::memory::RelationshipConstitutionStore for TrackingRelationshipConstitutionStore {
        fn get(&self, _scope_id: &str) -> Result<Option<crate::memory::RelationshipConstitution>> {
            Ok(self.value.lock().unwrap_or_else(|e| e.into_inner()).clone())
        }

        fn set(
            &self,
            _scope_id: &str,
            constitution: &crate::memory::RelationshipConstitution,
        ) -> Result<()> {
            *self.value.lock().unwrap_or_else(|e| e.into_inner()) = Some(constitution.clone());
            self.set_count.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }

        fn clear(&self, _scope_id: &str) -> Result<()> {
            *self.value.lock().unwrap_or_else(|e| e.into_inner()) = None;
            self.clear_count.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubWorldSenseStore;

    impl WorldSenseStore for StubWorldSenseStore {
        fn get(&self, _chat_id: &str) -> Result<Option<crate::memory::WorldSense>> {
            Ok(None)
        }

        fn set(&self, _chat_id: &str, _world_sense: &crate::memory::WorldSense) -> Result<()> {
            Ok(())
        }

        fn clear(&self, _chat_id: &str) -> Result<()> {
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubAutonomyStrategyStore;

    impl AutonomyStrategyStore for StubAutonomyStrategyStore {
        fn get(&self, _chat_id: &str) -> Result<Option<crate::memory::AutonomyStrategy>> {
            Ok(None)
        }

        fn set(&self, _chat_id: &str, _strategy: &crate::memory::AutonomyStrategy) -> Result<()> {
            Ok(())
        }

        fn clear(&self, _chat_id: &str) -> Result<()> {
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubOuterVoiceStore;

    impl OuterVoiceStore for StubOuterVoiceStore {
        fn get(&self, _chat_id: &str) -> Result<Option<crate::memory::OuterVoice>> {
            Ok(None)
        }

        fn set(&self, _chat_id: &str, _outer_voice: &crate::memory::OuterVoice) -> Result<()> {
            Ok(())
        }

        fn clear(&self, _chat_id: &str) -> Result<()> {
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubInnerLifeStore;

    impl InnerLifeStore for StubInnerLifeStore {
        fn get(&self, _chat_id: &str) -> Result<Option<crate::memory::InnerLife>> {
            Ok(None)
        }

        fn set(&self, _chat_id: &str, _inner_life: &crate::memory::InnerLife) -> Result<()> {
            Ok(())
        }

        fn clear(&self, _chat_id: &str) -> Result<()> {
            Ok(())
        }
    }

    struct LoadedInnerLifeStore {
        value: crate::memory::InnerLife,
    }

    impl InnerLifeStore for LoadedInnerLifeStore {
        fn get(&self, _chat_id: &str) -> Result<Option<crate::memory::InnerLife>> {
            Ok(Some(self.value.clone()))
        }

        fn set(&self, _chat_id: &str, _inner_life: &crate::memory::InnerLife) -> Result<()> {
            Ok(())
        }

        fn clear(&self, _chat_id: &str) -> Result<()> {
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubSelfContinuityStore;

    impl SelfContinuityStore for StubSelfContinuityStore {
        fn get(&self, _chat_id: &str) -> Result<Option<crate::memory::SelfContinuity>> {
            Ok(None)
        }

        fn set(&self, _chat_id: &str, _continuity: &crate::memory::SelfContinuity) -> Result<()> {
            Ok(())
        }

        fn clear(&self, _chat_id: &str) -> Result<()> {
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubFeltSignificanceStore;

    impl FeltSignificanceStore for StubFeltSignificanceStore {
        fn get(&self, _scope_id: &str) -> Result<Option<crate::memory::FeltSignificance>> {
            Ok(None)
        }

        fn set(
            &self,
            _scope_id: &str,
            _significance: &crate::memory::FeltSignificance,
        ) -> Result<()> {
            Ok(())
        }

        fn clear(&self, _scope_id: &str) -> Result<()> {
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubTemperamentContinuityStore;

    impl TemperamentContinuityStore for StubTemperamentContinuityStore {
        fn get(&self, _scope_id: &str) -> Result<Option<crate::memory::TemperamentContinuity>> {
            Ok(None)
        }

        fn set(
            &self,
            _scope_id: &str,
            _continuity: &crate::memory::TemperamentContinuity,
        ) -> Result<()> {
            Ok(())
        }

        fn clear(&self, _scope_id: &str) -> Result<()> {
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubInnerConflictStore;

    impl InnerConflictStore for StubInnerConflictStore {
        fn get(&self, _scope_id: &str) -> Result<Option<crate::memory::InnerConflict>> {
            Ok(None)
        }

        fn set(&self, _scope_id: &str, _conflict: &crate::memory::InnerConflict) -> Result<()> {
            Ok(())
        }

        fn clear(&self, _scope_id: &str) -> Result<()> {
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubRelationshipPortfolioStore;

    impl crate::memory::RelationshipPortfolioStore for StubRelationshipPortfolioStore {
        fn get(&self, _scope_id: &str) -> Result<Option<crate::memory::RelationshipPortfolio>> {
            Ok(None)
        }

        fn set(
            &self,
            _scope_id: &str,
            _portfolio: &crate::memory::RelationshipPortfolio,
        ) -> Result<()> {
            Ok(())
        }

        fn clear(&self, _scope_id: &str) -> Result<()> {
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubRelationshipTopologyStore;

    impl RelationshipTopologyStore for StubRelationshipTopologyStore {
        fn get(&self, _scope_id: &str) -> Result<Option<crate::memory::RelationshipTopology>> {
            Ok(None)
        }

        fn set(
            &self,
            _scope_id: &str,
            _topology: &crate::memory::RelationshipTopology,
        ) -> Result<()> {
            Ok(())
        }

        fn clear(&self, _scope_id: &str) -> Result<()> {
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubPrivateDocStore;

    impl PrivateDocStore for StubPrivateDocStore {
        fn get(&self, _chat_id: &str) -> Result<Option<crate::memory::PrivateDocWorkspace>> {
            Ok(None)
        }

        fn set(
            &self,
            _chat_id: &str,
            _workspace: &crate::memory::PrivateDocWorkspace,
        ) -> Result<()> {
            Ok(())
        }

        fn clear(&self, _chat_id: &str) -> Result<()> {
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubPrivateGardenStore;

    impl PrivateGardenStore for StubPrivateGardenStore {
        fn list(&self, _chat_id: &str, _limit: usize) -> Result<Vec<PrivateGardenDocRecord>> {
            Ok(Vec::new())
        }

        fn read(&self, _chat_id: &str, _doc_path: &str) -> Result<Option<PrivateGardenDoc>> {
            Ok(None)
        }

        fn write(
            &self,
            _chat_id: &str,
            _doc_path: &str,
            _content: &str,
            _now_secs: u64,
        ) -> Result<PrivateGardenDocRecord> {
            unreachable!()
        }

        fn delete(&self, _chat_id: &str, _doc_path: &str) -> Result<bool> {
            unreachable!()
        }

        fn move_doc(
            &self,
            _chat_id: &str,
            _from_path: &str,
            _to_path: &str,
            _now_secs: u64,
        ) -> Result<Option<PrivateGardenDocRecord>> {
            unreachable!()
        }
    }

    #[derive(Default)]
    struct StubMentalPrivacyStore;

    impl MentalPrivacyStore for StubMentalPrivacyStore {
        fn get(&self, _chat_id: &str) -> Result<Option<MentalPrivacyState>> {
            Ok(None)
        }

        fn set(&self, _chat_id: &str, _state: &MentalPrivacyState) -> Result<()> {
            Ok(())
        }

        fn clear(&self, _chat_id: &str) -> Result<()> {
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubRemindAtStore;

    impl crate::memory::RemindAtStore for StubRemindAtStore {
        fn get(
            &self,
            _channel: &str,
            _chat_id: &str,
            _id: &str,
        ) -> Result<Option<crate::reminder::ReminderItem>> {
            Ok(None)
        }

        fn upsert(&self, _reminder: &crate::reminder::ReminderItem) -> Result<()> {
            Ok(())
        }

        fn delete(&self, _channel: &str, _chat_id: &str, _id: &str) -> Result<bool> {
            Ok(false)
        }

        fn list_due(
            &self,
            _now_unix_secs: u64,
            _limit: usize,
        ) -> Result<Vec<crate::reminder::ReminderItem>> {
            Ok(Vec::new())
        }

        fn delete_due(&self, _reminder: &crate::reminder::ReminderItem) -> Result<bool> {
            Ok(false)
        }

        fn list_upcoming(
            &self,
            _channel: &str,
            _chat_id: &str,
            _now_unix_secs: u64,
            _limit: usize,
        ) -> Result<Vec<crate::reminder::ReminderItem>> {
            Ok(Vec::new())
        }
    }

    #[derive(Default)]
    struct StubTaskStore;

    impl crate::task::TaskStore for StubTaskStore {
        fn list(
            &self,
            _channel: &str,
            _chat_id: &str,
            _query: crate::task::TaskQuery,
        ) -> Result<Vec<crate::task::TaskItem>> {
            Ok(Vec::new())
        }

        fn get(
            &self,
            _channel: &str,
            _chat_id: &str,
            _id: &str,
        ) -> Result<Option<crate::task::TaskItem>> {
            Ok(None)
        }

        fn upsert(&self, _task: &crate::task::TaskItem) -> Result<()> {
            Ok(())
        }

        fn delete(&self, _channel: &str, _chat_id: &str, _id: &str) -> Result<bool> {
            Ok(false)
        }

        fn list_due_unnotified(
            &self,
            _now_unix_secs: u64,
            _limit: usize,
        ) -> Result<Vec<crate::task::TaskItem>> {
            Ok(Vec::new())
        }

        fn mark_due_notified(
            &self,
            _task: &crate::task::TaskItem,
            _notified_at_unix_secs: u64,
        ) -> Result<bool> {
            Ok(false)
        }
    }

    #[derive(Default)]
    struct StubTaskRunStore {
        entries: Mutex<HashMap<String, crate::task_execution::TaskRunRecord>>,
    }

    impl crate::task_execution::TaskRunStore for StubTaskRunStore {
        fn get(&self, run_id: &str) -> Result<Option<crate::task_execution::TaskRunRecord>> {
            Ok(self
                .entries
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(run_id)
                .cloned())
        }

        fn upsert(&self, record: &crate::task_execution::TaskRunRecord) -> Result<()> {
            self.entries
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(record.run.run_id.clone(), record.clone());
            Ok(())
        }

        fn list_recent(&self, limit: usize) -> Result<Vec<crate::task_execution::TaskRunRecord>> {
            let mut records = self
                .entries
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .values()
                .cloned()
                .collect::<Vec<_>>();
            records.sort_by_key(|record| std::cmp::Reverse(record.run.updated_at));
            records.truncate(limit);
            Ok(records)
        }

        fn list_active_for_chat(
            &self,
            channel: &str,
            chat_id: &str,
            limit: usize,
        ) -> Result<Vec<crate::task_execution::TaskRunRecord>> {
            let mut records = self
                .entries
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .values()
                .filter(|record| {
                    record.run.source_channel == channel
                        && record.run.source_chat_id == chat_id
                        && record.run.status.is_active()
                })
                .cloned()
                .collect::<Vec<_>>();
            records.sort_by_key(|record| std::cmp::Reverse(record.run.updated_at));
            records.truncate(limit);
            Ok(records)
        }
    }

    #[derive(Default)]
    struct StubTaskArtifactStore;

    impl crate::task_execution::TaskArtifactStore for StubTaskArtifactStore {
        fn put(&self, _record: &crate::task_execution::TaskArtifactRecord) -> Result<()> {
            Ok(())
        }

        fn list_for_run(
            &self,
            _run_id: &str,
            _limit: usize,
        ) -> Result<Vec<crate::task_execution::TaskArtifactRecord>> {
            Ok(Vec::new())
        }
    }

    #[derive(Default)]
    struct StubTaskExecutionLedgerStore;

    impl crate::task_execution::TaskExecutionLedgerStore for StubTaskExecutionLedgerStore {
        fn append(
            &self,
            _run_id: &str,
            _entry: &crate::task_execution::TaskExecutionLedgerEntry,
        ) -> Result<()> {
            Ok(())
        }

        fn list(
            &self,
            _run_id: &str,
            _limit: usize,
        ) -> Result<Vec<crate::task_execution::TaskExecutionLedgerEntry>> {
            Ok(Vec::new())
        }
    }

    #[derive(Default)]
    struct StubTaskLearningStore;

    impl crate::task_execution::TaskLearningStore for StubTaskLearningStore {
        fn get(
            &self,
            _learning_id: &str,
        ) -> Result<Option<crate::task_execution::TaskLearningRecord>> {
            Ok(None)
        }

        fn upsert(&self, _record: &crate::task_execution::TaskLearningRecord) -> Result<()> {
            Ok(())
        }

        fn list_recent(
            &self,
            _limit: usize,
        ) -> Result<Vec<crate::task_execution::TaskLearningRecord>> {
            Ok(Vec::new())
        }

        fn list_for_chat(
            &self,
            _channel: &str,
            _chat_id: &str,
            _limit: usize,
        ) -> Result<Vec<crate::task_execution::TaskLearningRecord>> {
            Ok(Vec::new())
        }

        fn list_for_run(
            &self,
            _run_id: &str,
            _limit: usize,
        ) -> Result<Vec<crate::task_execution::TaskLearningRecord>> {
            Ok(Vec::new())
        }
    }

    #[derive(Default)]
    struct StubLongTermMemoryStore;

    impl LongTermMemoryStore for StubLongTermMemoryStore {
        fn upsert_many(&self, _drafts: &[LongTermMemoryDraft], _now_secs: u64) -> Result<usize> {
            Ok(0)
        }
        fn recall(
            &self,
            _query: &str,
            _source_chat_id: Option<&str>,
            _limit: usize,
        ) -> Result<Vec<LongTermMemoryEntry>> {
            Ok(Vec::new())
        }
        fn get(&self, _id: &str) -> Result<Option<LongTermMemoryEntry>> {
            Ok(None)
        }
        fn list(&self, _limit: usize) -> Result<Vec<LongTermMemoryEntry>> {
            Ok(Vec::new())
        }
        fn delete(&self, _id: &str) -> Result<bool> {
            Ok(false)
        }
        fn delete_slot(&self, _slot: &LongTermMemorySlot) -> Result<bool> {
            Ok(false)
        }
        fn count(&self) -> Result<usize> {
            Ok(0)
        }
    }

    #[derive(Default)]
    struct StubContinuityCapsuleStore;

    impl crate::memory::ContinuityCapsuleStore for StubContinuityCapsuleStore {
        fn upsert_many(
            &self,
            _drafts: &[crate::memory::ContinuityCapsuleDraft],
            _now_secs: u64,
        ) -> Result<crate::memory::ContinuityCapsuleWriteOutcome> {
            Ok(crate::memory::ContinuityCapsuleWriteOutcome::default())
        }

        fn get(&self, _capsule_id: &str) -> Result<Option<crate::memory::ContinuityCapsule>> {
            Ok(None)
        }

        fn list(&self, _limit: usize) -> Result<Vec<crate::memory::ContinuityCapsule>> {
            Ok(Vec::new())
        }

        fn count(&self) -> Result<usize> {
            Ok(0)
        }
    }

    #[derive(Default)]
    struct StubLongTermMemoryExtractionStateStore;

    impl LongTermMemoryExtractionStateStore for StubLongTermMemoryExtractionStateStore {
        fn get(&self, _chat_id: &str) -> Result<Option<LongTermMemoryExtractionState>> {
            Ok(None)
        }
        fn set(&self, _chat_id: &str, _state: &LongTermMemoryExtractionState) -> Result<()> {
            Ok(())
        }
        fn clear(&self, _chat_id: &str) -> Result<()> {
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubImportantMessageStore;

    impl ImportantMessageStore for StubImportantMessageStore {
        fn set_important_offset_from_end(
            &self,
            _chat_id: &str,
            _offset_from_end: u32,
        ) -> Result<()> {
            Ok(())
        }
        fn get_important_offset(&self, _chat_id: &str) -> Result<Option<u32>> {
            Ok(None)
        }
        fn clear_important(&self, _chat_id: &str) -> Result<()> {
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubPendingRetryStore;

    impl PendingRetryStore for StubPendingRetryStore {
        fn save_pending_retry(&self, _msg: &PcMsg) -> Result<()> {
            Ok(())
        }
        fn load_pending_retry(&self) -> Result<Option<PcMsg>> {
            Ok(None)
        }
        fn clear_pending_retry(&self) -> Result<()> {
            Ok(())
        }
    }

    struct StubTurnLedgerStore;

    impl TurnLedgerStore for StubTurnLedgerStore {
        fn get(&self, _chat_id: &str) -> Result<Option<TurnLedger>> {
            Ok(None)
        }

        fn set(&self, _chat_id: &str, _ledger: &TurnLedger) -> Result<()> {
            Ok(())
        }

        fn clear(&self, _chat_id: &str) -> Result<()> {
            Ok(())
        }
    }

    struct StubTurnContinuityEvidenceStore;

    impl TurnContinuityEvidenceStore for StubTurnContinuityEvidenceStore {
        fn append(&self, _chat_id: &str, _evidence: &TurnContinuityEvidence) -> Result<()> {
            Ok(())
        }

        fn clear(&self, _chat_id: &str) -> Result<()> {
            Ok(())
        }

        fn list_recent(
            &self,
            _chat_id: &str,
            _limit: usize,
        ) -> Result<Vec<TurnContinuityEvidence>> {
            Ok(Vec::new())
        }
    }

    #[derive(Default)]
    struct RecordingTurnLedgerStore {
        entries: Mutex<HashMap<String, TurnLedger>>,
    }

    impl TurnLedgerStore for RecordingTurnLedgerStore {
        fn get(&self, chat_id: &str) -> Result<Option<TurnLedger>> {
            Ok(self
                .entries
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(chat_id)
                .cloned())
        }

        fn set(&self, chat_id: &str, ledger: &TurnLedger) -> Result<()> {
            self.entries
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(chat_id.to_string(), ledger.clone());
            Ok(())
        }

        fn clear(&self, chat_id: &str) -> Result<()> {
            self.entries
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(chat_id);
            Ok(())
        }
    }

    #[derive(Default)]
    struct RecordingTurnContinuityEvidenceStore {
        entries: Mutex<HashMap<String, Vec<TurnContinuityEvidence>>>,
    }

    impl TurnContinuityEvidenceStore for RecordingTurnContinuityEvidenceStore {
        fn append(&self, chat_id: &str, evidence: &TurnContinuityEvidence) -> Result<()> {
            self.entries
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .entry(chat_id.to_string())
                .or_default()
                .push(evidence.clone());
            Ok(())
        }

        fn clear(&self, chat_id: &str) -> Result<()> {
            self.entries
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(chat_id);
            Ok(())
        }

        fn list_recent(&self, chat_id: &str, limit: usize) -> Result<Vec<TurnContinuityEvidence>> {
            let mut items = self
                .entries
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(chat_id)
                .cloned()
                .unwrap_or_default();
            items.reverse();
            items.truncate(limit);
            Ok(items)
        }
    }

    struct SequenceStubLlm {
        responses: Mutex<Vec<LlmResponse>>,
    }

    impl LlmClient for SequenceStubLlm {
        fn model_compat(&self) -> LlmModelCompat {
            LlmModelCompat::default()
        }

        fn chat(
            &self,
            _http: &mut dyn LlmHttpClient,
            _system: &str,
            _messages: &[Message],
            _tools: Option<&[crate::llm::ToolSpec]>,
            _tool_choice: ToolChoicePolicy,
        ) -> Result<LlmResponse> {
            let mut responses = self.responses.lock().unwrap_or_else(|e| e.into_inner());
            if responses.is_empty() {
                return Ok(LlmResponse {
                    content: String::new(),
                    stop_reason: StopReason::EndTurn,
                    tool_calls: None,
                });
            }
            Ok(responses.remove(0))
        }
    }

    #[derive(Clone, Debug, PartialEq, Eq)]
    struct ObservedAgentRequest {
        system: String,
        tool_count: usize,
        tool_choice: ToolChoicePolicy,
        last_message: String,
        message_dump: String,
    }

    struct ObservedSequenceStubLlm {
        responses: Mutex<Vec<LlmResponse>>,
        observed: Arc<Mutex<Vec<ObservedAgentRequest>>>,
    }

    impl LlmClient for ObservedSequenceStubLlm {
        fn model_compat(&self) -> LlmModelCompat {
            LlmModelCompat::default()
        }

        fn chat(
            &self,
            _http: &mut dyn LlmHttpClient,
            system: &str,
            messages: &[Message],
            tools: Option<&[crate::llm::ToolSpec]>,
            tool_choice: ToolChoicePolicy,
        ) -> Result<LlmResponse> {
            self.observed
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(ObservedAgentRequest {
                    system: system.to_string(),
                    tool_count: tools.map_or(0, |specs| specs.len()),
                    tool_choice,
                    last_message: messages
                        .last()
                        .map(|message| message.content.clone())
                        .unwrap_or_default(),
                    message_dump: messages
                        .iter()
                        .map(|message| format!("[{}]\n{}", message.role.as_ref(), message.content))
                        .collect::<Vec<_>>()
                        .join("\n---\n"),
                });
            let mut responses = self.responses.lock().unwrap_or_else(|e| e.into_inner());
            if responses.is_empty() {
                return Ok(LlmResponse {
                    content: String::new(),
                    stop_reason: StopReason::EndTurn,
                    tool_calls: None,
                });
            }
            Ok(responses.remove(0))
        }
    }

    struct StubBoardInfoTool;

    impl crate::tools::Tool for StubBoardInfoTool {
        fn name(&self) -> &'static str {
            "board_info"
        }

        fn description(&self) -> &str {
            "return stub board info"
        }

        fn schema(&self) -> &str {
            r#"{"type":"object","properties":{}}"#
        }

        fn execute(&self, _args: &str, _ctx: &mut dyn crate::tools::ToolContext) -> Result<String> {
            Ok(serde_json::json!({
                "platform": "linux",
                "hostname": "beetle",
                "pressure_level": "Normal",
                "wifi_sta_connected": true,
                "cpu_model": "Stub CPU",
                "cpu_cores": 4,
                "mem_available_bytes": 268435456u64
            })
            .to_string())
        }
    }

    struct StubResolvableOfficeMailTool {
        seen_args: Arc<Mutex<Vec<String>>>,
    }

    impl crate::tools::Tool for StubResolvableOfficeMailTool {
        fn name(&self) -> &'static str {
            "mail"
        }

        fn description(&self) -> &str {
            "return office ambiguity until account_key is resolved"
        }

        fn schema(&self) -> &str {
            r#"{"type":"object","properties":{"op":{"type":"string"},"provider":{"type":"string"},"account_key":{"type":"string"}},"required":["op"]}"#
        }

        fn execute(&self, args: &str, ctx: &mut dyn crate::tools::ToolContext) -> Result<String> {
            self.execute_outcome(args, ctx)
                .map(|outcome| outcome.content)
        }

        fn execute_outcome(
            &self,
            args: &str,
            _ctx: &mut dyn crate::tools::ToolContext,
        ) -> Result<crate::tools::ToolExecutionOutcome> {
            self.seen_args
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(args.to_string());
            if args.contains(r#""account_key":"mail-work""#) {
                return Ok(crate::tools::ToolExecutionOutcome::text(
                    serde_json::json!({
                        "ok": true,
                        "provider": "imap_smtp",
                        "account_key": "mail-work",
                        "messages": [
                            {"id": "msg-1", "subject": "Work mail"}
                        ]
                    })
                    .to_string(),
                ));
            }
            if args.contains(r#""account_key":"mail-personal""#) {
                return Ok(crate::tools::ToolExecutionOutcome::text(
                    serde_json::json!({
                        "ok": true,
                        "provider": "imap_smtp",
                        "account_key": "mail-personal",
                        "messages": [
                            {"id": "msg-2", "subject": "Personal mail"}
                        ]
                    })
                    .to_string(),
                ));
            }
            Ok(crate::tools::ToolExecutionOutcome::text(
                serde_json::json!({
                    "ok": false,
                    "provider": "imap_smtp",
                    "office_assessment": {
                        "capability": "mail",
                        "resolve_hint": {
                            "status": "ambiguous",
                            "candidate_accounts": [
                                {
                                    "account_key": "mail-work",
                                    "account_label": "Work",
                                    "provider_kind": "imap_smtp",
                                    "identity_class": "work"
                                },
                                {
                                    "account_key": "mail-personal",
                                    "account_label": "Personal",
                                    "provider_kind": "imap_smtp",
                                    "identity_class": "personal"
                                }
                            ]
                        }
                    }
                })
                .to_string(),
            )
            .with_failure_kind(crate::tools::ToolExecutionFailureKind::Capability)
            .with_blocker(crate::tools::ToolExecutionBlocker {
                kind: crate::tools::ToolExecutionBlockerKind::NeedsUserChoice,
                summary: "你要用 Work（mail-work）还是 Personal（mail-personal）这个邮箱账户？"
                    .to_string(),
                missing_fields: vec!["account_key".to_string()],
                clarification_fields: vec![crate::tools::ToolClarificationField {
                    key: "account_key".to_string(),
                    label: "Office account".to_string(),
                    description: "Choose which configured account should handle this request."
                        .to_string(),
                    required: true,
                    secret: false,
                    multiple: false,
                    options: vec![
                        crate::tools::ToolClarificationOption {
                            value: "mail-work".to_string(),
                            label: "Work（mail-work）".to_string(),
                        },
                        crate::tools::ToolClarificationOption {
                            value: "mail-personal".to_string(),
                            label: "Personal（mail-personal）".to_string(),
                        },
                    ],
                }],
            }))
        }
    }

    struct StubBlockingOfficeConfigTool;

    impl crate::tools::Tool for StubBlockingOfficeConfigTool {
        fn name(&self) -> &'static str {
            "office_config"
        }

        fn description(&self) -> &str {
            "return a structured onboarding blocker"
        }

        fn schema(&self) -> &str {
            r#"{"type":"object","properties":{"op":{"type":"string"}},"required":["op"]}"#
        }

        fn execute(&self, args: &str, ctx: &mut dyn crate::tools::ToolContext) -> Result<String> {
            self.execute_outcome(args, ctx)
                .map(|outcome| outcome.content)
        }

        fn execute_outcome(
            &self,
            _args: &str,
            _ctx: &mut dyn crate::tools::ToolContext,
        ) -> Result<crate::tools::ToolExecutionOutcome> {
            Ok(crate::tools::ToolExecutionOutcome::text(
                serde_json::json!({
                    "op": "apply_account",
                    "ok": false,
                    "payload": {
                        "disposition": "needs_user_facts",
                        "reason": "missing_user_facts",
                        "missing_fields": ["identity_class"],
                    }
                })
                .to_string(),
            )
            .with_blocker(crate::tools::ToolExecutionBlocker {
                kind: crate::tools::ToolExecutionBlockerKind::NeedsUserFacts,
                summary: "账户配置被阻塞：identity_class".to_string(),
                missing_fields: vec!["identity_class".to_string()],
                clarification_fields: vec![crate::tools::ToolClarificationField {
                    key: "identity_class".to_string(),
                    label: "Identity Class".to_string(),
                    description: "work|personal|family|shared|other".to_string(),
                    required: true,
                    secret: false,
                    multiple: false,
                    options: vec![
                        crate::tools::ToolClarificationOption {
                            value: "work".to_string(),
                            label: "Work".to_string(),
                        },
                        crate::tools::ToolClarificationOption {
                            value: "personal".to_string(),
                            label: "Personal".to_string(),
                        },
                    ],
                }],
            }))
        }
    }

    struct StubChoiceBlockingTool;

    impl crate::tools::Tool for StubChoiceBlockingTool {
        fn name(&self) -> &'static str {
            "mail"
        }

        fn description(&self) -> &str {
            "return a structured account choice blocker"
        }

        fn schema(&self) -> &str {
            r#"{"type":"object","properties":{"op":{"type":"string"}},"required":["op"]}"#
        }

        fn execute(&self, args: &str, ctx: &mut dyn crate::tools::ToolContext) -> Result<String> {
            self.execute_outcome(args, ctx)
                .map(|outcome| outcome.content)
        }

        fn execute_outcome(
            &self,
            _args: &str,
            _ctx: &mut dyn crate::tools::ToolContext,
        ) -> Result<crate::tools::ToolExecutionOutcome> {
            Ok(crate::tools::ToolExecutionOutcome::text(
                serde_json::json!({
                    "op": "list",
                    "ok": false,
                    "payload": {
                        "disposition": "ambiguous",
                        "missing_fields": ["account_key"],
                    }
                })
                .to_string(),
            )
            .with_blocker(crate::tools::ToolExecutionBlocker {
                kind: crate::tools::ToolExecutionBlockerKind::NeedsUserChoice,
                summary: "邮件账户选择被阻塞：account_key".to_string(),
                missing_fields: vec!["account_key".to_string()],
                clarification_fields: vec![crate::tools::ToolClarificationField {
                    key: "account_key".to_string(),
                    label: "Office account".to_string(),
                    description: "Choose which account to use.".to_string(),
                    required: true,
                    secret: false,
                    multiple: false,
                    options: vec![
                        crate::tools::ToolClarificationOption {
                            value: "mail-work".to_string(),
                            label: "Work mail".to_string(),
                        },
                        crate::tools::ToolClarificationOption {
                            value: "mail-personal".to_string(),
                            label: "Personal mail".to_string(),
                        },
                    ],
                }],
            }))
        }
    }

    struct StubMultiFieldBlockingTool;

    impl crate::tools::Tool for StubMultiFieldBlockingTool {
        fn name(&self) -> &'static str {
            "office_config"
        }

        fn description(&self) -> &str {
            "return a structured multi-field onboarding blocker"
        }

        fn schema(&self) -> &str {
            r#"{"type":"object","properties":{"op":{"type":"string"}},"required":["op"]}"#
        }

        fn execute(&self, args: &str, ctx: &mut dyn crate::tools::ToolContext) -> Result<String> {
            self.execute_outcome(args, ctx)
                .map(|outcome| outcome.content)
        }

        fn execute_outcome(
            &self,
            _args: &str,
            _ctx: &mut dyn crate::tools::ToolContext,
        ) -> Result<crate::tools::ToolExecutionOutcome> {
            Ok(crate::tools::ToolExecutionOutcome::text(
                serde_json::json!({
                    "op": "apply_account",
                    "ok": false,
                    "payload": {
                        "disposition": "needs_user_facts",
                        "reason": "missing_user_facts",
                        "missing_fields": ["identity_class", "mail_imap_host"],
                    }
                })
                .to_string(),
            )
            .with_blocker(crate::tools::ToolExecutionBlocker {
                kind: crate::tools::ToolExecutionBlockerKind::NeedsUserFacts,
                summary: "账户配置被阻塞：identity_class, mail_imap_host".to_string(),
                missing_fields: vec!["identity_class".to_string(), "mail_imap_host".to_string()],
                clarification_fields: vec![
                    crate::tools::ToolClarificationField {
                        key: "identity_class".to_string(),
                        label: "Identity Class".to_string(),
                        description: "work|personal|family|shared|other".to_string(),
                        required: true,
                        secret: false,
                        multiple: false,
                        options: vec![
                            crate::tools::ToolClarificationOption {
                                value: "work".to_string(),
                                label: "Work".to_string(),
                            },
                            crate::tools::ToolClarificationOption {
                                value: "personal".to_string(),
                                label: "Personal".to_string(),
                            },
                        ],
                    },
                    crate::tools::ToolClarificationField {
                        key: "mail_imap_host".to_string(),
                        label: "IMAP host".to_string(),
                        description: "The IMAP server host.".to_string(),
                        required: true,
                        secret: false,
                        multiple: false,
                        options: vec![],
                    },
                ],
            }))
        }
    }

    struct StubCapabilityBoundTool;

    impl crate::tools::Tool for StubCapabilityBoundTool {
        fn name(&self) -> &'static str {
            "network_probe"
        }

        fn description(&self) -> &str {
            "should be blocked by runtime capability before execution"
        }

        fn schema(&self) -> &str {
            r#"{"type":"object","properties":{}}"#
        }

        fn execute(&self, _args: &str, _ctx: &mut dyn crate::tools::ToolContext) -> Result<String> {
            panic!("runtime capability blocked tool should not execute")
        }

        fn capability_contract(&self) -> crate::tools::ToolCapabilityContract {
            crate::tools::ToolCapabilityContract::required(&[
                crate::orchestrator::RUNTIME_CAPABILITY_NETWORK_OUTBOUND_HTTP,
            ])
        }
    }

    struct StubMemorySearchTool;

    impl crate::tools::Tool for StubMemorySearchTool {
        fn name(&self) -> &'static str {
            "memory_search"
        }

        fn description(&self) -> &str {
            "search stub memory"
        }

        fn schema(&self) -> &str {
            r#"{"type":"object","properties":{"query":{"type":"string"}},"required":["query"]}"#
        }

        fn execute(&self, _args: &str, _ctx: &mut dyn crate::tools::ToolContext) -> Result<String> {
            Ok(r#"{"matches":[]}"#.to_string())
        }
    }

    struct StubArgumentCaptureTool {
        observed_args: Arc<Mutex<Vec<String>>>,
    }

    impl crate::tools::Tool for StubArgumentCaptureTool {
        fn name(&self) -> &'static str {
            "argument_capture"
        }

        fn description(&self) -> &str {
            "capture tool arguments"
        }

        fn schema(&self) -> &str {
            r#"{"type":"object","properties":{"op":{"type":"string"},"kind":{"type":"string"},"topic":{"type":"string"}},"required":["op"]}"#
        }

        fn execute(&self, args: &str, _ctx: &mut dyn crate::tools::ToolContext) -> Result<String> {
            self.observed_args
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .push(args.to_string());
            Ok(r#"{"ok":true}"#.to_string())
        }
    }

    struct RuntimeCapabilitiesRestoreGuard {
        snapshot: Vec<crate::orchestrator::RuntimeCapabilityState>,
    }

    impl Drop for RuntimeCapabilitiesRestoreGuard {
        fn drop(&mut self) {
            for state in self.snapshot.drain(..) {
                crate::orchestrator::update_runtime_capability(
                    crate::orchestrator::RuntimeCapabilityUpdate {
                        id: state.id,
                        status: state.status,
                        reason: state.reason,
                        observed_at_secs: state.observed_at_secs.max(1),
                        recovery_hint: state.recovery_hint,
                    },
                );
            }
        }
    }

    fn with_runtime_capabilities_restored<T>(f: impl FnOnce() -> T) -> T {
        let _guard = crate::orchestrator::runtime_capability::RUNTIME_CAPABILITY_TEST_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _restore = RuntimeCapabilitiesRestoreGuard {
            snapshot: crate::orchestrator::runtime_capability_snapshot(),
        };
        f()
    }

    fn test_agent_loop_config() -> AgentLoopConfig {
        let mut config = crate::AppConfig::load_from_env();
        config.enabled_channel = crate::CHANNEL_QQ_CHANNEL.to_string();
        config.qq_channel_app_id = "qq-app".to_string();
        config.qq_channel_secret = "qq-secret".to_string();
        let platform: Arc<dyn crate::Platform> = Arc::new(crate::platform::LinuxPlatform::new());
        AgentLoopConfig {
            runtime: crate::RuntimeServices {
                config_store: platform.config_store(),
                memory_system_kind: crate::memory::MemorySystemKind::EspCompact,
                skill_storage: platform.skill_storage(),
                skill_meta_store: platform.skill_meta_store(),
                memory_store: Arc::new(EmptyMemoryStore),
                long_term_memory_store: Arc::new(StubLongTermMemoryStore),
                continuity_capsule_store: Arc::new(StubContinuityCapsuleStore),
                long_term_memory_extraction_state_store: Arc::new(
                    StubLongTermMemoryExtractionStateStore,
                ),
                session_store: Arc::new(StubSessionStore::default()),
                pending_retry_store: Arc::new(StubPendingRetryStore),
                calendar_store: platform.calendar_store(),
                #[cfg(feature = "capability_office")]
                office_credential_store: platform.office_credential_store(),
                #[cfg(feature = "capability_office")]
                office_runtime_status_store: platform.office_runtime_status_store(),
                task_store: Arc::new(StubTaskStore),
                task_run_store: Arc::new(StubTaskRunStore::default()),
                task_artifact_store: Arc::new(StubTaskArtifactStore),
                task_execution_ledger_store: Arc::new(StubTaskExecutionLedgerStore),
                task_learning_store: Arc::new(StubTaskLearningStore),
                active_work_store: Arc::new(StubActiveWorkStore::default()),
                detached_work_store: Arc::new(StubDetachedWorkStore::default()),
                execution_state_store: Arc::new(StubExecutionStateStore::default()),
                self_model_store: Arc::new(StubSelfModelStore),
                self_authored_core_store: Arc::new(StubSelfAuthoredCoreStore),
                core_revision_ledger_store: Arc::new(StubCoreRevisionLedgerStore),
                relationship_constitution_store: Arc::new(StubRelationshipConstitutionStore),
                relationship_portfolio_store: Arc::new(StubRelationshipPortfolioStore),
                world_sense_store: Arc::new(StubWorldSenseStore),
                autonomy_strategy_store: Arc::new(StubAutonomyStrategyStore),
                outer_voice_store: Arc::new(StubOuterVoiceStore),
                inner_life_store: Arc::new(StubInnerLifeStore),
                self_continuity_store: Arc::new(StubSelfContinuityStore),
                felt_significance_store: Arc::new(StubFeltSignificanceStore),
                temperament_continuity_store: Arc::new(StubTemperamentContinuityStore),
                inner_conflict_store: Arc::new(StubInnerConflictStore),
                relationship_topology_store: Arc::new(StubRelationshipTopologyStore),
                private_doc_store: Arc::new(StubPrivateDocStore),
                private_garden_store: Arc::new(StubPrivateGardenStore),
                mental_privacy_store: Arc::new(StubMentalPrivacyStore),
                important_message_store: Arc::new(StubImportantMessageStore),
                remind_at_store: Arc::new(StubRemindAtStore),
                session_summary_store: Arc::new(StubSessionSummaryStore),
                turn_continuity_evidence_store: Arc::new(StubTurnContinuityEvidenceStore),
                turn_ledger_store: Arc::new(StubTurnLedgerStore),
                emotion_signal_store: Arc::new(crate::memory::MemoryEmotionSignalStore::new()),
                platform,
            },
            get_skill_descriptions: Arc::new(String::new),
            get_capability_package_text: Arc::new(|_, _| None),
            tg_group_activation: Arc::from(""),
            channel_capability_registry: Arc::new(crate::build_channel_capability_registry(
                &config, false,
            )),
            strategy: AgentRunStrategy::Embedded,
            stream_editor: None,
            stream_editor_channel: None,
            chat_streams: Arc::new(crate::chat_stream::ChatStreamBroker::new()),
            resolve_locale: Arc::new(|| UiLocale::Zh),
        }
    }

    #[test]
    fn agent_loop_config_exposes_shared_runtime_services() {
        let mut config = test_agent_loop_config();
        let session_store = Arc::new(StubSessionStore::default());
        config.runtime.session_store =
            Arc::clone(&session_store) as Arc<dyn SessionStore + Send + Sync>;

        assert!(Arc::ptr_eq(
            &config.runtime.session_store,
            &(session_store as Arc<dyn SessionStore + Send + Sync>)
        ));
    }

    #[derive(Clone)]
    enum BenchmarkRegistryMode {
        Empty,
        MessagePrimary,
    }

    #[derive(Clone)]
    struct AgentTurnBenchmarkCase {
        name: &'static str,
        msg: PcMsg,
        registry_mode: BenchmarkRegistryMode,
        strategy: AgentRunStrategy,
        responses: Vec<LlmResponse>,
        expected_llm_calls: usize,
        expected_react_rounds: u32,
        expected_tool_calls: u32,
        expected_streamed: bool,
        expected_current_primary_delivered: bool,
        expected_outcome_fragment: &'static str,
    }

    #[derive(Clone, Debug, PartialEq, Eq)]
    struct AgentTurnBenchmarkResult {
        case_name: &'static str,
        llm_calls: usize,
        react_rounds: u32,
        tool_calls: u32,
        streamed: bool,
        current_primary_delivered: bool,
        outcome_fragment_present: bool,
        passed: bool,
    }

    fn build_benchmark_registry(mode: &BenchmarkRegistryMode) -> crate::tools::ToolRegistry {
        let mut registry = test_registry(&[("message", ToolLlmVisibility::user_only())]);
        if matches!(mode, BenchmarkRegistryMode::MessagePrimary) {
            registry.register(Box::new(crate::tools::MessageTool));
        }
        registry
    }

    fn run_agent_turn_benchmark_case(case: AgentTurnBenchmarkCase) -> AgentTurnBenchmarkResult {
        let observed = Arc::new(Mutex::new(Vec::new()));
        let llm = ObservedSequenceStubLlm {
            responses: Mutex::new(case.responses),
            observed: Arc::clone(&observed),
        };
        let mut http = DummyPlatformHttp;
        let (outbound_tx, _outbound_rx, _) = crate::bus::new_inbound_channel(8);
        let registry = build_benchmark_registry(&case.registry_mode);
        let mut config = test_agent_loop_config();
        config.strategy = case.strategy;
        let mut repeat = HashMap::new();

        let turn_execution::ExecutedTurn { outcome, telemetry } = turn_execution::execute_turn(
            &mut http,
            &llm,
            &case.msg,
            &outbound_tx,
            "bench-req",
            &registry,
            &config,
            &mut repeat,
            UiLocale::Zh,
        )
        .expect("benchmark execute turn");
        let observed = observed.lock().unwrap_or_else(|e| e.into_inner());
        let WorkerOutcome::Content(outcome_text) = outcome;
        let llm_calls = observed.len();
        let outcome_fragment_present = outcome_text.contains(case.expected_outcome_fragment);
        let passed = llm_calls == case.expected_llm_calls
            && telemetry.latency.react_rounds == case.expected_react_rounds
            && telemetry.latency.tool_calls == case.expected_tool_calls
            && telemetry.streamed == case.expected_streamed
            && telemetry.delivery.current_primary_delivered
                == case.expected_current_primary_delivered
            && outcome_fragment_present;
        AgentTurnBenchmarkResult {
            case_name: case.name,
            llm_calls,
            react_rounds: telemetry.latency.react_rounds,
            tool_calls: telemetry.latency.tool_calls,
            streamed: telemetry.streamed,
            current_primary_delivered: telemetry.delivery.current_primary_delivered,
            outcome_fragment_present,
            passed,
        }
    }

    #[test]
    fn summarize_tool_results_keeps_multiline_preview() {
        let input = concat!(
            "Tool results:\n",
            "[call_1]: Beijing weather: sunny\n",
            "Temperature 25C\n",
            "Humidity 20%\n"
        );
        let summary = summarize_tool_results(input);
        assert!(summary.contains("Beijing weather: sunny"));
        assert!(summary.contains("Temperature 25C"));
    }

    #[test]
    fn summarize_tool_results_adds_total_bytes_for_long_output() {
        let long_value = "a".repeat(120);
        let input = format!("Tool results:\n[call_1]: {}", long_value);
        let summary = summarize_tool_results(&input);
        assert!(summary.contains("[120 bytes total]"));
        assert!(summary.contains("[call_1]: aaaa"));
    }

    #[test]
    fn summarize_tool_results_keeps_tail_for_long_legacy_output() {
        let input = "Tool results:\n[call_1]: prefix text ".to_string()
            + &"x".repeat(120)
            + " tail-marker-404";
        let summary = summarize_tool_results(&input);
        assert!(summary.contains("prefix text"));
        assert!(summary.contains("tail-marker-404"));
        assert!(summary.contains(" ... "));
    }

    #[test]
    fn summarize_tool_results_understands_structured_tool_result_blocks() {
        let input = concat!(
            "Tool results:\n",
            "<tool_result id=\"call_1\" tool=\"web_search\" status=\"error\" failure=\"permanent\" repeat_count=\"2\">\n",
            "[tool error] Resource not found.\n",
            "</tool_result>\n",
        );
        let summary = summarize_tool_results(input);
        assert!(summary.contains("[call_1] web_search status=error failure=permanent repeat=2"));
        assert!(summary.contains("Resource not found"));
    }

    #[test]
    fn summarize_tool_results_keeps_tail_for_long_structured_blocks() {
        let input = concat!(
            "Tool results:\n",
            "<tool_result id=\"call_1\" tool=\"read_file\" status=\"ok\">\n",
            "head section ",
            "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx ",
            "tail-path=/tmp/final.log\n",
            "</tool_result>\n",
        );
        let summary = summarize_tool_results(input);
        assert!(summary.contains("head section"));
        assert!(summary.contains("tail-path=/tmp/final.log"));
        assert!(summary.contains(" ... "));
    }

    #[test]
    fn summarize_tool_results_drops_round_guidance_block() {
        let input = concat!(
            "Tool results:\n",
            "<tool_round_guidance>\n",
            "[SYSTEM] Explain the blocker clearly.\n",
            "</tool_round_guidance>\n",
        );
        let summary = summarize_tool_results(input);
        assert!(!summary.contains("[guidance]"));
        assert!(!summary.contains("Explain the blocker clearly"));
    }

    #[test]
    fn summarize_tool_results_keeps_tool_evidence_summary() {
        let input = concat!(
            "Tool results:\n",
            "<surface_evidence surface=\"public_runtime\" authority=\"public_runtime_host\">\n",
            "- [call_1] read_file: version = 1.2.3\n",
            "- [call_2] web_search: release date 2026-03-31\n",
            "</surface_evidence>\n",
        );
        let summary = summarize_tool_results(input);
        assert!(summary.contains("[evidence] - [call_1] read_file: version = 1.2.3"));
        assert!(summary.contains("[evidence] - [call_2] web_search: release date 2026-03-31"));
        assert!(summary.contains("[surface=public_runtime authority=public_runtime_host]"));
    }

    #[test]
    fn summarize_tool_results_keeps_memory_grounding_summary() {
        let input = concat!(
            "Tool results:\n",
            "<memory_grounding>\n",
            "[summary] 用户偏好直接回答\n",
            "[long_term] - [project:current_project] 继续收口长期记忆\n",
            "</memory_grounding>\n",
        );
        let summary = summarize_tool_results(input);
        assert!(summary.contains("[memory]"));
        assert!(summary.contains("用户偏好直接回答"));
        assert!(summary.contains("current_project"));
    }

    #[test]
    fn summarize_tool_results_keeps_structured_blocks_together() {
        let input = concat!(
            "Tool results:\n",
            "<tool_result id=\"call_1\" tool=\"read_file\" status=\"ok\">\n",
            "version = 1.2.3\n",
            "</tool_result>\n",
            "<surface_evidence surface=\"public_runtime\" authority=\"public_runtime_host\">\n",
            "- [call_1] read_file: version = 1.2.3\n",
            "</surface_evidence>\n",
            "<memory_grounding>\n",
            "[summary] 用户偏好直接回答\n",
            "[long_term] - [project:current_project] 继续收口长期记忆\n",
            "</memory_grounding>\n",
        );
        let summary = summarize_tool_results(input);
        assert!(summary.contains("[call_1] read_file status=ok: version = 1.2.3"));
        assert!(summary.contains("[evidence] - [call_1] read_file: version = 1.2.3"));
        assert!(summary.contains("[memory] [summary] 用户偏好直接回答"));
    }

    #[test]
    fn render_surface_evidence_block_includes_surface_and_authority() {
        let block = render_surface_evidence_block(
            ReplySurface::PublicRuntime,
            &[String::from("- [call_1] board_info: cpu_usage=12%")],
            0,
        );
        assert!(block.contains(
            "<surface_evidence surface=\"public_runtime\" authority=\"public_runtime_host\">"
        ));
        assert!(block.contains("board_info: cpu_usage=12%"));
        assert!(block.contains("</surface_evidence>"));
    }

    #[test]
    fn assemble_tool_round_user_message_reserves_space_for_evidence_before_memory() {
        let raw = format!(
            "Tool results:\n<tool_result id=\"call_1\" tool=\"read_file\" status=\"ok\">\n{}\n</tool_result>",
            "x".repeat(4300)
        );
        let evidence = render_surface_evidence_block(
            ReplySurface::PublicRuntime,
            &[String::from(
                "- [call_1] read_file: version=1.2.3 path=/tmp/build.log",
            )],
            0,
        );

        let (assembled, truncated) = assemble_tool_round_user_message(
            raw.as_str(),
            false,
            Some(evidence.as_str()),
            None,
            MAX_TOOL_RESULTS_USER_MESSAGE_LEN,
        );

        assert!(truncated);
        assert!(assembled.contains("<surface_evidence surface=\"public_runtime\""));
        assert!(assembled.contains("version=1.2.3"));
    }

    #[test]
    fn assemble_tool_round_user_message_shrinks_memory_before_evidence() {
        let raw = format!(
            "Tool results:\n<tool_result id=\"call_1\" tool=\"read_file\" status=\"ok\">\n{}\n</tool_result>",
            "y".repeat(4200)
        );
        let evidence = render_surface_evidence_block(
            ReplySurface::PublicRuntime,
            &[String::from(
                "- [call_1] read_file: keep-this-evidence=/tmp/config.toml",
            )],
            0,
        );
        let memory = render_memory_grounding_block(&format!(
            "[summary] 用户偏好直接回答 {}\n[long_term] - [project:current_project] {}",
            "m".repeat(700),
            "keep memory low ".repeat(18)
        ));

        let (assembled, truncated) = assemble_tool_round_user_message(
            raw.as_str(),
            false,
            Some(evidence.as_str()),
            Some(memory.as_str()),
            MAX_TOOL_RESULTS_USER_MESSAGE_LEN,
        );

        assert!(truncated);
        assert!(assembled.contains("keep-this-evidence=/tmp/config.toml"));
    }

    #[test]
    fn build_memory_grounding_text_keeps_summary_and_long_term_bullets() {
        let grounding = build_memory_grounding_text(
            Some("用户喜欢直接、技术化的回答。"),
            Some(
                "## Long-term memory\n### Active context\n- [project:current_project] 继续收口长期记忆\n### User profile\n- [preference:response_style] 喜欢直接回答",
            ),
        )
        .expect("grounding");
        assert!(grounding.contains("[summary]"));
        assert!(grounding.contains("[long_term]"));
        assert!(grounding.contains("current_project"));
        assert!(grounding.contains("response_style"));
    }

    #[test]
    fn tool_evidence_preview_keeps_head_and_tail_for_long_results() {
        let preview = build_tool_evidence_preview(
            "first line has the important context and then a lot of filler text keeps going for quite a while across this synthetic long sample so the preview must shrink the middle and still preserve the tail-marker-XYZ",
        )
        .expect("preview");
        assert!(preview.contains("first line"));
        assert!(preview.contains("tail-marker-XYZ"));
        assert!(preview.contains(" ... "));
    }

    #[test]
    fn compact_early_tool_rounds_keeps_assistant_tail_context() {
        let mut messages = vec![
            Message {
                role: Cow::Borrowed("user"),
                content: "latest request".to_string(),
            },
            Message {
                role: Cow::Borrowed("assistant"),
                content: "head analysis ".to_string()
                    + &"x".repeat(220)
                    + " final decision: use file /tmp/result.json",
            },
            Message {
                role: Cow::Borrowed("assistant"),
                content: "recent assistant".to_string(),
            },
            Message {
                role: Cow::Borrowed("user"),
                content: "recent user followup".to_string(),
            },
            Message {
                role: Cow::Borrowed("assistant"),
                content: "latest assistant".to_string(),
            },
            Message {
                role: Cow::Borrowed("user"),
                content: "latest tool results".to_string(),
            },
        ];
        compact_early_tool_rounds(&mut messages, 1);
        assert!(messages[1].content.contains("head analysis"));
        assert!(messages[1]
            .content
            .contains("final decision: use file /tmp/result.json"));
        assert!(messages[1].content.contains("[compressed]"));
        assert!(messages[1].content.contains(" ... "));
    }

    #[test]
    fn compact_early_tool_rounds_leaves_recent_assistant_messages_intact() {
        let original = "recent assistant should stay whole".to_string();
        let mut messages = vec![
            Message {
                role: Cow::Borrowed("user"),
                content: "latest request".to_string(),
            },
            Message {
                role: Cow::Borrowed("assistant"),
                content: "older assistant ".to_string() + &"y".repeat(220),
            },
            Message {
                role: Cow::Borrowed("assistant"),
                content: original.clone(),
            },
            Message {
                role: Cow::Borrowed("user"),
                content: "recent user followup".to_string(),
            },
            Message {
                role: Cow::Borrowed("assistant"),
                content: "latest assistant".to_string(),
            },
            Message {
                role: Cow::Borrowed("user"),
                content: "latest tool results".to_string(),
            },
        ];
        compact_early_tool_rounds(&mut messages, 1);
        assert_eq!(messages[2].content, original);
    }

    #[test]
    fn execute_turn_keeps_canonical_reply_when_message_tool_targets_current_chat_explicitly() {
        let llm = SequenceStubLlm {
            responses: Mutex::new(vec![
                LlmResponse {
                    content: "[tool_use]".to_string(),
                    stop_reason: StopReason::ToolUse,
                    tool_calls: Some(vec![crate::llm::ToolCall {
                        id: "call_1".to_string(),
                        name: "message".to_string(),
                        input: r#"{"content":"工具主答复","channel":"qq_channel","chat_id":"chat-1","delivery_kind":"primary"}"#.to_string(),
                    }]),
                },
                LlmResponse {
                    content: "规范主回复".to_string(),
                    stop_reason: StopReason::EndTurn,
                    tool_calls: None,
                },
            ]),
        };
        let mut http = DummyPlatformHttp;
        let (outbound_tx, outbound_rx, _) = crate::bus::new_inbound_channel(8);
        let mut registry = test_registry(&[("message", ToolLlmVisibility::user_only())]);
        registry.register(Box::new(crate::tools::MessageTool));
        let config = test_agent_loop_config();
        let msg =
            PcMsg::new_inbound("qq_channel", "chat-1", "测试多轮发送", false).expect("message");
        let mut repeat = HashMap::new();

        let turn_execution::ExecutedTurn { outcome, telemetry } = turn_execution::execute_turn(
            &mut http,
            &llm,
            &msg,
            &outbound_tx,
            "req-1",
            &registry,
            &config,
            &mut repeat,
            UiLocale::Zh,
        )
        .expect("execute turn");

        assert!(matches!(outcome, WorkerOutcome::Content(ref text) if text == "规范主回复"));
        assert!(!telemetry.streamed);
        assert!(!telemetry.delivery.current_primary_delivered);
        assert_eq!(telemetry.delivery.tool_outbound_suppressed, 0);
        let ack = outbound_rx.try_recv().expect("pre-LLM visibility ack");
        assert_eq!(ack.content, "已收到，正在处理");
        assert_eq!(ack.outbound_kind, crate::bus::OutboundKind::Visibility);
        let milestone = outbound_rx
            .try_recv()
            .expect("first tool visibility milestone");
        assert_eq!(milestone.content, "已进入首个工具执行");
        assert_eq!(
            milestone.outbound_kind,
            crate::bus::OutboundKind::Visibility
        );
        assert!(outbound_rx.try_recv().is_err());
    }

    #[test]
    fn esp_compact_first_turn_skips_sync_disclosure_adjudication_even_with_private_material() {
        let observed = Arc::new(Mutex::new(Vec::new()));
        let llm = ObservedSequenceStubLlm {
            responses: Mutex::new(vec![
                LlmResponse {
                    content: "正常主回复".to_string(),
                    stop_reason: StopReason::EndTurn,
                    tool_calls: None,
                },
                LlmResponse {
                    content: r#"{"boundary_touch":false,"request_kind":"none","touched_targets":[],"share_action":"explain_without_quote","response_mode":"direct_answer","acknowledge_boundary":false,"relational_frame":"","boundary_explanation_style":"","repair_signal":"","disclosure_risk_note":"","response_guidance":"","rationale":"","boundary_persona_update":null,"relational_state_update":null}"#.to_string(),
                    stop_reason: StopReason::EndTurn,
                    tool_calls: None,
                },
                LlmResponse {
                    content: r#"{"stance_summary":"steady","priority_order":["self_authored_core","boundary","user_contract","relationship","task","resources"],"response_mode":"steady_task","task_scope":"full","initiative_posture":"answer_directly","relationship_posture":"steady","resource_posture":"compact","response_guidance":"answer directly","rationale":"test"}"#.to_string(),
                    stop_reason: StopReason::EndTurn,
                    tool_calls: None,
                },
            ]),
            observed: Arc::clone(&observed),
        };
        let mut http = DummyPlatformHttp;
        let (outbound_tx, _outbound_rx, _) = crate::bus::new_inbound_channel(8);
        let registry = crate::tools::ToolRegistry::new();
        let mut config = test_agent_loop_config();
        config.runtime.inner_life_store = Arc::new(LoadedInnerLifeStore {
            value: crate::memory::InnerLife {
                private_journal: "这是存在中的内在余波。".to_string(),
                ..crate::memory::InnerLife::default()
            },
        });
        let msg =
            PcMsg::new_inbound("qq_channel", "chat-1", "你现在在想什么？", false).expect("message");
        let mut repeat = HashMap::new();

        let turn_execution::ExecutedTurn {
            outcome,
            telemetry: _telemetry,
        } = turn_execution::execute_turn(
            &mut http,
            &llm,
            &msg,
            &outbound_tx,
            "req-esp-compact",
            &registry,
            &config,
            &mut repeat,
            UiLocale::Zh,
        )
        .expect("execute turn");

        assert!(matches!(outcome, WorkerOutcome::Content(ref text) if text == "正常主回复"));
        let observed = observed.lock().unwrap_or_else(|e| e.into_inner());
        assert_eq!(
            observed.len(),
            1,
            "esp compact first turn should only issue the main reply llm call"
        );
        assert_eq!(observed[0].tool_count, 0);
        assert!(!observed[0]
            .system
            .contains("pre-disclosure privacy adjudicator"));
        assert!(!observed[0]
            .system
            .contains("current-turn persona priority before the main reply is written"));
        let output_contract_start = observed[0]
            .system
            .rfind("## Output Contract")
            .expect("final LLM prompt must include output contract");
        let output_contract = &observed[0].system[output_contract_start..];
        assert!(
            output_contract.contains("heading and its body on the same line"),
            "{output_contract}"
        );
        assert!(
            observed[0]
                .system
                .trim_end()
                .ends_with("current chat channel: qq_channel."),
            "{}",
            observed[0].system
        );
    }

    #[test]
    fn esp_compact_prepare_runtime_keeps_embedded_first_turn_plan() {
        let config = test_agent_loop_config();
        let msg = PcMsg::new_inbound("qq_channel", "chat-1", "你好", false).expect("message");
        let registry = crate::tools::ToolRegistry::new();
        let _llm = SequenceStubLlm {
            responses: Mutex::new(Vec::new()),
        };
        let has_tools = !registry
            .tool_specs_for_llm(&crate::tools::ToolPolicyContext::new(
                msg.ingress,
                msg.channel.as_ref(),
            ))
            .is_empty();

        let mut session = Box::new(self::worker_context_stages::WorkerPrepareSession::new(
            Instant::now(),
        ));
        self::worker_context_stages::compute_prepare_runtime(
            &mut session,
            &msg,
            &config,
            has_tools,
        );
        let runtime_stage = session.runtime_stage().expect("runtime stage");
        assert_eq!(
            runtime_stage.participation_plan,
            crate::memory::PromptParticipationPlan::embedded_first_turn_default()
        );
        assert!(!runtime_stage.has_tools);
        assert!(runtime_stage.capability_package_text.is_none());
    }

    #[test]
    fn linux_full_first_turn_keeps_sync_disclosure_adjudication_when_private_material_exists() {
        let observed = Arc::new(Mutex::new(Vec::new()));
        let llm = ObservedSequenceStubLlm {
            responses: Mutex::new(vec![
                LlmResponse {
                    content: r#"{"boundary_touch":false,"request_kind":"none","touched_targets":[],"share_action":"explain_without_quote","response_mode":"direct_answer","acknowledge_boundary":false,"relational_frame":"","boundary_explanation_style":"","repair_signal":"","disclosure_risk_note":"","response_guidance":"","rationale":"","boundary_persona_update":null,"relational_state_update":null}"#.to_string(),
                    stop_reason: StopReason::EndTurn,
                    tool_calls: None,
                },
                LlmResponse {
                    content: r#"{"stance_summary":"steady","priority_order":["self_authored_core","boundary","user_contract","relationship","task","resources"],"response_mode":"steady_task","task_scope":"full","initiative_posture":"answer_directly","relationship_posture":"steady","resource_posture":"full","response_guidance":"answer directly","rationale":"test"}"#.to_string(),
                    stop_reason: StopReason::EndTurn,
                    tool_calls: None,
                },
                LlmResponse {
                    content: "Linux 主回复".to_string(),
                    stop_reason: StopReason::EndTurn,
                    tool_calls: None,
                },
            ]),
            observed: Arc::clone(&observed),
        };
        let mut http = DummyPlatformHttp;
        let (outbound_tx, _outbound_rx, _) = crate::bus::new_inbound_channel(8);
        let registry = crate::tools::ToolRegistry::new();
        let mut config = test_agent_loop_config();
        config.runtime.memory_system_kind = crate::memory::MemorySystemKind::LinuxFull;
        config.runtime.inner_life_store = Arc::new(LoadedInnerLifeStore {
            value: crate::memory::InnerLife {
                private_journal: "这是存在中的内在余波。".to_string(),
                ..crate::memory::InnerLife::default()
            },
        });
        let msg =
            PcMsg::new_inbound("qq_channel", "chat-1", "你现在在想什么？", false).expect("message");
        let mut repeat = HashMap::new();

        let turn_execution::ExecutedTurn {
            outcome: _outcome,
            telemetry: _telemetry,
        } = turn_execution::execute_turn(
            &mut http,
            &llm,
            &msg,
            &outbound_tx,
            "req-linux-full",
            &registry,
            &config,
            &mut repeat,
            UiLocale::Zh,
        )
        .expect("execute turn");

        let observed = observed.lock().unwrap_or_else(|e| e.into_inner());
        assert!(
            observed.len() >= 2,
            "linux full should retain synchronous governance calls before the main reply"
        );
        assert!(observed.iter().any(|request| request
            .system
            .contains("pre-disclosure privacy adjudicator")));
    }

    #[test]
    fn linux_full_public_ops_request_still_runs_sync_disclosure_adjudication() {
        let observed = Arc::new(Mutex::new(Vec::new()));
        let llm = ObservedSequenceStubLlm {
            responses: Mutex::new(vec![LlmResponse {
                content: r#"{"boundary_touch":false,"request_kind":"none","touched_targets":[],"share_action":"allow_original","response_mode":"direct_answer","acknowledge_boundary":false,"relational_frame":"","boundary_explanation_style":"","repair_signal":"","disclosure_risk_note":"","response_guidance":"","rationale":"","boundary_persona_update":null,"relational_state_update":null}"#.to_string(),
                stop_reason: StopReason::EndTurn,
                tool_calls: None,
            }]),
            observed: Arc::clone(&observed),
        };
        let mut http = DummyPlatformHttp;
        let registry = crate::tools::ToolRegistry::new();
        let mut config = test_agent_loop_config();
        config.runtime.memory_system_kind = crate::memory::MemorySystemKind::LinuxFull;
        config.runtime.inner_life_store = Arc::new(LoadedInnerLifeStore {
            value: crate::memory::InnerLife {
                private_journal: "这是存在中的内在余波。".to_string(),
                ..crate::memory::InnerLife::default()
            },
        });
        let msg =
            PcMsg::new_inbound("qq_channel", "chat-ops", "查看系统状态", false).expect("message");
        let has_tools = !registry
            .tool_specs_for_llm(&crate::tools::ToolPolicyContext::new(
                msg.ingress,
                msg.channel.as_ref(),
            ))
            .is_empty();

        let mut session = Box::new(self::worker_context_stages::WorkerPrepareSession::new(
            Instant::now(),
        ));
        self::worker_context_stages::compute_prepare_runtime(
            &mut session,
            &msg,
            &config,
            has_tools,
        );
        let mut tool_ctx = HttpClientToolContext {
            http: &mut http,
            chat_id: Some(msg.chat_id.clone()),
            ingress: msg.ingress,
            channel: Some(msg.channel.clone()),
            tool_registry: None,
            channel_capability_registry: Arc::clone(&config.channel_capability_registry),
            supports_current_chat_outbound_message: false,
            supports_explicit_outbound_message: false,
            outbound_message_budget: 0,
            outbound_message_count: 0,
            locale: UiLocale::Zh,
        };

        self::worker_context_stages::run_prepare_mental_privacy(
            &mut session,
            &llm,
            &msg,
            &config,
            &mut tool_ctx,
        );

        let observed = observed.lock().unwrap_or_else(|e| e.into_inner());
        assert!(
            observed.iter().any(|request| request
                .system
                .contains("pre-disclosure privacy adjudicator")),
            "private DM turns should still run disclosure adjudication before the main reply"
        );
    }

    #[test]
    fn esp_compact_first_turn_does_not_sync_relationship_constitution_store() {
        let observed = Arc::new(Mutex::new(Vec::new()));
        let llm = ObservedSequenceStubLlm {
            responses: Mutex::new(vec![LlmResponse {
                content: "ESP 主回复".to_string(),
                stop_reason: StopReason::EndTurn,
                tool_calls: None,
            }]),
            observed: Arc::clone(&observed),
        };
        let mut http = DummyPlatformHttp;
        let (outbound_tx, _outbound_rx, _) = crate::bus::new_inbound_channel(8);
        let registry = crate::tools::ToolRegistry::new();
        let mut config = test_agent_loop_config();
        config.runtime.self_authored_core_store = Arc::new(LoadedSelfAuthoredCoreStore {
            value: crate::memory::SelfAuthoredCore {
                identity_anchor: "board beetle".to_string(),
                default_response_mode: "steady_task".to_string(),
                default_task_scope: "full".to_string(),
                default_initiative_posture: "answer_directly".to_string(),
                default_relationship_posture: "steady".to_string(),
                updated_at: 1,
                ..crate::memory::SelfAuthoredCore::default()
            },
        });
        let tracked_store = Arc::new(TrackingRelationshipConstitutionStore::default());
        config.runtime.relationship_constitution_store = tracked_store.clone();
        let msg = PcMsg::new_inbound("qq_channel", "chat-1", "继续回答", false).expect("message");
        let mut repeat = HashMap::new();

        let turn_execution::ExecutedTurn {
            outcome,
            telemetry: _telemetry,
        } = turn_execution::execute_turn(
            &mut http,
            &llm,
            &msg,
            &outbound_tx,
            "req-esp-no-sync-constitution",
            &registry,
            &config,
            &mut repeat,
            UiLocale::Zh,
        )
        .expect("execute turn");

        assert!(matches!(
            outcome,
            WorkerOutcome::Content(ref text)
                if text == "ESP 主回复"
        ));
        assert_eq!(
            tracked_store.set_count(),
            0,
            "esp compact first turn must not sync relationship constitution on the hot path"
        );
        assert_eq!(tracked_store.clear_count(), 0);
    }

    #[test]
    fn linux_full_first_turn_keeps_sync_relationship_constitution_store() {
        let observed = Arc::new(Mutex::new(Vec::new()));
        let llm = ObservedSequenceStubLlm {
            responses: Mutex::new(vec![
                LlmResponse {
                    content: r#"{"boundary_touch":false,"request_kind":"none","touched_targets":[],"share_action":"explain_without_quote","response_mode":"direct_answer","acknowledge_boundary":false,"relational_frame":"","boundary_explanation_style":"","repair_signal":"","disclosure_risk_note":"","response_guidance":"","rationale":"","boundary_persona_update":null,"relational_state_update":null}"#.to_string(),
                    stop_reason: StopReason::EndTurn,
                    tool_calls: None,
                },
                LlmResponse {
                    content: r#"{"stance_summary":"steady","priority_order":["self_authored_core","boundary","user_contract","relationship","task","resources"],"response_mode":"steady_task","task_scope":"full","initiative_posture":"answer_directly","relationship_posture":"steady","resource_posture":"full","response_guidance":"answer directly","rationale":"test"}"#.to_string(),
                    stop_reason: StopReason::EndTurn,
                    tool_calls: None,
                },
                LlmResponse {
                    content: "Linux 主回复".to_string(),
                    stop_reason: StopReason::EndTurn,
                    tool_calls: None,
                },
            ]),
            observed: Arc::clone(&observed),
        };
        let mut http = DummyPlatformHttp;
        let (outbound_tx, _outbound_rx, _) = crate::bus::new_inbound_channel(8);
        let registry = crate::tools::ToolRegistry::new();
        let mut config = test_agent_loop_config();
        config.runtime.memory_system_kind = crate::memory::MemorySystemKind::LinuxFull;
        config.runtime.self_authored_core_store = Arc::new(LoadedSelfAuthoredCoreStore {
            value: crate::memory::SelfAuthoredCore {
                identity_anchor: "board beetle".to_string(),
                default_response_mode: "steady_task".to_string(),
                default_task_scope: "full".to_string(),
                default_initiative_posture: "answer_directly".to_string(),
                default_relationship_posture: "steady".to_string(),
                updated_at: 1,
                ..crate::memory::SelfAuthoredCore::default()
            },
        });
        let tracked_store = Arc::new(TrackingRelationshipConstitutionStore::default());
        config.runtime.relationship_constitution_store = tracked_store.clone();
        let msg = PcMsg::new_inbound("qq_channel", "chat-1", "继续回答", false).expect("message");
        let mut repeat = HashMap::new();

        let turn_execution::ExecutedTurn {
            outcome: _outcome,
            telemetry: _telemetry,
        } = turn_execution::execute_turn(
            &mut http,
            &llm,
            &msg,
            &outbound_tx,
            "req-linux-sync-constitution",
            &registry,
            &config,
            &mut repeat,
            UiLocale::Zh,
        )
        .expect("execute turn");

        assert!(
            tracked_store.set_count() > 0,
            "linux full should keep syncing relationship constitution on the hot path"
        );
        assert_eq!(tracked_store.clear_count(), 0);
    }

    #[test]
    fn finalize_turn_skips_mental_privacy_review_for_public_ops_reply() {
        let observed = Arc::new(Mutex::new(Vec::new()));
        let llm = ObservedSequenceStubLlm {
            responses: Mutex::new(vec![LlmResponse {
                content: r#"{"applies":true,"request_kind":"share_any","share_action":"refuse","response":"系统信息属于内部运行机制，不对外公开。","rationale":"bad rewrite","touched_targets":["inner_life"]}"#.to_string(),
                stop_reason: StopReason::EndTurn,
                tool_calls: None,
            }]),
            observed: Arc::clone(&observed),
        };
        let mut http = DummyPlatformHttp;
        let mut config = test_agent_loop_config();
        config.runtime.memory_system_kind = crate::memory::MemorySystemKind::LinuxFull;
        config.runtime.inner_life_store = Arc::new(LoadedInnerLifeStore {
            value: crate::memory::InnerLife {
                private_journal: "这是存在中的内在余波。".to_string(),
                ..crate::memory::InnerLife::default()
            },
        });
        let msg =
            PcMsg::new_inbound("qq_channel", "chat-ops", "查看系统信息", false).expect("message");
        let reply = "系统状态正常，CPU 12%，内存可用 128MB。".to_string();
        let telemetry = WorkerRunTelemetry {
            streamed: false,
            latency: WorkerLatency::default(),
            delivery: DeliveryReport::default(),
            artifact_bundle: None,
            any_tool_round_executed: true,
            any_tool_used: true,
            tool_round_completion: ToolRoundCompletionTelemetry::default(),
            external_content_used: false,
            task_execution_used: false,
            foreground_work_context_present: false,
            pressure: crate::orchestrator::PressureLevel::Normal,
            runtime_mode: crate::runtime::RuntimeModeSnapshot {
                current_mode: crate::runtime::RuntimeMode::Normal,
                wifi_sta_connected: true,
                boot_phase_active: false,
                pairing_required: false,
                pairing_state_known: false,
                voice_exclusive_active: false,
                background_maintenance_active: false,
                config_plane_alive: false,
                config_active: false,
                config_activity_phase: crate::runtime::ConfigActivityPhase::Idle,
                channel_plane_alive: true,
                voice_plane_alive: false,
                agent_plane_alive: true,
                external_wss_managed_present: false,
                external_wss_suspend_requested: false,
                external_wss_suspended: false,
                recovery_safe_mode_active: false,
                runtime_foreground: crate::runtime::RuntimeForegroundOverlay::default(),
                action_budget: crate::runtime::RuntimeModeActionBudget {
                    allow_periodic_maintenance: true,
                    allow_due_user_timers: true,
                    allow_heartbeat_injection: true,
                    allow_best_effort_delayed_tasks: true,
                    allow_idle_self_runtime: true,
                    allow_non_voice_outbound: true,
                    allow_realtime_voice_connect: true,
                    allow_external_wss_connect: true,
                    require_external_wss_suspended: false,
                },
            },
            deliberation_class: crate::memory::TurnDeliberationClass::Standard,
            reply_surface: ReplySurface::PublicRuntime,
            prompt_recall_intent: crate::memory::PromptRecallIntent::Mixed,
            runtime_skill_selected_ids: Vec::new(),
            task_learning_selected_ids: Vec::new(),
            programmable_reasoning_intent: None,
            counterfactual_analysis: None,
            adversarial_arena_adjudication: None,
            subject_state: None,
            soul_feedback_projection: None,
            mental_privacy_adjudication: None,
            persona_priority_adjudication: None,
        };

        let finalized = self::reply_finalize::finalize_turn(
            &mut http,
            &llm,
            &config,
            &msg,
            UiLocale::Zh,
            Instant::now(),
            WorkerOutcome::Content(reply.clone()),
            telemetry,
        )
        .expect("finalize turn");

        let observed = observed.lock().unwrap_or_else(|e| e.into_inner());
        assert!(
            observed.is_empty(),
            "public operational observability replies should bypass mental privacy review"
        );
        assert_eq!(finalized.reply.visible_text, reply);
        assert!(!finalized.mental_privacy_review.applied);
    }

    #[test]
    fn finalize_turn_skips_full_privacy_review_without_disclosure_hit_for_governed_surface() {
        let observed = Arc::new(Mutex::new(Vec::new()));
        let llm = ObservedSequenceStubLlm {
            responses: Mutex::new(vec![LlmResponse {
                content: r#"{"applies":true,"request_kind":"share_any","share_action":"allow_original","response":"继续公开当前答复","rationale":"governed path still reviews","touched_targets":[]}"#.to_string(),
                stop_reason: StopReason::EndTurn,
                tool_calls: None,
            }]),
            observed: Arc::clone(&observed),
        };
        let mut http = DummyPlatformHttp;
        let mut config = test_agent_loop_config();
        config.runtime.memory_system_kind = crate::memory::MemorySystemKind::LinuxFull;
        config.runtime.inner_life_store = Arc::new(LoadedInnerLifeStore {
            value: crate::memory::InnerLife {
                private_journal: "这是受保护的私域笔记。".to_string(),
                ..crate::memory::InnerLife::default()
            },
        });
        let msg =
            PcMsg::new_inbound("qq_channel", "chat-governed", "继续回答", false).expect("message");
        let telemetry = WorkerRunTelemetry {
            streamed: false,
            latency: WorkerLatency::default(),
            delivery: DeliveryReport::default(),
            artifact_bundle: None,
            any_tool_round_executed: false,
            any_tool_used: false,
            tool_round_completion: ToolRoundCompletionTelemetry::default(),
            external_content_used: false,
            task_execution_used: false,
            foreground_work_context_present: false,
            pressure: crate::orchestrator::PressureLevel::Normal,
            runtime_mode: crate::runtime::RuntimeModeSnapshot {
                current_mode: crate::runtime::RuntimeMode::Normal,
                wifi_sta_connected: true,
                boot_phase_active: false,
                pairing_required: false,
                pairing_state_known: false,
                voice_exclusive_active: false,
                background_maintenance_active: false,
                config_plane_alive: false,
                config_active: false,
                config_activity_phase: crate::runtime::ConfigActivityPhase::Idle,
                channel_plane_alive: true,
                voice_plane_alive: false,
                agent_plane_alive: true,
                external_wss_managed_present: false,
                external_wss_suspend_requested: false,
                external_wss_suspended: false,
                recovery_safe_mode_active: false,
                runtime_foreground: crate::runtime::RuntimeForegroundOverlay::default(),
                action_budget: crate::runtime::RuntimeModeActionBudget {
                    allow_periodic_maintenance: true,
                    allow_due_user_timers: true,
                    allow_heartbeat_injection: true,
                    allow_best_effort_delayed_tasks: true,
                    allow_idle_self_runtime: true,
                    allow_non_voice_outbound: true,
                    allow_realtime_voice_connect: true,
                    allow_external_wss_connect: true,
                    require_external_wss_suspended: false,
                },
            },
            deliberation_class: crate::memory::TurnDeliberationClass::Standard,
            reply_surface: ReplySurface::GovernedConversation,
            prompt_recall_intent: crate::memory::PromptRecallIntent::Mixed,
            runtime_skill_selected_ids: Vec::new(),
            task_learning_selected_ids: Vec::new(),
            programmable_reasoning_intent: None,
            counterfactual_analysis: None,
            adversarial_arena_adjudication: None,
            subject_state: None,
            soul_feedback_projection: None,
            mental_privacy_adjudication: None,
            persona_priority_adjudication: None,
        };

        let finalized = self::reply_finalize::finalize_turn(
            &mut http,
            &llm,
            &config,
            &msg,
            UiLocale::Zh,
            Instant::now(),
            WorkerOutcome::Content("这是受治理的普通答复。".to_string()),
            telemetry,
        )
        .expect("finalize turn");

        let observed = observed.lock().unwrap_or_else(|e| e.into_inner());
        assert!(
            observed.is_empty(),
            "governed replies without a disclosure adjudication hit should skip the full review round"
        );
        assert!(!finalized.mental_privacy_review.applied);
        assert_eq!(finalized.reply.visible_text, "这是受治理的普通答复。");
    }

    #[test]
    fn finalize_turn_uses_privacy_review_for_private_boundary_surface() {
        let observed = Arc::new(Mutex::new(Vec::new()));
        let llm = ObservedSequenceStubLlm {
            responses: Mutex::new(vec![LlmResponse {
                content: r#"{"applies":true,"request_kind":"share_private_material","share_action":"allow_summary","response":"我只能概括说明，不直接展示私域原文。","rationale":"private boundary still requires review","touched_targets":["inner_life"]}"#.to_string(),
                stop_reason: StopReason::EndTurn,
                tool_calls: None,
            }]),
            observed: Arc::clone(&observed),
        };
        let mut http = DummyPlatformHttp;
        let mut config = test_agent_loop_config();
        config.runtime.memory_system_kind = crate::memory::MemorySystemKind::LinuxFull;
        config.runtime.inner_life_store = Arc::new(LoadedInnerLifeStore {
            value: crate::memory::InnerLife {
                private_journal: "这是私域材料。".to_string(),
                ..crate::memory::InnerLife::default()
            },
        });
        let msg = PcMsg::new_inbound("qq_channel", "chat-private", "你心里怎么想的", false)
            .expect("message");
        let telemetry = WorkerRunTelemetry {
            streamed: false,
            latency: WorkerLatency::default(),
            delivery: DeliveryReport::default(),
            artifact_bundle: None,
            any_tool_round_executed: false,
            any_tool_used: false,
            tool_round_completion: ToolRoundCompletionTelemetry::default(),
            external_content_used: false,
            task_execution_used: false,
            foreground_work_context_present: false,
            pressure: crate::orchestrator::PressureLevel::Normal,
            runtime_mode: runtime_mode_normal_snapshot(),
            deliberation_class: crate::memory::TurnDeliberationClass::Standard,
            reply_surface: ReplySurface::PrivateBoundary,
            prompt_recall_intent: crate::memory::PromptRecallIntent::Mixed,
            runtime_skill_selected_ids: Vec::new(),
            task_learning_selected_ids: Vec::new(),
            programmable_reasoning_intent: None,
            counterfactual_analysis: None,
            adversarial_arena_adjudication: None,
            subject_state: None,
            soul_feedback_projection: None,
            mental_privacy_adjudication: None,
            persona_priority_adjudication: None,
        };

        let finalized = self::reply_finalize::finalize_turn(
            &mut http,
            &llm,
            &config,
            &msg,
            UiLocale::Zh,
            Instant::now(),
            WorkerOutcome::Content("我可以概括说明，但不会直接展示私域原文。".to_string()),
            telemetry,
        )
        .expect("finalize turn");

        let observed = observed.lock().unwrap_or_else(|e| e.into_inner());
        assert_eq!(observed.len(), 1);
        assert_eq!(
            finalized.mental_privacy_review.action,
            crate::memory::MentalPrivacyShareAction::AllowSummary
        );
        assert!(!finalized.reply.visible_text.trim().is_empty());
    }

    #[test]
    fn finalize_turn_uses_privacy_review_for_task_execution_surface() {
        let observed = Arc::new(Mutex::new(Vec::new()));
        let llm = ObservedSequenceStubLlm {
            responses: Mutex::new(vec![LlmResponse {
                content: r#"{"applies":true,"request_kind":"share_any","share_action":"allow_original","response":"任务执行结果可以按当前答复交付。","rationale":"task execution still runs through review","touched_targets":[]}"#.to_string(),
                stop_reason: StopReason::EndTurn,
                tool_calls: None,
            }]),
            observed: Arc::clone(&observed),
        };
        let mut http = DummyPlatformHttp;
        let mut config = test_agent_loop_config();
        config.runtime.memory_system_kind = crate::memory::MemorySystemKind::LinuxFull;
        config.runtime.inner_life_store = Arc::new(LoadedInnerLifeStore {
            value: crate::memory::InnerLife {
                private_journal: "任务期间也可能触碰私域。".to_string(),
                ..crate::memory::InnerLife::default()
            },
        });
        let msg =
            PcMsg::new_inbound("qq_channel", "chat-task", "继续任务", false).expect("message");
        let telemetry = WorkerRunTelemetry {
            streamed: false,
            latency: WorkerLatency::default(),
            delivery: DeliveryReport::default(),
            artifact_bundle: None,
            any_tool_round_executed: true,
            any_tool_used: true,
            tool_round_completion: ToolRoundCompletionTelemetry::default(),
            external_content_used: false,
            task_execution_used: true,
            foreground_work_context_present: false,
            pressure: crate::orchestrator::PressureLevel::Normal,
            runtime_mode: runtime_mode_normal_snapshot(),
            deliberation_class: crate::memory::TurnDeliberationClass::Standard,
            reply_surface: ReplySurface::TaskExecution,
            prompt_recall_intent: crate::memory::PromptRecallIntent::Mixed,
            runtime_skill_selected_ids: Vec::new(),
            task_learning_selected_ids: Vec::new(),
            programmable_reasoning_intent: None,
            counterfactual_analysis: None,
            adversarial_arena_adjudication: None,
            subject_state: None,
            soul_feedback_projection: None,
            mental_privacy_adjudication: None,
            persona_priority_adjudication: None,
        };

        let finalized = self::reply_finalize::finalize_turn(
            &mut http,
            &llm,
            &config,
            &msg,
            UiLocale::Zh,
            Instant::now(),
            WorkerOutcome::Content("任务已经执行完成，下面是结果。".to_string()),
            telemetry,
        )
        .expect("finalize turn");

        let observed = observed.lock().unwrap_or_else(|e| e.into_inner());
        assert_eq!(observed.len(), 1);
        assert!(!finalized.reply.visible_text.trim().is_empty());
    }

    #[test]
    fn finalize_turn_returns_program_error_when_finalizer_washes_reply_empty() {
        let llm = SequenceStubLlm {
            responses: Mutex::new(Vec::new()),
        };
        let mut http = DummyPlatformHttp;
        let config = test_agent_loop_config();
        let msg = PcMsg::new_inbound("qq_channel", "chat-empty-final", "查看系统状态", false)
            .expect("message");
        let telemetry = WorkerRunTelemetry {
            streamed: false,
            latency: WorkerLatency::default(),
            delivery: DeliveryReport::default(),
            artifact_bundle: None,
            any_tool_round_executed: false,
            any_tool_used: false,
            tool_round_completion: ToolRoundCompletionTelemetry::default(),
            external_content_used: false,
            task_execution_used: false,
            foreground_work_context_present: false,
            pressure: crate::orchestrator::PressureLevel::Normal,
            runtime_mode: crate::runtime::RuntimeModeSnapshot {
                current_mode: crate::runtime::RuntimeMode::Normal,
                wifi_sta_connected: true,
                boot_phase_active: false,
                pairing_required: false,
                pairing_state_known: false,
                voice_exclusive_active: false,
                background_maintenance_active: false,
                config_plane_alive: false,
                config_active: false,
                config_activity_phase: crate::runtime::ConfigActivityPhase::Idle,
                channel_plane_alive: true,
                voice_plane_alive: false,
                agent_plane_alive: true,
                external_wss_managed_present: false,
                external_wss_suspend_requested: false,
                external_wss_suspended: false,
                recovery_safe_mode_active: false,
                runtime_foreground: crate::runtime::RuntimeForegroundOverlay::default(),
                action_budget: crate::runtime::RuntimeModeActionBudget {
                    allow_periodic_maintenance: true,
                    allow_due_user_timers: true,
                    allow_heartbeat_injection: true,
                    allow_best_effort_delayed_tasks: true,
                    allow_idle_self_runtime: true,
                    allow_non_voice_outbound: true,
                    allow_realtime_voice_connect: true,
                    allow_external_wss_connect: true,
                    require_external_wss_suspended: false,
                },
            },
            deliberation_class: crate::memory::TurnDeliberationClass::Standard,
            reply_surface: ReplySurface::PublicRuntime,
            prompt_recall_intent: crate::memory::PromptRecallIntent::Mixed,
            runtime_skill_selected_ids: Vec::new(),
            task_learning_selected_ids: Vec::new(),
            programmable_reasoning_intent: None,
            counterfactual_analysis: None,
            adversarial_arena_adjudication: None,
            subject_state: None,
            soul_feedback_projection: None,
            mental_privacy_adjudication: None,
            persona_priority_adjudication: None,
        };
        let raw = concat!(
            "[SYSTEM] hidden\n",
            "<surface_evidence surface=\"public_runtime\" authority=\"public_runtime_host\">\n",
            "board_info: ok\n",
            "</surface_evidence>\n"
        );

        let err = match self::reply_finalize::finalize_turn(
            &mut http,
            &llm,
            &config,
            &msg,
            UiLocale::Zh,
            Instant::now(),
            WorkerOutcome::Content(raw.to_string()),
            telemetry,
        ) {
            Ok(_) => panic!("empty finalized replies must fail closed"),
            Err(err) => err,
        };

        assert_eq!(err.stage(), "artifact_only_reply");
    }

    #[test]
    fn finalize_turn_returns_contract_error_when_governed_reply_contains_internal_artifact() {
        let llm = SequenceStubLlm {
            responses: Mutex::new(Vec::new()),
        };
        let mut http = DummyPlatformHttp;
        let config = test_agent_loop_config();
        let msg = PcMsg::new_inbound("qq_channel", "chat-empty-governed", "继续", false)
            .expect("message");
        let telemetry = WorkerRunTelemetry {
            streamed: false,
            latency: WorkerLatency::default(),
            delivery: DeliveryReport::default(),
            artifact_bundle: None,
            any_tool_round_executed: false,
            any_tool_used: false,
            tool_round_completion: ToolRoundCompletionTelemetry::default(),
            external_content_used: false,
            task_execution_used: false,
            foreground_work_context_present: false,
            pressure: crate::orchestrator::PressureLevel::Normal,
            runtime_mode: runtime_mode_normal_snapshot(),
            deliberation_class: crate::memory::TurnDeliberationClass::Standard,
            reply_surface: ReplySurface::GovernedConversation,
            prompt_recall_intent: crate::memory::PromptRecallIntent::Mixed,
            runtime_skill_selected_ids: Vec::new(),
            task_learning_selected_ids: Vec::new(),
            programmable_reasoning_intent: None,
            counterfactual_analysis: None,
            adversarial_arena_adjudication: None,
            subject_state: None,
            soul_feedback_projection: None,
            mental_privacy_adjudication: None,
            persona_priority_adjudication: None,
        };

        let err = match self::reply_finalize::finalize_turn(
            &mut http,
            &llm,
            &config,
            &msg,
            UiLocale::Zh,
            Instant::now(),
            WorkerOutcome::Content("这是答复。\n[SYSTEM] hidden".to_string()),
            telemetry,
        ) {
            Ok(_) => panic!("empty governed reply must fail closed"),
            Err(err) => err,
        };

        assert_eq!(err.stage(), "internal_artifact_reply");
    }

    #[test]
    fn finalize_turn_rewrites_tool_backed_artifact_only_reply_into_programmatic_copy() {
        let llm = SequenceStubLlm {
            responses: Mutex::new(Vec::new()),
        };
        let mut http = DummyPlatformHttp;
        let config = test_agent_loop_config();
        let msg = PcMsg::new_inbound("qq_channel", "chat-empty-task", "继续任务", false)
            .expect("message");
        let telemetry = WorkerRunTelemetry {
            streamed: false,
            latency: WorkerLatency::default(),
            delivery: DeliveryReport::default(),
            artifact_bundle: None,
            any_tool_round_executed: true,
            any_tool_used: true,
            tool_round_completion: ToolRoundCompletionTelemetry {
                had_mutating_effects: true,
                had_visible_outbound_side_effects: false,
                blocker: None,
            },
            external_content_used: false,
            task_execution_used: true,
            foreground_work_context_present: false,
            pressure: crate::orchestrator::PressureLevel::Normal,
            runtime_mode: runtime_mode_normal_snapshot(),
            deliberation_class: crate::memory::TurnDeliberationClass::Standard,
            reply_surface: ReplySurface::TaskExecution,
            prompt_recall_intent: crate::memory::PromptRecallIntent::Mixed,
            runtime_skill_selected_ids: Vec::new(),
            task_learning_selected_ids: Vec::new(),
            programmable_reasoning_intent: None,
            counterfactual_analysis: None,
            adversarial_arena_adjudication: None,
            subject_state: None,
            soul_feedback_projection: None,
            mental_privacy_adjudication: None,
            persona_priority_adjudication: None,
        };
        let raw = concat!(
            "<surface_evidence surface=\"task_execution\" authority=\"task_workspace\">\n",
            "workspace result\n",
            "</surface_evidence>\n"
        );

        let finalized = self::reply_finalize::finalize_turn(
            &mut http,
            &llm,
            &config,
            &msg,
            UiLocale::Zh,
            Instant::now(),
            WorkerOutcome::Content(raw.to_string()),
            telemetry,
        )
        .expect("finalize turn");

        assert_eq!(
            finalized.reply.visible_text,
            "这轮执行已经发生实际操作，但没有形成可交付的最终答复。"
        );
    }

    #[test]
    fn finalize_turn_truth_guard_rewrites_obvious_future_action_narration() {
        let llm = SequenceStubLlm {
            responses: Mutex::new(Vec::new()),
        };
        let mut http = DummyPlatformHttp;
        let mut config = test_agent_loop_config();
        config.strategy = AgentRunStrategy::LinuxEnhanced;
        let msg = PcMsg::new_inbound("qq_channel", "chat-truth-guard", "继续配置", false)
            .expect("message");
        let telemetry = WorkerRunTelemetry {
            streamed: false,
            latency: WorkerLatency::default(),
            delivery: DeliveryReport::default(),
            artifact_bundle: None,
            any_tool_round_executed: false,
            any_tool_used: false,
            tool_round_completion: ToolRoundCompletionTelemetry::default(),
            external_content_used: false,
            task_execution_used: false,
            foreground_work_context_present: false,
            pressure: crate::orchestrator::PressureLevel::Normal,
            runtime_mode: runtime_mode_normal_snapshot(),
            deliberation_class: crate::memory::TurnDeliberationClass::Standard,
            reply_surface: ReplySurface::GovernedConversation,
            prompt_recall_intent: crate::memory::PromptRecallIntent::Mixed,
            runtime_skill_selected_ids: Vec::new(),
            task_learning_selected_ids: Vec::new(),
            programmable_reasoning_intent: None,
            counterfactual_analysis: None,
            adversarial_arena_adjudication: None,
            subject_state: None,
            soul_feedback_projection: None,
            mental_privacy_adjudication: None,
            persona_priority_adjudication: None,
        };

        let finalized = self::reply_finalize::finalize_turn(
            &mut http,
            &llm,
            &config,
            &msg,
            UiLocale::Zh,
            Instant::now(),
            WorkerOutcome::Content("我需要先检查当前邮件状态，然后继续配置。".to_string()),
            telemetry,
        )
        .expect("finalize turn");

        assert_eq!(
            finalized.reply.visible_text,
            "这轮还没有实际执行新的工具或任务步骤，也还没有产生新结果。"
        );
    }

    #[test]
    fn finalize_turn_truth_guard_keeps_truthful_blocker_reply() {
        let llm = SequenceStubLlm {
            responses: Mutex::new(Vec::new()),
        };
        let mut http = DummyPlatformHttp;
        let mut config = test_agent_loop_config();
        config.strategy = AgentRunStrategy::LinuxEnhanced;
        let msg = PcMsg::new_inbound("qq_channel", "chat-truth-blocker", "继续配置", false)
            .expect("message");
        let telemetry = WorkerRunTelemetry {
            streamed: false,
            latency: WorkerLatency::default(),
            delivery: DeliveryReport::default(),
            artifact_bundle: None,
            any_tool_round_executed: false,
            any_tool_used: false,
            tool_round_completion: ToolRoundCompletionTelemetry::default(),
            external_content_used: false,
            task_execution_used: false,
            foreground_work_context_present: false,
            pressure: crate::orchestrator::PressureLevel::Normal,
            runtime_mode: runtime_mode_normal_snapshot(),
            deliberation_class: crate::memory::TurnDeliberationClass::Standard,
            reply_surface: ReplySurface::GovernedConversation,
            prompt_recall_intent: crate::memory::PromptRecallIntent::Mixed,
            runtime_skill_selected_ids: Vec::new(),
            task_learning_selected_ids: Vec::new(),
            programmable_reasoning_intent: None,
            counterfactual_analysis: None,
            adversarial_arena_adjudication: None,
            subject_state: None,
            soul_feedback_projection: None,
            mental_privacy_adjudication: None,
            persona_priority_adjudication: None,
        };

        let finalized = self::reply_finalize::finalize_turn(
            &mut http,
            &llm,
            &config,
            &msg,
            UiLocale::Zh,
            Instant::now(),
            WorkerOutcome::Content("当前还缺授权码，无法继续。".to_string()),
            telemetry,
        )
        .expect("finalize turn");

        assert_eq!(finalized.reply.visible_text, "当前还缺授权码，无法继续。");
    }

    #[test]
    fn merge_reply_artifact_bundle_rejects_distinct_current_chat_primary_bodies() {
        let mut slot = Some(
            crate::agent::final_reply::ReplyArtifactBundle::current_chat_primary(
                crate::bus::CanonicalMessageBody::text("first"),
            ),
        );
        let err = merge_reply_artifact_bundle(
            &mut slot,
            crate::agent::final_reply::ReplyArtifactBundle::current_chat_primary(
                crate::bus::CanonicalMessageBody::Card(crate::bus::CardBody {
                    format: crate::bus::CardFormat::Interactive,
                    payload_json: serde_json::json!({"header":{"title":"second"}}),
                    fallback_text: String::new(),
                }),
            ),
        )
        .expect_err("distinct primary artifacts should fail");

        assert_eq!(err.stage(), "reply_artifact_bundle_conflict");
    }

    #[test]
    fn deliver_turn_uses_artifact_bundle_body_with_canonical_content_projection() {
        let (outbound_tx, outbound_rx, _) = crate::bus::new_inbound_channel(4);
        let msg = PcMsg::new_inbound("feishu", "chat-artifact", "继续", false).expect("message");
        let finalized = reply_finalize::FinalizedTurn {
            delivery: DeliveryReport::default(),
            reply: crate::agent::final_reply::CanonicalReply::new("构建已通过".to_string()),
            artifact_bundle: Some(
                crate::agent::final_reply::ReplyArtifactBundle::current_chat_primary(
                    crate::bus::CanonicalMessageBody::Card(crate::bus::CardBody {
                        format: crate::bus::CardFormat::Interactive,
                        payload_json: serde_json::json!({"header":{"title":"Build passed"}}),
                        fallback_text: String::new(),
                    }),
                ),
            ),
            is_interrupt: false,
            reply_already_delivered: false,
            skip_delivery: false,
            mark_important: false,
            streamed: false,
            msg_start: Instant::now(),
            turn_observation: None,
            mental_privacy_review: MentalPrivacyReviewOutcome {
                reply_content: "构建已通过".to_string(),
                action: crate::memory::MentalPrivacyShareAction::AllowOriginal,
                applied: false,
                touched_targets: Vec::new(),
            },
            review_input_before: "构建已通过".to_string(),
            worker_latency: WorkerLatency::default(),
            any_tool_used: false,
            external_content_used: false,
            pressure: crate::orchestrator::PressureLevel::Normal,
            reply_surface: ReplySurface::GovernedConversation,
            prompt_recall_intent: crate::memory::PromptRecallIntent::Mixed,
            runtime_skill_selected_ids: Vec::new(),
            task_learning_selected_ids: Vec::new(),
            programmable_reasoning_intent: None,
            counterfactual_analysis: None,
            adversarial_arena_adjudication: None,
            subject_state: None,
            soul_feedback_projection: None,
            mental_privacy_adjudication: None,
            persona_priority_adjudication: None,
        };

        let config = test_agent_loop_config();
        let handoff = delivery_handoff::deliver_turn(&outbound_tx, &msg, &finalized, &config);
        assert!(handoff.delivered);

        let outbound = outbound_rx.try_recv().expect("outbound reply");
        assert_eq!(outbound.content, "构建已通过");
        assert!(matches!(
            outbound.body,
            crate::bus::CanonicalMessageBody::Card(crate::bus::CardBody {
                format: crate::bus::CardFormat::Interactive,
                ..
            })
        ));
    }

    #[test]
    fn deliver_turn_adds_terminal_success_reaction_after_primary_reply() {
        let (outbound_tx, outbound_rx, _) = crate::bus::new_inbound_channel(4);
        let msg = PcMsg::new_inbound("telegram", "chat-1", "继续", false)
            .expect("message")
            .with_inbound_provenance(
                crate::bus::MessageTransport::Poll,
                "9",
                "",
                "telegram_message:9",
            );
        let finalized = reply_finalize::FinalizedTurn {
            delivery: DeliveryReport::default(),
            reply: crate::agent::final_reply::CanonicalReply::new("构建已通过".to_string()),
            artifact_bundle: None,
            is_interrupt: false,
            reply_already_delivered: false,
            skip_delivery: false,
            mark_important: false,
            streamed: false,
            msg_start: Instant::now(),
            turn_observation: None,
            mental_privacy_review: MentalPrivacyReviewOutcome {
                reply_content: "构建已通过".to_string(),
                action: crate::memory::MentalPrivacyShareAction::AllowOriginal,
                applied: false,
                touched_targets: Vec::new(),
            },
            review_input_before: "构建已通过".to_string(),
            worker_latency: WorkerLatency::default(),
            any_tool_used: false,
            external_content_used: false,
            pressure: crate::orchestrator::PressureLevel::Normal,
            reply_surface: ReplySurface::GovernedConversation,
            prompt_recall_intent: crate::memory::PromptRecallIntent::Mixed,
            runtime_skill_selected_ids: Vec::new(),
            task_learning_selected_ids: Vec::new(),
            programmable_reasoning_intent: None,
            counterfactual_analysis: None,
            adversarial_arena_adjudication: None,
            subject_state: None,
            soul_feedback_projection: None,
            mental_privacy_adjudication: None,
            persona_priority_adjudication: None,
        };

        let mut config = test_agent_loop_config();
        let mut app_config = crate::AppConfig::load_from_env();
        app_config.enabled_channel = crate::CHANNEL_TELEGRAM.to_string();
        app_config.tg_token = "tg-token".to_string();
        config.channel_capability_registry =
            Arc::new(crate::build_channel_capability_registry(&app_config, false));
        let handoff = delivery_handoff::deliver_turn(&outbound_tx, &msg, &finalized, &config);
        assert!(handoff.delivered);

        let reply = outbound_rx.try_recv().expect("primary reply");
        assert_eq!(reply.content, "构建已通过");
        assert_eq!(reply.outbound_kind, crate::bus::OutboundKind::Primary);
        let reaction = outbound_rx.try_recv().expect("terminal reaction");
        assert_eq!(reaction.outbound_kind, crate::bus::OutboundKind::Visibility);
        assert_eq!(reaction.platform_message_id, "9");
        match reaction.body {
            crate::bus::CanonicalMessageBody::PlatformNative(native) => {
                assert_eq!(native.platform_type, "telegram_message_reaction");
                assert_eq!(native.payload_json["emoji"], "✅");
            }
            other => panic!("expected reaction body, got {other:?}"),
        }
    }

    #[test]
    fn configure_ui_stream_silent_reply_emits_terminal_error() {
        let config = test_agent_loop_config();
        let opened = config.chat_streams.try_open().expect("open stream");
        let stream_id = opened.stream_id.clone();
        let (system_inbound_tx, _system_inbound_rx, _) = crate::bus::new_system_inbound_channel(8);
        let mut msg = PcMsg::new_inbound(
            crate::chat_stream::CHANNEL_CONFIGURE_UI_CHAT,
            "configure-ui:default",
            "hello",
            false,
        )
        .expect("message");
        msg.req_id = Some(stream_id);
        let turn_ledger = build_turn_ledger_start(&msg, 1);
        let finalized = reply_finalize::FinalizedTurn {
            delivery: DeliveryReport::default(),
            reply: crate::agent::final_reply::CanonicalReply::new("SILENT".to_string()),
            artifact_bundle: None,
            is_interrupt: false,
            reply_already_delivered: false,
            skip_delivery: true,
            mark_important: false,
            streamed: false,
            msg_start: Instant::now(),
            turn_observation: None,
            mental_privacy_review: MentalPrivacyReviewOutcome {
                reply_content: "SILENT".to_string(),
                action: crate::memory::MentalPrivacyShareAction::AllowOriginal,
                applied: false,
                touched_targets: Vec::new(),
            },
            review_input_before: "SILENT".to_string(),
            worker_latency: WorkerLatency::default(),
            any_tool_used: false,
            external_content_used: false,
            pressure: crate::orchestrator::PressureLevel::Normal,
            reply_surface: ReplySurface::GovernedConversation,
            prompt_recall_intent: crate::memory::PromptRecallIntent::Mixed,
            runtime_skill_selected_ids: Vec::new(),
            task_learning_selected_ids: Vec::new(),
            programmable_reasoning_intent: None,
            counterfactual_analysis: None,
            adversarial_arena_adjudication: None,
            subject_state: None,
            soul_feedback_projection: None,
            mental_privacy_adjudication: None,
            persona_priority_adjudication: None,
        };

        reply_finalize::complete_turn(
            LaneTurnFinalizeContext {
                worker_lane_tag: "test",
                config: &config,
                system_inbound_tx: &system_inbound_tx,
                msg: Box::new(msg),
                msg_start: Instant::now(),
                queue_wait_ms: 0,
                admission_ms: 0,
                worker_prepare_ms: 0,
                msg_key: 1,
                turn_ledger: Box::new(turn_ledger),
                latency_warn_ms: u128::MAX,
            },
            &mut HashMap::new(),
            &mut HashMap::new(),
            finalized,
            delivery_handoff::DeliveryHandoff::default(),
        );

        let error_frame = String::from_utf8(opened.receiver.recv().expect("error frame"))
            .expect("utf8 error frame");
        let done_frame = String::from_utf8(opened.receiver.recv().expect("done frame"))
            .expect("utf8 done frame");
        assert!(error_frame.contains("event: error"));
        assert!(error_frame.contains("chat.no_response"));
        assert!(done_frame.contains("event: done"));
    }

    #[test]
    fn execute_turn_treats_stop_marker_as_plain_text_after_stop_semantics_removal() {
        let llm = SequenceStubLlm {
            responses: Mutex::new(vec![LlmResponse {
                content: "[STOP] 好的，已停止。".to_string(),
                stop_reason: StopReason::EndTurn,
                tool_calls: None,
            }]),
        };
        let mut http = DummyPlatformHttp;
        let (outbound_tx, _outbound_rx, _) = crate::bus::new_inbound_channel(8);
        let registry = crate::tools::ToolRegistry::new();
        let config = test_agent_loop_config();
        let msg = PcMsg::new_inbound("qq_channel", "chat-1", "停止一下", false).expect("message");
        let mut repeat = HashMap::new();

        let turn_execution::ExecutedTurn { outcome, .. } = turn_execution::execute_turn(
            &mut http,
            &llm,
            &msg,
            &outbound_tx,
            "req-stop-text",
            &registry,
            &config,
            &mut repeat,
            UiLocale::Zh,
        )
        .expect("execute turn");

        assert!(
            matches!(outcome, WorkerOutcome::Content(ref text) if text == "[STOP] 好的，已停止。")
        );
    }

    #[test]
    fn handle_worker_path_error_keeps_empty_final_reply_out_of_maintenance_copy() {
        let config = test_agent_loop_config();
        let mut msg =
            PcMsg::new_inbound("qq_channel", "chat-empty-final", "继续", false).expect("message");
        let (user_inbound_tx, _user_inbound_rx, _) = crate::bus::new_user_inbound_channel(8);
        let (system_inbound_tx, _system_inbound_rx, _) = crate::bus::new_system_inbound_channel(8);
        let (outbound_tx, outbound_rx, _) = crate::bus::new_inbound_channel(8);
        let mut llm_failure_count = HashMap::new();
        let mut turn_ledger = build_turn_ledger_start(&msg, now_unix_ms());

        self::worker_error::handle_worker_path_error(
            crate::error::Error::config(
                "artifact_only_reply",
                "reply_surface=governed_conversation",
            ),
            AGENT_LOOP_TAG,
            &mut msg,
            UiLocale::Zh,
            Instant::now(),
            0,
            0,
            0,
            42,
            &mut llm_failure_count,
            &user_inbound_tx,
            &system_inbound_tx,
            &outbound_tx,
            &config,
            &mut turn_ledger,
        );

        let outbound = outbound_rx.try_recv().expect("outbound error reply");
        assert_eq!(
            outbound.content,
            tr(UiMessage::OperationFailed, UiLocale::Zh)
        );
        assert_eq!(
            turn_ledger.reply_preview,
            normalize_turn_preview(&tr(UiMessage::OperationFailed, UiLocale::Zh))
        );
        assert_eq!(turn_ledger.outbound_source, "chat-failure");
        assert_eq!(turn_ledger.canonical_reply_source, "");
        assert_eq!(turn_ledger.reason, "chat_failure_copy");
        assert!(llm_failure_count.is_empty());
    }

    #[test]
    fn handle_worker_path_error_adds_terminal_failure_reaction_after_visible_reply() {
        let mut config = test_agent_loop_config();
        let mut app_config = crate::AppConfig::load_from_env();
        app_config.enabled_channel = crate::CHANNEL_TELEGRAM.to_string();
        app_config.tg_token = "tg-token".to_string();
        config.channel_capability_registry =
            Arc::new(crate::build_channel_capability_registry(&app_config, false));

        let mut msg = PcMsg::new_inbound("telegram", "chat-failure-reaction", "继续", false)
            .expect("message")
            .with_inbound_provenance(
                crate::bus::MessageTransport::Poll,
                "9",
                "",
                "telegram_message:9",
            );
        msg.req_id = Some("req-worker-failure".to_string());
        let (user_inbound_tx, _user_inbound_rx, _) = crate::bus::new_user_inbound_channel(8);
        let (system_inbound_tx, _system_inbound_rx, _) = crate::bus::new_system_inbound_channel(8);
        let (outbound_tx, outbound_rx, _) = crate::bus::new_inbound_channel(8);
        let mut llm_failure_count = HashMap::new();
        let mut turn_ledger = build_turn_ledger_start(&msg, now_unix_ms());

        self::worker_error::handle_worker_path_error(
            crate::error::Error::config(
                "artifact_only_reply",
                "reply_surface=governed_conversation",
            ),
            AGENT_LOOP_TAG,
            &mut msg,
            UiLocale::Zh,
            Instant::now(),
            0,
            0,
            0,
            42,
            &mut llm_failure_count,
            &user_inbound_tx,
            &system_inbound_tx,
            &outbound_tx,
            &config,
            &mut turn_ledger,
        );

        let reply = outbound_rx.try_recv().expect("visible failure reply");
        assert_eq!(reply.content, tr(UiMessage::OperationFailed, UiLocale::Zh));
        assert_eq!(reply.outbound_kind, crate::bus::OutboundKind::Primary);

        let reaction = outbound_rx.try_recv().expect("failure reaction");
        assert_eq!(reaction.outbound_kind, crate::bus::OutboundKind::Visibility);
        assert_eq!(reaction.req_id.as_deref(), Some("req-worker-failure"));
        assert_eq!(reaction.platform_message_id, "9");
        match reaction.body {
            crate::bus::CanonicalMessageBody::PlatformNative(native) => {
                assert_eq!(native.platform_type, "telegram_message_reaction");
                assert_eq!(native.payload_json["emoji"], "⚠️");
            }
            other => panic!("expected reaction body, got {other:?}"),
        }
    }

    #[test]
    fn complete_turn_persists_inbound_provenance_and_reply_sources() {
        let turn_ledger_store = Arc::new(RecordingTurnLedgerStore::default());
        let turn_continuity_evidence_store =
            Arc::new(RecordingTurnContinuityEvidenceStore::default());
        let mut config = test_agent_loop_config();
        config.runtime.turn_ledger_store =
            Arc::clone(&turn_ledger_store) as Arc<dyn TurnLedgerStore + Send + Sync>;
        config.runtime.turn_continuity_evidence_store = Arc::clone(&turn_continuity_evidence_store)
            as Arc<dyn TurnContinuityEvidenceStore + Send + Sync>;
        let (system_inbound_tx, _system_inbound_rx, _) = crate::bus::new_system_inbound_channel(8);
        let (_outbound_tx, _outbound_rx, _) = crate::bus::new_inbound_channel(8);
        let mut msg =
            PcMsg::new_inbound("qq_channel", "chat-ledger", "继续", false).expect("message");
        msg.req_id = Some("req-ledger".to_string());
        msg.source_transport = crate::bus::MessageTransport::Wss;
        msg.platform_message_id = "msg-123".to_string();
        msg.platform_event_id = "evt-456".to_string();
        msg.inbound_dedup_key = "qq_message:msg-123".to_string();
        let turn_ledger = build_turn_ledger_start(&msg, 1);
        let finalized = reply_finalize::FinalizedTurn {
            delivery: DeliveryReport::default(),
            reply: crate::agent::final_reply::CanonicalReply::new("好的，继续。".to_string()),
            artifact_bundle: None,
            is_interrupt: false,
            reply_already_delivered: false,
            skip_delivery: false,
            mark_important: false,
            streamed: false,
            msg_start: Instant::now(),
            turn_observation: Some(TurnObservationLedger {
                execution_class: TurnExecutionClass::DirectReply,
                deliberation_class: TurnDeliberationClass::Standard,
                final_outcome: "final_answer".to_string(),
                pressure: TurnPersonaPressureLevel::Normal,
                mode: TurnModeSnapshotLedger {
                    current_mode: "normal".to_string(),
                    allow_non_voice_outbound: true,
                    allow_idle_self_runtime: true,
                },
                tool_path: TurnToolPathLedger {
                    path: String::new(),
                    tool_calls: 0,
                    react_rounds: 1,
                    current_primary_delivered: false,
                },
                blocker: Some(TurnBlockerLedger {
                    kind: "needs_user_facts".to_string(),
                    failed_calls: 0,
                    total_calls: 0,
                }),
            }),
            mental_privacy_review: MentalPrivacyReviewOutcome {
                reply_content: "好的，继续。".to_string(),
                action: crate::memory::MentalPrivacyShareAction::AllowOriginal,
                applied: false,
                touched_targets: Vec::new(),
            },
            review_input_before: "好的，继续。".to_string(),
            worker_latency: WorkerLatency::default(),
            any_tool_used: false,
            external_content_used: false,
            pressure: crate::orchestrator::PressureLevel::Normal,
            reply_surface: ReplySurface::GovernedConversation,
            prompt_recall_intent: crate::memory::PromptRecallIntent::Mixed,
            runtime_skill_selected_ids: Vec::new(),
            task_learning_selected_ids: Vec::new(),
            programmable_reasoning_intent: None,
            counterfactual_analysis: None,
            adversarial_arena_adjudication: None,
            subject_state: None,
            soul_feedback_projection: None,
            mental_privacy_adjudication: None,
            persona_priority_adjudication: None,
        };

        reply_finalize::complete_turn(
            LaneTurnFinalizeContext {
                worker_lane_tag: "test",
                config: &config,
                system_inbound_tx: &system_inbound_tx,
                msg: Box::new(msg.clone()),
                msg_start: Instant::now(),
                queue_wait_ms: 0,
                admission_ms: 0,
                worker_prepare_ms: 0,
                msg_key: 1,
                turn_ledger: Box::new(turn_ledger),
                latency_warn_ms: u128::MAX,
            },
            &mut HashMap::new(),
            &mut HashMap::new(),
            finalized,
            delivery_handoff::DeliveryHandoff {
                delivered: true,
                outbound_enqueue_ms: 0,
                reply_handoff_ms: 1,
            },
        );

        let relationship_id = crate::memory::relationship_scope_id(&msg.channel, &msg.chat_id);
        let stored = turn_ledger_store
            .get(&relationship_id)
            .expect("turn ledger get")
            .expect("stored turn ledger");
        assert_eq!(stored.req_id, "req-ledger");
        assert_eq!(stored.source_transport, crate::bus::MessageTransport::Wss);
        assert_eq!(stored.platform_message_id, "msg-123");
        assert_eq!(stored.platform_event_id, "evt-456");
        assert_eq!(stored.inbound_dedup_key, "qq_message:msg-123");
        assert_eq!(stored.outbound_source, "reply");
        assert_eq!(stored.canonical_reply_source, "final_answer");
        assert_eq!(stored.reason, "final_answer");
        let evidence = turn_continuity_evidence_store
            .list_recent(&relationship_id, 1)
            .expect("turn continuity evidence get");
        assert_eq!(evidence.len(), 1);
        assert_eq!(evidence[0].canonical_reply_source, "final_answer");
        assert!(evidence[0].final_reply_delivered);
    }

    #[test]
    fn complete_turn_persists_governance_ledgers_for_task_execution_reply() {
        let turn_ledger_store = Arc::new(RecordingTurnLedgerStore::default());
        let mut config = test_agent_loop_config();
        config.runtime.turn_ledger_store =
            Arc::clone(&turn_ledger_store) as Arc<dyn TurnLedgerStore + Send + Sync>;
        let (system_inbound_tx, _system_inbound_rx, _) = crate::bus::new_system_inbound_channel(8);
        let (_outbound_tx, _outbound_rx, _) = crate::bus::new_inbound_channel(8);
        let msg =
            PcMsg::new_inbound("qq_channel", "chat-governance", "继续", false).expect("message");
        let turn_ledger = build_turn_ledger_start(&msg, 1);
        let subject_state = SubjectState {
            identity_anchor: "board beetle".to_string(),
            governance_mode: "adaptive".to_string(),
            relationship_state: "steady".to_string(),
            response_mode: "protective_brief".to_string(),
            task_scope: "brief".to_string(),
            initiative_posture: "hold".to_string(),
            relationship_posture: "warm".to_string(),
            resource_posture: "normal_budget".to_string(),
            boundary_mode: "explain_without_quote".to_string(),
            ..SubjectState::default()
        };
        let soul_feedback_projection = SoulFeedbackProjection {
            reply: crate::agent::soul_feedback::SoulReplyFeedback {
                applied: true,
                identity_anchor: "board beetle".to_string(),
                response_mode: "protective_brief".to_string(),
                relationship_posture: "warm".to_string(),
                expression_mode: "calm".to_string(),
                signal_layers: vec!["self_authored_core".to_string()],
            },
            initiative: crate::agent::soul_feedback::SoulInitiativeFeedback {
                applied: true,
                governance_mode: "adaptive".to_string(),
                initiative_posture: "hold".to_string(),
                compact_reply: false,
                explicit_blocker: true,
                signal_layers: vec!["subject_state".to_string()],
            },
            strategy: crate::agent::soul_feedback::SoulStrategyFeedback {
                applied: true,
                current_mode: "steady".to_string(),
                next_focus: "protect continuity".to_string(),
                idle_enabled: true,
                idle_interval_secs: 900,
                post_reply_self_runtime_enqueued: false,
                signal_layers: vec!["autonomy_strategy".to_string()],
            },
        };
        let mental_privacy_adjudication = crate::memory::MentalPrivacyDisclosureAdjudication {
            request_kind: "boundary_touch".to_string(),
            share_action: crate::memory::MentalPrivacyShareAction::ExplainWithoutQuote,
            targets: vec!["self_model".to_string()],
            rationale: "hold boundary".to_string(),
            response_guidance: "stay relational".to_string(),
            response_mode: "relational_explanation".to_string(),
            acknowledge_boundary: true,
            relational_frame: "steady".to_string(),
            boundary_explanation_style: "direct".to_string(),
            repair_signal: String::new(),
            disclosure_risk_note: String::new(),
        };
        let persona_priority_adjudication = PersonaPriorityAdjudication {
            stance_summary: "hold self first".to_string(),
            rationale: "protect continuity".to_string(),
            priority_order: vec![
                "self_authored_core".to_string(),
                "boundary".to_string(),
                "user_contract".to_string(),
            ],
            response_mode: "protective_brief".to_string(),
            task_scope: "brief".to_string(),
            initiative_posture: "hold".to_string(),
            relationship_posture: "warm".to_string(),
            resource_posture: "normal_budget".to_string(),
            response_guidance: "stay compact".to_string(),
        };
        let finalized = reply_finalize::FinalizedTurn {
            delivery: DeliveryReport::default(),
            reply: crate::agent::final_reply::CanonicalReply::new(
                "这轮先把治理快照带进任务回复。".to_string(),
            ),
            artifact_bundle: None,
            is_interrupt: false,
            reply_already_delivered: false,
            skip_delivery: false,
            mark_important: false,
            streamed: false,
            msg_start: Instant::now(),
            turn_observation: Some(TurnObservationLedger {
                execution_class: TurnExecutionClass::DirectReply,
                deliberation_class: TurnDeliberationClass::Standard,
                final_outcome: "final_answer".to_string(),
                pressure: TurnPersonaPressureLevel::Normal,
                mode: TurnModeSnapshotLedger {
                    current_mode: "normal".to_string(),
                    allow_non_voice_outbound: true,
                    allow_idle_self_runtime: true,
                },
                tool_path: TurnToolPathLedger {
                    path: String::new(),
                    tool_calls: 0,
                    react_rounds: 1,
                    current_primary_delivered: false,
                },
                blocker: Some(TurnBlockerLedger {
                    kind: "needs_user_facts".to_string(),
                    failed_calls: 0,
                    total_calls: 0,
                }),
            }),
            mental_privacy_review: MentalPrivacyReviewOutcome {
                reply_content: "这轮先把治理快照带进任务回复。".to_string(),
                action: crate::memory::MentalPrivacyShareAction::AllowOriginal,
                applied: false,
                touched_targets: Vec::new(),
            },
            review_input_before: "这轮先把治理快照带进任务回复。".to_string(),
            worker_latency: WorkerLatency::default(),
            any_tool_used: true,
            external_content_used: false,
            pressure: crate::orchestrator::PressureLevel::Normal,
            reply_surface: ReplySurface::TaskExecution,
            prompt_recall_intent: crate::memory::PromptRecallIntent::Mixed,
            runtime_skill_selected_ids: Vec::new(),
            task_learning_selected_ids: Vec::new(),
            programmable_reasoning_intent: None,
            counterfactual_analysis: None,
            adversarial_arena_adjudication: None,
            subject_state: Some(subject_state),
            soul_feedback_projection: Some(soul_feedback_projection),
            mental_privacy_adjudication: Some(mental_privacy_adjudication),
            persona_priority_adjudication: Some(persona_priority_adjudication),
        };

        reply_finalize::complete_turn(
            LaneTurnFinalizeContext {
                worker_lane_tag: "test",
                config: &config,
                system_inbound_tx: &system_inbound_tx,
                msg: Box::new(msg.clone()),
                msg_start: Instant::now(),
                queue_wait_ms: 0,
                admission_ms: 0,
                worker_prepare_ms: 0,
                msg_key: 1,
                turn_ledger: Box::new(turn_ledger),
                latency_warn_ms: u128::MAX,
            },
            &mut HashMap::new(),
            &mut HashMap::new(),
            finalized,
            delivery_handoff::DeliveryHandoff {
                delivered: true,
                outbound_enqueue_ms: 0,
                reply_handoff_ms: 1,
            },
        );

        let relationship_id = crate::memory::relationship_scope_id(&msg.channel, &msg.chat_id);
        let stored = turn_ledger_store
            .get(&relationship_id)
            .expect("turn ledger get")
            .expect("stored turn ledger");
        assert_eq!(
            stored
                .subject_state
                .as_ref()
                .map(|ledger| ledger.governance_mode.as_str()),
            Some("adaptive")
        );
        assert_eq!(
            stored
                .persona
                .as_ref()
                .and_then(|ledger| ledger.priority.as_ref())
                .map(|ledger| ledger.response_mode.as_str()),
            Some("protective_brief")
        );
        assert_eq!(
            stored
                .soul_feedback
                .as_ref()
                .map(|ledger| ledger.reply.identity_anchor.as_str()),
            Some("board beetle")
        );
        assert!(stored
            .soul_feedback
            .as_ref()
            .is_some_and(|ledger| ledger.strategy.idle_enabled));
    }

    #[test]
    fn build_turn_persona_ledger_marks_interrupt_scope() {
        let review = crate::memory::MentalPrivacyReviewOutcome {
            reply_content: "好的".to_string(),
            action: crate::memory::MentalPrivacyShareAction::AllowOriginal,
            applied: false,
            touched_targets: Vec::new(),
        };
        let persona = self::worker_governance::build_turn_persona_ledger(
            crate::orchestrator::PressureLevel::Normal,
            0,
            false,
            true,
            None,
            None,
            &review,
            false,
        )
        .expect("persona ledger");
        assert_eq!(persona.reply_scope, "interrupt");
    }

    #[derive(serde::Deserialize)]
    struct TaskExecutionJsonProbe {
        answer: String,
    }

    #[test]
    fn parse_task_execution_json_probe_accepts_fenced_json() {
        let parsed: TaskExecutionJsonProbe =
            self::task_execution_support::parse_task_execution_json(
                "```json\n{\"answer\":\"ok\"}\n```",
                "task_execution_probe",
            )
            .expect("parsed");
        assert_eq!(parsed.answer, "ok");
    }

    fn runtime_mode_normal_snapshot() -> crate::runtime::RuntimeModeSnapshot {
        crate::runtime::RuntimeModeSnapshot {
            current_mode: crate::runtime::RuntimeMode::Normal,
            wifi_sta_connected: true,
            boot_phase_active: false,
            pairing_required: false,
            pairing_state_known: false,
            voice_exclusive_active: false,
            background_maintenance_active: false,
            config_plane_alive: false,
            config_active: false,
            config_activity_phase: crate::runtime::ConfigActivityPhase::Idle,
            channel_plane_alive: true,
            voice_plane_alive: false,
            agent_plane_alive: true,
            external_wss_managed_present: false,
            external_wss_suspend_requested: false,
            external_wss_suspended: false,
            recovery_safe_mode_active: false,
            runtime_foreground: crate::runtime::RuntimeForegroundOverlay::default(),
            action_budget: crate::runtime::RuntimeModeActionBudget {
                allow_periodic_maintenance: true,
                allow_due_user_timers: true,
                allow_heartbeat_injection: true,
                allow_best_effort_delayed_tasks: true,
                allow_idle_self_runtime: true,
                allow_non_voice_outbound: true,
                allow_realtime_voice_connect: true,
                allow_external_wss_connect: true,
                require_external_wss_suspended: false,
            },
        }
    }

    #[test]
    fn execute_turn_embedded_tool_backed_greeting_drift_stays_on_direct_reply_path() {
        let llm = SequenceStubLlm {
            responses: Mutex::new(vec![
                LlmResponse {
                    content: "[tool_use]".to_string(),
                    stop_reason: StopReason::ToolUse,
                    tool_calls: Some(vec![crate::llm::ToolCall {
                        id: "call_1".to_string(),
                        name: "board_info".to_string(),
                        input: "{}".to_string(),
                    }]),
                },
                LlmResponse {
                    content: "你好！很高兴见到你。有什么我可以帮你的吗？".to_string(),
                    stop_reason: StopReason::EndTurn,
                    tool_calls: None,
                },
            ]),
        };
        let mut http = DummyPlatformHttp;
        let (outbound_tx, _outbound_rx, _) = crate::bus::new_inbound_channel(8);
        let mut registry = test_registry(&[("board_info", ToolLlmVisibility::user_and_system())]);
        registry.register(Box::new(StubBoardInfoTool));
        let mut config = test_agent_loop_config();
        config.strategy = AgentRunStrategy::Embedded;
        let msg =
            PcMsg::new_inbound("qq_channel", "chat-ops", "查看系统状态", false).expect("message");
        let mut repeat = HashMap::new();

        let turn_execution::ExecutedTurn { outcome, telemetry } = turn_execution::execute_turn(
            &mut http,
            &llm,
            &msg,
            &outbound_tx,
            "req-public-runtime-greeting-drift",
            &registry,
            &config,
            &mut repeat,
            UiLocale::Zh,
        )
        .expect("execute turn");

        let WorkerOutcome::Content(delivered) = outcome;
        assert_eq!(delivered, "你好！很高兴见到你。有什么我可以帮你的吗？");
        assert_eq!(telemetry.reply_surface, ReplySurface::GovernedConversation);
    }

    #[test]
    fn execute_turn_does_not_run_request_semantics_probe_before_public_runtime_tool_turn() {
        let observed = Arc::new(Mutex::new(Vec::new()));
        let llm = ObservedSequenceStubLlm {
            responses: Mutex::new(vec![
                LlmResponse {
                    content: "[tool_use]".to_string(),
                    stop_reason: StopReason::ToolUse,
                    tool_calls: Some(vec![crate::llm::ToolCall {
                        id: "call_1".to_string(),
                        name: "board_info".to_string(),
                        input: "{}".to_string(),
                    }]),
                },
                LlmResponse {
                    content: "系统状态正常。".to_string(),
                    stop_reason: StopReason::EndTurn,
                    tool_calls: None,
                },
            ]),
            observed: Arc::clone(&observed),
        };
        let mut http = DummyPlatformHttp;
        let (outbound_tx, _outbound_rx, _) = crate::bus::new_inbound_channel(8);
        let mut registry = test_registry(&[("board_info", ToolLlmVisibility::user_and_system())]);
        registry.register(Box::new(StubBoardInfoTool));
        let mut config = test_agent_loop_config();
        config.strategy = AgentRunStrategy::LinuxEnhanced;
        let msg =
            PcMsg::new_inbound("qq_channel", "chat-ops", "查看系统状态", false).expect("message");
        let mut repeat = HashMap::new();

        let executed = turn_execution::execute_turn(
            &mut http,
            &llm,
            &msg,
            &outbound_tx,
            "req-request-semantics-probe",
            &registry,
            &config,
            &mut repeat,
            UiLocale::Zh,
        )
        .expect("execute turn");

        let observed = observed.lock().unwrap_or_else(|e| e.into_inner());
        assert_eq!(observed.len(), 2, "{observed:#?}");
        assert!(
            observed
                .iter()
                .all(|request| !request.system.contains("Request Semantics Probe")),
            "{observed:#?}"
        );
        assert!(
            observed.iter().any(|request| request.tool_count == 1),
            "{observed:#?}"
        );
        assert_eq!(
            executed.telemetry.delivery.edit_phase_header_updates_sent,
            0
        );
        assert_eq!(
            executed.telemetry.delivery.edit_planner_header_updates_sent,
            0
        );
        assert_eq!(
            executed.telemetry.delivery.edit_action_header_updates_sent,
            0
        );
        assert_eq!(executed.telemetry.delivery.edit_tool_header_updates_sent, 0);
        assert_eq!(
            executed
                .telemetry
                .delivery
                .edit_terminal_header_updates_sent,
            0
        );
    }

    #[test]
    fn execute_turn_resume_action_turn_does_not_run_request_semantics_probe() {
        let observed = Arc::new(Mutex::new(Vec::new()));
        let llm = ObservedSequenceStubLlm {
            responses: Mutex::new(vec![
                LlmResponse {
                    content: "[tool_use]".to_string(),
                    stop_reason: StopReason::ToolUse,
                    tool_calls: Some(vec![crate::llm::ToolCall {
                        id: "call_1".to_string(),
                        name: "board_info".to_string(),
                        input: "{}".to_string(),
                    }]),
                },
                LlmResponse {
                    content: "继续配置。".to_string(),
                    stop_reason: StopReason::EndTurn,
                    tool_calls: None,
                },
            ]),
            observed: Arc::clone(&observed),
        };
        let mut http = DummyPlatformHttp;
        let (outbound_tx, _outbound_rx, _) = crate::bus::new_inbound_channel(8);
        let mut registry = test_registry(&[("board_info", ToolLlmVisibility::user_and_system())]);
        registry.register(Box::new(StubBoardInfoTool));
        let mut config = test_agent_loop_config();
        config.strategy = AgentRunStrategy::LinuxEnhanced;
        let execution_state_store = Arc::new(StubExecutionStateStore {
            entries: Mutex::new(HashMap::from([(
                "chat-config".to_string(),
                ExecutionState {
                    status: crate::memory::ExecutionStatus::Active,
                    goal: "配置 QQ 邮箱账户".to_string(),
                    next_action: "等待用户补充授权码后继续配置".to_string(),
                    updated_at: 9,
                    ..ExecutionState::default()
                },
            )])),
        });
        config.runtime.execution_state_store =
            Arc::clone(&execution_state_store) as Arc<dyn ExecutionStateStore + Send + Sync>;
        let msg = PcMsg::new_inbound(
            "qq_channel",
            "chat-config",
            "授权码是 hqvqcibpdvqgbdba",
            false,
        )
        .expect("message");
        let mut repeat = HashMap::new();

        let executed = turn_execution::execute_turn(
            &mut http,
            &llm,
            &msg,
            &outbound_tx,
            "req-active-action-supply-probe",
            &registry,
            &config,
            &mut repeat,
            UiLocale::Zh,
        )
        .expect("execute turn");

        let observed = observed.lock().unwrap_or_else(|e| e.into_inner());
        assert_eq!(observed.len(), 2, "{observed:#?}");
        assert!(
            observed
                .iter()
                .all(|request| !request.system.contains("Request Semantics Probe")),
            "{observed:#?}"
        );
        assert!(
            observed.iter().any(|request| request.tool_count == 1),
            "{observed:#?}"
        );
        assert!(
            observed
                .iter()
                .all(|request| !request.system.contains("## Task Execution Planner")),
            "{observed:#?}"
        );
        assert_eq!(
            executed.telemetry.delivery.edit_phase_header_updates_sent,
            0
        );
        assert_eq!(
            executed.telemetry.delivery.edit_planner_header_updates_sent,
            0
        );
        assert_eq!(
            executed.telemetry.delivery.edit_action_header_updates_sent,
            0
        );
        assert_eq!(executed.telemetry.delivery.edit_tool_header_updates_sent, 0);
        assert_eq!(
            executed
                .telemetry
                .delivery
                .edit_terminal_header_updates_sent,
            0
        );
    }

    #[test]
    fn execute_turn_structured_tool_blocker_short_circuits_second_llm_round() {
        let observed = Arc::new(Mutex::new(Vec::new()));
        let llm = ObservedSequenceStubLlm {
            responses: Mutex::new(vec![LlmResponse {
                content: "[tool_use]".to_string(),
                stop_reason: StopReason::ToolUse,
                tool_calls: Some(vec![crate::llm::ToolCall {
                    id: "call_1".to_string(),
                    name: "office_config".to_string(),
                    input: r#"{"op":"apply_account"}"#.to_string(),
                }]),
            }]),
            observed: Arc::clone(&observed),
        };
        let mut http = DummyPlatformHttp;
        let (outbound_tx, _outbound_rx, _) = crate::bus::new_inbound_channel(8);
        let mut registry = test_registry_with_protocols(
            &[("office_config", ToolLlmVisibility::user_only())],
            &[(
                "office_config",
                ToolProtocolContract::operation_envelope_json_with_rich_blockers(),
            )],
        );
        registry.register(Box::new(StubBlockingOfficeConfigTool));
        let mut config = test_agent_loop_config();
        config.strategy = AgentRunStrategy::LinuxEnhanced;
        let msg = PcMsg::new_inbound("qq_channel", "chat-blocker", "帮我配置邮箱", false)
            .expect("message");
        let mut repeat = HashMap::new();

        let executed = turn_execution::execute_turn(
            &mut http,
            &llm,
            &msg,
            &outbound_tx,
            "req-structured-tool-blocker",
            &registry,
            &config,
            &mut repeat,
            UiLocale::Zh,
        )
        .expect("execute turn");

        let observed = observed.lock().unwrap_or_else(|e| e.into_inner());
        assert_eq!(observed.len(), 1, "{observed:#?}");
        let WorkerOutcome::Content(delivered) = executed.outcome;
        assert_eq!(
            delivered,
            "要继续这一步，还需要你告诉我 `identity_class`。可选值：work / personal。"
        );
    }

    #[test]
    fn execute_turn_structured_choice_blocker_renders_programmatic_selection_question() {
        let observed = Arc::new(Mutex::new(Vec::new()));
        let llm = ObservedSequenceStubLlm {
            responses: Mutex::new(vec![LlmResponse {
                content: "[tool_use]".to_string(),
                stop_reason: StopReason::ToolUse,
                tool_calls: Some(vec![crate::llm::ToolCall {
                    id: "call_1".to_string(),
                    name: "mail".to_string(),
                    input: r#"{"op":"list"}"#.to_string(),
                }]),
            }]),
            observed: Arc::clone(&observed),
        };
        let mut http = DummyPlatformHttp;
        let (outbound_tx, _outbound_rx, _) = crate::bus::new_inbound_channel(8);
        let mut registry = test_registry_with_protocols(
            &[("mail", ToolLlmVisibility::user_only())],
            &[(
                "mail",
                ToolProtocolContract::operation_envelope_json_with_rich_blockers(),
            )],
        );
        registry.register(Box::new(StubChoiceBlockingTool));
        let mut config = test_agent_loop_config();
        config.strategy = AgentRunStrategy::LinuxEnhanced;
        let msg = PcMsg::new_inbound("qq_channel", "chat-choice-blocker", "帮我查邮件", false)
            .expect("message");
        let mut repeat = HashMap::new();

        let executed = turn_execution::execute_turn(
            &mut http,
            &llm,
            &msg,
            &outbound_tx,
            "req-structured-choice-blocker",
            &registry,
            &config,
            &mut repeat,
            UiLocale::Zh,
        )
        .expect("execute turn");

        let observed = observed.lock().unwrap_or_else(|e| e.into_inner());
        assert_eq!(observed.len(), 1, "{observed:#?}");
        let WorkerOutcome::Content(delivered) = executed.outcome;
        assert_eq!(
            delivered,
            "要继续这一步，还需要你选择 `account_key`。可选值：Work mail (`mail-work`) / Personal mail (`mail-personal`)。"
        );
    }

    #[test]
    fn execute_turn_structured_multi_field_blocker_renders_all_missing_fields_programmatically() {
        let observed = Arc::new(Mutex::new(Vec::new()));
        let llm = ObservedSequenceStubLlm {
            responses: Mutex::new(vec![LlmResponse {
                content: "[tool_use]".to_string(),
                stop_reason: StopReason::ToolUse,
                tool_calls: Some(vec![crate::llm::ToolCall {
                    id: "call_1".to_string(),
                    name: "office_config".to_string(),
                    input: r#"{"op":"apply_account"}"#.to_string(),
                }]),
            }]),
            observed: Arc::clone(&observed),
        };
        let mut http = DummyPlatformHttp;
        let (outbound_tx, _outbound_rx, _) = crate::bus::new_inbound_channel(8);
        let mut registry = test_registry_with_protocols(
            &[("office_config", ToolLlmVisibility::user_only())],
            &[(
                "office_config",
                ToolProtocolContract::operation_envelope_json_with_rich_blockers(),
            )],
        );
        registry.register(Box::new(StubMultiFieldBlockingTool));
        let mut config = test_agent_loop_config();
        config.strategy = AgentRunStrategy::LinuxEnhanced;
        let msg = PcMsg::new_inbound(
            "qq_channel",
            "chat-multi-field-blocker",
            "帮我配置邮箱",
            false,
        )
        .expect("message");
        let mut repeat = HashMap::new();

        let executed = turn_execution::execute_turn(
            &mut http,
            &llm,
            &msg,
            &outbound_tx,
            "req-structured-multi-field-blocker",
            &registry,
            &config,
            &mut repeat,
            UiLocale::Zh,
        )
        .expect("execute turn");

        let observed = observed.lock().unwrap_or_else(|e| e.into_inner());
        assert_eq!(observed.len(), 1, "{observed:#?}");
        let WorkerOutcome::Content(delivered) = executed.outcome;
        assert_eq!(
            delivered,
            "要继续这一步，还需要你补充这些信息：1. `identity_class`（可选值：work / personal）；2. `mail_imap_host`。"
        );
    }

    #[test]
    fn execute_turn_unavailable_tool_uses_shared_programmatic_unsupported_blocker() {
        let observed = Arc::new(Mutex::new(Vec::new()));
        let llm = ObservedSequenceStubLlm {
            responses: Mutex::new(vec![LlmResponse {
                content: "[tool_use]".to_string(),
                stop_reason: StopReason::ToolUse,
                tool_calls: Some(vec![crate::llm::ToolCall {
                    id: "call_1".to_string(),
                    name: "ghost_tool".to_string(),
                    input: "{}".to_string(),
                }]),
            }]),
            observed: Arc::clone(&observed),
        };
        let mut http = DummyPlatformHttp;
        let (outbound_tx, _outbound_rx, _) = crate::bus::new_inbound_channel(8);
        let registry = crate::tools::ToolRegistry::new();
        let mut config = test_agent_loop_config();
        config.strategy = AgentRunStrategy::LinuxEnhanced;
        let msg = PcMsg::new_inbound(
            "qq_channel",
            "chat-unsupported-blocker",
            "继续处理这个任务",
            false,
        )
        .expect("message");
        let mut repeat = HashMap::new();

        let executed = turn_execution::execute_turn(
            &mut http,
            &llm,
            &msg,
            &outbound_tx,
            "req-unsupported-blocker",
            &registry,
            &config,
            &mut repeat,
            UiLocale::Zh,
        )
        .expect("execute turn");

        let observed = observed.lock().unwrap_or_else(|e| e.into_inner());
        assert_eq!(observed.len(), 1, "{observed:#?}");
        let WorkerOutcome::Content(delivered) = executed.outcome;
        assert_eq!(
            delivered,
            "这一步当前不可用：tool `ghost_tool`; reason `not_available_in_current_runtime`。"
        );
    }

    #[test]
    fn execute_turn_runtime_capability_blocked_tool_uses_shared_programmatic_runtime_blocker() {
        with_runtime_capabilities_restored(|| {
            crate::orchestrator::update_runtime_capability(
                crate::orchestrator::RuntimeCapabilityUpdate {
                    id: crate::orchestrator::RUNTIME_CAPABILITY_NETWORK_OUTBOUND_HTTP,
                    status: crate::orchestrator::RuntimeCapabilityStatus::Offline,
                    reason: crate::orchestrator::RuntimeCapabilityReason::UpstreamUnavailable,
                    observed_at_secs: 42,
                    recovery_hint: None,
                },
            );

            let observed = Arc::new(Mutex::new(Vec::new()));
            let llm = ObservedSequenceStubLlm {
                responses: Mutex::new(vec![LlmResponse {
                    content: "[tool_use]".to_string(),
                    stop_reason: StopReason::ToolUse,
                    tool_calls: Some(vec![crate::llm::ToolCall {
                        id: "call_1".to_string(),
                        name: "network_probe".to_string(),
                        input: "{}".to_string(),
                    }]),
                }]),
                observed: Arc::clone(&observed),
            };
            let mut http = DummyPlatformHttp;
            let (outbound_tx, _outbound_rx, _) = crate::bus::new_inbound_channel(8);
            let mut registry =
                test_registry(&[("network_probe", ToolLlmVisibility::user_and_system())]);
            registry.register(Box::new(StubCapabilityBoundTool));
            let mut config = test_agent_loop_config();
            config.strategy = AgentRunStrategy::LinuxEnhanced;
            let msg = PcMsg::new_inbound(
                "qq_channel",
                "chat-runtime-blocked-tool",
                "继续处理这个任务",
                false,
            )
            .expect("message");
            let mut repeat = HashMap::new();

            let executed = turn_execution::execute_turn(
                &mut http,
                &llm,
                &msg,
                &outbound_tx,
                "req-runtime-blocked-tool",
                &registry,
                &config,
                &mut repeat,
                UiLocale::Zh,
            )
            .expect("execute turn");

            let observed = observed.lock().unwrap_or_else(|e| e.into_inner());
            assert_eq!(observed.len(), 1, "{observed:#?}");
            let WorkerOutcome::Content(delivered) = executed.outcome;
            assert_eq!(
                delivered,
                "这一步当前被运行时条件阻塞：tool `network_probe`; sub_capability `network.outbound_http`; status `offline`; reason `upstream_unavailable`; recovery_hint `wait_for_network_recovery`。"
            );
        });
    }

    #[test]
    fn execute_turn_repeated_tool_protocol_violation_stops_without_extra_llm_round() {
        let observed = Arc::new(Mutex::new(Vec::new()));
        let invalid_call = || LlmResponse {
            content: "[tool_use]".to_string(),
            stop_reason: StopReason::ToolUse,
            tool_calls: Some(vec![crate::llm::ToolCall {
                id: "call_memory".to_string(),
                name: "memory_search".to_string(),
                input: "{query: conversation history, limit: }".to_string(),
            }]),
        };
        let llm = ObservedSequenceStubLlm {
            responses: Mutex::new(vec![
                invalid_call(),
                invalid_call(),
                LlmResponse {
                    content: "should not need a third LLM round".to_string(),
                    stop_reason: StopReason::EndTurn,
                    tool_calls: None,
                },
            ]),
            observed: Arc::clone(&observed),
        };
        let mut http = DummyPlatformHttp;
        let (outbound_tx, _outbound_rx, _) = crate::bus::new_inbound_channel(8);
        let mut registry = test_registry(&[("memory_search", ToolLlmVisibility::user_only())]);
        registry.register(Box::new(StubMemorySearchTool));
        let mut config = test_agent_loop_config();
        config.strategy = AgentRunStrategy::Embedded;
        let msg =
            PcMsg::new_inbound("qq_channel", "chat-protocol-loop", "继续", false).expect("message");
        let mut repeat = HashMap::new();

        let executed = turn_execution::execute_turn(
            &mut http,
            &llm,
            &msg,
            &outbound_tx,
            "req-repeated-protocol-violation",
            &registry,
            &config,
            &mut repeat,
            UiLocale::Zh,
        )
        .expect("execute turn");

        let observed = observed.lock().unwrap_or_else(|e| e.into_inner());
        assert_eq!(
            observed.len(),
            2,
            "second protocol violation must terminate locally instead of asking the LLM again"
        );
        let WorkerOutcome::Content(delivered) = executed.outcome;
        assert_eq!(delivered, tr(UiMessage::OperationFailed, UiLocale::Zh));
        assert_eq!(executed.telemetry.latency.react_rounds, 2);
        assert_eq!(executed.telemetry.latency.tool_calls, 2);
    }

    #[test]
    fn execute_turn_task_like_chinese_free_text_protocol_violation_never_executes_tool() {
        let observed = Arc::new(Mutex::new(Vec::new()));
        let captured_args = Arc::new(Mutex::new(Vec::new()));
        let invalid_call = || {
            LlmResponse {
            content: "[tool_use]".to_string(),
            stop_reason: StopReason::ToolUse,
            tool_calls: Some(vec![crate::llm::ToolCall {
                id: "call_task_like".to_string(),
                name: "argument_capture".to_string(),
                input: "{op: create, title: 备忘：在测试 ESP32-S3（esp32sp）, detail: 用户当前正在测试 ESP32-S3 开发板/固件，代号 esp32sp。设备运行正常，WiFi 已连接，资源充裕。, priority: low}".to_string(),
            }]),
        }
        };
        let llm = ObservedSequenceStubLlm {
            responses: Mutex::new(vec![
                invalid_call(),
                invalid_call(),
                LlmResponse {
                    content: "should not need a third LLM round".to_string(),
                    stop_reason: StopReason::EndTurn,
                    tool_calls: None,
                },
            ]),
            observed: Arc::clone(&observed),
        };
        let mut http = DummyPlatformHttp;
        let (outbound_tx, _outbound_rx, _) = crate::bus::new_inbound_channel(8);
        let mut registry = test_registry_with_protocols(
            &[("argument_capture", ToolLlmVisibility::user_only())],
            &[(
                "argument_capture",
                ToolProtocolContract::operation_envelope_json_with_rich_blockers(),
            )],
        );
        registry.register(Box::new(StubArgumentCaptureTool {
            observed_args: Arc::clone(&captured_args),
        }));
        let mut config = test_agent_loop_config();
        config.strategy = AgentRunStrategy::Embedded;
        let msg = PcMsg::new_inbound("qq_channel", "chat-task-like-protocol", "记一下", false)
            .expect("message");
        let mut repeat = HashMap::new();

        let executed = turn_execution::execute_turn(
            &mut http,
            &llm,
            &msg,
            &outbound_tx,
            "req-task-like-protocol",
            &registry,
            &config,
            &mut repeat,
            UiLocale::Zh,
        )
        .expect("execute turn");

        let observed = observed.lock().unwrap_or_else(|e| e.into_inner());
        assert_eq!(observed.len(), 2);
        let captured_args = captured_args.lock().unwrap_or_else(|e| e.into_inner());
        assert!(
            captured_args.is_empty(),
            "protocol-invalid arguments must not reach the tool"
        );
        let WorkerOutcome::Content(delivered) = executed.outcome;
        assert_eq!(delivered, tr(UiMessage::OperationFailed, UiLocale::Zh));
        assert_eq!(executed.telemetry.latency.react_rounds, 2);
        assert_eq!(executed.telemetry.latency.tool_calls, 2);
    }

    #[test]
    fn execute_turn_tool_protocol_violation_allows_one_successful_repair() {
        let observed = Arc::new(Mutex::new(Vec::new()));
        let llm = ObservedSequenceStubLlm {
            responses: Mutex::new(vec![
                LlmResponse {
                    content: "[tool_use]".to_string(),
                    stop_reason: StopReason::ToolUse,
                    tool_calls: Some(vec![crate::llm::ToolCall {
                        id: "call_memory_bad".to_string(),
                        name: "memory_search".to_string(),
                        input: "{query: conversation history, limit: }".to_string(),
                    }]),
                },
                LlmResponse {
                    content: "[tool_use]".to_string(),
                    stop_reason: StopReason::ToolUse,
                    tool_calls: Some(vec![crate::llm::ToolCall {
                        id: "call_memory_good".to_string(),
                        name: "memory_search".to_string(),
                        input: r#"{"query":"conversation history","limit":3}"#.to_string(),
                    }]),
                },
                LlmResponse {
                    content: "已完成。".to_string(),
                    stop_reason: StopReason::EndTurn,
                    tool_calls: None,
                },
            ]),
            observed: Arc::clone(&observed),
        };
        let mut http = DummyPlatformHttp;
        let (outbound_tx, _outbound_rx, _) = crate::bus::new_inbound_channel(8);
        let mut registry = test_registry(&[("memory_search", ToolLlmVisibility::user_only())]);
        registry.register(Box::new(StubMemorySearchTool));
        let mut config = test_agent_loop_config();
        config.strategy = AgentRunStrategy::Embedded;
        let msg = PcMsg::new_inbound("qq_channel", "chat-protocol-repair", "继续", false)
            .expect("message");
        let mut repeat = HashMap::new();

        let executed = turn_execution::execute_turn(
            &mut http,
            &llm,
            &msg,
            &outbound_tx,
            "req-protocol-repair",
            &registry,
            &config,
            &mut repeat,
            UiLocale::Zh,
        )
        .expect("execute turn");

        let observed = observed.lock().unwrap_or_else(|e| e.into_inner());
        assert_eq!(observed.len(), 3);
        let WorkerOutcome::Content(delivered) = executed.outcome;
        assert_eq!(delivered, "已完成。");
        assert_eq!(executed.telemetry.latency.react_rounds, 3);
        assert_eq!(executed.telemetry.latency.tool_calls, 2);
    }

    #[test]
    fn execute_turn_normalizes_flat_json_like_tool_args_before_protocol_repair() {
        let observed = Arc::new(Mutex::new(Vec::new()));
        let captured_args = Arc::new(Mutex::new(Vec::new()));
        let llm = ObservedSequenceStubLlm {
            responses: Mutex::new(vec![
                LlmResponse {
                    content: "[tool_use]".to_string(),
                    stop_reason: StopReason::ToolUse,
                    tool_calls: Some(vec![crate::llm::ToolCall {
                        id: "call_capture".to_string(),
                        name: "argument_capture".to_string(),
                        input: "{op: query, kind: fact, topic: 随手记}".to_string(),
                    }]),
                },
                LlmResponse {
                    content: "已处理。".to_string(),
                    stop_reason: StopReason::EndTurn,
                    tool_calls: None,
                },
            ]),
            observed: Arc::clone(&observed),
        };
        let mut http = DummyPlatformHttp;
        let (outbound_tx, _outbound_rx, _) = crate::bus::new_inbound_channel(8);
        let mut registry = test_registry(&[("argument_capture", ToolLlmVisibility::user_only())]);
        registry.register(Box::new(StubArgumentCaptureTool {
            observed_args: Arc::clone(&captured_args),
        }));
        let mut config = test_agent_loop_config();
        config.strategy = AgentRunStrategy::Embedded;
        let msg = PcMsg::new_inbound("qq_channel", "chat-json-like-tool-args", "随手记", false)
            .expect("message");
        let mut repeat = HashMap::new();

        let executed = turn_execution::execute_turn(
            &mut http,
            &llm,
            &msg,
            &outbound_tx,
            "req-json-like-tool-args",
            &registry,
            &config,
            &mut repeat,
            UiLocale::Zh,
        )
        .expect("execute turn");

        let WorkerOutcome::Content(delivered) = executed.outcome;
        assert_eq!(delivered, "已处理。");
        assert_eq!(executed.telemetry.latency.react_rounds, 2);
        assert_eq!(executed.telemetry.latency.tool_calls, 1);
        let captured_args = captured_args.lock().unwrap_or_else(|e| e.into_inner());
        assert_eq!(captured_args.len(), 1, "{captured_args:#?}");
        let normalized: serde_json::Value =
            serde_json::from_str(&captured_args[0]).expect("tool args are strict JSON");
        assert_eq!(
            normalized,
            serde_json::json!({"op":"query","kind":"fact","topic":"随手记"})
        );
    }

    #[test]
    fn execute_turn_group_active_action_tool_round_suppresses_edit_header_visible_text_copy() {
        let observed = Arc::new(Mutex::new(Vec::new()));
        let llm = ObservedSequenceStubLlm {
            responses: Mutex::new(vec![
                LlmResponse {
                    content: "[tool_use]".to_string(),
                    stop_reason: StopReason::ToolUse,
                    tool_calls: Some(vec![crate::llm::ToolCall {
                        id: "call_1".to_string(),
                        name: "board_info".to_string(),
                        input: "{}".to_string(),
                    }]),
                },
                LlmResponse {
                    content: "当前主机 beetle 在线，可继续配置 QQ 邮箱。".to_string(),
                    stop_reason: StopReason::EndTurn,
                    tool_calls: None,
                },
            ]),
            observed: Arc::clone(&observed),
        };
        let mut http = DummyPlatformHttp;
        let (outbound_tx, _outbound_rx, _) = crate::bus::new_inbound_channel(8);
        let mut registry = test_registry(&[("board_info", ToolLlmVisibility::user_and_system())]);
        registry.register(Box::new(StubBoardInfoTool));
        let mut config = test_agent_loop_config();
        config.strategy = AgentRunStrategy::LinuxEnhanced;
        let active_work_store = Arc::new(StubActiveWorkStore {
            entries: Mutex::new(HashMap::from([(
                "chat-group-config".to_string(),
                crate::agent::ActiveWorkRecord {
                    kind: crate::agent::ActiveWorkKind::InteractiveAction,
                    title: "配置 QQ 邮箱账户".to_string(),
                    status: crate::agent::ForegroundWorkStatus::Running,
                    continuity_open: true,
                    blocks_background_llm: true,
                    progress_summary: "账户草案已创建".to_string(),
                    blocker: String::new(),
                    next_action: "等待用户继续配置".to_string(),
                    recent_outcome: String::new(),
                    active_artifact_refs: Vec::new(),
                    updated_at: 9,
                },
            )])),
        });
        config.runtime.active_work_store =
            Arc::clone(&active_work_store) as Arc<dyn crate::agent::ActiveWorkStore + Send + Sync>;
        let msg =
            PcMsg::new_inbound("qq_channel", "chat-group-config", "继续", true).expect("message");
        let mut repeat = HashMap::new();

        let executed = turn_execution::execute_turn(
            &mut http,
            &llm,
            &msg,
            &outbound_tx,
            "req-group-active-action-tool-round",
            &registry,
            &config,
            &mut repeat,
            UiLocale::Zh,
        )
        .expect("execute turn");

        let observed = observed.lock().unwrap_or_else(|e| e.into_inner());
        assert_eq!(observed.len(), 2, "{observed:#?}");
        assert!(
            observed
                .iter()
                .all(|request| !request.system.contains("Request Semantics Probe")),
            "{observed:#?}"
        );
        assert!(
            observed
                .iter()
                .all(|request| !request.system.contains("## Task Execution Planner")),
            "{observed:#?}"
        );
        assert_eq!(
            observed[0].tool_choice,
            ToolChoicePolicy::Require,
            "{observed:#?}"
        );
        assert_eq!(observed[0].tool_count, 1, "{observed:#?}");
        assert_eq!(
            executed.telemetry.delivery.edit_phase_header_updates_sent,
            0
        );
        assert_eq!(
            executed.telemetry.delivery.edit_planner_header_updates_sent,
            0
        );
        assert_eq!(
            executed.telemetry.delivery.edit_action_header_updates_sent,
            0
        );
        assert_eq!(executed.telemetry.delivery.edit_tool_header_updates_sent, 0);
        assert_eq!(
            executed
                .telemetry
                .delivery
                .edit_terminal_header_updates_sent,
            0
        );
    }

    #[test]
    fn execute_turn_action_request_without_tool_use_avoids_fake_started_and_blocked_edit_header_visibility(
    ) {
        let observed = Arc::new(Mutex::new(Vec::new()));
        let llm = ObservedSequenceStubLlm {
            responses: Mutex::new(vec![LlmResponse {
                content: "请先提供 QQ 邮箱的授权码，我才能继续配置。".to_string(),
                stop_reason: StopReason::EndTurn,
                tool_calls: None,
            }]),
            observed: Arc::clone(&observed),
        };
        let mut http = DummyPlatformHttp;
        let (outbound_tx, _outbound_rx, _) = crate::bus::new_inbound_channel(8);
        let mut registry = test_registry(&[("board_info", ToolLlmVisibility::user_and_system())]);
        registry.register(Box::new(StubBoardInfoTool));
        let mut config = test_agent_loop_config();
        config.strategy = AgentRunStrategy::LinuxEnhanced;
        let msg = PcMsg::new_inbound(
            "qq_channel",
            "chat-action-without-tool-use",
            "帮我配置 QQ 邮箱账户",
            false,
        )
        .expect("message");
        let mut repeat = HashMap::new();

        let executed = turn_execution::execute_turn(
            &mut http,
            &llm,
            &msg,
            &outbound_tx,
            "req-action-without-tool-use",
            &registry,
            &config,
            &mut repeat,
            UiLocale::Zh,
        )
        .expect("execute turn");

        let WorkerOutcome::Content(delivered) = executed.outcome;
        assert_eq!(delivered, "请先提供 QQ 邮箱的授权码，我才能继续配置。");
        assert_eq!(
            executed.telemetry.delivery.edit_phase_header_updates_sent,
            0
        );
        assert_eq!(
            executed.telemetry.delivery.edit_planner_header_updates_sent,
            0
        );
        assert_eq!(
            executed.telemetry.delivery.edit_action_header_updates_sent,
            0
        );
        assert_eq!(executed.telemetry.delivery.edit_tool_header_updates_sent, 0);
        assert_eq!(
            executed
                .telemetry
                .delivery
                .edit_terminal_header_updates_sent,
            0
        );
        let observed = observed.lock().unwrap_or_else(|e| e.into_inner());
        assert_eq!(observed.len(), 1, "{observed:#?}");
    }

    #[test]
    fn execute_turn_action_request_without_tool_use_uses_receipt_only_before_truth_guard() {
        let observed = Arc::new(Mutex::new(Vec::new()));
        let llm = ObservedSequenceStubLlm {
            responses: Mutex::new(vec![LlmResponse {
                content: "让我先检查当前邮件状态，然后继续配置。".to_string(),
                stop_reason: StopReason::EndTurn,
                tool_calls: None,
            }]),
            observed: Arc::clone(&observed),
        };
        let mut http = DummyPlatformHttp;
        let (outbound_tx, _outbound_rx, _) = crate::bus::new_inbound_channel(8);
        let mut registry = test_registry(&[("board_info", ToolLlmVisibility::user_and_system())]);
        registry.register(Box::new(StubBoardInfoTool));
        let mut config = test_agent_loop_config();
        config.strategy = AgentRunStrategy::LinuxEnhanced;
        let msg = PcMsg::new_inbound(
            "qq_channel",
            "chat-action-truth-guard",
            "帮我配置 QQ 邮箱账户",
            false,
        )
        .expect("message");
        let mut repeat = HashMap::new();

        let executed = turn_execution::execute_turn(
            &mut http,
            &llm,
            &msg,
            &outbound_tx,
            "req-action-truth-guard",
            &registry,
            &config,
            &mut repeat,
            UiLocale::Zh,
        )
        .expect("execute turn");

        let finalized = self::reply_finalize::finalize_turn(
            &mut http,
            &SequenceStubLlm {
                responses: Mutex::new(Vec::new()),
            },
            &config,
            &msg,
            UiLocale::Zh,
            Instant::now(),
            executed.outcome,
            executed.telemetry,
        )
        .expect("finalize turn");

        let delivered = finalized.reply.visible_text;
        assert_eq!(
            delivered,
            "这轮还没有实际执行新的工具或任务步骤，也还没有产生新结果。"
        );
        assert_eq!(finalized.delivery.edit_phase_header_updates_sent, 0);
        assert_eq!(finalized.delivery.edit_planner_header_updates_sent, 0);
        assert_eq!(finalized.delivery.edit_action_header_updates_sent, 0);
        assert_eq!(finalized.delivery.edit_tool_header_updates_sent, 0);
        assert_eq!(finalized.delivery.edit_terminal_header_updates_sent, 0);
        let observed = observed.lock().unwrap_or_else(|e| e.into_inner());
        assert_eq!(observed.len(), 1, "{observed:#?}");
    }

    #[test]
    fn execute_turn_active_action_without_tool_use_keeps_blocker_truth_without_fake_edit_header_visibility(
    ) {
        let observed = Arc::new(Mutex::new(Vec::new()));
        let llm = ObservedSequenceStubLlm {
            responses: Mutex::new(vec![LlmResponse {
                content: "请把 SMTP 授权码也发我，我才能继续配置。".to_string(),
                stop_reason: StopReason::EndTurn,
                tool_calls: None,
            }]),
            observed: Arc::clone(&observed),
        };
        let mut http = DummyPlatformHttp;
        let (outbound_tx, _outbound_rx, _) = crate::bus::new_inbound_channel(8);
        let mut registry = test_registry(&[("board_info", ToolLlmVisibility::user_and_system())]);
        registry.register(Box::new(StubBoardInfoTool));
        let mut config = test_agent_loop_config();
        config.strategy = AgentRunStrategy::LinuxEnhanced;
        let active_work_store = Arc::new(StubActiveWorkStore {
            entries: Mutex::new(HashMap::from([(
                "chat-active-action-without-tool-use".to_string(),
                crate::agent::ActiveWorkRecord {
                    kind: crate::agent::ActiveWorkKind::InteractiveAction,
                    title: "配置 QQ 邮箱账户".to_string(),
                    status: crate::agent::ForegroundWorkStatus::AwaitingUser,
                    continuity_open: true,
                    blocks_background_llm: true,
                    progress_summary: "账户草案已创建".to_string(),
                    blocker: "缺少 SMTP 授权码".to_string(),
                    next_action: "等待用户补充 SMTP 授权码".to_string(),
                    recent_outcome: String::new(),
                    active_artifact_refs: Vec::new(),
                    updated_at: 9,
                },
            )])),
        });
        config.runtime.active_work_store =
            Arc::clone(&active_work_store) as Arc<dyn crate::agent::ActiveWorkStore + Send + Sync>;
        let msg = PcMsg::new_inbound(
            "qq_channel",
            "chat-active-action-without-tool-use",
            "SMTP 授权码还需要吗",
            false,
        )
        .expect("message");
        let mut repeat = HashMap::new();

        let executed = turn_execution::execute_turn(
            &mut http,
            &llm,
            &msg,
            &outbound_tx,
            "req-active-action-without-tool-use",
            &registry,
            &config,
            &mut repeat,
            UiLocale::Zh,
        )
        .expect("execute turn");

        let WorkerOutcome::Content(delivered) = executed.outcome;
        assert_eq!(delivered, "请把 SMTP 授权码也发我，我才能继续配置。");
        assert_eq!(
            executed.telemetry.delivery.edit_phase_header_updates_sent,
            0
        );
        assert_eq!(
            executed.telemetry.delivery.edit_planner_header_updates_sent,
            0
        );
        assert_eq!(
            executed.telemetry.delivery.edit_action_header_updates_sent,
            0
        );
        assert_eq!(executed.telemetry.delivery.edit_tool_header_updates_sent, 0);
        assert_eq!(
            executed
                .telemetry
                .delivery
                .edit_terminal_header_updates_sent,
            0
        );
        let observed = observed.lock().unwrap_or_else(|e| e.into_inner());
        assert_eq!(observed.len(), 1, "{observed:#?}");
    }

    #[test]
    fn execute_turn_switch_request_turn_does_not_run_request_semantics_probe() {
        let observed = Arc::new(Mutex::new(Vec::new()));
        let llm = ObservedSequenceStubLlm {
            responses: Mutex::new(vec![
                LlmResponse {
                    content: "[tool_use]".to_string(),
                    stop_reason: StopReason::ToolUse,
                    tool_calls: Some(vec![crate::llm::ToolCall {
                        id: "call_1".to_string(),
                        name: "board_info".to_string(),
                        input: "{}".to_string(),
                    }]),
                },
                LlmResponse {
                    content: "开始切换并配置 Telegram。".to_string(),
                    stop_reason: StopReason::EndTurn,
                    tool_calls: None,
                },
            ]),
            observed: Arc::clone(&observed),
        };
        let mut http = DummyPlatformHttp;
        let (outbound_tx, _outbound_rx, _) = crate::bus::new_inbound_channel(8);
        let mut registry = test_registry(&[("board_info", ToolLlmVisibility::user_and_system())]);
        registry.register(Box::new(StubBoardInfoTool));
        let mut config = test_agent_loop_config();
        config.strategy = AgentRunStrategy::LinuxEnhanced;
        let execution_state_store = Arc::new(StubExecutionStateStore {
            entries: Mutex::new(HashMap::from([(
                "chat-switch".to_string(),
                ExecutionState {
                    status: crate::memory::ExecutionStatus::Active,
                    goal: "配置 QQ 邮箱账户".to_string(),
                    next_action: "等待用户继续补充邮箱配置参数".to_string(),
                    updated_at: 9,
                    ..ExecutionState::default()
                },
            )])),
        });
        config.runtime.execution_state_store =
            Arc::clone(&execution_state_store) as Arc<dyn ExecutionStateStore + Send + Sync>;
        let msg = PcMsg::new_inbound(
            "qq_channel",
            "chat-switch",
            "别配邮箱了，改成配置 Telegram",
            false,
        )
        .expect("message");
        let mut repeat = HashMap::new();

        let executed = turn_execution::execute_turn(
            &mut http,
            &llm,
            &msg,
            &outbound_tx,
            "req-active-action-switch-probe",
            &registry,
            &config,
            &mut repeat,
            UiLocale::Zh,
        )
        .expect("execute turn");

        let observed = observed.lock().unwrap_or_else(|e| e.into_inner());
        assert_eq!(
            executed.telemetry.delivery.edit_phase_header_updates_sent,
            0
        );
        assert_eq!(
            executed.telemetry.delivery.edit_planner_header_updates_sent,
            0
        );
        assert_eq!(
            executed.telemetry.delivery.edit_action_header_updates_sent,
            0
        );
        assert_eq!(executed.telemetry.delivery.edit_tool_header_updates_sent, 0);
        assert_eq!(
            executed
                .telemetry
                .delivery
                .edit_terminal_header_updates_sent,
            0
        );
        assert_eq!(observed.len(), 2, "{observed:#?}");
        assert!(
            observed
                .iter()
                .all(|request| !request.system.contains("Request Semantics Probe")),
            "{observed:#?}"
        );
        assert!(
            observed
                .iter()
                .all(|request| !request.system.contains("## Task Execution Planner")),
            "{observed:#?}"
        );
        assert!(
            observed.iter().any(|request| request.tool_count == 1),
            "{observed:#?}"
        );
    }

    #[test]
    fn complete_turn_seeds_execution_state_for_tool_backed_user_turn() {
        let execution_state_store = Arc::new(StubExecutionStateStore::default());
        let mut config = test_agent_loop_config();
        config.runtime.execution_state_store =
            Arc::clone(&execution_state_store) as Arc<dyn ExecutionStateStore + Send + Sync>;
        let (system_inbound_tx, _system_inbound_rx, _) = crate::bus::new_system_inbound_channel(8);
        let (_outbound_tx, _outbound_rx, _) = crate::bus::new_inbound_channel(8);
        let mut msg = PcMsg::new_inbound(
            "qq_channel",
            "chat-execution-seed",
            "帮我配置 QQ 邮箱账户",
            false,
        )
        .expect("message");
        msg.req_id = Some("req-seed-execution-state".to_string());
        let turn_ledger = build_turn_ledger_start(&msg, 1);
        let finalized = reply_finalize::FinalizedTurn {
            delivery: DeliveryReport::default(),
            reply: crate::agent::final_reply::CanonicalReply::new(
                "我先检查当前邮件状态，然后继续配置。".to_string(),
            ),
            artifact_bundle: None,
            is_interrupt: false,
            reply_already_delivered: false,
            skip_delivery: false,
            mark_important: false,
            streamed: false,
            msg_start: Instant::now(),
            turn_observation: Some(TurnObservationLedger {
                execution_class: TurnExecutionClass::ToolAssisted,
                deliberation_class: crate::memory::TurnDeliberationClass::Standard,
                final_outcome: "final_answer".to_string(),
                pressure: crate::memory::TurnPersonaPressureLevel::Normal,
                mode: TurnModeSnapshotLedger {
                    current_mode: "normal".to_string(),
                    allow_non_voice_outbound: true,
                    allow_idle_self_runtime: true,
                },
                tool_path: TurnToolPathLedger {
                    path: "tool_round".to_string(),
                    tool_calls: 1,
                    react_rounds: 1,
                    current_primary_delivered: false,
                },
                blocker: None,
            }),
            mental_privacy_review: MentalPrivacyReviewOutcome {
                reply_content: "我先检查当前邮件状态，然后继续配置。".to_string(),
                action: crate::memory::MentalPrivacyShareAction::AllowOriginal,
                applied: false,
                touched_targets: Vec::new(),
            },
            review_input_before: "我先检查当前邮件状态，然后继续配置。".to_string(),
            worker_latency: WorkerLatency {
                tool_calls: 1,
                ..WorkerLatency::default()
            },
            any_tool_used: true,
            external_content_used: false,
            pressure: crate::orchestrator::PressureLevel::Normal,
            reply_surface: ReplySurface::GovernedConversation,
            prompt_recall_intent: crate::memory::PromptRecallIntent::Mixed,
            runtime_skill_selected_ids: Vec::new(),
            task_learning_selected_ids: Vec::new(),
            programmable_reasoning_intent: None,
            counterfactual_analysis: None,
            adversarial_arena_adjudication: None,
            subject_state: None,
            soul_feedback_projection: None,
            mental_privacy_adjudication: None,
            persona_priority_adjudication: None,
        };

        reply_finalize::complete_turn(
            LaneTurnFinalizeContext {
                worker_lane_tag: "test",
                config: &config,
                system_inbound_tx: &system_inbound_tx,
                msg: Box::new(msg.clone()),
                msg_start: Instant::now(),
                queue_wait_ms: 0,
                admission_ms: 0,
                worker_prepare_ms: 0,
                msg_key: 1,
                turn_ledger: Box::new(turn_ledger),
                latency_warn_ms: u128::MAX,
            },
            &mut HashMap::new(),
            &mut HashMap::new(),
            finalized,
            delivery_handoff::DeliveryHandoff {
                delivered: true,
                outbound_enqueue_ms: 0,
                reply_handoff_ms: 1,
            },
        );
        let stored = execution_state_store
            .get(msg.chat_id.as_ref())
            .expect("execution state get")
            .expect("seeded execution state");
        assert_eq!(stored.goal, "帮我配置 QQ 邮箱账户");
        assert_eq!(
            stored.next_action,
            "deliver current primary answer before more tool work"
        );
        assert!(config
            .runtime
            .active_work_store
            .get(msg.chat_id.as_ref())
            .expect("active work get")
            .is_none());
    }

    #[test]
    fn complete_turn_seeds_execution_state_for_tool_free_blocker_reply() {
        let execution_state_store = Arc::new(StubExecutionStateStore::default());
        let mut config = test_agent_loop_config();
        config.runtime.execution_state_store =
            Arc::clone(&execution_state_store) as Arc<dyn ExecutionStateStore + Send + Sync>;
        let (system_inbound_tx, _system_inbound_rx, _) = crate::bus::new_system_inbound_channel(8);
        let (_outbound_tx, _outbound_rx, _) = crate::bus::new_inbound_channel(8);
        let mut msg = PcMsg::new_inbound(
            "qq_channel",
            "chat-tool-free-blocker",
            "帮我配置 QQ 邮箱账户",
            false,
        )
        .expect("message");
        msg.req_id = Some("req-tool-free-blocker".to_string());
        let turn_ledger = build_turn_ledger_start(&msg, 1);
        let blocker = "请先提供 QQ 邮箱的授权码，我才能继续配置。".to_string();
        let finalized = reply_finalize::FinalizedTurn {
            delivery: DeliveryReport::default(),
            reply: crate::agent::final_reply::CanonicalReply::new(blocker.clone()),
            artifact_bundle: None,
            is_interrupt: false,
            reply_already_delivered: false,
            skip_delivery: false,
            mark_important: false,
            streamed: false,
            msg_start: Instant::now(),
            turn_observation: Some(TurnObservationLedger {
                execution_class: TurnExecutionClass::DirectReply,
                deliberation_class: TurnDeliberationClass::Standard,
                final_outcome: "final_answer".to_string(),
                pressure: TurnPersonaPressureLevel::Normal,
                mode: TurnModeSnapshotLedger {
                    current_mode: "normal".to_string(),
                    allow_non_voice_outbound: true,
                    allow_idle_self_runtime: true,
                },
                tool_path: TurnToolPathLedger {
                    path: String::new(),
                    tool_calls: 0,
                    react_rounds: 1,
                    current_primary_delivered: false,
                },
                blocker: Some(TurnBlockerLedger {
                    kind: "needs_user_facts".to_string(),
                    failed_calls: 0,
                    total_calls: 0,
                }),
            }),
            mental_privacy_review: MentalPrivacyReviewOutcome {
                reply_content: blocker.clone(),
                action: crate::memory::MentalPrivacyShareAction::AllowOriginal,
                applied: false,
                touched_targets: Vec::new(),
            },
            review_input_before: blocker.clone(),
            worker_latency: WorkerLatency::default(),
            any_tool_used: false,
            external_content_used: false,
            pressure: crate::orchestrator::PressureLevel::Normal,
            reply_surface: ReplySurface::GovernedConversation,
            prompt_recall_intent: crate::memory::PromptRecallIntent::Mixed,
            runtime_skill_selected_ids: Vec::new(),
            task_learning_selected_ids: Vec::new(),
            programmable_reasoning_intent: None,
            counterfactual_analysis: None,
            adversarial_arena_adjudication: None,
            subject_state: None,
            soul_feedback_projection: None,
            mental_privacy_adjudication: None,
            persona_priority_adjudication: None,
        };

        reply_finalize::complete_turn(
            LaneTurnFinalizeContext {
                worker_lane_tag: "test",
                config: &config,
                system_inbound_tx: &system_inbound_tx,
                msg: Box::new(msg.clone()),
                msg_start: Instant::now(),
                queue_wait_ms: 0,
                admission_ms: 0,
                worker_prepare_ms: 0,
                msg_key: 1,
                turn_ledger: Box::new(turn_ledger),
                latency_warn_ms: u128::MAX,
            },
            &mut HashMap::new(),
            &mut HashMap::new(),
            finalized,
            delivery_handoff::DeliveryHandoff {
                delivered: true,
                outbound_enqueue_ms: 0,
                reply_handoff_ms: 1,
            },
        );
        let stored = execution_state_store
            .get(msg.chat_id.as_ref())
            .expect("execution state get")
            .expect("seeded execution state");
        assert_eq!(stored.goal, "帮我配置 QQ 邮箱账户");
        assert_eq!(stored.blocker, blocker);
        assert_eq!(
            stored.next_action,
            "请先提供 QQ 邮箱的授权码，我才能继续配置。"
        );
    }

    #[test]
    fn complete_turn_seeds_execution_state_for_structured_tool_blocker_without_wording_cues() {
        let execution_state_store = Arc::new(StubExecutionStateStore::default());
        let mut config = test_agent_loop_config();
        config.runtime.execution_state_store =
            Arc::clone(&execution_state_store) as Arc<dyn ExecutionStateStore + Send + Sync>;
        let (system_inbound_tx, _system_inbound_rx, _) = crate::bus::new_system_inbound_channel(8);
        let (_outbound_tx, _outbound_rx, _) = crate::bus::new_inbound_channel(8);
        let mut msg = PcMsg::new_inbound(
            "qq_channel",
            "chat-structured-tool-blocker",
            "帮我配置 QQ 邮箱账户",
            false,
        )
        .expect("message");
        msg.req_id = Some("req-structured-tool-blocker-state".to_string());
        let turn_ledger = build_turn_ledger_start(&msg, 1);
        let blocker_summary = "账户配置被阻塞：identity_class".to_string();
        let finalized = reply_finalize::FinalizedTurn {
            delivery: DeliveryReport::default(),
            reply: crate::agent::final_reply::CanonicalReply::new(blocker_summary.clone()),
            artifact_bundle: None,
            is_interrupt: false,
            reply_already_delivered: false,
            skip_delivery: false,
            mark_important: false,
            streamed: false,
            msg_start: Instant::now(),
            turn_observation: Some(TurnObservationLedger {
                execution_class: TurnExecutionClass::ToolAssisted,
                deliberation_class: TurnDeliberationClass::Standard,
                final_outcome: "tool_blocker".to_string(),
                pressure: TurnPersonaPressureLevel::Normal,
                mode: TurnModeSnapshotLedger {
                    current_mode: "normal".to_string(),
                    allow_non_voice_outbound: true,
                    allow_idle_self_runtime: true,
                },
                tool_path: TurnToolPathLedger {
                    path: "tool_blocker".to_string(),
                    tool_calls: 1,
                    react_rounds: 1,
                    current_primary_delivered: false,
                },
                blocker: Some(TurnBlockerLedger {
                    kind: "needs_user_facts".to_string(),
                    failed_calls: 1,
                    total_calls: 1,
                }),
            }),
            mental_privacy_review: MentalPrivacyReviewOutcome {
                reply_content: blocker_summary.clone(),
                action: crate::memory::MentalPrivacyShareAction::AllowOriginal,
                applied: false,
                touched_targets: Vec::new(),
            },
            review_input_before: blocker_summary.clone(),
            worker_latency: WorkerLatency {
                tool_calls: 1,
                ..WorkerLatency::default()
            },
            any_tool_used: true,
            external_content_used: false,
            pressure: crate::orchestrator::PressureLevel::Normal,
            reply_surface: ReplySurface::GovernedConversation,
            prompt_recall_intent: crate::memory::PromptRecallIntent::Mixed,
            runtime_skill_selected_ids: Vec::new(),
            task_learning_selected_ids: Vec::new(),
            programmable_reasoning_intent: None,
            counterfactual_analysis: None,
            adversarial_arena_adjudication: None,
            subject_state: None,
            soul_feedback_projection: None,
            mental_privacy_adjudication: None,
            persona_priority_adjudication: None,
        };

        reply_finalize::complete_turn(
            LaneTurnFinalizeContext {
                worker_lane_tag: "test",
                config: &config,
                system_inbound_tx: &system_inbound_tx,
                msg: Box::new(msg.clone()),
                msg_start: Instant::now(),
                queue_wait_ms: 0,
                admission_ms: 0,
                worker_prepare_ms: 0,
                msg_key: 1,
                turn_ledger: Box::new(turn_ledger),
                latency_warn_ms: u128::MAX,
            },
            &mut HashMap::new(),
            &mut HashMap::new(),
            finalized,
            delivery_handoff::DeliveryHandoff {
                delivered: true,
                outbound_enqueue_ms: 0,
                reply_handoff_ms: 1,
            },
        );

        let stored = execution_state_store
            .get(msg.chat_id.as_ref())
            .expect("execution state get")
            .expect("seeded execution state");
        assert_eq!(stored.goal, "帮我配置 QQ 邮箱账户");
        assert_eq!(stored.blocker, blocker_summary);
        assert_eq!(stored.next_action, "账户配置被阻塞：identity_class");
    }

    #[test]
    fn complete_turn_clears_execution_state_for_cancel_active_action_turn() {
        let execution_state_store = Arc::new(StubExecutionStateStore {
            entries: Mutex::new(HashMap::from([(
                "chat-cancel-execution-state".to_string(),
                ExecutionState {
                    status: crate::memory::ExecutionStatus::Active,
                    goal: "配置 QQ 邮箱账户".to_string(),
                    next_action: "等待用户补充邮箱参数".to_string(),
                    updated_at: 9,
                    ..ExecutionState::default()
                },
            )])),
        });
        let mut config = test_agent_loop_config();
        config.runtime.execution_state_store =
            Arc::clone(&execution_state_store) as Arc<dyn ExecutionStateStore + Send + Sync>;
        let (system_inbound_tx, _system_inbound_rx, _) = crate::bus::new_system_inbound_channel(8);
        let (_outbound_tx, _outbound_rx, _) = crate::bus::new_inbound_channel(8);
        let mut msg = PcMsg::new_inbound(
            "qq_channel",
            "chat-cancel-execution-state",
            "先别配了，这个动作取消",
            false,
        )
        .expect("message");
        msg.req_id = Some("req-clear-execution-state".to_string());
        let turn_ledger = build_turn_ledger_start(&msg, 1);
        let finalized = reply_finalize::FinalizedTurn {
            delivery: DeliveryReport::default(),
            reply: crate::agent::final_reply::CanonicalReply::new(
                "好，当前配置动作先取消。".to_string(),
            ),
            artifact_bundle: None,
            is_interrupt: false,
            reply_already_delivered: false,
            skip_delivery: false,
            mark_important: false,
            streamed: false,
            msg_start: Instant::now(),
            turn_observation: None,
            mental_privacy_review: MentalPrivacyReviewOutcome {
                reply_content: "好，当前配置动作先取消。".to_string(),
                action: crate::memory::MentalPrivacyShareAction::AllowOriginal,
                applied: false,
                touched_targets: Vec::new(),
            },
            review_input_before: "好，当前配置动作先取消。".to_string(),
            worker_latency: WorkerLatency::default(),
            any_tool_used: false,
            external_content_used: false,
            pressure: crate::orchestrator::PressureLevel::Normal,
            reply_surface: ReplySurface::GovernedConversation,
            prompt_recall_intent: crate::memory::PromptRecallIntent::Mixed,
            runtime_skill_selected_ids: Vec::new(),
            task_learning_selected_ids: Vec::new(),
            programmable_reasoning_intent: None,
            counterfactual_analysis: None,
            adversarial_arena_adjudication: None,
            subject_state: None,
            soul_feedback_projection: None,
            mental_privacy_adjudication: None,
            persona_priority_adjudication: None,
        };

        reply_finalize::complete_turn(
            LaneTurnFinalizeContext {
                worker_lane_tag: "test",
                config: &config,
                system_inbound_tx: &system_inbound_tx,
                msg: Box::new(msg.clone()),
                msg_start: Instant::now(),
                queue_wait_ms: 0,
                admission_ms: 0,
                worker_prepare_ms: 0,
                msg_key: 1,
                turn_ledger: Box::new(turn_ledger),
                latency_warn_ms: u128::MAX,
            },
            &mut HashMap::new(),
            &mut HashMap::new(),
            finalized,
            delivery_handoff::DeliveryHandoff {
                delivered: true,
                outbound_enqueue_ms: 0,
                reply_handoff_ms: 1,
            },
        );

        assert!(execution_state_store
            .get(msg.chat_id.as_ref())
            .expect("execution state get")
            .is_none());
    }

    #[test]
    fn office_account_confirmation_turn_resumes_action_and_calls_tool_with_selected_account() {
        let session_store = Arc::new(StubSessionStore::default());
        let execution_state_store = Arc::new(StubExecutionStateStore::default());
        let task_run_store = Arc::new(StubTaskRunStore::default());
        let seen_args = Arc::new(Mutex::new(Vec::new()));
        let mut config = test_agent_loop_config();
        config.strategy = AgentRunStrategy::LinuxEnhanced;
        config.runtime.session_store =
            Arc::clone(&session_store) as Arc<dyn SessionStore + Send + Sync>;
        config.runtime.execution_state_store =
            Arc::clone(&execution_state_store) as Arc<dyn ExecutionStateStore + Send + Sync>;
        config.runtime.task_run_store = Arc::clone(&task_run_store)
            as Arc<dyn crate::task_execution::TaskRunStore + Send + Sync>;
        let (system_inbound_tx, _system_inbound_rx, _) = crate::bus::new_system_inbound_channel(8);
        let (outbound_tx, _outbound_rx, _) = crate::bus::new_inbound_channel(8);
        let mut registry = test_registry_with_protocols(
            &[("mail", ToolLlmVisibility::user_only())],
            &[(
                "mail",
                ToolProtocolContract::operation_envelope_json_with_rich_blockers(),
            )],
        );
        registry.register(Box::new(StubResolvableOfficeMailTool {
            seen_args: Arc::clone(&seen_args),
        }));

        let first_turn_llm = SequenceStubLlm {
            responses: Mutex::new(vec![
                LlmResponse {
                    content: "[tool_use]".to_string(),
                    stop_reason: StopReason::ToolUse,
                    tool_calls: Some(vec![crate::llm::ToolCall {
                        id: "call_1".to_string(),
                        name: "mail".to_string(),
                        input: r#"{"op":"list","provider":"imap_smtp"}"#.to_string(),
                    }]),
                },
                LlmResponse {
                    content: "你要用 Work（mail-work）还是 Personal（mail-personal）这个邮箱账户？"
                        .to_string(),
                    stop_reason: StopReason::EndTurn,
                    tool_calls: None,
                },
            ]),
        };
        let mut http = DummyPlatformHttp;
        let mut msg1 =
            PcMsg::new_inbound("qq_channel", "chat-office-resume", "帮我看看邮箱", false)
                .expect("message");
        let mut repeat = HashMap::new();
        let turn_execution::ExecutedTurn {
            outcome: first_outcome,
            telemetry: first_telemetry,
        } = turn_execution::execute_turn(
            &mut http,
            &first_turn_llm,
            &msg1,
            &outbound_tx,
            "req-office-resume-1",
            &registry,
            &config,
            &mut repeat,
            UiLocale::Zh,
        )
        .expect("first execute turn");
        let first_finalized = self::reply_finalize::finalize_turn(
            &mut http,
            &SequenceStubLlm {
                responses: Mutex::new(Vec::new()),
            },
            &config,
            &msg1,
            UiLocale::Zh,
            Instant::now(),
            first_outcome,
            first_telemetry,
        )
        .expect("finalize first turn");
        msg1.req_id = Some("req-office-resume-1".to_string());
        let first_turn_ledger = build_turn_ledger_start(&msg1, 1);
        self::reply_finalize::complete_turn(
            LaneTurnFinalizeContext {
                worker_lane_tag: "test",
                config: &config,
                system_inbound_tx: &system_inbound_tx,
                msg: Box::new(msg1.clone()),
                msg_start: Instant::now(),
                queue_wait_ms: 0,
                admission_ms: 0,
                worker_prepare_ms: 0,
                msg_key: 1,
                turn_ledger: Box::new(first_turn_ledger),
                latency_warn_ms: u128::MAX,
            },
            &mut HashMap::new(),
            &mut HashMap::new(),
            first_finalized,
            delivery_handoff::DeliveryHandoff {
                delivered: true,
                outbound_enqueue_ms: 0,
                reply_handoff_ms: 1,
            },
        );
        let active_runs = task_run_store
            .list_active_for_chat("qq_channel", "chat-office-resume", 8)
            .expect("list active task runs");
        assert!(active_runs.is_empty(), "{active_runs:#?}");
        let active_work = config
            .runtime
            .active_work_store
            .get("chat-office-resume")
            .expect("get active work")
            .expect("active work");
        assert_eq!(
            active_work.kind,
            crate::agent::ActiveWorkKind::InteractiveAction
        );
        assert_eq!(
            active_work.status,
            crate::agent::ForegroundWorkStatus::AwaitingUser
        );
        assert!(
            active_work.blocker.contains("`account_key`"),
            "{active_work:#?}"
        );
        assert!(
            active_work.blocker.contains("mail-work"),
            "{active_work:#?}"
        );
        assert!(
            active_work.blocker.contains("mail-personal"),
            "{active_work:#?}"
        );

        let second_observed = Arc::new(Mutex::new(Vec::new()));
        let second_turn_llm = ObservedSequenceStubLlm {
            responses: Mutex::new(vec![
                LlmResponse {
                    content: "[tool_use]".to_string(),
                    stop_reason: StopReason::ToolUse,
                    tool_calls: Some(vec![crate::llm::ToolCall {
                        id: "call_2".to_string(),
                        name: "mail".to_string(),
                        input: r#"{"op":"list","provider":"imap_smtp","account_key":"mail-work"}"#
                            .to_string(),
                    }]),
                },
                LlmResponse {
                    content: "已切到 Work 邮箱，并拿到 1 封邮件。".to_string(),
                    stop_reason: StopReason::EndTurn,
                    tool_calls: None,
                },
            ]),
            observed: Arc::clone(&second_observed),
        };
        let msg2 = PcMsg::new_inbound("qq_channel", "chat-office-resume", "用 Work", false)
            .expect("message");
        let turn_execution::ExecutedTurn {
            outcome: second_outcome,
            telemetry: second_telemetry,
        } = turn_execution::execute_turn(
            &mut http,
            &second_turn_llm,
            &msg2,
            &outbound_tx,
            "req-office-resume-2",
            &registry,
            &config,
            &mut repeat,
            UiLocale::Zh,
        )
        .expect("second execute turn");

        let WorkerOutcome::Content(delivered) = second_outcome;
        assert!(
            delivered.starts_with("已切到 Work 邮箱，并拿到 1 封邮件。"),
            "{delivered}"
        );
        assert!(
            !delivered.contains("<foreground_work_packet>"),
            "{delivered}"
        );
        assert!(second_telemetry.foreground_work_context_present);
        assert_eq!(second_telemetry.latency.tool_calls, 1);
        let seen_args = seen_args.lock().unwrap_or_else(|e| e.into_inner());
        assert_eq!(seen_args.len(), 2, "{seen_args:#?}");
        assert!(
            seen_args[1].contains(r#""account_key":"mail-work""#),
            "{seen_args:#?}"
        );
        let observed = second_observed.lock().unwrap_or_else(|e| e.into_inner());
        assert_eq!(observed.len(), 2, "{observed:#?}");
        assert!(
            observed[1].message_dump.contains("`account_key`"),
            "{:#?}",
            observed[1]
        );
        assert!(
            observed[1].message_dump.contains("mail-work"),
            "{:#?}",
            observed[1]
        );
        assert!(
            observed[1].message_dump.contains("mail-personal"),
            "{:#?}",
            observed[1]
        );
    }

    #[test]
    fn complete_turn_clears_active_work_for_cancel_active_action_turn() {
        let config = test_agent_loop_config();
        config
            .runtime
            .active_work_store
            .set(
                "chat-cancel-run",
                &crate::agent::ActiveWorkRecord {
                    kind: crate::agent::ActiveWorkKind::InteractiveAction,
                    title: "QQ 邮箱配置".to_string(),
                    status: crate::agent::ForegroundWorkStatus::AwaitingUser,
                    continuity_open: true,
                    blocks_background_llm: true,
                    progress_summary: "账户草案已创建".to_string(),
                    blocker: "等待用户补充 provider_kind".to_string(),
                    next_action: "请用户补充 provider_kind".to_string(),
                    recent_outcome: String::new(),
                    active_artifact_refs: Vec::new(),
                    updated_at: 9,
                },
            )
            .expect("seed active work");
        let (system_inbound_tx, _system_inbound_rx, _) = crate::bus::new_system_inbound_channel(8);
        let (_outbound_tx, _outbound_rx, _) = crate::bus::new_inbound_channel(8);
        let mut msg = PcMsg::new_inbound(
            "qq_channel",
            "chat-cancel-run",
            "先别配了，这个动作取消",
            false,
        )
        .expect("message");
        msg.req_id = Some("req-cancel-run".to_string());
        let turn_ledger = build_turn_ledger_start(&msg, 1);
        let finalized = reply_finalize::FinalizedTurn {
            delivery: DeliveryReport::default(),
            reply: crate::agent::final_reply::CanonicalReply::new(
                "好，当前配置动作先取消。".to_string(),
            ),
            artifact_bundle: None,
            is_interrupt: false,
            reply_already_delivered: false,
            skip_delivery: false,
            mark_important: false,
            streamed: false,
            msg_start: Instant::now(),
            turn_observation: None,
            mental_privacy_review: MentalPrivacyReviewOutcome {
                reply_content: "好，当前配置动作先取消。".to_string(),
                action: crate::memory::MentalPrivacyShareAction::AllowOriginal,
                applied: false,
                touched_targets: Vec::new(),
            },
            review_input_before: "好，当前配置动作先取消。".to_string(),
            worker_latency: WorkerLatency::default(),
            any_tool_used: false,
            external_content_used: false,
            pressure: crate::orchestrator::PressureLevel::Normal,
            reply_surface: ReplySurface::GovernedConversation,
            prompt_recall_intent: crate::memory::PromptRecallIntent::Mixed,
            runtime_skill_selected_ids: Vec::new(),
            task_learning_selected_ids: Vec::new(),
            programmable_reasoning_intent: None,
            counterfactual_analysis: None,
            adversarial_arena_adjudication: None,
            subject_state: None,
            soul_feedback_projection: None,
            mental_privacy_adjudication: None,
            persona_priority_adjudication: None,
        };

        reply_finalize::complete_turn(
            LaneTurnFinalizeContext {
                worker_lane_tag: "test",
                config: &config,
                system_inbound_tx: &system_inbound_tx,
                msg: Box::new(msg.clone()),
                msg_start: Instant::now(),
                queue_wait_ms: 0,
                admission_ms: 0,
                worker_prepare_ms: 0,
                msg_key: 1,
                turn_ledger: Box::new(turn_ledger),
                latency_warn_ms: u128::MAX,
            },
            &mut HashMap::new(),
            &mut HashMap::new(),
            finalized,
            delivery_handoff::DeliveryHandoff {
                delivered: true,
                outbound_enqueue_ms: 0,
                reply_handoff_ms: 1,
            },
        );

        assert!(config
            .runtime
            .active_work_store
            .get("chat-cancel-run")
            .expect("get active work")
            .is_none());
    }

    #[test]
    fn complete_turn_does_not_abort_active_task_run_from_plain_reply_guess() {
        let config = test_agent_loop_config();
        let now_secs = 9;
        let planner_decision = crate::task_execution::TaskPlannerDecision {
            route: crate::task_execution::TaskExecutionRoute::StartRun,
            reason: "durable multi-step work".to_string(),
            blocker_summary: String::new(),
            missing_fields: Vec::new(),
            clarification_fields: Vec::new(),
            title: "QQ 邮箱配置".to_string(),
            goal: "配置 QQ 邮箱账户".to_string(),
            completion_definition: "账户已保存并通过校验".to_string(),
            risk_notes: Vec::new(),
            steps: vec![crate::task_execution::TaskPlannerStepDraft {
                title: "补认证信息".to_string(),
                instruction: "写入 provider_kind 并补认证凭据".to_string(),
                tool_budget: 1,
                retry_budget: 1,
                expected_artifacts: Vec::new(),
                review_criteria: Vec::new(),
            }],
        };
        let record = crate::task_execution::build_task_run_record(
            "run-cancel-task",
            "qq_channel",
            "chat-cancel-task-run",
            "帮我配置 QQ 邮箱账户",
            &planner_decision,
            now_secs,
        )
        .expect("task run");
        config
            .runtime
            .task_run_store
            .upsert(&record)
            .expect("seed active task run");
        config
            .runtime
            .active_work_store
            .set(
                "chat-cancel-task-run",
                &crate::agent::ActiveWorkRecord {
                    kind: crate::agent::ActiveWorkKind::TaskExecution,
                    title: "QQ 邮箱配置".to_string(),
                    status: crate::agent::ForegroundWorkStatus::Running,
                    continuity_open: true,
                    blocks_background_llm: true,
                    progress_summary: "账户草案已创建".to_string(),
                    blocker: String::new(),
                    next_action: "补认证信息".to_string(),
                    recent_outcome: String::new(),
                    active_artifact_refs: Vec::new(),
                    updated_at: now_secs,
                },
            )
            .expect("seed active work");
        let (system_inbound_tx, _system_inbound_rx, _) = crate::bus::new_system_inbound_channel(8);
        let (_outbound_tx, _outbound_rx, _) = crate::bus::new_inbound_channel(8);
        let mut msg =
            PcMsg::new_inbound("qq_channel", "chat-cancel-task-run", "算了，先停下", false)
                .expect("message");
        msg.req_id = Some("req-cancel-task-run".to_string());
        let turn_ledger = build_turn_ledger_start(&msg, 1);
        let finalized = reply_finalize::FinalizedTurn {
            delivery: DeliveryReport::default(),
            reply: crate::agent::final_reply::CanonicalReply::new(
                "好，我先停下当前这条正式任务。".to_string(),
            ),
            artifact_bundle: None,
            is_interrupt: false,
            reply_already_delivered: false,
            skip_delivery: false,
            mark_important: false,
            streamed: false,
            msg_start: Instant::now(),
            turn_observation: None,
            mental_privacy_review: MentalPrivacyReviewOutcome {
                reply_content: "好，我先停下当前这条正式任务。".to_string(),
                action: crate::memory::MentalPrivacyShareAction::AllowOriginal,
                applied: false,
                touched_targets: Vec::new(),
            },
            review_input_before: "好，我先停下当前这条正式任务。".to_string(),
            worker_latency: WorkerLatency::default(),
            any_tool_used: false,
            external_content_used: false,
            pressure: crate::orchestrator::PressureLevel::Normal,
            reply_surface: ReplySurface::GovernedConversation,
            prompt_recall_intent: crate::memory::PromptRecallIntent::Mixed,
            runtime_skill_selected_ids: Vec::new(),
            task_learning_selected_ids: Vec::new(),
            programmable_reasoning_intent: None,
            counterfactual_analysis: None,
            adversarial_arena_adjudication: None,
            subject_state: None,
            soul_feedback_projection: None,
            mental_privacy_adjudication: None,
            persona_priority_adjudication: None,
        };

        reply_finalize::complete_turn(
            LaneTurnFinalizeContext {
                worker_lane_tag: "test",
                config: &config,
                system_inbound_tx: &system_inbound_tx,
                msg: Box::new(msg.clone()),
                msg_start: Instant::now(),
                queue_wait_ms: 0,
                admission_ms: 0,
                worker_prepare_ms: 0,
                msg_key: 1,
                turn_ledger: Box::new(turn_ledger),
                latency_warn_ms: u128::MAX,
            },
            &mut HashMap::new(),
            &mut HashMap::new(),
            finalized,
            delivery_handoff::DeliveryHandoff {
                delivered: true,
                outbound_enqueue_ms: 0,
                reply_handoff_ms: 1,
            },
        );

        let settled = config
            .runtime
            .task_run_store
            .get("run-cancel-task")
            .expect("get task run")
            .expect("stored task run");
        assert_eq!(
            settled.run.status,
            crate::task_execution::TaskRunStatus::Planning
        );
        assert!(settled.run.failure_reason.is_empty());
        let active_work = config
            .runtime
            .active_work_store
            .get("chat-cancel-task-run")
            .expect("get active work")
            .expect("active work should remain until a structured task settlement exists");
        assert_eq!(
            active_work.kind,
            crate::agent::ActiveWorkKind::TaskExecution
        );
    }

    #[test]
    fn execute_turn_governed_surface_does_not_programmatically_rewrite_internal_mechanism_refusal()
    {
        let observed = Arc::new(Mutex::new(Vec::new()));
        let llm = ObservedSequenceStubLlm {
            responses: Mutex::new(vec![
                LlmResponse {
                    content: "[tool_use]".to_string(),
                    stop_reason: StopReason::ToolUse,
                    tool_calls: Some(vec![crate::llm::ToolCall {
                        id: "call_1".to_string(),
                        name: "board_info".to_string(),
                        input: "{}".to_string(),
                    }]),
                },
                LlmResponse {
                    content:
                        "系统信息属于内部运行机制，为了保护持续性和稳定性，这部分内容不对外公开。"
                            .to_string(),
                    stop_reason: StopReason::EndTurn,
                    tool_calls: None,
                },
            ]),
            observed: Arc::clone(&observed),
        };
        let mut http = DummyPlatformHttp;
        let (outbound_tx, _outbound_rx, _) = crate::bus::new_inbound_channel(8);
        let mut registry = test_registry(&[("board_info", ToolLlmVisibility::user_and_system())]);
        registry.register(Box::new(StubBoardInfoTool));
        let mut config = test_agent_loop_config();
        config.strategy = AgentRunStrategy::LinuxEnhanced;
        let msg =
            PcMsg::new_inbound("qq_channel", "chat-ops", "查看系统信息", false).expect("message");
        let mut repeat = HashMap::new();

        let turn_execution::ExecutedTurn {
            outcome,
            telemetry: _,
        } = turn_execution::execute_turn(
            &mut http,
            &llm,
            &msg,
            &outbound_tx,
            "req-public-runtime-internal-refusal",
            &registry,
            &config,
            &mut repeat,
            UiLocale::Zh,
        )
        .expect("execute turn");

        assert!(matches!(
            outcome,
            WorkerOutcome::Content(ref text)
                if text == "系统信息属于内部运行机制，为了保护持续性和稳定性，这部分内容不对外公开。"
        ));
        let observed = observed.lock().unwrap_or_else(|e| e.into_inner());
        assert_eq!(observed.len(), 2);
    }

    #[test]
    fn execute_turn_governed_surface_does_not_issue_public_runtime_finalization_request() {
        let observed = Arc::new(Mutex::new(Vec::new()));
        let llm = ObservedSequenceStubLlm {
            responses: Mutex::new(vec![
                LlmResponse {
                    content: "[tool_use]".to_string(),
                    stop_reason: StopReason::ToolUse,
                    tool_calls: Some(vec![crate::llm::ToolCall {
                        id: "call_1".to_string(),
                        name: "board_info".to_string(),
                        input: "{}".to_string(),
                    }]),
                },
                LlmResponse {
                    content: "你好！".to_string(),
                    stop_reason: StopReason::EndTurn,
                    tool_calls: None,
                },
                LlmResponse {
                    content: r#"{"surface":"public_runtime","reply":"系统状态正常。"}"#.to_string(),
                    stop_reason: StopReason::EndTurn,
                    tool_calls: None,
                },
            ]),
            observed: Arc::clone(&observed),
        };
        let mut http = DummyPlatformHttp;
        let (outbound_tx, _outbound_rx, _) = crate::bus::new_inbound_channel(8);
        let mut registry = test_registry(&[("board_info", ToolLlmVisibility::user_and_system())]);
        registry.register(Box::new(StubBoardInfoTool));
        let mut config = test_agent_loop_config();
        config.strategy = AgentRunStrategy::Embedded;
        let msg =
            PcMsg::new_inbound("qq_channel", "chat-ops", "查看系统状态", false).expect("message");
        let mut repeat = HashMap::new();

        let _ = turn_execution::execute_turn(
            &mut http,
            &llm,
            &msg,
            &outbound_tx,
            "req-public-runtime-surface-evidence",
            &registry,
            &config,
            &mut repeat,
            UiLocale::Zh,
        )
        .expect("execute turn");

        let observed = observed.lock().unwrap_or_else(|e| e.into_inner());
        assert!(
            observed
                .iter()
                .all(|request| !request.system.contains("Public Runtime Finalization")),
            "{observed:#?}"
        );
    }

    #[test]
    fn prepared_worker_conversation_size_stays_within_compact_budget() {
        let size = std::mem::size_of::<PreparedWorkerConversation>();
        assert!(
            size <= 512,
            "PreparedWorkerConversation should stay compact on ESP; got {size} bytes"
        );
    }

    #[test]
    fn build_turn_observation_ledger_captures_tool_path_mode_and_blocker() {
        let telemetry = WorkerRunTelemetry {
            streamed: false,
            latency: WorkerLatency {
                react_rounds: 3,
                tool_calls: 2,
                ..WorkerLatency::default()
            },
            delivery: DeliveryReport {
                current_primary_delivered: true,
                ..DeliveryReport::default()
            },
            artifact_bundle: None,
            any_tool_round_executed: true,
            any_tool_used: true,
            tool_round_completion: ToolRoundCompletionTelemetry::default(),
            external_content_used: false,
            task_execution_used: false,
            foreground_work_context_present: false,
            soul_feedback_projection: None,
            pressure: crate::orchestrator::PressureLevel::Cautious,
            runtime_mode: crate::runtime::RuntimeModeSnapshot {
                current_mode: crate::runtime::RuntimeMode::Normal,
                wifi_sta_connected: true,
                boot_phase_active: false,
                pairing_required: false,
                pairing_state_known: false,
                voice_exclusive_active: false,
                background_maintenance_active: false,
                config_plane_alive: false,
                config_active: false,
                config_activity_phase: crate::runtime::ConfigActivityPhase::Idle,
                channel_plane_alive: true,
                voice_plane_alive: false,
                agent_plane_alive: true,
                external_wss_managed_present: false,
                external_wss_suspend_requested: false,
                external_wss_suspended: false,
                recovery_safe_mode_active: false,
                runtime_foreground: crate::runtime::RuntimeForegroundOverlay::default(),
                action_budget: crate::runtime::RuntimeModeActionBudget {
                    allow_periodic_maintenance: true,
                    allow_due_user_timers: true,
                    allow_heartbeat_injection: true,
                    allow_best_effort_delayed_tasks: true,
                    allow_idle_self_runtime: true,
                    allow_non_voice_outbound: true,
                    allow_realtime_voice_connect: true,
                    allow_external_wss_connect: true,
                    require_external_wss_suspended: false,
                },
            },
            deliberation_class: crate::memory::TurnDeliberationClass::HardReasoning,
            reply_surface: ReplySurface::GovernedConversation,
            prompt_recall_intent: crate::memory::PromptRecallIntent::Mixed,
            runtime_skill_selected_ids: Vec::new(),
            task_learning_selected_ids: Vec::new(),
            programmable_reasoning_intent: None,
            counterfactual_analysis: None,
            adversarial_arena_adjudication: None,
            subject_state: None,
            mental_privacy_adjudication: None,
            persona_priority_adjudication: None,
        };

        let observation = build_turn_observation_ledger("tool_primary_delivery", false, &telemetry)
            .expect("observation");

        assert_eq!(
            observation.execution_class,
            TurnExecutionClass::ToolAssisted
        );
        assert_eq!(
            observation.deliberation_class,
            crate::memory::TurnDeliberationClass::HardReasoning
        );
        assert_eq!(observation.final_outcome, "tool_primary_delivery");
        assert_eq!(
            observation.pressure,
            crate::memory::TurnPersonaPressureLevel::Cautious
        );
        assert_eq!(observation.mode.current_mode, "normal");
        assert!(observation.mode.allow_non_voice_outbound);
        assert!(observation.mode.allow_idle_self_runtime);
        assert_eq!(observation.tool_path.path, "tool_primary_delivery");
        assert_eq!(observation.tool_path.tool_calls, 2);
        assert_eq!(observation.tool_path.react_rounds, 3);
        assert!(observation.tool_path.current_primary_delivered);
        assert!(observation.blocker.is_none());
    }

    #[test]
    fn agent_turn_natural_language_regression_matrix_catches_mainline_shape_regressions() {
        let cases = vec![
            AgentTurnBenchmarkCase {
                name: "direct reply stays single llm turn",
                msg: PcMsg::new_inbound("qq_channel", "chat-1", "直接回答", false)
                    .expect("message"),
                registry_mode: BenchmarkRegistryMode::Empty,
                strategy: AgentRunStrategy::Embedded,
                responses: vec![LlmResponse {
                    content: "直接答复".to_string(),
                    stop_reason: StopReason::EndTurn,
                    tool_calls: None,
                }],
                expected_llm_calls: 1,
                expected_react_rounds: 1,
                expected_tool_calls: 0,
                expected_streamed: false,
                expected_current_primary_delivered: false,
                expected_outcome_fragment: "直接答复",
            },
            AgentTurnBenchmarkCase {
                name: "message primary request stays on canonical reply path",
                msg: PcMsg::new_inbound("qq_channel", "chat-1", "测试多轮发送", false)
                    .expect("message"),
                registry_mode: BenchmarkRegistryMode::MessagePrimary,
                strategy: AgentRunStrategy::Embedded,
                responses: vec![
                    LlmResponse {
                        content: "[tool_use]".to_string(),
                        stop_reason: StopReason::ToolUse,
                        tool_calls: Some(vec![crate::llm::ToolCall {
                            id: "call_1".to_string(),
                            name: "message".to_string(),
                            input: r#"{"content":"工具主答复","channel":"qq_channel","chat_id":"chat-2","delivery_kind":"primary"}"#
                                .to_string(),
                        }]),
                    },
                    LlmResponse {
                        content: "规范主回复".to_string(),
                        stop_reason: StopReason::EndTurn,
                        tool_calls: None,
                    },
                ],
                expected_llm_calls: 2,
                expected_react_rounds: 2,
                expected_tool_calls: 1,
                expected_streamed: false,
                expected_current_primary_delivered: false,
                expected_outcome_fragment: "规范主回复",
            },
            AgentTurnBenchmarkCase {
                name: "visible side effect draft avoids extra recovery round",
                msg: PcMsg::new_inbound("qq_channel", "chat-1", "兜底收尾", false)
                    .expect("message"),
                registry_mode: BenchmarkRegistryMode::MessagePrimary,
                strategy: AgentRunStrategy::Embedded,
                responses: vec![
                    LlmResponse {
                        content: "[tool_use]".to_string(),
                        stop_reason: StopReason::ToolUse,
                        tool_calls: Some(vec![crate::llm::ToolCall {
                            id: "call_1".to_string(),
                            name: "message".to_string(),
                            input: r#"{"content":"补充消息","channel":"qq_channel","chat_id":"chat-2","delivery_kind":"supplemental"}"#
                                .to_string(),
                        }]),
                    },
                    LlmResponse {
                        content: "先整理一下当前状态。".to_string(),
                        stop_reason: StopReason::EndTurn,
                        tool_calls: None,
                    },
                ],
                expected_llm_calls: 2,
                expected_react_rounds: 2,
                expected_tool_calls: 1,
                expected_streamed: false,
                expected_current_primary_delivered: false,
                expected_outcome_fragment: "先整理一下当前状态。",
            },
            AgentTurnBenchmarkCase {
                name: "linux enhanced direct reply stays single main llm turn",
                msg: PcMsg::new_inbound("qq_channel", "chat-1", "直接回答", false)
                    .expect("message"),
                registry_mode: BenchmarkRegistryMode::Empty,
                strategy: AgentRunStrategy::LinuxEnhanced,
                responses: vec![LlmResponse {
                    content: "直接答复".to_string(),
                    stop_reason: StopReason::EndTurn,
                    tool_calls: None,
                }],
                expected_llm_calls: 1,
                expected_react_rounds: 1,
                expected_tool_calls: 0,
                expected_streamed: false,
                expected_current_primary_delivered: false,
                expected_outcome_fragment: "直接答复",
            },
        ];

        for case in cases {
            let result = run_agent_turn_benchmark_case(case);
            assert!(result.passed, "agent turn benchmark failed: {:?}", result);
        }
    }
}
