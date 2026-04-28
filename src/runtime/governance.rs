//! Runtime governance guards and source-sync helpers.
//! 运行态治理 guard 与 source 同步辅助。

use crate::platform::ConfigStore;
use serde::Serialize;
use std::sync::{Mutex, OnceLock};

pub const CONFIG_ACTIVITY_WINDOW_SECS: u64 = 30;
const CONFIG_READ_BURST_LEASE_TTL_MS: u64 = 5_000;

pub struct ConfigPlaneGuard;

impl ConfigPlaneGuard {
    pub fn enter() -> Self {
        crate::state::set_config_plane_active(true);
        Self
    }
}

impl Drop for ConfigPlaneGuard {
    fn drop(&mut self) {
        crate::state::set_config_plane_active(false);
    }
}

pub struct BackgroundMaintenanceGuard;

impl BackgroundMaintenanceGuard {
    pub fn enter() -> Self {
        crate::state::set_background_maintenance_active(true);
        Self
    }
}

impl Drop for BackgroundMaintenanceGuard {
    fn drop(&mut self) {
        crate::state::set_background_maintenance_active(false);
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigActivityPhase {
    #[default]
    Idle,
    Starting,
    Active,
    Persisting,
    Success,
    Fail,
    Stopping,
    Cleanup,
}

impl ConfigActivityPhase {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Starting => "starting",
            Self::Active => "active",
            Self::Persisting => "persisting",
            Self::Success => "success",
            Self::Fail => "fail",
            Self::Stopping => "stopping",
            Self::Cleanup => "cleanup",
        }
    }

    pub fn blocks_new_non_voice_network_work(self) -> bool {
        matches!(self, Self::Persisting | Self::Stopping)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ConfigActivitySnapshot {
    pub active: bool,
    pub phase: ConfigActivityPhase,
    pub route: Option<String>,
    pub active_until_secs: u64,
}

#[derive(Clone, Debug)]
struct ConfigActivityState {
    phase: ConfigActivityPhase,
    route: Option<String>,
    active_until_secs: u64,
}

impl Default for ConfigActivityState {
    fn default() -> Self {
        Self {
            phase: ConfigActivityPhase::Idle,
            route: None,
            active_until_secs: 0,
        }
    }
}

fn config_activity_state() -> &'static Mutex<ConfigActivityState> {
    static STATE: OnceLock<Mutex<ConfigActivityState>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(ConfigActivityState::default()))
}

fn extend_config_activity_at(phase: ConfigActivityPhase, route: &str, now_secs: u64) {
    let mut state = config_activity_state()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    state.phase = phase;
    state.route = Some(route.to_string());
    state.active_until_secs = now_secs.saturating_add(CONFIG_ACTIVITY_WINDOW_SECS);
}

pub fn config_activity_active() -> bool {
    config_activity_active_at(crate::util::current_unix_secs())
}

pub(crate) fn config_activity_active_at(now_secs: u64) -> bool {
    config_activity_snapshot_at(now_secs).active
}

pub fn config_activity_snapshot() -> ConfigActivitySnapshot {
    config_activity_snapshot_at(crate::util::current_unix_secs())
}

pub(crate) fn config_activity_snapshot_at(now_secs: u64) -> ConfigActivitySnapshot {
    let mut state = config_activity_state()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if state.active_until_secs == 0 || state.active_until_secs < now_secs {
        state.phase = ConfigActivityPhase::Idle;
        state.route = None;
        state.active_until_secs = 0;
    }
    ConfigActivitySnapshot {
        active: state.active_until_secs >= now_secs && state.active_until_secs != 0,
        phase: state.phase,
        route: state.route.clone(),
        active_until_secs: state.active_until_secs,
    }
}

pub struct ConfigActivityGuard {
    route: &'static str,
    finished: bool,
    external_wss_suspend: Option<crate::network::ExternalWssSuspendGuard>,
}

#[derive(Debug)]
struct ConfigReadBurstLeaseGuard {
    owner: crate::runtime::lease::LeaseOwner,
    token: u64,
}

impl Drop for ConfigReadBurstLeaseGuard {
    fn drop(&mut self) {
        let _ = crate::runtime::lease::release_token(
            crate::runtime::lease::LeaseKind::ConfigReadBurst,
            self.owner,
            self.token,
        );
    }
}

pub struct ConfigReadBurstGuard {
    finished: bool,
    _lease: Option<ConfigReadBurstLeaseGuard>,
}

impl ConfigReadBurstGuard {
    pub fn enter(route: &'static str) -> Self {
        let owner = crate::runtime::lease::LeaseOwner::new("config_recovery", "http_config_read");
        let decision = crate::runtime::lease::try_acquire(
            crate::runtime::lease::LeaseKind::ConfigReadBurst,
            owner,
            crate::runtime::lease::LeaseMode::Shared,
            Some(CONFIG_READ_BURST_LEASE_TTL_MS),
        );
        Self::from_decision(route, owner, decision)
    }

