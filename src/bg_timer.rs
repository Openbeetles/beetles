//! 合并 cron + heartbeat + remind/task 为单线程后台定时器。
//! Merged background timer: cron, heartbeat, and remind/task in one thread to save ~20KB SRAM.
//!
//! heartbeat / cron 维持固定周期；remind/task 根据最近 deadline 唤醒，避免固定 60s 粗轮询。

use crate::bus::SystemInboundTx;
use crate::cron::{CronTickState, SensorWatchContext};
use crate::heartbeat::HeartbeatTickState;
use crate::i18n::Locale;
use crate::memory::{
    AutonomyStrategyStore, MemoryProfile, MemoryStore, RelationshipPortfolioStore,
    RelationshipTopologyStore, RemindAtStore, SelfAuthoredCoreStore, SelfContinuityStore,
    SessionStore,
};
use crate::task::TaskStore;
use std::sync::atomic::AtomicUsize;
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

const TAG: &str = "bg_timer";
const HEARTBEAT_INTERVAL_SECS: u64 = 30;
const CRON_INTERVAL_SECS: u64 = 60;

fn wake_state() -> &'static (Mutex<u64>, Condvar) {
    static STATE: OnceLock<(Mutex<u64>, Condvar)> = OnceLock::new();
    STATE.get_or_init(|| (Mutex::new(0), Condvar::new()))
}

pub fn notify_deadline_changed() {
    let (lock, cv) = wake_state();
    let mut generation = lock.lock().unwrap_or_else(|e| e.into_inner());
    *generation = generation.wrapping_add(1);
    cv.notify_one();
}

fn wait_until_or_notified(deadline: Instant) {
    let now = Instant::now();
    if deadline <= now {
        return;
    }
    let timeout = deadline.saturating_duration_since(now);
    let (lock, cv) = wake_state();
    let generation = lock.lock().unwrap_or_else(|e| e.into_inner());
    let _ = cv
        .wait_timeout(generation, timeout)
        .unwrap_or_else(|e| e.into_inner());
}

fn advance_periodic_deadline(deadline: &mut Instant, interval: Duration, now: Instant) {
    while *deadline <= now {
        *deadline += interval;
    }
}

fn unix_deadline_to_instant(
    next_due_at: Option<u64>,
    now_unix_secs: u64,
    now: Instant,
) -> Option<Instant> {
    next_due_at.map(|due_at| {
        if due_at <= now_unix_secs {
            now
        } else {
            now + Duration::from_secs(due_at - now_unix_secs)
        }
    })
}

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
    pub self_authored_core_store: Arc<dyn SelfAuthoredCoreStore + Send + Sync>,
    pub self_continuity_store: Arc<dyn SelfContinuityStore + Send + Sync>,
    pub relationship_portfolio_store: Arc<dyn RelationshipPortfolioStore + Send + Sync>,
    pub relationship_topology_store: Arc<dyn RelationshipTopologyStore + Send + Sync>,
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
        crate::util::STACK_BG_TIMER,
        Some(crate::util::SpawnCore::Core1),
        crate::util::HttpThreadRole::Background,
        move || {
            let heartbeat_interval = Duration::from_secs(HEARTBEAT_INTERVAL_SECS);
            let cron_interval = Duration::from_secs(CRON_INTERVAL_SECS);
            let mut heartbeat_state = HeartbeatTickState::new();
            let mut cron_state = CronTickState::new();
            let mut next_heartbeat_at = Instant::now() + heartbeat_interval;
            let mut next_cron_at = Instant::now() + cron_interval;

            loop {
                let now = Instant::now();
                let now_unix_secs = crate::util::current_unix_secs();
                let runtime_mode = crate::runtime::thread_registry::runtime_mode_snapshot();
                let next_remind_at = if runtime_mode.action_budget.allow_due_user_timers {
                    unix_deadline_to_instant(
                        ctx.remind_store.next_due_at().ok().flatten(),
                        now_unix_secs,
                        now,
                    )
                } else {
                    None
                };
                let next_task_at = if runtime_mode.action_budget.allow_due_user_timers {
                    unix_deadline_to_instant(
                        ctx.task_store.next_due_at().ok().flatten(),
                        now_unix_secs,
                        now,
                    )
                } else {
                    None
                };
                let next_delayed_task_at =
                    now + crate::runtime::next_delayed_task_wait(heartbeat_interval);
                let next_wake_at = [
                    Some(next_heartbeat_at),
                    Some(next_cron_at),
                    next_remind_at,
                    next_task_at,
                    Some(next_delayed_task_at),
                ]
                .into_iter()
                .flatten()
                .min()
                .unwrap_or(now + heartbeat_interval);
                wait_until_or_notified(next_wake_at);
                crate::runtime::service_delayed_tasks();

                let now = Instant::now();
                let now_unix_secs = crate::util::current_unix_secs();
                let runtime_mode = crate::runtime::thread_registry::runtime_mode_snapshot();

                if now >= next_heartbeat_at {
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
                    advance_periodic_deadline(&mut next_heartbeat_at, heartbeat_interval, now);
                }

                if now >= next_cron_at && runtime_mode.action_budget.allow_periodic_maintenance {
                    crate::cron::cron_tick(
                        &ctx.system_inbound_tx,
                        ctx.memory_store.as_ref(),
                        ctx.sensor_watch.as_ref(),
                        &ctx.resolve_locale,
                        &mut cron_state,
                    );

                    crate::memory::self_runtime_tick(
                        &ctx.system_inbound_tx,
                        ctx.session_store.as_ref(),
                        ctx.self_continuity_store.as_ref(),
                        ctx.autonomy_strategy_store.as_ref(),
                        ctx.self_authored_core_store.as_ref(),
                        ctx.relationship_portfolio_store.as_ref(),
                        ctx.relationship_topology_store.as_ref(),
                        ctx.memory_profile,
                        now_unix_secs,
                    );
                    crate::runtime::initiative_tick(
                        ctx.platform.as_ref(),
                        &ctx.system_inbound_tx,
                        now_unix_secs,
                    );
                    advance_periodic_deadline(&mut next_cron_at, cron_interval, now);
                } else if now >= next_cron_at {
                    advance_periodic_deadline(&mut next_cron_at, cron_interval, now);
                }

                if runtime_mode.action_budget.allow_due_user_timers
                    && ctx
                        .remind_store
                        .next_due_at()
                        .ok()
                        .flatten()
                        .is_some_and(|due_at| due_at <= now_unix_secs)
                {
                    crate::memory::remind_tick(
                        ctx.remind_store.as_ref(),
                        &ctx.system_inbound_tx,
                        &ctx.resolve_locale,
                    );
                }

                if runtime_mode.action_budget.allow_due_user_timers
                    && ctx
                        .task_store
                        .next_due_at()
                        .ok()
                        .flatten()
                        .is_some_and(|due_at| due_at <= now_unix_secs)
                {
                    crate::task::task_due_tick(
                        ctx.task_store.as_ref(),
                        &ctx.system_inbound_tx,
                        &ctx.resolve_locale,
                    );
                }
            }
        },
    );
    log::info!(
        "[{}] bg_timer started (heartbeat every {}s, cron every {}s, remind/task deadline-driven)",
        TAG,
        HEARTBEAT_INTERVAL_SECS,
        CRON_INTERVAL_SECS
    );
}
