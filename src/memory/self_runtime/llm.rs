use super::*;

fn compose_self_runtime_grounding_body(parts: &[Option<&str>]) -> Option<String> {
    let mut out = String::new();
    for part in parts.iter().flatten() {
        let trimmed = part.trim();
        if trimmed.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push_str("\n\n");
        }
        out.push_str(trimmed);
    }
    (!out.is_empty()).then_some(out)
}

fn append_self_runtime_grounding_section(out: &mut String, title: &str, parts: &[Option<&str>]) {
    let Some(body) = compose_self_runtime_grounding_body(parts) else {
        return;
    };
    let _ = writeln!(out, "\n{}\n{}\n", title, body);
}

#[allow(clippy::too_many_arguments)]
pub(super) fn decide_self_runtime(
    http: &mut dyn LlmHttpClient,
    llm: &(dyn LlmClient + Send + Sync),
    session_store: &dyn SessionStore,
    _long_term_memory_store: &dyn LongTermMemoryStore,
    memory_store: &dyn MemoryStore,
    turn_ledger_store: &dyn TurnLedgerStore,
    chat_id: &str,
    payload: &SelfRuntimeJobPayload,
    summary_text: Option<&str>,
    execution_state: Option<&crate::memory::ExecutionState>,
    self_model: Option<&crate::memory::SelfModel>,
    self_authored_core: Option<&crate::memory::SelfAuthoredCore>,
    core_revision_ledger: Option<&crate::memory::CoreRevisionLedger>,
    core_revision_governance: &CoreRevisionGovernanceDigest,
    private_docs: Option<&crate::memory::PrivateDocWorkspace>,
    private_garden_docs: &[crate::memory::PrivateGardenDocRecord],
    inner_life: Option<&crate::memory::InnerLife>,
    self_continuity: Option<&crate::memory::SelfContinuity>,
    _outer_voice: Option<&crate::memory::OuterVoice>,
    mental_privacy_state: Option<&crate::memory::MentalPrivacyState>,
    relationship_portfolio: Option<&RelationshipPortfolio>,
    relationship_topology: Option<&RelationshipTopology>,
    relationship_constitution: Option<&RelationshipConstitution>,
    current_relationship_scope_id: &str,
    recent_persona_evidence: Option<&crate::memory::RecentPersonaEvidence>,
    world_sense: Option<&crate::memory::WorldSense>,
    world_snapshot: &crate::memory::WorldSnapshot,
    autonomy_strategy: Option<&crate::memory::AutonomyStrategy>,
    profile: MemoryProfile,
    recent: &[crate::memory::SessionMessage],
    factual_snapshot: &SharedFactualPlaneSnapshot,
    boundary_signal: &SelfRuntimeBoundarySignal,
) -> Result<SelfRuntimeDecision> {
    let policy = memory_policy(profile).self_runtime;
    let query_hint = if !payload.user_content.trim().is_empty() {
        payload.user_content.trim()
    } else {
        payload.reply_content.trim()
    };
    let mut input = String::with_capacity(2048);
    let _ = writeln!(input, "Trigger: {:?}", payload.trigger);
    if !payload.source_channel.trim().is_empty() {
        let _ = writeln!(input, "Source channel: {}", payload.source_channel.trim());
    }
    if !payload.user_content.trim().is_empty() {
        let _ = writeln!(
            input,
            "Latest user: {}",
            scrub_credentials(
                truncate_content_to_max(
                    payload.user_content.trim(),
                    policy.transcript_preview_chars
                )
                .as_ref()
            )
        );
    }
    if !payload.reply_content.trim().is_empty() {
        let _ = writeln!(
            input,
            "Latest reply: {}",
            scrub_credentials(
                truncate_content_to_max(
                    payload.reply_content.trim(),
                    policy.transcript_preview_chars
                )
                .as_ref()
            )
        );
    }
    let summary_line = summary_text
        .filter(|s| !s.trim().is_empty())
        .map(|summary_text| {
            let summary = truncate_content_to_max(summary_text.trim(), policy.grounding_max_len);
            format!("Summary: {}", scrub_credentials(summary.as_ref()))
        });
    let execution_state_block = execution_state.and_then(|state| {
        render_execution_state_block(
            state,
            policy
                .grounding_max_len
                .min(memory_policy(profile).execution_state.render_max_len),
        )
    });
    let factual_snapshot_block = factual_snapshot.block.clone();
    let archive_evidence_block = build_archive_evidence_block(
        session_store,
        memory_store,
        turn_ledger_store,
        chat_id,
        query_hint,
        policy.grounding_max_len,
        profile,
    );
    let world_snapshot_block = render_world_snapshot_block(
        world_snapshot,
        memory_policy(profile).world_sense.snapshot_max_len,
    );
    let self_state_block = render_self_state_block(
        &build_self_state(
            self_model,
            private_docs,
            autonomy_strategy,
            inner_life,
            self_continuity,
            private_garden_docs,
            payload.now_secs,
            profile,
        ),
        memory_policy(profile).self_state.render_max_len,
    );
    let self_authored_core_block = self_authored_core
        .and_then(|core| render_persistent_self_authored_core_block(core, policy.grounding_max_len))
        .or_else(|| {
            render_self_authored_core_block(
                self_model,
                self_continuity,
                mental_privacy_state,
                policy.grounding_max_len,
            )
        });
    let core_revision_governance_block = core_revision_ledger.and_then(|ledger| {
        render_core_revision_governance_block(
            ledger,
            core_revision_governance,
            payload.now_secs,
            policy.grounding_max_len,
        )
    });
    let constitution_governance_line = (core_revision_governance.review_due
        || core_revision_governance.conservative_mode)
        .then(|| {
            format!(
                "Constitution governance: review_due={} conservative_mode={} pressure={} repeated_rejections={} corrections={} contradictions={}",
                core_revision_governance.review_due,
                core_revision_governance.conservative_mode,
                core_revision_governance.pressure_summary(),
                core_revision_governance.repeated_rejected_direction_count,
                core_revision_governance.recent_correction_count,
                core_revision_governance.contradiction_count
            )
        });
    let internal_memory_topology_block = render_internal_memory_topology_block(
        self_model,
        private_docs,
        private_garden_docs,
        payload.now_secs,
        profile,
        InternalMemoryLayerFocus::Router,
        policy.grounding_max_len,
    );
    let relationship_portfolio_block = relationship_portfolio.and_then(|portfolio| {
        render_relationship_portfolio_block(
            portfolio,
            payload.now_secs,
            Some(current_relationship_scope_id),
            policy.grounding_max_len,
        )
    });
    let relationship_topology_block = relationship_topology.and_then(|topology| {
        render_relationship_topology_block(
            topology,
            payload.now_secs,
            Some(current_relationship_scope_id),
            policy.grounding_max_len,
        )
    });
    let relationship_constitution_block = relationship_constitution.and_then(|constitution| {
        render_relationship_constitution_block(constitution, policy.grounding_max_len)
    });
    let world_sense_block = world_sense
        .and_then(|world_sense| render_world_sense_block(world_sense, policy.grounding_max_len));
    let autonomy_strategy_block = autonomy_strategy
        .and_then(|strategy| render_autonomy_strategy_block(strategy, policy.grounding_max_len));
    let private_memory_boundary_block = render_private_memory_boundary_block(
        "self_runtime",
        "governing private inward writes while keeping objective facts in the shared plane",
        policy.grounding_max_len,
    );
    let mental_privacy_boundary_block = render_mental_privacy_boundary_block(
        mental_privacy_state,
        &crate::memory::collect_private_targets(
            self_model,
            self_continuity,
            inner_life,
            private_docs,
            private_garden_docs,
        ),
        policy.grounding_max_len,
    );
    let recent_persona_evidence_block = recent_persona_evidence.and_then(|evidence| {
        render_recent_persona_evidence_block(evidence, policy.grounding_max_len)
    });
    let boundary_flush_line = boundary_signal
        .is_active()
        .then(|| format!("Boundary flush signal: {}", boundary_signal.human_summary()));
    let factual_refresh_summary_line = factual_snapshot
        .refresh_summary()
        .map(|summary| format!("Shared factual reconcile summary: {}", summary));
    let external_content_line = payload.external_content_used.then_some(
        "Latest turn used external content/tools that may have changed what deserves inward organization."
            .to_string(),
    );

    append_self_runtime_grounding_section(
        &mut input,
        "## Program Memory Grounding",
        &[
            summary_line.as_deref(),
            execution_state_block.as_deref(),
            factual_snapshot_block.as_deref(),
            archive_evidence_block.as_deref(),
            world_snapshot_block.as_deref(),
        ],
    );
    append_self_runtime_grounding_section(
        &mut input,
        "## Soul Growth Grounding",
        &[
            self_state_block.as_deref(),
            self_authored_core_block.as_deref(),
            core_revision_governance_block.as_deref(),
            constitution_governance_line.as_deref(),
            internal_memory_topology_block.as_deref(),
            relationship_portfolio_block.as_deref(),
            relationship_topology_block.as_deref(),
            relationship_constitution_block.as_deref(),
            world_sense_block.as_deref(),
            autonomy_strategy_block.as_deref(),
            private_memory_boundary_block.as_deref(),
            mental_privacy_boundary_block.as_deref(),
            recent_persona_evidence_block.as_deref(),
            boundary_flush_line.as_deref(),
            factual_refresh_summary_line.as_deref(),
            external_content_line.as_deref(),
        ],
    );
    input.push_str("Source ids you may reference for upward distillation: inner_life, private_docs, private_garden, self_model, self_authored_core, self_continuity, boundary_persona, outer_voice, world_sense, autonomy_strategy, recent_persona_evidence, relationship_constitution, recent_transcript.\n");
    input.push_str("Recent transcript:\n");
    for message in recent {
        let preview = truncate_content_to_max(&message.content, policy.transcript_preview_chars);
        let _ = writeln!(
            input,
            "- {}: {}",
            message.role,
            scrub_credentials(preview.as_ref())
        );
    }
    let messages = [Message {
        role: Cow::Borrowed("user"),
        content: input,
    }];
    let response = llm.chat(
        http,
        SELF_RUNTIME_SYSTEM_PROMPT,
        &messages,
        None,
        ToolChoicePolicy::Auto,
    )?;
    Ok(parse_self_runtime_decision(response.content.trim()))
}

