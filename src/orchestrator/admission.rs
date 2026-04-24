//! 四维门禁决策：入站/出站/LLM/工具，基于统一资源快照做全局协调。
//! Four-dimensional admission: inbound/outbound/LLM/tool, coordinated via unified resource snapshot.

use crate::bus::IngressKind;
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
use crate::constants::OUTBOUND_DEFER_DELAY_MS_CAUTIOUS;
use crate::constants::TLS_ADMISSION_MIN_INTERNAL_BYTES;
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
use crate::constants::TLS_ADMISSION_MIN_LARGEST_BLOCK_BYTES;
use crate::constants::{
    LLM_RETRY_LATER_DELAY_MS, LOW_MEM_DEFER_SLEEP_MS, OUTBOUND_DEFER_DELAY_MS,
    PRESSURE_QUEUE_CONGESTION_THRESHOLD,
};
use crate::runtime::system_work::{classify_system_work, SystemWorkClass};
use std::sync::atomic::Ordering;

use super::pressure::PressureLevel;
use super::state::OrchestratorState;

/// 入站/出站通用决策。
/// Common admission decision for inbound/outbound.
pub enum AdmissionDecision {
    Accept,
    Defer { delay_ms: u64 },
    Reject { reason: &'static str },
}

/// LLM 调用门控决策。
/// LLM call gating decision.
#[derive(Debug)]
pub enum LlmDecision {
    Proceed,
    RetryLater { delay_ms: u64 },
    Degrade { reason: &'static str },
}

/// 工具执行门控决策。
/// Tool execution gating decision.
pub enum ToolDecision {
    Allow,
    Deny { reason: &'static str },
}

const LOW_MEM_DEFER_SLEEP_MS_MIN: u64 = 650;
const LLM_RETRY_LATER_DELAY_MS_MIN: u64 = 240;
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
const OUTBOUND_DEFER_DELAY_MS_CAUTIOUS_MIN: u64 = 120;
const OUTBOUND_BACKGROUND_YIELD_MS_MIN: u64 = 80;
const OUTBOUND_BACKGROUND_YIELD_MS_MAX: u64 = 260;
const MODE_TRANSITION_DEFER_MS: u64 = 250;

#[inline]
fn queue_total(state: &OrchestratorState) -> u32 {
    let inbound = state.inbound_depth.load(Ordering::Relaxed);
    let outbound = state.outbound_depth.load(Ordering::Relaxed);
    inbound.saturating_add(outbound)
}

#[inline]
fn is_queue_congested(state: &OrchestratorState) -> bool {
    queue_total(state) >= PRESSURE_QUEUE_CONGESTION_THRESHOLD
}

#[inline]
fn critical_inbound_defer_delay_ms(state: &OrchestratorState) -> u64 {
    // Keep protective backoff under pressure, but avoid fixed 1.8s stall on near-threshold cases.
    let base = LOW_MEM_DEFER_SLEEP_MS;
    let total = queue_total(state) as u64;
    let threshold = (PRESSURE_QUEUE_CONGESTION_THRESHOLD as u64).max(1);
    if total >= threshold {
        return base;
    }
    let scaled = LOW_MEM_DEFER_SLEEP_MS_MIN
        + (base.saturating_sub(LOW_MEM_DEFER_SLEEP_MS_MIN)) * total / threshold;
    scaled.clamp(LOW_MEM_DEFER_SLEEP_MS_MIN, base)
}

#[inline]
fn current_runtime_mode() -> crate::runtime::RuntimeModeSnapshot {
    crate::runtime::thread_registry::runtime_mode_snapshot()
}

#[inline]
fn is_voice_channel(channel: &str) -> bool {
    channel == crate::constants::VOICE_CHANNEL_NAME
}

#[inline]
fn non_voice_block_mode(
    mode: crate::runtime::RuntimeModeSnapshot,
    channel: &str,
) -> Option<crate::runtime::RuntimeMode> {
    if !mode.action_budget.allow_non_voice_outbound && !is_voice_channel(channel) {
        Some(mode.current_mode)
    } else {
        None
    }
}

fn non_voice_background_reason(mode: crate::runtime::RuntimeMode) -> &'static str {
    match mode {
        crate::runtime::RuntimeMode::ConfigActive => "config_active_background",
        crate::runtime::RuntimeMode::VoiceExclusive => "voice_exclusive_background",
        _ => "mode_background_skip",
    }
}

fn non_voice_network_tool_reason(mode: crate::runtime::RuntimeMode) -> &'static str {
    match mode {
        crate::runtime::RuntimeMode::ConfigActive => "config_active_non_voice_network_tool",
        crate::runtime::RuntimeMode::VoiceExclusive => "voice_exclusive_non_voice_network_tool",
        _ => "mode_non_voice_network_tool",
    }
}

fn non_voice_outbound_reason(mode: crate::runtime::RuntimeMode) -> &'static str {
    match mode {
        crate::runtime::RuntimeMode::ConfigActive => "config_active_non_voice_outbound",
        crate::runtime::RuntimeMode::VoiceExclusive => "voice_exclusive_non_voice_outbound",
        _ => "mode_non_voice_outbound",
    }
}

