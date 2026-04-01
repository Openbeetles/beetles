//! 飞书出站：flush、token 类型、event_body_to_pcmsg、连通性检查。Sink 统一为 dispatch::QueuedSink。

use crate::bus::PcMsg;
use crate::channels::ChannelHttpClient;
use crate::config::AppConfig;

pub const FEISHU_TOKEN_URL: &str =
    "https://open.feishu.cn/open-apis/auth/v3/tenant_access_token/internal";
const FEISHU_SEND_URL: &str =
    "https://open.feishu.cn/open-apis/im/v1/messages?receive_id_type=chat_id";
const FEISHU_MAX_MESSAGE_LEN: usize = 4096;
/// Token 缓存提前刷新余量（秒），避免使用即将过期的 token。
const TOKEN_REFRESH_MARGIN_SECS: u64 = 300;

#[derive(serde::Serialize)]
pub struct FeishuTokenRequest {
    pub app_id: String,
    pub app_secret: String,
}

#[derive(serde::Deserialize)]
pub struct FeishuTokenResponse {
    pub tenant_access_token: Option<String>,
    #[serde(default)]
    pub code: i32,
}

pub fn acquire_tenant_token<H: ChannelHttpClient>(
    http: &mut H,
    app_id: &str,
    app_secret: &str,
) -> Option<String> {
    const TAG: &str = "feishu_send";
    let body = FeishuTokenRequest {
        app_id: app_id.to_string(),
        app_secret: app_secret.to_string(),
    };
    let body_bytes = match serde_json::to_vec(&body) {
        Ok(b) => b,
        Err(e) => {
            log::warn!("[{}] token json: {}", TAG, e);
            return None;
        }
    };
    let (status, resp_body) = match http.http_post(FEISHU_TOKEN_URL, &body_bytes) {
        Ok(r) => r,
        Err(e) => {
            log::warn!("[{}] tenant_access_token failed: {}", TAG, e);
            return None;
        }
    };
    if status >= 400 {
        log::warn!("[{}] token status={}", TAG, status);
        return None;
    }
    let token_resp: FeishuTokenResponse = match serde_json::from_slice(resp_body.as_ref()) {
        Ok(t) => t,
        Err(e) => {
            log::warn!("[{}] token parse: {}", TAG, e);
            return None;
        }
    };
    match token_resp.tenant_access_token {
        Some(t) if !t.is_empty() => Some(t),
        _ => {
            log::warn!("[{}] token empty code={}", TAG, token_resp.code);
            None
        }
    }
}

fn send_feishu_message<H: ChannelHttpClient>(
    http: &mut H,
    token: &str,
    chat_id: &str,
    content: &str,
) {
    const TAG: &str = "feishu_send";
    let auth_val = format!("Bearer {}", token);
    for chunk in crate::channels::chunk::chunk_text_by_char_count(content, FEISHU_MAX_MESSAGE_LEN) {
        let text_json = serde_json::json!({ "text": chunk });
        let content_str =
            serde_json::to_string(&text_json).unwrap_or_else(|_| "{\"text\":\"\"}".to_string());
        let body = serde_json::json!({
            "receive_id": chat_id,
            "msg_type": "text",
            "content": content_str,
        });
        let body_bytes = match serde_json::to_vec(&body) {
            Ok(b) => b,
            Err(e) => {
                log::warn!("[{}] send json: {}", TAG, e);
                continue;
            }
        };
        let headers = [
            ("Authorization", auth_val.as_str()),
            ("Content-Type", "application/json; charset=utf-8"),
        ];
        let _ = crate::channels::send::send_post_with_headers(
            TAG,
            http,
            FEISHU_SEND_URL,
            &headers,
            &body_bytes,
        );
    }
}

/// 从 rx 取出待发送，鉴权后调用飞书发消息 API（一次性 drain）。
pub fn flush_feishu_sends<H: ChannelHttpClient>(
    rx: &std::sync::mpsc::Receiver<(String, String, Option<String>)>,
    app_id: &str,
    app_secret: &str,
    http: &mut H,
) {
    if app_id.is_empty() || app_secret.is_empty() {
        return;
    }
    let token = match acquire_tenant_token(http, app_id, app_secret) {
        Some(t) => t,
        None => return,
    };
    while let Ok((chat_id, content, _req_id)) = rx.try_recv() {
        send_feishu_message(http, &token, &chat_id, &content);
    }
}

