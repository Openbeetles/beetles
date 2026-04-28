//! Static runtime execution-plane registry.
//! ESP 运行面静态真源，用于把线程、资源租约和模式边界收口到一张表。

use crate::runtime::lease::LeaseKind;
use crate::runtime::mode::RuntimeMode;
use crate::runtime::thread_registry::{ThreadExecutionClass, ThreadRiskClass};

/// Stable identifier for a Beetle runtime execution plane.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaneId {
    Bootstrap,
    ConfigRecovery,
    ChannelWss,
    ChannelOutbound,
    AgentMain,
    Display,
    Voice,
    StorageWriteBack,
    Diagnostic,
    Maintenance,
    PlatformWifi,
    PlatformAudio,
    RuntimeAux,
}

/// When a plane is expected to occupy resources.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaneResidency {
    Steady,
    Lazy,
    Transient,
}

/// Startup phase that owns the plane creation decision.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaneStartupPhase {
    Boot,
    Runtime,
    OnDemand,
    Recovery,
}

/// Static queue budget attached to a plane.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub struct PlaneQueueBudget {
    pub name: &'static str,
    pub capacity: usize,
}

/// Static plane profile consumed by thread and resource observability.
#[derive(Clone, Copy, Debug, serde::Serialize)]
pub struct PlaneProfile {
    pub id: PlaneId,
    pub owner: &'static str,
    pub startup_phase: PlaneStartupPhase,
    pub residency: PlaneResidency,
    pub allowed_modes: &'static [RuntimeMode],
    /// Runtime leases this plane must actually acquire while occupying its
    /// execution window. Capability/headroom-only facts stay on the capability
    /// booleans and route worker contracts.
    pub required_leases: &'static [LeaseKind],
    pub thread_names: &'static [&'static str],
    pub queue_budget: Option<PlaneQueueBudget>,
    pub drain_timeout_secs: Option<u64>,
    pub execution_class: ThreadExecutionClass,
    pub risk_class: ThreadRiskClass,
    pub tls_capable: bool,
    pub http_capable: bool,
    pub wss_capable: bool,
    pub mode_sensitive: bool,
}

/// Compact snapshot of the static plane registry.
#[derive(Clone, Copy, Debug, serde::Serialize)]
pub struct PlaneRegistrySnapshot {
    pub profile_count: usize,
    pub steady_count: usize,
    pub lazy_count: usize,
    pub transient_count: usize,
    pub mode_sensitive_count: usize,
    pub tls_capable_count: usize,
    pub http_capable_count: usize,
    pub wss_capable_count: usize,
    pub queue_budget_count: usize,
    pub lease_reference_count: usize,
}

const MODES_ALL: &[RuntimeMode] = &[
    RuntimeMode::Booting,
    RuntimeMode::Pairing,
    RuntimeMode::Normal,
    RuntimeMode::ConfigActive,
    RuntimeMode::VoiceExclusive,
    RuntimeMode::Maintenance,
    RuntimeMode::Upgrade,
    RuntimeMode::RecoverySafeMode,
];

const MODES_NON_VOICE_NETWORK: &[RuntimeMode] = &[
    RuntimeMode::Booting,
    RuntimeMode::Pairing,
    RuntimeMode::Normal,
    RuntimeMode::ConfigActive,
    RuntimeMode::Maintenance,
];

const MODES_NORMAL_AND_MAINTENANCE: &[RuntimeMode] = &[
    RuntimeMode::Normal,
    RuntimeMode::ConfigActive,
    RuntimeMode::Maintenance,
];

const MODES_VOICE: &[RuntimeMode] = &[RuntimeMode::Normal, RuntimeMode::VoiceExclusive];

const MODES_BOOT_ONLY: &[RuntimeMode] = &[RuntimeMode::Booting];

const MODES_RECOVERY: &[RuntimeMode] = &[
    RuntimeMode::Booting,
    RuntimeMode::Pairing,
    RuntimeMode::Normal,
    RuntimeMode::ConfigActive,
    RuntimeMode::RecoverySafeMode,
    RuntimeMode::Upgrade,
];

