//! External WSS worker supervision for evictable ESP resource windows.
//! 外部 WSS worker 监管：voice-exclusive 窗口可卸载 worker，恢复后再重建。

use crate::error::Result;
use crate::util::TaskHandle;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

const CHANNEL_WSS_SUPERVISOR_RETRY_DELAY: Duration = Duration::from_secs(5);

pub type ChannelWssSpawner = dyn Fn() -> Result<TaskHandle> + Send + Sync + 'static;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ChannelWssRestartDelay {
    reason: &'static str,
    delay: Duration,
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

fn current_channel_wss_restart_delay() -> Option<ChannelWssRestartDelay> {
    let resource = crate::orchestrator::resource_light_snapshot();
    channel_wss_restart_delay_for_scheduler_context(
        crate::runtime::current_runtime_scheduler_context(
            crate::runtime::default_runtime_scheduler_profile(),
            resource.pressure,
        ),
    )
}

fn channel_wss_restart_delay_for_scheduler_context(
    context: crate::runtime::RuntimeSchedulerContext,
) -> Option<ChannelWssRestartDelay> {
    match crate::runtime::admit_runtime_work(
        crate::runtime::RuntimeWorkRequest::new(
            crate::runtime::RuntimeWorkClass::ChannelReconnect,
            crate::runtime::RuntimeWorkSource::Background,
        ),
        context,
    ) {
        crate::runtime::RuntimeWorkDecision::Proceed => None,
        crate::runtime::RuntimeWorkDecision::Defer {
            reason,
            retry_after_ms,
        }
        | crate::runtime::RuntimeWorkDecision::DrainAndResume {
            reason,
            retry_after_ms,
        } => Some(ChannelWssRestartDelay {
            reason,
            delay: Duration::from_millis(retry_after_ms),
        }),
        crate::runtime::RuntimeWorkDecision::Degrade { reason }
        | crate::runtime::RuntimeWorkDecision::Suspend { reason }
        | crate::runtime::RuntimeWorkDecision::RejectWithStableKey { reason, .. }
        | crate::runtime::RuntimeWorkDecision::RejectWithUserVisibleReason { reason } => {
            Some(ChannelWssRestartDelay {
                reason,
                delay: CHANNEL_WSS_SUPERVISOR_RETRY_DELAY,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            },
            pressure: crate::orchestrator::PressureLevel::Normal,
        }
    }

    #[test]
    fn channel_wss_restart_admission_defers_channel_reconnect_during_foreground() {
        let delay = channel_wss_restart_delay_for_scheduler_context(foreground_scheduler_context())
            .expect("foreground should defer external WSS restart");

        assert_eq!(delay.reason, "foreground_active");
        assert_eq!(delay.delay, Duration::from_millis(29_500));
    }
}
