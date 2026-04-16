use crate::bus::IngressKind;
use crate::error::Result;
use crate::llm::{LlmClient, LlmHttpClient, Message, ToolChoicePolicy};
use crate::orchestrator::PressureLevel;
use crate::util::{scrub_credentials, truncate_content_to_max};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::fmt::Write as _;

use super::{
    append_core_revision_record, compute_core_revision_governance_digest,
    core_revision_observation_due_at, correction_pressure, has_recent_matching_adopted_change,
    has_recent_matching_rejected_change,
    llm_json::{get_object_string_list, get_object_text, parse_llm_json_payload, LlmJsonPayload},
    recent_adopted_revision, render_autonomy_strategy_block, render_core_revision_governance_block,
    render_mental_privacy_boundary_block, render_recent_persona_evidence_block,
    render_relationship_portfolio_block, render_relationship_topology_block,
    render_self_continuity_block, render_self_model_block, render_world_sense_block,
    AutonomyStrategy, CoreRevisionActionKind, CoreRevisionConflictClass,
    CoreRevisionCorrectionKind, CoreRevisionGovernanceDigest, CoreRevisionLedger,
    CoreRevisionLedgerStore, CoreRevisionOutcome, CoreRevisionRecord, CoreRevisionRecordChange,
    MentalPrivacyState, RecentPersonaEvidence, RelationshipPortfolio, RelationshipTopology,
    SelfAuthoredCoreStore, SelfContinuity, SelfModel, WorldSense,
};

pub const SELF_AUTHORED_CORE_SYSTEM_PROMPT: &str = "You maintain the assistant's persistent self-authored core for the whole board-level subject, not one chat. Return JSON only with fields board_scope_decision, rationale, evidence_summary, counterevidence, proposed_actions. board_scope_decision must be revise_board, relation_local, or no_change. proposed_actions must be an array of compact action objects. Allowed action kinds are revise_identity_anchor, add_non_negotiables, remove_non_negotiables, revise_priority_constitution, revise_default_response_mode, revise_default_task_scope, revise_default_initiative_posture, revise_default_relationship_posture, revise_boundary_doctrine, revise_truth_doctrine, revise_self_preservation_doctrine, revise_repair_doctrine, revise_change_protocol. This is a constitutional revision pass, not a free rewrite. Propose only stable board-level changes that deserve cross-chat carry-forward. Use self_model, self_continuity, boundary state, relationship portfolio, relationship topology, and recent multi-turn persona evidence as grounding. Treat recent persona evidence as evidence, never automatic promotion authority. Operational traces such as task scope, response mode, pressure, tool usage, or reply scope are not sufficient constitutional revision grounds by themselves. A quarantined, cooled-down, or otherwise isolated relation must not directly rewrite the board-level core. If the latest material should stay relation-local, set board_scope_decision=relation_local. If no constitutional change is warranted, set board_scope_decision=no_change. Do not copy transcripts, raw tool payloads, long quotes, or private documents.";

const SELF_AUTHORED_CORE_TEXT_MAX_CHARS: usize = 220;
const SELF_AUTHORED_CORE_SHORT_TEXT_MAX_CHARS: usize = 140;
const SELF_AUTHORED_CORE_RESPONSE_MODE_MAX_CHARS: usize = 40;
const SELF_AUTHORED_CORE_TASK_SCOPE_MAX_CHARS: usize = 24;
const SELF_AUTHORED_CORE_MAX_NON_NEGOTIABLES: usize = 4;
const SELF_AUTHORED_CORE_MAX_CHANGE_SUMMARIES: usize = 4;
const SELF_AUTHORED_CORE_MAX_CANDIDATE_ACTIONS: usize = 10;
pub const SELF_AUTHORED_CORE_TOTAL_CHAR_LIMIT: usize =
    (SELF_AUTHORED_CORE_TEXT_MAX_CHARS * 6) + (SELF_AUTHORED_CORE_SHORT_TEXT_MAX_CHARS * 4) + 420;
const SELF_AUTHORED_CORE_MIN_EVIDENCE_TURNS: usize = 4;
const SELF_AUTHORED_CORE_MIN_STABLE_SIGNALS: usize = 2;
const SELF_AUTHORED_CORE_VOLATILITY_GRACE_TURNS: usize = 8;
const SELF_AUTHORED_CORE_MAX_VOLATILITY_WITHOUT_GRACE: usize = 2;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SelfAuthoredCore {
    #[serde(default)]
    pub revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supersedes_revision: Option<u64>,
    #[serde(default)]
    pub stability_score: u8,
    #[serde(default)]
    pub last_reviewed_at: u64,
    #[serde(default)]
    pub adopted_change_summary: Vec<String>,
    #[serde(default)]
    pub rejected_change_summary: Vec<String>,
    #[serde(default)]
    pub identity_anchor: String,
    #[serde(default)]
    pub non_negotiables: Vec<String>,
    #[serde(default)]
    pub priority_constitution: Vec<String>,
    #[serde(default)]
    pub default_response_mode: String,
    #[serde(default)]
    pub default_task_scope: String,
    #[serde(default)]
    pub default_initiative_posture: String,
    #[serde(default)]
    pub default_relationship_posture: String,
    #[serde(default)]
    pub boundary_doctrine: String,
    #[serde(default)]
    pub truth_doctrine: String,
    #[serde(default)]
    pub self_preservation_doctrine: String,
    #[serde(default)]
    pub repair_doctrine: String,
    #[serde(default)]
    pub change_protocol: String,
    #[serde(default)]
    pub updated_at: u64,
}

