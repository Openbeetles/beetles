//! Structured register-table helper contracts for P7 engineering synthesis.

use crate::error::{Error, Result};
use crate::util::truncate_content_to_max;
use serde::{Deserialize, Serialize};
use serde_json::Value;

const REGISTER_TABLE_MAX_SUMMARY_CHARS: usize = 220;
const REGISTER_TABLE_MAX_REGISTERS: usize = 16;
const REGISTER_TABLE_MAX_FIELDS_PER_REGISTER: usize = 24;
const REGISTER_TABLE_MAX_REGISTER_NAME_CHARS: usize = 64;
const REGISTER_TABLE_MAX_REGISTER_ADDRESS_CHARS: usize = 32;
const REGISTER_TABLE_MAX_REGISTER_SUMMARY_CHARS: usize = 160;
const REGISTER_TABLE_MAX_FIELD_NAME_CHARS: usize = 64;
const REGISTER_TABLE_MAX_FIELD_SUMMARY_CHARS: usize = 160;
const REGISTER_TABLE_MAX_RESET_VALUE_CHARS: usize = 32;
const REGISTER_TABLE_MAX_EVIDENCE_REFS: usize = 8;
const REGISTER_TABLE_MAX_EVIDENCE_REF_CHARS: usize = 120;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RegisterFieldAccess {
    ReadOnly,
    WriteOnly,
    ReadWrite,
    WriteOneToClear,
    Reserved,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegisterTableBitRange {
    pub msb: u8,
    pub lsb: u8,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegisterTableField {
    pub name: String,
    pub bit_range: RegisterTableBitRange,
    pub access: RegisterFieldAccess,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reset_value: Option<String>,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    #[serde(default)]
    pub requires_adjudication: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegisterTableEntry {
    pub name: String,
    pub address: String,
    pub summary: String,
    #[serde(default)]
    pub fields: Vec<RegisterTableField>,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    #[serde(default)]
    pub requires_adjudication: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegisterTableResult {
    pub summary: String,
    #[serde(default)]
    pub registers: Vec<RegisterTableEntry>,
}

pub fn validate_register_table_result(value: Value) -> Result<RegisterTableResult> {
    let result: RegisterTableResult = serde_json::from_value(value)
        .map_err(|error| Error::config("register_table_result_decode", error.to_string()))?;
    normalize_register_table_result(result)
}

fn normalize_register_table_result(result: RegisterTableResult) -> Result<RegisterTableResult> {
    let summary = truncate_content_to_max(result.summary.trim(), REGISTER_TABLE_MAX_SUMMARY_CHARS)
        .into_owned();
    if summary.is_empty() {
        return Err(Error::config(
            "register_table_result_validate",
            "missing summary",
        ));
    }
    if result.registers.len() > REGISTER_TABLE_MAX_REGISTERS {
        return Err(Error::config(
            "register_table_result_validate",
            format!("registers exceeds {}", REGISTER_TABLE_MAX_REGISTERS),
        ));
    }
    let registers = result
        .registers
        .into_iter()
        .map(normalize_register_table_entry)
        .collect::<Result<Vec<_>>>()?;
    Ok(RegisterTableResult { summary, registers })
}

fn normalize_register_table_entry(entry: RegisterTableEntry) -> Result<RegisterTableEntry> {
    if !entry.requires_adjudication {
        return Err(Error::config(
            "register_table_entry_validate",
            "register table entry must require adjudication",
        ));
    }
    let name = truncate_content_to_max(entry.name.trim(), REGISTER_TABLE_MAX_REGISTER_NAME_CHARS)
        .into_owned();
    let address = truncate_content_to_max(
        entry.address.trim(),
        REGISTER_TABLE_MAX_REGISTER_ADDRESS_CHARS,
    )
    .into_owned();
    let summary = truncate_content_to_max(
        entry.summary.trim(),
        REGISTER_TABLE_MAX_REGISTER_SUMMARY_CHARS,
    )
    .into_owned();
    if name.is_empty() || address.is_empty() || summary.is_empty() {
        return Err(Error::config(
            "register_table_entry_validate",
            "register table entry requires name, address, and summary",
        ));
    }
    if entry.fields.is_empty() {
        return Err(Error::config(
            "register_table_entry_validate",
            "register table entry requires at least one field",
        ));
    }
    if entry.fields.len() > REGISTER_TABLE_MAX_FIELDS_PER_REGISTER {
        return Err(Error::config(
            "register_table_entry_validate",
            format!("fields exceeds {}", REGISTER_TABLE_MAX_FIELDS_PER_REGISTER),
        ));
    }
    let fields = entry
        .fields
        .into_iter()
        .map(normalize_register_table_field)
        .collect::<Result<Vec<_>>>()?;
    let evidence_refs = normalize_evidence_refs(
        entry.evidence_refs,
        "register_table_entry_validate",
        "register table entry requires evidence_refs",
    )?;

    Ok(RegisterTableEntry {
        name,
        address,
        summary,
        fields,
        evidence_refs,
        requires_adjudication: true,
    })
}

fn normalize_register_table_field(field: RegisterTableField) -> Result<RegisterTableField> {
    if !field.requires_adjudication {
        return Err(Error::config(
            "register_table_field_validate",
            "register table field must require adjudication",
        ));
    }
    let name = truncate_content_to_max(field.name.trim(), REGISTER_TABLE_MAX_FIELD_NAME_CHARS)
        .into_owned();
    let summary =
        truncate_content_to_max(field.summary.trim(), REGISTER_TABLE_MAX_FIELD_SUMMARY_CHARS)
            .into_owned();
    if name.is_empty() || summary.is_empty() {
        return Err(Error::config(
            "register_table_field_validate",
            "register table field requires name and summary",
        ));
    }
    if field.bit_range.msb > 63 || field.bit_range.lsb > field.bit_range.msb {
        return Err(Error::config(
            "register_table_field_validate",
            "register table field bit_range must satisfy 0 <= lsb <= msb <= 63",
        ));
    }
    let reset_value = field
        .reset_value
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| {
            truncate_content_to_max(value, REGISTER_TABLE_MAX_RESET_VALUE_CHARS).into_owned()
        });
    let evidence_refs = normalize_evidence_refs(
        field.evidence_refs,
        "register_table_field_validate",
        "register table field requires evidence_refs",
    )?;

    Ok(RegisterTableField {
        name,
        bit_range: field.bit_range,
        access: field.access,
        summary,
        reset_value,
        evidence_refs,
        requires_adjudication: true,
    })
}

fn normalize_evidence_refs(
    refs: Vec<String>,
    stage: &'static str,
    missing_message: &'static str,
) -> Result<Vec<String>> {
    if refs.len() > REGISTER_TABLE_MAX_EVIDENCE_REFS {
        return Err(Error::config(
            stage,
            format!("evidence_refs exceeds {}", REGISTER_TABLE_MAX_EVIDENCE_REFS),
        ));
    }
    let evidence_refs = refs
        .into_iter()
        .map(|reference| {
            truncate_content_to_max(reference.trim(), REGISTER_TABLE_MAX_EVIDENCE_REF_CHARS)
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
    fn validate_register_table_result_accepts_adjudicated_registers_and_fields() {
        let result = validate_register_table_result(json!({
            "summary": "Distilled one control register.",
            "registers": [{
                "name": "CTRL_MEAS",
                "address": "0xF4",
                "summary": "Control register for oversampling and mode.",
                "fields": [{
                    "name": "osrs_t",
                    "bit_range": {"msb": 7, "lsb": 5},
                    "access": "read_write",
                    "summary": "Temperature oversampling control.",
                    "reset_value": "0b000",
                    "evidence_refs": ["table 18", "section 5.4.3"],
                    "requires_adjudication": true
                }],
                "evidence_refs": ["table 18", "section 5.4.3"],
                "requires_adjudication": true
            }]
        }))
        .expect("valid register table result");

        assert_eq!(result.registers.len(), 1);
        assert_eq!(
            result.registers[0].fields[0].bit_range,
            RegisterTableBitRange { msb: 7, lsb: 5 }
        );
    }

    #[test]
    fn validate_register_table_result_rejects_invalid_bit_range() {
        let error = validate_register_table_result(json!({
            "summary": "bad",
            "registers": [{
                "name": "STATUS",
                "address": "0x00",
                "summary": "Status register.",
                "fields": [{
                    "name": "busy",
                    "bit_range": {"msb": 1, "lsb": 3},
                    "access": "read_only",
                    "summary": "Busy flag.",
                    "evidence_refs": ["section 2.1"],
                    "requires_adjudication": true
                }],
                "evidence_refs": ["section 2.1"],
                "requires_adjudication": true
            }]
        }))
        .expect_err("invalid bit range should be rejected");

        assert!(error
            .to_string()
            .contains("register table field bit_range must satisfy"));
    }
}
