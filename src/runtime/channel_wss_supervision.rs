//! External WSS worker supervision for evictable ESP resource windows.
//! 外部 WSS worker 监管：voice-exclusive 窗口可卸载 worker，恢复后再重建。

use crate::error::Result;
use crate::util::TaskHandle;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

const CHANNEL_WSS_SUPERVISOR_RETRY_DELAY: Duration = Duration::from_secs(5);
const CHANNEL_WSS_FOREGROUND_ACTIVITY_RETRY_DELAY: Duration = Duration::from_millis(500);
const CHANNEL_WSS_NETWORK_RETRY_DELAY: Duration = Duration::from_secs(1);

pub type ChannelWssSpawner = dyn Fn() -> Result<TaskHandle> + Send + Sync + 'static;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ChannelWssRestartDelay {
    reason: &'static str,
    delay: Duration,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ChannelWssRestartAction {
    Allowed,
    Deferred {
        reason: &'static str,
        delay: Duration,
    },
}

struct ChannelWssSupervisor {
    owner: &'static str,
    handle: Option<TaskHandle>,
    spawner: Arc<ChannelWssSpawner>,
    next_retry_at: Option<Instant>,
}

fn supervisors() -> &'static Mutex<Vec<ChannelWssSupervisor>> {
    static SUPERVISORS: OnceLock<Mutex<Vec<ChannelWssSupervisor>>> = OnceLock::new();
    SUPERVISORS.get_or_init(|| Mutex::new(Vec::new()))
}

fn mark_channel_wss_lifecycle(
    owner: &'static str,
    state: crate::runtime::PlaneLifecycleState,
    reason: &'static str,
) {
    let _ = crate::runtime::plane_lifecycle::mark(
        crate::runtime::PlaneId::ChannelWss,
        owner,
        state,
        reason,
    );
}

/// Register one enabled external WSS worker for restart after voice-exclusive eviction.
pub fn register_channel_wss_supervisor(
    owner: &'static str,
    handle: TaskHandle,
    spawner: Arc<ChannelWssSpawner>,
) {
    let mut state = supervisors().lock().unwrap_or_else(|e| e.into_inner());
    if let Some(existing) = state.iter_mut().find(|entry| entry.owner == owner) {
        existing.handle = Some(handle);
        existing.spawner = spawner;
        existing.next_retry_at = None;
    } else {
        state.push(ChannelWssSupervisor {
            owner,
            handle: Some(handle),
            spawner,
            next_retry_at: None,
        });
    }
}

/// Register one enabled external WSS worker without spawning it yet.
pub fn register_deferred_channel_wss_supervisor(
    owner: &'static str,
    spawner: Arc<ChannelWssSpawner>,
    reason: &'static str,
) {
    let mut state = supervisors().lock().unwrap_or_else(|e| e.into_inner());
    if let Some(existing) = state.iter_mut().find(|entry| entry.owner == owner) {
        existing.handle = None;
        existing.spawner = spawner;
        existing.next_retry_at = None;
    } else {
        state.push(ChannelWssSupervisor {
            owner,
            handle: None,
            spawner,
            next_retry_at: None,
        });
    }
    mark_channel_wss_lifecycle(
        owner,
        crate::runtime::PlaneLifecycleState::Suspended,
        reason,
    );
    crate::bg_timer::notify_deadline_changed();
}

/// Current reason why an external WSS worker should stay unloaded.
pub fn channel_wss_worker_start_defer_reason() -> Option<&'static str> {
    current_channel_wss_restart_delay().map(|delay| delay.reason)
}

