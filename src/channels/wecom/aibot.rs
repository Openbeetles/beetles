//! WeCom AI Bot long-connection protocol.
//! 企业微信智能机器人长连接协议：订阅、入站 callback、同连接出站命令。

use crate::bus::{
    AssetSourcePlatform, AudioBody, CanonicalMessageBody, FileBody, ImageBody, InboundTx,
    MediaAssetRef, MessageTransport, PcMsg, TextBody, VideoBody,
};
use crate::channels::send::{
    record_outbound_http_failure, record_outbound_http_success, QueuedOutboundMessage,
};
use crate::channels::wss_gateway::{WssConnection, WssEvent};
use crate::channels::ChannelHttpClient;
use crate::config::AppConfig;
use crate::error::{Error, Result};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const WECOM_AIBOT_WS_URL: &str = "wss://openws.work.weixin.qq.com";

const TAG: &str = "wecom_aibot";
const CMD_SUBSCRIBE: &str = "aibot_subscribe";
const CMD_PING: &str = "ping";
const CMD_MSG_CALLBACK: &str = "aibot_msg_callback";
const CMD_EVENT_CALLBACK: &str = "aibot_event_callback";
const CMD_RESPOND_MSG: &str = "aibot_respond_msg";
const CMD_SEND_MSG: &str = "aibot_send_msg";
const RECV_TIMEOUT_SECS: u64 = 2;
const HEARTBEAT_INTERVAL_SECS: u64 = 30;
const BACKOFF_MAX_SECS: u64 = 120;
const ROUTE_STORE_MAX_ENTRIES: usize = 256;
const ROUTE_TTL_SECS: u64 = 24 * 60 * 60;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WecomAibotRoute {
    pub req_id: String,
    pub chat_id: String,
    pub chat_type: u32,
    pub updated_at_secs: u64,
}

pub type WecomAibotRouteStore = Arc<Mutex<HashMap<String, WecomAibotRoute>>>;

pub fn new_wecom_aibot_route_store() -> WecomAibotRouteStore {
    Arc::new(Mutex::new(HashMap::new()))
}

/// Reports AI Bot long-connection readiness from local credentials.
/// 企业微信 AI Bot 长连接没有可复用的轻量 HTTP 探测；这里仅报告配置是否完整。
pub fn check_connectivity<H: ChannelHttpClient + ?Sized>(
    config: &AppConfig,
    _http: &mut H,
) -> crate::channels::ChannelConnectivityItem {
    use crate::channels::connectivity::{self, CONNECTIVITY_NOT_CONFIGURED_KEY};
    let configured =
        !config.wecom_bot_id.trim().is_empty() && !config.wecom_bot_secret.trim().is_empty();
    connectivity::item(
        "wecom",
        configured,
        configured,
        (!configured).then_some(CONNECTIVITY_NOT_CONFIGURED_KEY),
    )
}

#[derive(serde::Deserialize)]
struct WecomEnvelope {
    #[serde(default)]
    cmd: String,
    #[serde(default)]
    headers: WecomHeaders,
    #[serde(default)]
    body: Value,
}

#[derive(Default, serde::Deserialize)]
struct WecomHeaders {
    #[serde(default)]
    req_id: String,
}

#[derive(serde::Deserialize)]
struct WecomIncomingMessage {
    #[serde(default)]
    msgid: String,
    #[serde(default)]
    chatid: String,
    #[serde(default)]
    chattype: String,
    #[serde(default)]
    from: WecomIncomingFrom,
    #[serde(default)]
    msgtype: String,
    #[serde(default)]
    text: Option<WecomText>,
    #[serde(default)]
    image: Option<WecomRemoteAsset>,
    #[serde(default)]
    file: Option<WecomRemoteAsset>,
    #[serde(default)]
    video: Option<WecomRemoteAsset>,
    #[serde(default)]
    voice: Option<WecomVoice>,
    #[serde(default)]
    event: Option<WecomEvent>,
}

#[derive(Default, serde::Deserialize)]
struct WecomIncomingFrom {
    #[serde(default)]
    userid: String,
}

