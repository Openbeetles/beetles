//! 消息总线：入站/出站 channel，固定容量，背压由 `SyncSender::send` 阻塞实现。
//! Message bus: inbound/outbound channels, fixed capacity; backpressure = blocking send when full.

use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::hash_map::DefaultHasher;
use std::fmt::Write as _;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU32, AtomicU8, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

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
    Visibility,
    Supplemental,
}

impl OutboundKind {
    pub fn is_supplemental(self) -> bool {
        matches!(self, Self::Supplemental | Self::Visibility)
    }

    pub fn is_ordinary_supplemental(self) -> bool {
        matches!(self, Self::Supplemental)
    }

    pub fn is_visibility(self) -> bool {
        matches!(self, Self::Visibility)
    }

    pub fn runtime_work_class(self) -> crate::runtime::RuntimeWorkClass {
        match self {
            Self::Primary => crate::runtime::RuntimeWorkClass::PrimaryReplyDelivery,
            Self::Visibility => crate::runtime::RuntimeWorkClass::VisibilityDelivery,
            Self::Supplemental => crate::runtime::RuntimeWorkClass::SupplementalDelivery,
        }
    }

    pub fn runtime_work_source(self) -> crate::runtime::RuntimeWorkSource {
        match self {
            Self::Primary | Self::Visibility => crate::runtime::RuntimeWorkSource::UserFacing,
            Self::Supplemental => crate::runtime::RuntimeWorkSource::Background,
        }
    }

    pub fn is_best_effort_delivery(self) -> bool {
        self.runtime_work_class() == crate::runtime::RuntimeWorkClass::SupplementalDelivery
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Primary => "primary",
            Self::Visibility => "visibility",
            Self::Supplemental => "supplemental",
        }
    }
}

const INGRESS_VISIBILITY_ACK_PENDING: u8 = 0;
const INGRESS_VISIBILITY_ACK_CLAIMING: u8 = 1;
const INGRESS_VISIBILITY_ACK_SENT: u8 = 2;
const INGRESS_VISIBILITY_ACK_FAILED: u8 = 3;

/// Transient claim that an accepted user ingress already owns the first visibility ack.
/// It is intentionally not serialized; pending-retry replay falls back to normal agent ack.
#[derive(Clone, Debug, Default)]
pub struct IngressVisibilityAckClaim {
    state: Option<Arc<AtomicU8>>,
}

impl PartialEq for IngressVisibilityAckClaim {
    fn eq(&self, other: &Self) -> bool {
        self.status_for_eq() == other.status_for_eq()
    }
}

impl Eq for IngressVisibilityAckClaim {}

impl IngressVisibilityAckClaim {
    fn pending() -> Self {
        Self {
            state: Some(Arc::new(AtomicU8::new(INGRESS_VISIBILITY_ACK_PENDING))),
        }
    }

    fn status_for_eq(&self) -> Option<u8> {
        self.state
            .as_ref()
            .map(|state| state.load(Ordering::Relaxed))
    }

    fn is_disabled(&self) -> bool {
        self.state.is_none()
    }

    fn mark_claiming(&self) {
        if let Some(state) = self.state.as_ref() {
            state.store(INGRESS_VISIBILITY_ACK_CLAIMING, Ordering::Release);
        }
    }

    fn mark_sent(&self) {
        if let Some(state) = self.state.as_ref() {
            state.store(INGRESS_VISIBILITY_ACK_SENT, Ordering::Release);
        }
    }

    fn mark_failed(&self) {
        if let Some(state) = self.state.as_ref() {
            state.store(INGRESS_VISIBILITY_ACK_FAILED, Ordering::Release);
        }
    }

    fn suppress_agent_ack(&self) -> bool {
        self.state
            .as_ref()
            .is_some_and(|state| state.load(Ordering::Acquire) == INGRESS_VISIBILITY_ACK_SENT)
    }

