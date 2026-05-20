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
struct CurrentChannelHealth {
    id: String,
}

#[derive(serde::Serialize)]
struct HealthBody {
    status: &'static str,
    runtime_startup: StartupHealthStatus,
    network_status: NetworkHealthStatus,
    last_error: String,
    current_channel: CurrentChannelHealth,
    display: DisplayHealth,
    audio: AudioHealth,
}

#[derive(serde::Serialize)]
struct NetworkHealthStatus {
    stage: crate::state::NetworkWifiStage,
    sta_connected: bool,
    #[serde(rename = "wall_clock_trusted")]
    wall_clock_trustworthy: bool,
}

#[derive(serde::Serialize)]
struct StartupHealthStatus {
    phase: crate::runtime::RuntimeStartupPhase,
    reason: &'static str,
    network_reason: crate::runtime::RuntimeStartupNetworkReason,
    allow_config_recovery_routes: bool,
    allow_default_status_routes: bool,
    allow_display_status_surface: bool,
}

/// 生成 health JSON body（轻量状态摘要，无敏感信息）。
pub fn body(ctx: &HandlerContext) -> Result<String, std::io::Error> {
    crate::platform::refresh_runtime_state();
    let last_err = state::get_current_error().unwrap_or_else(|| "none".to_string());
    let status = if last_err == "none" { "ok" } else { "degraded" };
    let network = crate::state::network_runtime_snapshot(
        crate::platform::time::wall_clock_is_trustworthy(),
        3,
    );
    let startup = crate::runtime::runtime_startup_readiness_snapshot();
    let audio_caps = if crate::compiled_voice_capability() {
        ctx.platform.audio_duplex_capabilities()
    } else {
        crate::platform::AudioDuplexCapabilities::unavailable()
    };
    let current_channel_id = {
        let config = ctx.config();
        crate::normalize_compiled_enabled_channel(&config.enabled_channel).to_string()
    };
    let payload = HealthBody {
        status,
        runtime_startup: StartupHealthStatus {
            phase: startup.phase,
            reason: startup.reason,
            network_reason: startup.network_reason,
            allow_config_recovery_routes: startup.allow_config_recovery_routes,
            allow_default_status_routes: startup.allow_default_status_routes,
            allow_display_status_surface: startup.allow_display_status_surface,
        },
        network_status: NetworkHealthStatus {
            stage: network.last_wifi_stage,
            sta_connected: network.sta_ip_present,
            wall_clock_trustworthy: network.wall_clock_trustworthy,
        },
        last_error: last_err,
        current_channel: CurrentChannelHealth {
            id: current_channel_id,
        },
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
    use serde_json::Value;

    #[test]
    fn body_reports_default_health_state() {
        let _guard = crate::state::test_state_guard();
        crate::state::clear_error_state_for_tests();
        crate::state::set_network_sta_expected(false, false);
        crate::state::clear_wifi_sta_state();
        let ctx = build_test_context();

        let payload = body(&ctx).unwrap();
        let parsed: Value = serde_json::from_str(&payload).unwrap();

        assert_eq!(parsed.get("status").and_then(Value::as_str), Some("ok"));
        assert!(
            parsed.get("wifi").is_none(),
            "health must not expose legacy wifi"
        );
        assert!(
            parsed.get("network").is_none(),
            "health must not expose full network snapshot"
        );
        assert!(
            parsed.get("workflow").is_none(),
            "health must not expose workflow diagnostics"
        );
        assert!(
            parsed.get("runtime_scheduler").is_none(),
            "health must not expose scheduler deep diagnostics"
        );
        assert!(
            parsed.get("runtime_policy").is_none(),
            "health must not expose scheduler policy diagnostics"
        );
        assert!(parsed.get("display").is_some());
        assert!(parsed.get("audio").is_some());
        assert!(parsed.get("runtime_startup").is_some());
        assert!(parsed.get("network_status").is_some());
        assert!(parsed.get("last_error").is_some());
        assert_eq!(
            parsed["network_status"]
                .get("stage")
                .and_then(Value::as_str),
            Some("ap_only")
        );
        assert_eq!(
            parsed["network_status"]
                .get("sta_connected")
                .and_then(Value::as_bool),
            Some(false)
        );
        assert!(parsed["network_status"].get("wall_clock_trusted").is_some());
        assert!(parsed["runtime_startup"].get("phase").is_some());
        assert!(parsed["runtime_startup"].get("reason").is_some());
        assert!(parsed["runtime_startup"].get("network_reason").is_some());
        assert!(parsed["display"]["available"].is_boolean());
        assert!(parsed["audio"]["duplex_profile"].is_string());
        assert!(parsed["audio"]["duplex_capabilities"]
            .get("microphone_input")
            .is_some());
        assert!(parsed["audio"]["duplex_capabilities"]
            .get("speaker_output")
            .is_some());
    }

    #[test]
    fn body_reports_current_channel_from_cached_config() {
        let _guard = crate::state::test_state_guard();
        crate::state::clear_error_state_for_tests();
        let ctx = build_test_context();
        let enabled_channel = crate::compiled_enabled_channel_ids()
            .iter()
            .copied()
            .find(|channel| !channel.is_empty())
            .unwrap_or("");
        ctx.update_cached_config(|config| {
            config.enabled_channel = enabled_channel.to_string();
        });

        let payload = body(&ctx).unwrap();
        let parsed: Value = serde_json::from_str(&payload).unwrap();

        assert_eq!(
            parsed["current_channel"].get("id").and_then(Value::as_str),
            Some(enabled_channel)
        );
        assert!(
            parsed.get("channel_connectivity").is_none(),
            "health must not expose channel connectivity diagnostics"
        );
    }

    fn build_test_context() -> crate::platform::http_server::handlers::HandlerContext {
        crate::platform::http_server::handlers::build_default_test_handler_context()
    }
}
