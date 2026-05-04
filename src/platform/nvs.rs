//! NVS 初始化与读写：失败时 erase 再 init；按命名空间 pc_cfg 读写字符串。
//! NVS init and read/write: erase then init on failure; read/write strings in namespace pc_cfg.
//! 所有对 NVS 的读写均经本模块，ESP 下用 NVS_MUTEX 串行化；open/commit 返回 4361 时单次 recover+重试。
//! 配置策略：NVS 仅存系统小键（wifi_ssid、wifi_pass、proxy_url、locale）；
//! LLM 与通道存 storage（config/llm.json、config/channels.json），技能元数据存 config/skills_meta.json，以减少 NVS 写放大与 4361。

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
use std::ffi::CString;
use std::sync::{Mutex, OnceLock};

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
use std::collections::HashMap;

use crate::error::{Error, Result};

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
const ESP_ERR_NVS_NO_FREE_PAGES: i32 = 0x1102;
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
const ESP_ERR_NVS_NEW_VERSION_FOUND: i32 = 0x1103;
/// NVS 处于不一致状态（写入中断或分区异常），需 erase 后重新 init。
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
const ESP_ERR_NVS_INVALID_STATE: i32 = 0x1109;

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
/// 串行化 NVS 访问的全局锁（HTTP 线程写配置与主线程读配置等会并发，ESP-IDF NVS 不支持并发）。
static NVS_MUTEX: OnceLock<Mutex<()>> = OnceLock::new();

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
fn lock_nvs() -> std::sync::MutexGuard<'static, ()> {
    NVS_MUTEX
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

/// 配置用 NVS 命名空间。与 config 层键名配合使用。
pub const NVS_NAMESPACE: &str = "pc_cfg";
/// 单条 value 最大长度（字节）。
const NVS_VALUE_MAX_LEN: usize = 512;
/// key 最大长度（字节）；与 ESP-IDF NVS_KEY_NAME_MAX_SIZE（15 字符）一致。
const NVS_KEY_MAX_LEN: usize = 15;

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
static NVS_HOST_MUTEX: OnceLock<Mutex<()>> = OnceLock::new();

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
fn lock_host_nvs() -> std::sync::MutexGuard<'static, ()> {
    NVS_HOST_MUTEX
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

/// Host：`pc_cfg` 键值与 ESP NVS 同语义，单文件 JSON 原子替换。
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
const PC_CFG_JSON_REL: &str = "nvs/pc_cfg.json";

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
fn pc_cfg_path() -> std::path::PathBuf {
    crate::platform::state_root::state_mount_path().join(PC_CFG_JSON_REL)
}

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
fn load_pc_cfg_map() -> Result<HashMap<String, String>> {
    let path = pc_cfg_path();
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(HashMap::new()),
        Err(e) => return Err(Error::io("nvs_pc_cfg", e)),
    };
    if bytes.is_empty() {
        return Ok(HashMap::new());
    }
    serde_json::from_slice(&bytes).map_err(|e| Error::config("nvs_pc_cfg", e.to_string()))
}

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
fn save_pc_cfg_map(map: &HashMap<String, String>) -> Result<()> {
    let v =
        serde_json::to_vec_pretty(map).map_err(|e| Error::config("nvs_pc_cfg", e.to_string()))?;
    if v.len() > crate::platform::storage::MAX_WRITE_SIZE {
        return Err(Error::config(
            "nvs_pc_cfg",
            format!(
                "serialized size {} exceeds {}",
                v.len(),
                crate::platform::storage::MAX_WRITE_SIZE
            ),
        ));
    }
    crate::platform::fs_atomic::atomic_write(&pc_cfg_path(), &v)
}

