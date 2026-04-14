use crate::error::{Error, Result};
use crate::office::{OfficeAccountAssessment, OfficeCapability};
use crate::tools::{serialize_tool_output, ToolExecutionFailureKind, ToolExecutionOutcome};
use serde::Serialize;

#[derive(Serialize)]
struct OfficeOperationFailureResponse {
    op: String,
    ok: bool,
    error: String,
    error_stage: String,
    failure_kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    account_key: Option<String>,
    office_assessment: OfficeOperationAssessmentHint,
}

#[derive(Serialize)]
struct OfficeOperationAssessmentHint {
    capability: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    default_account_key: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    account_assessments: Vec<OfficeAccountAssessment>,
}

pub(crate) struct OfficeOperationFailureInput<'a> {
    pub(crate) stage: &'static str,
    pub(crate) op: &'a str,
    pub(crate) provider: Option<&'a str>,
    pub(crate) account_key: Option<&'a str>,
    pub(crate) capability: OfficeCapability,
    pub(crate) default_account_key: Option<String>,
    pub(crate) account_assessments: Vec<OfficeAccountAssessment>,
    pub(crate) error: &'a Error,
}

pub(crate) fn build_office_operation_failure_outcome(
    input: OfficeOperationFailureInput<'_>,
) -> Result<ToolExecutionOutcome> {
    let failure_kind = classify_office_failure_kind(input.error);
    let payload = OfficeOperationFailureResponse {
        op: input.op.to_string(),
        ok: false,
        error: input.error.to_string(),
        error_stage: input.error.stage().to_string(),
        failure_kind: office_failure_kind_label(failure_kind).to_string(),
        provider: input
            .provider
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string),
        account_key: input
            .account_key
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string),
        office_assessment: OfficeOperationAssessmentHint {
            capability: office_capability_label(input.capability).to_string(),
            default_account_key: input.default_account_key,
            account_assessments: filter_relevant_assessments(
                input.account_assessments,
                input.provider,
                input.account_key,
            ),
        },
    };
    Ok(
        ToolExecutionOutcome::text(serialize_tool_output(input.stage, &payload)?)
            .with_failure_kind(failure_kind),
    )
}

fn filter_relevant_assessments(
    account_assessments: Vec<OfficeAccountAssessment>,
    provider: Option<&str>,
    account_key: Option<&str>,
) -> Vec<OfficeAccountAssessment> {
    if let Some(account_key) = account_key.map(str::trim).filter(|value| !value.is_empty()) {
        let matching = account_assessments
            .iter()
            .filter(|item| item.account_key == account_key)
            .cloned()
            .collect::<Vec<_>>();
        if !matching.is_empty() {
            return matching;
        }
    }
    if let Some(provider) = provider.map(str::trim).filter(|value| !value.is_empty()) {
        let matching = account_assessments
            .iter()
            .filter(|item| item.provider_kind == provider)
            .cloned()
            .collect::<Vec<_>>();
        if !matching.is_empty() {
            return matching;
        }
    }
    account_assessments
}

fn classify_office_failure_kind(error: &Error) -> ToolExecutionFailureKind {
    if error.is_retryable_upstream() {
        return ToolExecutionFailureKind::Retryable;
    }
    match error {
        Error::Config { message, .. } => {
            let lower = message.to_ascii_lowercase();
            if contains_any(
                &lower,
                &[
                    "credential",
                    "access_token",
                    "token",
                    "not configured",
                    "no configured",
                    "multiple configured accounts",
                    "provider is required",
                    "permission denied",
                    "forbidden",
                ],
            ) {
                ToolExecutionFailureKind::Capability
            } else {
                ToolExecutionFailureKind::Permanent
            }
        }
        Error::Http { status_code, .. } => match status_code {
            401 | 403 => ToolExecutionFailureKind::Capability,
            408 | 409 | 425 | 429 | 500..=599 => ToolExecutionFailureKind::Retryable,
            _ => ToolExecutionFailureKind::Permanent,
        },
        Error::Io { source, .. } => match source.kind() {
            std::io::ErrorKind::TimedOut
            | std::io::ErrorKind::Interrupted
            | std::io::ErrorKind::WouldBlock
            | std::io::ErrorKind::ConnectionRefused
            | std::io::ErrorKind::ConnectionReset
            | std::io::ErrorKind::ConnectionAborted
            | std::io::ErrorKind::NotConnected
            | std::io::ErrorKind::BrokenPipe => ToolExecutionFailureKind::Retryable,
            std::io::ErrorKind::PermissionDenied => ToolExecutionFailureKind::Capability,
            _ => ToolExecutionFailureKind::Permanent,
        },
        Error::Nvs { .. } | Error::Spiffs { .. } => ToolExecutionFailureKind::Capability,
        Error::Esp { .. } | Error::Other { .. } => ToolExecutionFailureKind::Permanent,
    }
}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| haystack.contains(needle))
}

fn office_failure_kind_label(failure_kind: ToolExecutionFailureKind) -> &'static str {
    match failure_kind {
        ToolExecutionFailureKind::Retryable => "retryable",
        ToolExecutionFailureKind::Permanent => "permanent",
        ToolExecutionFailureKind::Capability => "capability",
    }
}

fn office_capability_label(capability: OfficeCapability) -> &'static str {
    match capability {
        OfficeCapability::Mail => "mail",
        OfficeCapability::Calendar => "calendar",
        OfficeCapability::Documents => "documents",
        OfficeCapability::ContactsDirectory => "contacts_directory",
    }
}
