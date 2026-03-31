//! Agent ReAct 循环：入站一条 → context → chat（含 tool_use 多轮）→ 会话持久化 → 出站一条。
//! 仅依赖 trait；HTTP/Tool 由 main 注入同一实现（如 EspHttpClient）。
use super::parse_tools::{append_tool_fallback_instructions, recover_text_tool_calls};
use super::strategy::{append_execution_plan, should_generate_execution_plan, AgentRunStrategy};
use crate::agent::context::{build_context, RuntimeContext};
use crate::bus::{
    InboundRx, IngressKind, OutboundTx, PcMsg, SystemInboundTx, UserInboundRx, UserInboundTx,
    MAX_CONTENT_LEN,
};
use crate::constants::{
    AGENT_MARKER_MARK_IMPORTANT, AGENT_MARKER_SIGNAL_COMFORT, AGENT_MARKER_STOP,
    AGENT_RETRY_BASE_MS, AGENT_RETRY_MAX_MS, INBOUND_RECV_TIMEOUT_SECS, MAX_DEFER_RETRIES,
    MAX_TOOL_RESULTS_USER_MESSAGE_LEN, SESSION_SUMMARY_MAX_LEN,
    TASK_CONTINUATION_CONTINUE_THRESHOLD_LEN,
};
use crate::error::Result;
use crate::i18n::{tr, Locale as UiLocale, Message as UiMessage};
use crate::llm::{LlmClient, Message, StopReason, ToolChoicePolicy};
use crate::memory::{
    EmotionSignalStore, ImportantMessageStore, MemoryStore, PendingRetryStore, SessionMessage,
    SessionStore, SessionSummaryStore, TaskContinuationStore,
};
use crate::metrics;
use crate::orchestrator::admission::{AdmissionDecision, LlmDecision, ToolDecision};
use crate::state;
use crate::tools::http_bridge::HttpClientToolContext;
use crate::tools::ToolPolicyContext;
use crate::util::{
    remove_substrings_all_trim, strip_agent_stop_confirmation, truncate_content_to_max,
};
use crate::PlatformHttpClient;
use std::borrow::Cow;
use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::fmt::Write as _;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::RecvTimeoutError;
use std::sync::Arc;
use std::time::{Duration, Instant};
/// 最大 ReAct 轮数（含首轮 chat），防止无限 tool 循环。
const MAX_REACT_ROUNDS: usize = 10;

/// 工具结果 user 消息前缀；与 `compact_early_tool_rounds` / 摘要逻辑一致。
const TOOL_RESULTS_PREFIX: &str = "Tool results:\n";

/// ReAct 轮间保留完整内容的最近轮数（每轮 assistant + user 各 1 条 = 4 条）。
const REACT_FULL_ROUNDS_KEPT: usize = 2;
const REACT_FULL_MSGS_KEPT: usize = REACT_FULL_ROUNDS_KEPT * 2;

/// 早期轮次工具结果摘要：每条结果保留的首行预览字符数（UTF-8 安全截断）。
const TOOL_RESULT_PREVIEW_CHARS: usize = 80;
const TOOL_REPEAT_NOTE_2: &str =
    "[NOTE: identical tool call #2 - check the result above. If it's the same as before or didn't help, this approach isn't working. Try a completely different strategy.]\n";
const TOOL_REPEAT_NOTE_3: &str =
    "[NOTE: identical tool call #3 - you've tried this exact call multiple times. The repeated results show this method won't work. Either try a fundamentally different approach or explain the blocker to the user.]\n";
const TOOL_REPEAT_NOTE_MANY: &str =
    "[NOTE: identical tool call (repeated many times) - this is clearly not working. Stop repeating the same call. Either find a completely different solution or honestly explain to the user why you're stuck.]\n";
const PLAN_SYSTEM_SUFFIX: &str = "\n\n## Internal planning\nBefore solving the latest user request, create a short internal execution plan. Return 3-6 concise numbered steps only. Do not answer the user. Do not call tools in this planning step.";
const REFLECTION_NOTE: &str = "\n\n[SYSTEM] The last tool round did not produce a useful result. Reassess the plan, avoid repeating the same call, and either switch strategy or explain the blocker clearly.";

/// 程序性会话摘要：单次轻量 LLM 调用的 system 提示。
const SUMMARY_SYSTEM: &str = "You are a conversation summarizer. Compress the following conversation into a concise summary (max 800 chars) preserving key facts, user preferences and pending tasks. Reply with the summary only.";
const SESSION_SUMMARY_MIN_MESSAGES: usize = 20;
const SESSION_SUMMARY_REFRESH_DELTA: usize = 10;

/// 同一 chat_id 的 "low memory, defer" 日志最少间隔，避免刷屏。
const LOW_MEM_DEFER_LOG_INTERVAL: Duration = Duration::from_secs(60);
static REQ_SEQ: AtomicU32 = AtomicU32::new(1);

fn next_req_id(channel: &str, chat_id: &str) -> String {
    let seq = REQ_SEQ.fetch_add(1, Ordering::Relaxed);
    let ts_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let mut hasher = DefaultHasher::new();
    channel.hash(&mut hasher);
    chat_id.hash(&mut hasher);
    let short = (hasher.finish() & 0xffff) as u16;
    let mut s = String::with_capacity(40);
    let _ = write!(&mut s, "r{}-{}-{:04x}", ts_ms, seq, short);
    s
}

fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis().min(u64::MAX as u128) as u64)
        .unwrap_or(0)
}

fn should_refresh_session_summary(current_count: usize, last_summary_count: usize) -> bool {
    current_count >= SESSION_SUMMARY_MIN_MESSAGES
        && current_count.saturating_sub(last_summary_count) >= SESSION_SUMMARY_REFRESH_DELTA
}

fn fallback_session_summary(recent: &[SessionMessage]) -> String {
    use std::fmt::Write;

    let start = recent.len().saturating_sub(5);
    let mut fallback = String::with_capacity(640);
    for (idx, message) in recent[start..].iter().enumerate() {
        if idx > 0 {
            fallback.push_str(" | ");
        }
        let _ = write!(
            fallback,
            "{}: {}",
            message.role,
            truncate_content_to_max(&message.content, 100).as_ref()
        );
    }
    truncate_content_to_max(&fallback, SESSION_SUMMARY_MAX_LEN).into_owned()
}

fn choose_inbound_tx<'a>(
    ingress: IngressKind,
    user_inbound_tx: &'a UserInboundTx,
    system_inbound_tx: &'a SystemInboundTx,
) -> &'a crate::bus::InboundTx {
    match ingress {
        IngressKind::User => user_inbound_tx,
        IngressKind::System => system_inbound_tx,
    }
}

#[derive(Clone, Copy)]
enum AgentWorkerLane {
    User,
    System,
}

impl AgentWorkerLane {
    fn as_str(self) -> &'static str {
        match self {
            AgentWorkerLane::User => "user",
            AgentWorkerLane::System => "system",
        }
    }
}

#[derive(Default)]
struct WorkerLatency {
    context_ms: u128,
    llm_round_total_ms: u128,
    tool_exec_ms: u128,
    session_write_ms: u128,
    ttft_ms: Option<u128>,
    react_rounds: u32,
    tool_calls: u32,
}

/// 将文本按 UTF-8 边界追加到 dst，确保总字节不超过 max_bytes。
/// 返回 true 表示本次发生截断（达到上限）。
fn push_bounded_utf8(dst: &mut String, text: &str, max_bytes: usize) -> bool {
    if dst.len() >= max_bytes {
        return true;
    }
    let remain = max_bytes - dst.len();
    if text.len() <= remain {
        dst.push_str(text);
        return false;
    }
    let mut end = 0usize;
    for (i, ch) in text.char_indices() {
        let next = i + ch.len_utf8();
        if next > remain {
            break;
        }
        end = next;
    }
    if end > 0 {
        dst.push_str(&text[..end]);
    }
    true
}

/// 对 (tool_name, args) 做稳定哈希，用于重复工具调用检测。
fn hash_tool_call(name: &str, args: &str) -> u64 {
    let mut h = DefaultHasher::new();
    name.hash(&mut h);
    0x9e37_79b9_7f4a_7c15u64.hash(&mut h);
    args.hash(&mut h);
    h.finish()
}

