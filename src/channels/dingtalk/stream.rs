//! DingTalk Stream Mode inbound loop.
//! 钉钉 Stream Mode 入站：注册 WSS 连接、处理系统帧与机器人消息回调。

use crate::bus::InboundTx;
use crate::channels::wss_gateway::{WssConnection, WssEvent};
use crate::channels::ChannelHttpClient;
use crate::error::{Error, Result};
use serde_json::Value;
use std::time::Duration;

const TAG: &str = "dingtalk_stream";
const GATEWAY_OPEN_URL: &str = "https://api.dingtalk.com/v1.0/gateway/connections/open";
const BOT_MESSAGE_TOPIC: &str = "/v1.0/im/bot/messages/get";
const BACKOFF_MAX_SECS: u64 = 120;
const RECV_TIMEOUT_SECS: u64 = 25;

#[derive(Debug, Default, PartialEq, Eq)]
pub struct DingtalkStreamFrameOutcome {
    pub reply: Option<String>,
}

#[derive(serde::Deserialize)]
struct DingtalkStreamFrame {
    #[serde(default, rename = "type")]
    frame_type: String,
    #[serde(default)]
    headers: Value,
    #[serde(default)]
    data: Value,
}

#[derive(serde::Deserialize)]
struct DingtalkGatewayOpenResponse {
    endpoint: String,
    ticket: String,
}