    #[cfg(test)]
    pub(crate) fn enter_at(route: &'static str, now_ms: u64) -> Self {
        let owner = crate::runtime::lease::LeaseOwner::new("config_recovery", "http_config_read");
        let decision = crate::runtime::lease::try_acquire_at(
            crate::runtime::lease::LeaseKind::ConfigReadBurst,
            owner,
            crate::runtime::lease::LeaseMode::Shared,
            Some(CONFIG_READ_BURST_LEASE_TTL_MS),
            now_ms,
        );
        Self::from_decision(route, owner, decision)
    }

    fn from_decision(
        route: &'static str,
        owner: crate::runtime::lease::LeaseOwner,
        decision: crate::runtime::lease::LeaseDecision,
    ) -> Self {
        let lease = match decision {
            crate::runtime::lease::LeaseDecision::Acquired(record)
            | crate::runtime::lease::LeaseDecision::Reentered(record)
            | crate::runtime::lease::LeaseDecision::ReplacedExpired {
                current: record, ..
            } => Some(ConfigReadBurstLeaseGuard {
                owner,
                token: record.token,
            }),
            crate::runtime::lease::LeaseDecision::Denied(denial) => {
                log::warn!(
                    "[config_recovery] config read burst lease denied route={} reason={} held_by={:?}",
                    route,
                    denial.reason,
                    denial.held_by
                );
                None
            }
        };
        let _ = crate::runtime::plane_lifecycle::mark(
            crate::runtime::PlaneId::ConfigRecovery,
            "http_config_read",
            crate::runtime::PlaneLifecycleState::Active,
            "config_read_burst",
        );
        Self {
            finished: false,
            _lease: lease,
        }
    }

    pub fn finish_status(&mut self, status: u16) {
        let (state, reason) = if status >= 400 {
            (
                crate::runtime::PlaneLifecycleState::Failed,
                "config_read_failed",
            )
        } else {
            (
                crate::runtime::PlaneLifecycleState::Stopping,
                "config_read_complete",
            )
        };
        let _ = crate::runtime::plane_lifecycle::mark(
            crate::runtime::PlaneId::ConfigRecovery,
            "http_config_read",
            state,
            reason,
        );
        self.finished = true;
    }
}

impl Drop for ConfigReadBurstGuard {
    fn drop(&mut self) {
        if !self.finished {
            let _ = crate::runtime::plane_lifecycle::mark(
                crate::runtime::PlaneId::ConfigRecovery,
                "http_config_read",
                crate::runtime::PlaneLifecycleState::Failed,
                "config_read_aborted",
            );
        } else {
            let _ = crate::runtime::plane_lifecycle::mark(
                crate::runtime::PlaneId::ConfigRecovery,
                "http_config_read",
                crate::runtime::PlaneLifecycleState::Unloaded,
                "config_read_released",
            );
        }
    }
}

impl ConfigActivityGuard {
    pub fn enter(phase: ConfigActivityPhase, route: &'static str) -> Self {
        Self::enter_at(phase, route, crate::util::current_unix_secs())
    }

    pub(crate) fn enter_at(phase: ConfigActivityPhase, route: &'static str, now_secs: u64) -> Self {
        extend_config_activity_at(phase, route, now_secs);
        let external_wss_suspend = if phase.blocks_new_non_voice_network_work() {
            let guard = crate::network::begin_external_wss_suspend_request();
            crate::network::wait_for_external_wss_suspend(route);
            Some(guard)
        } else {
            None
        };
        Self {
            route,
            finished: false,
            external_wss_suspend,
        }
    }

    pub fn finish_status(&mut self, status: u16) {
        self.finish_status_at(status, crate::util::current_unix_secs());
    }

    pub(crate) fn finish_status_at(&mut self, status: u16, now_secs: u64) {
        let phase = if status >= 400 {
            ConfigActivityPhase::Fail
        } else {
            ConfigActivityPhase::Success
        };
        extend_config_activity_at(phase, self.route, now_secs);
        self.external_wss_suspend.take();
        self.finished = true;
    }
}

impl Drop for ConfigActivityGuard {
    fn drop(&mut self) {
        if !self.finished {
            extend_config_activity_at(
                ConfigActivityPhase::Fail,
                self.route,
                crate::util::current_unix_secs(),
            );
        }
    }
}

pub fn sync_pairing_state_from_store(store: &dyn ConfigStore) {
    crate::state::set_pairing_state_known(true);
    crate::state::set_pairing_required(!crate::platform::pairing::code_set(store));
}

pub fn set_recovery_safe_mode_active(active: bool) {
    crate::state::set_recovery_safe_mode_active(active);
}

pub fn set_upgrade_active(active: bool) {
    crate::state::set_upgrade_active(active);
}

