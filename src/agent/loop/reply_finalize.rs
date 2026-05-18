use super::*;
use crate::agent::final_reply::{
    build_canonical_reply, classify_reply_artifacts, finalize_user_visible_reply,
    reply_has_concrete_anchor, reply_looks_like_future_action_narration,
    reply_looks_like_transition_colon_draft, strip_legacy_internal_reply_blocks, CanonicalReply,
    ReplyArtifactBundle, ReplyArtifactState,
};
use crate::memory::EmotionSignalStore;

pub(super) struct FinalizedTurn {
    pub(super) delivery: DeliveryReport,
    pub(super) reply: CanonicalReply,
    pub(super) artifact_bundle: Option<ReplyArtifactBundle>,
    pub(super) is_interrupt: bool,
    pub(super) reply_already_delivered: bool,
    pub(super) skip_delivery: bool,
    pub(super) mark_important: bool,
    pub(super) streamed: bool,
    pub(super) msg_start: Instant,
    pub(super) turn_observation: Option<TurnObservationLedger>,
    pub(super) mental_privacy_review: MentalPrivacyReviewOutcome,
    pub(super) review_input_before: String,
    pub(super) worker_latency: WorkerLatency,
    pub(super) any_tool_used: bool,
    pub(super) external_content_used: bool,
    pub(super) pressure: crate::orchestrator::PressureLevel,
    pub(super) reply_surface: ReplySurface,
    pub(super) prompt_recall_intent: crate::memory::PromptRecallIntent,
    pub(super) runtime_skill_selected_ids: Vec<String>,
    pub(super) task_learning_selected_ids: Vec<String>,
    pub(super) programmable_reasoning_intent:
        Option<crate::agent::reasoning_intent::ProgrammableReasoningIntent>,
    pub(super) counterfactual_analysis:
        Option<crate::agent::counterfactual::CounterfactualAnalysis>,
    pub(super) adversarial_arena_adjudication:
        Option<crate::reasoning::AdversarialArenaAdjudication>,
    pub(super) subject_state: Option<SubjectState>,
    pub(super) soul_feedback_projection: Option<SoulFeedbackProjection>,
    pub(super) mental_privacy_adjudication:
        Option<crate::memory::MentalPrivacyDisclosureAdjudication>,
    pub(super) persona_priority_adjudication: Option<PersonaPriorityAdjudication>,
}

fn should_run_full_mental_privacy_review(
    reply_surface: ReplySurface,
    disclosure: Option<&crate::memory::MentalPrivacyDisclosureAdjudication>,
) -> bool {
    match reply_surface.governance_policy() {
        crate::agent::reply_surface::SurfaceGovernancePolicy::SkipMentalPrivacyReview
        | crate::agent::reply_surface::SurfaceGovernancePolicy::SuppressUserDelivery => false,
        crate::agent::reply_surface::SurfaceGovernancePolicy::PrivateBoundaryReview
        | crate::agent::reply_surface::SurfaceGovernancePolicy::TaskExecutionReview => true,
        crate::agent::reply_surface::SurfaceGovernancePolicy::ApplyMentalPrivacyReview => {
            disclosure.is_some()
        }
    }
}

fn truthful_no_new_execution_result_copy(loc: UiLocale) -> &'static str {
    match loc {
        UiLocale::Zh => "这轮还没有实际执行新的工具或任务步骤，也还没有产生新结果。",
        UiLocale::En => "This turn has not executed a new tool or task step yet, so there is no new result to report.",
    }
}

