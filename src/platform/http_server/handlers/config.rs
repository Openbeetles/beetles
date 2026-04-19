//! 配置 API：GET /api/config、POST /api/config/wifi、POST /api/config/llm、/channels、/system、/hardware。

use crate::config;
use crate::i18n::{locale_from_store, tr, tr_error, Message};
#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
use crate::office::{
    parse_public_account_upsert_request_value, OfficeAccountConfigSaveRequest, OfficeCapability,
    OfficeConfigAccountDetail, OfficeConfigAccountSummary, OfficeConfigCapabilityStatus,
    OfficeConfigManagementService, OfficeConfigProviderCatalogItem,
};
use crate::platform::http_server::common::{to_io, ApiResponse, WifiConfigPayload};
use serde_json::Value;
#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
use std::sync::Arc;

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

#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
#[derive(serde::Serialize)]
struct AccountSummaryListResponse {
    count: usize,
    items: Vec<OfficeConfigAccountSummary>,
}

#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
#[derive(serde::Serialize)]
struct CapabilityStatusListResponse {
    count: usize,
    items: Vec<OfficeConfigCapabilityStatus>,
}

#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
#[derive(serde::Serialize)]
struct ProviderCatalogListResponse {
    count: usize,
    items: Vec<OfficeConfigProviderCatalogItem>,
}

#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
#[derive(serde::Deserialize)]
struct RevokeRequest {
    #[serde(default = "default_true")]
    clear_runtime_status: bool,
}

#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
fn default_true() -> bool {
    true
}

#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
fn office_config_service(ctx: &HandlerContext) -> OfficeConfigManagementService {
    OfficeConfigManagementService::new(
        Arc::clone(&ctx.config_file_store),
        ctx.platform.office_credential_store(),
        ctx.platform.office_runtime_status_store(),
    )
    .with_default_probe_adapters()
}

#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
fn parse_capability(capability: Option<&str>) -> Result<Option<OfficeCapability>, std::io::Error> {
    let Some(capability) = capability else {
        return Ok(None);
    };
    let value = match capability.trim() {
        "mail" => OfficeCapability::Mail,
        "calendar" => OfficeCapability::Calendar,
        "documents" => OfficeCapability::Documents,
        "contacts_directory" => OfficeCapability::ContactsDirectory,
        other => {
            return Err(to_io(format!("unknown office capability '{}'", other)));
        }
    };
    Ok(Some(value))
}

#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
/// GET /api/config/accounts：返回账户配置摘要列表。
pub fn get_accounts_body(
    ctx: &HandlerContext,
    provider_kind: Option<&str>,
    capability: Option<&str>,
) -> Result<String, std::io::Error> {
    let items = office_config_service(ctx)
        .account_summaries(provider_kind, parse_capability(capability)?)
        .map_err(|e| to_io(e.to_string()))?;
    serde_json::to_string(&AccountSummaryListResponse {
        count: items.len(),
        items,
    })
    .map_err(|e| to_io(e.to_string()))
}

#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
/// GET /api/config/providers：返回账户创建所需的 provider catalog。
pub fn get_providers_body(
    ctx: &HandlerContext,
    capability: Option<&str>,
) -> Result<String, std::io::Error> {
    let items = office_config_service(ctx)
        .provider_catalog(parse_capability(capability)?)
        .map_err(|e| to_io(e.to_string()))?;
    serde_json::to_string(&ProviderCatalogListResponse {
        count: items.len(),
        items,
    })
    .map_err(|e| to_io(e.to_string()))
}

#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
/// GET /api/config/capabilities：返回能力配置读模型。
pub fn get_capabilities_body(ctx: &HandlerContext) -> Result<String, std::io::Error> {
    let items = office_config_service(ctx)
        .capability_statuses(None)
        .map_err(|e| to_io(e.to_string()))?;
    serde_json::to_string(&CapabilityStatusListResponse {
        count: items.len(),
        items,
    })
    .map_err(|e| to_io(e.to_string()))
}

#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
/// GET /api/config/capabilities/:capability：返回单能力配置读模型。
pub fn get_capability_detail_body(
    ctx: &HandlerContext,
    capability: &str,
) -> Result<String, std::io::Error> {
    let capability =
        parse_capability(Some(capability))?.ok_or_else(|| to_io("missing office capability"))?;
    let status = office_config_service(ctx)
        .capability_statuses(Some(capability))
        .map_err(|e| to_io(e.to_string()))?
        .into_iter()
        .next()
        .ok_or_else(|| to_io(format!("missing capability status for '{capability:?}'")))?;
    serde_json::to_string(&status).map_err(|e| to_io(e.to_string()))
}

