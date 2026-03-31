//! Agent 运行策略：按平台选择轻量或增强链路。
//! Internal agent strategy helpers for platform-specific behavior.

use super::tool_outcome::ToolFailureSummary;
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

pub(crate) fn build_tool_round_guidance(
    strategy: AgentRunStrategy,
    round_had_success: bool,
    consecutive_stalled_rounds: u8,
    total_calls: usize,
    repeated_calls: usize,
    failure_summary: ToolFailureSummary,
    ping_pong_detected: bool,
) -> Option<String> {
    if strategy != AgentRunStrategy::LinuxEnhanced
        || round_had_success
        || total_calls == 0
        || failure_summary.failed_calls == 0
    {
        return None;
    }
    if ping_pong_detected {
        return Some(
            "\n\n[SYSTEM] Recent tool rounds are bouncing between two failed call patterns without progress. Stop alternating between the same approaches. Choose a clearly different tool path or explain the blocker and remaining uncertainty to the user."
                .to_string(),
        );
    }
    if failure_summary.failed_calls == total_calls
        && failure_summary.capability_failures == failure_summary.failed_calls
    {
        return Some(
            "\n\n[SYSTEM] All tool calls in this round were blocked by permissions, unavailable tools, or runtime policy. Stop retrying the same requests. Explain the limitation clearly to the user or switch to a tool path that is actually allowed."
                .to_string(),
        );
    }
    if failure_summary.failed_calls == total_calls
        && failure_summary.permanent_failures == failure_summary.failed_calls
    {
        return Some(
            "\n\n[SYSTEM] All tool calls in this round failed due to invalid input, missing resources, or unsupported actions. Do not retry unchanged parameters. Fix the inputs or explain the blocker clearly."
                .to_string(),
        );
    }
    if failure_summary.failed_calls == total_calls
        && failure_summary.retryable_failures == failure_summary.failed_calls
        && consecutive_stalled_rounds >= 2
    {
        return Some(
            "\n\n[SYSTEM] Recent tool calls are failing for transient reasons such as timeouts, network issues, or server pressure. Retry at most with a meaningfully different path; otherwise explain the temporary blocker instead of brute-forcing."
                .to_string(),
        );
    }
    if consecutive_stalled_rounds >= 3 {
        return Some(
            "\n\n[SYSTEM] You have spent multiple tool rounds without useful progress. Stop retrying the same path. Either choose a clearly different tool strategy or explain the blocker and remaining uncertainty to the user."
                .to_string(),
        );
    }
    if failure_summary.failed_calls == total_calls && repeated_calls > 0 {
        return Some(format!(
            "\n\n[SYSTEM] All {} tool call(s) in this round failed, and {} repeated a previous call pattern. Do not retry the same call again unless new evidence changes the inputs. Either switch to a different tool path or explain the blocker clearly.",
            total_calls, repeated_calls
        ));
    }
    if failure_summary.failed_calls == total_calls && total_calls > 1 {
        return Some(format!(
            "\n\n[SYSTEM] All {} tool call(s) in this round failed. Do not brute-force similar retries. Try a different tool family or explain the blocker clearly if no better action exists.",
            total_calls
        ));
    }
    if repeated_calls > 0 {
        return Some(format!(
            "\n\n[SYSTEM] {} tool call(s) repeated a previous call pattern without progress. Do not keep retrying the same action. Change strategy or explain the blocker.",
            repeated_calls
        ));
    }
    Some(
        "\n\n[SYSTEM] The last tool round did not produce a useful result. Reassess the plan, avoid repeating the same call, and either switch strategy or explain the blocker clearly."
            .to_string(),
    )
}

pub(crate) fn detect_ping_pong_tool_rounds(recent_round_signatures: &[Option<u64>; 4]) -> bool {
    match *recent_round_signatures {
        [Some(a), Some(b), Some(c), Some(d)] => a != b && a == c && b == d,
        _ => false,
    }
}

pub(crate) fn stalled_end_turn_followup(
    strategy: AgentRunStrategy,
    consecutive_stalled_rounds: u8,
    content: &str,
) -> Option<&'static str> {
    if strategy != AgentRunStrategy::LinuxEnhanced || consecutive_stalled_rounds < 2 {
        return None;
    }
    if content_signals_blocker(content) {
        return None;
    }
    if content.chars().count() >= 320 {
        return None;
    }
    Some(
        "[SYSTEM] Previous tool attempts failed or made no useful progress. Do not keep retrying silently. Either switch to a clearly different approach now or explain the blocker, what remains unknown, and what the user can do next.",
    )
}

