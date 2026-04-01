//! QQ 频道出站：flush、sign/verify、msg_id 缓存、连通性检查。Sink 统一为 dispatch::QueuedSink。

use crate::channels::ChannelHttpClient;
use crate::config::AppConfig;
use crate::error::{Error, Result};
use ed25519_dalek::{Signer, SigningKey};
use hex;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// 单条消息最大字符数，与现有通道对齐。
const QQ_MAX_MESSAGE_LEN: usize = 4096;

/// 被动回复 msg_id 有效时长（秒）。
const QQ_MSG_ID_TTL_SECS: u64 = 300;

/// 将 Bot Secret 字符串重复至 32 字节作为 Ed25519 种子；不足则循环填充。
/// Panics if secret is empty — caller must validate.
fn secret_to_seed(secret: &str) -> [u8; 32] {
    let mut seed = [0u8; 32];
    let bytes = secret.as_bytes();
    // Empty secret produces all-zero seed which is cryptographically weak;
    // callers (sign/verify/flush) already guard with is_empty() checks,
    // but log a warning as defence-in-depth.
    if bytes.is_empty() {
        log::warn!("[qq_send] secret_to_seed called with empty secret");
        return seed;
    }
    for (i, b) in seed.iter_mut().enumerate() {
        *b = bytes[i % bytes.len()];
    }
    seed
}

/// op=13：对 event_ts + plain_token 做 Ed25519 签名，返回 hex 编码的 signature。
pub fn sign_qq_url_verify(secret: &str, event_ts: &str, plain_token: &str) -> Result<String> {
    let seed = secret_to_seed(secret);
    let signing_key = SigningKey::from_bytes(&seed);
    let message = format!("{}{}", event_ts, plain_token);
    let signature = signing_key.sign(message.as_bytes());
    Ok(hex::encode(signature.to_bytes()))
}

/// op=0：校验 X-Signature-Ed25519、X-Signature-Timestamp 与 body 的 Ed25519 验签。
pub fn verify_qq_signature(
    secret: &str,
    timestamp: &str,
    body: &[u8],
    signature_hex: &str,
) -> Result<()> {
    let seed = secret_to_seed(secret);
    let signing_key = SigningKey::from_bytes(&seed);
    let verifying_key = signing_key.verifying_key();
    let sig_bytes: [u8; 64] = hex::decode(signature_hex)
        .map_err(|e| Error::config("qq_verify_hex", e.to_string()))?
        .try_into()
        .map_err(|_| Error::config("qq_verify", "signature length must be 64 bytes"))?;
    let signature = ed25519_dalek::Signature::from_bytes(&sig_bytes);
    let message: Vec<u8> = timestamp
        .as_bytes()
        .iter()
        .chain(body.iter())
        .copied()
        .collect();
    verifying_key
        .verify_strict(&message, &signature)
        .map_err(|_| Error::config("qq_verify", "signature verification failed"))?;
    Ok(())
}

/// msg_id 缓存类型：channel_id -> (msg_id, unix_ts)。上限 QQ_MSG_ID_CACHE_MAX 条。
pub type QqMsgIdCache = Arc<Mutex<HashMap<String, (String, u64)>>>;

/// msg_id 缓存最大条目数。
const QQ_MSG_ID_CACHE_MAX: usize = 64;

pub const QQ_GET_APP_ACCESS_TOKEN_URL: &str = "https://bots.qq.com/app/getAppAccessToken";
const QQ_MESSAGES_BASE: &str = "https://api.sgroup.qq.com/channels";
const QQ_V2_BASE: &str = "https://api.sgroup.qq.com/v2";

#[derive(serde::Serialize)]
pub struct QqTokenRequest {
    #[serde(rename = "appId")]
    pub app_id: String,
    #[serde(rename = "clientSecret")]
    pub client_secret: String,
}

#[derive(serde::Deserialize)]
pub struct QqTokenResponse {
    pub access_token: Option<String>,
    #[serde(default, deserialize_with = "deserialize_u64_or_string")]
    #[allow(dead_code)]
    pub expires_in: u64,
}

/// QQ API 的 expires_in 可能返回数字或字符串，兼容两种格式。
fn deserialize_u64_or_string<'de, D>(deserializer: D) -> std::result::Result<u64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize;
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum U64OrString {
        U64(u64),
        Str(String),
    }
    match U64OrString::deserialize(deserializer)? {
        U64OrString::U64(v) => Ok(v),
        U64OrString::Str(s) => s.parse::<u64>().map_err(serde::de::Error::custom),
    }
}

