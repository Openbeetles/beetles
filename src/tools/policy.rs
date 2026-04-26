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
    LocalPure,
    #[default]
    ReadOnly,
    ConfigRead,
    NetworkSearch,
    PersistentStateWrite,
    StorageWrite,
    ConfigWrite,
    VisibleOutbound,
    HardwareRead,
    HardwareActuation,
    Diagnostic,
    HostInspection,
    HostExecution,
    SystemControl,
    Admin,
}

impl ToolEffectClass {
    pub fn label(self) -> &'static str {
        match self {
            Self::LocalPure => "local_pure",
            Self::ReadOnly => "read_only",
            Self::ConfigRead => "config_read",
            Self::NetworkSearch => "network_search",
            Self::PersistentStateWrite => "persistent_state_write",
            Self::StorageWrite => "storage_write",
            Self::ConfigWrite => "config_write",
            Self::VisibleOutbound => "visible_outbound",
            Self::HardwareRead => "hardware_read",
            Self::HardwareActuation => "hardware_actuation",
            Self::Diagnostic => "diagnostic",
            Self::HostInspection => "host_inspection",
            Self::HostExecution => "host_execution",
            Self::SystemControl => "system_control",
            Self::Admin => "admin",
        }
    }

    pub fn is_mutating(self) -> bool {
        matches!(
            self,
            Self::PersistentStateWrite
                | Self::StorageWrite
                | Self::ConfigWrite
                | Self::VisibleOutbound
                | Self::HardwareActuation
                | Self::HostExecution
                | Self::SystemControl
                | Self::Admin
        )
    }

    pub fn conservative_rank(self) -> u8 {
        match self {
            Self::LocalPure | Self::ReadOnly => 0,
            Self::ConfigRead => 1,
            Self::HardwareRead => 2,
            Self::Diagnostic | Self::HostInspection => 3,
            Self::NetworkSearch => 4,
            Self::PersistentStateWrite | Self::StorageWrite => 5,
            Self::VisibleOutbound => 6,
            Self::ConfigWrite => 7,
            Self::HardwareActuation => 8,
            Self::HostExecution => 9,
            Self::SystemControl | Self::Admin => 10,
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
    pub effect_class: ToolEffectClass,
    pub risk_level: ToolRiskLevel,
    pub approval_mode: ToolApprovalMode,
    pub rollback_kind: ToolRollbackKind,
}

impl ToolMetadata {
    pub const fn task() -> Self {
        Self {
            exposure: ToolExposure::Task,
            effect_class: ToolEffectClass::ReadOnly,
            risk_level: ToolRiskLevel::Low,
            approval_mode: ToolApprovalMode::Automatic,
            rollback_kind: ToolRollbackKind::None,
        }
    }

    pub const fn stateful() -> Self {
        Self {
            exposure: ToolExposure::Stateful,
            effect_class: ToolEffectClass::PersistentStateWrite,
            risk_level: ToolRiskLevel::Medium,
            approval_mode: ToolApprovalMode::Automatic,
            rollback_kind: ToolRollbackKind::CompensatingWrite,
        }
    }

    pub const fn admin() -> Self {
        Self {
            exposure: ToolExposure::Admin,
            effect_class: ToolEffectClass::ConfigWrite,
            risk_level: ToolRiskLevel::Critical,
            approval_mode: ToolApprovalMode::OperatorOnly,
            rollback_kind: ToolRollbackKind::ConfigRestore,
        }
    }

    pub const fn debug() -> Self {
        Self {
            exposure: ToolExposure::Debug,
            effect_class: ToolEffectClass::HostExecution,
            risk_level: ToolRiskLevel::Critical,
            approval_mode: ToolApprovalMode::OperatorOnly,
            rollback_kind: ToolRollbackKind::Irreversible,
        }
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
pub const DEFAULT_EMBEDDED_TOOL_PROFILE: bool = cfg!(any(
    target_arch = "xtensa",
    target_arch = "riscv32",
    target_os = "linux"
));

#[derive(Clone, Copy, Debug)]
pub struct ToolPolicyContext<'a> {
    pub ingress: IngressKind,
    pub channel: &'a str,
    pub runtime_mode: Option<crate::runtime::RuntimeMode>,
    pub embedded_profile: bool,
}

impl<'a> ToolPolicyContext<'a> {
    pub fn new(ingress: IngressKind, channel: &'a str) -> Self {
        Self {
            ingress,
            channel,
            runtime_mode: None,
            embedded_profile: DEFAULT_EMBEDDED_TOOL_PROFILE,
        }
    }

    pub fn with_runtime_mode(mut self, runtime_mode: crate::runtime::RuntimeMode) -> Self {
        self.runtime_mode = Some(runtime_mode);
        self
    }

    pub fn with_embedded_profile(mut self, embedded_profile: bool) -> Self {
        self.embedded_profile = embedded_profile;
        self
    }

    pub fn is_internal_system_channel(&self) -> bool {
        matches!(self.channel, "cron" | "heartbeat")
    }
}

/// Returns whether a tool effect class may be exposed/executed for the current runtime policy.
///
/// This is based only on system state and tool metadata. It must not inspect user text.
pub fn tool_effect_visible_in_mode(
    effect_class: ToolEffectClass,
    policy: &ToolPolicyContext<'_>,
) -> bool {
    if !policy.embedded_profile {
        return true;
    }
    let Some(runtime_mode) = policy.runtime_mode else {
        return true;
    };
    match runtime_mode {
        crate::runtime::RuntimeMode::Normal => true,
        crate::runtime::RuntimeMode::VoiceExclusive => matches!(
            effect_class,
            ToolEffectClass::LocalPure | ToolEffectClass::ReadOnly | ToolEffectClass::HardwareRead
        ),
        crate::runtime::RuntimeMode::Booting
        | crate::runtime::RuntimeMode::Pairing
        | crate::runtime::RuntimeMode::ConfigActive
        | crate::runtime::RuntimeMode::Maintenance
        | crate::runtime::RuntimeMode::RecoverySafeMode => matches!(
            effect_class,
            ToolEffectClass::LocalPure
                | ToolEffectClass::ReadOnly
                | ToolEffectClass::ConfigRead
                | ToolEffectClass::Diagnostic
                | ToolEffectClass::HardwareRead
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::{ToolEffectClass, ToolMetadata, ToolPolicyContext};

    #[test]
    fn p0_effect_classes_have_stable_labels_and_mutation_semantics() {
        assert_eq!(ToolEffectClass::LocalPure.label(), "local_pure");
        assert_eq!(ToolEffectClass::NetworkSearch.label(), "network_search");
        assert_eq!(ToolEffectClass::ConfigRead.label(), "config_read");
        assert_eq!(ToolEffectClass::StorageWrite.label(), "storage_write");
        assert_eq!(ToolEffectClass::Diagnostic.label(), "diagnostic");
        assert_eq!(ToolEffectClass::Admin.label(), "admin");

        assert!(!ToolEffectClass::LocalPure.is_mutating());
        assert!(!ToolEffectClass::ConfigRead.is_mutating());
        assert!(!ToolEffectClass::NetworkSearch.is_mutating());
        assert!(!ToolEffectClass::Diagnostic.is_mutating());
        assert!(ToolEffectClass::StorageWrite.is_mutating());
        assert!(ToolEffectClass::Admin.is_mutating());
    }

    #[test]
    fn p0_effect_classes_merge_by_conservative_rank() {
        let local = ToolMetadata::task()
            .with_effect_class(ToolEffectClass::LocalPure)
            .default_execution_shape("local");
        let network = ToolMetadata::task()
            .with_effect_class(ToolEffectClass::NetworkSearch)
            .default_execution_shape("search");
        let merged = super::conservative_merge_execution_shapes(local, network);

        assert_eq!(merged.effect_class, ToolEffectClass::NetworkSearch);
        assert_eq!(merged.operation, "local");
    }

    #[test]
    fn embedded_runtime_mode_filters_effect_classes_without_user_text() {
        let voice_policy = ToolPolicyContext::new(crate::bus::IngressKind::User, "voice")
            .with_embedded_profile(true)
            .with_runtime_mode(crate::runtime::RuntimeMode::VoiceExclusive);
        assert!(super::tool_effect_visible_in_mode(
            ToolEffectClass::ReadOnly,
            &voice_policy
        ));
        assert!(!super::tool_effect_visible_in_mode(
            ToolEffectClass::NetworkSearch,
            &voice_policy
        ));
        assert!(!super::tool_effect_visible_in_mode(
            ToolEffectClass::VisibleOutbound,
            &voice_policy
        ));

        let recovery_policy = ToolPolicyContext::new(crate::bus::IngressKind::System, "cron")
            .with_embedded_profile(true)
            .with_runtime_mode(crate::runtime::RuntimeMode::RecoverySafeMode);
        assert!(super::tool_effect_visible_in_mode(
            ToolEffectClass::Diagnostic,
            &recovery_policy
        ));
        assert!(!super::tool_effect_visible_in_mode(
            ToolEffectClass::SystemControl,
            &recovery_policy
        ));
    }

    #[test]
    fn default_embedded_profile_tracks_embedded_target_family() {
        let policy = ToolPolicyContext::new(crate::bus::IngressKind::User, "telegram");
        assert_eq!(
            policy.embedded_profile,
            super::DEFAULT_EMBEDDED_TOOL_PROFILE
        );
    }
}
