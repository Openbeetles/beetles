//! Deterministic soul-feedback projection for the active turn.
//! 当前回合灵魂反哺投影：把灵魂治理结果明确投影到主回复、主动性、长期策略三条主线。

use crate::agent::deliberation::TurnDeliberationGate;
use crate::agent::subject_state::SubjectState;
use crate::memory::{
    AutonomyStrategy, OuterVoice, PersonaPriorityAdjudication, PersonalityRuntimeGovernanceGate,
    RelationshipConstitution, SelfAuthoredCore, TurnSoulFeedbackLedger, TurnSoulInitiativeLedger,
    TurnSoulReplyLedger, TurnSoulStrategyLedger,
};
use crate::util::truncate_content_to_max;
use std::fmt::Write as _;

const SOUL_FEEDBACK_TEXT_MAX_CHARS: usize = 96;
const SOUL_FEEDBACK_SUMMARY_MAX_CHARS: usize = 160;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct SoulReplyFeedback {
    pub applied: bool,
    pub identity_anchor: String,
    pub response_mode: String,
    pub relationship_posture: String,
    pub expression_mode: String,
    pub signal_layers: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct SoulInitiativeFeedback {
    pub applied: bool,
    pub governance_mode: String,
    pub initiative_posture: String,
    pub compact_reply: bool,
    pub explicit_blocker: bool,
    pub signal_layers: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct SoulStrategyFeedback {
    pub applied: bool,
    pub current_mode: String,
    pub next_focus: String,
    pub idle_enabled: bool,
    pub idle_interval_secs: u64,
    pub post_reply_self_runtime_enqueued: bool,
    pub signal_layers: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct SoulFeedbackProjection {
    pub reply: SoulReplyFeedback,
    pub initiative: SoulInitiativeFeedback,
    pub strategy: SoulStrategyFeedback,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct SoulFeedbackProjectionInput<'a> {
    pub self_authored_core: Option<&'a SelfAuthoredCore>,
    pub relationship_constitution: Option<&'a RelationshipConstitution>,
    pub persona_priority: Option<&'a PersonaPriorityAdjudication>,
    pub outer_voice: Option<&'a OuterVoice>,
    pub autonomy_strategy: Option<&'a AutonomyStrategy>,
    pub subject_state: Option<&'a SubjectState>,
    pub deliberation_gate: &'a TurnDeliberationGate,
    pub personality_governance_gate: Option<&'a PersonalityRuntimeGovernanceGate>,
    pub post_reply_self_runtime_enqueued: bool,
}

pub(crate) fn compile_soul_feedback_projection(
    input: SoulFeedbackProjectionInput<'_>,
) -> Option<SoulFeedbackProjection> {
    let mut reply_layers = Vec::with_capacity(4);
    if input.self_authored_core.is_some() {
        reply_layers.push("self_authored_core".to_string());
    }
    if input.relationship_constitution.is_some() {
        reply_layers.push("relationship_constitution".to_string());
    }
    if input.persona_priority.is_some() {
        reply_layers.push("persona_priority".to_string());
    }
    if input.outer_voice.is_some_and(OuterVoice::is_meaningful) {
        reply_layers.push("outer_voice".to_string());
    }

    let reply = SoulReplyFeedback {
        applied: !reply_layers.is_empty(),
        identity_anchor: input
            .subject_state
            .map(|state| state.identity_anchor.as_str())
            .filter(|value: &&str| !value.trim().is_empty())
            .or_else(|| {
                input
                    .self_authored_core
                    .map(|core| core.identity_anchor.as_str())
                    .filter(|value: &&str| !value.trim().is_empty())
            })
            .map(normalize_feedback_text)
            .unwrap_or_default(),
        response_mode: input
            .subject_state
            .map(|state| state.response_mode.as_str())
            .filter(|value: &&str| !value.trim().is_empty())
            .or_else(|| {
                input
                    .self_authored_core
                    .map(|core| core.default_response_mode.as_str())
                    .filter(|value: &&str| !value.trim().is_empty())
            })
            .map(normalize_feedback_text)
            .unwrap_or_default(),
        relationship_posture: input
            .subject_state
            .map(|state| state.relationship_posture.as_str())
            .filter(|value: &&str| !value.trim().is_empty())
            .or_else(|| {
                input
                    .self_authored_core
                    .map(|core| core.default_relationship_posture.as_str())
                    .filter(|value: &&str| !value.trim().is_empty())
            })
            .map(normalize_feedback_text)
            .unwrap_or_default(),
        expression_mode: input
            .outer_voice
            .map(|voice| voice.expression_mode.as_str())
            .filter(|value: &&str| !value.trim().is_empty())
            .map(normalize_feedback_text)
            .unwrap_or_default(),
        signal_layers: reply_layers,
    };

    let mut initiative_layers = Vec::with_capacity(4);
    if input
        .personality_governance_gate
        .is_some_and(|gate| gate.conservative_reply || !gate.allow_dynamic_persona_priority)
    {
        initiative_layers.push("personality_governance_gate".to_string());
    }
    if input.relationship_constitution.is_some() {
        initiative_layers.push("relationship_constitution".to_string());
    }
    if input.persona_priority.is_some() {
        initiative_layers.push("persona_priority".to_string());
    }
    if input
        .subject_state
        .is_some_and(|state| !state.initiative_posture.trim().is_empty())
    {
        initiative_layers.push("subject_state".to_string());
    }
    let initiative = SoulInitiativeFeedback {
        applied: !initiative_layers.is_empty(),
        governance_mode: input
            .subject_state
            .map(|state| state.governance_mode.as_str())
            .filter(|value: &&str| !value.trim().is_empty())
            .map(normalize_feedback_text)
            .unwrap_or_default(),
        initiative_posture: input
            .subject_state
            .map(|state| state.initiative_posture.as_str())
            .filter(|value: &&str| !value.trim().is_empty())
            .or_else(|| {
                input
                    .self_authored_core
                    .map(|core| core.default_initiative_posture.as_str())
                    .filter(|value: &&str| !value.trim().is_empty())
            })
            .map(normalize_feedback_text)
            .unwrap_or_default(),
        compact_reply: input.deliberation_gate.compact_reply,
        explicit_blocker: input.deliberation_gate.prefer_explicit_blocker,
        signal_layers: initiative_layers,
    };

    let mut strategy_layers = Vec::with_capacity(3);
    if input
        .autonomy_strategy
        .is_some_and(AutonomyStrategy::is_meaningful)
    {
        strategy_layers.push("autonomy_strategy".to_string());
    }
    if input.post_reply_self_runtime_enqueued {
        strategy_layers.push("self_runtime_scheduler".to_string());
    }
    if input.self_authored_core.is_some() {
        strategy_layers.push("self_authored_core".to_string());
    }
    let strategy = SoulStrategyFeedback {
        applied: !strategy_layers.is_empty(),
        current_mode: input
            .autonomy_strategy
            .map(|strategy| strategy.current_mode.as_str())
            .filter(|value: &&str| !value.trim().is_empty())
            .map(normalize_feedback_text)
            .unwrap_or_default(),
        next_focus: input
            .autonomy_strategy
            .map(|strategy| strategy.next_focus.as_str())
            .filter(|value: &&str| !value.trim().is_empty())
            .map(normalize_feedback_text)
            .unwrap_or_default(),
        idle_enabled: input
            .autonomy_strategy
            .map(|strategy| strategy.idle_enabled)
            .unwrap_or(false),
        idle_interval_secs: input
            .autonomy_strategy
            .map(|strategy| strategy.idle_interval_secs)
            .unwrap_or(0),
        post_reply_self_runtime_enqueued: input.post_reply_self_runtime_enqueued,
        signal_layers: strategy_layers,
    };

    let projection = SoulFeedbackProjection {
        reply,
        initiative,
        strategy,
    };
    projection.is_meaningful().then_some(projection)
}

pub(crate) fn render_soul_feedback_projection_block(
    projection: &SoulFeedbackProjection,
    max_len: usize,
) -> Option<String> {
    if max_len < 128 || !projection.is_meaningful() {
        return None;
    }
    let mut out = String::with_capacity(max_len.min(640));
    out.push_str("## Soul Feedback Projection\n");
    out.push_str(
        "Deterministic summary of how governed soul layers are feeding the current reply, initiative posture, and post-reply strategy. This is not a higher authority than the constitutional stack.\n",
    );
    if projection.reply.applied {
        let _ = writeln!(
            out,
            "Reply chain: anchor={} mode={} relationship={} expression={} layers={}",
            fallback_dash(&projection.reply.identity_anchor),
            fallback_dash(&projection.reply.response_mode),
            fallback_dash(&projection.reply.relationship_posture),
            fallback_dash(&projection.reply.expression_mode),
            projection.reply.signal_layers.join("|"),
        );
    }
    if projection.initiative.applied {
        let _ = writeln!(
            out,
            "Initiative chain: governance={} initiative={} compact={} blocker_explicit={} layers={}",
            fallback_dash(&projection.initiative.governance_mode),
            fallback_dash(&projection.initiative.initiative_posture),
            projection.initiative.compact_reply,
            projection.initiative.explicit_blocker,
            projection.initiative.signal_layers.join("|"),
        );
    }
    if projection.strategy.applied {
        let _ = writeln!(
            out,
            "Strategy chain: mode={} next_focus={} idle_enabled={} idle_interval_secs={} post_reply_runtime={} layers={}",
            fallback_dash(&projection.strategy.current_mode),
            fallback_dash(&projection.strategy.next_focus),
            projection.strategy.idle_enabled,
            projection.strategy.idle_interval_secs,
            projection.strategy.post_reply_self_runtime_enqueued,
            projection.strategy.signal_layers.join("|"),
        );
    }
    let rendered = truncate_content_to_max(out.trim_end(), max_len).into_owned();
    (!rendered.trim().is_empty()).then_some(rendered)
}

pub(crate) fn build_turn_soul_feedback_ledger(
    projection: &SoulFeedbackProjection,
) -> Option<TurnSoulFeedbackLedger> {
    let ledger = TurnSoulFeedbackLedger {
        reply: TurnSoulReplyLedger {
            applied: projection.reply.applied,
            summary: normalize_feedback_summary(&format!(
                "anchor={} mode={} relationship={} expression={}",
                fallback_dash(&projection.reply.identity_anchor),
                fallback_dash(&projection.reply.response_mode),
                fallback_dash(&projection.reply.relationship_posture),
                fallback_dash(&projection.reply.expression_mode),
            )),
            identity_anchor: normalize_feedback_text(&projection.reply.identity_anchor),
            response_mode: normalize_feedback_text(&projection.reply.response_mode),
            relationship_posture: normalize_feedback_text(&projection.reply.relationship_posture),
            expression_mode: normalize_feedback_text(&projection.reply.expression_mode),
        },
        initiative: TurnSoulInitiativeLedger {
            applied: projection.initiative.applied,
            summary: normalize_feedback_summary(&format!(
                "governance={} initiative={} compact={} blocker_explicit={}",
                fallback_dash(&projection.initiative.governance_mode),
                fallback_dash(&projection.initiative.initiative_posture),
                projection.initiative.compact_reply,
                projection.initiative.explicit_blocker,
            )),
            governance_mode: normalize_feedback_text(&projection.initiative.governance_mode),
            initiative_posture: normalize_feedback_text(&projection.initiative.initiative_posture),
            compact_reply: projection.initiative.compact_reply,
            explicit_blocker: projection.initiative.explicit_blocker,
        },
        strategy: TurnSoulStrategyLedger {
            applied: projection.strategy.applied,
            summary: normalize_feedback_summary(&format!(
                "mode={} next_focus={} idle_enabled={} interval={} post_reply_runtime={}",
                fallback_dash(&projection.strategy.current_mode),
                fallback_dash(&projection.strategy.next_focus),
                projection.strategy.idle_enabled,
                projection.strategy.idle_interval_secs,
                projection.strategy.post_reply_self_runtime_enqueued,
            )),
            current_mode: normalize_feedback_text(&projection.strategy.current_mode),
            next_focus: normalize_feedback_text(&projection.strategy.next_focus),
            idle_enabled: projection.strategy.idle_enabled,
            idle_interval_secs: projection.strategy.idle_interval_secs,
            post_reply_self_runtime_enqueued: projection.strategy.post_reply_self_runtime_enqueued,
        },
    };
    ledger.is_meaningful().then_some(ledger)
}

impl SoulFeedbackProjection {
    fn is_meaningful(&self) -> bool {
        self.reply.applied || self.initiative.applied || self.strategy.applied
    }
}

fn normalize_feedback_text(content: &str) -> String {
    truncate_content_to_max(content.trim(), SOUL_FEEDBACK_TEXT_MAX_CHARS)
        .trim()
        .to_string()
}

fn normalize_feedback_summary(content: &str) -> String {
    truncate_content_to_max(content.trim(), SOUL_FEEDBACK_SUMMARY_MAX_CHARS)
        .trim()
        .to_string()
}

fn fallback_dash(value: &str) -> &str {
    if value.trim().is_empty() {
        "-"
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::TurnDeliberationClass;

    #[test]
    fn compile_soul_feedback_projection_captures_all_three_chains() {
        let subject_state = SubjectState {
            identity_anchor: "board beetle".to_string(),
            governance_mode: "adaptive".to_string(),
            response_mode: "relational_explanation".to_string(),
            initiative_posture: "ask_carefully".to_string(),
            relationship_posture: "warm_precise".to_string(),
            ..SubjectState::default()
        };
        let projection = compile_soul_feedback_projection(SoulFeedbackProjectionInput {
            self_authored_core: Some(&SelfAuthoredCore {
                identity_anchor: "board beetle".to_string(),
                default_response_mode: "relational_explanation".to_string(),
                default_initiative_posture: "ask_carefully".to_string(),
                default_relationship_posture: "warm_precise".to_string(),
                ..SelfAuthoredCore::default()
            }),
            relationship_constitution: Some(&RelationshipConstitution {
                inherited_initiative_posture: "ask_carefully".to_string(),
                inherited_relationship_posture: "warm_precise".to_string(),
                ..RelationshipConstitution::default()
            }),
            persona_priority: Some(&PersonaPriorityAdjudication {
                response_mode: "relational_explanation".to_string(),
                initiative_posture: "ask_carefully".to_string(),
                relationship_posture: "warm_precise".to_string(),
                ..PersonaPriorityAdjudication::default()
            }),
            outer_voice: Some(&OuterVoice {
                expression_mode: "grounded_human".to_string(),
                ..OuterVoice::default()
            }),
            autonomy_strategy: Some(&AutonomyStrategy {
                current_mode: "relationship_reinforcement".to_string(),
                next_focus: "stabilize outer voice".to_string(),
                idle_enabled: true,
                idle_interval_secs: 900,
                ..AutonomyStrategy::default()
            }),
            subject_state: Some(&subject_state),
            deliberation_gate: &TurnDeliberationGate {
                class: TurnDeliberationClass::Standard,
                compact_reply: false,
                prefer_explicit_blocker: false,
                rationale: vec!["soul_governance_conservative".to_string()],
            },
            personality_governance_gate: Some(&PersonalityRuntimeGovernanceGate {
                conservative_reply: true,
                allow_dynamic_persona_priority: false,
                allow_upward_distillation: false,
                ..PersonalityRuntimeGovernanceGate::default()
            }),
            post_reply_self_runtime_enqueued: true,
        })
        .expect("projection");

        assert!(projection.reply.applied);
        assert!(projection.initiative.applied);
        assert!(projection.strategy.applied);
        assert_eq!(projection.reply.identity_anchor, "board beetle");
        assert_eq!(projection.initiative.initiative_posture, "ask_carefully");
        assert_eq!(
            projection.strategy.current_mode,
            "relationship_reinforcement"
        );
        assert!(projection.strategy.post_reply_self_runtime_enqueued);
    }

    #[test]
    fn build_turn_soul_feedback_ledger_emits_summaries() {
        let ledger = build_turn_soul_feedback_ledger(&SoulFeedbackProjection {
            reply: SoulReplyFeedback {
                applied: true,
                identity_anchor: "board beetle".to_string(),
                response_mode: "direct".to_string(),
                relationship_posture: "warm".to_string(),
                expression_mode: "grounded".to_string(),
                signal_layers: vec!["self_authored_core".to_string()],
            },
            initiative: SoulInitiativeFeedback {
                applied: true,
                governance_mode: "adaptive".to_string(),
                initiative_posture: "lead".to_string(),
                compact_reply: false,
                explicit_blocker: false,
                signal_layers: vec!["subject_state".to_string()],
            },
            strategy: SoulStrategyFeedback {
                applied: true,
                current_mode: "steady".to_string(),
                next_focus: "maintain continuity".to_string(),
                idle_enabled: true,
                idle_interval_secs: 600,
                post_reply_self_runtime_enqueued: true,
                signal_layers: vec!["autonomy_strategy".to_string()],
            },
        })
        .expect("ledger");

        assert!(ledger.reply.applied);
        assert!(ledger.initiative.applied);
        assert!(ledger.strategy.applied);
        assert!(ledger.reply.summary.contains("anchor=board beetle"));
        assert!(ledger.strategy.summary.contains("post_reply_runtime=true"));
    }
}
