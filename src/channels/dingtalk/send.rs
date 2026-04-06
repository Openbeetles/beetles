//! 钉钉通道：出站经 MessageSink 队列，由 main 用 HTTP 向 Webhook 发送；入站无。
//! 仅支持自定义机器人 Webhook（不加签）；单条按 4096 字符分片。Sink 统一为 dispatch::QueuedSink。

use crate::channels::send::{
    ensure_sender_http, feed_sender_loop_wdt, log_sender_drop, recv_sender_loop_event,
    sleep_sender_retry_delay, start_sender_loop, SenderLoopEvent, CHANNEL_SENDER_MAX_RETRIES,
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
    use super::super::connectivity;
    use crate::i18n::{tr, Message};
    let configured = !config.dingtalk_webhook_url.trim().is_empty();
    if !configured {
        return connectivity::item(
            "dingtalk",
            false,
            false,
            Some(tr(Message::ConnectivityNotConfigured, loc)),
        );
    }
    let body = serde_json::json!({
        "msgtype": "text",
        "text": { "content": CONNECTIVITY_MESSAGE }
    });
    let body_bytes = match serde_json::to_vec(&body) {
        Ok(b) => b,
        Err(e) => {
            log::warn!("[dingtalk_connectivity] json: {}", e);
            return connectivity::item(
                "dingtalk",
                configured,
                false,
                Some(tr(Message::ConnectivityCheckFailed, loc)),
            );
        }
    };
    let (status, _) = match http.http_post(config.dingtalk_webhook_url.trim(), &body_bytes) {
        Ok(r) => r,
        Err(e) => {
            log::warn!("[dingtalk_connectivity] post: {}", e);
            return connectivity::item(
                "dingtalk",
                configured,
                false,
                Some(tr(Message::ConnectivityCheckFailed, loc)),
            );
        }
    };
    if (200..300).contains(&status) {
        connectivity::item("dingtalk", configured, true, None)
    } else {
        log::warn!("[dingtalk_connectivity] webhook status {}", status);
        connectivity::item(
            "dingtalk",
            configured,
            false,
            Some(tr(Message::ConnectivityCheckFailed, loc)),
        )
    }
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
            log::warn!("[dingtalk_flush] send failed: {}", error);
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
    start_sender_loop(TAG);

    let mut http: Option<H> = None;
    loop {
        let (_chat_id, content, req_id) = match recv_sender_loop_event(&rx, TAG) {
            SenderLoopEvent::Message(item) => item,
            SenderLoopEvent::Timeout => continue,
            SenderLoopEvent::Disconnected => break,
        };
        feed_sender_loop_wdt();
        let mut sent = false;
        for retry in 0..CHANNEL_SENDER_MAX_RETRIES {
            if retry > 0 {
                sleep_sender_retry_delay();
            }
            if !ensure_sender_http(&mut http, &mut create_http, TAG, retry + 1) {
                continue;
            }
            let Some(h) = http.as_mut() else {
                continue;
            };
            match send_one_dingtalk(h, webhook_url, &content) {
                Ok(()) => crate::metrics::record_channel_http_result(true),
                Err(error) => {
                    crate::metrics::record_channel_http_result(false);
                    log::warn!("[{}] send failed (attempt {}): {}", TAG, retry + 1, error);
                    http = None;
                    continue;
                }
            }
            while let Ok((_, cnt, _)) = rx.try_recv() {
                if let Err(error) = send_one_dingtalk(h, webhook_url, &cnt) {
                    crate::metrics::record_channel_http_result(false);
                    log::warn!("[{}] drain send failed: {}", TAG, error);
                    break;
                }
                crate::metrics::record_channel_http_result(true);
            }
            sent = true;
            break;
        }
        if !sent {
            log_sender_drop(TAG, req_id.as_deref(), None, CHANNEL_SENDER_MAX_RETRIES);
        }
    }
}
