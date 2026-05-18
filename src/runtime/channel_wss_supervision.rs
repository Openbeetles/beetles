//! External WSS worker supervision for evictable ESP resource windows.
//! 外部 WSS worker 监管：voice-exclusive 窗口可卸载 worker，恢复后再重建。

use crate::error::Result;
use crate::util::TaskHandle;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

const CHANNEL_WSS_SUPERVISOR_RETRY_DELAY: Duration = Duration::from_secs(5);

pub type ChannelWssSpawner = dyn Fn() -> Result<TaskHandle> + Send + Sync + 'static;

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
