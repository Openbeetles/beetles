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

fn push_context_message(messages: &mut Vec<Message>, role: Cow<'static, str>, content: String) {
    if let Some(last) = messages.last_mut() {
        if last.role.as_ref() == role.as_ref() && !last.content.starts_with("[CONTEXT_SUMMARY]") {
            last.content.push('\n');
            last.content.push_str(&content);
            return;
        }
    }
    messages.push(Message { role, content });
}

pub fn build_context_messages(
    session: &dyn SessionStore,
    important_message_store: &dyn ImportantMessageStore,
    msg: &PcMsg,
    session_max_messages: usize,
    messages_max_len: usize,
    summary_text: Option<&str>,
) -> Vec<Message> {
    let n = session_max_messages.clamp(1, 128);
    let recent = session
        .load_recent(&msg.chat_id, n)
        .unwrap_or_else(|_| vec![]);
    let cap = recent.len() + if summary_text.is_some() { 2 } else { 1 };
    let mut messages: Vec<Message> = Vec::with_capacity(cap);
    if let Some(summary) = summary_text {
        messages.push(Message {
            role: Cow::Borrowed("user"),
            content: build_context_summary_message(summary),
        });
    }
    for m in recent {
        push_context_message(&mut messages, Cow::Owned(m.role), m.content);
    }
    push_context_message(&mut messages, Cow::Borrowed("user"), msg.content.clone());

    let important_offset = important_message_store
        .get_important_offset(&msg.chat_id)
        .ok()
        .flatten();
    truncate_messages_to_len(
        &mut messages,
        messages_max_len,
        important_offset,
        summary_text.is_some(),
    );
    if important_offset.is_some() {
        let _ = important_message_store.clear_important(&msg.chat_id);
    }
    messages
}

fn truncate_messages_to_len(
    messages: &mut Vec<Message>,
    max_len: usize,
    protected_offset_from_end: Option<u32>,
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
    let protected_idx = protected_offset_from_end.and_then(|offset| {
        let len = messages.len();
        let idx = len.saturating_sub(1).saturating_sub(offset as usize);
        if idx < len {
            Some(idx)
        } else {
            None
        }
    });
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
}
