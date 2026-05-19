use crate::bus::IngressKind;
use crate::memory::{
    PersonalityRuntimeGovernanceGate, RecallSelectionReport, TurnDeliberationClass,
    TurnObservationLedger,
};
use crate::orchestrator::PressureLevel;
use crate::runtime::{RuntimeMode, RuntimeModeSnapshot};
use crate::util::truncate_content_to_max;

use super::{request_semantics::request_shape_metrics, strategy::AgentRunStrategy};

const TURN_DELIBERATION_TEXT_MAX_CHARS: usize = 360;
const FAST_USER_MAX_CHARS: usize = 48;
const HARD_USER_MIN_CHARS: usize = 96;
const HARD_SEPARATOR_MIN: usize = 4;
const HARD_LINE_MIN: usize = 3;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TurnDeliberationGate {
    pub(crate) class: TurnDeliberationClass,
    pub(crate) compact_reply: bool,
    pub(crate) prefer_explicit_blocker: bool,
    pub(crate) rationale: Vec<String>,
}

pub(crate) struct TurnDeliberationInput<'a> {
    pub(crate) strategy: AgentRunStrategy,
    pub(crate) ingress: IngressKind,
    pub(crate) is_group: bool,
    pub(crate) user_content: &'a str,
    pub(crate) has_tools: bool,
    pub(crate) pressure: PressureLevel,
    pub(crate) runtime_mode: RuntimeModeSnapshot,
    pub(crate) recent_observation: Option<&'a TurnObservationLedger>,
    pub(crate) execution_state_text: Option<&'a str>,
    pub(crate) shared_factual_report: &'a RecallSelectionReport,
    pub(crate) continuity_capsule_report: &'a RecallSelectionReport,
    pub(crate) archive_report: &'a RecallSelectionReport,
    pub(crate) runtime_skill_report: &'a RecallSelectionReport,
    pub(crate) task_recall_report: Option<&'a RecallSelectionReport>,
    pub(crate) personality_governance_gate: Option<&'a PersonalityRuntimeGovernanceGate>,
}

pub(crate) fn compile_turn_deliberation_gate(
    input: TurnDeliberationInput<'_>,
) -> TurnDeliberationGate {
    let mut rationale = Vec::with_capacity(4);
    let runtime_restricted = input.pressure != PressureLevel::Normal
        || input.runtime_mode.current_mode != RuntimeMode::Normal;
    if runtime_restricted {
        rationale.push(format!(
            "runtime={} pressure={:?}",
            input.runtime_mode.current_mode.as_str(),
            input.pressure
        ));
    }
    let soul_governance_conservative = input
        .personality_governance_gate
        .is_some_and(|gate| gate.conservative_reply);
    if soul_governance_conservative {
        rationale.push("soul_governance_conservative".to_string());
    }
    let prior_blocker = input
        .recent_observation
        .and_then(|observation| observation.blocker.as_ref())
        .is_some();
    if prior_blocker {
        rationale.push("recent_blocker_or_recovery".to_string());
    }
    let recall_dispersion = recall_dispersion_count(&input) >= 2;
    if recall_dispersion {
        rationale.push("recall_dispersion".to_string());
    }
    let dense_working_set = execution_state_working_set_sections(input.execution_state_text) >= 2;
    if dense_working_set {
        rationale.push("working_set_dense".to_string());
    }
    let complex_request = request_looks_complex(input.user_content);
    if complex_request {
        rationale.push("request_complexity".to_string());
    }

    let class = if input.strategy == AgentRunStrategy::LinuxEnhanced
        && input.ingress == IngressKind::User
        && !input.is_group
        && !runtime_restricted
        && ((prior_blocker && (recall_dispersion || dense_working_set || complex_request))
            || (complex_request && recall_dispersion)
            || (complex_request && dense_working_set && input.has_tools))
    {
        TurnDeliberationClass::HardReasoning
    } else if input.ingress == IngressKind::User
        && !input.is_group
        && !input.has_tools
        && !prior_blocker
        && !runtime_restricted
        && !recall_dispersion
        && !dense_working_set
        && request_looks_fast(input.user_content)
    {
        TurnDeliberationClass::FastInteractive
    } else {
        TurnDeliberationClass::Standard
    };

    TurnDeliberationGate {
        class,
        compact_reply: runtime_restricted
            || soul_governance_conservative
            || class == TurnDeliberationClass::FastInteractive,
        prefer_explicit_blocker: runtime_restricted
            || soul_governance_conservative
            || prior_blocker,
        rationale,
    }
}

