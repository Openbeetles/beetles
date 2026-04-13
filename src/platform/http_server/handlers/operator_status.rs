//! GET /api/operator/status: unified operator-facing status contract.

use super::HandlerContext;
use crate::platform::operator_status::{build_operator_status, OperatorStatusInput};

pub fn body(ctx: &HandlerContext) -> Result<String, std::io::Error> {
    let config = ctx.config();
    let snapshot = build_operator_status(OperatorStatusInput {
        config: &config,
        platform: ctx.platform.as_ref(),
        tool_registry: ctx.tool_registry.as_ref(),
    })
    .map_err(std::io::Error::other)?;
    drop(config);
    serde_json::to_string(&snapshot).map_err(std::io::Error::other)
}

#[cfg(test)]
mod tests {
    use super::body;
    use serde_json::Value;

    #[test]
    fn body_keeps_operator_status_contract_fields() {
        let ctx = build_test_context();

        let payload = body(&ctx).unwrap();
        let parsed: Value = serde_json::from_str(&payload).unwrap();

        assert!(parsed.get("platform_contract").is_some());
        assert!(parsed.get("build_package").is_some());
        assert!(parsed.get("operator_surface").is_some());
        assert!(parsed.get("memory_operator_surface").is_some());
        assert!(parsed.get("workflow").is_some());
        assert!(parsed.get("programmable_reasoning").is_some());
        assert!(parsed["workflow"].get("summary").is_some());
        assert!(parsed["workflow"].get("recent_records").is_some());
        assert_eq!(
            parsed["programmable_reasoning"]["stage"].as_str(),
            Some("experience_crystal")
        );
        assert_eq!(
            parsed["programmable_reasoning"]["runtime_contract"]["execution_enabled"].as_bool(),
            Some(cfg!(target_os = "linux"))
        );
        assert!(parsed["memory_operator_surface"].get("inspect").is_some());
        assert!(parsed["memory_operator_surface"].get("trace").is_some());
        assert!(parsed["memory_operator_surface"].get("diff").is_some());
        assert!(parsed["memory_operator_surface"].get("repair").is_some());
        assert!(parsed["memory_operator_surface"].get("forge").is_some());
        assert!(parsed["memory_operator_surface"]["forge"]
            .get("attack_findings")
            .is_some());
        assert!(parsed["memory_operator_surface"]["forge"]
            .get("distillation_candidates")
            .is_some());
        assert!(parsed["memory_operator_surface"]
            .get("policy_view")
            .is_some());
        assert!(parsed.get("os_closure").is_some());
        assert!(parsed.get("initiative").is_some());
        assert!(parsed.get("presence").is_some());
        assert!(parsed.get("runtime_mode").is_some());
        assert!(parsed.get("soul_kernel").is_some());
        assert!(parsed.get("capability_planes").is_some());
        assert!(parsed.get("continuity_tooling").is_none());
        assert!(parsed.get("task_execution").is_none());
        assert!(parsed.get("personality_governance").is_none());
        assert!(parsed.get("capability_packages").is_none());
        assert!(parsed.get("tools").is_none());
        assert!(parsed.get("metrics").is_none());
        assert!(parsed.get("resource").is_none());
        assert!(parsed.get("channels").is_none());
        assert!(parsed.get("inbound_depth").is_none());
        assert!(parsed.get("outbound_depth").is_none());
        assert!(parsed.get("last_error").is_none());
        assert!(parsed["platform_contract"].get("board_id").is_none());
        assert!(parsed["platform_contract"]
            .get("firmware_version")
            .is_none());
        assert!(parsed["platform_contract"].get("wifi_connected").is_none());
        assert!(parsed["platform_contract"]
            .get("display_available")
            .is_none());
        assert!(parsed["platform_contract"].get("ota_supported").is_none());
        assert!(parsed["platform_contract"]
            .get("audio_duplex_profile")
            .is_none());
        assert!(parsed["platform_contract"]
            .get("storage_media_count")
            .is_none());
        assert!(parsed["platform_contract"]
            .get("storage_media_error")
            .is_none());
    }

    #[test]
    fn body_keeps_os_closure_consistent_with_presence_and_runtime_mode() {
        let ctx = build_test_context();

        let payload = body(&ctx).unwrap();
        let parsed: Value = serde_json::from_str(&payload).unwrap();

        let os_closure = &parsed["os_closure"];
        assert_eq!(
            os_closure["current_mode"].as_str(),
            parsed["runtime_mode"]["current_mode"].as_str()
        );
        assert_eq!(
            os_closure["presence_state"].as_str(),
            parsed["presence"]["state"].as_str()
        );
        assert_eq!(
            parsed["soul_kernel"]["minimum_viable"].as_bool(),
            parsed["presence"]["soul_kernel"]["minimum_viable"].as_bool()
        );
    }

    fn build_test_context() -> crate::platform::http_server::handlers::HandlerContext {
        crate::platform::http_server::handlers::build_default_test_handler_context()
    }
}
