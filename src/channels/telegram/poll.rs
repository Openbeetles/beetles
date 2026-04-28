//! Telegram 入站 long poll：getUpdates，解析消息入队，命令处理。

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::sync::RwLock;

use crate::bus::{
    AssetSourcePlatform, AudioBody, CanonicalMessageBody, FileBody, ImageBody, InboundTx,
    MediaAssetRef, MessageTransport, OutboundTx, PcMsg, TextBody, VideoBody, MAX_CONTENT_LEN,
};
use crate::channels::inbound_backpressure::{self, EventIngressSource, InboundBackpressureOutcome};
use crate::channels::ChannelHttpClient;
use crate::error::{Error, Result};
use crate::i18n::{tr, Locale as UiLocale, Message as UiMessage};
use crate::memory::{PendingRetryStore, SessionStore};

use super::send::set_message_reaction;

const TAG_POLL: &str = "telegram";

/// Telegram 群聊激活策略写入回调，由 main 注入，避免 poll 层知道持久化后端。
pub type TelegramGroupActivationSetter = Box<dyn Fn(&str) -> Result<()> + Send>;

/// Telegram 控制命令（/activation、/session clear、/status）执行所需的上下文，由 main 传入轮询线程。
pub struct TelegramCommandCtx {
    pub outbound_tx: OutboundTx,
    pub session_store: Arc<dyn SessionStore + Send + Sync>,
    pub inbound_depth: Arc<std::sync::atomic::AtomicUsize>,
    pub outbound_depth: Arc<std::sync::atomic::AtomicUsize>,
    pub set_group_activation: TelegramGroupActivationSetter,
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

#[derive(serde::Deserialize)]
struct TelegramUpdates {
    result: Option<Vec<TelegramUpdate>>,
}

#[derive(serde::Deserialize)]
struct TelegramUpdate {
    update_id: i64,
    message: Option<TelegramMessage>,
}

#[derive(serde::Deserialize)]
struct TelegramMessage {
    chat: TelegramChat,
    #[serde(default)]
    message_id: i64,
    #[serde(default)]
    message_thread_id: Option<i64>,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    entities: Option<Vec<MessageEntity>>,
    #[serde(default)]
    caption: Option<String>,
    #[serde(default)]
    caption_entities: Option<Vec<MessageEntity>>,
    #[serde(default)]
    photo: Option<Vec<TelegramPhotoSize>>,
    #[serde(default)]
    audio: Option<TelegramAudio>,
    #[serde(default)]
    voice: Option<TelegramVoice>,
    #[serde(default)]
    video: Option<TelegramVideo>,
    #[serde(default)]
    document: Option<TelegramDocument>,
}

#[derive(serde::Deserialize)]
struct TelegramChat {
    id: i64,
    #[serde(rename = "type")]
    type_: Option<String>,
}

#[derive(serde::Deserialize)]
struct MessageEntity {
    #[serde(rename = "type")]
    type_: String,
    offset: Option<i32>,
    length: Option<i32>,
}

#[derive(Clone, serde::Deserialize)]
struct TelegramPhotoSize {
    file_id: String,
    #[serde(default)]
    width: Option<u32>,
    #[serde(default)]
    height: Option<u32>,
    #[serde(default)]
    file_size: Option<u64>,
}

#[derive(Clone, serde::Deserialize)]
struct TelegramAudio {
    file_id: String,
    #[serde(default)]
    duration: Option<u32>,
    #[serde(default)]
    file_name: Option<String>,
    #[serde(default)]
    mime_type: Option<String>,
    #[serde(default)]
    file_size: Option<u64>,
}

#[derive(Clone, serde::Deserialize)]
struct TelegramVoice {
    file_id: String,
    #[serde(default)]
    duration: Option<u32>,
    #[serde(default)]
    mime_type: Option<String>,
    #[serde(default)]
    file_size: Option<u64>,
}

#[derive(Clone, serde::Deserialize)]
struct TelegramVideo {
    file_id: String,
    #[serde(default)]
    width: Option<u32>,
    #[serde(default)]
    height: Option<u32>,
    #[serde(default)]
    duration: Option<u32>,
    #[serde(default)]
    file_name: Option<String>,
    #[serde(default)]
    mime_type: Option<String>,
    #[serde(default)]
    file_size: Option<u64>,
}

#[derive(Clone, serde::Deserialize)]
struct TelegramDocument {
    file_id: String,
    #[serde(default)]
    file_name: Option<String>,
    #[serde(default)]
    mime_type: Option<String>,
    #[serde(default)]
    file_size: Option<u64>,
}

fn caption_body(caption: Option<&str>) -> Option<TextBody> {
    caption
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(TextBody::plain)
}

fn duration_ms(duration_secs: Option<u32>) -> Option<u32> {
    duration_secs.map(|duration| duration.saturating_mul(1000))
}

fn richest_photo_asset(photo_sizes: &[TelegramPhotoSize]) -> Option<MediaAssetRef> {
    photo_sizes
        .iter()
        .max_by_key(|photo| {
            let width = u64::from(photo.width.unwrap_or(0));
            let height = u64::from(photo.height.unwrap_or(0));
            (width.saturating_mul(height), photo.file_size.unwrap_or(0))
        })
        .map(|photo| MediaAssetRef {
            source_platform: AssetSourcePlatform::Telegram,
            locator_kind: crate::bus::MediaLocatorKind::PlatformHandle,
            locator: photo.file_id.clone(),
            width_px: photo.width,
            height_px: photo.height,
            size_bytes: photo.file_size,
            ..MediaAssetRef::default()
        })
}

fn parse_telegram_body(message: &TelegramMessage) -> Option<CanonicalMessageBody> {
    if let Some(text) = message
        .text
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty())
    {
        let projection = if text.len() > MAX_CONTENT_LEN {
            text.chars().take(MAX_CONTENT_LEN).collect::<String>()
        } else {
            text.to_string()
        };
        return Some(CanonicalMessageBody::text(projection));
    }

