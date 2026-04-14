use beetle::{validate_state_machine_result, StateMachineFindingKind, StateNodeRole};
use serde_json::json;

#[test]
fn validate_state_machine_result_accepts_adjudicated_models() {
    let result = validate_state_machine_result(json!({
        "summary": "Checked one boot state machine.",
        "machines": [{
            "name": "boot_flow",
            "summary": "Boot state progression from reset to ready.",
            "states": [
                {
                    "name": "RESET",
                    "role": "initial",
                    "summary": "Power-on reset state.",
                    "evidence_refs": ["figure 2"],
                    "requires_adjudication": true
                },
                {
                    "name": "READY",
                    "role": "terminal",
                    "summary": "Ready for commands.",
                    "evidence_refs": ["figure 2"],
                    "requires_adjudication": true
                }
            ],
            "transitions": [{
                "from": "RESET",
                "to": "READY",
                "trigger": "init_complete",
                "summary": "Initialization finishes successfully.",
                "evidence_refs": ["section 3.1"],
                "requires_adjudication": true
            }],
            "findings": [{
                "kind": "unsafe_loop",
                "summary": "Review whether retries can loop forever before READY.",
                "state_refs": ["RESET"],
                "transition_refs": ["RESET->READY:init_complete"],
                "evidence_refs": ["section 3.1"],
                "requires_adjudication": true
            }],
            "evidence_refs": ["figure 2", "section 3.1"],
            "requires_adjudication": true
        }]
    }))
    .expect("valid state machine result");

    assert_eq!(result.machines[0].states[0].role, StateNodeRole::Initial);
    assert_eq!(
        result.machines[0].findings[0].kind,
        StateMachineFindingKind::UnsafeLoop
    );
}

#[test]
fn validate_state_machine_result_rejects_non_adjudicated_transition() {
    let error = validate_state_machine_result(json!({
        "summary": "bad",
        "machines": [{
            "name": "boot_flow",
            "summary": "Boot state progression.",
            "states": [
                {
                    "name": "RESET",
                    "role": "initial",
                    "summary": "Reset.",
                    "evidence_refs": ["figure 2"],
                    "requires_adjudication": true
                },
                {
                    "name": "READY",
                    "role": "terminal",
                    "summary": "Ready.",
                    "evidence_refs": ["figure 2"],
                    "requires_adjudication": true
                }
            ],
            "transitions": [{
                "from": "RESET",
                "to": "READY",
                "trigger": "init_complete",
                "summary": "Initialization completes.",
                "evidence_refs": ["section 3.1"],
                "requires_adjudication": false
            }],
            "evidence_refs": ["figure 2"],
            "requires_adjudication": true
        }]
    }))
    .expect_err("transition should be rejected");

    assert!(error
        .to_string()
        .contains("state machine transition must require adjudication"));
}
