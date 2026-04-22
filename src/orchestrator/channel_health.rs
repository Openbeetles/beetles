//! 通道健康追踪：全原子无锁设计，吸收 dispatch.rs 的熔断逻辑。
//! Channel health tracking: fully atomic lock-free, absorbs dispatch.rs circuit breaker.

use crate::constants::{CHANNEL_FAIL_COOLDOWN_SECS, CHANNEL_FAIL_THRESHOLD};
use std::sync::atomic::{AtomicU32, Ordering};

use super::state::{channel_to_index, OrchestratorState};

/// 每通道健康状态——全部原子，无锁。
/// Per-channel health state — all atomic, no locks.
pub struct ChannelHealthSlot {
    pub consecutive_failures: AtomicU32,
    pub last_failure_uptime_secs: AtomicU32, // 系统启动后秒数
    pub total_failures: AtomicU32,
    pub total_successes: AtomicU32,
}

impl Default for ChannelHealthSlot {
    fn default() -> Self {
        Self::new()
    }
}

impl ChannelHealthSlot {
    pub const fn new() -> Self {
        Self {
            consecutive_failures: AtomicU32::new(0),
            last_failure_uptime_secs: AtomicU32::new(0),
            total_failures: AtomicU32::new(0),
            total_successes: AtomicU32::new(0),
        }
    }
}

/// 系统启动后经过的秒数（单调递增）。
fn uptime_secs() -> u32 {
    crate::platform::time::uptime_secs() as u32
}

/// 记录通道发送结果（成功/失败）。
/// Record channel send result (success/failure).
pub fn record_channel_result(state: &OrchestratorState, channel: &str, success: bool) {
    let idx = match channel_to_index(channel) {
        Some(i) => i,
        None => return,
    };
    let slot = &state.channel_health[idx];
    if success {
        slot.consecutive_failures.store(0, Ordering::Relaxed);
        slot.total_successes.fetch_add(1, Ordering::Relaxed);
    } else {
        slot.consecutive_failures.fetch_add(1, Ordering::Relaxed);
        slot.last_failure_uptime_secs
            .store(uptime_secs(), Ordering::Relaxed);
        slot.total_failures.fetch_add(1, Ordering::Relaxed);
    }
}

/// 通道是否健康（未处于熔断冷却中）。
/// Whether channel is healthy (not in circuit breaker cooldown).
pub fn is_channel_healthy(state: &OrchestratorState, channel: &str) -> bool {
    let idx = match channel_to_index(channel) {
        Some(i) => i,
        None => return true, // 未知通道默认健康
    };
    is_channel_healthy_by_index(state, idx)
}

#[cfg(feature = "qq_channel")]
fn qq_channel_index() -> Option<usize> {
    channel_to_index(crate::channel_capability::CHANNEL_QQ_CHANNEL)
}

fn channel_health_with_runtime_overlays(idx: usize, healthy: bool) -> bool {
    #[cfg(not(feature = "qq_channel"))]
    return {
        let _ = idx;
        healthy
    };

    #[cfg(feature = "qq_channel")]
    if qq_channel_index() == Some(idx) {
        healthy && crate::channels::is_ws_online()
    } else {
        healthy
    }
}

/// 按索引查询通道健康状态。
pub fn is_channel_healthy_by_index(state: &OrchestratorState, idx: usize) -> bool {
    let slot = &state.channel_health[idx];
    let failures = slot.consecutive_failures.load(Ordering::Relaxed);
    let healthy = if failures < CHANNEL_FAIL_THRESHOLD {
        true
    } else {
        // 冷却期已过则恢复
        let last = slot.last_failure_uptime_secs.load(Ordering::Relaxed);
        uptime_secs().saturating_sub(last) >= CHANNEL_FAIL_COOLDOWN_SECS as u32
    };
    channel_health_with_runtime_overlays(idx, healthy)
}

/// 构建单通道健康快照（用于 ResourceSnapshot 序列化）。
/// Build per-channel health snapshot for ResourceSnapshot serialization.
pub fn snapshot_by_index(
    state: &OrchestratorState,
    idx: usize,
) -> super::state::ChannelHealthSnapshot {
    let slot = &state.channel_health[idx];
    let consecutive_failures = slot.consecutive_failures.load(Ordering::Relaxed);
    let healthy = if consecutive_failures < CHANNEL_FAIL_THRESHOLD {
        true
    } else {
        let last = slot.last_failure_uptime_secs.load(Ordering::Relaxed);
        uptime_secs().saturating_sub(last) >= CHANNEL_FAIL_COOLDOWN_SECS as u32
    };
    let healthy = channel_health_with_runtime_overlays(idx, healthy);
    super::state::ChannelHealthSnapshot {
        consecutive_failures,
        total_failures: slot.total_failures.load(Ordering::Relaxed),
        total_successes: slot.total_successes.load(Ordering::Relaxed),
        healthy,
    }
}

pub fn snapshot_for_channel(
    state: &OrchestratorState,
    channel: &str,
) -> super::state::ChannelHealthSnapshot {
    match channel_to_index(channel) {
        Some(idx) => snapshot_by_index(state, idx),
        None => super::state::ChannelHealthSnapshot::healthy(),
    }
}
