use beetle::{validate_engineering_distillation_result, EngineeringDistillationAssetKind};
use serde_json::json;

#[test]
fn validate_engineering_distillation_result_accepts_adjudicated_asset_candidates() {
    let result = validate_engineering_distillation_result(json!({
        "summary": "Distilled one register table candidate from the reference text.",
        "asset_candidates": [{
            "kind": "register_table",
            "title": "BME280 ctrl_meas register sketch",
            "summary": "Summarizes the control register layout for later driver review.",
            "content": "| Bits | Name | Meaning |\\n| 7:5 | osrs_t | temperature oversampling |",
            "evidence_refs": ["table 18", "section 5.4.3"],
            "requires_adjudication": true
        }]
    }))
    .expect("valid engineering distillation result");

    assert_eq!(result.asset_candidates.len(), 1);
    assert_eq!(
        result.asset_candidates[0].kind,
        EngineeringDistillationAssetKind::RegisterTable
    );
}

#[test]
fn validate_engineering_distillation_result_rejects_non_adjudicated_candidates() {
    let error = validate_engineering_distillation_result(json!({
        "summary": "bad",
        "asset_candidates": [{
            "kind": "datasheet_note",
            "title": "Timing note",
            "summary": "Captures an initialization delay requirement.",
            "content": "Wait at least 2 ms after reset before sampling status.",
            "evidence_refs": ["section 3.2"],
            "requires_adjudication": false
        }]
    }))
    .expect_err("candidate should be rejected");

    assert!(error
        .to_string()
        .contains("engineering distillation candidate must require adjudication"));
}
