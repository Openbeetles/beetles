//! SPIFFS 挂载与路径约定。提供读/写/列目录最小 API，写操作有大小约束。
//! SPIFFS mount and path convention. Min API: read/write/list; write has size limit.
//! ESP-IDF VFS/SPIFFS 多线程并发会引发 fd 错用或死锁，故所有通过本模块的 SPIFFS 访问在 ESP 下由 SPIFFS_MUTEX 串行化。

use crate::error::{Error, Result};
use crate::platform::psram_vec::PsramVec;
use crate::platform::state_root::state_mount_path;
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
use std::ffi::CString;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

#[cfg_attr(
    not(any(target_arch = "xtensa", target_arch = "riscv32")),
    allow(dead_code)
)]
const ESP_SAFE_REL_PATH_LEN: usize = 31;

/// 与 `state_mount_path().join(rel)` 相同；供各 `Spiffs*` 存储拼接路径。
pub(crate) fn state_path_join(rel: impl AsRef<Path>) -> PathBuf {
    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
    {
        state_mount_path().join(esp_storage_rel_path(rel.as_ref()))
    }
    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
    {
        state_mount_path().join(rel.as_ref())
    }
}

#[cfg_attr(
    not(any(target_arch = "xtensa", target_arch = "riscv32")),
    allow(dead_code)
)]
fn fnv1a64_hash(s: &str) -> u64 {
    let mut h: u64 = 14695981039346656037;
    for b in s.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(1099511628211);
    }
    h
}

#[cfg_attr(
    not(any(target_arch = "xtensa", target_arch = "riscv32")),
    allow(dead_code)
)]
pub(crate) fn esp_storage_rel_path(rel: &Path) -> PathBuf {
    let rel_str = rel.to_string_lossy();
    match rel_str.as_ref() {
        crate::agent::REL_PATH_ACTIVE_WORKS => PathBuf::from("m/aw.json"),
        crate::agent::REL_PATH_DETACHED_WORKS => PathBuf::from("m/dw.json"),
        crate::memory::REL_PATH_EXECUTION_STATES => PathBuf::from("m/es.json"),
        crate::memory::REL_PATH_IMPORTANT_MESSAGE => PathBuf::from("m/im.json"),
        crate::memory::REL_PATH_SESSION_SUMMARIES => PathBuf::from("m/ss.json"),
        crate::memory::REL_PATH_LONG_TERM_MEMORIES => PathBuf::from("m/ltm.json"),
        crate::memory::REL_PATH_AUTONOMY_STRATEGIES => PathBuf::from("m/as.json"),
        crate::memory::REL_PATH_CONTINUITY_CAPSULES => PathBuf::from("m/cc.json"),
        "memory/continuity_capsule_index.sqlite3" => PathBuf::from("m/cci.db"),
        crate::memory::REL_PATH_SELF_AUTHORED_CORES => PathBuf::from("m/sac.json"),
        crate::memory::REL_PATH_CORE_REVISION_LEDGERS => PathBuf::from("m/crl.json"),
        "memory/relationship_constitutions.json" => PathBuf::from("m/rct.json"),
        crate::memory::REL_PATH_RELATIONSHIP_PORTFOLIOS => PathBuf::from("m/rpf.json"),
        "memory/relationship_topologies.json" => PathBuf::from("m/rtp.json"),
        crate::memory::REL_PATH_LONG_TERM_EXTRACTION_STATES => PathBuf::from("m/lte.json"),
        crate::memory::REL_PATH_MENTAL_PRIVACY_STATES => PathBuf::from("m/mps.json"),
        crate::memory::REL_PATH_PRIVATE_DOC_WORKSPACES => PathBuf::from("m/pdw.json"),
        crate::memory::REL_PATH_PRIVATE_GARDEN_INDEX => PathBuf::from("m/pgi.json"),
        "memory/tool_execution_governance.json" => PathBuf::from("m/teg.json"),
        _ => {
            if rel_str.starts_with(crate::memory::REL_PATH_PRIVATE_GARDEN_DIR) {
                PathBuf::from(format!("g/{:016x}.md", fnv1a64_hash(rel_str.as_ref())))
            } else if rel_str.len() > ESP_SAFE_REL_PATH_LEN {
                esp_hashed_rel_path(rel_str.as_ref())
            } else {
                rel.to_path_buf()
            }
        }
    }
}