pub(crate) fn render_turn_deliberation_gate_block(
    gate: &TurnDeliberationGate,
    max_len: usize,
) -> Option<String> {
    if max_len < 96 {
        return None;
    }
    let reply_budget = if gate.compact_reply {
        "compact"
    } else if gate.class == TurnDeliberationClass::HardReasoning {
        "deliberate"
    } else {
        "standard"
    };
    let mut out = String::with_capacity(max_len.min(512));
    out.push_str("## Turn Deliberation Gate\n");
    out.push_str("Class: ");
    out.push_str(gate.class.label());
    out.push('\n');
    out.push_str("Reply budget: ");
    out.push_str(reply_budget);
    out.push('\n');
    out.push_str("Blocker posture: ");
    out.push_str(if gate.prefer_explicit_blocker {
        "explicit"
    } else {
        "normal"
    });
    out.push('\n');
    out.push_str("Guidance: ");
    out.push_str(match gate.class {
        TurnDeliberationClass::FastInteractive => {
            "answer directly if current context is sufficient; avoid over-analysis"
        }
        TurnDeliberationClass::Standard => {
            "solve the current request with the lightest sufficient path"
        }
        TurnDeliberationClass::HardReasoning => {
            "reconcile task context, evidence, and blocker history before committing"
        }
    });
    if !gate.rationale.is_empty() {
        out.push('\n');
        out.push_str("Signals: ");
        out.push_str(&gate.rationale.join(" | "));
    }
    let rendered = truncate_content_to_max(
        out.trim_end(),
        max_len.min(TURN_DELIBERATION_TEXT_MAX_CHARS),
    )
    .into_owned();
    (!rendered.trim().is_empty()).then_some(rendered)
}

fn request_looks_complex(content: &str) -> bool {
    let metrics = request_shape_metrics(content);
    metrics.char_count >= HARD_USER_MIN_CHARS
        || metrics.line_count >= HARD_LINE_MIN
        || metrics.separator_count >= HARD_SEPARATOR_MIN
}

fn request_looks_fast(content: &str) -> bool {
    let metrics = request_shape_metrics(content);
    metrics.char_count <= FAST_USER_MAX_CHARS
        && metrics.line_count <= 1
        && metrics.separator_count <= 1
}

fn execution_state_working_set_sections(execution_state_text: Option<&str>) -> usize {
    let Some(text) = execution_state_text else {
        return 0;
    };
    text.lines()
        .filter(|line| {
            line.starts_with("Constraints:")
                || line.starts_with("Open questions:")
                || line.starts_with("Observations:")
                || line.starts_with("Next best actions:")
        })
        .count()
}

