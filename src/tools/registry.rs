//! ToolRegistry：按 name 注册与查找，生成 API 用 tool specs。
//! ToolRegistry: register, get by name, tool_specs for API.

use crate::config::AppConfig;
use crate::error::{Error, Result};
use crate::llm::ToolSpec as LlmToolSpec;
use crate::tools::{
    MAX_TOOL_ARGS_LEN, MAX_TOOL_RESULT_LEN, Tool, ToolExecutionGateDecision,
    ToolExecutionGovernance, ToolExecutionGovernanceState, ToolExecutionOutcome,
    ToolExecutionPermit, ToolExecutionRecord, ToolExecutionRequest, ToolMetadata,
    ToolPolicyContext,
};
use crate::util::truncate_to_byte_len;
use indexmap::IndexMap;
use serde::Serialize;
use std::sync::Arc;

pub const DEFAULT_LLM_TOOL_SPECS_MAX_TOTAL_LEN: usize = 32 * 1024;

struct RegisteredTool {
    tool: Box<dyn Tool>,
    llm_spec: LlmToolSpec,
    metadata: ToolMetadata,
    requires_network: bool,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct ToolCatalogEntry {
    pub name: String,
    pub exposure: String,
    pub effect_class: String,
    pub risk_level: String,
    pub approval_mode: String,
    pub rollback_kind: String,
    pub requires_network: bool,
    pub llm_visible_user: bool,
    pub llm_visible_system: bool,
    pub llm_visible_internal_system: bool,
    pub governance_breaker_tripped: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub governance_last_status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub governance_last_reason: Option<String>,
}

/// 按 name 注册与派发工具；可生成带总长度上界的 tool specs。IndexMap 保证工具顺序稳定。
pub struct ToolRegistry {
    tools: IndexMap<&'static str, RegisteredTool>,
    execution_governance: Option<Arc<ToolExecutionGovernance>>,
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
            execution_governance: None,
        }
    }

    pub fn with_execution_governance(
        mut self,
        execution_governance: Arc<ToolExecutionGovernance>,
    ) -> Self {
        self.execution_governance = Some(execution_governance);
        self
    }

    pub fn register(&mut self, tool: Box<dyn Tool>) {
        let name = tool.name();
        let metadata = tool.metadata();
        let requires_network = tool.requires_network();
        let llm_spec = LlmToolSpec {
            name: name.to_string(),
            description: tool.description().to_string(),
            parameters_json: tool.schema().to_owned().into_boxed_str(),
        };
        self.tools.insert(
            name,
            RegisteredTool {
                tool,
                llm_spec,
                metadata,
                requires_network,
            },
        );
    }

    pub fn get(&self, name: &str) -> Option<&dyn Tool> {
        self.tools.get(name).map(|entry| entry.tool.as_ref())
    }

    /// API / 调试使用：返回当前注册顺序下的工具名列表。
    pub fn tool_names(&self) -> Vec<&'static str> {
        self.tools.keys().copied().collect()
    }

    /// 该工具是否需要网络（从 Tool trait 推导）。未注册工具返回 false。
    /// Whether the named tool requires network (derived from Tool trait). Returns false for unknown tools.
    pub fn is_network_tool(&self, name: &str) -> bool {
        self.tools
            .get(name)
            .is_some_and(|entry| entry.requires_network)
    }

    /// 该工具在本次 LLM 请求上下文中是否可见。
    pub fn is_llm_tool_visible(&self, name: &str, policy: &ToolPolicyContext<'_>) -> bool {
        self.tools
            .get(name)
            .is_some_and(|entry| entry.metadata.is_exposed_to_llm(policy))
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
        for entry in self.tools.values() {
            if !entry.metadata.is_exposed_to_llm(policy) {
                continue;
            }
            let add_len = entry.llm_spec.name.len()
                + entry.llm_spec.description.len()
                + entry.llm_spec.parameters_json.len()
                + 2;
            if len + add_len > max_total_len && !out.is_empty() {
                break;
            }
            len += add_len;
            out.push(entry.llm_spec.clone());
        }
        out
    }

    /// 当前上下文下是否存在至少一个可暴露给 LLM 的工具。
    pub fn has_llm_visible_tools(&self, policy: &ToolPolicyContext<'_>) -> bool {
        self.tools
            .values()
            .any(|entry| entry.metadata.is_exposed_to_llm(policy))
    }

    /// 按 name 执行工具；args 超限返回 Error::Config；返回值在 Registry 内截断至 MAX_TOOL_RESULT_LEN。
    pub fn execute(
        &self,
        name: &str,
        args: &str,
        ctx: &mut dyn crate::tools::ToolContext,
    ) -> Result<ToolExecutionOutcome> {
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
        let mut outcome = tool.execute_outcome(args, ctx)?;
        outcome.content = truncate_to_byte_len(&outcome.content, MAX_TOOL_RESULT_LEN);
        Ok(outcome)
    }

    pub fn assess_llm_execution(
        &self,
        name: &str,
        args: &str,
        policy: &ToolPolicyContext<'_>,
    ) -> Result<ToolExecutionGateDecision> {
        if args.len() > MAX_TOOL_ARGS_LEN {
            return Err(Error::config(
                "tool_execute",
                format!("args length exceeds {}", MAX_TOOL_ARGS_LEN),
            ));
        }
        let Some(entry) = self.tools.get(name) else {
            return Err(Error::Other {
                source: Box::new(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("tool not found: {name}"),
                )),
                stage: "tool_execute",
            });
        };
        let shape = entry.tool.execution_shape(args)?;
        let Some(governance) = self.execution_governance.as_ref() else {
            return Ok(ToolExecutionGateDecision::Allow(ToolExecutionPermit {
                tool_name: name.to_string(),
                ingress: policy.ingress,
                channel: policy.channel.to_string(),
                metadata: entry.metadata,
                shape,
                requires_network: entry.requires_network,
            }));
        };
        governance.assess(ToolExecutionRequest {
            tool_name: name.to_string(),
            ingress: policy.ingress,
            channel: policy.channel.to_string(),
            metadata: entry.metadata,
            shape,
            requires_network: entry.requires_network,
        })
    }

    pub fn record_resource_denial(&self, permit: &ToolExecutionPermit, reason: &str) -> Result<()> {
        if let Some(governance) = self.execution_governance.as_ref() {
            governance.record_resource_denial(permit, reason)?;
        }
        Ok(())
    }

    pub fn execute_permitted(
        &self,
        permit: &ToolExecutionPermit,
        args: &str,
        ctx: &mut dyn crate::tools::ToolContext,
    ) -> Result<ToolExecutionOutcome> {
        let tool = self.get(permit.tool_name()).ok_or_else(|| Error::Other {
            source: Box::new(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("tool not found: {}", permit.tool_name()),
            )),
            stage: "tool_execute",
        })?;
        let mut outcome = tool.execute_outcome(args, ctx)?;
        outcome.content = truncate_to_byte_len(&outcome.content, MAX_TOOL_RESULT_LEN);
        if let Some(governance) = self.execution_governance.as_ref() {
            if let Err(error) = governance.record_success(permit, &outcome) {
                log::warn!(
                    "[tool_registry] failed to persist success audit for {}: {}",
                    permit.tool_name(),
                    error
                );
            }
        }
        Ok(outcome)
    }

    pub fn record_execution_failure(
        &self,
        permit: &ToolExecutionPermit,
        error: &Error,
    ) -> Result<()> {
        if let Some(governance) = self.execution_governance.as_ref() {
            governance.record_failure(permit, error)?;
        }
        Ok(())
    }

    pub fn inspect_execution_governance(&self) -> Result<Option<ToolExecutionGovernanceState>> {
        match self.execution_governance.as_ref() {
            Some(governance) => governance.inspect().map(Some),
            None => Ok(None),
        }
    }

    pub fn tool_catalog(&self) -> Result<Vec<ToolCatalogEntry>> {
        let governance = self.inspect_execution_governance()?;
        let user_policy = ToolPolicyContext::new(crate::bus::IngressKind::User, "telegram");
        let system_policy = ToolPolicyContext::new(crate::bus::IngressKind::System, "telegram");
        let internal_policy = ToolPolicyContext::new(crate::bus::IngressKind::System, "cron");
        let mut out = Vec::with_capacity(self.tools.len());
        for (name, entry) in &self.tools {
            let metadata = entry.metadata;
            let shape = metadata.default_execution_shape(name);
            let breaker_tripped = governance
                .as_ref()
                .and_then(|state| {
                    state
                        .breakers
                        .iter()
                        .find(|breaker| breaker.tool_name == *name)
                })
                .is_some_and(|breaker| breaker.tripped_until > crate::util::current_unix_secs());
            let last_record = governance
                .as_ref()
                .and_then(|state| last_record_for_tool(state, name));
            out.push(ToolCatalogEntry {
                name: (*name).to_string(),
                exposure: metadata.exposure.label().to_string(),
                effect_class: shape.effect_class.label().to_string(),
                risk_level: shape.risk_level.label().to_string(),
                approval_mode: shape.approval_mode.label().to_string(),
                rollback_kind: shape.rollback_kind.label().to_string(),
                requires_network: entry.requires_network,
                llm_visible_user: metadata.is_exposed_to_llm(&user_policy),
                llm_visible_system: metadata.is_exposed_to_llm(&system_policy),
                llm_visible_internal_system: metadata.is_exposed_to_llm(&internal_policy),
                governance_breaker_tripped: breaker_tripped,
                governance_last_status: last_record.map(|record| record.status.label().to_string()),
                governance_last_reason: last_record
                    .map(|record| record.reason.trim())
                    .filter(|reason| !reason.is_empty())
                    .map(str::to_string),
            });
        }
        Ok(out)
    }
}

