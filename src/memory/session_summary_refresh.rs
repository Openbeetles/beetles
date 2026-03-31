//! 会话摘要刷新策略与执行。
//! Session summary refresh policy and execution.

use crate::constants::SESSION_SUMMARY_MAX_LEN;
use crate::error::Result;
use crate::llm::{LlmClient, LlmHttpClient, Message, ToolChoicePolicy};
use crate::util::truncate_content_to_max;
use std::borrow::Cow;
use std::fmt::Write as _;

use super::{
    memory_policy, MemoryProfile, SessionMessage, SessionStore, SessionSummaryPolicy,
    SessionSummaryStore,
};

const SESSION_SUMMARY_SYSTEM_PROMPT: &str = "You are a conversation summarizer. Compress the following conversation into a concise summary (max 800 chars) preserving key facts, user preferences and pending tasks. Reply with the summary only.";

impl SessionSummaryPolicy {
    fn should_refresh(self, current_count: usize, last_summary_count: usize) -> bool {
        current_count >= self.refresh_min_messages
            && current_count.saturating_sub(last_summary_count) >= self.refresh_delta_messages
    }
}

pub struct SessionSummaryRefreshContext<'a> {
    pub session_store: &'a dyn SessionStore,
    pub session_summary_store: &'a dyn SessionSummaryStore,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SessionSummaryRefreshOutcome {
    Skipped,
    Updated { used_fallback: bool },
}

pub fn should_refresh_session_summary(
    current_count: usize,
    last_summary_count: usize,
    profile: MemoryProfile,
) -> bool {
    memory_policy(profile)
        .session_summary
        .should_refresh(current_count, last_summary_count)
}

pub fn fallback_session_summary(recent: &[SessionMessage], profile: MemoryProfile) -> String {
    let policy = memory_policy(profile).session_summary;
    let start = recent
        .len()
        .saturating_sub(policy.fallback_recent_message_count);
    let mut fallback = String::with_capacity(640);
    for (idx, message) in recent[start..].iter().enumerate() {
        if idx > 0 {
            fallback.push_str(" | ");
        }
        let _ = write!(
            fallback,
            "{}: {}",
            message.role,
            truncate_content_to_max(&message.content, policy.fallback_preview_chars).as_ref()
        );
    }
    truncate_content_to_max(&fallback, SESSION_SUMMARY_MAX_LEN).into_owned()
}

pub fn run_session_summary_refresh(
    http: &mut dyn LlmHttpClient,
    llm: &(dyn LlmClient + Send + Sync),
    ctx: SessionSummaryRefreshContext<'_>,
    chat_id: &str,
    current_count: usize,
    profile: MemoryProfile,
) -> Result<SessionSummaryRefreshOutcome> {
    let policy = memory_policy(profile).session_summary;
    let last_summary_count = match ctx.session_summary_store.get_with_count(chat_id) {
        Ok(entry) => entry.map(|(_, count)| count).unwrap_or(0),
        Err(error) => {
            log::warn!(
                "[agent_summary] failed to read summary metadata for chat_id={}: {}",
                chat_id,
                error
            );
            0
        }
    };
    if !should_refresh_session_summary(current_count, last_summary_count, profile) {
        return Ok(SessionSummaryRefreshOutcome::Skipped);
    }

    let recent = ctx
        .session_store
        .load_recent(chat_id, policy.recent_message_count)?;
    let fallback = fallback_session_summary(&recent, profile);
    let transcript = build_session_summary_transcript(&recent, policy);
    let messages = [Message {
        role: Cow::Borrowed("user"),
        content: transcript,
    }];

    let (summary, used_fallback) = match llm.chat(
        http,
        SESSION_SUMMARY_SYSTEM_PROMPT,
        &messages,
        None,
        ToolChoicePolicy::Auto,
    ) {
        Ok(response) => {
            let summary = truncate_content_to_max(response.content.trim(), SESSION_SUMMARY_MAX_LEN)
                .into_owned();
            if summary.is_empty() {
                (fallback, true)
            } else {
                (summary, false)
            }
        }
        Err(error) => {
            log::warn!(
                "[agent_summary] LLM summary failed for chat_id={}: {}",
                chat_id,
                error
            );
            (fallback, true)
        }
    };

    ctx.session_summary_store
        .set_with_count(chat_id, &summary, current_count)?;
    Ok(SessionSummaryRefreshOutcome::Updated { used_fallback })
}

