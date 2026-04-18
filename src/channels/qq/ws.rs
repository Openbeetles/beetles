//! QQ WSS 入站：取 gateway URL → Hello/Identify → 心跳 → Dispatch 入队。
//! 支持频道 AT_MESSAGE_CREATE、群聊 GROUP_AT_MESSAGE_CREATE、私聊 C2C_MESSAGE_CREATE。
//! 与 HTTP webhook 可并存，由 main 按配置决定是否 spawn。

use crate::channels::send::{record_outbound_http_failure, record_outbound_http_success};
use crate::channels::wss_gateway::{
    run_wss_gateway_loop, WssConnection, WssGatewayDriver, WssRecvAction, WssSessionState,
};
use crate::channels::ChannelHttpClient;
use crate::error::{Error, Result};
use crate::memory::PendingRetryStore;

use super::msg_id::{cache_msg_id, consume_inbound_dedup_key, QqInboundDedupStore, QqMsgIdCache};
use super::token::{
    cached_qq_token_value, clear_shared_cached_qq_token, ensure_cached_qq_token,
    fetch_and_cache_qq_token, invalidate_cached_qq_token, load_shared_cached_qq_token,
    sync_shared_cached_qq_token, CachedQqToken, SharedQqTokenCache,
};

const TAG: &str = "qq_ws";
const QQ_GATEWAY_URL: &str = "https://api.sgroup.qq.com/gateway";
const QQ_OP_HELLO: u64 = 10;
const QQ_OP_IDENTIFY: u64 = 2;
const QQ_OP_DISPATCH: u64 = 0;
const QQ_OP_HEARTBEAT: u64 = 1;
const QQ_OP_HEARTBEAT_ACK: u64 = 11;
const QQ_OP_RECONNECT: u64 = 7;
const QQ_OP_INVALID_SESSION: u64 = 9;
const AT_MESSAGE_CREATE: &str = "AT_MESSAGE_CREATE";
const GROUP_AT_MESSAGE_CREATE: &str = "GROUP_AT_MESSAGE_CREATE";
const C2C_MESSAGE_CREATE: &str = "C2C_MESSAGE_CREATE";
/// 频道公域消息 intent（频道 @ 消息）
const PUBLIC_GUILD_MESSAGES_INTENT: u64 = 1 << 30;
/// 群聊与私聊 intent（GROUP_AT_MESSAGE_CREATE + C2C_MESSAGE_CREATE）
const GROUP_AND_C2C_INTENT: u64 = 1 << 25;
const QQ_TOKEN_REFRESH_SKEW_SECS: u64 = 60;
const DEDUP_CACHE_CAPACITY: usize = 64;

pub struct QqWsLoopConfig {
    pub app_id: String,
    pub client_secret: String,
    pub msg_id_cache: QqMsgIdCache,
    pub inbound_dedup_store: QqInboundDedupStore,
    pub shared_token_cache: SharedQqTokenCache,
}

#[derive(serde::Deserialize)]
struct QqGatewayEnvelope {
    op: u64,
    #[serde(default)]
    s: Option<u64>,
    #[serde(default)]
    t: Option<String>,
    #[serde(default)]
    d: Option<QqGatewayData>,
}

#[derive(serde::Deserialize, Default)]
struct QqGatewayData {
    #[serde(default)]
    heartbeat_interval: Option<u64>,
    #[serde(default)]
    channel_id: Option<String>,
    #[serde(default)]
    group_openid: Option<String>,
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    author: Option<QqGatewayAuthor>,
}

#[derive(serde::Deserialize, Default)]
struct QqGatewayAuthor {
    #[serde(default)]
    user_openid: Option<String>,
}

struct DeduplicateRing {
    ids: Vec<String>,
    pos: usize,
    cap: usize,
}

impl DeduplicateRing {
    fn new(cap: usize) -> Self {
        Self {
            ids: Vec::with_capacity(cap),
            pos: 0,
            cap,
        }
    }

    fn contains_or_insert(&mut self, id: &str) -> bool {
        if self.ids.iter().any(|existing| existing == id) {
            return true;
        }
        if self.ids.len() < self.cap {
            self.ids.push(id.to_string());
        } else {
            self.ids[self.pos] = id.to_string();
        }
        self.pos = (self.pos + 1) % self.cap.max(1);
        false
    }
}

