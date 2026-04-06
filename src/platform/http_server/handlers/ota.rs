//! POST /api/ota：配对后根据 body.url 执行 OTA，成功则需重启（由 mod spawn）。
//! GET /api/ota/check：按板型与渠道查 manifest，返回是否有更新及 url。

use crate::error::Error;
use crate::i18n::{locale_from_store, tr, tr_error, Message};
use crate::platform::http_server::common::ApiResponse;

use super::HandlerContext;

const MAX_MANIFEST_LEN: usize = 8192;

/// 简单 semver 比较：仅当 a > b 时返回 true；解析失败返回 false。
fn semver_gt(a: &str, b: &str) -> bool {
    let parse = |s: &str| -> Option<(u32, u32, u32)> {
        let mut it = s.splitn(3, '.');
        let maj = it.next()?.trim_start_matches('v').parse().ok()?;
        let min = it.next().unwrap_or("0").parse().ok().unwrap_or(0);
        let patch = it.next().unwrap_or("0").parse().ok().unwrap_or(0);
        Some((maj, min, patch))
    };
    let Some((a_maj, a_min, a_patch)) = parse(a) else {
        return false;
    };
    let Some((b_maj, b_min, b_patch)) = parse(b) else {
        return false;
    };
    if a_maj != b_maj {
        return a_maj > b_maj;
    }
    if a_min != b_min {
        return a_min > b_min;
    }
    a_patch > b_patch
}

/// GET /api/ota/check：拉取 manifest，按 board_id + channel 查表，与当前版本比较；返回 JSON body。
#[cfg(feature = "ota")]
pub fn get_check(ctx: &HandlerContext, channel: &str) -> Result<String, std::io::Error> {
    use crate::platform::http_server::common::to_io;

    let loc = locale_from_store(ctx.config_store.as_ref());
    let current = ctx.version.as_ref();

    if crate::ota_manifest_url().is_empty() {
        let err_msg = tr(Message::OtaChannelNotConfigured, loc);
        let json = serde_json::json!({
            "current_version": current,
            "update_available": false,
            "error": err_msg,
        });
        return serde_json::to_string(&json).map_err(to_io);
    }

    let body = match ctx.fetch_url(crate::ota_manifest_url(), MAX_MANIFEST_LEN) {
        Ok(b) => b,
        Err(_) => {
            let err_msg = tr(Message::OtaCheckFail, loc);
            let json = serde_json::json!({
                "current_version": current,
                "update_available": false,
                "error": err_msg,
            });
            return serde_json::to_string(&json).map_err(to_io);
        }
    };

    let body_str = match std::str::from_utf8(&body) {
        Ok(s) => s,
        Err(_) => {
            let err_msg = tr(Message::OtaCheckFail, loc);
            let json = serde_json::json!({
                "current_version": current,
                "update_available": false,
                "error": err_msg,
            });
            return serde_json::to_string(&json).map_err(to_io);
        }
    };

    let root: serde_json::Value = match serde_json::from_str(body_str) {
        Ok(v) => v,
        Err(_) => {
            let err_msg = tr(Message::OtaCheckFail, loc);
            let json = serde_json::json!({
                "current_version": current,
                "update_available": false,
                "error": err_msg,
            });
            return serde_json::to_string(&json).map_err(to_io);
        }
    };

    let boards = match root
        .get("boards")
        .and_then(|b| b.get(ctx.board_id.as_ref()))
    {
        Some(b) => b,
        None => {
            let json = serde_json::json!({
                "current_version": current,
                "update_available": false,
            });
            return serde_json::to_string(&json).map_err(to_io);
        }
    };

    let channel_entry = match boards.get(channel) {
        Some(c) => c,
        None => {
            let json = serde_json::json!({
                "current_version": current,
                "update_available": false,
            });
            return serde_json::to_string(&json).map_err(to_io);
        }
    };

    let latest_version = match channel_entry.get("version").and_then(|v| v.as_str()) {
        Some(v) => v,
        None => {
            let json = serde_json::json!({
                "current_version": current,
                "update_available": false,
            });
            return serde_json::to_string(&json).map_err(to_io);
        }
    };

    let url = channel_entry.get("url").and_then(|u| u.as_str());
    let update_available = semver_gt(latest_version, current) && url.is_some();

    if update_available {
        let mut json = serde_json::json!({
            "current_version": current,
            "latest_version": latest_version,
            "update_available": true,
            "url": url.unwrap_or(""),
        });
        if let Some(notes) = channel_entry.get("release_notes").and_then(|n| n.as_str()) {
            json["release_notes"] = serde_json::Value::String(notes.to_string());
        }
        serde_json::to_string(&json).map_err(to_io)
    } else {
        let json = serde_json::json!({
            "current_version": current,
            "latest_version": latest_version,
            "update_available": false,
        });
        serde_json::to_string(&json).map_err(to_io)
    }
}

/// body 为请求体 UTF-8 字符串。返回 (ApiResponse, should_spawn_restart)。
#[cfg(feature = "ota")]
pub fn post(ctx: &HandlerContext, body: &str) -> Result<(ApiResponse, bool), std::io::Error> {
    let loc = locale_from_store(ctx.config_store.as_ref());
    let url = match serde_json::from_str::<serde_json::Value>(body) {
        Ok(v) => v
            .get("url")
            .and_then(|u| u.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        Err(_) => None,
    };
    let url = match url {
        Some(u) => u,
        None => return Ok((ApiResponse::err_400(&tr(Message::InvalidUrl, loc)), false)),
    };
    let valid = (url.starts_with("http://") || url.starts_with("https://")) && url.len() > 8;
    if !valid {
        return Ok((ApiResponse::err_400(&tr(Message::InvalidUrl, loc)), false));
    }
    match ctx.platform.ota_from_url(&url) {
        Ok(()) => Ok((ApiResponse::ok_200_json("{\"ok\":true}"), true)),
        Err(e) => {
            let msg = match &e {
                Error::Esp { stage, .. } => {
                    let m = match *stage {
                        "ota_download" => Message::OtaDownload,
                        "ota_validate" => Message::OtaValidate,
                        "ota_write" => Message::OtaWrite,
                        _ => Message::OperationFailed,
                    };
                    tr(m, loc)
                }
                _ => tr_error(&e, loc),
            };
            Ok((ApiResponse::err_500(&msg), false))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::semver_gt;

    #[test]
    fn semver_gt_works_for_basic_cases() {
        assert!(semver_gt("1.2.4", "1.2.3"));
        assert!(semver_gt("v2.0.0", "1.9.9"));
        assert!(!semver_gt("1.2.3", "1.2.3"));
        assert!(!semver_gt("1.2.2", "1.2.3"));
    }

    #[test]
    fn semver_gt_handles_invalid_input() {
        assert!(!semver_gt("bad", "1.2.3"));
        assert!(!semver_gt("1.2.3", "bad"));
        assert!(!semver_gt("v", "v"));
    }
}
