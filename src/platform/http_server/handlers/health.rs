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
    network: crate::state::NetworkRuntimeSnapshot,
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
        network: crate::state::network_runtime_snapshot(
            crate::platform::time::wall_clock_is_trustworthy(),
            3,
        ),
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
    fn body_reports_default_health_state() {
        let _guard = crate::state::test_state_guard();
        crate::state::set_network_sta_expected(false, false);
        crate::state::clear_wifi_sta_state();
        let ctx = build_test_context();

        let payload = body(&ctx).unwrap();
        let parsed: Value = serde_json::from_str(&payload).unwrap();

        assert_eq!(
            parsed.get("wifi").and_then(Value::as_str),
            Some("disconnected")
        );
        assert!(parsed.get("display").is_some());
        assert!(parsed.get("audio").is_some());
        assert!(parsed.get("workflow").is_some());
        assert!(parsed.get("network").is_some());
        assert!(parsed.get("last_error").is_some());
        assert_eq!(
            parsed["network"]
                .get("last_wifi_stage")
                .and_then(Value::as_str),
            Some("ap_only")
        );
        assert!(parsed["display"]["available"].is_boolean());
        assert!(parsed["audio"]["duplex_profile"].is_string());
        assert!(parsed["audio"]["duplex_capabilities"]
            .get("microphone_input")
            .is_some());
        assert!(parsed["audio"]["duplex_capabilities"]
            .get("speaker_output")
            .is_some());
        assert!(parsed["workflow"]["executed"].is_number());
    }

    fn build_test_context() -> crate::platform::http_server::handlers::HandlerContext {
        crate::platform::http_server::handlers::build_default_test_handler_context()
    }
}