const MODES_DIAGNOSTIC: &[RuntimeMode] = &[
    RuntimeMode::Normal,
    RuntimeMode::ConfigActive,
    RuntimeMode::Maintenance,
    RuntimeMode::RecoverySafeMode,
];

const PLANE_PROFILES: &[PlaneProfile] = &[
    PlaneProfile {
        id: PlaneId::Bootstrap,
        owner: "runtime_bootstrap",
        startup_phase: PlaneStartupPhase::Boot,
        residency: PlaneResidency::Transient,
        allowed_modes: MODES_BOOT_ONLY,
        required_leases: &[],
        thread_names: &["runtime_bootstrap", "startup_recovery"],
        queue_budget: None,
        drain_timeout_secs: Some(60),
        execution_class: ThreadExecutionClass::Runtime,
        risk_class: ThreadRiskClass::Low,
        tls_capable: false,
        http_capable: false,
        wss_capable: false,
        mode_sensitive: true,
    },
    PlaneProfile {
        id: PlaneId::ConfigRecovery,
        owner: "config_plane",
        startup_phase: PlaneStartupPhase::Runtime,
        residency: PlaneResidency::Steady,
        allowed_modes: MODES_RECOVERY,
        required_leases: &[],
        thread_names: &["config_plane_watch", "http_server"],
        queue_budget: None,
        drain_timeout_secs: Some(10),
        execution_class: ThreadExecutionClass::Config,
        risk_class: ThreadRiskClass::Low,
        tls_capable: false,
        http_capable: true,
        wss_capable: false,
        mode_sensitive: true,
    },
    PlaneProfile {
        id: PlaneId::Diagnostic,
        owner: "http_snapshot",
        startup_phase: PlaneStartupPhase::OnDemand,
        residency: PlaneResidency::Lazy,
        allowed_modes: MODES_DIAGNOSTIC,
        required_leases: &[LeaseKind::SnapshotHttpWorker],
        thread_names: &["http_snapshot_exec"],
        queue_budget: Some(PlaneQueueBudget {
            name: "http_snapshot_exec",
            capacity: 2,
        }),
        drain_timeout_secs: Some(5),
        execution_class: ThreadExecutionClass::Config,
        risk_class: ThreadRiskClass::Medium,
        tls_capable: false,
        http_capable: true,
        wss_capable: false,
        mode_sensitive: true,
    },
    PlaneProfile {
        id: PlaneId::ConfigRecovery,
        owner: "http_config",
        startup_phase: PlaneStartupPhase::OnDemand,
        residency: PlaneResidency::Lazy,
        allowed_modes: MODES_RECOVERY,
        required_leases: &[LeaseKind::ConfigHttpWorker],
        thread_names: &["http_config_exec"],
        queue_budget: Some(PlaneQueueBudget {
            name: "http_config_exec",
            capacity: 2,
        }),
        drain_timeout_secs: Some(10),
        execution_class: ThreadExecutionClass::Config,
        risk_class: ThreadRiskClass::High,
        tls_capable: true,
        http_capable: true,
        wss_capable: false,
        mode_sensitive: true,
    },
    PlaneProfile {
        id: PlaneId::Diagnostic,
        owner: "http_diagnostic",
        startup_phase: PlaneStartupPhase::OnDemand,
        residency: PlaneResidency::Lazy,
        allowed_modes: MODES_DIAGNOSTIC,
        required_leases: &[LeaseKind::DiagnosticHttpWorker],
        thread_names: &["http_diag_exec"],
        queue_budget: Some(PlaneQueueBudget {
            name: "http_diag_exec",
            capacity: 2,
        }),
        drain_timeout_secs: Some(8),
        execution_class: ThreadExecutionClass::Config,
        risk_class: ThreadRiskClass::High,
        tls_capable: true,
        http_capable: true,
        wss_capable: false,
        mode_sensitive: true,
    },
    PlaneProfile {
        id: PlaneId::ChannelWss,
        owner: "external_wss",
        startup_phase: PlaneStartupPhase::Runtime,
        residency: PlaneResidency::Steady,
        allowed_modes: MODES_NON_VOICE_NETWORK,
        required_leases: &[LeaseKind::ExternalWss, LeaseKind::TlsHandshake],
        thread_names: &["qq_ws", "feishu_ws", "wecom_aibot", "dingtalk_stream"],
        queue_budget: None,
        drain_timeout_secs: Some(30),
        execution_class: ThreadExecutionClass::Channel,
        risk_class: ThreadRiskClass::Critical,
        tls_capable: true,
        http_capable: true,
        wss_capable: true,
        mode_sensitive: true,
    },
    PlaneProfile {
        id: PlaneId::ChannelOutbound,
        owner: "channel_outbound",
        startup_phase: PlaneStartupPhase::Runtime,
        residency: PlaneResidency::Steady,
        allowed_modes: MODES_NON_VOICE_NETWORK,
        required_leases: &[],
        thread_names: &[
            "qq_sender",
            "tg_sender",
            "fs_sender",
            "dt_sender",
            "wc_sender",
            "tg_poll",
            "os_outbound",
        ],
        queue_budget: Some(PlaneQueueBudget {
            name: "channel_outbound",
            capacity: crate::constants::CHANNEL_SENDER_QUEUE_DEPTH,
        }),
        drain_timeout_secs: Some(30),
        execution_class: ThreadExecutionClass::Channel,
        risk_class: ThreadRiskClass::High,
        tls_capable: true,
        http_capable: true,
        wss_capable: false,
        mode_sensitive: true,
    },
    PlaneProfile {
        id: PlaneId::ChannelOutbound,
        owner: "dispatch",
        startup_phase: PlaneStartupPhase::Runtime,
        residency: PlaneResidency::Steady,
        allowed_modes: MODES_NON_VOICE_NETWORK,
        required_leases: &[],
        thread_names: &["dispatch"],
        queue_budget: Some(PlaneQueueBudget {
            name: "runtime_outbound",
            capacity: crate::constants::DEFAULT_CAPACITY,
        }),
        drain_timeout_secs: Some(30),
        execution_class: ThreadExecutionClass::Runtime,
        risk_class: ThreadRiskClass::High,
        tls_capable: false,
        http_capable: false,
        wss_capable: false,
        mode_sensitive: true,
    },
    PlaneProfile {
        id: PlaneId::AgentMain,
        owner: "agent_loop",
        startup_phase: PlaneStartupPhase::Runtime,
        residency: PlaneResidency::Steady,
        allowed_modes: MODES_NORMAL_AND_MAINTENANCE,
        required_leases: &[LeaseKind::AgentHeavyTurn],
        thread_names: &["agent_loop"],
        queue_budget: Some(PlaneQueueBudget {
            name: "agent_inbound",
            capacity: crate::constants::DEFAULT_CAPACITY,
        }),
        drain_timeout_secs: Some(60),
        execution_class: ThreadExecutionClass::Agent,
        risk_class: ThreadRiskClass::Critical,
        tls_capable: true,
        http_capable: true,
        wss_capable: false,
        mode_sensitive: true,
    },
    PlaneProfile {
        id: PlaneId::Display,
        owner: "display",
        startup_phase: PlaneStartupPhase::Runtime,
        residency: PlaneResidency::Steady,
        allowed_modes: MODES_ALL,
        required_leases: &[LeaseKind::Display],
        thread_names: &["display"],
        queue_budget: None,
        drain_timeout_secs: Some(5),
        execution_class: ThreadExecutionClass::Ui,
        risk_class: ThreadRiskClass::Low,
        tls_capable: false,
        http_capable: false,
        wss_capable: false,
        mode_sensitive: true,
    },
    PlaneProfile {
        id: PlaneId::Voice,
        owner: "voice_session_control",
        startup_phase: PlaneStartupPhase::OnDemand,
        residency: PlaneResidency::Lazy,
        allowed_modes: MODES_ALL,
        required_leases: &[],
        thread_names: &["voice_session"],
        queue_budget: Some(PlaneQueueBudget {
            name: "voice_event",
            capacity: crate::constants::VOICE_EVENT_QUEUE_CAPACITY,
        }),
        drain_timeout_secs: Some(15),
        execution_class: ThreadExecutionClass::Voice,
        risk_class: ThreadRiskClass::Medium,
        tls_capable: false,
        http_capable: false,
        wss_capable: false,
        mode_sensitive: true,
    },
    PlaneProfile {
        id: PlaneId::Voice,
        owner: "voice_session_worker",
        startup_phase: PlaneStartupPhase::OnDemand,
        residency: PlaneResidency::Lazy,
        allowed_modes: MODES_VOICE,
        required_leases: &[
            LeaseKind::AudioInput,
            LeaseKind::AudioOutput,
            LeaseKind::TlsHandshake,
        ],
        thread_names: &["voice_session_worker"],
        queue_budget: None,
        drain_timeout_secs: Some(15),
        execution_class: ThreadExecutionClass::Voice,
        risk_class: ThreadRiskClass::Critical,
        tls_capable: true,
        http_capable: true,
        wss_capable: false,
        mode_sensitive: true,
    },
    PlaneProfile {
        id: PlaneId::Voice,
        owner: "voice_realtime",
        startup_phase: PlaneStartupPhase::OnDemand,
        residency: PlaneResidency::Lazy,
        allowed_modes: &[RuntimeMode::VoiceExclusive],
        required_leases: &[
            LeaseKind::AudioInput,
            LeaseKind::AudioOutput,
            LeaseKind::TlsHandshake,
        ],
        thread_names: &["voice_realtime", "voice_realtime_connect"],
        queue_budget: None,
        drain_timeout_secs: Some(15),
        execution_class: ThreadExecutionClass::Voice,
        risk_class: ThreadRiskClass::Critical,
        tls_capable: true,
        http_capable: true,
        wss_capable: true,
        mode_sensitive: true,
    },
    PlaneProfile {
        id: PlaneId::StorageWriteBack,
        owner: "write_back",
        startup_phase: PlaneStartupPhase::OnDemand,
        residency: PlaneResidency::Lazy,
        allowed_modes: MODES_NORMAL_AND_MAINTENANCE,
        required_leases: &[LeaseKind::StorageSessionWrite],
        thread_names: &["write_back"],
        queue_budget: Some(PlaneQueueBudget {
            name: "write_back",
            capacity: crate::runtime::write_back::WRITE_BACK_QUEUE_MAX,
        }),
        drain_timeout_secs: Some(5),
        execution_class: ThreadExecutionClass::Runtime,
        risk_class: ThreadRiskClass::High,
        tls_capable: false,
        http_capable: false,
        wss_capable: false,
        mode_sensitive: true,
    },
    PlaneProfile {
        id: PlaneId::PlatformWifi,
        owner: "wifi_worker",
        startup_phase: PlaneStartupPhase::Boot,
        residency: PlaneResidency::Steady,
        allowed_modes: MODES_ALL,
        required_leases: &[],
        thread_names: &["wifi_worker"],
        queue_budget: None,
        drain_timeout_secs: Some(15),
        execution_class: ThreadExecutionClass::Platform,
        risk_class: ThreadRiskClass::High,
        tls_capable: false,
        http_capable: false,
        wss_capable: false,
        mode_sensitive: false,
    },
    PlaneProfile {
        id: PlaneId::PlatformAudio,
        owner: "audio_io_worker",
        startup_phase: PlaneStartupPhase::Runtime,
        residency: PlaneResidency::Steady,
        allowed_modes: MODES_ALL,
        required_leases: &[],
        thread_names: &["audio_io_worker"],
        queue_budget: None,
        drain_timeout_secs: Some(10),
        execution_class: ThreadExecutionClass::Platform,
        risk_class: ThreadRiskClass::Medium,
        tls_capable: false,
        http_capable: false,
        wss_capable: false,
        mode_sensitive: false,
    },
    PlaneProfile {
        id: PlaneId::Maintenance,
        owner: "runtime_timers",
        startup_phase: PlaneStartupPhase::Runtime,
        residency: PlaneResidency::Steady,
        allowed_modes: MODES_ALL,
        required_leases: &[],
        thread_names: &["bg_timer", "heartbeat", "cron", "remind"],
        queue_budget: None,
        drain_timeout_secs: Some(10),
        execution_class: ThreadExecutionClass::Runtime,
        risk_class: ThreadRiskClass::Low,
        tls_capable: false,
        http_capable: false,
        wss_capable: false,
        mode_sensitive: false,
    },
    PlaneProfile {
        id: PlaneId::RuntimeAux,
        owner: "runtime_aux",
        startup_phase: PlaneStartupPhase::Runtime,
        residency: PlaneResidency::Transient,
        allowed_modes: MODES_ALL,
        required_leases: &[],
        thread_names: &["sntp", "cli_repl"],
        queue_budget: None,
        drain_timeout_secs: Some(5),
        execution_class: ThreadExecutionClass::Runtime,
        risk_class: ThreadRiskClass::Low,
        tls_capable: false,
        http_capable: false,
        wss_capable: false,
        mode_sensitive: false,
    },
];