/// 将早期轮次的工具结果压缩为「预览 + 总字节数」摘要，保留语义锚点、不丢轮次结构。
fn summarize_tool_results(content: &str) -> String {
    let body = content.strip_prefix(TOOL_RESULTS_PREFIX).unwrap_or(content);
    let mut out = String::with_capacity(512);
    out.push_str("Tool results (prior round):\n");
    let mut wrote_any = false;
    let mut lines = body.lines().peekable();
    while let Some(line) = lines.next() {
        if let Some(idx) = line.find("]: ") {
            let id_part = &line[..idx + 3];
            let first_val = &line[idx + 3..];
            let mut preview = String::with_capacity(TOOL_RESULT_PREVIEW_CHARS + 16);
            let mut preview_chars = 0usize;
            let mut preview_truncated =
                !push_preview_chars(&mut preview, first_val, &mut preview_chars);
            let mut total_bytes = first_val.len();
            while let Some(next) = lines.peek().copied() {
                if next.contains("]: ") && next.starts_with('[') {
                    break;
                }
                total_bytes = total_bytes.saturating_add(1).saturating_add(next.len());
                if preview_chars < TOOL_RESULT_PREVIEW_CHARS {
                    preview_truncated |=
                        !push_preview_chars(&mut preview, "\n", &mut preview_chars);
                    preview_truncated |=
                        !push_preview_chars(&mut preview, next, &mut preview_chars);
                } else {
                    preview_truncated = true;
                }
                let _ = lines.next();
            }
            if preview_truncated {
                let _ = writeln!(out, "{}{}…[{} bytes total]", id_part, preview, total_bytes);
            } else {
                let _ = writeln!(out, "{}{}", id_part, preview);
            }
            wrote_any = true;
        }
    }
    if !wrote_any {
        let _ = writeln!(out, "[{} bytes total, format not parsed]", body.len());
    }
    out
}

fn push_preview_chars(dst: &mut String, text: &str, used_chars: &mut usize) -> bool {
    if *used_chars >= TOOL_RESULT_PREVIEW_CHARS {
        return false;
    }
    let remain = TOOL_RESULT_PREVIEW_CHARS - *used_chars;
    let mut chars = 0usize;
    if text.chars().count() > remain {
        let mut end = 0usize;
        for (idx, ch) in text.char_indices().take(remain) {
            end = idx + ch.len_utf8();
            chars += 1;
        }
        dst.push_str(&text[..end]);
        *used_chars += chars;
        false
    } else {
        dst.push_str(text);
        *used_chars += text.chars().count();
        true
    }
}

/// 滑动窗口：保留最近 `REACT_FULL_MSGS_KEPT` 条 ReAct 追加消息完整，更早的 assistant / tool 结果做机械摘要。
fn compact_early_tool_rounds(messages: &mut [Message], initial_count: usize) {
    let react_start = initial_count;
    let react_end = messages.len();
    let react_count = react_end.saturating_sub(react_start);
    if react_count <= REACT_FULL_MSGS_KEPT {
        return;
    }
    let compact_end = react_end - REACT_FULL_MSGS_KEPT;
    for msg in messages[react_start..compact_end].iter_mut() {
        if msg.content.starts_with(TOOL_RESULTS_PREFIX) && msg.content.len() > 128 {
            let s = summarize_tool_results(&msg.content);
            msg.content = s;
        } else if msg.role == "assistant" && msg.content.len() > 200 {
            let mut end = 150;
            while end > 0 && !msg.content.is_char_boundary(end) {
                end -= 1;
            }
            msg.content.truncate(end);
            msg.content.push_str("…[compressed]");
        }
    }
}

fn try_send_outbound(outbound_tx: &OutboundTx, msg: PcMsg, log_prefix: &str) -> bool {
    match outbound_tx.try_send(msg) {
        Ok(()) => {
            metrics::record_message_out();
            true
        }
        Err(e) => {
            metrics::record_outbound_enqueue_fail();
            log::error!("[agent] {} outbound enqueue failed: {}", log_prefix, e);
            false
        }
    }
}

enum GateResult {
    Proceed,
    Skipped,
}

#[allow(clippy::too_many_arguments)]
fn handle_llm_gate(
    msg: &PcMsg,
    req_id: &str,
    loc: UiLocale,
    user_inbound_tx: &UserInboundTx,
    system_inbound_tx: &SystemInboundTx,
    outbound_tx: &OutboundTx,
    config: &AgentLoopConfig,
) -> GateResult {
    crate::orchestrator::refresh_heap_if_stale();
    match crate::orchestrator::can_call_llm_pub() {
        LlmDecision::Proceed => GateResult::Proceed,
        LlmDecision::RetryLater { delay_ms } => {
            let mut retry_msg = msg.clone();
            retry_msg.enqueue_ts_ms = now_unix_ms();
            let inbound_tx =
                choose_inbound_tx(retry_msg.ingress, user_inbound_tx, system_inbound_tx);
            match inbound_tx.try_send(retry_msg) {
                Ok(()) => {}
                Err(std::sync::mpsc::TrySendError::Full(m)) => {
                    let _ = config.pending_retry.save_pending_retry(&m);
                    let suffix = if msg.ingress == IngressKind::System {
                        "(system)"
                    } else {
                        ""
                    };
                    log::warn!(
                        "[agent] llm retry-later{}: inbound full, pending_retry saved chat_id={}",
                        suffix,
                        m.chat_id
                    );
                }
                Err(std::sync::mpsc::TrySendError::Disconnected(_)) => {
                    let suffix = if msg.ingress == IngressKind::System {
                        "(system)"
                    } else {
                        ""
                    };
                    log::error!(
                        "[agent] inbound_tx disconnected during retry-later{}",
                        suffix
                    );
                }
            }
            std::thread::sleep(Duration::from_millis(delay_ms));
            crate::platform::task_wdt::feed_current_task();
            GateResult::Skipped
        }
        LlmDecision::Degrade { reason } => {
            if msg.ingress == IngressKind::System {
                log::info!("[agent] system task degraded, retry later: {}", reason);
                let mut retry_msg = msg.clone();
                retry_msg.enqueue_ts_ms = now_unix_ms();
                let inbound_tx =
                    choose_inbound_tx(retry_msg.ingress, user_inbound_tx, system_inbound_tx);
                if let Err(std::sync::mpsc::TrySendError::Full(m)) = inbound_tx.try_send(retry_msg)
                {
                    let _ = config.pending_retry.save_pending_retry(&m);
                }
            } else {
                log::info!("[agent] LLM degraded: {}", reason);
                let out = PcMsg {
                    channel: msg.channel.clone(),
                    chat_id: msg.chat_id.clone(),
                    content: tr(UiMessage::LowMemoryUserDefer, loc),
                    req_id: Some(req_id.to_owned()),
                    ingress: IngressKind::User,
                    enqueue_ts_ms: now_unix_ms(),
                    is_group: false,
                };
                let _ = try_send_outbound(outbound_tx, out, "llm-degrade");
            }
            GateResult::Skipped
        }
    }
}

/// 程序性触发：用最近会话生成摘要并 `set_with_count`。LLM 失败时确定性回退，仍落盘。
fn generate_session_summary(
    http: &mut dyn PlatformHttpClient,
    llm: &(dyn LlmClient + Send + Sync),
    config: &AgentLoopConfig,
    chat_id: &str,
    current_count: usize,
) -> Result<()> {
    use std::fmt::Write;

    let recent = config.session_store.load_recent(chat_id, 20)?;
    let fallback = fallback_session_summary(&recent);
    let mut transcript = String::with_capacity(2048);
    for m in &recent {
        let preview = truncate_content_to_max(&m.content, 200);
        let _ = writeln!(
            transcript,
            "{}: {}",
            m.role.to_uppercase(),
            preview.as_ref()
        );
    }
    let user_msg = Message {
        role: Cow::Borrowed("user"),
        content: transcript,
    };
    let messages = [user_msg];
    let loc = (config.resolve_locale)();
    let mut ctx = HttpClientToolContext {
        http,
        chat_id: Some(Arc::from(chat_id)),
        channel: Some(Arc::from("system")),
        locale: loc,
    };
    match llm.chat(
        &mut ctx,
        SUMMARY_SYSTEM,
        &messages,
        None,
        ToolChoicePolicy::Auto,
    ) {
        Ok(resp) => {
            let summary =
                truncate_content_to_max(resp.content.trim(), SESSION_SUMMARY_MAX_LEN).into_owned();
            let summary = if summary.is_empty() {
                fallback
            } else {
                summary
            };
            config
                .session_summary_store
                .set_with_count(chat_id, &summary, current_count)?;
            Ok(())
        }
        Err(e) => {
            log::warn!(
                "[agent_summary] LLM summary failed for chat_id={}: {}",
                chat_id,
                e
            );
            config
                .session_summary_store
                .set_with_count(chat_id, &fallback, current_count)?;
            Ok(())
        }
    }
}