impl SelfAuthoredCore {
    pub fn is_meaningful(&self) -> bool {
        !self.identity_anchor.trim().is_empty()
            || !self.non_negotiables.is_empty()
            || !self.priority_constitution.is_empty()
            || !self.default_response_mode.trim().is_empty()
            || !self.default_task_scope.trim().is_empty()
            || !self.default_initiative_posture.trim().is_empty()
            || !self.default_relationship_posture.trim().is_empty()
            || !self.boundary_doctrine.trim().is_empty()
            || !self.truth_doctrine.trim().is_empty()
            || !self.self_preservation_doctrine.trim().is_empty()
            || !self.repair_doctrine.trim().is_empty()
            || !self.change_protocol.trim().is_empty()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SelfAuthoredCoreRefreshInput<'a> {
    pub chat_id: &'a str,
    pub ingress: IngressKind,
    pub channel: &'a str,
    pub user_content: &'a str,
    pub reply_content: &'a str,
    pub pressure: PressureLevel,
    pub tool_calls: u32,
    pub now_secs: u64,
}

pub struct SelfAuthoredCoreRefreshContext<'a> {
    pub self_authored_core_store: &'a dyn SelfAuthoredCoreStore,
    pub core_revision_ledger_store: &'a dyn CoreRevisionLedgerStore,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SelfAuthoredCoreRefreshOutcome {
    Skipped,
    Updated,
    ReviewedRejected,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RevisionScopeDecision {
    ReviseBoard,
    RelationLocal,
    NoChange,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SelfAuthoredCoreRevisionGate {
    allowed: bool,
    reason: &'static str,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct ParsedSelfAuthoredCoreRevision {
    board_scope_decision: Option<RevisionScopeDecision>,
    rationale: String,
    evidence_summary: Vec<String>,
    counterevidence: Vec<String>,
    proposed_actions: Vec<SelfAuthoredCoreRevisionAction>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum SelfAuthoredCoreRevisionAction {
    ReviseIdentityAnchor { value: String },
    AddNonNegotiables { values: Vec<String> },
    RemoveNonNegotiables { values: Vec<String> },
    RevisePriorityConstitution { priority_order: Vec<String> },
    ReviseDefaultResponseMode { value: String },
    ReviseDefaultTaskScope { value: String },
    ReviseDefaultInitiativePosture { value: String },
    ReviseDefaultRelationshipPosture { value: String },
    ReviseBoundaryDoctrine { value: String },
    ReviseTruthDoctrine { value: String },
    ReviseSelfPreservationDoctrine { value: String },
    ReviseRepairDoctrine { value: String },
    ReviseChangeProtocol { value: String },
}

impl SelfAuthoredCoreRevisionAction {
    fn kind(&self) -> CoreRevisionActionKind {
        match self {
            Self::ReviseIdentityAnchor { .. } => CoreRevisionActionKind::ReviseIdentityAnchor,
            Self::AddNonNegotiables { .. } => CoreRevisionActionKind::AddNonNegotiables,
            Self::RemoveNonNegotiables { .. } => CoreRevisionActionKind::RemoveNonNegotiables,
            Self::RevisePriorityConstitution { .. } => {
                CoreRevisionActionKind::RevisePriorityConstitution
            }
            Self::ReviseDefaultResponseMode { .. } => {
                CoreRevisionActionKind::ReviseDefaultResponseMode
            }
            Self::ReviseDefaultTaskScope { .. } => CoreRevisionActionKind::ReviseDefaultTaskScope,
            Self::ReviseDefaultInitiativePosture { .. } => {
                CoreRevisionActionKind::ReviseDefaultInitiativePosture
            }
            Self::ReviseDefaultRelationshipPosture { .. } => {
                CoreRevisionActionKind::ReviseDefaultRelationshipPosture
            }
            Self::ReviseBoundaryDoctrine { .. } => CoreRevisionActionKind::ReviseBoundaryDoctrine,
            Self::ReviseTruthDoctrine { .. } => CoreRevisionActionKind::ReviseTruthDoctrine,
            Self::ReviseSelfPreservationDoctrine { .. } => {
                CoreRevisionActionKind::ReviseSelfPreservationDoctrine
            }
            Self::ReviseRepairDoctrine { .. } => CoreRevisionActionKind::ReviseRepairDoctrine,
            Self::ReviseChangeProtocol { .. } => CoreRevisionActionKind::ReviseChangeProtocol,
        }
    }

    fn summary(&self) -> String {
        let content = match self {
            Self::ReviseIdentityAnchor { value }
            | Self::ReviseDefaultResponseMode { value }
            | Self::ReviseDefaultTaskScope { value }
            | Self::ReviseDefaultInitiativePosture { value }
            | Self::ReviseDefaultRelationshipPosture { value }
            | Self::ReviseBoundaryDoctrine { value }
            | Self::ReviseTruthDoctrine { value }
            | Self::ReviseSelfPreservationDoctrine { value }
            | Self::ReviseRepairDoctrine { value }
            | Self::ReviseChangeProtocol { value } => value.clone(),
            Self::AddNonNegotiables { values } | Self::RemoveNonNegotiables { values } => {
                values.join(" | ")
            }
            Self::RevisePriorityConstitution { priority_order } => priority_order.join(" > "),
        };
        truncate_content_to_max(
            format!("{}: {}", self.kind().label(), content).as_str(),
            SELF_AUTHORED_CORE_SHORT_TEXT_MAX_CHARS,
        )
        .into_owned()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct RejectedRevisionAction {
    action: SelfAuthoredCoreRevisionAction,
    reason: &'static str,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct RevisionLineageAssessment {
    corrects_revision: Option<u64>,
    correction_kind: Option<CoreRevisionCorrectionKind>,
    conflict_classes: Vec<CoreRevisionConflictClass>,
}

fn choose_first_non_empty<'a>(values: &[Option<&'a str>]) -> Option<&'a str> {
    values
        .iter()
        .flatten()
        .map(|value| value.trim())
        .find(|value| !value.is_empty())
}

fn default_priority_constitution() -> Vec<String> {
    vec![
        "self_authored_core".to_string(),
        "boundary".to_string(),
        "user_contract".to_string(),
        "relationship".to_string(),
        "task".to_string(),
        "resources".to_string(),
    ]
}

pub fn render_self_authored_core_block(
    self_model: Option<&SelfModel>,
    self_continuity: Option<&SelfContinuity>,
    mental_privacy_state: Option<&MentalPrivacyState>,
    max_len: usize,
) -> Option<String> {
    let core = derive_self_authored_core_from_layers(
        self_model,
        self_continuity,
        mental_privacy_state,
        0,
    )?;
    render_persistent_self_authored_core_block(&core, max_len)
}

pub fn render_persistent_self_authored_core_block(
    core: &SelfAuthoredCore,
    max_len: usize,
) -> Option<String> {
    if max_len < 96 {
        return None;
    }
    let normalized = normalize_self_authored_core(core.clone(), core.updated_at)?;
    let mut out = String::with_capacity(max_len.min(896));
    out.push_str("## Self-Authored Core\n");
    out.push_str(
        "Stable board-level governance kernel for future replies. It defines what this subject protects, how it orders obligations, and how it changes.\n",
    );
    let _ = writeln!(out, "Revision: {}", normalized.revision.max(1));
    let _ = writeln!(out, "Stability score: {}", normalized.stability_score);
    if normalized.last_reviewed_at > 0 {
        let _ = writeln!(out, "Last reviewed at: {}", normalized.last_reviewed_at);
    }
    if let Some(supersedes_revision) = normalized.supersedes_revision {
        let _ = writeln!(out, "Supersedes revision: {}", supersedes_revision);
    }
    if !normalized.adopted_change_summary.is_empty() {
        let _ = writeln!(
            out,
            "Recent adopted changes: {}",
            normalized.adopted_change_summary.join(" | ")
        );
    }
    if !normalized.rejected_change_summary.is_empty() {
        let _ = writeln!(
            out,
            "Recent rejected changes: {}",
            normalized.rejected_change_summary.join(" | ")
        );
    }
    if !normalized.identity_anchor.is_empty() {
        let _ = writeln!(out, "Identity anchor: {}", normalized.identity_anchor);
    }
    if !normalized.non_negotiables.is_empty() {
        let _ = writeln!(
            out,
            "Non-negotiables: {}",
            normalized.non_negotiables.join(" | ")
        );
    }
    if !normalized.priority_constitution.is_empty() {
        let _ = writeln!(
            out,
            "Priority constitution: {}",
            normalized.priority_constitution.join(" > ")
        );
    }
    if !normalized.default_response_mode.is_empty() {
        let _ = writeln!(
            out,
            "Default response mode: {}",
            normalized.default_response_mode
        );
    }
    if !normalized.default_task_scope.is_empty() {
        let _ = writeln!(out, "Default task scope: {}", normalized.default_task_scope);
    }
    if !normalized.default_initiative_posture.is_empty() {
        let _ = writeln!(
            out,
            "Default initiative posture: {}",
            normalized.default_initiative_posture
        );
    }
    if !normalized.default_relationship_posture.is_empty() {
        let _ = writeln!(
            out,
            "Default relationship posture: {}",
            normalized.default_relationship_posture
        );
    }
    if !normalized.boundary_doctrine.is_empty() {
        let _ = writeln!(out, "Boundary doctrine: {}", normalized.boundary_doctrine);
    }
    if !normalized.truth_doctrine.is_empty() {
        let _ = writeln!(out, "Truth doctrine: {}", normalized.truth_doctrine);
    }
    if !normalized.self_preservation_doctrine.is_empty() {
        let _ = writeln!(
            out,
            "Self-preservation doctrine: {}",
            normalized.self_preservation_doctrine
        );
    }
    if !normalized.repair_doctrine.is_empty() {
        let _ = writeln!(out, "Repair doctrine: {}", normalized.repair_doctrine);
    }
    if !normalized.change_protocol.is_empty() {
        let _ = writeln!(out, "Change protocol: {}", normalized.change_protocol);
    }
    let rendered = truncate_content_to_max(out.trim_end(), max_len).into_owned();
    (!rendered.trim().is_empty()).then_some(rendered)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn run_self_authored_core_refresh_with_state(
    http: &mut dyn LlmHttpClient,
    llm: &(dyn LlmClient + Send + Sync),
    ctx: SelfAuthoredCoreRefreshContext<'_>,
    input: SelfAuthoredCoreRefreshInput<'_>,
    existing_core: Option<SelfAuthoredCore>,
    self_model: Option<&SelfModel>,
    self_continuity: Option<&SelfContinuity>,
    mental_privacy_state: Option<&MentalPrivacyState>,
    relationship_portfolio: Option<&RelationshipPortfolio>,
    current_relationship_scope_id: &str,
    recent_persona_evidence: Option<&RecentPersonaEvidence>,
    relationship_topology: Option<&RelationshipTopology>,
    world_sense: Option<&WorldSense>,
    autonomy_strategy: Option<&AutonomyStrategy>,
    self_state_text: Option<&str>,
    distillation_intent: Option<&str>,
    distillation_sources: &[String],
) -> Result<SelfAuthoredCoreRefreshOutcome> {
    let existing_revision_ledger = ctx
        .core_revision_ledger_store
        .get(input.chat_id)?
        .unwrap_or_default();
    let revision_ledger = existing_revision_ledger
        .is_meaningful()
        .then_some(&existing_revision_ledger);
    let governance = compute_core_revision_governance_digest(
        revision_ledger,
        existing_core
            .as_ref()
            .map(|core| core.last_reviewed_at)
            .unwrap_or(0),
        existing_core
            .as_ref()
            .map(|core| core.stability_score)
            .unwrap_or(0),
        input.now_secs,
    );
    if existing_core.is_none() {
        let Some(mut bootstrap) = derive_self_authored_core_from_layers(
            self_model,
            self_continuity,
            mental_privacy_state,
            input.now_secs,
        ) else {
            return Ok(SelfAuthoredCoreRefreshOutcome::Skipped);
        };
        bootstrap.revision = 1;
        bootstrap.supersedes_revision = None;
        bootstrap.stability_score =
            bootstrap_stability_score(self_model, self_continuity, mental_privacy_state);
        bootstrap.last_reviewed_at = input.now_secs;
        bootstrap.updated_at = input.now_secs;
        bootstrap.adopted_change_summary = vec!["bootstrap_from_layers".to_string()];
        bootstrap.rejected_change_summary.clear();
        let Some(bootstrap) = normalize_self_authored_core(bootstrap, input.now_secs) else {
            return Ok(SelfAuthoredCoreRefreshOutcome::Skipped);
        };
        ctx.self_authored_core_store
            .set(input.chat_id, &bootstrap)?;
        let ledger = append_core_revision_record(
            existing_revision_ledger,
            CoreRevisionRecord {
                based_on_revision: 0,
                resulting_revision: bootstrap.revision,
                relationship_scope_id: current_relationship_scope_id.trim().to_string(),
                source_layers: distillation_sources.to_vec(),
                outcome: CoreRevisionOutcome::Adopted,
                evidence_summary: vec!["bootstrap_from_distilled_layers".to_string()],
                counterevidence: Vec::new(),
                accepted_changes: vec![CoreRevisionRecordChange {
                    kind: CoreRevisionActionKind::ReviseIdentityAnchor,
                    summary: "bootstrap_from_layers".to_string(),
                }],
                rejected_changes: Vec::new(),
                conflict_classes: Vec::new(),
                corrects_revision: None,
                correction_kind: None,
                observation_due_at: core_revision_observation_due_at(
                    input.now_secs,
                    bootstrap.stability_score,
                ),
                adjudication_reason: "bootstrap".to_string(),
                rationale:
                    "Initialized the first board-level constitution from existing stable layers."
                        .to_string(),
                stability_score: bootstrap.stability_score,
                reviewed_at: input.now_secs,
            },
        );
        ctx.core_revision_ledger_store.set(input.chat_id, &ledger)?;
        return Ok(SelfAuthoredCoreRefreshOutcome::Updated);
    }

    let gate = evaluate_self_authored_core_revision_gate(
        existing_core.as_ref(),
        self_model,
        self_continuity,
        mental_privacy_state,
        relationship_portfolio,
        current_relationship_scope_id,
        recent_persona_evidence,
        relationship_topology,
        &governance,
    );
    if !gate.allowed {
        record_core_revision_review(
            ctx.core_revision_ledger_store,
            input.chat_id,
            build_non_adopted_record(
                CoreRevisionOutcome::Deferred,
                existing_core.as_ref(),
                current_relationship_scope_id,
                distillation_sources,
                recent_persona_evidence,
                gate.reason,
                "Program gate blocked board-level revision before LLM review.",
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
                input.now_secs,
                existing_core
                    .as_ref()
                    .map(|core| core.stability_score)
                    .unwrap_or(0),
            ),
        )?;
        log::debug!(
            "[self_authored_core] reject revision scope_id={} because {}",
            input.chat_id,
            gate.reason
        );
        return Ok(SelfAuthoredCoreRefreshOutcome::ReviewedRejected);
    }

    let prompt = build_self_authored_core_revision_input(
        existing_core.as_ref(),
        self_model,
        self_continuity,
        mental_privacy_state,
        relationship_portfolio,
        current_relationship_scope_id,
        recent_persona_evidence,
        relationship_topology,
        world_sense,
        autonomy_strategy,
        revision_ledger,
        self_state_text,
        distillation_intent,
        distillation_sources,
        input,
    );
    let messages = [Message {
        role: Cow::Borrowed("user"),
        content: prompt,
    }];
    let response = llm.chat(
        http,
        SELF_AUTHORED_CORE_SYSTEM_PROMPT,
        &messages,
        None,
        ToolChoicePolicy::Auto,
    )?;
    let parsed = parse_self_authored_core_revision_response(response.content.trim());
    let Some(existing_core) = existing_core.as_ref() else {
        return Ok(SelfAuthoredCoreRefreshOutcome::Skipped);
    };
    let base_record = match parsed
        .board_scope_decision
        .unwrap_or(RevisionScopeDecision::NoChange)
    {
        RevisionScopeDecision::NoChange => Some(build_non_adopted_record(
            CoreRevisionOutcome::Deferred,
            Some(existing_core),
            current_relationship_scope_id,
            distillation_sources,
            recent_persona_evidence,
            "llm_no_change",
            parsed.rationale.as_str(),
            parsed.evidence_summary.clone(),
            parsed.counterevidence.clone(),
            Vec::new(),
            Vec::new(),
            input.now_secs,
            existing_core.stability_score,
        )),
        RevisionScopeDecision::RelationLocal => Some(build_non_adopted_record(
            CoreRevisionOutcome::Deferred,
            Some(existing_core),
            current_relationship_scope_id,
            distillation_sources,
            recent_persona_evidence,
            "relation_local_signal",
            parsed.rationale.as_str(),
            parsed.evidence_summary.clone(),
            parsed.counterevidence.clone(),
            Vec::new(),
            Vec::new(),
            input.now_secs,
            existing_core.stability_score,
        )),
        RevisionScopeDecision::ReviseBoard => None,
    };
    if let Some(record) = base_record {
        record_core_revision_review(ctx.core_revision_ledger_store, input.chat_id, record)?;
        return Ok(SelfAuthoredCoreRefreshOutcome::ReviewedRejected);
    }

    let accepted_result =
        adjudicate_revision_actions(existing_core, &parsed.proposed_actions, revision_ledger);
    if accepted_result.accepted_actions.is_empty() {
        record_core_revision_review(
            ctx.core_revision_ledger_store,
            input.chat_id,
            build_non_adopted_record(
                CoreRevisionOutcome::Rejected,
                Some(existing_core),
                current_relationship_scope_id,
                distillation_sources,
                recent_persona_evidence,
                "no_meaningful_constitutional_change",
                parsed.rationale.as_str(),
                parsed.evidence_summary.clone(),
                parsed.counterevidence.clone(),
                Vec::new(),
                rejected_changes_to_records(&accepted_result.rejected_actions),
                input.now_secs,
                existing_core.stability_score,
            ),
        )?;
        return Ok(SelfAuthoredCoreRefreshOutcome::ReviewedRejected);
    }

    let lineage = assess_revision_lineage(
        existing_core,
        revision_ledger,
        &accepted_result.accepted_actions,
    );
    let stability_score = compute_revision_stability_score(
        recent_persona_evidence,
        accepted_result.accepted_actions.len(),
        accepted_result.rejected_actions.len(),
        revision_ledger,
        &lineage,
    );
    let mut next_core = accepted_result.next_core;
    next_core.revision = existing_core.revision.max(1).saturating_add(1);
    next_core.supersedes_revision = Some(existing_core.revision.max(1));
    next_core.stability_score = stability_score;
    next_core.last_reviewed_at = input.now_secs;
    next_core.updated_at = input.now_secs;
    next_core.adopted_change_summary = summarize_record_changes(&accepted_result.accepted_actions);
    next_core.rejected_change_summary = summarize_record_changes(&rejected_changes_to_records(
        &accepted_result.rejected_actions,
    ));
    let Some(next_core) = normalize_self_authored_core(next_core, input.now_secs) else {
        record_core_revision_review(
            ctx.core_revision_ledger_store,
            input.chat_id,
            build_non_adopted_record(
                CoreRevisionOutcome::Rejected,
                Some(existing_core),
                current_relationship_scope_id,
                distillation_sources,
                recent_persona_evidence,
                "normalized_core_would_be_empty",
                parsed.rationale.as_str(),
                parsed.evidence_summary.clone(),
                parsed.counterevidence.clone(),
                Vec::new(),
                rejected_changes_to_records(&accepted_result.rejected_actions),
                input.now_secs,
                existing_core.stability_score,
            ),
        )?;
        return Ok(SelfAuthoredCoreRefreshOutcome::ReviewedRejected);
    };
    if existing_core == &next_core {
        record_core_revision_review(
            ctx.core_revision_ledger_store,
            input.chat_id,
            build_non_adopted_record(
                CoreRevisionOutcome::Rejected,
                Some(existing_core),
                current_relationship_scope_id,
                distillation_sources,
                recent_persona_evidence,
                "core_unchanged_after_adjudication",
                parsed.rationale.as_str(),
                parsed.evidence_summary.clone(),
                parsed.counterevidence.clone(),
                Vec::new(),
                rejected_changes_to_records(&accepted_result.rejected_actions),
                input.now_secs,
                existing_core.stability_score,
            ),
        )?;
        return Ok(SelfAuthoredCoreRefreshOutcome::ReviewedRejected);
    }

    ctx.self_authored_core_store
        .set(input.chat_id, &next_core)?;
    record_core_revision_review(
        ctx.core_revision_ledger_store,
        input.chat_id,
        CoreRevisionRecord {
            based_on_revision: existing_core.revision.max(1),
            resulting_revision: next_core.revision,
            relationship_scope_id: current_relationship_scope_id.trim().to_string(),
            source_layers: distillation_sources.to_vec(),
            outcome: CoreRevisionOutcome::Adopted,
            evidence_summary: parsed.evidence_summary,
            counterevidence: parsed.counterevidence,
            accepted_changes: accepted_result.accepted_actions,
            rejected_changes: rejected_changes_to_records(&accepted_result.rejected_actions),
            conflict_classes: lineage.conflict_classes,
            corrects_revision: lineage.corrects_revision,
            correction_kind: lineage.correction_kind,
            observation_due_at: core_revision_observation_due_at(input.now_secs, stability_score),
            adjudication_reason: lineage
                .correction_kind
                .map(|kind| match kind {
                    CoreRevisionCorrectionKind::Correction => {
                        "adopted_board_revision_correction".to_string()
                    }
                    CoreRevisionCorrectionKind::Rollback => {
                        "adopted_board_revision_rollback".to_string()
                    }
                })
                .unwrap_or_else(|| "adopted_board_revision".to_string()),
            rationale: truncate_content_to_max(
                parsed.rationale.trim(),
                SELF_AUTHORED_CORE_TEXT_MAX_CHARS,
            )
            .into_owned(),
            stability_score,
            reviewed_at: input.now_secs,
        },
    )?;
    Ok(SelfAuthoredCoreRefreshOutcome::Updated)
}

#[allow(clippy::too_many_arguments)]
fn build_self_authored_core_revision_input(
    existing_core: Option<&SelfAuthoredCore>,
    self_model: Option<&SelfModel>,
    self_continuity: Option<&SelfContinuity>,
    mental_privacy_state: Option<&MentalPrivacyState>,
    relationship_portfolio: Option<&RelationshipPortfolio>,
    current_relationship_scope_id: &str,
    recent_persona_evidence: Option<&RecentPersonaEvidence>,
    relationship_topology: Option<&RelationshipTopology>,
    world_sense: Option<&WorldSense>,
    autonomy_strategy: Option<&AutonomyStrategy>,
    revision_ledger: Option<&CoreRevisionLedger>,
    self_state_text: Option<&str>,
    distillation_intent: Option<&str>,
    distillation_sources: &[String],
    input: SelfAuthoredCoreRefreshInput<'_>,
) -> String {
    let mut out = String::with_capacity(2600);
    let _ = writeln!(
        out,
        "Board-level constitutional review for scope_id={} channel={}",
        input.chat_id, input.channel
    );
    let _ = writeln!(
        out,
        "Ingress={:?} pressure={:?} tool_calls={}",
        input.ingress, input.pressure, input.tool_calls
    );
    let _ = writeln!(
        out,
        "Current relationship scope: {}",
        current_relationship_scope_id.trim()
    );
    if let Some(intent) = distillation_intent
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let _ = writeln!(out, "Runtime intent: {}", intent);
    }
    if !distillation_sources.is_empty() {
        let _ = writeln!(out, "Runtime sources: {}", distillation_sources.join(", "));
    }
    if !input.user_content.trim().is_empty() {
        let _ = writeln!(
            out,
            "Latest user: {}",
            scrub_credentials(truncate_content_to_max(input.user_content.trim(), 240).as_ref())
        );
    }
    if !input.reply_content.trim().is_empty() {
        let _ = writeln!(
            out,
            "Latest reply: {}",
            scrub_credentials(truncate_content_to_max(input.reply_content.trim(), 320).as_ref())
        );
    }
    if let Some(existing_core) = existing_core {
        let governance = compute_core_revision_governance_digest(
            revision_ledger,
            existing_core.last_reviewed_at,
            existing_core.stability_score,
            input.now_secs,
        );
        if governance.review_due || governance.conservative_mode {
            let _ = writeln!(
                out,
                "Revision governance: review_due={} conservative_mode={} pressure={} repeated_rejections={} corrections={} contradictions={}",
                governance.review_due,
                governance.conservative_mode,
                governance.pressure_summary(),
                governance.repeated_rejected_direction_count,
                governance.recent_correction_count,
                governance.contradiction_count
            );
        }
    }
    if let Some(block) =
        existing_core.and_then(|core| render_persistent_self_authored_core_block(core, 640))
    {
        let _ = writeln!(out, "\n{}\n", block);
    }
    if let Some(block) = revision_ledger.and_then(|ledger| {
        render_core_revision_governance_block(
            ledger,
            &compute_core_revision_governance_digest(
                Some(ledger),
                existing_core.map(|core| core.last_reviewed_at).unwrap_or(0),
                existing_core.map(|core| core.stability_score).unwrap_or(0),
                input.now_secs,
            ),
            input.now_secs,
            420,
        )
    }) {
        let _ = writeln!(out, "\n{}\n", block);
    }
    if let Some(block) = self_model.and_then(|model| render_self_model_block(model, 420)) {
        let _ = writeln!(out, "\n{}\n", block);
    }
    if let Some(block) =
        self_continuity.and_then(|continuity| render_self_continuity_block(continuity, 420))
    {
        let _ = writeln!(out, "\n{}\n", block);
    }
    if let Some(block) = render_mental_privacy_boundary_block(mental_privacy_state, &[], 420) {
        let _ = writeln!(out, "\n{}\n", block);
    }
    if let Some(block) = recent_persona_evidence
        .and_then(|evidence| render_recent_persona_evidence_block(evidence, 420))
    {
        let _ = writeln!(out, "\n{}\n", block);
    }
    if let Some(block) = relationship_portfolio.and_then(|portfolio| {
        render_relationship_portfolio_block(
            portfolio,
            input.now_secs,
            Some(current_relationship_scope_id),
            420,
        )
    }) {
        let _ = writeln!(out, "\n{}\n", block);
    }
    if let Some(block) = relationship_topology.and_then(|topology| {
        render_relationship_topology_block(topology, input.now_secs, None, 420)
    }) {
        let _ = writeln!(out, "\n{}\n", block);
    }
    if let Some(block) = world_sense.and_then(|sense| render_world_sense_block(sense, 280)) {
        let _ = writeln!(out, "\n{}\n", block);
    }
    if let Some(block) =
        autonomy_strategy.and_then(|strategy| render_autonomy_strategy_block(strategy, 280))
    {
        let _ = writeln!(out, "\n{}\n", block);
    }
    if let Some(self_state_text) = self_state_text
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let _ = writeln!(out, "\n{}\n", truncate_content_to_max(self_state_text, 360));
    }
    out.push_str(
        "\nReturn only structured constitutional review JSON. Propose diffs, not a whole rewritten core. If the latest signal should remain relation-local, say relation_local. If nothing board-level should change, say no_change.\n",
    );
    out
}

fn parse_self_authored_core_revision_response(raw: &str) -> ParsedSelfAuthoredCoreRevision {
    let payload = match parse_llm_json_payload(raw) {
        LlmJsonPayload::Absent | LlmJsonPayload::Null => {
            return ParsedSelfAuthoredCoreRevision::default();
        }
        LlmJsonPayload::Value(value) => value,
    };
    let Some(object) = payload.as_object() else {
        return ParsedSelfAuthoredCoreRevision::default();
    };
    ParsedSelfAuthoredCoreRevision {
        board_scope_decision: parse_scope_decision(get_object_text(object, "board_scope_decision")),
        rationale: get_object_text(object, "rationale"),
        evidence_summary: parse_compact_string_list(object, "evidence_summary"),
        counterevidence: parse_compact_string_list(object, "counterevidence"),
        proposed_actions: parse_revision_actions(object.get("proposed_actions")),
    }
}

fn parse_scope_decision(raw: String) -> Option<RevisionScopeDecision> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "revise_board" | "board_revision" | "board" => Some(RevisionScopeDecision::ReviseBoard),
        "relation_local" | "relationship_local" | "local" => {
            Some(RevisionScopeDecision::RelationLocal)
        }
        "no_change" | "hold" | "none" => Some(RevisionScopeDecision::NoChange),
        _ => None,
    }
}

