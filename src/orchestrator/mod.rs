//! 统一资源编排器：单一权威状态中心，所有资源决策基于统一快照。
//! Unified resource orchestrator: single authority for all resource decisions.
//!
//! 零堆分配、零锁（除 TLS 单并发 Mutex）、xtensa 兼容（仅 AtomicU32/AtomicU8）。
//! Zero heap alloc, zero locks (except TLS single-concurrency Mutex), xtensa compatible.

pub mod admission;
pub mod channel_health;
pub mod permit;
pub mod pressure;
pub mod runtime_capability;
pub mod state;

use crate::error::Result;
use crate::platform::MemorySnapshot;
use std::sync::atomic::AtomicU32;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

pub use admission::{AdmissionDecision, LlmDecision, ToolDecision};
pub use channel_health::is_channel_healthy;
pub use permit::{AgentTaskGuard, HttpPermitGuard, HttpThreadRole, Priority, WssSessionGuard};
pub use pressure::{PressureLevel, ResourceBudget, TlsFragmentationRisk};
#[cfg(test)]
pub use runtime_capability::reset_runtime_capabilities_for_tests;
pub use runtime_capability::{
    begin_runtime_capability_draining, finish_runtime_capability_unloaded,
    format_runtime_capability_baseline_line, get_runtime_capability,
    mark_runtime_capability_disabled, mark_runtime_capability_failed,
    observe_runtime_capabilities_from_platform, observe_runtime_capability_failure,
    observe_runtime_capability_success, runtime_capability_blocker,
    runtime_capability_drain_denied_total, runtime_capability_snapshot, runtime_capability_summary,
    try_begin_runtime_capability_call, update_runtime_capability, RuntimeCapabilityBlocker,
    RuntimeCapabilityCallGuard, RuntimeCapabilityReason, RuntimeCapabilityState,
    RuntimeCapabilityStatus, RuntimeCapabilitySummary, RuntimeCapabilityUpdate,
    RUNTIME_CAPABILITY_AUDIO_INPUT, RUNTIME_CAPABILITY_AUDIO_OUTPUT,
    RUNTIME_CAPABILITY_NETWORK_OUTBOUND_HTTP, RUNTIME_CAPABILITY_STORAGE_STATE_FS,
};
pub use state::{
    CrashMetadataSnapshot, ResourceAdmissionSnapshot, ResourceDiagnosticSnapshot,
    ResourceGovernanceMetricsSnapshot, ResourceSnapshot, StorageContentionRisk,
};

/// 全局单例 orchestrator 状态。
/// Global singleton orchestrator state.
static STATE: state::OrchestratorState = state::OrchestratorState::new();
static TLS_PERMIT: Mutex<()> = Mutex::new(());
static INITIALIZED: OnceLock<()> = OnceLock::new();
/// 上次刷新堆状态的 uptime 秒数（AtomicU32 for xtensa compatibility）。
static LAST_REFRESH_SECS: AtomicU32 = AtomicU32::new(0);
/// refresh_heap_if_stale 使用的启动时刻基准。
static REFRESH_START: OnceLock<std::time::Instant> = OnceLock::new();

/// 装配期注入：`Platform::memory_snapshot` 闭包，由装配入口在 bootstrap 前注册一次。
/// Injected at assembly: `Platform::memory_snapshot` closure, registered once from the
/// assembly entry before bootstrap starts.
static MEMORY_SNAPSHOT_PROVIDER: OnceLock<Arc<dyn Fn() -> MemorySnapshot + Send + Sync>> =
    OnceLock::new();
static CRASH_METADATA_PROVIDER: OnceLock<Arc<dyn Fn() -> CrashMetadataSnapshot + Send + Sync>> =
    OnceLock::new();
static RECORDED_CRASH_METADATA: Mutex<Option<CrashMetadataSnapshot>> = Mutex::new(None);

/// 注册内存快照来源（幂等：仅首次成功）。须在首次调用 [`update_heap_state`] 之前调用。
/// Register memory snapshot source (first call wins). Must run before first [`update_heap_state`].
pub fn register_memory_snapshot_provider(f: Arc<dyn Fn() -> MemorySnapshot + Send + Sync>) {
    if MEMORY_SNAPSHOT_PROVIDER.set(f).is_err() {
        log::error!("[orchestrator] register_memory_snapshot_provider: already registered");
        debug_assert!(
            false,
            "memory snapshot provider must be registered at most once"
        );
    }
}

