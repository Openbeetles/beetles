//! 甲壳虫 (beetle) - 稳定对外 API。
//! beetle - stable public API.

mod build_info;
pub mod constants;
pub mod metrics;
pub mod util;

pub use build_info::ota_manifest_url;
/// Re-export PlatformHttpClient at crate root so core modules (agent, tools) can depend on
/// `crate::PlatformHttpClient` without importing `crate::platform` directly.
pub use platform::PlatformHttpClient;
pub use platform::runtime_board::resolved_board_id;
pub mod agent;
pub mod audio;
pub mod bg_timer;
pub mod bus;
pub mod calendar;
pub mod channels;
pub mod config;
pub mod display;
pub mod doctor;
pub mod error;
pub mod llm;
pub mod memory;
pub mod platform;
pub mod state;
pub mod task;
pub mod tools;

#[cfg(feature = "cli")]
pub mod cli;

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub mod commands;

pub mod bootstrap;
pub mod cron;
pub mod heartbeat;
pub mod i18n;
pub mod orchestrator;
pub mod runtime;
pub mod skills;

pub use agent::{
    AgentLoopConfig, ContextParams, DEFAULT_MESSAGES_MAX_LEN, DEFAULT_SYSTEM_MAX_LEN,
    SESSION_RECENT_N, StreamEditor, TypingNotifier, build_context, run_system_agent_loop,
    run_user_agent_loop,
};
pub use bus::{DEFAULT_CAPACITY, MAX_CONTENT_LEN, MessageBus, PcMsg};
pub use channels::connect_wss;
#[cfg(feature = "feishu")]
pub use channels::run_feishu_ws_loop;
pub use channels::run_qq_ws_loop;
pub use channels::{
    ChannelHttpClient, ChannelSinks, LogSink, MessageSink, QueuedSink, WebSocketSink,
    WssConnectProfile, feishu_acquire_token, feishu_edit_message, feishu_send_and_get_id,
    flush_dingtalk_sends, flush_feishu_sends, flush_qq_channel_sends, flush_telegram_sends,
    flush_wecom_sends, get_bot_username, poll_telegram_once, run_dingtalk_sender_loop,
    run_dispatch, run_feishu_sender_loop, run_qq_sender_loop, run_telegram_poll_loop,
    run_telegram_sender_loop, run_wecom_sender_loop, send_chat_action, tg_edit_message_text,
    tg_send_and_get_id,
};
pub use channels::{connect_wss_with_headers, connect_wss_with_headers_and_profile};
pub use config::{
    AppConfig, DeviceEntry, HardwareSegment, I2cBusConfig, I2cDeviceEntry, I2cSensorEntry,
    LlmSource, PinConfig, parse_allowed_chat_ids, save_hardware_segment,
};
pub use display::{
    DisplayBus, DisplayChannelStatus, DisplayColorOrder, DisplayCommand, DisplayConfig,
    DisplayDriver, DisplayPressureLevel, DisplaySystemState, default_disabled_display_config,
    validate_display_config_core,
};
pub use error::{Error, Result};
pub use llm::{
    AnthropicClient, FallbackLlmClient, LlmClient, LlmHttpClient, LlmResponse, Message,
    OpenAiCompatibleClient, build_llm_clients,
};
pub use platform::{ConfigStore, MemorySnapshot, Platform, SkillStorage, StateFs};
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
pub use platform::{
    Esp32Platform, EspHttpClient, SpiffsLongTermMemoryStore, SpiffsMemoryStore, SpiffsSessionStore,
    connect_wifi, init_nvs, init_spiffs, spiffs_base_string, spiffs_usage, state_mount_path,
};
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub use platform::{
    EspHttpClient, LinuxPlatform, SpiffsLongTermMemoryStore, SpiffsMemoryStore, SpiffsSessionStore,
    connect_wifi, init_nvs, init_spiffs, spiffs_base_string, spiffs_usage, state_mount_path,
};
pub use tools::{
    CalendarTool, DefaultRegistryDeps, FileEditTool, FileWriteTool, FilesTool, GetTimeTool,
    KvStoreTool, PrivateGardenTool, RemindAtTool, TaskTool, Tool, ToolContext, ToolExposure,
    ToolMetadata, ToolPolicyContext, ToolRegistry, VoiceInputTool, VoiceOutputTool,
    build_default_registry,
};
#[cfg(feature = "tools_diagnostics")]
pub use tools::{
    CronManageTool, DeviceControlTool, I2cDeviceTool, I2cSensorTool, MemoryManageTool,
    NetworkScanTool, SensorWatchTool, SessionManageTool, SystemControlTool,
};
#[cfg(all(
    feature = "tools_network_extra",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
