//! GET/POST/DELETE /api/skills、POST /api/skills/import：技能列表、单技能内容、排序、启用/禁用、写入、删除、URL 导入。

use crate::i18n::{locale_from_store, tr, tr_error, Message};
use crate::platform::http_server::common::ApiResponse;
use crate::skills;
use crate::state;

use super::HandlerContext;

/// GET 成功时：单技能返回 text/plain，列表返回 JSON 字符串。
#[derive(Clone)]
pub enum SkillsGetResult {
    TextPlain(String),
    Json(String),
}

/// GET /api/skills：name 为 query 中的 name。有 name 返回单技能内容或 404；无 name 返回 { skills, order } JSON。
pub fn get(ctx: &HandlerContext, name: Option<String>) -> Result<SkillsGetResult, ApiResponse> {
    let loc = locale_from_store(ctx.config_store.as_ref());
    if let Some(n) = name {
        let n = n.strip_suffix(".md").unwrap_or(&n);
        return match skills::get_skill_content(ctx.skill_storage.as_ref(), n) {
            Some(content) => Ok(SkillsGetResult::TextPlain(content)),
            None => Err(ApiResponse::err_404(&tr(Message::SkillNotFound, loc))),
        };
    }
    let disabled = skills::get_disabled_skills(ctx.skill_meta_store.as_ref());
    let list: Vec<serde_json::Value> = skills::list_skill_names(ctx.skill_storage.as_ref())
        .into_iter()
        .map(|name| {
            let enabled = !disabled.contains(&name);
            serde_json::json!({ "name": name, "enabled": enabled })
        })
        .collect();
    let order = skills::get_ordered_enabled_skill_names(
        ctx.skill_meta_store.as_ref(),
        ctx.skill_storage.as_ref(),
    );
    let payload = serde_json::json!({ "skills": list, "order": order });
    let body = serde_json::to_string(&payload)
        .map_err(|_| ApiResponse::err_500(&tr(Message::OperationFailed, loc)))?;
    Ok(SkillsGetResult::Json(body))
}

/// POST /api/skills：body 为 JSON。支持 order / name+content / name+enabled。
pub fn post(ctx: &HandlerContext, body: &str) -> ApiResponse {
    let loc = locale_from_store(ctx.config_store.as_ref());
    let v = match serde_json::from_str::<serde_json::Value>(body) {
        Ok(x) => x,
        Err(_) => return ApiResponse::err_400(&tr(Message::InvalidJson, loc)),
    };
    let name = v.get("name").and_then(|n| n.as_str()).map(String::from);
    if let Some(order_arr) = v.get("order").and_then(|o| o.as_array()) {
        let order: Vec<String> = order_arr
            .iter()
            .filter_map(|x| x.as_str().map(String::from))
            .collect();
        return match skills::set_skills_order(ctx.skill_meta_store.as_ref(), &order) {
            Ok(()) => ApiResponse::ok_200_json("{\"ok\":true}"),
            Err(e) => ApiResponse::err_400(&tr_error(&e, loc)),
        };
    }
    if let Some(content) = v.get("content").and_then(|c| c.as_str()) {
        if let Some(ref name) = name {
            let name = name.strip_suffix(".md").unwrap_or(name);
            return match skills::write_skill(ctx.skill_storage.as_ref(), name, content) {
                Ok(()) => ApiResponse::ok_200_json("{\"ok\":true}"),
                Err(e) => ApiResponse::err_400(&tr_error(&e, loc)),
            };
        }
        return ApiResponse::err_400(&tr(Message::MissingNameForWrite, loc));
    }
    if let Some(enabled) = v.get("enabled").and_then(|e| e.as_bool()) {
        if let Some(ref name) = name {
            return match skills::set_skill_enabled(ctx.skill_meta_store.as_ref(), name, enabled) {
                Ok(()) => ApiResponse::ok_200_json("{\"ok\":true}"),
                Err(e) => ApiResponse::err_400(&tr_error(&e, loc)),
            };
        }
        return ApiResponse::err_400(&tr(Message::MissingNameOrEnabled, loc));
    }
    ApiResponse::err_400(&tr(Message::MissingOrderNameContent, loc))
}

/// DELETE /api/skills?name=：name 由 mod 从 query 解析后传入（必填）。
pub fn delete(ctx: &HandlerContext, name: &str) -> ApiResponse {
    let loc = locale_from_store(ctx.config_store.as_ref());
    let name = name.strip_suffix(".md").unwrap_or(name);
    match skills::delete_skill(ctx.skill_storage.as_ref(), name) {
        Ok(()) => ApiResponse::ok_200_json("{\"ok\":true}"),
        Err(e) => {
            let msg = state::sanitize_error_for_log(&e);
            if msg.contains("not found") || msg.contains("No such file") {
                ApiResponse::err_404(&tr(Message::SkillNotFound, loc))
            } else {
                ApiResponse::err_400(&tr_error(&e, loc))
            }
        }
    }
}

const IMPORT_MAX: usize = 32 * 1024;

/// POST /api/skills/import：body 为 JSON { url, name }，拉取 URL 内容后写入技能。
pub fn import(ctx: &HandlerContext, body: &str) -> Result<ApiResponse, std::io::Error> {
    let loc = locale_from_store(ctx.config_store.as_ref());
    let v = match serde_json::from_str::<serde_json::Value>(body) {
        Ok(x) => x,
        Err(_) => return Ok(ApiResponse::err_400(&tr(Message::InvalidJson, loc))),
    };
    let url = v
        .get("url")
        .and_then(|u| u.as_str())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty());
    let name = v
        .get("name")
        .and_then(|n| n.as_str())
        .map(|s| s.to_string());
    let url = match url {
        Some(u) => u,
        None => return Ok(ApiResponse::err_400(&tr(Message::MissingUrl, loc))),
    };
    let name = match name {
        Some(n) => n.strip_suffix(".md").unwrap_or(&n).to_string(),
        None => return Ok(ApiResponse::err_400(&tr(Message::MissingName, loc))),
    };
    if !(url.starts_with("http://") || url.starts_with("https://")) || url.len() <= 8 {
        return Ok(ApiResponse::err_400(&tr(Message::InvalidUrl, loc)));
    }
    let body_bytes = match ctx.fetch_url(url, IMPORT_MAX) {
        Ok(b) => b,
        Err(e) => {
            let status = ApiResponse::err_500(&tr_error(&e, loc));
            return Ok(status);
        }
    };
    let content = match String::from_utf8(body_bytes) {
        Ok(s) => s,
        Err(_) => return Ok(ApiResponse::err_400(&tr(Message::UrlBodyNotUtf8, loc))),
    };
    match skills::write_skill(ctx.skill_storage.as_ref(), &name, &content) {
        Ok(()) => Ok(ApiResponse::ok_200_json("{\"ok\":true}")),
        Err(e) => Ok(ApiResponse::err_400(&tr_error(&e, loc))),
    }
}
