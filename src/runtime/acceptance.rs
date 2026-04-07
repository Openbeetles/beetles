//! Beetle OS closure and acceptance gate.
//! Beetle OS 全系统闭环与验收门禁。

use crate::runtime::{
    InitiativeAction, InitiativeSnapshot, InitiativeSuppressionReason, PresenceSnapshot,
    PresenceState, RuntimeMode, RuntimeModeSnapshot, SoulKernelStatus,
};
use crate::Platform;
use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BeetleOsPlane {
    RuntimeMode,
    Presence,
    SoulKernel,
    Initiative,
    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
    Supervisor,
    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
    Release,
}

impl BeetleOsPlane {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RuntimeMode => "runtime_mode",
            Self::Presence => "presence",
            Self::SoulKernel => "soul_kernel",
            Self::Initiative => "initiative",
            #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
            Self::Supervisor => "supervisor",
            #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
            Self::Release => "release",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct BeetleOsPlaneReport {
    pub plane: BeetleOsPlane,
    pub ready: bool,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub outstanding: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct BeetleOsClosureReport {
    pub ready: bool,
    pub summary: String,
    pub current_mode: String,
    pub presence_state: String,
    pub plane_count: usize,
    pub ready_planes: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub outstanding: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub planes: Vec<BeetleOsPlaneReport>,
}

pub fn inspect_platform_beetle_os_closure(
    platform: &dyn Platform,
    now_secs: u64,
) -> BeetleOsClosureReport {
    let presence = crate::runtime::inspect_platform_presence(platform, now_secs);
    let initiative = crate::runtime::inspect_platform_initiative(platform, now_secs);
    inspect_beetle_os_closure(&presence, &initiative)
}

pub fn inspect_beetle_os_closure(
    presence: &PresenceSnapshot,
    initiative: &InitiativeSnapshot,
) -> BeetleOsClosureReport {
    let planes = {
        let planes = vec![
            inspect_runtime_mode_plane(presence.runtime_mode),
            inspect_presence_plane(presence),
            inspect_soul_kernel_plane(&presence.soul_kernel),
            inspect_initiative_plane(initiative),
        ];
        #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
        {
            let mut planes = planes;
            planes.push(inspect_supervisor_plane(presence.supervisor.as_ref()));
            planes.push(inspect_release_plane(presence.release.as_ref()));
            planes
        }
        #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
        {
            planes
        }
    };
    let ready_planes = planes.iter().filter(|plane| plane.ready).count();
    let outstanding = planes
        .iter()
        .filter(|plane| !plane.ready)
        .flat_map(|plane| {
            plane
                .outstanding
                .iter()
                .map(move |reason| format!("{}:{}", plane.plane.as_str(), reason))
        })
        .collect::<Vec<_>>();
    let ready = outstanding.is_empty();
    let summary = if ready {
        "beetle_os_closure_ready".to_string()
    } else {
        format!(
            "beetle_os_closure_blocked:{}",
            outstanding
                .first()
                .cloned()
                .unwrap_or_else(|| "unknown".to_string())
        )
    };
    BeetleOsClosureReport {
        ready,
        summary,
        current_mode: presence.runtime_mode.current_mode.as_str().to_string(),
        presence_state: presence.state.as_str().to_string(),
        plane_count: planes.len(),
        ready_planes,
        outstanding,
        planes,
    }
}

fn plane_report(
    plane: BeetleOsPlane,
    healthy_summary: &str,
    outstanding: Vec<String>,
) -> BeetleOsPlaneReport {
    let ready = outstanding.is_empty();
    BeetleOsPlaneReport {
        plane,
        ready,
        summary: if ready {
            healthy_summary.to_string()
        } else {
            format!("{}_blocked:{}", plane.as_str(), outstanding.join(","))
        },
        outstanding,
    }
}

fn inspect_runtime_mode_plane(mode: RuntimeModeSnapshot) -> BeetleOsPlaneReport {
    let mut outstanding = Vec::new();
    match mode.current_mode {
        RuntimeMode::Booting => {
            if !mode.boot_phase_active {
                outstanding.push("booting_mode_without_boot_phase".to_string());
            }
        }
        RuntimeMode::Pairing => {
            if !(mode.pairing_state_known && mode.pairing_required) {
                outstanding.push("pairing_mode_without_pairing_requirement".to_string());
            }
        }
        RuntimeMode::VoiceExclusive => {
            if !mode.voice_exclusive_active {
                outstanding.push("voice_exclusive_mode_without_voice_flag".to_string());
            }
            if mode.action_budget.allow_non_voice_outbound {
                outstanding.push("voice_exclusive_allows_non_voice_outbound".to_string());
            }
            if mode.action_budget.allow_external_wss_connect {
                outstanding.push("voice_exclusive_allows_external_wss_connect".to_string());
            }
            if !mode.action_budget.require_external_wss_suspended {
                outstanding.push("voice_exclusive_does_not_require_wss_suspend".to_string());
            }
        }
        RuntimeMode::Maintenance => {
            if !mode.background_maintenance_active {
                outstanding.push("maintenance_mode_without_background_flag".to_string());
            }
        }
        RuntimeMode::RecoverySafeMode => {
            if !mode.recovery_safe_mode_active {
                outstanding.push("recovery_mode_without_safe_mode_flag".to_string());
            }
        }
        RuntimeMode::Normal => {
            if mode.boot_phase_active {
                outstanding.push("normal_mode_with_boot_phase".to_string());
            }
            if mode.recovery_safe_mode_active {
                outstanding.push("normal_mode_with_safe_mode_flag".to_string());
            }
        }
    }
    plane_report(
        BeetleOsPlane::RuntimeMode,
        "runtime_mode_contract_ready",
        outstanding,
    )
}

fn inspect_presence_plane(presence: &PresenceSnapshot) -> BeetleOsPlaneReport {
    let mut outstanding = Vec::new();
    if presence.headline.trim().is_empty() {
        outstanding.push("missing_presence_headline".to_string());
    }
    if presence.subtitle.trim().is_empty() {
        outstanding.push("missing_presence_subtitle".to_string());
    }
    if presence.rationale.trim().is_empty() {
        outstanding.push("missing_presence_rationale".to_string());
    }
    match presence.state {
        PresenceState::Booting => {
            if presence.runtime_mode.current_mode != RuntimeMode::Booting {
                outstanding.push("booting_presence_without_boot_mode".to_string());
            }
        }
        PresenceState::Pairing => {
            if presence.runtime_mode.current_mode != RuntimeMode::Pairing {
                outstanding.push("pairing_presence_without_pairing_mode".to_string());
            }
        }
        PresenceState::Recovery => {
            if !presence.runtime_mode.recovery_safe_mode_active
                && presence.soul_kernel.safe_mode_minimum_readable
            {
                outstanding.push("recovery_presence_without_recovery_trigger".to_string());
            }
        }
        PresenceState::Fault => {
            if !presence.critical_pressure && presence.soul_kernel.minimum_viable {
                outstanding.push("fault_presence_without_fault_trigger".to_string());
            }
        }
        PresenceState::NoWifi => {
            if presence.wifi_connected {
                outstanding.push("no_wifi_presence_while_connected".to_string());
            }
        }
        PresenceState::Idle => {
            if presence.busy {
                outstanding.push("idle_presence_marked_busy".to_string());
            }
        }
        PresenceState::Busy => {
            if !presence.busy {
                outstanding.push("busy_presence_without_busy_flag".to_string());
            }
        }
        PresenceState::Listening => {
            if !presence.audio_recording {
                outstanding.push("listening_presence_without_recording".to_string());
            }
        }
        PresenceState::Speaking => {
            if !presence.audio_playing {
                outstanding.push("speaking_presence_without_playback".to_string());
            }
        }
    }
    plane_report(
        BeetleOsPlane::Presence,
        "presence_contract_ready",
        outstanding,
    )
}

fn inspect_soul_kernel_plane(soul_kernel: &SoulKernelStatus) -> BeetleOsPlaneReport {
    let mut outstanding = Vec::new();
    if soul_kernel.degraded && soul_kernel.degradation_reasons.is_empty() {
        outstanding.push("degraded_kernel_without_reason".to_string());
    }
    if !soul_kernel.degraded && !soul_kernel.degradation_reasons.is_empty() {
        outstanding.push("kernel_has_degradation_reasons_without_flag".to_string());
    }
    if !soul_kernel.expected_bootstrap_empty
        && !soul_kernel.minimum_viable
        && !soul_kernel.safe_mode_minimum_readable
    {
        outstanding.push("kernel_unrecoverable".to_string());
    }
    plane_report(
        BeetleOsPlane::SoulKernel,
        "soul_kernel_contract_ready",
        outstanding,
    )
}

fn inspect_initiative_plane(initiative: &InitiativeSnapshot) -> BeetleOsPlaneReport {
    let mut outstanding = Vec::new();
    if initiative.ready {
        if initiative.action == InitiativeAction::Hold {
            outstanding.push("ready_initiative_cannot_hold".to_string());
        }
        if initiative.suppression_reason.is_some() {
            outstanding.push("ready_initiative_has_suppression".to_string());
        }
        if initiative.target.is_none() {
            outstanding.push("ready_initiative_missing_target".to_string());
        }
        if initiative.signal.is_none() {
            outstanding.push("ready_initiative_missing_signal".to_string());
        }
        if initiative
            .message_preview
            .as_deref()
            .is_none_or(|value| value.trim().is_empty())
        {
            outstanding.push("ready_initiative_missing_preview".to_string());
        }
    } else {
        if initiative.action != InitiativeAction::Hold {
            outstanding.push("suppressed_initiative_keeps_non_hold_action".to_string());
        }
        if initiative.suppression_reason.is_none() {
            outstanding.push("suppressed_initiative_missing_reason".to_string());
        }
        if initiative.suppression_reason == Some(InitiativeSuppressionReason::CooldownActive)
            && (initiative.last_triggered_at.is_none() || initiative.next_allowed_at.is_none())
        {
            outstanding.push("cooldown_suppression_missing_window".to_string());
        }
        if initiative.suppression_reason != Some(InitiativeSuppressionReason::CooldownActive)
            && initiative.next_allowed_at.is_some()
        {
            outstanding.push("non_cooldown_suppression_exposes_next_allowed_at".to_string());
        }
    }
    plane_report(
        BeetleOsPlane::Initiative,
        "initiative_contract_ready",
        outstanding,
    )
}

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
fn inspect_supervisor_plane(
    supervisor: Option<&crate::runtime::linux_supervisor::LinuxSupervisorStatusSnapshot>,
) -> BeetleOsPlaneReport {
    let mut outstanding = Vec::new();
    let Some(supervisor) = supervisor else {
        outstanding.push("supervisor_status_missing".to_string());
        return plane_report(
            BeetleOsPlane::Supervisor,
            "linux_supervisor_contract_ready",
            outstanding,
        );
    };
    if !supervisor.supervisor_alive {
        outstanding.push("supervisor_not_alive".to_string());
    }
    if supervisor.state.current_state == "running" && !supervisor.agent_alive {
        outstanding.push("running_supervisor_without_agent".to_string());
    }
    if supervisor.state.current_state == "safe_mode"
        && supervisor
            .state
            .safe_mode_reason
            .as_deref()
            .is_none_or(|reason| reason.trim().is_empty())
    {
        outstanding.push("safe_mode_without_reason".to_string());
    }
    if supervisor
        .state
        .safe_mode_reason
        .as_deref()
        .is_some_and(|reason| reason.trim().is_empty())
    {
        outstanding.push("empty_safe_mode_reason".to_string());
    }
    plane_report(
        BeetleOsPlane::Supervisor,
        "linux_supervisor_contract_ready",
        outstanding,
    )
}

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
fn inspect_release_plane(
    release: Option<&crate::runtime::LinuxReleaseStatus>,
) -> BeetleOsPlaneReport {
    let mut outstanding = Vec::new();
    let Some(release) = release else {
        outstanding.push("release_status_missing".to_string());
        return plane_report(
            BeetleOsPlane::Release,
            "linux_release_contract_ready",
            outstanding,
        );
    };
    if !release.managed {
        outstanding.push("linux_release_unmanaged".to_string());
    }
    if release.managed && release.current.is_none() {
        outstanding.push("managed_release_missing_current_pointer".to_string());
    }
    if !release.state_schema_current {
        outstanding.push("state_schema_outdated".to_string());
    }
    if release.rollout_state == crate::runtime::LinuxReleaseRolloutState::RollbackTriggered {
        outstanding.push("release_stuck_in_rollback_triggered".to_string());
    }
    if release.systemd_unit_consistent == Some(false) {
        outstanding.push("systemd_unit_inconsistent".to_string());
    }
    if release.init_script_consistent == Some(false) {
        outstanding.push("init_script_inconsistent".to_string());
    }
    plane_report(
        BeetleOsPlane::Release,
        "linux_release_contract_ready",
        outstanding,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::display::DisplaySystemState;
    use crate::runtime::soul_kernel::{SoulKernelLayerStatus, SoulKernelRuntimeBundleStatus};
    use crate::runtime::{
        InitiativeTarget, LinuxReleasePointer, LinuxReleaseRolloutState, PresenceState,
        RuntimeModeActionBudget,
    };

    fn runtime_mode(mode: RuntimeMode) -> RuntimeModeSnapshot {
        let action_budget = match mode {
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
        };
        RuntimeModeSnapshot {
            current_mode: mode,
            wifi_sta_connected: true,
            boot_phase_active: mode == RuntimeMode::Booting,
            pairing_required: mode == RuntimeMode::Pairing,
            pairing_state_known: mode == RuntimeMode::Pairing,
            voice_exclusive_active: mode == RuntimeMode::VoiceExclusive,
            background_maintenance_active: mode == RuntimeMode::Maintenance,
            config_plane_alive: false,
            channel_plane_alive: false,
            voice_plane_alive: false,
            agent_plane_alive: false,
            user_agent_lane_alive: false,
            system_agent_lane_alive: false,
            dual_agent_lanes_alive: false,
            external_wss_managed_present: false,
            external_wss_suspend_requested: mode == RuntimeMode::VoiceExclusive,
            external_wss_suspended: mode == RuntimeMode::VoiceExclusive,
            supervisor_present: true,
            supervisor_alive: true,
            supervisor_agent_alive: true,
            recovery_safe_mode_active: mode == RuntimeMode::RecoverySafeMode,
            action_budget,
        }
    }

    fn soul_kernel() -> SoulKernelStatus {
        SoulKernelStatus {
            subject_id: "board".to_string(),
            session_chat_count: 1,
            active_chat_ids: vec!["chat-1".to_string()],
            self_model: SoulKernelLayerStatus {
                readable: true,
                present: true,
                updated_at: Some(1),
                error: None,
            },
            self_authored_core: SoulKernelLayerStatus {
                readable: true,
                present: true,
                updated_at: Some(1),
                error: None,
            },
            core_revision_ledger: SoulKernelLayerStatus {
                readable: true,
                present: true,
                updated_at: Some(1),
                error: None,
            },
            self_continuity: SoulKernelLayerStatus {
                readable: true,
                present: true,
                updated_at: Some(1),
                error: None,
            },
            key_memory_readable: true,
            key_memory_count: 4,
            expected_bootstrap_empty: false,
            minimum_viable: true,
            safe_mode_minimum_readable: true,
            degraded: false,
            degradation_reasons: Vec::new(),
            runtime_bundle: SoulKernelRuntimeBundleStatus {
                present: true,
                loadable: true,
                snapshot_count: 1,
                primary_chat_id: Some("chat-1".to_string()),
                reason: None,
                flushed_at: Some(1),
                error: None,
            },
        }
    }

    fn presence() -> PresenceSnapshot {
        PresenceSnapshot {
            state: PresenceState::Idle,
            display_state: DisplaySystemState::Idle,
            headline: "READY".to_string(),
            subtitle: "present and waiting".to_string(),
            rationale: "idle".to_string(),
            busy: false,
            wifi_connected: true,
            pairing_required: false,
            audio_recording: false,
            audio_playing: false,
            critical_pressure: false,
            display_sleep_candidate: true,
            runtime_mode: runtime_mode(RuntimeMode::Normal),
            soul_kernel: soul_kernel(),
            supervisor: Some(
                crate::runtime::linux_supervisor::LinuxSupervisorStatusSnapshot {
                    supervisor_alive: true,
                    agent_alive: true,
                    state: crate::runtime::linux_supervisor::LinuxSupervisorState {
                        supervisor_pid: 1,
                        current_state: "running".to_string(),
                        started_at: 1,
                        last_start_at: 1,
                        restart_count: 0,
                        failure_burst_count: 0,
                        failure_burst_started_at: None,
                        safe_mode_entered_at: None,
                        safe_mode_reason: None,
                        config_path: None,
                        last_event: "running".to_string(),
                        last_error: None,
                        agent: crate::runtime::linux_supervisor::LinuxSupervisorAgentState {
                            pid: Some(2),
                            state: "running".to_string(),
                            last_started_at: Some(1),
                            last_exited_at: None,
                            last_exit_code: None,
                            last_exit_signal: None,
                            last_exit_reason: String::new(),
                        },
                    },
                },
            ),
            release: Some(crate::runtime::LinuxReleaseStatus {
                managed: true,
                deploy_root: Some("/opt/beetle".to_string()),
                current: Some(LinuxReleasePointer {
                    name: "2026.04.07".to_string(),
                    path: "/opt/beetle/releases/2026.04.07".to_string(),
                }),
                rollback: Some(LinuxReleasePointer {
                    name: "2026.04.06".to_string(),
                    path: "/opt/beetle/releases/2026.04.06".to_string(),
                }),
                rollout_state: LinuxReleaseRolloutState::Steady,
                rollback_available: true,
                current_exe: "/opt/beetle/current/bin/beetle".to_string(),
                state_schema_version: crate::runtime::BEETLE_STATE_SCHEMA_VERSION,
                state_schema_current: true,
                systemd_unit_consistent: Some(true),
                init_script_consistent: Some(true),
                last_updated_at: 1,
                last_action: "validation_passed".to_string(),
            }),
        }
    }

    fn initiative() -> InitiativeSnapshot {
        InitiativeSnapshot {
            action: InitiativeAction::Hold,
            ready: false,
            presence_state: PresenceState::Idle,
            runtime_mode: runtime_mode(RuntimeMode::Normal),
            rationale: "no_boundary_safe_trigger".to_string(),
            suppression_reason: Some(InitiativeSuppressionReason::NoUsefulTrigger),
            target: Some(InitiativeTarget {
                scope_id: "rel:qq:chat-1".to_string(),
                channel: "qq_channel".to_string(),
                chat_id: "chat-1".to_string(),
                selection_reason: "preferred_relation".to_string(),
            }),
            signal: Some(crate::runtime::InitiativeSignalSnapshot {
                user_idle_secs: 60,
                autonomy_idle_secs: 60,
                in_progress_tasks: 0,
                due_tasks: 0,
                high_priority_tasks: 0,
                upcoming_reminders: 0,
                next_reminder_at: 0,
            }),
            strategy_mode: "steady".to_string(),
            strategy_focus: "assist".to_string(),
            idle_enabled: true,
            message_preview: None,
            last_triggered_at: None,
            next_allowed_at: None,
        }
    }

    #[test]
    fn beetle_os_acceptance_suite_catches_plane_and_delivery_regressions() {
        let base_presence = presence();
        let base_initiative = initiative();

        let steady = inspect_beetle_os_closure(&base_presence, &base_initiative);
        assert!(steady.ready, "{steady:?}");

        let mut unmanaged_presence = base_presence.clone();
        unmanaged_presence.supervisor = None;
        unmanaged_presence.release = Some(crate::runtime::LinuxReleaseStatus {
            managed: false,
            ..unmanaged_presence.release.clone().unwrap_or_default()
        });
        let unmanaged = inspect_beetle_os_closure(&unmanaged_presence, &base_initiative);
        assert!(!unmanaged.ready, "{unmanaged:?}");
        assert!(unmanaged
            .outstanding
            .iter()
            .any(|item| item == "supervisor:supervisor_status_missing"));
        assert!(unmanaged
            .outstanding
            .iter()
            .any(|item| item == "release:linux_release_unmanaged"));

        let mut broken_initiative = base_initiative.clone();
        broken_initiative.ready = true;
        broken_initiative.action = InitiativeAction::UpcomingReminderNudge;
        broken_initiative.suppression_reason = None;
        broken_initiative.target = None;
        broken_initiative.message_preview = Some("reminder soon".to_string());
        let initiative_report = inspect_beetle_os_closure(&base_presence, &broken_initiative);
        assert!(!initiative_report.ready, "{initiative_report:?}");
        assert!(initiative_report
            .outstanding
            .iter()
            .any(|item| item == "initiative:ready_initiative_missing_target"));

        let mut unrecoverable_presence = base_presence.clone();
        unrecoverable_presence.soul_kernel.minimum_viable = false;
        unrecoverable_presence
            .soul_kernel
            .safe_mode_minimum_readable = false;
        let soul_report = inspect_beetle_os_closure(&unrecoverable_presence, &base_initiative);
        assert!(!soul_report.ready, "{soul_report:?}");
        assert!(soul_report
            .outstanding
            .iter()
            .any(|item| item == "soul_kernel:kernel_unrecoverable"));

        let mut drift_presence = base_presence.clone();
        drift_presence.runtime_mode.current_mode = RuntimeMode::VoiceExclusive;
        drift_presence.runtime_mode.voice_exclusive_active = false;
        drift_presence
            .runtime_mode
            .action_budget
            .allow_non_voice_outbound = true;
        let drift_report = inspect_beetle_os_closure(&drift_presence, &base_initiative);
        assert!(!drift_report.ready, "{drift_report:?}");
        assert!(drift_report
            .outstanding
            .iter()
            .any(|item| item == "runtime_mode:voice_exclusive_mode_without_voice_flag"));
        assert!(drift_report
            .outstanding
            .iter()
            .any(|item| item == "runtime_mode:voice_exclusive_allows_non_voice_outbound"));
    }
}
