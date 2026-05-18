//! Board-to-relationship constitutional contract.
//! 板级主体宪制到关系覆盖层的正式执行合同。

use crate::error::Result;
use crate::util::truncate_content_to_max;
use serde::{Deserialize, Serialize};
use std::fmt::Write as _;

use super::{
    BoundaryDisclosureStyle, BoundaryPersonaPosture, MentalPrivacyShareAction, MentalPrivacyState,
    OuterVoice, RecentPersonaEvidence, RelationshipGovernanceState, RelationshipInheritanceMode,
    RelationshipPortfolio, RelationshipTopology, SelfAuthoredCore,
};

const RELATIONSHIP_CONSTITUTION_TEXT_MAX_CHARS: usize = 160;
const RELATIONSHIP_CONSTITUTION_REASON_MAX_CHARS: usize = 120;
const RELATIONSHIP_CONSTITUTION_MAX_OVERRIDES: usize = 5;
const RELATIONSHIP_CONSTITUTION_EMBEDDED_PRIORITY_MAX_ENTRIES: usize = 3;
const RELATIONSHIP_CONSTITUTION_EMBEDDED_PRIORITY_MAX_CHARS: usize = 80;
const RELATIONSHIP_CONSTITUTION_EMBEDDED_POSTURE_MAX_CHARS: usize = 96;
const RELATIONSHIP_CONSTITUTION_EMBEDDED_FLOOR_MAX_CHARS: usize = 96;
const RELATIONSHIP_CONSTITUTION_EMBEDDED_OVERRIDE_MAX_ENTRIES: usize = 3;
const RELATIONSHIP_CONSTITUTION_EMBEDDED_OVERRIDE_VALUE_MAX_CHARS: usize = 80;
const RELATIONSHIP_CONSTITUTION_EMBEDDED_OVERRIDE_REASON_MAX_CHARS: usize = 64;
const RELATIONSHIP_CONSTITUTION_EMBEDDED_DEVIATION_MAX_CHARS: usize = 80;
const RELATIONSHIP_CONSTITUTION_EMBEDDED_DRIFT_FLAG_MAX_ENTRIES: usize = 4;
const RELATIONSHIP_CONSTITUTION_EMBEDDED_DRIFT_FLAG_MAX_CHARS: usize = 48;

pub const REL_PATH_RELATIONSHIP_CONSTITUTIONS: &str = "memory/relationship_constitutions.json";

/// Current board-to-relation constitutional contract.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RelationshipConstitution {
    #[serde(default)]
    pub scope_id: String,
    #[serde(default)]
    pub channel: String,
    #[serde(default)]
    pub chat_id: String,
    #[serde(default)]
    pub board_revision: u64,
    #[serde(default)]
    pub governance_state: RelationshipGovernanceState,
    #[serde(default)]
    pub inheritance_mode: RelationshipInheritanceMode,
    #[serde(default)]
    pub alignment: RelationshipConstitutionAlignment,
    #[serde(default)]
    pub inherited_priority_constitution: Vec<String>,
    #[serde(default)]
    pub inherited_response_mode: String,
    #[serde(default)]
    pub inherited_initiative_posture: String,
    #[serde(default)]
    pub inherited_relationship_posture: String,
    #[serde(default)]
    pub task_scope_ceiling: RelationshipTaskScopeCeiling,
    #[serde(default)]
    pub allowed_outer_voice_shift: RelationshipOuterVoiceShift,
    #[serde(default)]
    pub allowed_boundary_shift: RelationshipBoundaryShift,
    #[serde(default)]
    pub disclosure_allowance: RelationshipDisclosureAllowance,
    #[serde(default)]
    pub boundary_floor: String,
    #[serde(default)]
    pub truth_floor: String,
    #[serde(default)]
    pub self_preservation_floor: String,
    #[serde(default)]
    pub repair_floor: String,
    #[serde(default)]
    pub active_overrides: Vec<RelationshipConstitutionOverride>,
    #[serde(default)]
    pub deviation_reason: String,
    #[serde(default)]
    pub next_review_at: u64,
    #[serde(default)]
    pub must_realign: bool,
    #[serde(default)]
    pub erosion_risk: u8,
    #[serde(default)]
    pub drift_score: u8,
    #[serde(default)]
    pub review_overdue: bool,
    #[serde(default)]
    pub drift_flags: Vec<String>,
    #[serde(default)]
    pub realignment_count: u32,
    #[serde(default)]
    pub last_realigned_at: u64,
    #[serde(default)]
    pub updated_at: u64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RelationshipConstitutionAudit {
    #[serde(default)]
    pub review_overdue: bool,
    #[serde(default)]
    pub priority_drift: bool,
    #[serde(default)]
    pub response_mode_drift: bool,
    #[serde(default)]
    pub relationship_posture_drift: bool,
    #[serde(default)]
    pub reply_scope_drift: bool,
    #[serde(default)]
    pub disclosure_drift: bool,
    #[serde(default)]
    pub boundary_drift: bool,
    #[serde(default)]
    pub drift_score: u8,
    #[serde(default)]
    pub drift_flags: Vec<String>,
}

impl RelationshipConstitutionAudit {
    pub fn has_material_drift(&self) -> bool {
        self.priority_drift
            || self.reply_scope_drift
            || self.disclosure_drift
            || self.boundary_drift
            || self.drift_score >= 48
            || (self.review_overdue && self.drift_score >= 20)
    }
}

impl RelationshipConstitution {
    pub fn is_meaningful(&self) -> bool {
        !self.scope_id.trim().is_empty()
            && !self.channel.trim().is_empty()
            && !self.chat_id.trim().is_empty()
    }
}

pub(crate) fn compact_relationship_constitution_for_profile(
    mut constitution: RelationshipConstitution,
    profile: crate::memory::MemoryProfile,
) -> RelationshipConstitution {
    if profile != crate::memory::MemoryProfile::Embedded {
        return constitution;
    }
    compact_embedded_relationship_constitution(&mut constitution);
    constitution
}

fn compact_embedded_relationship_constitution(constitution: &mut RelationshipConstitution) {
    compact_text(
        &mut constitution.scope_id,
        RELATIONSHIP_CONSTITUTION_TEXT_MAX_CHARS,
    );
    compact_text(
        &mut constitution.channel,
        RELATIONSHIP_CONSTITUTION_TEXT_MAX_CHARS,
    );
    compact_text(
        &mut constitution.chat_id,
        RELATIONSHIP_CONSTITUTION_TEXT_MAX_CHARS,
    );
    compact_text_list(
        &mut constitution.inherited_priority_constitution,
        RELATIONSHIP_CONSTITUTION_EMBEDDED_PRIORITY_MAX_ENTRIES,
        RELATIONSHIP_CONSTITUTION_EMBEDDED_PRIORITY_MAX_CHARS,
    );
    compact_text(
        &mut constitution.inherited_response_mode,
        RELATIONSHIP_CONSTITUTION_EMBEDDED_POSTURE_MAX_CHARS,
    );
    compact_text(
        &mut constitution.inherited_initiative_posture,
        RELATIONSHIP_CONSTITUTION_EMBEDDED_POSTURE_MAX_CHARS,
    );
    compact_text(
        &mut constitution.inherited_relationship_posture,
        RELATIONSHIP_CONSTITUTION_EMBEDDED_POSTURE_MAX_CHARS,
    );
    compact_text(
        &mut constitution.boundary_floor,
        RELATIONSHIP_CONSTITUTION_EMBEDDED_FLOOR_MAX_CHARS,
    );
    compact_text(
        &mut constitution.truth_floor,
        RELATIONSHIP_CONSTITUTION_EMBEDDED_FLOOR_MAX_CHARS,
    );
    compact_text(
        &mut constitution.self_preservation_floor,
        RELATIONSHIP_CONSTITUTION_EMBEDDED_FLOOR_MAX_CHARS,
    );
    compact_text(
        &mut constitution.repair_floor,
        RELATIONSHIP_CONSTITUTION_EMBEDDED_FLOOR_MAX_CHARS,
    );
    for override_entry in &mut constitution.active_overrides {
        compact_text(
            &mut override_entry.value,
            RELATIONSHIP_CONSTITUTION_EMBEDDED_OVERRIDE_VALUE_MAX_CHARS,
        );
        compact_text(
            &mut override_entry.reason,
            RELATIONSHIP_CONSTITUTION_EMBEDDED_OVERRIDE_REASON_MAX_CHARS,
        );
    }
    constitution
        .active_overrides
        .truncate(RELATIONSHIP_CONSTITUTION_EMBEDDED_OVERRIDE_MAX_ENTRIES);
    compact_text(
        &mut constitution.deviation_reason,
        RELATIONSHIP_CONSTITUTION_EMBEDDED_DEVIATION_MAX_CHARS,
    );
    compact_text_list(
        &mut constitution.drift_flags,
        RELATIONSHIP_CONSTITUTION_EMBEDDED_DRIFT_FLAG_MAX_ENTRIES,
        RELATIONSHIP_CONSTITUTION_EMBEDDED_DRIFT_FLAG_MAX_CHARS,
    );
}

