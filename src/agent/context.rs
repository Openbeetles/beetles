//! Agent 上下文构建：从 MemoryStore + SessionStore 聚合 system 与 messages。
//! Pure logic; no platform dependency; for use by agent::loop.

use crate::bus::PcMsg;
use crate::error::Result;
use crate::llm::Message;
use crate::memory::{build_system_prompt, ImportantMessageStore, MemoryStore, SessionStore};
use crate::state;
use std::borrow::Cow;
use std::fmt::Write as _;

pub use crate::constants::{DEFAULT_MESSAGES_MAX_LEN, DEFAULT_SYSTEM_MAX_LEN};
/// 从 SessionStore 加载的最近条数。
pub const SESSION_RECENT_N: usize = 32;
/// 每日笔记取最近条数。
const DAILY_RECENT_N: usize = 5;

#[derive(Clone, Copy)]
pub struct RuntimeContext {
    pub now_secs: u64,
    pub platform: &'static str,
    pub pressure: crate::orchestrator::PressureLevel,
    pub active_agent_tasks: u32,
    pub inbound_depth: u32,
    pub outbound_depth: u32,
    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
    pub cpu_usage_percent: f32,
    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
    pub load_average: (f32, f32, f32),
    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
    pub process_memory_kb: u32,
}

/// 预构建的 structured output 指令块（三个 marker 均为编译期常量）。
const STRUCTURED_BLOCK: &str = concat!(
    "\n\n## Structured output\n",
    "When the user clearly asks to stop or cancel the current task, reply with ",
    "[STOP]",
    " then a short confirmation. ",
    "When you want to mark the current user message as important for context truncation, include ",
    "[MARK_IMPORTANT]",
    " in your reply. ",
    "When you sense the user may need comfort or encouragement, include ",
    "[SIGNAL:comfort]",
    " in your reply."
);

/// build_context 参数聚合，减少函数签名复杂度。
///
/// 所有与资源预算相关的字段（`system_max_len`、`messages_max_len`、`llm_hint`）
/// 由调用方从 `orchestrator::current_budget()` 取值后显式传入，
/// 避免 `build_context` 直接依赖 orchestrator 全局状态，保持函数可单独测试。
pub struct ContextParams<'a> {
    pub msg: &'a PcMsg,
    pub memory: &'a dyn MemoryStore,
    pub session: &'a dyn SessionStore,
    pub important_message_store: &'a dyn ImportantMessageStore,
    pub has_tools: bool,
    pub skill_descriptions: &'a str,
    pub system_max_len: usize,
    pub messages_max_len: usize,
    pub session_max_messages: usize,
    pub group_activation: &'a str,
    pub system_continuation_suffix: Option<&'a str>,
    pub emotion_signal_suffix: Option<&'a str>,
    pub summary_text: Option<&'a str>,
    pub runtime: Option<RuntimeContext>,
    /// orchestrator 在高压力时附加到 system 末尾的提示文字；由调用方从 `budget.llm_hint` 传入。
    pub llm_hint: &'a str,
}

fn push_if_fits(system: &mut String, addition: &str, max_len: usize) -> bool {
    if system.len().saturating_add(addition.len()) > max_len {
        return false;
    }
    system.push_str(addition);
    true
}

fn append_runtime_context(system: &mut String, max_len: usize, runtime: Option<RuntimeContext>) {
    let Some(runtime) = runtime else {
        return;
    };
    if runtime.now_secs == 0 {
        return;
    }
    let (y, mo, d, h, mi, s_sec) = crate::util::epoch_to_ymdhms(runtime.now_secs);
    let weekday = crate::util::weekday_name(runtime.now_secs / 86400);

    let mut header = String::with_capacity(72);
    let _ = write!(
        header,
        "\n\n## Runtime\nUTC: {:04}-{:02}-{:02} {} {:02}:{:02}:{:02}\nPlatform: {}",
        y, mo, d, weekday, h, mi, s_sec, runtime.platform
    );
    if !push_if_fits(system, &header, max_len) {
        return;
    }

    let mut pressure_line = String::with_capacity(64);
    let _ = write!(pressure_line, "\nPressure: {:?}", runtime.pressure);
    if !push_if_fits(system, &pressure_line, max_len) {
        return;
    }

    let mut queue_line = String::with_capacity(48);
    let _ = write!(
        queue_line,
        "\nAgent: tasks={} queues={}/{}",
        runtime.active_agent_tasks, runtime.inbound_depth, runtime.outbound_depth
    );
    let _ = push_if_fits(system, &queue_line, max_len);

    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
    {
        let mut linux_block = String::with_capacity(96);
        let _ = write!(
            linux_block,
            "\nLinux: cpu={:.0}% load={:.2}/{:.2}/{:.2} rss={}KB",
            runtime.cpu_usage_percent,
            runtime.load_average.0,
            runtime.load_average.1,
            runtime.load_average.2,
            runtime.process_memory_kb
        );
        let _ = push_if_fits(system, &linux_block, max_len);
    }
}

