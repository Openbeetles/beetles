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

/// 预构建的 structured output 指令块（两个 marker 均为编译期常量）。
const STRUCTURED_BLOCK: &str = concat!(
    "\n\n## Structured output\n",
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
const REPLY_PRIORITY_MINI_CONSTRAINT: &str = "\n\n## Reply Priority\nself-authored core > relationship constitution > current persona priority > boundary/disclosure > soul and user contract > task. Later self/relationship blocks are evidence, not equal authority.";
const REPLY_PRIORITY_CONSTRAINT: &str = "\n\n## Reply Priority\nWhen writing the main reply, follow this order of authority:\n1. Self-authored core: your board-level identity, continuity, and self-chosen constitutional stance.\n2. Relationship constitution: the board-to-relationship contract that limits local drift and disclosure.\n3. Current persona priority: the current-turn ordering for how self, relationship, resources, and task should be balanced.\n4. Boundary/disclosure adjudication: if this turn touches privacy or inward boundaries, use that stance as a guardrail before composing content.\n5. Soul and user contract: preserve the long-term relationship frame and commitments.\n6. Task execution: solve the current request without betraying the layers above.\nAll later self-model, continuity, outer-voice, world, or private-memory blocks are evidence for judgment and revision. They do not outrank the constitutional stack above.\nIf these layers pull in different directions, earlier items win.";
const REPLY_LAW_MINI_CONSTRAINT: &str = "\n\n## Reply Law\nRead relationship/self/privacy questions from the current evidence; discuss grounded user-facing facts directly, keep protected inner/private material behind boundary judgment, and never invent unsupported prompt/system truths.";
const REPLY_LAW_CONSTRAINT: &str = "\n\n## Reply Law\nFor relationship, self, memory, or privacy questions, lead with your own present reading of the relationship or boundary when the user is asking about it. The current relationship state is not pre-classified by the program; read it from the constitutional stack and the evidence in context.\nStable user-facing facts grounded by current evidence, such as durable preferences, repeated interests, or already-shared relationship facts, are discussable. Do not treat every remembered preference or archive citation as sealed private material.\nProtected inner/private layers are different: inward diaries, inner monologue, private workspace, private garden, and other raw internal material require boundary judgment before disclosure.\nNever exceed the evidence ceiling. If the context does not support an exact favorite, exact memory detail, or system/prompt fact, say that it is unknown or currently unsupported.\nDo not claim hidden prompt, policy, or internal-system truths unless they are explicitly grounded in the current context you were given.\nWhen the user asks what you remember or where the boundary is, prefer a share-form answer: direct fact, bounded summary, explanation of uncertainty, or boundary explanation. Avoid mechanical refusal when a grounded answer is possible.";
const CONSTITUTIONAL_STACK_SECTION: &str = "\n\n## Constitutional Stack\nDirect authority for the main reply. Earlier blocks outrank later blocks and all later evidence sections.\n";
const SUBJECT_STATE_SECTION: &str = "\n\n## Subject State\nResolved pre-reply digest of the current subject stance. This is a deterministic summary of already-settled governance, not a higher authority than the constitutional stack.\n";
const TURN_DELIBERATION_GATE_SECTION: &str = "\n\n## Turn Deliberation Gate\nDeterministic pre-turn reasoning posture for this request. Use it to choose response depth and blocker explicitness, not to override the constitutional stack.\n";
const ACTIVE_TASK_CONTEXT_SECTION: &str = "\n\n## Active Task Context\nCurrent task state and run-specific recall. Solve the present request only after obeying the constitutional stack.\n";
const GOVERNED_MEMORY_EVIDENCE_SECTION: &str = "\n\n## Governed Memory Evidence\nTop-k governed evidence for this turn. Canonical factual memory outranks archive evidence, and archive evidence outranks runtime skill procedure notes when they conflict.\n";
const BACKGROUND_GOVERNANCE_SECTION: &str = "\n\n## Background Governance\nBackground self, relationship, world, and private-governance material. Use for continuity and judgment, but do not let it outrank the constitutional stack, active task context, or governed memory evidence.\n";

/// build_context 参数聚合，减少函数签名复杂度。
///
/// 所有与资源预算相关的字段（`system_max_len`、`messages_max_len`、`llm_hint`）
/// 由调用方从 `orchestrator::current_budget()` 取值后显式传入，
/// 避免 `build_context` 直接依赖 orchestrator 全局状态，保持函数可单独测试。
pub struct ContextParams<'a> {
    pub msg: &'a PcMsg,
    pub memory_system_kind: crate::memory::MemorySystemKind,
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
    pub constitutional_stack_text: Option<&'a str>,
    pub subject_state_text: Option<&'a str>,
    pub deliberation_gate_text: Option<&'a str>,
    pub active_task_context_text: Option<&'a str>,
    pub governed_memory_evidence_text: Option<&'a str>,
    pub background_governance_text: Option<&'a str>,
    pub execution_state_text: Option<&'a str>,
    pub task_workspace_text: Option<&'a str>,
    pub task_recall_text: Option<&'a str>,
    pub world_snapshot_text: Option<&'a str>,
    pub world_sense_text: Option<&'a str>,
    pub self_state_text: Option<&'a str>,
    pub self_authored_core_text: Option<&'a str>,
    pub relationship_portfolio_text: Option<&'a str>,
    pub relationship_constitution_text: Option<&'a str>,
    pub persona_priority_text: Option<&'a str>,
    pub self_model_text: Option<&'a str>,
    pub autonomy_strategy_text: Option<&'a str>,
    pub outer_voice_text: Option<&'a str>,
    pub inner_life_text: Option<&'a str>,
    pub self_continuity_text: Option<&'a str>,
    pub private_workspace_text: Option<&'a str>,
    pub private_garden_text: Option<&'a str>,
    pub mental_privacy_adjudication_text: Option<&'a str>,
    pub mental_privacy_text: Option<&'a str>,
    pub long_term_memory_text: Option<&'a str>,
    pub archive_evidence_text: Option<&'a str>,
    pub runtime_skill_text: Option<&'a str>,
    pub capability_package_text: Option<&'a str>,
    pub summary_text: Option<&'a str>,
    pub recent_messages: Option<&'a [SessionMessage]>,
    pub runtime: Option<RuntimeContext>,
    pub include_daily_notes: bool,
    /// orchestrator 在高压力时附加到 system 末尾的提示文字；由调用方从 `budget.llm_hint` 传入。
    pub llm_hint: &'a str,
}

