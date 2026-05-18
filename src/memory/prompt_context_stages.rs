use crate::agent::{load_active_work_for_chat, ActiveWorkRecord};
use crate::task_execution::{
    active_task_run_for_chat, build_task_recall_bundle, render_task_workspace_block, TaskRunRecord,
};
use std::collections::BTreeSet;

use super::{
    board_subject_scope_id, build_archive_evidence_block, build_continuity_recall_query,
    build_self_state, build_world_snapshot_from_commitments, collect_private_targets,
    decide_prompt_recall_route, derive_relationship_constitution,
    inspect_continuity_capsule_recall, load_recent_persona_evidence, load_world_snapshot_reminders,
    load_world_snapshot_tasks, memory_capability_profile, memory_policy,
    parse_explicit_long_term_slot_query, private_garden_scope_id, recall_long_term_memory_block,
    relationship_scope_id, render_autonomy_strategy_block, render_continuity_capsule_block,
    render_exact_long_term_memory_block, render_execution_state_block, render_inner_life_block,
    render_mental_privacy_boundary_block, render_outer_voice_block,
    render_persistent_self_authored_core_block, render_private_doc_workspace_block,
    render_private_garden_block, render_relationship_constitution_block,
    render_relationship_portfolio_block, render_self_continuity_block, render_self_model_block,
    render_self_state_block, render_turn_observation_ledger_block, render_work_continuity_block,
    render_world_sense_block, render_world_snapshot_block, ContinuityCapsuleRecallInspectionInput,
    ContinuityCapsuleScopeKind, MemoryProfile, PromptMemoryContextParams,
    PromptRecallRouterDecision, RecallPlane, RecallQuery, RecallSelectionReport, SessionMessage,
    WorldSnapshotContext, MAX_WORK_CONTINUITY_BLOCK_LEN,
};

pub(crate) struct PromptContextSeed {
    pub profile: MemoryProfile,
    pub subject_id: &'static str,
    pub relationship_id: String,
    pub esp_compact_first_turn_graph: bool,
    pub reuse_stored_relationship_constitution: bool,
    pub governed_memory_enabled: bool,
    pub relationship_constitution_existing: Option<super::RelationshipConstitution>,
}

pub(crate) struct PromptSessionStage {
    pub recent_messages: Vec<SessionMessage>,
    pub summary_text: Option<String>,
    pub active_work: Option<Box<ActiveWorkRecord>>,
    pub work_continuity_text: Option<String>,
    pub execution_state_text: Option<String>,
    pub active_task_run: Option<Box<TaskRunRecord>>,
    pub task_workspace_text: Option<String>,
    pub task_recall_text: Option<String>,
}

pub(crate) struct PromptConstitutionalStage {
    pub recent_turn_observation_text: Option<String>,
    pub self_authored_core: Option<Box<super::SelfAuthoredCore>>,
    pub self_authored_core_text: Option<String>,
    pub relationship_portfolio_text: Option<String>,
    pub self_model: Option<Box<super::SelfModel>>,
    pub self_continuity: Option<Box<super::SelfContinuity>>,
    pub felt_significance: Option<Box<super::FeltSignificance>>,
    pub temperament_continuity: Option<Box<super::TemperamentContinuity>>,
    pub inner_conflict: Option<Box<super::InnerConflict>>,
    pub autonomy_strategy: Option<Box<super::AutonomyStrategy>>,
    pub outer_voice: Option<Box<super::OuterVoice>>,
    pub inner_life: Option<Box<super::InnerLife>>,
    pub private_workspace: Option<Box<super::PrivateDocWorkspace>>,
    pub recent_private_garden_docs: Vec<super::PrivateGardenDocRecord>,
    pub mental_privacy_state: Option<Box<super::MentalPrivacyState>>,
    pub relationship_constitution: Option<Box<super::RelationshipConstitution>>,
    pub relationship_constitution_text: Option<String>,
}

