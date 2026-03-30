//! 原子状态聚合：堆、socket、压力等级、通道健康，全部固定大小 + 原子变量，零堆分配。
//! Atomic state aggregation: heap, socket, pressure, channel health — fixed-size + atomics, zero heap alloc.

use std::sync::atomic::{AtomicU32, AtomicU8, Ordering};

use super::channel_health::ChannelHealthSlot;

/// 通道索引枚举，编译时确定，避免 HashMap + String 的堆分配。
/// Channel index enum, compile-time fixed, avoids HashMap + String heap allocation.
#[repr(u8)]
#[derive(Debug, Clone, Copy)]
pub enum ChannelIndex {
    Telegram = 0,
    Feishu = 1,
    DingTalk = 2,
    WeCom = 3,
    QqChannel = 4,
}

pub const MAX_CHANNELS: usize = 5;

/// 通道名 → ChannelIndex 映射（编译时已知的 5 个通道）。
/// Channel name to index mapping (5 channels known at compile time).
pub fn channel_to_index(channel: &str) -> Option<ChannelIndex> {
    match channel {
        "telegram" => Some(ChannelIndex::Telegram),
        "feishu" => Some(ChannelIndex::Feishu),
        "dingtalk" => Some(ChannelIndex::DingTalk),
        "wecom" => Some(ChannelIndex::WeCom),
        "qq_channel" => Some(ChannelIndex::QqChannel),
        _ => None,
    }
}

/// Orchestrator 全局原子状态。零堆分配，仅使用 AtomicU32/AtomicU8（xtensa 兼容）。
/// Global atomic state. Zero heap alloc, only AtomicU32/AtomicU8 (xtensa compatible).
pub struct OrchestratorState {
    // 堆状态（heartbeat 定期更新）
    pub heap_free_internal: AtomicU32,
    pub heap_free_spiram: AtomicU32,
    pub heap_largest_block: AtomicU32,
    /// internal 堆基线（首次 update_heap 时设置，用于相对使用率计算）。
    /// Internal heap baseline (set on first update_heap, used for relative usage calculation).
    pub heap_baseline_internal: AtomicU32,

    // 连接计数（permit acquire/release 时增减）
    pub active_http_count: AtomicU32,

    /// Agent 正在处理的任务数（AgentTaskGuard 持有期间非零）。
    /// Number of agent tasks currently in flight (non-zero while AgentTaskGuard is held).
    pub active_agent_tasks: AtomicU32,

    // 压力等级（由 update_heap_state 计算写入）
    pub pressure_level: AtomicU8,

    // 通道健康（channel_health.rs 管理）—— 固定大小数组，无堆分配
    pub channel_health: [ChannelHealthSlot; MAX_CHANNELS],

    // 队列深度（heartbeat 定期更新）
    pub inbound_depth: AtomicU32,
    pub outbound_depth: AtomicU32,

    // 会话与存储指标（heartbeat 定期更新）
    // Session & storage metrics (updated periodically by heartbeat)
    pub session_count: AtomicU32,
    pub storage_used_kb: AtomicU32,
    pub storage_total_kb: AtomicU32,

    /// 麦克风录音中标志（0=idle, 1=recording）。voice_input 工具设置，显示循环读取。
    /// Microphone recording flag (0=idle, 1=recording). Set by voice_input tool, read by display loop.
    pub audio_recording: AtomicU8,
    /// 喇叭播放中标志（0=idle, 1=playing）。voice_output 工具设置，显示循环读取。
    /// Speaker playing flag (0=idle, 1=playing). Set by voice_output tool, read by display loop.
    pub audio_playing: AtomicU8,
}

impl Default for OrchestratorState {
    fn default() -> Self {
        Self::new()
    }
}

impl OrchestratorState {
    pub const fn new() -> Self {
        Self {
            heap_free_internal: AtomicU32::new(u32::MAX),
            heap_free_spiram: AtomicU32::new(0),
            heap_largest_block: AtomicU32::new(u32::MAX),
            heap_baseline_internal: AtomicU32::new(0),
            active_http_count: AtomicU32::new(0),
            active_agent_tasks: AtomicU32::new(0),
            pressure_level: AtomicU8::new(0), // PressureLevel::Normal
            channel_health: [
                ChannelHealthSlot::new(),
                ChannelHealthSlot::new(),
                ChannelHealthSlot::new(),
                ChannelHealthSlot::new(),
                ChannelHealthSlot::new(),
            ],
            inbound_depth: AtomicU32::new(0),
            outbound_depth: AtomicU32::new(0),
            session_count: AtomicU32::new(0),
            storage_used_kb: AtomicU32::new(0),
            storage_total_kb: AtomicU32::new(0),
            audio_recording: AtomicU8::new(0),
            audio_playing: AtomicU8::new(0),
        }
    }

    /// 更新堆状态（由 heartbeat / update_heap_state 调用）。
    /// 首次调用时设置 baseline，后续若空闲增加则更新 baseline（避免负使用率）。
    pub fn update_heap(&self, internal: u32, spiram: u32, largest_block: u32) {
        let baseline = self.heap_baseline_internal.load(Ordering::Relaxed);
        if baseline == 0 {
            self.heap_baseline_internal.store(internal, Ordering::Relaxed);
        } else if internal > baseline {
            self.heap_baseline_internal.store(internal, Ordering::Relaxed);
        }
        self.heap_free_internal.store(internal, Ordering::Relaxed);
        self.heap_free_spiram.store(spiram, Ordering::Relaxed);
        self.heap_largest_block
            .store(largest_block, Ordering::Relaxed);
    }
}

