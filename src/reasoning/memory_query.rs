//! Read-only programmable memory query contracts built on the P1 Lua substrate.

use crate::error::{Error, Result};
use crate::memory::{
    ContinuityCapsule, ContinuityCapsuleScopeKind, LongTermMemoryEntry, LongTermMemoryQuery,
};
use crate::util::truncate_content_to_max;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

pub const MEMORY_QUERY_SNAPSHOT_SCHEMA_VERSION: u32 = 1;
pub const MEMORY_QUERY_DEFAULT_LONG_TERM_LIMIT: usize = 8;
pub const MEMORY_QUERY_DEFAULT_CONTINUITY_LIMIT: usize = 6;
pub const MEMORY_QUERY_MAX_LONG_TERM_LIMIT: usize = 16;
pub const MEMORY_QUERY_MAX_CONTINUITY_LIMIT: usize = 12;
const MEMORY_QUERY_MAX_GROUPS: usize = 8;
const MEMORY_QUERY_MAX_CANDIDATES: usize = 12;
const MEMORY_QUERY_MAX_SUMMARY_CHARS: usize = 220;
const MEMORY_QUERY_MAX_GROUP_LABEL_CHARS: usize = 64;
const MEMORY_QUERY_MAX_GROUP_SUMMARY_CHARS: usize = 160;
const MEMORY_QUERY_MAX_CANDIDATE_SUMMARY_CHARS: usize = 160;
const MEMORY_QUERY_MAX_CANDIDATE_RATIONALE_CHARS: usize = 220;
const MEMORY_QUERY_MAX_RECORD_REFS: usize = 8;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryQuerySelection {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub long_term_query: Option<LongTermMemoryQuery>,
    #[serde(default)]
    pub long_term_limit: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub continuity_scope: Option<MemoryQueryContinuityScope>,
    #[serde(default)]
    pub continuity_limit: usize,
    #[serde(default = "default_true")]
    pub include_long_term: bool,
    #[serde(default = "default_true")]
    pub include_continuity: bool,
}

