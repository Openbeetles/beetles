//! Heuristic selector for archive evidence injection.

use std::collections::HashMap;

use super::{
    archive_search::normalize_archive_match_text, memory_capability_profile, ArchiveRecordSource,
    ArchiveSearchHit, MemoryProfile,
};

#[derive(Clone, Copy)]
struct ArchiveSelectorPolicy {
    max_items: usize,
    max_chars: usize,
    transcript_quota: usize,
    daily_note_quota: usize,
    turn_log_quota: usize,
}

struct PreparedArchivePromptHit {
    hit: ArchiveSearchHit,
    similarity_key: String,
    prompt_line_len: usize,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct ArchivePromptSelectionSourceStats {
    pub source: ArchiveRecordSource,
    #[serde(default, skip_serializing_if = "is_zero_usize")]
    pub selected_count: usize,
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct ArchivePromptSelectionReport {
    #[serde(default, skip_serializing_if = "is_zero_usize")]
    pub input_hits: usize,
    #[serde(default, skip_serializing_if = "is_zero_usize")]
    pub selected_hits: usize,
    #[serde(default, skip_serializing_if = "is_zero_usize")]
    pub max_items: usize,
    #[serde(default, skip_serializing_if = "is_zero_usize")]
    pub max_chars: usize,
    #[serde(default, skip_serializing_if = "is_zero_usize")]
    pub skipped_by_chars: usize,
    #[serde(default, skip_serializing_if = "is_zero_usize")]
    pub skipped_by_similarity: usize,
    #[serde(default, skip_serializing_if = "is_zero_usize")]
    pub deferred_by_quota: usize,
    #[serde(default, skip_serializing_if = "is_zero_usize")]
    pub relaxed_quota_selected: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_stats: Vec<ArchivePromptSelectionSourceStats>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selection_note: Option<String>,
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct ArchivePromptSelectionResult {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hits: Vec<ArchiveSearchHit>,
    pub report: ArchivePromptSelectionReport,
}

fn is_zero_usize(value: &usize) -> bool {
    *value == 0
}

fn selector_policy(profile: MemoryProfile, max_chars: usize) -> ArchiveSelectorPolicy {
    let capability = memory_capability_profile(profile);
    match profile {
        MemoryProfile::Standard => ArchiveSelectorPolicy {
            max_items: capability.archive_prompt_max_items.min(4),
            max_chars: max_chars.min(capability.archive_prompt_max_chars),
            transcript_quota: 2,
            daily_note_quota: 1,
            turn_log_quota: 1,
        },
        MemoryProfile::Embedded => ArchiveSelectorPolicy {
            max_items: capability.archive_prompt_max_items.min(3),
            max_chars: max_chars.min(capability.archive_prompt_max_chars),
            transcript_quota: 1,
            daily_note_quota: 1,
            turn_log_quota: 1,
        },
    }
}

fn source_quota(policy: ArchiveSelectorPolicy, source: ArchiveRecordSource) -> usize {
    match source {
        ArchiveRecordSource::Transcript => policy.transcript_quota,
        ArchiveRecordSource::DailyNote => policy.daily_note_quota,
        ArchiveRecordSource::TurnLog => policy.turn_log_quota,
    }
}

fn normalized_similarity_key(hit: &ArchiveSearchHit) -> String {
    normalize_archive_match_text(&format!("{} {}", hit.title, hit.excerpt))
}

fn is_too_similar(existing_keys: &[String], candidate_key: &str) -> bool {
    if candidate_key.is_empty() {
        return false;
    }
    existing_keys.iter().any(|existing| {
        existing == candidate_key
            || existing.contains(candidate_key)
            || candidate_key.contains(existing)
            || shared_archive_terms(existing, candidate_key) >= 5
    })
}

fn shared_archive_terms(a: &str, b: &str) -> usize {
    let mut count = 0usize;
    for term in a.split_whitespace() {
        if term.len() < 3 {
            continue;
        }
        if b.split_whitespace().any(|candidate| candidate == term) {
            count = count.saturating_add(1);
        }
    }
    count
}

fn archive_prompt_line_len(hit: &ArchiveSearchHit) -> usize {
    16usize
        .saturating_add(hit.title.len())
        .saturating_add(hit.excerpt.len())
        .saturating_add(hit.citation.len())
        .saturating_add(hit.cues.iter().map(|cue| cue.len()).sum::<usize>())
}

fn annotate_selector_reason(mut hit: ArchiveSearchHit, reason: String) -> ArchiveSearchHit {
    if let Some(trace) = hit.retrieval_trace.as_mut() {
        trace.selector_reason = Some(reason);
    }
    hit
}

fn primary_selector_reason(hit: &ArchiveSearchHit, used: usize, quota: usize) -> String {
    format!(
        "selected in primary quota pass as top {} evidence ({}/{})",
        hit.source.label(),
        used.saturating_add(1),
        quota
    )
}

fn relaxed_selector_reason(hit: &ArchiveSearchHit) -> String {
    format!(
        "selected in quota-relax pass to fill remaining archive budget with {} evidence",
        hit.source.label()
    )
}

pub(crate) fn select_archive_hits_for_prompt_with_report(
    hits: Vec<ArchiveSearchHit>,
    profile: MemoryProfile,
    max_chars: usize,
) -> ArchivePromptSelectionResult {
    let policy = selector_policy(profile, max_chars);
    if hits.is_empty() || policy.max_items == 0 || policy.max_chars == 0 {
        return ArchivePromptSelectionResult {
            hits: Vec::new(),
            report: ArchivePromptSelectionReport {
                input_hits: hits.len(),
                selected_hits: 0,
                max_items: policy.max_items,
                max_chars: policy.max_chars,
                selection_note: Some(if hits.is_empty() {
                    "no_archive_hits_available".to_string()
                } else {
                    "archive_prompt_budget_zero".to_string()
                }),
                ..ArchivePromptSelectionReport::default()
            },
        };
    }

    let mut prepared_hits = prepare_archive_prompt_hits(hits);

    let mut selected = Vec::with_capacity(policy.max_items);
    let mut deferred = Vec::new();
    let mut used_chars = 0usize;
    let mut skipped_by_chars = 0usize;
    let mut skipped_by_similarity = 0usize;
    let mut deferred_by_quota = 0usize;
    let mut relaxed_quota_selected = 0usize;
    let mut per_source = HashMap::<ArchiveRecordSource, usize>::new();
    let mut similarity_keys = Vec::with_capacity(policy.max_items);
    let input_hits = prepared_hits.len();

    for prepared in prepared_hits.drain(..) {
        if selected.len() >= policy.max_items {
            break;
        }
        if used_chars.saturating_add(prepared.prompt_line_len) > policy.max_chars {
            skipped_by_chars = skipped_by_chars.saturating_add(1);
            continue;
        }
        let used = per_source.get(&prepared.hit.source).copied().unwrap_or(0);
        if used >= source_quota(policy, prepared.hit.source) {
            deferred_by_quota = deferred_by_quota.saturating_add(1);
            deferred.push(prepared);
            continue;
        }
        if is_too_similar(&similarity_keys, &prepared.similarity_key) {
            skipped_by_similarity = skipped_by_similarity.saturating_add(1);
            continue;
        }
        used_chars = used_chars.saturating_add(prepared.prompt_line_len);
        *per_source.entry(prepared.hit.source).or_insert(0) += 1;
        similarity_keys.push(prepared.similarity_key);
        let reason = primary_selector_reason(
            &prepared.hit,
            used,
            source_quota(policy, prepared.hit.source),
        );
        selected.push(annotate_selector_reason(prepared.hit, reason));
    }

    if selected.len() < policy.max_items {
        for prepared in deferred {
            if selected.len() >= policy.max_items {
                break;
            }
            if used_chars.saturating_add(prepared.prompt_line_len) > policy.max_chars {
                skipped_by_chars = skipped_by_chars.saturating_add(1);
                continue;
            }
            if is_too_similar(&similarity_keys, &prepared.similarity_key) {
                skipped_by_similarity = skipped_by_similarity.saturating_add(1);
                continue;
            }
            used_chars = used_chars.saturating_add(prepared.prompt_line_len);
            similarity_keys.push(prepared.similarity_key);
            let reason = relaxed_selector_reason(&prepared.hit);
            relaxed_quota_selected = relaxed_quota_selected.saturating_add(1);
            selected.push(annotate_selector_reason(prepared.hit, reason));
        }
    }

    let mut source_stats = per_source
        .into_iter()
        .map(
            |(source, selected_count)| ArchivePromptSelectionSourceStats {
                source,
                selected_count,
            },
        )
        .collect::<Vec<_>>();
    source_stats.sort_by_key(|item| item.source.label());
    let selection_note = if selected.is_empty() {
        Some("selector_kept_no_archive_hits".to_string())
    } else if relaxed_quota_selected > 0 {
        Some("selector_used_quota_relax_pass".to_string())
    } else {
        None
    };
    ArchivePromptSelectionResult {
        report: ArchivePromptSelectionReport {
            input_hits,
            selected_hits: selected.len(),
            max_items: policy.max_items,
            max_chars: policy.max_chars,
            skipped_by_chars,
            skipped_by_similarity,
            deferred_by_quota,
            relaxed_quota_selected,
            source_stats,
            selection_note,
        },
        hits: selected,
    }
}

fn prepare_archive_prompt_hits(mut hits: Vec<ArchiveSearchHit>) -> Vec<PreparedArchivePromptHit> {
    hits.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| b.observed_at.cmp(&a.observed_at))
            .then_with(|| a.citation.cmp(&b.citation))
    });
    hits.into_iter()
        .map(|hit| PreparedArchivePromptHit {
            similarity_key: normalized_similarity_key(&hit),
            prompt_line_len: archive_prompt_line_len(&hit),
            hit,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::{ArchiveRecordLocator, ArchiveRecordSource};

    fn build_hit(
        source: ArchiveRecordSource,
        title: &str,
        excerpt: &str,
        score: u32,
    ) -> ArchiveSearchHit {
        let locator = ArchiveRecordLocator {
            source,
            chat_id: Some("chat-1".to_string()),
            message_id: None,
            message_index: Some(0),
            note_name: None,
            req_id: None,
        };
        ArchiveSearchHit {
            record_id: locator.record_id(),
            citation: locator.citation(),
            locator,
            source,
            title: title.to_string(),
            excerpt: excerpt.to_string(),
            score,
            cues: vec!["test".to_string()],
            observed_at: Some(score as u64),
            retrieval_trace: Some(Default::default()),
        }
    }

    #[test]
    fn selector_enforces_diversity_and_source_quota() {
        let hits = vec![
            build_hit(
                ArchiveRecordSource::Transcript,
                "USER in chat-1",
                "memory pipeline still dominates the day",
                60,
            ),
            build_hit(
                ArchiveRecordSource::Transcript,
                "ASSISTANT in chat-1",
                "memory pipeline still dominates the day",
                58,
            ),
            build_hit(
                ArchiveRecordSource::DailyNote,
                "2026-04-03.md",
                "daily note says the memory pipeline is the main thread",
                55,
            ),
            build_hit(
                ArchiveRecordSource::TurnLog,
                "answered turn in chat-1",
                "reason=memory pipeline closeout",
                54,
            ),
        ];

        let selected =
            select_archive_hits_for_prompt_with_report(hits, MemoryProfile::Standard, 900);
        assert_eq!(selected.hits.len(), 3);
        assert_eq!(
            selected
                .hits
                .iter()
                .filter(|hit| hit.source == ArchiveRecordSource::Transcript)
                .count(),
            1
        );
        assert!(selected.hits.iter().all(|hit| {
            hit.retrieval_trace
                .as_ref()
                .and_then(|trace| trace.selector_reason.as_deref())
                .is_some()
        }));
        assert!(selected.report.deferred_by_quota + selected.report.skipped_by_similarity >= 1);
    }
}