    if let Some(photo_sizes) = message.photo.as_deref() {
        let asset = richest_photo_asset(photo_sizes)?;
        return Some(CanonicalMessageBody::Image(ImageBody {
            asset,
            caption: caption_body(message.caption.as_deref()),
        }));
    }

    if let Some(audio) = message.audio.as_ref() {
        return Some(CanonicalMessageBody::Audio(AudioBody {
            asset: MediaAssetRef {
                source_platform: AssetSourcePlatform::Telegram,
                locator_kind: crate::bus::MediaLocatorKind::PlatformHandle,
                locator: audio.file_id.clone(),
                file_name: audio.file_name.clone(),
                mime_type: audio.mime_type.clone(),
                size_bytes: audio.file_size,
                duration_ms: duration_ms(audio.duration),
                ..MediaAssetRef::default()
            },
            caption: caption_body(message.caption.as_deref()),
            transcript_text: None,
        }));
    }

    if let Some(voice) = message.voice.as_ref() {
        return Some(CanonicalMessageBody::Audio(AudioBody {
            asset: MediaAssetRef {
                source_platform: AssetSourcePlatform::Telegram,
                locator_kind: crate::bus::MediaLocatorKind::PlatformHandle,
                locator: voice.file_id.clone(),
                mime_type: voice
                    .mime_type
                    .clone()
                    .or_else(|| Some("audio/ogg".to_string())),
                size_bytes: voice.file_size,
                duration_ms: duration_ms(voice.duration),
                ..MediaAssetRef::default()
            },
            caption: caption_body(message.caption.as_deref()),
            transcript_text: None,
        }));
    }

    if let Some(video) = message.video.as_ref() {
        return Some(CanonicalMessageBody::Video(VideoBody {
            asset: MediaAssetRef {
                source_platform: AssetSourcePlatform::Telegram,
                locator_kind: crate::bus::MediaLocatorKind::PlatformHandle,
                locator: video.file_id.clone(),
                file_name: video.file_name.clone(),
                mime_type: video.mime_type.clone(),
                size_bytes: video.file_size,
                width_px: video.width,
                height_px: video.height,
                duration_ms: duration_ms(video.duration),
                ..MediaAssetRef::default()
            },
            caption: caption_body(message.caption.as_deref()),
            title: None,
            description: None,
        }));
    }

