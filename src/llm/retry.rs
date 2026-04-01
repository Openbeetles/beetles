//! LLM 请求重试：指数退避，共 retries 次；全部失败返回最后一 Err。重试前调用 http.reset_connection_for_retry 避免 "connection is not in initial phase"。
//! Shared retry helper for LLM clients.

use super::LlmHttpClient;
use crate::error::Result;

/// ESP 上重试前最小等待（毫秒），给 WSS/TLS 释放 internal 堆的机会，缓解 esp-aes 分配失败。
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
const RETRY_MIN_DELAY_MS_ESP: u64 = 2000;

/// 执行 f 最多 retries 次（含首次）；失败则 sleep(base_ms * 2^attempt)、reset 连接后重试；全部失败返回最后一 Err。
/// retries 至少为 1。ESP 上重试前至少等待 RETRY_MIN_DELAY_MS_ESP 以缓解双 TLS 内存竞争。
pub(crate) fn with_retry<F, T>(
    retries: u32,
    base_ms: u64,
    tag: &str,
    http: &mut dyn LlmHttpClient,
    mut f: F,
) -> Result<T>
where
    F: FnMut(&mut dyn LlmHttpClient) -> Result<T>,
{
    let mut last_err = None;
    for attempt in 0..retries {
        match f(http) {
            Ok(t) => {
                crate::platform::task_wdt::feed_current_task();
                return Ok(t);
            }
            Err(e) => {
                crate::platform::task_wdt::feed_current_task();
                let retryable = e.is_retryable_upstream();
                last_err = Some(e);
                if !retryable {
                    log::warn!(
                        "[{}] attempt {} failed (non-retryable), skipping retry",
                        tag,
                        attempt + 1,
                    );
                    break;
                }
                if attempt + 1 < retries {
                    let delay_ms = {
                        let d = base_ms * (1 << attempt.min(6));
                        #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
                        let d = d.max(RETRY_MIN_DELAY_MS_ESP);
                        d
                    };
                    log::warn!(
                        "[{}] attempt {} failed, retry in {}ms",
                        tag,
                        attempt + 1,
                        delay_ms
                    );
                    std::thread::sleep(std::time::Duration::from_millis(delay_ms));
                    http.reset_connection_for_retry();
                }
            }
        }
    }
    Err(last_err.unwrap_or_else(|| {
        crate::error::Error::config("retry", "with_retry requires retries >= 1")
    }))
}
