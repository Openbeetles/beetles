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

pub(crate) fn select_archive_hits_for_prompt(
    mut hits: Vec<ArchiveSearchHit>,
    profile: MemoryProfile,
    max_chars: usize,
) -> Vec<ArchiveSearchHit> {
    let policy = selector_policy(profile, max_chars);
    if hits.is_empty() || policy.max_items == 0 || policy.max_chars == 0 {
        return Vec::new();
    }

    hits.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| b.observed_at.cmp(&a.observed_at))
            .then_with(|| a.citation.cmp(&b.citation))
    });

    let mut selected = Vec::with_capacity(policy.max_items);
    let mut deferred = Vec::new();
    let mut used_chars = 0usize;
    let mut per_source = HashMap::<ArchiveRecordSource, usize>::new();
    let mut similarity_keys = Vec::with_capacity(policy.max_items);

    for hit in hits {
        if selected.len() >= policy.max_items {
            break;
        }
        let line_len = archive_prompt_line_len(&hit);
        if used_chars.saturating_add(line_len) > policy.max_chars {
            continue;
        }
        let key = normalized_similarity_key(&hit);
        if is_too_similar(&similarity_keys, &key) {
            continue;
        }
        let used = per_source.get(&hit.source).copied().unwrap_or(0);
        if used >= source_quota(policy, hit.source) {
            deferred.push((key, hit));
            continue;
        }
        used_chars = used_chars.saturating_add(line_len);
        *per_source.entry(hit.source).or_insert(0) += 1;
        similarity_keys.push(key);
        let reason = primary_selector_reason(&hit, used, source_quota(policy, hit.source));
        selected.push(annotate_selector_reason(hit, reason));
    }

    if selected.len() < policy.max_items {
        for (key, hit) in deferred {
            if selected.len() >= policy.max_items {
                break;
            }
            let line_len = archive_prompt_line_len(&hit);
            if used_chars.saturating_add(line_len) > policy.max_chars
                || is_too_similar(&similarity_keys, &key)
            {
                continue;
            }
            used_chars = used_chars.saturating_add(line_len);
            similarity_keys.push(key);
            let reason = relaxed_selector_reason(&hit);
            selected.push(annotate_selector_reason(hit, reason));
        }
    }

    selected
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

        let selected = select_archive_hits_for_prompt(hits, MemoryProfile::Standard, 900);
        assert_eq!(selected.len(), 3);
        assert_eq!(
            selected
                .iter()
                .filter(|hit| hit.source == ArchiveRecordSource::Transcript)
                .count(),
            1
        );
        assert!(selected.iter().all(|hit| {
            hit.retrieval_trace
                .as_ref()
                .and_then(|trace| trace.selector_reason.as_deref())
                .is_some()
        }));
    }
}
