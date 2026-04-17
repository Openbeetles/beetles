//! Archive evidence plane: transcript / daily notes / turn logs as non-canonical sources.

use crate::memory::{MemoryStore, SessionStore, TurnLedgerStore};
use crate::util::truncate_content_to_max;

use super::{
    memory_capability_profile, render_turn_observation_ledger_block,
    search_archive_records_detailed, select_archive_hits_for_prompt_with_report,
    ArchivePromptSelectionReport, ArchiveSearchQuery, ArchiveSearchQueryReport, MemoryProfile,
    MAX_ARCHIVE_SEARCH_LIMIT,
};

const MAX_ARCHIVE_EVIDENCE_BLOCK_LEN: usize = 768;
const MIN_ARCHIVE_EVIDENCE_BLOCK_LEN: usize = 220;

fn render_archive_trace_summary(hit: &super::ArchiveSearchHit) -> Option<String> {
    let trace = hit.retrieval_trace.as_ref()?;
    let mut parts = Vec::with_capacity(4);
    if let Some(reason) = trace.ranking_reason.as_deref() {
        parts.push(reason.to_string());
    }
    if let Some(reason) = trace.source_reason.as_deref() {
        parts.push(reason.to_string());
    }
    if let Some(reason) = trace.recency_reason.as_deref() {
        parts.push(reason.to_string());
    }
    if let Some(reason) = trace.selector_reason.as_deref() {
        parts.push(reason.to_string());
    }
    if parts.is_empty() {
        None
    } else {
        Some(truncate_content_to_max(&parts.join("; "), 180).into_owned())
    }
}

fn render_archive_recall_note(
    search_report: &ArchiveSearchQueryReport,
    selector_report: Option<&ArchivePromptSelectionReport>,
) -> Option<String> {
    let mut parts = vec![
        format!("backend={:?}", search_report.backend),
        format!("candidates={}", search_report.candidate_count),
        format!("hits={}", search_report.returned_hit_count),
    ];
    if let Some(selector_report) = selector_report {
        parts.push(format!("selected={}", selector_report.selected_hits));
    }
    if let Some(reason) = search_report.miss_reason.as_deref() {
        parts.push(format!("miss={reason}"));
    }
    if let Some(note) = selector_report.and_then(|report| report.selection_note.as_deref()) {
        parts.push(format!("selector={note}"));
    }
    (!parts.is_empty()).then(|| format!("- recall trace: {}", parts.join("; ")))
}

fn prepend_archive_recall_note(
    mut block: String,
    search_report: &ArchiveSearchQueryReport,
    selector_report: Option<&ArchivePromptSelectionReport>,
    block_max_len: usize,
) -> String {
    let Some(note) = render_archive_recall_note(search_report, selector_report) else {
        return block;
    };
    if let Some(insert_at) = block.find('\n') {
        let insertion = format!("{note}\n");
        if block.len().saturating_add(insertion.len()) <= block_max_len {
            block.insert_str(insert_at.saturating_add(1), &insertion);
        }
    }
    truncate_content_to_max(block.trim_end(), block_max_len).into_owned()
}

pub fn build_archive_evidence_block(
    session_store: &dyn SessionStore,
    memory_store: &dyn MemoryStore,
    turn_ledger_store: &dyn TurnLedgerStore,
    chat_id: &str,
    query: &str,
    system_max_len: usize,
    profile: MemoryProfile,
) -> Option<String> {
    let capability = memory_capability_profile(profile);
    let max_items = capability.archive_prompt_max_items;
    let cap = capability.archive_prompt_max_chars;
    let block_max_len = system_max_len.min(cap).min(MAX_ARCHIVE_EVIDENCE_BLOCK_LEN);
    if block_max_len < MIN_ARCHIVE_EVIDENCE_BLOCK_LEN {
        return None;
    }
    let search = search_archive_records_detailed(
        session_store,
        memory_store,
        turn_ledger_store,
        ArchiveSearchQuery {
            query,
            preferred_chat_id: Some(chat_id),
            chat_id_filter: None,
            sources: &[],
            limit: MAX_ARCHIVE_SEARCH_LIMIT,
        },
    )
    .ok()?;
    let selection = select_archive_hits_for_prompt_with_report(
        search.hits.clone(),
        profile,
        block_max_len.saturating_sub(128),
    );
    let selection_report = selection.report.clone();
    let selected = selection.hits;
    if selected.is_empty() {
        return build_archive_fallback_block(
            session_store,
            memory_store,
            turn_ledger_store,
            chat_id,
            block_max_len,
        )
        .map(|block| {
            prepend_archive_recall_note(
                block,
                &search.report,
                Some(&selection_report),
                block_max_len,
            )
        })
        .or_else(|| {
            render_archive_recall_note(&search.report, Some(&selection_report)).map(|note| {
                truncate_content_to_max(
                    &format!(
                        "## Archive evidence\nSupporting records only. These are evidence sources, not canonical shared memory. Use them to verify, cite, or distill factual memory updates.\n{}",
                        note
                    ),
                    block_max_len,
                )
                .into_owned()
            })
        });
    }

    let mut out = String::from(
        "## Archive evidence\nSupporting records only. These are evidence sources, not canonical shared memory. Use them to verify, cite, or distill factual memory updates.\n",
    );
    if let Some(note) = render_archive_recall_note(&search.report, Some(&selection_report)) {
        if out.len().saturating_add(note.len()).saturating_add(1) <= block_max_len {
            out.push_str(&note);
            out.push('\n');
        }
    }
    let mut appended = 0usize;
    for hit in selected.into_iter().take(max_items) {
        let cue_summary = if hit.cues.is_empty() {
            "supporting evidence".to_string()
        } else {
            hit.cues.join(", ")
        };
        let trace_summary = render_archive_trace_summary(&hit);
        let line = if let Some(trace_summary) = trace_summary {
            format!(
                "- [{}] {} (citation: {}; {}; why: {})",
                hit.title, hit.excerpt, hit.citation, cue_summary, trace_summary
            )
        } else {
            format!(
                "- [{}] {} (citation: {}; {})",
                hit.title, hit.excerpt, hit.citation, cue_summary
            )
        };
        if out.len().saturating_add(line.len()).saturating_add(1) > block_max_len {
            break;
        }
        out.push_str(&line);
        out.push('\n');
        appended += 1;
    }
    (appended > 0).then(|| out.trim_end().to_string())
}

