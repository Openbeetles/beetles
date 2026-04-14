use beetle::{validate_register_table_result, RegisterFieldAccess, RegisterTableBitRange};
use serde_json::json;

#[test]
fn validate_register_table_result_accepts_adjudicated_registers_and_fields() {
    let result = validate_register_table_result(json!({
        "summary": "Distilled one control register.",
        "registers": [{
            "name": "CTRL_MEAS",
            "address": "0xF4",
            "summary": "Control register for temperature, pressure, and mode.",
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
        result.registers[0].fields[0].access,
        RegisterFieldAccess::ReadWrite
    );
    assert_eq!(
        result.registers[0].fields[0].bit_range,
        RegisterTableBitRange { msb: 7, lsb: 5 }
    );
}

#[test]
fn validate_register_table_result_rejects_non_adjudicated_register_field() {
    let error = validate_register_table_result(json!({
        "summary": "bad",
        "registers": [{
            "name": "STATUS",
            "address": "0x00",
            "summary": "Status register.",
            "fields": [{
                "name": "busy",
                "bit_range": {"msb": 3, "lsb": 3},
                "access": "read_only",
                "summary": "Busy flag.",
                "evidence_refs": ["section 2.1"],
                "requires_adjudication": false
            }],
            "evidence_refs": ["section 2.1"],
            "requires_adjudication": true
        }]
    }))
    .expect_err("field should be rejected");

    assert!(error
        .to_string()
        .contains("register table field must require adjudication"));
}
