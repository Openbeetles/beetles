//! Linux / host：`std::fs` + `state_mount_path()`，写入语义与 ESP 状态存储一致。
//! Linux/host: std::fs under state_mount_path; write semantics align with ESP state storage.

use crate::error::{Error, Result};
use crate::platform::abstraction::StateFs;
use crate::platform::state_root::state_mount_path;
use crate::platform::storage::MAX_WRITE_SIZE;
use std::io::ErrorKind;
use std::path::PathBuf;

/// Linux/host 使用底层文件系统并发能力；写路径采用同目录原子替换。
pub struct LinuxStateFs;

impl Default for LinuxStateFs {
    fn default() -> Self {
        Self
    }
}

fn abs(rel_path: &str) -> Result<PathBuf> {
    let rel = crate::util::normalize_state_rel_path(rel_path)?;
    Ok(state_mount_path().join(rel))
}

fn map_read(r: std::result::Result<Vec<u8>, std::io::Error>) -> Result<Option<Vec<u8>>> {
    match r {
        Ok(b) => Ok(Some(b)),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
        Err(e) => Err(Error::io("state_fs", e)),
    }
}

impl StateFs for LinuxStateFs {
    fn read(&self, rel_path: &str) -> Result<Option<Vec<u8>>> {
        let path = abs(rel_path)?;
        map_read(std::fs::read(&path))
    }

    fn write(&self, rel_path: &str, data: &[u8]) -> Result<()> {
        if data.len() > MAX_WRITE_SIZE {
            return Err(Error::config(
                "state_fs",
                format!("write size {} exceeds limit {}", data.len(), MAX_WRITE_SIZE),
            ));
        }
        let path = abs(rel_path)?;
        crate::platform::fs_atomic::atomic_write(&path, data)
    }

    fn remove(&self, rel_path: &str) -> Result<()> {
        let path = abs(rel_path)?;
        match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
            Err(e) => Err(Error::io("state_fs", e)),
        }
    }

    fn exists(&self, rel_path: &str) -> Result<bool> {
        let path = abs(rel_path)?;
        match std::fs::metadata(&path) {
            Ok(metadata) => Ok(metadata.is_file()),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(false),
            Err(e) => Err(Error::io("state_fs", e)),
        }
    }

    fn list_dir(&self, rel_path: &str) -> Result<Vec<String>> {
        let rel = crate::util::normalize_state_rel_path(rel_path)?;
        let path = state_mount_path().join(rel);
        let mut names = Vec::new();
        for e in std::fs::read_dir(&path).map_err(|e| Error::io("state_fs", e))? {
            let e = e.map_err(|e| Error::io("state_fs", e))?;
            if let Some(s) = e.file_name().to_str() {
                let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
                names.push(if is_dir {
                    format!("{}/", s)
                } else {
                    s.to_string()
                });
            }
        }
        Ok(names)
    }
}