/// Restart unloaded external WSS workers once the resource window has resumed.
pub fn service_channel_wss_supervisors(tag: &str) {
    let mut state = supervisors().lock().unwrap_or_else(|e| e.into_inner());
    let now = Instant::now();
    for entry in state.iter_mut() {
        if entry
            .handle
            .as_ref()
            .is_some_and(crate::util::TaskHandle::is_finished)
        {
            if let Some(handle) = entry.handle.take() {
                let _ = handle.join();
            }
        }

        if entry.handle.is_some()
            || crate::network::external_wss_suspend_requested()
            || crate::network::external_wss_worker_evict_requested()
        {
            continue;
        }
        if entry
            .next_retry_at
            .is_some_and(|next_retry_at| now < next_retry_at)
        {
            continue;
        }
        if let Some(delay) = current_channel_wss_restart_delay() {
            entry.next_retry_at = Some(now + delay.delay);
            mark_channel_wss_lifecycle(
                entry.owner,
                crate::runtime::PlaneLifecycleState::Suspended,
                delay.reason,
            );
            log::debug!(
                "[{}] external WSS worker restart deferred owner={} reason={}",
                tag,
                entry.owner,
                delay.reason
            );
            continue;
        }

        match (entry.spawner)() {
            Ok(handle) => {
                log::info!(
                    "[{}] restarted external WSS worker owner={}",
                    tag,
                    entry.owner
                );
                entry.handle = Some(handle);
                entry.next_retry_at = None;
            }
            Err(error) => {
                crate::metrics::record_runtime_spawn_failure();
                entry.next_retry_at = Some(now + CHANNEL_WSS_SUPERVISOR_RETRY_DELAY);
                log::warn!(
                    "[{}] external WSS worker restart failed owner={}: {}",
                    tag,
                    entry.owner,
                    error
                );
            }
        }
    }
}

/// Earliest retry deadline for a deferred external WSS worker.
pub fn next_channel_wss_supervisor_retry_at() -> Option<Instant> {
    let state = supervisors().lock().unwrap_or_else(|e| e.into_inner());
    state
        .iter()
        .filter(|entry| entry.handle.is_none())
        .filter_map(|entry| entry.next_retry_at)
        .min()
}

fn current_channel_wss_restart_delay() -> Option<ChannelWssRestartDelay> {
    if let Some(delay) = channel_wss_restart_delay_for_startup() {
        return Some(delay);
    }
    let resource = crate::orchestrator::resource_light_snapshot();
    if let Some(delay) = channel_wss_restart_delay_for_resource_activity(&resource) {
        return Some(delay);
    }
    channel_wss_restart_delay_for_scheduler_context(
        crate::runtime::current_runtime_scheduler_context(
            crate::runtime::default_runtime_scheduler_profile(),
            resource.pressure,
        ),
    )
}

fn channel_wss_restart_delay_for_startup() -> Option<ChannelWssRestartDelay> {
    channel_wss_restart_delay_for_startup_readiness(
        crate::runtime::runtime_startup_readiness_snapshot(),
    )
}

fn channel_wss_restart_delay_for_startup_readiness(
    readiness: crate::runtime::RuntimeStartupReadiness,
) -> Option<ChannelWssRestartDelay> {
    if readiness.allow_external_wss_worker {
        return None;
    }
    match readiness.network_reason {
        crate::runtime::RuntimeStartupNetworkReason::None => Some(ChannelWssRestartDelay {
            reason: readiness.worker_block_reason(),
            delay: CHANNEL_WSS_NETWORK_RETRY_DELAY,
        }),
        reason => Some(ChannelWssRestartDelay {
            reason: reason.as_str(),
            delay: CHANNEL_WSS_NETWORK_RETRY_DELAY,
        }),
    }
}

#[cfg(test)]
fn channel_wss_restart_delay_for_network_snapshot(
    snapshot: &crate::state::NetworkRuntimeSnapshot,
) -> Option<ChannelWssRestartDelay> {
    crate::network::external_wss_network_suspend_reason(snapshot).map(|reason| {
        ChannelWssRestartDelay {
            reason,
            delay: CHANNEL_WSS_NETWORK_RETRY_DELAY,
        }
    })
}

