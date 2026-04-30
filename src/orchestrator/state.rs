//! 原子状态聚合：堆、socket、压力等级、通道健康，全部固定大小 + 原子变量，零堆分配。
//! Atomic state aggregation: heap, socket, pressure, channel health — fixed-size + atomics, zero heap alloc.

use std::sync::atomic::{AtomicU32, AtomicU8, Ordering};
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
use std::sync::{Mutex, OnceLock};

use super::channel_health::ChannelHealthSlot;
pub const MAX_CHANNELS: usize = 5;

/// 通道健康槽位仍保持固定 5 个，避免原子状态面引入动态分配；
/// 但槽位索引不再由另一份硬编码枚举维护，而是直接来自编译期通道目录。
/// Channel health keeps a fixed 5-slot atomic array, while slot lookup now derives from the
/// compile-time channel catalog instead of a second hard-coded enum.
pub fn channel_to_index(channel: &str) -> Option<usize> {
    crate::channel_catalog::connectivity_channel_entries()
        .enumerate()
        .find_map(|(index, entry)| (entry.id == channel).then_some(index))
}

/// Orchestrator 全局原子状态。零堆分配，仅使用 AtomicU32/AtomicU8（xtensa 兼容）。
/// Global atomic state. Zero heap alloc, only AtomicU32/AtomicU8 (xtensa compatible).
pub struct OrchestratorState {
    // 堆状态（heartbeat 定期更新）
    pub heap_free_internal: AtomicU32,
    pub heap_min_free_internal: AtomicU32,
    pub heap_free_spiram: AtomicU32,
    pub heap_total_spiram: AtomicU32,
    pub heap_min_free_spiram: AtomicU32,
    pub heap_largest_block_spiram: AtomicU32,
    pub heap_largest_block: AtomicU32,
    /// internal 堆基线（首次 update_heap 时设置，用于相对使用率计算）。
    /// Internal heap baseline (set on first update_heap, used for relative usage calculation).
    pub heap_baseline_internal: AtomicU32,

    // 连接计数
    pub active_http_count: AtomicU32,
    /// 已建立的长期 WSS 会话数（握手完成后持有，断开即释放）。
    /// Number of established long-lived WSS sessions (held after handshake completes).
    pub active_wss_count: AtomicU32,

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
    /// 播放期是否保持打断监听（0=off, 1=on）。
    /// Whether playback-period barge-in listening is armed (0=off, 1=on).
    pub audio_interrupt_listening: AtomicU8,
    /// 当前是否有待消费的本地打断请求（0=none, 1=pending）。
    /// Whether a local barge-in request is pending (0=none, 1=pending).
    pub audio_interrupt_requested: AtomicU8,
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
            heap_min_free_internal: AtomicU32::new(0),
            heap_free_spiram: AtomicU32::new(0),
            heap_total_spiram: AtomicU32::new(0),
            heap_min_free_spiram: AtomicU32::new(0),
            heap_largest_block_spiram: AtomicU32::new(0),
            heap_largest_block: AtomicU32::new(u32::MAX),
            heap_baseline_internal: AtomicU32::new(0),
            active_http_count: AtomicU32::new(0),
            active_wss_count: AtomicU32::new(0),
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
            audio_interrupt_listening: AtomicU8::new(0),
            audio_interrupt_requested: AtomicU8::new(0),
        }
    }

    /// 更新堆状态（由 heartbeat / update_heap_state 调用）。
    /// 首次调用时设置 baseline，后续若空闲增加则更新 baseline（避免负使用率）。
    pub fn update_heap(&self, internal: u32, spiram: u32, largest_block: u32) {
        let baseline = self.heap_baseline_internal.load(Ordering::Relaxed);
        if baseline == 0 || internal > baseline {
            self.heap_baseline_internal
                .store(internal, Ordering::Relaxed);
        }
        self.heap_free_internal.store(internal, Ordering::Relaxed);
        self.heap_free_spiram.store(spiram, Ordering::Relaxed);
        self.heap_largest_block
            .store(largest_block, Ordering::Relaxed);
    }

    /// 写入完整平台内存快照，保留 `update_heap` 的压力基线语义。
    pub fn update_memory_snapshot(&self, snap: crate::platform::MemorySnapshot) {
        self.update_heap(
            snap.heap_free_internal,
            snap.heap_free_spiram,
            snap.heap_largest_block,
        );
        self.heap_min_free_internal
            .store(snap.heap_min_free_internal, Ordering::Relaxed);
        self.heap_total_spiram
            .store(snap.heap_total_spiram, Ordering::Relaxed);
        self.heap_min_free_spiram
            .store(snap.heap_min_free_spiram, Ordering::Relaxed);
        self.heap_largest_block_spiram
            .store(snap.heap_largest_block_spiram, Ordering::Relaxed);
    }
}

