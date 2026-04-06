//! QQ passive-reply msg_id cache shared by sender, webhook, and WSS.
//! QQ 被动回复 msg_id 缓存，供 sender、webhook、WSS 共用。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

/// msg_id cache type: chat_id -> (msg_id, unix_ts).
/// msg_id 缓存类型：chat_id -> (msg_id, unix_ts)。
pub type QqMsgIdCache = Arc<Mutex<HashMap<String, (String, u64)>>>;

/// Passive reply msg_id retention window in seconds.
/// 被动回复 msg_id 保留时长（秒）。
const QQ_MSG_ID_TTL_SECS: u64 = 300;

/// Hard cap for cached msg_id entries.
/// msg_id 缓存最大条目数。
const QQ_MSG_ID_CACHE_MAX: usize = 64;

fn qq_now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn prune_msg_id_cache_locked(cache: &mut HashMap<String, (String, u64)>, now: u64) {
    cache.retain(|_, (_, ts)| now.saturating_sub(*ts) <= QQ_MSG_ID_TTL_SECS);
}

fn evict_oldest_msg_id_entries_locked(cache: &mut HashMap<String, (String, u64)>) {
    while cache.len() > QQ_MSG_ID_CACHE_MAX {
        let Some(oldest_key) = cache
            .iter()
            .min_by_key(|(_, (_, ts))| *ts)
            .map(|(chat_id, _)| chat_id.clone())
        else {
            break;
        };
        cache.remove(&oldest_key);
    }
}

fn insert_msg_id_locked(
    cache: &mut HashMap<String, (String, u64)>,
    chat_id: &str,
    msg_id: &str,
    now: u64,
) {
    prune_msg_id_cache_locked(cache, now);
    cache.insert(chat_id.to_string(), (msg_id.to_string(), now));
    evict_oldest_msg_id_entries_locked(cache);
}

fn pop_msg_id_locked(
    cache: &mut HashMap<String, (String, u64)>,
    chat_id: &str,
    now: u64,
) -> Option<String> {
    prune_msg_id_cache_locked(cache, now);
    cache.remove(chat_id).map(|(msg_id, _)| msg_id)
}

/// Stores a msg_id for later passive reply use.
/// 写入 msg_id，供后续被动回复使用。
pub fn cache_msg_id(cache: &QqMsgIdCache, chat_id: &str, msg_id: &str) -> crate::error::Result<()> {
    let now = qq_now_unix_secs();
    let mut guard = cache.lock().map_err(|e| crate::error::Error::Other {
        source: Box::new(std::io::Error::other(e.to_string())),
        stage: "qq_msg_id_cache_lock",
    })?;
    insert_msg_id_locked(&mut guard, chat_id, msg_id, now);
    Ok(())
}

/// Pops a cached msg_id for the given chat_id if still valid.
/// 取出指定 chat_id 的缓存 msg_id；若已过期则返回 None。
pub(crate) fn pop_msg_id(cache: &QqMsgIdCache, chat_id: &str) -> Option<String> {
    let now = qq_now_unix_secs();
    cache
        .lock()
        .ok()
        .and_then(|mut c| pop_msg_id_locked(&mut c, chat_id, now))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn msg_id_cache_enforces_hard_cap_on_insert() {
        let mut cache = HashMap::new();
        let base = 10_000_u64;
        for idx in 0..QQ_MSG_ID_CACHE_MAX {
            insert_msg_id_locked(
                &mut cache,
                &format!("chat-{idx}"),
                &format!("msg-{idx}"),
                base + idx as u64,
            );
        }

        insert_msg_id_locked(
            &mut cache,
            "chat-new",
            "msg-new",
            base + QQ_MSG_ID_CACHE_MAX as u64 + 1,
        );

        assert_eq!(cache.len(), QQ_MSG_ID_CACHE_MAX);
        assert!(!cache.contains_key("chat-0"));
        assert!(cache.contains_key("chat-1"));
        assert!(cache.contains_key("chat-new"));
    }

    #[test]
    fn msg_id_cache_prunes_expired_entries_before_pop() {
        let mut cache = HashMap::new();
        insert_msg_id_locked(&mut cache, "fresh", "msg-fresh", 100);
        insert_msg_id_locked(&mut cache, "expired", "msg-expired", 100);

        let got = pop_msg_id_locked(&mut cache, "fresh", 100 + QQ_MSG_ID_TTL_SECS - 1);
        assert_eq!(got.as_deref(), Some("msg-fresh"));

        let expired = pop_msg_id_locked(&mut cache, "expired", 100 + QQ_MSG_ID_TTL_SECS + 1);
        assert_eq!(expired, None);
        assert!(!cache.contains_key("expired"));
    }
}