#[derive(serde::Deserialize)]
struct WecomText {
    #[serde(default)]
    content: String,
}

#[derive(serde::Deserialize)]
struct WecomRemoteAsset {
    #[serde(default)]
    url: String,
    #[serde(default)]
    aeskey: String,
}

#[derive(serde::Deserialize)]
struct WecomVoice {
    #[serde(default)]
    content: String,
}

#[derive(serde::Deserialize)]
struct WecomEvent {
    #[serde(default)]
    eventtype: String,
}

static REQ_COUNTER: AtomicU64 = AtomicU64::new(1);

fn next_req_id(prefix: &str) -> String {
    let seq = REQ_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{prefix}-{}-{seq}", crate::util::current_unix_secs())
}

fn incoming_chat_id(message: &WecomIncomingMessage) -> String {
    if !message.chatid.trim().is_empty() {
        message.chatid.trim().to_string()
    } else if !message.from.userid.trim().is_empty() {
        message.from.userid.trim().to_string()
    } else {
        "wecom_default".to_string()
    }
}

fn chat_type_code(chattype: &str) -> u32 {
    if chattype == "group" {
        2
    } else {
        1
    }
}

fn remote_asset_ref(asset: &WecomRemoteAsset) -> Option<MediaAssetRef> {
    let url = asset.url.trim();
    if url.is_empty() {
        return None;
    }
    let mut media = MediaAssetRef::external_url(url);
    media.source_platform = AssetSourcePlatform::WeCom;
    if !asset.aeskey.trim().is_empty() {
        media.ttl_seconds = Some(300);
    }
    Some(media)
}

fn build_inbound_body(message: &WecomIncomingMessage) -> Result<Option<CanonicalMessageBody>> {
    match message.msgtype.trim() {
        "text" => {
            let text = message
                .text
                .as_ref()
                .map(|text| text.content.trim().to_string())
                .unwrap_or_default();
            if text.is_empty() {
                Ok(None)
            } else {
                Ok(Some(CanonicalMessageBody::Text(TextBody::plain(text))))
            }
        }
        "voice" => {
            let text = message
                .voice
                .as_ref()
                .map(|voice| voice.content.trim().to_string())
                .unwrap_or_default();
            if text.is_empty() {
                Ok(None)
            } else {
                Ok(Some(CanonicalMessageBody::Audio(AudioBody {
                    asset: MediaAssetRef::platform_handle(AssetSourcePlatform::WeCom, ""),
                    caption: None,
                    transcript_text: Some(text),
                })))
            }
        }
        "image" => Ok(message
            .image
            .as_ref()
            .and_then(remote_asset_ref)
            .map(|asset| {
                CanonicalMessageBody::Image(ImageBody {
                    asset,
                    caption: None,
                })
            })),
        "file" => Ok(message
            .file
            .as_ref()
            .and_then(remote_asset_ref)
            .map(|asset| {
                CanonicalMessageBody::File(FileBody {
                    asset,
                    caption: None,
                })
            })),
        "video" => Ok(message
            .video
            .as_ref()
            .and_then(remote_asset_ref)
            .map(|asset| {
                CanonicalMessageBody::Video(VideoBody {
                    asset,
                    caption: None,
                    title: None,
                    description: None,
                })
            })),
        _ => Ok(None),
    }
}

fn remember_route(
    route_store: &WecomAibotRouteStore,
    chat_id: &str,
    req_id: &str,
    chat_type: u32,
) -> Result<()> {
    if chat_id.trim().is_empty() || req_id.trim().is_empty() {
        return Ok(());
    }
    let mut guard = route_store.lock().map_err(|e| Error::Other {
        source: Box::new(std::io::Error::other(e.to_string())),
        stage: "wecom_aibot_route_lock",
    })?;
    let now = crate::util::current_unix_secs();
    guard.retain(|_, route| now.saturating_sub(route.updated_at_secs) <= ROUTE_TTL_SECS);
    if !guard.contains_key(chat_id) && guard.len() >= ROUTE_STORE_MAX_ENTRIES {
        if let Some(oldest_key) = guard
            .iter()
            .min_by_key(|(_, route)| route.updated_at_secs)
            .map(|(key, _)| key.clone())
        {
            guard.remove(&oldest_key);
        }
    }
    guard.insert(
        chat_id.to_string(),
        WecomAibotRoute {
            req_id: req_id.to_string(),
            chat_id: chat_id.to_string(),
            chat_type,
            updated_at_secs: now,
        },
    );
    Ok(())
}

