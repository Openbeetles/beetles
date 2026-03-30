//! 运行指标与错误画像：消息吞吐、队列深度、LLM/tool 耗时与错误按 stage 聚合，供基线对比与 health 暴露。
//! Metrics and error profile: throughput, queue depth, LLM/tool timing, errors by stage.

// 32 位目标（xtensa/riscv32）无 AtomicU64，统一用 AtomicU32；快照仍以 u64 暴露。
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, OnceLock};

/// 已知 stage 的错误计数（与 Error::stage() 对齐）；其他 stage 归入 other。
const STAGE_AGENT_CHAT: &str = "agent_chat";
const STAGE_AGENT_CONTEXT: &str = "agent_context";
const STAGE_TOOL_EXECUTE: &str = "tool_execute";
const STAGE_LLM_REQUEST: &str = "llm_request";
const STAGE_LLM_PARSE: &str = "llm_parse";
const STAGE_CHANNEL_DISPATCH: &str = "channel_dispatch";
const STAGE_SESSION_APPEND: &str = "session_append";
const STAGE_TLS_ADMISSION: &str = "tls_admission";

static MESSAGES_IN: AtomicU32 = AtomicU32::new(0);
static MESSAGES_OUT: AtomicU32 = AtomicU32::new(0);
static LLM_CALLS: AtomicU32 = AtomicU32::new(0);
static LLM_ERRORS: AtomicU32 = AtomicU32::new(0);
static LLM_LAST_MS: AtomicU32 = AtomicU32::new(0);
static TTFT_LAST_MS: AtomicU32 = AtomicU32::new(0);
static E2E_LAST_MS: AtomicU32 = AtomicU32::new(0);
static USER_QUEUE_WAIT_LAST_MS: AtomicU32 = AtomicU32::new(0);
static SYSTEM_QUEUE_WAIT_LAST_MS: AtomicU32 = AtomicU32::new(0);
static CRON_E2E_LAST_MS: AtomicU32 = AtomicU32::new(0);
static REACT_ROUNDS_LAST: AtomicU32 = AtomicU32::new(0);
static TOOL_CALLS_LAST: AtomicU32 = AtomicU32::new(0);
static USER_MESSAGES_DONE: AtomicU32 = AtomicU32::new(0);
static SYSTEM_MESSAGES_DONE: AtomicU32 = AtomicU32::new(0);
static CRON_MESSAGES_DONE: AtomicU32 = AtomicU32::new(0);
static TOOL_CALLS: AtomicU32 = AtomicU32::new(0);
static TOOL_ERRORS: AtomicU32 = AtomicU32::new(0);
static TOOL_PROTOCOL_FORCED_ROUNDS: AtomicU32 = AtomicU32::new(0);
static TOOL_PROTOCOL_VIOLATION: AtomicU32 = AtomicU32::new(0);
static FINAL_ANSWER_CALLS: AtomicU32 = AtomicU32::new(0);
static WDT_FEEDS: AtomicU32 = AtomicU32::new(0);
static DISPATCH_SEND_OK: AtomicU32 = AtomicU32::new(0);
static DISPATCH_SEND_FAIL: AtomicU32 = AtomicU32::new(0);
static OUTBOUND_ENQUEUE_FAIL: AtomicU32 = AtomicU32::new(0);
static CHANNEL_HTTP_OK: AtomicU32 = AtomicU32::new(0);
static CHANNEL_HTTP_FAIL: AtomicU32 = AtomicU32::new(0);
static LAST_ACTIVE_EPOCH_SECS: AtomicU32 = AtomicU32::new(0);
static HTTP_PERMIT_WAIT_LAST_MS: AtomicU32 = AtomicU32::new(0);
static VOICE_INPUT_CAPTURE_LAST_MS: AtomicU32 = AtomicU32::new(0);
static VOICE_INPUT_STT_HTTP_LAST_MS: AtomicU32 = AtomicU32::new(0);
static VOICE_OUTPUT_TTS_HTTP_LAST_MS: AtomicU32 = AtomicU32::new(0);
static VOICE_OUTPUT_PLAY_LAST_MS: AtomicU32 = AtomicU32::new(0);
static VOICE_INPUT_FAIL_TOTAL: AtomicU32 = AtomicU32::new(0);
static VOICE_OUTPUT_FAIL_TOTAL: AtomicU32 = AtomicU32::new(0);
static WAKE_WORD_TRIGGER_TOTAL: AtomicU32 = AtomicU32::new(0);

