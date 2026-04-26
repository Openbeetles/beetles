//! Runtime plane lifecycle state.
//! 运行面生命周期状态。

use crate::runtime::PlaneId;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

/// Dynamic lifecycle state of a runtime execution plane.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaneLifecycleState {
    Registered,
    Starting,
    Active,
    Suspended,
    Draining,
    Stopping,
    Disabled,
    Unloaded,
    Failed,
}

/// Dynamic lifecycle record keyed by `plane + owner`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub struct PlaneLifecycleRecord {
    pub plane: PlaneId,
    pub owner: &'static str,
    pub state: PlaneLifecycleState,
    pub updated_at_ms: u64,
    pub generation: u64,
    pub transition_count: u32,
    pub failure_count: u32,
    pub last_reason: &'static str,
}

/// Point-in-time lifecycle snapshot.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct PlaneLifecycleSnapshot {
    pub total_records: usize,
    pub active_count: usize,
    pub suspended_count: usize,
    pub draining_count: usize,
    pub stopping_count: usize,
    pub unloaded_count: usize,
    pub failed_count: usize,
    pub records: Vec<PlaneLifecycleRecord>,
}

struct PlaneLifecycleRegistry {
    records: Vec<PlaneLifecycleRecord>,
    next_generation: u64,
}

static LIFECYCLE: OnceLock<Mutex<PlaneLifecycleRegistry>> = OnceLock::new();
static MONOTONIC_START: OnceLock<Instant> = OnceLock::new();

fn registry() -> &'static Mutex<PlaneLifecycleRegistry> {
    LIFECYCLE.get_or_init(|| {
        Mutex::new(PlaneLifecycleRegistry {
            records: Vec::new(),
            next_generation: 1,
        })
    })
}

fn now_ms() -> u64 {
    MONOTONIC_START
        .get_or_init(Instant::now)
        .elapsed()
        .as_millis()
        .min(u64::MAX as u128) as u64
}

fn next_generation(registry: &mut PlaneLifecycleRegistry) -> u64 {
    let generation = registry.next_generation;
    registry.next_generation = registry.next_generation.saturating_add(1).max(1);
    generation
}

fn should_start_new_generation(
    previous: Option<PlaneLifecycleState>,
    next: PlaneLifecycleState,
) -> bool {
    previous.is_none()
        || matches!(next, PlaneLifecycleState::Starting)
        || matches!(
            previous,
            Some(PlaneLifecycleState::Unloaded | PlaneLifecycleState::Failed)
        )
}

fn plane_sort_key(plane: PlaneId) -> u8 {
    match plane {
        PlaneId::Bootstrap => 0,
        PlaneId::ConfigRecovery => 1,
        PlaneId::ChannelWss => 2,
        PlaneId::ChannelOutbound => 3,
        PlaneId::AgentMain => 4,
        PlaneId::Display => 5,
        PlaneId::Voice => 6,
        PlaneId::StorageWriteBack => 7,
        PlaneId::Diagnostic => 8,
        PlaneId::Maintenance => 9,
        PlaneId::PlatformWifi => 10,
        PlaneId::PlatformAudio => 11,
        PlaneId::RuntimeAux => 12,
    }
}

/// Mark a lifecycle transition. This is best-effort and never fails callers.
pub fn mark(
    plane: PlaneId,
    owner: &'static str,
    state: PlaneLifecycleState,
    reason: &'static str,
) -> PlaneLifecycleRecord {
    mark_at(plane, owner, state, reason, now_ms())
}

/// Mark a lifecycle transition at a supplied monotonic timestamp.
pub fn mark_at(
    plane: PlaneId,
    owner: &'static str,
    state: PlaneLifecycleState,
    reason: &'static str,
    now_ms: u64,
) -> PlaneLifecycleRecord {
    let mut guard = registry().lock().unwrap_or_else(|e| e.into_inner());
    if let Some(index) = guard
        .records
        .iter()
        .position(|record| record.plane == plane && record.owner == owner)
    {
        let previous_state = guard.records[index].state;
        let generation = if should_start_new_generation(Some(previous_state), state) {
            next_generation(&mut guard)
        } else {
            guard.records[index].generation
        };
        guard.records[index] = PlaneLifecycleRecord {
            plane,
            owner,
            state,
            updated_at_ms: now_ms,
            generation,
            transition_count: guard.records[index].transition_count.saturating_add(1),
            failure_count: guard.records[index].failure_count
                + u32::from(state == PlaneLifecycleState::Failed),
            last_reason: reason,
        };
        guard.records[index]
    } else {
        let generation = next_generation(&mut guard);
        let record = PlaneLifecycleRecord {
            plane,
            owner,
            state,
            updated_at_ms: now_ms,
            generation,
            transition_count: 1,
            failure_count: u32::from(state == PlaneLifecycleState::Failed),
            last_reason: reason,
        };
        guard.records.push(record);
        record
    }
}