fn route_for(route_store: &WecomAibotRouteStore, chat_id: &str) -> Result<Option<WecomAibotRoute>> {
    let mut guard = route_store.lock().map_err(|e| Error::Other {
        source: Box::new(std::io::Error::other(e.to_string())),
        stage: "wecom_aibot_route_lock",
    })?;
    let Some(route) = guard.get(chat_id).cloned() else {
        return Ok(None);
    };
    if crate::util::current_unix_secs().saturating_sub(route.updated_at_secs) > ROUTE_TTL_SECS {
        guard.remove(chat_id);
        return Ok(None);
    }
    Ok(Some(route))
}

pub fn handle_aibot_frame(
    frame: &str,
    inbound_tx: &InboundTx,
    route_store: &WecomAibotRouteStore,
) -> Result<()> {
    let envelope: WecomEnvelope =
        serde_json::from_str(frame).map_err(|e| Error::config(TAG, e.to_string()))?;
    match envelope.cmd.as_str() {
        CMD_MSG_CALLBACK => {
            let message: WecomIncomingMessage = serde_json::from_value(envelope.body)
                .map_err(|e| Error::config(TAG, e.to_string()))?;
            if message
                .event
                .as_ref()
                .is_some_and(|event| !event.eventtype.trim().is_empty())
            {
                return Ok(());
            }
            let Some(body) = build_inbound_body(&message)? else {
                return Ok(());
            };
            let chat_id = incoming_chat_id(&message);
            let chat_type = chat_type_code(&message.chattype);
            remember_route(route_store, &chat_id, &envelope.headers.req_id, chat_type)?;
            let inbound_dedup_key = if message.msgid.trim().is_empty() {
                String::new()
            } else {
                format!("wecom_aibot_message:{}", message.msgid.trim())
            };
            let msg = PcMsg::new_inbound_with_body("wecom", &chat_id, body, chat_type == 2)?
                .with_inbound_provenance(
                    MessageTransport::Wss,
                    message.msgid.trim(),
                    envelope.headers.req_id.trim(),
                    inbound_dedup_key,
                );
            match inbound_tx.try_send(msg) {
                Ok(()) => {}
                Err(std::sync::mpsc::TrySendError::Full(_)) => {
                    log::warn!("[{}] inbound queue full, dropping callback", TAG);
                }
                Err(std::sync::mpsc::TrySendError::Disconnected(_)) => {
                    log::warn!("[{}] inbound_tx disconnected, dropping callback", TAG);
                }
            }
            Ok(())
        }
        CMD_EVENT_CALLBACK => Ok(()),
        _ => Ok(()),
    }
}

fn outbound_text(message: &QueuedOutboundMessage) -> Result<String> {
    let text = match &message.body {
        CanonicalMessageBody::Text(text) => {
            if text.text.trim().is_empty() {
                message.content.trim().to_string()
            } else {
                text.text.trim().to_string()
            }
        }
        _ => message.body.text_projection().trim().to_string(),
    };
    if text.is_empty() {
        Err(Error::config(
            "wecom_aibot_send",
            "refusing to send empty WeCom AI Bot message",
        ))
    } else {
        Ok(text)
    }
}

pub fn build_outbound_command(
    message: &QueuedOutboundMessage,
    route_store: &WecomAibotRouteStore,
) -> Result<Value> {
    let content = outbound_text(message)?;
    if let Some(route) = route_for(route_store, &message.chat_id)? {
        return Ok(serde_json::json!({
            "cmd": CMD_RESPOND_MSG,
            "headers": { "req_id": route.req_id },
            "body": {
                "msgtype": "markdown",
                "markdown": { "content": content },
            }
        }));
    }
    Ok(serde_json::json!({
        "cmd": CMD_SEND_MSG,
        "headers": { "req_id": message.req_id.clone().unwrap_or_else(|| next_req_id("send")) },
        "body": {
            "chatid": message.chat_id,
            "msgtype": "markdown",
            "markdown": { "content": content },
        }
    }))
}

