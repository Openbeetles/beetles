//! ToolRegistry：按 name 注册与查找，生成 API 用 tool specs。
//! ToolRegistry: register, get by name, tool_specs for API.
#![allow(clippy::too_many_arguments)]

use crate::config::AppConfig;
use crate::error::{Error, Result};
use crate::llm::ToolSpec as LlmToolSpec;
use crate::tools::{
    Tool, ToolApprovalMode, ToolCapabilityContract, ToolCatalogAuthority, ToolEffectClass,
    ToolExecutionGateDecision, ToolExecutionGovernance, ToolExecutionGovernanceState,
    ToolExecutionOutcome, ToolExecutionPermit, ToolExecutionRecord, ToolExecutionRequest,
    ToolInputProtocolKind, ToolMetadata, ToolOutputProtocolKind, ToolPolicyContext,
    ToolProtocolAuthority, ToolRiskLevel, ToolRollbackKind, MAX_TOOL_ARGS_LEN, MAX_TOOL_RESULT_LEN,
};
use crate::util::truncate_to_byte_len;
use indexmap::IndexMap;
use serde::Serialize;
use std::sync::Arc;

pub const DEFAULT_LLM_TOOL_SPECS_MAX_TOTAL_LEN: usize = 32 * 1024;

type LlmVisibilityOverlayProvider = Arc<
    dyn Fn(
            crate::bus::IngressKind,
            &str,
        ) -> crate::capability_package::CapabilityPackageToolPolicySet
        + Send
        + Sync,
>;

