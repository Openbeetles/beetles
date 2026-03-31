//! Tool outcome classification helpers for the agent loop.
//! Centralizes failure semantics so loop/state code stays thin.

use crate::error::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ToolFailureKind {
    Retryable,
    Permanent,
    Capability,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ToolBlockerKind {
    Retryable,
    Permanent,
    Capability,
    Mixed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ToolFailureAssessment {
    pub(crate) kind: ToolFailureKind,
    pub(crate) hint: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ToolBlockerSummary {
    pub(crate) kind: ToolBlockerKind,
    pub(crate) failed_calls: usize,
    pub(crate) total_calls: usize,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ToolFailureSummary {
    pub(crate) failed_calls: usize,
    pub(crate) retryable_failures: usize,
    pub(crate) permanent_failures: usize,
    pub(crate) capability_failures: usize,
}

impl ToolFailureSummary {
    pub(crate) fn record(&mut self, kind: ToolFailureKind) {
        self.failed_calls = self.failed_calls.saturating_add(1);
        match kind {
            ToolFailureKind::Retryable => {
                self.retryable_failures = self.retryable_failures.saturating_add(1);
            }
            ToolFailureKind::Permanent => {
                self.permanent_failures = self.permanent_failures.saturating_add(1);
            }
            ToolFailureKind::Capability => {
                self.capability_failures = self.capability_failures.saturating_add(1);
            }
        }
    }
}

pub(crate) fn summarize_tool_blocker(
    total_calls: usize,
    summary: ToolFailureSummary,
) -> Option<ToolBlockerSummary> {
    if total_calls == 0 || summary.failed_calls != total_calls {
        return None;
    }
    let kind = if summary.capability_failures == summary.failed_calls {
        ToolBlockerKind::Capability
    } else if summary.permanent_failures == summary.failed_calls {
        ToolBlockerKind::Permanent
    } else if summary.retryable_failures == summary.failed_calls {
        ToolBlockerKind::Retryable
    } else {
        ToolBlockerKind::Mixed
    };
    Some(ToolBlockerSummary {
        kind,
        failed_calls: summary.failed_calls,
        total_calls,
    })
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
    match err {
        Error::Config { message, .. } => classify_config_error(message),
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
        Error::Other { source, .. } => {
            if err.is_tls_admission() || err.is_connect_error() {
                ToolFailureAssessment {
                    kind: ToolFailureKind::Retryable,
                    hint: " Connection or runtime pressure blocked the operation. Retry only if conditions change or use a different path.",
                }
            } else {
                classify_text_error(&source.to_string())
            }
        }
    }
}

fn classify_denied_reason(reason: &str) -> ToolFailureKind {
    match reason {
        "critical_no_network_tools" | "cautious_low_heap_for_http_tool" => {
            return ToolFailureKind::Retryable;
        }
        _ => {}
    }

    let lower = reason.to_ascii_lowercase();
    if contains_any(
        &lower,
        &[
            "permission",
            "forbidden",
            "not available",
            "not allowed",
            "disabled",
            "unsupported",
            "not supported",
        ],
    ) {
        ToolFailureKind::Capability
    } else if contains_any(
        &lower,
        &[
            "retry", "timeout", "busy", "pressure", "critical", "low_heap", "later",
        ],
    ) {
        ToolFailureKind::Retryable
    } else {
        ToolFailureKind::Permanent
    }
}

fn classify_config_error(message: &str) -> ToolFailureAssessment {
    let lower = message.to_ascii_lowercase();
    if contains_any(
        &lower,
        &[
            "missing api key",
            "api key",
            "token",
            "credential",
            "not configured",
            "disabled",
            "not enabled",
            "permission denied",
            "forbidden",
        ],
    ) {
        return ToolFailureAssessment {
            kind: ToolFailureKind::Capability,
            hint: " Required configuration, credentials, or permissions are missing. Stop retrying the same call and explain the limitation if needed.",
        };
    }
    if contains_any(
        &lower,
        &[
            "invalid",
            "parse",
            "not found",
            "does not exist",
            "unsupported",
        ],
    ) {
        return ToolFailureAssessment {
            kind: ToolFailureKind::Permanent,
            hint: " Check the input or target resource. Do not retry unchanged parameters.",
        };
    }
    ToolFailureAssessment {
        kind: ToolFailureKind::Permanent,
        hint: " Review the parameters and change the approach instead of retrying the same call.",
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

fn classify_text_error(message: &str) -> ToolFailureAssessment {
    let lower = message.to_ascii_lowercase();
    if contains_any(
        &lower,
        &[
            "permission denied",
            "forbidden",
            "unauthorized",
            "missing api key",
            "credential",
            "not supported",
            "not available",
        ],
    ) {
        return ToolFailureAssessment {
            kind: ToolFailureKind::Capability,
            hint: " The runtime lacks permission or capability for this operation. Explain the limitation instead of retrying unchanged calls.",
        };
    }
    if contains_any(
        &lower,
        &[
            "timeout",
            "timed out",
            "connection",
            "temporar",
            "retry later",
            "rate limit",
            "too many requests",
            "unavailable",
        ],
    ) {
        return ToolFailureAssessment {
            kind: ToolFailureKind::Retryable,
            hint: " This looks transient. Retry sparingly or switch to a different path.",
        };
    }
    ToolFailureAssessment {
        kind: ToolFailureKind::Permanent,
        hint: " This request failed in a way that likely needs different inputs or a different approach.",
    }
}

fn contains_any(text: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| text.contains(needle))
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
    fn summarize_tool_blocker_requires_full_round_failure() {
        let blocker = summarize_tool_blocker(
            2,
            ToolFailureSummary {
                failed_calls: 1,
                permanent_failures: 1,
                ..ToolFailureSummary::default()
            },
        );
        assert!(blocker.is_none());
    }

    #[test]
    fn summarize_tool_blocker_classifies_capability_rounds() {
        let blocker = summarize_tool_blocker(
            2,
            ToolFailureSummary {
                failed_calls: 2,
                capability_failures: 2,
                ..ToolFailureSummary::default()
            },
        )
        .expect("blocker");
        assert_eq!(blocker.kind, ToolBlockerKind::Capability);
    }

    #[test]
    fn summarize_tool_blocker_marks_mixed_rounds() {
        let blocker = summarize_tool_blocker(
            2,
            ToolFailureSummary {
                failed_calls: 2,
                retryable_failures: 1,
                permanent_failures: 1,
                ..ToolFailureSummary::default()
            },
        )
        .expect("blocker");
        assert_eq!(blocker.kind, ToolBlockerKind::Mixed);
    }

    #[test]
    fn config_error_missing_credentials_is_capability_failure() {
        let assessment = classify_tool_error(&Error::config(
            "tool_test",
            "missing api key for external search",
        ));
        assert_eq!(assessment.kind, ToolFailureKind::Capability);
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
    fn timed_out_io_is_retryable_failure() {
        let assessment = classify_tool_error(&Error::io(
            "tool_io",
            std::io::Error::new(std::io::ErrorKind::TimedOut, "timeout"),
        ));
        assert_eq!(assessment.kind, ToolFailureKind::Retryable);
    }
}
