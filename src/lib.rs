//! 甲壳虫 (beetle) - 稳定对外 API。
//! beetle - stable public API.

mod build_info;
mod build_package;
pub mod capability_package;
pub mod channel_capability;
pub mod constants;
pub mod device_capability;
pub mod metrics;
pub mod network;
pub mod util;

pub use build_info::ota_manifest_url;
pub use build_package::{
    compiled_build_package_capabilities, compiled_sensor_capability, compiled_vision_capability,
    compiled_voice_capability, current_build_package, BuildPackageCapabilities,
    BuildPackageSnapshot, BuildPackageTargetFamily, BUILD_PACKAGE_PROFILE_CORE_ONLY,
    BUILD_PACKAGE_PROFILE_ESP_FULL, BUILD_PACKAGE_PROFILE_HOST_FULL,
    BUILD_PACKAGE_PROFILE_LINUX_FULL, BUILD_PACKAGE_PROFILE_SENSOR, BUILD_PACKAGE_PROFILE_VISION,
    BUILD_PACKAGE_PROFILE_VISION_SENSOR, BUILD_PACKAGE_PROFILE_VOICE,
    BUILD_PACKAGE_PROFILE_VOICE_SENSOR, BUILD_PACKAGE_PROFILE_VOICE_VISION,
};
pub use capability_package::{
    build_capability_package_operator_snapshot, build_capability_package_runtime_capabilities,
    build_capability_package_runtime_prompt_bundle, build_capability_package_tool_policy_set,
    install_capability_package, rollback_capability_package, set_capability_package_enabled,
    uninstall_capability_package, CapabilityPackageInstallPayload, CapabilityPackageOperationKind,
    CapabilityPackageOperationOutcome, CapabilityPackageOperatorSnapshot,
    CapabilityPackageRuntimeCapabilities, CapabilityPackageRuntimePromptBundle,
    CapabilityPackageToolPolicySet, MAX_CAPABILITY_PACKAGE_HTTP_BODY_LEN,
};
pub use channel_capability::{
    build_channel_capability_registry, build_channel_capability_snapshots,
    ChannelCapabilityContract, ChannelCapabilityEntry, ChannelCapabilityRegistry,
    ChannelCapabilitySnapshot, ChannelDeliveryOrderingModel, CHANNEL_DINGTALK, CHANNEL_FEISHU,
    CHANNEL_QQ_CHANNEL, CHANNEL_TELEGRAM, CHANNEL_VOICE, CHANNEL_WEBSOCKET, CHANNEL_WECOM,
};
pub use device_capability::{
    build_device_capability_registry, build_device_capability_registry_from_input,
    build_device_capability_snapshots, build_device_capability_snapshots_for_registry,
    DeviceCapabilityBuildInput, DeviceCapabilityObservationMode, DeviceCapabilityPlaneContract,
    DeviceCapabilityPlaneEntry, DeviceCapabilityPlaneMountModel, DeviceCapabilityPlaneSnapshot,
    DeviceCapabilityRegistry, DEVICE_CAPABILITY_SENSOR, DEVICE_CAPABILITY_VISION,
    DEVICE_CAPABILITY_VOICE,
};
pub use platform::runtime_board::resolved_board_id;
/// Re-export PlatformHttpClient at crate root so core modules (agent, tools) can depend on
/// `crate::PlatformHttpClient` without importing `crate::platform` directly.
pub use platform::PlatformHttpClient;
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
pub mod task_execution;
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
    build_context, run_agent_loop, AgentLoopConfig, ContextParams, StreamEditor, TypingNotifier,
    DEFAULT_MESSAGES_MAX_LEN, DEFAULT_SYSTEM_MAX_LEN, SESSION_RECENT_N,
};
pub use bus::{MessageBus, PcMsg, DEFAULT_CAPACITY, MAX_CONTENT_LEN};
#[cfg(feature = "feishu")]
pub use channels::run_feishu_ws_loop;
pub use channels::run_qq_ws_loop;
pub use channels::{
    feishu_acquire_token, feishu_edit_message, feishu_send_and_get_id, flush_dingtalk_sends,
    flush_feishu_sends, flush_qq_channel_sends, flush_telegram_sends, flush_wecom_sends,
    get_bot_username, poll_telegram_once, run_dingtalk_sender_loop, run_dispatch,
    run_feishu_sender_loop, run_qq_sender_loop, run_telegram_poll_loop, run_telegram_sender_loop,
    run_wecom_sender_loop, send_chat_action, tg_edit_message_text, tg_send_and_get_id,
    ChannelHttpClient, ChannelSinks, FeishuTokenCache, LogSink, MessageSink, QueuedSink,
    WebSocketSink, WssConnectProfile,
};
pub use config::{
    parse_allowed_chat_ids, save_hardware_segment, AppConfig, DeviceEntry, HardwareSegment,
    I2cBusConfig, I2cDeviceEntry, I2cSensorEntry, LlmSource, PinConfig,
};
pub use display::{
    default_disabled_display_config, validate_display_config_core, DisplayBus,
    DisplayChannelStatus, DisplayColorOrder, DisplayCommand, DisplayConfig, DisplayDriver,
    DisplayPressureLevel, DisplaySystemState,
};
pub use error::{Error, Result};
pub use llm::{
    build_llm_clients, AnthropicClient, FallbackLlmClient, LlmClient, LlmHttpClient, LlmResponse,
    Message, OpenAiCompatibleClient,
};
pub use network::{HttpClientClass, NetworkGovernor, VoiceExclusiveTransportGuard};
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
pub use platform::{
    connect_wifi, init_nvs, init_spiffs, spiffs_usage, state_mount_path, Esp32Platform,
    EspHttpClient, SpiffsLongTermMemoryStore, SpiffsMemoryStore, SpiffsSessionStore,
};
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub use platform::{
    connect_wifi, init_nvs, init_spiffs, spiffs_usage, state_mount_path, EspHttpClient,
    LinuxPlatform, SpiffsLongTermMemoryStore, SpiffsMemoryStore, SpiffsSessionStore,
};
pub use platform::{
    AudioDuplexCapabilities, AudioDuplexProfile, AudioEchoCancellationCapability,
    AudioReferenceCapability, ConfigStore, MemorySnapshot, Platform, SkillStorage, StateFs,
    StorageMediaInfo, StorageMediaKind,
};
pub use tools::{
    build_default_registry, CalendarTool, DefaultRegistryDeps, FileEditTool, FileWriteTool,
    FilesTool, GetTimeTool, KvStoreTool, PrivateGardenTool, RemindAtTool, TaskTool, Tool,
    ToolCapabilityContract, ToolContext, ToolExposure, ToolMetadata, ToolPolicyContext,
    ToolRegistry, VoiceInputTool, VoiceOutputTool,
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
