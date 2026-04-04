//! SPIFFS 实现的 MemoryStore。路径 = SPIFFS_BASE + memory::REL_PATH_*。
//! MemoryStore implementation over SPIFFS.

use crate::error::{Error, Result};
use crate::memory::{
    MemoryStore, MAX_MEMORY_CONTENT_LEN, MAX_SOUL_USER_LEN, REL_PATH_DAILY_DIR, REL_PATH_MEMORY,
    REL_PATH_SOUL, REL_PATH_USER,
};
use std::path::PathBuf;

use super::{list_dir, read_file, state_path_join, write_file};

fn full_path(rel: &str) -> PathBuf {
    state_path_join(rel)
}

/// MemoryStore 的 SPIFFS 实现。
pub struct SpiffsMemoryStore;

impl Default for SpiffsMemoryStore {
    fn default() -> Self {
        Self::new()
    }
}

impl SpiffsMemoryStore {
    pub fn new() -> Self {
        SpiffsMemoryStore
    }
}

impl MemoryStore for SpiffsMemoryStore {
    fn get_memory(&self) -> Result<String> {
        let buf = read_file(full_path(REL_PATH_MEMORY))?;
        Ok(String::from_utf8_lossy(&buf).into_owned())
    }

    fn set_memory(&self, content: &str) -> Result<()> {
        if content.len() > MAX_MEMORY_CONTENT_LEN {
            return Err(Error::config(
                "set_memory",
                format!(
                    "content length {} exceeds {}",
                    content.len(),
                    MAX_MEMORY_CONTENT_LEN
                ),
            ));
        }
        write_file(full_path(REL_PATH_MEMORY), content.as_bytes())
    }

    fn get_soul(&self) -> Result<String> {
        let buf = read_file(full_path(REL_PATH_SOUL))?;
        Ok(String::from_utf8_lossy(&buf).into_owned())
    }

    fn set_soul(&self, content: &str) -> Result<()> {
        if content.len() > MAX_SOUL_USER_LEN {
            return Err(Error::config(
                "set_soul",
                format!(
                    "content length {} exceeds {}",
                    content.len(),
                    MAX_SOUL_USER_LEN
                ),
            ));
        }
        write_file(full_path(REL_PATH_SOUL), content.as_bytes())
    }

    fn get_user(&self) -> Result<String> {
        let buf = read_file(full_path(REL_PATH_USER))?;
        Ok(String::from_utf8_lossy(&buf).into_owned())
    }

    fn set_user(&self, content: &str) -> Result<()> {
        if content.len() > MAX_SOUL_USER_LEN {
            return Err(Error::config(
                "set_user",
                format!(
                    "content length {} exceeds {}",
                    content.len(),
                    MAX_SOUL_USER_LEN
                ),
            ));
        }
        write_file(full_path(REL_PATH_USER), content.as_bytes())
    }

    fn list_daily_note_names(&self, recent_n: usize) -> Result<Vec<String>> {
        let dir = full_path(REL_PATH_DAILY_DIR);
        let names = match list_dir(&dir) {
            Ok(n) => n,
            Err(_) => return Ok(Vec::new()),
        };
        let mut names = names;
        names.sort_by(|a, b| b.cmp(a));
        names.truncate(recent_n);
        Ok(names)
    }

    fn get_daily_note(&self, name: &str) -> Result<String> {
        let mut p = state_path_join(REL_PATH_DAILY_DIR);
        p.push(name);
        let buf = read_file(&p)?;
        Ok(String::from_utf8_lossy(&buf).into_owned())
    }

    fn write_daily_note(&self, name: &str, content: &str) -> Result<()> {
        if content.len() > MAX_MEMORY_CONTENT_LEN {
            return Err(Error::config(
                "write_daily_note",
                format!(
                    "content length {} exceeds {}",
                    content.len(),
                    MAX_MEMORY_CONTENT_LEN
                ),
            ));
        }
        let mut p = state_path_join(REL_PATH_DAILY_DIR);
        p.push(name);
        write_file(&p, content.as_bytes())
    }
}