struct RegisteredTool {
    tool: Box<dyn Tool>,
    llm_spec: LlmToolSpec,
    metadata: ToolMetadata,
    requires_network: bool,
    capability_contract: ToolCapabilityContract,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct ToolCatalogEntry {
    pub name: String,
    pub exposure: String,
    pub input_protocol: String,
    pub output_protocol: String,
    pub supports_rich_blockers: bool,
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

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct ToolBridgeCatalogEntry {
    pub name: String,
    pub description: String,
    pub parameters_json: String,
    pub input_protocol: ToolInputProtocolKind,
    pub output_protocol: ToolOutputProtocolKind,
    pub supports_rich_blockers: bool,
    pub effect_class: ToolEffectClass,
    pub risk_level: ToolRiskLevel,
    pub approval_mode: ToolApprovalMode,
    pub rollback_kind: ToolRollbackKind,
    pub requires_network: bool,
    pub required_runtime_capabilities: Vec<String>,
    pub allow_when_degraded: bool,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ToolBridgeProposalDecision {
    Allowed,
    Denied,
    UnknownTool,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct ToolBridgeProposalAssessment {
    pub tool_name: String,
    pub decision: ToolBridgeProposalDecision,
    pub summary: String,
    pub effect_class: ToolEffectClass,
    pub risk_level: ToolRiskLevel,
    pub approval_mode: ToolApprovalMode,
    pub rollback_kind: ToolRollbackKind,
    pub requires_network: bool,
    pub required_runtime_capabilities: Vec<String>,
    pub allow_when_degraded: bool,
}

/// 按 name 注册与派发工具；可生成带总长度上界的 tool specs。IndexMap 保证工具顺序稳定。
pub struct ToolRegistry {
    tools: IndexMap<&'static str, RegisteredTool>,
    execution_governance: Option<Arc<ToolExecutionGovernance>>,
    llm_visibility_overlay_provider: Option<LlmVisibilityOverlayProvider>,
    llm_catalog_authority: Arc<ToolCatalogAuthority>,
    tool_protocol_authority: Arc<ToolProtocolAuthority>,
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
            llm_visibility_overlay_provider: None,
            llm_catalog_authority: Arc::new(ToolCatalogAuthority::default()),
            tool_protocol_authority: Arc::new(ToolProtocolAuthority::default()),
        }
    }

    pub fn with_execution_governance(
        mut self,
        execution_governance: Arc<ToolExecutionGovernance>,
    ) -> Self {
        self.execution_governance = Some(execution_governance);
        self
    }

    pub fn set_llm_visibility_overlay_provider(&mut self, provider: LlmVisibilityOverlayProvider) {
        self.llm_visibility_overlay_provider = Some(provider);
    }

    pub fn with_llm_catalog_authority(mut self, authority: Arc<ToolCatalogAuthority>) -> Self {
        self.llm_catalog_authority = authority;
        self
    }

    pub fn set_llm_catalog_authority(&mut self, authority: Arc<ToolCatalogAuthority>) {
        self.llm_catalog_authority = authority;
    }

    pub fn with_tool_protocol_authority(mut self, authority: Arc<ToolProtocolAuthority>) -> Self {
        self.tool_protocol_authority = authority;
        self
    }

    pub fn set_tool_protocol_authority(&mut self, authority: Arc<ToolProtocolAuthority>) {
        self.tool_protocol_authority = authority;
    }

    pub fn register(&mut self, tool: Box<dyn Tool>) {
        let name = tool.name();
        let metadata = tool.metadata();
        let requires_network = tool.requires_network();
        let capability_contract = tool.capability_contract();
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
                capability_contract,
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

    pub fn missing_llm_catalog_entries(&self) -> Vec<String> {
        self.llm_catalog_authority
            .missing_entries(self.tools.keys().copied())
    }

    pub fn missing_tool_protocol_entries(&self) -> Vec<String> {
        self.tool_protocol_authority
            .missing_entries(self.tools.keys().copied())
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
        let Some(entry) = self.tools.get(name) else {
            return false;
        };
        let overlay_set = self.llm_visibility_overlay_set(policy);
        self.is_entry_llm_visible(entry, name, policy, overlay_set.as_ref())
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
        let overlay_set = self.llm_visibility_overlay_set(policy);
        for (name, entry) in &self.tools {
            if !self.is_entry_llm_visible(entry, name, policy, overlay_set.as_ref()) {
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
        let overlay_set = self.llm_visibility_overlay_set(policy);
        self.tools.iter().any(|(name, entry)| {
            self.is_entry_llm_visible(entry, name, policy, overlay_set.as_ref())
        })
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
        let protocol = self.tool_protocol_contract(name);
        validate_tool_input_protocol(name, args, protocol)?;
        if let Some(blocker) = self.runtime_capability_blocker(name) {
            return Err(runtime_capability_error(name, &blocker));
        }
        let shape = tool.execution_shape(args)?;
        enforce_direct_execution_governance(
            self.tools
                .get(name)
                .map(|entry| entry.metadata)
                .unwrap_or_else(|| tool.metadata()),
            &shape,
        )?;
        let mut outcome = tool.execute_outcome(args, ctx)?;
        normalize_and_validate_tool_outcome(name, protocol, &mut outcome)?;
        if outcome.is_success() {
            self.observe_runtime_capability_success(name);
        }
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
        validate_tool_input_protocol(name, args, self.tool_protocol_contract(name))?;
        let shape = entry.tool.execution_shape(args)?;
        let requires_network = entry.tool.requires_network_for(args)?;
        let Some(governance) = self.execution_governance.as_ref() else {
            return Ok(ToolExecutionGateDecision::Allow(ToolExecutionPermit {
                tool_name: name.to_string(),
                ingress: policy.ingress,
                channel: policy.channel.to_string(),
                metadata: entry.metadata,
                shape,
                requires_network,
            }));
        };
        governance.assess(ToolExecutionRequest {
            tool_name: name.to_string(),
            ingress: policy.ingress,
            channel: policy.channel.to_string(),
            metadata: entry.metadata,
            shape,
            requires_network,
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
        if let Some(blocker) = self.runtime_capability_blocker(permit.tool_name()) {
            return Err(runtime_capability_error(permit.tool_name(), &blocker));
        }
        let tool = self.get(permit.tool_name()).ok_or_else(|| Error::Other {
            source: Box::new(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("tool not found: {}", permit.tool_name()),
            )),
            stage: "tool_execute",
        })?;
        let protocol = self.tool_protocol_contract(permit.tool_name());
        validate_tool_input_protocol(permit.tool_name(), args, protocol)?;
        let mut outcome = tool.execute_outcome(args, ctx)?;
        normalize_and_validate_tool_outcome(permit.tool_name(), protocol, &mut outcome)?;
        if outcome.is_success() {
            self.observe_runtime_capability_success(permit.tool_name());
            if let Some(governance) = self.execution_governance.as_ref() {
                if let Err(error) = governance.record_success(permit, &outcome) {
                    log::warn!(
                        "[tool_registry] failed to persist success audit for {}: {}",
                        permit.tool_name(),
                        error
                    );
                }
            }
        }
        Ok(outcome)
    }

    pub fn record_execution_failure(
        &self,
        permit: &ToolExecutionPermit,
        error: &Error,
    ) -> Result<()> {
        self.observe_runtime_capability_failure(permit.tool_name(), error);
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

    fn tool_protocol_contract(&self, tool_name: &str) -> crate::tools::ToolProtocolContract {
        self.tool_protocol_authority
            .get(tool_name)
            .unwrap_or_else(crate::tools::ToolProtocolContract::structured_object_json)
    }

    pub fn tool_catalog(&self) -> Result<Vec<ToolCatalogEntry>> {
        let governance = self.inspect_execution_governance()?;
        let user_policy = ToolPolicyContext::new(crate::bus::IngressKind::User, "telegram");
        let system_policy = ToolPolicyContext::new(crate::bus::IngressKind::System, "telegram");
        let internal_policy = ToolPolicyContext::new(crate::bus::IngressKind::System, "cron");
        let user_overlay_set = self.llm_visibility_overlay_set(&user_policy);
        let system_overlay_set = self.llm_visibility_overlay_set(&system_policy);
        let internal_overlay_set = self.llm_visibility_overlay_set(&internal_policy);
        let mut out = Vec::with_capacity(self.tools.len());
        for (name, entry) in &self.tools {
            let metadata = entry.metadata;
            let shape = entry.tool.catalog_execution_shape();
            let protocol = self.tool_protocol_contract(name);
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
                input_protocol: protocol.input_kind.label().to_string(),
                output_protocol: protocol.output_kind.label().to_string(),
                supports_rich_blockers: protocol.supports_rich_blockers,
                effect_class: shape.effect_class.label().to_string(),
                risk_level: shape.risk_level.label().to_string(),
                approval_mode: shape.approval_mode.label().to_string(),
                rollback_kind: shape.rollback_kind.label().to_string(),
                requires_network: entry.requires_network,
                llm_visible_user: self.is_entry_llm_visible(
                    entry,
                    name,
                    &user_policy,
                    user_overlay_set.as_ref(),
                ),
                llm_visible_system: self.is_entry_llm_visible(
                    entry,
                    name,
                    &system_policy,
                    system_overlay_set.as_ref(),
                ),
                llm_visible_internal_system: self.is_entry_llm_visible(
                    entry,
                    name,
                    &internal_policy,
                    internal_overlay_set.as_ref(),
                ),
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

    pub fn tool_bridge_catalog_for_policy(
        &self,
        policy: &ToolPolicyContext<'_>,
    ) -> Vec<ToolBridgeCatalogEntry> {
        let overlay_set = self.llm_visibility_overlay_set(policy);
        let mut out = Vec::new();
        for (name, entry) in &self.tools {
            if !self.is_entry_llm_visible(entry, name, policy, overlay_set.as_ref()) {
                continue;
            }
            let shape = entry.tool.catalog_execution_shape();
            let protocol = self.tool_protocol_contract(name);
            out.push(ToolBridgeCatalogEntry {
                name: (*name).to_string(),
                description: entry.llm_spec.description.to_string(),
                parameters_json: entry.llm_spec.parameters_json.to_string(),
                input_protocol: protocol.input_kind,
                output_protocol: protocol.output_kind,
                supports_rich_blockers: protocol.supports_rich_blockers,
                effect_class: shape.effect_class,
                risk_level: shape.risk_level,
                approval_mode: shape.approval_mode,
                rollback_kind: shape.rollback_kind,
                requires_network: entry.requires_network,
                required_runtime_capabilities: entry
                    .capability_contract
                    .required
                    .iter()
                    .map(|value| (*value).to_string())
                    .collect(),
                allow_when_degraded: entry.capability_contract.allow_when_degraded,
            });
        }
        out
    }

    pub fn assess_tool_request_proposal(
        &self,
        name: &str,
        args: &serde_json::Value,
        policy: &ToolPolicyContext<'_>,
    ) -> ToolBridgeProposalAssessment {
        let Some(entry) = self.tools.get(name) else {
            return ToolBridgeProposalAssessment {
                tool_name: name.to_string(),
                decision: ToolBridgeProposalDecision::UnknownTool,
                summary: format!("tool '{name}' is not registered"),
                effect_class: ToolEffectClass::ReadOnly,
                risk_level: ToolRiskLevel::Low,
                approval_mode: ToolApprovalMode::OperatorOnly,
                rollback_kind: ToolRollbackKind::None,
                requires_network: false,
                required_runtime_capabilities: Vec::new(),
                allow_when_degraded: false,
            };
        };
        let default_shape = entry.tool.catalog_execution_shape();
        if !self.is_llm_tool_visible(name, policy) {
            return ToolBridgeProposalAssessment {
                tool_name: name.to_string(),
                decision: ToolBridgeProposalDecision::Denied,
                summary: format!("tool '{name}' is not visible in the current policy"),
                effect_class: default_shape.effect_class,
                risk_level: default_shape.risk_level,
                approval_mode: default_shape.approval_mode,
                rollback_kind: default_shape.rollback_kind,
                requires_network: entry.requires_network,
                required_runtime_capabilities: entry
                    .capability_contract
                    .required
                    .iter()
                    .map(|value| (*value).to_string())
                    .collect(),
                allow_when_degraded: entry.capability_contract.allow_when_degraded,
            };
        }
        let args_json = match serde_json::to_string(args) {
            Ok(value) => value,
            Err(error) => {
                return ToolBridgeProposalAssessment {
                    tool_name: name.to_string(),
                    decision: ToolBridgeProposalDecision::Denied,
                    summary: format!("tool args serialization failed: {error}"),
                    effect_class: default_shape.effect_class,
                    risk_level: default_shape.risk_level,
                    approval_mode: default_shape.approval_mode,
                    rollback_kind: default_shape.rollback_kind,
                    requires_network: entry.requires_network,
                    required_runtime_capabilities: entry
                        .capability_contract
                        .required
                        .iter()
                        .map(|value| (*value).to_string())
                        .collect(),
                    allow_when_degraded: entry.capability_contract.allow_when_degraded,
                };
            }
        };
        let decision = match self.assess_llm_execution(name, &args_json, policy) {
            Ok(ToolExecutionGateDecision::Allow(permit)) => ToolBridgeProposalAssessment {
                tool_name: name.to_string(),
                decision: ToolBridgeProposalDecision::Allowed,
                summary: "proposal matches current tool governance contract".to_string(),
                effect_class: permit.shape().effect_class,
                risk_level: permit.shape().risk_level,
                approval_mode: permit.shape().approval_mode,
                rollback_kind: permit.shape().rollback_kind,
                requires_network: permit.requires_network(),
                required_runtime_capabilities: entry
                    .capability_contract
                    .required
                    .iter()
                    .map(|value| (*value).to_string())
                    .collect(),
                allow_when_degraded: entry.capability_contract.allow_when_degraded,
            },
            Ok(ToolExecutionGateDecision::Deny { reason }) => ToolBridgeProposalAssessment {
                tool_name: name.to_string(),
                decision: ToolBridgeProposalDecision::Denied,
                summary: reason,
                effect_class: default_shape.effect_class,
                risk_level: default_shape.risk_level,
                approval_mode: default_shape.approval_mode,
                rollback_kind: default_shape.rollback_kind,
                requires_network: entry.requires_network,
                required_runtime_capabilities: entry
                    .capability_contract
                    .required
                    .iter()
                    .map(|value| (*value).to_string())
                    .collect(),
                allow_when_degraded: entry.capability_contract.allow_when_degraded,
            },
            Err(error) => ToolBridgeProposalAssessment {
                tool_name: name.to_string(),
                decision: ToolBridgeProposalDecision::Denied,
                summary: error.to_string(),
                effect_class: default_shape.effect_class,
                risk_level: default_shape.risk_level,
                approval_mode: default_shape.approval_mode,
                rollback_kind: default_shape.rollback_kind,
                requires_network: entry.requires_network,
                required_runtime_capabilities: entry
                    .capability_contract
                    .required
                    .iter()
                    .map(|value| (*value).to_string())
                    .collect(),
                allow_when_degraded: entry.capability_contract.allow_when_degraded,
            },
        };
        decision
    }

    fn llm_visibility_overlay_set(
        &self,
        policy: &ToolPolicyContext<'_>,
    ) -> Option<crate::capability_package::CapabilityPackageToolPolicySet> {
        self.llm_visibility_overlay_provider
            .as_ref()
            .map(|provider| provider(policy.ingress, policy.channel))
    }

    fn is_entry_llm_visible(
        &self,
        _entry: &RegisteredTool,
        tool_name: &str,
        policy: &ToolPolicyContext<'_>,
        overlay_set: Option<&crate::capability_package::CapabilityPackageToolPolicySet>,
    ) -> bool {
        let Some(base_visibility) = self.llm_catalog_authority.get(tool_name) else {
            return false;
        };
        let base_visible = if policy.is_internal_system_channel() {
            base_visibility.internal_system_llm
        } else if policy.ingress == crate::bus::IngressKind::System {
            base_visibility.system_llm
        } else {
            base_visibility.user_llm
        };
        let policy_visible = overlay_set.map_or(base_visible, |overlays| {
            overlays.llm_visibility_for(tool_name, policy, base_visible)
        });
        policy_visible && self.runtime_capability_blocker(tool_name).is_none()
    }

    pub(crate) fn runtime_capability_blocker(
        &self,
        tool_name: &str,
    ) -> Option<crate::orchestrator::RuntimeCapabilityBlocker> {
        let entry = self.tools.get(tool_name)?;
        if entry.capability_contract.is_empty() {
            return None;
        }
        let blocker =
            crate::orchestrator::runtime_capability_blocker(entry.capability_contract.required)?;
        match blocker.capability_status {
            crate::orchestrator::RuntimeCapabilityStatus::Degraded
                if entry.capability_contract.allow_when_degraded =>
            {
                None
            }
            _ => Some(blocker),
        }
    }

    fn observe_runtime_capability_success(&self, tool_name: &str) {
        let Some(entry) = self.tools.get(tool_name) else {
            return;
        };
        if entry.capability_contract.is_empty() {
            return;
        }
        crate::orchestrator::observe_runtime_capability_success(entry.capability_contract.required);
    }

    fn observe_runtime_capability_failure(&self, tool_name: &str, error: &Error) {
        let Some(entry) = self.tools.get(tool_name) else {
            return;
        };
        for capability in entry.capability_contract.required {
            let reason = match (*capability, error) {
                (
                    crate::orchestrator::RUNTIME_CAPABILITY_AUDIO_OUTPUT,
                    Error::Config { message, .. },
                ) if message.contains("speaker unavailable") => {
                    Some(crate::orchestrator::RuntimeCapabilityReason::DeviceDisconnected)
                }
                (
                    crate::orchestrator::RUNTIME_CAPABILITY_AUDIO_INPUT,
                    Error::Config { message, .. },
                ) if message.contains("microphone unavailable") => {
                    Some(crate::orchestrator::RuntimeCapabilityReason::DeviceDisconnected)
                }
                (crate::orchestrator::RUNTIME_CAPABILITY_NETWORK_OUTBOUND_HTTP, err)
                    if err.is_tls_admission()
                        || err.is_connect_error()
                        || err.is_retryable_upstream() =>
                {
                    Some(crate::orchestrator::RuntimeCapabilityReason::UpstreamUnavailable)
                }
                (
                    crate::orchestrator::RUNTIME_CAPABILITY_STORAGE_STATE_FS,
                    Error::Spiffs { .. } | Error::Nvs { .. } | Error::Io { .. },
                ) => Some(crate::orchestrator::RuntimeCapabilityReason::DriverError),
                _ => None,
            };
            if let Some(reason) = reason {
                crate::orchestrator::observe_runtime_capability_failure(capability, reason);
            }
        }
    }
}

fn validate_tool_input_protocol(
    tool_name: &str,
    args: &str,
    contract: crate::tools::ToolProtocolContract,
) -> Result<()> {
    let value: serde_json::Value = serde_json::from_str(args).map_err(|error| {
        protocol_contract_error(
            tool_name,
            format!(
                "declared {} but received invalid json args: {error}",
                contract.input_kind.label()
            ),
        )
    })?;
    let Some(object) = value.as_object() else {
        return Err(protocol_contract_error(
            tool_name,
            format!(
                "declared {} but received non-object args",
                contract.input_kind.label()
            ),
        ));
    };
    if matches!(
        contract.input_kind,
        crate::tools::ToolInputProtocolKind::OperationEnvelope
    ) && object
        .get("op")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .is_none()
    {
        return Err(protocol_contract_error(
            tool_name,
            "declared operation_envelope requires non-empty string field `op`".to_string(),
        ));
    }
    Ok(())
}

fn normalize_and_validate_tool_outcome(
    tool_name: &str,
    contract: crate::tools::ToolProtocolContract,
    outcome: &mut ToolExecutionOutcome,
) -> Result<()> {
    match contract.output_kind {
        crate::tools::ToolOutputProtocolKind::PlainText => {
            outcome.content = truncate_to_byte_len(&outcome.content, MAX_TOOL_RESULT_LEN);
            if !outcome.outbound_intents.is_empty() {
                return Err(protocol_contract_error(
                    tool_name,
                    "declared plain_text but returned outbound intents".to_string(),
                ));
            }
        }
        crate::tools::ToolOutputProtocolKind::StructuredJson => {
            ensure_structured_output_length(tool_name, &outcome.content)?;
            validate_structured_json_content(tool_name, contract, &outcome.content)?;
            if !outcome.outbound_intents.is_empty() {
                return Err(protocol_contract_error(
                    tool_name,
                    "declared structured_json but returned outbound intents".to_string(),
                ));
            }
        }
        crate::tools::ToolOutputProtocolKind::StructuredJsonWithOutbound => {
            ensure_structured_output_length(tool_name, &outcome.content)?;
            validate_structured_json_content(tool_name, contract, &outcome.content)?;
            if outcome.outbound_intents.is_empty() {
                return Err(protocol_contract_error(
                    tool_name,
                    "declared structured_json_with_outbound but returned no outbound intents"
                        .to_string(),
                ));
            }
        }
    }
    if outcome.blocker.is_some() && !contract.supports_rich_blockers {
        return Err(protocol_contract_error(
            tool_name,
            "returned blocker semantics without rich blocker protocol support".to_string(),
        ));
    }
    Ok(())
}

fn ensure_structured_output_length(tool_name: &str, content: &str) -> Result<()> {
    if content.len() > MAX_TOOL_RESULT_LEN {
        return Err(protocol_contract_error(
            tool_name,
            format!(
                "declared structured json output exceeds max length {}",
                MAX_TOOL_RESULT_LEN
            ),
        ));
    }
    Ok(())
}

fn validate_structured_json_content(
    tool_name: &str,
    contract: crate::tools::ToolProtocolContract,
    content: &str,
) -> Result<()> {
    serde_json::from_str::<serde_json::Value>(content).map_err(|error| {
        protocol_contract_error(
            tool_name,
            format!(
                "declared {} but returned non-json content: {error}",
                contract.output_kind.label()
            ),
        )
    })?;
    Ok(())
}

fn protocol_contract_error(tool_name: &str, message: String) -> Error {
    crate::metrics::record_tool_protocol_violation();
    Error::config(
        "tool_protocol_contract",
        format!("tool '{tool_name}' {message}"),
    )
}

fn runtime_capability_error(
    tool_name: &str,
    blocker: &crate::orchestrator::RuntimeCapabilityBlocker,
) -> Error {
    Error::config(
        "tool_execute_capability",
        format!(
            "tool '{tool_name}' requires sub-capability '{}' but it is {:?} ({:?})",
            blocker.sub_capability, blocker.capability_status, blocker.capability_reason
        )
        .to_ascii_lowercase(),
    )
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

fn enforce_direct_execution_governance(
    metadata: ToolMetadata,
    shape: &crate::tools::ToolExecutionShape,
) -> Result<()> {
    if matches!(
        metadata.exposure,
        crate::tools::ToolExposure::Admin | crate::tools::ToolExposure::Debug
    ) || matches!(shape.approval_mode, ToolApprovalMode::OperatorOnly)
    {
        return Err(Error::config("tool_execute", "operator_only_tool"));
    }
    if matches!(shape.approval_mode, ToolApprovalMode::ExplicitIntent) && !shape.approval_granted {
        return Err(Error::config("tool_execute", "explicit_intent_required"));
    }
    Ok(())
}

/// 构建包含所有内置工具的注册表。`platform` 用于 `board_info` 等依赖平台能力的工具。
/// Returns `(registry, Option<baidu_token_cache>)` — the cache is shared with voice_session.
#[cold]
#[inline(never)]
fn register_core_tools(
    registry: &mut ToolRegistry,
    config: &AppConfig,
    services: &crate::RuntimeServices,
    tool_execution_governance: &Arc<ToolExecutionGovernance>,
) {
    let platform = &services.platform;
    registry.register(Box::new(super::GetTimeTool));
    registry.register(Box::new(super::EnvTool));
    registry.register(Box::new(super::MessageTool));
    registry.register(Box::new(super::TaskTool::new(
        Arc::clone(&services.task_store),
        Arc::clone(&services.calendar_store),
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
    registry.register(Box::new(super::RemindAtTool::with_local_calendar(
        Arc::clone(&services.remind_at_store),
        Arc::clone(&services.calendar_store),
    )));
    registry.register(Box::new(super::RemindListTool::new(Arc::clone(
        &services.remind_at_store,
    ))));
    registry.register(Box::new(super::BoardInfoTool::new(Arc::clone(platform))));
    registry.register(Box::new(super::DiagnoseDeliveryTool::new(
        config.enabled_channel.clone(),
    )));
    registry.register(Box::new(super::DiagnoseSystemTool::new(
        Arc::clone(platform),
        config.enabled_channel.clone(),
    )));
    registry.register(Box::new(super::DiagnoseNetworkPathTool::new(
        Arc::clone(platform),
        Arc::clone(&services.config_store),
    )));
    registry.register(Box::new(super::KvStoreTool::new(platform.state_fs())));
    registry.register(Box::new(super::PrivateGardenTool::new(Arc::clone(
        &services.private_garden_store,
    ))));
    registry.register(Box::new(super::FactualMemoryTool::new(Arc::clone(
        &services.long_term_memory_store,
    ))));
    registry.register(Box::new(super::MemorySearchTool::new(
        Arc::clone(&services.session_store),
        Arc::clone(&services.memory_store),
        Arc::clone(&services.turn_ledger_store),
    )));
    registry.register(Box::new(super::MemoryGetTool::new(
        Arc::clone(&services.session_store),
        Arc::clone(&services.memory_store),
        Arc::clone(&services.turn_ledger_store),
    )));
    registry.register(Box::new(super::ContinuitySnapshotTool::new(
        platform.state_fs(),
        Arc::clone(&services.session_store),
        Arc::clone(&services.memory_store),
        Arc::clone(&services.long_term_memory_store),
        Arc::clone(&services.continuity_capsule_store),
        Arc::clone(&services.session_summary_store),
        Arc::clone(&services.execution_state_store),
        Arc::clone(&services.active_work_store),
        Arc::clone(&services.self_model_store),
        Arc::clone(&services.self_authored_core_store),
        Arc::clone(&services.core_revision_ledger_store),
        Arc::clone(&services.self_continuity_store),
        Arc::clone(&services.turn_ledger_store),
        Arc::clone(&services.relationship_constitution_store),
        Arc::clone(&services.relationship_portfolio_store),
        Arc::clone(&services.relationship_topology_store),
        Arc::clone(&services.task_run_store),
        Arc::clone(&services.task_artifact_store),
        Arc::clone(&services.task_execution_ledger_store),
        Arc::clone(&services.task_learning_store),
        Arc::clone(&services.skill_storage),
        Arc::clone(tool_execution_governance),
    )));
    let continuity_snapshot_supported = registry.get("continuity_snapshot").is_some();
    registry.register(Box::new(super::DiagnoseMemoryRuntimeTool::new(
        Arc::clone(platform),
        continuity_snapshot_supported,
    )));
    registry.register(Box::new(super::DiagnoseVoicePathTool::new(
        Arc::clone(platform),
        Arc::clone(&services.config_store),
    )));
    #[cfg(feature = "tools_diagnostics")]
    if !config.hardware_devices.is_empty() {
        registry.register(Box::new(super::DeviceControlTool::new(
            config.hardware_devices.clone(),
            Arc::clone(platform),
        )));
    }
}

#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
#[cold]
#[inline(never)]
fn register_office_tools(
    registry: &mut ToolRegistry,
    _config: &AppConfig,
    services: &crate::RuntimeServices,
) {
    let platform = &services.platform;
    let topology = crate::office::build_default_office_integration_topology();
    let contacts_directory_store: Arc<
        dyn crate::contacts_directory::ContactsDirectoryStore + Send + Sync,
    > = Arc::new(
        crate::contacts_directory::StateFsContactsDirectoryStore::new(platform.state_fs()),
    );
    let office_config_service = crate::office::OfficeConfigManagementService::new(
        Arc::new(crate::config::PlatformConfigFileStore(Arc::clone(platform))),
        Arc::clone(&services.office_credential_store),
        Arc::clone(&services.office_runtime_status_store),
    )
    .with_probe_adapters(topology.probe_adapters());
    let office_authority = Arc::new(crate::office::ReloadingOfficeAuthoritySource::new(
        Arc::new(crate::config::PlatformConfigFileStore(Arc::clone(platform))),
        Arc::clone(&services.office_credential_store),
        Arc::clone(&services.office_runtime_status_store),
    ));
    let calendar_credential_store: Arc<
        dyn crate::calendar::CalendarProviderCredentialStore + Send + Sync,
    > = Arc::new(
        crate::calendar::OfficeBackedCalendarProviderCredentialStore::with_authority(
            office_authority.clone(),
        ),
    );
    let mail_credential_store: Arc<dyn crate::mail::MailProviderCredentialStore + Send + Sync> =
        Arc::new(
            crate::mail::OfficeBackedMailProviderCredentialStore::with_authority(
                office_authority.clone(),
            ),
        );
    let documents_credential_store: Arc<
        dyn crate::documents::DocumentsProviderCredentialStore + Send + Sync,
    > = Arc::new(
        crate::documents::OfficeBackedDocumentsProviderCredentialStore::with_authority(
            office_authority.clone(),
        ),
    );
    let contacts_directory_credential_store: Arc<
        dyn crate::contacts_directory::ContactsDirectoryProviderCredentialStore + Send + Sync,
    > = Arc::new(
        crate::contacts_directory::OfficeBackedContactsDirectoryProviderCredentialStore::with_authority(
            office_authority.clone(),
        ),
    );
    let mail_providers = topology.mail_providers();
    let documents_providers = topology.documents_providers();
    let calendar_providers = topology.calendar_providers();
    let task_calendar_providers = topology.calendar_providers();
    let reminder_calendar_providers = topology.calendar_providers();
    let contacts_service =
        crate::contacts_directory::ContactsDirectoryService::with_office_authority(
            Arc::clone(&contacts_directory_store),
            Arc::clone(&contacts_directory_credential_store),
            topology.contacts_directory_providers(),
            office_authority.clone(),
        );
    let probe_supported_provider_kinds = topology.probe_supported_provider_kinds();

    registry.register(Box::new(
        super::CalendarTool::with_office_authority_and_contacts_service(
            Arc::clone(&services.calendar_store),
            Arc::clone(&calendar_credential_store),
            calendar_providers,
            office_authority.clone(),
            contacts_service.clone(),
        ),
    ));
    registry.register(Box::new(super::TaskTool::with_office_authority(
        Arc::clone(&services.task_store),
        Arc::clone(&services.calendar_store),
        task_calendar_providers,
        office_authority.clone(),
    )));
    registry.register(Box::new(super::RemindAtTool::with_office_authority(
        Arc::clone(&services.remind_at_store),
        Arc::clone(&services.calendar_store),
        reminder_calendar_providers,
        office_authority.clone(),
    )));
    registry.register(Box::new(
        super::MailTool::with_office_authority_and_contacts_service(
            Arc::clone(&mail_credential_store),
            mail_providers,
            office_authority.clone(),
            contacts_service.clone(),
        ),
    ));
    registry.register(Box::new(
        super::ContactsDirectoryTool::with_office_authority(
            Arc::clone(&contacts_directory_store),
            Arc::clone(&contacts_directory_credential_store),
            topology.contacts_directory_providers(),
            office_authority.clone(),
        ),
    ));
    registry.register(Box::new(
        super::DocumentsTool::with_office_authority_and_contacts_service(
            Arc::clone(&documents_credential_store),
            documents_providers,
            office_authority.clone(),
            contacts_service,
        ),
    ));
    registry.register(Box::new(super::OfficeConfigTool::new(
        office_config_service,
    )));
    registry.register(Box::new(
        super::OfficeStatusTool::with_probe_supported_provider_kinds(
            office_authority,
            probe_supported_provider_kinds,
        ),
    ));
}

#[cold]
#[inline(never)]
fn register_extended_runtime_tools(
    registry: &mut ToolRegistry,
    config: &AppConfig,
    device_capability_registry: &crate::DeviceCapabilityRegistry,
    services: &crate::RuntimeServices,
    tool_execution_governance: &Arc<ToolExecutionGovernance>,
) {
    let platform = &services.platform;
    #[cfg(feature = "tools_diagnostics")]
    registry.register(Box::new(super::MemoryManageTool::new(
        Arc::clone(&services.memory_store),
        Arc::clone(&services.long_term_memory_store),
        Arc::clone(&services.skill_storage),
    )));
    #[cfg(all(
        feature = "tools_network_extra",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    ))]
    registry.register(Box::new(super::HttpRequestTool));
    #[cfg(feature = "tools_diagnostics")]
    registry.register(Box::new(super::SessionManageTool::new(Arc::clone(
        &services.session_store,
    ))));
    registry.register(Box::new(super::FileWriteTool::new(platform.state_fs())));
    #[cfg(feature = "tools_diagnostics")]
    registry.register(Box::new(super::SystemControlTool::new(
        Arc::clone(platform),
        Arc::clone(tool_execution_governance),
    )));
    #[cfg(feature = "tools_diagnostics")]
    registry.register(Box::new(super::CronManageTool::new(Arc::clone(
        &services.memory_store,
    ))));
    #[cfg(feature = "tools_network_extra")]
    registry.register(Box::new(super::ProxyConfigTool::new(Arc::clone(
        &services.config_store,
    ))));
    #[cfg(feature = "tools_network_extra")]
    registry.register(Box::new(super::ModelConfigTool::new(Arc::clone(platform))));
    #[cfg(feature = "tools_diagnostics")]
    registry.register(Box::new(super::NetworkScanTool::new(Arc::clone(platform))));
    #[cfg(feature = "tools_diagnostics")]
    if device_capability_registry.is_mounted(crate::DEVICE_CAPABILITY_SENSOR) {
        registry.register(Box::new(super::SensorWatchTool::new(
            Arc::clone(&services.memory_store),
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
    device_capability_registry: &crate::DeviceCapabilityRegistry,
    services: &crate::RuntimeServices,
) -> Option<Arc<crate::audio::baidu_token::BaiduTokenCache>> {
    let platform = &services.platform;
    if !device_capability_registry.is_mounted(crate::DEVICE_CAPABILITY_VOICE) {
        return None;
    }
    let audio_cfg = config.audio.clone()?;
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
fn register_host_only_tools(
    registry: &mut ToolRegistry,
    #[cfg(target_os = "linux")] skill_storage: &Arc<
        dyn crate::platform::SkillStorage + Send + Sync,
    >,
    #[cfg(target_os = "linux")] long_term_memory_store: &Arc<
        dyn crate::memory::LongTermMemoryStore + Send + Sync,
    >,
    #[cfg(target_os = "linux")] continuity_capsule_store: &Arc<
        dyn crate::memory::ContinuityCapsuleStore + Send + Sync,
    >,
) {
    registry.register(Box::new(super::ShellTool));
    registry.register(Box::new(super::ProcessTool));
    registry.register(Box::new(super::NetworkTool));
    #[cfg(target_os = "linux")]
    registry.register(Box::new(super::LuaQueryTool::default()));
    #[cfg(target_os = "linux")]
    registry.register(Box::new(super::LuaDatasheetDistillTool::default()));
    #[cfg(target_os = "linux")]
    registry.register(Box::new(super::LuaProtocolFrameHelperTool::default()));
    #[cfg(target_os = "linux")]
    registry.register(Box::new(super::LuaRegisterTableHelperTool::default()));
    #[cfg(target_os = "linux")]
    registry.register(Box::new(super::LuaStateMachineCheckerTool::default()));
    #[cfg(target_os = "linux")]
    registry.register(Box::new(super::LuaMemoryQueryTool::new(
        Arc::new(crate::reasoning::CurrentExecutableLuaSandboxExecutor),
        Arc::clone(long_term_memory_store),
        Arc::clone(continuity_capsule_store),
    )));
    #[cfg(target_os = "linux")]
    registry.register(Box::new(super::LuaToolBridgeTool::default()));
    #[cfg(target_os = "linux")]
    registry.register(Box::new(super::CapabilityAtomsExchangeTool::new(
        Arc::clone(skill_storage),
    )));
    #[cfg(target_os = "linux")]
    registry.register(Box::new(super::CapabilityAtomsInspectTool::new(
        Arc::clone(skill_storage),
    )));
}

pub fn build_default_registry(
    config: &AppConfig,
    services: &crate::RuntimeServices,
) -> (
    ToolRegistry,
    Option<Arc<crate::audio::baidu_token::BaiduTokenCache>>,
) {
    let device_capability_registry =
        crate::build_device_capability_registry(config, services.platform.as_ref());
    let tool_execution_governance =
        Arc::new(ToolExecutionGovernance::new(services.platform.state_fs()));
    let mut registry = ToolRegistry::new()
        .with_execution_governance(Arc::clone(&tool_execution_governance))
        .with_llm_catalog_authority(Arc::new(crate::tools::build_default_llm_catalog_authority()))
        .with_tool_protocol_authority(Arc::new(
            crate::tools::build_default_tool_protocol_authority(),
        ));
    register_core_tools(&mut registry, config, services, &tool_execution_governance);
    #[cfg(all(
        feature = "capability_office",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    ))]
    register_office_tools(&mut registry, config, services);
    register_extended_runtime_tools(
        &mut registry,
        config,
        &device_capability_registry,
        services,
        &tool_execution_governance,
    );
    let shared_baidu_token =
        register_audio_tools(&mut registry, config, &device_capability_registry, services);
    #[cfg(all(
        not(any(target_arch = "xtensa", target_arch = "riscv32")),
        target_os = "linux"
    ))]
    register_host_only_tools(
        &mut registry,
        &services.skill_storage,
        &services.long_term_memory_store,
        &services.continuity_capsule_store,
    );
    #[cfg(all(
        not(any(target_arch = "xtensa", target_arch = "riscv32")),
        not(target_os = "linux")
    ))]
    register_host_only_tools(&mut registry);
    (registry, shared_baidu_token)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::{PrivateGardenDoc, PrivateGardenDocRecord, PrivateGardenStore};
    use crate::tools::{
        ToolCatalogAuthority, ToolExecutionShape, ToolInputProtocolKind, ToolLlmVisibility,
        ToolMetadata, ToolOutputProtocolKind, ToolProtocolAuthority, ToolProtocolContract,
    };
    use std::sync::Mutex;

    static RUNTIME_CAPABILITY_TEST_GUARD: Mutex<()> = Mutex::new(());

    struct VisibleTool;
    struct StatefulTool;
    struct AdminTool;
    struct InternalOnlyTool;
    struct UserOnlyTaskTool;
    struct OutcomeTool;
    struct OperationEnvelopeTool;
    struct InvalidStructuredJsonTool;
    struct MissingOutboundIntentTool;
    struct RogueBlockerTool;
    struct RichBlockerTool;
    struct CapabilityBoundTool;
    struct ConditionalNetworkTool;
    struct StubToolContext;
    #[derive(Default)]
    struct StubPrivateGardenStore;

    fn with_catalog_runtime_capabilities_online<T>(f: impl FnOnce() -> T) -> T {
        let _guard = RUNTIME_CAPABILITY_TEST_GUARD
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        crate::orchestrator::reset_runtime_capabilities_for_tests();
        for capability in [
            crate::orchestrator::RUNTIME_CAPABILITY_AUDIO_OUTPUT,
            crate::orchestrator::RUNTIME_CAPABILITY_AUDIO_INPUT,
            crate::orchestrator::RUNTIME_CAPABILITY_NETWORK_OUTBOUND_HTTP,
            crate::orchestrator::RUNTIME_CAPABILITY_STORAGE_STATE_FS,
        ] {
            crate::orchestrator::update_runtime_capability(
                crate::orchestrator::RuntimeCapabilityUpdate {
                    id: capability,
                    status: crate::orchestrator::RuntimeCapabilityStatus::Online,
                    reason: crate::orchestrator::RuntimeCapabilityReason::Nominal,
                    observed_at_secs: 1,
                    recovery_hint: None,
                },
            );
        }
        let outcome = f();
        crate::orchestrator::reset_runtime_capabilities_for_tests();
        outcome
    }

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
            ToolMetadata::task()
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
            ToolMetadata::task()
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
            Ok(
                ToolExecutionOutcome::text(r#"{"ok":true,"summary":"outcome body"}"#)
                    .with_current_chat_reply("tool delivered reply"),
            )
        }
    }

    impl Tool for OperationEnvelopeTool {
        fn name(&self) -> &'static str {
            "operation_envelope"
        }

        fn description(&self) -> &str {
            "operation envelope tool"
        }

        fn schema(&self) -> &str {
            r#"{"type":"object","properties":{"op":{"type":"string"}},"required":["op"]}"#
        }

        fn execute(&self, _args: &str, _ctx: &mut dyn crate::tools::ToolContext) -> Result<String> {
            Ok(r#"{"ok":true}"#.to_string())
        }
    }

    impl Tool for InvalidStructuredJsonTool {
        fn name(&self) -> &'static str {
            "invalid_structured_json"
        }

        fn description(&self) -> &str {
            "declares structured json but returns invalid json"
        }

        fn schema(&self) -> &str {
            r#"{"type":"object"}"#
        }

        fn execute(&self, _args: &str, _ctx: &mut dyn crate::tools::ToolContext) -> Result<String> {
            Ok(String::new())
        }

        fn execute_outcome(
            &self,
            _args: &str,
            _ctx: &mut dyn crate::tools::ToolContext,
        ) -> Result<ToolExecutionOutcome> {
            Ok(ToolExecutionOutcome::text("not valid json"))
        }
    }

    impl Tool for MissingOutboundIntentTool {
        fn name(&self) -> &'static str {
            "missing_outbound_intent"
        }

        fn description(&self) -> &str {
            "declares outbound protocol but omits outbound intents"
        }

        fn schema(&self) -> &str {
            r#"{"type":"object"}"#
        }

        fn execute(&self, _args: &str, _ctx: &mut dyn crate::tools::ToolContext) -> Result<String> {
            Ok(String::new())
        }

        fn execute_outcome(
            &self,
            _args: &str,
            _ctx: &mut dyn crate::tools::ToolContext,
        ) -> Result<ToolExecutionOutcome> {
            Ok(ToolExecutionOutcome::text(r#"{"ok":true}"#))
        }
    }

    impl Tool for RogueBlockerTool {
        fn name(&self) -> &'static str {
            "rogue_blocker"
        }

        fn description(&self) -> &str {
            "returns a blocker without rich blocker protocol support"
        }

        fn schema(&self) -> &str {
            r#"{"type":"object"}"#
        }

        fn execute(&self, _args: &str, _ctx: &mut dyn crate::tools::ToolContext) -> Result<String> {
            Ok(String::new())
        }

        fn execute_outcome(
            &self,
            _args: &str,
            _ctx: &mut dyn crate::tools::ToolContext,
        ) -> Result<ToolExecutionOutcome> {
            Ok(ToolExecutionOutcome::text(r#"{"ok":false}"#).with_blocker(
                crate::tools::ToolExecutionBlocker::needs_user_facts(
                    "missing detail",
                    vec!["field".to_string()],
                    Vec::new(),
                ),
            ))
        }
    }

    impl Tool for RichBlockerTool {
        fn name(&self) -> &'static str {
            "rich_blocker"
        }

        fn description(&self) -> &str {
            "returns a blocker with rich blocker protocol support"
        }

        fn schema(&self) -> &str {
            r#"{"type":"object","properties":{"op":{"type":"string"}},"required":["op"]}"#
        }

        fn execute(&self, _args: &str, _ctx: &mut dyn crate::tools::ToolContext) -> Result<String> {
            Ok(String::new())
        }

        fn execute_outcome(
            &self,
            _args: &str,
            _ctx: &mut dyn crate::tools::ToolContext,
        ) -> Result<ToolExecutionOutcome> {
            Ok(ToolExecutionOutcome::text(r#"{"ok":false}"#).with_blocker(
                crate::tools::ToolExecutionBlocker::needs_user_facts(
                    "missing detail",
                    vec!["field".to_string()],
                    Vec::new(),
                ),
            ))
        }
    }

    impl Tool for CapabilityBoundTool {
        fn name(&self) -> &'static str {
            "capability_bound"
        }
        fn description(&self) -> &str {
            "tool guarded by runtime capability"
        }
        fn schema(&self) -> &str {
            r#"{"type":"object"}"#
        }
        fn execute(&self, _args: &str, _ctx: &mut dyn crate::tools::ToolContext) -> Result<String> {
            Ok("ok".to_string())
        }
        fn capability_contract(&self) -> crate::tools::ToolCapabilityContract {
            crate::tools::ToolCapabilityContract::required(&["audio_output"])
        }
    }

    impl Tool for ConditionalNetworkTool {
        fn name(&self) -> &'static str {
            "conditional_network"
        }

        fn description(&self) -> &str {
            "tool with per-call network requirements"
        }

        fn schema(&self) -> &str {
            r#"{"type":"object","properties":{"op":{"type":"string"}}}"#
        }

        fn execute(&self, _args: &str, _ctx: &mut dyn crate::tools::ToolContext) -> Result<String> {
            Ok(String::new())
        }

        fn requires_network(&self) -> bool {
            true
        }

        fn requires_network_for(&self, args: &str) -> Result<bool> {
            let obj = crate::tools::parse_tool_args(args, "conditional_network_tool")?;
            Ok(matches!(
                obj.get("op").and_then(|value| value.as_str()),
                Some("remote")
            ))
        }
    }

    impl PrivateGardenStore for StubPrivateGardenStore {
        fn list(&self, _chat_id: &str, _limit: usize) -> Result<Vec<PrivateGardenDocRecord>> {
            Ok(Vec::new())
        }

        fn read(&self, _chat_id: &str, _doc_path: &str) -> Result<Option<PrivateGardenDoc>> {
            Ok(None)
        }

        fn write(
            &self,
            _chat_id: &str,
            doc_path: &str,
            content: &str,
            now_secs: u64,
        ) -> Result<PrivateGardenDocRecord> {
            Ok(PrivateGardenDocRecord {
                path: doc_path.to_string(),
                updated_at: now_secs,
                revision: 1,
                bytes: content.len(),
                preview: content.to_string(),
            })
        }

        fn delete(&self, _chat_id: &str, _doc_path: &str) -> Result<bool> {
            Ok(false)
        }

        fn move_doc(
            &self,
            _chat_id: &str,
            _from_path: &str,
            _to_path: &str,
            _now_secs: u64,
        ) -> Result<Option<PrivateGardenDocRecord>> {
            Ok(None)
        }
    }

    struct ExplicitIntentTool;

    struct SemanticFailureTool;
    struct DynamicGovernanceTool;

    impl Tool for ExplicitIntentTool {
        fn name(&self) -> &'static str {
            "explicit_tool"
        }

        fn description(&self) -> &str {
            "needs explicit intent"
        }

        fn schema(&self) -> &str {
            r#"{"type":"object"}"#
        }

        fn execute(&self, _args: &str, _ctx: &mut dyn crate::tools::ToolContext) -> Result<String> {
            Ok(String::new())
        }

        fn metadata(&self) -> ToolMetadata {
            ToolMetadata::task().with_approval_mode(ToolApprovalMode::ExplicitIntent)
        }
    }