/// 单通道健康快照（用于 API 序列化）。
/// Per-channel health snapshot for API serialization.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub struct ChannelHealthSnapshot {
    pub consecutive_failures: u32,
    pub total_failures: u32,
    pub total_successes: u32,
    pub healthy: bool,
}

impl ChannelHealthSnapshot {
    pub const fn healthy() -> Self {
        Self {
            consecutive_failures: 0,
            total_failures: 0,
            total_successes: 0,
            healthy: true,
        }
    }
}

impl Default for ChannelHealthSnapshot {
    fn default() -> Self {
        Self::healthy()
    }
}

/// 全部通道健康快照（具名结构，API 输出更易读）。
/// All channels health snapshot (named struct for readable API output).
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct ChannelsHealthSnapshot {
    #[cfg(feature = "telegram")]
    pub telegram: ChannelHealthSnapshot,
    #[cfg(feature = "feishu")]
    pub feishu: ChannelHealthSnapshot,
    #[cfg(feature = "dingtalk")]
    pub dingtalk: ChannelHealthSnapshot,
    #[cfg(feature = "wecom")]
    pub wecom: ChannelHealthSnapshot,
    #[cfg(feature = "qq_channel")]
    pub qq_channel: ChannelHealthSnapshot,
}

impl ChannelsHealthSnapshot {
    pub const fn all(value: ChannelHealthSnapshot) -> Self {
        #[cfg(not(any(
            feature = "telegram",
            feature = "feishu",
            feature = "dingtalk",
            feature = "wecom",
            feature = "qq_channel"
        )))]
        let _ = value;
        Self {
            #[cfg(feature = "telegram")]
            telegram: value,
            #[cfg(feature = "feishu")]
            feishu: value,
            #[cfg(feature = "dingtalk")]
            dingtalk: value,
            #[cfg(feature = "wecom")]
            wecom: value,
            #[cfg(feature = "qq_channel")]
            qq_channel: value,
        }
    }

    pub fn get(&self, channel: &str) -> Option<&ChannelHealthSnapshot> {
        match channel {
            #[cfg(feature = "telegram")]
            crate::channel_capability::CHANNEL_TELEGRAM => Some(&self.telegram),
            #[cfg(feature = "feishu")]
            crate::channel_capability::CHANNEL_FEISHU => Some(&self.feishu),
            #[cfg(feature = "dingtalk")]
            crate::channel_capability::CHANNEL_DINGTALK => Some(&self.dingtalk),
            #[cfg(feature = "wecom")]
            crate::channel_capability::CHANNEL_WECOM => Some(&self.wecom),
            #[cfg(feature = "qq_channel")]
            crate::channel_capability::CHANNEL_QQ_CHANNEL => Some(&self.qq_channel),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageContentionRisk {
    Healthy,
    Cautious,
    Critical,
}

const STORAGE_CONTENTION_SAMPLE_TTL_MS: u64 = 10_000;

fn storage_contention_risk_from_metrics(
    metrics: &crate::metrics::MetricsSnapshot,
) -> StorageContentionRisk {
    if metrics.spiffs_lock_ops_total == 0
        || metrics.spiffs_lock_hold_last_stage.is_empty()
        || metrics.spiffs_lock_last_age_ms > STORAGE_CONTENTION_SAMPLE_TTL_MS
    {
        return StorageContentionRisk::Healthy;
    }
    if metrics.spiffs_lock_hold_last_us >= 1_000_000 || metrics.spiffs_lock_wait_last_us >= 50_000 {
        StorageContentionRisk::Critical
    } else if metrics.spiffs_lock_hold_last_us >= 200_000
        || metrics.spiffs_lock_wait_last_us >= 5_000
    {
        StorageContentionRisk::Cautious
    } else {
        StorageContentionRisk::Healthy
    }
}

/// 全局资源快照（无锁原子读取）。
/// Global resource snapshot (lock-free atomic reads).
#[derive(serde::Serialize)]
pub struct ResourceSnapshot {
    pub pressure: super::pressure::PressureLevel,
    pub tls_fragmentation_risk: super::pressure::TlsFragmentationRisk,
    pub storage_contention_risk: StorageContentionRisk,
    pub heap_free_internal: u32,
    pub heap_min_free_internal: u32,
    pub heap_free_spiram: u32,
    pub heap_total_spiram: u32,
    pub heap_min_free_spiram: u32,
    pub heap_largest_block_spiram: u32,
    /// 估算 PSRAM 已用字节数：`heap_total_spiram - heap_free_spiram`，仅用于观测判读。
    pub heap_used_spiram_est: u32,
    /// internal 堆最大连续空闲块（字节）；ESP 上用于 TLS 碎片门禁。Linux 上为 **0（N/A）**，与 `MemAvailable` 映射的 `heap_free_internal` 分开表述。
    pub heap_largest_block_internal: u32,
    pub active_http_count: u32,
    pub active_wss_count: u32,
    /// Agent 当前处理中的任务数（0 表示空闲）。
    pub active_agent_tasks: u32,
    pub inbound_depth: u32,
    pub outbound_depth: u32,
    pub budget: super::pressure::ResourceBudget,
    pub channels: ChannelsHealthSnapshot,
    pub leases: crate::runtime::LeaseSnapshot,
    pub session_count: u32,
    pub storage_used_kb: u32,
    pub storage_total_kb: u32,
    /// 麦克风是否正在录音。
    pub audio_recording: bool,
    /// 喇叭是否正在播放。
    pub audio_playing: bool,
    /// 播放期本地打断监听是否已打开。
    pub audio_interrupt_listening: bool,
    /// 是否存在待处理的本地打断请求。
    pub audio_interrupt_requested: bool,
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

/// Admission counters and last-latency facts folded into the resource diagnostic snapshot.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct ResourceAdmissionSnapshot {
    pub http_permit_wait_last_ms: u64,
    pub http_route_queue_wait_last_ms: u64,
    pub http_route_handler_last_ms: u64,
    pub http_route_timeout_total: u64,
}

/// Runtime governance counters exposed through `/api/resource`.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct ResourceGovernanceMetricsSnapshot {
    pub runtime_spawn_failure_total: u64,
    pub http_route_reject_total: u64,
    pub lease_conflict_total: u64,
    pub lease_expired_replacement_total: u64,
    pub plane_drain_timeout_total: u64,
    pub inbound_queue_full_total: u64,
    pub inbound_defer_total: u64,
    pub inbound_drop_total: u64,
    pub event_ingress_enqueued_total: u64,
    pub event_ingress_rejected_total: u64,
    pub event_ingress_purged_total: u64,
    pub event_ingress_cancelled_total: u64,
    pub event_ingress_stale_drop_total: u64,
}

/// Last crash metadata for operator and diagnostic explanation surfaces.
///
/// These fields remain `None` until a real panic/coredump evidence source has provided data;
/// callers must not synthesize fake PCs or reasons.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub struct CrashMetadataSnapshot {
    pub last_panic_pc: Option<String>,
    pub last_panic_core: Option<u32>,
    pub last_panic_reason: Option<String>,
    pub last_symbolize_hint: Option<String>,
    pub last_resource_baseline_before_panic: Option<String>,
}

