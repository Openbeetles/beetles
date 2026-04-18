//! Agent 运行策略：按平台选择轻量或增强链路。
//! Internal agent strategy selector for platform-specific behavior.

/// 内部运行策略：ESP 保持轻量，Linux 启用更完整的治理与观测链路。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentRunStrategy {
    Embedded,
    LinuxEnhanced,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strategy_variants_remain_stable() {
        assert_eq!(AgentRunStrategy::Embedded, AgentRunStrategy::Embedded);
        assert_eq!(
            AgentRunStrategy::LinuxEnhanced,
            AgentRunStrategy::LinuxEnhanced
        );
    }
}