    fn settle_before_agent_ack(&self, max_wait: Duration) {
        let Some(state) = self.state.as_ref() else {
            return;
        };
        let deadline = Instant::now() + max_wait;
        while matches!(
            state.load(Ordering::Acquire),
            INGRESS_VISIBILITY_ACK_PENDING | INGRESS_VISIBILITY_ACK_CLAIMING
        ) {
            if Instant::now() >= deadline {
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
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
    /// 出站消息类别：primary 为 canonical reply，visibility 为当前 turn 活性证明，
    /// supplemental 为 best-effort 附加可见性。
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
    /// 入站接受层是否已经承接首个可见 ack；瞬态运行态，不进入 pending retry。
    #[serde(skip)]
    #[doc(hidden)]
    pub ingress_visibility_ack: IngressVisibilityAckClaim,
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
            ingress_visibility_ack: IngressVisibilityAckClaim::default(),
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

static REQ_SEQ: AtomicU32 = AtomicU32::new(1);

fn next_req_id(channel: &str, chat_id: &str) -> String {
    let seq = REQ_SEQ.fetch_add(1, Ordering::Relaxed);
    let ts_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let mut hasher = DefaultHasher::new();
    channel.hash(&mut hasher);
    chat_id.hash(&mut hasher);
    let short = (hasher.finish() & 0xffff) as u16;
    let mut s = String::with_capacity(40);
    let _ = write!(&mut s, "r{}-{}-{:04x}", ts_ms, seq, short);
    s
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
            ingress_visibility_ack: IngressVisibilityAckClaim::default(),
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

    pub fn ensure_req_id(&mut self) -> &str {
        if self.req_id.is_none() {
            self.req_id = Some(next_req_id(&self.channel, &self.chat_id));
        }
        self.req_id.as_deref().unwrap_or_default()
    }

    fn ensure_ingress_visibility_ack_claim(&mut self) -> IngressVisibilityAckClaim {
        if self.ingress_visibility_ack.is_disabled() {
            self.ingress_visibility_ack = IngressVisibilityAckClaim::pending();
        }
        self.ingress_visibility_ack.clone()
    }

    pub(crate) fn suppress_agent_ack_for_ingress_claim(&self) -> bool {
        self.ingress_visibility_ack.suppress_agent_ack()
    }

    pub(crate) fn settle_ingress_visibility_ack_claim(&self, max_wait: Duration) {
        self.ingress_visibility_ack
            .settle_before_agent_ack(max_wait);
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

    /// Whether this message represents a user-originated inbound event from an external surface.
    pub fn is_external_user_ingress(&self) -> bool {
        self.ingress == IngressKind::User && self.source_transport != MessageTransport::Internal
    }

    pub fn runtime_foreground_source(&self) -> Option<crate::runtime::RuntimeForegroundSource> {
        if self.ingress != IngressKind::User {
            return None;
        }
        match self.channel.as_ref() {
            crate::chat_stream::CHANNEL_CONFIGURE_UI_CHAT => {
                Some(crate::runtime::RuntimeForegroundSource::ConfigUiChat)
            }
            crate::channel_capability::CHANNEL_VOICE => {
                Some(crate::runtime::RuntimeForegroundSource::VoiceFallbackInteraction)
            }
            _ => Some(crate::runtime::RuntimeForegroundSource::ExternalUserMessage),
        }
    }
}

/// 带深度计数的发送端，send/try_send 成功时递增，供 health 查询。
type SuccessfulSendHook = Arc<dyn Fn() + Send + Sync>;

pub struct TrackedSender<T> {
    inner: SyncSender<T>,
    depth: Arc<AtomicUsize>,
    capacity: usize,
    after_successful_send: Arc<Mutex<Option<SuccessfulSendHook>>>,
}

impl<T> TrackedSender<T> {
    fn notify_successful_send(&self) {
        let hook = self
            .after_successful_send
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone();
        if let Some(hook) = hook {
            hook();
        }
    }

    /// 仅当 send 成功时递增深度。
    pub fn send(&self, t: T) -> std::result::Result<(), mpsc::SendError<T>> {
        let result = self.inner.send(t);
        if result.is_ok() {
            self.depth.fetch_add(1, Ordering::Relaxed);
            self.notify_successful_send();
        }
        result
    }

    /// 非阻塞发送；队列满时返回 Err(TrySendError::Full(t))。
    pub fn try_send(&self, t: T) -> std::result::Result<(), mpsc::TrySendError<T>> {
        let result = self.inner.try_send(t);
        if result.is_ok() {
            self.depth.fetch_add(1, Ordering::Relaxed);
            self.notify_successful_send();
        }
        result
    }

    pub fn set_after_successful_send_hook(&self, hook: SuccessfulSendHook) {
        *self
            .after_successful_send
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(hook);
    }

    pub fn queued_len(&self) -> usize {
        self.depth.load(Ordering::Relaxed)
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn remaining_capacity(&self) -> usize {
        self.capacity.saturating_sub(self.queued_len())
    }
}

impl<T> Clone for TrackedSender<T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            depth: Arc::clone(&self.depth),
            capacity: self.capacity,
            after_successful_send: Arc::clone(&self.after_successful_send),
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
pub type UserInboundRx = InboundRx;
pub type SystemInboundRx = InboundRx;

/// User inbound queue sender. Successful user submissions renew runtime foreground.
type AcceptedUserIngressHook = Arc<dyn Fn(PcMsg) -> bool + Send + Sync>;

#[derive(Clone)]
pub struct UserInboundTx {
    inner: InboundTx,
    after_accepted_user_ingress: Arc<Mutex<Option<AcceptedUserIngressHook>>>,
}

impl UserInboundTx {
    pub fn new(inner: InboundTx) -> Self {
        Self {
            inner,
            after_accepted_user_ingress: Arc::new(Mutex::new(None)),
        }
    }

    pub fn set_after_accepted_user_ingress_hook<F>(&self, hook: F)
    where
        F: Fn(PcMsg) -> bool + Send + Sync + 'static,
    {
        *self
            .after_accepted_user_ingress
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(Arc::new(hook));
    }

    pub fn set_after_successful_send_hook<F>(&self, hook: F)
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.inner.set_after_successful_send_hook(Arc::new(hook));
    }

    fn accepted_user_ingress_hook(&self) -> Option<AcceptedUserIngressHook> {
        self.after_accepted_user_ingress
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }

    fn prepare_accepted_user_ingress_observer(
        &self,
        msg: &mut PcMsg,
        source: crate::runtime::RuntimeForegroundSource,
    ) -> Option<(AcceptedUserIngressHook, PcMsg, IngressVisibilityAckClaim)> {
        if msg.ingress != IngressKind::User
            || source != crate::runtime::RuntimeForegroundSource::ExternalUserMessage
        {
            return None;
        }
        let hook = self.accepted_user_ingress_hook()?;
        msg.ensure_req_id();
        let ack_claim = msg.ensure_ingress_visibility_ack_claim();
        Some((hook, msg.clone(), ack_claim))
    }

    fn notify_accepted_user_ingress(
        observer: Option<(AcceptedUserIngressHook, PcMsg, IngressVisibilityAckClaim)>,
    ) {
        let Some((hook, msg, ack_claim)) = observer else {
            return;
        };
        ack_claim.mark_claiming();
        if hook(msg) {
            ack_claim.mark_sent();
        } else {
            ack_claim.mark_failed();
        }
    }

    #[allow(clippy::result_large_err)]
    pub fn send_user(
        &self,
        mut msg: PcMsg,
        source: crate::runtime::RuntimeForegroundSource,
    ) -> std::result::Result<(), mpsc::SendError<PcMsg>> {
        let should_renew = msg.ingress == IngressKind::User;
        if should_renew {
            msg.ensure_req_id();
        }
        let observer = self.prepare_accepted_user_ingress_observer(&mut msg, source);
        let result = self.inner.send(msg);
        if result.is_ok() && should_renew {
            crate::runtime::renew_runtime_foreground_now(source);
        }
        if result.is_ok() {
            Self::notify_accepted_user_ingress(observer);
        }
        result
    }

    #[allow(clippy::result_large_err)]
    pub fn try_submit_user(
        &self,
        mut msg: PcMsg,
        source: crate::runtime::RuntimeForegroundSource,
    ) -> std::result::Result<(), mpsc::TrySendError<PcMsg>> {
        let should_renew = msg.ingress == IngressKind::User;
        if should_renew {
            msg.ensure_req_id();
        }
        let observer = self.prepare_accepted_user_ingress_observer(&mut msg, source);
        let result = self.inner.try_send(msg);
        if should_renew && !matches!(result, Err(mpsc::TrySendError::Disconnected(_))) {
            crate::runtime::renew_runtime_foreground_now(source);
        }
        if result.is_ok() {
            Self::notify_accepted_user_ingress(observer);
        }
        result
    }

    /// Requeue an already-accounted user turn without extending runtime foreground.
    #[allow(clippy::result_large_err)]
    pub fn try_resubmit_user_without_foreground_renewal(
        &self,
        msg: PcMsg,
    ) -> std::result::Result<(), mpsc::TrySendError<PcMsg>> {
        self.inner.try_send(msg)
    }

    #[cfg(test)]
    #[allow(clippy::result_large_err)]
    pub fn try_submit_user_at(
        &self,
        mut msg: PcMsg,
        source: crate::runtime::RuntimeForegroundSource,
        now_ms: u64,
    ) -> std::result::Result<(), mpsc::TrySendError<PcMsg>> {
        let should_renew = msg.ingress == IngressKind::User;
        if should_renew {
            msg.ensure_req_id();
        }
        let observer = self.prepare_accepted_user_ingress_observer(&mut msg, source);
        let result = self.inner.try_send(msg);
        if should_renew && !matches!(result, Err(mpsc::TrySendError::Disconnected(_))) {
            crate::runtime::renew_runtime_foreground(source, now_ms);
        }
        if result.is_ok() {
            Self::notify_accepted_user_ingress(observer);
        }
        result
    }

    pub fn queued_len(&self) -> usize {
        self.inner.queued_len()
    }

    pub fn capacity(&self) -> usize {
        self.inner.capacity()
    }

    pub fn remaining_capacity(&self) -> usize {
        self.inner.remaining_capacity()
    }
}

/// System inbound queue sender. It never renews runtime foreground.
#[derive(Clone)]
pub struct SystemInboundTx {
    inner: InboundTx,
}

impl SystemInboundTx {
    pub fn new(inner: InboundTx) -> Self {
        Self { inner }
    }

    pub fn set_after_successful_send_hook<F>(&self, hook: F)
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.inner.set_after_successful_send_hook(Arc::new(hook));
    }

    #[allow(clippy::result_large_err)]
    pub fn send(&self, msg: PcMsg) -> std::result::Result<(), mpsc::SendError<PcMsg>> {
        self.inner.send(msg)
    }

    #[allow(clippy::result_large_err)]
    pub fn try_send(&self, msg: PcMsg) -> std::result::Result<(), mpsc::TrySendError<PcMsg>> {
        self.inner.try_send(msg)
    }

    pub fn queued_len(&self) -> usize {
        self.inner.queued_len()
    }

    pub fn capacity(&self) -> usize {
        self.inner.capacity()
    }

    pub fn remaining_capacity(&self) -> usize {
        self.inner.remaining_capacity()
    }
}

/// 独立创建一个 PcMsg 入站队列（用于 user/system 双队列拆分）。
pub fn new_inbound_channel(capacity: usize) -> (InboundTx, InboundRx, Arc<AtomicUsize>) {
    let (tx, rx) = mpsc::sync_channel(capacity);
    let depth = Arc::new(AtomicUsize::new(0));
    let depth_rx = Arc::clone(&depth);
    (
        TrackedSender {
            inner: tx,
            depth: Arc::clone(&depth),
            capacity,
            after_successful_send: Arc::new(Mutex::new(None)),
        },
        TrackedReceiver {
            inner: rx,
            depth: depth_rx,
        },
        depth,
    )
}

pub fn new_user_inbound_channel(
    capacity: usize,
) -> (UserInboundTx, UserInboundRx, Arc<AtomicUsize>) {
    let (tx, rx, depth) = new_inbound_channel(capacity);
    (UserInboundTx::new(tx), rx, depth)
}

pub fn new_system_inbound_channel(
    capacity: usize,
) -> (SystemInboundTx, SystemInboundRx, Arc<AtomicUsize>) {
    let (tx, rx, depth) = new_inbound_channel(capacity);
    (SystemInboundTx::new(tx), rx, depth)
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
                    capacity,
                    after_successful_send: Arc::new(Mutex::new(None)),
                },
                outbound_tx: TrackedSender {
                    inner: outbound_tx,
                    depth: Arc::clone(&outbound_depth),
                    capacity,
                    after_successful_send: Arc::new(Mutex::new(None)),
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

    #[test]
    fn outbound_sender_success_hook_runs_for_shared_clones() {
        let (bus, _inbound_rx, outbound_rx) = MessageBus::new(1);
        let calls = Arc::new(AtomicUsize::new(0));
        let hook_calls = Arc::clone(&calls);

        bus.outbound_tx
            .set_after_successful_send_hook(Arc::new(move || {
                hook_calls.fetch_add(1, Ordering::Relaxed);
            }));

        let tx_clone = bus.outbound_tx.clone();
        tx_clone
            .try_send(PcMsg::new("qq_channel", "chat-1", "first").expect("first"))
            .expect("first enqueue");

        assert_eq!(calls.load(Ordering::Relaxed), 1);
        assert_eq!(outbound_rx.try_recv().expect("first recv").content, "first");

        bus.outbound_tx
            .try_send(PcMsg::new("qq_channel", "chat-1", "second").expect("second"))
            .expect("second enqueue");
        let full = bus
            .outbound_tx
            .try_send(PcMsg::new("qq_channel", "chat-1", "third").expect("third"));

        assert!(matches!(full, Err(mpsc::TrySendError::Full(_))));
        assert_eq!(
            calls.load(Ordering::Relaxed),
            2,
            "failed sends must not wake lazy outbound execution"
        );
    }

    #[test]
    fn user_and_system_inbound_success_hooks_run() {
        let (user_tx, user_rx, _) = new_user_inbound_channel(2);
        let user_calls = Arc::new(AtomicUsize::new(0));
        let user_hook_calls = Arc::clone(&user_calls);
        user_tx.set_after_successful_send_hook(move || {
            user_hook_calls.fetch_add(1, Ordering::Relaxed);
        });

        user_tx
            .try_submit_user(
                PcMsg::new_inbound("voice", "device", "hi", false).expect("user msg"),
                crate::runtime::RuntimeForegroundSource::VoiceFallbackInteraction,
            )
            .expect("user enqueue");
        assert_eq!(user_calls.load(Ordering::Relaxed), 1);
        assert_eq!(user_rx.try_recv().expect("user recv").content, "hi");

        let (system_tx, system_rx, _) = new_system_inbound_channel(2);
        let system_calls = Arc::new(AtomicUsize::new(0));
        let system_hook_calls = Arc::clone(&system_calls);
        system_tx.set_after_successful_send_hook(move || {
            system_hook_calls.fetch_add(1, Ordering::Relaxed);
        });

        system_tx
            .try_send(PcMsg::new("system", "device", "tick").expect("system msg"))
            .expect("system enqueue");
        assert_eq!(system_calls.load(Ordering::Relaxed), 1);
        assert_eq!(system_rx.try_recv().expect("system recv").content, "tick");
    }

    #[test]
    fn outbound_kind_maps_to_runtime_delivery_work_class() {
        assert_eq!(
            OutboundKind::Primary.runtime_work_class(),
            crate::runtime::RuntimeWorkClass::PrimaryReplyDelivery
        );
        assert_eq!(
            OutboundKind::Visibility.runtime_work_class(),
            crate::runtime::RuntimeWorkClass::VisibilityDelivery
        );
        assert_eq!(
            OutboundKind::Supplemental.runtime_work_class(),
            crate::runtime::RuntimeWorkClass::SupplementalDelivery
        );
        assert_eq!(
            OutboundKind::Visibility.runtime_work_source(),
            crate::runtime::RuntimeWorkSource::UserFacing
        );
        assert_eq!(
            OutboundKind::Supplemental.runtime_work_source(),
            crate::runtime::RuntimeWorkSource::Background
        );
        assert!(OutboundKind::Visibility.is_supplemental());
        assert!(!OutboundKind::Visibility.is_ordinary_supplemental());
        assert!(!OutboundKind::Visibility.is_best_effort_delivery());
        assert!(OutboundKind::Supplemental.is_best_effort_delivery());
    }

    #[test]
    fn successful_user_submission_renews_runtime_foreground() {
        let _guard = crate::runtime::foreground::runtime_foreground_test_guard();
        crate::runtime::foreground::reset_runtime_foreground_for_tests();
        let (tx, _rx, _) = new_user_inbound_channel(2);
        let msg = PcMsg::new_inbound("qq_channel", "chat-1", "hello", false)
            .expect("user message")
            .with_inbound_provenance(MessageTransport::Wss, "m1", "e1", "k1");
        let now_ms = 1_000_000_000_000;

        tx.try_submit_user_at(
            msg,
            crate::runtime::RuntimeForegroundSource::ExternalUserMessage,
            now_ms,
        )
        .expect("enqueue user message");

        let snapshot = crate::runtime::foreground::runtime_foreground_snapshot_at(now_ms + 1);
        assert!(snapshot.active);
        assert_eq!(
            snapshot.primary_source,
            Some(crate::runtime::RuntimeForegroundSource::ExternalUserMessage)
        );
    }

    #[test]
    fn accepted_user_ingress_hook_claims_visibility_ack_after_successful_enqueue() {
        let _guard = crate::runtime::foreground::runtime_foreground_test_guard();
        crate::runtime::foreground::reset_runtime_foreground_for_tests();
        let (tx, rx, _) = new_user_inbound_channel(1);
        let calls = Arc::new(AtomicUsize::new(0));
        let hook_calls = Arc::clone(&calls);
        tx.set_after_accepted_user_ingress_hook(move |msg| {
            assert_eq!(msg.channel.as_ref(), "qq_channel");
            assert!(msg.req_id.is_some());
            hook_calls.fetch_add(1, Ordering::Relaxed);
            true
        });
        let msg = PcMsg::new_inbound("qq_channel", "chat-1", "hello", false)
            .expect("user message")
            .with_inbound_provenance(MessageTransport::Wss, "m1", "e1", "k1");

        tx.try_submit_user(
            msg,
            crate::runtime::RuntimeForegroundSource::ExternalUserMessage,
        )
        .expect("accepted user message");

        let queued = rx.try_recv().expect("queued user message");
        assert!(queued.req_id.is_some());
        assert!(queued.suppress_agent_ack_for_ingress_claim());
        assert_eq!(calls.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn full_user_queue_renews_runtime_foreground_without_record_growth() {
        let _guard = crate::runtime::foreground::runtime_foreground_test_guard();
        crate::runtime::foreground::reset_runtime_foreground_for_tests();
        let (tx, _rx, _) = new_user_inbound_channel(1);
        let now_ms = 1_000_000_000_000;
        tx.try_submit_user_at(
            PcMsg::new_inbound("qq_channel", "chat-1", "first", false).expect("first"),
            crate::runtime::RuntimeForegroundSource::ExternalUserMessage,
            now_ms,
        )
        .expect("fill user queue");

        let rejected = tx.try_submit_user_at(
            PcMsg::new_inbound("qq_channel", "chat-1", "second", false).expect("second"),
            crate::runtime::RuntimeForegroundSource::ExternalUserMessage,
            now_ms + 1_000,
        );

        assert!(matches!(
            rejected,
            Err(std::sync::mpsc::TrySendError::Full(_))
        ));
        let snapshot = crate::runtime::foreground::runtime_foreground_snapshot_at(now_ms + 1_001);
        assert_eq!(snapshot.active_count, 1);
        assert_eq!(snapshot.records.len(), 1);
        assert_eq!(snapshot.age_ms, Some(1));
    }

    #[test]
    fn delayed_user_replay_requeues_without_renewing_runtime_foreground() {
        let _guard = crate::runtime::foreground::runtime_foreground_test_guard();
        crate::runtime::foreground::reset_runtime_foreground_for_tests();
        let (tx, _rx, _) = new_user_inbound_channel(2);
        let now_ms = 1_000_000_000_000;
        tx.try_submit_user_at(
            PcMsg::new_inbound("qq_channel", "chat-1", "first", false).expect("first"),
            crate::runtime::RuntimeForegroundSource::ExternalUserMessage,
            now_ms,
        )
        .expect("initial user message");

        tx.try_resubmit_user_without_foreground_renewal(
            PcMsg::new_inbound("qq_channel", "chat-1", "replay", false).expect("replay"),
        )
        .expect("delayed replay should requeue");

        let snapshot = crate::runtime::foreground::runtime_foreground_snapshot_at(now_ms + 1_001);
        assert_eq!(snapshot.active_count, 1);
        assert_eq!(snapshot.records.len(), 1);
        assert_eq!(snapshot.age_ms, Some(1_001));
        assert_eq!(snapshot.resume_after_ms, Some(28_999));
    }

    #[test]
    fn system_sender_never_renews_runtime_foreground() {
        let _guard = crate::runtime::foreground::runtime_foreground_test_guard();
        crate::runtime::foreground::reset_runtime_foreground_for_tests();
        let (tx, _rx, _) = new_system_inbound_channel(2);
        let msg = PcMsg::new_system("heartbeat", "system", "tick").expect("system message");

        tx.try_send(msg).expect("enqueue system message");

        assert!(!crate::runtime::runtime_foreground_active(1_000));
    }
}
