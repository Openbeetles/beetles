use crate::error::{Error, Result};
use crate::office::{
    OfficeAccountAssessment, OfficeAccountIdentityClass, OfficeCapability, OfficeConfigNextAction,
    OfficeProviderFieldSchema, OfficeResolveCandidate, OfficeResolveResult,
};
use crate::tools::{
    office_diagnostics::{build_account_diagnostics, OfficeAccountDiagnostic},
    serialize_tool_output, ToolClarificationField, ToolClarificationOption, ToolExecutionBlocker,
    ToolExecutionBlockerKind, ToolExecutionFailureKind, ToolExecutionOutcome,
};
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
    resolve_hint: Option<OfficeResolveResult>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    account_assessments: Vec<OfficeAccountAssessment>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    account_diagnostics: Vec<OfficeAccountDiagnostic>,
}

pub(crate) struct OfficeOperationFailureInput<'a> {
    pub(crate) stage: &'static str,
    pub(crate) op: &'a str,
    pub(crate) provider: Option<&'a str>,
    pub(crate) account_key: Option<&'a str>,
    pub(crate) capability: OfficeCapability,
    pub(crate) resolve_hint: Option<OfficeResolveResult>,
    pub(crate) account_assessments: Vec<OfficeAccountAssessment>,
    pub(crate) error: &'a Error,
}

pub(crate) fn build_office_operation_failure_outcome(
    input: OfficeOperationFailureInput<'_>,
) -> Result<ToolExecutionOutcome> {
    let failure_kind = classify_office_failure_kind(input.error);
    let relevant_assessments =
        filter_relevant_assessments(input.account_assessments, input.provider, input.account_key);
    let blocker = derive_office_operation_blocker(
        input.capability,
        input.resolve_hint.as_ref(),
        &relevant_assessments,
    );
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
            resolve_hint: input.resolve_hint,
            account_diagnostics: build_account_diagnostics(&relevant_assessments),
            account_assessments: relevant_assessments,
        },
    };
    let outcome = ToolExecutionOutcome::text(serialize_tool_output(input.stage, &payload)?)
        .with_failure_kind(failure_kind);
    if let Some(blocker) = blocker {
        return Ok(outcome.with_blocker(blocker));
    }
    Ok(outcome)
}

fn derive_office_operation_blocker(
    capability: OfficeCapability,
    resolve_hint: Option<&OfficeResolveResult>,
    relevant_assessments: &[OfficeAccountAssessment],
) -> Option<ToolExecutionBlocker> {
    if let Some(OfficeResolveResult::Ambiguous(ambiguity)) = resolve_hint {
        return Some(ToolExecutionBlocker {
            kind: ToolExecutionBlockerKind::NeedsUserChoice,
            summary: format!("账户选择被阻塞：{}", office_capability_label(capability)),
            missing_fields: vec!["account_key".to_string()],
            clarification_fields: vec![ToolClarificationField {
                key: "account_key".to_string(),
                label: "Office account".to_string(),
                description: "Choose which configured account should handle this request."
                    .to_string(),
                required: true,
                secret: false,
                multiple: false,
                options: ambiguity
                    .candidate_accounts
                    .iter()
                    .map(choice_option_from_office_candidate)
                    .collect(),
            }],
        });
    }

    if let Some(assessment) = relevant_assessments.iter().find(|assessment| {
        assessment.next_action == OfficeConfigNextAction::ConfigureAccount
            && !assessment.missing_fields.is_empty()
    }) {
        return Some(ToolExecutionBlocker {
            kind: ToolExecutionBlockerKind::NeedsUserFacts,
            summary: format!(
                "账户配置缺少必要信息：{}",
                assessment.missing_fields.join(", ")
            ),
            missing_fields: assessment.missing_fields.clone(),
            clarification_fields: assessment
                .missing_field_details
                .iter()
                .map(clarification_field_from_provider_schema)
                .collect(),
        });
    }

    if matches!(resolve_hint, Some(OfficeResolveResult::Missing(_))) {
        return Some(ToolExecutionBlocker {
            kind: ToolExecutionBlockerKind::NeedsUserFacts,
            summary: format!(
                "要继续这一步，还需要先配置可用的 {} 账户。",
                office_capability_label(capability)
            ),
            missing_fields: Vec::new(),
            clarification_fields: Vec::new(),
        });
    }

    None
}

fn clarification_field_from_provider_schema(
    field: &OfficeProviderFieldSchema,
) -> ToolClarificationField {
    ToolClarificationField {
        key: field.key.clone(),
        label: field.label.clone(),
        description: field.description.clone(),
        required: field.required,
        secret: field.secret,
        multiple: false,
        options: Vec::new(),
    }
}

fn choice_option_from_office_candidate(
    candidate: &OfficeResolveCandidate,
) -> ToolClarificationOption {
    ToolClarificationOption {
        value: candidate.account_key.clone(),
        label: format!(
            "{} | {} | {}",
            if candidate.account_label.trim().is_empty() {
                candidate.provider_kind.as_str()
            } else {
                candidate.account_label.as_str()
            },
            candidate.provider_kind,
            office_identity_class_label(candidate.identity_class)
        ),
    }
}

