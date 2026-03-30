//! POST /api/webhook：校验 webhook token 后把 body 作为 PcMsg 入队。

use crate::bus::{InboundTx, PcMsg};
use crate::i18n::{locale_from_store, tr, Message};
use crate::platform::http_server::common::{constant_time_eq, ApiResponse};

use super::HandlerContext;

/// 需配对；body 与 provided_token（来自 Header/Query）由 mod 传入。
pub fn post(
    ctx: &HandlerContext,
    inbound_tx: &InboundTx,
    body: String,
    provided_token: &str,
) -> Result<ApiResponse, std::io::Error> {
    let loc = locale_from_store(ctx.config_store.as_ref());
    let cfg = ctx.config();
    if !cfg.webhook_enabled || cfg.webhook_token.is_empty() {
        return Ok(ApiResponse::err_403(&tr(Message::WebhookDisabled, loc)));
    }
    if !constant_time_eq(provided_token, &cfg.webhook_token) {
        return Ok(ApiResponse::err_401(&tr(Message::InvalidToken, loc)));
    }
    let msg = match PcMsg::new("webhook", "webhook", body) {
        Ok(m) => m,
        Err(_) => return Ok(ApiResponse::err_413(&tr(Message::ContentTooLong, loc))),
    };
    if inbound_tx.send(msg).is_err() {
        return Ok(ApiResponse::err_503(&tr(Message::QueueFull, loc)));
    }
    Ok(ApiResponse::ok_200_json("{\"ok\":true}"))
}