/// run_worker_path 返回：正常内容或用户要求停止时的确认文案。
pub enum WorkerOutcome {
    Content(String),
    Interrupt(String),
}

/// 流式编辑器：LLM 流式输出期间，发送占位消息并逐步编辑内容。
/// 实现方内部自行创建/管理 HTTP 连接，不占用 agent 的 LLM HTTP 连接。
pub trait StreamEditor {
    /// 发送初始占位消息，返回 message_id（用于后续编辑）。
    fn send_initial(&self, chat_id: &str, content: &str) -> Result<Option<String>>;
    /// 编辑已发送的消息。
    fn edit(&self, chat_id: &str, message_id: &str, content: &str) -> Result<()>;
}

/// 单轮进度指标，用于检测 agent 是否陷入无效循环。
#[derive(Clone, Copy)]
struct RoundProgress {
    /// 本轮是否产生新信息（工具成功或内容长度显著增加）
    new_info: bool,
}

/// Agent 循环的存储与运行参数，由 main 构建并传入 run_agent_loop，减少参数数量。
pub struct AgentLoopConfig {
    pub memory_store: Arc<dyn MemoryStore + Send + Sync>,
    pub session_store: Arc<dyn SessionStore + Send + Sync>,
    pub session_summary_store: Arc<dyn SessionSummaryStore + Send + Sync>,
    pub get_skill_descriptions: Arc<dyn Fn() -> String + Send + Sync>,
    pub session_max_messages: usize,
    pub tg_group_activation: Arc<str>,
    pub task_continuation: Arc<dyn TaskContinuationStore + Send + Sync>,
    pub task_continuation_max_rounds: u32,
    pub important_message_store: Arc<dyn ImportantMessageStore + Send + Sync>,
    pub emotion_signal_store: Arc<dyn EmotionSignalStore + Send + Sync>,
    pub pending_retry: Arc<dyn PendingRetryStore + Send + Sync>,
    pub strategy: AgentRunStrategy,
    /// 全局 LLM 流式模式；true 时 agent 使用 chat_with_progress 回调。
    pub llm_stream: bool,
    /// 流式编辑器；llm_stream 开且通道支持编辑时由 main 传入。
    pub stream_editor: Option<Arc<dyn StreamEditor + Send + Sync>>,
    /// 流式编辑器对应的通道名；仅当前消息来自该通道时才允许流式编辑。
    pub stream_editor_channel: Option<Arc<str>>,
    /// 当前 NVS 语言；工具与降级文案按此本地化。
    pub resolve_locale: std::sync::Arc<dyn Fn() -> UiLocale + Send + Sync>,
}

pub type TypingNotifier = Box<dyn FnMut(&str, &str, &mut dyn PlatformHttpClient) + Send>;

/// User worker：只消费 user inbound 队列。
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn run_user_agent_loop(
    http: &mut dyn PlatformHttpClient,
    worker_llm: &(dyn LlmClient + Send + Sync),
    registry: &crate::tools::ToolRegistry,
    config: &AgentLoopConfig,
    user_inbound_tx: UserInboundTx,
    user_inbound_rx: UserInboundRx,
    system_inbound_tx: SystemInboundTx,
    outbound_tx: OutboundTx,
    typing_notifier: Option<TypingNotifier>,
) -> Result<()> {
    run_agent_loop_lane(
        http,
        worker_llm,
        registry,
        config,
        user_inbound_tx,
        user_inbound_rx,
        system_inbound_tx,
        outbound_tx,
        typing_notifier,
        AgentWorkerLane::User,
        true,
    )
}

/// System worker：只消费 system inbound 队列。
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn run_system_agent_loop(
    http: &mut dyn PlatformHttpClient,
    worker_llm: &(dyn LlmClient + Send + Sync),
    registry: &crate::tools::ToolRegistry,
    config: &AgentLoopConfig,
    user_inbound_tx: UserInboundTx,
    system_inbound_tx: SystemInboundTx,
    system_inbound_rx: InboundRx,
    outbound_tx: OutboundTx,
) -> Result<()> {
    run_agent_loop_lane(
        http,
        worker_llm,
        registry,
        config,
        user_inbound_tx,
        system_inbound_rx,
        system_inbound_tx,
        outbound_tx,
        None,
        AgentWorkerLane::System,
        false,
    )
}

