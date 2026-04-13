//! 工具抽象与注册。核心域不依赖 platform；HTTP 等由 main 注入 ToolContext。
//! Tool trait and registry; no platform dependency.

mod execution_governance;
mod policy;
mod registry;
mod state_file_guard;

#[cfg(all(
    feature = "tools_network_extra",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
pub mod analyze_image;
pub mod board_info;
pub mod calendar;
pub mod continuity_snapshot;
pub mod cron;
pub mod cron_manage;
#[cfg(all(
    feature = "tools_network_extra",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
pub mod document_extract;
#[cfg(all(
    feature = "tools_network_extra",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
pub mod document_read;
#[cfg(all(
    feature = "tools_network_extra",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
pub mod document_search;
pub mod env;
pub mod factual_memory;
pub mod file_edit;
pub mod file_write;
pub mod files;
pub mod get_time;
#[cfg(feature = "tools_diagnostics")]
pub mod hardware;
pub(crate) mod http_bridge;
#[cfg(all(
    feature = "tools_network_extra",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
pub mod http_request;
#[cfg(feature = "tools_diagnostics")]
pub mod i2c_device;
#[cfg(feature = "tools_diagnostics")]
pub mod i2c_sensor;
pub mod kv_store;
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub mod lua_query;
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub mod lua_memory_query;
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub mod lua_tool_bridge;
pub mod memory_get;
#[cfg(feature = "tools_diagnostics")]
pub mod memory_manage;
pub mod memory_search;
pub mod message;
#[cfg(feature = "tools_network_extra")]
pub mod model_config;
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub mod network;
#[cfg(feature = "tools_diagnostics")]
pub mod network_scan;
#[cfg(all(
    feature = "tools_network_extra",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
pub mod pdf_read;
pub mod private_garden;
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub mod process;
#[cfg(feature = "tools_network_extra")]
pub mod proxy_config;
pub mod remind_at;
pub mod sensor_watch;
#[cfg(feature = "tools_diagnostics")]
pub mod session_manage;
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub mod shell;
#[cfg(feature = "tools_diagnostics")]
pub mod system_control;
pub mod task;
pub mod voice_input;
pub mod voice_output;
#[cfg(all(
    feature = "tools_network_extra",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
pub mod web_fetch;
#[cfg(feature = "tools_network_extra")]
pub mod web_search;

#[cfg(all(
    feature = "tools_network_extra",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
pub use analyze_image::AnalyzeImageTool;
pub use board_info::BoardInfoTool;
pub use calendar::CalendarTool;
pub use continuity_snapshot::ContinuitySnapshotTool;
pub use cron_manage::CronManageTool;
#[cfg(all(
    feature = "tools_network_extra",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
pub use document_extract::DocumentExtractTool;
#[cfg(all(
    feature = "tools_network_extra",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
pub use document_read::DocumentReadTool;
#[cfg(all(
    feature = "tools_network_extra",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
pub use document_search::DocumentSearchTool;
pub use env::EnvTool;
pub use execution_governance::{
    render_tool_execution_governance_markdown, ToolEmergencyStopState, ToolExecutionGateDecision,
    ToolExecutionGovernance, ToolExecutionGovernanceState, ToolExecutionPermit,
    ToolExecutionRecord, ToolExecutionRecordStatus, ToolExecutionRequest,
};
pub use factual_memory::FactualMemoryTool;
pub use file_edit::FileEditTool;
pub use file_write::FileWriteTool;
pub use files::FilesTool;
pub use get_time::GetTimeTool;
#[cfg(feature = "tools_diagnostics")]
pub use hardware::DeviceControlTool;
#[cfg(all(
    feature = "tools_network_extra",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
pub use http_request::HttpRequestTool;
#[cfg(feature = "tools_diagnostics")]
pub use i2c_device::I2cDeviceTool;
#[cfg(feature = "tools_diagnostics")]
pub use i2c_sensor::I2cSensorTool;
pub use kv_store::KvStoreTool;
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub use lua_query::LuaQueryTool;
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub use lua_memory_query::LuaMemoryQueryTool;
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub use lua_tool_bridge::LuaToolBridgeTool;
pub use memory_get::MemoryGetTool;
#[cfg(feature = "tools_diagnostics")]
pub use memory_manage::MemoryManageTool;
pub use memory_search::MemorySearchTool;
pub use message::MessageTool;
#[cfg(feature = "tools_network_extra")]
pub use model_config::ModelConfigTool;
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub use network::NetworkTool;
#[cfg(feature = "tools_diagnostics")]
pub use network_scan::NetworkScanTool;
#[cfg(all(
    feature = "tools_network_extra",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
