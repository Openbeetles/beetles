use crate::diagnosis::{
    DiagnosisAction, DiagnosisConfidence, DiagnosisDegradation, DiagnosisEvidence,
    DiagnosisFinding, DiagnosisKind, DiagnosisResult, DiagnosisRootCause,
};
use crate::platform::memory_operator_surface::{
    build_memory_operator_surface_with_capabilities, MemoryOperatorSurfaceSummary,
};

pub fn build_memory_runtime_diagnosis(
    surface: &MemoryOperatorSurfaceSummary,
) -> DiagnosisResult {
    let gate = &surface.policy_view.runtime_governance_gate;
    let mut findings = Vec::new();
    let mut suspected_root_causes = Vec::new();
    let mut recommended_next_steps = Vec::new();
    let mut degraded_by = Vec::new();
    let mut evidence = vec![
        DiagnosisEvidence::new(
            "memory_system_kind",
            surface.inspect.memory_system_kind.as_str(),
        ),
        DiagnosisEvidence::new(
            "runtime_skill_count",
            surface.inspect.runtime_skill_count.to_string(),
        ),
        DiagnosisEvidence::new("long_term_count", surface.inspect.long_term_count.to_string()),
        DiagnosisEvidence::new(
            "continuity_capsule_count",
            surface.inspect.continuity_capsule_count.to_string(),
        ),
        DiagnosisEvidence::new(
            "continuity_snapshot_supported",
            surface.inspect.continuity_snapshot_supported.to_string(),
        ),
        DiagnosisEvidence::new("repair_needed", surface.repair.repair_needed.to_string()),
        DiagnosisEvidence::new("primary_action", surface.repair.primary_action.as_str()),
        DiagnosisEvidence::new("board_review_due", surface.diff.board_review_due.to_string()),
        DiagnosisEvidence::new(
            "relationship_needs_runtime_attention",
            surface.diff.relationship_needs_runtime_attention.to_string(),
        ),
        DiagnosisEvidence::new(
            "drift_flag_count",
            surface.diff.drift_flags.len().to_string(),
        ),
        DiagnosisEvidence::new(
            "outstanding_count",
            surface.diff.outstanding.len().to_string(),
        ),
        DiagnosisEvidence::new(
            "conservative_reply",
            gate.conservative_reply.to_string(),
        ),
        DiagnosisEvidence::new(
            "allow_dynamic_persona_priority",
            gate.allow_dynamic_persona_priority.to_string(),
        ),
        DiagnosisEvidence::new(
            "allow_upward_distillation",
            gate.allow_upward_distillation.to_string(),
        ),
        DiagnosisEvidence::new(
            "forge_attack_findings",
            surface.forge.attack_findings.to_string(),
        ),
        DiagnosisEvidence::new(
            "forge_distillation_candidates",
            surface.forge.distillation_candidates.to_string(),
        ),
    ];
    let mut confidence = DiagnosisConfidence::Medium;
    let mut summary = "The current memory runtime snapshot looks generally stable.".to_string();

    if let Some(target) = surface.inspect.active_relationship_target.as_ref() {
        evidence.push(DiagnosisEvidence::new(
            "active_relationship_target",
            format!("{}:{}", target.channel, target.chat_id),
        ));
    }

    if surface.repair.repair_needed {
        summary = format!(
            "Memory runtime governance currently recommends {}.",
            surface.repair.primary_action
        );
        confidence = DiagnosisConfidence::High;
        findings.push(DiagnosisFinding::observed(
            "memory governance repair is currently required",
        ));
        suspected_root_causes.push(DiagnosisRootCause::new(
            "memory_runtime_repair_needed",
            "memory runtime governance has an active repair plan for the current board or relationship state",
            DiagnosisConfidence::High,
        ));
        recommended_next_steps.push(DiagnosisAction::new(
            "inspect_memory_status",
            "inspect memory operator status, repair reasons, and active relationship targets",
        ));
        recommended_next_steps.push(DiagnosisAction::new(
            "inspect_personality_governance",
            "inspect personality governance closure and outstanding repair reasons before blaming recall quality",
        ));
    }

    if surface.diff.board_review_due
        || surface.diff.relationship_needs_runtime_attention
        || !surface.diff.drift_flags.is_empty()
        || !surface.diff.outstanding.is_empty()
    {
        findings.push(DiagnosisFinding::correlated(
            "memory governance drift or outstanding closure work is present",
        ));
        suspected_root_causes.push(DiagnosisRootCause::new(
            "memory_governance_drift",
            "review debt, relationship drift, or unresolved closure work is affecting memory runtime stability",
            DiagnosisConfidence::Medium,
        ));
        recommended_next_steps.push(DiagnosisAction::new(
            "inspect_memory_governance_drift",
            "review drift flags, outstanding closure items, and relationship runtime attention markers",
        ));
    }

    if gate.conservative_reply
        || !gate.allow_dynamic_persona_priority
        || !gate.allow_upward_distillation
    {
        findings.push(DiagnosisFinding::correlated(
            "runtime personality governance is currently conservative",
        ));
        suspected_root_causes.push(DiagnosisRootCause::new(
            "runtime_governance_guarded",
            "runtime governance is intentionally guarded, which can reduce adaptive memory behavior until repairs settle",
            DiagnosisConfidence::Medium,
        ));
        recommended_next_steps.push(DiagnosisAction::new(
            "inspect_runtime_governance_gate",
            "inspect runtime governance gate reasons and outstanding items before treating behavior as model failure",
        ));
    }

    if surface.inspect.long_term_count == 0
        && surface.inspect.continuity_capsule_count == 0
        && surface.inspect.runtime_skill_count == 0
    {
        findings.push(DiagnosisFinding::suspected(
            "persistent memory state is currently sparse",
        ));
        suspected_root_causes.push(DiagnosisRootCause::new(
            "sparse_persistent_memory",
            "persistent memory stores are still sparse, so recall depth may be limited even when runtime is healthy",
            DiagnosisConfidence::Low,
        ));
        recommended_next_steps.push(DiagnosisAction::new(
            "inspect_memory_population",
            "confirm whether the board is freshly initialized or whether memory persistence is unexpectedly empty",
        ));
    }

    if !surface.inspect.continuity_snapshot_supported {
        degraded_by.push(DiagnosisDegradation::new(
            "continuity_snapshot_tooling_unavailable",
            "continuity snapshot tooling is unavailable, which reduces repair and replay inspection depth",
        ));
    }

    if surface.forge.attack_findings > 0 || surface.forge.distillation_candidates > 0 {
        findings.push(DiagnosisFinding::observed(
            "memory forge has recent findings or distillation candidates pending review",
        ));
        recommended_next_steps.push(DiagnosisAction::new(
            "inspect_memory_forge",
            "review recent memory forge findings and distillation candidates if learning drift is suspected",
        ));
    }

    let mut safe_actions_available = vec![
        "inspect_memory_status".to_string(),
        "inspect_personality_governance".to_string(),
        "inspect_runtime_governance_gate".to_string(),
        "inspect_memory_forge".to_string(),
    ];
    if surface.inspect.continuity_snapshot_supported {
        safe_actions_available.push("inspect_continuity_snapshot".to_string());
    }

    DiagnosisResult {
        kind: DiagnosisKind::MemoryRuntime,
        summary,
        findings,
        suspected_root_causes,
        recommended_next_steps,
        evidence,
        confidence,
        degraded_by,
        safe_actions_available,
    }
}

