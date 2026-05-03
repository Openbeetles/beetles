//! 飞书出站：flush、token 类型、event_body_to_pcmsg、连通性检查。
//! Rich message bodies follow Feishu official `msg_type` / `content` contracts.

use crate::bus::{
    AssetSourcePlatform, CanonicalMessageBody, CardBody, CardFormat, FileBody, ImageBody,
    MediaAssetRef, MediaLocatorKind, MessageTransport, PcMsg, PlatformNativeBody, TextBody,
    TextFormat, VideoBody,
};
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
use crate::channels::send::ActiveChannelSender;
use crate::channels::send::{
    ensure_sender_http, record_outbound_http_failure, record_outbound_http_success,
    run_buffered_sender_loop, QueuedOutboundMessage,
};
use crate::channels::ChannelHttpClient;
use crate::config::AppConfig;
use crate::error::{Error, Result};
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
use crate::platform::PlatformHttpClient;
use serde_json::{json, Value};
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
use std::sync::Arc;

pub const FEISHU_TOKEN_URL: &str =
    "https://open.feishu.cn/open-apis/auth/v3/tenant_access_token/internal";
const FEISHU_SEND_URL: &str =
    "https://open.feishu.cn/open-apis/im/v1/messages?receive_id_type=chat_id";
const FEISHU_EDIT_URL_PREFIX: &str = "https://open.feishu.cn/open-apis/im/v1/messages/";
const FEISHU_MAX_MESSAGE_LEN: usize = 4096;
const FEISHU_MAX_CARD_FALLBACK_LEN: usize = 2048;
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
    ) -> Result<String> {
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
            .ok_or_else(|| Error::config(stage, "token missing after refresh"))
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
) -> Result<String> {
    acquire_tenant_token_with_stage(http, app_id, app_secret, "feishu_token")
}

fn acquire_tenant_token_with_stage<H: ChannelHttpClient + ?Sized>(
    http: &mut H,
    app_id: &str,
    app_secret: &str,
    stage: &'static str,
) -> Result<String> {
    let body = FeishuTokenRequest {
        app_id: app_id.to_string(),
        app_secret: app_secret.to_string(),
    };
    let body_bytes = serde_json::to_vec(&body).map_err(|e| Error::Other {
        source: Box::new(e),
        stage,
    })?;
    let (status, resp_body) = match http.http_post(FEISHU_TOKEN_URL, &body_bytes) {
        Ok(r) => r,
        Err(e) => {
            let error = Error::Other {
                source: Box::new(e),
                stage,
            };
            record_outbound_http_failure(&error);
            return Err(error);
        }
    };
    if status >= 400 {
        let error = Error::Http {
            status_code: status,
            stage,
        };
        record_outbound_http_failure(&error);
        return Err(error);
    }
    record_outbound_http_success();
    let token_resp: FeishuTokenResponse =
        serde_json::from_slice(resp_body.as_ref()).map_err(|e| Error::Other {
            source: Box::new(e),
            stage,
        })?;
    if token_resp.code != 0 {
        return Err(Error::config(
            stage,
            format!(
                "feishu tenant_access_token returned code={}",
                token_resp.code
            ),
        ));
    }
    match token_resp.tenant_access_token {
        Some(t) if !t.is_empty() => Ok(t),
        _ => Err(Error::config(stage, "tenant_access_token missing")),
    }
}

fn feishu_platform_handle<'a>(
    stage: &'static str,
    asset: &'a MediaAssetRef,
    kind: &'static str,
) -> Result<&'a str> {
    if asset.locator_kind != MediaLocatorKind::PlatformHandle {
        return Err(Error::config(
            stage,
            format!("Feishu {kind} requires a platform handle key"),
        ));
    }
    let locator = asset.locator.trim();
    if locator.is_empty() {
        return Err(Error::config(stage, format!("Feishu {kind} key is empty")));
    }
    Ok(locator)
}