pub(crate) struct PromptPrivateProjectionStage {
    pub world_snapshot_text: Option<String>,
    pub world_sense_text: Option<String>,
    pub self_state_text: Option<String>,
    pub self_model_text: Option<String>,
    pub autonomy_strategy_text: Option<String>,
    pub outer_voice_text: Option<String>,
    pub inner_life_text: Option<String>,
    pub self_continuity_text: Option<String>,
    pub private_workspace_text: Option<String>,
    pub private_garden_text: Option<String>,
    pub mental_privacy_text: Option<String>,
}

pub(crate) struct PromptGovernedMemoryScratch {
    pub shared_factual_recall_report: RecallSelectionReport,
    pub continuity_capsule_report: RecallSelectionReport,
    pub archive_recall_report: RecallSelectionReport,
    pub runtime_skill_recall_report: RecallSelectionReport,
    pub task_recall_report: Option<RecallSelectionReport>,
}

pub(crate) struct PromptGovernedMemoryStage {
    pub long_term_memory_text: Option<String>,
    pub continuity_capsule_text: Option<String>,
    pub archive_evidence_text: Option<String>,
    pub runtime_skill_text: Option<String>,
    pub recall_router: PromptRecallRouterDecision,
    pub scratch: Box<PromptGovernedMemoryScratch>,
}

#[derive(Default)]
pub(crate) struct PromptContextLoadHealth {
    issues: Vec<String>,
}

impl PromptContextLoadHealth {
    fn record(&mut self, layer: &'static str, error: &crate::error::Error) {
        self.issues
            .push(format!("{layer} ({})", error.stage().trim()));
    }

    pub(crate) fn issues(self) -> Vec<String> {
        let mut seen = BTreeSet::new();
        self.issues
            .into_iter()
            .filter(|issue| seen.insert(issue.clone()))
            .collect()
    }
}

fn load_optional_with_health<T>(
    health: &mut PromptContextLoadHealth,
    layer: &'static str,
    load: impl FnOnce() -> crate::error::Result<Option<T>>,
) -> Option<T> {
    match load() {
        Ok(value) => value,
        Err(error) => {
            health.record(layer, &error);
            None
        }
    }
}

fn load_vec_with_health<T>(
    health: &mut PromptContextLoadHealth,
    layer: &'static str,
    load: impl FnOnce() -> crate::error::Result<Vec<T>>,
) -> Vec<T> {
    match load() {
        Ok(value) => value,
        Err(error) => {
            health.record(layer, &error);
            Vec::new()
        }
    }
}

fn prompt_private_garden_doc_limit(profile: MemoryProfile) -> usize {
    memory_policy(profile)
        .private_garden
        .recent_doc_count
        .max(1)
}

fn disabled_recall_report(
    plane: RecallPlane,
    backend: &str,
    miss_reason: Option<String>,
) -> RecallSelectionReport {
    RecallSelectionReport {
        plane,
        query: RecallQuery {
            plane,
            ..RecallQuery::default()
        },
        backend: backend.to_string(),
        candidate_count: 0,
        selected_count: 0,
        selected_ids: Vec::new(),
        miss_reason,
        selection_note: None,
        candidates: Vec::new(),
    }
}

fn governed_recall_disabled_reason(params: &PromptMemoryContextParams<'_>) -> String {
    if !params.participation_plan.load_l2_governed_recall {
        "prompt_participation_disabled".to_string()
    } else if !params.load_long_term_memory {
        "long_term_recall_disabled".to_string()
    } else {
        "system_budget_below_block_threshold".to_string()
    }
}

fn should_load_recent_persona_evidence_for_prompt(
    params: &PromptMemoryContextParams<'_>,
    seed: &PromptContextSeed,
) -> bool {
    if matches!(
        params.memory_system_kind,
        super::MemorySystemKind::EspCompact
    ) && seed.esp_compact_first_turn_graph
    {
        return false;
    }
    !seed.reuse_stored_relationship_constitution
        || params.participation_plan.load_l2_background_governance
        || params.participation_plan.load_l3_private_depth
}