pub(super) fn parse_self_runtime_decision(raw: &str) -> SelfRuntimeDecision {
    let LlmJsonPayload::Value(value) = parse_llm_json_payload(raw) else {
        return SelfRuntimeDecision::default();
    };
    let Some(object) = value.as_object() else {
        return SelfRuntimeDecision::default();
    };
    SelfRuntimeDecision {
        refresh_inner_life: get_object_bool(object, "refresh_inner_life").unwrap_or(false),
        inner_life_intent: get_object_text(object, "inner_life_intent"),
        refresh_private_docs: get_object_bool(object, "refresh_private_docs").unwrap_or(false),
        private_docs_intent: get_object_text(object, "private_docs_intent"),
        private_docs_action: SelfRuntimeGovernanceAction::from_text(&get_object_text(
            object,
            "private_docs_action",
        )),
        refresh_self_model: get_object_bool(object, "refresh_self_model").unwrap_or(false),
        self_model_intent: get_object_text(object, "self_model_intent"),
        self_model_sources: parse_runtime_sources(object, "self_model_sources"),
        refresh_self_authored_core: get_object_bool(object, "refresh_self_authored_core")
            .unwrap_or(false),
        self_authored_core_intent: get_object_text(object, "self_authored_core_intent"),
        self_authored_core_sources: parse_runtime_sources(object, "self_authored_core_sources"),
        refresh_self_continuity: get_object_bool(object, "refresh_self_continuity")
            .unwrap_or(false),
        self_continuity_intent: get_object_text(object, "self_continuity_intent"),
        self_continuity_sources: parse_runtime_sources(object, "self_continuity_sources"),
        refresh_private_garden: get_object_bool(object, "refresh_private_garden").unwrap_or(false),
        private_garden_intent: get_object_text(object, "private_garden_intent"),
        private_garden_action: SelfRuntimeGovernanceAction::from_text(&get_object_text(
            object,
            "private_garden_action",
        )),
        refresh_boundary_persona: get_object_bool(object, "refresh_boundary_persona")
            .unwrap_or(false),
        boundary_persona_intent: get_object_text(object, "boundary_persona_intent"),
        refresh_outer_voice: get_object_bool(object, "refresh_outer_voice").unwrap_or(false),
        outer_voice_intent: get_object_text(object, "outer_voice_intent"),
        outer_voice_sources: parse_runtime_sources(object, "outer_voice_sources"),
        boundary_flush: get_object_bool(object, "boundary_flush").unwrap_or(false),
        boundary_flush_reason: get_object_text(object, "boundary_flush_reason"),
        request_factual_refresh: get_object_bool(object, "request_factual_refresh")
            .unwrap_or(false),
        factual_reconcile_action: parse_shared_factual_reconcile_action(&get_object_text(
            object,
            "factual_reconcile_action",
        )),
        factual_reconcile_intent: get_object_text(object, "factual_reconcile_intent"),
    }
}

