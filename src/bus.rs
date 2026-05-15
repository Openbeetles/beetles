//! 消息总线：入站/出站 channel，固定容量，背压由 `SyncSender::send` 阻塞实现。
//! Message bus: inbound/outbound channels, fixed capacity; backpressure = blocking send when full.

use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::Arc;

pub use crate::constants::{DEFAULT_CAPACITY, MAX_CONTENT_LEN};
pub use crate::util::{truncate_content_to_max, truncate_to_byte_len};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, Hash)]
#[serde(rename_all = "snake_case")]
pub enum MessageBodyKind {
    #[default]
    Text,
    Image,
    Audio,
    Video,
    File,
    Card,
    PlatformNative,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, Hash)]
#[serde(rename_all = "snake_case")]
pub enum TextFormat {
    #[default]
    Plain,
    Markdown,
    Html,
    RichText,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, Hash)]
#[serde(rename_all = "snake_case")]
pub enum AssetSourcePlatform {
    Telegram,
    Feishu,
    DingTalk,
    WeCom,
    Qq,
    Beetle,
    #[default]
    External,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, Hash)]
#[serde(rename_all = "snake_case")]
pub enum MediaLocatorKind {
    PlatformHandle,
    BeetleBlob,
    #[default]
    ExternalUrl,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, Hash)]