fn channel_wss_restart_delay_for_resource_activity(
    resource: &crate::orchestrator::ResourceLightSnapshot,
) -> Option<ChannelWssRestartDelay> {
    let reason = if resource.active_agent_tasks > 0 {
        "active_agent_task"
    } else if resource.active_http_count > 0 {
        "active_http"
    } else if crate::channels::active_os_outbound_worker_count() > 0 {
        "active_os_outbound"
    } else if resource.outbound_depth > 0 {
        "outbound_pending"
    } else {
        return None;
    };
    Some(ChannelWssRestartDelay {
        reason,
        delay: CHANNEL_WSS_FOREGROUND_ACTIVITY_RETRY_DELAY,
    })
}

fn channel_wss_restart_delay_for_scheduler_context(
    context: crate::runtime::RuntimeSchedulerContext,
) -> Option<ChannelWssRestartDelay> {
    match channel_wss_restart_action_for_scheduler_context(context) {
        ChannelWssRestartAction::Allowed => None,
        ChannelWssRestartAction::Deferred { reason, delay } => {
            Some(ChannelWssRestartDelay { reason, delay })
        }
    }
}

fn channel_wss_restart_action_for_scheduler_context(
    context: crate::runtime::RuntimeSchedulerContext,
) -> ChannelWssRestartAction {
    match crate::runtime::admit_runtime_work(
        crate::runtime::RuntimeWorkRequest::new(
            crate::runtime::RuntimeWorkClass::ChannelReconnect,
            crate::runtime::RuntimeWorkSource::Background,
        ),
        context,
    ) {
        crate::runtime::RuntimeWorkDecision::Proceed => ChannelWssRestartAction::Allowed,
        crate::runtime::RuntimeWorkDecision::Defer {
            reason,
            retry_after_ms,
        }
        | crate::runtime::RuntimeWorkDecision::DrainAndResume {
            reason,
            retry_after_ms,
        } => ChannelWssRestartAction::Deferred {
            reason,
            delay: Duration::from_millis(retry_after_ms),
        },
        crate::runtime::RuntimeWorkDecision::Degrade { reason }
        | crate::runtime::RuntimeWorkDecision::Suspend { reason }
        | crate::runtime::RuntimeWorkDecision::RejectWithStableKey { reason, .. }
        | crate::runtime::RuntimeWorkDecision::RejectWithUserVisibleReason { reason } => {
            ChannelWssRestartAction::Deferred {
                reason,
                delay: CHANNEL_WSS_SUPERVISOR_RETRY_DELAY,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resource_snapshot() -> crate::orchestrator::ResourceLightSnapshot {
        crate::orchestrator::ResourceLightSnapshot {
            pressure: crate::orchestrator::PressureLevel::Normal,
            tls_fragmentation_risk: crate::orchestrator::TlsFragmentationRisk::Healthy,
            storage_contention_risk: crate::orchestrator::StorageContentionRisk::Healthy,
            heap_free_internal: 96 * 1024,
            heap_min_free_internal: 80 * 1024,
            heap_free_spiram: 8 * 1024 * 1024,
            heap_total_spiram: 16 * 1024 * 1024,
            heap_min_free_spiram: 8 * 1024 * 1024,
            heap_largest_block_spiram: 8 * 1024 * 1024,
            heap_used_spiram_est: 8 * 1024 * 1024,
            heap_largest_block_internal: 32 * 1024,
            active_http_count: 0,
            active_wss_count: 0,
            active_agent_tasks: 0,
            inbound_depth: 0,
            outbound_depth: 0,
            budget: crate::orchestrator::pressure::budget_for_level(
                crate::orchestrator::PressureLevel::Normal,
            ),
            session_count: 0,
            storage_used_kb: 0,
            storage_total_kb: 0,
            #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
            cpu_usage_percent: 0.0,
            #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
            load_average: (0.0, 0.0, 0.0),
            #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
            process_memory_kb: 0,
        }
    }

    fn foreground_scheduler_context() -> crate::runtime::RuntimeSchedulerContext {
        crate::runtime::RuntimeSchedulerContext {
            profile: crate::runtime::RuntimePlanePolicyProfile::EspCompact,
            runtime_mode: crate::runtime::mode::snapshot_from_source(
                crate::runtime::mode::RuntimeModeSource::default(),
            ),
            foreground: crate::runtime::RuntimeForegroundOverlay {
                active: true,
                active_count: 1,
                primary_source: Some(crate::runtime::RuntimeForegroundSource::ExternalUserMessage),
                age_ms: Some(500),
                resume_after_ms: Some(29_500),
                ..crate::runtime::RuntimeForegroundOverlay::default()
            },
            pressure: crate::orchestrator::PressureLevel::Normal,
        }
    }

    fn foreground_recovery_scheduler_context() -> crate::runtime::RuntimeSchedulerContext {
        crate::runtime::RuntimeSchedulerContext {
            foreground: crate::runtime::RuntimeForegroundOverlay {
                recovery_active: true,
                recovery_source: Some(crate::runtime::RuntimeForegroundSource::ExternalUserMessage),
                recovery_age_ms: Some(500),
                recovery_resume_after_ms: Some(9_500),
                ..crate::runtime::RuntimeForegroundOverlay::default()
            },
            ..foreground_scheduler_context()
        }
    }

    #[test]
    fn channel_wss_restart_admission_defers_channel_reconnect_during_foreground() {
        let delay = channel_wss_restart_delay_for_scheduler_context(foreground_scheduler_context())
            .expect("foreground should defer external WSS restart");

        assert_eq!(delay.reason, "foreground_active");
        assert_eq!(delay.delay, Duration::from_millis(29_500));
    }

    #[test]
    fn channel_wss_restart_admission_allows_reconnect_during_post_foreground_recovery() {
        let delay = channel_wss_restart_delay_for_scheduler_context(
            foreground_recovery_scheduler_context(),
        );

        assert!(
            delay.is_none(),
            "post-foreground recovery keeps WSS reconnect user-reachable while background work stays deferred"
        );
    }

    #[test]
    fn channel_wss_restart_waits_for_primary_delivery_activity_to_settle() {
        let mut resource = resource_snapshot();
        resource.active_agent_tasks = 1;
        assert_eq!(
            channel_wss_restart_delay_for_resource_activity(&resource),
            Some(ChannelWssRestartDelay {
                reason: "active_agent_task",
                delay: CHANNEL_WSS_FOREGROUND_ACTIVITY_RETRY_DELAY,
            })
        );

        resource.active_agent_tasks = 0;
        resource.active_http_count = 1;
        assert_eq!(
            channel_wss_restart_delay_for_resource_activity(&resource),
            Some(ChannelWssRestartDelay {
                reason: "active_http",
                delay: CHANNEL_WSS_FOREGROUND_ACTIVITY_RETRY_DELAY,
            })
        );

        resource.active_http_count = 0;
        crate::channels::set_active_os_outbound_worker_count_for_tests(1);
        assert_eq!(
            channel_wss_restart_delay_for_resource_activity(&resource),
            Some(ChannelWssRestartDelay {
                reason: "active_os_outbound",
                delay: CHANNEL_WSS_FOREGROUND_ACTIVITY_RETRY_DELAY,
            })
        );

        crate::channels::set_active_os_outbound_worker_count_for_tests(0);
        resource.outbound_depth = 1;
        assert_eq!(
            channel_wss_restart_delay_for_resource_activity(&resource),
            Some(ChannelWssRestartDelay {
                reason: "outbound_pending",
                delay: CHANNEL_WSS_FOREGROUND_ACTIVITY_RETRY_DELAY,
            })
        );
    }

    #[test]
    fn channel_wss_restart_waits_outside_worker_until_wifi_outbound_ready() {
        let snapshot = crate::state::NetworkRuntimeSnapshot {
            sta_expected: true,
            sta_configured: true,
            sta_connecting: false,
            sta_l2_connected: false,
            sta_ip_present: false,
            outbound_settled: false,
            wall_clock_trustworthy: false,
            last_wifi_stage: crate::state::NetworkWifiStage::StaApNotFound,
            last_wifi_reason_code: Some(201),
        };

        assert_eq!(
            channel_wss_restart_delay_for_network_snapshot(&snapshot),
            Some(ChannelWssRestartDelay {
                reason: "wifi_not_ready",
                delay: CHANNEL_WSS_NETWORK_RETRY_DELAY,
            })
        );
    }

    #[test]
    fn channel_wss_restart_uses_startup_readiness_before_spawning_worker() {
        let readiness = crate::runtime::RuntimeStartupReadiness {
            phase: crate::runtime::RuntimeStartupPhase::LocalRuntimeAssembled,
            reason: "wifi_not_ready",
            network_reason: crate::runtime::RuntimeStartupNetworkReason::WifiNotReady,
            allow_config_recovery_routes: true,
            allow_default_status_routes: true,
            allow_external_wss_worker: false,
            allow_agent_heavy_execution: false,
            allow_channel_outbound_worker: false,
            allow_voice_realtime_connect: false,
            allow_write_back_worker: false,
            allow_display_status_surface: true,
            allow_display_heavy_refresh: true,
            config_worker_floor_available: true,
        };

        assert_eq!(
            channel_wss_restart_delay_for_startup_readiness(readiness),
            Some(ChannelWssRestartDelay {
                reason: "wifi_not_ready",
                delay: CHANNEL_WSS_NETWORK_RETRY_DELAY,
            })
        );
    }

    #[test]
    fn channel_wss_start_defer_reason_uses_current_wifi_runtime_state() {
        let _guard = crate::state::test_state_guard();
        crate::state::set_network_sta_expected(true, true);
        crate::state::set_network_wifi_stage(
            crate::state::NetworkWifiStage::StaApNotFound,
            Some(201),
        );
        crate::state::clear_wifi_sta_state();

        assert_eq!(
            channel_wss_worker_start_defer_reason(),
            Some("wifi_not_ready")
        );
    }

    #[test]
    fn channel_wss_restart_waits_outside_worker_until_wall_clock_ready() {
        let snapshot = crate::state::NetworkRuntimeSnapshot {
            sta_expected: true,
            sta_configured: true,
            sta_connecting: false,
            sta_l2_connected: true,
            sta_ip_present: true,
            outbound_settled: true,
            wall_clock_trustworthy: false,
            last_wifi_stage: crate::state::NetworkWifiStage::StaIpReady,
            last_wifi_reason_code: None,
        };

        assert_eq!(
            channel_wss_restart_delay_for_network_snapshot(&snapshot),
            Some(ChannelWssRestartDelay {
                reason: "wall_clock_untrusted",
                delay: CHANNEL_WSS_NETWORK_RETRY_DELAY,
            })
        );
    }

    #[test]
    fn channel_wss_restart_allows_worker_after_network_snapshot_ready() {
        let snapshot = crate::state::NetworkRuntimeSnapshot {
            sta_expected: true,
            sta_configured: true,
            sta_connecting: false,
            sta_l2_connected: true,
            sta_ip_present: true,
            outbound_settled: true,
            wall_clock_trustworthy: true,
            last_wifi_stage: crate::state::NetworkWifiStage::StaIpReady,
            last_wifi_reason_code: None,
        };

        assert_eq!(
            channel_wss_restart_delay_for_network_snapshot(&snapshot),
            None
        );
    }

    #[test]
    fn channel_wss_restart_admission_defers_under_critical_pressure() {
        let mut context = foreground_scheduler_context();
        context.foreground = crate::runtime::RuntimeForegroundOverlay::default();
        context.pressure = crate::orchestrator::PressureLevel::Critical;

        let action = channel_wss_restart_action_for_scheduler_context(context);

        assert_eq!(
            action,
            ChannelWssRestartAction::Deferred {
                reason: "critical_pressure",
                delay: Duration::from_millis(1_500),
            }
        );
    }
}
