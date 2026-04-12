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
    workflow: crate::runtime::WorkflowAuditSummary,
}

/// 生成 health JSON body（轻量状态摘要，无敏感信息）。
pub fn body(ctx: &HandlerContext) -> Result<String, std::io::Error> {
    crate::platform::refresh_runtime_state();
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
        workflow: crate::runtime::workflow_audit_snapshot(8).summary,
    };
    serde_json::to_string(&payload).map_err(std::io::Error::other)
}

#[cfg(test)]
mod tests {
    use super::body;
    use serde_json::Value;

    #[test]
    fn body_keeps_health_contract_fields() {
        let ctx = build_test_context();

        let payload = body(&ctx).unwrap();
        let parsed: Value = serde_json::from_str(&payload).unwrap();

        assert_eq!(
            parsed.get("wifi").and_then(Value::as_str),
            Some("disconnected")
        );
        assert!(parsed.get("last_error").is_some());
        assert!(parsed.get("display").is_some());
        assert!(parsed.get("audio").is_some());
        assert!(parsed.get("workflow").is_some());
        assert!(parsed["workflow"].get("executed").is_some());
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
        crate::platform::http_server::handlers::build_default_test_handler_context()
    }
}