/// Deep resource diagnostic snapshot. This is the single aggregation point for `/api/resource`.
#[derive(serde::Serialize)]
pub struct ResourceDiagnosticSnapshot {
    pub resource: ResourceSnapshot,
    pub admission: ResourceAdmissionSnapshot,
    pub governance_metrics: ResourceGovernanceMetricsSnapshot,
    pub runtime_capabilities: Vec<crate::orchestrator::RuntimeCapabilityState>,
    pub crash: CrashMetadataSnapshot,
    pub planes: crate::runtime::PlaneRegistrySnapshot,
    pub plane_lifecycle: crate::runtime::PlaneLifecycleSnapshot,
    pub leases: crate::runtime::LeaseSnapshot,
    pub threads: crate::runtime::thread_registry::ThreadRegistrySnapshot,
    pub display_lease_denied_total: u64,
    pub write_back: crate::runtime::write_back::WriteBackSnapshot,
}

impl ResourceSnapshot {
    pub fn from_state(state: &OrchestratorState) -> Self {
        let pressure =
            super::pressure::PressureLevel::from_byte(state.pressure_level.load(Ordering::Relaxed));
        let metrics = crate::metrics::snapshot();
        let channels = ChannelsHealthSnapshot {
            #[cfg(feature = "telegram")]
            telegram: super::channel_health::snapshot_for_channel(
                state,
                crate::channel_capability::CHANNEL_TELEGRAM,
            ),
            #[cfg(feature = "feishu")]
            feishu: super::channel_health::snapshot_for_channel(
                state,
                crate::channel_capability::CHANNEL_FEISHU,
            ),
            #[cfg(feature = "dingtalk")]
            dingtalk: super::channel_health::snapshot_for_channel(
                state,
                crate::channel_capability::CHANNEL_DINGTALK,
            ),
            #[cfg(feature = "wecom")]
            wecom: super::channel_health::snapshot_for_channel(
                state,
                crate::channel_capability::CHANNEL_WECOM,
            ),
            #[cfg(feature = "qq_channel")]
            qq_channel: super::channel_health::snapshot_for_channel(
                state,
                crate::channel_capability::CHANNEL_QQ_CHANNEL,
            ),
        };
        Self {
            pressure,
            tls_fragmentation_risk: super::pressure::tls_fragmentation_risk(
                state.heap_largest_block.load(Ordering::Relaxed),
                state.heap_free_spiram.load(Ordering::Relaxed),
            ),
            storage_contention_risk: storage_contention_risk_from_metrics(&metrics),
            heap_free_internal: state.heap_free_internal.load(Ordering::Relaxed),
            heap_min_free_internal: state.heap_min_free_internal.load(Ordering::Relaxed),
            heap_free_spiram: state.heap_free_spiram.load(Ordering::Relaxed),
            heap_total_spiram: state.heap_total_spiram.load(Ordering::Relaxed),
            heap_min_free_spiram: state.heap_min_free_spiram.load(Ordering::Relaxed),
            heap_largest_block_spiram: state.heap_largest_block_spiram.load(Ordering::Relaxed),
            heap_used_spiram_est: state
                .heap_total_spiram
                .load(Ordering::Relaxed)
                .saturating_sub(state.heap_free_spiram.load(Ordering::Relaxed)),
            heap_largest_block_internal: state.heap_largest_block.load(Ordering::Relaxed),
            active_http_count: crate::network::active_http_count(),
            active_wss_count: crate::network::active_wss_count(),
            active_agent_tasks: state.active_agent_tasks.load(Ordering::Relaxed),
            inbound_depth: state.inbound_depth.load(Ordering::Relaxed),
            outbound_depth: state.outbound_depth.load(Ordering::Relaxed),
            budget: super::pressure::budget_for_level(pressure),
            channels,
            leases: crate::runtime::lease::snapshot(),
            session_count: state.session_count.load(Ordering::Relaxed),
            storage_used_kb: state.storage_used_kb.load(Ordering::Relaxed),
            storage_total_kb: state.storage_total_kb.load(Ordering::Relaxed),
            audio_recording: state.audio_recording.load(Ordering::Relaxed) != 0,
            audio_playing: state.audio_playing.load(Ordering::Relaxed) != 0,
            audio_interrupt_listening: state.audio_interrupt_listening.load(Ordering::Relaxed) != 0,
            audio_interrupt_requested: state.audio_interrupt_requested.load(Ordering::Relaxed) != 0,
            #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
            cpu_usage_percent: get_cpu_usage(),
            #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
            load_average: get_load_average(),
            #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
            process_memory_kb: get_process_memory_kb(),
        }
    }
}

