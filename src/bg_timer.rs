//! 合并 cron + heartbeat + remind/task deadline wake 为单线程后台定时器。
//! Merged background timer: cron, heartbeat, and remind/task deadline wake in one thread.
//!
//! heartbeat / cron 维持固定周期；remind/task 根据最近 deadline 唤醒。
//! ESP 侧到期提醒/任务的存储突变由 write-back plane 执行；非 ESP office 日历清理
//! 从 write-back 回调投递 bounded delayed task，再由 `bg_timer` 串行执行 HTTP/TLS。

use crate::bus::SystemInboundTx;
use crate::config::AppConfig;
use crate::cron::{CronTickState, SensorWatchContext};
use crate::heartbeat::HeartbeatTickState;
use crate::i18n::Locale;
use crate::memory::{
    AutonomyStrategyStore, MemoryStore, MemorySystemKind, RelationshipPortfolioStore,
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
const AGENT_GUARD_INTERVAL_SECS: u64 = 10;
const WAIT_WDT_FEED_SLICE_SECS: u64 = 5;
const DUE_STORAGE_SWEEP_RETRY_MS: u64 = 250;

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
    let (lock, cv) = wake_state();
    let mut observed_generation = *lock.lock().unwrap_or_else(|e| e.into_inner());
    loop {
        let now = Instant::now();
        if deadline <= now {
            return;
        }
        let timeout = deadline
            .saturating_duration_since(now)
            .min(Duration::from_secs(WAIT_WDT_FEED_SLICE_SECS));
        let generation = lock.lock().unwrap_or_else(|e| e.into_inner());
        if *generation != observed_generation {
            return;
        }
        let (generation, _) = cv
            .wait_timeout(generation, timeout)
            .unwrap_or_else(|e| e.into_inner());
        if *generation != observed_generation {
            return;
        }
        observed_generation = *generation;
        crate::platform::task_wdt::feed_current_task();
    }
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
    pub config: Arc<AppConfig>,

    // heartbeat
    pub version: &'static str,
    pub read_heartbeat: Box<dyn Fn() -> String + Send>,
    pub user_inbound_depth: Arc<AtomicUsize>,
    pub system_inbound_depth: Arc<AtomicUsize>,
    pub outbound_depth: Arc<AtomicUsize>,
    pub session_store: Arc<dyn SessionStore + Send + Sync>,
    pub memory_system_kind: MemorySystemKind,
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

/// 启动 bg_timer 后台线程，返回真实 spawn 结果供启动链裁决。
pub fn run_bg_timer(ctx: BgTimerContext) -> std::io::Result<crate::util::TaskHandle> {
    crate::util::spawn_guarded_with_profile_handle(
        "bg_timer",
        crate::util::STACK_BG_TIMER,
        Some(crate::util::SpawnCore::Core1),
        crate::util::HttpThreadRole::Background,
        move || {
            let heartbeat_interval = Duration::from_secs(HEARTBEAT_INTERVAL_SECS);
            let cron_interval = Duration::from_secs(CRON_INTERVAL_SECS);
            let agent_guard_interval = Duration::from_secs(AGENT_GUARD_INTERVAL_SECS);
            let mut heartbeat_state = HeartbeatTickState::new();
            let mut cron_state = CronTickState::new();
            let mut next_heartbeat_at = Instant::now() + heartbeat_interval;
            let mut next_cron_at = Instant::now() + cron_interval;
            let mut next_agent_guard_at = Instant::now() + agent_guard_interval;
            let mut next_reminder_sweep_retry_at: Option<Instant> = None;
            let mut next_task_sweep_retry_at: Option<Instant> = None;

            loop {
                crate::platform::task_wdt::feed_current_task();
                let now = Instant::now();
                let now_unix_secs = crate::util::current_unix_secs();
                let runtime_mode = crate::runtime::thread_registry::runtime_mode_snapshot();
                let next_remind_at = if !runtime_mode.action_budget.allow_due_user_timers {
                    None
                } else if let Some(retry_at) = next_reminder_sweep_retry_at {
                    Some(retry_at)
                } else {
                    unix_deadline_to_instant(
                        ctx.remind_store.next_due_at().ok().flatten(),
                        now_unix_secs,
                        now,
                    )
                };
                let next_task_at = if !runtime_mode.action_budget.allow_due_user_timers {
                    None
                } else if let Some(retry_at) = next_task_sweep_retry_at {
                    Some(retry_at)
                } else {
                    unix_deadline_to_instant(
                        ctx.task_store.next_due_at().ok().flatten(),
                        now_unix_secs,
                        now,
                    )
                };
                let next_delayed_task_at =
                    now + crate::runtime::next_delayed_task_wait(heartbeat_interval);
                let next_wake_at = [
                    Some(next_heartbeat_at),
                    Some(next_cron_at),
                    next_remind_at,
                    next_task_at,
                    Some(next_delayed_task_at),
                    Some(next_agent_guard_at),
                ]
                .into_iter()
                .flatten()
                .min()
                .unwrap_or(now + heartbeat_interval);
                wait_until_or_notified(next_wake_at);
                crate::platform::task_wdt::feed_current_task();
                crate::runtime::service_delayed_tasks();
                crate::runtime::service_channel_wss_supervisors(TAG);

                let now = Instant::now();
                let now_unix_secs = crate::util::current_unix_secs();
                let runtime_mode = crate::runtime::thread_registry::runtime_mode_snapshot();

                if now >= next_agent_guard_at {
                    crate::runtime::service_agent_loop_guard(TAG);
                    advance_periodic_deadline(&mut next_agent_guard_at, agent_guard_interval, now);
                }

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
                        ctx.config.as_ref(),
                        &ctx.resolve_locale,
                        &mut heartbeat_state,
                    );
                    if runtime_mode.action_budget.allow_periodic_maintenance
                        && heartbeat_state.should_schedule_session_gc()
                    {
                        let scheduled = crate::runtime::write_back::schedule_session_gc(
                            Arc::clone(&ctx.session_store),
                            crate::constants::SESSION_GC_MAX_AGE_SECS,
                        );
                        if !scheduled {
                            log::debug!("[{}] session GC not queued by write-back scheduler", TAG);
                        }
                    }
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

                    let scheduled = crate::runtime::write_back::schedule_idle_self_runtime_tick(
                        crate::runtime::write_back::IdleSelfRuntimeTickInputs {
                            system_inbound_tx: ctx.system_inbound_tx.clone(),
                            detached_work_store: ctx.platform.detached_work_store(),
                            session_store: Arc::clone(&ctx.session_store),
                            self_continuity_store: Arc::clone(&ctx.self_continuity_store),
                            autonomy_strategy_store: Arc::clone(&ctx.autonomy_strategy_store),
                            self_authored_core_store: Arc::clone(&ctx.self_authored_core_store),
                            relationship_portfolio_store: Arc::clone(
                                &ctx.relationship_portfolio_store,
                            ),
                            relationship_topology_store: Arc::clone(
                                &ctx.relationship_topology_store,
                            ),
                            profile: ctx.memory_system_kind.memory_profile(),
                            now_secs: now_unix_secs,
                        },
                    );
                    if !scheduled {
                        log::debug!(
                            "[{}] self-runtime idle tick not queued by write-back scheduler",
                            TAG
                        );
                    }
                    crate::reasoning::enqueue_idle_memory_forge_tick(
                        &ctx.system_inbound_tx,
                        ctx.self_continuity_store.as_ref(),
                        ctx.memory_system_kind.memory_profile(),
                        ctx.platform.state_fs().as_ref(),
                        now_unix_secs,
                    );
                    let scheduled = crate::runtime::write_back::schedule_initiative_tick(
                        Arc::clone(&ctx.platform),
                        ctx.system_inbound_tx.clone(),
                        now_unix_secs,
                    );
                    if !scheduled {
                        log::debug!(
                            "[{}] initiative tick not queued by write-back scheduler",
                            TAG
                        );
                    }
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
                    if next_reminder_sweep_retry_at.is_none_or(|retry_at| retry_at <= now) {
                        let platform = Arc::clone(&ctx.platform);
                        let config = Arc::clone(&ctx.config);
                        let scheduled = crate::runtime::write_back::schedule_due_reminder_sweep(
                            Arc::clone(&ctx.remind_store),
                            move |reminder| {
                                clear_due_reminder_calendar_link(
                                    reminder,
                                    Arc::clone(&platform),
                                    config.as_ref(),
                                )
                            },
                            ctx.system_inbound_tx.clone(),
                            Arc::clone(&ctx.resolve_locale),
                        );
                        if !scheduled {
                            log::warn!(
                                "[{}] due reminder sweep deferred because write-back queue is full",
                                TAG
                            );
                        }
                        next_reminder_sweep_retry_at =
                            Some(now + Duration::from_millis(DUE_STORAGE_SWEEP_RETRY_MS));
                    }
                } else {
                    next_reminder_sweep_retry_at = None;
                }

                if runtime_mode.action_budget.allow_due_user_timers
                    && ctx
                        .task_store
                        .next_due_at()
                        .ok()
                        .flatten()
                        .is_some_and(|due_at| due_at <= now_unix_secs)
                {
                    if next_task_sweep_retry_at.is_none_or(|retry_at| retry_at <= now) {
                        let scheduled = crate::runtime::write_back::schedule_due_task_sweep(
                            Arc::clone(&ctx.task_store),
                            ctx.system_inbound_tx.clone(),
                            Arc::clone(&ctx.resolve_locale),
                        );
                        if !scheduled {
                            log::warn!(
                                "[{}] due task sweep deferred because write-back queue is full",
                                TAG
                            );
                        }
                        next_task_sweep_retry_at =
                            Some(now + Duration::from_millis(DUE_STORAGE_SWEEP_RETRY_MS));
                    }
                } else {
                    next_task_sweep_retry_at = None;
                }
            }
        },
    )
}

