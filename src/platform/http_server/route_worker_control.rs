//! Host-testable route worker control contracts for ESP transport.

use super::router::catalog::{
    route_worker_memory_requirements, RouteExecutionClass, RouteWorkerContract, RouteWorkerLane,
    RouteWorkerMemoryRequirements,
};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const ESP_ROUTE_WORKER_MIN_LARGEST_BLOCK_BYTES: usize = 32 * 1024;
const ESP_ROUTE_WORKER_START_BACKOFF_SECS: u64 = 10;

#[derive(Clone)]
pub(crate) struct RouteStartCooldown {
    until: Arc<Mutex<Option<Instant>>>,
}

impl RouteStartCooldown {
    pub(crate) fn new() -> Self {
        Self {
            until: Arc::new(Mutex::new(None)),
        }
    }

    pub(crate) fn record_failure(&self) {
        let mut until = self
            .until
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *until = Some(Instant::now() + Duration::from_secs(ESP_ROUTE_WORKER_START_BACKOFF_SECS));
    }

    pub(crate) fn clear(&self) {
        let mut until = self
            .until
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *until = None;
    }

    pub(crate) fn reject_detail(&self, lane: RouteWorkerLane) -> Option<String> {
        let mut until = self
            .until
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let deadline = (*until)?;
        let now = Instant::now();
        if now >= deadline {
            *until = None;
            return None;
        }
        Some(format!(
            "route worker start cooling down for {:?}: retry_after_ms={}",
            lane,
            deadline.saturating_duration_since(now).as_millis()
        ))
    }
}

pub(crate) fn route_worker_spawn_contract(lane: RouteWorkerLane) -> RouteWorkerContract {
    match lane {
        RouteWorkerLane::Snapshot => RouteExecutionClass::SnapshotRoute,
        RouteWorkerLane::Config => RouteExecutionClass::AsyncConfigRoute,
        RouteWorkerLane::Diagnostic => RouteExecutionClass::SlowDiagnosticRoute,
    }
    .worker_contract()
    .expect("route worker lane must have a spawn contract")
}

pub(crate) fn effective_route_worker_memory_requirements(
    contract: RouteWorkerContract,
) -> RouteWorkerMemoryRequirements {
    let direct = route_worker_memory_requirements(contract);
    let spawn = route_worker_memory_requirements(route_worker_spawn_contract(contract.lane));
    let largest_floor = match contract.lane {
        RouteWorkerLane::Snapshot | RouteWorkerLane::Diagnostic => {
            ESP_ROUTE_WORKER_MIN_LARGEST_BLOCK_BYTES
        }
        RouteWorkerLane::Config => 0,
    };
    RouteWorkerMemoryRequirements {
        required_internal: direct.required_internal.max(spawn.required_internal),
        required_largest: direct
            .required_largest
            .max(spawn.required_largest)
            .max(largest_floor),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostic_lane_admission_uses_spawn_contract_even_for_local_routes() {
        let local = RouteExecutionClass::LocalDiagnosticRoute
            .worker_contract()
            .expect("local diagnostic contract");
        let slow = RouteExecutionClass::SlowDiagnosticRoute
            .worker_contract()
            .expect("slow diagnostic contract");
        let local_direct = route_worker_memory_requirements(local);
        let slow_spawn = route_worker_memory_requirements(slow);
        let effective = effective_route_worker_memory_requirements(local);

        assert!(
            local_direct.required_largest < slow_spawn.required_largest,
            "local diagnostic route has weaker direct requirements than the shared lane spawn"
        );
        assert_eq!(effective.required_largest, slow_spawn.required_largest);
        assert_eq!(effective.required_internal, slow_spawn.required_internal);
    }

    #[test]
    fn snapshot_worker_admission_preserves_global_largest_block_floor() {
        let snapshot = RouteExecutionClass::SnapshotRoute
            .worker_contract()
            .expect("snapshot contract");
        let direct = route_worker_memory_requirements(snapshot);
        let effective = effective_route_worker_memory_requirements(snapshot);

        assert!(direct.required_largest < ESP_ROUTE_WORKER_MIN_LARGEST_BLOCK_BYTES);
        assert_eq!(
            effective.required_largest,
            ESP_ROUTE_WORKER_MIN_LARGEST_BLOCK_BYTES
        );
    }

    #[test]
    fn route_worker_start_failure_enters_short_cooldown() {
        let cooldown = RouteStartCooldown::new();

        assert!(cooldown.reject_detail(RouteWorkerLane::Snapshot).is_none());
        cooldown.record_failure();
        let detail = cooldown
            .reject_detail(RouteWorkerLane::Snapshot)
            .expect("cooldown should reject immediately after a spawn failure");
        assert!(detail.contains("Snapshot"));

        cooldown.clear();
        assert!(cooldown.reject_detail(RouteWorkerLane::Snapshot).is_none());
    }
}
