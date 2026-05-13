//! `/api/sessions` product chat resource.

use super::HandlerContext;
use crate::bus::{MessageTransport, PcMsg};
use crate::memory::{SessionMessageRecord, MAX_SESSION_ENTRIES};
use crate::platform::http_server::common::CORS_AND_EVENT_STREAM;
use crate::platform::http_server::common::CORS_HEADERS;
use crate::platform::http_server::router::{IncomingRequest, OutgoingResponse, RouterEnv};
use crate::state;
use std::sync::mpsc::TrySendError;

const SESSION_LIST_LIMIT_MAX: usize = 50;
const SESSION_MESSAGE_LIMIT_MAX: usize = 50;
const SESSION_SUMMARY_RECENT_LIMIT: usize = 8;
const SESSION_TITLE_MAX_CHARS: usize = 32;
const SESSION_PREVIEW_MAX_CHARS: usize = 80;

#[derive(serde::Deserialize)]
struct SessionPostBody {
    #[serde(default)]
    chat_id: String,
    content: String,
}

/// 返回会话列表产品投影。
pub fn body(ctx: &HandlerContext, cursor: Option<&str>, limit: usize) -> Result<String, String> {
    let all_ids = ctx
        .session_store
        .list_chat_ids()
        .map_err(|e| state::sanitize_error_for_log(&e))?;

    let offset = cursor
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(0)
        .min(all_ids.len());
    let limit = limit.clamp(1, SESSION_LIST_LIMIT_MAX);

    let items = all_ids
        .iter()
        .skip(offset)
        .take(limit)
        .map(|chat_id| session_summary(ctx, chat_id))
        .collect::<Result<Vec<_>, _>>()?;
    let next_offset = offset.saturating_add(items.len());
    let next_cursor = (next_offset < all_ids.len()).then(|| next_offset.to_string());

    let response = serde_json::json!({
        "items": items,
        "next_cursor": next_cursor,
        "limit": limit,
    });

    serde_json::to_string(&response).map_err(|e| e.to_string())
}

/// 返回指定 chat_id 的消息历史产品投影。
pub fn detail(
    ctx: &HandlerContext,
    chat_id: &str,
    before: Option<&str>,
    limit: usize,
) -> Result<String, String> {
    let limit = limit.clamp(1, SESSION_MESSAGE_LIMIT_MAX);
    let records_limit = if before.is_some() {
        MAX_SESSION_ENTRIES
    } else {
        limit.saturating_add(1).min(MAX_SESSION_ENTRIES)
    };
    let records = ctx
        .session_store
        .load_recent_records(chat_id, records_limit)
        .map_err(|e| state::sanitize_error_for_log(&e))?;
    let end = before
        .and_then(|message_id| {
            records
                .iter()
                .position(|record| record.message_id == message_id)
        })
        .unwrap_or(records.len());
    let start = end.saturating_sub(limit);
    let items = records[start..end]
        .iter()
        .map(|record| {
            serde_json::json!({
                "message_id": record.message_id,
                "role": record.role,
                "content": record.content,
            })
        })
        .collect::<Vec<_>>();
    let next_before = (start > 0).then(|| records[start].message_id.clone());
    let response = serde_json::json!({
        "items": items,
        "next_before": next_before,
        "limit": limit,
    });
    serde_json::to_string(&response).map_err(|e| e.to_string())
}

/// 创建 Configure UI chat SSE 响应并入队到唯一 agent loop。
pub fn post_stream(
    ctx: &HandlerContext,
    env: &RouterEnv,
    incoming: &IncomingRequest,
) -> OutgoingResponse {
    let accept = incoming.header_ci("Accept").unwrap_or("");
    if !accept.split(',').any(|item| {
        item.trim()
            .split(';')
            .next()
            .is_some_and(|value| value.eq_ignore_ascii_case("text/event-stream"))
    }) {
        return json_error(
            406,
            "Not Acceptable",
            "chat.event_stream_required",
            "sessions_post",
        );
    }

    let body: SessionPostBody = match serde_json::from_slice(incoming.body.as_ref()) {
        Ok(body) => body,
        Err(_) => return json_error(400, "Bad Request", "common.invalid_json", "sessions_post"),
    };
    let chat_id = body.chat_id.trim();
    let content = body.content.trim();
    if chat_id.is_empty() {
        return json_error(400, "Bad Request", "chat.chat_id_required", "sessions_post");
    }
    if content.is_empty() {
        return json_error(400, "Bad Request", "chat.content_required", "sessions_post");
    }

    if let Some((error_key, stage)) = chat_stream_admission_error() {
        return json_error(503, "Service Unavailable", error_key, stage);
    }

    let opened = match ctx.chat_streams.try_open() {
        Ok(opened) => opened,
        Err(crate::chat_stream::ChatStreamOpenError::Busy) => {
            return json_error(409, "Conflict", "chat.stream_busy", "sessions_post");
        }
    };
    let stream_id = opened.stream_id.clone();
    let msg = match PcMsg::new_inbound(
        crate::chat_stream::CHANNEL_CONFIGURE_UI_CHAT,
        chat_id,
        content,
        false,
    ) {
        Ok(msg) => msg
            .with_req_id(Some(stream_id.clone()))
            .with_inbound_provenance(
                MessageTransport::Webhook,
                stream_id.clone(),
                stream_id.clone(),
                stream_id.clone(),
            ),
        Err(error) => {
            ctx.chat_streams
                .emit_error(&stream_id, "chat.message_invalid", Some(error.stage()));
            return OutgoingResponse::stream(200, "OK", CORS_AND_EVENT_STREAM, opened.receiver);
        }
    };
    match env.inbound_tx.try_send(msg) {
        Ok(()) => ctx.chat_streams.emit_queued(&stream_id, chat_id),
        Err(TrySendError::Full(_)) => {
            ctx.chat_streams
                .emit_error(&stream_id, "chat.inbound_queue_full", None);
        }
        Err(TrySendError::Disconnected(_)) => {
            ctx.chat_streams
                .emit_error(&stream_id, "chat.inbound_queue_closed", None);
        }
    }
    OutgoingResponse::stream(200, "OK", CORS_AND_EVENT_STREAM, opened.receiver)
}

