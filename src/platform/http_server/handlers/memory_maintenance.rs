//! POST /api/memory/maintenance: structured operator-requested maintenance execution.

use super::HandlerContext;
use crate::platform::http_server::common::ApiResponse;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct MemoryMaintenancePostBody {
    action: crate::runtime::OperatorMaintenanceAction,
    #[serde(default)]
    chat_id: Option<String>,
    #[serde(default)]
    channel: Option<String>,
}

pub fn post(ctx: &HandlerContext, body: &str) -> ApiResponse {
    let payload: MemoryMaintenancePostBody = match serde_json::from_str(body) {
        Ok(payload) => payload,
        Err(error) => {
            return ApiResponse::err_400(&format!(
                "invalid operator maintenance request: {}",
                error
            ));
        }
    };
    let request = crate::runtime::OperatorMaintenanceRequest::new(
        payload.action,
        payload.chat_id,
        payload.channel,
        "control_plane_http",
    );
    match crate::runtime::submit_operator_maintenance_request(
        ctx.system_inbound_tx.as_ref(),
        request,
    ) {
        Ok(submission) => {
            let body = serde_json::to_vec(&submission)
                .unwrap_or_else(|_| br#"{"accepted":true}"#.to_vec());
            ApiResponse {
                status: 202,
                status_text: "Accepted",
                body,
            }
        }
        Err(error) => {
            let text = error.to_string();
            if text.contains("queue unavailable") {
                ApiResponse::err_503(&text)
            } else {
                ApiResponse::err_500(&text)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::post;
    use crate::bus::new_inbound_channel;
    use crate::platform::http_server::handlers::build_default_test_handler_context;
    use serde_json::Value;

    #[test]
    fn post_enqueues_structured_operator_maintenance_request() {
        let (system_inbound_tx, system_inbound_rx, _depth) =
            new_inbound_channel(crate::constants::DEFAULT_CAPACITY);
        let mut ctx = build_default_test_handler_context();
        ctx.system_inbound_tx = Some(system_inbound_tx);

        let response = post(
            &ctx,
            r#"{"action":"run_repair_plan","chat_id":"chat-1","channel":"qq_channel"}"#,
        );

        assert_eq!(response.status, 202);
        let parsed: Value = serde_json::from_slice(&response.body).expect("parse response");
        assert_eq!(parsed["accepted"], true);
        assert_eq!(parsed["delivery"], "in_memory");
        let queued = system_inbound_rx.try_recv().expect("queued request");
        assert_eq!(
            queued.channel.as_ref(),
            crate::runtime::CHANNEL_OPERATOR_MAINTENANCE
        );
    }
}