pub fn build_memory_runtime_diagnosis_from_runtime(
    platform: &dyn crate::Platform,
    continuity_snapshot_supported: bool,
) -> crate::Result<DiagnosisResult> {
    let surface = build_memory_operator_surface_with_capabilities(
        platform,
        continuity_snapshot_supported,
        None,
    )?;
    Ok(build_memory_runtime_diagnosis(&surface))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::{
        PersonalityGovernanceInspection, PersonalityGovernanceRepairPlan,
        PersonalityRuntimeGovernanceGate,
    };
    use crate::platform::memory_operator_surface::{
        MemoryOperatorDiffView, MemoryOperatorForgeView, MemoryOperatorInspectView,
        MemoryOperatorPolicyView, MemoryOperatorRepairView, MemoryOperatorSurfaceSummary,
    };

    #[test]
    fn memory_runtime_diagnosis_marks_repair_needed_as_root_cause() {
        let diagnosis = build_memory_runtime_diagnosis(&MemoryOperatorSurfaceSummary {
            inspect: MemoryOperatorInspectView {
                memory_system_kind: "linux_full".to_string(),
                continuity_snapshot_supported: true,
                ..MemoryOperatorInspectView::default()
            },
            repair: MemoryOperatorRepairView {
                repair_needed: true,
                primary_action: "repair_self_authored_core".to_string(),
                continuity_snapshot_supported: true,
                reasons: vec!["board_core_review_due".to_string()],
                ..MemoryOperatorRepairView::default()
            },
            policy_view: MemoryOperatorPolicyView {
                personality_governance: PersonalityGovernanceInspection::default(),
                runtime_governance_gate: PersonalityRuntimeGovernanceGate {
                    repair_plan: PersonalityGovernanceRepairPlan {
                        repair_needed: true,
                        primary_action:
                            crate::memory::PersonalityGovernanceRepairAction::RepairSelfAuthoredCore,
                        ..PersonalityGovernanceRepairPlan::default()
                    },
                    ..PersonalityRuntimeGovernanceGate::default()
                },
                ..MemoryOperatorPolicyView::default()
            },
            ..MemoryOperatorSurfaceSummary::default()
        });

        assert_eq!(diagnosis.kind, DiagnosisKind::MemoryRuntime);
        assert!(diagnosis
            .suspected_root_causes
            .iter()
            .any(|cause| cause.code == "memory_runtime_repair_needed"));
    }

    #[test]
    fn memory_runtime_diagnosis_reports_governance_drift_and_sparse_state() {
        let diagnosis = build_memory_runtime_diagnosis(&MemoryOperatorSurfaceSummary {
            inspect: MemoryOperatorInspectView {
                memory_system_kind: "linux_full".to_string(),
                continuity_snapshot_supported: false,
                ..MemoryOperatorInspectView::default()
            },
            diff: MemoryOperatorDiffView {
                board_review_due: true,
                relationship_needs_runtime_attention: true,
                drift_flags: vec!["persona_drift".to_string()],
                outstanding: vec!["governance_review_due".to_string()],
                ..MemoryOperatorDiffView::default()
            },
            forge: MemoryOperatorForgeView {
                attack_findings: 1,
                distillation_candidates: 2,
                ..MemoryOperatorForgeView::default()
            },
            policy_view: MemoryOperatorPolicyView {
                personality_governance: PersonalityGovernanceInspection::default(),
                runtime_governance_gate: PersonalityRuntimeGovernanceGate {
                    conservative_reply: true,
                    allow_dynamic_persona_priority: false,
                    allow_upward_distillation: false,
                    ..PersonalityRuntimeGovernanceGate::default()
                },
                ..MemoryOperatorPolicyView::default()
            },
            ..MemoryOperatorSurfaceSummary::default()
        });

        assert!(diagnosis
            .suspected_root_causes
            .iter()
            .any(|cause| cause.code == "memory_governance_drift"));
        assert!(diagnosis
            .suspected_root_causes
            .iter()
            .any(|cause| cause.code == "runtime_governance_guarded"));
        assert!(diagnosis
            .suspected_root_causes
            .iter()
            .any(|cause| cause.code == "sparse_persistent_memory"));
        assert!(!diagnosis.degraded_by.is_empty());
    }
}
