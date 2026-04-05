//! POST /api/feishu/event：读 body 后调通道入口，写响应。

use crate::bus::InboundTx;
use crate::channels::{FeishuEventResponse, handle_http_event};
use crate::platform::http_server::common::ApiResponse;

use super::HandlerContext;

/// 读 body 由 mod 完成；此处仅调通道并写响应。
pub fn post(
    ctx: &HandlerContext,
    inbound_tx: &InboundTx,
    body: &str,
) -> Result<ApiResponse, std::io::Error> {
    let config = ctx.config();
    let r = handle_http_event(&config, inbound_tx, body);
    let api = match r {
        FeishuEventResponse::Ok200Json(s) => ApiResponse::ok_200_json(&s),
        FeishuEventResponse::Err400(msg) => ApiResponse::err_400(msg),
        FeishuEventResponse::Err404(msg) => ApiResponse::err_404(msg),
    };
    Ok(api)
}