/// Snapshot lifecycle state using the process monotonic clock.
pub fn snapshot() -> PlaneLifecycleSnapshot {
    let guard = registry().lock().unwrap_or_else(|e| e.into_inner());
    let mut records = guard.records.clone();
    records.sort_by(|left, right| {
        plane_sort_key(left.plane)
            .cmp(&plane_sort_key(right.plane))
            .then_with(|| left.owner.cmp(right.owner))
    });
    PlaneLifecycleSnapshot {
        total_records: records.len(),
        active_count: records
            .iter()
            .filter(|record| record.state == PlaneLifecycleState::Active)
            .count(),
        suspended_count: records
            .iter()
            .filter(|record| record.state == PlaneLifecycleState::Suspended)
            .count(),
        draining_count: records
            .iter()
            .filter(|record| record.state == PlaneLifecycleState::Draining)
            .count(),
        stopping_count: records
            .iter()
            .filter(|record| record.state == PlaneLifecycleState::Stopping)
            .count(),
        unloaded_count: records
            .iter()
            .filter(|record| record.state == PlaneLifecycleState::Unloaded)
            .count(),
        failed_count: records
            .iter()
            .filter(|record| record.state == PlaneLifecycleState::Failed)
            .count(),
        records,
    }
}

/// Return a compact lifecycle baseline for heartbeat logs.
pub fn format_baseline_log_line() -> String {
    let snapshot = snapshot();
    format!(
        "plane_lifecycle records={} active={} suspended={} draining={} stopping={} unloaded={} failed={}",
        snapshot.total_records,
        snapshot.active_count,
        snapshot.suspended_count,
        snapshot.draining_count,
        snapshot.stopping_count,
        snapshot.unloaded_count,
        snapshot.failed_count
    )
}

#[cfg(test)]
pub(crate) fn reset_for_tests() {
    let mut guard = registry().lock().unwrap_or_else(|e| e.into_inner());
    guard.records.clear();
    guard.next_generation = 1;
}

#[cfg(test)]
pub(crate) fn plane_lifecycle_test_guard() -> std::sync::MutexGuard<'static, ()> {
    static TEST_LOCK: Mutex<()> = Mutex::new(());
    let guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    reset_for_tests();
    guard
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::PlaneId;

    #[test]
    fn lifecycle_transitions_update_snapshot_and_failure_counts() {
        let _guard = plane_lifecycle_test_guard();

        mark(
            PlaneId::StorageWriteBack,
            "write_back",
            PlaneLifecycleState::Registered,
            "registered",
        );
        mark(
            PlaneId::StorageWriteBack,
            "write_back",
            PlaneLifecycleState::Starting,
            "spawn",
        );
        mark(
            PlaneId::StorageWriteBack,
            "write_back",
            PlaneLifecycleState::Active,
            "worker_alive",
        );
        mark(
            PlaneId::StorageWriteBack,
            "write_back",
            PlaneLifecycleState::Failed,
            "panic",
        );

        let snapshot = snapshot();
        assert_eq!(snapshot.total_records, 1);
        assert_eq!(snapshot.active_count, 0);
        assert_eq!(snapshot.failed_count, 1);
        let record = snapshot
            .records
            .iter()
            .find(|record| record.owner == "write_back")
            .expect("write_back record");
        assert_eq!(record.state, PlaneLifecycleState::Failed);
        assert_eq!(record.transition_count, 4);
        assert_eq!(record.failure_count, 1);
        assert_eq!(record.last_reason, "panic");
    }

    #[test]
    fn lifecycle_keys_distinguish_route_workers_with_same_plane_id() {
        let _guard = plane_lifecycle_test_guard();

        mark(
            PlaneId::Diagnostic,
            "http_snapshot",
            PlaneLifecycleState::Active,
            "spawn_ok",
        );
        mark(
            PlaneId::Diagnostic,
            "http_ota",
            PlaneLifecycleState::Active,
            "spawn_ok",
        );

        let snapshot = snapshot();
        assert_eq!(snapshot.total_records, 2);
        assert_eq!(snapshot.active_count, 2);
        assert!(snapshot
            .records
            .iter()
            .any(|record| record.owner == "http_snapshot"));
        assert!(snapshot
            .records
            .iter()
            .any(|record| record.owner == "http_ota"));
    }
}
