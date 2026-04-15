use super::*;
use std::collections::HashSet;
use std::time::{Duration, Instant};

fn workflow_kind_for_self_runtime_trigger(
    trigger: SelfRuntimeTrigger,
) -> crate::runtime::WorkflowKind {
    match trigger {
        SelfRuntimeTrigger::PostReply => crate::runtime::WorkflowKind::SelfRuntimePostReply,
        SelfRuntimeTrigger::IdleTick => crate::runtime::WorkflowKind::SelfRuntimeIdleTick,
        SelfRuntimeTrigger::OperatorRequested => crate::runtime::WorkflowKind::OperatorMaintenance,
    }
}

fn workflow_trigger_for_self_runtime_trigger(
    trigger: SelfRuntimeTrigger,
) -> crate::runtime::WorkflowTrigger {
    match trigger {
        SelfRuntimeTrigger::PostReply => crate::runtime::WorkflowTrigger::PostReply,
        SelfRuntimeTrigger::IdleTick => crate::runtime::WorkflowTrigger::CronTick,
        SelfRuntimeTrigger::OperatorRequested => crate::runtime::WorkflowTrigger::OperatorRequested,
    }
}

fn append_self_runtime_workflow_audit(
    trigger: SelfRuntimeTrigger,
    disposition: crate::runtime::WorkflowDisposition,
    rationale: &str,
    effect: crate::runtime::WorkflowEffect,
    chat_id: Option<&str>,
    source_channel: Option<&str>,
) {
    let channel = source_channel.and_then(|value| {
        let trimmed = value.trim();
        (!trimmed.is_empty()).then_some(trimmed)
    });
    crate::runtime::append_workflow_audit(
        crate::runtime::WorkflowAuditRecord::new(
            workflow_kind_for_self_runtime_trigger(trigger),
            workflow_trigger_for_self_runtime_trigger(trigger),
            disposition,
            effect,
            crate::runtime::WorkflowRecoveryPolicy::DropOnModeExit,
            rationale,
            crate::util::current_unix_secs(),
        )
        .with_target(None, channel, chat_id)
        .with_suppression_reason(
            matches!(disposition, crate::runtime::WorkflowDisposition::Suppress)
                .then_some(rationale),
        ),
    );
}

fn self_runtime_post_reply_no_trigger_reason(
    continuity: Option<&crate::memory::SelfContinuity>,
    strategy: Option<&crate::memory::AutonomyStrategy>,
    has_self_authored_core: bool,
    source_channel: &str,
    tool_calls: u32,
    external_content_used: bool,
    now_secs: u64,
    profile: MemoryProfile,
) -> Option<&'static str> {
    if tool_calls > 0 || external_content_used {
        return None;
    }
    if !has_self_authored_core || strategy.is_none() {
        return None;
    }
    let current_channel = source_channel.trim();
    let previous_channel = continuity
        .map(|state| state.last_user_channel.trim())
        .unwrap_or_default();
    if !current_channel.is_empty()
        && !previous_channel.is_empty()
        && current_channel != previous_channel
    {
        return None;
    }
    let Some(idle_interval_secs) = autonomy_idle_interval_secs(strategy, profile) else {
        return Some("post_reply_autonomy_disabled");
    };
    let last_autonomy_run_at = continuity
        .map(|state| state.last_autonomy_run_at)
        .unwrap_or(0);
    if last_autonomy_run_at == 0
        || now_secs.saturating_sub(last_autonomy_run_at) >= idle_interval_secs
    {
        return None;
    }
    Some("post_reply_runtime_recently_ran")
}

