//! 通道抽象与出站分发。仅依赖 bus、error、config；不依赖 agent、llm、tools。
//! Channel sink trait and types; dispatch consumes outbound and sends to sinks.

mod chunk;
mod connectivity;
pub(crate) mod dingtalk;
mod dispatch;
pub(crate) mod feishu;
mod http_client;
mod qq;
mod send;
pub(crate) mod telegram;
pub(crate) mod voice_sink;
mod websocket;
pub(crate) mod wecom;
mod wss_gateway;

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
pub use connectivity::build_unavailable_snapshot;
pub use connectivity::{ChannelConnectivityItem, ChannelConnectivitySnapshot, build_snapshot};
pub use dingtalk::{flush_dingtalk_sends, run_dingtalk_sender_loop};
pub use dispatch::{ChannelRxSet, build_channel_sinks, spawn_sender_threads};
pub use dispatch::{ChannelSinks, MessageSink, QueuedSink, run_dispatch};
#[cfg(feature = "feishu")]
pub use feishu::run_feishu_ws_loop;
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub use feishu::{FeishuEventResponse, handle_http_event};
pub use feishu::{
    acquire_tenant_token as feishu_acquire_token, event_body_to_pcmsg, feishu_edit_message,
    feishu_send_and_get_id, flush_feishu_sends, run_feishu_sender_loop,
};
pub use http_client::ChannelHttpClient;
pub use qq::run_qq_ws_loop;
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub use qq::{QQ_WEBHOOK_BODY_MAX, QqHandlerResult, handle_webhook};
pub use qq::{QqMsgIdCache, flush_qq_channel_sends, run_qq_sender_loop};

pub use telegram::{
    TelegramCommandCtx, edit_message_text as tg_edit_message_text, flush_telegram_sends,
    get_bot_username, poll_telegram_once, run_telegram_poll_loop, run_telegram_sender_loop,
    send_chat_action, tg_send_and_get_id,
};
pub use voice_sink::VoiceSink;
pub use websocket::{MAX_WS_CONNECTIONS, MAX_WS_MESSAGE_LEN, WebSocketSink};
pub use wecom::{flush_wecom_sends, run_wecom_sender_loop};
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
pub use wss_gateway::{EspWssConnection, connect_esp_wss};
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub use wss_gateway::{LinuxWssConnection, connect_linux_wss};
pub use wss_gateway::{
    WssCloseInfo, WssConnectProfile, WssConnection, WssEvent, connect_wss,
    connect_wss_with_headers, connect_wss_with_headers_and_profile,
};

/// 占位 sink：打日志并返回 Ok，供 8.1 验收。
pub struct LogSink {
    pub tag: String,
}

impl LogSink {
    pub fn new(tag: &str) -> Self {
        Self {
            tag: tag.to_string(),
        }
    }
}

impl MessageSink for LogSink {
    fn send(&self, chat_id: &str, content: &str) -> crate::error::Result<()> {
        log::info!(
            "[{}] send chat_id={} content_len={}",
            self.tag,
            chat_id,
            content.len()
        );
        Ok(())
    }
}