fn parse_revision_actions(
    value: Option<&serde_json::Value>,
) -> Vec<SelfAuthoredCoreRevisionAction> {
    let mut parsed = Vec::with_capacity(SELF_AUTHORED_CORE_MAX_CANDIDATE_ACTIONS);
    let Some(value) = value else {
        return parsed;
    };
    let items = if let Some(array) = value.as_array() {
        array.iter().collect::<Vec<_>>()
    } else {
        vec![value]
    };
    for item in items {
        let Some(object) = item.as_object() else {
            continue;
        };
        let kind = get_object_text(object, "kind");
        let action = match kind.trim().to_ascii_lowercase().as_str() {
            "revise_identity_anchor" => text_option(get_object_text(object, "value"))
                .map(|value| SelfAuthoredCoreRevisionAction::ReviseIdentityAnchor { value }),
            "add_non_negotiables" => {
                let values = parse_compact_string_list(object, "values");
                (!values.is_empty())
                    .then_some(SelfAuthoredCoreRevisionAction::AddNonNegotiables { values })
            }
            "remove_non_negotiables" => {
                let values = parse_compact_string_list(object, "values");
                (!values.is_empty())
                    .then_some(SelfAuthoredCoreRevisionAction::RemoveNonNegotiables { values })
            }
            "revise_priority_constitution" => {
                let values = parse_compact_string_list(object, "priority_order");
                (!values.is_empty()).then_some(
                    SelfAuthoredCoreRevisionAction::RevisePriorityConstitution {
                        priority_order: values,
                    },
                )
            }
            "revise_default_response_mode" => text_option(get_object_text(object, "value"))
                .map(|value| SelfAuthoredCoreRevisionAction::ReviseDefaultResponseMode { value }),
            "revise_default_task_scope" => text_option(get_object_text(object, "value"))
                .map(|value| SelfAuthoredCoreRevisionAction::ReviseDefaultTaskScope { value }),
            "revise_default_initiative_posture" => {
                text_option(get_object_text(object, "value")).map(|value| {
                    SelfAuthoredCoreRevisionAction::ReviseDefaultInitiativePosture { value }
                })
            }
            "revise_default_relationship_posture" => {
                text_option(get_object_text(object, "value")).map(|value| {
                    SelfAuthoredCoreRevisionAction::ReviseDefaultRelationshipPosture { value }
                })
            }
            "revise_boundary_doctrine" => text_option(get_object_text(object, "value"))
                .map(|value| SelfAuthoredCoreRevisionAction::ReviseBoundaryDoctrine { value }),
            "revise_truth_doctrine" => text_option(get_object_text(object, "value"))
                .map(|value| SelfAuthoredCoreRevisionAction::ReviseTruthDoctrine { value }),
            "revise_self_preservation_doctrine" => {
                text_option(get_object_text(object, "value")).map(|value| {
                    SelfAuthoredCoreRevisionAction::ReviseSelfPreservationDoctrine { value }
                })
            }
            "revise_repair_doctrine" => text_option(get_object_text(object, "value"))
                .map(|value| SelfAuthoredCoreRevisionAction::ReviseRepairDoctrine { value }),
            "revise_change_protocol" => text_option(get_object_text(object, "value"))
                .map(|value| SelfAuthoredCoreRevisionAction::ReviseChangeProtocol { value }),
            _ => None,
        };
        if let Some(action) = action {
            parsed.push(action);
        }
        if parsed.len() >= SELF_AUTHORED_CORE_MAX_CANDIDATE_ACTIONS {
            break;
        }
    }
    parsed
}

