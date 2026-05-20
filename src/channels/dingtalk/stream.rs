//! DingTalk Stream Mode inbound loop.
//! 钉钉 Stream Mode 入站：注册 WSS 连接、处理系统帧与机器人消息回调。

use crate::bus::UserInboundTx;
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

fn mark_wss_lifecycle(state: crate::runtime::PlaneLifecycleState, reason: &'static str) {
    let _ = crate::runtime::plane_lifecycle::mark(
        crate::runtime::PlaneId::ChannelWss,
        TAG,
        state,
        reason,
    );
}

fn external_wss_suspend_lifecycle_reason() -> &'static str {
    crate::network::external_wss_suspend_reason()
        .map(crate::network::ExternalWssSuspendReason::as_str)
        .unwrap_or("external_wss_suspend")
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
fn esp_network_suspend_reason() -> Option<&'static str> {
    let snapshot = crate::state::network_runtime_snapshot(
        crate::platform::time::wall_clock_is_trustworthy(),
        crate::network::EXTERNAL_WSS_OUTBOUND_SETTLE_SECS,
    );
    crate::network::external_wss_network_suspend_reason(&snapshot)
}

pub fn handle_stream_frame(
    frame: &str,
    inbound_tx: &UserInboundTx,
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

fn dingtalk_stream_connect_admission() -> Option<crate::network::TransportAdmissionRejection> {
    let resource = crate::orchestrator::resource_light_snapshot();
    let context = crate::runtime::current_runtime_scheduler_context(
        crate::runtime::default_runtime_scheduler_profile(),
        resource.pressure,
    );
    dingtalk_stream_connect_admission_for_context(
        crate::runtime::thread_registry::runtime_mode_snapshot(),
        context,
    )
}

fn dingtalk_stream_connect_admission_for_context(
    mode: crate::runtime::RuntimeModeSnapshot,
    mut context: crate::runtime::RuntimeSchedulerContext,
) -> Option<crate::network::TransportAdmissionRejection> {
    context.runtime_mode = mode;
    crate::network::runtime_transport_admission_for_work(
        crate::network::TransportAdmissionKind::ExternalWssConnect,
        crate::runtime::RuntimeWorkRequest::new(
            crate::runtime::RuntimeWorkClass::ChannelReconnect,
            crate::runtime::RuntimeWorkSource::Background,
        ),
        context,
    )
    .rejection()
}

pub fn run_dingtalk_stream_loop<H, C, CreateHttp, Connect>(
    client_id: String,
    client_secret: String,
    inbound_tx: UserInboundTx,
    session_store: super::DingtalkSessionStore,
    mut create_http: CreateHttp,
    mut connect: Connect,
) where
    H: ChannelHttpClient,
    C: WssConnection,
    CreateHttp: FnMut() -> Result<H>,
    Connect: FnMut(&str) -> Result<C>,
{
    crate::network::set_external_wss_managed_present(true);
    let mut backoff_secs = crate::orchestrator::current_budget().reconnect_backoff_secs;
    loop {
        if !crate::runtime::thread_registry::runtime_mode_snapshot()
            .action_budget
            .allow_external_wss_connect
        {
            mark_wss_lifecycle(
                crate::runtime::PlaneLifecycleState::Suspended,
                "runtime_mode_gate",
            );
            std::thread::sleep(Duration::from_secs(backoff_secs));
            continue;
        }
        #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
        if let Some(reason) = esp_network_suspend_reason() {
            mark_wss_lifecycle(crate::runtime::PlaneLifecycleState::Suspended, reason);
            if reason == "wall_clock_untrusted" {
                let _ = crate::platform::time::wait_for_wall_clock_trustworthy(
                    Duration::from_secs(backoff_secs),
                );
            } else {
                let _ = crate::platform::wifi::wait_for_network_ready();
                std::thread::sleep(Duration::from_secs(backoff_secs));
            }
            continue;
        }
        if crate::network::external_wss_suspend_requested() {
            mark_wss_lifecycle(
                crate::runtime::PlaneLifecycleState::Suspended,
                external_wss_suspend_lifecycle_reason(),
            );
        }
        crate::network::wait_for_external_wss_resume(TAG);
        if let Some(rejection) = dingtalk_stream_connect_admission() {
            mark_wss_lifecycle(
                crate::runtime::PlaneLifecycleState::Suspended,
                rejection.reason,
            );
            log::info!(
                "[{}] stream connect/register deferred stage={} reason={}",
                TAG,
                rejection.stage,
                rejection.reason
            );
            std::thread::sleep(Duration::from_secs(backoff_secs));
            continue;
        }
        mark_wss_lifecycle(
            crate::runtime::PlaneLifecycleState::Starting,
            "connect_attempt",
        );
        let mut http = match create_http() {
            Ok(http) => http,
            Err(error) => {
                mark_wss_lifecycle(crate::runtime::PlaneLifecycleState::Failed, "http_create");
                log::warn!("[{}] create_http failed: {}", TAG, error);
                std::thread::sleep(Duration::from_secs(backoff_secs));
                backoff_secs = (backoff_secs * 2).min(BACKOFF_MAX_SECS);
                continue;
            }
        };
        let url = match register_connection(&mut http, &client_id, &client_secret) {
            Ok(url) => url,
            Err(error) => {
                mark_wss_lifecycle(
                    crate::runtime::PlaneLifecycleState::Failed,
                    "register_connection",
                );
                log::warn!("[{}] register connection failed: {}", TAG, error);
                std::thread::sleep(Duration::from_secs(backoff_secs));
                backoff_secs = (backoff_secs * 2).min(BACKOFF_MAX_SECS);
                continue;
            }
        };
        let mut conn = match connect(&url) {
            Ok(conn) => conn,
            Err(error) => {
                mark_wss_lifecycle(crate::runtime::PlaneLifecycleState::Failed, "connect");
                log::warn!("[{}] connect failed: {}", TAG, error);
                std::thread::sleep(Duration::from_secs(backoff_secs));
                backoff_secs = (backoff_secs * 2).min(BACKOFF_MAX_SECS);
                continue;
            }
        };
        backoff_secs = crate::orchestrator::current_budget().reconnect_backoff_secs;
        mark_wss_lifecycle(
            crate::runtime::PlaneLifecycleState::Active,
            "session_active",
        );
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
                let reason = if crate::network::external_wss_suspend_requested() {
                    external_wss_suspend_lifecycle_reason()
                } else {
                    "runtime_mode_gate"
                };
                mark_wss_lifecycle(crate::runtime::PlaneLifecycleState::Suspended, reason);
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
    use crate::bus::{new_user_inbound_channel, MessageTransport};
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
        let (inbound_tx, inbound_rx, _) = new_user_inbound_channel(4);
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
        let (inbound_tx, inbound_rx, _) = new_user_inbound_channel(4);
        let session_store = Arc::new(Mutex::new(HashMap::new()));

        let outcome =
            super::handle_stream_frame(&frame, &inbound_tx, &session_store).expect("frame");

        let reply = outcome.reply.expect("pong");
        let reply_json: serde_json::Value = serde_json::from_str(&reply).expect("reply json");
        assert_eq!(reply_json["code"], 200);
        assert_eq!(reply_json["headers"]["messageId"], "system-1");
        assert!(inbound_rx.try_recv().is_err());
    }

    #[test]
    fn dingtalk_stream_connect_admission_defers_before_register_during_foreground() {
        let mode = crate::runtime::mode::snapshot_from_source(
            crate::runtime::mode::RuntimeModeSource::default(),
        );
        let rejection = super::dingtalk_stream_connect_admission_for_context(
            mode,
            crate::runtime::RuntimeSchedulerContext {
                profile: crate::runtime::RuntimePlanePolicyProfile::EspCompact,
                runtime_mode: mode,
                foreground: crate::runtime::RuntimeForegroundOverlay {
                    active: true,
                    active_count: 1,
                    primary_source: Some(
                        crate::runtime::RuntimeForegroundSource::ExternalUserMessage,
                    ),
                    age_ms: Some(500),
                    resume_after_ms: Some(29_500),
                    ..crate::runtime::RuntimeForegroundOverlay::default()
                },
                pressure: crate::orchestrator::PressureLevel::Normal,
            },
        )
        .expect("foreground should defer dingtalk stream register/connect");

        assert_eq!(rejection.stage, "transport_runtime_scheduler");
        assert_eq!(rejection.reason, "foreground_active");
    }
}
