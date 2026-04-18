//! QQ 入站 HTTP 回调：op=13 验址、op=0 Ed25519 验签，支持 AT_MESSAGE_CREATE / GROUP_AT_MESSAGE_CREATE / C2C_MESSAGE_CREATE 入队。

use super::msg_id::{cache_msg_id, consume_inbound_dedup_key, QqInboundDedupStore, QqMsgIdCache};
use super::signature::{sign_qq_url_verify, verify_qq_signature};
use crate::bus::InboundTx;
use crate::error::{Error, Result};

/// Body 最大字节（拒绝超长请求）。与 http_server 读 body 上限一致，单一数据源。
pub const QQ_WEBHOOK_BODY_MAX: usize = 64 * 1024;

/// 入站处理结果，供 HTTP 层写响应。
pub enum QqHandlerResult {
    /// op=13：需返回 200 且 body {"plain_token":"...","signature":"..."}
    UrlVerification {
        plain_token: String,
        signature: String,
    },
    /// 已处理（含 op=0 或其他 op），返回 200 空 body 或 ACK。
    EventHandled,
}

#[derive(serde::Deserialize)]
struct QqWebhookEnvelope {
    op: u64,
    #[serde(default)]
    t: Option<String>,
    #[serde(default)]
    d: Option<QqWebhookData>,
}

#[derive(serde::Deserialize, Default)]
struct QqWebhookData {
    #[serde(default)]
    plain_token: Option<String>,
    #[serde(default)]
    event_ts: Option<String>,
    #[serde(default)]
    channel_id: Option<String>,
    #[serde(default)]
    group_openid: Option<String>,
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    author: Option<QqWebhookAuthor>,
}

#[derive(serde::Deserialize, Default)]
struct QqWebhookAuthor {
    #[serde(default)]
    user_openid: Option<String>,
}

