//! Global runtime work scheduler.
//! 全局运行时 work 调度准入真源。

use crate::memory::MemorySystemKind;
use crate::orchestrator::PressureLevel;
use crate::runtime::{
    RuntimeForegroundOverlay, RuntimeForegroundSource, RuntimeMode, RuntimeModeSnapshot,
};
use std::sync::{Mutex, OnceLock};

const RECENT_DECISION_LIMIT: usize = 16;
const FOREGROUND_RECOVERY_RETRY_FLOOR_MS: u64 = 1_000;

/// Platform policy profile used to project one scheduler decision model.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimePlanePolicyProfile {
    EspCompact,
    #[default]
    LinuxFull,
    EmbeddedLinux,
}

impl From<MemorySystemKind> for RuntimePlanePolicyProfile {
    fn from(value: MemorySystemKind) -> Self {
        match value {
            MemorySystemKind::EspCompact => Self::EspCompact,
            MemorySystemKind::LinuxFull => Self::LinuxFull,
        }
    }
}

impl RuntimePlanePolicyProfile {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::EspCompact => "esp_compact",
            Self::LinuxFull => "linux_full",
            Self::EmbeddedLinux => "embedded_linux",
        }
    }
}

/// Runtime work class consumed by scheduler admission.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeWorkClass {
    ExternalUserMessage,
    ConfigUiChat,
    RealtimeVoiceSession,
    VoiceFallbackInteraction,
    VisibilityDelivery,
    PrimaryReplyDelivery,
    SupplementalDelivery,
    ChannelIngressWss,
    ChannelReconnect,
    ImmediateStatusRoute,
    ConfigUiChatHistoryRoute,
    DeepRouteWorker,
    DisplayStatusSurface,
    DisplayHeavyRefresh,
    DurableWriteBack,
    DueUserTimer,
    OptionalMaintenance,
    SelfRuntimeLlmWork,
    HardwareRealtimeCapture,
    WakePcmFeed,
}

impl RuntimeWorkClass {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ExternalUserMessage => "external_user_message",
            Self::ConfigUiChat => "config_ui_chat",
            Self::RealtimeVoiceSession => "realtime_voice_session",
            Self::VoiceFallbackInteraction => "voice_fallback_interaction",
            Self::VisibilityDelivery => "visibility_delivery",
            Self::PrimaryReplyDelivery => "primary_reply_delivery",
            Self::SupplementalDelivery => "supplemental_delivery",
            Self::ChannelIngressWss => "channel_ingress_wss",
            Self::ChannelReconnect => "channel_reconnect",
            Self::ImmediateStatusRoute => "immediate_status_route",
            Self::ConfigUiChatHistoryRoute => "config_ui_chat_history_route",
            Self::DeepRouteWorker => "deep_route_worker",
            Self::DisplayStatusSurface => "display_status_surface",
            Self::DisplayHeavyRefresh => "display_heavy_refresh",
            Self::DurableWriteBack => "durable_write_back",
            Self::DueUserTimer => "due_user_timer",
            Self::OptionalMaintenance => "optional_maintenance",
            Self::SelfRuntimeLlmWork => "self_runtime_llm_work",
            Self::HardwareRealtimeCapture => "hardware_realtime_capture",
            Self::WakePcmFeed => "wake_pcm_feed",
        }
    }

    pub fn priority(self) -> RuntimeWorkPriority {
        match self {
            Self::ExternalUserMessage
            | Self::ConfigUiChat
            | Self::RealtimeVoiceSession
            | Self::VoiceFallbackInteraction
            | Self::VisibilityDelivery
            | Self::PrimaryReplyDelivery
            | Self::WakePcmFeed => RuntimeWorkPriority::Critical,
            Self::ChannelIngressWss
            | Self::ImmediateStatusRoute
            | Self::DueUserTimer
            | Self::HardwareRealtimeCapture => RuntimeWorkPriority::High,
            Self::SupplementalDelivery
            | Self::ChannelReconnect
            | Self::ConfigUiChatHistoryRoute
            | Self::DeepRouteWorker
            | Self::DurableWriteBack => RuntimeWorkPriority::Normal,
            Self::DisplayStatusSurface => RuntimeWorkPriority::High,
            Self::DisplayHeavyRefresh | Self::OptionalMaintenance | Self::SelfRuntimeLlmWork => {
                RuntimeWorkPriority::Low
            }
        }
    }

    pub fn foreground_source(self) -> Option<RuntimeForegroundSource> {
        match self {
            Self::ExternalUserMessage => Some(RuntimeForegroundSource::ExternalUserMessage),
            Self::ConfigUiChat => Some(RuntimeForegroundSource::ConfigUiChat),
            Self::RealtimeVoiceSession => Some(RuntimeForegroundSource::RealtimeVoiceSession),
            Self::VoiceFallbackInteraction => {
                Some(RuntimeForegroundSource::VoiceFallbackInteraction)
            }
            _ => None,
        }
    }

    fn is_voice_foreground(self) -> bool {
        matches!(
            self,
            Self::RealtimeVoiceSession | Self::VoiceFallbackInteraction | Self::WakePcmFeed
        )
    }

    fn is_primary_user_visible(self) -> bool {
        matches!(
            self,
            Self::ExternalUserMessage
                | Self::ConfigUiChat
                | Self::RealtimeVoiceSession
                | Self::VoiceFallbackInteraction
                | Self::VisibilityDelivery
                | Self::PrimaryReplyDelivery
                | Self::WakePcmFeed
                | Self::ImmediateStatusRoute
                | Self::DisplayStatusSurface
                | Self::ChannelIngressWss
        )
    }

    fn is_deferred_during_post_foreground_recovery(self) -> bool {
        matches!(
            self,
            Self::DeepRouteWorker
                | Self::ConfigUiChatHistoryRoute
                | Self::DurableWriteBack
                | Self::OptionalMaintenance
                | Self::SelfRuntimeLlmWork
                | Self::SupplementalDelivery
        )
    }
}

