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
    #[test]
    fn body_does_not_expose_speaker_queue_depth_metrics() {
        let ctx = crate::platform::http_server::handlers::build_default_test_handler_context();
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
