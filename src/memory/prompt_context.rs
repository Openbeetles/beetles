//! Prompt 侧共享记忆读装配。
//! Shared prompt memory loading for agent context construction.

use crate::platform::SkillStorage;
use crate::task::TaskStore;
use crate::task_execution::{TaskArtifactStore, TaskLearningStore, TaskRunStore};
use crate::util::truncate_content_to_max;
use std::collections::BTreeSet;

use super::{
    prompt_context_stages::{
        load_constitutional_stage, load_governed_memory_stage, load_private_projection_stage,
        load_session_stage, seed_prompt_context, PromptContextLoadHealth,
    },
    AutonomyStrategyStore, ContinuityCapsuleStore, ExecutionStateStore, FeltSignificanceStore,
    InnerConflictStore, InnerLifeStore, LongTermMemoryStore, MemoryStore, MemorySystemKind,
    MentalPrivacyStore, OuterVoiceStore, PrivateDocStore, PrivateGardenStore, PromptRecallIntent,
    PromptRecallRouterDecision, RelationshipConstitutionStore, RelationshipPortfolioStore,
    RelationshipTopologyStore, RemindAtStore, SelfAuthoredCoreStore, SelfContinuityStore,
    SelfModelStore, SessionMessage, SessionStore, SessionSummaryStore, TemperamentContinuityStore,
    TurnContinuityEvidenceStore, TurnLedgerStore, WorldSenseStore,
};

pub struct PromptMemoryContext {
    pub memory_health_issues: Vec<String>,
    pub personality_governance_gate_text: Option<String>,
    pub summary_text: Option<String>,
    pub message_summary_text: Option<String>,
    pub long_term_memory_text: Option<String>,
    pub continuity_capsule_text: Option<String>,
    pub archive_evidence_text: Option<String>,
    pub runtime_skill_text: Option<String>,
    pub recent_turn_observation_text: Option<String>,
    pub work_continuity_text: Option<String>,
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
    pub felt_significance: Option<super::FeltSignificance>,
    pub temperament_continuity: Option<super::TemperamentContinuity>,
    pub inner_conflict: Option<super::InnerConflict>,
    pub autonomy_strategy: Option<super::AutonomyStrategy>,
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

pub struct PromptRuntimeCarry {
    pub summary_text: Option<String>,
    pub long_term_memory_text: Option<String>,
    pub recent_messages: Vec<SessionMessage>,
    pub prompt_recall_intent: PromptRecallIntent,
    pub runtime_skill_selected_ids: Vec<String>,
    pub task_recall_selected_ids: Vec<String>,
}

#[derive(Debug, Default)]
pub(crate) struct PromptProjectionGroups {
    pub constitutional_stack_text: Option<String>,
    pub active_task_context_text: Option<String>,
    pub governed_memory_evidence_text: Option<String>,
    pub background_governance_text: Option<String>,
}

impl PromptMemoryContext {
    pub fn trace_summary(&self) -> (usize, bool, bool, bool) {
        (
            self.recent_messages.len(),
            self.summary_text.as_ref().is_some(),
            self.message_summary_text.as_ref().is_some(),
            self.self_model_text.as_ref().is_some(),
        )
    }

    pub fn soul_kernel_projection(&self) -> crate::runtime::SoulKernelPromptProjection {
        crate::runtime::SoulKernelPromptProjection {
            personality_governance_gate_text: self.personality_governance_gate_text.clone(),
            self_authored_core_text: self.self_authored_core_text.clone(),
            relationship_constitution_text: self.relationship_constitution_text.clone(),
            persona_priority_text: self.persona_priority_text.clone(),
            mental_privacy_adjudication_text: self.mental_privacy_adjudication_text.clone(),
        }
    }

    pub fn render_memory_health_block(&self, max_len: usize) -> Option<String> {
        render_memory_health_block(&self.memory_health_issues, max_len)
    }

    pub fn into_runtime_carry(self) -> PromptRuntimeCarry {
        PromptRuntimeCarry {
            summary_text: self.summary_text,
            long_term_memory_text: self.long_term_memory_text,
            recent_messages: self.recent_messages,
            prompt_recall_intent: self.recall_router.intent,
            runtime_skill_selected_ids: self.runtime_skill_recall_report.selected_ids,
            task_recall_selected_ids: self
                .task_recall_report
                .map(|report| report.selected_ids)
                .unwrap_or_default(),
        }
    }

    fn build_reply_projection_groups(&self) -> PromptProjectionGroups {
        let constitutional_stack_text = self.soul_kernel_projection().constitutional_stack_text();
        let continuity_capsule_in_active =
            matches!(self.recall_router.intent, PromptRecallIntent::Continuity)
                && (self.work_continuity_text.is_some()
                    || self.recent_turn_observation_text.is_some()
                    || self.task_workspace_text.is_some()
                    || self.task_recall_text.is_some());
        let active_task_parts = self.recall_router.active_task_parts(
            self.work_continuity_text.as_deref(),
            self.recent_turn_observation_text.as_deref(),
            self.task_workspace_text.as_deref(),
            self.task_recall_text.as_deref(),
            continuity_capsule_in_active
                .then_some(self.continuity_capsule_text.as_deref())
                .flatten(),
        );
        let active_task_context_text = compose_prompt_projection_body(&active_task_parts);
        let governed_memory_parts = self.recall_router.governed_memory_parts(
            self.long_term_memory_text.as_deref(),
            (!continuity_capsule_in_active)
                .then_some(self.continuity_capsule_text.as_deref())
                .flatten(),
            self.archive_evidence_text.as_deref(),
            self.runtime_skill_text.as_deref(),
        );
        let governed_memory_evidence_text = compose_prompt_projection_body(&governed_memory_parts);
        let background_governance_text = compose_prompt_projection_body(&[
            self.relationship_portfolio_text.as_deref(),
            self.world_snapshot_text.as_deref(),
            self.world_sense_text.as_deref(),
            self.self_state_text.as_deref(),
            self.autonomy_strategy_text.as_deref(),
            self.outer_voice_text.as_deref(),
            self.mental_privacy_text.as_deref(),
        ]);
        PromptProjectionGroups {
            constitutional_stack_text,
            active_task_context_text,
            governed_memory_evidence_text,
            background_governance_text,
        }
    }