#[serde(rename_all = "snake_case")]
pub enum CardFormat {
    #[default]
    Interactive,
    TemplateCard,
    Ark,
    Embed,
    RichPost,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct TextBody {
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub format: TextFormat,
}

impl TextBody {
    pub fn plain(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            format: TextFormat::Plain,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct MediaAssetRef {
    #[serde(default)]
    pub source_platform: AssetSourcePlatform,
    #[serde(default)]
    pub locator_kind: MediaLocatorKind,
    #[serde(default)]
    pub locator: String,
    #[serde(default)]
    pub file_name: Option<String>,
    #[serde(default)]
    pub mime_type: Option<String>,
    #[serde(default)]
    pub size_bytes: Option<u64>,
    #[serde(default)]
    pub width_px: Option<u32>,
    #[serde(default)]
    pub height_px: Option<u32>,
    #[serde(default)]
    pub duration_ms: Option<u32>,
    #[serde(default)]
    pub ttl_seconds: Option<u32>,
}

impl MediaAssetRef {
    pub fn platform_handle(
        source_platform: AssetSourcePlatform,
        locator: impl Into<String>,
    ) -> Self {
        Self {
            source_platform,
            locator_kind: MediaLocatorKind::PlatformHandle,
            locator: locator.into(),
            ..Self::default()
        }
    }

    pub fn external_url(url: impl Into<String>) -> Self {
        Self {
            source_platform: AssetSourcePlatform::External,
            locator_kind: MediaLocatorKind::ExternalUrl,
            locator: url.into(),
            ..Self::default()
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ImageBody {
    #[serde(default)]
    pub asset: MediaAssetRef,
    #[serde(default)]
    pub caption: Option<TextBody>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct AudioBody {
    #[serde(default)]
    pub asset: MediaAssetRef,
    #[serde(default)]
    pub caption: Option<TextBody>,
    #[serde(default)]
    pub transcript_text: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct VideoBody {
    #[serde(default)]
    pub asset: MediaAssetRef,
    #[serde(default)]
    pub caption: Option<TextBody>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct FileBody {
    #[serde(default)]
    pub asset: MediaAssetRef,
    #[serde(default)]
    pub caption: Option<TextBody>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct CardBody {
    #[serde(default)]
    pub format: CardFormat,
    #[serde(default)]
    pub payload_json: Value,
    #[serde(default)]
    pub fallback_text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PlatformNativeBody {
    #[serde(default)]
    pub platform_type: String,
    #[serde(default)]
    pub payload_json: Value,
    #[serde(default)]
    pub fallback_text: String,
}

pub const PLATFORM_NATIVE_TYPE_TELEGRAM_MESSAGE_REACTION: &str = "telegram_message_reaction";
pub const PLATFORM_NATIVE_TELEGRAM_REACTION_EMOJI_KEY: &str = "emoji";

impl PlatformNativeBody {
    pub fn telegram_message_reaction(emoji: impl Into<String>) -> Self {
        let emoji = emoji.into();
        Self {
            platform_type: PLATFORM_NATIVE_TYPE_TELEGRAM_MESSAGE_REACTION.to_string(),
            payload_json: serde_json::json!({
                PLATFORM_NATIVE_TELEGRAM_REACTION_EMOJI_KEY: emoji,
            }),
            fallback_text: emoji,
        }
    }

    pub fn is_telegram_message_reaction(&self) -> bool {
        self.platform_type == PLATFORM_NATIVE_TYPE_TELEGRAM_MESSAGE_REACTION
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CanonicalMessageBody {
    Text(TextBody),
    Image(ImageBody),
    Audio(AudioBody),
    Video(VideoBody),
    File(FileBody),
    Card(CardBody),
    PlatformNative(PlatformNativeBody),
}

impl Default for CanonicalMessageBody {
    fn default() -> Self {
        Self::Text(TextBody::default())
    }
}

impl CanonicalMessageBody {
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text(TextBody::plain(text))
    }

    pub fn kind(&self) -> MessageBodyKind {
        match self {
            Self::Text(_) => MessageBodyKind::Text,
            Self::Image(_) => MessageBodyKind::Image,
            Self::Audio(_) => MessageBodyKind::Audio,
            Self::Video(_) => MessageBodyKind::Video,
            Self::File(_) => MessageBodyKind::File,
            Self::Card(_) => MessageBodyKind::Card,
            Self::PlatformNative(_) => MessageBodyKind::PlatformNative,
        }
    }

    pub fn has_media(&self) -> bool {
        matches!(
            self,
            Self::Image(_) | Self::Audio(_) | Self::Video(_) | Self::File(_)
        )
    }

    pub fn text_projection(&self) -> String {
        match self {
            Self::Text(body) => body.text.clone(),
            Self::Image(body) => media_caption_projection(
                body.caption.as_ref(),
                body.asset.file_name.as_deref(),
                "[image]",
            ),
            Self::Audio(body) => first_nonempty_projection(&[
                body.transcript_text.as_deref(),
                body.caption.as_ref().map(|caption| caption.text.as_str()),
                Some("[audio]"),
            ]),
            Self::Video(body) => {
                if let Some(caption) = body
                    .caption
                    .as_ref()
                    .filter(|caption| !caption.text.trim().is_empty())
                {
                    return caption.text.clone();
                }
                if let Some(title_or_description) =
                    join_nonempty_lines(&[body.title.as_deref(), body.description.as_deref()])
                {
                    return title_or_description;
                }
                "[video]".to_string()
            }
            Self::File(body) => media_caption_projection(
                body.caption.as_ref(),
                body.asset.file_name.as_deref(),
                "[file]",
            ),
            Self::Card(body) => fallback_projection(&body.fallback_text, "[card]"),
            Self::PlatformNative(body) => {
                fallback_projection(&body.fallback_text, "[platform_native]")
            }
        }
    }
}

fn first_nonempty_projection(candidates: &[Option<&str>]) -> String {
    candidates
        .iter()
        .flatten()
        .map(|candidate| candidate.trim())
        .find(|candidate| !candidate.is_empty())
        .unwrap_or_default()
        .to_string()
}

fn fallback_projection(fallback_text: &str, default: &str) -> String {
    let trimmed = fallback_text.trim();
    if trimmed.is_empty() {
        default.to_string()
    } else {
        trimmed.to_string()
    }
}

fn join_nonempty_lines(parts: &[Option<&str>]) -> Option<String> {
    let mut normalized = Vec::new();
    for part in parts.iter().flatten() {
        let trimmed = part.trim();
        if trimmed.is_empty() {
            continue;
        }
        normalized.push(trimmed.to_string());
    }
    if normalized.is_empty() {
        None
    } else {
        Some(normalized.join("\n"))
    }
}

fn media_caption_projection(
    caption: Option<&TextBody>,
    file_name: Option<&str>,
    default: &str,
) -> String {
    if let Some(caption) = caption.filter(|caption| !caption.text.trim().is_empty()) {
        return caption.text.clone();
    }
    if let Some(file_name) = file_name.map(str::trim).filter(|name| !name.is_empty()) {
        return format!("{default} {file_name}");
    }
    default.to_string()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MessageTransport {
    #[default]
    Unknown,
    Wss,
    Webhook,
    Poll,
    Internal,
}

impl MessageTransport {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Wss => "wss",
            Self::Webhook => "webhook",
            Self::Poll => "poll",
            Self::Internal => "internal",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum OutboundKind {
    #[default]
    Primary,
    Supplemental,
}

impl OutboundKind {
    pub fn is_supplemental(self) -> bool {
        matches!(self, Self::Supplemental)
    }
}

/// 总线消息。入队前需校验 `content.len() <= MAX_CONTENT_LEN`。可序列化供 pending_retry 持久化。
/// channel/chat_id 用 Arc<str> 减少 clone 开销。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PcMsg {
    #[serde(serialize_with = "serialize_arc_str")]
    pub channel: Arc<str>,
    #[serde(serialize_with = "serialize_arc_str")]
    pub chat_id: Arc<str>,
    pub content: String,
    /// authoritative payload；现阶段 `content` 只是兼容 text projection。
    #[serde(default)]
    pub body: CanonicalMessageBody,
    /// 平台侧线程/话题 ID（若有）。
    #[serde(default)]
    pub platform_thread_id: String,
    /// 请求关联 ID：用于贯通 agent -> dispatch -> sender 的端到端时延日志。
    #[serde(default)]
    pub req_id: Option<String>,
    /// 出站消息类别：primary 保留 canonical reply 语义，supplemental 为 best-effort 附加可见性。
    #[serde(default)]
    pub outbound_kind: OutboundKind,
    /// 入站来源：用于调度与指标分流；默认 user（兼容历史持久化消息）。
    #[serde(default)]
    pub ingress: IngressKind,
    /// 消息入队时间（Unix ms）；用于排队等待时延与 cron 端到端时延基线。
    #[serde(default = "current_unix_ms")]
    pub enqueue_ts_ms: u64,
    /// 入站 transport 来源，仅用于 provenance / dedup / audit。
    #[serde(default)]
    pub source_transport: MessageTransport,
    /// 平台侧消息 ID（若有）。
    #[serde(default)]
    pub platform_message_id: String,
    /// 平台侧事件 ID（若有）。
    #[serde(default)]
    pub platform_event_id: String,
    /// 入站去重键（若有）。
    #[serde(default)]
    pub inbound_dedup_key: String,
    /// 是否来自群组（group/supergroup）；用于 system 注入与 SILENT 约定。
    pub is_group: bool,
}

#[derive(Deserialize)]
struct RawPcMsg {
    channel: String,
    chat_id: String,
    #[serde(default)]
    content: String,
    #[serde(default)]
    body: Option<CanonicalMessageBody>,
    #[serde(default)]
    platform_thread_id: String,
    #[serde(default)]
    req_id: Option<String>,
    #[serde(default)]
    outbound_kind: OutboundKind,
    #[serde(default)]
    ingress: IngressKind,
    #[serde(default = "current_unix_ms")]
    enqueue_ts_ms: u64,
    #[serde(default)]
    source_transport: MessageTransport,
    #[serde(default)]
    platform_message_id: String,
    #[serde(default)]
    platform_event_id: String,
    #[serde(default)]
    inbound_dedup_key: String,
    #[serde(default)]
    is_group: bool,
}

impl<'de> Deserialize<'de> for PcMsg {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = RawPcMsg::deserialize(deserializer)?;
        let body = raw
            .body
            .unwrap_or_else(|| CanonicalMessageBody::text(raw.content.clone()));
        let content = if raw.content.trim().is_empty() {
            body.text_projection()
        } else {
            raw.content
        };
        Ok(Self {
            channel: Arc::from(raw.channel.as_str()),
            chat_id: Arc::from(raw.chat_id.as_str()),
            content,
            body,
            platform_thread_id: raw.platform_thread_id,
            req_id: raw.req_id,
            outbound_kind: raw.outbound_kind,
            ingress: raw.ingress,
            enqueue_ts_ms: raw.enqueue_ts_ms,
            source_transport: raw.source_transport,
            platform_message_id: raw.platform_message_id,
            platform_event_id: raw.platform_event_id,
            inbound_dedup_key: raw.inbound_dedup_key,
            is_group: raw.is_group,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum IngressKind {
    #[default]
    User,
    System,
}

fn current_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis().min(u64::MAX as u128) as u64)
        .unwrap_or(0)
}

fn serialize_arc_str<S>(arc: &Arc<str>, serializer: S) -> std::result::Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_str(arc)
}

impl PcMsg {
    fn validate_content_len(stage: &'static str, content: &str) -> Result<()> {
        if content.len() > MAX_CONTENT_LEN {
            return Err(Error::config(
                stage,
                format!(
                    "content length {} exceeds max {}",
                    content.len(),
                    MAX_CONTENT_LEN
                ),
            ));
        }
        Ok(())
    }

    fn build(
        channel: impl Into<String>,
        chat_id: impl Into<String>,
        content: String,
        body: CanonicalMessageBody,
        is_group: bool,
        ingress: IngressKind,
    ) -> Result<Self> {
        Self::validate_content_len("PcMsg::build", &content)?;
        Ok(PcMsg {
            channel: Arc::from(channel.into().as_str()),
            chat_id: Arc::from(chat_id.into().as_str()),
            content,
            body,
            platform_thread_id: String::new(),
            req_id: None,
            outbound_kind: OutboundKind::Primary,
            ingress,
            enqueue_ts_ms: current_unix_ms(),
            source_transport: MessageTransport::Unknown,
            platform_message_id: String::new(),
            platform_event_id: String::new(),
            inbound_dedup_key: String::new(),
            is_group,
        })
    }

    /// 构造并校验 content 长度，超限返回 `Error::Config`。出站消息 is_group 恒为 false。
    pub fn new(
        channel: impl Into<String>,
        chat_id: impl Into<String>,
        content: impl Into<String>,
    ) -> Result<Self> {
        Self::new_inbound_with_ingress(channel, chat_id, content, false, IngressKind::User)
    }

    /// 系统入站消息构造（cron/remind/heartbeat 等），默认非群消息。
    pub fn new_system(
        channel: impl Into<String>,
        chat_id: impl Into<String>,
        content: impl Into<String>,
    ) -> Result<Self> {
        let mut msg =
            Self::new_inbound_with_ingress(channel, chat_id, content, false, IngressKind::System)?;
        msg.source_transport = MessageTransport::Internal;
        Ok(msg)
    }

    /// 入站消息用；与 `new` 相同但可指定 is_group（群聊/话题群为 true）。
    pub fn new_inbound(
        channel: impl Into<String>,
        chat_id: impl Into<String>,
        content: impl Into<String>,
        is_group: bool,
    ) -> Result<Self> {
        Self::new_inbound_with_ingress(channel, chat_id, content, is_group, IngressKind::User)
    }

    /// 入站消息构造（可显式指定 ingress）。
    pub fn new_inbound_with_ingress(
        channel: impl Into<String>,
        chat_id: impl Into<String>,
        content: impl Into<String>,
        is_group: bool,
        ingress: IngressKind,
    ) -> Result<Self> {
        let content = content.into();
        Self::new_inbound_with_body_and_ingress(
            channel,
            chat_id,
            CanonicalMessageBody::text(content.clone()),
            content,
            is_group,
            ingress,
        )
    }

    pub fn new_inbound_with_body(
        channel: impl Into<String>,
        chat_id: impl Into<String>,
        body: CanonicalMessageBody,
        is_group: bool,
    ) -> Result<Self> {
        Self::new_inbound_with_body_and_ingress(
            channel,
            chat_id,
            body.clone(),
            body.text_projection(),
            is_group,
            IngressKind::User,
        )
    }

    pub fn new_inbound_with_body_and_ingress(
        channel: impl Into<String>,
        chat_id: impl Into<String>,
        body: CanonicalMessageBody,
        content_projection: impl Into<String>,
        is_group: bool,
        ingress: IngressKind,
    ) -> Result<Self> {
        Self::build(
            channel,
            chat_id,
            content_projection.into(),
            body,
            is_group,
            ingress,
        )
    }

    /// 当前会话出站消息构造：保留 channel/chat_id/is_group，并显式设置 req_id。
    pub fn new_outbound_for_chat(
        channel: &Arc<str>,
        chat_id: &Arc<str>,
        content: impl Into<String>,
        req_id: Option<String>,
        is_group: bool,
    ) -> Result<Self> {
        let content = content.into();
        Self::validate_content_len("PcMsg::new_outbound_for_chat", &content)?;
        Self::new_outbound_for_chat_with_body(
            channel,
            chat_id,
            CanonicalMessageBody::text(content.clone()),
            content,
            req_id,
            is_group,
        )
    }

    pub fn new_outbound_for_chat_with_body(
        channel: &Arc<str>,
        chat_id: &Arc<str>,
        body: CanonicalMessageBody,
        content_projection: impl Into<String>,
        req_id: Option<String>,
        is_group: bool,
    ) -> Result<Self> {
        let content_projection = content_projection.into();
        Self::validate_content_len(
            "PcMsg::new_outbound_for_chat_with_body",
            &content_projection,
        )?;
        let mut msg = Self::build(
            Arc::clone(channel).to_string(),
            Arc::clone(chat_id).to_string(),
            content_projection,
            body,
            is_group,
            IngressKind::User,
        )?;
        msg.req_id = req_id;
        Ok(msg)
    }

    /// 基于当前入站消息构造回给同一会话的出站消息，保留群聊语义与 req_id。
    pub fn new_outbound_reply_to(source: &PcMsg, content: impl Into<String>) -> Result<Self> {
        let content = content.into();
        let mut reply = Self::new_outbound_for_chat_with_body(
            &source.channel,
            &source.chat_id,
            CanonicalMessageBody::text(content.clone()),
            content,
            source.req_id.clone(),
            source.is_group,
        )?;
        reply.copy_inbound_provenance_from(source);
        Ok(reply)
    }

    pub fn new_outbound_reply_to_with_body(
        source: &PcMsg,
        body: CanonicalMessageBody,
    ) -> Result<Self> {
        let mut reply = Self::new_outbound_for_chat_with_body(
            &source.channel,
            &source.chat_id,
            body.clone(),
            body.text_projection(),
            source.req_id.clone(),
            source.is_group,
        )?;
        reply.copy_inbound_provenance_from(source);
        Ok(reply)
    }

    /// 基于当前入站消息构造回给同一会话的出站消息，允许显式指定 authoritative body
    /// 与兼容 text projection（通常使用 canonical final reply）。
    pub fn new_outbound_reply_to_with_body_projection(
        source: &PcMsg,
        body: CanonicalMessageBody,
        content_projection: impl Into<String>,
    ) -> Result<Self> {
        let mut reply = Self::new_outbound_for_chat_with_body(
            &source.channel,
            &source.chat_id,
            body,
            content_projection.into(),
            source.req_id.clone(),
            source.is_group,
        )?;
        reply.copy_inbound_provenance_from(source);
        Ok(reply)
    }

    pub fn with_inbound_provenance(
        mut self,
        source_transport: MessageTransport,
        platform_message_id: impl Into<String>,
        platform_event_id: impl Into<String>,
        inbound_dedup_key: impl Into<String>,
    ) -> Self {
        self.source_transport = source_transport;
        self.platform_message_id = platform_message_id.into();
        self.platform_event_id = platform_event_id.into();
        self.inbound_dedup_key = inbound_dedup_key.into();
        self
    }

    pub fn with_outbound_kind(mut self, outbound_kind: OutboundKind) -> Self {
        self.outbound_kind = outbound_kind;
        self
    }

    pub fn with_req_id(mut self, req_id: impl Into<Option<String>>) -> Self {
        self.req_id = req_id.into();
        self
    }

    pub fn with_platform_thread_id(mut self, platform_thread_id: impl Into<String>) -> Self {
        self.platform_thread_id = platform_thread_id.into();
        self
    }

    pub fn body_kind(&self) -> MessageBodyKind {
        self.body.kind()
    }

    pub fn has_media_body(&self) -> bool {
        self.body.has_media()
    }

    fn copy_inbound_provenance_from(&mut self, source: &PcMsg) {
        self.source_transport = source.source_transport;
        self.platform_message_id = source.platform_message_id.clone();
        self.platform_event_id = source.platform_event_id.clone();
        self.inbound_dedup_key = source.inbound_dedup_key.clone();
        self.platform_thread_id = source.platform_thread_id.clone();
    }
}

/// 带深度计数的发送端，send/try_send 成功时递增，供 health 查询。
pub struct TrackedSender<T> {
    inner: SyncSender<T>,
    depth: Arc<AtomicUsize>,
}

impl<T> TrackedSender<T> {
    /// 仅当 send 成功时递增深度。
    pub fn send(&self, t: T) -> std::result::Result<(), mpsc::SendError<T>> {
        let result = self.inner.send(t);
        if result.is_ok() {
            self.depth.fetch_add(1, Ordering::Relaxed);
        }
        result
    }

    /// 非阻塞发送；队列满时返回 Err(TrySendError::Full(t))。
    pub fn try_send(&self, t: T) -> std::result::Result<(), mpsc::TrySendError<T>> {
        let result = self.inner.try_send(t);
        if result.is_ok() {
            self.depth.fetch_add(1, Ordering::Relaxed);
        }
        result
    }
}

impl<T> Clone for TrackedSender<T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            depth: Arc::clone(&self.depth),
        }
    }
}

/// 带深度计数的接收端，recv/recv_timeout 成功时递减。
pub struct TrackedReceiver<T> {
    inner: Receiver<T>,
    depth: Arc<AtomicUsize>,
}

impl<T> TrackedReceiver<T> {
    pub fn recv(&self) -> std::result::Result<T, mpsc::RecvError> {
        let result = self.inner.recv();
        if result.is_ok() {
            self.depth.fetch_sub(1, Ordering::Relaxed);
        }
        result
    }

    /// 带超时的接收；超时返回 Err(RecvTimeoutError::Timeout)。
    pub fn recv_timeout(
        &self,
        timeout: std::time::Duration,
    ) -> std::result::Result<T, mpsc::RecvTimeoutError> {
        let result = self.inner.recv_timeout(timeout);
        if result.is_ok() {
            self.depth.fetch_sub(1, Ordering::Relaxed);
        }
        result
    }

    /// 非阻塞接收；队列空返回 Err(TryRecvError::Empty)。
    pub fn try_recv(&self) -> std::result::Result<T, mpsc::TryRecvError> {
        let result = self.inner.try_recv();
        if result.is_ok() {
            self.depth.fetch_sub(1, Ordering::Relaxed);
        }
        result
    }
}

pub type InboundTx = TrackedSender<PcMsg>;
pub type OutboundTx = TrackedSender<PcMsg>;
pub type InboundRx = TrackedReceiver<PcMsg>;
pub type OutboundRx = TrackedReceiver<PcMsg>;
pub type UserInboundTx = InboundTx;
pub type UserInboundRx = InboundRx;
pub type SystemInboundTx = InboundTx;
pub type SystemInboundRx = InboundRx;

/// 独立创建一个 PcMsg 入站队列（用于 user/system 双队列拆分）。
pub fn new_inbound_channel(capacity: usize) -> (InboundTx, InboundRx, Arc<AtomicUsize>) {
    let (tx, rx) = mpsc::sync_channel(capacity);
    let depth = Arc::new(AtomicUsize::new(0));
    let depth_rx = Arc::clone(&depth);
    (
        TrackedSender {
            inner: tx,
            depth: Arc::clone(&depth),
        },
        TrackedReceiver {
            inner: rx,
            depth: depth_rx,
        },
        depth,
    )
}

/// 消息总线：main 唯一创建；通道侧持 `inbound_tx` 推入站，dispatch 持 `outbound_rx` 取出站。
/// 背压：队满时 `send()` 阻塞，直至有空间。深度由 Arc<AtomicUsize> 暴露供 health 使用。
pub struct MessageBus {
    pub inbound_tx: InboundTx,
    pub outbound_tx: OutboundTx,
    pub inbound_depth: Arc<AtomicUsize>,
    pub outbound_depth: Arc<AtomicUsize>,
}

impl MessageBus {
    /// 创建入站/出站 channel，容量均为 `capacity`。返回 (bus, inbound_rx, outbound_rx)。
    pub fn new(capacity: usize) -> (Self, InboundRx, OutboundRx) {
        let (inbound_tx, inbound_rx) = mpsc::sync_channel(capacity);
        let (outbound_tx, outbound_rx) = mpsc::sync_channel(capacity);
        let inbound_depth = Arc::new(AtomicUsize::new(0));
        let outbound_depth = Arc::new(AtomicUsize::new(0));
        let inbound_depth_rx = Arc::clone(&inbound_depth);
        let outbound_depth_rx = Arc::clone(&outbound_depth);
        (
            MessageBus {
                inbound_tx: TrackedSender {
                    inner: inbound_tx,
                    depth: Arc::clone(&inbound_depth),
                },
                outbound_tx: TrackedSender {
                    inner: outbound_tx,
                    depth: Arc::clone(&outbound_depth),
                },
                inbound_depth,
                outbound_depth,
            },
            TrackedReceiver {
                inner: inbound_rx,
                depth: inbound_depth_rx,
            },
            TrackedReceiver {
                inner: outbound_rx,
                depth: outbound_depth_rx,
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outbound_reply_to_preserves_chat_metadata() {
        let mut inbound = PcMsg::new_inbound("qq_channel", "group:chat-1", "hello", true)
            .expect("inbound message");
        inbound.req_id = Some("req-1".to_string());
        inbound.source_transport = MessageTransport::Wss;
        inbound.platform_message_id = "msg-1".to_string();
        inbound.platform_event_id = "evt-1".to_string();
        inbound.inbound_dedup_key = "qq_message:msg-1".to_string();

        let outbound = PcMsg::new_outbound_reply_to(&inbound, "world").expect("outbound reply");

        assert_eq!(outbound.channel.as_ref(), "qq_channel");
        assert_eq!(outbound.chat_id.as_ref(), "group:chat-1");
        assert_eq!(outbound.content, "world");
        assert_eq!(outbound.req_id.as_deref(), Some("req-1"));
        assert_eq!(outbound.ingress, IngressKind::User);
        assert!(outbound.is_group);
        assert_eq!(outbound.source_transport, MessageTransport::Wss);
        assert_eq!(outbound.platform_message_id, "msg-1");
        assert_eq!(outbound.platform_event_id, "evt-1");
        assert_eq!(outbound.inbound_dedup_key, "qq_message:msg-1");
    }

    #[test]
    fn outbound_reply_to_with_body_projection_keeps_canonical_content() {
        let inbound =
            PcMsg::new_inbound("feishu", "chat-1", "hello", false).expect("inbound message");
        let outbound = PcMsg::new_outbound_reply_to_with_body_projection(
            &inbound,
            CanonicalMessageBody::Card(CardBody {
                format: CardFormat::Interactive,
                payload_json: serde_json::json!({"header":{"title":"Build passed"}}),
                fallback_text: String::new(),
            }),
            "构建已通过",
        )
        .expect("outbound reply");

        assert_eq!(outbound.content, "构建已通过");
        assert!(matches!(outbound.body, CanonicalMessageBody::Card(_)));
    }

    #[test]
    fn outbound_for_chat_validates_content_len() {
        let channel: Arc<str> = Arc::from("telegram");
        let chat_id: Arc<str> = Arc::from("chat-1");
        let too_long = "x".repeat(MAX_CONTENT_LEN + 1);

        let err = PcMsg::new_outbound_for_chat(
            &channel,
            &chat_id,
            too_long,
            Some("req-1".to_string()),
            false,
        )
        .expect_err("should reject oversized outbound content");

        assert_eq!(err.stage(), "PcMsg::new_outbound_for_chat");
    }
}
