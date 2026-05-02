//! 会话消息窗口构建与截断。
//! Conversation message window assembly and truncation.

use crate::bus::PcMsg;
use crate::llm::Message;
use std::borrow::Cow;

use super::{ImportantMessageStore, SessionStore};

fn build_context_summary_message(summary: &str) -> String {
    let trimmed = summary.trim();
    let mut out = String::with_capacity(trimmed.len().saturating_add(32));
    out.push_str("[CONTEXT_SUMMARY]\n");
    out.push_str(trimmed);
    out.push_str("\n[/CONTEXT_SUMMARY]");
    out
}

fn push_context_message(
    messages: &mut Vec<Message>,
    role: Cow<'static, str>,
    content: String,
) -> usize {
    let last_index = messages.len().saturating_sub(1);
    if let Some(last) = messages.last_mut() {
        if last.role.as_ref() == role.as_ref() && !last.content.starts_with("[CONTEXT_SUMMARY]") {
            last.content.push('\n');
            last.content.push_str(&content);
            return last_index;
        }
    }
    messages.push(Message { role, content });
    messages.len().saturating_sub(1)
}

pub fn build_context_messages(
    session: &dyn SessionStore,
    important_message_store: &dyn ImportantMessageStore,
    msg: &PcMsg,
    recent_messages_limit: usize,
    messages_max_len: usize,
    summary_text: Option<&str>,
    recent_override: Option<&[super::SessionMessage]>,
) -> Vec<Message> {
    let n = recent_messages_limit.clamp(1, 128);
    let owned_recent;
    let recent = if let Some(recent_override) = recent_override {
        recent_override
    } else {
        owned_recent = session
            .load_recent(&msg.chat_id, n)
            .unwrap_or_else(|_| vec![]);
        owned_recent.as_slice()
    };
    let cap = recent.len() + if summary_text.is_some() { 2 } else { 1 };
    let mut messages: Vec<Message> = Vec::with_capacity(cap);
    if let Some(summary) = summary_text {
        messages.push(Message {
            role: Cow::Borrowed("user"),
            content: build_context_summary_message(summary),
        });
    }
    let mut session_message_indices = Vec::with_capacity(recent.len());
    for m in recent.iter().cloned() {
        let index = push_context_message(&mut messages, Cow::Owned(m.role), m.content);
        session_message_indices.push(index);
    }
    push_context_message(&mut messages, Cow::Borrowed("user"), msg.content.clone());

    let important_offset = important_message_store
        .get_important_offset(&msg.chat_id)
        .ok()
        .flatten();
    let protected_idx = important_offset.and_then(|offset| {
        let offset = offset as usize;
        if offset >= session_message_indices.len() {
            return None;
        }
        session_message_indices
            .get(session_message_indices.len().saturating_sub(1 + offset))
            .copied()
    });
    truncate_messages_to_len(
        &mut messages,
        messages_max_len,
        protected_idx,
        summary_text.is_some(),
    );
    messages
}

fn truncate_messages_to_len(
    messages: &mut Vec<Message>,
    max_len: usize,
    protected_idx: Option<usize>,
    preserve_summary: bool,
) {
    let mut total = 0usize;
    for message in messages.iter() {
        total = total
            .saturating_add(message.role.len())
            .saturating_add(message.content.len())
            .saturating_add(2);
    }
    let summary_idx = preserve_summary.then_some(0usize);
    let mut indices_to_remove = Vec::new();
    for (index, message) in messages.iter().enumerate() {
        if total <= max_len {
            break;
        }
        if Some(index) == protected_idx || Some(index) == summary_idx {
            continue;
        }
        if messages.len() - indices_to_remove.len() <= 1 {
            break;
        }
        let size = message
            .role
            .len()
            .saturating_add(message.content.len())
            .saturating_add(2);
        total = total.saturating_sub(size);
        indices_to_remove.push(index);
    }
    if total > max_len && summary_idx.is_some() {
        let len_after_first_pass = messages.len().saturating_sub(indices_to_remove.len());
        if len_after_first_pass > 1 && Some(0usize) != protected_idx {
            indices_to_remove.push(0);
        }
    }
    indices_to_remove.sort_unstable();
    let remove_indices = indices_to_remove;
    let drained = std::mem::take(messages);
    let mut kept = Vec::with_capacity(drained.len().saturating_sub(remove_indices.len()));
    let mut remove_cursor = 0usize;
    for (index, message) in drained.into_iter().enumerate() {
        let should_remove =
            remove_cursor < remove_indices.len() && remove_indices[remove_cursor] == index;
        if should_remove {
            remove_cursor += 1;
        } else {
            kept.push(message);
        }
    }
    *messages = kept;
    merge_consecutive_same_role(messages);
}