/// 单队列 worker 主循环：由 user/system 两个入口复用同一处理逻辑。
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn run_agent_loop_lane(
    http: &mut dyn PlatformHttpClient,
    worker_llm: &(dyn LlmClient + Send + Sync),
    registry: &crate::tools::ToolRegistry,
    config: &AgentLoopConfig,
    user_inbound_tx: UserInboundTx,
    inbound_rx: InboundRx,
    system_inbound_tx: SystemInboundTx,
    outbound_tx: OutboundTx,
    mut typing_notifier: Option<TypingNotifier>,
    worker_lane: AgentWorkerLane,
    bootstrap_pending_retry: bool,
) -> Result<()> {
    let worker_lane_tag = worker_lane.as_str();

    // Track repeated LLM failure for same request body, avoid infinite retry.
    // Key: u64 hash of (channel, chat_id, content) — avoids per-message format! String alloc.
    // Value: (failure count, last failure time) — entries expire after 5 minutes.
    let mut llm_failure_count: HashMap<u64, (u8, Instant)> = HashMap::new();
    // Reuse per-request tool repeat map to reduce heap churn.
    let mut tool_call_repeat_buf: HashMap<u64, u8> = HashMap::with_capacity(16);
    // Track consecutive defer count per message key to break infinite defer loops.
    let mut defer_tracker: HashMap<u64, (u8, Instant)> = HashMap::new();
    // Throttle "low memory, defer" log per chat_id to avoid log spam.
    let mut low_mem_defer_log: Option<(Arc<str>, Instant)> = None;
    // Periodic GC for llm_failure_count + defer_tracker: evict expired entries every N messages.
    let mut msg_since_gc: u16 = 0;
    const GC_INTERVAL_MSGS: u16 = 50;
    const FAILURE_EXPIRY: Duration = Duration::from_secs(300);
    const DEFER_EXPIRY: Duration = Duration::from_secs(300);
    const LATENCY_WARN_MS: u128 = 3000;

    if bootstrap_pending_retry {
        if let Ok(Some(m)) = config.pending_retry.load_pending_retry() {
            let _ = config.pending_retry.clear_pending_retry();
            let inbound_tx = choose_inbound_tx(m.ingress, &user_inbound_tx, &system_inbound_tx);
            let _ = inbound_tx.send(m);
        }
    }

    let recv_timeout = Duration::from_secs(INBOUND_RECV_TIMEOUT_SECS);
    loop {
        let mut msg = match inbound_rx.recv_timeout(recv_timeout) {
            Ok(m) => m,
            Err(RecvTimeoutError::Timeout) => {
                crate::platform::task_wdt::feed_current_task();
                metrics::record_wdt_feed();
                continue;
            }
            Err(RecvTimeoutError::Disconnected) => break,
        };
        metrics::record_message_in();
        crate::platform::task_wdt::feed_current_task();
        let loc = (config.resolve_locale)();
        let msg_start = Instant::now();
        if msg.req_id.is_none() {
            msg.req_id = Some(next_req_id(&msg.channel, &msg.chat_id));
        }
        let req_id = msg.req_id.clone().unwrap_or_default();
        let queue_wait_ms = now_unix_ms().saturating_sub(msg.enqueue_ts_ms) as u128;
        if msg.ingress == IngressKind::System {
            metrics::record_system_queue_wait_ms(queue_wait_ms);
        } else {
            metrics::record_user_queue_wait_ms(queue_wait_ms);
        }

        // Periodic GC: evict expired failure/defer entries to prevent unbounded growth.
        msg_since_gc += 1;
        if msg_since_gc >= GC_INTERVAL_MSGS
            || llm_failure_count.len() > 64
            || defer_tracker.len() > 64
        {
            msg_since_gc = 0;
            let now_gc = Instant::now();
            llm_failure_count.retain(|_, (_, ts)| now_gc.duration_since(*ts) < FAILURE_EXPIRY);
            defer_tracker.retain(|_, (_, ts)| now_gc.duration_since(*ts) < DEFER_EXPIRY);
        }

        let msg_key = {
            let mut hasher = DefaultHasher::new();
            msg.channel.hash(&mut hasher);
            msg.chat_id.hash(&mut hasher);
            msg.content.hash(&mut hasher);
            hasher.finish()
        };
        let now_for_key = Instant::now();
        if llm_failure_count
            .get(&msg_key)
            .map(|(count, ts)| *count >= 3 && now_for_key.duration_since(*ts) < FAILURE_EXPIRY)
            .unwrap_or(false)
        {
            let out = PcMsg {
                channel: msg.channel.clone(),
                chat_id: msg.chat_id.clone(),
                content: tr(UiMessage::NodeMaintenance, loc),
                req_id: Some(req_id.clone()),
                ingress: IngressKind::User,
                enqueue_ts_ms: now_unix_ms(),
                is_group: false,
            };
            let _ = try_send_outbound(&outbound_tx, out, "maintenance");
            continue;
        }

        // Refresh heap state if stale before admission check.
        crate::orchestrator::refresh_heap_if_stale();
        match crate::orchestrator::should_accept_inbound_pub(&msg.channel, &msg.chat_id) {
            AdmissionDecision::Accept => {}
            AdmissionDecision::Defer { delay_ms } => {
                // Check defer count for this message; drop after MAX_DEFER_RETRIES.
                let entry = defer_tracker.entry(msg_key).or_insert((0, Instant::now()));
                entry.0 = entry.0.saturating_add(1);
                entry.1 = Instant::now();
                let defer_count = entry.0;

                if defer_count >= MAX_DEFER_RETRIES {
                    log::warn!(
                        "[agent] defer limit reached ({}) for chat_id={}, dropping message",
                        MAX_DEFER_RETRIES,
                        msg.chat_id
                    );
                    defer_tracker.remove(&msg_key);
                    if msg.ingress == IngressKind::User {
                        let defer_out = PcMsg {
                            channel: msg.channel.clone(),
                            chat_id: msg.chat_id.clone(),
                            content: tr(UiMessage::LowMemoryUserDefer, loc),
                            req_id: Some(req_id.clone()),
                            ingress: IngressKind::User,
                            enqueue_ts_ms: now_unix_ms(),
                            is_group: false,
                        };
                        let _ = try_send_outbound(&outbound_tx, defer_out, "defer-limit");
                    }
                    continue;
                }

                if msg.ingress == IngressKind::User {
                    let defer_out = PcMsg {
                        channel: msg.channel.clone(),
                        chat_id: msg.chat_id.clone(),
                        content: tr(UiMessage::LowMemoryUserDefer, loc),
                        req_id: Some(req_id.clone()),
                        ingress: IngressKind::User,
                        enqueue_ts_ms: now_unix_ms(),
                        is_group: false,
                    };
                    let _ = try_send_outbound(&outbound_tx, defer_out, "defer");
                }
                let chat_id = msg.chat_id.clone();
                msg.enqueue_ts_ms = now_unix_ms();
                let inbound_tx =
                    choose_inbound_tx(msg.ingress, &user_inbound_tx, &system_inbound_tx);
                match inbound_tx.try_send(msg) {
                    Ok(()) => {
                        let now = Instant::now();
                        let should_log = low_mem_defer_log
                            .as_ref()
                            .map(|(id, t)| {
                                id.as_ref() != chat_id.as_ref()
                                    || t.elapsed() >= LOW_MEM_DEFER_LOG_INTERVAL
                            })
                            .unwrap_or(true);
                        if should_log {
                            log::warn!("[agent] admission defer chat_id={}", chat_id);
                            low_mem_defer_log = Some((chat_id.clone(), now));
                        }
                    }
                    Err(std::sync::mpsc::TrySendError::Full(m)) => {
                        let _ = config.pending_retry.save_pending_retry(&m);
                        log::warn!(
                            "[agent] admission defer, pending_retry saved chat_id={}",
                            m.chat_id
                        );
                    }
                    Err(std::sync::mpsc::TrySendError::Disconnected(_)) => {
                        log::error!("[agent] inbound_tx disconnected");
                    }
                }
                std::thread::sleep(Duration::from_millis(delay_ms));
                crate::platform::task_wdt::feed_current_task();
                metrics::record_wdt_feed();
                continue;
            }
            AdmissionDecision::Reject { reason } => {
                let now = Instant::now();
                let should_log = low_mem_defer_log
                    .as_ref()
                    .map(|(id, t)| {
                        id.as_ref() != reason || t.elapsed() >= LOW_MEM_DEFER_LOG_INTERVAL
                    })
                    .unwrap_or(true);
                if should_log {
                    log::warn!("[agent] inbound rejected: {}", reason);
                    low_mem_defer_log = Some((Arc::from(reason), now));
                }
                continue;
            }
        }
        let admission_ms = msg_start.elapsed().as_millis();

        // LLM 门控先于任务槽位获取与 typing 提示，确保：
        // 1. RetryLater 睡眠期间 active_agent_tasks 不被错误计为 1；
        // 2. typing 仅在真正进入 LLM 路径时才发送，避免产生空响应。
        if matches!(
            handle_llm_gate(
                &msg,
                &req_id,
                loc,
                &user_inbound_tx,
                &system_inbound_tx,
                &outbound_tx,
                config,
            ),
            GateResult::Skipped
        ) {
            continue;
        }

        // Gate 通过后获取任务槽位：Guard Drop 时自动递减，覆盖整个任务生命周期（含工具调用、会话写入、回复发送）。
        // Acquire task slot only after gate passes; guard auto-decrements on drop.
        let _agent_task_guard = crate::orchestrator::begin_agent_task();
        if let Some(ref mut f) = typing_notifier {
            f(&msg.channel, &msg.chat_id, http);
        }
        let final_content = run_worker_path(
            http,
            worker_llm,
            &msg,
            registry,
            config,
            &mut tool_call_repeat_buf,
            loc,
        );

        let (outcome, consumed_round, streamed, mut worker_latency) = match final_content {
            Ok(ok) => ok,
            Err(e) => {
                let llm_ms = msg_start.elapsed().as_millis().saturating_sub(admission_ms);
                let total_ms = msg_start.elapsed().as_millis();
                crate::platform::task_wdt::feed_current_task();
                metrics::record_error_by_stage(e.stage());
                log::warn!("[agent:{}] chat loop failed: {}", worker_lane_tag, e);
                log::warn!(
                    "[latency][agent:{}] req_id={} channel={} chat_id={} admission_ms={} llm_ms={} total_ms={} status=llm_error",
                    worker_lane_tag,
                    req_id,
                    msg.channel,
                    msg.chat_id,
                    admission_ms,
                    llm_ms,
                    total_ms
                );
                state::set_last_error(&e);

                let is_conn = e.is_connect_error();
                let (counter, _) = llm_failure_count
                    .entry(msg_key)
                    .or_insert((0, Instant::now()));
                *counter = counter.saturating_add(1);

                if *counter < 3 && !is_conn {
                    let mut retry_msg = msg.clone();
                    retry_msg.enqueue_ts_ms = now_unix_ms();
                    let inbound_tx =
                        choose_inbound_tx(retry_msg.ingress, &user_inbound_tx, &system_inbound_tx);
                    match inbound_tx.try_send(retry_msg) {
                        Ok(()) => {}
                        Err(std::sync::mpsc::TrySendError::Full(_)) => {
                            let _ = config.pending_retry.save_pending_retry(&msg);
                            log::warn!(
                                "[agent] llm retry: inbound full, pending_retry saved chat_id={}",
                                msg.chat_id
                            );
                        }
                        Err(std::sync::mpsc::TrySendError::Disconnected(_)) => {
                            log::error!("[agent] inbound_tx disconnected during llm retry");
                        }
                    }
                    let delay_ms = (AGENT_RETRY_BASE_MS * (1 << (*counter as u64).min(4)))
                        .min(AGENT_RETRY_MAX_MS);
                    std::thread::sleep(std::time::Duration::from_millis(delay_ms));
                    continue;
                }

                let reply = PcMsg {
                    channel: msg.channel.clone(),
                    chat_id: msg.chat_id.clone(),
                    content: tr(UiMessage::NodeMaintenance, loc),
                    req_id: Some(req_id.clone()),
                    ingress: IngressKind::User,
                    enqueue_ts_ms: now_unix_ms(),
                    is_group: false,
                };
                let _ = try_send_outbound(&outbound_tx, reply, "chat-failure");
                continue;
            }
        };
        let (mut reply_content, is_interrupt) = match outcome {
            WorkerOutcome::Interrupt(confirm) => {
                let cow = truncate_content_to_max(&confirm, MAX_CONTENT_LEN);
                let s = if let std::borrow::Cow::Borrowed(_) = &cow {
                    confirm
                } else {
                    cow.into_owned()
                };
                (s, true)
            }
            WorkerOutcome::Content(s) => {
                let cow = truncate_content_to_max(&s, MAX_CONTENT_LEN);
                let s = if let std::borrow::Cow::Borrowed(_) = &cow {
                    s
                } else {
                    cow.into_owned()
                };
                (s, false)
            }
        };
        let mark_important = !is_interrupt && reply_content.contains(AGENT_MARKER_MARK_IMPORTANT);
        let signal_comfort = !is_interrupt && reply_content.contains(AGENT_MARKER_SIGNAL_COMFORT);
        if mark_important || signal_comfort {
            reply_content = remove_substrings_all_trim(
                &reply_content,
                &[AGENT_MARKER_MARK_IMPORTANT, AGENT_MARKER_SIGNAL_COMFORT],
            );
            if signal_comfort {
                let _ = config.emotion_signal_store.set(&msg.chat_id, "comfort");
            }
            reply_content = truncate_content_to_max(&reply_content, MAX_CONTENT_LEN).into_owned();
        }

        if !is_interrupt && config.task_continuation_max_rounds > 0 {
            match consumed_round {
                Some(round) => {
                    if round < config.task_continuation_max_rounds
                        && (reply_content.contains("[CONTINUE]")
                            || reply_content.len() > TASK_CONTINUATION_CONTINUE_THRESHOLD_LEN)
                    {
                        let _ = config.task_continuation.set_task_continuation(
                            &msg.chat_id,
                            round + 1,
                            &reply_content,
                        );
                    } else {
                        let _ = config
                            .task_continuation
                            .clear_task_continuation(&msg.chat_id);
                    }
                }
                None => {
                    let _ = config
                        .task_continuation
                        .clear_task_continuation(&msg.chat_id);
                }
            }
        }

        // SILENT 或 cron 空回复不写 session，直接跳过。
        if reply_content.trim() == "SILENT"
            || (msg.channel.as_ref() == "cron" && reply_content.is_empty())
        {
            llm_failure_count.remove(&msg_key);
            defer_tracker.remove(&msg_key);
            let total_ms = msg_start.elapsed().as_millis();
            metrics::record_e2e_ms(total_ms);
            if msg.ingress == IngressKind::System {
                let is_cron = msg.channel.as_ref() == "cron";
                metrics::record_system_message_done(is_cron);
                if is_cron {
                    let cron_e2e = now_unix_ms().saturating_sub(msg.enqueue_ts_ms) as u128;
                    metrics::record_cron_e2e_ms(cron_e2e);
                }
            } else {
                metrics::record_user_message_done();
            }
            continue;
        }

        let session_start = Instant::now();
        if let Err(e) = config
            .session_store
            .append(&msg.chat_id, "user", &msg.content)
        {
            log::warn!("[agent_session] append user failed: {}", e);
            metrics::record_error_by_stage("session_append");
        }
        worker_latency.session_write_ms = worker_latency
            .session_write_ms
            .saturating_add(session_start.elapsed().as_millis());
        llm_failure_count.remove(&msg_key);
        defer_tracker.remove(&msg_key);

        // 流式编辑已发送到通道时，跳过 outbound_tx 避免重复发送。
        let llm_ms = worker_latency
            .context_ms
            .saturating_add(worker_latency.llm_round_total_ms)
            .saturating_add(worker_latency.tool_exec_ms)
            .saturating_add(worker_latency.session_write_ms);
        let outbound_start = Instant::now();
        let delivered = if !streamed {
            let out = PcMsg {
                channel: msg.channel.clone(),
                chat_id: msg.chat_id.clone(),
                content: reply_content.clone(),
                req_id: Some(req_id.clone()),
                ingress: IngressKind::User,
                enqueue_ts_ms: now_unix_ms(),
                is_group: false,
            };
            crate::platform::task_wdt::feed_current_task();
            try_send_outbound(&outbound_tx, out, "reply")
        } else {
            metrics::record_message_out();
            crate::platform::task_wdt::feed_current_task();
            true
        };
        let outbound_enqueue_ms = outbound_start.elapsed().as_millis();

        if delivered {
            let session_assistant_start = Instant::now();
            if let Err(e) = config
                .session_store
                .append(&msg.chat_id, "assistant", &reply_content)
            {
                log::warn!("[agent_session] append assistant failed: {}", e);
                metrics::record_error_by_stage("session_append");
            }
            worker_latency.session_write_ms = worker_latency
                .session_write_ms
                .saturating_add(session_assistant_start.elapsed().as_millis());
            if mark_important {
                let _ = config
                    .important_message_store
                    .set_important_offset_from_end(&msg.chat_id, 1);
            }
        }

        // Programmatic session summary — only after the reply is visible to user or streamed successfully.
        if delivered {
            let after_count = config
                .session_store
                .message_count(&msg.chat_id)
                .unwrap_or(0);
            let last_summary_count = config
                .session_summary_store
                .get_with_count(&msg.chat_id)
                .ok()
                .flatten()
                .map(|(_, c)| c)
                .unwrap_or(0);
            if should_refresh_session_summary(after_count, last_summary_count) {
                match generate_session_summary(http, worker_llm, config, &msg.chat_id, after_count)
                {
                    Ok(()) => log::info!("[agent_summary] updated for {}", msg.chat_id),
                    Err(e) => log::warn!("[agent_summary] failed: {}", e),
                }
            }
        }
        let total_ms = msg_start.elapsed().as_millis();
        metrics::record_react_rounds(worker_latency.react_rounds);
        metrics::record_tool_calls_last(worker_latency.tool_calls);
        metrics::record_ttft_ms(worker_latency.ttft_ms.unwrap_or(0));
        metrics::record_e2e_ms(total_ms);
        if msg.ingress == IngressKind::System {
            let is_cron = msg.channel.as_ref() == "cron";
            metrics::record_system_message_done(is_cron);
            if is_cron {
                let cron_e2e = now_unix_ms().saturating_sub(msg.enqueue_ts_ms) as u128;
                metrics::record_cron_e2e_ms(cron_e2e);
            }
        } else {
            metrics::record_user_message_done();
        }
        if total_ms >= LATENCY_WARN_MS {
            log::warn!(
                "[latency][agent:{}] req_id={} channel={} chat_id={} admission_ms={} context_ms={} llm_round_total_ms={} tool_exec_ms={} session_write_ms={} llm_ms={} outbound_enqueue_ms={} total_ms={} react_rounds={} tool_calls={} ttft_ms={} streamed={} delivered={} level=slow",
                worker_lane_tag,
                req_id,
                msg.channel,
                msg.chat_id,
                admission_ms,
                worker_latency.context_ms,
                worker_latency.llm_round_total_ms,
                worker_latency.tool_exec_ms,
                worker_latency.session_write_ms,
                llm_ms,
                outbound_enqueue_ms,
                total_ms,
                worker_latency.react_rounds,
                worker_latency.tool_calls,
                worker_latency.ttft_ms.unwrap_or(0),
                streamed,
                delivered
            );
        } else {
            log::info!(
                "[latency][agent:{}] req_id={} channel={} chat_id={} admission_ms={} context_ms={} llm_round_total_ms={} tool_exec_ms={} session_write_ms={} llm_ms={} outbound_enqueue_ms={} total_ms={} react_rounds={} tool_calls={} ttft_ms={} streamed={} delivered={}",
                worker_lane_tag,
                req_id,
                msg.channel,
                msg.chat_id,
                admission_ms,
                worker_latency.context_ms,
                worker_latency.llm_round_total_ms,
                worker_latency.tool_exec_ms,
                worker_latency.session_write_ms,
                llm_ms,
                outbound_enqueue_ms,
                total_ms,
                worker_latency.react_rounds,
                worker_latency.tool_calls,
                worker_latency.ttft_ms.unwrap_or(0),
                streamed,
                delivered
            );
        }
    }
    Ok(())
}