    if let Some(document) = message.document.as_ref() {
        return Some(CanonicalMessageBody::File(FileBody {
            asset: MediaAssetRef {
                source_platform: AssetSourcePlatform::Telegram,
                locator_kind: crate::bus::MediaLocatorKind::PlatformHandle,
                locator: document.file_id.clone(),
                file_name: document.file_name.clone(),
                mime_type: document.mime_type.clone(),
                size_bytes: document.file_size,
                ..MediaAssetRef::default()
            },
            caption: caption_body(message.caption.as_deref()),
        }));
    }

    None
}

fn message_mentions_bot(
    text: &str,
    entities: Option<&[MessageEntity]>,
    bot_username: &str,
) -> bool {
    if bot_username.is_empty() {
        return false;
    }
    let mention = format!("@{}", bot_username);
    if text.contains(&mention) {
        return true;
    }
    let Some(entities) = entities else {
        return false;
    };
    for e in entities {
        if e.type_ != "mention" {
            continue;
        }
        let (off, len) = match (e.offset, e.length) {
            (Some(o), Some(l)) if o >= 0 && l > 0 => (o as usize, l as usize),
            _ => continue,
        };
        if let Some(slice) = text.get(off..off.saturating_add(len)) {
            if slice.eq_ignore_ascii_case(&mention) {
                return true;
            }
        }
    }
    false
}

const TELEGRAM_API_BASE: &str = "https://api.telegram.org/bot";

fn clear_telegram_webhook<H: ChannelHttpClient>(http: &mut H, token: &str) -> Result<()> {
    let url = format!(
        "{}{}/deleteWebhook?drop_pending_updates=false",
        TELEGRAM_API_BASE, token
    );
    let (status, _) = http
        .http_get(&url)
        .map_err(|e| map_stage(e, "telegram_delete_webhook"))?;
    if status >= 400 {
        return Err(Error::Http {
            status_code: status,
            stage: "telegram_delete_webhook",
        });
    }
    Ok(())
}

