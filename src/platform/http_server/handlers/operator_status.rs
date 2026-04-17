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
        let _guard = crate::platform::http_server::handlers::default_test_handler_context_guard();
        let ctx = build_test_context();

        let payload = body(&ctx).unwrap();
        let parsed: Value = serde_json::from_str(&payload).unwrap();

        assert!(parsed.get("platform_contract").is_some());
        assert!(parsed.get("build_package").is_some());
        assert!(parsed.get("operator_surface").is_some());
        assert!(parsed.get("reply_pipeline").is_some());
        assert!(parsed.get("delivery_diagnosis").is_some());
        assert!(parsed.get("system_diagnosis").is_some());
        assert!(parsed.get("memory_operator_surface").is_some());
        assert!(parsed.get("workflow").is_some());
        assert!(parsed.get("programmable_reasoning").is_some());
        assert!(parsed["reply_pipeline"]
            .get("request_semantics_last_ms")
            .is_some());
        assert!(parsed["reply_pipeline"].get("tool_exec_last_ms").is_some());
        assert!(parsed["reply_pipeline"].get("dominant_stage").is_some());
        assert_eq!(
            parsed["delivery_diagnosis"]["kind"].as_str(),
            Some("delivery")
        );
        assert!(parsed["delivery_diagnosis"].get("summary").is_some());
        assert_eq!(parsed["system_diagnosis"]["kind"].as_str(), Some("system"));
        assert!(parsed["system_diagnosis"].get("summary").is_some());
        assert_eq!(
            parsed["memory_runtime_diagnosis"]["kind"].as_str(),
            Some("memory_runtime")
        );
        assert!(parsed["memory_runtime_diagnosis"].get("summary").is_some());
        assert_eq!(
            parsed["network_path_diagnosis"]["kind"].as_str(),
            Some("network_path")
        );
        assert!(parsed["network_path_diagnosis"].get("summary").is_some());
        assert_eq!(
            parsed["voice_path_diagnosis"]["kind"].as_str(),
            Some("voice_path")
        );
        assert!(parsed["voice_path_diagnosis"].get("summary").is_some());
        assert!(parsed["workflow"].get("summary").is_some());
        assert!(parsed["workflow"].get("recent_records").is_some());
        assert_eq!(
            parsed["programmable_reasoning"]["stage"].as_str(),
            Some("capability_atoms_exchange")
        );
        assert_eq!(
            parsed["programmable_reasoning"]["runtime_contract"]["execution_enabled"].as_bool(),
            Some(cfg!(target_os = "linux"))
        );
        assert!(parsed["programmable_reasoning"]
            .get("usage_analytics")
            .is_some());
        assert!(parsed["programmable_reasoning"]["usage_analytics"]
            .get("recent_total_attempts")
            .is_some());
        assert!(parsed["programmable_reasoning"]["usage_analytics"]
            .get("recent_succeeded")
            .is_some());
        assert!(parsed["programmable_reasoning"]["usage_analytics"]
            .get("tool_counts")
            .is_some());
        assert!(parsed["programmable_reasoning"].get("timeline").is_some());
        assert!(parsed["programmable_reasoning"]["timeline"]
            .get("recent_events")
            .is_some());
        assert!(parsed["programmable_reasoning"]
            .get("adversarial_arena")
            .is_some());
        assert!(parsed["programmable_reasoning"]["adversarial_arena"]
            .get("summary")
            .is_some());
        assert!(parsed["programmable_reasoning"].get("doctrine").is_some());
        assert!(parsed["programmable_reasoning"]["doctrine"]
            .get("recent_clauses")
            .is_some());
        assert!(parsed["programmable_reasoning"].get("genome").is_some());
        assert!(parsed["programmable_reasoning"]["genome"]
            .get("recent_lineages")
            .is_some());
        assert!(parsed["programmable_reasoning"]
            .get("capability_atoms")
            .is_some());
        assert!(parsed["programmable_reasoning"]["capability_atoms"]
            .get("total")
            .is_some());
        assert!(parsed["programmable_reasoning"]
            .get("product_surface")
            .is_some());
        assert!(parsed["programmable_reasoning"]["product_surface"]
            .get("headline")
            .is_some());
        assert!(parsed["programmable_reasoning"]["product_surface"]
            .get("demo_scenarios")
            .is_some());
        assert!(parsed["programmable_reasoning"].get("inspection").is_some());
        assert!(parsed["programmable_reasoning"]["inspection"]
            .get("doctrine")
            .is_some());
        assert!(parsed["programmable_reasoning"]["inspection"]
            .get("genome")
            .is_some());
        assert!(parsed["programmable_reasoning"]["inspection"]
            .get("tension")
            .is_some());
        assert!(parsed["programmable_reasoning"].get("replay").is_some());
        assert!(parsed["programmable_reasoning"]["replay"]
            .get("recent_branch_replays")
            .is_some());
        assert!(parsed["programmable_reasoning"]["replay"]
            .get("recent_arena_replays")
            .is_some());
        assert!(parsed["programmable_reasoning"]
            .get("maintenance_digest")
            .is_some());
        assert!(parsed["programmable_reasoning"]["maintenance_digest"]
            .get("status")
            .is_some());
        assert!(parsed["programmable_reasoning"]["maintenance_digest"]
            .get("headline")
            .is_some());
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
        let _guard = crate::platform::http_server::handlers::default_test_handler_context_guard();
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