pub fn enqueue_self_runtime_post_reply(
    system_inbound_tx: &SystemInboundTx,
    self_continuity_store: &dyn SelfContinuityStore,
    autonomy_strategy_store: &dyn AutonomyStrategyStore,
    self_authored_core_store: &dyn SelfAuthoredCoreStore,
    profile: MemoryProfile,
    chat_id: &str,
    source_channel: &str,
    user_content: &str,
    reply_content: &str,
    tool_calls: u32,
    external_content_used: bool,
) -> bool {
    let now_secs = current_unix_secs();
    let subject_id = board_subject_scope_id();
    let continuity = self_continuity_store.get(subject_id).ok().flatten();
    let strategy = autonomy_strategy_store.get(subject_id).ok().flatten();
    let has_self_authored_core = self_authored_core_store
        .get(subject_id)
        .ok()
        .flatten()
        .is_some();
    if let Some(reason) = self_runtime_post_reply_no_trigger_reason(
        continuity.as_ref(),
        strategy.as_ref(),
        has_self_authored_core,
        source_channel,
        tool_calls,
        external_content_used,
        now_secs,
        profile,
    ) {
        append_self_runtime_workflow_audit(
            SelfRuntimeTrigger::PostReply,
            crate::runtime::WorkflowDisposition::NoTrigger,
            reason,
            crate::runtime::WorkflowEffect::Noop,
            Some(chat_id),
            Some(source_channel),
        );
        return false;
    }
    schedule_self_runtime_job(
        system_inbound_tx,
        chat_id,
        SelfRuntimeJobPayload {
            trigger: SelfRuntimeTrigger::PostReply,
            source_channel: source_channel.to_string(),
            user_content: truncate_content_to_max(user_content, 512).into_owned(),
            reply_content: truncate_content_to_max(reply_content, 768).into_owned(),
            tool_calls,
            external_content_used,
            now_secs,
        },
        SELF_RUNTIME_POST_REPLY_DELAY_MS,
    )
}

#[cfg_attr(not(test), allow(dead_code))]
pub(super) fn should_enqueue_self_runtime_post_reply_with_state(
    continuity: Option<&crate::memory::SelfContinuity>,
    strategy: Option<&crate::memory::AutonomyStrategy>,
    has_self_authored_core: bool,
    source_channel: &str,
    tool_calls: u32,
    external_content_used: bool,
    now_secs: u64,
    profile: MemoryProfile,
) -> bool {
    self_runtime_post_reply_no_trigger_reason(
        continuity,
        strategy,
        has_self_authored_core,
        source_channel,
        tool_calls,
        external_content_used,
        now_secs,
        profile,
    )
    .is_none()
}

pub fn enqueue_self_runtime_idle_tick(system_inbound_tx: &SystemInboundTx, chat_id: &str) -> bool {
    enqueue_self_runtime_idle_tick_for_relation(system_inbound_tx, chat_id, "self_runtime_idle")
}

pub fn enqueue_self_runtime_operator_request(
    system_inbound_tx: &SystemInboundTx,
    chat_id: &str,
    source_channel: &str,
) -> bool {
    enqueue_self_runtime_job_now(
        system_inbound_tx,
        chat_id,
        SelfRuntimeJobPayload {
            trigger: SelfRuntimeTrigger::OperatorRequested,
            source_channel: source_channel.trim().to_string(),
            user_content: String::new(),
            reply_content: String::new(),
            tool_calls: 0,
            external_content_used: false,
            now_secs: current_unix_secs(),
        },
    )
}

fn enqueue_self_runtime_idle_tick_for_relation(
    system_inbound_tx: &SystemInboundTx,
    chat_id: &str,
    source_channel: &str,
) -> bool {
    schedule_self_runtime_job(
        system_inbound_tx,
        chat_id,
        SelfRuntimeJobPayload {
            trigger: SelfRuntimeTrigger::IdleTick,
            source_channel: source_channel.to_string(),
            user_content: String::new(),
            reply_content: String::new(),
            tool_calls: 0,
            external_content_used: false,
            now_secs: current_unix_secs(),
        },
        SELF_RUNTIME_IDLE_TICK_DELAY_MS,
    )
}

fn schedule_self_runtime_job(
    system_inbound_tx: &SystemInboundTx,
    chat_id: &str,
    payload: SelfRuntimeJobPayload,
    delay_ms: u64,
) -> bool {
    let system_inbound_tx = system_inbound_tx.clone();
    let chat_id = chat_id.to_string();
    let audit_chat_id = chat_id.clone();
    let audit_channel = payload.source_channel.clone();
    let trigger = payload.trigger;
    let delayed_chat_id = chat_id.clone();
    let scheduled = crate::runtime::schedule_delayed_task(
        Instant::now() + Duration::from_millis(delay_ms),
        Box::new(move || {
            if let Some(reason) = self_runtime_enqueue_block_reason(payload.trigger) {
                log::debug!(
                    "[self_runtime] skip delayed enqueue because {} chat_id={}",
                    reason,
                    delayed_chat_id
                );
                append_self_runtime_workflow_audit(
                    payload.trigger,
                    crate::runtime::WorkflowDisposition::Suppress,
                    reason,
                    crate::runtime::WorkflowEffect::Noop,
                    Some(delayed_chat_id.as_str()),
                    Some(payload.source_channel.as_str()),
                );
                return;
            }
            let _ = enqueue_self_runtime_job_now(&system_inbound_tx, &delayed_chat_id, payload);
        }),
    );
    if !scheduled {
        log::debug!(
            "[self_runtime] delayed queue full, skip schedule chat_id={}",
            chat_id
        );
        append_self_runtime_workflow_audit(
            trigger,
            crate::runtime::WorkflowDisposition::ExecuteFailed,
            "self_runtime_schedule_failed",
            crate::runtime::WorkflowEffect::Noop,
            Some(audit_chat_id.as_str()),
            Some(audit_channel.as_str()),
        );
    } else {
        append_self_runtime_workflow_audit(
            trigger,
            crate::runtime::WorkflowDisposition::DeferUntil,
            "self_runtime_scheduled",
            crate::runtime::WorkflowEffect::EnqueueSystemJob,
            Some(audit_chat_id.as_str()),
            Some(audit_channel.as_str()),
        );
    }
    scheduled
}

