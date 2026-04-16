use super::TAG;
use beetle::bus::IngressKind;
use beetle::memory::MemoryStore;
use beetle::Platform;
use beetle::RuntimeServices;
use std::sync::Arc;

/// 启动自检：存储可读（memory 或 soul 至少其一成功）。失败返回 false，调用方应 log 并 return。
pub(crate) fn startup_self_check(memory_store: &dyn MemoryStore) -> bool {
    memory_store.get_memory().is_ok() || memory_store.get_soul().is_ok()
}

/// 首次启动或空存储：当 get_memory 与 get_soul 均失败时写入占位数据，使后续自检可过、业务可进（如引导配置）。
pub(crate) fn ensure_storage_ready(memory_store: &dyn MemoryStore) {
    let need_memory = memory_store.get_memory().is_err();
    let need_soul = memory_store.get_soul().is_err();
    let need_user = memory_store.get_user().is_err();
    if !need_memory && !need_soul && !need_user {
        return;
    }
    log::info!(
        "[{}] preparing default storage files memory_missing={} soul_missing={} user_missing={}",
        TAG,
        need_memory,
        need_soul,
        need_user
    );
    if need_memory {
        if let Err(error) = memory_store.set_memory("") {
            log::warn!("[{}] set_memory default failed: {}", TAG, error);
        }
    }
    if need_soul {
        if let Err(error) = memory_store.set_soul("") {
            log::warn!("[{}] set_soul default failed: {}", TAG, error);
        }
    }
    if need_user {
        if let Err(error) = memory_store.set_user("") {
            log::warn!("[{}] set_user default failed: {}", TAG, error);
        }
    }
}

pub(crate) fn log_runtime_store_lengths(runtime: &RuntimeServices) {
    if let Ok(memory) = runtime.memory_store.get_memory() {
        log::info!("[{}] memory len={}", TAG, memory.len());
    } else {
        log::warn!("[{}] memory read failed or empty", TAG);
    }
    if let Ok(soul) = runtime.memory_store.get_soul() {
        log::info!("[{}] soul len={}", TAG, soul.len());
    } else {
        log::warn!("[{}] soul read failed", TAG);
    }
    if let Ok(user) = runtime.memory_store.get_user() {
        log::info!("[{}] user len={}", TAG, user.len());
    } else {
        log::warn!("[{}] user read failed", TAG);
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
    if let Err(error) = pending_retry.clear_pending_retry() {
        log::warn!(
            "[main] pending_retry clear failed during bootstrap: {}",
            error
        );
    }
    let inbound_tx = match msg.ingress {
        IngressKind::User => user_inbound_tx,
        IngressKind::System => system_inbound_tx,
    };
    if let Err(error) = inbound_tx.try_send(msg) {
        log::warn!("[main] pending_retry bootstrap enqueue failed: {}", error);
    }
}
