use crate::bus::IngressKind;
use serde::{Deserialize, Serialize};

/// 工具暴露级别：用于运行时 tool policy 进行粗粒度裁决。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolExposure {
    /// 普通任务工具：默认允许用户会话调用。
    Task,
    /// 会修改持久化状态，但仍属于业务能力。
    Stateful,
    /// 系统/配置级能力；默认不向 LLM 暴露。
    Admin,
    /// 调试/诊断类能力；默认不向 LLM 暴露。
    Debug,
}

impl ToolExposure {
    pub fn label(self) -> &'static str {
        match self {
            Self::Task => "task",
            Self::Stateful => "stateful",
            Self::Admin => "admin",
            Self::Debug => "debug",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ToolEffectClass {
    #[default]
    ReadOnly,
    PersistentStateWrite,
    ConfigWrite,
    VisibleOutbound,
    HardwareRead,
    HardwareActuation,
    HostInspection,
    HostExecution,
    SystemControl,
}

impl ToolEffectClass {
    pub fn label(self) -> &'static str {
        match self {
            Self::ReadOnly => "read_only",
            Self::PersistentStateWrite => "persistent_state_write",
            Self::ConfigWrite => "config_write",
            Self::VisibleOutbound => "visible_outbound",
            Self::HardwareRead => "hardware_read",
            Self::HardwareActuation => "hardware_actuation",
            Self::HostInspection => "host_inspection",
            Self::HostExecution => "host_execution",
            Self::SystemControl => "system_control",
        }
    }

    pub fn is_mutating(self) -> bool {
        matches!(
            self,
            Self::PersistentStateWrite
                | Self::ConfigWrite
                | Self::VisibleOutbound
                | Self::HardwareActuation
                | Self::HostExecution
                | Self::SystemControl
        )
    }

    pub fn conservative_rank(self) -> u8 {
        match self {
            Self::ReadOnly => 0,
            Self::HardwareRead => 1,
            Self::HostInspection => 2,
            Self::PersistentStateWrite => 3,
            Self::VisibleOutbound => 4,
            Self::ConfigWrite => 5,
            Self::HardwareActuation => 6,
            Self::HostExecution => 7,
            Self::SystemControl => 8,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum ToolRiskLevel {
    #[default]
    Low,
    Medium,
    High,
    Critical,
}

impl ToolRiskLevel {
    pub fn label(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Critical => "critical",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum ToolApprovalMode {
    #[default]
    Automatic,
    ExplicitIntent,
    OperatorOnly,
}

impl ToolApprovalMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Automatic => "automatic",
            Self::ExplicitIntent => "explicit_intent",
            Self::OperatorOnly => "operator_only",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ToolRollbackKind {
    #[default]
    None,
    CompensatingWrite,
    ConfigRestore,
    ReplayableMutation,
    Irreversible,
}

impl ToolRollbackKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::CompensatingWrite => "compensating_write",
            Self::ConfigRestore => "config_restore",
            Self::ReplayableMutation => "replayable_mutation",
            Self::Irreversible => "irreversible",
        }
    }

    pub fn conservative_rank(self) -> u8 {
        match self {
            Self::None => 0,
            Self::CompensatingWrite => 1,
            Self::ReplayableMutation => 2,
            Self::ConfigRestore => 3,
            Self::Irreversible => 4,
        }
    }
}

/// 工具元数据：由各工具声明基础属性，最终是否暴露由 policy 统一决定。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ToolMetadata {
    pub exposure: ToolExposure,
    /// 普通用户 ingress 是否允许暴露给 LLM；task/stateful 默认允许。
    pub allow_in_user_ingress: bool,
    /// system ingress 是否允许调用；普通 task 默认允许，stateful/admin/debug 默认禁止。
    pub allow_in_system_ingress: bool,
    /// 内部系统通道（如 cron / heartbeat）是否允许调用；默认禁止。
    pub allow_in_system_channel: bool,
    pub effect_class: ToolEffectClass,
    pub risk_level: ToolRiskLevel,
    pub approval_mode: ToolApprovalMode,
    pub rollback_kind: ToolRollbackKind,
}

impl ToolMetadata {
    pub const fn task() -> Self {
        Self {
            exposure: ToolExposure::Task,
            allow_in_user_ingress: true,
            allow_in_system_ingress: true,
            allow_in_system_channel: false,
            effect_class: ToolEffectClass::ReadOnly,
            risk_level: ToolRiskLevel::Low,
            approval_mode: ToolApprovalMode::Automatic,
            rollback_kind: ToolRollbackKind::None,
        }
    }

    pub const fn stateful() -> Self {
        Self {
            exposure: ToolExposure::Stateful,
            allow_in_user_ingress: true,
            allow_in_system_ingress: false,
            allow_in_system_channel: false,
            effect_class: ToolEffectClass::PersistentStateWrite,
            risk_level: ToolRiskLevel::Medium,
            approval_mode: ToolApprovalMode::Automatic,
            rollback_kind: ToolRollbackKind::CompensatingWrite,
        }
    }

    pub const fn admin() -> Self {
        Self {
            exposure: ToolExposure::Admin,
            allow_in_user_ingress: false,
            allow_in_system_ingress: false,
            allow_in_system_channel: false,
            effect_class: ToolEffectClass::ConfigWrite,
            risk_level: ToolRiskLevel::Critical,
            approval_mode: ToolApprovalMode::OperatorOnly,
            rollback_kind: ToolRollbackKind::ConfigRestore,
        }
    }

    pub const fn debug() -> Self {
        Self {
            exposure: ToolExposure::Debug,
            allow_in_user_ingress: false,
            allow_in_system_ingress: false,
            allow_in_system_channel: false,
            effect_class: ToolEffectClass::HostExecution,
            risk_level: ToolRiskLevel::Critical,
            approval_mode: ToolApprovalMode::OperatorOnly,
            rollback_kind: ToolRollbackKind::Irreversible,
        }
    }

    pub const fn with_system_channel(mut self, allowed: bool) -> Self {
        self.allow_in_system_channel = allowed;
        self
    }

    pub const fn with_user_ingress(mut self, allowed: bool) -> Self {
        self.allow_in_user_ingress = allowed;
        self
    }

    pub const fn with_system_ingress(mut self, allowed: bool) -> Self {
        self.allow_in_system_ingress = allowed;
        self
    }

    pub const fn with_effect_class(mut self, effect_class: ToolEffectClass) -> Self {
        self.effect_class = effect_class;
        self
    }

    pub const fn with_risk_level(mut self, risk_level: ToolRiskLevel) -> Self {
        self.risk_level = risk_level;
        self
    }

    pub const fn with_approval_mode(mut self, approval_mode: ToolApprovalMode) -> Self {
        self.approval_mode = approval_mode;
        self
    }

    pub const fn with_rollback_kind(mut self, rollback_kind: ToolRollbackKind) -> Self {
        self.rollback_kind = rollback_kind;
        self
    }

    pub fn is_exposed_to_llm(self, ctx: &ToolPolicyContext<'_>) -> bool {
        match self.exposure {
            ToolExposure::Admin | ToolExposure::Debug => false,
            ToolExposure::Task | ToolExposure::Stateful => {
                if ctx.is_internal_system_channel() {
                    self.allow_in_system_channel
                } else if ctx.ingress == IngressKind::System {
                    self.allow_in_system_ingress
                } else {
                    self.allow_in_user_ingress
                }
            }
        }
    }

    pub fn default_execution_shape(self, tool_name: &str) -> ToolExecutionShape {
        ToolExecutionShape {
            operation: tool_name.trim().to_string(),
            effect_class: self.effect_class,
            risk_level: self.risk_level,
            approval_mode: self.approval_mode,
            approval_granted: matches!(self.approval_mode, ToolApprovalMode::Automatic),
            rollback_kind: self.rollback_kind,
        }
    }
}

impl Default for ToolMetadata {
    fn default() -> Self {
        Self::task()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolExecutionShape {
    pub operation: String,
    pub effect_class: ToolEffectClass,
    pub risk_level: ToolRiskLevel,
    pub approval_mode: ToolApprovalMode,
    pub approval_granted: bool,
    pub rollback_kind: ToolRollbackKind,
}

impl ToolExecutionShape {
    pub fn with_operation(mut self, operation: impl Into<String>) -> Self {
        self.operation = operation.into();
        self
    }

    pub fn with_effect_class(mut self, effect_class: ToolEffectClass) -> Self {
        self.effect_class = effect_class;
        self
    }

    pub fn with_risk_level(mut self, risk_level: ToolRiskLevel) -> Self {
        self.risk_level = risk_level;
        self
    }

    pub fn with_approval_mode(mut self, approval_mode: ToolApprovalMode) -> Self {
        self.approval_mode = approval_mode;
        self
    }

    pub fn with_approval_granted(mut self, approval_granted: bool) -> Self {
        self.approval_granted = approval_granted;
        self
    }

    pub fn with_rollback_kind(mut self, rollback_kind: ToolRollbackKind) -> Self {
        self.rollback_kind = rollback_kind;
        self
    }
}

pub fn conservative_merge_execution_shapes(
    left: ToolExecutionShape,
    right: ToolExecutionShape,
) -> ToolExecutionShape {
    let effect_class =
        if right.effect_class.conservative_rank() > left.effect_class.conservative_rank() {
            right.effect_class
        } else {
            left.effect_class
        };
    let risk_level = left.risk_level.max(right.risk_level);
    let approval_mode = left.approval_mode.max(right.approval_mode);
    let rollback_kind =
        if right.rollback_kind.conservative_rank() > left.rollback_kind.conservative_rank() {
            right.rollback_kind
        } else {
            left.rollback_kind
        };
    ToolExecutionShape {
        operation: left.operation,
        effect_class,
        risk_level,
        approval_mode,
        approval_granted: matches!(approval_mode, ToolApprovalMode::Automatic),
        rollback_kind,
    }
}

/// 单次 LLM 请求的 tool policy 上下文。
#[derive(Clone, Copy, Debug)]
pub struct ToolPolicyContext<'a> {
    pub ingress: IngressKind,
    pub channel: &'a str,
}

impl<'a> ToolPolicyContext<'a> {
    pub fn new(ingress: IngressKind, channel: &'a str) -> Self {
        Self { ingress, channel }
    }

    pub fn is_internal_system_channel(&self) -> bool {
        matches!(self.channel, "cron" | "heartbeat")
    }
}
