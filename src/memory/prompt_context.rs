//! Prompt 侧共享记忆读装配。
//! Shared prompt memory loading for agent context construction.

use super::{
    memory_policy, recall_long_term_memory_block, render_execution_state_block,
    ExecutionStateStore, LongTermMemoryStore, MemoryProfile, SessionStore, SessionSummaryStore,
};

pub struct PromptMemoryContext {
    pub summary_text: Option<String>,
    pub long_term_memory_text: Option<String>,
    pub execution_state_text: Option<String>,
}

pub struct PromptMemoryContextParams<'a> {
    pub chat_id: &'a str,
    pub user_query: &'a str,
    pub system_max_len: usize,
    pub profile: MemoryProfile,
    pub session_store: &'a dyn SessionStore,
    pub session_summary_store: &'a dyn SessionSummaryStore,
    pub long_term_memory_store: &'a dyn LongTermMemoryStore,
    pub execution_state_store: &'a dyn ExecutionStateStore,
}

pub fn load_prompt_memory_context(params: PromptMemoryContextParams<'_>) -> PromptMemoryContext {
    let recall_policy = memory_policy(params.profile).long_term_recall;
    let summary_text = params
        .session_summary_store
        .get_with_count(params.chat_id)
        .ok()
        .flatten()
        .map(|(summary, _)| summary);
    let long_term_memory_text = if params.system_max_len < recall_policy.block_min_len {
        None
    } else {
        let recent_messages = params
            .session_store
            .load_recent(params.chat_id, recall_policy.recent_grounding_message_count)
            .unwrap_or_default();
        recall_long_term_memory_block(
            params.long_term_memory_store,
            params.chat_id,
            params.user_query,
            summary_text.as_deref(),
            &recent_messages,
            params.system_max_len,
            params.profile,
        )
    };
    let execution_state_text = params
        .execution_state_store
        .get(params.chat_id)
        .ok()
        .flatten()
        .and_then(|state| {
            render_execution_state_block(
                &state,
                memory_policy(params.profile).execution_state.render_max_len,
            )
        });
    PromptMemoryContext {
        summary_text,
        long_term_memory_text,
        execution_state_text,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Result;
    use crate::memory::{
        ExecutionState, ExecutionStateStore, ExecutionStatus, LongTermMemoryEntry,
        LongTermMemoryKind, LongTermMemorySlot, LongTermMemoryStore, SessionMessage, SessionStore,
        SessionSummaryStore,
    };
    use std::sync::Mutex;

    #[derive(Default)]
    struct StubSessionStore {
        recent: Mutex<Vec<SessionMessage>>,
    }

    impl SessionStore for StubSessionStore {
        fn append(&self, _chat_id: &str, _role: &str, _content: &str) -> Result<()> {
            Ok(())
        }

        fn load_recent(&self, _chat_id: &str, limit: usize) -> Result<Vec<SessionMessage>> {
            let recent = self.recent.lock().unwrap_or_else(|e| e.into_inner());
            let start = recent.len().saturating_sub(limit);
            Ok(recent[start..].to_vec())
        }

        fn clear(&self, _chat_id: &str) -> Result<()> {
            Ok(())
        }

        fn list_chat_ids(&self) -> Result<Vec<String>> {
            Ok(Vec::new())
        }
    }

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

    #[derive(Default)]
    struct StubExecutionStateStore {
        state: Mutex<Option<ExecutionState>>,
    }

    impl ExecutionStateStore for StubExecutionStateStore {
        fn get(&self, _chat_id: &str) -> Result<Option<ExecutionState>> {
            Ok(self.state.lock().unwrap_or_else(|e| e.into_inner()).clone())
        }

        fn set(&self, _chat_id: &str, _state: &ExecutionState) -> Result<()> {
            Ok(())
        }

        fn clear(&self, _chat_id: &str) -> Result<()> {
            Ok(())
        }
    }

    #[test]
    fn loads_summary_and_uses_it_for_weak_query_recall() {
        let session_store = StubSessionStore {
            recent: Mutex::new(vec![
                SessionMessage {
                    role: "assistant".to_string(),
                    content: "我们继续收口甲壳虫的长期记忆".to_string(),
                },
                SessionMessage {
                    role: "user".to_string(),
                    content: "重点是咖啡偏好和昵称".to_string(),
                },
            ]),
        };
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
        let execution_state_store = StubExecutionStateStore {
            state: Mutex::new(Some(ExecutionState {
                status: ExecutionStatus::Active,
                goal: "收口 prompt memory".to_string(),
                progress: "已经有 summary".to_string(),
                blocker: String::new(),
                next_action: "接 execution state".to_string(),
                last_output: String::new(),
                updated_at: 1,
            })),
        };

        let context = load_prompt_memory_context(PromptMemoryContextParams {
            chat_id: "chat-1",
            user_query: "嗯?",
            system_max_len: 1024,
            profile: MemoryProfile::Standard,
            session_store: &session_store,
            session_summary_store: &summary_store,
            long_term_memory_store: &memory_store,
            execution_state_store: &execution_state_store,
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
        assert!(memory_store
            .last_query
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_deref()
            .unwrap_or_default()
            .contains("重点是咖啡偏好和昵称"));
        assert!(context
            .execution_state_text
            .as_deref()
            .unwrap_or_default()
            .contains("Goal: 收口 prompt memory"));
    }

    #[test]
    fn skips_long_term_recall_when_system_budget_is_below_block_threshold() {
        let session_store = StubSessionStore {
            recent: Mutex::new(vec![SessionMessage {
                role: "user".to_string(),
                content: "记一下我喜欢冷萃".to_string(),
            }]),
        };
        let summary_store = StubSessionSummaryStore {
            summary: Mutex::new(Some(("user prefers cold brew".to_string(), 3))),
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
        let execution_state_store = StubExecutionStateStore::default();

        let context = load_prompt_memory_context(PromptMemoryContextParams {
            chat_id: "chat-1",
            user_query: "嗯?",
            system_max_len: 80,
            profile: MemoryProfile::Embedded,
            session_store: &session_store,
            session_summary_store: &summary_store,
            long_term_memory_store: &memory_store,
            execution_state_store: &execution_state_store,
        });

        assert_eq!(
            context.summary_text.as_deref(),
            Some("user prefers cold brew")
        );
        assert!(context.long_term_memory_text.is_none());
        assert!(memory_store
            .last_query
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_none());
    }
}
