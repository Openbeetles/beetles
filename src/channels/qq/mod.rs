//! QQ 频道/群聊/私聊：入站 HTTP 回调（验签）与 WSS 入站；出站 Sink/flush、msg_id 被动回复，连通性检查。
//! 支持 AT_MESSAGE_CREATE（频道）、GROUP_AT_MESSAGE_CREATE（群聊）、C2C_MESSAGE_CREATE（私聊）。

use crate::bus::{MessageTransport, PcMsg};
use crate::error::Result;

mod msg_id;
mod send;
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
mod signature;
mod token;
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
mod webhook;

mod ws;

pub use msg_id::QqMsgIdCache;
pub use send::{check_connectivity, flush_qq_channel_sends, run_qq_sender_loop};
pub use token::{new_shared_qq_token_cache, SharedQqTokenCache};
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub use webhook::{handle_webhook, QqHandlerResult, QQ_WEBHOOK_BODY_MAX};

pub use ws::{run_qq_ws_loop, QqWsLoopConfig};

pub(crate) fn build_inbound_message(
    chat_id: &str,
    content: &str,
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
    Ok(PcMsg::new_inbound(
        "qq_channel",
        chat_id,
        content,
        chat_id.starts_with("group:"),
    )?
    .with_inbound_provenance(
        source_transport,
        platform_message_id,
        platform_event_id,
        inbound_dedup_key,
    ))
}
