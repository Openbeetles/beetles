//! Initiative runtime contract for bounded proactive behavior.
//! 边界内主动性运行时合同：统一感知、裁决、冷却与可观测口径。

use crate::bus::{IngressKind, PcMsg, SystemInboundTx};
use crate::i18n::{locale_from_store, tr, Message};
use crate::memory::{
    board_subject_scope_id, build_world_snapshot, select_relationship_portfolio_targets,
    touch_relationship_portfolio_selection, RelationshipPortfolioSelectorInput, SelfContinuity,
    WorldSnapshot, WorldSnapshotContext,
};
use crate::orchestrator::{self, PressureLevel, ResourceSnapshot};
use crate::platform::Platform;
use crate::runtime::{inspect_platform_presence, PresenceState, RuntimeModeSnapshot};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

const INITIATIVE_REMINDER_LOOKAHEAD_SECS: u64 = 15 * 60;
const INITIATIVE_REMINDER_ACTIVE_WINDOW_SECS: u64 = 20 * 60;
const INITIATIVE_RESUME_IDLE_MIN_SECS: u64 = 45 * 60;
const INITIATIVE_RESUME_IDLE_MAX_SECS: u64 = 6 * 3600;
const INITIATIVE_ACTIVE_RELATION_MAX_IDLE_SECS: u64 = 12 * 3600;
const INITIATIVE_REMINDER_COOLDOWN_SECS: u64 = 30 * 60;
const INITIATIVE_RESUME_COOLDOWN_SECS: u64 = 4 * 3600;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InitiativeAction {
    Hold,
    UpcomingReminderNudge,
    ResumeTaskCheckIn,
}

