//! SPIFFS 挂载与路径约定。提供读/写/列目录最小 API，写操作有大小约束。
//! SPIFFS mount and path convention. Min API: read/write/list; write has size limit.
//! ESP-IDF VFS/SPIFFS 多线程并发会引发 fd 错用或死锁，故所有通过本模块的 SPIFFS 访问在 ESP 下由 SPIFFS_MUTEX 串行化。

use crate::error::{Error, Result};
use crate::platform::state_root::state_mount_path;
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
use std::ffi::CString;
use std::io::Read;
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

/// 兼容旧名：状态根路径字符串。ESP 上为 `/spiffs`；host 上为 `state_mount_path()` 的运行时值。
/// Legacy name for state root path string.
pub fn spiffs_base_string() -> String {
    state_mount_path().to_string_lossy().into_owned()
}

/// 与 `state_mount_path().join(rel)` 相同；供各 `Spiffs*` 存储拼接路径。
pub(crate) fn state_path_join(rel: impl AsRef<Path>) -> PathBuf {
    state_mount_path().join(rel.as_ref())
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
static SPIFFS_MUTEX: OnceLock<Mutex<()>> = OnceLock::new();

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
static HOST_SPIFFS_MUTEX: OnceLock<Mutex<()>> = OnceLock::new();

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
fn lock_spiffs() -> std::sync::MutexGuard<'static, ()> {
    SPIFFS_MUTEX
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
fn lock_host_spiffs() -> std::sync::MutexGuard<'static, ()> {
    HOST_SPIFFS_MUTEX
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

pub(crate) fn with_fs_lock<R>(f: impl FnOnce() -> Result<R>) -> Result<R> {
    let wait_start = Instant::now();
    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
    let _guard = lock_spiffs();
    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
    let _guard = lock_host_spiffs();
    crate::metrics::record_spiffs_lock_wait_us(wait_start.elapsed().as_micros());
    let hold_start = Instant::now();
    let result = f();
    crate::metrics::record_spiffs_lock_hold_us(hold_start.elapsed().as_micros());
    result
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
fn with_fs_lock_value<R>(f: impl FnOnce() -> R) -> R {
    let wait_start = Instant::now();
    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
    let _guard = lock_spiffs();
    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
    let _guard = lock_host_spiffs();
    crate::metrics::record_spiffs_lock_wait_us(wait_start.elapsed().as_micros());
    let hold_start = Instant::now();
    let result = f();
    crate::metrics::record_spiffs_lock_hold_us(hold_start.elapsed().as_micros());
    result
}

/// 单次写入最大字节数：ESP 与 SPIFFS 分区一致；host/Linux 放宽至 1MiB（仍与 orchestrator 上界策略独立）。
/// Max write size: ESP SPIFFS bound; host allows 1MiB single-file writes (still bounded).
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
pub(crate) const MAX_WRITE_SIZE: usize = 256 * 1024;
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub(crate) const MAX_WRITE_SIZE: usize = 1024 * 1024;

/// 挂载 SPIFFS（ESP）或创建状态根目录（host）。partition_label=None 表示默认 "storage"；format_if_mount_failed=true。
pub fn init_spiffs() -> Result<()> {
    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
    {
        crate::platform::state_root::init_host_state_root()?;
        let root = state_mount_path();
        log::info!(
            "[platform::spiffs] state root ready (file-backed, not SPIFFS flash): {:?}",
            root
        );
        Ok(())
    }
    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
    {
        let root = state_mount_path();
        let base_str = root
            .to_str()
            .ok_or_else(|| Error::config("spiffs", "invalid state mount path"))?;
        let base = CString::new(base_str).map_err(|e| Error::config("spiffs", e.to_string()))?;
        let conf = esp_idf_svc::sys::esp_vfs_spiffs_conf_t {
            base_path: base.as_ptr(),
            partition_label: std::ptr::null(),
            max_files: 10,
            format_if_mount_failed: true,
        };
        let err = unsafe { esp_idf_svc::sys::esp_vfs_spiffs_register(&conf) };
        if err != 0 {
            return Err(Error::esp("spiffs_register", err));
        }
        let mut total: usize = 0;
        let mut used: usize = 0;
        unsafe { esp_idf_svc::sys::esp_spiffs_info(std::ptr::null(), &mut total, &mut used) };
        log::info!(
            "[platform::spiffs] mounted base={} total={} used={}",
            base_str,
            total,
            used
        );
        Ok(())
    }
}

/// 返回 SPIFFS 总字节数与已用字节数；用于启动自检或运维。失败返回 None（如未挂载）。host 返回 None。
pub fn spiffs_usage() -> Option<(usize, usize)> {
    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
    {
        with_fs_lock_value(|| {
            let mut total: usize = 0;
            let mut used: usize = 0;
            let ret = unsafe {
                esp_idf_svc::sys::esp_spiffs_info(std::ptr::null(), &mut total, &mut used)
            };
            if ret == 0 {
                Some((total, used))
            } else {
                None
            }
        })
    }
    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
    {
        crate::platform::board_info::host_state_root_usage()
    }
}

const PSRAM_FILE_THRESHOLD: usize = 8 * 1024;

/// Allocate a `Vec<u8>` with `len=0, capacity=cap` backed by PSRAM when available.
/// Safe to drop: `Esp32Alloc::dealloc` uses `heap_caps_free` for both regions.
fn psram_vec_with_capacity(cap: usize) -> Vec<u8> {
    if let Some(ptr) = crate::platform::heap::alloc_spiram_buffer(cap) {
        unsafe { Vec::from_raw_parts(ptr, 0, cap) }
    } else {
        Vec::with_capacity(cap)
    }
}

/// 读整个文件到 Vec。路径相对于 SPIFFS_BASE，或绝对如 /spiffs/config/SOUL.md。
/// 有 metadata 时预分配 capacity，减少 read_to_end 的多次 realloc。
/// 大文件（>= 8KB）优先使用 PSRAM 分配。
pub fn read_file(path: impl AsRef<Path>) -> Result<Vec<u8>> {
    with_fs_lock(|| {
        let p = path.as_ref();
        let path_str = p
            .to_str()
            .ok_or_else(|| Error::config("spiffs_read", "invalid path"))?;
        let mut f = std::fs::File::open(path_str).map_err(|e| Error::io("spiffs_read", e))?;
        let capacity = f
            .metadata()
            .ok()
            .and_then(|m| m.len().try_into().ok())
            .map(|len: usize| len.min(MAX_WRITE_SIZE))
            .unwrap_or(0);
        let mut buf = if capacity >= PSRAM_FILE_THRESHOLD {
            psram_vec_with_capacity(capacity)
        } else if capacity > 0 {
            Vec::with_capacity(capacity)
        } else {
            Vec::new()
        };
        f.read_to_end(&mut buf)
            .map_err(|e| Error::io("spiffs_read", e))?;
        if buf.len() > MAX_WRITE_SIZE {
            return Err(Error::config(
                "spiffs_read",
                format!("file size {} exceeds {}", buf.len(), MAX_WRITE_SIZE),
            ));
        }
        Ok(buf)
    })
}

/// 写字节到文件。超过 MAX_WRITE_SIZE 返回错误。
/// ESP：SPIFFS 不支持可靠 rename，直接覆盖。host：同目录 tmp + fsync + rename（原子替换）。
pub fn write_file(path: impl AsRef<Path>, data: &[u8]) -> Result<()> {
    if data.len() > MAX_WRITE_SIZE {
        return Err(Error::config(
            "spiffs_write",
            format!("write size {} exceeds limit {}", data.len(), MAX_WRITE_SIZE),
        ));
    }
    let p = path.as_ref();
    with_fs_lock(|| {
        #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
        {
            let path_str = p
                .to_str()
                .ok_or_else(|| Error::config("spiffs_write", "invalid path"))?;
            // ESP-IDF SPIFFS+VFS：部分环境下 `create` 短写不会缩短对象长度，文件尾残留旧字节，
            // 导致 JSON 解析报 trailing characters。先删再建与截断等价且更可靠。
            // ESP-IDF SPIFFS+VFS: shorter writes may not shrink the object; stale tail breaks JSON parse.
            let _ = std::fs::remove_file(path_str);
            let mut f =
                std::fs::File::create(path_str).map_err(|e| Error::io("spiffs_write", e))?;
            f.write_all(data)
                .map_err(|e| Error::io("spiffs_write", e))?;
            f.sync_all().map_err(|e| Error::io("spiffs_write", e))?;
            Ok(())
        }
        #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
        {
            crate::platform::fs_atomic::atomic_write(p, data)
        }
    })
}

/// 删除文件。仅删除文件，不删目录。用于技能删除等。
pub fn remove_file(path: impl AsRef<Path>) -> Result<()> {
    let p = path.as_ref().to_path_buf();
    with_fs_lock(|| {
        let path_str = p
            .to_str()
            .ok_or_else(|| Error::config("spiffs_remove", "invalid path"))?;
        std::fs::remove_file(path_str).map_err(|e| Error::io("spiffs_remove", e))?;
        Ok(())
    })
}

/// 列目录条目（仅一层）。路径如 /spiffs/config。
pub fn list_dir(path: impl AsRef<Path>) -> Result<Vec<String>> {
    let p = path.as_ref().to_path_buf();
    with_fs_lock(|| {
        let path_str = p
            .to_str()
            .ok_or_else(|| Error::config("spiffs_list", "invalid path"))?;
        let mut names = Vec::new();
        for e in std::fs::read_dir(path_str).map_err(|e| Error::io("spiffs_list", e))? {
            let e = e.map_err(|e| Error::io("spiffs_list", e))?;
            if let Some(s) = e.file_name().to_str() {
                names.push(s.to_string());
            }
        }
        Ok(names)
    })
}

// --- 子模块与对外类型 ---

pub(crate) mod cached_json;
pub mod calendar_credentials;
pub mod calendar_store;
pub mod execution_state;
pub mod important_message;
pub mod inner_life;
pub mod long_term_extraction_state;
pub mod long_term_memory;
pub mod memory;
pub mod pending_retry;
pub mod private_docs;
pub mod private_garden;
pub mod remind_at;
pub mod self_continuity;
pub mod self_model;
pub mod session;
pub mod session_summary;
pub mod skill_meta;
pub mod skill_storage;
pub mod task_store;
pub mod turn_ledger;
pub use turn_ledger::SpiffsTurnLedgerStore;

pub use calendar_credentials::SpiffsCalendarProviderCredentialStore;
pub use calendar_store::SpiffsCalendarStore;
pub use execution_state::SpiffsExecutionStateStore;
pub use important_message::SpiffsImportantMessageStore;
pub use inner_life::SpiffsInnerLifeStore;
pub use long_term_extraction_state::SpiffsLongTermMemoryExtractionStateStore;
pub use long_term_memory::SpiffsLongTermMemoryStore;
pub use memory::SpiffsMemoryStore;
pub use pending_retry::SpiffsPendingRetryStore;
pub use private_docs::SpiffsPrivateDocStore;
pub use private_garden::SpiffsPrivateGardenStore;
pub use remind_at::SpiffsRemindAtStore;
pub use self_continuity::SpiffsSelfContinuityStore;
pub use self_model::SpiffsSelfModelStore;
pub use session::SpiffsSessionStore;
pub use session_summary::SpiffsSessionSummaryStore;
pub use skill_meta::SpiffsSkillMetaStore;
pub use skill_storage::{default_skill_storage_arc, SpiffsSkillStorage};
pub use task_store::SpiffsTaskStore;
