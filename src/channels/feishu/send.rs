//! 飞书出站：flush、token 类型、event_body_to_pcmsg、连通性检查。Sink 统一为 dispatch::QueuedSink。

use crate::bus::PcMsg;
use crate::channels::send::{
    ensure_sender_http, record_outbound_http_failure, record_outbound_http_success,
    run_buffered_sender_loop,
};
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

/// Shared Feishu tenant token cache for sender and stream editor.
/// 飞书 tenant_access_token 共享缓存，统一 sender 与 stream editor 的 TTL / 失效语义。
#[derive(Default)]
pub struct FeishuTokenCache {
    token: Option<(String, std::time::Instant)>,
}

impl FeishuTokenCache {
    const TTL: std::time::Duration =
        std::time::Duration::from_secs(7200 - TOKEN_REFRESH_MARGIN_SECS);

    /// Creates an empty token cache.
    /// 创建空的飞书 token 缓存。
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns a usable tenant token, refreshing it when TTL is reached.
    /// 返回可用 tenant token；若达到 TTL 则自动刷新。
    pub fn ensure_token<H: ChannelHttpClient + ?Sized>(
        &mut self,
        http: &mut H,
        app_id: &str,
        app_secret: &str,
        stage: &'static str,
    ) -> crate::error::Result<String> {
        let need_refresh = match &self.token {
            Some((_, acquired_at)) => acquired_at.elapsed() >= Self::TTL,
            None => true,
        };
        if need_refresh {
            let token = acquire_tenant_token_with_stage(http, app_id, app_secret, stage)?;
            self.token = Some((token.clone(), std::time::Instant::now()));
            return Ok(token);
        }
        self.token
            .as_ref()
            .map(|(token, _)| token.clone())
            .ok_or_else(|| crate::error::Error::config(stage, "token missing after refresh"))
    }

    /// Drops the cached token so next use performs a refresh.
    /// 清空缓存 token，使下次使用时强制刷新。
    pub fn invalidate(&mut self) {
        self.token = None;
    }
}

pub fn acquire_tenant_token<H: ChannelHttpClient + ?Sized>(
    http: &mut H,
    app_id: &str,
    app_secret: &str,
) -> crate::error::Result<String> {
    acquire_tenant_token_with_stage(http, app_id, app_secret, "feishu_token")
}

fn acquire_tenant_token_with_stage<H: ChannelHttpClient + ?Sized>(
    http: &mut H,
    app_id: &str,
    app_secret: &str,
    stage: &'static str,
) -> crate::error::Result<String> {
    const TAG: &str = "feishu_send";
    let body = FeishuTokenRequest {
        app_id: app_id.to_string(),
        app_secret: app_secret.to_string(),
    };
    let body_bytes = serde_json::to_vec(&body).map_err(|e| crate::error::Error::Other {
        source: Box::new(e),
        stage,
    })?;
    let (status, resp_body) = match http.http_post(FEISHU_TOKEN_URL, &body_bytes) {
        Ok(r) => r,
        Err(e) => {
            let error = crate::error::Error::Other {
                source: Box::new(e),
                stage,
            };
            record_outbound_http_failure(&error);
            return Err(error);
        }
    };
    if status >= 400 {
        let error = crate::error::Error::Http {
            status_code: status,
            stage,
        };
        record_outbound_http_failure(&error);
        return Err(error);
    }
    record_outbound_http_success();
    let token_resp: FeishuTokenResponse =
        serde_json::from_slice(resp_body.as_ref()).map_err(|e| crate::error::Error::Other {
            source: Box::new(e),
            stage,
        })?;
    match token_resp.tenant_access_token {
        Some(t) if !t.is_empty() => Ok(t),
        _ => {
            log::warn!("[{}] token empty code={}", TAG, token_resp.code);
            Err(crate::error::Error::config(
                stage,
                "tenant_access_token missing",
            ))
        }
    }
}

fn build_feishu_text_body(receive_id: Option<&str>, content: &str) -> Vec<u8> {
    let mut inner = String::with_capacity(content.len() + 16);
    inner.push('{');
    inner.push_str("\"text\":");
    crate::util::push_json_string_escaped(&mut inner, content);
    inner.push('}');

    let mut body = String::with_capacity(inner.len() + receive_id.map_or(32, |id| id.len() + 32));
    body.push('{');
    if let Some(chat_id) = receive_id {
        body.push_str("\"receive_id\":");
        crate::util::push_json_string_escaped(&mut body, chat_id);
        body.push(',');
    }
    body.push_str("\"msg_type\":\"text\",\"content\":");
    crate::util::push_json_string_escaped(&mut body, &inner);
    body.push('}');
    body.into_bytes()
}