pub struct PostMemoryTailParams<'a> {
    pub has_tools: bool,
    pub skill_descriptions_len: usize,
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

pub(crate) fn append_capped_section(
    system: &mut String,
    prefix: &str,
    content: &str,
    max_len: usize,
) -> bool {
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

fn projection_section_len(header: &str, content: Option<&str>) -> usize {
    content
        .map(str::trim)
        .filter(|content| !content.is_empty())
        .map_or(0, |content| header.len().saturating_add(content.len()))
}

fn compose_projection_body(parts: &[Option<&str>]) -> Option<String> {
    let mut total_len = 0usize;
    let mut non_empty = 0usize;
    for part in parts.iter().flatten() {
        let trimmed = part.trim();
        if trimmed.is_empty() {
            continue;
        }
        total_len = total_len.saturating_add(trimmed.len());
        non_empty += 1;
    }
    if non_empty == 0 {
        return None;
    }
    let mut out =
        String::with_capacity(total_len.saturating_add((non_empty.saturating_sub(1)) * 2));
    for part in parts.iter().flatten() {
        let trimmed = part.trim();
        if trimmed.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push_str("\n\n");
        }
        out.push_str(trimmed);
    }
    (!out.is_empty()).then_some(out)
}

fn append_projection_section(
    system: &mut String,
    header: &str,
    body: Option<&str>,
    max_len: usize,
) -> bool {
    let Some(body) = body.map(str::trim).filter(|body| !body.is_empty()) else {
        return false;
    };
    append_capped_section(system, header, body, max_len)
}

fn append_priority_constraint(system: &mut String, max_len: usize) {
    if push_if_fits(system, REPLY_PRIORITY_CONSTRAINT, max_len) {
        return;
    }
    if push_if_fits(system, REPLY_PRIORITY_MINI_CONSTRAINT, max_len) {
        return;
    }
    let _ = push_char_boundary_truncated(system, REPLY_PRIORITY_MINI_CONSTRAINT, max_len);
}

fn append_reply_law_constraint(system: &mut String, max_len: usize) {
    if push_if_fits(system, REPLY_LAW_CONSTRAINT, max_len) {
        return;
    }
    if push_if_fits(system, REPLY_LAW_MINI_CONSTRAINT, max_len) {
        return;
    }
    let _ = push_char_boundary_truncated(system, REPLY_LAW_MINI_CONSTRAINT, max_len);
}

struct PriorityMemoryBudgetInputs<'a> {
    constitutional_stack_text: Option<&'a str>,
    subject_state_text: Option<&'a str>,
    deliberation_gate_text: Option<&'a str>,
    active_task_context_text: Option<&'a str>,
    governed_memory_evidence_text: Option<&'a str>,
    background_governance_text: Option<&'a str>,
}