/// Register the platform crash metadata source.
///
/// The provider must return only facts from reset reason, coredump, or captured
/// boot evidence. Missing facts stay `None`; callers must not synthesize PCs.
pub fn register_crash_metadata_provider(f: Arc<dyn Fn() -> CrashMetadataSnapshot + Send + Sync>) {
    if CRASH_METADATA_PROVIDER.set(f).is_err() {
        log::error!("[orchestrator] register_crash_metadata_provider: already registered");
        debug_assert!(
            false,
            "crash metadata provider must be registered at most once"
        );
    }
}

/// Record crash metadata from a real evidence source.
pub fn record_crash_metadata(snapshot: CrashMetadataSnapshot) {
    if snapshot.is_empty() {
        return;
    }
    if let Ok(mut guard) = RECORDED_CRASH_METADATA.lock() {
        *guard = Some(snapshot);
    }
}

#[cfg(test)]
pub fn reset_crash_metadata_for_tests() {
    if let Ok(mut guard) = RECORDED_CRASH_METADATA.lock() {
        *guard = None;
    }
}

/// 实时取当前平台内存快照（TLS 准入、堆刷新共用）。未注册时返回零值（装配错误）。
pub(crate) fn memory_snapshot_live() -> MemorySnapshot {
    match MEMORY_SNAPSHOT_PROVIDER.get() {
        Some(provider) => provider(),
        None => {
            log::error!(
                "[orchestrator] memory snapshot provider not registered; returning zero snapshot"
            );
            MemorySnapshot {
                heap_free_internal: 0,
                heap_free_spiram: 0,
                heap_largest_block: 0,
            }
        }
    }
}

/// 将快照写入 orchestrator 并重算压力等级。
pub(crate) fn apply_memory_snapshot(snap: MemorySnapshot) {
    use std::sync::atomic::Ordering;
    STATE.update_heap(
        snap.heap_free_internal,
        snap.heap_free_spiram,
        snap.heap_largest_block,
    );
    let level = pressure::compute_pressure(&STATE);
    STATE.pressure_level.store(level as u8, Ordering::Relaxed);
}

/// main 启动时调用一次，初始化 orchestrator（幂等）。
/// Called once by main at startup (idempotent).
pub fn init() {
    INITIALIZED.get_or_init(|| {
        update_heap_state();
        log::info!(
            "[orchestrator] initialized, pressure={:?}",
            current_pressure()
        );
    });
}

/// 更新堆状态并重算压力等级。由 heartbeat 定期调用。
/// Update heap state and recompute pressure level. Called periodically by heartbeat.
pub fn update_heap_state() {
    apply_memory_snapshot(memory_snapshot_live());
}

/// 若距上次刷新 ≥2s 则重新采样堆状态并返回最新压力等级，否则返回缓存值。
/// Refresh heap state if stale (≥2s since last refresh), otherwise return cached pressure.
const REFRESH_MIN_INTERVAL_SECS: u32 = 2;

pub fn refresh_heap_if_stale() -> PressureLevel {
    let start = REFRESH_START.get_or_init(std::time::Instant::now);
    let now_secs = start.elapsed().as_secs() as u32;
    let last = LAST_REFRESH_SECS.load(std::sync::atomic::Ordering::Relaxed);
    if now_secs.wrapping_sub(last) >= REFRESH_MIN_INTERVAL_SECS {
        LAST_REFRESH_SECS.store(now_secs, std::sync::atomic::Ordering::Relaxed);
        update_heap_state();
    }
    current_pressure()
}

/// 返回当前压力等级。
pub fn current_pressure() -> PressureLevel {
    PressureLevel::from_byte(
        STATE
            .pressure_level
            .load(std::sync::atomic::Ordering::Relaxed),
    )
}

/// 返回当前压力对应的预算与策略，无锁只读。
/// Return budget for current pressure level, lock-free read-only.
pub fn current_budget() -> ResourceBudget {
    pressure::budget_for_level(current_pressure())
}