fn build_identify_payload(token: &str) -> Vec<u8> {
    let auth = format!("QQBot {}", token);
    let mut payload = String::with_capacity(auth.len() + 160);
    payload.push_str("{\"op\":");
    payload.push_str(&QQ_OP_IDENTIFY.to_string());
    payload.push_str(",\"d\":{\"token\":");
    crate::util::push_json_string_escaped(&mut payload, &auth);
    payload.push_str(",\"intents\":");
    payload.push_str(&(PUBLIC_GUILD_MESSAGES_INTENT | GROUP_AND_C2C_INTENT).to_string());
    payload.push_str(",\"shard\":[0,1],\"properties\":{\"$os\":\"linux\",\"$browser\":\"my_library\",\"$device\":\"my_library\"}}}");
    payload.into_bytes()
}

fn build_heartbeat_payload(seq: u64) -> Vec<u8> {
    format!("{{\"op\":{},\"d\":{}}}", QQ_OP_HEARTBEAT, seq).into_bytes()
}

fn get_gateway_url<H: ChannelHttpClient + ?Sized>(http: &mut H, token: &str) -> Result<String> {
    let auth = format!("QQBot {}", token);
    let headers = [("Authorization", auth.as_str())];
    let (status, resp_body) = match http.http_get_with_headers(QQ_GATEWAY_URL, &headers) {
        Ok(resp) => resp,
        Err(e) => {
            let error = Error::Other {
                source: Box::new(e),
                stage: "qq_ws_gateway",
            };
            record_outbound_http_failure(&error);
            return Err(error);
        }
    };
    if status >= 400 {
        let error = Error::Http {
            status_code: status,
            stage: "qq_ws_gateway",
        };
        record_outbound_http_failure(&error);
        return Err(error);
    }
    record_outbound_http_success();
    #[derive(serde::Deserialize)]
    struct GatewayResp {
        url: Option<String>,
    }
    let r: GatewayResp = serde_json::from_slice(resp_body.as_ref()).map_err(|e| Error::Other {
        source: Box::new(e),
        stage: "qq_ws_gateway",
    })?;
    r.url
        .filter(|u| u.starts_with("wss://") || u.starts_with("ws://"))
        .ok_or_else(|| Error::Other {
            source: Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "qq_ws gateway missing url",
            )),
            stage: "qq_ws_gateway",
        })
}

/// QQ WSS 协议驱动：取 token + GET gateway、Hello(op=10)、Identify(op=2)、心跳(op=1)、Dispatch 解析。
struct QqWssDriver {
    app_id: String,
    client_secret: String,
    cached_token: Option<CachedQqToken>,
    shared_token_cache: SharedQqTokenCache,
    last_seq: Option<u64>,
    msg_id_cache: QqMsgIdCache,
    inbound_dedup_store: QqInboundDedupStore,
    dedup: DeduplicateRing,
}

impl QqWssDriver {
    fn new(
        app_id: String,
        client_secret: String,
        msg_id_cache: QqMsgIdCache,
        inbound_dedup_store: QqInboundDedupStore,
        shared_token_cache: SharedQqTokenCache,
    ) -> Self {
        Self {
            app_id,
            client_secret,
            cached_token: None,
            shared_token_cache,
            last_seq: None,
            msg_id_cache,
            inbound_dedup_store,
            dedup: DeduplicateRing::new(DEDUP_CACHE_CAPACITY),
        }
    }

    /// 将 msg_id 存入缓存，供发送时被动回复使用。
    fn cache_msg_id(&self, chat_id: &str, msg_id: &str) {
        if let Err(e) = cache_msg_id(&self.msg_id_cache, chat_id, msg_id) {
            log::warn!("[{}] cache_msg_id failed: {}", TAG, e);
        }
    }
}

