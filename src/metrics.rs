//! 运行指标与错误画像：消息吞吐、队列深度、LLM/tool 耗时与错误按 stage 聚合，供基线对比与 health 暴露。
//! Metrics and error profile: throughput, queue depth, LLM/tool timing, errors by stage.

// 32 位目标（xtensa/riscv32）无 AtomicU64，统一用 AtomicU32；快照仍以 u64 暴露。
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

/// 已知 stage 的错误计数（与 Error::stage() 对齐）；其他 stage 归入 other。
const STAGE_AGENT_CHAT: &str = "agent_chat";
const STAGE_AGENT_CONTEXT: &str = "agent_context";
const STAGE_TOOL_EXECUTE: &str = "tool_execute";
const STAGE_LLM_REQUEST: &str = "llm_request";
const STAGE_LLM_PARSE: &str = "llm_parse";
const STAGE_CHANNEL_DISPATCH: &str = "channel_dispatch";
const STAGE_SESSION_APPEND: &str = "session_append";
const STAGE_TLS_ADMISSION: &str = "tls_admission";

static USER_MESSAGES_IN: AtomicU32 = AtomicU32::new(0);
static SYSTEM_MESSAGES_IN: AtomicU32 = AtomicU32::new(0);
static MESSAGES_OUT: AtomicU32 = AtomicU32::new(0);
static LLM_CALLS: AtomicU32 = AtomicU32::new(0);
static LLM_ERRORS: AtomicU32 = AtomicU32::new(0);
static LLM_LAST_MS: AtomicU32 = AtomicU32::new(0);
static LLM_REQUEST_BODY_LAST_BYTES: AtomicU32 = AtomicU32::new(0);
static LLM_REQUEST_BODY_MAX_BYTES: AtomicU32 = AtomicU32::new(0);
static REQUEST_SEMANTICS_LAST_MS: AtomicU32 = AtomicU32::new(0);
static TOOL_EXEC_LAST_MS: AtomicU32 = AtomicU32::new(0);
static MENTAL_PRIVACY_REVIEW_LAST_MS: AtomicU32 = AtomicU32::new(0);
static TTFT_LAST_MS: AtomicU32 = AtomicU32::new(0);
static E2E_LAST_MS: AtomicU32 = AtomicU32::new(0);
static POST_REPLY_LAST_MS: AtomicU32 = AtomicU32::new(0);
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
static DISPATCH_SEND_OK: AtomicU32 = AtomicU32::new(0);
static DISPATCH_SEND_FAIL: AtomicU32 = AtomicU32::new(0);
static OUTBOUND_ENQUEUE_FAIL: AtomicU32 = AtomicU32::new(0);
static INBOUND_QUEUE_FULL_TOTAL: AtomicU32 = AtomicU32::new(0);
static INBOUND_DEFER_TOTAL: AtomicU32 = AtomicU32::new(0);
static INBOUND_DROP_TOTAL: AtomicU32 = AtomicU32::new(0);
static EVENT_INGRESS_ENQUEUED_TOTAL: AtomicU32 = AtomicU32::new(0);
static EVENT_INGRESS_REJECTED_TOTAL: AtomicU32 = AtomicU32::new(0);
static EVENT_INGRESS_PURGED_TOTAL: AtomicU32 = AtomicU32::new(0);
static EVENT_INGRESS_CANCELLED_TOTAL: AtomicU32 = AtomicU32::new(0);
static EVENT_INGRESS_STALE_DROP_TOTAL: AtomicU32 = AtomicU32::new(0);
static RUNTIME_SPAWN_FAILURE_TOTAL: AtomicU32 = AtomicU32::new(0);
static HTTP_ROUTE_REJECT_TOTAL: AtomicU32 = AtomicU32::new(0);
static LEASE_CONFLICT_TOTAL: AtomicU32 = AtomicU32::new(0);
static LEASE_EXPIRED_REPLACEMENT_TOTAL: AtomicU32 = AtomicU32::new(0);
static PLANE_DRAIN_TIMEOUT_TOTAL: AtomicU32 = AtomicU32::new(0);
static TOOL_SUCCEEDED_FINAL_DRIFT_TOTAL: AtomicU32 = AtomicU32::new(0);
static EMPTY_FINAL_BLOCKED_TOTAL: AtomicU32 = AtomicU32::new(0);
static INTERNAL_ERROR_COPY_SUPPRESSED_TOTAL: AtomicU32 = AtomicU32::new(0);
static CHANNEL_HTTP_OK: AtomicU32 = AtomicU32::new(0);
static CHANNEL_HTTP_FAIL: AtomicU32 = AtomicU32::new(0);
static LAST_ACTIVE_EPOCH_SECS: AtomicU32 = AtomicU32::new(0);
static HTTP_PERMIT_WAIT_LAST_MS: AtomicU32 = AtomicU32::new(0);
static HTTP_ROUTE_QUEUE_WAIT_LAST_MS: AtomicU32 = AtomicU32::new(0);
static HTTP_ROUTE_HANDLER_LAST_MS: AtomicU32 = AtomicU32::new(0);
static HTTP_ROUTE_TIMEOUT_TOTAL: AtomicU32 = AtomicU32::new(0);
static VOICE_INPUT_CAPTURE_LAST_MS: AtomicU32 = AtomicU32::new(0);
static VOICE_INPUT_STT_HTTP_LAST_MS: AtomicU32 = AtomicU32::new(0);
static VOICE_OUTPUT_TTS_HTTP_LAST_MS: AtomicU32 = AtomicU32::new(0);
static VOICE_OUTPUT_PLAY_LAST_MS: AtomicU32 = AtomicU32::new(0);
static VOICE_INPUT_FAIL_TOTAL: AtomicU32 = AtomicU32::new(0);
static VOICE_OUTPUT_FAIL_TOTAL: AtomicU32 = AtomicU32::new(0);
static VOICE_INTERRUPT_REQUEST_TOTAL: AtomicU32 = AtomicU32::new(0);
static VOICE_INTERRUPT_ACCEPT_TOTAL: AtomicU32 = AtomicU32::new(0);
static VOICE_CANCEL_SENT_TOTAL: AtomicU32 = AtomicU32::new(0);
static VOICE_STALE_AUDIO_DROP_TOTAL: AtomicU32 = AtomicU32::new(0);
static VOICE_INTERRUPT_REFERENCE_SUPPRESS_TOTAL: AtomicU32 = AtomicU32::new(0);
static VOICE_NO_SPEECH_TIMEOUT_TOTAL: AtomicU32 = AtomicU32::new(0);
static VOICE_RESPONSE_WAIT_TIMEOUT_TOTAL: AtomicU32 = AtomicU32::new(0);
static VOICE_POST_PLAYBACK_TIMEOUT_TOTAL: AtomicU32 = AtomicU32::new(0);
static WAKE_WORD_TRIGGER_TOTAL: AtomicU32 = AtomicU32::new(0);
static AUDIO_WORKER_TURNS_TOTAL: AtomicU32 = AtomicU32::new(0);
static AUDIO_WORKER_IDLE_TURNS_TOTAL: AtomicU32 = AtomicU32::new(0);
static AUDIO_MIC_POLL_TURNS_TOTAL: AtomicU32 = AtomicU32::new(0);
static AUDIO_MIC_FRAMES_TOTAL: AtomicU32 = AtomicU32::new(0);
static AUDIO_MIC_ZERO_READ_TOTAL: AtomicU32 = AtomicU32::new(0);
static AUDIO_LOOP_LAST_US: AtomicU32 = AtomicU32::new(0);
static AUDIO_MIC_READ_LAST_US: AtomicU32 = AtomicU32::new(0);
static AUDIO_SPEAKER_WRITE_LAST_US: AtomicU32 = AtomicU32::new(0);
static AUDIO_REFERENCE_FRAMES_TOTAL: AtomicU32 = AtomicU32::new(0);
static AUDIO_REFERENCE_ZERO_READ_TOTAL: AtomicU32 = AtomicU32::new(0);
static AUDIO_REFERENCE_QUEUE_DEPTH_LAST_SAMPLES: AtomicU32 = AtomicU32::new(0);
static AUDIO_SPEAKER_QUEUE_DEPTH_LAST_SAMPLES: AtomicU32 = AtomicU32::new(0);
static AUDIO_SPEAKER_QUEUE_DEPTH_MIN_SAMPLES: AtomicU32 = AtomicU32::new(u32::MAX);
static AUDIO_SPEAKER_UNDERRUN_TOTAL: AtomicU32 = AtomicU32::new(0);
static WAKE_WORD_FEED_CALLS_TOTAL: AtomicU32 = AtomicU32::new(0);
static WAKE_WORD_FEED_SKIP_BUSY_TOTAL: AtomicU32 = AtomicU32::new(0);
static WAKE_WORD_FEED_SKIP_COOLDOWN_TOTAL: AtomicU32 = AtomicU32::new(0);
static WAKE_WORD_FEED_DETECT_TOTAL: AtomicU32 = AtomicU32::new(0);
static WAKE_WORD_FEED_LAST_US: AtomicU32 = AtomicU32::new(0);
static STORAGE_LOCK_OPS_TOTAL: AtomicU32 = AtomicU32::new(0);
static STORAGE_LOCK_CONTENTION_TOTAL: AtomicU32 = AtomicU32::new(0);
static STORAGE_LOCK_WAIT_LAST_US: AtomicU32 = AtomicU32::new(0);
static STORAGE_LOCK_WAIT_TOTAL_US: AtomicU32 = AtomicU32::new(0);
static STORAGE_LOCK_HOLD_LAST_US: AtomicU32 = AtomicU32::new(0);
static STORAGE_LOCK_HOLD_TOTAL_US: AtomicU32 = AtomicU32::new(0);
static STORAGE_LOCK_HOLD_LAST_STAGE: OnceLock<Mutex<String>> = OnceLock::new();
static STORAGE_LOCK_LAST_OBSERVED_AT: OnceLock<Mutex<Option<Instant>>> = OnceLock::new();