/// 完整 context + worker LLM + ReAct 循环，返回 (WorkerOutcome, consumed_round, streamed, latency)。不写 session，由调用方写。
/// streamed=true 表示已通过流式编辑发送到通道，调用方应跳过 outbound_tx。
#[allow(clippy::too_many_arguments)]
fn run_worker_path(
    http: &mut dyn PlatformHttpClient,
    worker_llm: &(dyn LlmClient + Send + Sync),
    msg: &crate::bus::PcMsg,
    registry: &crate::tools::ToolRegistry,
    config: &AgentLoopConfig,
    tool_call_repeat: &mut HashMap<u64, u8>,
    loc: UiLocale,
) -> Result<(WorkerOutcome, Option<u32>, bool, WorkerLatency)> {
    let mut latency = WorkerLatency::default();
    let llm_tool_choice = ToolChoicePolicy::Auto;
    let tool_policy = ToolPolicyContext::new(msg.ingress, msg.channel.as_ref());
    let tool_specs = registry.tool_specs_for_llm(&tool_policy);
    let has_tools = !tool_specs.is_empty();
    let native_tool_calling = has_tools && worker_llm.supports_native_tools();
    let mut tool_ctx = HttpClientToolContext {
        http,
        chat_id: Some(msg.chat_id.clone()),
        channel: Some(msg.channel.clone()),
        locale: loc,
    };
    let (suffix, consumed_round) =
        match config.task_continuation.get_task_continuation(&msg.chat_id) {
            Ok(Some((r, out))) => {
                let _ = config
                    .task_continuation
                    .clear_task_continuation(&msg.chat_id);
                let mut s = String::with_capacity(out.len().saturating_add(48));
                let _ = write!(
                    &mut s,
                    "上一轮产出（第{}轮）：\n{}\n\n本轮请在此基础上继续。",
                    r, out
                );
                (Some(s), Some(r))
            }
            _ => (None, None),
        };
    let emotion_signal_suffix = config
        .emotion_signal_store
        .get_then_clear(&msg.chat_id)
        .ok()
        .flatten()
        .and_then(|s| {
            if s == "comfort" {
                Some("用户可能需安慰，回复时可适当照顾情绪。")
            } else {
                None
            }
        });
    let summary_with_count = config
        .session_summary_store
        .get_with_count(&msg.chat_id)
        .ok()
        .flatten();
    let summary_text = summary_with_count.as_ref().map(|(s, _)| s.as_str());
    let budget = crate::orchestrator::current_budget();
    let snapshot = crate::orchestrator::snapshot();
    let runtime = RuntimeContext {
        now_secs: crate::util::current_unix_secs(),
        platform: if cfg!(any(target_arch = "xtensa", target_arch = "riscv32")) {
            "ESP32-S3"
        } else {
            "Linux"
        },
        pressure: snapshot.pressure,
        active_agent_tasks: snapshot.active_agent_tasks,
        inbound_depth: snapshot.inbound_depth,
        outbound_depth: snapshot.outbound_depth,
        #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
        cpu_usage_percent: snapshot.cpu_usage_percent,
        #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
        load_average: snapshot.load_average,
        #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
        process_memory_kb: snapshot.process_memory_kb,
    };
    let context_start = Instant::now();
    let skill_descriptions = (config.get_skill_descriptions)();
    let (mut system, mut messages) = build_context(&super::ContextParams {
        msg,
        memory: config.memory_store.as_ref(),
        session: config.session_store.as_ref(),
        important_message_store: config.important_message_store.as_ref(),
        has_tools,
        native_tool_calling,
        skill_descriptions: &skill_descriptions,
        system_max_len: budget.system_prompt_max,
        messages_max_len: budget.messages_max,
        session_max_messages: config.session_max_messages,
        group_activation: config.tg_group_activation.as_ref(),
        system_continuation_suffix: suffix.as_deref(),
        emotion_signal_suffix,
        summary_text,
        runtime: Some(runtime),
        llm_hint: budget.llm_hint,
    })
    .map_err(|e| e.with_stage("agent_context"))?;
    latency.context_ms = context_start.elapsed().as_millis();
    if has_tools && !native_tool_calling {
        append_tool_fallback_instructions(&mut system, budget.system_prompt_max, &tool_specs);
    }
    if config.strategy.enables_preplanning()
        && should_generate_execution_plan(msg, has_tools, snapshot.pressure)
    {
        let planning_system = format!("{}{}", system, PLAN_SYSTEM_SUFFIX);
        match worker_llm.chat(
            &mut tool_ctx,
            &planning_system,
            &messages,
            None,
            ToolChoicePolicy::Auto,
        ) {
            Ok(resp) => append_execution_plan(&mut system, budget.system_prompt_max, &resp.content),
            Err(e) => {
                log::debug!("[agent_plan] skipped after planning error: {}", e);
            }
        }
    }

    // ReAct 追加消息起始下标；用于滑动窗口压缩早期轮次。
    let initial_msg_count = messages.len();
    // 跨请求复用容器，每次新请求清空；跨轮次仍保留本请求内状态。
    tool_call_repeat.clear();
    // 复用工具错误消息缓冲区，避免错误路径反复分配。
    let mut tool_error_buf = String::with_capacity(256);
    let mut final_content = String::with_capacity(4096);
    // 流式编辑状态（跨 ReAct 轮次共享）。
    let editor = if config.llm_stream
        && config.stream_editor_channel.as_deref() == Some(msg.channel.as_ref())
    {
        config.stream_editor.as_deref()
    } else {
        None
    };
    let mut stream_msg_id: Option<String> = None;
    let mut last_edit_time = Instant::now();
    let mut stream_edit_disabled = false; // send_initial 失败后禁用流式编辑
    let mut stream_edit_fail_count: u8 = 0; // edit 连续失败计数
    const EDIT_THROTTLE_MS: u64 = 500;
    const MAX_EDIT_FAILURES: u8 = 3;
    // P1 Enhancement 3: 进度跟踪（最近3轮），用于检测无效循环。
    let mut progress_history: [Option<RoundProgress>; 3] = [None; 3];
    let mut any_tool_used = false; // 本次请求是否使用过任何工具

    for round in 0..MAX_REACT_ROUNDS {
        latency.react_rounds = round as u32 + 1;
        // Inter-round pressure check: skip first round (already gated by caller).
        if round > 0 {
            crate::orchestrator::update_heap_state();
            match crate::orchestrator::can_call_llm_pub() {
                LlmDecision::Proceed => {}
                LlmDecision::RetryLater { .. } | LlmDecision::Degrade { .. } => {
                    if final_content.is_empty() {
                        final_content = tr(UiMessage::LowMemoryUserDefer, loc);
                    } else {
                        final_content.push_str("\n\n");
                        final_content.push_str(&tr(UiMessage::StreamLowMemoryOmitted, loc));
                    }
                    break;
                }
            }
        }
        if round >= 2 {
            compact_early_tool_rounds(&mut messages, initial_msg_count);
        }
        // P1 Enhancement 3: 检测连续3轮无进展，注入提示。
        if round >= 3
            && progress_history[0].is_some_and(|p| !p.new_info)
            && progress_history[1].is_some_and(|p| !p.new_info)
            && progress_history[2].is_some_and(|p| !p.new_info)
        {
            messages.push(Message {
                role: Cow::Borrowed("user"),
                content: "[SYSTEM] You've made no progress in the last 3 rounds. The current approach isn't working. Either try a fundamentally different strategy or explain the blocker to the user.".to_string(),
            });
        }
        let t0 = metrics::record_llm_call_start();
        let llm_round_start = Instant::now();
        let mut first_token_marked = latency.ttft_ms.is_some();
        let response = if config.llm_stream {
            let chat_id_for_cb = msg.chat_id.clone();
            let progress_base = llm_round_start;
            let mut progress_cb = |_delta: &str, accumulated: &str| {
                crate::platform::task_wdt::feed_current_task();
                if !first_token_marked && !accumulated.is_empty() {
                    latency.ttft_ms = Some(progress_base.elapsed().as_millis());
                    first_token_marked = true;
                }
                let Some(ed) = editor else { return };
                // Critical 压力下跳过流式编辑，节省 HTTP 连接与堆开销。
                if stream_edit_disabled
                    || matches!(
                        crate::orchestrator::current_pressure(),
                        crate::orchestrator::PressureLevel::Critical
                    )
                {
                    return;
                }
                let now = Instant::now();

                if stream_msg_id.is_none() {
                    // 首次收到文本：发送占位消息并记录 message_id。
                    match ed.send_initial(&chat_id_for_cb, accumulated) {
                        Ok(Some(id)) => {
                            stream_msg_id = Some(id);
                            last_edit_time = now;
                        }
                        Ok(None) => {}
                        Err(e) => {
                            log::warn!(
                                "[agent_stream] send_initial failed, disabling stream edit: {}",
                                e
                            );
                            stream_edit_disabled = true;
                        }
                    }
                } else if now.duration_since(last_edit_time)
                    >= Duration::from_millis(EDIT_THROTTLE_MS)
                {
                    if let Some(ref mid) = stream_msg_id {
                        if let Err(e) = ed.edit(&chat_id_for_cb, mid, accumulated) {
                            stream_edit_fail_count += 1;
                            if log::log_enabled!(log::Level::Debug) {
                                log::debug!(
                                    "[agent_stream] edit failed ({}/{}): {}",
                                    stream_edit_fail_count,
                                    MAX_EDIT_FAILURES,
                                    e
                                );
                            }
                            if stream_edit_fail_count >= MAX_EDIT_FAILURES {
                                log::warn!(
                                    "[agent_stream] edit failed {} times, disabling stream edit",
                                    MAX_EDIT_FAILURES
                                );
                                stream_edit_disabled = true;
                            }
                        } else {
                            stream_edit_fail_count = 0;
                        }
                        last_edit_time = now;
                    }
                }
            };
            worker_llm.chat_with_progress(
                &mut tool_ctx,
                &system,
                &messages,
                native_tool_calling.then_some(tool_specs.as_slice()),
                llm_tool_choice,
                &mut progress_cb,
            )
        } else {
            worker_llm.chat(
                &mut tool_ctx,
                &system,
                &messages,
                native_tool_calling.then_some(tool_specs.as_slice()),
                llm_tool_choice,
            )
        };
        let response = match response {
            Ok(r) => {
                metrics::record_llm_call_end(t0);
                latency.llm_round_total_ms = latency
                    .llm_round_total_ms
                    .saturating_add(llm_round_start.elapsed().as_millis());
                r
            }
            Err(e) => {
                metrics::record_llm_call_end(t0);
                metrics::record_llm_error();
                metrics::record_error_by_stage("agent_chat");
                return Err(e.with_stage("agent_chat"));
            }
        };
        let response = if has_tools {
            recover_text_tool_calls(response)
        } else {
            response
        };
        crate::platform::task_wdt::feed_current_task();
        metrics::record_wdt_feed();

        let tc_count = response.tool_calls.as_ref().map_or(0, |v| v.len());
        if log::log_enabled!(log::Level::Debug) {
            log::debug!(
                "[agent] llm round={} stop_reason={:?} tool_calls={} content_len={}",
                round,
                response.stop_reason,
                tc_count,
                response.content.len()
            );
        }

        if response.stop_reason == StopReason::MaxTokens {
            let mut content = response.content;
            if !content.is_empty() {
                content.push_str("\n\n");
                content.push_str(&tr(UiMessage::ReplyTruncated, loc));
            }
            final_content = content;
            break;
        }

        if response.stop_reason == StopReason::EndTurn {
            let content = response.content;
            if content.contains(AGENT_MARKER_STOP) {
                let confirmation = strip_agent_stop_confirmation(&content);
                // 流式编辑：更新为清理后的确认文案，避免用户看到原始标记。
                let streamed = if let (Some(ref mid), Some(ed)) = (&stream_msg_id, editor) {
                    if !confirmation.is_empty() {
                        let _ = ed.edit(&msg.chat_id, mid, &confirmation);
                    }
                    true
                } else {
                    false
                };
                return Ok((
                    WorkerOutcome::Interrupt(confirmation),
                    consumed_round,
                    streamed,
                    latency,
                ));
            }
            // P1 Enhancement 4: 任务完成检查 - 检测回复是否过短且没用工具（语言无关）。
            if !any_tool_used && round < MAX_REACT_ROUNDS - 1 && content.len() < 100 {
                // 回复很短且没用任何工具，可能是敷衍回复，给 LLM 一次机会。
                messages.push(Message {
                    role: Cow::Borrowed("assistant"),
                    content: content.clone(),
                });
                messages.push(Message {
                    role: Cow::Borrowed("user"),
                    content: "[SYSTEM] Your response is very brief and you haven't used any tools. If the user's query requires gathering information or performing actions, please use appropriate tools to provide a complete answer.".to_string(),
                });
                // 记录本轮进度（任务未完成，继续下一轮）。
                progress_history[0] = progress_history[1];
                progress_history[1] = progress_history[2];
                progress_history[2] = Some(RoundProgress { new_info: false });
                continue;
            }

            final_content = content;
            break;
        }

        if response.stop_reason == StopReason::ToolUse {
            let tool_calls = response.tool_calls.as_deref().unwrap_or(&[]);
            if tool_calls.is_empty() {
                final_content = response.content;
                break;
            }
            messages.push(Message {
                role: Cow::Borrowed("assistant"),
                // Anthropic API 要求 tool_use 轮的 assistant content 非空；空时用占位符。
                content: if response.content.is_empty() {
                    "[tool_use]".to_string()
                } else {
                    response.content
                },
            });
            let mut cap =
                MAX_TOOL_RESULTS_USER_MESSAGE_LEN.min(tool_calls.len().saturating_mul(192));
            cap = cap.max(TOOL_RESULTS_PREFIX.len());
            let mut user_content_raw = String::with_capacity(cap);
            user_content_raw.push_str(TOOL_RESULTS_PREFIX);
            let mut truncated = false;
            latency.tool_calls = latency.tool_calls.saturating_add(tool_calls.len() as u32);
            // P1 Enhancement 3: 跟踪本轮工具是否有成功。
            let mut round_tool_success = false;
            for (i, tc) in tool_calls.iter().enumerate() {
                // 流式编辑：进入每个工具前更新进度（Telegram typing ~5s 过期；此处用 edit 续期可见活跃状态）。
                if let (Some(ref mid), Some(ed)) = (&stream_msg_id, editor) {
                    if !stream_edit_disabled {
                        let progress = if tool_calls.len() == 1 {
                            tr(
                                UiMessage::ToolProgressSingle {
                                    name: tc.name.clone(),
                                },
                                loc,
                            )
                        } else {
                            tr(
                                UiMessage::ToolProgress {
                                    name: tc.name.clone(),
                                    index: i,
                                    total: tool_calls.len(),
                                },
                                loc,
                            )
                        };
                        let _ = ed.edit(&msg.chat_id, mid, &progress);
                    }
                }
                // 工具执行门控
                let result_owned = {
                    if !registry.is_llm_tool_visible(&tc.name, &tool_policy) {
                        metrics::record_tool_call(false);
                        crate::util::scrub_credentials(
                            &serde_json::json!({
                                "error": format!(
                                    "tool '{}' is not available in the current runtime context",
                                    tc.name
                                )
                            })
                            .to_string(),
                        )
                    } else {
                        let needs_net = registry.is_network_tool(&tc.name);
                        match crate::orchestrator::can_execute_tool_pub(&tc.name, needs_net) {
                            ToolDecision::Deny { reason } => {
                                log::info!("[agent_tool] {} denied: {}", tc.name, reason);
                                crate::util::scrub_credentials(
                                    &serde_json::json!({ "error": reason }).to_string(),
                                )
                            }
                            ToolDecision::Allow => {
                                let tool_exec_start = Instant::now();
                                match registry.execute(&tc.name, &tc.input, &mut tool_ctx) {
                                    Ok(s) => {
                                        latency.tool_exec_ms = latency
                                            .tool_exec_ms
                                            .saturating_add(tool_exec_start.elapsed().as_millis());
                                        metrics::record_tool_call(true);
                                        round_tool_success = true;
                                        any_tool_used = true;
                                        crate::util::scrub_credentials(&s)
                                    }
                                    Err(e) => {
                                        latency.tool_exec_ms = latency
                                            .tool_exec_ms
                                            .saturating_add(tool_exec_start.elapsed().as_millis());
                                        metrics::record_tool_call(false);
                                        metrics::record_error_by_stage(e.stage());
                                        log::error!(
                                            "[agent_tool] {} execute failed: {} input={:?}",
                                            tc.name,
                                            e,
                                            crate::util::truncate_content_to_max(&tc.input, 200)
                                                .as_ref()
                                        );
                                        state::set_last_error(&e);
                                        tool_error_buf.clear();
                                        // 根据错误类型生成具体的引导提示
                                        let hint = match &e {
                                            crate::error::Error::Config { message, .. } => {
                                                if message.contains("not found")
                                                    || message.contains("does not exist")
                                                {
                                                    " Try a different approach or verify the resource exists."
                                                } else if message.contains("invalid")
                                                    || message.contains("parse")
                                                {
                                                    " Check the input format and try with corrected parameters."
                                                } else {
                                                    " Review the parameters and try a different approach."
                                                }
                                            }
                                            crate::error::Error::Http { status_code, .. } => {
                                                if *status_code == 404 {
                                                    " Resource not found. Verify the URL or identifier."
                                                } else if *status_code == 403 || *status_code == 401
                                                {
                                                    " Permission denied. This operation may not be allowed."
                                                } else if *status_code >= 500 {
                                                    " Server error. Try again later or use an alternative method."
                                                } else {
                                                    " Consider an alternative approach."
                                                }
                                            }
                                            crate::error::Error::Io { source, .. } => {
                                                if source.kind() == std::io::ErrorKind::NotFound {
                                                    " File or resource not found. Check the path."
                                                } else if source.kind()
                                                    == std::io::ErrorKind::PermissionDenied
                                                {
                                                    " Permission denied. This operation may not be allowed."
                                                } else if source.kind()
                                                    == std::io::ErrorKind::TimedOut
                                                {
                                                    " Operation timed out. Try with simpler parameters or check connectivity."
                                                } else {
                                                    " Try a different approach."
                                                }
                                            }
                                            _ => {
                                                if e.is_connect_error() {
                                                    " Connection failed. Check network connectivity or try later."
                                                } else {
                                                    " Consider an alternative strategy."
                                                }
                                            }
                                        };
                                        let _ = write!(
                                            &mut tool_error_buf,
                                            "[tool error] {}.{}",
                                            e, hint
                                        );
                                        crate::util::scrub_credentials(tool_error_buf.as_str())
                                    }
                                }
                            }
                        }
                    }
                };
                let result_view = result_owned.as_str();
                let call_key = hash_tool_call(&tc.name, &tc.input);
                let n = tool_call_repeat.entry(call_key).or_insert(0);
                *n = (*n).saturating_add(1);
                let repeat_note = if *n >= 2 {
                    Some(match *n {
                        2 => TOOL_REPEAT_NOTE_2,
                        3 => TOOL_REPEAT_NOTE_3,
                        _ => TOOL_REPEAT_NOTE_MANY,
                    })
                } else {
                    None
                };
                crate::platform::task_wdt::feed_current_task();
                if i > 0
                    && push_bounded_utf8(
                        &mut user_content_raw,
                        "\n",
                        MAX_TOOL_RESULTS_USER_MESSAGE_LEN,
                    )
                {
                    truncated = true;
                    break;
                }
                if push_bounded_utf8(
                    &mut user_content_raw,
                    "[",
                    MAX_TOOL_RESULTS_USER_MESSAGE_LEN,
                ) || push_bounded_utf8(
                    &mut user_content_raw,
                    &tc.id,
                    MAX_TOOL_RESULTS_USER_MESSAGE_LEN,
                ) || push_bounded_utf8(
                    &mut user_content_raw,
                    "]: ",
                    MAX_TOOL_RESULTS_USER_MESSAGE_LEN,
                ) || repeat_note.is_some_and(|note| {
                    push_bounded_utf8(
                        &mut user_content_raw,
                        note,
                        MAX_TOOL_RESULTS_USER_MESSAGE_LEN,
                    )
                }) || push_bounded_utf8(
                    &mut user_content_raw,
                    result_view,
                    MAX_TOOL_RESULTS_USER_MESSAGE_LEN,
                ) {
                    truncated = true;
                    break;
                }
            }
            if truncated && user_content_raw.len() < MAX_TOOL_RESULTS_USER_MESSAGE_LEN {
                let _ = push_bounded_utf8(
                    &mut user_content_raw,
                    "\n[truncated]",
                    MAX_TOOL_RESULTS_USER_MESSAGE_LEN,
                );
            }
            if config.strategy.enables_reflection_boost() && !round_tool_success {
                let _ = push_bounded_utf8(
                    &mut user_content_raw,
                    REFLECTION_NOTE,
                    MAX_TOOL_RESULTS_USER_MESSAGE_LEN,
                );
            }
            messages.push(Message {
                role: Cow::Borrowed("user"),
                content: user_content_raw,
            });
            // P1 Enhancement 3: 记录本轮进度（ToolUse 路径）。
            progress_history[0] = progress_history[1];
            progress_history[1] = progress_history[2];
            progress_history[2] = Some(RoundProgress {
                new_info: round_tool_success,
            });
            continue;
        }

        let content = response.content;
        if content.contains(AGENT_MARKER_STOP) {
            let confirmation = strip_agent_stop_confirmation(&content);
            let streamed = if let (Some(ref mid), Some(ed)) = (&stream_msg_id, editor) {
                if !confirmation.is_empty() {
                    if let Err(e) = ed.edit(&msg.chat_id, mid, &confirmation) {
                        log::warn!(
                            "[agent_stream] final interrupt edit failed, fallback outbound: {}",
                            e
                        );
                        false
                    } else {
                        true
                    }
                } else {
                    true
                }
            } else {
                false
            };
            return Ok((
                WorkerOutcome::Interrupt(confirmation),
                consumed_round,
                streamed,
                latency,
            ));
        }
        final_content = content;
        break;
    }
    // 流式编辑：最终确认发送完整内容（工具执行中已通过 per-tool 进度 edit 续期可见性）。
    let streamed = if let (Some(ref mid), Some(ed)) = (&stream_msg_id, editor) {
        if !final_content.is_empty() {
            if let Err(e) = ed.edit(&msg.chat_id, mid, &final_content) {
                log::warn!("[agent_stream] final edit failed, fallback outbound: {}", e);
                false
            } else {
                true
            }
        } else {
            true
        }
    } else {
        false
    };
    Ok((
        WorkerOutcome::Content(final_content),
        consumed_round,
        streamed,
        latency,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summarize_tool_results_keeps_multiline_preview() {
        let input = concat!(
            "Tool results:\n",
            "[call_1]: Beijing weather: sunny\n",
            "Temperature 25C\n",
            "Humidity 20%\n"
        );
        let summary = summarize_tool_results(input);
        assert!(summary.contains("Beijing weather: sunny"));
        assert!(summary.contains("Temperature 25C"));
    }

    #[test]
    fn summarize_tool_results_adds_total_bytes_for_long_output() {
        let long_value = "a".repeat(120);
        let input = format!("Tool results:\n[call_1]: {}", long_value);
        let summary = summarize_tool_results(&input);
        assert!(summary.contains("[120 bytes total]"));
        assert!(summary.contains("[call_1]: aaaa"));
    }

    #[test]
    fn session_summary_refresh_threshold_is_programmatic() {
        assert!(!should_refresh_session_summary(19, 0));
        assert!(!should_refresh_session_summary(20, 15));
        assert!(should_refresh_session_summary(20, 10));
        assert!(should_refresh_session_summary(35, 20));
    }

    #[test]
    fn fallback_session_summary_keeps_recent_messages_in_order() {
        let recent = vec![
            SessionMessage {
                role: "user".to_string(),
                content: "first".to_string(),
            },
            SessionMessage {
                role: "assistant".to_string(),
                content: "second".to_string(),
            },
            SessionMessage {
                role: "user".to_string(),
                content: "third".to_string(),
            },
        ];
        let summary = fallback_session_summary(&recent);
        assert!(summary.contains("user: first | assistant: second | user: third"));
    }
}
