use beetle::{
    validate_engineering_distillation_result, validate_protocol_frame_result,
    validate_register_table_result, validate_state_machine_result,
    EngineeringDistillationAssetKind, ProtocolFieldEncoding, ProtocolFrameByteRange,
    ProtocolFrameDirection, RegisterFieldAccess, RegisterTableBitRange, StateMachineFindingKind,
    StateNodeRole,
};
use serde_json::json;

#[test]
fn validate_p7_public_validator_facades_accept_compact_happy_paths() {
    let engineering = validate_engineering_distillation_result(json!({
        "summary": "Distilled one register table candidate from the reference text.",
        "asset_candidates": [{
            "kind": "register_table",
            "title": "BME280 ctrl_meas register sketch",
            "summary": "Summarizes the control register layout for later driver review.",
            "content": "| Bits | Name | Meaning |\\n| 7:5 | osrs_t | temperature oversampling |",
            "evidence_refs": ["table 18"],
            "requires_adjudication": true
        }]
    }))
    .expect("valid engineering distillation result");
    assert_eq!(
        engineering.asset_candidates[0].kind,
        EngineeringDistillationAssetKind::RegisterTable
    );

    let protocol = validate_protocol_frame_result(json!({
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
                "evidence_refs": ["section 4.1"],
                "requires_adjudication": true
            }],
            "evidence_refs": ["section 4.1"],
            "requires_adjudication": true
        }]
    }))
    .expect("valid protocol frame result");
    assert_eq!(
        protocol.frames[0].direction,
        ProtocolFrameDirection::DeviceToHost
    );
    assert_eq!(
        protocol.frames[0].fields[0].encoding,
        ProtocolFieldEncoding::U8
    );
    assert_eq!(
        protocol.frames[0].fields[0].byte_range,
        ProtocolFrameByteRange { start: 0, end: 0 }
    );

    let register = validate_register_table_result(json!({
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
                "evidence_refs": ["table 18"],
                "requires_adjudication": true
            }],
            "evidence_refs": ["table 18"],
            "requires_adjudication": true
        }]
    }))
    .expect("valid register table result");
    assert_eq!(
        register.registers[0].fields[0].access,
        RegisterFieldAccess::ReadWrite
    );
    assert_eq!(
        register.registers[0].fields[0].bit_range,
        RegisterTableBitRange { msb: 7, lsb: 5 }
    );

    let state_machine = validate_state_machine_result(json!({
        "summary": "Checked one boot state machine.",
        "machines": [{
            "name": "boot_flow",
            "summary": "Boot state progression from reset to ready.",
            "states": [{
                "name": "RESET",
                "role": "initial",
                "summary": "Power-on reset state.",
                "evidence_refs": ["figure 2"],
                "requires_adjudication": true
            }],
            "transitions": [{
                "from": "RESET",
                "to": "RESET",
                "trigger": "stay_put",
                "summary": "Initialization keeps the machine in reset for this smoke check.",
                "evidence_refs": ["section 3.1"],
                "requires_adjudication": true
            }],
            "findings": [{
                "kind": "unsafe_loop",
                "summary": "Review whether retries can loop forever before READY.",
                "state_refs": ["RESET"],
                "transition_refs": ["RESET->RESET:stay_put"],
                "evidence_refs": ["section 3.1"],
                "requires_adjudication": true
            }],
            "evidence_refs": ["figure 2", "section 3.1"],
            "requires_adjudication": true
        }]
    }))
    .expect("valid state machine result");
    assert_eq!(
        state_machine.machines[0].states[0].role,
        StateNodeRole::Initial
    );
    assert_eq!(
        state_machine.machines[0].findings[0].kind,
        StateMachineFindingKind::UnsafeLoop
    );
}
