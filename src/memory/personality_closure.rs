//! Personality governance inspection and closure gate.
//! 人格治理检查与人格封板门。

use crate::util::truncate_content_to_max;
use serde::{Deserialize, Serialize};
use std::fmt::Write as _;

use super::{
    CoreRevisionGovernanceDigest, CoreRevisionLedger, CoreRevisionTimelineEntry,
    RecentPersonaEvidence, RelationshipConstitution, RelationshipConstitutionAudit,
    RelationshipTopology, SelfAuthoredCore, audit_relationship_constitution,
    build_core_revision_timeline, compute_core_revision_governance_digest, relationship_scope_id,
};

const PERSONALITY_CLOSURE_TEXT_MAX_CHARS: usize = 160;
const PERSONALITY_CLOSURE_OUTSTANDING_MAX: usize = 6;
const PERSONALITY_CLOSURE_EVENT_LIMIT: usize = 8;
const PERSONALITY_MIN_EVIDENCE_TURNS: usize = 4;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct PersonalityGovernanceEvent {
    #[serde(default)]
    pub layer: String,
    #[serde(default)]
    pub at: u64,
    #[serde(default)]
    pub summary: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct PersonalityClosureReport {
    #[serde(default)]
    pub ready: bool,
    #[serde(default)]
    pub board_core_ready: bool,
    #[serde(default)]
    pub revision_governance_ready: bool,
    #[serde(default)]
    pub relationship_governance_ready: bool,
    #[serde(default)]
    pub evidence_loop_ready: bool,
    #[serde(default)]
    pub drift_control_ready: bool,
    #[serde(default)]
    pub observation_control_ready: bool,
    #[serde(default)]
    pub review_cadence_ready: bool,
    #[serde(default)]
    pub outstanding: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct PersonalityGovernanceInspection {
    #[serde(default)]
    pub subject_id: String,
    #[serde(default)]
    pub relationship_scope_id: String,
    #[serde(default)]
    pub core_revision_governance: CoreRevisionGovernanceDigest,
    #[serde(default)]
    pub core_revision_timeline: Vec<CoreRevisionTimelineEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relationship_audit: Option<RelationshipConstitutionAudit>,
    #[serde(default)]
    pub governance_events: Vec<PersonalityGovernanceEvent>,
    #[serde(default)]
    pub closure: PersonalityClosureReport,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PersonalityGovernanceInspectionInput<'a> {
    pub channel: &'a str,
    pub chat_id: &'a str,
    pub now_secs: u64,
    pub self_authored_core: Option<&'a SelfAuthoredCore>,
    pub core_revision_ledger: Option<&'a CoreRevisionLedger>,
    pub relationship_constitution: Option<&'a RelationshipConstitution>,
    pub relationship_topology: Option<&'a RelationshipTopology>,
    pub recent_persona_evidence: Option<&'a RecentPersonaEvidence>,
}

pub fn inspect_personality_governance(
    input: PersonalityGovernanceInspectionInput<'_>,
) -> PersonalityGovernanceInspection {
    let relationship_scope_id = relationship_scope_id(input.channel, input.chat_id);
    let core_revision_governance = compute_core_revision_governance_digest(
        input.core_revision_ledger,
        input
            .self_authored_core
            .map(|core| core.last_reviewed_at)
            .unwrap_or(0),
        input
            .self_authored_core
            .map(|core| core.stability_score)
            .unwrap_or(0),
        input.now_secs,
    );
    let core_revision_timeline = input
        .core_revision_ledger
        .map(|ledger| build_core_revision_timeline(ledger, PERSONALITY_CLOSURE_EVENT_LIMIT))
        .unwrap_or_default();
    let relationship_audit = input.relationship_constitution.map(|constitution| {
        let topology_entry = input.relationship_topology.and_then(|topology| {
            topology
                .entries
                .iter()
                .find(|entry| entry.scope_id.trim() == relationship_scope_id)
        });
        audit_relationship_constitution(
            constitution,
            topology_entry,
            None,
            input.recent_persona_evidence,
            input.now_secs,
        )
    });
    let governance_events = build_governance_events(
        &core_revision_timeline,
        input.relationship_constitution,
        relationship_audit.as_ref(),
        input.recent_persona_evidence,
    );
    let closure = build_personality_closure_report(
        input.self_authored_core,
        input.core_revision_ledger,
        &core_revision_governance,
        input.relationship_constitution,
        relationship_audit.as_ref(),
        input.recent_persona_evidence,
    );
    PersonalityGovernanceInspection {
        subject_id: super::board_subject_scope_id().to_string(),
        relationship_scope_id,
        core_revision_governance,
        core_revision_timeline,
        relationship_audit,
        governance_events,
        closure,
    }
}

pub fn render_personality_governance_inspection_markdown(
    inspection: &PersonalityGovernanceInspection,
) -> String {
    let mut out = String::with_capacity(2048);
    out.push_str("# Personality Governance Inspection\n\n");
    let _ = writeln!(out, "- Subject: {}", inspection.subject_id);
    let _ = writeln!(
        out,
        "- Relationship scope: {}",
        inspection.relationship_scope_id
    );
    let _ = writeln!(out, "- Closure ready: {}", inspection.closure.ready);
    let _ = writeln!(
        out,
        "- Board core ready: {} | revision governance ready: {} | relationship governance ready: {}",
        inspection.closure.board_core_ready,
        inspection.closure.revision_governance_ready,
        inspection.closure.relationship_governance_ready
    );
    let _ = writeln!(
        out,
        "- Evidence loop ready: {} | drift control ready: {} | observation control ready: {} | review cadence ready: {}",
        inspection.closure.evidence_loop_ready,
        inspection.closure.drift_control_ready,
        inspection.closure.observation_control_ready,
        inspection.closure.review_cadence_ready
    );
    if !inspection.closure.outstanding.is_empty() {
        out.push_str("\n## Outstanding\n");
        for item in &inspection.closure.outstanding {
            let _ = writeln!(out, "- {}", item);
        }
    }
    out.push_str("\n## Governance Events\n");
    if inspection.governance_events.is_empty() {
        out.push_str("- No governance events available.\n");
    } else {
        for event in &inspection.governance_events {
            let _ = writeln!(out, "- [{} @ {}] {}", event.layer, event.at, event.summary);
        }
    }
    out
}

fn build_governance_events(
    core_revision_timeline: &[CoreRevisionTimelineEntry],
    relationship_constitution: Option<&RelationshipConstitution>,
    relationship_audit: Option<&RelationshipConstitutionAudit>,
    recent_persona_evidence: Option<&RecentPersonaEvidence>,
) -> Vec<PersonalityGovernanceEvent> {
    let mut events = core_revision_timeline
        .iter()
        .map(|entry| {
            let mut summary = format!("rev {} {}", entry.resulting_revision, entry.outcome.label());
            if let Some(kind) = entry.correction_kind {
                let _ = write!(
                    summary,
                    " {}={}",
                    kind.label(),
                    entry.corrects_revision.unwrap_or(0)
                );
            }
            if !entry.adjudication_reason.trim().is_empty() {
                let _ = write!(summary, " {}", entry.adjudication_reason.trim());
            }
            PersonalityGovernanceEvent {
                layer: "board_core".to_string(),
                at: entry.reviewed_at,
                summary: truncate_content_to_max(
                    summary.trim(),
                    PERSONALITY_CLOSURE_TEXT_MAX_CHARS,
                )
                .into_owned(),
            }
        })
        .collect::<Vec<_>>();
    if let Some(constitution) = relationship_constitution {
        let mut summary = format!(
            "alignment={} must_realign={} drift_score={} review_overdue={}",
            constitution.alignment.label(),
            constitution.must_realign,
            constitution.drift_score,
            constitution.review_overdue
        );
        if let Some(audit) = relationship_audit {
            if !audit.drift_flags.is_empty() {
                let _ = write!(summary, " flags={}", audit.drift_flags.join(","));
            }
        }
        events.push(PersonalityGovernanceEvent {
            layer: "relationship".to_string(),
            at: constitution.updated_at,
            summary: truncate_content_to_max(summary.trim(), PERSONALITY_CLOSURE_TEXT_MAX_CHARS)
                .into_owned(),
        });
    }
    if let Some(evidence) = recent_persona_evidence {
        let mut summary = format!(
            "sampled={} meaningful={} scope={} disclosure={}",
            evidence.sampled_turns,
            evidence.meaningful_turns,
            evidence.repeated_reply_scope.trim(),
            evidence.repeated_disclosure_action.trim()
        );
        if !evidence.volatility_flags.is_empty() {
            let _ = write!(
                summary,
                " volatility={}",
                evidence.volatility_flags.join(",")
            );
        }
        events.push(PersonalityGovernanceEvent {
            layer: "recent_evidence".to_string(),
            at: evidence.updated_at,
            summary: truncate_content_to_max(summary.trim(), PERSONALITY_CLOSURE_TEXT_MAX_CHARS)
                .into_owned(),
        });
    }
    events.sort_by(|left, right| {
        right
            .at
            .cmp(&left.at)
            .then_with(|| left.layer.cmp(&right.layer))
    });
    events.truncate(PERSONALITY_CLOSURE_EVENT_LIMIT);
    events
}

fn build_personality_closure_report(
    self_authored_core: Option<&SelfAuthoredCore>,
    core_revision_ledger: Option<&CoreRevisionLedger>,
    governance: &CoreRevisionGovernanceDigest,
    relationship_constitution: Option<&RelationshipConstitution>,
    relationship_audit: Option<&RelationshipConstitutionAudit>,
    recent_persona_evidence: Option<&RecentPersonaEvidence>,
) -> PersonalityClosureReport {
    let board_core_ready = self_authored_core.is_some_and(|core| {
        core.revision > 0 && !core.identity_anchor.trim().is_empty() && core.stability_score > 0
    });
    let revision_governance_ready =
        core_revision_ledger.is_some_and(|ledger| ledger.is_meaningful());
    let relationship_governance_ready = relationship_constitution.is_some_and(|constitution| {
        constitution.is_meaningful() && constitution.board_revision > 0
    });
    let evidence_loop_ready = recent_persona_evidence.is_some_and(|evidence| {
        evidence.is_meaningful() && evidence.meaningful_turns >= PERSONALITY_MIN_EVIDENCE_TURNS
    });
    let drift_control_ready = relationship_audit.is_some_and(|audit| !audit.has_material_drift());
    let observation_control_ready = !governance.observation_active;
    let review_cadence_ready = !governance.review_due
        && relationship_constitution.is_none_or(|constitution| !constitution.review_overdue);
    let mut outstanding = Vec::with_capacity(PERSONALITY_CLOSURE_OUTSTANDING_MAX);
    if !board_core_ready {
        outstanding.push("board_core_not_stable".to_string());
    }
    if !revision_governance_ready {
        outstanding.push("revision_governance_history_missing".to_string());
    }
    if !relationship_governance_ready {
        outstanding.push("relationship_constitution_missing".to_string());
    }
    if !evidence_loop_ready {
        outstanding.push("recent_persona_evidence_insufficient".to_string());
    }
    if !drift_control_ready {
        outstanding.push("relationship_drift_not_under_control".to_string());
    }
    if !observation_control_ready {
        outstanding.push("board_core_still_under_observation".to_string());
    }
    if !review_cadence_ready {
        outstanding.push("governance_review_due".to_string());
    }
    outstanding.truncate(PERSONALITY_CLOSURE_OUTSTANDING_MAX);
    PersonalityClosureReport {
        ready: board_core_ready
            && revision_governance_ready
            && relationship_governance_ready
            && evidence_loop_ready
            && drift_control_ready
            && observation_control_ready
            && review_cadence_ready,
        board_core_ready,
        revision_governance_ready,
        relationship_governance_ready,
        evidence_loop_ready,
        drift_control_ready,
        observation_control_ready,
        review_cadence_ready,
        outstanding,
    }
}
