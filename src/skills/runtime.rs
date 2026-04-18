use crate::platform::SkillStorage;
use crate::skills::{
    get_skill_content, runtime_skill_name_for_topic, write_skill, RuntimeSkillWrite,
    MAX_SKILL_CONTENT_LEN,
};
use crate::util::{
    collect_retrieval_terms, looks_like_raw_payload_text, normalize_retrieval_text,
    procedural_text_signal_count, trigram_overlap_score, truncate_content_to_max,
};
#[cfg(target_os = "linux")]
use rusqlite::{params, Connection, OptionalExtension};
#[cfg(target_os = "linux")]
use std::collections::hash_map::DefaultHasher;
#[cfg(target_os = "linux")]
use std::hash::{Hash, Hasher};
#[cfg(target_os = "linux")]
use std::path::PathBuf;

const RUNTIME_SKILL_MARKER: &str = "<!-- beetle:runtime-skill -->";
const MAX_RUNTIME_SKILL_HITS: usize = 4;
const MAX_RUNTIME_SKILL_CITATIONS: usize = 8;
const MIN_RUNTIME_SKILL_BLOCK_LEN: usize = 180;
const RUNTIME_SKILL_TOUCH_INTERVAL_SECS: u64 = 6 * 60 * 60;
const RUNTIME_SKILL_STALE_AFTER_SECS: u64 = 90 * 86_400;
const RUNTIME_SKILL_DUPLICATE_SIMILARITY: u32 = 16;
const MAX_RUNTIME_SKILL_GENOME_NODES: usize = 8;
const MAX_RUNTIME_SKILL_STRATEGY_DIFFS: usize = 8;
const MAX_RUNTIME_SKILL_DOCTRINE_RECORDS: usize = 6;
const MAX_RUNTIME_SKILL_GENOME_RECORDS: usize = 6;
#[cfg(target_os = "linux")]
const RUNTIME_SKILL_INDEX_VERSION: u32 = 1;
#[cfg(target_os = "linux")]
const RUNTIME_SKILL_INDEX_CANDIDATE_LIMIT: usize = 24;
#[cfg(target_os = "linux")]
const REL_PATH_RUNTIME_SKILL_INDEX: &str = "memory/runtime_skill_index.sqlite3";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum RuntimeSkillRecallBackend {
    #[default]
    Heuristic,
    #[cfg(target_os = "linux")]
    SqliteFtsHybrid,
}

impl RuntimeSkillRecallBackend {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Heuristic => "runtime_skill_hybrid",
            #[cfg(target_os = "linux")]
            Self::SqliteFtsHybrid => "runtime_skill_sqlite_fts_hybrid",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct RuntimeSkillRecallResult {
    pub hits: Vec<RuntimeSkillHit>,
    pub backend: RuntimeSkillRecallBackend,
}

#[cfg(target_os = "linux")]
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
struct RuntimeSkillIndexSignature {
    record_count: usize,
    latest_updated_at: u64,
    digest: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct RuntimeSkillIndexHint {
    semantic_bonus: u32,
    reasons: Vec<String>,
}

const MAX_RUNTIME_SKILL_OPERATOR_RECORDS: usize = 6;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeSkillStatus {
    #[default]
    Active,
    Stale,
    LowValue,
    Retired,
}

impl RuntimeSkillStatus {
    fn label(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Stale => "stale",
            Self::LowValue => "low_value",
            Self::Retired => "retired",
        }
    }

