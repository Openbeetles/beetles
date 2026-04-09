//! Runtime sub-capability authority: compact, fixed-size, single-writer-friendly state.
//! 运行态子能力权威中心：固定小状态、单一口径、低开销。

use crate::memory::MemorySystemKind;
use crate::Platform;
use serde::Serialize;
use std::sync::atomic::{AtomicU32, AtomicU8, Ordering};

pub const RUNTIME_CAPABILITY_AUDIO_OUTPUT: &str = "audio_output";
pub const RUNTIME_CAPABILITY_AUDIO_INPUT: &str = "audio_input";
pub const RUNTIME_CAPABILITY_NETWORK_OUTBOUND_HTTP: &str = "network.outbound_http";
pub const RUNTIME_CAPABILITY_STORAGE_STATE_FS: &str = "storage.state_fs";

const RUNTIME_CAPABILITY_IDS: [&str; 4] = [
    RUNTIME_CAPABILITY_AUDIO_OUTPUT,
    RUNTIME_CAPABILITY_AUDIO_INPUT,
    RUNTIME_CAPABILITY_NETWORK_OUTBOUND_HTTP,
    RUNTIME_CAPABILITY_STORAGE_STATE_FS,
];

#[repr(u8)]
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeCapabilityStatus {
    Online = 0,
    Degraded = 1,
    Offline = 2,
}