/// Stream HTTP 连接槽位统计：由 `network` 治理面写入，metrics 快照统一暴露。
/// stream_http connection slot stats, written by the unified `network` governor.
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
fn record_activity_now() {
    let epoch_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .min(u32::MAX as u64) as u32;
    LAST_ACTIVE_EPOCH_SECS.store(epoch_secs, Ordering::Relaxed);
}

#[inline]
pub fn record_message_in() {
    record_user_message_in();
}

#[inline]
pub fn record_user_message_in() {
    USER_MESSAGES_IN.fetch_add(1, Ordering::Relaxed);
}

#[inline]
pub fn record_system_message_in() {
    SYSTEM_MESSAGES_IN.fetch_add(1, Ordering::Relaxed);
}

#[inline]
pub fn record_user_activity() {
    record_activity_now();
}

#[inline]
pub fn record_message_out() {
    MESSAGES_OUT.fetch_add(1, Ordering::Relaxed);
    record_activity_now();
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
pub fn record_llm_request_body_bytes(len: usize) {
    let len = len.min(u32::MAX as usize) as u32;
    LLM_REQUEST_BODY_LAST_BYTES.store(len, Ordering::Relaxed);
    let mut observed = LLM_REQUEST_BODY_MAX_BYTES.load(Ordering::Relaxed);
    while len > observed {
        match LLM_REQUEST_BODY_MAX_BYTES.compare_exchange_weak(
            observed,
            len,
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => break,
            Err(next) => observed = next,
        }
    }
}

#[inline]
pub fn record_request_semantics_ms(ms: u128) {
    REQUEST_SEMANTICS_LAST_MS.store(ms.min(u32::MAX as u128) as u32, Ordering::Relaxed);
}

#[inline]
pub fn record_tool_exec_ms(ms: u128) {
    TOOL_EXEC_LAST_MS.store(ms.min(u32::MAX as u128) as u32, Ordering::Relaxed);
}

#[inline]
pub fn record_mental_privacy_review_ms(ms: u128) {
    MENTAL_PRIVACY_REVIEW_LAST_MS.store(ms.min(u32::MAX as u128) as u32, Ordering::Relaxed);
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
pub fn record_post_reply_ms(ms: u128) {
    POST_REPLY_LAST_MS.store(ms.min(u32::MAX as u128) as u32, Ordering::Relaxed);
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

/// Channel/event-source inbound queue full. Outcome is recorded separately as
/// defer or drop because several platforms deliberately avoid acking.
#[inline]
pub fn record_inbound_queue_full() {
    INBOUND_QUEUE_FULL_TOTAL.fetch_add(1, Ordering::Relaxed);
}

/// Inbound event intentionally deferred via pending retry or upstream redelivery.
#[inline]
pub fn record_inbound_defer() {
    INBOUND_DEFER_TOTAL.fetch_add(1, Ordering::Relaxed);
}

/// Inbound event was dropped and is not expected to be replayed.
#[inline]
pub fn record_inbound_drop() {
    INBOUND_DROP_TOTAL.fetch_add(1, Ordering::Relaxed);
}

/// Event ingress accepted into its bounded execution queue.
#[inline]
pub fn record_event_ingress_enqueued() {
    EVENT_INGRESS_ENQUEUED_TOTAL.fetch_add(1, Ordering::Relaxed);
}

/// Event ingress rejected by bounded admission before execution.
#[inline]
pub fn record_event_ingress_rejected() {
    EVENT_INGRESS_REJECTED_TOTAL.fetch_add(1, Ordering::Relaxed);
}

/// Best-effort ingress work purged because a runtime mode transition made it stale.
#[inline]
pub fn record_event_ingress_purged() {
    EVENT_INGRESS_PURGED_TOTAL.fetch_add(1, Ordering::Relaxed);
}

/// Ingress work cancelled or coalesced under the same owner key.
#[inline]
pub fn record_event_ingress_cancelled() {
    EVENT_INGRESS_CANCELLED_TOTAL.fetch_add(1, Ordering::Relaxed);
}

/// Ingress work dropped after it became stale before execution.
#[inline]
pub fn record_event_ingress_stale_drop() {
    EVENT_INGRESS_STALE_DROP_TOTAL.fetch_add(1, Ordering::Relaxed);
}

#[inline]
pub fn record_runtime_spawn_failure() {
    RUNTIME_SPAWN_FAILURE_TOTAL.fetch_add(1, Ordering::Relaxed);
}

#[inline]
pub fn record_http_route_reject() {
    HTTP_ROUTE_REJECT_TOTAL.fetch_add(1, Ordering::Relaxed);
}

#[inline]
pub fn record_lease_conflict() {
    LEASE_CONFLICT_TOTAL.fetch_add(1, Ordering::Relaxed);
}

#[inline]
pub fn record_lease_expired_replacement() {
    LEASE_EXPIRED_REPLACEMENT_TOTAL.fetch_add(1, Ordering::Relaxed);
}

#[inline]
pub fn record_plane_drain_timeout() {
    PLANE_DRAIN_TIMEOUT_TOTAL.fetch_add(1, Ordering::Relaxed);
}

#[inline]
pub fn record_tool_succeeded_final_drift() {
    TOOL_SUCCEEDED_FINAL_DRIFT_TOTAL.fetch_add(1, Ordering::Relaxed);
}

#[inline]
pub fn record_empty_final_blocked() {
    EMPTY_FINAL_BLOCKED_TOTAL.fetch_add(1, Ordering::Relaxed);
}

#[inline]
pub fn record_internal_error_copy_suppressed() {
    INTERNAL_ERROR_COPY_SUPPRESSED_TOTAL.fetch_add(1, Ordering::Relaxed);
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
pub fn record_http_route_queue_wait_ms(ms: u128) {
    HTTP_ROUTE_QUEUE_WAIT_LAST_MS.store(ms.min(u32::MAX as u128) as u32, Ordering::Relaxed);
}

#[inline]
pub fn record_http_route_handler_ms(ms: u128) {
    HTTP_ROUTE_HANDLER_LAST_MS.store(ms.min(u32::MAX as u128) as u32, Ordering::Relaxed);
}

#[inline]
pub fn record_http_route_timeout() {
    HTTP_ROUTE_TIMEOUT_TOTAL.fetch_add(1, Ordering::Relaxed);
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
pub fn record_voice_interrupt_requested() {
    VOICE_INTERRUPT_REQUEST_TOTAL.fetch_add(1, Ordering::Relaxed);
}

#[inline]
pub fn record_voice_interrupt_accepted() {
    VOICE_INTERRUPT_ACCEPT_TOTAL.fetch_add(1, Ordering::Relaxed);
}

#[inline]
pub fn record_voice_cancel_sent() {
    VOICE_CANCEL_SENT_TOTAL.fetch_add(1, Ordering::Relaxed);
}

#[inline]
pub fn record_voice_stale_audio_drop() {
    VOICE_STALE_AUDIO_DROP_TOTAL.fetch_add(1, Ordering::Relaxed);
}

#[inline]
pub fn record_voice_interrupt_reference_suppressed() {
    VOICE_INTERRUPT_REFERENCE_SUPPRESS_TOTAL.fetch_add(1, Ordering::Relaxed);
}

#[inline]
pub fn record_voice_no_speech_timeout() {
    VOICE_NO_SPEECH_TIMEOUT_TOTAL.fetch_add(1, Ordering::Relaxed);
}

#[inline]
pub fn record_voice_response_wait_timeout() {
    VOICE_RESPONSE_WAIT_TIMEOUT_TOTAL.fetch_add(1, Ordering::Relaxed);
}

#[inline]
pub fn record_voice_post_playback_timeout() {
    VOICE_POST_PLAYBACK_TIMEOUT_TOTAL.fetch_add(1, Ordering::Relaxed);
}

#[inline]
pub fn record_wake_word_trigger() {
    WAKE_WORD_TRIGGER_TOTAL.fetch_add(1, Ordering::Relaxed);
}

#[inline]
pub fn record_audio_worker_turn() {
    AUDIO_WORKER_TURNS_TOTAL.fetch_add(1, Ordering::Relaxed);
}

#[inline]
pub fn record_audio_worker_idle_turn() {
    AUDIO_WORKER_IDLE_TURNS_TOTAL.fetch_add(1, Ordering::Relaxed);
}

#[inline]
pub fn record_audio_mic_poll_turn() {
    AUDIO_MIC_POLL_TURNS_TOTAL.fetch_add(1, Ordering::Relaxed);
}

#[inline]
pub fn record_audio_mic_frame_read() {
    AUDIO_MIC_FRAMES_TOTAL.fetch_add(1, Ordering::Relaxed);
}

#[inline]
pub fn record_audio_mic_zero_read() {
    AUDIO_MIC_ZERO_READ_TOTAL.fetch_add(1, Ordering::Relaxed);
}

#[inline]
pub fn record_audio_loop_us(us: u128) {
    AUDIO_LOOP_LAST_US.store(us.min(u32::MAX as u128) as u32, Ordering::Relaxed);
}

#[inline]
pub fn record_audio_mic_read_us(us: u128) {
    AUDIO_MIC_READ_LAST_US.store(us.min(u32::MAX as u128) as u32, Ordering::Relaxed);
}

#[inline]
pub fn record_audio_speaker_write_us(us: u128) {
    AUDIO_SPEAKER_WRITE_LAST_US.store(us.min(u32::MAX as u128) as u32, Ordering::Relaxed);
}

#[inline]
pub fn record_audio_reference_frame_read() {
    AUDIO_REFERENCE_FRAMES_TOTAL.fetch_add(1, Ordering::Relaxed);
}

#[inline]
pub fn record_audio_reference_zero_read() {
    AUDIO_REFERENCE_ZERO_READ_TOTAL.fetch_add(1, Ordering::Relaxed);
}

#[inline]
pub fn record_audio_reference_queue_depth_last_samples(samples: usize) {
    AUDIO_REFERENCE_QUEUE_DEPTH_LAST_SAMPLES
        .store(samples.min(u32::MAX as usize) as u32, Ordering::Relaxed);
}

#[inline]
pub fn record_audio_speaker_queue_depth_last_samples(samples: usize) {
    AUDIO_SPEAKER_QUEUE_DEPTH_LAST_SAMPLES
        .store(samples.min(u32::MAX as usize) as u32, Ordering::Relaxed);
}

#[inline]
pub fn reset_audio_speaker_queue_depth_min_samples() {
    AUDIO_SPEAKER_QUEUE_DEPTH_MIN_SAMPLES.store(u32::MAX, Ordering::Relaxed);
}

#[inline]
pub fn record_audio_speaker_queue_depth_min_candidate(samples: usize) {
    let candidate = samples.min(u32::MAX as usize) as u32;
    let mut current = AUDIO_SPEAKER_QUEUE_DEPTH_MIN_SAMPLES.load(Ordering::Relaxed);
    loop {
        if candidate >= current {
            return;
        }
        match AUDIO_SPEAKER_QUEUE_DEPTH_MIN_SAMPLES.compare_exchange_weak(
            current,
            candidate,
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => return,
            Err(next) => current = next,
        }
    }
}

#[inline]
pub fn record_audio_speaker_underrun() {
    AUDIO_SPEAKER_UNDERRUN_TOTAL.fetch_add(1, Ordering::Relaxed);
}

#[inline]
pub fn record_wake_word_feed_call() {
    WAKE_WORD_FEED_CALLS_TOTAL.fetch_add(1, Ordering::Relaxed);
}

#[inline]
pub fn record_wake_word_feed_skip_busy() {
    WAKE_WORD_FEED_SKIP_BUSY_TOTAL.fetch_add(1, Ordering::Relaxed);
}

#[inline]
pub fn record_wake_word_feed_skip_cooldown() {
    WAKE_WORD_FEED_SKIP_COOLDOWN_TOTAL.fetch_add(1, Ordering::Relaxed);
}

#[inline]
pub fn record_wake_word_feed_detect() {
    WAKE_WORD_FEED_DETECT_TOTAL.fetch_add(1, Ordering::Relaxed);
}

#[inline]
pub fn record_wake_word_feed_us(us: u128) {
    WAKE_WORD_FEED_LAST_US.store(us.min(u32::MAX as u128) as u32, Ordering::Relaxed);
}

#[inline]
pub fn record_storage_lock_wait_us(us: u128) {
    let clamped = us.min(u32::MAX as u128) as u32;
    STORAGE_LOCK_OPS_TOTAL.fetch_add(1, Ordering::Relaxed);
    STORAGE_LOCK_WAIT_LAST_US.store(clamped, Ordering::Relaxed);
    STORAGE_LOCK_WAIT_TOTAL_US.fetch_add(clamped, Ordering::Relaxed);
    if clamped >= 1_000 {
        STORAGE_LOCK_CONTENTION_TOTAL.fetch_add(1, Ordering::Relaxed);
    }
}

#[inline]
pub fn record_storage_lock_hold_us(us: u128) {
    record_storage_lock_hold_us_for_stage("storage", us);
}

pub fn record_storage_lock_hold_us_for_stage(stage: &'static str, us: u128) {
    let clamped = us.min(u32::MAX as u128) as u32;
    STORAGE_LOCK_HOLD_LAST_US.store(clamped, Ordering::Relaxed);
    STORAGE_LOCK_HOLD_TOTAL_US.fetch_add(clamped, Ordering::Relaxed);
    let stage_store = STORAGE_LOCK_HOLD_LAST_STAGE.get_or_init(|| Mutex::new(String::new()));
    if let Ok(mut guard) = stage_store.lock() {
        guard.clear();
        guard.push_str(stage);
    }
    let observed_at = STORAGE_LOCK_LAST_OBSERVED_AT.get_or_init(|| Mutex::new(None));
    if let Ok(mut guard) = observed_at.lock() {
        *guard = Some(Instant::now());
    }
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
        _ if stage.starts_with("tool_") => &ERRORS_TOOL_EXECUTE,
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
    let user_messages_in = USER_MESSAGES_IN.load(Ordering::Relaxed) as u64;
    let system_messages_in = SYSTEM_MESSAGES_IN.load(Ordering::Relaxed) as u64;
    MetricsSnapshot {
        messages_in: user_messages_in,
        user_messages_in,
        agent_messages_in: user_messages_in.saturating_add(system_messages_in),
        system_messages_in,
        messages_out: MESSAGES_OUT.load(Ordering::Relaxed) as u64,
        llm_calls: LLM_CALLS.load(Ordering::Relaxed) as u64,
        llm_errors: LLM_ERRORS.load(Ordering::Relaxed) as u64,
        llm_last_ms: LLM_LAST_MS.load(Ordering::Relaxed) as u64,
        llm_request_body_last_bytes: LLM_REQUEST_BODY_LAST_BYTES.load(Ordering::Relaxed) as u64,
        llm_request_body_max_bytes: LLM_REQUEST_BODY_MAX_BYTES.load(Ordering::Relaxed) as u64,
        request_semantics_last_ms: REQUEST_SEMANTICS_LAST_MS.load(Ordering::Relaxed) as u64,
        tool_exec_last_ms: TOOL_EXEC_LAST_MS.load(Ordering::Relaxed) as u64,
        mental_privacy_review_last_ms: MENTAL_PRIVACY_REVIEW_LAST_MS.load(Ordering::Relaxed) as u64,
        ttft_last_ms: TTFT_LAST_MS.load(Ordering::Relaxed) as u64,
        e2e_last_ms: E2E_LAST_MS.load(Ordering::Relaxed) as u64,
        post_reply_last_ms: POST_REPLY_LAST_MS.load(Ordering::Relaxed) as u64,
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
        dispatch_send_ok: DISPATCH_SEND_OK.load(Ordering::Relaxed) as u64,
        dispatch_send_fail: DISPATCH_SEND_FAIL.load(Ordering::Relaxed) as u64,
        outbound_enqueue_fail: OUTBOUND_ENQUEUE_FAIL.load(Ordering::Relaxed) as u64,
        inbound_queue_full_total: INBOUND_QUEUE_FULL_TOTAL.load(Ordering::Relaxed) as u64,
        inbound_defer_total: INBOUND_DEFER_TOTAL.load(Ordering::Relaxed) as u64,
        inbound_drop_total: INBOUND_DROP_TOTAL.load(Ordering::Relaxed) as u64,
        event_ingress_enqueued_total: EVENT_INGRESS_ENQUEUED_TOTAL.load(Ordering::Relaxed) as u64,
        event_ingress_rejected_total: EVENT_INGRESS_REJECTED_TOTAL.load(Ordering::Relaxed) as u64,
        event_ingress_purged_total: EVENT_INGRESS_PURGED_TOTAL.load(Ordering::Relaxed) as u64,
        event_ingress_cancelled_total: EVENT_INGRESS_CANCELLED_TOTAL.load(Ordering::Relaxed) as u64,
        event_ingress_stale_drop_total: EVENT_INGRESS_STALE_DROP_TOTAL.load(Ordering::Relaxed)
            as u64,
        runtime_spawn_failure_total: RUNTIME_SPAWN_FAILURE_TOTAL.load(Ordering::Relaxed) as u64,
        http_route_reject_total: HTTP_ROUTE_REJECT_TOTAL.load(Ordering::Relaxed) as u64,
        lease_conflict_total: LEASE_CONFLICT_TOTAL.load(Ordering::Relaxed) as u64,
        lease_expired_replacement_total: LEASE_EXPIRED_REPLACEMENT_TOTAL.load(Ordering::Relaxed)
            as u64,
        plane_drain_timeout_total: PLANE_DRAIN_TIMEOUT_TOTAL.load(Ordering::Relaxed) as u64,
        tool_succeeded_final_drift_total: TOOL_SUCCEEDED_FINAL_DRIFT_TOTAL.load(Ordering::Relaxed)
            as u64,
        empty_final_blocked_total: EMPTY_FINAL_BLOCKED_TOTAL.load(Ordering::Relaxed) as u64,
        internal_error_copy_suppressed_total: INTERNAL_ERROR_COPY_SUPPRESSED_TOTAL
            .load(Ordering::Relaxed) as u64,
        channel_http_ok: CHANNEL_HTTP_OK.load(Ordering::Relaxed) as u64,
        channel_http_fail: CHANNEL_HTTP_FAIL.load(Ordering::Relaxed) as u64,
        http_permit_wait_last_ms: HTTP_PERMIT_WAIT_LAST_MS.load(Ordering::Relaxed) as u64,
        http_route_queue_wait_last_ms: HTTP_ROUTE_QUEUE_WAIT_LAST_MS.load(Ordering::Relaxed) as u64,
        http_route_handler_last_ms: HTTP_ROUTE_HANDLER_LAST_MS.load(Ordering::Relaxed) as u64,
        http_route_timeout_total: HTTP_ROUTE_TIMEOUT_TOTAL.load(Ordering::Relaxed) as u64,
        voice_input_capture_last_ms: VOICE_INPUT_CAPTURE_LAST_MS.load(Ordering::Relaxed) as u64,
        voice_input_stt_http_last_ms: VOICE_INPUT_STT_HTTP_LAST_MS.load(Ordering::Relaxed) as u64,
        voice_output_tts_http_last_ms: VOICE_OUTPUT_TTS_HTTP_LAST_MS.load(Ordering::Relaxed) as u64,
        voice_output_play_last_ms: VOICE_OUTPUT_PLAY_LAST_MS.load(Ordering::Relaxed) as u64,
        voice_input_fail_total: VOICE_INPUT_FAIL_TOTAL.load(Ordering::Relaxed) as u64,
        voice_output_fail_total: VOICE_OUTPUT_FAIL_TOTAL.load(Ordering::Relaxed) as u64,
        voice_interrupt_request_total: VOICE_INTERRUPT_REQUEST_TOTAL.load(Ordering::Relaxed) as u64,
        voice_interrupt_accept_total: VOICE_INTERRUPT_ACCEPT_TOTAL.load(Ordering::Relaxed) as u64,
        voice_cancel_sent_total: VOICE_CANCEL_SENT_TOTAL.load(Ordering::Relaxed) as u64,
        voice_stale_audio_drop_total: VOICE_STALE_AUDIO_DROP_TOTAL.load(Ordering::Relaxed) as u64,
        voice_interrupt_reference_suppress_total: VOICE_INTERRUPT_REFERENCE_SUPPRESS_TOTAL
            .load(Ordering::Relaxed) as u64,
        voice_no_speech_timeout_total: VOICE_NO_SPEECH_TIMEOUT_TOTAL.load(Ordering::Relaxed) as u64,
        voice_response_wait_timeout_total: VOICE_RESPONSE_WAIT_TIMEOUT_TOTAL.load(Ordering::Relaxed)
            as u64,
        voice_post_playback_timeout_total: VOICE_POST_PLAYBACK_TIMEOUT_TOTAL.load(Ordering::Relaxed)
            as u64,
        wake_trigger_total: WAKE_WORD_TRIGGER_TOTAL.load(Ordering::Relaxed) as u64,
        audio_worker_turns_total: AUDIO_WORKER_TURNS_TOTAL.load(Ordering::Relaxed) as u64,
        audio_worker_idle_turns_total: AUDIO_WORKER_IDLE_TURNS_TOTAL.load(Ordering::Relaxed) as u64,
        audio_mic_poll_turns_total: AUDIO_MIC_POLL_TURNS_TOTAL.load(Ordering::Relaxed) as u64,
        audio_mic_frames_total: AUDIO_MIC_FRAMES_TOTAL.load(Ordering::Relaxed) as u64,
        audio_mic_zero_read_total: AUDIO_MIC_ZERO_READ_TOTAL.load(Ordering::Relaxed) as u64,
        audio_loop_last_us: AUDIO_LOOP_LAST_US.load(Ordering::Relaxed) as u64,
        audio_mic_read_last_us: AUDIO_MIC_READ_LAST_US.load(Ordering::Relaxed) as u64,
        audio_speaker_write_last_us: AUDIO_SPEAKER_WRITE_LAST_US.load(Ordering::Relaxed) as u64,
        audio_reference_frames_total: AUDIO_REFERENCE_FRAMES_TOTAL.load(Ordering::Relaxed) as u64,
        audio_reference_zero_read_total: AUDIO_REFERENCE_ZERO_READ_TOTAL.load(Ordering::Relaxed)
            as u64,
        audio_reference_queue_depth_last_samples: AUDIO_REFERENCE_QUEUE_DEPTH_LAST_SAMPLES
            .load(Ordering::Relaxed) as u64,
        wake_feed_calls_total: WAKE_WORD_FEED_CALLS_TOTAL.load(Ordering::Relaxed) as u64,
        wake_feed_skip_busy_total: WAKE_WORD_FEED_SKIP_BUSY_TOTAL.load(Ordering::Relaxed) as u64,
        wake_feed_skip_cooldown_total: WAKE_WORD_FEED_SKIP_COOLDOWN_TOTAL.load(Ordering::Relaxed)
            as u64,
        wake_feed_detect_total: WAKE_WORD_FEED_DETECT_TOTAL.load(Ordering::Relaxed) as u64,
        wake_feed_last_us: WAKE_WORD_FEED_LAST_US.load(Ordering::Relaxed) as u64,
        storage_lock_ops_total: STORAGE_LOCK_OPS_TOTAL.load(Ordering::Relaxed) as u64,
        storage_lock_contention_total: STORAGE_LOCK_CONTENTION_TOTAL.load(Ordering::Relaxed) as u64,
        storage_lock_wait_last_us: STORAGE_LOCK_WAIT_LAST_US.load(Ordering::Relaxed) as u64,
        storage_lock_wait_total_us: STORAGE_LOCK_WAIT_TOTAL_US.load(Ordering::Relaxed) as u64,
        storage_lock_hold_last_us: STORAGE_LOCK_HOLD_LAST_US.load(Ordering::Relaxed) as u64,
        storage_lock_hold_total_us: STORAGE_LOCK_HOLD_TOTAL_US.load(Ordering::Relaxed) as u64,
        storage_lock_hold_last_stage: STORAGE_LOCK_HOLD_LAST_STAGE
            .get()
            .and_then(|m| m.lock().ok())
            .map(|g| g.clone())
            .unwrap_or_default(),
        storage_lock_last_age_ms: STORAGE_LOCK_LAST_OBSERVED_AT
            .get()
            .and_then(|m| m.lock().ok())
            .and_then(|g| g.as_ref().copied())
            .map(|instant| instant.elapsed().as_millis().min(u64::MAX as u128) as u64)
            .unwrap_or(u64::MAX),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_prefixed_stages_count_as_tool_errors() {
        let before = snapshot();
        record_error_by_stage("tool_files");
        let after = snapshot();

        assert_eq!(after.errors_tool_execute, before.errors_tool_execute + 1);
        assert_eq!(after.errors_other, before.errors_other);
    }

    #[test]
    fn http_route_worker_metrics_are_recorded_and_logged() {
        let before = snapshot();

        record_http_route_queue_wait_ms(123);
        record_http_route_handler_ms(456);
        record_http_route_timeout();

        let after = snapshot();
        assert_eq!(after.http_route_queue_wait_last_ms, 123);
        assert_eq!(after.http_route_handler_last_ms, 456);
        assert_eq!(
            after.http_route_timeout_total,
            before.http_route_timeout_total + 1
        );

        let line = after.to_baseline_log_line();
        assert!(line.contains("http_route_queue_wait_ms=123"));
        assert!(line.contains("http_route_handler_ms=456"));
        assert!(line.contains("http_route_timeout_total="));
    }

    #[test]
    fn inbound_backpressure_metrics_are_recorded_and_logged() {
        let before = snapshot();

        record_inbound_queue_full();
        record_inbound_defer();
        record_inbound_drop();

        let after = snapshot();
        assert!(after.inbound_queue_full_total > before.inbound_queue_full_total);
        assert!(after.inbound_defer_total > before.inbound_defer_total);
        assert!(after.inbound_drop_total > before.inbound_drop_total);

        let line = after.to_baseline_log_line();
        assert!(line.contains("inbound_q_full="));
        assert!(line.contains("inbound_defer="));
        assert!(line.contains("inbound_drop="));
    }

    #[test]
    fn event_ingress_metrics_are_recorded_and_logged() {
        let before = snapshot();

        record_event_ingress_enqueued();
        record_event_ingress_rejected();
        record_event_ingress_purged();
        record_event_ingress_cancelled();
        record_event_ingress_stale_drop();

        let after = snapshot();
        assert!(after.event_ingress_enqueued_total > before.event_ingress_enqueued_total);
        assert!(after.event_ingress_rejected_total > before.event_ingress_rejected_total);
        assert!(after.event_ingress_purged_total > before.event_ingress_purged_total);
        assert!(after.event_ingress_cancelled_total > before.event_ingress_cancelled_total);
        assert!(after.event_ingress_stale_drop_total > before.event_ingress_stale_drop_total);

        let line = after.to_baseline_log_line();
        assert!(line.contains("event_ingress_enqueued_total="));
        assert!(line.contains("event_ingress_rejected_total="));
        assert!(line.contains("event_ingress_purged_total="));
        assert!(line.contains("event_ingress_cancelled_total="));
        assert!(line.contains("event_ingress_stale_drop_total="));
    }

    #[test]
    fn runtime_governance_metrics_are_recorded_and_logged() {
        let before = snapshot();

        record_runtime_spawn_failure();
        record_http_route_reject();
        record_lease_conflict();
        record_lease_expired_replacement();
        record_plane_drain_timeout();

        let after = snapshot();
        assert!(after.runtime_spawn_failure_total > before.runtime_spawn_failure_total);
        assert!(after.http_route_reject_total > before.http_route_reject_total);
        assert!(after.lease_conflict_total > before.lease_conflict_total);
        assert!(after.lease_expired_replacement_total > before.lease_expired_replacement_total);
        assert!(after.plane_drain_timeout_total > before.plane_drain_timeout_total);

        let line = after.to_baseline_log_line();
        assert!(line.contains("spawn_fail="));
        assert!(line.contains("http_route_reject="));
        assert!(line.contains("lease_conflict="));
        assert!(line.contains("lease_expired_replace="));
        assert!(line.contains("plane_drain_timeout="));
    }

    #[test]
    fn llm_request_body_size_is_recorded_and_logged() {
        let before = snapshot();

        record_llm_request_body_bytes(1234);
        let after_first = snapshot();
        assert_eq!(after_first.llm_request_body_last_bytes, 1234);
        assert!(after_first.llm_request_body_max_bytes >= 1234);

        record_llm_request_body_bytes(777);
        let after_second = snapshot();
        assert_eq!(after_second.llm_request_body_last_bytes, 777);
        assert!(after_second.llm_request_body_max_bytes >= after_first.llm_request_body_max_bytes);
        assert!(after_second.llm_request_body_max_bytes >= before.llm_request_body_max_bytes);

        let line = after_second.to_baseline_log_line();
        assert!(line.contains("llm_req_body_last_b=777"));
        assert!(line.contains("llm_req_body_max_b="));
    }

    #[test]
    fn user_visible_message_in_excludes_system_agent_work() {
        let before = snapshot();

        record_system_message_in();
        let after_system = snapshot();
        assert_eq!(after_system.messages_in, before.messages_in);
        assert_eq!(after_system.user_messages_in, after_system.messages_in);
        assert_eq!(
            after_system.system_messages_in,
            before.system_messages_in + 1
        );
        assert_eq!(after_system.agent_messages_in, before.agent_messages_in + 1);

        record_user_message_in();
        let after_user = snapshot();
        assert_eq!(after_user.messages_in, before.messages_in + 1);
        assert_eq!(after_user.user_messages_in, after_user.messages_in);
        assert_eq!(after_user.system_messages_in, before.system_messages_in + 1);
        assert_eq!(after_user.agent_messages_in, before.agent_messages_in + 2);
        assert!(after_user.to_baseline_log_line().contains("user_msg_in="));
    }
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct MetricsSnapshot {
    /// User/external inbound messages. Internal agent-plane work is counted separately.
    pub messages_in: u64,
    /// Explicit user/external inbound alias for new dashboards and logs.
    pub user_messages_in: u64,
    /// All messages consumed by the agent plane, including system maintenance work.
    pub agent_messages_in: u64,
    /// Internal system messages consumed by the agent plane.
    pub system_messages_in: u64,
    pub messages_out: u64,
    pub llm_calls: u64,
    pub llm_errors: u64,
    pub llm_last_ms: u64,
    pub llm_request_body_last_bytes: u64,
    pub llm_request_body_max_bytes: u64,
    pub request_semantics_last_ms: u64,
    pub tool_exec_last_ms: u64,
    pub mental_privacy_review_last_ms: u64,
    pub ttft_last_ms: u64,
    pub e2e_last_ms: u64,
    pub post_reply_last_ms: u64,
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
    pub dispatch_send_ok: u64,
    pub dispatch_send_fail: u64,
    pub outbound_enqueue_fail: u64,
    pub inbound_queue_full_total: u64,
    pub inbound_defer_total: u64,
    pub inbound_drop_total: u64,
    pub event_ingress_enqueued_total: u64,
    pub event_ingress_rejected_total: u64,
    pub event_ingress_purged_total: u64,
    pub event_ingress_cancelled_total: u64,
    pub event_ingress_stale_drop_total: u64,
    pub runtime_spawn_failure_total: u64,
    pub http_route_reject_total: u64,
    pub lease_conflict_total: u64,
    pub lease_expired_replacement_total: u64,
    pub plane_drain_timeout_total: u64,
    pub tool_succeeded_final_drift_total: u64,
    pub empty_final_blocked_total: u64,
    pub internal_error_copy_suppressed_total: u64,
    pub channel_http_ok: u64,
    pub channel_http_fail: u64,
    pub http_permit_wait_last_ms: u64,
    pub http_route_queue_wait_last_ms: u64,
    pub http_route_handler_last_ms: u64,
    pub http_route_timeout_total: u64,
    pub voice_input_capture_last_ms: u64,
    pub voice_input_stt_http_last_ms: u64,
    pub voice_output_tts_http_last_ms: u64,
    pub voice_output_play_last_ms: u64,
    pub voice_input_fail_total: u64,
    pub voice_output_fail_total: u64,
    pub voice_interrupt_request_total: u64,
    pub voice_interrupt_accept_total: u64,
    pub voice_cancel_sent_total: u64,
    pub voice_stale_audio_drop_total: u64,
    pub voice_interrupt_reference_suppress_total: u64,
    pub voice_no_speech_timeout_total: u64,
    pub voice_response_wait_timeout_total: u64,
    pub voice_post_playback_timeout_total: u64,
    pub wake_trigger_total: u64,
    pub audio_worker_turns_total: u64,
    pub audio_worker_idle_turns_total: u64,
    pub audio_mic_poll_turns_total: u64,
    pub audio_mic_frames_total: u64,
    pub audio_mic_zero_read_total: u64,
    pub audio_loop_last_us: u64,
    pub audio_mic_read_last_us: u64,
    pub audio_speaker_write_last_us: u64,
    pub audio_reference_frames_total: u64,
    pub audio_reference_zero_read_total: u64,
    pub audio_reference_queue_depth_last_samples: u64,
    pub wake_feed_calls_total: u64,
    pub wake_feed_skip_busy_total: u64,
    pub wake_feed_skip_cooldown_total: u64,
    pub wake_feed_detect_total: u64,
    pub wake_feed_last_us: u64,
    pub storage_lock_ops_total: u64,
    pub storage_lock_contention_total: u64,
    pub storage_lock_wait_last_us: u64,
    pub storage_lock_wait_total_us: u64,
    pub storage_lock_hold_last_us: u64,
    pub storage_lock_hold_total_us: u64,
    pub storage_lock_hold_last_stage: String,
    pub storage_lock_last_age_ms: u64,
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
            "metrics msg_in={} user_msg_in={} msg_out={} agent_msg_in={} sys_msg_in={} llm_calls={} llm_err={} llm_last_ms={} llm_req_body_last_b={} llm_req_body_max_b={} request_semantics_ms={} tool_exec_ms={} mental_privacy_review_ms={} ttft_last_ms={} e2e_last_ms={} post_reply_last_ms={} user_q_wait_ms={} sys_q_wait_ms={} cron_e2e_ms={} react_rounds_last={} tool_calls_last={} user_done={} sys_done={} cron_done={} tool_calls={} tool_err={} tool_protocol_forced={} tool_protocol_violation={} final_answer_calls={} dispatch_ok={} dispatch_fail={} outbound_enq_fail={} inbound_q_full={} inbound_defer={} inbound_drop={} event_ingress_enqueued_total={} event_ingress_rejected_total={} event_ingress_purged_total={} event_ingress_cancelled_total={} event_ingress_stale_drop_total={} spawn_fail={} http_route_reject={} lease_conflict={} lease_expired_replace={} plane_drain_timeout={} final_drift_total={} empty_final_blocked_total={} internal_error_copy_suppressed_total={} channel_http_ok={} channel_http_fail={} http_permit_wait_ms={} http_route_queue_wait_ms={} http_route_handler_ms={} http_route_timeout_total={} voice_in_capture_ms={} voice_in_stt_http_ms={} voice_out_tts_http_ms={} voice_out_play_ms={} voice_in_fail={} voice_out_fail={} voice_interrupt_req={} voice_interrupt_accept={} voice_cancel_sent={} voice_stale_drop={} voice_interrupt_ref_suppress={} voice_no_speech_to={} voice_resp_wait_to={} voice_post_play_to={} wake_trigger={} audio_turns={} audio_idle={} audio_mic_poll={} audio_mic_frames={} audio_mic_zero={} audio_loop_last_us={} audio_mic_read_last_us={} audio_spk_write_last_us={} audio_ref_frames={} audio_ref_zero={} audio_ref_depth_last={} wake_feed_calls={} wake_feed_busy_skip={} wake_feed_cooldown_skip={} wake_feed_detect={} wake_feed_last_us={} storage_ops={} storage_contention={} storage_wait_last_us={} storage_wait_total_us={} storage_hold_last_us={} storage_hold_total_us={} storage_hold_last_stage={} storage_last_age_ms={} err_chat={} err_ctx={} err_tool={} err_llm_req={} err_llm_parse={} err_dispatch={} err_session={} err_tls_admission={} err_other={} last_active_epoch={} wifi_reconn={} wifi_ap_restart={} wifi_last_fail_stage={} shttp_reuse={} shttp_create={} shttp_reset={} shttp_invalidate={}",
            self.messages_in,
            self.user_messages_in,
            self.messages_out,
            self.agent_messages_in,
            self.system_messages_in,
            self.llm_calls,
            self.llm_errors,
            self.llm_last_ms,
            self.llm_request_body_last_bytes,
            self.llm_request_body_max_bytes,
            self.request_semantics_last_ms,
            self.tool_exec_last_ms,
            self.mental_privacy_review_last_ms,
            self.ttft_last_ms,
            self.e2e_last_ms,
            self.post_reply_last_ms,
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
            self.dispatch_send_ok,
            self.dispatch_send_fail,
            self.outbound_enqueue_fail,
            self.inbound_queue_full_total,
            self.inbound_defer_total,
            self.inbound_drop_total,
            self.event_ingress_enqueued_total,
            self.event_ingress_rejected_total,
            self.event_ingress_purged_total,
            self.event_ingress_cancelled_total,
            self.event_ingress_stale_drop_total,
            self.runtime_spawn_failure_total,
            self.http_route_reject_total,
            self.lease_conflict_total,
            self.lease_expired_replacement_total,
            self.plane_drain_timeout_total,
            self.tool_succeeded_final_drift_total,
            self.empty_final_blocked_total,
            self.internal_error_copy_suppressed_total,
            self.channel_http_ok,
            self.channel_http_fail,
            self.http_permit_wait_last_ms,
            self.http_route_queue_wait_last_ms,
            self.http_route_handler_last_ms,
            self.http_route_timeout_total,
            self.voice_input_capture_last_ms,
            self.voice_input_stt_http_last_ms,
            self.voice_output_tts_http_last_ms,
            self.voice_output_play_last_ms,
            self.voice_input_fail_total,
            self.voice_output_fail_total,
            self.voice_interrupt_request_total,
            self.voice_interrupt_accept_total,
            self.voice_cancel_sent_total,
            self.voice_stale_audio_drop_total,
            self.voice_interrupt_reference_suppress_total,
            self.voice_no_speech_timeout_total,
            self.voice_response_wait_timeout_total,
            self.voice_post_playback_timeout_total,
            self.wake_trigger_total,
            self.audio_worker_turns_total,
            self.audio_worker_idle_turns_total,
            self.audio_mic_poll_turns_total,
            self.audio_mic_frames_total,
            self.audio_mic_zero_read_total,
            self.audio_loop_last_us,
            self.audio_mic_read_last_us,
            self.audio_speaker_write_last_us,
            self.audio_reference_frames_total,
            self.audio_reference_zero_read_total,
            self.audio_reference_queue_depth_last_samples,
            self.wake_feed_calls_total,
            self.wake_feed_skip_busy_total,
            self.wake_feed_skip_cooldown_total,
            self.wake_feed_detect_total,
            self.wake_feed_last_us,
            self.storage_lock_ops_total,
            self.storage_lock_contention_total,
            self.storage_lock_wait_last_us,
            self.storage_lock_wait_total_us,
            self.storage_lock_hold_last_us,
            self.storage_lock_hold_total_us,
            self.storage_lock_hold_last_stage,
            self.storage_lock_last_age_ms,
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