    fn parse(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "stale" => Self::Stale,
            "low_value" | "low-value" | "low value" => Self::LowValue,
            "retired" => Self::Retired,
            _ => Self::Active,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeSkillGenomeDisposition {
    #[default]
    Active,
    Superseded,
    Retired,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RuntimeSkillGenomeNode {
    pub node_id: String,
    pub strategy_digest: String,
    pub recorded_at: u64,
    pub summary: String,
    pub disposition: RuntimeSkillGenomeDisposition,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeSkillStrategyDiffKind {
    #[default]
    SummaryRevision,
    ProcedureRefinement,
    DoctrineRevision,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RuntimeSkillStrategyDiff {
    pub recorded_at: u64,
    pub from_node_id: String,
    pub to_node_id: String,
    pub change_kind: RuntimeSkillStrategyDiffKind,
    pub summary: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RuntimeSkillDoctrineClauseRecord {
    pub source_skill_name: String,
    pub topic: String,
    pub clause: String,
    pub validated_success_count: u32,
    pub revision_pending: bool,
    pub evidence_ref_count: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RuntimeSkillDoctrineSnapshot {
    pub total_clauses: usize,
    pub stable_clauses: usize,
    pub revision_pending_clauses: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recent_clauses: Vec<RuntimeSkillDoctrineClauseRecord>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RuntimeSkillGenomeLineageRecord {
    pub skill_name: String,
    pub topic: String,
    pub title: String,
    pub status: RuntimeSkillStatus,
    pub lineage_depth: usize,
    pub diff_events: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_node_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_transition_at: Option<u64>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RuntimeSkillGenomeSnapshot {
    pub total_lineages: usize,
    pub active_lineages: usize,
    pub retired_lineages: usize,
    pub total_diff_events: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recent_lineages: Vec<RuntimeSkillGenomeLineageRecord>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RuntimeSkillGovernanceOutcome {
    pub merged: usize,
    pub pruned: usize,
    pub stale_marked: usize,
    pub low_value_marked: usize,
    pub retired_marked: usize,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeSkillReuseOutcome {
    #[default]
    Neutral,
    Succeeded,
    Mismatch,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeSkillWriteSource {
    #[default]
    Manual,
    Extraction,
    TaskLearning,
    ProgrammableReasoning,
}

impl RuntimeSkillWriteSource {
    pub fn label(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Extraction => "extraction",
            Self::TaskLearning => "task_learning",
            Self::ProgrammableReasoning => "programmable_reasoning",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeSkillWriteAction {
    #[default]
    Accepted,
    Rejected,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeSkillWriteReason {
    ProceduralMemory,
    EmptyOrInvalid,
    RawPayloadOrLog,
    WeakProcedure,
}

impl RuntimeSkillWriteReason {
    pub fn label(self) -> &'static str {
        match self {
            Self::ProceduralMemory => "procedural_memory",
            Self::EmptyOrInvalid => "empty_or_invalid",
            Self::RawPayloadOrLog => "raw_payload_or_log",
            Self::WeakProcedure => "weak_procedure",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RuntimeSkillWriteItemReport {
    pub source: RuntimeSkillWriteSource,
    pub action: RuntimeSkillWriteAction,
    pub reason: RuntimeSkillWriteReason,
    pub topic: String,
    pub detail: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RuntimeSkillWriteOutcome {
    pub source: RuntimeSkillWriteSource,
    pub submitted: usize,
    pub accepted: usize,
    pub rejected: usize,
    pub changed: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reports: Vec<RuntimeSkillWriteItemReport>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeSkillRecord {
    pub name: String,
    pub title: String,
    pub topic: String,
    pub summary: String,
    pub procedure: String,
    pub citations: Vec<String>,
    pub source_chat_id: Option<String>,
    pub observed_at: u64,
    pub updated_at: u64,
    pub last_used_at: Option<u64>,
    pub use_count: u32,
    pub quality_score: u8,
    pub status: RuntimeSkillStatus,
    pub validated_success_count: u32,
    pub mismatch_count: u32,
    pub revision_count: u32,
    pub revision_pending: bool,
    pub last_outcome_at: Option<u64>,
    pub last_outcome_note: String,
    pub supersedes: Vec<String>,
    pub component_topics: Vec<String>,
    pub genome_lineage: Vec<RuntimeSkillGenomeNode>,
    pub strategy_diffs: Vec<RuntimeSkillStrategyDiff>,
    pub retired_at: Option<u64>,
    pub retirement_reason: String,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct RuntimeSkillOperatorRecord {
    pub name: String,
    pub title: String,
    pub topic: String,
    pub status: RuntimeSkillStatus,
    pub quality_score: u8,
    pub use_count: u32,
    pub validated_success_count: u32,
    pub mismatch_count: u32,
    pub revision_pending: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_outcome_at: Option<u64>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub last_outcome_note: String,
    pub updated_at: u64,
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct RuntimeSkillOperatorSummary {
    #[serde(default)]
    pub total: usize,
    #[serde(default)]
    pub active: usize,
    #[serde(default)]
    pub stale: usize,
    #[serde(default)]
    pub low_value: usize,
    #[serde(default)]
    pub retired: usize,
    #[serde(default)]
    pub validated: usize,
    #[serde(default)]
    pub revision_pending: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recent_records: Vec<RuntimeSkillOperatorRecord>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RuntimeSkillRecallScoreBreakdown {
    pub lexical_score: u32,
    pub semantic_score: u32,
    pub exact_match_score: u32,
    pub recency_score: u32,
    pub confidence_score: u32,
    pub importance_score: u32,
    pub scope_affinity_score: u32,
    pub governance_score: u32,
    pub source_score: u32,
    pub total_score: u32,
    pub reason_fragments: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeSkillHit {
    pub record: RuntimeSkillRecord,
    pub score: u32,
    pub reasons: Vec<String>,
    pub score_breakdown: RuntimeSkillRecallScoreBreakdown,
}

#[derive(Clone, Debug)]
struct RuntimeSkillUpsertInput {
    name: String,
    title: String,
    topic: String,
    summary: String,
    procedure: String,
    citations: Vec<String>,
    source_chat_id: Option<String>,
    observed_at: u64,
    updated_at: u64,
}

pub fn is_runtime_skill_name(name: &str) -> bool {
    name.starts_with("runtime_skill__")
}

pub fn retrieve_runtime_skill_hits(
    storage: &dyn SkillStorage,
    query: &str,
    preferred_chat_id: Option<&str>,
    now_secs: u64,
    limit: usize,
) -> Vec<RuntimeSkillHit> {
    retrieve_runtime_skill_hits_with_backend(storage, query, preferred_chat_id, now_secs, limit)
        .hits
}

pub fn build_runtime_skill_operator_summary(
    storage: &dyn SkillStorage,
) -> RuntimeSkillOperatorSummary {
    let mut records = list_runtime_skill_records(storage);
    let mut summary = RuntimeSkillOperatorSummary {
        total: records.len(),
        ..RuntimeSkillOperatorSummary::default()
    };
    for record in &records {
        match record.status {
            RuntimeSkillStatus::Active => summary.active = summary.active.saturating_add(1),
            RuntimeSkillStatus::Stale => summary.stale = summary.stale.saturating_add(1),
            RuntimeSkillStatus::LowValue => {
                summary.low_value = summary.low_value.saturating_add(1);
            }
            RuntimeSkillStatus::Retired => {
                summary.retired = summary.retired.saturating_add(1);
            }
        }
        if record.validated_success_count > 0 {
            summary.validated = summary.validated.saturating_add(1);
        }
        if record.revision_pending {
            summary.revision_pending = summary.revision_pending.saturating_add(1);
        }
    }
    records.sort_by(|a, b| {
        b.last_outcome_at
            .cmp(&a.last_outcome_at)
            .then_with(|| b.updated_at.cmp(&a.updated_at))
            .then_with(|| {
                b.validated_success_count
                    .cmp(&a.validated_success_count)
                    .then_with(|| b.use_count.cmp(&a.use_count))
            })
            .then_with(|| a.name.cmp(&b.name))
    });
    summary.recent_records = records
        .into_iter()
        .take(MAX_RUNTIME_SKILL_OPERATOR_RECORDS)
        .map(|record| RuntimeSkillOperatorRecord {
            name: record.name,
            title: record.title,
            topic: record.topic,
            status: record.status,
            quality_score: record.quality_score,
            use_count: record.use_count,
            validated_success_count: record.validated_success_count,
            mismatch_count: record.mismatch_count,
            revision_pending: record.revision_pending,
            last_outcome_at: record.last_outcome_at,
            last_outcome_note: record.last_outcome_note,
            updated_at: record.updated_at,
        })
        .collect();
    summary
}

pub fn build_runtime_skill_doctrine_snapshot(
    storage: &dyn SkillStorage,
) -> RuntimeSkillDoctrineSnapshot {
    let mut clauses = list_runtime_skill_records(storage)
        .into_iter()
        .filter_map(runtime_skill_doctrine_clause_from_record)
        .collect::<Vec<_>>();
    clauses.sort_by(|a, b| {
        b.validated_success_count
            .cmp(&a.validated_success_count)
            .then_with(|| a.revision_pending.cmp(&b.revision_pending))
            .then_with(|| b.evidence_ref_count.cmp(&a.evidence_ref_count))
            .then_with(|| a.source_skill_name.cmp(&b.source_skill_name))
    });
    let total_clauses = clauses.len();
    let stable_clauses = clauses
        .iter()
        .filter(|record| !record.revision_pending)
        .count();
    let revision_pending_clauses = clauses
        .iter()
        .filter(|record| record.revision_pending)
        .count();
    clauses.truncate(MAX_RUNTIME_SKILL_DOCTRINE_RECORDS);
    RuntimeSkillDoctrineSnapshot {
        total_clauses,
        stable_clauses,
        revision_pending_clauses,
        recent_clauses: clauses,
    }
}

pub fn build_runtime_skill_genome_snapshot(
    storage: &dyn SkillStorage,
) -> RuntimeSkillGenomeSnapshot {
    let mut records = list_runtime_skill_records(storage);
    let total_lineages = records.len();
    let active_lineages = records
        .iter()
        .filter(|record| record.status != RuntimeSkillStatus::Retired)
        .count();
    let retired_lineages = records
        .iter()
        .filter(|record| record.status == RuntimeSkillStatus::Retired)
        .count();
    let total_diff_events = records
        .iter()
        .map(|record| record.strategy_diffs.len())
        .sum();
    records.sort_by(|a, b| {
        runtime_skill_last_transition_at(b)
            .cmp(&runtime_skill_last_transition_at(a))
            .then_with(|| b.updated_at.cmp(&a.updated_at))
            .then_with(|| a.name.cmp(&b.name))
    });
    let recent_lineages = records
        .into_iter()
        .take(MAX_RUNTIME_SKILL_GENOME_RECORDS)
        .map(|record| {
            let active_node_id =
                active_runtime_skill_genome_node(&record).map(|node| node.node_id.clone());
            let last_transition_at = runtime_skill_last_transition_at(&record);
            let lineage_depth = runtime_skill_effective_lineage(&record).len();
            let diff_events = record.strategy_diffs.len();
            RuntimeSkillGenomeLineageRecord {
                active_node_id,
                last_transition_at,
                skill_name: record.name,
                topic: record.topic,
                title: record.title,
                status: record.status,
                lineage_depth,
                diff_events,
            }
        })
        .collect();
    RuntimeSkillGenomeSnapshot {
        total_lineages,
        active_lineages,
        retired_lineages,
        total_diff_events,
        recent_lineages,
    }
}

pub(crate) fn retrieve_runtime_skill_hits_with_backend(
    storage: &dyn SkillStorage,
    query: &str,
    preferred_chat_id: Option<&str>,
    now_secs: u64,
    limit: usize,
) -> RuntimeSkillRecallResult {
    let normalized_query = normalize_runtime_skill_text(query);
    if normalized_query.is_empty() || normalized_query.chars().count() < 2 {
        return RuntimeSkillRecallResult::default();
    }
    let terms = collect_runtime_skill_terms(&normalized_query);
    let records = list_runtime_skill_records(storage);
    let (index_hints, backend) =
        runtime_skill_index_hints(&records, &normalized_query, &terms, preferred_chat_id);
    let mut hits = records
        .into_iter()
        .filter_map(|record| {
            let index_hint = index_hints.get(record.name.as_str());
            score_runtime_skill_record(
                record,
                &normalized_query,
                &terms,
                index_hint,
                preferred_chat_id,
                now_secs,
            )
        })
        .collect::<Vec<_>>();
    hits.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| {
                b.score_breakdown
                    .semantic_score
                    .cmp(&a.score_breakdown.semantic_score)
            })
            .then_with(|| b.record.quality_score.cmp(&a.record.quality_score))
            .then_with(|| b.record.last_used_at.cmp(&a.record.last_used_at))
            .then_with(|| b.record.updated_at.cmp(&a.record.updated_at))
            .then_with(|| a.record.name.cmp(&b.record.name))
    });
    let mut selected = Vec::with_capacity(limit.min(MAX_RUNTIME_SKILL_HITS));
    let mut seen_topics = Vec::new();
    for hit in hits {
        if selected.len() >= limit.min(MAX_RUNTIME_SKILL_HITS) {
            break;
        }
        let key =
            normalize_runtime_skill_text(&format!("{} {}", hit.record.topic, hit.record.summary));
        if !key.is_empty()
            && seen_topics.iter().any(|existing: &String| {
                existing == &key || existing.contains(&key) || key.contains(existing)
            })
        {
            continue;
        }
        if !key.is_empty() {
            seen_topics.push(key);
        }
        selected.push(hit);
    }
    RuntimeSkillRecallResult {
        hits: selected,
        backend,
    }
}

pub fn touch_runtime_skill_hits(
    storage: &dyn SkillStorage,
    hits: &[RuntimeSkillHit],
    now_secs: u64,
) -> usize {
    let mut changed = 0usize;
    for hit in hits {
        let mut record = hit.record.clone();
        if record.last_used_at.is_some_and(|previous| {
            now_secs.saturating_sub(previous) < RUNTIME_SKILL_TOUCH_INTERVAL_SECS
        }) {
            continue;
        }
        record.last_used_at = Some(now_secs);
        record.use_count = record.use_count.saturating_add(1);
        record.quality_score = compute_runtime_skill_quality(&record);
        record.status = RuntimeSkillStatus::Active;
        record.retired_at = None;
        record.retirement_reason.clear();
        if let Some(last) = record.genome_lineage.last_mut() {
            last.disposition = RuntimeSkillGenomeDisposition::Active;
        }
        if write_runtime_skill_record(storage, &record).is_ok() {
            changed = changed.saturating_add(1);
        }
    }
    changed
}

pub fn build_runtime_skill_recall_block(
    storage: &dyn SkillStorage,
    query: &str,
    preferred_chat_id: Option<&str>,
    now_secs: u64,
    max_chars: usize,
) -> Option<String> {
    if max_chars < MIN_RUNTIME_SKILL_BLOCK_LEN {
        return None;
    }
    let mut hits = retrieve_runtime_skill_hits(storage, query, preferred_chat_id, now_secs, 3);
    if hits.is_empty() {
        hits = fallback_runtime_skill_hits(storage, preferred_chat_id, now_secs, 2);
    }
    if hits.is_empty() {
        return None;
    }
    let mut out = String::from(
        "## Runtime skills\nProcedural memory distilled from proven prior operations. Reuse the method when it fits, but adapt it to current constraints instead of quoting it blindly.\n",
    );
    let mut appended = 0usize;
    if let Some(composition_line) = build_runtime_skill_composition_line(&hits, query, max_chars) {
        if out
            .len()
            .saturating_add(composition_line.len())
            .saturating_add(1)
            <= max_chars
        {
            out.push_str(&composition_line);
            out.push('\n');
        }
    }
    for hit in &hits {
        let reasons_joined = hit.reasons.join(", ");
        let reasons = truncate_content_to_max(&reasons_joined, 140);
        let citations = hit
            .record
            .citations
            .iter()
            .take(2)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        let line = if citations.is_empty() {
            format!(
                "- [{}] {} (topic: {}; why: {}; quality={}; reused={}; status={})",
                hit.record.title,
                truncate_content_to_max(hit.record.summary.trim(), 120),
                hit.record.topic,
                reasons,
                hit.record.quality_score,
                hit.record.use_count,
                hit.record.status.label(),
            )
        } else {
            format!(
                "- [{}] {} (topic: {}; why: {}; quality={}; reused={}; status={}; provenance={})",
                hit.record.title,
                truncate_content_to_max(hit.record.summary.trim(), 120),
                hit.record.topic,
                reasons,
                hit.record.quality_score,
                hit.record.use_count,
                hit.record.status.label(),
                citations,
            )
        };
        let remaining = max_chars.saturating_sub(out.len()).saturating_sub(1);
        if line.len() > remaining {
            if remaining < 64 {
                break;
            }
            out.push_str(&truncate_content_to_max(&line, remaining));
            out.push('\n');
            appended = appended.saturating_add(1);
            break;
        }
        out.push_str(&line);
        out.push('\n');
        appended = appended.saturating_add(1);
    }
    if appended == 0 {
        None
    } else {
        let _ = touch_runtime_skill_hits(storage, &hits[..appended], now_secs);
        Some(out.trim_end().to_string())
    }
}

pub fn record_runtime_skill_outcomes(
    storage: &dyn SkillStorage,
    skill_names: &[String],
    outcome: RuntimeSkillReuseOutcome,
    now_secs: u64,
    outcome_note: &str,
) -> crate::error::Result<usize> {
    if matches!(outcome, RuntimeSkillReuseOutcome::Neutral) || skill_names.is_empty() {
        return Ok(0);
    }
    let mut changed = 0usize;
    for skill_name in skill_names {
        let Some(content) = get_skill_content(storage, skill_name) else {
            continue;
        };
        let Some(mut record) = parse_runtime_skill_record(skill_name, &content) else {
            continue;
        };
        let note = truncate_content_to_max(outcome_note.trim(), 160).into_owned();
        match outcome {
            RuntimeSkillReuseOutcome::Neutral => {}
            RuntimeSkillReuseOutcome::Succeeded => {
                record.validated_success_count = record.validated_success_count.saturating_add(1);
                record.revision_pending = false;
            }
            RuntimeSkillReuseOutcome::Mismatch => {
                record.mismatch_count = record.mismatch_count.saturating_add(1);
                record.revision_count = record.revision_count.saturating_add(1);
                record.revision_pending = true;
            }
        }
        record.last_outcome_at = Some(now_secs);
        record.last_outcome_note = note;
        record.updated_at = record.updated_at.max(now_secs);
        record.quality_score = compute_runtime_skill_quality(&record);
        write_runtime_skill_record(storage, &record)?;
        changed = changed.saturating_add(1);
    }
    if changed > 0 {
        super::capability_atoms::sync_capability_atoms_from_runtime_skills(storage, now_secs)?;
    }
    Ok(changed)
}

pub fn write_governed_runtime_skills(
    storage: &dyn SkillStorage,
    writes: &[RuntimeSkillWrite],
    source: RuntimeSkillWriteSource,
) -> crate::error::Result<RuntimeSkillWriteOutcome> {
    let mut outcome = RuntimeSkillWriteOutcome {
        source,
        submitted: writes.len(),
        ..RuntimeSkillWriteOutcome::default()
    };
    for write in writes {
        let topic = write.topic.trim().to_string();
        let (reason, detail) = match inspect_runtime_skill_write_shape(write) {
            Ok(()) => {
                let changed = upsert_runtime_skill_inner(storage, write, false)?;
                outcome.accepted = outcome.accepted.saturating_add(1);
                outcome.changed = outcome.changed.saturating_add(usize::from(changed));
                outcome.reports.push(RuntimeSkillWriteItemReport {
                    source,
                    action: RuntimeSkillWriteAction::Accepted,
                    reason: RuntimeSkillWriteReason::ProceduralMemory,
                    topic,
                    detail: format!("accepted as runtime skill via {}", source.label()),
                });
                continue;
            }
            Err(report) => (report.reason, report.detail),
        };
        outcome.rejected = outcome.rejected.saturating_add(1);
        outcome.reports.push(RuntimeSkillWriteItemReport {
            source,
            action: RuntimeSkillWriteAction::Rejected,
            reason,
            topic,
            detail,
        });
    }
    if outcome.accepted > 0 {
        super::capability_atoms::sync_capability_atoms_from_runtime_skills(
            storage,
            crate::util::current_unix_secs(),
        )?;
    }
    Ok(outcome)
}

pub fn govern_runtime_skills(
    storage: &dyn SkillStorage,
    now_secs: u64,
) -> crate::error::Result<RuntimeSkillGovernanceOutcome> {
    let mut records = list_runtime_skill_records(storage);
    if records.is_empty() {
        return Ok(RuntimeSkillGovernanceOutcome::default());
    }
    let mut outcome = RuntimeSkillGovernanceOutcome::default();
    let mut changed_records = std::collections::HashMap::<String, RuntimeSkillRecord>::new();
    let mut removed_names = Vec::new();

    records.sort_by(|a, b| a.name.cmp(&b.name));
    let mut consumed = vec![false; records.len()];
    for idx in 0..records.len() {
        if consumed[idx] {
            continue;
        }
        let mut group = vec![records[idx].clone()];
        consumed[idx] = true;
        for other_idx in (idx + 1)..records.len() {
            if consumed[other_idx] {
                continue;
            }
            if runtime_skill_similarity(&records[idx], &records[other_idx])
                < RUNTIME_SKILL_DUPLICATE_SIMILARITY
            {
                continue;
            }
            group.push(records[other_idx].clone());
            consumed[other_idx] = true;
        }
        let canonical = select_canonical_runtime_skill_index(&group);
        let merged = merge_runtime_skill_group(group, canonical);
        for superseded in &merged.supersedes {
            if superseded != &merged.name {
                removed_names.push(superseded.clone());
                outcome.merged = outcome.merged.saturating_add(1);
            }
        }
        let governed = apply_runtime_skill_status(merged, now_secs, &mut outcome);
        if should_prune_runtime_skill(&governed, now_secs) {
            removed_names.push(governed.name.clone());
            outcome.pruned = outcome.pruned.saturating_add(1);
            continue;
        }
        changed_records.insert(governed.name.clone(), governed);
    }

    for record in changed_records.values() {
        write_runtime_skill_record(storage, record)?;
    }
    removed_names.sort();
    removed_names.dedup();
    for name in removed_names {
        let _ = storage.remove(&name);
    }
    super::capability_atoms::sync_capability_atoms_from_runtime_skills(storage, now_secs)?;
    Ok(outcome)
}

fn fallback_runtime_skill_hits(
    storage: &dyn SkillStorage,
    preferred_chat_id: Option<&str>,
    now_secs: u64,
    limit: usize,
) -> Vec<RuntimeSkillHit> {
    let mut hits = list_runtime_skill_records(storage)
        .into_iter()
        .filter_map(|record| {
            if should_prune_runtime_skill(&record, now_secs)
                || matches!(record.status, RuntimeSkillStatus::Retired)
            {
                return None;
            }
            let mut reasons = vec!["fallback procedural memory".to_string()];
            let scope_affinity_score = preferred_chat_id
                .filter(|chat_id| record.source_chat_id.as_deref() == Some(*chat_id))
                .map(|_| 4)
                .unwrap_or(0);
            if scope_affinity_score > 0 {
                reasons.push("same-chat provenance".to_string());
            }
            let recency_score = record
                .last_used_at
                .filter(|last_used_at| now_secs.saturating_sub(*last_used_at) <= 30 * 86_400)
                .map(|_| 4)
                .unwrap_or(0);
            if recency_score > 0 {
                reasons.push("recently reused".to_string());
            }
            if runtime_skill_is_stale(&record, now_secs) {
                reasons.push("stale".to_string());
            }
            if matches!(record.status, RuntimeSkillStatus::LowValue) {
                reasons.push("low-value".to_string());
            }
            if record.validated_success_count > 0 {
                reasons.push("validated reuse evidence".to_string());
            }
            if record.revision_pending {
                reasons.push("revision pending".to_string());
            }
            let confidence_score =
                (record.quality_score / 8) as u32 + runtime_skill_validated_bonus(&record);
            let importance_score = record.use_count.min(6).saturating_mul(2);
            let governance_score = runtime_skill_governance_score(
                &record,
                match record.status {
                    RuntimeSkillStatus::Active => 4,
                    RuntimeSkillStatus::Stale => 1,
                    RuntimeSkillStatus::LowValue | RuntimeSkillStatus::Retired => 0,
                },
            );
            let source_score = record.citations.len().min(3) as u32 * 2;
            let breakdown = RuntimeSkillRecallScoreBreakdown {
                lexical_score: 0,
                semantic_score: 0,
                exact_match_score: 0,
                recency_score,
                confidence_score,
                importance_score,
                scope_affinity_score,
                governance_score,
                source_score,
                total_score: recency_score
                    .saturating_add(confidence_score)
                    .saturating_add(importance_score)
                    .saturating_add(scope_affinity_score)
                    .saturating_add(governance_score)
                    .saturating_add(source_score),
                reason_fragments: reasons.clone(),
            };
            Some(RuntimeSkillHit {
                record,
                score: breakdown.total_score,
                reasons,
                score_breakdown: breakdown,
            })
        })
        .collect::<Vec<_>>();
    hits.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| b.record.quality_score.cmp(&a.record.quality_score))
            .then_with(|| b.record.updated_at.cmp(&a.record.updated_at))
            .then_with(|| a.record.name.cmp(&b.record.name))
    });
    hits.truncate(limit.min(MAX_RUNTIME_SKILL_HITS));
    hits
}

pub fn upsert_runtime_skill(
    storage: &dyn SkillStorage,
    write: &RuntimeSkillWrite,
) -> crate::error::Result<bool> {
    upsert_runtime_skill_inner(storage, write, true)
}

fn upsert_runtime_skill_inner(
    storage: &dyn SkillStorage,
    write: &RuntimeSkillWrite,
    sync_atoms: bool,
) -> crate::error::Result<bool> {
    let mut input = RuntimeSkillUpsertInput {
        name: if write.name.trim().is_empty() {
            runtime_skill_name_for_topic(&write.topic)
        } else {
            write.name.trim().to_string()
        },
        title: if write.title.trim().is_empty() {
            write.topic.trim().replace('_', " ")
        } else {
            write.title.trim().to_string()
        },
        topic: write.topic.trim().to_string(),
        summary: write.summary.trim().to_string(),
        procedure: write.content.trim().to_string(),
        citations: normalize_citations(&write.citations),
        source_chat_id: write
            .source_chat_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string),
        observed_at: write.observed_at,
        updated_at: write.observed_at.max(crate::util::current_unix_secs()),
    };
    if input.summary.is_empty() {
        input.summary = build_runtime_skill_summary(&input.procedure);
    }
    let existing = get_skill_content(storage, &input.name)
        .and_then(|content| parse_runtime_skill_record(&input.name, &content));
    let all_records = list_runtime_skill_records(storage);
    let canonical_name = find_canonical_runtime_skill_name(&all_records, &input);
    let existing = canonical_name
        .as_deref()
        .and_then(|name| {
            get_skill_content(storage, name)
                .and_then(|content| parse_runtime_skill_record(name, &content))
        })
        .or(existing);
    if let Some(canonical_name) = canonical_name {
        input.name = canonical_name;
    }
    let record = merge_runtime_skill_record(existing.as_ref(), input);
    let rendered = render_runtime_skill_record(&record);
    let changed = get_skill_content(storage, &record.name)
        .map(|existing| existing.trim() != rendered.trim())
        .unwrap_or(true);
    if !changed {
        return Ok(false);
    }
    if rendered.len() > MAX_SKILL_CONTENT_LEN {
        return Err(crate::error::Error::config(
            "runtime_skill",
            format!(
                "content length {} exceeds {}",
                rendered.len(),
                MAX_SKILL_CONTENT_LEN
            ),
        ));
    }
    write_skill(storage, &record.name, &rendered)?;
    if sync_atoms {
        super::capability_atoms::sync_capability_atoms_from_runtime_skills(
            storage,
            crate::util::current_unix_secs(),
        )?;
    }
    Ok(true)
}

pub(crate) fn list_runtime_skill_records(storage: &dyn SkillStorage) -> Vec<RuntimeSkillRecord> {
    let mut out = Vec::new();
    for name in crate::skills::list_skill_names(storage) {
        if !is_runtime_skill_name(&name) {
            continue;
        }
        let Some(content) = get_skill_content(storage, &name) else {
            continue;
        };
        let Some(record) = parse_runtime_skill_record(&name, &content) else {
            continue;
        };
        out.push(record);
    }
    out
}

fn runtime_skill_index_hints(
    records: &[RuntimeSkillRecord],
    normalized_query: &str,
    terms: &[String],
    preferred_chat_id: Option<&str>,
) -> (
    std::collections::HashMap<String, RuntimeSkillIndexHint>,
    RuntimeSkillRecallBackend,
) {
    #[cfg(target_os = "linux")]
    {
        if records.is_empty() || normalized_query.is_empty() || terms.is_empty() {
            return (
                std::collections::HashMap::new(),
                RuntimeSkillRecallBackend::Heuristic,
            );
        }
        match runtime_skill_index_hints_sqlite(records, normalized_query, terms, preferred_chat_id)
        {
            Ok(hints) => (hints, RuntimeSkillRecallBackend::SqliteFtsHybrid),
            Err(error) => {
                log::debug!("[runtime_skill] sqlite recall fallback: {}", error);
                (
                    std::collections::HashMap::new(),
                    RuntimeSkillRecallBackend::Heuristic,
                )
            }
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (records, normalized_query, terms, preferred_chat_id);
        (
            std::collections::HashMap::new(),
            RuntimeSkillRecallBackend::Heuristic,
        )
    }
}

#[cfg(target_os = "linux")]
fn runtime_skill_index_hints_sqlite(
    records: &[RuntimeSkillRecord],
    normalized_query: &str,
    terms: &[String],
    preferred_chat_id: Option<&str>,
) -> crate::error::Result<std::collections::HashMap<String, RuntimeSkillIndexHint>> {
    let signature = build_runtime_skill_index_signature(records);
    let path = runtime_skill_index_path(&signature);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| crate::error::Error::io("runtime_skill_index", e))?;
    }
    let mut conn = Connection::open(path)
        .map_err(|e| crate::error::Error::config("runtime_skill_index", e.to_string()))?;
    ensure_runtime_skill_sqlite_schema(&conn)?;
    if runtime_skill_sqlite_needs_rebuild(&conn, &signature)? {
        runtime_skill_sqlite_rebuild(&mut conn, records, &signature)?;
    }
    query_runtime_skill_hints_sqlite(&conn, normalized_query, terms, preferred_chat_id)
}

#[cfg(target_os = "linux")]
fn build_runtime_skill_index_signature(
    records: &[RuntimeSkillRecord],
) -> RuntimeSkillIndexSignature {
    let mut hasher = DefaultHasher::new();
    let mut latest_updated_at = 0u64;
    for record in records {
        record.name.hash(&mut hasher);
        record.title.hash(&mut hasher);
        record.topic.hash(&mut hasher);
        record.summary.hash(&mut hasher);
        record.procedure.hash(&mut hasher);
        record.citations.hash(&mut hasher);
        record.source_chat_id.hash(&mut hasher);
        record.observed_at.hash(&mut hasher);
        record.updated_at.hash(&mut hasher);
        record.last_used_at.hash(&mut hasher);
        record.use_count.hash(&mut hasher);
        record.quality_score.hash(&mut hasher);
        record.status.label().hash(&mut hasher);
        record.supersedes.hash(&mut hasher);
        record.component_topics.hash(&mut hasher);
        latest_updated_at = latest_updated_at.max(record.updated_at.max(record.observed_at));
    }
    RuntimeSkillIndexSignature {
        record_count: records.len(),
        latest_updated_at,
        digest: hasher.finish(),
    }
}

#[cfg(target_os = "linux")]
fn runtime_skill_index_path(_signature: &RuntimeSkillIndexSignature) -> PathBuf {
    crate::platform::state_mount_path().join(REL_PATH_RUNTIME_SKILL_INDEX)
}

#[cfg(target_os = "linux")]
fn ensure_runtime_skill_sqlite_schema(conn: &Connection) -> crate::error::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS runtime_skill_meta (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS runtime_skill_documents (
            name TEXT PRIMARY KEY,
            title TEXT NOT NULL,
            topic TEXT NOT NULL,
            summary TEXT NOT NULL,
            procedure TEXT NOT NULL,
            citations TEXT NOT NULL,
            component_topics TEXT NOT NULL,
            source_chat_id TEXT,
            observed_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            last_used_at INTEGER,
            use_count INTEGER NOT NULL,
            quality_score INTEGER NOT NULL,
            status TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_runtime_skill_documents_chat
            ON runtime_skill_documents(source_chat_id);
        CREATE INDEX IF NOT EXISTS idx_runtime_skill_documents_updated
            ON runtime_skill_documents(updated_at);
        CREATE VIRTUAL TABLE IF NOT EXISTS runtime_skill_documents_fts USING fts5(
            name UNINDEXED,
            title,
            topic,
            summary,
            procedure,
            citations,
            component_topics,
            tokenize='unicode61 remove_diacritics 2'
        );",
    )
    .map_err(|e| crate::error::Error::config("runtime_skill_index", e.to_string()))
}

#[cfg(target_os = "linux")]
fn runtime_skill_sqlite_needs_rebuild(
    conn: &Connection,
    signature: &RuntimeSkillIndexSignature,
) -> crate::error::Result<bool> {
    let version = conn
        .query_row(
            "SELECT value FROM runtime_skill_meta WHERE key = 'version'",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|e| crate::error::Error::config("runtime_skill_index", e.to_string()))?;
    if version.as_deref() != Some(&RUNTIME_SKILL_INDEX_VERSION.to_string()) {
        return Ok(true);
    }
    let stored_signature = conn
        .query_row(
            "SELECT value FROM runtime_skill_meta WHERE key = 'signature'",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|e| crate::error::Error::config("runtime_skill_index", e.to_string()))?;
    let Some(stored_signature) = stored_signature else {
        return Ok(true);
    };
    let parsed = serde_json::from_str::<RuntimeSkillIndexSignature>(&stored_signature)
        .map_err(|e| crate::error::Error::config("runtime_skill_index", e.to_string()))?;
    Ok(parsed != *signature)
}

#[cfg(target_os = "linux")]
fn runtime_skill_sqlite_rebuild(
    conn: &mut Connection,
    records: &[RuntimeSkillRecord],
    signature: &RuntimeSkillIndexSignature,
) -> crate::error::Result<()> {
    let tx = conn
        .transaction()
        .map_err(|e| crate::error::Error::config("runtime_skill_index", e.to_string()))?;
    tx.execute("DELETE FROM runtime_skill_documents_fts", [])
        .map_err(|e| crate::error::Error::config("runtime_skill_index", e.to_string()))?;
    tx.execute("DELETE FROM runtime_skill_documents", [])
        .map_err(|e| crate::error::Error::config("runtime_skill_index", e.to_string()))?;
    for record in records {
        let citations = record.citations.join("\n");
        let component_topics = record.component_topics.join("\n");
        tx.execute(
            "INSERT INTO runtime_skill_documents (
                name, title, topic, summary, procedure, citations, component_topics,
                source_chat_id, observed_at, updated_at, last_used_at, use_count,
                quality_score, status
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            params![
                record.name,
                record.title,
                record.topic,
                record.summary,
                record.procedure,
                citations,
                component_topics,
                record.source_chat_id,
                record.observed_at as i64,
                record.updated_at as i64,
                record.last_used_at.map(|value| value as i64),
                record.use_count as i64,
                record.quality_score as i64,
                record.status.label(),
            ],
        )
        .map_err(|e| crate::error::Error::config("runtime_skill_index", e.to_string()))?;
        let rowid = tx.last_insert_rowid();
        tx.execute(
            "INSERT INTO runtime_skill_documents_fts(
                rowid, name, title, topic, summary, procedure, citations, component_topics
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                rowid,
                record.name,
                record.title,
                record.topic,
                record.summary,
                record.procedure,
                record.citations.join(" "),
                record.component_topics.join(" "),
            ],
        )
        .map_err(|e| crate::error::Error::config("runtime_skill_index", e.to_string()))?;
    }
    tx.execute(
        "INSERT INTO runtime_skill_meta(key, value) VALUES('version', ?1)
         ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        params![RUNTIME_SKILL_INDEX_VERSION.to_string()],
    )
    .map_err(|e| crate::error::Error::config("runtime_skill_index", e.to_string()))?;
    tx.execute(
        "INSERT INTO runtime_skill_meta(key, value) VALUES('signature', ?1)
         ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        params![serde_json::to_string(signature)
            .map_err(|e| crate::error::Error::config("runtime_skill_index", e.to_string()))?],
    )
    .map_err(|e| crate::error::Error::config("runtime_skill_index", e.to_string()))?;
    tx.commit()
        .map_err(|e| crate::error::Error::config("runtime_skill_index", e.to_string()))
}

#[cfg(target_os = "linux")]
fn query_runtime_skill_hints_sqlite(
    conn: &Connection,
    normalized_query: &str,
    terms: &[String],
    preferred_chat_id: Option<&str>,
) -> crate::error::Result<std::collections::HashMap<String, RuntimeSkillIndexHint>> {
    let Some(match_expr) = runtime_skill_match_expression(normalized_query, terms) else {
        return Ok(std::collections::HashMap::new());
    };
    let mut stmt = conn
        .prepare(
            "SELECT d.name, d.source_chat_id, bm25(
                    runtime_skill_documents_fts, 5.0, 6.0, 2.5, 1.2, 1.0, 2.0
             ) AS rank
             FROM runtime_skill_documents_fts
             JOIN runtime_skill_documents d ON d.rowid = runtime_skill_documents_fts.rowid
             WHERE runtime_skill_documents_fts MATCH ?1
             ORDER BY CASE
                    WHEN ?2 IS NOT NULL AND d.source_chat_id = ?2 THEN 0
                    ELSE 1
                 END ASC,
                 rank ASC,
                 d.updated_at DESC
             LIMIT ?3",
        )
        .map_err(|e| crate::error::Error::config("runtime_skill_index", e.to_string()))?;
    let rows = stmt
        .query_map(
            params![
                match_expr,
                preferred_chat_id
                    .map(str::trim)
                    .filter(|value| !value.is_empty()),
                RUNTIME_SKILL_INDEX_CANDIDATE_LIMIT as i64,
            ],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, f64>(2)?,
                ))
            },
        )
        .map_err(|e| crate::error::Error::config("runtime_skill_index", e.to_string()))?;
    let mut hints = std::collections::HashMap::new();
    for (idx, row) in rows.enumerate() {
        let (name, source_chat_id, _rank) =
            row.map_err(|e| crate::error::Error::config("runtime_skill_index", e.to_string()))?;
        let semantic_bonus = match idx {
            0 => 18,
            1 => 14,
            2 => 11,
            3 => 9,
            4..=7 => 7,
            _ => 5,
        };
        let same_chat = preferred_chat_id
            .map(str::trim)
            .filter(|value| !value.is_empty())
            == source_chat_id.as_deref();
        let mut reasons = vec!["indexed recall hit".to_string()];
        if same_chat {
            reasons.push("indexed same-chat prior".to_string());
        }
        hints.insert(
            name,
            RuntimeSkillIndexHint {
                semantic_bonus: semantic_bonus + u32::from(same_chat) * 2,
                reasons,
            },
        );
    }
    Ok(hints)
}

#[cfg(target_os = "linux")]
fn runtime_skill_match_expression(normalized_query: &str, terms: &[String]) -> Option<String> {
    let mut parts = Vec::new();
    if normalized_query.contains(' ') {
        parts.push(format!("\"{}\"", normalized_query.replace('"', "\"\"")));
    }
    for term in terms {
        let escaped = term.replace('"', "\"\"");
        if escaped.trim().is_empty() {
            continue;
        }
        parts.push(format!("\"{}\"", escaped));
    }
    parts.sort();
    parts.dedup();
    (!parts.is_empty()).then(|| parts.join(" OR "))
}

fn parse_runtime_skill_json_section<T>(
    sections: &std::collections::HashMap<String, String>,
    key: &str,
) -> Option<T>
where
    T: serde::de::DeserializeOwned,
{
    let raw = sections.get(key)?.trim();
    if raw.is_empty() {
        return None;
    }
    serde_json::from_str(raw).ok()
}

fn runtime_skill_strategy_digest(summary: &str, procedure: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    normalize_runtime_skill_text(summary).hash(&mut hasher);
    normalize_runtime_skill_text(procedure).hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn runtime_skill_summary_excerpt(summary: &str, procedure: &str) -> String {
    let summary = summary.trim();
    if !summary.is_empty() {
        return truncate_content_to_max(summary, 120).into_owned();
    }
    truncate_content_to_max(procedure.trim(), 120).into_owned()
}

fn runtime_skill_genome_node_id(skill_name: &str, digest: &str) -> String {
    format!(
        "{}@{}",
        skill_name.trim(),
        digest.chars().take(10).collect::<String>()
    )
}

fn build_runtime_skill_genome_node(
    skill_name: &str,
    summary: &str,
    procedure: &str,
    recorded_at: u64,
    disposition: RuntimeSkillGenomeDisposition,
) -> RuntimeSkillGenomeNode {
    let strategy_digest = runtime_skill_strategy_digest(summary, procedure);
    RuntimeSkillGenomeNode {
        node_id: runtime_skill_genome_node_id(skill_name, &strategy_digest),
        strategy_digest,
        recorded_at,
        summary: runtime_skill_summary_excerpt(summary, procedure),
        disposition,
    }
}

fn runtime_skill_effective_lineage(record: &RuntimeSkillRecord) -> Vec<RuntimeSkillGenomeNode> {
    if !record.genome_lineage.is_empty() {
        return record.genome_lineage.clone();
    }
    vec![build_runtime_skill_genome_node(
        &record.name,
        &record.summary,
        &record.procedure,
        record.updated_at.max(record.observed_at),
        match record.status {
            RuntimeSkillStatus::Retired => RuntimeSkillGenomeDisposition::Retired,
            _ => RuntimeSkillGenomeDisposition::Active,
        },
    )]
}

fn active_runtime_skill_genome_node(record: &RuntimeSkillRecord) -> Option<RuntimeSkillGenomeNode> {
    runtime_skill_effective_lineage(record)
        .into_iter()
        .rev()
        .find(|node| node.disposition == RuntimeSkillGenomeDisposition::Active)
}

pub(crate) fn runtime_skill_last_transition_at(record: &RuntimeSkillRecord) -> Option<u64> {
    let lineage_transition_at = runtime_skill_effective_lineage(record)
        .into_iter()
        .map(|node| node.recorded_at)
        .max()
        .unwrap_or(0);
    let transition_at = lineage_transition_at
        .max(record.retired_at.unwrap_or(0))
        .max(record.last_outcome_at.unwrap_or(0))
        .max(record.updated_at.max(record.observed_at));
    (transition_at > 0).then_some(transition_at)
}

pub(crate) fn runtime_skill_doctrine_event_at(record: &RuntimeSkillRecord) -> Option<u64> {
    let doctrine_diff_at = record
        .strategy_diffs
        .iter()
        .filter(|diff| {
            !matches!(
                diff.change_kind,
                RuntimeSkillStrategyDiffKind::ProcedureRefinement
            )
        })
        .map(|diff| diff.recorded_at)
        .max();
    record
        .last_outcome_at
        .map(|outcome_at| doctrine_diff_at.map_or(outcome_at, |diff_at| diff_at.max(outcome_at)))
        .or(doctrine_diff_at)
        .or(Some(record.observed_at).filter(|value| *value > 0))
        .or(Some(record.updated_at).filter(|value| *value > 0))
}

pub(crate) fn runtime_skill_genome_event_at(record: &RuntimeSkillRecord) -> Option<u64> {
    record
        .strategy_diffs
        .last()
        .map(|diff| diff.recorded_at)
        .or(record.retired_at)
        .or_else(|| {
            runtime_skill_effective_lineage(record)
                .into_iter()
                .map(|node| node.recorded_at)
                .max()
        })
        .or(Some(record.observed_at).filter(|value| *value > 0))
        .or(Some(record.updated_at).filter(|value| *value > 0))
}

fn runtime_skill_doctrine_clause_from_record(
    record: RuntimeSkillRecord,
) -> Option<RuntimeSkillDoctrineClauseRecord> {
    if matches!(record.status, RuntimeSkillStatus::Retired) {
        return None;
    }
    if record.summary.trim().is_empty() {
        return None;
    }
    if record.validated_success_count == 0 && record.use_count == 0 {
        return None;
    }
    Some(RuntimeSkillDoctrineClauseRecord {
        source_skill_name: record.name,
        topic: record.topic,
        clause: truncate_content_to_max(record.summary.trim(), 180).into_owned(),
        validated_success_count: record.validated_success_count,
        revision_pending: record.revision_pending,
        evidence_ref_count: record.citations.len(),
    })
}

fn build_runtime_skill_strategy_diff(
    previous: &RuntimeSkillRecord,
    next: &RuntimeSkillRecord,
    from_node_id: &str,
    to_node_id: &str,
    recorded_at: u64,
) -> RuntimeSkillStrategyDiff {
    let summary_changed = normalize_runtime_skill_text(&previous.summary)
        != normalize_runtime_skill_text(&next.summary);
    let procedure_changed = normalize_runtime_skill_text(&previous.procedure)
        != normalize_runtime_skill_text(&next.procedure);
    let change_kind = match (summary_changed, procedure_changed) {
        (true, true) => RuntimeSkillStrategyDiffKind::DoctrineRevision,
        (false, true) => RuntimeSkillStrategyDiffKind::ProcedureRefinement,
        _ => RuntimeSkillStrategyDiffKind::SummaryRevision,
    };
    let summary = match change_kind {
        RuntimeSkillStrategyDiffKind::DoctrineRevision => {
            "summary and procedure changed under the same canonical strategy".to_string()
        }
        RuntimeSkillStrategyDiffKind::ProcedureRefinement => {
            "procedure changed while the canonical doctrine stayed stable".to_string()
        }
        RuntimeSkillStrategyDiffKind::SummaryRevision => {
            "summary changed while the procedure stayed stable".to_string()
        }
    };
    RuntimeSkillStrategyDiff {
        recorded_at,
        from_node_id: from_node_id.to_string(),
        to_node_id: to_node_id.to_string(),
        change_kind,
        summary,
    }
}

fn parse_runtime_skill_record(name: &str, content: &str) -> Option<RuntimeSkillRecord> {
    if !content.trim_start().starts_with(RUNTIME_SKILL_MARKER) {
        return None;
    }
    let mut lines = content.lines();
    let marker = lines.next()?;
    if marker.trim() != RUNTIME_SKILL_MARKER {
        return None;
    }
    let title = lines
        .next()
        .map(str::trim)
        .and_then(|line| line.strip_prefix('#'))
        .map(str::trim)
        .filter(|value| !value.is_empty())?
        .to_string();
    let mut meta = std::collections::HashMap::<String, String>::new();
    let mut section_lines = Vec::new();
    let mut seen_meta = false;
    for line in &mut lines {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if seen_meta {
                break;
            }
            continue;
        }
        if trimmed.starts_with("## ") {
            section_lines.push(line.to_string());
            break;
        }
        let Some((key, value)) = trimmed.split_once(':') else {
            continue;
        };
        seen_meta = true;
        meta.insert(key.trim().to_ascii_lowercase(), value.trim().to_string());
    }
    section_lines.extend(lines.map(str::to_string));
    let sections = collect_runtime_skill_sections(&section_lines.join("\n"));
    let topic = meta.get("topic")?.trim().to_string();
    let summary = sections
        .get("summary")
        .cloned()
        .unwrap_or_else(|| meta.get("summary").cloned().unwrap_or_default())
        .trim()
        .to_string();
    let procedure = sections
        .get("procedure")
        .cloned()
        .unwrap_or_default()
        .trim()
        .to_string();
    if topic.is_empty() || procedure.is_empty() {
        return None;
    }
    let citations = sections
        .get("provenance")
        .map(|value| {
            value
                .lines()
                .filter_map(|line| {
                    let trimmed = line.trim().trim_start_matches("- ").trim();
                    (!trimmed.is_empty()).then(|| trimmed.to_string())
                })
                .take(MAX_RUNTIME_SKILL_CITATIONS)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let source_chat_id = meta
        .get("source chat")
        .or_else(|| meta.get("source_chat"))
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let observed_at = meta
        .get("observed at")
        .or_else(|| meta.get("observed_at"))
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0);
    let updated_at = meta
        .get("updated at")
        .or_else(|| meta.get("updated_at"))
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(observed_at);
    let last_used_at = meta
        .get("last used at")
        .or_else(|| meta.get("last_used_at"))
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0);
    let retired_at = meta
        .get("retired at")
        .or_else(|| meta.get("retired_at"))
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0);
    let use_count = meta
        .get("use count")
        .or_else(|| meta.get("use_count"))
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(0);
    let quality_score = meta
        .get("quality")
        .and_then(|value| value.parse::<u8>().ok())
        .unwrap_or_else(|| {
            compute_runtime_skill_quality(&RuntimeSkillRecord {
                name: name.to_string(),
                title: title.clone(),
                topic: topic.clone(),
                summary: summary.clone(),
                procedure: procedure.clone(),
                citations: citations.clone(),
                source_chat_id: source_chat_id.clone(),
                observed_at,
                updated_at,
                last_used_at,
                use_count,
                quality_score: 0,
                status: RuntimeSkillStatus::Active,
                validated_success_count: meta
                    .get("validated success count")
                    .or_else(|| meta.get("validated_success_count"))
                    .and_then(|value| value.parse::<u32>().ok())
                    .unwrap_or(0),
                mismatch_count: meta
                    .get("mismatch count")
                    .or_else(|| meta.get("mismatch_count"))
                    .and_then(|value| value.parse::<u32>().ok())
                    .unwrap_or(0),
                revision_count: meta
                    .get("revision count")
                    .or_else(|| meta.get("revision_count"))
                    .and_then(|value| value.parse::<u32>().ok())
                    .unwrap_or(0),
                revision_pending: meta
                    .get("revision pending")
                    .or_else(|| meta.get("revision_pending"))
                    .map(|value| matches!(value.trim(), "true" | "1" | "yes"))
                    .unwrap_or(false),
                last_outcome_at: meta
                    .get("last outcome at")
                    .or_else(|| meta.get("last_outcome_at"))
                    .and_then(|value| value.parse::<u64>().ok())
                    .filter(|value| *value > 0),
                last_outcome_note: meta
                    .get("last outcome note")
                    .or_else(|| meta.get("last_outcome_note"))
                    .cloned()
                    .unwrap_or_default(),
                supersedes: Vec::new(),
                component_topics: vec![topic.clone()],
                genome_lineage: parse_runtime_skill_json_section(&sections, "genome lineage")
                    .unwrap_or_default(),
                strategy_diffs: parse_runtime_skill_json_section(&sections, "strategy diff ledger")
                    .unwrap_or_default(),
                retired_at,
                retirement_reason: meta
                    .get("retirement reason")
                    .or_else(|| meta.get("retirement_reason"))
                    .cloned()
                    .unwrap_or_default(),
            })
        });
    let mut component_topics = meta
        .get("components")
        .or_else(|| meta.get("component_topics"))
        .map(|value| parse_list_field(value))
        .unwrap_or_default();
    if !component_topics.iter().any(|candidate| candidate == &topic) {
        component_topics.push(topic.clone());
    }
    component_topics.sort();
    component_topics.dedup();
    Some(RuntimeSkillRecord {
        name: name.to_string(),
        title,
        topic,
        summary,
        procedure,
        citations,
        source_chat_id,
        observed_at,
        updated_at,
        last_used_at,
        use_count,
        quality_score,
        status: meta
            .get("status")
            .map(|value| RuntimeSkillStatus::parse(value))
            .unwrap_or(RuntimeSkillStatus::Active),
        validated_success_count: meta
            .get("validated success count")
            .or_else(|| meta.get("validated_success_count"))
            .and_then(|value| value.parse::<u32>().ok())
            .unwrap_or(0),
        mismatch_count: meta
            .get("mismatch count")
            .or_else(|| meta.get("mismatch_count"))
            .and_then(|value| value.parse::<u32>().ok())
            .unwrap_or(0),
        revision_count: meta
            .get("revision count")
            .or_else(|| meta.get("revision_count"))
            .and_then(|value| value.parse::<u32>().ok())
            .unwrap_or(0),
        revision_pending: meta
            .get("revision pending")
            .or_else(|| meta.get("revision_pending"))
            .map(|value| matches!(value.trim(), "true" | "1" | "yes"))
            .unwrap_or(false),
        last_outcome_at: meta
            .get("last outcome at")
            .or_else(|| meta.get("last_outcome_at"))
            .and_then(|value| value.parse::<u64>().ok())
            .filter(|value| *value > 0),
        last_outcome_note: meta
            .get("last outcome note")
            .or_else(|| meta.get("last_outcome_note"))
            .cloned()
            .unwrap_or_default(),
        supersedes: meta
            .get("supersedes")
            .map(|value| parse_list_field(value))
            .unwrap_or_default(),
        component_topics,
        genome_lineage: parse_runtime_skill_json_section(&sections, "genome lineage")
            .unwrap_or_default(),
        strategy_diffs: parse_runtime_skill_json_section(&sections, "strategy diff ledger")
            .unwrap_or_default(),
        retired_at,
        retirement_reason: meta
            .get("retirement reason")
            .or_else(|| meta.get("retirement_reason"))
            .cloned()
            .unwrap_or_default(),
    })
}

fn collect_runtime_skill_sections(content: &str) -> std::collections::HashMap<String, String> {
    let mut sections = std::collections::HashMap::new();
    let mut current_key: Option<String> = None;
    let mut buffer = String::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(heading) = trimmed.strip_prefix("## ") {
            if let Some(key) = current_key.take() {
                sections.insert(key, buffer.trim().to_string());
                buffer.clear();
            }
            current_key = Some(heading.trim().to_ascii_lowercase());
            continue;
        }
        if !buffer.is_empty() {
            buffer.push('\n');
        }
        buffer.push_str(line);
    }
    if let Some(key) = current_key {
        sections.insert(key, buffer.trim().to_string());
    }
    sections
}

fn merge_runtime_skill_record(
    existing: Option<&RuntimeSkillRecord>,
    input: RuntimeSkillUpsertInput,
) -> RuntimeSkillRecord {
    let RuntimeSkillUpsertInput {
        name,
        title,
        topic,
        summary,
        procedure,
        citations: input_citations,
        source_chat_id,
        observed_at,
        updated_at,
    } = input;
    let mut citations = existing
        .map(|record| record.citations.clone())
        .unwrap_or_default();
    for citation in input_citations {
        if citations.iter().any(|existing| existing == &citation) {
            continue;
        }
        citations.push(citation);
        if citations.len() >= MAX_RUNTIME_SKILL_CITATIONS {
            break;
        }
    }
    let mut record = RuntimeSkillRecord {
        name,
        title,
        topic: topic.clone(),
        summary,
        procedure,
        citations,
        source_chat_id,
        observed_at,
        updated_at,
        last_used_at: existing.and_then(|record| record.last_used_at),
        use_count: existing.map(|record| record.use_count).unwrap_or(0),
        quality_score: 0,
        status: existing
            .map(|record| record.status)
            .filter(|status| *status != RuntimeSkillStatus::Retired)
            .unwrap_or(RuntimeSkillStatus::Active),
        validated_success_count: existing
            .map(|record| record.validated_success_count)
            .unwrap_or(0),
        mismatch_count: existing.map(|record| record.mismatch_count).unwrap_or(0),
        revision_count: existing.map(|record| record.revision_count).unwrap_or(0),
        revision_pending: existing
            .map(|record| record.revision_pending)
            .unwrap_or(false),
        last_outcome_at: existing.and_then(|record| record.last_outcome_at),
        last_outcome_note: existing
            .map(|record| record.last_outcome_note.clone())
            .unwrap_or_default(),
        supersedes: existing
            .map(|record| record.supersedes.clone())
            .unwrap_or_default(),
        component_topics: existing
            .map(|record| record.component_topics.clone())
            .unwrap_or_else(|| vec![topic]),
        genome_lineage: existing
            .map(runtime_skill_effective_lineage)
            .unwrap_or_default(),
        strategy_diffs: existing
            .map(|record| record.strategy_diffs.clone())
            .unwrap_or_default(),
        retired_at: None,
        retirement_reason: String::new(),
    };
    if let Some(existing) = existing {
        if record.summary.is_empty() {
            record.summary = existing.summary.clone();
        }
        if record.source_chat_id.is_none() {
            record.source_chat_id = existing.source_chat_id.clone();
        }
        if record.observed_at == 0 {
            record.observed_at = existing.observed_at;
        }
        record.updated_at = record.updated_at.max(existing.updated_at);
        for topic in &existing.component_topics {
            if !record
                .component_topics
                .iter()
                .any(|candidate| candidate == topic)
            {
                record.component_topics.push(topic.clone());
            }
        }
        if !record
            .component_topics
            .iter()
            .any(|candidate| candidate == &record.topic)
        {
            record.component_topics.push(record.topic.clone());
        }
        if normalized_runtime_skill_identity(existing) == normalized_runtime_skill_identity(&record)
            && normalize_runtime_skill_text(&existing.procedure)
                == normalize_runtime_skill_text(&record.procedure)
        {
            record.summary = if record.summary.is_empty() {
                existing.summary.clone()
            } else {
                record.summary
            };
        }
    }
    if !record
        .component_topics
        .iter()
        .any(|candidate| candidate == &record.topic)
    {
        record.component_topics.push(record.topic.clone());
    }
    record.component_topics.sort();
    record.component_topics.dedup();
    record.supersedes.sort();
    record.supersedes.dedup();
    let next_node = build_runtime_skill_genome_node(
        &record.name,
        &record.summary,
        &record.procedure,
        record.updated_at.max(record.observed_at),
        RuntimeSkillGenomeDisposition::Active,
    );
    if let Some(existing) = existing {
        let existing_lineage = runtime_skill_effective_lineage(existing);
        let previous_node = existing_lineage.last().cloned().unwrap_or_else(|| {
            build_runtime_skill_genome_node(
                &existing.name,
                &existing.summary,
                &existing.procedure,
                existing.updated_at.max(existing.observed_at),
                match existing.status {
                    RuntimeSkillStatus::Retired => RuntimeSkillGenomeDisposition::Retired,
                    _ => RuntimeSkillGenomeDisposition::Active,
                },
            )
        });
        if previous_node.strategy_digest == next_node.strategy_digest {
            if record.genome_lineage.is_empty() {
                record.genome_lineage.push(next_node);
            } else if let Some(last) = record.genome_lineage.last_mut() {
                last.disposition = RuntimeSkillGenomeDisposition::Active;
                last.recorded_at = last
                    .recorded_at
                    .max(record.updated_at.max(record.observed_at));
                last.summary = runtime_skill_summary_excerpt(&record.summary, &record.procedure);
            }
        } else {
            if let Some(last) = record.genome_lineage.last_mut() {
                if last.disposition == RuntimeSkillGenomeDisposition::Active {
                    last.disposition = RuntimeSkillGenomeDisposition::Superseded;
                }
            }
            record.genome_lineage.push(next_node.clone());
            record
                .strategy_diffs
                .push(build_runtime_skill_strategy_diff(
                    existing,
                    &record,
                    &previous_node.node_id,
                    &next_node.node_id,
                    record.updated_at.max(record.observed_at),
                ));
        }
    } else {
        record.genome_lineage.push(next_node);
    }
    if record.genome_lineage.is_empty() {
        record.genome_lineage.push(build_runtime_skill_genome_node(
            &record.name,
            &record.summary,
            &record.procedure,
            record.updated_at.max(record.observed_at),
            RuntimeSkillGenomeDisposition::Active,
        ));
    }
    if record.genome_lineage.len() > MAX_RUNTIME_SKILL_GENOME_NODES {
        let drain = record.genome_lineage.len() - MAX_RUNTIME_SKILL_GENOME_NODES;
        record.genome_lineage.drain(0..drain);
    }
    if record.strategy_diffs.len() > MAX_RUNTIME_SKILL_STRATEGY_DIFFS {
        let drain = record.strategy_diffs.len() - MAX_RUNTIME_SKILL_STRATEGY_DIFFS;
        record.strategy_diffs.drain(0..drain);
    }
    record.quality_score = compute_runtime_skill_quality(&record);
    record
}

fn normalized_runtime_skill_identity(record: &RuntimeSkillRecord) -> String {
    normalize_runtime_skill_text(&format!("{} {}", record.topic, record.title))
}

fn render_runtime_skill_record(record: &RuntimeSkillRecord) -> String {
    let mut out = String::new();
    out.push_str(RUNTIME_SKILL_MARKER);
    out.push('\n');
    out.push_str("# ");
    out.push_str(record.title.trim());
    out.push_str("\n\n");
    out.push_str("Type: procedural_runtime_skill\n");
    out.push_str("Topic: ");
    out.push_str(record.topic.trim());
    out.push('\n');
    if let Some(chat_id) = record.source_chat_id.as_deref() {
        out.push_str("Source chat: ");
        out.push_str(chat_id);
        out.push('\n');
    }
    out.push_str("Status: ");
    out.push_str(record.status.label());
    out.push('\n');
    if record.observed_at > 0 {
        out.push_str("Observed at: ");
        out.push_str(&record.observed_at.to_string());
        out.push('\n');
    }
    if record.updated_at > 0 {
        out.push_str("Updated at: ");
        out.push_str(&record.updated_at.to_string());
        out.push('\n');
    }
    if let Some(last_used_at) = record.last_used_at.filter(|value| *value > 0) {
        out.push_str("Last used at: ");
        out.push_str(&last_used_at.to_string());
        out.push('\n');
    }
    out.push_str("Use count: ");
    out.push_str(&record.use_count.to_string());
    out.push('\n');
    out.push_str("Quality: ");
    out.push_str(&record.quality_score.to_string());
    out.push('\n');
    out.push_str("Validated success count: ");
    out.push_str(&record.validated_success_count.to_string());
    out.push('\n');
    out.push_str("Mismatch count: ");
    out.push_str(&record.mismatch_count.to_string());
    out.push('\n');
    out.push_str("Revision count: ");
    out.push_str(&record.revision_count.to_string());
    out.push('\n');
    out.push_str("Revision pending: ");
    out.push_str(if record.revision_pending {
        "true"
    } else {
        "false"
    });
    out.push('\n');
    if let Some(last_outcome_at) = record.last_outcome_at.filter(|value| *value > 0) {
        out.push_str("Last outcome at: ");
        out.push_str(&last_outcome_at.to_string());
        out.push('\n');
    }
    if !record.last_outcome_note.trim().is_empty() {
        out.push_str("Last outcome note: ");
        out.push_str(record.last_outcome_note.trim());
        out.push('\n');
    }
    if let Some(retired_at) = record.retired_at.filter(|value| *value > 0) {
        out.push_str("Retired at: ");
        out.push_str(&retired_at.to_string());
        out.push('\n');
    }
    if !record.retirement_reason.trim().is_empty() {
        out.push_str("Retirement reason: ");
        out.push_str(record.retirement_reason.trim());
        out.push('\n');
    }
    if !record.supersedes.is_empty() {
        out.push_str("Supersedes: ");
        out.push_str(&record.supersedes.join(", "));
        out.push('\n');
    }
    if !record.component_topics.is_empty() {
        out.push_str("Components: ");
        out.push_str(&record.component_topics.join(", "));
        out.push('\n');
    }
    out.push_str("\n## Summary\n");
    out.push_str(record.summary.trim());
    out.push_str("\n\n## Procedure\n");
    out.push_str(record.procedure.trim());
    if !record.citations.is_empty() {
        out.push_str("\n\n## Provenance\n");
        for citation in &record.citations {
            out.push_str("- ");
            out.push_str(citation.trim());
            out.push('\n');
        }
    }
    if !record.genome_lineage.is_empty() {
        out.push_str("\n\n## Genome lineage\n");
        if let Ok(value) = serde_json::to_string_pretty(&record.genome_lineage) {
            out.push_str(&value);
        }
    }
    if !record.strategy_diffs.is_empty() {
        out.push_str("\n\n## Strategy diff ledger\n");
        if let Ok(value) = serde_json::to_string_pretty(&record.strategy_diffs) {
            out.push_str(&value);
        }
    }
    out
}

fn compute_runtime_skill_quality(record: &RuntimeSkillRecord) -> u8 {
    let summary_signal = u8::from(!record.summary.trim().is_empty()) * 18;
    let provenance_signal = (record.citations.len().min(4) as u8).saturating_mul(10);
    let reuse_signal = (record.use_count.min(5) as u8).saturating_mul(6);
    let validated_signal = (record.validated_success_count.min(4) as u8).saturating_mul(5);
    let structure_signal = u8::from(record.procedure.lines().count() >= 2) * 14;
    let chat_signal = u8::from(record.source_chat_id.is_some()) * 6;
    let penalty = (record.mismatch_count.min(3) as u8)
        .saturating_mul(4)
        .saturating_add(u8::from(record.revision_pending) * 6);
    20u8.saturating_add(summary_signal)
        .saturating_add(provenance_signal)
        .saturating_add(reuse_signal)
        .saturating_add(validated_signal)
        .saturating_add(structure_signal)
        .saturating_add(chat_signal)
        .saturating_sub(penalty)
        .min(100)
}

fn score_runtime_skill_record(
    record: RuntimeSkillRecord,
    normalized_query: &str,
    terms: &[String],
    index_hint: Option<&RuntimeSkillIndexHint>,
    preferred_chat_id: Option<&str>,
    now_secs: u64,
) -> Option<RuntimeSkillHit> {
    let breakdown = score_runtime_skill_record_breakdown(
        &record,
        normalized_query,
        terms,
        index_hint,
        preferred_chat_id,
        now_secs,
    )?;
    Some(RuntimeSkillHit {
        record,
        score: breakdown.total_score,
        reasons: breakdown.reason_fragments.clone(),
        score_breakdown: breakdown,
    })
}

fn score_runtime_skill_record_breakdown(
    record: &RuntimeSkillRecord,
    normalized_query: &str,
    terms: &[String],
    index_hint: Option<&RuntimeSkillIndexHint>,
    preferred_chat_id: Option<&str>,
    now_secs: u64,
) -> Option<RuntimeSkillRecallScoreBreakdown> {
    if matches!(
        record.status,
        RuntimeSkillStatus::LowValue | RuntimeSkillStatus::Retired
    ) && runtime_skill_is_stale(record, now_secs)
        && record.use_count == 0
    {
        return None;
    }
    let haystack = normalize_runtime_skill_text(&format!(
        "{} {} {}",
        record.topic, record.summary, record.procedure
    ));
    if haystack.is_empty() {
        return None;
    }
    let normalized_title = normalize_runtime_skill_text(&record.title);
    let normalized_topic = normalize_runtime_skill_text(&record.topic);
    let normalized_summary = normalize_runtime_skill_text(&record.summary);
    let mut lexical_score = 0u32;
    let mut exact_match_score = 0u32;
    let mut reasons = Vec::new();
    if normalized_query == normalized_topic
        || (!normalized_topic.is_empty()
            && (normalized_query.contains(&normalized_topic)
                || normalized_topic.contains(normalized_query)))
    {
        exact_match_score = exact_match_score.saturating_add(14);
        reasons.push("exact topic overlap".to_string());
    }
    if normalized_query == normalized_title {
        exact_match_score = exact_match_score.saturating_add(10);
        reasons.push("exact title overlap".to_string());
    }
    for term in terms {
        if normalized_topic.contains(term) {
            lexical_score = lexical_score.saturating_add(10);
        }
        if normalized_title.contains(term) {
            lexical_score = lexical_score.saturating_add(8);
        }
        if normalized_summary.contains(term) {
            lexical_score = lexical_score.saturating_add(5);
        }
        if haystack.contains(term) {
            lexical_score = lexical_score.saturating_add(3);
        }
    }
    if lexical_score > 0 {
        reasons.push("term overlap".to_string());
    }
    let semantic_score = trigram_overlap_score(normalized_query, &haystack, 18);
    let indexed_semantic_bonus = index_hint.map(|hint| hint.semantic_bonus).unwrap_or(0);
    let semantic_score = semantic_score.saturating_add(indexed_semantic_bonus);
    if semantic_score > 0 {
        reasons.push("semantic overlap".to_string());
    }
    if let Some(hint) = index_hint {
        reasons.extend(hint.reasons.iter().cloned());
    }
    let scope_affinity_score = preferred_chat_id
        .filter(|chat_id| record.source_chat_id.as_deref() == Some(*chat_id))
        .map(|_| 6)
        .unwrap_or(0);
    if scope_affinity_score > 0 {
        reasons.push("same-chat provenance".to_string());
    }
    let recency_score = record
        .last_used_at
        .map(|last_used_at| {
            let age = now_secs.saturating_sub(last_used_at);
            if age <= 7 * 86_400 {
                6
            } else if age <= 30 * 86_400 {
                3
            } else {
                0
            }
        })
        .unwrap_or(0);
    if recency_score > 0 {
        reasons.push("recently reused".to_string());
    }
    if record.validated_success_count > 0 {
        reasons.push("validated reuse evidence".to_string());
    }
    if record.revision_pending {
        reasons.push("revision pending".to_string());
    }
    let confidence_score =
        (record.quality_score / 8) as u32 + runtime_skill_validated_bonus(record);
    let importance_score = record.use_count.min(6).saturating_mul(2);
    let source_score = if record.citations.is_empty() {
        0
    } else {
        record.citations.len().min(3) as u32 * 2
    };
    if source_score > 0 {
        reasons.push(format!("{} provenance refs", record.citations.len()));
    }
    let governance_score = runtime_skill_governance_score(
        record,
        match record.status {
            RuntimeSkillStatus::Active => 6,
            RuntimeSkillStatus::Stale => 1,
            RuntimeSkillStatus::LowValue | RuntimeSkillStatus::Retired => 0,
        },
    );
    if runtime_skill_is_stale(record, now_secs) {
        reasons.push("stale".to_string());
    }
    if matches!(
        record.status,
        RuntimeSkillStatus::LowValue | RuntimeSkillStatus::Retired
    ) {
        reasons.push("low-value".to_string());
    }
    let total_score = lexical_score
        .saturating_add(semantic_score)
        .saturating_add(exact_match_score)
        .saturating_add(recency_score)
        .saturating_add(confidence_score)
        .saturating_add(importance_score)
        .saturating_add(scope_affinity_score)
        .saturating_add(governance_score)
        .saturating_add(source_score);
    (total_score > 0).then_some(RuntimeSkillRecallScoreBreakdown {
        lexical_score,
        semantic_score,
        exact_match_score,
        recency_score,
        confidence_score,
        importance_score,
        scope_affinity_score,
        governance_score,
        source_score,
        total_score,
        reason_fragments: reasons,
    })
}

fn runtime_skill_validated_bonus(record: &RuntimeSkillRecord) -> u32 {
    record.validated_success_count.min(3).saturating_mul(4)
}

fn runtime_skill_governance_score(record: &RuntimeSkillRecord, base: u32) -> u32 {
    let penalty = u32::from(record.revision_pending).saturating_mul(5)
        + record.mismatch_count.min(2).saturating_mul(2);
    base.saturating_sub(penalty)
}

fn runtime_skill_is_stale(record: &RuntimeSkillRecord, now_secs: u64) -> bool {
    let freshness_anchor = record
        .last_used_at
        .unwrap_or(record.updated_at.max(record.observed_at));
    now_secs > 0 && now_secs.saturating_sub(freshness_anchor) > RUNTIME_SKILL_STALE_AFTER_SECS
}

fn build_runtime_skill_summary(procedure: &str) -> String {
    procedure
        .lines()
        .find_map(|line| {
            let trimmed = line.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        })
        .unwrap_or_else(|| truncate_content_to_max(procedure.trim(), 96).to_string())
}

fn normalize_citations(citations: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for citation in citations {
        let trimmed = citation.trim();
        if trimmed.is_empty() || out.iter().any(|existing| existing == trimmed) {
            continue;
        }
        out.push(trimmed.to_string());
        if out.len() >= MAX_RUNTIME_SKILL_CITATIONS {
            break;
        }
    }
    out
}

fn parse_list_field(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect()
}

fn find_canonical_runtime_skill_name(
    records: &[RuntimeSkillRecord],
    input: &RuntimeSkillUpsertInput,
) -> Option<String> {
    let probe = RuntimeSkillRecord {
        name: input.name.clone(),
        title: input.title.clone(),
        topic: input.topic.clone(),
        summary: input.summary.clone(),
        procedure: input.procedure.clone(),
        citations: input.citations.clone(),
        source_chat_id: input.source_chat_id.clone(),
        observed_at: input.observed_at,
        updated_at: input.updated_at,
        last_used_at: None,
        use_count: 0,
        quality_score: 0,
        status: RuntimeSkillStatus::Active,
        validated_success_count: 0,
        mismatch_count: 0,
        revision_count: 0,
        revision_pending: false,
        last_outcome_at: None,
        last_outcome_note: String::new(),
        supersedes: Vec::new(),
        component_topics: vec![input.topic.clone()],
        genome_lineage: Vec::new(),
        strategy_diffs: Vec::new(),
        retired_at: None,
        retirement_reason: String::new(),
    };
    records
        .iter()
        .filter(|record| {
            runtime_skill_similarity(record, &probe) >= RUNTIME_SKILL_DUPLICATE_SIMILARITY
        })
        .max_by_key(|record| runtime_skill_rank(record))
        .map(|record| record.name.clone())
}

fn runtime_skill_similarity(left: &RuntimeSkillRecord, right: &RuntimeSkillRecord) -> u32 {
    let left_id = normalize_runtime_skill_text(&format!("{} {}", left.topic, left.title));
    let right_id = normalize_runtime_skill_text(&format!("{} {}", right.topic, right.title));
    if left_id == right_id {
        return 32;
    }
    trigram_overlap_score(&left_id, &right_id, 24)
        .saturating_add(u32::from(left_id.contains(&right_id) || right_id.contains(&left_id)) * 12)
}

fn runtime_skill_rank(record: &RuntimeSkillRecord) -> u32 {
    (record.quality_score as u32)
        .saturating_add(record.use_count.saturating_mul(3))
        .saturating_add(record.citations.len() as u32 * 2)
        .saturating_add(u32::from(record.status == RuntimeSkillStatus::Active) * 6)
        .saturating_add((record.updated_at / 86_400) as u32)
}

fn select_canonical_runtime_skill_index(group: &[RuntimeSkillRecord]) -> usize {
    let mut best_idx = 0usize;
    let mut best_rank = 0u32;
    for (idx, record) in group.iter().enumerate() {
        let rank = runtime_skill_rank(record);
        if idx == 0 || rank > best_rank {
            best_idx = idx;
            best_rank = rank;
        }
    }
    best_idx
}

fn merge_runtime_skill_group(
    mut group: Vec<RuntimeSkillRecord>,
    canonical_idx: usize,
) -> RuntimeSkillRecord {
    let mut canonical = group.swap_remove(canonical_idx);
    for duplicate in group {
        let duplicate_lineage = runtime_skill_effective_lineage(&duplicate);
        let duplicate_strategy_diffs = duplicate.strategy_diffs.clone();
        let duplicate_source_chat_id = duplicate.source_chat_id.clone();
        if duplicate.name != canonical.name
            && !canonical
                .supersedes
                .iter()
                .any(|existing| existing == &duplicate.name)
        {
            canonical.supersedes.push(duplicate.name.clone());
        }
        for topic in duplicate
            .component_topics
            .iter()
            .chain(std::iter::once(&duplicate.topic))
        {
            if !canonical
                .component_topics
                .iter()
                .any(|existing| existing == topic)
            {
                canonical.component_topics.push(topic.clone());
            }
        }
        for citation in duplicate.citations {
            if !canonical
                .citations
                .iter()
                .any(|existing| existing == &citation)
            {
                canonical.citations.push(citation);
                canonical.citations.truncate(MAX_RUNTIME_SKILL_CITATIONS);
            }
        }
        canonical.use_count = canonical.use_count.saturating_add(duplicate.use_count);
        canonical.last_used_at = canonical.last_used_at.max(duplicate.last_used_at);
        canonical.observed_at = canonical.observed_at.max(duplicate.observed_at);
        canonical.updated_at = canonical.updated_at.max(duplicate.updated_at);
        if duplicate.quality_score > canonical.quality_score
            && duplicate.summary.len() > canonical.summary.len()
        {
            canonical.summary = duplicate.summary;
        }
        if duplicate.procedure.lines().count() > canonical.procedure.lines().count() {
            canonical.procedure = duplicate.procedure;
        }
        for mut node in duplicate_lineage {
            if node.disposition == RuntimeSkillGenomeDisposition::Active {
                node.disposition = RuntimeSkillGenomeDisposition::Superseded;
            }
            if canonical
                .genome_lineage
                .iter()
                .all(|existing| existing.node_id != node.node_id)
            {
                canonical.genome_lineage.push(node);
            }
        }
        for diff in duplicate_strategy_diffs {
            if canonical.strategy_diffs.iter().all(|existing| {
                existing.from_node_id != diff.from_node_id || existing.to_node_id != diff.to_node_id
            }) {
                canonical.strategy_diffs.push(diff);
            }
        }
        if canonical.source_chat_id.is_none() {
            canonical.source_chat_id = duplicate_source_chat_id;
        }
    }
    canonical.component_topics.sort();
    canonical.component_topics.dedup();
    canonical.supersedes.sort();
    canonical.supersedes.dedup();
    canonical.genome_lineage.sort_by(|a, b| {
        a.recorded_at
            .cmp(&b.recorded_at)
            .then_with(|| a.node_id.cmp(&b.node_id))
    });
    if canonical.genome_lineage.len() > MAX_RUNTIME_SKILL_GENOME_NODES {
        let drain = canonical.genome_lineage.len() - MAX_RUNTIME_SKILL_GENOME_NODES;
        canonical.genome_lineage.drain(0..drain);
    }
    canonical.strategy_diffs.sort_by(|a, b| {
        a.recorded_at
            .cmp(&b.recorded_at)
            .then_with(|| a.to_node_id.cmp(&b.to_node_id))
    });
    if canonical.strategy_diffs.len() > MAX_RUNTIME_SKILL_STRATEGY_DIFFS {
        let drain = canonical.strategy_diffs.len() - MAX_RUNTIME_SKILL_STRATEGY_DIFFS;
        canonical.strategy_diffs.drain(0..drain);
    }
    canonical.quality_score = compute_runtime_skill_quality(&canonical);
    canonical
}

fn apply_runtime_skill_status(
    mut record: RuntimeSkillRecord,
    now_secs: u64,
    outcome: &mut RuntimeSkillGovernanceOutcome,
) -> RuntimeSkillRecord {
    let stale = runtime_skill_is_stale(&record, now_secs);
    let has_preservable_lineage = record.validated_success_count > 0
        || record.genome_lineage.len() > 1
        || !record.strategy_diffs.is_empty()
        || !record.supersedes.is_empty();
    let next_status = if stale && has_preservable_lineage && record.use_count == 0 {
        RuntimeSkillStatus::Retired
    } else if stale && record.quality_score < 45 && record.use_count == 0 {
        RuntimeSkillStatus::LowValue
    } else if stale {
        RuntimeSkillStatus::Stale
    } else if record.quality_score < 32 && record.use_count == 0 {
        RuntimeSkillStatus::LowValue
    } else {
        RuntimeSkillStatus::Active
    };
    if next_status != record.status {
        match next_status {
            RuntimeSkillStatus::Stale => {
                outcome.stale_marked = outcome.stale_marked.saturating_add(1)
            }
            RuntimeSkillStatus::LowValue => {
                outcome.low_value_marked = outcome.low_value_marked.saturating_add(1)
            }
            RuntimeSkillStatus::Retired => {
                outcome.retired_marked = outcome.retired_marked.saturating_add(1)
            }
            RuntimeSkillStatus::Active => {}
        }
    }
    record.status = next_status;
    match next_status {
        RuntimeSkillStatus::Retired => {
            record.retired_at = Some(now_secs);
            record.retirement_reason =
                "retired after proven lineage aged out of the active working set".to_string();
            if let Some(last) = record.genome_lineage.last_mut() {
                last.disposition = RuntimeSkillGenomeDisposition::Retired;
                last.recorded_at = now_secs;
            }
        }
        _ => {
            record.retired_at = None;
            record.retirement_reason.clear();
            if let Some(last) = record.genome_lineage.last_mut() {
                if last.disposition == RuntimeSkillGenomeDisposition::Retired {
                    last.disposition = RuntimeSkillGenomeDisposition::Active;
                }
            }
        }
    }
    record
}

fn should_prune_runtime_skill(record: &RuntimeSkillRecord, now_secs: u64) -> bool {
    matches!(record.status, RuntimeSkillStatus::LowValue)
        && runtime_skill_is_stale(record, now_secs)
        && record.use_count == 0
        && record.citations.len() <= 1
}

fn inspect_runtime_skill_write_shape(
    write: &RuntimeSkillWrite,
) -> std::result::Result<(), RuntimeSkillWriteItemReport> {
    let topic = write.topic.trim().to_string();
    let content = write.content.trim();
    if topic.is_empty() || content.is_empty() {
        return Err(RuntimeSkillWriteItemReport {
            source: RuntimeSkillWriteSource::Manual,
            action: RuntimeSkillWriteAction::Rejected,
            reason: RuntimeSkillWriteReason::EmptyOrInvalid,
            topic,
            detail: "runtime skill write requires non-empty topic and procedure content"
                .to_string(),
        });
    }
    if looks_like_raw_payload_text(content) {
        return Err(RuntimeSkillWriteItemReport {
            source: RuntimeSkillWriteSource::Manual,
            action: RuntimeSkillWriteAction::Rejected,
            reason: RuntimeSkillWriteReason::RawPayloadOrLog,
            topic,
            detail: "runtime skill write rejected raw payload / log shaped content".to_string(),
        });
    }
    let signal = procedural_text_signal_count(content);
    let non_empty_lines = content
        .lines()
        .filter(|line| !line.trim().is_empty())
        .count();
    let has_summary = !write.summary.trim().is_empty();
    if signal < 2 && !(signal >= 1 && non_empty_lines >= 2 && has_summary) {
        return Err(RuntimeSkillWriteItemReport {
            source: RuntimeSkillWriteSource::Manual,
            action: RuntimeSkillWriteAction::Rejected,
            reason: RuntimeSkillWriteReason::WeakProcedure,
            topic,
            detail: "runtime skill write requires reusable procedure structure, not a bare factual sentence".to_string(),
        });
    }
    Ok(())
}

fn build_runtime_skill_composition_line(
    hits: &[RuntimeSkillHit],
    query: &str,
    max_chars: usize,
) -> Option<String> {
    if hits.len() < 2 {
        return None;
    }
    let normalized_query = normalize_runtime_skill_text(query);
    let first = &hits[0];
    let second = hits
        .iter()
        .skip(1)
        .find(|candidate| candidate.record.topic != first.record.topic)?;
    let query_terms = collect_runtime_skill_terms(&normalized_query);
    let first_overlap = query_terms
        .iter()
        .filter(|term| normalize_runtime_skill_text(&first.record.summary).contains(term.as_str()))
        .count();
    let second_overlap = query_terms
        .iter()
        .filter(|term| normalize_runtime_skill_text(&second.record.summary).contains(term.as_str()))
        .count();
    if first_overlap == 0 || second_overlap == 0 {
        return None;
    }
    let line = format!(
        "- [Composition] Combine {} then {} for this turn when both setup and verification are needed.",
        first.record.title, second.record.title
    );
    (line.len() <= max_chars / 2).then_some(line)
}

fn normalize_runtime_skill_text(input: &str) -> String {
    normalize_retrieval_text(input)
}

fn collect_runtime_skill_terms(normalized_query: &str) -> Vec<String> {
    collect_retrieval_terms(normalized_query, 2, 24, &[2, 3])
}

fn write_runtime_skill_record(
    storage: &dyn SkillStorage,
    record: &RuntimeSkillRecord,
) -> crate::error::Result<()> {
    write_skill(storage, &record.name, &render_runtime_skill_record(record))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::{Error, Result};
    use std::collections::HashMap;
    use std::sync::Mutex;

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
            self.files
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(name)
                .cloned()
                .ok_or_else(|| Error::config("skill", "missing"))
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

    fn runtime_skill_record(
        name: &str,
        topic: &str,
        title: &str,
        summary: &str,
        procedure: &str,
        observed_at: u64,
    ) -> RuntimeSkillRecord {
        RuntimeSkillRecord {
            name: name.to_string(),
            title: title.to_string(),
            topic: topic.to_string(),
            summary: summary.to_string(),
            procedure: procedure.to_string(),
            citations: Vec::new(),
            source_chat_id: Some("chat-1".to_string()),
            observed_at,
            updated_at: observed_at,
            last_used_at: None,
            use_count: 0,
            quality_score: 0,
            status: RuntimeSkillStatus::Active,
            validated_success_count: 0,
            mismatch_count: 0,
            revision_count: 0,
            revision_pending: false,
            last_outcome_at: None,
            last_outcome_note: String::new(),
            supersedes: Vec::new(),
            component_topics: vec![topic.to_string()],
            genome_lineage: Vec::new(),
            strategy_diffs: Vec::new(),
            retired_at: None,
            retirement_reason: String::new(),
        }
    }

    #[test]
    fn record_runtime_skill_outcome_marks_validated_and_revision_pending_states() {
        let storage = StubSkillStorage::default();
        upsert_runtime_skill(
            &storage,
            &RuntimeSkillWrite {
                name: String::new(),
                topic: "release_patch_flow".to_string(),
                title: "Release patch flow".to_string(),
                summary: "Apply the release patch safely.".to_string(),
                content: "1. inspect diff\n2. patch\n3. verify".to_string(),
                citations: Vec::new(),
                source_chat_id: Some("chat-1".to_string()),
                observed_at: 100,
            },
        )
        .unwrap();

        record_runtime_skill_outcomes(
            &storage,
            &[String::from("runtime_skill__release_patch_flow")],
            RuntimeSkillReuseOutcome::Succeeded,
            200,
            "final_answer",
        )
        .unwrap();
        record_runtime_skill_outcomes(
            &storage,
            &[String::from("runtime_skill__release_patch_flow")],
            RuntimeSkillReuseOutcome::Mismatch,
            260,
            "surface_finalization",
        )
        .unwrap();

        let record = parse_runtime_skill_record(
            "runtime_skill__release_patch_flow",
            &get_skill_content(&storage, "runtime_skill__release_patch_flow").unwrap(),
        )
        .unwrap();
        assert_eq!(record.validated_success_count, 1);
        assert_eq!(record.mismatch_count, 1);
        assert_eq!(record.revision_count, 1);
        assert!(record.revision_pending);
        assert_eq!(record.last_outcome_at, Some(260));
        assert_eq!(record.last_outcome_note, "surface_finalization");
    }

    #[test]
    fn runtime_skill_recall_prefers_exact_topic_and_provenance() {
        let storage = StubSkillStorage::default();
        let changed = upsert_runtime_skill(
            &storage,
            &RuntimeSkillWrite {
                name: String::new(),
                topic: "network_setup".to_string(),
                title: "Network setup".to_string(),
                summary: "Bring up Wi-Fi and verify logs".to_string(),
                content: "- connect wifi\n- check /tmp/log".to_string(),
                citations: vec!["transcript:chat-1#message=2".to_string()],
                source_chat_id: Some("chat-1".to_string()),
                observed_at: 100,
            },
        )
        .unwrap();
        assert!(changed);

        let hits =
            retrieve_runtime_skill_hits(&storage, "继续 network setup", Some("chat-1"), 200, 3);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].record.topic, "network_setup");
        assert!(hits[0]
            .reasons
            .iter()
            .any(|reason| reason.contains("exact topic")));
    }

    #[test]
    fn runtime_skill_recall_prefers_validated_skill_and_surfaces_learning_reason() {
        let storage = StubSkillStorage::default();
        upsert_runtime_skill(
            &storage,
            &RuntimeSkillWrite {
                name: String::new(),
                topic: "release_patch_flow".to_string(),
                title: "Release patch flow".to_string(),
                summary: "Validated release patch procedure.".to_string(),
                content: "1. inspect diff\n2. patch\n3. verify".to_string(),
                citations: Vec::new(),
                source_chat_id: Some("chat-1".to_string()),
                observed_at: 100,
            },
        )
        .unwrap();
        upsert_runtime_skill(
            &storage,
            &RuntimeSkillWrite {
                name: "runtime_skill__release_patch_fallback".to_string(),
                topic: "release_patch_flow".to_string(),
                title: "Release patch fallback".to_string(),
                summary: "Fresh promoted procedure.".to_string(),
                content: "1. inspect diff\n2. patch\n3. verify".to_string(),
                citations: Vec::new(),
                source_chat_id: Some("chat-1".to_string()),
                observed_at: 120,
            },
        )
        .unwrap();
        record_runtime_skill_outcomes(
            &storage,
            &[String::from("runtime_skill__release_patch_flow")],
            RuntimeSkillReuseOutcome::Succeeded,
            200,
            "final_answer",
        )
        .unwrap();

        let hits = retrieve_runtime_skill_hits(
            &storage,
            "继续按 release patch flow 做",
            Some("chat-1"),
            300,
            3,
        );

        assert_eq!(hits[0].record.name, "runtime_skill__release_patch_flow");
        assert!(hits[0]
            .reasons
            .iter()
            .any(|reason| reason.contains("validated reuse")));
    }

    #[test]
    fn governed_runtime_skill_write_rejects_weak_procedure() {
        let storage = StubSkillStorage::default();
        let outcome = write_governed_runtime_skills(
            &storage,
            &[RuntimeSkillWrite {
                name: String::new(),
                topic: "owner_timezone".to_string(),
                title: "Owner timezone".to_string(),
                summary: "Timezone note".to_string(),
                content: "Owner timezone is Asia/Shanghai.".to_string(),
                citations: Vec::new(),
                source_chat_id: Some("chat-1".to_string()),
                observed_at: 100,
            }],
            RuntimeSkillWriteSource::Extraction,
        )
        .unwrap();
        assert_eq!(outcome.accepted, 0);
        assert_eq!(outcome.rejected, 1);
        assert_eq!(
            outcome.reports[0].reason,
            RuntimeSkillWriteReason::WeakProcedure
        );
        assert!(storage.list_names().unwrap().is_empty());
    }

    #[test]
    fn governed_runtime_skill_write_accepts_structured_procedure() {
        let storage = StubSkillStorage::default();
        let outcome = write_governed_runtime_skills(
            &storage,
            &[RuntimeSkillWrite {
                name: String::new(),
                topic: "release_patch_flow".to_string(),
                title: "Release patch flow".to_string(),
                summary: "Patch the release and verify the result".to_string(),
                content: "1. inspect release diff\n2. patch rollback guards\n3. verify logs"
                    .to_string(),
                citations: Vec::new(),
                source_chat_id: Some("chat-1".to_string()),
                observed_at: 100,
            }],
            RuntimeSkillWriteSource::TaskLearning,
        )
        .unwrap();
        assert_eq!(outcome.accepted, 1);
        assert_eq!(outcome.changed, 1);
        assert!(storage
            .list_names()
            .unwrap()
            .iter()
            .any(|name| name == "runtime_skill__release_patch_flow"));
    }

    #[test]
    fn governed_runtime_skill_write_regression_suite_covers_reason_matrix() {
        let weak_storage = StubSkillStorage::default();
        let weak = write_governed_runtime_skills(
            &weak_storage,
            &[RuntimeSkillWrite {
                name: String::new(),
                topic: "owner_timezone".to_string(),
                title: "Owner timezone".to_string(),
                summary: "Timezone note".to_string(),
                content: "Owner timezone is Asia/Shanghai.".to_string(),
                citations: Vec::new(),
                source_chat_id: Some("chat-1".to_string()),
                observed_at: 100,
            }],
            RuntimeSkillWriteSource::Extraction,
        )
        .unwrap();
        assert_eq!(weak.accepted, 0);
        assert_eq!(
            weak.reports[0].reason,
            RuntimeSkillWriteReason::WeakProcedure
        );

        let raw_storage = StubSkillStorage::default();
        let raw = write_governed_runtime_skills(
            &raw_storage,
            &[RuntimeSkillWrite {
                name: String::new(),
                topic: "panic_log".to_string(),
                title: "panic log".to_string(),
                summary: "raw failure output".to_string(),
                content:
                    "[2026-04-03] level=info key=value\n[2026-04-03] payload={\"a\":1,\"b\":2}\n[2026-04-03] more={\"c\":3}"
                        .to_string(),
                citations: Vec::new(),
                source_chat_id: Some("chat-1".to_string()),
                observed_at: 100,
            }],
            RuntimeSkillWriteSource::Manual,
        )
        .unwrap();
        assert_eq!(raw.accepted, 0);
        assert_eq!(
            raw.reports[0].reason,
            RuntimeSkillWriteReason::RawPayloadOrLog
        );

        let accepted_storage = StubSkillStorage::default();
        let accepted = write_governed_runtime_skills(
            &accepted_storage,
            &[RuntimeSkillWrite {
                name: String::new(),
                topic: "release_patch_flow".to_string(),
                title: "Release patch flow".to_string(),
                summary: "Patch the release and verify the result".to_string(),
                content: "1. inspect release diff\n2. patch rollback guards\n3. verify logs"
                    .to_string(),
                citations: Vec::new(),
                source_chat_id: Some("chat-1".to_string()),
                observed_at: 100,
            }],
            RuntimeSkillWriteSource::TaskLearning,
        )
        .unwrap();
        assert_eq!(accepted.accepted, 1);
        assert_eq!(
            accepted.reports[0].reason,
            RuntimeSkillWriteReason::ProceduralMemory
        );
        assert!(accepted_storage
            .list_names()
            .unwrap()
            .iter()
            .any(|name| name == "runtime_skill__release_patch_flow"));
    }

    #[test]
    fn build_runtime_skill_block_touches_usage() {
        let storage = StubSkillStorage::default();
        upsert_runtime_skill(
            &storage,
            &RuntimeSkillWrite {
                name: String::new(),
                topic: "archive_debug".to_string(),
                title: "Archive debug".to_string(),
                summary: "Inspect retrieval trace before trusting evidence.".to_string(),
                content: "- run memory_search\n- inspect retrieval_trace".to_string(),
                citations: vec!["turn_log:chat-1#req=req-1".to_string()],
                source_chat_id: Some("chat-1".to_string()),
                observed_at: 100,
            },
        )
        .unwrap();
        let block = build_runtime_skill_recall_block(
            &storage,
            "看看 archive debug",
            Some("chat-1"),
            1000,
            400,
        )
        .unwrap();
        assert!(block.contains("Runtime skills"));
        let record = parse_runtime_skill_record(
            "runtime_skill__archive_debug",
            &get_skill_content(&storage, "runtime_skill__archive_debug").unwrap(),
        )
        .unwrap();
        assert_eq!(record.use_count, 1);
        assert_eq!(record.last_used_at, Some(1000));
    }

    #[test]
    fn governance_merges_duplicate_runtime_skills_into_canonical_record() {
        let storage = StubSkillStorage::default();
        let mut canonical = runtime_skill_record(
            "runtime_skill__wifi_setup",
            "wifi_setup",
            "Wi-Fi setup",
            "Bring Wi-Fi up before verification.",
            "- connect wifi\n- verify connectivity",
            100,
        );
        canonical.citations = vec!["transcript:chat-1#message=1".to_string()];
        canonical.quality_score = compute_runtime_skill_quality(&canonical);
        write_runtime_skill_record(&storage, &canonical).unwrap();

        let mut duplicate = runtime_skill_record(
            "runtime_skill__wifi_verification",
            "wifi setup",
            "Wi-Fi setup flow",
            "Bring Wi-Fi up before verification.",
            "- connect wifi\n- verify connectivity",
            120,
        );
        duplicate.citations = vec!["turn_log:chat-1#req=req-1".to_string()];
        duplicate.quality_score = compute_runtime_skill_quality(&duplicate);
        write_runtime_skill_record(&storage, &duplicate).unwrap();

        let outcome = govern_runtime_skills(&storage, 200).unwrap();
        assert_eq!(outcome.merged, 1);
        assert!(get_skill_content(&storage, "runtime_skill__wifi_verification").is_none());
        let merged = parse_runtime_skill_record(
            "runtime_skill__wifi_setup",
            &get_skill_content(&storage, "runtime_skill__wifi_setup").unwrap(),
        )
        .unwrap();
        assert!(merged
            .supersedes
            .iter()
            .any(|name| name == "runtime_skill__wifi_verification"));
        assert!(merged
            .component_topics
            .iter()
            .any(|topic| topic == "wifi_setup"));
        assert!(merged
            .component_topics
            .iter()
            .any(|topic| topic == "wifi setup"));
        assert_eq!(merged.citations.len(), 2);
    }

    #[test]
    fn governance_prunes_stale_low_value_runtime_skills() {
        let storage = StubSkillStorage::default();
        let mut low_value = runtime_skill_record(
            "runtime_skill__temp_probe",
            "temp_probe",
            "Temp probe",
            "",
            "probe",
            1,
        );
        low_value.quality_score = compute_runtime_skill_quality(&low_value);
        write_runtime_skill_record(&storage, &low_value).unwrap();

        let outcome =
            govern_runtime_skills(&storage, RUNTIME_SKILL_STALE_AFTER_SECS.saturating_add(10))
                .unwrap();
        assert_eq!(outcome.pruned, 1);
        assert!(get_skill_content(&storage, "runtime_skill__temp_probe").is_none());
    }

    #[test]
    fn runtime_skill_recall_can_suggest_composition() {
        let storage = StubSkillStorage::default();
        upsert_runtime_skill(
            &storage,
            &RuntimeSkillWrite {
                name: String::new(),
                topic: "network_setup".to_string(),
                title: "Network setup".to_string(),
                summary: "Network setup checklist for bring-up.".to_string(),
                content: "- connect wifi\n- collect link status".to_string(),
                citations: vec!["transcript:chat-1#message=1".to_string()],
                source_chat_id: Some("chat-1".to_string()),
                observed_at: 100,
            },
        )
        .unwrap();
        upsert_runtime_skill(
            &storage,
            &RuntimeSkillWrite {
                name: String::new(),
                topic: "network_verification".to_string(),
                title: "Network verification".to_string(),
                summary: "Verification pass for setup and connectivity.".to_string(),
                content: "- inspect retrieval trace\n- verify connectivity".to_string(),
                citations: vec!["turn_log:chat-1#req=req-1".to_string()],
                source_chat_id: Some("chat-1".to_string()),
                observed_at: 120,
            },
        )
        .unwrap();

        let block = build_runtime_skill_recall_block(
            &storage,
            "network setup verification",
            Some("chat-1"),
            200,
            480,
        )
        .unwrap();
        assert!(block.contains("[Composition]"));
    }

    #[test]
    fn runtime_skill_revision_records_genome_lineage_and_strategy_diff() {
        let storage = StubSkillStorage::default();
        upsert_runtime_skill(
            &storage,
            &RuntimeSkillWrite {
                name: String::new(),
                topic: "release_patch_flow".to_string(),
                title: "Release patch flow".to_string(),
                summary: "Patch the release and verify the result".to_string(),
                content: "1. inspect diff\n2. patch rollback guards\n3. verify logs".to_string(),
                citations: vec!["turn_log:chat-1#req=req-1".to_string()],
                source_chat_id: Some("chat-1".to_string()),
                observed_at: 100,
            },
        )
        .unwrap();
        upsert_runtime_skill(
            &storage,
            &RuntimeSkillWrite {
                name: String::new(),
                topic: "release_patch_flow".to_string(),
                title: "Release patch flow".to_string(),
                summary: "Patch the release, then verify rollback and health signals".to_string(),
                content: "1. inspect diff and rollback blast radius\n2. patch rollback guards\n3. verify health signals and logs".to_string(),
                citations: vec!["turn_log:chat-1#req=req-2".to_string()],
                source_chat_id: Some("chat-1".to_string()),
                observed_at: 160,
            },
        )
        .unwrap();

        let record = parse_runtime_skill_record(
            "runtime_skill__release_patch_flow",
            &get_skill_content(&storage, "runtime_skill__release_patch_flow").unwrap(),
        )
        .unwrap();
        assert_eq!(record.genome_lineage.len(), 2);
        assert_eq!(
            record.genome_lineage[0].disposition,
            RuntimeSkillGenomeDisposition::Superseded
        );
        assert_eq!(
            record.genome_lineage[1].disposition,
            RuntimeSkillGenomeDisposition::Active
        );
        assert_eq!(record.strategy_diffs.len(), 1);
        assert_eq!(
            record.strategy_diffs[0].change_kind,
            RuntimeSkillStrategyDiffKind::DoctrineRevision
        );
        assert!(record.strategy_diffs[0]
            .summary
            .contains("summary and procedure changed"));
    }

    #[test]
    fn governance_retires_stale_validated_runtime_skill_instead_of_pruning() {
        let storage = StubSkillStorage::default();
        let mut record = runtime_skill_record(
            "runtime_skill__wifi_recovery",
            "wifi_recovery",
            "Wi-Fi recovery",
            "Recover Wi-Fi first, then validate the route.",
            "1. reconnect wifi\n2. validate route",
            1,
        );
        record.validated_success_count = 2;
        record.updated_at = 1;
        record.quality_score = compute_runtime_skill_quality(&record);
        write_runtime_skill_record(&storage, &record).unwrap();

        let outcome =
            govern_runtime_skills(&storage, RUNTIME_SKILL_STALE_AFTER_SECS.saturating_add(20))
                .unwrap();
        assert_eq!(outcome.pruned, 0);
        let record = parse_runtime_skill_record(
            "runtime_skill__wifi_recovery",
            &get_skill_content(&storage, "runtime_skill__wifi_recovery").unwrap(),
        )
        .unwrap();
        assert_eq!(record.status, RuntimeSkillStatus::Retired);
        assert_eq!(record.retired_at, Some(RUNTIME_SKILL_STALE_AFTER_SECS + 20));
        assert!(record
            .retirement_reason
            .contains("retired after proven lineage aged out"));
    }

    #[test]
    fn doctrine_and_genome_snapshots_surface_stable_assets() {
        let storage = StubSkillStorage::default();
        upsert_runtime_skill(
            &storage,
            &RuntimeSkillWrite {
                name: String::new(),
                topic: "release_patch_flow".to_string(),
                title: "Release patch flow".to_string(),
                summary: "Patch the release only after inspection, then verify health.".to_string(),
                content: "1. inspect diff\n2. patch rollback guards\n3. verify health".to_string(),
                citations: vec!["turn_log:chat-1#req=req-1".to_string()],
                source_chat_id: Some("chat-1".to_string()),
                observed_at: 100,
            },
        )
        .unwrap();
        record_runtime_skill_outcomes(
            &storage,
            &[String::from("runtime_skill__release_patch_flow")],
            RuntimeSkillReuseOutcome::Succeeded,
            130,
            "final_answer",
        )
        .unwrap();

        let doctrine = build_runtime_skill_doctrine_snapshot(&storage);
        let genome = build_runtime_skill_genome_snapshot(&storage);

        assert_eq!(doctrine.total_clauses, 1);
        assert_eq!(doctrine.stable_clauses, 1);
        assert_eq!(genome.total_lineages, 1);
        assert_eq!(genome.retired_lineages, 0);
        assert_eq!(genome.total_diff_events, 0);
        assert!(doctrine.recent_clauses[0].clause.contains("verify health"));
    }
}
