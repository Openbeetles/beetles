//! Structured state-machine checker contracts for P7 engineering synthesis.

use crate::error::{Error, Result};
use crate::util::truncate_content_to_max;
use serde::{Deserialize, Serialize};
use serde_json::Value;

const STATE_MACHINE_MAX_SUMMARY_CHARS: usize = 220;
const STATE_MACHINE_MAX_MACHINES: usize = 8;
const STATE_MACHINE_MAX_STATES: usize = 24;
const STATE_MACHINE_MAX_TRANSITIONS: usize = 32;
const STATE_MACHINE_MAX_FINDINGS: usize = 12;
const STATE_MACHINE_MAX_NAME_CHARS: usize = 64;
const STATE_MACHINE_MAX_TEXT_CHARS: usize = 160;
const STATE_MACHINE_MAX_GUARD_CHARS: usize = 120;
const STATE_MACHINE_MAX_REFS: usize = 8;
const STATE_MACHINE_MAX_REF_CHARS: usize = 120;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StateNodeRole {
    Initial,
    Intermediate,
    Terminal,
    Error,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StateMachineFindingKind {
    MissingTransition,
    ContradictoryTransition,
    DeadState,
    UnsafeLoop,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StateMachineState {
    pub name: String,
    pub role: StateNodeRole,
    pub summary: String,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    #[serde(default)]
    pub requires_adjudication: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StateMachineTransition {
    pub from: String,
    pub to: String,
    pub trigger: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub guard: Option<String>,
    pub summary: String,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    #[serde(default)]
    pub requires_adjudication: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StateMachineFinding {
    pub kind: StateMachineFindingKind,
    pub summary: String,
    #[serde(default)]
    pub state_refs: Vec<String>,
    #[serde(default)]
    pub transition_refs: Vec<String>,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    #[serde(default)]
    pub requires_adjudication: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StateMachineModel {
    pub name: String,
    pub summary: String,
    #[serde(default)]
    pub states: Vec<StateMachineState>,
    #[serde(default)]
    pub transitions: Vec<StateMachineTransition>,
    #[serde(default)]
    pub findings: Vec<StateMachineFinding>,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    #[serde(default)]
    pub requires_adjudication: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StateMachineResult {
    pub summary: String,
    #[serde(default)]
    pub machines: Vec<StateMachineModel>,
}

pub fn validate_state_machine_result(value: Value) -> Result<StateMachineResult> {
    let result: StateMachineResult = serde_json::from_value(value)
        .map_err(|error| Error::config("state_machine_result_decode", error.to_string()))?;
    normalize_state_machine_result(result)
}

fn normalize_state_machine_result(result: StateMachineResult) -> Result<StateMachineResult> {
    let summary = truncate_content_to_max(result.summary.trim(), STATE_MACHINE_MAX_SUMMARY_CHARS)
        .into_owned();
    if summary.is_empty() {
        return Err(Error::config(
            "state_machine_result_validate",
            "missing summary",
        ));
    }
    if result.machines.len() > STATE_MACHINE_MAX_MACHINES {
        return Err(Error::config(
            "state_machine_result_validate",
            format!("machines exceeds {}", STATE_MACHINE_MAX_MACHINES),
        ));
    }
    let machines = result
        .machines
        .into_iter()
        .map(normalize_state_machine_model)
        .collect::<Result<Vec<_>>>()?;
    Ok(StateMachineResult { summary, machines })
}

fn normalize_state_machine_model(model: StateMachineModel) -> Result<StateMachineModel> {
    if !model.requires_adjudication {
        return Err(Error::config(
            "state_machine_model_validate",
            "state machine model must require adjudication",
        ));
    }
    let name =
        truncate_content_to_max(model.name.trim(), STATE_MACHINE_MAX_NAME_CHARS).into_owned();
    let summary =
        truncate_content_to_max(model.summary.trim(), STATE_MACHINE_MAX_TEXT_CHARS).into_owned();
    if name.is_empty() || summary.is_empty() {
        return Err(Error::config(
            "state_machine_model_validate",
            "state machine model requires name and summary",
        ));
    }
    if model.states.is_empty() {
        return Err(Error::config(
            "state_machine_model_validate",
            "state machine model requires at least one state",
        ));
    }
    if model.transitions.is_empty() {
        return Err(Error::config(
            "state_machine_model_validate",
            "state machine model requires at least one transition",
        ));
    }
    if model.states.len() > STATE_MACHINE_MAX_STATES {
        return Err(Error::config(
            "state_machine_model_validate",
            format!("states exceeds {}", STATE_MACHINE_MAX_STATES),
        ));
    }
    if model.transitions.len() > STATE_MACHINE_MAX_TRANSITIONS {
        return Err(Error::config(
            "state_machine_model_validate",
            format!("transitions exceeds {}", STATE_MACHINE_MAX_TRANSITIONS),
        ));
    }
    if model.findings.len() > STATE_MACHINE_MAX_FINDINGS {
        return Err(Error::config(
            "state_machine_model_validate",
            format!("findings exceeds {}", STATE_MACHINE_MAX_FINDINGS),
        ));
    }
    let states = model
        .states
        .into_iter()
        .map(normalize_state_machine_state)
        .collect::<Result<Vec<_>>>()?;
    let state_names = states
        .iter()
        .map(|state| state.name.as_str())
        .collect::<Vec<_>>();
    let transitions = model
        .transitions
        .into_iter()
        .map(|transition| normalize_state_machine_transition(transition, &state_names))
        .collect::<Result<Vec<_>>>()?;
    let transition_refs = transitions
        .iter()
        .map(|transition| {
            format!(
                "{}->{}:{}",
                transition.from, transition.to, transition.trigger
            )
        })
        .collect::<Vec<_>>();
    let findings = model
        .findings
        .into_iter()
        .map(|finding| normalize_state_machine_finding(finding, &state_names, &transition_refs))
        .collect::<Result<Vec<_>>>()?;
    let evidence_refs = normalize_refs(
        model.evidence_refs,
        "state_machine_model_validate",
        "state machine model requires evidence_refs",
    )?;
    Ok(StateMachineModel {
        name,
        summary,
        states,
        transitions,
        findings,
        evidence_refs,
        requires_adjudication: true,
    })
}

fn normalize_state_machine_state(state: StateMachineState) -> Result<StateMachineState> {
    if !state.requires_adjudication {
        return Err(Error::config(
            "state_machine_state_validate",
            "state machine state must require adjudication",
        ));
    }
    let name =
        truncate_content_to_max(state.name.trim(), STATE_MACHINE_MAX_NAME_CHARS).into_owned();
    let summary =
        truncate_content_to_max(state.summary.trim(), STATE_MACHINE_MAX_TEXT_CHARS).into_owned();
    if name.is_empty() || summary.is_empty() {
        return Err(Error::config(
            "state_machine_state_validate",
            "state machine state requires name and summary",
        ));
    }
    let evidence_refs = normalize_refs(
        state.evidence_refs,
        "state_machine_state_validate",
        "state machine state requires evidence_refs",
    )?;
    Ok(StateMachineState {
        name,
        role: state.role,
        summary,
        evidence_refs,
        requires_adjudication: true,
    })
}

fn normalize_state_machine_transition(
    transition: StateMachineTransition,
    state_names: &[&str],
) -> Result<StateMachineTransition> {
    if !transition.requires_adjudication {
        return Err(Error::config(
            "state_machine_transition_validate",
            "state machine transition must require adjudication",
        ));
    }
    let from =
        truncate_content_to_max(transition.from.trim(), STATE_MACHINE_MAX_NAME_CHARS).into_owned();
    let to =
        truncate_content_to_max(transition.to.trim(), STATE_MACHINE_MAX_NAME_CHARS).into_owned();
    let trigger = truncate_content_to_max(transition.trigger.trim(), STATE_MACHINE_MAX_NAME_CHARS)
        .into_owned();
    let summary = truncate_content_to_max(transition.summary.trim(), STATE_MACHINE_MAX_TEXT_CHARS)
        .into_owned();
    if from.is_empty() || to.is_empty() || trigger.is_empty() || summary.is_empty() {
        return Err(Error::config(
            "state_machine_transition_validate",
            "state machine transition requires from, to, trigger, and summary",
        ));
    }
    if !state_names.contains(&from.as_str()) || !state_names.contains(&to.as_str()) {
        return Err(Error::config(
            "state_machine_transition_validate",
            "state machine transition references unknown state",
        ));
    }
    let guard = transition
        .guard
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| truncate_content_to_max(value, STATE_MACHINE_MAX_GUARD_CHARS).into_owned());
    let evidence_refs = normalize_refs(
        transition.evidence_refs,
        "state_machine_transition_validate",
        "state machine transition requires evidence_refs",
    )?;
    Ok(StateMachineTransition {
        from,
        to,
        trigger,
        guard,
        summary,
        evidence_refs,
        requires_adjudication: true,
    })
}

fn normalize_state_machine_finding(
    finding: StateMachineFinding,
    state_names: &[&str],
    known_transition_refs: &[String],
) -> Result<StateMachineFinding> {
    if !finding.requires_adjudication {
        return Err(Error::config(
            "state_machine_finding_validate",
            "state machine finding must require adjudication",
        ));
    }
    let summary =
        truncate_content_to_max(finding.summary.trim(), STATE_MACHINE_MAX_TEXT_CHARS).into_owned();
    if summary.is_empty() {
        return Err(Error::config(
            "state_machine_finding_validate",
            "state machine finding requires summary",
        ));
    }
    let state_refs = normalize_refs(
        finding.state_refs,
        "state_machine_finding_validate",
        "state machine finding requires state_refs",
    )?;
    if state_refs
        .iter()
        .any(|state| !state_names.contains(&state.as_str()))
    {
        return Err(Error::config(
            "state_machine_finding_validate",
            "state machine finding references unknown state",
        ));
    }
    let transition_refs =
        normalize_optional_refs(finding.transition_refs, "state_machine_finding_validate")?;
    let evidence_refs = normalize_refs(
        finding.evidence_refs,
        "state_machine_finding_validate",
        "state machine finding requires evidence_refs",
    )?;
    if transition_refs
        .iter()
        .any(|reference| !known_transition_refs.iter().any(|item| item == reference))
    {
        return Err(Error::config(
            "state_machine_finding_validate",
            "state machine finding references unknown transition",
        ));
    }
    Ok(StateMachineFinding {
        kind: finding.kind,
        summary,
        state_refs,
        transition_refs,
        evidence_refs,
        requires_adjudication: true,
    })
}

fn normalize_refs(
    refs: Vec<String>,
    stage: &'static str,
    missing_message: &'static str,
) -> Result<Vec<String>> {
    let refs = normalize_optional_refs(refs, stage)?;
    if refs.is_empty() {
        return Err(Error::config(stage, missing_message));
    }
    Ok(refs)
}

fn normalize_optional_refs(refs: Vec<String>, stage: &'static str) -> Result<Vec<String>> {
    if refs.len() > STATE_MACHINE_MAX_REFS {
        return Err(Error::config(
            stage,
            format!("refs exceeds {}", STATE_MACHINE_MAX_REFS),
        ));
    }
    Ok(refs
        .into_iter()
        .map(|reference| {
            truncate_content_to_max(reference.trim(), STATE_MACHINE_MAX_REF_CHARS).into_owned()
        })
        .filter(|reference| !reference.is_empty())
        .collect::<Vec<_>>())
}

#[cfg(test)]
mod tests {
    use super::*;
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
                    "evidence_refs": ["figure 2", "section 3.1"],
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

        assert_eq!(result.machines.len(), 1);
        assert_eq!(result.machines[0].states[0].role, StateNodeRole::Initial);
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
}
