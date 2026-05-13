//! Runtime resource lease registry.
//! 运行时稀缺资源租约表，P0.2 先提供 snapshot-only 中心模型。

use std::sync::{Mutex, OnceLock};
use std::time::Instant;

/// Resource kind governed by the central lease registry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LeaseKind {
    Display,
    CameraFrame,
    AudioInput,
    AudioOutput,
    VoiceExclusive,
    ExternalWss,
    ConfigReadBurst,
    SnapshotHttpWorker,
    ChatHistoryHttpWorker,
    ConfigHttpWorker,
    DiagnosticHttpWorker,
    StorageSessionWrite,
    AgentHeavyTurn,
    TlsHandshake,
}

impl LeaseKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Display => "display",
            Self::CameraFrame => "camera_frame",
            Self::AudioInput => "audio_input",
            Self::AudioOutput => "audio_output",
            Self::VoiceExclusive => "voice_exclusive",
            Self::ExternalWss => "external_wss",
            Self::ConfigReadBurst => "config_read_burst",
            Self::SnapshotHttpWorker => "snapshot_http_worker",
            Self::ChatHistoryHttpWorker => "chat_history_http_worker",
            Self::ConfigHttpWorker => "config_http_worker",
            Self::DiagnosticHttpWorker => "diagnostic_http_worker",
            Self::StorageSessionWrite => "storage_session_write",
            Self::AgentHeavyTurn => "agent_heavy_turn",
            Self::TlsHandshake => "tls_handshake",
        }
    }
}

/// Logical owner of a lease record.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize)]
pub struct LeaseOwner {
    pub plane: &'static str,
    pub name: &'static str,
}

impl LeaseOwner {
    pub const fn new(plane: &'static str, name: &'static str) -> Self {
        Self { plane, name }
    }
}

/// Lease sharing mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LeaseMode {
    Exclusive,
    Shared,
}

/// Whether an expired lease may be replaced during acquisition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LeaseReplacePolicy {
    ReplaceExpired,
    Never,
}

/// Active or retained lease record.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub struct LeaseRecord {
    pub token: u64,
    pub kind: LeaseKind,
    pub owner: LeaseOwner,
    pub mode: LeaseMode,
    pub acquired_at_ms: u64,
    pub expires_at_ms: Option<u64>,
    pub hold_count: u32,
}

/// Denial returned by a failed non-blocking lease acquisition.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub struct LeaseDenial {
    pub kind: LeaseKind,
    pub owner: LeaseOwner,
    pub held_by: Option<LeaseOwner>,
    pub reason: &'static str,
}

/// Result of a non-blocking lease acquisition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LeaseDecision {
    Acquired(LeaseRecord),
    Reentered(LeaseRecord),
    ReplacedExpired {
        previous: LeaseRecord,
        current: LeaseRecord,
    },
    Denied(LeaseDenial),
}

/// Serializable lease record with expiry state evaluated at snapshot time.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub struct LeaseSnapshotRecord {
    pub token: u64,
    pub kind: LeaseKind,
    pub owner: LeaseOwner,
    pub mode: LeaseMode,
    pub acquired_at_ms: u64,
    pub expires_at_ms: Option<u64>,
    pub hold_count: u32,
    pub expired: bool,
}

/// Point-in-time lease registry snapshot.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct LeaseSnapshot {
    pub total_records: usize,
    pub active_count: usize,
    pub expired_count: usize,
    pub exclusive_count: usize,
    pub shared_count: usize,
    pub records: Vec<LeaseSnapshotRecord>,
}

#[derive(Default)]
struct LeaseRegistry {
    records: Vec<LeaseRecord>,
    next_token: u64,
}

static LEASES: OnceLock<Mutex<LeaseRegistry>> = OnceLock::new();
static MONOTONIC_START: OnceLock<Instant> = OnceLock::new();

