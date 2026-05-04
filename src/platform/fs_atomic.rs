//! 同目录原子替换写入（Linux/host）：tmp + fsync + rename，供 `storage::write_file` 与 NVS JSON 使用。
//! Same-directory atomic replace for host: tmp, fsync, rename.

use crate::error::{Error, Result};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;

/// 将 `data` 原子写入 `path`（覆盖已有文件）。临时文件与目标同目录，避免跨文件系统 rename 失败。
/// Atomically writes `data` to `path`. Temp file lives beside the target for rename atomicity.
pub fn atomic_write(path: &Path, data: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent).map_err(|e| Error::io("atomic_write", e))?;
    let fname = path.file_name().and_then(|s| s.to_str()).unwrap_or("file");
    let tmp = parent.join(format!(".{}.tmp.{}", fname, std::process::id()));
    let write_result = (|| -> std::result::Result<(), std::io::Error> {
        let mut f = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&tmp)?;
        f.write_all(data)?;
        f.sync_all()?;
        Ok(())
    })();
    if write_result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    write_result.map_err(|e| Error::io("atomic_write", e))?;
    match std::fs::rename(&tmp, path) {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(Error::io("atomic_write", e))
        }
    }
}
