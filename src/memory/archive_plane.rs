//! Archive evidence plane: transcript / daily notes / turn logs as non-canonical sources.

use crate::memory::{MemoryStore, SessionMessage, TurnLedgerStore};
use crate::util::truncate_content_to_max;

use super::{
    archive_match_score, collect_archive_match_terms, pick_archive_excerpt, MemoryProfile,
};

const MAX_ARCHIVE_EVIDENCE_BLOCK_LEN: usize = 768;
const MIN_ARCHIVE_EVIDENCE_BLOCK_LEN: usize = 220;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ArchiveEvidenceKind {
    Transcript,
    DailyNote,
    TurnLog,
}

impl ArchiveEvidenceKind {
    fn label(self) -> &'static str {
        match self {
            Self::Transcript => "transcript",
            Self::DailyNote => "daily note",
            Self::TurnLog => "turn log",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ArchiveEvidenceCandidate {
    score: u32,
    recency_rank: u8,
    kind: ArchiveEvidenceKind,
    title: String,
    content: String,
    cues: String,
}

pub fn build_archive_evidence_block(
    recent_messages: &[SessionMessage],
    memory_store: &dyn MemoryStore,
    turn_ledger_store: &dyn TurnLedgerStore,
    chat_id: &str,
    query: &str,
    system_max_len: usize,
    profile: MemoryProfile,
) -> Option<String> {
    let (max_messages, max_notes, max_items, line_chars, cap) = match profile {
        MemoryProfile::Standard => (6usize, 3usize, 4usize, 180usize, 768usize),
        MemoryProfile::Embedded => (4usize, 2usize, 3usize, 132usize, 512usize),
    };
    let block_max_len = system_max_len.min(cap).min(MAX_ARCHIVE_EVIDENCE_BLOCK_LEN);
    if block_max_len < MIN_ARCHIVE_EVIDENCE_BLOCK_LEN {
        return None;
    }

    let terms = collect_archive_match_terms(query);
    let weak_query = terms.is_empty() || query.trim().chars().count() <= 8;
    let mut candidates =
        Vec::with_capacity(max_messages.saturating_add(max_notes).saturating_add(1));

    let start = recent_messages.len().saturating_sub(max_messages);
    for (index, message) in recent_messages[start..].iter().enumerate() {
        let content = message.content.trim();
        if content.is_empty() {
            continue;
        }
        let preview = truncate_content_to_max(content, line_chars).to_string();
        let score = archive_match_score(&preview, message.role.as_str(), &terms).saturating_add(3);
        if !weak_query && score <= 3 {
            continue;
        }
        candidates.push(ArchiveEvidenceCandidate {
            score,
            recency_rank: (max_messages.saturating_sub(index)) as u8,
            kind: ArchiveEvidenceKind::Transcript,
            title: message.role.to_uppercase(),
            content: preview,
            cues: "recent interaction".to_string(),
        });
    }

    for (index, name) in memory_store
        .list_daily_note_names(max_notes)
        .unwrap_or_default()
        .into_iter()
        .enumerate()
    {
        let Ok(note) = memory_store.get_daily_note(&name) else {
            continue;
        };
        let excerpt = pick_archive_excerpt(&note, &terms, line_chars);
        if excerpt.is_empty() {
            continue;
        }
        let score = archive_match_score(&excerpt, &name, &terms).saturating_add(2);
        if !weak_query && score <= 2 {
            continue;
        }
        candidates.push(ArchiveEvidenceCandidate {
            score,
            recency_rank: (max_notes.saturating_sub(index)) as u8,
            kind: ArchiveEvidenceKind::DailyNote,
            title: name,
            content: excerpt,
            cues: "daily archive context".to_string(),
        });
    }

    if let Ok(Some(ledger)) = turn_ledger_store.get(chat_id) {
        let preview = [
            (!ledger.reason.trim().is_empty()).then(|| format!("reason={}", ledger.reason.trim())),
            (!ledger.user_preview.trim().is_empty())
                .then(|| format!("user={}", ledger.user_preview.trim())),
            (!ledger.reply_preview.trim().is_empty())
                .then(|| format!("reply={}", ledger.reply_preview.trim())),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join("; ");
        if !preview.is_empty() {
            let preview = truncate_content_to_max(&preview, line_chars).to_string();
            let score =
                archive_match_score(&preview, ledger.status.label(), &terms).saturating_add(2);
            if weak_query || score > 2 {
                candidates.push(ArchiveEvidenceCandidate {
                    score,
                    recency_rank: 0,
                    kind: ArchiveEvidenceKind::TurnLog,
                    title: ledger.status.label().to_string(),
                    content: preview,
                    cues: "execution log".to_string(),
                });
            }
        }
    }

    if candidates.is_empty() {
        return None;
    }

    candidates.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| b.recency_rank.cmp(&a.recency_rank))
            .then_with(|| a.kind.label().cmp(b.kind.label()))
    });
    candidates.truncate(max_items);

    let mut out = String::from(
        "## Archive evidence\nSupporting records only. These are evidence sources, not canonical shared memory. Use them to verify, cite, or distill factual memory updates.\n",
    );
    let mut appended = 0usize;
    for candidate in candidates {
        let line = format!(
            "- [{}:{}] {} (source: {}; {})",
            candidate.kind.label(),
            candidate.title,
            candidate.content,
            candidate.kind.label(),
            candidate.cues
        );
        if out.len().saturating_add(line.len()).saturating_add(1) > block_max_len {
            break;
        }
        out.push_str(&line);
        out.push('\n');
        appended += 1;
    }
    (appended > 0).then(|| out.trim_end().to_string())
}
