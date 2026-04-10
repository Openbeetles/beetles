//! GET /api/health：仅生成轻量响应体 JSON，配对与写响应在 mod.rs。
//! Lightweight health JSON for UI status surfaces.

use super::HandlerContext;
use crate::state;

#[derive(serde::Serialize)]
struct DisplayHealth {
    available: bool,
}

#[derive(serde::Serialize)]
struct AudioHealth {
    duplex_profile: crate::platform::AudioDuplexProfile,
    duplex_capabilities: AudioHealthCapabilities,
}

#[derive(serde::Serialize)]
struct AudioHealthCapabilities {
    microphone_input: bool,
    speaker_output: bool,
}

#[derive(serde::Serialize)]
struct HealthBody {
    wifi: &'static str,
    last_error: String,
    display: DisplayHealth,
    audio: AudioHealth,
}

/// 生成 health JSON body（轻量状态摘要，无敏感信息）。
pub fn body(ctx: &HandlerContext) -> Result<String, std::io::Error> {
    let wifi = if crate::state::wifi_sta_connected() {
        "connected"
    } else {
        "disconnected"
    };
    let last_err = state::get_current_error().unwrap_or_else(|| "none".to_string());
    let audio_caps = if crate::compiled_voice_capability() {
        ctx.platform.audio_duplex_capabilities()
    } else {
        crate::platform::AudioDuplexCapabilities::unavailable()
    };
    let payload = HealthBody {
        wifi,
        last_error: last_err,
        display: DisplayHealth {
            available: ctx.platform.display_available(),
        },
        audio: AudioHealth {
            duplex_profile: audio_caps.profile(),
            duplex_capabilities: AudioHealthCapabilities {
                microphone_input: audio_caps.microphone_input,
                speaker_output: audio_caps.speaker_output,
            },
        },
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

        assert_eq!(parsed.get("wifi").and_then(Value::as_str), Some("disconnected"));
        assert!(parsed.get("last_error").is_some());
        assert!(parsed.get("display").is_some());
        assert!(parsed.get("audio").is_some());
        assert!(parsed["audio"]["duplex_capabilities"]
            .get("reference_capture")
            .is_none());
        assert!(parsed["audio"]["duplex_capabilities"]
            .get("concurrent_capture_playback")
            .is_none());
        assert!(parsed["audio"]["duplex_capabilities"]
            .get("barge_in")
            .is_none());
        assert!(parsed["audio"]["duplex_capabilities"]
            .get("echo_cancellation")
            .is_none());
        assert!(parsed.get("metrics").is_none());
        assert!(parsed.get("resource").is_none());
        assert!(parsed.get("inbound_depth").is_none());
        assert!(parsed.get("outbound_depth").is_none());
        assert!(parsed.get("build_package").is_none());
        assert!(parsed.get("capability_planes").is_none());
        assert!(parsed.get("runtime_capabilities").is_none());
        assert!(parsed.get("threads").is_none());
        assert!(parsed.get("os_closure").is_none());
        assert!(parsed.get("initiative").is_none());
        assert!(parsed.get("presence").is_none());
        assert!(parsed.get("runtime_mode").is_none());
        assert!(parsed.get("soul_kernel").is_none());
        assert!(parsed.get("supervisor").is_none());
        assert!(parsed.get("release").is_none());
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
            route_contract: crate::platform::http_server::handlers::ControlPlaneRouteContract::FULL,
        }
    }
}