pub fn log_bg_timer_started() {
    log::info!(
        "[{}] bg_timer started (heartbeat every {}s, cron every {}s, remind/task write-back scheduled)",
        TAG,
        HEARTBEAT_INTERVAL_SECS,
        CRON_INTERVAL_SECS
    );
}

#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
fn build_reminder_calendar_service(
    platform: Arc<dyn crate::Platform>,
) -> crate::calendar::CalendarService {
    let topology = crate::office::build_default_office_integration_topology();
    let office_authority = std::sync::Arc::new(crate::office::ReloadingOfficeAuthoritySource::new(
        std::sync::Arc::new(crate::config::PlatformConfigFileStore(Arc::clone(
            &platform,
        ))),
        platform.office_credential_store(),
        platform.office_runtime_status_store(),
    ));
    let credential_store = std::sync::Arc::new(
        crate::calendar::OfficeBackedCalendarProviderCredentialStore::with_authority(
            office_authority.clone(),
        ),
    );
    crate::calendar::CalendarService::with_office_authority(
        platform.calendar_store(),
        credential_store,
        topology.calendar_providers(),
        Some(office_authority),
    )
}

fn clear_due_reminder_calendar_link(
    reminder: &crate::reminder::ReminderItem,
    platform: Arc<dyn crate::Platform>,
    _config: &AppConfig,
) -> crate::Result<()> {
    if reminder.calendar_event_id.trim().is_empty() {
        return Ok(());
    }
    let provider = reminder.calendar_provider.trim();
    if provider.is_empty() || provider == crate::calendar::CALENDAR_PROVIDER_LOCAL {
        let _ = platform
            .calendar_store()
            .delete(&reminder.calendar_event_id)?;
        return Ok(());
    }
    #[cfg(all(
        feature = "capability_office",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    ))]
    {
        return schedule_due_reminder_remote_calendar_cleanup(
            reminder.clone(),
            platform,
            _config.clone(),
        );
    }
    #[allow(unreachable_code)]
    Err(crate::Error::config(
        "bg_timer_reminder_calendar_cleanup",
        format!("calendar provider '{provider}' unavailable in this runtime"),
    ))
}

