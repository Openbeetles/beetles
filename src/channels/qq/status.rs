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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum QqWsStatusRegistration {
    Registered,
    AlreadyRegisteredSameOwner,
    AlreadyRegisteredDifferentOwner,
}

struct RegisteredQqWsStatus {
    owner: &'static str,
    status: SharedQqWsStatus,
}

static QQ_WS_STATUS: OnceLock<RegisteredQqWsStatus> = OnceLock::new();

pub fn new_shared_qq_ws_status() -> SharedQqWsStatus {
    SharedQqWsStatus::new()
}

pub(crate) fn register_shared_qq_ws_status(status: SharedQqWsStatus) {
    match register_shared_qq_ws_status_for_owner("qq_ws", status) {
        QqWsStatusRegistration::Registered | QqWsStatusRegistration::AlreadyRegisteredSameOwner => {
        }
        QqWsStatusRegistration::AlreadyRegisteredDifferentOwner => {
            log::warn!("[qq_status] shared ws status owned by another worker")
        }
    }
}

pub(crate) fn register_shared_qq_ws_status_for_owner(
    owner: &'static str,
    status: SharedQqWsStatus,
) -> QqWsStatusRegistration {
    if let Some(existing) = QQ_WS_STATUS.get() {
        if existing.owner == owner {
            return QqWsStatusRegistration::AlreadyRegisteredSameOwner;
        }
        return QqWsStatusRegistration::AlreadyRegisteredDifferentOwner;
    }
    match QQ_WS_STATUS.set(RegisteredQqWsStatus { owner, status }) {
        Ok(()) => QqWsStatusRegistration::Registered,
        Err(existing) if existing.owner == owner => {
            QqWsStatusRegistration::AlreadyRegisteredSameOwner
        }
        Err(_) => QqWsStatusRegistration::AlreadyRegisteredDifferentOwner,
    }
}

pub fn is_ws_online() -> bool {
    QQ_WS_STATUS
        .get()
        .map(|registered| registered.status.is_online())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::{QqWsStatusRegistration, SharedQqWsStatus};

    #[test]
    fn shared_ws_status_defaults_to_offline_and_updates() {
        let status = SharedQqWsStatus::new();
        assert!(!status.is_online());
        status.set_online(true);
        assert!(status.is_online());
        status.set_online(false);
        assert!(!status.is_online());
    }

    #[test]
    fn shared_ws_status_registration_is_idempotent_for_same_owner() {
        let status = SharedQqWsStatus::new();

        assert_eq!(
            super::register_shared_qq_ws_status_for_owner("qq_ws", status.clone()),
            QqWsStatusRegistration::Registered
        );
        assert_eq!(
            super::register_shared_qq_ws_status_for_owner("qq_ws", status),
            QqWsStatusRegistration::AlreadyRegisteredSameOwner
        );
    }
}