fn should_load_p3_subjective_projection(params: &PromptMemoryContextParams<'_>) -> bool {
    matches!(
        params.memory_system_kind,
        super::MemorySystemKind::LinuxFull
    ) && (params.participation_plan.load_l2_background_governance
        || params.participation_plan.load_l3_private_depth)
}

#[inline(never)]
pub(crate) fn seed_prompt_context(
    params: &PromptMemoryContextParams<'_>,
    health: &mut PromptContextLoadHealth,
) -> PromptContextSeed {
    let profile = params.memory_system_kind.memory_profile();
    let relationship_id = relationship_scope_id(params.current_channel, params.chat_id);
    let relationship_constitution_existing =
        load_optional_with_health(health, "relationship_constitution_existing", || {
            params.relationship_constitution_store.get(&relationship_id)
        });
    let esp_compact_first_turn_graph = matches!(
        params.memory_system_kind,
        super::MemorySystemKind::EspCompact
    ) && params.participation_plan.load_l1_constitutional
        && params.participation_plan.load_l1_session
        && !params.participation_plan.load_l2_background_governance
        && !params.participation_plan.load_l3_private_depth;
    let reuse_stored_relationship_constitution =
        esp_compact_first_turn_graph && relationship_constitution_existing.is_some();
    let governed_memory_enabled = params.participation_plan.load_l2_governed_recall
        && params.load_long_term_memory
        && params.system_max_len >= memory_policy(profile).long_term_recall.block_min_len;

    PromptContextSeed {
        profile,
        subject_id: board_subject_scope_id(),
        relationship_id,
        esp_compact_first_turn_graph,
        reuse_stored_relationship_constitution,
        governed_memory_enabled,
        relationship_constitution_existing,
    }
}

