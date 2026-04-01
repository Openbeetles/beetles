//! 对话回复后的共享记忆维护编排。
//! Shared post-reply memory maintenance orchestration.

use crate::bus::IngressKind;
use crate::error::Result;
use crate::llm::{LlmClient, LlmHttpClient};
use crate::orchestrator::PressureLevel;

use super::{
    evaluate_long_term_memory_extraction_turn, mark_long_term_memory_extraction_requested,
    persist_long_term_memory_extraction_state, run_execution_state_refresh,
    run_session_summary_refresh, ExecutionStateRefreshContext, ExecutionStateRefreshInput,
    ExecutionStateRefreshOutcome, ExecutionStateStore, LongTermMemoryExtractionStateStore,
    LongTermMemoryExtractionTurnInput, MemoryProfile, SessionStore, SessionSummaryRefreshOutcome,
    SessionSummaryStore,
};

pub struct PostReplyMemoryMaintenanceContext<'a> {
    pub session_store: &'a dyn SessionStore,
    pub session_summary_store: &'a dyn SessionSummaryStore,
    pub execution_state_store: &'a dyn ExecutionStateStore,
    pub extraction_state_store: &'a dyn LongTermMemoryExtractionStateStore,
}

pub struct PostReplyMemoryMaintenanceInput<'a> {
    pub chat_id: &'a str,
    pub ingress: IngressKind,
    pub channel: &'a str,
    pub user_content: &'a str,
    pub reply_content: &'a str,
    pub pressure: PressureLevel,
    pub memory_profile: MemoryProfile,
    pub tool_calls: u32,
    pub now_secs: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LongTermMemoryRefreshRequestOutcome {
    NotRequested,
    Requested,
    RequestFailed,
}

pub struct PostReplyMemoryMaintenanceOutcome {
    pub after_count: usize,
    pub summary_result: Result<SessionSummaryRefreshOutcome>,
    pub execution_state_result: Result<ExecutionStateRefreshOutcome>,
    pub extraction_request_outcome: LongTermMemoryRefreshRequestOutcome,
}