impl WssGatewayDriver for QqWssDriver {
    fn get_url(&mut self, http: &mut dyn ChannelHttpClient) -> Result<String> {
        if self.cached_token.is_none() {
            self.cached_token = load_shared_cached_qq_token(&self.shared_token_cache);
        }
        let token = ensure_cached_qq_token(
            http,
            &mut self.cached_token,
            &self.app_id,
            &self.client_secret,
            "qq_ws_token",
            QQ_TOKEN_REFRESH_SKEW_SECS,
        )?;
        sync_shared_cached_qq_token(&self.shared_token_cache, &self.cached_token);
        log::debug!("[{}] token obtained", TAG);
        let url = match get_gateway_url(http, &token) {
            Ok(url) => url,
            Err(e) if e.http_status_code() == Some(401) => {
                log::warn!("[{}] gateway rejected cached token, refreshing once", TAG);
                invalidate_cached_qq_token(&mut self.cached_token);
                clear_shared_cached_qq_token(&self.shared_token_cache);
                let refreshed = fetch_and_cache_qq_token(
                    http,
                    &mut self.cached_token,
                    &self.app_id,
                    &self.client_secret,
                    "qq_ws_token",
                    QQ_TOKEN_REFRESH_SKEW_SECS,
                )?;
                sync_shared_cached_qq_token(&self.shared_token_cache, &self.cached_token);
                get_gateway_url(http, &refreshed)?
            }
            Err(e) => return Err(e),
        };
        log::debug!("[{}] gateway url len={}", TAG, url.len());
        Ok(url)
    }

    fn expects_hello(&self) -> bool {
        true
    }

    fn on_hello(&mut self, first_message: &[u8]) -> Result<WssSessionState> {
        let value: QqGatewayEnvelope =
            serde_json::from_slice(first_message).map_err(|e| Error::Other {
                source: Box::new(e),
                stage: "qq_ws_hello",
            })?;
        if value.op != QQ_OP_HELLO {
            return Ok(WssSessionState {
                heartbeat_interval_ms: 45_000,
                identify_payload: None,
            });
        }
        let interval = value
            .d
            .as_ref()
            .and_then(|d| d.heartbeat_interval)
            .unwrap_or(45_000);
        let identify_payload = cached_qq_token_value(&self.cached_token)
            .map(build_identify_payload)
            .filter(|v| !v.is_empty());
        log::info!("[{}] hello ok, heartbeat_interval_ms={}", TAG, interval);
        Ok(WssSessionState {
            heartbeat_interval_ms: interval,
            identify_payload,
        })
    }

