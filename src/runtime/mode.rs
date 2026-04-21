//! Unified runtime mode contract for Beetle OS.
//! Beetle OS 统一运行模式契约。

use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeMode {
    Booting,
    Pairing,
    Normal,
    VoiceExclusive,
    Maintenance,
    RecoverySafeMode,
}

impl RuntimeMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Booting => "booting",
            Self::Pairing => "pairing",
            Self::Normal => "normal",
            Self::VoiceExclusive => "voice_exclusive",
            Self::Maintenance => "maintenance",
            Self::RecoverySafeMode => "recovery_safe_mode",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct RuntimeModeActionBudget {
    pub allow_periodic_maintenance: bool,
    pub allow_due_user_timers: bool,
    pub allow_heartbeat_injection: bool,
    pub allow_best_effort_delayed_tasks: bool,
    pub allow_idle_self_runtime: bool,
    pub allow_non_voice_outbound: bool,
    pub allow_external_wss_connect: bool,
    pub require_external_wss_suspended: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RuntimeModeSource {
    pub wifi_sta_connected: bool,
    pub boot_phase_active: bool,
    pub pairing_required: bool,
    pub pairing_state_known: bool,
    pub voice_exclusive_active: bool,
    pub background_maintenance_active: bool,
    pub config_plane_alive: bool,
    pub channel_plane_alive: bool,
    pub voice_plane_alive: bool,
    pub agent_plane_alive: bool,
    pub user_agent_lane_alive: bool,
    pub system_agent_lane_alive: bool,
    pub dual_agent_lanes_alive: bool,
    pub external_wss_managed_present: bool,
    pub external_wss_suspend_requested: bool,
    pub external_wss_suspended: bool,
    pub recovery_safe_mode_active: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct RuntimeModeSnapshot {
    pub current_mode: RuntimeMode,
    pub wifi_sta_connected: bool,
    pub boot_phase_active: bool,
    pub pairing_required: bool,
    pub pairing_state_known: bool,
    pub voice_exclusive_active: bool,
    pub background_maintenance_active: bool,
    pub config_plane_alive: bool,
    pub channel_plane_alive: bool,
    pub voice_plane_alive: bool,
    pub agent_plane_alive: bool,
    pub user_agent_lane_alive: bool,
    pub system_agent_lane_alive: bool,
    pub dual_agent_lanes_alive: bool,
    pub external_wss_managed_present: bool,
    pub external_wss_suspend_requested: bool,
    pub external_wss_suspended: bool,
    pub recovery_safe_mode_active: bool,
    pub action_budget: RuntimeModeActionBudget,
}

impl RuntimeModeSnapshot {
    pub fn mode_block_reason(self) -> Option<&'static str> {
        match self.current_mode {
            RuntimeMode::Booting => Some("boot_phase_active"),
            RuntimeMode::Pairing => Some("pairing_required"),
            RuntimeMode::Normal => None,
            RuntimeMode::VoiceExclusive => Some("voice_exclusive_active"),
            RuntimeMode::Maintenance => Some("background_maintenance_active"),
            RuntimeMode::RecoverySafeMode => Some("recovery_safe_mode"),
        }
    }

    pub fn allows_prompt_governed_recall(
        self,
        pressure: crate::orchestrator::PressureLevel,
    ) -> bool {
        self.action_budget.allow_non_voice_outbound
            && !matches!(pressure, crate::orchestrator::PressureLevel::Critical)
    }

    pub fn allows_prompt_background_governance(
        self,
        pressure: crate::orchestrator::PressureLevel,
    ) -> bool {
        self.current_mode == RuntimeMode::Normal
            && pressure == crate::orchestrator::PressureLevel::Normal
    }

    pub fn allows_prompt_private_depth(self, pressure: crate::orchestrator::PressureLevel) -> bool {
        self.action_budget.allow_idle_self_runtime
            && self.allows_prompt_background_governance(pressure)
    }
}

pub fn snapshot_from_source(source: RuntimeModeSource) -> RuntimeModeSnapshot {
    let current_mode = derive_mode(source);
    RuntimeModeSnapshot {
        current_mode,
        wifi_sta_connected: source.wifi_sta_connected,
        boot_phase_active: source.boot_phase_active,
        pairing_required: source.pairing_required,
        pairing_state_known: source.pairing_state_known,
        voice_exclusive_active: source.voice_exclusive_active,
        background_maintenance_active: source.background_maintenance_active,
        config_plane_alive: source.config_plane_alive,
        channel_plane_alive: source.channel_plane_alive,
        voice_plane_alive: source.voice_plane_alive,
        agent_plane_alive: source.agent_plane_alive,
        user_agent_lane_alive: source.user_agent_lane_alive,
        system_agent_lane_alive: source.system_agent_lane_alive,
        dual_agent_lanes_alive: source.dual_agent_lanes_alive,
        external_wss_managed_present: source.external_wss_managed_present,
        external_wss_suspend_requested: source.external_wss_suspend_requested,
        external_wss_suspended: source.external_wss_suspended,
        recovery_safe_mode_active: source.recovery_safe_mode_active,
        action_budget: action_budget_for_mode(current_mode),
    }
}

fn derive_mode(source: RuntimeModeSource) -> RuntimeMode {
    if source.recovery_safe_mode_active {
        RuntimeMode::RecoverySafeMode
    } else if source.boot_phase_active {
        RuntimeMode::Booting
    } else if source.voice_exclusive_active {
        RuntimeMode::VoiceExclusive
    } else if source.background_maintenance_active {
        RuntimeMode::Maintenance
    } else if source.pairing_state_known && source.pairing_required {
        RuntimeMode::Pairing
    } else {
        RuntimeMode::Normal
    }
}

fn action_budget_for_mode(mode: RuntimeMode) -> RuntimeModeActionBudget {
    match mode {
        RuntimeMode::Booting | RuntimeMode::Pairing => RuntimeModeActionBudget {
            allow_periodic_maintenance: false,
            allow_due_user_timers: false,
            allow_heartbeat_injection: false,
            allow_best_effort_delayed_tasks: false,
            allow_idle_self_runtime: false,
            allow_non_voice_outbound: true,
            allow_external_wss_connect: true,
            require_external_wss_suspended: false,
        },
        RuntimeMode::Normal => RuntimeModeActionBudget {
            allow_periodic_maintenance: true,
            allow_due_user_timers: true,
            allow_heartbeat_injection: true,
            allow_best_effort_delayed_tasks: true,
            allow_idle_self_runtime: true,
            allow_non_voice_outbound: true,
            allow_external_wss_connect: true,
            require_external_wss_suspended: false,
        },
        RuntimeMode::VoiceExclusive => RuntimeModeActionBudget {
            allow_periodic_maintenance: false,
            allow_due_user_timers: false,
            allow_heartbeat_injection: false,
            allow_best_effort_delayed_tasks: false,
            allow_idle_self_runtime: false,
            allow_non_voice_outbound: false,
            allow_external_wss_connect: false,
            require_external_wss_suspended: true,
        },
        RuntimeMode::Maintenance => RuntimeModeActionBudget {
            allow_periodic_maintenance: false,
            allow_due_user_timers: true,
            allow_heartbeat_injection: false,
            allow_best_effort_delayed_tasks: false,
            allow_idle_self_runtime: false,
            allow_non_voice_outbound: true,
            allow_external_wss_connect: true,
            require_external_wss_suspended: false,
        },
        RuntimeMode::RecoverySafeMode => RuntimeModeActionBudget {
            allow_periodic_maintenance: false,
            allow_due_user_timers: false,
            allow_heartbeat_injection: false,
            allow_best_effort_delayed_tasks: false,
            allow_idle_self_runtime: false,
            allow_non_voice_outbound: true,
            allow_external_wss_connect: false,
            require_external_wss_suspended: false,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::{snapshot_from_source, RuntimeMode, RuntimeModeSource};
    use crate::orchestrator::PressureLevel;

    #[test]
    fn recovery_safe_mode_has_highest_priority() {
        let snapshot = snapshot_from_source(RuntimeModeSource {
            boot_phase_active: true,
            pairing_required: true,
            pairing_state_known: true,
            voice_exclusive_active: true,
            background_maintenance_active: true,
            recovery_safe_mode_active: true,
            ..RuntimeModeSource::default()
        });
        assert_eq!(snapshot.current_mode, RuntimeMode::RecoverySafeMode);
        assert!(!snapshot.action_budget.allow_periodic_maintenance);
        assert!(!snapshot.action_budget.allow_external_wss_connect);
    }

    #[test]
    fn voice_exclusive_outranks_pairing_and_blocks_non_voice_outbound() {
        let snapshot = snapshot_from_source(RuntimeModeSource {
            pairing_required: true,
            pairing_state_known: true,
            voice_exclusive_active: true,
            ..RuntimeModeSource::default()
        });
        assert_eq!(snapshot.current_mode, RuntimeMode::VoiceExclusive);
        assert!(!snapshot.action_budget.allow_non_voice_outbound);
        assert!(snapshot.action_budget.require_external_wss_suspended);
    }

    #[test]
    fn maintenance_mode_blocks_more_background_enqueues_but_keeps_user_timers() {
        let snapshot = snapshot_from_source(RuntimeModeSource {
            background_maintenance_active: true,
            ..RuntimeModeSource::default()
        });
        assert_eq!(snapshot.current_mode, RuntimeMode::Maintenance);
        assert!(!snapshot.action_budget.allow_periodic_maintenance);
        assert!(snapshot.action_budget.allow_due_user_timers);
        assert!(!snapshot.action_budget.allow_idle_self_runtime);
    }

    #[test]
    fn pairing_mode_applies_when_known_and_unpaired() {
        let snapshot = snapshot_from_source(RuntimeModeSource {
            pairing_required: true,
            pairing_state_known: true,
            ..RuntimeModeSource::default()
        });
        assert_eq!(snapshot.current_mode, RuntimeMode::Pairing);
        assert!(!snapshot.action_budget.allow_heartbeat_injection);
    }

    #[test]
    fn voice_exclusive_prompt_budget_blocks_governed_and_background_layers() {
        let snapshot = snapshot_from_source(RuntimeModeSource {
            voice_exclusive_active: true,
            ..RuntimeModeSource::default()
        });

        assert!(!snapshot.allows_prompt_governed_recall(PressureLevel::Normal));
        assert!(!snapshot.allows_prompt_background_governance(PressureLevel::Normal));
        assert!(!snapshot.allows_prompt_private_depth(PressureLevel::Normal));
    }

    #[test]
    fn normal_prompt_budget_allows_background_only_under_normal_pressure() {
        let snapshot = snapshot_from_source(RuntimeModeSource::default());

        assert!(snapshot.allows_prompt_governed_recall(PressureLevel::Normal));
        assert!(snapshot.allows_prompt_background_governance(PressureLevel::Normal));
        assert!(snapshot.allows_prompt_private_depth(PressureLevel::Normal));
        assert!(snapshot.allows_prompt_governed_recall(PressureLevel::Cautious));
        assert!(!snapshot.allows_prompt_background_governance(PressureLevel::Cautious));
        assert!(!snapshot.allows_prompt_private_depth(PressureLevel::Cautious));
        assert!(!snapshot.allows_prompt_governed_recall(PressureLevel::Critical));
    }
}
