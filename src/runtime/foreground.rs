//! Runtime foreground ticket registry.
//! 运行时前台交互 ticket 注册表。

use std::sync::{Mutex, OnceLock};
use std::time::Instant;

/// Default idle window for user-visible foreground work.
pub const RUNTIME_FOREGROUND_IDLE_SECS: u64 = 30;
const RUNTIME_FOREGROUND_IDLE_MS: u64 = RUNTIME_FOREGROUND_IDLE_SECS * 1_000;
/// Short recovery window after foreground work releases its exclusive pressure.
pub const RUNTIME_FOREGROUND_RECOVERY_SECS: u64 = 10;
const RUNTIME_FOREGROUND_RECOVERY_MS: u64 = RUNTIME_FOREGROUND_RECOVERY_SECS * 1_000;
const FOREGROUND_RECORD_LIMIT: usize = 16;

/// Source that creates or renews a user-visible runtime foreground ticket.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeForegroundSource {
    ExternalUserMessage,
    ConfigUiChat,
    RealtimeVoiceSession,
    VoiceFallbackInteraction,
    ManualOperatorAction,
}

impl RuntimeForegroundSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ExternalUserMessage => "external_user_message",
            Self::ConfigUiChat => "config_ui_chat",
            Self::RealtimeVoiceSession => "realtime_voice_session",
            Self::VoiceFallbackInteraction => "voice_fallback_interaction",
            Self::ManualOperatorAction => "manual_operator_action",
        }
    }
}

/// Lightweight handle returned to the foreground work owner.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize)]
pub struct RuntimeForegroundTicket {
    pub id: u64,
    pub source: RuntimeForegroundSource,
}

/// Foreground ticket state retained for diagnostics.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeForegroundTicketState {
    Active,
    Finished,
    Expired,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub struct RuntimeForegroundTicketRecord {
    pub ticket: RuntimeForegroundTicket,
    pub state: RuntimeForegroundTicketState,
    pub started_at_ms: u64,
    pub renewed_at_ms: u64,
    pub expires_at_ms: u64,
}

/// Compact foreground overlay embedded in runtime-mode and scheduler snapshots.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub struct RuntimeForegroundOverlay {
    pub active: bool,
    pub active_count: usize,
    pub primary_source: Option<RuntimeForegroundSource>,
    pub age_ms: Option<u64>,
    pub resume_after_ms: Option<u64>,
    pub recovery_active: bool,
    pub recovery_source: Option<RuntimeForegroundSource>,
    pub recovery_age_ms: Option<u64>,
    pub recovery_resume_after_ms: Option<u64>,
}

/// Full foreground snapshot for diagnostics and tests.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct RuntimeForegroundSnapshot {
    pub active: bool,
    pub active_count: usize,
    pub primary_source: Option<RuntimeForegroundSource>,
    pub age_ms: Option<u64>,
    pub resume_after_ms: Option<u64>,
    pub recovery_active: bool,
    pub recovery_source: Option<RuntimeForegroundSource>,
    pub recovery_age_ms: Option<u64>,
    pub recovery_resume_after_ms: Option<u64>,
    pub records: Vec<RuntimeForegroundTicketRecord>,
}

impl RuntimeForegroundSnapshot {
    pub fn overlay(&self) -> RuntimeForegroundOverlay {
        RuntimeForegroundOverlay {
            active: self.active,
            active_count: self.active_count,
            primary_source: self.primary_source,
            age_ms: self.age_ms,
            resume_after_ms: self.resume_after_ms,
            recovery_active: self.recovery_active,
            recovery_source: self.recovery_source,
            recovery_age_ms: self.recovery_age_ms,
            recovery_resume_after_ms: self.recovery_resume_after_ms,
        }
    }
}

#[derive(Default)]
struct ForegroundRegistry {
    records: Vec<RuntimeForegroundTicketRecord>,
    next_id: u64,
}

static FOREGROUND: OnceLock<Mutex<ForegroundRegistry>> = OnceLock::new();
static MONOTONIC_START: OnceLock<Instant> = OnceLock::new();

