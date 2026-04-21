//! POST /api/dingtalk/webhook：钉钉 Outgoing 机器人回调入站。

use crate::bus::InboundTx;
use crate::platform::http_server::common::ApiResponse;

/// 处理钉钉回调 body，调用通道 webhook::handle 入队。
pub fn post(
    inbound_tx: &InboundTx,
    session_store: &crate::channels::DingtalkSessionStore,
    body: &str,
) -> Result<ApiResponse, std::io::Error> {
    match crate::channels::dingtalk::webhook::handle(body, inbound_tx, session_store) {
        Ok(()) => Ok(ApiResponse::ok_200_json("{\"ok\":true}")),
        Err(e) => {
            log::warn!("[dingtalk_webhook_handler] {}", e);
            Ok(ApiResponse::err_400(&e.to_string()))
        }
    }
}
