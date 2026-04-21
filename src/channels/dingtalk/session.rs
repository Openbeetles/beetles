//! 钉钉 sessionWebhook 缓存，供回调入站与 sender 共享。
//! Shared DingTalk sessionWebhook cache for inbound callback and sender.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

pub type DingtalkSessionStore = Arc<Mutex<HashMap<String, DingtalkSessionWebhook>>>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DingtalkSessionWebhook {
    pub webhook_url: String,
    pub expires_at_ms: Option<u64>,
}

const SESSION_STORE_MAX: usize = 128;

fn now_unix_ms() -> u64 {
    crate::util::current_unix_secs().saturating_mul(1000)
}

fn prune_locked(cache: &mut HashMap<String, DingtalkSessionWebhook>, now_ms: u64) {
    cache.retain(|_, entry| {
        entry
            .expires_at_ms
            .is_none_or(|expires_at_ms| expires_at_ms > now_ms)
            && !entry.webhook_url.trim().is_empty()
    });
}

fn evict_oldest_locked(cache: &mut HashMap<String, DingtalkSessionWebhook>) {
    while cache.len() > SESSION_STORE_MAX {
        let Some(oldest_key) = cache
            .iter()
            .min_by_key(|(_, entry)| entry.expires_at_ms.unwrap_or(u64::MAX))
            .map(|(chat_id, _)| chat_id.clone())
        else {
            break;
        };
        cache.remove(&oldest_key);
    }
}

pub fn store_session_webhook(
    store: &DingtalkSessionStore,
    chat_id: &str,
    webhook_url: &str,
    expires_at_ms: Option<u64>,
) -> crate::error::Result<()> {
    let chat_id = chat_id.trim();
    let webhook_url = webhook_url.trim();
    if chat_id.is_empty() || webhook_url.is_empty() {
        return Ok(());
    }
    let mut guard = store.lock().map_err(|e| crate::error::Error::Other {
        source: Box::new(std::io::Error::other(e.to_string())),
        stage: "dingtalk_session_store_lock",
    })?;
    let now_ms = now_unix_ms();
    prune_locked(&mut guard, now_ms);
    guard.insert(
        chat_id.to_string(),
        DingtalkSessionWebhook {
            webhook_url: webhook_url.to_string(),
            expires_at_ms,
        },
    );
    evict_oldest_locked(&mut guard);
    Ok(())
}

pub fn active_session_webhook(
    store: &DingtalkSessionStore,
    chat_id: &str,
) -> crate::error::Result<Option<String>> {
    let chat_id = chat_id.trim();
    if chat_id.is_empty() {
        return Ok(None);
    }
    let mut guard = store.lock().map_err(|e| crate::error::Error::Other {
        source: Box::new(std::io::Error::other(e.to_string())),
        stage: "dingtalk_session_store_lock",
    })?;
    let now_ms = now_unix_ms();
    prune_locked(&mut guard, now_ms);
    Ok(guard.get(chat_id).map(|entry| entry.webhook_url.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expired_session_is_not_returned() {
        let store: DingtalkSessionStore = Arc::new(Mutex::new(HashMap::new()));
        let now_ms = now_unix_ms();
        {
            let mut guard = store.lock().expect("lock");
            guard.insert(
                "chat-1".to_string(),
                DingtalkSessionWebhook {
                    webhook_url: "https://example.invalid/expired".to_string(),
                    expires_at_ms: Some(now_ms.saturating_sub(1)),
                },
            );
        }

        let active = active_session_webhook(&store, "chat-1").expect("active");
        assert_eq!(active, None);
    }
}