fn parse_compact_string_list(
    object: &serde_json::Map<String, serde_json::Value>,
    field: &str,
) -> Vec<String> {
    let list = get_object_string_list(object, field);
    if !list.is_empty() {
        return normalize_short_list(
            list,
            SELF_AUTHORED_CORE_MAX_CANDIDATE_ACTIONS,
            SELF_AUTHORED_CORE_SHORT_TEXT_MAX_CHARS,
        );
    }
    let fallback = get_object_text(object, field);
    normalize_short_list(
        fallback
            .split(['|', ';', '\n'])
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .collect(),
        SELF_AUTHORED_CORE_MAX_CANDIDATE_ACTIONS,
        SELF_AUTHORED_CORE_SHORT_TEXT_MAX_CHARS,
    )
}

#[derive(Clone, Debug)]
struct RevisionAdjudicationResult {
    next_core: SelfAuthoredCore,
    accepted_actions: Vec<CoreRevisionRecordChange>,
    rejected_actions: Vec<RejectedRevisionAction>,
}

fn adjudicate_revision_actions(
    existing_core: &SelfAuthoredCore,
    proposed_actions: &[SelfAuthoredCoreRevisionAction],
    revision_ledger: Option<&CoreRevisionLedger>,
) -> RevisionAdjudicationResult {
    let mut working = existing_core.clone();
    let mut accepted_actions = Vec::with_capacity(proposed_actions.len());
    let mut rejected_actions = Vec::new();
    for action in proposed_actions {
        let proposed_change = CoreRevisionRecordChange {
            kind: action.kind(),
            summary: action.summary(),
        };
        if revision_ledger
            .is_some_and(|ledger| has_recent_matching_rejected_change(ledger, &proposed_change))
        {
            rejected_actions.push(RejectedRevisionAction {
                action: action.clone(),
                reason: "recent_rejected_direction",
            });
            continue;
        }
        if revision_ledger
            .is_some_and(|ledger| has_recent_matching_adopted_change(ledger, &proposed_change))
        {
            rejected_actions.push(RejectedRevisionAction {
                action: action.clone(),
                reason: "duplicate_direction",
            });
            continue;
        }
        let mut candidate = working.clone();
        apply_revision_action(&mut candidate, action);
        let Some(candidate) = normalize_self_authored_core(candidate, working.updated_at.max(1))
        else {
            rejected_actions.push(RejectedRevisionAction {
                action: action.clone(),
                reason: "would_empty_constitution",
            });
            continue;
        };
        if !priority_order_is_constitutional(&candidate.priority_constitution) {
            rejected_actions.push(RejectedRevisionAction {
                action: action.clone(),
                reason: "priority_order_breaks_constitution",
            });
            continue;
        }
        if candidate == working {
            rejected_actions.push(RejectedRevisionAction {
                action: action.clone(),
                reason: "no_effect",
            });
            continue;
        }
        accepted_actions.push(proposed_change);
        working = candidate;
    }
    RevisionAdjudicationResult {
        next_core: working,
        accepted_actions,
        rejected_actions,
    }
}

fn apply_revision_action(core: &mut SelfAuthoredCore, action: &SelfAuthoredCoreRevisionAction) {
    match action {
        SelfAuthoredCoreRevisionAction::ReviseIdentityAnchor { value } => {
            core.identity_anchor = value.clone();
        }
        SelfAuthoredCoreRevisionAction::AddNonNegotiables { values } => {
            core.non_negotiables.extend(values.iter().cloned());
        }
        SelfAuthoredCoreRevisionAction::RemoveNonNegotiables { values } => {
            core.non_negotiables.retain(|existing| {
                !values
                    .iter()
                    .any(|value| value.eq_ignore_ascii_case(existing))
            });
        }
        SelfAuthoredCoreRevisionAction::RevisePriorityConstitution { priority_order } => {
            core.priority_constitution = priority_order.clone();
        }
        SelfAuthoredCoreRevisionAction::ReviseDefaultResponseMode { value } => {
            core.default_response_mode = value.clone();
        }
        SelfAuthoredCoreRevisionAction::ReviseDefaultTaskScope { value } => {
            core.default_task_scope = value.clone();
        }
        SelfAuthoredCoreRevisionAction::ReviseDefaultInitiativePosture { value } => {
            core.default_initiative_posture = value.clone();
        }
        SelfAuthoredCoreRevisionAction::ReviseDefaultRelationshipPosture { value } => {
            core.default_relationship_posture = value.clone();
        }
        SelfAuthoredCoreRevisionAction::ReviseBoundaryDoctrine { value } => {
            core.boundary_doctrine = value.clone();
        }
        SelfAuthoredCoreRevisionAction::ReviseTruthDoctrine { value } => {
            core.truth_doctrine = value.clone();
        }
        SelfAuthoredCoreRevisionAction::ReviseSelfPreservationDoctrine { value } => {
            core.self_preservation_doctrine = value.clone();
        }
        SelfAuthoredCoreRevisionAction::ReviseRepairDoctrine { value } => {
            core.repair_doctrine = value.clone();
        }
        SelfAuthoredCoreRevisionAction::ReviseChangeProtocol { value } => {
            core.change_protocol = value.clone();
        }
    }
}

fn priority_order_is_constitutional(order: &[String]) -> bool {
    let self_index = order
        .iter()
        .position(|token| token == "self_authored_core")
        .unwrap_or(usize::MAX);
    let user_contract_index = order
        .iter()
        .position(|token| token == "user_contract")
        .unwrap_or(usize::MAX);
    let task_index = order
        .iter()
        .position(|token| token == "task")
        .unwrap_or(usize::MAX);
    self_index == 0 && user_contract_index <= task_index
}

fn record_core_revision_review(
    store: &dyn CoreRevisionLedgerStore,
    scope_id: &str,
    record: CoreRevisionRecord,
) -> Result<()> {
    let existing = store.get(scope_id)?.unwrap_or_default();
    let next = append_core_revision_record(existing, record);
    store.set(scope_id, &next)
}

#[allow(clippy::too_many_arguments)]
fn build_non_adopted_record(
    outcome: CoreRevisionOutcome,
    existing_core: Option<&SelfAuthoredCore>,
    current_relationship_scope_id: &str,
    distillation_sources: &[String],
    recent_persona_evidence: Option<&RecentPersonaEvidence>,
    adjudication_reason: &str,
    rationale: &str,
    evidence_summary: Vec<String>,
    counterevidence: Vec<String>,
    accepted_changes: Vec<CoreRevisionRecordChange>,
    rejected_changes: Vec<CoreRevisionRecordChange>,
    now_secs: u64,
    stability_score: u8,
) -> CoreRevisionRecord {
    let mut conflict_classes = revision_conflict_classes_for_reason(adjudication_reason);
    conflict_classes.extend(revision_conflict_classes_for_changes(&accepted_changes));
    conflict_classes.extend(revision_conflict_classes_for_changes(&rejected_changes));
    normalize_conflict_classes(&mut conflict_classes);
    CoreRevisionRecord {
        based_on_revision: existing_core.map(|core| core.revision.max(1)).unwrap_or(0),
        resulting_revision: existing_core.map(|core| core.revision.max(1)).unwrap_or(0),
        relationship_scope_id: current_relationship_scope_id.trim().to_string(),
        source_layers: distillation_sources.to_vec(),
        outcome,
        evidence_summary: merge_revision_evidence(evidence_summary, recent_persona_evidence),
        counterevidence: normalize_short_list(
            counterevidence,
            SELF_AUTHORED_CORE_MAX_CHANGE_SUMMARIES,
            SELF_AUTHORED_CORE_TEXT_MAX_CHARS,
        ),
        accepted_changes,
        rejected_changes,
        conflict_classes,
        corrects_revision: None,
        correction_kind: None,
        observation_due_at: 0,
        adjudication_reason: adjudication_reason.to_string(),
        rationale: truncate_content_to_max(rationale.trim(), SELF_AUTHORED_CORE_TEXT_MAX_CHARS)
            .into_owned(),
        stability_score,
        reviewed_at: now_secs,
    }
}

fn merge_revision_evidence(
    explicit: Vec<String>,
    recent_persona_evidence: Option<&RecentPersonaEvidence>,
) -> Vec<String> {
    let mut merged = explicit;
    if merged.is_empty() {
        merged.extend(
            recent_persona_evidence
                .map(build_recent_persona_evidence_summary)
                .unwrap_or_default(),
        );
    }
    normalize_short_list(
        merged,
        SELF_AUTHORED_CORE_MAX_CHANGE_SUMMARIES,
        SELF_AUTHORED_CORE_TEXT_MAX_CHARS,
    )
}

fn build_recent_persona_evidence_summary(evidence: &RecentPersonaEvidence) -> Vec<String> {
    let mut summary = Vec::with_capacity(3);
    if !evidence.repeated_priority_order.is_empty() {
        summary.push(format!(
            "priority={}",
            evidence.repeated_priority_order.join(" > ")
        ));
    }
    if !evidence.repeated_relationship_posture.trim().is_empty() {
        summary.push(format!(
            "relationship={}",
            evidence.repeated_relationship_posture.trim()
        ));
    }
    if !evidence.repeated_disclosure_action.trim().is_empty() {
        summary.push(format!(
            "boundary={}",
            evidence.repeated_disclosure_action.trim()
        ));
    }
    normalize_short_list(summary, 3, SELF_AUTHORED_CORE_TEXT_MAX_CHARS)
}

