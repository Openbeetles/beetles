//! Prompt 侧共享记忆读装配。
//! Shared prompt memory loading for agent context construction.

use super::{
    recall_long_term_memory_block, LongTermMemoryStore, MemoryProfile, SessionSummaryStore,
};

pub struct PromptMemoryContext {
    pub summary_text: Option<String>,
    pub long_term_memory_text: Option<String>,
}

pub struct PromptMemoryContextParams<'a> {
    pub chat_id: &'a str,
    pub user_query: &'a str,
    pub system_max_len: usize,
    pub profile: MemoryProfile,
    pub session_summary_store: &'a dyn SessionSummaryStore,
    pub long_term_memory_store: &'a dyn LongTermMemoryStore,
}

pub fn load_prompt_memory_context(params: PromptMemoryContextParams<'_>) -> PromptMemoryContext {
    let summary_text = params
        .session_summary_store
        .get_with_count(params.chat_id)
        .ok()
        .flatten()
        .map(|(summary, _)| summary);
    let long_term_memory_text = recall_long_term_memory_block(
        params.long_term_memory_store,
        params.chat_id,
        params.user_query,
        summary_text.as_deref(),
        params.system_max_len,
        params.profile,
    );
    PromptMemoryContext {
        summary_text,
        long_term_memory_text,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Result;
    use crate::memory::{
        LongTermMemoryEntry, LongTermMemoryKind, LongTermMemorySlot, LongTermMemoryStore,
        SessionSummaryStore,
    };
    use std::sync::Mutex;

    #[derive(Default)]
    struct StubSessionSummaryStore {
        summary: Mutex<Option<(String, usize)>>,
    }

    impl SessionSummaryStore for StubSessionSummaryStore {
        fn get(&self, _chat_id: &str) -> Result<Option<String>> {
            Ok(self
                .summary
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .as_ref()
                .map(|(summary, _)| summary.clone()))
        }

        fn set(&self, _chat_id: &str, _summary: &str) -> Result<()> {
            Ok(())
        }

        fn get_with_count(&self, _chat_id: &str) -> Result<Option<(String, usize)>> {
            Ok(self
                .summary
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone())
        }
    }

    #[derive(Default)]
    struct StubLongTermMemoryStore {
        entries: Mutex<Vec<LongTermMemoryEntry>>,
        last_query: Mutex<Option<String>>,
    }

    impl LongTermMemoryStore for StubLongTermMemoryStore {
        fn upsert_many(
            &self,
            _drafts: &[crate::memory::LongTermMemoryDraft],
            _now_secs: u64,
        ) -> Result<usize> {
            unreachable!()
        }

        fn list(&self, _limit: usize) -> Result<Vec<LongTermMemoryEntry>> {
            Ok(self
                .entries
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone())
        }

        fn recall(
            &self,
            query: &str,
            _chat_id: Option<&str>,
            _limit: usize,
        ) -> Result<Vec<LongTermMemoryEntry>> {
            *self.last_query.lock().unwrap_or_else(|e| e.into_inner()) = Some(query.to_string());
            Ok(self
                .entries
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone())
        }

        fn get(&self, _id: &str) -> Result<Option<LongTermMemoryEntry>> {
            unreachable!()
        }

        fn delete(&self, _id: &str) -> Result<bool> {
            unreachable!()
        }

        fn delete_slot(&self, _slot: &LongTermMemorySlot) -> Result<bool> {
            unreachable!()
        }

        fn count(&self) -> Result<usize> {
            unreachable!()
        }
    }

    #[test]
    fn loads_summary_and_uses_it_for_weak_query_recall() {
        let summary_store = StubSessionSummaryStore {
            summary: Mutex::new(Some(("user prefers cold brew".to_string(), 6))),
        };
        let memory_store = StubLongTermMemoryStore {
            entries: Mutex::new(vec![LongTermMemoryEntry {
                id: "pref-coffee".to_string(),
                kind: LongTermMemoryKind::Preference,
                topic: "coffee".to_string(),
                content: "Likes cold brew".to_string(),
                keywords: vec!["coffee".to_string()],
                source_chat_id: Some("chat-1".to_string()),
                created_at: 1,
                updated_at: 1,
            }]),
            last_query: Mutex::new(None),
        };

        let context = load_prompt_memory_context(PromptMemoryContextParams {
            chat_id: "chat-1",
            user_query: "嗯?",
            system_max_len: 1024,
            profile: MemoryProfile::Standard,
            session_summary_store: &summary_store,
            long_term_memory_store: &memory_store,
        });

        assert_eq!(
            context.summary_text.as_deref(),
            Some("user prefers cold brew")
        );
        assert!(context
            .long_term_memory_text
            .as_deref()
            .unwrap_or_default()
            .contains("Likes cold brew"));
        assert!(memory_store
            .last_query
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_deref()
            .unwrap_or_default()
            .contains("user prefers cold brew"));
    }
}
