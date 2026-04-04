//! QQ 频道/群聊/私聊：入站 HTTP 回调（验签）与 WSS 入站；出站 Sink/flush、msg_id 被动回复，连通性检查。
//! 支持 AT_MESSAGE_CREATE（频道）、GROUP_AT_MESSAGE_CREATE（群聊）、C2C_MESSAGE_CREATE（私聊）。

mod send;
mod token;
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
mod webhook;

mod ws;

pub use send::{check_connectivity, flush_qq_channel_sends, run_qq_sender_loop, QqMsgIdCache};
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub use webhook::{handle_webhook, QqHandlerResult, QQ_WEBHOOK_BODY_MAX};

pub use ws::run_qq_ws_loop;
