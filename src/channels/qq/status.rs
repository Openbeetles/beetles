//! QQ WebSocket 在线状态共享。
//! Shared QQ WebSocket online state.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

#[derive(Clone, Debug, Default)]
pub struct SharedQqWsStatus {
    online: Arc<AtomicBool>,
}

impl SharedQqWsStatus {
    pub fn new() -> Self {
        Self {
            online: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn set_online(&self, online: bool) {
        self.online.store(online, Ordering::Relaxed);
    }

    pub fn is_online(&self) -> bool {
        self.online.load(Ordering::Relaxed)
    }
}

static QQ_WS_STATUS: OnceLock<SharedQqWsStatus> = OnceLock::new();

pub fn new_shared_qq_ws_status() -> SharedQqWsStatus {
    SharedQqWsStatus::new()
}

pub(crate) fn register_shared_qq_ws_status(status: SharedQqWsStatus) {
    if QQ_WS_STATUS.set(status).is_err() {
        log::error!("[qq_status] shared ws status already registered");
    }
}

pub fn is_ws_online() -> bool {
    QQ_WS_STATUS
        .get()
        .map(SharedQqWsStatus::is_online)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::SharedQqWsStatus;

    #[test]
    fn shared_ws_status_defaults_to_offline_and_updates() {
        let status = SharedQqWsStatus::new();
        assert!(!status.is_online());
        status.set_online(true);
        assert!(status.is_online());
        status.set_online(false);
        assert!(!status.is_online());
    }
}