fn last_record_for_tool<'a>(
    state: &'a ToolExecutionGovernanceState,
    tool_name: &str,
) -> Option<&'a ToolExecutionRecord> {
    state
        .recent_records
        .iter()
        .rev()
        .find(|record| record.tool_name == tool_name)
}

/// 构建包含所有内置工具的注册表。`platform` 用于 `board_info` 等依赖平台能力的工具。
/// Returns `(registry, Option<baidu_token_cache>)` — the cache is shared with voice_session.
pub struct DefaultRegistryDeps {
    pub platform: Arc<dyn crate::Platform>,
    pub remind_at_store: Arc<dyn crate::memory::RemindAtStore + Send + Sync>,
    pub session_store: Arc<dyn crate::memory::SessionStore + Send + Sync>,
    pub memory_store: Arc<dyn crate::memory::MemoryStore + Send + Sync>,
    pub long_term_memory_store: Arc<dyn crate::memory::LongTermMemoryStore + Send + Sync>,
    pub turn_ledger_store: Arc<dyn crate::memory::TurnLedgerStore + Send + Sync>,
    pub private_garden_store: Arc<dyn crate::memory::PrivateGardenStore + Send + Sync>,
    pub config_store: Arc<dyn crate::platform::ConfigStore + Send + Sync>,
}

