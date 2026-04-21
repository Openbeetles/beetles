//! QQ 频道出站与连通性检查。Sink 统一为 dispatch::QueuedSink。

use crate::channels::ChannelHttpClient;
use crate::config::AppConfig;
use crate::error::{Error as BeetleError, Result as BeetleResult};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::channels::send::{
    ensure_sender_http, feed_sender_loop_wdt, record_outbound_http_failure,
    record_outbound_http_success, run_buffered_sender_loop, QueuedOutboundMessage,
};

use super::msg_id::{pop_msg_id, QqMsgIdCache};
use super::token::{
    cached_qq_token_value, clear_shared_cached_qq_token, ensure_cached_qq_token,
    fetch_qq_access_token, invalidate_cached_qq_token, load_shared_cached_qq_token,
    sync_shared_cached_qq_token, CachedQqToken, SharedQqTokenCache,
};

/// 单条消息最大字符数，与现有通道对齐。
const QQ_MAX_MESSAGE_LEN: usize = 4096;
const QQ_TURN_RESERVATION_TTL_SECS: u64 = 300;
const QQ_TURN_RESERVATION_CACHE_MAX: usize = 64;

const QQ_MESSAGES_BASE: &str = "https://api.sgroup.qq.com/channels";
const QQ_V2_BASE: &str = "https://api.sgroup.qq.com/v2";

struct QqSendRuntime<'a, H, F> {
    app_id: &'a str,
    secret: &'a str,
    cache: &'a QqMsgIdCache,
    shared_token_cache: &'a SharedQqTokenCache,
    http: &'a mut Option<H>,
    token_cache: &'a mut Option<CachedQqToken>,
    turn_tracker: &'a mut QqTurnReservationTracker,
    active_reservation: &'a mut Option<QqRetryableSendReservation>,
    create_http: &'a mut F,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct QqMsgSeqReservation {
    start: u64,
    chunk_count: usize,
}

