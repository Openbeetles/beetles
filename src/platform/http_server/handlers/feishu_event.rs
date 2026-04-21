//! POST /api/feishu/event：读 body 后调通道入口，写响应。

use crate::bus::InboundTx;
use crate::channels::{handle_http_event, FeishuEventResponse};
use crate::platform::http_server::common::ApiResponse;

use super::HandlerContext;

/// 读 body 由 mod 完成；此处仅调通道并写响应。
pub fn post(
    ctx: &HandlerContext,
    inbound_tx: &InboundTx,
    dedup_store: &crate::channels::FeishuMessageDedupStore,
    signature: &str,
    timestamp: &str,
    nonce: &str,
    body: &str,
) -> Result<ApiResponse, std::io::Error> {
    let config = ctx.config();
    let r = handle_http_event(
        &config,
        inbound_tx,
        dedup_store,
        crate::channels::FeishuRequestHeaders {
            signature,
            timestamp,
            nonce,
        },
        body,
    );
    let api = match r {
        FeishuEventResponse::Ok200Json(s) => ApiResponse::ok_200_json(&s),
        FeishuEventResponse::Err400(msg) => ApiResponse::err_400(msg),
        FeishuEventResponse::Err404(msg) => ApiResponse::err_404(msg),
    };
    Ok(api)
}
