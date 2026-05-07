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
        "beetle_user_messages_in_total {}\n",
        snap.user_messages_in
    ));
    buf.push_str(&format!(
        "beetle_agent_messages_in_total {}\n",
        snap.agent_messages_in
    ));
    buf.push_str(&format!(
        "beetle_system_messages_in_total {}\n",
        snap.system_messages_in
    ));
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
        "beetle_voice_input_last_ms {}\n",
        snap.voice_input_last_ms
    ));
    buf.push_str(&format!(
        "beetle_voice_output_last_ms {}\n",
        snap.voice_output_last_ms
    ));
    buf.push_str(&format!(
        "beetle_voice_input_fail_total {}\n",
        snap.voice_input_fail_total
    ));
    buf.push_str(&format!(
        "beetle_voice_output_fail_total {}\n",
        snap.voice_output_fail_total
    ));
    buf.push_str(&format!(
        "beetle_wake_trigger_total {}\n",
        snap.wake_trigger_total
    ));
    buf.push_str(&format!(
        "beetle_storage_lock_ops_total {}\n",
        snap.storage_lock_ops_total
    ));
    buf.push_str(&format!(
        "beetle_storage_lock_contention_total {}\n",
        snap.storage_lock_contention_total
    ));

    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn body_does_not_expose_low_level_audio_metrics() {
        let ctx = crate::platform::http_server::handlers::build_default_test_handler_context();
        let payload = body(&ctx).expect("metrics body");
        let parsed: serde_json::Value = serde_json::from_str(&payload).expect("valid metrics json");
        let keys = parsed.as_object().expect("metrics object").keys();
        for key in keys {
            assert!(
                !key.starts_with("audio_") && !key.starts_with("wake_feed_"),
                "metrics must not expose low-level audio internals: {key}"
            );
        }
    }

    #[test]
    fn body_exposes_only_user_meaningful_voice_audio_metrics() {
        let ctx = crate::platform::http_server::handlers::build_default_test_handler_context();
        let payload = body(&ctx).expect("metrics body");
        let parsed: serde_json::Value = serde_json::from_str(&payload).expect("valid metrics json");

        for key in [
            "voice_input_last_ms",
            "voice_output_last_ms",
            "voice_input_fail_total",
            "voice_output_fail_total",
            "voice_interrupt_total",
            "voice_interrupt_missed_total",
            "voice_no_speech_timeout_total",
            "voice_response_wait_timeout_total",
            "voice_playback_timeout_total",
            "wake_trigger_total",
        ] {
            assert!(
                parsed.get(key).is_some(),
                "metrics must expose user-meaningful voice/audio signal: {key}"
            );
        }

        let prometheus = body_prometheus(&ctx).expect("prometheus metrics body");
        assert!(prometheus.contains("beetle_voice_input_last_ms"));
        assert!(prometheus.contains("beetle_voice_output_last_ms"));
        assert!(!prometheus.contains("beetle_audio_"));
        assert!(!prometheus.contains("beetle_wake_feed_"));
    }

    #[test]
    fn body_does_not_expose_health_or_resource_objects() {
        let ctx = crate::platform::http_server::handlers::build_default_test_handler_context();
        let payload = body(&ctx).expect("metrics body");
        let parsed: serde_json::Value = serde_json::from_str(&payload).expect("valid metrics json");

        for key in [
            "status",
            "network_status",
            "display",
            "audio",
            "pressure",
            "budget",
            "admission",
            "governance_metrics",
            "network_gate_summary",
            "planes",
            "leases",
            "threads",
        ] {
            assert!(
                parsed.get(key).is_none(),
                "metrics must not expose health/resource object: {key}"
            );
        }

        assert!(
            parsed.get("storage_lock_wait_last_us").is_some(),
            "metrics must keep the public storage last-wait signal"
        );
        assert!(
            parsed.get("storage_lock_hold_last_us").is_some(),
            "metrics must keep the public storage last-hold signal"
        );
    }
}