/// Return all static plane profiles.
pub fn profiles() -> &'static [PlaneProfile] {
    PLANE_PROFILES
}

/// Find the static plane profile that owns a thread name.
pub fn profile_for_thread(name: &str) -> Option<&'static PlaneProfile> {
    PLANE_PROFILES
        .iter()
        .find(|profile| profile.thread_names.contains(&name))
}

/// Return a compact static plane registry snapshot.
pub fn snapshot() -> PlaneRegistrySnapshot {
    let mut steady_count = 0usize;
    let mut lazy_count = 0usize;
    let mut transient_count = 0usize;
    let mut mode_sensitive_count = 0usize;
    let mut tls_capable_count = 0usize;
    let mut http_capable_count = 0usize;
    let mut wss_capable_count = 0usize;
    let mut queue_budget_count = 0usize;
    let mut lease_reference_count = 0usize;

    for profile in PLANE_PROFILES {
        match profile.residency {
            PlaneResidency::Steady => steady_count += 1,
            PlaneResidency::Lazy => lazy_count += 1,
            PlaneResidency::Transient => transient_count += 1,
        }
        if profile.mode_sensitive {
            mode_sensitive_count += 1;
        }
        if profile.tls_capable {
            tls_capable_count += 1;
        }
        if profile.http_capable {
            http_capable_count += 1;
        }
        if profile.wss_capable {
            wss_capable_count += 1;
        }
        if profile.queue_budget.is_some() {
            queue_budget_count += 1;
        }
        lease_reference_count = lease_reference_count.saturating_add(profile.required_leases.len());
    }

    PlaneRegistrySnapshot {
        profile_count: PLANE_PROFILES.len(),
        steady_count,
        lazy_count,
        transient_count,
        mode_sensitive_count,
        tls_capable_count,
        http_capable_count,
        wss_capable_count,
        queue_budget_count,
        lease_reference_count,
    }
}

