//! 钉钉通道：出站经 MessageSink 队列，由 main 用 HTTP 向 Webhook 发送；入站无。
//! 仅支持自定义机器人 Webhook（不加签）；单条按 4096 字符分片。Sink 统一为 dispatch::QueuedSink。

use crate::channels::send::{
    ensure_sender_http, record_outbound_http_failure, record_outbound_http_success,
    run_buffered_sender_loop,
};
use crate::channels::ChannelHttpClient;
use crate::config::AppConfig;

/// 单条消息最大字符数，与飞书/Telegram 对齐。
const DINGTALK_MAX_MESSAGE_LEN: usize = 4096;

const CONNECTIVITY_MESSAGE: &str = "BOT, Hello";

/// 连通性检查：供 GET /api/channel_connectivity 使用。
pub fn check_connectivity<H: ChannelHttpClient + ?Sized>(
    config: &AppConfig,
    http: &mut H,
    loc: crate::i18n::Locale,
) -> super::super::connectivity::ChannelConnectivityItem {
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
        let (status, _) = match http.http_post(config.dingtalk_webhook_url.trim(), &body_bytes) {
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
    webhook_url: &str,
    content: &str,
) -> crate::error::Result<()> {
    const TAG: &str = "dingtalk_send";
    if content.trim().is_empty() {
        return Err(crate::error::Error::config(
            "dingtalk_send",
            "refusing to send empty DingTalk message",
        ));
    }
    let chunks =
        crate::channels::chunk::chunk_text_by_char_count(content, DINGTALK_MAX_MESSAGE_LEN);
    for chunk in chunks {
        let body = serde_json::json!({
            "msgtype": "text",
            "text": { "content": chunk }
        });
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
    rx: &std::sync::mpsc::Receiver<(String, String, Option<String>)>,
    webhook_url: &str,
    http: &mut H,
) {
    if webhook_url.is_empty() {
        return;
    }
    while let Ok((_chat_id, content, _req_id)) = rx.try_recv() {
        if let Err(error) = send_one_dingtalk(http, webhook_url, &content) {
            record_outbound_http_failure(&error);
            log::warn!("[dingtalk_flush] send failed: {}", error);
        } else {
            record_outbound_http_success();
        }
    }
}

/// 持续运行的钉钉发送循环：sender 线程内**复用**同一 HTTP 客户端，减轻 lwIP socket / TLS 压力。
pub fn run_dingtalk_sender_loop<H, F>(
    rx: std::sync::mpsc::Receiver<(String, String, Option<String>)>,
    webhook_url: &str,
    mut create_http: F,
) where
    H: ChannelHttpClient,
    F: FnMut() -> crate::error::Result<H>,
{
    const TAG: &str = "dingtalk_sender";
    if webhook_url.is_empty() {
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
        match send_one_dingtalk(h, webhook_url, &message.1) {
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
