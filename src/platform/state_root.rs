//! 状态挂载根路径：ESP 固定 `/spiffs`；host/Linux 由 `BEETLE_STATE_ROOT` 或行业默认（`/var/lib/beetle` → `/data/beetle`），在 `init_spiffs` 中解析并缓存。
//! State mount root: `/spiffs` on ESP; host uses `BEETLE_STATE_ROOT` or FHS defaults, resolved in `init_spiffs`.

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
use crate::error::{Error, Result};
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
use crate::memory::REL_PATH_SESSIONS_DIR;
use std::path::PathBuf;

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
use std::sync::OnceLock;

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
static STATE_ROOT: OnceLock<PathBuf> = OnceLock::new();

/// Host：解析并创建状态根（及 `nvs/`）。幂等；由 `init_spiffs` 调用。失败时返回明确错误（须设置 `BEETLE_STATE_ROOT` 或保证 `/var/lib` 或 `/data` 可写）。
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub(crate) fn init_host_state_root() -> Result<()> {
    if STATE_ROOT.get().is_some() {
        return Ok(());
    }
    let root = resolve_host_state_root()?;
    std::fs::create_dir_all(&root).map_err(|e| Error::io("state_root", e))?;
    std::fs::create_dir_all(root.join("nvs")).map_err(|e| Error::io("state_root", e))?;
    std::fs::create_dir_all(root.join(REL_PATH_SESSIONS_DIR))
        .map_err(|e| Error::io("state_root", e))?;
    // `SpiffsSkillStorage::list_names` uses read_dir; host must have the dir (unlike single-file stores).
    std::fs::create_dir_all(root.join("skills")).map_err(|e| Error::io("state_root", e))?;
    STATE_ROOT
        .set(root)
        .map_err(|_| Error::config("state_root", "double init_host_state_root"))?;
    Ok(())
}

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
fn resolve_host_state_root() -> Result<PathBuf> {
    if let Ok(s) = std::env::var("BEETLE_STATE_ROOT") {
        let trimmed = s.trim();
        if trimmed.is_empty() {
            return Err(Error::config(
                "state_root",
                "BEETLE_STATE_ROOT is set but empty",
            ));
        }
        let p = PathBuf::from(trimmed);
        std::fs::create_dir_all(&p).map_err(|e| Error::io("state_root", e))?;
        return Ok(p);
    }
    for candidate in ["/var/lib/beetle", "/data/beetle"] {
        let p = PathBuf::from(candidate);
        match std::fs::create_dir_all(&p) {
            Ok(()) => {
                log::info!("[state_root] using default state root: {}", candidate);
                return Ok(p);
            }
            Err(e) => {
                log::debug!(
                    "[state_root] cannot create {}: {} — trying next candidate",
                    candidate,
                    e
                );
            }
        }
    }
    Err(Error::config(
        "state_root",
        "cannot create /var/lib/beetle or /data/beetle; set BEETLE_STATE_ROOT to a writable directory",
    ))
}

/// 返回状态根目录（SPIFFS 挂载点或 host 已初始化的根）。**host 上**须先 `init_spiffs`（会调用 `init_host_state_root`）。
/// Returns state filesystem root. On host, `init_spiffs` must run first.
pub fn state_mount_path() -> PathBuf {
    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
    {
        PathBuf::from("/spiffs")
    }
    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
    {
        STATE_ROOT.get().cloned().unwrap_or_else(|| {
            log::error!("[state_root] not initialized; init_spiffs must run before state_mount_path; using fallback /tmp/beetle");
            PathBuf::from("/tmp/beetle")
        })
    }
}
