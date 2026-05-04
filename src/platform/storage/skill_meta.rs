//! 技能元数据（顺序、禁用列表）存 storage config/skills_meta.json，避免 NVS 高频单键写触发 4361。

use crate::error::{Error, Result};
use crate::platform::abstraction::SkillMetaStore;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use super::{read_file, state_path_join, write_json_file};

const REL_PATH: &str = "config/skills_meta.json";

fn full_path() -> PathBuf {
    state_path_join(REL_PATH)
}

#[derive(Default, Serialize, Deserialize)]
struct Meta {
    #[serde(default)]
    order: Vec<String>,
    #[serde(default)]
    disabled: Vec<String>,
}

fn is_missing_skill_meta_error(error: &Error) -> bool {
    matches!(
        error,
        Error::Io { source, .. } if source.kind() == std::io::ErrorKind::NotFound
    )
}

fn parse_skill_meta_bytes(buf: &[u8]) -> Result<(Vec<String>, Vec<String>)> {
    let s = String::from_utf8_lossy(buf);
    let meta: Meta =
        serde_json::from_str(&s).map_err(|e| Error::config("skills_meta_parse", e.to_string()))?;
    Ok((meta.order, meta.disabled))
}

/// storage 实现的 SkillMetaStore；单文件 config/skills_meta.json。
pub struct StorageSkillMetaStore;

#[derive(Default)]
struct SkillMetaCache {
    loaded: bool,
    order: Vec<String>,
    disabled: Vec<String>,
}

/// 进程内 skill meta 缓存，避免热路径反复读取 storage config/skills_meta.json。
pub struct CachedSkillMetaStore {
    inner: Arc<dyn SkillMetaStore + Send + Sync>,
    cache: Mutex<SkillMetaCache>,
}

impl CachedSkillMetaStore {
    pub fn wrap(
        inner: Arc<dyn SkillMetaStore + Send + Sync>,
    ) -> Arc<dyn SkillMetaStore + Send + Sync> {
        Arc::new(Self {
            inner,
            cache: Mutex::new(SkillMetaCache::default()),
        }) as Arc<dyn SkillMetaStore + Send + Sync>
    }

    fn update_cache(cache: &mut SkillMetaCache, order: &[String], disabled: &[String]) {
        cache.loaded = true;
        cache.order = order.to_vec();
        cache.disabled = disabled.to_vec();
    }
}

impl SkillMetaStore for StorageSkillMetaStore {
    fn read_meta(&self) -> Result<(Vec<String>, Vec<String>)> {
        let buf = match read_file(full_path()) {
            Ok(b) => b,
            Err(error) if is_missing_skill_meta_error(&error) => {
                return Ok((Vec::new(), Vec::new()))
            }
            Err(error) => return Err(error.with_stage("skills_meta_read")),
        };
        parse_skill_meta_bytes(&buf)
    }

    fn write_meta(&self, order: &[String], disabled: &[String]) -> Result<()> {
        let meta = Meta {
            order: order.to_vec(),
            disabled: disabled.to_vec(),
        };
        let json = serde_json::to_string(&meta)
            .map_err(|e| Error::config("skills_meta", e.to_string()))?;
        write_json_file(full_path(), json.as_bytes())
    }
}

impl SkillMetaStore for CachedSkillMetaStore {
    fn read_meta(&self) -> Result<(Vec<String>, Vec<String>)> {
        {
            let cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
            if cache.loaded {
                return Ok((cache.order.clone(), cache.disabled.clone()));
            }
        }
        let (order, disabled) = self.inner.read_meta()?;
        let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        Self::update_cache(&mut cache, &order, &disabled);
        Ok((order, disabled))
    }

    fn write_meta(&self, order: &[String], disabled: &[String]) -> Result<()> {
        self.inner.write_meta(order, disabled)?;
        let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        Self::update_cache(&mut cache, order, disabled);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Default)]
    struct CountingMetaStore {
        order: Mutex<Vec<String>>,
        disabled: Mutex<Vec<String>>,
        reads: AtomicUsize,
        writes: AtomicUsize,
    }

    impl CountingMetaStore {
        fn new(order: &[&str], disabled: &[&str]) -> Self {
            Self {
                order: Mutex::new(order.iter().map(|value| (*value).to_string()).collect()),
                disabled: Mutex::new(disabled.iter().map(|value| (*value).to_string()).collect()),
                reads: AtomicUsize::new(0),
                writes: AtomicUsize::new(0),
            }
        }
    }

    impl SkillMetaStore for CountingMetaStore {
        fn read_meta(&self) -> Result<(Vec<String>, Vec<String>)> {
            self.reads.fetch_add(1, Ordering::SeqCst);
            Ok((
                self.order.lock().unwrap_or_else(|e| e.into_inner()).clone(),
                self.disabled
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .clone(),
            ))
        }

        fn write_meta(&self, order: &[String], disabled: &[String]) -> Result<()> {
            self.writes.fetch_add(1, Ordering::SeqCst);
            *self.order.lock().unwrap_or_else(|e| e.into_inner()) = order.to_vec();
            *self.disabled.lock().unwrap_or_else(|e| e.into_inner()) = disabled.to_vec();
            Ok(())
        }
    }

    #[test]
    fn cached_meta_store_reads_inner_once_until_write() {
        let store = Arc::new(CountingMetaStore::new(&["alpha"], &["beta"]));
        let cached =
            CachedSkillMetaStore::wrap(Arc::clone(&store) as Arc<dyn SkillMetaStore + Send + Sync>);

        assert_eq!(
            cached.read_meta().unwrap(),
            (vec!["alpha".to_string()], vec!["beta".to_string()])
        );
        assert_eq!(
            cached.read_meta().unwrap(),
            (vec!["alpha".to_string()], vec!["beta".to_string()])
        );
        assert_eq!(store.reads.load(Ordering::SeqCst), 1);

        let order = vec!["gamma".to_string()];
        let disabled = vec!["delta".to_string()];
        cached.write_meta(&order, &disabled).unwrap();

        assert_eq!(
            cached.read_meta().unwrap(),
            (order.clone(), disabled.clone())
        );
        assert_eq!(store.reads.load(Ordering::SeqCst), 1);
        assert_eq!(store.writes.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn parse_skill_meta_bytes_rejects_corrupt_json() {
        let error =
            parse_skill_meta_bytes(br#"{"order":["alpha"],"disabled": }"#).expect_err("corrupt");
        assert_eq!(error.stage(), "skills_meta_parse");
    }
}