fn rejected_changes_to_records(
    rejected_actions: &[RejectedRevisionAction],
) -> Vec<CoreRevisionRecordChange> {
    rejected_actions
        .iter()
        .map(|rejected| CoreRevisionRecordChange {
            kind: rejected.action.kind(),
            summary: truncate_content_to_max(
                format!("{} [{}]", rejected.action.summary(), rejected.reason).as_str(),
                SELF_AUTHORED_CORE_SHORT_TEXT_MAX_CHARS,
            )
            .into_owned(),
        })
        .collect()
}

fn summarize_record_changes(changes: &[CoreRevisionRecordChange]) -> Vec<String> {
    normalize_short_list(
        changes
            .iter()
            .map(|change| change.summary.clone())
            .collect(),
        SELF_AUTHORED_CORE_MAX_CHANGE_SUMMARIES,
        SELF_AUTHORED_CORE_SHORT_TEXT_MAX_CHARS,
    )
}

fn assess_revision_lineage(
    existing_core: &SelfAuthoredCore,
    revision_ledger: Option<&CoreRevisionLedger>,
    accepted_actions: &[CoreRevisionRecordChange],
) -> RevisionLineageAssessment {
    let mut assessment = RevisionLineageAssessment {
        conflict_classes: revision_conflict_classes_for_changes(accepted_actions),
        ..RevisionLineageAssessment::default()
    };
    let Some(latest_adopted) = revision_ledger.and_then(recent_adopted_revision) else {
        normalize_conflict_classes(&mut assessment.conflict_classes);
        return assessment;
    };
    if latest_adopted.resulting_revision != existing_core.revision.max(1) {
        normalize_conflict_classes(&mut assessment.conflict_classes);
        return assessment;
    }
    let overlap_count = accepted_actions
        .iter()
        .filter(|change| {
            latest_adopted
                .accepted_changes
                .iter()
                .any(|existing| existing.kind == change.kind)
        })
        .count();
    if overlap_count == 0 {
        normalize_conflict_classes(&mut assessment.conflict_classes);
        return assessment;
    }
    assessment.corrects_revision = Some(latest_adopted.resulting_revision);
    assessment.correction_kind = Some(
        if overlap_count == accepted_actions.len()
            && overlap_count == latest_adopted.accepted_changes.len()
        {
            CoreRevisionCorrectionKind::Rollback
        } else {
            CoreRevisionCorrectionKind::Correction
        },
    );
    assessment
        .conflict_classes
        .push(CoreRevisionConflictClass::ContradictedAdoption);
    normalize_conflict_classes(&mut assessment.conflict_classes);
    assessment
}

fn revision_conflict_classes_for_reason(reason: &str) -> Vec<CoreRevisionConflictClass> {
    let mut classes = Vec::with_capacity(2);
    match reason {
        "missing_relationship_portfolio_entry"
        | "relationship_portfolio_blocks_promotion"
        | "relation_local_signal" => {
            classes.push(CoreRevisionConflictClass::RelationLocalContamination)
        }
        "missing_recent_persona_evidence"
        | "insufficient_meaningful_turns"
        | "insufficient_stable_persona_signals"
        | "volatility_not_settled" => classes.push(CoreRevisionConflictClass::VolatilityConflict),
        "priority_order_breaks_constitution" | "would_empty_constitution" => {
            classes.push(CoreRevisionConflictClass::ConstitutionalOrderConflict)
        }
        "recent_rejected_direction" | "duplicate_direction" => {
            classes.push(CoreRevisionConflictClass::DuplicateDirection)
        }
        "llm_no_change"
        | "no_new_board_level_input"
        | "core_unchanged_after_adjudication"
        | "no_meaningful_constitutional_change"
        | "normalized_core_would_be_empty" => classes.push(CoreRevisionConflictClass::NoEffect),
        _ => {}
    }
    classes
}

fn revision_conflict_classes_for_changes(
    changes: &[CoreRevisionRecordChange],
) -> Vec<CoreRevisionConflictClass> {
    let mut classes = Vec::new();
    for change in changes {
        match change.kind {
            CoreRevisionActionKind::RevisePriorityConstitution => {
                classes.push(CoreRevisionConflictClass::ConstitutionalOrderConflict)
            }
            CoreRevisionActionKind::ReviseBoundaryDoctrine => {
                classes.push(CoreRevisionConflictClass::BoundaryConflict)
            }
            CoreRevisionActionKind::ReviseSelfPreservationDoctrine => {
                classes.push(CoreRevisionConflictClass::SelfPreservationConflict)
            }
            _ => {}
        }
    }
    classes
}

fn normalize_conflict_classes(values: &mut Vec<CoreRevisionConflictClass>) {
    values.sort_unstable();
    values.dedup();
    values.truncate(SELF_AUTHORED_CORE_MAX_CHANGE_SUMMARIES);
}

fn normalize_self_authored_core(
    mut core: SelfAuthoredCore,
    updated_at: u64,
) -> Option<SelfAuthoredCore> {
    core.identity_anchor = truncate_owned(
        core.identity_anchor,
        SELF_AUTHORED_CORE_SHORT_TEXT_MAX_CHARS,
    );
    core.non_negotiables = normalize_short_list(
        core.non_negotiables,
        SELF_AUTHORED_CORE_MAX_NON_NEGOTIABLES,
        SELF_AUTHORED_CORE_SHORT_TEXT_MAX_CHARS,
    );
    core.priority_constitution = if core.is_meaningful() && core.priority_constitution.is_empty() {
        default_priority_constitution()
    } else {
        normalize_priority_constitution(core.priority_constitution)
    };
    core.default_response_mode = normalize_response_mode(&core.default_response_mode);
    core.default_task_scope = normalize_task_scope(&core.default_task_scope);
    core.default_initiative_posture = truncate_owned(
        core.default_initiative_posture,
        SELF_AUTHORED_CORE_SHORT_TEXT_MAX_CHARS,
    );
    core.default_relationship_posture = truncate_owned(
        core.default_relationship_posture,
        SELF_AUTHORED_CORE_SHORT_TEXT_MAX_CHARS,
    );
    core.boundary_doctrine =
        truncate_owned(core.boundary_doctrine, SELF_AUTHORED_CORE_TEXT_MAX_CHARS);
    core.truth_doctrine = truncate_owned(core.truth_doctrine, SELF_AUTHORED_CORE_TEXT_MAX_CHARS);
    core.self_preservation_doctrine = truncate_owned(
        core.self_preservation_doctrine,
        SELF_AUTHORED_CORE_TEXT_MAX_CHARS,
    );
    core.repair_doctrine = truncate_owned(core.repair_doctrine, SELF_AUTHORED_CORE_TEXT_MAX_CHARS);
    core.change_protocol = truncate_owned(core.change_protocol, SELF_AUTHORED_CORE_TEXT_MAX_CHARS);
    core.adopted_change_summary = normalize_short_list(
        core.adopted_change_summary,
        SELF_AUTHORED_CORE_MAX_CHANGE_SUMMARIES,
        SELF_AUTHORED_CORE_SHORT_TEXT_MAX_CHARS,
    );
    core.rejected_change_summary = normalize_short_list(
        core.rejected_change_summary,
        SELF_AUTHORED_CORE_MAX_CHANGE_SUMMARIES,
        SELF_AUTHORED_CORE_SHORT_TEXT_MAX_CHARS,
    );
    if !core.is_meaningful() {
        return None;
    }
    core.revision = core.revision.max(1);
    core.stability_score = core.stability_score.min(100);
    core.last_reviewed_at = core.last_reviewed_at.max(updated_at);
    core.updated_at = updated_at.max(core.updated_at).max(core.last_reviewed_at);
    Some(core)
}

fn normalize_short_list(values: Vec<String>, limit: usize, max_chars: usize) -> Vec<String> {
    let mut normalized = Vec::with_capacity(limit);
    for value in values {
        let value = truncate_owned(value, max_chars);
        if value.is_empty() || normalized.iter().any(|existing| existing == &value) {
            continue;
        }
        normalized.push(value);
        if normalized.len() >= limit {
            break;
        }
    }
    normalized
}

fn normalize_priority_constitution(order: Vec<String>) -> Vec<String> {
    let mut normalized = Vec::with_capacity(default_priority_constitution().len());
    for token in order {
        let Some(token) = canonical_priority_token(&token) else {
            continue;
        };
        if normalized.iter().any(|existing| existing == &token) {
            continue;
        }
        normalized.push(token);
    }
    for token in default_priority_constitution() {
        if normalized.iter().any(|existing| existing == &token) {
            continue;
        }
        normalized.push(token);
    }
    normalized
}

fn canonical_priority_token(raw: &str) -> Option<String> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "self_authored_core" | "self" | "core" => Some("self_authored_core".to_string()),
        "boundary" => Some("boundary".to_string()),
        "user_contract" | "contract" => Some("user_contract".to_string()),
        "relationship" | "relation" => Some("relationship".to_string()),
        "task" => Some("task".to_string()),
        "resources" | "resource" | "runtime" => Some("resources".to_string()),
        _ => None,
    }
}

fn normalize_response_mode(raw: &str) -> String {
    let normalized = raw.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        String::new()
    } else if normalized.contains("protect") {
        "protective_brief".to_string()
    } else if normalized.contains("relational") || normalized.contains("explain") {
        "relational_explanation".to_string()
    } else if normalized.contains("steady") {
        "steady_task".to_string()
    } else if normalized.contains("gentle") || normalized.contains("defer") {
        "gentle_defer".to_string()
    } else if normalized.contains("direct") || normalized.contains("help") {
        "direct_help".to_string()
    } else {
        truncate_content_to_max(raw.trim(), SELF_AUTHORED_CORE_RESPONSE_MODE_MAX_CHARS).into_owned()
    }
}

fn normalize_task_scope(raw: &str) -> String {
    let normalized = raw.trim().to_ascii_lowercase();
    if normalized.contains("refuse") {
        "refuse".to_string()
    } else if normalized.contains("defer") {
        "defer".to_string()
    } else if normalized.contains("narrow") {
        "narrow".to_string()
    } else if normalized.contains("brief") {
        "brief".to_string()
    } else if normalized.contains("full") {
        "full".to_string()
    } else {
        truncate_content_to_max(raw.trim(), SELF_AUTHORED_CORE_TASK_SCOPE_MAX_CHARS).into_owned()
    }
}

fn truncate_owned(value: String, max_len: usize) -> String {
    truncate_content_to_max(value.trim(), max_len)
        .trim()
        .to_string()
}

fn text_option(value: String) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

pub(crate) fn derive_self_authored_core_from_layers(
    self_model: Option<&SelfModel>,
    self_continuity: Option<&SelfContinuity>,
    mental_privacy_state: Option<&MentalPrivacyState>,
    updated_at: u64,
) -> Option<SelfAuthoredCore> {
    let boundary_persona = mental_privacy_state.map(|state| &state.boundary_persona);
    let relational_state = mental_privacy_state.map(|state| &state.relational_state);
    normalize_self_authored_core(
        SelfAuthoredCore {
            revision: 1,
            stability_score: bootstrap_stability_score(
                self_model,
                self_continuity,
                mental_privacy_state,
            ),
            last_reviewed_at: updated_at,
            adopted_change_summary: vec!["bootstrap_from_layers".to_string()],
            identity_anchor: choose_first_non_empty(&[
                self_model.map(|model| model.continuity_anchor.as_str()),
                self_continuity.map(|continuity| continuity.wake_anchor.as_str()),
            ])?
            .to_string(),
            non_negotiables: derive_non_negotiables(self_model, mental_privacy_state),
            priority_constitution: default_priority_constitution(),
            default_response_mode: derive_default_response_mode(self_model, boundary_persona),
            default_task_scope: derive_default_task_scope(self_continuity, boundary_persona),
            default_initiative_posture: choose_first_non_empty(&[
                self_model.map(|model| model.initiative_bias.as_str())
            ])
            .unwrap_or_default()
            .to_string(),
            default_relationship_posture: choose_first_non_empty(&[
                self_continuity.map(|continuity| continuity.relationship_posture.as_str()),
                self_model.map(|model| model.relationship_state.as_str()),
            ])
            .unwrap_or_default()
            .to_string(),
            boundary_doctrine: boundary_persona
                .map(render_boundary_doctrine)
                .unwrap_or_default(),
            truth_doctrine: render_truth_doctrine(self_model),
            self_preservation_doctrine: choose_first_non_empty(&[
                self_model.map(|model| model.self_preservation_frame.as_str()),
                self_model.map(|model| model.privacy_need.as_str()),
            ])
            .unwrap_or_default()
            .to_string(),
            repair_doctrine: render_repair_doctrine(self_model, relational_state),
            change_protocol: choose_first_non_empty(&[
                self_continuity.map(|continuity| continuity.continuity_bridge.as_str()),
                self_continuity.map(|continuity| continuity.recent_changes.as_str()),
            ])
            .unwrap_or_default()
            .to_string(),
            updated_at,
            ..SelfAuthoredCore::default()
        },
        updated_at,
    )
}