fn subscribe_command(bot_id: &str, bot_secret: &str) -> Value {
    serde_json::json!({
        "cmd": CMD_SUBSCRIBE,
        "headers": { "req_id": next_req_id("sub") },
        "body": {
            "bot_id": bot_id,
            "secret": bot_secret,
        }
    })
}

fn ping_command() -> Value {
    serde_json::json!({
        "cmd": CMD_PING,
        "headers": { "req_id": next_req_id("ping") },
    })
}

fn send_json_command<C: WssConnection>(conn: &mut C, command: &Value) -> Result<()> {
    conn.send_text(&command.to_string())
}

pub fn run_wecom_aibot_loop<C, Connect>(
    bot_id: String,
    bot_secret: String,
    websocket_url: String,
    inbound_tx: InboundTx,
    outbound_rx: std::sync::mpsc::Receiver<QueuedOutboundMessage>,
    route_store: WecomAibotRouteStore,
    mut connect: Connect,
) where
    C: WssConnection,
    Connect: FnMut(&str) -> Result<C>,
{
    let url = if websocket_url.trim().is_empty() {
        WECOM_AIBOT_WS_URL.to_string()
    } else {
        websocket_url.trim().to_string()
    };
    let mut backoff_secs = crate::orchestrator::current_budget().reconnect_backoff_secs;
    let mut pending_outbound: Option<QueuedOutboundMessage> = None;
    loop {
        let mut conn = match connect(&url) {
            Ok(conn) => conn,
            Err(error) => {
                log::warn!("[{}] connect failed: {}", TAG, error);
                std::thread::sleep(Duration::from_secs(backoff_secs));
                backoff_secs = (backoff_secs * 2).min(BACKOFF_MAX_SECS);
                continue;
            }
        };
        backoff_secs = crate::orchestrator::current_budget().reconnect_backoff_secs;
        if let Err(error) = send_json_command(&mut conn, &subscribe_command(&bot_id, &bot_secret)) {
            log::warn!("[{}] subscribe failed: {}", TAG, error);
            std::thread::sleep(Duration::from_secs(backoff_secs));
            continue;
        }
        let mut last_ping = Instant::now();
        'session: loop {
            while let Some(message) = pending_outbound
                .take()
                .or_else(|| outbound_rx.try_recv().ok())
            {
                let command = match build_outbound_command(&message, &route_store) {
                    Ok(command) => command,
                    Err(error) => {
                        record_outbound_http_failure(&error);
                        log::warn!("[{}] build outbound command failed: {}", TAG, error);
                        continue;
                    }
                };
                if let Err(error) = send_json_command(&mut conn, &command) {
                    record_outbound_http_failure(&error);
                    log::warn!(
                        "[{}] send failed, will retry after reconnect: {}",
                        TAG,
                        error
                    );
                    pending_outbound = Some(message);
                    break 'session;
                }
                record_outbound_http_success();
            }
            if last_ping.elapsed() >= Duration::from_secs(HEARTBEAT_INTERVAL_SECS) {
                if let Err(error) = send_json_command(&mut conn, &ping_command()) {
                    log::warn!("[{}] ping failed: {}", TAG, error);
                    break;
                }
                last_ping = Instant::now();
            }
            match conn.recv_timeout(Duration::from_secs(RECV_TIMEOUT_SECS)) {
                Ok(Some(WssEvent::Binary(data))) => {
                    let frame = match std::str::from_utf8(data.as_slice()) {
                        Ok(frame) => frame,
                        Err(error) => {
                            log::warn!("[{}] invalid utf-8 frame: {}", TAG, error);
                            continue;
                        }
                    };
                    if let Err(error) = handle_aibot_frame(frame, &inbound_tx, &route_store) {
                        log::warn!("[{}] handle frame failed: {}", TAG, error);
                    }
                }
                Ok(Some(WssEvent::Closed(info))) => {
                    log::info!("[{}] closed: {:?}", TAG, info);
                    break;
                }
                Ok(Some(WssEvent::Disconnected)) => {
                    log::info!("[{}] disconnected", TAG);
                    break;
                }
                Ok(None) => {}
                Err(error) => {
                    log::warn!("[{}] recv failed: {}", TAG, error);
                    break;
                }
            }
        }
        std::thread::sleep(Duration::from_secs(backoff_secs));
    }
}

