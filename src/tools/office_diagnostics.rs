use crate::office::{
    OfficeAccountAssessment, OfficeAccountRuntimeStatus, OfficeConfigReadiness,
    OfficeProviderFieldSchema,
};
use serde::Serialize;

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(crate) struct OfficeAccountDiagnostic {
    pub(crate) account_key: String,
    pub(crate) provider_kind: String,
    pub(crate) diagnosis_kind: &'static str,
    pub(crate) summary: String,
    pub(crate) recommended_action: &'static str,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub(crate) last_error: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub(crate) last_activity_kind: String,
    #[serde(skip_serializing_if = "is_zero")]
    pub(crate) last_activity_at_unix_secs: u64,
}

pub(crate) fn build_account_diagnostics(
    account_assessments: &[OfficeAccountAssessment],
) -> Vec<OfficeAccountDiagnostic> {
    account_assessments
        .iter()
        .map(build_account_diagnostic)
        .collect()
}

fn build_account_diagnostic(assessment: &OfficeAccountAssessment) -> OfficeAccountDiagnostic {
    let runtime = assessment.runtime_status.as_ref();
    let (diagnosis_kind, summary, recommended_action) = if assessment.readiness
        == OfficeConfigReadiness::NeedsConfiguration
    {
        (
            "needs_configuration",
            format_missing_fields_summary(
                &assessment.missing_fields,
                &assessment.missing_field_details,
            ),
            "configure_account",
        )
    } else if runtime_indicates_failure(runtime) {
        (
            "runtime_failure",
            format_runtime_failure_summary(runtime),
            "review_runtime_error",
        )
    } else {
        match assessment.readiness {
                OfficeConfigReadiness::ReadyForProbe => (
                    "needs_probe",
                    "Credential is configured, but this account has not passed a successful probe or runtime activity yet.".to_string(),
                    "probe",
                ),
                OfficeConfigReadiness::ProbeUnavailable => (
                    "probe_unavailable",
                    "Credential is configured, but this provider has no probe adapter on the current platform.".to_string(),
                    "none",
                ),
                OfficeConfigReadiness::Ready => (
                    "ready",
                    format_ready_summary(runtime),
                    "none",
                ),
                OfficeConfigReadiness::NeedsConfiguration => unreachable!(),
            }
    };
    OfficeAccountDiagnostic {
        account_key: assessment.account_key.clone(),
        provider_kind: assessment.provider_kind.clone(),
        diagnosis_kind,
        summary,
        recommended_action,
        last_error: runtime
            .map(|status| status.last_error.clone())
            .unwrap_or_default(),
        last_activity_kind: runtime
            .map(|status| status.last_activity_kind.clone())
            .unwrap_or_default(),
        last_activity_at_unix_secs: runtime
            .map(|status| status.last_activity_at_unix_secs)
            .unwrap_or(0),
    }
}

fn format_missing_fields_summary(
    missing_fields: &[String],
    missing_field_details: &[OfficeProviderFieldSchema],
) -> String {
    let detail_summary = if missing_field_details.is_empty() {
        missing_fields.join(", ")
    } else {
        missing_field_details
            .iter()
            .map(|field| format!("{} ({})", field.label, field.key))
            .collect::<Vec<_>>()
            .join(", ")
    };
    format!(
        "Missing required credential/config fields: {}.",
        detail_summary
    )
}

fn runtime_indicates_failure(runtime: Option<&OfficeAccountRuntimeStatus>) -> bool {
    runtime.is_some_and(|status| {
        !status.last_error.trim().is_empty()
            || (!status.last_activity_kind.trim().is_empty() && !status.last_activity_ok)
    })
}

fn format_runtime_failure_summary(runtime: Option<&OfficeAccountRuntimeStatus>) -> String {
    let Some(runtime) = runtime else {
        return "Latest runtime activity failed.".to_string();
    };
    match (
        runtime.last_activity_kind.trim().is_empty(),
        runtime.last_error.trim().is_empty(),
    ) {
        (false, false) => format!(
            "Latest activity {} failed: {}.",
            runtime.last_activity_kind, runtime.last_error
        ),
        (false, true) => format!("Latest activity {} failed.", runtime.last_activity_kind),
        (true, false) => format!("Latest runtime failure: {}.", runtime.last_error),
        (true, true) => "Latest runtime activity failed.".to_string(),
    }
}

fn format_ready_summary(runtime: Option<&OfficeAccountRuntimeStatus>) -> String {
    if let Some(runtime) = runtime {
        if runtime.probe_ok
            && !runtime.last_activity_kind.trim().is_empty()
            && runtime.last_activity_ok
        {
            return format!(
                "Ready. Latest successful activity: {}.",
                runtime.last_activity_kind
            );
        }
        if runtime.probe_ok {
            return "Ready. Probe succeeded and no runtime failure is recorded.".to_string();
        }
        if !runtime.last_activity_kind.trim().is_empty() && runtime.last_activity_ok {
            return format!(
                "Ready. Latest successful activity: {}.",
                runtime.last_activity_kind
            );
        }
    }
    "Ready. Probe or successful runtime activity is present.".to_string()
}

fn is_zero(value: &u64) -> bool {
    *value == 0
}
