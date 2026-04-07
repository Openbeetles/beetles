//! Governance gate for canonical shared-memory writes.

use crate::error::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::{
    inspect_long_term_memory_merge_guard, route_long_term_draft, LongTermMemoryDraft,
    LongTermMemoryKind, LongTermMemoryStore, MemoryPlane, MAX_LONG_TERM_MEMORY_ITEMS,
};

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SharedMemoryWriteSource {
    #[default]
    ManualTool,
    Extraction,
    TaskLearning,
    SnapshotImport,
    HygieneReconcile,
    HygieneCompaction,
}

impl SharedMemoryWriteSource {
    pub fn label(self) -> &'static str {
        match self {
            Self::ManualTool => "manual_tool",
            Self::Extraction => "extraction",
            Self::TaskLearning => "task_learning",
            Self::SnapshotImport => "snapshot_import",
            Self::HygieneReconcile => "hygiene_reconcile",
            Self::HygieneCompaction => "hygiene_compaction",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SharedMemoryWriteAction {
    #[default]
    Accepted,
    Rejected,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SharedMemoryWriteReason {
    DurableFact,
    EmptyOrInvalid,
    RawPayloadOrLog,
    RoutedToSkill,
    StructuredMaterial,
    WeakCanonicalStatement,
    OlderThanExisting,
    LowerConfidenceThanExisting,
}

impl SharedMemoryWriteReason {
    pub fn label(self) -> &'static str {
        match self {
            Self::DurableFact => "durable_fact",
            Self::EmptyOrInvalid => "empty_or_invalid",
            Self::RawPayloadOrLog => "raw_payload_or_log",
            Self::RoutedToSkill => "routed_to_skill",
            Self::StructuredMaterial => "structured_material",
            Self::WeakCanonicalStatement => "weak_canonical_statement",
            Self::OlderThanExisting => "older_than_existing",
            Self::LowerConfidenceThanExisting => "lower_confidence_than_existing",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SharedMemoryWriteItemReport {
    pub source: SharedMemoryWriteSource,
    pub action: SharedMemoryWriteAction,
    pub reason: SharedMemoryWriteReason,
    pub topic: String,
    pub kind: LongTermMemoryKind,
    pub detail: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SharedMemoryWriteOutcome {
    pub source: SharedMemoryWriteSource,
    pub submitted: usize,
    pub accepted: usize,
    pub rejected: usize,
    pub changed: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reports: Vec<SharedMemoryWriteItemReport>,
}

pub fn write_governed_shared_memory(
    store: &dyn LongTermMemoryStore,
    drafts: &[LongTermMemoryDraft],
    now_secs: u64,
    source: SharedMemoryWriteSource,
) -> Result<SharedMemoryWriteOutcome> {
    let existing_entries = store.list(MAX_LONG_TERM_MEMORY_ITEMS)?;
    let existing_by_id = existing_entries
        .into_iter()
        .map(|entry| (entry.id.clone(), entry))
        .collect::<HashMap<_, _>>();
    let mut accepted = Vec::with_capacity(drafts.len());
    let mut reports = Vec::with_capacity(drafts.len());
    for draft in drafts {
        let report_topic = draft.topic.trim().to_string();
        let routed = route_long_term_draft(draft);
        let report_kind = draft.kind.clone();
        let Some(factual_draft) = routed.factual_draft else {
            let (reason, detail) = match routed.plane {
                MemoryPlane::Skill => (
                    SharedMemoryWriteReason::RoutedToSkill,
                    format!(
                        "shared memory rejected this write because it belongs to the skill plane ({})",
                        routed.reason
                    ),
                ),
                MemoryPlane::Reject => {
                    let reason = if routed.reason == "raw_payload_or_log" {
                        SharedMemoryWriteReason::RawPayloadOrLog
                    } else {
                        SharedMemoryWriteReason::EmptyOrInvalid
                    };
                    let detail = format!(
                        "shared memory rejected this write before persistence ({})",
                        routed.reason
                    );
                    (reason, detail)
                }
                MemoryPlane::Factual => (
                    SharedMemoryWriteReason::EmptyOrInvalid,
                    "shared memory write was missing a normalized factual draft".to_string(),
                ),
            };
            reports.push(SharedMemoryWriteItemReport {
                source,
                action: SharedMemoryWriteAction::Rejected,
                reason,
                topic: report_topic,
                kind: report_kind,
                detail,
            });
            continue;
        };
        if let Some((reason, detail)) = inspect_canonical_factual_shape(&factual_draft) {
            reports.push(SharedMemoryWriteItemReport {
                source,
                action: SharedMemoryWriteAction::Rejected,
                reason,
                topic: factual_draft.topic.clone(),
                kind: factual_draft.kind.clone(),
                detail,
            });
            continue;
        }
        if let Some(stable_id) = factual_draft.stable_id() {
            if let Some(existing) = existing_by_id.get(&stable_id) {
                match inspect_long_term_memory_merge_guard(existing, &factual_draft, now_secs) {
                    super::LongTermMemoryMergeGuardDecision::Allow => {}
                    super::LongTermMemoryMergeGuardDecision::RejectOlderObservation => {
                        reports.push(SharedMemoryWriteItemReport {
                            source,
                            action: SharedMemoryWriteAction::Rejected,
                            reason: SharedMemoryWriteReason::OlderThanExisting,
                            topic: factual_draft.topic.clone(),
                            kind: factual_draft.kind.clone(),
                            detail: format!(
                                "incoming write is older than existing canonical slot {}",
                                factual_draft.topic
                            ),
                        });
                        continue;
                    }
                    super::LongTermMemoryMergeGuardDecision::RejectLowerConfidenceContent => {
                        reports.push(SharedMemoryWriteItemReport {
                            source,
                            action: SharedMemoryWriteAction::Rejected,
                            reason: SharedMemoryWriteReason::LowerConfidenceThanExisting,
                            topic: factual_draft.topic.clone(),
                            kind: factual_draft.kind.clone(),
                            detail: format!(
                                "incoming write would overwrite {} with lower confidence than the existing canonical record",
                                factual_draft.topic
                            ),
                        });
                        continue;
                    }
                }
            }
        }
        reports.push(SharedMemoryWriteItemReport {
            source,
            action: SharedMemoryWriteAction::Accepted,
            reason: SharedMemoryWriteReason::DurableFact,
            topic: factual_draft.topic.clone(),
            kind: factual_draft.kind.clone(),
            detail: format!("accepted as canonical shared fact via {}", source.label()),
        });
        accepted.push(factual_draft);
    }
    let changed = if accepted.is_empty() {
        0
    } else {
        store.upsert_many(&accepted, now_secs)?
    };
    let accepted_count = accepted.len();
    let rejected_count = reports
        .iter()
        .filter(|report| matches!(report.action, SharedMemoryWriteAction::Rejected))
        .count();
    Ok(SharedMemoryWriteOutcome {
        source,
        submitted: drafts.len(),
        accepted: accepted_count,
        rejected: rejected_count,
        changed,
        reports,
    })
}

fn inspect_canonical_factual_shape(
    draft: &LongTermMemoryDraft,
) -> Option<(SharedMemoryWriteReason, String)> {
    let content = draft.content.trim();
    if content.is_empty() {
        return Some((
            SharedMemoryWriteReason::EmptyOrInvalid,
            "canonical shared memory requires non-empty factual content".to_string(),
        ));
    }
    let non_empty_lines = content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    let line_count = non_empty_lines.len();
    let bullet_like = non_empty_lines
        .iter()
        .filter(|line| {
            line.starts_with("- ")
                || line.starts_with("* ")
                || line.starts_with("> ")
                || line.chars().next().is_some_and(|ch| ch.is_ascii_digit()) && line.contains('.')
        })
        .count();
    let heading_like = non_empty_lines
        .iter()
        .filter(|line| line.starts_with('#'))
        .count();
    if content.contains("```")
        || line_count >= 5
        || bullet_like >= 2
        || heading_like >= 1
        || (line_count >= 3 && content.contains('|'))
    {
        return Some((
            SharedMemoryWriteReason::StructuredMaterial,
            "canonical shared memory requires one compact factual statement, not a structured excerpt, checklist, or copied formatted block".to_string(),
        ));
    }
    let content_compact = compact_text(content);
    let topic_compact = compact_text(&draft.topic);
    if content_compact.len() < 6 || content_compact == topic_compact {
        return Some((
            SharedMemoryWriteReason::WeakCanonicalStatement,
            "canonical shared memory requires a concrete factual statement, not a bare label or near-empty fragment".to_string(),
        ));
    }
    None
}

fn compact_text(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_alphanumeric() || is_cjk(*ch))
        .flat_map(char::to_lowercase)
        .collect()
}

fn is_cjk(ch: char) -> bool {
    matches!(
        ch as u32,
        0x4E00..=0x9FFF
            | 0x3400..=0x4DBF
            | 0x20000..=0x2A6DF
            | 0x2A700..=0x2B73F
            | 0x2B740..=0x2B81F
            | 0x2B820..=0x2CEAF
            | 0xF900..=0xFAFF
            | 0x2F800..=0x2FA1F
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Result;
    use crate::memory::{
        LongTermMemoryConfidence, LongTermMemoryEntry, LongTermMemoryFreshness,
        LongTermMemorySourceScope, LongTermMemorySourceType, LongTermMemoryStaleHint,
    };
    use std::sync::Mutex;

    #[derive(Default)]
    struct MemoryStoreStub {
        entries: Mutex<Vec<LongTermMemoryEntry>>,
        upserts: Mutex<Vec<Vec<LongTermMemoryDraft>>>,
    }

    impl LongTermMemoryStore for MemoryStoreStub {
        fn upsert_many(&self, drafts: &[LongTermMemoryDraft], _now_secs: u64) -> Result<usize> {
            self.upserts.lock().unwrap().push(drafts.to_vec());
            Ok(drafts.len())
        }

        fn recall(
            &self,
            _query: &str,
            _source_chat_id: Option<&str>,
            _limit: usize,
        ) -> Result<Vec<LongTermMemoryEntry>> {
            Ok(Vec::new())
        }

        fn get(&self, _id: &str) -> Result<Option<LongTermMemoryEntry>> {
            Ok(None)
        }

        fn list(&self, _limit: usize) -> Result<Vec<LongTermMemoryEntry>> {
            Ok(self.entries.lock().unwrap().clone())
        }

        fn delete(&self, _id: &str) -> Result<bool> {
            Ok(false)
        }

        fn delete_slot(&self, _slot: &super::super::LongTermMemorySlot) -> Result<bool> {
            Ok(false)
        }

        fn count(&self) -> Result<usize> {
            Ok(self.entries.lock().unwrap().len())
        }
    }

    fn draft(content: &str) -> LongTermMemoryDraft {
        LongTermMemoryDraft {
            kind: LongTermMemoryKind::Profile,
            topic: "owner_timezone".to_string(),
            content: content.to_string(),
            keywords: vec!["timezone".to_string()],
            source_chat_id: Some("chat-1".to_string()),
            source_type: Some(LongTermMemorySourceType::ManualTool),
            source_scope: Some(LongTermMemorySourceScope::User),
            confidence: Some(LongTermMemoryConfidence::High),
            freshness: Some(LongTermMemoryFreshness::Stable),
            stale_hint: Some(LongTermMemoryStaleHint::None),
            supporting_citations: Vec::new(),
            evidence_count: None,
            observed_at: Some(20),
            last_confirmed_at: Some(20),
            source_revision: Some(2),
        }
    }

    #[test]
    fn rejects_structured_material_for_canonical_shared_memory() {
        let store = MemoryStoreStub::default();
        let outcome = write_governed_shared_memory(
            &store,
            &[draft("- step one\n- step two\n- step three")],
            30,
            SharedMemoryWriteSource::ManualTool,
        )
        .unwrap();
        assert_eq!(outcome.accepted, 0);
        assert_eq!(outcome.rejected, 1);
        assert_eq!(
            outcome.reports[0].reason,
            SharedMemoryWriteReason::StructuredMaterial
        );
    }

    #[test]
    fn rejects_lower_confidence_overwrite_before_persisting() {
        let store = MemoryStoreStub {
            entries: Mutex::new(vec![LongTermMemoryEntry {
                id: draft("Owner timezone is Asia/Shanghai.")
                    .stable_id()
                    .unwrap(),
                kind: LongTermMemoryKind::Profile,
                topic: "owner_timezone".to_string(),
                content: "Owner timezone is Asia/Shanghai.".to_string(),
                keywords: vec!["timezone".to_string()],
                source_chat_id: Some("chat-1".to_string()),
                source_type: LongTermMemorySourceType::Conversation,
                source_scope: LongTermMemorySourceScope::User,
                confidence: LongTermMemoryConfidence::High,
                freshness: LongTermMemoryFreshness::Stable,
                stale_hint: LongTermMemoryStaleHint::None,
                supporting_citations: Vec::new(),
                evidence_count: 1,
                created_at: 10,
                updated_at: 20,
                observed_at: 20,
                last_confirmed_at: 20,
                source_revision: 5,
                last_used_at: 0,
            }]),
            upserts: Mutex::new(Vec::new()),
        };
        let mut incoming = draft("Owner timezone is UTC+8.");
        incoming.confidence = Some(LongTermMemoryConfidence::Medium);
        incoming.observed_at = Some(30);
        incoming.last_confirmed_at = Some(30);
        incoming.source_revision = Some(6);
        let outcome = write_governed_shared_memory(
            &store,
            &[incoming],
            30,
            SharedMemoryWriteSource::SnapshotImport,
        )
        .unwrap();
        assert_eq!(outcome.accepted, 0);
        assert_eq!(outcome.rejected, 1);
        assert_eq!(
            outcome.reports[0].reason,
            SharedMemoryWriteReason::LowerConfidenceThanExisting
        );
        assert!(store.upserts.lock().unwrap().is_empty());
    }

    #[test]
    fn shared_memory_governance_regression_suite_covers_accept_reject_matrix() {
        let accepted_store = MemoryStoreStub::default();
        let accepted = write_governed_shared_memory(
            &accepted_store,
            &[draft("Owner timezone is Asia/Shanghai.")],
            30,
            SharedMemoryWriteSource::ManualTool,
        )
        .unwrap();
        assert_eq!(accepted.accepted, 1);
        assert_eq!(accepted.rejected, 0);
        assert_eq!(
            accepted.reports[0].reason,
            SharedMemoryWriteReason::DurableFact
        );
        assert_eq!(accepted_store.upserts.lock().unwrap().len(), 1);

        let skill_store = MemoryStoreStub::default();
        let mut procedure =
            draft("1. inspect release diff\n2. patch rollback guard\n3. verify logs");
        procedure.kind = LongTermMemoryKind::Task;
        procedure.topic = "apply_release_patch".to_string();
        let routed_to_skill = write_governed_shared_memory(
            &skill_store,
            &[procedure],
            30,
            SharedMemoryWriteSource::TaskLearning,
        )
        .unwrap();
        assert_eq!(routed_to_skill.accepted, 0);
        assert_eq!(
            routed_to_skill.reports[0].reason,
            SharedMemoryWriteReason::RoutedToSkill
        );

        let raw_store = MemoryStoreStub::default();
        let raw_payload = write_governed_shared_memory(
            &raw_store,
            &[draft(
                "[2026-04-03] level=info key=value\n[2026-04-03] payload={\"a\":1,\"b\":2}\n[2026-04-03] more={\"c\":3}",
            )],
            30,
            SharedMemoryWriteSource::Extraction,
        )
        .unwrap();
        assert_eq!(raw_payload.accepted, 0);
        assert_eq!(
            raw_payload.reports[0].reason,
            SharedMemoryWriteReason::RawPayloadOrLog
        );

        let structured_store = MemoryStoreStub::default();
        let structured = write_governed_shared_memory(
            &structured_store,
            &[draft("- step one\n- step two\n- step three")],
            30,
            SharedMemoryWriteSource::ManualTool,
        )
        .unwrap();
        assert_eq!(structured.accepted, 0);
        assert_eq!(
            structured.reports[0].reason,
            SharedMemoryWriteReason::StructuredMaterial
        );

        let existing_store = MemoryStoreStub {
            entries: Mutex::new(vec![LongTermMemoryEntry {
                id: draft("Owner timezone is Asia/Shanghai.")
                    .stable_id()
                    .unwrap(),
                kind: LongTermMemoryKind::Profile,
                topic: "owner_timezone".to_string(),
                content: "Owner timezone is Asia/Shanghai.".to_string(),
                keywords: vec!["timezone".to_string()],
                source_chat_id: Some("chat-1".to_string()),
                source_type: LongTermMemorySourceType::Conversation,
                source_scope: LongTermMemorySourceScope::User,
                confidence: LongTermMemoryConfidence::High,
                freshness: LongTermMemoryFreshness::Stable,
                stale_hint: LongTermMemoryStaleHint::None,
                supporting_citations: Vec::new(),
                evidence_count: 1,
                created_at: 10,
                updated_at: 20,
                observed_at: 20,
                last_confirmed_at: 20,
                source_revision: 5,
                last_used_at: 0,
            }]),
            upserts: Mutex::new(Vec::new()),
        };
        let mut incoming = draft("Owner timezone is UTC+8.");
        incoming.confidence = Some(LongTermMemoryConfidence::Medium);
        incoming.observed_at = Some(30);
        incoming.last_confirmed_at = Some(30);
        incoming.source_revision = Some(6);
        let lower_confidence = write_governed_shared_memory(
            &existing_store,
            &[incoming],
            30,
            SharedMemoryWriteSource::SnapshotImport,
        )
        .unwrap();
        assert_eq!(lower_confidence.accepted, 0);
        assert_eq!(
            lower_confidence.reports[0].reason,
            SharedMemoryWriteReason::LowerConfidenceThanExisting
        );
        assert!(existing_store.upserts.lock().unwrap().is_empty());
    }
}
