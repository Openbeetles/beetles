//! 飞书 HTTP 事件回调逻辑：url_verification、encrypt 检查、allowed、event_body_to_pcmsg、入队。
//! 返回类型由 handler 层转换为 ApiResponse，避免 channels 依赖 platform::http_server。

use crate::bus::InboundTx;
use crate::config::{AppConfig, parse_allowed_chat_ids};

use super::send::event_body_to_pcmsg;

const TAG: &str = "feishu_event";

#[derive(serde::Deserialize)]
struct FeishuHttpEventEnvelope {
    #[serde(default)]
    r#type: Option<String>,
    #[serde(default)]
    challenge: Option<String>,
    #[serde(default)]
    encrypt: Option<String>,
}

/// 事件处理结果，由 handler 转为 ApiResponse 写响应。
#[derive(Debug)]
pub enum FeishuEventResponse {
    Ok200Json(String),
    Err400(&'static str),
    Err404(&'static str),
}

/// 处理 POST body，返回响应描述。不校验配对码（飞书服务器不会带）。
/// config 由调用方从 ctx 加载后传入，避免 channels 依赖 HandlerContext。
pub fn handle_http_event(
    config: &AppConfig,
    inbound_tx: &InboundTx,
    body: &str,
) -> FeishuEventResponse {
    if config.feishu_app_id.trim().is_empty() || config.feishu_app_secret.trim().is_empty() {
        return FeishuEventResponse::Err404("feishu not configured");
    }

    let v: FeishuHttpEventEnvelope = match serde_json::from_str(body) {
        Ok(x) => x,
        Err(e) => {
            log::warn!("[{}] parse json: {}", TAG, e);
            return FeishuEventResponse::Err400("invalid json");
        }
    };

    if v.r#type.as_deref() == Some("url_verification") {
        let challenge = v.challenge.as_deref().unwrap_or("");
        let mut out = String::with_capacity(challenge.len() + 24);
        out.push_str("{\"challenge\":");
        crate::util::push_json_string_escaped(&mut out, challenge);
        out.push('}');
        return FeishuEventResponse::Ok200Json(out);
    }

    if v.encrypt.is_some() {
        log::warn!(
            "[{}] encrypted payload not supported, returning error to trigger retry",
            TAG
        );
        return FeishuEventResponse::Err400("encrypted payload not supported");
    }

    let allowed = parse_allowed_chat_ids(&config.feishu_allowed_chat_ids);
    if allowed.is_empty() {
        log::debug!("[{}] feishu_allowed_chat_ids empty, drop message", TAG);
        return FeishuEventResponse::Ok200Json("{}".into());
    }
    let msg = match event_body_to_pcmsg(body, &allowed) {
        Some(m) => m,
        None => return FeishuEventResponse::Ok200Json("{}".into()),
    };
    if inbound_tx.send(msg).is_err() {
        log::warn!("[{}] inbound queue full", TAG);
    }
    FeishuEventResponse::Ok200Json("{}".into())
}
