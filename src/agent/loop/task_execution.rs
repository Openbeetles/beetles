use super::*;

pub(super) fn extract_worker_outcome_text(outcome: WorkerOutcome) -> String {
    let WorkerOutcome::Content(text) = outcome;
    text
}

fn terminal_progress_kind_for_status(
    status: TaskRunStatus,
) -> Option<crate::agent::delivery::TaskTerminalProgressKind> {
    match status {
        TaskRunStatus::Completed => {
            Some(crate::agent::delivery::TaskTerminalProgressKind::Completed)
        }
        TaskRunStatus::PartialComplete => {
            Some(crate::agent::delivery::TaskTerminalProgressKind::PartialComplete)
        }
        TaskRunStatus::Blocked | TaskRunStatus::Failed => {
            Some(crate::agent::delivery::TaskTerminalProgressKind::Blocked)
        }
        TaskRunStatus::Aborted => Some(crate::agent::delivery::TaskTerminalProgressKind::Aborted),
        TaskRunStatus::Planning | TaskRunStatus::Running => None,
    }
}

fn normalize_task_execution_route(
    route: TaskExecutionRoute,
    has_active_run: bool,
) -> TaskExecutionRoute {
    if route == TaskExecutionRoute::ResumeRun && !has_active_run {
        TaskExecutionRoute::StartRun
    } else {
        route
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn try_run_task_execution(
    worker_llm: &(dyn LlmClient + Send + Sync),
    msg: &crate::bus::PcMsg,
    outbound_tx: &OutboundTx,
    delivery: &mut DeliverySession<'_>,
    registry: &crate::tools::ToolRegistry,
    config: &AgentLoopConfig,
    request_plan: &AgentRequestPlan<'_>,
    tool_ctx: &mut HttpClientToolContext<'_>,
    loc: UiLocale,
    latency: &mut WorkerLatency,
    system: &str,
    messages: &[Message],
    system_scratch: &mut String,
    pressure: crate::orchestrator::PressureLevel,
    deliberation_class: crate::memory::TurnDeliberationClass,
    request_semantics: crate::agent::request_semantics::RequestSemantics,
    active_run: Option<TaskRunRecord>,
    subject_state: Option<SubjectState>,
    soul_feedback_projection: Option<SoulFeedbackProjection>,
    mental_privacy_adjudication: Option<crate::memory::MentalPrivacyDisclosureAdjudication>,
    persona_priority_adjudication: Option<PersonaPriorityAdjudication>,
) -> Result<Option<(WorkerOutcome, WorkerRunTelemetry)>> {
    let admission = super::task_execution_support::decide_formal_task_admission(
        msg,
        request_plan.has_tools(),
        pressure,
        deliberation_class,
        request_semantics,
        request_plan.reply_surface(),
        active_run.is_some(),
    );
    if admission == super::task_execution_support::FormalTaskAdmission::None {
        return Ok(None);
    }

    delivery.emit_task_planner_progress();
    let planner_system = super::prepare_system_with_suffix(
        system,
        TASK_EXECUTION_PLANNER_SYSTEM_SUFFIX,
        system_scratch,
    );
    let planner_t0 = metrics::record_llm_call_start();
    let planner_started = Instant::now();
    let planner_response = match worker_llm.chat(
        tool_ctx,
        planner_system,
        messages,
        None,
        ToolChoicePolicy::Auto,
    ) {
        Ok(response) => {
            metrics::record_llm_call_end(planner_t0);
            latency.llm_round_total_ms = latency
                .llm_round_total_ms
                .saturating_add(planner_started.elapsed().as_millis());
            response
        }
        Err(error) => {
            metrics::record_llm_call_end(planner_t0);
            log::debug!(
                "[task_execution] planner skipped after llm error: {}",
                error
            );
            return Ok(None);
        }
    };
    let mut planner_decision = match super::task_execution_support::parse_task_execution_json::<
        TaskPlannerDecision,
    >(&planner_response.content, "task_execution_planner")
    .and_then(normalize_task_planner_decision)
    {
        Ok(decision) => decision,
        Err(error) => {
            log::debug!(
                "[task_execution] planner skipped after parse error chat_id={}: {}",
                msg.chat_id,
                error
            );
            return Ok(None);
        }
    };
    if planner_decision.route == TaskExecutionRoute::DirectReply {
        return Ok(None);
    }
    planner_decision.route = match admission {
        super::task_execution_support::FormalTaskAdmission::ConsiderNewRun => {
            normalize_task_execution_route(planner_decision.route, active_run.is_some())
        }
        super::task_execution_support::FormalTaskAdmission::None => TaskExecutionRoute::DirectReply,
    };
    delivery.emit_task_action_progress(match planner_decision.route {
        TaskExecutionRoute::ResumeRun => crate::agent::delivery::TaskActionProgressKind::Resumed,
        TaskExecutionRoute::StartRun | TaskExecutionRoute::DirectReply => {
            crate::agent::delivery::TaskActionProgressKind::Started
        }
    });

    let now_secs = crate::util::current_unix_secs();
    let mut record = match planner_decision.route {
        TaskExecutionRoute::ResumeRun => {
            let mut record = active_run.clone().ok_or_else(|| {
                crate::Error::config(
                    "task_execution_resume",
                    "planner requested resume_run without an active run",
                )
            })?;
            record.run.status = TaskRunStatus::Planning;
            record.run.updated_at = now_secs;
            if !planner_decision.title.is_empty() {
                record.run.title = planner_decision.title.clone();
            }
            if !planner_decision.reason.is_empty() {
                record.run.planner_reason = planner_decision.reason.clone();
            }
            if !planner_decision.goal.is_empty() {
                record.plan.goal = planner_decision.goal.clone();
            }
            if !planner_decision.completion_definition.is_empty() {
                record.plan.completion_definition = planner_decision.completion_definition.clone();
            }
            if !planner_decision.risk_notes.is_empty() {
                record.plan.risk_notes = planner_decision.risk_notes.clone();
            }
            if !planner_decision.steps.is_empty() {
                apply_revised_remaining_steps(&mut record, &planner_decision.steps)?;
            }
            record
        }
        TaskExecutionRoute::StartRun | TaskExecutionRoute::DirectReply => build_task_run_record(
            &super::generate_task_run_id(msg),
            &msg.channel,
            &msg.chat_id,
            &msg.content,
            &planner_decision,
            now_secs,
        )?,
    };

    if planner_decision.route == TaskExecutionRoute::StartRun {
        if let Some(mut previous_run) = active_run {
            if previous_run.run.run_id != record.run.run_id {
                previous_run.run.status = TaskRunStatus::Blocked;
                previous_run.run.failure_reason =
                    "superseded by a newer task run in the same relationship".to_string();
                previous_run.run.updated_at = now_secs;
                previous_run.run.finished_at = now_secs;
                super::task_execution_support::persist_task_run_record(
                    config.runtime.task_run_store.as_ref(),
                    &previous_run,
                    "supersede_previous_run",
                );
            }
        }
    }

    super::task_execution_support::persist_task_run_record(
        config.runtime.task_run_store.as_ref(),
        &record,
        "task_plan_start",
    );
    let existing_ledger = config
        .runtime
        .task_execution_ledger_store
        .list(&record.run.run_id, usize::MAX)
        .unwrap_or_default();
    let mut ledger_sequence = next_ledger_sequence(&existing_ledger);
    super::task_execution_support::append_task_execution_ledger_entry(
        config.runtime.task_execution_ledger_store.as_ref(),
        &super::build_task_ledger_entry(
            &record.run.run_id,
            "",
            TaskLedgerKind::RunCreated,
            record.run.status,
            &record.run.planner_reason,
            ledger_sequence,
            now_secs,
        ),
        "task_run_created",
    );
    ledger_sequence = ledger_sequence.saturating_add(1);
    super::task_execution_support::append_task_execution_ledger_entry(
        config.runtime.task_execution_ledger_store.as_ref(),
        &super::build_task_ledger_entry(
            &record.run.run_id,
            "",
            if planner_decision.route == TaskExecutionRoute::ResumeRun {
                TaskLedgerKind::PlanRevised
            } else {
                TaskLedgerKind::PlanAccepted
            },
            record.run.status,
            &record.plan.goal,
            ledger_sequence,
            now_secs,
        ),
        "task_plan_accepted",
    );
    ledger_sequence = ledger_sequence.saturating_add(1);

    let mut any_tool_used = false;
    let mut external_content_used = false;
    let mut max_react_rounds = 0u32;

    loop {
        let step_index = record
            .plan
            .ordered_steps
            .iter()
            .position(|step| step.step_id == record.run.current_step_id)
            .or_else(|| {
                record
                    .plan
                    .ordered_steps
                    .iter()
                    .position(|step| !step.status.is_terminal())
            });
        let Some(step_index) = step_index else {
            record.run.status = TaskRunStatus::Completed;
            record.run.finished_at = now_secs;
            break;
        };

        record.run.status = TaskRunStatus::Running;
        record.run.current_step_id = record.plan.ordered_steps[step_index].step_id.clone();
        {
            let step = &mut record.plan.ordered_steps[step_index];
            step.status = TaskStepStatus::Running;
            step.attempt_count = step.attempt_count.saturating_add(1);
            if step.started_at == 0 {
                step.started_at = now_secs;
            }
        }
        record.run.updated_at = crate::util::current_unix_secs();
        super::task_execution_support::persist_task_run_record(
            config.runtime.task_run_store.as_ref(),
            &record,
            "step_started",
        );
        let current_step = record.plan.ordered_steps[step_index].clone();
        super::task_execution_support::append_task_execution_ledger_entry(
            config.runtime.task_execution_ledger_store.as_ref(),
            &super::build_task_ledger_entry(
                &record.run.run_id,
                &current_step.step_id,
                TaskLedgerKind::StepStarted,
                record.run.status,
                &current_step.title,
                ledger_sequence,
                crate::util::current_unix_secs(),
            ),
            "task_step_started",
        );
        ledger_sequence = ledger_sequence.saturating_add(1);

        let current_artifacts = config
            .runtime
            .task_artifact_store
            .list_for_run(&record.run.run_id, TASK_EXECUTION_ARTIFACT_PREVIEW_LIMIT)
            .unwrap_or_default();
        let step_request =
            super::build_task_step_request(&record, &current_step, &current_artifacts);
        let step_req_id = format!("{}-{}", record.run.run_id, current_step.step_id);
        let step_msg = PcMsg {
            channel: msg.channel.clone(),
            chat_id: msg.chat_id.clone(),
            content: step_request,
            req_id: Some(step_req_id.clone()),
            ingress: IngressKind::System,
            enqueue_ts_ms: super::now_unix_ms(),
            source_transport: crate::bus::MessageTransport::Internal,
            platform_message_id: String::new(),
            platform_event_id: String::new(),
            inbound_dedup_key: String::new(),
            is_group: false,
        };
        let mut step_repeat = HashMap::new();
        let super::turn_execution::ExecutedTurn {
            outcome: step_outcome,
            telemetry: step_telemetry,
        } = super::turn_execution::execute_turn(
            tool_ctx,
            worker_llm,
            &step_msg,
            outbound_tx,
            &step_req_id,
            registry,
            config,
            &mut step_repeat,
            loc,
        )?;
        latency.context_ms = latency
            .context_ms
            .saturating_add(step_telemetry.latency.context_ms);
        max_react_rounds = max_react_rounds.max(step_telemetry.latency.react_rounds);
        latency.llm_round_total_ms = latency
            .llm_round_total_ms
            .saturating_add(step_telemetry.latency.llm_round_total_ms);
        latency.tool_exec_ms = latency
            .tool_exec_ms
            .saturating_add(step_telemetry.latency.tool_exec_ms);
        latency.tool_calls = latency
            .tool_calls
            .saturating_add(step_telemetry.latency.tool_calls);
        if latency.ttft_ms.is_none() {
            latency.ttft_ms = step_telemetry.latency.ttft_ms;
        }
        any_tool_used |= step_telemetry.any_tool_used;
        external_content_used |= step_telemetry.external_content_used;
        let step_result = extract_worker_outcome_text(step_outcome);

        let artifact_sequence = config
            .runtime
            .task_artifact_store
            .list_for_run(&record.run.run_id, usize::MAX)
            .map(|items| items.len() + 1)
            .unwrap_or(1);
        let step_artifact = super::build_task_artifact_record(
            &record.run.run_id,
            &current_step.step_id,
            TaskArtifactKind::StepResult,
            &step_result,
            "executor",
            artifact_sequence,
            crate::util::current_unix_secs(),
        );
        super::task_execution_support::persist_task_artifact_record(
            config.runtime.task_artifact_store.as_ref(),
            &step_artifact,
            "task_step_result",
        );
        super::task_execution_support::append_task_execution_ledger_entry(
            config.runtime.task_execution_ledger_store.as_ref(),
            &super::build_task_ledger_entry(
                &record.run.run_id,
                &current_step.step_id,
                TaskLedgerKind::StepResultRecorded,
                record.run.status,
                &step_artifact.artifact.summary,
                ledger_sequence,
                crate::util::current_unix_secs(),
            ),
            "task_step_result",
        );
        ledger_sequence = ledger_sequence.saturating_add(1);

        let review_request = super::build_task_review_request(
            &record,
            &current_step,
            &step_artifact,
            &current_artifacts,
        );
        let review_system = super::prepare_system_with_suffix(
            system,
            TASK_EXECUTION_REVIEW_SYSTEM_SUFFIX,
            system_scratch,
        );
        let review_started = Instant::now();
        let review_t0 = metrics::record_llm_call_start();
        let review_outcome = match worker_llm.chat(
            tool_ctx,
            review_system,
            &[Message {
                role: Cow::Borrowed("user"),
                content: review_request,
            }],
            None,
            ToolChoicePolicy::Auto,
        ) {
            Ok(response) => {
                metrics::record_llm_call_end(review_t0);
                latency.llm_round_total_ms = latency
                    .llm_round_total_ms
                    .saturating_add(review_started.elapsed().as_millis());
                super::task_execution_support::parse_task_execution_json::<TaskReviewOutcome>(
                    &response.content,
                    "task_execution_review",
                )
                .and_then(normalize_task_review_outcome)
                .unwrap_or(TaskReviewOutcome {
                    decision: TaskReviewDecision::PartialComplete,
                    summary: "task review unavailable; stopped without claiming completion"
                        .to_string(),
                    artifact_summary: summarize_task_artifact_content(&step_result),
                    revised_steps: Vec::new(),
                    durable_facts: Vec::new(),
                    reusable_procedures: Vec::new(),
                    evidence_only: Vec::new(),
                    transient_artifact_ids: Vec::new(),
                })
            }
            Err(error) => {
                metrics::record_llm_call_end(review_t0);
                latency.llm_round_total_ms = latency
                    .llm_round_total_ms
                    .saturating_add(review_started.elapsed().as_millis());
                log::warn!(
                    "[task_execution] review failed run_id={}: {}",
                    record.run.run_id,
                    error
                );
                TaskReviewOutcome {
                    decision: TaskReviewDecision::PartialComplete,
                    summary: "task review failed; stopped without claiming completion".to_string(),
                    artifact_summary: summarize_task_artifact_content(&step_result),
                    revised_steps: Vec::new(),
                    durable_facts: Vec::new(),
                    reusable_procedures: Vec::new(),
                    evidence_only: Vec::new(),
                    transient_artifact_ids: Vec::new(),
                }
            }
        };
        let review_artifact = super::build_task_artifact_record(
            &record.run.run_id,
            &current_step.step_id,
            TaskArtifactKind::Review,
            &review_outcome.summary,
            "reviewer",
            artifact_sequence + 1,
            crate::util::current_unix_secs(),
        );
        super::task_execution_support::persist_task_artifact_record(
            config.runtime.task_artifact_store.as_ref(),
            &review_artifact,
            "task_review_result",
        );
        for learning_record in build_task_learning_records(
            &record,
            &current_step.step_id,
            &step_artifact,
            &review_artifact,
            &review_outcome.durable_facts,
            &review_outcome.reusable_procedures,
            &review_outcome.evidence_only,
            &review_outcome.transient_artifact_ids,
            &review_outcome.summary,
            crate::util::current_unix_secs(),
        ) {
            if let Err(error) = config.runtime.task_learning_store.upsert(&learning_record) {
                log::warn!(
                    "[task_execution] task learning persist failed run_id={} learning_id={}: {}",
                    record.run.run_id,
                    learning_record.learning_id,
                    error
                );
            }
        }
        super::task_execution_support::append_task_execution_ledger_entry(
            config.runtime.task_execution_ledger_store.as_ref(),
            &super::build_task_ledger_entry(
                &record.run.run_id,
                &current_step.step_id,
                TaskLedgerKind::StepReviewRecorded,
                record.run.status,
                &review_outcome.summary,
                ledger_sequence,
                crate::util::current_unix_secs(),
            ),
            "task_step_review",
        );
        ledger_sequence = ledger_sequence.saturating_add(1);

        {
            let step = &mut record.plan.ordered_steps[step_index];
            step.last_result_summary = step_artifact.artifact.summary.clone();
            step.last_review_summary = review_outcome.summary.clone();
        }
        match review_outcome.decision {
            TaskReviewDecision::Pass => {
                let step = &mut record.plan.ordered_steps[step_index];
                step.status = TaskStepStatus::Passed;
                step.finished_at = crate::util::current_unix_secs();
            }
            TaskReviewDecision::RevisePlan => {
                let step = &mut record.plan.ordered_steps[step_index];
                step.status = TaskStepStatus::Passed;
                step.finished_at = crate::util::current_unix_secs();
                apply_revised_remaining_steps(&mut record, &review_outcome.revised_steps)?;
                super::task_execution_support::append_task_execution_ledger_entry(
                    config.runtime.task_execution_ledger_store.as_ref(),
                    &super::build_task_ledger_entry(
                        &record.run.run_id,
                        &current_step.step_id,
                        TaskLedgerKind::PlanRevised,
                        record.run.status,
                        &review_outcome.summary,
                        ledger_sequence,
                        crate::util::current_unix_secs(),
                    ),
                    "task_plan_revised",
                );
                ledger_sequence = ledger_sequence.saturating_add(1);
            }
            TaskReviewDecision::RetryStep => {
                let step = &mut record.plan.ordered_steps[step_index];
                if step.attempt_count <= step.retry_budget {
                    step.status = TaskStepStatus::Retrying;
                    record.run.updated_at = crate::util::current_unix_secs();
                    super::task_execution_support::persist_task_run_record(
                        config.runtime.task_run_store.as_ref(),
                        &record,
                        "step_retry",
                    );
                    continue;
                }
                step.status = TaskStepStatus::Blocked;
                step.finished_at = crate::util::current_unix_secs();
                record.run.status = TaskRunStatus::Blocked;
                record.run.failure_reason = if review_outcome.summary.is_empty() {
                    format!("retry budget exhausted at {}", step.title)
                } else {
                    review_outcome.summary.clone()
                };
                record.run.final_summary = record.run.failure_reason.clone();
                record.run.finished_at = crate::util::current_unix_secs();
                break;
            }
            TaskReviewDecision::AbortRun => {
                let step = &mut record.plan.ordered_steps[step_index];
                step.status = TaskStepStatus::Failed;
                step.finished_at = crate::util::current_unix_secs();
                record.run.status = TaskRunStatus::Aborted;
                record.run.failure_reason = review_outcome.summary.clone();
                record.run.final_summary = review_outcome.summary.clone();
                record.run.finished_at = crate::util::current_unix_secs();
                break;
            }
            TaskReviewDecision::PartialComplete => {
                let step = &mut record.plan.ordered_steps[step_index];
                step.status = TaskStepStatus::Blocked;
                step.finished_at = crate::util::current_unix_secs();
                record.run.status = TaskRunStatus::PartialComplete;
                record.run.failure_reason = review_outcome.summary.clone();
                record.run.final_summary = review_outcome.summary.clone();
                record.run.finished_at = crate::util::current_unix_secs();
                break;
            }
        }

        let next_step_id = record
            .plan
            .ordered_steps
            .iter()
            .find(|step| !step.status.is_terminal())
            .map(|step| step.step_id.clone())
            .unwrap_or_default();
        record.run.current_step_id = next_step_id;
        record.run.updated_at = crate::util::current_unix_secs();
        if record.run.current_step_id.is_empty() {
            record.run.status = TaskRunStatus::Completed;
            record.run.final_summary = review_outcome.summary.clone();
            record.run.finished_at = crate::util::current_unix_secs();
            break;
        }
        super::task_execution_support::persist_task_run_record(
            config.runtime.task_run_store.as_ref(),
            &record,
            "step_passed",
        );
    }

    record.run.updated_at = crate::util::current_unix_secs();
    if record.run.status == TaskRunStatus::Planning {
        record.run.status = TaskRunStatus::Completed;
    }
    if record.run.status.is_terminal() && record.run.finished_at == 0 {
        record.run.finished_at = record.run.updated_at;
    }
    if let Some(kind) = terminal_progress_kind_for_status(record.run.status) {
        delivery.emit_task_terminal_progress(kind);
    }
    let final_artifact_count = config
        .runtime
        .task_artifact_store
        .list_for_run(&record.run.run_id, usize::MAX)
        .map(|items| items.len())
        .unwrap_or(0);
    let final_artifacts = config
        .runtime
        .task_artifact_store
        .list_for_run(&record.run.run_id, TASK_EXECUTION_ARTIFACT_PREVIEW_LIMIT)
        .unwrap_or_default();
    let finisher_request = super::build_task_finisher_request(&record, &final_artifacts);
    let finisher_system = super::prepare_system_with_suffix(
        system,
        TASK_EXECUTION_FINISHER_SYSTEM_SUFFIX,
        system_scratch,
    );
    let mut finisher_system = finisher_system.to_string();
    crate::agent::append_foreground_work_packet_guidance(
        &mut finisher_system,
        crate::orchestrator::current_budget().system_prompt_max,
    );
    let finisher_started = Instant::now();
    let finisher_t0 = metrics::record_llm_call_start();
    let final_reply = match worker_llm.chat(
        tool_ctx,
        &finisher_system,
        &[Message {
            role: Cow::Borrowed("user"),
            content: finisher_request,
        }],
        None,
        ToolChoicePolicy::Auto,
    ) {
        Ok(response) => {
            metrics::record_llm_call_end(finisher_t0);
            latency.llm_round_total_ms = latency
                .llm_round_total_ms
                .saturating_add(finisher_started.elapsed().as_millis());
            if latency.ttft_ms.is_none() && !response.content.trim().is_empty() {
                latency.ttft_ms = Some(finisher_started.elapsed().as_millis());
            }
            response.content.trim().to_string()
        }
        Err(error) => {
            metrics::record_llm_call_end(finisher_t0);
            latency.llm_round_total_ms = latency
                .llm_round_total_ms
                .saturating_add(finisher_started.elapsed().as_millis());
            log::warn!(
                "[task_execution] finisher failed run_id={}: {}",
                record.run.run_id,
                error
            );
            if record.run.final_summary.is_empty() {
                "This task run stopped before a clean final summary could be produced.".to_string()
            } else {
                record.run.final_summary.clone()
            }
        }
    };
    let final_artifact = super::build_task_artifact_record(
        &record.run.run_id,
        "",
        TaskArtifactKind::FinalReply,
        &final_reply,
        "finisher",
        final_artifact_count + 1,
        crate::util::current_unix_secs(),
    );
    super::task_execution_support::persist_task_artifact_record(
        config.runtime.task_artifact_store.as_ref(),
        &final_artifact,
        "task_final_reply",
    );
    super::task_execution_support::append_task_execution_ledger_entry(
        config.runtime.task_execution_ledger_store.as_ref(),
        &super::build_task_ledger_entry(
            &record.run.run_id,
            "",
            TaskLedgerKind::RunFinished,
            record.run.status,
            &final_reply,
            ledger_sequence,
            crate::util::current_unix_secs(),
        ),
        "task_run_finished",
    );
    record.run.updated_at = crate::util::current_unix_secs();
    if record.run.final_summary.is_empty() {
        record.run.final_summary = summarize_task_artifact_content(&final_reply);
    }
    if record.run.status.is_terminal() && record.run.finished_at == 0 {
        record.run.finished_at = record.run.updated_at;
    }
    super::task_execution_support::persist_task_run_record(
        config.runtime.task_run_store.as_ref(),
        &record,
        "task_run_finished",
    );
    latency.react_rounds = latency.react_rounds.max(max_react_rounds.max(1));
    Ok(Some((
        WorkerOutcome::Content(final_reply),
        build_task_execution_telemetry(
            &delivery.report(),
            any_tool_used,
            external_content_used,
            pressure,
            deliberation_class,
            request_semantics,
            latency,
            subject_state,
            soul_feedback_projection,
            mental_privacy_adjudication,
            persona_priority_adjudication,
        ),
    )))
}

#[allow(clippy::too_many_arguments)]
fn build_task_execution_telemetry(
    delivery: &DeliveryReport,
    any_tool_used: bool,
    external_content_used: bool,
    pressure: crate::orchestrator::PressureLevel,
    deliberation_class: crate::memory::TurnDeliberationClass,
    _request_semantics: crate::agent::request_semantics::RequestSemantics,
    latency: &WorkerLatency,
    subject_state: Option<SubjectState>,
    soul_feedback_projection: Option<SoulFeedbackProjection>,
    mental_privacy_adjudication: Option<crate::memory::MentalPrivacyDisclosureAdjudication>,
    persona_priority_adjudication: Option<PersonaPriorityAdjudication>,
) -> WorkerRunTelemetry {
    WorkerRunTelemetry {
        streamed: false,
        latency: WorkerLatency {
            context_ms: latency.context_ms,
            request_semantics_ms: latency.request_semantics_ms,
            surface_finalize_ms: latency.surface_finalize_ms,
            mental_privacy_review_ms: latency.mental_privacy_review_ms,
            llm_round_total_ms: latency.llm_round_total_ms,
            tool_exec_ms: latency.tool_exec_ms,
            session_write_ms: latency.session_write_ms,
            ttft_ms: latency.ttft_ms,
            react_rounds: latency.react_rounds,
            tool_calls: latency.tool_calls,
        },
        delivery: *delivery,
        any_tool_used,
        external_content_used,
        used_surface_finalization: false,
        task_execution_used: true,
        foreground_work_context_present: true,
        pressure,
        runtime_mode: crate::runtime::thread_registry::runtime_mode_snapshot(),
        deliberation_class,
        reply_surface: ReplySurface::TaskExecution,
        prompt_recall_intent: crate::memory::PromptRecallIntent::Mixed,
        runtime_skill_selected_ids: Vec::new(),
        task_learning_selected_ids: Vec::new(),
        programmable_reasoning_intent: None,
        counterfactual_analysis: None,
        adversarial_arena_adjudication: None,
        subject_state,
        soul_feedback_projection,
        mental_privacy_adjudication,
        persona_priority_adjudication,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_task_execution_route_downgrades_resume_without_active_run() {
        assert_eq!(
            normalize_task_execution_route(TaskExecutionRoute::ResumeRun, false),
            TaskExecutionRoute::StartRun
        );
        assert_eq!(
            normalize_task_execution_route(TaskExecutionRoute::ResumeRun, true),
            TaskExecutionRoute::ResumeRun
        );
    }

    #[test]
    fn task_execution_telemetry_preserves_governance_snapshot() {
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

        let telemetry = build_task_execution_telemetry(
            &DeliveryReport::default(),
            true,
            false,
            crate::orchestrator::PressureLevel::Normal,
            crate::memory::TurnDeliberationClass::HardReasoning,
            crate::agent::request_semantics::RequestSemantics::conservative_default(),
            &WorkerLatency::default(),
            Some(subject_state.clone()),
            Some(soul_feedback_projection.clone()),
            Some(mental_privacy_adjudication.clone()),
            Some(persona_priority_adjudication.clone()),
        );

        assert!(telemetry.task_execution_used);
        assert_eq!(telemetry.reply_surface, ReplySurface::TaskExecution);
        assert_eq!(telemetry.subject_state, Some(subject_state));
        assert_eq!(
            telemetry.soul_feedback_projection,
            Some(soul_feedback_projection)
        );
        assert_eq!(
            telemetry.mental_privacy_adjudication,
            Some(mental_privacy_adjudication)
        );
        assert_eq!(
            telemetry.persona_priority_adjudication,
            Some(persona_priority_adjudication)
        );
    }
}