pub use pdf_read::PdfReadTool;
pub use policy::{
    ToolApprovalMode, ToolEffectClass, ToolExecutionShape, ToolRiskLevel, ToolRollbackKind,
};
pub use policy::{ToolExposure, ToolMetadata, ToolPolicyContext};
pub use private_garden::PrivateGardenTool;
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub use process::ProcessTool;
#[cfg(feature = "tools_network_extra")]
pub use proxy_config::ProxyConfigTool;
pub use registry::{
    build_default_registry, DefaultRegistryDeps, ToolBridgeCatalogEntry,
    ToolBridgeProposalAssessment, ToolBridgeProposalDecision, ToolCatalogEntry, ToolRegistry,
};
pub use remind_at::{RemindAtTool, RemindListTool};
pub use sensor_watch::SensorWatchTool;
#[cfg(feature = "tools_diagnostics")]
pub use session_manage::SessionManageTool;
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub use shell::ShellTool;
#[cfg(feature = "tools_diagnostics")]
pub use system_control::SystemControlTool;
pub use task::TaskTool;
pub use voice_input::VoiceInputTool;
pub use voice_output::VoiceOutputTool;
#[cfg(all(
    feature = "tools_network_extra",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
pub use web_fetch::WebFetchTool;
#[cfg(feature = "tools_network_extra")]
pub use web_search::WebSearchTool;

use crate::error::{Error, Result};
use serde::Serialize;
use serde_json::{Map, Value};

/// 将 args 解析为 JSON 对象；供各 tool execute 统一使用，stage 用于错误上下文。
pub fn parse_tool_args(args: &str, stage: &'static str) -> Result<Map<String, Value>> {
    serde_json::from_str::<Map<String, Value>>(args).map_err(|e| Error::Other {
        source: Box::new(e),
        stage,
    })
}

pub fn serialize_tool_output<T: Serialize>(stage: &'static str, value: &T) -> Result<String> {
    serde_json::to_string(value).map_err(|e| Error::config(stage, e.to_string()))
}