fn registry() -> &'static Mutex<LeaseRegistry> {
    LEASES.get_or_init(|| {
        Mutex::new(LeaseRegistry {
            records: Vec::new(),
            next_token: 1,
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

fn is_expired(record: &LeaseRecord, now_ms: u64) -> bool {
    record
        .expires_at_ms
        .is_some_and(|expires| now_ms >= expires)
}

fn next_token(registry: &mut LeaseRegistry) -> u64 {
    let token = registry.next_token;
    registry.next_token = registry.next_token.saturating_add(1).max(1);
    token
}

fn new_record(
    registry: &mut LeaseRegistry,
    kind: LeaseKind,
    owner: LeaseOwner,
    mode: LeaseMode,
    ttl_ms: Option<u64>,
    now_ms: u64,
) -> LeaseRecord {
    LeaseRecord {
        token: next_token(registry),
        kind,
        owner,
        mode,
        acquired_at_ms: now_ms,
        expires_at_ms: ttl_ms.map(|ttl| now_ms.saturating_add(ttl)),
        hold_count: 1,
    }
}

/// Acquire a lease using the process monotonic clock.
pub fn try_acquire(
    kind: LeaseKind,
    owner: LeaseOwner,
    mode: LeaseMode,
    ttl_ms: Option<u64>,
) -> LeaseDecision {
    try_acquire_at(kind, owner, mode, ttl_ms, now_ms())
}

/// Acquire a lease at a supplied monotonic timestamp.
pub fn try_acquire_at(
    kind: LeaseKind,
    owner: LeaseOwner,
    mode: LeaseMode,
    ttl_ms: Option<u64>,
    now_ms: u64,
) -> LeaseDecision {
    try_acquire_with_policy_at(
        kind,
        owner,
        mode,
        ttl_ms,
        now_ms,
        LeaseReplacePolicy::ReplaceExpired,
    )
}

/// Acquire a lease with an explicit expired-record replacement policy.
pub fn try_acquire_with_policy_at(
    kind: LeaseKind,
    owner: LeaseOwner,
    mode: LeaseMode,
    ttl_ms: Option<u64>,
    now_ms: u64,
    replace_policy: LeaseReplacePolicy,
) -> LeaseDecision {
    let mut guard = registry().lock().unwrap_or_else(|e| e.into_inner());

    if let Some(denied) =
        deny_expired_replacement_if_forbidden(&guard, kind, owner, now_ms, replace_policy)
    {
        return denied;
    }

    let previous_expired = if replace_policy == LeaseReplacePolicy::ReplaceExpired {
        remove_expired_for_kind(&mut guard, kind, now_ms)
    } else {
        None
    };

    if let Some(index) = guard
        .records
        .iter()
        .position(|record| record.kind == kind && record.owner == owner)
    {
        if guard.records[index].mode != mode {
            return LeaseDecision::Denied(LeaseDenial {
                kind,
                owner,
                held_by: Some(owner),
                reason: "mode_mismatch",
            });
        }
        guard.records[index].hold_count = guard.records[index].hold_count.saturating_add(1);
        return LeaseDecision::Reentered(guard.records[index]);
    }

    if let Some(conflict) = first_active_conflict(&guard.records, kind, mode, now_ms) {
        return LeaseDecision::Denied(LeaseDenial {
            kind,
            owner,
            held_by: Some(conflict.owner),
            reason: conflict_reason(conflict.mode, mode),
        });
    }

    let current = new_record(&mut guard, kind, owner, mode, ttl_ms, now_ms);
    guard.records.push(current);
    if let Some(previous) = previous_expired {
        LeaseDecision::ReplacedExpired { previous, current }
    } else {
        LeaseDecision::Acquired(current)
    }
}

/// Acquire an exclusive lease only when no active record for `kind` exists.
pub fn try_acquire_exclusive_once(
    kind: LeaseKind,
    owner: LeaseOwner,
    ttl_ms: Option<u64>,
    replace_policy: LeaseReplacePolicy,
) -> LeaseDecision {
    try_acquire_exclusive_once_at(kind, owner, ttl_ms, now_ms(), replace_policy)
}

/// Acquire an exclusive lease at a supplied timestamp without owner reentry.
pub fn try_acquire_exclusive_once_at(
    kind: LeaseKind,
    owner: LeaseOwner,
    ttl_ms: Option<u64>,
    now_ms: u64,
    replace_policy: LeaseReplacePolicy,
) -> LeaseDecision {
    let mut guard = registry().lock().unwrap_or_else(|e| e.into_inner());

    if let Some(denied) =
        deny_expired_replacement_if_forbidden(&guard, kind, owner, now_ms, replace_policy)
    {
        return denied;
    }

    let previous_expired = if replace_policy == LeaseReplacePolicy::ReplaceExpired {
        remove_expired_for_kind(&mut guard, kind, now_ms)
    } else {
        None
    };

    if let Some(conflict) = guard
        .records
        .iter()
        .find(|record| record.kind == kind && !is_expired(record, now_ms))
    {
        return LeaseDecision::Denied(LeaseDenial {
            kind,
            owner,
            held_by: Some(conflict.owner),
            reason: "exclusive_conflict",
        });
    }

    let current = new_record(
        &mut guard,
        kind,
        owner,
        LeaseMode::Exclusive,
        ttl_ms,
        now_ms,
    );
    guard.records.push(current);
    if let Some(previous) = previous_expired {
        LeaseDecision::ReplacedExpired { previous, current }
    } else {
        LeaseDecision::Acquired(current)
    }
}

fn deny_expired_replacement_if_forbidden(
    registry: &LeaseRegistry,
    kind: LeaseKind,
    owner: LeaseOwner,
    now_ms: u64,
    replace_policy: LeaseReplacePolicy,
) -> Option<LeaseDecision> {
    if replace_policy != LeaseReplacePolicy::Never
        || !registry
            .records
            .iter()
            .any(|record| record.kind == kind && is_expired(record, now_ms))
        || registry
            .records
            .iter()
            .any(|record| record.kind == kind && !is_expired(record, now_ms))
    {
        return None;
    }

    let held_by = registry
        .records
        .iter()
        .find(|record| record.kind == kind)
        .map(|record| record.owner);
    Some(LeaseDecision::Denied(LeaseDenial {
        kind,
        owner,
        held_by,
        reason: "expired_replacement_forbidden",
    }))
}

fn remove_expired_for_kind(
    registry: &mut LeaseRegistry,
    kind: LeaseKind,
    now_ms: u64,
) -> Option<LeaseRecord> {
    let mut first = None;
    let mut index = 0;
    while index < registry.records.len() {
        if registry.records[index].kind == kind && is_expired(&registry.records[index], now_ms) {
            let removed = registry.records.remove(index);
            first.get_or_insert(removed);
        } else {
            index += 1;
        }
    }
    first
}

fn first_active_conflict(
    records: &[LeaseRecord],
    kind: LeaseKind,
    requested_mode: LeaseMode,
    now_ms: u64,
) -> Option<&LeaseRecord> {
    records
        .iter()
        .filter(|record| record.kind == kind && !is_expired(record, now_ms))
        .find(|record| {
            record.mode == LeaseMode::Exclusive || requested_mode == LeaseMode::Exclusive
        })
}

fn conflict_reason(held_mode: LeaseMode, requested_mode: LeaseMode) -> &'static str {
    match (held_mode, requested_mode) {
        (LeaseMode::Exclusive, _) => "exclusive_conflict",
        (_, LeaseMode::Exclusive) => "shared_conflict",
        _ => "shared_conflict",
    }
}

/// Release one hold for an owner/kind pair.
pub fn release(kind: LeaseKind, owner: LeaseOwner) -> bool {
    let mut guard = registry().lock().unwrap_or_else(|e| e.into_inner());
    release_locked(&mut guard, kind, owner, None)
}

/// Release one hold for an owner/kind pair only when the token matches.
pub fn release_token(kind: LeaseKind, owner: LeaseOwner, token: u64) -> bool {
    let mut guard = registry().lock().unwrap_or_else(|e| e.into_inner());
    release_locked(&mut guard, kind, owner, Some(token))
}

fn release_locked(
    registry: &mut LeaseRegistry,
    kind: LeaseKind,
    owner: LeaseOwner,
    token: Option<u64>,
) -> bool {
    let Some(index) = registry.records.iter().position(|record| {
        record.kind == kind && record.owner == owner && token.is_none_or(|t| record.token == t)
    }) else {
        return false;
    };

    if registry.records[index].hold_count > 1 {
        registry.records[index].hold_count -= 1;
    } else {
        registry.records.remove(index);
    }
    true
}

/// Release all records owned by `owner`.
pub fn release_owner(owner: LeaseOwner) -> usize {
    let mut guard = registry().lock().unwrap_or_else(|e| e.into_inner());
    let before = guard.records.len();
    guard.records.retain(|record| record.owner != owner);
    before.saturating_sub(guard.records.len())
}

/// Release all records for one resource kind.
pub fn release_kind(kind: LeaseKind) -> usize {
    let mut guard = registry().lock().unwrap_or_else(|e| e.into_inner());
    let before = guard.records.len();
    guard.records.retain(|record| record.kind != kind);
    before.saturating_sub(guard.records.len())
}

/// Snapshot leases using the process monotonic clock.
pub fn snapshot() -> LeaseSnapshot {
    snapshot_at(now_ms())
}

/// Return a compact lease baseline for heartbeat logs.
pub fn format_baseline_log_line() -> String {
    let snapshot = snapshot();
    format!(
        "leases total={} active={} expired={} exclusive={} shared={}",
        snapshot.total_records,
        snapshot.active_count,
        snapshot.expired_count,
        snapshot.exclusive_count,
        snapshot.shared_count
    )
}

/// Snapshot leases at a supplied monotonic timestamp.
pub fn snapshot_at(now_ms: u64) -> LeaseSnapshot {
    let guard = registry().lock().unwrap_or_else(|e| e.into_inner());
    snapshot_from_records(&guard.records, now_ms)
}

/// Count active records for one resource kind using the process monotonic clock.
pub fn active_count_for_kind(kind: LeaseKind) -> usize {
    active_count_for_kind_at(kind, now_ms())
}

/// Count active records for one resource kind at a supplied timestamp.
pub fn active_count_for_kind_at(kind: LeaseKind, now_ms: u64) -> usize {
    let guard = registry().lock().unwrap_or_else(|e| e.into_inner());
    guard
        .records
        .iter()
        .filter(|record| record.kind == kind && !is_expired(record, now_ms))
        .count()
}

fn snapshot_from_records(records: &[LeaseRecord], now_ms: u64) -> LeaseSnapshot {
    let mut rows: Vec<LeaseSnapshotRecord> = records
        .iter()
        .map(|record| {
            let expired = is_expired(record, now_ms);
            LeaseSnapshotRecord {
                token: record.token,
                kind: record.kind,
                owner: record.owner,
                mode: record.mode,
                acquired_at_ms: record.acquired_at_ms,
                expires_at_ms: record.expires_at_ms,
                hold_count: record.hold_count,
                expired,
            }
        })
        .collect();
    rows.sort_by(|left, right| {
        left.kind
            .as_str()
            .cmp(right.kind.as_str())
            .then_with(|| left.owner.plane.cmp(right.owner.plane))
            .then_with(|| left.owner.name.cmp(right.owner.name))
            .then_with(|| left.token.cmp(&right.token))
    });

    let active_count = rows.iter().filter(|record| !record.expired).count();
    let expired_count = rows.len().saturating_sub(active_count);
    let exclusive_count = rows
        .iter()
        .filter(|record| !record.expired && record.mode == LeaseMode::Exclusive)
        .count();
    let shared_count = rows
        .iter()
        .filter(|record| !record.expired && record.mode == LeaseMode::Shared)
        .count();

    LeaseSnapshot {
        total_records: rows.len(),
        active_count,
        expired_count,
        exclusive_count,
        shared_count,
        records: rows,
    }
}

#[cfg(test)]
fn reset_for_tests() {
    let mut guard = registry().lock().unwrap_or_else(|e| e.into_inner());
    guard.records.clear();
    guard.next_token = 1;
}

#[cfg(test)]
pub(crate) fn lease_test_guard() -> std::sync::MutexGuard<'static, ()> {
    static TEST_LOCK: Mutex<()> = Mutex::new(());
    let guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    reset_for_tests();
    guard
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_owner_reentrant_lease_requires_matching_releases() {
        let _guard = lease_test_guard();
        let owner = LeaseOwner::new("display", "display_loop");

        assert!(matches!(
            try_acquire_at(LeaseKind::Display, owner, LeaseMode::Exclusive, None, 100),
            LeaseDecision::Acquired(_)
        ));
        let reentered = try_acquire_at(LeaseKind::Display, owner, LeaseMode::Exclusive, None, 110);

        match reentered {
            LeaseDecision::Reentered(record) => assert_eq!(record.hold_count, 2),
            other => panic!("expected reentrant lease, got {other:?}"),
        }

        assert!(release(LeaseKind::Display, owner));
        assert_eq!(snapshot_at(120).active_count, 1);
        assert!(release(LeaseKind::Display, owner));
        assert_eq!(snapshot_at(130).active_count, 0);
    }

    #[test]
    fn different_owner_exclusive_conflict_is_denied() {
        let _guard = lease_test_guard();
        let display = LeaseOwner::new("display", "display_loop");
        let voice = LeaseOwner::new("voice", "voice_session");

        let _ = try_acquire_at(LeaseKind::Display, display, LeaseMode::Exclusive, None, 100);
        let denied = try_acquire_at(LeaseKind::Display, voice, LeaseMode::Exclusive, None, 101);

        match denied {
            LeaseDecision::Denied(denial) => {
                assert_eq!(denial.held_by, Some(display));
                assert_eq!(denial.reason, "exclusive_conflict");
            }
            other => panic!("expected conflict denial, got {other:?}"),
        }
    }

    #[test]
    fn expired_lease_can_be_replaced_by_new_owner() {
        let _guard = lease_test_guard();
        let old = LeaseOwner::new("config", "http_config_exec");
        let new = LeaseOwner::new("diag", "http_diag_exec");

        let _ = try_acquire_at(
            LeaseKind::TlsHandshake,
            old,
            LeaseMode::Exclusive,
            Some(50),
            100,
        );
        let decision = try_acquire_at(
            LeaseKind::TlsHandshake,
            new,
            LeaseMode::Exclusive,
            Some(50),
            151,
        );

        match decision {
            LeaseDecision::ReplacedExpired { previous, current } => {
                assert_eq!(previous.owner, old);
                assert_eq!(current.owner, new);
            }
            other => panic!("expected expired replacement, got {other:?}"),
        }
        assert_eq!(snapshot_at(152).active_count, 1);
    }

    #[test]
    fn shared_mode_allows_multiple_shared_owners() {
        let _guard = lease_test_guard();
        let left = LeaseOwner::new("diag", "snapshot");
        let right = LeaseOwner::new("diag", "status");

        assert!(matches!(
            try_acquire_at(
                LeaseKind::DiagnosticHttpWorker,
                left,
                LeaseMode::Shared,
                None,
                100
            ),
            LeaseDecision::Acquired(_)
        ));
        assert!(matches!(
            try_acquire_at(
                LeaseKind::DiagnosticHttpWorker,
                right,
                LeaseMode::Shared,
                None,
                101
            ),
            LeaseDecision::Acquired(_)
        ));

        let snapshot = snapshot_at(102);
        assert_eq!(snapshot.active_count, 2);
        assert_eq!(snapshot.shared_count, 2);
    }

    #[test]
    fn shared_and_exclusive_modes_conflict() {
        let _guard = lease_test_guard();
        let shared = LeaseOwner::new("diag", "snapshot");
        let exclusive = LeaseOwner::new("config", "http_config_exec");

        let _ = try_acquire_at(
            LeaseKind::DiagnosticHttpWorker,
            shared,
            LeaseMode::Shared,
            None,
            100,
        );
        let denied = try_acquire_at(
            LeaseKind::DiagnosticHttpWorker,
            exclusive,
            LeaseMode::Exclusive,
            None,
            101,
        );

        match denied {
            LeaseDecision::Denied(denial) => assert_eq!(denial.reason, "shared_conflict"),
            other => panic!("expected shared conflict, got {other:?}"),
        }
    }

    #[test]
    fn release_token_rejects_wrong_owner_or_stale_token() {
        let _guard = lease_test_guard();
        let owner = LeaseOwner::new("agent", "agent_loop");
        let wrong = LeaseOwner::new("agent", "other_agent");

        let acquired = match try_acquire_at(
            LeaseKind::AgentHeavyTurn,
            owner,
            LeaseMode::Exclusive,
            None,
            100,
        ) {
            LeaseDecision::Acquired(record) => record,
            other => panic!("expected acquire, got {other:?}"),
        };

        assert!(!release_token(
            LeaseKind::AgentHeavyTurn,
            wrong,
            acquired.token
        ));
        assert!(!release_token(
            LeaseKind::AgentHeavyTurn,
            owner,
            acquired.token + 1
        ));
        assert_eq!(snapshot_at(101).active_count, 1);
        assert!(release_token(
            LeaseKind::AgentHeavyTurn,
            owner,
            acquired.token
        ));
        assert_eq!(snapshot_at(102).active_count, 0);
    }

    #[test]
    fn expired_replacement_can_be_disabled() {
        let _guard = lease_test_guard();
        let old = LeaseOwner::new("config", "http_config_exec");
        let new = LeaseOwner::new("diag", "http_diag_exec");

        let _ = try_acquire_at(
            LeaseKind::TlsHandshake,
            old,
            LeaseMode::Exclusive,
            Some(50),
            100,
        );
        let denied = try_acquire_with_policy_at(
            LeaseKind::TlsHandshake,
            new,
            LeaseMode::Exclusive,
            None,
            151,
            LeaseReplacePolicy::Never,
        );

        match denied {
            LeaseDecision::Denied(denial) => {
                assert_eq!(denial.held_by, Some(old));
                assert_eq!(denial.reason, "expired_replacement_forbidden");
            }
            other => panic!("expected expired replacement denial, got {other:?}"),
        }
        let snapshot = snapshot_at(152);
        assert_eq!(snapshot.active_count, 0);
        assert_eq!(snapshot.expired_count, 1);
    }

    #[test]
    fn release_owner_clears_all_owned_records() {
        let _guard = lease_test_guard();
        let owner = LeaseOwner::new("voice", "voice_session");

        let _ = try_acquire_at(
            LeaseKind::AudioInput,
            owner,
            LeaseMode::Exclusive,
            None,
            100,
        );
        let _ = try_acquire_at(
            LeaseKind::AudioOutput,
            owner,
            LeaseMode::Exclusive,
            None,
            100,
        );

        assert_eq!(release_owner(owner), 2);
        assert_eq!(snapshot_at(101).active_count, 0);
    }

    #[test]
    fn snapshot_order_is_deterministic_and_compact() {
        let _guard = lease_test_guard();
        let voice = LeaseOwner::new("voice", "voice_session");
        let display = LeaseOwner::new("display", "display_loop");

        let _ = try_acquire_at(LeaseKind::Display, display, LeaseMode::Exclusive, None, 100);
        let _ = try_acquire_at(
            LeaseKind::AudioOutput,
            voice,
            LeaseMode::Exclusive,
            None,
            100,
        );

        let snapshot = snapshot_at(101);
        let kinds: Vec<_> = snapshot.records.iter().map(|record| record.kind).collect();
        assert_eq!(kinds, vec![LeaseKind::AudioOutput, LeaseKind::Display]);
    }

    #[test]
    fn active_count_for_kind_and_release_kind_scope_to_requested_resource() {
        let _guard = lease_test_guard();
        let qq = LeaseOwner::new("channel_wss", "qq_ws");
        let display = LeaseOwner::new("display", "default");

        let _ = try_acquire_at(LeaseKind::ExternalWss, qq, LeaseMode::Exclusive, None, 100);
        let _ = try_acquire_at(LeaseKind::Display, display, LeaseMode::Exclusive, None, 101);

        assert_eq!(active_count_for_kind_at(LeaseKind::ExternalWss, 102), 1);
        assert_eq!(active_count_for_kind_at(LeaseKind::Display, 102), 1);
        assert_eq!(release_kind(LeaseKind::ExternalWss), 1);
        assert_eq!(active_count_for_kind_at(LeaseKind::ExternalWss, 103), 0);
        assert_eq!(active_count_for_kind_at(LeaseKind::Display, 103), 1);
    }

    #[test]
    fn baseline_log_line_reports_compact_lease_counts() {
        let _guard = lease_test_guard();
        let owner = LeaseOwner::new("display", "display_loop");
        let _ = try_acquire_at(LeaseKind::Display, owner, LeaseMode::Exclusive, None, 100);

        let line = format_baseline_log_line();
        assert!(line.contains("leases total="));
        assert!(line.contains("active=1"));
        assert!(line.contains("exclusive=1"));
        assert!(line.contains("shared=0"));
    }

    #[test]
    fn every_plane_required_lease_uses_the_runtime_lease_kind() {
        for profile in crate::runtime::plane::profiles() {
            for lease in profile.required_leases {
                assert!(!lease.as_str().is_empty());
            }
        }
    }
}
