//! 飞书 HTTP 入站 message_id 去重缓存。
//! Shared message_id dedup cache for Feishu HTTP webhook delivery.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

pub type FeishuMessageDedupStore = Arc<Mutex<HashMap<String, u64>>>;

const FEISHU_MESSAGE_DEDUP_TTL_SECS: u64 = 300;
const FEISHU_MESSAGE_DEDUP_MAX: usize = 256;

fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn prune_locked(cache: &mut HashMap<String, u64>, now: u64) {
    cache.retain(|_, ts| now.saturating_sub(*ts) <= FEISHU_MESSAGE_DEDUP_TTL_SECS);
}

fn evict_oldest_locked(cache: &mut HashMap<String, u64>) {
    while cache.len() > FEISHU_MESSAGE_DEDUP_MAX {
        let Some(oldest_key) = cache
            .iter()
            .min_by_key(|(_, ts)| *ts)
            .map(|(message_id, _)| message_id.clone())
        else {
            break;
        };
        cache.remove(&oldest_key);
    }
}

pub fn consume_message_id(
    store: &FeishuMessageDedupStore,
    message_id: &str,
) -> crate::error::Result<bool> {
    let normalized = message_id.trim();
    if normalized.is_empty() {
        return Ok(false);
    }
    let now = now_unix_secs();
    let mut guard = store.lock().map_err(|e| crate::error::Error::Other {
        source: Box::new(std::io::Error::other(e.to_string())),
        stage: "feishu_message_dedup_lock",
    })?;
    prune_locked(&mut guard, now);
    if guard.contains_key(normalized) {
        return Ok(true);
    }
    guard.insert(normalized.to_string(), now);
    evict_oldest_locked(&mut guard);
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reject_duplicate_message_id_until_ttl_expires() {
        let store: FeishuMessageDedupStore = Arc::new(Mutex::new(HashMap::new()));

        assert!(!consume_message_id(&store, "om_1").expect("first"));
        assert!(consume_message_id(&store, "om_1").expect("duplicate"));

        let now = now_unix_secs();
        let mut guard = store.lock().expect("lock");
        guard.insert(
            "om_1".to_string(),
            now.saturating_sub(FEISHU_MESSAGE_DEDUP_TTL_SECS + 1),
        );
        drop(guard);

        assert!(!consume_message_id(&store, "om_1").expect("expired"));
    }
}
