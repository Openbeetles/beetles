//! HEARTBEAT.md 文件读取：storage 路径与 8KB 上限，不存在或失败返回空。
//! Read HEARTBEAT.md from storage; 8KB cap; empty on missing or error.

use crate::error::Result;
use crate::memory::REL_PATH_HEARTBEAT;
const MAX_HEARTBEAT_LEN: usize = 8192;

/// 读取 HEARTBEAT.md 内容。路径 = storage_BASE + REL_PATH_HEARTBEAT。
/// 文件不存在或读失败返回空字符串；内容超过 8KB 截断。
pub fn read_heartbeat_file() -> Result<String> {
    let mut path = crate::platform::state_root::state_mount_path();
    path.push(REL_PATH_HEARTBEAT);
    let buf = match crate::platform::storage::read_file(&path) {
        Ok(b) => b,
        Err(_) => return Ok(String::new()),
    };
    let capped = if buf.len() > MAX_HEARTBEAT_LEN {
        &buf[..MAX_HEARTBEAT_LEN]
    } else {
        &buf[..]
    };
    Ok(String::from_utf8_lossy(capped).into_owned())
}
