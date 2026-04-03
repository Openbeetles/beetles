//! Background hygiene jobs for archive evidence and factual-memory upkeep.

use crate::error::Result;
use crate::platform::SkillStorage;
use crate::skills::{govern_runtime_skills, RuntimeSkillGovernanceOutcome};
use crate::util::{current_unix_secs, truncate_content_to_max};

use super::{
    build_archive_reconcile_drafts, maintain_archive_search_backend, LongTermMemoryDraft,
    LongTermMemoryStore, MemoryProfile, MemoryStore, SessionStore, SessionSummaryStore,
    TurnLedgerStore,
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
    pub skill_storage: &'a dyn SkillStorage,
}

#[derive(Default, Clone, Debug, PartialEq, Eq)]
pub struct MemoryHygieneOutcome {
    pub daily_notes_aggregated: usize,
    pub transcripts_rolled_up: usize,
    pub sessions_gc: usize,
    pub factual_metadata_updates: usize,
    pub factual_evidence_compacted: usize,
    pub archive_index_maintained: bool,
    pub runtime_skill_governance: RuntimeSkillGovernanceOutcome,
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
    outcome.factual_evidence_compacted = compact_factual_evidence_metadata(
        ctx.long_term_memory_store,
        &factual_drafts,
        if now_secs > 0 {
            now_secs
        } else {
            current_unix_secs()
        },
    )
    .unwrap_or(0);
    outcome.archive_index_maintained =
        maintain_archive_search_backend(ctx.session_store, ctx.memory_store, ctx.turn_ledger_store)
            .unwrap_or(false);
    outcome.runtime_skill_governance = govern_runtime_skills(
        ctx.skill_storage,
        if now_secs > 0 {
            now_secs
        } else {
            current_unix_secs()
        },
    )
    .unwrap_or_default();
    outcome
}

