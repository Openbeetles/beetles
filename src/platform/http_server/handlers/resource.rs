//! GET /api/resource：返回 orchestrator cached-light resource JSON。

use super::HandlerContext;
use crate::orchestrator;

#[derive(serde::Serialize)]
struct ResourceBody {
    pressure: orchestrator::pressure::PressureLevel,
    tls_fragmentation_risk: orchestrator::pressure::TlsFragmentationRisk,
    storage_contention_risk: orchestrator::StorageContentionRisk,
    heap_free_internal: u32,
    heap_min_free_internal: u32,
    heap_free_spiram: u32,
    heap_total_spiram: u32,
    heap_min_free_spiram: u32,
    heap_largest_block_spiram: u32,
    heap_used_spiram_est: u32,
    heap_largest_block_internal: u32,
    active_http_count: u32,
    active_wss_count: u32,
    active_agent_tasks: u32,
    inbound_depth: u32,
    outbound_depth: u32,
    budget: ResourceBudgetBody,
    governance_metrics: orchestrator::ResourceGovernanceMetricsSnapshot,
    session_count: u32,
    storage_used_kb: u32,
    storage_total_kb: u32,
    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
    cpu_usage_percent: f32,
    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
    load_average: (f32, f32, f32),
    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
    process_memory_kb: u32,
}

#[derive(serde::Serialize)]
struct ResourceBudgetBody {
    level: orchestrator::pressure::PressureLevel,
    system_prompt_max: usize,
    messages_max: usize,
    response_body_max: usize,
    reconnect_backoff_secs: u64,
}

/// 生成 resource JSON body。
pub fn body(_ctx: &HandlerContext) -> Result<String, std::io::Error> {
    let snap = orchestrator::resource_light_snapshot();
    let payload = ResourceBody {
        pressure: snap.pressure,
        tls_fragmentation_risk: snap.tls_fragmentation_risk,
        storage_contention_risk: snap.storage_contention_risk,
        heap_free_internal: snap.heap_free_internal,
        heap_min_free_internal: snap.heap_min_free_internal,
        heap_free_spiram: snap.heap_free_spiram,
        heap_total_spiram: snap.heap_total_spiram,
        heap_min_free_spiram: snap.heap_min_free_spiram,
        heap_largest_block_spiram: snap.heap_largest_block_spiram,
        heap_used_spiram_est: snap.heap_used_spiram_est,
        heap_largest_block_internal: snap.heap_largest_block_internal,
        active_http_count: snap.active_http_count,
        active_wss_count: snap.active_wss_count,
        active_agent_tasks: snap.active_agent_tasks,
        inbound_depth: snap.inbound_depth,
        outbound_depth: snap.outbound_depth,
        budget: ResourceBudgetBody {
            level: snap.budget.level,
            system_prompt_max: snap.budget.system_prompt_max,
            messages_max: snap.budget.messages_max,
            response_body_max: snap.budget.response_body_max,
            reconnect_backoff_secs: snap.budget.reconnect_backoff_secs,
        },
        governance_metrics: orchestrator::resource_governance_metrics_snapshot(),
        session_count: snap.session_count,
        storage_used_kb: snap.storage_used_kb,
        storage_total_kb: snap.storage_total_kb,
        #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
        cpu_usage_percent: snap.cpu_usage_percent,
        #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
        load_average: snap.load_average,
        #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
        process_memory_kb: snap.process_memory_kb,
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
        assert!(parsed["governance_metrics"]["runtime_spawn_failure_total"].is_number());
        assert!(parsed["governance_metrics"]["http_route_reject_total"].is_number());
        assert!(parsed["governance_metrics"]["lease_conflict_total"].is_number());
        assert!(parsed["governance_metrics"]["lease_expired_replacement_total"].is_number());
        assert!(parsed["governance_metrics"]["plane_drain_timeout_total"].is_number());
        assert!(parsed["governance_metrics"]["event_ingress_enqueued_total"].is_number());
        assert!(parsed["governance_metrics"]["event_ingress_rejected_total"].is_number());
        assert!(parsed["governance_metrics"]["event_ingress_purged_total"].is_number());
        assert!(parsed["governance_metrics"]["event_ingress_cancelled_total"].is_number());
        assert!(parsed["governance_metrics"]["event_ingress_stale_drop_total"].is_number());
    }
}