#[inline(never)]
pub(crate) fn load_session_stage(
    params: &PromptMemoryContextParams<'_>,
    seed: &PromptContextSeed,
    health: &mut PromptContextLoadHealth,
) -> Box<PromptSessionStage> {
    let recall_policy = memory_policy(seed.profile).long_term_recall;
    let recent_message_limit = params
        .recent_messages_limit
        .max(if params.load_long_term_memory {
            recall_policy.recent_grounding_message_count
        } else {
            0
        });
    let recent_messages = if !params.participation_plan.load_l1_session || recent_message_limit == 0
    {
        Vec::new()
    } else {
        load_vec_with_health(health, "session_recent_messages", || {
            params
                .session_store
                .load_recent(params.chat_id, recent_message_limit)
        })
    };
    let summary_text = params
        .participation_plan
        .load_l1_session
        .then(|| {
            load_optional_with_health(health, "session_summary", || {
                params.session_summary_store.get_with_count(params.chat_id)
            })
            .map(|(summary, _)| summary.trim().to_string())
            .filter(|summary| !summary.is_empty())
        })
        .flatten();
    let active_task_run = params
        .participation_plan
        .load_l1_session
        .then(|| {
            load_optional_with_health(health, "active_task_run", || {
                active_task_run_for_chat(
                    params.task_run_store,
                    params.current_channel,
                    params.chat_id,
                )
            })
            .map(Box::new)
        })
        .flatten();
    let active_work = params
        .participation_plan
        .load_l1_session
        .then(|| {
            load_optional_with_health(health, "active_work", || {
                load_active_work_for_chat(
                    params.active_work_store,
                    active_task_run.as_deref(),
                    params.chat_id,
                )
            })
            .map(Box::new)
        })
        .flatten();
    let execution_state_text = active_work.as_ref().and_then(|record| {
        render_execution_state_block(
            &record.execution_state_projection(),
            memory_policy(seed.profile).execution_state.render_max_len,
        )
    });
    let work_continuity_text =
        super::build_work_continuity_record(active_work.as_deref(), summary_text.as_deref())
            .and_then(|record| {
                render_work_continuity_block(
                    &record,
                    params.system_max_len.min(MAX_WORK_CONTINUITY_BLOCK_LEN),
                )
            });
    let task_workspace_text = active_task_run.as_ref().and_then(|record| {
        let artifacts = load_vec_with_health(health, "task_artifacts", || {
            params
                .task_artifact_store
                .list_for_run(&record.run.run_id, 4)
        });
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

    Box::new(PromptSessionStage {
        recent_messages,
        summary_text,
        active_work,
        work_continuity_text,
        execution_state_text,
        active_task_run,
        task_workspace_text,
        task_recall_text,
    })
}

#[inline(never)]
pub(crate) fn load_constitutional_stage(
    params: &PromptMemoryContextParams<'_>,
    seed: &PromptContextSeed,
    health: &mut PromptContextLoadHealth,
) -> Box<PromptConstitutionalStage> {
    let self_authored_core = params
        .participation_plan
        .load_l1_constitutional
        .then(|| {
            load_optional_with_health(health, "self_authored_core", || {
                params.self_authored_core_store.get(seed.subject_id)
            })
            .map(Box::new)
        })
        .flatten();
    let relationship_portfolio = params
        .participation_plan
        .load_l2_background_governance
        .then(|| {
            load_optional_with_health(health, "relationship_portfolio", || {
                params.relationship_portfolio_store.get(seed.subject_id)
            })
        })
        .flatten();
    let relationship_topology = ((params.participation_plan.load_l1_constitutional
        && !seed.reuse_stored_relationship_constitution)
        || params.participation_plan.load_l2_background_governance)
        .then(|| {
            load_optional_with_health(health, "relationship_topology", || {
                params.relationship_topology_store.get(seed.subject_id)
            })
        })
        .flatten();
    let self_model = params
        .participation_plan
        .load_l2_background_governance
        .then(|| {
            load_optional_with_health(health, "self_model", || {
                params.self_model_store.get(seed.subject_id)
            })
            .map(Box::new)
        })
        .flatten();
    let self_continuity = (params.participation_plan.load_l1_constitutional
        || params.participation_plan.load_l2_background_governance)
        .then(|| {
            load_optional_with_health(health, "self_continuity", || {
                params.self_continuity_store.get(seed.subject_id)
            })
            .map(Box::new)
        })
        .flatten();
    let load_subjective_projection = should_load_p3_subjective_projection(params);
    let felt_significance = load_subjective_projection
        .then(|| {
            load_optional_with_health(health, "felt_significance", || {
                params.felt_significance_store.get(seed.subject_id)
            })
            .map(Box::new)
        })
        .flatten();
    let temperament_continuity = load_subjective_projection
        .then(|| {
            load_optional_with_health(health, "temperament_continuity", || {
                params.temperament_continuity_store.get(seed.subject_id)
            })
            .map(Box::new)
        })
        .flatten();
    let inner_conflict = load_subjective_projection
        .then(|| {
            load_optional_with_health(health, "inner_conflict", || {
                params.inner_conflict_store.get(seed.subject_id)
            })
            .map(Box::new)
        })
        .flatten();
    let autonomy_strategy = params
        .participation_plan
        .load_l2_background_governance
        .then(|| {
            load_optional_with_health(health, "autonomy_strategy", || {
                params.autonomy_strategy_store.get(seed.subject_id)
            })
            .map(Box::new)
        })
        .flatten();
    let outer_voice = ((params.participation_plan.load_l1_constitutional
        && !seed.reuse_stored_relationship_constitution)
        || params.participation_plan.load_l2_background_governance)
        .then(|| {
            load_optional_with_health(health, "outer_voice", || {
                params.outer_voice_store.get(&seed.relationship_id)
            })
            .map(Box::new)
        })
        .flatten();
    let inner_life = params
        .participation_plan
        .load_l3_private_depth
        .then(|| {
            load_optional_with_health(health, "inner_life", || {
                params.inner_life_store.get(seed.subject_id)
            })
            .map(Box::new)
        })
        .flatten();
    let private_workspace = params
        .participation_plan
        .load_l3_private_depth
        .then(|| {
            load_optional_with_health(health, "private_workspace", || {
                params.private_doc_store.get(seed.subject_id)
            })
            .map(Box::new)
        })
        .flatten();
    let recent_private_garden_docs = if params.participation_plan.load_l3_private_depth {
        load_vec_with_health(health, "private_garden", || {
            params.private_garden_store.list(
                private_garden_scope_id(),
                prompt_private_garden_doc_limit(seed.profile),
            )
        })
    } else {
        Vec::new()
    };
    let mental_privacy_state = (!seed.reuse_stored_relationship_constitution
        || params.participation_plan.load_l2_background_governance
        || params.participation_plan.load_l3_private_depth)
        .then(|| {
            load_optional_with_health(health, "mental_privacy_state", || {
                params.mental_privacy_store.get(&seed.relationship_id)
            })
            .map(Box::new)
        })
        .flatten();
    let recent_persona_evidence = should_load_recent_persona_evidence_for_prompt(params, seed)
        .then(|| {
            load_optional_with_health(health, "recent_persona_evidence", || {
                load_recent_persona_evidence(
                    params.turn_continuity_evidence_store,
                    &seed.relationship_id,
                )
            })
        })
        .flatten();
    let recent_turn_ledger = params
        .turn_ledger_store
        .get(&seed.relationship_id)
        .inspect_err(|error| {
            health.record("recent_turn_ledger", error);
        })
        .ok()
        .flatten();
    let recent_turn_observation_text = recent_turn_ledger
        .as_ref()
        .and_then(|ledger| ledger.observation.as_ref())
        .and_then(|observation| {
            render_turn_observation_ledger_block(observation, params.system_max_len.clamp(160, 320))
        });
    let self_authored_core_text = self_authored_core
        .as_ref()
        .and_then(|core| render_persistent_self_authored_core_block(core, 420));
    let relationship_portfolio_text = relationship_portfolio.as_ref().and_then(|portfolio| {
        render_relationship_portfolio_block(
            portfolio,
            params.now_secs,
            Some(&seed.relationship_id),
            420,
        )
    });
    let relationship_constitution = if seed.reuse_stored_relationship_constitution {
        seed.relationship_constitution_existing.clone()
    } else {
        derive_relationship_constitution(
            seed.relationship_constitution_existing.as_ref(),
            super::RelationshipConstitutionSyncInput {
                scope_id: &seed.relationship_id,
                channel: params.current_channel,
                chat_id: params.chat_id,
                now_secs: params.now_secs,
                self_authored_core: self_authored_core.as_deref(),
                relationship_portfolio: relationship_portfolio.as_ref(),
                relationship_topology: relationship_topology.as_ref(),
                mental_privacy_state: mental_privacy_state.as_deref(),
                outer_voice: outer_voice.as_deref(),
                recent_persona_evidence: recent_persona_evidence.as_ref(),
            },
        )
    };
    let relationship_constitution_text = relationship_constitution
        .as_ref()
        .and_then(|constitution| render_relationship_constitution_block(constitution, 420));

    Box::new(PromptConstitutionalStage {
        recent_turn_observation_text,
        self_authored_core,
        self_authored_core_text,
        relationship_portfolio_text,
        self_model,
        self_continuity,
        felt_significance,
        temperament_continuity,
        inner_conflict,
        autonomy_strategy,
        outer_voice,
        inner_life,
        private_workspace,
        recent_private_garden_docs,
        mental_privacy_state,
        relationship_constitution: relationship_constitution.map(Box::new),
        relationship_constitution_text,
    })
}

#[inline(never)]
pub(crate) fn load_private_projection_stage(
    params: &PromptMemoryContextParams<'_>,
    seed: &PromptContextSeed,
    constitutional: &PromptConstitutionalStage,
    health: &mut PromptContextLoadHealth,
) -> Box<PromptPrivateProjectionStage> {
    let self_model_text = constitutional.self_model.as_ref().and_then(|model| {
        render_self_model_block(model, memory_policy(seed.profile).self_model.render_max_len)
    });
    let world_snapshot_text = params
        .participation_plan
        .load_l2_background_governance
        .then(|| {
            let world_snapshot_ctx = WorldSnapshotContext {
                chat_id: params.chat_id,
                source_channel: params.current_channel,
                now_secs: params.now_secs,
                self_continuity: constitutional.self_continuity.as_deref(),
                remind_store: params.remind_store,
                task_store: params.task_store,
            };
            let reminders = match load_world_snapshot_reminders(world_snapshot_ctx) {
                Ok(reminders) => Some(reminders),
                Err(error) => {
                    health.record("world_snapshot_reminders", &error);
                    None
                }
            };
            let tasks = match load_world_snapshot_tasks(world_snapshot_ctx) {
                Ok(tasks) => Some(tasks),
                Err(error) => {
                    health.record("world_snapshot_tasks", &error);
                    None
                }
            };
            let (Some(reminders), Some(tasks)) = (reminders, tasks) else {
                return None;
            };
            let world_snapshot =
                build_world_snapshot_from_commitments(world_snapshot_ctx, &reminders, &tasks);
            render_world_snapshot_block(
                &world_snapshot,
                memory_policy(seed.profile).world_sense.snapshot_max_len,
            )
        })
        .flatten();
    let world_sense_text = params
        .participation_plan
        .load_l2_background_governance
        .then(|| {
            load_optional_with_health(health, "world_sense", || {
                params.world_sense_store.get(&seed.relationship_id)
            })
            .and_then(|world_sense| {
                render_world_sense_block(
                    &world_sense,
                    memory_policy(seed.profile).world_sense.render_max_len,
                )
            })
        })
        .flatten();
    let autonomy_strategy_text = constitutional
        .autonomy_strategy
        .as_ref()
        .and_then(|strategy| {
            render_autonomy_strategy_block(
                strategy,
                memory_policy(seed.profile).autonomy_strategy.render_max_len,
            )
        });

    let outer_voice_text = params
        .participation_plan
        .load_l2_background_governance
        .then(|| {
            constitutional.outer_voice.as_ref().and_then(|outer_voice| {
                render_outer_voice_block(
                    outer_voice,
                    memory_policy(seed.profile).outer_voice.render_max_len,
                )
            })
        })
        .flatten();
    let inner_life_text = constitutional.inner_life.as_ref().and_then(|inner_life| {
        render_inner_life_block(
            inner_life,
            memory_policy(seed.profile).inner_life.render_max_len,
        )
    });
    let self_continuity_text = params
        .participation_plan
        .load_l2_background_governance
        .then(|| {
            constitutional
                .self_continuity
                .as_ref()
                .and_then(|self_continuity| {
                    render_self_continuity_block(
                        self_continuity,
                        memory_policy(seed.profile).self_continuity.render_max_len,
                    )
                })
        })
        .flatten();
    let private_workspace_text = constitutional
        .private_workspace
        .as_ref()
        .and_then(|workspace| {
            render_private_doc_workspace_block(
                workspace,
                memory_policy(seed.profile).private_docs.render_max_len,
            )
        });
    let private_garden_text = (params.participation_plan.load_l3_private_depth
        && params.include_private_garden_projection)
        .then(|| {
            render_private_garden_block(
                &constitutional.recent_private_garden_docs,
                memory_policy(seed.profile).private_garden.recent_doc_count,
                memory_policy(seed.profile).private_garden.render_max_len,
            )
        })
        .flatten();
    let mental_privacy_targets = collect_private_targets(
        constitutional.self_model.as_deref(),
        constitutional.self_continuity.as_deref(),
        constitutional.inner_life.as_deref(),
        constitutional.private_workspace.as_deref(),
        &constitutional.recent_private_garden_docs,
    );
    let mental_privacy_text = (params.participation_plan.load_l2_background_governance
        || params.participation_plan.load_l3_private_depth)
        .then(|| {
            render_mental_privacy_boundary_block(
                constitutional.mental_privacy_state.as_deref(),
                &mental_privacy_targets,
                420,
            )
        })
        .flatten();
    let self_state_text = params
        .participation_plan
        .load_l2_background_governance
        .then(|| {
            render_self_state_block(
                &build_self_state(
                    constitutional.self_model.as_deref(),
                    constitutional.private_workspace.as_deref(),
                    constitutional.autonomy_strategy.as_deref(),
                    constitutional.inner_life.as_deref(),
                    constitutional.self_continuity.as_deref(),
                    &constitutional.recent_private_garden_docs,
                    params.now_secs,
                    seed.profile,
                ),
                memory_policy(seed.profile).self_state.render_max_len,
            )
        })
        .flatten();

    Box::new(PromptPrivateProjectionStage {
        world_snapshot_text,
        world_sense_text,
        self_state_text,
        self_model_text,
        autonomy_strategy_text,
        outer_voice_text,
        inner_life_text,
        self_continuity_text,
        private_workspace_text,
        private_garden_text,
        mental_privacy_text,
    })
}

#[inline(never)]
pub(crate) fn load_governed_memory_stage(
    params: &PromptMemoryContextParams<'_>,
    seed: &PromptContextSeed,
    session: &PromptSessionStage,
) -> Box<PromptGovernedMemoryStage> {
    let recall_policy = memory_policy(seed.profile).long_term_recall;
    let long_term_memory_text = if !seed.governed_memory_enabled {
        None
    } else {
        let grounding_start = session
            .recent_messages
            .len()
            .saturating_sub(recall_policy.recent_grounding_message_count);
        let capability = memory_capability_profile(seed.profile);
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
                        session.summary_text.as_deref(),
                        &session.recent_messages[grounding_start..],
                        params.system_max_len,
                        seed.profile,
                    )
                })
        } else {
            recall_long_term_memory_block(
                params.long_term_memory_store,
                params.chat_id,
                params.user_query,
                session.summary_text.as_deref(),
                &session.recent_messages[grounding_start..],
                params.system_max_len,
                seed.profile,
            )
        }
    };
    let continuity_recall_query = build_continuity_recall_query(
        params.user_query,
        session.summary_text.as_deref(),
        &session.recent_messages,
        session.active_work.as_deref(),
        session.active_task_run.as_deref(),
    );
    let (continuity_capsule_report, continuity_capsules) = if seed.governed_memory_enabled {
        inspect_continuity_capsule_recall(ContinuityCapsuleRecallInspectionInput {
            store: params.continuity_capsule_store,
            scope_kind: ContinuityCapsuleScopeKind::Chat,
            scope_id: params.chat_id,
            preferred_chat_id: Some(params.chat_id),
            query: &continuity_recall_query,
            summary_text: session.summary_text.as_deref(),
            recent_messages: &session.recent_messages,
            max_chars: params.system_max_len.min(480),
            now_secs: params.now_secs,
        })
    } else {
        (
            disabled_recall_report(
                RecallPlane::ContinuityCapsule,
                "continuity_capsule_heuristic",
                Some(governed_recall_disabled_reason(params)),
            ),
            Vec::new(),
        )
    };
    let continuity_capsule_text = seed
        .governed_memory_enabled
        .then(|| {
            render_continuity_capsule_block(&continuity_capsules, params.system_max_len.min(480))
        })
        .flatten();
    let archive_evidence_text =
        if !seed.governed_memory_enabled || seed.esp_compact_first_turn_graph {
            None
        } else {
            build_archive_evidence_block(
                params.session_store,
                params.memory_store,
                params.turn_ledger_store,
                params.chat_id,
                params.user_query,
                params.system_max_len,
                seed.profile,
            )
        };
    let shared_factual_recall_report = if seed.governed_memory_enabled {
        super::inspect_shared_factual_recall(
            params.long_term_memory_store,
            params.chat_id,
            params.user_query,
            session.summary_text.as_deref(),
            &session.recent_messages,
            params.system_max_len,
            seed.profile,
            params.now_secs,
        )
    } else {
        disabled_recall_report(
            RecallPlane::SharedFactual,
            "hybrid_canonical",
            Some(governed_recall_disabled_reason(params)),
        )
    };
    let archive_recall_report =
        if seed.governed_memory_enabled && !seed.esp_compact_first_turn_graph {
            super::inspect_archive_recall(
                params.session_store,
                params.memory_store,
                params.turn_ledger_store,
                params.chat_id,
                params.user_query,
                session.summary_text.as_deref(),
                &session.recent_messages,
                params.system_max_len.min(768),
                seed.profile,
            )
        } else {
            disabled_recall_report(
                RecallPlane::Archive,
                "archive_search",
                Some(if seed.esp_compact_first_turn_graph {
                    "assembly_graph_disabled".to_string()
                } else {
                    governed_recall_disabled_reason(params)
                }),
            )
        };
    let runtime_skill_query = {
        let combined = if parse_explicit_long_term_slot_query(params.user_query).is_some() {
            params.user_query.to_string()
        } else if super::archive_search::collect_archive_match_terms(params.user_query).is_empty() {
            [
                Some(params.user_query.trim().to_string()).filter(|value| !value.is_empty()),
                session.summary_text.clone(),
                session
                    .recent_messages
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
    let runtime_skill_text = (seed.governed_memory_enabled && !seed.esp_compact_first_turn_graph)
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
    let runtime_skill_recall_report =
        if seed.governed_memory_enabled && !seed.esp_compact_first_turn_graph {
            super::inspect_runtime_skill_recall(
                params.skill_storage,
                &runtime_skill_query,
                Some(params.chat_id),
                session.summary_text.as_deref(),
                &session.recent_messages,
                params.now_secs,
                params.system_max_len.min(420),
            )
        } else {
            disabled_recall_report(
                RecallPlane::RuntimeSkill,
                "runtime_skill_hybrid",
                Some(if seed.esp_compact_first_turn_graph {
                    "assembly_graph_disabled".to_string()
                } else {
                    governed_recall_disabled_reason(params)
                }),
            )
        };
    let task_recall_report = session.active_task_run.as_ref().map(|record| {
        super::inspect_task_recall(
            Some(record),
            params.task_learning_store,
            params.current_channel,
            params.chat_id,
            params.user_query,
            session.summary_text.as_deref(),
            &session.recent_messages,
            params.system_max_len.min(520),
        )
    });
    let recall_router = decide_prompt_recall_route(super::recall_router::PromptRecallRouterInput {
        user_query: params.user_query,
        has_active_continuity: session.active_work.is_some(),
        has_active_task_run: session.active_task_run.is_some(),
        shared_factual_report: &shared_factual_recall_report,
        continuity_capsule_report: &continuity_capsule_report,
        archive_report: &archive_recall_report,
        runtime_skill_report: &runtime_skill_recall_report,
        task_recall_report: task_recall_report.as_ref(),
    });

    Box::new(PromptGovernedMemoryStage {
        long_term_memory_text,
        continuity_capsule_text,
        archive_evidence_text,
        runtime_skill_text,
        recall_router,
        scratch: Box::new(PromptGovernedMemoryScratch {
            shared_factual_recall_report,
            continuity_capsule_report,
            archive_recall_report,
            runtime_skill_recall_report,
            task_recall_report,
        }),
    })
}

#[cfg(test)]
mod stack_shape_tests {
    use super::*;

    #[test]
    fn prompt_stage_structs_stay_small_enough_for_esp_prepare_chain() {
        assert!(
            core::mem::size_of::<PromptSessionStage>() <= 256,
            "PromptSessionStage regressed; large by-value stage objects must stay heap-backed"
        );
        assert!(
            core::mem::size_of::<PromptConstitutionalStage>() <= 256,
            "PromptConstitutionalStage regressed; constitutional stage must stay compact on stack"
        );
        assert!(
            core::mem::size_of::<PromptPrivateProjectionStage>() <= 320,
            "PromptPrivateProjectionStage regressed; projection stage stack footprint is too large"
        );
        assert!(
            core::mem::size_of::<PromptGovernedMemoryStage>() <= 160,
            "PromptGovernedMemoryStage regressed; governed stage should not grow on stack"
        );
    }
}