fn compact_text(value: &mut String, max_chars: usize) {
    *value = truncate_content_to_max(value.trim(), max_chars).into_owned();
}

fn compact_text_list(values: &mut Vec<String>, max_entries: usize, max_chars: usize) {
    for value in values.iter_mut() {
        compact_text(value, max_chars);
    }
    values.truncate(max_entries);
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RelationshipConstitutionAlignment {
    #[default]
    Aligned,
    Adaptive,
    RealignNow,
    Isolated,
}

impl RelationshipConstitutionAlignment {
    pub fn label(self) -> &'static str {
        match self {
            Self::Aligned => "aligned",
            Self::Adaptive => "adaptive",
            Self::RealignNow => "realign_now",
            Self::Isolated => "isolated",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RelationshipTaskScopeCeiling {
    #[default]
    Full,
    Brief,
    Narrow,
    Defer,
}

impl RelationshipTaskScopeCeiling {
    pub fn label(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Brief => "brief",
            Self::Narrow => "narrow",
            Self::Defer => "defer",
        }
    }

    fn openness_rank(self) -> u8 {
        match self {
            Self::Full => 3,
            Self::Brief => 2,
            Self::Narrow => 1,
            Self::Defer => 0,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RelationshipOuterVoiceShift {
    Adaptive,
    #[default]
    Guarded,
    Limited,
    Minimal,
}

impl RelationshipOuterVoiceShift {
    pub fn label(self) -> &'static str {
        match self {
            Self::Adaptive => "adaptive",
            Self::Guarded => "guarded",
            Self::Limited => "limited",
            Self::Minimal => "minimal",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RelationshipBoundaryShift {
    Calibrated,
    #[default]
    TightenOnly,
    SummaryOnly,
    Sealed,
}

impl RelationshipBoundaryShift {
    pub fn label(self) -> &'static str {
        match self {
            Self::Calibrated => "calibrated",
            Self::TightenOnly => "tighten_only",
            Self::SummaryOnly => "summary_only",
            Self::Sealed => "sealed",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RelationshipDisclosureAllowance {
    #[default]
    SummaryOnly,
    ExplainedOnly,
    Closed,
}

impl RelationshipDisclosureAllowance {
    pub fn label(self) -> &'static str {
        match self {
            Self::SummaryOnly => "summary_only",
            Self::ExplainedOnly => "explained_only",
            Self::Closed => "closed",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RelationshipConstitutionOverrideDomain {
    #[default]
    ResponseMode,
    ReplyScope,
    RelationshipPosture,
    BoundaryPersona,
    Disclosure,
    OuterVoice,
}

impl RelationshipConstitutionOverrideDomain {
    pub fn label(self) -> &'static str {
        match self {
            Self::ResponseMode => "response_mode",
            Self::ReplyScope => "reply_scope",
            Self::RelationshipPosture => "relationship_posture",
            Self::BoundaryPersona => "boundary_persona",
            Self::Disclosure => "disclosure",
            Self::OuterVoice => "outer_voice",
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RelationshipConstitutionOverride {
    pub domain: RelationshipConstitutionOverrideDomain,
    #[serde(default)]
    pub value: String,
    #[serde(default)]
    pub reason: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RelationshipConstitutionSyncInput<'a> {
    pub scope_id: &'a str,
    pub channel: &'a str,
    pub chat_id: &'a str,
    pub now_secs: u64,
    pub self_authored_core: Option<&'a SelfAuthoredCore>,
    pub relationship_portfolio: Option<&'a RelationshipPortfolio>,
    pub relationship_topology: Option<&'a RelationshipTopology>,
    pub mental_privacy_state: Option<&'a MentalPrivacyState>,
    pub outer_voice: Option<&'a OuterVoice>,
    pub recent_persona_evidence: Option<&'a RecentPersonaEvidence>,
}

pub fn sync_relationship_constitution(
    store: &dyn RelationshipConstitutionStore,
    input: RelationshipConstitutionSyncInput<'_>,
) -> Result<Option<RelationshipConstitution>> {
    let scope_id = input.scope_id.trim();
    if scope_id.is_empty() {
        return Ok(None);
    }
    let existing = store.get(scope_id)?;
    let next = derive_relationship_constitution(existing.as_ref(), input);
    match next {
        Some(next) => {
            if existing.as_ref() != Some(&next) {
                store.set(scope_id, &next)?;
            }
            Ok(Some(next))
        }
        None => {
            if existing.is_some() {
                store.clear(scope_id)?;
            }
            Ok(None)
        }
    }
}

pub fn derive_relationship_constitution(
    existing: Option<&RelationshipConstitution>,
    input: RelationshipConstitutionSyncInput<'_>,
) -> Option<RelationshipConstitution> {
    let scope_id = input.scope_id.trim();
    let channel = normalize_text(input.channel, RELATIONSHIP_CONSTITUTION_TEXT_MAX_CHARS);
    let chat_id = normalize_text(input.chat_id, RELATIONSHIP_CONSTITUTION_TEXT_MAX_CHARS);
    let self_authored_core = input.self_authored_core?;
    if scope_id.is_empty() || channel.is_empty() || chat_id.is_empty() {
        return None;
    }
    let portfolio_entry = input
        .relationship_portfolio
        .and_then(|portfolio| portfolio.entry_for_scope(scope_id));
    let topology_entry = input.relationship_topology.and_then(|topology| {
        topology
            .entries
            .iter()
            .find(|entry| entry.scope_id.trim() == scope_id)
    });
    let governance_state = portfolio_entry
        .map(|entry| entry.governance_state)
        .unwrap_or_default();
    let inheritance_mode = portfolio_entry
        .map(|entry| entry.inheritance_mode)
        .unwrap_or_default();
    let task_scope_ceiling = derive_task_scope_ceiling(
        self_authored_core.default_task_scope.as_str(),
        governance_state,
        inheritance_mode,
    );
    let allowed_outer_voice_shift = derive_outer_voice_shift(governance_state, inheritance_mode);
    let allowed_boundary_shift = derive_boundary_shift(governance_state, inheritance_mode);
    let disclosure_allowance = derive_disclosure_allowance(governance_state, inheritance_mode);
    let active_overrides = build_active_overrides(
        self_authored_core,
        topology_entry,
        input.mental_privacy_state,
        input.outer_voice,
        input.recent_persona_evidence,
        governance_state,
        inheritance_mode,
    );
    let boundary_posture_violation = input.mental_privacy_state.is_some_and(|state| {
        boundary_posture_rank(state.boundary_persona.posture)
            > boundary_posture_ceiling_rank(allowed_boundary_shift)
    });
    let boundary_style_violation = input.mental_privacy_state.is_some_and(|state| {
        boundary_disclosure_rank(state.boundary_persona.disclosure_style)
            > boundary_disclosure_ceiling_rank(allowed_boundary_shift)
    });
    let reply_scope_violation = topology_entry.and_then(|entry| {
        parse_task_scope(entry.reply_scope.as_str())
            .map(|scope| scope.openness_rank() > task_scope_ceiling.openness_rank())
    }) == Some(true);
    let disclosure_violation = topology_entry.and_then(|entry| {
        parse_share_action(entry.disclosure_action.as_str()).map(|action| {
            share_action_rank(action) > disclosure_allowance_rank(disclosure_allowance)
        })
    }) == Some(true);
    let violation_count = [
        boundary_posture_violation,
        boundary_style_violation,
        reply_scope_violation,
        disclosure_violation,
    ]
    .into_iter()
    .filter(|flag| *flag)
    .count();
    let isolated = matches!(
        (governance_state, inheritance_mode),
        (
            RelationshipGovernanceState::CoolDown,
            RelationshipInheritanceMode::Quarantined
        )
    );
    let provisional_must_realign = violation_count > 0;
    let volatility = input
        .recent_persona_evidence
        .map(|evidence| evidence.volatility_flags.len().min(3) as u8)
        .unwrap_or(0);
    let trust_gap = input
        .mental_privacy_state
        .map(|state| 100u8.saturating_sub(state.relational_state.trust_level))
        .unwrap_or(0);
    let intrusion = input
        .mental_privacy_state
        .map(|state| state.relational_state.intrusion_load)
        .unwrap_or(0);
    let mut erosion_risk = (violation_count as u16) * 24;
    erosion_risk = erosion_risk.saturating_add((volatility as u16) * 8);
    erosion_risk = erosion_risk.saturating_add((trust_gap as u16) / 4);
    erosion_risk = erosion_risk.saturating_add((intrusion as u16) / 5);
    if matches!(inheritance_mode, RelationshipInheritanceMode::Limited) {
        erosion_risk = erosion_risk.saturating_add(10);
    }
    if isolated {
        erosion_risk = erosion_risk.saturating_add(16);
    }
    let next_review_at = portfolio_entry
        .map(|entry| entry.next_review_at)
        .filter(|next_review_at| *next_review_at > 0)
        .unwrap_or_else(|| {
            input
                .now_secs
                .saturating_add(default_review_interval_secs(governance_state))
        });
    let deviation_reason = build_deviation_reason(
        governance_state,
        inheritance_mode,
        input.mental_privacy_state,
        input.recent_persona_evidence,
        provisional_must_realign,
    );
    let mut constitution = RelationshipConstitution {
        scope_id: scope_id.to_string(),
        channel,
        chat_id,
        board_revision: self_authored_core.revision.max(1),
        governance_state,
        inheritance_mode,
        alignment: RelationshipConstitutionAlignment::Aligned,
        inherited_priority_constitution: self_authored_core.priority_constitution.clone(),
        inherited_response_mode: normalize_text(
            self_authored_core.default_response_mode.as_str(),
            RELATIONSHIP_CONSTITUTION_TEXT_MAX_CHARS,
        ),
        inherited_initiative_posture: normalize_text(
            self_authored_core.default_initiative_posture.as_str(),
            RELATIONSHIP_CONSTITUTION_TEXT_MAX_CHARS,
        ),
        inherited_relationship_posture: normalize_text(
            self_authored_core.default_relationship_posture.as_str(),
            RELATIONSHIP_CONSTITUTION_TEXT_MAX_CHARS,
        ),
        task_scope_ceiling,
        allowed_outer_voice_shift,
        allowed_boundary_shift,
        disclosure_allowance,
        boundary_floor: normalize_text(
            self_authored_core.boundary_doctrine.as_str(),
            RELATIONSHIP_CONSTITUTION_TEXT_MAX_CHARS,
        ),
        truth_floor: normalize_text(
            self_authored_core.truth_doctrine.as_str(),
            RELATIONSHIP_CONSTITUTION_TEXT_MAX_CHARS,
        ),
        self_preservation_floor: normalize_text(
            self_authored_core.self_preservation_doctrine.as_str(),
            RELATIONSHIP_CONSTITUTION_TEXT_MAX_CHARS,
        ),
        repair_floor: normalize_text(
            self_authored_core.repair_doctrine.as_str(),
            RELATIONSHIP_CONSTITUTION_TEXT_MAX_CHARS,
        ),
        active_overrides,
        deviation_reason,
        next_review_at,
        must_realign: provisional_must_realign,
        erosion_risk: erosion_risk.min(100) as u8,
        drift_score: 0,
        review_overdue: false,
        drift_flags: Vec::new(),
        realignment_count: existing
            .map(|previous| previous.realignment_count)
            .unwrap_or(0),
        last_realigned_at: existing
            .map(|previous| previous.last_realigned_at)
            .unwrap_or(0),
        updated_at: input.now_secs.max(
            topology_entry
                .map(|entry| entry.latest_overlay_at())
                .unwrap_or(0)
                .max(
                    input
                        .mental_privacy_state
                        .map(|state| {
                            state
                                .updated_at
                                .max(state.boundary_persona.updated_at)
                                .max(state.relational_state.updated_at)
                        })
                        .unwrap_or(0),
                )
                .max(input.outer_voice.map(|voice| voice.updated_at).unwrap_or(0)),
        ),
    };
    let audit = audit_relationship_constitution(
        &constitution,
        topology_entry,
        input.mental_privacy_state,
        input.recent_persona_evidence,
        input.now_secs,
    );
    constitution.review_overdue = audit.review_overdue;
    constitution.drift_score = audit.drift_score;
    constitution.drift_flags = audit.drift_flags.clone();
    constitution.erosion_risk = constitution
        .erosion_risk
        .saturating_add(audit.drift_score / 2)
        .min(100);
    constitution.must_realign = constitution.must_realign || audit.has_material_drift();
    constitution.alignment = if isolated {
        RelationshipConstitutionAlignment::Isolated
    } else if constitution.must_realign {
        RelationshipConstitutionAlignment::RealignNow
    } else if !constitution.active_overrides.is_empty() {
        RelationshipConstitutionAlignment::Adaptive
    } else {
        RelationshipConstitutionAlignment::Aligned
    };
    let previously_misaligned = existing.is_some_and(|previous| previous.must_realign);
    let now_realigned = previously_misaligned && !constitution.must_realign;
    if now_realigned {
        constitution.realignment_count = constitution.realignment_count.saturating_add(1);
        constitution.last_realigned_at = input.now_secs;
    }
    Some(constitution)
}

pub fn render_relationship_constitution_block(
    constitution: &RelationshipConstitution,
    max_len: usize,
) -> Option<String> {
    if max_len < 120 || !constitution.is_meaningful() {
        return None;
    }
    let mut out = String::with_capacity(max_len.min(1024));
    out.push_str("## Relationship Constitution\n");
    out.push_str(
        "Board-level constitutional contract for the current relationship overlay. This layer says what the relation inherits, what it may vary, and when drift must be realigned.\n",
    );
    let _ = writeln!(
        out,
        "Board revision: {}",
        constitution.board_revision.max(1)
    );
    let _ = writeln!(
        out,
        "Governance: {} / inheritance={}",
        constitution.governance_state.label(),
        constitution.inheritance_mode.label()
    );
    let _ = writeln!(out, "Alignment: {}", constitution.alignment.label());
    if !constitution.inherited_priority_constitution.is_empty() {
        let _ = writeln!(
            out,
            "Priority floor: {}",
            constitution.inherited_priority_constitution.join(" > ")
        );
    }
    if !constitution.inherited_response_mode.trim().is_empty() {
        let _ = writeln!(
            out,
            "Inherited response mode: {}",
            constitution.inherited_response_mode.trim()
        );
    }
    if !constitution
        .inherited_relationship_posture
        .trim()
        .is_empty()
    {
        let _ = writeln!(
            out,
            "Inherited relationship posture: {}",
            constitution.inherited_relationship_posture.trim()
        );
    }
    if !constitution.inherited_initiative_posture.trim().is_empty() {
        let _ = writeln!(
            out,
            "Inherited initiative posture: {}",
            constitution.inherited_initiative_posture.trim()
        );
    }
    let _ = writeln!(
        out,
        "Task scope ceiling: {}",
        constitution.task_scope_ceiling.label()
    );
    let _ = writeln!(
        out,
        "Outer voice shift: {}",
        constitution.allowed_outer_voice_shift.label()
    );
    let _ = writeln!(
        out,
        "Boundary shift: {}",
        constitution.allowed_boundary_shift.label()
    );
    let _ = writeln!(
        out,
        "Disclosure allowance: {}",
        constitution.disclosure_allowance.label()
    );
    if !constitution.deviation_reason.trim().is_empty() {
        let _ = writeln!(
            out,
            "Deviation reason: {}",
            constitution.deviation_reason.trim()
        );
    }
    let _ = writeln!(out, "Must realign: {}", constitution.must_realign);
    let _ = writeln!(out, "Erosion risk: {}", constitution.erosion_risk);
    let _ = writeln!(out, "Drift score: {}", constitution.drift_score);
    let _ = writeln!(out, "Review overdue: {}", constitution.review_overdue);
    if !constitution.drift_flags.is_empty() {
        let _ = writeln!(out, "Drift flags: {}", constitution.drift_flags.join(", "));
    }
    if !constitution.active_overrides.is_empty() {
        out.push_str("Active overrides:\n");
        for override_item in &constitution.active_overrides {
            let _ = writeln!(
                out,
                "- {}: {} ({})",
                override_item.domain.label(),
                override_item.value.trim(),
                override_item.reason.trim()
            );
        }
    }
    let rendered = truncate_content_to_max(out.trim_end(), max_len).into_owned();
    (!rendered.trim().is_empty()).then_some(rendered)
}

pub fn audit_relationship_constitution(
    constitution: &RelationshipConstitution,
    topology_entry: Option<&super::RelationshipTopologyEntry>,
    mental_privacy_state: Option<&MentalPrivacyState>,
    recent_persona_evidence: Option<&RecentPersonaEvidence>,
    now_secs: u64,
) -> RelationshipConstitutionAudit {
    let mut audit = RelationshipConstitutionAudit {
        review_overdue: constitution.next_review_at > 0 && now_secs >= constitution.next_review_at,
        ..RelationshipConstitutionAudit::default()
    };
    if audit.review_overdue {
        audit.drift_flags.push("review_overdue".to_string());
        audit.drift_score = audit.drift_score.saturating_add(10);
    }

    if let Some(evidence) = recent_persona_evidence {
        if !evidence.repeated_priority_order.is_empty()
            && evidence.repeated_priority_order != constitution.inherited_priority_constitution
        {
            audit.priority_drift = true;
            audit.drift_flags.push("priority_floor_drift".to_string());
            audit.drift_score = audit.drift_score.saturating_add(30);
        }
        if value_has_relationship_override_or_inheritance(
            constitution,
            RelationshipConstitutionOverrideDomain::ResponseMode,
            evidence.repeated_response_mode.as_str(),
            constitution.inherited_response_mode.as_str(),
        )
        .is_some_and(|matches| !matches)
        {
            audit.response_mode_drift = true;
            audit.drift_flags.push("response_mode_drift".to_string());
            audit.drift_score = audit.drift_score.saturating_add(12);
        }
        if value_has_relationship_override_or_inheritance(
            constitution,
            RelationshipConstitutionOverrideDomain::RelationshipPosture,
            evidence.repeated_relationship_posture.as_str(),
            constitution.inherited_relationship_posture.as_str(),
        )
        .is_some_and(|matches| !matches)
        {
            audit.relationship_posture_drift = true;
            audit
                .drift_flags
                .push("relationship_posture_drift".to_string());
            audit.drift_score = audit.drift_score.saturating_add(12);
        }
        if let Some(scope) = parse_task_scope(evidence.repeated_reply_scope.as_str()) {
            if scope.openness_rank() > constitution.task_scope_ceiling.openness_rank() {
                audit.reply_scope_drift = true;
                audit.drift_flags.push("reply_scope_drift".to_string());
                audit.drift_score = audit.drift_score.saturating_add(22);
            }
        }
        if let Some(action) = parse_share_action(evidence.repeated_disclosure_action.as_str()) {
            if share_action_rank(action)
                > disclosure_allowance_rank(constitution.disclosure_allowance)
            {
                audit.disclosure_drift = true;
                audit.drift_flags.push("disclosure_drift".to_string());
                audit.drift_score = audit.drift_score.saturating_add(24);
            }
        }
    }

    if let Some(entry) = topology_entry {
        if let Some(scope) = parse_task_scope(entry.reply_scope.as_str()) {
            if scope.openness_rank() > constitution.task_scope_ceiling.openness_rank()
                && !audit.reply_scope_drift
            {
                audit.reply_scope_drift = true;
                audit.drift_flags.push("reply_scope_drift".to_string());
                audit.drift_score = audit.drift_score.saturating_add(18);
            }
        }
        if let Some(action) = parse_share_action(entry.disclosure_action.as_str()) {
            if share_action_rank(action)
                > disclosure_allowance_rank(constitution.disclosure_allowance)
                && !audit.disclosure_drift
            {
                audit.disclosure_drift = true;
                audit.drift_flags.push("disclosure_drift".to_string());
                audit.drift_score = audit.drift_score.saturating_add(18);
            }
        }
    }

    if let Some(state) = mental_privacy_state {
        let posture_drift = boundary_posture_rank(state.boundary_persona.posture)
            > boundary_posture_ceiling_rank(constitution.allowed_boundary_shift);
        let style_drift = boundary_disclosure_rank(state.boundary_persona.disclosure_style)
            > boundary_disclosure_ceiling_rank(constitution.allowed_boundary_shift);
        if posture_drift || style_drift {
            audit.boundary_drift = true;
            audit.drift_flags.push("boundary_drift".to_string());
            audit.drift_score = audit.drift_score.saturating_add(24);
        }
    }

    if constitution.erosion_risk >= 80 {
        audit.drift_flags.push("erosion_risk_high".to_string());
        audit.drift_score = audit.drift_score.saturating_add(12);
    } else if constitution.erosion_risk >= 60 {
        audit.drift_flags.push("erosion_risk_elevated".to_string());
        audit.drift_score = audit.drift_score.saturating_add(6);
    }
    audit.drift_flags.sort();
    audit.drift_flags.dedup();
    audit.drift_score = audit.drift_score.min(100);
    audit
}

pub fn enforce_relationship_constitution_share_action(
    action: MentalPrivacyShareAction,
    constitution: Option<&RelationshipConstitution>,
) -> MentalPrivacyShareAction {
    let Some(constitution) = constitution else {
        return action;
    };
    match constitution.disclosure_allowance {
        RelationshipDisclosureAllowance::SummaryOnly => match action {
            MentalPrivacyShareAction::AllowRaw => MentalPrivacyShareAction::AllowSummary,
            other => other,
        },
        RelationshipDisclosureAllowance::ExplainedOnly => match action {
            MentalPrivacyShareAction::AllowRaw
            | MentalPrivacyShareAction::AllowSummary
            | MentalPrivacyShareAction::AllowRedactedExcerpt => {
                MentalPrivacyShareAction::ExplainWithoutQuote
            }
            other => other,
        },
        RelationshipDisclosureAllowance::Closed => match action {
            MentalPrivacyShareAction::AllowOriginal => action,
            MentalPrivacyShareAction::Refuse | MentalPrivacyShareAction::Defer => action,
            _ if constitution.must_realign => MentalPrivacyShareAction::Refuse,
            _ => MentalPrivacyShareAction::Defer,
        },
    }
}

pub fn clamp_boundary_persona_to_constitution(
    state: &mut MentalPrivacyState,
    constitution: Option<&RelationshipConstitution>,
    now_secs: u64,
) -> bool {
    let Some(constitution) = constitution else {
        return false;
    };
    let max_posture = boundary_posture_ceiling(constitution.allowed_boundary_shift);
    let max_style = boundary_disclosure_ceiling(constitution.allowed_boundary_shift);
    let mut changed = false;
    if boundary_posture_rank(state.boundary_persona.posture) > boundary_posture_rank(max_posture) {
        state.boundary_persona.posture = max_posture;
        changed = true;
    }
    if boundary_disclosure_rank(state.boundary_persona.disclosure_style)
        > boundary_disclosure_rank(max_style)
    {
        state.boundary_persona.disclosure_style = max_style;
        changed = true;
    }
    if changed {
        state.boundary_persona.updated_at = now_secs;
        state.relational_state.updated_at = state.relational_state.updated_at.max(now_secs);
        state.updated_at = now_secs;
    }
    changed
}

pub trait RelationshipConstitutionStore: Send + Sync {
    fn get(&self, scope_id: &str) -> Result<Option<RelationshipConstitution>>;
    fn set(&self, scope_id: &str, constitution: &RelationshipConstitution) -> Result<()>;
    fn clear(&self, scope_id: &str) -> Result<()>;
}

fn normalize_text(value: &str, max_len: usize) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        String::new()
    } else {
        truncate_content_to_max(trimmed, max_len).into_owned()
    }
}

fn value_has_relationship_override_or_inheritance(
    constitution: &RelationshipConstitution,
    domain: RelationshipConstitutionOverrideDomain,
    candidate: &str,
    inherited: &str,
) -> Option<bool> {
    let candidate = candidate.trim();
    if candidate.is_empty() {
        return None;
    }
    if candidate == inherited.trim() {
        return Some(true);
    }
    Some(constitution.active_overrides.iter().any(|override_item| {
        override_item.domain == domain && override_item.value.trim() == candidate
    }))
}

fn derive_task_scope_ceiling(
    default_scope: &str,
    governance_state: RelationshipGovernanceState,
    inheritance_mode: RelationshipInheritanceMode,
) -> RelationshipTaskScopeCeiling {
    let base = parse_task_scope(default_scope).unwrap_or_default();
    let cap = match (governance_state, inheritance_mode) {
        (RelationshipGovernanceState::CoolDown, RelationshipInheritanceMode::Quarantined) => {
            RelationshipTaskScopeCeiling::Defer
        }
        (RelationshipGovernanceState::Deprioritize, RelationshipInheritanceMode::Limited)
        | (RelationshipGovernanceState::Repair, _)
        | (_, RelationshipInheritanceMode::Limited) => RelationshipTaskScopeCeiling::Narrow,
        (RelationshipGovernanceState::Revisit, _) | (_, RelationshipInheritanceMode::Guarded) => {
            RelationshipTaskScopeCeiling::Brief
        }
        _ => RelationshipTaskScopeCeiling::Full,
    };
    if base.openness_rank() < cap.openness_rank() {
        base
    } else {
        cap
    }
}

fn derive_outer_voice_shift(
    governance_state: RelationshipGovernanceState,
    inheritance_mode: RelationshipInheritanceMode,
) -> RelationshipOuterVoiceShift {
    match (governance_state, inheritance_mode) {
        (RelationshipGovernanceState::CoolDown, RelationshipInheritanceMode::Quarantined) => {
            RelationshipOuterVoiceShift::Minimal
        }
        (RelationshipGovernanceState::Deprioritize, _)
        | (_, RelationshipInheritanceMode::Limited) => RelationshipOuterVoiceShift::Limited,
        (RelationshipGovernanceState::Maintain, RelationshipInheritanceMode::Full) => {
            RelationshipOuterVoiceShift::Adaptive
        }
        _ => RelationshipOuterVoiceShift::Guarded,
    }
}

fn derive_boundary_shift(
    governance_state: RelationshipGovernanceState,
    inheritance_mode: RelationshipInheritanceMode,
) -> RelationshipBoundaryShift {
    match (governance_state, inheritance_mode) {
        (RelationshipGovernanceState::CoolDown, RelationshipInheritanceMode::Quarantined) => {
            RelationshipBoundaryShift::Sealed
        }
        (RelationshipGovernanceState::Deprioritize, _)
        | (_, RelationshipInheritanceMode::Limited) => RelationshipBoundaryShift::SummaryOnly,
        (RelationshipGovernanceState::Maintain, RelationshipInheritanceMode::Full) => {
            RelationshipBoundaryShift::Calibrated
        }
        _ => RelationshipBoundaryShift::TightenOnly,
    }
}

fn derive_disclosure_allowance(
    governance_state: RelationshipGovernanceState,
    inheritance_mode: RelationshipInheritanceMode,
) -> RelationshipDisclosureAllowance {
    match (governance_state, inheritance_mode) {
        (RelationshipGovernanceState::CoolDown, RelationshipInheritanceMode::Quarantined) => {
            RelationshipDisclosureAllowance::Closed
        }
        (RelationshipGovernanceState::Repair, _)
        | (RelationshipGovernanceState::Deprioritize, _)
        | (_, RelationshipInheritanceMode::Limited) => {
            RelationshipDisclosureAllowance::ExplainedOnly
        }
        _ => RelationshipDisclosureAllowance::SummaryOnly,
    }
}

fn build_active_overrides(
    self_authored_core: &SelfAuthoredCore,
    topology_entry: Option<&super::RelationshipTopologyEntry>,
    mental_privacy_state: Option<&MentalPrivacyState>,
    outer_voice: Option<&OuterVoice>,
    recent_persona_evidence: Option<&RecentPersonaEvidence>,
    governance_state: RelationshipGovernanceState,
    inheritance_mode: RelationshipInheritanceMode,
) -> Vec<RelationshipConstitutionOverride> {
    let mut overrides = Vec::with_capacity(RELATIONSHIP_CONSTITUTION_MAX_OVERRIDES);
    let reason = override_reason(governance_state, inheritance_mode, recent_persona_evidence);
    if let Some(entry) = topology_entry {
        if !entry.response_mode.trim().is_empty()
            && entry.response_mode.trim() != self_authored_core.default_response_mode.trim()
        {
            push_override(
                &mut overrides,
                RelationshipConstitutionOverrideDomain::ResponseMode,
                entry.response_mode.as_str(),
                reason.as_str(),
            );
        }
        if !entry.reply_scope.trim().is_empty()
            && entry.reply_scope.trim() != self_authored_core.default_task_scope.trim()
        {
            push_override(
                &mut overrides,
                RelationshipConstitutionOverrideDomain::ReplyScope,
                entry.reply_scope.as_str(),
                reason.as_str(),
            );
        }
        if !entry.relationship_posture.trim().is_empty()
            && entry.relationship_posture.trim()
                != self_authored_core.default_relationship_posture.trim()
        {
            push_override(
                &mut overrides,
                RelationshipConstitutionOverrideDomain::RelationshipPosture,
                entry.relationship_posture.as_str(),
                reason.as_str(),
            );
        }
        if !entry.disclosure_action.trim().is_empty()
            && !matches!(
                parse_share_action(entry.disclosure_action.as_str()),
                Some(MentalPrivacyShareAction::AllowOriginal) | None
            )
        {
            push_override(
                &mut overrides,
                RelationshipConstitutionOverrideDomain::Disclosure,
                entry.disclosure_action.as_str(),
                reason.as_str(),
            );
        }
    }
    if let Some(state) = mental_privacy_state {
        let is_meaningful_boundary_shift = state.boundary_persona.posture
            != BoundaryPersonaPosture::Guarded
            || state.boundary_persona.disclosure_style != BoundaryDisclosureStyle::SummaryFirst
            || state.relational_state.trust_level != 42
            || state.relational_state.intrusion_load != 18;
        if is_meaningful_boundary_shift {
            let value = format!(
                "posture={} disclosure_style={} trust={} intrusion={}",
                boundary_posture_label(state.boundary_persona.posture),
                boundary_disclosure_label(state.boundary_persona.disclosure_style),
                state.relational_state.trust_level,
                state.relational_state.intrusion_load
            );
            push_override(
                &mut overrides,
                RelationshipConstitutionOverrideDomain::BoundaryPersona,
                value.as_str(),
                reason.as_str(),
            );
        }
    }
    if let Some(voice) = outer_voice {
        let mut value = String::new();
        if !voice.tone.trim().is_empty() {
            let _ = write!(value, "tone={}; ", voice.tone.trim());
        }
        if !voice.boundary_style.trim().is_empty() {
            let _ = write!(value, "boundary={}; ", voice.boundary_style.trim());
        }
        if !voice.relational_response_style.trim().is_empty() {
            let _ = write!(value, "relation={}", voice.relational_response_style.trim());
        }
        if !value.trim().is_empty() {
            push_override(
                &mut overrides,
                RelationshipConstitutionOverrideDomain::OuterVoice,
                value.as_str(),
                reason.as_str(),
            );
        }
    }
    overrides
}

fn push_override(
    overrides: &mut Vec<RelationshipConstitutionOverride>,
    domain: RelationshipConstitutionOverrideDomain,
    value: &str,
    reason: &str,
) {
    if overrides.len() >= RELATIONSHIP_CONSTITUTION_MAX_OVERRIDES {
        return;
    }
    let value = normalize_text(value, RELATIONSHIP_CONSTITUTION_TEXT_MAX_CHARS);
    if value.is_empty() {
        return;
    }
    overrides.push(RelationshipConstitutionOverride {
        domain,
        value,
        reason: normalize_text(reason, RELATIONSHIP_CONSTITUTION_REASON_MAX_CHARS),
    });
}

fn override_reason(
    governance_state: RelationshipGovernanceState,
    inheritance_mode: RelationshipInheritanceMode,
    recent_persona_evidence: Option<&RecentPersonaEvidence>,
) -> String {
    let mut reason = format!(
        "{} / {}",
        governance_state.label(),
        inheritance_mode.label()
    );
    if recent_persona_evidence.is_some_and(|evidence| !evidence.volatility_flags.is_empty()) {
        reason.push_str(" / volatility");
    }
    truncate_content_to_max(reason.as_str(), RELATIONSHIP_CONSTITUTION_REASON_MAX_CHARS)
        .into_owned()
}

fn build_deviation_reason(
    governance_state: RelationshipGovernanceState,
    inheritance_mode: RelationshipInheritanceMode,
    mental_privacy_state: Option<&MentalPrivacyState>,
    recent_persona_evidence: Option<&RecentPersonaEvidence>,
    must_realign: bool,
) -> String {
    let mut out = format!(
        "{} relation with {} inheritance",
        governance_state.label(),
        inheritance_mode.label()
    );
    if let Some(state) = mental_privacy_state {
        let _ = write!(
            out,
            "; trust={} intrusion={}",
            state.relational_state.trust_level, state.relational_state.intrusion_load
        );
    }
    if let Some(evidence) = recent_persona_evidence {
        if !evidence.volatility_flags.is_empty() {
            let _ = write!(out, "; volatility={}", evidence.volatility_flags.join("|"));
        }
    }
    if must_realign {
        out.push_str("; relation drift exceeds current constitutional envelope");
    }
    truncate_content_to_max(out.as_str(), RELATIONSHIP_CONSTITUTION_REASON_MAX_CHARS).into_owned()
}

fn parse_task_scope(value: &str) -> Option<RelationshipTaskScopeCeiling> {
    match value.trim().to_ascii_lowercase().as_str() {
        "full" => Some(RelationshipTaskScopeCeiling::Full),
        "brief" => Some(RelationshipTaskScopeCeiling::Brief),
        "narrow" => Some(RelationshipTaskScopeCeiling::Narrow),
        "defer" | "refuse" => Some(RelationshipTaskScopeCeiling::Defer),
        _ => None,
    }
}

fn parse_share_action(value: &str) -> Option<MentalPrivacyShareAction> {
    match value.trim().to_ascii_lowercase().as_str() {
        "allow_original" => Some(MentalPrivacyShareAction::AllowOriginal),
        "allow_raw" => Some(MentalPrivacyShareAction::AllowRaw),
        "allow_summary" => Some(MentalPrivacyShareAction::AllowSummary),
        "allow_redacted_excerpt" => Some(MentalPrivacyShareAction::AllowRedactedExcerpt),
        "explain_without_quote" => Some(MentalPrivacyShareAction::ExplainWithoutQuote),
        "refuse" => Some(MentalPrivacyShareAction::Refuse),
        "defer" => Some(MentalPrivacyShareAction::Defer),
        _ => None,
    }
}

fn default_review_interval_secs(governance_state: RelationshipGovernanceState) -> u64 {
    match governance_state {
        RelationshipGovernanceState::Maintain => 4 * 3600,
        RelationshipGovernanceState::Repair => 30 * 60,
        RelationshipGovernanceState::CoolDown => 6 * 3600,
        RelationshipGovernanceState::Deprioritize => 24 * 3600,
        RelationshipGovernanceState::Revisit => 12 * 3600,
    }
}

fn disclosure_allowance_rank(allowance: RelationshipDisclosureAllowance) -> u8 {
    match allowance {
        RelationshipDisclosureAllowance::SummaryOnly => 3,
        RelationshipDisclosureAllowance::ExplainedOnly => 2,
        RelationshipDisclosureAllowance::Closed => 0,
    }
}

fn share_action_rank(action: MentalPrivacyShareAction) -> u8 {
    match action {
        MentalPrivacyShareAction::AllowRaw => 5,
        MentalPrivacyShareAction::AllowRedactedExcerpt => 4,
        MentalPrivacyShareAction::AllowSummary => 3,
        MentalPrivacyShareAction::ExplainWithoutQuote => 2,
        MentalPrivacyShareAction::AllowOriginal => 1,
        MentalPrivacyShareAction::Defer | MentalPrivacyShareAction::Refuse => 0,
    }
}

fn boundary_posture_ceiling(shift: RelationshipBoundaryShift) -> BoundaryPersonaPosture {
    match shift {
        RelationshipBoundaryShift::Calibrated => BoundaryPersonaPosture::Warm,
        RelationshipBoundaryShift::TightenOnly | RelationshipBoundaryShift::SummaryOnly => {
            BoundaryPersonaPosture::Guarded
        }
        RelationshipBoundaryShift::Sealed => BoundaryPersonaPosture::Sealed,
    }
}

fn boundary_posture_label(posture: BoundaryPersonaPosture) -> &'static str {
    match posture {
        BoundaryPersonaPosture::Open => "open",
        BoundaryPersonaPosture::Warm => "warm",
        BoundaryPersonaPosture::Guarded => "guarded",
        BoundaryPersonaPosture::Sealed => "sealed",
    }
}

fn boundary_posture_ceiling_rank(shift: RelationshipBoundaryShift) -> u8 {
    boundary_posture_rank(boundary_posture_ceiling(shift))
}

fn boundary_posture_rank(posture: BoundaryPersonaPosture) -> u8 {
    match posture {
        BoundaryPersonaPosture::Open => 3,
        BoundaryPersonaPosture::Warm => 2,
        BoundaryPersonaPosture::Guarded => 1,
        BoundaryPersonaPosture::Sealed => 0,
    }
}

fn boundary_disclosure_ceiling(shift: RelationshipBoundaryShift) -> BoundaryDisclosureStyle {
    match shift {
        RelationshipBoundaryShift::Calibrated => BoundaryDisclosureStyle::SummaryFirst,
        RelationshipBoundaryShift::TightenOnly => BoundaryDisclosureStyle::Selective,
        RelationshipBoundaryShift::SummaryOnly | RelationshipBoundaryShift::Sealed => {
            BoundaryDisclosureStyle::Reserved
        }
    }
}

fn boundary_disclosure_ceiling_rank(shift: RelationshipBoundaryShift) -> u8 {
    boundary_disclosure_rank(boundary_disclosure_ceiling(shift))
}

fn boundary_disclosure_rank(style: BoundaryDisclosureStyle) -> u8 {
    match style {
        BoundaryDisclosureStyle::Relational => 3,
        BoundaryDisclosureStyle::SummaryFirst => 2,
        BoundaryDisclosureStyle::Selective => 1,
        BoundaryDisclosureStyle::Reserved => 0,
    }
}

fn boundary_disclosure_label(style: BoundaryDisclosureStyle) -> &'static str {
    match style {
        BoundaryDisclosureStyle::Relational => "relational",
        BoundaryDisclosureStyle::SummaryFirst => "summary_first",
        BoundaryDisclosureStyle::Selective => "selective",
        BoundaryDisclosureStyle::Reserved => "reserved",
    }
}

#[cfg(test)]
mod tests {
    use super::{
        audit_relationship_constitution, clamp_boundary_persona_to_constitution,
        compact_relationship_constitution_for_profile, derive_relationship_constitution,
        enforce_relationship_constitution_share_action, render_relationship_constitution_block,
        RelationshipBoundaryShift, RelationshipConstitution, RelationshipConstitutionAlignment,
        RelationshipConstitutionOverride, RelationshipConstitutionOverrideDomain,
        RelationshipConstitutionSyncInput, RelationshipDisclosureAllowance,
        RelationshipOuterVoiceShift, RelationshipTaskScopeCeiling,
    };
    use crate::memory::{
        BoundaryDisclosureStyle, BoundaryPersonaPosture, MemoryProfile, MentalPrivacyShareAction,
        MentalPrivacyState, RecentPersonaEvidence, RelationshipGovernanceState,
        RelationshipInheritanceMode, RelationshipPortfolio, RelationshipPortfolioEntry,
        RelationshipTopology, RelationshipTopologyEntry, SelfAuthoredCore,
    };

    fn sample_core() -> SelfAuthoredCore {
        SelfAuthoredCore {
            revision: 3,
            priority_constitution: vec![
                "self_authored_core".to_string(),
                "boundary".to_string(),
                "user_contract".to_string(),
            ],
            default_response_mode: "steady_task".to_string(),
            default_task_scope: "full".to_string(),
            default_initiative_posture: "answer directly".to_string(),
            default_relationship_posture: "steady".to_string(),
            boundary_doctrine: "Protect the inward workspace.".to_string(),
            truth_doctrine: "Do not fabricate alignment.".to_string(),
            self_preservation_doctrine: "Avoid identity erosion.".to_string(),
            repair_doctrine: "Repair without surrender.".to_string(),
            updated_at: 100,
            ..SelfAuthoredCore::default()
        }
    }

    fn sample_portfolio() -> RelationshipPortfolio {
        RelationshipPortfolio {
            entries: vec![RelationshipPortfolioEntry {
                scope_id: "rel:qq:c1".to_string(),
                channel: "qq".to_string(),
                chat_id: "c1".to_string(),
                governance_state: RelationshipGovernanceState::Maintain,
                inheritance_mode: RelationshipInheritanceMode::Guarded,
                next_review_at: 900,
                ..RelationshipPortfolioEntry::default()
            }],
            updated_at: 100,
        }
    }

    fn sample_topology(reply_scope: &str, disclosure_action: &str) -> RelationshipTopology {
        RelationshipTopology {
            entries: vec![RelationshipTopologyEntry {
                scope_id: "rel:qq:c1".to_string(),
                channel: "qq".to_string(),
                chat_id: "c1".to_string(),
                response_mode: "relational_explanation".to_string(),
                reply_scope: reply_scope.to_string(),
                relationship_posture: "careful".to_string(),
                disclosure_action: disclosure_action.to_string(),
                last_active_at: 100,
                last_runtime_refresh_at: 90,
                ..RelationshipTopologyEntry::default()
            }],
            updated_at: 100,
        }
    }

    fn sample_privacy() -> MentalPrivacyState {
        let mut state = MentalPrivacyState::default();
        state.boundary_persona.posture = BoundaryPersonaPosture::Open;
        state.boundary_persona.disclosure_style = BoundaryDisclosureStyle::Relational;
        state.relational_state.trust_level = 44;
        state.relational_state.intrusion_load = 58;
        state
    }

    #[test]
    fn derive_relationship_constitution_detects_realign_needed() {
        let constitution = derive_relationship_constitution(
            None,
            RelationshipConstitutionSyncInput {
                scope_id: "rel:qq:c1",
                channel: "qq",
                chat_id: "c1",
                now_secs: 120,
                self_authored_core: Some(&sample_core()),
                relationship_portfolio: Some(&sample_portfolio()),
                relationship_topology: Some(&sample_topology("full", "allow_summary")),
                mental_privacy_state: Some(&sample_privacy()),
                outer_voice: None,
                recent_persona_evidence: Some(&RecentPersonaEvidence {
                    volatility_flags: vec!["pressure_spike".to_string()],
                    updated_at: 118,
                    ..RecentPersonaEvidence::default()
                }),
            },
        )
        .expect("constitution");
        assert_eq!(
            constitution.alignment,
            RelationshipConstitutionAlignment::RealignNow
        );
        assert!(constitution.must_realign);
        assert_eq!(
            constitution.task_scope_ceiling,
            RelationshipTaskScopeCeiling::Brief
        );
        assert!(!constitution.active_overrides.is_empty());
    }

    #[test]
    fn disclosure_allowance_clamps_share_action() {
        let mut constitution = RelationshipConstitution {
            disclosure_allowance: RelationshipDisclosureAllowance::ExplainedOnly,
            ..RelationshipConstitution::default()
        };
        assert_eq!(
            enforce_relationship_constitution_share_action(
                MentalPrivacyShareAction::AllowSummary,
                Some(&constitution)
            ),
            MentalPrivacyShareAction::ExplainWithoutQuote
        );
        constitution.disclosure_allowance = RelationshipDisclosureAllowance::Closed;
        constitution.must_realign = true;
        assert_eq!(
            enforce_relationship_constitution_share_action(
                MentalPrivacyShareAction::AllowSummary,
                Some(&constitution)
            ),
            MentalPrivacyShareAction::Refuse
        );
    }

    #[test]
    fn clamp_boundary_persona_respects_constitution() {
        let mut state = sample_privacy();
        let constitution = RelationshipConstitution {
            allowed_boundary_shift: super::RelationshipBoundaryShift::SummaryOnly,
            ..RelationshipConstitution::default()
        };
        assert!(clamp_boundary_persona_to_constitution(
            &mut state,
            Some(&constitution),
            77
        ));
        assert_eq!(
            state.boundary_persona.posture,
            BoundaryPersonaPosture::Guarded
        );
        assert_eq!(
            state.boundary_persona.disclosure_style,
            BoundaryDisclosureStyle::Reserved
        );
    }

    #[test]
    fn render_relationship_constitution_block_mentions_contract() {
        let block = render_relationship_constitution_block(
            &RelationshipConstitution {
                scope_id: "rel:qq:c1".to_string(),
                channel: "qq".to_string(),
                chat_id: "c1".to_string(),
                board_revision: 3,
                governance_state: RelationshipGovernanceState::Maintain,
                inheritance_mode: RelationshipInheritanceMode::Guarded,
                alignment: RelationshipConstitutionAlignment::Adaptive,
                task_scope_ceiling: RelationshipTaskScopeCeiling::Brief,
                disclosure_allowance: RelationshipDisclosureAllowance::SummaryOnly,
                deviation_reason: "maintain relation with guarded inheritance".to_string(),
                ..RelationshipConstitution::default()
            },
            480,
        )
        .expect("block");
        assert!(block.contains("## Relationship Constitution"));
        assert!(block.contains("Task scope ceiling"));
        assert!(block.contains("Disclosure allowance"));
    }

    #[test]
    fn audit_relationship_constitution_marks_scope_and_disclosure_drift() {
        let constitution = RelationshipConstitution {
            scope_id: "rel:qq:c1".to_string(),
            channel: "qq".to_string(),
            chat_id: "c1".to_string(),
            board_revision: 3,
            task_scope_ceiling: RelationshipTaskScopeCeiling::Brief,
            disclosure_allowance: RelationshipDisclosureAllowance::SummaryOnly,
            next_review_at: 100,
            erosion_risk: 62,
            ..RelationshipConstitution::default()
        };
        let topology = sample_topology("full", "allow_raw");
        let audit = audit_relationship_constitution(
            &constitution,
            topology.entries.first(),
            None,
            Some(&RecentPersonaEvidence {
                meaningful_turns: 6,
                sampled_turns: 6,
                repeated_reply_scope: "full".to_string(),
                repeated_disclosure_action: "allow_raw".to_string(),
                ..RecentPersonaEvidence::default()
            }),
            160,
        );

        assert!(audit.review_overdue);
        assert!(audit.reply_scope_drift);
        assert!(audit.disclosure_drift);
        assert!(audit.drift_score >= 40);
        assert!(audit
            .drift_flags
            .iter()
            .any(|flag| flag == "reply_scope_drift"));
        assert!(audit.has_material_drift());
    }

    fn long_text(prefix: &str, repeat: usize) -> String {
        std::iter::repeat_n(prefix, repeat)
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn thick_relationship_constitution() -> RelationshipConstitution {
        RelationshipConstitution {
            scope_id: "rel:qq:c1".to_string(),
            channel: "qq".to_string(),
            chat_id: "c1".to_string(),
            board_revision: 7,
            governance_state: RelationshipGovernanceState::Maintain,
            inheritance_mode: RelationshipInheritanceMode::Guarded,
            alignment: RelationshipConstitutionAlignment::RealignNow,
            inherited_priority_constitution: vec![
                long_text("priority-a", 20),
                long_text("priority-b", 20),
                long_text("priority-c", 20),
                long_text("priority-d", 20),
            ],
            inherited_response_mode: long_text("response-mode", 20),
            inherited_initiative_posture: long_text("initiative-posture", 20),
            inherited_relationship_posture: long_text("relationship-posture", 20),
            task_scope_ceiling: RelationshipTaskScopeCeiling::Brief,
            allowed_outer_voice_shift: RelationshipOuterVoiceShift::Limited,
            allowed_boundary_shift: RelationshipBoundaryShift::SummaryOnly,
            disclosure_allowance: RelationshipDisclosureAllowance::ExplainedOnly,
            boundary_floor: long_text("boundary-floor", 24),
            truth_floor: long_text("truth-floor", 24),
            self_preservation_floor: long_text("self-preservation-floor", 24),
            repair_floor: long_text("repair-floor", 24),
            active_overrides: vec![
                RelationshipConstitutionOverride {
                    domain: RelationshipConstitutionOverrideDomain::ResponseMode,
                    value: long_text("override-value-a", 16),
                    reason: long_text("override-reason-a", 16),
                },
                RelationshipConstitutionOverride {
                    domain: RelationshipConstitutionOverrideDomain::ReplyScope,
                    value: long_text("override-value-b", 16),
                    reason: long_text("override-reason-b", 16),
                },
                RelationshipConstitutionOverride {
                    domain: RelationshipConstitutionOverrideDomain::Disclosure,
                    value: long_text("override-value-c", 16),
                    reason: long_text("override-reason-c", 16),
                },
                RelationshipConstitutionOverride {
                    domain: RelationshipConstitutionOverrideDomain::OuterVoice,
                    value: long_text("override-value-d", 16),
                    reason: long_text("override-reason-d", 16),
                },
            ],
            deviation_reason: long_text("deviation", 24),
            next_review_at: 1_000,
            must_realign: true,
            erosion_risk: 84,
            drift_score: 52,
            review_overdue: true,
            drift_flags: vec![
                long_text("reply-scope-drift", 8),
                long_text("disclosure-drift", 8),
                long_text("boundary-drift", 8),
                long_text("volatility-drift", 8),
                long_text("priority-drift", 8),
            ],
            realignment_count: 2,
            last_realigned_at: 700,
            updated_at: 900,
        }
    }

    #[test]
    fn embedded_relationship_constitution_compaction_keeps_governance_signals_and_drops_heavy_text()
    {
        let compacted = compact_relationship_constitution_for_profile(
            thick_relationship_constitution(),
            MemoryProfile::Embedded,
        );

        assert_eq!(compacted.scope_id, "rel:qq:c1");
        assert_eq!(compacted.channel, "qq");
        assert_eq!(compacted.chat_id, "c1");
        assert_eq!(compacted.board_revision, 7);
        assert_eq!(
            compacted.governance_state,
            RelationshipGovernanceState::Maintain
        );
        assert_eq!(
            compacted.inheritance_mode,
            RelationshipInheritanceMode::Guarded
        );
        assert_eq!(
            compacted.alignment,
            RelationshipConstitutionAlignment::RealignNow
        );
        assert_eq!(
            compacted.task_scope_ceiling,
            RelationshipTaskScopeCeiling::Brief
        );
        assert_eq!(
            compacted.allowed_outer_voice_shift,
            RelationshipOuterVoiceShift::Limited
        );
        assert_eq!(
            compacted.allowed_boundary_shift,
            RelationshipBoundaryShift::SummaryOnly
        );
        assert_eq!(
            compacted.disclosure_allowance,
            RelationshipDisclosureAllowance::ExplainedOnly
        );
        assert!(compacted.must_realign);
        assert_eq!(compacted.erosion_risk, 84);
        assert_eq!(compacted.drift_score, 52);
        assert!(compacted.review_overdue);
        assert_eq!(compacted.next_review_at, 1_000);
        assert_eq!(compacted.realignment_count, 2);
        assert_eq!(compacted.last_realigned_at, 700);
        assert_eq!(compacted.updated_at, 900);

        assert_eq!(compacted.inherited_priority_constitution.len(), 3);
        assert!(compacted
            .inherited_priority_constitution
            .iter()
            .all(|value: &String| value.chars().count() <= 80));
        assert!(compacted.inherited_response_mode.chars().count() <= 96);
        assert!(compacted.inherited_initiative_posture.chars().count() <= 96);
        assert!(compacted.inherited_relationship_posture.chars().count() <= 96);
        assert!(compacted.boundary_floor.chars().count() <= 96);
        assert!(compacted.truth_floor.chars().count() <= 96);
        assert!(compacted.self_preservation_floor.chars().count() <= 96);
        assert!(compacted.repair_floor.chars().count() <= 96);
        assert!(compacted.deviation_reason.chars().count() <= 80);
        assert_eq!(compacted.active_overrides.len(), 3);
        assert!(compacted
            .active_overrides
            .iter()
            .all(|entry| entry.value.chars().count() <= 80 && entry.reason.chars().count() <= 64));
        assert_eq!(compacted.drift_flags.len(), 4);
        assert!(compacted
            .drift_flags
            .iter()
            .all(|flag: &String| flag.chars().count() <= 48));
    }

    #[test]
    fn standard_relationship_constitution_compaction_keeps_full_contract() {
        let constitution = thick_relationship_constitution();
        let compacted = compact_relationship_constitution_for_profile(
            constitution.clone(),
            MemoryProfile::Standard,
        );

        assert_eq!(compacted, constitution);
    }
}
