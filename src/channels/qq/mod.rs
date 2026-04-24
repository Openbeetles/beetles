//! QQ 频道/群聊/私聊：WSS 入站；出站 Sink/flush、msg_id 被动回复，连通性检查。
//! 支持 AT_MESSAGE_CREATE（频道）、GROUP_AT_MESSAGE_CREATE（群聊）、C2C_MESSAGE_CREATE（私聊）。

use crate::bus::{
    CanonicalMessageBody, CardBody, CardFormat, MessageTransport, PcMsg, TextBody, TextFormat,
};
use crate::error::Result;
use serde_json::Value;

mod msg_id;
mod send;
mod status;
mod token;

mod ws;

pub use msg_id::{QqInboundDedupStore, QqMsgIdCache};
pub use send::{check_connectivity, flush_qq_channel_sends, run_qq_sender_loop};
pub use status::{is_ws_online, new_shared_qq_ws_status, SharedQqWsStatus};
pub use token::{new_shared_qq_token_cache, SharedQqTokenCache};

pub use ws::{run_qq_ws_loop, QqWsLoopConfig};

pub(crate) fn build_inbound_body_from_parts(
    content: Option<&str>,
    markdown: Option<&Value>,
    ark: Option<&Value>,
    embed: Option<&Value>,
) -> (CanonicalMessageBody, String) {
    let content = content.unwrap_or("").trim();
    if let Some(markdown) = markdown {
        let markdown_content = markdown
            .get("content")
            .and_then(|value| value.as_str())
            .unwrap_or(content)
            .trim()
            .to_string();
        return (
            CanonicalMessageBody::Text(TextBody {
                text: markdown_content.clone(),
                format: TextFormat::Markdown,
            }),
            markdown_content,
        );
    }
    if let Some(ark) = ark {
        let fallback = if content.is_empty() {
            "[ark]".to_string()
        } else {
            content.to_string()
        };
        return (
            CanonicalMessageBody::Card(CardBody {
                format: CardFormat::Ark,
                payload_json: ark.clone(),
                fallback_text: fallback.clone(),
            }),
            fallback,
        );
    }
    if let Some(embed) = embed {
        let fallback = if content.is_empty() {
            "[embed]".to_string()
        } else {
            content.to_string()
        };
        return (
            CanonicalMessageBody::Card(CardBody {
                format: CardFormat::Embed,
                payload_json: embed.clone(),
                fallback_text: fallback.clone(),
            }),
            fallback,
        );
    }
    (CanonicalMessageBody::text(content), content.to_string())
}

pub(crate) fn build_inbound_message_with_body(
    chat_id: &str,
    content_projection: &str,
    body: CanonicalMessageBody,
    source_transport: MessageTransport,
    platform_message_id: Option<&str>,
    platform_event_id: Option<&str>,
) -> Result<PcMsg> {
    let platform_message_id = platform_message_id.unwrap_or("").trim();
    let platform_event_id = platform_event_id.unwrap_or("").trim();
    let inbound_dedup_key = if !platform_message_id.is_empty() {
        format!("qq_message:{platform_message_id}")
    } else if !platform_event_id.is_empty() {
        format!("qq_event:{platform_event_id}")
    } else {
        String::new()
    };
    Ok(PcMsg::new_inbound_with_body_and_ingress(
        "qq_channel",
        chat_id,
        body,
        content_projection,
        chat_id.starts_with("group:"),
        crate::bus::IngressKind::User,
    )?
    .with_inbound_provenance(
        source_transport,
        platform_message_id,
        platform_event_id,
        inbound_dedup_key,
    )
    .with_platform_thread_id(""))
}