fn send_feishu_message<H: ChannelHttpClient>(
    http: &mut H,
    token: &str,
    chat_id: &str,
    content: &str,
) -> crate::error::Result<()> {
    const TAG: &str = "feishu_send";
    if content.trim().is_empty() {
        return Err(crate::error::Error::config(
            "feishu_send",
            "refusing to send empty Feishu message",
        ));
    }
    let auth_val = format!("Bearer {}", token);
    for chunk in crate::channels::chunk::chunk_text_by_char_count(content, FEISHU_MAX_MESSAGE_LEN) {
        let body_bytes = build_feishu_text_body(Some(chat_id), &chunk);
        let headers = [
            ("Authorization", auth_val.as_str()),
            ("Content-Type", "application/json; charset=utf-8"),
        ];
        let (status, _) = crate::channels::send::send_post_with_headers(
            TAG,
            http,
            FEISHU_SEND_URL,
            &headers,
            &body_bytes,
        )
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
    }
    Ok(())
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
    let token =
        match acquire_tenant_token_with_stage(http, app_id, app_secret, "feishu_flush_token") {
            Ok(t) => t,
            Err(error) => {
                log::warn!("[feishu_flush] acquire token failed: {}", error);
                return;
            }
        };
    while let Ok((chat_id, content, _req_id)) = rx.try_recv() {
        if let Err(error) = send_feishu_message(http, &token, &chat_id, &content) {
            record_outbound_http_failure(&error);
            log::warn!(
                "[feishu_flush] send failed for chat_id={}: {}",
                chat_id,
                error
            );
        } else {
            record_outbound_http_success();
        }
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
    let mut http: Option<H> = None;
    let mut token_cache = FeishuTokenCache::new();
    run_buffered_sender_loop(rx, TAG, |message, attempt| {
        if !ensure_sender_http(&mut http, &mut create_http, TAG, attempt) {
            return Err(crate::error::Error::config(TAG, "create http failed"));
        }
        let Some(h) = http.as_mut() else {
            return Err(crate::error::Error::config(
                TAG,
                "sender http missing after ensure",
            ));
        };
        let token = match token_cache.ensure_token(h, app_id, app_secret, TAG) {
            Ok(token) => token,
            Err(error) => {
                log::warn!(
                    "[{}] acquire token failed (attempt {}): {}",
                    TAG,
                    attempt,
                    error
                );
                token_cache.invalidate();
                http = None;
                return Err(error);
            }
        };
        match send_feishu_message(h, token.as_str(), &message.0, &message.1) {
            Ok(()) => {
                record_outbound_http_success();
                Ok(())
            }
            Err(error) => {
                record_outbound_http_failure(&error);
                log::warn!(
                    "[{}] send failed (attempt {}), chat_id={}: {}",
                    TAG,
                    attempt,
                    message.0,
                    error
                );
                token_cache.invalidate();
                http = None;
                Err(error)
            }
        }
    });
}

/// 发送消息并返回平台侧 message_id（字符串形式）；供流式编辑使用。
/// 需先调用 acquire_tenant_token 获取 token。
pub fn send_and_get_id<H: ChannelHttpClient>(
    http: &mut H,
    token: &str,
    chat_id: &str,
    content: &str,
) -> crate::error::Result<Option<String>> {
    let body_bytes = build_feishu_text_body(Some(chat_id), content);
    let auth_val = format!("Bearer {}", token);
    let headers = [
        ("Authorization", auth_val.as_str()),
        ("Content-Type", "application/json; charset=utf-8"),
    ];
    let (status, resp_body) =
        match http.http_post_with_headers(FEISHU_SEND_URL, &headers, &body_bytes) {
            Ok(resp) => resp,
            Err(e) => {
                let error = crate::error::Error::Other {
                    source: Box::new(e),
                    stage: "feishu_send",
                };
                record_outbound_http_failure(&error);
                return Err(error);
            }
        };
    if status >= 400 {
        let error = crate::error::Error::Http {
            status_code: status,
            stage: "feishu_send",
        };
        record_outbound_http_failure(&error);
        return Err(error);
    }
    record_outbound_http_success();
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
    let body_bytes = build_feishu_text_body(None, content);
    let url = format!(
        "https://open.feishu.cn/open-apis/im/v1/messages/{}",
        message_id
    );
    let auth_val = format!("Bearer {}", token);
    let headers = [
        ("Authorization", auth_val.as_str()),
        ("Content-Type", "application/json; charset=utf-8"),
    ];
    let (status, _) = match http.http_patch_with_headers(&url, &headers, &body_bytes) {
        Ok(resp) => resp,
        Err(e) => {
            let error = crate::error::Error::Other {
                source: Box::new(e),
                stage: "feishu_edit",
            };
            record_outbound_http_failure(&error);
            return Err(error);
        }
    };
    if status >= 400 {
        let error = crate::error::Error::Http {
            status_code: status,
            stage: "feishu_edit",
        };
        record_outbound_http_failure(&error);
        return Err(error);
    }
    record_outbound_http_success();
    Ok(())
}

/// 连通性检查：供 GET /api/channel_connectivity 使用。
pub fn check_connectivity<H: ChannelHttpClient + ?Sized>(
    config: &AppConfig,
    http: &mut H,
    loc: crate::i18n::Locale,
) -> super::super::connectivity::ChannelConnectivityItem {
    use super::super::connectivity;
    let configured =
        !config.feishu_app_id.trim().is_empty() && !config.feishu_app_secret.trim().is_empty();
    connectivity::probe_item("feishu", configured, loc, || {
        let body = FeishuTokenRequest {
            app_id: config.feishu_app_id.trim().to_string(),
            app_secret: config.feishu_app_secret.trim().to_string(),
        };
        let body_bytes = match serde_json::to_vec(&body) {
            Ok(b) => b,
            Err(e) => {
                log::warn!("[feishu_connectivity] json: {}", e);
                return connectivity::ProbeStatus::CheckFailed;
            }
        };
        let (status, resp_body) = match http.http_post(FEISHU_TOKEN_URL, &body_bytes) {
            Ok(r) => r,
            Err(e) => {
                log::warn!("[feishu_connectivity] post: {}", e);
                return connectivity::ProbeStatus::CheckFailed;
            }
        };
        if status >= 400 {
            log::warn!("[feishu_connectivity] token api status {}", status);
            return connectivity::ProbeStatus::InvalidToken;
        }
        let r: FeishuTokenResponse = match serde_json::from_slice(resp_body.as_ref()) {
            Ok(x) => x,
            Err(e) => {
                log::warn!("[feishu_connectivity] parse: {}", e);
                return connectivity::ProbeStatus::CheckFailed;
            }
        };
        match r.tenant_access_token {
            Some(t) if !t.is_empty() => connectivity::ProbeStatus::Ok,
            _ => {
                log::warn!("[feishu_connectivity] no token code={}", r.code);
                connectivity::ProbeStatus::InvalidToken
            }
        }
    })
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::ResponseBody;
    use std::collections::VecDeque;

    #[derive(Default)]
    struct StubHttp {
        post_results: VecDeque<crate::error::Result<(u16, ResponseBody)>>,
        patch_results: VecDeque<crate::error::Result<(u16, ResponseBody)>>,
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
            self.post_results
                .pop_front()
                .unwrap_or_else(|| Ok((200, ResponseBody::Heap(b"{}".to_vec()))))
        }

        fn http_post_with_headers(
            &mut self,
            _url: &str,
            _headers: &[(&str, &str)],
            _body: &[u8],
        ) -> crate::error::Result<(u16, ResponseBody)> {
            self.http_post("", &[])
        }

        fn http_patch_with_headers(
            &mut self,
            _url: &str,
            _headers: &[(&str, &str)],
            _body: &[u8],
        ) -> crate::error::Result<(u16, ResponseBody)> {
            self.patch_results
                .pop_front()
                .unwrap_or_else(|| Ok((200, ResponseBody::Heap(b"{}".to_vec()))))
        }
    }

    #[test]
    fn ensure_token_preserves_transport_error() {
        let mut cache = FeishuTokenCache::new();
        let mut http = StubHttp {
            post_results: VecDeque::from([Err(crate::error::Error::config(
                "tls_admission",
                "permit timeout",
            ))]),
            ..Default::default()
        };

        let err = cache
            .ensure_token(&mut http, "app", "secret", "feishu_stream")
            .expect_err("token refresh should fail");

        assert!(err.is_tls_admission());
    }

    #[test]
    fn send_and_get_id_records_outbound_http_success() {
        crate::orchestrator::reset_runtime_capabilities_for_tests();
        let before = crate::metrics::snapshot();
        let mut http = StubHttp {
            post_results: VecDeque::from([Ok((
                200,
                ResponseBody::Heap(br#"{"data":{"message_id":"om_123"}}"#.to_vec()),
            ))]),
            ..Default::default()
        };

        let message_id = send_and_get_id(&mut http, "token", "chat-1", "hello").expect("send");

        let after = crate::metrics::snapshot();
        assert_eq!(message_id.as_deref(), Some("om_123"));
        assert!(after.channel_http_ok >= before.channel_http_ok + 1);
    }
}
