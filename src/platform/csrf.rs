//! CSRF token 生成与验证,防止跨站请求伪造攻击。

use std::sync::OnceLock;

static CSRF_TOKEN: OnceLock<String> = OnceLock::new();

/// 生成新的 CSRF token (16 字节随机数的 hex 字符串)。
pub fn generate_token() -> crate::Result<String> {
    let mut bytes = [0u8; 16];
    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
    unsafe {
        esp_idf_svc::sys::esp_fill_random(bytes.as_mut_ptr() as *mut _, 16);
    }
    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
    {
        getrandom::getrandom(&mut bytes)
            .map_err(|e| crate::Error::config("csrf_entropy", e.to_string()))?;
    }
    Ok(hex_encode(&bytes))
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        s.push(HEX[(b >> 4) as usize] as char);
        s.push(HEX[(b & 0x0f) as usize] as char);
    }
    s
}

/// 初始化 CSRF token (启动时调用一次)。
pub fn init() -> crate::Result<()> {
    if CSRF_TOKEN.get().is_some() {
        return Ok(());
    }
    let token = generate_token()?;
    if CSRF_TOKEN.set(token).is_ok() {
        log::info!("[csrf] token initialized");
    }
    Ok(())
}

/// 获取当前 CSRF token。
pub fn get_token() -> Option<String> {
    CSRF_TOKEN.get().cloned()
}

/// 验证请求的 CSRF token 是否匹配。
pub fn verify_token(token: &str) -> bool {
    CSRF_TOKEN
        .get()
        .is_some_and(|expected| crate::util::constant_time_eq(token, expected))
}

#[cfg(test)]
mod tests {
    #[test]
    fn init_is_write_once_after_token_exists() {
        super::init().expect("first init");
        let first = super::get_token().expect("first token");

        super::init().expect("second init");
        let second = super::get_token().expect("second token");

        assert_eq!(second, first);
        assert!(super::verify_token(&first));
    }
}
