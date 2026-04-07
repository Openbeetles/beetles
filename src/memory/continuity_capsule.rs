//! Continuity capsule plane: compact work-continuation contracts.
//! 连续性 capsule 平面：保存“做到哪、为什么停、下一步是什么”的紧凑工作续接合同。

use crate::error::Result;
use crate::util::truncate_content_to_max;
use serde::{Deserialize, Serialize};
use std::cmp::Reverse;
use std::collections::{hash_map::DefaultHasher, HashMap};
use std::fmt::Write as _;
use std::hash::{Hash, Hasher};

use super::{
    RecallCandidate, RecallPlane, RecallQuery, RecallScoreBreakdown, RecallSelectionReport,
    SessionMessage,
};

pub const REL_PATH_CONTINUITY_CAPSULES: &str = "memory/continuity_capsules.json";
pub const MAX_CONTINUITY_CAPSULES: usize = 96;
pub const MAX_CONTINUITY_CAPSULES_PER_SCOPE: usize = 16;
pub const MAX_CONTINUITY_CAPSULE_BLOCK_LEN: usize = 900;

const MAX_CONTINUITY_CAPSULE_TOPIC_CHARS: usize = 96;
const MAX_CONTINUITY_CAPSULE_SUMMARY_CHARS: usize = 220;
const MAX_CONTINUITY_CAPSULE_OUTCOME_CHARS: usize = 180;
const MAX_CONTINUITY_CAPSULE_NEXT_STEP_CHARS: usize = 180;
const MAX_CONTINUITY_CAPSULE_REF_CHARS: usize = 96;
const MAX_CONTINUITY_CAPSULE_LIST_ITEMS: usize = 4;
const MAX_CONTINUITY_CAPSULE_RECALL_CANDIDATES: usize = 12;
const MAX_CONTINUITY_CAPSULE_RECALL_SELECTED: usize = 3;
const CONTINUITY_CAPSULE_STALE_AFTER_SECS: u64 = 14 * 24 * 60 * 60;

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ContinuityCapsuleKind {
    TaskResolution,
    WorkSession,
    DeviceIncident,
    RelationshipEvent,
    #[default]
    HandoffState,
}

impl ContinuityCapsuleKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::TaskResolution => "task_resolution",
            Self::WorkSession => "work_session",
            Self::DeviceIncident => "device_incident",
            Self::RelationshipEvent => "relationship_event",
            Self::HandoffState => "handoff_state",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ContinuityCapsuleScopeKind {
    Board,
    Relationship,
    #[default]
    Chat,
}

impl ContinuityCapsuleScopeKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Board => "board",
            Self::Relationship => "relationship",
            Self::Chat => "chat",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ContinuityCapsuleStatus {
    #[default]
    Active,
    Done,
    Stale,
    Superseded,
}

impl ContinuityCapsuleStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Done => "done",
            Self::Stale => "stale",
            Self::Superseded => "superseded",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ContinuityCapsuleSource {
    #[default]
    PostReplyMaintenance,
    TaskCompletion,
    BoundaryFlush,
    HandoffFlush,
    RebootContinuity,
}