fn enqueue_self_runtime_job_now(
    system_inbound_tx: &SystemInboundTx,
    chat_id: &str,
    payload: SelfRuntimeJobPayload,
) -> bool {
    let body = match serde_json::to_string(&payload) {
        Ok(body) => body,
        Err(error) => {
            log::warn!(
                "[self_runtime] serialize job failed chat_id={}: {}",
                chat_id,
                error
            );
            append_self_runtime_workflow_audit(
                payload.trigger,
                crate::runtime::WorkflowDisposition::ExecuteFailed,
                "self_runtime_serialize_failed",
                crate::runtime::WorkflowEffect::Noop,
                Some(chat_id),
                Some(payload.source_channel.as_str()),
            );
            return false;
        }
    };
    let job = match PcMsg::new_system(SELF_RUNTIME_CHANNEL, chat_id, body) {
        Ok(job) => job,
        Err(error) => {
            log::warn!(
                "[self_runtime] build job failed chat_id={}: {}",
                chat_id,
                error
            );
            append_self_runtime_workflow_audit(
                payload.trigger,
                crate::runtime::WorkflowDisposition::ExecuteFailed,
                "self_runtime_build_failed",
                crate::runtime::WorkflowEffect::Noop,
                Some(chat_id),
                Some(payload.source_channel.as_str()),
            );
            return false;
        }
    };
    match system_inbound_tx.try_send(job) {
        Ok(()) => {
            append_self_runtime_workflow_audit(
                payload.trigger,
                crate::runtime::WorkflowDisposition::ExecuteNow,
                "self_runtime_enqueued",
                crate::runtime::WorkflowEffect::EnqueueSystemJob,
                Some(chat_id),
                Some(payload.source_channel.as_str()),
            );
            true
        }
        Err(std::sync::mpsc::TrySendError::Full(_)) => {
            log::debug!(
                "[self_runtime] skip enqueue because system queue is full chat_id={}",
                chat_id
            );
            append_self_runtime_workflow_audit(
                payload.trigger,
                crate::runtime::WorkflowDisposition::ExecuteFailed,
                "self_runtime_queue_full",
                crate::runtime::WorkflowEffect::Noop,
                Some(chat_id),
                Some(payload.source_channel.as_str()),
            );
            false
        }
        Err(std::sync::mpsc::TrySendError::Disconnected(_)) => {
            log::warn!("[self_runtime] enqueue failed: system queue disconnected");
            append_self_runtime_workflow_audit(
                payload.trigger,
                crate::runtime::WorkflowDisposition::ExecuteFailed,
                "self_runtime_queue_disconnected",
                crate::runtime::WorkflowEffect::Noop,
                Some(chat_id),
                Some(payload.source_channel.as_str()),
            );
            false
        }
    }
}

fn self_runtime_enqueue_block_reason(trigger: SelfRuntimeTrigger) -> Option<&'static str> {
    let runtime_mode = crate::runtime::thread_registry::runtime_mode_snapshot();
    if !runtime_mode.action_budget.allow_idle_self_runtime {
        return Some(
            runtime_mode
                .mode_block_reason()
                .unwrap_or("runtime_mode_blocked"),
        );
    }
    if let Some(reason) = idle_self_runtime_scheduler_block_reason() {
        return Some(reason);
    }
    if matches!(trigger, SelfRuntimeTrigger::IdleTick) {
        return idle_self_runtime_block_reason();
    }
    None
}