#[cfg(test)]
mod tests {
    use crate::bus::{new_inbound_channel, CanonicalMessageBody, MessageTransport, TextBody};
    use crate::channels::send::QueuedOutboundMessage;

    #[test]
    fn wecom_aibot_msg_callback_enqueues_text_and_records_route() {
        let frame = serde_json::json!({
            "cmd": "aibot_msg_callback",
            "headers": { "req_id": "req-1" },
            "body": {
                "msgid": "msg-1",
                "aibotid": "bot-1",
                "chatid": "chat-1",
                "chattype": "group",
                "from": { "userid": "user-1" },
                "msgtype": "text",
                "text": { "content": "hello wecom" }
            }
        })
        .to_string();
        let (inbound_tx, inbound_rx, _) = new_inbound_channel(4);
        let route_store = super::new_wecom_aibot_route_store();

        super::handle_aibot_frame(&frame, &inbound_tx, &route_store).expect("frame");

        let msg = inbound_rx.try_recv().expect("inbound");
        assert_eq!(msg.channel.as_ref(), "wecom");
        assert_eq!(msg.chat_id.as_ref(), "chat-1");
        assert_eq!(msg.content, "hello wecom");
        assert_eq!(msg.source_transport, MessageTransport::Wss);
        assert_eq!(msg.platform_message_id, "msg-1");

        let command = super::build_outbound_command(
            &QueuedOutboundMessage {
                transport_send_id: 1,
                chat_id: "chat-1".to_string(),
                content: "reply text".to_string(),
                body: CanonicalMessageBody::Text(TextBody::plain("reply text")),
                platform_thread_id: String::new(),
                req_id: Some("out-1".to_string()),
                outbound_kind: crate::bus::OutboundKind::Primary,
            },
            &route_store,
        )
        .expect("command");
        assert_eq!(command["cmd"], "aibot_respond_msg");
        assert_eq!(command["headers"]["req_id"], "req-1");
        assert_eq!(command["body"]["msgtype"], "markdown");
        assert_eq!(command["body"]["markdown"]["content"], "reply text");
    }

    #[test]
    fn wecom_aibot_outbound_without_route_uses_active_send() {
        let route_store = super::new_wecom_aibot_route_store();
        let command = super::build_outbound_command(
            &QueuedOutboundMessage {
                transport_send_id: 1,
                chat_id: "chat-2".to_string(),
                content: "active text".to_string(),
                body: CanonicalMessageBody::Text(TextBody::plain("active text")),
                platform_thread_id: String::new(),
                req_id: Some("out-2".to_string()),
                outbound_kind: crate::bus::OutboundKind::Primary,
            },
            &route_store,
        )
        .expect("command");

        assert_eq!(command["cmd"], "aibot_send_msg");
        assert_eq!(command["body"]["chatid"], "chat-2");
        assert_eq!(command["body"]["msgtype"], "markdown");
        assert_eq!(command["body"]["markdown"]["content"], "active text");
    }

    #[test]
    fn wecom_route_store_evicts_oldest_entry_at_capacity() {
        let route_store = super::new_wecom_aibot_route_store();
        for idx in 0..(super::ROUTE_STORE_MAX_ENTRIES + 1) {
            super::remember_route(
                &route_store,
                &format!("chat-{idx}"),
                &format!("req-{idx}"),
                2,
            )
            .expect("remember route");
        }

        let guard = route_store.lock().expect("route store");
        assert_eq!(guard.len(), super::ROUTE_STORE_MAX_ENTRIES);
        assert!(guard.contains_key(&format!("chat-{}", super::ROUTE_STORE_MAX_ENTRIES)));
    }
}