#[cold]
#[inline(never)]
fn register_core_tools(
    registry: &mut ToolRegistry,
    config: &AppConfig,
    platform: &Arc<dyn crate::Platform>,
    tool_execution_governance: &Arc<ToolExecutionGovernance>,
    remind_at_store: &Arc<dyn crate::memory::RemindAtStore + Send + Sync>,
    session_store: &Arc<dyn crate::memory::SessionStore + Send + Sync>,
    memory_store: &Arc<dyn crate::memory::MemoryStore + Send + Sync>,
    long_term_memory_store: &Arc<dyn crate::memory::LongTermMemoryStore + Send + Sync>,
    turn_ledger_store: &Arc<dyn crate::memory::TurnLedgerStore + Send + Sync>,
    private_garden_store: &Arc<dyn crate::memory::PrivateGardenStore + Send + Sync>,
) {
    registry.register(Box::new(super::GetTimeTool));
    registry.register(Box::new(super::EnvTool));
    registry.register(Box::new(super::MessageTool));
    registry.register(Box::new(super::TaskTool::new(
        platform.task_store(),
        platform.calendar_store(),
        platform.calendar_provider_credential_store(),
    )));
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
    #[cfg(all(
        feature = "tools_network_extra",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    ))]
    registry.register(Box::new(super::AnalyzeImageTool::new(config)));
    registry.register(Box::new(super::RemindAtTool::new(Arc::clone(
        remind_at_store,
    ))));
    registry.register(Box::new(super::RemindListTool::new(Arc::clone(
        remind_at_store,
    ))));
    registry.register(Box::new(super::BoardInfoTool::new(Arc::clone(platform))));
    registry.register(Box::new(super::KvStoreTool::new(platform.state_fs())));
    registry.register(Box::new(super::PrivateGardenTool::new(Arc::clone(
        private_garden_store,
    ))));
    registry.register(Box::new(super::FactualMemoryTool::new(Arc::clone(
        long_term_memory_store,
    ))));
    registry.register(Box::new(super::MemorySearchTool::new(
        Arc::clone(session_store),
        Arc::clone(memory_store),
        Arc::clone(turn_ledger_store),
    )));
    registry.register(Box::new(super::MemoryGetTool::new(
        Arc::clone(session_store),
        Arc::clone(memory_store),
        Arc::clone(turn_ledger_store),
    )));
    registry.register(Box::new(super::ContinuitySnapshotTool::new(
        platform.state_fs(),
        Arc::clone(session_store),
        Arc::clone(memory_store),
        platform.long_term_memory_store(),
        platform.session_summary_store(),
        platform.execution_state_store(),
        platform.self_model_store(),
        platform.self_authored_core_store(),
        platform.core_revision_ledger_store(),
        platform.self_continuity_store(),
        Arc::clone(turn_ledger_store),
        platform.relationship_constitution_store(),
        platform.relationship_portfolio_store(),
        platform.relationship_topology_store(),
        platform.skill_storage(),
        Arc::clone(tool_execution_governance),
    )));
    #[cfg(feature = "tools_diagnostics")]
    if !config.hardware_devices.is_empty() {
        registry.register(Box::new(super::DeviceControlTool::new(
            config.hardware_devices.clone(),
            Arc::clone(platform),
        )));
    }
}