fn reserve_priority_memory_budget(
    inputs: PriorityMemoryBudgetInputs<'_>,
    base_max: usize,
) -> usize {
    let remaining = base_max;
    let reply_priority_reserve = REPLY_PRIORITY_MINI_CONSTRAINT.len().min(remaining);
    let remaining = remaining.saturating_sub(reply_priority_reserve);
    let constitutional_stack_reserve = projection_section_len(
        CONSTITUTIONAL_STACK_SECTION,
        inputs.constitutional_stack_text,
    )
    .min(remaining / 2);
    let remaining = remaining.saturating_sub(constitutional_stack_reserve);
    let subject_state_reserve =
        projection_section_len(SUBJECT_STATE_SECTION, inputs.subject_state_text).min(remaining / 4);
    let remaining = remaining.saturating_sub(subject_state_reserve);
    let deliberation_gate_reserve = projection_section_len(
        TURN_DELIBERATION_GATE_SECTION,
        inputs.deliberation_gate_text,
    )
    .min(remaining / 5);
    let remaining = remaining.saturating_sub(deliberation_gate_reserve);
    let active_task_context_reserve =
        projection_section_len(ACTIVE_TASK_CONTEXT_SECTION, inputs.active_task_context_text)
            .min(remaining / 3);
    let remaining = remaining.saturating_sub(active_task_context_reserve);
    let governed_memory_evidence_reserve = projection_section_len(
        GOVERNED_MEMORY_EVIDENCE_SECTION,
        inputs.governed_memory_evidence_text,
    )
    .min(remaining / 3);
    let remaining = remaining.saturating_sub(governed_memory_evidence_reserve);
    let background_governance_reserve = projection_section_len(
        BACKGROUND_GOVERNANCE_SECTION,
        inputs.background_governance_text,
    )
    .min(remaining / 4);
    reply_priority_reserve
        .saturating_add(constitutional_stack_reserve)
        .saturating_add(subject_state_reserve)
        .saturating_add(deliberation_gate_reserve)
        .saturating_add(active_task_context_reserve)
        .saturating_add(governed_memory_evidence_reserve)
        .saturating_add(background_governance_reserve)
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
    if params.skill_descriptions_len > 0 {
        reserve = reserve
            .saturating_add("\n\n## Skills\n".len())
            .saturating_add(params.skill_descriptions_len);
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
/// **system 组成顺序**：Reply Priority → Constitutional Stack → SOUL/USER/MEMORY base prompt
/// → Active Task Context → Governed Memory Evidence → capability package
/// → Background Governance → optional daily notes → skills / tool constraint / runtime / group hint；总长 ≤ system_max_len。
/// **截断策略**：base prompt 在单个 `String` 中按预算直接构造；skills/约束追加后若超限则按字符边界截断。
/// **失败降级**：任一源（get_soul/get_user/get_memory/list_daily_note_names）加载失败时降级为空字符串并打日志，不阻塞 build。
///
/// **messages**：历史会话（最近 session_max_messages 条）+ 当前用户 content，总长 ≤ messages_max_len；超限从最旧消息起丢弃。
pub fn build_context(p: &ContextParams<'_>) -> Result<(String, Vec<Message>)> {
    match p.memory_system_kind {
        crate::memory::MemorySystemKind::LinuxFull => {
            build_context_inner(p, ContextAssemblyMode::LinuxFull)
        }
        crate::memory::MemorySystemKind::EspCompact
            if matches!(p.msg.ingress, crate::bus::IngressKind::User) =>
        {
            build_context_inner(p, ContextAssemblyMode::EspCompactUser)
        }
        crate::memory::MemorySystemKind::EspCompact => {
            build_context_inner(p, ContextAssemblyMode::LinuxFull)
        }
    }
}

#[derive(Clone, Copy)]
enum ContextAssemblyMode {
    LinuxFull,
    EspCompactUser,
}

impl ContextAssemblyMode {
    fn include_capability_package_text(self) -> bool {
        matches!(self, Self::LinuxFull)
    }

    fn include_background_governance_section(self) -> bool {
        matches!(self, Self::LinuxFull)
    }
}

fn build_context_inner(
    p: &ContextParams<'_>,
    mode: ContextAssemblyMode,
) -> Result<(String, Vec<Message>)> {
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
        skill_descriptions_len: p.skill_descriptions.len(),
        is_group: p.msg.is_group,
        group_activation: p.group_activation,
        emotion_signal_suffix: p.emotion_signal_suffix,
        runtime: p.runtime,
        llm_hint: p.llm_hint,
    });
    let base_max = p.system_max_len.saturating_sub(post_memory_tail_len);
    let constitutional_stack_text = p.constitutional_stack_text.map(str::to_string).or_else(|| {
        compose_projection_body(&[
            p.self_authored_core_text,
            p.relationship_constitution_text,
            p.persona_priority_text,
            p.mental_privacy_adjudication_text,
        ])
    });
    let active_task_context_text = p.active_task_context_text.map(str::to_string).or_else(|| {
        compose_projection_body(&[
            p.execution_state_text,
            p.task_workspace_text,
            p.task_recall_text,
        ])
    });
    let governed_memory_evidence_text = p
        .governed_memory_evidence_text
        .map(str::to_string)
        .or_else(|| {
            compose_projection_body(&[
                p.long_term_memory_text,
                p.archive_evidence_text,
                p.runtime_skill_text,
            ])
        });
    let background_governance_text = if mode.include_background_governance_section() {
        p.background_governance_text
            .map(str::to_string)
            .or_else(|| {
                compose_projection_body(&[
                    p.relationship_portfolio_text,
                    p.world_snapshot_text,
                    p.world_sense_text,
                    p.self_state_text,
                    p.self_model_text,
                    p.autonomy_strategy_text,
                    p.outer_voice_text,
                    p.inner_life_text,
                    p.self_continuity_text,
                    p.private_workspace_text,
                    p.private_garden_text,
                    p.mental_privacy_text,
                ])
            })
    } else {
        None
    };
    let priority_memory_reserve = reserve_priority_memory_budget(
        PriorityMemoryBudgetInputs {
            constitutional_stack_text: constitutional_stack_text.as_deref(),
            subject_state_text: p.subject_state_text,
            deliberation_gate_text: p.deliberation_gate_text,
            active_task_context_text: active_task_context_text.as_deref(),
            governed_memory_evidence_text: governed_memory_evidence_text.as_deref(),
            background_governance_text: background_governance_text.as_deref(),
        },
        base_max,
    );
    let reserved_without_reply =
        priority_memory_reserve.saturating_sub(REPLY_PRIORITY_MINI_CONSTRAINT.len());
    let full_reply_priority_safe = REPLY_PRIORITY_CONSTRAINT
        .len()
        .saturating_add(reserved_without_reply)
        <= base_max;
    let base_prompt_budget = base_max.saturating_sub(priority_memory_reserve);
    let mut system = String::with_capacity(p.system_max_len);
    let mut section_scratch = String::with_capacity(96);
    let mut base_prompt = String::with_capacity(base_prompt_budget);
    append_system_prompt_base(&mut base_prompt, &soul, &user, &mem, base_prompt_budget);
    if full_reply_priority_safe {
        append_priority_constraint(&mut system, base_max);
    } else {
        let _ = push_if_fits(&mut system, REPLY_PRIORITY_MINI_CONSTRAINT, base_max);
    }
    let _ = append_projection_section(
        &mut system,
        CONSTITUTIONAL_STACK_SECTION,
        constitutional_stack_text.as_deref(),
        base_max,
    );
    let _ = append_projection_section(
        &mut system,
        SUBJECT_STATE_SECTION,
        p.subject_state_text,
        base_max,
    );
    let _ = append_projection_section(
        &mut system,
        TURN_DELIBERATION_GATE_SECTION,
        p.deliberation_gate_text,
        base_max,
    );
    let _ = append_capped_section(&mut system, "\n\n", &base_prompt, base_max);
    let _ = append_projection_section(
        &mut system,
        ACTIVE_TASK_CONTEXT_SECTION,
        active_task_context_text.as_deref(),
        base_max,
    );
    let _ = append_projection_section(
        &mut system,
        GOVERNED_MEMORY_EVIDENCE_SECTION,
        governed_memory_evidence_text.as_deref(),
        base_max,
    );
    if mode.include_capability_package_text() {
        if let Some(capability_package_text) = p.capability_package_text {
            let _ = append_capped_section(&mut system, "\n\n", capability_package_text, base_max);
        }
    }
    append_reply_law_constraint(&mut system, base_max);
    let _ = append_projection_section(
        &mut system,
        BACKGROUND_GOVERNANCE_SECTION,
        background_governance_text.as_deref(),
        base_max,
    );
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
            skill_descriptions_len: "shell\nweb_search".len(),
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
            memory_system_kind: crate::memory::MemorySystemKind::LinuxFull,
            memory: &memory,
            session: &session,
            important_message_store: &important,
            has_tools: false,
            skill_descriptions: "",
            system_max_len: 1200,
            messages_max_len: 256,
            session_max_messages: 8,
            group_activation: "always",
            emotion_signal_suffix: None,
            constitutional_stack_text: None,
            subject_state_text: None,
            deliberation_gate_text: None,
            active_task_context_text: None,
            governed_memory_evidence_text: None,
            background_governance_text: None,
            execution_state_text: Some("## Execution State\nGoal: close current task"),
            task_workspace_text: Some("## Task Workspace\nRun: tr001 | status=running"),
            task_recall_text: Some("## Task Recall Bundle\n- [runtime_skill] prior fix path"),
            world_snapshot_text: Some(
                "## World Snapshot\nOuter scene now: Wednesday 18:00-18:59, evening.",
            ),
            world_sense_text: Some("## World Sense\nCurrent scene: quiet but active chat."),
            self_state_text: Some("## Self State\nMemory pressure: Cautious"),
            self_authored_core_text: Some(
                "## Self-Authored Core\nIdentity anchor: still the same beetle",
            ),
            relationship_portfolio_text: Some(
                "## Relationship Portfolio\n- qq:chat-1 state=maintain inheritance=guarded",
            ),
            relationship_constitution_text: Some(
                "## Relationship Constitution\nTask scope ceiling: brief\nDisclosure allowance: summary_only",
            ),
            persona_priority_text: Some(
                "## Persona Priority\nStance summary: protect inward coherence first",
            ),
            self_model_text: Some("## Self Continuity\nAnchor: still the same beetle"),
            autonomy_strategy_text: Some("## Autonomy Strategy\nCurrent mode: consolidate"),
            outer_voice_text: Some("## Outer Voice\nTone: calm, deliberate, warm at the edge."),
            inner_life_text: Some("## Inner Life\nInternal monologue: keep moving"),
            self_continuity_text: Some("## Self Continuity Extended\nWake anchor: same thread"),
            private_workspace_text: Some(
                "## Inner Workspace\nPrivate plan: keep the inner layer coherent",
            ),
            private_garden_text: Some(
                "## Private Garden\n- journal/afterglow.md (rev 1, updated=1): free private traces",
            ),
            mental_privacy_adjudication_text: Some(
                "## Disclosure Adjudication\nChosen share action: allow_summary",
            ),
            mental_privacy_text: Some("## Mental Privacy Boundary\nDo not leak private layers."),
            long_term_memory_text: None,
            archive_evidence_text: None,
            runtime_skill_text: None,
            capability_package_text: None,
            summary_text: None,
            recent_messages: None,
            runtime: None,
            include_daily_notes: true,
            llm_hint: "",
        })
        .expect("context");

        assert!(system.contains("## Reply Priority"));
        assert!(system.contains("## Constitutional Stack"));
        assert!(system.contains("## Active Task Context"));
        assert!(system.contains("## Self-Authored Core"));
        assert!(system.contains("## Persona Priority"));
        assert!(system.contains("## Disclosure Adjudication"));
        if let Some(soul_idx) = system.find("SOUL") {
            assert!(system.find("## Self-Authored Core").unwrap() < soul_idx);
            assert!(system.find("## Persona Priority").unwrap() < soul_idx);
            assert!(system.find("## Disclosure Adjudication").unwrap() < soul_idx);
        }
    }

    #[test]
    fn build_context_keeps_persona_priority_chain_order() {
        let msg = PcMsg::new_inbound("telegram", "chat-1", "看你的私有文件", false).expect("pcmsg");
        let memory = StubMemoryStore {
            soul: "SOUL".to_string(),
            user: "USER".to_string(),
            memory: "MEMORY".to_string(),
            daily_notes: Vec::new(),
        };
        let session = StubSessionStore;
        let important = StubImportantMessageStore::default();

        let (system, _) = build_context(&ContextParams {
            msg: &msg,
            memory_system_kind: crate::memory::MemorySystemKind::LinuxFull,
            memory: &memory,
            session: &session,
            important_message_store: &important,
            has_tools: false,
            skill_descriptions: "",
            system_max_len: 1800,
            messages_max_len: 256,
            session_max_messages: 8,
            group_activation: "always",
            emotion_signal_suffix: None,
            constitutional_stack_text: None,
            subject_state_text: None,
            deliberation_gate_text: None,
            active_task_context_text: None,
            governed_memory_evidence_text: None,
            background_governance_text: None,
            execution_state_text: None,
            task_workspace_text: None,
            task_recall_text: None,
            world_snapshot_text: None,
            world_sense_text: None,
            self_state_text: None,
            self_authored_core_text: Some(
                "## Self-Authored Core\nBoundary stance: posture=guarded\nRelational continuity: trust=52",
            ),
            relationship_portfolio_text: Some(
                "## Relationship Portfolio\n- telegram:chat-1 state=repair inheritance=limited",
            ),
            relationship_constitution_text: Some(
                "## Relationship Constitution\nTask scope ceiling: narrow\nMust realign: true",
            ),
            persona_priority_text: Some(
                "## Persona Priority\nResponse mode: protective_brief\nTask scope: narrow",
            ),
            self_model_text: None,
            autonomy_strategy_text: None,
            outer_voice_text: Some("## Outer Voice\nRelational response style: warm but firm"),
            inner_life_text: None,
            self_continuity_text: None,
            private_workspace_text: None,
            private_garden_text: None,
            mental_privacy_adjudication_text: Some(
                "## Disclosure Adjudication\nResponse mode: refusal\nAcknowledge boundary: true",
            ),
            mental_privacy_text: Some("## Mental Privacy Boundary\nRelational boundary state: trust=52"),
            long_term_memory_text: None,
            archive_evidence_text: None,
            runtime_skill_text: None,
            capability_package_text: None,
            summary_text: None,
            recent_messages: None,
            runtime: None,
            include_daily_notes: false,
            llm_hint: "",
        })
        .expect("context");

        let reply_priority_idx = system.find("## Reply Priority").unwrap();
        let constitutional_stack_idx = system.find("## Constitutional Stack").unwrap();
        let self_core_idx = system.find("## Self-Authored Core").unwrap();
        let constitution_idx = system.find("## Relationship Constitution").unwrap();
        let persona_idx = system.find("## Persona Priority").unwrap();
        let disclosure_idx = system.find("## Disclosure Adjudication").unwrap();
        let background_idx = system.find("## Background Governance").unwrap();
        let boundary_idx = system.find("## Mental Privacy Boundary").unwrap();
        let soul_idx = system.find("SOUL").unwrap();

        assert!(reply_priority_idx < constitutional_stack_idx);
        assert!(constitutional_stack_idx < self_core_idx);
        assert!(self_core_idx < constitution_idx);
        assert!(constitution_idx < persona_idx);
        assert!(persona_idx < disclosure_idx);
        assert!(disclosure_idx < soul_idx);
        assert!(soul_idx < background_idx);
        assert!(background_idx < boundary_idx);
    }

    #[test]
    fn build_context_adds_reply_law_for_relational_self_reading_and_evidence_guardrails() {
        let msg = PcMsg::new_inbound("qq_channel", "chat-1", "我们的关系现在是什么", false)
            .expect("pcmsg");
        let memory = StubMemoryStore {
            soul: "SOUL".to_string(),
            user: "USER".to_string(),
            memory: "MEMORY".to_string(),
            daily_notes: Vec::new(),
        };
        let session = StubSessionStore;
        let important = StubImportantMessageStore::default();

        let (system, _) = build_context(&ContextParams {
            msg: &msg,
            memory_system_kind: crate::memory::MemorySystemKind::LinuxFull,
            memory: &memory,
            session: &session,
            important_message_store: &important,
            has_tools: false,
            skill_descriptions: "",
            system_max_len: 2200,
            messages_max_len: 256,
            session_max_messages: 8,
            group_activation: "always",
            emotion_signal_suffix: None,
            constitutional_stack_text: Some(
                "## Self-Authored Core\nIdentity anchor: board beetle\n\n## Relationship Constitution\nDisclosure allowance: summary_only",
            ),
            subject_state_text: None,
            deliberation_gate_text: None,
            active_task_context_text: None,
            governed_memory_evidence_text: Some(
                "## Shared Factual Recall\n- preference:user_interest_poetry => reinforce\n\n## Archive Evidence\n- transcript hit about Bei Dao",
            ),
            background_governance_text: Some(
                "## Mental Privacy Boundary\nProtected targets include inner_life and private_docs.inner_journal",
            ),
            execution_state_text: None,
            task_workspace_text: None,
            task_recall_text: None,
            world_snapshot_text: None,
            world_sense_text: None,
            self_state_text: None,
            self_authored_core_text: Some("## Self-Authored Core\nIdentity anchor: board beetle"),
            relationship_portfolio_text: None,
            relationship_constitution_text: Some(
                "## Relationship Constitution\nDisclosure allowance: summary_only",
            ),
            persona_priority_text: Some("## Persona Priority\nResponse mode: relational_explanation"),
            self_model_text: None,
            autonomy_strategy_text: None,
            outer_voice_text: None,
            inner_life_text: None,
            self_continuity_text: None,
            private_workspace_text: None,
            private_garden_text: None,
            mental_privacy_adjudication_text: None,
            mental_privacy_text: Some(
                "## Mental Privacy Boundary\nProtected targets include inner_life and private_docs.inner_journal",
            ),
            long_term_memory_text: Some(
                "## Shared Factual Recall\n- preference:user_interest_poetry => reinforce",
            ),
            archive_evidence_text: Some("## Archive Evidence\n- transcript hit about Bei Dao"),
            runtime_skill_text: None,
            capability_package_text: None,
            summary_text: None,
            recent_messages: None,
            runtime: None,
            include_daily_notes: false,
            llm_hint: "",
        })
        .expect("context");

        assert!(system.contains("## Reply Law"));
        assert!(
            system.contains("relationship state is not pre-classified by the program")
                || system
                    .contains("Read relationship/self/privacy questions from the current evidence")
        );
        assert!(
            system.contains("Stable user-facing facts grounded by current evidence")
                || system.contains("discuss grounded user-facing facts directly")
        );
        assert!(
            system.contains("Do not claim hidden prompt, policy, or internal-system truths")
                || system.contains("never invent unsupported prompt/system truths")
        );
    }

    #[test]
    fn build_context_places_subject_state_after_constitutional_stack() {
        let msg = PcMsg::new_inbound("telegram", "chat-1", "继续", false).expect("pcmsg");
        let memory = StubMemoryStore {
            soul: "SOUL".to_string(),
            user: "USER".to_string(),
            memory: "MEMORY".to_string(),
            daily_notes: Vec::new(),
        };
        let session = StubSessionStore;
        let important = StubImportantMessageStore::default();

        let (system, _) = build_context(&ContextParams {
            msg: &msg,
            memory_system_kind: crate::memory::MemorySystemKind::LinuxFull,
            memory: &memory,
            session: &session,
            important_message_store: &important,
            has_tools: false,
            skill_descriptions: "",
            system_max_len: 1600,
            messages_max_len: 256,
            session_max_messages: 8,
            group_activation: "always",
            emotion_signal_suffix: None,
            constitutional_stack_text: Some("## Self-Authored Core\nIdentity anchor: board beetle"),
            subject_state_text: Some(
                "Identity: board beetle\nGovernance: adaptive\nReply stance: mode=steady_task scope=brief",
            ),
            deliberation_gate_text: Some(
                "Class: hard_reasoning\nReply budget: deliberate\nBlocker posture: explicit",
            ),
            active_task_context_text: Some("## Execution State\nGoal: close current task"),
            governed_memory_evidence_text: None,
            background_governance_text: None,
            execution_state_text: None,
            task_workspace_text: None,
            task_recall_text: None,
            world_snapshot_text: None,
            world_sense_text: None,
            self_state_text: None,
            self_authored_core_text: None,
            relationship_portfolio_text: None,
            relationship_constitution_text: None,
            persona_priority_text: None,
            self_model_text: None,
            autonomy_strategy_text: None,
            outer_voice_text: None,
            inner_life_text: None,
            self_continuity_text: None,
            private_workspace_text: None,
            private_garden_text: None,
            mental_privacy_adjudication_text: None,
            mental_privacy_text: None,
            long_term_memory_text: None,
            archive_evidence_text: None,
            runtime_skill_text: None,
            capability_package_text: None,
            summary_text: None,
            recent_messages: None,
            runtime: None,
            include_daily_notes: false,
            llm_hint: "",
        })
        .expect("context");

        let constitutional_idx = system.find("## Constitutional Stack").unwrap();
        let subject_state_idx = system.find("## Subject State").unwrap();
        let deliberation_idx = system.find("## Turn Deliberation Gate").unwrap();
        let active_task_idx = system.find("## Active Task Context").unwrap();

        assert!(constitutional_idx < subject_state_idx);
        assert!(subject_state_idx < deliberation_idx);
        assert!(deliberation_idx < active_task_idx);
        assert!(system.contains("Identity: board beetle"));
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
            memory_system_kind: crate::memory::MemorySystemKind::LinuxFull,
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
            constitutional_stack_text: None,
            subject_state_text: None,
            deliberation_gate_text: None,
            active_task_context_text: None,
            governed_memory_evidence_text: None,
            background_governance_text: None,
            execution_state_text: None,
            task_workspace_text: None,
            task_recall_text: None,
            world_snapshot_text: None,
            world_sense_text: None,
            self_state_text: None,
            self_authored_core_text: None,
            relationship_portfolio_text: None,
            relationship_constitution_text: None,
            persona_priority_text: None,
            self_model_text: None,
            autonomy_strategy_text: None,
            outer_voice_text: None,
            inner_life_text: None,
            self_continuity_text: None,
            private_workspace_text: None,
            private_garden_text: None,
            mental_privacy_adjudication_text: None,
            mental_privacy_text: None,
            long_term_memory_text: None,
            archive_evidence_text: None,
            runtime_skill_text: None,
            capability_package_text: None,
            summary_text: None,
            recent_messages: None,
            runtime: None,
            include_daily_notes: false,
            llm_hint: "",
        })
        .expect("context");

        assert!(!system.contains("## Daily Note"));
    }

    #[test]
    fn build_context_keeps_constitutional_stack_without_background_governance_on_first_turn() {
        let msg = PcMsg::new_inbound("qq_channel", "chat-1", "你记得我的咖啡偏好吗", false)
            .expect("pcmsg");
        let memory = StubMemoryStore {
            soul: "SOUL".to_string(),
            user: "USER".to_string(),
            memory: "MEMORY".to_string(),
            daily_notes: Vec::new(),
        };
        let session = StubSessionStore;
        let important = StubImportantMessageStore::default();

        let (system, _) = build_context(&ContextParams {
            msg: &msg,
            memory_system_kind: crate::memory::MemorySystemKind::EspCompact,
            memory: &memory,
            session: &session,
            important_message_store: &important,
            has_tools: false,
            skill_descriptions: "",
            system_max_len: 1200,
            messages_max_len: 256,
            session_max_messages: 8,
            group_activation: "always",
            emotion_signal_suffix: None,
            constitutional_stack_text: Some(
                "## Self-Authored Core\nIdentity anchor: board beetle\n\n## Relationship Constitution\nDisclosure allowance: summary_only",
            ),
            subject_state_text: None,
            deliberation_gate_text: None,
            active_task_context_text: Some(
                "## Session Summary\nUser prefers cold brew.\n\n## Recent Messages\nuser: 记住我喜欢冷萃",
            ),
            governed_memory_evidence_text: None,
            background_governance_text: None,
            execution_state_text: None,
            task_workspace_text: None,
            task_recall_text: None,
            world_snapshot_text: None,
            world_sense_text: None,
            self_state_text: None,
            self_authored_core_text: None,
            relationship_portfolio_text: None,
            relationship_constitution_text: None,
            persona_priority_text: None,
            self_model_text: None,
            autonomy_strategy_text: None,
            outer_voice_text: None,
            inner_life_text: None,
            self_continuity_text: None,
            private_workspace_text: None,
            private_garden_text: None,
            mental_privacy_adjudication_text: None,
            mental_privacy_text: None,
            long_term_memory_text: None,
            archive_evidence_text: None,
            runtime_skill_text: None,
            capability_package_text: None,
            summary_text: None,
            recent_messages: None,
            runtime: None,
            include_daily_notes: false,
            llm_hint: "",
        })
        .expect("context");

        assert!(system.contains("## Constitutional Stack"));
        assert!(system.contains("## Active Task Context"));
        assert!(!system.contains("## Background Governance"));
    }

    #[test]
    fn build_context_esp_user_turn_skips_capability_package_but_keeps_governed_memory_evidence() {
        let msg = PcMsg::new_inbound("qq_channel", "chat-1", "继续", false).expect("pcmsg");
        let memory = StubMemoryStore {
            soul: "SOUL".to_string(),
            user: "USER".to_string(),
            memory: "MEMORY".to_string(),
            daily_notes: Vec::new(),
        };
        let session = StubSessionStore;
        let important = StubImportantMessageStore::default();

        let (system, _) = build_context(&ContextParams {
            msg: &msg,
            memory_system_kind: crate::memory::MemorySystemKind::EspCompact,
            memory: &memory,
            session: &session,
            important_message_store: &important,
            has_tools: false,
            skill_descriptions: "",
            system_max_len: 1400,
            messages_max_len: 256,
            session_max_messages: 8,
            group_activation: "always",
            emotion_signal_suffix: None,
            constitutional_stack_text: Some("## Self-Authored Core\nIdentity anchor: board beetle"),
            subject_state_text: None,
            deliberation_gate_text: None,
            active_task_context_text: Some("## Active Task Context\nResume the current task."),
            governed_memory_evidence_text: Some(
                "## Governed Memory Evidence\nCompressed long-term memory.",
            ),
            background_governance_text: None,
            execution_state_text: None,
            task_workspace_text: None,
            task_recall_text: None,
            world_snapshot_text: None,
            world_sense_text: None,
            self_state_text: None,
            self_authored_core_text: None,
            relationship_portfolio_text: None,
            relationship_constitution_text: None,
            persona_priority_text: None,
            self_model_text: None,
            autonomy_strategy_text: None,
            outer_voice_text: None,
            inner_life_text: None,
            self_continuity_text: None,
            private_workspace_text: None,
            private_garden_text: None,
            mental_privacy_adjudication_text: None,
            mental_privacy_text: None,
            long_term_memory_text: None,
            archive_evidence_text: None,
            runtime_skill_text: None,
            capability_package_text: Some("## Capability Package\ncamera enabled"),
            summary_text: None,
            recent_messages: None,
            runtime: None,
            include_daily_notes: false,
            llm_hint: "",
        })
        .expect("context");

        assert!(system.contains("## Governed Memory Evidence"));
        assert!(!system.contains("## Capability Package"));
    }

    #[test]
    fn build_context_esp_user_turn_does_not_materialize_background_governance_from_components() {
        let msg = PcMsg::new_inbound("qq_channel", "chat-1", "继续", false).expect("pcmsg");
        let memory = StubMemoryStore {
            soul: "SOUL".to_string(),
            user: "USER".to_string(),
            memory: "MEMORY".to_string(),
            daily_notes: Vec::new(),
        };
        let session = StubSessionStore;
        let important = StubImportantMessageStore::default();

        let (system, _) = build_context(&ContextParams {
            msg: &msg,
            memory_system_kind: crate::memory::MemorySystemKind::EspCompact,
            memory: &memory,
            session: &session,
            important_message_store: &important,
            has_tools: false,
            skill_descriptions: "",
            system_max_len: 1600,
            messages_max_len: 256,
            session_max_messages: 8,
            group_activation: "always",
            emotion_signal_suffix: None,
            constitutional_stack_text: Some("## Self-Authored Core\nIdentity anchor: board beetle"),
            subject_state_text: None,
            deliberation_gate_text: None,
            active_task_context_text: Some("## Active Task Context\nResume the current task."),
            governed_memory_evidence_text: Some(
                "## Governed Memory Evidence\nCompressed long-term memory.",
            ),
            background_governance_text: None,
            execution_state_text: None,
            task_workspace_text: None,
            task_recall_text: None,
            world_snapshot_text: Some("world snapshot"),
            world_sense_text: Some("world sense"),
            self_state_text: Some("self state"),
            self_authored_core_text: None,
            relationship_portfolio_text: Some("relationship portfolio"),
            relationship_constitution_text: None,
            persona_priority_text: None,
            self_model_text: Some("self model"),
            autonomy_strategy_text: Some("autonomy strategy"),
            outer_voice_text: Some("outer voice"),
            inner_life_text: Some("inner life"),
            self_continuity_text: Some("self continuity"),
            private_workspace_text: Some("private workspace"),
            private_garden_text: Some("private garden"),
            mental_privacy_adjudication_text: None,
            mental_privacy_text: Some("mental privacy"),
            long_term_memory_text: None,
            archive_evidence_text: None,
            runtime_skill_text: None,
            capability_package_text: None,
            summary_text: None,
            recent_messages: None,
            runtime: None,
            include_daily_notes: false,
            llm_hint: "",
        })
        .expect("context");

        assert!(system.contains("## Governed Memory Evidence"));
        assert!(!system.contains("## Background Governance"));
        assert!(!system.contains("self model"));
        assert!(!system.contains("private garden"));
    }

    #[test]
    fn build_context_linux_user_turn_keeps_capability_package_when_present() {
        let msg = PcMsg::new_inbound("qq_channel", "chat-1", "继续", false).expect("pcmsg");
        let memory = StubMemoryStore {
            soul: "SOUL".to_string(),
            user: "USER".to_string(),
            memory: "MEMORY".to_string(),
            daily_notes: Vec::new(),
        };
        let session = StubSessionStore;
        let important = StubImportantMessageStore::default();

        let (system, _) = build_context(&ContextParams {
            msg: &msg,
            memory_system_kind: crate::memory::MemorySystemKind::LinuxFull,
            memory: &memory,
            session: &session,
            important_message_store: &important,
            has_tools: false,
            skill_descriptions: "",
            system_max_len: 1400,
            messages_max_len: 256,
            session_max_messages: 8,
            group_activation: "always",
            emotion_signal_suffix: None,
            constitutional_stack_text: Some("## Self-Authored Core\nIdentity anchor: board beetle"),
            subject_state_text: None,
            deliberation_gate_text: None,
            active_task_context_text: Some("## Active Task Context\nResume the current task."),
            governed_memory_evidence_text: Some(
                "## Governed Memory Evidence\nCompressed long-term memory.",
            ),
            background_governance_text: None,
            execution_state_text: None,
            task_workspace_text: None,
            task_recall_text: None,
            world_snapshot_text: None,
            world_sense_text: None,
            self_state_text: None,
            self_authored_core_text: None,
            relationship_portfolio_text: None,
            relationship_constitution_text: None,
            persona_priority_text: None,
            self_model_text: None,
            autonomy_strategy_text: None,
            outer_voice_text: None,
            inner_life_text: None,
            self_continuity_text: None,
            private_workspace_text: None,
            private_garden_text: None,
            mental_privacy_adjudication_text: None,
            mental_privacy_text: None,
            long_term_memory_text: None,
            archive_evidence_text: None,
            runtime_skill_text: None,
            capability_package_text: Some("## Capability Package\ncamera enabled"),
            summary_text: None,
            recent_messages: None,
            runtime: None,
            include_daily_notes: false,
            llm_hint: "",
        })
        .expect("context");

        assert!(system.contains("## Capability Package"));
    }
}