/// Stream HTTP 连接槽位统计：由 `runtime::stream_http` 写入，metrics 快照统一暴露。
/// stream_http connection slot stats, written by `runtime::stream_http`.
static STREAM_HTTP_REUSE_HITS: AtomicU32 = AtomicU32::new(0);
static STREAM_HTTP_CREATES: AtomicU32 = AtomicU32::new(0);
static STREAM_HTTP_RESETS: AtomicU32 = AtomicU32::new(0);
static STREAM_HTTP_INVALIDATES: AtomicU32 = AtomicU32::new(0);

/// Linux 嵌入式 WiFi：wpa 守护恢复、AP 栈重启计数；失败 stage 摘要（脱敏，固定长度）。
static WIFI_RECONNECT_TOTAL: AtomicU32 = AtomicU32::new(0);
static WIFI_AP_RESTART_TOTAL: AtomicU32 = AtomicU32::new(0);
static WIFI_LAST_FAILURE_STAGE: OnceLock<Mutex<String>> = OnceLock::new();

static ERRORS_AGENT_CHAT: AtomicU32 = AtomicU32::new(0);
static ERRORS_AGENT_CONTEXT: AtomicU32 = AtomicU32::new(0);
static ERRORS_TOOL_EXECUTE: AtomicU32 = AtomicU32::new(0);
static ERRORS_LLM_REQUEST: AtomicU32 = AtomicU32::new(0);
static ERRORS_LLM_PARSE: AtomicU32 = AtomicU32::new(0);
static ERRORS_CHANNEL_DISPATCH: AtomicU32 = AtomicU32::new(0);
static ERRORS_SESSION_APPEND: AtomicU32 = AtomicU32::new(0);
static ERRORS_TLS_ADMISSION: AtomicU32 = AtomicU32::new(0);
static ERRORS_OTHER: AtomicU32 = AtomicU32::new(0);

#[inline]
pub fn record_message_in() {
    MESSAGES_IN.fetch_add(1, Ordering::Relaxed);
}

#[inline]
pub fn record_message_out() {
    MESSAGES_OUT.fetch_add(1, Ordering::Relaxed);
    // Update last-active timestamp (epoch seconds).
    let epoch_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .min(u32::MAX as u64) as u32;
    LAST_ACTIVE_EPOCH_SECS.store(epoch_secs, Ordering::Relaxed);
}

#[inline]
pub fn record_llm_call_start() -> std::time::Instant {
    LLM_CALLS.fetch_add(1, Ordering::Relaxed);
    std::time::Instant::now()
}

#[inline]
pub fn record_llm_call_end(start: std::time::Instant) {
    let ms = start.elapsed().as_millis().min(u32::MAX as u128) as u32;
    LLM_LAST_MS.store(ms, Ordering::Relaxed);
}

#[inline]
pub fn record_ttft_ms(ms: u128) {
    TTFT_LAST_MS.store(ms.min(u32::MAX as u128) as u32, Ordering::Relaxed);
}

#[inline]
pub fn record_e2e_ms(ms: u128) {
    E2E_LAST_MS.store(ms.min(u32::MAX as u128) as u32, Ordering::Relaxed);
}

#[inline]
pub fn record_user_queue_wait_ms(ms: u128) {
    USER_QUEUE_WAIT_LAST_MS.store(ms.min(u32::MAX as u128) as u32, Ordering::Relaxed);
}