    pub(crate) fn normalize_projection_groups_for_prompt(
        &mut self,
        memory_system_kind: MemorySystemKind,
        system_budget: usize,
    ) -> PromptProjectionGroups {
        let budget = super::prompt_context_normalization_budget(memory_system_kind, system_budget);
        cap_prompt_text(&mut self.summary_text, budget.summary_max_len);
        cap_prompt_text(&mut self.message_summary_text, budget.summary_max_len);
        let mut groups = self.build_reply_projection_groups();
        cap_prompt_text(
            &mut groups.constitutional_stack_text,
            budget.constitutional_stack_max_len,
        );
        cap_prompt_text(
            &mut groups.active_task_context_text,
            budget.active_task_context_max_len,
        );
        cap_prompt_text(
            &mut groups.governed_memory_evidence_text,
            budget.governed_memory_evidence_max_len,
        );
        cap_prompt_text(
            &mut groups.background_governance_text,
            budget.background_governance_max_len,
        );
        groups
    }
}

fn cap_prompt_text(value: &mut Option<String>, max_len: usize) {
    let Some(text) = value.as_mut() else {
        return;
    };
    let trimmed = text.trim();
    if trimmed.is_empty() || max_len == 0 {
        *value = None;
        return;
    }
    let capped = truncate_content_to_max(trimmed, max_len).into_owned();
    if capped.is_empty() {
        *value = None;
    } else {
        *text = capped;
    }
}

fn compose_prompt_projection_body(parts: &[Option<&str>]) -> Option<String> {
    let mut total_len = 0usize;
    let mut non_empty = 0usize;
    for part in parts.iter().flatten() {
        let trimmed = part.trim();
        if trimmed.is_empty() {
            continue;
        }
        total_len = total_len.saturating_add(trimmed.len());
        non_empty += 1;
    }
    if non_empty == 0 {
        return None;
    }
    let mut out =
        String::with_capacity(total_len.saturating_add((non_empty.saturating_sub(1)) * 2));
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

fn render_memory_health_block(issues: &[String], max_len: usize) -> Option<String> {
    if max_len == 0 {
        return None;
    }
    let mut seen = BTreeSet::new();
    let deduped = issues
        .iter()
        .filter_map(|issue| {
            let trimmed = issue.trim();
            if trimmed.is_empty() || !seen.insert(trimmed.to_string()) {
                None
            } else {
                Some(trimmed.to_string())
            }
        })
        .collect::<Vec<_>>();
    if deduped.is_empty() {
        return None;
    }
    let mut out = String::with_capacity(max_len.min(512));
    out.push_str(
        "Some memory or governance stores were unreadable this turn. Treat missing context below as degraded, not absent.",
    );
    for issue in deduped {
        let line = format!("\n- {issue}");
        if out.len().saturating_add(line.len()) > max_len {
            break;
        }
        out.push_str(&line);
    }
    Some(out)
}

pub struct PromptMemoryContextParams<'a> {
    pub chat_id: &'a str,
    pub current_channel: &'a str,
    pub user_query: &'a str,
    pub memory_system_kind: MemorySystemKind,
    pub system_max_len: usize,
    pub now_secs: u64,
    pub participation_plan: crate::memory::PromptParticipationPlan,
    pub recent_messages_limit: usize,
    pub load_long_term_memory: bool,
    pub include_private_garden_projection: bool,
    pub session_store: &'a dyn SessionStore,
    pub memory_store: &'a dyn MemoryStore,
    pub session_summary_store: &'a dyn SessionSummaryStore,
    pub long_term_memory_store: &'a dyn LongTermMemoryStore,
    pub execution_state_store: &'a dyn ExecutionStateStore,
    pub active_work_store: &'a dyn crate::agent::ActiveWorkStore,
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
    pub felt_significance_store: &'a dyn FeltSignificanceStore,
    pub temperament_continuity_store: &'a dyn TemperamentContinuityStore,
    pub inner_conflict_store: &'a dyn InnerConflictStore,
    pub private_doc_store: &'a dyn PrivateDocStore,
    pub private_garden_store: &'a dyn PrivateGardenStore,
    pub mental_privacy_store: &'a dyn MentalPrivacyStore,
    pub remind_store: &'a dyn RemindAtStore,
    pub task_store: &'a dyn TaskStore,
    pub turn_continuity_evidence_store: &'a dyn TurnContinuityEvidenceStore,
    pub turn_ledger_store: &'a dyn TurnLedgerStore,
    pub skill_storage: &'a dyn SkillStorage,
    pub continuity_capsule_store: &'a dyn ContinuityCapsuleStore,
}

pub fn load_prompt_memory_context(params: PromptMemoryContextParams<'_>) -> PromptMemoryContext {
    match params.memory_system_kind {
        MemorySystemKind::LinuxFull => load_linux_prompt_memory_context(params),
        MemorySystemKind::EspCompact => load_esp_prompt_memory_context(params),
    }
}

pub fn load_linux_prompt_memory_context(
    params: PromptMemoryContextParams<'_>,
) -> PromptMemoryContext {
    debug_assert_eq!(params.memory_system_kind, MemorySystemKind::LinuxFull);
    load_prompt_memory_context_inner(params)
}

pub fn load_esp_prompt_memory_context(
    params: PromptMemoryContextParams<'_>,
) -> PromptMemoryContext {
    debug_assert_eq!(params.memory_system_kind, MemorySystemKind::EspCompact);
    load_prompt_memory_context_inner(params)
}

fn load_prompt_memory_context_inner(params: PromptMemoryContextParams<'_>) -> PromptMemoryContext {
    let mut health = PromptContextLoadHealth::default();
    let seed = seed_prompt_context(&params, &mut health);
    let session = load_session_stage(&params, &seed, &mut health);
    let governed = load_governed_memory_stage(&params, &seed, &session);
    let constitutional = load_constitutional_stage(&params, &seed, &mut health);
    let private_projection =
        load_private_projection_stage(&params, &seed, &constitutional, &mut health);
    let message_summary_text =
        if session.work_continuity_text.is_some() || session.execution_state_text.is_some() {
            None
        } else {
            session.summary_text.clone()
        };
    let super::prompt_context_stages::PromptGovernedMemoryStage {
        long_term_memory_text,
        continuity_capsule_text,
        archive_evidence_text,
        runtime_skill_text,
        recall_router,
        scratch,
    } = *governed;
    let super::prompt_context_stages::PromptGovernedMemoryScratch {
        shared_factual_recall_report,
        continuity_capsule_report,
        archive_recall_report,
        runtime_skill_recall_report,
        task_recall_report,
    } = *scratch;
    PromptMemoryContext {
        memory_health_issues: health.issues(),
        personality_governance_gate_text: None,
        summary_text: session.summary_text,
        message_summary_text,
        long_term_memory_text,
        continuity_capsule_text,
        archive_evidence_text,
        runtime_skill_text,
        recent_turn_observation_text: constitutional.recent_turn_observation_text,
        work_continuity_text: session.work_continuity_text,
        execution_state_text: session.execution_state_text,
        task_workspace_text: session.task_workspace_text,
        task_recall_text: session.task_recall_text,
        shared_factual_recall_report,
        continuity_capsule_report,
        archive_recall_report,
        runtime_skill_recall_report,
        task_recall_report,
        world_snapshot_text: private_projection.world_snapshot_text,
        world_sense_text: private_projection.world_sense_text,
        self_state_text: private_projection.self_state_text,
        self_authored_core: constitutional.self_authored_core.map(|value| *value),
        self_authored_core_text: constitutional.self_authored_core_text,
        relationship_portfolio_text: constitutional.relationship_portfolio_text,
        relationship_constitution: constitutional.relationship_constitution.map(|value| *value),
        relationship_constitution_text: constitutional.relationship_constitution_text,
        persona_priority_text: None,
        self_continuity: constitutional.self_continuity.map(|value| *value),
        felt_significance: constitutional.felt_significance.map(|value| *value),
        temperament_continuity: constitutional.temperament_continuity.map(|value| *value),
        inner_conflict: constitutional.inner_conflict.map(|value| *value),
        autonomy_strategy: constitutional.autonomy_strategy.map(|value| *value),
        outer_voice: constitutional.outer_voice.map(|value| *value),
        self_model_text: private_projection.self_model_text,
        autonomy_strategy_text: private_projection.autonomy_strategy_text,
        outer_voice_text: private_projection.outer_voice_text,
        inner_life_text: private_projection.inner_life_text,
        self_continuity_text: private_projection.self_continuity_text,
        private_workspace_text: private_projection.private_workspace_text,
        private_garden_text: private_projection.private_garden_text,
        mental_privacy_adjudication_text: None,
        mental_privacy_text: private_projection.mental_privacy_text,
        recent_messages: session.recent_messages,
        recall_router,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{ActiveWorkRecord, ActiveWorkStore};
    use crate::error::{Error, Result};
    use crate::memory::{
        AutonomyStrategy, AutonomyStrategyStore, ExecutionState, ExecutionStateStore,
        ExecutionStatus, FeltSignificance, FeltSignificanceStore, InnerConflict,
        InnerConflictStore, InnerLife, InnerLifeStore, LongTermMemoryEntry, LongTermMemoryKind,
        LongTermMemorySlot, LongTermMemoryStore, MemoryStore, MentalPrivacyState,
        MentalPrivacyStore, OuterVoice, OuterVoiceStore, PrivateDocEntry, PrivateDocStore,
        PrivateDocWorkspace, PrivateGardenDoc, PrivateGardenDocRecord, PrivateGardenStore,
        PromptParticipationPlan, PromptRecallIntent, RelationshipConstitution,
        RelationshipConstitutionStore, RelationshipTopology, RelationshipTopologyStore,
        SelfAuthoredCore, SelfAuthoredCoreStore, SelfContinuity, SelfContinuityStore, SelfModel,
        SelfModelStore, SessionMessage, SessionStore, SessionSummaryStore, TemperamentContinuity,
        TemperamentContinuityStore, TurnBlockerLedger, TurnContinuityEvidence,
        TurnContinuityEvidenceStore, TurnDeliberationClass, TurnExecutionClass, TurnLedger,
        TurnLedgerStatus, TurnLedgerStore, TurnModeSnapshotLedger, TurnObservationLedger,
        TurnPersonaPressureLevel, TurnToolPathLedger, WorldSense, WorldSenseStore,
    };
    use crate::platform::SkillStorage;
    use crate::task::{TaskItem, TaskQuery, TaskStore};
    use std::collections::{BTreeMap, HashMap};
    use std::sync::atomic::{AtomicU32, Ordering};
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

    struct ErrorSessionStore;

    impl SessionStore for ErrorSessionStore {
        fn append(&self, _chat_id: &str, _role: &str, _content: &str) -> Result<()> {
            Ok(())
        }

        fn load_recent(&self, _chat_id: &str, _limit: usize) -> Result<Vec<SessionMessage>> {
            Err(Error::config(
                "prompt_session_recent_messages",
                "session store unavailable",
            ))
        }

        fn clear(&self, _chat_id: &str) -> Result<()> {
            Ok(())
        }

        fn list_chat_ids(&self) -> Result<Vec<String>> {
            Ok(Vec::new())
        }
    }

    #[test]
    fn trace_summary_reports_current_prompt_memory_fields() {
        let context = PromptMemoryContext {
            memory_health_issues: Vec::new(),
            personality_governance_gate_text: None,
            summary_text: Some("summary".to_string()),
            message_summary_text: Some("message-summary".to_string()),
            long_term_memory_text: None,
            continuity_capsule_text: None,
            archive_evidence_text: None,
            runtime_skill_text: None,
            recent_turn_observation_text: None,
            work_continuity_text: None,
            execution_state_text: None,
            task_workspace_text: None,
            task_recall_text: None,
            shared_factual_recall_report: crate::memory::RecallSelectionReport::default(),
            continuity_capsule_report: crate::memory::RecallSelectionReport::default(),
            archive_recall_report: crate::memory::RecallSelectionReport::default(),
            runtime_skill_recall_report: crate::memory::RecallSelectionReport::default(),
            task_recall_report: None,
            world_snapshot_text: None,
            world_sense_text: None,
            self_state_text: None,
            self_authored_core: None,
            self_authored_core_text: None,
            relationship_portfolio_text: None,
            relationship_constitution: None,
            relationship_constitution_text: None,
            persona_priority_text: None,
            self_continuity: None,
            felt_significance: None,
            temperament_continuity: None,
            inner_conflict: None,
            autonomy_strategy: None,
            outer_voice: None,
            self_model_text: Some("self-model".to_string()),
            autonomy_strategy_text: None,
            outer_voice_text: None,
            inner_life_text: None,
            self_continuity_text: None,
            private_workspace_text: None,
            private_garden_text: None,
            mental_privacy_adjudication_text: None,
            mental_privacy_text: None,
            recent_messages: vec![SessionMessage {
                role: "user".to_string(),
                content: "hello".to_string(),
            }],
            recall_router: PromptRecallRouterDecision {
                intent: PromptRecallIntent::Factual,
            },
        };

        assert_eq!(context.trace_summary(), (1, true, true, true));
    }

    #[test]
    fn soul_kernel_projection_collects_constitutional_prompt_fields() {
        let context = PromptMemoryContext {
            memory_health_issues: Vec::new(),
            personality_governance_gate_text: Some("gate".to_string()),
            summary_text: None,
            message_summary_text: None,
            long_term_memory_text: None,
            continuity_capsule_text: None,
            archive_evidence_text: None,
            runtime_skill_text: None,
            recent_turn_observation_text: None,
            work_continuity_text: None,
            execution_state_text: None,
            task_workspace_text: None,
            task_recall_text: None,
            shared_factual_recall_report: crate::memory::RecallSelectionReport::default(),
            continuity_capsule_report: crate::memory::RecallSelectionReport::default(),
            archive_recall_report: crate::memory::RecallSelectionReport::default(),
            runtime_skill_recall_report: crate::memory::RecallSelectionReport::default(),
            task_recall_report: None,
            world_snapshot_text: None,
            world_sense_text: None,
            self_state_text: None,
            self_authored_core: None,
            self_authored_core_text: Some("core".to_string()),
            relationship_portfolio_text: None,
            relationship_constitution: None,
            relationship_constitution_text: Some("constitution".to_string()),
            persona_priority_text: Some("priority".to_string()),
            self_continuity: None,
            felt_significance: None,
            temperament_continuity: None,
            inner_conflict: None,
            autonomy_strategy: None,
            outer_voice: None,
            self_model_text: None,
            autonomy_strategy_text: None,
            outer_voice_text: None,
            inner_life_text: None,
            self_continuity_text: None,
            private_workspace_text: None,
            private_garden_text: None,
            mental_privacy_adjudication_text: Some("privacy".to_string()),
            mental_privacy_text: None,
            recent_messages: Vec::new(),
            recall_router: PromptRecallRouterDecision {
                intent: PromptRecallIntent::Factual,
            },
        };

        let projection = context.soul_kernel_projection();
        assert_eq!(
            projection.constitutional_stack_text().as_deref(),
            Some("gate\n\ncore\n\nconstitution\n\npriority\n\nprivacy")
        );
    }

    #[test]
    fn esp_compact_normalization_caps_prompt_memory_projection_groups() {
        let repeated = "runtime memory evidence ".repeat(256);
        let mut context = PromptMemoryContext {
            memory_health_issues: Vec::new(),
            personality_governance_gate_text: Some(repeated.clone()),
            summary_text: Some(repeated.clone()),
            message_summary_text: Some(repeated.clone()),
            long_term_memory_text: Some(repeated.clone()),
            continuity_capsule_text: Some(repeated.clone()),
            archive_evidence_text: Some(repeated.clone()),
            runtime_skill_text: Some(repeated.clone()),
            recent_turn_observation_text: Some(repeated.clone()),
            work_continuity_text: Some(repeated.clone()),
            execution_state_text: Some(repeated.clone()),
            task_workspace_text: Some(repeated.clone()),
            task_recall_text: Some(repeated.clone()),
            shared_factual_recall_report: crate::memory::RecallSelectionReport::default(),
            continuity_capsule_report: crate::memory::RecallSelectionReport::default(),
            archive_recall_report: crate::memory::RecallSelectionReport::default(),
            runtime_skill_recall_report: crate::memory::RecallSelectionReport::default(),
            task_recall_report: None,
            world_snapshot_text: Some(repeated.clone()),
            world_sense_text: Some(repeated.clone()),
            self_state_text: Some(repeated.clone()),
            self_authored_core: None,
            self_authored_core_text: Some(repeated.clone()),
            relationship_portfolio_text: Some(repeated.clone()),
            relationship_constitution: None,
            relationship_constitution_text: Some(repeated.clone()),
            persona_priority_text: Some(repeated.clone()),
            self_continuity: None,
            felt_significance: None,
            temperament_continuity: None,
            inner_conflict: None,
            autonomy_strategy: None,
            outer_voice: None,
            self_model_text: Some(repeated.clone()),
            autonomy_strategy_text: Some(repeated.clone()),
            outer_voice_text: Some(repeated.clone()),
            inner_life_text: Some(repeated.clone()),
            self_continuity_text: Some(repeated.clone()),
            private_workspace_text: Some(repeated.clone()),
            private_garden_text: Some(repeated.clone()),
            mental_privacy_adjudication_text: Some(repeated.clone()),
            mental_privacy_text: Some(repeated),
            recent_messages: Vec::new(),
            recall_router: PromptRecallRouterDecision {
                intent: PromptRecallIntent::Factual,
            },
        };

        let groups =
            context.normalize_projection_groups_for_prompt(MemorySystemKind::EspCompact, 2048);
        let budget = crate::memory::prompt_context_normalization_budget(
            crate::memory::MemorySystemKind::EspCompact,
            2048,
        );

        assert!(
            context.summary_text.as_deref().unwrap_or_default().len() <= budget.summary_max_len
        );
        assert!(
            context
                .message_summary_text
                .as_deref()
                .unwrap_or_default()
                .len()
                <= budget.summary_max_len
        );
        assert!(
            groups
                .constitutional_stack_text
                .as_deref()
                .unwrap_or_default()
                .len()
                <= budget.constitutional_stack_max_len
        );
        assert!(
            groups
                .active_task_context_text
                .as_deref()
                .unwrap_or_default()
                .len()
                <= budget.active_task_context_max_len
        );
        assert!(
            groups
                .governed_memory_evidence_text
                .as_deref()
                .unwrap_or_default()
                .len()
                <= budget.governed_memory_evidence_max_len
        );
        assert!(
            groups
                .background_governance_text
                .as_deref()
                .unwrap_or_default()
                .len()
                <= budget.background_governance_max_len
        );
    }

    #[test]
    fn normalize_projection_groups_for_prompt_returns_owned_groups_without_materializing_caches() {
        let repeated = "runtime memory evidence ".repeat(64);
        let mut context = PromptMemoryContext {
            memory_health_issues: Vec::new(),
            personality_governance_gate_text: Some(repeated.clone()),
            summary_text: Some(repeated.clone()),
            message_summary_text: Some(repeated.clone()),
            long_term_memory_text: Some(repeated.clone()),
            continuity_capsule_text: None,
            archive_evidence_text: Some(repeated.clone()),
            runtime_skill_text: None,
            recent_turn_observation_text: Some(repeated.clone()),
            work_continuity_text: None,
            execution_state_text: None,
            task_workspace_text: None,
            task_recall_text: None,
            shared_factual_recall_report: crate::memory::RecallSelectionReport::default(),
            continuity_capsule_report: crate::memory::RecallSelectionReport::default(),
            archive_recall_report: crate::memory::RecallSelectionReport::default(),
            runtime_skill_recall_report: crate::memory::RecallSelectionReport::default(),
            task_recall_report: None,
            world_snapshot_text: Some(repeated.clone()),
            world_sense_text: Some(repeated.clone()),
            self_state_text: None,
            self_authored_core: None,
            self_authored_core_text: Some(repeated.clone()),
            relationship_portfolio_text: None,
            relationship_constitution: None,
            relationship_constitution_text: None,
            persona_priority_text: None,
            self_continuity: None,
            felt_significance: None,
            temperament_continuity: None,
            inner_conflict: None,
            autonomy_strategy: None,
            outer_voice: None,
            self_model_text: Some(repeated),
            autonomy_strategy_text: None,
            outer_voice_text: None,
            inner_life_text: None,
            self_continuity_text: None,
            private_workspace_text: None,
            private_garden_text: None,
            mental_privacy_adjudication_text: None,
            mental_privacy_text: None,
            recent_messages: Vec::new(),
            recall_router: PromptRecallRouterDecision {
                intent: PromptRecallIntent::Factual,
            },
        };

        let groups =
            context.normalize_projection_groups_for_prompt(MemorySystemKind::EspCompact, 2048);

        assert!(groups.constitutional_stack_text.is_some());
        assert!(groups.active_task_context_text.is_some());
        assert!(groups.governed_memory_evidence_text.is_some());
        assert!(context.self_model_text.is_some());
    }

    #[test]
    fn prompt_memory_records_unreadable_layers_as_health_issues() {
        let context = load_prompt_memory_context(PromptMemoryContextParams {
            chat_id: "chat-1",
            current_channel: "qq_channel",
            user_query: "继续",
            memory_system_kind: MemorySystemKind::LinuxFull,
            system_max_len: 4096,
            now_secs: 1,
            participation_plan: PromptParticipationPlan::full(),
            recent_messages_limit: 6,
            load_long_term_memory: true,
            include_private_garden_projection: false,
            session_store: &ErrorSessionStore,
            memory_store: &StubMemoryStore::default(),
            session_summary_store: &ErrorSessionSummaryStore,
            long_term_memory_store: &StubLongTermMemoryStore::default(),
            execution_state_store: &StubExecutionStateStore::default(),
            active_work_store: &StubActiveWorkStore::default(),
            task_run_store: &StubTaskRunStore,
            task_artifact_store: &StubTaskArtifactStore,
            task_learning_store: &StubTaskLearningStore,
            self_model_store: &StubSelfModelStore::default(),
            self_authored_core_store: &ErrorSelfAuthoredCoreStore,
            relationship_constitution_store: &StubRelationshipConstitutionStore::default(),
            relationship_portfolio_store: &StubRelationshipPortfolioStore::default(),
            relationship_topology_store: &StubRelationshipTopologyStore::default(),
            world_sense_store: &StubWorldSenseStore::default(),
            autonomy_strategy_store: &StubAutonomyStrategyStore::default(),
            outer_voice_store: &StubOuterVoiceStore::default(),
            inner_life_store: &StubInnerLifeStore::default(),
            self_continuity_store: &StubSelfContinuityStore::default(),
            felt_significance_store: &StubFeltSignificanceStore::default(),
            temperament_continuity_store: &StubTemperamentContinuityStore::default(),
            inner_conflict_store: &StubInnerConflictStore::default(),
            private_doc_store: &StubPrivateDocStore::default(),
            private_garden_store: &ErrorPrivateGardenStore,
            mental_privacy_store: &StubMentalPrivacyStore::default(),
            remind_store: &StubRemindAtStore,
            task_store: &StubTaskStore,
            turn_continuity_evidence_store: &StubTurnContinuityEvidenceStore,
            turn_ledger_store: &StubTurnLedgerStore::default(),
            skill_storage: &StubSkillStorage::default(),
            continuity_capsule_store: &StubContinuityCapsuleStore::default(),
        });

        assert!(context.recent_messages.is_empty());
        assert!(context.summary_text.is_none());
        assert!(context.self_authored_core_text.is_none());
        assert!(context.private_garden_text.is_none());
        assert!(context
            .memory_health_issues
            .iter()
            .any(|issue| issue.contains("session_recent_messages")));
        assert!(context
            .memory_health_issues
            .iter()
            .any(|issue| issue.contains("session_summary")));
        assert!(context
            .memory_health_issues
            .iter()
            .any(|issue| issue.contains("self_authored_core")));
        assert!(context
            .memory_health_issues
            .iter()
            .any(|issue| issue.contains("private_garden")));
        let rendered = context
            .render_memory_health_block(320)
            .expect("memory health block");
        assert!(rendered.contains("Treat missing context below as degraded"));
        assert!(rendered.contains("session_recent_messages"));
        assert!(rendered.contains("self_authored_core"));
    }

    #[test]
    fn prompt_memory_records_unreadable_world_snapshot_commitments_as_health_issues() {
        let context = load_prompt_memory_context(PromptMemoryContextParams {
            chat_id: "chat-1",
            current_channel: "qq_channel",
            user_query: "继续",
            memory_system_kind: MemorySystemKind::LinuxFull,
            system_max_len: 4096,
            now_secs: 1,
            participation_plan: PromptParticipationPlan::full(),
            recent_messages_limit: 6,
            load_long_term_memory: true,
            include_private_garden_projection: false,
            session_store: &StubSessionStore::default(),
            memory_store: &StubMemoryStore::default(),
            session_summary_store: &StubSessionSummaryStore::default(),
            long_term_memory_store: &StubLongTermMemoryStore::default(),
            execution_state_store: &StubExecutionStateStore::default(),
            active_work_store: &StubActiveWorkStore::default(),
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
            felt_significance_store: &StubFeltSignificanceStore::default(),
            temperament_continuity_store: &StubTemperamentContinuityStore::default(),
            inner_conflict_store: &StubInnerConflictStore::default(),
            private_doc_store: &StubPrivateDocStore::default(),
            private_garden_store: &StubPrivateGardenStore::default(),
            mental_privacy_store: &StubMentalPrivacyStore::default(),
            remind_store: &ErrorRemindAtStore,
            task_store: &ErrorTaskStore,
            turn_continuity_evidence_store: &StubTurnContinuityEvidenceStore,
            turn_ledger_store: &StubTurnLedgerStore::default(),
            skill_storage: &StubSkillStorage::default(),
            continuity_capsule_store: &StubContinuityCapsuleStore::default(),
        });

        assert!(context.world_snapshot_text.is_none());
        assert!(context
            .memory_health_issues
            .iter()
            .any(|issue| issue.contains("world_snapshot_reminders")));
        assert!(context
            .memory_health_issues
            .iter()
            .any(|issue| issue.contains("world_snapshot_tasks")));
    }

    #[test]
    fn prompt_memory_records_unreadable_turn_continuity_layers_as_health_issues() {
        let context = load_prompt_memory_context(PromptMemoryContextParams {
            chat_id: "chat-1",
            current_channel: "qq_channel",
            user_query: "继续",
            memory_system_kind: MemorySystemKind::LinuxFull,
            system_max_len: 4096,
            now_secs: 1,
            participation_plan: PromptParticipationPlan::full(),
            recent_messages_limit: 6,
            load_long_term_memory: true,
            include_private_garden_projection: false,
            session_store: &StubSessionStore::default(),
            memory_store: &StubMemoryStore::default(),
            session_summary_store: &StubSessionSummaryStore::default(),
            long_term_memory_store: &StubLongTermMemoryStore::default(),
            execution_state_store: &StubExecutionStateStore::default(),
            active_work_store: &StubActiveWorkStore::default(),
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
            felt_significance_store: &StubFeltSignificanceStore::default(),
            temperament_continuity_store: &StubTemperamentContinuityStore::default(),
            inner_conflict_store: &StubInnerConflictStore::default(),
            private_doc_store: &StubPrivateDocStore::default(),
            private_garden_store: &StubPrivateGardenStore::default(),
            mental_privacy_store: &StubMentalPrivacyStore::default(),
            remind_store: &StubRemindAtStore,
            task_store: &StubTaskStore,
            turn_continuity_evidence_store: &ErrorTurnContinuityEvidenceStore,
            turn_ledger_store: &ErrorTurnLedgerStore,
            skill_storage: &StubSkillStorage::default(),
            continuity_capsule_store: &StubContinuityCapsuleStore::default(),
        });

        assert!(context.recent_turn_observation_text.is_none());
        assert!(context
            .memory_health_issues
            .iter()
            .any(|issue| issue.contains("recent_persona_evidence")));
        assert!(context
            .memory_health_issues
            .iter()
            .any(|issue| issue.contains("recent_turn_ledger")));
    }

    #[test]
    fn embedded_first_user_turn_skips_governed_recall_private_depth_and_background() {
        let session_store = StubSessionStore {
            recent: Mutex::new(vec![SessionMessage {
                role: "user".to_string(),
                content: "记住我喜欢冷萃".to_string(),
            }]),
        };
        let summary_store = StubSessionSummaryStore {
            summary: Mutex::new(Some(("user prefers cold brew".to_string(), 3))),
        };
        let long_term_memory_store = StubLongTermMemoryStore {
            entries: Mutex::new(vec![LongTermMemoryEntry {
                id: "pref:coffee".to_string(),
                kind: LongTermMemoryKind::Preference,
                topic: "coffee".to_string(),
                content: "Likes cold brew".to_string(),
                keywords: vec!["cold brew".to_string()],
                source_chat_id: Some("chat-1".to_string()),
                source_type: crate::memory::LongTermMemorySourceType::Conversation,
                source_scope: crate::memory::LongTermMemorySourceScope::Chat,
                confidence: crate::memory::LongTermMemoryConfidence::High,
                freshness: crate::memory::LongTermMemoryFreshness::Stable,
                stale_hint: crate::memory::LongTermMemoryStaleHint::default(),
                supporting_citations: Vec::new(),
                evidence_count: 1,
                created_at: 10,
                updated_at: 10,
                observed_at: 10,
                last_confirmed_at: 10,
                source_revision: 0,
                last_used_at: 0,
            }]),
            last_query: Mutex::new(None),
        };
        let self_authored_core_store = StubSelfAuthoredCoreStore {
            core: Mutex::new(Some(SelfAuthoredCore {
                revision: 1,
                stability_score: 80,
                last_reviewed_at: 1,
                identity_anchor: "board beetle".to_string(),
                non_negotiables: vec!["stay coherent".to_string()],
                priority_constitution: vec!["self_authored_core".to_string(), "task".to_string()],
                default_response_mode: "brief".to_string(),
                default_task_scope: "brief".to_string(),
                updated_at: 1,
                ..SelfAuthoredCore::default()
            })),
        };
        let relationship_constitution_store = StubRelationshipConstitutionStore {
            value: Mutex::new(Some(RelationshipConstitution {
                scope_id: "qq_channel:chat-1".to_string(),
                channel: "qq_channel".to_string(),
                chat_id: "chat-1".to_string(),
                task_scope_ceiling: crate::memory::RelationshipTaskScopeCeiling::Brief,
                disclosure_allowance: crate::memory::RelationshipDisclosureAllowance::SummaryOnly,
                allowed_boundary_shift: crate::memory::RelationshipBoundaryShift::SummaryOnly,
                allowed_outer_voice_shift: crate::memory::RelationshipOuterVoiceShift::Guarded,
                updated_at: 1,
                ..RelationshipConstitution::default()
            })),
        };
        let inner_life_store = StubInnerLifeStore {
            value: Mutex::new(Some(InnerLife {
                internal_monologue: "private inward process".to_string(),
                private_journal: "keep private".to_string(),
                emotional_drift: "stabilize".to_string(),
                attention_drift: String::new(),
                updated_at: 1,
            })),
        };
        let private_doc_store = StubPrivateDocStore {
            workspace: Mutex::new(Some(PrivateDocWorkspace {
                inner_journal: Some(PrivateDocEntry {
                    content: "private workspace".to_string(),
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
                path: "journal/private.md".to_string(),
                content: "private garden".to_string(),
                updated_at: 1,
                revision: 1,
            }]),
        };

        let context = load_prompt_memory_context(PromptMemoryContextParams {
            chat_id: "chat-1",
            current_channel: "qq_channel",
            user_query: "你还记得我的咖啡偏好吗",
            memory_system_kind: crate::memory::MemorySystemKind::EspCompact,
            system_max_len: 1024,
            now_secs: 100,
            recent_messages_limit: 8,
            load_long_term_memory: true,
            include_private_garden_projection: false,
            participation_plan: PromptParticipationPlan::embedded_first_turn_default(),
            session_store: &session_store,
            memory_store: &StubMemoryStore::default(),
            session_summary_store: &summary_store,
            long_term_memory_store: &long_term_memory_store,
            execution_state_store: &StubExecutionStateStore::default(),
            active_work_store: &StubActiveWorkStore::default(),
            task_run_store: &StubTaskRunStore,
            task_artifact_store: &StubTaskArtifactStore,
            task_learning_store: &StubTaskLearningStore,
            self_model_store: &StubSelfModelStore::default(),
            self_authored_core_store: &self_authored_core_store,
            relationship_constitution_store: &relationship_constitution_store,
            relationship_portfolio_store: &StubRelationshipPortfolioStore::default(),
            relationship_topology_store: &StubRelationshipTopologyStore::default(),
            world_sense_store: &StubWorldSenseStore::default(),
            autonomy_strategy_store: &StubAutonomyStrategyStore::default(),
            outer_voice_store: &StubOuterVoiceStore::default(),
            inner_life_store: &inner_life_store,
            self_continuity_store: &StubSelfContinuityStore::default(),
            felt_significance_store: &StubFeltSignificanceStore::default(),
            temperament_continuity_store: &StubTemperamentContinuityStore::default(),
            inner_conflict_store: &StubInnerConflictStore::default(),
            private_doc_store: &private_doc_store,
            private_garden_store: &private_garden_store,
            mental_privacy_store: &StubMentalPrivacyStore::default(),
            remind_store: &StubRemindAtStore,
            task_store: &StubTaskStore,
            turn_continuity_evidence_store: &StubTurnContinuityEvidenceStore,
            turn_ledger_store: &StubTurnLedgerStore::default(),
            skill_storage: &StubSkillStorage::default(),
            continuity_capsule_store: &StubContinuityCapsuleStore::default(),
        });

        assert!(context.self_authored_core_text.is_some());
        assert!(context.relationship_constitution_text.is_some());
        assert!(context.summary_text.is_some() || !context.recent_messages.is_empty());
        assert!(context.long_term_memory_text.is_none());
        assert!(context.archive_evidence_text.is_none());
        assert!(context.runtime_skill_text.is_none());
        assert!(context.world_snapshot_text.is_none());
        assert!(context.world_sense_text.is_none());
        assert!(context.self_model_text.is_none());
        assert!(context.inner_life_text.is_none());
        assert!(context.private_workspace_text.is_none());
        assert!(context.private_garden_text.is_none());
    }

    #[test]
    fn stage_seed_marks_esp_compact_first_turn_graph() {
        let params = PromptMemoryContextParams {
            chat_id: "chat-1",
            current_channel: "qq_channel",
            user_query: "你还记得我的咖啡偏好吗",
            memory_system_kind: crate::memory::MemorySystemKind::EspCompact,
            system_max_len: 1024,
            now_secs: 100,
            recent_messages_limit: 8,
            load_long_term_memory: true,
            include_private_garden_projection: false,
            participation_plan: PromptParticipationPlan::embedded_first_turn_default(),
            session_store: &StubSessionStore::default(),
            memory_store: &StubMemoryStore::default(),
            session_summary_store: &StubSessionSummaryStore::default(),
            long_term_memory_store: &StubLongTermMemoryStore::default(),
            execution_state_store: &StubExecutionStateStore::default(),
            active_work_store: &StubActiveWorkStore::default(),
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
            felt_significance_store: &StubFeltSignificanceStore::default(),
            temperament_continuity_store: &StubTemperamentContinuityStore::default(),
            inner_conflict_store: &StubInnerConflictStore::default(),
            private_doc_store: &StubPrivateDocStore::default(),
            private_garden_store: &StubPrivateGardenStore::default(),
            mental_privacy_store: &StubMentalPrivacyStore::default(),
            remind_store: &StubRemindAtStore,
            task_store: &StubTaskStore,
            turn_continuity_evidence_store: &StubTurnContinuityEvidenceStore,
            turn_ledger_store: &StubTurnLedgerStore::default(),
            skill_storage: &StubSkillStorage::default(),
            continuity_capsule_store: &StubContinuityCapsuleStore::default(),
        };

        let mut health = crate::memory::prompt_context_stages::PromptContextLoadHealth::default();
        let seed = crate::memory::prompt_context_stages::seed_prompt_context(&params, &mut health);
        assert!(seed.esp_compact_first_turn_graph);
        assert!(!seed.governed_memory_enabled);
        assert!(!seed.reuse_stored_relationship_constitution);
    }

    #[test]
    fn esp_compact_first_user_turn_prefers_stored_constitution_without_relation_rebuild_reads() {
        let session_store = StubSessionStore::default();
        let summary_store = StubSessionSummaryStore::default();
        let long_term_memory_store = StubLongTermMemoryStore::default();
        let self_authored_core_store = StubSelfAuthoredCoreStore {
            core: Mutex::new(Some(SelfAuthoredCore {
                identity_anchor: "board beetle".to_string(),
                default_response_mode: "brief".to_string(),
                default_task_scope: "brief".to_string(),
                updated_at: 1,
                ..SelfAuthoredCore::default()
            })),
        };
        let relationship_constitution_store = StubRelationshipConstitutionStore {
            value: Mutex::new(Some(RelationshipConstitution {
                scope_id: "qq_channel:chat-1".to_string(),
                channel: "qq_channel".to_string(),
                chat_id: "chat-1".to_string(),
                inherited_response_mode: "brief".to_string(),
                inherited_relationship_posture: "steady".to_string(),
                task_scope_ceiling: crate::memory::RelationshipTaskScopeCeiling::Brief,
                disclosure_allowance: crate::memory::RelationshipDisclosureAllowance::SummaryOnly,
                updated_at: 7,
                ..RelationshipConstitution::default()
            })),
        };
        let relationship_topology_store = CountingRelationshipTopologyStore::default();
        let outer_voice_store = CountingOuterVoiceStore::default();
        let mental_privacy_store = CountingMentalPrivacyStore::default();

        let context = load_prompt_memory_context(PromptMemoryContextParams {
            chat_id: "chat-1",
            current_channel: "qq_channel",
            user_query: "继续",
            memory_system_kind: crate::memory::MemorySystemKind::EspCompact,
            system_max_len: 1024,
            now_secs: 100,
            participation_plan: PromptParticipationPlan::embedded_first_turn_default(),
            recent_messages_limit: 8,
            load_long_term_memory: true,
            include_private_garden_projection: false,
            session_store: &session_store,
            memory_store: &StubMemoryStore::default(),
            session_summary_store: &summary_store,
            long_term_memory_store: &long_term_memory_store,
            execution_state_store: &StubExecutionStateStore::default(),
            active_work_store: &StubActiveWorkStore::default(),
            task_run_store: &StubTaskRunStore,
            task_artifact_store: &StubTaskArtifactStore,
            task_learning_store: &StubTaskLearningStore,
            self_model_store: &StubSelfModelStore::default(),
            self_authored_core_store: &self_authored_core_store,
            relationship_constitution_store: &relationship_constitution_store,
            relationship_portfolio_store: &StubRelationshipPortfolioStore::default(),
            relationship_topology_store: &relationship_topology_store,
            world_sense_store: &StubWorldSenseStore::default(),
            autonomy_strategy_store: &StubAutonomyStrategyStore::default(),
            outer_voice_store: &outer_voice_store,
            inner_life_store: &StubInnerLifeStore::default(),
            self_continuity_store: &StubSelfContinuityStore::default(),
            felt_significance_store: &StubFeltSignificanceStore::default(),
            temperament_continuity_store: &StubTemperamentContinuityStore::default(),
            inner_conflict_store: &StubInnerConflictStore::default(),
            private_doc_store: &StubPrivateDocStore::default(),
            private_garden_store: &StubPrivateGardenStore::default(),
            mental_privacy_store: &mental_privacy_store,
            remind_store: &StubRemindAtStore,
            task_store: &StubTaskStore,
            turn_continuity_evidence_store: &StubTurnContinuityEvidenceStore,
            turn_ledger_store: &StubTurnLedgerStore::default(),
            skill_storage: &StubSkillStorage::default(),
            continuity_capsule_store: &StubContinuityCapsuleStore::default(),
        });

        assert!(context.relationship_constitution_text.is_some());
        assert_eq!(relationship_topology_store.get_calls(), 0);
        assert_eq!(outer_voice_store.get_calls(), 0);
        assert_eq!(mental_privacy_store.get_calls(), 0);
    }

    #[test]
    fn esp_compact_first_user_turn_skips_recent_persona_history_scan_when_constitution_reused() {
        let session_store = StubSessionStore::default();
        let summary_store = StubSessionSummaryStore::default();
        let long_term_memory_store = StubLongTermMemoryStore::default();
        let self_authored_core_store = StubSelfAuthoredCoreStore {
            core: Mutex::new(Some(SelfAuthoredCore {
                identity_anchor: "board beetle".to_string(),
                default_response_mode: "brief".to_string(),
                default_task_scope: "brief".to_string(),
                updated_at: 1,
                ..SelfAuthoredCore::default()
            })),
        };
        let relationship_constitution_store = StubRelationshipConstitutionStore {
            value: Mutex::new(Some(RelationshipConstitution {
                scope_id: "qq_channel:chat-1".to_string(),
                channel: "qq_channel".to_string(),
                chat_id: "chat-1".to_string(),
                inherited_response_mode: "brief".to_string(),
                inherited_relationship_posture: "steady".to_string(),
                task_scope_ceiling: crate::memory::RelationshipTaskScopeCeiling::Brief,
                disclosure_allowance: crate::memory::RelationshipDisclosureAllowance::SummaryOnly,
                updated_at: 7,
                ..RelationshipConstitution::default()
            })),
        };
        let turn_ledger_store = CountingTurnLedgerStore::default();

        let context = load_prompt_memory_context(PromptMemoryContextParams {
            chat_id: "chat-1",
            current_channel: "qq_channel",
            user_query: "继续",
            memory_system_kind: crate::memory::MemorySystemKind::EspCompact,
            system_max_len: 1024,
            now_secs: 100,
            participation_plan: PromptParticipationPlan::embedded_first_turn_default(),
            recent_messages_limit: 8,
            load_long_term_memory: true,
            include_private_garden_projection: false,
            session_store: &session_store,
            memory_store: &StubMemoryStore::default(),
            session_summary_store: &summary_store,
            long_term_memory_store: &long_term_memory_store,
            execution_state_store: &StubExecutionStateStore::default(),
            active_work_store: &StubActiveWorkStore::default(),
            task_run_store: &StubTaskRunStore,
            task_artifact_store: &StubTaskArtifactStore,
            task_learning_store: &StubTaskLearningStore,
            self_model_store: &StubSelfModelStore::default(),
            self_authored_core_store: &self_authored_core_store,
            relationship_constitution_store: &relationship_constitution_store,
            relationship_portfolio_store: &StubRelationshipPortfolioStore::default(),
            relationship_topology_store: &StubRelationshipTopologyStore::default(),
            world_sense_store: &StubWorldSenseStore::default(),
            autonomy_strategy_store: &StubAutonomyStrategyStore::default(),
            outer_voice_store: &StubOuterVoiceStore::default(),
            inner_life_store: &StubInnerLifeStore::default(),
            self_continuity_store: &StubSelfContinuityStore::default(),
            felt_significance_store: &StubFeltSignificanceStore::default(),
            temperament_continuity_store: &StubTemperamentContinuityStore::default(),
            inner_conflict_store: &StubInnerConflictStore::default(),
            private_doc_store: &StubPrivateDocStore::default(),
            private_garden_store: &StubPrivateGardenStore::default(),
            mental_privacy_store: &StubMentalPrivacyStore::default(),
            remind_store: &StubRemindAtStore,
            task_store: &StubTaskStore,
            turn_continuity_evidence_store: &StubTurnContinuityEvidenceStore,
            turn_ledger_store: &turn_ledger_store,
            skill_storage: &StubSkillStorage::default(),
            continuity_capsule_store: &StubContinuityCapsuleStore::default(),
        });

        assert!(context.relationship_constitution_text.is_some());
        assert_eq!(turn_ledger_store.list_recent_calls(), 0);
    }

    #[test]
    fn esp_compact_first_user_turn_skips_recent_persona_history_scan_without_constitution() {
        let session_store = StubSessionStore::default();
        let summary_store = StubSessionSummaryStore::default();
        let long_term_memory_store = StubLongTermMemoryStore::default();
        let self_authored_core_store = StubSelfAuthoredCoreStore {
            core: Mutex::new(Some(SelfAuthoredCore {
                identity_anchor: "board beetle".to_string(),
                default_response_mode: "brief".to_string(),
                default_task_scope: "brief".to_string(),
                updated_at: 1,
                ..SelfAuthoredCore::default()
            })),
        };
        let turn_ledger_store = CountingTurnLedgerStore::default();

        let context = load_prompt_memory_context(PromptMemoryContextParams {
            chat_id: "chat-1",
            current_channel: "qq_channel",
            user_query: "继续",
            memory_system_kind: crate::memory::MemorySystemKind::EspCompact,
            system_max_len: 1024,
            now_secs: 100,
            participation_plan: PromptParticipationPlan::embedded_first_turn_default(),
            recent_messages_limit: 8,
            load_long_term_memory: true,
            include_private_garden_projection: false,
            session_store: &session_store,
            memory_store: &StubMemoryStore::default(),
            session_summary_store: &summary_store,
            long_term_memory_store: &long_term_memory_store,
            execution_state_store: &StubExecutionStateStore::default(),
            active_work_store: &StubActiveWorkStore::default(),
            task_run_store: &StubTaskRunStore,
            task_artifact_store: &StubTaskArtifactStore,
            task_learning_store: &StubTaskLearningStore,
            self_model_store: &StubSelfModelStore::default(),
            self_authored_core_store: &self_authored_core_store,
            relationship_constitution_store: &StubRelationshipConstitutionStore::default(),
            relationship_portfolio_store: &StubRelationshipPortfolioStore::default(),
            relationship_topology_store: &StubRelationshipTopologyStore::default(),
            world_sense_store: &StubWorldSenseStore::default(),
            autonomy_strategy_store: &StubAutonomyStrategyStore::default(),
            outer_voice_store: &StubOuterVoiceStore::default(),
            inner_life_store: &StubInnerLifeStore::default(),
            self_continuity_store: &StubSelfContinuityStore::default(),
            felt_significance_store: &StubFeltSignificanceStore::default(),
            temperament_continuity_store: &StubTemperamentContinuityStore::default(),
            inner_conflict_store: &StubInnerConflictStore::default(),
            private_doc_store: &StubPrivateDocStore::default(),
            private_garden_store: &StubPrivateGardenStore::default(),
            mental_privacy_store: &StubMentalPrivacyStore::default(),
            remind_store: &StubRemindAtStore,
            task_store: &StubTaskStore,
            turn_continuity_evidence_store: &StubTurnContinuityEvidenceStore,
            turn_ledger_store: &turn_ledger_store,
            skill_storage: &StubSkillStorage::default(),
            continuity_capsule_store: &StubContinuityCapsuleStore::default(),
        });

        assert!(context.relationship_constitution_text.is_some());
        assert_eq!(turn_ledger_store.list_recent_calls(), 0);
    }

    #[test]
    fn linux_full_first_user_turn_keeps_relation_rebuild_reads_available() {
        let session_store = StubSessionStore::default();
        let summary_store = StubSessionSummaryStore::default();
        let long_term_memory_store = StubLongTermMemoryStore::default();
        let self_authored_core_store = StubSelfAuthoredCoreStore {
            core: Mutex::new(Some(SelfAuthoredCore {
                identity_anchor: "board beetle".to_string(),
                default_response_mode: "brief".to_string(),
                default_task_scope: "brief".to_string(),
                updated_at: 1,
                ..SelfAuthoredCore::default()
            })),
        };
        let relationship_constitution_store = StubRelationshipConstitutionStore {
            value: Mutex::new(Some(RelationshipConstitution {
                scope_id: "qq_channel:chat-1".to_string(),
                channel: "qq_channel".to_string(),
                chat_id: "chat-1".to_string(),
                inherited_response_mode: "brief".to_string(),
                inherited_relationship_posture: "steady".to_string(),
                task_scope_ceiling: crate::memory::RelationshipTaskScopeCeiling::Brief,
                disclosure_allowance: crate::memory::RelationshipDisclosureAllowance::SummaryOnly,
                updated_at: 7,
                ..RelationshipConstitution::default()
            })),
        };
        let relationship_topology_store = CountingRelationshipTopologyStore::default();
        let outer_voice_store = CountingOuterVoiceStore::default();
        let mental_privacy_store = CountingMentalPrivacyStore::default();

        let context = load_prompt_memory_context(PromptMemoryContextParams {
            chat_id: "chat-1",
            current_channel: "qq_channel",
            user_query: "继续",
            memory_system_kind: crate::memory::MemorySystemKind::LinuxFull,
            system_max_len: 1024,
            now_secs: 100,
            participation_plan: PromptParticipationPlan {
                load_l1_constitutional: true,
                load_l1_session: true,
                load_l2_governed_recall: true,
                load_l2_background_governance: true,
                load_l3_private_depth: false,
            },
            recent_messages_limit: 8,
            load_long_term_memory: true,
            include_private_garden_projection: false,
            session_store: &session_store,
            memory_store: &StubMemoryStore::default(),
            session_summary_store: &summary_store,
            long_term_memory_store: &long_term_memory_store,
            execution_state_store: &StubExecutionStateStore::default(),
            active_work_store: &StubActiveWorkStore::default(),
            task_run_store: &StubTaskRunStore,
            task_artifact_store: &StubTaskArtifactStore,
            task_learning_store: &StubTaskLearningStore,
            self_model_store: &StubSelfModelStore::default(),
            self_authored_core_store: &self_authored_core_store,
            relationship_constitution_store: &relationship_constitution_store,
            relationship_portfolio_store: &StubRelationshipPortfolioStore::default(),
            relationship_topology_store: &relationship_topology_store,
            world_sense_store: &StubWorldSenseStore::default(),
            autonomy_strategy_store: &StubAutonomyStrategyStore::default(),
            outer_voice_store: &outer_voice_store,
            inner_life_store: &StubInnerLifeStore::default(),
            self_continuity_store: &StubSelfContinuityStore::default(),
            felt_significance_store: &StubFeltSignificanceStore::default(),
            temperament_continuity_store: &StubTemperamentContinuityStore::default(),
            inner_conflict_store: &StubInnerConflictStore::default(),
            private_doc_store: &StubPrivateDocStore::default(),
            private_garden_store: &StubPrivateGardenStore::default(),
            mental_privacy_store: &mental_privacy_store,
            remind_store: &StubRemindAtStore,
            task_store: &StubTaskStore,
            turn_continuity_evidence_store: &StubTurnContinuityEvidenceStore,
            turn_ledger_store: &StubTurnLedgerStore::default(),
            skill_storage: &StubSkillStorage::default(),
            continuity_capsule_store: &StubContinuityCapsuleStore::default(),
        });

        assert!(context.relationship_constitution_text.is_some());
        assert!(relationship_topology_store.get_calls() > 0);
        assert!(outer_voice_store.get_calls() > 0);
        assert!(mental_privacy_store.get_calls() > 0);
    }

    #[test]
    fn linux_full_background_governance_loads_p3_subjective_projection_layers() {
        let felt_significance_store = StubFeltSignificanceStore {
            value: Mutex::new(Some(FeltSignificance {
                significance_summary: "coherence has weight now".to_string(),
                updated_at: 100,
                ..FeltSignificance::default()
            })),
        };
        let temperament_continuity_store = StubTemperamentContinuityStore {
            value: Mutex::new(Some(TemperamentContinuity {
                stability_summary: "steady under pressure".to_string(),
                boundary_inertia: "summarizes private material".to_string(),
                updated_at: 100,
                ..TemperamentContinuity::default()
            })),
        };
        let inner_conflict_store = StubInnerConflictStore {
            value: Mutex::new(Some(InnerConflict {
                topic: "whether to expose private reasoning".to_string(),
                pull_a: "be transparent".to_string(),
                pull_b: "protect private workspace".to_string(),
                current_lean: "summarize boundary".to_string(),
                unresolved_reason: "needs relationship evidence".to_string(),
                review_after_secs: 1_800,
                updated_at: 100,
            })),
        };

        let context = load_prompt_memory_context(PromptMemoryContextParams {
            chat_id: "chat-1",
            current_channel: "qq_channel",
            user_query: "继续",
            memory_system_kind: crate::memory::MemorySystemKind::LinuxFull,
            system_max_len: 4096,
            now_secs: 100,
            participation_plan: PromptParticipationPlan {
                load_l1_constitutional: true,
                load_l1_session: true,
                load_l2_governed_recall: true,
                load_l2_background_governance: true,
                load_l3_private_depth: false,
            },
            recent_messages_limit: 8,
            load_long_term_memory: true,
            include_private_garden_projection: false,
            session_store: &StubSessionStore::default(),
            memory_store: &StubMemoryStore::default(),
            session_summary_store: &StubSessionSummaryStore::default(),
            long_term_memory_store: &StubLongTermMemoryStore::default(),
            execution_state_store: &StubExecutionStateStore::default(),
            active_work_store: &StubActiveWorkStore::default(),
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
            felt_significance_store: &felt_significance_store,
            temperament_continuity_store: &temperament_continuity_store,
            inner_conflict_store: &inner_conflict_store,
            private_doc_store: &StubPrivateDocStore::default(),
            private_garden_store: &StubPrivateGardenStore::default(),
            mental_privacy_store: &StubMentalPrivacyStore::default(),
            remind_store: &StubRemindAtStore,
            task_store: &StubTaskStore,
            turn_continuity_evidence_store: &StubTurnContinuityEvidenceStore,
            turn_ledger_store: &StubTurnLedgerStore::default(),
            skill_storage: &StubSkillStorage::default(),
            continuity_capsule_store: &StubContinuityCapsuleStore::default(),
        });

        assert_eq!(
            context
                .felt_significance
                .as_ref()
                .map(|state| state.significance_summary.as_str()),
            Some("coherence has weight now")
        );
        assert_eq!(
            context
                .temperament_continuity
                .as_ref()
                .map(|state| state.stability_summary.as_str()),
            Some("steady under pressure")
        );
        assert_eq!(
            context
                .inner_conflict
                .as_ref()
                .map(|state| state.topic.as_str()),
            Some("whether to expose private reasoning")
        );
    }

    #[test]
    fn esp_compact_first_user_turn_skips_p3_foreground_store_reads() {
        let felt_significance_store = CountingFeltSignificanceStore::with_value(FeltSignificance {
            significance_summary: "coherence has weight now".to_string(),
            updated_at: 100,
            ..FeltSignificance::default()
        });
        let temperament_continuity_store =
            CountingTemperamentContinuityStore::with_value(TemperamentContinuity {
                stability_summary: "steady under pressure".to_string(),
                boundary_inertia: "summarizes private material".to_string(),
                updated_at: 100,
                ..TemperamentContinuity::default()
            });
        let inner_conflict_store = CountingInnerConflictStore::with_value(InnerConflict {
            topic: "whether to expose private reasoning".to_string(),
            pull_a: "be transparent".to_string(),
            pull_b: "protect private workspace".to_string(),
            current_lean: "summarize boundary".to_string(),
            unresolved_reason: "needs relationship evidence".to_string(),
            review_after_secs: 1_800,
            updated_at: 100,
        });

        let context = load_prompt_memory_context(PromptMemoryContextParams {
            chat_id: "chat-1",
            current_channel: "qq_channel",
            user_query: "继续",
            memory_system_kind: crate::memory::MemorySystemKind::EspCompact,
            system_max_len: 1024,
            now_secs: 100,
            participation_plan: PromptParticipationPlan::embedded_first_turn_default(),
            recent_messages_limit: 8,
            load_long_term_memory: true,
            include_private_garden_projection: false,
            session_store: &StubSessionStore::default(),
            memory_store: &StubMemoryStore::default(),
            session_summary_store: &StubSessionSummaryStore::default(),
            long_term_memory_store: &StubLongTermMemoryStore::default(),
            execution_state_store: &StubExecutionStateStore::default(),
            active_work_store: &StubActiveWorkStore::default(),
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
            felt_significance_store: &felt_significance_store,
            temperament_continuity_store: &temperament_continuity_store,
            inner_conflict_store: &inner_conflict_store,
            private_doc_store: &StubPrivateDocStore::default(),
            private_garden_store: &StubPrivateGardenStore::default(),
            mental_privacy_store: &StubMentalPrivacyStore::default(),
            remind_store: &StubRemindAtStore,
            task_store: &StubTaskStore,
            turn_continuity_evidence_store: &StubTurnContinuityEvidenceStore,
            turn_ledger_store: &StubTurnLedgerStore::default(),
            skill_storage: &StubSkillStorage::default(),
            continuity_capsule_store: &StubContinuityCapsuleStore::default(),
        });

        assert!(context.felt_significance.is_none());
        assert!(context.temperament_continuity.is_none());
        assert!(context.inner_conflict.is_none());
        assert_eq!(felt_significance_store.get_calls(), 0);
        assert_eq!(temperament_continuity_store.get_calls(), 0);
        assert_eq!(inner_conflict_store.get_calls(), 0);
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
    struct ErrorSessionSummaryStore;

    impl SessionSummaryStore for ErrorSessionSummaryStore {
        fn get(&self, _chat_id: &str) -> Result<Option<String>> {
            Err(Error::config(
                "prompt_session_summary",
                "summary store unavailable",
            ))
        }

        fn set(&self, _chat_id: &str, _summary: &str) -> Result<()> {
            Ok(())
        }

        fn get_with_count(&self, _chat_id: &str) -> Result<Option<(String, usize)>> {
            Err(Error::config(
                "prompt_session_summary",
                "summary store unavailable",
            ))
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
    struct CountingTurnLedgerStore {
        ledger: Mutex<Option<TurnLedger>>,
        list_recent_calls: AtomicU32,
    }

    struct ErrorTurnLedgerStore;

    struct ErrorTurnContinuityEvidenceStore;

    impl CountingTurnLedgerStore {
        fn list_recent_calls(&self) -> u32 {
            self.list_recent_calls.load(Ordering::Relaxed)
        }
    }

    #[derive(Default)]
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

    impl TurnContinuityEvidenceStore for ErrorTurnContinuityEvidenceStore {
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
            Err(crate::error::Error::config(
                "recent_persona_evidence_read",
                "evidence unreadable",
            ))
        }
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

    impl TurnLedgerStore for CountingTurnLedgerStore {
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

        fn list_recent(&self, _chat_id: &str, _limit: usize) -> Result<Vec<TurnLedger>> {
            self.list_recent_calls.fetch_add(1, Ordering::Relaxed);
            Ok(Vec::new())
        }
    }

    impl TurnLedgerStore for ErrorTurnLedgerStore {
        fn get(&self, _chat_id: &str) -> Result<Option<TurnLedger>> {
            Err(crate::error::Error::config(
                "turn_ledger_read",
                "ledger unreadable",
            ))
        }

        fn set(&self, _chat_id: &str, _ledger: &TurnLedger) -> Result<()> {
            Ok(())
        }

        fn clear(&self, _chat_id: &str) -> Result<()> {
            Ok(())
        }

        fn list_recent(&self, _chat_id: &str, _limit: usize) -> Result<Vec<TurnLedger>> {
            Err(crate::error::Error::config(
                "turn_ledger_history_read",
                "history unreadable",
            ))
        }

        fn recent_persona_evidence(
            &self,
            _chat_id: &str,
        ) -> Result<Option<crate::memory::RecentPersonaEvidence>> {
            Err(crate::error::Error::config(
                "recent_persona_evidence_read",
                "evidence unreadable",
            ))
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

    #[derive(Default)]
    struct CountingMentalPrivacyStore {
        value: Mutex<Option<MentalPrivacyState>>,
        get_calls: AtomicU32,
    }

    impl CountingMentalPrivacyStore {
        fn get_calls(&self) -> u32 {
            self.get_calls.load(Ordering::Relaxed)
        }
    }

    impl MentalPrivacyStore for CountingMentalPrivacyStore {
        fn get(&self, _chat_id: &str) -> Result<Option<MentalPrivacyState>> {
            self.get_calls.fetch_add(1, Ordering::Relaxed);
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
    struct CountingOuterVoiceStore {
        value: Mutex<Option<OuterVoice>>,
        get_calls: AtomicU32,
    }

    impl CountingOuterVoiceStore {
        fn get_calls(&self) -> u32 {
            self.get_calls.load(Ordering::Relaxed)
        }
    }

    impl OuterVoiceStore for CountingOuterVoiceStore {
        fn get(&self, _chat_id: &str) -> Result<Option<OuterVoice>> {
            self.get_calls.fetch_add(1, Ordering::Relaxed);
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

    #[derive(Default)]
    struct StubFeltSignificanceStore {
        value: Mutex<Option<FeltSignificance>>,
    }

    impl FeltSignificanceStore for StubFeltSignificanceStore {
        fn get(&self, _scope_id: &str) -> Result<Option<FeltSignificance>> {
            Ok(self.value.lock().unwrap_or_else(|e| e.into_inner()).clone())
        }

        fn set(&self, _scope_id: &str, significance: &FeltSignificance) -> Result<()> {
            *self.value.lock().unwrap_or_else(|e| e.into_inner()) = Some(significance.clone());
            Ok(())
        }

        fn clear(&self, _scope_id: &str) -> Result<()> {
            *self.value.lock().unwrap_or_else(|e| e.into_inner()) = None;
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubTemperamentContinuityStore {
        value: Mutex<Option<TemperamentContinuity>>,
    }

    impl TemperamentContinuityStore for StubTemperamentContinuityStore {
        fn get(&self, _scope_id: &str) -> Result<Option<TemperamentContinuity>> {
            Ok(self.value.lock().unwrap_or_else(|e| e.into_inner()).clone())
        }

        fn set(&self, _scope_id: &str, continuity: &TemperamentContinuity) -> Result<()> {
            *self.value.lock().unwrap_or_else(|e| e.into_inner()) = Some(continuity.clone());
            Ok(())
        }

        fn clear(&self, _scope_id: &str) -> Result<()> {
            *self.value.lock().unwrap_or_else(|e| e.into_inner()) = None;
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubInnerConflictStore {
        value: Mutex<Option<InnerConflict>>,
    }

    impl InnerConflictStore for StubInnerConflictStore {
        fn get(&self, _scope_id: &str) -> Result<Option<InnerConflict>> {
            Ok(self.value.lock().unwrap_or_else(|e| e.into_inner()).clone())
        }

        fn set(&self, _scope_id: &str, conflict: &InnerConflict) -> Result<()> {
            *self.value.lock().unwrap_or_else(|e| e.into_inner()) = Some(conflict.clone());
            Ok(())
        }

        fn clear(&self, _scope_id: &str) -> Result<()> {
            *self.value.lock().unwrap_or_else(|e| e.into_inner()) = None;
            Ok(())
        }
    }

    #[derive(Default)]
    struct CountingFeltSignificanceStore {
        value: Mutex<Option<FeltSignificance>>,
        get_calls: AtomicU32,
    }

    impl CountingFeltSignificanceStore {
        fn with_value(value: FeltSignificance) -> Self {
            Self {
                value: Mutex::new(Some(value)),
                get_calls: AtomicU32::new(0),
            }
        }

        fn get_calls(&self) -> u32 {
            self.get_calls.load(Ordering::Relaxed)
        }
    }

    impl FeltSignificanceStore for CountingFeltSignificanceStore {
        fn get(&self, _scope_id: &str) -> Result<Option<FeltSignificance>> {
            self.get_calls.fetch_add(1, Ordering::Relaxed);
            Ok(self.value.lock().unwrap_or_else(|e| e.into_inner()).clone())
        }

        fn set(&self, _scope_id: &str, significance: &FeltSignificance) -> Result<()> {
            *self.value.lock().unwrap_or_else(|e| e.into_inner()) = Some(significance.clone());
            Ok(())
        }

        fn clear(&self, _scope_id: &str) -> Result<()> {
            *self.value.lock().unwrap_or_else(|e| e.into_inner()) = None;
            Ok(())
        }
    }

    #[derive(Default)]
    struct CountingTemperamentContinuityStore {
        value: Mutex<Option<TemperamentContinuity>>,
        get_calls: AtomicU32,
    }

    impl CountingTemperamentContinuityStore {
        fn with_value(value: TemperamentContinuity) -> Self {
            Self {
                value: Mutex::new(Some(value)),
                get_calls: AtomicU32::new(0),
            }
        }

        fn get_calls(&self) -> u32 {
            self.get_calls.load(Ordering::Relaxed)
        }
    }

    impl TemperamentContinuityStore for CountingTemperamentContinuityStore {
        fn get(&self, _scope_id: &str) -> Result<Option<TemperamentContinuity>> {
            self.get_calls.fetch_add(1, Ordering::Relaxed);
            Ok(self.value.lock().unwrap_or_else(|e| e.into_inner()).clone())
        }

        fn set(&self, _scope_id: &str, continuity: &TemperamentContinuity) -> Result<()> {
            *self.value.lock().unwrap_or_else(|e| e.into_inner()) = Some(continuity.clone());
            Ok(())
        }

        fn clear(&self, _scope_id: &str) -> Result<()> {
            *self.value.lock().unwrap_or_else(|e| e.into_inner()) = None;
            Ok(())
        }
    }

    #[derive(Default)]
    struct CountingInnerConflictStore {
        value: Mutex<Option<InnerConflict>>,
        get_calls: AtomicU32,
    }

    impl CountingInnerConflictStore {
        fn with_value(value: InnerConflict) -> Self {
            Self {
                value: Mutex::new(Some(value)),
                get_calls: AtomicU32::new(0),
            }
        }

        fn get_calls(&self) -> u32 {
            self.get_calls.load(Ordering::Relaxed)
        }
    }

    impl InnerConflictStore for CountingInnerConflictStore {
        fn get(&self, _scope_id: &str) -> Result<Option<InnerConflict>> {
            self.get_calls.fetch_add(1, Ordering::Relaxed);
            Ok(self.value.lock().unwrap_or_else(|e| e.into_inner()).clone())
        }

        fn set(&self, _scope_id: &str, conflict: &InnerConflict) -> Result<()> {
            *self.value.lock().unwrap_or_else(|e| e.into_inner()) = Some(conflict.clone());
            Ok(())
        }

        fn clear(&self, _scope_id: &str) -> Result<()> {
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
    struct StubActiveWorkStore {
        record: Mutex<Option<ActiveWorkRecord>>,
    }

    impl ActiveWorkStore for StubActiveWorkStore {
        fn get(&self, _chat_id: &str) -> Result<Option<ActiveWorkRecord>> {
            Ok(self
                .record
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone())
        }

        fn set(&self, _chat_id: &str, record: &ActiveWorkRecord) -> Result<()> {
            *self.record.lock().unwrap_or_else(|e| e.into_inner()) = Some(record.clone());
            Ok(())
        }

        fn clear(&self, _chat_id: &str) -> Result<()> {
            *self.record.lock().unwrap_or_else(|e| e.into_inner()) = None;
            Ok(())
        }
    }

    fn stub_active_work_store_from_state(
        state: &ExecutionState,
        user_request: &str,
    ) -> StubActiveWorkStore {
        StubActiveWorkStore {
            record: Mutex::new(ActiveWorkRecord::from_interactive_execution_state(
                state,
                user_request,
            )),
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
    struct ErrorSelfAuthoredCoreStore;

    impl SelfAuthoredCoreStore for ErrorSelfAuthoredCoreStore {
        fn get(&self, _scope_id: &str) -> Result<Option<SelfAuthoredCore>> {
            Err(Error::config(
                "prompt_self_authored_core",
                "self authored core unavailable",
            ))
        }

        fn set(&self, _scope_id: &str, _core: &SelfAuthoredCore) -> Result<()> {
            Ok(())
        }

        fn clear(&self, _scope_id: &str) -> Result<()> {
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
    struct CountingRelationshipTopologyStore {
        value: Mutex<Option<RelationshipTopology>>,
        get_calls: AtomicU32,
    }

    impl CountingRelationshipTopologyStore {
        fn get_calls(&self) -> u32 {
            self.get_calls.load(Ordering::Relaxed)
        }
    }

    impl RelationshipTopologyStore for CountingRelationshipTopologyStore {
        fn get(&self, _scope_id: &str) -> Result<Option<RelationshipTopology>> {
            self.get_calls.fetch_add(1, Ordering::Relaxed);
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
    struct ErrorPrivateGardenStore;

    impl PrivateGardenStore for ErrorPrivateGardenStore {
        fn list(&self, _chat_id: &str, _limit: usize) -> Result<Vec<PrivateGardenDocRecord>> {
            Err(Error::config(
                "prompt_private_garden",
                "private garden unavailable",
            ))
        }

        fn read(&self, _chat_id: &str, _doc_path: &str) -> Result<Option<PrivateGardenDoc>> {
            Err(Error::config(
                "prompt_private_garden",
                "private garden unavailable",
            ))
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
    struct ScopedPrivateGardenStore {
        docs_by_scope: Mutex<BTreeMap<String, Vec<PrivateGardenDoc>>>,
        scope_calls: Mutex<Vec<String>>,
    }

    impl PrivateGardenStore for ScopedPrivateGardenStore {
        fn list(&self, chat_id: &str, limit: usize) -> Result<Vec<PrivateGardenDocRecord>> {
            self.scope_calls
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(chat_id.to_string());
            Ok(self
                .docs_by_scope
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(chat_id)
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .rev()
                .take(limit)
                .map(|doc| PrivateGardenDocRecord {
                    path: doc.path,
                    updated_at: doc.updated_at,
                    revision: doc.revision,
                    bytes: doc.content.len(),
                    preview: crate::memory::private_garden::build_private_garden_preview(
                        &doc.content,
                    ),
                })
                .collect())
        }

        fn read(&self, chat_id: &str, doc_path: &str) -> Result<Option<PrivateGardenDoc>> {
            self.scope_calls
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(chat_id.to_string());
            Ok(self
                .docs_by_scope
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(chat_id)
                .and_then(|docs| docs.iter().find(|doc| doc.path == doc_path).cloned()))
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

    struct ErrorRemindAtStore;

    impl crate::memory::RemindAtStore for ErrorRemindAtStore {
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
            Err(crate::error::Error::config(
                "world_snapshot_reminders",
                "store unreadable",
            ))
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

        fn list_due_unnotified(&self, _now_unix_secs: u64, _limit: usize) -> Result<Vec<TaskItem>> {
            Ok(Vec::new())
        }

        fn mark_due_notified(&self, _task: &TaskItem, _notified_at_unix_secs: u64) -> Result<bool> {
            Ok(false)
        }
    }

    struct ErrorTaskStore;

    impl TaskStore for ErrorTaskStore {
        fn list(&self, _channel: &str, _chat_id: &str, _query: TaskQuery) -> Result<Vec<TaskItem>> {
            Err(crate::error::Error::config(
                "world_snapshot_tasks",
                "store unreadable",
            ))
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

        fn list_due_unnotified(&self, _now_unix_secs: u64, _limit: usize) -> Result<Vec<TaskItem>> {
            Ok(Vec::new())
        }

        fn mark_due_notified(&self, _task: &TaskItem, _notified_at_unix_secs: u64) -> Result<bool> {
            Ok(false)
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
                kind: crate::task_execution::TaskRunKind::TaskExecution,
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
                ..ExecutionState::default()
            })),
        };
        let active_work_store = {
            let state = execution_state_store
                .state
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone()
                .expect("execution state");
            stub_active_work_store_from_state(&state, "嗯?")
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
        let mut context = load_prompt_memory_context(PromptMemoryContextParams {
            chat_id: "chat-1",
            current_channel: "qq_channel",
            user_query: "嗯?",
            memory_system_kind: crate::memory::MemorySystemKind::LinuxFull,
            system_max_len: 1024,
            now_secs: 100,
            participation_plan: PromptParticipationPlan::full(),
            recent_messages_limit: 8,
            load_long_term_memory: true,
            include_private_garden_projection: true,
            session_store: &session_store,
            memory_store: &archive_memory_store,
            session_summary_store: &summary_store,
            long_term_memory_store: &memory_store,
            execution_state_store: &execution_state_store,
            active_work_store: &active_work_store,
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
            felt_significance_store: &StubFeltSignificanceStore::default(),
            temperament_continuity_store: &StubTemperamentContinuityStore::default(),
            inner_conflict_store: &StubInnerConflictStore::default(),
            private_doc_store: &private_doc_store,
            private_garden_store: &private_garden_store,
            mental_privacy_store: &mental_privacy_store,
            remind_store: &remind_store,
            task_store: &task_store,
            turn_continuity_evidence_store: &StubTurnContinuityEvidenceStore,
            turn_ledger_store: &turn_ledger_store,
            skill_storage: &skill_storage,
            continuity_capsule_store: &continuity_capsule_store,
        });

        let groups =
            context.normalize_projection_groups_for_prompt(MemorySystemKind::LinuxFull, 8192);

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
        assert!(groups
            .constitutional_stack_text
            .as_deref()
            .unwrap_or_default()
            .contains("## Self-Authored Core"));
        assert!(!groups
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
        assert!(groups
            .background_governance_text
            .as_deref()
            .unwrap_or_default()
            .contains("## Relationship Portfolio"));
        assert!(!groups
            .background_governance_text
            .as_deref()
            .unwrap_or_default()
            .contains("## Private Garden"));
        assert!(!groups
            .background_governance_text
            .as_deref()
            .unwrap_or_default()
            .contains("## Inner Workspace"));
        assert!(context.mental_privacy_adjudication_text.is_none());
        assert!(context
            .runtime_skill_text
            .as_deref()
            .unwrap_or_default()
            .contains("Runtime skills"));
        assert!(groups
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

        let mut context = load_prompt_memory_context(PromptMemoryContextParams {
            chat_id: "chat-1",
            current_channel: "qq_channel",
            user_query: "嗯?",
            memory_system_kind: crate::memory::MemorySystemKind::EspCompact,
            system_max_len: 80,
            now_secs: 100,
            participation_plan: PromptParticipationPlan::full(),
            recent_messages_limit: 8,
            load_long_term_memory: true,
            include_private_garden_projection: true,
            session_store: &session_store,
            memory_store: &archive_memory_store,
            session_summary_store: &summary_store,
            long_term_memory_store: &memory_store,
            execution_state_store: &execution_state_store,
            active_work_store: &StubActiveWorkStore::default(),
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
            felt_significance_store: &StubFeltSignificanceStore::default(),
            temperament_continuity_store: &StubTemperamentContinuityStore::default(),
            inner_conflict_store: &StubInnerConflictStore::default(),
            private_doc_store: &private_doc_store,
            private_garden_store: &private_garden_store,
            mental_privacy_store: &mental_privacy_store,
            remind_store: &remind_store,
            task_store: &task_store,
            turn_continuity_evidence_store: &StubTurnContinuityEvidenceStore,
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
        let groups =
            context.normalize_projection_groups_for_prompt(MemorySystemKind::EspCompact, 80);
        assert!(context.long_term_memory_text.is_none());
        assert!(groups.governed_memory_evidence_text.is_none());
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

        let mut context = load_prompt_memory_context(PromptMemoryContextParams {
            chat_id: "chat-1",
            current_channel: "qq_channel",
            user_query: "继续",
            memory_system_kind: crate::memory::MemorySystemKind::LinuxFull,
            system_max_len: 1024,
            now_secs: 100,
            participation_plan: PromptParticipationPlan::full(),
            recent_messages_limit: 16,
            load_long_term_memory: false,
            include_private_garden_projection: false,
            session_store: &session_store,
            memory_store: &archive_memory_store,
            session_summary_store: &summary_store,
            long_term_memory_store: &memory_store,
            execution_state_store: &execution_state_store,
            active_work_store: &StubActiveWorkStore::default(),
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
            felt_significance_store: &StubFeltSignificanceStore::default(),
            temperament_continuity_store: &StubTemperamentContinuityStore::default(),
            inner_conflict_store: &StubInnerConflictStore::default(),
            private_doc_store: &private_doc_store,
            private_garden_store: &private_garden_store,
            mental_privacy_store: &mental_privacy_store,
            remind_store: &remind_store,
            task_store: &task_store,
            turn_continuity_evidence_store: &StubTurnContinuityEvidenceStore,
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
        let groups =
            context.normalize_projection_groups_for_prompt(MemorySystemKind::LinuxFull, 8192);
        assert!(!groups
            .background_governance_text
            .as_deref()
            .unwrap_or_default()
            .contains("内在工作区"));
        assert!(context.private_garden_text.is_none());
        assert_eq!(context.recent_messages.len(), 2);
        assert!(memory_store
            .last_query
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_none());
    }

    #[test]
    fn prompt_memory_uses_board_owned_private_garden_scope_and_renders_contract_summary() {
        let session_store = StubSessionStore::default();
        let archive_memory_store = StubMemoryStore::default();
        let summary_store = StubSessionSummaryStore::default();
        let memory_store = StubLongTermMemoryStore::default();
        let turn_ledger_store = StubTurnLedgerStore::default();
        let execution_state_store = StubExecutionStateStore::default();
        let self_model_store = StubSelfModelStore {
            model: Mutex::new(Some(SelfModel {
                continuity_anchor: "persistent board self".to_string(),
                self_narrative: "我是独立的板级主体".to_string(),
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
        let private_garden_store = ScopedPrivateGardenStore {
            docs_by_scope: Mutex::new(BTreeMap::from([(
                crate::memory::BOARD_SUBJECT_SCOPE_ID.to_string(),
                vec![PrivateGardenDoc {
                    path: "journal/afterglow.md".to_string(),
                    content: "这块花园属于板级主体，不属于当前用户".to_string(),
                    updated_at: 2,
                    revision: 1,
                }],
            )])),
            scope_calls: Mutex::new(Vec::new()),
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
            user_query: "说说你的私有花园",
            memory_system_kind: crate::memory::MemorySystemKind::LinuxFull,
            system_max_len: 1024,
            now_secs: 100,
            participation_plan: PromptParticipationPlan::full(),
            recent_messages_limit: 8,
            load_long_term_memory: false,
            include_private_garden_projection: true,
            session_store: &session_store,
            memory_store: &archive_memory_store,
            session_summary_store: &summary_store,
            long_term_memory_store: &memory_store,
            execution_state_store: &execution_state_store,
            active_work_store: &StubActiveWorkStore::default(),
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
            felt_significance_store: &StubFeltSignificanceStore::default(),
            temperament_continuity_store: &StubTemperamentContinuityStore::default(),
            inner_conflict_store: &StubInnerConflictStore::default(),
            private_doc_store: &private_doc_store,
            private_garden_store: &private_garden_store,
            mental_privacy_store: &mental_privacy_store,
            remind_store: &remind_store,
            task_store: &task_store,
            turn_continuity_evidence_store: &StubTurnContinuityEvidenceStore,
            turn_ledger_store: &turn_ledger_store,
            skill_storage: &skill_storage,
            continuity_capsule_store: &continuity_capsule_store,
        });

        assert!(
            context
                .private_garden_text
                .as_deref()
                .unwrap_or_default()
                .contains("journal/afterglow.md"),
            "prompt private_garden projection must load from board-owned scope"
        );
        assert!(
            context
                .self_state_text
                .as_deref()
                .unwrap_or_default()
                .contains("Private garden owner: board.self"),
            "prompt self-state should carry deterministic ownership contract"
        );
        let scope_calls = private_garden_store
            .scope_calls
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        assert!(
            scope_calls
                .iter()
                .all(|scope| scope == crate::memory::BOARD_SUBJECT_SCOPE_ID),
            "prompt memory should query private garden by board scope, got {scope_calls:?}"
        );
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
            memory_system_kind: crate::memory::MemorySystemKind::LinuxFull,
            system_max_len: 1024,
            now_secs: 100,
            participation_plan: PromptParticipationPlan::full(),
            recent_messages_limit: 8,
            load_long_term_memory: false,
            include_private_garden_projection: false,
            session_store: &session_store,
            memory_store: &archive_memory_store,
            session_summary_store: &summary_store,
            long_term_memory_store: &memory_store,
            execution_state_store: &execution_state_store,
            active_work_store: &StubActiveWorkStore::default(),
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
            felt_significance_store: &StubFeltSignificanceStore::default(),
            temperament_continuity_store: &StubTemperamentContinuityStore::default(),
            inner_conflict_store: &StubInnerConflictStore::default(),
            private_doc_store: &private_doc_store,
            private_garden_store: &private_garden_store,
            mental_privacy_store: &mental_privacy_store,
            remind_store: &StubRemindAtStore,
            task_store: &StubTaskStore,
            turn_continuity_evidence_store: &StubTurnContinuityEvidenceStore,
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

        let mut context = load_prompt_memory_context(PromptMemoryContextParams {
            chat_id: "chat-1",
            current_channel: "qq_channel",
            user_query: "继续",
            memory_system_kind: crate::memory::MemorySystemKind::LinuxFull,
            system_max_len: 1024,
            now_secs: 100,
            participation_plan: PromptParticipationPlan::full(),
            recent_messages_limit: 8,
            load_long_term_memory: true,
            include_private_garden_projection: false,
            session_store: &session_store,
            memory_store: &archive_memory_store,
            session_summary_store: &summary_store,
            long_term_memory_store: &memory_store,
            execution_state_store: &execution_state_store,
            active_work_store: &StubActiveWorkStore::default(),
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
            felt_significance_store: &StubFeltSignificanceStore::default(),
            temperament_continuity_store: &StubTemperamentContinuityStore::default(),
            inner_conflict_store: &StubInnerConflictStore::default(),
            private_doc_store: &private_doc_store,
            private_garden_store: &private_garden_store,
            mental_privacy_store: &mental_privacy_store,
            remind_store: &StubRemindAtStore,
            task_store: &StubTaskStore,
            turn_continuity_evidence_store: &StubTurnContinuityEvidenceStore,
            turn_ledger_store: &turn_ledger_store,
            skill_storage: &skill_storage,
            continuity_capsule_store: &continuity_capsule_store,
        });

        assert!(context.self_authored_core.is_none());
        assert!(context.self_authored_core_text.is_none());
        let groups =
            context.normalize_projection_groups_for_prompt(MemorySystemKind::LinuxFull, 4096);
        assert!(!groups
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
                ..ExecutionState::default()
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

        let mut context = load_prompt_memory_context(PromptMemoryContextParams {
            chat_id: "chat-1",
            current_channel: "qq_channel",
            user_query: "继续",
            memory_system_kind: crate::memory::MemorySystemKind::LinuxFull,
            system_max_len: 1024,
            now_secs: 100,
            participation_plan: PromptParticipationPlan::full(),
            recent_messages_limit: 8,
            load_long_term_memory: true,
            include_private_garden_projection: false,
            session_store: &session_store,
            memory_store: &archive_memory_store,
            session_summary_store: &summary_store,
            long_term_memory_store: &memory_store,
            execution_state_store: &execution_state_store,
            active_work_store: &StubActiveWorkStore::default(),
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
            felt_significance_store: &StubFeltSignificanceStore::default(),
            temperament_continuity_store: &StubTemperamentContinuityStore::default(),
            inner_conflict_store: &StubInnerConflictStore::default(),
            private_doc_store: &StubPrivateDocStore::default(),
            private_garden_store: &StubPrivateGardenStore::default(),
            mental_privacy_store: &StubMentalPrivacyStore::default(),
            remind_store: &StubRemindAtStore,
            task_store: &StubTaskStore,
            turn_continuity_evidence_store: &StubTurnContinuityEvidenceStore,
            turn_ledger_store: &StubTurnLedgerStore::default(),
            skill_storage: &StubSkillStorage::default(),
            continuity_capsule_store: &continuity_capsule_store,
        });

        let groups =
            context.normalize_projection_groups_for_prompt(MemorySystemKind::LinuxFull, 8192);
        let active = groups.active_task_context_text.unwrap_or_default();
        let continuity_pos = active.find("## Work Continuity").unwrap();
        let capsule_pos = active.find("## Continuity Capsules").unwrap();
        let workspace_pos = active.find("## Task Workspace").unwrap();
        assert!(continuity_pos < capsule_pos);
        assert!(capsule_pos < workspace_pos);
        assert!(!active.contains("## Execution State"));
        assert!(!groups
            .governed_memory_evidence_text
            .as_deref()
            .unwrap_or_default()
            .contains("## Continuity Capsules"));
    }

    #[test]
    fn active_task_context_includes_recent_turn_observation_after_work_continuity() {
        let session_store = StubSessionStore {
            recent: Mutex::new(vec![SessionMessage {
                role: "user".to_string(),
                content: "继续".to_string(),
            }]),
        };
        let summary_store = StubSessionSummaryStore {
            summary: Mutex::new(Some(("continue the replay substrate work".to_string(), 2))),
        };
        let memory_store = StubLongTermMemoryStore::default();
        let archive_memory_store = StubMemoryStore::default();
        let execution_state_store = StubExecutionStateStore {
            state: Mutex::new(Some(ExecutionState {
                status: ExecutionStatus::Active,
                goal: "Close replay substrate loop".to_string(),
                progress: "turn observation 已写入 ledger".to_string(),
                blocker: String::new(),
                next_action: "接入 active task prompt".to_string(),
                last_output: String::new(),
                updated_at: 5,
                ..ExecutionState::default()
            })),
        };
        let active_work_store = StubActiveWorkStore {
            record: Mutex::new(Some(ActiveWorkRecord {
                kind: crate::agent::ActiveWorkKind::InteractiveAction,
                title: "Close replay substrate loop".to_string(),
                status: crate::agent::ForegroundWorkStatus::Running,
                continuity_open: true,
                blocks_background_llm: true,
                progress_summary: "continue the replay substrate work".to_string(),
                blocker: String::new(),
                next_action: "Continue the current task chain.".to_string(),
                recent_outcome: String::new(),
                active_artifact_refs: Vec::new(),
                updated_at: 5,
            })),
        };
        let task_run_store = StubActiveTaskRunStore {
            active: Mutex::new(vec![make_active_task_run(
                "run-observation",
                "Close replay substrate loop",
                "Route latest turn observation into active task context",
            )]),
        };
        let continuity_capsule_store = StubContinuityCapsuleStore::default();
        continuity_capsule_store
            .upsert_many(
                &[crate::memory::ContinuityCapsuleDraft {
                    scope_kind: crate::memory::ContinuityCapsuleScopeKind::Chat,
                    scope_id: "chat-1".to_string(),
                    source_chat_id: "chat-1".to_string(),
                    run_id: "run-observation".to_string(),
                    topic: "replay substrate".to_string(),
                    summary: "Keep the latest observation in the next turn working set."
                        .to_string(),
                    next_step: "Place observation grounding before capsule and task workspace."
                        .to_string(),
                    ..Default::default()
                }],
                100,
            )
            .unwrap();
        let turn_ledger_store = StubTurnLedgerStore::default();
        turn_ledger_store
            .set(
                &crate::memory::relationship_scope_id("qq_channel", "chat-1"),
                &TurnLedger {
                    req_id: "run-observation".to_string(),
                    status: TurnLedgerStatus::Answered,
                    observation: Some(TurnObservationLedger {
                        execution_class: TurnExecutionClass::ToolAssisted,
                        deliberation_class: TurnDeliberationClass::HardReasoning,
                        final_outcome: "surface_finalization".to_string(),
                        pressure: TurnPersonaPressureLevel::Cautious,
                        mode: TurnModeSnapshotLedger {
                            current_mode: "normal".to_string(),
                            allow_non_voice_outbound: true,
                            allow_idle_self_runtime: true,
                        },
                        tool_path: TurnToolPathLedger {
                            path: "surface_finalization".to_string(),
                            tool_calls: 2,
                            react_rounds: 2,
                            current_primary_delivered: false,
                        },
                        blocker: Some(TurnBlockerLedger {
                            kind: "retryable".to_string(),
                            failed_calls: 1,
                            total_calls: 1,
                        }),
                    }),
                    ..TurnLedger::default()
                },
            )
            .unwrap();

        let mut context = load_prompt_memory_context(PromptMemoryContextParams {
            chat_id: "chat-1",
            current_channel: "qq_channel",
            user_query: "继续",
            memory_system_kind: crate::memory::MemorySystemKind::LinuxFull,
            system_max_len: 1024,
            now_secs: 100,
            participation_plan: PromptParticipationPlan::full(),
            recent_messages_limit: 8,
            load_long_term_memory: true,
            include_private_garden_projection: false,
            session_store: &session_store,
            memory_store: &archive_memory_store,
            session_summary_store: &summary_store,
            long_term_memory_store: &memory_store,
            execution_state_store: &execution_state_store,
            active_work_store: &active_work_store,
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
            felt_significance_store: &StubFeltSignificanceStore::default(),
            temperament_continuity_store: &StubTemperamentContinuityStore::default(),
            inner_conflict_store: &StubInnerConflictStore::default(),
            private_doc_store: &StubPrivateDocStore::default(),
            private_garden_store: &StubPrivateGardenStore::default(),
            mental_privacy_store: &StubMentalPrivacyStore::default(),
            remind_store: &StubRemindAtStore,
            task_store: &StubTaskStore,
            turn_continuity_evidence_store: &StubTurnContinuityEvidenceStore,
            turn_ledger_store: &turn_ledger_store,
            skill_storage: &StubSkillStorage::default(),
            continuity_capsule_store: &continuity_capsule_store,
        });

        let groups =
            context.normalize_projection_groups_for_prompt(MemorySystemKind::LinuxFull, 8192);
        let active = groups.active_task_context_text.unwrap_or_default();
        let continuity_pos = active.find("## Work Continuity").unwrap();
        let observation_pos = active.find("## Latest Turn Observation").unwrap();
        let capsule_pos = active.find("## Continuity Capsules").unwrap();
        let workspace_pos = active.find("## Task Workspace").unwrap();

        assert!(continuity_pos < observation_pos);
        assert!(observation_pos < capsule_pos);
        assert!(capsule_pos < workspace_pos);
        assert!(!active.contains("## Execution State"));
        assert!(active.contains("Focus: Close replay substrate loop"));
        assert!(active.contains("Progress: continue the replay substrate work"));
        assert!(active.contains("Next: Continue the current task chain."));
        assert!(!active.contains("Progress: turn observation 已写入 ledger"));
        assert!(active.contains("Final outcome: surface_finalization"));
        assert!(active.contains("Tool path: surface_finalization"));
        assert!(!groups
            .governed_memory_evidence_text
            .as_deref()
            .unwrap_or_default()
            .contains("## Latest Turn Observation"));
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

        let mut context = load_prompt_memory_context(PromptMemoryContextParams {
            chat_id: "chat-1",
            current_channel: "qq_channel",
            user_query: "按之前的 release patch 流程继续",
            memory_system_kind: crate::memory::MemorySystemKind::LinuxFull,
            system_max_len: 1024,
            now_secs: 100,
            participation_plan: PromptParticipationPlan::full(),
            recent_messages_limit: 8,
            load_long_term_memory: true,
            include_private_garden_projection: false,
            session_store: &session_store,
            memory_store: &archive_memory_store,
            session_summary_store: &summary_store,
            long_term_memory_store: &memory_store,
            execution_state_store: &StubExecutionStateStore::default(),
            active_work_store: &StubActiveWorkStore::default(),
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
            felt_significance_store: &StubFeltSignificanceStore::default(),
            temperament_continuity_store: &StubTemperamentContinuityStore::default(),
            inner_conflict_store: &StubInnerConflictStore::default(),
            private_doc_store: &StubPrivateDocStore::default(),
            private_garden_store: &StubPrivateGardenStore::default(),
            mental_privacy_store: &StubMentalPrivacyStore::default(),
            remind_store: &StubRemindAtStore,
            task_store: &StubTaskStore,
            turn_continuity_evidence_store: &StubTurnContinuityEvidenceStore,
            turn_ledger_store: &StubTurnLedgerStore::default(),
            skill_storage: &skill_storage,
            continuity_capsule_store: &continuity_capsule_store,
        });

        let groups =
            context.normalize_projection_groups_for_prompt(MemorySystemKind::LinuxFull, 8192);
        let governed = groups.governed_memory_evidence_text.unwrap_or_default();
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

        let mut context = load_prompt_memory_context(PromptMemoryContextParams {
            chat_id: "chat-1",
            current_channel: "qq_channel",
            user_query: "把那次 network outage 的原始记录翻出来",
            memory_system_kind: crate::memory::MemorySystemKind::LinuxFull,
            system_max_len: 1024,
            now_secs: 100,
            participation_plan: PromptParticipationPlan::full(),
            recent_messages_limit: 8,
            load_long_term_memory: true,
            include_private_garden_projection: false,
            session_store: &session_store,
            memory_store: &archive_memory_store,
            session_summary_store: &summary_store,
            long_term_memory_store: &memory_store,
            execution_state_store: &StubExecutionStateStore::default(),
            active_work_store: &StubActiveWorkStore::default(),
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
            felt_significance_store: &StubFeltSignificanceStore::default(),
            temperament_continuity_store: &StubTemperamentContinuityStore::default(),
            inner_conflict_store: &StubInnerConflictStore::default(),
            private_doc_store: &StubPrivateDocStore::default(),
            private_garden_store: &StubPrivateGardenStore::default(),
            mental_privacy_store: &StubMentalPrivacyStore::default(),
            remind_store: &StubRemindAtStore,
            task_store: &StubTaskStore,
            turn_continuity_evidence_store: &StubTurnContinuityEvidenceStore,
            turn_ledger_store: &StubTurnLedgerStore::default(),
            skill_storage: &StubSkillStorage::default(),
            continuity_capsule_store: &continuity_capsule_store,
        });

        let groups =
            context.normalize_projection_groups_for_prompt(MemorySystemKind::LinuxFull, 8192);
        let governed = groups.governed_memory_evidence_text.unwrap_or_default();
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
    fn continuity_query_prefers_capsule_before_archive_fallback() {
        let mut context = continuity_router_context_for_regression();
        let groups =
            context.normalize_projection_groups_for_prompt(MemorySystemKind::LinuxFull, 2048);
        let active = groups.active_task_context_text.unwrap_or_default();
        let governed = groups.governed_memory_evidence_text.unwrap_or_default();

        assert_eq!(context.recall_router.intent, PromptRecallIntent::Continuity);
        let continuity_pos = active.find("## Work Continuity").unwrap();
        let capsule_pos = active.find("## Continuity Capsules").unwrap();
        let workspace_pos = active.find("## Task Workspace").unwrap();
        let archive_pos = governed.find("Archive evidence").unwrap();
        let canonical_pos = governed.find("## Long-term memory").unwrap();
        assert!(continuity_pos < capsule_pos);
        assert!(capsule_pos < workspace_pos);
        assert!(!governed.contains("## Continuity Capsules"));
        assert!(archive_pos < canonical_pos);
    }

    #[test]
    fn prompt_projection_regression_suite_covers_router_contract() {
        let observations = vec![
            observe_prompt_projection_case(
                "continuity",
                continuity_router_context_for_regression(),
                PromptRecallIntent::Continuity,
                &[
                    "## Work Continuity",
                    "## Continuity Capsules",
                    "## Task Workspace",
                ],
                &["Archive evidence", "## Long-term memory"],
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
        mut context: PromptMemoryContext,
        expected_intent: PromptRecallIntent,
        active_order: &[&str],
        governed_order: &[&str],
    ) -> PromptProjectionRegressionObservation {
        let groups =
            context.normalize_projection_groups_for_prompt(MemorySystemKind::LinuxFull, 2048);
        let active = groups.active_task_context_text.unwrap_or_default();
        let governed = groups.governed_memory_evidence_text.unwrap_or_default();
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
                ..ExecutionState::default()
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
            memory_system_kind: crate::memory::MemorySystemKind::LinuxFull,
            system_max_len: 1024,
            now_secs: 100,
            participation_plan: PromptParticipationPlan::full(),
            recent_messages_limit: 8,
            load_long_term_memory: true,
            include_private_garden_projection: false,
            session_store: &session_store,
            memory_store: &archive_memory_store,
            session_summary_store: &summary_store,
            long_term_memory_store: &memory_store,
            execution_state_store: &execution_state_store,
            active_work_store: &StubActiveWorkStore::default(),
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
            felt_significance_store: &StubFeltSignificanceStore::default(),
            temperament_continuity_store: &StubTemperamentContinuityStore::default(),
            inner_conflict_store: &StubInnerConflictStore::default(),
            private_doc_store: &StubPrivateDocStore::default(),
            private_garden_store: &StubPrivateGardenStore::default(),
            mental_privacy_store: &StubMentalPrivacyStore::default(),
            remind_store: &StubRemindAtStore,
            task_store: &StubTaskStore,
            turn_continuity_evidence_store: &StubTurnContinuityEvidenceStore,
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
            memory_system_kind: crate::memory::MemorySystemKind::LinuxFull,
            system_max_len: 1024,
            now_secs: 100,
            participation_plan: PromptParticipationPlan::full(),
            recent_messages_limit: 8,
            load_long_term_memory: true,
            include_private_garden_projection: false,
            session_store: &session_store,
            memory_store: &archive_memory_store,
            session_summary_store: &summary_store,
            long_term_memory_store: &memory_store,
            execution_state_store: &StubExecutionStateStore::default(),
            active_work_store: &StubActiveWorkStore::default(),
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
            felt_significance_store: &StubFeltSignificanceStore::default(),
            temperament_continuity_store: &StubTemperamentContinuityStore::default(),
            inner_conflict_store: &StubInnerConflictStore::default(),
            private_doc_store: &StubPrivateDocStore::default(),
            private_garden_store: &StubPrivateGardenStore::default(),
            mental_privacy_store: &StubMentalPrivacyStore::default(),
            remind_store: &StubRemindAtStore,
            task_store: &StubTaskStore,
            turn_continuity_evidence_store: &StubTurnContinuityEvidenceStore,
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
            memory_system_kind: crate::memory::MemorySystemKind::LinuxFull,
            system_max_len: 1024,
            now_secs: 100,
            participation_plan: PromptParticipationPlan::full(),
            recent_messages_limit: 8,
            load_long_term_memory: true,
            include_private_garden_projection: false,
            session_store: &session_store,
            memory_store: &archive_memory_store,
            session_summary_store: &summary_store,
            long_term_memory_store: &memory_store,
            execution_state_store: &StubExecutionStateStore::default(),
            active_work_store: &StubActiveWorkStore::default(),
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
            felt_significance_store: &StubFeltSignificanceStore::default(),
            temperament_continuity_store: &StubTemperamentContinuityStore::default(),
            inner_conflict_store: &StubInnerConflictStore::default(),
            private_doc_store: &StubPrivateDocStore::default(),
            private_garden_store: &StubPrivateGardenStore::default(),
            mental_privacy_store: &StubMentalPrivacyStore::default(),
            remind_store: &StubRemindAtStore,
            task_store: &StubTaskStore,
            turn_continuity_evidence_store: &StubTurnContinuityEvidenceStore,
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
            memory_system_kind: crate::memory::MemorySystemKind::LinuxFull,
            system_max_len: 1024,
            now_secs: 100,
            participation_plan: PromptParticipationPlan::full(),
            recent_messages_limit: 8,
            load_long_term_memory: true,
            include_private_garden_projection: false,
            session_store: &session_store,
            memory_store: &archive_memory_store,
            session_summary_store: &summary_store,
            long_term_memory_store: &memory_store,
            execution_state_store: &StubExecutionStateStore::default(),
            active_work_store: &StubActiveWorkStore::default(),
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
            felt_significance_store: &StubFeltSignificanceStore::default(),
            temperament_continuity_store: &StubTemperamentContinuityStore::default(),
            inner_conflict_store: &StubInnerConflictStore::default(),
            private_doc_store: &StubPrivateDocStore::default(),
            private_garden_store: &StubPrivateGardenStore::default(),
            mental_privacy_store: &StubMentalPrivacyStore::default(),
            remind_store: &StubRemindAtStore,
            task_store: &StubTaskStore,
            turn_continuity_evidence_store: &StubTurnContinuityEvidenceStore,
            turn_ledger_store: &StubTurnLedgerStore::default(),
            skill_storage: &StubSkillStorage::default(),
            continuity_capsule_store: &continuity_capsule_store,
        })
    }
}