#[cfg_attr(
    not(any(target_arch = "xtensa", target_arch = "riscv32")),
    allow(dead_code)
)]
fn esp_hashed_rel_path(rel: &str) -> PathBuf {
    let namespace = match rel.split('/').next().unwrap_or_default() {
        "memory" => "m",
        "runtime" => "r",
        "config" => "c",
        "skills" => "k",
        _ => "s",
    };
    let ext = esp_alias_extension(rel);
    PathBuf::from(format!("{namespace}/h{:016x}.{ext}", fnv1a64_hash(rel)))
}

#[cfg_attr(
    not(any(target_arch = "xtensa", target_arch = "riscv32")),
    allow(dead_code)
)]
fn esp_alias_extension(rel: &str) -> &'static str {
    match Path::new(rel).extension().and_then(|ext| ext.to_str()) {
        Some("md") => "md",
        Some("json") => "j",
        Some("jsonl") => "jl",
        Some("sqlite3") => "db",
        Some("txt") => "txt",
        _ => "bin",
    }
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
pub fn spiffs_usage() -> Option<(u64, u64)> {
    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
    {
        with_fs_lock_value(|| {
            let mut total: usize = 0;
            let mut used: usize = 0;
            let ret = unsafe {
                esp_idf_svc::sys::esp_spiffs_info(std::ptr::null(), &mut total, &mut used)
            };
            if ret == 0 {
                Some((total as u64, used as u64))
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
/// Returned buffer keeps PSRAM ownership explicit instead of handing raw pointers to `Vec`.
fn psram_vec_with_capacity(cap: usize) -> PsramVec<u8> {
    PsramVec::with_max_capacity(cap)
}

/// 读整个文件到 Vec。路径相对于 SPIFFS_BASE，或绝对如 /spiffs/config/SOUL.md。
/// 有 metadata 时预分配 capacity，减少 read_to_end 的多次 realloc。
/// 大文件（>= 8KB）优先使用 PSRAM 分配。
pub fn read_file(path: impl AsRef<Path>) -> Result<PsramVec<u8>> {
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
            PsramVec::from(Vec::with_capacity(capacity))
        } else {
            PsramVec::from(Vec::new())
        };
        let mut chunk = [0u8; 1024];
        loop {
            let n = f
                .read(&mut chunk)
                .map_err(|e| Error::io("spiffs_read", e))?;
            if n == 0 {
                break;
            }
            buf.write_all(&chunk[..n])
                .map_err(|e| Error::io("spiffs_read", e))?;
        }
        if buf.len() > MAX_WRITE_SIZE {
            return Err(Error::config(
                "spiffs_read",
                format!("file size {} exceeds {}", buf.len(), MAX_WRITE_SIZE),
            ));
        }
        Ok(buf)
    })
}

/// 读整个文件到普通 `Vec<u8>`。用于最终 API 本身就要求 `Vec<u8>` 的路径，
/// 避免先落 PSRAM 再 `into_vec()` 复制一遍。
pub fn read_file_to_vec(path: impl AsRef<Path>) -> Result<Vec<u8>> {
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
        let mut buf = if capacity > 0 {
            Vec::with_capacity(capacity)
        } else {
            Vec::new()
        };
        let mut chunk = [0u8; 1024];
        loop {
            let n = f
                .read(&mut chunk)
                .map_err(|e| Error::io("spiffs_read", e))?;
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&chunk[..n]);
        }
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

pub mod active_work;
pub mod autonomy_strategy;
pub(crate) mod cached_json;
pub mod calendar_store;
pub mod continuity_capsule;
pub mod core_revision_ledger;
pub mod detached_work;
pub mod execution_state;
pub mod important_message;
pub mod inner_life;
pub mod long_term_extraction_state;
pub mod long_term_memory;
pub mod memory;
pub mod mental_privacy;
#[cfg(feature = "capability_office")]
pub mod office_credentials;
#[cfg(feature = "capability_office")]
pub mod office_runtime_status;
pub mod outer_voice;
pub mod pending_retry;
pub mod private_docs;
pub mod private_garden;
pub mod relationship_constitution;
pub mod relationship_portfolio;
pub mod relationship_topology;
pub mod remind_at;
pub mod self_authored_core;
pub mod self_continuity;
pub mod self_model;
pub mod session;
pub mod session_summary;
pub mod skill_meta;
pub mod skill_storage;
pub mod task_execution;
pub mod task_store;
pub mod turn_ledger;
pub mod world_sense;
pub use turn_ledger::SpiffsTurnLedgerStore;

pub use active_work::SpiffsActiveWorkStore;
pub use autonomy_strategy::SpiffsAutonomyStrategyStore;
pub use calendar_store::SpiffsCalendarStore;
pub use continuity_capsule::SpiffsContinuityCapsuleStore;
pub use core_revision_ledger::SpiffsCoreRevisionLedgerStore;
pub use detached_work::SpiffsDetachedWorkStore;
pub use execution_state::SpiffsExecutionStateStore;
pub use important_message::SpiffsImportantMessageStore;
pub use inner_life::SpiffsInnerLifeStore;
pub use long_term_extraction_state::SpiffsLongTermMemoryExtractionStateStore;
pub use long_term_memory::SpiffsLongTermMemoryStore;
pub use memory::SpiffsMemoryStore;
pub use mental_privacy::SpiffsMentalPrivacyStore;
#[cfg(feature = "capability_office")]
pub use office_credentials::SpiffsOfficeCredentialStore;
#[cfg(feature = "capability_office")]
pub use office_runtime_status::SpiffsOfficeRuntimeStatusStore;
pub use outer_voice::SpiffsOuterVoiceStore;
pub use pending_retry::SpiffsPendingRetryStore;
pub use private_docs::SpiffsPrivateDocStore;
pub use private_garden::SpiffsPrivateGardenStore;
pub use relationship_constitution::SpiffsRelationshipConstitutionStore;
pub use relationship_portfolio::SpiffsRelationshipPortfolioStore;
pub use relationship_topology::SpiffsRelationshipTopologyStore;
pub use remind_at::SpiffsRemindAtStore;
pub use self_authored_core::SpiffsSelfAuthoredCoreStore;
pub use self_continuity::SpiffsSelfContinuityStore;
pub use self_model::SpiffsSelfModelStore;
pub use session::SpiffsSessionStore;
pub use session_summary::SpiffsSessionSummaryStore;
pub use skill_meta::{CachedSkillMetaStore, SpiffsSkillMetaStore};
pub use skill_storage::{default_skill_storage_arc, CachedSkillStorage, SpiffsSkillStorage};
pub use task_execution::{
    SpiffsTaskArtifactStore, SpiffsTaskExecutionLedgerStore, SpiffsTaskLearningStore,
    SpiffsTaskRunStore,
};
pub use task_store::SpiffsTaskStore;
pub use world_sense::SpiffsWorldSenseStore;

#[cfg(test)]
mod tests {
    use super::esp_storage_rel_path;
    use crate::agent::REL_PATH_ACTIVE_WORKS;
    use crate::memory::{
        REL_PATH_AUTONOMY_STRATEGIES, REL_PATH_CONTINUITY_CAPSULES, REL_PATH_CORE_REVISION_LEDGERS,
        REL_PATH_IMPORTANT_MESSAGE, REL_PATH_LONG_TERM_EXTRACTION_STATES,
        REL_PATH_PRIVATE_DOC_WORKSPACES, REL_PATH_PRIVATE_GARDEN_DIR,
        REL_PATH_PRIVATE_GARDEN_INDEX, REL_PATH_RELATIONSHIP_PORTFOLIOS,
        REL_PATH_SELF_AUTHORED_CORES, REL_PATH_SESSION_SUMMARIES,
    };
    use crate::runtime::REL_PATH_LINUX_RELEASE_STATE;
    use std::path::{Path, PathBuf};

    const MAX_SAFE_ESP_REL_PATH_LEN: usize = 31;

    #[test]
    fn esp_storage_rel_path_keeps_short_paths_stable() {
        assert_eq!(
            esp_storage_rel_path(Path::new("memory/world_sense.json")),
            PathBuf::from("memory/world_sense.json")
        );
        assert_eq!(
            esp_storage_rel_path(Path::new("config/llm.json")),
            PathBuf::from("config/llm.json")
        );
    }

    #[test]
    fn esp_storage_rel_path_aliases_known_long_internal_paths() {
        let cases = [
            (REL_PATH_ACTIVE_WORKS, "m/aw.json"),
            (REL_PATH_AUTONOMY_STRATEGIES, "m/as.json"),
            (REL_PATH_CONTINUITY_CAPSULES, "m/cc.json"),
            (REL_PATH_CORE_REVISION_LEDGERS, "m/crl.json"),
            (REL_PATH_IMPORTANT_MESSAGE, "m/im.json"),
            (REL_PATH_LONG_TERM_EXTRACTION_STATES, "m/lte.json"),
            (REL_PATH_PRIVATE_DOC_WORKSPACES, "m/pdw.json"),
            (REL_PATH_PRIVATE_GARDEN_INDEX, "m/pgi.json"),
            ("memory/relationship_constitutions.json", "m/rct.json"),
            (REL_PATH_RELATIONSHIP_PORTFOLIOS, "m/rpf.json"),
            ("memory/relationship_topologies.json", "m/rtp.json"),
            (REL_PATH_SELF_AUTHORED_CORES, "m/sac.json"),
            (REL_PATH_SESSION_SUMMARIES, "m/ss.json"),
            ("memory/tool_execution_governance.json", "m/teg.json"),
        ];
        for (rel, expected) in cases {
            assert_eq!(
                esp_storage_rel_path(Path::new(rel)),
                PathBuf::from(expected)
            );
        }
    }

    #[test]
    fn esp_storage_rel_path_hashes_long_runtime_paths_into_safe_namespace() {
        let bundle = esp_storage_rel_path(Path::new(
            "memory/continuity_snapshots/runtime/latest_reboot_bundle.json",
        ));
        let markdown = esp_storage_rel_path(Path::new(
            "memory/continuity_snapshots/runtime/latest_reboot_bundle.md",
        ));
        let linux_release = esp_storage_rel_path(Path::new(REL_PATH_LINUX_RELEASE_STATE));

        for mapped in [&bundle, &markdown] {
            let rendered = mapped.to_string_lossy();
            assert!(rendered.len() <= MAX_SAFE_ESP_REL_PATH_LEN);
            assert!(rendered.starts_with("m/"));
        }
        let linux_release_rendered = linux_release.to_string_lossy();
        assert!(linux_release_rendered.len() <= MAX_SAFE_ESP_REL_PATH_LEN);
        assert!(linux_release_rendered.starts_with("r/"));
        assert_ne!(
            bundle,
            PathBuf::from("memory/continuity_snapshots/runtime/latest_reboot_bundle.json")
        );
        assert_eq!(
            esp_storage_rel_path(Path::new(
                "memory/continuity_snapshots/runtime/latest_reboot_bundle.json",
            )),
            bundle
        );
    }

    #[test]
    fn esp_storage_rel_path_preserves_private_garden_hashing() {
        let rel = format!("{REL_PATH_PRIVATE_GARDEN_DIR}/board.self/entry.md");
        let mapped = esp_storage_rel_path(Path::new(&rel));
        let rendered = mapped.to_string_lossy();
        assert!(rendered.starts_with("g/"));
        assert!(rendered.ends_with(".md"));
        assert!(rendered.len() <= MAX_SAFE_ESP_REL_PATH_LEN);
    }
}