impl MemoryQuerySelection {
    pub fn normalized(&self) -> Self {
        Self {
            long_term_query: self.long_term_query.as_ref().map(|query| {
                let mut normalized = query.normalized();
                normalized.limit = self
                    .long_term_limit
                    .max(1)
                    .min(MEMORY_QUERY_MAX_LONG_TERM_LIMIT);
                normalized
            }),
            long_term_limit: self
                .long_term_limit
                .max(1)
                .min(MEMORY_QUERY_MAX_LONG_TERM_LIMIT),
            continuity_scope: self.continuity_scope.as_ref().and_then(|scope| scope.normalized()),
            continuity_limit: self
                .continuity_limit
                .max(1)
                .min(MEMORY_QUERY_MAX_CONTINUITY_LIMIT),
            include_long_term: self.include_long_term,
            include_continuity: self.include_continuity,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryQueryContinuityScope {
    pub scope_kind: ContinuityCapsuleScopeKind,
    #[serde(default)]
    pub scope_id: String,
}

impl MemoryQueryContinuityScope {
    pub fn normalized(&self) -> Option<Self> {
        let scope_id = truncate_content_to_max(self.scope_id.trim(), 96).into_owned();
        if scope_id.is_empty() {
            return None;
        }
        Some(Self {
            scope_kind: self.scope_kind,
            scope_id,
        })
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryQuerySnapshotCounts {
    pub long_term_entries: usize,
    pub continuity_capsules: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryQueryLongTermRecord {
    pub record_ref: String,
    pub entry: LongTermMemoryEntry,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryQueryContinuityRecord {
    pub record_ref: String,
    pub capsule: ContinuityCapsule,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryQuerySnapshot {
    pub schema_version: u32,
    pub selection: MemoryQuerySelection,
    pub counts: MemoryQuerySnapshotCounts,
    pub snapshot_digest: String,
    #[serde(default)]
    pub long_term_entries: Vec<MemoryQueryLongTermRecord>,
    #[serde(default)]
    pub continuity_capsules: Vec<MemoryQueryContinuityRecord>,
}

impl MemoryQuerySnapshot {
    pub fn new(
        selection: MemoryQuerySelection,
        long_term_entries: Vec<LongTermMemoryEntry>,
        continuity_capsules: Vec<ContinuityCapsule>,
    ) -> Self {
        let normalized = selection.normalized();
        let long_term_records = long_term_entries
            .into_iter()
            .map(|entry| MemoryQueryLongTermRecord {
                record_ref: long_term_record_ref(&entry),
                entry,
            })
            .collect::<Vec<_>>();
        let continuity_records = continuity_capsules
            .into_iter()
            .map(|capsule| MemoryQueryContinuityRecord {
                record_ref: continuity_record_ref(&capsule),
                capsule,
            })
            .collect::<Vec<_>>();
        let counts = MemoryQuerySnapshotCounts {
            long_term_entries: long_term_records.len(),
            continuity_capsules: continuity_records.len(),
        };
        let snapshot_digest =
            compute_snapshot_digest(&normalized, &long_term_records, &continuity_records);
        Self {
            schema_version: MEMORY_QUERY_SNAPSHOT_SCHEMA_VERSION,
            selection: normalized,
            counts,
            snapshot_digest,
            long_term_entries: long_term_records,
            continuity_capsules: continuity_records,
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MemoryQueryCandidateKind {
    Merge,
    Split,
    Stale,
    Conflict,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryQueryGroup {
    pub label: String,
    pub summary: String,
    #[serde(default)]
    pub record_refs: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryQueryCandidate {
    pub kind: MemoryQueryCandidateKind,
    pub summary: String,
    pub rationale: String,
    #[serde(default)]
    pub record_refs: Vec<String>,
    #[serde(default)]
    pub requires_adjudication: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryQueryResult {
    pub summary: String,
    #[serde(default)]
    pub groups: Vec<MemoryQueryGroup>,
    #[serde(default)]
    pub candidates: Vec<MemoryQueryCandidate>,
}

pub fn default_lua_memory_query_capabilities() -> Vec<String> {
    let mut capabilities = crate::reasoning::default_lua_query_capabilities();
    capabilities.push("read_memory_snapshot".to_string());
    capabilities.push("emit_memory_candidates".to_string());
    capabilities
}

pub fn validate_memory_query_result(value: Value) -> Result<MemoryQueryResult> {
    let result: MemoryQueryResult = serde_json::from_value(value)
        .map_err(|error| Error::config("memory_query_result_decode", error.to_string()))?;
    normalize_memory_query_result(result)
}

fn normalize_memory_query_result(result: MemoryQueryResult) -> Result<MemoryQueryResult> {
    let summary = truncate_content_to_max(result.summary.trim(), MEMORY_QUERY_MAX_SUMMARY_CHARS)
        .into_owned();
    if summary.is_empty() {
        return Err(Error::config(
            "memory_query_result_validate",
            "missing summary",
        ));
    }
    if result.groups.len() > MEMORY_QUERY_MAX_GROUPS {
        return Err(Error::config(
            "memory_query_result_validate",
            format!("groups exceeds {}", MEMORY_QUERY_MAX_GROUPS),
        ));
    }
    if result.candidates.len() > MEMORY_QUERY_MAX_CANDIDATES {
        return Err(Error::config(
            "memory_query_result_validate",
            format!("candidates exceeds {}", MEMORY_QUERY_MAX_CANDIDATES),
        ));
    }

    let groups = result
        .groups
        .into_iter()
        .map(normalize_memory_query_group)
        .collect::<Result<Vec<_>>>()?;
    let candidates = result
        .candidates
        .into_iter()
        .map(normalize_memory_query_candidate)
        .collect::<Result<Vec<_>>>()?;

    Ok(MemoryQueryResult {
        summary,
        groups,
        candidates,
    })
}

fn normalize_memory_query_group(group: MemoryQueryGroup) -> Result<MemoryQueryGroup> {
    let label = truncate_content_to_max(group.label.trim(), MEMORY_QUERY_MAX_GROUP_LABEL_CHARS)
        .into_owned();
    let summary =
        truncate_content_to_max(group.summary.trim(), MEMORY_QUERY_MAX_GROUP_SUMMARY_CHARS)
            .into_owned();
    if label.is_empty() || summary.is_empty() {
        return Err(Error::config(
            "memory_query_group_validate",
            "group label and summary must be non-empty",
        ));
    }
    let record_refs = normalize_record_refs(group.record_refs)?;
    Ok(MemoryQueryGroup {
        label,
        summary,
        record_refs,
    })
}

fn normalize_memory_query_candidate(candidate: MemoryQueryCandidate) -> Result<MemoryQueryCandidate> {
    let summary =
        truncate_content_to_max(candidate.summary.trim(), MEMORY_QUERY_MAX_CANDIDATE_SUMMARY_CHARS)
            .into_owned();
    let rationale = truncate_content_to_max(
        candidate.rationale.trim(),
        MEMORY_QUERY_MAX_CANDIDATE_RATIONALE_CHARS,
    )
    .into_owned();
    if summary.is_empty() || rationale.is_empty() {
        return Err(Error::config(
            "memory_query_candidate_validate",
            "candidate summary and rationale must be non-empty",
        ));
    }
    if !candidate.requires_adjudication {
        return Err(Error::config(
            "memory_query_candidate_validate",
            "candidate must require adjudication",
        ));
    }
    let record_refs = normalize_record_refs(candidate.record_refs)?;
    if record_refs.is_empty() {
        return Err(Error::config(
            "memory_query_candidate_validate",
            "candidate must reference at least one record",
        ));
    }
    Ok(MemoryQueryCandidate {
        kind: candidate.kind,
        summary,
        rationale,
        record_refs,
        requires_adjudication: true,
    })
}

fn normalize_record_refs(record_refs: Vec<String>) -> Result<Vec<String>> {
    let mut normalized = Vec::with_capacity(record_refs.len().min(MEMORY_QUERY_MAX_RECORD_REFS));
    for item in record_refs {
        let value = truncate_content_to_max(item.trim(), 96).into_owned();
        if value.is_empty() {
            continue;
        }
        if !value.contains(':') {
            return Err(Error::config(
                "memory_query_record_ref_validate",
                format!("invalid record_ref: {}", value),
            ));
        }
        if normalized.iter().any(|existing| existing == &value) {
            continue;
        }
        normalized.push(value);
        if normalized.len() >= MEMORY_QUERY_MAX_RECORD_REFS {
            break;
        }
    }
    Ok(normalized)
}

fn long_term_record_ref(entry: &LongTermMemoryEntry) -> String {
    format!("ltm:{}", entry.id)
}

fn continuity_record_ref(capsule: &ContinuityCapsule) -> String {
    format!("capsule:{}", capsule.capsule_id)
}

fn compute_snapshot_digest(
    selection: &MemoryQuerySelection,
    long_term_entries: &[MemoryQueryLongTermRecord],
    continuity_capsules: &[MemoryQueryContinuityRecord],
) -> String {
    let mut hasher = DefaultHasher::new();
    if let Ok(encoded) = serde_json::to_vec(selection) {
        encoded.hash(&mut hasher);
    }
    for entry in long_term_entries {
        entry.record_ref.hash(&mut hasher);
        entry.entry.id.hash(&mut hasher);
        entry.entry.updated_at.hash(&mut hasher);
        entry.entry.topic.hash(&mut hasher);
    }
    for capsule in continuity_capsules {
        capsule.record_ref.hash(&mut hasher);
        capsule.capsule.capsule_id.hash(&mut hasher);
        capsule.capsule.updated_at.hash(&mut hasher);
        capsule.capsule.topic.hash(&mut hasher);
    }
    format!("{:016x}", hasher.finish())
}

fn default_true() -> bool {
    true
}

impl Hash for MemoryQueryContinuityScope {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.scope_kind.hash(state);
        self.scope_id.hash(state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::{
        ContinuityCapsule, ContinuityCapsuleKind, ContinuityCapsuleScopeKind,
        ContinuityCapsuleSource, ContinuityCapsuleStatus, LongTermMemoryConfidence,
        LongTermMemoryEntry, LongTermMemoryFreshness, LongTermMemoryKind,
        LongTermMemorySourceScope, LongTermMemorySourceType, LongTermMemoryStaleHint,
    };
    use serde_json::json;

    #[test]
    fn snapshot_assigns_record_refs_and_digest() {
        let snapshot = MemoryQuerySnapshot::new(
            MemoryQuerySelection {
                long_term_limit: 4,
                continuity_limit: 2,
                ..MemoryQuerySelection::default()
            },
            vec![sample_long_term_entry()],
            vec![sample_continuity_capsule()],
        );

        assert_eq!(snapshot.schema_version, MEMORY_QUERY_SNAPSHOT_SCHEMA_VERSION);
        assert_eq!(snapshot.counts.long_term_entries, 1);
        assert_eq!(snapshot.counts.continuity_capsules, 1);
        assert_eq!(snapshot.long_term_entries[0].record_ref, "ltm:fact:device_info");
        assert_eq!(
            snapshot.continuity_capsules[0].record_ref,
            "capsule:capsule:device_status"
        );
        assert!(!snapshot.snapshot_digest.is_empty());
    }

    #[test]
    fn result_validation_accepts_reviewable_candidates() {
        let result = validate_memory_query_result(json!({
            "summary": "Found one stale device fact and one conflicting continuity note.",
            "groups": [{
                "label": "device",
                "summary": "Device-related records cluster together.",
                "record_refs": ["ltm:fact:device_info", "capsule:capsule:device_status"]
            }],
            "candidates": [{
                "kind": "stale",
                "summary": "Review old device info fact.",
                "rationale": "The fact is marked volatile and is older than the active continuity capsule.",
                "record_refs": ["ltm:fact:device_info"],
                "requires_adjudication": true
            }]
        }))
        .expect("valid result");

        assert_eq!(result.groups.len(), 1);
        assert_eq!(result.candidates.len(), 1);
        assert_eq!(result.candidates[0].kind, MemoryQueryCandidateKind::Stale);
        assert!(result.candidates[0].requires_adjudication);
    }

    #[test]
    fn result_validation_rejects_non_adjudicated_candidate() {
        let error = validate_memory_query_result(json!({
            "summary": "bad",
            "candidates": [{
                "kind": "conflict",
                "summary": "Conflict",
                "rationale": "Two records disagree.",
                "record_refs": ["ltm:fact:device_info"],
                "requires_adjudication": false
            }]
        }))
        .expect_err("candidate should be rejected");

        assert!(error.to_string().contains("candidate must require adjudication"));
    }

    fn sample_long_term_entry() -> LongTermMemoryEntry {
        LongTermMemoryEntry {
            id: "fact:device_info".to_string(),
            kind: LongTermMemoryKind::Fact,
            topic: "device_info".to_string(),
            content: "Board is Beetle Linux".to_string(),
            keywords: vec!["device".to_string(), "linux".to_string()],
            source_chat_id: Some("c2c:test".to_string()),
            source_type: LongTermMemorySourceType::SystemRuntime,
            source_scope: LongTermMemorySourceScope::World,
            confidence: LongTermMemoryConfidence::High,
            freshness: LongTermMemoryFreshness::Dynamic,
            stale_hint: LongTermMemoryStaleHint::VerifyAgainstCurrentState,
            supporting_citations: vec!["board_info".to_string()],
            evidence_count: 2,
            created_at: 10,
            updated_at: 20,
            observed_at: 20,
            last_confirmed_at: 20,
            source_revision: 1,
            last_used_at: 0,
        }
    }

    fn sample_continuity_capsule() -> ContinuityCapsule {
        ContinuityCapsule {
            capsule_id: "capsule:device_status".to_string(),
            kind: ContinuityCapsuleKind::HandoffState,
            scope_kind: ContinuityCapsuleScopeKind::Chat,
            scope_id: "c2c:test".to_string(),
            source_chat_id: "c2c:test".to_string(),
            source_channel: "qq_channel".to_string(),
            run_id: String::new(),
            topic: "device_status".to_string(),
            summary: "User is investigating system status.".to_string(),
            outcome: String::new(),
            decisions: Vec::new(),
            next_step: "Inspect latest runtime status".to_string(),
            unresolved: vec!["Need refreshed board state".to_string()],
            artifact_refs: Vec::new(),
            provenance_refs: vec!["source=post_reply_maintenance".to_string()],
            source: ContinuityCapsuleSource::PostReplyMaintenance,
            status: ContinuityCapsuleStatus::Active,
            supersedes: Vec::new(),
            observed_at: 20,
            updated_at: 25,
        }
    }
}