#[inline]
pub fn record_system_queue_wait_ms(ms: u128) {
    SYSTEM_QUEUE_WAIT_LAST_MS.store(ms.min(u32::MAX as u128) as u32, Ordering::Relaxed);
}

#[inline]
pub fn record_cron_e2e_ms(ms: u128) {
    CRON_E2E_LAST_MS.store(ms.min(u32::MAX as u128) as u32, Ordering::Relaxed);
}

#[inline]
pub fn record_user_message_done() {
    USER_MESSAGES_DONE.fetch_add(1, Ordering::Relaxed);
}

#[inline]
pub fn record_system_message_done(is_cron: bool) {
    SYSTEM_MESSAGES_DONE.fetch_add(1, Ordering::Relaxed);
    if is_cron {
        CRON_MESSAGES_DONE.fetch_add(1, Ordering::Relaxed);
    }
}

pub fn record_react_rounds(rounds: u32) {
    REACT_ROUNDS_LAST.store(rounds, Ordering::Relaxed);
}

#[inline]
pub fn record_tool_calls_last(calls: u32) {
    TOOL_CALLS_LAST.store(calls, Ordering::Relaxed);
}

#[inline]
pub fn record_llm_error() {
    LLM_ERRORS.fetch_add(1, Ordering::Relaxed);
}

#[inline]
pub fn record_tool_call(ok: bool) {
    TOOL_CALLS.fetch_add(1, Ordering::Relaxed);
    if !ok {
        TOOL_ERRORS.fetch_add(1, Ordering::Relaxed);
    }
}

#[inline]
pub fn record_tool_protocol_forced_round() {
    TOOL_PROTOCOL_FORCED_ROUNDS.fetch_add(1, Ordering::Relaxed);
}

#[inline]
pub fn record_tool_protocol_violation() {
    TOOL_PROTOCOL_VIOLATION.fetch_add(1, Ordering::Relaxed);
}

#[inline]
pub fn record_final_answer_call() {
    FINAL_ANSWER_CALLS.fetch_add(1, Ordering::Relaxed);
}

#[inline]
pub fn record_wdt_feed() {
    WDT_FEEDS.fetch_add(1, Ordering::Relaxed);
}

#[inline]
pub fn record_dispatch_send(ok: bool) {
    if ok {
        DISPATCH_SEND_OK.fetch_add(1, Ordering::Relaxed);
    } else {
        DISPATCH_SEND_FAIL.fetch_add(1, Ordering::Relaxed);
    }
}

/// agent 侧 outbound_tx.try_send 失败（队列满或断开）时调用。
#[inline]
pub fn record_outbound_enqueue_fail() {
    OUTBOUND_ENQUEUE_FAIL.fetch_add(1, Ordering::Relaxed);
}

/// sender 线程在真实通道 HTTP 请求后上报（区别于 dispatch 入队成功）。
#[inline]
pub fn record_channel_http_result(ok: bool) {
    if ok {
        CHANNEL_HTTP_OK.fetch_add(1, Ordering::Relaxed);
    } else {
        CHANNEL_HTTP_FAIL.fetch_add(1, Ordering::Relaxed);
    }
}

#[inline]
pub fn record_http_permit_wait_ms(ms: u128) {
    HTTP_PERMIT_WAIT_LAST_MS.store(ms.min(u32::MAX as u128) as u32, Ordering::Relaxed);
}

#[inline]
pub fn record_voice_input_capture_ms(ms: u128) {
    VOICE_INPUT_CAPTURE_LAST_MS.store(ms.min(u32::MAX as u128) as u32, Ordering::Relaxed);
}

#[inline]
pub fn record_voice_input_stt_http_ms(ms: u128) {
    VOICE_INPUT_STT_HTTP_LAST_MS.store(ms.min(u32::MAX as u128) as u32, Ordering::Relaxed);
}

