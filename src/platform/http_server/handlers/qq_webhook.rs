//! POST /api/webhook/qq：QQ 频道机器人回调。URL 校验、事件验签与入队；body 读取与长度校验由 mod 完成。

use crate::channels::QQ_WEBHOOK_BODY_MAX;
use crate::i18n::{locale_from_store, tr, Message};
use crate::platform::http_server::common::ApiResponse;

/// QQ webhook 成功时的响应：URL 校验返回 token+signature；事件已处理返回 200 空 body。
pub enum QqWebhookOutcome {
    UrlVerification {
        plain_token: String,
        signature: String,
    },
    EventHandled,
}

/// 处理已读取的 body 与签名头，验签并调用通道 handle_webhook；超长 body 由调用方拒绝后不再调用。
#[allow(clippy::too_many_arguments)]
pub fn post(
    store: &dyn crate::platform::ConfigStore,
    body: &[u8],
    ts_header: Option<&str>,
    sig_header: Option<&str>,
    app_id: &str,
    secret: &str,
    inbound_tx: &crate::bus::InboundTx,
    cache: crate::channels::QqMsgIdCache,
) -> Result<QqWebhookOutcome, ApiResponse> {
    let loc = locale_from_store(store);
    if body.len() > QQ_WEBHOOK_BODY_MAX {
        let msg = tr(Message::BodyTooLarge, loc);
        return Err(ApiResponse::err_413(&msg));
    }

    match crate::channels::handle_webhook(
        body, ts_header, sig_header, app_id, secret, inbound_tx, cache,
    ) {
        Ok(crate::channels::QqHandlerResult::UrlVerification {
            plain_token,
            signature,
        }) => Ok(QqWebhookOutcome::UrlVerification {
            plain_token,
            signature,
        }),
        Ok(crate::channels::QqHandlerResult::EventHandled) => Ok(QqWebhookOutcome::EventHandled),
        Err(e) => {
            let msg_str = e.to_string();
            let msg = if msg_str.contains("verify") || msg_str.contains("signature") {
                tr(Message::InvalidToken, loc)
            } else if msg_str.contains("too large") {
                tr(Message::BodyTooLarge, loc)
            } else {
                tr(Message::OperationFailed, loc)
            };
            let r = if msg_str.contains("verify") || msg_str.contains("signature") {
                ApiResponse::err_401(&msg)
            } else if msg_str.contains("too large") {
                ApiResponse::err_413(&msg)
            } else {
                ApiResponse::err_400(&msg)
            };
            Err(r)
        }
    }
}