impl QqMsgSeqReservation {
    fn seq_for_chunk(self, chunk_index: usize) -> Option<u64> {
        if chunk_index >= self.chunk_count {
            return None;
        }
        Some(self.start.saturating_add(chunk_index as u64))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct QqTurnKey {
    chat_id: String,
    req_id: String,
}

#[derive(Clone, Debug)]
struct QqTurnReservationState {
    msg_id: Option<String>,
    next_seq: u64,
    last_used_at_secs: u64,
}

#[derive(Default)]
struct QqTurnReservationTracker {
    by_turn: HashMap<QqTurnKey, QqTurnReservationState>,
}

impl QqTurnReservationTracker {
    fn reserve(
        &mut self,
        cache: &QqMsgIdCache,
        message: &QueuedOutboundMessage,
        chunk_count: usize,
    ) -> QqRetryableSendReservation {
        let normalized_chunk_count = chunk_count.max(1);
        let now_secs = qq_now_unix_secs();
        self.by_turn.retain(|_, state| {
            now_secs.saturating_sub(state.last_used_at_secs) <= QQ_TURN_RESERVATION_TTL_SECS
        });
        while self.by_turn.len() > QQ_TURN_RESERVATION_CACHE_MAX {
            let Some(oldest_key) = self
                .by_turn
                .iter()
                .min_by_key(|(_, state)| state.last_used_at_secs)
                .map(|(key, _)| key.clone())
            else {
                break;
            };
            self.by_turn.remove(&oldest_key);
        }
        let state = self
            .by_turn
            .entry(turn_key_for_message(message))
            .or_insert_with(|| QqTurnReservationState {
                msg_id: pop_msg_id(cache, &message.chat_id),
                next_seq: 1,
                last_used_at_secs: now_secs,
            });
        let start = state.next_seq;
        state.next_seq = state.next_seq.saturating_add(normalized_chunk_count as u64);
        state.last_used_at_secs = now_secs;
        QqRetryableSendReservation {
            transport_send_id: message.transport_send_id,
            msg_id: state.msg_id.clone(),
            msg_seq: if is_v2_chat(&message.chat_id) {
                Some(QqMsgSeqReservation {
                    start,
                    chunk_count: normalized_chunk_count,
                })
            } else {
                None
            },
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct QqRetryableSendReservation {
    transport_send_id: u32,
    msg_id: Option<String>,
    msg_seq: Option<QqMsgSeqReservation>,
}

fn qq_now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn turn_key_for_message(message: &QueuedOutboundMessage) -> QqTurnKey {
    QqTurnKey {
        chat_id: message.chat_id.clone(),
        req_id: message
            .req_id
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("-")
            .to_string(),
    }
}

fn reserve_fresh_send_reservation(
    cache: &QqMsgIdCache,
    turn_tracker: &mut QqTurnReservationTracker,
    message: &QueuedOutboundMessage,
) -> QqRetryableSendReservation {
    let chunk_count =
        crate::channels::chunk::chunk_str_by_char_count_iter(&message.content, QQ_MAX_MESSAGE_LEN)
            .count()
            .max(1);
    turn_tracker.reserve(cache, message, chunk_count)
}

fn resolve_retryable_send_reservation(
    active: &mut Option<QqRetryableSendReservation>,
    turn_tracker: &mut QqTurnReservationTracker,
    cache: &QqMsgIdCache,
    message: &QueuedOutboundMessage,
) -> QqRetryableSendReservation {
    if let Some(existing) = active.as_ref() {
        if existing.transport_send_id == message.transport_send_id {
            return existing.clone();
        }
    }
    let reservation = reserve_fresh_send_reservation(cache, turn_tracker, message);
    *active = Some(reservation.clone());
    reservation
}

fn release_retryable_send_reservation(
    active: &mut Option<QqRetryableSendReservation>,
    transport_send_id: u32,
) {
    if active
        .as_ref()
        .map(|reservation| reservation.transport_send_id)
        == Some(transport_send_id)
    {
        *active = None;
    }
}

/// 连通性检查：供 GET /api/channel_connectivity 使用。
pub fn check_connectivity<H: ChannelHttpClient + ?Sized>(
    config: &AppConfig,
    http: &mut H,
    loc: crate::i18n::Locale,
) -> super::super::connectivity::ChannelConnectivityItem {
    use super::super::connectivity;
    let configured =
        !config.qq_channel_app_id.trim().is_empty() && !config.qq_channel_secret.trim().is_empty();
    connectivity::probe_item(
        "qq_channel",
        configured,
        loc,
        || match fetch_qq_access_token(
            http,
            config.qq_channel_app_id.trim(),
            config.qq_channel_secret.trim(),
            "qq_connectivity",
        ) {
            Ok(_) => connectivity::ProbeStatus::Ok,
            Err(e) => {
                log::warn!("[qq_connectivity] {}", e);
                match e {
                    BeetleError::Http { status_code, .. } if status_code >= 400 => {
                        connectivity::ProbeStatus::InvalidToken
                    }
                    BeetleError::Config { .. } => connectivity::ProbeStatus::InvalidToken,
                    _ => connectivity::ProbeStatus::CheckFailed,
                }
            }
        },
    )
}

fn acquire_qq_token<H: ChannelHttpClient>(
    http: &mut H,
    app_id: &str,
    secret: &str,
) -> BeetleResult<String> {
    fetch_qq_access_token(http, app_id, secret, "qq_send_token")
}

/// 根据 chat_id 前缀确定 API 端点：
/// - "group:{group_openid}" → /v2/groups/{group_openid}/messages（群聊）
/// - "c2c:{user_openid}"   → /v2/users/{user_openid}/messages（C2C 单聊）
/// - 其他                   → /channels/{channel_id}/messages（频道）
fn build_qq_message_url(chat_id: &str) -> String {
    if let Some(group_openid) = chat_id.strip_prefix("group:") {
        format!("{}/groups/{}/messages", QQ_V2_BASE, group_openid)
    } else if let Some(user_openid) = chat_id.strip_prefix("c2c:") {
        format!("{}/users/{}/messages", QQ_V2_BASE, user_openid)
    } else {
        format!("{}/{}/messages", QQ_MESSAGES_BASE, chat_id)
    }
}

/// 群聊和私聊（v2 API）需要 msg_type 字段；频道 API 不需要。
fn is_v2_chat(chat_id: &str) -> bool {
    chat_id.starts_with("group:") || chat_id.starts_with("c2c:")
}

fn build_qq_send_body(content: &str, msg_id: Option<&str>, msg_seq: Option<u64>) -> Vec<u8> {
    let mut body = String::with_capacity(content.len() + msg_id.map_or(32, |id| id.len() + 32));
    body.push('{');
    body.push_str("\"content\":");
    crate::util::push_json_string_escaped(&mut body, content);
    if let Some(seq) = msg_seq {
        body.push_str(",\"msg_type\":0,\"msg_seq\":");
        body.push_str(&seq.to_string());
    }
    if let Some(id) = msg_id {
        body.push_str(",\"msg_id\":");
        crate::util::push_json_string_escaped(&mut body, id);
    }
    body.push('}');
    body.into_bytes()
}

/// 发送单条 QQ 消息（含自动分片）。返回 `Ok(())` 表示所有分片都成功（HTTP 2xx）。
/// 任一分片 HTTP 失败或 4xx+ 即返回 `Err`，供 sender loop 决定重试/熔断。
fn send_one_qq<H: ChannelHttpClient>(
    http: &mut H,
    token: &str,
    chat_id: &str,
    content: &str,
    msg_id: Option<&str>,
    msg_seq: Option<QqMsgSeqReservation>,
) -> crate::error::Result<()> {
    const TAG: &str = "qq_send";
    if content.trim().is_empty() {
        return Err(crate::error::Error::config(
            "qq_send_empty",
            "refusing to send empty QQ message",
        ));
    }
    let send_start = std::time::Instant::now();
    let url = build_qq_message_url(chat_id);
    let v2 = is_v2_chat(chat_id);
    let chunks = crate::channels::chunk::chunk_text_by_char_count(content, QQ_MAX_MESSAGE_LEN);
    for (i, chunk) in chunks.iter().enumerate() {
        let body_bytes = build_qq_send_body(
            chunk,
            if i == 0 { msg_id } else { None },
            if v2 {
                msg_seq.and_then(|reservation| reservation.seq_for_chunk(i))
            } else {
                None
            },
        );
        let auth_header = format!("QQBot {}", token);
        let mut cl_buf = [0u8; 20];
        let content_length = crate::util::usize_to_decimal_buf(&mut cl_buf, body_bytes.len());
        let headers = [
            ("Authorization", auth_header.as_str()),
            ("content-type", "application/json"),
            ("content-length", content_length),
        ];
        let http_start = std::time::Instant::now();
        match crate::channels::send::send_post_with_headers(TAG, http, &url, &headers, &body_bytes)
        {
            Ok((status, ref body)) if status >= 400 => {
                let preview =
                    String::from_utf8_lossy(&body.as_ref()[..body.as_ref().len().min(256)]);
                log::warn!(
                    "[{}] send status={} body={} chat_id={} chunk={}/{} http_ms={} total_ms={}",
                    TAG,
                    status,
                    preview,
                    chat_id,
                    i + 1,
                    chunks.len(),
                    http_start.elapsed().as_millis(),
                    send_start.elapsed().as_millis()
                );
                return Err(crate::error::Error::http("qq_send_http", status));
            }
            Err(e) => {
                log::warn!(
                    "[{}] send error: {} chat_id={} chunk={}/{} http_ms={} total_ms={}",
                    TAG,
                    e,
                    chat_id,
                    i + 1,
                    chunks.len(),
                    http_start.elapsed().as_millis(),
                    send_start.elapsed().as_millis()
                );
                return Err(e);
            }
            _ => {}
        }
    }
    log::debug!(
        "[latency][qq_http] chat_id={} chunks={} total_ms={}",
        chat_id,
        chunks.len(),
        send_start.elapsed().as_millis()
    );
    Ok(())
}

/// 从 rx 取出待发送（一次性 drain）。
pub fn flush_qq_channel_sends<H: ChannelHttpClient>(
    rx: &std::sync::mpsc::Receiver<QueuedOutboundMessage>,
    app_id: &str,
    secret: &str,
    cache: QqMsgIdCache,
    http: &mut H,
) {
    if app_id.is_empty() || secret.is_empty() {
        return;
    }
    let mut token: Option<String> = None;
    let mut turn_tracker = QqTurnReservationTracker::default();
    while let Ok(message) = rx.try_recv() {
        if token.is_none() {
            if message.outbound_kind.is_supplemental() {
                log::warn!(
                    "[qq_flush] supplemental dropped without cached token req_id={} chat_id={}",
                    message.req_id.as_deref().unwrap_or("-"),
                    message.chat_id
                );
                continue;
            }
            token = match acquire_qq_token(http, app_id, secret) {
                Ok(token) => Some(token),
                Err(error) => {
                    log::warn!("[qq_flush] acquire token failed: {}", error);
                    break;
                }
            };
        }
        let reservation = reserve_fresh_send_reservation(&cache, &mut turn_tracker, &message);
        if let Err(e) = send_one_qq(
            http,
            token.as_deref().unwrap_or_default(),
            &message.chat_id,
            &message.content,
            reservation.msg_id.as_deref(),
            reservation.msg_seq,
        ) {
            if !message.outbound_kind.is_supplemental() {
                record_outbound_http_failure(&e);
            }
            log::warn!(
                "[qq_flush] req_id={} send failed for chat_id={}: {}",
                message.req_id.as_deref().unwrap_or("-"),
                message.chat_id,
                e
            );
        } else if !message.outbound_kind.is_supplemental() {
            record_outbound_http_success();
        }
    }
}

/// QQ access_token 缓存提前刷新余量（秒），避免用即将过期的 token。
const QQ_TOKEN_CACHE_MARGIN_SECS: u64 = 120;

fn send_queued_qq_message<H, F>(
    message: &QueuedOutboundMessage,
    attempt: u8,
    runtime: &mut QqSendRuntime<'_, H, F>,
) -> crate::error::Result<()>
where
    H: ChannelHttpClient,
    F: FnMut() -> crate::error::Result<H>,
{
    const TAG: &str = "qq_sender";
    let msg_start = std::time::Instant::now();
    let mut token_wait_ms: u128 = 0;
    let is_supplemental = message.outbound_kind.is_supplemental();

    if !ensure_sender_http(runtime.http, runtime.create_http, TAG, attempt) {
        return Err(crate::error::Error::config(TAG, "create http failed"));
    }
    let Some(h) = runtime.http.as_mut() else {
        return Err(crate::error::Error::config(
            TAG,
            "sender http missing after ensure",
        ));
    };
    if runtime.token_cache.is_none() {
        *runtime.token_cache = load_shared_cached_qq_token(runtime.shared_token_cache);
    }
    let token = if is_supplemental {
        match cached_qq_token_value(runtime.token_cache) {
            Some(token) => token.to_string(),
            None => {
                log::warn!(
                    "[{}] supplemental dropped without cached token req_id={} chat_id={}",
                    TAG,
                    message.req_id.as_deref().unwrap_or("-"),
                    message.chat_id
                );
                return Ok(());
            }
        }
    } else {
        let had_cached_token = cached_qq_token_value(runtime.token_cache).is_some();
        let token_start = std::time::Instant::now();
        match ensure_cached_qq_token(
            h,
            runtime.token_cache,
            runtime.app_id,
            runtime.secret,
            "qq_send_token",
            QQ_TOKEN_CACHE_MARGIN_SECS,
        ) {
            Ok(token) => {
                if !had_cached_token {
                    token_wait_ms = token_wait_ms.saturating_add(token_start.elapsed().as_millis());
                }
                sync_shared_cached_qq_token(runtime.shared_token_cache, runtime.token_cache);
                token
            }
            Err(error) => {
                if !had_cached_token {
                    token_wait_ms = token_wait_ms.saturating_add(token_start.elapsed().as_millis());
                }
                log::warn!(
                    "[{}] acquire token failed (attempt {}): {} token_wait_ms={}",
                    TAG,
                    attempt,
                    error,
                    token_wait_ms
                );
                *runtime.http = None;
                invalidate_cached_qq_token(runtime.token_cache);
                clear_shared_cached_qq_token(runtime.shared_token_cache);
                return Err(error);
            }
        }
    };

    let reservation = resolve_retryable_send_reservation(
        runtime.active_reservation,
        runtime.turn_tracker,
        runtime.cache,
        message,
    );
    let http_send_start = std::time::Instant::now();
    match send_one_qq(
        h,
        &token,
        &message.chat_id,
        &message.content,
        reservation.msg_id.as_deref(),
        reservation.msg_seq,
    ) {
        Ok(()) => {
            release_retryable_send_reservation(
                runtime.active_reservation,
                message.transport_send_id,
            );
            if !is_supplemental {
                crate::orchestrator::record_channel_result_pub("qq_channel", true);
                record_outbound_http_success();
            }
            log::debug!(
                "[latency][qq_sender] req_id={} chat_id={} outbound_kind={:?} attempt={} token_wait_ms={} http_send_ms={} total_ms={} status=ok",
                message.req_id.as_deref().unwrap_or("-"),
                message.chat_id,
                message.outbound_kind,
                attempt,
                token_wait_ms,
                http_send_start.elapsed().as_millis(),
                msg_start.elapsed().as_millis()
            );
            Ok(())
        }
        Err(error) => {
            if !is_supplemental {
                crate::orchestrator::record_channel_result_pub("qq_channel", false);
                record_outbound_http_failure(&error);
            }
            log::warn!(
                "[{}] req_id={} send failed (attempt {}): {} chat_id={} outbound_kind={:?} token_wait_ms={} http_send_ms={} total_ms={}",
                TAG,
                message.req_id.as_deref().unwrap_or("-"),
                attempt,
                error,
                message.chat_id,
                message.outbound_kind,
                token_wait_ms,
                http_send_start.elapsed().as_millis(),
                msg_start.elapsed().as_millis()
            );
            *runtime.http = None;
            invalidate_cached_qq_token(runtime.token_cache);
            clear_shared_cached_qq_token(runtime.shared_token_cache);
            Err(error)
        }
    }
}

/// 持续运行的 QQ 频道发送循环：本线程**复用**同一 HTTP 客户端（少占 lwIP socket，避免与 WSS 抢 fd），
/// 并按 `expires_in` **缓存** token，减少 `getAppAccessToken` 调用。
pub fn run_qq_sender_loop<H, F>(
    rx: std::sync::mpsc::Receiver<QueuedOutboundMessage>,
    app_id: &str,
    secret: &str,
    cache: QqMsgIdCache,
    shared_token_cache: SharedQqTokenCache,
    mut create_http: F,
) where
    H: ChannelHttpClient,
    F: FnMut() -> crate::error::Result<H>,
{
    const TAG: &str = "qq_sender";
    if app_id.is_empty() || secret.is_empty() {
        return;
    }
    let mut http: Option<H> = None;
    let mut token_cache: Option<CachedQqToken> = None;
    let mut turn_tracker = QqTurnReservationTracker::default();
    let mut active_reservation: Option<QqRetryableSendReservation> = None;
    let mut runtime = QqSendRuntime {
        app_id,
        secret,
        cache: &cache,
        shared_token_cache: &shared_token_cache,
        http: &mut http,
        token_cache: &mut token_cache,
        turn_tracker: &mut turn_tracker,
        active_reservation: &mut active_reservation,
        create_http: &mut create_http,
    };
    run_buffered_sender_loop(rx, TAG, |message, attempt| {
        feed_sender_loop_wdt();
        send_queued_qq_message(message, attempt, &mut runtime)
    });
}

#[cfg(test)]
mod tests {
    use crate::bus::OutboundKind;

    use super::super::msg_id::cache_msg_id;
    use super::*;
    use crate::platform::ResponseBody;
    use std::collections::HashMap;
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    fn queued_message(
        transport_send_id: u32,
        chat_id: &str,
        content: &str,
        req_id: Option<&str>,
        outbound_kind: OutboundKind,
    ) -> QueuedOutboundMessage {
        QueuedOutboundMessage {
            transport_send_id,
            chat_id: chat_id.to_string(),
            content: content.to_string(),
            req_id: req_id.map(str::to_string),
            outbound_kind,
        }
    }

    #[test]
    fn supplemental_and_primary_share_same_turn_msg_id() {
        let cache: QqMsgIdCache = Arc::new(Mutex::new(HashMap::new()));
        cache_msg_id(&cache, "c2c:chat-1", "msg-1").expect("cache msg_id");
        let supplemental = queued_message(
            7,
            "c2c:chat-1",
            "ack",
            Some("req-1"),
            OutboundKind::Supplemental,
        );
        let primary = queued_message(
            8,
            "c2c:chat-1",
            "final reply",
            Some("req-1"),
            OutboundKind::Primary,
        );
        let mut active = None;
        let mut turn_tracker = QqTurnReservationTracker::default();

        let first = resolve_retryable_send_reservation(
            &mut active,
            &mut turn_tracker,
            &cache,
            &supplemental,
        );
        release_retryable_send_reservation(&mut active, supplemental.transport_send_id);
        let second =
            resolve_retryable_send_reservation(&mut active, &mut turn_tracker, &cache, &primary);

        assert_eq!(first.msg_id.as_deref(), Some("msg-1"));
        assert_eq!(second.msg_id.as_deref(), Some("msg-1"));
        assert_eq!(pop_msg_id(&cache, "c2c:chat-1"), None);
    }

    #[test]
    fn retryable_reservation_reuses_primary_msg_id_and_msg_seq() {
        let cache: QqMsgIdCache = Arc::new(Mutex::new(HashMap::new()));
        cache_msg_id(&cache, "c2c:chat-1", "msg-1").expect("cache msg_id");
        let message = queued_message(
            41,
            "c2c:chat-1",
            "hello",
            Some("req-1"),
            OutboundKind::Primary,
        );
        let mut active = None;
        let mut turn_tracker = QqTurnReservationTracker::default();

        let first =
            resolve_retryable_send_reservation(&mut active, &mut turn_tracker, &cache, &message);
        let second =
            resolve_retryable_send_reservation(&mut active, &mut turn_tracker, &cache, &message);

        assert_eq!(first.msg_id.as_deref(), Some("msg-1"));
        assert_eq!(second.msg_id.as_deref(), Some("msg-1"));
        assert_eq!(first.msg_seq, second.msg_seq);
        assert_eq!(pop_msg_id(&cache, "c2c:chat-1"), None);
    }

    #[test]
    fn msg_seq_advances_within_same_turn_from_one() {
        let cache: QqMsgIdCache = Arc::new(Mutex::new(HashMap::new()));
        cache_msg_id(&cache, "c2c:chat-1", "msg-1").expect("cache msg_id");
        let supplemental = queued_message(
            7,
            "c2c:chat-1",
            "ack",
            Some("req-1"),
            OutboundKind::Supplemental,
        );
        let primary = queued_message(
            8,
            "c2c:chat-1",
            "final reply",
            Some("req-1"),
            OutboundKind::Primary,
        );
        let mut active = None;
        let mut turn_tracker = QqTurnReservationTracker::default();

        let first = resolve_retryable_send_reservation(
            &mut active,
            &mut turn_tracker,
            &cache,
            &supplemental,
        );
        release_retryable_send_reservation(&mut active, supplemental.transport_send_id);
        let second =
            resolve_retryable_send_reservation(&mut active, &mut turn_tracker, &cache, &primary);

        let first_seq = first.msg_seq.expect("supplemental seq");
        let second_seq = second.msg_seq.expect("primary seq");
        assert_eq!(first_seq.start, 1);
        assert_eq!(second_seq.start, 2);
        assert_eq!(second.msg_id.as_deref(), Some("msg-1"));
    }

    #[derive(Default)]
    struct StubHttpState {
        token_results: VecDeque<crate::error::Result<(u16, ResponseBody)>>,
        send_results: VecDeque<crate::error::Result<(u16, ResponseBody)>>,
        sent_bodies: Vec<Vec<u8>>,
    }

    #[derive(Clone, Default)]
    struct StubHttp {
        state: Arc<Mutex<StubHttpState>>,
    }

    impl ChannelHttpClient for StubHttp {
        fn http_get(&mut self, _url: &str) -> crate::error::Result<(u16, ResponseBody)> {
            Ok((200, ResponseBody::Heap(b"{}".to_vec())))
        }

        fn http_get_with_headers(
            &mut self,
            _url: &str,
            _headers: &[(&str, &str)],
        ) -> crate::error::Result<(u16, ResponseBody)> {
            Ok((200, ResponseBody::Heap(b"{}".to_vec())))
        }

        fn http_post(
            &mut self,
            _url: &str,
            _body: &[u8],
        ) -> crate::error::Result<(u16, ResponseBody)> {
            self.state
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .token_results
                .pop_front()
                .unwrap_or_else(|| {
                    Ok((
                        200,
                        ResponseBody::Heap(
                            br#"{"access_token":"qq-token","expires_in":7200}"#.to_vec(),
                        ),
                    ))
                })
        }

        fn http_post_with_headers(
            &mut self,
            _url: &str,
            _headers: &[(&str, &str)],
            body: &[u8],
        ) -> crate::error::Result<(u16, ResponseBody)> {
            let mut guard = self.state.lock().unwrap_or_else(|e| e.into_inner());
            guard.sent_bodies.push(body.to_vec());
            guard
                .send_results
                .pop_front()
                .unwrap_or_else(|| Ok((200, ResponseBody::Heap(b"{}".to_vec()))))
        }
    }

    #[test]
    fn sender_retry_reuses_same_http_msg_id_and_msg_seq_payload() {
        let cache: QqMsgIdCache = Arc::new(Mutex::new(HashMap::new()));
        cache_msg_id(&cache, "c2c:chat-1", "msg-1").expect("cache msg_id");
        let shared_token_cache = crate::channels::qq::new_shared_qq_token_cache();
        let shared_http_state = Arc::new(Mutex::new(StubHttpState {
            token_results: VecDeque::from([
                Ok((
                    200,
                    ResponseBody::Heap(
                        br#"{"access_token":"qq-token","expires_in":7200}"#.to_vec(),
                    ),
                )),
                Ok((
                    200,
                    ResponseBody::Heap(
                        br#"{"access_token":"qq-token","expires_in":7200}"#.to_vec(),
                    ),
                )),
            ]),
            send_results: VecDeque::from([
                Ok((
                    400,
                    ResponseBody::Heap(
                        r#"{"message":"消息被去重，请检查请求msgseq","code":40054005}"#
                            .as_bytes()
                            .to_vec(),
                    ),
                )),
                Ok((200, ResponseBody::Heap(b"{}".to_vec()))),
            ]),
            sent_bodies: Vec::new(),
        }));
        let mut http = Some(StubHttp {
            state: Arc::clone(&shared_http_state),
        });
        let mut token_cache = None;
        let mut turn_tracker = QqTurnReservationTracker::default();
        let mut active_reservation = None;
        let create_http_state = Arc::clone(&shared_http_state);
        let mut create_http = || -> crate::error::Result<StubHttp> {
            Ok(StubHttp {
                state: Arc::clone(&create_http_state),
            })
        };
        let mut runtime = QqSendRuntime {
            app_id: "app-id",
            secret: "secret",
            cache: &cache,
            shared_token_cache: &shared_token_cache,
            http: &mut http,
            token_cache: &mut token_cache,
            turn_tracker: &mut turn_tracker,
            active_reservation: &mut active_reservation,
            create_http: &mut create_http,
        };
        let message = queued_message(
            99,
            "c2c:chat-1",
            "final reply",
            Some("req-1"),
            OutboundKind::Primary,
        );

        assert!(send_queued_qq_message(&message, 1, &mut runtime).is_err());
        assert!(send_queued_qq_message(&message, 2, &mut runtime).is_ok());

        let guard = shared_http_state.lock().unwrap_or_else(|e| e.into_inner());
        let sent_bodies = &guard.sent_bodies;
        assert_eq!(sent_bodies.len(), 2);
        let first: serde_json::Value =
            serde_json::from_slice(&sent_bodies[0]).expect("first payload json");
        let second: serde_json::Value =
            serde_json::from_slice(&sent_bodies[1]).expect("second payload json");
        assert_eq!(first.get("msg_id"), second.get("msg_id"));
        assert_eq!(first.get("msg_seq"), second.get("msg_seq"));
    }
}
