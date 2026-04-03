//! Shared factual plane helpers for personality and private-memory layers.

use crate::util::truncate_content_to_max;
use serde::{Deserialize, Serialize};

use super::{
    long_term_memory_effective_stale_hint, long_term_memory_evidence_state,
    recall_long_term_memory_block, recall_long_term_memory_entries, render_long_term_memory_block,
    search_archive_records, ArchiveRecordSource, ArchiveSearchHit, ArchiveSearchQuery,
    LongTermMemoryConfidence, LongTermMemoryEntry, LongTermMemoryEvidenceState,
    LongTermMemoryStore, MemoryProfile, MemoryStore, SessionMessage, TurnLedgerStore,
};

const SHARED_FACTUAL_HEADER_LEN: usize = 128;
const SHARED_FACTUAL_RECONCILE_LIMIT: usize = 3;
const SHARED_FACTUAL_OBSERVATION_TERM_LIMIT: usize = 12;

fn build_shared_factual_query(query_hint: Option<&str>, recent: &[SessionMessage]) -> String {
    let hinted = query_hint
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| truncate_content_to_max(value, 160).into_owned());
    if hinted.is_some() {
        return hinted.unwrap_or_default();
    }
    recent
        .iter()
        .rev()
        .find_map(|message| {
            let content = message.content.trim();
            (!content.is_empty()).then(|| truncate_content_to_max(content, 160).into_owned())
        })
        .unwrap_or_default()
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SharedFactualReconcileAction {
    #[default]
    Hold,
    Reinforce,
    Correct,
    Conflict,
    Stale,
}

