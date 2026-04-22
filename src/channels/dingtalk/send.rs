//! 钉钉通道：出站优先走应用机器人 sessionWebhook，会话外再回退到自定义机器人 Webhook。
//! 单条按 4096 字符分片。Sink 统一为 dispatch::QueuedSink。

use crate::bus::{CanonicalMessageBody, CardBody, TextBody, TextFormat};
use crate::channels::send::{
    ensure_sender_http, record_outbound_http_failure, record_outbound_http_success,
    run_buffered_sender_loop, QueuedOutboundMessage,
};
use crate::channels::ChannelHttpClient;
use crate::config::AppConfig;

use base64::Engine as _;
use hmac::{Hmac, Mac};
use sha2::Sha256;

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
use crate::channels::dingtalk::active_session_webhook;

/// 单条消息最大字符数，与飞书/Telegram 对齐。
const DINGTALK_MAX_MESSAGE_LEN: usize = 4096;

const CONNECTIVITY_MESSAGE: &str = "BOT, Hello";

type HmacSha256 = Hmac<Sha256>;

#[derive(Clone, Debug, PartialEq, Eq)]
enum DingtalkWebhookTarget {
    SessionWebhook(String),
    CustomWebhook(String),
}

fn signed_custom_webhook_url(webhook_url: &str, secret: &str) -> crate::error::Result<String> {
    let webhook_url = webhook_url.trim();
    let secret = secret.trim();
    if webhook_url.is_empty() || secret.is_empty() {
        return Ok(webhook_url.to_string());
    }
    let timestamp_ms = crate::util::current_unix_secs().saturating_mul(1000);
    let string_to_sign = format!("{timestamp_ms}\n{secret}");
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).map_err(|e| {
        crate::error::Error::config("dingtalk_sign", format!("invalid secret: {}", e))
    })?;
    mac.update(string_to_sign.as_bytes());
    let sign = base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes());
    let sep = if webhook_url.contains('?') { '&' } else { '?' };
    Ok(format!(
        "{webhook_url}{sep}timestamp={timestamp_ms}&sign={}",
        urlencoding::encode(&sign)
    ))
}

fn resolve_target_webhook(
    chat_id: &str,
    default_webhook_url: &str,
    app_secret: &str,
    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
    session_store: &super::DingtalkSessionStore,
) -> crate::error::Result<DingtalkWebhookTarget> {
    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
    if let Some(webhook_url) = active_session_webhook(session_store, chat_id)? {
        return Ok(DingtalkWebhookTarget::SessionWebhook(webhook_url));
    }

    if default_webhook_url.trim().is_empty() {
        return Err(crate::error::Error::config(
            "dingtalk_send",
            "no active sessionWebhook and dingtalk_webhook_url is empty",
        ));
    }
    Ok(DingtalkWebhookTarget::CustomWebhook(
        signed_custom_webhook_url(default_webhook_url, app_secret)?,
    ))
}

fn markdown_title(markdown: &str) -> String {
    let first_nonempty = markdown
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("beetle");
    let mut title = String::new();
    for ch in first_nonempty.chars().take(64) {
        title.push(ch);
    }
    if title.is_empty() {
        "beetle".to_string()
    } else {
        title
    }
}

fn render_text_payloads(text: &TextBody) -> crate::error::Result<Vec<serde_json::Value>> {
    let content = text.text.trim();
    if content.is_empty() {
        return Err(crate::error::Error::config(
            "dingtalk_send",
            "refusing to send empty DingTalk text body",
        ));
    }
    match text.format {
        TextFormat::Plain => Ok(crate::channels::chunk::chunk_text_by_char_count(
            content,
            DINGTALK_MAX_MESSAGE_LEN,
        )
        .into_iter()
        .map(|chunk| {
            serde_json::json!({
                "msgtype": "text",
                "text": { "content": chunk }
            })
        })
        .collect()),
        TextFormat::Markdown => Ok(crate::channels::chunk::chunk_text_by_char_count(
            content,
            DINGTALK_MAX_MESSAGE_LEN,
        )
        .into_iter()
        .map(|chunk| {
            serde_json::json!({
                "msgtype": "markdown",
                "markdown": {
                    "title": markdown_title(&chunk),
                    "text": chunk,
                }
            })
        })
        .collect()),
        TextFormat::Html => Err(crate::error::Error::config(
            "dingtalk_send",
            "DingTalk does not support HTML text bodies",
        )),
        TextFormat::RichText => Err(crate::error::Error::config(
            "dingtalk_send",
            "DingTalk rich_text body requires a CardBody payload, not TextBody::RichText",
        )),
    }
}

