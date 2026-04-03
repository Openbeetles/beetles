//! Background hygiene jobs for archive evidence and factual-memory upkeep.

use crate::error::Result;
use crate::util::{current_unix_secs, truncate_content_to_max};

use super::{
    build_archive_reconcile_drafts, LongTermMemoryStore, MemoryProfile, MemoryStore, SessionStore,
    SessionSummaryStore, TurnLedgerStore,
};

const DAILY_AGGREGATE_MARKER: &str = "<!-- beetle:hygiene:daily-aggregate -->";
const DAILY_PLACEHOLDER_MARKER: &str = "<!-- beetle:hygiene:daily-placeholder -->";
const TRANSCRIPT_AGING_PREFIX: &str = "transcript-aging-";
const TRANSCRIPT_AGING_MAX_CHATS: usize = 4;
const DAILY_AGGREGATE_MIN_AGE_DAYS: u64 = 7;
const SESSION_GC_AGE_SECS: u64 = 21 * 86_400;

pub struct MemoryHygieneContext<'a> {
    pub session_store: &'a dyn SessionStore,
    pub session_summary_store: &'a dyn SessionSummaryStore,
    pub memory_store: &'a dyn MemoryStore,
    pub turn_ledger_store: &'a dyn TurnLedgerStore,
    pub long_term_memory_store: &'a dyn LongTermMemoryStore,
}

#[derive(Default, Clone, Debug, PartialEq, Eq)]
pub struct MemoryHygieneOutcome {
    pub daily_notes_aggregated: usize,
    pub transcripts_rolled_up: usize,
    pub sessions_gc: usize,
    pub factual_metadata_updates: usize,
}

pub fn run_memory_hygiene_jobs(
    ctx: MemoryHygieneContext<'_>,
    current_chat_id: &str,
    profile: MemoryProfile,
    now_secs: u64,
) -> MemoryHygieneOutcome {
    let mut outcome = MemoryHygieneOutcome::default();
    outcome.daily_notes_aggregated =
        aggregate_old_daily_notes(ctx.memory_store, now_secs).unwrap_or(0);
    outcome.transcripts_rolled_up = rollup_aging_transcripts(
        ctx.session_store,
        ctx.session_summary_store,
        ctx.memory_store,
    )
    .unwrap_or(0);
    outcome.sessions_gc = ctx.session_store.gc_stale(SESSION_GC_AGE_SECS).unwrap_or(0);
    let factual_drafts = build_archive_reconcile_drafts(
        ctx.session_store,
        ctx.long_term_memory_store,
        ctx.memory_store,
        ctx.turn_ledger_store,
        current_chat_id,
        profile,
        6,
    );
    if !factual_drafts.is_empty() {
        outcome.factual_metadata_updates = ctx
            .long_term_memory_store
            .upsert_many(
                &factual_drafts,
                if now_secs > 0 {
                    now_secs
                } else {
                    current_unix_secs()
                },
            )
            .unwrap_or(0);
    }
    outcome
}

fn aggregate_old_daily_notes(store: &dyn MemoryStore, now_secs: u64) -> Result<usize> {
    let mut aggregated = 0usize;
    let mut monthly: std::collections::BTreeMap<String, Vec<String>> =
        std::collections::BTreeMap::new();
    for name in store.list_daily_note_names(usize::MAX)? {
        if !name.ends_with(".md")
            || name.contains("archive")
            || name.starts_with(TRANSCRIPT_AGING_PREFIX)
            || super::parse_daily_note_observed_at(&name).is_none()
        {
            continue;
        }
        let observed_at = super::parse_daily_note_observed_at(&name).unwrap_or(0);
        if now_secs > 0
            && now_secs.saturating_sub(observed_at) < DAILY_AGGREGATE_MIN_AGE_DAYS * 86_400
        {
            continue;
        }
        let content = store.get_daily_note(&name)?;
        if content.contains(DAILY_PLACEHOLDER_MARKER) {
            continue;
        }
        let month_key = name.chars().take(7).collect::<String>();
        monthly.entry(month_key).or_default().push(name);
    }

    for (month_key, mut names) in monthly {
        if names.len() < 2 {
            continue;
        }
        names.sort();
        let aggregate_name = format!("{month_key}-archive.md");
        let mut body = String::new();
        body.push_str(DAILY_AGGREGATE_MARKER);
        body.push_str("\n# Daily Aggregate ");
        body.push_str(&month_key);
        body.push_str("\n\n");
        for name in &names {
            let content = store.get_daily_note(name)?;
            let preview = truncate_content_to_max(content.trim(), 220);
            body.push_str("- ");
            body.push_str(name);
            body.push_str(": ");
            body.push_str(preview.as_ref());
            body.push('\n');
        }
        store.write_daily_note(&aggregate_name, body.trim_end())?;
        for name in names {
            let content = store.get_daily_note(&name)?;
            let preview = truncate_content_to_max(content.trim(), 160);
            let placeholder = format!(
                "{DAILY_PLACEHOLDER_MARKER}\nArchived into {aggregate_name}.\nSummary: {}",
                preview
            );
            store.write_daily_note(&name, &placeholder)?;
            aggregated = aggregated.saturating_add(1);
        }
    }
    Ok(aggregated)
}

