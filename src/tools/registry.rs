//! ToolRegistry：按 name 注册与查找，生成 API 用 tool specs。
//! ToolRegistry: register, get by name, tool_specs for API.

use crate::config::AppConfig;
use crate::error::{Error, Result};
use crate::llm::ToolSpec as LlmToolSpec;
use crate::tools::{Tool, MAX_TOOL_ARGS_LEN, MAX_TOOL_RESULT_LEN};
use crate::util::truncate_to_byte_len;
use indexmap::IndexMap;
use std::fmt::Write as _;
use std::sync::Arc;

/// 按 name 注册与派发工具；可生成带总长度上界的 tool specs。IndexMap 保证工具顺序稳定。
pub struct ToolRegistry {
    tools: IndexMap<&'static str, Box<dyn Tool>>,
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: IndexMap::new(),
        }
    }

    pub fn register(&mut self, tool: Box<dyn Tool>) {
        let name = tool.name();
        self.tools.insert(name, tool);
    }

    pub fn get(&self, name: &str) -> Option<&dyn Tool> {
        self.tools.get(name).map(|b| b.as_ref())
    }

    /// 该工具是否需要网络（从 Tool trait 推导）。未注册工具返回 false。
    /// Whether the named tool requires network (derived from Tool trait). Returns false for unknown tools.
    pub fn is_network_tool(&self, name: &str) -> bool {
        self.tools.get(name).is_some_and(|t| t.requires_network())
    }

    /// 生成供 LLM API 使用的 tool specs，总描述长度不超过 max_total_len（字符数）。
    /// 超限时从尾部丢弃工具。
    pub fn tool_specs_for_api(&self, max_total_len: usize) -> Vec<LlmToolSpec> {
        let mut out = Vec::with_capacity(self.tools.len());
        let mut len = 0usize;
        for tool in self.tools.values() {
            let name = tool.name();
            let description = tool.description();
            let parameters = tool.schema();
            let add_len = name.len() + description.len() + parameters.to_string().len() + 2;
            if len + add_len > max_total_len && !out.is_empty() {
                break;
            }
            len += add_len;
            out.push(LlmToolSpec {
                name: name.to_string(),
                description: description.to_string(),
                parameters,
            });
        }
        out
    }

    /// Whether registry contains any tool.
    pub fn has_tools(&self) -> bool {
        !self.tools.is_empty()
    }

    /// 仅供调试/外部消费：格式化工具说明文本，总长度不超过 max_chars。
    /// NOTE: agent context 不再注入该文本；工具规格由 LLM API `tools` 参数承载。
    pub fn format_descriptions_for_system_prompt(&self, max_chars: usize) -> String {
        let mut s = String::with_capacity(max_chars.min(4096));
        for tool in self.tools.values() {
            let before = s.len();
            let _ = writeln!(&mut s, "- {}: {}", tool.name(), tool.description());
            if s.len() > max_chars {
                s.truncate(before);
                break;
            }
        }
        s
    }

    /// 按 name 执行工具；args 超限返回 Error::Config；返回值在 Registry 内截断至 MAX_TOOL_RESULT_LEN。
    pub fn execute(
        &self,
        name: &str,
        args: &str,
        ctx: &mut dyn crate::tools::ToolContext,
    ) -> Result<String> {
        if args.len() > MAX_TOOL_ARGS_LEN {
            return Err(Error::config(
                "tool_execute",
                format!("args length exceeds {}", MAX_TOOL_ARGS_LEN),
            ));
        }
        let tool = self.get(name).ok_or_else(|| Error::Other {
            source: Box::new(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("tool not found: {}", name),
            )),
            stage: "tool_execute",
        })?;
        let result = tool.execute(args, ctx)?;
        Ok(truncate_to_byte_len(&result, MAX_TOOL_RESULT_LEN))
    }
}