/// Scheduler priority class.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeWorkPriority {
    Low,
    Normal,
    High,
    Critical,
}

impl RuntimeWorkPriority {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Normal => "normal",
            Self::High => "high",
            Self::Critical => "critical",
        }
    }
}

/// Logical source of a scheduler request.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeWorkSource {
    UserFacing,
    System,
    Background,
    Operator,
}

impl RuntimeWorkSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::UserFacing => "user_facing",
            Self::System => "system",
            Self::Background => "background",
            Self::Operator => "operator",
        }
    }
}

/// Runtime work admission request.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub struct RuntimeWorkRequest {
    pub class: RuntimeWorkClass,
    pub source: RuntimeWorkSource,
    pub priority: RuntimeWorkPriority,
}

impl RuntimeWorkRequest {
    pub fn new(class: RuntimeWorkClass, source: RuntimeWorkSource) -> Self {
        Self {
            class,
            source,
            priority: class.priority(),
        }
    }
}

/// Scheduler decision returned to existing resource/transport consumers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeWorkDecision {
    Proceed,
    Defer {
        reason: &'static str,
        retry_after_ms: u64,
    },
    Degrade {
        reason: &'static str,
    },
    DrainAndResume {
        reason: &'static str,
        retry_after_ms: u64,
    },
    Suspend {
        reason: &'static str,
    },
    RejectWithStableKey {
        key: &'static str,
        reason: &'static str,
    },
    RejectWithUserVisibleReason {
        reason: &'static str,
    },
}

impl RuntimeWorkDecision {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Proceed => "proceed",
            Self::Defer { .. } => "defer",
            Self::Degrade { .. } => "degrade",
            Self::DrainAndResume { .. } => "drain_and_resume",
            Self::Suspend { .. } => "suspend",
            Self::RejectWithStableKey { .. } | Self::RejectWithUserVisibleReason { .. } => "reject",
        }
    }

    fn reason(self) -> Option<&'static str> {
        match self {
            Self::Proceed => None,
            Self::Defer { reason, .. }
            | Self::Degrade { reason }
            | Self::DrainAndResume { reason, .. }
            | Self::Suspend { reason }
            | Self::RejectWithStableKey { reason, .. }
            | Self::RejectWithUserVisibleReason { reason } => Some(reason),
        }
    }

    fn retry_after_ms(self) -> Option<u64> {
        match self {
            Self::Defer { retry_after_ms, .. } | Self::DrainAndResume { retry_after_ms, .. } => {
                Some(retry_after_ms)
            }
            _ => None,
        }
    }
}

/// Snapshot of scheduler inputs for a single admission decision.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub struct RuntimeSchedulerContext {
    pub profile: RuntimePlanePolicyProfile,
    pub runtime_mode: RuntimeModeSnapshot,
    pub foreground: RuntimeForegroundOverlay,
    pub pressure: PressureLevel,
}

/// Compact scheduler observability snapshot.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub struct RuntimeSchedulerSnapshot {
    pub active_foreground: bool,
    pub active_work: usize,
    pub foreground_source: Option<RuntimeForegroundSource>,
    pub foreground_age_ms: Option<u64>,
    pub resume_after_ms: Option<u64>,
    pub foreground_recovery_active: bool,
    pub foreground_recovery_source: Option<RuntimeForegroundSource>,
    pub foreground_recovery_age_ms: Option<u64>,
    pub foreground_recovery_resume_after_ms: Option<u64>,
    pub profile: RuntimePlanePolicyProfile,
    pub permits: u64,
    pub defers: u64,
    pub degrades: u64,
    pub suspends: u64,
    pub drains: u64,
    pub rejects: u64,
    pub recent_decisions: Vec<RuntimeSchedulerDecisionRecord>,
}

/// Bounded record of a recent scheduler admission decision.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub struct RuntimeSchedulerDecisionRecord {
    pub request: RuntimeWorkRequest,
    pub decision: RuntimeWorkDecision,
    pub profile: RuntimePlanePolicyProfile,
    pub pressure: PressureLevel,
    pub foreground_active: bool,
    pub foreground_source: Option<RuntimeForegroundSource>,
    pub foreground_resume_after_ms: Option<u64>,
    pub foreground_recovery_active: bool,
    pub foreground_recovery_source: Option<RuntimeForegroundSource>,
    pub foreground_recovery_resume_after_ms: Option<u64>,
}

/// Admit one runtime work item without taking ownership of resource execution.
pub fn admit_runtime_work(
    request: RuntimeWorkRequest,
    context: RuntimeSchedulerContext,
) -> RuntimeWorkDecision {
    let decision = decide_runtime_work(request, context);
    record_runtime_scheduler_decision(request, context, decision);
    decision
}