    impl Tool for SemanticFailureTool {
        fn name(&self) -> &'static str {
            "semantic_failure"
        }

        fn description(&self) -> &str {
            "returns a structured semantic failure"
        }

        fn schema(&self) -> &str {
            r#"{"type":"object"}"#
        }

        fn execute(&self, _args: &str, _ctx: &mut dyn crate::tools::ToolContext) -> Result<String> {
            Ok(String::new())
        }

        fn execute_outcome(
            &self,
            _args: &str,
            _ctx: &mut dyn crate::tools::ToolContext,
        ) -> Result<ToolExecutionOutcome> {
            Ok(ToolExecutionOutcome::text(r#"{"ok":false}"#)
                .with_failure_kind(crate::tools::ToolExecutionFailureKind::Capability))
        }
    }

    impl Tool for DynamicGovernanceTool {
        fn name(&self) -> &'static str {
            "dynamic_governance"
        }

        fn description(&self) -> &str {
            "dynamic governance sample tool"
        }

        fn schema(&self) -> &str {
            r#"{"type":"object"}"#
        }

        fn execute(&self, _args: &str, _ctx: &mut dyn crate::tools::ToolContext) -> Result<String> {
            Ok(String::new())
        }

        fn metadata(&self) -> ToolMetadata {
            ToolMetadata::task()
        }

