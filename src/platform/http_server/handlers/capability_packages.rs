//! GET/POST /api/capability_packages：能力包状态与 install/enable/disable/uninstall/rollback 管理。

use super::HandlerContext;
use crate::capability_package::{
    build_capability_package_operator_snapshot, install_capability_package,
    rollback_capability_package, set_capability_package_enabled, uninstall_capability_package,
    CapabilityPackageInstallPayload, CapabilityPackageOperationKind,
};
use crate::i18n::{locale_from_store, tr, tr_error, Message};
use crate::platform::http_server::common::ApiResponse;
use serde::Deserialize;

#[derive(Deserialize)]
struct CapabilityPackagePostRequest {
    op: CapabilityPackageOperationKind,
    #[serde(default)]
    package_id: String,
    #[serde(default)]
    payload: Option<CapabilityPackageInstallPayload>,
}

pub fn get(ctx: &HandlerContext) -> Result<String, std::io::Error> {
    let config = ctx.config();
    let current_channel = config.enabled_channel.clone();
    drop(config);
    let snapshot = build_capability_package_operator_snapshot(
        ctx.platform.state_fs().as_ref(),
        ctx.capability_package_runtime_capabilities.as_ref(),
        current_channel.as_str(),
    )
    .map_err(std::io::Error::other)?;
    serde_json::to_string(&snapshot).map_err(std::io::Error::other)
}

pub fn post(ctx: &HandlerContext, body: &str) -> ApiResponse {
    let loc = locale_from_store(ctx.config_store.as_ref());
    let request = match serde_json::from_str::<CapabilityPackagePostRequest>(body) {
        Ok(value) => value,
        Err(_) => return ApiResponse::err_400(&tr(Message::InvalidJson, loc)),
    };
    let state_fs = ctx.platform.state_fs();
    let now_secs = crate::util::current_unix_secs();
    let outcome = match request.op {
        CapabilityPackageOperationKind::Install => {
            let Some(payload) = request.payload.as_ref() else {
                return ApiResponse::err_400("missing payload for install");
            };
            install_capability_package(
                state_fs.as_ref(),
                ctx.capability_package_runtime_capabilities.as_ref(),
                payload,
                now_secs,
            )
        }
        CapabilityPackageOperationKind::Enable => set_capability_package_enabled(
            state_fs.as_ref(),
            ctx.capability_package_runtime_capabilities.as_ref(),
            request.package_id.trim(),
            true,
            now_secs,
        ),
        CapabilityPackageOperationKind::Disable => set_capability_package_enabled(
            state_fs.as_ref(),
            ctx.capability_package_runtime_capabilities.as_ref(),
            request.package_id.trim(),
            false,
            now_secs,
        ),
        CapabilityPackageOperationKind::Uninstall => {
            uninstall_capability_package(state_fs.as_ref(), request.package_id.trim(), now_secs)
        }
        CapabilityPackageOperationKind::Rollback => rollback_capability_package(
            state_fs.as_ref(),
            ctx.capability_package_runtime_capabilities.as_ref(),
            request.package_id.trim(),
            now_secs,
        ),
    };
    match outcome {
        Ok(outcome) => match serde_json::to_string(&serde_json::json!({
            "ok": true,
            "outcome": outcome,
        })) {
            Ok(body) => ApiResponse::ok_200_json(&body),
            Err(error) => ApiResponse::err_500(&error.to_string()),
        },
        Err(error) => ApiResponse::err_400(&tr_error(&error, loc)),
    }
}