pub fn self_runtime_tick(
    system_inbound_tx: &SystemInboundTx,
    session_store: &dyn SessionStore,
    self_continuity_store: &dyn SelfContinuityStore,
    autonomy_strategy_store: &dyn AutonomyStrategyStore,
    self_authored_core_store: &dyn SelfAuthoredCoreStore,
    relationship_portfolio_store: &dyn RelationshipPortfolioStore,
    relationship_topology_store: &dyn RelationshipTopologyStore,
    profile: MemoryProfile,
    now_secs: u64,
) {
    if let Some(reason) = idle_self_runtime_block_reason() {
        log::debug!("[self_runtime] skip idle tick enqueue because {}", reason);
        append_self_runtime_workflow_audit(
            SelfRuntimeTrigger::IdleTick,
            crate::runtime::WorkflowDisposition::Suppress,
            reason,
            crate::runtime::WorkflowEffect::Noop,
            None,
            None,
        );
        return;
    }

    let policy = memory_policy(profile).self_runtime;
    let capability = memory_capability_profile(profile);
    let uptime_secs = crate::platform::time::uptime_secs();
    let mut enqueued = 0usize;
    let subject_id = board_subject_scope_id();
    let continuity = match self_continuity_store.get(subject_id) {
        Ok(value) => value,
        Err(error) => {
            log::warn!(
                "[self_runtime] failed to read subject continuity: {}",
                error
            );
            append_self_runtime_workflow_audit(
                SelfRuntimeTrigger::IdleTick,
                crate::runtime::WorkflowDisposition::ExecuteFailed,
                "self_runtime_continuity_read_failed",
                crate::runtime::WorkflowEffect::Noop,
                None,
                None,
            );
            return;
        }
    };
    let strategy = match autonomy_strategy_store.get(subject_id) {
        Ok(value) => value,
        Err(error) => {
            log::warn!(
                "[self_runtime] failed to read subject autonomy strategy: {}",
                error
            );
            None
        }
    };
    let last_user_turn_at = continuity
        .as_ref()
        .map(|c| c.last_user_turn_at)
        .unwrap_or(0);
    if last_user_turn_at > 0
        && now_secs.saturating_sub(last_user_turn_at) > policy.active_chat_window_secs
    {
        append_self_runtime_workflow_audit(
            SelfRuntimeTrigger::IdleTick,
            crate::runtime::WorkflowDisposition::NoTrigger,
            "idle_active_chat_window_expired",
            crate::runtime::WorkflowEffect::Noop,
            None,
            None,
        );
        return;
    }
    let last_autonomy = continuity
        .as_ref()
        .map(|c| c.last_autonomy_run_at)
        .unwrap_or(0);
    let preferred_chat_id = continuity
        .as_ref()
        .map(|c| c.last_user_chat_id.trim())
        .filter(|value| !value.is_empty());
    let preferred_channel = continuity
        .as_ref()
        .map(|c| c.last_user_channel.trim())
        .filter(|value| !value.is_empty());
    let idle_interval_secs = match autonomy_idle_interval_secs(strategy.as_ref(), profile) {
        Some(interval) => interval,
        None if strategy.is_some() => {
            append_self_runtime_workflow_audit(
                SelfRuntimeTrigger::IdleTick,
                crate::runtime::WorkflowDisposition::NoTrigger,
                "idle_autonomy_disabled",
                crate::runtime::WorkflowEffect::Noop,
                None,
                None,
            );
            return;
        }
        None => policy.idle_tick_interval_secs,
    };
    if !idle_self_runtime_due(
        now_secs,
        uptime_secs,
        last_user_turn_at,
        last_autonomy,
        idle_interval_secs,
    ) {
        append_self_runtime_workflow_audit(
            SelfRuntimeTrigger::IdleTick,
            crate::runtime::WorkflowDisposition::NoTrigger,
            "idle_self_runtime_not_due",
            crate::runtime::WorkflowEffect::Noop,
            None,
            None,
        );
        return;
    }

    let max_jobs_per_tick = policy
        .max_jobs_per_tick
        .min(capability.runtime_max_jobs_per_tick);
    let topology = match relationship_topology_store.get(subject_id) {
        Ok(value) => value,
        Err(error) => {
            log::warn!(
                "[self_runtime] failed to read relationship topology: {}",
                error
            );
            None
        }
    };
    let self_authored_core = match self_authored_core_store.get(subject_id) {
        Ok(value) => value,
        Err(error) => {
            log::warn!(
                "[self_runtime] failed to read self-authored core for portfolio sync: {}",
                error
            );
            None
        }
    };
    let portfolio = match sync_relationship_portfolio(
        relationship_portfolio_store,
        topology.as_ref(),
        self_authored_core.as_ref(),
        now_secs,
    ) {
        Ok(value) => value,
        Err(error) => {
            log::warn!(
                "[self_runtime] failed to sync relationship portfolio: {}",
                error
            );
            relationship_portfolio_store.get(subject_id).ok().flatten()
        }
    };
    let mut scheduled_chat_ids = HashSet::with_capacity(max_jobs_per_tick);
    if let Some(portfolio) = portfolio.as_ref() {
        let targets = select_relationship_portfolio_targets(
            Some(portfolio),
            RelationshipPortfolioSelectorInput {
                preferred_chat_id,
                preferred_channel,
                now_secs,
                max_targets: max_jobs_per_tick,
            },
        );
        for target in targets {
            if enqueued >= max_jobs_per_tick {
                break;
            }
            if !scheduled_chat_ids.insert(target.chat_id.clone()) {
                continue;
            }
            let _ = touch_relationship_portfolio_selection(
                relationship_portfolio_store,
                target.scope_id.as_str(),
                now_secs,
            );
            if enqueue_self_runtime_idle_tick_for_relation(
                system_inbound_tx,
                &target.chat_id,
                &target.channel,
            ) {
                enqueued += 1;
            }
        }
    }

    if enqueued >= max_jobs_per_tick {
        return;
    }

    let chat_ids = match session_store.list_chat_ids() {
        Ok(chat_ids) => chat_ids,
        Err(error) => {
            log::warn!("[self_runtime] failed to list chat ids: {}", error);
            append_self_runtime_workflow_audit(
                SelfRuntimeTrigger::IdleTick,
                crate::runtime::WorkflowDisposition::ExecuteFailed,
                "self_runtime_session_list_failed",
                crate::runtime::WorkflowEffect::Noop,
                None,
                None,
            );
            return;
        }
    };

    for chat_id in chat_ids {
        if enqueued >= max_jobs_per_tick {
            break;
        }
        if preferred_chat_id.is_some_and(|preferred| preferred != chat_id) {
            continue;
        }
        if !scheduled_chat_ids.insert(chat_id.clone()) {
            continue;
        }
        let fallback_channel = if preferred_chat_id == Some(chat_id.as_str()) {
            preferred_channel.unwrap_or("self_runtime_idle")
        } else {
            "self_runtime_idle"
        };
        if enqueue_self_runtime_idle_tick_for_relation(
            system_inbound_tx,
            &chat_id,
            fallback_channel,
        ) {
            enqueued += 1;
        }
    }

    if enqueued == 0 {
        append_self_runtime_workflow_audit(
            SelfRuntimeTrigger::IdleTick,
            crate::runtime::WorkflowDisposition::NoTrigger,
            "no_idle_self_runtime_targets",
            crate::runtime::WorkflowEffect::Noop,
            None,
            None,
        );
    }
}