fn header_string(headers: &Value, key: &str) -> String {
    headers
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn parse_stream_data(data: &Value) -> Result<Option<String>> {
    match data {
        Value::Null => Ok(None),
        Value::String(raw) if raw.trim().is_empty() => Ok(None),
        Value::String(raw) => {
            serde_json::from_str::<Value>(raw).map_err(|e| Error::config(TAG, e.to_string()))?;
            Ok(Some(raw.to_string()))
        }
        Value::Object(_) => serde_json::to_string(data)
            .map(Some)
            .map_err(|e| Error::config(TAG, e.to_string())),
        _ => Ok(None),
    }
}

fn build_reply(message_id: &str, data: &str) -> String {
    serde_json::json!({
        "code": 200,
        "headers": {
            "contentType": "application/json",
            "messageId": message_id,
        },
        "message": "OK",
        "data": data,
    })
    .to_string()
}

pub fn handle_stream_frame(
    frame: &str,
    inbound_tx: &InboundTx,
    session_store: &super::DingtalkSessionStore,
) -> Result<DingtalkStreamFrameOutcome> {
    let frame: DingtalkStreamFrame =
        serde_json::from_str(frame).map_err(|e| Error::config(TAG, e.to_string()))?;
    let frame_type = frame.frame_type.trim();
    let message_id = header_string(&frame.headers, "messageId");
    match frame_type {
        "SYSTEM" => Ok(DingtalkStreamFrameOutcome {
            reply: Some(build_reply(&message_id, "")),
        }),
        "CALLBACK" | "EVENT" => {
            let topic = header_string(&frame.headers, "topic");
            if topic != BOT_MESSAGE_TOPIC {
                return Ok(DingtalkStreamFrameOutcome { reply: None });
            }
            if let Some(data) = parse_stream_data(&frame.data)? {
                if !super::inbound::handle_stream_callback_body(&data, inbound_tx, session_store)? {
                    return Ok(DingtalkStreamFrameOutcome { reply: None });
                }
            }
            Ok(DingtalkStreamFrameOutcome {
                reply: Some(build_reply(&message_id, r#"{"response":null}"#)),
            })
        }
        _ => Ok(DingtalkStreamFrameOutcome { reply: None }),
    }
}

fn register_connection<H: ChannelHttpClient>(
    http: &mut H,
    client_id: &str,
    client_secret: &str,
) -> Result<String> {
    let body = serde_json::json!({
        "clientId": client_id,
        "clientSecret": client_secret,
        "subscriptions": [
            {
                "type": "CALLBACK",
                "topic": BOT_MESSAGE_TOPIC,
            }
        ],
    });
    let body = serde_json::to_vec(&body).map_err(|e| Error::config(TAG, e.to_string()))?;
    let (status, resp_body) =
        http.http_post(GATEWAY_OPEN_URL, &body)
            .map_err(|e| Error::Other {
                source: Box::new(e),
                stage: TAG,
            })?;
    if status >= 400 {
        return Err(Error::http(TAG, status));
    }
    let response: DingtalkGatewayOpenResponse = serde_json::from_slice(resp_body.as_ref())
        .map_err(|e| Error::Other {
            source: Box::new(e),
            stage: TAG,
        })?;
    if response.endpoint.trim().is_empty() || response.ticket.trim().is_empty() {
        return Err(Error::config(
            TAG,
            "gateway response missing endpoint or ticket",
        ));
    }
    Ok(format!(
        "{}?ticket={}",
        response.endpoint.trim(),
        urlencoding::encode(response.ticket.trim())
    ))
}

pub fn run_dingtalk_stream_loop<H, C, CreateHttp, Connect>(
    client_id: String,
    client_secret: String,
    inbound_tx: InboundTx,
    session_store: super::DingtalkSessionStore,
    mut create_http: CreateHttp,
    mut connect: Connect,
) where
    H: ChannelHttpClient,
    C: WssConnection,
    CreateHttp: FnMut() -> Result<H>,
    Connect: FnMut(&str) -> Result<C>,
{
    let mut backoff_secs = crate::orchestrator::current_budget().reconnect_backoff_secs;
    loop {
        if !crate::runtime::thread_registry::runtime_mode_snapshot()
            .action_budget
            .allow_external_wss_connect
        {
            std::thread::sleep(Duration::from_secs(backoff_secs));
            continue;
        }
        let mut http = match create_http() {
            Ok(http) => http,
            Err(error) => {
                log::warn!("[{}] create_http failed: {}", TAG, error);
                std::thread::sleep(Duration::from_secs(backoff_secs));
                backoff_secs = (backoff_secs * 2).min(BACKOFF_MAX_SECS);
                continue;
            }
        };
        let url = match register_connection(&mut http, &client_id, &client_secret) {
            Ok(url) => url,
            Err(error) => {
                log::warn!("[{}] register connection failed: {}", TAG, error);
                std::thread::sleep(Duration::from_secs(backoff_secs));
                backoff_secs = (backoff_secs * 2).min(BACKOFF_MAX_SECS);
                continue;
            }
        };
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
        loop {
            crate::platform::task_wdt::feed_current_task();
            if !crate::runtime::thread_registry::runtime_mode_snapshot()
                .action_budget
                .allow_external_wss_connect
            {
                log::info!(
                    "[{}] disconnecting external WSS under runtime mode gate",
                    TAG
                );
                break;
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
                    match handle_stream_frame(frame, &inbound_tx, &session_store) {
                        Ok(outcome) => {
                            if let Some(reply) = outcome.reply {
                                if let Err(error) = conn.send_text(&reply) {
                                    log::warn!("[{}] send reply failed: {}", TAG, error);
                                    break;
                                }
                            }
                        }
                        Err(error) => log::warn!("[{}] handle frame failed: {}", TAG, error),
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
    use crate::bus::{new_inbound_channel, MessageTransport};
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    #[test]
    fn dingtalk_stream_callback_enqueues_message_and_ack() {
        let frame = serde_json::json!({
            "specVersion": "1.0",
            "type": "CALLBACK",
            "headers": {
                "messageId": "stream-msg-1",
                "topic": "/v1.0/im/bot/messages/get",
                "contentType": "application/json"
            },
            "data": serde_json::json!({
                "msgtype": "text",
                "text": { "content": "hello stream" },
                "msgId": "dt-msg-1",
                "senderId": "sender-1",
                "senderNick": "Alice",
                "conversationId": "conv-1",
                "conversationType": "2",
                "sessionWebhook": "https://oapi.dingtalk.com/robot/sendBySession?session=abc",
                "sessionWebhookExpiredTime": 1735689600000u64
            }).to_string()
        })
        .to_string();
        let (inbound_tx, inbound_rx, _) = new_inbound_channel(4);
        let session_store = Arc::new(Mutex::new(HashMap::new()));

        let outcome =
            super::handle_stream_frame(&frame, &inbound_tx, &session_store).expect("frame");

        let reply = outcome.reply.expect("ack");
        let reply_json: serde_json::Value = serde_json::from_str(&reply).expect("reply json");
        assert_eq!(reply_json["code"], 200);
        assert_eq!(reply_json["headers"]["messageId"], "stream-msg-1");
        assert_eq!(reply_json["data"], r#"{"response":null}"#);

        let msg = inbound_rx.try_recv().expect("inbound");
        assert_eq!(msg.channel.as_ref(), "dingtalk");
        assert_eq!(msg.chat_id.as_ref(), "conv-1");
        assert_eq!(msg.content, "hello stream");
        assert_eq!(msg.source_transport, MessageTransport::Wss);
        assert_eq!(msg.platform_message_id, "dt-msg-1");

        let guard = session_store.lock().expect("lock");
        let stored = guard.get("conv-1").expect("session webhook");
        assert!(stored.webhook_url.contains("sendBySession"));
    }

    #[test]
    fn dingtalk_stream_system_frame_builds_pong() {
        let frame = serde_json::json!({
            "type": "SYSTEM",
            "headers": {
                "messageId": "system-1",
                "contentType": "application/json"
            },
            "data": ""
        })
        .to_string();
        let (inbound_tx, inbound_rx, _) = new_inbound_channel(4);
        let session_store = Arc::new(Mutex::new(HashMap::new()));

        let outcome =
            super::handle_stream_frame(&frame, &inbound_tx, &session_store).expect("frame");

        let reply = outcome.reply.expect("pong");
        let reply_json: serde_json::Value = serde_json::from_str(&reply).expect("reply json");
        assert_eq!(reply_json["code"], 200);
        assert_eq!(reply_json["headers"]["messageId"], "system-1");
        assert!(inbound_rx.try_recv().is_err());
    }
}