fn render_card_payload(
    card: &CardBody,
    target: &DingtalkWebhookTarget,
) -> crate::error::Result<serde_json::Value> {
    let payload = card.payload_json.as_object().ok_or_else(|| {
        crate::error::Error::config(
            "dingtalk_send",
            "DingTalk CardBody payload_json must be a JSON object",
        )
    })?;
    if let Some(msg_key) = payload.get("msgKey").and_then(serde_json::Value::as_str) {
        if !matches!(target, DingtalkWebhookTarget::SessionWebhook(_)) {
            return Err(crate::error::Error::config(
                "dingtalk_send",
                "msgKey/msgParam cards require an active DingTalk sessionWebhook target",
            ));
        }
        let msg_param = match payload.get("msgParam") {
            Some(serde_json::Value::String(value)) => value.trim().to_string(),
            Some(value) if !value.is_null() => serde_json::to_string(value)
                .map_err(|e| crate::error::Error::config("dingtalk_send", e.to_string()))?,
            _ => String::new(),
        };
        if msg_key.trim().is_empty() || msg_param.trim().is_empty() {
            return Err(crate::error::Error::config(
                "dingtalk_send",
                "msgKey/msgParam card payload must not be empty",
            ));
        }
        return Ok(serde_json::json!({
            "msgKey": msg_key.trim(),
            "msgParam": msg_param,
        }));
    }
    let Some(msg_type) = payload.get("msgtype").and_then(serde_json::Value::as_str) else {
        return Err(crate::error::Error::config(
            "dingtalk_send",
            "DingTalk card payload must contain msgtype or msgKey/msgParam",
        ));
    };
    let supported = match msg_type {
        "link" => payload
            .get("link")
            .is_some_and(serde_json::Value::is_object),
        "actionCard" => payload
            .get("actionCard")
            .is_some_and(serde_json::Value::is_object),
        "feedCard" => payload
            .get("feedCard")
            .is_some_and(serde_json::Value::is_object),
        _ => false,
    };
    if supported {
        Ok(card.payload_json.clone())
    } else {
        Err(crate::error::Error::config(
            "dingtalk_send",
            format!("unsupported DingTalk card msgtype={msg_type}"),
        ))
    }
}