/// Return a compact plane registry baseline for heartbeat logs.
pub fn format_baseline_log_line() -> String {
    let snapshot = snapshot();
    format!(
        "planes profiles={} steady={} lazy={} transient={} mode_sensitive={} tls={} http={} wss={} queue_budgets={} lease_refs={}",
        snapshot.profile_count,
        snapshot.steady_count,
        snapshot.lazy_count,
        snapshot.transient_count,
        snapshot.mode_sensitive_count,
        snapshot.tls_capable_count,
        snapshot.http_capable_count,
        snapshot.wss_capable_count,
        snapshot.queue_budget_count,
        snapshot.lease_reference_count,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_plane_is_mode_sensitive() {
        let profile = profile_for_thread("display").expect("display profile");

        assert_eq!(profile.id, PlaneId::Display);
        assert_eq!(profile.residency, PlaneResidency::Steady);
        assert!(profile.mode_sensitive);
        assert_eq!(profile.required_leases, &[LeaseKind::Display]);
    }

    #[test]
    fn snapshot_route_profile_does_not_reserve_tls() {
        let profile = profile_for_thread("http_snapshot_exec").expect("snapshot profile");

        assert_eq!(profile.execution_class, ThreadExecutionClass::Config);
        assert_eq!(profile.risk_class, ThreadRiskClass::Medium);
        assert!(profile.http_capable);
        assert!(!profile.tls_capable);
        assert_eq!(profile.required_leases, &[LeaseKind::SnapshotHttpWorker]);
        assert_eq!(
            profile.queue_budget,
            Some(PlaneQueueBudget {
                name: "http_snapshot_exec",
                capacity: 2,
            })
        );
    }

    #[test]
    fn http_worker_profiles_match_route_worker_lane_leases() {
        let cases = [
            (
                "http_snapshot_exec",
                crate::platform::http_server::router::catalog::RouteExecutionClass::SnapshotRoute,
            ),
            (
                "http_config_exec",
                crate::platform::http_server::router::catalog::RouteExecutionClass::AsyncConfigRoute,
            ),
            (
                "http_diag_exec",
                crate::platform::http_server::router::catalog::RouteExecutionClass::SlowDiagnosticRoute,
            ),
        ];

        for (thread_name, class) in cases {
            let profile = profile_for_thread(thread_name).expect("http worker profile");
            let contract = class.worker_contract().expect("route worker contract");
            assert!(
                profile
                    .required_leases
                    .contains(&contract.lane.lease_kind()),
                "{thread_name} must declare its route worker lease"
            );
        }
    }

    #[test]
    fn http_worker_profiles_do_not_claim_precise_tls_handshake_lease_yet() {
        for thread_name in ["http_config_exec", "http_diag_exec"] {
            let profile = profile_for_thread(thread_name).expect("http worker profile");

            assert!(profile.tls_capable);
            assert!(
                !profile.required_leases.contains(&LeaseKind::TlsHandshake),
                "{thread_name} must not claim a precise TLS handshake lease until the HTTP seam acquires it"
            );
        }
    }

    #[test]
    fn http_calling_planes_do_not_claim_precise_tls_handshake_lease_yet() {
        for thread_name in ["os_outbound", "agent_loop"] {
            let profile = profile_for_thread(thread_name).expect("http calling profile");

            assert!(profile.tls_capable);
            assert!(
                !profile.required_leases.contains(&LeaseKind::TlsHandshake),
                "{thread_name} must not claim a precise TLS handshake lease until the HTTP seam acquires it"
            );
        }
    }

    #[test]
    fn official_ota_worker_plane_is_not_registered_without_an_implementation() {
        assert!(profile_for_thread(concat!("http_", "ota_exec")).is_none());
    }

    #[test]
    fn write_back_plane_is_lazy_storage_owner() {
        let profile = profile_for_thread("write_back").expect("write_back profile");

        assert_eq!(profile.id, PlaneId::StorageWriteBack);
        assert_eq!(profile.residency, PlaneResidency::Lazy);
        assert_eq!(profile.required_leases, &[LeaseKind::StorageSessionWrite]);
        assert_eq!(profile.risk_class, ThreadRiskClass::High);
    }

    #[test]
    fn voice_control_is_separate_from_realtime_tls_plane() {
        let control = profile_for_thread("voice_session").expect("voice control profile");
        let realtime =
            profile_for_thread("voice_realtime_connect").expect("voice realtime profile");

        assert_eq!(control.id, PlaneId::Voice);
        assert!(!control.tls_capable);
        assert!(!control.wss_capable);
        assert_eq!(control.required_leases, &[]);
        assert_eq!(realtime.allowed_modes, &[RuntimeMode::VoiceExclusive]);
        assert!(realtime.tls_capable);
        assert!(realtime.wss_capable);
    }

    #[test]
    fn platform_audio_worker_does_not_hold_session_audio_leases() {
        let profile = profile_for_thread("audio_io_worker").expect("audio io profile");

        assert_eq!(profile.id, PlaneId::PlatformAudio);
        assert_eq!(profile.required_leases, &[]);
    }

    #[test]
    fn baseline_counts_static_profiles() {
        let snapshot = snapshot();

        assert!(snapshot.profile_count >= 10);
        assert!(snapshot.steady_count > 0);
        assert!(snapshot.lazy_count > 0);
        assert!(format_baseline_log_line().contains("planes profiles="));
    }
}