#[cfg(test)]
pub fn set_pairing_state_for_tests(known: bool, required: bool) {
    crate::state::set_pairing_state_known(known);
    crate::state::set_pairing_required(required);
}

#[cfg(test)]
pub fn reset_runtime_governance_state_for_tests() {
    crate::state::set_voice_exclusive_active(false);
    crate::state::set_background_maintenance_active(false);
    crate::state::set_config_plane_active(false);
    crate::state::set_boot_phase_active(false);
    crate::state::set_pairing_state_known(false);
    crate::state::set_pairing_required(false);
    crate::state::set_recovery_safe_mode_active(false);
    crate::state::set_upgrade_active(false);
    let mut state = config_activity_state()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    *state = ConfigActivityState::default();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guards_round_trip_runtime_flags() {
        crate::state::set_config_plane_active(false);
        crate::state::set_background_maintenance_active(false);

        {
            let _config = ConfigPlaneGuard::enter();
            let _bg = BackgroundMaintenanceGuard::enter();
            assert!(crate::state::config_plane_active());
            assert!(crate::state::background_maintenance_active());
        }

        assert!(!crate::state::config_plane_active());
        assert!(!crate::state::background_maintenance_active());
    }

    #[test]
    fn config_activity_guard_extends_window_after_request_completion() {
        let _state_guard = crate::state::test_state_guard();
        reset_runtime_governance_state_for_tests();
        crate::network::set_external_wss_managed_present(false);

        {
            let mut guard = ConfigActivityGuard::enter_at(
                ConfigActivityPhase::Persisting,
                "/api/config/system",
                100,
            );
            assert!(crate::network::external_wss_suspend_requested());
            assert!(config_activity_active_at(100));
            let snapshot = config_activity_snapshot_at(100);
            assert_eq!(snapshot.phase, ConfigActivityPhase::Persisting);
            assert_eq!(snapshot.route.as_deref(), Some("/api/config/system"));
            guard.finish_status_at(200, 105);
            assert!(!crate::network::external_wss_suspend_requested());
        }

        let snapshot = config_activity_snapshot_at(106);
        assert!(snapshot.active);
        assert_eq!(snapshot.phase, ConfigActivityPhase::Success);
        assert_eq!(snapshot.active_until_secs, 135);
        assert!(!config_activity_active_at(136));
        assert_eq!(
            config_activity_snapshot_at(136).phase,
            ConfigActivityPhase::Idle
        );
    }

    #[test]
    fn unfinished_config_activity_drops_to_fail_without_blocking_wss_resume() {
        let _state_guard = crate::state::test_state_guard();
        reset_runtime_governance_state_for_tests();
        crate::network::set_external_wss_managed_present(false);

        {
            let _guard = ConfigActivityGuard::enter_at(
                ConfigActivityPhase::Persisting,
                "/api/config/system",
                200,
            );
            assert!(crate::network::external_wss_suspend_requested());
        }

        assert!(!crate::network::external_wss_suspend_requested());
        let snapshot = config_activity_snapshot_at(201);
        assert!(snapshot.active);
        assert_eq!(snapshot.phase, ConfigActivityPhase::Fail);
        assert!(!snapshot.phase.blocks_new_non_voice_network_work());
    }

    #[test]
    fn config_read_burst_guard_marks_config_recovery_lease_and_lifecycle() {
        let _lease_guard = crate::runtime::lease::lease_test_guard();
        let _lifecycle_guard = crate::runtime::plane_lifecycle::plane_lifecycle_test_guard();

        {
            let mut guard = ConfigReadBurstGuard::enter_at("/api/config/system", 100);
            assert_eq!(
                crate::runtime::lease::active_count_for_kind_at(
                    crate::runtime::lease::LeaseKind::ConfigReadBurst,
                    101
                ),
                1
            );
            let snapshot = crate::runtime::plane_lifecycle::snapshot();
            let record = snapshot
                .records
                .iter()
                .find(|record| {
                    record.plane == crate::runtime::PlaneId::ConfigRecovery
                        && record.owner == "http_config_read"
                })
                .expect("config read lifecycle record");
            assert_eq!(record.state, crate::runtime::PlaneLifecycleState::Active);
            assert_eq!(record.last_reason, "config_read_burst");

            guard.finish_status(200);
        }

        assert_eq!(
            crate::runtime::lease::active_count_for_kind_at(
                crate::runtime::lease::LeaseKind::ConfigReadBurst,
                102
            ),
            0
        );
        let snapshot = crate::runtime::plane_lifecycle::snapshot();
        let record = snapshot
            .records
            .iter()
            .find(|record| {
                record.plane == crate::runtime::PlaneId::ConfigRecovery
                    && record.owner == "http_config_read"
            })
            .expect("config read lifecycle record after drop");
        assert_eq!(record.state, crate::runtime::PlaneLifecycleState::Unloaded);
        assert_eq!(record.last_reason, "config_read_released");
    }
}
