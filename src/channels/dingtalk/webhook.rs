//! 钉钉入站 Webhook：解析应用机器人回调，缓存 sessionWebhook，并按官方消息体入队。

use crate::bus::{InboundTx, MessageTransport, PcMsg};
use crate::error::Result;

const TAG: &str = "dingtalk_webhook";

/// 钉钉回调请求体核心字段（仅解析需要的部分）。
#[derive(serde::Deserialize)]
struct DingtalkCallbackBody {
    #[serde(default)]
    text: Option<DingtalkText>,
    #[serde(default, rename = "msgId")]
    msg_id: Option<String>,
    #[serde(default, rename = "senderId")]
    sender_id: Option<String>,
    #[serde(default, rename = "senderNick")]
    sender_nick: Option<String>,
    #[serde(default, rename = "conversationId")]
    conversation_id: Option<String>,
    #[serde(default, rename = "sessionWebhook")]
    session_webhook: Option<String>,
    #[serde(default, rename = "sessionWebhookExpiredTime")]
    session_webhook_expired_time: Option<u64>,
}

#[derive(serde::Deserialize)]
struct DingtalkText {
    #[serde(default)]
    content: String,
}

/// 处理钉钉回调 body，提取消息并入队。返回 Ok(()) 表示成功入队或无需入队。
pub fn handle(
    body: &str,
    inbound_tx: &InboundTx,
    session_store: &super::DingtalkSessionStore,
) -> Result<()> {
    let cb: DingtalkCallbackBody = serde_json::from_str(body).map_err(|e| {
        log::warn!("[{}] parse body failed: {}", TAG, e);
        crate::error::Error::config("dingtalk_webhook", e.to_string())
    })?;

    let content = cb
        .text
        .as_ref()
        .map(|t| t.content.trim().to_string())
        .unwrap_or_default();
    if content.is_empty() {
        log::debug!("[{}] empty content, skip", TAG);
        return Ok(());
    }

    // chat_id: prefer conversationId (group), fallback to senderId.
    let chat_id = cb
        .conversation_id
        .as_deref()
        .filter(|s| !s.is_empty())
        .or(cb.sender_id.as_deref())
        .unwrap_or("dingtalk_default");
    let is_group = cb
        .conversation_id
        .as_deref()
        .is_some_and(|id| !id.is_empty());
    if let Some(session_webhook) = cb.session_webhook.as_deref() {
        super::store_session_webhook(
            session_store,
            chat_id,
            session_webhook,
            cb.session_webhook_expired_time,
        )?;
    }

    let sender = cb.sender_nick.as_deref().unwrap_or("unknown");
    log::info!(
        "[{}] received from sender={} chat_id={} len={}",
        TAG,
        sender,
        chat_id,
        content.len()
    );

    let msg_id = cb.msg_id.as_deref().unwrap_or("").trim();
    let inbound_dedup_key = if msg_id.is_empty() {
        String::new()
    } else {
        format!("dingtalk_message:{msg_id}")
    };
    let msg = PcMsg::new_inbound("dingtalk", chat_id, content, is_group)?.with_inbound_provenance(
        MessageTransport::Webhook,
        msg_id,
        "",
        inbound_dedup_key,
    );
    if inbound_tx.send(msg).is_err() {
        log::warn!("[{}] inbound_tx send failed (queue full?)", TAG);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::handle;
    use crate::bus::new_inbound_channel;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    #[test]
    fn conversation_id_is_marked_as_group_message() {
        let body = serde_json::json!({
            "text": { "content": "hello" },
            "senderId": "user-1",
            "conversationId": "conv-1"
        })
        .to_string();
        let (inbound_tx, inbound_rx, _) = new_inbound_channel(4);
        let session_store = Arc::new(Mutex::new(HashMap::new()));

        handle(&body, &inbound_tx, &session_store).expect("handle");

        let msg = inbound_rx.try_recv().expect("message");
        assert_eq!(msg.chat_id.as_ref(), "conv-1");
        assert!(msg.is_group);
    }
}
