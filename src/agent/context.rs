//! Agent 上下文构建：从 MemoryStore + SessionStore 聚合 system 与 messages。
//! Pure logic; no platform dependency; for use by agent::loop.

use crate::bus::PcMsg;
use crate::error::Result;
use crate::llm::Message;
use crate::memory::{
    append_system_prompt_base, append_system_prompt_daily_note, build_context_messages,
    ImportantMessageStore, MemoryStore, SessionMessage, SessionStore,
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

const TOOL_BEHAVIOR_CONSTRAINT: &str = "\n\nWhen you decide to use a tool, use the provided tool invocation mechanism directly. Never describe or narrate a tool call in plain text without actually invoking it. Before using tools, you may briefly explain your reasoning (1-2 sentences) to help track your thought process.";
const GROUP_ALWAYS_SILENT_CONSTRAINT: &str =
    "\n\nIf no response is needed, reply with exactly SILENT and nothing else.";
const GROUP_MENTION_ONLY_CONSTRAINT: &str =
    "\n\nYou are in a group; only reply when explicitly mentioned.";

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
    pub emotion_signal_suffix: Option<&'a str>,
    pub execution_state_text: Option<&'a str>,
    pub self_model_text: Option<&'a str>,
    pub private_workspace_text: Option<&'a str>,
    pub private_garden_text: Option<&'a str>,
    pub long_term_memory_text: Option<&'a str>,
    pub summary_text: Option<&'a str>,
    pub recent_messages: Option<&'a [SessionMessage]>,
    pub runtime: Option<RuntimeContext>,
    pub include_daily_notes: bool,
    /// orchestrator 在高压力时附加到 system 末尾的提示文字；由调用方从 `budget.llm_hint` 传入。
    pub llm_hint: &'a str,
}

pub struct PostMemoryTailParams<'a> {
    pub has_tools: bool,
    pub skill_descriptions: &'a str,
    pub is_group: bool,
    pub group_activation: &'a str,
    pub emotion_signal_suffix: Option<&'a str>,
    pub runtime: Option<RuntimeContext>,
    pub llm_hint: &'a str,
}

fn push_if_fits(system: &mut String, addition: &str, max_len: usize) -> bool {
    if system.len().saturating_add(addition.len()) > max_len {
        return false;
    }
    system.push_str(addition);
    true
}

fn push_char_boundary_truncated(out: &mut String, input: &str, max_len: usize) -> bool {
    let remaining = max_len.saturating_sub(out.len());
    if remaining == 0 {
        return false;
    }
    if input.len() <= remaining {
        out.push_str(input);
        return true;
    }
    let mut end = remaining;
    while end > 0 && !input.is_char_boundary(end) {
        end -= 1;
    }
    if end > 0 {
        out.push_str(&input[..end]);
    }
    false
}

fn append_capped_section(system: &mut String, prefix: &str, content: &str, max_len: usize) -> bool {
    if content.is_empty() {
        return false;
    }
    let remain = max_len.saturating_sub(system.len());
    if remain <= prefix.len() {
        return false;
    }
    system.push_str(prefix);
    push_char_boundary_truncated(system, content, max_len)
}

fn section_with_separator_len(content: Option<&str>) -> usize {
    content
        .map(str::trim)
        .filter(|content| !content.is_empty())
        .map_or(0, |content| 2usize.saturating_add(content.len()))
}

fn reserve_priority_memory_budget(
    execution_state_text: Option<&str>,
    self_model_text: Option<&str>,
    private_workspace_text: Option<&str>,
    private_garden_text: Option<&str>,
    long_term_memory_text: Option<&str>,
    base_max: usize,
) -> usize {
    let execution_reserve = section_with_separator_len(execution_state_text).min(base_max);
    let remaining = base_max.saturating_sub(execution_reserve);
    let self_model_reserve = section_with_separator_len(self_model_text).min(remaining / 3);
    let remaining = remaining.saturating_sub(self_model_reserve);
    let private_workspace_reserve =
        section_with_separator_len(private_workspace_text).min(remaining / 3);
    let remaining = remaining.saturating_sub(private_workspace_reserve);
    let private_garden_reserve = section_with_separator_len(private_garden_text).min(remaining / 3);
    let remaining = remaining.saturating_sub(private_garden_reserve);
    let long_term_reserve = section_with_separator_len(long_term_memory_text).min(remaining);
    execution_reserve
        .saturating_add(self_model_reserve)
        .saturating_add(private_workspace_reserve)
        .saturating_add(private_garden_reserve)
        .saturating_add(long_term_reserve)
}