pub fn current_tls_fragmentation_risk() -> TlsFragmentationRisk {
    let snap = snapshot();
    snap.tls_fragmentation_risk
}

pub fn current_storage_contention_risk() -> StorageContentionRisk {
    snapshot().storage_contention_risk
}

/// 返回全局资源快照（无锁原子读取）。
/// Return global resource snapshot (lock-free atomic reads).
pub fn snapshot() -> ResourceSnapshot {
    state::ResourceSnapshot::from_state(&STATE)
}

/// Deep diagnostic resource snapshot for `/api/resource`.
pub fn resource_diagnostic_snapshot() -> ResourceDiagnosticSnapshot {
    state::ResourceDiagnosticSnapshot::from_state(&STATE)
}

/// 单行资源基线字符串，与 [`snapshot`] 及 `GET /api/resource` 字段一致，供心跳与串口对齐观测。
/// Single-line resource baseline aligned with [`snapshot`] and `GET /api/resource` for heartbeat/serial.
pub fn format_resource_baseline_line() -> String {
    let s = snapshot();
    #[cfg(target_os = "linux")]
    {
        let largest = if s.heap_largest_block_internal == 0 {
            "n/a".to_string()
        } else {
            s.heap_largest_block_internal.to_string()
        };
        return format!(
            "resource pressure={:?} tls_fragmentation={:?} storage_contention={:?} mem_available={} heap_spiram={} heap_largest={} active_http={} active_wss={} agent_tasks={} inbound={} outbound={}",
            s.pressure,
            s.tls_fragmentation_risk,
            s.storage_contention_risk,
            s.heap_free_internal,
            s.heap_free_spiram,
            largest,
            s.active_http_count,
            s.active_wss_count,
            s.active_agent_tasks,
            s.inbound_depth,
            s.outbound_depth,
        );
    }
    #[cfg(not(target_os = "linux"))]
    format!(
        "resource pressure={:?} tls_fragmentation={:?} storage_contention={:?} heap_internal={} heap_spiram={} heap_largest={} active_http={} active_wss={} agent_tasks={} inbound={} outbound={}",
        s.pressure,
        s.tls_fragmentation_risk,
        s.storage_contention_risk,
        s.heap_free_internal,
        s.heap_free_spiram,
        s.heap_largest_block_internal,
        s.active_http_count,
        s.active_wss_count,
        s.active_agent_tasks,
        s.inbound_depth,
        s.outbound_depth,
    )
}

/// 请求 HTTP 准入令牌。
/// Request HTTP admission permit.
pub fn request_http_permit(priority: Priority, timeout: Duration) -> Result<HttpPermitGuard> {
    permit::request_http_permit(&STATE, &TLS_PERMIT, priority, timeout)
}

/// 设置当前线程的 HTTP 准入角色，用于 TLS 准入前降噪。
pub fn set_current_http_thread_role(role: HttpThreadRole) {
    permit::set_current_http_thread_role(role);
}

/// 读取当前线程在 transport/TLS 准入中的角色。
pub fn current_http_thread_role() -> HttpThreadRole {
    permit::current_http_thread_role()
}

/// 开始一个 agent 任务：返回 RAII guard，Drop 时自动递减计数。
/// Begin an agent task: returns an RAII guard that decrements the counter on drop.
/// 应在准入通过后、开始处理消息前立即调用，确保整个任务生命周期内 `active_agent_tasks > 0`。
pub fn begin_agent_task() -> AgentTaskGuard {
    AgentTaskGuard::new(&STATE)
}

/// 标记一个已建立的 WSS 会话开始存活；返回 RAII guard，Drop 时自动递减。
pub fn begin_wss_session() -> WssSessionGuard {
    crate::network::begin_wss_session()
}

/// 记录通道发送结果（成功/失败）。
/// Record channel send result.
pub fn record_channel_result_pub(channel: &str, success: bool) {
    channel_health::record_channel_result(&STATE, channel, success);
}

/// 通道是否健康。
/// Whether channel is healthy.
pub fn is_channel_healthy_pub(channel: &str) -> bool {
    channel_health::is_channel_healthy(&STATE, channel)
}