/// 持续运行的飞书发送循环：sender 线程内**复用**同一 HTTP；tenant_access_token 仍按 TTL 缓存，减少 getToken 次数。
pub fn run_feishu_sender_loop<H, F>(
    rx: std::sync::mpsc::Receiver<(String, String, Option<String>)>,
    app_id: &str,
    app_secret: &str,
    mut create_http: F,
) where
    H: ChannelHttpClient,
    F: FnMut() -> crate::error::Result<H>,
{
    const TAG: &str = "feishu_sender";
    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
    crate::platform::task_wdt::register_current_task_to_task_wdt();
    log::info!("[{}] sender loop started", TAG);

    let mut http: Option<H> = None;
    let recv_timeout = std::time::Duration::from_secs(30);
    // 飞书 tenant_access_token 有效期 2h，缓存避免每批消息都请求。
    let token_ttl = std::time::Duration::from_secs(7200 - TOKEN_REFRESH_MARGIN_SECS);
    let mut cached_token: Option<(String, std::time::Instant)> = None;
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

        let mut sent = false;
        for retry in 0..3u8 {
            if retry > 0 {
                std::thread::sleep(std::time::Duration::from_secs(2));
                crate::platform::task_wdt::feed_current_task();
            }
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
            let need_refresh = match &cached_token {
                None => true,
                Some((_, acquired_at)) => acquired_at.elapsed() >= token_ttl,
            };
            if need_refresh {
                cached_token = None;
                match acquire_tenant_token(h, app_id, app_secret) {
                    Some(t) => {
                        cached_token = Some((t, std::time::Instant::now()));
                    }
                    None => {
                        http = None;
                        continue;
                    }
                }
            }
            let Some((token, _)) = cached_token.as_ref() else {
                log::warn!("[{}] missing tenant token after refresh", TAG);
                continue;
            };
            send_feishu_message(h, token.as_str(), &chat_id, &content);
            while let Ok((cid, cnt, _)) = rx.try_recv() {
                send_feishu_message(h, token, &cid, &cnt);
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
            cached_token = None; // 连续失败后清除缓存，下次强制刷新
        }
    }
}

/// 发送消息并返回平台侧 message_id（字符串形式）；供流式编辑使用。
/// 需先调用 acquire_tenant_token 获取 token。
pub fn send_and_get_id<H: ChannelHttpClient>(
    http: &mut H,
    token: &str,
    chat_id: &str,
    content: &str,
) -> crate::error::Result<Option<String>> {
    let text_json = serde_json::json!({ "text": content });
    let content_str =
        serde_json::to_string(&text_json).unwrap_or_else(|_| "{\"text\":\"\"}".to_string());
    let body = serde_json::json!({
        "receive_id": chat_id,
        "msg_type": "text",
        "content": content_str,
    });
    let body_bytes = serde_json::to_vec(&body).map_err(|e| crate::error::Error::Other {
        source: Box::new(e),
        stage: "feishu_send",
    })?;
    let auth_val = format!("Bearer {}", token);
    let headers = [
        ("Authorization", auth_val.as_str()),
        ("Content-Type", "application/json; charset=utf-8"),
    ];
    let (status, resp_body) = http
        .http_post_with_headers(FEISHU_SEND_URL, &headers, &body_bytes)
        .map_err(|e| crate::error::Error::Other {
            source: Box::new(e),
            stage: "feishu_send",
        })?;
    if status >= 400 {
        return Err(crate::error::Error::Http {
            status_code: status,
            stage: "feishu_send",
        });
    }
    #[derive(serde::Deserialize)]
    struct R {
        data: Option<Inner>,
    }
    #[derive(serde::Deserialize)]
    struct Inner {
        message_id: Option<String>,
    }
    let r: R = match serde_json::from_slice(resp_body.as_ref()) {
        Ok(parsed) => parsed,
        Err(e) => {
            log::warn!("[feishu_send] failed to parse send response: {}", e);
            R { data: None }
        }
    };
    Ok(r.data.and_then(|d| d.message_id))
}