fn compact_factual_evidence_metadata(
    store: &dyn LongTermMemoryStore,
    reconcile_drafts: &[LongTermMemoryDraft],
    now_secs: u64,
) -> Result<usize> {
    let mut compacted = Vec::new();
    let list_limit = store.count().unwrap_or(24).max(24);
    for entry in store.list(list_limit)? {
        let mut citations = entry.supporting_citations.clone();
        citations.sort();
        citations.dedup();
        citations.truncate(6);
        let mut should_compact = citations != entry.supporting_citations;
        let mut draft = LongTermMemoryDraft {
            kind: entry.kind.clone(),
            topic: entry.topic.clone(),
            content: entry.content.clone(),
            keywords: entry.keywords.clone(),
            source_chat_id: entry.source_chat_id.clone(),
            source_type: Some(entry.source_type),
            source_scope: Some(entry.source_scope),
            confidence: Some(entry.confidence),
            freshness: Some(entry.freshness),
            stale_hint: Some(entry.stale_hint),
            supporting_citations: citations,
            evidence_count: Some(
                entry
                    .evidence_count
                    .min(6)
                    .max(entry.supporting_citations.len().min(6) as u32),
            ),
            observed_at: Some(entry.observed_at),
            last_confirmed_at: Some(
                entry
                    .last_confirmed_at
                    .max(entry.observed_at)
                    .max(entry.updated_at),
            ),
            source_revision: Some(entry.source_revision),
        };
        if draft.evidence_count != Some(entry.evidence_count)
            || draft.last_confirmed_at != Some(entry.last_confirmed_at)
        {
            should_compact = true;
        }
        if let Some(reconcile) = reconcile_drafts
            .iter()
            .find(|candidate| candidate.kind == entry.kind && candidate.topic == entry.topic)
        {
            if reconcile.last_confirmed_at.unwrap_or(0) > draft.last_confirmed_at.unwrap_or(0) {
                draft.last_confirmed_at = reconcile.last_confirmed_at;
                should_compact = true;
            }
            if reconcile.evidence_count.unwrap_or(0) > draft.evidence_count.unwrap_or(0) {
                draft.evidence_count = reconcile.evidence_count;
                should_compact = true;
            }
        }
        if should_compact {
            compacted.push(draft);
        }
    }
    if compacted.is_empty() {
        Ok(0)
    } else {
        store.upsert_many(&compacted, now_secs)
    }
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
    use crate::memory::{
        LongTermMemoryConfidence, LongTermMemoryDraft, LongTermMemoryEntry,
        LongTermMemoryFreshness, LongTermMemoryKind, LongTermMemorySlot, LongTermMemorySourceScope,
        LongTermMemorySourceType, LongTermMemoryStaleHint, SessionMessage, TurnLedger,
    };
    use crate::platform::SkillStorage;
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

    #[derive(Default)]
    struct StubTurnLedgerStore;

    impl crate::memory::TurnLedgerStore for StubTurnLedgerStore {
        fn get(&self, _chat_id: &str) -> Result<Option<TurnLedger>> {
            Ok(None)
        }

        fn set(&self, _chat_id: &str, _ledger: &TurnLedger) -> Result<()> {
            Ok(())
        }

        fn clear(&self, _chat_id: &str) -> Result<()> {
            Ok(())
        }
    }

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
            Ok(self
                .files
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(name)
                .cloned()
                .unwrap_or_default())
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

    #[derive(Default)]
    struct StubLongTermMemoryStore {
        entries: Mutex<Vec<LongTermMemoryEntry>>,
        upserts: Mutex<Vec<Vec<LongTermMemoryDraft>>>,
    }

    impl crate::memory::LongTermMemoryStore for StubLongTermMemoryStore {
        fn upsert_many(&self, drafts: &[LongTermMemoryDraft], _now_secs: u64) -> Result<usize> {
            self.upserts
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(drafts.to_vec());
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

        fn get(&self, id: &str) -> Result<Option<LongTermMemoryEntry>> {
            Ok(self
                .entries
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .iter()
                .find(|entry| entry.id == id)
                .cloned())
        }

        fn list(&self, limit: usize) -> Result<Vec<LongTermMemoryEntry>> {
            let mut entries = self
                .entries
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone();
            entries.truncate(limit);
            Ok(entries)
        }

        fn delete(&self, _id: &str) -> Result<bool> {
            Ok(false)
        }

        fn delete_slot(&self, _slot: &LongTermMemorySlot) -> Result<bool> {
            Ok(false)
        }

        fn count(&self) -> Result<usize> {
            Ok(self.entries.lock().unwrap_or_else(|e| e.into_inner()).len())
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

    #[test]
    fn factual_evidence_compaction_preserves_observed_at_and_deduplicates_citations() {
        let store = StubLongTermMemoryStore {
            entries: Mutex::new(vec![LongTermMemoryEntry {
                id: "fact:router".to_string(),
                kind: LongTermMemoryKind::Fact,
                topic: "router_position".to_string(),
                content: "Router sits near the window.".to_string(),
                keywords: vec!["router".to_string()],
                source_chat_id: Some("chat-1".to_string()),
                source_type: LongTermMemorySourceType::Conversation,
                source_scope: LongTermMemorySourceScope::User,
                confidence: LongTermMemoryConfidence::Medium,
                freshness: LongTermMemoryFreshness::Stable,
                stale_hint: LongTermMemoryStaleHint::None,
                supporting_citations: vec![
                    "transcript:chat-1#message=1".to_string(),
                    "transcript:chat-1#message=1".to_string(),
                    "daily_note:2026-04-02.md".to_string(),
                ],
                evidence_count: 1,
                created_at: 3,
                updated_at: 9,
                observed_at: 7,
                last_confirmed_at: 5,
                source_revision: 0,
                last_used_at: 0,
            }]),
            upserts: Mutex::new(Vec::new()),
        };

        let changed = compact_factual_evidence_metadata(&store, &[], 100).unwrap();
        assert_eq!(changed, 1);
        let upserts = store.upserts.lock().unwrap_or_else(|e| e.into_inner());
        let draft = &upserts[0][0];
        assert_eq!(draft.observed_at, Some(7));
        assert_eq!(draft.last_confirmed_at, Some(9));
        assert_eq!(draft.supporting_citations.len(), 2);
        assert_eq!(draft.evidence_count, Some(3));
    }

    #[test]
    fn hygiene_runs_runtime_skill_governance() {
        let session_store = StubSessionStore;
        let summary_store = StubSummaryStore;
        let memory_store = StubMemoryStore::default();
        let long_term_memory_store = StubLongTermMemoryStore::default();
        let turn_ledger_store = StubTurnLedgerStore;
        let skill_storage = StubSkillStorage::default();
        skill_storage
            .write(
                "runtime_skill__temp_probe",
                br#"<!-- beetle:runtime-skill -->
# Temp probe

Type: procedural_runtime_skill
Topic: temp_probe
Source chat: chat-1
Status: active
Observed at: 1
Updated at: 1
Use count: 0
Quality: 20

## Summary

## Procedure
probe"#,
            )
            .unwrap();

        let outcome = run_memory_hygiene_jobs(
            MemoryHygieneContext {
                session_store: &session_store,
                session_summary_store: &summary_store,
                memory_store: &memory_store,
                turn_ledger_store: &turn_ledger_store,
                long_term_memory_store: &long_term_memory_store,
                skill_storage: &skill_storage,
            },
            "chat-1",
            crate::memory::MemoryProfile::Embedded,
            90 * 86_400 + 10,
        );

        assert_eq!(outcome.runtime_skill_governance.pruned, 1);
        assert!(skill_storage
            .files
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_empty());
    }
}