#[inline]
pub fn record_voice_output_tts_http_ms(ms: u128) {
    VOICE_OUTPUT_TTS_HTTP_LAST_MS.store(ms.min(u32::MAX as u128) as u32, Ordering::Relaxed);
}

#[inline]
pub fn record_voice_output_play_ms(ms: u128) {
    VOICE_OUTPUT_PLAY_LAST_MS.store(ms.min(u32::MAX as u128) as u32, Ordering::Relaxed);
}

#[inline]
pub fn record_wake_word_trigger() {
    WAKE_WORD_TRIGGER_TOTAL.fetch_add(1, Ordering::Relaxed);
}

#[inline]
pub fn record_voice_tool_failure(tool_name: &str) {
    match tool_name {
        "voice_input" => {
            VOICE_INPUT_FAIL_TOTAL.fetch_add(1, Ordering::Relaxed);
        }
        "voice_output" => {
            VOICE_OUTPUT_FAIL_TOTAL.fetch_add(1, Ordering::Relaxed);
        }
        _ => {}
    }
}

/// 按 stage 记录错误，用于故障画像 TopN；已知 stage 用常量匹配，其余归入 other。
/// wpa_supplicant 由看门狗重新拉起（PID 丢失或进程死亡）。
/// wpa_supplicant re-ensured by watchdog (missing PID or dead process).
#[inline]
pub fn record_wifi_reconnect() {
    WIFI_RECONNECT_TOTAL.fetch_add(1, Ordering::Relaxed);
}

/// hostapd/dnsmasq 看门狗检测到 AP 栈失效并执行重启尝试（无论是否成功）。
/// Watchdog detected AP stack down and attempted restart (counted per attempt).
#[inline]
pub fn record_wifi_ap_restart() {
    WIFI_AP_RESTART_TOTAL.fetch_add(1, Ordering::Relaxed);
}

/// 记录最近一次 WiFi 路径失败 stage（仅 [a-zA-Z0-9_]，最长 64，无密钥/SSID）。
/// Records last WiFi-path failure stage (alphanumeric + `_`, max 64; no secrets/SSID).
pub fn record_wifi_failure_stage(stage: &str) {
    let sanitized: String = stage
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
        .take(64)
        .collect();
    let m = WIFI_LAST_FAILURE_STAGE.get_or_init(|| Mutex::new(String::new()));
    if let Ok(mut g) = m.lock() {
        *g = sanitized;
    }
}

pub fn record_error_by_stage(stage: &str) {
    let c = match stage {
        STAGE_AGENT_CHAT => &ERRORS_AGENT_CHAT,
        STAGE_AGENT_CONTEXT => &ERRORS_AGENT_CONTEXT,
        STAGE_TOOL_EXECUTE => &ERRORS_TOOL_EXECUTE,
        STAGE_LLM_REQUEST => &ERRORS_LLM_REQUEST,
        STAGE_LLM_PARSE => &ERRORS_LLM_PARSE,
        STAGE_CHANNEL_DISPATCH => &ERRORS_CHANNEL_DISPATCH,
        STAGE_SESSION_APPEND => &ERRORS_SESSION_APPEND,
        STAGE_TLS_ADMISSION => &ERRORS_TLS_ADMISSION,
        _ => &ERRORS_OTHER,
    };
    c.fetch_add(1, Ordering::Relaxed);
}

/// Stream HTTP 槽位复用命中（slot 已存在，直接使用）。
pub fn record_stream_http_reuse() {
    STREAM_HTTP_REUSE_HITS.fetch_add(1, Ordering::Relaxed);
}
/// Stream HTTP 槽位新建（首次使用或已无效）。
pub fn record_stream_http_create() {
    STREAM_HTTP_CREATES.fetch_add(1, Ordering::Relaxed);
}
/// Stream HTTP 连接重置重试（keep-alive 恢复）。
pub fn record_stream_http_reset() {
    STREAM_HTTP_RESETS.fetch_add(1, Ordering::Relaxed);
}
/// Stream HTTP 槽位失效（连续失败后清空）。
pub fn record_stream_http_invalidate() {
    STREAM_HTTP_INVALIDATES.fetch_add(1, Ordering::Relaxed);
}