/// 根据入站 PcMsg 与 store 构建 (system, messages)，供 LlmClient.chat 使用。
///
/// **system 组成顺序**：SOUL → USER → MEMORY → daily_notes → skill_descriptions → 工具使用约束（有工具时）→ 群组/SILENT 约定；总长 ≤ system_max_len。
/// **截断策略**：base_max 直接使用 `system_max_len` 构造 system_base；skills/约束追加后若超限则按字符边界截断。
/// **失败降级**：任一源（get_soul/get_user/get_memory/list_daily_note_names）加载失败时降级为空字符串并打日志，不阻塞 build。
///
/// **messages**：历史会话（最近 session_max_messages 条）+ 当前用户 content，总长 ≤ messages_max_len；超限从最旧消息起丢弃。
/// **system_continuation_suffix**：多轮延续时追加到 system 末尾的上一轮产出说明；若提供则追加后再做最终截断。
pub fn build_context(p: &ContextParams<'_>) -> Result<(String, Vec<Message>)> {
    let soul_res = p.memory.get_soul();
    state::set_soul_load_ok(soul_res.is_ok());
    let soul = soul_res.unwrap_or_else(|e| {
        log::warn!("[context] get_soul failed: {}", e);
        String::new()
    });
    let user = p.memory.get_user().unwrap_or_else(|e| {
        log::warn!("[context] get_user failed: {}", e);
        String::new()
    });
    let mem_res = p.memory.get_memory();
    state::set_memory_load_ok(mem_res.is_ok());
    let mem = mem_res.unwrap_or_else(|e| {
        log::warn!("[context] get_memory failed: {}", e);
        String::new()
    });
    let names = p
        .memory
        .list_daily_note_names(DAILY_RECENT_N)
        .unwrap_or_else(|_| vec![]);
    let mut daily_contents: Vec<String> = Vec::with_capacity(names.len());
    for name in &names {
        if let Ok(c) = p.memory.get_daily_note(name) {
            daily_contents.push(c);
        }
    }
    let base_max = p.system_max_len;
    let system_base = build_system_prompt(&soul, &user, &mem, &daily_contents, base_max);
    let mut system = String::with_capacity(p.system_max_len);
    system.push_str(&system_base);
    // NOTE: tool_descriptions 不再注入 system prompt。工具规格已通过 API `tools` 参数
    // 以结构化 JSON schema 传递；在 system prompt 中重复文字版描述会导致部分模型
    // （尤其 OpenAI 兼容的国产模型）退化为"用文字说要调工具"而不走 tool_use 路径。
    if !p.skill_descriptions.is_empty() {
        let remain = p.system_max_len.saturating_sub(system.len());
        if remain > 0 {
            system.push_str("\n\n## Skills\n");
            if p.skill_descriptions.len() <= remain {
                system.push_str(p.skill_descriptions);
            } else {
                let mut end = remain;
                while end > 0 && !p.skill_descriptions.is_char_boundary(end) {
                    end -= 1;
                }
                system.push_str(&p.skill_descriptions[..end]);
            }
        }
    }
    // 工具使用行为约束：告诉模型直接调用而非文字描述，并鼓励简要说明推理
    if p.has_tools {
        let constraint = "\n\nWhen you decide to use a tool, call it directly via structured tool_use. Never describe or narrate the tool call in plain text. Before using tools, you may briefly explain your reasoning (1-2 sentences) to help track your thought process.";
        let remain = p.system_max_len.saturating_sub(system.len());
        if constraint.len() <= remain {
            system.push_str(constraint);
        }
    }
    append_runtime_context(&mut system, p.system_max_len, p.runtime);
    if p.msg.is_group {
        let remain = p.system_max_len.saturating_sub(system.len());
        if remain > 64 {
            if p.group_activation == "always" {
                system.push_str(
                    "\n\nIf no response is needed, reply with exactly SILENT and nothing else.",
                );
            } else if p.group_activation == "mention" {
                system.push_str("\n\nYou are in a group; only reply when explicitly mentioned.");
            }
        }
    }
    if let Some(suffix) = p.system_continuation_suffix {
        system.push_str("\n\n");
        system.push_str(suffix);
    }
    if system.len().saturating_add(STRUCTURED_BLOCK.len()) <= p.system_max_len {
        system.push_str(STRUCTURED_BLOCK);
    }
    if let Some(em) = p.emotion_signal_suffix {
        system.push_str("\n\n");
        system.push_str(em);
    }
    if !p.llm_hint.is_empty()
        && system
            .len()
            .saturating_add(p.llm_hint.len())
            .saturating_add(2)
            <= p.system_max_len
    {
        system.push_str("\n\n");
        system.push_str(p.llm_hint);
    }
    if system.len() > p.system_max_len {
        let mut end = p.system_max_len;
        while end > 0 && !system.is_char_boundary(end) {
            end -= 1;
        }
        system.truncate(end);
    }

    let n = p.session_max_messages.clamp(1, 128);
    let recent = p
        .session
        .load_recent(&p.msg.chat_id, n)
        .unwrap_or_else(|_| vec![]);
    let cap = recent.len() + if p.summary_text.is_some() { 2 } else { 1 };
    let mut messages: Vec<Message> = Vec::with_capacity(cap);
    if let Some(summary) = p.summary_text {
        messages.push(Message {
            role: Cow::Borrowed("user"),
            content: format!("[CONTEXT_SUMMARY]\n{}\n[/CONTEXT_SUMMARY]", summary),
        });
    }
    messages.extend(recent.into_iter().map(|m| Message {
        role: Cow::Owned(m.role),
        content: m.content,
    }));
    messages.push(Message {
        role: Cow::Borrowed("user"),
        content: p.msg.content.clone(),
    });

    let important_offset = p
        .important_message_store
        .get_important_offset(&p.msg.chat_id)
        .ok()
        .flatten();
    truncate_messages_to_len(&mut messages, p.messages_max_len, important_offset);
    if important_offset.is_some() {
        let _ = p.important_message_store.clear_important(&p.msg.chat_id);
    }

    Ok((system, messages))
}

