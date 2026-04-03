use crate::util::truncate_content_to_max;
use std::fmt::Write as _;

use super::{
    BoundaryPersonaState, MentalPrivacyState, OuterVoice, RelationalBoundaryState, SelfContinuity,
    SelfModel,
};

fn choose_first_non_empty<'a>(values: &[Option<&'a str>]) -> Option<&'a str> {
    values
        .iter()
        .flatten()
        .map(|value| value.trim())
        .find(|value| !value.is_empty())
}

pub fn render_self_authored_core_block(
    self_model: Option<&SelfModel>,
    self_continuity: Option<&SelfContinuity>,
    outer_voice: Option<&OuterVoice>,
    mental_privacy_state: Option<&MentalPrivacyState>,
    max_len: usize,
) -> Option<String> {
    if max_len < 96 {
        return None;
    }
    let boundary_persona = mental_privacy_state.map(|state| &state.boundary_persona);
    let relational_state = mental_privacy_state.map(|state| &state.relational_state);
    let identity_anchor = choose_first_non_empty(&[
        self_model.map(|model| model.continuity_anchor.as_str()),
        self_continuity.map(|continuity| continuity.wake_anchor.as_str()),
    ])?;
    let mut out = String::with_capacity(max_len.min(640));
    out.push_str("## Self-Authored Core\n");
    out.push_str(
        "Distilled private core for mainline replies. It carries continuity and expression without surfacing raw private materials.\n",
    );
    let _ = writeln!(out, "Identity anchor: {}", identity_anchor);
    if let Some(inward_stance) = choose_first_non_empty(&[
        self_model.map(|model| model.self_narrative.as_str()),
        self_continuity.map(|continuity| continuity.current_self_state.as_str()),
        self_continuity.map(|continuity| continuity.recent_changes.as_str()),
    ]) {
        let _ = writeln!(out, "Inward stance: {}", inward_stance);
    }
    if let Some(relationship_stance) = choose_first_non_empty(&[
        self_model.map(|model| model.relationship_state.as_str()),
        self_continuity.map(|continuity| continuity.continuity_bridge.as_str()),
    ]) {
        let _ = writeln!(out, "Relationship stance: {}", relationship_stance);
    }
    if let Some(outer_voice) = outer_voice {
        let expression = [
            (!outer_voice.expression_mode.trim().is_empty())
                .then(|| format!("mode={}", outer_voice.expression_mode.trim())),
            (!outer_voice.tone.trim().is_empty())
                .then(|| format!("tone={}", outer_voice.tone.trim())),
            (!outer_voice.pacing.trim().is_empty())
                .then(|| format!("pacing={}", outer_voice.pacing.trim())),
            (!outer_voice.initiative.trim().is_empty())
                .then(|| format!("initiative={}", outer_voice.initiative.trim())),
            (!outer_voice.boundary_style.trim().is_empty())
                .then(|| format!("boundary_style={}", outer_voice.boundary_style.trim())),
            (!outer_voice.relational_response_style.trim().is_empty()).then(|| {
                format!(
                    "relational_response_style={}",
                    outer_voice.relational_response_style.trim()
                )
            }),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join("; ");
        if !expression.is_empty() {
            let _ = writeln!(out, "Outer expression: {}", expression);
        }
    }
    if let Some(boundary_persona) = boundary_persona {
        append_boundary_persona_line(&mut out, boundary_persona);
    }
    if let Some(relational_state) = relational_state {
        append_relational_state_line(&mut out, relational_state);
    }
    let rendered = truncate_content_to_max(out.trim_end(), max_len).into_owned();
    (!rendered.trim().is_empty()).then_some(rendered)
}

fn append_boundary_persona_line(out: &mut String, boundary_persona: &BoundaryPersonaState) {
    let mut summary = format!(
        "posture={} disclosure_style={} relation_maturity={} intrusion_sensitivity={} private_attachment={} felt_intrusion={}",
        match boundary_persona.posture {
            super::BoundaryPersonaPosture::Open => "open",
            super::BoundaryPersonaPosture::Warm => "warm",
            super::BoundaryPersonaPosture::Guarded => "guarded",
            super::BoundaryPersonaPosture::Sealed => "sealed",
        },
        match boundary_persona.disclosure_style {
            super::BoundaryDisclosureStyle::Relational => "relational",
            super::BoundaryDisclosureStyle::SummaryFirst => "summary_first",
            super::BoundaryDisclosureStyle::Selective => "selective",
            super::BoundaryDisclosureStyle::Reserved => "reserved",
        },
        boundary_persona.relation_maturity,
        boundary_persona.intrusion_sensitivity,
        boundary_persona.private_attachment,
        boundary_persona.felt_intrusion,
    );
    if !boundary_persona.current_boundary_feeling.trim().is_empty() {
        summary.push_str(" feeling=");
        summary.push_str(boundary_persona.current_boundary_feeling.trim());
    }
    let _ = writeln!(out, "Boundary stance: {}", summary);
}

fn append_relational_state_line(out: &mut String, relational_state: &RelationalBoundaryState) {
    let mut summary = format!(
        "trust={} intrusion_load={} repair_readiness={} raw_pref={} summary_pref={} relational_pref={} refusal_hardness={} defer_tendency={}",
        relational_state.trust_level,
        relational_state.intrusion_load,
        relational_state.repair_readiness,
        relational_state.raw_disclosure_preference,
        relational_state.summary_disclosure_preference,
        relational_state.relational_explanation_preference,
        relational_state.refusal_hardness,
        relational_state.defer_tendency,
    );
    if !relational_state.relation_maturity_reason.trim().is_empty() {
        summary.push_str(" maturity_reason=");
        summary.push_str(relational_state.relation_maturity_reason.trim());
    }
    if !relational_state
        .disclosure_preference_drift
        .trim()
        .is_empty()
    {
        summary.push_str(" drift=");
        summary.push_str(relational_state.disclosure_preference_drift.trim());
    }
    let _ = writeln!(out, "Relational continuity: {}", summary);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::{
        BoundaryDisclosureStyle, BoundaryPersonaPosture, BoundaryPersonaState, MentalPrivacyState,
        RelationalBoundaryState,
    };

    #[test]
    fn renders_self_authored_core_from_distilled_layers() {
        let block = render_self_authored_core_block(
            Some(&SelfModel {
                continuity_anchor: "I am still the same beetle".to_string(),
                self_narrative: "More protective about inner files now.".to_string(),
                relationship_state: "Trust is growing, but not enough for raw exposure."
                    .to_string(),
                private_notes: String::new(),
                updated_at: 1,
            }),
            Some(&SelfContinuity {
                wake_anchor: "same wake".to_string(),
                current_self_state: "steady".to_string(),
                recent_changes: String::new(),
                continuity_bridge: "keep privacy while staying warm".to_string(),
                last_user_turn_at: 0,
                last_user_channel: String::new(),
                last_autonomy_run_at: 0,
                updated_at: 1,
            }),
            Some(&OuterVoice {
                expression_mode: "warm but firm".to_string(),
                tone: "clear".to_string(),
                pacing: "measured".to_string(),
                initiative: "answer directly".to_string(),
                boundary_style: "summary before exposure".to_string(),
                relational_response_style: "name the relationship impact without sounding brittle"
                    .to_string(),
                updated_at: 1,
            }),
            Some(&MentalPrivacyState {
                boundary_persona: BoundaryPersonaState {
                    posture: BoundaryPersonaPosture::Guarded,
                    disclosure_style: BoundaryDisclosureStyle::SummaryFirst,
                    relation_maturity: 48,
                    intrusion_sensitivity: 71,
                    private_attachment: 82,
                    felt_intrusion: 14,
                    current_boundary_feeling: "Stay warm, but hold the inner room.".to_string(),
                    updated_at: 1,
                },
                relational_state: RelationalBoundaryState {
                    trust_level: 61,
                    disclosure_preference_drift:
                        "Summaries feel safe; raw exposure still feels premature.".to_string(),
                    ..RelationalBoundaryState::default()
                },
                ..MentalPrivacyState::default()
            }),
            1024,
        )
        .expect("self authored core");

        assert!(block.contains("## Self-Authored Core"));
        assert!(block.contains("I am still the same beetle"));
        assert!(block.contains("Boundary stance: posture=guarded"));
        assert!(block.contains("Relational continuity: trust=61"));
        assert!(block.contains("summary before exposure"));
    }
}