fn decide_runtime_work(
    request: RuntimeWorkRequest,
    context: RuntimeSchedulerContext,
) -> RuntimeWorkDecision {
    if request.class == RuntimeWorkClass::WakePcmFeed {
        return RuntimeWorkDecision::Proceed;
    }

    if context.runtime_mode.current_mode == RuntimeMode::VoiceExclusive
        && !request.class.is_voice_foreground()
        && request.class != RuntimeWorkClass::DisplayStatusSurface
    {
        return RuntimeWorkDecision::Defer {
            reason: "voice_exclusive_active",
            retry_after_ms: 1_000,
        };
    }

    if context.profile != RuntimePlanePolicyProfile::EspCompact {
        return RuntimeWorkDecision::Proceed;
    }

    if request.source == RuntimeWorkSource::Background
        && matches!(
            request.class,
            RuntimeWorkClass::RealtimeVoiceSession | RuntimeWorkClass::VoiceFallbackInteraction
        )
        && !matches!(context.pressure, PressureLevel::Normal)
    {
        return RuntimeWorkDecision::Defer {
            reason: if matches!(context.pressure, PressureLevel::Critical) {
                "critical_pressure"
            } else {
                "cautious_pressure"
            },
            retry_after_ms: 1_500,
        };
    }

    if matches!(context.pressure, PressureLevel::Critical)
        && !request.class.is_primary_user_visible()
    {
        return RuntimeWorkDecision::Defer {
            reason: "critical_pressure",
            retry_after_ms: 1_500,
        };
    }

    if context.foreground.active {
        match request.class {
            RuntimeWorkClass::RealtimeVoiceSession | RuntimeWorkClass::VoiceFallbackInteraction
                if request.source != RuntimeWorkSource::UserFacing
                    && context.foreground.primary_source != request.class.foreground_source() =>
            {
                return RuntimeWorkDecision::Defer {
                    reason: "foreground_active",
                    retry_after_ms: context.foreground.resume_after_ms.unwrap_or(1_000),
                };
            }
            RuntimeWorkClass::HardwareRealtimeCapture
                if request.source != RuntimeWorkSource::UserFacing =>
            {
                return RuntimeWorkDecision::Defer {
                    reason: "foreground_active",
                    retry_after_ms: context.foreground.resume_after_ms.unwrap_or(1_000),
                };
            }
            RuntimeWorkClass::DisplayHeavyRefresh => {
                return RuntimeWorkDecision::Degrade {
                    reason: "foreground_active",
                };
            }
            RuntimeWorkClass::DeepRouteWorker
            | RuntimeWorkClass::ConfigUiChatHistoryRoute
            | RuntimeWorkClass::DurableWriteBack
            | RuntimeWorkClass::OptionalMaintenance
            | RuntimeWorkClass::SelfRuntimeLlmWork
            | RuntimeWorkClass::ChannelReconnect
            | RuntimeWorkClass::SupplementalDelivery => {
                return RuntimeWorkDecision::Defer {
                    reason: "foreground_active",
                    retry_after_ms: context.foreground.resume_after_ms.unwrap_or(1_000),
                };
            }
            _ => {}
        }
    }

    if context.foreground.recovery_active {
        let retry_after_ms = context
            .foreground
            .recovery_resume_after_ms
            .unwrap_or(FOREGROUND_RECOVERY_RETRY_FLOOR_MS)
            .max(FOREGROUND_RECOVERY_RETRY_FLOOR_MS);
        if request.class == RuntimeWorkClass::DisplayHeavyRefresh {
            return RuntimeWorkDecision::Degrade {
                reason: "foreground_recovery",
            };
        }
        if request.source == RuntimeWorkSource::Background
            && matches!(
                request.class,
                RuntimeWorkClass::RealtimeVoiceSession | RuntimeWorkClass::VoiceFallbackInteraction
            )
        {
            return RuntimeWorkDecision::Defer {
                reason: "foreground_recovery",
                retry_after_ms,
            };
        }
        if request.class.is_deferred_during_post_foreground_recovery() {
            return RuntimeWorkDecision::Defer {
                reason: "foreground_recovery",
                retry_after_ms,
            };
        }
    }

    RuntimeWorkDecision::Proceed
}

#[derive(Default)]
struct RuntimeSchedulerObservability {
    permits: u64,
    defers: u64,
    degrades: u64,
    suspends: u64,
    drains: u64,
    rejects: u64,
    recent_decisions: Vec<RuntimeSchedulerDecisionRecord>,
}

static OBSERVABILITY: OnceLock<Mutex<RuntimeSchedulerObservability>> = OnceLock::new();

fn observability() -> &'static Mutex<RuntimeSchedulerObservability> {
    OBSERVABILITY.get_or_init(|| Mutex::new(RuntimeSchedulerObservability::default()))
}

