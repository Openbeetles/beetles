//! 配置 API：GET/POST /api/config/llm、/channels、/system，GET/POST /api/config/hardware。

use crate::config;
#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
use crate::error::Error;
#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
use crate::office::{
    parse_public_account_upsert_request_value, OfficeAccountConfigSaveRequest,
    OfficeAccountOnboardingDisposition, OfficeCapability, OfficeConfigAccountDetail,
    OfficeConfigAccountSummary, OfficeConfigCapabilityStatus, OfficeConfigManagementService,
    OfficeConfigProviderCatalogItem,
};
use crate::platform::http_server::api_contract;
use crate::platform::http_server::common::{to_io, ApiResponse};
#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
use serde::Serialize;
#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
use serde_json::Value;
#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
use std::sync::Arc;

use super::HandlerContext;

/// GET /api/config/llm：从缓存返回独立 LLM 段 JSON，避免配置页为 LLM 读取整包 AppConfig。
pub fn get_llm_body(ctx: &HandlerContext) -> Result<String, std::io::Error> {
    let config = ctx.config();
    let segment = config::LlmSegment::from_app_config(&config);
    serde_json::to_string(&segment).map_err(|e| to_io(e.to_string()))
}

#[derive(serde::Serialize)]
struct ChannelsConfigView {
    available_channels: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    unavailable_enabled_channel: Option<String>,
    #[serde(flatten)]
    segment: config::ChannelsSegment,
}

/// GET /api/config/channels：返回通道配置段 + 当前构建可见通道目录。
pub fn get_channels_body(ctx: &HandlerContext) -> Result<String, std::io::Error> {
    let config = ctx.config();
    let unavailable_enabled_channel = (!config.enabled_channel.trim().is_empty()
        && crate::normalize_compiled_enabled_channel(&config.enabled_channel).is_empty())
    .then(|| config.enabled_channel.clone());
    let segment = config::ChannelsSegment::from_app_config(&config);
    let payload = ChannelsConfigView {
        available_channels: crate::compiled_enabled_channel_ids()
            .iter()
            .copied()
            .map(str::to_string)
            .collect(),
        unavailable_enabled_channel,
        segment,
    };
    serde_json::to_string(&payload).map_err(|e| to_io(e.to_string()))
}

/// GET /api/config/system：返回系统配置段。
pub fn get_system_body(ctx: &HandlerContext) -> Result<String, std::io::Error> {
    let config = ctx.config();
    let segment = config::SystemSegment::from_app_config(&config);
    serde_json::to_string(&segment).map_err(|e| to_io(e.to_string()))
}

/// POST /api/config/llm：仅写 LLM 段，body 为 LlmSegment JSON。
pub fn post_llm(ctx: &HandlerContext, body: &str) -> Result<ApiResponse, std::io::Error> {
    let segment: config::LlmSegment = match serde_json::from_str(body) {
        Ok(segment) => segment,
        Err(_) => return Ok(ApiResponse::err_400_key(api_contract::COMMON_INVALID_JSON)),
    };
    match config::save_llm_segment_value(ctx.config_file_store.as_ref(), &segment) {
        Ok(()) => {
            ctx.update_cached_config(|config| {
                config::apply_llm_segment_to_config(config, &segment);
            });
            Ok(ApiResponse::ok_200_json("{\"ok\":true}"))
        }
        Err(e) => Ok(ApiResponse::err_400_key(api_contract::error_key(&e))),
    }
}

/// POST /api/config/channels：写通道段；通道配置只落 config/channels.json。
pub fn post_channels(ctx: &HandlerContext, body: &str) -> Result<ApiResponse, std::io::Error> {
    let segment: config::ChannelsSegment = match serde_json::from_str(body) {
        Ok(segment) => segment,
        Err(_) => return Ok(ApiResponse::err_400_key(api_contract::COMMON_INVALID_JSON)),
    };
    match config::save_channels_segment_value(ctx.config_file_store.as_ref(), &segment) {
        Ok(()) => {
            ctx.update_cached_config(|config| {
                config::apply_channels_segment_to_config(config, &segment);
            });
            Ok(ApiResponse::ok_200_json("{\"ok\":true}"))
        }
        Err(e) => Ok(ApiResponse::err_400_key(api_contract::error_key(&e))),
    }
}

