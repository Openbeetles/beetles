//! GET /api/diagnose：仅生成响应体 JSON。

use super::HandlerContext;
use crate::platform::http_server::common::to_io;
use crate::state;

pub fn body(ctx: &HandlerContext) -> Result<String, std::io::Error> {
    let memory_loaded = ctx.memory_store.get_memory().is_ok();
    let storage_ok = memory_loaded;
    let spiffs_ok = ctx.platform.spiffs_usage();
    let nvs_ok = ctx.platform.config_store().read_string("wifi_ssid").is_ok();
    let last_error = state::get_last_error();
    let inc_val = ctx.inbound_depth.load(std::sync::atomic::Ordering::Relaxed);
    let out_val = ctx
        .outbound_depth
        .load(std::sync::atomic::Ordering::Relaxed);
    let skills_count = crate::skills::list_skill_names(ctx.skill_storage.as_ref()).len();
    let last_errors_count = state::get_last_errors_count();
    let results = crate::doctor::diagnose(
        crate::state::wifi_sta_connected(),
        inc_val,
        out_val,
        last_error,
        storage_ok,
        spiffs_ok,
        nvs_ok,
        memory_loaded,
        skills_count,
        last_errors_count,
    );
    serde_json::to_string(&results).map_err(to_io)
}
