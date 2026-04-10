//! Telegram 出站：flush、send_chat_action、get_bot_username、set_message_reaction；连通性检查。Sink 统一为 dispatch::QueuedSink。
use crate::channels::ChannelHttpClient;
use crate::config::AppConfig;
use crate::error::{Error, Result};

use super::super::connectivity;
use super::super::send::{
    ensure_sender_http, feed_sender_loop_wdt, log_sender_drop, record_outbound_http_failure,
    record_outbound_http_success, recv_sender_loop_event, sleep_sender_retry_delay,
    start_sender_loop, SenderLoopEvent, CHANNEL_SENDER_MAX_RETRIES,
};

const TELEGRAM_API_BASE: &str = "https://api.telegram.org/bot";
const TELEGRAM_MAX_MESSAGE_LEN: usize = 4096;

/// 连通性检查：供 GET /api/channel_connectivity 使用。
pub fn check_connectivity<H: ChannelHttpClient + ?Sized>(
    config: &AppConfig,
    http: &mut H,
    loc: crate::i18n::Locale,
) -> super::super::connectivity::ChannelConnectivityItem {
    let configured = !config.tg_token.trim().is_empty();
    connectivity::probe_item("telegram", configured, loc, || {
        match get_bot_username(http, config.tg_token.trim()) {
            Ok(Some(_)) => connectivity::ProbeStatus::Ok,
            Ok(None) => connectivity::ProbeStatus::InvalidToken,
            Err(e) => {
                log::warn!("[telegram_connectivity] getMe: {}", e);
                connectivity::ProbeStatus::CheckFailed
            }
        }
    })
}

fn send_one_telegram<H: ChannelHttpClient>(
    http: &mut H,
    token: &str,
    chat_id: &str,
    content: &str,
) -> Result<()> {
    const TAG: &str = "telegram_send";
    if content.trim().is_empty() {
        return Err(Error::config(
            "telegram_send",
            "refusing to send empty Telegram message",
        ));
    }
    let url = format!("{}{}/sendMessage", TELEGRAM_API_BASE, token);
    let mut reply_to_message_id: Option<i64> = None;
    for chunk in crate::channels::chunk::chunk_text_by_char_count(content, TELEGRAM_MAX_MESSAGE_LEN)
    {
        let mut body = serde_json::json!({
            "chat_id": chat_id,
            "text": chunk,
        });
        if let Some(id) = reply_to_message_id {
            body["reply_to_message_id"] = serde_json::json!(id);
        }
        let body_bytes =
            serde_json::to_vec(&body).map_err(|e| Error::config("telegram_send", e.to_string()))?;
        let (status, resp_body) = crate::channels::send::send_post(TAG, http, &url, &body_bytes)
            .map_err(|e| map_stage(e, "telegram_send"))?;
        if status >= 400 {
            return Err(Error::Http {
                status_code: status,
                stage: "telegram_send",
            });
        }
        #[derive(serde::Deserialize)]
        struct SendMessageResult {
            result: Option<SendMessageResultInner>,
        }
        #[derive(serde::Deserialize)]
        struct SendMessageResultInner {
            message_id: Option<i64>,
        }
        if let Ok(r) = serde_json::from_slice::<SendMessageResult>(resp_body.as_ref()) {
            if let Some(inner) = r.result {
                reply_to_message_id = inner.message_id;
            }
        }
    }
    Ok(())
}

/// 从 rx 取出所有待发送（一次性 drain）。
pub fn flush_telegram_sends<H: ChannelHttpClient>(
    rx: &std::sync::mpsc::Receiver<(String, String, Option<String>)>,
    token: &str,
    http: &mut H,
) {
    while let Ok((chat_id, content, _req_id)) = rx.try_recv() {
        if let Err(error) = send_one_telegram(http, token, &chat_id, &content) {
            record_outbound_http_failure(&error);
            log::warn!(
                "[telegram_flush] send failed for chat_id={}: {}",
                chat_id,
                error
            );
        } else {
            record_outbound_http_success();
        }
    }
}

