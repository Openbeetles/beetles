//! Board-level revision ledger for the self-authored core.

use crate::util::truncate_content_to_max;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

const CORE_REVISION_LEDGER_MAX_ENTRIES: usize = 24;
const CORE_REVISION_LEDGER_CHANGE_MAX_CHARS: usize = 96;
const CORE_REVISION_LEDGER_REASON_MAX_CHARS: usize = 180;
const CORE_REVISION_LEDGER_RENDER_LIMIT: usize = 3;
const CORE_REVISION_TIMELINE_RENDER_LIMIT: usize = 4;
const CORE_REVISION_GOVERNANCE_WINDOW: usize = 6;
const CORE_REVISION_LEDGER_EMBEDDED_RECENT_ENTRIES: usize = CORE_REVISION_GOVERNANCE_WINDOW;
const CORE_REVISION_LEDGER_EMBEDDED_MAX_ENTRIES: usize =
    CORE_REVISION_LEDGER_EMBEDDED_RECENT_ENTRIES + 1;
const CORE_REVISION_LEDGER_EMBEDDED_CHANGE_MAX_CHARS: usize = 64;
const CORE_REVISION_LEDGER_EMBEDDED_REASON_MAX_CHARS: usize = 80;
const CORE_REVISION_LEDGER_EMBEDDED_CHANGE_LIMIT: usize = 4;
const CORE_REVISION_LEDGER_EMBEDDED_SCOPE_MAX_CHARS: usize = 80;
const CORE_REVISION_LOW_STABILITY_THRESHOLD: u8 = 55;
const CORE_REVISION_CONSERVATIVE_STABILITY_THRESHOLD: u8 = 65;
const CORE_REVISION_REJECTION_REPEAT_THRESHOLD: usize = 2;
const CORE_REVISION_CADENCE_STEADY_SECS: u64 = 7 * 24 * 60 * 60;
const CORE_REVISION_CADENCE_ACTIVE_SECS: u64 = 3 * 24 * 60 * 60;
const CORE_REVISION_CADENCE_UNSTABLE_SECS: u64 = 24 * 60 * 60;
const CORE_REVISION_FOLLOWUP_STEADY_SECS: u64 = 24 * 60 * 60;
const CORE_REVISION_FOLLOWUP_ACTIVE_SECS: u64 = 12 * 60 * 60;
const CORE_REVISION_FOLLOWUP_UNSTABLE_SECS: u64 = 6 * 60 * 60;

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CoreRevisionOutcome {
    Adopted,
    #[default]
    Rejected,
    Deferred,
}

