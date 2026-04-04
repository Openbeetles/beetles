//! QQ WSS 入站：取 gateway URL → Hello/Identify → 心跳 → Dispatch 入队。
//! 支持频道 AT_MESSAGE_CREATE、群聊 GROUP_AT_MESSAGE_CREATE、私聊 C2C_MESSAGE_CREATE。
//! 与 HTTP webhook 可并存，由 main 按配置决定是否 spawn。

use crate::bus::PcMsg;
use crate::channels::wss_gateway::{
    run_wss_gateway_loop, WssConnection, WssGatewayDriver, WssRecvAction, WssSessionState,
};
use crate::channels::ChannelHttpClient;
use crate::error::{Error, Result};
use crate::memory::PendingRetryStore;

use super::send::{cache_msg_id, QqMsgIdCache};
use super::token::fetch_qq_access_token;

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

fn get_qq_access_token<H: ChannelHttpClient + ?Sized>(
    http: &mut H,
    app_id: &str,
    client_secret: &str,
) -> Result<String> {
    fetch_qq_access_token(http, app_id, client_secret, "qq_ws_token")
}

fn get_gateway_url<H: ChannelHttpClient + ?Sized>(http: &mut H, token: &str) -> Result<String> {
    let auth = format!("QQBot {}", token);
    let headers = [("Authorization", auth.as_str())];
    let (status, resp_body) = match http.http_get_with_headers(QQ_GATEWAY_URL, &headers) {
        Ok(resp) => resp,
        Err(e) => {
            crate::metrics::record_channel_http_result(false);
            return Err(Error::Other {
                source: Box::new(e),
                stage: "qq_ws_gateway",
            });
        }
    };
    if status >= 400 {
        crate::metrics::record_channel_http_result(false);
        return Err(Error::Http {
            status_code: status,
            stage: "qq_ws_gateway",
        });
    }
    crate::metrics::record_channel_http_result(true);
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
    cached_token: Option<String>,
    last_seq: Option<u64>,
    msg_id_cache: QqMsgIdCache,
}

impl QqWssDriver {
    fn new(app_id: String, client_secret: String, msg_id_cache: QqMsgIdCache) -> Self {
        Self {
            app_id,
            client_secret,
            cached_token: None,
            last_seq: None,
            msg_id_cache,
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
        let token = get_qq_access_token(http, &self.app_id, &self.client_secret)?;
        log::debug!("[{}] token obtained", TAG);
        self.cached_token = Some(token.clone());
        let url = get_gateway_url(http, &token)?;
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
        let identify_payload = self
            .cached_token
            .as_ref()
            .map(|token| build_identify_payload(token))
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
                                        self.cache_msg_id(ch, mid);
                                    }
                                    if let Ok(msg) = PcMsg::new("qq_channel", ch, content) {
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
                                        self.cache_msg_id(&chat_id, mid);
                                    }
                                    if let Ok(msg) = PcMsg::new("qq_channel", &chat_id, content) {
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
                                        self.cache_msg_id(&chat_id, mid);
                                    }
                                    if let Ok(msg) = PcMsg::new("qq_channel", &chat_id, content) {
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
    app_id: String,
    client_secret: String,
    inbound_tx: crate::bus::InboundTx,
    msg_id_cache: QqMsgIdCache,
    pending_retry: &dyn PendingRetryStore,
    create_http: CreateHttp,
    connect: Conn,
) where
    H: ChannelHttpClient,
    C: WssConnection,
    CreateHttp: FnMut() -> Result<H>,
    Conn: FnMut(&str) -> Result<C>,
{
    let driver = QqWssDriver::new(app_id, client_secret, msg_id_cache);
    run_wss_gateway_loop(TAG, driver, inbound_tx, pending_retry, create_http, connect);
}