/// 处理 QQ 回调 body，完成验签/解析/入队；不读 HTTP Header，由调用方传入 timestamp 与 signature。
/// 返回 Ok(result) 时由调用方写 200；Err 时写 401/413 等。
#[allow(clippy::too_many_arguments)]
pub fn handle_webhook(
    body: &[u8],
    signature_timestamp: Option<&str>,
    signature_ed25519: Option<&str>,
    _app_id: &str,
    secret: &str,
    inbound_tx: &InboundTx,
    msg_id_cache: QqMsgIdCache,
    inbound_dedup_store: QqInboundDedupStore,
) -> Result<QqHandlerResult> {
    if body.len() > QQ_WEBHOOK_BODY_MAX {
        return Err(Error::config("qq_webhook", "body too large"));
    }
    let value: QqWebhookEnvelope =
        serde_json::from_slice(body).map_err(|e| Error::config("qq_json", e.to_string()))?;

    if value.op == 13 {
        let d = value
            .d
            .as_ref()
            .ok_or_else(|| Error::config("qq_op13", "missing d"))?;
        let plain_token = d
            .plain_token
            .clone()
            .ok_or_else(|| Error::config("qq_op13", "missing plain_token"))?;
        let event_ts = d.event_ts.as_deref().unwrap_or("").to_string();
        let signature = sign_qq_url_verify(secret, &event_ts, &plain_token)?;
        return Ok(QqHandlerResult::UrlVerification {
            plain_token,
            signature,
        });
    }

    if value.op == 0 {
        let ts = signature_timestamp
            .ok_or_else(|| Error::config("qq_op0", "missing X-Signature-Timestamp"))?;
        let sig = signature_ed25519
            .ok_or_else(|| Error::config("qq_op0", "missing X-Signature-Ed25519"))?;
        verify_qq_signature(secret, ts, body, sig)?;

        let t = value.t.as_deref().unwrap_or("");
        let d = value.d.as_ref();
        if let Some(d) = d {
            let (chat_id, content, msg_id) = match t {
                "AT_MESSAGE_CREATE" => {
                    // 频道消息：chat_id = channel_id
                    let ch = d.channel_id.clone();
                    let ct = d.content.clone();
                    let mid = d.id.clone();
                    (ch, ct, mid)
                }
                "GROUP_AT_MESSAGE_CREATE" => {
                    // 群聊 @ 消息：chat_id = "group:{group_openid}"
                    let gid = d.group_openid.as_deref().map(|s| format!("group:{}", s));
                    let ct = d.content.clone();
                    let mid = d.id.clone();
                    (gid, ct, mid)
                }
                "C2C_MESSAGE_CREATE" => {
                    // C2C 单聊：与 WSS/发送链路保持一致，统一用 author.user_openid 作为 chat_id。
                    let uid = d
                        .author
                        .as_ref()
                        .and_then(|a| a.user_openid.as_deref())
                        .map(|s| format!("c2c:{}", s));
                    let ct = d.content.clone();
                    let mid = d.id.clone();
                    (uid, ct, mid)
                }
                _ => (None, None, None),
            };
            if let (Some(id), Some(ch), Some(content)) = (msg_id, chat_id, content) {
                if !ch.is_empty() && !content.is_empty() {
                    cache_msg_id(&msg_id_cache, &ch, &id)?;
                    let msg = super::build_inbound_message(
                        &ch,
                        &content,
                        crate::bus::MessageTransport::Webhook,
                        Some(&id),
                        None,
                    )?;
                    if consume_inbound_dedup_key(&inbound_dedup_store, &msg.inbound_dedup_key)? {
                        return Ok(QqHandlerResult::EventHandled);
                    }
                    inbound_tx.send(msg).map_err(|e| Error::Other {
                        source: Box::new(e),
                        stage: "qq_inbound_send",
                    })?;
                }
            }
        }
        return Ok(QqHandlerResult::EventHandled);
    }

    Ok(QqHandlerResult::EventHandled)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bus::new_inbound_channel;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    fn sign_event(secret: &str, timestamp: &str, body: &[u8]) -> String {
        sign_qq_url_verify(secret, timestamp, std::str::from_utf8(body).unwrap()).unwrap()
    }

    #[test]
    fn c2c_webhook_uses_user_openid_chat_id() {
        let secret = "qq-test-secret";
        let timestamp = "1711936800";
        let body = serde_json::json!({
            "op": 0,
            "t": "C2C_MESSAGE_CREATE",
            "d": {
                "id": "msg-1",
                "guild_id": "guild-legacy",
                "content": "hello",
                "author": {
                    "user_openid": "user-openid-42"
                }
            }
        });
        let body_bytes = serde_json::to_vec(&body).unwrap();
        let signature = sign_event(secret, timestamp, &body_bytes);
        let (inbound_tx, inbound_rx, _) = new_inbound_channel(4);
        let cache: QqMsgIdCache = Arc::new(Mutex::new(HashMap::new()));
        let dedup_store: QqInboundDedupStore = Arc::new(Mutex::new(HashMap::new()));

        let result = handle_webhook(
            &body_bytes,
            Some(timestamp),
            Some(&signature),
            "",
            secret,
            &inbound_tx,
            Arc::clone(&cache),
            Arc::clone(&dedup_store),
        )
        .unwrap();

        assert!(matches!(result, QqHandlerResult::EventHandled));
        let msg = inbound_rx.try_recv().unwrap();
        assert_eq!(msg.channel.as_ref(), "qq_channel");
        assert_eq!(msg.chat_id.as_ref(), "c2c:user-openid-42");
        assert_eq!(msg.content, "hello");
        assert_eq!(msg.source_transport, crate::bus::MessageTransport::Webhook);
        assert_eq!(msg.platform_message_id, "msg-1");
        assert_eq!(msg.inbound_dedup_key, "qq_message:msg-1");
        let cached = cache.lock().unwrap();
        assert_eq!(
            cached.get("c2c:user-openid-42").map(|(id, _)| id.as_str()),
            Some("msg-1")
        );
        assert!(!cached.contains_key("c2c:guild-legacy"));
    }

    #[test]
    fn c2c_webhook_requires_user_openid_for_dispatch() {
        let secret = "qq-test-secret";
        let timestamp = "1711936800";
        let body = serde_json::json!({
            "op": 0,
            "t": "C2C_MESSAGE_CREATE",
            "d": {
                "id": "msg-2",
                "guild_id": "guild-only",
                "content": "hello"
            }
        });
        let body_bytes = serde_json::to_vec(&body).unwrap();
        let signature = sign_event(secret, timestamp, &body_bytes);
        let (inbound_tx, inbound_rx, _) = new_inbound_channel(4);
        let cache: QqMsgIdCache = Arc::new(Mutex::new(HashMap::new()));
        let dedup_store: QqInboundDedupStore = Arc::new(Mutex::new(HashMap::new()));

        let result = handle_webhook(
            &body_bytes,
            Some(timestamp),
            Some(&signature),
            "",
            secret,
            &inbound_tx,
            Arc::clone(&cache),
            Arc::clone(&dedup_store),
        )
        .unwrap();

        assert!(matches!(result, QqHandlerResult::EventHandled));
        assert!(inbound_rx.try_recv().is_err());
        assert!(cache.lock().unwrap().is_empty());
    }

    #[test]
    fn group_webhook_marks_message_as_group() {
        let secret = "qq-test-secret";
        let timestamp = "1711936800";
        let body = serde_json::json!({
            "op": 0,
            "t": "GROUP_AT_MESSAGE_CREATE",
            "d": {
                "id": "msg-group-1",
                "group_openid": "group-openid-42",
                "content": "@beetle hi"
            }
        });
        let body_bytes = serde_json::to_vec(&body).unwrap();
        let signature = sign_event(secret, timestamp, &body_bytes);
        let (inbound_tx, inbound_rx, _) = new_inbound_channel(4);
        let cache: QqMsgIdCache = Arc::new(Mutex::new(HashMap::new()));
        let dedup_store: QqInboundDedupStore = Arc::new(Mutex::new(HashMap::new()));

        let result = handle_webhook(
            &body_bytes,
            Some(timestamp),
            Some(&signature),
            "",
            secret,
            &inbound_tx,
            Arc::clone(&cache),
            Arc::clone(&dedup_store),
        )
        .unwrap();

        assert!(matches!(result, QqHandlerResult::EventHandled));
        let msg = inbound_rx.try_recv().unwrap();
        assert_eq!(msg.chat_id.as_ref(), "group:group-openid-42");
        assert!(msg.is_group);
    }
}