/// 持续运行的 Telegram 发送循环：sender 线程内**复用**同一 HTTP 客户端，减轻 lwIP socket / TLS 压力。
pub fn run_telegram_sender_loop<H, F>(
    rx: std::sync::mpsc::Receiver<(String, String, Option<String>)>,
    token: &str,
    mut create_http: F,
) where
    H: ChannelHttpClient,
    F: FnMut() -> crate::error::Result<H>,
{
    const TAG: &str = "telegram_sender";
    start_sender_loop(TAG);

    let mut http: Option<H> = None;
    loop {
        let (chat_id, content, req_id) = match recv_sender_loop_event(&rx, TAG) {
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
            match send_one_telegram(h, token, &chat_id, &content) {
                Ok(()) => record_outbound_http_success(),
                Err(error) => {
                    record_outbound_http_failure(&error);
                    log::warn!(
                        "[{}] send failed (attempt {}), chat_id={}: {}",
                        TAG,
                        retry + 1,
                        chat_id,
                        error
                    );
                    http = None;
                    continue;
                }
            }
            while let Ok((cid, cnt, _)) = rx.try_recv() {
                if let Err(error) = send_one_telegram(h, token, &cid, &cnt) {
                    record_outbound_http_failure(&error);
                    log::warn!("[{}] drain send failed for chat_id={}: {}", TAG, cid, error);
                    break;
                }
                record_outbound_http_success();
            }
            sent = true;
            break;
        }
        if !sent {
            log_sender_drop(
                TAG,
                req_id.as_deref(),
                Some(chat_id.as_str()),
                CHANNEL_SENDER_MAX_RETRIES,
            );
        }
    }
}

fn map_stage(e: Error, stage: &'static str) -> Error {
    match e {
        Error::Http { status_code, .. } => Error::Http { status_code, stage },
        other => Error::Other {
            source: Box::new(other),
            stage,
        },
    }
}

/// 发送 typing 指示。连续 401 时 60s 内不再请求（退避）。失败 return Ok(()) 不阻塞 agent。
pub fn send_chat_action<H: ChannelHttpClient + ?Sized>(
    http: &mut H,
    token: &str,
    chat_id: &str,
    action: &str,
) -> Result<()> {
    use std::sync::atomic::{AtomicU32, Ordering};
    static LAST_401_SECS: AtomicU32 = AtomicU32::new(0);
    const BACKOFF_SECS: u32 = 60;

    let now_secs = std::time::SystemTime::UNIX_EPOCH
        .elapsed()
        .map(|d| d.as_secs() as u32)
        .unwrap_or(0);
    if now_secs.wrapping_sub(LAST_401_SECS.load(Ordering::Relaxed)) < BACKOFF_SECS {
        return Ok(());
    }
    let url = format!("{}{}/sendChatAction", TELEGRAM_API_BASE, token);
    let body = serde_json::json!({
        "chat_id": chat_id,
        "action": action,
    });
    let body_bytes = serde_json::to_vec(&body).map_err(|e| Error::Other {
        source: Box::new(e),
        stage: "sendChatAction",
    })?;
    let (status, _) = http
        .http_post(&url, &body_bytes)
        .map_err(|e| map_stage(e, "sendChatAction"))?;
    if status == 401 {
        LAST_401_SECS.store(now_secs, Ordering::Relaxed);
        return Ok(());
    }
    if status >= 400 {
        return Err(Error::Http {
            status_code: status,
            stage: "sendChatAction",
        });
    }
    Ok(())
}