impl RuntimeCapabilityStatus {
    const fn from_byte(raw: u8) -> Self {
        match raw {
            1 => Self::Degraded,
            2 => Self::Offline,
            _ => Self::Online,
        }
    }
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeCapabilityReason {
    Nominal = 0,
    NotConfigured = 1,
    RuntimeNotInitialized = 2,
    DeviceMissing = 3,
    DeviceDisconnected = 4,
    WorkerDead = 5,
    DriverError = 6,
    PermissionDenied = 7,
    UpstreamUnavailable = 8,
    RecoveryStabilizing = 9,
    OperatorDisabled = 10,
}

impl RuntimeCapabilityReason {
    const fn from_byte(raw: u8) -> Self {
        match raw {
            1 => Self::NotConfigured,
            2 => Self::RuntimeNotInitialized,
            3 => Self::DeviceMissing,
            4 => Self::DeviceDisconnected,
            5 => Self::WorkerDead,
            6 => Self::DriverError,
            7 => Self::PermissionDenied,
            8 => Self::UpstreamUnavailable,
            9 => Self::RecoveryStabilizing,
            10 => Self::OperatorDisabled,
            _ => Self::Nominal,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RuntimeCapabilityUpdate {
    pub id: &'static str,
    pub status: RuntimeCapabilityStatus,
    pub reason: RuntimeCapabilityReason,
    pub observed_at_secs: u32,
    pub recovery_hint: Option<&'static str>,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
pub struct RuntimeCapabilityState {
    pub id: &'static str,
    pub status: RuntimeCapabilityStatus,
    pub reason: RuntimeCapabilityReason,
    pub epoch: u32,
    pub changed_at_secs: u32,
    pub observed_at_secs: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recovery_hint: Option<&'static str>,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
pub struct RuntimeCapabilityBlocker {
    pub sub_capability: &'static str,
    pub capability_status: RuntimeCapabilityStatus,
    pub capability_reason: RuntimeCapabilityReason,
    pub epoch: u32,
    pub epoch_changed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recovery_hint: Option<&'static str>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct RuntimeCapabilitySummary {
    pub offline_count: usize,
    pub degraded_count: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub offline_ids: Vec<&'static str>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub degraded_ids: Vec<&'static str>,
}

struct RuntimeCapabilitySlot {
    status: AtomicU8,
    reason: AtomicU8,
    epoch: AtomicU32,
    changed_at_secs: AtomicU32,
    observed_at_secs: AtomicU32,
}

impl RuntimeCapabilitySlot {
    const fn new() -> Self {
        Self {
            status: AtomicU8::new(RuntimeCapabilityStatus::Offline as u8),
            reason: AtomicU8::new(RuntimeCapabilityReason::RuntimeNotInitialized as u8),
            epoch: AtomicU32::new(0),
            changed_at_secs: AtomicU32::new(0),
            observed_at_secs: AtomicU32::new(0),
        }
    }
}

struct RuntimeCapabilityAuthorityState {
    slots: [RuntimeCapabilitySlot; RUNTIME_CAPABILITY_IDS.len()],
}

impl RuntimeCapabilityAuthorityState {
    const fn new() -> Self {
        Self {
            slots: [
                RuntimeCapabilitySlot::new(),
                RuntimeCapabilitySlot::new(),
                RuntimeCapabilitySlot::new(),
                RuntimeCapabilitySlot::new(),
            ],
        }
    }
}

static RUNTIME_CAPABILITY_STATE: RuntimeCapabilityAuthorityState =
    RuntimeCapabilityAuthorityState::new();

fn capability_index(id: &str) -> Option<usize> {
    RUNTIME_CAPABILITY_IDS
        .iter()
        .position(|candidate| *candidate == id)
}

fn default_recovery_hint(
    id: &'static str,
    reason: RuntimeCapabilityReason,
) -> Option<&'static str> {
    match (id, reason) {
        (RUNTIME_CAPABILITY_AUDIO_OUTPUT, RuntimeCapabilityReason::DeviceDisconnected) => {
            Some("wait_for_audio_output_recovery")
        }
        (RUNTIME_CAPABILITY_AUDIO_INPUT, RuntimeCapabilityReason::DeviceDisconnected) => {
            Some("wait_for_audio_input_recovery")
        }
        (
            RUNTIME_CAPABILITY_NETWORK_OUTBOUND_HTTP,
            RuntimeCapabilityReason::UpstreamUnavailable,
        ) => Some("wait_for_network_recovery"),
        (RUNTIME_CAPABILITY_STORAGE_STATE_FS, RuntimeCapabilityReason::DriverError)
        | (RUNTIME_CAPABILITY_STORAGE_STATE_FS, RuntimeCapabilityReason::RuntimeNotInitialized) => {
            Some("wait_for_storage_recovery")
        }
        (_, RuntimeCapabilityReason::RecoveryStabilizing) => {
            Some("wait_for_recovery_stabilization")
        }
        _ => None,
    }
}

pub fn update_runtime_capability(update: RuntimeCapabilityUpdate) {
    let Some(index) = capability_index(update.id) else {
        log::warn!(
            "[orchestrator] unknown runtime capability update ignored: {}",
            update.id
        );
        return;
    };
    let slot = &RUNTIME_CAPABILITY_STATE.slots[index];
    let current_status = RuntimeCapabilityStatus::from_byte(slot.status.load(Ordering::Relaxed));
    let current_reason = RuntimeCapabilityReason::from_byte(slot.reason.load(Ordering::Relaxed));
    let current_epoch = slot.epoch.load(Ordering::Relaxed);
    let changed =
        current_epoch == 0 || current_status != update.status || current_reason != update.reason;
    let next_epoch = if changed {
        current_epoch.saturating_add(1).max(1)
    } else {
        current_epoch.max(1)
    };

    slot.status.store(update.status as u8, Ordering::Relaxed);
    slot.reason.store(update.reason as u8, Ordering::Relaxed);
    slot.epoch.store(next_epoch, Ordering::Relaxed);
    slot.observed_at_secs
        .store(update.observed_at_secs, Ordering::Relaxed);
    if changed {
        slot.changed_at_secs
            .store(update.observed_at_secs, Ordering::Relaxed);
    }
}

pub fn runtime_capability_snapshot() -> Vec<RuntimeCapabilityState> {
    RUNTIME_CAPABILITY_IDS
        .iter()
        .enumerate()
        .map(|(index, id)| {
            let slot = &RUNTIME_CAPABILITY_STATE.slots[index];
            let status = RuntimeCapabilityStatus::from_byte(slot.status.load(Ordering::Relaxed));
            let reason = RuntimeCapabilityReason::from_byte(slot.reason.load(Ordering::Relaxed));
            RuntimeCapabilityState {
                id,
                status,
                reason,
                epoch: slot.epoch.load(Ordering::Relaxed),
                changed_at_secs: slot.changed_at_secs.load(Ordering::Relaxed),
                observed_at_secs: slot.observed_at_secs.load(Ordering::Relaxed),
                recovery_hint: default_recovery_hint(id, reason),
            }
        })
        .collect()
}

pub fn get_runtime_capability(id: &str) -> Option<RuntimeCapabilityState> {
    let index = capability_index(id)?;
    let slot = &RUNTIME_CAPABILITY_STATE.slots[index];
    let status = RuntimeCapabilityStatus::from_byte(slot.status.load(Ordering::Relaxed));
    let reason = RuntimeCapabilityReason::from_byte(slot.reason.load(Ordering::Relaxed));
    Some(RuntimeCapabilityState {
        id: RUNTIME_CAPABILITY_IDS[index],
        status,
        reason,
        epoch: slot.epoch.load(Ordering::Relaxed),
        changed_at_secs: slot.changed_at_secs.load(Ordering::Relaxed),
        observed_at_secs: slot.observed_at_secs.load(Ordering::Relaxed),
        recovery_hint: default_recovery_hint(RUNTIME_CAPABILITY_IDS[index], reason),
    })
}

pub fn runtime_capability_blocker(required: &[&'static str]) -> Option<RuntimeCapabilityBlocker> {
    required.iter().find_map(|id| {
        let state = get_runtime_capability(id)?;
        match state.status {
            RuntimeCapabilityStatus::Online => None,
            RuntimeCapabilityStatus::Degraded | RuntimeCapabilityStatus::Offline => {
                Some(RuntimeCapabilityBlocker {
                    sub_capability: state.id,
                    capability_status: state.status,
                    capability_reason: state.reason,
                    epoch: state.epoch,
                    epoch_changed: state.epoch > 1,
                    recovery_hint: state.recovery_hint,
                })
            }
        }
    })
}

pub fn runtime_capability_summary() -> RuntimeCapabilitySummary {
    let mut offline_ids = Vec::new();
    let mut degraded_ids = Vec::new();
    for state in runtime_capability_snapshot() {
        match state.status {
            RuntimeCapabilityStatus::Offline => offline_ids.push(state.id),
            RuntimeCapabilityStatus::Degraded => degraded_ids.push(state.id),
            RuntimeCapabilityStatus::Online => {}
        }
    }
    RuntimeCapabilitySummary {
        offline_count: offline_ids.len(),
        degraded_count: degraded_ids.len(),
        offline_ids,
        degraded_ids,
    }
}

pub fn format_runtime_capability_baseline_line() -> String {
    let summary = runtime_capability_summary();
    let offline = if summary.offline_ids.is_empty() {
        "none".to_string()
    } else {
        summary.offline_ids.join(",")
    };
    let degraded = if summary.degraded_ids.is_empty() {
        "none".to_string()
    } else {
        summary.degraded_ids.join(",")
    };
    format!(
        "runtime_capabilities offline={} degraded={} offline_ids={} degraded_ids={}",
        summary.offline_count, summary.degraded_count, offline, degraded
    )
}

pub fn observe_runtime_capabilities_from_platform(
    platform: &dyn Platform,
    outbound_http_client_ready: bool,
    storage_state_fs_ready: Option<bool>,
) {
    let now_secs = crate::util::current_unix_secs().min(u32::MAX as u64) as u32;
    let speaker_online = platform.audio_speaker_ready();
    update_runtime_capability(RuntimeCapabilityUpdate {
        id: RUNTIME_CAPABILITY_AUDIO_OUTPUT,
        status: if speaker_online {
            RuntimeCapabilityStatus::Online
        } else {
            RuntimeCapabilityStatus::Offline
        },
        reason: if speaker_online {
            RuntimeCapabilityReason::Nominal
        } else {
            RuntimeCapabilityReason::DeviceMissing
        },
        observed_at_secs: now_secs,
        recovery_hint: None,
    });
    let mic_online = platform.audio_mic_ready();
    update_runtime_capability(RuntimeCapabilityUpdate {
        id: RUNTIME_CAPABILITY_AUDIO_INPUT,
        status: if mic_online {
            RuntimeCapabilityStatus::Online
        } else {
            RuntimeCapabilityStatus::Offline
        },
        reason: if mic_online {
            RuntimeCapabilityReason::Nominal
        } else {
            RuntimeCapabilityReason::DeviceMissing
        },
        observed_at_secs: now_secs,
        recovery_hint: None,
    });
    let outbound_online = match platform.memory_system_kind() {
        MemorySystemKind::LinuxFull => outbound_http_client_ready,
        MemorySystemKind::EspCompact => {
            outbound_http_client_ready && crate::state::wifi_sta_connected()
        }
    };
    update_runtime_capability(RuntimeCapabilityUpdate {
        id: RUNTIME_CAPABILITY_NETWORK_OUTBOUND_HTTP,
        status: if outbound_online {
            RuntimeCapabilityStatus::Online
        } else {
            RuntimeCapabilityStatus::Offline
        },
        reason: if outbound_online {
            RuntimeCapabilityReason::Nominal
        } else if outbound_http_client_ready {
            RuntimeCapabilityReason::UpstreamUnavailable
        } else {
            RuntimeCapabilityReason::RuntimeNotInitialized
        },
        observed_at_secs: now_secs,
        recovery_hint: None,
    });
    if let Some(storage_ready) = storage_state_fs_ready {
        update_runtime_capability(RuntimeCapabilityUpdate {
            id: RUNTIME_CAPABILITY_STORAGE_STATE_FS,
            status: if storage_ready {
                RuntimeCapabilityStatus::Online
            } else {
                RuntimeCapabilityStatus::Offline
            },
            reason: if storage_ready {
                RuntimeCapabilityReason::Nominal
            } else {
                RuntimeCapabilityReason::RuntimeNotInitialized
            },
            observed_at_secs: now_secs,
            recovery_hint: None,
        });
    }
}

pub fn observe_runtime_capability_success(required: &[&'static str]) {
    let now_secs = crate::util::current_unix_secs().min(u32::MAX as u64) as u32;
    for id in required {
        if capability_index(id).is_none() {
            continue;
        }
        update_runtime_capability(RuntimeCapabilityUpdate {
            id,
            status: RuntimeCapabilityStatus::Online,
            reason: RuntimeCapabilityReason::Nominal,
            observed_at_secs: now_secs,
            recovery_hint: None,
        });
    }
}

pub fn observe_runtime_capability_failure(id: &'static str, reason: RuntimeCapabilityReason) {
    if capability_index(id).is_none() {
        return;
    }
    let now_secs = crate::util::current_unix_secs().min(u32::MAX as u64) as u32;
    update_runtime_capability(RuntimeCapabilityUpdate {
        id,
        status: RuntimeCapabilityStatus::Offline,
        reason,
        observed_at_secs: now_secs,
        recovery_hint: None,
    });
}

#[cfg(test)]
pub fn reset_runtime_capabilities_for_tests() {
    for slot in &RUNTIME_CAPABILITY_STATE.slots {
        slot.status
            .store(RuntimeCapabilityStatus::Offline as u8, Ordering::Relaxed);
        slot.reason.store(
            RuntimeCapabilityReason::RuntimeNotInitialized as u8,
            Ordering::Relaxed,
        );
        slot.epoch.store(0, Ordering::Relaxed);
        slot.changed_at_secs.store(0, Ordering::Relaxed);
        slot.observed_at_secs.store(0, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static TEST_GUARD: Mutex<()> = Mutex::new(());

    #[test]
    fn update_runtime_capability_bumps_epoch_only_on_meaningful_change() {
        let _guard = TEST_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        reset_runtime_capabilities_for_tests();
        update_runtime_capability(RuntimeCapabilityUpdate {
            id: RUNTIME_CAPABILITY_AUDIO_OUTPUT,
            status: RuntimeCapabilityStatus::Offline,
            reason: RuntimeCapabilityReason::DeviceDisconnected,
            observed_at_secs: 10,
            recovery_hint: None,
        });
        let first = get_runtime_capability(RUNTIME_CAPABILITY_AUDIO_OUTPUT).expect("state");
        assert_eq!(first.epoch, 1);

        update_runtime_capability(RuntimeCapabilityUpdate {
            id: RUNTIME_CAPABILITY_AUDIO_OUTPUT,
            status: RuntimeCapabilityStatus::Offline,
            reason: RuntimeCapabilityReason::DeviceDisconnected,
            observed_at_secs: 11,
            recovery_hint: None,
        });
        let second = get_runtime_capability(RUNTIME_CAPABILITY_AUDIO_OUTPUT).expect("state");
        assert_eq!(second.epoch, 1);
        assert_eq!(second.observed_at_secs, 11);

        update_runtime_capability(RuntimeCapabilityUpdate {
            id: RUNTIME_CAPABILITY_AUDIO_OUTPUT,
            status: RuntimeCapabilityStatus::Online,
            reason: RuntimeCapabilityReason::Nominal,
            observed_at_secs: 12,
            recovery_hint: None,
        });
        let third = get_runtime_capability(RUNTIME_CAPABILITY_AUDIO_OUTPUT).expect("state");
        assert_eq!(third.epoch, 2);
        assert_eq!(third.status, RuntimeCapabilityStatus::Online);
    }
}
