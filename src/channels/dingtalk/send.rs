//! 钉钉通道：出站经 MessageSink 队列，由 main 用 HTTP 向 Webhook 发送；入站无。
//! 仅支持自定义机器人 Webhook（不加签）；单条按 4096 字符分片。Sink 统一为 dispatch::QueuedSink。

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

fn send_one_dingtalk<H: ChannelHttpClient>(http: &mut H, webhook_url: &str, content: &str) {
    const TAG: &str = "dingtalk_send";
    let chunks =
        crate::channels::chunk::chunk_text_by_char_count(content, DINGTALK_MAX_MESSAGE_LEN);
    for chunk in chunks {
        let body = serde_json::json!({
            "msgtype": "text",
            "text": { "content": chunk }
        });
        let body_bytes = match serde_json::to_vec(&body) {
            Ok(b) => b,
            Err(e) => {
                log::warn!("[{}] send json: {}", TAG, e);
                continue;
            }
        };
        let _ = crate::channels::send::send_post(TAG, http, webhook_url, &body_bytes);
    }
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
        send_one_dingtalk(http, webhook_url, &content);
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
    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
    crate::platform::task_wdt::register_current_task_to_task_wdt();
    log::info!("[{}] sender loop started", TAG);

    let mut http: Option<H> = None;
    let recv_timeout = std::time::Duration::from_secs(30);
    loop {
        let (_chat_id, content, req_id) = match rx.recv_timeout(recv_timeout) {
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
            send_one_dingtalk(h, webhook_url, &content);
            while let Ok((_, cnt, _)) = rx.try_recv() {
                send_one_dingtalk(h, webhook_url, &cnt);
            }
            sent = true;
            break;
        }
        if !sent {
            log::error!(
                "[{}] req_id={} message dropped after 3 retries",
                TAG,
                req_id.as_deref().unwrap_or("-")
            );
        }
    }
}