/// 对指定消息设置 emoji 反应（入站 ACK）。失败仅打日志，不阻塞入队。
pub fn set_message_reaction<H: ChannelHttpClient>(
    http: &mut H,
    token: &str,
    chat_id: &str,
    message_id: i64,
    emoji: &str,
) -> Result<()> {
    let url = format!("{}{}/setMessageReaction", TELEGRAM_API_BASE, token);
    let body = serde_json::json!({
        "chat_id": chat_id,
        "message_id": message_id,
        "reaction": [{"type": "emoji", "emoji": emoji}]
    });
    let body_bytes = serde_json::to_vec(&body).map_err(|e| Error::Other {
        source: Box::new(e),
        stage: "setMessageReaction",
    })?;
    let (status, _) = http
        .http_post(&url, &body_bytes)
        .map_err(|e| map_stage(e, "setMessageReaction"))?;
    if status >= 400 {
        return Err(Error::Http {
            status_code: status,
            stage: "setMessageReaction",
        });
    }
    Ok(())
}

/// 发送消息并返回平台侧 message_id（字符串形式）；供流式编辑使用。
pub fn send_and_get_id<H: ChannelHttpClient>(
    http: &mut H,
    token: &str,
    chat_id: &str,
    content: &str,
) -> Result<Option<String>> {
    let body = serde_json::json!({
        "chat_id": chat_id,
        "text": content,
    });
    let body_bytes = serde_json::to_vec(&body).map_err(|e| Error::Other {
        source: Box::new(e),
        stage: "telegram_send",
    })?;
    let url = format!("{}{}/sendMessage", TELEGRAM_API_BASE, token);
    let (status, resp_body) = http
        .http_post(&url, &body_bytes)
        .map_err(|e| map_stage(e, "telegram_send"))?;
    if status >= 400 {
        return Err(Error::Http {
            status_code: status,
            stage: "telegram_send",
        });
    }
    #[derive(serde::Deserialize)]
    struct R {
        result: Option<Inner>,
    }
    #[derive(serde::Deserialize)]
    struct Inner {
        message_id: Option<i64>,
    }
    let r: R = serde_json::from_slice(resp_body.as_ref()).map_err(|e| Error::Other {
        source: Box::new(e),
        stage: "telegram_send_parse",
    })?;
    Ok(r.result.and_then(|i| i.message_id).map(|id| id.to_string()))
}

/// 编辑已发送的 Telegram 消息文本（editMessageText API）。
pub fn edit_message_text<H: ChannelHttpClient>(
    http: &mut H,
    token: &str,
    chat_id: &str,
    message_id: &str,
    content: &str,
) -> Result<()> {
    let msg_id: i64 = message_id
        .parse()
        .map_err(|_| Error::config("telegram_edit", "invalid message_id"))?;
    let body = serde_json::json!({
        "chat_id": chat_id,
        "message_id": msg_id,
        "text": content,
    });
    let body_bytes = serde_json::to_vec(&body).map_err(|e| Error::Other {
        source: Box::new(e),
        stage: "telegram_edit",
    })?;
    let url = format!("{}{}/editMessageText", TELEGRAM_API_BASE, token);
    let (status, _) = http
        .http_post(&url, &body_bytes)
        .map_err(|e| map_stage(e, "telegram_edit"))?;
    if status >= 400 {
        return Err(Error::Http {
            status_code: status,
            stage: "telegram_edit",
        });
    }
    Ok(())
}

/// 调用 getMe 获取 bot username（不含 @），供 mention 门控使用。失败或缺失返回 Ok(None)。
pub fn get_bot_username<H: ChannelHttpClient + ?Sized>(
    http: &mut H,
    token: &str,
) -> Result<Option<String>> {
    let url = format!("{}{}/getMe", TELEGRAM_API_BASE, token);
    let (status, body) = http.http_get(&url).map_err(|e| map_stage(e, "getMe"))?;
    if status >= 400 {
        return Ok(None);
    }
    #[derive(serde::Deserialize)]
    struct GetMeResult {
        result: Option<GetMeUser>,
    }
    #[derive(serde::Deserialize)]
    struct GetMeUser {
        username: Option<String>,
    }
    let r: GetMeResult = serde_json::from_slice(body.as_ref()).map_err(|e| Error::Other {
        source: Box::new(e),
        stage: "getMe_parse",
    })?;
    Ok(r.result.and_then(|u| u.username).filter(|s| !s.is_empty()))
}