pub use tools::{
    DocumentExtractTool, DocumentReadTool, DocumentSearchTool, HttpRequestTool, PdfReadTool,
    WebFetchTool,
};
#[cfg(feature = "tools_network_extra")]
pub use tools::{ModelConfigTool, ProxyConfigTool, WebSearchTool};

/// 任何 PlatformHttpClient 均可作为 LlmHttpClient 使用。
/// ToolContext 的实现由 `tools::http_bridge::HttpClientToolContext` 承载（含会话元数据），
/// 不再提供硬编码 locale 的 blanket impl。
impl<T: platform::PlatformHttpClient + ?Sized> llm::LlmHttpClient for T {
    fn do_post(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
        body: &[u8],
    ) -> Result<(u16, platform::ResponseBody)> {
        platform::PlatformHttpClient::post(self, url, headers, body)
    }
    fn do_post_streaming(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
        body: &[u8],
        max_response_bytes: Option<usize>,
        on_chunk: &mut dyn FnMut(&[u8]) -> Result<()>,
    ) -> Result<u16> {
        platform::PlatformHttpClient::post_streaming(
            self,
            url,
            headers,
            body,
            max_response_bytes,
            on_chunk,
        )
    }
    fn reset_connection_for_retry(&mut self) {
        platform::PlatformHttpClient::reset_connection_for_retry(self);
    }
}

/// 任何 PlatformHttpClient（含 `dyn PlatformHttpClient`）均可作为 ChannelHttpClient 使用。
/// `?Sized` 覆盖 `dyn PlatformHttpClient` / `dyn PlatformHttpClient + Send`，
/// 替代原先三份重复的手写 dyn 实现。
impl<T: platform::PlatformHttpClient + ?Sized> channels::ChannelHttpClient for T {
    fn http_get(&mut self, url: &str) -> Result<(u16, platform::ResponseBody)> {
        platform::PlatformHttpClient::get(self, url, &[])
    }
    fn http_get_with_headers(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
    ) -> Result<(u16, platform::ResponseBody)> {
        platform::PlatformHttpClient::get(self, url, headers)
    }
    fn http_post(&mut self, url: &str, body: &[u8]) -> Result<(u16, platform::ResponseBody)> {
        platform::PlatformHttpClient::post(self, url, &[], body)
    }
    fn http_post_with_headers(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
        body: &[u8],
    ) -> Result<(u16, platform::ResponseBody)> {
        platform::PlatformHttpClient::post(self, url, headers, body)
    }
    fn http_patch_with_headers(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
        body: &[u8],
    ) -> Result<(u16, platform::ResponseBody)> {
        platform::PlatformHttpClient::patch(self, url, headers, body)
    }
    fn reset_connection_for_retry(&mut self) {
        platform::PlatformHttpClient::reset_connection_for_retry(self);
    }
}

impl<T: platform::PlatformHttpClient + ?Sized> calendar::CalendarHttpClient for T {
    fn get_with_headers(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
    ) -> Result<(u16, platform::ResponseBody)> {
        platform::PlatformHttpClient::get(self, url, headers)
    }

    fn post_with_headers(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
        body: &[u8],
    ) -> Result<(u16, platform::ResponseBody)> {
        platform::PlatformHttpClient::post(self, url, headers, body)
    }

    fn patch_with_headers(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
        body: &[u8],
    ) -> Result<(u16, platform::ResponseBody)> {
        platform::PlatformHttpClient::patch(self, url, headers, body)
    }

    fn put_with_headers(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
        body: &[u8],
    ) -> Result<(u16, platform::ResponseBody)> {
        platform::PlatformHttpClient::put(self, url, headers, body)
    }

    fn delete_with_headers(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
    ) -> Result<(u16, platform::ResponseBody)> {
        platform::PlatformHttpClient::delete(self, url, headers)
    }
}
