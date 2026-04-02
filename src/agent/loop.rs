//! Agent ReAct 循环：入站一条 → context → chat（含 tool_use 多轮）→ 会话持久化 → 出站一条。
//! 仅依赖 trait；HTTP/Tool 由 main 注入同一实现（如 EspHttpClient）。
use super::delivery::{DeliveryReport, DeliverySession};
use super::final_reply::finalize_user_visible_reply;
use super::request_plan::AgentRequestPlan;
use super::strategy::{
    append_execution_plan, blocker_end_turn_followup, build_success_tool_round_guidance,
    build_tool_round_guidance, detect_ping_pong_tool_rounds, empty_final_answer_followup,
    final_answer_followup, repeated_answer_followup, should_generate_execution_plan,
    stalled_end_turn_followup, AgentRunStrategy, SuccessfulToolRoundSummary,
};
use super::tool_guidance::{
    build_success_tool_execution_guidance, record_successful_tool_result,
    SuccessfulToolRoundObservations,
};
use super::tool_outcome::{
    classify_tool_error, denied_tool_assessment, summarize_tool_blocker,
    unavailable_tool_assessment, ToolBlockerSummary, ToolFailureSummary,
};
use super::StreamEditor;
use crate::agent::context::{
    build_context, estimate_post_memory_system_tail_len, PostMemoryTailParams, RuntimeContext,
};
use crate::bus::{
    InboundRx, IngressKind, OutboundTx, PcMsg, SystemInboundTx, UserInboundRx, UserInboundTx,
    MAX_CONTENT_LEN,
};
use crate::constants::{
    AGENT_MARKER_MARK_IMPORTANT, AGENT_MARKER_SIGNAL_COMFORT, AGENT_MARKER_STOP,
    AGENT_RETRY_BASE_MS, AGENT_RETRY_MAX_MS, INBOUND_RECV_TIMEOUT_SECS, MAX_DEFER_RETRIES,
    MAX_TOOL_RESULTS_USER_MESSAGE_LEN,
};
use crate::error::Result;
use crate::i18n::{tr, Locale as UiLocale, Message as UiMessage};
use crate::llm::{LlmClient, Message, StopReason, ToolChoicePolicy};
use crate::memory::{
    build_turn_ledger_start, load_prompt_memory_context, normalize_turn_preview,
    normalize_turn_reason, run_long_term_memory_refresh, run_post_reply_memory_maintenance,
    EmotionSignalStore, ExecutionStateStore, ImportantMessageStore,
    LongTermMemoryExtractionStateStore, LongTermMemoryRefreshContext, LongTermMemoryRefreshOutcome,
    LongTermMemoryRefreshRequestOutcome, LongTermMemoryStore, MemoryStore, PendingRetryStore,
    PostReplyMemoryMaintenanceContext, PostReplyMemoryMaintenanceInput, PromptMemoryContextParams,
    SessionStore, SessionSummaryRefreshOutcome, SessionSummaryStore, TurnDeliveryLedger,
    TurnLedger, TurnLedgerStatus, TurnLedgerStore,
};
use crate::metrics;
use crate::orchestrator::admission::{AdmissionDecision, LlmDecision, ToolDecision};
use crate::state;
use crate::tools::http_bridge::HttpClientToolContext;
use crate::util::{
    remove_substrings_all_trim, strip_agent_stop_confirmation, truncate_content_to_max,
};
use crate::PlatformHttpClient;
use serde::{Deserialize, Serialize};
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
const MEMORY_GROUNDING_SUMMARY_PREVIEW_CHARS: usize = 160;
const MEMORY_GROUNDING_LONG_TERM_PREVIEW_CHARS: usize = 240;

/// ReAct 轮间保留完整内容的最近轮数（每轮 assistant + user 各 1 条 = 4 条）。
const REACT_FULL_ROUNDS_KEPT: usize = 2;
const REACT_FULL_MSGS_KEPT: usize = REACT_FULL_ROUNDS_KEPT * 2;

/// 早期轮次工具结果摘要：每条结果保留的首行预览字符数（UTF-8 安全截断）。
const TOOL_RESULT_PREVIEW_CHARS: usize = 80;
const TOOL_RESULT_TAIL_CHARS: usize = 24;
const TOOL_EVIDENCE_PREVIEW_CHARS: usize = 120;
const TOOL_EVIDENCE_TAIL_CHARS: usize = 32;
const MAX_TOOL_EVIDENCE_ITEMS: usize = 4;
const ASSISTANT_COMPACT_PREVIEW_CHARS: usize = 160;
const ASSISTANT_COMPACT_TAIL_CHARS: usize = 48;
const LONG_TERM_MEMORY_REFRESH_CHANNEL: &str = "_memory_refresh";
const POST_REPLY_MAINTENANCE_CHANNEL: &str = "_post_reply_maintenance";
const POST_REPLY_MAINTENANCE_USER_PREVIEW_CHARS: usize = 512;
const POST_REPLY_MAINTENANCE_REPLY_PREVIEW_CHARS: usize = 768;
const PLAN_SYSTEM_SUFFIX: &str = "\n\n## Internal planning\nBefore solving the latest user request, create a short internal execution plan. Return 3-6 concise numbered steps only. Do not answer the user. Do not call tools in this planning step.";
const FINAL_RECOVERY_SYSTEM_SUFFIX: &str = "\n\n## Final delivery\nThe tool-execution budget for this turn is exhausted. Do not call any tool. Using only the completed tool results and current conclusions already present in this conversation, produce the final user-facing answer now. Do not output execution transcripts, numbered step logs, or future-step sections.";

/// 程序性会话摘要：单次轻量 LLM 调用的 system 提示。
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

fn is_long_term_memory_refresh_job(msg: &PcMsg) -> bool {
    msg.ingress == IngressKind::System && msg.channel.as_ref() == LONG_TERM_MEMORY_REFRESH_CHANNEL
}

fn is_post_reply_maintenance_job(msg: &PcMsg) -> bool {
    msg.ingress == IngressKind::System && msg.channel.as_ref() == POST_REPLY_MAINTENANCE_CHANNEL
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct PostReplyMaintenanceJobPayload {
    ingress: IngressKind,
    source_channel: String,
    user_content: String,
    reply_content: String,
    tool_calls: u32,
    now_secs: u64,
}

impl PostReplyMaintenanceJobPayload {
    fn from_turn(msg: &PcMsg, reply_content: &str, tool_calls: u32) -> Self {
        Self {
            ingress: msg.ingress,
            source_channel: msg.channel.to_string(),
            user_content: truncate_content_to_max(
                &msg.content,
                POST_REPLY_MAINTENANCE_USER_PREVIEW_CHARS,
            )
            .into_owned(),
            reply_content: truncate_content_to_max(
                reply_content,
                POST_REPLY_MAINTENANCE_REPLY_PREVIEW_CHARS,
            )
            .into_owned(),
            tool_calls,
            now_secs: crate::util::current_unix_secs(),
        }
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

struct WorkerRunTelemetry {
    streamed: bool,
    latency: WorkerLatency,
    delivery: DeliveryReport,
    any_tool_used: bool,
    used_final_answer_recovery: bool,
}

fn mark_ttft_if_visible(latency: &mut WorkerLatency, worker_start: Instant, content: &str) {
    if latency.ttft_ms.is_none() && !content.trim().is_empty() {
        latency.ttft_ms = Some(worker_start.elapsed().as_millis());
    }
}

fn persist_turn_ledger(
    store: &dyn TurnLedgerStore,
    chat_id: &str,
    ledger: &TurnLedger,
    stage: &str,
) {
    if let Err(error) = store.set(chat_id, ledger) {
        log::warn!(
            "[agent_turn] failed to persist ledger stage={} chat_id={}: {}",
            stage,
            chat_id,
            error
        );
    }
}

fn build_turn_delivery_ledger(report: DeliveryReport) -> TurnDeliveryLedger {
    TurnDeliveryLedger {
        waiting_notice_sent: report.waiting_notice_sent,
        progress_updates_sent: report.progress_updates_sent,
        partial_updates_sent: report.partial_updates_sent,
        current_primary_delivered: report.current_primary_delivered,
        finalize_streamed: report.finalize_streamed,
        visible_text_updates_sent: report.visible_text_updates_sent,
    }
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

fn hash_tool_round(call_keys: &[u64]) -> u64 {
    let mut h = DefaultHasher::new();
    call_keys.len().hash(&mut h);
    for key in call_keys {
        key.hash(&mut h);
    }
    h.finish()
}

fn tool_result_status_attr(call_failed: bool) -> &'static str {
    if call_failed {
        "error"
    } else {
        "ok"
    }
}

fn failure_kind_attr(
    failure_kind: Option<super::tool_outcome::ToolFailureKind>,
) -> Option<&'static str> {
    match failure_kind {
        Some(super::tool_outcome::ToolFailureKind::Retryable) => Some("retryable"),
        Some(super::tool_outcome::ToolFailureKind::Permanent) => Some("permanent"),
        Some(super::tool_outcome::ToolFailureKind::Capability) => Some("capability"),
        None => None,
    }
}

struct ToolResultBlock<'a> {
    call_id: &'a str,
    tool_name: &'a str,
    status: &'a str,
    failure: Option<&'a str>,
    repeat_count: usize,
    content: &'a str,
}

fn append_tool_result_block(
    dst: &mut String,
    block: ToolResultBlock<'_>,
    max_bytes: usize,
) -> bool {
    if push_bounded_utf8(dst, "<tool_result id=\"", max_bytes)
        || push_bounded_utf8(dst, block.call_id, max_bytes)
        || push_bounded_utf8(dst, "\" tool=\"", max_bytes)
        || push_bounded_utf8(dst, block.tool_name, max_bytes)
        || push_bounded_utf8(dst, "\" status=\"", max_bytes)
        || push_bounded_utf8(dst, block.status, max_bytes)
    {
        return true;
    }
    if let Some(failure) = block.failure {
        if push_bounded_utf8(dst, "\" failure=\"", max_bytes)
            || push_bounded_utf8(dst, failure, max_bytes)
        {
            return true;
        }
    }
    if block.repeat_count > 1 {
        let repeat_attr = block.repeat_count.to_string();
        if push_bounded_utf8(dst, "\" repeat_count=\"", max_bytes)
            || push_bounded_utf8(dst, &repeat_attr, max_bytes)
        {
            return true;
        }
    }
    push_bounded_utf8(dst, "\">\n", max_bytes)
        || push_bounded_utf8(dst, block.content, max_bytes)
        || push_bounded_utf8(dst, "\n</tool_result>", max_bytes)
}

fn append_tool_round_guidance_block(dst: &mut String, guidance: &str, max_bytes: usize) -> bool {
    push_bounded_utf8(dst, "<tool_round_guidance>\n", max_bytes)
        || push_bounded_utf8(dst, guidance, max_bytes)
        || push_bounded_utf8(dst, "\n</tool_round_guidance>", max_bytes)
}

fn merge_tool_round_guidance(primary: Option<String>, extra: Option<String>) -> Option<String> {
    match (primary, extra) {
        (Some(mut primary), Some(extra)) => {
            if !primary.ends_with('\n') {
                primary.push('\n');
            }
            primary.push_str(extra.trim());
            Some(primary)
        }
        (Some(primary), None) => Some(primary),
        (None, Some(extra)) => Some(extra),
        (None, None) => None,
    }
}

fn append_tool_evidence_summary_block(
    dst: &mut String,
    evidence_lines: &[String],
    omitted_count: usize,
    max_bytes: usize,
) -> bool {
    if push_bounded_utf8(dst, "<tool_evidence_summary>\n", max_bytes) {
        return true;
    }
    for line in evidence_lines {
        if push_bounded_utf8(dst, line, max_bytes) || push_bounded_utf8(dst, "\n", max_bytes) {
            return true;
        }
    }
    if omitted_count > 0 {
        let more = format!("- [more] {omitted_count} additional successful tool result(s)");
        if push_bounded_utf8(dst, &more, max_bytes) || push_bounded_utf8(dst, "\n", max_bytes) {
            return true;
        }
    }
    push_bounded_utf8(dst, "</tool_evidence_summary>", max_bytes)
}

fn append_memory_grounding_block(dst: &mut String, grounding: &str, max_bytes: usize) -> bool {
    push_bounded_utf8(dst, "<memory_grounding>\n", max_bytes)
        || push_bounded_utf8(dst, grounding, max_bytes)
        || push_bounded_utf8(dst, "\n</memory_grounding>", max_bytes)
}

fn build_memory_grounding_text(
    summary_text: Option<&str>,
    long_term_memory_text: Option<&str>,
) -> Option<String> {
    let mut out = String::new();
    if let Some(summary) = summary_text
        .map(str::trim)
        .filter(|summary| !summary.is_empty())
    {
        let preview = truncate_content_to_max(summary, MEMORY_GROUNDING_SUMMARY_PREVIEW_CHARS);
        let _ = writeln!(out, "[summary] {}", preview.as_ref());
    }
    if let Some(long_term) = long_term_memory_text {
        let bullets = extract_long_term_memory_bullets(long_term);
        if !bullets.is_empty() {
            let joined = bullets.join(" | ");
            let preview =
                truncate_content_to_max(&joined, MEMORY_GROUNDING_LONG_TERM_PREVIEW_CHARS);
            let _ = writeln!(out, "[long_term] {}", preview.as_ref());
        }
    }
    let trimmed = out.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn extract_long_term_memory_bullets(block: &str) -> Vec<String> {
    let mut bullets = Vec::new();
    for line in block.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with("- ") {
            continue;
        }
        bullets.push(trimmed.to_string());
        if bullets.len() >= 3 {
            break;
        }
    }
    bullets
}

fn extract_tag_attr<'a>(line: &'a str, attr: &str) -> Option<&'a str> {
    let pattern = format!("{attr}=\"");
    let start = line.find(&pattern)? + pattern.len();
    let remain = &line[start..];
    let end = remain.find('"')?;
    Some(&remain[..end])
}