impl CrashMetadataSnapshot {
    pub fn is_empty(&self) -> bool {
        self.last_panic_pc.is_none()
            && self.last_panic_core.is_none()
            && self.last_panic_reason.is_none()
            && self.last_symbolize_hint.is_none()
            && self.last_resource_baseline_before_panic.is_none()
    }

    pub fn merge_prefer_self(mut self, fallback: Self) -> Self {
        self.last_panic_pc = self.last_panic_pc.or(fallback.last_panic_pc);
        self.last_panic_core = self.last_panic_core.or(fallback.last_panic_core);
        self.last_panic_reason = self.last_panic_reason.or(fallback.last_panic_reason);
        self.last_symbolize_hint = self.last_symbolize_hint.or(fallback.last_symbolize_hint);
        self.last_resource_baseline_before_panic = self
            .last_resource_baseline_before_panic
            .or(fallback.last_resource_baseline_before_panic);
        self
    }
}

impl ResourceDiagnosticSnapshot {
    pub fn from_state(state: &OrchestratorState) -> Self {
        let resource = ResourceSnapshot::from_state(state);
        let metrics = crate::metrics::snapshot();
        let leases = resource.leases.clone();
        Self {
            admission: ResourceAdmissionSnapshot {
                http_permit_wait_last_ms: metrics.http_permit_wait_last_ms,
                http_route_queue_wait_last_ms: metrics.http_route_queue_wait_last_ms,
                http_route_handler_last_ms: metrics.http_route_handler_last_ms,
                http_route_timeout_total: metrics.http_route_timeout_total,
            },
            governance_metrics: ResourceGovernanceMetricsSnapshot {
                runtime_spawn_failure_total: metrics.runtime_spawn_failure_total,
                http_route_reject_total: metrics.http_route_reject_total,
                lease_conflict_total: metrics.lease_conflict_total,
                lease_expired_replacement_total: metrics.lease_expired_replacement_total,
                plane_drain_timeout_total: metrics.plane_drain_timeout_total,
                inbound_queue_full_total: metrics.inbound_queue_full_total,
                inbound_defer_total: metrics.inbound_defer_total,
                inbound_drop_total: metrics.inbound_drop_total,
                event_ingress_enqueued_total: metrics.event_ingress_enqueued_total,
                event_ingress_rejected_total: metrics.event_ingress_rejected_total,
                event_ingress_purged_total: metrics.event_ingress_purged_total,
                event_ingress_cancelled_total: metrics.event_ingress_cancelled_total,
                event_ingress_stale_drop_total: metrics.event_ingress_stale_drop_total,
            },
            runtime_capabilities: crate::orchestrator::runtime_capability_snapshot(),
            crash: crate::orchestrator::crash_metadata_snapshot(),
            planes: crate::runtime::plane::snapshot(),
            plane_lifecycle: crate::runtime::plane_lifecycle::snapshot(),
            leases,
            threads: crate::runtime::thread_registry::snapshot(),
            display_lease_denied_total: crate::display::display_lease_denied_total(),
            write_back: crate::runtime::write_back::snapshot(),
            resource,
        }
    }
}

