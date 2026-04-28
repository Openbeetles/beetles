//! Shared factual plane helpers for personality and private-memory layers.
#![allow(clippy::too_many_arguments)]

use crate::util::truncate_content_to_max;
use serde::{Deserialize, Serialize};

use super::{
    long_term_memory_effective_stale_hint, long_term_memory_evidence_state,
    memory_capability_profile, parse_explicit_long_term_slot_query, recall_long_term_memory_block,
    recall_long_term_memory_entries, render_exact_long_term_memory_block,
    render_long_term_memory_block, search_archive_records, ArchiveRecordSource, ArchiveSearchHit,
    ArchiveSearchQuery, LongTermMemoryConfidence, LongTermMemoryEntry, LongTermMemoryEvidenceState,
    LongTermMemoryStore, MemoryProfile, MemoryStore, SessionMessage, TurnLedgerStore,
};

const SHARED_FACTUAL_HEADER_LEN: usize = 128;
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

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SharedFactualPlaneObservation {
    pub entry_id: String,
    pub topic: String,
    pub evidence_state: LongTermMemoryEvidenceState,
    pub reconcile_action: SharedFactualReconcileAction,
    pub support_count: usize,
    pub conflict_count: usize,
    pub top_citations: Vec<String>,
    pub evidence_summary: String,
    pub summary: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
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

    pub fn extraction_brief(&self) -> Option<String> {
        let mut out = String::from(
            "## Shared factual reconcile\nArchive evidence can suggest reinforce/correct/conflict/stale actions for canonical facts, but archive evidence is not canonical by itself.\n",
        );
        let mut appended = 0usize;
        for observation in self.observations.iter().filter(|observation| {
            observation.reconcile_action.should_request_refresh()
                || observation.support_count > 0
                || observation.conflict_count > 0
        }) {
            let line = format!(
                "- {}: action={}; supports={}; conflicts={}; evidence={}",
                observation.topic,
                observation.reconcile_action.label(),
                observation.support_count,
                observation.conflict_count,
                observation.evidence_summary
            );
            if out.len().saturating_add(line.len()).saturating_add(1) > 768 {
                break;
            }
            out.push_str(&line);
            out.push('\n');
            appended += 1;
            if appended >= 4 {
                break;
            }
        }
        (appended > 0).then(|| out.trim_end().to_string())
    }
}

fn observation_to_metadata_draft(
    entry: &LongTermMemoryEntry,
    observation: &SharedFactualPlaneObservation,
    last_confirmed_at: u64,
) -> Option<super::LongTermMemoryDraft> {
    if matches!(
        observation.reconcile_action,
        SharedFactualReconcileAction::Hold
    ) {
        return None;
    }
    let confidence = match observation.reconcile_action {
        SharedFactualReconcileAction::Reinforce => match observation.support_count {
            0 => entry.confidence,
            1 => match entry.confidence {
                LongTermMemoryConfidence::Low => LongTermMemoryConfidence::Medium,
                existing => existing,
            },
            _ => LongTermMemoryConfidence::High,
        },
        SharedFactualReconcileAction::Correct
        | SharedFactualReconcileAction::Conflict
        | SharedFactualReconcileAction::Stale => LongTermMemoryConfidence::Low,
        SharedFactualReconcileAction::Hold => entry.confidence,
    };
    let stale_hint = match observation.reconcile_action {
        SharedFactualReconcileAction::Reinforce => {
            if matches!(
                observation.evidence_state,
                LongTermMemoryEvidenceState::StableFact
            ) {
                entry.stale_hint
            } else {
                super::LongTermMemoryStaleHint::ReviewBeforeUse
            }
        }
        SharedFactualReconcileAction::Correct
        | SharedFactualReconcileAction::Conflict
        | SharedFactualReconcileAction::Stale => {
            super::LongTermMemoryStaleHint::VerifyAgainstCurrentState
        }
        SharedFactualReconcileAction::Hold => entry.stale_hint,
    };
    Some(super::LongTermMemoryDraft {
        kind: entry.kind.clone(),
        topic: entry.topic.clone(),
        content: entry.content.clone(),
        keywords: entry.keywords.clone(),
        source_chat_id: entry.source_chat_id.clone(),
        source_type: Some(entry.source_type),
        source_scope: Some(entry.source_scope),
        confidence: Some(confidence),
        freshness: Some(entry.freshness),
        stale_hint: Some(stale_hint),
        supporting_citations: observation.top_citations.clone(),
        evidence_count: Some(
            observation
                .support_count
                .max(observation.top_citations.len()) as u32,
        ),
        observed_at: (last_confirmed_at > 0).then_some(last_confirmed_at),
        last_confirmed_at: (last_confirmed_at > 0).then_some(last_confirmed_at),
        source_revision: None,
    })
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
    profile: MemoryProfile,
) -> Vec<ArchiveSearchHit> {
    let query = build_entry_archive_query(entry, query_hint, summary_text, recent);
    if query.trim().is_empty() {
        return Vec::new();
    }
    let capability = memory_capability_profile(profile);
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
            limit: capability.shared_factual_archive_hits,
        },
    )
    .unwrap_or_default()
}

