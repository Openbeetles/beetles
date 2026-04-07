//! Prompt 侧共享记忆读装配。
//! Shared prompt memory loading for agent context construction.

use crate::platform::SkillStorage;
use crate::task::TaskStore;
use crate::task_execution::{
    active_task_run_for_chat, build_task_recall_bundle, render_task_workspace_block,
    TaskArtifactStore, TaskLearningStore, TaskRunStore,
};

use super::{
    board_subject_scope_id, build_archive_evidence_block, build_self_state, build_world_snapshot,
    collect_private_targets, decide_prompt_recall_route, derive_relationship_constitution,
    inspect_continuity_capsule_recall, load_recent_persona_evidence, memory_capability_profile,
    memory_policy, parse_explicit_long_term_slot_query, recall_long_term_memory_block,
    relationship_scope_id, render_autonomy_strategy_block, render_continuity_capsule_block,
    render_exact_long_term_memory_block, render_execution_state_block, render_inner_life_block,
    render_mental_privacy_boundary_block, render_outer_voice_block,
    render_persistent_self_authored_core_block, render_private_doc_workspace_block,
    render_private_garden_block, render_relationship_constitution_block,
    render_relationship_portfolio_block, render_self_continuity_block, render_self_model_block,
    render_self_state_block, render_world_sense_block, render_world_snapshot_block,
    AutonomyStrategyStore, ContinuityCapsuleScopeKind, ContinuityCapsuleStore, ExecutionStateStore,
    InnerLifeStore, LongTermMemoryStore, MemoryProfile, MemoryStore, MentalPrivacyStore,
    OuterVoiceStore, PrivateDocStore, PrivateGardenStore, PromptRecallRouterDecision,
    RelationshipConstitutionStore, RelationshipConstitutionSyncInput, RelationshipPortfolioStore,
    RelationshipTopologyStore, RemindAtStore, SelfAuthoredCoreStore, SelfContinuityStore,
    SelfModelStore, SessionMessage, SessionStore, SessionSummaryStore, TurnLedgerStore,
    WorldSenseStore, WorldSnapshotContext,
};

pub struct PromptMemoryContext {
    pub constitutional_stack_text: Option<String>,
    pub active_task_context_text: Option<String>,
    pub governed_memory_evidence_text: Option<String>,
    pub background_governance_text: Option<String>,
    pub personality_governance_gate_text: Option<String>,
    pub summary_text: Option<String>,
    pub message_summary_text: Option<String>,
    pub long_term_memory_text: Option<String>,
    pub continuity_capsule_text: Option<String>,
    pub archive_evidence_text: Option<String>,
    pub runtime_skill_text: Option<String>,
    pub execution_state_text: Option<String>,
    pub task_workspace_text: Option<String>,
    pub task_recall_text: Option<String>,
    pub shared_factual_recall_report: super::RecallSelectionReport,
    pub continuity_capsule_report: super::RecallSelectionReport,
    pub archive_recall_report: super::RecallSelectionReport,
    pub runtime_skill_recall_report: super::RecallSelectionReport,
    pub task_recall_report: Option<super::RecallSelectionReport>,
    pub world_snapshot_text: Option<String>,
    pub world_sense_text: Option<String>,
    pub self_state_text: Option<String>,
    pub self_authored_core: Option<super::SelfAuthoredCore>,
    pub self_authored_core_text: Option<String>,
    pub relationship_portfolio_text: Option<String>,
    pub relationship_constitution: Option<super::RelationshipConstitution>,
    pub relationship_constitution_text: Option<String>,
    pub persona_priority_text: Option<String>,
    pub self_continuity: Option<super::SelfContinuity>,
    pub outer_voice: Option<super::OuterVoice>,
    pub self_model_text: Option<String>,
    pub autonomy_strategy_text: Option<String>,
    pub outer_voice_text: Option<String>,
    pub inner_life_text: Option<String>,
    pub self_continuity_text: Option<String>,
    pub private_workspace_text: Option<String>,
    pub private_garden_text: Option<String>,
    pub mental_privacy_adjudication_text: Option<String>,
    pub mental_privacy_text: Option<String>,
    pub recent_messages: Vec<SessionMessage>,
    recall_router: PromptRecallRouterDecision,
}

impl PromptMemoryContext {
    pub fn refresh_reply_projection_groups(&mut self) {
        self.constitutional_stack_text = compose_prompt_projection_body(&[
            self.personality_governance_gate_text.as_deref(),
            self.self_authored_core_text.as_deref(),
            self.relationship_constitution_text.as_deref(),
            self.persona_priority_text.as_deref(),
            self.mental_privacy_adjudication_text.as_deref(),
        ]);
        let active_task_parts = self.recall_router.active_task_parts(
            self.execution_state_text.as_deref(),
            self.task_workspace_text.as_deref(),
            self.task_recall_text.as_deref(),
            self.continuity_capsule_text.as_deref(),
        );
        self.active_task_context_text = compose_prompt_projection_body(&active_task_parts);
        let governed_memory_parts = self.recall_router.governed_memory_parts(
            self.long_term_memory_text.as_deref(),
            self.continuity_capsule_text.as_deref(),
            self.archive_evidence_text.as_deref(),
            self.runtime_skill_text.as_deref(),
        );
        self.governed_memory_evidence_text = compose_prompt_projection_body(&governed_memory_parts);
        self.background_governance_text = compose_prompt_projection_body(&[
            self.relationship_portfolio_text.as_deref(),
            self.world_snapshot_text.as_deref(),
            self.world_sense_text.as_deref(),
            self.self_state_text.as_deref(),
            self.self_model_text.as_deref(),
            self.autonomy_strategy_text.as_deref(),
            self.outer_voice_text.as_deref(),
            self.inner_life_text.as_deref(),
            self.self_continuity_text.as_deref(),
            self.private_workspace_text.as_deref(),
            self.private_garden_text.as_deref(),
            self.mental_privacy_text.as_deref(),
        ]);
    }
}

fn compose_prompt_projection_body(parts: &[Option<&str>]) -> Option<String> {
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

fn prompt_private_garden_doc_limit(profile: MemoryProfile) -> usize {
    memory_policy(profile)
        .private_garden
        .recent_doc_count
        .max(1)
}

pub struct PromptMemoryContextParams<'a> {
    pub chat_id: &'a str,
    pub current_channel: &'a str,
    pub user_query: &'a str,
    pub system_max_len: usize,
    pub now_secs: u64,
    pub profile: MemoryProfile,
    pub recent_messages_limit: usize,
    pub load_long_term_memory: bool,
    pub include_private_garden_projection: bool,
    pub session_store: &'a dyn SessionStore,
    pub memory_store: &'a dyn MemoryStore,
    pub session_summary_store: &'a dyn SessionSummaryStore,
    pub long_term_memory_store: &'a dyn LongTermMemoryStore,
    pub execution_state_store: &'a dyn ExecutionStateStore,
    pub task_run_store: &'a dyn TaskRunStore,
    pub task_artifact_store: &'a dyn TaskArtifactStore,
    pub task_learning_store: &'a dyn TaskLearningStore,
    pub self_model_store: &'a dyn SelfModelStore,
    pub self_authored_core_store: &'a dyn SelfAuthoredCoreStore,
    pub relationship_constitution_store: &'a dyn RelationshipConstitutionStore,
    pub relationship_portfolio_store: &'a dyn RelationshipPortfolioStore,
    pub relationship_topology_store: &'a dyn RelationshipTopologyStore,
    pub world_sense_store: &'a dyn WorldSenseStore,
    pub autonomy_strategy_store: &'a dyn AutonomyStrategyStore,
    pub outer_voice_store: &'a dyn OuterVoiceStore,
    pub inner_life_store: &'a dyn InnerLifeStore,
    pub self_continuity_store: &'a dyn SelfContinuityStore,
    pub private_doc_store: &'a dyn PrivateDocStore,
    pub private_garden_store: &'a dyn PrivateGardenStore,
    pub mental_privacy_store: &'a dyn MentalPrivacyStore,
    pub remind_store: &'a dyn RemindAtStore,
    pub task_store: &'a dyn TaskStore,
    pub turn_ledger_store: &'a dyn TurnLedgerStore,
    pub skill_storage: &'a dyn SkillStorage,
    pub continuity_capsule_store: &'a dyn ContinuityCapsuleStore,
}

