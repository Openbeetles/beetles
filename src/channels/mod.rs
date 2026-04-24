//! 通道抽象与出站分发。仅依赖 bus、error、config；不依赖 agent、llm、tools。
//! Channel sink trait and types; dispatch consumes outbound and sends to sinks.

#[cfg(any(
    feature = "telegram",
    feature = "feishu",
    feature = "dingtalk",
    feature = "qq_channel",
    test
))]
mod chunk;
mod connectivity;
#[cfg(feature = "dingtalk")]
pub(crate) mod dingtalk;
mod dispatch;
#[cfg(feature = "feishu")]
pub(crate) mod feishu;
mod http_client;
mod outbound_text;
#[cfg(feature = "qq_channel")]
mod qq;
#[cfg(any(
    feature = "telegram",
    feature = "feishu",
    feature = "dingtalk",
    feature = "wecom",
    feature = "qq_channel",
    test
))]
mod send;
#[cfg(feature = "telegram")]
pub(crate) mod telegram;
pub(crate) mod voice_sink;
#[cfg(feature = "websocket")]
mod websocket;
#[cfg(feature = "wecom")]
pub(crate) mod wecom;
mod wss_gateway;

pub use connectivity::build_unavailable_snapshot;
pub use connectivity::{build_snapshot, ChannelConnectivityItem, ChannelConnectivitySnapshot};
#[cfg(all(
    feature = "dingtalk",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
pub use dingtalk::run_dingtalk_stream_loop;
#[cfg(all(
    feature = "dingtalk",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
pub use dingtalk::DingtalkSessionStore;
#[cfg(feature = "dingtalk")]
pub use dingtalk::{flush_dingtalk_sends, run_dingtalk_sender_loop};
#[cfg(any(
    feature = "telegram",
    feature = "feishu",
    feature = "dingtalk",
    feature = "wecom",
    feature = "qq_channel"
))]
pub use dispatch::QueuedSink;
pub use dispatch::{build_channel_sinks, spawn_sender_threads, ChannelRxSet};
pub use dispatch::{run_dispatch, ChannelSinks, MessageSink};
#[cfg(feature = "feishu")]
pub use feishu::run_feishu_ws_loop;
#[cfg(feature = "feishu")]
pub use feishu::{
    acquire_tenant_token as feishu_acquire_token, event_body_to_pcmsg, feishu_edit_message,
    feishu_send_and_get_id, flush_feishu_sends, run_feishu_sender_loop, FeishuTokenCache,
};
pub use http_client::ChannelHttpClient;
#[cfg(feature = "qq_channel")]
pub use qq::{
    flush_qq_channel_sends, is_ws_online, new_shared_qq_token_cache, new_shared_qq_ws_status,
    run_qq_sender_loop, QqInboundDedupStore, QqMsgIdCache, SharedQqTokenCache, SharedQqWsStatus,
};
#[cfg(feature = "qq_channel")]
pub use qq::{run_qq_ws_loop, QqWsLoopConfig};

#[cfg(feature = "telegram")]
pub use telegram::{
    edit_message_text as tg_edit_message_text, flush_telegram_sends, get_bot_username,
    poll_telegram_once, run_telegram_poll_loop, run_telegram_sender_loop, send_chat_action,
    tg_send_and_get_id, TelegramCommandCtx,
};
pub use voice_sink::VoiceSink;
#[cfg(feature = "websocket")]
pub use websocket::{WebSocketSink, MAX_WS_CONNECTIONS, MAX_WS_MESSAGE_LEN};
#[cfg(feature = "wecom")]
pub use wecom::{
    new_wecom_aibot_route_store, run_wecom_aibot_loop, WecomAibotRouteStore, WECOM_AIBOT_WS_URL,
};
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
pub use wss_gateway::{connect_esp_wss, EspWssConnection};
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub use wss_gateway::{connect_linux_wss, LinuxWssConnection};
pub(crate) use wss_gateway::{connect_wss, connect_wss_with_headers_and_profile};
pub use wss_gateway::{WssCloseInfo, WssConnectProfile, WssConnection, WssEvent};

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