impl SharedFactualReconcileAction {
    pub fn label(self) -> &'static str {
        match self {
            Self::Hold => "hold",
            Self::Reinforce => "reinforce",
            Self::Correct => "correct",
            Self::Conflict => "conflict",
            Self::Stale => "stale",
        }
    }

    pub fn should_request_refresh(self) -> bool {
        matches!(
            self,
            SharedFactualReconcileAction::Reinforce
                | SharedFactualReconcileAction::Correct
                | SharedFactualReconcileAction::Conflict
                | SharedFactualReconcileAction::Stale
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SharedFactualPlaneObservation {
    pub entry_id: String,
    pub topic: String,
    pub evidence_state: LongTermMemoryEvidenceState,
    pub reconcile_action: SharedFactualReconcileAction,
    pub summary: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SharedFactualPlaneSnapshot {
    pub block: Option<String>,
    pub observations: Vec<SharedFactualPlaneObservation>,
}

impl SharedFactualPlaneSnapshot {
    pub fn strongest_refresh_action(&self) -> Option<SharedFactualReconcileAction> {
        self.observations
            .iter()
            .map(|observation| observation.reconcile_action)
            .max_by_key(|action| factual_action_priority(*action))
            .filter(|action| action.should_request_refresh())
    }

    pub fn refresh_summary(&self) -> Option<String> {
        let mut lines = self
            .observations
            .iter()
            .filter(|observation| observation.reconcile_action.should_request_refresh())
            .map(|observation| observation.summary.clone())
            .collect::<Vec<_>>();
        lines.truncate(3);
        (!lines.is_empty()).then(|| lines.join(" | "))
    }
}

fn factual_action_priority(action: SharedFactualReconcileAction) -> u8 {
    match action {
        SharedFactualReconcileAction::Hold => 0,
        SharedFactualReconcileAction::Reinforce => 1,
        SharedFactualReconcileAction::Stale => 2,
        SharedFactualReconcileAction::Correct => 3,
        SharedFactualReconcileAction::Conflict => 4,
    }
}

fn normalize_match_text(value: &str) -> String {
    value
        .chars()
        .flat_map(|ch| {
            if ch.is_alphanumeric() || is_cjk(ch) {
                ch.to_lowercase()
                    .collect::<String>()
                    .chars()
                    .collect::<Vec<_>>()
            } else {
                vec![' ']
            }
        })
        .collect::<String>()
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

fn match_terms(value: &str) -> Vec<String> {
    let mut terms = Vec::new();
    for term in normalize_match_text(value)
        .split_whitespace()
        .filter(|term| term.len() >= 2)
    {
        if terms.iter().any(|existing| existing == term) {
            continue;
        }
        terms.push(term.to_string());
        if terms.len() >= SHARED_FACTUAL_OBSERVATION_TERM_LIMIT {
            break;
        }
    }
    terms
}

fn overlap_ratio(left: &[String], right: &[String]) -> f32 {
    if left.is_empty() || right.is_empty() {
        return 0.0;
    }
    let overlap = left
        .iter()
        .filter(|term| right.iter().any(|candidate| candidate == *term))
        .count();
    overlap as f32 / left.len().max(right.len()) as f32
}

fn build_entry_archive_query(
    entry: &LongTermMemoryEntry,
    query_hint: &str,
    summary_text: Option<&str>,
    recent: &[SessionMessage],
) -> String {
    let mut parts = Vec::with_capacity(4);
    parts.push(format!("{} {}", entry.topic.trim(), entry.content.trim()));
    if !query_hint.trim().is_empty() {
        parts.push(truncate_content_to_max(query_hint.trim(), 160).into_owned());
    }
    if let Some(summary) = summary_text
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        parts.push(truncate_content_to_max(summary, 160).into_owned());
    }
    if let Some(latest) = recent.iter().rev().find_map(|message| {
        let content = message.content.trim();
        (!content.is_empty()).then(|| truncate_content_to_max(content, 160).into_owned())
    }) {
        parts.push(latest);
    }
    parts.join("\n")
}

fn lookup_archive_hits_for_entry(
    entry: &LongTermMemoryEntry,
    memory_store: &dyn MemoryStore,
    turn_ledger_store: &dyn TurnLedgerStore,
    session_store: &dyn super::SessionStore,
    chat_id: &str,
    query_hint: &str,
    summary_text: Option<&str>,
    recent: &[SessionMessage],
) -> Vec<ArchiveSearchHit> {
    let query = build_entry_archive_query(entry, query_hint, summary_text, recent);
    if query.trim().is_empty() {
        return Vec::new();
    }
    search_archive_records(
        session_store,
        memory_store,
        turn_ledger_store,
        ArchiveSearchQuery {
            query: &query,
            preferred_chat_id: Some(chat_id),
            chat_id_filter: None,
            sources: &[
                ArchiveRecordSource::Transcript,
                ArchiveRecordSource::DailyNote,
                ArchiveRecordSource::TurnLog,
            ],
            limit: SHARED_FACTUAL_RECONCILE_LIMIT,
        },
    )
    .unwrap_or_default()
}

fn hit_supports_entry(entry: &LongTermMemoryEntry, hit: &ArchiveSearchHit) -> bool {
    let entry_terms = match_terms(&format!(
        "{} {} {}",
        entry.topic,
        entry.content,
        entry.keywords.join(" ")
    ));
    let hit_terms = match_terms(&format!(
        "{} {} {}",
        hit.title,
        hit.excerpt,
        hit.cues.join(" ")
    ));
    overlap_ratio(&entry_terms, &hit_terms) >= 0.24
}

fn hit_conflicts_with_entry(entry: &LongTermMemoryEntry, hit: &ArchiveSearchHit) -> bool {
    let topic_terms = match_terms(&format!("{} {}", entry.topic, entry.keywords.join(" ")));
    let entry_terms = match_terms(&entry.content);
    let hit_terms = match_terms(&hit.excerpt);
    let topic_overlap = overlap_ratio(&topic_terms, &hit_terms);
    let content_overlap = overlap_ratio(&entry_terms, &hit_terms);
    topic_overlap >= 0.20 && content_overlap <= 0.08
}

fn reconcile_entry_observation(
    entry: &LongTermMemoryEntry,
    hits: &[ArchiveSearchHit],
    now_secs: u64,
) -> SharedFactualPlaneObservation {
    let evidence_state = long_term_memory_evidence_state(entry, now_secs);
    let latest_confirmation = entry
        .last_confirmed_at
        .max(entry.observed_at)
        .max(entry.updated_at)
        .max(entry.created_at);
    let has_overlap_citation = hits.iter().any(|hit| {
        entry
            .supporting_citations
            .iter()
            .any(|citation| citation == &hit.citation)
    });
    let has_recent_hit = hits.iter().any(|hit| {
        hit.observed_at.unwrap_or(0) > latest_confirmation
            && matches!(
                hit.source,
                ArchiveRecordSource::Transcript | ArchiveRecordSource::DailyNote
            )
    });
    let has_supporting_hit = hits.iter().any(|hit| hit_supports_entry(entry, hit));
    let has_conflicting_hit = hits.iter().any(|hit| hit_conflicts_with_entry(entry, hit));
    let reconcile_action = if hits.is_empty() {
        match evidence_state {
            LongTermMemoryEvidenceState::PossiblyStale
            | LongTermMemoryEvidenceState::NeedsReview => SharedFactualReconcileAction::Stale,
            LongTermMemoryEvidenceState::StableFact | LongTermMemoryEvidenceState::RecentState => {
                SharedFactualReconcileAction::Hold
            }
        }
    } else if has_conflicting_hit && has_recent_hit {
        match entry.confidence {
            LongTermMemoryConfidence::High => SharedFactualReconcileAction::Conflict,
            LongTermMemoryConfidence::Low | LongTermMemoryConfidence::Medium => {
                SharedFactualReconcileAction::Correct
            }
        }
    } else if matches!(
        evidence_state,
        LongTermMemoryEvidenceState::PossiblyStale | LongTermMemoryEvidenceState::NeedsReview
    ) && has_recent_hit
    {
        SharedFactualReconcileAction::Correct
    } else if has_overlap_citation || has_supporting_hit || has_recent_hit {
        SharedFactualReconcileAction::Reinforce
    } else {
        SharedFactualReconcileAction::Hold
    };

    let mut summary = format!(
        "{}:{} => {} ({})",
        entry.kind.label(),
        entry.topic,
        reconcile_action.label(),
        evidence_state.label()
    );
    if let Some(label) = long_term_memory_effective_stale_hint(entry, now_secs).label() {
        summary.push_str(&format!(", stale_hint={label}"));
    }
    if let Some(hit) = hits.first() {
        summary.push_str(&format!(", archive={}", hit.citation));
    }
    SharedFactualPlaneObservation {
        entry_id: entry.id.clone(),
        topic: entry.topic.clone(),
        evidence_state,
        reconcile_action,
        summary,
    }
}

pub(crate) fn build_shared_factual_plane_snapshot(
    session_store: &dyn super::SessionStore,
    long_term_store: &dyn LongTermMemoryStore,
    memory_store: &dyn MemoryStore,
    turn_ledger_store: &dyn TurnLedgerStore,
    chat_id: &str,
    query_hint: &str,
    summary_text: Option<&str>,
    recent: &[SessionMessage],
    max_len: usize,
    profile: MemoryProfile,
) -> SharedFactualPlaneSnapshot {
    if max_len < 96 {
        return SharedFactualPlaneSnapshot::default();
    }
    let recalled_entries = recall_long_term_memory_entries(
        long_term_store,
        chat_id,
        query_hint,
        summary_text,
        recent,
        profile,
    );
    if recalled_entries.is_empty() {
        return SharedFactualPlaneSnapshot {
            block: Some(
                "## Shared Factual Plane\nCanonical shared record for evidence-backed durable user/world facts. Private layers may rely on it, but they do not own it.\nNo recalled canonical facts for this turn.".to_string(),
            ),
            observations: Vec::new(),
        };
    }

    let now_secs = crate::util::current_unix_secs();
    let query = build_shared_factual_query(Some(query_hint), recent);
    let observations = recalled_entries
        .iter()
        .map(|entry| {
            let hits = lookup_archive_hits_for_entry(
                entry,
                memory_store,
                turn_ledger_store,
                session_store,
                chat_id,
                &query,
                summary_text,
                recent,
            );
            reconcile_entry_observation(entry, &hits, now_secs)
        })
        .collect::<Vec<_>>();

    let mut out = String::with_capacity(max_len.min(1024));
    out.push_str("## Shared Factual Plane\n");
    out.push_str(
        "Canonical shared record for evidence-backed durable user/world facts. Private layers may rely on it, but they do not own it.\n",
    );
    if let Some(rendered) = render_long_term_memory_block(
        &recalled_entries,
        max_len.saturating_sub(SHARED_FACTUAL_HEADER_LEN),
    ) {
        out.push_str(rendered.trim());
    } else {
        out.push_str("No recalled canonical facts for this turn.");
    }
    let observation_budget = max_len.saturating_sub(out.len()).saturating_sub(32);
    if observation_budget >= 120 && !observations.is_empty() {
        out.push_str("\n\n### Evidence posture\n");
        for observation in &observations {
            let line = format!("- {}", observation.summary);
            if out.len().saturating_add(line.len()).saturating_add(1) > max_len {
                break;
            }
            out.push_str(&line);
            out.push('\n');
        }
    }

    SharedFactualPlaneSnapshot {
        block: Some(truncate_content_to_max(out.trim_end(), max_len).into_owned()),
        observations,
    }
}

pub(crate) fn render_shared_factual_plane_block(
    store: &dyn LongTermMemoryStore,
    chat_id: &str,
    summary_text: Option<&str>,
    recent: &[SessionMessage],
    max_len: usize,
    profile: MemoryProfile,
) -> Option<String> {
    if max_len < 96 {
        return None;
    }
    let query = build_shared_factual_query(None, recent);
    let recall_budget = max_len.saturating_sub(SHARED_FACTUAL_HEADER_LEN).max(96);
    let recalled = recall_long_term_memory_block(
        store,
        chat_id,
        &query,
        summary_text,
        recent,
        recall_budget,
        profile,
    );
    let mut out = String::with_capacity(max_len.min(768));
    out.push_str("## Shared Factual Plane\n");
    out.push_str(
        "Canonical shared record for evidence-backed durable user/world facts. Private layers may rely on it, but they do not own it.\n",
    );
    if let Some(recalled) = recalled {
        out.push_str(recalled.trim());
    } else {
        out.push_str("No recalled canonical facts for this turn.");
    }
    let rendered = truncate_content_to_max(out.trim_end(), max_len).into_owned();
    (!rendered.trim().is_empty()).then_some(rendered)
}

pub(crate) fn render_private_memory_boundary_block(
    layer_name: &str,
    layer_role: &str,
    max_len: usize,
) -> Option<String> {
    if max_len < 96 {
        return None;
    }
    let mut out = String::with_capacity(max_len.min(384));
    out.push_str("## Shared/Private Boundary\n");
    out.push_str(
        "- Shared factual plane is canonical for durable, evidence-backed objective facts.\n",
    );
    out.push_str(&format!(
        "- {} may use shared facts as grounding, but must not rewrite, restate, or compete with them.\n",
        layer_name
    ));
    out.push_str(&format!("- Use {} only for {}.\n", layer_name, layer_role));
    out.push_str("- If something is objective and durable, leave it in shared facts; keep only subjective meaning, continuity, or governance here.\n");
    let rendered = truncate_content_to_max(out.trim_end(), max_len).into_owned();
    (!rendered.trim().is_empty()).then_some(rendered)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Result;
    use crate::memory::{LongTermMemoryEntry, LongTermMemoryStore};

    #[derive(Default)]
    struct StubLongTermMemoryStore {
        entries: Vec<LongTermMemoryEntry>,
    }

    impl LongTermMemoryStore for StubLongTermMemoryStore {
        fn upsert_many(
            &self,
            _drafts: &[crate::memory::LongTermMemoryDraft],
            _now_secs: u64,
        ) -> Result<usize> {
            Ok(0)
        }

        fn recall(
            &self,
            _query: &str,
            _source_chat_id: Option<&str>,
            limit: usize,
        ) -> Result<Vec<LongTermMemoryEntry>> {
            Ok(self.entries.iter().take(limit).cloned().collect())
        }

        fn get(&self, _id: &str) -> Result<Option<LongTermMemoryEntry>> {
            Ok(None)
        }

        fn list(&self, limit: usize) -> Result<Vec<LongTermMemoryEntry>> {
            Ok(self.entries.iter().take(limit).cloned().collect())
        }

        fn delete(&self, _id: &str) -> Result<bool> {
            Ok(false)
        }

        fn delete_slot(&self, _slot: &crate::memory::LongTermMemorySlot) -> Result<bool> {
            Ok(false)
        }

        fn count(&self) -> Result<usize> {
            Ok(self.entries.len())
        }
    }

    #[test]
    fn shared_factual_plane_block_wraps_recalled_memory() {
        let store = StubLongTermMemoryStore {
            entries: vec![LongTermMemoryEntry {
                id: "ltm-1".to_string(),
                kind: crate::memory::LongTermMemoryKind::Fact,
                topic: "primary_llm".to_string(),
                content: "当前主模型是 OpenAI。".to_string(),
                keywords: vec!["openai".to_string()],
                source_chat_id: Some("chat-1".to_string()),
                source_type: crate::memory::LongTermMemorySourceType::Conversation,
                source_scope: crate::memory::LongTermMemorySourceScope::World,
                confidence: crate::memory::LongTermMemoryConfidence::Medium,
                freshness: crate::memory::LongTermMemoryFreshness::Dynamic,
                stale_hint: crate::memory::LongTermMemoryStaleHint::ReviewBeforeUse,
                supporting_citations: vec!["transcript:chat-1#message=1".to_string()],
                evidence_count: 1,
                created_at: 1,
                updated_at: 1,
                observed_at: 1,
                last_confirmed_at: 1,
                source_revision: 0,
                last_used_at: 0,
            }],
        };

        let block = render_shared_factual_plane_block(
            &store,
            "chat-1",
            Some("summary"),
            &[SessionMessage {
                role: "user".to_string(),
                content: "主模型现在是什么".to_string(),
            }],
            512,
            MemoryProfile::Embedded,
        )
        .unwrap();

        assert!(block.contains("## Shared Factual Plane"));
        assert!(block.contains("Long-term memory"));
        assert!(block.contains("primary_llm"));
    }

    #[test]
    fn private_memory_boundary_block_mentions_shared_facts() {
        let block = render_private_memory_boundary_block(
            "self_model",
            "durable private continuity and stance",
            256,
        )
        .unwrap();

        assert!(block.contains("Shared factual plane is canonical"));
        assert!(block.contains("self_model"));
    }
}
