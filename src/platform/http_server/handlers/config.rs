//! 配置 API：GET /api/config、POST /api/config/wifi、POST /api/config/llm、/channels、/system、/hardware。

use crate::config;
use crate::i18n::{locale_from_store, tr, tr_error, Message};
use crate::platform::http_server::common::{to_io, ApiResponse, WifiConfigPayload};
use serde_json::Value;

use super::HandlerContext;

/// GET /api/config：从缓存返回完整配置 JSON（含密钥）+ locale。路由层要求配对码。
pub fn get_body(ctx: &HandlerContext) -> Result<String, std::io::Error> {
    let config = ctx.config();
    let mut j: Value = serde_json::to_value(&*config).map_err(|e| to_io(e.to_string()))?;
    j["locale"] = serde_json::Value::String(config::get_locale(ctx.config_store.as_ref()));
    j["build_package"] =
        serde_json::to_value(crate::current_build_package()).map_err(|e| to_io(e.to_string()))?;
    serde_json::to_string(&j).map_err(|e| to_io(e.to_string()))
}

/// POST /api/config/wifi：body 为 JSON，写 WiFi SSID/密码到 NVS。成功时返回 restart_required 提示需重启生效。
pub fn post_wifi(ctx: &HandlerContext, body: &str) -> Result<ApiResponse, std::io::Error> {
    let loc = locale_from_store(ctx.config_store.as_ref());
    let payload: WifiConfigPayload = match serde_json::from_str(body) {
        Ok(p) => p,
        Err(_) => return Ok(ApiResponse::err_400(&tr(Message::InvalidJson, loc))),
    };
    match config::save_wifi_to_nvs(
        ctx.config_store.as_ref(),
        &payload.wifi_ssid,
        &payload.wifi_pass,
    ) {
        Ok(()) => {
            ctx.reload_config();
            Ok(ApiResponse::ok_200_json(
                r#"{"ok":true,"restart_required":true}"#,
            ))
        }
        Err(e) => Ok(ApiResponse::err_400(&tr_error(&e, loc))),
    }
}

/// POST /api/config/llm：仅写 LLM 段，body 为 LlmSegment JSON。
pub fn post_llm(ctx: &HandlerContext, body: &str) -> Result<ApiResponse, std::io::Error> {
    let loc = locale_from_store(ctx.config_store.as_ref());
    match config::save_llm_segment(ctx.config_file_store.as_ref(), body) {
        Ok(()) => {
            ctx.reload_config();
            Ok(ApiResponse::ok_200_json("{\"ok\":true}"))
        }
        Err(e) => Ok(ApiResponse::err_400(&tr_error(&e, loc))),
    }
}

/// POST /api/config/channels：仅写通道段，body 为 ChannelsSegment JSON。
pub fn post_channels(ctx: &HandlerContext, body: &str) -> Result<ApiResponse, std::io::Error> {
    let loc = locale_from_store(ctx.config_store.as_ref());
    match config::save_channels_segment(ctx.config_file_store.as_ref(), body) {
        Ok(()) => {
            ctx.reload_config();
            Ok(ApiResponse::ok_200_json("{\"ok\":true}"))
        }
        Err(e) => Ok(ApiResponse::err_400(&tr_error(&e, loc))),
    }
}

/// POST /api/config/system：仅写系统段（wifi/proxy/session/tg_group/locale），body 为 SystemSegment JSON。
pub fn post_system(ctx: &HandlerContext, body: &str) -> Result<ApiResponse, std::io::Error> {
    let loc = locale_from_store(ctx.config_store.as_ref());
    match config::save_system_segment_to_nvs(ctx.config_store.as_ref(), body) {
        Ok(()) => {
            ctx.reload_config();
            Ok(ApiResponse::ok_200_json("{\"ok\":true}"))
        }
        Err(e) => Ok(ApiResponse::err_400(&tr_error(&e, loc))),
    }
}

/// GET /api/config/accounts：返回 OfficeAccountsSegment JSON（文件不存在时返回空默认配置）。
pub fn get_accounts_body(ctx: &HandlerContext) -> Result<String, std::io::Error> {
    config::get_office_accounts_segment(ctx.config_file_store.as_ref())
        .map_err(|e| to_io(e.to_string()))
}

/// POST /api/config/accounts：仅写办公账户段，body 为 OfficeAccountsSegment JSON。
pub fn post_accounts(ctx: &HandlerContext, body: &str) -> Result<ApiResponse, std::io::Error> {
    let loc = locale_from_store(ctx.config_store.as_ref());
    match config::save_office_accounts_segment(ctx.config_file_store.as_ref(), body) {
        Ok(()) => {
            ctx.reload_config();
            Ok(ApiResponse::ok_200_json("{\"ok\":true}"))
        }
        Err(e) => Ok(ApiResponse::err_400(&tr_error(&e, loc))),
    }
}

/// GET /api/config/office_credentials：返回 OfficeCredentialsSegment JSON。
pub fn get_office_credentials_body(ctx: &HandlerContext) -> Result<String, std::io::Error> {
    config::get_office_credentials_segment(ctx.platform.office_credential_store().as_ref())
        .map_err(|e| to_io(e.to_string()))
}