fn load_shared_factual_entries(
    long_term_store: &dyn LongTermMemoryStore,
    chat_id: &str,
    query_hint: &str,
    summary_text: Option<&str>,
    recent: &[SessionMessage],
    profile: MemoryProfile,
) -> Vec<LongTermMemoryEntry> {
    let capability = memory_capability_profile(profile);
    if capability.prompt_exact_lookup_enabled {
        if let Some(slot) = parse_explicit_long_term_slot_query(query_hint) {
            if let Ok(Some(entry)) = long_term_store.get_slot(&slot) {
                return vec![entry];
            }
        }
    }
    recall_long_term_memory_entries(
        long_term_store,
        chat_id,
        query_hint,
        summary_text,
        recent,
        profile,
    )
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

fn hit_is_recent_since(hit: &ArchiveSearchHit, timestamp: u64) -> bool {
    hit.observed_at.unwrap_or(0) > timestamp
        && matches!(
            hit.source,
            ArchiveRecordSource::Transcript | ArchiveRecordSource::DailyNote
        )
}

fn collect_top_archive_citations(hits: &[ArchiveSearchHit], max_items: usize) -> Vec<String> {
    let mut citations = Vec::with_capacity(hits.len().min(max_items));
    for hit in hits {
        if citations.iter().any(|existing| existing == &hit.citation) {
            continue;
        }
        citations.push(hit.citation.clone());
        if citations.len() >= max_items {
            break;
        }
    }
    citations
}

fn archive_hit_reason(hit: &ArchiveSearchHit) -> Option<String> {
    let trace = hit.retrieval_trace.as_ref()?;
    let mut parts = Vec::with_capacity(3);
    if let Some(reason) = trace.ranking_reason.as_deref() {
        parts.push(reason.to_string());
    }
    if let Some(reason) = trace.source_reason.as_deref() {
        parts.push(reason.to_string());
    }
    if let Some(reason) = trace.recency_reason.as_deref() {
        parts.push(reason.to_string());
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("; "))
    }
}

