//! Telegram 出站：flush、send_chat_action、get_bot_username、set_message_reaction；连通性检查。Sink 统一为 dispatch::QueuedSink。
use crate::channels::ChannelHttpClient;
use crate::config::AppConfig;
use crate::error::{Error, Result};

use super::super::connectivity;

const TELEGRAM_API_BASE: &str = "https://api.telegram.org/bot";
const TELEGRAM_MAX_MESSAGE_LEN: usize = 4096;

/// 连通性检查：供 GET /api/channel_connectivity 使用。
pub fn check_connectivity<H: ChannelHttpClient + ?Sized>(
    config: &AppConfig,
    http: &mut H,
    loc: crate::i18n::Locale,
) -> super::super::connectivity::ChannelConnectivityItem {
    use crate::i18n::{tr, Message};
    let configured = !config.tg_token.trim().is_empty();
    if !configured {
        return connectivity::item(
            "telegram",
            false,
            false,
            Some(tr(Message::ConnectivityNotConfigured, loc)),
        );
    }
    match get_bot_username(http, config.tg_token.trim()) {
        Ok(Some(_)) => connectivity::item("telegram", true, true, None),
        Ok(None) => connectivity::item(
            "telegram",
            true,
            false,
            Some(tr(Message::ConnectivityTokenInvalid, loc)),
        ),
        Err(e) => {
            log::warn!("[telegram_connectivity] getMe: {}", e);
            connectivity::item(
                "telegram",
                true,
                false,
                Some(tr(Message::ConnectivityCheckFailed, loc)),
            )
        }
    }
}

fn send_one_telegram<H: ChannelHttpClient>(
    http: &mut H,
    token: &str,
    chat_id: &str,
    content: &str,
) {
    const TAG: &str = "telegram_send";
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
        let body_bytes = match serde_json::to_vec(&body) {
            Ok(b) => b,
            Err(e) => {
                log::warn!("[{}] json: {}", TAG, e);
                continue;
            }
        };
        if let Ok((status, resp_body)) =
            crate::channels::send::send_post(TAG, http, &url, &body_bytes)
        {
            if status >= 400 {
                continue;
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
    }
}

/// 从 rx 取出所有待发送（一次性 drain）。
pub fn flush_telegram_sends<H: ChannelHttpClient>(
    rx: &std::sync::mpsc::Receiver<(String, String, Option<String>)>,
    token: &str,
    http: &mut H,
) {
    while let Ok((chat_id, content, _req_id)) = rx.try_recv() {
        send_one_telegram(http, token, &chat_id, &content);
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
    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
    crate::platform::task_wdt::register_current_task_to_task_wdt();
    log::info!("[{}] sender loop started", TAG);

    let mut http: Option<H> = None;
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
            send_one_telegram(h, token, &chat_id, &content);
            while let Ok((cid, cnt, _)) = rx.try_recv() {
                send_one_telegram(h, token, &cid, &cnt);
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
