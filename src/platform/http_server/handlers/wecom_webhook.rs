//! GET/POST /api/wecom/webhook：企业微信回调。
//! GET: URL 验证（验签 + 返回 echostr）。
//! POST: 消息回调（验签 + 解析 XML 入队）。

use crate::bus::InboundTx;
use crate::platform::http_server::common::ApiResponse;

use super::HandlerContext;

/// GET URL 验证：验签后从 query 取 echostr 并原样返回。
pub fn get_verify(uri: &str, wecom_token: &str) -> ApiResponse {
    let msg_signature = extract_query_param(uri, "msg_signature").unwrap_or_default();
    let timestamp = extract_query_param(uri, "timestamp").unwrap_or_default();
    let nonce = extract_query_param(uri, "nonce").unwrap_or_default();
    let echostr = extract_query_param(uri, "echostr").unwrap_or_default();

    if echostr.is_empty() {
        return ApiResponse::err_400("missing echostr");
    }

    if !crate::channels::wecom::webhook::verify_signature(
        wecom_token,
        &timestamp,
        &nonce,
        &msg_signature,
    ) {
        return ApiResponse::err_400("invalid signature");
    }

    ApiResponse {
        status: 200,
        status_text: "OK",
        body: echostr.into_bytes(),
    }
}

/// POST 消息回调。
pub fn post(
    ctx: &HandlerContext,
    uri: &str,
    inbound_tx: &InboundTx,
    body: &str,
) -> Result<ApiResponse, std::io::Error> {
    let config = ctx.config();
    let wecom_token = &config.wecom_token;
    let msg_signature = extract_query_param(uri, "msg_signature").unwrap_or_default();
    let timestamp = extract_query_param(uri, "timestamp").unwrap_or_default();
    let nonce = extract_query_param(uri, "nonce").unwrap_or_default();

    match crate::channels::wecom::webhook::handle_message(
        body,
        wecom_token,
        &timestamp,
        &nonce,
        &msg_signature,
        inbound_tx,
    ) {
        Ok(()) => Ok(ApiResponse::ok_200_json("{\"ok\":true}")),
        Err(e) => {
            log::warn!("[wecom_webhook_handler] {}", e);
            Ok(ApiResponse::err_400(&e.to_string()))
        }
    }
}

fn extract_query_param(uri: &str, key: &str) -> Option<String> {
    let query = uri.find('?').map(|i| &uri[i + 1..]).unwrap_or("");
    for pair in query.split('&') {
        let mut it = pair.splitn(2, '=');
        if it.next().is_some_and(|k| k.eq_ignore_ascii_case(key)) {
            return it
                .next()
                .filter(|s| !s.is_empty())
                .map(crate::util::percent_decode_query);
        }
    }
    None
}