    fn on_recv(&mut self, data: &[u8]) -> Result<WssRecvAction> {
        let value: QqGatewayEnvelope = match serde_json::from_slice(data) {
            Ok(v) => v,
            Err(_) => return Ok(WssRecvAction::Ignore),
        };
        if let Some(seq) = value.s {
            self.last_seq = Some(seq);
        }
        log::debug!("[{}] recv op={} s={:?}", TAG, value.op, value.s);
        match value.op {
            QQ_OP_DISPATCH => {
                let t = value.t.as_deref().unwrap_or("");
                log::debug!("[{}] dispatch t={}", TAG, t);
                let d = value.d.as_ref();
                match t {
                    AT_MESSAGE_CREATE => {
                        // 频道消息：chat_id = channel_id
                        if let Some(d) = d {
                            let channel_id = d.channel_id.as_deref();
                            let content = d.content.as_deref();
                            let msg_id = d.id.as_deref();
                            if let (Some(ch), Some(content)) = (channel_id, content) {
                                if !ch.is_empty() && !content.is_empty() {
                                    if let Some(mid) = msg_id {
                                        if self.dedup.contains_or_insert(mid) {
                                            log::info!(
                                                "[{}] duplicate QQ inbound msg_id={} ignored",
                                                TAG,
                                                mid
                                            );
                                            return Ok(WssRecvAction::Dispatch(None));
                                        }
                                        self.cache_msg_id(ch, mid);
                                    }
                                    if let Ok(msg) = super::build_inbound_message(
                                        ch,
                                        content,
                                        crate::bus::MessageTransport::Wss,
                                        msg_id,
                                        None,
                                    ) {
                                        if consume_inbound_dedup_key(
                                            &self.inbound_dedup_store,
                                            &msg.inbound_dedup_key,
                                        )
                                        .unwrap_or(false)
                                        {
                                            return Ok(WssRecvAction::Dispatch(None));
                                        }
                                        return Ok(WssRecvAction::Dispatch(Some(msg)));
                                    }
                                }
                            }
                        }
                    }
                    GROUP_AT_MESSAGE_CREATE => {
                        // 群聊 @ 消息：chat_id = "group:{group_openid}"
                        if let Some(d) = d {
                            let group_openid = d.group_openid.as_deref();
                            let content = d.content.as_deref();
                            let msg_id = d.id.as_deref();
                            if let (Some(gid), Some(content)) = (group_openid, content) {
                                if !gid.is_empty() && !content.is_empty() {
                                    let chat_id = format!("group:{}", gid);
                                    if let Some(mid) = msg_id {
                                        if self.dedup.contains_or_insert(mid) {
                                            log::info!(
                                                "[{}] duplicate QQ inbound msg_id={} ignored",
                                                TAG,
                                                mid
                                            );
                                            return Ok(WssRecvAction::Dispatch(None));
                                        }
                                        self.cache_msg_id(&chat_id, mid);
                                    }
                                    if let Ok(msg) = super::build_inbound_message(
                                        &chat_id,
                                        content,
                                        crate::bus::MessageTransport::Wss,
                                        msg_id,
                                        None,
                                    ) {
                                        if consume_inbound_dedup_key(
                                            &self.inbound_dedup_store,
                                            &msg.inbound_dedup_key,
                                        )
                                        .unwrap_or(false)
                                        {
                                            return Ok(WssRecvAction::Dispatch(None));
                                        }
                                        return Ok(WssRecvAction::Dispatch(Some(msg)));
                                    }
                                }
                            }
                        }
                    }
                    C2C_MESSAGE_CREATE => {
                        // C2C 单聊：用 author.user_openid 标识对方，chat_id = "c2c:{user_openid}"
                        if let Some(d) = d {
                            let user_openid =
                                d.author.as_ref().and_then(|a| a.user_openid.as_deref());
                            let content = d.content.as_deref();
                            let msg_id = d.id.as_deref();
                            if let (Some(uid), Some(content)) = (user_openid, content) {
                                if !uid.is_empty() && !content.is_empty() {
                                    let chat_id = format!("c2c:{}", uid);
                                    if let Some(mid) = msg_id {
                                        if self.dedup.contains_or_insert(mid) {
                                            log::info!(
                                                "[{}] duplicate QQ inbound msg_id={} ignored",
                                                TAG,
                                                mid
                                            );
                                            return Ok(WssRecvAction::Dispatch(None));
                                        }
                                        self.cache_msg_id(&chat_id, mid);
                                    }
                                    if let Ok(msg) = super::build_inbound_message(
                                        &chat_id,
                                        content,
                                        crate::bus::MessageTransport::Wss,
                                        msg_id,
                                        None,
                                    ) {
                                        if consume_inbound_dedup_key(
                                            &self.inbound_dedup_store,
                                            &msg.inbound_dedup_key,
                                        )
                                        .unwrap_or(false)
                                        {
                                            return Ok(WssRecvAction::Dispatch(None));
                                        }
                                        return Ok(WssRecvAction::Dispatch(Some(msg)));
                                    }
                                }
                            }
                        }
                    }
                    _ => {}
                }
                Ok(WssRecvAction::Dispatch(None))
            }
            QQ_OP_HEARTBEAT_ACK => Ok(WssRecvAction::SendHeartbeat(self.last_seq.unwrap_or(0))),
            QQ_OP_RECONNECT => {
                log::info!("[{}] server requested reconnect", TAG);
                Ok(WssRecvAction::Disconnect)
            }
            QQ_OP_INVALID_SESSION => {
                log::warn!("[{}] invalid session", TAG);
                invalidate_cached_qq_token(&mut self.cached_token);
                clear_shared_cached_qq_token(&self.shared_token_cache);
                Ok(WssRecvAction::Disconnect)
            }
            _ => Ok(WssRecvAction::Ignore),
        }
    }

    fn build_heartbeat(&self, seq: Option<u64>) -> Result<Vec<u8>> {
        let d = seq.unwrap_or(0);
        log::debug!("[{}] build_heartbeat seq={}", TAG, d);
        Ok(build_heartbeat_payload(d))
    }
}

