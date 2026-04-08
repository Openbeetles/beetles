//! GET /api/health：仅生成响应体 JSON，配对与写响应在 mod.rs。含 metrics 与 orchestrator resource 快照。
//! Health JSON includes metrics and orchestrator [`crate::orchestrator::ResourceSnapshot`] for UI/ops.

use super::HandlerContext;
use crate::metrics;
use crate::orchestrator;
use crate::runtime;
use crate::state;
use std::sync::atomic::Ordering;

#[derive(serde::Serialize)]
struct DisplayHealth {
    available: bool,
}

#[derive(serde::Serialize)]
struct AudioHealth {
    duplex_profile: crate::platform::AudioDuplexProfile,
    duplex_capabilities: crate::platform::AudioDuplexCapabilities,
}

#[derive(serde::Serialize)]
struct HealthBody {
    wifi: &'static str,
    inbound_depth: usize,
    outbound_depth: usize,
    last_error: String,
    display: DisplayHealth,
    audio: AudioHealth,
    metrics: metrics::MetricsSnapshot,
    resource: orchestrator::ResourceSnapshot,
    threads: runtime::ThreadRegistrySnapshot,
    os_closure: runtime::BeetleOsClosureReport,
    initiative: runtime::InitiativeSnapshot,
    presence: runtime::PresenceSnapshot,
    runtime_mode: runtime::RuntimeModeSnapshot,
    soul_kernel: runtime::SoulKernelStatus,
    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
    #[serde(skip_serializing_if = "Option::is_none")]
    supervisor: Option<crate::runtime::linux_supervisor::LinuxSupervisorStatusSnapshot>,
    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
    #[serde(skip_serializing_if = "Option::is_none")]
    release: Option<crate::runtime::LinuxReleaseStatus>,
}

/// 生成 health JSON body（含 metrics 与 resource 快照，无敏感信息）。
pub fn body(ctx: &HandlerContext) -> Result<String, std::io::Error> {
    let now_secs = crate::util::current_unix_secs();
    let wifi = if crate::state::wifi_sta_connected() {
        "connected"
    } else {
        "disconnected"
    };
    let last_err = state::get_current_error().unwrap_or_else(|| "none".to_string());
    let audio_caps = ctx.platform.audio_duplex_capabilities();
    let presence = runtime::inspect_platform_presence(ctx.platform.as_ref(), now_secs);
    let runtime_mode = presence.runtime_mode;
    let soul_kernel = presence.soul_kernel.clone();
    let initiative = runtime::inspect_platform_initiative(ctx.platform.as_ref(), now_secs);
    let os_closure = runtime::inspect_beetle_os_closure(&presence, &initiative);
    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
    let supervisor = presence.supervisor.clone();
    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
    let release = presence.release.clone();
    let payload = HealthBody {
        wifi,
        inbound_depth: ctx.inbound_depth.load(Ordering::Relaxed),
        outbound_depth: ctx.outbound_depth.load(Ordering::Relaxed),
        last_error: last_err,
        display: DisplayHealth {
            available: ctx.platform.display_available(),
        },
        audio: AudioHealth {
            duplex_profile: audio_caps.profile(),
            duplex_capabilities: audio_caps,
        },
        metrics: metrics::snapshot(),
        resource: orchestrator::snapshot(),
        threads: runtime::thread_registry::snapshot(),
        os_closure,
        initiative,
        runtime_mode,
        soul_kernel,
        presence,
        #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
        supervisor,
        #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
        release,
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
    fn body_keeps_health_contract_fields() {
        let ctx = build_test_context();

        let payload = body(&ctx).unwrap();
        let parsed: Value = serde_json::from_str(&payload).unwrap();

        assert!(parsed.get("metrics").is_some());
        assert!(parsed.get("resource").is_some());
        assert!(parsed.get("runtime_mode").is_some());
        assert!(parsed.get("presence").is_some());
    }

    fn build_test_context() -> crate::platform::http_server::handlers::HandlerContext {
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
        crate::platform::http_server::handlers::HandlerContext {
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
        }
    }
}
