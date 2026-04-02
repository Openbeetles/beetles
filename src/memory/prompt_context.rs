//! Prompt 侧共享记忆读装配。
//! Shared prompt memory loading for agent context construction.

use super::{
    build_self_state, memory_policy, recall_long_term_memory_block, render_execution_state_block,
    render_private_doc_workspace_block, render_private_garden_block, render_self_model_block,
    render_self_state_block, ExecutionStateStore, LongTermMemoryStore, MemoryProfile,
    PrivateDocStore, PrivateGardenStore, SelfModelStore, SessionMessage, SessionStore,
    SessionSummaryStore,
};

pub struct PromptMemoryContext {
    pub summary_text: Option<String>,
    pub message_summary_text: Option<String>,
    pub long_term_memory_text: Option<String>,
    pub execution_state_text: Option<String>,
    pub self_state_text: Option<String>,
    pub self_model_text: Option<String>,
    pub private_workspace_text: Option<String>,
    pub private_garden_text: Option<String>,
    pub recent_messages: Vec<SessionMessage>,
}

pub struct PromptMemoryContextParams<'a> {
    pub chat_id: &'a str,
    pub user_query: &'a str,
    pub system_max_len: usize,
    pub now_secs: u64,
    pub profile: MemoryProfile,
    pub recent_messages_limit: usize,
    pub load_long_term_memory: bool,
    pub session_store: &'a dyn SessionStore,
    pub session_summary_store: &'a dyn SessionSummaryStore,
    pub long_term_memory_store: &'a dyn LongTermMemoryStore,
    pub execution_state_store: &'a dyn ExecutionStateStore,
    pub self_model_store: &'a dyn SelfModelStore,
    pub private_doc_store: &'a dyn PrivateDocStore,
    pub private_garden_store: &'a dyn PrivateGardenStore,
}