fn feishu_post_text_segments(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::Array(items) => {
            for item in items {
                feishu_post_text_segments(item, out);
            }
        }
        Value::Object(map) => {
            if let Some(tag) = map.get("tag").and_then(Value::as_str) {
                match tag {
                    "text" | "a" | "code_block" | "md" => {
                        if let Some(text) = map.get("text").and_then(Value::as_str) {
                            let trimmed = text.trim();
                            if !trimmed.is_empty() {
                                out.push(trimmed.to_string());
                            }
                        }
                        return;
                    }
                    "at" => {
                        if let Some(name) = map
                            .get("user_name")
                            .and_then(Value::as_str)
                            .or_else(|| map.get("user_id").and_then(Value::as_str))
                        {
                            let trimmed = name.trim();
                            if !trimmed.is_empty() {
                                out.push(trimmed.to_string());
                            }
                        }
                        return;
                    }
                    "img" => {
                        out.push("[image]".to_string());
                        return;
                    }
                    "media" => {
                        out.push("[video]".to_string());
                        return;
                    }
                    _ => {}
                }
            }
            if let Some(title) = map.get("title").and_then(Value::as_str) {
                let trimmed = title.trim();
                if !trimmed.is_empty() {
                    out.push(trimmed.to_string());
                }
            }
            if let Some(content) = map.get("content") {
                feishu_post_text_segments(content, out);
            }
            for value in map.values() {
                feishu_post_text_segments(value, out);
            }
        }
        _ => {}
    }
}