/// 快照：用于 health API 与结构化基线日志（无敏感信息）。内部用 u32 存储，以 u64 暴露。
pub fn snapshot() -> MetricsSnapshot {
    MetricsSnapshot {
        messages_in: MESSAGES_IN.load(Ordering::Relaxed) as u64,
        messages_out: MESSAGES_OUT.load(Ordering::Relaxed) as u64,
        llm_calls: LLM_CALLS.load(Ordering::Relaxed) as u64,
        llm_errors: LLM_ERRORS.load(Ordering::Relaxed) as u64,
        llm_last_ms: LLM_LAST_MS.load(Ordering::Relaxed) as u64,
        ttft_last_ms: TTFT_LAST_MS.load(Ordering::Relaxed) as u64,
        e2e_last_ms: E2E_LAST_MS.load(Ordering::Relaxed) as u64,
        user_queue_wait_last_ms: USER_QUEUE_WAIT_LAST_MS.load(Ordering::Relaxed) as u64,
        system_queue_wait_last_ms: SYSTEM_QUEUE_WAIT_LAST_MS.load(Ordering::Relaxed) as u64,
        cron_e2e_last_ms: CRON_E2E_LAST_MS.load(Ordering::Relaxed) as u64,
        react_rounds_last: REACT_ROUNDS_LAST.load(Ordering::Relaxed) as u64,
        tool_calls_last: TOOL_CALLS_LAST.load(Ordering::Relaxed) as u64,
        user_messages_done: USER_MESSAGES_DONE.load(Ordering::Relaxed) as u64,
        system_messages_done: SYSTEM_MESSAGES_DONE.load(Ordering::Relaxed) as u64,
        cron_messages_done: CRON_MESSAGES_DONE.load(Ordering::Relaxed) as u64,
        tool_calls: TOOL_CALLS.load(Ordering::Relaxed) as u64,
        tool_errors: TOOL_ERRORS.load(Ordering::Relaxed) as u64,
        tool_protocol_forced_rounds: TOOL_PROTOCOL_FORCED_ROUNDS.load(Ordering::Relaxed) as u64,
        tool_protocol_violation: TOOL_PROTOCOL_VIOLATION.load(Ordering::Relaxed) as u64,
        final_answer_calls: FINAL_ANSWER_CALLS.load(Ordering::Relaxed) as u64,
        wdt_feeds: WDT_FEEDS.load(Ordering::Relaxed) as u64,
        dispatch_send_ok: DISPATCH_SEND_OK.load(Ordering::Relaxed) as u64,
        dispatch_send_fail: DISPATCH_SEND_FAIL.load(Ordering::Relaxed) as u64,
        outbound_enqueue_fail: OUTBOUND_ENQUEUE_FAIL.load(Ordering::Relaxed) as u64,
        channel_http_ok: CHANNEL_HTTP_OK.load(Ordering::Relaxed) as u64,
        channel_http_fail: CHANNEL_HTTP_FAIL.load(Ordering::Relaxed) as u64,
        http_permit_wait_last_ms: HTTP_PERMIT_WAIT_LAST_MS.load(Ordering::Relaxed) as u64,
        voice_input_capture_last_ms: VOICE_INPUT_CAPTURE_LAST_MS.load(Ordering::Relaxed) as u64,
        voice_input_stt_http_last_ms: VOICE_INPUT_STT_HTTP_LAST_MS.load(Ordering::Relaxed) as u64,
        voice_output_tts_http_last_ms: VOICE_OUTPUT_TTS_HTTP_LAST_MS.load(Ordering::Relaxed) as u64,
        voice_output_play_last_ms: VOICE_OUTPUT_PLAY_LAST_MS.load(Ordering::Relaxed) as u64,
        voice_input_fail_total: VOICE_INPUT_FAIL_TOTAL.load(Ordering::Relaxed) as u64,
        voice_output_fail_total: VOICE_OUTPUT_FAIL_TOTAL.load(Ordering::Relaxed) as u64,
        wake_word_trigger_total: WAKE_WORD_TRIGGER_TOTAL.load(Ordering::Relaxed) as u64,
        errors_agent_chat: ERRORS_AGENT_CHAT.load(Ordering::Relaxed) as u64,
        errors_agent_context: ERRORS_AGENT_CONTEXT.load(Ordering::Relaxed) as u64,
        errors_tool_execute: ERRORS_TOOL_EXECUTE.load(Ordering::Relaxed) as u64,
        errors_llm_request: ERRORS_LLM_REQUEST.load(Ordering::Relaxed) as u64,
        errors_llm_parse: ERRORS_LLM_PARSE.load(Ordering::Relaxed) as u64,
        errors_channel_dispatch: ERRORS_CHANNEL_DISPATCH.load(Ordering::Relaxed) as u64,
        errors_session_append: ERRORS_SESSION_APPEND.load(Ordering::Relaxed) as u64,
        errors_tls_admission: ERRORS_TLS_ADMISSION.load(Ordering::Relaxed) as u64,
        errors_other: ERRORS_OTHER.load(Ordering::Relaxed) as u64,
        last_active_epoch_secs: LAST_ACTIVE_EPOCH_SECS.load(Ordering::Relaxed) as u64,
        wifi_reconnect_total: WIFI_RECONNECT_TOTAL.load(Ordering::Relaxed) as u64,
        wifi_ap_restart_total: WIFI_AP_RESTART_TOTAL.load(Ordering::Relaxed) as u64,
        wifi_last_failure_stage: WIFI_LAST_FAILURE_STAGE
            .get()
            .and_then(|m| m.lock().ok())
            .map(|g| g.clone())
            .unwrap_or_default(),
        stream_http_reuse_hits: STREAM_HTTP_REUSE_HITS.load(Ordering::Relaxed) as u64,
        stream_http_creates: STREAM_HTTP_CREATES.load(Ordering::Relaxed) as u64,
        stream_http_resets: STREAM_HTTP_RESETS.load(Ordering::Relaxed) as u64,
        stream_http_invalidates: STREAM_HTTP_INVALIDATES.load(Ordering::Relaxed) as u64,
    }
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct MetricsSnapshot {
    pub messages_in: u64,
    pub messages_out: u64,
    pub llm_calls: u64,
    pub llm_errors: u64,
    pub llm_last_ms: u64,
    pub ttft_last_ms: u64,
    pub e2e_last_ms: u64,
    pub user_queue_wait_last_ms: u64,
    pub system_queue_wait_last_ms: u64,
    pub cron_e2e_last_ms: u64,
    pub react_rounds_last: u64,
    pub tool_calls_last: u64,
    pub user_messages_done: u64,
    pub system_messages_done: u64,
    pub cron_messages_done: u64,
    pub tool_calls: u64,
    pub tool_errors: u64,
    pub tool_protocol_forced_rounds: u64,
    pub tool_protocol_violation: u64,
    pub final_answer_calls: u64,
    pub wdt_feeds: u64,
    pub dispatch_send_ok: u64,
    pub dispatch_send_fail: u64,
    pub outbound_enqueue_fail: u64,
    pub channel_http_ok: u64,
    pub channel_http_fail: u64,
    pub http_permit_wait_last_ms: u64,
    pub voice_input_capture_last_ms: u64,
    pub voice_input_stt_http_last_ms: u64,
    pub voice_output_tts_http_last_ms: u64,
    pub voice_output_play_last_ms: u64,
    pub voice_input_fail_total: u64,
    pub voice_output_fail_total: u64,
    pub wake_word_trigger_total: u64,
    pub errors_agent_chat: u64,
    pub errors_agent_context: u64,
    pub errors_tool_execute: u64,
    pub errors_llm_request: u64,
    pub errors_llm_parse: u64,
    pub errors_channel_dispatch: u64,
    pub errors_session_append: u64,
    pub errors_tls_admission: u64,
    pub errors_other: u64,
    pub last_active_epoch_secs: u64,
    pub wifi_reconnect_total: u64,
    pub wifi_ap_restart_total: u64,
    pub wifi_last_failure_stage: String,
    /// Stream HTTP 连接槽位统计（复用命中 / 新建 / 重置 / 失效）。
    pub stream_http_reuse_hits: u64,
    pub stream_http_creates: u64,
    pub stream_http_resets: u64,
    pub stream_http_invalidates: u64,
}

impl MetricsSnapshot {
    /// 结构化单行日志，便于基线对比（key=value，无敏感信息）。
    pub fn to_baseline_log_line(&self) -> String {
        use std::fmt::Write;
        // Pre-allocate: typical line ~320 bytes (incl. WiFi counters).
        let mut buf = String::with_capacity(384);
        let _ = write!(
            buf,
            "metrics msg_in={} msg_out={} llm_calls={} llm_err={} llm_last_ms={} ttft_last_ms={} e2e_last_ms={} user_q_wait_ms={} sys_q_wait_ms={} cron_e2e_ms={} react_rounds_last={} tool_calls_last={} user_done={} sys_done={} cron_done={} tool_calls={} tool_err={} tool_protocol_forced={} tool_protocol_violation={} final_answer_calls={} wdt_feeds={} dispatch_ok={} dispatch_fail={} outbound_enq_fail={} channel_http_ok={} channel_http_fail={} http_permit_wait_ms={} voice_in_capture_ms={} voice_in_stt_http_ms={} voice_out_tts_http_ms={} voice_out_play_ms={} voice_in_fail={} voice_out_fail={} wake_word_trigger={} err_chat={} err_ctx={} err_tool={} err_llm_req={} err_llm_parse={} err_dispatch={} err_session={} err_tls_admission={} err_other={} last_active_epoch={} wifi_reconn={} wifi_ap_restart={} wifi_last_fail_stage={} shttp_reuse={} shttp_create={} shttp_reset={} shttp_invalidate={}",
            self.messages_in,
            self.messages_out,
            self.llm_calls,
            self.llm_errors,
            self.llm_last_ms,
            self.ttft_last_ms,
            self.e2e_last_ms,
            self.user_queue_wait_last_ms,
            self.system_queue_wait_last_ms,
            self.cron_e2e_last_ms,
            self.react_rounds_last,
            self.tool_calls_last,
            self.user_messages_done,
            self.system_messages_done,
            self.cron_messages_done,
            self.tool_calls,
            self.tool_errors,
            self.tool_protocol_forced_rounds,
            self.tool_protocol_violation,
            self.final_answer_calls,
            self.wdt_feeds,
            self.dispatch_send_ok,
            self.dispatch_send_fail,
            self.outbound_enqueue_fail,
            self.channel_http_ok,
            self.channel_http_fail,
            self.http_permit_wait_last_ms,
            self.voice_input_capture_last_ms,
            self.voice_input_stt_http_last_ms,
            self.voice_output_tts_http_last_ms,
            self.voice_output_play_last_ms,
            self.voice_input_fail_total,
            self.voice_output_fail_total,
            self.wake_word_trigger_total,
            self.errors_agent_chat,
            self.errors_agent_context,
            self.errors_tool_execute,
            self.errors_llm_request,
            self.errors_llm_parse,
            self.errors_channel_dispatch,
            self.errors_session_append,
            self.errors_tls_admission,
            self.errors_other,
            self.last_active_epoch_secs,
            self.wifi_reconnect_total,
            self.wifi_ap_restart_total,
            self.wifi_last_failure_stage,
            self.stream_http_reuse_hits,
            self.stream_http_creates,
            self.stream_http_resets,
            self.stream_http_invalidates,
        );
        buf
    }
}
