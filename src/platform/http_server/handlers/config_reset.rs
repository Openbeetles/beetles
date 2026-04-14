//! POST /api/config_reset：需带配对码；成功后恢复默认配置并清除配对码（回到未激活）。

use crate::config;
use crate::platform::http_server::common::ApiResponse;
use crate::platform::pairing;

use super::HandlerContext;

pub fn post(ctx: &HandlerContext) -> Result<ApiResponse, std::io::Error> {
    config::reset_to_defaults(ctx.config_store.as_ref())
        .map_err(|_| std::io::Error::other("reset_to_defaults failed"))?;
    let _ = pairing::clear_code(ctx.config_store.as_ref());
    let _ = ctx.platform.remove_config_file("config/skills_meta.json");
    let _ = ctx.platform.remove_config_file("config/llm.json");
    let _ = ctx.platform.remove_config_file("config/channels.json");
    let _ = ctx.platform.remove_config_file("config/accounts.json");
    let _ = ctx
        .platform
        .remove_config_file("config/office_credentials.json");
    let _ = ctx.platform.remove_config_file("config/hardware.json");
    let _ = ctx.platform.remove_config_file("config/audio.json");
    let _ = ctx.platform.remove_config_file("config/display.json");
    let _ = ctx
        .platform
        .remove_config_file("runtime/office_runtime_status.json");
    Ok(ApiResponse::ok_200_json("{\"ok\":true}"))
}