fn build_session_summary_transcript(
    recent: &[SessionMessage],
    policy: SessionSummaryPolicy,
) -> String {
    let mut transcript = String::with_capacity(2048);
    for message in recent {
        let preview = truncate_content_to_max(&message.content, policy.transcript_preview_chars);
        let _ = writeln!(
            transcript,
            "{}: {}",
            message.role.to_uppercase(),
            preview.as_ref()
        );
    }
    transcript
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Result;
    use crate::llm::{LlmModelCompat, LlmResponse, StopReason};
    use std::collections::HashMap;
    use std::sync::Mutex;

    #[derive(Default)]
    struct StubSessionStore {
        recent: Vec<SessionMessage>,
    }

    impl SessionStore for StubSessionStore {
        fn append(&self, _chat_id: &str, _role: &str, _content: &str) -> Result<()> {
            Ok(())
        }

        fn load_recent(&self, _chat_id: &str, limit: usize) -> Result<Vec<SessionMessage>> {
            Ok(self.recent.iter().take(limit).cloned().collect())
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
            Ok(self
                .entries
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(chat_id)
                .cloned())
        }
    }

    struct FixedLlmClient {
        response: Option<LlmResponse>,
        fail_message: Option<&'static str>,
    }

    impl LlmClient for FixedLlmClient {
        fn model_compat(&self) -> LlmModelCompat {
            LlmModelCompat::default()
        }

        fn chat(
            &self,
            _http: &mut dyn LlmHttpClient,
            _system: &str,
            _messages: &[Message],
            _tools: Option<&[crate::llm::ToolSpec]>,
            _tool_choice: ToolChoicePolicy,
        ) -> Result<LlmResponse> {
            if let Some(response) = &self.response {
                Ok(response.clone())
            } else {
                Err(crate::error::Error::config(
                    "llm",
                    self.fail_message.unwrap_or("boom"),
                ))
            }
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
    fn session_summary_refresh_threshold_is_programmatic() {
        assert!(!should_refresh_session_summary(
            19,
            0,
            MemoryProfile::Embedded
        ));
        assert!(!should_refresh_session_summary(
            20,
            15,
            MemoryProfile::Embedded
        ));
        assert!(should_refresh_session_summary(
            20,
            10,
            MemoryProfile::Embedded
        ));
        assert!(should_refresh_session_summary(
            35,
            20,
            MemoryProfile::Embedded
        ));
        assert!(should_refresh_session_summary(
            16,
            8,
            MemoryProfile::Standard
        ));
    }

    #[test]
    fn fallback_session_summary_keeps_recent_messages_in_order() {
        let recent = vec![
            SessionMessage {
                role: "user".to_string(),
                content: "one".to_string(),
            },
            SessionMessage {
                role: "assistant".to_string(),
                content: "two".to_string(),
            },
            SessionMessage {
                role: "user".to_string(),
                content: "three".to_string(),
            },
        ];
        let summary = fallback_session_summary(&recent, MemoryProfile::Standard);
        assert!(summary.contains("user: one"));
        assert!(summary.contains("assistant: two"));
        assert!(summary.contains("user: three"));
    }

    #[test]
    fn refresh_runner_skips_when_threshold_not_met() {
        let session_store = StubSessionStore::default();
        let summary_store = StubSessionSummaryStore::default();
        let mut http = DummyHttpClient;
        let llm = FixedLlmClient {
            response: Some(LlmResponse {
                content: "summary".to_string(),
                stop_reason: StopReason::EndTurn,
                tool_calls: None,
            }),
            fail_message: None,
        };

        let outcome = run_session_summary_refresh(
            &mut http,
            &llm,
            SessionSummaryRefreshContext {
                session_store: &session_store,
                session_summary_store: &summary_store,
            },
            "chat-1",
            12,
            MemoryProfile::Embedded,
        )
        .unwrap();

        assert_eq!(outcome, SessionSummaryRefreshOutcome::Skipped);
    }

    #[test]
    fn refresh_runner_persists_model_summary_with_count() {
        let session_store = StubSessionStore {
            recent: vec![
                SessionMessage {
                    role: "user".to_string(),
                    content: "hello".to_string(),
                },
                SessionMessage {
                    role: "assistant".to_string(),
                    content: "world".to_string(),
                },
            ],
        };
        let summary_store = StubSessionSummaryStore::default();
        let mut http = DummyHttpClient;
        let llm = FixedLlmClient {
            response: Some(LlmResponse {
                content: "fresh summary".to_string(),
                stop_reason: StopReason::EndTurn,
                tool_calls: None,
            }),
            fail_message: None,
        };

        let outcome = run_session_summary_refresh(
            &mut http,
            &llm,
            SessionSummaryRefreshContext {
                session_store: &session_store,
                session_summary_store: &summary_store,
            },
            "chat-1",
            20,
            MemoryProfile::Embedded,
        )
        .unwrap();

        assert_eq!(
            outcome,
            SessionSummaryRefreshOutcome::Updated {
                used_fallback: false
            }
        );
        assert_eq!(
            summary_store.get_with_count("chat-1").unwrap(),
            Some(("fresh summary".to_string(), 20))
        );
    }

    #[test]
    fn refresh_runner_falls_back_when_llm_fails() {
        let session_store = StubSessionStore {
            recent: vec![
                SessionMessage {
                    role: "user".to_string(),
                    content: "最近在做 memory maintenance 收口".to_string(),
                },
                SessionMessage {
                    role: "assistant".to_string(),
                    content: "这轮会把 session summary 从 loop 拆走".to_string(),
                },
            ],
        };
        let summary_store = StubSessionSummaryStore::default();
        let mut http = DummyHttpClient;
        let llm = FixedLlmClient {
            response: None,
            fail_message: Some("boom"),
        };

        let outcome = run_session_summary_refresh(
            &mut http,
            &llm,
            SessionSummaryRefreshContext {
                session_store: &session_store,
                session_summary_store: &summary_store,
            },
            "chat-1",
            20,
            MemoryProfile::Embedded,
        )
        .unwrap();

        assert_eq!(
            outcome,
            SessionSummaryRefreshOutcome::Updated {
                used_fallback: true
            }
        );
        let stored = summary_store.get("chat-1").unwrap().unwrap();
        assert!(stored.contains("user: 最近在做 memory maintenance 收口"));
        assert!(stored.contains("assistant: 这轮会把 session summary 从 loop 拆走"));
    }
}
