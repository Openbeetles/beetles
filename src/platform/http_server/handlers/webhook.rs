//! POST /api/webhook：校验 webhook token 后把 body 作为 PcMsg 入队。

use crate::bus::{InboundTx, PcMsg};
use crate::platform::http_server::api_contract;
use crate::platform::http_server::common::{constant_time_eq, ApiResponse};
use std::sync::mpsc::TrySendError;

use super::HandlerContext;

/// 需配对；body 与 provided_token（来自 Header/Query）由 mod 传入。
pub fn post(
    ctx: &HandlerContext,
    inbound_tx: &InboundTx,
    body: String,
    provided_token: &str,
) -> Result<ApiResponse, std::io::Error> {
    let cfg = ctx.config();
    if !cfg.webhook_enabled || cfg.webhook_token.is_empty() {
        return Ok(ApiResponse::err_403_key(api_contract::WEBHOOK_DISABLED));
    }
    if !constant_time_eq(provided_token, &cfg.webhook_token) {
        return Ok(ApiResponse::err_401_key(
            api_contract::WEBHOOK_INVALID_TOKEN,
        ));
    }
    let msg = match PcMsg::new("webhook", "webhook", body) {
        Ok(m) => m,
        Err(_) => {
            return Ok(ApiResponse::err_key(
                413,
                "Payload Too Large",
                api_contract::WEBHOOK_CONTENT_TOO_LONG,
            ));
        }
    };
    match inbound_tx.try_send(msg) {
        Ok(()) => Ok(ApiResponse::ok_200_json("{\"ok\":true}")),
        Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => {
            Ok(ApiResponse::err_503_key(api_contract::COMMON_QUEUE_FULL))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::post;
    use crate::bus::new_inbound_channel;
    use crate::platform::http_server::api_contract;
    use crate::platform::http_server::handlers::build_default_test_handler_context;
    use serde_json::Value;
    use std::time::Duration;

    #[test]
    fn disabled_webhook_uses_error_key_contract() {
        let ctx = build_default_test_handler_context();
        let (inbound_tx, _inbound_rx, _depth) =
            new_inbound_channel(crate::constants::DEFAULT_CAPACITY);

        let response = post(&ctx, &inbound_tx, "{}".to_string(), "").expect("webhook response");

        assert_eq!(response.status, 403);
        let parsed: Value = serde_json::from_slice(&response.body).expect("parse response");
        assert_eq!(parsed["error_key"], api_contract::WEBHOOK_DISABLED);
        assert!(parsed.get("error").is_none(), "body={parsed}");
        assert!(parsed.get("upstream_error").is_none(), "body={parsed}");
    }

    #[test]
    fn invalid_token_uses_error_key_contract() {
        let ctx = build_default_test_handler_context();
        ctx.update_cached_config(|config| {
            config.webhook_enabled = true;
            config.webhook_token = "secret".to_string();
        });
        let (inbound_tx, _inbound_rx, _depth) =
            new_inbound_channel(crate::constants::DEFAULT_CAPACITY);

        let response =
            post(&ctx, &inbound_tx, "{}".to_string(), "wrong").expect("webhook response");

        assert_eq!(response.status, 401);
        let parsed: Value = serde_json::from_slice(&response.body).expect("parse response");
        assert_eq!(parsed["error_key"], api_contract::WEBHOOK_INVALID_TOKEN);
        assert!(parsed.get("error").is_none(), "body={parsed}");
        assert!(parsed.get("upstream_error").is_none(), "body={parsed}");
    }

    #[test]
    fn full_inbound_queue_returns_503_without_blocking() {
        let (inbound_tx, inbound_rx, _depth) = new_inbound_channel(1);
        inbound_tx
            .try_send(crate::bus::PcMsg::new("test", "chat", "queued").expect("seed message"))
            .expect("fill inbound queue");

        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let thread_tx = inbound_tx.clone();
        let handle = std::thread::spawn(move || {
            let ctx = build_default_test_handler_context();
            ctx.update_cached_config(|config| {
                config.webhook_enabled = true;
                config.webhook_token = "secret".to_string();
            });
            let status = post(&ctx, &thread_tx, "{}".to_string(), "secret")
                .expect("webhook response")
                .status;
            let _ = done_tx.send(status);
        });

        match done_rx.recv_timeout(Duration::from_millis(100)) {
            Ok(status) => assert_eq!(status, 503),
            Err(error) => {
                drop(inbound_rx);
                handle
                    .join()
                    .expect("webhook thread exits after receiver drop");
                panic!("webhook post blocked instead of returning 503: {error}");
            }
        }
        drop(inbound_rx);
        handle.join().expect("webhook thread exits");
    }
}
