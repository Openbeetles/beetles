//! 飞书 HTTP 事件回调逻辑：url_verification、token/signature 校验、解密、幂等、入队。
//! 返回类型由 handler 层转换为 ApiResponse，避免 channels 依赖 platform::http_server。

use crate::bus::InboundTx;
use crate::config::{parse_allowed_chat_ids, AppConfig};

use super::dedup::{consume_message_id, FeishuMessageDedupStore};
use super::send::event_body_to_pcmsg_with_transport;

const TAG: &str = "feishu_event";

#[derive(Clone, Copy, Debug)]
pub struct FeishuRequestHeaders<'a> {
    pub signature: &'a str,
    pub timestamp: &'a str,
    pub nonce: &'a str,
}

#[derive(serde::Deserialize)]
struct FeishuHttpEventEnvelope {
    #[serde(default)]
    r#type: Option<String>,
    #[serde(default)]
    token: Option<String>,
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

fn verify_signature(headers: FeishuRequestHeaders<'_>, encrypt_key: &str, body: &str) -> bool {
    if encrypt_key.trim().is_empty()
        || headers.signature.trim().is_empty()
        || headers.timestamp.trim().is_empty()
        || headers.nonce.trim().is_empty()
    {
        return false;
    }
    let mut material = String::with_capacity(
        headers.timestamp.len() + headers.nonce.len() + encrypt_key.len() + body.len(),
    );
    material.push_str(headers.timestamp);
    material.push_str(headers.nonce);
    material.push_str(encrypt_key);
    material.push_str(body);
    let computed = hex::encode(crate::channels::crypto::sha256_bytes(material.as_bytes()));
    crate::util::constant_time_eq(&computed, headers.signature.trim())
}

fn decrypt_event_payload(encrypt_key: &str, encrypted: &str) -> Result<String, ()> {
    use base64::Engine as _;

    let decoded = base64::engine::general_purpose::STANDARD
        .decode(encrypted.as_bytes())
        .map_err(|_| ())?;
    if decoded.len() < 16 {
        return Err(());
    }
    let mut iv = [0u8; 16];
    iv.copy_from_slice(&decoded[..16]);
    let key = crate::channels::crypto::sha256_bytes(encrypt_key.as_bytes());
    let plain = crate::channels::crypto::aes256_cbc_decrypt_pkcs7(&key, &iv, &decoded[16..], TAG)
        .map_err(|_| ())?;
    String::from_utf8(plain).map_err(|_| ())
}

fn validate_verification_token(
    config: &AppConfig,
    envelope: &FeishuHttpEventEnvelope,
    body: &serde_json::Value,
) -> bool {
    let incoming = envelope
        .token
        .as_deref()
        .or_else(|| {
            body.get("header")
                .and_then(|header| header.get("token"))
                .and_then(|token| token.as_str())
        })
        .or_else(|| body.get("token").and_then(|token| token.as_str()))
        .unwrap_or("");
    let expected = config.feishu_verification_token.trim();
    if expected.is_empty() {
        return !config.feishu_encrypt_key.trim().is_empty();
    }
    !incoming.is_empty() && crate::util::constant_time_eq(incoming, expected)
}

/// 处理 POST body，返回响应描述。config 由调用方从 ctx 加载后传入，避免 channels 依赖 HandlerContext。
pub fn handle_http_event(
    config: &AppConfig,
    inbound_tx: &InboundTx,
    dedup_store: &FeishuMessageDedupStore,
    headers: FeishuRequestHeaders<'_>,
    body: &str,
) -> FeishuEventResponse {
    if config.feishu_app_id.trim().is_empty() || config.feishu_app_secret.trim().is_empty() {
        return FeishuEventResponse::Err404("feishu not configured");
    }

    let payload = if config.feishu_encrypt_key.trim().is_empty() {
        body.to_string()
    } else {
        if !verify_signature(headers, &config.feishu_encrypt_key, body) {
            log::warn!("[{}] invalid encrypted callback signature", TAG);
            return FeishuEventResponse::Err400("invalid signature");
        }
        let encrypted_envelope: FeishuHttpEventEnvelope = match serde_json::from_str(body) {
            Ok(x) => x,
            Err(e) => {
                log::warn!("[{}] parse encrypted json: {}", TAG, e);
                return FeishuEventResponse::Err400("invalid json");
            }
        };
        let Some(encrypted) = encrypted_envelope.encrypt.as_deref() else {
            log::warn!("[{}] missing encrypt field", TAG);
            return FeishuEventResponse::Err400("missing encrypt");
        };
        match decrypt_event_payload(&config.feishu_encrypt_key, encrypted) {
            Ok(payload) => payload,
            Err(()) => {
                log::warn!("[{}] failed to decrypt event payload", TAG);
                return FeishuEventResponse::Err400("decrypt failed");
            }
        }
    };

    let envelope: FeishuHttpEventEnvelope = match serde_json::from_str(&payload) {
        Ok(x) => x,
        Err(e) => {
            log::warn!("[{}] parse json: {}", TAG, e);
            return FeishuEventResponse::Err400("invalid json");
        }
    };
    let body_json: serde_json::Value = match serde_json::from_str(&payload) {
        Ok(value) => value,
        Err(e) => {
            log::warn!("[{}] parse body json: {}", TAG, e);
            return FeishuEventResponse::Err400("invalid json");
        }
    };
    if !validate_verification_token(config, &envelope, &body_json) {
        log::warn!("[{}] verification token mismatch", TAG);
        return FeishuEventResponse::Err400("invalid token");
    }

    if envelope.r#type.as_deref() == Some("url_verification") {
        let challenge = envelope.challenge.as_deref().unwrap_or("");
        let mut out = String::with_capacity(challenge.len() + 24);
        out.push_str("{\"challenge\":");
        crate::util::push_json_string_escaped(&mut out, challenge);
        out.push('}');
        return FeishuEventResponse::Ok200Json(out);
    }

    let allowed = parse_allowed_chat_ids(&config.feishu_allowed_chat_ids);
    if allowed.is_empty() {
        log::debug!("[{}] feishu_allowed_chat_ids empty, drop message", TAG);
        return FeishuEventResponse::Ok200Json("{}".into());
    }
    let msg = match event_body_to_pcmsg_with_transport(
        &payload,
        &allowed,
        crate::bus::MessageTransport::Webhook,
    ) {
        Some(m) => m,
        None => return FeishuEventResponse::Ok200Json("{}".into()),
    };
    match consume_message_id(dedup_store, &msg.platform_message_id) {
        Ok(true) => {
            log::info!(
                "[{}] duplicate message_id={}, ack only",
                TAG,
                msg.platform_message_id
            );
            return FeishuEventResponse::Ok200Json("{}".into());
        }
        Ok(false) => {}
        Err(error) => {
            log::warn!("[{}] dedup store failed: {}", TAG, error);
            return FeishuEventResponse::Err400("dedup failed");
        }
    }
    if inbound_tx.send(msg).is_err() {
        log::warn!("[{}] inbound queue full", TAG);
    }
    FeishuEventResponse::Ok200Json("{}".into())
}
