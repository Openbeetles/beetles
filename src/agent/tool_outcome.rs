//! Tool failure classification helpers for the agent loop.
//! Keeps tool-local failure assessment separate from workflow-level blocker truth.

use crate::error::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ToolFailureKind {
    Retryable,
    Permanent,
    Capability,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ToolFailureAssessment {
    pub(crate) kind: ToolFailureKind,
    pub(crate) hint: &'static str,
}

pub(crate) fn unavailable_tool_assessment() -> ToolFailureAssessment {
    ToolFailureAssessment {
        kind: ToolFailureKind::Capability,
        hint: "",
    }
}

pub(crate) fn denied_tool_assessment(reason: &str) -> ToolFailureAssessment {
    let kind = classify_denied_reason(reason);
    let hint = match kind {
        ToolFailureKind::Retryable => {
            " Runtime pressure blocked this tool. Retry only if conditions change or switch to a lighter tool path."
        }
        ToolFailureKind::Capability => {
            " This tool path is not available in the current runtime or policy context. Stop retrying it unchanged."
        }
        ToolFailureKind::Permanent => {
            " This tool request cannot succeed as issued. Change the approach instead of repeating it."
        }
    };
    ToolFailureAssessment { kind, hint }
}

pub(crate) fn classify_tool_error(err: &Error) -> ToolFailureAssessment {
    if err.is_retryable_upstream() {
        return ToolFailureAssessment {
            kind: ToolFailureKind::Retryable,
            hint: " Connection, upstream timeout, or rate pressure blocked the operation. Retry only if conditions change or use a different path.",
        };
    }

    match err {
        Error::Config { .. } => ToolFailureAssessment {
            kind: ToolFailureKind::Permanent,
            hint: " The tool rejected this request or its local configuration contract is not satisfied. Change the request or configuration before retrying.",
        },
        Error::Http { status_code, .. } => classify_http_error(*status_code),
        Error::Io { source, .. } => classify_io_error(source.kind()),
        Error::Esp { .. } => ToolFailureAssessment {
            kind: ToolFailureKind::Permanent,
            hint: " The underlying platform rejected this operation. Do not keep retrying unchanged parameters.",
        },
        Error::Nvs { .. } | Error::Spiffs { .. } => ToolFailureAssessment {
            kind: ToolFailureKind::Capability,
            hint: " This operation is blocked by local storage state or permissions. Explain the limitation if it cannot be corrected now.",
        },
        Error::Other { source, .. } => source
            .downcast_ref::<Error>()
            .map(classify_tool_error)
            .unwrap_or(ToolFailureAssessment {
                kind: ToolFailureKind::Permanent,
                hint: " The operation failed in a non-recoverable way. Change the request or route instead of retrying unchanged input.",
            }),
    }
}

fn classify_denied_reason(reason: &str) -> ToolFailureKind {
    match reason {
        "critical_no_network_tools" | "cautious_low_heap_for_http_tool" => {
            ToolFailureKind::Retryable
        }
        "operator_only_tool" => ToolFailureKind::Capability,
        "explicit_intent_required" => ToolFailureKind::Permanent,
        _ => ToolFailureKind::Permanent,
    }
}

fn classify_http_error(status_code: u16) -> ToolFailureAssessment {
    match status_code {
        401 | 403 => ToolFailureAssessment {
            kind: ToolFailureKind::Capability,
            hint: " Permission denied. This operation is not allowed in the current context.",
        },
        404 => ToolFailureAssessment {
            kind: ToolFailureKind::Permanent,
            hint: " Resource not found. Verify the URL, identifier, or path before trying again.",
        },
        408 | 429 => ToolFailureAssessment {
            kind: ToolFailureKind::Retryable,
            hint: " The service asked for a retry later or timed out. Retry only if conditions change.",
        },
        500..=599 => ToolFailureAssessment {
            kind: ToolFailureKind::Retryable,
            hint: " Server-side failure. Retry sparingly or switch to a different path.",
        },
        _ => ToolFailureAssessment {
            kind: ToolFailureKind::Permanent,
            hint: " The request was rejected. Change the parameters instead of retrying unchanged input.",
        },
    }
}

fn classify_io_error(kind: std::io::ErrorKind) -> ToolFailureAssessment {
    match kind {
        std::io::ErrorKind::PermissionDenied => ToolFailureAssessment {
            kind: ToolFailureKind::Capability,
            hint: " Permission denied. This operation cannot proceed with current access.",
        },
        std::io::ErrorKind::TimedOut
        | std::io::ErrorKind::Interrupted
        | std::io::ErrorKind::WouldBlock
        | std::io::ErrorKind::ConnectionRefused
        | std::io::ErrorKind::ConnectionReset
        | std::io::ErrorKind::ConnectionAborted
        | std::io::ErrorKind::NotConnected
        | std::io::ErrorKind::BrokenPipe => ToolFailureAssessment {
            kind: ToolFailureKind::Retryable,
            hint: " I/O connectivity failed transiently. Retry only if conditions change.",
        },
        std::io::ErrorKind::NotFound => ToolFailureAssessment {
            kind: ToolFailureKind::Permanent,
            hint: " File or resource not found. Check the path or identifier before retrying.",
        },
        _ => ToolFailureAssessment {
            kind: ToolFailureKind::Permanent,
            hint: " The I/O request failed in a non-recoverable way. Change the approach instead of retrying blindly.",
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn denied_reason_distinguishes_retryable_pressure() {
        let assessment = denied_tool_assessment("critical_no_network_tools");
        assert_eq!(assessment.kind, ToolFailureKind::Retryable);
    }

    #[test]
    fn config_error_wording_does_not_change_failure_kind() {
        let missing_api_key = classify_tool_error(&Error::config(
            "tool_test",
            "missing api key for external search",
        ));
        let permission_denied =
            classify_tool_error(&Error::config("tool_test", "permission denied"));
        let arbitrary =
            classify_tool_error(&Error::config("tool_test", "totally different wording"));

        assert_eq!(missing_api_key.kind, ToolFailureKind::Permanent);
        assert_eq!(permission_denied.kind, ToolFailureKind::Permanent);
        assert_eq!(arbitrary.kind, ToolFailureKind::Permanent);
    }

    #[test]
    fn http_not_found_is_permanent_failure() {
        let assessment = classify_tool_error(&Error::http("http_get_request", 404));
        assert_eq!(assessment.kind, ToolFailureKind::Permanent);
    }

    #[test]
    fn connect_stage_other_error_is_retryable_failure() {
        let assessment = classify_tool_error(&Error::Other {
            source: "connection reset".into(),
            stage: "http_get_request",
        });
        assert_eq!(assessment.kind, ToolFailureKind::Retryable);
    }

    #[test]
    fn non_retryable_other_error_wording_does_not_change_failure_kind() {
        let permission_denied = classify_tool_error(&Error::Other {
            source: "permission denied".into(),
            stage: "tool_test",
        });
        let missing_token = classify_tool_error(&Error::Other {
            source: "missing api key".into(),
            stage: "tool_test",
        });
        let arbitrary = classify_tool_error(&Error::Other {
            source: "custom domain wording".into(),
            stage: "tool_test",
        });

        assert_eq!(permission_denied.kind, ToolFailureKind::Permanent);
        assert_eq!(missing_token.kind, ToolFailureKind::Permanent);
        assert_eq!(arbitrary.kind, ToolFailureKind::Permanent);
    }

    #[test]
    fn timed_out_io_is_retryable_failure() {
        let assessment = classify_tool_error(&Error::io(
            "tool_io",
            std::io::Error::new(std::io::ErrorKind::TimedOut, "timeout"),
        ));
        assert_eq!(assessment.kind, ToolFailureKind::Retryable);
    }
}