fn merge_consecutive_same_role(messages: &mut Vec<Message>) {
    let mut index = 0;
    while index + 1 < messages.len() {
        let has_summary_marker = messages[index].content.starts_with("[CONTEXT_SUMMARY]")
            || messages[index + 1].content.starts_with("[CONTEXT_SUMMARY]");
        if messages[index].role == messages[index + 1].role && !has_summary_marker {
            let next_content = messages.remove(index + 1).content;
            messages[index].content.push('\n');
            messages[index].content.push_str(&next_content);
        } else {
            index += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Result;
    use std::sync::Mutex;

    #[test]
    fn summary_is_preserved_before_dropping_recent_history() {
        let mut messages = vec![
            Message {
                role: Cow::Borrowed("user"),
                content: "[CONTEXT_SUMMARY]\nsummary\n[/CONTEXT_SUMMARY]".to_string(),
            },
            Message {
                role: Cow::Borrowed("assistant"),
                content: "old assistant reply".to_string(),
            },
            Message {
                role: Cow::Borrowed("user"),
                content: "latest user message".to_string(),
            },
        ];
        let max_len = messages[0].role.len()
            + messages[0].content.len()
            + messages[2].role.len()
            + messages[2].content.len()
            + 4;
        truncate_messages_to_len(&mut messages, max_len, None, true);
        assert_eq!(messages.len(), 2);
        assert!(messages[0].content.contains("[CONTEXT_SUMMARY]"));
        assert_eq!(messages[1].content, "latest user message");
    }

    #[derive(Default)]
    struct EmptySessionStore;

    impl SessionStore for EmptySessionStore {
        fn append(&self, _chat_id: &str, _role: &str, _content: &str) -> Result<()> {
            Ok(())
        }

        fn load_recent(
            &self,
            _chat_id: &str,
            _n: usize,
        ) -> Result<Vec<crate::memory::SessionMessage>> {
            Ok(Vec::new())
        }

        fn clear(&self, _chat_id: &str) -> Result<()> {
            Ok(())
        }

        fn list_chat_ids(&self) -> Result<Vec<String>> {
            Ok(Vec::new())
        }
    }

    struct RecentSessionStore {
        recent: Vec<crate::memory::SessionMessage>,
    }

    impl SessionStore for RecentSessionStore {
        fn append(&self, _chat_id: &str, _role: &str, _content: &str) -> Result<()> {
            Ok(())
        }

        fn load_recent(
            &self,
            _chat_id: &str,
            _n: usize,
        ) -> Result<Vec<crate::memory::SessionMessage>> {
            Ok(self.recent.clone())
        }

        fn clear(&self, _chat_id: &str) -> Result<()> {
            Ok(())
        }

        fn list_chat_ids(&self) -> Result<Vec<String>> {
            Ok(Vec::new())
        }
    }

    #[derive(Default)]
    struct CountingImportantStore {
        clear_calls: Mutex<u32>,
    }

    impl ImportantMessageStore for CountingImportantStore {
        fn set_important_offset_from_end(
            &self,
            _chat_id: &str,
            _offset_from_end: u32,
        ) -> Result<()> {
            Ok(())
        }

        fn get_important_offset(&self, _chat_id: &str) -> Result<Option<u32>> {
            Ok(Some(1))
        }

        fn clear_important(&self, _chat_id: &str) -> Result<()> {
            let mut calls = self.clear_calls.lock().unwrap_or_else(|e| e.into_inner());
            *calls += 1;
            Ok(())
        }
    }

    #[test]
    fn build_context_messages_does_not_mutate_important_marker() {
        let session = EmptySessionStore;
        let important = CountingImportantStore::default();
        let msg = PcMsg::new("telegram", "chat", "hello").expect("message");

        let messages = build_context_messages(&session, &important, &msg, 3, 128, None, None);

        assert_eq!(messages.len(), 1);
        assert_eq!(
            *important
                .clear_calls
                .lock()
                .unwrap_or_else(|e| e.into_inner()),
            0
        );
    }

    #[test]
    fn build_context_messages_preserves_marked_session_user_not_previous_assistant() {
        let session = RecentSessionStore {
            recent: vec![
                crate::memory::SessionMessage {
                    role: "user".to_string(),
                    content: "KEEP_ME important user marker".to_string(),
                },
                crate::memory::SessionMessage {
                    role: "assistant".to_string(),
                    content: "assistant filler ".repeat(32),
                },
            ],
        };
        let important = CountingImportantStore::default();
        let msg = PcMsg::new("telegram", "chat", "new question").expect("message");

        let messages = build_context_messages(&session, &important, &msg, 3, 80, None, None);
        let joined = messages
            .iter()
            .map(|message| message.content.as_str())
            .collect::<Vec<_>>()
            .join("\n");

        assert!(
            joined.contains("KEEP_ME important user marker"),
            "marked persisted user message must be protected against transient current-message offset drift; got {joined:?}",
        );
    }
}