/// 连通性检查：供 GET /api/channel_connectivity 使用。
pub fn check_connectivity<H: ChannelHttpClient + ?Sized>(
    config: &AppConfig,
    http: &mut H,
    loc: crate::i18n::Locale,
) -> super::super::connectivity::ChannelConnectivityItem {
    use super::super::connectivity;
    use crate::i18n::{tr, Message};
    let configured =
        !config.qq_channel_app_id.trim().is_empty() && !config.qq_channel_secret.trim().is_empty();
    if !configured {
        return connectivity::item(
            "qq_channel",
            false,
            false,
            Some(tr(Message::ConnectivityNotConfigured, loc)),
        );
    }
    let body = QqTokenRequest {
        app_id: config.qq_channel_app_id.trim().to_string(),
        client_secret: config.qq_channel_secret.trim().to_string(),
    };
    let body_bytes = match serde_json::to_vec(&body) {
        Ok(b) => b,
        Err(e) => {
            log::warn!("[qq_connectivity] json: {}", e);
            return connectivity::item(
                "qq_channel",
                configured,
                false,
                Some(tr(Message::ConnectivityCheckFailed, loc)),
            );
        }
    };
    let (status, resp_body) = match http.http_post(QQ_GET_APP_ACCESS_TOKEN_URL, &body_bytes) {
        Ok(r) => r,
        Err(e) => {
            log::warn!("[qq_connectivity] post: {}", e);
            return connectivity::item(
                "qq_channel",
                configured,
                false,
                Some(tr(Message::ConnectivityCheckFailed, loc)),
            );
        }
    };
    if status >= 400 {
        log::warn!("[qq_connectivity] status {}", status);
        return connectivity::item(
            "qq_channel",
            configured,
            false,
            Some(tr(Message::ConnectivityTokenInvalid, loc)),
        );
    }
    let r: QqTokenResponse = match serde_json::from_slice(resp_body.as_ref()) {
        Ok(x) => x,
        Err(e) => {
            log::warn!("[qq_connectivity] parse: {}", e);
            return connectivity::item(
                "qq_channel",
                configured,
                false,
                Some(tr(Message::ConnectivityCheckFailed, loc)),
            );
        }
    };
    match r.access_token {
        Some(t) if !t.is_empty() => connectivity::item("qq_channel", configured, true, None),
        _ => connectivity::item(
            "qq_channel",
            configured,
            false,
            Some(tr(Message::ConnectivityTokenInvalid, loc)),
        ),
    }
}

/// Returns `(access_token, expires_in_secs)` from QQ API for caching.
fn acquire_qq_token_with_expiry<H: ChannelHttpClient>(
    http: &mut H,
    app_id: &str,
    secret: &str,
) -> Option<(String, u64)> {
    const TAG: &str = "qq_send";
    let body = QqTokenRequest {
        app_id: app_id.to_string(),
        client_secret: secret.to_string(),
    };
    let body_bytes = match serde_json::to_vec(&body) {
        Ok(b) => b,
        Err(e) => {
            log::warn!("[{}] token json: {}", TAG, e);
            return None;
        }
    };
    let (status, resp_body) = match http.http_post(QQ_GET_APP_ACCESS_TOKEN_URL, &body_bytes) {
        Ok(r) => r,
        Err(e) => {
            log::warn!("[{}] getAppAccessToken failed: {}", TAG, e);
            return None;
        }
    };
    if status >= 400 {
        log::warn!("[{}] getAppAccessToken status={}", TAG, status);
        return None;
    }
    let token_resp: QqTokenResponse = match serde_json::from_slice(resp_body.as_ref()) {
        Ok(t) => t,
        Err(e) => {
            log::warn!("[{}] token parse: {}", TAG, e);
            return None;
        }
    };
    match token_resp.access_token {
        Some(t) if !t.is_empty() => {
            let exp = token_resp.expires_in.max(60);
            Some((t, exp))
        }
        _ => {
            log::warn!("[{}] no access_token in response", TAG);
            None
        }
    }
}