/// 构建包含所有内置工具的注册表。`platform` 用于 `board_info` 等依赖平台能力的工具。
/// Returns `(registry, Option<baidu_token_cache>)` — the cache is shared with voice_session.
pub fn build_default_registry(
    config: &AppConfig,
    platform: Arc<dyn crate::Platform>,
    remind_at_store: Arc<dyn crate::memory::RemindAtStore + Send + Sync>,
    session_summary_store: Arc<dyn crate::memory::SessionSummaryStore + Send + Sync>,
    session_store: Arc<dyn crate::memory::SessionStore + Send + Sync>,
    _memory_store: Arc<dyn crate::memory::MemoryStore + Send + Sync>,
    _config_store: Arc<dyn crate::platform::ConfigStore + Send + Sync>,
) -> (
    ToolRegistry,
    Option<Arc<crate::audio::baidu_token::BaiduTokenCache>>,
) {
    let mut registry = ToolRegistry::new();
    registry.register(Box::new(super::GetTimeTool));
    registry.register(Box::new(super::EnvTool));
    registry.register(Box::new(super::FilesTool::new(platform.state_fs())));
    #[cfg(feature = "tools_network_extra")]
    registry.register(Box::new(super::WebSearchTool::new(config)));
    #[cfg(feature = "tools_network_extra")]
    registry.register(Box::new(super::AnalyzeImageTool::new(config)));
    let remind_at_store_for_list = Arc::clone(&remind_at_store);
    registry.register(Box::new(super::RemindAtTool::new(remind_at_store)));
    registry.register(Box::new(super::RemindListTool::new(
        remind_at_store_for_list,
    )));
    registry.register(Box::new(super::UpdateSessionSummaryTool::new(
        session_summary_store,
        Arc::clone(&session_store),
    )));
    registry.register(Box::new(super::BoardInfoTool::new(Arc::clone(&platform))));
    registry.register(Box::new(super::KvStoreTool::new(platform.state_fs())));
    #[cfg(feature = "tools_diagnostics")]
    if !config.hardware_devices.is_empty() {
        registry.register(Box::new(super::DeviceControlTool::new(
            config.hardware_devices.clone(),
            Arc::clone(&platform),
        )));
    }
    // --- New tools ---
    #[cfg(feature = "tools_diagnostics")]
    registry.register(Box::new(super::MemoryManageTool::new(Arc::clone(
        &_memory_store,
    ))));
    #[cfg(feature = "tools_network_extra")]
    registry.register(Box::new(super::HttpRequestTool));
    #[cfg(feature = "tools_diagnostics")]
    registry.register(Box::new(super::SessionManageTool::new(session_store)));
    registry.register(Box::new(super::FileWriteTool::new(platform.state_fs())));
    #[cfg(feature = "tools_diagnostics")]
    registry.register(Box::new(super::SystemControlTool::new(Arc::clone(
        &platform,
    ))));
    #[cfg(feature = "tools_diagnostics")]
    registry.register(Box::new(super::CronManageTool::new(Arc::clone(
        &_memory_store,
    ))));
    #[cfg(feature = "tools_network_extra")]
    registry.register(Box::new(super::ProxyConfigTool::new(_config_store)));
    #[cfg(feature = "tools_network_extra")]
    registry.register(Box::new(super::ModelConfigTool::new(Arc::clone(&platform))));
    #[cfg(feature = "tools_diagnostics")]
    registry.register(Box::new(super::NetworkScanTool::new(Arc::clone(&platform))));
    #[cfg(feature = "tools_diagnostics")]
    if !config.hardware_devices.is_empty() || !config.i2c_sensors.is_empty() {
        registry.register(Box::new(super::SensorWatchTool::new(
            Arc::clone(&_memory_store),
            config.hardware_devices.clone(),
            config.i2c_sensors.clone(),
        )));
    }
    #[cfg(feature = "tools_diagnostics")]
    if config.i2c_bus.is_some() && !config.i2c_devices.is_empty() {
        registry.register(Box::new(super::I2cDeviceTool::new(
            Arc::clone(&platform),
            config.i2c_devices.clone(),
        )));
    }
    #[cfg(feature = "tools_diagnostics")]
    if config.i2c_bus.is_some() && !config.i2c_sensors.is_empty() {
        registry.register(Box::new(super::I2cSensorTool::new(
            Arc::clone(&platform),
            config.i2c_sensors.clone(),
        )));
    }
    let mut shared_baidu_token: Option<Arc<crate::audio::baidu_token::BaiduTokenCache>> = None;
    if let Some(audio_cfg) = config.audio.clone() {
        let baidu_stt_credential_ok =
            !audio_cfg.stt.api_key.trim().is_empty() && !audio_cfg.stt.api_secret.trim().is_empty();
        let stt_ok = audio_cfg.stt.provider == "baidu"
            && baidu_stt_credential_ok
            && audio_cfg.microphone.enabled;
        let tts_ok = audio_cfg.tts.provider == "baidu"
            && audio_cfg.stt.provider == "baidu"
            && baidu_stt_credential_ok
            && audio_cfg.speaker.enabled;
        let baidu_token_cache = if audio_cfg.enabled && (stt_ok || tts_ok) {
            Some(Arc::new(crate::audio::baidu_token::BaiduTokenCache::new()))
        } else {
            None
        };
        if audio_cfg.enabled && stt_ok {
            if let Some(ref cache) = baidu_token_cache {
                registry.register(Box::new(super::VoiceInputTool::new(
                    Arc::clone(&platform),
                    audio_cfg.clone(),
                    Arc::clone(cache),
                )));
            }
        }
        if audio_cfg.enabled && tts_ok {
            if let Some(ref cache) = baidu_token_cache {
                registry.register(Box::new(super::VoiceOutputTool::new(
                    Arc::clone(&platform),
                    audio_cfg,
                    Arc::clone(cache),
                )));
            }
        }
        shared_baidu_token = baidu_token_cache;
    }
    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
    registry.register(Box::new(super::ShellTool));
    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
    registry.register(Box::new(super::ProcessTool));
    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
    registry.register(Box::new(super::NetworkTool));
    (registry, shared_baidu_token)
}