/// ESP：按「最大连续块」相对 `TLS_ADMISSION_MIN_LARGEST_BLOCK_BYTES` 的缺口缩放退避。
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
#[inline]
fn cautious_llm_retry_delay_ms(state: &OrchestratorState) -> u64 {
    let largest = state.heap_largest_block.load(Ordering::Relaxed) as u64;
    let need = TLS_ADMISSION_MIN_LARGEST_BLOCK_BYTES as u64;
    let deficit = need.saturating_sub(largest);
    cautious_llm_retry_delay_scaled(deficit, need)
}

/// Linux/host：`heap_largest_block` 为 N/A，按 `MemAvailable` 映射的 internal 相对 `TLS_ADMISSION_MIN_INTERNAL_BYTES` 缩放退避。
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
#[inline]
fn cautious_llm_retry_delay_ms(state: &OrchestratorState) -> u64 {
    let internal = state.heap_free_internal.load(Ordering::Relaxed) as u64;
    let need = TLS_ADMISSION_MIN_INTERNAL_BYTES as u64;
    let deficit = need.saturating_sub(internal);
    cautious_llm_retry_delay_scaled(deficit, need)
}

#[inline]
fn cautious_llm_retry_delay_scaled(deficit: u64, need: u64) -> u64 {
    let range = LLM_RETRY_LATER_DELAY_MS.saturating_sub(LLM_RETRY_LATER_DELAY_MS_MIN);
    if range == 0 || need == 0 {
        return LLM_RETRY_LATER_DELAY_MS;
    }
    let scaled = LLM_RETRY_LATER_DELAY_MS_MIN + range * deficit.min(need) / need;
    scaled.clamp(LLM_RETRY_LATER_DELAY_MS_MIN, LLM_RETRY_LATER_DELAY_MS)
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
#[inline]
fn cautious_outbound_defer_delay_ms(state: &OrchestratorState) -> u64 {
    let base = OUTBOUND_DEFER_DELAY_MS_CAUTIOUS;
    let total = queue_total(state) as u64;
    let threshold = (PRESSURE_QUEUE_CONGESTION_THRESHOLD as u64).max(1);
    // only called when congested; just-over-threshold uses minimum defer for better responsiveness.
    let over = total.saturating_sub(threshold);
    let scaled = OUTBOUND_DEFER_DELAY_MS_CAUTIOUS_MIN
        + (base.saturating_sub(OUTBOUND_DEFER_DELAY_MS_CAUTIOUS_MIN)) * over.min(threshold)
            / threshold;
    scaled.clamp(OUTBOUND_DEFER_DELAY_MS_CAUTIOUS_MIN, base)
}

/// agent loop 收到消息后、处理前调用。
/// Called by agent loop after receiving a message, before processing.
pub fn should_accept_inbound(
    state: &OrchestratorState,
    channel: &str,
    ingress: IngressKind,
) -> AdmissionDecision {
    should_accept_inbound_with_mode(state, channel, ingress, current_runtime_mode())
}

pub(crate) fn should_accept_inbound_with_mode(
    state: &OrchestratorState,
    channel: &str,
    ingress: IngressKind,
    mode: crate::runtime::RuntimeModeSnapshot,
) -> AdmissionDecision {
    let pressure = PressureLevel::from_byte(state.pressure_level.load(Ordering::Relaxed));
    let work_class = classify_system_work(channel, ingress);
    if let Some(block_mode) = non_voice_block_mode(mode, channel) {
        if work_class == SystemWorkClass::BackgroundLowPriority {
            return AdmissionDecision::Reject {
                reason: non_voice_background_reason(block_mode),
            };
        }
        return AdmissionDecision::Defer {
            delay_ms: MODE_TRANSITION_DEFER_MS,
        };
    }
    if !mode.action_budget.allow_periodic_maintenance
        && work_class == SystemWorkClass::BackgroundLowPriority
    {
        return AdmissionDecision::Reject {
            reason: non_voice_background_reason(mode.current_mode),
        };
    }

    match pressure {
        PressureLevel::Critical => {
            if work_class == SystemWorkClass::BackgroundLowPriority {
                return AdmissionDecision::Reject {
                    reason: "critical_pressure_background",
                };
            }
            AdmissionDecision::Defer {
                delay_ms: critical_inbound_defer_delay_ms(state),
            }
        }
        PressureLevel::Cautious => {
            match work_class {
                SystemWorkClass::BackgroundLowPriority => {
                    return AdmissionDecision::Reject {
                        reason: "cautious_background_skip",
                    };
                }
                SystemWorkClass::Maintenance if is_queue_congested(state) => {
                    return AdmissionDecision::Defer {
                        delay_ms: LOW_MEM_DEFER_SLEEP_MS_MIN,
                    };
                }
                _ => {}
            }
            AdmissionDecision::Accept
        }
        PressureLevel::Normal => AdmissionDecision::Accept,
    }
}

/// agent 准备调用 LLM 前调用。
/// Called by agent before invoking LLM.
pub fn can_call_llm(state: &OrchestratorState) -> LlmDecision {
    can_call_llm_for_channel(state, "")
}

pub fn can_call_llm_for_channel(state: &OrchestratorState, channel: &str) -> LlmDecision {
    can_call_llm_for_channel_with_mode(state, channel, current_runtime_mode())
}

pub(crate) fn can_call_llm_for_channel_with_mode(
    state: &OrchestratorState,
    channel: &str,
    mode: crate::runtime::RuntimeModeSnapshot,
) -> LlmDecision {
    if non_voice_block_mode(mode, channel).is_some() {
        return LlmDecision::RetryLater {
            delay_ms: MODE_TRANSITION_DEFER_MS,
        };
    }
    let pressure = PressureLevel::from_byte(state.pressure_level.load(Ordering::Relaxed));

    match pressure {
        PressureLevel::Critical => LlmDecision::Degrade {
            reason: "critical_pressure",
        },
        PressureLevel::Cautious => {
            #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
            {
                let largest_block = state.heap_largest_block.load(Ordering::Relaxed);
                if largest_block < TLS_ADMISSION_MIN_LARGEST_BLOCK_BYTES as u32 {
                    LlmDecision::RetryLater {
                        delay_ms: cautious_llm_retry_delay_ms(state),
                    }
                } else {
                    LlmDecision::Proceed
                }
            }
            #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
            {
                let internal = state.heap_free_internal.load(Ordering::Relaxed);
                if internal < TLS_ADMISSION_MIN_INTERNAL_BYTES as u32 {
                    LlmDecision::RetryLater {
                        delay_ms: cautious_llm_retry_delay_ms(state),
                    }
                } else {
                    LlmDecision::Proceed
                }
            }
        }
        PressureLevel::Normal => LlmDecision::Proceed,
    }
}

/// agent 准备执行工具前调用；`requires_network` 由调用方从 ToolRegistry 推导。
/// Called by agent before executing a tool; `requires_network` is derived from ToolRegistry by the caller.
pub fn can_execute_tool(
    state: &OrchestratorState,
    _tool_name: &str,
    requires_network: bool,
) -> ToolDecision {
    can_execute_tool_for_channel(state, _tool_name, requires_network, "")
}

pub fn can_execute_tool_for_channel(
    state: &OrchestratorState,
    _tool_name: &str,
    requires_network: bool,
    channel: &str,
) -> ToolDecision {
    can_execute_tool_for_channel_with_mode(
        state,
        _tool_name,
        requires_network,
        channel,
        current_runtime_mode(),
    )
}

pub(crate) fn can_execute_tool_for_channel_with_mode(
    state: &OrchestratorState,
    _tool_name: &str,
    requires_network: bool,
    channel: &str,
    mode: crate::runtime::RuntimeModeSnapshot,
) -> ToolDecision {
    if requires_network {
        if let Some(block_mode) = non_voice_block_mode(mode, channel) {
            return ToolDecision::Deny {
                reason: non_voice_network_tool_reason(block_mode),
            };
        }
    }
    let pressure = PressureLevel::from_byte(state.pressure_level.load(Ordering::Relaxed));

    match pressure {
        PressureLevel::Critical => {
            if requires_network {
                ToolDecision::Deny {
                    reason: "critical_no_network_tools",
                }
            } else {
                ToolDecision::Allow
            }
        }
        PressureLevel::Cautious => {
            if requires_network {
                let internal = state.heap_free_internal.load(Ordering::Relaxed);
                if internal < TLS_ADMISSION_MIN_INTERNAL_BYTES as u32 {
                    return ToolDecision::Deny {
                        reason: "cautious_low_heap_for_http_tool",
                    };
                }
            }
            ToolDecision::Allow
        }
        PressureLevel::Normal => ToolDecision::Allow,
    }
}

/// dispatch 发送前调用。出站消息已消耗 LLM 计算资源，优先 Defer 而非 Reject。
/// Called by dispatch before sending. Outbound messages already consumed LLM compute; prefer Defer over Reject.
pub fn should_accept_outbound(state: &OrchestratorState, _channel: &str) -> AdmissionDecision {
    should_accept_outbound_with_mode(state, _channel, current_runtime_mode())
}

pub(crate) fn should_accept_outbound_with_mode(
    state: &OrchestratorState,
    channel: &str,
    mode: crate::runtime::RuntimeModeSnapshot,
) -> AdmissionDecision {
    if let Some(block_mode) = non_voice_block_mode(mode, channel) {
        return AdmissionDecision::Reject {
            reason: non_voice_outbound_reason(block_mode),
        };
    }
    let pressure = PressureLevel::from_byte(state.pressure_level.load(Ordering::Relaxed));
    let congested = is_queue_congested(state);
    match pressure {
        PressureLevel::Critical => {
            if congested {
                AdmissionDecision::Defer {
                    delay_ms: OUTBOUND_DEFER_DELAY_MS,
                }
            } else {
                AdmissionDecision::Accept
            }
        }
        PressureLevel::Cautious => {
            // Linux Cautious 不做出站延迟——已有真实内存阈值保障，避免无意义拖慢。
            #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
            return AdmissionDecision::Accept;
            #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
            {
                if congested {
                    AdmissionDecision::Defer {
                        delay_ms: cautious_outbound_defer_delay_ms(state),
                    }
                } else {
                    AdmissionDecision::Accept
                }
            }
        }
        PressureLevel::Normal => AdmissionDecision::Accept,
    }
}

/// 出站发送线程在进入实际 HTTP 发送前的额外让行时间（毫秒）。
/// 仅用于 sender/dispatch 背景路径，避免与交互关键路径争抢 TLS 互斥。
pub fn background_outbound_yield_ms(state: &OrchestratorState) -> u64 {
    let pressure = PressureLevel::from_byte(state.pressure_level.load(Ordering::Relaxed));
    let active_agent = state.active_agent_tasks.load(Ordering::Relaxed) as u64;
    if active_agent == 0 {
        return 0;
    }
    let base = match pressure {
        PressureLevel::Normal => OUTBOUND_BACKGROUND_YIELD_MS_MIN,
        PressureLevel::Cautious => {
            (OUTBOUND_BACKGROUND_YIELD_MS_MIN + OUTBOUND_BACKGROUND_YIELD_MS_MAX) / 2
        }
        PressureLevel::Critical => OUTBOUND_BACKGROUND_YIELD_MS_MAX,
    };
    // At most 3 in-flight agent tasks are counted for additional yield.
    let extra = active_agent.min(3) * 30;
    (base + extra).min(OUTBOUND_BACKGROUND_YIELD_MS_MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bus::IngressKind;
    use crate::constants::{LLM_RETRY_LATER_DELAY_MS, TLS_ADMISSION_MIN_INTERNAL_BYTES};
    use crate::runtime::system_work::{
        CHANNEL_CRON, CHANNEL_POST_REPLY_MAINTENANCE, CHANNEL_SELF_RUNTIME,
    };

    fn state_with_heap(internal: u32, largest: u32, pressure: PressureLevel) -> OrchestratorState {
        let s = OrchestratorState::new();
        s.heap_free_internal.store(internal, Ordering::Relaxed);
        s.heap_largest_block.store(largest, Ordering::Relaxed);
        s.pressure_level.store(pressure as u8, Ordering::Relaxed);
        s
    }

    fn normal_mode() -> crate::runtime::RuntimeModeSnapshot {
        crate::runtime::mode::snapshot_from_source(crate::runtime::mode::RuntimeModeSource {
            boot_phase_active: false,
            pairing_required: false,
            pairing_state_known: false,
            voice_exclusive_active: false,
            background_maintenance_active: false,
            recovery_safe_mode_active: false,
            ..crate::runtime::mode::RuntimeModeSource::default()
        })
    }

    fn voice_exclusive_mode() -> crate::runtime::RuntimeModeSnapshot {
        crate::runtime::mode::snapshot_from_source(crate::runtime::mode::RuntimeModeSource {
            voice_exclusive_active: true,
            ..crate::runtime::mode::RuntimeModeSource::default()
        })
    }

    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
    #[test]
    fn linux_cautious_llm_proceeds_when_internal_above_min() {
        let s = state_with_heap(
            TLS_ADMISSION_MIN_INTERNAL_BYTES as u32 + 1_000_000,
            0,
            PressureLevel::Cautious,
        );
        assert!(matches!(
            can_call_llm_for_channel_with_mode(&s, "qq_channel", normal_mode()),
            LlmDecision::Proceed
        ));
    }

    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
    #[test]
    fn linux_cautious_llm_retry_when_internal_below_min() {
        let s = state_with_heap(1024, 0, PressureLevel::Cautious);
        match can_call_llm_for_channel_with_mode(&s, "qq_channel", normal_mode()) {
            LlmDecision::RetryLater { delay_ms } => {
                assert!(delay_ms >= super::LLM_RETRY_LATER_DELAY_MS_MIN);
                assert!(delay_ms <= LLM_RETRY_LATER_DELAY_MS);
            }
            other => panic!("expected RetryLater, got {:?}", other),
        }
    }

    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
    use crate::constants::TLS_ADMISSION_MIN_LARGEST_BLOCK_BYTES;

    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
    #[test]
    fn esp_cautious_llm_proceeds_when_largest_block_ok() {
        let s = state_with_heap(
            200_000,
            TLS_ADMISSION_MIN_LARGEST_BLOCK_BYTES as u32 + 1024,
            PressureLevel::Cautious,
        );
        assert!(matches!(
            can_call_llm_for_channel_with_mode(&s, "qq_channel", normal_mode()),
            LlmDecision::Proceed
        ));
    }

    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
    #[test]
    fn esp_cautious_llm_retry_when_largest_block_low() {
        let s = state_with_heap(
            200_000,
            (TLS_ADMISSION_MIN_LARGEST_BLOCK_BYTES as u32).saturating_sub(1024),
            PressureLevel::Cautious,
        );
        assert!(matches!(
            can_call_llm_for_channel_with_mode(&s, "qq_channel", normal_mode()),
            LlmDecision::RetryLater { .. }
        ));
    }

    #[test]
    fn cautious_rejects_low_priority_background_jobs() {
        let s = state_with_heap(200_000, 200_000, PressureLevel::Cautious);
        assert!(matches!(
            should_accept_inbound_with_mode(&s, CHANNEL_CRON, IngressKind::System, normal_mode()),
            AdmissionDecision::Reject {
                reason: "cautious_background_skip"
            }
        ));
    }

    #[test]
    fn cautious_defers_maintenance_when_queue_congested() {
        let s = state_with_heap(200_000, 200_000, PressureLevel::Cautious);
        s.inbound_depth
            .store(PRESSURE_QUEUE_CONGESTION_THRESHOLD, Ordering::Relaxed);
        assert!(matches!(
            should_accept_inbound_with_mode(
                &s,
                CHANNEL_SELF_RUNTIME,
                IngressKind::System,
                normal_mode()
            ),
            AdmissionDecision::Defer { .. }
        ));
        assert!(matches!(
            should_accept_inbound_with_mode(
                &s,
                CHANNEL_POST_REPLY_MAINTENANCE,
                IngressKind::System,
                normal_mode()
            ),
            AdmissionDecision::Defer { .. }
        ));
    }

    #[test]
    fn cautious_keeps_interactive_system_messages() {
        let s = state_with_heap(200_000, 200_000, PressureLevel::Cautious);
        assert!(matches!(
            should_accept_inbound_with_mode(&s, "qq_channel", IngressKind::System, normal_mode()),
            AdmissionDecision::Accept
        ));
    }

    #[test]
    fn config_active_blocks_background_but_allows_user_channel_network_work() {
        let s = state_with_heap(200_000, 200_000, PressureLevel::Normal);
        let mode =
            crate::runtime::mode::snapshot_from_source(crate::runtime::mode::RuntimeModeSource {
                config_active: true,
                config_activity_phase: crate::runtime::ConfigActivityPhase::Active,
                ..crate::runtime::mode::RuntimeModeSource::default()
            });

        assert!(matches!(
            should_accept_inbound_with_mode(&s, CHANNEL_CRON, IngressKind::System, mode),
            AdmissionDecision::Reject {
                reason: "config_active_background"
            }
        ));
        assert!(matches!(
            should_accept_outbound_with_mode(&s, "qq_channel", mode),
            AdmissionDecision::Accept
        ));
        assert!(matches!(
            can_execute_tool_for_channel_with_mode(&s, "web_fetch", true, "qq_channel", mode),
            ToolDecision::Allow
        ));
        assert!(matches!(
            can_call_llm_for_channel_with_mode(&s, "qq_channel", mode),
            LlmDecision::Proceed
        ));
    }

    #[test]
    fn config_persisting_defers_new_non_voice_network_work() {
        let s = state_with_heap(200_000, 200_000, PressureLevel::Normal);
        let mode =
            crate::runtime::mode::snapshot_from_source(crate::runtime::mode::RuntimeModeSource {
                config_active: true,
                config_activity_phase: crate::runtime::ConfigActivityPhase::Persisting,
                ..crate::runtime::mode::RuntimeModeSource::default()
            });

        assert!(matches!(
            should_accept_outbound_with_mode(&s, "qq_channel", mode),
            AdmissionDecision::Reject {
                reason: "config_active_non_voice_outbound"
            }
        ));
        assert!(matches!(
            can_call_llm_for_channel_with_mode(&s, "qq_channel", mode),
            LlmDecision::RetryLater { .. }
        ));
        assert!(matches!(
            can_execute_tool_for_channel_with_mode(&s, "web_fetch", true, "qq_channel", mode),
            ToolDecision::Deny {
                reason: "config_active_non_voice_network_tool"
            }
        ));
        assert!(matches!(
            should_accept_outbound_with_mode(&s, crate::constants::VOICE_CHANNEL_NAME, mode),
            AdmissionDecision::Accept
        ));
    }

    #[test]
    fn voice_exclusive_blocks_non_voice_outbound_at_admission() {
        let s = state_with_heap(200_000, 200_000, PressureLevel::Normal);
        let mode = voice_exclusive_mode();

        assert!(matches!(
            should_accept_outbound_with_mode(&s, "qq_channel", mode),
            AdmissionDecision::Reject {
                reason: "voice_exclusive_non_voice_outbound"
            }
        ));
        assert!(matches!(
            should_accept_outbound_with_mode(&s, crate::constants::VOICE_CHANNEL_NAME, mode),
            AdmissionDecision::Accept
        ));
    }

    #[test]
    fn voice_exclusive_defers_non_voice_inbound_at_admission() {
        let s = state_with_heap(200_000, 200_000, PressureLevel::Normal);
        let mode = voice_exclusive_mode();

        assert!(matches!(
            should_accept_inbound_with_mode(&s, "qq_channel", IngressKind::User, mode),
            AdmissionDecision::Defer { .. }
        ));
        assert!(matches!(
            should_accept_inbound_with_mode(
                &s,
                crate::constants::VOICE_CHANNEL_NAME,
                IngressKind::User,
                mode
            ),
            AdmissionDecision::Accept
        ));
    }

    #[test]
    fn voice_exclusive_gates_llm_and_network_tools_by_channel() {
        let s = state_with_heap(200_000, 200_000, PressureLevel::Normal);
        let mode = voice_exclusive_mode();

        assert!(matches!(
            can_call_llm_for_channel_with_mode(&s, crate::constants::VOICE_CHANNEL_NAME, mode),
            LlmDecision::Proceed
        ));
        assert!(matches!(
            can_call_llm_for_channel_with_mode(&s, "qq_channel", mode),
            LlmDecision::RetryLater { .. }
        ));
        assert!(matches!(
            can_execute_tool_for_channel_with_mode(&s, "web_fetch", true, "qq_channel", mode),
            ToolDecision::Deny {
                reason: "voice_exclusive_non_voice_network_tool"
            }
        ));
        assert!(matches!(
            can_execute_tool_for_channel_with_mode(&s, "memory_manage", false, "qq_channel", mode),
            ToolDecision::Allow
        ));
    }
}