#[cold]
#[inline(never)]
fn register_extended_runtime_tools(
    registry: &mut ToolRegistry,
    config: &AppConfig,
    platform: &Arc<dyn crate::Platform>,
    tool_execution_governance: &Arc<ToolExecutionGovernance>,
    memory_store: &Arc<dyn crate::memory::MemoryStore + Send + Sync>,
    long_term_memory_store: &Arc<dyn crate::memory::LongTermMemoryStore + Send + Sync>,
    session_store: &Arc<dyn crate::memory::SessionStore + Send + Sync>,
    config_store: &Arc<dyn crate::platform::ConfigStore + Send + Sync>,
) {
    #[cfg(feature = "tools_diagnostics")]
    registry.register(Box::new(super::MemoryManageTool::new(
        Arc::clone(memory_store),
        Arc::clone(long_term_memory_store),
        platform.skill_storage(),
    )));
    #[cfg(all(
        feature = "tools_network_extra",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    ))]
    registry.register(Box::new(super::HttpRequestTool));
    #[cfg(feature = "tools_diagnostics")]
    registry.register(Box::new(super::SessionManageTool::new(Arc::clone(
        session_store,
    ))));
    registry.register(Box::new(super::FileWriteTool::new(platform.state_fs())));
    #[cfg(feature = "tools_diagnostics")]
    registry.register(Box::new(super::SystemControlTool::new(
        Arc::clone(platform),
        Arc::clone(tool_execution_governance),
    )));
    #[cfg(feature = "tools_diagnostics")]
    registry.register(Box::new(super::CronManageTool::new(Arc::clone(
        memory_store,
    ))));
    #[cfg(feature = "tools_network_extra")]
    registry.register(Box::new(super::ProxyConfigTool::new(Arc::clone(
        config_store,
    ))));
    #[cfg(feature = "tools_network_extra")]
    registry.register(Box::new(super::ModelConfigTool::new(Arc::clone(platform))));
    #[cfg(feature = "tools_diagnostics")]
    registry.register(Box::new(super::NetworkScanTool::new(Arc::clone(platform))));
    #[cfg(feature = "tools_diagnostics")]
    if !config.hardware_devices.is_empty() || !config.i2c_sensors.is_empty() {
        registry.register(Box::new(super::SensorWatchTool::new(
            Arc::clone(memory_store),
            config.hardware_devices.clone(),
            config.i2c_sensors.clone(),
        )));
    }
    #[cfg(feature = "tools_diagnostics")]
    if config.i2c_bus.is_some() && !config.i2c_devices.is_empty() {
        registry.register(Box::new(super::I2cDeviceTool::new(
            Arc::clone(platform),
            config.i2c_devices.clone(),
        )));
    }
    #[cfg(feature = "tools_diagnostics")]
    if config.i2c_bus.is_some() && !config.i2c_sensors.is_empty() {
        registry.register(Box::new(super::I2cSensorTool::new(
            Arc::clone(platform),
            config.i2c_sensors.clone(),
        )));
    }
}

