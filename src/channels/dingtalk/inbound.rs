//! 钉钉 Stream 入站：解析机器人消息 callback data，缓存 sessionWebhook，并按官方消息体入队。

use crate::bus::{
    AssetSourcePlatform, AudioBody, CanonicalMessageBody, CardBody, CardFormat, FileBody,
    ImageBody, MediaAssetRef, MessageTransport, PcMsg, TextBody, UserInboundTx, VideoBody,
};
use crate::channels::inbound_backpressure::{self, EventIngressSource, InboundBackpressureOutcome};
use crate::error::Result;
use serde_json::Value;

const TAG: &str = "dingtalk_stream";

/// 钉钉回调请求体核心字段（仅解析需要的部分）。
#[derive(serde::Deserialize)]
struct DingtalkCallbackBody {
    #[serde(default, rename = "msgtype")]
    msg_type: String,
    #[serde(default)]
    text: Option<DingtalkText>,
    #[serde(default)]
    content: Option<Value>,
    #[serde(default, rename = "msgId")]
    msg_id: Option<String>,
    #[serde(default, rename = "senderId")]
    sender_id: Option<String>,
    #[serde(default, rename = "senderNick")]
    sender_nick: Option<String>,
    #[serde(default, rename = "conversationId")]
    conversation_id: Option<String>,
    #[serde(default, rename = "conversationType")]
    conversation_type: Option<String>,
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

fn content_field<'a>(content: &'a Option<Value>, field: &str) -> Option<&'a Value> {
    content.as_ref()?.get(field)
}

fn content_string(content: &Option<Value>, field: &str) -> Option<String> {
    content_field(content, field)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn content_u32(content: &Option<Value>, field: &str) -> Option<u32> {
    match content_field(content, field) {
        Some(Value::Number(number)) => number.as_u64().and_then(|value| value.try_into().ok()),
        Some(Value::String(text)) => text.trim().parse::<u32>().ok(),
        _ => None,
    }
}

fn platform_handle(locator: Option<String>) -> Option<MediaAssetRef> {
    locator
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(|value| MediaAssetRef::platform_handle(AssetSourcePlatform::DingTalk, value))
}

fn rich_text_fallback(content: &Option<Value>) -> String {
    let Some(items) = content_field(content, "richText").and_then(Value::as_array) else {
        return String::new();
    };
    let mut parts = Vec::new();
    for item in items {
        if let Some(text) = item.get("text").and_then(Value::as_str) {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                parts.push(trimmed.to_string());
            }
            continue;
        }
        if item.get("type").and_then(Value::as_str) == Some("picture") {
            parts.push("[image]".to_string());
        }
    }
    parts.join("\n")
}

fn build_body(cb: &DingtalkCallbackBody) -> Result<Option<CanonicalMessageBody>> {
    let msg_type = cb.msg_type.trim();
    let normalized = if msg_type.is_empty() && cb.text.is_some() {
        "text".to_string()
    } else {
        msg_type.to_ascii_lowercase()
    };
    if normalized.is_empty() {
        return Ok(None);
    }
    let body = match normalized.as_str() {
        "text" => {
            let content = cb
                .text
                .as_ref()
                .map(|text| text.content.trim().to_string())
                .unwrap_or_default();
            if content.is_empty() {
                return Ok(None);
            }
            CanonicalMessageBody::Text(TextBody::plain(content))
        }
        "picture" => {
            let Some(asset) = platform_handle(
                content_string(&cb.content, "pictureDownloadCode")
                    .or_else(|| content_string(&cb.content, "downloadCode")),
            ) else {
                return Ok(None);
            };
            CanonicalMessageBody::Image(ImageBody {
                asset,
                caption: None,
            })
        }
        "audio" => {
            let Some(asset) = platform_handle(content_string(&cb.content, "downloadCode")) else {
                return Ok(None);
            };
            CanonicalMessageBody::Audio(AudioBody {
                asset,
                caption: None,
                transcript_text: content_string(&cb.content, "recognition"),
            })
        }
        "video" => {
            let Some(mut asset) = platform_handle(content_string(&cb.content, "downloadCode"))
            else {
                return Ok(None);
            };
            if let Some(video_type) = content_string(&cb.content, "videoType") {
                asset.mime_type = Some(if video_type.contains('/') {
                    video_type
                } else {
                    format!("video/{video_type}")
                });
            }
            asset.duration_ms =
                content_u32(&cb.content, "duration").map(|seconds| seconds.saturating_mul(1000));
            CanonicalMessageBody::Video(VideoBody {
                asset,
                caption: None,
                title: None,
                description: None,
            })
        }
        "file" => {
            let Some(mut asset) = platform_handle(
                content_string(&cb.content, "fileId")
                    .or_else(|| content_string(&cb.content, "downloadCode")),
            ) else {
                return Ok(None);
            };
            asset.file_name = content_string(&cb.content, "fileName");
            CanonicalMessageBody::File(FileBody {
                asset,
                caption: None,
            })
        }
        "richtext" => CanonicalMessageBody::Card(CardBody {
            format: CardFormat::RichPost,
            payload_json: cb.content.clone().unwrap_or(Value::Null),
            fallback_text: rich_text_fallback(&cb.content),
        }),
        _ => {
            log::debug!("[{}] unsupported msgtype={}, skip", TAG, msg_type);
            return Ok(None);
        }
    };
    Ok(Some(body))
}