fn record_runtime_scheduler_decision(
    request: RuntimeWorkRequest,
    context: RuntimeSchedulerContext,
    decision: RuntimeWorkDecision,
) {
    let mut guard = observability().lock().unwrap_or_else(|e| e.into_inner());
    match decision {
        RuntimeWorkDecision::Proceed => guard.permits = guard.permits.saturating_add(1),
        RuntimeWorkDecision::Defer { .. } => guard.defers = guard.defers.saturating_add(1),
        RuntimeWorkDecision::Degrade { .. } => guard.degrades = guard.degrades.saturating_add(1),
        RuntimeWorkDecision::DrainAndResume { .. } => {
            guard.drains = guard.drains.saturating_add(1);
        }
        RuntimeWorkDecision::Suspend { .. } => guard.suspends = guard.suspends.saturating_add(1),
        RuntimeWorkDecision::RejectWithStableKey { .. }
        | RuntimeWorkDecision::RejectWithUserVisibleReason { .. } => {
            guard.rejects = guard.rejects.saturating_add(1);
        }
    }

    if guard.recent_decisions.len() >= RECENT_DECISION_LIMIT {
        guard.recent_decisions.remove(0);
    }
    guard.recent_decisions.push(RuntimeSchedulerDecisionRecord {
        request,
        decision,
        profile: context.profile,
        pressure: context.pressure,
        foreground_active: context.foreground.active,
        foreground_source: context.foreground.primary_source,
        foreground_resume_after_ms: context.foreground.resume_after_ms,
        foreground_recovery_active: context.foreground.recovery_active,
        foreground_recovery_source: context.foreground.recovery_source,
        foreground_recovery_resume_after_ms: context.foreground.recovery_resume_after_ms,
    });
    log::info!(
        "[runtime_scheduler] runtime_scheduler_decision class={} source={} decision={} reason={} retry_after_ms={} foreground_active={} foreground_source={} resume_after_ms={} foreground_recovery_active={} foreground_recovery_source={} recovery_resume_after_ms={} profile={} pressure={:?}",
        request.class.as_str(),
        request.source.as_str(),
        decision.as_str(),
        decision.reason().unwrap_or("none"),
        decision
            .retry_after_ms()
            .map(|retry_after| retry_after.to_string())
            .unwrap_or_else(|| "none".to_string()),
        context.foreground.active,
        context
            .foreground
            .primary_source
            .map(|source| source.as_str())
            .unwrap_or("none"),
        context
            .foreground
            .resume_after_ms
            .map(|resume_after| resume_after.to_string())
            .unwrap_or_else(|| "none".to_string()),
        context.foreground.recovery_active,
        context
            .foreground
            .recovery_source
            .map(|source| source.as_str())
            .unwrap_or("none"),
        context
            .foreground
            .recovery_resume_after_ms
            .map(|resume_after| resume_after.to_string())
            .unwrap_or_else(|| "none".to_string()),
        context.profile.as_str(),
        context.pressure
    );
}

/// Return the current compact scheduler observability snapshot.
pub fn runtime_scheduler_snapshot() -> RuntimeSchedulerSnapshot {
    let pressure = crate::orchestrator::snapshot().pressure;
    runtime_scheduler_snapshot_for_context(current_runtime_scheduler_context(
        default_runtime_scheduler_profile(),
        pressure,
    ))
}

fn runtime_scheduler_snapshot_for_context(
    context: RuntimeSchedulerContext,
) -> RuntimeSchedulerSnapshot {
    let guard = observability().lock().unwrap_or_else(|e| e.into_inner());
    RuntimeSchedulerSnapshot {
        active_foreground: context.foreground.active,
        active_work: context.foreground.active_count,
        foreground_source: context.foreground.primary_source,
        foreground_age_ms: context.foreground.age_ms,
        resume_after_ms: context.foreground.resume_after_ms,
        foreground_recovery_active: context.foreground.recovery_active,
        foreground_recovery_source: context.foreground.recovery_source,
        foreground_recovery_age_ms: context.foreground.recovery_age_ms,
        foreground_recovery_resume_after_ms: context.foreground.recovery_resume_after_ms,
        profile: context.profile,
        permits: guard.permits,
        defers: guard.defers,
        degrades: guard.degrades,
        suspends: guard.suspends,
        drains: guard.drains,
        rejects: guard.rejects,
        recent_decisions: guard.recent_decisions.clone(),
    }
}

/// Return a compact scheduler baseline line for heartbeat and serial logs.
pub fn format_baseline_log_line() -> String {
    let pressure = crate::orchestrator::snapshot().pressure;
    format_baseline_log_line_for_context(current_runtime_scheduler_context(
        default_runtime_scheduler_profile(),
        pressure,
    ))
}

fn format_baseline_log_line_for_context(context: RuntimeSchedulerContext) -> String {
    let snapshot = runtime_scheduler_snapshot_for_context(context);
    let last = snapshot.recent_decisions.last().copied();
    format!(
        "runtime_scheduler active_foreground={} source={} age_ms={} resume_after_ms={} foreground_recovery_active={} recovery_source={} recovery_age_ms={} recovery_resume_after_ms={} active_work={} profile={} permits={} defers={} degrades={} suspends={} drains={} rejects={} last_class={} last_source={} last_decision={} last_reason={} last_retry_after_ms={}",
        snapshot.active_foreground,
        snapshot
            .foreground_source
            .map(|source| source.as_str())
            .unwrap_or("none"),
        snapshot
            .foreground_age_ms
            .map(|age| age.to_string())
            .unwrap_or_else(|| "none".to_string()),
        snapshot
            .resume_after_ms
            .map(|resume_after| resume_after.to_string())
            .unwrap_or_else(|| "none".to_string()),
        snapshot.foreground_recovery_active,
        snapshot
            .foreground_recovery_source
            .map(|source| source.as_str())
            .unwrap_or("none"),
        snapshot
            .foreground_recovery_age_ms
            .map(|age| age.to_string())
            .unwrap_or_else(|| "none".to_string()),
        snapshot
            .foreground_recovery_resume_after_ms
            .map(|resume_after| resume_after.to_string())
            .unwrap_or_else(|| "none".to_string()),
        snapshot.active_work,
        snapshot.profile.as_str(),
        snapshot.permits,
        snapshot.defers,
        snapshot.degrades,
        snapshot.suspends,
        snapshot.drains,
        snapshot.rejects,
        last.map(|record| record.request.class.as_str())
            .unwrap_or("none"),
        last.map(|record| record.request.source.as_str())
            .unwrap_or("none"),
        last.map(|record| record.decision.as_str()).unwrap_or("none"),
        last.and_then(|record| record.decision.reason())
            .unwrap_or("none"),
        last.and_then(|record| record.decision.retry_after_ms())
            .map(|retry_after| retry_after.to_string())
            .unwrap_or_else(|| "none".to_string()),
    )
}

