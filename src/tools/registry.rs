//! ToolRegistry：按 name 注册与查找，生成 API 用 tool specs。
//! ToolRegistry: register, get by name, tool_specs for API.

use crate::config::AppConfig;
use crate::error::{Error, Result};
use crate::llm::ToolSpec as LlmToolSpec;
use crate::tools::{Tool, ToolPolicyContext, MAX_TOOL_ARGS_LEN, MAX_TOOL_RESULT_LEN};
use crate::util::truncate_to_byte_len;
use indexmap::IndexMap;
use std::sync::Arc;

pub const DEFAULT_LLM_TOOL_SPECS_MAX_TOTAL_LEN: usize = 32 * 1024;

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

    /// 该工具在本次 LLM 请求上下文中是否可见。
    pub fn is_llm_tool_visible(&self, name: &str, policy: &ToolPolicyContext<'_>) -> bool {
        self.tools
            .get(name)
            .is_some_and(|tool| tool.metadata().is_exposed_to_llm(policy))
    }

    /// 生成供 LLM 默认调用的 tool specs。
    pub fn tool_specs_for_llm(&self, policy: &ToolPolicyContext<'_>) -> Vec<LlmToolSpec> {
        self.tool_specs_for_llm_with_max(policy, DEFAULT_LLM_TOOL_SPECS_MAX_TOTAL_LEN)
    }

    /// 生成供 LLM 调用的 tool specs，总描述长度不超过 max_total_len（字符数）。
    /// 只包含当前 policy 上下文下可见的工具；超限时从尾部丢弃工具。
    pub fn tool_specs_for_llm_with_max(
        &self,
        policy: &ToolPolicyContext<'_>,
        max_total_len: usize,
    ) -> Vec<LlmToolSpec> {
        let mut out = Vec::with_capacity(self.tools.len());
        let mut len = 0usize;
        for tool in self.tools.values() {
            if !tool.metadata().is_exposed_to_llm(policy) {
                continue;
            }
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

    /// 当前上下文下是否存在至少一个可暴露给 LLM 的工具。
    pub fn has_llm_visible_tools(&self, policy: &ToolPolicyContext<'_>) -> bool {
        self.tools
            .values()
            .any(|tool| tool.metadata().is_exposed_to_llm(policy))
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
    session_store: Arc<dyn crate::memory::SessionStore + Send + Sync>,
    _memory_store: Arc<dyn crate::memory::MemoryStore + Send + Sync>,
    _long_term_memory_store: Arc<dyn crate::memory::LongTermMemoryStore + Send + Sync>,
    _config_store: Arc<dyn crate::platform::ConfigStore + Send + Sync>,
) -> (
    ToolRegistry,
    Option<Arc<crate::audio::baidu_token::BaiduTokenCache>>,
) {
    let mut registry = ToolRegistry::new();
    registry.register(Box::new(super::GetTimeTool));
    registry.register(Box::new(super::EnvTool));
    registry.register(Box::new(super::CalendarTool::new(
        platform.calendar_store(),
        platform.calendar_provider_credential_store(),
    )));
    registry.register(Box::new(super::FilesTool::new(platform.state_fs())));
    registry.register(Box::new(super::FileEditTool::new(platform.state_fs())));
    #[cfg(all(
        feature = "tools_network_extra",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    ))]
    registry.register(Box::new(super::DocumentSearchTool::new(
        platform.state_fs(),
    )));
    #[cfg(all(
        feature = "tools_network_extra",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    ))]
    registry.register(Box::new(super::DocumentReadTool::new(platform.state_fs())));
    #[cfg(all(
        feature = "tools_network_extra",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    ))]
    registry.register(Box::new(super::DocumentExtractTool::new(
        platform.state_fs(),
    )));
    #[cfg(feature = "tools_network_extra")]
    registry.register(Box::new(super::WebSearchTool::new(config)));
    #[cfg(all(
        feature = "tools_network_extra",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    ))]
    registry.register(Box::new(super::WebFetchTool));
    #[cfg(all(
        feature = "tools_network_extra",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    ))]
    registry.register(Box::new(super::PdfReadTool));
    #[cfg(feature = "tools_network_extra")]
    registry.register(Box::new(super::AnalyzeImageTool::new(config)));
    let remind_at_store_for_list = Arc::clone(&remind_at_store);
    registry.register(Box::new(super::RemindAtTool::new(remind_at_store)));
    registry.register(Box::new(super::RemindListTool::new(
        remind_at_store_for_list,
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
    registry.register(Box::new(super::MemoryManageTool::new(
        Arc::clone(&_memory_store),
        Arc::clone(&_long_term_memory_store),
    )));
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::{ToolExposure, ToolMetadata};
    use serde_json::json;

    struct VisibleTool;
    struct StatefulTool;
    struct AdminTool;
    struct InternalOnlyTool;
    struct UserOnlyTaskTool;

    impl Tool for VisibleTool {
        fn name(&self) -> &'static str {
            "visible"
        }
        fn description(&self) -> &str {
            "visible tool"
        }
        fn schema(&self) -> serde_json::Value {
            json!({"type":"object","properties":{"x":{"type":"string"}}})
        }
        fn execute(&self, _args: &str, _ctx: &mut dyn crate::tools::ToolContext) -> Result<String> {
            Ok(String::new())
        }
    }

    impl Tool for StatefulTool {
        fn name(&self) -> &'static str {
            "stateful"
        }
        fn description(&self) -> &str {
            "stateful tool"
        }
        fn schema(&self) -> serde_json::Value {
            json!({"type":"object"})
        }
        fn execute(&self, _args: &str, _ctx: &mut dyn crate::tools::ToolContext) -> Result<String> {
            Ok(String::new())
        }
        fn metadata(&self) -> ToolMetadata {
            ToolMetadata::stateful()
        }
    }

    impl Tool for AdminTool {
        fn name(&self) -> &'static str {
            "admin"
        }
        fn description(&self) -> &str {
            "admin tool"
        }
        fn schema(&self) -> serde_json::Value {
            json!({"type":"object"})
        }
        fn execute(&self, _args: &str, _ctx: &mut dyn crate::tools::ToolContext) -> Result<String> {
            Ok(String::new())
        }
        fn metadata(&self) -> ToolMetadata {
            ToolMetadata::admin()
        }
    }

    impl Tool for InternalOnlyTool {
        fn name(&self) -> &'static str {
            "internal_only"
        }
        fn description(&self) -> &str {
            "internal only tool"
        }
        fn schema(&self) -> serde_json::Value {
            json!({"type":"object"})
        }
        fn execute(&self, _args: &str, _ctx: &mut dyn crate::tools::ToolContext) -> Result<String> {
            Ok(String::new())
        }
        fn metadata(&self) -> ToolMetadata {
            ToolMetadata {
                exposure: ToolExposure::Task,
                allow_in_system_ingress: false,
                allow_in_system_channel: true,
            }
        }
    }

    impl Tool for UserOnlyTaskTool {
        fn name(&self) -> &'static str {
            "user_only_task"
        }
        fn description(&self) -> &str {
            "user only task"
        }
        fn schema(&self) -> serde_json::Value {
            json!({"type":"object"})
        }
        fn execute(&self, _args: &str, _ctx: &mut dyn crate::tools::ToolContext) -> Result<String> {
            Ok(String::new())
        }
        fn metadata(&self) -> ToolMetadata {
            ToolMetadata::task().with_system_ingress(false)
        }
    }

    #[test]
    fn llm_tool_specs_follow_runtime_policy() {
        let mut registry = ToolRegistry::new();
        registry.register(Box::new(VisibleTool));
        registry.register(Box::new(StatefulTool));
        registry.register(Box::new(AdminTool));
        registry.register(Box::new(InternalOnlyTool));
        registry.register(Box::new(UserOnlyTaskTool));

        let user = ToolPolicyContext::new(crate::bus::IngressKind::User, "telegram");
        let user_specs = registry.tool_specs_for_llm_with_max(&user, 4096);
        let user_names: Vec<&str> = user_specs.iter().map(|spec| spec.name.as_str()).collect();
        assert_eq!(
            user_names,
            vec!["visible", "stateful", "internal_only", "user_only_task"]
        );

        let system = ToolPolicyContext::new(crate::bus::IngressKind::System, "telegram");
        let system_specs = registry.tool_specs_for_llm_with_max(&system, 4096);
        let system_names: Vec<&str> = system_specs.iter().map(|spec| spec.name.as_str()).collect();
        assert_eq!(system_names, vec!["visible"]);

        let cron = ToolPolicyContext::new(crate::bus::IngressKind::System, "cron");
        let specs = registry.tool_specs_for_llm_with_max(&cron, 4096);
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].name, "internal_only");
    }
}