/// 上次 /proc/stat 采样的总 tick 数与 idle tick 数（Linux delta CPU 采样）。
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
struct CpuSample {
    total: u64,
    idle: u64,
}

/// 读取 /proc/stat 第一行（`cpu  ...`）并返回 (total, idle) tick 对；
/// idle 包含 iowait，与主流工具（top、vmstat）语义一致。
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
fn read_cpu_stat() -> Option<CpuSample> {
    let s = std::fs::read_to_string("/proc/stat").ok()?;
    for line in s.lines() {
        if !line.starts_with("cpu ") {
            continue;
        }
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 5 {
            break;
        }
        let user = parts[1].parse::<u64>().unwrap_or(0);
        let nice = parts[2].parse::<u64>().unwrap_or(0);
        let system = parts[3].parse::<u64>().unwrap_or(0);
        let idle = parts[4].parse::<u64>().unwrap_or(0);
        let iowait = parts
            .get(5)
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0);
        let irq = parts
            .get(6)
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0);
        let softirq = parts
            .get(7)
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0);
        let steal = parts
            .get(8)
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0);
        let total = user + nice + system + idle + iowait + irq + softirq + steal;
        return Some(CpuSample {
            total,
            idle: idle + iowait,
        });
    }
    None
}

/// 上次读取的 CPU 样本，用于 delta 计算。`None` 表示尚无前一次采样（首次调用返回 0.0）。
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
static CPU_PREV: OnceLock<Mutex<Option<CpuSample>>> = OnceLock::new();