/// 单通道健康快照（用于 API 序列化）。
/// Per-channel health snapshot for API serialization.
#[derive(serde::Serialize)]
pub struct ChannelHealthSnapshot {
    pub consecutive_failures: u32,
    pub total_failures: u32,
    pub total_successes: u32,
    pub healthy: bool,
}

/// 全部通道健康快照（具名结构，API 输出更易读）。
/// All channels health snapshot (named struct for readable API output).
#[derive(serde::Serialize)]
pub struct ChannelsHealthSnapshot {
    pub telegram: ChannelHealthSnapshot,
    pub feishu: ChannelHealthSnapshot,
    pub dingtalk: ChannelHealthSnapshot,
    pub wecom: ChannelHealthSnapshot,
    pub qq_channel: ChannelHealthSnapshot,
}

/// 全局资源快照（无锁原子读取）。
/// Global resource snapshot (lock-free atomic reads).
#[derive(serde::Serialize)]
pub struct ResourceSnapshot {
    pub pressure: super::pressure::PressureLevel,
    pub heap_free_internal: u32,
    pub heap_free_spiram: u32,
    /// internal 堆最大连续空闲块（字节）；ESP 上用于 TLS 碎片门禁。Linux 上为 **0（N/A）**，与 `MemAvailable` 映射的 `heap_free_internal` 分开表述。
    pub heap_largest_block_internal: u32,
    pub active_http_count: u32,
    /// Agent 当前处理中的任务数（0 表示空闲）。
    pub active_agent_tasks: u32,
    pub inbound_depth: u32,
    pub outbound_depth: u32,
    pub budget: super::pressure::ResourceBudget,
    pub channels: ChannelsHealthSnapshot,
    pub session_count: u32,
    pub storage_used_kb: u32,
    pub storage_total_kb: u32,
    /// 麦克风是否正在录音。
    pub audio_recording: bool,
    /// 喇叭是否正在播放。
    pub audio_playing: bool,
    /// Linux 特有：CPU 使用率（百分比）
    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
    pub cpu_usage_percent: f32,
    /// Linux 特有：系统负载（1/5/15分钟）
    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
    pub load_average: (f32, f32, f32),
    /// Linux 特有：进程内存使用（KB）
    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
    pub process_memory_kb: u32,
}

impl ResourceSnapshot {
    pub fn from_state(state: &OrchestratorState) -> Self {
        let pressure =
            super::pressure::PressureLevel::from_byte(state.pressure_level.load(Ordering::Relaxed));
        let channels = ChannelsHealthSnapshot {
            telegram: super::channel_health::snapshot_by_index(
                state,
                ChannelIndex::Telegram as usize,
            ),
            feishu: super::channel_health::snapshot_by_index(state, ChannelIndex::Feishu as usize),
            dingtalk: super::channel_health::snapshot_by_index(
                state,
                ChannelIndex::DingTalk as usize,
            ),
            wecom: super::channel_health::snapshot_by_index(state, ChannelIndex::WeCom as usize),
            qq_channel: super::channel_health::snapshot_by_index(
                state,
                ChannelIndex::QqChannel as usize,
            ),
        };
        Self {
            pressure,
            heap_free_internal: state.heap_free_internal.load(Ordering::Relaxed),
            heap_free_spiram: state.heap_free_spiram.load(Ordering::Relaxed),
            heap_largest_block_internal: state.heap_largest_block.load(Ordering::Relaxed),
            active_http_count: state.active_http_count.load(Ordering::Relaxed),
            active_agent_tasks: state.active_agent_tasks.load(Ordering::Relaxed),
            inbound_depth: state.inbound_depth.load(Ordering::Relaxed),
            outbound_depth: state.outbound_depth.load(Ordering::Relaxed),
            budget: super::pressure::budget_for_level(pressure),
            channels,
            session_count: state.session_count.load(Ordering::Relaxed),
            storage_used_kb: state.storage_used_kb.load(Ordering::Relaxed),
            storage_total_kb: state.storage_total_kb.load(Ordering::Relaxed),
            audio_recording: state.audio_recording.load(Ordering::Relaxed) != 0,
            audio_playing: state.audio_playing.load(Ordering::Relaxed) != 0,
            #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
            cpu_usage_percent: get_cpu_usage(),
            #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
            load_average: get_load_average(),
            #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
            process_memory_kb: get_process_memory_kb(),
        }
    }
}

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
fn get_cpu_usage() -> f32 {
    // 简化实现：返回0，后续可以通过读取 /proc/stat 实现
    0.0
}

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
fn get_load_average() -> (f32, f32, f32) {
    use std::fs;
    if let Ok(content) = fs::read_to_string("/proc/loadavg") {
        let parts: Vec<&str> = content.split_whitespace().collect();
        if parts.len() >= 3 {
            let load1 = parts[0].parse::<f32>().unwrap_or(0.0);
            let load5 = parts[1].parse::<f32>().unwrap_or(0.0);
            let load15 = parts[2].parse::<f32>().unwrap_or(0.0);
            return (load1, load5, load15);
        }
    }
    (0.0, 0.0, 0.0)
}

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
fn get_process_memory_kb() -> u32 {
    use std::fs;
    if let Ok(content) = fs::read_to_string("/proc/self/status") {
        for line in content.lines() {
            if line.starts_with("VmRSS:") {
                if let Some(kb_str) = line.split_whitespace().nth(1) {
                    return kb_str.parse::<u32>().unwrap_or(0);
                }
            }
        }
    }
    0
}