#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
fn schedule_due_reminder_remote_calendar_cleanup(
    reminder: crate::reminder::ReminderItem,
    platform: Arc<dyn crate::Platform>,
    config: AppConfig,
) -> crate::Result<()> {
    let reminder_id = reminder.id.clone();
    crate::runtime::schedule_critical_delayed_task(
        Instant::now(),
        Box::new(move || {
            if let Err(error) =
                clear_due_reminder_remote_calendar_link(&reminder, platform, &config)
            {
                log::warn!(
                    "[{}] linked calendar cleanup failed for reminder {}: {}",
                    TAG,
                    reminder_id,
                    error
                );
            }
        }),
    )
    .map_err(|_| {
        crate::Error::config(
            "bg_timer_reminder_calendar_cleanup",
            "failed to queue remote calendar cleanup delayed task",
        )
    })
}

#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
fn clear_due_reminder_remote_calendar_link(
    reminder: &crate::reminder::ReminderItem,
    platform: Arc<dyn crate::Platform>,
    config: &AppConfig,
) -> crate::Result<()> {
    let service = build_reminder_calendar_service(Arc::clone(&platform));
    let mut http = crate::network::create_http_client_with_config(
        platform.as_ref(),
        config,
        crate::network::HttpClientClass::Background,
    )?;
    let http = crate::office::as_office_http_client(&mut http);
    let _ = service.delete(
        Some(http),
        reminder.calendar_provider.trim(),
        (!reminder.calendar_account_key.trim().is_empty())
            .then_some(reminder.calendar_account_key.as_str()),
        &reminder.calendar_event_id,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wait_until_or_notified_returns_early_after_notify() {
        let handle = std::thread::spawn(|| {
            std::thread::sleep(Duration::from_millis(30));
            notify_deadline_changed();
        });
        let started = Instant::now();
        wait_until_or_notified(Instant::now() + Duration::from_millis(250));
        handle.join().expect("notify join");
        assert!(
            started.elapsed() < Duration::from_millis(150),
            "wait should wake on notify instead of waiting for the original deadline"
        );
    }
}