impl ContinuityCapsuleSource {
    pub fn label(self) -> &'static str {
        match self {
            Self::PostReplyMaintenance => "post_reply_maintenance",
            Self::TaskCompletion => "task_completion",
            Self::BoundaryFlush => "boundary_flush",
            Self::HandoffFlush => "handoff_flush",
            Self::RebootContinuity => "reboot_continuity",
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContinuityCapsuleDraft {
    pub kind: ContinuityCapsuleKind,
    pub scope_kind: ContinuityCapsuleScopeKind,
    #[serde(default)]
    pub scope_id: String,
    #[serde(default)]
    pub source_chat_id: String,
    #[serde(default)]
    pub source_channel: String,
    #[serde(default)]
    pub run_id: String,
    #[serde(default)]
    pub topic: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub outcome: String,
    #[serde(default)]
    pub decisions: Vec<String>,
    #[serde(default)]
    pub next_step: String,
    #[serde(default)]
    pub unresolved: Vec<String>,
    #[serde(default)]
    pub artifact_refs: Vec<String>,
    #[serde(default)]
    pub provenance_refs: Vec<String>,
    #[serde(default)]
    pub source: ContinuityCapsuleSource,
    #[serde(default)]
    pub status: ContinuityCapsuleStatus,
    #[serde(default)]
    pub observed_at: u64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContinuityCapsule {
    pub capsule_id: String,
    pub kind: ContinuityCapsuleKind,
    pub scope_kind: ContinuityCapsuleScopeKind,
    #[serde(default)]
    pub scope_id: String,
    #[serde(default)]
    pub source_chat_id: String,
    #[serde(default)]
    pub source_channel: String,
    #[serde(default)]
    pub run_id: String,
    #[serde(default)]
    pub topic: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub outcome: String,
    #[serde(default)]
    pub decisions: Vec<String>,
    #[serde(default)]
    pub next_step: String,
    #[serde(default)]
    pub unresolved: Vec<String>,
    #[serde(default)]
    pub artifact_refs: Vec<String>,
    #[serde(default)]
    pub provenance_refs: Vec<String>,
    #[serde(default)]
    pub source: ContinuityCapsuleSource,
    #[serde(default)]
    pub status: ContinuityCapsuleStatus,
    #[serde(default)]
    pub supersedes: Vec<String>,
    #[serde(default)]
    pub observed_at: u64,
    #[serde(default)]
    pub updated_at: u64,
}

impl ContinuityCapsule {
    pub fn is_meaningful(&self) -> bool {
        !self.topic.trim().is_empty()
            && (!self.summary.trim().is_empty()
                || !self.outcome.trim().is_empty()
                || !self.next_step.trim().is_empty()
                || !self.decisions.is_empty()
                || !self.unresolved.is_empty())
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ContinuityCapsuleWriteOutcome {
    pub considered: usize,
    pub upserted: usize,
    pub superseded: usize,
    pub total: usize,
}

pub trait ContinuityCapsuleStore: Send + Sync {
    fn upsert_many(
        &self,
        drafts: &[ContinuityCapsuleDraft],
        now_secs: u64,
    ) -> Result<ContinuityCapsuleWriteOutcome>;
    fn get(&self, capsule_id: &str) -> Result<Option<ContinuityCapsule>>;
    fn list(&self, limit: usize) -> Result<Vec<ContinuityCapsule>>;
    fn count(&self) -> Result<usize>;

    fn list_for_scope(
        &self,
        scope_kind: ContinuityCapsuleScopeKind,
        scope_id: &str,
        limit: usize,
    ) -> Result<Vec<ContinuityCapsule>> {
        let normalized_scope_id = normalize_inline(scope_id, MAX_CONTINUITY_CAPSULE_TOPIC_CHARS);
        if normalized_scope_id.is_empty() {
            return Ok(Vec::new());
        }
        let mut items = self.list(MAX_CONTINUITY_CAPSULES)?;
        items.retain(|capsule| {
            capsule.scope_kind == scope_kind && capsule.scope_id == normalized_scope_id
        });
        items.sort_by_key(|capsule| Reverse(capsule.updated_at));
        items.truncate(limit.max(1));
        Ok(items)
    }
}

pub(crate) fn apply_continuity_capsule_drafts(
    entries: &mut Vec<ContinuityCapsule>,
    drafts: &[ContinuityCapsuleDraft],
    now_secs: u64,
) -> ContinuityCapsuleWriteOutcome {
    let mut outcome = ContinuityCapsuleWriteOutcome {
        considered: drafts.len(),
        ..ContinuityCapsuleWriteOutcome::default()
    };

    let mut changed = false;
    for draft in drafts {
        let Some(mut next) = continuity_capsule_from_draft(draft, now_secs) else {
            continue;
        };

        for existing in entries.iter_mut() {
            if existing.capsule_id == next.capsule_id {
                changed |= merge_continuity_capsule(existing, &next);
                next = existing.clone();
                break;
            }
        }

        for existing in entries.iter_mut() {
            if existing.capsule_id == next.capsule_id {
                continue;
            }
            if !continuity_capsules_conflict(existing, &next) {
                continue;
            }
            if existing.status != ContinuityCapsuleStatus::Superseded {
                existing.status = ContinuityCapsuleStatus::Superseded;
                existing.updated_at = now_secs.max(existing.updated_at);
                changed = true;
                outcome.superseded = outcome.superseded.saturating_add(1);
            }
            if !next
                .supersedes
                .iter()
                .any(|capsule_id| capsule_id == &existing.capsule_id)
            {
                next.supersedes.push(existing.capsule_id.clone());
            }
        }

        if let Some(existing) = entries
            .iter_mut()
            .find(|existing| existing.capsule_id == next.capsule_id)
        {
            changed |= merge_continuity_capsule(existing, &next);
            outcome.upserted = outcome.upserted.saturating_add(1);
        } else {
            entries.push(next);
            changed = true;
            outcome.upserted = outcome.upserted.saturating_add(1);
        }
    }

    changed |= govern_continuity_capsules(entries, now_secs);
    if changed {
        entries.sort_by_key(|capsule| Reverse(capsule.updated_at));
    }
    outcome.total = entries.len();
    outcome
}

pub fn render_continuity_capsule_block(
    capsules: &[ContinuityCapsule],
    max_len: usize,
) -> Option<String> {
    if capsules.is_empty() {
        return None;
    }
    let mut out = String::with_capacity(max_len.min(MAX_CONTINUITY_CAPSULE_BLOCK_LEN));
    out.push_str("## Continuity Capsules\n");
    out.push_str("Compact continuation contracts for recent work and handoff state.\n");
    for capsule in capsules.iter().take(MAX_CONTINUITY_CAPSULE_RECALL_SELECTED) {
        let _ = writeln!(
            out,
            "- [{}|{}] {}",
            capsule.kind.label(),
            capsule.status.label(),
            capsule.topic
        );
        if !capsule.summary.is_empty() {
            let _ = writeln!(out, "  Summary: {}", capsule.summary);
        }
        if !capsule.outcome.is_empty() {
            let _ = writeln!(out, "  Outcome: {}", capsule.outcome);
        }
        if !capsule.next_step.is_empty() {
            let _ = writeln!(out, "  Next: {}", capsule.next_step);
        }
        if !capsule.unresolved.is_empty() {
            let _ = writeln!(out, "  Unresolved: {}", capsule.unresolved.join(" | "));
        }
        if !capsule.provenance_refs.is_empty() {
            let _ = writeln!(out, "  Provenance: {}", capsule.provenance_refs.join(", "));
        }
    }
    let capped = truncate_content_to_max(out.trim_end(), max_len).into_owned();
    (!capped.trim().is_empty()).then_some(capped)
}

pub fn inspect_continuity_capsule_recall(
    store: &dyn ContinuityCapsuleStore,
    scope_kind: ContinuityCapsuleScopeKind,
    scope_id: &str,
    preferred_chat_id: Option<&str>,
    query: &str,
    summary_text: Option<&str>,
    recent_messages: &[SessionMessage],
    max_chars: usize,
    now_secs: u64,
) -> (RecallSelectionReport, Vec<ContinuityCapsule>) {
    let normalized_scope_id = normalize_inline(scope_id, MAX_CONTINUITY_CAPSULE_TOPIC_CHARS);
    let normalized_query = normalize_match_text(query);
    let terms = collect_terms(&normalized_query);
    let mut all = store.list(MAX_CONTINUITY_CAPSULES).unwrap_or_default();
    all.retain(|capsule| {
        capsule.scope_kind == scope_kind && capsule.scope_id == normalized_scope_id
    });

    let mut scored = all
        .into_iter()
        .filter_map(|capsule| {
            score_continuity_capsule(
                &capsule,
                preferred_chat_id,
                &normalized_query,
                &terms,
                now_secs,
            )
            .map(|(score, reasons)| (score, reasons, capsule))
        })
        .collect::<Vec<_>>();
    scored.sort_by(|left, right| {
        right
            .0
            .cmp(&left.0)
            .then_with(|| right.2.updated_at.cmp(&left.2.updated_at))
    });

    let selected = scored
        .iter()
        .take(MAX_CONTINUITY_CAPSULE_RECALL_SELECTED)
        .map(|(_, _, capsule)| capsule.clone())
        .collect::<Vec<_>>();
    let selected_ids = selected
        .iter()
        .map(|capsule| capsule.capsule_id.clone())
        .collect::<Vec<_>>();
    let query = RecallQuery {
        plane: RecallPlane::ContinuityCapsule,
        raw_query: query.trim().to_string(),
        normalized_query: truncate_content_to_max(&normalized_query, 240)
            .trim()
            .to_string(),
        preferred_chat_id: preferred_chat_id.map(str::to_string),
        current_channel: None,
        summary_text: summary_text
            .map(|value| {
                truncate_content_to_max(value.trim(), 220)
                    .trim()
                    .to_string()
            })
            .filter(|value| !value.is_empty()),
        recent_grounding: recent_messages
            .iter()
            .rev()
            .take(2)
            .filter_map(|message| {
                let content = truncate_content_to_max(message.content.trim(), 120)
                    .trim()
                    .to_string();
                (!content.is_empty()).then(|| format!("{}: {}", message.role, content))
            })
            .collect(),
        active_run_id: None,
        exact_lookup: None,
        requested_limit: MAX_CONTINUITY_CAPSULE_RECALL_SELECTED,
        max_chars,
        notes: vec![format!(
            "scope={}{}",
            scope_kind.label(),
            if normalized_scope_id.is_empty() {
                String::new()
            } else {
                format!(":{normalized_scope_id}")
            }
        )],
    };
    let report = RecallSelectionReport {
        plane: RecallPlane::ContinuityCapsule,
        query,
        backend: "continuity_capsule_heuristic".to_string(),
        candidate_count: scored.len(),
        selected_count: selected.len(),
        selected_ids,
        miss_reason: scored
            .is_empty()
            .then(|| "no_continuity_capsule_candidates".to_string()),
        selection_note: (!selected.is_empty())
            .then(|| "recent_work_continuity_contracts".to_string()),
        candidates: scored
            .into_iter()
            .take(MAX_CONTINUITY_CAPSULE_RECALL_CANDIDATES)
            .map(|(score, reasons, capsule)| {
                let selected_hit = selected
                    .iter()
                    .any(|selected_capsule| selected_capsule.capsule_id == capsule.capsule_id);
                build_continuity_capsule_candidate(capsule, selected_hit, score, reasons)
            })
            .collect(),
    };
    (report, selected)
}

pub(crate) fn continuity_capsule_from_draft(
    draft: &ContinuityCapsuleDraft,
    now_secs: u64,
) -> Option<ContinuityCapsule> {
    let normalized = normalize_continuity_capsule_draft(draft.clone())?;
    let capsule_id = continuity_capsule_stable_id(&normalized)?;
    Some(ContinuityCapsule {
        capsule_id,
        kind: normalized.kind,
        scope_kind: normalized.scope_kind,
        scope_id: normalized.scope_id,
        source_chat_id: normalized.source_chat_id,
        source_channel: normalized.source_channel,
        run_id: normalized.run_id,
        topic: normalized.topic,
        summary: normalized.summary,
        outcome: normalized.outcome,
        decisions: normalized.decisions,
        next_step: normalized.next_step,
        unresolved: normalized.unresolved,
        artifact_refs: normalized.artifact_refs,
        provenance_refs: normalized.provenance_refs,
        source: normalized.source,
        status: normalized.status,
        supersedes: Vec::new(),
        observed_at: normalized.observed_at.max(1),
        updated_at: now_secs.max(normalized.observed_at.max(1)),
    })
}

pub(crate) fn canonicalize_continuity_capsule(
    capsule: ContinuityCapsule,
) -> Option<ContinuityCapsule> {
    let mut normalized = normalize_continuity_capsule(capsule)?;
    if normalized.updated_at == 0 {
        normalized.updated_at = normalized.observed_at.max(1);
    }
    if normalized.observed_at == 0 {
        normalized.observed_at = normalized.updated_at.max(1);
    }
    Some(normalized)
}

fn normalize_continuity_capsule_draft(
    mut draft: ContinuityCapsuleDraft,
) -> Option<ContinuityCapsuleDraft> {
    draft.scope_id = normalize_inline(&draft.scope_id, MAX_CONTINUITY_CAPSULE_TOPIC_CHARS);
    draft.source_chat_id =
        normalize_inline(&draft.source_chat_id, MAX_CONTINUITY_CAPSULE_TOPIC_CHARS);
    draft.source_channel = normalize_inline(&draft.source_channel, 32);
    draft.run_id = normalize_inline(&draft.run_id, 32);
    draft.topic = normalize_inline(&draft.topic, MAX_CONTINUITY_CAPSULE_TOPIC_CHARS);
    draft.summary = normalize_multiline(&draft.summary, MAX_CONTINUITY_CAPSULE_SUMMARY_CHARS);
    draft.outcome = normalize_multiline(&draft.outcome, MAX_CONTINUITY_CAPSULE_OUTCOME_CHARS);
    draft.next_step = normalize_multiline(&draft.next_step, MAX_CONTINUITY_CAPSULE_NEXT_STEP_CHARS);
    draft.decisions = normalize_ref_list(&draft.decisions);
    draft.unresolved = normalize_ref_list(&draft.unresolved);
    draft.artifact_refs = normalize_compact_refs(&draft.artifact_refs);
    draft.provenance_refs = normalize_compact_refs(&draft.provenance_refs);
    if draft.scope_id.is_empty() || draft.topic.is_empty() {
        return None;
    }
    let meaningful = !draft.summary.is_empty()
        || !draft.outcome.is_empty()
        || !draft.next_step.is_empty()
        || !draft.decisions.is_empty()
        || !draft.unresolved.is_empty();
    meaningful.then_some(draft)
}

fn normalize_continuity_capsule(mut capsule: ContinuityCapsule) -> Option<ContinuityCapsule> {
    capsule.scope_id = normalize_inline(&capsule.scope_id, MAX_CONTINUITY_CAPSULE_TOPIC_CHARS);
    capsule.source_chat_id =
        normalize_inline(&capsule.source_chat_id, MAX_CONTINUITY_CAPSULE_TOPIC_CHARS);
    capsule.source_channel = normalize_inline(&capsule.source_channel, 32);
    capsule.run_id = normalize_inline(&capsule.run_id, 32);
    capsule.topic = normalize_inline(&capsule.topic, MAX_CONTINUITY_CAPSULE_TOPIC_CHARS);
    capsule.summary = normalize_multiline(&capsule.summary, MAX_CONTINUITY_CAPSULE_SUMMARY_CHARS);
    capsule.outcome = normalize_multiline(&capsule.outcome, MAX_CONTINUITY_CAPSULE_OUTCOME_CHARS);
    capsule.next_step =
        normalize_multiline(&capsule.next_step, MAX_CONTINUITY_CAPSULE_NEXT_STEP_CHARS);
    capsule.decisions = normalize_ref_list(&capsule.decisions);
    capsule.unresolved = normalize_ref_list(&capsule.unresolved);
    capsule.artifact_refs = normalize_compact_refs(&capsule.artifact_refs);
    capsule.provenance_refs = normalize_compact_refs(&capsule.provenance_refs);
    capsule.supersedes = normalize_compact_refs(&capsule.supersedes);
    if capsule.scope_id.is_empty() || capsule.topic.is_empty() {
        return None;
    }
    let meaningful = !capsule.summary.is_empty()
        || !capsule.outcome.is_empty()
        || !capsule.next_step.is_empty()
        || !capsule.decisions.is_empty()
        || !capsule.unresolved.is_empty();
    meaningful.then_some(capsule)
}

fn continuity_capsule_stable_id(draft: &ContinuityCapsuleDraft) -> Option<String> {
    if draft.scope_id.is_empty() {
        return None;
    }
    let mut hasher = DefaultHasher::new();
    draft.scope_kind.hash(&mut hasher);
    0x6133_d4ab_u32.hash(&mut hasher);
    draft.scope_id.hash(&mut hasher);
    if !draft.run_id.is_empty() {
        draft.run_id.hash(&mut hasher);
    } else {
        normalize_match_text(&draft.topic).hash(&mut hasher);
    }
    let mut id = String::with_capacity(24);
    id.push_str("cc-");
    id.push_str(&format!("{:016x}", hasher.finish()));
    Some(id)
}

fn merge_continuity_capsule(existing: &mut ContinuityCapsule, next: &ContinuityCapsule) -> bool {
    let mut changed = false;
    macro_rules! replace_field {
        ($field:ident) => {
            if existing.$field != next.$field {
                existing.$field = next.$field.clone();
                changed = true;
            }
        };
    }
    if existing.kind != next.kind {
        existing.kind = next.kind;
        changed = true;
    }
    if existing.scope_kind != next.scope_kind {
        existing.scope_kind = next.scope_kind;
        changed = true;
    }
    if existing.source != next.source {
        existing.source = next.source;
        changed = true;
    }
    if existing.status != next.status {
        existing.status = next.status;
        changed = true;
    }
    replace_field!(scope_id);
    replace_field!(source_chat_id);
    replace_field!(source_channel);
    replace_field!(run_id);
    replace_field!(topic);
    replace_field!(summary);
    replace_field!(outcome);
    replace_field!(decisions);
    replace_field!(next_step);
    replace_field!(unresolved);
    replace_field!(artifact_refs);
    replace_field!(provenance_refs);
    if existing.observed_at != 0
        && next.observed_at != 0
        && existing.observed_at != existing.observed_at.min(next.observed_at)
    {
        existing.observed_at = existing.observed_at.min(next.observed_at);
        changed = true;
    } else if existing.observed_at == 0 && next.observed_at != 0 {
        existing.observed_at = next.observed_at;
        changed = true;
    }
    if existing.updated_at != existing.updated_at.max(next.updated_at) {
        existing.updated_at = existing.updated_at.max(next.updated_at);
        changed = true;
    }
    let mut merged_supersedes = existing.supersedes.clone();
    for value in &next.supersedes {
        if !merged_supersedes
            .iter()
            .any(|existing_value| existing_value == value)
        {
            merged_supersedes.push(value.clone());
        }
    }
    merged_supersedes = normalize_compact_refs(&merged_supersedes);
    if existing.supersedes != merged_supersedes {
        existing.supersedes = merged_supersedes;
        changed = true;
    }
    changed
}

fn govern_continuity_capsules(entries: &mut Vec<ContinuityCapsule>, now_secs: u64) -> bool {
    let mut changed = false;
    *entries = entries
        .drain(..)
        .filter_map(|capsule| canonicalize_continuity_capsule(capsule))
        .collect();
    for capsule in entries.iter_mut() {
        if capsule.status == ContinuityCapsuleStatus::Done
            && now_secs.saturating_sub(capsule.updated_at) >= CONTINUITY_CAPSULE_STALE_AFTER_SECS
        {
            capsule.status = ContinuityCapsuleStatus::Stale;
            changed = true;
        }
    }
    entries.sort_by(|left, right| {
        capsule_retention_rank(right.status)
            .cmp(&capsule_retention_rank(left.status))
            .then_with(|| right.updated_at.cmp(&left.updated_at))
    });
    let mut kept = Vec::with_capacity(entries.len().min(MAX_CONTINUITY_CAPSULES));
    let mut per_scope = HashMap::<String, usize>::new();
    for capsule in entries.drain(..) {
        let scope_key = format!("{}:{}", capsule.scope_kind.label(), capsule.scope_id);
        let scope_count = per_scope.entry(scope_key).or_insert(0);
        if kept.len() >= MAX_CONTINUITY_CAPSULES
            || *scope_count >= MAX_CONTINUITY_CAPSULES_PER_SCOPE
        {
            changed = true;
            continue;
        }
        *scope_count = scope_count.saturating_add(1);
        kept.push(capsule);
    }
    kept.sort_by_key(|capsule| Reverse(capsule.updated_at));
    if *entries != kept {
        changed = true;
    }
    *entries = kept;
    changed
}

fn continuity_capsules_conflict(existing: &ContinuityCapsule, next: &ContinuityCapsule) -> bool {
    if existing.scope_kind != next.scope_kind || existing.scope_id != next.scope_id {
        return false;
    }
    if !existing.run_id.is_empty() && !next.run_id.is_empty() {
        return existing.run_id == next.run_id;
    }
    normalize_match_text(&existing.topic) == normalize_match_text(&next.topic)
}

fn capsule_retention_rank(status: ContinuityCapsuleStatus) -> u8 {
    match status {
        ContinuityCapsuleStatus::Active => 4,
        ContinuityCapsuleStatus::Done => 3,
        ContinuityCapsuleStatus::Stale => 2,
        ContinuityCapsuleStatus::Superseded => 1,
    }
}

fn build_continuity_capsule_candidate(
    capsule: ContinuityCapsule,
    selected: bool,
    total_score: u32,
    reason_fragments: Vec<String>,
) -> RecallCandidate {
    let excerpt = if !capsule.summary.is_empty() {
        capsule.summary.clone()
    } else if !capsule.outcome.is_empty() {
        capsule.outcome.clone()
    } else {
        capsule.next_step.clone()
    };
    let exact_match_score = u32::from(
        reason_fragments
            .iter()
            .any(|reason| reason.contains("exact topic overlap")),
    ) * 16;
    let lexical_score = reason_fragments
        .iter()
        .find_map(|reason| reason.strip_prefix("term_overlap="))
        .and_then(|value| value.parse::<u32>().ok())
        .map(|overlap| overlap.saturating_mul(4))
        .unwrap_or(0);
    let recency_score =
        continuity_capsule_recency_score(capsule.updated_at, crate::util::current_unix_secs());
    let scope_affinity_score = u32::from(
        reason_fragments
            .iter()
            .any(|reason| reason == "same-chat provenance"),
    ) * 6;
    let governance_score = match capsule.status {
        ContinuityCapsuleStatus::Active => 6,
        ContinuityCapsuleStatus::Done => 3,
        ContinuityCapsuleStatus::Stale => 1,
        ContinuityCapsuleStatus::Superseded => 0,
    };
    RecallCandidate {
        plane: RecallPlane::ContinuityCapsule,
        candidate_id: capsule.capsule_id.clone(),
        title: capsule.topic,
        excerpt: truncate_content_to_max(&excerpt, 180).trim().to_string(),
        citation: capsule.provenance_refs.first().cloned(),
        source: format!("{}_{}", capsule.kind.label(), capsule.status.label()),
        observed_at: Some(capsule.updated_at),
        selected,
        score: RecallScoreBreakdown {
            lexical_score,
            semantic_score: 0,
            exact_match_score,
            recency_score,
            confidence_score: 0,
            importance_score: (capsule.decisions.len() + capsule.unresolved.len())
                .min(MAX_CONTINUITY_CAPSULE_LIST_ITEMS) as u32,
            scope_affinity_score,
            governance_score,
            source_score: u32::from(!capsule.run_id.is_empty()) * 2,
            total_score,
            reason_fragments,
        },
    }
}

fn score_continuity_capsule(
    capsule: &ContinuityCapsule,
    preferred_chat_id: Option<&str>,
    normalized_query: &str,
    terms: &[String],
    now_secs: u64,
) -> Option<(u32, Vec<String>)> {
    let corpus = normalize_match_text(&format!(
        "{} {} {} {} {}",
        capsule.topic,
        capsule.summary,
        capsule.outcome,
        capsule.next_step,
        capsule.decisions.join(" ")
    ));
    let mut score = 0u32;
    let mut reasons = Vec::new();

    if let Some(chat_id) = preferred_chat_id {
        if !chat_id.trim().is_empty() && capsule.source_chat_id == chat_id.trim() {
            score = score.saturating_add(6);
            reasons.push("same-chat provenance".to_string());
        }
    }

    match capsule.status {
        ContinuityCapsuleStatus::Active => {
            score = score.saturating_add(6);
            reasons.push("active capsule".to_string());
        }
        ContinuityCapsuleStatus::Done => {
            score = score.saturating_add(3);
            reasons.push("completed capsule".to_string());
        }
        ContinuityCapsuleStatus::Stale => reasons.push("stale capsule".to_string()),
        ContinuityCapsuleStatus::Superseded => reasons.push("superseded capsule".to_string()),
    }

    let recency_score = continuity_capsule_recency_score(capsule.updated_at, now_secs);
    if recency_score > 0 {
        score = score.saturating_add(recency_score);
        reasons.push("recently updated".to_string());
    }

    if normalized_query.is_empty() {
        return Some((score.max(1), reasons));
    }

    let normalized_topic = normalize_match_text(&capsule.topic);
    if normalized_topic.contains(normalized_query) || normalized_query.contains(&normalized_topic) {
        score = score.saturating_add(16);
        reasons.push("exact topic overlap".to_string());
    }

    let overlap = terms
        .iter()
        .filter(|term| corpus.contains(term.as_str()))
        .count() as u32;
    if overlap == 0 && !reasons.iter().any(|reason| reason == "exact topic overlap") {
        return None;
    }
    if overlap > 0 {
        score = score.saturating_add(overlap.saturating_mul(4));
        reasons.push(format!("term_overlap={overlap}"));
    }
    Some((score, normalize_reason_fragments(reasons)))
}

fn continuity_capsule_recency_score(updated_at: u64, now_secs: u64) -> u32 {
    let age = now_secs.saturating_sub(updated_at);
    if age <= 30 * 60 {
        6
    } else if age <= 6 * 60 * 60 {
        4
    } else if age <= 24 * 60 * 60 {
        2
    } else {
        0
    }
}

fn normalize_reason_fragments(values: Vec<String>) -> Vec<String> {
    values
        .into_iter()
        .filter_map(|value| {
            let normalized = truncate_content_to_max(value.trim(), 96).trim().to_string();
            (!normalized.is_empty()).then_some(normalized)
        })
        .take(6)
        .collect()
}

fn normalize_ref_list(values: &[String]) -> Vec<String> {
    values
        .iter()
        .filter_map(|value| {
            let normalized =
                truncate_content_to_max(value.trim(), MAX_CONTINUITY_CAPSULE_SUMMARY_CHARS)
                    .trim()
                    .to_string();
            (!normalized.is_empty()).then_some(normalized)
        })
        .take(MAX_CONTINUITY_CAPSULE_LIST_ITEMS)
        .collect()
}

fn normalize_compact_refs(values: &[String]) -> Vec<String> {
    values
        .iter()
        .filter_map(|value| {
            let normalized =
                truncate_content_to_max(value.trim(), MAX_CONTINUITY_CAPSULE_REF_CHARS)
                    .trim()
                    .to_string();
            (!normalized.is_empty()).then_some(normalized)
        })
        .take(MAX_CONTINUITY_CAPSULE_LIST_ITEMS)
        .collect()
}

fn normalize_multiline(value: &str, max_chars: usize) -> String {
    truncate_content_to_max(value.trim(), max_chars)
        .trim()
        .to_string()
}

fn normalize_inline(value: &str, max_chars: usize) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .chars()
        .take(max_chars)
        .collect()
}

fn normalize_match_text(input: &str) -> String {
    input
        .chars()
        .map(|ch| {
            if ch.is_alphanumeric() || ('\u{4e00}'..='\u{9fff}').contains(&ch) {
                ch.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn collect_terms(normalized: &str) -> Vec<String> {
    let mut terms = normalized
        .split_whitespace()
        .filter(|term| term.len() >= 2 || term.chars().count() >= 2)
        .map(str::to_string)
        .collect::<Vec<_>>();
    terms.truncate(12);
    terms
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_id_prefers_run_identity_over_kind_shift() {
        let active = ContinuityCapsuleDraft {
            kind: ContinuityCapsuleKind::WorkSession,
            scope_kind: ContinuityCapsuleScopeKind::Chat,
            scope_id: "chat-1".to_string(),
            run_id: "run-1".to_string(),
            topic: "memory enhancement".to_string(),
            summary: "continue retrieval work".to_string(),
            ..Default::default()
        };
        let done = ContinuityCapsuleDraft {
            kind: ContinuityCapsuleKind::TaskResolution,
            outcome: "phase closed".to_string(),
            status: ContinuityCapsuleStatus::Done,
            ..active.clone()
        };
        assert_eq!(
            continuity_capsule_stable_id(&active),
            continuity_capsule_stable_id(&done)
        );
    }

    #[test]
    fn apply_drafts_supersedes_same_topic_within_scope() {
        let mut entries = Vec::new();
        let first = ContinuityCapsuleDraft {
            scope_kind: ContinuityCapsuleScopeKind::Chat,
            scope_id: "chat-1".to_string(),
            topic: "memory enhancement".to_string(),
            summary: "first pass".to_string(),
            ..Default::default()
        };
        let second = ContinuityCapsuleDraft {
            scope_kind: ContinuityCapsuleScopeKind::Chat,
            scope_id: "chat-1".to_string(),
            topic: "memory enhancement".to_string(),
            summary: "second pass".to_string(),
            outcome: "narrowed to continuity capsules".to_string(),
            status: ContinuityCapsuleStatus::Done,
            source: ContinuityCapsuleSource::TaskCompletion,
            ..Default::default()
        };
        let first_outcome = apply_continuity_capsule_drafts(&mut entries, &[first], 10);
        assert_eq!(first_outcome.upserted, 1);
        let second_outcome = apply_continuity_capsule_drafts(&mut entries, &[second], 20);
        assert_eq!(second_outcome.upserted, 1);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].status, ContinuityCapsuleStatus::Done);
        assert_eq!(entries[0].summary, "second pass");
        assert_eq!(entries[0].outcome, "narrowed to continuity capsules");
    }

    #[test]
    fn continuity_capsule_recall_prefers_exact_topic_and_same_chat() {
        struct StubStore {
            entries: Vec<ContinuityCapsule>,
        }
        impl ContinuityCapsuleStore for StubStore {
            fn upsert_many(
                &self,
                _drafts: &[ContinuityCapsuleDraft],
                _now_secs: u64,
            ) -> Result<ContinuityCapsuleWriteOutcome> {
                Ok(ContinuityCapsuleWriteOutcome::default())
            }

            fn get(&self, capsule_id: &str) -> Result<Option<ContinuityCapsule>> {
                Ok(self
                    .entries
                    .iter()
                    .find(|entry| entry.capsule_id == capsule_id)
                    .cloned())
            }

            fn list(&self, limit: usize) -> Result<Vec<ContinuityCapsule>> {
                Ok(self.entries.iter().take(limit).cloned().collect())
            }

            fn count(&self) -> Result<usize> {
                Ok(self.entries.len())
            }
        }

        let store = StubStore {
            entries: vec![
                continuity_capsule_from_draft(
                    &ContinuityCapsuleDraft {
                        scope_kind: ContinuityCapsuleScopeKind::Chat,
                        scope_id: "chat-1".to_string(),
                        source_chat_id: "chat-1".to_string(),
                        topic: "network setup".to_string(),
                        summary: "bring wifi up and verify route".to_string(),
                        source: ContinuityCapsuleSource::PostReplyMaintenance,
                        ..Default::default()
                    },
                    100,
                )
                .unwrap(),
                continuity_capsule_from_draft(
                    &ContinuityCapsuleDraft {
                        scope_kind: ContinuityCapsuleScopeKind::Chat,
                        scope_id: "chat-1".to_string(),
                        source_chat_id: "chat-2".to_string(),
                        topic: "archive cleanup".to_string(),
                        summary: "compact old notes".to_string(),
                        status: ContinuityCapsuleStatus::Done,
                        ..Default::default()
                    },
                    90,
                )
                .unwrap(),
            ],
        };
        let (report, hits) = inspect_continuity_capsule_recall(
            &store,
            ContinuityCapsuleScopeKind::Chat,
            "chat-1",
            Some("chat-1"),
            "继续 network setup",
            None,
            &[],
            400,
            120,
        );
        assert_eq!(report.selected_count, 1);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].topic, "network setup");
        assert!(report.candidates[0]
            .score
            .reason_fragments
            .iter()
            .any(|reason| reason.contains("exact topic")));
    }
}
