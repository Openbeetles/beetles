//! GET /api/resource：返回 orchestrator ResourceSnapshot JSON。

use super::HandlerContext;
use crate::orchestrator;

#[derive(serde::Serialize)]
struct ResourceBody {
    pressure: orchestrator::pressure::PressureLevel,
    tls_fragmentation_risk: orchestrator::pressure::TlsFragmentationRisk,
    heap_free_internal: u32,
    heap_free_spiram: u32,
    heap_largest_block_internal: u32,
    active_http_count: u32,
    active_wss_count: u32,
    active_agent_tasks: u32,
    inbound_depth: u32,
    outbound_depth: u32,
    budget: ResourceBudgetBody,
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
    let snap = orchestrator::snapshot();
    let payload = ResourceBody {
        pressure: snap.pressure,
        tls_fragmentation_risk: snap.tls_fragmentation_risk,
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
    use crate::config::AppConfig;
    use crate::platform::Platform;
    use serde_json::Value;
    use std::sync::Arc;

    #[test]
    fn body_keeps_resource_contract_without_channels() {
        let config = AppConfig::load_from_env();
        let platform: Arc<dyn Platform> = Arc::new(crate::platform::LinuxPlatform::new());
        let (registry, _) = crate::tools::build_default_registry(
            &config,
            crate::tools::DefaultRegistryDeps {
                platform: Arc::clone(&platform),
                remind_at_store: platform.remind_at_store(),
                session_store: platform.session_store(),
                memory_store: platform.memory_store(),
                long_term_memory_store: platform.long_term_memory_store(),
                turn_ledger_store: platform.turn_ledger_store(),
                private_garden_store: platform.private_garden_store(),
                config_store: platform.config_store(),
            },
        );
        let channel_capability_registry =
            Arc::new(crate::build_channel_capability_registry(&config, false));
        let skill_storage = platform.skill_storage();
        let skill_meta_store = platform.skill_meta_store();
        let skill_prompt_cache = Arc::new(crate::skills::SkillPromptCache::new(
            Arc::clone(&skill_meta_store),
            Arc::clone(&skill_storage),
            8192,
        ));
        let ctx = crate::platform::http_server::handlers::HandlerContext {
            config_store: platform.config_store(),
            config_file_store: Arc::new(crate::config::PlatformConfigFileStore(Arc::clone(
                &platform,
            ))),
            platform: Arc::clone(&platform),
            memory_store: platform.memory_store(),
            session_store: platform.session_store(),
            skill_storage,
            skill_meta_store,
            skill_prompt_cache,
            tool_registry: Arc::new(registry),
            channel_capability_registry: Arc::clone(&channel_capability_registry),
            capability_package_runtime_capabilities: Arc::new(
                crate::build_capability_package_runtime_capabilities(
                    channel_capability_registry.as_ref(),
                    false,
                ),
            ),
            inbound_depth: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            outbound_depth: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            version: Arc::from("0.0.0"),
            board_id: Arc::from("test-board"),
            cached_config: Arc::new(std::sync::RwLock::new(config)),
            llm_stream_enabled: false,
            route_contract: crate::platform::http_server::handlers::ControlPlaneRouteContract::FULL,
        };

        let payload = body(&ctx).unwrap();
        let parsed: Value = serde_json::from_str(&payload).unwrap();

        assert!(parsed.get("pressure").is_some());
        assert!(parsed.get("budget").is_some());
        assert!(parsed.get("inbound_depth").is_some());
        assert!(parsed.get("outbound_depth").is_some());
        assert!(parsed.get("session_count").is_some());
        assert!(parsed.get("channels").is_none());
        assert!(parsed.get("audio_recording").is_none());
        assert!(parsed.get("audio_playing").is_none());
        assert!(parsed.get("audio_interrupt_listening").is_none());
        assert!(parsed.get("audio_interrupt_requested").is_none());
        assert!(parsed["budget"].get("llm_hint").is_none());
    }
}