fn build_archive_fallback_block(
    session_store: &dyn SessionStore,
    memory_store: &dyn MemoryStore,
    turn_ledger_store: &dyn TurnLedgerStore,
    chat_id: &str,
    block_max_len: usize,
) -> Option<String> {
    let mut out = String::from(
        "## Archive evidence\nSupporting records only. These are evidence sources, not canonical shared memory. Use them to verify, cite, or distill factual memory updates.\n",
    );
    let mut appended = 0usize;
    for message in session_store
        .load_recent(chat_id, 3)
        .unwrap_or_default()
        .into_iter()
        .rev()
    {
        let content = message.content.trim();
        if content.is_empty() {
            continue;
        }
        let line = format!(
            "- [{}] {} (fallback transcript)",
            message.role.to_uppercase(),
            truncate_content_to_max(content, 160)
        );
        if out.len().saturating_add(line.len()).saturating_add(1) > block_max_len {
            break;
        }
        out.push_str(&line);
        out.push('\n');
        appended += 1;
    }
    if appended == 0 {
        if let Some(name) = memory_store
            .list_daily_note_names(1)
            .unwrap_or_default()
            .into_iter()
            .next()
        {
            if let Ok(note) = memory_store.get_daily_note(&name) {
                let line = format!(
                    "- [{}] {} (fallback daily note)",
                    name,
                    truncate_content_to_max(note.trim(), 160)
                );
                if out.len().saturating_add(line.len()).saturating_add(1) <= block_max_len {
                    out.push_str(&line);
                    out.push('\n');
                    appended += 1;
                }
            }
        }
    }
    if appended == 0 {
        match turn_ledger_store.get(chat_id) {
            Ok(Some(ledger)) => {
                let mut preview = [
                    (!ledger.reason.trim().is_empty())
                        .then(|| format!("reason={}", ledger.reason.trim())),
                    (!ledger.user_preview.trim().is_empty())
                        .then(|| format!("user={}", ledger.user_preview.trim())),
                    (!ledger.reply_preview.trim().is_empty())
                        .then(|| format!("reply={}", ledger.reply_preview.trim())),
                ]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>();
                if let Some(observation_summary) =
                    ledger.observation.as_ref().and_then(|observation| {
                        render_turn_observation_ledger_block(observation, 220)
                            .map(|block| block.lines().skip(1).collect::<Vec<_>>().join(" | "))
                    })
                {
                    preview.push(format!("observation={observation_summary}"));
                }
                let preview = preview.join("; ");
                if !preview.is_empty() {
                    let line = format!(
                        "- [turn log] {} (fallback execution log)",
                        truncate_content_to_max(&preview, 160)
                    );
                    if out.len().saturating_add(line.len()).saturating_add(1) <= block_max_len {
                        out.push_str(&line);
                        out.push('\n');
                        appended += 1;
                    }
                }
            }
            Ok(None) => {}
            Err(error) => {
                let line = format!(
                    "- [turn log] unavailable (fallback degraded: {})",
                    error.stage()
                );
                if out.len().saturating_add(line.len()).saturating_add(1) <= block_max_len {
                    out.push_str(&line);
                    out.push('\n');
                    appended += 1;
                }
            }
        }
    }
    (appended > 0).then(|| out.trim_end().to_string())
}
