//! GET /api/memory/status：仅生成响应体 JSON。

use super::HandlerContext;

pub fn body(ctx: &HandlerContext) -> String {
    let memory_len = ctx.memory_store.get_memory().map(|s| s.len()).unwrap_or(0);
    let soul_len = ctx.memory_store.get_soul().map(|s| s.len()).unwrap_or(0);
    let user_len = ctx.memory_store.get_user().map(|s| s.len()).unwrap_or(0);
    let long_term_count = ctx.platform.long_term_memory_store().count().unwrap_or(0);
    let continuity_capsule_count = ctx.platform.continuity_capsule_store().count().unwrap_or(0);
    format!(
        r#"{{"memory_len":{},"soul_len":{},"user_len":{},"long_term_count":{},"continuity_capsule_count":{}}}"#,
        memory_len, soul_len, user_len, long_term_count, continuity_capsule_count
    )
}
