//! Agent 上下文构建：从 MemoryStore + SessionStore 聚合 system 与 messages。
//! Pure logic; no platform dependency; for use by agent::loop.

use crate::bus::PcMsg;
use crate::error::Result;
use crate::llm::Message;
use crate::memory::{
    build_context_messages, build_system_prompt, ImportantMessageStore, MemoryStore, SessionStore,
};
use crate::state;
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
    pub long_term_memory_text: Option<&'a str>,
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
    if let Some(long_term_memory_text) = p.long_term_memory_text {
        let remain = p.system_max_len.saturating_sub(system.len());
        if remain > 0 {
            system.push_str("\n\n");
            if long_term_memory_text.len() <= remain {
                system.push_str(long_term_memory_text);
            } else {
                let mut end = remain;
                while end > 0 && !long_term_memory_text.is_char_boundary(end) {
                    end -= 1;
                }
                system.push_str(&long_term_memory_text[..end]);
            }
        }
    }
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
    // 工具使用行为约束：给模型一个模式无关的硬约束，
    // 具体是原生 tools 还是 prompt-guided 协议，由后续请求装配层决定。
    if p.has_tools {
        let constraint = "\n\nWhen you decide to use a tool, use the provided tool invocation mechanism directly. Never describe or narrate a tool call in plain text without actually invoking it. Before using tools, you may briefly explain your reasoning (1-2 sentences) to help track your thought process.";
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

    let messages = build_context_messages(
        p.session,
        p.important_message_store,
        p.msg,
        p.session_max_messages,
        p.messages_max_len,
        p.summary_text,
    );

    Ok((system, messages))
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