fn render_dingtalk_payloads(
    message: &QueuedOutboundMessage,
    target: &DingtalkWebhookTarget,
) -> crate::error::Result<Vec<serde_json::Value>> {
    match &message.body {
        CanonicalMessageBody::Text(text) => {
            let mut normalized = text.clone();
            if normalized.text.trim().is_empty() {
                normalized.text = message.content.clone();
            }
            render_text_payloads(&normalized)
        }
        CanonicalMessageBody::Card(card) => Ok(vec![render_card_payload(card, target)?]),
        CanonicalMessageBody::Image(_)
        | CanonicalMessageBody::Audio(_)
        | CanonicalMessageBody::Video(_)
        | CanonicalMessageBody::File(_) => Err(crate::error::Error::config(
            "dingtalk_send",
            format!(
                "DingTalk {:?} outbound is not enabled in beetle without a verified native payload contract",
                message.body.kind()
            ),
        )),
        CanonicalMessageBody::PlatformNative(native) => {
            if native.payload_json.is_object() {
                Ok(vec![native.payload_json.clone()])
            } else {
                Err(crate::error::Error::config(
                    "dingtalk_send",
                    "DingTalk PlatformNativeBody payload_json must be a JSON object",
                ))
            }
        }
    }
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
fn sender_has_any_target(webhook_url: &str) -> bool {
    !webhook_url.is_empty()
}

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
fn sender_has_any_target(webhook_url: &str, session_store: &super::DingtalkSessionStore) -> bool {
    if !webhook_url.is_empty() {
        return true;
    }
    session_store
        .lock()
        .map(|guard| !guard.is_empty())
        .unwrap_or(false)
}

/// 连通性检查：供 GET /api/channel_connectivity 使用。
pub fn check_connectivity<H: ChannelHttpClient + ?Sized>(
    config: &AppConfig,
    http: &mut H,
) -> super::super::connectivity::ChannelConnectivityItem {
    if config.dingtalk_webhook_url.trim().is_empty() && config.enabled_channel == "dingtalk" {
        return crate::channels::connectivity::item(
            "dingtalk",
            true,
            true,
            Some(crate::channels::connectivity::CONNECTIVITY_SESSION_REPLY_ONLY_KEY),
        );
    }
    let configured = !config.dingtalk_webhook_url.trim().is_empty();
    crate::channels::connectivity::probe_item("dingtalk", configured, || {
        let body = serde_json::json!({
            "msgtype": "text",
            "text": { "content": CONNECTIVITY_MESSAGE }
        });
        let body_bytes = match serde_json::to_vec(&body) {
            Ok(b) => b,
            Err(e) => {
                log::warn!("[dingtalk_connectivity] json: {}", e);
                return crate::channels::connectivity::ProbeStatus::CheckFailed;
            }
        };
        let signed_url = match signed_custom_webhook_url(
            config.dingtalk_webhook_url.trim(),
            &config.dingtalk_app_secret,
        ) {
            Ok(url) => url,
            Err(e) => {
                log::warn!("[dingtalk_connectivity] sign: {}", e);
                return crate::channels::connectivity::ProbeStatus::CheckFailed;
            }
        };
        let (status, _) = match http.http_post(&signed_url, &body_bytes) {
            Ok(r) => r,
            Err(e) => {
                log::warn!("[dingtalk_connectivity] post: {}", e);
                return crate::channels::connectivity::ProbeStatus::CheckFailed;
            }
        };
        if (200..300).contains(&status) {
            crate::channels::connectivity::ProbeStatus::Ok
        } else {
            log::warn!("[dingtalk_connectivity] webhook status {}", status);
            crate::channels::connectivity::ProbeStatus::CheckFailed
        }
    })
}

fn send_one_dingtalk<H: ChannelHttpClient>(
    http: &mut H,
    message: &QueuedOutboundMessage,
    default_webhook_url: &str,
    app_secret: &str,
    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
    session_store: &super::DingtalkSessionStore,
) -> crate::error::Result<()> {
    const TAG: &str = "dingtalk_send";
    let target = resolve_target_webhook(
        &message.chat_id,
        default_webhook_url,
        app_secret,
        #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
        session_store,
    )?;
    let webhook_url = match &target {
        DingtalkWebhookTarget::SessionWebhook(url) | DingtalkWebhookTarget::CustomWebhook(url) => {
            url.as_str()
        }
    };
    for body in render_dingtalk_payloads(message, &target)? {
        let body_bytes = serde_json::to_vec(&body)
            .map_err(|e| crate::error::Error::config("dingtalk_send", e.to_string()))?;
        let (status, _) = crate::channels::send::send_post(TAG, http, webhook_url, &body_bytes)?;
        if status >= 400 {
            return Err(crate::error::Error::http("dingtalk_send", status));
        }
    }
    Ok(())
}

/// 从 rx 取出待发送（一次性 drain）。
pub fn flush_dingtalk_sends<H: ChannelHttpClient>(
    rx: &std::sync::mpsc::Receiver<QueuedOutboundMessage>,
    webhook_url: &str,
    app_secret: &str,
    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
    session_store: &super::DingtalkSessionStore,
    http: &mut H,
) {
    if !sender_has_any_target(
        webhook_url,
        #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
        session_store,
    ) {
        return;
    }
    while let Ok(message) = rx.try_recv() {
        if let Err(error) = send_one_dingtalk(
            http,
            &message,
            webhook_url,
            app_secret,
            #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
            session_store,
        ) {
            record_outbound_http_failure(&error);
            log::warn!("[dingtalk_flush] send failed: {}", error);
        } else {
            record_outbound_http_success();
        }
    }
}

/// 持续运行的钉钉发送循环：sender 线程内**复用**同一 HTTP 客户端，减轻 lwIP socket / TLS 压力。
pub fn run_dingtalk_sender_loop<H, F>(
    rx: std::sync::mpsc::Receiver<QueuedOutboundMessage>,
    webhook_url: &str,
    app_secret: &str,
    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
    session_store: &super::DingtalkSessionStore,
    mut create_http: F,
) where
    H: ChannelHttpClient,
    F: FnMut() -> crate::error::Result<H>,
{
    const TAG: &str = "dingtalk_sender";
    if !sender_has_any_target(
        webhook_url,
        #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
        session_store,
    ) {
        return;
    }
    let mut http: Option<H> = None;
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
        match send_one_dingtalk(
            h,
            message,
            webhook_url,
            app_secret,
            #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
            session_store,
        ) {
            Ok(()) => {
                record_outbound_http_success();
                Ok(())
            }
            Err(error) => {
                record_outbound_http_failure(&error);
                log::warn!("[{}] send failed (attempt {}): {}", TAG, attempt, error);
                http = None;
                Err(error)
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::{render_dingtalk_payloads, send_one_dingtalk, DingtalkWebhookTarget};
    use crate::bus::{CanonicalMessageBody, CardBody, TextBody, TextFormat};
    use crate::channels::send::QueuedOutboundMessage;
    use crate::channels::ChannelHttpClient;
    use crate::platform::ResponseBody;

    #[derive(Default)]
    struct FakeHttp {
        posts: Vec<(String, Vec<u8>)>,
    }

    impl ChannelHttpClient for FakeHttp {
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
            url: &str,
            body: &[u8],
        ) -> crate::error::Result<(u16, ResponseBody)> {
            self.posts.push((url.to_string(), body.to_vec()));
            Ok((200, ResponseBody::Heap(b"{}".to_vec())))
        }

        fn http_post_with_headers(
            &mut self,
            url: &str,
            _headers: &[(&str, &str)],
            body: &[u8],
        ) -> crate::error::Result<(u16, ResponseBody)> {
            self.http_post(url, body)
        }
    }

    fn queued_message(body: CanonicalMessageBody, content: &str) -> QueuedOutboundMessage {
        QueuedOutboundMessage {
            transport_send_id: 1,
            chat_id: "chat-1".to_string(),
            content: content.to_string(),
            body,
            platform_thread_id: String::new(),
            req_id: Some("req-1".to_string()),
            outbound_kind: crate::bus::OutboundKind::Primary,
        }
    }

    #[test]
    fn markdown_text_body_renders_markdown_payload() {
        let message = queued_message(
            CanonicalMessageBody::Text(TextBody {
                text: "# Title\nbody".to_string(),
                format: TextFormat::Markdown,
            }),
            "",
        );
        let payloads = render_dingtalk_payloads(
            &message,
            &DingtalkWebhookTarget::SessionWebhook("https://example.invalid".to_string()),
        )
        .expect("payloads");
        assert_eq!(payloads.len(), 1);
        assert_eq!(payloads[0]["msgtype"], "markdown");
        assert_eq!(payloads[0]["markdown"]["text"], "# Title\nbody");
    }

    #[test]
    fn card_body_pass_through_posts_action_card_payload() {
        let message = queued_message(
            CanonicalMessageBody::Card(CardBody {
                format: crate::bus::CardFormat::Interactive,
                payload_json: serde_json::json!({
                    "msgtype": "actionCard",
                    "actionCard": {
                        "title": "Alert",
                        "text": "content",
                        "singleTitle": "Open",
                        "singleURL": "https://example.invalid"
                    }
                }),
                fallback_text: "Alert".to_string(),
            }),
            "Alert",
        );
        let mut http = FakeHttp::default();

        send_one_dingtalk(
            &mut http,
            &message,
            "https://example.invalid/custom",
            "",
            #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
            &std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
        )
        .expect("send");

        assert_eq!(http.posts.len(), 1);
        let posted: serde_json::Value = serde_json::from_slice(&http.posts[0].1).expect("json");
        assert_eq!(posted["msgtype"], "actionCard");
        assert_eq!(posted["actionCard"]["title"], "Alert");
    }
}