fn build_observation_evidence_summary(
    hits: &[ArchiveSearchHit],
    support_count: usize,
    conflict_count: usize,
) -> String {
    if hits.is_empty() {
        return "no archive evidence".to_string();
    }
    let top_citations = collect_top_archive_citations(hits, 2);
    let head = if top_citations.is_empty() {
        format!(
            "{} hits, {} support, {} conflict",
            hits.len(),
            support_count,
            conflict_count
        )
    } else {
        format!(
            "{} hits, {} support, {} conflict, top={}",
            hits.len(),
            support_count,
            conflict_count,
            top_citations.join(", ")
        )
    };
    let reason = hits.iter().find_map(archive_hit_reason);
    if let Some(reason) = reason {
        format!("{head}; why={reason}")
    } else {
        head
    }
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
    let support_hits = hits
        .iter()
        .filter(|hit| hit_supports_entry(entry, hit))
        .collect::<Vec<_>>();
    let conflict_hits = hits
        .iter()
        .filter(|hit| hit_conflicts_with_entry(entry, hit))
        .collect::<Vec<_>>();
    let support_count = support_hits.len();
    let conflict_count = conflict_hits.len();
    let recent_support_count = support_hits
        .iter()
        .filter(|hit| hit_is_recent_since(hit, latest_confirmation))
        .count();
    let recent_conflict_count = conflict_hits
        .iter()
        .filter(|hit| hit_is_recent_since(hit, latest_confirmation))
        .count();
    let has_recent_hit = hits
        .iter()
        .any(|hit| hit_is_recent_since(hit, latest_confirmation));
    let base_reconcile_action = if hits.is_empty() {
        match evidence_state {
            LongTermMemoryEvidenceState::PossiblyStale
            | LongTermMemoryEvidenceState::NeedsReview => SharedFactualReconcileAction::Stale,
            LongTermMemoryEvidenceState::StableFact | LongTermMemoryEvidenceState::RecentState => {
                SharedFactualReconcileAction::Hold
            }
        }
    } else if recent_conflict_count > 0
        && (support_count == 0 || recent_conflict_count >= recent_support_count)
    {
        match entry.confidence {
            LongTermMemoryConfidence::High => SharedFactualReconcileAction::Conflict,
            LongTermMemoryConfidence::Low | LongTermMemoryConfidence::Medium => {
                SharedFactualReconcileAction::Correct
            }
        }
    } else if conflict_count > 0 && support_count == 0 {
        match entry.confidence {
            LongTermMemoryConfidence::High => SharedFactualReconcileAction::Conflict,
            LongTermMemoryConfidence::Low | LongTermMemoryConfidence::Medium => {
                SharedFactualReconcileAction::Correct
            }
        }
    } else if matches!(
        evidence_state,
        LongTermMemoryEvidenceState::PossiblyStale | LongTermMemoryEvidenceState::NeedsReview
    ) && recent_support_count > 0
    {
        SharedFactualReconcileAction::Correct
    } else if has_overlap_citation || support_count > 0 || has_recent_hit {
        SharedFactualReconcileAction::Reinforce
    } else {
        SharedFactualReconcileAction::Hold
    };
    let runtime_override = runtime_authoritative_reconcile_override(entry);
    let reconcile_action = runtime_override
        .map(|(_, action)| action)
        .unwrap_or(base_reconcile_action);

    let top_citations = collect_top_archive_citations(hits, 3);
    let evidence_summary = build_observation_evidence_summary(hits, support_count, conflict_count);

    let mut summary = format!(
        "{}:{} => {} ({})",
        entry.kind.label(),
        entry.topic,
        reconcile_action.label(),
        evidence_state.label()
    );
    summary.push_str(&format!(
        ", support={}, conflict={}",
        support_count, conflict_count
    ));
    if let Some(label) = long_term_memory_effective_stale_hint(entry, now_secs).label() {
        summary.push_str(&format!(", stale_hint={label}"));
    }
    if let Some(citation) = top_citations.first() {
        summary.push_str(&format!(", archive={}", citation));
    }
    if let Some((reason, _)) = runtime_override {
        summary.push_str(&format!(", runtime={reason}"));
    }
    summary.push_str(&format!(
        ", evidence={}",
        truncate_content_to_max(&evidence_summary, 180)
    ));
    SharedFactualPlaneObservation {
        entry_id: entry.id.clone(),
        topic: entry.topic.clone(),
        evidence_state,
        reconcile_action,
        support_count,
        conflict_count,
        top_citations,
        evidence_summary,
        summary,
    }
}

fn runtime_authoritative_reconcile_override(
    entry: &LongTermMemoryEntry,
) -> Option<(&'static str, SharedFactualReconcileAction)> {
    if entry.topic != "audio_profile_status" {
        return None;
    }
    let input_offline = crate::orchestrator::get_runtime_capability(
        crate::orchestrator::RUNTIME_CAPABILITY_AUDIO_INPUT,
    )
    .is_some_and(|state| state.status != crate::orchestrator::RuntimeCapabilityStatus::Online);
    let output_offline = crate::orchestrator::get_runtime_capability(
        crate::orchestrator::RUNTIME_CAPABILITY_AUDIO_OUTPUT,
    )
    .is_some_and(|state| state.status != crate::orchestrator::RuntimeCapabilityStatus::Online);
    if input_offline || output_offline {
        Some((
            "audio_capability_offline",
            SharedFactualReconcileAction::Stale,
        ))
    } else {
        None
    }
}

