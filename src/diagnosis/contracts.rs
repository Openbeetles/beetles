use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosisKind {
    Delivery,
    System,
    MemoryRuntime,
    NetworkPath,
    VoicePath,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosisConfidence {
    Low,
    Medium,
    High,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DiagnosisFinding {
    pub level: &'static str,
    pub message: String,
}

impl DiagnosisFinding {
    pub fn observed(message: impl Into<String>) -> Self {
        Self {
            level: "observed",
            message: message.into(),
        }
    }

    pub fn correlated(message: impl Into<String>) -> Self {
        Self {
            level: "correlated",
            message: message.into(),
        }
    }

    pub fn suspected(message: impl Into<String>) -> Self {
        Self {
            level: "suspected",
            message: message.into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DiagnosisRootCause {
    pub code: String,
    pub message: String,
    pub confidence: DiagnosisConfidence,
}

impl DiagnosisRootCause {
    pub fn new(
        code: impl Into<String>,
        message: impl Into<String>,
        confidence: DiagnosisConfidence,
    ) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            confidence,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DiagnosisAction {
    pub code: String,
    pub message: String,
}

impl DiagnosisAction {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DiagnosisEvidence {
    pub key: String,
    pub value: String,
}

impl DiagnosisEvidence {
    pub fn new(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            value: value.into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DiagnosisDegradation {
    pub code: String,
    pub message: String,
}

impl DiagnosisDegradation {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DiagnosisResult {
    pub kind: DiagnosisKind,
    pub summary: String,
    pub findings: Vec<DiagnosisFinding>,
    pub suspected_root_causes: Vec<DiagnosisRootCause>,
    pub recommended_next_steps: Vec<DiagnosisAction>,
    pub evidence: Vec<DiagnosisEvidence>,
    pub confidence: DiagnosisConfidence,
    pub degraded_by: Vec<DiagnosisDegradation>,
    pub safe_actions_available: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnosis_result_serializes_with_required_fields() {
        let result = DiagnosisResult {
            kind: DiagnosisKind::Delivery,
            summary: "dispatch sender is failing".into(),
            findings: vec![DiagnosisFinding::observed("sender failures increased")],
            suspected_root_causes: vec![DiagnosisRootCause::new(
                "delivery_failure",
                "dispatch sender failures dominate recent attempts",
                DiagnosisConfidence::High,
            )],
            recommended_next_steps: vec![DiagnosisAction::new(
                "inspect_channel_connectivity",
                "check channel connectivity and last sender error",
            )],
            evidence: vec![DiagnosisEvidence::new("dispatch_send_fail_total", "3")],
            confidence: DiagnosisConfidence::High,
            degraded_by: Vec::new(),
            safe_actions_available: vec!["inspect_channel_connectivity".into()],
        };

        let json = serde_json::to_value(&result).unwrap();
        assert_eq!(json["kind"], "delivery");
        assert!(json.get("summary").is_some());
        assert!(json.get("findings").is_some());
        assert!(json.get("suspected_root_causes").is_some());
        assert!(json.get("recommended_next_steps").is_some());
        assert!(json.get("evidence").is_some());
        assert!(json.get("confidence").is_some());
        assert!(json.get("degraded_by").is_some());
        assert!(json.get("safe_actions_available").is_some());
    }
}