/// Return the static runtime policy projection line for heartbeat logs.
pub fn format_policy_baseline_log_line() -> String {
    let profile = default_runtime_scheduler_profile();
    format!(
        "runtime_policy profile={} deep_route=foreground_defer_esp voice=auto_defer_user_facing_proceed display=status_retained_heavy_degrade wss=reconnect_defer_active_session_retained write_back=foreground_defer_quiet_drain",
        profile.as_str()
    )
}

#[cfg(test)]
fn reset_runtime_scheduler_observability_for_tests() {
    let mut guard = observability().lock().unwrap_or_else(|e| e.into_inner());
    *guard = RuntimeSchedulerObservability::default();
}

/// Runtime scheduler profile used by the current binary target.
pub fn default_runtime_scheduler_profile() -> RuntimePlanePolicyProfile {
    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
    {
        RuntimePlanePolicyProfile::EspCompact
    }
    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
    {
        RuntimePlanePolicyProfile::LinuxFull
    }
}

/// Build a scheduler context from current runtime truth sources.
pub fn current_runtime_scheduler_context(
    profile: RuntimePlanePolicyProfile,
    pressure: PressureLevel,
) -> RuntimeSchedulerContext {
    RuntimeSchedulerContext {
        profile,
        runtime_mode: crate::runtime::thread_registry::runtime_mode_snapshot(),
        foreground: crate::runtime::foreground::runtime_foreground_overlay(),
        pressure,
    }
}

