//! 合并 cron + heartbeat + remind 为单线程后台定时器。
//! Merged background timer: cron, heartbeat, and remind in one thread to save ~20KB SRAM.
//!
//! Tick 间隔 10s，分频：heartbeat 每 3 tick (30s)，cron/remind 每 6 tick (60s)。

use crate::bus::SystemInboundTx;
use crate::cron::{CronTickState, SensorWatchContext};
use crate::heartbeat::HeartbeatTickState;
use crate::i18n::Locale;
use crate::memory::{
    AutonomyStrategyStore, MemoryProfile, MemoryStore, RemindAtStore, SelfContinuityStore,
    SessionStore,
};
use crate::task::TaskStore;
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;
use std::time::Duration;

const TAG: &str = "bg_timer";
const TICK_INTERVAL_SECS: u64 = 10;
const HEARTBEAT_DIVISOR: u32 = 3; // 30s
const CRON_REMIND_DIVISOR: u32 = 6; // 60s

/// 聚合 bg_timer 线程所需的全部依赖。
pub struct BgTimerContext {
    // shared
    pub system_inbound_tx: SystemInboundTx,
    pub resolve_locale: Arc<dyn Fn() -> Locale + Send + Sync>,
    pub platform: Arc<dyn crate::Platform>,

    // heartbeat
    pub version: &'static str,
    pub read_heartbeat: Box<dyn Fn() -> String + Send>,
    pub user_inbound_depth: Arc<AtomicUsize>,
    pub system_inbound_depth: Arc<AtomicUsize>,
    pub outbound_depth: Arc<AtomicUsize>,
    pub session_store: Arc<dyn SessionStore + Send + Sync>,
    pub memory_profile: MemoryProfile,
    pub autonomy_strategy_store: Arc<dyn AutonomyStrategyStore + Send + Sync>,
    pub self_continuity_store: Arc<dyn SelfContinuityStore + Send + Sync>,

    // cron
    pub memory_store: Option<Arc<dyn MemoryStore + Send + Sync>>,
    pub sensor_watch: Option<SensorWatchContext>,

    // remind
    pub remind_store: Arc<dyn RemindAtStore + Send + Sync>,
    pub task_store: Arc<dyn TaskStore + Send + Sync>,
}

/// 启动 bg_timer 后台线程（内部 spawn，立即返回）。
pub fn run_bg_timer(ctx: BgTimerContext) {
    crate::util::spawn_guarded_with_profile(
        "bg_timer",
        6144,
        Some(crate::util::SpawnCore::Core1),
        crate::util::HttpThreadRole::Background,
        move || {
            let interval = Duration::from_secs(TICK_INTERVAL_SECS);
            let mut tick: u32 = 0;
            let mut heartbeat_state = HeartbeatTickState::new();
            let mut cron_state = CronTickState::new();

            loop {
                std::thread::sleep(interval);
                tick = tick.wrapping_add(1);

                // heartbeat: every 3 ticks (30s)
                if tick.is_multiple_of(HEARTBEAT_DIVISOR) {
                    crate::heartbeat::heartbeat_tick(
                        ctx.version,
                        &ctx.system_inbound_tx,
                        ctx.read_heartbeat.as_ref(),
                        &ctx.user_inbound_depth,
                        &ctx.system_inbound_depth,
                        &ctx.outbound_depth,
                        ctx.session_store.as_ref(),
                        ctx.platform.as_ref(),
                        &ctx.resolve_locale,
                        &mut heartbeat_state,
                    );
                }

                // cron + remind + task due: every 6 ticks (60s)
                if tick.is_multiple_of(CRON_REMIND_DIVISOR) {
                    crate::cron::cron_tick(
                        &ctx.system_inbound_tx,
                        ctx.memory_store.as_ref(),
                        ctx.sensor_watch.as_ref(),
                        &ctx.resolve_locale,
                        &mut cron_state,
                    );

                    crate::memory::remind_tick(
                        ctx.remind_store.as_ref(),
                        &ctx.system_inbound_tx,
                        &ctx.resolve_locale,
                    );

                    crate::task::task_due_tick(
                        ctx.task_store.as_ref(),
                        &ctx.system_inbound_tx,
                        &ctx.resolve_locale,
                    );

                    crate::memory::self_runtime_tick(
                        &ctx.system_inbound_tx,
                        ctx.session_store.as_ref(),
                        ctx.self_continuity_store.as_ref(),
                        ctx.autonomy_strategy_store.as_ref(),
                        ctx.memory_profile,
                        crate::util::current_unix_secs(),
                    );
                }
            }
        },
    );
    log::info!(
        "[{}] bg_timer started (tick {}s, heartbeat every {}s, cron/remind/task every {}s)",
        TAG,
        TICK_INTERVAL_SECS,
        TICK_INTERVAL_SECS * HEARTBEAT_DIVISOR as u64,
        TICK_INTERVAL_SECS * CRON_REMIND_DIVISOR as u64
    );
}