fn handle_with_transport(
    body: &str,
    inbound_tx: &UserInboundTx,
    session_store: &super::DingtalkSessionStore,
    source_transport: MessageTransport,
) -> Result<bool> {
    let cb: DingtalkCallbackBody = serde_json::from_str(body).map_err(|e| {
        log::warn!("[{}] parse body failed: {}", TAG, e);
        crate::error::Error::config("dingtalk_stream", e.to_string())
    })?;

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
        .is_some_and(|id| !id.is_empty())
        || cb.conversation_type.as_deref() == Some("2");
    if let Some(session_webhook) = cb.session_webhook.as_deref() {
        super::store_session_webhook(
            session_store,
            chat_id,
            session_webhook,
            cb.session_webhook_expired_time,
        )?;
    }
    let Some(body) = build_body(&cb)? else {
        log::debug!("[{}] empty or unsupported body, skip", TAG);
        return Ok(true);
    };

    let sender = cb.sender_nick.as_deref().unwrap_or("unknown");
    log::info!(
        "[{}] received from sender={} chat_id={} kind={:?} len={}",
        TAG,
        sender,
        chat_id,
        body.kind(),
        body.text_projection().len()
    );

    let msg_id = cb.msg_id.as_deref().unwrap_or("").trim();
    let inbound_dedup_key = if msg_id.is_empty() {
        String::new()
    } else {
        format!("dingtalk_message:{msg_id}")
    };
    let msg = PcMsg::new_inbound_with_body("dingtalk", chat_id, body, is_group)?
        .with_inbound_provenance(source_transport, msg_id, "", inbound_dedup_key);
    match inbound_tx.try_submit_user(
        msg,
        crate::runtime::RuntimeForegroundSource::ExternalUserMessage,
    ) {
        Ok(()) => {
            inbound_backpressure::record_enqueued(EventIngressSource::WssGateway);
            Ok(true)
        }
        Err(std::sync::mpsc::TrySendError::Full(_)) => {
            log::warn!("[{}] inbound queue full, skip stream ack", TAG);
            inbound_backpressure::record_queue_full_for_source(
                EventIngressSource::WssGateway,
                InboundBackpressureOutcome::RedeliveryRequested,
            );
            Ok(false)
        }
        Err(std::sync::mpsc::TrySendError::Disconnected(_)) => {
            log::warn!("[{}] inbound_tx disconnected, skip stream ack", TAG);
            inbound_backpressure::record_disconnected_drop_for_source(
                EventIngressSource::WssGateway,
            );
            Ok(false)
        }
    }
}

pub(super) fn handle_stream_callback_body(
    body: &str,
    inbound_tx: &UserInboundTx,
    session_store: &super::DingtalkSessionStore,
) -> Result<bool> {
    handle_with_transport(body, inbound_tx, session_store, MessageTransport::Wss)
}

#[cfg(test)]
mod tests {
    use super::handle_stream_callback_body;
    use crate::bus::{new_user_inbound_channel, CanonicalMessageBody};
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
        let (inbound_tx, inbound_rx, _) = new_user_inbound_channel(4);
        let session_store = Arc::new(Mutex::new(HashMap::new()));

        handle_stream_callback_body(&body, &inbound_tx, &session_store).expect("handle");

        let msg = inbound_rx.try_recv().expect("message");
        assert_eq!(msg.chat_id.as_ref(), "conv-1");
        assert!(msg.is_group);
    }

    #[test]
    fn picture_message_maps_to_image_body_and_caches_session_webhook() {
        let body = serde_json::json!({
            "msgtype": "picture",
            "content": {
                "pictureDownloadCode": "pic-code-1",
                "downloadCode": "download-code-1"
            },
            "msgId": "msg-1",
            "senderId": "user-1",
            "conversationId": "conv-1",
            "sessionWebhook": "https://oapi.dingtalk.com/robot/sendBySession?session=abc",
            "sessionWebhookExpiredTime": 1735689600000u64
        })
        .to_string();
        let (inbound_tx, inbound_rx, _) = new_user_inbound_channel(4);
        let session_store = Arc::new(Mutex::new(HashMap::new()));

        handle_stream_callback_body(&body, &inbound_tx, &session_store).expect("handle");

        let msg = inbound_rx.try_recv().expect("message");
        match msg.body {
            CanonicalMessageBody::Image(image) => {
                assert_eq!(image.asset.locator, "pic-code-1");
            }
            other => panic!("unexpected body: {:?}", other),
        }
        let guard = session_store.lock().expect("lock");
        let stored = guard.get("conv-1").expect("session");
        assert!(stored.webhook_url.contains("sendBySession"));
    }

    #[test]
    fn rich_text_message_maps_to_card_body() {
        let body = serde_json::json!({
            "msgtype": "richText",
            "content": {
                "richText": [
                    { "text": "hello" },
                    {
                        "type": "picture",
                        "pictureDownloadCode": "pic-code-2",
                        "downloadCode": "download-code-2"
                    },
                    { "text": "world" }
                ]
            },
            "senderId": "user-1"
        })
        .to_string();
        let (inbound_tx, inbound_rx, _) = new_user_inbound_channel(4);
        let session_store = Arc::new(Mutex::new(HashMap::new()));

        handle_stream_callback_body(&body, &inbound_tx, &session_store).expect("handle");

        let msg = inbound_rx.try_recv().expect("message");
        match msg.body {
            CanonicalMessageBody::Card(card) => {
                assert_eq!(card.format, crate::bus::CardFormat::RichPost);
                assert!(card.fallback_text.contains("hello"));
                assert!(card.fallback_text.contains("[image]"));
            }
            other => panic!("unexpected body: {:?}", other),
        }
    }
}