/// POST /api/config/system：仅写系统段（wifi/proxy/locale），body 为 SystemSegment JSON。
pub fn post_system(ctx: &HandlerContext, body: &str) -> Result<ApiResponse, std::io::Error> {
    let segment: config::SystemSegment = match serde_json::from_str(body) {
        Ok(segment) => segment,
        Err(_) => return Ok(ApiResponse::err_400_key(api_contract::COMMON_INVALID_JSON)),
    };
    match config::save_system_segment_value_to_nvs(ctx.config_store.as_ref(), &segment) {
        Ok(()) => {
            ctx.update_cached_config(|config| {
                config::apply_system_segment_to_config(config, &segment);
            });
            Ok(ApiResponse::ok_200_json("{\"ok\":true}"))
        }
        Err(e) => Ok(ApiResponse::err_400_key(api_contract::error_key(&e))),
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
    let service = OfficeConfigManagementService::new(
        Arc::clone(&ctx.config_file_store),
        ctx.platform.office_credential_store(),
        ctx.platform.office_runtime_status_store(),
    );
    #[cfg(test)]
    {
        if let Some(probe_adapters) = ctx.office_probe_adapters.as_ref() {
            return service.with_probe_adapters(probe_adapters.clone());
        }
    }
    service.with_default_probe_adapters()
}

#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
fn strip_office_http_prose(value: &mut Value) {
    match value {
        Value::Object(map) => {
            map.remove("display_name");
            map.remove("label");
            map.remove("description");
            map.remove("error_message");
            for child in map.values_mut() {
                strip_office_http_prose(child);
            }
        }
        Value::Array(items) => {
            for item in items {
                strip_office_http_prose(item);
            }
        }
        _ => {}
    }
}

#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
fn serialize_office_http_payload<T: Serialize>(payload: &T) -> Result<String, std::io::Error> {
    let mut value = serde_json::to_value(payload).map_err(|error| to_io(error.to_string()))?;
    strip_office_http_prose(&mut value);
    serde_json::to_string(&value).map_err(|error| to_io(error.to_string()))
}

#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
fn serialize_office_http_payload_api<T: Serialize>(
    payload: &T,
    stage: &'static str,
) -> crate::error::Result<String> {
    let mut value = serde_json::to_value(payload).map_err(|error| Error::Other {
        source: Box::new(error),
        stage,
    })?;
    strip_office_http_prose(&mut value);
    serde_json::to_string(&value).map_err(|error| Error::Other {
        source: Box::new(error),
        stage,
    })
}

#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
fn parse_capability(capability: Option<&str>) -> crate::error::Result<Option<OfficeCapability>> {
    let Some(capability) = capability else {
        return Ok(None);
    };
    let value = match capability.trim() {
        "mail" => OfficeCapability::Mail,
        "calendar" => OfficeCapability::Calendar,
        "documents" => OfficeCapability::Documents,
        "contacts_directory" => OfficeCapability::ContactsDirectory,
        other => {
            return Err(Error::config(
                "office_capability",
                format!("unsupported office capability '{other}'"),
            ));
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
        .account_summaries(
            provider_kind,
            parse_capability(capability).map_err(|error| to_io(error.to_string()))?,
        )
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
) -> crate::error::Result<String> {
    let items = office_config_service(ctx)
        .provider_catalog(parse_capability(capability)?)
        .map_err(|error| error.with_stage("http_config_provider_catalog"))?;
    serialize_office_http_payload_api(
        &ProviderCatalogListResponse {
            count: items.len(),
            items,
        },
        "http_config_provider_catalog",
    )
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
) -> crate::error::Result<String> {
    let capability = parse_capability(Some(capability))?.ok_or_else(|| Error::Other {
        source: Box::new(std::io::Error::other("missing office capability")),
        stage: "http_config_capability_detail",
    })?;
    let status = office_config_service(ctx)
        .capability_statuses(Some(capability))
        .map_err(|error| error.with_stage("http_config_capability_detail"))?
        .into_iter()
        .next()
        .ok_or_else(|| Error::Other {
            source: Box::new(std::io::Error::other(format!(
                "missing capability status for '{capability:?}'"
            ))),
            stage: "http_config_capability_detail",
        })?;
    serialize_office_http_payload_api(&status, "http_config_capability_detail")
}

#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
/// POST /api/config/accounts：创建或更新单个账户注册。
pub fn post_accounts(ctx: &HandlerContext, body: &str) -> Result<ApiResponse, std::io::Error> {
    let request = match serde_json::from_str::<Value>(body) {
        Ok(Value::Object(obj)) => {
            match parse_public_account_upsert_request_value(&obj, "http_config_accounts_post") {
                Ok(request) => request,
                Err(error) => {
                    return Ok(ApiResponse::err_400_key(api_contract::error_key(&error)));
                }
            }
        }
        Err(_) => return Ok(ApiResponse::err_400_key(api_contract::COMMON_INVALID_JSON)),
        Ok(_) => return Ok(ApiResponse::err_400_key(api_contract::COMMON_INVALID_JSON)),
    };
    let cfg = ctx.config();
    let mut http = crate::network::create_http_client_with_config(
        ctx.platform.as_ref(),
        &cfg,
        crate::network::HttpClientClass::Background,
    )
    .map_err(|error| to_io(error.to_string()))?;
    drop(cfg);
    let office_http = crate::office::as_office_http_client(&mut http);
    match office_config_service(ctx).apply_account_with_http(office_http, &request) {
        Ok(result)
            if matches!(
                result.disposition,
                OfficeAccountOnboardingDisposition::Applied
            ) =>
        {
            let detail = result
                .account
                .expect("applied onboarding result must include detail");
            ctx.reload_config();
            let body = serialize_office_http_payload(&detail)?;
            Ok(ApiResponse::ok_200_json(&body))
        }
        Ok(result) => {
            let body = serialize_office_http_payload(&result)?;
            Ok(ApiResponse {
                status: 400,
                status_text: "Bad Request",
                body: body.into_bytes(),
            })
        }
        Err(error) => Ok(ApiResponse::err_400_key(api_contract::error_key(&error))),
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
    serialize_office_http_payload(&detail)
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
    let request: OfficeAccountConfigSaveRequest = match serde_json::from_str(body) {
        Ok(value) => value,
        Err(_) => return Ok(ApiResponse::err_400_key(api_contract::COMMON_INVALID_JSON)),
    };
    match office_config_service(ctx).save_account_config(account_key, &request) {
        Ok(detail) => {
            ctx.reload_config();
            let body = serialize_office_http_payload(&detail)?;
            Ok(ApiResponse::ok_200_json(&body))
        }
        Err(error) => Ok(ApiResponse::err_400_key(api_contract::error_key(&error))),
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
    let cfg = ctx.config();
    let mut http = crate::network::create_http_client_with_config(
        ctx.platform.as_ref(),
        &cfg,
        crate::network::HttpClientClass::Background,
    )
    .map_err(|error| to_io(error.to_string()))?;
    drop(cfg);
    let office_http = crate::office::as_office_http_client(&mut http);
    match office_config_service(ctx).probe_with_http(office_http, account_key) {
        Ok(result) => {
            let body = serde_json::to_string(&result).map_err(|error| to_io(error.to_string()))?;
            Ok(ApiResponse::ok_200_json(&body))
        }
        Err(error) => {
            let expose_upstream = !error.stage().starts_with("office_config_");
            let upstream_error = expose_upstream.then(|| error.to_string());
            Ok(ApiResponse::err_key_with_meta(
                400,
                "Bad Request",
                api_contract::error_key(&error),
                Some(error.stage()),
                upstream_error.as_deref(),
                error.http_status_code(),
                None,
                serde_json::Map::new(),
            ))
        }
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
    let request: RevokeRequest = if body.trim().is_empty() {
        RevokeRequest {
            clear_runtime_status: true,
        }
    } else {
        match serde_json::from_str(body) {
            Ok(value) => value,
            Err(_) => return Ok(ApiResponse::err_400_key(api_contract::COMMON_INVALID_JSON)),
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
        Err(error) => Ok(ApiResponse::err_400_key(api_contract::error_key(&error))),
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
        Err(error) => Ok(ApiResponse::err_400_key(api_contract::error_key(&error))),
    }
}

/// GET /api/config/hardware：从缓存返回 HardwareSegment JSON。路由层要求配对码。
pub fn get_hardware_body(ctx: &HandlerContext) -> Result<String, std::io::Error> {
    let config = ctx.config();
    let segment = config::HardwareSegment::from_app_config(&config);
    serde_json::to_string(&segment).map_err(|e| to_io(e.to_string()))
}

/// POST /api/config/hardware：校验并写入 HardwareSegment 到 SPIFFS config/hardware.json。
pub fn post_hardware(ctx: &HandlerContext, body: &str) -> Result<ApiResponse, std::io::Error> {
    let segment: config::HardwareSegment = match serde_json::from_str(body) {
        Ok(segment) => segment,
        Err(_) => return Ok(ApiResponse::err_400_key(api_contract::COMMON_INVALID_JSON)),
    };
    match config::save_hardware_segment_value(ctx.config_file_store.as_ref(), &segment) {
        Ok(()) => {
            ctx.update_cached_config(|config| {
                config::apply_hardware_segment_to_config(config, &segment);
            });
            Ok(ApiResponse::ok_200_json("{\"ok\":true}"))
        }
        Err(e) => Ok(ApiResponse::err_400_key(api_contract::error_key(&e))),
    }
}

/// GET /api/config/audio：从缓存返回 AudioSegment JSON（未配置时返回 disabled 默认配置）。路由层要求配对码。
pub fn get_audio_body(ctx: &HandlerContext) -> Result<String, std::io::Error> {
    let config = ctx.config();
    let segment = config::AudioSegment::from_app_config(&config);
    serde_json::to_string(&segment).map_err(|e| to_io(e.to_string()))
}

/// POST /api/config/audio：校验并写入 AudioSegment 到 SPIFFS config/audio.json。
pub fn post_audio(ctx: &HandlerContext, body: &str) -> Result<ApiResponse, std::io::Error> {
    let segment: config::AudioSegment = match serde_json::from_str(body) {
        Ok(segment) => segment,
        Err(_) => return Ok(ApiResponse::err_400_key(api_contract::COMMON_INVALID_JSON)),
    };
    match config::save_audio_segment_value(ctx.config_file_store.as_ref(), segment) {
        Ok(segment) => {
            ctx.update_cached_config(|config| {
                config::apply_audio_segment_to_config(config, segment);
            });
            Ok(ApiResponse::ok_200_json(
                r#"{"ok":true,"restart_required":true}"#,
            ))
        }
        Err(e) => Ok(ApiResponse::err_400_key(api_contract::error_key(&e))),
    }
}

/// GET /api/config/display：从缓存返回 DisplayConfig JSON（未配置时返回 disabled 默认配置）。路由层要求配对码。
pub fn get_display_body(ctx: &HandlerContext) -> Result<String, std::io::Error> {
    let config = ctx.config();
    let segment = config::display_segment_from_app_config(&config);
    serde_json::to_string(&segment).map_err(|e| to_io(e.to_string()))
}

/// POST /api/config/display：校验并写入 DisplayConfig 到 SPIFFS config/display.json。
pub fn post_display(ctx: &HandlerContext, body: &str) -> Result<ApiResponse, std::io::Error> {
    let segment: crate::display::DisplayConfig = match serde_json::from_str(body) {
        Ok(segment) => segment,
        Err(_) => return Ok(ApiResponse::err_400_key(api_contract::COMMON_INVALID_JSON)),
    };
    let config = ctx.config();
    let result = config::save_display_segment_value(
        ctx.config_file_store.as_ref(),
        &config.hardware_devices,
        segment,
    );
    drop(config);
    match result {
        Ok(segment) => {
            ctx.update_cached_config(|config| {
                config::apply_display_segment_to_config(config, segment);
            });
            Ok(ApiResponse::ok_200_json(
                r#"{"ok":true,"restart_required":true}"#,
            ))
        }
        Err(e) => Ok(ApiResponse::err_400_key(api_contract::error_key(&e))),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        get_audio_body, get_channels_body, get_display_body, get_hardware_body, get_system_body,
        post_audio, post_channels, post_display, post_hardware, post_llm, post_system,
    };
    use crate::config::{self, ConfigFileStore};
    use crate::error::Result;
    use serde_json::Value;
    use std::sync::Arc;

    struct PanicConfigFileStore;

    impl ConfigFileStore for PanicConfigFileStore {
        fn read_config_file(&self, _rel_path: &str) -> Result<Option<Vec<u8>>> {
            panic!("cached config GET should not read config files");
        }

        fn write_config_file(&self, _rel_path: &str, _data: &[u8]) -> Result<()> {
            panic!("cached config GET should not write config files");
        }

        fn remove_config_file(&self, _rel_path: &str) -> Result<()> {
            panic!("cached config GET should not remove config files");
        }
    }

    struct WriteOnlyConfigFileStore;

    impl ConfigFileStore for WriteOnlyConfigFileStore {
        fn read_config_file(&self, rel_path: &str) -> Result<Option<Vec<u8>>> {
            panic!("config POST should not reload config file {rel_path}");
        }

        fn write_config_file(&self, _rel_path: &str, _data: &[u8]) -> Result<()> {
            Ok(())
        }

        fn remove_config_file(&self, _rel_path: &str) -> Result<()> {
            Ok(())
        }
    }

    #[test]
    fn get_system_body_returns_only_system_segment() {
        let ctx = build_test_context();
        let store = ctx.config_store.as_ref();
        config::save_system_segment_to_nvs(
            store,
            r#"{
                "wifi_ssid":"BeetleNet",
                "wifi_pass":"secret-pass",
                "proxy_url":"http://proxy.local:8080",
                "locale":"en"
            }"#,
        )
        .unwrap();
        ctx.reload_config();

        let body = get_system_body(&ctx).unwrap();
        let parsed: Value = serde_json::from_str(&body).unwrap();

        assert_eq!(parsed["wifi_ssid"], "BeetleNet");
        assert_eq!(parsed["wifi_pass"], "secret-pass");
        assert_eq!(parsed["proxy_url"], "http://proxy.local:8080");
        assert_eq!(parsed["locale"], "en");
        assert!(parsed.get("tg_group_activation").is_none());
        assert!(parsed.get("tg_token").is_none());
        assert!(parsed.get("build_package").is_none());
        assert!(
            !body.contains('\n'),
            "system segment response should stay compact on ESP default path"
        );
    }

    #[test]
    fn post_system_rejects_invalid_locale() {
        let ctx = build_test_context();

        let response = post_system(
            &ctx,
            r#"{
                "wifi_ssid":"BeetleNet",
                "wifi_pass":"secret-pass",
                "proxy_url":"",
                "locale":"ja"
            }"#,
        )
        .expect("post_system response");

        assert_eq!(response.status, 400);
    }

    #[test]
    fn get_channels_body_returns_channel_activation_policy() {
        let ctx = build_test_context();
        ctx.update_cached_config(|config| {
            config.tg_group_activation = "always".to_string();
            config.tg_token = "token".to_string();
        });

        let body = get_channels_body(&ctx).unwrap();
        let parsed: Value = serde_json::from_str(&body).unwrap();

        assert_eq!(parsed["tg_group_activation"], "always");
        assert_eq!(parsed["tg_token"], "token");
        assert!(parsed.get("wifi_ssid").is_none());
    }

    #[test]
    fn cached_runtime_config_getters_do_not_read_config_files() {
        let mut ctx = build_test_context();
        ctx.config_file_store = Arc::new(PanicConfigFileStore);

        let hardware: Value =
            serde_json::from_str(&get_hardware_body(&ctx).expect("hardware body")).unwrap();
        let audio: Value =
            serde_json::from_str(&get_audio_body(&ctx).expect("audio body")).unwrap();
        let display: Value =
            serde_json::from_str(&get_display_body(&ctx).expect("display body")).unwrap();

        assert_eq!(hardware["hardware_devices"].as_array().unwrap().len(), 0);
        assert_eq!(audio["enabled"], false);
        assert_eq!(display["enabled"], false);
    }

    #[test]
    fn post_system_updates_cached_config_without_reloading_config_files() {
        let mut ctx = build_test_context();
        ctx.config_file_store = Arc::new(PanicConfigFileStore);

        let response = post_system(
            &ctx,
            r#"{
                "wifi_ssid":"BeetleNet",
                "wifi_pass":"secret-pass",
                "proxy_url":"http://proxy.local:8080",
                "locale":"en"
            }"#,
        )
        .expect("post_system response");

        assert_eq!(response.status, 200);
        let config = ctx.config();
        assert_eq!(config.wifi_ssid, "BeetleNet");
        assert_eq!(config.wifi_pass, "secret-pass");
        assert_eq!(config.proxy_url, "http://proxy.local:8080");
        assert_eq!(config.tg_group_activation, "mention");
        assert_eq!(config.locale.as_deref(), Some("en"));
    }

    #[test]
    fn post_llm_updates_cached_config_without_reloading_config_files() {
        let mut ctx = build_test_context();
        ctx.config_file_store = Arc::new(WriteOnlyConfigFileStore);

        let response = post_llm(
            &ctx,
            r#"{
                "llm_sources":[{
                    "provider":"openai",
                    "api_key":"test-key",
                    "model":"gpt-test",
                    "api_url":"https://api.openai.com/v1",
                    "max_tokens":1024
                }],
                "llm_router_source_index":0,
                "llm_worker_source_index":0
            }"#,
        )
        .expect("post_llm response");

        assert_eq!(response.status, 200);
        let config = ctx.config();
        assert_eq!(config.llm_sources.len(), 1);
        assert_eq!(config.llm_sources[0].api_key, "test-key");
        assert_eq!(config.api_key, "test-key");
        assert_eq!(config.model, "gpt-test");
        assert_eq!(config.model_provider, "openai");
        assert_eq!(config.api_url, "https://api.openai.com/v1");
        assert_eq!(config.llm_router_source_index, Some(0));
        assert_eq!(config.llm_worker_source_index, Some(0));
    }

    #[test]
    fn post_channels_updates_cached_config_without_reloading_config_files() {
        let mut ctx = build_test_context();
        ctx.config_file_store = Arc::new(WriteOnlyConfigFileStore);

        let response = post_channels(
            &ctx,
            r#"{
                "enabled_channel":"",
                "tg_group_activation":"always",
                "tg_token":"token",
                "tg_allowed_chat_ids":"1,2",
                "webhook_enabled":true,
                "webhook_token":"hook"
            }"#,
        )
        .expect("post_channels response");

        assert_eq!(response.status, 200);
        let config = ctx.config();
        assert_eq!(config.enabled_channel, "");
        assert_eq!(config.tg_group_activation, "always");
        assert_eq!(config.tg_token, "token");
        assert_eq!(config.tg_allowed_chat_ids, "1,2");
        assert!(config.webhook_enabled);
        assert_eq!(config.webhook_token, "hook");
    }

    #[test]
    fn post_channels_rejects_invalid_activation_policy() {
        let mut ctx = build_test_context();
        ctx.config_file_store = Arc::new(WriteOnlyConfigFileStore);

        let response = post_channels(
            &ctx,
            r#"{
                "enabled_channel":"",
                "tg_group_activation":"legacy",
                "tg_token":"token",
                "tg_allowed_chat_ids":"",
                "webhook_enabled":false,
                "webhook_token":""
            }"#,
        )
        .expect("post_channels response");

        assert_eq!(response.status, 400);
        assert_eq!(ctx.config().tg_group_activation, "mention");
    }

    #[test]
    fn post_hardware_updates_cached_config_without_reloading_config_files() {
        let mut ctx = build_test_context();
        ctx.config_file_store = Arc::new(WriteOnlyConfigFileStore);

        let response = post_hardware(
            &ctx,
            r#"{
                "hardware_devices":[],
                "i2c_bus":null,
                "i2c_devices":[],
                "i2c_sensors":[]
            }"#,
        )
        .expect("post_hardware response");

        assert_eq!(response.status, 200);
        let config = ctx.config();
        assert_eq!(config.hardware_devices.len(), 0);
        assert!(config.i2c_bus.is_none());
        assert_eq!(config.i2c_devices.len(), 0);
        assert_eq!(config.i2c_sensors.len(), 0);
    }

    #[test]
    fn post_hardware_drops_cached_display_when_new_hardware_conflicts() {
        let mut ctx = build_test_context();
        ctx.config_file_store = Arc::new(WriteOnlyConfigFileStore);
        ctx.update_cached_config(|config| {
            let mut display = crate::display::default_disabled_display_config();
            display.enabled = true;
            display.spi.dc = 40;
            config.display = Some(display);
        });

        let response = post_hardware(
            &ctx,
            r#"{
                "hardware_devices":[{
                    "id":"display_conflict",
                    "device_type":"gpio_out",
                    "pins":{"pin":40},
                    "what":"test conflict",
                    "how":"test"
                }],
                "i2c_bus":null,
                "i2c_devices":[],
                "i2c_sensors":[]
            }"#,
        )
        .expect("post_hardware response");

        assert_eq!(response.status, 200);
        assert!(ctx.config().display.is_none());
    }

    #[test]
    fn post_audio_updates_cached_config_without_reloading_config_files() {
        let mut ctx = build_test_context();
        ctx.config_file_store = Arc::new(WriteOnlyConfigFileStore);
        let body = serde_json::to_string(&config::default_disabled_audio_segment())
            .expect("serialize audio");

        let response = post_audio(&ctx, &body).expect("post_audio response");

        assert_eq!(response.status, 200);
        let config = ctx.config();
        assert!(config.audio.as_ref().is_some_and(|audio| !audio.enabled));
    }

    #[test]
    fn post_display_updates_cached_config_without_reloading_config_files() {
        let mut ctx = build_test_context();
        ctx.config_file_store = Arc::new(WriteOnlyConfigFileStore);
        let body = serde_json::to_string(&crate::display::default_disabled_display_config())
            .expect("serialize display");

        let response = post_display(&ctx, &body).expect("post_display response");

        assert_eq!(response.status, 200);
        let config = ctx.config();
        assert!(config
            .display
            .as_ref()
            .is_some_and(|display| !display.enabled));
    }

    fn build_test_context() -> crate::platform::http_server::handlers::HandlerContext {
        crate::platform::http_server::handlers::build_default_test_handler_context()
    }
}
