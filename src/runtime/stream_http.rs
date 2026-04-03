//! Stream HTTP connection slot management for agent thread.
//! Agent 线程内的 Stream HTTP 连接槽位管理。
//!
//! 连接槽位统计（reuse/create/reset/invalidate）通过 `crate::metrics` 记录，
//! 可在 `/api/metrics` 等端点统一查询，不再只靠周期日志排障。

use crate::PlatformHttpClient;
use std::sync::atomic::{AtomicU32, Ordering};

const TAG: &str = "runtime::stream_http";
const STREAM_HTTP_STATS_LOG_EVERY: u32 = 50;

/// HTTP client factory function type.
pub type HttpFactory = dyn Fn() -> crate::error::Result<Box<dyn PlatformHttpClient>> + Send + Sync;

thread_local! {
    static STREAM_EDITOR_HTTP_SLOT: std::cell::RefCell<Option<Box<dyn PlatformHttpClient>>> =
        const { std::cell::RefCell::new(None) };
}

/// 总操作次数，仅用于周期日志触发（每 N 次打印一次摘要）；不在 metrics 快照中暴露。
static STREAM_HTTP_SLOT_OPS: AtomicU32 = AtomicU32::new(0);

fn maybe_log_stream_http_stats(trigger: &str) {
    let ops = STREAM_HTTP_SLOT_OPS.load(Ordering::Relaxed);
    if ops == 0 || !ops.is_multiple_of(STREAM_HTTP_STATS_LOG_EVERY) {
        return;
    }
    let snap = crate::metrics::snapshot();
    let hits = snap.stream_http_reuse_hits;
    let creates = snap.stream_http_creates;
    let resets = snap.stream_http_resets;
    let invalidates = snap.stream_http_invalidates;
    let total = hits + creates;
    let reuse_rate = if total == 0 {
        0u64
    } else {
        (hits * 100) / total
    };
    log::info!(
        "[{}] stream_http_stats trigger={} ops={} reuse_hits={} creates={} resets={} invalidates={} reuse_rate={}%",
        TAG,
        trigger,
        ops,
        hits,
        creates,
        resets,
        invalidates,
        reuse_rate
    );
}

pub fn invalidate_stream_http_slot(reason: &str) {
    STREAM_EDITOR_HTTP_SLOT.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.take().is_some() {
            crate::metrics::record_stream_http_invalidate();
            log::warn!("[{}] stream_http invalidate reason={}", TAG, reason);
        }
    });
}

fn reset_stream_http_slot(reason: &str) -> bool {
    let mut reset = false;
    STREAM_EDITOR_HTTP_SLOT.with(|slot| {
        let mut slot = slot.borrow_mut();
        if let Some(http) = slot.as_mut() {
            PlatformHttpClient::reset_connection_for_retry(http.as_mut());
            crate::metrics::record_stream_http_reset();
            reset = true;
        }
    });
    if reset {
        log::warn!("[{}] stream_http reset_for_retry reason={}", TAG, reason);
    }
    reset
}

fn with_stream_http_slot<T>(
    create_http: &HttpFactory,
    op_name: &str,
    op: &mut dyn FnMut(&mut Box<dyn PlatformHttpClient>) -> crate::error::Result<T>,
) -> crate::error::Result<T> {
    STREAM_EDITOR_HTTP_SLOT.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.is_none() {
            *slot = Some(create_http()?);
            crate::metrics::record_stream_http_create();
            log::info!("[{}] stream_http create op={}", TAG, op_name);
        } else {
            crate::metrics::record_stream_http_reuse();
        }
        STREAM_HTTP_SLOT_OPS.fetch_add(1, Ordering::Relaxed);
        let http = slot.as_mut().ok_or_else(|| {
            crate::error::Error::config("stream_http", "http client missing in slot")
        })?;
        op(http)
    })
}

pub fn execute_stream_http_op<T, F>(
    create_http: &HttpFactory,
    op_name: &str,
    mut op: F,
) -> crate::error::Result<T>
where
    F: FnMut(&mut Box<dyn PlatformHttpClient>) -> crate::error::Result<T>,
{
    let first = with_stream_http_slot(create_http, op_name, &mut op);
    match first {
        Ok(v) => {
            maybe_log_stream_http_stats(op_name);
            Ok(v)
        }
        Err(first_err) => {
            let reason = format!("{} first_try: {}", op_name, first_err);
            let _ = reset_stream_http_slot(&reason);
            let second = with_stream_http_slot(create_http, op_name, &mut op);
            match second {
                Ok(v) => {
                    maybe_log_stream_http_stats(op_name);
                    Ok(v)
                }
                Err(second_err) => {
                    invalidate_stream_http_slot(&format!("{} second_try: {}", op_name, second_err));
                    maybe_log_stream_http_stats(op_name);
                    Err(second_err)
                }
            }
        }
    }
}
