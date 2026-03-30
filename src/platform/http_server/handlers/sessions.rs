//! GET /api/sessions：仅生成响应体或错误信息，配对与写响应在 mod.rs。
//! GET /api/sessions?chat_id=xxx：返回指定会话最近消息。
//! DELETE /api/sessions?chat_id=xxx：删除指定会话。

use super::HandlerContext;
use crate::state;

/// 成功返回 JSON 数组字符串，失败返回错误信息（mod 层写 500）。
/// 支持分页：page（默认1）、limit（默认20，最大100）。
pub fn body(ctx: &HandlerContext, page: usize, limit: usize) -> Result<String, String> {
    let all_ids = ctx
        .session_store
        .list_chat_ids()
        .map_err(|e| state::sanitize_error_for_log(&e))?;

    let total = all_ids.len();
    let limit = limit.min(100).max(1);
    let page = page.max(1);
    let total_pages = (total + limit - 1) / limit;
    let skip = (page - 1) * limit;

    let items: Vec<&str> = all_ids
        .iter()
        .skip(skip)
        .take(limit)
        .map(|s| s.as_str())
        .collect();

    let response = serde_json::json!({
        "items": items,
        "total": total,
        "page": page,
        "limit": limit,
        "total_pages": total_pages
    });

    serde_json::to_string(&response).map_err(|e| e.to_string())
}

/// 返回指定 chat_id 的最近 N 条消息 JSON。
pub fn detail(ctx: &HandlerContext, chat_id: &str) -> Result<String, String> {
    let messages = ctx
        .session_store
        .load_recent(chat_id, 50)
        .map_err(|e| state::sanitize_error_for_log(&e))?;
    serde_json::to_string(&messages).map_err(|e| e.to_string())
}

/// 删除指定 chat_id 的会话。
pub fn delete(ctx: &HandlerContext, chat_id: &str) -> Result<String, String> {
    ctx.session_store
        .delete(chat_id)
        .map_err(|e| state::sanitize_error_for_log(&e))?;
    Ok(r#"{"ok":true}"#.to_string())
}