#[cold]
#[inline(never)]
fn register_audio_tools(
    registry: &mut ToolRegistry,
    config: &AppConfig,
    platform: &Arc<dyn crate::Platform>,
) -> Option<Arc<crate::audio::baidu_token::BaiduTokenCache>> {
    let Some(audio_cfg) = config.audio.clone() else {
        return None;
    };
    let baidu_speech_credentials_ok = !audio_cfg.speech.api_key.trim().is_empty()
        && !audio_cfg.speech.api_secret.trim().is_empty();
    let speech_input_ok = audio_cfg.service_provider == "baidu"
        && baidu_speech_credentials_ok
        && audio_cfg.microphone.enabled;
    let speech_output_ok = audio_cfg.service_provider == "baidu"
        && baidu_speech_credentials_ok
        && audio_cfg.speaker.enabled;
    let baidu_token_cache = if audio_cfg.enabled && (speech_input_ok || speech_output_ok) {
        Some(Arc::new(crate::audio::baidu_token::BaiduTokenCache::new()))
    } else {
        None
    };
    if audio_cfg.enabled && speech_input_ok {
        if let Some(ref cache) = baidu_token_cache {
            registry.register(Box::new(super::VoiceInputTool::new(
                Arc::clone(platform),
                audio_cfg.clone(),
                Arc::clone(cache),
            )));
        }
    }
    if audio_cfg.enabled && speech_output_ok {
        if let Some(ref cache) = baidu_token_cache {
            registry.register(Box::new(super::VoiceOutputTool::new(
                Arc::clone(platform),
                audio_cfg,
                Arc::clone(cache),
            )));
        }
    }
    baidu_token_cache
}

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
#[cold]
#[inline(never)]
fn register_host_only_tools(registry: &mut ToolRegistry) {
    registry.register(Box::new(super::ShellTool));
    registry.register(Box::new(super::ProcessTool));
    registry.register(Box::new(super::NetworkTool));
}