#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
/// POST /api/config/accounts：创建或更新单个账户注册。
pub fn post_accounts(ctx: &HandlerContext, body: &str) -> Result<ApiResponse, std::io::Error> {
    let loc = locale_from_store(ctx.config_store.as_ref());
    let request = match serde_json::from_str::<Value>(body) {
        Ok(Value::Object(obj)) => {
            match parse_public_account_upsert_request_value(&obj, "http_config_accounts_post") {
                Ok(request) => request,
                Err(error) => return Ok(ApiResponse::err_400(&error.to_string())),
            }
        }
        Err(error) => return Ok(ApiResponse::err_400(&error.to_string())),
        Ok(_) => {
            return Ok(ApiResponse::err_400(
                "account create body must be a JSON object",
            ))
        }
    };
    match office_config_service(ctx).save_account_upsert(&request) {
        Ok(detail) => {
            ctx.reload_config();
            let body = serde_json::to_string(&detail).map_err(|error| to_io(error.to_string()))?;
            Ok(ApiResponse::ok_200_json(&body))
        }
        Err(e) => Ok(ApiResponse::err_400(&tr_error(&e, loc))),
    }
}

#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
/// GET /api/config/accounts/:account_key：返回单账户详情和编辑字段。
pub fn get_account_detail_body(
    ctx: &HandlerContext,
    account_key: &str,
) -> Result<String, std::io::Error> {
    let detail: OfficeConfigAccountDetail = office_config_service(ctx)
        .account_detail(account_key)
        .map_err(|e| to_io(e.to_string()))?;
    serde_json::to_string(&detail).map_err(|e| to_io(e.to_string()))
}

#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
/// POST /api/config/accounts/:account_key/config：保存单账户 provider 配置。
pub fn post_account_config(
    ctx: &HandlerContext,
    account_key: &str,
    body: &str,
) -> Result<ApiResponse, std::io::Error> {
    let loc = locale_from_store(ctx.config_store.as_ref());
    let request: OfficeAccountConfigSaveRequest = match serde_json::from_str(body) {
        Ok(value) => value,
        Err(error) => return Ok(ApiResponse::err_400(&error.to_string())),
    };
    match office_config_service(ctx).save_account_config(account_key, &request) {
        Ok(detail) => {
            ctx.reload_config();
            let body = serde_json::to_string(&detail).map_err(|error| to_io(error.to_string()))?;
            Ok(ApiResponse::ok_200_json(&body))
        }
        Err(error) => Ok(ApiResponse::err_400(&tr_error(&error, loc))),
    }
}

#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
/// POST /api/config/accounts/:account_key/probe：对单账户执行真实远端探测。
pub fn post_account_probe(
    ctx: &HandlerContext,
    account_key: &str,
) -> Result<ApiResponse, std::io::Error> {
    let loc = locale_from_store(ctx.config_store.as_ref());
    match office_config_service(ctx).probe(account_key) {
        Ok(result) => {
            let body = serde_json::to_string(&result).map_err(|error| to_io(error.to_string()))?;
            Ok(ApiResponse::ok_200_json(&body))
        }
        Err(error) => Ok(ApiResponse::err_400(&tr_error(&error, loc))),
    }
}

#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
/// POST /api/config/accounts/:account_key/revoke：清除单账户凭证并可选清理运行态。
pub fn post_account_revoke(
    ctx: &HandlerContext,
    account_key: &str,
    body: &str,
) -> Result<ApiResponse, std::io::Error> {
    let loc = locale_from_store(ctx.config_store.as_ref());
    let request: RevokeRequest = if body.trim().is_empty() {
        RevokeRequest {
            clear_runtime_status: true,
        }
    } else {
        match serde_json::from_str(body) {
            Ok(value) => value,
            Err(error) => return Ok(ApiResponse::err_400(&error.to_string())),
        }
    };
    match office_config_service(ctx).revoke(account_key, request.clear_runtime_status) {
        Ok(()) => Ok(ApiResponse::ok_200_json(
            &serde_json::json!({
                "ok": true,
                "account_key": account_key,
                "cleared_runtime_status": request.clear_runtime_status,
            })
            .to_string(),
        )),
        Err(error) => Ok(ApiResponse::err_400(&tr_error(&error, loc))),
    }
}

#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
/// DELETE /api/config/accounts/:account_key：删除单账户注册及其关联状态。
pub fn delete_account(
    ctx: &HandlerContext,
    account_key: &str,
) -> Result<ApiResponse, std::io::Error> {
    let loc = locale_from_store(ctx.config_store.as_ref());
    match office_config_service(ctx).delete_account(account_key) {
        Ok(()) => {
            ctx.reload_config();
            Ok(ApiResponse::ok_200_json(
                &serde_json::json!({
                    "ok": true,
                    "account_key": account_key,
                    "deleted": true,
                })
                .to_string(),
            ))
        }
        Err(error) => Ok(ApiResponse::err_400(&tr_error(&error, loc))),
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