fn acquire_qq_token<H: ChannelHttpClient>(
    http: &mut H,
    app_id: &str,
    secret: &str,
) -> Option<String> {
    acquire_qq_token_with_expiry(http, app_id, secret).map(|(t, _)| t)
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

/// 发送单条 QQ 消息（含自动分片）。返回 `Ok(())` 表示所有分片都成功（HTTP 2xx）。
/// 任一分片 HTTP 失败或 4xx+ 即返回 `Err`，供 sender loop 决定重试/熔断。
fn send_one_qq<H: ChannelHttpClient>(
    http: &mut H,
    token: &str,
    chat_id: &str,
    content: &str,
    msg_id: Option<&str>,
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
        let mut body_obj = serde_json::json!({ "content": chunk });
        if v2 {
            body_obj["msg_type"] = serde_json::json!(0);
            body_obj["msg_seq"] = serde_json::json!(i + 1);
        }
        if i == 0 {
            if let Some(id) = msg_id {
                body_obj["msg_id"] = serde_json::json!(id);
            }
        }
        let body_bytes = serde_json::to_vec(&body_obj).map_err(|e| {
            crate::error::Error::config("qq_send_json", format!("serialize failed: {}", e))
        })?;
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
    log::info!(
        "[latency][qq_http] chat_id={} chunks={} total_ms={}",
        chat_id,
        chunks.len(),
        send_start.elapsed().as_millis()
    );
    Ok(())
}

fn pop_msg_id(cache: &QqMsgIdCache, chat_id: &str) -> Option<String> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    cache.lock().ok().and_then(|mut c| {
        // Evict expired entries to prevent unbounded growth.
        if c.len() > QQ_MSG_ID_CACHE_MAX / 2 {
            c.retain(|_, (_, ts)| now.saturating_sub(*ts) <= QQ_MSG_ID_TTL_SECS);
        }
        let entry = c.get(chat_id).map(|(id, ts)| (id.clone(), *ts));
        if let Some((id_clone, ts)) = entry {
            if now.saturating_sub(ts) <= QQ_MSG_ID_TTL_SECS {
                c.remove(chat_id);
                return Some(id_clone);
            }
            // Expired — remove stale entry.
            c.remove(chat_id);
        }
        None
    })
}

/// 从 rx 取出待发送（一次性 drain）。
pub fn flush_qq_channel_sends<H: ChannelHttpClient>(
    rx: &std::sync::mpsc::Receiver<(String, String, Option<String>)>,
    app_id: &str,
    secret: &str,
    cache: QqMsgIdCache,
    http: &mut H,
) {
    if app_id.is_empty() || secret.is_empty() {
        return;
    }
    let token = match acquire_qq_token(http, app_id, secret) {
        Some(t) => t,
        None => return,
    };
    while let Ok((chat_id, content, req_id)) = rx.try_recv() {
        let msg_id = pop_msg_id(&cache, &chat_id);
        if let Err(e) = send_one_qq(http, &token, &chat_id, &content, msg_id.as_deref()) {
            crate::metrics::record_channel_http_result(false);
            log::warn!(
                "[qq_flush] req_id={} send failed for chat_id={}: {}",
                req_id.as_deref().unwrap_or("-"),
                chat_id,
                e
            );
        } else {
            crate::metrics::record_channel_http_result(true);
        }
    }
}

/// QQ access_token 缓存提前刷新余量（秒），避免用即将过期的 token。
const QQ_TOKEN_CACHE_MARGIN_SECS: u64 = 120;