/// 单次 execute 的 args 最大长度（字符）。超限返回 Error::Config。
pub const MAX_TOOL_ARGS_LEN: usize = 8 * 1024;
/// 单次 execute 返回值最大长度（字符）。超限截断或返回 Error::Config。
pub const MAX_TOOL_RESULT_LEN: usize = 16 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ToolOutboundTarget {
    CurrentChat,
    Explicit { channel: String, chat_id: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolOutboundDeliveryKind {
    Supplemental,
    Primary,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolOutboundIntent {
    pub target: ToolOutboundTarget,
    pub delivery_kind: ToolOutboundDeliveryKind,
    pub content: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct ToolExecutionOutcome {
    pub content: String,
    pub outbound_intents: Vec<ToolOutboundIntent>,
}

impl ToolExecutionOutcome {
    pub fn text(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            outbound_intents: Vec::new(),
        }
    }

    pub fn with_outbound_intent(mut self, intent: ToolOutboundIntent) -> Self {
        self.outbound_intents.push(intent);
        self
    }

    pub fn with_current_chat_reply(mut self, content: impl Into<String>) -> Self {
        self.outbound_intents.push(ToolOutboundIntent {
            target: ToolOutboundTarget::CurrentChat,
            delivery_kind: ToolOutboundDeliveryKind::Primary,
            content: content.into(),
        });
        self
    }
}

impl From<String> for ToolExecutionOutcome {
    fn from(value: String) -> Self {
        Self::text(value)
    }
}

impl From<&str> for ToolExecutionOutcome {
    fn from(value: &str) -> Self {
        Self::text(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct ToolCapabilityContract {
    pub required: &'static [&'static str],
    pub allow_when_degraded: bool,
}

impl ToolCapabilityContract {
    pub const fn required(required: &'static [&'static str]) -> Self {
        Self {
            required,
            allow_when_degraded: false,
        }
    }

    pub const fn required_allowing_degraded(required: &'static [&'static str]) -> Self {
        Self {
            required,
            allow_when_degraded: true,
        }
    }

    pub const fn is_empty(self) -> bool {
        self.required.is_empty()
    }
}

/// 工具执行时注入的上下文；HTTP 等由 lib 实现（如 EspHttpClient）。
/// 当前会话的 chat_id/channel 供 remind_at 等工具使用；默认 None，agent 循环内用 wrapper 注入。
pub trait ToolContext {
    fn get(&mut self, url: &str) -> Result<(u16, crate::platform::ResponseBody)> {
        self.get_with_headers(url, &[])
    }
    fn get_with_headers(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
    ) -> Result<(u16, crate::platform::ResponseBody)>;
    /// POST 请求，自定义 headers（须含 Content-Type 等）；供 web_search Tavily 等使用。
    fn post_with_headers(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
        body: &[u8],
    ) -> Result<(u16, crate::platform::ResponseBody)>;
    /// 流式 POST：逐块回调响应体，默认回退到整包 post_with_headers。
    /// `max_response_bytes`: None = 无限制；Some(n) = 限制总字节数。
    fn post_streaming(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
        body: &[u8],
        _max_response_bytes: Option<usize>,
        on_chunk: &mut dyn FnMut(&[u8]) -> Result<()>,
    ) -> Result<u16> {
        let (status, resp) = self.post_with_headers(url, headers, body)?;
        on_chunk(resp.as_slice())?;
        Ok(status)
    }
    /// HTTP PATCH 请求；默认回退到 post_with_headers。
    fn patch_with_headers(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
        body: &[u8],
    ) -> Result<(u16, crate::platform::ResponseBody)> {
        self.post_with_headers(url, headers, body)
    }
    /// HTTP PUT 请求；默认回退到 post_with_headers。
    fn put_with_headers(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
        body: &[u8],
    ) -> Result<(u16, crate::platform::ResponseBody)> {
        self.post_with_headers(url, headers, body)
    }
    /// HTTP DELETE 请求；默认回退到 get_with_headers。
    fn delete_with_headers(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
    ) -> Result<(u16, crate::platform::ResponseBody)> {
        self.get_with_headers(url, headers)
    }
    /// 当前入站消息的 chat_id；remind_at 等工具写存储时使用。默认 None。
    fn current_chat_id(&self) -> Option<&str> {
        None
    }
    /// 当前入站消息的 channel。默认 None。
    fn current_channel(&self) -> Option<&str> {
        None
    }
    /// 当前入站消息的 ingress；默认 None。
    fn current_ingress(&self) -> Option<crate::bus::IngressKind> {
        None
    }
    /// 查询某个通道在当前运行时的能力合同；默认不可用。
    fn channel_capability(
        &self,
        _channel: &str,
    ) -> Option<crate::channel_capability::ChannelCapabilityEntry> {
        None
    }
    /// 当前运行时是否允许工具声明“当前聊天主答复已由工具交付”。
    /// 目前仅在不会与编辑型交付通道冲突的运行时开启。
    fn supports_current_chat_outbound_message(&self) -> bool {
        false
    }
    /// 当前运行时是否允许工具声明“当前聊天主答复已由工具交付”。
    /// 目前仅在不会与编辑型交付通道冲突的运行时开启。
    fn supports_current_chat_primary_reply(&self) -> bool {
        false
    }
    /// 当前运行时是否允许工具把消息发往非当前聊天的显式目标。
    fn supports_explicit_outbound_message(&self) -> bool {
        false
    }
    /// 为本轮工具侧外发消息预留一次发送额度；运行时可在这里做限流、去重和权限裁决。
    fn claim_outbound_message_delivery(
        &mut self,
        _target_is_current: bool,
        _primary: bool,
    ) -> Result<()> {
        Ok(())
    }
    /// 返回当前 policy 下允许脚本观察的工具目录；默认不可用。
    fn tool_bridge_catalog(&self) -> Result<Vec<ToolBridgeCatalogEntry>> {
        Err(Error::config(
            "tool_bridge_catalog",
            "tool bridge catalog unavailable in this runtime context",
        ))
    }
    /// 对脚本提出的 tool request proposal 做治理评估；默认不可用。
    fn assess_tool_request_proposal(
        &self,
        _tool_name: &str,
        _args: &Value,
    ) -> Result<ToolBridgeProposalAssessment> {
        Err(Error::config(
            "tool_bridge_assess",
            "tool bridge assessment unavailable in this runtime context",
        ))
    }
    /// 当前用户界面语言（来自设备 NVS），供工具返回人话时使用。
    fn user_locale(&self) -> crate::i18n::Locale;
}

/// 工具 trait；Agent 按 name 派发，execute 时传入 ctx 以发 HTTP 等。
pub trait Tool: Send + Sync {
    fn name(&self) -> &'static str;
    fn description(&self) -> &str;
    fn schema(&self) -> &str;
    fn execute(&self, args: &str, ctx: &mut dyn ToolContext) -> Result<String>;
    fn execute_outcome(
        &self,
        args: &str,
        ctx: &mut dyn ToolContext,
    ) -> Result<ToolExecutionOutcome> {
        self.execute(args, ctx).map(ToolExecutionOutcome::text)
    }
    /// 工具元数据：由统一的 tool policy 在运行时决定是否暴露给 LLM。
    /// Tool metadata only declares capability/risk shape; runtime exposure is resolved centrally.
    fn metadata(&self) -> ToolMetadata {
        ToolMetadata::default()
    }
    /// 单次执行的治理形状。默认沿用静态元数据；危险工具可按 args 动态提升风险、要求显式确认等。
    fn execution_shape(&self, _args: &str) -> Result<ToolExecutionShape> {
        Ok(self.metadata().default_execution_shape(self.name()))
    }
    /// 该工具是否需要网络（HTTP/TLS）；orchestrator 在高压力时拒绝网络工具。
    /// Whether this tool requires network (HTTP/TLS); orchestrator denies network tools under high pressure.
    fn requires_network(&self) -> bool {
        false
    }
    /// 单次执行是否真的会走网络；默认回退到静态 `requires_network`。
    /// Dynamic per-call network admission hook. Defaults to static `requires_network`.
    fn requires_network_for(&self, _args: &str) -> Result<bool> {
        Ok(self.requires_network())
    }
    /// 运行态能力合同：由 ToolRegistry 在 LLM 暴露与执行前统一裁决，不由工具体自行零散判断。
    fn capability_contract(&self) -> ToolCapabilityContract {
        ToolCapabilityContract::default()
    }
}