fn rollup_aging_transcripts(
    session_store: &dyn SessionStore,
    session_summary_store: &dyn SessionSummaryStore,
    memory_store: &dyn MemoryStore,
) -> Result<usize> {
    let mut rolled = 0usize;
    for chat_id in session_store
        .list_chat_ids()?
        .into_iter()
        .take(TRANSCRIPT_AGING_MAX_CHATS)
    {
        let Some((summary, count)) = session_summary_store.get_with_count(&chat_id)? else {
            continue;
        };
        if summary.trim().is_empty() {
            continue;
        }
        let note_name = format!("{TRANSCRIPT_AGING_PREFIX}{}.md", short_chat_slug(&chat_id));
        let content = format!(
            "<!-- beetle:hygiene:transcript-rollup -->\nChat: {chat_id}\nMessages summarized: {count}\n\n{summary}"
        );
        memory_store.write_daily_note(&note_name, &content)?;
        rolled = rolled.saturating_add(1);
    }
    Ok(rolled)
}

fn short_chat_slug(chat_id: &str) -> String {
    let slug = chat_id
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '-' })
        .collect::<String>();
    slug.chars().take(24).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Result;
    use crate::memory::SessionMessage;
    use std::collections::HashMap;
    use std::sync::Mutex;

    #[derive(Default)]
    struct StubMemoryStore {
        notes: Mutex<HashMap<String, String>>,
    }

    impl crate::memory::MemoryStore for StubMemoryStore {
        fn get_memory(&self) -> Result<String> {
            Ok(String::new())
        }
        fn set_memory(&self, _content: &str) -> Result<()> {
            Ok(())
        }
        fn get_soul(&self) -> Result<String> {
            Ok(String::new())
        }
        fn set_soul(&self, _content: &str) -> Result<()> {
            Ok(())
        }
        fn get_user(&self) -> Result<String> {
            Ok(String::new())
        }
        fn set_user(&self, _content: &str) -> Result<()> {
            Ok(())
        }
        fn list_daily_note_names(&self, recent_n: usize) -> Result<Vec<String>> {
            let mut names = self
                .notes
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .keys()
                .cloned()
                .collect::<Vec<_>>();
            names.sort_by(|a, b| b.cmp(a));
            names.truncate(recent_n);
            Ok(names)
        }
        fn get_daily_note(&self, name: &str) -> Result<String> {
            Ok(self
                .notes
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(name)
                .cloned()
                .unwrap_or_default())
        }
        fn write_daily_note(&self, name: &str, content: &str) -> Result<()> {
            self.notes
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(name.to_string(), content.to_string());
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubSessionStore;

    impl crate::memory::SessionStore for StubSessionStore {
        fn append(&self, _chat_id: &str, _role: &str, _content: &str) -> Result<()> {
            Ok(())
        }
        fn load_recent(&self, _chat_id: &str, _n: usize) -> Result<Vec<SessionMessage>> {
            Ok(Vec::new())
        }
        fn clear(&self, _chat_id: &str) -> Result<()> {
            Ok(())
        }
        fn list_chat_ids(&self) -> Result<Vec<String>> {
            Ok(vec!["chat-1".to_string()])
        }
    }

    #[derive(Default)]
    struct StubSummaryStore;

    impl crate::memory::SessionSummaryStore for StubSummaryStore {
        fn get(&self, _chat_id: &str) -> Result<Option<String>> {
            Ok(Some("summary".to_string()))
        }
        fn set(&self, _chat_id: &str, _summary: &str) -> Result<()> {
            Ok(())
        }
        fn get_with_count(&self, _chat_id: &str) -> Result<Option<(String, usize)>> {
            Ok(Some(("summary".to_string(), 8)))
        }
    }

    #[test]
    fn aggregates_old_daily_notes_into_month_archive() {
        let store = StubMemoryStore::default();
        store.write_daily_note("2026-03-01.md", "第一天").unwrap();
        store.write_daily_note("2026-03-02.md", "第二天").unwrap();
        let changed = aggregate_old_daily_notes(&store, 1_775_000_000).unwrap();
        assert_eq!(changed, 2);
        let aggregate = store.get_daily_note("2026-03-archive.md").unwrap();
        assert!(aggregate.contains("2026-03-01.md"));
        assert!(store
            .get_daily_note("2026-03-01.md")
            .unwrap()
            .contains("Archived into"));
    }

    #[test]
    fn transcript_rollup_writes_daily_note() {
        let session_store = StubSessionStore;
        let summary_store = StubSummaryStore;
        let memory_store = StubMemoryStore::default();
        let rolled =
            rollup_aging_transcripts(&session_store, &summary_store, &memory_store).unwrap();
        assert_eq!(rolled, 1);
        assert!(memory_store
            .get_daily_note("transcript-aging-chat-1.md")
            .unwrap()
            .contains("summary"));
    }
}
