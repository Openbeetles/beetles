//! 钉钉通道：出站优先走应用机器人 sessionWebhook，会话外再回退到自定义机器人 Webhook。
//! 单条按 4096 字符分片。Sink 统一为 dispatch::QueuedSink。

use crate::channels::send::{
    ensure_sender_http, record_outbound_http_failure, record_outbound_http_success,
    run_buffered_sender_loop, QueuedOutboundMessage,
};
use crate::channels::ChannelHttpClient;
use crate::config::AppConfig;
use crate::i18n::{tr, Message};

use base64::Engine as _;
use hmac::{Hmac, Mac};
use sha2::Sha256;

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
use crate::channels::dingtalk::active_session_webhook;

/// 单条消息最大字符数，与飞书/Telegram 对齐。
const DINGTALK_MAX_MESSAGE_LEN: usize = 4096;

const CONNECTIVITY_MESSAGE: &str = "BOT, Hello";

type HmacSha256 = Hmac<Sha256>;

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
) -> crate::error::Result<String> {
    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
    if let Some(webhook_url) = active_session_webhook(session_store, chat_id)? {
        return Ok(webhook_url);
    }

    if default_webhook_url.trim().is_empty() {
        return Err(crate::error::Error::config(
            "dingtalk_send",
            "no active sessionWebhook and dingtalk_webhook_url is empty",
        ));
    }
    signed_custom_webhook_url(default_webhook_url, app_secret)
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
    loc: crate::i18n::Locale,
) -> super::super::connectivity::ChannelConnectivityItem {
    if config.dingtalk_webhook_url.trim().is_empty() && config.enabled_channel == "dingtalk" {
        return crate::channels::connectivity::item(
            "dingtalk",
            true,
            true,
            Some(tr(Message::ConnectivitySessionReplyOnly, loc)),
        );
    }
    let configured = !config.dingtalk_webhook_url.trim().is_empty();
    crate::channels::connectivity::probe_item("dingtalk", configured, loc, || {
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
    if message.content.trim().is_empty() {
        return Err(crate::error::Error::config(
            "dingtalk_send",
            "refusing to send empty DingTalk message",
        ));
    }
    let webhook_url = resolve_target_webhook(
        &message.chat_id,
        default_webhook_url,
        app_secret,
        #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
        session_store,
    )?;
    let chunks = crate::channels::chunk::chunk_text_by_char_count(
        &message.content,
        DINGTALK_MAX_MESSAGE_LEN,
    );
    for chunk in chunks {
        let body = serde_json::json!({
            "msgtype": "text",
            "text": { "content": chunk }
        });
        let body_bytes = serde_json::to_vec(&body)
            .map_err(|e| crate::error::Error::config("dingtalk_send", e.to_string()))?;
        let (status, _) = crate::channels::send::send_post(TAG, http, &webhook_url, &body_bytes)?;
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