/// POST /api/config/office_credentials：仅写办公 credential 段。
pub fn post_office_credentials(
    ctx: &HandlerContext,
    body: &str,
) -> Result<ApiResponse, std::io::Error> {
    let loc = locale_from_store(ctx.config_store.as_ref());
    match config::save_office_credentials_segment(ctx.platform.office_credential_store().as_ref(), body)
    {
        Ok(()) => Ok(ApiResponse::ok_200_json("{\"ok\":true}")),
        Err(e) => Ok(ApiResponse::err_400(&tr_error(&e, loc))),
    }
}

/// GET /api/config/hardware：返回 HardwareSegment JSON（文件不存在时返回空 devices）。路由层要求配对码。
pub fn get_hardware_body(ctx: &HandlerContext) -> Result<String, std::io::Error> {
    match ctx
        .config_file_store
        .read_config_file("config/hardware.json")
    {
        Ok(Some(b)) => {
            let s = String::from_utf8_lossy(&b);
            Ok(s.into_owned())
        }
        Ok(None) => Ok(r#"{"hardware_devices":[]}"#.to_string()),
        Err(e) => Err(to_io(e.to_string())),
    }
}

/// POST /api/config/hardware：校验并写入 HardwareSegment 到 SPIFFS config/hardware.json。
pub fn post_hardware(ctx: &HandlerContext, body: &str) -> Result<ApiResponse, std::io::Error> {
    let loc = locale_from_store(ctx.config_store.as_ref());
    match config::save_hardware_segment(ctx.config_file_store.as_ref(), body) {
        Ok(()) => {
            ctx.reload_config();
            Ok(ApiResponse::ok_200_json("{\"ok\":true}"))
        }
        Err(e) => Ok(ApiResponse::err_400(&tr_error(&e, loc))),
    }
}

/// GET /api/config/audio：返回 AudioSegment JSON（文件不存在时返回 disabled 默认配置）。路由层要求配对码。
pub fn get_audio_body(ctx: &HandlerContext) -> Result<String, std::io::Error> {
    config::get_audio_segment(ctx.config_file_store.as_ref()).map_err(|e| to_io(e.to_string()))
}

/// POST /api/config/audio：校验并写入 AudioSegment 到 SPIFFS config/audio.json。
pub fn post_audio(ctx: &HandlerContext, body: &str) -> Result<ApiResponse, std::io::Error> {
    let loc = locale_from_store(ctx.config_store.as_ref());
    match config::save_audio_segment(ctx.config_file_store.as_ref(), body) {
        Ok(()) => {
            ctx.reload_config();
            Ok(ApiResponse::ok_200_json(
                r#"{"ok":true,"restart_required":true}"#,
            ))
        }
        Err(e) => Ok(ApiResponse::err_400(&tr_error(&e, loc))),
    }
}

/// GET /api/config/display：返回 DisplayConfig JSON（文件不存在时返回 disabled 默认配置）。路由层要求配对码。
pub fn get_display_body(ctx: &HandlerContext) -> Result<String, std::io::Error> {
    config::get_display_segment(ctx.config_file_store.as_ref()).map_err(|e| to_io(e.to_string()))
}

/// POST /api/config/display：校验并写入 DisplayConfig 到 SPIFFS config/display.json。
pub fn post_display(ctx: &HandlerContext, body: &str) -> Result<ApiResponse, std::io::Error> {
    let loc = locale_from_store(ctx.config_store.as_ref());
    let hw_devices = ctx.config().hardware_devices.clone();
    match config::save_display_segment(ctx.config_file_store.as_ref(), &hw_devices, body) {
        Ok(()) => {
            ctx.reload_config();
            Ok(ApiResponse::ok_200_json(
                r#"{"ok":true,"restart_required":true}"#,
            ))
        }
        Err(e) => Ok(ApiResponse::err_400(&tr_error(&e, loc))),
    }
}

#[cfg(test)]
mod tests {
    use super::get_body;
    use crate::config;
    use serde_json::Value;

    #[test]
    fn get_body_returns_compact_json_and_forced_locale() {
        let ctx = build_test_context();
        config::set_locale(ctx.config_store.as_ref(), "en").unwrap();

        let body = get_body(&ctx).unwrap();
        let parsed: Value = serde_json::from_str(&body).unwrap();

        assert_eq!(parsed["locale"], "en");
        assert!(parsed.get("wifi_ssid").is_some());
        assert!(parsed.get("build_package").is_some());
        assert!(parsed["build_package"].get("profile").is_some());
        assert!(parsed["build_package"]
            .get("default_full_package")
            .is_some());
        assert!(parsed["build_package"]["capabilities"]
            .get("voice")
            .is_some());
        assert!(parsed["build_package"]["capabilities"]
            .get("vision")
            .is_some());
        assert!(parsed["build_package"]["capabilities"]
            .get("sensor")
            .is_some());
        assert!(
            !body.contains('\n'),
            "config response should stay compact on ESP default path"
        );
    }

    fn build_test_context() -> crate::platform::http_server::handlers::HandlerContext {
        crate::platform::http_server::handlers::build_default_test_handler_context()
    }
}