/// Admit one work item against current runtime mode and foreground overlay.
pub fn admit_current_runtime_work(
    class: RuntimeWorkClass,
    source: RuntimeWorkSource,
    profile: RuntimePlanePolicyProfile,
    pressure: PressureLevel,
) -> RuntimeWorkDecision {
    admit_runtime_work(
        RuntimeWorkRequest::new(class, source),
        current_runtime_scheduler_context(profile, pressure),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orchestrator::PressureLevel;
    use crate::runtime::mode::{snapshot_from_source, RuntimeModeSource};
    use crate::runtime::{RuntimeForegroundOverlay, RuntimeForegroundSource};

    fn context(profile: RuntimePlanePolicyProfile) -> RuntimeSchedulerContext {
        RuntimeSchedulerContext {
            profile,
            runtime_mode: snapshot_from_source(RuntimeModeSource::default()),
            foreground: RuntimeForegroundOverlay::default(),
            pressure: PressureLevel::Normal,
        }
    }

    fn foreground_context(profile: RuntimePlanePolicyProfile) -> RuntimeSchedulerContext {
        RuntimeSchedulerContext {
            foreground: RuntimeForegroundOverlay {
                active: true,
                active_count: 1,
                primary_source: Some(RuntimeForegroundSource::ExternalUserMessage),
                age_ms: Some(500),
                resume_after_ms: Some(29_500),
                ..RuntimeForegroundOverlay::default()
            },
            ..context(profile)
        }
    }

    fn foreground_recovery_context(profile: RuntimePlanePolicyProfile) -> RuntimeSchedulerContext {
        RuntimeSchedulerContext {
            foreground: RuntimeForegroundOverlay {
                recovery_active: true,
                recovery_source: Some(RuntimeForegroundSource::ExternalUserMessage),
                recovery_age_ms: Some(500),
                recovery_resume_after_ms: Some(9_500),
                ..RuntimeForegroundOverlay::default()
            },
            ..context(profile)
        }
    }

    #[test]
    fn critical_foreground_work_classes_proceed_under_normal_pressure() {
        for class in [
            RuntimeWorkClass::ExternalUserMessage,
            RuntimeWorkClass::ConfigUiChat,
            RuntimeWorkClass::RealtimeVoiceSession,
            RuntimeWorkClass::VoiceFallbackInteraction,
            RuntimeWorkClass::VisibilityDelivery,
            RuntimeWorkClass::PrimaryReplyDelivery,
            RuntimeWorkClass::WakePcmFeed,
        ] {
            let decision = admit_runtime_work(
                RuntimeWorkRequest::new(class, RuntimeWorkSource::UserFacing),
                context(RuntimePlanePolicyProfile::EspCompact),
            );
            assert_eq!(decision, RuntimeWorkDecision::Proceed);
        }
    }

    #[test]
    fn esp_compact_defers_low_priority_work_while_foreground_is_active() {
        for class in [
            RuntimeWorkClass::DeepRouteWorker,
            RuntimeWorkClass::DisplayHeavyRefresh,
            RuntimeWorkClass::DurableWriteBack,
            RuntimeWorkClass::OptionalMaintenance,
            RuntimeWorkClass::SelfRuntimeLlmWork,
        ] {
            let decision = admit_runtime_work(
                RuntimeWorkRequest::new(class, RuntimeWorkSource::Background),
                foreground_context(RuntimePlanePolicyProfile::EspCompact),
            );
            assert!(matches!(
                decision,
                RuntimeWorkDecision::Defer {
                    reason: "foreground_active",
                    ..
                } | RuntimeWorkDecision::Degrade {
                    reason: "foreground_active"
                }
            ));
        }
    }

    #[test]
    fn esp_compact_defers_background_work_during_post_foreground_recovery() {
        for class in [
            RuntimeWorkClass::DeepRouteWorker,
            RuntimeWorkClass::ConfigUiChatHistoryRoute,
            RuntimeWorkClass::DurableWriteBack,
            RuntimeWorkClass::OptionalMaintenance,
            RuntimeWorkClass::SelfRuntimeLlmWork,
            RuntimeWorkClass::SupplementalDelivery,
        ] {
            let decision = admit_runtime_work(
                RuntimeWorkRequest::new(class, RuntimeWorkSource::Background),
                foreground_recovery_context(RuntimePlanePolicyProfile::EspCompact),
            );
            assert_eq!(
                decision,
                RuntimeWorkDecision::Defer {
                    reason: "foreground_recovery",
                    retry_after_ms: 9_500,
                },
                "{class:?} must not compete with WSS resume and post-reply heap recovery"
            );
        }
    }

    #[test]
    fn post_foreground_recovery_keeps_user_visible_and_channel_reconnect_work() {
        for (class, source) in [
            (
                RuntimeWorkClass::PrimaryReplyDelivery,
                RuntimeWorkSource::UserFacing,
            ),
            (
                RuntimeWorkClass::VisibilityDelivery,
                RuntimeWorkSource::UserFacing,
            ),
            (
                RuntimeWorkClass::ChannelReconnect,
                RuntimeWorkSource::Background,
            ),
            (
                RuntimeWorkClass::ImmediateStatusRoute,
                RuntimeWorkSource::System,
            ),
            (
                RuntimeWorkClass::DisplayStatusSurface,
                RuntimeWorkSource::System,
            ),
            (RuntimeWorkClass::DueUserTimer, RuntimeWorkSource::System),
        ] {
            let decision = admit_runtime_work(
                RuntimeWorkRequest::new(class, source),
                foreground_recovery_context(RuntimePlanePolicyProfile::EspCompact),
            );
            assert_eq!(decision, RuntimeWorkDecision::Proceed, "{class:?}");
        }
    }

    #[test]
    fn post_foreground_recovery_degrades_heavy_display_and_auto_voice() {
        let heavy_display = admit_runtime_work(
            RuntimeWorkRequest::new(
                RuntimeWorkClass::DisplayHeavyRefresh,
                RuntimeWorkSource::Background,
            ),
            foreground_recovery_context(RuntimePlanePolicyProfile::EspCompact),
        );
        assert_eq!(
            heavy_display,
            RuntimeWorkDecision::Degrade {
                reason: "foreground_recovery",
            }
        );

        let auto_voice = admit_runtime_work(
            RuntimeWorkRequest::new(
                RuntimeWorkClass::RealtimeVoiceSession,
                RuntimeWorkSource::Background,
            ),
            foreground_recovery_context(RuntimePlanePolicyProfile::EspCompact),
        );
        assert_eq!(
            auto_voice,
            RuntimeWorkDecision::Defer {
                reason: "foreground_recovery",
                retry_after_ms: 9_500,
            }
        );

        let user_voice = admit_runtime_work(
            RuntimeWorkRequest::new(
                RuntimeWorkClass::RealtimeVoiceSession,
                RuntimeWorkSource::UserFacing,
            ),
            foreground_recovery_context(RuntimePlanePolicyProfile::EspCompact),
        );
        assert_eq!(user_voice, RuntimeWorkDecision::Proceed);
    }

    #[test]
    fn due_user_timer_remains_foreground_safe_but_not_voice_exclusive_safe() {
        let foreground_decision = admit_runtime_work(
            RuntimeWorkRequest::new(RuntimeWorkClass::DueUserTimer, RuntimeWorkSource::System),
            foreground_context(RuntimePlanePolicyProfile::EspCompact),
        );
        assert_eq!(foreground_decision, RuntimeWorkDecision::Proceed);

        let source = RuntimeModeSource {
            voice_exclusive_active: true,
            ..RuntimeModeSource::default()
        };
        let voice_decision = admit_runtime_work(
            RuntimeWorkRequest::new(RuntimeWorkClass::DueUserTimer, RuntimeWorkSource::System),
            RuntimeSchedulerContext {
                runtime_mode: snapshot_from_source(source),
                ..context(RuntimePlanePolicyProfile::EspCompact)
            },
        );
        assert!(matches!(
            voice_decision,
            RuntimeWorkDecision::Defer {
                reason: "voice_exclusive_active",
                ..
            }
        ));
    }

    #[test]
    fn display_status_surface_survives_foreground_and_critical_pressure() {
        let foreground_decision = admit_runtime_work(
            RuntimeWorkRequest::new(
                RuntimeWorkClass::DisplayStatusSurface,
                RuntimeWorkSource::System,
            ),
            foreground_context(RuntimePlanePolicyProfile::EspCompact),
        );
        assert_eq!(foreground_decision, RuntimeWorkDecision::Proceed);

        let critical_decision = admit_runtime_work(
            RuntimeWorkRequest::new(
                RuntimeWorkClass::DisplayStatusSurface,
                RuntimeWorkSource::System,
            ),
            RuntimeSchedulerContext {
                pressure: PressureLevel::Critical,
                ..context(RuntimePlanePolicyProfile::EspCompact)
            },
        );
        assert_eq!(critical_decision, RuntimeWorkDecision::Proceed);

        let source = RuntimeModeSource {
            voice_exclusive_active: true,
            ..RuntimeModeSource::default()
        };
        let voice_exclusive_decision = admit_runtime_work(
            RuntimeWorkRequest::new(
                RuntimeWorkClass::DisplayStatusSurface,
                RuntimeWorkSource::System,
            ),
            RuntimeSchedulerContext {
                runtime_mode: snapshot_from_source(source),
                ..context(RuntimePlanePolicyProfile::EspCompact)
            },
        );
        assert_eq!(voice_exclusive_decision, RuntimeWorkDecision::Proceed);
    }

    #[test]
    fn esp_compact_defers_auto_voice_connect_during_other_foreground() {
        let auto_realtime = admit_runtime_work(
            RuntimeWorkRequest::new(
                RuntimeWorkClass::RealtimeVoiceSession,
                RuntimeWorkSource::Background,
            ),
            foreground_context(RuntimePlanePolicyProfile::EspCompact),
        );
        assert!(matches!(
            auto_realtime,
            RuntimeWorkDecision::Defer {
                reason: "foreground_active",
                ..
            }
        ));

        let auto_fallback = admit_runtime_work(
            RuntimeWorkRequest::new(
                RuntimeWorkClass::VoiceFallbackInteraction,
                RuntimeWorkSource::Background,
            ),
            foreground_context(RuntimePlanePolicyProfile::EspCompact),
        );
        assert!(matches!(
            auto_fallback,
            RuntimeWorkDecision::Defer {
                reason: "foreground_active",
                ..
            }
        ));

        let manual_realtime = admit_runtime_work(
            RuntimeWorkRequest::new(
                RuntimeWorkClass::RealtimeVoiceSession,
                RuntimeWorkSource::UserFacing,
            ),
            foreground_context(RuntimePlanePolicyProfile::EspCompact),
        );
        assert_eq!(manual_realtime, RuntimeWorkDecision::Proceed);

        let linux_auto_realtime = admit_runtime_work(
            RuntimeWorkRequest::new(
                RuntimeWorkClass::RealtimeVoiceSession,
                RuntimeWorkSource::Background,
            ),
            foreground_context(RuntimePlanePolicyProfile::LinuxFull),
        );
        assert_eq!(linux_auto_realtime, RuntimeWorkDecision::Proceed);
    }

    #[test]
    fn esp_compact_defers_background_auto_voice_under_cautious_pressure() {
        for class in [
            RuntimeWorkClass::RealtimeVoiceSession,
            RuntimeWorkClass::VoiceFallbackInteraction,
        ] {
            let auto_voice = admit_runtime_work(
                RuntimeWorkRequest::new(class, RuntimeWorkSource::Background),
                RuntimeSchedulerContext {
                    pressure: PressureLevel::Cautious,
                    ..context(RuntimePlanePolicyProfile::EspCompact)
                },
            );
            assert!(matches!(
                auto_voice,
                RuntimeWorkDecision::Defer {
                    reason: "cautious_pressure",
                    ..
                }
            ));

            let user_facing_voice = admit_runtime_work(
                RuntimeWorkRequest::new(class, RuntimeWorkSource::UserFacing),
                RuntimeSchedulerContext {
                    pressure: PressureLevel::Cautious,
                    ..context(RuntimePlanePolicyProfile::EspCompact)
                },
            );
            assert_eq!(user_facing_voice, RuntimeWorkDecision::Proceed);
        }
    }

    #[test]
    fn hardware_realtime_capture_defers_background_but_not_user_facing_foreground_work() {
        let user_facing = admit_runtime_work(
            RuntimeWorkRequest::new(
                RuntimeWorkClass::HardwareRealtimeCapture,
                RuntimeWorkSource::UserFacing,
            ),
            foreground_context(RuntimePlanePolicyProfile::EspCompact),
        );
        assert_eq!(
            user_facing,
            RuntimeWorkDecision::Proceed,
            "a user-requested camera/frame capture is part of the foreground turn and must not self-defer"
        );

        let background = admit_runtime_work(
            RuntimeWorkRequest::new(
                RuntimeWorkClass::HardwareRealtimeCapture,
                RuntimeWorkSource::Background,
            ),
            foreground_context(RuntimePlanePolicyProfile::EspCompact),
        );
        assert!(matches!(
            background,
            RuntimeWorkDecision::Defer {
                reason: "foreground_active",
                ..
            }
        ));

        let critical = admit_runtime_work(
            RuntimeWorkRequest::new(
                RuntimeWorkClass::HardwareRealtimeCapture,
                RuntimeWorkSource::UserFacing,
            ),
            RuntimeSchedulerContext {
                pressure: PressureLevel::Critical,
                ..context(RuntimePlanePolicyProfile::EspCompact)
            },
        );
        assert!(matches!(
            critical,
            RuntimeWorkDecision::Defer {
                reason: "critical_pressure",
                ..
            }
        ));
    }

    #[test]
    fn linux_full_keeps_deep_and_heavy_work_proceeding_under_foreground_overlay() {
        for class in [
            RuntimeWorkClass::DeepRouteWorker,
            RuntimeWorkClass::DisplayHeavyRefresh,
            RuntimeWorkClass::OptionalMaintenance,
        ] {
            let decision = admit_runtime_work(
                RuntimeWorkRequest::new(class, RuntimeWorkSource::Background),
                foreground_context(RuntimePlanePolicyProfile::LinuxFull),
            );
            assert_eq!(decision, RuntimeWorkDecision::Proceed);
        }
    }

    #[test]
    fn embedded_linux_keeps_linux_full_policy_until_a_separate_profile_is_proven() {
        for class in [
            RuntimeWorkClass::RealtimeVoiceSession,
            RuntimeWorkClass::DeepRouteWorker,
            RuntimeWorkClass::DisplayHeavyRefresh,
            RuntimeWorkClass::OptionalMaintenance,
        ] {
            let decision = admit_runtime_work(
                RuntimeWorkRequest::new(class, RuntimeWorkSource::Background),
                foreground_context(RuntimePlanePolicyProfile::EmbeddedLinux),
            );
            assert_eq!(decision, RuntimeWorkDecision::Proceed);
        }
    }

    #[test]
    fn scheduler_observability_records_decision_counts_and_recent_work() {
        reset_runtime_scheduler_observability_for_tests();
        let context = foreground_context(RuntimePlanePolicyProfile::EspCompact);

        assert!(matches!(
            admit_runtime_work(
                RuntimeWorkRequest::new(
                    RuntimeWorkClass::DeepRouteWorker,
                    RuntimeWorkSource::Background
                ),
                context,
            ),
            RuntimeWorkDecision::Defer {
                reason: "foreground_active",
                ..
            }
        ));
        assert_eq!(
            admit_runtime_work(
                RuntimeWorkRequest::new(
                    RuntimeWorkClass::DisplayHeavyRefresh,
                    RuntimeWorkSource::Background
                ),
                context,
            ),
            RuntimeWorkDecision::Degrade {
                reason: "foreground_active",
            }
        );
        assert_eq!(
            admit_runtime_work(
                RuntimeWorkRequest::new(
                    RuntimeWorkClass::ExternalUserMessage,
                    RuntimeWorkSource::UserFacing
                ),
                context,
            ),
            RuntimeWorkDecision::Proceed
        );

        let snapshot = runtime_scheduler_snapshot_for_context(context);
        assert!(snapshot.active_foreground);
        assert!(!snapshot.foreground_recovery_active);
        assert_eq!(
            snapshot.foreground_source,
            Some(RuntimeForegroundSource::ExternalUserMessage)
        );
        assert_eq!(snapshot.active_work, 1);
        assert!(snapshot.permits >= 1);
        assert!(snapshot.defers >= 1);
        assert!(snapshot.degrades >= 1);
        assert!(snapshot
            .recent_decisions
            .iter()
            .any(
                |record| record.request.class == RuntimeWorkClass::DeepRouteWorker
                    && matches!(
                        record.decision,
                        RuntimeWorkDecision::Defer {
                            reason: "foreground_active",
                            ..
                        }
                    )
            ));
        assert!(snapshot
            .recent_decisions
            .iter()
            .any(
                |record| record.request.class == RuntimeWorkClass::DisplayHeavyRefresh
                    && record.decision
                        == RuntimeWorkDecision::Degrade {
                            reason: "foreground_active"
                        }
            ));
        assert!(snapshot
            .recent_decisions
            .iter()
            .any(
                |record| record.request.class == RuntimeWorkClass::ExternalUserMessage
                    && record.decision == RuntimeWorkDecision::Proceed
            ));
    }

    #[test]
    fn scheduler_baseline_log_line_exposes_policy_and_decision_summary() {
        reset_runtime_scheduler_observability_for_tests();
        let context = foreground_context(RuntimePlanePolicyProfile::EspCompact);
        let _ = admit_runtime_work(
            RuntimeWorkRequest::new(
                RuntimeWorkClass::DurableWriteBack,
                RuntimeWorkSource::Background,
            ),
            context,
        );

        let line = format_baseline_log_line_for_context(context);
        assert!(line.contains("runtime_scheduler active_foreground=true"));
        assert!(line.contains("source=external_user_message"));
        assert!(line.contains("foreground_recovery_active=false"));
        assert!(line.contains("profile=esp_compact"));
        assert!(line.contains("active_work=1"));
        assert!(line.contains("defers="));
        assert!(line.contains("last_class="));
        assert!(line.contains("last_decision="));
        assert!(line.contains("last_reason="));
    }

    #[test]
    fn voice_exclusive_defers_non_voice_foreground_work_without_replacing_the_mode_truth() {
        let source = RuntimeModeSource {
            voice_exclusive_active: true,
            ..RuntimeModeSource::default()
        };
        let decision = admit_runtime_work(
            RuntimeWorkRequest::new(
                RuntimeWorkClass::ConfigUiChat,
                RuntimeWorkSource::UserFacing,
            ),
            RuntimeSchedulerContext {
                runtime_mode: snapshot_from_source(source),
                ..context(RuntimePlanePolicyProfile::EspCompact)
            },
        );

        assert!(matches!(
            decision,
            RuntimeWorkDecision::Defer {
                reason: "voice_exclusive_active",
                ..
            }
        ));
    }
}