pub(crate) fn build_archive_reconcile_drafts(
    session_store: &dyn super::SessionStore,
    long_term_store: &dyn LongTermMemoryStore,
    memory_store: &dyn MemoryStore,
    turn_ledger_store: &dyn TurnLedgerStore,
    current_chat_id: &str,
    profile: MemoryProfile,
    limit: usize,
) -> Vec<super::LongTermMemoryDraft> {
    let entries = long_term_store.list(limit.clamp(1, 24)).unwrap_or_default();
    if entries.is_empty() {
        return Vec::new();
    }
    let now_secs = crate::util::current_unix_secs();
    let mut drafts = Vec::new();
    for entry in entries {
        let preferred_chat = entry
            .source_chat_id
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(current_chat_id);
        let hits = lookup_archive_hits_for_entry(
            &entry,
            memory_store,
            turn_ledger_store,
            session_store,
            preferred_chat,
            &entry.content,
            None,
            &[],
            profile,
        );
        let last_confirmed_at = hits
            .iter()
            .filter_map(|hit| hit.observed_at)
            .max()
            .unwrap_or(0);
        let observation = reconcile_entry_observation(&entry, &hits, now_secs);
        if let Some(draft) = observation_to_metadata_draft(&entry, &observation, last_confirmed_at)
        {
            drafts.push(draft);
        }
    }
    drafts
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
    let recalled_entries = load_shared_factual_entries(
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
                profile,
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
    let recalled = parse_explicit_long_term_slot_query(&query)
        .and_then(|slot| render_exact_long_term_memory_block(store, &slot, recall_budget))
        .or_else(|| {
            recall_long_term_memory_block(
                store,
                chat_id,
                &query,
                summary_text,
                recent,
                recall_budget,
                profile,
            )
        });
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
    use crate::orchestrator::{
        reset_runtime_capabilities_for_tests, update_runtime_capability, RuntimeCapabilityReason,
        RuntimeCapabilityStatus, RuntimeCapabilityUpdate, RUNTIME_CAPABILITY_AUDIO_INPUT,
        RUNTIME_CAPABILITY_AUDIO_OUTPUT,
    };

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

    #[test]
    fn audio_profile_fact_turns_stale_when_runtime_audio_is_offline() {
        let _guard = crate::orchestrator::runtime_capability::RUNTIME_CAPABILITY_TEST_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        reset_runtime_capabilities_for_tests();
        update_runtime_capability(RuntimeCapabilityUpdate {
            id: RUNTIME_CAPABILITY_AUDIO_INPUT,
            status: RuntimeCapabilityStatus::Offline,
            reason: RuntimeCapabilityReason::DeviceMissing,
            observed_at_secs: 100,
            recovery_hint: None,
        });
        update_runtime_capability(RuntimeCapabilityUpdate {
            id: RUNTIME_CAPABILITY_AUDIO_OUTPUT,
            status: RuntimeCapabilityStatus::Offline,
            reason: RuntimeCapabilityReason::DeviceMissing,
            observed_at_secs: 100,
            recovery_hint: None,
        });

        let entry = LongTermMemoryEntry {
            id: "ltm-audio".to_string(),
            kind: crate::memory::LongTermMemoryKind::Fact,
            topic: "audio_profile_status".to_string(),
            content: "audio input and output are available".to_string(),
            keywords: vec!["audio".to_string(), "duplex".to_string()],
            source_chat_id: Some("chat-1".to_string()),
            source_type: crate::memory::LongTermMemorySourceType::Conversation,
            source_scope: crate::memory::LongTermMemorySourceScope::World,
            confidence: crate::memory::LongTermMemoryConfidence::High,
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
        };

        let observation = reconcile_entry_observation(&entry, &[], 100);
        assert_eq!(
            observation.reconcile_action,
            SharedFactualReconcileAction::Stale
        );
        reset_runtime_capabilities_for_tests();
    }
}
