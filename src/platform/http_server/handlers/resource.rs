//! GET /api/resource：返回 orchestrator ResourceSnapshot JSON。

use super::HandlerContext;
use crate::orchestrator;
use crate::runtime;

#[derive(serde::Serialize)]
struct ResourceBody {
    pressure: orchestrator::pressure::PressureLevel,
    tls_fragmentation_risk: orchestrator::pressure::TlsFragmentationRisk,
    storage_contention_risk: orchestrator::StorageContentionRisk,
    heap_free_internal: u32,
    heap_free_spiram: u32,
    heap_largest_block_internal: u32,
    active_http_count: u32,
    active_wss_count: u32,
    active_agent_tasks: u32,
    inbound_depth: u32,
    outbound_depth: u32,
    budget: ResourceBudgetBody,
    admission: orchestrator::ResourceAdmissionSnapshot,
    governance_metrics: orchestrator::ResourceGovernanceMetricsSnapshot,
    planes: runtime::PlaneRegistrySnapshot,
    plane_lifecycle: runtime::PlaneLifecycleSnapshot,
    leases: runtime::LeaseSnapshot,
    threads: runtime::thread_registry::ThreadRegistrySnapshot,
    display_lease_denied_total: u64,
    write_back: runtime::write_back::WriteBackSnapshot,
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
    let diag = orchestrator::resource_diagnostic_snapshot();
    let snap = diag.resource;
    let payload = ResourceBody {
        pressure: snap.pressure,
        tls_fragmentation_risk: snap.tls_fragmentation_risk,
        storage_contention_risk: snap.storage_contention_risk,
        heap_free_internal: snap.heap_free_internal,
        heap_free_spiram: snap.heap_free_spiram,
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
        admission: diag.admission,
        governance_metrics: diag.governance_metrics,
        planes: diag.planes,
        plane_lifecycle: diag.plane_lifecycle,
        leases: diag.leases,
        threads: diag.threads,
        display_lease_denied_total: diag.display_lease_denied_total,
        write_back: diag.write_back,
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
        let ctx = crate::platform::http_server::handlers::build_default_test_handler_context();

        let payload = body(&ctx).expect("resource body");
        let parsed: Value = serde_json::from_str(&payload).expect("valid resource json");

        for key in [
            "pressure",
            "tls_fragmentation_risk",
            "storage_contention_risk",
            "heap_free_internal",
            "heap_free_spiram",
            "heap_largest_block_internal",
            "active_http_count",
            "active_wss_count",
            "active_agent_tasks",
            "inbound_depth",
            "outbound_depth",
            "budget",
            "admission",
            "governance_metrics",
            "planes",
            "plane_lifecycle",
            "leases",
            "threads",
            "display_lease_denied_total",
            "write_back",
            "session_count",
            "storage_used_kb",
            "storage_total_kb",
        ] {
            assert!(parsed.get(key).is_some(), "missing resource field: {key}");
        }

        assert!(parsed["budget"]["level"].is_string());
        assert!(parsed["budget"]["system_prompt_max"].is_number());
        assert!(parsed["budget"]["messages_max"].is_number());
        assert!(parsed["budget"]["response_body_max"].is_number());
        assert!(parsed["budget"]["reconnect_backoff_secs"].is_number());
        assert!(parsed["admission"]["active_http_count"].is_number());
        assert!(parsed["admission"]["http_permit_wait_last_ms"].is_number());
        assert!(parsed["admission"]["http_route_queue_wait_last_ms"].is_number());
        assert!(parsed["admission"]["http_route_handler_last_ms"].is_number());
        assert!(parsed["admission"]["http_route_timeout_total"].is_number());
        assert!(parsed["governance_metrics"]["runtime_spawn_failure_total"].is_number());
        assert!(parsed["governance_metrics"]["http_route_reject_total"].is_number());
        assert!(parsed["governance_metrics"]["lease_conflict_total"].is_number());
        assert!(parsed["governance_metrics"]["lease_expired_replacement_total"].is_number());
        assert!(parsed["governance_metrics"]["plane_drain_timeout_total"].is_number());
        assert!(parsed["planes"]["profile_count"].is_number());
        assert!(parsed["planes"]["lease_reference_count"].is_number());
        assert!(parsed["planes"].get("profiles").is_none());
        assert!(parsed["plane_lifecycle"]["total_records"].is_number());
        assert!(parsed["plane_lifecycle"]["records"].is_array());
        assert!(parsed["leases"]["active_count"].is_number());
        assert!(parsed["leases"]["records"].is_array());
        assert!(parsed["threads"]["alive_threads"].is_number());
        assert!(parsed["threads"]["details"].is_array());
        assert!(parsed["display_lease_denied_total"].is_number());
        assert!(parsed["write_back"]["queued"].is_number());
        assert!(parsed["write_back"]["deferred_total"].is_number());
    }
}