/// 入站准入决策。
pub fn should_accept_inbound_pub(
    channel: &str,
    ingress: crate::bus::IngressKind,
) -> AdmissionDecision {
    admission::should_accept_inbound(&STATE, channel, ingress)
}

/// 更新队列深度（由 heartbeat 定期调用）。
/// Update queue depth snapshot (called periodically by heartbeat).
pub fn update_queue_depth(inbound: u32, outbound: u32) {
    STATE
        .inbound_depth
        .store(inbound, std::sync::atomic::Ordering::Relaxed);
    STATE
        .outbound_depth
        .store(outbound, std::sync::atomic::Ordering::Relaxed);
}

/// 更新会话与存储指标（由 heartbeat 定期调用）。
/// Update session & storage metrics (called periodically by heartbeat).
pub fn update_session_storage(session_count: u32, storage_used_kb: u32, storage_total_kb: u32) {
    STATE
        .session_count
        .store(session_count, std::sync::atomic::Ordering::Relaxed);
    STATE
        .storage_used_kb
        .store(storage_used_kb, std::sync::atomic::Ordering::Relaxed);
    STATE
        .storage_total_kb
        .store(storage_total_kb, std::sync::atomic::Ordering::Relaxed);
}

/// LLM 调用门控。
pub fn can_call_llm_pub() -> LlmDecision {
    admission::can_call_llm(&STATE)
}

/// LLM 调用门控，带当前请求通道上下文。
pub fn can_call_llm_for_channel_pub(channel: &str) -> LlmDecision {
    admission::can_call_llm_for_channel(&STATE, channel)
}

/// 工具执行门控。
pub fn can_execute_tool_pub(tool_name: &str, requires_network: bool) -> ToolDecision {
    admission::can_execute_tool(&STATE, tool_name, requires_network)
}

/// 工具执行门控，带当前请求通道上下文。
pub fn can_execute_tool_for_channel_pub(
    tool_name: &str,
    requires_network: bool,
    channel: &str,
) -> ToolDecision {
    admission::can_execute_tool_for_channel(&STATE, tool_name, requires_network, channel)
}

/// 出站门禁决策。
/// Outbound admission decision.
pub fn should_accept_outbound_pub(channel: &str) -> AdmissionDecision {
    admission::should_accept_outbound(&STATE, channel)
}

/// sender/dispatch 背景路径在发送前的额外让行时间（毫秒）。
pub fn background_outbound_yield_ms_pub() -> u64 {
    admission::background_outbound_yield_ms(&STATE)
}

/// 设置麦克风录音标志（voice_input 工具调用）。
/// Set microphone recording flag (called by voice_input tool).
pub fn set_audio_recording(active: bool) {
    STATE.audio_recording.store(
        if active { 1 } else { 0 },
        std::sync::atomic::Ordering::Relaxed,
    );
}

/// 麦克风是否正在录音。
/// Whether microphone is currently recording.
pub fn is_audio_recording() -> bool {
    STATE
        .audio_recording
        .load(std::sync::atomic::Ordering::Relaxed)
        != 0
}

/// 设置喇叭播放标志（voice_output 工具调用）。
/// Set speaker playing flag (called by voice_output tool).
pub fn set_audio_playing(active: bool) {
    STATE.audio_playing.store(
        if active { 1 } else { 0 },
        std::sync::atomic::Ordering::Relaxed,
    );
}

/// 喇叭是否正在播放。
/// Whether speaker is currently playing.
pub fn is_audio_playing() -> bool {
    STATE
        .audio_playing
        .load(std::sync::atomic::Ordering::Relaxed)
        != 0
}

/// 设置播放期打断监听标志。
/// Set playback-period barge-in listening flag.
pub fn set_audio_interrupt_listening(active: bool) {
    STATE.audio_interrupt_listening.store(
        if active { 1 } else { 0 },
        std::sync::atomic::Ordering::Relaxed,
    );
    if !active {
        STATE
            .audio_interrupt_requested
            .store(0, std::sync::atomic::Ordering::Relaxed);
    }
}

