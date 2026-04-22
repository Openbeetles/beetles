//! 配置 API：GET /api/config、GET/POST /api/config/llm、POST /api/config/wifi、/channels、/system、/hardware。

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
    OfficeConfigProviderCatalogItem, OfficeHttpClient, OfficeStreamingResponse,
};
use crate::platform::http_server::api_contract;
use crate::platform::http_server::common::{to_io, ApiResponse, WifiConfigPayload};
#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
use crate::platform::{PlatformHttpClient, ResponseBody};
#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
use serde::Serialize;
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

/// GET /api/config/llm：从缓存返回独立 LLM 段 JSON，避免配置页为 LLM 读取整包 AppConfig。
pub fn get_llm_body(ctx: &HandlerContext) -> Result<String, std::io::Error> {
    let config = ctx.config();
    let segment = config::LlmSegment::from_app_config(&config);
    serde_json::to_string(&segment).map_err(|e| to_io(e.to_string()))
}

/// POST /api/config/wifi：body 为 JSON，写 WiFi SSID/密码到 NVS。成功时返回 restart_required 提示需重启生效。
pub fn post_wifi(ctx: &HandlerContext, body: &str) -> Result<ApiResponse, std::io::Error> {
    let payload: WifiConfigPayload = match serde_json::from_str(body) {
        Ok(p) => p,
        Err(_) => return Ok(ApiResponse::err_400_key(api_contract::COMMON_INVALID_JSON)),
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
        Err(e) => Ok(ApiResponse::err_400_key(api_contract::error_key(&e))),
    }
}

/// POST /api/config/llm：仅写 LLM 段，body 为 LlmSegment JSON。
pub fn post_llm(ctx: &HandlerContext, body: &str) -> Result<ApiResponse, std::io::Error> {
    match config::save_llm_segment(ctx.config_file_store.as_ref(), body) {
        Ok(()) => {
            ctx.reload_config();
            Ok(ApiResponse::ok_200_json("{\"ok\":true}"))
        }
        Err(e) => Ok(ApiResponse::err_400_key(api_contract::error_key(&e))),
    }
}

/// POST /api/config/channels：仅写通道段，body 为 ChannelsSegment JSON。
pub fn post_channels(ctx: &HandlerContext, body: &str) -> Result<ApiResponse, std::io::Error> {
    match config::save_channels_segment(ctx.config_file_store.as_ref(), body) {
        Ok(()) => {
            ctx.reload_config();
            Ok(ApiResponse::ok_200_json("{\"ok\":true}"))
        }
        Err(e) => Ok(ApiResponse::err_400_key(api_contract::error_key(&e))),
    }
}

/// POST /api/config/system：仅写系统段（wifi/proxy/session/tg_group/locale），body 为 SystemSegment JSON。
pub fn post_system(ctx: &HandlerContext, body: &str) -> Result<ApiResponse, std::io::Error> {
    match config::save_system_segment_to_nvs(ctx.config_store.as_ref(), body) {
        Ok(()) => {
            ctx.reload_config();
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
struct HandlerOfficeHttpClient<'a> {
    http: &'a mut dyn PlatformHttpClient,
}

#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
impl OfficeHttpClient for HandlerOfficeHttpClient<'_> {
    fn request_with_headers(
        &mut self,
        method: &str,
        url: &str,
        headers: &[(&str, &str)],
        body: Option<&[u8]>,
    ) -> crate::error::Result<(u16, ResponseBody)> {
        self.http.request(method, url, headers, body)
    }

    fn get_with_headers(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
    ) -> crate::error::Result<(u16, ResponseBody)> {
        self.http.get(url, headers)
    }

    fn get_streaming_with_headers(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
        max_response_bytes: Option<usize>,
        on_chunk: &mut dyn FnMut(&[u8]) -> crate::error::Result<()>,
    ) -> crate::error::Result<OfficeStreamingResponse> {
        let mut streamed_bytes = 0usize;
        let limit = max_response_bytes.filter(|value| *value > 0);
        let status = self
            .http
            .get_streaming(url, headers, max_response_bytes, &mut |chunk| {
                streamed_bytes = streamed_bytes.saturating_add(chunk.len());
                on_chunk(chunk)
            })?;
        Ok(OfficeStreamingResponse {
            status,
            truncated: limit.map(|value| streamed_bytes >= value).unwrap_or(false),
        })
    }

    fn post_with_headers(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
        body: &[u8],
    ) -> crate::error::Result<(u16, ResponseBody)> {
        self.http.post(url, headers, body)
    }

    fn patch_with_headers(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
        body: &[u8],
    ) -> crate::error::Result<(u16, ResponseBody)> {
        self.http.patch(url, headers, body)
    }

    fn put_with_headers(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
        body: &[u8],
    ) -> crate::error::Result<(u16, ResponseBody)> {
        self.http.put(url, headers, body)
    }

    fn delete_with_headers(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
    ) -> crate::error::Result<(u16, ResponseBody)> {
        self.http.delete(url, headers)
    }
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
    let mut office_http = HandlerOfficeHttpClient {
        http: http.as_mut(),
    };
    match office_config_service(ctx).apply_account_with_http(&mut office_http, &request) {
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
    match office_config_service(ctx).probe(account_key) {
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
    match config::save_hardware_segment(ctx.config_file_store.as_ref(), body) {
        Ok(()) => {
            ctx.reload_config();
            Ok(ApiResponse::ok_200_json("{\"ok\":true}"))
        }
        Err(e) => Ok(ApiResponse::err_400_key(api_contract::error_key(&e))),
    }
}

/// GET /api/config/audio：返回 AudioSegment JSON（文件不存在时返回 disabled 默认配置）。路由层要求配对码。
pub fn get_audio_body(ctx: &HandlerContext) -> Result<String, std::io::Error> {
    config::get_audio_segment(ctx.config_file_store.as_ref()).map_err(|e| to_io(e.to_string()))
}

/// POST /api/config/audio：校验并写入 AudioSegment 到 SPIFFS config/audio.json。
pub fn post_audio(ctx: &HandlerContext, body: &str) -> Result<ApiResponse, std::io::Error> {
    match config::save_audio_segment(ctx.config_file_store.as_ref(), body) {
        Ok(()) => {
            ctx.reload_config();
            Ok(ApiResponse::ok_200_json(
                r#"{"ok":true,"restart_required":true}"#,
            ))
        }
        Err(e) => Ok(ApiResponse::err_400_key(api_contract::error_key(&e))),
    }
}

/// GET /api/config/display：返回 DisplayConfig JSON（文件不存在时返回 disabled 默认配置）。路由层要求配对码。
pub fn get_display_body(ctx: &HandlerContext) -> Result<String, std::io::Error> {
    config::get_display_segment(ctx.config_file_store.as_ref()).map_err(|e| to_io(e.to_string()))
}

/// POST /api/config/display：校验并写入 DisplayConfig 到 SPIFFS config/display.json。
pub fn post_display(ctx: &HandlerContext, body: &str) -> Result<ApiResponse, std::io::Error> {
    let hw_devices = ctx.config().hardware_devices.clone();
    match config::save_display_segment(ctx.config_file_store.as_ref(), &hw_devices, body) {
        Ok(()) => {
            ctx.reload_config();
            Ok(ApiResponse::ok_200_json(
                r#"{"ok":true,"restart_required":true}"#,
            ))
        }
        Err(e) => Ok(ApiResponse::err_400_key(api_contract::error_key(&e))),
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
