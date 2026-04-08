//! GET /api/operator/status: unified operator-facing status contract.

use super::HandlerContext;
use crate::platform::operator_status::{build_operator_status, OperatorStatusInput};
use std::sync::atomic::Ordering;

pub fn body(ctx: &HandlerContext) -> Result<String, std::io::Error> {
    let config = ctx.config();
    let current_channel = config.enabled_channel.clone();
    let snapshot = build_operator_status(OperatorStatusInput {
        config: &config,
        platform: ctx.platform.as_ref(),
        tool_registry: ctx.tool_registry.as_ref(),
        channel_capability_registry: ctx.channel_capability_registry.as_ref(),
        capability_package_runtime_capabilities: ctx
            .capability_package_runtime_capabilities
            .as_ref(),
        current_channel: current_channel.as_str(),
        inbound_depth: ctx.inbound_depth.load(Ordering::Relaxed),
        outbound_depth: ctx.outbound_depth.load(Ordering::Relaxed),
        version: ctx.version.as_ref(),
        board_id: ctx.board_id.as_ref(),
        llm_stream_enabled: ctx.llm_stream_enabled,
    })
    .map_err(std::io::Error::other)?;
    drop(config);
    serde_json::to_string(&snapshot).map_err(std::io::Error::other)
}

#[cfg(test)]
mod tests {
    use super::body;
    use crate::config::AppConfig;
    use crate::platform::Platform;
    use serde_json::Value;
    use std::sync::Arc;

    #[test]
    fn body_keeps_operator_status_contract_fields() {
        let ctx = build_test_context();

        let payload = body(&ctx).unwrap();
        let parsed: Value = serde_json::from_str(&payload).unwrap();

        assert!(parsed.get("platform_contract").is_some());
        assert!(parsed.get("build_package").is_some());
        assert!(parsed.get("operator_surface").is_some());
        assert!(parsed.get("os_closure").is_some());
        assert!(parsed.get("initiative").is_some());
        assert!(parsed.get("presence").is_some());
        assert!(parsed.get("runtime_mode").is_some());
        assert!(parsed.get("soul_kernel").is_some());
        assert!(parsed.get("capability_planes").is_some());
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