fn incomplete_turn_copy(
    loc: UiLocale,
    had_mutating_effects: bool,
    had_visible_side_effects: bool,
) -> &'static str {
    match loc {
        UiLocale::Zh if had_visible_side_effects => {
            "这轮执行已经产生对外更新，但没有形成可交付的最终答复。请以已发送内容为准。"
        }
        UiLocale::Zh if had_mutating_effects => {
            "这轮执行已经发生实际操作，但没有形成可交付的最终答复。"
        }
        UiLocale::Zh => "这轮执行没有形成可交付的最终答复。",
        UiLocale::En if had_visible_side_effects => {
            "This turn already produced visible outbound updates, but it did not form a deliverable final reply. Treat the sent updates as authoritative."
        }
        UiLocale::En if had_mutating_effects => {
            "This turn already performed a real operation, but it did not form a deliverable final reply."
        }
        UiLocale::En => "This turn did not form a deliverable final reply.",
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum TurnCompletionKind {
    FinalResult,
    TruthfulBlocker,
    PlanningOnly,
    IncompleteTurn,
    ArtifactOnly,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct TurnCompletionAssessment {
    pub(super) kind: TurnCompletionKind,
    pub(super) replay_safe: bool,
    pub(super) had_tool_activity: bool,
    pub(super) had_mutating_effects: bool,
    pub(super) had_visible_side_effects: bool,
}

impl TurnCompletionAssessment {
    fn should_rewrite_to_truthful_copy(self) -> bool {
        !self.had_tool_activity && matches!(self.kind, TurnCompletionKind::PlanningOnly)
    }

    fn replacement_copy(self, loc: UiLocale) -> Option<&'static str> {
        if self.should_rewrite_to_truthful_copy() {
            return Some(truthful_no_new_execution_result_copy(loc));
        }
        if self.had_tool_activity
            && matches!(
                self.kind,
                TurnCompletionKind::PlanningOnly
                    | TurnCompletionKind::IncompleteTurn
                    | TurnCompletionKind::ArtifactOnly
            )
        {
            return Some(incomplete_turn_copy(
                loc,
                self.had_mutating_effects,
                self.had_visible_side_effects,
            ));
        }
        None
    }
}

pub(super) fn assess_turn_completion(
    delivery: &DeliveryReport,
    any_tool_round_executed: bool,
    any_tool_used: bool,
    tool_round_completion: &ToolRoundCompletionTelemetry,
    reply_surface: ReplySurface,
    reply_content: &str,
) -> TurnCompletionAssessment {
    let trimmed = reply_content.trim();
    let had_tool_activity = any_tool_round_executed || any_tool_used;
    let had_visible_side_effects = tool_round_completion.had_visible_outbound_side_effects
        || delivery.tool_visible_updates_sent > 0
        || delivery.explicit_outbound_sent > 0
        || delivery.append_only_ack_sent > 0
        || delivery.append_only_heartbeat_sent > 0
        || delivery.append_only_first_tool_milestone_sent > 0
        || delivery.visible_text_updates_sent > 0
        || delivery.current_primary_delivered
        || delivery.finalize_streamed;
    let replay_safe = !tool_round_completion.had_mutating_effects && !had_visible_side_effects;
    let artifact_state = classify_reply_artifacts(reply_content);
    let kind = match artifact_state {
        _ if tool_round_completion.blocker.is_some() => TurnCompletionKind::TruthfulBlocker,
        ReplyArtifactState::ArtifactOnly => TurnCompletionKind::ArtifactOnly,
        ReplyArtifactState::InternalArtifactLeak => TurnCompletionKind::FinalResult,
        ReplyArtifactState::None if trimmed.is_empty() => TurnCompletionKind::IncompleteTurn,
        ReplyArtifactState::None
            if had_tool_activity
                && reply_surface == ReplySurface::PublicRuntime
                && !reply_has_concrete_anchor(trimmed) =>
        {
            TurnCompletionKind::IncompleteTurn
        }
        ReplyArtifactState::None
            if reply_looks_like_future_action_narration(trimmed)
                || reply_looks_like_transition_colon_draft(trimmed) =>
        {
            TurnCompletionKind::PlanningOnly
        }
        ReplyArtifactState::None => TurnCompletionKind::FinalResult,
    };
    TurnCompletionAssessment {
        kind,
        replay_safe,
        had_tool_activity,
        had_mutating_effects: tool_round_completion.had_mutating_effects,
        had_visible_side_effects,
    }
}

#[allow(clippy::too_many_arguments)]
#[cfg(test)]
pub(super) fn finalize_turn(
    http: &mut dyn PlatformHttpClient,
    worker_llm: &(dyn LlmClient + Send + Sync),
    config: &AgentLoopConfig,
    msg: &PcMsg,
    loc: UiLocale,
    msg_start: Instant,
    outcome: WorkerOutcome,
    telemetry: WorkerRunTelemetry,
) -> Result<FinalizedTurn> {
    finalize_turn_boxed(
        http,
        worker_llm,
        config,
        msg,
        loc,
        msg_start,
        Box::new(super::turn_execution::ExecutedTurn { outcome, telemetry }),
    )
    .map(|finalized| *finalized)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn finalize_turn_boxed(
    http: &mut dyn PlatformHttpClient,
    worker_llm: &(dyn LlmClient + Send + Sync),
    config: &AgentLoopConfig,
    msg: &PcMsg,
    loc: UiLocale,
    msg_start: Instant,
    executed: Box<super::turn_execution::ExecutedTurn>,
) -> Result<Box<FinalizedTurn>> {
    let mut executed = executed;
    let final_outcome = if executed.telemetry.delivery.current_primary_delivered {
        "current_primary"
    } else {
        "final_answer"
    };
    let turn_observation = build_turn_observation_ledger(final_outcome, false, &executed.telemetry);
    let streamed = executed.telemetry.streamed;
    let mut worker_latency = std::mem::take(&mut executed.telemetry.latency);
    let delivery = std::mem::take(&mut executed.telemetry.delivery);
    let artifact_bundle = executed.telemetry.artifact_bundle.take();
    let any_tool_round_executed = executed.telemetry.any_tool_round_executed;
    let any_tool_used = executed.telemetry.any_tool_used;
    let external_content_used = executed.telemetry.external_content_used;
    let _foreground_work_context_present = executed.telemetry.foreground_work_context_present;
    let pressure = executed.telemetry.pressure;
    let reply_surface = executed.telemetry.reply_surface;
    let prompt_recall_intent = executed.telemetry.prompt_recall_intent;
    let mental_privacy_adjudication = executed.telemetry.mental_privacy_adjudication.take();
    let persona_priority_adjudication = executed.telemetry.persona_priority_adjudication.take();

    // Canonical final reply is produced exactly once here.
    // Delivery only transports the already-finalized reply afterwards.
    let outcome = std::mem::replace(&mut executed.outcome, WorkerOutcome::Content(String::new()));
    let (mut reply_content, is_interrupt, reply_already_delivered, apply_finalizer) = match outcome
    {
        WorkerOutcome::Content(s) => {
            let cow = truncate_content_to_max(&s, MAX_CONTENT_LEN);
            let s = if let Cow::Borrowed(_) = &cow {
                s
            } else {
                cow.into_owned()
            };
            (s, false, false, true)
        }
    };
    if !is_interrupt {
        reply_content = strip_legacy_internal_reply_blocks(&reply_content);
    }

    if !is_interrupt && apply_finalizer {
        reply_content = finalize_user_visible_reply(config.strategy, &reply_content);
    }
    if !is_interrupt {
        let completion = assess_turn_completion(
            &delivery,
            any_tool_round_executed,
            any_tool_used,
            &executed.telemetry.tool_round_completion,
            reply_surface,
            &reply_content,
        );
        if let Some(copy) = completion.replacement_copy(loc) {
            log::warn!(
                "[reply_surface] completion_assessment rewrote non-deliverable reply kind={:?} replay_safe={} surface={} channel={} chat_id={}",
                completion.kind,
                completion.replay_safe,
                reply_surface.as_str(),
                msg.channel,
                msg.chat_id
            );
            reply_content = copy.to_string();
        }
    }
    let review_input_before = reply_content.clone();
    let mut mental_privacy_review = MentalPrivacyReviewOutcome {
        reply_content: reply_content.clone(),
        action: crate::memory::MentalPrivacyShareAction::AllowOriginal,
        applied: false,
        touched_targets: Vec::new(),
    };
    if !is_interrupt
        && should_run_full_mental_privacy_review(
            reply_surface,
            mental_privacy_adjudication.as_ref(),
        )
    {
        mental_privacy_review = super::worker_governance::maybe_apply_mental_privacy_review(
            http,
            worker_llm,
            config,
            msg,
            loc,
            reply_surface,
            reply_content,
            &mut worker_latency,
        );
        reply_content = mental_privacy_review.reply_content.clone();
    }
    let mark_important = !is_interrupt && reply_content.contains(AGENT_MARKER_MARK_IMPORTANT);
    let signal_comfort = !is_interrupt && reply_content.contains(AGENT_MARKER_SIGNAL_COMFORT);
    if mark_important || signal_comfort {
        reply_content = remove_substrings_all_trim(
            &reply_content,
            &[AGENT_MARKER_MARK_IMPORTANT, AGENT_MARKER_SIGNAL_COMFORT],
        );
        if signal_comfort {
            let _ = config
                .runtime
                .emotion_signal_store
                .set(&msg.chat_id, "comfort");
        }
        reply_content = truncate_content_to_max(&reply_content, MAX_CONTENT_LEN).into_owned();
    }
    if !is_interrupt && !reply_content.is_empty() {
        metrics::record_final_answer_call();
    }
    let reply = if is_interrupt {
        CanonicalReply::new(reply_content.clone())
    } else {
        match build_canonical_reply(config.strategy, &reply_content) {
            Ok(reply) => reply,
            Err(kind) => {
                metrics::record_empty_final_blocked();
                log::warn!(
                    "[reply_surface] canonical reply contract breached stage={} surface={} governance_policy={:?} channel={} chat_id={}",
                    kind.stage(),
                    reply_surface.as_str(),
                    reply_surface.governance_policy(),
                    msg.channel,
                    msg.chat_id
                );
                return Err(crate::error::Error::config(
                    kind.stage(),
                    format!(
                        "reply_surface={} governance_policy={:?} channel={} chat_id={}",
                        reply_surface.as_str(),
                        reply_surface.governance_policy(),
                        msg.channel,
                        msg.chat_id
                    ),
                ));
            }
        }
    };

    Ok(Box::new(FinalizedTurn {
        delivery,
        artifact_bundle,
        skip_delivery: reply.as_str().trim() == "SILENT"
            || (msg.channel.as_ref() == CHANNEL_CRON && reply.as_str().is_empty()),
        reply,
        is_interrupt,
        reply_already_delivered,
        mark_important,
        streamed,
        msg_start,
        turn_observation,
        mental_privacy_review,
        review_input_before,
        worker_latency,
        any_tool_used,
        external_content_used,
        pressure,
        reply_surface,
        prompt_recall_intent,
        runtime_skill_selected_ids: std::mem::take(
            &mut executed.telemetry.runtime_skill_selected_ids,
        ),
        task_learning_selected_ids: std::mem::take(
            &mut executed.telemetry.task_learning_selected_ids,
        ),
        programmable_reasoning_intent: executed.telemetry.programmable_reasoning_intent.take(),
        counterfactual_analysis: executed.telemetry.counterfactual_analysis.take(),
        adversarial_arena_adjudication: executed.telemetry.adversarial_arena_adjudication.take(),
        subject_state: executed.telemetry.subject_state.take(),
        soul_feedback_projection: executed.telemetry.soul_feedback_projection.take(),
        mental_privacy_adjudication,
        persona_priority_adjudication,
    }))
}

#[cfg(test)]
pub(super) fn complete_turn(
    ctx: LaneTurnFinalizeContext<'_>,
    llm_failure_count: &mut HashMap<u64, (u8, Instant)>,
    defer_tracker: &mut HashMap<u64, (u8, Instant)>,
    finalized: FinalizedTurn,
    handoff: super::delivery_handoff::DeliveryHandoff,
) {
    complete_turn_boxed(
        Box::new(ctx),
        llm_failure_count,
        defer_tracker,
        Box::new(finalized),
        handoff,
    );
}

#[allow(clippy::boxed_local)]
pub(super) fn complete_turn_boxed(
    mut ctx: Box<LaneTurnFinalizeContext<'_>>,
    llm_failure_count: &mut HashMap<u64, (u8, Instant)>,
    defer_tracker: &mut HashMap<u64, (u8, Instant)>,
    mut finalized: Box<FinalizedTurn>,
    handoff: super::delivery_handoff::DeliveryHandoff,
) {
    let reply_content = std::mem::take(&mut finalized.reply.visible_text);

    if finalized.skip_delivery {
        if crate::chat_stream::is_configure_ui_stream_turn(&ctx.msg) {
            if let Some(stream_id) = ctx.msg.req_id.as_deref() {
                ctx.config
                    .chat_streams
                    .emit_error(stream_id, "chat.no_response", None);
            }
        }
        llm_failure_count.remove(&ctx.msg_key);
        defer_tracker.remove(&ctx.msg_key);
        let total_ms = ctx.msg_start.elapsed().as_millis();
        metrics::record_e2e_ms(total_ms);
        if ctx.msg.ingress == IngressKind::System {
            let is_cron = ctx.msg.channel.as_ref() == CHANNEL_CRON;
            if is_cron {
                let cron_e2e = super::now_unix_ms().saturating_sub(ctx.msg.enqueue_ts_ms) as u128;
                metrics::record_cron_e2e_ms(cron_e2e);
            }
        }
        return;
    }

    let delivered = handoff.delivered;
    let outbound_enqueue_ms = handoff.outbound_enqueue_ms;
    let reply_handoff_ms = handoff.reply_handoff_ms;
    finalized.delivery.current_primary_delivered |= finalized.reply_already_delivered;
    finalized.delivery.finalize_streamed |= finalized.streamed && delivered;

    let session_start = Instant::now();
    let session_write_result = if delivered {
        let entries = [
            SessionMessage {
                role: "user".to_string(),
                content: ctx.msg.content.clone(),
            },
            SessionMessage {
                role: "assistant".to_string(),
                content: reply_content.clone(),
            },
        ];
        ctx.config
            .runtime
            .session_store
            .append_batch(&ctx.msg.chat_id, &entries)
    } else {
        ctx.config
            .runtime
            .session_store
            .append(&ctx.msg.chat_id, "user", &ctx.msg.content)
    };
    let session_appended = session_write_result.is_ok();
    if let Err(e) = &session_write_result {
        log::warn!("[agent_session] append failed: {}", e);
        metrics::record_error_by_stage("session_append");
    }
    if delivered && crate::chat_stream::is_configure_ui_stream_turn(&ctx.msg) {
        if let Some(stream_id) = ctx.msg.req_id.as_deref() {
            let message_id = if session_appended {
                ctx.config
                    .runtime
                    .session_store
                    .load_recent_records(&ctx.msg.chat_id, 1)
                    .ok()
                    .and_then(|records| records.last().map(|record| record.message_id.clone()))
            } else {
                None
            };
            ctx.config.chat_streams.emit_final(
                stream_id,
                &reply_content,
                session_appended,
                message_id.as_deref(),
            );
        }
    }
    finalized.worker_latency.session_write_ms = finalized
        .worker_latency
        .session_write_ms
        .saturating_add(session_start.elapsed().as_millis());
    llm_failure_count.remove(&ctx.msg_key);
    defer_tracker.remove(&ctx.msg_key);

    if delivered && finalized.mark_important {
        let _ = ctx
            .config
            .runtime
            .important_message_store
            .set_important_offset_from_end(&ctx.msg.chat_id, 1);
    } else if delivered
        && ctx
            .config
            .runtime
            .important_message_store
            .get_important_offset(&ctx.msg.chat_id)
            .ok()
            .flatten()
            .is_some()
    {
        let _ = ctx
            .config
            .runtime
            .important_message_store
            .clear_important(&ctx.msg.chat_id);
    }
    let now_secs = super::now_unix_ms() / 1000;
    let reply_requests_input = finalized
        .turn_observation
        .as_ref()
        .and_then(|observation| observation.blocker.as_ref())
        .and_then(|blocker| crate::agent::parse_workflow_outcome_kind(&blocker.kind))
        .map(crate::agent::WorkflowOutcomeKind::requests_user_input)
        .unwrap_or(false);
    let clear_execution_state = delivered
        && ctx.msg.ingress == IngressKind::User
        && finalized.reply_surface != ReplySurface::TaskExecution;
    if clear_execution_state {
        if let Err(error) = ctx
            .config
            .runtime
            .execution_state_store
            .clear(&ctx.msg.chat_id)
        {
            log::warn!(
                "[agent_execution_state] clear failed chat_id={}: {}",
                ctx.msg.chat_id,
                error
            );
        }
    }
    let should_seed_execution_state = delivered
        && ctx.msg.ingress == IngressKind::User
        && finalized.reply_surface != ReplySurface::PrivateBoundary
        && (reply_requests_input
            || finalized.any_tool_used
            || matches!(finalized.reply_surface, ReplySurface::TaskExecution)
            || finalized
                .turn_observation
                .as_ref()
                .and_then(|observation| observation.blocker.as_ref())
                .is_some());
    if should_seed_execution_state {
        if let Err(error) = crate::memory::seed_execution_state_from_turn(
            ctx.config.runtime.execution_state_store.as_ref(),
            crate::memory::ProvisionalExecutionStateInput {
                chat_id: &ctx.msg.chat_id,
                ingress: ctx.msg.ingress,
                channel: ctx.msg.channel.as_ref(),
                user_content: &ctx.msg.content,
                reply_content: &reply_content,
                reply_requests_input,
                tool_calls: finalized.worker_latency.tool_calls,
                now_secs,
                turn_observation: finalized.turn_observation.as_ref(),
            },
        ) {
            log::warn!(
                "[agent_execution_state] provisional seed failed chat_id={}: {}",
                ctx.msg.chat_id,
                error
            );
        }
    }
    if delivered && ctx.msg.ingress == IngressKind::User {
        let active_task_run = active_task_run_for_chat(
            ctx.config.runtime.task_run_store.as_ref(),
            ctx.msg.channel.as_ref(),
            ctx.msg.chat_id.as_ref(),
        )
        .ok()
        .flatten();
        let execution_state = if active_task_run.is_none() {
            match ctx
                .config
                .runtime
                .execution_state_store
                .get(&ctx.msg.chat_id)
            {
                Ok(state) => state,
                Err(error) => {
                    log::warn!(
                        "[agent_execution_state] read failed chat_id={}: {}",
                        ctx.msg.chat_id,
                        error
                    );
                    None
                }
            }
        } else {
            None
        };
        if let Err(error) = crate::agent::sync_active_work_after_turn(
            ctx.config.runtime.active_work_store.as_ref(),
            crate::agent::ActiveWorkSyncInput {
                chat_id: &ctx.msg.chat_id,
                active_task_run: active_task_run.as_ref(),
                execution_state: execution_state.as_ref(),
                user_request: &ctx.msg.content,
                now_secs,
            },
        ) {
            log::warn!(
                "[agent_active_work] sync failed chat_id={}: {}",
                ctx.msg.chat_id,
                error
            );
        }
    }
    let llm_ms = finalized
        .worker_latency
        .context_ms
        .saturating_add(finalized.worker_latency.llm_round_total_ms)
        .saturating_add(finalized.worker_latency.tool_exec_ms)
        .saturating_add(finalized.worker_latency.session_write_ms);

    let reuse_outcome = if finalized.is_interrupt
        || (finalized.runtime_skill_selected_ids.is_empty()
            && finalized.task_learning_selected_ids.is_empty())
    {
        crate::skills::RuntimeSkillReuseOutcome::Neutral
    } else {
        crate::skills::RuntimeSkillReuseOutcome::Succeeded
    };
    let reuse_outcome_note =
        if finalized.reply_already_delivered || finalized.delivery.current_primary_delivered {
            "current_primary"
        } else {
            "final_answer"
        };

    if delivered
        && !super::background_jobs::enqueue_post_reply_maintenance_job(
            ctx.config.runtime.active_work_store.as_ref(),
            ctx.config.runtime.detached_work_store.as_ref(),
            ctx.system_inbound_tx,
            &ctx.msg,
            &reply_content,
            finalized.worker_latency.tool_calls,
            finalized.external_content_used,
            finalized.prompt_recall_intent,
            &finalized.runtime_skill_selected_ids,
            &finalized.task_learning_selected_ids,
            reuse_outcome,
            reuse_outcome_note,
            ctx.config.runtime.memory_system_kind.memory_profile(),
        )
    {
        log::debug!(
            "[agent_memory] post-reply maintenance job skipped chat_id={}",
            ctx.msg.chat_id
        );
    }
    let self_runtime_post_reply_enqueued = delivered
        && crate::memory::enqueue_self_runtime_post_reply(
            ctx.system_inbound_tx,
            ctx.config.runtime.detached_work_store.as_ref(),
            ctx.config.runtime.active_work_store.as_ref(),
            ctx.config.runtime.self_continuity_store.as_ref(),
            ctx.config.runtime.autonomy_strategy_store.as_ref(),
            ctx.config.runtime.self_authored_core_store.as_ref(),
            ctx.config.runtime.memory_system_kind.memory_profile(),
            ctx.msg.chat_id.as_ref(),
            ctx.msg.channel.as_ref(),
            &ctx.msg.content,
            &reply_content,
            finalized.worker_latency.tool_calls,
            finalized.external_content_used,
        );
    if delivered && !self_runtime_post_reply_enqueued {
        log::debug!(
            "[self_runtime] post-reply job skipped chat_id={}",
            ctx.msg.chat_id
        );
    }
    if let Some(projection) = finalized.soul_feedback_projection.as_mut() {
        projection.strategy.post_reply_self_runtime_enqueued = self_runtime_post_reply_enqueued;
        if self_runtime_post_reply_enqueued {
            projection.strategy.applied = true;
            if !projection
                .strategy
                .signal_layers
                .iter()
                .any(|layer| layer == "self_runtime_scheduler")
            {
                projection
                    .strategy
                    .signal_layers
                    .push("self_runtime_scheduler".to_string());
            }
        }
    }

    let total_ms = ctx.msg_start.elapsed().as_millis();
    let post_reply_ms = total_ms.saturating_sub(reply_handoff_ms);
    let canonical_reply_source = if finalized.is_interrupt {
        "interrupt"
    } else if finalized.reply_already_delivered || finalized.delivery.current_primary_delivered {
        "current_primary"
    } else {
        "final_answer"
    };
    let outbound_source =
        if finalized.reply_already_delivered || finalized.delivery.current_primary_delivered {
            "current_primary"
        } else if finalized.streamed && delivered {
            "stream_edit"
        } else if delivered {
            "reply"
        } else {
            ""
        };
    ctx.turn_ledger.status = if finalized.is_interrupt {
        TurnLedgerStatus::Interrupted
    } else {
        TurnLedgerStatus::Answered
    };
    ctx.turn_ledger.reason = normalize_turn_reason(canonical_reply_source);
    ctx.turn_ledger.outbound_source = normalize_turn_reason(outbound_source);
    ctx.turn_ledger.canonical_reply_source = normalize_turn_reason(canonical_reply_source);
    ctx.turn_ledger.reply_preview = normalize_turn_preview(&reply_content);
    ctx.turn_ledger.updated_at_ms = super::now_unix_ms();
    ctx.turn_ledger.finished_at_ms = ctx.turn_ledger.updated_at_ms;
    ctx.turn_ledger.react_rounds = finalized.worker_latency.react_rounds;
    ctx.turn_ledger.tool_calls = finalized.worker_latency.tool_calls;
    ctx.turn_ledger.any_tool_used = finalized.any_tool_used;
    ctx.turn_ledger.final_reply_delivered = delivered;
    ctx.turn_ledger.reply_handoff_ms = reply_handoff_ms.min(u64::MAX as u128) as u64;
    ctx.turn_ledger.post_reply_ms = post_reply_ms.min(u64::MAX as u128) as u64;
    ctx.turn_ledger.total_ms = total_ms.min(u64::MAX as u128) as u64;
    ctx.turn_ledger.ttft_ms = finalized
        .worker_latency
        .ttft_ms
        .unwrap_or(0)
        .min(u64::MAX as u128) as u64;
    ctx.turn_ledger.delivery = super::turn_finalize::build_turn_delivery_ledger(finalized.delivery);
    ctx.turn_ledger.observation = finalized.turn_observation.take();
    ctx.turn_ledger.subject_state = if ctx.msg.ingress == IngressKind::User {
        finalized
            .subject_state
            .as_ref()
            .and_then(build_turn_subject_state_ledger)
    } else {
        let relationship_id =
            crate::memory::relationship_scope_id(&ctx.msg.channel, &ctx.msg.chat_id);
        ctx.config
            .runtime
            .turn_ledger_store
            .get(&relationship_id)
            .ok()
            .flatten()
            .and_then(|ledger| ledger.subject_state)
    };
    ctx.turn_ledger.persona = if ctx.msg.ingress == IngressKind::User {
        super::worker_governance::build_turn_persona_ledger(
            finalized.pressure,
            finalized.worker_latency.tool_calls,
            delivered,
            finalized.is_interrupt,
            finalized.mental_privacy_adjudication.as_ref(),
            finalized.persona_priority_adjudication.as_ref(),
            &finalized.mental_privacy_review,
            finalized.mental_privacy_review.applied
                && finalized.mental_privacy_review.reply_content.trim()
                    != finalized.review_input_before.trim(),
        )
    } else {
        let relationship_id =
            crate::memory::relationship_scope_id(&ctx.msg.channel, &ctx.msg.chat_id);
        ctx.config
            .runtime
            .turn_ledger_store
            .get(&relationship_id)
            .ok()
            .flatten()
            .and_then(|ledger| ledger.persona)
    };
    ctx.turn_ledger.soul_feedback = if ctx.msg.ingress == IngressKind::User {
        finalized
            .soul_feedback_projection
            .as_ref()
            .and_then(build_turn_soul_feedback_ledger)
    } else {
        let relationship_id =
            crate::memory::relationship_scope_id(&ctx.msg.channel, &ctx.msg.chat_id);
        ctx.config
            .runtime
            .turn_ledger_store
            .get(&relationship_id)
            .ok()
            .flatten()
            .and_then(|ledger| ledger.soul_feedback)
    };
    ctx.turn_ledger.reasoning_intent = if ctx.msg.ingress == IngressKind::User {
        finalized
            .programmable_reasoning_intent
            .as_ref()
            .map(crate::agent::reasoning_intent::build_turn_reasoning_intent_ledger)
    } else {
        let relationship_id =
            crate::memory::relationship_scope_id(&ctx.msg.channel, &ctx.msg.chat_id);
        ctx.config
            .runtime
            .turn_ledger_store
            .get(&relationship_id)
            .ok()
            .flatten()
            .and_then(|ledger| ledger.reasoning_intent)
    };
    ctx.turn_ledger.counterfactual = if ctx.msg.ingress == IngressKind::User {
        finalized
            .counterfactual_analysis
            .as_ref()
            .map(crate::agent::counterfactual::build_turn_counterfactual_ledger)
    } else {
        let relationship_id =
            crate::memory::relationship_scope_id(&ctx.msg.channel, &ctx.msg.chat_id);
        ctx.config
            .runtime
            .turn_ledger_store
            .get(&relationship_id)
            .ok()
            .flatten()
            .and_then(|ledger| ledger.counterfactual)
    };
    ctx.turn_ledger.adversarial_arena = if ctx.msg.ingress == IngressKind::User {
        finalized
            .adversarial_arena_adjudication
            .as_ref()
            .map(crate::agent::adversarial_arena::build_turn_adversarial_arena_ledger)
    } else {
        let relationship_id =
            crate::memory::relationship_scope_id(&ctx.msg.channel, &ctx.msg.chat_id);
        ctx.config
            .runtime
            .turn_ledger_store
            .get(&relationship_id)
            .ok()
            .flatten()
            .and_then(|ledger| ledger.adversarial_arena)
    };
    super::turn_finalize::persist_turn_ledger(
        ctx.config.runtime.turn_ledger_store.as_ref(),
        &crate::memory::relationship_scope_id(&ctx.msg.channel, &ctx.msg.chat_id),
        &ctx.turn_ledger,
        "finish",
    );
    super::turn_finalize::persist_turn_continuity_evidence(
        ctx.config.runtime.turn_continuity_evidence_store.as_ref(),
        &crate::memory::relationship_scope_id(&ctx.msg.channel, &ctx.msg.chat_id),
        &ctx.turn_ledger,
        "finish",
    );
    if let Some(adjudication) = finalized.adversarial_arena_adjudication.as_ref() {
        crate::reasoning::append_adversarial_arena_event(
            crate::agent::adversarial_arena::build_turn_adversarial_arena_timeline_event(
                adjudication,
                ctx.turn_ledger.finished_at_ms / 1000,
            ),
        );
    }
    metrics::record_react_rounds(finalized.worker_latency.react_rounds);
    metrics::record_tool_calls_last(finalized.worker_latency.tool_calls);
    metrics::record_request_semantics_ms(finalized.worker_latency.request_semantics_ms);
    metrics::record_tool_exec_ms(finalized.worker_latency.tool_exec_ms);
    metrics::record_mental_privacy_review_ms(finalized.worker_latency.mental_privacy_review_ms);
    metrics::record_ttft_ms(finalized.worker_latency.ttft_ms.unwrap_or(0));
    metrics::record_e2e_ms(reply_handoff_ms);
    metrics::record_post_reply_ms(post_reply_ms);
    if ctx.msg.ingress == IngressKind::System {
        let is_cron = ctx.msg.channel.as_ref() == CHANNEL_CRON;
        if is_cron {
            let cron_e2e = super::now_unix_ms().saturating_sub(ctx.msg.enqueue_ts_ms) as u128;
            metrics::record_cron_e2e_ms(cron_e2e);
        }
    }
    super::log_agent_latency_summary(
        ctx.worker_lane_tag,
        ctx.msg.req_id.as_deref().unwrap_or_default(),
        ctx.msg.channel.as_ref(),
        ctx.msg.chat_id.as_ref(),
        ctx.queue_wait_ms,
        ctx.admission_ms,
        ctx.worker_prepare_ms,
        &finalized.worker_latency,
        llm_ms,
        outbound_enqueue_ms,
        reply_handoff_ms,
        post_reply_ms,
        total_ms,
        finalized.streamed,
        delivered,
        ctx.latency_warn_ms,
    );
}
