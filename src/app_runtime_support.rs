use super::TAG;
use beetle::bus::IngressKind;
use beetle::memory::MemoryStore;
use beetle::Platform;
use beetle::RuntimeServices;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

const QQ_C2C_PENDING_RETRY_PASSIVE_REPLY_TTL_MS: u64 = 60 * 60 * 1000;
const QQ_GROUP_PENDING_RETRY_PASSIVE_REPLY_TTL_MS: u64 = 5 * 60 * 1000;

/// 启动自检：共享 memory 可读。失败返回 false，调用方应 log 并 return。
pub(crate) fn startup_self_check(memory_store: &dyn MemoryStore) -> bool {
    memory_store.get_memory().is_ok()
}

/// 首次启动或空存储：当 get_memory 失败时写入占位数据，使后续自检可过、业务可进（如引导配置）。
pub(crate) fn ensure_storage_ready(memory_store: &dyn MemoryStore) {
    let need_memory = memory_store.get_memory().is_err();
    if !need_memory {
        return;
    }
    log::info!(
        "[{}] preparing default storage files memory_missing={}",
        TAG,
        need_memory,
    );
    if need_memory {
        if let Err(error) = memory_store.set_memory("") {
            log::warn!("[{}] set_memory default failed: {}", TAG, error);
        }
    }
}

pub(crate) fn log_runtime_store_lengths(runtime: &RuntimeServices) {
    if let Ok(memory) = runtime.memory_store.get_memory() {
        log::info!("[{}] memory len={}", TAG, memory.len());
    } else {
        log::warn!("[{}] memory read failed or empty", TAG);
    }
}

pub(crate) fn record_startup_failure_and_request_restart(
    platform: &Arc<dyn Platform>,
    error: &beetle::Error,
    reason: &'static str,
) {
    beetle::state::set_last_error(error);
    beetle::runtime::request_restart_with_continuity_flush(Arc::clone(platform), None, reason);
}

pub(crate) fn bootstrap_pending_retry_into_inbound(
    pending_retry: &dyn beetle::memory::PendingRetryStore,
    user_inbound_tx: &beetle::bus::UserInboundTx,
    system_inbound_tx: &beetle::bus::SystemInboundTx,
) {
    let Ok(Some(msg)) = pending_retry.load_pending_retry() else {
        return;
    };
    if should_drop_stale_pending_retry(&msg, current_unix_ms()) {
        beetle::metrics::record_event_ingress_stale_drop();
        if let Err(error) = pending_retry.clear_pending_retry() {
            log::warn!(
                "[main] stale pending_retry clear failed during bootstrap: {}",
                error
            );
        }
        log::warn!(
            "[main] stale QQ pending_retry dropped during bootstrap channel={} chat_id={} age_ms={}",
            msg.channel,
            msg.chat_id,
            current_unix_ms().saturating_sub(msg.enqueue_ts_ms)
        );
        return;
    }
    let inbound_tx = match msg.ingress {
        IngressKind::User => user_inbound_tx,
        IngressKind::System => system_inbound_tx,
    };
    match inbound_tx.try_send(msg) {
        Ok(()) => {
            beetle::metrics::record_event_ingress_enqueued();
            if let Err(error) = pending_retry.clear_pending_retry() {
                log::warn!(
                    "[main] pending_retry clear failed during bootstrap: {}",
                    error
                );
            }
        }
        Err(std::sync::mpsc::TrySendError::Full(_)) => {
            beetle::metrics::record_inbound_queue_full();
            beetle::metrics::record_inbound_defer();
            beetle::metrics::record_event_ingress_rejected();
            log::warn!("[main] pending_retry bootstrap skipped because inbound queue is full");
        }
        Err(std::sync::mpsc::TrySendError::Disconnected(_)) => {
            beetle::metrics::record_inbound_drop();
            beetle::metrics::record_event_ingress_rejected();
            log::warn!("[main] pending_retry bootstrap skipped because inbound queue is closed");
        }
    }
}

fn should_drop_stale_pending_retry(msg: &beetle::PcMsg, now_ms: u64) -> bool {
    if msg.channel.as_ref() != beetle::CHANNEL_QQ_CHANNEL {
        return false;
    }
    let Some(ttl_ms) = qq_v2_passive_reply_ttl_ms(msg.chat_id.as_ref()) else {
        return false;
    };
    now_ms.saturating_sub(msg.enqueue_ts_ms) > ttl_ms
}

fn qq_v2_passive_reply_ttl_ms(chat_id: &str) -> Option<u64> {
    if chat_id.starts_with("c2c:") {
        Some(QQ_C2C_PENDING_RETRY_PASSIVE_REPLY_TTL_MS)
    } else if chat_id.starts_with("group:") {
        Some(QQ_GROUP_PENDING_RETRY_PASSIVE_REPLY_TTL_MS)
    } else {
        None
    }
}

fn current_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_millis().min(u64::MAX as u128) as u64)
        .unwrap_or(0)
}
