//! Structured protocol-frame helper contracts for P7 engineering synthesis.

use crate::error::{Error, Result};
use crate::util::truncate_content_to_max;
use serde::{Deserialize, Serialize};
use serde_json::Value;

const PROTOCOL_FRAME_MAX_SUMMARY_CHARS: usize = 220;
const PROTOCOL_FRAME_MAX_FRAMES: usize = 12;
const PROTOCOL_FRAME_MAX_FIELDS_PER_FRAME: usize = 24;
const PROTOCOL_FRAME_MAX_NAME_CHARS: usize = 64;
const PROTOCOL_FRAME_MAX_FRAME_SUMMARY_CHARS: usize = 160;
const PROTOCOL_FRAME_MAX_FIELD_SUMMARY_CHARS: usize = 160;
const PROTOCOL_FRAME_MAX_FIXED_VALUE_CHARS: usize = 32;
const PROTOCOL_FRAME_MAX_EVIDENCE_REFS: usize = 8;
const PROTOCOL_FRAME_MAX_EVIDENCE_REF_CHARS: usize = 120;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProtocolFrameDirection {
    HostToDevice,
    DeviceToHost,
    Bidirectional,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProtocolFieldEncoding {
    U8,
    U16Le,
    U16Be,
    Bytes,
    BitFlags,
    Checksum,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtocolFrameByteRange {
    pub start: u16,
    pub end: u16,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtocolFrameField {
    pub name: String,
    pub byte_range: ProtocolFrameByteRange,
    pub encoding: ProtocolFieldEncoding,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fixed_value: Option<String>,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    #[serde(default)]
    pub requires_adjudication: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtocolFrameEntry {
    pub name: String,
    pub direction: ProtocolFrameDirection,
    pub summary: String,
    #[serde(default)]
    pub fields: Vec<ProtocolFrameField>,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    #[serde(default)]
    pub requires_adjudication: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtocolFrameResult {
    pub summary: String,
    #[serde(default)]
    pub frames: Vec<ProtocolFrameEntry>,
}

pub fn validate_protocol_frame_result(value: Value) -> Result<ProtocolFrameResult> {
    let result: ProtocolFrameResult = serde_json::from_value(value)
        .map_err(|error| Error::config("protocol_frame_result_decode", error.to_string()))?;
    normalize_protocol_frame_result(result)
}

fn normalize_protocol_frame_result(result: ProtocolFrameResult) -> Result<ProtocolFrameResult> {
    let summary = truncate_content_to_max(result.summary.trim(), PROTOCOL_FRAME_MAX_SUMMARY_CHARS)
        .into_owned();
    if summary.is_empty() {
        return Err(Error::config(
            "protocol_frame_result_validate",
            "missing summary",
        ));
    }
    if result.frames.len() > PROTOCOL_FRAME_MAX_FRAMES {
        return Err(Error::config(
            "protocol_frame_result_validate",
            format!("frames exceeds {}", PROTOCOL_FRAME_MAX_FRAMES),
        ));
    }
    let frames = result
        .frames
        .into_iter()
        .map(normalize_protocol_frame_entry)
        .collect::<Result<Vec<_>>>()?;
    Ok(ProtocolFrameResult { summary, frames })
}

fn normalize_protocol_frame_entry(entry: ProtocolFrameEntry) -> Result<ProtocolFrameEntry> {
    if !entry.requires_adjudication {
        return Err(Error::config(
            "protocol_frame_entry_validate",
            "protocol frame entry must require adjudication",
        ));
    }
    let name =
        truncate_content_to_max(entry.name.trim(), PROTOCOL_FRAME_MAX_NAME_CHARS).into_owned();
    let summary =
        truncate_content_to_max(entry.summary.trim(), PROTOCOL_FRAME_MAX_FRAME_SUMMARY_CHARS)
            .into_owned();
    if name.is_empty() || summary.is_empty() {
        return Err(Error::config(
            "protocol_frame_entry_validate",
            "protocol frame entry requires name and summary",
        ));
    }
    if entry.fields.is_empty() {
        return Err(Error::config(
            "protocol_frame_entry_validate",
            "protocol frame entry requires at least one field",
        ));
    }
    if entry.fields.len() > PROTOCOL_FRAME_MAX_FIELDS_PER_FRAME {
        return Err(Error::config(
            "protocol_frame_entry_validate",
            format!("fields exceeds {}", PROTOCOL_FRAME_MAX_FIELDS_PER_FRAME),
        ));
    }
    let fields = entry
        .fields
        .into_iter()
        .map(normalize_protocol_frame_field)
        .collect::<Result<Vec<_>>>()?;
    let evidence_refs = normalize_evidence_refs(
        entry.evidence_refs,
        "protocol_frame_entry_validate",
        "protocol frame entry requires evidence_refs",
    )?;
    Ok(ProtocolFrameEntry {
        name,
        direction: entry.direction,
        summary,
        fields,
        evidence_refs,
        requires_adjudication: true,
    })
}

fn normalize_protocol_frame_field(field: ProtocolFrameField) -> Result<ProtocolFrameField> {
    if !field.requires_adjudication {
        return Err(Error::config(
            "protocol_frame_field_validate",
            "protocol frame field must require adjudication",
        ));
    }
    let name =
        truncate_content_to_max(field.name.trim(), PROTOCOL_FRAME_MAX_NAME_CHARS).into_owned();
    let summary =
        truncate_content_to_max(field.summary.trim(), PROTOCOL_FRAME_MAX_FIELD_SUMMARY_CHARS)
            .into_owned();
    if name.is_empty() || summary.is_empty() {
        return Err(Error::config(
            "protocol_frame_field_validate",
            "protocol frame field requires name and summary",
        ));
    }
    if field.byte_range.end < field.byte_range.start {
        return Err(Error::config(
            "protocol_frame_field_validate",
            "protocol frame field byte_range must satisfy start <= end",
        ));
    }
    let fixed_value = field
        .fixed_value
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| {
            truncate_content_to_max(value, PROTOCOL_FRAME_MAX_FIXED_VALUE_CHARS).into_owned()
        });
    let evidence_refs = normalize_evidence_refs(
        field.evidence_refs,
        "protocol_frame_field_validate",
        "protocol frame field requires evidence_refs",
    )?;
    Ok(ProtocolFrameField {
        name,
        byte_range: field.byte_range,
        encoding: field.encoding,
        summary,
        fixed_value,
        evidence_refs,
        requires_adjudication: true,
    })
}

fn normalize_evidence_refs(
    refs: Vec<String>,
    stage: &'static str,
    missing_message: &'static str,
) -> Result<Vec<String>> {
    if refs.len() > PROTOCOL_FRAME_MAX_EVIDENCE_REFS {
        return Err(Error::config(
            stage,
            format!("evidence_refs exceeds {}", PROTOCOL_FRAME_MAX_EVIDENCE_REFS),
        ));
    }
    let evidence_refs = refs
        .into_iter()
        .map(|reference| {
            truncate_content_to_max(reference.trim(), PROTOCOL_FRAME_MAX_EVIDENCE_REF_CHARS)
                .into_owned()
        })
        .filter(|reference| !reference.is_empty())
        .collect::<Vec<_>>();
    if evidence_refs.is_empty() {
        return Err(Error::config(stage, missing_message));
    }
    Ok(evidence_refs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn validate_protocol_frame_result_accepts_adjudicated_frames_and_fields() {
        let result = validate_protocol_frame_result(json!({
            "summary": "Distilled one status response frame.",
            "frames": [{
                "name": "status_response",
                "direction": "device_to_host",
                "summary": "Status response frame carrying opcode and state bits.",
                "fields": [{
                    "name": "opcode",
                    "byte_range": {"start": 0, "end": 0},
                    "encoding": "u8",
                    "summary": "Response opcode byte.",
                    "fixed_value": "0x81",
                    "evidence_refs": ["section 4.1", "table 7"],
                    "requires_adjudication": true
                }],
                "evidence_refs": ["section 4.1", "table 7"],
                "requires_adjudication": true
            }]
        }))
        .expect("valid protocol frame result");

        assert_eq!(result.frames.len(), 1);
        assert_eq!(
            result.frames[0].direction,
            ProtocolFrameDirection::DeviceToHost
        );
        assert_eq!(
            result.frames[0].fields[0].encoding,
            ProtocolFieldEncoding::U8
        );
    }

    #[test]
    fn validate_protocol_frame_result_rejects_invalid_byte_range() {
        let error = validate_protocol_frame_result(json!({
            "summary": "bad",
            "frames": [{
                "name": "request",
                "direction": "host_to_device",
                "summary": "Command request frame.",
                "fields": [{
                    "name": "length",
                    "byte_range": {"start": 3, "end": 1},
                    "encoding": "u8",
                    "summary": "Payload length byte.",
                    "evidence_refs": ["section 3.2"],
                    "requires_adjudication": true
                }],
                "evidence_refs": ["section 3.2"],
                "requires_adjudication": true
            }]
        }))
        .expect_err("invalid byte range should be rejected");

        assert!(error
            .to_string()
            .contains("protocol frame field byte_range must satisfy"));
    }
}