impl InitiativeAction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Hold => "hold",
            Self::UpcomingReminderNudge => "upcoming_reminder_nudge",
            Self::ResumeTaskCheckIn => "resume_task_check_in",
        }
    }

    fn cooldown_secs(self) -> u64 {
        match self {
            Self::Hold => 0,
            Self::UpcomingReminderNudge => INITIATIVE_REMINDER_COOLDOWN_SECS,
            Self::ResumeTaskCheckIn => INITIATIVE_RESUME_COOLDOWN_SECS,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InitiativeSuppressionReason {
    NoTargetRelation,
    StaleActiveRelation,
    RuntimeModeBlocked,
    PresenceNotIdle,
    ResourcePressure,
    QueuesBusy,
    AutonomyDisabled,
    ExactDueSignalPending,
    CooldownActive,
    NoUsefulTrigger,
}

impl InitiativeSuppressionReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NoTargetRelation => "no_target_relation",
            Self::StaleActiveRelation => "stale_active_relation",
            Self::RuntimeModeBlocked => "runtime_mode_blocked",
            Self::PresenceNotIdle => "presence_not_idle",
            Self::ResourcePressure => "resource_pressure",
            Self::QueuesBusy => "queues_busy",
            Self::AutonomyDisabled => "autonomy_disabled",
            Self::ExactDueSignalPending => "exact_due_signal_pending",
            Self::CooldownActive => "cooldown_active",
            Self::NoUsefulTrigger => "no_useful_trigger",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct InitiativeTarget {
    pub scope_id: String,
    pub channel: String,
    pub chat_id: String,
    pub selection_reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct InitiativeSignalSnapshot {
    pub user_idle_secs: u64,
    pub autonomy_idle_secs: u64,
    pub in_progress_tasks: usize,
    pub due_tasks: usize,
    pub high_priority_tasks: usize,
    pub upcoming_reminders: usize,
    pub next_reminder_at: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct InitiativeSnapshot {
    pub action: InitiativeAction,
    pub ready: bool,
    pub presence_state: PresenceState,
    pub runtime_mode: RuntimeModeSnapshot,
    pub rationale: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suppression_reason: Option<InitiativeSuppressionReason>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<InitiativeTarget>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signal: Option<InitiativeSignalSnapshot>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub strategy_mode: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub strategy_focus: String,
    pub idle_enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_preview: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_triggered_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_allowed_at: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct InitiativeDecision {
    action: InitiativeAction,
    rationale: &'static str,
    suppression_reason: Option<InitiativeSuppressionReason>,
    last_triggered_at: Option<u64>,
    next_allowed_at: Option<u64>,
}

fn initiative_runtime_state() -> &'static Mutex<HashMap<String, u64>> {
    static STATE: OnceLock<Mutex<HashMap<String, u64>>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn cooldown_key(scope_id: &str, action: InitiativeAction) -> String {
    format!("{}:{}", scope_id.trim(), action.as_str())
}

fn last_triggered_at(scope_id: &str, action: InitiativeAction) -> Option<u64> {
    initiative_runtime_state()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&cooldown_key(scope_id, action))
        .copied()
}

fn record_trigger(scope_id: &str, action: InitiativeAction, now_secs: u64) {
    initiative_runtime_state()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(cooldown_key(scope_id, action), now_secs);
}

fn minutes_ceil(secs: u64) -> u64 {
    if secs == 0 {
        0
    } else {
        secs.saturating_add(59) / 60
    }
}

fn preferred_relation(continuity: Option<&SelfContinuity>) -> Option<(String, String)> {
    let continuity = continuity?;
    let channel = continuity.last_user_channel.trim();
    let chat_id = continuity.last_user_chat_id.trim();
    if channel.is_empty() || chat_id.is_empty() {
        None
    } else {
        Some((channel.to_string(), chat_id.to_string()))
    }
}

fn select_target(
    continuity: Option<&SelfContinuity>,
    now_secs: u64,
    platform: &dyn Platform,
) -> Option<InitiativeTarget> {
    let (preferred_channel, preferred_chat_id) = preferred_relation(continuity)?;
    let subject_id = board_subject_scope_id();
    let portfolio = platform
        .relationship_portfolio_store()
        .get(subject_id)
        .ok()
        .flatten();
    if let Some(target) = select_relationship_portfolio_targets(
        portfolio.as_ref(),
        RelationshipPortfolioSelectorInput {
            preferred_chat_id: Some(preferred_chat_id.as_str()),
            preferred_channel: Some(preferred_channel.as_str()),
            now_secs,
            max_targets: 1,
        },
    )
    .into_iter()
    .next()
    {
        if target.channel == preferred_channel && target.chat_id == preferred_chat_id {
            return Some(InitiativeTarget {
                scope_id: target.scope_id,
                channel: target.channel,
                chat_id: target.chat_id,
                selection_reason: target.reason,
            });
        }
    }
    Some(InitiativeTarget {
        scope_id: crate::memory::relationship_scope_id(&preferred_channel, &preferred_chat_id),
        channel: preferred_channel,
        chat_id: preferred_chat_id,
        selection_reason: "preferred_relation_fallback".to_string(),
    })
}

fn build_signal(world: &WorldSnapshot) -> InitiativeSignalSnapshot {
    InitiativeSignalSnapshot {
        user_idle_secs: world.user_idle_secs,
        autonomy_idle_secs: world.autonomy_idle_secs,
        in_progress_tasks: world.in_progress_tasks,
        due_tasks: world.due_tasks,
        high_priority_tasks: world.high_priority_tasks,
        upcoming_reminders: world.upcoming_reminders,
        next_reminder_at: world.next_reminder_at,
    }
}

fn decide_initiative(
    target: Option<&InitiativeTarget>,
    presence_state: PresenceState,
    runtime_mode: RuntimeModeSnapshot,
    resource: &ResourceSnapshot,
    signal: Option<&InitiativeSignalSnapshot>,
    idle_enabled: bool,
    now_secs: u64,
) -> InitiativeDecision {
    let Some(target) = target else {
        return InitiativeDecision {
            action: InitiativeAction::Hold,
            rationale: "no_active_relation_target",
            suppression_reason: Some(InitiativeSuppressionReason::NoTargetRelation),
            last_triggered_at: None,
            next_allowed_at: None,
        };
    };
    let Some(signal) = signal else {
        return InitiativeDecision {
            action: InitiativeAction::Hold,
            rationale: "active_relation_missing_signal",
            suppression_reason: Some(InitiativeSuppressionReason::StaleActiveRelation),
            last_triggered_at: None,
            next_allowed_at: None,
        };
    };
    if signal.user_idle_secs > INITIATIVE_ACTIVE_RELATION_MAX_IDLE_SECS {
        return InitiativeDecision {
            action: InitiativeAction::Hold,
            rationale: "active_relation_went_stale",
            suppression_reason: Some(InitiativeSuppressionReason::StaleActiveRelation),
            last_triggered_at: None,
            next_allowed_at: None,
        };
    }
    if !runtime_mode.action_budget.allow_idle_self_runtime
        || !runtime_mode.action_budget.allow_non_voice_outbound
    {
        return InitiativeDecision {
            action: InitiativeAction::Hold,
            rationale: "runtime_mode_blocks_proactive_outbound",
            suppression_reason: Some(InitiativeSuppressionReason::RuntimeModeBlocked),
            last_triggered_at: None,
            next_allowed_at: None,
        };
    }
    if presence_state != PresenceState::Idle {
        return InitiativeDecision {
            action: InitiativeAction::Hold,
            rationale: "device_presence_not_idle",
            suppression_reason: Some(InitiativeSuppressionReason::PresenceNotIdle),
            last_triggered_at: None,
            next_allowed_at: None,
        };
    }
    if resource.pressure != PressureLevel::Normal {
        return InitiativeDecision {
            action: InitiativeAction::Hold,
            rationale: "resource_pressure_requires_silence",
            suppression_reason: Some(InitiativeSuppressionReason::ResourcePressure),
            last_triggered_at: None,
            next_allowed_at: None,
        };
    }
    if resource.active_agent_tasks > 0 || resource.inbound_depth > 0 || resource.outbound_depth > 0
    {
        return InitiativeDecision {
            action: InitiativeAction::Hold,
            rationale: "runtime_queues_are_busy",
            suppression_reason: Some(InitiativeSuppressionReason::QueuesBusy),
            last_triggered_at: None,
            next_allowed_at: None,
        };
    }
    if !idle_enabled {
        return InitiativeDecision {
            action: InitiativeAction::Hold,
            rationale: "autonomy_strategy_disabled_idle_intervention",
            suppression_reason: Some(InitiativeSuppressionReason::AutonomyDisabled),
            last_triggered_at: None,
            next_allowed_at: None,
        };
    }
    if signal.due_tasks > 0 || (signal.next_reminder_at > 0 && signal.next_reminder_at <= now_secs)
    {
        return InitiativeDecision {
            action: InitiativeAction::Hold,
            rationale: "exact_due_signal_will_handle_this",
            suppression_reason: Some(InitiativeSuppressionReason::ExactDueSignalPending),
            last_triggered_at: None,
            next_allowed_at: None,
        };
    }

    let action = if signal.next_reminder_at > now_secs
        && signal.next_reminder_at.saturating_sub(now_secs) <= INITIATIVE_REMINDER_LOOKAHEAD_SECS
        && signal.user_idle_secs <= INITIATIVE_REMINDER_ACTIVE_WINDOW_SECS
    {
        InitiativeAction::UpcomingReminderNudge
    } else if signal.next_reminder_at == 0
        && signal.in_progress_tasks > 0
        && signal.user_idle_secs >= INITIATIVE_RESUME_IDLE_MIN_SECS
        && signal.user_idle_secs <= INITIATIVE_RESUME_IDLE_MAX_SECS
    {
        InitiativeAction::ResumeTaskCheckIn
    } else {
        InitiativeAction::Hold
    };

    if action == InitiativeAction::Hold {
        return InitiativeDecision {
            action,
            rationale: "no_boundary_safe_trigger",
            suppression_reason: Some(InitiativeSuppressionReason::NoUsefulTrigger),
            last_triggered_at: None,
            next_allowed_at: None,
        };
    }

    let last_triggered = last_triggered_at(&target.scope_id, action);
    let cooldown_secs = action.cooldown_secs();
    if let Some(last_triggered_at) = last_triggered {
        let next_allowed_at = last_triggered_at.saturating_add(cooldown_secs);
        if now_secs < next_allowed_at {
            return InitiativeDecision {
                action: InitiativeAction::Hold,
                rationale: "initiative_cooldown_active",
                suppression_reason: Some(InitiativeSuppressionReason::CooldownActive),
                last_triggered_at: Some(last_triggered_at),
                next_allowed_at: Some(next_allowed_at),
            };
        }
    }

    InitiativeDecision {
        action,
        rationale: match action {
            InitiativeAction::UpcomingReminderNudge => "reminder_due_soon_with_active_user_context",
            InitiativeAction::ResumeTaskCheckIn => {
                "in_progress_task_stalled_but_relation_still_warm"
            }
            InitiativeAction::Hold => "no_boundary_safe_trigger",
        },
        suppression_reason: None,
        last_triggered_at: last_triggered,
        next_allowed_at: None,
    }
}

fn build_message_preview(
    action: InitiativeAction,
    signal: &InitiativeSignalSnapshot,
    now_secs: u64,
    loc: crate::i18n::Locale,
) -> Option<String> {
    match action {
        InitiativeAction::Hold => None,
        InitiativeAction::UpcomingReminderNudge => {
            let minutes = minutes_ceil(signal.next_reminder_at.saturating_sub(now_secs));
            Some(tr(
                Message::InitiativeUpcomingReminder {
                    minutes: minutes.max(1),
                },
                loc,
            ))
        }
        InitiativeAction::ResumeTaskCheckIn => Some(tr(
            Message::InitiativeResumeTaskCheckIn {
                idle_minutes: minutes_ceil(signal.user_idle_secs).max(1),
                high_priority: signal.high_priority_tasks > 0,
            },
            loc,
        )),
    }
}

pub fn inspect_platform_initiative(platform: &dyn Platform, now_secs: u64) -> InitiativeSnapshot {
    let presence = inspect_platform_presence(platform, now_secs);
    let resource = orchestrator::snapshot();
    let subject_id = board_subject_scope_id();
    let continuity = platform
        .self_continuity_store()
        .get(subject_id)
        .ok()
        .flatten();
    let strategy = platform
        .autonomy_strategy_store()
        .get(subject_id)
        .ok()
        .flatten();
    let target = select_target(continuity.as_ref(), now_secs, platform);
    let world = target.as_ref().map(|target| {
        let remind_store = platform.remind_at_store();
        let task_store = platform.task_store();
        build_world_snapshot(WorldSnapshotContext {
            chat_id: target.chat_id.as_str(),
            source_channel: target.channel.as_str(),
            now_secs,
            self_continuity: continuity.as_ref(),
            remind_store: remind_store.as_ref(),
            task_store: task_store.as_ref(),
        })
    });
    let signal = world.as_ref().map(build_signal);
    let idle_enabled = strategy
        .as_ref()
        .is_none_or(|strategy| strategy.idle_enabled);
    let decision = decide_initiative(
        target.as_ref(),
        presence.state,
        presence.runtime_mode,
        &resource,
        signal.as_ref(),
        idle_enabled,
        now_secs,
    );
    let locale = locale_from_store(platform.config_store().as_ref());
    let message_preview = signal
        .as_ref()
        .and_then(|signal| build_message_preview(decision.action, signal, now_secs, locale));
    InitiativeSnapshot {
        action: decision.action,
        ready: decision.suppression_reason.is_none() && target.is_some() && signal.is_some(),
        presence_state: presence.state,
        runtime_mode: presence.runtime_mode,
        rationale: decision.rationale.to_string(),
        suppression_reason: decision.suppression_reason,
        target,
        signal,
        strategy_mode: strategy
            .as_ref()
            .map(|strategy| strategy.current_mode.trim().to_string())
            .unwrap_or_default(),
        strategy_focus: strategy
            .as_ref()
            .map(|strategy| strategy.next_focus.trim().to_string())
            .unwrap_or_default(),
        idle_enabled,
        message_preview,
        last_triggered_at: decision.last_triggered_at,
        next_allowed_at: decision.next_allowed_at,
    }
}

pub fn initiative_tick(
    platform: &dyn Platform,
    system_inbound_tx: &SystemInboundTx,
    now_secs: u64,
) -> bool {
    let snapshot = inspect_platform_initiative(platform, now_secs);
    if !snapshot.ready {
        return false;
    }
    let Some(target) = snapshot.target.as_ref() else {
        return false;
    };
    let Some(message_preview) = snapshot.message_preview.as_ref() else {
        return false;
    };
    let msg = match PcMsg::new_inbound_with_ingress(
        target.channel.as_str(),
        target.chat_id.as_str(),
        message_preview,
        false,
        IngressKind::System,
    ) {
        Ok(msg) => msg,
        Err(error) => {
            log::warn!(
                "[initiative] failed to build proactive message scope_id={}: {}",
                target.scope_id,
                error
            );
            return false;
        }
    };
    match system_inbound_tx.try_send(msg) {
        Ok(()) => {
            record_trigger(&target.scope_id, snapshot.action, now_secs);
            let _ = touch_relationship_portfolio_selection(
                platform.relationship_portfolio_store().as_ref(),
                target.scope_id.as_str(),
                now_secs,
            );
            log::info!(
                "[initiative] triggered action={} scope_id={} rationale={}",
                snapshot.action.as_str(),
                target.scope_id,
                snapshot.rationale
            );
            true
        }
        Err(std::sync::mpsc::TrySendError::Full(_)) => {
            log::debug!("[initiative] skip proactive message because system queue is full");
            false
        }
        Err(std::sync::mpsc::TrySendError::Disconnected(_)) => {
            log::warn!("[initiative] system queue disconnected");
            false
        }
    }
}

#[cfg(test)]
pub fn reset_initiative_runtime_for_tests() {
    initiative_runtime_state()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clear();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::{RuntimeMode, RuntimeModeActionBudget};

    fn runtime_mode() -> RuntimeModeSnapshot {
        RuntimeModeSnapshot {
            current_mode: RuntimeMode::Normal,
            wifi_sta_connected: true,
            boot_phase_active: false,
            pairing_required: false,
            pairing_state_known: true,
            voice_exclusive_active: false,
            background_maintenance_active: false,
            config_plane_alive: false,
            channel_plane_alive: false,
            voice_plane_alive: false,
            agent_plane_alive: false,
            user_agent_lane_alive: false,
            system_agent_lane_alive: false,
            dual_agent_lanes_alive: false,
            external_wss_managed_present: false,
            external_wss_suspend_requested: false,
            external_wss_suspended: false,
            supervisor_present: false,
            supervisor_alive: false,
            supervisor_agent_alive: false,
            recovery_safe_mode_active: false,
            action_budget: RuntimeModeActionBudget {
                allow_periodic_maintenance: true,
                allow_due_user_timers: true,
                allow_heartbeat_injection: true,
                allow_best_effort_delayed_tasks: true,
                allow_idle_self_runtime: true,
                allow_non_voice_outbound: true,
                allow_external_wss_connect: true,
                require_external_wss_suspended: false,
            },
        }
    }

    fn resource() -> ResourceSnapshot {
        ResourceSnapshot {
            pressure: PressureLevel::Normal,
            heap_free_internal: 0,
            heap_free_spiram: 0,
            heap_largest_block_internal: 0,
            active_http_count: 0,
            active_wss_count: 0,
            active_agent_tasks: 0,
            inbound_depth: 0,
            outbound_depth: 0,
            budget: crate::orchestrator::current_budget(),
            channels: crate::orchestrator::state::ChannelsHealthSnapshot {
                telegram: healthy_channel(),
                feishu: healthy_channel(),
                dingtalk: healthy_channel(),
                wecom: healthy_channel(),
                qq_channel: healthy_channel(),
            },
            session_count: 0,
            storage_used_kb: 0,
            storage_total_kb: 0,
            audio_recording: false,
            audio_playing: false,
            audio_interrupt_listening: false,
            audio_interrupt_requested: false,
            #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
            cpu_usage_percent: 0.0,
            #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
            load_average: (0.0, 0.0, 0.0),
            #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
            process_memory_kb: 0,
        }
    }

    fn healthy_channel() -> crate::orchestrator::state::ChannelHealthSnapshot {
        crate::orchestrator::state::ChannelHealthSnapshot {
            consecutive_failures: 0,
            total_failures: 0,
            total_successes: 0,
            healthy: true,
        }
    }

    fn target() -> InitiativeTarget {
        InitiativeTarget {
            scope_id: "rel:qq:test".to_string(),
            channel: "qq_channel".to_string(),
            chat_id: "chat-1".to_string(),
            selection_reason: "preferred_relation".to_string(),
        }
    }

    fn signal() -> InitiativeSignalSnapshot {
        InitiativeSignalSnapshot {
            user_idle_secs: 10 * 60,
            autonomy_idle_secs: 10 * 60,
            in_progress_tasks: 1,
            due_tasks: 0,
            high_priority_tasks: 1,
            upcoming_reminders: 1,
            next_reminder_at: 10_000,
        }
    }

    #[test]
    fn upcoming_reminder_nudge_wins_when_reminder_is_near() {
        reset_initiative_runtime_for_tests();
        let mut signal = signal();
        signal.next_reminder_at = 10_000;
        let decision = decide_initiative(
            Some(&target()),
            PresenceState::Idle,
            runtime_mode(),
            &resource(),
            Some(&signal),
            true,
            10_000 - 8 * 60,
        );
        assert_eq!(decision.action, InitiativeAction::UpcomingReminderNudge);
        assert!(decision.suppression_reason.is_none());
    }

    #[test]
    fn resume_checkin_applies_after_idle_gap() {
        reset_initiative_runtime_for_tests();
        let mut signal = signal();
        signal.upcoming_reminders = 0;
        signal.next_reminder_at = 0;
        signal.user_idle_secs = 70 * 60;
        let decision = decide_initiative(
            Some(&target()),
            PresenceState::Idle,
            runtime_mode(),
            &resource(),
            Some(&signal),
            true,
            20_000,
        );
        assert_eq!(decision.action, InitiativeAction::ResumeTaskCheckIn);
    }

    #[test]
    fn cooldown_suppresses_repeat_nudge() {
        reset_initiative_runtime_for_tests();
        let now_secs = 1_000 + 60;
        let mut signal = signal();
        signal.next_reminder_at = now_secs + 8 * 60;
        record_trigger(
            &target().scope_id,
            InitiativeAction::UpcomingReminderNudge,
            1_000,
        );
        let decision = decide_initiative(
            Some(&target()),
            PresenceState::Idle,
            runtime_mode(),
            &resource(),
            Some(&signal),
            true,
            now_secs,
        );
        assert_eq!(
            decision.suppression_reason,
            Some(InitiativeSuppressionReason::CooldownActive)
        );
    }

    #[test]
    fn non_idle_presence_blocks_proactive_nudge() {
        reset_initiative_runtime_for_tests();
        let decision = decide_initiative(
            Some(&target()),
            PresenceState::Busy,
            runtime_mode(),
            &resource(),
            Some(&signal()),
            true,
            20_000,
        );
        assert_eq!(
            decision.suppression_reason,
            Some(InitiativeSuppressionReason::PresenceNotIdle)
        );
    }
}