/// 初始化 NVS 分区。若返回 NO_FREE_PAGES 或 NEW_VERSION_FOUND 则先 erase 再 init。
pub fn init_nvs() -> Result<()> {
    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
    {
        let mut err = unsafe { esp_idf_svc::sys::nvs_flash_init() };
        if err == ESP_ERR_NVS_NO_FREE_PAGES
            || err == ESP_ERR_NVS_NEW_VERSION_FOUND
            || err == ESP_ERR_NVS_INVALID_STATE
        {
            log::warn!(
                "[platform::nvs] NVS partition needs erase (err={}), erasing...",
                err
            );
            unsafe { esp_idf_svc::sys::nvs_flash_erase() };
            err = unsafe { esp_idf_svc::sys::nvs_flash_init() };
        }
        if err != 0 {
            return Err(Error::esp("nvs_init", err));
        }
    }
    Ok(())
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
fn recover_nvs_invalid_state() -> Result<()> {
    log::warn!("[platform::nvs] attempting to recover from ESP_ERR_NVS_INVALID_STATE");
    init_nvs()
}

/// 从 NVS 命名空间 pc_cfg 读取字符串。key 不存在返回 Ok(None)。open 返回 4361 时 recover 并重试一次。
pub fn read_string(key: &str) -> Result<Option<String>> {
    if key.len() > NVS_KEY_MAX_LEN {
        return Err(Error::config("nvs", "key too long"));
    }
    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
    {
        let _guard = lock_nvs();
        let r = do_read_string(key);
        if r.is_err() {
            if let Err(Error::Esp { code, .. }) = r {
                if code == ESP_ERR_NVS_INVALID_STATE {
                    recover_nvs_invalid_state()?;
                    return do_read_string(key);
                }
            }
            return r;
        }
        r
    }
    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
    {
        let _guard = lock_host_nvs();
        let map = load_pc_cfg_map()?;
        Ok(map.get(key).cloned())
    }
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
fn do_read_string(key: &str) -> Result<Option<String>> {
    use std::os::raw::c_char;
    let c_ns = CString::new(NVS_NAMESPACE).map_err(|_| Error::nvs_stage("nvs_open"))?;
    let c_key = CString::new(key).map_err(|_| Error::nvs_stage("nvs_get"))?;
    let mut handle: esp_idf_svc::sys::nvs_handle_t = 0;
    let err = unsafe {
        esp_idf_svc::sys::nvs_open(
            c_ns.as_ptr(),
            esp_idf_svc::sys::nvs_open_mode_t_NVS_READONLY,
            &mut handle as *mut _,
        )
    };
    if err != 0 {
        return Err(Error::esp("nvs_open", err));
    }
    let mut len: usize = 0;
    let err = unsafe {
        esp_idf_svc::sys::nvs_get_str(
            handle,
            c_key.as_ptr(),
            std::ptr::null_mut(),
            &mut len as *mut _,
        )
    };
    if err == esp_idf_svc::sys::ESP_ERR_NVS_NOT_FOUND {
        unsafe { esp_idf_svc::sys::nvs_close(handle) };
        return Ok(None);
    }
    if err != 0 {
        unsafe { esp_idf_svc::sys::nvs_close(handle) };
        return Err(Error::esp("nvs_get", err));
    }
    let mut buf = vec![0u8; len];
    let err = unsafe {
        esp_idf_svc::sys::nvs_get_str(
            handle,
            c_key.as_ptr(),
            buf.as_mut_ptr() as *mut c_char,
            &mut len as *mut _,
        )
    };
    unsafe { esp_idf_svc::sys::nvs_close(handle) };
    if err != 0 {
        return Err(Error::esp("nvs_get", err));
    }
    let s = String::from_utf8_lossy(&buf[..len.saturating_sub(1)]).into_owned();
    Ok(Some(s))
}

/// 批量读取：一次 open，多次 nvs_get_str，一次 close。open 返回 4361 时 recover 并重试一次。
pub fn read_strings_batch(keys: &[&str]) -> Result<Vec<Option<String>>> {
    for k in keys.iter() {
        if k.len() > NVS_KEY_MAX_LEN {
            return Err(Error::config("nvs", "key too long"));
        }
    }
    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
    {
        let _guard = lock_nvs();
        let r = do_read_strings_batch(keys);
        if r.is_err() {
            if let Err(Error::Esp { code, .. }) = r {
                if code == ESP_ERR_NVS_INVALID_STATE {
                    recover_nvs_invalid_state()?;
                    return do_read_strings_batch(keys);
                }
            }
            return r;
        }
        r
    }
    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
    {
        let _guard = lock_host_nvs();
        let map = load_pc_cfg_map()?;
        Ok(keys.iter().map(|k| map.get(*k).cloned()).collect())
    }
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
fn do_read_strings_batch(keys: &[&str]) -> Result<Vec<Option<String>>> {
    use std::os::raw::c_char;
    let c_ns = CString::new(NVS_NAMESPACE).map_err(|_| Error::nvs_stage("nvs_open"))?;
    let mut handle: esp_idf_svc::sys::nvs_handle_t = 0;
    let err = unsafe {
        esp_idf_svc::sys::nvs_open(
            c_ns.as_ptr(),
            esp_idf_svc::sys::nvs_open_mode_t_NVS_READONLY,
            &mut handle as *mut _,
        )
    };
    if err != 0 {
        return Err(Error::esp("nvs_open", err));
    }
    let mut out = Vec::with_capacity(keys.len());
    for key in keys.iter() {
        let c_key = CString::new(*key).map_err(|_| {
            unsafe { esp_idf_svc::sys::nvs_close(handle) };
            Error::nvs_stage("nvs_get")
        })?;
        let mut len: usize = 0;
        let err = unsafe {
            esp_idf_svc::sys::nvs_get_str(
                handle,
                c_key.as_ptr(),
                std::ptr::null_mut(),
                &mut len as *mut _,
            )
        };
        if err == esp_idf_svc::sys::ESP_ERR_NVS_NOT_FOUND {
            out.push(None);
            continue;
        }
        if err != 0 {
            unsafe { esp_idf_svc::sys::nvs_close(handle) };
            return Err(Error::esp("nvs_get", err));
        }
        let mut buf = vec![0u8; len];
        let err = unsafe {
            esp_idf_svc::sys::nvs_get_str(
                handle,
                c_key.as_ptr(),
                buf.as_mut_ptr() as *mut c_char,
                &mut len as *mut _,
            )
        };
        if err != 0 {
            unsafe { esp_idf_svc::sys::nvs_close(handle) };
            return Err(Error::esp("nvs_get", err));
        }
        let s = String::from_utf8_lossy(&buf[..len.saturating_sub(1)]).into_owned();
        out.push(Some(s));
    }
    unsafe { esp_idf_svc::sys::nvs_close(handle) };
    Ok(out)
}

/// 批量写入：一次 open，多次 nvs_set_str，一次 commit，一次 close。commit 返回 4361 时 recover 并重试一次。
pub fn write_strings(pairs: &[(&str, &str)]) -> Result<()> {
    for (k, v) in pairs.iter() {
        if k.len() > NVS_KEY_MAX_LEN {
            return Err(Error::config("nvs", "key too long"));
        }
        if v.len() >= NVS_VALUE_MAX_LEN {
            return Err(Error::config("nvs", "value too long"));
        }
    }
    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
    {
        let _guard = lock_nvs();
        let r = do_write_strings(pairs);
        if r.is_err() {
            if let Err(Error::Esp { code, .. }) = r {
                if code == ESP_ERR_NVS_INVALID_STATE {
                    recover_nvs_invalid_state()?;
                    return do_write_strings(pairs);
                }
            }
            return r;
        }
        r
    }
    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
    {
        let _guard = lock_host_nvs();
        let mut map = load_pc_cfg_map()?;
        for (k, v) in pairs.iter() {
            if k.len() > NVS_KEY_MAX_LEN {
                return Err(Error::config("nvs", "key too long"));
            }
            if v.len() >= NVS_VALUE_MAX_LEN {
                return Err(Error::config("nvs", "value too long"));
            }
            map.insert((*k).to_string(), (*v).to_string());
        }
        save_pc_cfg_map(&map)
    }
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
fn do_write_strings(pairs: &[(&str, &str)]) -> Result<()> {
    let c_ns = CString::new(NVS_NAMESPACE).map_err(|_| Error::nvs_stage("nvs_open"))?;
    let mut handle: esp_idf_svc::sys::nvs_handle_t = 0;
    let err = unsafe {
        esp_idf_svc::sys::nvs_open(
            c_ns.as_ptr(),
            esp_idf_svc::sys::nvs_open_mode_t_NVS_READWRITE,
            &mut handle as *mut _,
        )
    };
    if err != 0 {
        return Err(Error::esp("nvs_open", err));
    }
    log::debug!("[platform::nvs] write_strings begin, pairs={}", pairs.len());
    for (key, value) in pairs.iter() {
        log::debug!(
            "[platform::nvs] write_strings key={} len={}",
            key,
            value.len()
        );
        let c_key = CString::new(*key).map_err(|_| {
            unsafe { esp_idf_svc::sys::nvs_close(handle) };
            Error::nvs_stage("nvs_set")
        })?;
        let c_val = CString::new(*value).map_err(|_| {
            unsafe { esp_idf_svc::sys::nvs_close(handle) };
            Error::nvs_stage("nvs_set")
        })?;
        let err = unsafe { esp_idf_svc::sys::nvs_set_str(handle, c_key.as_ptr(), c_val.as_ptr()) };
        if err != 0 {
            log::error!("[platform::nvs] nvs_set failed key={} err={}", key, err);
            unsafe { esp_idf_svc::sys::nvs_close(handle) };
            return Err(Error::esp("nvs_set", err));
        }
    }
    let err = unsafe { esp_idf_svc::sys::nvs_commit(handle) };
    unsafe { esp_idf_svc::sys::nvs_close(handle) };
    if err != 0 {
        return Err(Error::esp("nvs_commit", err));
    }
    Ok(())
}

/// 向 NVS 命名空间 pc_cfg 写入字符串。value 超长返回错误。commit 返回 4361 时 recover 并重试一次。
pub fn write_string(key: &str, value: &str) -> Result<()> {
    if key.len() > NVS_KEY_MAX_LEN {
        return Err(Error::config("nvs", "key too long"));
    }
    if value.len() >= NVS_VALUE_MAX_LEN {
        return Err(Error::config("nvs", "value too long"));
    }
    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
    {
        let _guard = lock_nvs();
        let r = do_write_string(key, value);
        if r.is_err() {
            if let Err(Error::Esp { code, .. }) = r {
                if code == ESP_ERR_NVS_INVALID_STATE {
                    recover_nvs_invalid_state()?;
                    return do_write_string(key, value);
                }
            }
            return r;
        }
        r
    }
    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
    {
        let _guard = lock_host_nvs();
        let mut map = load_pc_cfg_map()?;
        map.insert(key.to_string(), value.to_string());
        save_pc_cfg_map(&map)
    }
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
fn do_write_string(key: &str, value: &str) -> Result<()> {
    let c_ns = CString::new(NVS_NAMESPACE).map_err(|_| Error::nvs_stage("nvs_open"))?;
    let c_key = CString::new(key).map_err(|_| Error::nvs_stage("nvs_set"))?;
    let c_val = CString::new(value).map_err(|_| Error::nvs_stage("nvs_set"))?;
    let mut handle: esp_idf_svc::sys::nvs_handle_t = 0;
    let err = unsafe {
        esp_idf_svc::sys::nvs_open(
            c_ns.as_ptr(),
            esp_idf_svc::sys::nvs_open_mode_t_NVS_READWRITE,
            &mut handle as *mut _,
        )
    };
    if err != 0 {
        return Err(Error::esp("nvs_open", err));
    }
    log::debug!(
        "[platform::nvs] write_string key={} len={}",
        key,
        value.len()
    );
    let err = unsafe { esp_idf_svc::sys::nvs_set_str(handle, c_key.as_ptr(), c_val.as_ptr()) };
    if err != 0 {
        log::error!("[platform::nvs] nvs_set failed key={} err={}", key, err);
        unsafe { esp_idf_svc::sys::nvs_close(handle) };
        return Err(Error::esp("nvs_set", err));
    }
    let err = unsafe { esp_idf_svc::sys::nvs_commit(handle) };
    unsafe { esp_idf_svc::sys::nvs_close(handle) };
    if err != 0 {
        return Err(Error::esp("nvs_commit", err));
    }
    Ok(())
}

/// 擦除命名空间内指定键；ESP-IDF 无整命名空间擦除，故按 key 列表逐条 nvs_erase_key。Host 返回 Err。
pub fn erase_namespace(namespace: &str, keys: &[&str]) -> Result<()> {
    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
    {
        let _guard = lock_nvs();
        let c_ns = CString::new(namespace).map_err(|_| Error::nvs_stage("nvs_open"))?;
        let mut handle: esp_idf_svc::sys::nvs_handle_t = 0;
        let err = unsafe {
            esp_idf_svc::sys::nvs_open(
                c_ns.as_ptr(),
                esp_idf_svc::sys::nvs_open_mode_t_NVS_READWRITE,
                &mut handle as *mut _,
            )
        };
        if err != 0 {
            return Err(Error::esp("nvs_open", err));
        }
        for key in keys {
            if key.len() <= NVS_KEY_MAX_LEN {
                let c_key = CString::new(*key).map_err(|_| Error::nvs_stage("nvs_erase"))?;
                unsafe { esp_idf_svc::sys::nvs_erase_key(handle, c_key.as_ptr()) };
            }
        }
        let err = unsafe { esp_idf_svc::sys::nvs_commit(handle) };
        unsafe { esp_idf_svc::sys::nvs_close(handle) };
        if err != 0 {
            return Err(Error::esp("nvs_commit", err));
        }
        Ok(())
    }
    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
    {
        if namespace != NVS_NAMESPACE {
            return Err(Error::config(
                "nvs",
                "host file backend only supports pc_cfg namespace",
            ));
        }
        let _guard = lock_host_nvs();
        let mut map = load_pc_cfg_map()?;
        for k in keys.iter() {
            map.remove(*k);
        }
        save_pc_cfg_map(&map)
    }
}

/// NVS 的 ConfigStore 实现；供 config/pairing/skills 通过抽象使用。命名空间固定为 pc_cfg。
pub struct NvsConfigStore;

impl crate::platform::ConfigStore for NvsConfigStore {
    fn read_string(&self, key: &str) -> Result<Option<String>> {
        read_string(key)
    }
    fn read_strings(&self, keys: &[&str]) -> Result<Vec<Option<String>>> {
        read_strings_batch(keys)
    }
    fn write_string(&self, key: &str, value: &str) -> Result<()> {
        write_string(key, value)
    }
    fn write_strings(&self, pairs: &[(&str, &str)]) -> Result<()> {
        write_strings(pairs)
    }
    fn erase_keys(&self, keys: &[&str]) -> Result<()> {
        erase_namespace(NVS_NAMESPACE, keys)
    }
}

/// 默认配置存储（当前平台 NVS）。步骤 2 过渡用；步骤 5 后 main 改用 platform.config_store()。
pub fn default_config_store() -> NvsConfigStore {
    NvsConfigStore
}

/// 默认配置存储的 Arc，供跨线程使用（如 http_server spawn）。
pub fn default_config_store_arc() -> std::sync::Arc<dyn crate::platform::ConfigStore + Send + Sync>
{
    std::sync::Arc::new(NvsConfigStore)
}
