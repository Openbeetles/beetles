//! Agent 运行策略：按平台选择轻量或增强链路。
//! Internal agent strategy helpers for platform-specific behavior.

use crate::bus::{IngressKind, PcMsg};
use crate::orchestrator::PressureLevel;
use crate::util::truncate_content_to_max;

const PLAN_HINT_PREFIX: &str = "\n\n## Execution plan\n";
const PLAN_HINT_SUFFIX: &str =
    "\nFollow this plan, but adapt immediately when tool results contradict it.";
const PLAN_MAX_CHARS: usize = 512;

/// 内部运行策略：ESP 保持轻量，Linux 启用额外 planning / reflection 增强。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentRunStrategy {
    Embedded,
    LinuxEnhanced,
}

impl AgentRunStrategy {
    pub(crate) fn enables_preplanning(self) -> bool {
        matches!(self, Self::LinuxEnhanced)
    }

    pub(crate) fn enables_reflection_boost(self) -> bool {
        matches!(self, Self::LinuxEnhanced)
    }
}

pub(crate) fn should_generate_execution_plan(
    msg: &PcMsg,
    has_tools: bool,
    pressure: PressureLevel,
) -> bool {
    if msg.ingress != IngressKind::User
        || msg.is_group
        || !has_tools
        || pressure != PressureLevel::Normal
    {
        return false;
    }
    let content = msg.content.trim();
    let char_count = content.chars().count();
    let separators = ['\n', ',', '，', '.', '。', '?', '？', ';', '；'];
    let separator_count = content.chars().filter(|ch| separators.contains(ch)).count();
    let task_markers = [
        "然后",
        "并且",
        "同时",
        "先",
        "再",
        "分别",
        "步骤",
        "排查",
        "分析",
        "修复",
        "设计",
        "implement",
        "debug",
        "analyze",
        "review",
        "plan",
    ];
    let marker_hits = task_markers
        .iter()
        .filter(|marker| content.contains(**marker))
        .count();
    if char_count < 48 && separator_count < 2 && marker_hits < 2 {
        return false;
    }
    separator_count >= 2 || marker_hits >= 2 || char_count >= 120
}

pub(crate) fn append_execution_plan(system: &mut String, max_len: usize, plan: &str) {
    let plan = truncate_content_to_max(plan.trim(), PLAN_MAX_CHARS);
    if plan.is_empty() {
        return;
    }
    let addition = format!("{}{}{}", PLAN_HINT_PREFIX, plan, PLAN_HINT_SUFFIX);
    if system.len().saturating_add(addition.len()) <= max_len {
        system.push_str(&addition);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn complex_requests_trigger_internal_planning() {
        let msg = PcMsg::new_inbound(
            "telegram",
            "chat",
            "先分析问题，再给出修复步骤，并说明验证方式。",
            false,
        )
        .expect("pcmsg");
        assert!(should_generate_execution_plan(
            &msg,
            true,
            PressureLevel::Normal
        ));
    }

    #[test]
    fn simple_requests_skip_internal_planning() {
        let msg = PcMsg::new_inbound("telegram", "chat", "现在几点", false).expect("pcmsg");
        assert!(!should_generate_execution_plan(
            &msg,
            true,
            PressureLevel::Normal
        ));
    }

    #[test]
    fn execution_plan_hint_respects_system_budget() {
        let mut system = "base".to_string();
        append_execution_plan(&mut system, 12, "1. step");
        assert_eq!(system, "base");
    }
}
