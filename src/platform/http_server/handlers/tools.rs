//! GET /api/tools: returns available tools with translation keys only.

use super::HandlerContext;

#[derive(serde::Serialize)]
struct ToolInfo {
    name: &'static str,
    i18n_key: String,
}

impl ToolInfo {
    fn new(name: &'static str) -> Self {
        Self {
            name,
            i18n_key: format!("tools.{}", name),
        }
    }
}

fn tool_infos(ctx: &HandlerContext) -> Vec<ToolInfo> {
    ctx.tool_registry
        .tool_names()
        .into_iter()
        .map(ToolInfo::new)
        .collect()
}

/// 生成工具列表 JSON body。
pub fn body(ctx: &HandlerContext) -> Result<String, std::io::Error> {
    serde_json::to_string(&tool_infos(ctx)).map_err(std::io::Error::other)
}

#[cfg(test)]
mod tests {
    use super::tool_infos;
    use crate::config::AppConfig;
    use crate::platform::Platform;
    use serde_json::Value;
    use std::sync::Arc;

    #[test]
    fn tools_api_uses_registry_order() {
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
        let ctx = crate::platform::http_server::handlers::HandlerContext {
            config_store: platform.config_store(),
            config_file_store: Arc::new(crate::config::PlatformConfigFileStore(Arc::clone(
                &platform,
            ))),
            platform: Arc::clone(&platform),
            memory_store: platform.memory_store(),
            session_store: platform.session_store(),
            skill_storage: Arc::clone(&skill_storage),
            skill_meta_store: Arc::clone(&skill_meta_store),
            skill_prompt_cache: Arc::new(crate::skills::SkillPromptCache::new(
                Arc::clone(&skill_meta_store),
                Arc::clone(&skill_storage),
                8192,
            )),
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
            board_id: Arc::from("test"),
            cached_config: Arc::new(std::sync::RwLock::new(config)),
            llm_stream_enabled: false,
            route_contract: crate::platform::http_server::handlers::ControlPlaneRouteContract::FULL,
        };
        let payload = serde_json::to_string(&tool_infos(&ctx)).unwrap();
        let parsed: Value = serde_json::from_str(&payload).unwrap();
        let names: Vec<&str> = parsed
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|item| item.get("name").and_then(Value::as_str))
            .collect();
        assert_eq!(names, ctx.tool_registry.tool_names());
        assert!(!names.is_empty());
    }

    #[test]
    fn tools_api_uses_compact_translation_keys() {
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
        let ctx = crate::platform::http_server::handlers::HandlerContext {
            config_store: platform.config_store(),
            config_file_store: Arc::new(crate::config::PlatformConfigFileStore(Arc::clone(
                &platform,
            ))),
            platform: Arc::clone(&platform),
            memory_store: platform.memory_store(),
            session_store: platform.session_store(),
            skill_storage: Arc::clone(&skill_storage),
            skill_meta_store: Arc::clone(&skill_meta_store),
            skill_prompt_cache: Arc::new(crate::skills::SkillPromptCache::new(
                Arc::clone(&skill_meta_store),
                Arc::clone(&skill_storage),
                8192,
            )),
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
            board_id: Arc::from("test"),
            cached_config: Arc::new(std::sync::RwLock::new(config)),
            llm_stream_enabled: false,
            route_contract: crate::platform::http_server::handlers::ControlPlaneRouteContract::FULL,
        };
        let tools = tool_infos(&ctx);
        let board_info = tools.iter().find(|tool| tool.name == "board_info").unwrap();
        assert_eq!(board_info.i18n_key, "tools.board_info");

        #[cfg(feature = "tools_diagnostics")]
        {
            let network_scan = tools
                .iter()
                .find(|tool| tool.name == "network_scan")
                .unwrap();
            assert_eq!(network_scan.i18n_key, "tools.network_scan");
        }

        #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
        {
            let process = tools.iter().find(|tool| tool.name == "process").unwrap();
            assert_eq!(process.i18n_key, "tools.process");

            let network = tools.iter().find(|tool| tool.name == "network").unwrap();
            assert_eq!(network.i18n_key, "tools.network");
        }
    }
}