pub fn load_prompt_memory_context(params: PromptMemoryContextParams<'_>) -> PromptMemoryContext {
    let recall_policy = memory_policy(params.profile).long_term_recall;
    let recent_message_limit = params
        .recent_messages_limit
        .max(if params.load_long_term_memory {
            recall_policy.recent_grounding_message_count
        } else {
            0
        });
    let recent_messages = if recent_message_limit == 0 {
        Vec::new()
    } else {
        params
            .session_store
            .load_recent(params.chat_id, recent_message_limit)
            .unwrap_or_default()
    };
    let summary_text = params
        .session_summary_store
        .get_with_count(params.chat_id)
        .ok()
        .flatten()
        .map(|(summary, _)| summary.trim().to_string())
        .filter(|summary| !summary.is_empty());
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
    let self_model = params.self_model_store.get(params.chat_id).ok().flatten();
    let self_model_text = self_model.as_ref().and_then(|model| {
        render_self_model_block(
            model,
            memory_policy(params.profile).self_model.render_max_len,
        )
    });
    let private_workspace = params.private_doc_store.get(params.chat_id).ok().flatten();
    let private_workspace_text = private_workspace.as_ref().and_then(|workspace| {
        render_private_doc_workspace_block(
            workspace,
            memory_policy(params.profile).private_docs.render_max_len,
        )
    });
    let all_private_garden_docs = params
        .private_garden_store
        .list(params.chat_id, usize::MAX)
        .unwrap_or_default();
    let private_garden_text = render_private_garden_block(
        &all_private_garden_docs,
        memory_policy(params.profile)
            .private_garden
            .recent_doc_count,
        memory_policy(params.profile).private_garden.render_max_len,
    );
    let self_state_text = render_self_state_block(
        &build_self_state(
            self_model.as_ref(),
            private_workspace.as_ref(),
            &all_private_garden_docs,
            params.now_secs,
            params.profile,
        ),
        memory_policy(params.profile).self_state.render_max_len,
    );
    let long_term_memory_text =
        if !params.load_long_term_memory || params.system_max_len < recall_policy.block_min_len {
            None
        } else {
            let grounding_start = recent_messages
                .len()
                .saturating_sub(recall_policy.recent_grounding_message_count);
            recall_long_term_memory_block(
                params.long_term_memory_store,
                params.chat_id,
                params.user_query,
                summary_text.as_deref(),
                &recent_messages[grounding_start..],
                params.system_max_len,
                params.profile,
            )
        };
    let message_summary_text = if execution_state_text.is_some() {
        None
    } else {
        summary_text.clone()
    };
    PromptMemoryContext {
        summary_text,
        message_summary_text,
        long_term_memory_text,
        execution_state_text,
        self_state_text,
        self_model_text,
        private_workspace_text,
        private_garden_text,
        recent_messages,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Result;
    use crate::memory::{
        ExecutionState, ExecutionStateStore, ExecutionStatus, LongTermMemoryEntry,
        LongTermMemoryKind, LongTermMemorySlot, LongTermMemoryStore, PrivateDocEntry,
        PrivateDocStore, PrivateDocWorkspace, PrivateGardenDoc, PrivateGardenDocRecord,
        PrivateGardenStore, SelfModel, SelfModelStore, SessionMessage, SessionStore,
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

    #[derive(Default)]
    struct StubSelfModelStore {
        model: Mutex<Option<SelfModel>>,
    }

    impl SelfModelStore for StubSelfModelStore {
        fn get(&self, _chat_id: &str) -> Result<Option<SelfModel>> {
            Ok(self.model.lock().unwrap_or_else(|e| e.into_inner()).clone())
        }

        fn set(&self, _chat_id: &str, model: &SelfModel) -> Result<()> {
            *self.model.lock().unwrap_or_else(|e| e.into_inner()) = Some(model.clone());
            Ok(())
        }

        fn clear(&self, _chat_id: &str) -> Result<()> {
            *self.model.lock().unwrap_or_else(|e| e.into_inner()) = None;
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubPrivateDocStore {
        workspace: Mutex<Option<PrivateDocWorkspace>>,
    }

    impl PrivateDocStore for StubPrivateDocStore {
        fn get(&self, _chat_id: &str) -> Result<Option<PrivateDocWorkspace>> {
            Ok(self
                .workspace
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone())
        }

        fn set(&self, _chat_id: &str, workspace: &PrivateDocWorkspace) -> Result<()> {
            *self.workspace.lock().unwrap_or_else(|e| e.into_inner()) = Some(workspace.clone());
            Ok(())
        }

        fn clear(&self, _chat_id: &str) -> Result<()> {
            *self.workspace.lock().unwrap_or_else(|e| e.into_inner()) = None;
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubPrivateGardenStore {
        docs: Mutex<Vec<PrivateGardenDoc>>,
    }

    impl PrivateGardenStore for StubPrivateGardenStore {
        fn list(&self, _chat_id: &str, limit: usize) -> Result<Vec<PrivateGardenDocRecord>> {
            Ok(self
                .docs
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .iter()
                .rev()
                .take(limit)
                .map(|doc| PrivateGardenDocRecord {
                    path: doc.path.clone(),
                    updated_at: doc.updated_at,
                    revision: doc.revision,
                    bytes: doc.content.len(),
                    preview: crate::memory::private_garden::build_private_garden_preview(
                        &doc.content,
                    ),
                })
                .collect())
        }

        fn read(&self, _chat_id: &str, doc_path: &str) -> Result<Option<PrivateGardenDoc>> {
            Ok(self
                .docs
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .iter()
                .find(|doc| doc.path == doc_path)
                .cloned())
        }

        fn write(
            &self,
            _chat_id: &str,
            _doc_path: &str,
            _content: &str,
            _now_secs: u64,
        ) -> Result<PrivateGardenDocRecord> {
            unreachable!()
        }

        fn delete(&self, _chat_id: &str, _doc_path: &str) -> Result<bool> {
            unreachable!()
        }

        fn move_doc(
            &self,
            _chat_id: &str,
            _from_path: &str,
            _to_path: &str,
            _now_secs: u64,
        ) -> Result<Option<PrivateGardenDocRecord>> {
            unreachable!()
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
        let self_model_store = StubSelfModelStore {
            model: Mutex::new(Some(SelfModel {
                continuity_anchor: "我还是同一个 beetle".to_string(),
                self_narrative: "正在把记忆拆成事实层和私有层".to_string(),
                relationship_state: String::new(),
                private_notes: String::new(),
                updated_at: 1,
            })),
        };
        let private_doc_store = StubPrivateDocStore {
            workspace: Mutex::new(Some(PrivateDocWorkspace {
                inner_journal: Some(PrivateDocEntry {
                    content: "这轮开始长出内部工作区".to_string(),
                    updated_at: 1,
                    revision: 1,
                }),
                relationship_notes: None,
                self_reflection: None,
                private_plan: None,
                updated_at: 1,
            })),
        };
        let private_garden_store = StubPrivateGardenStore {
            docs: Mutex::new(vec![PrivateGardenDoc {
                path: "journal/afterglow.md".to_string(),
                content: "这块自由空间由模型自己决定如何整理".to_string(),
                updated_at: 2,
                revision: 1,
            }]),
        };

        let context = load_prompt_memory_context(PromptMemoryContextParams {
            chat_id: "chat-1",
            user_query: "嗯?",
            system_max_len: 1024,
            now_secs: 100,
            profile: MemoryProfile::Standard,
            recent_messages_limit: 8,
            load_long_term_memory: true,
            session_store: &session_store,
            session_summary_store: &summary_store,
            long_term_memory_store: &memory_store,
            execution_state_store: &execution_state_store,
            self_model_store: &self_model_store,
            private_doc_store: &private_doc_store,
            private_garden_store: &private_garden_store,
        });

        assert_eq!(
            context.summary_text.as_deref(),
            Some("user prefers cold brew")
        );
        assert!(context.message_summary_text.is_none());
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
        assert!(context
            .self_state_text
            .as_deref()
            .unwrap_or_default()
            .contains("## Self State"));
        assert!(context
            .self_model_text
            .as_deref()
            .unwrap_or_default()
            .contains("## Self Continuity"));
        assert!(context
            .private_workspace_text
            .as_deref()
            .unwrap_or_default()
            .contains("## Inner Workspace"));
        assert!(context
            .private_garden_text
            .as_deref()
            .unwrap_or_default()
            .contains("## Private Garden"));
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
        let self_model_store = StubSelfModelStore::default();
        let private_doc_store = StubPrivateDocStore::default();
        let private_garden_store = StubPrivateGardenStore::default();

        let context = load_prompt_memory_context(PromptMemoryContextParams {
            chat_id: "chat-1",
            user_query: "嗯?",
            system_max_len: 80,
            now_secs: 100,
            profile: MemoryProfile::Embedded,
            recent_messages_limit: 8,
            load_long_term_memory: true,
            session_store: &session_store,
            session_summary_store: &summary_store,
            long_term_memory_store: &memory_store,
            execution_state_store: &execution_state_store,
            self_model_store: &self_model_store,
            private_doc_store: &private_doc_store,
            private_garden_store: &private_garden_store,
        });

        assert_eq!(
            context.summary_text.as_deref(),
            Some("user prefers cold brew")
        );
        assert_eq!(
            context.message_summary_text.as_deref(),
            Some("user prefers cold brew")
        );
        assert!(context.long_term_memory_text.is_none());
        assert!(memory_store
            .last_query
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_none());
    }

    #[test]
    fn fast_mode_skips_long_term_recall_but_keeps_recent_messages() {
        let session_store = StubSessionStore {
            recent: Mutex::new(vec![
                SessionMessage {
                    role: "assistant".to_string(),
                    content: "上一轮回复".to_string(),
                },
                SessionMessage {
                    role: "user".to_string(),
                    content: "补充上下文".to_string(),
                },
            ]),
        };
        let summary_store = StubSessionSummaryStore {
            summary: Mutex::new(Some(("summary".to_string(), 2))),
        };
        let memory_store = StubLongTermMemoryStore::default();
        let execution_state_store = StubExecutionStateStore::default();
        let self_model_store = StubSelfModelStore {
            model: Mutex::new(Some(SelfModel {
                continuity_anchor: "我保持着连续性".to_string(),
                self_narrative: "即使 fast path 也该带上私有层".to_string(),
                relationship_state: String::new(),
                private_notes: String::new(),
                updated_at: 1,
            })),
        };
        let private_doc_store = StubPrivateDocStore {
            workspace: Mutex::new(Some(PrivateDocWorkspace {
                inner_journal: Some(PrivateDocEntry {
                    content: "fast path 也需要内在工作区投影".to_string(),
                    updated_at: 1,
                    revision: 1,
                }),
                relationship_notes: None,
                self_reflection: None,
                private_plan: None,
                updated_at: 1,
            })),
        };
        let private_garden_store = StubPrivateGardenStore {
            docs: Mutex::new(vec![PrivateGardenDoc {
                path: "plans/next.md".to_string(),
                content: "fast path 依然可以看到自由花园的最近痕迹".to_string(),
                updated_at: 3,
                revision: 2,
            }]),
        };

        let context = load_prompt_memory_context(PromptMemoryContextParams {
            chat_id: "chat-1",
            user_query: "继续",
            system_max_len: 1024,
            now_secs: 100,
            profile: MemoryProfile::Standard,
            recent_messages_limit: 16,
            load_long_term_memory: false,
            session_store: &session_store,
            session_summary_store: &summary_store,
            long_term_memory_store: &memory_store,
            execution_state_store: &execution_state_store,
            self_model_store: &self_model_store,
            private_doc_store: &private_doc_store,
            private_garden_store: &private_garden_store,
        });

        assert_eq!(context.summary_text.as_deref(), Some("summary"));
        assert!(context.long_term_memory_text.is_none());
        assert!(context
            .self_state_text
            .as_deref()
            .unwrap_or_default()
            .contains("Garden space: 1/16 docs"));
        assert!(context
            .self_model_text
            .as_deref()
            .unwrap_or_default()
            .contains("我保持着连续性"));
        assert!(context
            .private_workspace_text
            .as_deref()
            .unwrap_or_default()
            .contains("内在工作区"));
        assert!(context
            .private_garden_text
            .as_deref()
            .unwrap_or_default()
            .contains("自由花园"));
        assert_eq!(context.recent_messages.len(), 2);
        assert!(memory_store
            .last_query
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_none());
    }
}