        fn execution_shape(&self, args: &str) -> Result<ToolExecutionShape> {
            let obj = crate::tools::parse_tool_args(args, "dynamic_governance_tool")?;
            let op = obj
                .get("op")
                .and_then(|value| value.as_str())
                .unwrap_or("inspect");
            Ok(match op {
                "write" => self
                    .metadata()
                    .default_execution_shape("write")
                    .with_effect_class(ToolEffectClass::PersistentStateWrite)
                    .with_risk_level(ToolRiskLevel::Medium)
                    .with_approval_mode(ToolApprovalMode::ExplicitIntent)
                    .with_approval_granted(false)
                    .with_rollback_kind(ToolRollbackKind::CompensatingWrite),
                _ => self.metadata().default_execution_shape("inspect"),
            })
        }

        fn governance_examples(&self) -> &'static [&'static str] {
            &[r#"{"op":"inspect"}"#, r#"{"op":"write"}"#]
        }
    }

    #[derive(Default)]
    struct MemoryStateFs {
        files: std::sync::Mutex<std::collections::BTreeMap<String, Vec<u8>>>,
    }

    #[derive(Default)]
    struct StubSkillStorage {
        files: std::sync::Mutex<std::collections::BTreeMap<String, Vec<u8>>>,
    }

    impl crate::platform::StateFs for MemoryStateFs {
        fn read(&self, rel_path: &str) -> Result<Option<Vec<u8>>> {
            Ok(self
                .files
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .get(rel_path)
                .cloned())
        }

        fn write(&self, rel_path: &str, data: &[u8]) -> Result<()> {
            self.files
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .insert(rel_path.to_string(), data.to_vec());
            Ok(())
        }

        fn remove(&self, rel_path: &str) -> Result<()> {
            self.files
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .remove(rel_path);
            Ok(())
        }

        fn list_dir(&self, rel_path: &str) -> Result<Vec<String>> {
            let prefix = if rel_path.is_empty() {
                String::new()
            } else {
                format!("{}/", rel_path.trim_end_matches('/'))
            };
            let files = self.files.lock().unwrap_or_else(|error| error.into_inner());
            let mut names = std::collections::BTreeSet::new();
            for key in files.keys() {
                if !key.starts_with(&prefix) {
                    continue;
                }
                let tail = &key[prefix.len()..];
                if tail.is_empty() {
                    continue;
                }
                if let Some((dir, _)) = tail.split_once('/') {
                    names.insert(format!("{dir}/"));
                } else {
                    names.insert(tail.to_string());
                }
            }
            Ok(names.into_iter().collect())
        }
    }

    impl crate::platform::SkillStorage for StubSkillStorage {
        fn list_names(&self) -> Result<Vec<String>> {
            Ok(self
                .files
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .keys()
                .cloned()
                .collect())
        }

        fn read(&self, name: &str) -> Result<Vec<u8>> {
            self.files
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .get(name)
                .cloned()
                .ok_or_else(|| Error::config("registry_test_skill_storage_read", "missing"))
        }

        fn write(&self, name: &str, content: &[u8]) -> Result<()> {
            self.files
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .insert(name.to_string(), content.to_vec());
            Ok(())
        }

        fn remove(&self, name: &str) -> Result<()> {
            self.files
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .remove(name);
            Ok(())
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

    fn user_visible_tool_names_from_default_registry() -> std::collections::BTreeSet<String> {
        with_catalog_runtime_capabilities_online(|| {
            crate::platform::http_server::handlers::build_default_test_handler_context()
                .tool_registry
                .tool_catalog()
                .expect("tool catalog")
                .into_iter()
                .filter(|entry| entry.llm_visible_user)
                .map(|entry| entry.name)
                .collect()
        })
    }

    fn user_visible_tool_descriptions_from_default_registry(
    ) -> std::collections::BTreeMap<String, String> {
        with_catalog_runtime_capabilities_online(|| {
            let ctx = crate::platform::http_server::handlers::build_default_test_handler_context();
            let policy = ToolPolicyContext::new(crate::bus::IngressKind::User, "telegram");
            ctx.tool_registry
                .tool_specs_for_llm_with_max(&policy, 256 * 1024)
                .into_iter()
                .map(|spec| (spec.name.to_string(), spec.description.to_string()))
                .collect()
        })
    }

    fn synthetic_catalog(entries: &[(&str, ToolLlmVisibility)]) -> Arc<ToolCatalogAuthority> {
        let mut authority = ToolCatalogAuthority::default();
        for (name, visibility) in entries {
            authority.insert(name, *visibility);
        }
        Arc::new(authority)
    }

    fn synthetic_protocol_authority(
        entries: &[(&str, ToolProtocolContract)],
    ) -> Arc<ToolProtocolAuthority> {
        let mut authority = ToolProtocolAuthority::default();
        for (name, contract) in entries {
            authority.insert(name, *contract);
        }
        Arc::new(authority)
    }

    #[test]
    fn llm_tool_specs_follow_runtime_policy() {
        let mut registry = ToolRegistry::new().with_llm_catalog_authority(synthetic_catalog(&[
            ("visible", ToolLlmVisibility::user_and_system()),
            ("stateful", ToolLlmVisibility::user_only()),
            ("internal_only", ToolLlmVisibility::internal_only()),
            ("user_only_task", ToolLlmVisibility::user_only()),
        ]));
        registry.register(Box::new(VisibleTool));
        registry.register(Box::new(StatefulTool));
        registry.register(Box::new(AdminTool));
        registry.register(Box::new(InternalOnlyTool));
        registry.register(Box::new(UserOnlyTaskTool));

        let user = ToolPolicyContext::new(crate::bus::IngressKind::User, "telegram");
        let user_specs = registry.tool_specs_for_llm_with_max(&user, 4096);
        let user_names: Vec<&str> = user_specs.iter().map(|spec| spec.name.as_str()).collect();
        assert_eq!(user_names, vec!["visible", "stateful", "user_only_task"]);

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
    fn llm_tool_visibility_requires_explicit_catalog_membership() {
        let mut registry = ToolRegistry::new();
        registry.register(Box::new(VisibleTool));

        let user = ToolPolicyContext::new(crate::bus::IngressKind::User, "telegram");
        let system = ToolPolicyContext::new(crate::bus::IngressKind::System, "telegram");
        let internal = ToolPolicyContext::new(crate::bus::IngressKind::System, "cron");

        assert!(
            registry.tool_specs_for_llm_with_max(&user, 4096).is_empty(),
            "tools without explicit catalog membership must stay hidden from user ingress"
        );
        assert!(
            registry
                .tool_specs_for_llm_with_max(&system, 4096)
                .is_empty(),
            "tools without explicit catalog membership must stay hidden from system ingress"
        );
        assert!(
            registry.tool_specs_for_llm_with_max(&internal, 4096).is_empty(),
            "tools without explicit catalog membership must stay hidden from internal system ingress"
        );
    }

    #[test]
    fn default_registry_declares_catalog_authority_for_every_registered_tool() {
        let ctx = crate::platform::http_server::handlers::build_default_test_handler_context();
        let missing = ctx.tool_registry.missing_llm_catalog_entries();
        assert!(
            missing.is_empty(),
            "default registry tools missing explicit catalog authority: {missing:?}"
        );
    }

    #[test]
    fn default_registry_declares_protocol_authority_for_every_registered_tool() {
        let ctx = crate::platform::http_server::handlers::build_default_test_handler_context();
        let missing = ctx.tool_registry.missing_tool_protocol_entries();
        assert!(
            missing.is_empty(),
            "default registry tools missing explicit protocol authority: {missing:?}"
        );
    }

    #[test]
    fn default_registry_user_ingress_catalog_curates_domain_surface() {
        let user_visible = user_visible_tool_names_from_default_registry();

        for required in [
            "get_time",
            "board_info",
            "diagnose_delivery",
            "diagnose_system",
            "diagnose_network_path",
            "diagnose_voice_path",
            "task",
            "remind_at",
            "remind_list",
            "memory_search",
            "memory_get",
            "factual_memory",
        ] {
            assert!(
                user_visible.contains(required),
                "expected user ingress catalog to keep {required}"
            );
        }

        #[cfg(all(
            feature = "capability_office",
            not(any(target_arch = "xtensa", target_arch = "riscv32"))
        ))]
        for required in [
            "office_config",
            "office_status",
            "mail",
            "calendar",
            "documents",
            "contacts_directory",
        ] {
            assert!(
                user_visible.contains(required),
                "expected user ingress catalog to keep {required}"
            );
        }

        #[cfg(feature = "tools_network_extra")]
        assert!(
            user_visible.contains("web_search"),
            "expected user ingress catalog to keep web_search"
        );

        #[cfg(all(
            feature = "tools_network_extra",
            not(any(target_arch = "xtensa", target_arch = "riscv32"))
        ))]
        for required in ["document_read", "document_search", "analyze_image"] {
            assert!(
                user_visible.contains(required),
                "expected user ingress catalog to keep {required}"
            );
        }

        for hidden in [
            "env",
            "continuity_snapshot",
            "private_garden",
            "files",
            "file_edit",
            "file_write",
            "kv_store",
            "cron_manage",
            "http_request",
            "web_fetch",
            "pdf_read",
            "network",
            "process",
            "session_manage",
            "memory_manage",
            "system_control",
            "shell",
            "proxy_config",
            "model_config",
            "capability_atoms_exchange",
            "capability_atoms_inspect",
            "lua_query",
            "lua_memory_query",
            "lua_tool_bridge",
            "lua_datasheet_distill",
            "lua_register_table_helper",
            "lua_protocol_frame_helper",
            "lua_state_machine_checker",
        ] {
            assert!(
                !user_visible.contains(hidden),
                "expected user ingress catalog to hide {hidden}"
            );
        }
    }

    #[test]
    fn default_registry_user_visible_descriptions_do_not_route_toward_hidden_operator_tools() {
        let descriptions = user_visible_tool_descriptions_from_default_registry();

        #[cfg(all(
            feature = "capability_office",
            not(any(target_arch = "xtensa", target_arch = "riscv32"))
        ))]
        {
            let office_config = descriptions
                .get("office_config")
                .expect("office_config description");
            assert!(
                office_config.contains("Configure")
                    || office_config.contains("configure")
                    || office_config.contains("reconfigure"),
                "expected office_config description to explain onboarding/reconfigure ownership"
            );
            assert!(
                office_config.contains("mail"),
                "expected office_config description to mention mail account onboarding"
            );
            assert!(
                !office_config.contains("http_request"),
                "user-visible office_config description must not point at hidden tools by name"
            );

            let mail = descriptions.get("mail").expect("mail description");
            assert!(
                mail.contains("office_config"),
                "mail description should point missing-account repair back to office_config"
            );
        }

        let board_info = descriptions
            .get("board_info")
            .expect("board_info description");
        assert!(
            !board_info.contains("process tool") && !board_info.contains("network tool"),
            "user-visible board_info description must not point at hidden operator tools"
        );

        if let Some(network_scan) = descriptions.get("network_scan") {
            assert!(
                !network_scan.contains("network tool"),
                "user-visible network_scan description must not point at hidden operator tools"
            );
        }

        #[cfg(all(
            feature = "tools_network_extra",
            not(any(target_arch = "xtensa", target_arch = "riscv32"))
        ))]
        {
            let ctx = crate::platform::http_server::handlers::build_default_test_handler_context();
            let http_request = ctx
                .tool_registry
                .get("http_request")
                .expect("http_request tool registered");
            assert!(
                http_request.description().contains("external HTTP APIs"),
                "http_request description must be narrowed to external HTTP APIs"
            );
        }
    }

    #[test]
    fn registry_execute_preserves_structured_outcome() {
        let mut registry =
            ToolRegistry::new().with_tool_protocol_authority(synthetic_protocol_authority(&[(
                "outcome",
                ToolProtocolContract::structured_object_json_with_outbound(),
            )]));
        registry.register(Box::new(OutcomeTool));
        let mut ctx = StubToolContext;
        let outcome = registry
            .execute("outcome", "{}", &mut ctx)
            .expect("execute");
        assert_eq!(outcome.content, r#"{"ok":true,"summary":"outcome body"}"#);
        assert_eq!(
            outcome.outbound_intents.as_slice(),
            &[crate::tools::ToolOutboundIntent {
                target: crate::tools::ToolOutboundTarget::CurrentChat,
                delivery_kind: crate::tools::ToolOutboundDeliveryKind::Primary,
                content: "tool delivered reply".to_string(),
            }]
        );
    }

    #[test]
    fn registry_execute_preserves_reported_failure_kind() {
        let mut registry =
            ToolRegistry::new().with_tool_protocol_authority(synthetic_protocol_authority(&[(
                "semantic_failure",
                ToolProtocolContract::structured_object_json(),
            )]));
        registry.register(Box::new(SemanticFailureTool));
        let mut ctx = StubToolContext;
        let outcome = registry
            .execute("semantic_failure", "{}", &mut ctx)
            .expect("execute");
        assert_eq!(outcome.content, r#"{"ok":false}"#);
        assert_eq!(
            outcome.failure_kind,
            Some(crate::tools::ToolExecutionFailureKind::Capability)
        );
    }

    #[test]
    fn assess_llm_execution_rejects_operation_envelope_without_op() {
        let mut registry =
            ToolRegistry::new().with_tool_protocol_authority(synthetic_protocol_authority(&[(
                "operation_envelope",
                ToolProtocolContract::operation_envelope_json(),
            )]));
        registry.register(Box::new(OperationEnvelopeTool));

        let error = registry
            .assess_llm_execution(
                "operation_envelope",
                r#"{"title":"missing op"}"#,
                &ToolPolicyContext::new(crate::bus::IngressKind::User, "telegram"),
            )
            .expect_err("operation envelope args without op must fail protocol validation");

        assert_eq!(error.stage(), "tool_protocol_contract");
        assert!(
            error
                .to_string()
                .contains("operation_envelope requires non-empty string field `op`"),
            "unexpected protocol error: {error}"
        );
    }

    #[test]
    fn registry_execute_rejects_invalid_structured_json_output() {
        let mut registry =
            ToolRegistry::new().with_tool_protocol_authority(synthetic_protocol_authority(&[(
                "invalid_structured_json",
                ToolProtocolContract::structured_object_json(),
            )]));
        registry.register(Box::new(InvalidStructuredJsonTool));
        let mut ctx = StubToolContext;

        let error = registry
            .execute("invalid_structured_json", "{}", &mut ctx)
            .expect_err("invalid structured json output must fail protocol validation");

        assert_eq!(error.stage(), "tool_protocol_contract");
        assert!(
            error
                .to_string()
                .contains("declared structured_json but returned non-json content"),
            "unexpected protocol error: {error}"
        );
    }

    #[test]
    fn registry_execute_rejects_missing_outbound_intent_for_outbound_protocol() {
        let mut registry =
            ToolRegistry::new().with_tool_protocol_authority(synthetic_protocol_authority(&[(
                "missing_outbound_intent",
                ToolProtocolContract::structured_object_json_with_outbound(),
            )]));
        registry.register(Box::new(MissingOutboundIntentTool));
        let mut ctx = StubToolContext;

        let error = registry
            .execute("missing_outbound_intent", "{}", &mut ctx)
            .expect_err("outbound protocol without outbound intents must fail");

        assert_eq!(error.stage(), "tool_protocol_contract");
        assert!(
            error.to_string().contains(
                "declared structured_json_with_outbound but returned no outbound intents"
            ),
            "unexpected protocol error: {error}"
        );
    }

    #[test]
    fn registry_execute_rejects_blocker_without_rich_blocker_contract() {
        let mut registry =
            ToolRegistry::new().with_tool_protocol_authority(synthetic_protocol_authority(&[(
                "rogue_blocker",
                ToolProtocolContract::structured_object_json(),
            )]));
        registry.register(Box::new(RogueBlockerTool));
        let mut ctx = StubToolContext;

        let error = registry
            .execute("rogue_blocker", "{}", &mut ctx)
            .expect_err("non-rich-blocker tools must not return blockers");

        assert_eq!(error.stage(), "tool_protocol_contract");
        assert!(
            error
                .to_string()
                .contains("returned blocker semantics without rich blocker protocol support"),
            "unexpected protocol error: {error}"
        );
    }

    #[test]
    fn registry_execute_allows_rich_blocker_when_protocol_declares_it() {
        let mut registry =
            ToolRegistry::new().with_tool_protocol_authority(synthetic_protocol_authority(&[(
                "rich_blocker",
                ToolProtocolContract::operation_envelope_json_with_rich_blockers(),
            )]));
        registry.register(Box::new(RichBlockerTool));
        let mut ctx = StubToolContext;

        let outcome = registry
            .execute("rich_blocker", r#"{"op":"inspect"}"#, &mut ctx)
            .expect("rich blocker contract should allow blocker outcome");

        assert!(outcome.blocker.is_some());
    }

    #[test]
    fn default_registry_registers_diagnose_memory_runtime_tool() {
        let ctx = crate::platform::http_server::handlers::build_default_test_handler_context();
        assert!(ctx.tool_registry.get("diagnose_memory_runtime").is_some());
    }

    #[test]
    fn default_registry_builds_from_runtime_services() {
        let config = AppConfig::load_from_env();
        let platform: Arc<dyn crate::Platform> = Arc::new(crate::platform::LinuxPlatform::new());
        let services = crate::RuntimeServices::from_platform(platform);
        let (registry, _) = build_default_registry(&config, &services);

        assert!(registry.get("get_time").is_some());
    }

    #[test]
    fn diagnose_memory_runtime_tool_returns_structured_diagnosis_json() {
        let ctx = crate::platform::http_server::handlers::build_default_test_handler_context();
        let tool = ctx
            .tool_registry
            .get("diagnose_memory_runtime")
            .expect("diagnose_memory_runtime registered");
        let mut tool_ctx = StubToolContext;

        let result = tool.execute("{}", &mut tool_ctx).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();

        assert_eq!(parsed["kind"].as_str(), Some("memory_runtime"));
        assert!(parsed.get("summary").is_some());
        assert!(parsed.get("suspected_root_causes").is_some());
    }

    #[test]
    fn default_registry_registers_diagnose_network_path_tool() {
        let ctx = crate::platform::http_server::handlers::build_default_test_handler_context();
        assert!(ctx.tool_registry.get("diagnose_network_path").is_some());
    }

    #[test]
    fn diagnose_network_path_tool_returns_structured_diagnosis_json() {
        let ctx = crate::platform::http_server::handlers::build_default_test_handler_context();
        let tool = ctx
            .tool_registry
            .get("diagnose_network_path")
            .expect("diagnose_network_path registered");
        let mut tool_ctx = StubToolContext;

        let result = tool.execute("{}", &mut tool_ctx).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();

        assert_eq!(parsed["kind"].as_str(), Some("network_path"));
        assert!(parsed.get("summary").is_some());
        assert!(parsed.get("suspected_root_causes").is_some());
    }

    #[test]
    fn default_registry_registers_diagnose_voice_path_tool() {
        let ctx = crate::platform::http_server::handlers::build_default_test_handler_context();
        assert!(ctx.tool_registry.get("diagnose_voice_path").is_some());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn default_registry_registers_lua_datasheet_distill_tool() {
        let ctx = crate::platform::http_server::handlers::build_default_test_handler_context();
        assert!(ctx.tool_registry.get("lua_datasheet_distill").is_some());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn default_registry_registers_lua_register_table_helper_tool() {
        let ctx = crate::platform::http_server::handlers::build_default_test_handler_context();
        assert!(ctx.tool_registry.get("lua_register_table_helper").is_some());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn default_registry_registers_lua_protocol_frame_helper_tool() {
        let ctx = crate::platform::http_server::handlers::build_default_test_handler_context();
        assert!(ctx.tool_registry.get("lua_protocol_frame_helper").is_some());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn default_registry_registers_lua_state_machine_checker_tool() {
        let ctx = crate::platform::http_server::handlers::build_default_test_handler_context();
        assert!(ctx.tool_registry.get("lua_state_machine_checker").is_some());
    }

    #[test]
    fn default_registry_registers_capability_atoms_exchange_tool() {
        let mut registry = ToolRegistry::new();
        let skill_storage: Arc<dyn crate::platform::SkillStorage + Send + Sync> =
            Arc::new(StubSkillStorage::default());
        registry.register(Box::new(crate::tools::CapabilityAtomsExchangeTool::new(
            Arc::clone(&skill_storage),
        )));
        registry.register(Box::new(crate::tools::CapabilityAtomsInspectTool::new(
            skill_storage,
        )));
        assert!(registry.get("capability_atoms_exchange").is_some());
        assert!(registry.get("capability_atoms_inspect").is_some());
    }

    #[cfg(feature = "capability_office")]
    #[test]
    fn default_registry_registers_documents_tool() {
        let ctx = crate::platform::http_server::handlers::build_default_test_handler_context();
        assert!(ctx.tool_registry.get("documents").is_some());
    }

    #[cfg(feature = "capability_office")]
    #[test]
    fn default_registry_registers_contacts_directory_tool() {
        let ctx = crate::platform::http_server::handlers::build_default_test_handler_context();
        assert!(ctx.tool_registry.get("contacts_directory").is_some());
    }

    #[test]
    fn diagnose_voice_path_tool_returns_structured_diagnosis_json() {
        let ctx = crate::platform::http_server::handlers::build_default_test_handler_context();
        let tool = ctx
            .tool_registry
            .get("diagnose_voice_path")
            .expect("diagnose_voice_path registered");
        let mut tool_ctx = StubToolContext;

        let result = tool.execute("{}", &mut tool_ctx).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();

        assert_eq!(parsed["kind"].as_str(), Some("voice_path"));
        assert!(parsed.get("summary").is_some());
        assert!(parsed.get("suspected_root_causes").is_some());
    }

    #[test]
    fn assess_llm_execution_uses_dynamic_network_requirement() {
        let mut registry = ToolRegistry::new();
        registry.register(Box::new(ConditionalNetworkTool));
        let policy = ToolPolicyContext::new(crate::bus::IngressKind::User, "telegram");

        let local = registry
            .assess_llm_execution("conditional_network", r#"{"op":"local"}"#, &policy)
            .expect("assess local");
        let remote = registry
            .assess_llm_execution("conditional_network", r#"{"op":"remote"}"#, &policy)
            .expect("assess remote");

        let local_requires_network = match local {
            crate::tools::ToolExecutionGateDecision::Allow(permit) => permit.requires_network(),
            other => panic!("expected allow for local op, got {other:?}"),
        };
        let remote_requires_network = match remote {
            crate::tools::ToolExecutionGateDecision::Allow(permit) => permit.requires_network(),
            other => panic!("expected allow for remote op, got {other:?}"),
        };

        assert!(!local_requires_network);
        assert!(remote_requires_network);
    }

    #[test]
    fn tool_bridge_catalog_respects_llm_visibility_policy() {
        let mut registry = ToolRegistry::new()
            .with_llm_catalog_authority(synthetic_catalog(&[
                ("visible", ToolLlmVisibility::user_and_system()),
                ("user_only_task", ToolLlmVisibility::user_only()),
            ]))
            .with_tool_protocol_authority(synthetic_protocol_authority(&[
                ("visible", ToolProtocolContract::structured_object_json()),
                (
                    "user_only_task",
                    ToolProtocolContract::operation_envelope_json(),
                ),
            ]));
        registry.register(Box::new(VisibleTool));
        registry.register(Box::new(AdminTool));
        registry.register(Box::new(UserOnlyTaskTool));

        let user = ToolPolicyContext::new(crate::bus::IngressKind::User, "telegram");
        let user_names = registry
            .tool_bridge_catalog_for_policy(&user)
            .into_iter()
            .map(|entry| entry.name)
            .collect::<Vec<_>>();
        assert_eq!(
            user_names,
            vec!["visible".to_string(), "user_only_task".to_string()]
        );

        let system = ToolPolicyContext::new(crate::bus::IngressKind::System, "telegram");
        let system_names = registry
            .tool_bridge_catalog_for_policy(&system)
            .into_iter()
            .map(|entry| entry.name)
            .collect::<Vec<_>>();
        assert_eq!(system_names, vec!["visible".to_string()]);
    }

    #[test]
    fn tool_bridge_catalog_reports_protocol_contract_truth() {
        let mut registry = ToolRegistry::new()
            .with_llm_catalog_authority(synthetic_catalog(&[
                ("visible", ToolLlmVisibility::user_and_system()),
                ("user_only_task", ToolLlmVisibility::user_only()),
            ]))
            .with_tool_protocol_authority(synthetic_protocol_authority(&[
                ("visible", ToolProtocolContract::structured_object_json()),
                (
                    "user_only_task",
                    ToolProtocolContract::operation_envelope_json(),
                ),
            ]));
        registry.register(Box::new(VisibleTool));
        registry.register(Box::new(UserOnlyTaskTool));

        let user = ToolPolicyContext::new(crate::bus::IngressKind::User, "telegram");
        let entries = registry.tool_bridge_catalog_for_policy(&user);

        let visible = entries
            .iter()
            .find(|entry| entry.name == "visible")
            .expect("visible bridge entry");
        assert_eq!(
            visible.input_protocol,
            ToolInputProtocolKind::StructuredObject
        );
        assert_eq!(
            visible.output_protocol,
            ToolOutputProtocolKind::StructuredJson
        );
        assert!(!visible.supports_rich_blockers);

        let task = entries
            .iter()
            .find(|entry| entry.name == "user_only_task")
            .expect("user_only_task bridge entry");
        assert_eq!(
            task.input_protocol,
            ToolInputProtocolKind::OperationEnvelope
        );
        assert_eq!(task.output_protocol, ToolOutputProtocolKind::StructuredJson);
        assert!(!task.supports_rich_blockers);
    }

    #[test]
    fn default_registry_protocol_contract_truth_captures_representative_tools() {
        let ctx = crate::platform::http_server::handlers::build_default_test_handler_context();
        let entries = ctx.tool_registry.tool_catalog().expect("tool catalog");

        let board_info = entries
            .iter()
            .find(|entry| entry.name == "board_info")
            .expect("board_info catalog entry");
        assert_eq!(board_info.input_protocol, "structured_object");
        assert_eq!(board_info.output_protocol, "structured_json");
        assert!(!board_info.supports_rich_blockers);

        let get_time = entries
            .iter()
            .find(|entry| entry.name == "get_time")
            .expect("get_time catalog entry");
        assert_eq!(get_time.input_protocol, "structured_object");
        assert_eq!(get_time.output_protocol, "plain_text");
        assert!(!get_time.supports_rich_blockers);

        let message = entries
            .iter()
            .find(|entry| entry.name == "message")
            .expect("message catalog entry");
        assert_eq!(message.input_protocol, "structured_object");
        assert_eq!(message.output_protocol, "structured_json_with_outbound");
        assert!(!message.supports_rich_blockers);

        #[cfg(all(
            feature = "capability_office",
            not(any(target_arch = "xtensa", target_arch = "riscv32"))
        ))]
        {
            let office_config = entries
                .iter()
                .find(|entry| entry.name == "office_config")
                .expect("office_config catalog entry");
            assert_eq!(office_config.input_protocol, "operation_envelope");
            assert_eq!(office_config.output_protocol, "structured_json");
            assert!(office_config.supports_rich_blockers);
        }
    }

    #[test]
    fn tool_catalog_uses_governance_examples_to_compute_conservative_shape() {
        let mut registry = ToolRegistry::new().with_llm_catalog_authority(synthetic_catalog(&[(
            "dynamic_governance",
            ToolLlmVisibility::user_only(),
        )]));
        registry.register(Box::new(DynamicGovernanceTool));

        let catalog_entry = registry
            .tool_catalog()
            .expect("tool catalog")
            .into_iter()
            .find(|entry| entry.name == "dynamic_governance")
            .expect("dynamic_governance entry");
        assert_eq!(catalog_entry.effect_class, "persistent_state_write");
        assert_eq!(catalog_entry.risk_level, "medium");
        assert_eq!(catalog_entry.approval_mode, "explicit_intent");

        let bridge_entry = registry
            .tool_bridge_catalog_for_policy(&ToolPolicyContext::new(
                crate::bus::IngressKind::User,
                "telegram",
            ))
            .into_iter()
            .find(|entry| entry.name == "dynamic_governance")
            .expect("dynamic_governance bridge entry");
        assert_eq!(
            bridge_entry.effect_class,
            ToolEffectClass::PersistentStateWrite
        );
        assert_eq!(bridge_entry.risk_level, ToolRiskLevel::Medium);
        assert_eq!(bridge_entry.approval_mode, ToolApprovalMode::ExplicitIntent);
    }

    #[test]
    fn capability_atoms_exchange_catalog_reports_governed_write_and_stays_out_of_system_ingress() {
        let mut registry = ToolRegistry::new().with_llm_catalog_authority(synthetic_catalog(&[
            ("capability_atoms_exchange", ToolLlmVisibility::hidden()),
            ("capability_atoms_inspect", ToolLlmVisibility::hidden()),
        ]));
        let skill_storage: Arc<dyn crate::platform::SkillStorage + Send + Sync> =
            Arc::new(StubSkillStorage::default());
        registry.register(Box::new(crate::tools::CapabilityAtomsExchangeTool::new(
            Arc::clone(&skill_storage),
        )));
        registry.register(Box::new(crate::tools::CapabilityAtomsInspectTool::new(
            skill_storage,
        )));
        let catalog_entry = registry
            .tool_catalog()
            .expect("tool catalog")
            .into_iter()
            .find(|entry| entry.name == "capability_atoms_exchange")
            .expect("capability_atoms_exchange catalog entry");
        assert_eq!(catalog_entry.effect_class, "persistent_state_write");
        assert_eq!(catalog_entry.risk_level, "medium");
        assert_eq!(catalog_entry.approval_mode, "explicit_intent");
        assert!(!catalog_entry.llm_visible_user);
        assert!(!catalog_entry.llm_visible_system);
        assert!(!catalog_entry.llm_visible_internal_system);

        let inspect_catalog_entry = registry
            .tool_catalog()
            .expect("tool catalog")
            .into_iter()
            .find(|entry| entry.name == "capability_atoms_inspect")
            .expect("capability_atoms_inspect catalog entry");
        assert_eq!(inspect_catalog_entry.effect_class, "read_only");
        assert_eq!(inspect_catalog_entry.risk_level, "low");
        assert_eq!(inspect_catalog_entry.approval_mode, "automatic");
        assert!(!inspect_catalog_entry.llm_visible_user);
        assert!(!inspect_catalog_entry.llm_visible_system);
        assert!(!inspect_catalog_entry.llm_visible_internal_system);

        let user = ToolPolicyContext::new(crate::bus::IngressKind::User, "telegram");
        assert!(registry
            .tool_bridge_catalog_for_policy(&user)
            .into_iter()
            .all(|entry| {
                entry.name != "capability_atoms_exchange"
                    && entry.name != "capability_atoms_inspect"
            }));

        let system = ToolPolicyContext::new(crate::bus::IngressKind::System, "telegram");
        let system_names = registry
            .tool_bridge_catalog_for_policy(&system)
            .into_iter()
            .map(|entry| entry.name)
            .collect::<Vec<_>>();
        assert!(!system_names
            .iter()
            .any(|name| name == "capability_atoms_exchange"));
        assert!(!system_names
            .iter()
            .any(|name| name == "capability_atoms_inspect"));
    }

    #[test]
    fn capability_atoms_exchange_import_requires_explicit_confirm_in_assessment() {
        let mut registry = ToolRegistry::new();
        let skill_storage: Arc<dyn crate::platform::SkillStorage + Send + Sync> =
            Arc::new(StubSkillStorage::default());
        registry.register(Box::new(crate::tools::CapabilityAtomsExchangeTool::new(
            skill_storage,
        )));
        let registry = registry.with_execution_governance(Arc::new(ToolExecutionGovernance::new(
            Arc::new(MemoryStateFs::default()),
        )));
        let policy = ToolPolicyContext::new(crate::bus::IngressKind::User, "telegram");

        let denied = registry.assess_tool_request_proposal(
            "capability_atoms_exchange",
            &serde_json::json!({
                "op": "import",
                "envelope": {"version":1}
            }),
            &policy,
        );
        assert_eq!(denied.decision, ToolBridgeProposalDecision::Denied);
        assert!(denied.summary.contains("not visible in the current policy"));

        let allowed = registry
            .assess_llm_execution(
                "capability_atoms_exchange",
                r#"{"op":"import","confirm":true,"envelope":{"version":1}}"#,
                &policy,
            )
            .expect("assess import");
        let permit = match allowed {
            ToolExecutionGateDecision::Allow(permit) => permit,
            other => panic!("expected allow, got {other:?}"),
        };
        assert_eq!(
            permit.shape().approval_mode,
            crate::tools::ToolApprovalMode::ExplicitIntent
        );
        assert!(permit.shape().approval_granted);
    }

    #[test]
    fn registry_execute_denies_unconfirmed_explicit_intent_tool() {
        let mut registry = ToolRegistry::new();
        let skill_storage: Arc<dyn crate::platform::SkillStorage + Send + Sync> =
            Arc::new(StubSkillStorage::default());
        registry.register(Box::new(crate::tools::CapabilityAtomsExchangeTool::new(
            skill_storage,
        )));
        let mut ctx = StubToolContext;

        let error = registry
            .execute(
                "capability_atoms_exchange",
                r#"{"op":"export","atom_name":"demo"}"#,
                &mut ctx,
            )
            .expect_err("unconfirmed exchange must fail");
        assert!(error.to_string().contains("explicit_intent_required"));
    }

    #[test]
    fn production_tools_with_dynamic_execution_shape_declare_contract_truth_source() {
        let tools_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/tools");
        let mut missing = Vec::new();
        for entry in std::fs::read_dir(&tools_dir).expect("read tools dir") {
            let entry = entry.expect("dir entry");
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
                continue;
            }
            let file_name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("");
            if matches!(
                file_name,
                "mod.rs"
                    | "registry.rs"
                    | "policy.rs"
                    | "execution_governance.rs"
                    | "state_file_guard.rs"
                    | "http_bridge.rs"
            ) {
                continue;
            }
            let content = std::fs::read_to_string(&path).expect("read tool source");
            if content.contains("fn execution_shape(")
                && !content.contains("fn governance_examples(")
                && !content.contains("fn catalog_execution_shape(")
            {
                missing.push(file_name.to_string());
            }
        }
        assert!(
            missing.is_empty(),
            "dynamic execution_shape tools missing governance truth source: {missing:?}"
        );
    }

    #[test]
    fn private_garden_tool_is_hidden_from_user_ingress_but_visible_to_system() {
        let mut registry = ToolRegistry::new().with_llm_catalog_authority(synthetic_catalog(&[(
            "private_garden",
            ToolLlmVisibility::system_and_internal(),
        )]));
        registry.register(Box::new(crate::tools::PrivateGardenTool::new(Arc::new(
            StubPrivateGardenStore,
        ))));

        let user = ToolPolicyContext::new(crate::bus::IngressKind::User, "telegram");
        let system = ToolPolicyContext::new(crate::bus::IngressKind::System, "telegram");
        let internal = ToolPolicyContext::new(crate::bus::IngressKind::User, "cron");

        assert!(!registry.is_llm_tool_visible("private_garden", &user));
        assert!(registry.is_llm_tool_visible("private_garden", &system));
        assert!(registry.is_llm_tool_visible("private_garden", &internal));

        let entry = registry
            .tool_catalog()
            .expect("tool catalog")
            .into_iter()
            .find(|entry| entry.name == "private_garden")
            .expect("private_garden catalog entry");
        assert!(!entry.llm_visible_user);
        assert!(entry.llm_visible_system);
        assert!(entry.llm_visible_internal_system);
    }

    #[test]
    fn tool_bridge_assessment_denies_unknown_and_explicit_intent_tool() {
        let mut registry = ToolRegistry::new()
            .with_execution_governance(Arc::new(ToolExecutionGovernance::new(Arc::new(
                MemoryStateFs::default(),
            ))))
            .with_llm_catalog_authority(synthetic_catalog(&[(
                "explicit_tool",
                ToolLlmVisibility::user_only(),
            )]));
        registry.register(Box::new(ExplicitIntentTool));
        let policy = ToolPolicyContext::new(crate::bus::IngressKind::User, "telegram");

        let unknown =
            registry.assess_tool_request_proposal("missing", &serde_json::json!({}), &policy);
        assert_eq!(unknown.decision, ToolBridgeProposalDecision::UnknownTool);

        let denied =
            registry.assess_tool_request_proposal("explicit_tool", &serde_json::json!({}), &policy);
        assert_eq!(denied.decision, ToolBridgeProposalDecision::Denied);
        assert!(denied.summary.contains("explicit_intent_required"));
    }

    #[test]
    fn llm_tool_specs_hide_tools_when_required_runtime_capability_is_offline() {
        let _guard = RUNTIME_CAPABILITY_TEST_GUARD
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        crate::orchestrator::reset_runtime_capabilities_for_tests();
        crate::orchestrator::update_runtime_capability(
            crate::orchestrator::RuntimeCapabilityUpdate {
                id: "audio_output",
                status: crate::orchestrator::RuntimeCapabilityStatus::Offline,
                reason: crate::orchestrator::RuntimeCapabilityReason::DeviceDisconnected,
                observed_at_secs: 10,
                recovery_hint: Some("wait_for_audio_output_recovery"),
            },
        );

        let mut registry = ToolRegistry::new();
        registry.register(Box::new(CapabilityBoundTool));

        let user = ToolPolicyContext::new(crate::bus::IngressKind::User, "telegram");
        let user_specs = registry.tool_specs_for_llm_with_max(&user, 4096);

        assert!(user_specs.is_empty());
        let blocker = registry
            .runtime_capability_blocker("capability_bound")
            .expect("runtime capability blocker");
        assert_eq!(blocker.sub_capability, "audio_output");
        assert_eq!(
            blocker.capability_status,
            crate::orchestrator::RuntimeCapabilityStatus::Offline
        );
    }

    #[test]
    fn execute_permitted_rechecks_runtime_capability_before_tool_body_runs() {
        let _guard = RUNTIME_CAPABILITY_TEST_GUARD
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        crate::orchestrator::reset_runtime_capabilities_for_tests();
        crate::orchestrator::update_runtime_capability(
            crate::orchestrator::RuntimeCapabilityUpdate {
                id: "audio_output",
                status: crate::orchestrator::RuntimeCapabilityStatus::Online,
                reason: crate::orchestrator::RuntimeCapabilityReason::Nominal,
                observed_at_secs: 1,
                recovery_hint: None,
            },
        );

        let mut registry = ToolRegistry::new();
        registry.register(Box::new(CapabilityBoundTool));
        let policy = ToolPolicyContext::new(crate::bus::IngressKind::User, "telegram");
        let permit = match registry
            .assess_llm_execution("capability_bound", "{}", &policy)
            .expect("assess")
        {
            crate::tools::ToolExecutionGateDecision::Allow(permit) => permit,
            other => panic!("expected allow, got {:?}", other),
        };

        crate::orchestrator::update_runtime_capability(
            crate::orchestrator::RuntimeCapabilityUpdate {
                id: "audio_output",
                status: crate::orchestrator::RuntimeCapabilityStatus::Offline,
                reason: crate::orchestrator::RuntimeCapabilityReason::DeviceDisconnected,
                observed_at_secs: 2,
                recovery_hint: Some("wait_for_audio_output_recovery"),
            },
        );

        let mut ctx = StubToolContext;
        let err = registry
            .execute_permitted(&permit, "{}", &mut ctx)
            .expect_err("runtime capability gate should deny execution");

        match err {
            crate::Error::Config { message, stage } => {
                assert_eq!(stage, "tool_execute_capability");
                assert!(message.contains("audio_output"));
            }
            other => panic!("expected config error, got {other:?}"),
        }
    }
}