fn registry() -> &'static Mutex<ForegroundRegistry> {
    FOREGROUND.get_or_init(|| {
        Mutex::new(ForegroundRegistry {
            records: Vec::new(),
            next_id: 1,
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

fn active_at(record: &RuntimeForegroundTicketRecord, now_ms: u64) -> bool {
    record.state == RuntimeForegroundTicketState::Active
        && now_ms >= record.started_at_ms
        && now_ms < record.expires_at_ms
}

fn normalize_expired(records: &mut [RuntimeForegroundTicketRecord], now_ms: u64) {
    for record in records {
        if record.state == RuntimeForegroundTicketState::Active && now_ms >= record.expires_at_ms {
            record.state = RuntimeForegroundTicketState::Expired;
        }
    }
}

fn prune_records(records: &mut Vec<RuntimeForegroundTicketRecord>) {
    if records.len() <= FOREGROUND_RECORD_LIMIT {
        return;
    }
    let mut remove_count = records.len() - FOREGROUND_RECORD_LIMIT;
    records.retain(|record| {
        if remove_count > 0 && record.state != RuntimeForegroundTicketState::Active {
            remove_count -= 1;
            false
        } else {
            true
        }
    });
    if records.len() > FOREGROUND_RECORD_LIMIT {
        let overflow = records.len() - FOREGROUND_RECORD_LIMIT;
        records.drain(0..overflow);
    }
}

/// Create or renew a runtime foreground ticket at the supplied monotonic time.
pub fn renew_runtime_foreground(
    source: RuntimeForegroundSource,
    now_ms: u64,
) -> RuntimeForegroundTicket {
    let mut guard = registry().lock().unwrap_or_else(|e| e.into_inner());
    normalize_expired(&mut guard.records, now_ms);
    if let Some(record) = guard
        .records
        .iter_mut()
        .find(|record| record.ticket.source == source && active_at(record, now_ms))
    {
        record.renewed_at_ms = now_ms;
        record.expires_at_ms = now_ms.saturating_add(RUNTIME_FOREGROUND_IDLE_MS);
        return record.ticket;
    }
    let id = guard.next_id.max(1);
    guard.next_id = guard.next_id.saturating_add(1).max(1);
    let ticket = RuntimeForegroundTicket { id, source };
    guard.records.push(RuntimeForegroundTicketRecord {
        ticket,
        state: RuntimeForegroundTicketState::Active,
        started_at_ms: now_ms,
        renewed_at_ms: now_ms,
        expires_at_ms: now_ms.saturating_add(RUNTIME_FOREGROUND_IDLE_MS),
    });
    prune_records(&mut guard.records);
    ticket
}

/// Create or renew a runtime foreground ticket at the process monotonic clock.
pub fn renew_runtime_foreground_now(source: RuntimeForegroundSource) -> RuntimeForegroundTicket {
    renew_runtime_foreground(source, now_ms())
}

/// Finish a foreground ticket. Returns false when the ticket is unknown or stale.
pub fn finish_runtime_foreground(ticket: RuntimeForegroundTicket) -> bool {
    finish_runtime_foreground_at(ticket, now_ms())
}

fn finish_runtime_foreground_at(ticket: RuntimeForegroundTicket, now_ms: u64) -> bool {
    let mut guard = registry().lock().unwrap_or_else(|e| e.into_inner());
    let Some(record) = guard
        .records
        .iter_mut()
        .find(|record| record.ticket == ticket)
    else {
        return false;
    };
    if record.state != RuntimeForegroundTicketState::Active {
        return false;
    }
    record.state = RuntimeForegroundTicketState::Finished;
    record.expires_at_ms = now_ms.max(record.started_at_ms);
    prune_records(&mut guard.records);
    drop(guard);
    crate::bg_timer::notify_deadline_changed();
    true
}

/// Snapshot foreground state at the process monotonic clock.
pub fn runtime_foreground_snapshot() -> RuntimeForegroundSnapshot {
    runtime_foreground_snapshot_at(now_ms())
}

/// Snapshot only the compact foreground overlay at the process monotonic clock.
pub fn runtime_foreground_overlay() -> RuntimeForegroundOverlay {
    runtime_foreground_overlay_at(now_ms())
}

/// Snapshot foreground state at a supplied monotonic timestamp.
pub fn runtime_foreground_snapshot_at(now_ms: u64) -> RuntimeForegroundSnapshot {
    let mut guard = registry().lock().unwrap_or_else(|e| e.into_inner());
    normalize_expired(&mut guard.records, now_ms);
    prune_records(&mut guard.records);
    build_snapshot(&guard.records, now_ms)
}

/// Snapshot only the compact foreground overlay at a supplied monotonic timestamp.
pub fn runtime_foreground_overlay_at(now_ms: u64) -> RuntimeForegroundOverlay {
    let mut guard = registry().lock().unwrap_or_else(|e| e.into_inner());
    normalize_expired(&mut guard.records, now_ms);
    prune_records(&mut guard.records);
    build_overlay(&guard.records, now_ms)
}

/// Return whether runtime foreground is active at the supplied monotonic time.
pub fn runtime_foreground_active(now_ms: u64) -> bool {
    runtime_foreground_overlay_at(now_ms).active
}

fn build_snapshot(
    records: &[RuntimeForegroundTicketRecord],
    now_ms: u64,
) -> RuntimeForegroundSnapshot {
    let overlay = build_overlay(records, now_ms);
    RuntimeForegroundSnapshot {
        active: overlay.active,
        active_count: overlay.active_count,
        primary_source: overlay.primary_source,
        age_ms: overlay.age_ms,
        resume_after_ms: overlay.resume_after_ms,
        recovery_active: overlay.recovery_active,
        recovery_source: overlay.recovery_source,
        recovery_age_ms: overlay.recovery_age_ms,
        recovery_resume_after_ms: overlay.recovery_resume_after_ms,
        records: records.to_vec(),
    }
}

fn build_overlay(
    records: &[RuntimeForegroundTicketRecord],
    now_ms: u64,
) -> RuntimeForegroundOverlay {
    let mut active_count = 0;
    let mut primary: Option<RuntimeForegroundTicketRecord> = None;
    let mut resume_after_ms: Option<u64> = None;
    for record in records
        .iter()
        .copied()
        .filter(|record| active_at(record, now_ms))
    {
        active_count += 1;
        if primary
            .map(|current| record.renewed_at_ms > current.renewed_at_ms)
            .unwrap_or(true)
        {
            primary = Some(record);
        }
        let resume_after = record.expires_at_ms.saturating_sub(now_ms);
        resume_after_ms =
            Some(resume_after_ms.map_or(resume_after, |current| current.max(resume_after)));
    }
    let active = active_count > 0;
    let mut recovery: Option<RuntimeForegroundTicketRecord> = None;
    let mut recovery_resume_after_ms: Option<u64> = None;
    if !active {
        for record in records.iter().copied().filter(|record| {
            record.state != RuntimeForegroundTicketState::Active
                && record.expires_at_ms <= now_ms
                && now_ms
                    < record
                        .expires_at_ms
                        .saturating_add(RUNTIME_FOREGROUND_RECOVERY_MS)
        }) {
            if recovery
                .map(|current| record.expires_at_ms > current.expires_at_ms)
                .unwrap_or(true)
            {
                let resume_after = record
                    .expires_at_ms
                    .saturating_add(RUNTIME_FOREGROUND_RECOVERY_MS)
                    .saturating_sub(now_ms);
                recovery = Some(record);
                recovery_resume_after_ms = Some(resume_after);
            }
        }
    }
    RuntimeForegroundOverlay {
        active,
        active_count,
        primary_source: primary.map(|record| record.ticket.source),
        age_ms: primary.map(|record| now_ms.saturating_sub(record.renewed_at_ms)),
        resume_after_ms,
        recovery_active: recovery.is_some(),
        recovery_source: recovery.map(|record| record.ticket.source),
        recovery_age_ms: recovery.map(|record| now_ms.saturating_sub(record.expires_at_ms)),
        recovery_resume_after_ms,
    }
}

#[cfg(test)]
pub fn reset_runtime_foreground_for_tests() {
    let mut guard = registry().lock().unwrap_or_else(|e| e.into_inner());
    guard.records.clear();
    guard.next_id = 1;
}

#[cfg(test)]
pub fn runtime_foreground_test_guard() -> std::sync::MutexGuard<'static, ()> {
    static GUARD: OnceLock<Mutex<()>> = OnceLock::new();
    GUARD
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn foreground_sources_renew_one_runtime_snapshot_without_lease_denial() {
        let _guard = runtime_foreground_test_guard();
        reset_runtime_foreground_for_tests();

        let external =
            renew_runtime_foreground(RuntimeForegroundSource::ExternalUserMessage, 1_000);
        let chat = renew_runtime_foreground(RuntimeForegroundSource::ConfigUiChat, 1_500);
        let voice = renew_runtime_foreground(RuntimeForegroundSource::RealtimeVoiceSession, 2_000);
        let fallback =
            renew_runtime_foreground(RuntimeForegroundSource::VoiceFallbackInteraction, 2_500);

        assert_ne!(external.id, chat.id);
        assert_ne!(chat.id, voice.id);
        assert_ne!(voice.id, fallback.id);

        let snapshot = runtime_foreground_snapshot_at(3_000);
        assert!(snapshot.active);
        assert_eq!(snapshot.active_count, 4);
        assert!(!snapshot.recovery_active);
        assert_eq!(
            snapshot.primary_source,
            Some(RuntimeForegroundSource::VoiceFallbackInteraction)
        );
        assert_eq!(snapshot.resume_after_ms, Some(29_500));
    }

    #[test]
    fn same_source_renews_existing_active_ticket_without_record_growth() {
        let _guard = runtime_foreground_test_guard();
        reset_runtime_foreground_for_tests();

        let first = renew_runtime_foreground(RuntimeForegroundSource::ExternalUserMessage, 1_000);
        let renewed = renew_runtime_foreground(RuntimeForegroundSource::ExternalUserMessage, 2_000);

        assert_eq!(first, renewed);
        let snapshot = runtime_foreground_snapshot_at(2_500);
        assert_eq!(snapshot.records.len(), 1);
        assert_eq!(snapshot.active_count, 1);
        assert!(!snapshot.recovery_active);
        assert_eq!(snapshot.age_ms, Some(500));
        assert_eq!(snapshot.resume_after_ms, Some(29_500));
    }

    #[test]
    fn expired_foreground_records_are_bounded_for_long_running_esp_uptime() {
        let _guard = runtime_foreground_test_guard();
        reset_runtime_foreground_for_tests();

        for index in 0..(FOREGROUND_RECORD_LIMIT + 8) {
            renew_runtime_foreground(
                RuntimeForegroundSource::ExternalUserMessage,
                1_000 + (index as u64 * (RUNTIME_FOREGROUND_IDLE_MS + 1)),
            );
        }

        let snapshot = runtime_foreground_snapshot_at(
            1_000 + ((FOREGROUND_RECORD_LIMIT + 8) as u64 * (RUNTIME_FOREGROUND_IDLE_MS + 1)),
        );
        assert!(snapshot.records.len() <= FOREGROUND_RECORD_LIMIT);
    }

    #[test]
    fn finish_releases_only_the_matching_foreground_ticket() {
        let _guard = runtime_foreground_test_guard();
        reset_runtime_foreground_for_tests();

        let external =
            renew_runtime_foreground(RuntimeForegroundSource::ExternalUserMessage, 1_000);
        let chat = renew_runtime_foreground(RuntimeForegroundSource::ConfigUiChat, 1_500);

        assert!(finish_runtime_foreground(external));

        let snapshot = runtime_foreground_snapshot_at(2_000);
        assert!(snapshot.active);
        assert_eq!(snapshot.active_count, 1);
        assert_eq!(
            snapshot.primary_source,
            Some(RuntimeForegroundSource::ConfigUiChat)
        );

        assert!(finish_runtime_foreground_at(chat, 2_000));
        assert!(!runtime_foreground_active(2_100));
        let recovery = runtime_foreground_snapshot_at(2_100);
        assert!(recovery.recovery_active);
        assert_eq!(
            recovery.recovery_source,
            Some(RuntimeForegroundSource::ConfigUiChat)
        );
        assert_eq!(recovery.recovery_age_ms, Some(100));
        assert_eq!(recovery.recovery_resume_after_ms, Some(9_900));
    }

    #[test]
    fn expired_tickets_leave_a_bounded_recovery_overlay_without_staying_active() {
        let _guard = runtime_foreground_test_guard();
        reset_runtime_foreground_for_tests();

        renew_runtime_foreground(RuntimeForegroundSource::ExternalUserMessage, 1_000);

        assert!(runtime_foreground_active(30_999));
        assert!(!runtime_foreground_active(31_001));
        let recovery = runtime_foreground_snapshot_at(31_001);
        assert!(recovery.recovery_active);
        assert_eq!(
            recovery.recovery_source,
            Some(RuntimeForegroundSource::ExternalUserMessage)
        );
        assert_eq!(recovery.recovery_age_ms, Some(1));
        assert_eq!(recovery.recovery_resume_after_ms, Some(9_999));
        let settled = runtime_foreground_snapshot_at(41_001);
        assert!(!settled.active);
        assert!(!settled.recovery_active);
    }
}
