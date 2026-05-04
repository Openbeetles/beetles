//! storage 实现的 SkillStorage；目录固定为 storage root/skills，文件名为 {name}.md。
//! SkillStorage implementation over storage; dir = storage root/skills, files = {name}.md.

use crate::error::Result;
use crate::platform::abstraction::SkillStorage;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use super::{list_dir, read_file_to_vec, remove_file, state_path_join, write_file};

const SKILLS_SUBDIR: &str = "skills";
const SKILL_CACHE_MAX_FILES: usize = 64;

fn skills_dir() -> PathBuf {
    state_path_join(SKILLS_SUBDIR)
}

/// storage 上的 skills 目录存储；list_names 返回不含 .md 的名称。
pub struct StorageSkillStorage;

#[derive(Default)]
struct SkillStorageCache {
    list_loaded: bool,
    names: Vec<String>,
    files: std::collections::HashMap<String, Vec<u8>>,
}

/// 进程内技能缓存：将高频 prompt/runtime skill 读取从 storage 热路径移出。
pub struct CachedSkillStorage {
    inner: Arc<dyn SkillStorage + Send + Sync>,
    cache: Mutex<SkillStorageCache>,
}

impl CachedSkillStorage {
    pub fn wrap(inner: Arc<dyn SkillStorage + Send + Sync>) -> Arc<dyn SkillStorage + Send + Sync> {
        Arc::new(Self {
            inner,
            cache: Mutex::new(SkillStorageCache::default()),
        }) as Arc<dyn SkillStorage + Send + Sync>
    }

    fn cache_file(cache: &mut SkillStorageCache, name: &str, content: &[u8]) {
        if !cache.files.contains_key(name) && cache.files.len() >= SKILL_CACHE_MAX_FILES {
            cache.files.clear();
        }
        cache.files.insert(name.to_string(), content.to_vec());
    }
}

impl SkillStorage for StorageSkillStorage {
    fn list_names(&self) -> Result<Vec<String>> {
        let dir = skills_dir();
        let names = list_dir(&dir)?;
        let out: Vec<String> = names
            .into_iter()
            .filter(|n| n.ends_with(".md"))
            .map(|n| n.trim_end_matches(".md").to_string())
            .collect();
        Ok(out)
    }

    fn read(&self, name: &str) -> Result<Vec<u8>> {
        let mut path = skills_dir();
        path.push(format!("{}.md", name));
        read_file_to_vec(&path)
    }

    fn write(&self, name: &str, content: &[u8]) -> Result<()> {
        let mut path = skills_dir();
        path.push(format!("{}.md", name));
        write_file(&path, content)
    }

    fn remove(&self, name: &str) -> Result<()> {
        let mut path = skills_dir();
        path.push(format!("{}.md", name));
        remove_file(&path)
    }
}

impl SkillStorage for CachedSkillStorage {
    fn list_names(&self) -> Result<Vec<String>> {
        {
            let cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
            if cache.list_loaded {
                return Ok(cache.names.clone());
            }
        }
        let names = self.inner.list_names()?;
        let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        cache.list_loaded = true;
        cache.names = names.clone();
        Ok(names)
    }

    fn read(&self, name: &str) -> Result<Vec<u8>> {
        {
            let cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(content) = cache.files.get(name) {
                return Ok(content.clone());
            }
        }
        let content = self.inner.read(name)?;
        let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        Self::cache_file(&mut cache, name, &content);
        Ok(content)
    }

    fn write(&self, name: &str, content: &[u8]) -> Result<()> {
        self.inner.write(name, content)?;
        let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        if cache.list_loaded && !cache.names.iter().any(|candidate| candidate == name) {
            cache.names.push(name.to_string());
        }
        Self::cache_file(&mut cache, name, content);
        Ok(())
    }

    fn remove(&self, name: &str) -> Result<()> {
        self.inner.remove(name)?;
        let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        cache.files.remove(name);
        if cache.list_loaded {
            cache.names.retain(|candidate| candidate != name);
        }
        Ok(())
    }
}

/// 默认 skill 存储的 Arc，供跨线程使用。步骤 4 过渡；步骤 5 后 main 改用 platform.skill_storage()。
pub fn default_skill_storage_arc() -> std::sync::Arc<dyn SkillStorage + Send + Sync> {
    CachedSkillStorage::wrap(Arc::new(StorageSkillStorage))
}
