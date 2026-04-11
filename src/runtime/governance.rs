//! Runtime governance guards and source-sync helpers.
//! 运行态治理 guard 与 source 同步辅助。

use crate::platform::ConfigStore;

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

pub fn sync_pairing_state_from_store(store: &dyn ConfigStore) {
    crate::state::set_pairing_state_known(true);
    crate::state::set_pairing_required(!crate::platform::pairing::code_set(store));
}

pub fn set_recovery_safe_mode_active(active: bool) {
    crate::state::set_recovery_safe_mode_active(active);
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
}