fn recall_dispersion_count(input: &TurnDeliberationInput<'_>) -> usize {
    [
        input.shared_factual_report.selected_count > 0,
        input.continuity_capsule_report.selected_count > 0,
        input.archive_report.selected_count > 0,
        input.runtime_skill_report.selected_count > 0,
        input
            .task_recall_report
            .is_some_and(|report| report.selected_count > 0),
    ]
    .into_iter()
    .filter(|selected| *selected)
    .count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::{RecallPlane, RecallQuery, TurnBlockerLedger, TurnModeSnapshotLedger};
    use crate::runtime::{RuntimeMode, RuntimeModeActionBudget};

    fn runtime_mode(mode: RuntimeMode) -> RuntimeModeSnapshot {
        RuntimeModeSnapshot {
            current_mode: mode,
            wifi_sta_connected: true,
            boot_phase_active: false,
            pairing_required: false,
            pairing_state_known: false,
            voice_exclusive_active: false,
            background_maintenance_active: false,
            config_plane_alive: false,
            config_active: mode == RuntimeMode::ConfigActive,
            config_activity_phase: if mode == RuntimeMode::ConfigActive {
                crate::runtime::ConfigActivityPhase::Active
            } else {
                crate::runtime::ConfigActivityPhase::Idle
            },
            channel_plane_alive: true,
            voice_plane_alive: false,
            agent_plane_alive: true,
            external_wss_managed_present: false,
            external_wss_suspend_requested: false,
            external_wss_suspended: false,
            recovery_safe_mode_active: mode == RuntimeMode::RecoverySafeMode,
            runtime_foreground: crate::runtime::RuntimeForegroundOverlay::default(),
            action_budget: RuntimeModeActionBudget {
                allow_periodic_maintenance: true,
                allow_due_user_timers: true,
                allow_heartbeat_injection: true,
                allow_best_effort_delayed_tasks: true,
                allow_idle_self_runtime: mode == RuntimeMode::Normal,
                allow_non_voice_outbound: !matches!(
                    mode,
                    RuntimeMode::VoiceExclusive | RuntimeMode::ConfigActive
                ),
                allow_realtime_voice_connect: mode != RuntimeMode::ConfigActive,
                allow_external_wss_connect: mode == RuntimeMode::Normal,
                require_external_wss_suspended: mode == RuntimeMode::VoiceExclusive,
            },
        }
    }

    fn report(selected_count: usize) -> RecallSelectionReport {
        RecallSelectionReport {
            plane: RecallPlane::Archive,
            query: RecallQuery {
                plane: RecallPlane::Archive,
                ..RecallQuery::default()
            },
            backend: "test".to_string(),
            candidate_count: selected_count,
            selected_count,
            selected_ids: Vec::new(),
            miss_reason: None,
            selection_note: None,
            candidates: Vec::new(),
        }
    }

    #[test]
    fn compile_turn_deliberation_gate_marks_hard_reasoning_for_blocker_and_dispersion() {
        let gate = compile_turn_deliberation_gate(TurnDeliberationInput {
            strategy: AgentRunStrategy::LinuxEnhanced,
            ingress: IngressKind::User,
            is_group: false,
            user_content: "请基于当前上下文给我一个完整方案，并说明取舍、风险和下一步。",
            has_tools: true,
            pressure: PressureLevel::Normal,
            runtime_mode: runtime_mode(RuntimeMode::Normal),
            recent_observation: Some(&TurnObservationLedger {
                execution_class: crate::memory::TurnExecutionClass::ToolAssisted,
                deliberation_class: TurnDeliberationClass::Standard,
                final_outcome: "surface_finalization".to_string(),
                pressure: crate::memory::TurnPersonaPressureLevel::Cautious,
                mode: TurnModeSnapshotLedger {
                    current_mode: "normal".to_string(),
                    allow_non_voice_outbound: true,
                    allow_idle_self_runtime: true,
                },
                tool_path: crate::memory::TurnToolPathLedger {
                    path: "surface_finalization".to_string(),
                    tool_calls: 2,
                    react_rounds: 3,
                    current_primary_delivered: false,
                },
                blocker: Some(TurnBlockerLedger {
                    kind: "retryable".to_string(),
                    failed_calls: 2,
                    total_calls: 2,
                }),
            }),
            execution_state_text: Some(
                "## Execution State\nConstraints: 不能新开 store\nOpen questions: blocker 是否要前移\nObservations: archive/skill 都命中\nNext best actions: 先收敛 hard turn gate",
            ),
            shared_factual_report: &report(1),
            continuity_capsule_report: &report(1),
            archive_report: &report(1),
            runtime_skill_report: &report(0),
            task_recall_report: Some(&report(1)),
            personality_governance_gate: None,
        });

        assert_eq!(gate.class, TurnDeliberationClass::HardReasoning);
        assert!(gate.prefer_explicit_blocker);
    }

    #[test]
    fn compile_turn_deliberation_gate_compacts_reply_under_recovery_mode() {
        let gate = compile_turn_deliberation_gate(TurnDeliberationInput {
            strategy: AgentRunStrategy::LinuxEnhanced,
            ingress: IngressKind::User,
            is_group: false,
            user_content: "帮我继续处理一下",
            has_tools: true,
            pressure: PressureLevel::Cautious,
            runtime_mode: runtime_mode(RuntimeMode::RecoverySafeMode),
            recent_observation: None,
            execution_state_text: None,
            shared_factual_report: &report(0),
            continuity_capsule_report: &report(0),
            archive_report: &report(0),
            runtime_skill_report: &report(0),
            task_recall_report: None,
            personality_governance_gate: None,
        });

        assert_eq!(gate.class, TurnDeliberationClass::Standard);
        assert!(gate.compact_reply);
        assert!(gate.prefer_explicit_blocker);
    }

    #[test]
    fn compile_turn_deliberation_gate_respects_soul_governance_conservative_mode() {
        let gate = compile_turn_deliberation_gate(TurnDeliberationInput {
            strategy: AgentRunStrategy::LinuxEnhanced,
            ingress: IngressKind::User,
            is_group: false,
            user_content: "继续处理",
            has_tools: true,
            pressure: PressureLevel::Normal,
            runtime_mode: runtime_mode(RuntimeMode::Normal),
            recent_observation: None,
            execution_state_text: None,
            shared_factual_report: &report(0),
            continuity_capsule_report: &report(0),
            archive_report: &report(0),
            runtime_skill_report: &report(0),
            task_recall_report: None,
            personality_governance_gate: Some(&PersonalityRuntimeGovernanceGate {
                conservative_reply: true,
                allow_dynamic_persona_priority: false,
                allow_upward_distillation: false,
                reason_summary: "board review due".to_string(),
                ..PersonalityRuntimeGovernanceGate::default()
            }),
        });

        assert!(gate.compact_reply);
        assert!(gate.prefer_explicit_blocker);
        assert!(gate
            .rationale
            .iter()
            .any(|reason| reason == "soul_governance_conservative"));
    }
}
