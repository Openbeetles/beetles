//! GET /api/metrics：返回 MetricsSnapshot JSON 或 Prometheus 文本格式。

use super::HandlerContext;
use crate::metrics;

/// 生成 metrics JSON body。
pub fn body(_ctx: &HandlerContext) -> Result<String, std::io::Error> {
    let snap = metrics::snapshot();
    serde_json::to_string(&snap).map_err(std::io::Error::other)
}

/// 生成 Prometheus 文本格式 body。
pub fn body_prometheus(_ctx: &HandlerContext) -> Result<String, std::io::Error> {
    let snap = metrics::snapshot();
    let mut buf = String::with_capacity(2048);

    // Counters
    buf.push_str(&format!("beetle_messages_in_total {}\n", snap.messages_in));
    buf.push_str(&format!(
        "beetle_messages_out_total {}\n",
        snap.messages_out
    ));
    buf.push_str(&format!("beetle_llm_calls_total {}\n", snap.llm_calls));
    buf.push_str(&format!("beetle_llm_errors_total {}\n", snap.llm_errors));
    buf.push_str(&format!("beetle_tool_calls_total {}\n", snap.tool_calls));
    buf.push_str(&format!("beetle_tool_errors_total {}\n", snap.tool_errors));
    buf.push_str(&format!(
        "beetle_dispatch_send_ok_total {}\n",
        snap.dispatch_send_ok
    ));
    buf.push_str(&format!(
        "beetle_dispatch_send_fail_total {}\n",
        snap.dispatch_send_fail
    ));

    // Gauges (last values)
    buf.push_str(&format!("beetle_llm_last_ms {}\n", snap.llm_last_ms));
    buf.push_str(&format!("beetle_e2e_last_ms {}\n", snap.e2e_last_ms));
    buf.push_str(&format!(
        "beetle_post_reply_last_ms {}\n",
        snap.post_reply_last_ms
    ));
    buf.push_str(&format!(
        "beetle_user_queue_wait_last_ms {}\n",
        snap.user_queue_wait_last_ms
    ));

    // Errors by stage
    buf.push_str(&format!(
        "beetle_errors_agent_chat_total {}\n",
        snap.errors_agent_chat
    ));
    buf.push_str(&format!(
        "beetle_errors_tool_execute_total {}\n",
        snap.errors_tool_execute
    ));
    buf.push_str(&format!(
        "beetle_errors_llm_request_total {}\n",
        snap.errors_llm_request
    ));
    buf.push_str(&format!(
        "beetle_errors_channel_dispatch_total {}\n",
        snap.errors_channel_dispatch
    ));
    buf.push_str(&format!(
        "beetle_audio_worker_turns_total {}\n",
        snap.audio_worker_turns_total
    ));
    buf.push_str(&format!(
        "beetle_audio_worker_idle_turns_total {}\n",
        snap.audio_worker_idle_turns_total
    ));
    buf.push_str(&format!(
        "beetle_wake_word_feed_calls_total {}\n",
        snap.wake_word_feed_calls_total
    ));
    buf.push_str(&format!(
        "beetle_spiffs_lock_ops_total {}\n",
        snap.spiffs_lock_ops_total
    ));
    buf.push_str(&format!(
        "beetle_spiffs_lock_contention_total {}\n",
        snap.spiffs_lock_contention_total
    ));

    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppConfig;
    use crate::platform::Platform;
    use std::sync::Arc;

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

    #[test]
    fn body_does_not_expose_speaker_queue_depth_metrics() {
        let ctx = build_test_context();
        let body = body(&ctx).expect("metrics body");
        assert!(
            !body.contains("audio_speaker_queue_depth_last_samples"),
            "metrics body should not expose speaker depth last samples"
        );
        assert!(
            !body.contains("audio_speaker_queue_depth_min_samples"),
            "metrics body should not expose speaker depth min samples"
        );
        assert!(
            !body.contains("audio_speaker_underrun_total"),
            "metrics body should not expose speaker underrun total"
        );
    }
}
