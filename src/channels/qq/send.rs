//! QQ 频道出站与连通性检查。Sink 统一为 dispatch::QueuedSink。

use crate::bus::{CanonicalMessageBody, CardFormat, MediaLocatorKind, MessageBodyKind, TextFormat};
use crate::channels::ChannelHttpClient;
use crate::config::AppConfig;
use crate::error::{Error as BeetleError, Result as BeetleResult};
use crate::platform::ByteBuffer;
use std::collections::HashMap;
use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
use crate::channels::send::ActiveChannelSender;
use crate::channels::send::{
    ensure_sender_http, feed_sender_loop_wdt, record_outbound_http_failure,
    record_outbound_http_success, run_buffered_sender_loop, QueuedOutboundMessage,
};
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
use crate::platform::PlatformHttpClient;
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
use std::sync::Arc;

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

#[cfg(test)]
fn reply_http_priority_for_message_kind(
    kind: crate::bus::OutboundKind,
) -> crate::orchestrator::Priority {
    crate::channels::send::reply_http_priority_for_message_kind(kind)
}

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
    record_channel_health: bool,
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
                msg_id: message_platform_message_id(message)
                    .or_else(|| pop_msg_id(cache, &message.chat_id)),
                next_seq: 1,
                last_used_at_secs: now_secs,
            });
        if state.msg_id.is_none() {
            state.msg_id = message_platform_message_id(message);
        }
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