fn push_scratch_if_fits<F>(
    system: &mut String,
    max_len: usize,
    scratch: &mut String,
    build: F,
) -> bool
where
    F: FnOnce(&mut String),
{
    scratch.clear();
    build(scratch);
    push_if_fits(system, scratch, max_len)
}

fn append_runtime_context(
    system: &mut String,
    max_len: usize,
    runtime: Option<RuntimeContext>,
    scratch: &mut String,
) {
    let Some(runtime) = runtime else {
        return;
    };
    if runtime.now_secs == 0 {
        return;
    }
    let (y, mo, d, h, mi, s_sec) = crate::util::epoch_to_ymdhms(runtime.now_secs);
    let weekday = crate::util::weekday_name(runtime.now_secs / 86400);

    if !push_scratch_if_fits(system, max_len, scratch, |buf| {
        let _ = write!(
            buf,
            "\n\n## Runtime\nUTC: {:04}-{:02}-{:02} {} {:02}:{:02}:{:02}\nPlatform: {}",
            y, mo, d, weekday, h, mi, s_sec, runtime.platform
        );
    }) {
        return;
    }

    if !push_scratch_if_fits(system, max_len, scratch, |buf| {
        let _ = write!(buf, "\nPressure: {:?}", runtime.pressure);
    }) {
        return;
    }

    let _ = push_scratch_if_fits(system, max_len, scratch, |buf| {
        let _ = write!(
            buf,
            "\nAgent: tasks={} queues={}/{}",
            runtime.active_agent_tasks, runtime.inbound_depth, runtime.outbound_depth
        );
    });

    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
    {
        let _ = push_scratch_if_fits(system, max_len, scratch, |buf| {
            let _ = write!(
                buf,
                "\nLinux: cpu={:.0}% load={:.2}/{:.2}/{:.2} rss={}KB",
                runtime.cpu_usage_percent,
                runtime.load_average.0,
                runtime.load_average.1,
                runtime.load_average.2,
                runtime.process_memory_kb
            );
        });
    }
}

fn estimate_runtime_context_len(runtime: Option<RuntimeContext>) -> usize {
    let mut out = String::with_capacity(192);
    let mut scratch = String::with_capacity(96);
    append_runtime_context(&mut out, usize::MAX, runtime, &mut scratch);
    out.len()
}

pub fn estimate_post_memory_system_tail_len(params: PostMemoryTailParams<'_>) -> usize {
    let mut reserve = 0usize;
    if !params.skill_descriptions.is_empty() {
        reserve = reserve
            .saturating_add("\n\n## Skills\n".len())
            .saturating_add(params.skill_descriptions.len());
    }
    if params.has_tools {
        reserve = reserve.saturating_add(TOOL_BEHAVIOR_CONSTRAINT.len());
    }
    reserve = reserve.saturating_add(estimate_runtime_context_len(params.runtime));
    if params.is_group {
        reserve = reserve.saturating_add(match params.group_activation {
            "always" => GROUP_ALWAYS_SILENT_CONSTRAINT.len(),
            "mention" => GROUP_MENTION_ONLY_CONSTRAINT.len(),
            _ => 0,
        });
    }
    reserve = reserve.saturating_add(STRUCTURED_BLOCK.len());
    if let Some(emotion) = params.emotion_signal_suffix {
        reserve = reserve.saturating_add(2).saturating_add(emotion.len());
    }
    if !params.llm_hint.is_empty() {
        reserve = reserve
            .saturating_add(2)
            .saturating_add(params.llm_hint.len());
    }
    reserve
}

