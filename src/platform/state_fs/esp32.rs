//! ESP32：状态文件委托平台存储后端（含存储互斥）。
//! ESP32: state files delegate to the platform storage backend.

use crate::error::{Error, Result};
use crate::platform::abstraction::{StateBytes, StateFs};
use crate::platform::state_root::state_mount_path;
use crate::platform::storage::{self, MAX_WRITE_SIZE};
use std::path::{Path, PathBuf};

/// 零大小类型；存储串行化在平台后端内完成。
#[derive(Debug, Default)]
pub struct Esp32StateFs;

fn abs_path(rel_path: &str) -> Result<PathBuf> {
    let rel = crate::util::normalize_state_rel_path(rel_path)?;
    Ok(state_mount_path().join(storage::esp_storage_rel_path(Path::new(&rel))))
}

fn map_read_result(r: std::result::Result<Vec<u8>, Error>) -> Result<Option<Vec<u8>>> {
    match r {
        Ok(b) => Ok(Some(b)),
        Err(e) => match &e {
            Error::Io { source, .. } if source.kind() == std::io::ErrorKind::NotFound => Ok(None),
            _ => Err(e),
        },
    }
}

fn map_read_bytes_result(
    r: std::result::Result<crate::platform::psram_vec::PsramVec<u8>, Error>,
) -> Result<Option<StateBytes>> {
    match r {
        Ok(b) => Ok(Some(StateBytes::from_psram_vec(b))),
        Err(e) => match &e {
            Error::Io { source, .. } if source.kind() == std::io::ErrorKind::NotFound => Ok(None),
            _ => Err(e),
        },
    }
}

impl StateFs for Esp32StateFs {
    fn read(&self, rel_path: &str) -> Result<Option<Vec<u8>>> {
        let path = abs_path(rel_path)?;
        map_read_result(storage::read_file_to_vec(&path))
    }

    fn read_bytes(&self, rel_path: &str) -> Result<Option<StateBytes>> {
        let path = abs_path(rel_path)?;
        map_read_bytes_result(storage::read_file(&path))
    }

    fn write(&self, rel_path: &str, data: &[u8]) -> Result<()> {
        if data.len() > MAX_WRITE_SIZE {
            return Err(Error::config(
                "state_fs",
                format!("write size {} exceeds limit {}", data.len(), MAX_WRITE_SIZE),
            ));
        }
        let rel = crate::util::normalize_state_rel_path(rel_path)?;
        let rel_path = Path::new(&rel);
        let path = state_mount_path().join(storage::esp_storage_rel_path(rel_path));
        // ESP 存储后端无真实目录：`mkdir`/`create_dir_all` 会返回 Not supported（如 raw_os_error 134）。
        // 带 `/` 的路径由 VFS 直接 `File::create` 即可（见后端 `write_file`）。
        match storage::state_write_tail_padding(rel_path) {
            storage::WriteTailPadding::JsonWhitespace => storage::write_json_file(&path, data),
            storage::WriteTailPadding::Newlines => storage::write_line_file(&path, data),
            storage::WriteTailPadding::None => storage::write_file(&path, data),
        }
    }

    fn remove(&self, rel_path: &str) -> Result<()> {
        let path = abs_path(rel_path)?;
        match storage::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) => match &e {
                Error::Io { source, .. } if source.kind() == std::io::ErrorKind::NotFound => Ok(()),
                _ => Err(e),
            },
        }
    }

    fn exists(&self, rel_path: &str) -> Result<bool> {
        let path = abs_path(rel_path)?;
        match std::fs::metadata(&path) {
            Ok(metadata) => Ok(metadata.is_file()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(Error::io("state_fs", error)),
        }
    }

    fn list_dir(&self, rel_path: &str) -> Result<Vec<String>> {
        let path = abs_path(rel_path)?;
        storage::list_dir(&path)
    }
}
