//! GET /：API 信息 JSON，已激活后返回。

use super::HandlerContext;

pub fn body(ctx: &HandlerContext) -> Result<String, std::io::Error> {
    let mut endpoints: Vec<&'static str> = vec![
        "GET /pairing",
        "GET /wifi",
        "GET /api/pairing_code",
        "POST /api/pairing_code",
        "GET /api/config",
        "POST /api/config/llm",
        "POST /api/config/channels",
        "POST /api/config/system",
        "GET /api/config/hardware",
        "POST /api/config/hardware",
        "GET /api/config/audio",
        "POST /api/config/audio",
        "GET /api/wifi/scan",
        "GET /api/health",
        "GET /api/diagnose",
        "GET /api/system_info",
        "GET /api/channel_connectivity",
        "GET /api/tools",
        "GET /api/soul",
        "GET /api/user",
        "POST /api/soul",
        "POST /api/user",
        "GET /api/sessions",
        "GET /api/memory/status",
        "GET /api/skills",
        "POST /api/skills",
        "DELETE /api/skills",
        "POST /api/skills/import",
        "POST /api/restart",
        "POST /api/config_reset",
        "POST /api/webhook",
    ];
    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
    {
        endpoints.push("POST /api/feishu/event");
        endpoints.push("POST /api/dingtalk/webhook");
        endpoints.push("GET /api/wecom/webhook");
        endpoints.push("POST /api/wecom/webhook");
        endpoints.push("POST /api/webhook/qq");
    }
    if cfg!(feature = "ota") {
        endpoints.push("GET /api/ota/check");
        endpoints.push("POST /api/ota");
    }
    let endpoints_json =
        serde_json::to_string(&endpoints).unwrap_or_else(|_| r#"["GET /api/config"]"#.into());
    let s = format!(
        r#"{{"name":"beetle","version":"{}","endpoints":{}}}"#,
        ctx.version.as_ref(),
        endpoints_json
    );
    Ok(s)
}