/// 根据入站 PcMsg 与 store 构建 (system, messages)，供 LlmClient.chat 使用。
///
/// **system 组成顺序**：SOUL → USER → MEMORY → daily_notes → skill_descriptions → 工具使用约束（有工具时）→ 群组/SILENT 约定；总长 ≤ system_max_len。
/// **截断策略**：base prompt 在单个 `String` 中按预算直接构造；skills/约束追加后若超限则按字符边界截断。
/// **失败降级**：任一源（get_soul/get_user/get_memory/list_daily_note_names）加载失败时降级为空字符串并打日志，不阻塞 build。
///
/// **messages**：历史会话（最近 session_max_messages 条）+ 当前用户 content，总长 ≤ messages_max_len；超限从最旧消息起丢弃。
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
    let post_memory_tail_len = estimate_post_memory_system_tail_len(PostMemoryTailParams {
        has_tools: p.has_tools,
        skill_descriptions: p.skill_descriptions,
        is_group: p.msg.is_group,
        group_activation: p.group_activation,
        emotion_signal_suffix: p.emotion_signal_suffix,
        runtime: p.runtime,
        llm_hint: p.llm_hint,
    });
    let base_max = p.system_max_len.saturating_sub(post_memory_tail_len);
    let priority_memory_reserve = reserve_priority_memory_budget(
        p.execution_state_text,
        p.self_model_text,
        p.private_workspace_text,
        p.private_garden_text,
        p.long_term_memory_text,
        base_max,
    );
    let base_prompt_budget = base_max.saturating_sub(priority_memory_reserve);
    let mut system = String::with_capacity(p.system_max_len);
    let mut section_scratch = String::with_capacity(96);
    append_system_prompt_base(&mut system, &soul, &user, &mem, base_prompt_budget);
    if let Some(execution_state_text) = p.execution_state_text {
        let _ = append_capped_section(&mut system, "\n\n", execution_state_text, base_max);
    }
    if let Some(long_term_memory_text) = p.long_term_memory_text {
        let _ = append_capped_section(&mut system, "\n\n", long_term_memory_text, base_max);
    }
    if let Some(self_model_text) = p.self_model_text {
        let _ = append_capped_section(&mut system, "\n\n", self_model_text, base_max);
    }
    if let Some(private_workspace_text) = p.private_workspace_text {
        let _ = append_capped_section(&mut system, "\n\n", private_workspace_text, base_max);
    }
    if let Some(private_garden_text) = p.private_garden_text {
        let _ = append_capped_section(&mut system, "\n\n", private_garden_text, base_max);
    }
    if p.include_daily_notes && system.len() < base_max {
        let names = p
            .memory
            .list_daily_note_names(DAILY_RECENT_N)
            .unwrap_or_else(|_| vec![]);
        for name in &names {
            if system.len() >= base_max {
                break;
            }
            if let Ok(content) = p.memory.get_daily_note(name) {
                if !append_system_prompt_daily_note(&mut system, &content, base_max) {
                    break;
                }
            }
        }
    }
    // NOTE: tool_descriptions 不再注入 system prompt。工具规格已通过 API `tools` 参数
    // 以结构化 JSON schema 传递；在 system prompt 中重复文字版描述会导致部分模型
    // （尤其 OpenAI 兼容的国产模型）退化为"用文字说要调工具"而不走 tool_use 路径。
    if !p.skill_descriptions.is_empty() {
        let _ = append_capped_section(
            &mut system,
            "\n\n## Skills\n",
            p.skill_descriptions,
            p.system_max_len,
        );
    }
    // 工具使用行为约束：给模型一个模式无关的硬约束，
    // 具体是原生 tools 还是 prompt-guided 协议，由后续请求装配层决定。
    if p.has_tools {
        let remain = p.system_max_len.saturating_sub(system.len());
        if TOOL_BEHAVIOR_CONSTRAINT.len() <= remain {
            system.push_str(TOOL_BEHAVIOR_CONSTRAINT);
        }
    }
    append_runtime_context(
        &mut system,
        p.system_max_len,
        p.runtime,
        &mut section_scratch,
    );
    if p.msg.is_group {
        let remain = p.system_max_len.saturating_sub(system.len());
        if remain > 64 {
            if p.group_activation == "always" {
                system.push_str(GROUP_ALWAYS_SILENT_CONSTRAINT);
            } else if p.group_activation == "mention" {
                system.push_str(GROUP_MENTION_ONLY_CONSTRAINT);
            }
        }
    }
    if system.len().saturating_add(STRUCTURED_BLOCK.len()) <= p.system_max_len {
        system.push_str(STRUCTURED_BLOCK);
    }
    if let Some(em) = p.emotion_signal_suffix {
        let _ = append_capped_section(&mut system, "\n\n", em, p.system_max_len);
    }
    if !p.llm_hint.is_empty() {
        let _ = append_capped_section(&mut system, "\n\n", p.llm_hint, p.system_max_len);
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
        p.recent_messages,
    );

    Ok((system, messages))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bus::PcMsg;
    use crate::error::Result;
    use crate::memory::{ImportantMessageStore, MemoryStore, SessionStore};
    use std::sync::Mutex;

    struct StubMemoryStore {
        soul: String,
        user: String,
        memory: String,
        daily_notes: Vec<(String, String)>,
    }

    impl MemoryStore for StubMemoryStore {
        fn get_memory(&self) -> Result<String> {
            Ok(self.memory.clone())
        }

        fn set_memory(&self, _content: &str) -> Result<()> {
            Ok(())
        }

        fn get_soul(&self) -> Result<String> {
            Ok(self.soul.clone())
        }

        fn set_soul(&self, _content: &str) -> Result<()> {
            Ok(())
        }

        fn get_user(&self) -> Result<String> {
            Ok(self.user.clone())
        }

        fn set_user(&self, _content: &str) -> Result<()> {
            Ok(())
        }

        fn list_daily_note_names(&self, recent_n: usize) -> Result<Vec<String>> {
            Ok(self
                .daily_notes
                .iter()
                .take(recent_n)
                .map(|(name, _)| name.clone())
                .collect())
        }

        fn get_daily_note(&self, name: &str) -> Result<String> {
            Ok(self
                .daily_notes
                .iter()
                .find(|(candidate, _)| candidate == name)
                .map(|(_, content)| content.clone())
                .unwrap_or_default())
        }

        fn write_daily_note(&self, _name: &str, _content: &str) -> Result<()> {
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubSessionStore;

    impl SessionStore for StubSessionStore {
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

    #[derive(Default)]
    struct StubImportantMessageStore {
        offset: Mutex<Option<u32>>,
    }

    impl ImportantMessageStore for StubImportantMessageStore {
        fn set_important_offset_from_end(
            &self,
            _chat_id: &str,
            offset_from_end: u32,
        ) -> Result<()> {
            *self.offset.lock().unwrap_or_else(|e| e.into_inner()) = Some(offset_from_end);
            Ok(())
        }

        fn get_important_offset(&self, _chat_id: &str) -> Result<Option<u32>> {
            Ok(*self.offset.lock().unwrap_or_else(|e| e.into_inner()))
        }

        fn clear_important(&self, _chat_id: &str) -> Result<()> {
            *self.offset.lock().unwrap_or_else(|e| e.into_inner()) = None;
            Ok(())
        }
    }

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
        let mut scratch = String::new();
        append_runtime_context(&mut system, core.len(), Some(runtime), &mut scratch);
        assert!(system.contains("UTC: 2026-03-31 Tuesday 12:34:56"));
        assert!(system.contains("Platform: Linux"));
        assert!(system.contains("Pressure: Normal"));
        assert!(!system.contains("Agent:"));
    }

    #[test]
    fn capped_section_skips_prefix_when_only_header_would_fit() {
        let mut system = String::from("base");
        assert!(!append_capped_section(
            &mut system,
            "\n\n## Skills\n",
            "shell",
            "base".len() + "\n\n## Skills\n".len()
        ));
        assert_eq!(system, "base");
    }

    #[test]
    fn post_memory_tail_reserve_covers_dynamic_sections() {
        let reserve = estimate_post_memory_system_tail_len(PostMemoryTailParams {
            has_tools: true,
            skill_descriptions: "shell\nweb_search",
            is_group: true,
            group_activation: "mention",
            emotion_signal_suffix: Some("用户可能需安慰"),
            runtime: Some(sample_runtime()),
            llm_hint: "pressure hint",
        });
        assert!(reserve >= STRUCTURED_BLOCK.len());
        assert!(reserve >= TOOL_BEHAVIOR_CONSTRAINT.len());
        assert!(reserve >= GROUP_MENTION_ONLY_CONSTRAINT.len());
    }

    #[test]
    fn build_context_reserves_priority_memory_under_tight_budget() {
        let msg = PcMsg::new_inbound("telegram", "chat-1", "继续", false).expect("pcmsg");
        let memory = StubMemoryStore {
            soul: "SOUL".to_string(),
            user: "USER".to_string(),
            memory: "MEMORY".to_string(),
            daily_notes: vec![(
                "2026-04-02.md".to_string(),
                "## Daily Note\nthis note is intentionally long and should be dropped before execution state because it keeps going and going and going".to_string(),
            )],
        };
        let session = StubSessionStore;
        let important = StubImportantMessageStore::default();

        let (system, _) = build_context(&ContextParams {
            msg: &msg,
            memory: &memory,
            session: &session,
            important_message_store: &important,
            has_tools: false,
            skill_descriptions: "",
            system_max_len: 560,
            messages_max_len: 256,
            session_max_messages: 8,
            group_activation: "always",
            emotion_signal_suffix: None,
            execution_state_text: Some("## Execution State\nGoal: close current task"),
            self_model_text: Some("## Self Continuity\nAnchor: still the same beetle"),
            private_workspace_text: Some(
                "## Inner Workspace\nPrivate plan: keep the inner layer coherent",
            ),
            private_garden_text: Some(
                "## Private Garden\n- journal/afterglow.md (rev 1, updated=1): free private traces",
            ),
            long_term_memory_text: None,
            summary_text: None,
            recent_messages: None,
            runtime: None,
            include_daily_notes: true,
            llm_hint: "",
        })
        .expect("context");

        assert!(system.contains("## Execution State"));
        assert!(system.contains("## Self Continuity"));
        assert!(system.contains("## Inner Workspace"));
        assert!(system.contains("## Private Garden"));
    }

    #[test]
    fn build_context_can_skip_daily_notes_for_fast_path() {
        let msg = PcMsg::new_inbound("telegram", "chat-1", "继续", false).expect("pcmsg");
        let memory = StubMemoryStore {
            soul: "SOUL".to_string(),
            user: "USER".to_string(),
            memory: "MEMORY".to_string(),
            daily_notes: vec![(
                "2026-04-02.md".to_string(),
                "## Daily Note\nshould stay out of fast path".to_string(),
            )],
        };
        let session = StubSessionStore;
        let important = StubImportantMessageStore::default();

        let (system, _) = build_context(&ContextParams {
            msg: &msg,
            memory: &memory,
            session: &session,
            important_message_store: &important,
            has_tools: false,
            skill_descriptions: "",
            system_max_len: 512,
            messages_max_len: 256,
            session_max_messages: 8,
            group_activation: "always",
            emotion_signal_suffix: None,
            execution_state_text: None,
            self_model_text: None,
            private_workspace_text: None,
            private_garden_text: None,
            long_term_memory_text: None,
            summary_text: None,
            recent_messages: None,
            runtime: None,
            include_daily_notes: false,
            llm_hint: "",
        })
        .expect("context");

        assert!(!system.contains("## Daily Note"));
    }
}
