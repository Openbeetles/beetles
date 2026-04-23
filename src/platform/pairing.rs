//! 配对码：store 仅存 pairing_code（6 位数字）；未设置时仅白名单可访问，已设置后 GET 类放行、写操作需带码校验。
//! Pairing: single key "pairing_code"; when not set only whitelist; when set, GETs pass, writes require code.

use crate::error::Result;
use crate::platform::ConfigStore;

const NVS_KEY_PAIRING_CODE: &str = "pairing_code";
const CODE_LEN: usize = 6;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SetCodeOutcome {
    Stored,
    AlreadySet,
    Invalid,
}

fn constant_time_eq(a: &str, b: &str) -> bool {
    let a = a.as_bytes();
    let b = b.as_bytes();
    if a.len() != b.len() {
        return false;
    }
    a.iter()
        .zip(b.iter())
        .fold(0u8, |acc, (x, y)| acc | (x ^ y))
        == 0
}

fn valid_code_format(code: &str) -> bool {
    code.len() == CODE_LEN && code.chars().all(|c| c.is_ascii_digit())
}

fn read_valid_code(store: &dyn ConfigStore) -> Option<String> {
    match store.read_string(NVS_KEY_PAIRING_CODE) {
        Ok(Some(code)) => {
            let code = code.trim();
            valid_code_format(code).then(|| code.to_string())
        }
        _ => None,
    }
}

/// 是否已设置配对码（store 中存在有效 6 位码）。
pub fn code_set(store: &dyn ConfigStore) -> bool {
    read_valid_code(store).is_some()
}

/// 仅当未设置时写入用户提供的 6 位码；校验格式，常量时间不暴露存储内容。返回 Ok(true) 表示已写入。
pub fn set_code(store: &dyn ConfigStore, code: &str) -> Result<bool> {
    Ok(matches!(
        set_code_checked(store, code)?,
        SetCodeOutcome::Stored
    ))
}

/// 仅当未设置时写入用户提供的 6 位码；返回明确状态，供 handler 避免额外 NVS 读取。
pub fn set_code_checked(store: &dyn ConfigStore, code: &str) -> Result<SetCodeOutcome> {
    let code = code.trim();
    if code_set(store) {
        return Ok(SetCodeOutcome::AlreadySet);
    }
    if !valid_code_format(code) {
        return Ok(SetCodeOutcome::Invalid);
    }
    store.write_string(NVS_KEY_PAIRING_CODE, code)?;
    log::info!("[pairing] code set (6 digits)");
    Ok(SetCodeOutcome::Stored)
}

/// 读取已设置且格式有效的配对码；未设置或内容非法时返回 None。
pub(crate) fn get_valid_code(store: &dyn ConfigStore) -> Option<String> {
    read_valid_code(store)
}

/// 校验用户输入与已读取的有效配对码是否一致（常量时间）。
pub(crate) fn verify_code_value(stored_code: &str, code: &str) -> bool {
    let stored = stored_code.trim();
    let provided = code.trim();
    if !valid_code_format(stored) || !valid_code_format(provided) {
        return false;
    }
    constant_time_eq(provided, stored)
}

/// 校验传入码与 store 中一致（常量时间）；未设置时返回 false。
pub fn verify_code(store: &dyn ConfigStore, code: &str) -> bool {
    get_valid_code(store)
        .as_deref()
        .is_some_and(|stored| verify_code_value(stored, code))
}

/// 清除配对码（恢复出厂后调用），使设备回到未激活状态。
pub fn clear_code(store: &dyn ConfigStore) -> Result<()> {
    store.erase_keys(&[NVS_KEY_PAIRING_CODE])
}

#[cfg(test)]
mod tests {
    use super::{clear_code, code_set, set_code_checked, verify_code, SetCodeOutcome};
    use crate::error::{Error, Result};
    use crate::platform::ConfigStore;
    use std::sync::Mutex;

    #[derive(Default)]
    struct MemoryConfigStore {
        code: Mutex<Option<String>>,
    }

    impl ConfigStore for MemoryConfigStore {
        fn read_string(&self, key: &str) -> Result<Option<String>> {
            assert_eq!(key, "pairing_code");
            Ok(self.code.lock().unwrap_or_else(|e| e.into_inner()).clone())
        }

        fn write_string(&self, key: &str, value: &str) -> Result<()> {
            assert_eq!(key, "pairing_code");
            *self.code.lock().unwrap_or_else(|e| e.into_inner()) = Some(value.to_string());
            Ok(())
        }

        fn erase_keys(&self, keys: &[&str]) -> Result<()> {
            if keys != ["pairing_code"] {
                return Err(Error::config("pairing_test", "unexpected erase keys"));
            }
            *self.code.lock().unwrap_or_else(|e| e.into_inner()) = None;
            Ok(())
        }
    }

    #[test]
    fn set_code_checked_reports_invalid_without_persisting() {
        let store = MemoryConfigStore::default();

        assert_eq!(
            set_code_checked(&store, "12ab56").expect("invalid input should not error"),
            SetCodeOutcome::Invalid
        );
        assert!(!code_set(&store));
    }

    #[test]
    fn set_code_checked_short_circuits_when_code_already_exists() {
        let store = MemoryConfigStore::default();
        assert_eq!(
            set_code_checked(&store, "123456").expect("initial set should succeed"),
            SetCodeOutcome::Stored
        );
        assert_eq!(
            set_code_checked(&store, "654321").expect("second set should not error"),
            SetCodeOutcome::AlreadySet
        );
        assert!(verify_code(&store, "123456"));
        assert!(!verify_code(&store, "654321"));
        clear_code(&store).expect("clear should succeed");
        assert!(!code_set(&store));
    }
}