/// 将早期轮次的工具结果压缩为「预览 + 总字节数」摘要，保留语义锚点、不丢轮次结构。
fn summarize_tool_results(content: &str) -> String {
    let body = content.strip_prefix(TOOL_RESULTS_PREFIX).unwrap_or(content);
    let mut out = String::with_capacity(512);
    out.push_str("Tool results (prior round):\n");
    let mut wrote_any = false;
    let mut lines = body.lines().peekable();
    while let Some(line) = lines.next() {
        if line.starts_with("<tool_result ") {
            let call_id = extract_tag_attr(line, "id").unwrap_or("unknown");
            let tool_name = extract_tag_attr(line, "tool").unwrap_or("unknown");
            let status = extract_tag_attr(line, "status").unwrap_or("unknown");
            let failure = extract_tag_attr(line, "failure");
            let repeat_count = extract_tag_attr(line, "repeat_count");
            let mut block_body = String::new();
            for next in lines.by_ref() {
                if next == "</tool_result>" {
                    break;
                }
                if !block_body.is_empty() {
                    block_body.push('\n');
                }
                block_body.push_str(next);
            }
            let preview = build_tool_result_preview(&block_body);
            let header = match (failure, repeat_count) {
                (Some(failure), Some(repeat_count)) => {
                    format!("[{call_id}] {tool_name} status={status} failure={failure} repeat={repeat_count}: ")
                }
                (Some(failure), None) => {
                    format!("[{call_id}] {tool_name} status={status} failure={failure}: ")
                }
                (None, Some(repeat_count)) => {
                    format!("[{call_id}] {tool_name} status={status} repeat={repeat_count}: ")
                }
                (None, None) => format!("[{call_id}] {tool_name} status={status}: "),
            };
            if block_body.chars().count() <= TOOL_RESULT_PREVIEW_CHARS {
                let _ = writeln!(out, "{header}{preview}");
            } else {
                let _ = writeln!(out, "{header}{}[{} bytes total]", preview, block_body.len());
            }
            wrote_any = true;
            continue;
        }
        if line == "<tool_round_guidance>" {
            let mut guidance = String::new();
            for next in lines.by_ref() {
                if next == "</tool_round_guidance>" {
                    break;
                }
                if !guidance.is_empty() {
                    guidance.push('\n');
                }
                guidance.push_str(next);
            }
            let _ = writeln!(
                out,
                "[guidance] {}",
                truncate_content_to_max(&guidance, 140).as_ref()
            );
            wrote_any = true;
            continue;
        }
        if line == "<tool_evidence_summary>" {
            for next in lines.by_ref() {
                if next == "</tool_evidence_summary>" {
                    break;
                }
                let trimmed = next.trim();
                if trimmed.is_empty() {
                    continue;
                }
                let _ = writeln!(
                    out,
                    "[evidence] {}",
                    build_tool_evidence_preview(trimmed).unwrap_or_default()
                );
                wrote_any = true;
            }
            continue;
        }
        if line == "<memory_grounding>" {
            let mut memory = String::new();
            for next in lines.by_ref() {
                if next == "</memory_grounding>" {
                    break;
                }
                let trimmed = next.trim();
                if trimmed.is_empty() {
                    continue;
                }
                if !memory.is_empty() {
                    memory.push(' ');
                }
                memory.push_str(trimmed);
            }
            let _ = writeln!(
                out,
                "[memory] {}",
                truncate_content_to_max(&memory, 180).as_ref()
            );
            wrote_any = true;
            continue;
        }
        if let Some(idx) = line.find("]: ") {
            let id_part = &line[..idx + 3];
            let first_val = &line[idx + 3..];
            let mut block_body = String::new();
            block_body.push_str(first_val);
            let mut total_bytes = first_val.len();
            while let Some(next) = lines.peek().copied() {
                if next.contains("]: ") && next.starts_with('[') {
                    break;
                }
                total_bytes = total_bytes.saturating_add(1).saturating_add(next.len());
                block_body.push('\n');
                block_body.push_str(next);
                let _ = lines.next();
            }
            let preview = build_tool_result_preview(&block_body);
            if block_body.chars().count() > TOOL_RESULT_PREVIEW_CHARS {
                let _ = writeln!(out, "{}{}[{} bytes total]", id_part, preview, total_bytes);
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

fn take_suffix_chars(text: &str, max_chars: usize) -> &str {
    if max_chars == 0 {
        return "";
    }
    let total_chars = text.chars().count();
    if total_chars <= max_chars {
        return text;
    }
    let keep_from = total_chars.saturating_sub(max_chars);
    match text.char_indices().nth(keep_from) {
        Some((idx, _)) => &text[idx..],
        None => text,
    }
}

fn collapse_inline_whitespace(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    let mut normalized = String::with_capacity(trimmed.len().min(256));
    let mut pending_space = false;
    for ch in trimmed.chars() {
        if ch.is_whitespace() {
            pending_space = !normalized.is_empty();
            continue;
        }
        if pending_space {
            normalized.push(' ');
            pending_space = false;
        }
        normalized.push(ch);
    }
    if normalized.is_empty() {
        return None;
    }
    Some(normalized)
}

fn build_head_tail_preview(text: &str, max_chars: usize, tail_chars: usize) -> Option<String> {
    let normalized = collapse_inline_whitespace(text)?;
    let total_chars = normalized.chars().count();
    if total_chars <= max_chars {
        return Some(normalized);
    }
    let head_chars = max_chars
        .saturating_sub(tail_chars)
        .saturating_sub(5)
        .max(1);
    let head = truncate_content_to_max(&normalized, head_chars).into_owned();
    let tail = take_suffix_chars(&normalized, tail_chars);
    Some(format!("{head} ... {tail}"))
}

fn build_tool_result_preview(text: &str) -> String {
    build_head_tail_preview(text, TOOL_RESULT_PREVIEW_CHARS, TOOL_RESULT_TAIL_CHARS)
        .unwrap_or_default()
}

fn build_tool_evidence_preview(text: &str) -> Option<String> {
    build_head_tail_preview(text, TOOL_EVIDENCE_PREVIEW_CHARS, TOOL_EVIDENCE_TAIL_CHARS)
}

fn build_assistant_compact_preview(text: &str) -> Option<String> {
    build_head_tail_preview(
        text,
        ASSISTANT_COMPACT_PREVIEW_CHARS,
        ASSISTANT_COMPACT_TAIL_CHARS,
    )
}

fn build_tool_evidence_line(call_id: &str, tool_name: &str, content: &str) -> Option<String> {
    let preview = build_tool_evidence_preview(content)?;
    Some(format!("- [{call_id}] {tool_name}: {preview}"))
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
        } else if msg.role.as_ref() == "assistant" && msg.content.len() > 200 {
            if let Some(preview) = build_assistant_compact_preview(&msg.content) {
                if preview.chars().count() < msg.content.chars().count() {
                    msg.content = preview;
                    msg.content.push_str(" [compressed]");
                }
            }
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
    Proceed(PcMsg),
    Skipped,
}

#[allow(clippy::too_many_arguments)]
fn handle_llm_gate(
    mut msg: PcMsg,
    loc: UiLocale,
    user_inbound_tx: &UserInboundTx,
    system_inbound_tx: &SystemInboundTx,
    outbound_tx: &OutboundTx,
    config: &AgentLoopConfig,
) -> GateResult {
    crate::orchestrator::refresh_heap_if_stale();
    match crate::orchestrator::can_call_llm_pub() {
        LlmDecision::Proceed => GateResult::Proceed(msg),
        LlmDecision::RetryLater { delay_ms } => {
            let is_system = msg.ingress == IngressKind::System;
            msg.enqueue_ts_ms = now_unix_ms();
            let inbound_tx = choose_inbound_tx(msg.ingress, user_inbound_tx, system_inbound_tx);
            match inbound_tx.try_send(msg) {
                Ok(()) => {}
                Err(std::sync::mpsc::TrySendError::Full(m)) => {
                    let _ = config.pending_retry.save_pending_retry(&m);
                    let suffix = if m.ingress == IngressKind::System {
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
                    let suffix = if is_system { "(system)" } else { "" };
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
                msg.enqueue_ts_ms = now_unix_ms();
                let inbound_tx = choose_inbound_tx(msg.ingress, user_inbound_tx, system_inbound_tx);
                if let Err(std::sync::mpsc::TrySendError::Full(m)) = inbound_tx.try_send(msg) {
                    let _ = config.pending_retry.save_pending_retry(&m);
                }
            } else {
                log::info!("[agent] LLM degraded: {}", reason);
                let out = PcMsg {
                    channel: msg.channel.clone(),
                    chat_id: msg.chat_id.clone(),
                    content: tr(UiMessage::LowMemoryUserDefer, loc),
                    req_id: Some(msg.req_id.as_deref().unwrap_or_default().to_owned()),
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

fn run_long_term_memory_refresh_job(
    http: &mut dyn PlatformHttpClient,
    worker_llm: &(dyn LlmClient + Send + Sync),
    config: &AgentLoopConfig,
    msg: &PcMsg,
) {
    let loc = (config.resolve_locale)();
    let mut llm_ctx = HttpClientToolContext {
        http,
        chat_id: Some(Arc::from(msg.chat_id.as_ref())),
        channel: Some(Arc::from("system")),
        outbound_tx: None,
        req_id: None,
        supports_current_chat_primary_reply: false,
        supports_explicit_outbound_message: false,
        outbound_message_budget: 0,
        outbound_message_count: 0,
        current_primary_message_delivered: false,
        locale: loc,
    };
    let outcome = run_long_term_memory_refresh(
        &mut llm_ctx,
        worker_llm,
        LongTermMemoryRefreshContext {
            session_store: config.session_store.as_ref(),
            session_summary_store: config.session_summary_store.as_ref(),
            long_term_memory_store: config.long_term_memory_store.as_ref(),
            extraction_state_store: config.long_term_memory_extraction_state_store.as_ref(),
        },
        &msg.chat_id,
        crate::orchestrator::snapshot().pressure,
        config.memory_profile,
    );
    outcome.persist(
        config.long_term_memory_extraction_state_store.as_ref(),
        &msg.chat_id,
    );
    match outcome {
        LongTermMemoryRefreshOutcome::Processed { changed_count, .. } => {
            if changed_count > 0 {
                log::info!(
                    "[agent_memory] long-term memory refreshed for {} (count={})",
                    msg.chat_id,
                    changed_count
                );
            }
        }
        LongTermMemoryRefreshOutcome::Failed { error, .. } => {
            log::warn!("[agent_memory] refresh failed: {}", error);
        }
        LongTermMemoryRefreshOutcome::Deferred { .. } => {}
    }
}

fn enqueue_post_reply_maintenance_job(
    system_inbound_tx: &SystemInboundTx,
    msg: &PcMsg,
    reply_content: &str,
    tool_calls: u32,
) -> bool {
    let payload = PostReplyMaintenanceJobPayload::from_turn(msg, reply_content, tool_calls);
    let body = match serde_json::to_string(&payload) {
        Ok(body) => body,
        Err(error) => {
            log::warn!(
                "[agent_memory] maintenance job serialize failed chat_id={}: {}",
                msg.chat_id,
                error
            );
            return false;
        }
    };
    let job = match PcMsg::new_system(POST_REPLY_MAINTENANCE_CHANNEL, msg.chat_id.as_ref(), body) {
        Ok(job) => job,
        Err(error) => {
            log::warn!(
                "[agent_memory] maintenance job build failed chat_id={}: {}",
                msg.chat_id,
                error
            );
            return false;
        }
    };
    match system_inbound_tx.try_send(job) {
        Ok(()) => true,
        Err(std::sync::mpsc::TrySendError::Full(_)) => {
            log::debug!(
                "[agent_memory] skip maintenance enqueue because system queue is full chat_id={}",
                msg.chat_id
            );
            false
        }
        Err(std::sync::mpsc::TrySendError::Disconnected(_)) => {
            log::warn!("[agent_memory] maintenance enqueue failed: system queue disconnected");
            false
        }
    }
}

fn run_post_reply_maintenance_job(
    http: &mut dyn PlatformHttpClient,
    worker_llm: &(dyn LlmClient + Send + Sync),
    config: &AgentLoopConfig,
    system_inbound_tx: &SystemInboundTx,
    msg: &PcMsg,
) {
    let payload: PostReplyMaintenanceJobPayload = match serde_json::from_str(&msg.content) {
        Ok(payload) => payload,
        Err(error) => {
            log::warn!(
                "[agent_memory] maintenance job decode failed chat_id={}: {}",
                msg.chat_id,
                error
            );
            return;
        }
    };
    let loc = (config.resolve_locale)();
    let mut llm_ctx = HttpClientToolContext {
        http,
        chat_id: Some(Arc::from(msg.chat_id.as_ref())),
        channel: Some(Arc::from("system")),
        outbound_tx: None,
        req_id: None,
        supports_current_chat_primary_reply: false,
        supports_explicit_outbound_message: false,
        outbound_message_budget: 0,
        outbound_message_count: 0,
        current_primary_message_delivered: false,
        locale: loc,
    };
    let maintenance_outcome = run_post_reply_memory_maintenance(
        &mut llm_ctx,
        worker_llm,
        PostReplyMemoryMaintenanceContext {
            session_store: config.session_store.as_ref(),
            session_summary_store: config.session_summary_store.as_ref(),
            execution_state_store: config.execution_state_store.as_ref(),
            extraction_state_store: config.long_term_memory_extraction_state_store.as_ref(),
        },
        PostReplyMemoryMaintenanceInput {
            chat_id: &msg.chat_id,
            ingress: payload.ingress,
            channel: &payload.source_channel,
            user_content: &payload.user_content,
            reply_content: &payload.reply_content,
            pressure: crate::orchestrator::snapshot().pressure,
            memory_profile: config.memory_profile,
            tool_calls: payload.tool_calls,
            now_secs: payload.now_secs,
        },
        || match PcMsg::new_system(LONG_TERM_MEMORY_REFRESH_CHANNEL, msg.chat_id.as_ref(), "") {
            Ok(job) => match system_inbound_tx.try_send(job) {
                Ok(()) => true,
                Err(std::sync::mpsc::TrySendError::Full(_)) => {
                    log::debug!(
                        "[agent_memory] skip refresh enqueue because system queue is full chat_id={}",
                        msg.chat_id
                    );
                    false
                }
                Err(std::sync::mpsc::TrySendError::Disconnected(_)) => {
                    log::warn!("[agent_memory] refresh enqueue failed: system queue disconnected");
                    false
                }
            },
            Err(error) => {
                log::warn!("[agent_memory] refresh job build failed: {}", error);
                false
            }
        },
    );
    match maintenance_outcome.summary_result {
        Ok(SessionSummaryRefreshOutcome::Updated { used_fallback }) => {
            if used_fallback {
                log::info!("[agent_summary] updated for {} (fallback)", msg.chat_id);
            } else {
                log::info!("[agent_summary] updated for {}", msg.chat_id);
            }
        }
        Ok(SessionSummaryRefreshOutcome::Skipped) => {}
        Err(error) => log::warn!("[agent_summary] failed: {}", error),
    }
    match maintenance_outcome.execution_state_result {
        Ok(crate::memory::ExecutionStateRefreshOutcome::Updated) => {
            log::info!("[agent_execution_state] updated for {}", msg.chat_id);
        }
        Ok(crate::memory::ExecutionStateRefreshOutcome::Cleared) => {
            log::info!("[agent_execution_state] cleared for {}", msg.chat_id);
        }
        Ok(crate::memory::ExecutionStateRefreshOutcome::Skipped) => {}
        Err(error) => log::warn!("[agent_execution_state] failed: {}", error),
    }
    if maintenance_outcome.extraction_request_outcome
        == LongTermMemoryRefreshRequestOutcome::RequestFailed
    {
        log::debug!(
            "[agent_memory] refresh request was eligible but not enqueued chat_id={}",
            msg.chat_id
        );
    }
}

/// run_worker_path 返回：正常内容或用户要求停止时的确认文案。
pub enum WorkerOutcome {
    Content(String),
    Delivered(String),
    Interrupt(String),
}

/// 单轮进度指标，用于检测 agent 是否陷入无效循环。
#[derive(Clone, Copy)]
struct RoundProgress {
    /// 本轮是否产生新信息（工具成功或内容长度显著增加）
    new_info: bool,
}

#[derive(Default)]
struct RecentToolRoundState {
    consecutive_stalled_rounds: u8,
    stalled_signatures: [Option<u64>; 4],
    blocker: Option<ToolBlockerSummary>,
    successful_round: Option<SuccessfulToolRoundSummary>,
}

impl RecentToolRoundState {
    fn record_round(
        &mut self,
        total_calls: usize,
        round_had_success: bool,
        round_signature: u64,
        failure_summary: ToolFailureSummary,
    ) {
        if round_had_success {
            self.consecutive_stalled_rounds = 0;
            self.stalled_signatures = [None; 4];
            self.blocker = None;
            self.successful_round = Some(SuccessfulToolRoundSummary {
                total_calls,
                successful_calls: total_calls.saturating_sub(failure_summary.failed_calls),
            });
            return;
        }
        self.consecutive_stalled_rounds = self.consecutive_stalled_rounds.saturating_add(1);
        self.stalled_signatures[0] = self.stalled_signatures[1];
        self.stalled_signatures[1] = self.stalled_signatures[2];
        self.stalled_signatures[2] = self.stalled_signatures[3];
        self.stalled_signatures[3] = Some(round_signature);
        self.blocker = summarize_tool_blocker(total_calls, failure_summary);
        self.successful_round = None;
    }

    fn ping_pong_detected(&self) -> bool {
        detect_ping_pong_tool_rounds(&self.stalled_signatures)
    }
}

fn enqueue_end_turn_followup(
    messages: &mut Vec<Message>,
    progress_history: &mut [Option<RoundProgress>; 3],
    content: &str,
    followup: &str,
) {
    if !content.trim().is_empty() {
        messages.push(Message {
            role: Cow::Borrowed("assistant"),
            content: content.to_string(),
        });
    }
    messages.push(Message {
        role: Cow::Borrowed("user"),
        content: followup.to_string(),
    });
    progress_history[0] = progress_history[1];
    progress_history[1] = progress_history[2];
    progress_history[2] = Some(RoundProgress { new_info: false });
}

fn collect_recent_assistant_messages<'a>(
    messages: &'a [Message],
    limit: usize,
    out: &mut Vec<&'a str>,
) {
    out.clear();
    if out.capacity() < limit {
        out.reserve(limit - out.capacity());
    }
    for message in messages.iter().rev() {
        if out.len() >= limit {
            break;
        }
        if message.role.as_ref() != "assistant" {
            continue;
        }
        let content = message.content.trim();
        if content.is_empty() || content == "[tool_use]" {
            continue;
        }
        out.push(content);
    }
}

fn prepare_system_with_suffix<'a>(base: &str, suffix: &str, scratch: &'a mut String) -> &'a str {
    scratch.clear();
    let required = base.len().saturating_add(suffix.len());
    if scratch.capacity() < required {
        scratch.reserve(required - scratch.capacity());
    }
    scratch.push_str(base);
    scratch.push_str(suffix);
    scratch.as_str()
}

struct EndTurnFollowupContext<'a> {
    request_plan: &'a AgentRequestPlan<'a>,
    strategy: AgentRunStrategy,
    round: usize,
    any_tool_used: bool,
    end_turn_followup_used: bool,
    recent_tool_round: &'a RecentToolRoundState,
    messages: &'a [Message],
    content: &'a str,
}

fn resolve_end_turn_followup(ctx: EndTurnFollowupContext<'_>) -> Option<(String, bool)> {
    if let Some(followup) =
        ctx.request_plan
            .missing_tool_followup(ctx.round, ctx.any_tool_used, ctx.content)
    {
        return Some((followup.to_string(), false));
    }
    if ctx.end_turn_followup_used {
        return None;
    }
    if let Some(followup) = final_answer_followup(
        ctx.strategy,
        ctx.recent_tool_round.successful_round,
        ctx.content,
    ) {
        return Some((followup, true));
    }
    let mut recent_assistant_messages = Vec::with_capacity(3);
    collect_recent_assistant_messages(ctx.messages, 3, &mut recent_assistant_messages);
    if let Some(followup) =
        repeated_answer_followup(ctx.strategy, &recent_assistant_messages, ctx.content)
    {
        return Some((followup.to_string(), true));
    }
    if let Some(followup) =
        blocker_end_turn_followup(ctx.strategy, ctx.recent_tool_round.blocker, ctx.content)
    {
        return Some((followup.to_string(), true));
    }
    stalled_end_turn_followup(
        ctx.strategy,
        ctx.recent_tool_round.consecutive_stalled_rounds,
        ctx.content,
    )
    .map(|followup| (followup.to_string(), true))
}

fn run_final_answer_recovery_round(
    worker_llm: &(dyn LlmClient + Send + Sync),
    tool_ctx: &mut HttpClientToolContext<'_>,
    system: &str,
    messages: &[Message],
    llm_stream: bool,
    latency: &mut WorkerLatency,
    system_scratch: &mut String,
) -> Result<String> {
    let recovery_system =
        prepare_system_with_suffix(system, FINAL_RECOVERY_SYSTEM_SUFFIX, system_scratch);
    let t0 = metrics::record_llm_call_start();
    let llm_round_start = Instant::now();
    let response = if llm_stream {
        let mut ignore_progress = |_delta: &str, _accumulated: &str| {
            crate::platform::task_wdt::feed_current_task();
        };
        worker_llm.chat_with_progress(
            tool_ctx,
            recovery_system,
            messages,
            None,
            ToolChoicePolicy::Auto,
            &mut ignore_progress,
        )
    } else {
        worker_llm.chat(
            tool_ctx,
            recovery_system,
            messages,
            None,
            ToolChoicePolicy::Auto,
        )
    };
    match response {
        Ok(response) => {
            metrics::record_llm_call_end(t0);
            latency.react_rounds = latency.react_rounds.saturating_add(1);
            latency.llm_round_total_ms = latency
                .llm_round_total_ms
                .saturating_add(llm_round_start.elapsed().as_millis());
            Ok(response.content)
        }
        Err(e) => {
            metrics::record_llm_call_end(t0);
            metrics::record_llm_error();
            metrics::record_error_by_stage("agent_chat");
            Err(e.with_stage("agent_chat"))
        }
    }
}

/// Agent 循环的存储与运行参数，由 main 构建并传入 run_agent_loop，减少参数数量。
pub struct AgentLoopConfig {
    pub memory_store: Arc<dyn MemoryStore + Send + Sync>,
    pub long_term_memory_store: Arc<dyn LongTermMemoryStore + Send + Sync>,
    pub long_term_memory_extraction_state_store:
        Arc<dyn LongTermMemoryExtractionStateStore + Send + Sync>,
    pub session_store: Arc<dyn SessionStore + Send + Sync>,
    pub session_summary_store: Arc<dyn SessionSummaryStore + Send + Sync>,
    pub execution_state_store: Arc<dyn ExecutionStateStore + Send + Sync>,
    pub turn_ledger_store: Arc<dyn TurnLedgerStore + Send + Sync>,
    pub memory_profile: crate::memory::MemoryProfile,
    pub get_skill_descriptions: Arc<dyn Fn() -> String + Send + Sync>,
    pub session_max_messages: usize,
    pub tg_group_activation: Arc<str>,
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
        let queue_wait_ms = now_unix_ms().saturating_sub(msg.enqueue_ts_ms) as u128;
        if msg.ingress == IngressKind::System {
            metrics::record_system_queue_wait_ms(queue_wait_ms);
        } else {
            metrics::record_user_queue_wait_ms(queue_wait_ms);
        }
        if is_long_term_memory_refresh_job(&msg) {
            run_long_term_memory_refresh_job(http, worker_llm, config, &msg);
            metrics::record_system_message_done(false);
            continue;
        }
        if is_post_reply_maintenance_job(&msg) {
            run_post_reply_maintenance_job(http, worker_llm, config, &system_inbound_tx, &msg);
            metrics::record_system_message_done(false);
            continue;
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
                req_id: Some(msg.req_id.as_deref().unwrap_or_default().to_owned()),
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
                            req_id: Some(msg.req_id.as_deref().unwrap_or_default().to_owned()),
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
                        req_id: Some(msg.req_id.as_deref().unwrap_or_default().to_owned()),
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
        msg = match handle_llm_gate(
            msg,
            loc,
            &user_inbound_tx,
            &system_inbound_tx,
            &outbound_tx,
            config,
        ) {
            GateResult::Proceed(msg) => msg,
            GateResult::Skipped => continue,
        };

        // Gate 通过后获取任务槽位：Guard Drop 时自动递减，覆盖整个任务生命周期（含工具调用、会话写入、回复发送）。
        // Acquire task slot only after gate passes; guard auto-decrements on drop.
        let _agent_task_guard = crate::orchestrator::begin_agent_task();
        let turn_started_at_ms = now_unix_ms();
        let mut turn_ledger = build_turn_ledger_start(
            msg.req_id.as_deref().unwrap_or_default(),
            &msg.channel,
            msg.ingress,
            &msg.content,
            turn_started_at_ms,
        );
        persist_turn_ledger(
            config.turn_ledger_store.as_ref(),
            &msg.chat_id,
            &turn_ledger,
            "start",
        );
        if let Some(ref mut f) = typing_notifier {
            f(&msg.channel, &msg.chat_id, http);
        }
        let final_content = run_worker_path(
            http,
            worker_llm,
            &msg,
            &outbound_tx,
            msg.req_id.as_deref().unwrap_or_default(),
            registry,
            config,
            &mut tool_call_repeat_buf,
            loc,
        );

        let (outcome, telemetry) = match final_content {
            Ok(ok) => ok,
            Err(e) => {
                let llm_ms = msg_start.elapsed().as_millis().saturating_sub(admission_ms);
                let total_ms = msg_start.elapsed().as_millis();
                turn_ledger.status = TurnLedgerStatus::Failed;
                turn_ledger.reason = normalize_turn_reason(e.stage());
                turn_ledger.updated_at_ms = now_unix_ms();
                turn_ledger.finished_at_ms = turn_ledger.updated_at_ms;
                turn_ledger.total_ms = total_ms.min(u64::MAX as u128) as u64;
                turn_ledger.reply_preview =
                    normalize_turn_preview(&tr(UiMessage::NodeMaintenance, loc));
                persist_turn_ledger(
                    config.turn_ledger_store.as_ref(),
                    &msg.chat_id,
                    &turn_ledger,
                    "error",
                );
                crate::platform::task_wdt::feed_current_task();
                metrics::record_error_by_stage(e.stage());
                log::warn!("[agent:{}] chat loop failed: {}", worker_lane_tag, e);
                log::warn!(
                    "[latency][agent:{}] req_id={} channel={} chat_id={} admission_ms={} llm_ms={} total_ms={} status=llm_error",
                    worker_lane_tag,
                    msg.req_id.as_deref().unwrap_or_default(),
                    msg.channel,
                    msg.chat_id,
                    admission_ms,
                    llm_ms,
                    total_ms
                );
                state::set_last_error(&e);

                let (counter, _) = llm_failure_count
                    .entry(msg_key)
                    .or_insert((0, Instant::now()));
                *counter = counter.saturating_add(1);

                if *counter < 3 && e.is_retryable_upstream() {
                    msg.enqueue_ts_ms = now_unix_ms();
                    let inbound_tx =
                        choose_inbound_tx(msg.ingress, &user_inbound_tx, &system_inbound_tx);
                    match inbound_tx.try_send(msg) {
                        Ok(()) => {}
                        Err(std::sync::mpsc::TrySendError::Full(m)) => {
                            let _ = config.pending_retry.save_pending_retry(&m);
                            log::warn!(
                                "[agent] llm retry: inbound full, pending_retry saved chat_id={}",
                                m.chat_id
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
                    req_id: Some(msg.req_id.as_deref().unwrap_or_default().to_owned()),
                    ingress: IngressKind::User,
                    enqueue_ts_ms: now_unix_ms(),
                    is_group: false,
                };
                let _ = try_send_outbound(&outbound_tx, reply, "chat-failure");
                continue;
            }
        };
        let WorkerRunTelemetry {
            streamed,
            latency: mut worker_latency,
            delivery,
            any_tool_used,
            used_final_answer_recovery,
        } = telemetry;
        let (mut reply_content, is_interrupt, reply_already_delivered, apply_finalizer) =
            match outcome {
                WorkerOutcome::Interrupt(confirm) => {
                    let cow = truncate_content_to_max(&confirm, MAX_CONTENT_LEN);
                    let s = if let std::borrow::Cow::Borrowed(_) = &cow {
                        confirm
                    } else {
                        cow.into_owned()
                    };
                    (s, true, false, false)
                }
                WorkerOutcome::Content(s) => {
                    let cow = truncate_content_to_max(&s, MAX_CONTENT_LEN);
                    let s = if let std::borrow::Cow::Borrowed(_) = &cow {
                        s
                    } else {
                        cow.into_owned()
                    };
                    (s, false, false, true)
                }
                WorkerOutcome::Delivered(s) => {
                    let cow = truncate_content_to_max(&s, MAX_CONTENT_LEN);
                    let s = if let std::borrow::Cow::Borrowed(_) = &cow {
                        s
                    } else {
                        cow.into_owned()
                    };
                    (s, false, true, false)
                }
            };
        if !is_interrupt && apply_finalizer {
            reply_content = finalize_user_visible_reply(config.strategy, &reply_content);
        }
        if !is_interrupt
            && reply_content.trim().is_empty()
            && msg.ingress == IngressKind::User
            && msg.channel.as_ref() != "cron"
        {
            reply_content = tr(UiMessage::AgentNoFinalReply, loc);
        }
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
        if !is_interrupt && !reply_content.is_empty() {
            metrics::record_final_answer_call();
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

        // 已由 delivery 侧直接交付到通道时，跳过 outbound_tx 避免重复发送。
        let outbound_start = Instant::now();
        let delivered = if reply_already_delivered {
            crate::platform::task_wdt::feed_current_task();
            true
        } else if !streamed {
            let out = PcMsg {
                channel: msg.channel.clone(),
                chat_id: msg.chat_id.clone(),
                content: reply_content.clone(),
                req_id: Some(msg.req_id.as_deref().unwrap_or_default().to_owned()),
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
        let reply_handoff_ms = if delivered {
            msg_start.elapsed().as_millis()
        } else {
            0
        };

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
        let llm_ms = worker_latency
            .context_ms
            .saturating_add(worker_latency.llm_round_total_ms)
            .saturating_add(worker_latency.tool_exec_ms)
            .saturating_add(worker_latency.session_write_ms);

        // Post-reply maintenance is best-effort and runs on the system queue so the
        // interactive lane can hand the reply off without paying for another LLM/storage pass.
        if delivered
            && !enqueue_post_reply_maintenance_job(
                &system_inbound_tx,
                &msg,
                &reply_content,
                worker_latency.tool_calls,
            )
        {
            log::debug!(
                "[agent_memory] post-reply maintenance job skipped chat_id={}",
                msg.chat_id
            );
        }
        let total_ms = msg_start.elapsed().as_millis();
        let post_reply_ms = total_ms.saturating_sub(reply_handoff_ms);
        turn_ledger.status = if is_interrupt {
            TurnLedgerStatus::Interrupted
        } else {
            TurnLedgerStatus::Answered
        };
        turn_ledger.reason = normalize_turn_reason(if is_interrupt {
            "interrupt"
        } else if reply_already_delivered || delivery.current_primary_delivered {
            "current_primary"
        } else if used_final_answer_recovery {
            "final_recovery"
        } else {
            "final_answer"
        });
        turn_ledger.reply_preview = normalize_turn_preview(&reply_content);
        turn_ledger.updated_at_ms = now_unix_ms();
        turn_ledger.finished_at_ms = turn_ledger.updated_at_ms;
        turn_ledger.react_rounds = worker_latency.react_rounds;
        turn_ledger.tool_calls = worker_latency.tool_calls;
        turn_ledger.any_tool_used = any_tool_used;
        turn_ledger.final_answer_recovered = used_final_answer_recovery;
        turn_ledger.final_reply_delivered = delivered;
        turn_ledger.reply_handoff_ms = reply_handoff_ms.min(u64::MAX as u128) as u64;
        turn_ledger.post_reply_ms = post_reply_ms.min(u64::MAX as u128) as u64;
        turn_ledger.total_ms = total_ms.min(u64::MAX as u128) as u64;
        turn_ledger.ttft_ms = worker_latency.ttft_ms.unwrap_or(0).min(u64::MAX as u128) as u64;
        turn_ledger.delivery = build_turn_delivery_ledger(delivery);
        persist_turn_ledger(
            config.turn_ledger_store.as_ref(),
            &msg.chat_id,
            &turn_ledger,
            "finish",
        );
        metrics::record_react_rounds(worker_latency.react_rounds);
        metrics::record_tool_calls_last(worker_latency.tool_calls);
        metrics::record_ttft_ms(worker_latency.ttft_ms.unwrap_or(0));
        metrics::record_e2e_ms(reply_handoff_ms);
        metrics::record_post_reply_ms(post_reply_ms);
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
                "[latency][agent:{}] req_id={} channel={} chat_id={} admission_ms={} context_ms={} llm_round_total_ms={} tool_exec_ms={} session_write_ms={} llm_ms={} outbound_enqueue_ms={} reply_handoff_ms={} post_reply_ms={} total_ms={} react_rounds={} tool_calls={} ttft_ms={} streamed={} delivered={} level=slow",
                worker_lane_tag,
                msg.req_id.as_deref().unwrap_or_default(),
                msg.channel,
                msg.chat_id,
                admission_ms,
                worker_latency.context_ms,
                worker_latency.llm_round_total_ms,
                worker_latency.tool_exec_ms,
                worker_latency.session_write_ms,
                llm_ms,
                outbound_enqueue_ms,
                reply_handoff_ms,
                post_reply_ms,
                total_ms,
                worker_latency.react_rounds,
                worker_latency.tool_calls,
                worker_latency.ttft_ms.unwrap_or(0),
                streamed,
                delivered
            );
        } else {
            log::info!(
                "[latency][agent:{}] req_id={} channel={} chat_id={} admission_ms={} context_ms={} llm_round_total_ms={} tool_exec_ms={} session_write_ms={} llm_ms={} outbound_enqueue_ms={} reply_handoff_ms={} post_reply_ms={} total_ms={} react_rounds={} tool_calls={} ttft_ms={} streamed={} delivered={}",
                worker_lane_tag,
                msg.req_id.as_deref().unwrap_or_default(),
                msg.channel,
                msg.chat_id,
                admission_ms,
                worker_latency.context_ms,
                worker_latency.llm_round_total_ms,
                worker_latency.tool_exec_ms,
                worker_latency.session_write_ms,
                llm_ms,
                outbound_enqueue_ms,
                reply_handoff_ms,
                post_reply_ms,
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

/// 完整 context + worker LLM + ReAct 循环，返回 (WorkerOutcome, telemetry)。不写 session，由调用方写。
/// telemetry.streamed=true 表示已通过流式编辑发送到通道，调用方应跳过 outbound_tx。
#[allow(clippy::too_many_arguments)]
fn run_worker_path(
    http: &mut dyn PlatformHttpClient,
    worker_llm: &(dyn LlmClient + Send + Sync),
    msg: &crate::bus::PcMsg,
    outbound_tx: &OutboundTx,
    req_id: &str,
    registry: &crate::tools::ToolRegistry,
    config: &AgentLoopConfig,
    tool_call_repeat: &mut HashMap<u64, u8>,
    loc: UiLocale,
) -> Result<(WorkerOutcome, WorkerRunTelemetry)> {
    let mut latency = WorkerLatency::default();
    let worker_start = Instant::now();
    let request_plan = AgentRequestPlan::build(msg, registry, worker_llm, config.strategy);
    let has_tools = request_plan.has_tools();
    let mut tool_ctx = HttpClientToolContext {
        http,
        chat_id: Some(msg.chat_id.clone()),
        channel: Some(msg.channel.clone()),
        outbound_tx: Some(outbound_tx),
        req_id: Some(req_id),
        supports_current_chat_primary_reply: false,
        supports_explicit_outbound_message: false,
        outbound_message_budget: 2,
        outbound_message_count: 0,
        current_primary_message_delivered: false,
        locale: loc,
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
    let post_memory_tail_len = estimate_post_memory_system_tail_len(PostMemoryTailParams {
        has_tools,
        skill_descriptions: &skill_descriptions,
        is_group: msg.is_group,
        group_activation: config.tg_group_activation.as_ref(),
        emotion_signal_suffix,
        runtime: Some(runtime),
        llm_hint: budget.llm_hint,
    });
    let prompt_memory = load_prompt_memory_context(PromptMemoryContextParams {
        chat_id: &msg.chat_id,
        user_query: &msg.content,
        system_max_len: budget
            .system_prompt_max
            .saturating_sub(post_memory_tail_len),
        profile: config.memory_profile,
        session_store: config.session_store.as_ref(),
        session_summary_store: config.session_summary_store.as_ref(),
        long_term_memory_store: config.long_term_memory_store.as_ref(),
        execution_state_store: config.execution_state_store.as_ref(),
    });
    let (mut system, mut messages) = build_context(&super::ContextParams {
        msg,
        memory: config.memory_store.as_ref(),
        session: config.session_store.as_ref(),
        important_message_store: config.important_message_store.as_ref(),
        has_tools,
        skill_descriptions: &skill_descriptions,
        system_max_len: budget.system_prompt_max,
        messages_max_len: budget.messages_max,
        session_max_messages: config.session_max_messages,
        group_activation: config.tg_group_activation.as_ref(),
        emotion_signal_suffix,
        execution_state_text: prompt_memory.execution_state_text.as_deref(),
        long_term_memory_text: prompt_memory.long_term_memory_text.as_deref(),
        summary_text: prompt_memory.summary_text.as_deref(),
        recent_messages: (!prompt_memory.recent_messages.is_empty())
            .then_some(prompt_memory.recent_messages.as_slice()),
        runtime: Some(runtime),
        llm_hint: budget.llm_hint,
    })
    .map_err(|e| e.with_stage("agent_context"))?;
    latency.context_ms = context_start.elapsed().as_millis();
    request_plan.apply_system_prompt(&mut system, budget.system_prompt_max);
    let mut system_scratch =
        String::with_capacity(system.len().saturating_add(PLAN_SYSTEM_SUFFIX.len()));
    if config.strategy.enables_preplanning()
        && should_generate_execution_plan(msg, has_tools, snapshot.pressure)
    {
        let planning_system =
            prepare_system_with_suffix(&system, PLAN_SYSTEM_SUFFIX, &mut system_scratch);
        match worker_llm.chat(
            &mut tool_ctx,
            planning_system,
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
    let mut memory_grounding: Option<String> = None;
    let mut tool_result_user_content = String::with_capacity(1024);
    let mut round_call_keys = Vec::with_capacity(4);
    let mut round_evidence_lines = Vec::with_capacity(MAX_TOOL_EVIDENCE_ITEMS);
    // 流式编辑状态（跨 ReAct 轮次共享）。
    let editor = if config.llm_stream
        && config.stream_editor_channel.as_deref() == Some(msg.channel.as_ref())
    {
        config.stream_editor.as_deref()
    } else {
        None
    };
    tool_ctx.supports_current_chat_primary_reply =
        msg.ingress == IngressKind::User && msg.channel.as_ref() != "voice";
    let mut delivery = DeliverySession::new(msg, req_id, outbound_tx, editor, loc);
    // P1 Enhancement 3: 进度跟踪（最近3轮），用于检测无效循环。
    let mut progress_history: [Option<RoundProgress>; 3] = [None; 3];
    let mut any_tool_used = false; // 本次请求是否使用过任何工具
    let mut end_turn_followup_used = false;
    let mut recent_tool_round = RecentToolRoundState::default();
    let mut delivered_current_chat_reply: Option<String> = None;
    let mut used_final_answer_recovery = false;

    for round in 0..MAX_REACT_ROUNDS {
        latency.react_rounds = round as u32 + 1;
        // Inter-round pressure check: skip first round (already gated by caller).
        // Use stale-refresh instead of unconditional resample so the runtime
        // does not thrash the global snapshot between tightly packed rounds.
        if round > 0 {
            match crate::orchestrator::refresh_heap_if_stale() {
                crate::orchestrator::PressureLevel::Normal => {}
                crate::orchestrator::PressureLevel::Cautious
                | crate::orchestrator::PressureLevel::Critical => {
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
        let round_tools = request_plan.request_tools();
        let response = if config.llm_stream {
            let progress_base = worker_start;
            let llm_tool_choice = request_plan.tool_choice(round, any_tool_used);
            let mut progress_cb = |_delta: &str, accumulated: &str| {
                crate::platform::task_wdt::feed_current_task();
                if !first_token_marked && !accumulated.is_empty() {
                    latency.ttft_ms = Some(progress_base.elapsed().as_millis());
                    first_token_marked = true;
                }
                if matches!(
                    crate::orchestrator::current_pressure(),
                    crate::orchestrator::PressureLevel::Critical
                ) {
                    return;
                }
                delivery.on_stream_delta(accumulated);
            };
            worker_llm.chat_with_progress(
                &mut tool_ctx,
                &system,
                &messages,
                round_tools,
                llm_tool_choice,
                &mut progress_cb,
            )
        } else {
            let llm_tool_choice = request_plan.tool_choice(round, any_tool_used);
            worker_llm.chat(
                &mut tool_ctx,
                &system,
                &messages,
                round_tools,
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
        let response = request_plan.recover_response(response);
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
            mark_ttft_if_visible(&mut latency, worker_start, &content);
            final_content = content;
            break;
        }

        if response.stop_reason == StopReason::EndTurn {
            let content = response.content;
            if content.contains(AGENT_MARKER_STOP) {
                let confirmation = strip_agent_stop_confirmation(&content);
                mark_ttft_if_visible(&mut latency, worker_start, &confirmation);
                let streamed = delivery.finalize(&confirmation);
                let telemetry = WorkerRunTelemetry {
                    streamed,
                    latency,
                    delivery: delivery.report(),
                    any_tool_used,
                    used_final_answer_recovery,
                };
                return Ok((WorkerOutcome::Interrupt(confirmation), telemetry));
            }
            if let Some(followup) =
                empty_final_answer_followup(config.strategy, any_tool_used, &content)
            {
                enqueue_end_turn_followup(&mut messages, &mut progress_history, &content, followup);
                continue;
            }
            if let Some((followup, consume_single_use_budget)) =
                resolve_end_turn_followup(EndTurnFollowupContext {
                    request_plan: &request_plan,
                    strategy: config.strategy,
                    round,
                    any_tool_used,
                    end_turn_followup_used,
                    recent_tool_round: &recent_tool_round,
                    messages: &messages,
                    content: &content,
                })
            {
                enqueue_end_turn_followup(
                    &mut messages,
                    &mut progress_history,
                    &content,
                    &followup,
                );
                if consume_single_use_budget {
                    end_turn_followup_used = true;
                }
                continue;
            }

            mark_ttft_if_visible(&mut latency, worker_start, &content);
            final_content = content;
            break;
        }

        if response.stop_reason == StopReason::ToolUse {
            let tool_calls = response.tool_calls.as_deref().unwrap_or(&[]);
            if tool_calls.is_empty() {
                mark_ttft_if_visible(&mut latency, worker_start, &response.content);
                final_content = response.content;
                break;
            }
            if !response.content.trim().is_empty() && response.content.trim() != "[tool_use]" {
                mark_ttft_if_visible(&mut latency, worker_start, &response.content);
                delivery.emit_partial(&response.content);
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
            tool_result_user_content.clear();
            if tool_result_user_content.capacity() < cap {
                tool_result_user_content.reserve(cap - tool_result_user_content.capacity());
            }
            tool_result_user_content.push_str(TOOL_RESULTS_PREFIX);
            let mut truncated = false;
            latency.tool_calls = latency.tool_calls.saturating_add(tool_calls.len() as u32);
            // P1 Enhancement 3: 跟踪本轮工具是否有成功。
            let mut round_tool_success = false;
            let mut round_repeat_count = 0usize;
            round_call_keys.clear();
            if round_call_keys.capacity() < tool_calls.len() {
                round_call_keys.reserve(tool_calls.len() - round_call_keys.capacity());
            }
            let mut round_failure_summary = ToolFailureSummary::default();
            round_evidence_lines.clear();
            let mut omitted_evidence_count = 0usize;
            let mut round_observations = SuccessfulToolRoundObservations::default();
            for (i, tc) in tool_calls.iter().enumerate() {
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
                delivery.emit_progress(&progress);
                // 工具执行门控
                let (result_owned, failure_kind, delivered_reply) = {
                    if !registry.is_llm_tool_visible(&tc.name, request_plan.policy()) {
                        metrics::record_tool_call(false);
                        let assessment = unavailable_tool_assessment();
                        (
                            crate::util::scrub_credentials(
                                &serde_json::json!({
                                    "error": format!(
                                        "tool '{}' is not available in the current runtime context",
                                        tc.name
                                    )
                                })
                                .to_string(),
                            ),
                            Some(assessment.kind),
                            None,
                        )
                    } else {
                        let needs_net = registry.is_network_tool(&tc.name);
                        match crate::orchestrator::can_execute_tool_pub(&tc.name, needs_net) {
                            ToolDecision::Deny { reason } => {
                                log::info!("[agent_tool] {} denied: {}", tc.name, reason);
                                let assessment = denied_tool_assessment(reason);
                                (
                                    crate::util::scrub_credentials(
                                        &serde_json::json!({ "error": reason }).to_string(),
                                    ),
                                    Some(assessment.kind),
                                    None,
                                )
                            }
                            ToolDecision::Allow => {
                                let tool_exec_start = Instant::now();
                                match registry.execute(&tc.name, &tc.input, &mut tool_ctx) {
                                    Ok(outcome) => {
                                        latency.tool_exec_ms = latency
                                            .tool_exec_ms
                                            .saturating_add(tool_exec_start.elapsed().as_millis());
                                        if let Some(reply) = outcome.current_chat_reply {
                                            match delivery.deliver_current_primary(&reply.content) {
                                                Ok(true) => {
                                                    metrics::record_tool_call(true);
                                                    round_tool_success = true;
                                                    any_tool_used = true;
                                                    (
                                                        crate::util::scrub_credentials(
                                                            &outcome.content,
                                                        ),
                                                        None,
                                                        Some(reply.content),
                                                    )
                                                }
                                                Ok(false) => {
                                                    metrics::record_tool_call(false);
                                                    let assessment = classify_tool_error(
                                                        &crate::error::Error::config(
                                                            "current_chat_delivery",
                                                            "current-chat primary reply was empty or not deliverable",
                                                        ),
                                                    );
                                                    tool_error_buf.clear();
                                                    let _ = write!(
                                                        &mut tool_error_buf,
                                                        "[tool error] failed to deliver current-chat primary reply.{}",
                                                        assessment.hint
                                                    );
                                                    (
                                                        crate::util::scrub_credentials(
                                                            tool_error_buf.as_str(),
                                                        ),
                                                        Some(assessment.kind),
                                                        None,
                                                    )
                                                }
                                                Err(e) => {
                                                    metrics::record_tool_call(false);
                                                    metrics::record_error_by_stage(e.stage());
                                                    log::error!(
                                                        "[agent_tool] {} current-chat delivery failed: {}",
                                                        tc.name,
                                                        e
                                                    );
                                                    state::set_last_error(&e);
                                                    let assessment = classify_tool_error(&e);
                                                    tool_error_buf.clear();
                                                    let _ = write!(
                                                        &mut tool_error_buf,
                                                        "[tool error] {}.{}",
                                                        e, assessment.hint
                                                    );
                                                    (
                                                        crate::util::scrub_credentials(
                                                            tool_error_buf.as_str(),
                                                        ),
                                                        Some(assessment.kind),
                                                        None,
                                                    )
                                                }
                                            }
                                        } else {
                                            metrics::record_tool_call(true);
                                            round_tool_success = true;
                                            any_tool_used = true;
                                            (
                                                crate::util::scrub_credentials(&outcome.content),
                                                None,
                                                None,
                                            )
                                        }
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
                                        let assessment = classify_tool_error(&e);
                                        let _ = write!(
                                            &mut tool_error_buf,
                                            "[tool error] {}.{}",
                                            e, assessment.hint
                                        );
                                        (
                                            crate::util::scrub_credentials(tool_error_buf.as_str()),
                                            Some(assessment.kind),
                                            None,
                                        )
                                    }
                                }
                            }
                        }
                    }
                };
                if let Some(reply) = delivered_reply {
                    delivered_current_chat_reply = Some(reply);
                }
                let result_view = result_owned.as_str();
                if let Some(kind) = failure_kind {
                    round_failure_summary.record(kind);
                } else if config.strategy == AgentRunStrategy::LinuxEnhanced {
                    record_successful_tool_result(&mut round_observations, &tc.name, result_view);
                    if round_evidence_lines.len() < MAX_TOOL_EVIDENCE_ITEMS {
                        if let Some(line) = build_tool_evidence_line(&tc.id, &tc.name, result_view)
                        {
                            round_evidence_lines.push(line);
                        }
                    } else {
                        omitted_evidence_count = omitted_evidence_count.saturating_add(1);
                    }
                }
                let call_key = hash_tool_call(&tc.name, &tc.input);
                round_call_keys.push(call_key);
                let n = tool_call_repeat.entry(call_key).or_insert(0);
                *n = (*n).saturating_add(1);
                let repeat_count = *n as usize;
                if *n >= 2 {
                    round_repeat_count = round_repeat_count.saturating_add(1);
                }
                crate::platform::task_wdt::feed_current_task();
                if i > 0
                    && push_bounded_utf8(
                        &mut tool_result_user_content,
                        "\n",
                        MAX_TOOL_RESULTS_USER_MESSAGE_LEN,
                    )
                {
                    truncated = true;
                    break;
                }
                if append_tool_result_block(
                    &mut tool_result_user_content,
                    ToolResultBlock {
                        call_id: &tc.id,
                        tool_name: &tc.name,
                        status: tool_result_status_attr(failure_kind.is_some()),
                        failure: failure_kind_attr(failure_kind),
                        repeat_count,
                        content: result_view,
                    },
                    MAX_TOOL_RESULTS_USER_MESSAGE_LEN,
                ) {
                    truncated = true;
                    break;
                }
            }
            if !round_evidence_lines.is_empty() {
                if !tool_result_user_content.ends_with('\n') {
                    let _ = push_bounded_utf8(
                        &mut tool_result_user_content,
                        "\n",
                        MAX_TOOL_RESULTS_USER_MESSAGE_LEN,
                    );
                }
                if append_tool_evidence_summary_block(
                    &mut tool_result_user_content,
                    &round_evidence_lines,
                    omitted_evidence_count,
                    MAX_TOOL_RESULTS_USER_MESSAGE_LEN,
                ) {
                    truncated = true;
                }
            }
            let round_signature = hash_tool_round(&round_call_keys);
            recent_tool_round.record_round(
                tool_calls.len(),
                round_tool_success,
                round_signature,
                round_failure_summary,
            );
            let ping_pong_detected = recent_tool_round.ping_pong_detected();
            let round_guidance = if round_tool_success {
                merge_tool_round_guidance(
                    build_success_tool_round_guidance(
                        config.strategy,
                        tool_calls.len(),
                        round_failure_summary,
                    ),
                    build_success_tool_execution_guidance(config.strategy, &round_observations),
                )
            } else {
                build_tool_round_guidance(
                    config.strategy,
                    round_tool_success,
                    recent_tool_round.consecutive_stalled_rounds,
                    tool_calls.len(),
                    round_repeat_count,
                    round_failure_summary,
                    ping_pong_detected,
                )
            };
            if let Some(guidance) = round_guidance {
                if !tool_result_user_content.ends_with('\n') {
                    let _ = push_bounded_utf8(
                        &mut tool_result_user_content,
                        "\n",
                        MAX_TOOL_RESULTS_USER_MESSAGE_LEN,
                    );
                }
                if append_tool_round_guidance_block(
                    &mut tool_result_user_content,
                    &guidance,
                    MAX_TOOL_RESULTS_USER_MESSAGE_LEN,
                ) {
                    truncated = true;
                }
            }
            if memory_grounding.is_none() {
                memory_grounding = build_memory_grounding_text(
                    prompt_memory.summary_text.as_deref(),
                    prompt_memory.long_term_memory_text.as_deref(),
                );
            }
            if let Some(memory_grounding) = memory_grounding.as_deref() {
                if !tool_result_user_content.ends_with('\n') {
                    let _ = push_bounded_utf8(
                        &mut tool_result_user_content,
                        "\n",
                        MAX_TOOL_RESULTS_USER_MESSAGE_LEN,
                    );
                }
                if append_memory_grounding_block(
                    &mut tool_result_user_content,
                    memory_grounding,
                    MAX_TOOL_RESULTS_USER_MESSAGE_LEN,
                ) {
                    truncated = true;
                }
            }
            if truncated && tool_result_user_content.len() < MAX_TOOL_RESULTS_USER_MESSAGE_LEN {
                let _ = push_bounded_utf8(
                    &mut tool_result_user_content,
                    "\n[truncated]",
                    MAX_TOOL_RESULTS_USER_MESSAGE_LEN,
                );
            }
            messages.push(Message {
                role: Cow::Borrowed("user"),
                content: std::mem::take(&mut tool_result_user_content),
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
            let streamed = delivery.finalize(&confirmation);
            let telemetry = WorkerRunTelemetry {
                streamed,
                latency,
                delivery: delivery.report(),
                any_tool_used,
                used_final_answer_recovery,
            };
            return Ok((WorkerOutcome::Interrupt(confirmation), telemetry));
        }
        final_content = content;
        break;
    }
    if final_content.trim().is_empty() && any_tool_used && delivered_current_chat_reply.is_none() {
        used_final_answer_recovery = true;
        final_content = run_final_answer_recovery_round(
            worker_llm,
            &mut tool_ctx,
            &system,
            &messages,
            config.llm_stream,
            &mut latency,
            &mut system_scratch,
        )?;
    }
    let streamed = delivery.finalize(&final_content);
    let outcome = if let Some(reply) = delivered_current_chat_reply {
        WorkerOutcome::Delivered(reply)
    } else {
        WorkerOutcome::Content(final_content)
    };
    Ok((
        outcome,
        WorkerRunTelemetry {
            streamed,
            latency,
            delivery: delivery.report(),
            any_tool_used,
            used_final_answer_recovery,
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Result;
    use crate::llm::{LlmHttpClient, LlmModelCompat, LlmResponse, StopReason, ToolChoicePolicy};
    use crate::memory::{
        EmotionSignalStore, ExecutionState, ExecutionStateStore, ImportantMessageStore,
        LongTermMemoryDraft, LongTermMemoryEntry, LongTermMemoryExtractionState,
        LongTermMemoryExtractionStateStore, LongTermMemorySlot, LongTermMemoryStore, MemoryStore,
        PendingRetryStore, SessionMessage, SessionStore, SessionSummaryStore, TurnLedger,
        TurnLedgerStore,
    };
    use crate::platform::{PlatformHttpClient, ResponseBody};
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct DummyPlatformHttp;

    impl PlatformHttpClient for DummyPlatformHttp {
        fn get(&mut self, _url: &str, _headers: &[(&str, &str)]) -> Result<(u16, ResponseBody)> {
            Ok((200, ResponseBody::Heap(Vec::new())))
        }

        fn post(
            &mut self,
            _url: &str,
            _headers: &[(&str, &str)],
            _body: &[u8],
        ) -> Result<(u16, ResponseBody)> {
            Ok((200, ResponseBody::Heap(Vec::new())))
        }
    }

    #[derive(Clone)]
    struct ObservedRecoveryRequest {
        system: String,
        tool_count: usize,
    }

    struct RecoveryStubLlm {
        observed: Arc<Mutex<Vec<ObservedRecoveryRequest>>>,
        response: LlmResponse,
    }

    impl LlmClient for RecoveryStubLlm {
        fn model_compat(&self) -> LlmModelCompat {
            LlmModelCompat::default()
        }

        fn chat(
            &self,
            _http: &mut dyn LlmHttpClient,
            system: &str,
            _messages: &[Message],
            tools: Option<&[crate::llm::ToolSpec]>,
            _tool_choice: ToolChoicePolicy,
        ) -> Result<LlmResponse> {
            self.observed
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(ObservedRecoveryRequest {
                    system: system.to_string(),
                    tool_count: tools.map_or(0, |specs| specs.len()),
                });
            Ok(self.response.clone())
        }
    }

    #[derive(Default)]
    struct EmptyMemoryStore;

    impl MemoryStore for EmptyMemoryStore {
        fn get_memory(&self) -> Result<String> {
            Ok(String::new())
        }
        fn set_memory(&self, _content: &str) -> Result<()> {
            Ok(())
        }
        fn get_soul(&self) -> Result<String> {
            Ok(String::new())
        }
        fn set_soul(&self, _content: &str) -> Result<()> {
            Ok(())
        }
        fn get_user(&self) -> Result<String> {
            Ok(String::new())
        }
        fn set_user(&self, _content: &str) -> Result<()> {
            Ok(())
        }
        fn list_daily_note_names(&self, _recent_n: usize) -> Result<Vec<String>> {
            Ok(Vec::new())
        }
        fn get_daily_note(&self, _name: &str) -> Result<String> {
            Ok(String::new())
        }
        fn write_daily_note(&self, _name: &str, _content: &str) -> Result<()> {
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubSessionStore {
        entries: Mutex<HashMap<String, Vec<SessionMessage>>>,
    }

    impl SessionStore for StubSessionStore {
        fn append(&self, chat_id: &str, role: &str, content: &str) -> Result<()> {
            self.entries
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .entry(chat_id.to_string())
                .or_default()
                .push(SessionMessage {
                    role: role.to_string(),
                    content: content.to_string(),
                });
            Ok(())
        }

        fn load_recent(&self, chat_id: &str, n: usize) -> Result<Vec<SessionMessage>> {
            let entries = self.entries.lock().unwrap_or_else(|e| e.into_inner());
            let values = entries.get(chat_id).cloned().unwrap_or_default();
            let start = values.len().saturating_sub(n);
            Ok(values[start..].to_vec())
        }

        fn clear(&self, chat_id: &str) -> Result<()> {
            self.entries
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(chat_id);
            Ok(())
        }

        fn list_chat_ids(&self) -> Result<Vec<String>> {
            Ok(self
                .entries
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .keys()
                .cloned()
                .collect())
        }
    }

    #[derive(Default)]
    struct StubSessionSummaryStore;

    impl SessionSummaryStore for StubSessionSummaryStore {
        fn get(&self, _chat_id: &str) -> Result<Option<String>> {
            Ok(None)
        }
        fn set(&self, _chat_id: &str, _summary: &str) -> Result<()> {
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubExecutionStateStore;

    impl ExecutionStateStore for StubExecutionStateStore {
        fn get(&self, _chat_id: &str) -> Result<Option<ExecutionState>> {
            Ok(None)
        }
        fn set(&self, _chat_id: &str, _state: &ExecutionState) -> Result<()> {
            Ok(())
        }
        fn clear(&self, _chat_id: &str) -> Result<()> {
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubLongTermMemoryStore;

    impl LongTermMemoryStore for StubLongTermMemoryStore {
        fn upsert_many(&self, _drafts: &[LongTermMemoryDraft], _now_secs: u64) -> Result<usize> {
            Ok(0)
        }
        fn recall(
            &self,
            _query: &str,
            _source_chat_id: Option<&str>,
            _limit: usize,
        ) -> Result<Vec<LongTermMemoryEntry>> {
            Ok(Vec::new())
        }
        fn get(&self, _id: &str) -> Result<Option<LongTermMemoryEntry>> {
            Ok(None)
        }
        fn list(&self, _limit: usize) -> Result<Vec<LongTermMemoryEntry>> {
            Ok(Vec::new())
        }
        fn delete(&self, _id: &str) -> Result<bool> {
            Ok(false)
        }
        fn delete_slot(&self, _slot: &LongTermMemorySlot) -> Result<bool> {
            Ok(false)
        }
        fn count(&self) -> Result<usize> {
            Ok(0)
        }
    }

    #[derive(Default)]
    struct StubLongTermMemoryExtractionStateStore;

    impl LongTermMemoryExtractionStateStore for StubLongTermMemoryExtractionStateStore {
        fn get(&self, _chat_id: &str) -> Result<Option<LongTermMemoryExtractionState>> {
            Ok(None)
        }
        fn set(&self, _chat_id: &str, _state: &LongTermMemoryExtractionState) -> Result<()> {
            Ok(())
        }
        fn clear(&self, _chat_id: &str) -> Result<()> {
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubImportantMessageStore;

    impl ImportantMessageStore for StubImportantMessageStore {
        fn set_important_offset_from_end(
            &self,
            _chat_id: &str,
            _offset_from_end: u32,
        ) -> Result<()> {
            Ok(())
        }
        fn get_important_offset(&self, _chat_id: &str) -> Result<Option<u32>> {
            Ok(None)
        }
        fn clear_important(&self, _chat_id: &str) -> Result<()> {
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubPendingRetryStore;

    impl PendingRetryStore for StubPendingRetryStore {
        fn save_pending_retry(&self, _msg: &PcMsg) -> Result<()> {
            Ok(())
        }
        fn load_pending_retry(&self) -> Result<Option<PcMsg>> {
            Ok(None)
        }
        fn clear_pending_retry(&self) -> Result<()> {
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubEmotionSignalStore;

    impl EmotionSignalStore for StubEmotionSignalStore {
        fn set(&self, _chat_id: &str, _signal: &str) -> Result<()> {
            Ok(())
        }
        fn get_then_clear(&self, _chat_id: &str) -> Result<Option<String>> {
            Ok(None)
        }
    }

    struct StubTurnLedgerStore;

    impl TurnLedgerStore for StubTurnLedgerStore {
        fn get(&self, _chat_id: &str) -> Result<Option<TurnLedger>> {
            Ok(None)
        }

        fn set(&self, _chat_id: &str, _ledger: &TurnLedger) -> Result<()> {
            Ok(())
        }

        fn clear(&self, _chat_id: &str) -> Result<()> {
            Ok(())
        }
    }

    struct SequenceStubLlm {
        responses: Mutex<Vec<LlmResponse>>,
    }

    impl LlmClient for SequenceStubLlm {
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
            let mut responses = self.responses.lock().unwrap_or_else(|e| e.into_inner());
            if responses.is_empty() {
                return Ok(LlmResponse {
                    content: String::new(),
                    stop_reason: StopReason::EndTurn,
                    tool_calls: None,
                });
            }
            Ok(responses.remove(0))
        }
    }

    fn test_agent_loop_config() -> AgentLoopConfig {
        AgentLoopConfig {
            memory_store: Arc::new(EmptyMemoryStore),
            long_term_memory_store: Arc::new(StubLongTermMemoryStore),
            long_term_memory_extraction_state_store: Arc::new(
                StubLongTermMemoryExtractionStateStore,
            ),
            session_store: Arc::new(StubSessionStore::default()),
            session_summary_store: Arc::new(StubSessionSummaryStore),
            execution_state_store: Arc::new(StubExecutionStateStore),
            turn_ledger_store: Arc::new(StubTurnLedgerStore),
            memory_profile: crate::memory::MemoryProfile::Embedded,
            get_skill_descriptions: Arc::new(String::new),
            session_max_messages: 16,
            tg_group_activation: Arc::from(""),
            important_message_store: Arc::new(StubImportantMessageStore),
            emotion_signal_store: Arc::new(StubEmotionSignalStore),
            pending_retry: Arc::new(StubPendingRetryStore),
            strategy: AgentRunStrategy::Embedded,
            llm_stream: false,
            stream_editor: None,
            stream_editor_channel: None,
            resolve_locale: Arc::new(|| UiLocale::Zh),
        }
    }

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
    fn summarize_tool_results_keeps_tail_for_long_legacy_output() {
        let input = "Tool results:\n[call_1]: prefix text ".to_string()
            + &"x".repeat(120)
            + " tail-marker-404";
        let summary = summarize_tool_results(&input);
        assert!(summary.contains("prefix text"));
        assert!(summary.contains("tail-marker-404"));
        assert!(summary.contains(" ... "));
    }

    #[test]
    fn summarize_tool_results_understands_structured_tool_result_blocks() {
        let input = concat!(
            "Tool results:\n",
            "<tool_result id=\"call_1\" tool=\"web_search\" status=\"error\" failure=\"permanent\" repeat_count=\"2\">\n",
            "[tool error] Resource not found.\n",
            "</tool_result>\n",
        );
        let summary = summarize_tool_results(input);
        assert!(summary.contains("[call_1] web_search status=error failure=permanent repeat=2"));
        assert!(summary.contains("Resource not found"));
    }

    #[test]
    fn summarize_tool_results_keeps_tail_for_long_structured_blocks() {
        let input = concat!(
            "Tool results:\n",
            "<tool_result id=\"call_1\" tool=\"read_file\" status=\"ok\">\n",
            "head section ",
            "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx ",
            "tail-path=/tmp/final.log\n",
            "</tool_result>\n",
        );
        let summary = summarize_tool_results(input);
        assert!(summary.contains("head section"));
        assert!(summary.contains("tail-path=/tmp/final.log"));
        assert!(summary.contains(" ... "));
    }

    #[test]
    fn summarize_tool_results_keeps_round_guidance_summary() {
        let input = concat!(
            "Tool results:\n",
            "<tool_round_guidance>\n",
            "[SYSTEM] Explain the blocker clearly.\n",
            "</tool_round_guidance>\n",
        );
        let summary = summarize_tool_results(input);
        assert!(summary.contains("[guidance]"));
        assert!(summary.contains("Explain the blocker clearly"));
    }

    #[test]
    fn summarize_tool_results_keeps_tool_evidence_summary() {
        let input = concat!(
            "Tool results:\n",
            "<tool_evidence_summary>\n",
            "- [call_1] read_file: version = 1.2.3\n",
            "- [call_2] web_search: release date 2026-03-31\n",
            "</tool_evidence_summary>\n",
        );
        let summary = summarize_tool_results(input);
        assert!(summary.contains("[evidence] - [call_1] read_file: version = 1.2.3"));
        assert!(summary.contains("[evidence] - [call_2] web_search: release date 2026-03-31"));
    }

    #[test]
    fn summarize_tool_results_keeps_memory_grounding_summary() {
        let input = concat!(
            "Tool results:\n",
            "<memory_grounding>\n",
            "[summary] 用户偏好直接回答\n",
            "[long_term] - [project:current_project] 继续收口长期记忆\n",
            "</memory_grounding>\n",
        );
        let summary = summarize_tool_results(input);
        assert!(summary.contains("[memory]"));
        assert!(summary.contains("用户偏好直接回答"));
        assert!(summary.contains("current_project"));
    }

    #[test]
    fn summarize_tool_results_keeps_structured_blocks_together() {
        let input = concat!(
            "Tool results:\n",
            "<tool_result id=\"call_1\" tool=\"read_file\" status=\"ok\">\n",
            "version = 1.2.3\n",
            "</tool_result>\n",
            "<tool_evidence_summary>\n",
            "- [call_1] read_file: version = 1.2.3\n",
            "</tool_evidence_summary>\n",
            "<tool_round_guidance>\n",
            "[SYSTEM] Use the evidence above to answer directly.\n",
            "</tool_round_guidance>\n",
            "<memory_grounding>\n",
            "[summary] 用户偏好直接回答\n",
            "[long_term] - [project:current_project] 继续收口长期记忆\n",
            "</memory_grounding>\n",
        );
        let summary = summarize_tool_results(input);
        assert!(summary.contains("[call_1] read_file status=ok: version = 1.2.3"));
        assert!(summary.contains("[evidence] - [call_1] read_file: version = 1.2.3"));
        assert!(summary.contains("[guidance] [SYSTEM] Use the evidence above to answer directly."));
        assert!(summary.contains("[memory] [summary] 用户偏好直接回答"));
    }

    #[test]
    fn build_memory_grounding_text_keeps_summary_and_long_term_bullets() {
        let grounding = build_memory_grounding_text(
            Some("用户喜欢直接、技术化的回答。"),
            Some(
                "## Long-term memory\n### Active context\n- [project:current_project] 继续收口长期记忆\n### User profile\n- [preference:response_style] 喜欢直接回答",
            ),
        )
        .expect("grounding");
        assert!(grounding.contains("[summary]"));
        assert!(grounding.contains("[long_term]"));
        assert!(grounding.contains("current_project"));
        assert!(grounding.contains("response_style"));
    }

    #[test]
    fn tool_evidence_preview_keeps_head_and_tail_for_long_results() {
        let preview = build_tool_evidence_preview(
            "first line has the important context and then a lot of filler text keeps going for quite a while across this synthetic long sample so the preview must shrink the middle and still preserve the tail-marker-XYZ",
        )
        .expect("preview");
        assert!(preview.contains("first line"));
        assert!(preview.contains("tail-marker-XYZ"));
        assert!(preview.contains(" ... "));
    }

    #[test]
    fn compact_early_tool_rounds_keeps_assistant_tail_context() {
        let mut messages = vec![
            Message {
                role: Cow::Borrowed("user"),
                content: "latest request".to_string(),
            },
            Message {
                role: Cow::Borrowed("assistant"),
                content: "head analysis ".to_string()
                    + &"x".repeat(220)
                    + " final decision: use file /tmp/result.json",
            },
            Message {
                role: Cow::Borrowed("assistant"),
                content: "recent assistant".to_string(),
            },
            Message {
                role: Cow::Borrowed("user"),
                content: "recent user followup".to_string(),
            },
            Message {
                role: Cow::Borrowed("assistant"),
                content: "latest assistant".to_string(),
            },
            Message {
                role: Cow::Borrowed("user"),
                content: "latest tool results".to_string(),
            },
        ];
        compact_early_tool_rounds(&mut messages, 1);
        assert!(messages[1].content.contains("head analysis"));
        assert!(messages[1]
            .content
            .contains("final decision: use file /tmp/result.json"));
        assert!(messages[1].content.contains("[compressed]"));
        assert!(messages[1].content.contains(" ... "));
    }

    #[test]
    fn compact_early_tool_rounds_leaves_recent_assistant_messages_intact() {
        let original = "recent assistant should stay whole".to_string();
        let mut messages = vec![
            Message {
                role: Cow::Borrowed("user"),
                content: "latest request".to_string(),
            },
            Message {
                role: Cow::Borrowed("assistant"),
                content: "older assistant ".to_string() + &"y".repeat(220),
            },
            Message {
                role: Cow::Borrowed("assistant"),
                content: original.clone(),
            },
            Message {
                role: Cow::Borrowed("user"),
                content: "recent user followup".to_string(),
            },
            Message {
                role: Cow::Borrowed("assistant"),
                content: "latest assistant".to_string(),
            },
            Message {
                role: Cow::Borrowed("user"),
                content: "latest tool results".to_string(),
            },
        ];
        compact_early_tool_rounds(&mut messages, 1);
        assert_eq!(messages[2].content, original);
    }

    #[test]
    fn final_answer_recovery_round_disables_tools_and_uses_recovery_suffix() {
        let observed = Arc::new(Mutex::new(Vec::new()));
        let llm = RecoveryStubLlm {
            observed: Arc::clone(&observed),
            response: LlmResponse {
                content: "最终答案".to_string(),
                stop_reason: StopReason::EndTurn,
                tool_calls: None,
            },
        };
        let mut http = DummyPlatformHttp;
        let mut tool_ctx = HttpClientToolContext {
            http: &mut http,
            chat_id: Some(Arc::from("chat-1")),
            channel: Some(Arc::from("qq_channel")),
            outbound_tx: None,
            req_id: None,
            supports_current_chat_primary_reply: false,
            supports_explicit_outbound_message: false,
            outbound_message_budget: 0,
            outbound_message_count: 0,
            current_primary_message_delivered: false,
            locale: UiLocale::Zh,
        };
        let messages = vec![Message {
            role: Cow::Borrowed("user"),
            content: concat!(
                "Tool results:\n",
                "<tool_result id=\"call_1\" tool=\"get_time\" status=\"ok\">\n",
                "2026-04-01T06:45:39Z\n",
                "</tool_result>\n",
            )
            .to_string(),
        }];
        let mut latency = WorkerLatency::default();
        let mut system_scratch = String::new();

        let content = run_final_answer_recovery_round(
            &llm,
            &mut tool_ctx,
            "base system",
            &messages,
            false,
            &mut latency,
            &mut system_scratch,
        )
        .expect("recovery round should succeed");

        assert_eq!(content, "最终答案");
        assert_eq!(latency.react_rounds, 1);
        let observed = observed.lock().unwrap_or_else(|e| e.into_inner());
        assert_eq!(observed.len(), 1);
        assert_eq!(observed[0].tool_count, 0);
        assert!(observed[0].system.contains(FINAL_RECOVERY_SYSTEM_SUFFIX));
    }

    #[test]
    fn run_worker_path_suppresses_final_reply_after_message_tool_primary_delivery() {
        let llm = SequenceStubLlm {
            responses: Mutex::new(vec![
                LlmResponse {
                    content: "[tool_use]".to_string(),
                    stop_reason: StopReason::ToolUse,
                    tool_calls: Some(vec![crate::llm::ToolCall {
                        id: "call_1".to_string(),
                        name: "message".to_string(),
                        input: r#"{"content":"工具主答复","delivery_kind":"primary"}"#.to_string(),
                    }]),
                },
                LlmResponse {
                    content: String::new(),
                    stop_reason: StopReason::EndTurn,
                    tool_calls: None,
                },
            ]),
        };
        let mut http = DummyPlatformHttp;
        let (outbound_tx, outbound_rx, _) = crate::bus::new_inbound_channel(8);
        let mut registry = crate::tools::ToolRegistry::new();
        registry.register(Box::new(crate::tools::MessageTool));
        let config = test_agent_loop_config();
        let msg =
            PcMsg::new_inbound("qq_channel", "chat-1", "测试多轮发送", false).expect("message");
        let mut repeat = HashMap::new();

        let (outcome, telemetry) = run_worker_path(
            &mut http,
            &llm,
            &msg,
            &outbound_tx,
            "req-1",
            &registry,
            &config,
            &mut repeat,
            UiLocale::Zh,
        )
        .expect("worker path");

        assert!(matches!(outcome, WorkerOutcome::Delivered(ref text) if text == "工具主答复"));
        assert!(telemetry.streamed);
        assert!(telemetry.delivery.current_primary_delivered);
        let first = outbound_rx.try_recv().expect("visible update");
        let second = outbound_rx.try_recv().expect("primary reply");
        let contents = [first.content.as_str(), second.content.as_str()];
        assert!(contents.contains(&"正在执行 message…"));
        assert!(contents.contains(&"工具主答复"));
        assert!(outbound_rx.try_recv().is_err());
    }
}
