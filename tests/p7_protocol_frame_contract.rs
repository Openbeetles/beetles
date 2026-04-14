use beetle::{
    validate_protocol_frame_result, ProtocolFieldEncoding, ProtocolFrameByteRange,
    ProtocolFrameDirection,
};
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
    assert_eq!(
        result.frames[0].fields[0].byte_range,
        ProtocolFrameByteRange { start: 0, end: 0 }
    );
}

#[test]
fn validate_protocol_frame_result_rejects_non_adjudicated_field() {
    let error = validate_protocol_frame_result(json!({
        "summary": "bad",
        "frames": [{
            "name": "command_request",
            "direction": "host_to_device",
            "summary": "Command request frame.",
            "fields": [{
                "name": "length",
                "byte_range": {"start": 1, "end": 1},
                "encoding": "u8",
                "summary": "Payload length byte.",
                "evidence_refs": ["section 3.2"],
                "requires_adjudication": false
            }],
            "evidence_refs": ["section 3.2"],
            "requires_adjudication": true
        }]
    }))
    .expect_err("field should be rejected");

    assert!(error
        .to_string()
        .contains("protocol frame field must require adjudication"));
}