/// 持续运行的 QQ 频道发送循环：本线程**复用**同一 HTTP 客户端（少占 lwIP socket，避免与 WSS 抢 fd），
/// 并按 `expires_in` **缓存** token，减少 `getAppAccessToken` 调用。
pub fn run_qq_sender_loop<H, F>(
    rx: std::sync::mpsc::Receiver<(String, String, Option<String>)>,
    app_id: &str,
    secret: &str,
    cache: QqMsgIdCache,
    mut create_http: F,
) where
    H: ChannelHttpClient,
    F: FnMut() -> crate::error::Result<H>,
{
    const TAG: &str = "qq_sender";
    if app_id.is_empty() || secret.is_empty() {
        return;
    }
    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
    crate::platform::task_wdt::register_current_task_to_task_wdt();
    log::info!("[{}] sender loop started", TAG);

    let mut http: Option<H> = None;
    let mut token_cache: Option<(String, std::time::Instant)> = None;

    let recv_timeout = std::time::Duration::from_secs(30);
    loop {
        let (chat_id, content, req_id) = match rx.recv_timeout(recv_timeout) {
            Ok(item) => item,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                crate::platform::task_wdt::feed_current_task();
                continue;
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                log::info!("[{}] rx disconnected, exiting", TAG);
                break;
            }
        };
        crate::platform::task_wdt::feed_current_task();
        let msg_start = std::time::Instant::now();
        let mut token_wait_ms: u128 = 0;
        let mut sent = false;
        for retry in 0..3u8 {
            if retry > 0 {
                let delay_ms = if retry == 1 {
                    crate::constants::QQ_SEND_RETRY_DELAY_MS_STEP1
                } else {
                    crate::constants::QQ_SEND_RETRY_DELAY_MS_STEP2
                };
                std::thread::sleep(std::time::Duration::from_millis(delay_ms));
                crate::platform::task_wdt::feed_current_task();
            }

            let now = std::time::Instant::now();
            let mut token_opt: Option<String> = token_cache
                .as_ref()
                .filter(|(_, exp)| now < *exp)
                .map(|(t, _)| t.clone());
            if token_opt.is_none() {
                token_cache = None;
                if http.is_none() {
                    match create_http() {
                        Ok(h) => http = Some(h),
                        Err(e) => {
                            log::warn!(
                                "[{}] create http failed (attempt {}): {}",
                                TAG,
                                retry + 1,
                                e
                            );
                            continue;
                        }
                    }
                }
                let Some(h) = http.as_mut() else {
                    continue;
                };
                let token_start = std::time::Instant::now();
                match acquire_qq_token_with_expiry(h, app_id, secret) {
                    Some((t, exp_secs)) => {
                        let keep = exp_secs.saturating_sub(QQ_TOKEN_CACHE_MARGIN_SECS).max(30);
                        token_cache = Some((t.clone(), now + std::time::Duration::from_secs(keep)));
                        token_opt = Some(t);
                        token_wait_ms =
                            token_wait_ms.saturating_add(token_start.elapsed().as_millis());
                    }
                    None => {
                        token_wait_ms =
                            token_wait_ms.saturating_add(token_start.elapsed().as_millis());
                        http = None;
                        continue;
                    }
                }
            }

            let token = match token_opt {
                Some(t) => t,
                None => continue,
            };

            if http.is_none() {
                match create_http() {
                    Ok(h) => http = Some(h),
                    Err(e) => {
                        log::warn!(
                            "[{}] create http failed (attempt {}): {}",
                            TAG,
                            retry + 1,
                            e
                        );
                        continue;
                    }
                }
            }
            let Some(h) = http.as_mut() else {
                continue;
            };
            let msg_id = pop_msg_id(&cache, &chat_id);
            let http_send_start = std::time::Instant::now();
            match send_one_qq(h, &token, &chat_id, &content, msg_id.as_deref()) {
                Ok(()) => {
                    crate::orchestrator::record_channel_result_pub("qq_channel", true);
                    crate::metrics::record_channel_http_result(true);
                    log::info!(
                        "[latency][qq_sender] req_id={} chat_id={} attempt={} token_wait_ms={} http_send_ms={} total_ms={} status=ok",
                        req_id.as_deref().unwrap_or("-"),
                        chat_id,
                        retry + 1,
                        token_wait_ms,
                        http_send_start.elapsed().as_millis(),
                        msg_start.elapsed().as_millis()
                    );
                }
                Err(ref e) => {
                    crate::orchestrator::record_channel_result_pub("qq_channel", false);
                    crate::metrics::record_channel_http_result(false);
                    log::warn!(
                        "[{}] req_id={} send failed (attempt {}): {} chat_id={} token_wait_ms={} http_send_ms={} total_ms={}",
                        TAG,
                        req_id.as_deref().unwrap_or("-"),
                        retry + 1,
                        e,
                        chat_id,
                        token_wait_ms,
                        http_send_start.elapsed().as_millis(),
                        msg_start.elapsed().as_millis()
                    );
                    http = None;
                    token_cache = None;
                    continue;
                }
            }
            while let Ok((cid, cnt, rid)) = rx.try_recv() {
                let mid = pop_msg_id(&cache, &cid);
                match send_one_qq(h, &token, &cid, &cnt, mid.as_deref()) {
                    Ok(()) => {
                        crate::orchestrator::record_channel_result_pub("qq_channel", true);
                        crate::metrics::record_channel_http_result(true);
                    }
                    Err(ref e) => {
                        crate::orchestrator::record_channel_result_pub("qq_channel", false);
                        crate::metrics::record_channel_http_result(false);
                        log::warn!(
                            "[{}] req_id={} drain send failed for {}: {}",
                            TAG,
                            rid.as_deref().unwrap_or("-"),
                            cid,
                            e
                        );
                    }
                }
            }
            sent = true;
            break;
        }
        if !sent {
            log::error!(
                "[{}] message dropped after 3 retries, chat_id={}",
                TAG,
                chat_id
            );
            log::error!(
                "[{}] req_id={} message dropped after retries",
                TAG,
                req_id.as_deref().unwrap_or("-")
            );
            crate::orchestrator::record_channel_result_pub("qq_channel", false);
        }
    }
}