fn message_platform_message_id(message: &QueuedOutboundMessage) -> Option<String> {
    let trimmed = message.platform_message_id.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn reserve_fresh_send_reservation(
    cache: &QqMsgIdCache,
    turn_tracker: &mut QqTurnReservationTracker,
    message: &QueuedOutboundMessage,
) -> QqRetryableSendReservation {
    let chunk_count = match &message.body {
        CanonicalMessageBody::Text(body) if body.format == TextFormat::Plain => {
            crate::channels::chunk::chunk_str_by_char_count_iter(
                &message.content,
                QQ_MAX_MESSAGE_LEN,
            )
            .count()
            .max(1)
        }
        _ => 1,
    };
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
) -> super::super::connectivity::ChannelConnectivityItem {
    use super::super::connectivity;
    let configured =
        !config.qq_channel_app_id.trim().is_empty() && !config.qq_channel_secret.trim().is_empty();
    connectivity::probe_item("qq_channel", configured, || {
        match fetch_qq_access_token(
            http,
            config.qq_channel_app_id.trim(),
            config.qq_channel_secret.trim(),
            "qq_connectivity",
        ) {
            Ok(_) => {
                if crate::channels::is_ws_online() {
                    connectivity::ProbeStatus::Ok
                } else {
                    log::warn!("[qq_connectivity] websocket offline");
                    connectivity::ProbeStatus::CheckFailed
                }
            }
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
        }
    })
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

fn push_json_string_escaped_bytes(body: &mut ByteBuffer, value: &str) -> crate::error::Result<()> {
    body.write_all(b"\"")
        .map_err(|e| crate::error::Error::io("qq_send", e))?;
    for ch in value.chars() {
        match ch {
            '"' => body.write_all(b"\\\""),
            '\\' => body.write_all(b"\\\\"),
            '\n' => body.write_all(b"\\n"),
            '\r' => body.write_all(b"\\r"),
            '\t' => body.write_all(b"\\t"),
            '\u{08}' => body.write_all(b"\\b"),
            '\u{0c}' => body.write_all(b"\\f"),
            c if c <= '\u{1f}' => {
                const HEX: &[u8; 16] = b"0123456789abcdef";
                let code = c as u32 as u8;
                body.write_all(b"\\u00").and_then(|_| {
                    body.write_all(&[HEX[(code >> 4) as usize], HEX[(code & 0x0f) as usize]])
                })
            }
            c => {
                let mut buf = [0u8; 4];
                body.write_all(c.encode_utf8(&mut buf).as_bytes())
            }
        }
        .map_err(|e| crate::error::Error::io("qq_send", e))?;
    }
    body.write_all(b"\"")
        .map_err(|e| crate::error::Error::io("qq_send", e))
}

fn build_qq_send_body(
    content: &str,
    msg_id: Option<&str>,
    msg_seq: Option<u64>,
) -> crate::error::Result<ByteBuffer> {
    let mut body = ByteBuffer::with_capacity(content.len() + msg_id.map_or(32, |id| id.len() + 32));
    body.write_all(b"{\"content\":")
        .map_err(|e| crate::error::Error::io("qq_send", e))?;
    push_json_string_escaped_bytes(&mut body, content)?;
    if let Some(seq) = msg_seq {
        write!(&mut body, ",\"msg_type\":0,\"msg_seq\":{seq}")
            .map_err(|e| crate::error::Error::io("qq_send", e))?;
    }
    if let Some(id) = msg_id {
        body.write_all(b",\"msg_id\":")
            .map_err(|e| crate::error::Error::io("qq_send", e))?;
        push_json_string_escaped_bytes(&mut body, id)?;
    }
    body.write_all(b"}")
        .map_err(|e| crate::error::Error::io("qq_send", e))?;
    Ok(body)
}

fn push_reply_metadata(
    map: &mut serde_json::Map<String, serde_json::Value>,
    msg_id: Option<&str>,
    msg_seq: Option<u64>,
) {
    if let Some(seq) = msg_seq {
        map.insert("msg_seq".to_string(), serde_json::json!(seq));
    }
    if let Some(msg_id) = msg_id {
        map.insert("msg_id".to_string(), serde_json::json!(msg_id));
    }
}

fn build_qq_markdown_body(
    content: &str,
    msg_id: Option<&str>,
    msg_seq: Option<u64>,
) -> crate::error::Result<Vec<u8>> {
    let content_projection =
        crate::channels::outbound_text::render_markdownish_to_plain_text(content);
    let mut map = serde_json::Map::new();
    map.insert("content".to_string(), serde_json::json!(content_projection));
    map.insert("msg_type".to_string(), serde_json::json!(2));
    map.insert(
        "markdown".to_string(),
        serde_json::json!({
            "content": content,
        }),
    );
    push_reply_metadata(&mut map, msg_id, msg_seq);
    serde_json::to_vec(&serde_json::Value::Object(map))
        .map_err(|e| crate::error::Error::config("qq_send", e.to_string()))
}

fn build_qq_card_body(
    msg_type: i32,
    field: &str,
    payload: &serde_json::Value,
    msg_id: Option<&str>,
    msg_seq: Option<u64>,
) -> crate::error::Result<Vec<u8>> {
    let mut map = serde_json::Map::new();
    map.insert("msg_type".to_string(), serde_json::json!(msg_type));
    map.insert(field.to_string(), payload.clone());
    push_reply_metadata(&mut map, msg_id, msg_seq);
    serde_json::to_vec(&serde_json::Value::Object(map))
        .map_err(|e| crate::error::Error::config("qq_send", e.to_string()))
}

fn build_qq_media_body(
    file_info: &str,
    msg_id: Option<&str>,
    msg_seq: Option<u64>,
) -> crate::error::Result<Vec<u8>> {
    let mut map = serde_json::Map::new();
    map.insert("msg_type".to_string(), serde_json::json!(7));
    map.insert(
        "media".to_string(),
        serde_json::json!({
            "file_info": file_info,
        }),
    );
    push_reply_metadata(&mut map, msg_id, msg_seq);
    serde_json::to_vec(&serde_json::Value::Object(map))
        .map_err(|e| crate::error::Error::config("qq_send", e.to_string()))
}

fn qq_media_file_type(body: &CanonicalMessageBody) -> Option<i32> {
    match body.kind() {
        MessageBodyKind::Image => Some(1),
        MessageBodyKind::Video => Some(2),
        MessageBodyKind::Audio => Some(3),
        MessageBodyKind::File => Some(4),
        _ => None,
    }
}

fn qq_message_text_fallback(message: &QueuedOutboundMessage) -> String {
    let text = message.body.text_projection();
    if text.trim().is_empty() {
        message.content.clone()
    } else {
        text
    }
}

fn qq_upload_media_url(chat_id: &str) -> crate::error::Result<String> {
    if let Some(group_openid) = chat_id.strip_prefix("group:") {
        return Ok(format!("{QQ_V2_BASE}/groups/{group_openid}/files"));
    }
    if let Some(user_openid) = chat_id.strip_prefix("c2c:") {
        return Ok(format!("{QQ_V2_BASE}/users/{user_openid}/files"));
    }
    Err(crate::error::Error::config(
        "qq_send",
        format!("qq media upload unsupported for chat_id={chat_id}"),
    ))
}

#[derive(serde::Deserialize)]
struct QqRichMediaUploadResponse {
    #[serde(default)]
    file_info: Option<String>,
    #[serde(default)]
    ttl: Option<u32>,
}

fn resolve_qq_media_file_info<H: ChannelHttpClient>(
    http: &mut H,
    token: &str,
    chat_id: &str,
    message: &QueuedOutboundMessage,
) -> crate::error::Result<String> {
    let body = match &message.body {
        CanonicalMessageBody::Image(image) => &image.asset,
        CanonicalMessageBody::Audio(audio) => &audio.asset,
        CanonicalMessageBody::Video(video) => &video.asset,
        CanonicalMessageBody::File(file) => &file.asset,
        _ => {
            return Err(crate::error::Error::config(
                "qq_send",
                "qq media send requires media body",
            ));
        }
    };
    match body.locator_kind {
        MediaLocatorKind::PlatformHandle => {
            let locator = body.locator.trim();
            if locator.is_empty() {
                return Err(crate::error::Error::config(
                    "qq_send",
                    "qq media platform handle is empty",
                ));
            }
            Ok(locator.to_string())
        }
        MediaLocatorKind::ExternalUrl => {
            let locator = body.locator.trim();
            if locator.is_empty() {
                return Err(crate::error::Error::config(
                    "qq_send",
                    "qq media external url is empty",
                ));
            }
            let file_type = qq_media_file_type(&message.body).ok_or_else(|| {
                crate::error::Error::config("qq_send", "unsupported qq media body kind")
            })?;
            if file_type == 4 && chat_id.starts_with("group:") {
                return Err(crate::error::Error::config(
                    "qq_send",
                    "qq group rich media upload does not support file body",
                ));
            }
            let url = qq_upload_media_url(chat_id)?;
            let auth_header = format!("QQBot {token}");
            let headers = [
                ("Authorization", auth_header.as_str()),
                ("content-type", "application/json"),
            ];
            let body_bytes = serde_json::to_vec(&serde_json::json!({
                "file_type": file_type,
                "url": locator,
                "srv_send_msg": false,
            }))
            .map_err(|e| crate::error::Error::config("qq_send", e.to_string()))?;
            let (status, response_body) = crate::channels::send::send_post_with_headers(
                "qq_send",
                http,
                &url,
                &headers,
                &body_bytes,
            )?;
            if status >= 400 {
                return Err(crate::error::Error::http("qq_send_http", status));
            }
            let response: QqRichMediaUploadResponse =
                serde_json::from_slice(response_body.as_ref())
                    .map_err(|e| crate::error::Error::config("qq_send", e.to_string()))?;
            let file_info = response.file_info.unwrap_or_default();
            if file_info.trim().is_empty() {
                return Err(crate::error::Error::config(
                    "qq_send",
                    "qq rich media upload returned empty file_info",
                ));
            }
            if let Some(ttl) = response.ttl {
                log::debug!(
                    "[qq_send] uploaded rich media chat_id={} ttl_seconds={}",
                    chat_id,
                    ttl
                );
            }
            Ok(file_info)
        }
        MediaLocatorKind::BeetleBlob => Err(crate::error::Error::config(
            "qq_send",
            "qq media send does not support BeetleBlob yet",
        )),
    }
}

fn render_qq_send_payloads<H: ChannelHttpClient>(
    http: &mut H,
    token: &str,
    message: &QueuedOutboundMessage,
    msg_id: Option<&str>,
    msg_seq: Option<QqMsgSeqReservation>,
) -> crate::error::Result<Vec<ByteBuffer>> {
    match &message.body {
        CanonicalMessageBody::Text(body) if body.format == TextFormat::Markdown => {
            Ok(vec![ByteBuffer::from_vec(build_qq_markdown_body(
                &body.text,
                msg_id,
                msg_seq.and_then(|reservation| reservation.seq_for_chunk(0)),
            )?)])
        }
        CanonicalMessageBody::Text(_) => {
            let is_v2 = is_v2_chat(&message.chat_id);
            let chunks = crate::channels::chunk::chunk_text_by_char_count(
                &message.content,
                QQ_MAX_MESSAGE_LEN,
            );
            let mut payloads = Vec::with_capacity(chunks.len().max(1));
            for (index, chunk) in chunks.iter().enumerate() {
                payloads.push(build_qq_send_body(
                    chunk,
                    if index == 0 { msg_id } else { None },
                    if is_v2 {
                        msg_seq.and_then(|reservation| reservation.seq_for_chunk(index))
                    } else {
                        None
                    },
                )?);
            }
            Ok(payloads)
        }
        CanonicalMessageBody::Card(body) => match body.format {
            CardFormat::Ark => Ok(vec![ByteBuffer::from_vec(build_qq_card_body(
                3,
                "ark",
                &body.payload_json,
                msg_id,
                msg_seq.and_then(|reservation| reservation.seq_for_chunk(0)),
            )?)]),
            CardFormat::Embed => Ok(vec![ByteBuffer::from_vec(build_qq_card_body(
                4,
                "embed",
                &body.payload_json,
                msg_id,
                msg_seq.and_then(|reservation| reservation.seq_for_chunk(0)),
            )?)]),
            _ => Ok(vec![build_qq_send_body(
                &qq_message_text_fallback(message),
                msg_id,
                msg_seq.and_then(|reservation| reservation.seq_for_chunk(0)),
            )?]),
        },
        CanonicalMessageBody::Image(_)
        | CanonicalMessageBody::Audio(_)
        | CanonicalMessageBody::Video(_)
        | CanonicalMessageBody::File(_) => {
            let file_info = resolve_qq_media_file_info(http, token, &message.chat_id, message)?;
            Ok(vec![ByteBuffer::from_vec(build_qq_media_body(
                &file_info,
                msg_id,
                msg_seq.and_then(|reservation| reservation.seq_for_chunk(0)),
            )?)])
        }
        CanonicalMessageBody::PlatformNative(_) => Ok(vec![build_qq_send_body(
            &qq_message_text_fallback(message),
            msg_id,
            msg_seq.and_then(|reservation| reservation.seq_for_chunk(0)),
        )?]),
    }
}

/// 发送单条 QQ 消息（含自动分片）。返回 `Ok(())` 表示所有分片都成功（HTTP 2xx）。
/// 任一分片 HTTP 失败或 4xx+ 即返回 `Err`，供 sender loop 决定重试/熔断。
fn send_one_qq<H: ChannelHttpClient>(
    http: &mut H,
    token: &str,
    message: &QueuedOutboundMessage,
    msg_id: Option<&str>,
    msg_seq: Option<QqMsgSeqReservation>,
) -> crate::error::Result<()> {
    const TAG: &str = "qq_send";
    if message.content.trim().is_empty() && message.body.kind() == MessageBodyKind::Text {
        return Err(crate::error::Error::config(
            "qq_send_empty",
            "refusing to send empty QQ message",
        ));
    }
    if is_v2_chat(&message.chat_id) && msg_id.is_none() {
        return Err(crate::error::Error::config(
            "qq_send",
            format!(
                "missing msg_id for QQ v2 passive reply chat_id={}",
                message.chat_id
            ),
        ));
    }
    let send_start = std::time::Instant::now();
    let url = build_qq_message_url(&message.chat_id);
    let payloads = render_qq_send_payloads(http, token, message, msg_id, msg_seq)?;
    let max_payload_len = payloads
        .iter()
        .map(|payload| payload.len())
        .max()
        .unwrap_or(0);
    for (i, body_bytes) in payloads.iter().enumerate() {
        let auth_header = format!("QQBot {}", token);
        let mut cl_buf = [0u8; 20];
        let content_length = crate::util::usize_to_decimal_buf(&mut cl_buf, body_bytes.len());
        let headers = [
            ("Authorization", auth_header.as_str()),
            ("content-type", "application/json"),
            ("content-length", content_length),
        ];
        let http_start = std::time::Instant::now();
        match crate::channels::send::send_post_with_headers(
            TAG,
            http,
            &url,
            &headers,
            body_bytes.as_ref(),
        ) {
            Ok((status, ref body)) if status >= 400 => {
                let preview =
                    String::from_utf8_lossy(&body.as_ref()[..body.as_ref().len().min(256)]);
                log::warn!(
                    "[{}] send status={} body={} chat_id={} chunk={}/{} http_ms={} total_ms={}",
                    TAG,
                    status,
                    preview,
                    message.chat_id,
                    i + 1,
                    payloads.len(),
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
                    message.chat_id,
                    i + 1,
                    payloads.len(),
                    http_start.elapsed().as_millis(),
                    send_start.elapsed().as_millis()
                );
                return Err(e);
            }
            _ => {}
        }
    }
    log::debug!(
        "[latency][qq_http] chat_id={} chunks={} max_payload_b={} total_ms={}",
        message.chat_id,
        payloads.len(),
        max_payload_len,
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
        let _reply_priority =
            crate::channels::send::begin_reply_http_priority_scope(message.outbound_kind);
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
            &message,
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
    let _reply_priority =
        crate::channels::send::begin_reply_http_priority_scope(message.outbound_kind);

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
        message,
        reservation.msg_id.as_deref(),
        reservation.msg_seq,
    ) {
        Ok(()) => {
            release_retryable_send_reservation(
                runtime.active_reservation,
                message.transport_send_id,
            );
            if !is_supplemental {
                if runtime.record_channel_health {
                    crate::orchestrator::record_channel_result_pub("qq_channel", true);
                }
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
                if runtime.record_channel_health {
                    crate::orchestrator::record_channel_result_pub("qq_channel", false);
                }
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
            if !matches!(error, crate::error::Error::Config { .. }) {
                *runtime.http = None;
                invalidate_cached_qq_token(runtime.token_cache);
                clear_shared_cached_qq_token(runtime.shared_token_cache);
            }
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
        record_channel_health: true,
    };
    run_buffered_sender_loop(rx, TAG, |message, attempt| {
        feed_sender_loop_wdt();
        send_queued_qq_message(message, attempt, &mut runtime)
    });
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
pub(crate) struct QqOutboundDriver {
    app_id: String,
    secret: String,
    cache: QqMsgIdCache,
    shared_token_cache: SharedQqTokenCache,
    http: Option<Box<dyn PlatformHttpClient>>,
    token_cache: Option<CachedQqToken>,
    turn_tracker: QqTurnReservationTracker,
    active_reservation: Option<QqRetryableSendReservation>,
    create_http: Arc<dyn Fn() -> crate::Result<Box<dyn PlatformHttpClient>> + Send + Sync>,
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
pub(crate) fn qq_outbound_driver(
    app_id: String,
    secret: String,
    cache: QqMsgIdCache,
    shared_token_cache: SharedQqTokenCache,
    create_http: Arc<dyn Fn() -> crate::Result<Box<dyn PlatformHttpClient>> + Send + Sync>,
) -> Box<dyn ActiveChannelSender> {
    Box::new(QqOutboundDriver {
        app_id,
        secret,
        cache,
        shared_token_cache,
        http: None,
        token_cache: None,
        turn_tracker: QqTurnReservationTracker::default(),
        active_reservation: None,
        create_http,
    })
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
impl ActiveChannelSender for QqOutboundDriver {
    fn tag(&self) -> &'static str {
        "qq_sender"
    }

    fn send_attempt(
        &mut self,
        message: &QueuedOutboundMessage,
        attempt: u8,
    ) -> crate::error::Result<()> {
        let create_http = Arc::clone(&self.create_http);
        let mut create = || create_http();
        let mut runtime = QqSendRuntime {
            app_id: &self.app_id,
            secret: &self.secret,
            cache: &self.cache,
            shared_token_cache: &self.shared_token_cache,
            http: &mut self.http,
            token_cache: &mut self.token_cache,
            turn_tracker: &mut self.turn_tracker,
            active_reservation: &mut self.active_reservation,
            create_http: &mut create,
            record_channel_health: false,
        };
        send_queued_qq_message(message, attempt, &mut runtime)
    }
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
        queued_message_with_body(
            transport_send_id,
            chat_id,
            content,
            crate::bus::CanonicalMessageBody::text(content),
            req_id,
            outbound_kind,
        )
    }

    fn queued_message_with_body(
        transport_send_id: u32,
        chat_id: &str,
        content: &str,
        body: crate::bus::CanonicalMessageBody,
        req_id: Option<&str>,
        outbound_kind: OutboundKind,
    ) -> QueuedOutboundMessage {
        QueuedOutboundMessage {
            transport_send_id,
            chat_id: chat_id.to_string(),
            content: content.to_string(),
            body,
            platform_thread_id: String::new(),
            platform_message_id: String::new(),
            req_id: req_id.map(str::to_string),
            outbound_kind,
        }
    }

    #[test]
    fn large_plain_text_body_uses_external_preferred_buffer() {
        let content = "甲".repeat(ByteBuffer::EXTERNAL_PREFERRED_THRESHOLD);

        let body = build_qq_send_body(&content, Some("msg-1"), Some(1)).expect("qq body");

        assert!(body.is_external_preferred());
        let parsed: serde_json::Value = serde_json::from_slice(body.as_ref()).expect("json body");
        assert_eq!(parsed["content"], content);
        assert_eq!(parsed["msg_id"], "msg-1");
        assert_eq!(parsed["msg_seq"], 1);
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
    fn retryable_reservation_uses_message_platform_message_id_when_cache_is_empty() {
        let cache: QqMsgIdCache = Arc::new(Mutex::new(HashMap::new()));
        let mut message = queued_message(
            42,
            "c2c:chat-1",
            "hello",
            Some("req-1"),
            OutboundKind::Primary,
        );
        message.platform_message_id = "msg-1".to_string();
        let mut active = None;
        let mut turn_tracker = QqTurnReservationTracker::default();

        let reservation =
            resolve_retryable_send_reservation(&mut active, &mut turn_tracker, &cache, &message);

        assert_eq!(reservation.msg_id.as_deref(), Some("msg-1"));
        assert_eq!(pop_msg_id(&cache, "c2c:chat-1"), None);
    }

    #[test]
    fn retryable_reservation_prefers_turn_platform_message_id_over_chat_cache() {
        let cache: QqMsgIdCache = Arc::new(Mutex::new(HashMap::new()));
        cache_msg_id(&cache, "c2c:chat-1", "msg-b").expect("cache msg_id");
        let mut message = queued_message(
            43,
            "c2c:chat-1",
            "reply A",
            Some("req-a"),
            OutboundKind::Primary,
        );
        message.platform_message_id = "msg-a".to_string();
        let mut active = None;
        let mut turn_tracker = QqTurnReservationTracker::default();

        let reservation =
            resolve_retryable_send_reservation(&mut active, &mut turn_tracker, &cache, &message);

        assert_eq!(reservation.msg_id.as_deref(), Some("msg-a"));
        assert_eq!(pop_msg_id(&cache, "c2c:chat-1").as_deref(), Some("msg-b"));
    }

    #[test]
    fn retryable_reservation_fills_missing_anchor_when_primary_follows_supplemental() {
        let cache: QqMsgIdCache = Arc::new(Mutex::new(HashMap::new()));
        let supplemental = queued_message(
            44,
            "c2c:chat-1",
            "ack",
            Some("req-b"),
            OutboundKind::Supplemental,
        );
        let mut primary = queued_message(
            45,
            "c2c:chat-1",
            "reply B",
            Some("req-b"),
            OutboundKind::Primary,
        );
        primary.platform_message_id = "msg-b".to_string();
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

        assert_eq!(first.msg_id, None);
        assert_eq!(second.msg_id.as_deref(), Some("msg-b"));
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
            record_channel_health: true,
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

    #[test]
    fn primary_reply_token_uses_reply_critical_priority() {
        assert_eq!(
            super::reply_http_priority_for_message_kind(OutboundKind::Primary),
            crate::orchestrator::Priority::Critical
        );
        assert_eq!(
            super::reply_http_priority_for_message_kind(OutboundKind::Supplemental),
            crate::orchestrator::Priority::Normal
        );
    }

    #[test]
    fn v2_send_requires_cached_msg_id_for_passive_reply() {
        let mut http = StubHttp::default();
        let message = queued_message(1, "c2c:chat-1", "final reply", None, OutboundKind::Primary);
        let err = send_one_qq(&mut http, "qq-token", &message, None, None)
            .expect_err("missing msg_id should be rejected");

        match err {
            crate::error::Error::Config { stage, .. } => assert_eq!(stage, "qq_send"),
            other => panic!("unexpected error: {other:?}"),
        }
        assert!(http
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .sent_bodies
            .is_empty());
    }

    #[test]
    fn send_one_qq_renders_markdown_body_with_msg_type_two() {
        let mut http = StubHttp::default();
        let message = queued_message_with_body(
            1,
            "c2c:chat-1",
            "## Hello",
            crate::bus::CanonicalMessageBody::Text(crate::bus::TextBody {
                text: "## Hello".to_string(),
                format: crate::bus::TextFormat::Markdown,
            }),
            Some("req-1"),
            OutboundKind::Primary,
        );

        send_one_qq(
            &mut http,
            "qq-token",
            &message,
            Some("msg-1"),
            Some(QqMsgSeqReservation {
                start: 1,
                chunk_count: 1,
            }),
        )
        .expect("markdown send");

        let guard = http.state.lock().unwrap_or_else(|e| e.into_inner());
        assert_eq!(guard.sent_bodies.len(), 1);
        let payload: serde_json::Value =
            serde_json::from_slice(&guard.sent_bodies[0]).expect("payload json");
        assert_eq!(payload.get("msg_type"), Some(&serde_json::json!(2)));
        assert_eq!(
            payload
                .get("markdown")
                .and_then(|markdown| markdown.get("content"))
                .and_then(|content| content.as_str()),
            Some("## Hello")
        );
        assert_eq!(
            payload.get("content").and_then(|content| content.as_str()),
            Some("Hello")
        );
    }

    #[test]
    fn send_one_qq_uploads_external_image_before_sending_media_payload() {
        let state = Arc::new(Mutex::new(StubHttpState {
            token_results: VecDeque::new(),
            send_results: VecDeque::from([
                Ok((
                    200,
                    ResponseBody::Heap(br#"{"file_info":"file-1","ttl":60}"#.to_vec()),
                )),
                Ok((200, ResponseBody::Heap(b"{}".to_vec()))),
            ]),
            sent_bodies: Vec::new(),
        }));
        let mut http = StubHttp {
            state: Arc::clone(&state),
        };
        let message = queued_message_with_body(
            2,
            "c2c:user-1",
            "[image] kitten",
            crate::bus::CanonicalMessageBody::Image(crate::bus::ImageBody {
                asset: crate::bus::MediaAssetRef::external_url("https://example.com/cat.png"),
                caption: None,
            }),
            Some("req-1"),
            OutboundKind::Primary,
        );

        send_one_qq(
            &mut http,
            "qq-token",
            &message,
            Some("msg-1"),
            Some(QqMsgSeqReservation {
                start: 1,
                chunk_count: 1,
            }),
        )
        .expect("media send");

        let guard = state.lock().unwrap_or_else(|e| e.into_inner());
        assert_eq!(guard.sent_bodies.len(), 2);
        let upload_payload: serde_json::Value =
            serde_json::from_slice(&guard.sent_bodies[0]).expect("upload payload");
        assert_eq!(upload_payload.get("file_type"), Some(&serde_json::json!(1)));
        assert_eq!(
            upload_payload.get("srv_send_msg"),
            Some(&serde_json::json!(false))
        );
        let send_payload: serde_json::Value =
            serde_json::from_slice(&guard.sent_bodies[1]).expect("send payload");
        assert_eq!(send_payload.get("msg_type"), Some(&serde_json::json!(7)));
        assert_eq!(
            send_payload
                .get("media")
                .and_then(|media| media.get("file_info"))
                .and_then(|file_info| file_info.as_str()),
            Some("file-1")
        );
    }

    #[test]
    fn connectivity_requires_websocket_online_even_when_token_is_valid() {
        let mut state = StubHttpState::default();
        state.token_results.push_back(Ok((
            200,
            ResponseBody::Heap(br#"{"access_token":"qq-token","expires_in":7200}"#.to_vec()),
        )));
        let mut http = StubHttp {
            state: Arc::new(Mutex::new(state)),
        };
        let mut config = AppConfig::load_from_env();
        config.qq_channel_app_id = "app-id".to_string();
        config.qq_channel_secret = "secret".to_string();

        let item = check_connectivity(&config, &mut http);

        assert!(!item.ok);
        assert_eq!(item.id, "qq_channel");
        assert!(item.configured);
        assert!(item.message_key.is_some());
    }
}