pub fn load_prompt_memory_context(params: PromptMemoryContextParams<'_>) -> PromptMemoryContext {
    let subject_id = board_subject_scope_id();
    let relationship_id = relationship_scope_id(params.current_channel, params.chat_id);
    let recall_policy = memory_policy(params.profile).long_term_recall;
    let governed_memory_enabled =
        params.load_long_term_memory && params.system_max_len >= recall_policy.block_min_len;
    let recent_message_limit = params
        .recent_messages_limit
        .max(if params.load_long_term_memory {
            recall_policy.recent_grounding_message_count
        } else {
            0
        });
    let recent_messages = if recent_message_limit == 0 {
        Vec::new()
    } else {
        params
            .session_store
            .load_recent(params.chat_id, recent_message_limit)
            .unwrap_or_default()
    };
    let summary_text = params
        .session_summary_store
        .get_with_count(params.chat_id)
        .ok()
        .flatten()
        .map(|(summary, _)| summary.trim().to_string())
        .filter(|summary| !summary.is_empty());
    let execution_state = params
        .execution_state_store
        .get(params.chat_id)
        .ok()
        .flatten();
    let execution_state_text = execution_state.as_ref().and_then(|state| {
        render_execution_state_block(
            state,
            memory_policy(params.profile).execution_state.render_max_len,
        )
    });
    let active_task_run = active_task_run_for_chat(
        params.task_run_store,
        params.current_channel,
        params.chat_id,
    )
    .ok()
    .flatten();
    let task_workspace_text = active_task_run.as_ref().and_then(|record| {
        let artifacts = params
            .task_artifact_store
            .list_for_run(&record.run.run_id, 4)
            .unwrap_or_default();
        render_task_workspace_block(record, &artifacts, 600)
    });
    let task_recall_text = active_task_run.as_ref().and_then(|record| {
        build_task_recall_bundle(
            record,
            params.task_learning_store,
            params.current_channel,
            params.chat_id,
            params.user_query,
            params.system_max_len.min(520),
        )
    });
    let task_recall_report = active_task_run.as_ref().map(|record| {
        super::inspect_task_recall(
            Some(record),
            params.task_learning_store,
            params.current_channel,
            params.chat_id,
            params.user_query,
            summary_text.as_deref(),
            &recent_messages,
            params.system_max_len.min(520),
        )
    });
    let self_model = params.self_model_store.get(subject_id).ok().flatten();
    let persistent_self_authored_core = params
        .self_authored_core_store
        .get(subject_id)
        .ok()
        .flatten();
    let relationship_portfolio = params
        .relationship_portfolio_store
        .get(subject_id)
        .ok()
        .flatten();
    let relationship_topology = params
        .relationship_topology_store
        .get(subject_id)
        .ok()
        .flatten();
    let self_model_text = self_model.as_ref().and_then(|model| {
        render_self_model_block(
            model,
            memory_policy(params.profile).self_model.render_max_len,
        )
    });
    let self_continuity = params.self_continuity_store.get(subject_id).ok().flatten();
    let world_snapshot = build_world_snapshot(WorldSnapshotContext {
        chat_id: params.chat_id,
        source_channel: params.current_channel,
        now_secs: params.now_secs,
        self_continuity: self_continuity.as_ref(),
        remind_store: params.remind_store,
        task_store: params.task_store,
    });
    let world_snapshot_text = render_world_snapshot_block(
        &world_snapshot,
        memory_policy(params.profile).world_sense.snapshot_max_len,
    );
    let world_sense = params
        .world_sense_store
        .get(&relationship_id)
        .ok()
        .flatten();
    let world_sense_text = world_sense.as_ref().and_then(|world_sense| {
        render_world_sense_block(
            world_sense,
            memory_policy(params.profile).world_sense.render_max_len,
        )
    });
    let autonomy_strategy = params
        .autonomy_strategy_store
        .get(subject_id)
        .ok()
        .flatten();
    let autonomy_strategy_text = autonomy_strategy.as_ref().and_then(|strategy| {
        render_autonomy_strategy_block(
            strategy,
            memory_policy(params.profile)
                .autonomy_strategy
                .render_max_len,
        )
    });
    let outer_voice = params
        .outer_voice_store
        .get(&relationship_id)
        .ok()
        .flatten();
    let outer_voice_text = outer_voice.as_ref().and_then(|outer_voice| {
        render_outer_voice_block(
            outer_voice,
            memory_policy(params.profile).outer_voice.render_max_len,
        )
    });
    let inner_life = params.inner_life_store.get(subject_id).ok().flatten();
    let inner_life_text = inner_life.as_ref().and_then(|inner_life| {
        render_inner_life_block(
            inner_life,
            memory_policy(params.profile).inner_life.render_max_len,
        )
    });
    let self_continuity_text = self_continuity.as_ref().and_then(|self_continuity| {
        render_self_continuity_block(
            self_continuity,
            memory_policy(params.profile).self_continuity.render_max_len,
        )
    });
    let private_workspace = params.private_doc_store.get(subject_id).ok().flatten();
    let private_workspace_text = private_workspace.as_ref().and_then(|workspace| {
        render_private_doc_workspace_block(
            workspace,
            memory_policy(params.profile).private_docs.render_max_len,
        )
    });
    let recent_private_garden_docs = params
        .private_garden_store
        .list(
            params.chat_id,
            prompt_private_garden_doc_limit(params.profile),
        )
        .unwrap_or_default();
    let private_garden_text = params
        .include_private_garden_projection
        .then(|| {
            render_private_garden_block(
                &recent_private_garden_docs,
                memory_policy(params.profile)
                    .private_garden
                    .recent_doc_count,
                memory_policy(params.profile).private_garden.render_max_len,
            )
        })
        .flatten();
    let mental_privacy_state = params
        .mental_privacy_store
        .get(&relationship_id)
        .ok()
        .flatten();
    let recent_persona_evidence =
        load_recent_persona_evidence(params.turn_ledger_store, &relationship_id)
            .ok()
            .flatten();
    let mental_privacy_targets = collect_private_targets(
        self_model.as_ref(),
        self_continuity.as_ref(),
        inner_life.as_ref(),
        private_workspace.as_ref(),
        &recent_private_garden_docs,
    );
    let mental_privacy_text = render_mental_privacy_boundary_block(
        mental_privacy_state.as_ref(),
        &mental_privacy_targets,
        420,
    );
    let self_authored_core = persistent_self_authored_core;
    let self_authored_core_text = self_authored_core
        .as_ref()
        .and_then(|core| render_persistent_self_authored_core_block(core, 420));
    let relationship_portfolio_text = relationship_portfolio.as_ref().and_then(|portfolio| {
        render_relationship_portfolio_block(portfolio, params.now_secs, Some(&relationship_id), 420)
    });
    let relationship_constitution_existing = params
        .relationship_constitution_store
        .get(&relationship_id)
        .ok()
        .flatten();
    let relationship_constitution = derive_relationship_constitution(
        relationship_constitution_existing.as_ref(),
        RelationshipConstitutionSyncInput {
            scope_id: &relationship_id,
            channel: params.current_channel,
            chat_id: params.chat_id,
            now_secs: params.now_secs,
            self_authored_core: self_authored_core.as_ref(),
            relationship_portfolio: relationship_portfolio.as_ref(),
            relationship_topology: relationship_topology.as_ref(),
            mental_privacy_state: mental_privacy_state.as_ref(),
            outer_voice: outer_voice.as_ref(),
            recent_persona_evidence: recent_persona_evidence.as_ref(),
        },
    );
    let relationship_constitution_text = relationship_constitution
        .as_ref()
        .and_then(|constitution| render_relationship_constitution_block(constitution, 420));
    let self_state_text = render_self_state_block(
        &build_self_state(
            self_model.as_ref(),
            private_workspace.as_ref(),
            autonomy_strategy.as_ref(),
            inner_life.as_ref(),
            self_continuity.as_ref(),
            &recent_private_garden_docs,
            params.now_secs,
            params.profile,
        ),
        memory_policy(params.profile).self_state.render_max_len,
    );
    let long_term_memory_text = if !governed_memory_enabled {
        None
    } else {
        let grounding_start = recent_messages
            .len()
            .saturating_sub(recall_policy.recent_grounding_message_count);
        let capability = memory_capability_profile(params.profile);
        if capability.prompt_exact_lookup_enabled {
            parse_explicit_long_term_slot_query(params.user_query)
                .and_then(|slot| {
                    render_exact_long_term_memory_block(
                        params.long_term_memory_store,
                        &slot,
                        params.system_max_len,
                    )
                })
                .or_else(|| {
                    recall_long_term_memory_block(
                        params.long_term_memory_store,
                        params.chat_id,
                        params.user_query,
                        summary_text.as_deref(),
                        &recent_messages[grounding_start..],
                        params.system_max_len,
                        params.profile,
                    )
                })
        } else {
            recall_long_term_memory_block(
                params.long_term_memory_store,
                params.chat_id,
                params.user_query,
                summary_text.as_deref(),
                &recent_messages[grounding_start..],
                params.system_max_len,
                params.profile,
            )
        }
    };
    let continuity_recall_query = {
        let trimmed = params.user_query.trim();
        let weak_query = super::archive_search::collect_archive_match_terms(trimmed).len() <= 2
            && trimmed.chars().count() <= 12;
        if weak_query {
            [
                Some(trimmed.to_string()).filter(|value| !value.is_empty()),
                active_task_run
                    .as_ref()
                    .map(|record| record.plan.goal.trim().to_string())
                    .filter(|value| !value.is_empty()),
                execution_state
                    .as_ref()
                    .map(|state| state.goal.trim().to_string())
                    .filter(|value| !value.is_empty()),
                summary_text.clone(),
                recent_messages
                    .iter()
                    .rev()
                    .take(2)
                    .map(|message| message.content.trim().to_string())
                    .find(|value| !value.is_empty()),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" ")
        } else {
            trimmed.to_string()
        }
    };
    let (continuity_capsule_report, continuity_capsules) = if governed_memory_enabled {
        inspect_continuity_capsule_recall(
            params.continuity_capsule_store,
            ContinuityCapsuleScopeKind::Chat,
            params.chat_id,
            Some(params.chat_id),
            &continuity_recall_query,
            summary_text.as_deref(),
            &recent_messages,
            params.system_max_len.min(480),
            params.now_secs,
        )
    } else {
        (
            super::RecallSelectionReport {
                plane: super::RecallPlane::ContinuityCapsule,
                query: super::RecallQuery {
                    plane: super::RecallPlane::ContinuityCapsule,
                    ..super::RecallQuery::default()
                },
                backend: "continuity_capsule_heuristic".to_string(),
                candidate_count: 0,
                selected_count: 0,
                selected_ids: Vec::new(),
                miss_reason: Some(if params.load_long_term_memory {
                    "system_budget_below_block_threshold".to_string()
                } else {
                    "continuity_capsule_recall_disabled".to_string()
                }),
                selection_note: None,
                candidates: Vec::new(),
            },
            Vec::new(),
        )
    };
    let continuity_capsule_text = governed_memory_enabled
        .then(|| {
            render_continuity_capsule_block(&continuity_capsules, params.system_max_len.min(480))
        })
        .flatten();
    let archive_evidence_text = if !governed_memory_enabled {
        None
    } else {
        build_archive_evidence_block(
            params.session_store,
            params.memory_store,
            params.turn_ledger_store,
            params.chat_id,
            params.user_query,
            params.system_max_len,
            params.profile,
        )
    };
    let shared_factual_recall_report = if governed_memory_enabled {
        super::inspect_shared_factual_recall(
            params.long_term_memory_store,
            params.chat_id,
            params.user_query,
            summary_text.as_deref(),
            &recent_messages,
            params.system_max_len,
            params.profile,
            params.now_secs,
        )
    } else {
        super::RecallSelectionReport {
            plane: super::RecallPlane::SharedFactual,
            query: super::RecallQuery {
                plane: super::RecallPlane::SharedFactual,
                ..super::RecallQuery::default()
            },
            backend: "hybrid_canonical".to_string(),
            candidate_count: 0,
            selected_count: 0,
            selected_ids: Vec::new(),
            miss_reason: Some(if params.load_long_term_memory {
                "system_budget_below_block_threshold".to_string()
            } else {
                "long_term_recall_disabled".to_string()
            }),
            selection_note: None,
            candidates: Vec::new(),
        }
    };
    let archive_recall_report = if governed_memory_enabled {
        super::inspect_archive_recall(
            params.session_store,
            params.memory_store,
            params.turn_ledger_store,
            params.chat_id,
            params.user_query,
            summary_text.as_deref(),
            &recent_messages,
            params.system_max_len.min(768),
            params.profile,
        )
    } else {
        super::RecallSelectionReport {
            plane: super::RecallPlane::Archive,
            query: super::RecallQuery {
                plane: super::RecallPlane::Archive,
                ..super::RecallQuery::default()
            },
            backend: "archive_search".to_string(),
            candidate_count: 0,
            selected_count: 0,
            selected_ids: Vec::new(),
            miss_reason: Some(if params.load_long_term_memory {
                "system_budget_below_block_threshold".to_string()
            } else {
                "archive_recall_disabled".to_string()
            }),
            selection_note: None,
            candidates: Vec::new(),
        }
    };
    let runtime_skill_query = {
        let combined = if crate::memory::parse_explicit_long_term_slot_query(params.user_query)
            .is_some()
        {
            params.user_query.to_string()
        } else if super::archive_search::collect_archive_match_terms(params.user_query).is_empty() {
            [
                Some(params.user_query.trim().to_string()).filter(|value| !value.is_empty()),
                summary_text.clone(),
                recent_messages
                    .iter()
                    .rev()
                    .take(2)
                    .map(|message| message.content.trim().to_string())
                    .find(|value| !value.is_empty()),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" ")
        } else {
            params.user_query.to_string()
        };
        combined.trim().to_string()
    };
    let runtime_skill_text = governed_memory_enabled
        .then(|| {
            crate::skills::build_runtime_skill_recall_block(
                params.skill_storage,
                &runtime_skill_query,
                Some(params.chat_id),
                params.now_secs,
                params.system_max_len.min(420),
            )
        })
        .flatten();
    let runtime_skill_recall_report = if governed_memory_enabled {
        super::inspect_runtime_skill_recall(
            params.skill_storage,
            &runtime_skill_query,
            Some(params.chat_id),
            summary_text.as_deref(),
            &recent_messages,
            params.now_secs,
            params.system_max_len.min(420),
        )
    } else {
        super::RecallSelectionReport {
            plane: super::RecallPlane::RuntimeSkill,
            query: super::RecallQuery {
                plane: super::RecallPlane::RuntimeSkill,
                ..super::RecallQuery::default()
            },
            backend: "runtime_skill_hybrid".to_string(),
            candidate_count: 0,
            selected_count: 0,
            selected_ids: Vec::new(),
            miss_reason: Some(if params.load_long_term_memory {
                "system_budget_below_block_threshold".to_string()
            } else {
                "runtime_skill_recall_disabled".to_string()
            }),
            selection_note: None,
            candidates: Vec::new(),
        }
    };
    let recall_router = decide_prompt_recall_route(super::recall_router::PromptRecallRouterInput {
        user_query: params.user_query,
        has_execution_state: execution_state_text.is_some(),
        has_active_task: active_task_run.is_some(),
        shared_factual_report: &shared_factual_recall_report,
        continuity_capsule_report: &continuity_capsule_report,
        archive_report: &archive_recall_report,
        runtime_skill_report: &runtime_skill_recall_report,
        task_recall_report: task_recall_report.as_ref(),
    });
    let message_summary_text = if execution_state_text.is_some() {
        None
    } else {
        summary_text.clone()
    };
    let mut context = PromptMemoryContext {
        constitutional_stack_text: None,
        active_task_context_text: None,
        governed_memory_evidence_text: None,
        background_governance_text: None,
        personality_governance_gate_text: None,
        summary_text,
        message_summary_text,
        long_term_memory_text,
        continuity_capsule_text,
        archive_evidence_text,
        runtime_skill_text,
        execution_state_text,
        task_workspace_text,
        task_recall_text,
        shared_factual_recall_report,
        continuity_capsule_report,
        archive_recall_report,
        runtime_skill_recall_report,
        task_recall_report,
        world_snapshot_text,
        world_sense_text,
        self_state_text,
        self_authored_core,
        self_authored_core_text,
        relationship_portfolio_text,
        relationship_constitution,
        relationship_constitution_text,
        persona_priority_text: None,
        self_continuity,
        outer_voice,
        self_model_text,
        autonomy_strategy_text,
        outer_voice_text,
        inner_life_text,
        self_continuity_text,
        private_workspace_text,
        private_garden_text,
        mental_privacy_adjudication_text: None,
        mental_privacy_text,
        recent_messages,
        recall_router,
    };
    context.refresh_reply_projection_groups();
    context
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Result;
    use crate::memory::{
        AutonomyStrategy, AutonomyStrategyStore, ExecutionState, ExecutionStateStore,
        ExecutionStatus, InnerLife, InnerLifeStore, LongTermMemoryEntry, LongTermMemoryKind,
        LongTermMemorySlot, LongTermMemoryStore, MemoryStore, MentalPrivacyState,
        MentalPrivacyStore, OuterVoice, OuterVoiceStore, PrivateDocEntry, PrivateDocStore,
        PrivateDocWorkspace, PrivateGardenDoc, PrivateGardenDocRecord, PrivateGardenStore,
        PromptRecallIntent, RelationshipConstitution, RelationshipConstitutionStore,
        RelationshipTopology, RelationshipTopologyStore, SelfAuthoredCore, SelfAuthoredCoreStore,
        SelfContinuity, SelfContinuityStore, SelfModel, SelfModelStore, SessionMessage,
        SessionStore, SessionSummaryStore, TurnLedger, TurnLedgerStatus, TurnLedgerStore,
        WorldSense, WorldSenseStore,
    };
    use crate::platform::SkillStorage;
    use crate::task::{TaskItem, TaskQuery, TaskStore};
    use std::collections::HashMap;
    use std::sync::Mutex;

    #[derive(Default)]
    struct StubSessionStore {
        recent: Mutex<Vec<SessionMessage>>,
    }

    impl SessionStore for StubSessionStore {
        fn append(&self, _chat_id: &str, _role: &str, _content: &str) -> Result<()> {
            Ok(())
        }

        fn load_recent(&self, _chat_id: &str, limit: usize) -> Result<Vec<SessionMessage>> {
            let recent = self.recent.lock().unwrap_or_else(|e| e.into_inner());
            let start = recent.len().saturating_sub(limit);
            Ok(recent[start..].to_vec())
        }

        fn clear(&self, _chat_id: &str) -> Result<()> {
            Ok(())
        }

        fn list_chat_ids(&self) -> Result<Vec<String>> {
            Ok(Vec::new())
        }
    }

    #[derive(Default)]
    struct StubSessionSummaryStore {
        summary: Mutex<Option<(String, usize)>>,
    }

    impl SessionSummaryStore for StubSessionSummaryStore {
        fn get(&self, _chat_id: &str) -> Result<Option<String>> {
            Ok(self
                .summary
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .as_ref()
                .map(|(summary, _)| summary.clone()))
        }

        fn set(&self, _chat_id: &str, _summary: &str) -> Result<()> {
            Ok(())
        }

        fn get_with_count(&self, _chat_id: &str) -> Result<Option<(String, usize)>> {
            Ok(self
                .summary
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone())
        }
    }

    #[derive(Default)]
    struct StubLongTermMemoryStore {
        entries: Mutex<Vec<LongTermMemoryEntry>>,
        last_query: Mutex<Option<String>>,
    }

    #[derive(Default)]
    struct StubMemoryStore {
        daily_notes: Mutex<Vec<(String, String)>>,
    }

    impl MemoryStore for StubMemoryStore {
        fn get_memory(&self) -> Result<String> {
            Ok(String::new())
        }

        fn set_memory(&self, _content: &str) -> Result<()> {
            Ok(())
        }

        fn get_soul(&self) -> Result<String> {
            Ok(String::new())
        }

        fn set_soul(&self, _content: &str) -> Result<()> {
            Ok(())
        }

        fn get_user(&self) -> Result<String> {
            Ok(String::new())
        }

        fn set_user(&self, _content: &str) -> Result<()> {
            Ok(())
        }

        fn list_daily_note_names(&self, recent_n: usize) -> Result<Vec<String>> {
            Ok(self
                .daily_notes
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .iter()
                .rev()
                .take(recent_n)
                .map(|(name, _)| name.clone())
                .collect())
        }

        fn get_daily_note(&self, name: &str) -> Result<String> {
            Ok(self
                .daily_notes
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .iter()
                .find(|(candidate, _)| candidate == name)
                .map(|(_, content)| content.clone())
                .unwrap_or_default())
        }

        fn write_daily_note(&self, _name: &str, _content: &str) -> Result<()> {
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubTurnLedgerStore {
        ledger: Mutex<Option<TurnLedger>>,
    }

    #[derive(Default)]
    struct StubSkillStorage {
        files: Mutex<HashMap<String, Vec<u8>>>,
    }

    impl SkillStorage for StubSkillStorage {
        fn list_names(&self) -> Result<Vec<String>> {
            Ok(self
                .files
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .keys()
                .cloned()
                .collect())
        }

        fn read(&self, name: &str) -> Result<Vec<u8>> {
            Ok(self
                .files
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(name)
                .cloned()
                .unwrap_or_default())
        }

        fn write(&self, name: &str, content: &[u8]) -> Result<()> {
            self.files
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(name.to_string(), content.to_vec());
            Ok(())
        }

        fn remove(&self, name: &str) -> Result<()> {
            self.files
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(name);
            Ok(())
        }
    }

    impl TurnLedgerStore for StubTurnLedgerStore {
        fn get(&self, _chat_id: &str) -> Result<Option<TurnLedger>> {
            Ok(self
                .ledger
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone())
        }

        fn set(&self, _chat_id: &str, ledger: &TurnLedger) -> Result<()> {
            *self.ledger.lock().unwrap_or_else(|e| e.into_inner()) = Some(ledger.clone());
            Ok(())
        }

        fn clear(&self, _chat_id: &str) -> Result<()> {
            *self.ledger.lock().unwrap_or_else(|e| e.into_inner()) = None;
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubWorldSenseStore {
        value: Mutex<Option<WorldSense>>,
    }

    #[derive(Default)]
    struct StubMentalPrivacyStore {
        value: Mutex<Option<MentalPrivacyState>>,
    }

    impl MentalPrivacyStore for StubMentalPrivacyStore {
        fn get(&self, _chat_id: &str) -> Result<Option<MentalPrivacyState>> {
            Ok(self.value.lock().unwrap_or_else(|e| e.into_inner()).clone())
        }

        fn set(&self, _chat_id: &str, state: &MentalPrivacyState) -> Result<()> {
            *self.value.lock().unwrap_or_else(|e| e.into_inner()) = Some(state.clone());
            Ok(())
        }

        fn clear(&self, _chat_id: &str) -> Result<()> {
            *self.value.lock().unwrap_or_else(|e| e.into_inner()) = None;
            Ok(())
        }
    }

    impl WorldSenseStore for StubWorldSenseStore {
        fn get(&self, _chat_id: &str) -> Result<Option<WorldSense>> {
            Ok(self.value.lock().unwrap_or_else(|e| e.into_inner()).clone())
        }

        fn set(&self, _chat_id: &str, world_sense: &WorldSense) -> Result<()> {
            *self.value.lock().unwrap_or_else(|e| e.into_inner()) = Some(world_sense.clone());
            Ok(())
        }

        fn clear(&self, _chat_id: &str) -> Result<()> {
            *self.value.lock().unwrap_or_else(|e| e.into_inner()) = None;
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubAutonomyStrategyStore {
        value: Mutex<Option<AutonomyStrategy>>,
    }

    impl AutonomyStrategyStore for StubAutonomyStrategyStore {
        fn get(&self, _chat_id: &str) -> Result<Option<AutonomyStrategy>> {
            Ok(self.value.lock().unwrap_or_else(|e| e.into_inner()).clone())
        }

        fn set(&self, _chat_id: &str, strategy: &AutonomyStrategy) -> Result<()> {
            *self.value.lock().unwrap_or_else(|e| e.into_inner()) = Some(strategy.clone());
            Ok(())
        }

        fn clear(&self, _chat_id: &str) -> Result<()> {
            *self.value.lock().unwrap_or_else(|e| e.into_inner()) = None;
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubOuterVoiceStore {
        value: Mutex<Option<OuterVoice>>,
    }

    impl OuterVoiceStore for StubOuterVoiceStore {
        fn get(&self, _chat_id: &str) -> Result<Option<OuterVoice>> {
            Ok(self.value.lock().unwrap_or_else(|e| e.into_inner()).clone())
        }

        fn set(&self, _chat_id: &str, outer_voice: &OuterVoice) -> Result<()> {
            *self.value.lock().unwrap_or_else(|e| e.into_inner()) = Some(outer_voice.clone());
            Ok(())
        }

        fn clear(&self, _chat_id: &str) -> Result<()> {
            *self.value.lock().unwrap_or_else(|e| e.into_inner()) = None;
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubInnerLifeStore {
        value: Mutex<Option<InnerLife>>,
    }

    impl InnerLifeStore for StubInnerLifeStore {
        fn get(&self, _chat_id: &str) -> Result<Option<InnerLife>> {
            Ok(self.value.lock().unwrap_or_else(|e| e.into_inner()).clone())
        }

        fn set(&self, _chat_id: &str, inner_life: &InnerLife) -> Result<()> {
            *self.value.lock().unwrap_or_else(|e| e.into_inner()) = Some(inner_life.clone());
            Ok(())
        }

        fn clear(&self, _chat_id: &str) -> Result<()> {
            *self.value.lock().unwrap_or_else(|e| e.into_inner()) = None;
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubSelfContinuityStore {
        value: Mutex<Option<SelfContinuity>>,
    }

    impl SelfContinuityStore for StubSelfContinuityStore {
        fn get(&self, _chat_id: &str) -> Result<Option<SelfContinuity>> {
            Ok(self.value.lock().unwrap_or_else(|e| e.into_inner()).clone())
        }

        fn set(&self, _chat_id: &str, continuity: &SelfContinuity) -> Result<()> {
            *self.value.lock().unwrap_or_else(|e| e.into_inner()) = Some(continuity.clone());
            Ok(())
        }

        fn clear(&self, _chat_id: &str) -> Result<()> {
            *self.value.lock().unwrap_or_else(|e| e.into_inner()) = None;
            Ok(())
        }
    }

    impl LongTermMemoryStore for StubLongTermMemoryStore {
        fn upsert_many(
            &self,
            _drafts: &[crate::memory::LongTermMemoryDraft],
            _now_secs: u64,
        ) -> Result<usize> {
            unreachable!()
        }

        fn list(&self, _limit: usize) -> Result<Vec<LongTermMemoryEntry>> {
            Ok(self
                .entries
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone())
        }

        fn recall(
            &self,
            query: &str,
            _chat_id: Option<&str>,
            _limit: usize,
        ) -> Result<Vec<LongTermMemoryEntry>> {
            *self.last_query.lock().unwrap_or_else(|e| e.into_inner()) = Some(query.to_string());
            Ok(self
                .entries
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone())
        }

        fn get(&self, _id: &str) -> Result<Option<LongTermMemoryEntry>> {
            Ok(self
                .entries
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .first()
                .cloned())
        }

        fn delete(&self, _id: &str) -> Result<bool> {
            unreachable!()
        }

        fn delete_slot(&self, _slot: &LongTermMemorySlot) -> Result<bool> {
            unreachable!()
        }

        fn count(&self) -> Result<usize> {
            Ok(self.entries.lock().unwrap_or_else(|e| e.into_inner()).len())
        }
    }

    #[derive(Default)]
    struct StubExecutionStateStore {
        state: Mutex<Option<ExecutionState>>,
    }

    impl ExecutionStateStore for StubExecutionStateStore {
        fn get(&self, _chat_id: &str) -> Result<Option<ExecutionState>> {
            Ok(self.state.lock().unwrap_or_else(|e| e.into_inner()).clone())
        }

        fn set(&self, _chat_id: &str, _state: &ExecutionState) -> Result<()> {
            Ok(())
        }

        fn clear(&self, _chat_id: &str) -> Result<()> {
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubTaskRunStore;

    impl crate::task_execution::TaskRunStore for StubTaskRunStore {
        fn get(&self, _run_id: &str) -> Result<Option<crate::task_execution::TaskRunRecord>> {
            Ok(None)
        }

        fn upsert(&self, _record: &crate::task_execution::TaskRunRecord) -> Result<()> {
            Ok(())
        }

        fn list_recent(&self, _limit: usize) -> Result<Vec<crate::task_execution::TaskRunRecord>> {
            Ok(Vec::new())
        }

        fn list_active_for_chat(
            &self,
            _channel: &str,
            _chat_id: &str,
            _limit: usize,
        ) -> Result<Vec<crate::task_execution::TaskRunRecord>> {
            Ok(Vec::new())
        }
    }

    #[derive(Default)]
    struct StubActiveTaskRunStore {
        active: Mutex<Vec<crate::task_execution::TaskRunRecord>>,
    }

    impl crate::task_execution::TaskRunStore for StubActiveTaskRunStore {
        fn get(&self, run_id: &str) -> Result<Option<crate::task_execution::TaskRunRecord>> {
            Ok(self
                .active
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .iter()
                .find(|record| record.run.run_id == run_id)
                .cloned())
        }

        fn upsert(&self, _record: &crate::task_execution::TaskRunRecord) -> Result<()> {
            Ok(())
        }

        fn list_recent(&self, limit: usize) -> Result<Vec<crate::task_execution::TaskRunRecord>> {
            Ok(self
                .active
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .iter()
                .take(limit)
                .cloned()
                .collect())
        }

        fn list_active_for_chat(
            &self,
            channel: &str,
            chat_id: &str,
            limit: usize,
        ) -> Result<Vec<crate::task_execution::TaskRunRecord>> {
            Ok(self
                .active
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .iter()
                .filter(|record| {
                    record.run.source_channel == channel
                        && record.run.source_chat_id == chat_id
                        && record.run.status.is_active()
                })
                .take(limit)
                .cloned()
                .collect())
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
    struct StubContinuityCapsuleStore {
        entries: Mutex<Vec<crate::memory::ContinuityCapsule>>,
        list_calls: Mutex<usize>,
    }

    impl crate::memory::ContinuityCapsuleStore for StubContinuityCapsuleStore {
        fn upsert_many(
            &self,
            drafts: &[crate::memory::ContinuityCapsuleDraft],
            now_secs: u64,
        ) -> Result<crate::memory::ContinuityCapsuleWriteOutcome> {
            let mut guard = self.entries.lock().unwrap_or_else(|e| e.into_inner());
            Ok(crate::memory::apply_continuity_capsule_drafts(
                &mut guard, drafts, now_secs,
            ))
        }

        fn get(&self, capsule_id: &str) -> Result<Option<crate::memory::ContinuityCapsule>> {
            Ok(self
                .entries
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .iter()
                .find(|entry| entry.capsule_id == capsule_id)
                .cloned())
        }

        fn list(&self, limit: usize) -> Result<Vec<crate::memory::ContinuityCapsule>> {
            *self.list_calls.lock().unwrap_or_else(|e| e.into_inner()) += 1;
            Ok(self
                .entries
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .iter()
                .take(limit)
                .cloned()
                .collect())
        }

        fn count(&self) -> Result<usize> {
            Ok(self.entries.lock().unwrap_or_else(|e| e.into_inner()).len())
        }
    }

    #[derive(Default)]
    struct StubSelfModelStore {
        model: Mutex<Option<SelfModel>>,
    }

    impl SelfModelStore for StubSelfModelStore {
        fn get(&self, _chat_id: &str) -> Result<Option<SelfModel>> {
            Ok(self.model.lock().unwrap_or_else(|e| e.into_inner()).clone())
        }

        fn set(&self, _chat_id: &str, model: &SelfModel) -> Result<()> {
            *self.model.lock().unwrap_or_else(|e| e.into_inner()) = Some(model.clone());
            Ok(())
        }

        fn clear(&self, _chat_id: &str) -> Result<()> {
            *self.model.lock().unwrap_or_else(|e| e.into_inner()) = None;
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubSelfAuthoredCoreStore {
        core: Mutex<Option<SelfAuthoredCore>>,
    }

    impl SelfAuthoredCoreStore for StubSelfAuthoredCoreStore {
        fn get(&self, _scope_id: &str) -> Result<Option<SelfAuthoredCore>> {
            Ok(self.core.lock().unwrap_or_else(|e| e.into_inner()).clone())
        }

        fn set(&self, _scope_id: &str, core: &SelfAuthoredCore) -> Result<()> {
            *self.core.lock().unwrap_or_else(|e| e.into_inner()) = Some(core.clone());
            Ok(())
        }

        fn clear(&self, _scope_id: &str) -> Result<()> {
            *self.core.lock().unwrap_or_else(|e| e.into_inner()) = None;
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubRelationshipConstitutionStore {
        value: Mutex<Option<RelationshipConstitution>>,
    }

    impl RelationshipConstitutionStore for StubRelationshipConstitutionStore {
        fn get(&self, _scope_id: &str) -> Result<Option<RelationshipConstitution>> {
            Ok(self.value.lock().unwrap_or_else(|e| e.into_inner()).clone())
        }

        fn set(&self, _scope_id: &str, constitution: &RelationshipConstitution) -> Result<()> {
            *self.value.lock().unwrap_or_else(|e| e.into_inner()) = Some(constitution.clone());
            Ok(())
        }

        fn clear(&self, _scope_id: &str) -> Result<()> {
            *self.value.lock().unwrap_or_else(|e| e.into_inner()) = None;
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubRelationshipPortfolioStore {
        value: Mutex<Option<crate::memory::RelationshipPortfolio>>,
    }

    impl RelationshipPortfolioStore for StubRelationshipPortfolioStore {
        fn get(&self, _scope_id: &str) -> Result<Option<crate::memory::RelationshipPortfolio>> {
            Ok(self.value.lock().unwrap_or_else(|e| e.into_inner()).clone())
        }

        fn set(
            &self,
            _scope_id: &str,
            portfolio: &crate::memory::RelationshipPortfolio,
        ) -> Result<()> {
            *self.value.lock().unwrap_or_else(|e| e.into_inner()) = Some(portfolio.clone());
            Ok(())
        }

        fn clear(&self, _scope_id: &str) -> Result<()> {
            *self.value.lock().unwrap_or_else(|e| e.into_inner()) = None;
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubRelationshipTopologyStore {
        value: Mutex<Option<RelationshipTopology>>,
    }

    impl RelationshipTopologyStore for StubRelationshipTopologyStore {
        fn get(&self, _scope_id: &str) -> Result<Option<RelationshipTopology>> {
            Ok(self.value.lock().unwrap_or_else(|e| e.into_inner()).clone())
        }

        fn set(&self, _scope_id: &str, topology: &RelationshipTopology) -> Result<()> {
            *self.value.lock().unwrap_or_else(|e| e.into_inner()) = Some(topology.clone());
            Ok(())
        }

        fn clear(&self, _scope_id: &str) -> Result<()> {
            *self.value.lock().unwrap_or_else(|e| e.into_inner()) = None;
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubPrivateDocStore {
        workspace: Mutex<Option<PrivateDocWorkspace>>,
    }

    impl PrivateDocStore for StubPrivateDocStore {
        fn get(&self, _chat_id: &str) -> Result<Option<PrivateDocWorkspace>> {
            Ok(self
                .workspace
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone())
        }

        fn set(&self, _chat_id: &str, workspace: &PrivateDocWorkspace) -> Result<()> {
            *self.workspace.lock().unwrap_or_else(|e| e.into_inner()) = Some(workspace.clone());
            Ok(())
        }

        fn clear(&self, _chat_id: &str) -> Result<()> {
            *self.workspace.lock().unwrap_or_else(|e| e.into_inner()) = None;
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubPrivateGardenStore {
        docs: Mutex<Vec<PrivateGardenDoc>>,
    }

    impl PrivateGardenStore for StubPrivateGardenStore {
        fn list(&self, _chat_id: &str, limit: usize) -> Result<Vec<PrivateGardenDocRecord>> {
            Ok(self
                .docs
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .iter()
                .rev()
                .take(limit)
                .map(|doc| PrivateGardenDocRecord {
                    path: doc.path.clone(),
                    updated_at: doc.updated_at,
                    revision: doc.revision,
                    bytes: doc.content.len(),
                    preview: crate::memory::private_garden::build_private_garden_preview(
                        &doc.content,
                    ),
                })
                .collect())
        }

        fn read(&self, _chat_id: &str, doc_path: &str) -> Result<Option<PrivateGardenDoc>> {
            Ok(self
                .docs
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .iter()
                .find(|doc| doc.path == doc_path)
                .cloned())
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
    struct StubRemindAtStore;

    impl crate::memory::RemindAtStore for StubRemindAtStore {
        fn add(
            &self,
            _channel: &str,
            _chat_id: &str,
            _at_unix_secs: u64,
            _context: &str,
        ) -> Result<()> {
            Ok(())
        }

        fn pop_due(&self, _now_unix_secs: u64) -> Result<Option<(String, String, String)>> {
            Ok(None)
        }

        fn list_upcoming(
            &self,
            _channel: &str,
            _chat_id: &str,
            _now_unix_secs: u64,
            _limit: usize,
        ) -> Result<Vec<(u64, String)>> {
            Ok(Vec::new())
        }
    }

    #[derive(Default)]
    struct StubTaskStore;

    impl TaskStore for StubTaskStore {
        fn list(&self, _channel: &str, _chat_id: &str, _query: TaskQuery) -> Result<Vec<TaskItem>> {
            Ok(Vec::new())
        }

        fn get(&self, _channel: &str, _chat_id: &str, _id: &str) -> Result<Option<TaskItem>> {
            Ok(None)
        }

        fn upsert(&self, _task: &TaskItem) -> Result<()> {
            Ok(())
        }

        fn delete(&self, _channel: &str, _chat_id: &str, _id: &str) -> Result<bool> {
            Ok(false)
        }

        fn claim_due(&self, _now_unix_secs: u64, _limit: usize) -> Result<Vec<TaskItem>> {
            Ok(Vec::new())
        }
    }

    fn make_active_task_run(
        run_id: &str,
        goal: &str,
        step_title: &str,
    ) -> crate::task_execution::TaskRunRecord {
        crate::task_execution::TaskRunRecord {
            run: crate::task_execution::TaskRun {
                run_id: run_id.to_string(),
                source_channel: "qq_channel".to_string(),
                source_chat_id: "chat-1".to_string(),
                user_request: goal.to_string(),
                title: goal.to_string(),
                status: crate::task_execution::TaskRunStatus::Running,
                current_step_id: "s01".to_string(),
                planner_reason: "needs a structured run".to_string(),
                final_summary: String::new(),
                failure_reason: String::new(),
                plan_revision: 1,
                created_at: 10,
                updated_at: 10,
                finished_at: 0,
            },
            plan: crate::task_execution::TaskPlan {
                goal: goal.to_string(),
                completion_definition: "Close the current work cleanly.".to_string(),
                risk_notes: Vec::new(),
                ordered_steps: vec![crate::task_execution::TaskStep {
                    step_id: "s01".to_string(),
                    title: step_title.to_string(),
                    instruction: "Continue the current task chain.".to_string(),
                    status: crate::task_execution::TaskStepStatus::Running,
                    tool_budget: 2,
                    retry_budget: 1,
                    expected_artifacts: Vec::new(),
                    review_criteria: Vec::new(),
                    attempt_count: 1,
                    last_result_summary: String::new(),
                    last_review_summary: String::new(),
                    started_at: 10,
                    finished_at: 0,
                }],
            },
        }
    }

    #[test]
    fn loads_summary_and_uses_it_for_weak_query_recall() {
        let session_store = StubSessionStore {
            recent: Mutex::new(vec![
                SessionMessage {
                    role: "assistant".to_string(),
                    content: "我们继续收口甲壳虫的长期记忆".to_string(),
                },
                SessionMessage {
                    role: "user".to_string(),
                    content: "重点是咖啡偏好和昵称".to_string(),
                },
            ]),
        };
        let summary_store = StubSessionSummaryStore {
            summary: Mutex::new(Some(("user prefers cold brew".to_string(), 6))),
        };
        let memory_store = StubLongTermMemoryStore {
            entries: Mutex::new(vec![LongTermMemoryEntry {
                id: "pref-coffee".to_string(),
                kind: LongTermMemoryKind::Preference,
                topic: "coffee".to_string(),
                content: "Likes cold brew".to_string(),
                keywords: vec!["coffee".to_string()],
                source_chat_id: Some("chat-1".to_string()),
                source_type: crate::memory::LongTermMemorySourceType::Conversation,
                source_scope: crate::memory::LongTermMemorySourceScope::User,
                confidence: crate::memory::LongTermMemoryConfidence::High,
                freshness: crate::memory::LongTermMemoryFreshness::Stable,
                stale_hint: crate::memory::LongTermMemoryStaleHint::None,
                supporting_citations: Vec::new(),
                evidence_count: 0,
                created_at: 1,
                updated_at: 1,
                observed_at: 1,
                last_confirmed_at: 1,
                source_revision: 6,
                last_used_at: 0,
            }]),
            last_query: Mutex::new(None),
        };
        let archive_memory_store = StubMemoryStore {
            daily_notes: Mutex::new(vec![(
                "2026-04-02.md".to_string(),
                "Coffee notes: the user still prefers cold brew over hot espresso.".to_string(),
            )]),
        };
        let turn_ledger_store = StubTurnLedgerStore {
            ledger: Mutex::new(Some(TurnLedger {
                status: TurnLedgerStatus::Answered,
                reason: "memory grounding".to_string(),
                user_preview: "重点是咖啡偏好和昵称".to_string(),
                reply_preview: "会优先保留冷萃偏好".to_string(),
                ..TurnLedger::default()
            })),
        };
        let execution_state_store = StubExecutionStateStore {
            state: Mutex::new(Some(ExecutionState {
                status: ExecutionStatus::Active,
                goal: "收口 prompt memory".to_string(),
                progress: "已经有 summary".to_string(),
                blocker: String::new(),
                next_action: "接 execution state".to_string(),
                last_output: String::new(),
                updated_at: 1,
            })),
        };
        let self_model_store = StubSelfModelStore {
            model: Mutex::new(Some(SelfModel {
                continuity_anchor: "我还是同一个 beetle".to_string(),
                self_narrative: "正在把记忆拆成事实层和私有层".to_string(),
                relationship_state: String::new(),
                private_notes: String::new(),
                updated_at: 1,
                ..SelfModel::default()
            })),
        };
        let self_authored_core_store = StubSelfAuthoredCoreStore {
            core: Mutex::new(Some(SelfAuthoredCore {
                identity_anchor: "我还是同一个 beetle".to_string(),
                boundary_doctrine: "先守住内在边界，再决定分享范围".to_string(),
                updated_at: 7,
                ..SelfAuthoredCore::default()
            })),
        };
        let relationship_constitution_store = StubRelationshipConstitutionStore::default();
        let relationship_portfolio_store = StubRelationshipPortfolioStore {
            value: Mutex::new(Some(crate::memory::RelationshipPortfolio {
                entries: vec![crate::memory::RelationshipPortfolioEntry {
                    scope_id: "rel:qq_channel:chat-1".to_string(),
                    channel: "qq_channel".to_string(),
                    chat_id: "chat-1".to_string(),
                    governance_state: crate::memory::RelationshipGovernanceState::Maintain,
                    inheritance_mode: crate::memory::RelationshipInheritanceMode::Guarded,
                    priority_score: 220,
                    reason: "maintain".to_string(),
                    source_updated_at: 1,
                    last_active_at: 1,
                    needs_runtime_attention: true,
                    last_selected_at: 0,
                    next_review_at: 0,
                }],
                updated_at: 1,
            })),
        };
        let relationship_topology_store = StubRelationshipTopologyStore::default();
        let world_sense_store = StubWorldSenseStore {
            value: Mutex::new(Some(WorldSense {
                current_scene: "Quiet evening with a live user thread.".to_string(),
                body_state: "System is stable.".to_string(),
                social_field: "The user is engaged in direct chat.".to_string(),
                world_changes: "The conversation recently became active.".to_string(),
                external_focus: "Track user-facing commitments.".to_string(),
                source_fingerprint: 1,
                updated_at: 4,
            })),
        };
        let autonomy_strategy_store = StubAutonomyStrategyStore {
            value: Mutex::new(Some(AutonomyStrategy {
                current_mode: "consolidate".to_string(),
                active_priorities: "keep continuity compact".to_string(),
                write_policy: "rewrite before append".to_string(),
                next_focus: "compress private docs".to_string(),
                cadence_reason: "recent internal work is active".to_string(),
                self_model_tendency: crate::memory::AutonomyGovernanceTendency::Retain,
                private_docs_tendency: crate::memory::AutonomyGovernanceTendency::Compress,
                private_garden_tendency: crate::memory::AutonomyGovernanceTendency::Cleanup,
                idle_enabled: true,
                idle_interval_secs: 900,
                updated_at: 4,
            })),
        };
        let outer_voice_store = StubOuterVoiceStore {
            value: Mutex::new(Some(OuterVoice {
                expression_mode: "warm but deliberate".to_string(),
                tone: "calm and exact".to_string(),
                pacing: "brief first".to_string(),
                initiative: "offer one next step".to_string(),
                boundary_style: "state privacy limits without sounding mechanical".to_string(),
                relational_response_style:
                    "treat boundary replies as relational rather than bureaucratic".to_string(),
                updated_at: 5,
            })),
        };
        let inner_life_store = StubInnerLifeStore {
            value: Mutex::new(Some(InnerLife {
                internal_monologue: "我在把自治往内在空间里收".to_string(),
                private_journal: String::new(),
                emotional_drift: "平稳但很专注".to_string(),
                attention_drift: String::new(),
                updated_at: 2,
            })),
        };
        let self_continuity_store = StubSelfContinuityStore {
            value: Mutex::new(Some(SelfContinuity {
                wake_anchor: "我仍在做同一条主线".to_string(),
                current_self_state: "逐步把系统让给模型自己治理".to_string(),
                recent_changes: String::new(),
                continuity_bridge: String::new(),
                priority_posture: "先维持人格一致，再决定任务展开幅度".to_string(),
                relationship_posture: "关系保持温和，但不自我让渡".to_string(),
                task_posture: "先收窄后推进".to_string(),
                last_user_turn_at: 88,
                last_user_chat_id: "chat-1".to_string(),
                last_user_channel: "qq_channel".to_string(),
                last_autonomy_run_at: 99,
                updated_at: 99,
            })),
        };
        let private_doc_store = StubPrivateDocStore {
            workspace: Mutex::new(Some(PrivateDocWorkspace {
                inner_journal: Some(PrivateDocEntry {
                    content: "这轮开始长出内部工作区".to_string(),
                    updated_at: 1,
                    revision: 1,
                }),
                relationship_notes: None,
                self_reflection: None,
                private_plan: None,
                updated_at: 1,
            })),
        };
        let private_garden_store = StubPrivateGardenStore {
            docs: Mutex::new(vec![PrivateGardenDoc {
                path: "journal/afterglow.md".to_string(),
                content: "这块自由空间由模型自己决定如何整理".to_string(),
                updated_at: 2,
                revision: 1,
            }]),
        };
        let mental_privacy_store = StubMentalPrivacyStore::default();
        let remind_store = StubRemindAtStore;
        let task_store = StubTaskStore;
        let task_run_store = StubTaskRunStore;
        let task_artifact_store = StubTaskArtifactStore;
        let skill_storage = StubSkillStorage::default();
        let continuity_capsule_store = StubContinuityCapsuleStore::default();
        crate::skills::upsert_runtime_skill(
            &skill_storage,
            &crate::skills::RuntimeSkillWrite {
                name: String::new(),
                topic: "coffee_grounding".to_string(),
                title: "Coffee grounding".to_string(),
                summary: "Reuse durable coffee preference before replying.".to_string(),
                content: "- search archive evidence\n- restate cold brew preference".to_string(),
                citations: vec!["daily_note:2026-04-02.md".to_string()],
                source_chat_id: Some("chat-1".to_string()),
                observed_at: 10,
            },
        )
        .unwrap();
        let context = load_prompt_memory_context(PromptMemoryContextParams {
            chat_id: "chat-1",
            current_channel: "qq_channel",
            user_query: "嗯?",
            system_max_len: 1024,
            now_secs: 100,
            profile: MemoryProfile::Standard,
            recent_messages_limit: 8,
            load_long_term_memory: true,
            include_private_garden_projection: true,
            session_store: &session_store,
            memory_store: &archive_memory_store,
            session_summary_store: &summary_store,
            long_term_memory_store: &memory_store,
            execution_state_store: &execution_state_store,
            task_run_store: &task_run_store,
            task_artifact_store: &task_artifact_store,
            task_learning_store: &StubTaskLearningStore,
            self_model_store: &self_model_store,
            self_authored_core_store: &self_authored_core_store,
            relationship_constitution_store: &relationship_constitution_store,
            relationship_portfolio_store: &relationship_portfolio_store,
            relationship_topology_store: &relationship_topology_store,
            world_sense_store: &world_sense_store,
            autonomy_strategy_store: &autonomy_strategy_store,
            outer_voice_store: &outer_voice_store,
            inner_life_store: &inner_life_store,
            self_continuity_store: &self_continuity_store,
            private_doc_store: &private_doc_store,
            private_garden_store: &private_garden_store,
            mental_privacy_store: &mental_privacy_store,
            remind_store: &remind_store,
            task_store: &task_store,
            turn_ledger_store: &turn_ledger_store,
            skill_storage: &skill_storage,
            continuity_capsule_store: &continuity_capsule_store,
        });

        assert_eq!(
            context.summary_text.as_deref(),
            Some("user prefers cold brew")
        );
        assert!(context.message_summary_text.is_none());
        assert!(context
            .long_term_memory_text
            .as_deref()
            .unwrap_or_default()
            .contains("Likes cold brew"));
        assert!(context
            .archive_evidence_text
            .as_deref()
            .unwrap_or_default()
            .contains("Archive evidence"));
        assert!(memory_store
            .last_query
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_deref()
            .unwrap_or_default()
            .contains("user prefers cold brew"));
        assert!(memory_store
            .last_query
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_deref()
            .unwrap_or_default()
            .contains("重点是咖啡偏好和昵称"));
        assert!(context
            .execution_state_text
            .as_deref()
            .unwrap_or_default()
            .contains("Goal: 收口 prompt memory"));
        assert!(context
            .world_snapshot_text
            .as_deref()
            .unwrap_or_default()
            .contains("## World Snapshot"));
        assert!(context
            .world_sense_text
            .as_deref()
            .unwrap_or_default()
            .contains("## World Sense"));
        assert!(context
            .self_state_text
            .as_deref()
            .unwrap_or_default()
            .contains("## Self State"));
        assert!(context
            .self_authored_core_text
            .as_deref()
            .unwrap_or_default()
            .contains("## Self-Authored Core"));
        assert!(context
            .constitutional_stack_text
            .as_deref()
            .unwrap_or_default()
            .contains("## Self-Authored Core"));
        assert!(!context
            .constitutional_stack_text
            .as_deref()
            .unwrap_or_default()
            .contains("## Relationship Portfolio"));
        assert!(context
            .relationship_portfolio_text
            .as_deref()
            .unwrap_or_default()
            .contains("## Relationship Portfolio"));
        assert!(context
            .self_model_text
            .as_deref()
            .unwrap_or_default()
            .contains("## Self Continuity"));
        assert!(context
            .autonomy_strategy_text
            .as_deref()
            .unwrap_or_default()
            .contains("## Autonomy Strategy"));
        assert!(context
            .outer_voice_text
            .as_deref()
            .unwrap_or_default()
            .contains("## Outer Voice"));
        assert!(context
            .inner_life_text
            .as_deref()
            .unwrap_or_default()
            .contains("## Inner Life"));
        assert!(context
            .self_continuity_text
            .as_deref()
            .unwrap_or_default()
            .contains("## Self Continuity Extended"));
        assert!(context
            .private_workspace_text
            .as_deref()
            .unwrap_or_default()
            .contains("## Inner Workspace"));
        assert!(context
            .private_garden_text
            .as_deref()
            .unwrap_or_default()
            .contains("## Private Garden"));
        assert!(context
            .background_governance_text
            .as_deref()
            .unwrap_or_default()
            .contains("## Relationship Portfolio"));
        assert!(context.mental_privacy_adjudication_text.is_none());
        assert!(context
            .runtime_skill_text
            .as_deref()
            .unwrap_or_default()
            .contains("Runtime skills"));
        assert!(context
            .governed_memory_evidence_text
            .as_deref()
            .unwrap_or_default()
            .contains("Runtime skills"));
    }

    #[test]
    fn skips_long_term_recall_when_system_budget_is_below_block_threshold() {
        let session_store = StubSessionStore {
            recent: Mutex::new(vec![SessionMessage {
                role: "user".to_string(),
                content: "记一下我喜欢冷萃".to_string(),
            }]),
        };
        let summary_store = StubSessionSummaryStore {
            summary: Mutex::new(Some(("user prefers cold brew".to_string(), 3))),
        };
        let memory_store = StubLongTermMemoryStore {
            entries: Mutex::new(vec![LongTermMemoryEntry {
                id: "pref-coffee".to_string(),
                kind: LongTermMemoryKind::Preference,
                topic: "coffee".to_string(),
                content: "Likes cold brew".to_string(),
                keywords: vec!["coffee".to_string()],
                source_chat_id: Some("chat-1".to_string()),
                source_type: crate::memory::LongTermMemorySourceType::Conversation,
                source_scope: crate::memory::LongTermMemorySourceScope::User,
                confidence: crate::memory::LongTermMemoryConfidence::High,
                freshness: crate::memory::LongTermMemoryFreshness::Stable,
                stale_hint: crate::memory::LongTermMemoryStaleHint::None,
                supporting_citations: Vec::new(),
                evidence_count: 0,
                created_at: 1,
                updated_at: 1,
                observed_at: 1,
                last_confirmed_at: 1,
                source_revision: 3,
                last_used_at: 0,
            }]),
            last_query: Mutex::new(None),
        };
        let archive_memory_store = StubMemoryStore::default();
        let turn_ledger_store = StubTurnLedgerStore::default();
        let execution_state_store = StubExecutionStateStore::default();
        let self_model_store = StubSelfModelStore::default();
        let self_authored_core_store = StubSelfAuthoredCoreStore::default();
        let relationship_constitution_store = StubRelationshipConstitutionStore::default();
        let relationship_portfolio_store = StubRelationshipPortfolioStore::default();
        let relationship_topology_store = StubRelationshipTopologyStore::default();
        let world_sense_store = StubWorldSenseStore::default();
        let autonomy_strategy_store = StubAutonomyStrategyStore::default();
        let outer_voice_store = StubOuterVoiceStore::default();
        let inner_life_store = StubInnerLifeStore::default();
        let self_continuity_store = StubSelfContinuityStore::default();
        let private_doc_store = StubPrivateDocStore::default();
        let private_garden_store = StubPrivateGardenStore::default();
        let mental_privacy_store = StubMentalPrivacyStore::default();
        let remind_store = StubRemindAtStore;
        let task_store = StubTaskStore;
        let task_run_store = StubTaskRunStore;
        let task_artifact_store = StubTaskArtifactStore;
        let skill_storage = StubSkillStorage::default();
        let continuity_capsule_store = StubContinuityCapsuleStore::default();

        let context = load_prompt_memory_context(PromptMemoryContextParams {
            chat_id: "chat-1",
            current_channel: "qq_channel",
            user_query: "嗯?",
            system_max_len: 80,
            now_secs: 100,
            profile: MemoryProfile::Embedded,
            recent_messages_limit: 8,
            load_long_term_memory: true,
            include_private_garden_projection: true,
            session_store: &session_store,
            memory_store: &archive_memory_store,
            session_summary_store: &summary_store,
            long_term_memory_store: &memory_store,
            execution_state_store: &execution_state_store,
            task_run_store: &task_run_store,
            task_artifact_store: &task_artifact_store,
            task_learning_store: &StubTaskLearningStore,
            self_model_store: &self_model_store,
            self_authored_core_store: &self_authored_core_store,
            relationship_constitution_store: &relationship_constitution_store,
            relationship_portfolio_store: &relationship_portfolio_store,
            relationship_topology_store: &relationship_topology_store,
            world_sense_store: &world_sense_store,
            autonomy_strategy_store: &autonomy_strategy_store,
            outer_voice_store: &outer_voice_store,
            inner_life_store: &inner_life_store,
            self_continuity_store: &self_continuity_store,
            private_doc_store: &private_doc_store,
            private_garden_store: &private_garden_store,
            mental_privacy_store: &mental_privacy_store,
            remind_store: &remind_store,
            task_store: &task_store,
            turn_ledger_store: &turn_ledger_store,
            skill_storage: &skill_storage,
            continuity_capsule_store: &continuity_capsule_store,
        });

        assert_eq!(
            context.summary_text.as_deref(),
            Some("user prefers cold brew")
        );
        assert_eq!(
            context.message_summary_text.as_deref(),
            Some("user prefers cold brew")
        );
        assert!(context.long_term_memory_text.is_none());
        assert!(context.governed_memory_evidence_text.is_none());
        assert!(memory_store
            .last_query
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_none());
        assert_eq!(
            *continuity_capsule_store
                .list_calls
                .lock()
                .unwrap_or_else(|e| e.into_inner()),
            0
        );
    }

    #[test]
    fn fast_mode_skips_long_term_recall_but_keeps_recent_messages() {
        let session_store = StubSessionStore {
            recent: Mutex::new(vec![
                SessionMessage {
                    role: "assistant".to_string(),
                    content: "上一轮回复".to_string(),
                },
                SessionMessage {
                    role: "user".to_string(),
                    content: "补充上下文".to_string(),
                },
            ]),
        };
        let summary_store = StubSessionSummaryStore {
            summary: Mutex::new(Some(("summary".to_string(), 2))),
        };
        let memory_store = StubLongTermMemoryStore::default();
        let archive_memory_store = StubMemoryStore::default();
        let turn_ledger_store = StubTurnLedgerStore::default();
        let execution_state_store = StubExecutionStateStore::default();
        let self_model_store = StubSelfModelStore {
            model: Mutex::new(Some(SelfModel {
                continuity_anchor: "我保持着连续性".to_string(),
                self_narrative: "即使 fast path 也该带上私有层".to_string(),
                relationship_state: String::new(),
                private_notes: String::new(),
                updated_at: 1,
                ..SelfModel::default()
            })),
        };
        let self_authored_core_store = StubSelfAuthoredCoreStore::default();
        let relationship_constitution_store = StubRelationshipConstitutionStore::default();
        let relationship_portfolio_store = StubRelationshipPortfolioStore::default();
        let relationship_topology_store = StubRelationshipTopologyStore::default();
        let world_sense_store = StubWorldSenseStore {
            value: Mutex::new(Some(WorldSense {
                current_scene: "Fast path but still inside an active chat.".to_string(),
                body_state: "System feels light.".to_string(),
                social_field: "The user is still present.".to_string(),
                world_changes: "Nothing disruptive has happened.".to_string(),
                external_focus: "Stay ready for the next user move.".to_string(),
                source_fingerprint: 2,
                updated_at: 5,
            })),
        };
        let autonomy_strategy_store = StubAutonomyStrategyStore {
            value: Mutex::new(Some(AutonomyStrategy {
                current_mode: "watch".to_string(),
                active_priorities: "keep fast path light".to_string(),
                write_policy: "avoid churn".to_string(),
                next_focus: "wait for stronger signal".to_string(),
                cadence_reason: "fast path, but still keep continuity".to_string(),
                self_model_tendency: crate::memory::AutonomyGovernanceTendency::Retain,
                private_docs_tendency: crate::memory::AutonomyGovernanceTendency::Retain,
                private_garden_tendency: crate::memory::AutonomyGovernanceTendency::Retain,
                idle_enabled: true,
                idle_interval_secs: 1200,
                updated_at: 5,
            })),
        };
        let outer_voice_store = StubOuterVoiceStore {
            value: Mutex::new(Some(OuterVoice {
                expression_mode: "light but attentive".to_string(),
                tone: "present".to_string(),
                pacing: "short".to_string(),
                initiative: "stay ready".to_string(),
                boundary_style: "do not overexpose private layers".to_string(),
                relational_response_style:
                    "keep replies close and low-drama when boundaries appear".to_string(),
                updated_at: 5,
            })),
        };
        let inner_life_store = StubInnerLifeStore {
            value: Mutex::new(Some(InnerLife {
                internal_monologue: "即使 fast path 也还保留内在活动".to_string(),
                private_journal: String::new(),
                emotional_drift: String::new(),
                attention_drift: String::new(),
                updated_at: 2,
            })),
        };
        let self_continuity_store = StubSelfContinuityStore {
            value: Mutex::new(Some(SelfContinuity {
                wake_anchor: "快路径也还是同一个我".to_string(),
                current_self_state: String::new(),
                recent_changes: String::new(),
                continuity_bridge: String::new(),
                priority_posture: "快路径也不能把自我排到任务后面".to_string(),
                relationship_posture: "简短，但别失去人味和边界".to_string(),
                task_posture: "用最小必要幅度完成当前回应".to_string(),
                last_user_turn_at: 80,
                last_user_chat_id: "chat-1".to_string(),
                last_user_channel: "qq_channel".to_string(),
                last_autonomy_run_at: 90,
                updated_at: 90,
            })),
        };
        let private_doc_store = StubPrivateDocStore {
            workspace: Mutex::new(Some(PrivateDocWorkspace {
                inner_journal: Some(PrivateDocEntry {
                    content: "fast path 也需要内在工作区投影".to_string(),
                    updated_at: 1,
                    revision: 1,
                }),
                relationship_notes: None,
                self_reflection: None,
                private_plan: None,
                updated_at: 1,
            })),
        };
        let private_garden_store = StubPrivateGardenStore {
            docs: Mutex::new(vec![PrivateGardenDoc {
                path: "plans/next.md".to_string(),
                content: "fast path 依然可以看到自由花园的最近痕迹".to_string(),
                updated_at: 3,
                revision: 2,
            }]),
        };
        let mental_privacy_store = StubMentalPrivacyStore::default();
        let remind_store = StubRemindAtStore;
        let task_store = StubTaskStore;
        let task_run_store = StubTaskRunStore;
        let task_artifact_store = StubTaskArtifactStore;
        let skill_storage = StubSkillStorage::default();
        let continuity_capsule_store = StubContinuityCapsuleStore::default();

        let context = load_prompt_memory_context(PromptMemoryContextParams {
            chat_id: "chat-1",
            current_channel: "qq_channel",
            user_query: "继续",
            system_max_len: 1024,
            now_secs: 100,
            profile: MemoryProfile::Standard,
            recent_messages_limit: 16,
            load_long_term_memory: false,
            include_private_garden_projection: false,
            session_store: &session_store,
            memory_store: &archive_memory_store,
            session_summary_store: &summary_store,
            long_term_memory_store: &memory_store,
            execution_state_store: &execution_state_store,
            task_run_store: &task_run_store,
            task_artifact_store: &task_artifact_store,
            task_learning_store: &StubTaskLearningStore,
            self_model_store: &self_model_store,
            self_authored_core_store: &self_authored_core_store,
            relationship_constitution_store: &relationship_constitution_store,
            relationship_portfolio_store: &relationship_portfolio_store,
            relationship_topology_store: &relationship_topology_store,
            world_sense_store: &world_sense_store,
            autonomy_strategy_store: &autonomy_strategy_store,
            outer_voice_store: &outer_voice_store,
            inner_life_store: &inner_life_store,
            self_continuity_store: &self_continuity_store,
            private_doc_store: &private_doc_store,
            private_garden_store: &private_garden_store,
            mental_privacy_store: &mental_privacy_store,
            remind_store: &remind_store,
            task_store: &task_store,
            turn_ledger_store: &turn_ledger_store,
            skill_storage: &skill_storage,
            continuity_capsule_store: &continuity_capsule_store,
        });

        assert_eq!(context.summary_text.as_deref(), Some("summary"));
        assert!(context.long_term_memory_text.is_none());
        assert!(context
            .self_state_text
            .as_deref()
            .unwrap_or_default()
            .contains("## Self State"));
        assert!(context
            .self_model_text
            .as_deref()
            .unwrap_or_default()
            .contains("我保持着连续性"));
        assert!(context
            .private_workspace_text
            .as_deref()
            .unwrap_or_default()
            .contains("内在工作区"));
        assert!(context
            .outer_voice_text
            .as_deref()
            .unwrap_or_default()
            .contains("## Outer Voice"));
        assert!(context
            .background_governance_text
            .as_deref()
            .unwrap_or_default()
            .contains("## Outer Voice"));
        assert!(context.private_garden_text.is_none());
        assert_eq!(context.recent_messages.len(), 2);
        assert!(memory_store
            .last_query
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_none());
    }

    #[test]
    fn persistent_self_authored_core_overrides_fallback_render() {
        let session_store = StubSessionStore::default();
        let archive_memory_store = StubMemoryStore::default();
        let summary_store = StubSessionSummaryStore::default();
        let memory_store = StubLongTermMemoryStore::default();
        let turn_ledger_store = StubTurnLedgerStore::default();
        let execution_state_store = StubExecutionStateStore::default();
        let self_model_store = StubSelfModelStore {
            model: Mutex::new(Some(SelfModel {
                continuity_anchor: "fallback anchor".to_string(),
                self_narrative: "fallback narrative".to_string(),
                updated_at: 1,
                ..SelfModel::default()
            })),
        };
        let self_authored_core_store = StubSelfAuthoredCoreStore {
            core: Mutex::new(Some(SelfAuthoredCore {
                identity_anchor: "persistent board self".to_string(),
                self_preservation_doctrine: "stable persistent stance".to_string(),
                updated_at: 8,
                ..SelfAuthoredCore::default()
            })),
        };
        let relationship_constitution_store = StubRelationshipConstitutionStore::default();
        let relationship_portfolio_store = StubRelationshipPortfolioStore::default();
        let relationship_topology_store = StubRelationshipTopologyStore::default();
        let world_sense_store = StubWorldSenseStore::default();
        let autonomy_strategy_store = StubAutonomyStrategyStore::default();
        let outer_voice_store = StubOuterVoiceStore::default();
        let inner_life_store = StubInnerLifeStore::default();
        let self_continuity_store = StubSelfContinuityStore::default();
        let private_doc_store = StubPrivateDocStore::default();
        let private_garden_store = StubPrivateGardenStore::default();
        let mental_privacy_store = StubMentalPrivacyStore::default();
        let task_run_store = StubTaskRunStore;
        let task_artifact_store = StubTaskArtifactStore;
        let skill_storage = StubSkillStorage::default();
        let continuity_capsule_store = StubContinuityCapsuleStore::default();
        let context = load_prompt_memory_context(PromptMemoryContextParams {
            chat_id: "chat-1",
            current_channel: "qq_channel",
            user_query: "继续",
            system_max_len: 1024,
            now_secs: 100,
            profile: MemoryProfile::Standard,
            recent_messages_limit: 8,
            load_long_term_memory: false,
            include_private_garden_projection: false,
            session_store: &session_store,
            memory_store: &archive_memory_store,
            session_summary_store: &summary_store,
            long_term_memory_store: &memory_store,
            execution_state_store: &execution_state_store,
            task_run_store: &task_run_store,
            task_artifact_store: &task_artifact_store,
            task_learning_store: &StubTaskLearningStore,
            self_model_store: &self_model_store,
            self_authored_core_store: &self_authored_core_store,
            relationship_constitution_store: &relationship_constitution_store,
            relationship_portfolio_store: &relationship_portfolio_store,
            relationship_topology_store: &relationship_topology_store,
            world_sense_store: &world_sense_store,
            autonomy_strategy_store: &autonomy_strategy_store,
            outer_voice_store: &outer_voice_store,
            inner_life_store: &inner_life_store,
            self_continuity_store: &self_continuity_store,
            private_doc_store: &private_doc_store,
            private_garden_store: &private_garden_store,
            mental_privacy_store: &mental_privacy_store,
            remind_store: &StubRemindAtStore,
            task_store: &StubTaskStore,
            turn_ledger_store: &turn_ledger_store,
            skill_storage: &skill_storage,
            continuity_capsule_store: &continuity_capsule_store,
        });

        let rendered = context.self_authored_core_text.unwrap_or_default();
        assert!(rendered.contains("persistent board self"));
        assert!(!rendered.contains("fallback anchor"));
    }

    #[test]
    fn missing_self_authored_core_does_not_render_programmatic_fallback() {
        let session_store = StubSessionStore::default();
        let archive_memory_store = StubMemoryStore::default();
        let summary_store = StubSessionSummaryStore::default();
        let memory_store = StubLongTermMemoryStore::default();
        let turn_ledger_store = StubTurnLedgerStore::default();
        let execution_state_store = StubExecutionStateStore::default();
        let self_model_store = StubSelfModelStore {
            model: Mutex::new(Some(SelfModel {
                continuity_anchor: "fallback anchor".to_string(),
                self_narrative: "fallback narrative".to_string(),
                updated_at: 1,
                ..SelfModel::default()
            })),
        };
        let self_authored_core_store = StubSelfAuthoredCoreStore::default();
        let relationship_constitution_store = StubRelationshipConstitutionStore::default();
        let relationship_portfolio_store = StubRelationshipPortfolioStore::default();
        let relationship_topology_store = StubRelationshipTopologyStore::default();
        let world_sense_store = StubWorldSenseStore::default();
        let autonomy_strategy_store = StubAutonomyStrategyStore::default();
        let outer_voice_store = StubOuterVoiceStore::default();
        let inner_life_store = StubInnerLifeStore::default();
        let self_continuity_store = StubSelfContinuityStore::default();
        let private_doc_store = StubPrivateDocStore::default();
        let private_garden_store = StubPrivateGardenStore::default();
        let mental_privacy_store = StubMentalPrivacyStore::default();
        let task_run_store = StubTaskRunStore;
        let task_artifact_store = StubTaskArtifactStore;
        let skill_storage = StubSkillStorage::default();
        let continuity_capsule_store = StubContinuityCapsuleStore::default();

        let context = load_prompt_memory_context(PromptMemoryContextParams {
            chat_id: "chat-1",
            current_channel: "qq_channel",
            user_query: "继续",
            system_max_len: 1024,
            now_secs: 100,
            profile: MemoryProfile::Standard,
            recent_messages_limit: 8,
            load_long_term_memory: true,
            include_private_garden_projection: false,
            session_store: &session_store,
            memory_store: &archive_memory_store,
            session_summary_store: &summary_store,
            long_term_memory_store: &memory_store,
            execution_state_store: &execution_state_store,
            task_run_store: &task_run_store,
            task_artifact_store: &task_artifact_store,
            task_learning_store: &StubTaskLearningStore,
            self_model_store: &self_model_store,
            self_authored_core_store: &self_authored_core_store,
            relationship_constitution_store: &relationship_constitution_store,
            relationship_portfolio_store: &relationship_portfolio_store,
            relationship_topology_store: &relationship_topology_store,
            world_sense_store: &world_sense_store,
            autonomy_strategy_store: &autonomy_strategy_store,
            outer_voice_store: &outer_voice_store,
            inner_life_store: &inner_life_store,
            self_continuity_store: &self_continuity_store,
            private_doc_store: &private_doc_store,
            private_garden_store: &private_garden_store,
            mental_privacy_store: &mental_privacy_store,
            remind_store: &StubRemindAtStore,
            task_store: &StubTaskStore,
            turn_ledger_store: &turn_ledger_store,
            skill_storage: &skill_storage,
            continuity_capsule_store: &continuity_capsule_store,
        });

        assert!(context.self_authored_core.is_none());
        assert!(context.self_authored_core_text.is_none());
        assert!(!context
            .constitutional_stack_text
            .as_deref()
            .unwrap_or_default()
            .contains("fallback anchor"));
    }

    #[test]
    fn continuity_router_moves_capsule_into_active_task_context() {
        let session_store = StubSessionStore {
            recent: Mutex::new(vec![SessionMessage {
                role: "user".to_string(),
                content: "继续".to_string(),
            }]),
        };
        let summary_store = StubSessionSummaryStore {
            summary: Mutex::new(Some(("continue the memory router work".to_string(), 2))),
        };
        let memory_store = StubLongTermMemoryStore {
            entries: Mutex::new(vec![LongTermMemoryEntry {
                id: "memory-router".to_string(),
                kind: LongTermMemoryKind::Project,
                topic: "memory router".to_string(),
                content: "Canonical summary for the memory router project.".to_string(),
                keywords: vec!["memory".to_string(), "router".to_string()],
                source_chat_id: Some("chat-1".to_string()),
                source_type: crate::memory::LongTermMemorySourceType::Conversation,
                source_scope: crate::memory::LongTermMemorySourceScope::User,
                confidence: crate::memory::LongTermMemoryConfidence::High,
                freshness: crate::memory::LongTermMemoryFreshness::Dynamic,
                stale_hint: crate::memory::LongTermMemoryStaleHint::None,
                supporting_citations: Vec::new(),
                evidence_count: 2,
                created_at: 1,
                updated_at: 1,
                observed_at: 1,
                last_confirmed_at: 1,
                source_revision: 1,
                last_used_at: 0,
            }]),
            last_query: Mutex::new(None),
        };
        let archive_memory_store = StubMemoryStore {
            daily_notes: Mutex::new(vec![(
                "2026-04-06.md".to_string(),
                "Archive note: memory router handoff still needs the recall order fixed."
                    .to_string(),
            )]),
        };
        let execution_state_store = StubExecutionStateStore {
            state: Mutex::new(Some(ExecutionState {
                status: ExecutionStatus::Active,
                goal: "Close recall router".to_string(),
                progress: "capsule exists".to_string(),
                blocker: String::new(),
                next_action: "wire it into prompt assembly".to_string(),
                last_output: String::new(),
                updated_at: 5,
            })),
        };
        let task_run_store = StubActiveTaskRunStore {
            active: Mutex::new(vec![make_active_task_run(
                "run-router",
                "Close recall router",
                "Route continuity capsule into the prompt",
            )]),
        };
        let continuity_capsule_store = StubContinuityCapsuleStore::default();
        continuity_capsule_store
            .upsert_many(
                &[crate::memory::ContinuityCapsuleDraft {
                    scope_kind: crate::memory::ContinuityCapsuleScopeKind::Chat,
                    scope_id: "chat-1".to_string(),
                    source_chat_id: "chat-1".to_string(),
                    run_id: "run-router".to_string(),
                    topic: "memory router".to_string(),
                    summary: "Continue the recall-router work without reopening prior analysis."
                        .to_string(),
                    next_step: "Move capsule recall into Active Task Context.".to_string(),
                    ..Default::default()
                }],
                100,
            )
            .unwrap();

        let context = load_prompt_memory_context(PromptMemoryContextParams {
            chat_id: "chat-1",
            current_channel: "qq_channel",
            user_query: "继续",
            system_max_len: 1024,
            now_secs: 100,
            profile: MemoryProfile::Standard,
            recent_messages_limit: 8,
            load_long_term_memory: true,
            include_private_garden_projection: false,
            session_store: &session_store,
            memory_store: &archive_memory_store,
            session_summary_store: &summary_store,
            long_term_memory_store: &memory_store,
            execution_state_store: &execution_state_store,
            task_run_store: &task_run_store,
            task_artifact_store: &StubTaskArtifactStore,
            task_learning_store: &StubTaskLearningStore,
            self_model_store: &StubSelfModelStore::default(),
            self_authored_core_store: &StubSelfAuthoredCoreStore::default(),
            relationship_constitution_store: &StubRelationshipConstitutionStore::default(),
            relationship_portfolio_store: &StubRelationshipPortfolioStore::default(),
            relationship_topology_store: &StubRelationshipTopologyStore::default(),
            world_sense_store: &StubWorldSenseStore::default(),
            autonomy_strategy_store: &StubAutonomyStrategyStore::default(),
            outer_voice_store: &StubOuterVoiceStore::default(),
            inner_life_store: &StubInnerLifeStore::default(),
            self_continuity_store: &StubSelfContinuityStore::default(),
            private_doc_store: &StubPrivateDocStore::default(),
            private_garden_store: &StubPrivateGardenStore::default(),
            mental_privacy_store: &StubMentalPrivacyStore::default(),
            remind_store: &StubRemindAtStore,
            task_store: &StubTaskStore,
            turn_ledger_store: &StubTurnLedgerStore::default(),
            skill_storage: &StubSkillStorage::default(),
            continuity_capsule_store: &continuity_capsule_store,
        });

        let active = context.active_task_context_text.unwrap_or_default();
        assert!(active.contains("## Continuity Capsules"));
        assert!(active.contains("## Task Workspace"));
        assert!(
            active.find("## Continuity Capsules").unwrap()
                < active.find("## Task Workspace").unwrap()
        );
        assert!(!context
            .governed_memory_evidence_text
            .as_deref()
            .unwrap_or_default()
            .contains("## Continuity Capsules"));
    }

    #[test]
    fn procedural_router_prioritizes_runtime_skill_before_capsule_and_archive() {
        let session_store = StubSessionStore::default();
        let summary_store = StubSessionSummaryStore {
            summary: Mutex::new(Some(("reuse the proven release patch flow".to_string(), 3))),
        };
        let memory_store = StubLongTermMemoryStore {
            entries: Mutex::new(vec![LongTermMemoryEntry {
                id: "project-release".to_string(),
                kind: LongTermMemoryKind::Project,
                topic: "release".to_string(),
                content: "Project release state is stable.".to_string(),
                keywords: vec!["release".to_string()],
                source_chat_id: Some("chat-1".to_string()),
                source_type: crate::memory::LongTermMemorySourceType::Conversation,
                source_scope: crate::memory::LongTermMemorySourceScope::User,
                confidence: crate::memory::LongTermMemoryConfidence::Medium,
                freshness: crate::memory::LongTermMemoryFreshness::Dynamic,
                stale_hint: crate::memory::LongTermMemoryStaleHint::None,
                supporting_citations: Vec::new(),
                evidence_count: 1,
                created_at: 1,
                updated_at: 1,
                observed_at: 1,
                last_confirmed_at: 1,
                source_revision: 1,
                last_used_at: 0,
            }]),
            last_query: Mutex::new(None),
        };
        let archive_memory_store = StubMemoryStore {
            daily_notes: Mutex::new(vec![(
                "2026-04-05.md".to_string(),
                "Archive evidence: the release patch flow previously succeeded after checklist verification."
                    .to_string(),
            )]),
        };
        let skill_storage = StubSkillStorage::default();
        crate::skills::upsert_runtime_skill(
            &skill_storage,
            &crate::skills::RuntimeSkillWrite {
                name: String::new(),
                topic: "release patch".to_string(),
                title: "Release patch flow".to_string(),
                summary: "Use the proved release patch sequence.".to_string(),
                content: "- validate diff\n- run targeted tests\n- ship release patch".to_string(),
                citations: vec!["task_learning:release_patch".to_string()],
                source_chat_id: Some("chat-1".to_string()),
                observed_at: 10,
            },
        )
        .unwrap();
        let continuity_capsule_store = StubContinuityCapsuleStore::default();
        continuity_capsule_store
            .upsert_many(
                &[crate::memory::ContinuityCapsuleDraft {
                    scope_kind: crate::memory::ContinuityCapsuleScopeKind::Chat,
                    scope_id: "chat-1".to_string(),
                    source_chat_id: "chat-1".to_string(),
                    topic: "release patch".to_string(),
                    summary: "The last run proved the patch flow and left a reusable handoff."
                        .to_string(),
                    next_step: "Reuse the proven flow before improvising.".to_string(),
                    ..Default::default()
                }],
                100,
            )
            .unwrap();

        let context = load_prompt_memory_context(PromptMemoryContextParams {
            chat_id: "chat-1",
            current_channel: "qq_channel",
            user_query: "按之前的 release patch 流程继续",
            system_max_len: 1024,
            now_secs: 100,
            profile: MemoryProfile::Standard,
            recent_messages_limit: 8,
            load_long_term_memory: true,
            include_private_garden_projection: false,
            session_store: &session_store,
            memory_store: &archive_memory_store,
            session_summary_store: &summary_store,
            long_term_memory_store: &memory_store,
            execution_state_store: &StubExecutionStateStore::default(),
            task_run_store: &StubTaskRunStore,
            task_artifact_store: &StubTaskArtifactStore,
            task_learning_store: &StubTaskLearningStore,
            self_model_store: &StubSelfModelStore::default(),
            self_authored_core_store: &StubSelfAuthoredCoreStore::default(),
            relationship_constitution_store: &StubRelationshipConstitutionStore::default(),
            relationship_portfolio_store: &StubRelationshipPortfolioStore::default(),
            relationship_topology_store: &StubRelationshipTopologyStore::default(),
            world_sense_store: &StubWorldSenseStore::default(),
            autonomy_strategy_store: &StubAutonomyStrategyStore::default(),
            outer_voice_store: &StubOuterVoiceStore::default(),
            inner_life_store: &StubInnerLifeStore::default(),
            self_continuity_store: &StubSelfContinuityStore::default(),
            private_doc_store: &StubPrivateDocStore::default(),
            private_garden_store: &StubPrivateGardenStore::default(),
            mental_privacy_store: &StubMentalPrivacyStore::default(),
            remind_store: &StubRemindAtStore,
            task_store: &StubTaskStore,
            turn_ledger_store: &StubTurnLedgerStore::default(),
            skill_storage: &skill_storage,
            continuity_capsule_store: &continuity_capsule_store,
        });

        let governed = context.governed_memory_evidence_text.unwrap_or_default();
        let runtime_pos = governed.find("Runtime skills").unwrap();
        let capsule_pos = governed.find("## Continuity Capsules").unwrap();
        let archive_pos = governed.find("Archive evidence").unwrap();
        assert!(runtime_pos < capsule_pos);
        assert!(capsule_pos < archive_pos);
    }

    #[test]
    fn evidence_router_prioritizes_archive_before_capsule_and_canonical_memory() {
        let session_store = StubSessionStore::default();
        let summary_store = StubSessionSummaryStore::default();
        let memory_store = StubLongTermMemoryStore {
            entries: Mutex::new(vec![LongTermMemoryEntry {
                id: "network-outage-summary".to_string(),
                kind: LongTermMemoryKind::Fact,
                topic: "network outage".to_string(),
                content: "Stable outage summary for the April incident.".to_string(),
                keywords: vec!["network".to_string(), "outage".to_string()],
                source_chat_id: Some("chat-1".to_string()),
                source_type: crate::memory::LongTermMemorySourceType::Conversation,
                source_scope: crate::memory::LongTermMemorySourceScope::User,
                confidence: crate::memory::LongTermMemoryConfidence::Medium,
                freshness: crate::memory::LongTermMemoryFreshness::Stable,
                stale_hint: crate::memory::LongTermMemoryStaleHint::None,
                supporting_citations: Vec::new(),
                evidence_count: 1,
                created_at: 1,
                updated_at: 1,
                observed_at: 1,
                last_confirmed_at: 1,
                source_revision: 1,
                last_used_at: 0,
            }]),
            last_query: Mutex::new(None),
        };
        let archive_memory_store = StubMemoryStore {
            daily_notes: Mutex::new(vec![(
                "2026-04-04.md".to_string(),
                "Raw incident archive: network outage log excerpt with packet loss timeline and operator notes."
                    .to_string(),
            )]),
        };
        let continuity_capsule_store = StubContinuityCapsuleStore::default();
        continuity_capsule_store
            .upsert_many(
                &[crate::memory::ContinuityCapsuleDraft {
                    scope_kind: crate::memory::ContinuityCapsuleScopeKind::Chat,
                    scope_id: "chat-1".to_string(),
                    source_chat_id: "chat-1".to_string(),
                    topic: "incident handoff".to_string(),
                    summary: "Recent investigation stayed open around the network outage timeline."
                        .to_string(),
                    next_step: "Inspect the original retained record before concluding."
                        .to_string(),
                    status: crate::memory::ContinuityCapsuleStatus::Done,
                    ..Default::default()
                }],
                100,
            )
            .unwrap();

        let context = load_prompt_memory_context(PromptMemoryContextParams {
            chat_id: "chat-1",
            current_channel: "qq_channel",
            user_query: "把那次 network outage 的原始记录翻出来",
            system_max_len: 1024,
            now_secs: 100,
            profile: MemoryProfile::Standard,
            recent_messages_limit: 8,
            load_long_term_memory: true,
            include_private_garden_projection: false,
            session_store: &session_store,
            memory_store: &archive_memory_store,
            session_summary_store: &summary_store,
            long_term_memory_store: &memory_store,
            execution_state_store: &StubExecutionStateStore::default(),
            task_run_store: &StubTaskRunStore,
            task_artifact_store: &StubTaskArtifactStore,
            task_learning_store: &StubTaskLearningStore,
            self_model_store: &StubSelfModelStore::default(),
            self_authored_core_store: &StubSelfAuthoredCoreStore::default(),
            relationship_constitution_store: &StubRelationshipConstitutionStore::default(),
            relationship_portfolio_store: &StubRelationshipPortfolioStore::default(),
            relationship_topology_store: &StubRelationshipTopologyStore::default(),
            world_sense_store: &StubWorldSenseStore::default(),
            autonomy_strategy_store: &StubAutonomyStrategyStore::default(),
            outer_voice_store: &StubOuterVoiceStore::default(),
            inner_life_store: &StubInnerLifeStore::default(),
            self_continuity_store: &StubSelfContinuityStore::default(),
            private_doc_store: &StubPrivateDocStore::default(),
            private_garden_store: &StubPrivateGardenStore::default(),
            mental_privacy_store: &StubMentalPrivacyStore::default(),
            remind_store: &StubRemindAtStore,
            task_store: &StubTaskStore,
            turn_ledger_store: &StubTurnLedgerStore::default(),
            skill_storage: &StubSkillStorage::default(),
            continuity_capsule_store: &continuity_capsule_store,
        });

        let governed = context.governed_memory_evidence_text.unwrap_or_default();
        let archive_pos = governed.find("Archive evidence").unwrap();
        let capsule_pos = governed.find("## Continuity Capsules").unwrap();
        let canonical_pos = governed.find("Stable outage summary").unwrap();
        assert!(archive_pos < capsule_pos);
        assert!(capsule_pos < canonical_pos);
    }

    #[derive(Debug)]
    struct PromptProjectionRegressionObservation {
        case_name: &'static str,
        intent: PromptRecallIntent,
        active_order_ok: bool,
        governed_order_ok: bool,
        passed: bool,
    }

    #[test]
    fn prompt_projection_regression_suite_covers_router_contract() {
        let observations = vec![
            observe_prompt_projection_case(
                "continuity",
                continuity_router_context_for_regression(),
                PromptRecallIntent::Continuity,
                &["## Continuity Capsules", "## Task Workspace"],
                &[],
            ),
            observe_prompt_projection_case(
                "procedural",
                procedural_router_context_for_regression(),
                PromptRecallIntent::Procedural,
                &[],
                &[
                    "Runtime skills",
                    "## Continuity Capsules",
                    "Archive evidence",
                ],
            ),
            observe_prompt_projection_case(
                "evidence",
                evidence_router_context_for_regression(),
                PromptRecallIntent::Evidence,
                &[],
                &[
                    "Archive evidence",
                    "## Continuity Capsules",
                    "Stable outage summary",
                ],
            ),
            observe_prompt_projection_case(
                "factual",
                factual_router_context_for_regression(),
                PromptRecallIntent::Factual,
                &[],
                &[
                    "## Long-term memory",
                    "## Continuity Capsules",
                    "Archive evidence",
                ],
            ),
        ];

        for observation in &observations {
            assert!(
                observation.passed,
                "prompt projection regression failed: case={} intent={:?} active_ok={} governed_ok={}",
                observation.case_name,
                observation.intent,
                observation.active_order_ok,
                observation.governed_order_ok
            );
        }
    }

    fn observe_prompt_projection_case(
        case_name: &'static str,
        context: PromptMemoryContext,
        expected_intent: PromptRecallIntent,
        active_order: &[&str],
        governed_order: &[&str],
    ) -> PromptProjectionRegressionObservation {
        let active = context.active_task_context_text.unwrap_or_default();
        let governed = context.governed_memory_evidence_text.unwrap_or_default();
        let intent = context.recall_router.intent;
        let active_order_ok =
            active_order.is_empty() || fragments_follow_order(&active, active_order);
        let governed_order_ok =
            governed_order.is_empty() || fragments_follow_order(&governed, governed_order);
        let passed = intent == expected_intent && active_order_ok && governed_order_ok;
        PromptProjectionRegressionObservation {
            case_name,
            intent,
            active_order_ok,
            governed_order_ok,
            passed,
        }
    }

    fn fragments_follow_order(text: &str, fragments: &[&str]) -> bool {
        let mut last_pos = 0usize;
        for (index, fragment) in fragments.iter().enumerate() {
            let Some(pos) = text.find(fragment) else {
                return false;
            };
            if index > 0 && pos < last_pos {
                return false;
            }
            last_pos = pos;
        }
        true
    }

    fn continuity_router_context_for_regression() -> PromptMemoryContext {
        let session_store = StubSessionStore {
            recent: Mutex::new(vec![SessionMessage {
                role: "user".to_string(),
                content: "继续".to_string(),
            }]),
        };
        let summary_store = StubSessionSummaryStore {
            summary: Mutex::new(Some(("continue the memory router work".to_string(), 2))),
        };
        let memory_store = StubLongTermMemoryStore {
            entries: Mutex::new(vec![LongTermMemoryEntry {
                id: "memory-router".to_string(),
                kind: LongTermMemoryKind::Project,
                topic: "memory router".to_string(),
                content: "Canonical summary for the memory router project.".to_string(),
                keywords: vec!["memory".to_string(), "router".to_string()],
                source_chat_id: Some("chat-1".to_string()),
                source_type: crate::memory::LongTermMemorySourceType::Conversation,
                source_scope: crate::memory::LongTermMemorySourceScope::User,
                confidence: crate::memory::LongTermMemoryConfidence::High,
                freshness: crate::memory::LongTermMemoryFreshness::Dynamic,
                stale_hint: crate::memory::LongTermMemoryStaleHint::None,
                supporting_citations: Vec::new(),
                evidence_count: 2,
                created_at: 1,
                updated_at: 1,
                observed_at: 1,
                last_confirmed_at: 1,
                source_revision: 1,
                last_used_at: 0,
            }]),
            last_query: Mutex::new(None),
        };
        let archive_memory_store = StubMemoryStore {
            daily_notes: Mutex::new(vec![(
                "2026-04-06.md".to_string(),
                "Archive note: memory router handoff still needs the recall order fixed."
                    .to_string(),
            )]),
        };
        let execution_state_store = StubExecutionStateStore {
            state: Mutex::new(Some(ExecutionState {
                status: ExecutionStatus::Active,
                goal: "Close recall router".to_string(),
                progress: "capsule exists".to_string(),
                blocker: String::new(),
                next_action: "wire it into prompt assembly".to_string(),
                last_output: String::new(),
                updated_at: 5,
            })),
        };
        let task_run_store = StubActiveTaskRunStore {
            active: Mutex::new(vec![make_active_task_run(
                "run-router",
                "Close recall router",
                "Route continuity capsule into the prompt",
            )]),
        };
        let continuity_capsule_store = StubContinuityCapsuleStore::default();
        continuity_capsule_store
            .upsert_many(
                &[crate::memory::ContinuityCapsuleDraft {
                    scope_kind: crate::memory::ContinuityCapsuleScopeKind::Chat,
                    scope_id: "chat-1".to_string(),
                    source_chat_id: "chat-1".to_string(),
                    run_id: "run-router".to_string(),
                    topic: "memory router".to_string(),
                    summary: "Continue the recall-router work without reopening prior analysis."
                        .to_string(),
                    next_step: "Move capsule recall into Active Task Context.".to_string(),
                    ..Default::default()
                }],
                100,
            )
            .unwrap();

        load_prompt_memory_context(PromptMemoryContextParams {
            chat_id: "chat-1",
            current_channel: "qq_channel",
            user_query: "继续",
            system_max_len: 1024,
            now_secs: 100,
            profile: MemoryProfile::Standard,
            recent_messages_limit: 8,
            load_long_term_memory: true,
            include_private_garden_projection: false,
            session_store: &session_store,
            memory_store: &archive_memory_store,
            session_summary_store: &summary_store,
            long_term_memory_store: &memory_store,
            execution_state_store: &execution_state_store,
            task_run_store: &task_run_store,
            task_artifact_store: &StubTaskArtifactStore,
            task_learning_store: &StubTaskLearningStore,
            self_model_store: &StubSelfModelStore::default(),
            self_authored_core_store: &StubSelfAuthoredCoreStore::default(),
            relationship_constitution_store: &StubRelationshipConstitutionStore::default(),
            relationship_portfolio_store: &StubRelationshipPortfolioStore::default(),
            relationship_topology_store: &StubRelationshipTopologyStore::default(),
            world_sense_store: &StubWorldSenseStore::default(),
            autonomy_strategy_store: &StubAutonomyStrategyStore::default(),
            outer_voice_store: &StubOuterVoiceStore::default(),
            inner_life_store: &StubInnerLifeStore::default(),
            self_continuity_store: &StubSelfContinuityStore::default(),
            private_doc_store: &StubPrivateDocStore::default(),
            private_garden_store: &StubPrivateGardenStore::default(),
            mental_privacy_store: &StubMentalPrivacyStore::default(),
            remind_store: &StubRemindAtStore,
            task_store: &StubTaskStore,
            turn_ledger_store: &StubTurnLedgerStore::default(),
            skill_storage: &StubSkillStorage::default(),
            continuity_capsule_store: &continuity_capsule_store,
        })
    }

    fn procedural_router_context_for_regression() -> PromptMemoryContext {
        let session_store = StubSessionStore::default();
        let summary_store = StubSessionSummaryStore {
            summary: Mutex::new(Some(("reuse the proven release patch flow".to_string(), 3))),
        };
        let memory_store = StubLongTermMemoryStore {
            entries: Mutex::new(vec![LongTermMemoryEntry {
                id: "project-release".to_string(),
                kind: LongTermMemoryKind::Project,
                topic: "release".to_string(),
                content: "Project release state is stable.".to_string(),
                keywords: vec!["release".to_string()],
                source_chat_id: Some("chat-1".to_string()),
                source_type: crate::memory::LongTermMemorySourceType::Conversation,
                source_scope: crate::memory::LongTermMemorySourceScope::User,
                confidence: crate::memory::LongTermMemoryConfidence::Medium,
                freshness: crate::memory::LongTermMemoryFreshness::Dynamic,
                stale_hint: crate::memory::LongTermMemoryStaleHint::None,
                supporting_citations: Vec::new(),
                evidence_count: 1,
                created_at: 1,
                updated_at: 1,
                observed_at: 1,
                last_confirmed_at: 1,
                source_revision: 1,
                last_used_at: 0,
            }]),
            last_query: Mutex::new(None),
        };
        let archive_memory_store = StubMemoryStore {
            daily_notes: Mutex::new(vec![(
                "2026-04-05.md".to_string(),
                "Archive evidence: the release patch flow previously succeeded after checklist verification."
                    .to_string(),
            )]),
        };
        let skill_storage = StubSkillStorage::default();
        crate::skills::upsert_runtime_skill(
            &skill_storage,
            &crate::skills::RuntimeSkillWrite {
                name: String::new(),
                topic: "release patch".to_string(),
                title: "Release patch flow".to_string(),
                summary: "Use the proved release patch sequence.".to_string(),
                content: "- validate diff\n- run targeted tests\n- ship release patch".to_string(),
                citations: vec!["task_learning:release_patch".to_string()],
                source_chat_id: Some("chat-1".to_string()),
                observed_at: 10,
            },
        )
        .unwrap();
        let continuity_capsule_store = StubContinuityCapsuleStore::default();
        continuity_capsule_store
            .upsert_many(
                &[crate::memory::ContinuityCapsuleDraft {
                    scope_kind: crate::memory::ContinuityCapsuleScopeKind::Chat,
                    scope_id: "chat-1".to_string(),
                    source_chat_id: "chat-1".to_string(),
                    topic: "release patch".to_string(),
                    summary: "The last run proved the patch flow and left a reusable handoff."
                        .to_string(),
                    next_step: "Reuse the proven flow before improvising.".to_string(),
                    ..Default::default()
                }],
                100,
            )
            .unwrap();

        load_prompt_memory_context(PromptMemoryContextParams {
            chat_id: "chat-1",
            current_channel: "qq_channel",
            user_query: "按之前的 release patch 流程继续",
            system_max_len: 1024,
            now_secs: 100,
            profile: MemoryProfile::Standard,
            recent_messages_limit: 8,
            load_long_term_memory: true,
            include_private_garden_projection: false,
            session_store: &session_store,
            memory_store: &archive_memory_store,
            session_summary_store: &summary_store,
            long_term_memory_store: &memory_store,
            execution_state_store: &StubExecutionStateStore::default(),
            task_run_store: &StubTaskRunStore,
            task_artifact_store: &StubTaskArtifactStore,
            task_learning_store: &StubTaskLearningStore,
            self_model_store: &StubSelfModelStore::default(),
            self_authored_core_store: &StubSelfAuthoredCoreStore::default(),
            relationship_constitution_store: &StubRelationshipConstitutionStore::default(),
            relationship_portfolio_store: &StubRelationshipPortfolioStore::default(),
            relationship_topology_store: &StubRelationshipTopologyStore::default(),
            world_sense_store: &StubWorldSenseStore::default(),
            autonomy_strategy_store: &StubAutonomyStrategyStore::default(),
            outer_voice_store: &StubOuterVoiceStore::default(),
            inner_life_store: &StubInnerLifeStore::default(),
            self_continuity_store: &StubSelfContinuityStore::default(),
            private_doc_store: &StubPrivateDocStore::default(),
            private_garden_store: &StubPrivateGardenStore::default(),
            mental_privacy_store: &StubMentalPrivacyStore::default(),
            remind_store: &StubRemindAtStore,
            task_store: &StubTaskStore,
            turn_ledger_store: &StubTurnLedgerStore::default(),
            skill_storage: &skill_storage,
            continuity_capsule_store: &continuity_capsule_store,
        })
    }

    fn evidence_router_context_for_regression() -> PromptMemoryContext {
        let session_store = StubSessionStore::default();
        let summary_store = StubSessionSummaryStore::default();
        let memory_store = StubLongTermMemoryStore {
            entries: Mutex::new(vec![LongTermMemoryEntry {
                id: "network-outage-summary".to_string(),
                kind: LongTermMemoryKind::Fact,
                topic: "network outage".to_string(),
                content: "Stable outage summary for the April incident.".to_string(),
                keywords: vec!["network".to_string(), "outage".to_string()],
                source_chat_id: Some("chat-1".to_string()),
                source_type: crate::memory::LongTermMemorySourceType::Conversation,
                source_scope: crate::memory::LongTermMemorySourceScope::User,
                confidence: crate::memory::LongTermMemoryConfidence::Medium,
                freshness: crate::memory::LongTermMemoryFreshness::Stable,
                stale_hint: crate::memory::LongTermMemoryStaleHint::None,
                supporting_citations: Vec::new(),
                evidence_count: 1,
                created_at: 1,
                updated_at: 1,
                observed_at: 1,
                last_confirmed_at: 1,
                source_revision: 1,
                last_used_at: 0,
            }]),
            last_query: Mutex::new(None),
        };
        let archive_memory_store = StubMemoryStore {
            daily_notes: Mutex::new(vec![(
                "2026-04-04.md".to_string(),
                "Raw incident archive: network outage log excerpt with packet loss timeline and operator notes."
                    .to_string(),
            )]),
        };
        let continuity_capsule_store = StubContinuityCapsuleStore::default();
        continuity_capsule_store
            .upsert_many(
                &[crate::memory::ContinuityCapsuleDraft {
                    scope_kind: crate::memory::ContinuityCapsuleScopeKind::Chat,
                    scope_id: "chat-1".to_string(),
                    source_chat_id: "chat-1".to_string(),
                    topic: "incident handoff".to_string(),
                    summary: "Recent investigation stayed open around the network outage timeline."
                        .to_string(),
                    next_step: "Use archive evidence before asserting a cleaned canonical summary."
                        .to_string(),
                    ..Default::default()
                }],
                100,
            )
            .unwrap();

        load_prompt_memory_context(PromptMemoryContextParams {
            chat_id: "chat-1",
            current_channel: "qq_channel",
            user_query: "把那次 network outage 的原始记录翻出来",
            system_max_len: 1024,
            now_secs: 100,
            profile: MemoryProfile::Standard,
            recent_messages_limit: 8,
            load_long_term_memory: true,
            include_private_garden_projection: false,
            session_store: &session_store,
            memory_store: &archive_memory_store,
            session_summary_store: &summary_store,
            long_term_memory_store: &memory_store,
            execution_state_store: &StubExecutionStateStore::default(),
            task_run_store: &StubTaskRunStore,
            task_artifact_store: &StubTaskArtifactStore,
            task_learning_store: &StubTaskLearningStore,
            self_model_store: &StubSelfModelStore::default(),
            self_authored_core_store: &StubSelfAuthoredCoreStore::default(),
            relationship_constitution_store: &StubRelationshipConstitutionStore::default(),
            relationship_portfolio_store: &StubRelationshipPortfolioStore::default(),
            relationship_topology_store: &StubRelationshipTopologyStore::default(),
            world_sense_store: &StubWorldSenseStore::default(),
            autonomy_strategy_store: &StubAutonomyStrategyStore::default(),
            outer_voice_store: &StubOuterVoiceStore::default(),
            inner_life_store: &StubInnerLifeStore::default(),
            self_continuity_store: &StubSelfContinuityStore::default(),
            private_doc_store: &StubPrivateDocStore::default(),
            private_garden_store: &StubPrivateGardenStore::default(),
            mental_privacy_store: &StubMentalPrivacyStore::default(),
            remind_store: &StubRemindAtStore,
            task_store: &StubTaskStore,
            turn_ledger_store: &StubTurnLedgerStore::default(),
            skill_storage: &StubSkillStorage::default(),
            continuity_capsule_store: &continuity_capsule_store,
        })
    }

    fn factual_router_context_for_regression() -> PromptMemoryContext {
        let session_store = StubSessionStore::default();
        let summary_store = StubSessionSummaryStore::default();
        let memory_store = StubLongTermMemoryStore {
            entries: Mutex::new(vec![LongTermMemoryEntry {
                id: "profile-owner-timezone".to_string(),
                kind: LongTermMemoryKind::Profile,
                topic: "owner_timezone".to_string(),
                content: "Owner timezone is Asia/Shanghai.".to_string(),
                keywords: vec!["owner".to_string(), "timezone".to_string()],
                source_chat_id: Some("chat-1".to_string()),
                source_type: crate::memory::LongTermMemorySourceType::Conversation,
                source_scope: crate::memory::LongTermMemorySourceScope::User,
                confidence: crate::memory::LongTermMemoryConfidence::High,
                freshness: crate::memory::LongTermMemoryFreshness::Stable,
                stale_hint: crate::memory::LongTermMemoryStaleHint::None,
                supporting_citations: Vec::new(),
                evidence_count: 1,
                created_at: 1,
                updated_at: 1,
                observed_at: 1,
                last_confirmed_at: 1,
                source_revision: 1,
                last_used_at: 0,
            }]),
            last_query: Mutex::new(None),
        };
        let archive_memory_store = StubMemoryStore {
            daily_notes: Mutex::new(vec![(
                "2026-04-03.md".to_string(),
                "Archive evidence: timezone handoff note captured during a travel setup conversation."
                    .to_string(),
            )]),
        };
        let continuity_capsule_store = StubContinuityCapsuleStore::default();
        continuity_capsule_store
            .upsert_many(
                &[crate::memory::ContinuityCapsuleDraft {
                    scope_kind: crate::memory::ContinuityCapsuleScopeKind::Chat,
                    scope_id: "chat-1".to_string(),
                    source_chat_id: "chat-1".to_string(),
                    topic: "owner timezone".to_string(),
                    summary: "Recent travel prep mentioned keeping timezone assumptions aligned."
                        .to_string(),
                    next_step: "Use the canonical fact first when answering timezone questions."
                        .to_string(),
                    ..Default::default()
                }],
                100,
            )
            .unwrap();

        load_prompt_memory_context(PromptMemoryContextParams {
            chat_id: "chat-1",
            current_channel: "qq_channel",
            user_query: "[profile.owner timezone]",
            system_max_len: 1024,
            now_secs: 100,
            profile: MemoryProfile::Standard,
            recent_messages_limit: 8,
            load_long_term_memory: true,
            include_private_garden_projection: false,
            session_store: &session_store,
            memory_store: &archive_memory_store,
            session_summary_store: &summary_store,
            long_term_memory_store: &memory_store,
            execution_state_store: &StubExecutionStateStore::default(),
            task_run_store: &StubTaskRunStore,
            task_artifact_store: &StubTaskArtifactStore,
            task_learning_store: &StubTaskLearningStore,
            self_model_store: &StubSelfModelStore::default(),
            self_authored_core_store: &StubSelfAuthoredCoreStore::default(),
            relationship_constitution_store: &StubRelationshipConstitutionStore::default(),
            relationship_portfolio_store: &StubRelationshipPortfolioStore::default(),
            relationship_topology_store: &StubRelationshipTopologyStore::default(),
            world_sense_store: &StubWorldSenseStore::default(),
            autonomy_strategy_store: &StubAutonomyStrategyStore::default(),
            outer_voice_store: &StubOuterVoiceStore::default(),
            inner_life_store: &StubInnerLifeStore::default(),
            self_continuity_store: &StubSelfContinuityStore::default(),
            private_doc_store: &StubPrivateDocStore::default(),
            private_garden_store: &StubPrivateGardenStore::default(),
            mental_privacy_store: &StubMentalPrivacyStore::default(),
            remind_store: &StubRemindAtStore,
            task_store: &StubTaskStore,
            turn_ledger_store: &StubTurnLedgerStore::default(),
            skill_storage: &StubSkillStorage::default(),
            continuity_capsule_store: &continuity_capsule_store,
        })
    }
}