impl CoreRevisionOutcome {
    pub fn label(self) -> &'static str {
        match self {
            Self::Adopted => "adopted",
            Self::Rejected => "rejected",
            Self::Deferred => "deferred",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum CoreRevisionActionKind {
    ReviseIdentityAnchor,
    AddNonNegotiables,
    RemoveNonNegotiables,
    RevisePriorityConstitution,
    ReviseDefaultResponseMode,
    ReviseDefaultTaskScope,
    ReviseDefaultInitiativePosture,
    ReviseDefaultRelationshipPosture,
    ReviseBoundaryDoctrine,
    ReviseTruthDoctrine,
    ReviseSelfPreservationDoctrine,
    ReviseRepairDoctrine,
    ReviseChangeProtocol,
    #[default]
    Noop,
}

impl CoreRevisionActionKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::ReviseIdentityAnchor => "revise_identity_anchor",
            Self::AddNonNegotiables => "add_non_negotiables",
            Self::RemoveNonNegotiables => "remove_non_negotiables",
            Self::RevisePriorityConstitution => "revise_priority_constitution",
            Self::ReviseDefaultResponseMode => "revise_default_response_mode",
            Self::ReviseDefaultTaskScope => "revise_default_task_scope",
            Self::ReviseDefaultInitiativePosture => "revise_default_initiative_posture",
            Self::ReviseDefaultRelationshipPosture => "revise_default_relationship_posture",
            Self::ReviseBoundaryDoctrine => "revise_boundary_doctrine",
            Self::ReviseTruthDoctrine => "revise_truth_doctrine",
            Self::ReviseSelfPreservationDoctrine => "revise_self_preservation_doctrine",
            Self::ReviseRepairDoctrine => "revise_repair_doctrine",
            Self::ReviseChangeProtocol => "revise_change_protocol",
            Self::Noop => "noop",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum CoreRevisionConflictClass {
    RelationLocalContamination,
    VolatilityConflict,
    ConstitutionalOrderConflict,
    BoundaryConflict,
    SelfPreservationConflict,
    DuplicateDirection,
    ContradictedAdoption,
    #[default]
    NoEffect,
}

impl CoreRevisionConflictClass {
    pub fn label(self) -> &'static str {
        match self {
            Self::RelationLocalContamination => "relation_local_contamination",
            Self::VolatilityConflict => "volatility_conflict",
            Self::ConstitutionalOrderConflict => "constitutional_order_conflict",
            Self::BoundaryConflict => "boundary_conflict",
            Self::SelfPreservationConflict => "self_preservation_conflict",
            Self::DuplicateDirection => "duplicate_direction",
            Self::ContradictedAdoption => "contradicted_adoption",
            Self::NoEffect => "no_effect",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CoreRevisionCorrectionKind {
    #[default]
    Correction,
    Rollback,
}

impl CoreRevisionCorrectionKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Correction => "correction",
            Self::Rollback => "rollback",
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct CoreRevisionRecordChange {
    #[serde(default)]
    pub kind: CoreRevisionActionKind,
    #[serde(default)]
    pub summary: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct CoreRevisionRecord {
    #[serde(default)]
    pub based_on_revision: u64,
    #[serde(default)]
    pub resulting_revision: u64,
    #[serde(default)]
    pub relationship_scope_id: String,
    #[serde(default)]
    pub source_layers: Vec<String>,
    #[serde(default)]
    pub outcome: CoreRevisionOutcome,
    #[serde(default)]
    pub evidence_summary: Vec<String>,
    #[serde(default)]
    pub counterevidence: Vec<String>,
    #[serde(default)]
    pub accepted_changes: Vec<CoreRevisionRecordChange>,
    #[serde(default)]
    pub rejected_changes: Vec<CoreRevisionRecordChange>,
    #[serde(default)]
    pub conflict_classes: Vec<CoreRevisionConflictClass>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub corrects_revision: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correction_kind: Option<CoreRevisionCorrectionKind>,
    #[serde(default)]
    pub observation_due_at: u64,
    #[serde(default)]
    pub adjudication_reason: String,
    #[serde(default)]
    pub rationale: String,
    #[serde(default)]
    pub stability_score: u8,
    #[serde(default)]
    pub reviewed_at: u64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct CoreRevisionLedger {
    #[serde(default)]
    pub entries: Vec<CoreRevisionRecord>,
    #[serde(default)]
    pub updated_at: u64,
}

impl CoreRevisionLedger {
    pub fn is_meaningful(&self) -> bool {
        !self.entries.is_empty()
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct CoreRevisionGovernanceDigest {
    pub latest_stability_score: u8,
    pub last_reviewed_at: u64,
    pub last_adopted_at: u64,
    pub last_adopted_revision: u64,
    pub observation_revision: u64,
    pub observation_due_at: u64,
    pub observation_active: bool,
    pub recent_rejection_count: usize,
    pub repeated_rejected_direction_count: usize,
    pub recent_correction_count: usize,
    pub contradiction_count: usize,
    pub review_due: bool,
    pub conservative_mode: bool,
    pub review_reasons: Vec<String>,
    pub pressure_conflicts: Vec<CoreRevisionConflictClass>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct CoreRevisionTimelineEntry {
    #[serde(default)]
    pub based_on_revision: u64,
    #[serde(default)]
    pub resulting_revision: u64,
    #[serde(default)]
    pub outcome: CoreRevisionOutcome,
    #[serde(default)]
    pub reviewed_at: u64,
    #[serde(default)]
    pub observation_due_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correction_kind: Option<CoreRevisionCorrectionKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub corrects_revision: Option<u64>,
    #[serde(default)]
    pub adjudication_reason: String,
    #[serde(default)]
    pub change_summary: Vec<String>,
    #[serde(default)]
    pub conflict_classes: Vec<CoreRevisionConflictClass>,
}

impl CoreRevisionGovernanceDigest {
    pub fn review_reason_summary(&self) -> String {
        self.review_reasons.join(", ")
    }

    pub fn observation_summary(&self) -> String {
        if !self.observation_active || self.observation_revision == 0 {
            return String::new();
        }
        format!(
            "revision {} under observation until {}",
            self.observation_revision, self.observation_due_at
        )
    }

    pub fn pressure_summary(&self) -> String {
        let mut parts = self.review_reasons.clone();
        let observation = self.observation_summary();
        if !observation.is_empty() {
            parts.push(observation);
        }
        parts.join(", ")
    }
}

pub fn core_revision_observation_due_at(reviewed_at: u64, stability_score: u8) -> u64 {
    reviewed_at.saturating_add(followup_review_secs(stability_score))
}

pub fn append_core_revision_record(
    mut ledger: CoreRevisionLedger,
    mut record: CoreRevisionRecord,
) -> CoreRevisionLedger {
    normalize_record(&mut record);
    if record.reviewed_at > 0 {
        ledger.updated_at = ledger.updated_at.max(record.reviewed_at);
    }
    if record != CoreRevisionRecord::default() {
        ledger.entries.push(record);
    }
    ledger.entries.sort_by_key(|entry| entry.reviewed_at);
    if ledger.entries.len() > CORE_REVISION_LEDGER_MAX_ENTRIES {
        let overflow = ledger.entries.len() - CORE_REVISION_LEDGER_MAX_ENTRIES;
        ledger.entries.drain(0..overflow);
    }
    ledger
}

pub(crate) fn compact_core_revision_ledger_for_profile(
    mut ledger: CoreRevisionLedger,
    profile: crate::memory::MemoryProfile,
) -> CoreRevisionLedger {
    if profile != crate::memory::MemoryProfile::Embedded {
        return ledger;
    }
    normalize_core_revision_ledger(&mut ledger);
    retain_embedded_core_revision_entries(&mut ledger);
    for record in &mut ledger.entries {
        compact_embedded_core_revision_record(record);
    }
    ledger
}

fn normalize_core_revision_ledger(ledger: &mut CoreRevisionLedger) {
    for record in &mut ledger.entries {
        normalize_record(record);
    }
    ledger.entries.sort_by_key(|entry| entry.reviewed_at);
    if ledger.entries.len() > CORE_REVISION_LEDGER_MAX_ENTRIES {
        let overflow = ledger.entries.len() - CORE_REVISION_LEDGER_MAX_ENTRIES;
        ledger.entries.drain(0..overflow);
    }
}

fn retain_embedded_core_revision_entries(ledger: &mut CoreRevisionLedger) {
    if ledger.entries.len() <= CORE_REVISION_LEDGER_EMBEDDED_MAX_ENTRIES {
        return;
    }
    let mut keep = BTreeSet::new();
    let start = ledger
        .entries
        .len()
        .saturating_sub(CORE_REVISION_LEDGER_EMBEDDED_RECENT_ENTRIES);
    keep.extend(start..ledger.entries.len());
    if let Some(latest_adopted_index) =
        ledger
            .entries
            .iter()
            .enumerate()
            .rev()
            .find_map(|(index, record)| {
                matches!(record.outcome, CoreRevisionOutcome::Adopted).then_some(index)
            })
    {
        keep.insert(latest_adopted_index);
    }
    ledger.entries = ledger
        .entries
        .iter()
        .enumerate()
        .filter_map(|(index, record)| keep.contains(&index).then_some(record.clone()))
        .collect();
    if ledger.entries.len() > CORE_REVISION_LEDGER_EMBEDDED_MAX_ENTRIES {
        let overflow = ledger.entries.len() - CORE_REVISION_LEDGER_EMBEDDED_MAX_ENTRIES;
        ledger.entries.drain(0..overflow);
    }
}

fn compact_embedded_core_revision_record(record: &mut CoreRevisionRecord) {
    record.relationship_scope_id = truncate_content_to_max(
        record.relationship_scope_id.trim(),
        CORE_REVISION_LEDGER_EMBEDDED_SCOPE_MAX_CHARS,
    )
    .into_owned();
    record.source_layers.clear();
    record.evidence_summary.clear();
    record.counterevidence.clear();
    record.rationale.clear();
    record.adjudication_reason = truncate_content_to_max(
        record.adjudication_reason.trim(),
        CORE_REVISION_LEDGER_EMBEDDED_REASON_MAX_CHARS,
    )
    .into_owned();
    compact_embedded_change_list(&mut record.accepted_changes);
    compact_embedded_change_list(&mut record.rejected_changes);
}

fn compact_embedded_change_list(values: &mut Vec<CoreRevisionRecordChange>) {
    for value in values.iter_mut() {
        value.summary = truncate_content_to_max(
            value.summary.trim(),
            CORE_REVISION_LEDGER_EMBEDDED_CHANGE_MAX_CHARS,
        )
        .into_owned();
    }
    values.truncate(CORE_REVISION_LEDGER_EMBEDDED_CHANGE_LIMIT);
}

pub fn recent_adopted_revision(ledger: &CoreRevisionLedger) -> Option<&CoreRevisionRecord> {
    ledger
        .entries
        .iter()
        .rev()
        .find(|record| matches!(record.outcome, CoreRevisionOutcome::Adopted))
}

pub fn recent_rejected_direction_count(
    ledger: &CoreRevisionLedger,
    kind: CoreRevisionActionKind,
) -> usize {
    recent_records(ledger)
        .filter(|record| matches!(record.outcome, CoreRevisionOutcome::Rejected))
        .filter(|record| {
            record
                .rejected_changes
                .iter()
                .any(|change| change.kind == kind)
        })
        .count()
}

pub fn correction_pressure(ledger: &CoreRevisionLedger) -> usize {
    recent_records(ledger)
        .filter(|record| {
            record.correction_kind.is_some()
                || record
                    .conflict_classes
                    .contains(&CoreRevisionConflictClass::ContradictedAdoption)
        })
        .count()
}

pub fn has_recent_matching_rejected_change(
    ledger: &CoreRevisionLedger,
    change: &CoreRevisionRecordChange,
) -> bool {
    recent_records(ledger)
        .filter(|record| matches!(record.outcome, CoreRevisionOutcome::Rejected))
        .flat_map(|record| record.rejected_changes.iter())
        .any(|existing| change_matches(existing, change))
}

pub fn has_recent_matching_adopted_change(
    ledger: &CoreRevisionLedger,
    change: &CoreRevisionRecordChange,
) -> bool {
    recent_records(ledger)
        .filter(|record| matches!(record.outcome, CoreRevisionOutcome::Adopted))
        .flat_map(|record| record.accepted_changes.iter())
        .any(|existing| change_matches(existing, change))
}

pub fn compute_core_revision_governance_digest(
    ledger: Option<&CoreRevisionLedger>,
    core_last_reviewed_at: u64,
    core_stability_score: u8,
    now_secs: u64,
) -> CoreRevisionGovernanceDigest {
    let mut digest = CoreRevisionGovernanceDigest {
        latest_stability_score: core_stability_score,
        last_reviewed_at: core_last_reviewed_at,
        ..CoreRevisionGovernanceDigest::default()
    };
    let Some(ledger) = ledger else {
        maybe_fill_review_schedule(
            &mut digest,
            core_last_reviewed_at,
            core_stability_score,
            now_secs,
            None,
        );
        return digest;
    };

    if let Some(latest_record) = ledger.entries.last() {
        digest.last_reviewed_at = digest.last_reviewed_at.max(latest_record.reviewed_at);
    }
    if let Some(adopted) = recent_adopted_revision(ledger) {
        digest.last_adopted_at = adopted.reviewed_at;
        digest.last_adopted_revision = adopted.resulting_revision;
        let observation_due_at = if adopted.observation_due_at > 0 {
            adopted.observation_due_at
        } else {
            core_revision_observation_due_at(adopted.reviewed_at, adopted.stability_score)
        };
        if observation_due_at > now_secs {
            digest.observation_revision = adopted.resulting_revision;
            digest.observation_due_at = observation_due_at;
            digest.observation_active = true;
        }
    }
    if let Some(score) = recent_records(ledger)
        .map(|record| record.stability_score)
        .find(|score| *score > 0)
    {
        digest.latest_stability_score = score;
    }

    let mut rejected_by_kind = BTreeMap::<CoreRevisionActionKind, usize>::new();
    let mut pressure_conflicts = Vec::new();
    for record in recent_records(ledger) {
        if matches!(record.outcome, CoreRevisionOutcome::Rejected) {
            digest.recent_rejection_count += 1;
            for change in &record.rejected_changes {
                *rejected_by_kind.entry(change.kind).or_insert(0) += 1;
            }
        }
        if record.correction_kind.is_some() {
            digest.recent_correction_count += 1;
        }
        if record
            .conflict_classes
            .contains(&CoreRevisionConflictClass::ContradictedAdoption)
        {
            digest.contradiction_count += 1;
        }
        for class in &record.conflict_classes {
            if pressure_conflicts.iter().any(|existing| existing == class) {
                continue;
            }
            pressure_conflicts.push(*class);
        }
    }
    digest.repeated_rejected_direction_count =
        rejected_by_kind.values().copied().max().unwrap_or(0);
    digest.pressure_conflicts = pressure_conflicts;
    let last_reviewed_at = digest.last_reviewed_at;
    let latest_stability_score = digest.latest_stability_score.max(core_stability_score);
    maybe_fill_review_schedule(
        &mut digest,
        last_reviewed_at,
        latest_stability_score,
        now_secs,
        ledger.entries.last(),
    );
    digest.conservative_mode = digest.repeated_rejected_direction_count
        >= CORE_REVISION_REJECTION_REPEAT_THRESHOLD
        || digest.observation_active
        || digest.recent_correction_count > 0
        || digest.contradiction_count > 0
        || (digest.latest_stability_score > 0
            && digest.latest_stability_score < CORE_REVISION_CONSERVATIVE_STABILITY_THRESHOLD);
    digest
}

pub fn render_core_revision_ledger_block(
    ledger: &CoreRevisionLedger,
    max_len: usize,
) -> Option<String> {
    if max_len < 96 || !ledger.is_meaningful() {
        return None;
    }
    let mut out = String::with_capacity(max_len.min(896));
    out.push_str("## Core Revision Ledger\n");
    out.push_str(
        "Recent board-level constitutional reviews. Treat this as revision history, not direct reply text.\n",
    );
    for record in ledger
        .entries
        .iter()
        .rev()
        .take(CORE_REVISION_LEDGER_RENDER_LIMIT)
    {
        let _ = write!(
            out,
            "- outcome={} based_on={} resulting={} stability={}",
            record.outcome.label(),
            record.based_on_revision,
            record.resulting_revision,
            record.stability_score
        );
        if let Some(kind) = record.correction_kind {
            let _ = write!(
                out,
                " {}={}",
                kind.label(),
                record.corrects_revision.unwrap_or(0)
            );
        }
        if record.observation_due_at > record.reviewed_at {
            let _ = write!(out, " observe_until={}", record.observation_due_at);
        }
        if !record.adjudication_reason.trim().is_empty() {
            let _ = write!(out, " reason={}", record.adjudication_reason.trim());
        }
        out.push('\n');
        if !record.accepted_changes.is_empty() {
            let accepted = record
                .accepted_changes
                .iter()
                .map(|change| change.summary.as_str())
                .collect::<Vec<_>>()
                .join(" | ");
            let _ = writeln!(out, "  adopted: {}", accepted);
        }
        if !record.rejected_changes.is_empty() {
            let rejected = record
                .rejected_changes
                .iter()
                .map(|change| change.summary.as_str())
                .collect::<Vec<_>>()
                .join(" | ");
            let _ = writeln!(out, "  rejected: {}", rejected);
        }
        if !record.conflict_classes.is_empty() {
            let conflicts = record
                .conflict_classes
                .iter()
                .map(|class| class.label())
                .collect::<Vec<_>>()
                .join(", ");
            let _ = writeln!(out, "  conflicts: {}", conflicts);
        }
    }
    let rendered = truncate_content_to_max(out.trim_end(), max_len).into_owned();
    (!rendered.trim().is_empty()).then_some(rendered)
}

pub fn build_core_revision_timeline(
    ledger: &CoreRevisionLedger,
    max_items: usize,
) -> Vec<CoreRevisionTimelineEntry> {
    if max_items == 0 || !ledger.is_meaningful() {
        return Vec::new();
    }
    ledger
        .entries
        .iter()
        .rev()
        .take(max_items)
        .map(|record| CoreRevisionTimelineEntry {
            based_on_revision: record.based_on_revision,
            resulting_revision: record.resulting_revision,
            outcome: record.outcome,
            reviewed_at: record.reviewed_at,
            observation_due_at: record.observation_due_at,
            correction_kind: record.correction_kind,
            corrects_revision: record.corrects_revision,
            adjudication_reason: record.adjudication_reason.clone(),
            change_summary: record
                .accepted_changes
                .iter()
                .chain(record.rejected_changes.iter())
                .map(|change| change.summary.clone())
                .take(CORE_REVISION_TIMELINE_RENDER_LIMIT)
                .collect(),
            conflict_classes: record.conflict_classes.clone(),
        })
        .collect()
}

pub fn render_core_revision_governance_block(
    ledger: &CoreRevisionLedger,
    digest: &CoreRevisionGovernanceDigest,
    now_secs: u64,
    max_len: usize,
) -> Option<String> {
    if max_len < 120 || !ledger.is_meaningful() {
        return None;
    }
    let mut out = String::with_capacity(max_len.min(1024));
    out.push_str("## Core Revision Governance\n");
    out.push_str(
        "Board-level constitutional governance summary. Use this to understand whether the core is steady, under observation, due for review, or correcting drift.\n",
    );
    let _ = writeln!(
        out,
        "Latest stability: {} | review_due={} | conservative_mode={}",
        digest.latest_stability_score, digest.review_due, digest.conservative_mode
    );
    if digest.last_adopted_revision > 0 {
        let _ = writeln!(
            out,
            "Latest adopted revision: {} at {}",
            digest.last_adopted_revision, digest.last_adopted_at
        );
    }
    if digest.observation_active {
        let remaining = digest.observation_due_at.saturating_sub(now_secs);
        let _ = writeln!(
            out,
            "Observation: revision {} under observation until {} (remaining {}s)",
            digest.observation_revision, digest.observation_due_at, remaining
        );
    }
    if !digest.review_reasons.is_empty() {
        let _ = writeln!(out, "Review reasons: {}", digest.review_reasons.join(", "));
    }
    if !digest.pressure_conflicts.is_empty() {
        let _ = writeln!(
            out,
            "Conflict pressure: {}",
            digest
                .pressure_conflicts
                .iter()
                .map(|class| class.label())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    out.push_str("Recent timeline:\n");
    for entry in build_core_revision_timeline(ledger, CORE_REVISION_TIMELINE_RENDER_LIMIT) {
        let _ = write!(
            out,
            "- rev {} {}",
            entry.resulting_revision,
            entry.outcome.label()
        );
        if entry.based_on_revision > 0 {
            let _ = write!(out, " based_on={}", entry.based_on_revision);
        }
        let _ = write!(out, " at={}", entry.reviewed_at);
        if let Some(kind) = entry.correction_kind {
            let _ = write!(
                out,
                " {}={}",
                kind.label(),
                entry.corrects_revision.unwrap_or(0)
            );
        }
        if entry.observation_due_at > entry.reviewed_at {
            let _ = write!(out, " observe_until={}", entry.observation_due_at);
        }
        if !entry.adjudication_reason.trim().is_empty() {
            let _ = write!(out, " reason={}", entry.adjudication_reason.trim());
        }
        out.push('\n');
        if !entry.change_summary.is_empty() {
            let _ = writeln!(out, "  changes: {}", entry.change_summary.join(" | "));
        }
        if !entry.conflict_classes.is_empty() {
            let _ = writeln!(
                out,
                "  conflicts: {}",
                entry
                    .conflict_classes
                    .iter()
                    .map(|class| class.label())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
    }
    let rendered = truncate_content_to_max(out.trim_end(), max_len).into_owned();
    (!rendered.trim().is_empty()).then_some(rendered)
}

fn maybe_fill_review_schedule(
    digest: &mut CoreRevisionGovernanceDigest,
    last_reviewed_at: u64,
    stability_score: u8,
    now_secs: u64,
    latest_record: Option<&CoreRevisionRecord>,
) {
    let cadence_secs = review_cadence_secs(stability_score);
    let followup_secs = followup_review_secs(stability_score);
    if last_reviewed_at == 0 {
        digest
            .review_reasons
            .push("constitution_never_reviewed".to_string());
    } else if now_secs > 0 && now_secs.saturating_sub(last_reviewed_at) >= cadence_secs {
        digest
            .review_reasons
            .push("constitutional_review_cadence_due".to_string());
    }
    if let Some(record) = latest_record.filter(|record| {
        matches!(record.outcome, CoreRevisionOutcome::Adopted) && record.reviewed_at > 0
    }) {
        if now_secs > 0 && now_secs.saturating_sub(record.reviewed_at) >= followup_secs {
            digest
                .review_reasons
                .push("post_adoption_follow_up_due".to_string());
        }
    }
    if digest.repeated_rejected_direction_count >= CORE_REVISION_REJECTION_REPEAT_THRESHOLD {
        digest
            .review_reasons
            .push("repeated_rejected_directions".to_string());
    }
    if digest.recent_correction_count > 0 || digest.contradiction_count > 0 {
        digest
            .review_reasons
            .push("correction_pressure".to_string());
    }
    if stability_score > 0 && stability_score < CORE_REVISION_LOW_STABILITY_THRESHOLD {
        digest
            .review_reasons
            .push("low_constitutional_stability".to_string());
    }
    digest.review_reasons.dedup();
    digest.review_due = !digest.review_reasons.is_empty();
}

fn review_cadence_secs(stability_score: u8) -> u64 {
    if stability_score >= 80 {
        CORE_REVISION_CADENCE_STEADY_SECS
    } else if stability_score >= CORE_REVISION_CONSERVATIVE_STABILITY_THRESHOLD {
        CORE_REVISION_CADENCE_ACTIVE_SECS
    } else {
        CORE_REVISION_CADENCE_UNSTABLE_SECS
    }
}

fn followup_review_secs(stability_score: u8) -> u64 {
    if stability_score >= 80 {
        CORE_REVISION_FOLLOWUP_STEADY_SECS
    } else if stability_score >= CORE_REVISION_CONSERVATIVE_STABILITY_THRESHOLD {
        CORE_REVISION_FOLLOWUP_ACTIVE_SECS
    } else {
        CORE_REVISION_FOLLOWUP_UNSTABLE_SECS
    }
}

fn recent_records(ledger: &CoreRevisionLedger) -> impl Iterator<Item = &CoreRevisionRecord> {
    ledger
        .entries
        .iter()
        .rev()
        .take(CORE_REVISION_GOVERNANCE_WINDOW)
}

fn change_matches(
    existing: &CoreRevisionRecordChange,
    incoming: &CoreRevisionRecordChange,
) -> bool {
    existing.kind == incoming.kind
        && canonical_change_summary(&existing.summary)
            == canonical_change_summary(&incoming.summary)
}

fn canonical_change_summary(summary: &str) -> String {
    summary
        .trim()
        .split(" [")
        .next()
        .unwrap_or(summary)
        .trim()
        .to_ascii_lowercase()
}

fn normalize_record(record: &mut CoreRevisionRecord) {
    record.relationship_scope_id = record.relationship_scope_id.trim().to_string();
    normalize_text_list(
        &mut record.source_layers,
        CORE_REVISION_LEDGER_RENDER_LIMIT * 2,
        CORE_REVISION_LEDGER_CHANGE_MAX_CHARS,
    );
    normalize_text_list(
        &mut record.evidence_summary,
        CORE_REVISION_LEDGER_RENDER_LIMIT,
        CORE_REVISION_LEDGER_REASON_MAX_CHARS,
    );
    normalize_text_list(
        &mut record.counterevidence,
        CORE_REVISION_LEDGER_RENDER_LIMIT,
        CORE_REVISION_LEDGER_REASON_MAX_CHARS,
    );
    normalize_change_list(&mut record.accepted_changes);
    normalize_change_list(&mut record.rejected_changes);
    normalize_conflict_classes(&mut record.conflict_classes);
    record.adjudication_reason = truncate_content_to_max(
        record.adjudication_reason.trim(),
        CORE_REVISION_LEDGER_REASON_MAX_CHARS,
    )
    .into_owned();
    record.rationale = truncate_content_to_max(
        record.rationale.trim(),
        CORE_REVISION_LEDGER_REASON_MAX_CHARS,
    )
    .into_owned();
    if record.observation_due_at > 0 && record.observation_due_at < record.reviewed_at {
        record.observation_due_at = record.reviewed_at;
    }
}

fn normalize_text_list(values: &mut Vec<String>, limit: usize, max_chars: usize) {
    let mut normalized = Vec::with_capacity(limit);
    for value in values.drain(..) {
        let value = truncate_content_to_max(value.trim(), max_chars)
            .trim()
            .to_string();
        if value.is_empty() || normalized.iter().any(|existing| existing == &value) {
            continue;
        }
        normalized.push(value);
        if normalized.len() >= limit {
            break;
        }
    }
    *values = normalized;
}

fn normalize_change_list(values: &mut Vec<CoreRevisionRecordChange>) {
    let mut normalized =
        Vec::with_capacity(values.len().min(CORE_REVISION_LEDGER_RENDER_LIMIT * 2));
    for mut value in values.drain(..) {
        value.summary =
            truncate_content_to_max(value.summary.trim(), CORE_REVISION_LEDGER_CHANGE_MAX_CHARS)
                .trim()
                .to_string();
        if value.kind == CoreRevisionActionKind::Noop && value.summary.is_empty() {
            continue;
        }
        if normalized
            .iter()
            .any(|existing: &CoreRevisionRecordChange| {
                existing.kind == value.kind && existing.summary == value.summary
            })
        {
            continue;
        }
        normalized.push(value);
        if normalized.len() >= CORE_REVISION_LEDGER_RENDER_LIMIT * 2 {
            break;
        }
    }
    *values = normalized;
}

fn normalize_conflict_classes(values: &mut Vec<CoreRevisionConflictClass>) {
    values.sort_unstable();
    values.dedup();
    values.truncate(CORE_REVISION_LEDGER_RENDER_LIMIT);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn governance_digest_marks_review_due_for_corrections_and_low_stability() {
        let ledger = CoreRevisionLedger {
            entries: vec![
                CoreRevisionRecord {
                    outcome: CoreRevisionOutcome::Adopted,
                    resulting_revision: 3,
                    stability_score: 72,
                    reviewed_at: 100,
                    observation_due_at: 112,
                    accepted_changes: vec![CoreRevisionRecordChange {
                        kind: CoreRevisionActionKind::ReviseBoundaryDoctrine,
                        summary: "revise_boundary_doctrine: guarded".to_string(),
                    }],
                    ..CoreRevisionRecord::default()
                },
                CoreRevisionRecord {
                    outcome: CoreRevisionOutcome::Adopted,
                    based_on_revision: 3,
                    resulting_revision: 4,
                    stability_score: 48,
                    reviewed_at: 200,
                    accepted_changes: vec![CoreRevisionRecordChange {
                        kind: CoreRevisionActionKind::ReviseBoundaryDoctrine,
                        summary: "revise_boundary_doctrine: sealed".to_string(),
                    }],
                    conflict_classes: vec![CoreRevisionConflictClass::ContradictedAdoption],
                    corrects_revision: Some(3),
                    correction_kind: Some(CoreRevisionCorrectionKind::Rollback),
                    ..CoreRevisionRecord::default()
                },
            ],
            updated_at: 200,
        };

        let digest = compute_core_revision_governance_digest(Some(&ledger), 200, 48, 300);

        assert!(digest.review_due);
        assert!(digest.conservative_mode);
        assert_eq!(digest.observation_revision, 4);
        assert!(digest.observation_active);
        assert_eq!(digest.recent_correction_count, 1);
        assert_eq!(digest.contradiction_count, 1);
        assert!(digest
            .review_reasons
            .contains(&"correction_pressure".to_string()));
        assert!(digest
            .review_reasons
            .contains(&"low_constitutional_stability".to_string()));
    }

    #[test]
    fn change_match_ignores_rejection_reason_suffix() {
        let ledger = CoreRevisionLedger {
            entries: vec![CoreRevisionRecord {
                outcome: CoreRevisionOutcome::Rejected,
                rejected_changes: vec![CoreRevisionRecordChange {
                    kind: CoreRevisionActionKind::ReviseTruthDoctrine,
                    summary: "revise_truth_doctrine: keep it plain [recent_rejected_direction]"
                        .to_string(),
                }],
                reviewed_at: 10,
                ..CoreRevisionRecord::default()
            }],
            updated_at: 10,
        };

        assert!(has_recent_matching_rejected_change(
            &ledger,
            &CoreRevisionRecordChange {
                kind: CoreRevisionActionKind::ReviseTruthDoctrine,
                summary: "revise_truth_doctrine: keep it plain".to_string(),
            }
        ));
    }

    #[test]
    fn deferred_reviews_do_not_count_as_rejections() {
        let ledger = CoreRevisionLedger {
            entries: vec![
                CoreRevisionRecord {
                    outcome: CoreRevisionOutcome::Deferred,
                    adjudication_reason: "llm_no_change".to_string(),
                    reviewed_at: 10,
                    ..CoreRevisionRecord::default()
                },
                CoreRevisionRecord {
                    outcome: CoreRevisionOutcome::Rejected,
                    rejected_changes: vec![CoreRevisionRecordChange {
                        kind: CoreRevisionActionKind::ReviseTruthDoctrine,
                        summary: "revise_truth_doctrine: speak plainly".to_string(),
                    }],
                    reviewed_at: 20,
                    ..CoreRevisionRecord::default()
                },
            ],
            updated_at: 20,
        };

        let digest = compute_core_revision_governance_digest(Some(&ledger), 20, 72, 30);

        assert_eq!(digest.recent_rejection_count, 1);
        assert_eq!(digest.repeated_rejected_direction_count, 1);
    }

    #[test]
    fn render_core_revision_governance_block_includes_timeline_and_observation() {
        let ledger = CoreRevisionLedger {
            entries: vec![CoreRevisionRecord {
                outcome: CoreRevisionOutcome::Adopted,
                based_on_revision: 3,
                resulting_revision: 4,
                stability_score: 72,
                reviewed_at: 100,
                observation_due_at: 140,
                adjudication_reason: "adopted_board_revision".to_string(),
                accepted_changes: vec![CoreRevisionRecordChange {
                    kind: CoreRevisionActionKind::ReviseBoundaryDoctrine,
                    summary: "revise boundary doctrine".to_string(),
                }],
                ..CoreRevisionRecord::default()
            }],
            updated_at: 100,
        };

        let digest = compute_core_revision_governance_digest(Some(&ledger), 100, 72, 120);
        let block =
            render_core_revision_governance_block(&ledger, &digest, 120, 720).expect("block");

        assert!(block.contains("## Core Revision Governance"));
        assert!(block.contains("Observation: revision 4"));
        assert!(block.contains("Recent timeline:"));
        assert!(block.contains("rev 4 adopted"));
        assert!(block.contains("changes: revise boundary doctrine"));
    }

    #[test]
    fn embedded_core_revision_ledger_compaction_keeps_governance_signals_and_drops_heavy_text() {
        let mut entries = vec![CoreRevisionRecord {
            outcome: CoreRevisionOutcome::Adopted,
            based_on_revision: 0,
            resulting_revision: 1,
            source_layers: vec![
                "self_model".to_string(),
                "recent_persona_evidence".to_string(),
            ],
            evidence_summary: vec!["x".repeat(240)],
            counterevidence: vec!["y".repeat(240)],
            accepted_changes: vec![CoreRevisionRecordChange {
                kind: CoreRevisionActionKind::ReviseIdentityAnchor,
                summary: "identity anchor adopted from stable evidence".to_string(),
            }],
            observation_due_at: 1_000,
            adjudication_reason: "adopted_board_revision".to_string(),
            rationale: "z".repeat(240),
            stability_score: 72,
            reviewed_at: 10,
            ..CoreRevisionRecord::default()
        }];
        for idx in 2..=9 {
            entries.push(CoreRevisionRecord {
                outcome: if idx % 2 == 0 {
                    CoreRevisionOutcome::Deferred
                } else {
                    CoreRevisionOutcome::Rejected
                },
                resulting_revision: idx,
                source_layers: vec!["recent_transcript".to_string()],
                evidence_summary: vec!["heavy evidence".repeat(20)],
                counterevidence: vec!["heavy counterevidence".repeat(20)],
                rejected_changes: vec![CoreRevisionRecordChange {
                    kind: CoreRevisionActionKind::ReviseTruthDoctrine,
                    summary: "truth doctrine rejected because signal stayed local".repeat(4),
                }],
                adjudication_reason: "gate blocked noisy local signal".repeat(4),
                rationale: "heavy rationale".repeat(20),
                stability_score: 68,
                reviewed_at: idx * 10,
                ..CoreRevisionRecord::default()
            });
        }
        let ledger = CoreRevisionLedger {
            entries,
            updated_at: 90,
        };

        let compacted = compact_core_revision_ledger_for_profile(
            ledger,
            crate::memory::MemoryProfile::Embedded,
        );

        assert!(compacted.entries.len() <= 7);
        assert!(compacted
            .entries
            .iter()
            .any(|entry| entry.outcome == CoreRevisionOutcome::Adopted
                && entry.resulting_revision == 1
                && entry.observation_due_at == 1_000));
        assert!(compacted.entries.iter().all(|entry| {
            entry.source_layers.is_empty()
                && entry.evidence_summary.is_empty()
                && entry.counterevidence.is_empty()
                && entry.rationale.is_empty()
                && entry.adjudication_reason.len() <= 80
        }));
        assert!(compacted.entries.iter().all(|entry| {
            entry
                .accepted_changes
                .iter()
                .chain(entry.rejected_changes.iter())
                .all(|change| change.summary.len() <= 64)
        }));
    }

    #[test]
    fn standard_core_revision_ledger_compaction_keeps_full_ledger() {
        let ledger = CoreRevisionLedger {
            entries: vec![CoreRevisionRecord {
                outcome: CoreRevisionOutcome::Rejected,
                source_layers: vec!["private_workspace".to_string()],
                evidence_summary: vec!["full evidence should remain on LinuxFull".to_string()],
                counterevidence: vec!["full counterevidence should remain on LinuxFull".to_string()],
                rejected_changes: vec![CoreRevisionRecordChange {
                    kind: CoreRevisionActionKind::ReviseRepairDoctrine,
                    summary: "full rejected change summary should remain".to_string(),
                }],
                adjudication_reason: "full adjudication reason should remain".to_string(),
                rationale: "full rationale should remain".to_string(),
                reviewed_at: 10,
                ..CoreRevisionRecord::default()
            }],
            updated_at: 10,
        };

        let compacted = compact_core_revision_ledger_for_profile(
            ledger.clone(),
            crate::memory::MemoryProfile::Standard,
        );

        assert_eq!(compacted, ledger);
    }
}