/// 编辑已发送的飞书消息（PATCH /im/v1/messages/{message_id}）。
pub fn edit_message<H: ChannelHttpClient>(
    http: &mut H,
    token: &str,
    message_id: &str,
    content: &str,
) -> crate::error::Result<()> {
    let text_json = serde_json::json!({ "text": content });
    let content_str =
        serde_json::to_string(&text_json).unwrap_or_else(|_| "{\"text\":\"\"}".to_string());
    let body = serde_json::json!({
        "msg_type": "text",
        "content": content_str,
    });
    let body_bytes = serde_json::to_vec(&body).map_err(|e| crate::error::Error::Other {
        source: Box::new(e),
        stage: "feishu_edit",
    })?;
    let url = format!(
        "https://open.feishu.cn/open-apis/im/v1/messages/{}",
        message_id
    );
    let auth_val = format!("Bearer {}", token);
    let headers = [
        ("Authorization", auth_val.as_str()),
        ("Content-Type", "application/json; charset=utf-8"),
    ];
    let (status, _) = http
        .http_patch_with_headers(&url, &headers, &body_bytes)
        .map_err(|e| crate::error::Error::Other {
            source: Box::new(e),
            stage: "feishu_edit",
        })?;
    if status >= 400 {
        return Err(crate::error::Error::Http {
            status_code: status,
            stage: "feishu_edit",
        });
    }
    Ok(())
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
        !config.feishu_app_id.trim().is_empty() && !config.feishu_app_secret.trim().is_empty();
    if !configured {
        return connectivity::item(
            "feishu",
            false,
            false,
            Some(tr(Message::ConnectivityNotConfigured, loc)),
        );
    }
    let body = FeishuTokenRequest {
        app_id: config.feishu_app_id.trim().to_string(),
        app_secret: config.feishu_app_secret.trim().to_string(),
    };
    let body_bytes = match serde_json::to_vec(&body) {
        Ok(b) => b,
        Err(e) => {
            log::warn!("[feishu_connectivity] json: {}", e);
            return connectivity::item(
                "feishu",
                configured,
                false,
                Some(tr(Message::ConnectivityCheckFailed, loc)),
            );
        }
    };
    let (status, resp_body) = match http.http_post(FEISHU_TOKEN_URL, &body_bytes) {
        Ok(r) => r,
        Err(e) => {
            log::warn!("[feishu_connectivity] post: {}", e);
            return connectivity::item(
                "feishu",
                configured,
                false,
                Some(tr(Message::ConnectivityCheckFailed, loc)),
            );
        }
    };
    if status >= 400 {
        log::warn!("[feishu_connectivity] token api status {}", status);
        return connectivity::item(
            "feishu",
            configured,
            false,
            Some(tr(Message::ConnectivityTokenInvalid, loc)),
        );
    }
    let r: FeishuTokenResponse = match serde_json::from_slice(resp_body.as_ref()) {
        Ok(x) => x,
        Err(e) => {
            log::warn!("[feishu_connectivity] parse: {}", e);
            return connectivity::item(
                "feishu",
                configured,
                false,
                Some(tr(Message::ConnectivityCheckFailed, loc)),
            );
        }
    };
    match r.tenant_access_token {
        Some(t) if !t.is_empty() => connectivity::item("feishu", configured, true, None),
        _ => {
            log::warn!("[feishu_connectivity] no token code={}", r.code);
            connectivity::item(
                "feishu",
                configured,
                false,
                Some(tr(Message::ConnectivityTokenInvalid, loc)),
            )
        }
    }
}

/// 从飞书事件 body（schema 2.0，含 header.event_type、event）解析出 im.message.receive_v1 文本消息，
/// 白名单校验通过则返回 PcMsg，否则 None。供 HTTP 回调与长连接入站共用。
pub fn event_body_to_pcmsg(event_body: &str, allowed_chat_ids: &[String]) -> Option<PcMsg> {
    const TAG: &str = "feishu_event_parse";
    let v: serde_json::Value = match serde_json::from_str(event_body) {
        Ok(x) => x,
        Err(_) => {
            log::debug!("[{}] body parse failed", TAG);
            return None;
        }
    };
    let event_type = v
        .get("header")
        .and_then(|h| h.get("event_type"))
        .and_then(|e| e.as_str());
    let event_type = match event_type {
        Some(t) => t,
        None => {
            log::debug!("[{}] missing header.event_type", TAG);
            return None;
        }
    };
    if event_type != "im.message.receive_v1" {
        log::debug!("[{}] skip event_type={}", TAG, event_type);
        return None;
    }
    let event = match v.get("event") {
        Some(e) => e,
        None => {
            log::debug!("[{}] missing event", TAG);
            return None;
        }
    };
    let message = match event.get("message") {
        Some(m) => m,
        None => {
            log::debug!("[{}] missing event.message", TAG);
            return None;
        }
    };
    let chat_id = message
        .get("chat_id")
        .and_then(|c| c.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    let chat_type = message
        .get("chat_type")
        .and_then(|c| c.as_str())
        .unwrap_or("");
    let message_type = message
        .get("message_type")
        .and_then(|m| m.as_str())
        .unwrap_or("");
    let content_str = message
        .get("content")
        .and_then(|c| c.as_str())
        .unwrap_or("");
    if message_type != "text" {
        log::debug!("[{}] skip message_type={}", TAG, message_type);
        return None;
    }
    let text = match serde_json::from_str::<serde_json::Value>(content_str) {
        Ok(c) => c
            .get("text")
            .and_then(|t| t.as_str())
            .unwrap_or("")
            .to_string(),
        Err(_) => String::new(),
    };
    let text = text.trim();
    if text.is_empty() {
        log::debug!("[{}] empty text", TAG);
        return None;
    }
    if allowed_chat_ids.is_empty() {
        log::warn!(
            "[{}] event dropped: allowed chat IDs not configured; add chat_id={} to channel config and save",
            TAG,
            chat_id
        );
        return None;
    }
    if !allowed_chat_ids.iter().any(|id| id.trim() == chat_id) {
        log::warn!(
            "[{}] event dropped: chat_id={} not in allowlist; add it to allowed chat IDs in channel config",
            TAG,
            chat_id
        );
        return None;
    }
    let is_group = matches!(chat_type, "group" | "topic_group");
    PcMsg::new_inbound("feishu", &chat_id, text, is_group).ok()
}