pub fn run_post_reply_memory_maintenance(
    http: &mut dyn LlmHttpClient,
    llm: &(dyn LlmClient + Send + Sync),
    ctx: PostReplyMemoryMaintenanceContext<'_>,
    input: PostReplyMemoryMaintenanceInput<'_>,
    mut enqueue_long_term_refresh: impl FnMut() -> bool,
) -> PostReplyMemoryMaintenanceOutcome {
    let after_count = ctx.session_store.message_count(input.chat_id).unwrap_or(0);
    let summary_result = run_session_summary_refresh(
        http,
        llm,
        super::SessionSummaryRefreshContext {
            session_store: ctx.session_store,
            session_summary_store: ctx.session_summary_store,
        },
        input.chat_id,
        after_count,
        input.memory_profile,
    );
    let execution_state_result = run_execution_state_refresh(
        http,
        llm,
        ExecutionStateRefreshContext {
            session_store: ctx.session_store,
            session_summary_store: ctx.session_summary_store,
            execution_state_store: ctx.execution_state_store,
        },
        ExecutionStateRefreshInput {
            chat_id: input.chat_id,
            ingress: input.ingress,
            channel: input.channel,
            user_content: input.user_content,
            reply_content: input.reply_content,
            pressure: input.pressure,
            tool_calls: input.tool_calls,
            now_secs: input.now_secs,
        },
        input.memory_profile,
    );

    let extraction_state = ctx.extraction_state_store.get(input.chat_id).ok().flatten();
    let extraction_decision = evaluate_long_term_memory_extraction_turn(
        LongTermMemoryExtractionTurnInput {
            ingress: input.ingress,
            channel: input.channel,
            user_content: input.user_content,
            reply_content: input.reply_content,
            after_count,
            pressure: input.pressure,
        },
        extraction_state.as_ref(),
        input.memory_profile,
    );
    let mut next_extraction_state = extraction_decision.next_state.clone();
    let extraction_request_outcome = if extraction_decision.should_enqueue {
        if enqueue_long_term_refresh() {
            next_extraction_state =
                mark_long_term_memory_extraction_requested(&next_extraction_state, after_count);
            LongTermMemoryRefreshRequestOutcome::Requested
        } else {
            LongTermMemoryRefreshRequestOutcome::RequestFailed
        }
    } else {
        LongTermMemoryRefreshRequestOutcome::NotRequested
    };
    persist_long_term_memory_extraction_state(
        ctx.extraction_state_store,
        input.chat_id,
        extraction_state.as_ref(),
        &next_extraction_state,
    );

    PostReplyMemoryMaintenanceOutcome {
        after_count,
        summary_result,
        execution_state_result,
        extraction_request_outcome,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Result;
    use crate::llm::{LlmModelCompat, LlmResponse, Message, StopReason, ToolChoicePolicy};
    use crate::memory::{
        ExecutionState, ExecutionStateStore, LongTermMemoryExtractionState,
        LongTermMemoryExtractionStateStore, SessionMessage, SessionSummaryStore,
    };
    use std::collections::HashMap;
    use std::sync::Mutex;

    #[derive(Default)]
    struct StubSessionStore {
        recent: Vec<SessionMessage>,
        count: usize,
    }

    impl SessionStore for StubSessionStore {
        fn append(&self, _chat_id: &str, _role: &str, _content: &str) -> Result<()> {
            Ok(())
        }

        fn load_recent(&self, _chat_id: &str, limit: usize) -> Result<Vec<SessionMessage>> {
            Ok(self.recent.iter().take(limit).cloned().collect())
        }

        fn message_count(&self, _chat_id: &str) -> Result<usize> {
            Ok(self.count)
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
        entries: Mutex<HashMap<String, (String, usize)>>,
        fail_get_with_count: bool,
    }

    impl SessionSummaryStore for StubSessionSummaryStore {
        fn get(&self, chat_id: &str) -> Result<Option<String>> {
            Ok(self
                .entries
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(chat_id)
                .map(|(summary, _)| summary.clone()))
        }

        fn set(&self, chat_id: &str, summary: &str) -> Result<()> {
            self.set_with_count(chat_id, summary, 0)
        }

        fn set_with_count(&self, chat_id: &str, summary: &str, message_count: usize) -> Result<()> {
            self.entries
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(chat_id.to_string(), (summary.to_string(), message_count));
            Ok(())
        }

        fn get_with_count(&self, chat_id: &str) -> Result<Option<(String, usize)>> {
            if self.fail_get_with_count {
                return Err(crate::error::Error::config("summary", "broken"));
            }
            Ok(self
                .entries
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(chat_id)
                .cloned())
        }
    }

    #[derive(Default)]
    struct StubExtractionStateStore {
        state: Mutex<Option<LongTermMemoryExtractionState>>,
        clears: Mutex<u32>,
    }

    impl LongTermMemoryExtractionStateStore for StubExtractionStateStore {
        fn get(&self, _chat_id: &str) -> Result<Option<LongTermMemoryExtractionState>> {
            Ok(self.state.lock().unwrap_or_else(|e| e.into_inner()).clone())
        }

        fn set(&self, _chat_id: &str, state: &LongTermMemoryExtractionState) -> Result<()> {
            *self.state.lock().unwrap_or_else(|e| e.into_inner()) = Some(state.clone());
            Ok(())
        }

        fn clear(&self, _chat_id: &str) -> Result<()> {
            *self.state.lock().unwrap_or_else(|e| e.into_inner()) = None;
            *self.clears.lock().unwrap_or_else(|e| e.into_inner()) += 1;
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubExecutionStateStore {
        state: Mutex<Option<ExecutionState>>,
        clears: Mutex<u32>,
    }

    impl ExecutionStateStore for StubExecutionStateStore {
        fn get(&self, _chat_id: &str) -> Result<Option<ExecutionState>> {
            Ok(self.state.lock().unwrap_or_else(|e| e.into_inner()).clone())
        }

        fn set(&self, _chat_id: &str, state: &ExecutionState) -> Result<()> {
            *self.state.lock().unwrap_or_else(|e| e.into_inner()) = Some(state.clone());
            Ok(())
        }

        fn clear(&self, _chat_id: &str) -> Result<()> {
            *self.state.lock().unwrap_or_else(|e| e.into_inner()) = None;
            *self.clears.lock().unwrap_or_else(|e| e.into_inner()) += 1;
            Ok(())
        }
    }

    struct FixedLlmClient;

    impl LlmClient for FixedLlmClient {
        fn model_compat(&self) -> LlmModelCompat {
            LlmModelCompat::default()
        }

        fn chat(
            &self,
            _http: &mut dyn LlmHttpClient,
            system: &str,
            _messages: &[Message],
            _tools: Option<&[crate::llm::ToolSpec]>,
            _tool_choice: ToolChoicePolicy,
        ) -> Result<LlmResponse> {
            let content = if system == crate::memory::EXECUTION_STATE_SYSTEM_PROMPT {
                r#"{"status":"active","goal":"长期记忆链路收口","progress":"继续拆 coordinator","next_action":"接 execution state"}"#
            } else {
                "summary"
            };
            Ok(LlmResponse {
                content: content.to_string(),
                stop_reason: StopReason::EndTurn,
                tool_calls: None,
            })
        }
    }

    #[derive(Default)]
    struct DummyHttpClient;

    impl LlmHttpClient for DummyHttpClient {
        fn do_post(
            &mut self,
            _url: &str,
            _headers: &[(&str, &str)],
            _body: &[u8],
        ) -> Result<(u16, crate::platform::ResponseBody)> {
            Ok((200, crate::platform::ResponseBody::Heap(Vec::new())))
        }
    }

    #[test]
    fn maintenance_continues_extraction_scheduling_when_summary_refresh_errors() {
        let session_store = StubSessionStore {
            recent: vec![
                SessionMessage {
                    role: "user".to_string(),
                    content: "我们在做长期记忆链路收口".to_string(),
                },
                SessionMessage {
                    role: "assistant".to_string(),
                    content: "这轮会继续拆 coordinator".to_string(),
                },
            ],
            count: 10,
        };
        let summary_store = StubSessionSummaryStore {
            fail_get_with_count: true,
            ..Default::default()
        };
        let extraction_state_store = StubExtractionStateStore {
            state: Mutex::new(Some(LongTermMemoryExtractionState {
                dirty_since_count: 4,
                dirty_turns: 1,
                last_requested_at_count: 0,
                last_processed_at_count: 0,
                pending: false,
            })),
            ..Default::default()
        };
        let execution_state_store = StubExecutionStateStore::default();
        let mut http = DummyHttpClient;
        let outcome = run_post_reply_memory_maintenance(
            &mut http,
            &FixedLlmClient,
            PostReplyMemoryMaintenanceContext {
                session_store: &session_store,
                session_summary_store: &summary_store,
                execution_state_store: &execution_state_store,
                extraction_state_store: &extraction_state_store,
            },
            PostReplyMemoryMaintenanceInput {
                chat_id: "chat-1",
                ingress: IngressKind::User,
                channel: "qq_channel",
                user_content: "我们在做长期记忆链路收口",
                reply_content: "这轮会继续拆 coordinator",
                pressure: PressureLevel::Normal,
                memory_profile: MemoryProfile::Embedded,
                tool_calls: 1,
                now_secs: 10,
            },
            || true,
        );

        assert!(matches!(
            outcome.summary_result,
            Ok(SessionSummaryRefreshOutcome::Skipped)
        ));
        assert!(matches!(
            outcome.execution_state_result,
            Ok(ExecutionStateRefreshOutcome::Updated)
        ));
        assert_eq!(
            outcome.extraction_request_outcome,
            LongTermMemoryRefreshRequestOutcome::Requested
        );
        let state = extraction_state_store
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
            .unwrap();
        assert!(state.pending);
        assert_eq!(state.last_requested_at_count, 10);
    }

    #[test]
    fn maintenance_persists_unrequested_extraction_state_without_enqueue() {
        let session_store = StubSessionStore {
            recent: vec![],
            count: 8,
        };
        let summary_store = StubSessionSummaryStore::default();
        let extraction_state_store = StubExtractionStateStore::default();
        let execution_state_store = StubExecutionStateStore::default();
        let mut http = DummyHttpClient;
        let outcome = run_post_reply_memory_maintenance(
            &mut http,
            &FixedLlmClient,
            PostReplyMemoryMaintenanceContext {
                session_store: &session_store,
                session_summary_store: &summary_store,
                execution_state_store: &execution_state_store,
                extraction_state_store: &extraction_state_store,
            },
            PostReplyMemoryMaintenanceInput {
                chat_id: "chat-1",
                ingress: IngressKind::User,
                channel: "qq_channel",
                user_content: "继续",
                reply_content: "好，继续。",
                pressure: PressureLevel::Normal,
                memory_profile: MemoryProfile::Embedded,
                tool_calls: 0,
                now_secs: 20,
            },
            || panic!("enqueue should not be called"),
        );

        assert!(matches!(
            outcome.summary_result,
            Ok(SessionSummaryRefreshOutcome::Skipped)
        ));
        assert!(matches!(
            outcome.execution_state_result,
            Ok(ExecutionStateRefreshOutcome::Skipped)
        ));
        assert_eq!(
            outcome.extraction_request_outcome,
            LongTermMemoryRefreshRequestOutcome::NotRequested
        );
        assert!(extraction_state_store
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_none());
    }
}