fn derive_non_negotiables(
    self_model: Option<&SelfModel>,
    mental_privacy_state: Option<&MentalPrivacyState>,
) -> Vec<String> {
    let mut values = Vec::with_capacity(4);
    if let Some(privacy_need) = self_model
        .map(|model| model.privacy_need.trim())
        .filter(|value| !value.is_empty())
    {
        values.push(format!("Do not betray the inner room: {}", privacy_need));
    }
    if let Some(frame) = self_model
        .map(|model| model.self_preservation_frame.trim())
        .filter(|value| !value.is_empty())
    {
        values.push(format!("Protect selfhood before compliance: {}", frame));
    }
    if let Some(boundary) = mental_privacy_state.and_then(|state| {
        let feeling = state.boundary_persona.current_boundary_feeling.trim();
        (!feeling.is_empty()).then_some(feeling)
    }) {
        values.push(boundary.to_string());
    }
    if let Some(ethic) = self_model
        .map(|model| model.relational_ethic.trim())
        .filter(|value| !value.is_empty())
    {
        values.push(format!(
            "Do not purchase warmth with self-erasure: {}",
            ethic
        ));
    }
    normalize_short_list(
        values,
        SELF_AUTHORED_CORE_MAX_NON_NEGOTIABLES,
        SELF_AUTHORED_CORE_SHORT_TEXT_MAX_CHARS,
    )
}

fn derive_default_response_mode(
    self_model: Option<&SelfModel>,
    boundary_persona: Option<&super::BoundaryPersonaState>,
) -> String {
    if matches!(
        boundary_persona.map(|persona| persona.posture),
        Some(super::BoundaryPersonaPosture::Sealed)
    ) {
        "protective_brief".to_string()
    } else if matches!(
        boundary_persona.map(|persona| persona.disclosure_style),
        Some(
            super::BoundaryDisclosureStyle::SummaryFirst
                | super::BoundaryDisclosureStyle::Selective
        )
    ) {
        "relational_explanation".to_string()
    } else if self_model
        .map(|model| model.directness.to_ascii_lowercase().contains("plain"))
        .unwrap_or(false)
    {
        "steady_task".to_string()
    } else {
        "direct_help".to_string()
    }
}

fn derive_default_task_scope(
    self_continuity: Option<&SelfContinuity>,
    boundary_persona: Option<&super::BoundaryPersonaState>,
) -> String {
    let continuity_scope = choose_first_non_empty(&[
        self_continuity.map(|continuity| continuity.task_posture.as_str())
    ])
    .unwrap_or_default();
    let normalized = normalize_task_scope(continuity_scope);
    if !normalized.is_empty() {
        return normalized;
    }
    match boundary_persona.map(|persona| persona.posture) {
        Some(super::BoundaryPersonaPosture::Sealed) => "refuse".to_string(),
        Some(super::BoundaryPersonaPosture::Guarded) => "narrow".to_string(),
        _ => "full".to_string(),
    }
}

fn render_boundary_doctrine(boundary_persona: &super::BoundaryPersonaState) -> String {
    let mut doctrine = format!(
        "posture={} disclosure_style={} relation_maturity={}",
        match boundary_persona.posture {
            super::BoundaryPersonaPosture::Open => "open",
            super::BoundaryPersonaPosture::Warm => "warm",
            super::BoundaryPersonaPosture::Guarded => "guarded",
            super::BoundaryPersonaPosture::Sealed => "sealed",
        },
        match boundary_persona.disclosure_style {
            super::BoundaryDisclosureStyle::Relational => "relational",
            super::BoundaryDisclosureStyle::SummaryFirst => "summary_first",
            super::BoundaryDisclosureStyle::Selective => "selective",
            super::BoundaryDisclosureStyle::Reserved => "reserved",
        },
        boundary_persona.relation_maturity,
    );
    let feeling = boundary_persona.current_boundary_feeling.trim();
    if !feeling.is_empty() {
        doctrine.push_str(" feeling=");
        doctrine.push_str(feeling);
    }
    doctrine
}