fn office_identity_class_label(identity_class: OfficeAccountIdentityClass) -> &'static str {
    match identity_class {
        OfficeAccountIdentityClass::Work => "work",
        OfficeAccountIdentityClass::Personal => "personal",
        OfficeAccountIdentityClass::Family => "family",
        OfficeAccountIdentityClass::Shared => "shared",
        OfficeAccountIdentityClass::Other => "other",
    }
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
        Error::Nvs { .. } | Error::Storage { .. } => ToolExecutionFailureKind::Capability,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::office::{
        OfficeAccountAssessment, OfficeAccountIdentityClass, OfficeCapability,
        OfficeConfigNextAction, OfficeConfigReadiness, OfficeProviderFieldLocation,
        OfficeProviderFieldSchema, OfficeProviderFieldValueKind, OfficeResolveAmbiguity,
        OfficeResolveAmbiguityReason, OfficeResolveCandidate, OfficeResolveMissing,
        OfficeResolveMissingReason, OfficeResolveResult,
    };

    fn sample_assessment() -> OfficeAccountAssessment {
        OfficeAccountAssessment {
            account_key: "mail-work".to_string(),
            provider_kind: "imap_smtp".to_string(),
            enabled_capabilities: vec![OfficeCapability::Mail],
            credential_present: false,
            credential_configured: false,
            probe_supported: true,
            missing_fields: vec!["access_token".to_string()],
            missing_field_details: vec![OfficeProviderFieldSchema {
                key: "access_token".to_string(),
                label: "App password".to_string(),
                label_key: "accounts.providerFieldLabels.access_token".to_string(),
                description: "Mailbox app password.".to_string(),
                description_key: "accounts.providerFieldDescriptions.access_token".to_string(),
                location: OfficeProviderFieldLocation::AccessToken,
                value_kind: OfficeProviderFieldValueKind::Secret,
                required: true,
                secret: true,
                default_value: None,
            }],
            readiness: OfficeConfigReadiness::NeedsConfiguration,
            next_action: OfficeConfigNextAction::ConfigureAccount,
            runtime_status: None,
        }
    }

    #[test]
    fn office_failure_outcome_reports_choice_blocker_for_ambiguous_accounts() {
        let outcome = build_office_operation_failure_outcome(OfficeOperationFailureInput {
            stage: "tool_mail",
            op: "list",
            provider: Some("imap_smtp"),
            account_key: None,
            capability: OfficeCapability::Mail,
            resolve_hint: Some(OfficeResolveResult::Ambiguous(OfficeResolveAmbiguity {
                reason: OfficeResolveAmbiguityReason::MultipleMatchingAccounts,
                candidate_accounts: vec![
                    OfficeResolveCandidate {
                        account_key: "mail-work".to_string(),
                        provider_kind: "imap_smtp".to_string(),
                        account_label: "Work".to_string(),
                        identity_class: OfficeAccountIdentityClass::Work,
                    },
                    OfficeResolveCandidate {
                        account_key: "mail-personal".to_string(),
                        provider_kind: "imap_smtp".to_string(),
                        account_label: "Personal".to_string(),
                        identity_class: OfficeAccountIdentityClass::Personal,
                    },
                ],
            })),
            account_assessments: vec![],
            error: &Error::config("tool_mail", "multiple configured accounts"),
        })
        .expect("structured ambiguity outcome");

        let blocker = outcome.blocker.expect("choice blocker");
        assert_eq!(
            blocker.kind,
            crate::tools::ToolExecutionBlockerKind::NeedsUserChoice
        );
        assert_eq!(blocker.missing_fields, vec!["account_key".to_string()]);
        assert_eq!(blocker.clarification_fields.len(), 1);
        assert_eq!(blocker.clarification_fields[0].key, "account_key");
        assert_eq!(blocker.clarification_fields[0].options.len(), 2);
    }

    #[test]
    fn office_failure_outcome_reports_missing_fact_blocker_for_needs_configuration_account() {
        let outcome = build_office_operation_failure_outcome(OfficeOperationFailureInput {
            stage: "tool_mail",
            op: "list",
            provider: Some("imap_smtp"),
            account_key: Some("mail-work"),
            capability: OfficeCapability::Mail,
            resolve_hint: None,
            account_assessments: vec![sample_assessment()],
            error: &Error::config("tool_mail", "credential state unavailable"),
        })
        .expect("structured needs-configuration outcome");

        let blocker = outcome.blocker.expect("missing-facts blocker");
        assert_eq!(
            blocker.kind,
            crate::tools::ToolExecutionBlockerKind::NeedsUserFacts
        );
        assert_eq!(blocker.missing_fields, vec!["access_token".to_string()]);
        assert_eq!(blocker.clarification_fields.len(), 1);
        assert_eq!(blocker.clarification_fields[0].key, "access_token");
        assert!(blocker.clarification_fields[0].secret);
    }

    #[test]
    fn office_failure_outcome_reports_missing_account_blocker_from_resolve_hint_without_error_wording(
    ) {
        let outcome = build_office_operation_failure_outcome(OfficeOperationFailureInput {
            stage: "tool_mail",
            op: "list",
            provider: Some("imap_smtp"),
            account_key: None,
            capability: OfficeCapability::Mail,
            resolve_hint: Some(OfficeResolveResult::Missing(OfficeResolveMissing {
                reason: OfficeResolveMissingReason::NoMatchingAccounts,
            })),
            account_assessments: vec![],
            error: &Error::config("tool_mail", "credential state unavailable"),
        })
        .expect("structured missing-account outcome");

        let blocker = outcome.blocker.expect("missing-account blocker");
        assert_eq!(
            blocker.kind,
            crate::tools::ToolExecutionBlockerKind::NeedsUserFacts
        );
        assert!(blocker.missing_fields.is_empty());
        assert!(blocker.clarification_fields.is_empty());
        assert!(blocker.summary.contains("mail"));
    }
}