fn truncate_messages_to_len(
    messages: &mut Vec<Message>,
    max_len: usize,
    protected_offset_from_end: Option<u32>,
) {
    let mut total = 0usize;
    for m in messages.iter() {
        total = total
            .saturating_add(m.role.len())
            .saturating_add(m.content.len())
            .saturating_add(2);
    }
    let protected_idx = protected_offset_from_end.and_then(|off| {
        let len = messages.len();
        let idx = len.saturating_sub(1).saturating_sub(off as usize);
        if idx < len {
            Some(idx)
        } else {
            None
        }
    });
    let mut indices_to_remove = Vec::new();
    for (i, m) in messages.iter().enumerate() {
        if total <= max_len {
            break;
        }
        if Some(i) == protected_idx {
            continue;
        }
        if messages.len() - indices_to_remove.len() <= 1 {
            break;
        }
        let sz = m
            .role
            .len()
            .saturating_add(m.content.len())
            .saturating_add(2);
        total = total.saturating_sub(sz);
        indices_to_remove.push(i);
    }
    let remove_indices = indices_to_remove;
    let drained = std::mem::take(messages);
    let mut kept = Vec::with_capacity(drained.len().saturating_sub(remove_indices.len()));
    let mut remove_cursor = 0usize;
    for (i, m) in drained.into_iter().enumerate() {
        let should_remove =
            remove_cursor < remove_indices.len() && remove_indices[remove_cursor] == i;
        if should_remove {
            remove_cursor += 1;
        } else {
            kept.push(m);
        }
    }
    *messages = kept;
    merge_consecutive_same_role(messages);
}

/// Merge consecutive messages with the same role (can happen after truncation removes intermediate messages).
fn merge_consecutive_same_role(messages: &mut Vec<Message>) {
    let mut i = 0;
    while i + 1 < messages.len() {
        if messages[i].role == messages[i + 1].role {
            let next_content = messages.remove(i + 1).content;
            messages[i].content.push('\n');
            messages[i].content.push_str(&next_content);
        } else {
            i += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_runtime() -> RuntimeContext {
        RuntimeContext {
            now_secs: crate::util::ymdhms_to_epoch(2026, 3, 31, 12, 34, 56),
            platform: "Linux",
            pressure: crate::orchestrator::PressureLevel::Normal,
            active_agent_tasks: 2,
            inbound_depth: 3,
            outbound_depth: 4,
            #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
            cpu_usage_percent: 17.0,
            #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
            load_average: (0.11, 0.22, 0.33),
            #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
            process_memory_kb: 4096,
        }
    }

    #[test]
    fn runtime_context_keeps_core_fields_under_tight_budget() {
        let runtime = sample_runtime();
        let core =
            "\n\n## Runtime\nUTC: 2026-03-31 Tuesday 12:34:56\nPlatform: Linux\nPressure: Normal";
        let mut system = String::new();
        append_runtime_context(&mut system, core.len(), Some(runtime));
        assert!(system.contains("UTC: 2026-03-31 Tuesday 12:34:56"));
        assert!(system.contains("Platform: Linux"));
        assert!(system.contains("Pressure: Normal"));
        assert!(!system.contains("Agent:"));
    }
}