/// 长连接循环：委托 run_wss_gateway_loop，使用 QqWssDriver。
/// create_http 与 connect 由调用方（main）注入，本模块不依赖具体平台类型。
pub fn run_qq_ws_loop<H, C, CreateHttp, Conn>(
    config: QqWsLoopConfig,
    inbound_tx: crate::bus::InboundTx,
    pending_retry: &dyn PendingRetryStore,
    create_http: CreateHttp,
    connect: Conn,
) where
    H: ChannelHttpClient,
    C: WssConnection,
    CreateHttp: FnMut() -> Result<H>,
    Conn: FnMut(&str) -> Result<C>,
{
    let driver = QqWssDriver::new(
        config.app_id,
        config.client_secret,
        config.msg_id_cache,
        config.inbound_dedup_store,
        config.shared_token_cache,
    );
    run_wss_gateway_loop(TAG, driver, inbound_tx, pending_retry, create_http, connect);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    #[test]
    fn group_dispatch_message_is_marked_as_group_and_duplicate_wss_dispatch_is_ignored() {
        let cache: QqMsgIdCache = Arc::new(Mutex::new(HashMap::new()));
        let dedup_store: QqInboundDedupStore = Arc::new(Mutex::new(HashMap::new()));
        let mut driver = QqWssDriver::new(
            "app".to_string(),
            "secret".to_string(),
            cache,
            dedup_store,
            crate::channels::qq::new_shared_qq_token_cache(),
        );
        let payload = serde_json::json!({
            "op": 0,
            "t": "GROUP_AT_MESSAGE_CREATE",
            "d": {
                "id": "msg-1",
                "group_openid": "group-openid-42",
                "content": "@beetle hello"
            }
        });

        let action = driver
            .on_recv(payload.to_string().as_bytes())
            .expect("recv action");

        let WssRecvAction::Dispatch(Some(msg)) = action else {
            panic!("expected dispatch with message");
        };
        assert_eq!(msg.chat_id.as_ref(), "group:group-openid-42");
        assert!(msg.is_group);
        assert_eq!(msg.source_transport, crate::bus::MessageTransport::Wss);
        assert_eq!(msg.platform_message_id, "msg-1");
        assert_eq!(msg.inbound_dedup_key, "qq_message:msg-1");

        let duplicate = driver
            .on_recv(payload.to_string().as_bytes())
            .expect("recv duplicate action");

        assert!(matches!(duplicate, WssRecvAction::Dispatch(None)));
    }

    #[test]
    fn duplicate_after_webhook_dispatch_is_ignored_by_wss_driver() {
        let secret = "qq-test-secret";
        let timestamp = "1711936800";
        let body = serde_json::json!({
            "op": 0,
            "t": "C2C_MESSAGE_CREATE",
            "d": {
                "id": "msg-shared-1",
                "content": "hello",
                "author": {
                    "user_openid": "user-openid-42"
                }
            }
        });
        let body_bytes = serde_json::to_vec(&body).unwrap();
        let signature = super::super::signature::sign_qq_url_verify(
            secret,
            timestamp,
            std::str::from_utf8(&body_bytes).unwrap(),
        )
        .unwrap();
        let (inbound_tx, inbound_rx, _) = crate::bus::new_inbound_channel(4);
        let cache: QqMsgIdCache = Arc::new(Mutex::new(HashMap::new()));
        let dedup_store: QqInboundDedupStore = Arc::new(Mutex::new(HashMap::new()));

        super::super::handle_webhook(
            &body_bytes,
            Some(timestamp),
            Some(&signature),
            "",
            secret,
            &inbound_tx,
            Arc::clone(&cache),
            Arc::clone(&dedup_store),
        )
        .expect("webhook dispatch");
        let webhook_msg = inbound_rx.try_recv().expect("webhook inbound");
        assert_eq!(webhook_msg.platform_message_id, "msg-shared-1");

        let mut driver = QqWssDriver::new(
            "app".to_string(),
            "secret".to_string(),
            cache,
            dedup_store,
            crate::channels::qq::new_shared_qq_token_cache(),
        );
        let duplicate = driver
            .on_recv(
                serde_json::json!({
                    "op": 0,
                    "t": "C2C_MESSAGE_CREATE",
                    "d": {
                        "id": "msg-shared-1",
                        "content": "hello",
                        "author": {
                            "user_openid": "user-openid-42"
                        }
                    }
                })
                .to_string()
                .as_bytes(),
            )
            .expect("recv duplicate action");

        assert!(matches!(duplicate, WssRecvAction::Dispatch(None)));
    }
}
