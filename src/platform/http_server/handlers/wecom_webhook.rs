//! GET/POST /api/wecom/webhook：企业微信回调。
//! GET: URL 验证（明文返回 echostr；安全模式验签并解密后返回）。
//! POST: 消息回调（明文解析原始 XML；安全模式验签、解密 XML 后入队）。

use crate::bus::InboundTx;
use crate::platform::http_server::common::ApiResponse;

use super::HandlerContext;

/// GET URL 验证：根据是否配置 EncodingAESKey 决定明文或安全模式。
pub fn get_verify(
    uri: &str,
    wecom_token: &str,
    wecom_encoding_aes_key: &str,
    corp_id: &str,
) -> ApiResponse {
    let callback = crate::channels::wecom::webhook::CallbackConfig {
        token: wecom_token,
        encoding_aes_key: wecom_encoding_aes_key,
        corp_id,
    };
    let msg_signature = extract_query_param(uri, "msg_signature").unwrap_or_default();
    let signature = if msg_signature.is_empty() {
        extract_query_param(uri, "signature").unwrap_or_default()
    } else {
        msg_signature
    };
    let timestamp = extract_query_param(uri, "timestamp").unwrap_or_default();
    let nonce = extract_query_param(uri, "nonce").unwrap_or_default();
    let echostr = extract_query_param(uri, "echostr").unwrap_or_default();

    if echostr.is_empty() {
        let error = crate::error::Error::config("wecom_webhook", "missing echostr");
        return ApiResponse::err_400(&error.to_string());
    }

    match crate::channels::wecom::webhook::verify_get_echostr(
        &callback, &timestamp, &nonce, &signature, &echostr,
    ) {
        Ok(body) => ApiResponse {
            status: 200,
            status_text: "OK",
            body: body.into_bytes(),
        },
        Err(e) => {
            log::warn!("[wecom_webhook_handler] {}", e);
            ApiResponse::err_400(&e.to_string())
        }
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
    let callback = crate::channels::wecom::webhook::CallbackConfig {
        token: &config.wecom_token,
        encoding_aes_key: &config.wecom_encoding_aes_key,
        corp_id: &config.wecom_corp_id,
    };
    let msg_signature = extract_query_param(uri, "msg_signature").unwrap_or_default();
    let signature = if msg_signature.is_empty() {
        extract_query_param(uri, "signature").unwrap_or_default()
    } else {
        msg_signature
    };
    let timestamp = extract_query_param(uri, "timestamp").unwrap_or_default();
    let nonce = extract_query_param(uri, "nonce").unwrap_or_default();

    let decoded_body = match crate::channels::wecom::webhook::decode_post_xml(
        &callback, &timestamp, &nonce, &signature, body,
    ) {
        Ok(decoded) => decoded,
        Err(e) => {
            log::warn!("[wecom_webhook_handler] {}", e);
            return Ok(ApiResponse::err_400(&e.to_string()));
        }
    };

    match crate::channels::wecom::webhook::handle_message(&decoded_body, inbound_tx) {
        Ok(()) => Ok(ApiResponse {
            status: 200,
            status_text: "OK",
            body: Vec::new(),
        }),
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