/// 基于两次 /proc/stat 采样计算 CPU 使用率（%）。
/// 首次调用存储基线快照并返回 0.0（无区间可比），之后返回区间利用率。
/// 与单次瞬时测量（`(total-idle)/total` 累计量）不同，此为真实区间 CPU 使用率。
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub(super) fn read_cpu_usage_percent() -> f32 {
    let prev_lock = CPU_PREV.get_or_init(|| Mutex::new(None));
    let Ok(mut prev_guard) = prev_lock.lock() else {
        return 0.0;
    };
    let Some(curr) = read_cpu_stat() else {
        return 0.0;
    };
    let result = if let Some(ref prev) = *prev_guard {
        let delta_total = curr.total.saturating_sub(prev.total);
        let delta_idle = curr.idle.saturating_sub(prev.idle);
        if delta_total > 0 {
            (delta_total.saturating_sub(delta_idle) as f32 / delta_total as f32) * 100.0
        } else {
            0.0
        }
    } else {
        0.0
    };
    *prev_guard = Some(curr);
    result
}

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
fn get_cpu_usage() -> f32 {
    read_cpu_usage_percent()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn storage_contention_risk_thresholds_round_trip() {
        let mut metrics = crate::metrics::snapshot();
        metrics.spiffs_lock_ops_total = 1;
        metrics.spiffs_lock_last_age_ms = 0;
        metrics.spiffs_lock_wait_last_us = 0;
        metrics.spiffs_lock_hold_last_us = 0;
        metrics.spiffs_lock_hold_last_stage.clear();
        assert_eq!(
            storage_contention_risk_from_metrics(&metrics),
            StorageContentionRisk::Healthy
        );

        metrics.spiffs_lock_wait_last_us = 7_500;
        assert_eq!(
            storage_contention_risk_from_metrics(&metrics),
            StorageContentionRisk::Healthy,
            "wait-only samples without a completed hold stage must not look fresh"
        );

        metrics.spiffs_lock_hold_last_stage = "spiffs_write_json".to_string();
        assert_eq!(
            storage_contention_risk_from_metrics(&metrics),
            StorageContentionRisk::Cautious
        );

        metrics.spiffs_lock_wait_last_us = 60_000;
        assert_eq!(
            storage_contention_risk_from_metrics(&metrics),
            StorageContentionRisk::Critical
        );

        metrics.spiffs_lock_last_age_ms = STORAGE_CONTENTION_SAMPLE_TTL_MS + 1;
        assert_eq!(
            storage_contention_risk_from_metrics(&metrics),
            StorageContentionRisk::Healthy
        );
    }

    #[test]
    fn resource_diagnostic_snapshot_includes_recorded_crash_metadata() {
        crate::orchestrator::reset_crash_metadata_for_tests();
        crate::orchestrator::record_crash_metadata(CrashMetadataSnapshot {
            last_panic_pc: Some("0x40380a45".to_string()),
            last_panic_core: Some(1),
            last_panic_reason: Some("LoadProhibited".to_string()),
            last_symbolize_hint: Some("scripts/esp_symbolize_panic.sh app 0x40380a45".to_string()),
            last_resource_baseline_before_panic: Some(
                "resource pressure=Critical heap_largest=24576".to_string(),
            ),
        });

        let state = OrchestratorState::new();
        let snapshot = ResourceDiagnosticSnapshot::from_state(&state);

        assert_eq!(snapshot.crash.last_panic_pc.as_deref(), Some("0x40380a45"));
        assert_eq!(snapshot.crash.last_panic_core, Some(1));
        assert_eq!(
            snapshot.crash.last_panic_reason.as_deref(),
            Some("LoadProhibited")
        );
        assert!(snapshot.crash.last_symbolize_hint.is_some());
        assert!(snapshot.crash.last_resource_baseline_before_panic.is_some());
        crate::orchestrator::reset_crash_metadata_for_tests();
    }
}
