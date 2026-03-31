use crate::bus::IngressKind;

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

/// 工具元数据：由各工具声明基础属性，最终是否暴露由 policy 统一决定。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ToolMetadata {
    pub exposure: ToolExposure,
    /// system ingress 是否允许调用；普通 task 默认允许，stateful/admin/debug 默认禁止。
    pub allow_in_system_ingress: bool,
    /// 内部系统通道（如 cron / heartbeat）是否允许调用；默认禁止。
    pub allow_in_system_channel: bool,
}

impl ToolMetadata {
    pub const fn task() -> Self {
        Self {
            exposure: ToolExposure::Task,
            allow_in_system_ingress: true,
            allow_in_system_channel: false,
        }
    }

    pub const fn stateful() -> Self {
        Self {
            exposure: ToolExposure::Stateful,
            allow_in_system_ingress: false,
            allow_in_system_channel: false,
        }
    }

    pub const fn admin() -> Self {
        Self {
            exposure: ToolExposure::Admin,
            allow_in_system_ingress: false,
            allow_in_system_channel: false,
        }
    }

    pub const fn debug() -> Self {
        Self {
            exposure: ToolExposure::Debug,
            allow_in_system_ingress: false,
            allow_in_system_channel: false,
        }
    }

    pub const fn with_system_channel(mut self, allowed: bool) -> Self {
        self.allow_in_system_channel = allowed;
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
                    true
                }
            }
        }
    }
}

impl Default for ToolMetadata {
    fn default() -> Self {
        Self::task()
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