pub fn build_default_registry(
    config: &AppConfig,
    deps: DefaultRegistryDeps,
) -> (
    ToolRegistry,
    Option<Arc<crate::audio::baidu_token::BaiduTokenCache>>,
) {
    let DefaultRegistryDeps {
        platform,
        remind_at_store,
        session_store,
        memory_store,
        long_term_memory_store,
        turn_ledger_store,
        private_garden_store,
        config_store,
    } = deps;
    let tool_execution_governance = Arc::new(ToolExecutionGovernance::new(platform.state_fs()));
    let mut registry =
        ToolRegistry::new().with_execution_governance(Arc::clone(&tool_execution_governance));
    register_core_tools(
        &mut registry,
        config,
        &platform,
        &tool_execution_governance,
        &remind_at_store,
        &session_store,
        &memory_store,
        &long_term_memory_store,
        &turn_ledger_store,
        &private_garden_store,
    );
    register_extended_runtime_tools(
        &mut registry,
        config,
        &platform,
        &tool_execution_governance,
        &memory_store,
        &long_term_memory_store,
        &session_store,
        &config_store,
    );
    let shared_baidu_token = register_audio_tools(&mut registry, config, &platform);
    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
    register_host_only_tools(&mut registry);
    (registry, shared_baidu_token)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::{ToolExposure, ToolMetadata};
    struct VisibleTool;
    struct StatefulTool;
    struct AdminTool;
    struct InternalOnlyTool;
    struct UserOnlyTaskTool;
    struct OutcomeTool;
    struct StubToolContext;

    impl Tool for VisibleTool {
        fn name(&self) -> &'static str {
            "visible"
        }
        fn description(&self) -> &str {
            "visible tool"
        }
        fn schema(&self) -> &str {
            r#"{"type":"object","properties":{"x":{"type":"string"}}}"#
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
        fn schema(&self) -> &str {
            r#"{"type":"object"}"#
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
        fn schema(&self) -> &str {
            r#"{"type":"object"}"#
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
        fn schema(&self) -> &str {
            r#"{"type":"object"}"#
        }
        fn execute(&self, _args: &str, _ctx: &mut dyn crate::tools::ToolContext) -> Result<String> {
            Ok(String::new())
        }
        fn metadata(&self) -> ToolMetadata {
            ToolMetadata {
                exposure: ToolExposure::Task,
                allow_in_system_ingress: false,
                allow_in_system_channel: true,
                ..ToolMetadata::task()
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
        fn schema(&self) -> &str {
            r#"{"type":"object"}"#
        }
        fn execute(&self, _args: &str, _ctx: &mut dyn crate::tools::ToolContext) -> Result<String> {
            Ok(String::new())
        }
        fn metadata(&self) -> ToolMetadata {
            ToolMetadata::task().with_system_ingress(false)
        }
    }

    impl Tool for OutcomeTool {
        fn name(&self) -> &'static str {
            "outcome"
        }
        fn description(&self) -> &str {
            "outcome tool"
        }
        fn schema(&self) -> &str {
            r#"{"type":"object"}"#
        }
        fn execute(&self, _args: &str, _ctx: &mut dyn crate::tools::ToolContext) -> Result<String> {
            Ok("legacy".to_string())
        }
        fn execute_outcome(
            &self,
            _args: &str,
            _ctx: &mut dyn crate::tools::ToolContext,
        ) -> Result<ToolExecutionOutcome> {
            Ok(ToolExecutionOutcome::text("outcome body")
                .with_current_chat_reply("tool delivered reply"))
        }
    }

    impl crate::tools::ToolContext for StubToolContext {
        fn get_with_headers(
            &mut self,
            _url: &str,
            _headers: &[(&str, &str)],
        ) -> Result<(u16, crate::platform::ResponseBody)> {
            unreachable!()
        }

        fn post_with_headers(
            &mut self,
            _url: &str,
            _headers: &[(&str, &str)],
            _body: &[u8],
        ) -> Result<(u16, crate::platform::ResponseBody)> {
            unreachable!()
        }

        fn user_locale(&self) -> crate::i18n::Locale {
            crate::i18n::Locale::Zh
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

    #[test]
    fn registry_execute_preserves_structured_outcome() {
        let mut registry = ToolRegistry::new();
        registry.register(Box::new(OutcomeTool));
        let mut ctx = StubToolContext;
        let outcome = registry
            .execute("outcome", "{}", &mut ctx)
            .expect("execute");
        assert_eq!(outcome.content, "outcome body");
        assert_eq!(
            outcome.outbound_intents.as_slice(),
            &[crate::tools::ToolOutboundIntent {
                target: crate::tools::ToolOutboundTarget::CurrentChat,
                delivery_kind: crate::tools::ToolOutboundDeliveryKind::Primary,
                content: "tool delivered reply".to_string(),
            }]
        );
    }
}