fn feishu_post_fallback_text(post: &Value) -> String {
    let mut segments = Vec::new();
    feishu_post_text_segments(post, &mut segments);
    let joined = segments
        .into_iter()
        .filter(|segment| !segment.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    if joined.trim().is_empty() {
        "[card]".to_string()
    } else {
        crate::bus::truncate_content_to_max(&joined, FEISHU_MAX_CARD_FALLBACK_LEN)
            .trim()
            .to_string()
    }
}

fn feishu_interactive_fallback_text(card: &Value) -> String {
    let summary = card
        .get("header")
        .and_then(|header| header.get("title"))
        .and_then(|title| title.get("content"))
        .and_then(Value::as_str)
        .or_else(|| card.get("type").and_then(Value::as_str))
        .unwrap_or("[card]");
    let trimmed = summary.trim();
    if trimmed.is_empty() {
        "[card]".to_string()
    } else {
        crate::bus::truncate_content_to_max(trimmed, FEISHU_MAX_CARD_FALLBACK_LEN)
            .trim()
            .to_string()
    }
}

fn feishu_text_body(text: &TextBody) -> Result<(String, String)> {
    if text.format == TextFormat::RichText {
        return Err(Error::config(
            "feishu_send",
            "TextFormat::RichText must use post payloads on Feishu",
        ));
    }
    let inner = json!({ "text": text.text });
    let content =
        serde_json::to_string(&inner).map_err(|e| Error::config("feishu_send", e.to_string()))?;
    Ok(("text".to_string(), content))
}

fn feishu_card_body(card: &CardBody) -> Result<(String, String)> {
    match card.format {
        CardFormat::Interactive => {
            let content = serde_json::to_string(&card.payload_json)
                .map_err(|e| Error::config("feishu_send", e.to_string()))?;
            Ok(("interactive".to_string(), content))
        }
        CardFormat::RichPost => {
            let content = serde_json::to_string(&card.payload_json)
                .map_err(|e| Error::config("feishu_send", e.to_string()))?;
            Ok(("post".to_string(), content))
        }
        _ if !card.fallback_text.trim().is_empty() => {
            feishu_text_body(&TextBody::plain(card.fallback_text.clone()))
        }
        _ => Err(Error::config(
            "feishu_send",
            "unsupported Feishu card format without fallback text",
        )),
    }
}

fn feishu_native_body(native: &PlatformNativeBody) -> Result<(String, String)> {
    match native.platform_type.trim() {
        "interactive" | "post" => {
            let content = serde_json::to_string(&native.payload_json)
                .map_err(|e| Error::config("feishu_send", e.to_string()))?;
            Ok((native.platform_type.trim().to_string(), content))
        }
        _ if !native.fallback_text.trim().is_empty() => {
            feishu_text_body(&TextBody::plain(native.fallback_text.clone()))
        }
        _ => Err(Error::config(
            "feishu_send",
            "unsupported Feishu native payload without fallback text",
        )),
    }
}

fn feishu_message_shape(body: &CanonicalMessageBody) -> Result<(String, String)> {
    match body {
        CanonicalMessageBody::Text(text) => feishu_text_body(text),
        CanonicalMessageBody::Image(image) => {
            let image_key = feishu_platform_handle("feishu_send", &image.asset, "image")?;
            let content = serde_json::to_string(&json!({ "image_key": image_key }))
                .map_err(|e| Error::config("feishu_send", e.to_string()))?;
            Ok(("image".to_string(), content))
        }
        CanonicalMessageBody::Audio(audio) => {
            let file_key = feishu_platform_handle("feishu_send", &audio.asset, "audio")?;
            let content = serde_json::to_string(&json!({ "file_key": file_key }))
                .map_err(|e| Error::config("feishu_send", e.to_string()))?;
            Ok(("audio".to_string(), content))
        }
        CanonicalMessageBody::Video(video) => {
            let file_key = feishu_platform_handle("feishu_send", &video.asset, "video")?;
            let mut inner = json!({ "file_key": file_key });
            if let Some(image_key) = video.description.as_deref().filter(|_| false) {
                inner["image_key"] = json!(image_key);
            }
            let content = serde_json::to_string(&inner)
                .map_err(|e| Error::config("feishu_send", e.to_string()))?;
            Ok(("media".to_string(), content))
        }
        CanonicalMessageBody::File(file) => {
            let file_key = feishu_platform_handle("feishu_send", &file.asset, "file")?;
            let content = serde_json::to_string(&json!({ "file_key": file_key }))
                .map_err(|e| Error::config("feishu_send", e.to_string()))?;
            Ok(("file".to_string(), content))
        }
        CanonicalMessageBody::Card(card) => feishu_card_body(card),
        CanonicalMessageBody::PlatformNative(native) => feishu_native_body(native),
    }
}

fn build_feishu_request_body(
    receive_id: Option<&str>,
    msg_type: &str,
    content: &str,
) -> Result<Vec<u8>> {
    let mut body = serde_json::Map::new();
    if let Some(chat_id) = receive_id {
        body.insert("receive_id".to_string(), json!(chat_id));
    }
    body.insert("msg_type".to_string(), json!(msg_type));
    body.insert("content".to_string(), json!(content));
    serde_json::to_vec(&Value::Object(body))
        .map_err(|e| Error::config("feishu_send", e.to_string()))
}

fn post_feishu_message<H: ChannelHttpClient>(
    http: &mut H,
    token: &str,
    receive_id: Option<&str>,
    body: &CanonicalMessageBody,
) -> Result<crate::platform::ResponseBody> {
    const TAG: &str = "feishu_send";
    let (msg_type, content) = feishu_message_shape(body)?;
    let body_bytes = build_feishu_request_body(receive_id, &msg_type, &content)?;
    let auth_val = format!("Bearer {}", token);
    let headers = [
        ("Authorization", auth_val.as_str()),
        ("Content-Type", "application/json; charset=utf-8"),
    ];
    let (status, resp_body) = crate::channels::send::send_post_with_headers(
        TAG,
        http,
        FEISHU_SEND_URL,
        &headers,
        &body_bytes,
    )?;
    if status >= 400 {
        return Err(Error::Http {
            status_code: status,
            stage: TAG,
        });
    }
    Ok(resp_body)
}

fn send_feishu_message<H: ChannelHttpClient>(
    http: &mut H,
    token: &str,
    message: &QueuedOutboundMessage,
) -> Result<()> {
    if message.content.trim().is_empty() && message.body.kind() == crate::bus::MessageBodyKind::Text
    {
        return Err(Error::config(
            "feishu_send",
            "refusing to send empty Feishu message",
        ));
    }
    match &message.body {
        CanonicalMessageBody::Text(text) => {
            for chunk in
                crate::channels::chunk::chunk_text_by_char_count(&text.text, FEISHU_MAX_MESSAGE_LEN)
            {
                let chunk_body = CanonicalMessageBody::Text(TextBody {
                    text: chunk,
                    format: text.format,
                });
                let _ = post_feishu_message(http, token, Some(&message.chat_id), &chunk_body)?;
            }
            Ok(())
        }
        other => {
            let _ = post_feishu_message(http, token, Some(&message.chat_id), other)?;
            Ok(())
        }
    }
}

/// 从 rx 取出待发送，鉴权后调用飞书发消息 API（一次性 drain）。
pub fn flush_feishu_sends<H: ChannelHttpClient>(
    rx: &std::sync::mpsc::Receiver<QueuedOutboundMessage>,
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
    while let Ok(message) = rx.try_recv() {
        if let Err(error) = send_feishu_message(http, &token, &message) {
            record_outbound_http_failure(&error);
            log::warn!(
                "[feishu_flush] send failed for chat_id={}: {}",
                message.chat_id,
                error
            );
        } else {
            record_outbound_http_success();
        }
    }
}

/// 持续运行的飞书发送循环：sender 线程内**复用**同一 HTTP；tenant_access_token 仍按 TTL 缓存，减少 getToken 次数。
pub fn run_feishu_sender_loop<H, F>(
    rx: std::sync::mpsc::Receiver<QueuedOutboundMessage>,
    app_id: &str,
    app_secret: &str,
    mut create_http: F,
) where
    H: ChannelHttpClient,
    F: FnMut() -> Result<H>,
{
    const TAG: &str = "feishu_sender";
    let mut http: Option<H> = None;
    let mut token_cache = FeishuTokenCache::new();
    run_buffered_sender_loop(rx, TAG, |message, attempt| {
        if !ensure_sender_http(&mut http, &mut create_http, TAG, attempt) {
            return Err(Error::config(TAG, "create http failed"));
        }
        let Some(h) = http.as_mut() else {
            return Err(Error::config(TAG, "sender http missing after ensure"));
        };
        let token = match token_cache.ensure_token(h, app_id, app_secret, TAG) {
            Ok(token) => token,
            Err(error) => {
                token_cache.invalidate();
                http = None;
                return Err(error);
            }
        };
        match send_feishu_message(h, token.as_str(), message) {
            Ok(()) => {
                record_outbound_http_success();
                Ok(())
            }
            Err(error) => {
                record_outbound_http_failure(&error);
                token_cache.invalidate();
                http = None;
                Err(error)
            }
        }
    });
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
pub(crate) struct FeishuOutboundDriver {
    app_id: String,
    app_secret: String,
    /// Kept only within one send attempt on ESP; steady-state TLS buffers must be released.
    http: Option<Box<dyn PlatformHttpClient>>,
    token_cache: FeishuTokenCache,
    create_http: Arc<dyn Fn() -> crate::Result<Box<dyn PlatformHttpClient>> + Send + Sync>,
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
pub(crate) fn feishu_outbound_driver(
    app_id: String,
    app_secret: String,
    create_http: Arc<dyn Fn() -> crate::Result<Box<dyn PlatformHttpClient>> + Send + Sync>,
) -> Box<dyn ActiveChannelSender> {
    Box::new(FeishuOutboundDriver {
        app_id,
        app_secret,
        http: None,
        token_cache: FeishuTokenCache::new(),
        create_http,
    })
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
impl ActiveChannelSender for FeishuOutboundDriver {
    fn tag(&self) -> &'static str {
        "feishu_sender"
    }

    fn send_attempt(
        &mut self,
        message: &QueuedOutboundMessage,
        attempt: u8,
    ) -> crate::error::Result<()> {
        const TAG: &str = "feishu_sender";
        let create_http = Arc::clone(&self.create_http);
        let mut create = || create_http();
        if !ensure_sender_http(&mut self.http, &mut create, TAG, attempt) {
            return Err(Error::config(TAG, "create http failed"));
        }
        let Some(h) = self.http.as_mut() else {
            return Err(Error::config(TAG, "sender http missing after ensure"));
        };
        let token = match self
            .token_cache
            .ensure_token(h, &self.app_id, &self.app_secret, TAG)
        {
            Ok(token) => token,
            Err(error) => {
                self.token_cache.invalidate();
                self.http = None;
                return Err(error);
            }
        };
        match send_feishu_message(h, token.as_str(), message) {
            Ok(()) => {
                record_outbound_http_success();
                self.http = None;
                Ok(())
            }
            Err(error) => {
                record_outbound_http_failure(&error);
                self.token_cache.invalidate();
                self.http = None;
                Err(error)
            }
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
) -> Result<Option<String>> {
    let body = CanonicalMessageBody::text(content.to_string());
    let resp_body = post_feishu_message(http, token, Some(chat_id), &body)?;
    #[derive(serde::Deserialize)]
    struct R {
        data: Option<Inner>,
    }
    #[derive(serde::Deserialize)]
    struct Inner {
        message_id: Option<String>,
    }
    let r: R = serde_json::from_slice(resp_body.as_ref()).unwrap_or(R { data: None });
    Ok(r.data.and_then(|d| d.message_id))
}

/// 编辑已发送的飞书消息（PUT /im/v1/messages/{message_id}）。
/// Rich message edit is intentionally unsupported until each msg_type has a stable patch contract.
pub fn edit_message<H: ChannelHttpClient>(
    http: &mut H,
    token: &str,
    message_id: &str,
    content: &str,
) -> Result<()> {
    let (msg_type, serialized_content) = feishu_text_body(&TextBody::plain(content))?;
    let body_bytes = build_feishu_request_body(None, &msg_type, &serialized_content)?;
    let url = format!("{FEISHU_EDIT_URL_PREFIX}{message_id}");
    let auth_val = format!("Bearer {}", token);
    let headers = [
        ("Authorization", auth_val.as_str()),
        ("Content-Type", "application/json; charset=utf-8"),
    ];
    let (status, _) = match http.http_put_with_headers(&url, &headers, &body_bytes) {
        Ok(resp) => resp,
        Err(e) => {
            let error = Error::Other {
                source: Box::new(e),
                stage: "feishu_edit",
            };
            record_outbound_http_failure(&error);
            return Err(error);
        }
    };
    if status >= 400 {
        let error = Error::Http {
            status_code: status,
            stage: "feishu_edit",
        };
        record_outbound_http_failure(&error);
        return Err(error);
    }
    record_outbound_http_success();
    Ok(())
}

#[derive(Debug)]
struct ParsedFeishuInboundMessage {
    chat_id: String,
    body: CanonicalMessageBody,
    content_projection: String,
    is_group: bool,
    message_id: String,
    event_id: String,
    inbound_dedup_key: String,
}

fn parse_feishu_content(
    message_type: &str,
    content_str: &str,
) -> Option<(CanonicalMessageBody, String)> {
    let content_json = serde_json::from_str::<Value>(content_str).ok()?;
    match message_type {
        "text" => {
            let text = content_json
                .get("text")
                .and_then(Value::as_str)?
                .trim()
                .to_string();
            if text.is_empty() {
                None
            } else {
                Some((CanonicalMessageBody::text(text.clone()), text))
            }
        }
        "post" => {
            let fallback = feishu_post_fallback_text(&content_json);
            Some((
                CanonicalMessageBody::Card(CardBody {
                    format: CardFormat::RichPost,
                    payload_json: content_json,
                    fallback_text: fallback.clone(),
                }),
                fallback,
            ))
        }
        "image" => {
            let image_key = content_json
                .get("image_key")
                .and_then(Value::as_str)?
                .trim();
            if image_key.is_empty() {
                return None;
            }
            let body = CanonicalMessageBody::Image(ImageBody {
                asset: MediaAssetRef::platform_handle(AssetSourcePlatform::Feishu, image_key),
                caption: None,
            });
            let projection = body.text_projection();
            Some((body, projection))
        }
        "file" => {
            let file_key = content_json.get("file_key").and_then(Value::as_str)?.trim();
            if file_key.is_empty() {
                return None;
            }
            let file_name = content_json
                .get("file_name")
                .and_then(Value::as_str)
                .map(ToString::to_string);
            let body = CanonicalMessageBody::File(FileBody {
                asset: MediaAssetRef {
                    source_platform: AssetSourcePlatform::Feishu,
                    locator_kind: MediaLocatorKind::PlatformHandle,
                    locator: file_key.to_string(),
                    file_name,
                    ..MediaAssetRef::default()
                },
                caption: None,
            });
            let projection = body.text_projection();
            Some((body, projection))
        }
        "audio" => {
            let file_key = content_json.get("file_key").and_then(Value::as_str)?.trim();
            if file_key.is_empty() {
                return None;
            }
            let duration_ms = content_json
                .get("duration")
                .and_then(Value::as_u64)
                .and_then(|value| u32::try_from(value).ok())
                .map(|secs| secs.saturating_mul(1000));
            let body = CanonicalMessageBody::Audio(crate::bus::AudioBody {
                asset: MediaAssetRef {
                    source_platform: AssetSourcePlatform::Feishu,
                    locator_kind: MediaLocatorKind::PlatformHandle,
                    locator: file_key.to_string(),
                    duration_ms,
                    ..MediaAssetRef::default()
                },
                caption: None,
                transcript_text: None,
            });
            let projection = body.text_projection();
            Some((body, projection))
        }
        "media" => {
            let file_key = content_json.get("file_key").and_then(Value::as_str)?.trim();
            if file_key.is_empty() {
                return None;
            }
            let image_key = content_json
                .get("image_key")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty());
            let title = content_json
                .get("file_name")
                .and_then(Value::as_str)
                .map(ToString::to_string);
            let body = CanonicalMessageBody::Video(VideoBody {
                asset: MediaAssetRef {
                    source_platform: AssetSourcePlatform::Feishu,
                    locator_kind: MediaLocatorKind::PlatformHandle,
                    locator: file_key.to_string(),
                    file_name: title.clone(),
                    ..MediaAssetRef::default()
                },
                caption: image_key
                    .map(|key| TextBody::plain(format!("[cover] {key}")))
                    .filter(|_| false),
                title,
                description: None,
            });
            let projection = body.text_projection();
            Some((body, projection))
        }
        "interactive" => {
            let fallback = feishu_interactive_fallback_text(&content_json);
            Some((
                CanonicalMessageBody::Card(CardBody {
                    format: CardFormat::Interactive,
                    payload_json: content_json,
                    fallback_text: fallback.clone(),
                }),
                fallback,
            ))
        }
        _ => None,
    }
}

fn parse_feishu_inbound_message(
    event_body: &str,
    allowed_chat_ids: &[String],
) -> Option<ParsedFeishuInboundMessage> {
    const TAG: &str = "feishu_event_parse";
    let v: Value = match serde_json::from_str(event_body) {
        Ok(x) => x,
        Err(_) => {
            log::debug!("[{}] body parse failed", TAG);
            return None;
        }
    };
    let event_type = v
        .get("header")
        .and_then(|h| h.get("event_type"))
        .and_then(Value::as_str)?;
    if event_type != "im.message.receive_v1" {
        log::debug!("[{}] skip event_type={}", TAG, event_type);
        return None;
    }
    let event_id = v
        .get("header")
        .and_then(|h| h.get("event_id"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    let message = v.get("event")?.get("message")?;
    let message_id = message
        .get("message_id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    let chat_id = message
        .get("chat_id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    if chat_id.is_empty() {
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
    let chat_type = message
        .get("chat_type")
        .and_then(Value::as_str)
        .unwrap_or("");
    let message_type = message
        .get("message_type")
        .and_then(Value::as_str)
        .unwrap_or("");
    let content_str = message.get("content").and_then(Value::as_str).unwrap_or("");
    let (body, content_projection) = parse_feishu_content(message_type, content_str)?;
    let is_group = matches!(chat_type, "group" | "topic_group");
    let inbound_dedup_key = if !message_id.is_empty() {
        format!("feishu_message:{message_id}")
    } else if !event_id.is_empty() {
        format!("feishu_event:{event_id}")
    } else {
        String::new()
    };
    Some(ParsedFeishuInboundMessage {
        chat_id,
        body,
        content_projection,
        is_group,
        message_id,
        event_id,
        inbound_dedup_key,
    })
}

/// 连通性检查：供 GET /api/channel_connectivity 使用。
pub fn check_connectivity<H: ChannelHttpClient + ?Sized>(
    config: &AppConfig,
    http: &mut H,
) -> super::super::connectivity::ChannelConnectivityItem {
    use super::super::connectivity;
    let configured =
        !config.feishu_app_id.trim().is_empty() && !config.feishu_app_secret.trim().is_empty();
    connectivity::probe_item("feishu", configured, || {
        let body = FeishuTokenRequest {
            app_id: config.feishu_app_id.trim().to_string(),
            app_secret: config.feishu_app_secret.trim().to_string(),
        };
        let body_bytes = match serde_json::to_vec(&body) {
            Ok(b) => b,
            Err(_) => return connectivity::ProbeStatus::CheckFailed,
        };
        let (status, resp_body) = match http.http_post(FEISHU_TOKEN_URL, &body_bytes) {
            Ok(r) => r,
            Err(_) => return connectivity::ProbeStatus::CheckFailed,
        };
        if status >= 400 {
            return connectivity::ProbeStatus::InvalidToken;
        }
        let r: FeishuTokenResponse = match serde_json::from_slice(resp_body.as_ref()) {
            Ok(x) => x,
            Err(_) => return connectivity::ProbeStatus::CheckFailed,
        };
        match r.tenant_access_token {
            Some(t) if !t.is_empty() => connectivity::ProbeStatus::Ok,
            _ => connectivity::ProbeStatus::InvalidToken,
        }
    })
}

/// 从飞书事件 body（schema 2.0，含 header.event_type、event）解析出 im.message.receive_v1 消息，
/// 白名单校验通过则返回 PcMsg，否则 None。供 HTTP 回调与长连接入站共用。
pub fn event_body_to_pcmsg(event_body: &str, allowed_chat_ids: &[String]) -> Option<PcMsg> {
    event_body_to_pcmsg_with_transport(event_body, allowed_chat_ids, MessageTransport::Unknown)
}

pub(crate) fn event_body_to_pcmsg_with_transport(
    event_body: &str,
    allowed_chat_ids: &[String],
    transport: MessageTransport,
) -> Option<PcMsg> {
    let parsed = parse_feishu_inbound_message(event_body, allowed_chat_ids)?;
    PcMsg::new_inbound_with_body_and_ingress(
        "feishu",
        &parsed.chat_id,
        parsed.body,
        parsed.content_projection,
        parsed.is_group,
        crate::bus::IngressKind::User,
    )
    .ok()
    .map(|msg| {
        msg.with_inbound_provenance(
            transport,
            parsed.message_id,
            parsed.event_id,
            parsed.inbound_dedup_key,
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bus::MessageBodyKind;
    use crate::platform::ResponseBody;
    use std::collections::VecDeque;

    #[derive(Default)]
    struct StubHttp {
        post_results: VecDeque<Result<(u16, ResponseBody)>>,
        put_results: VecDeque<Result<(u16, ResponseBody)>>,
        posted_bodies: Vec<Vec<u8>>,
        put_bodies: Vec<Vec<u8>>,
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

        fn http_post(&mut self, _url: &str, body: &[u8]) -> Result<(u16, ResponseBody)> {
            self.posted_bodies.push(body.to_vec());
            self.post_results
                .pop_front()
                .unwrap_or_else(|| Ok((200, ResponseBody::Heap(b"{}".to_vec()))))
        }

        fn http_post_with_headers(
            &mut self,
            _url: &str,
            _headers: &[(&str, &str)],
            body: &[u8],
        ) -> Result<(u16, ResponseBody)> {
            self.http_post("", body)
        }

        fn http_put_with_headers(
            &mut self,
            _url: &str,
            _headers: &[(&str, &str)],
            body: &[u8],
        ) -> Result<(u16, ResponseBody)> {
            self.put_bodies.push(body.to_vec());
            self.put_results
                .pop_front()
                .unwrap_or_else(|| Ok((200, ResponseBody::Heap(b"{}".to_vec()))))
        }
    }

    #[test]
    fn send_and_get_id_records_message_id() {
        let mut http = StubHttp {
            post_results: VecDeque::from([Ok((
                200,
                ResponseBody::Heap(br#"{"data":{"message_id":"om_123"}}"#.to_vec()),
            ))]),
            ..Default::default()
        };

        let message_id = send_and_get_id(&mut http, "token", "chat-1", "hello").expect("send");

        assert_eq!(message_id.as_deref(), Some("om_123"));
    }

    #[test]
    fn event_body_to_pcmsg_maps_post_to_card_body() {
        let body = serde_json::json!({
            "header": {
                "event_id": "evt-1",
                "event_type": "im.message.receive_v1"
            },
            "event": {
                "message": {
                    "message_id": "om_1",
                    "chat_id": "oc_1",
                    "chat_type": "group",
                    "message_type": "post",
                    "content": "{\"zh_cn\":{\"title\":\"标题\",\"content\":[[{\"tag\":\"text\",\"text\":\"第一行\"}],[{\"tag\":\"img\",\"image_key\":\"img_x\"}]]}}"
                }
            }
        })
        .to_string();

        let msg = event_body_to_pcmsg_with_transport(
            &body,
            &[String::from("oc_1")],
            MessageTransport::Webhook,
        )
        .expect("message");

        assert_eq!(msg.body_kind(), MessageBodyKind::Card);
        assert!(msg.content.contains("标题"));
        assert_eq!(msg.platform_message_id, "om_1");
    }

    #[test]
    fn event_body_to_pcmsg_maps_image_to_platform_handle() {
        let body = serde_json::json!({
            "header": {
                "event_id": "evt-1",
                "event_type": "im.message.receive_v1"
            },
            "event": {
                "message": {
                    "message_id": "om_2",
                    "chat_id": "oc_1",
                    "chat_type": "p2p",
                    "message_type": "image",
                    "content": "{\"image_key\":\"img_v2_123\"}"
                }
            }
        })
        .to_string();

        let msg = event_body_to_pcmsg(&body, &[String::from("oc_1")]).expect("message");

        assert_eq!(msg.body_kind(), MessageBodyKind::Image);
        match &msg.body {
            CanonicalMessageBody::Image(image) => {
                assert_eq!(image.asset.source_platform, AssetSourcePlatform::Feishu);
                assert_eq!(image.asset.locator, "img_v2_123");
            }
            other => panic!("unexpected body: {other:?}"),
        }
    }
}
