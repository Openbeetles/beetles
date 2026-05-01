//! Telegram 出站：flush、send_chat_action、get_bot_username、set_message_reaction；连通性检查。Sink 统一为 dispatch::QueuedSink。
use crate::bus::{
    AudioBody, CanonicalMessageBody, MediaLocatorKind, TextBody, TextFormat, VideoBody,
};
use crate::channels::ChannelHttpClient;
use crate::config::AppConfig;
use crate::error::{Error, Result};
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
use crate::platform::PlatformHttpClient;
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
use std::sync::Arc;

use super::super::connectivity;
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
use super::super::send::ActiveChannelSender;
use super::super::send::{
    ensure_sender_http, record_outbound_http_failure, record_outbound_http_success,
    run_buffered_sender_loop, QueuedOutboundMessage,
};

const TELEGRAM_API_BASE: &str = "https://api.telegram.org/bot";
const TELEGRAM_MAX_MESSAGE_LEN: usize = 4096;
const TELEGRAM_MAX_CAPTION_LEN: usize = 1024;

/// 连通性检查：供 GET /api/channel_connectivity 使用。
pub fn check_connectivity<H: ChannelHttpClient + ?Sized>(
    config: &AppConfig,
    http: &mut H,
) -> super::super::connectivity::ChannelConnectivityItem {
    let configured = !config.tg_token.trim().is_empty();
    connectivity::probe_item("telegram", configured, || {
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

fn parse_thread_id(stage: &'static str, platform_thread_id: &str) -> Result<Option<i64>> {
    let trimmed = platform_thread_id.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    trimmed
        .parse::<i64>()
        .map(Some)
        .map_err(|_| Error::config(stage, "invalid Telegram message_thread_id"))
}

fn parse_mode_for_format(stage: &'static str, format: TextFormat) -> Result<Option<&'static str>> {
    match format {
        TextFormat::Plain => Ok(None),
        TextFormat::Markdown => Ok(Some("MarkdownV2")),
        TextFormat::Html => Ok(Some("HTML")),
        TextFormat::RichText => Err(Error::config(
            stage,
            "Telegram does not support TextFormat::RichText",
        )),
    }
}

fn validate_nonempty_locator<'a>(
    stage: &'static str,
    locator_kind: MediaLocatorKind,
    locator: &'a str,
) -> Result<&'a str> {
    if locator.trim().is_empty() {
        return Err(Error::config(stage, "media locator is empty"));
    }
    if matches!(locator_kind, MediaLocatorKind::BeetleBlob) {
        return Err(Error::config(
            stage,
            "Telegram multipart upload is not implemented for BeetleBlob locators",
        ));
    }
    Ok(locator.trim())
}

fn apply_thread_id(
    body: &mut serde_json::Value,
    platform_thread_id: &str,
    stage: &'static str,
) -> Result<()> {
    if let Some(thread_id) = parse_thread_id(stage, platform_thread_id)? {
        body["message_thread_id"] = serde_json::json!(thread_id);
    }
    Ok(())
}

fn apply_text_parse_mode(
    body: &mut serde_json::Value,
    format: TextFormat,
    stage: &'static str,
) -> Result<()> {
    if let Some(parse_mode) = parse_mode_for_format(stage, format)? {
        body["parse_mode"] = serde_json::json!(parse_mode);
    }
    Ok(())
}

fn apply_caption(
    body: &mut serde_json::Value,
    caption: Option<&TextBody>,
    stage: &'static str,
) -> Result<()> {
    let Some(caption) = caption else {
        return Ok(());
    };
    let trimmed = caption.text.trim();
    if trimmed.is_empty() {
        return Ok(());
    }
    if trimmed.chars().count() > TELEGRAM_MAX_CAPTION_LEN {
        return Err(Error::config(
            stage,
            "caption exceeds Telegram 1024-character limit",
        ));
    }
    body["caption"] = serde_json::json!(caption.text);
    apply_text_parse_mode(body, caption.format, stage)
}

fn post_telegram_method<H: ChannelHttpClient>(
    http: &mut H,
    token: &str,
    method: &str,
    body: &serde_json::Value,
    stage: &'static str,
) -> Result<crate::platform::ResponseBody> {
    let body_bytes = serde_json::to_vec(body).map_err(|e| Error::Other {
        source: Box::new(e),
        stage,
    })?;
    let url = format!("{}{}/{}", TELEGRAM_API_BASE, token, method);
    let (status, resp_body) = crate::channels::send::send_post(stage, http, &url, &body_bytes)
        .map_err(|e| map_stage(e, stage))?;
    if status >= 400 {
        return Err(Error::Http {
            status_code: status,
            stage,
        });
    }
    Ok(resp_body)
}

fn parse_sent_message_id(resp_body: &crate::platform::ResponseBody) -> Option<i64> {
    #[derive(serde::Deserialize)]
    struct SendMessageResult {
        result: Option<SendMessageResultInner>,
    }
    #[derive(serde::Deserialize)]
    struct SendMessageResultInner {
        message_id: Option<i64>,
    }
    serde_json::from_slice::<SendMessageResult>(resp_body.as_ref())
        .ok()
        .and_then(|result| result.result.and_then(|inner| inner.message_id))
}

fn send_text_message<H: ChannelHttpClient>(
    http: &mut H,
    token: &str,
    chat_id: &str,
    platform_thread_id: &str,
    text: &str,
    format: TextFormat,
) -> Result<()> {
    const TAG: &str = "telegram_send";
    if text.trim().is_empty() {
        return Err(Error::config(
            TAG,
            "refusing to send empty Telegram message",
        ));
    }
    let mut reply_to_message_id: Option<i64> = None;
    for chunk in crate::channels::chunk::chunk_text_by_char_count(text, TELEGRAM_MAX_MESSAGE_LEN) {
        let mut body = serde_json::json!({
            "chat_id": chat_id,
            "text": chunk,
        });
        apply_thread_id(&mut body, platform_thread_id, TAG)?;
        apply_text_parse_mode(&mut body, format, TAG)?;
        if let Some(id) = reply_to_message_id {
            body["reply_to_message_id"] = serde_json::json!(id);
        }
        let resp_body = post_telegram_method(http, token, "sendMessage", &body, TAG)?;
        reply_to_message_id = parse_sent_message_id(&resp_body).or(reply_to_message_id);
    }
    Ok(())
}

fn looks_like_voice(audio: &AudioBody) -> bool {
    audio
        .asset
        .mime_type
        .as_deref()
        .is_some_and(|mime| mime.eq_ignore_ascii_case("audio/ogg") || mime.ends_with("/ogg"))
        || audio
            .asset
            .file_name
            .as_deref()
            .is_some_and(|name| name.to_ascii_lowercase().ends_with(".ogg"))
}

fn audio_send_method(audio: &AudioBody) -> (&'static str, &'static str) {
    if looks_like_voice(audio) {
        ("sendVoice", "voice")
    } else {
        ("sendAudio", "audio")
    }
}

fn duration_seconds(duration_ms: Option<u32>) -> Option<u32> {
    duration_ms.map(|duration| (duration.saturating_add(999)) / 1000)
}

fn send_audio_message<H: ChannelHttpClient>(
    http: &mut H,
    token: &str,
    message: &QueuedOutboundMessage,
    audio: &AudioBody,
) -> Result<()> {
    const TAG: &str = "telegram_send";
    let locator = validate_nonempty_locator(TAG, audio.asset.locator_kind, &audio.asset.locator)?;
    let (method, field_name) = audio_send_method(audio);
    let mut body = serde_json::json!({
        "chat_id": message.chat_id,
        field_name: locator,
    });
    apply_thread_id(&mut body, &message.platform_thread_id, TAG)?;
    apply_caption(&mut body, audio.caption.as_ref(), TAG)?;
    if let Some(duration) = duration_seconds(audio.asset.duration_ms) {
        body["duration"] = serde_json::json!(duration);
    }
    post_telegram_method(http, token, method, &body, TAG).map(|_| ())
}

fn send_video_message<H: ChannelHttpClient>(
    http: &mut H,
    token: &str,
    message: &QueuedOutboundMessage,
    video: &VideoBody,
) -> Result<()> {
    const TAG: &str = "telegram_send";
    let locator = validate_nonempty_locator(TAG, video.asset.locator_kind, &video.asset.locator)?;
    let mut body = serde_json::json!({
        "chat_id": message.chat_id,
        "video": locator,
    });
    apply_thread_id(&mut body, &message.platform_thread_id, TAG)?;
    apply_caption(&mut body, video.caption.as_ref(), TAG)?;
    if let Some(duration) = duration_seconds(video.asset.duration_ms) {
        body["duration"] = serde_json::json!(duration);
    }
    if let Some(width) = video.asset.width_px {
        body["width"] = serde_json::json!(width);
    }
    if let Some(height) = video.asset.height_px {
        body["height"] = serde_json::json!(height);
    }
    post_telegram_method(http, token, "sendVideo", &body, TAG).map(|_| ())
}

fn send_media_message<H: ChannelHttpClient>(
    http: &mut H,
    token: &str,
    message: &QueuedOutboundMessage,
) -> Result<()> {
    const TAG: &str = "telegram_send";
    match &message.body {
        CanonicalMessageBody::Text(text) => send_text_message(
            http,
            token,
            &message.chat_id,
            &message.platform_thread_id,
            &text.text,
            text.format,
        ),
        CanonicalMessageBody::Image(image) => {
            let locator =
                validate_nonempty_locator(TAG, image.asset.locator_kind, &image.asset.locator)?;
            let mut body = serde_json::json!({
                "chat_id": message.chat_id,
                "photo": locator,
            });
            apply_thread_id(&mut body, &message.platform_thread_id, TAG)?;
            apply_caption(&mut body, image.caption.as_ref(), TAG)?;
            post_telegram_method(http, token, "sendPhoto", &body, TAG).map(|_| ())
        }
        CanonicalMessageBody::Audio(audio) => send_audio_message(http, token, message, audio),
        CanonicalMessageBody::Video(video) => send_video_message(http, token, message, video),
        CanonicalMessageBody::File(file) => {
            let locator =
                validate_nonempty_locator(TAG, file.asset.locator_kind, &file.asset.locator)?;
            let mut body = serde_json::json!({
                "chat_id": message.chat_id,
                "document": locator,
            });
            apply_thread_id(&mut body, &message.platform_thread_id, TAG)?;
            apply_caption(&mut body, file.caption.as_ref(), TAG)?;
            post_telegram_method(http, token, "sendDocument", &body, TAG).map(|_| ())
        }
        CanonicalMessageBody::Card(card) => send_text_message(
            http,
            token,
            &message.chat_id,
            &message.platform_thread_id,
            &card.fallback_text,
            TextFormat::Plain,
        ),
        CanonicalMessageBody::PlatformNative(native) => send_text_message(
            http,
            token,
            &message.chat_id,
            &message.platform_thread_id,
            &native.fallback_text,
            TextFormat::Plain,
        ),
    }
}

/// 从 rx 取出所有待发送（一次性 drain）。
pub fn flush_telegram_sends<H: ChannelHttpClient>(
    rx: &std::sync::mpsc::Receiver<QueuedOutboundMessage>,
    token: &str,
    http: &mut H,
) {
    while let Ok(message) = rx.try_recv() {
        if let Err(error) = send_media_message(http, token, &message) {
            record_outbound_http_failure(&error);
            log::warn!(
                "[telegram_flush] send failed for chat_id={}: {}",
                message.chat_id,
                error
            );
        } else {
            record_outbound_http_success();
        }
    }
}

/// 持续运行的 Telegram 发送循环：sender 线程内**复用**同一 HTTP 客户端，减轻 lwIP socket / TLS 压力。
pub fn run_telegram_sender_loop<H, F>(
    rx: std::sync::mpsc::Receiver<QueuedOutboundMessage>,
    token: &str,
    mut create_http: F,
) where
    H: ChannelHttpClient,
    F: FnMut() -> crate::error::Result<H>,
{
    const TAG: &str = "telegram_sender";
    let mut http: Option<H> = None;
    run_buffered_sender_loop(rx, TAG, |message, attempt| {
        if !ensure_sender_http(&mut http, &mut create_http, TAG, attempt) {
            return Err(Error::config(TAG, "create http failed"));
        }
        let Some(h) = http.as_mut() else {
            return Err(Error::config(TAG, "sender http missing after ensure"));
        };
        match send_media_message(h, token, message) {
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
                    message.chat_id,
                    error
                );
                http = None;
                Err(error)
            }
        }
    });
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
pub(crate) struct TelegramOutboundDriver {
    token: String,
    http: Option<Box<dyn PlatformHttpClient>>,
    create_http: Arc<dyn Fn() -> crate::Result<Box<dyn PlatformHttpClient>> + Send + Sync>,
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
pub(crate) fn telegram_outbound_driver(
    token: String,
    create_http: Arc<dyn Fn() -> crate::Result<Box<dyn PlatformHttpClient>> + Send + Sync>,
) -> Box<dyn ActiveChannelSender> {
    Box::new(TelegramOutboundDriver {
        token,
        http: None,
        create_http,
    })
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
impl ActiveChannelSender for TelegramOutboundDriver {
    fn tag(&self) -> &'static str {
        "telegram_sender"
    }

    fn send_attempt(
        &mut self,
        message: &QueuedOutboundMessage,
        attempt: u8,
    ) -> crate::error::Result<()> {
        const TAG: &str = "telegram_sender";
        let create_http = Arc::clone(&self.create_http);
        let mut create = || create_http();
        if !ensure_sender_http(&mut self.http, &mut create, TAG, attempt) {
            return Err(Error::config(TAG, "create http failed"));
        }
        let Some(h) = self.http.as_mut() else {
            return Err(Error::config(TAG, "sender http missing after ensure"));
        };
        match send_media_message(h, &self.token, message) {
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
                    message.chat_id,
                    error
                );
                if !matches!(error, Error::Config { .. }) {
                    self.http = None;
                }
                Err(error)
            }
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
    let (status, _) = match http.http_post(&url, &body_bytes) {
        Ok(resp) => resp,
        Err(e) => {
            let error = map_stage(e, "sendChatAction");
            record_outbound_http_failure(&error);
            return Err(error);
        }
    };
    if status == 401 {
        LAST_401_SECS.store(now_secs, Ordering::Relaxed);
        let error = Error::Http {
            status_code: status,
            stage: "sendChatAction",
        };
        record_outbound_http_failure(&error);
        return Ok(());
    }
    if status >= 400 {
        let error = Error::Http {
            status_code: status,
            stage: "sendChatAction",
        };
        record_outbound_http_failure(&error);
        return Err(error);
    }
    record_outbound_http_success();
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
    let (status, _) = match http.http_post(&url, &body_bytes) {
        Ok(resp) => resp,
        Err(e) => {
            let error = map_stage(e, "setMessageReaction");
            record_outbound_http_failure(&error);
            return Err(error);
        }
    };
    if status >= 400 {
        let error = Error::Http {
            status_code: status,
            stage: "setMessageReaction",
        };
        record_outbound_http_failure(&error);
        return Err(error);
    }
    record_outbound_http_success();
    Ok(())
}

/// 发送消息并返回平台侧 message_id（字符串形式）；供流式编辑使用。
pub fn send_and_get_id<H: ChannelHttpClient>(
    http: &mut H,
    token: &str,
    chat_id: &str,
    content: &str,
) -> Result<Option<String>> {
    if content.chars().count() > TELEGRAM_MAX_MESSAGE_LEN {
        return Err(Error::config(
            "telegram_send",
            "content exceeds Telegram 4096-character limit",
        ));
    }
    let body = serde_json::json!({
        "chat_id": chat_id,
        "text": content,
    });
    let body_bytes = serde_json::to_vec(&body).map_err(|e| Error::Other {
        source: Box::new(e),
        stage: "telegram_send",
    })?;
    let url = format!("{}{}/sendMessage", TELEGRAM_API_BASE, token);
    let (status, resp_body) = match http.http_post(&url, &body_bytes) {
        Ok(resp) => resp,
        Err(e) => {
            let error = map_stage(e, "telegram_send");
            record_outbound_http_failure(&error);
            return Err(error);
        }
    };
    if status >= 400 {
        let error = Error::Http {
            status_code: status,
            stage: "telegram_send",
        };
        record_outbound_http_failure(&error);
        return Err(error);
    }
    record_outbound_http_success();
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
    if content.chars().count() > TELEGRAM_MAX_MESSAGE_LEN {
        return Err(Error::config(
            "telegram_edit",
            "content exceeds Telegram 4096-character limit",
        ));
    }
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
    let (status, _) = match http.http_post(&url, &body_bytes) {
        Ok(resp) => resp,
        Err(e) => {
            let error = map_stage(e, "telegram_edit");
            record_outbound_http_failure(&error);
            return Err(error);
        }
    };
    if status >= 400 {
        let error = Error::Http {
            status_code: status,
            stage: "telegram_edit",
        };
        record_outbound_http_failure(&error);
        return Err(error);
    }
    record_outbound_http_success();
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bus::{
        AudioBody, CanonicalMessageBody, CardBody, CardFormat, FileBody, ImageBody, MediaAssetRef,
        OutboundKind, TextBody, TextFormat, VideoBody,
    };
    use crate::platform::ResponseBody;
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    type PostRequests = Arc<Mutex<Vec<(String, Vec<u8>)>>>;

    #[derive(Default)]
    struct StubHttp {
        post_results: VecDeque<Result<(u16, ResponseBody)>>,
        post_requests: PostRequests,
    }

    impl ChannelHttpClient for StubHttp {
        fn http_get(&mut self, _url: &str) -> Result<(u16, ResponseBody)> {
            Ok((200, ResponseBody::Heap(b"{}".to_vec())))
        }

        fn http_get_with_headers(
            &mut self,
            _url: &str,
            _headers: &[(&str, &str)],
        ) -> Result<(u16, ResponseBody)> {
            Ok((200, ResponseBody::Heap(b"{}".to_vec())))
        }

        fn http_post(&mut self, _url: &str, _body: &[u8]) -> Result<(u16, ResponseBody)> {
            self.post_requests
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .push((_url.to_string(), _body.to_vec()));
            self.post_results
                .pop_front()
                .unwrap_or_else(|| Ok((200, ResponseBody::Heap(b"{}".to_vec()))))
        }

        fn http_post_with_headers(
            &mut self,
            _url: &str,
            _headers: &[(&str, &str)],
            _body: &[u8],
        ) -> Result<(u16, ResponseBody)> {
            self.http_post(_url, _body)
        }
    }

    fn queued_message(body: CanonicalMessageBody) -> QueuedOutboundMessage {
        QueuedOutboundMessage {
            transport_send_id: 1,
            chat_id: "chat-1".to_string(),
            content: body.text_projection(),
            body,
            platform_thread_id: "77".to_string(),
            platform_message_id: String::new(),
            req_id: Some("req-1".to_string()),
            outbound_kind: OutboundKind::Primary,
        }
    }

    fn parse_body(request: &(String, Vec<u8>)) -> serde_json::Value {
        serde_json::from_slice(&request.1).expect("telegram request body")
    }

    #[test]
    fn send_and_get_id_records_outbound_http_success() {
        let _guard = crate::orchestrator::runtime_capability::RUNTIME_CAPABILITY_TEST_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        crate::orchestrator::reset_runtime_capabilities_for_tests();
        let before = crate::metrics::snapshot();
        let mut http = StubHttp {
            post_results: VecDeque::from([Ok((
                200,
                ResponseBody::Heap(br#"{"result":{"message_id":42}}"#.to_vec()),
            ))]),
            ..Default::default()
        };

        let message_id = send_and_get_id(&mut http, "token", "chat-1", "hello").expect("send");

        let after = crate::metrics::snapshot();
        assert_eq!(message_id.as_deref(), Some("42"));
        assert!(after.channel_http_ok > before.channel_http_ok);
    }

    #[test]
    fn send_chat_action_tls_admission_failure_marks_outbound_http_recovering() {
        let _guard = crate::orchestrator::runtime_capability::RUNTIME_CAPABILITY_TEST_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        crate::orchestrator::reset_runtime_capabilities_for_tests();
        let before = crate::metrics::snapshot();
        let mut http = StubHttp {
            post_results: VecDeque::from([Err(Error::config("tls_admission", "permit timeout"))]),
            ..Default::default()
        };

        let _ = send_chat_action(&mut http, "token", "chat-1", "typing");

        let after = crate::metrics::snapshot();
        let capability = crate::orchestrator::get_runtime_capability(
            crate::orchestrator::RUNTIME_CAPABILITY_NETWORK_OUTBOUND_HTTP,
        )
        .expect("capability");
        assert!(after.channel_http_fail > before.channel_http_fail);
        assert_eq!(
            capability.status,
            crate::orchestrator::RuntimeCapabilityStatus::Degraded
        );
        assert_eq!(
            capability.reason,
            crate::orchestrator::RuntimeCapabilityReason::RecoveryStabilizing
        );
    }

    #[test]
    fn send_media_message_renders_photo_body_with_html_caption() {
        let mut http = StubHttp::default();
        let message = queued_message(CanonicalMessageBody::Image(ImageBody {
            asset: MediaAssetRef {
                locator_kind: crate::bus::MediaLocatorKind::ExternalUrl,
                locator: "https://example.com/image.png".to_string(),
                ..MediaAssetRef::default()
            },
            caption: Some(TextBody {
                text: "<b>hello</b>".to_string(),
                format: TextFormat::Html,
            }),
        }));

        send_media_message(&mut http, "token", &message).expect("photo send");

        let requests = http
            .post_requests
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone();
        assert_eq!(requests.len(), 1);
        assert!(requests[0].0.ends_with("/sendPhoto"));
        let body = parse_body(&requests[0]);
        assert_eq!(body["chat_id"], "chat-1");
        assert_eq!(body["photo"], "https://example.com/image.png");
        assert_eq!(body["caption"], "<b>hello</b>");
        assert_eq!(body["parse_mode"], "HTML");
        assert_eq!(body["message_thread_id"], 77);
    }

    #[test]
    fn send_media_message_renders_voice_body() {
        let mut http = StubHttp::default();
        let message = queued_message(CanonicalMessageBody::Audio(AudioBody {
            asset: MediaAssetRef {
                locator_kind: crate::bus::MediaLocatorKind::PlatformHandle,
                locator: "voice_file_id".to_string(),
                mime_type: Some("audio/ogg".to_string()),
                duration_ms: Some(2300),
                ..MediaAssetRef::default()
            },
            caption: Some(TextBody::plain("voice caption")),
            transcript_text: None,
        }));

        send_media_message(&mut http, "token", &message).expect("voice send");

        let requests = http
            .post_requests
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone();
        assert_eq!(requests.len(), 1);
        assert!(requests[0].0.ends_with("/sendVoice"));
        let body = parse_body(&requests[0]);
        assert_eq!(body["voice"], "voice_file_id");
        assert_eq!(body["caption"], "voice caption");
        assert_eq!(body["duration"], 3);
    }

    #[test]
    fn send_media_message_renders_document_body() {
        let mut http = StubHttp::default();
        let message = queued_message(CanonicalMessageBody::File(FileBody {
            asset: MediaAssetRef {
                locator_kind: crate::bus::MediaLocatorKind::PlatformHandle,
                locator: "document_file_id".to_string(),
                ..MediaAssetRef::default()
            },
            caption: Some(TextBody {
                text: "*report*".to_string(),
                format: TextFormat::Markdown,
            }),
        }));

        send_media_message(&mut http, "token", &message).expect("document send");

        let requests = http
            .post_requests
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone();
        assert_eq!(requests.len(), 1);
        assert!(requests[0].0.ends_with("/sendDocument"));
        let body = parse_body(&requests[0]);
        assert_eq!(body["document"], "document_file_id");
        assert_eq!(body["caption"], "*report*");
        assert_eq!(body["parse_mode"], "MarkdownV2");
    }

    #[test]
    fn send_media_message_falls_back_card_body_to_text_message() {
        let mut http = StubHttp::default();
        let message = queued_message(CanonicalMessageBody::Card(CardBody {
            format: CardFormat::Interactive,
            payload_json: serde_json::json!({"title":"ignored"}),
            fallback_text: "card fallback".to_string(),
        }));

        send_media_message(&mut http, "token", &message).expect("card fallback send");

        let requests = http
            .post_requests
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone();
        assert_eq!(requests.len(), 1);
        assert!(requests[0].0.ends_with("/sendMessage"));
        let body = parse_body(&requests[0]);
        assert_eq!(body["text"], "card fallback");
    }

    #[test]
    fn send_media_message_rejects_beetle_blob_media_locator() {
        let mut http = StubHttp::default();
        let message = queued_message(CanonicalMessageBody::Video(VideoBody {
            asset: MediaAssetRef {
                locator_kind: crate::bus::MediaLocatorKind::BeetleBlob,
                locator: "blob-1".to_string(),
                ..MediaAssetRef::default()
            },
            caption: None,
            title: None,
            description: None,
        }));

        let err = send_media_message(&mut http, "token", &message).expect_err("blob rejected");

        assert_eq!(err.stage(), "telegram_send");
    }
}
