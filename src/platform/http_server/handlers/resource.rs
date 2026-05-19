//! GET /api/resource：返回 orchestrator cached-light resource JSON。

use super::HandlerContext;
use crate::orchestrator;

#[derive(serde::Serialize)]
struct ResourceBody<'a> {
    #[serde(flatten)]
    snapshot: &'a orchestrator::ResourceLightSnapshot,
    governance_metrics: orchestrator::ResourceGovernanceMetricsSnapshot,
}

/// 生成 resource JSON body。
pub fn body(_ctx: &HandlerContext) -> Result<String, std::io::Error> {
    let snap = orchestrator::resource_light_snapshot();
    let payload = ResourceBody {
        snapshot: &snap,
        governance_metrics: orchestrator::resource_governance_metrics_snapshot(),
    };
    serde_json::to_string(&payload).map_err(std::io::Error::other)
}

#[cfg(test)]
mod tests {
    use super::body;
    use serde_json::Value;

    #[test]
    fn body_serializes_documented_resource_contract_fields() {
        let _guard = crate::platform::http_server::handlers::default_test_handler_context_guard();
        let _state_guard = crate::state::test_state_guard();
        crate::state::set_network_sta_expected(false, false);
        crate::state::clear_wifi_sta_state();
        crate::orchestrator::reset_crash_metadata_for_tests();
        let ctx = crate::platform::http_server::handlers::build_default_test_handler_context();

        let payload = body(&ctx).expect("resource body");
        let parsed: Value = serde_json::from_str(&payload).expect("valid resource json");

        for key in [
            "pressure",
            "tls_fragmentation_risk",
            "storage_contention_risk",
            "heap_free_internal",
            "heap_min_free_internal",
            "heap_free_spiram",
            "heap_total_spiram",
            "heap_min_free_spiram",
            "heap_largest_block_spiram",
            "heap_used_spiram_est",
            "heap_largest_block_internal",
            "active_http_count",
            "active_wss_count",
            "active_agent_tasks",
            "inbound_depth",
            "outbound_depth",
            "budget",
            "governance_metrics",
            "session_count",
            "storage_used_kb",
            "storage_total_kb",
        ] {
            assert!(parsed.get(key).is_some(), "missing resource field: {key}");
        }

        for key in [
            "network",
            "workflow",
            "last_error",
            "display",
            "audio",
            "firmware_identity",
            "admission",
            "network_gate_summary",
            "leases",
            "display_lease_denied_total",
            "runtime_capabilities",
            "execution_budget",
            "planes",
            "plane_lifecycle",
            "threads",
            "write_back",
            "scheduler",
            "runtime_scheduler",
            "runtime_policy",
            "recent_decisions",
            "crash",
        ] {
            assert!(
                parsed.get(key).is_none(),
                "resource must not expose cross-contract field: {key}"
            );
        }

        assert!(parsed["budget"]["level"].is_string());
        assert!(parsed["heap_min_free_internal"].is_number());
        assert!(parsed["heap_total_spiram"].is_number());
        assert!(parsed["heap_min_free_spiram"].is_number());
        assert!(parsed["heap_largest_block_spiram"].is_number());
        assert!(parsed["heap_used_spiram_est"].is_number());
        assert!(parsed["budget"]["system_prompt_max"].is_number());
        assert!(parsed["budget"]["messages_max"].is_number());
        assert!(parsed["budget"]["response_body_max"].is_number());
        assert!(parsed["budget"]["reconnect_backoff_secs"].is_number());
        assert!(parsed["budget"].get("llm_hint").is_none());
        assert!(parsed["governance_metrics"]["runtime_spawn_failure_total"].is_number());
        assert!(parsed["governance_metrics"]["http_route_reject_total"].is_number());
        assert!(parsed["governance_metrics"]
            .get("lease_conflict_total")
            .is_none());
        assert!(parsed["governance_metrics"]
            .get("lease_expired_replacement_total")
            .is_none());
        assert!(parsed["governance_metrics"]
            .get("plane_drain_timeout_total")
            .is_none());
        assert!(parsed["governance_metrics"]["event_ingress_enqueued_total"].is_number());
        assert!(parsed["governance_metrics"]["event_ingress_rejected_total"].is_number());
        assert!(parsed["governance_metrics"]["event_ingress_purged_total"].is_number());
        assert!(parsed["governance_metrics"]["event_ingress_cancelled_total"].is_number());
        assert!(parsed["governance_metrics"]["event_ingress_stale_drop_total"].is_number());
    }
}