/// 轮询一次 getUpdates，解析消息并推入 inbound_tx；失败返回 Err 带 stage，调用方退避。
/// NOTE: 保留参数显式传递，避免把状态收敛到全局可变对象；待后续仅提取参数对象时再移除 allow。
#[allow(clippy::too_many_arguments)]
pub fn poll_telegram_once<H: ChannelHttpClient>(
    http: &mut H,
    token: &str,
    offset: Option<i64>,
    inbound_tx: &InboundTx,
    pending_retry: &dyn PendingRetryStore,
    allowed_chat_ids: &[String],
    group_activation: &str,
    bot_username: Option<&str>,
    cmd_ctx: Option<&TelegramCommandCtx>,
    resolve_locale: &std::sync::Arc<dyn Fn() -> UiLocale + Send + Sync>,
) -> Result<Option<i64>> {
    let loc = resolve_locale();
    let url = format!(
        "{}{}/getUpdates?timeout=5{}",
        TELEGRAM_API_BASE,
        token,
        offset.map(|o| format!("&offset={}", o)).unwrap_or_default()
    );
    let (status, body) = http
        .http_get(&url)
        .map_err(|e| map_stage(e, "telegram_poll"))?;
    if status >= 400 {
        return Err(Error::Http {
            status_code: status,
            stage: "telegram_poll",
        });
    }
    let updates: TelegramUpdates =
        serde_json::from_slice(body.as_ref()).map_err(|e| Error::Other {
            source: Box::new(e),
            stage: "telegram_parse",
        })?;
    let mut next_offset = offset;
    for u in updates.result.unwrap_or_default() {
        next_offset = Some(u.update_id + 1);
        if let Some(msg) = u.message {
            let chat_id = msg.chat.id.to_string();
            if allowed_chat_ids.is_empty() {
                log::warn!(
                    "[{}] rejected chat_id={} (allowlist empty). {}",
                    TAG_POLL,
                    chat_id,
                    tr(UiMessage::BindHintEmpty, loc)
                );
                continue;
            }
            if !allowed_chat_ids.iter().any(|id| id == &chat_id) {
                log::warn!(
                    "[{}] rejected chat_id={} (not in allowlist). {} Example: ...{}",
                    TAG_POLL,
                    chat_id,
                    tr(UiMessage::BindHintNotInList, loc),
                    chat_id
                );
                continue;
            }
            let gating_text = msg
                .text
                .as_deref()
                .or(msg.caption.as_deref())
                .unwrap_or_default();
            if gating_text.is_empty() && parse_telegram_body(&msg).is_none() {
                continue;
            }
            let is_group = msg
                .chat
                .type_
                .as_deref()
                .is_some_and(|t| t == "group" || t == "supergroup");
            if is_group && group_activation == "mention" {
                let mentioned = message_mentions_bot(
                    gating_text,
                    msg.entities.as_deref().or(msg.caption_entities.as_deref()),
                    bot_username.unwrap_or(""),
                );
                if !mentioned {
                    continue;
                }
            }
            if let Some(ctx) = cmd_ctx {
                let loc_cmd = resolve_locale();
                if msg
                    .text
                    .as_deref()
                    .is_some_and(|text| text.starts_with('/'))
                {
                    let text = msg.text.as_deref().unwrap_or_default();
                    let parts: Vec<&str> = text.split_whitespace().collect();
                    let handled = match parts.as_slice() {
                        ["/activation", "mention"] => {
                            if let Err(e) = (ctx.set_group_activation)("mention") {
                                log::warn!("[{}] set_group_activation: {}", TAG_POLL, e);
                            }
                            let _ = PcMsg::new(
                                "telegram",
                                &chat_id,
                                tr(UiMessage::TgActivationMention, loc_cmd),
                            )
                            .map(|m| ctx.outbound_tx.send(m));
                            true
                        }
                        ["/activation", "always"] => {
                            if let Err(e) = (ctx.set_group_activation)("always") {
                                log::warn!("[{}] set_group_activation: {}", TAG_POLL, e);
                            }
                            let _ = PcMsg::new(
                                "telegram",
                                &chat_id,
                                tr(UiMessage::TgActivationAlways, loc_cmd),
                            )
                            .map(|m| ctx.outbound_tx.send(m));
                            true
                        }
                        ["/session", "clear"] => {
                            if let Err(e) = ctx.session_store.clear(&chat_id) {
                                log::warn!("[{}] session clear: {}", TAG_POLL, e);
                            }
                            let _ = PcMsg::new(
                                "telegram",
                                &chat_id,
                                tr(UiMessage::TgSessionCleared, loc_cmd),
                            )
                            .map(|m| ctx.outbound_tx.send(m));
                            true
                        }
                        ["/status"] => {
                            let inc = ctx.inbound_depth.load(Ordering::Relaxed);
                            let out = ctx.outbound_depth.load(Ordering::Relaxed);
                            let status = tr(
                                UiMessage::TelegramStatus {
                                    wifi_connected: crate::state::wifi_sta_connected(),
                                    inbound: inc,
                                    outbound: out,
                                },
                                loc_cmd,
                            );
                            let _ = PcMsg::new("telegram", &chat_id, status)
                                .map(|m| ctx.outbound_tx.send(m));
                            true
                        }
                        _ => false,
                    };
                    if handled {
                        continue;
                    }
                }
            }
            let Some(body) = parse_telegram_body(&msg) else {
                continue;
            };
            let _ = set_message_reaction(http, token, &chat_id, msg.message_id, "👍");
            let pc = match PcMsg::new_inbound_with_body("telegram", &chat_id, body, is_group) {
                Ok(message) => message.with_inbound_provenance(
                    MessageTransport::Poll,
                    msg.message_id.to_string(),
                    "",
                    format!("telegram_message:{}", msg.message_id),
                ),
                Err(error) => {
                    log::warn!(
                        "[{}] failed to build telegram inbound message chat_id={}: {}",
                        TAG_POLL,
                        chat_id,
                        error
                    );
                    continue;
                }
            }
            .with_platform_thread_id(
                msg.message_thread_id
                    .map(|thread_id| thread_id.to_string())
                    .unwrap_or_default(),
            );
            let mut enqueued = false;
            let mut disconnected = false;
            for _ in 0..3 {
                match inbound_tx.try_send(pc.clone()) {
                    Ok(()) => {
                        inbound_backpressure::record_enqueued(EventIngressSource::TelegramPoll);
                        enqueued = true;
                        break;
                    }
                    Err(std::sync::mpsc::TrySendError::Full(_)) => {
                        std::thread::sleep(std::time::Duration::from_millis(200));
                        continue;
                    }
                    Err(std::sync::mpsc::TrySendError::Disconnected(_)) => {
                        disconnected = true;
                        log::warn!(
                            "[{}] inbound_tx closed while enqueueing telegram msg",
                            TAG_POLL
                        );
                        break;
                    }
                }
            }
            if disconnected {
                inbound_backpressure::record_disconnected_drop_for_source(
                    EventIngressSource::TelegramPoll,
                );
            } else if !enqueued {
                log::warn!(
                    "[{}] inbound queue full, saved telegram msg to pending retry chat_id={}",
                    TAG_POLL,
                    chat_id
                );
                inbound_backpressure::record_queue_full_for_source(
                    EventIngressSource::TelegramPoll,
                    InboundBackpressureOutcome::DeferredToPendingRetry,
                );
                let _ = pending_retry.save_pending_retry(&pc);
            }
        }
    }
    Ok(next_offset)
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::*;
    use crate::bus::{new_inbound_channel, MessageBodyKind, MessageBus};
    use crate::memory::SessionMessage;
    use crate::platform::ResponseBody;
    use std::collections::VecDeque;

    #[derive(Default)]
    struct StubHttp {
        get_results: VecDeque<Result<(u16, ResponseBody)>>,
        post_results: VecDeque<Result<(u16, ResponseBody)>>,
        get_urls: Vec<String>,
    }

    impl ChannelHttpClient for StubHttp {
        fn http_get(&mut self, url: &str) -> Result<(u16, ResponseBody)> {
            self.get_urls.push(url.to_string());
            self.get_results
                .pop_front()
                .unwrap_or_else(|| Ok((200, ResponseBody::Heap(br#"{"result":[]}"#.to_vec()))))
        }

        fn http_get_with_headers(
            &mut self,
            _url: &str,
            _headers: &[(&str, &str)],
        ) -> Result<(u16, ResponseBody)> {
            self.http_get(_url)
        }

        fn http_post(&mut self, _url: &str, _body: &[u8]) -> Result<(u16, ResponseBody)> {
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

    #[test]
    fn telegram_delete_webhook_before_polling_uses_official_switch_back_endpoint() {
        let mut http = StubHttp {
            get_results: VecDeque::from([Ok((
                200,
                ResponseBody::Heap(br#"{"ok":true,"result":true}"#.to_vec()),
            ))]),
            ..Default::default()
        };

        clear_telegram_webhook(&mut http, "token").expect("clear webhook");

        assert_eq!(
            http.get_urls,
            ["https://api.telegram.org/bottoken/deleteWebhook?drop_pending_updates=false"]
        );
    }

    #[derive(Default)]
    struct StubPendingRetryStore;

    impl PendingRetryStore for StubPendingRetryStore {
        fn save_pending_retry(&self, _msg: &PcMsg) -> Result<()> {
            Ok(())
        }

        fn load_pending_retry(&self) -> Result<Option<PcMsg>> {
            Ok(None)
        }

        fn clear_pending_retry(&self) -> Result<()> {
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubSessionStore;

    impl SessionStore for StubSessionStore {
        fn append(&self, _chat_id: &str, _role: &str, _content: &str) -> Result<()> {
            Ok(())
        }

        fn load_recent(&self, _chat_id: &str, _n: usize) -> Result<Vec<SessionMessage>> {
            Ok(Vec::new())
        }

        fn clear(&self, _chat_id: &str) -> Result<()> {
            Ok(())
        }

        fn list_chat_ids(&self) -> Result<Vec<String>> {
            Ok(Vec::new())
        }
    }

    fn poll_single_update(body: serde_json::Value) -> PcMsg {
        let (inbound_tx, inbound_rx, _) = new_inbound_channel(4);
        let mut http = StubHttp {
            get_results: VecDeque::from([Ok((
                200,
                ResponseBody::Heap(body.to_string().into_bytes()),
            ))]),
            ..Default::default()
        };
        let pending_retry = StubPendingRetryStore;
        let resolve_locale: std::sync::Arc<dyn Fn() -> UiLocale + Send + Sync> =
            std::sync::Arc::new(|| UiLocale::Zh);

        let next_offset = poll_telegram_once(
            &mut http,
            "token",
            None,
            &inbound_tx,
            &pending_retry,
            &[String::from("1234")],
            "always",
            Some("beetle_bot"),
            None,
            &resolve_locale,
        )
        .expect("poll ok");

        assert_eq!(next_offset, Some(2));
        inbound_rx.try_recv().expect("inbound message")
    }

    #[test]
    fn poll_telegram_once_activation_command_uses_injected_setter() {
        let (inbound_tx, _inbound_rx, inbound_depth) = new_inbound_channel(4);
        let (bus, _bus_inbound_rx, outbound_rx) = MessageBus::new(4);
        let mut http = StubHttp {
            get_results: VecDeque::from([Ok((
                200,
                ResponseBody::Heap(
                    serde_json::json!({
                        "result": [{
                            "update_id": 1,
                            "message": {
                                "message_id": 9,
                                "chat": {"id": 1234, "type": "private"},
                                "text": "/activation always"
                            }
                        }]
                    })
                    .to_string()
                    .into_bytes(),
                ),
            ))]),
            ..Default::default()
        };
        let pending_retry = StubPendingRetryStore;
        let saved = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let saved_for_setter = std::sync::Arc::clone(&saved);
        let resolve_locale: std::sync::Arc<dyn Fn() -> UiLocale + Send + Sync> =
            std::sync::Arc::new(|| UiLocale::Zh);
        let cmd_ctx = TelegramCommandCtx {
            outbound_tx: bus.outbound_tx,
            session_store: std::sync::Arc::new(StubSessionStore),
            inbound_depth,
            outbound_depth: bus.outbound_depth,
            set_group_activation: Box::new(move |value| {
                saved_for_setter
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .push(value.to_string());
                Ok(())
            }),
        };

        let next_offset = poll_telegram_once(
            &mut http,
            "token",
            None,
            &inbound_tx,
            &pending_retry,
            &[String::from("1234")],
            "mention",
            Some("beetle_bot"),
            Some(&cmd_ctx),
            &resolve_locale,
        )
        .expect("poll ok");

        assert_eq!(next_offset, Some(2));
        assert_eq!(
            saved
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .clone(),
            vec![String::from("always")]
        );
        let ack = outbound_rx.try_recv().expect("activation ack");
        assert_eq!(ack.channel.as_ref(), "telegram");
        assert_eq!(ack.chat_id.as_ref(), "1234");
    }

    #[test]
    fn poll_telegram_once_maps_photo_update_to_image_body() {
        let msg = poll_single_update(serde_json::json!({
            "result": [{
                "update_id": 1,
                "message": {
                    "message_id": 9,
                    "chat": {"id": 1234, "type": "private"},
                    "caption": "photo caption",
                    "photo": [
                        {"file_id": "small", "width": 100, "height": 100, "file_size": 10},
                        {"file_id": "large", "width": 800, "height": 600, "file_size": 20}
                    ]
                }
            }]
        }));

        assert_eq!(msg.body_kind(), MessageBodyKind::Image);
        assert_eq!(msg.content, "photo caption");
        match &msg.body {
            CanonicalMessageBody::Image(image) => {
                assert_eq!(image.asset.locator, "large");
                assert_eq!(image.asset.width_px, Some(800));
                assert_eq!(
                    image.caption.as_ref().map(|caption| caption.text.as_str()),
                    Some("photo caption")
                );
            }
            other => panic!("unexpected body: {other:?}"),
        }
        assert_eq!(msg.platform_message_id, "9");
    }

    #[test]
    fn poll_telegram_once_maps_voice_update_to_audio_body() {
        let msg = poll_single_update(serde_json::json!({
            "result": [{
                "update_id": 1,
                "message": {
                    "message_id": 10,
                    "chat": {"id": 1234, "type": "private"},
                    "voice": {
                        "file_id": "voice_1",
                        "duration": 2,
                        "file_size": 99
                    }
                }
            }]
        }));

        assert_eq!(msg.body_kind(), MessageBodyKind::Audio);
        assert_eq!(msg.content, "[audio]");
        match &msg.body {
            CanonicalMessageBody::Audio(audio) => {
                assert_eq!(audio.asset.locator, "voice_1");
                assert_eq!(audio.asset.duration_ms, Some(2000));
                assert_eq!(audio.asset.mime_type.as_deref(), Some("audio/ogg"));
            }
            other => panic!("unexpected body: {other:?}"),
        }
    }

    #[test]
    fn poll_telegram_once_maps_document_update_to_file_body() {
        let msg = poll_single_update(serde_json::json!({
            "result": [{
                "update_id": 1,
                "message": {
                    "message_id": 11,
                    "message_thread_id": 88,
                    "chat": {"id": 1234, "type": "supergroup"},
                    "caption": "doc caption",
                    "document": {
                        "file_id": "doc_1",
                        "file_name": "report.pdf",
                        "mime_type": "application/pdf",
                        "file_size": 123
                    }
                }
            }]
        }));

        assert_eq!(msg.body_kind(), MessageBodyKind::File);
        assert_eq!(msg.platform_thread_id, "88");
        match &msg.body {
            CanonicalMessageBody::File(file) => {
                assert_eq!(file.asset.locator, "doc_1");
                assert_eq!(file.asset.file_name.as_deref(), Some("report.pdf"));
                assert_eq!(
                    file.caption.as_ref().map(|caption| caption.text.as_str()),
                    Some("doc caption")
                );
            }
            other => panic!("unexpected body: {other:?}"),
        }
    }
}

/// 启动 Telegram 长轮询循环（阻塞，应在独立线程调用）。
/// 内部通过 create_http 工厂创建 HTTP 客户端，执行轮询循环。
#[allow(clippy::too_many_arguments)]
pub fn run_telegram_poll_loop<H, F>(
    token: String,
    allowed_chat_ids: Vec<String>,
    group_activation: Arc<RwLock<String>>,
    inbound_tx: InboundTx,
    pending_retry: Arc<dyn PendingRetryStore + Send + Sync>,
    outbound_tx: OutboundTx,
    session_store: Arc<dyn SessionStore + Send + Sync>,
    inbound_depth: Arc<std::sync::atomic::AtomicUsize>,
    outbound_depth: Arc<std::sync::atomic::AtomicUsize>,
    set_group_activation: TelegramGroupActivationSetter,
    resolve_locale: Arc<dyn Fn() -> UiLocale + Send + Sync>,
    mut create_http: F,
) where
    H: ChannelHttpClient,
    F: FnMut() -> Result<H>,
{
    const TAG_TG: &str = "telegram_poll";

    let cmd_ctx = TelegramCommandCtx {
        outbound_tx,
        session_store,
        inbound_depth,
        outbound_depth,
        set_group_activation,
    };

    let mut http = match create_http() {
        Ok(h) => h,
        Err(e) => {
            log::warn!("[{}] create_http failed: {}", TAG_TG, e);
            return;
        }
    };

    let mut webhook_cleared = false;
    let mut bot_username: Option<String> = None;
    let mut offset: Option<i64> = None;
    const POLL_INTERVAL_SECS: u64 = 5;
    const BACKOFF_SECS: u64 = 30;

    loop {
        if !webhook_cleared {
            match clear_telegram_webhook(&mut http, &token) {
                Ok(()) => {
                    webhook_cleared = true;
                    bot_username = match super::send::get_bot_username(&mut http, &token) {
                        Ok(Some(u)) => Some(u),
                        _ => None,
                    };
                }
                Err(e) => {
                    log::warn!(
                        "[{}] deleteWebhook before getUpdates failed, retrying in {}s: {}",
                        TAG_TG,
                        BACKOFF_SECS,
                        e
                    );
                    ChannelHttpClient::reset_connection_for_retry(&mut http);
                    std::thread::sleep(std::time::Duration::from_secs(BACKOFF_SECS));
                    continue;
                }
            }
        }
        let current_group_activation = group_activation
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .clone();
        match poll_telegram_once(
            &mut http,
            &token,
            offset,
            &inbound_tx,
            pending_retry.as_ref(),
            &allowed_chat_ids,
            &current_group_activation,
            bot_username.as_deref(),
            Some(&cmd_ctx),
            &resolve_locale,
        ) {
            Ok(next) => offset = next,
            Err(e) => {
                log::warn!("[{}] poll failed: {}, backoff {}s", TAG_TG, e, BACKOFF_SECS);
                ChannelHttpClient::reset_connection_for_retry(&mut http);
                webhook_cleared = false;
                std::thread::sleep(std::time::Duration::from_secs(BACKOFF_SECS));
            }
        }
        std::thread::sleep(std::time::Duration::from_secs(POLL_INTERVAL_SECS));
    }
}