#[cfg_attr(not(test), allow(dead_code))]
pub(super) fn idle_self_runtime_due(
    now_secs: u64,
    uptime_secs: u64,
    last_user_turn_at: u64,
    last_autonomy_run_at: u64,
    idle_interval_secs: u64,
) -> bool {
    if last_autonomy_run_at > 0 {
        return now_secs.saturating_sub(last_autonomy_run_at) >= idle_interval_secs;
    }

    // First idle runtime after boot should still respect the strategy cadence instead of
    // firing immediately on the first 60s cron tick.
    if last_user_turn_at > 0 && now_secs.saturating_sub(last_user_turn_at) < idle_interval_secs {
        return false;
    }

    uptime_secs >= idle_interval_secs
}

fn idle_self_runtime_scheduler_block_reason() -> Option<&'static str> {
    let snap = crate::orchestrator::snapshot();
    if snap.active_agent_tasks > 0 {
        Some("agent_plane_busy")
    } else if cfg!(any(target_arch = "xtensa", target_arch = "riscv32"))
        && snap.active_wss_count > 0
    {
        Some("external_wss_active")
    } else if snap.inbound_depth > 0 || snap.outbound_depth > 0 {
        Some("message_queues_busy")
    } else {
        None
    }
}

pub(super) fn idle_memory_hygiene_budget_allows_run() -> bool {
    let snap = crate::orchestrator::snapshot();
    let wss_budget_available =
        !cfg!(any(target_arch = "xtensa", target_arch = "riscv32")) || snap.active_wss_count == 0;
    wss_budget_available
        && snap.active_agent_tasks == 0
        && snap.inbound_depth == 0
        && snap.outbound_depth == 0
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
fn idle_self_runtime_block_reason() -> Option<&'static str> {
    let runtime_mode = crate::runtime::thread_registry::runtime_mode_snapshot();
    if !runtime_mode.action_budget.allow_idle_self_runtime {
        return Some(
            runtime_mode
                .mode_block_reason()
                .unwrap_or("runtime_mode_blocked"),
        );
    }
    if let Some(reason) = idle_self_runtime_scheduler_block_reason() {
        return Some(reason);
    }

    let pressure = crate::orchestrator::refresh_heap_if_stale();
    let snap = crate::orchestrator::snapshot();
    let min_internal = if snap.heap_free_spiram > 0 {
        TLS_ADMISSION_MIN_INTERNAL_BYTES as u32
    } else {
        TLS_ADMISSION_NO_PSRAM_MIN_BYTES as u32
    };
    let fragmented = snap.heap_free_spiram > 0
        && snap.heap_largest_block_internal < TLS_ADMISSION_MIN_LARGEST_BLOCK_BYTES as u32;
    if pressure != PressureLevel::Normal {
        Some("resource_pressure")
    } else if snap.storage_contention_risk != crate::orchestrator::StorageContentionRisk::Healthy {
        Some("storage_contention")
    } else if fragmented {
        Some("internal_heap_fragmented")
    } else if snap.heap_free_internal < min_internal {
        Some("insufficient_internal_heap")
    } else {
        None
    }
}

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
fn idle_self_runtime_block_reason() -> Option<&'static str> {
    let runtime_mode = crate::runtime::thread_registry::runtime_mode_snapshot();
    if !runtime_mode.action_budget.allow_idle_self_runtime {
        return Some(
            runtime_mode
                .mode_block_reason()
                .unwrap_or("runtime_mode_blocked"),
        );
    }
    idle_self_runtime_scheduler_block_reason()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bus::new_inbound_channel;
    use crate::error::Result;
    use crate::memory::{
        AutonomyStrategy, AutonomyStrategyStore, MemoryProfile, RelationshipPortfolio,
        RelationshipPortfolioStore, RelationshipTopology, RelationshipTopologyStore,
        SelfAuthoredCore, SelfAuthoredCoreStore, SelfContinuity, SelfContinuityStore,
        SessionMessage, SessionStore,
    };
    use crate::runtime::workflow::{reset_workflow_audit_for_tests, workflow_audit_snapshot};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Mutex, OnceLock};

    fn test_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn delayed_task_runtime_guard() -> std::sync::MutexGuard<'static, ()> {
        let guard = crate::runtime::delayed_task::delayed_task_test_guard();
        crate::runtime::delayed_task::reset_delayed_tasks_for_tests();
        guard
    }

    #[derive(Default)]
    struct CountingSessionStore {
        list_calls: AtomicUsize,
    }

    impl SessionStore for CountingSessionStore {
        fn append(&self, _chat_id: &str, _role: &str, _content: &str) -> Result<()> {
            Ok(())
        }

        fn load_recent(&self, _chat_id: &str, _n: usize) -> Result<Vec<SessionMessage>> {
            Ok(Vec::new())
        }

        fn clear(&self, _chat_id: &str) -> Result<()> {
            Ok(())
        }

        fn list_chat_ids(&self) -> Result<Vec<String>> {
            self.list_calls.fetch_add(1, Ordering::SeqCst);
            Ok(vec!["chat-a".to_string()])
        }
    }

    #[derive(Default)]
    struct StubSelfContinuityStore {
        value: Mutex<Option<SelfContinuity>>,
    }

    impl StubSelfContinuityStore {
        fn with_value(value: SelfContinuity) -> Self {
            Self {
                value: Mutex::new(Some(value)),
            }
        }
    }

    impl SelfContinuityStore for StubSelfContinuityStore {
        fn get(&self, _chat_id: &str) -> Result<Option<SelfContinuity>> {
            Ok(self.value.lock().unwrap_or_else(|e| e.into_inner()).clone())
        }

        fn set(&self, _chat_id: &str, continuity: &SelfContinuity) -> Result<()> {
            *self.value.lock().unwrap_or_else(|e| e.into_inner()) = Some(continuity.clone());
            Ok(())
        }

        fn clear(&self, _chat_id: &str) -> Result<()> {
            *self.value.lock().unwrap_or_else(|e| e.into_inner()) = None;
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubAutonomyStrategyStore {
        value: Mutex<Option<AutonomyStrategy>>,
    }

    impl StubAutonomyStrategyStore {
        fn with_value(value: AutonomyStrategy) -> Self {
            Self {
                value: Mutex::new(Some(value)),
            }
        }
    }

    impl AutonomyStrategyStore for StubAutonomyStrategyStore {
        fn get(&self, _chat_id: &str) -> Result<Option<AutonomyStrategy>> {
            Ok(self.value.lock().unwrap_or_else(|e| e.into_inner()).clone())
        }

        fn set(&self, _chat_id: &str, strategy: &AutonomyStrategy) -> Result<()> {
            *self.value.lock().unwrap_or_else(|e| e.into_inner()) = Some(strategy.clone());
            Ok(())
        }

        fn clear(&self, _chat_id: &str) -> Result<()> {
            *self.value.lock().unwrap_or_else(|e| e.into_inner()) = None;
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubSelfAuthoredCoreStore;

    impl SelfAuthoredCoreStore for StubSelfAuthoredCoreStore {
        fn get(&self, _scope_id: &str) -> Result<Option<SelfAuthoredCore>> {
            Ok(None)
        }

        fn set(&self, _scope_id: &str, _core: &SelfAuthoredCore) -> Result<()> {
            Ok(())
        }

        fn clear(&self, _scope_id: &str) -> Result<()> {
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubRelationshipPortfolioStore;

    impl RelationshipPortfolioStore for StubRelationshipPortfolioStore {
        fn get(&self, _scope_id: &str) -> Result<Option<RelationshipPortfolio>> {
            Ok(None)
        }

        fn set(&self, _scope_id: &str, _portfolio: &RelationshipPortfolio) -> Result<()> {
            Ok(())
        }

        fn clear(&self, _scope_id: &str) -> Result<()> {
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubRelationshipTopologyStore;

    impl RelationshipTopologyStore for StubRelationshipTopologyStore {
        fn get(&self, _scope_id: &str) -> Result<Option<RelationshipTopology>> {
            Ok(None)
        }

        fn set(&self, _scope_id: &str, _topology: &RelationshipTopology) -> Result<()> {
            Ok(())
        }

        fn clear(&self, _scope_id: &str) -> Result<()> {
            Ok(())
        }
    }

    #[test]
    fn self_runtime_tick_skips_session_enumeration_when_idle_runtime_is_not_due() {
        let _guard = test_lock().lock().unwrap_or_else(|e| e.into_inner());
        let _delayed_task_guard = delayed_task_runtime_guard();
        let _audit_guard = crate::runtime::workflow_audit_test_guard();
        reset_workflow_audit_for_tests();
        let (system_inbound_tx, _system_inbound_rx, _depth) = new_inbound_channel(4);
        let session_store = CountingSessionStore::default();
        let now_secs = 10_000;
        let continuity = SelfContinuity {
            last_user_turn_at: now_secs - 30,
            last_user_chat_id: "chat-a".to_string(),
            last_user_channel: "qq_channel".to_string(),
            last_autonomy_run_at: now_secs - 10,
            updated_at: now_secs - 10,
            ..SelfContinuity::default()
        };
        let strategy = AutonomyStrategy {
            idle_enabled: true,
            idle_interval_secs: 300,
            updated_at: now_secs - 10,
            ..AutonomyStrategy::default()
        };
        let self_continuity_store = StubSelfContinuityStore::with_value(continuity);
        let autonomy_strategy_store = StubAutonomyStrategyStore::with_value(strategy);

        self_runtime_tick(
            &system_inbound_tx,
            &session_store,
            &self_continuity_store,
            &autonomy_strategy_store,
            &StubSelfAuthoredCoreStore,
            &StubRelationshipPortfolioStore,
            &StubRelationshipTopologyStore,
            MemoryProfile::Standard,
            now_secs,
        );

        assert_eq!(
            session_store.list_calls.load(Ordering::SeqCst),
            0,
            "session enumeration should stay behind idle due gates"
        );
        let audit = workflow_audit_snapshot(4);
        assert_eq!(audit.summary.no_trigger, 1);
        assert_eq!(
            audit.recent_records[0].workflow,
            crate::runtime::WorkflowKind::SelfRuntimeIdleTick
        );
    }

    #[test]
    fn self_runtime_tick_skips_session_enumeration_when_voice_exclusive_is_active() {
        let _guard = test_lock().lock().unwrap_or_else(|e| e.into_inner());
        let _delayed_task_guard = delayed_task_runtime_guard();
        let _audit_guard = crate::runtime::workflow_audit_test_guard();
        reset_workflow_audit_for_tests();
        crate::state::set_voice_exclusive_active(true);

        let (system_inbound_tx, _system_inbound_rx, _depth) = new_inbound_channel(4);
        let session_store = CountingSessionStore::default();
        let now_secs = 10_000;
        let continuity = SelfContinuity {
            last_user_turn_at: now_secs - 600,
            last_user_chat_id: "chat-a".to_string(),
            last_user_channel: "qq_channel".to_string(),
            last_autonomy_run_at: now_secs - 600,
            updated_at: now_secs - 10,
            ..SelfContinuity::default()
        };
        let strategy = AutonomyStrategy {
            idle_enabled: true,
            idle_interval_secs: 300,
            updated_at: now_secs - 10,
            ..AutonomyStrategy::default()
        };
        let self_continuity_store = StubSelfContinuityStore::with_value(continuity);
        let autonomy_strategy_store = StubAutonomyStrategyStore::with_value(strategy);

        self_runtime_tick(
            &system_inbound_tx,
            &session_store,
            &self_continuity_store,
            &autonomy_strategy_store,
            &StubSelfAuthoredCoreStore,
            &StubRelationshipPortfolioStore,
            &StubRelationshipTopologyStore,
            MemoryProfile::Standard,
            now_secs,
        );

        crate::state::set_voice_exclusive_active(false);

        assert_eq!(
            session_store.list_calls.load(Ordering::SeqCst),
            0,
            "voice-exclusive mode should block session enumeration before idle fan-out"
        );
        let audit = workflow_audit_snapshot(4);
        assert_eq!(audit.summary.suppressed, 1);
        assert_eq!(
            audit.recent_records[0].workflow,
            crate::runtime::WorkflowKind::SelfRuntimeIdleTick
        );
    }

    #[test]
    fn enqueue_self_runtime_post_reply_records_deferred_workflow_audit() {
        let _guard = test_lock().lock().unwrap_or_else(|e| e.into_inner());
        let _delayed_task_guard = delayed_task_runtime_guard();
        let _audit_guard = crate::runtime::workflow_audit_test_guard();
        reset_workflow_audit_for_tests();
        let (system_inbound_tx, _system_inbound_rx, _depth) = new_inbound_channel(4);

        let scheduled = enqueue_self_runtime_post_reply(
            &system_inbound_tx,
            &StubSelfContinuityStore::default(),
            &StubAutonomyStrategyStore::default(),
            &StubSelfAuthoredCoreStore,
            MemoryProfile::Standard,
            "chat-a",
            "qq_channel",
            "user",
            "reply",
            0,
            false,
        );

        assert!(scheduled);
        let audit = workflow_audit_snapshot(4);
        assert_eq!(audit.summary.deferred, 1);
        assert_eq!(
            audit.recent_records[0].workflow,
            crate::runtime::WorkflowKind::SelfRuntimePostReply
        );
    }

    #[test]
    fn enqueue_self_runtime_job_now_records_execute_now_workflow_audit() {
        let _guard = test_lock().lock().unwrap_or_else(|e| e.into_inner());
        let _delayed_task_guard = delayed_task_runtime_guard();
        let _audit_guard = crate::runtime::workflow_audit_test_guard();
        reset_workflow_audit_for_tests();
        let (system_inbound_tx, system_inbound_rx, _depth) = new_inbound_channel(4);

        let enqueued = enqueue_self_runtime_job_now(
            &system_inbound_tx,
            "chat-a",
            SelfRuntimeJobPayload {
                trigger: SelfRuntimeTrigger::IdleTick,
                source_channel: "qq_channel".to_string(),
                user_content: String::new(),
                reply_content: String::new(),
                tool_calls: 0,
                external_content_used: false,
                now_secs: 42,
            },
        );

        assert!(enqueued);
        let job = system_inbound_rx
            .try_recv()
            .expect("self runtime job should enqueue");
        assert_eq!(job.chat_id.as_ref(), "chat-a");
        let audit = workflow_audit_snapshot(4);
        assert_eq!(audit.summary.executed, 1);
        assert_eq!(
            audit.recent_records[0].workflow,
            crate::runtime::WorkflowKind::SelfRuntimeIdleTick
        );
    }
}
