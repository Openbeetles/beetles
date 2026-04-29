//! Runtime sub-capability authority: compact, fixed-size, single-writer-friendly state.
//! 运行态子能力权威中心：固定小状态、单一口径、低开销。

use crate::memory::MemorySystemKind;
use crate::Error;
use crate::Platform;
use crate::Result;
use serde::Serialize;
use std::sync::atomic::{AtomicU32, AtomicU8, Ordering};
use std::sync::OnceLock;
use std::time::Instant;

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

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RuntimeCapabilityCallState {
    Accepting = 0,
    Draining = 1,
    Disabled = 2,
    Unloaded = 3,
    Failed = 4,
}

impl RuntimeCapabilityCallState {
    const fn from_byte(raw: u8) -> Self {
        match raw {
            1 => Self::Draining,
            2 => Self::Disabled,
            3 => Self::Unloaded,
            4 => Self::Failed,
            _ => Self::Accepting,
        }
    }

    const fn accepts_new_calls(self) -> bool {
        matches!(self, Self::Accepting)
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
    pub active_calls: u32,
    pub draining: bool,
    pub last_transition_uptime_ms: u64,
    pub drain_denied_total: u64,
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
    call_word: AtomicU32,
    last_transition_uptime_ms: AtomicU32,
    drain_denied_total: AtomicU32,
}

impl RuntimeCapabilitySlot {
    const fn new() -> Self {
        Self {
            status: AtomicU8::new(RuntimeCapabilityStatus::Offline as u8),
            reason: AtomicU8::new(RuntimeCapabilityReason::RuntimeNotInitialized as u8),
            epoch: AtomicU32::new(0),
            changed_at_secs: AtomicU32::new(0),
            observed_at_secs: AtomicU32::new(0),
            call_word: AtomicU32::new(pack_call_word(RuntimeCapabilityCallState::Unloaded, 0)),
            last_transition_uptime_ms: AtomicU32::new(0),
            drain_denied_total: AtomicU32::new(0),
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

#[cfg(test)]
pub(crate) static RUNTIME_CAPABILITY_TEST_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn capability_index(id: &str) -> Option<usize> {
    RUNTIME_CAPABILITY_IDS
        .iter()
        .position(|candidate| *candidate == id)
}

const CALL_ACTIVE_MASK: u32 = 0xffff;
const CALL_STATE_SHIFT: u32 = 16;

const fn pack_call_word(state: RuntimeCapabilityCallState, active_calls: u32) -> u32 {
    ((state as u32) << CALL_STATE_SHIFT) | (active_calls & CALL_ACTIVE_MASK)
}

fn call_state_from_word(word: u32) -> RuntimeCapabilityCallState {
    RuntimeCapabilityCallState::from_byte((word >> CALL_STATE_SHIFT) as u8)
}

fn active_calls_from_word(word: u32) -> u32 {
    word & CALL_ACTIVE_MASK
}

fn runtime_capability_error(id: &str, message: &'static str) -> Error {
    Error::config("runtime_capability_call", format!("{message}: {id}"))
}

fn runtime_capability_unknown_error(id: &str) -> Error {
    runtime_capability_error(id, "unknown runtime capability")
}

fn now_secs_u32() -> u32 {
    crate::util::current_unix_secs().min(u32::MAX as u64) as u32
}

static RUNTIME_CAPABILITY_MONOTONIC_START: OnceLock<Instant> = OnceLock::new();

fn now_uptime_millis_u32() -> u32 {
    RUNTIME_CAPABILITY_MONOTONIC_START
        .get_or_init(Instant::now)
        .elapsed()
        .as_millis()
        .min(u32::MAX as u128) as u32
}

fn set_call_state(slot: &RuntimeCapabilitySlot, next_state: RuntimeCapabilityCallState) {
    let mut current = slot.call_word.load(Ordering::Relaxed);
    loop {
        let current_state = call_state_from_word(current);
        if current_state == next_state {
            return;
        }
        let active_calls = active_calls_from_word(current);
        let next = pack_call_word(next_state, active_calls);
        match slot.call_word.compare_exchange_weak(
            current,
            next,
            Ordering::AcqRel,
            Ordering::Relaxed,
        ) {
            Ok(_) => {
                slot.last_transition_uptime_ms
                    .store(now_uptime_millis_u32(), Ordering::Relaxed);
                return;
            }
            Err(actual) => current = actual,
        }
    }
}

fn reconcile_call_state_after_update(
    slot: &RuntimeCapabilitySlot,
    status: RuntimeCapabilityStatus,
    reason: RuntimeCapabilityReason,
) {
    let mut current = slot.call_word.load(Ordering::Relaxed);
    loop {
        let current_state = call_state_from_word(current);
        if current_state == RuntimeCapabilityCallState::Draining {
            return;
        }
        let next_state = match (status, reason) {
            (RuntimeCapabilityStatus::Online, _) | (RuntimeCapabilityStatus::Degraded, _) => {
                RuntimeCapabilityCallState::Accepting
            }
            (_, RuntimeCapabilityReason::OperatorDisabled) => RuntimeCapabilityCallState::Disabled,
            (_, RuntimeCapabilityReason::WorkerDead | RuntimeCapabilityReason::DriverError) => {
                RuntimeCapabilityCallState::Failed
            }
            (_, RuntimeCapabilityReason::RuntimeNotInitialized) => {
                RuntimeCapabilityCallState::Unloaded
            }
            _ => current_state,
        };
        if next_state == current_state {
            return;
        }
        let next = pack_call_word(next_state, active_calls_from_word(current));
        match slot.call_word.compare_exchange_weak(
            current,
            next,
            Ordering::AcqRel,
            Ordering::Relaxed,
        ) {
            Ok(_) => {
                slot.last_transition_uptime_ms
                    .store(now_uptime_millis_u32(), Ordering::Relaxed);
                return;
            }
            Err(actual) => current = actual,
        }
    }
}

fn increment_active_call(slot: &RuntimeCapabilitySlot) -> std::result::Result<(), &'static str> {
    let mut current = slot.call_word.load(Ordering::Relaxed);
    loop {
        let state = call_state_from_word(current);
        if !state.accepts_new_calls() {
            return Err("runtime capability is not accepting calls");
        }
        let active_calls = active_calls_from_word(current);
        let Some(next_active_calls) = active_calls.checked_add(1) else {
            return Err("runtime capability active call capacity exhausted");
        };
        if next_active_calls > CALL_ACTIVE_MASK {
            return Err("runtime capability active call capacity exhausted");
        }
        let next = pack_call_word(state, next_active_calls);
        match slot.call_word.compare_exchange_weak(
            current,
            next,
            Ordering::AcqRel,
            Ordering::Relaxed,
        ) {
            Ok(_) => return Ok(()),
            Err(actual) => current = actual,
        }
    }
}

fn decrement_active_call(slot: &RuntimeCapabilitySlot) {
    let mut current = slot.call_word.load(Ordering::Relaxed);
    loop {
        let active_calls = active_calls_from_word(current);
        if active_calls == 0 {
            return;
        }
        let next = pack_call_word(
            call_state_from_word(current),
            active_calls.saturating_sub(1),
        );
        match slot.call_word.compare_exchange_weak(
            current,
            next,
            Ordering::AcqRel,
            Ordering::Relaxed,
        ) {
            Ok(_) => return,
            Err(actual) => current = actual,
        }
    }
}

fn status_accepts_runtime_call(
    status: RuntimeCapabilityStatus,
    reason: RuntimeCapabilityReason,
    allow_when_degraded: bool,
) -> bool {
    reason != RuntimeCapabilityReason::OperatorDisabled
        && match status {
            RuntimeCapabilityStatus::Online => true,
            RuntimeCapabilityStatus::Degraded => allow_when_degraded,
            RuntimeCapabilityStatus::Offline => false,
        }
}

fn record_runtime_capability_denied(slot: &RuntimeCapabilitySlot) {
    slot.drain_denied_total.fetch_add(1, Ordering::Relaxed);
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
    reconcile_call_state_after_update(slot, update.status, update.reason);
}

/// RAII guard for an active runtime sub-capability call.
///
/// Holding the guard keeps the capability active-call counter incremented; dropping
/// or finishing it releases the counter exactly once.
#[derive(Debug)]
pub struct RuntimeCapabilityCallGuard {
    index: usize,
    active: bool,
}

impl RuntimeCapabilityCallGuard {
    /// Runtime sub-capability id guarded by this call.
    pub fn id(&self) -> &'static str {
        RUNTIME_CAPABILITY_IDS[self.index]
    }

    /// Release the active-call slot before natural drop.
    pub fn finish(&mut self) {
        if self.active {
            decrement_active_call(&RUNTIME_CAPABILITY_STATE.slots[self.index]);
            self.active = false;
        }
    }
}

impl Drop for RuntimeCapabilityCallGuard {
    fn drop(&mut self) {
        self.finish();
    }
}

fn try_begin_runtime_capability_call_inner(
    id: &'static str,
    allow_when_degraded: bool,
) -> Result<RuntimeCapabilityCallGuard> {
    let Some(index) = capability_index(id) else {
        return Err(runtime_capability_unknown_error(id));
    };
    let slot = &RUNTIME_CAPABILITY_STATE.slots[index];
    let status = RuntimeCapabilityStatus::from_byte(slot.status.load(Ordering::Relaxed));
    let reason = RuntimeCapabilityReason::from_byte(slot.reason.load(Ordering::Relaxed));
    if !status_accepts_runtime_call(status, reason, allow_when_degraded) {
        record_runtime_capability_denied(slot);
        return Err(runtime_capability_error(
            id,
            "runtime capability is unavailable",
        ));
    }
    if let Err(message) = increment_active_call(slot) {
        record_runtime_capability_denied(slot);
        return Err(runtime_capability_error(id, message));
    }
    let word_after_begin = slot.call_word.load(Ordering::Acquire);
    let status_after_begin =
        RuntimeCapabilityStatus::from_byte(slot.status.load(Ordering::Relaxed));
    let reason_after_begin =
        RuntimeCapabilityReason::from_byte(slot.reason.load(Ordering::Relaxed));
    if !call_state_from_word(word_after_begin).accepts_new_calls()
        || !status_accepts_runtime_call(status_after_begin, reason_after_begin, allow_when_degraded)
    {
        decrement_active_call(slot);
        record_runtime_capability_denied(slot);
        return Err(runtime_capability_error(
            id,
            "runtime capability is not accepting calls",
        ));
    }
    Ok(RuntimeCapabilityCallGuard {
        index,
        active: true,
    })
}

/// Try to begin an active call against a runtime sub-capability.
///
/// Degraded capabilities are denied by default; callers that own an explicit
/// allow-when-degraded contract must use the policy-aware crate-internal variant.
pub fn try_begin_runtime_capability_call(id: &'static str) -> Result<RuntimeCapabilityCallGuard> {
    try_begin_runtime_capability_call_inner(id, false)
}

pub(crate) fn try_begin_runtime_capability_call_with_policy(
    id: &'static str,
    allow_when_degraded: bool,
) -> Result<RuntimeCapabilityCallGuard> {
    try_begin_runtime_capability_call_inner(id, allow_when_degraded)
}

/// Current active-call count for a runtime sub-capability.
pub fn runtime_capability_active_calls(id: &str) -> Option<u32> {
    let index = capability_index(id)?;
    Some(active_calls_from_word(
        RUNTIME_CAPABILITY_STATE.slots[index]
            .call_word
            .load(Ordering::Relaxed),
    ))
}

/// Total refused begin attempts for a runtime sub-capability.
pub fn runtime_capability_drain_denied_total(id: &str) -> Option<u32> {
    let index = capability_index(id)?;
    Some(
        RUNTIME_CAPABILITY_STATE.slots[index]
            .drain_denied_total
            .load(Ordering::Relaxed),
    )
}

/// Enter draining: existing active calls may finish, but new calls are refused.
pub fn begin_runtime_capability_draining(id: &'static str) -> Result<()> {
    let Some(index) = capability_index(id) else {
        return Err(runtime_capability_unknown_error(id));
    };
    set_call_state(
        &RUNTIME_CAPABILITY_STATE.slots[index],
        RuntimeCapabilityCallState::Draining,
    );
    Ok(())
}

/// Mark a runtime sub-capability as disabled by operator policy.
pub fn mark_runtime_capability_disabled(id: &'static str) -> Result<()> {
    let Some(index) = capability_index(id) else {
        return Err(runtime_capability_unknown_error(id));
    };
    set_call_state(
        &RUNTIME_CAPABILITY_STATE.slots[index],
        RuntimeCapabilityCallState::Disabled,
    );
    update_runtime_capability(RuntimeCapabilityUpdate {
        id,
        status: RuntimeCapabilityStatus::Offline,
        reason: RuntimeCapabilityReason::OperatorDisabled,
        observed_at_secs: now_secs_u32(),
        recovery_hint: None,
    });
    Ok(())
}

/// Mark a runtime sub-capability as failed.
pub fn mark_runtime_capability_failed(id: &'static str) -> Result<()> {
    let Some(index) = capability_index(id) else {
        return Err(runtime_capability_unknown_error(id));
    };
    set_call_state(
        &RUNTIME_CAPABILITY_STATE.slots[index],
        RuntimeCapabilityCallState::Failed,
    );
    update_runtime_capability(RuntimeCapabilityUpdate {
        id,
        status: RuntimeCapabilityStatus::Offline,
        reason: RuntimeCapabilityReason::WorkerDead,
        observed_at_secs: now_secs_u32(),
        recovery_hint: None,
    });
    Ok(())
}

/// Finish drain by marking a runtime sub-capability unloaded.
pub fn finish_runtime_capability_unloaded(id: &'static str) -> Result<()> {
    let Some(index) = capability_index(id) else {
        return Err(runtime_capability_unknown_error(id));
    };
    let slot = &RUNTIME_CAPABILITY_STATE.slots[index];
    let mut current = slot.call_word.load(Ordering::Relaxed);
    loop {
        let current_state = call_state_from_word(current);
        let active_calls = active_calls_from_word(current);
        if current_state != RuntimeCapabilityCallState::Draining {
            return Err(runtime_capability_error(
                id,
                "runtime capability is not draining",
            ));
        }
        if active_calls != 0 {
            return Err(runtime_capability_error(
                id,
                "runtime capability still has active calls",
            ));
        }
        let next = pack_call_word(RuntimeCapabilityCallState::Unloaded, 0);
        match slot.call_word.compare_exchange_weak(
            current,
            next,
            Ordering::AcqRel,
            Ordering::Relaxed,
        ) {
            Ok(_) => {
                slot.last_transition_uptime_ms
                    .store(now_uptime_millis_u32(), Ordering::Relaxed);
                break;
            }
            Err(actual) => current = actual,
        }
    }
    update_runtime_capability(RuntimeCapabilityUpdate {
        id,
        status: RuntimeCapabilityStatus::Offline,
        reason: RuntimeCapabilityReason::RuntimeNotInitialized,
        observed_at_secs: now_secs_u32(),
        recovery_hint: None,
    });
    Ok(())
}

pub fn runtime_capability_snapshot() -> Vec<RuntimeCapabilityState> {
    RUNTIME_CAPABILITY_IDS
        .iter()
        .enumerate()
        .map(|(index, id)| {
            let slot = &RUNTIME_CAPABILITY_STATE.slots[index];
            let status = RuntimeCapabilityStatus::from_byte(slot.status.load(Ordering::Relaxed));
            let reason = RuntimeCapabilityReason::from_byte(slot.reason.load(Ordering::Relaxed));
            let call_word = slot.call_word.load(Ordering::Relaxed);
            RuntimeCapabilityState {
                id,
                status,
                reason,
                epoch: slot.epoch.load(Ordering::Relaxed),
                changed_at_secs: slot.changed_at_secs.load(Ordering::Relaxed),
                observed_at_secs: slot.observed_at_secs.load(Ordering::Relaxed),
                active_calls: active_calls_from_word(call_word),
                draining: call_state_from_word(call_word) == RuntimeCapabilityCallState::Draining,
                last_transition_uptime_ms: slot.last_transition_uptime_ms.load(Ordering::Relaxed)
                    as u64,
                drain_denied_total: slot.drain_denied_total.load(Ordering::Relaxed) as u64,
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
    let call_word = slot.call_word.load(Ordering::Relaxed);
    Some(RuntimeCapabilityState {
        id: RUNTIME_CAPABILITY_IDS[index],
        status,
        reason,
        epoch: slot.epoch.load(Ordering::Relaxed),
        changed_at_secs: slot.changed_at_secs.load(Ordering::Relaxed),
        observed_at_secs: slot.observed_at_secs.load(Ordering::Relaxed),
        active_calls: active_calls_from_word(call_word),
        draining: call_state_from_word(call_word) == RuntimeCapabilityCallState::Draining,
        last_transition_uptime_ms: slot.last_transition_uptime_ms.load(Ordering::Relaxed) as u64,
        drain_denied_total: slot.drain_denied_total.load(Ordering::Relaxed) as u64,
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
    let snapshot = runtime_capability_snapshot();
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
    let active_calls: u32 = snapshot.iter().map(|state| state.active_calls).sum();
    let drain_denied_total: u64 = snapshot.iter().map(|state| state.drain_denied_total).sum();
    let draining_ids: Vec<&'static str> = snapshot
        .iter()
        .filter_map(|state| state.draining.then_some(state.id))
        .collect();
    let draining = if draining_ids.is_empty() {
        "none".to_string()
    } else {
        draining_ids.join(",")
    };
    format!(
        "runtime_capabilities offline={} degraded={} active_calls={} draining={} drain_denied_total={} offline_ids={} degraded_ids={}",
        summary.offline_count,
        summary.degraded_count,
        active_calls,
        draining,
        drain_denied_total,
        offline,
        degraded
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
    let outbound_network_snapshot = match platform.memory_system_kind() {
        MemorySystemKind::LinuxFull => None,
        MemorySystemKind::EspCompact => Some(crate::state::network_runtime_snapshot(
            crate::platform::time::wall_clock_is_trustworthy(),
            3,
        )),
    };
    let mut outbound_update = resolve_outbound_http_capability_update_from_network(
        outbound_http_client_ready,
        outbound_network_snapshot,
    );
    outbound_update.observed_at_secs = now_secs;
    update_runtime_capability(outbound_update);
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

#[cfg(test)]
fn resolve_outbound_http_capability_update(
    outbound_transport_ready: bool,
) -> RuntimeCapabilityUpdate {
    resolve_outbound_http_capability_update_from_network(outbound_transport_ready, None)
}

fn resolve_outbound_http_capability_update_from_network(
    outbound_http_client_ready: bool,
    network: Option<crate::state::NetworkRuntimeSnapshot>,
) -> RuntimeCapabilityUpdate {
    let prior = get_runtime_capability(RUNTIME_CAPABILITY_NETWORK_OUTBOUND_HTTP);
    let (status, reason) = if !outbound_http_client_ready {
        (
            RuntimeCapabilityStatus::Offline,
            RuntimeCapabilityReason::RuntimeNotInitialized,
        )
    } else if let Some(snapshot) = network {
        outbound_http_status_for_network_snapshot(snapshot)
    } else if prior.is_some_and(|state| {
        state.status == RuntimeCapabilityStatus::Offline
            && state.reason == RuntimeCapabilityReason::UpstreamUnavailable
    }) {
        (
            RuntimeCapabilityStatus::Degraded,
            RuntimeCapabilityReason::UpstreamUnavailable,
        )
    } else {
        (
            RuntimeCapabilityStatus::Online,
            RuntimeCapabilityReason::Nominal,
        )
    };
    RuntimeCapabilityUpdate {
        id: RUNTIME_CAPABILITY_NETWORK_OUTBOUND_HTTP,
        status,
        reason,
        observed_at_secs: 0,
        recovery_hint: None,
    }
}

fn outbound_http_status_for_network_snapshot(
    snapshot: crate::state::NetworkRuntimeSnapshot,
) -> (RuntimeCapabilityStatus, RuntimeCapabilityReason) {
    if !snapshot.sta_expected || !snapshot.sta_configured {
        return (
            RuntimeCapabilityStatus::Offline,
            RuntimeCapabilityReason::NotConfigured,
        );
    }
    if !snapshot.sta_ip_present {
        let reason = match snapshot.last_wifi_stage {
            crate::state::NetworkWifiStage::StaConnecting
            | crate::state::NetworkWifiStage::StaL2Connected
            | crate::state::NetworkWifiStage::StaWaitingDhcp
            | crate::state::NetworkWifiStage::StaIpReady
            | crate::state::NetworkWifiStage::StaRecovering => {
                RuntimeCapabilityReason::RecoveryStabilizing
            }
            crate::state::NetworkWifiStage::StaAuthFailed
            | crate::state::NetworkWifiStage::StaApNotFound
            | crate::state::NetworkWifiStage::StaFallbackAp => {
                RuntimeCapabilityReason::UpstreamUnavailable
            }
            crate::state::NetworkWifiStage::ApOnly => RuntimeCapabilityReason::UpstreamUnavailable,
        };
        let status = if reason == RuntimeCapabilityReason::RecoveryStabilizing {
            RuntimeCapabilityStatus::Degraded
        } else {
            RuntimeCapabilityStatus::Offline
        };
        return (status, reason);
    }
    if !snapshot.outbound_settled {
        return (
            RuntimeCapabilityStatus::Degraded,
            RuntimeCapabilityReason::RecoveryStabilizing,
        );
    }
    (
        RuntimeCapabilityStatus::Online,
        RuntimeCapabilityReason::Nominal,
    )
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
    let status = match reason {
        RuntimeCapabilityReason::RecoveryStabilizing => RuntimeCapabilityStatus::Degraded,
        _ => RuntimeCapabilityStatus::Offline,
    };
    update_runtime_capability(RuntimeCapabilityUpdate {
        id,
        status,
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
        slot.call_word.store(
            pack_call_word(RuntimeCapabilityCallState::Unloaded, 0),
            Ordering::Relaxed,
        );
        slot.last_transition_uptime_ms.store(0, Ordering::Relaxed);
        slot.drain_denied_total.store(0, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_runtime_capability_bumps_epoch_only_on_meaningful_change() {
        let _guard = RUNTIME_CAPABILITY_TEST_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
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

    #[test]
    fn outbound_http_platform_refresh_reopens_probe_window_after_upstream_failure() {
        let _guard = RUNTIME_CAPABILITY_TEST_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        reset_runtime_capabilities_for_tests();
        update_runtime_capability(RuntimeCapabilityUpdate {
            id: RUNTIME_CAPABILITY_NETWORK_OUTBOUND_HTTP,
            status: RuntimeCapabilityStatus::Offline,
            reason: RuntimeCapabilityReason::UpstreamUnavailable,
            observed_at_secs: 10,
            recovery_hint: None,
        });

        let next = resolve_outbound_http_capability_update(true);
        assert_eq!(next.status, RuntimeCapabilityStatus::Degraded);
        assert_eq!(next.reason, RuntimeCapabilityReason::UpstreamUnavailable);
    }

    #[test]
    fn outbound_http_platform_refresh_clears_local_recovery_failure() {
        let _guard = RUNTIME_CAPABILITY_TEST_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        reset_runtime_capabilities_for_tests();
        update_runtime_capability(RuntimeCapabilityUpdate {
            id: RUNTIME_CAPABILITY_NETWORK_OUTBOUND_HTTP,
            status: RuntimeCapabilityStatus::Offline,
            reason: RuntimeCapabilityReason::RecoveryStabilizing,
            observed_at_secs: 10,
            recovery_hint: None,
        });

        let next = resolve_outbound_http_capability_update(true);
        assert_eq!(next.status, RuntimeCapabilityStatus::Online);
        assert_eq!(next.reason, RuntimeCapabilityReason::Nominal);
    }

    #[test]
    fn outbound_http_uses_network_snapshot_to_explain_esp_blockers() {
        let _guard = RUNTIME_CAPABILITY_TEST_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _state_guard = crate::state::test_state_guard();
        reset_runtime_capabilities_for_tests();
        crate::state::set_network_sta_expected(false, false);
        crate::state::clear_wifi_sta_state();

        let ap_only = resolve_outbound_http_capability_update_from_network(
            true,
            Some(crate::state::network_runtime_snapshot(false, 3)),
        );
        assert_eq!(ap_only.status, RuntimeCapabilityStatus::Offline);
        assert_eq!(ap_only.reason, RuntimeCapabilityReason::NotConfigured);

        crate::state::set_network_sta_expected(true, true);
        crate::state::set_network_wifi_stage(crate::state::NetworkWifiStage::StaRecovering, None);
        let recovering = resolve_outbound_http_capability_update_from_network(
            true,
            Some(crate::state::network_runtime_snapshot(false, 3)),
        );
        assert_eq!(recovering.status, RuntimeCapabilityStatus::Degraded);
        assert_eq!(
            recovering.reason,
            RuntimeCapabilityReason::RecoveryStabilizing
        );

        crate::state::set_network_wifi_stage(
            crate::state::NetworkWifiStage::StaApNotFound,
            Some(201),
        );
        let unavailable = resolve_outbound_http_capability_update_from_network(
            true,
            Some(crate::state::network_runtime_snapshot(false, 3)),
        );
        assert_eq!(unavailable.status, RuntimeCapabilityStatus::Offline);
        assert_eq!(
            unavailable.reason,
            RuntimeCapabilityReason::UpstreamUnavailable
        );
    }

    #[test]
    fn runtime_capability_call_guard_tracks_active_calls_until_drop() {
        let _guard = RUNTIME_CAPABILITY_TEST_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        reset_runtime_capabilities_for_tests();
        update_runtime_capability(RuntimeCapabilityUpdate {
            id: RUNTIME_CAPABILITY_NETWORK_OUTBOUND_HTTP,
            status: RuntimeCapabilityStatus::Online,
            reason: RuntimeCapabilityReason::Nominal,
            observed_at_secs: 10,
            recovery_hint: None,
        });

        let call =
            try_begin_runtime_capability_call(RUNTIME_CAPABILITY_NETWORK_OUTBOUND_HTTP).unwrap();
        assert_eq!(
            runtime_capability_active_calls(RUNTIME_CAPABILITY_NETWORK_OUTBOUND_HTTP),
            Some(1)
        );

        drop(call);
        assert_eq!(
            runtime_capability_active_calls(RUNTIME_CAPABILITY_NETWORK_OUTBOUND_HTTP),
            Some(0)
        );
    }

    #[test]
    fn draining_capability_blocks_new_calls_until_active_call_drops() {
        let _guard = RUNTIME_CAPABILITY_TEST_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        reset_runtime_capabilities_for_tests();
        update_runtime_capability(RuntimeCapabilityUpdate {
            id: RUNTIME_CAPABILITY_NETWORK_OUTBOUND_HTTP,
            status: RuntimeCapabilityStatus::Online,
            reason: RuntimeCapabilityReason::Nominal,
            observed_at_secs: 10,
            recovery_hint: None,
        });
        let call =
            try_begin_runtime_capability_call(RUNTIME_CAPABILITY_NETWORK_OUTBOUND_HTTP).unwrap();

        begin_runtime_capability_draining(RUNTIME_CAPABILITY_NETWORK_OUTBOUND_HTTP).unwrap();
        let state = get_runtime_capability(RUNTIME_CAPABILITY_NETWORK_OUTBOUND_HTTP).unwrap();
        assert!(state.draining);
        assert_eq!(state.active_calls, 1);
        assert!(
            try_begin_runtime_capability_call(RUNTIME_CAPABILITY_NETWORK_OUTBOUND_HTTP).is_err()
        );
        assert!(
            finish_runtime_capability_unloaded(RUNTIME_CAPABILITY_NETWORK_OUTBOUND_HTTP).is_err()
        );

        drop(call);
        finish_runtime_capability_unloaded(RUNTIME_CAPABILITY_NETWORK_OUTBOUND_HTTP).unwrap();
        let unloaded = get_runtime_capability(RUNTIME_CAPABILITY_NETWORK_OUTBOUND_HTTP).unwrap();
        assert_eq!(unloaded.active_calls, 0);
        assert!(!unloaded.draining);
        assert_eq!(unloaded.status, RuntimeCapabilityStatus::Offline);
        assert_eq!(
            unloaded.reason,
            RuntimeCapabilityReason::RuntimeNotInitialized
        );
    }

    #[test]
    fn unload_requires_prior_draining_state() {
        let _guard = RUNTIME_CAPABILITY_TEST_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        reset_runtime_capabilities_for_tests();
        update_runtime_capability(RuntimeCapabilityUpdate {
            id: RUNTIME_CAPABILITY_NETWORK_OUTBOUND_HTTP,
            status: RuntimeCapabilityStatus::Online,
            reason: RuntimeCapabilityReason::Nominal,
            observed_at_secs: 10,
            recovery_hint: None,
        });

        let error = finish_runtime_capability_unloaded(RUNTIME_CAPABILITY_NETWORK_OUTBOUND_HTTP)
            .expect_err("unload must be an explicit drain completion");

        assert_eq!(error.stage(), "runtime_capability_call");
        assert!(error.to_string().contains("not draining"));
        assert!(
            try_begin_runtime_capability_call(RUNTIME_CAPABILITY_NETWORK_OUTBOUND_HTTP).is_ok()
        );
    }

    #[test]
    fn degraded_capability_denies_default_calls_but_honors_explicit_policy() {
        let _guard = RUNTIME_CAPABILITY_TEST_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        reset_runtime_capabilities_for_tests();
        update_runtime_capability(RuntimeCapabilityUpdate {
            id: RUNTIME_CAPABILITY_NETWORK_OUTBOUND_HTTP,
            status: RuntimeCapabilityStatus::Degraded,
            reason: RuntimeCapabilityReason::RecoveryStabilizing,
            observed_at_secs: 10,
            recovery_hint: None,
        });

        assert!(
            try_begin_runtime_capability_call(RUNTIME_CAPABILITY_NETWORK_OUTBOUND_HTTP).is_err()
        );
        let call = try_begin_runtime_capability_call_with_policy(
            RUNTIME_CAPABILITY_NETWORK_OUTBOUND_HTTP,
            true,
        )
        .expect("explicit allow_when_degraded policy should accept degraded calls");
        assert_eq!(
            runtime_capability_active_calls(RUNTIME_CAPABILITY_NETWORK_OUTBOUND_HTTP),
            Some(1)
        );

        drop(call);
        assert_eq!(
            runtime_capability_active_calls(RUNTIME_CAPABILITY_NETWORK_OUTBOUND_HTTP),
            Some(0)
        );
    }

    #[test]
    fn runtime_capability_call_rejects_unknown_id_with_stable_error() {
        let _guard = RUNTIME_CAPABILITY_TEST_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        reset_runtime_capabilities_for_tests();

        let error = try_begin_runtime_capability_call("unknown.capability").unwrap_err();
        assert_eq!(error.stage(), "runtime_capability_call");
        assert!(error.to_string().contains("unknown runtime capability"));
    }

    #[test]
    fn runtime_capability_call_rejects_unavailable_or_draining_states_and_counts_denials() {
        let _guard = RUNTIME_CAPABILITY_TEST_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        reset_runtime_capabilities_for_tests();

        let offline = try_begin_runtime_capability_call(RUNTIME_CAPABILITY_AUDIO_INPUT);
        assert!(offline.is_err());
        assert_eq!(
            runtime_capability_drain_denied_total(RUNTIME_CAPABILITY_AUDIO_INPUT),
            Some(1)
        );

        update_runtime_capability(RuntimeCapabilityUpdate {
            id: RUNTIME_CAPABILITY_NETWORK_OUTBOUND_HTTP,
            status: RuntimeCapabilityStatus::Online,
            reason: RuntimeCapabilityReason::Nominal,
            observed_at_secs: 10,
            recovery_hint: None,
        });
        begin_runtime_capability_draining(RUNTIME_CAPABILITY_NETWORK_OUTBOUND_HTTP).unwrap();
        let draining = try_begin_runtime_capability_call(RUNTIME_CAPABILITY_NETWORK_OUTBOUND_HTTP);
        assert!(draining.is_err());
        assert_eq!(
            runtime_capability_drain_denied_total(RUNTIME_CAPABILITY_NETWORK_OUTBOUND_HTTP),
            Some(1)
        );

        update_runtime_capability(RuntimeCapabilityUpdate {
            id: RUNTIME_CAPABILITY_STORAGE_STATE_FS,
            status: RuntimeCapabilityStatus::Online,
            reason: RuntimeCapabilityReason::Nominal,
            observed_at_secs: 10,
            recovery_hint: None,
        });
        mark_runtime_capability_disabled(RUNTIME_CAPABILITY_STORAGE_STATE_FS).unwrap();
        let disabled = try_begin_runtime_capability_call(RUNTIME_CAPABILITY_STORAGE_STATE_FS);
        assert!(disabled.is_err());
        assert_eq!(
            runtime_capability_drain_denied_total(RUNTIME_CAPABILITY_STORAGE_STATE_FS),
            Some(1)
        );

        update_runtime_capability(RuntimeCapabilityUpdate {
            id: RUNTIME_CAPABILITY_AUDIO_OUTPUT,
            status: RuntimeCapabilityStatus::Online,
            reason: RuntimeCapabilityReason::Nominal,
            observed_at_secs: 10,
            recovery_hint: None,
        });
        mark_runtime_capability_failed(RUNTIME_CAPABILITY_AUDIO_OUTPUT).unwrap();
        let failed = try_begin_runtime_capability_call(RUNTIME_CAPABILITY_AUDIO_OUTPUT);
        assert!(failed.is_err());
        assert_eq!(
            runtime_capability_drain_denied_total(RUNTIME_CAPABILITY_AUDIO_OUTPUT),
            Some(1)
        );

        finish_runtime_capability_unloaded(RUNTIME_CAPABILITY_NETWORK_OUTBOUND_HTTP).unwrap();
        let unloaded = try_begin_runtime_capability_call(RUNTIME_CAPABILITY_NETWORK_OUTBOUND_HTTP);
        assert!(unloaded.is_err());
        assert_eq!(
            runtime_capability_drain_denied_total(RUNTIME_CAPABILITY_NETWORK_OUTBOUND_HTTP),
            Some(2)
        );
    }
}