/// 删除指定 chat_id 的会话。
pub fn delete(ctx: &HandlerContext, chat_id: &str) -> Result<String, String> {
    ctx.session_store
        .delete(chat_id)
        .map_err(|e| state::sanitize_error_for_log(&e))?;
    Ok(r#"{"ok":true}"#.to_string())
}

fn session_summary(ctx: &HandlerContext, chat_id: &str) -> Result<serde_json::Value, String> {
    let records = ctx
        .session_store
        .load_recent_records(chat_id, SESSION_SUMMARY_RECENT_LIMIT)
        .map_err(|e| state::sanitize_error_for_log(&e))?;
    let message_count = ctx
        .session_store
        .message_count(chat_id)
        .map_err(|e| state::sanitize_error_for_log(&e))?;
    let title = records
        .iter()
        .find(|record| record.role == "user" && !record.content.trim().is_empty())
        .or_else(|| {
            records
                .iter()
                .find(|record| !record.content.trim().is_empty())
        })
        .map(|record| compact_preview(record.content.as_str(), SESSION_TITLE_MAX_CHARS))
        .unwrap_or_else(|| "Untitled chat".to_string());
    let last_message = records.last().map(last_message_projection);
    Ok(serde_json::json!({
        "chat_id": chat_id,
        "title": title,
        "last_message": last_message,
        "message_count": message_count,
    }))
}

fn last_message_projection(record: &SessionMessageRecord) -> serde_json::Value {
    serde_json::json!({
        "message_id": record.message_id,
        "role": record.role,
        "preview": compact_preview(record.content.as_str(), SESSION_PREVIEW_MAX_CHARS),
    })
}

fn compact_preview(content: &str, max_chars: usize) -> String {
    let compact = content.split_whitespace().collect::<Vec<_>>().join(" ");
    let source = if compact.is_empty() {
        content.trim()
    } else {
        compact.as_str()
    };
    let mut out = source.chars().take(max_chars).collect::<String>();
    if source.chars().count() > max_chars {
        out.push_str("...");
    }
    out
}

fn chat_stream_admission_error() -> Option<(&'static str, &'static str)> {
    match crate::orchestrator::can_call_llm_for_channel_pub(
        crate::chat_stream::CHANNEL_CONFIGURE_UI_CHAT,
    ) {
        crate::orchestrator::LlmDecision::Proceed => {}
        crate::orchestrator::LlmDecision::RetryLater { .. } => {
            return Some(("chat.stream_pressure", "sessions_post_llm_admission"));
        }
        crate::orchestrator::LlmDecision::Degrade { .. } => {
            return Some(("chat.stream_pressure", "sessions_post_pressure"));
        }
    }
    let resource = crate::orchestrator::resource_light_snapshot();
    if resource.storage_contention_risk != crate::orchestrator::StorageContentionRisk::Healthy {
        return Some(("chat.stream_storage_busy", "sessions_post_storage"));
    }
    let largest_internal = resource.heap_largest_block_internal as usize;
    if largest_internal > 0
        && largest_internal < crate::constants::TLS_ADMISSION_MIN_LARGEST_BLOCK_BYTES
    {
        return Some(("chat.stream_pressure", "sessions_post_tls_headroom"));
    }
    None
}

fn json_error(
    status: u16,
    status_text: &'static str,
    error_key: &'static str,
    stage: &'static str,
) -> OutgoingResponse {
    let body = serde_json::json!({
        "error_key": error_key,
        "stage": stage,
    });
    OutgoingResponse::json(
        status,
        status_text,
        CORS_HEADERS,
        serde_json::to_vec(&body).unwrap_or_else(|_| br#"{"error_key":"common.error"}"#.to_vec()),
    )
}