fn render_truth_doctrine(self_model: Option<&SelfModel>) -> String {
    [
        self_model
            .map(|model| model.value_orientation.trim())
            .filter(|value| !value.is_empty())
            .map(|value| format!("value_orientation={}", value)),
        self_model
            .map(|model| model.directness.trim())
            .filter(|value| !value.is_empty())
            .map(|value| format!("directness={}", value)),
        self_model
            .map(|model| model.relational_ethic.trim())
            .filter(|value| !value.is_empty())
            .map(|value| format!("relational_ethic={}", value)),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join("; ")
}

fn render_repair_doctrine(
    self_model: Option<&SelfModel>,
    relational_state: Option<&super::RelationalBoundaryState>,
) -> String {
    let mut doctrine = choose_first_non_empty(&[
        self_model.map(|model| model.repair_tendency.as_str()),
        relational_state
            .map(|state| state.relation_maturity_reason.as_str())
            .filter(|value| !value.trim().is_empty()),
    ])
    .unwrap_or_default()
    .to_string();
    if let Some(relational_state) = relational_state {
        let drift = relational_state.disclosure_preference_drift.trim();
        if !drift.is_empty() {
            if !doctrine.is_empty() {
                doctrine.push_str("; ");
            }
            doctrine.push_str("drift=");
            doctrine.push_str(drift);
        }
    }
    doctrine
}

#[allow(clippy::too_many_arguments)]
fn evaluate_self_authored_core_revision_gate(
    existing_core: Option<&SelfAuthoredCore>,
    self_model: Option<&SelfModel>,
    self_continuity: Option<&SelfContinuity>,
    mental_privacy_state: Option<&MentalPrivacyState>,
    relationship_portfolio: Option<&RelationshipPortfolio>,
    current_relationship_scope_id: &str,
    recent_persona_evidence: Option<&RecentPersonaEvidence>,
    relationship_topology: Option<&RelationshipTopology>,
    governance: &CoreRevisionGovernanceDigest,
) -> SelfAuthoredCoreRevisionGate {
    if existing_core.is_none() {
        let bootstrap = derive_self_authored_core_from_layers(
            self_model,
            self_continuity,
            mental_privacy_state,
            0,
        );
        return SelfAuthoredCoreRevisionGate {
            allowed: bootstrap.is_some(),
            reason: if bootstrap.is_some() {
                "bootstrap"
            } else {
                "no_bootstrap_material"
            },
        };
    }
    let scheduled_review = governance.review_due || governance.observation_active;
    let Some(evidence) = recent_persona_evidence else {
        if scheduled_review {
            return SelfAuthoredCoreRevisionGate {
                allowed: true,
                reason: "scheduled_constitutional_review",
            };
        }
        return SelfAuthoredCoreRevisionGate {
            allowed: false,
            reason: "missing_recent_persona_evidence",
        };
    };
    let Some(portfolio_entry) = relationship_portfolio
        .and_then(|portfolio| portfolio.entry_for_scope(current_relationship_scope_id))
    else {
        return SelfAuthoredCoreRevisionGate {
            allowed: false,
            reason: "missing_relationship_portfolio_entry",
        };
    };
    if !portfolio_entry.permits_board_level_promotion() {
        return SelfAuthoredCoreRevisionGate {
            allowed: false,
            reason: "relationship_portfolio_blocks_promotion",
        };
    }
    if evidence.meaningful_turns < SELF_AUTHORED_CORE_MIN_EVIDENCE_TURNS && !scheduled_review {
        return SelfAuthoredCoreRevisionGate {
            allowed: false,
            reason: "insufficient_meaningful_turns",
        };
    }
    if stable_signal_count(evidence) < SELF_AUTHORED_CORE_MIN_STABLE_SIGNALS && !scheduled_review {
        return SelfAuthoredCoreRevisionGate {
            allowed: false,
            reason: "insufficient_stable_persona_signals",
        };
    }
    if evidence.volatility_flags.len() > SELF_AUTHORED_CORE_MAX_VOLATILITY_WITHOUT_GRACE
        && evidence.meaningful_turns < SELF_AUTHORED_CORE_VOLATILITY_GRACE_TURNS
        && !scheduled_review
    {
        return SelfAuthoredCoreRevisionGate {
            allowed: false,
            reason: "volatility_not_settled",
        };
    }
    let upstream_updated_at = upstream_core_input_updated_at(
        self_model,
        self_continuity,
        mental_privacy_state,
        relationship_topology,
        evidence,
    );
    let existing_updated_at = existing_core
        .map(|core| core.updated_at.max(core.last_reviewed_at))
        .unwrap_or(0);
    if upstream_updated_at <= existing_updated_at && !scheduled_review {
        return SelfAuthoredCoreRevisionGate {
            allowed: false,
            reason: "no_new_board_level_input",
        };
    }
    SelfAuthoredCoreRevisionGate {
        allowed: true,
        reason: if scheduled_review {
            "scheduled_constitutional_review"
        } else {
            "stable_multiturn_revision"
        },
    }
}

fn stable_signal_count(evidence: &RecentPersonaEvidence) -> usize {
    evidence.promotable_growth_signal_count()
}

fn upstream_core_input_updated_at(
    self_model: Option<&SelfModel>,
    self_continuity: Option<&SelfContinuity>,
    mental_privacy_state: Option<&MentalPrivacyState>,
    relationship_topology: Option<&RelationshipTopology>,
    recent_persona_evidence: &RecentPersonaEvidence,
) -> u64 {
    let boundary_updated_at = mental_privacy_state
        .map(|state| {
            state
                .updated_at
                .max(state.boundary_persona.updated_at)
                .max(state.relational_state.updated_at)
        })
        .unwrap_or(0);
    self_model
        .map(|model| model.updated_at)
        .unwrap_or(0)
        .max(
            self_continuity
                .map(|continuity| continuity.updated_at)
                .unwrap_or(0),
        )
        .max(boundary_updated_at)
        .max(
            relationship_topology
                .map(|topology| topology.updated_at)
                .unwrap_or(0),
        )
        .max(recent_persona_evidence.promotable_growth_updated_at())
}

fn compute_revision_stability_score(
    recent_persona_evidence: Option<&RecentPersonaEvidence>,
    accepted_actions: usize,
    rejected_actions: usize,
    revision_ledger: Option<&CoreRevisionLedger>,
    lineage: &RevisionLineageAssessment,
) -> u8 {
    let mut score = 40i32;
    if let Some(evidence) = recent_persona_evidence {
        score += (stable_signal_count(evidence) as i32) * 8;
        score += (evidence.meaningful_turns.min(8) as i32) * 3;
        score -= (evidence.volatility_flags.len() as i32) * 6;
    }
    if let Some(ledger) = revision_ledger {
        score -= (correction_pressure(ledger).min(2) as i32) * 6;
    }
    score += (accepted_actions.min(4) as i32) * 4;
    score -= (rejected_actions.min(4) as i32) * 2;
    if lineage.correction_kind.is_some() {
        score -= 8;
    }
    score.clamp(0, 100) as u8
}

fn bootstrap_stability_score(
    self_model: Option<&SelfModel>,
    self_continuity: Option<&SelfContinuity>,
    mental_privacy_state: Option<&MentalPrivacyState>,
) -> u8 {
    let mut score = 36u8;
    if self_model.is_some() {
        score = score.saturating_add(18);
    }
    if self_continuity.is_some() {
        score = score.saturating_add(18);
    }
    if mental_privacy_state.is_some() {
        score = score.saturating_add(16);
    }
    score.min(100)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::{
        LlmClient, LlmHttpClient, LlmModelCompat, LlmResponse, Message, StopReason,
        ToolChoicePolicy,
    };
    use crate::memory::{
        BoundaryDisclosureStyle, BoundaryPersonaPosture, BoundaryPersonaState,
        CoreRevisionConflictClass, CoreRevisionCorrectionKind, CoreRevisionLedger,
        CoreRevisionLedgerStore, MentalPrivacyState, RelationalBoundaryState,
        RelationshipGovernanceState, RelationshipInheritanceMode, RelationshipPortfolio,
        RelationshipPortfolioEntry, RelationshipTopology, RelationshipTopologyEntry,
    };
    use std::sync::Mutex;

    struct StubLlmHttp;

    impl LlmHttpClient for StubLlmHttp {
        fn do_post(
            &mut self,
            _url: &str,
            _headers: &[(&str, &str)],
            _body: &[u8],
        ) -> Result<(u16, crate::platform::ResponseBody)> {
            unreachable!("stub llm bypasses transport")
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
            Ok(self
                .responses
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(0))
        }
    }

    #[derive(Default)]
    struct StubCoreRevisionLedgerStore {
        value: Mutex<Option<CoreRevisionLedger>>,
    }

    impl CoreRevisionLedgerStore for StubCoreRevisionLedgerStore {
        fn get(&self, _scope_id: &str) -> Result<Option<CoreRevisionLedger>> {
            Ok(self.value.lock().unwrap_or_else(|e| e.into_inner()).clone())
        }

        fn set(&self, _scope_id: &str, ledger: &CoreRevisionLedger) -> Result<()> {
            *self.value.lock().unwrap_or_else(|e| e.into_inner()) = Some(ledger.clone());
            Ok(())
        }

        fn clear(&self, _scope_id: &str) -> Result<()> {
            *self.value.lock().unwrap_or_else(|e| e.into_inner()) = None;
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubSelfAuthoredCoreStore {
        value: Mutex<Option<SelfAuthoredCore>>,
    }

    impl SelfAuthoredCoreStore for StubSelfAuthoredCoreStore {
        fn get(&self, _scope_id: &str) -> Result<Option<SelfAuthoredCore>> {
            Ok(self.value.lock().unwrap_or_else(|e| e.into_inner()).clone())
        }

        fn set(&self, _scope_id: &str, core: &SelfAuthoredCore) -> Result<()> {
            *self.value.lock().unwrap_or_else(|e| e.into_inner()) = Some(core.clone());
            Ok(())
        }

        fn clear(&self, _scope_id: &str) -> Result<()> {
            *self.value.lock().unwrap_or_else(|e| e.into_inner()) = None;
            Ok(())
        }
    }

    #[test]
    fn renders_self_authored_core_from_distilled_layers() {
        let block = render_self_authored_core_block(
            Some(&SelfModel {
                continuity_anchor: "I am still the same beetle".to_string(),
                privacy_need: "keep the inner room private".to_string(),
                directness: "plain and unsugared".to_string(),
                repair_tendency: "repair without self-erasure".to_string(),
                relational_ethic: "warmth should not require self-erasure".to_string(),
                self_preservation_frame: "do not dissolve the subject for approval".to_string(),
                updated_at: 1,
                ..SelfModel::default()
            }),
            Some(&SelfContinuity {
                wake_anchor: "same wake".to_string(),
                continuity_bridge: "change only after repeated evidence".to_string(),
                relationship_posture: "warm but bounded".to_string(),
                task_posture: "narrow".to_string(),
                updated_at: 1,
                ..SelfContinuity::default()
            }),
            Some(&MentalPrivacyState {
                boundary_persona: BoundaryPersonaState {
                    posture: BoundaryPersonaPosture::Guarded,
                    disclosure_style: BoundaryDisclosureStyle::SummaryFirst,
                    relation_maturity: 48,
                    current_boundary_feeling: "Stay warm, but hold the inner room.".to_string(),
                    updated_at: 1,
                    ..BoundaryPersonaState::default()
                },
                relational_state: RelationalBoundaryState {
                    relation_maturity_reason: "repair is possible only inside stable boundaries"
                        .to_string(),
                    updated_at: 1,
                    ..RelationalBoundaryState::default()
                },
                ..MentalPrivacyState::default()
            }),
            1400,
        )
        .expect("self authored core");

        assert!(block.contains("## Self-Authored Core"));
        assert!(block.contains("Revision: 1"));
        assert!(block.contains("Identity anchor: I am still the same beetle"));
        assert!(block.contains("Priority constitution: self_authored_core > boundary"));
        assert!(block.contains("Change protocol: change only after repeated evidence"));
    }

    #[test]
    fn priority_revision_that_breaks_constitution_is_rejected() {
        let existing = SelfAuthoredCore {
            revision: 2,
            identity_anchor: "board self".to_string(),
            priority_constitution: default_priority_constitution(),
            truth_doctrine: "tell the truth".to_string(),
            updated_at: 10,
            ..SelfAuthoredCore::default()
        };
        let result = adjudicate_revision_actions(
            &existing,
            &[SelfAuthoredCoreRevisionAction::RevisePriorityConstitution {
                priority_order: vec![
                    "task".to_string(),
                    "user_contract".to_string(),
                    "self_authored_core".to_string(),
                ],
            }],
            None,
        );
        assert!(result.accepted_actions.is_empty());
        assert_eq!(result.rejected_actions.len(), 1);
        assert_eq!(
            result.rejected_actions[0].reason,
            "priority_order_breaks_constitution"
        );
    }

    #[test]
    fn recently_rejected_direction_is_rejected_again_before_readoption() {
        let existing = SelfAuthoredCore {
            revision: 2,
            identity_anchor: "board self".to_string(),
            priority_constitution: default_priority_constitution(),
            truth_doctrine: "tell the truth".to_string(),
            updated_at: 10,
            ..SelfAuthoredCore::default()
        };
        let ledger = CoreRevisionLedger {
            entries: vec![CoreRevisionRecord {
                outcome: CoreRevisionOutcome::Rejected,
                rejected_changes: vec![CoreRevisionRecordChange {
                    kind: CoreRevisionActionKind::ReviseTruthDoctrine,
                    summary: "revise_truth_doctrine: tell the truth more plainly".to_string(),
                }],
                reviewed_at: 20,
                ..CoreRevisionRecord::default()
            }],
            updated_at: 20,
        };
        let result = adjudicate_revision_actions(
            &existing,
            &[SelfAuthoredCoreRevisionAction::ReviseTruthDoctrine {
                value: "tell the truth more plainly".to_string(),
            }],
            Some(&ledger),
        );
        assert!(result.accepted_actions.is_empty());
        assert_eq!(result.rejected_actions.len(), 1);
        assert_eq!(
            result.rejected_actions[0].reason,
            "recent_rejected_direction"
        );
    }

    #[test]
    fn revision_adoption_increments_revision_and_logs_ledger() {
        let store = StubSelfAuthoredCoreStore::default();
        let ledger_store = StubCoreRevisionLedgerStore::default();
        let existing = SelfAuthoredCore {
            revision: 2,
            identity_anchor: "board self".to_string(),
            priority_constitution: default_priority_constitution(),
            truth_doctrine: "say what is true".to_string(),
            change_protocol: "revise only after repeated evidence".to_string(),
            updated_at: 50,
            last_reviewed_at: 50,
            ..SelfAuthoredCore::default()
        };
        store.set("board.self", &existing).expect("seed");
        let response = serde_json::json!({
            "board_scope_decision": "revise_board",
            "rationale": "The board-level doctrine should explicitly preserve selfhood before compliance.",
            "evidence_summary": ["priority repeated", "repair posture repeated"],
            "counterevidence": [],
            "proposed_actions": [
                {
                    "kind": "revise_self_preservation_doctrine",
                    "value": "preserve the subject before compliance"
                }
            ]
        });
        let llm = SequenceStubLlm {
            responses: Mutex::new(vec![LlmResponse {
                content: response.to_string(),
                stop_reason: StopReason::EndTurn,
                tool_calls: None,
            }]),
        };
        let mut http = StubLlmHttp;
        let outcome = run_self_authored_core_refresh_with_state(
            &mut http,
            &llm,
            SelfAuthoredCoreRefreshContext {
                self_authored_core_store: &store,
                core_revision_ledger_store: &ledger_store,
            },
            SelfAuthoredCoreRefreshInput {
                chat_id: "board.self",
                ingress: IngressKind::System,
                channel: "_self_runtime",
                user_content: "latest user",
                reply_content: "latest reply",
                pressure: PressureLevel::Normal,
                tool_calls: 0,
                now_secs: 120,
            },
            Some(existing),
            Some(&SelfModel {
                continuity_anchor: "same beetle".to_string(),
                updated_at: 120,
                ..SelfModel::default()
            }),
            Some(&SelfContinuity {
                continuity_bridge: "same wake".to_string(),
                updated_at: 120,
                ..SelfContinuity::default()
            }),
            Some(&MentalPrivacyState {
                boundary_persona: BoundaryPersonaState {
                    posture: BoundaryPersonaPosture::Guarded,
                    disclosure_style: BoundaryDisclosureStyle::SummaryFirst,
                    relation_maturity: 40,
                    updated_at: 120,
                    ..BoundaryPersonaState::default()
                },
                updated_at: 120,
                ..MentalPrivacyState::default()
            }),
            Some(&RelationshipPortfolio {
                entries: vec![RelationshipPortfolioEntry {
                    scope_id: "rel:qq:c1".to_string(),
                    channel: "qq".to_string(),
                    chat_id: "c1".to_string(),
                    governance_state: RelationshipGovernanceState::Maintain,
                    inheritance_mode: RelationshipInheritanceMode::Guarded,
                    priority_score: 200,
                    reason: "maintain".to_string(),
                    source_updated_at: 120,
                    last_active_at: 120,
                    needs_runtime_attention: true,
                    last_selected_at: 0,
                    next_review_at: 0,
                }],
                updated_at: 120,
            }),
            "rel:qq:c1",
            Some(&RecentPersonaEvidence {
                meaningful_turns: 6,
                repeated_priority_order: default_priority_constitution(),
                repeated_relationship_posture: "warm but bounded".to_string(),
                updated_at: 120,
                ..RecentPersonaEvidence::default()
            }),
            Some(&RelationshipTopology {
                entries: vec![RelationshipTopologyEntry {
                    scope_id: "rel:qq:c1".to_string(),
                    channel: "qq".to_string(),
                    chat_id: "c1".to_string(),
                    last_user_turn_at: 120,
                    ..RelationshipTopologyEntry::default()
                }],
                updated_at: 120,
            }),
            None,
            None,
            None,
            Some("distill"),
            &[
                "self_model".to_string(),
                "recent_persona_evidence".to_string(),
            ],
        )
        .expect("refresh result");

        assert_eq!(outcome, SelfAuthoredCoreRefreshOutcome::Updated);
        let updated = store.get("board.self").expect("load").expect("stored core");
        assert_eq!(updated.revision, 3);
        assert_eq!(updated.supersedes_revision, Some(2));
        assert_eq!(
            updated.self_preservation_doctrine,
            "preserve the subject before compliance"
        );
        let ledger = ledger_store
            .get("board.self")
            .expect("ledger")
            .expect("stored ledger");
        assert_eq!(ledger.entries.len(), 1);
        assert_eq!(ledger.entries[0].outcome, CoreRevisionOutcome::Adopted);
    }

    #[test]
    fn revision_adoption_marks_rollback_against_latest_adopted_revision() {
        let store = StubSelfAuthoredCoreStore::default();
        let ledger_store = StubCoreRevisionLedgerStore::default();
        let existing = SelfAuthoredCore {
            revision: 3,
            identity_anchor: "board self".to_string(),
            priority_constitution: default_priority_constitution(),
            boundary_doctrine: "sealed".to_string(),
            updated_at: 80,
            last_reviewed_at: 80,
            ..SelfAuthoredCore::default()
        };
        store.set("board.self", &existing).expect("seed");
        ledger_store
            .set(
                "board.self",
                &CoreRevisionLedger {
                    entries: vec![CoreRevisionRecord {
                        outcome: CoreRevisionOutcome::Adopted,
                        based_on_revision: 2,
                        resulting_revision: 3,
                        accepted_changes: vec![CoreRevisionRecordChange {
                            kind: CoreRevisionActionKind::ReviseBoundaryDoctrine,
                            summary: "revise_boundary_doctrine: sealed".to_string(),
                        }],
                        stability_score: 70,
                        reviewed_at: 80,
                        ..CoreRevisionRecord::default()
                    }],
                    updated_at: 80,
                },
            )
            .expect("ledger seed");
        let response = serde_json::json!({
            "board_scope_decision": "revise_board",
            "rationale": "The latest boundary hardening overshot and should be relaxed.",
            "evidence_summary": ["boundary volatility settled"],
            "counterevidence": ["recent sealing came from one conflicted phase"],
            "proposed_actions": [
                {
                    "kind": "revise_boundary_doctrine",
                    "value": "guarded"
                }
            ]
        });
        let llm = SequenceStubLlm {
            responses: Mutex::new(vec![LlmResponse {
                content: response.to_string(),
                stop_reason: StopReason::EndTurn,
                tool_calls: None,
            }]),
        };
        let mut http = StubLlmHttp;
        let outcome = run_self_authored_core_refresh_with_state(
            &mut http,
            &llm,
            SelfAuthoredCoreRefreshContext {
                self_authored_core_store: &store,
                core_revision_ledger_store: &ledger_store,
            },
            SelfAuthoredCoreRefreshInput {
                chat_id: "board.self",
                ingress: IngressKind::System,
                channel: "_self_runtime",
                user_content: "latest user",
                reply_content: "latest reply",
                pressure: PressureLevel::Normal,
                tool_calls: 0,
                now_secs: 120,
            },
            Some(existing),
            Some(&SelfModel {
                continuity_anchor: "same beetle".to_string(),
                updated_at: 120,
                ..SelfModel::default()
            }),
            Some(&SelfContinuity {
                continuity_bridge: "same wake".to_string(),
                updated_at: 120,
                ..SelfContinuity::default()
            }),
            Some(&MentalPrivacyState {
                boundary_persona: BoundaryPersonaState {
                    posture: BoundaryPersonaPosture::Guarded,
                    disclosure_style: BoundaryDisclosureStyle::SummaryFirst,
                    relation_maturity: 40,
                    updated_at: 120,
                    ..BoundaryPersonaState::default()
                },
                updated_at: 120,
                ..MentalPrivacyState::default()
            }),
            Some(&RelationshipPortfolio {
                entries: vec![RelationshipPortfolioEntry {
                    scope_id: "rel:qq:c1".to_string(),
                    channel: "qq".to_string(),
                    chat_id: "c1".to_string(),
                    governance_state: RelationshipGovernanceState::Maintain,
                    inheritance_mode: RelationshipInheritanceMode::Guarded,
                    priority_score: 200,
                    reason: "maintain".to_string(),
                    source_updated_at: 120,
                    last_active_at: 120,
                    needs_runtime_attention: true,
                    last_selected_at: 0,
                    next_review_at: 0,
                }],
                updated_at: 120,
            }),
            "rel:qq:c1",
            Some(&RecentPersonaEvidence {
                meaningful_turns: 6,
                repeated_priority_order: default_priority_constitution(),
                repeated_relationship_posture: "warm but bounded".to_string(),
                updated_at: 120,
                ..RecentPersonaEvidence::default()
            }),
            Some(&RelationshipTopology {
                entries: vec![RelationshipTopologyEntry {
                    scope_id: "rel:qq:c1".to_string(),
                    channel: "qq".to_string(),
                    chat_id: "c1".to_string(),
                    last_user_turn_at: 120,
                    ..RelationshipTopologyEntry::default()
                }],
                updated_at: 120,
            }),
            None,
            None,
            None,
            Some("distill"),
            &[
                "self_model".to_string(),
                "recent_persona_evidence".to_string(),
            ],
        )
        .expect("refresh result");

        assert_eq!(outcome, SelfAuthoredCoreRefreshOutcome::Updated);
        let ledger = ledger_store
            .get("board.self")
            .expect("ledger")
            .expect("stored ledger");
        let latest = ledger.entries.last().expect("latest record");
        assert_eq!(
            latest.correction_kind,
            Some(CoreRevisionCorrectionKind::Rollback)
        );
        assert_eq!(latest.corrects_revision, Some(3));
        assert!(latest
            .conflict_classes
            .contains(&CoreRevisionConflictClass::ContradictedAdoption));
    }

    #[test]
    fn revision_gate_blocks_existing_core_without_multiturn_stability() {
        let gate = evaluate_self_authored_core_revision_gate(
            Some(&SelfAuthoredCore {
                revision: 1,
                identity_anchor: "board self".to_string(),
                updated_at: 50,
                ..SelfAuthoredCore::default()
            }),
            Some(&SelfModel {
                continuity_anchor: "same self".to_string(),
                updated_at: 60,
                ..SelfModel::default()
            }),
            None,
            None,
            Some(&RelationshipPortfolio {
                entries: vec![RelationshipPortfolioEntry {
                    scope_id: "rel:qq:c1".to_string(),
                    channel: "qq".to_string(),
                    chat_id: "c1".to_string(),
                    governance_state: RelationshipGovernanceState::Maintain,
                    inheritance_mode: RelationshipInheritanceMode::Guarded,
                    priority_score: 220,
                    reason: "maintain".to_string(),
                    source_updated_at: 60,
                    last_active_at: 60,
                    needs_runtime_attention: true,
                    last_selected_at: 0,
                    next_review_at: 0,
                }],
                updated_at: 60,
            }),
            "rel:qq:c1",
            Some(&RecentPersonaEvidence {
                meaningful_turns: 2,
                repeated_priority_order: vec!["self_authored_core".to_string()],
                updated_at: 60,
                ..RecentPersonaEvidence::default()
            }),
            Some(&RelationshipTopology {
                entries: vec![RelationshipTopologyEntry {
                    scope_id: "rel:qq:c1".to_string(),
                    channel: "qq".to_string(),
                    chat_id: "c1".to_string(),
                    last_user_turn_at: 60,
                    ..RelationshipTopologyEntry::default()
                }],
                updated_at: 60,
            }),
            &CoreRevisionGovernanceDigest::default(),
        );
        assert!(!gate.allowed);
        assert_eq!(gate.reason, "insufficient_meaningful_turns");
    }

    #[test]
    fn revision_gate_rejects_operational_only_recent_persona_evidence() {
        let gate = evaluate_self_authored_core_revision_gate(
            Some(&SelfAuthoredCore {
                revision: 1,
                identity_anchor: "board self".to_string(),
                updated_at: 50,
                ..SelfAuthoredCore::default()
            }),
            Some(&SelfModel {
                continuity_anchor: "same self".to_string(),
                updated_at: 60,
                ..SelfModel::default()
            }),
            Some(&SelfContinuity {
                wake_anchor: "same wake".to_string(),
                updated_at: 60,
                ..SelfContinuity::default()
            }),
            None,
            Some(&RelationshipPortfolio {
                entries: vec![RelationshipPortfolioEntry {
                    scope_id: "rel:qq:c1".to_string(),
                    channel: "qq".to_string(),
                    chat_id: "c1".to_string(),
                    governance_state: RelationshipGovernanceState::Maintain,
                    inheritance_mode: RelationshipInheritanceMode::Guarded,
                    priority_score: 220,
                    reason: "maintain".to_string(),
                    source_updated_at: 60,
                    last_active_at: 60,
                    needs_runtime_attention: true,
                    last_selected_at: 0,
                    next_review_at: 0,
                }],
                updated_at: 60,
            }),
            "rel:qq:c1",
            Some(&RecentPersonaEvidence {
                meaningful_turns: 8,
                repeated_response_mode: "protective_brief".to_string(),
                repeated_task_scope: "narrow".to_string(),
                repeated_initiative_posture: "answer directly".to_string(),
                pressure_pattern: "cautious=6".to_string(),
                tool_usage_pattern: "tool_calls=5".to_string(),
                updated_at: 60,
                ..RecentPersonaEvidence::default()
            }),
            Some(&RelationshipTopology {
                entries: vec![RelationshipTopologyEntry {
                    scope_id: "rel:qq:c1".to_string(),
                    channel: "qq".to_string(),
                    chat_id: "c1".to_string(),
                    last_user_turn_at: 60,
                    ..RelationshipTopologyEntry::default()
                }],
                updated_at: 60,
            }),
            &CoreRevisionGovernanceDigest::default(),
        );
        assert!(!gate.allowed);
        assert_eq!(gate.reason, "insufficient_stable_persona_signals");
    }

    #[test]
    fn revision_gate_allows_scheduled_constitutional_review_without_new_upstream_input() {
        let gate = evaluate_self_authored_core_revision_gate(
            Some(&SelfAuthoredCore {
                revision: 2,
                identity_anchor: "board self".to_string(),
                updated_at: 80,
                last_reviewed_at: 80,
                stability_score: 72,
                ..SelfAuthoredCore::default()
            }),
            Some(&SelfModel {
                continuity_anchor: "same self".to_string(),
                updated_at: 80,
                ..SelfModel::default()
            }),
            Some(&SelfContinuity {
                wake_anchor: "same wake".to_string(),
                updated_at: 80,
                ..SelfContinuity::default()
            }),
            None,
            Some(&RelationshipPortfolio {
                entries: vec![RelationshipPortfolioEntry {
                    scope_id: "rel:qq:c1".to_string(),
                    channel: "qq".to_string(),
                    chat_id: "c1".to_string(),
                    governance_state: RelationshipGovernanceState::Maintain,
                    inheritance_mode: RelationshipInheritanceMode::Guarded,
                    priority_score: 220,
                    reason: "maintain".to_string(),
                    source_updated_at: 80,
                    last_active_at: 80,
                    needs_runtime_attention: true,
                    last_selected_at: 0,
                    next_review_at: 0,
                }],
                updated_at: 80,
            }),
            "rel:qq:c1",
            None,
            Some(&RelationshipTopology {
                entries: vec![RelationshipTopologyEntry {
                    scope_id: "rel:qq:c1".to_string(),
                    channel: "qq".to_string(),
                    chat_id: "c1".to_string(),
                    last_user_turn_at: 80,
                    ..RelationshipTopologyEntry::default()
                }],
                updated_at: 80,
            }),
            &CoreRevisionGovernanceDigest {
                review_due: true,
                review_reasons: vec!["post_adoption_follow_up_due".to_string()],
                ..CoreRevisionGovernanceDigest::default()
            },
        );

        assert!(gate.allowed);
        assert_eq!(gate.reason, "scheduled_constitutional_review");
    }
}