fn content_signals_blocker(content: &str) -> bool {
    let lower = content.to_ascii_lowercase();
    let markers = [
        "阻塞",
        "卡住",
        "受限",
        "限制",
        "无法",
        "不能",
        "没法",
        "没有权限",
        "没有工具",
        "不支持",
        "做不到",
        "失败",
        "blocker",
        "blocked",
        "limitation",
        "limited",
        "cannot",
        "can't",
        "unable",
        "failed",
        "do not have access",
        "don't have access",
        "not available",
        "not supported",
    ];
    markers
        .iter()
        .any(|marker| content.contains(marker) || lower.contains(marker))
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

    #[test]
    fn tool_round_guidance_escalates_after_repeated_stalls() {
        let guidance = build_tool_round_guidance(
            AgentRunStrategy::LinuxEnhanced,
            false,
            3,
            2,
            1,
            ToolFailureSummary {
                failed_calls: 2,
                retryable_failures: 1,
                permanent_failures: 1,
                ..ToolFailureSummary::default()
            },
            false,
        )
        .expect("guidance");
        assert!(guidance.contains("multiple tool rounds without useful progress"));
    }

    #[test]
    fn tool_round_guidance_prioritizes_ping_pong_loops() {
        let guidance = build_tool_round_guidance(
            AgentRunStrategy::LinuxEnhanced,
            false,
            4,
            1,
            0,
            ToolFailureSummary {
                failed_calls: 1,
                permanent_failures: 1,
                ..ToolFailureSummary::default()
            },
            true,
        )
        .expect("guidance");
        assert!(guidance.contains("bouncing between two failed call patterns"));
    }

    #[test]
    fn tool_round_guidance_surfaces_capability_limits() {
        let guidance = build_tool_round_guidance(
            AgentRunStrategy::LinuxEnhanced,
            false,
            1,
            2,
            0,
            ToolFailureSummary {
                failed_calls: 2,
                capability_failures: 2,
                ..ToolFailureSummary::default()
            },
            false,
        )
        .expect("guidance");
        assert!(guidance.contains("blocked by permissions"));
    }

    #[test]
    fn tool_round_guidance_surfaces_permanent_input_failures() {
        let guidance = build_tool_round_guidance(
            AgentRunStrategy::LinuxEnhanced,
            false,
            1,
            2,
            0,
            ToolFailureSummary {
                failed_calls: 2,
                permanent_failures: 2,
                ..ToolFailureSummary::default()
            },
            false,
        )
        .expect("guidance");
        assert!(guidance.contains("invalid input"));
    }

    #[test]
    fn tool_round_guidance_surfaces_retryable_failures_after_stalls() {
        let guidance = build_tool_round_guidance(
            AgentRunStrategy::LinuxEnhanced,
            false,
            2,
            2,
            0,
            ToolFailureSummary {
                failed_calls: 2,
                retryable_failures: 2,
                ..ToolFailureSummary::default()
            },
            false,
        )
        .expect("guidance");
        assert!(guidance.contains("transient reasons"));
    }

    #[test]
    fn tool_round_guidance_skips_partial_success_rounds() {
        let guidance = build_tool_round_guidance(
            AgentRunStrategy::LinuxEnhanced,
            true,
            1,
            2,
            1,
            ToolFailureSummary {
                failed_calls: 1,
                permanent_failures: 1,
                ..ToolFailureSummary::default()
            },
            false,
        );
        assert!(guidance.is_none());
    }

    #[test]
    fn ping_pong_detection_matches_alternating_rounds() {
        assert!(detect_ping_pong_tool_rounds(&[
            Some(11),
            Some(29),
            Some(11),
            Some(29),
        ]));
    }

    #[test]
    fn ping_pong_detection_rejects_single_pattern_repeats() {
        assert!(!detect_ping_pong_tool_rounds(&[
            Some(11),
            Some(11),
            Some(11),
            Some(11),
        ]));
    }

    #[test]
    fn stalled_end_turn_followup_requires_blocker_or_new_strategy() {
        let followup = stalled_end_turn_followup(
            AgentRunStrategy::LinuxEnhanced,
            2,
            "我先给你一个很简短的结论。",
        );
        assert!(followup.is_some());
    }

    #[test]
    fn stalled_end_turn_followup_skips_explicit_blockers() {
        let followup = stalled_end_turn_followup(
            AgentRunStrategy::LinuxEnhanced,
            2,
            "我无法继续，因为当前工具没有权限读取这个资源。",
        );
        assert!(followup.is_none());
    }
}