/// 当前是否打开了播放期打断监听。
/// Whether playback-period barge-in listening is armed.
pub fn is_audio_interrupt_listening() -> bool {
    STATE
        .audio_interrupt_listening
        .load(std::sync::atomic::Ordering::Relaxed)
        != 0
}

/// 请求当前实时语音会话执行本地打断。
/// Request a local barge-in against the active realtime session.
pub fn request_audio_interrupt() {
    STATE
        .audio_interrupt_requested
        .store(1, std::sync::atomic::Ordering::Relaxed);
}

/// 清空待处理打断请求。
/// Clear pending local barge-in request.
pub fn clear_audio_interrupt_request() {
    STATE
        .audio_interrupt_requested
        .store(0, std::sync::atomic::Ordering::Relaxed);
}

/// 读取并消费一次本地打断请求。
/// Read and consume one pending local barge-in request.
pub fn take_audio_interrupt_request() -> bool {
    STATE
        .audio_interrupt_requested
        .swap(0, std::sync::atomic::Ordering::Relaxed)
        != 0
}

/// 启动时打印 TLS 准入基线。
/// Log TLS admission baseline at startup.
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
pub fn log_baseline() {
    use crate::constants::{
        TLS_ADMISSION_MIN_INTERNAL_BYTES, TLS_ADMISSION_MIN_LARGEST_BLOCK_BYTES,
    };
    let snap = memory_snapshot_live();
    log::info!(
        "[orchestrator] TLS admission baseline: internal_free={} largest_block={} spiram_free={} min_internal={} min_largest={}",
        snap.heap_free_internal,
        snap.heap_largest_block,
        snap.heap_free_spiram,
        TLS_ADMISSION_MIN_INTERNAL_BYTES,
        TLS_ADMISSION_MIN_LARGEST_BLOCK_BYTES
    );
}

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub fn log_baseline() {}

/// 生成启动阶段 internal heap / largest block / PSRAM 的统一观测行。
/// Format a stable startup memory checkpoint line for ESP bring-up analysis.
pub fn format_startup_memory_checkpoint_line(
    stage: &str,
    snap: crate::platform::MemorySnapshot,
) -> String {
    let probe_state = state::OrchestratorState::new();
    probe_state.update_heap(
        snap.heap_free_internal,
        snap.heap_free_spiram,
        snap.heap_largest_block,
    );
    let pressure = pressure::compute_pressure(&probe_state);
    let tls_fragmentation =
        pressure::tls_fragmentation_risk(snap.heap_largest_block, snap.heap_free_spiram);
    format!(
        "[orchestrator] startup memory checkpoint stage={} internal_free={} largest_block={} spiram_free={} pressure={:?} tls_fragmentation={:?}",
        stage,
        snap.heap_free_internal,
        snap.heap_largest_block,
        snap.heap_free_spiram,
        pressure,
        tls_fragmentation
    )
}

/// 启动阶段实时打印当前内存观测点，并把快照回写到 orchestrator 单一权威状态。
/// Log a live startup memory checkpoint and update orchestrator state from the same sample.
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
pub fn log_startup_memory_checkpoint(stage: &'static str) {
    let snap = memory_snapshot_live();
    apply_memory_snapshot(snap);
    log::info!("{}", format_startup_memory_checkpoint_line(stage, snap));
}

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub fn log_startup_memory_checkpoint(_stage: &'static str) {}

#[cfg(test)]
mod tests {
    use super::format_startup_memory_checkpoint_line;
    use crate::platform::MemorySnapshot;

    #[test]
    fn startup_memory_checkpoint_line_reports_fragmentation_pressure() {
        let line = format_startup_memory_checkpoint_line(
            "after_voice_session_spawn",
            MemorySnapshot {
                heap_free_internal: 49_051,
                heap_free_spiram: 7_258_468,
                heap_largest_block: 20_480,
            },
        );

        assert!(line.contains("stage=after_voice_session_spawn"));
        assert!(line.contains("internal_free=49051"));
        assert!(line.contains("largest_block=20480"));
        assert!(line.contains("spiram_free=7258468"));
        assert!(line.contains("pressure=Critical"));
        assert!(line.contains("tls_fragmentation=Critical"));
    }
}
