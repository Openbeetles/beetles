//! Workflow-level blocker and outcome authority.
//! Centralizes clarification/blocker semantics above tool/domain adapters.

use crate::tools::{
    ToolClarificationField, ToolClarificationOption, ToolExecutionBlocker, ToolExecutionBlockerKind,
};

use super::tool_outcome::ToolFailureKind;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WorkflowClarificationOption {
    pub(crate) value: String,
    pub(crate) label: String,
}

impl From<&ToolClarificationOption> for WorkflowClarificationOption {
    fn from(value: &ToolClarificationOption) -> Self {
        Self {
            value: value.value.clone(),
            label: value.label.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WorkflowClarificationField {
    pub(crate) key: String,
    pub(crate) label: String,
    pub(crate) description: String,
    pub(crate) required: bool,
    pub(crate) secret: bool,
    pub(crate) multiple: bool,
    pub(crate) options: Vec<WorkflowClarificationOption>,
}

impl From<&ToolClarificationField> for WorkflowClarificationField {
    fn from(value: &ToolClarificationField) -> Self {
        Self {
            key: value.key.clone(),
            label: value.label.clone(),
            description: value.description.clone(),
            required: value.required,
            secret: value.secret,
            multiple: value.multiple,
            options: value
                .options
                .iter()
                .map(WorkflowClarificationOption::from)
                .collect(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub(crate) struct ClarificationRequest {
    pub(crate) fields: Vec<WorkflowClarificationField>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WorkflowBlockerKind {
    NeedsUserFacts,
    NeedsUserChoice,
    NeedsConfirmation,
    ProbeFailed,
    RuntimeBlocked,
    Unsupported,
    RetryLater,
    TaskBlocked,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WorkflowBlocker {
    pub(crate) kind: WorkflowBlockerKind,
    pub(crate) summary: String,
    pub(crate) missing_fields: Vec<String>,
    pub(crate) clarification: Option<ClarificationRequest>,
}

impl WorkflowBlocker {
    pub(crate) fn needs_user_facts(
        summary: impl Into<String>,
        missing_fields: Vec<String>,
        clarification: Option<ClarificationRequest>,
    ) -> Self {
        Self {
            kind: WorkflowBlockerKind::NeedsUserFacts,
            summary: summary.into(),
            missing_fields,
            clarification,
        }
    }

    pub(crate) fn needs_user_choice(
        summary: impl Into<String>,
        missing_fields: Vec<String>,
        clarification: Option<ClarificationRequest>,
    ) -> Self {
        Self {
            kind: WorkflowBlockerKind::NeedsUserChoice,
            summary: summary.into(),
            missing_fields,
            clarification,
        }
    }

    pub(crate) fn needs_confirmation(
        summary: impl Into<String>,
        clarification: Option<ClarificationRequest>,
    ) -> Self {
        Self {
            kind: WorkflowBlockerKind::NeedsConfirmation,
            summary: summary.into(),
            missing_fields: Vec::new(),
            clarification,
        }
    }

    pub(crate) fn probe_failed(summary: impl Into<String>) -> Self {
        Self {
            kind: WorkflowBlockerKind::ProbeFailed,
            summary: summary.into(),
            missing_fields: Vec::new(),
            clarification: None,
        }
    }

    pub(crate) fn runtime_blocked(summary: impl Into<String>) -> Self {
        Self {
            kind: WorkflowBlockerKind::RuntimeBlocked,
            summary: summary.into(),
            missing_fields: Vec::new(),
            clarification: None,
        }
    }

    pub(crate) fn unsupported(summary: impl Into<String>) -> Self {
        Self {
            kind: WorkflowBlockerKind::Unsupported,
            summary: summary.into(),
            missing_fields: Vec::new(),
            clarification: None,
        }
    }

    pub(crate) fn retry_later(summary: impl Into<String>) -> Self {
        Self {
            kind: WorkflowBlockerKind::RetryLater,
            summary: summary.into(),
            missing_fields: Vec::new(),
            clarification: None,
        }
    }

    pub(crate) fn task_blocked(summary: impl Into<String>) -> Self {
        Self {
            kind: WorkflowBlockerKind::TaskBlocked,
            summary: summary.into(),
            missing_fields: Vec::new(),
            clarification: None,
        }
    }

    pub(crate) fn outcome_kind(&self) -> WorkflowOutcomeKind {
        workflow_outcome_kind_from_blocker_kind(self.kind)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WorkflowOutcomeKind {
    Retryable,
    Permanent,
    Capability,
    NeedsUserFacts,
    NeedsUserChoice,
    NeedsConfirmation,
    ProbeFailed,
    RuntimeBlocked,
    Unsupported,
    RetryLater,
    TaskBlocked,
}

impl WorkflowOutcomeKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            WorkflowOutcomeKind::Retryable => "retryable",
            WorkflowOutcomeKind::Permanent => "permanent",
            WorkflowOutcomeKind::Capability => "capability",
            WorkflowOutcomeKind::NeedsUserFacts => "needs_user_facts",
            WorkflowOutcomeKind::NeedsUserChoice => "needs_user_choice",
            WorkflowOutcomeKind::NeedsConfirmation => "needs_confirmation",
            WorkflowOutcomeKind::ProbeFailed => "probe_failed",
            WorkflowOutcomeKind::RuntimeBlocked => "runtime_blocked",
            WorkflowOutcomeKind::Unsupported => "unsupported",
            WorkflowOutcomeKind::RetryLater => "retry_later",
            WorkflowOutcomeKind::TaskBlocked => "task_blocked",
        }
    }

    pub(crate) fn next_action_hint(self) -> &'static str {
        match self {
            WorkflowOutcomeKind::Retryable => {
                "decide whether to retry or route around the retryable blocker"
            }
            WorkflowOutcomeKind::Permanent => {
                "state the permanent blocker clearly and switch to a different approach"
            }
            WorkflowOutcomeKind::Capability => {
                "state the capability blocker clearly and switch to an alternative path"
            }
            WorkflowOutcomeKind::NeedsUserFacts => {
                "ask only for the missing user facts before continuing"
            }
            WorkflowOutcomeKind::NeedsUserChoice => {
                "ask the user to choose among the offered options before continuing"
            }
            WorkflowOutcomeKind::NeedsConfirmation => {
                "ask for an explicit user confirmation before continuing"
            }
            WorkflowOutcomeKind::ProbeFailed => {
                "explain the probe failure clearly before asking for corrected facts"
            }
            WorkflowOutcomeKind::RuntimeBlocked => {
                "state the runtime blocker clearly and wait for capability or policy recovery before continuing"
            }
            WorkflowOutcomeKind::Unsupported => {
                "state the unsupported path clearly before choosing another route"
            }
            WorkflowOutcomeKind::RetryLater => {
                "state that this workflow cannot continue yet and say what must change before retrying"
            }
            WorkflowOutcomeKind::TaskBlocked => {
                "state exactly what blocked the workflow and what concrete step can resume it"
            }
        }
    }

    pub(crate) fn requests_user_input(self) -> bool {
        matches!(
            self,
            WorkflowOutcomeKind::NeedsUserFacts
                | WorkflowOutcomeKind::NeedsUserChoice
                | WorkflowOutcomeKind::NeedsConfirmation
        )
    }
}

pub(crate) fn workflow_outcome_kind_from_tool_failure_kind(
    kind: ToolFailureKind,
) -> WorkflowOutcomeKind {
    match kind {
        ToolFailureKind::Retryable => WorkflowOutcomeKind::Retryable,
        ToolFailureKind::Permanent => WorkflowOutcomeKind::Permanent,
        ToolFailureKind::Capability => WorkflowOutcomeKind::Capability,
    }
}

pub(crate) fn workflow_outcome_kind_from_blocker_kind(
    kind: WorkflowBlockerKind,
) -> WorkflowOutcomeKind {
    match kind {
        WorkflowBlockerKind::NeedsUserFacts => WorkflowOutcomeKind::NeedsUserFacts,
        WorkflowBlockerKind::NeedsUserChoice => WorkflowOutcomeKind::NeedsUserChoice,
        WorkflowBlockerKind::NeedsConfirmation => WorkflowOutcomeKind::NeedsConfirmation,
        WorkflowBlockerKind::ProbeFailed => WorkflowOutcomeKind::ProbeFailed,
        WorkflowBlockerKind::RuntimeBlocked => WorkflowOutcomeKind::RuntimeBlocked,
        WorkflowBlockerKind::Unsupported => WorkflowOutcomeKind::Unsupported,
        WorkflowBlockerKind::RetryLater => WorkflowOutcomeKind::RetryLater,
        WorkflowBlockerKind::TaskBlocked => WorkflowOutcomeKind::TaskBlocked,
    }
}

pub(crate) fn workflow_blocker_from_tool_blocker(
    blocker: &ToolExecutionBlocker,
) -> WorkflowBlocker {
    let clarification = (!blocker.clarification_fields.is_empty()).then(|| ClarificationRequest {
        fields: blocker
            .clarification_fields
            .iter()
            .map(WorkflowClarificationField::from)
            .collect(),
    });
    match blocker.kind {
        ToolExecutionBlockerKind::NeedsUserFacts => WorkflowBlocker::needs_user_facts(
            blocker.summary.clone(),
            blocker.missing_fields.clone(),
            clarification,
        ),
        ToolExecutionBlockerKind::NeedsUserChoice => WorkflowBlocker::needs_user_choice(
            blocker.summary.clone(),
            blocker.missing_fields.clone(),
            clarification,
        ),
        ToolExecutionBlockerKind::ProbeFailed => {
            WorkflowBlocker::probe_failed(blocker.summary.clone())
        }
        ToolExecutionBlockerKind::RuntimeBlocked => {
            WorkflowBlocker::runtime_blocked(blocker.summary.clone())
        }
        ToolExecutionBlockerKind::Unsupported => {
            WorkflowBlocker::unsupported(blocker.summary.clone())
        }
    }
}

pub(crate) fn parse_workflow_outcome_kind(label: &str) -> Option<WorkflowOutcomeKind> {
    match label.trim() {
        "retryable" => Some(WorkflowOutcomeKind::Retryable),
        "permanent" => Some(WorkflowOutcomeKind::Permanent),
        "capability" => Some(WorkflowOutcomeKind::Capability),
        "needs_user_facts" => Some(WorkflowOutcomeKind::NeedsUserFacts),
        "needs_user_choice" => Some(WorkflowOutcomeKind::NeedsUserChoice),
        "needs_confirmation" => Some(WorkflowOutcomeKind::NeedsConfirmation),
        "probe_failed" => Some(WorkflowOutcomeKind::ProbeFailed),
        "runtime_blocked" => Some(WorkflowOutcomeKind::RuntimeBlocked),
        "unsupported" => Some(WorkflowOutcomeKind::Unsupported),
        "retry_later" => Some(WorkflowOutcomeKind::RetryLater),
        "task_blocked" => Some(WorkflowOutcomeKind::TaskBlocked),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workflow_blocker_adapter_preserves_clarification_request() {
        let tool_blocker = ToolExecutionBlocker::needs_user_facts(
            "missing account identity",
            vec!["identity_class".to_string()],
            vec![ToolClarificationField {
                key: "identity_class".to_string(),
                label: "Identity class".to_string(),
                description: "Which identity bucket should this account use?".to_string(),
                required: true,
                secret: false,
                multiple: false,
                options: vec![
                    ToolClarificationOption {
                        value: "work".to_string(),
                        label: "Work".to_string(),
                    },
                    ToolClarificationOption {
                        value: "personal".to_string(),
                        label: "Personal".to_string(),
                    },
                ],
            }],
        );

        let workflow_blocker = workflow_blocker_from_tool_blocker(&tool_blocker);

        assert_eq!(workflow_blocker.kind, WorkflowBlockerKind::NeedsUserFacts);
        assert_eq!(workflow_blocker.missing_fields, vec!["identity_class"]);
        let clarification = workflow_blocker
            .clarification
            .as_ref()
            .expect("workflow clarification");
        assert_eq!(clarification.fields.len(), 1);
        assert_eq!(clarification.fields[0].key, "identity_class");
        assert_eq!(clarification.fields[0].options.len(), 2);
    }

    #[test]
    fn task_blocked_kind_maps_to_shared_workflow_taxonomy() {
        let kind = workflow_outcome_kind_from_blocker_kind(WorkflowBlockerKind::TaskBlocked);
        assert_eq!(kind.as_str(), "task_blocked");
        assert_eq!(
            kind.next_action_hint(),
            "state exactly what blocked the workflow and what concrete step can resume it"
        );
        assert_eq!(parse_workflow_outcome_kind("task_blocked"), Some(kind));
    }
}