fn parse_runtime_sources(
    object: &serde_json::Map<String, serde_json::Value>,
    field: &str,
) -> Vec<String> {
    let mut sources = get_object_string_list(object, field)
        .into_iter()
        .filter_map(|source| normalize_runtime_source_id(&source))
        .collect::<Vec<_>>();
    sources.dedup();
    sources
}

fn parse_shared_factual_reconcile_action(value: &str) -> SharedFactualReconcileAction {
    match value.trim().to_ascii_lowercase().as_str() {
        "reinforce" => SharedFactualReconcileAction::Reinforce,
        "correct" => SharedFactualReconcileAction::Correct,
        "conflict" => SharedFactualReconcileAction::Conflict,
        "stale" => SharedFactualReconcileAction::Stale,
        _ => SharedFactualReconcileAction::Hold,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grounding_sections_keep_program_memory_and_soul_growth_separate() {
        let mut input = String::new();
        append_self_runtime_grounding_section(
            &mut input,
            "## Program Memory Grounding",
            &[
                Some("Summary: continue current task"),
                Some("## Execution State\nGoal: close loop"),
                Some("## Shared Factual Recall\n- owner_timezone => Asia/Shanghai"),
            ],
        );
        append_self_runtime_grounding_section(
            &mut input,
            "## Soul Growth Grounding",
            &[
                Some("## Self State\nCurrent mode: deliberate"),
                Some("## Recent Persona Evidence\n- pressure pattern: steady"),
                Some("## Mental Privacy Boundary\nKeep private garden inward"),
            ],
        );

        let program_idx = input.find("## Program Memory Grounding").unwrap();
        let soul_idx = input.find("## Soul Growth Grounding").unwrap();
        assert!(program_idx < soul_idx);
        assert!(input.contains("Summary: continue current task"));
        assert!(input.contains("## Execution State\nGoal: close loop"));
        assert!(input.contains("## Self State\nCurrent mode: deliberate"));
        let soul_slice = &input[soul_idx..];
        assert!(!soul_slice.contains("Summary: continue current task"));
    }
}
