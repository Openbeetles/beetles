//! Stream HTTP connection slot management for agent thread.
//! Agent 线程内的 Stream HTTP 连接槽位管理。

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

static STREAM_HTTP_SLOT_REUSE_HITS: AtomicU32 = AtomicU32::new(0);
static STREAM_HTTP_SLOT_CREATES: AtomicU32 = AtomicU32::new(0);
static STREAM_HTTP_SLOT_RESETS: AtomicU32 = AtomicU32::new(0);
static STREAM_HTTP_SLOT_INVALIDATES: AtomicU32 = AtomicU32::new(0);
static STREAM_HTTP_SLOT_OPS: AtomicU32 = AtomicU32::new(0);

fn maybe_log_stream_http_stats(trigger: &str) {
    let ops = STREAM_HTTP_SLOT_OPS.load(Ordering::Relaxed);
    if ops == 0 || !ops.is_multiple_of(STREAM_HTTP_STATS_LOG_EVERY) {
        return;
    }
    let hits = STREAM_HTTP_SLOT_REUSE_HITS.load(Ordering::Relaxed);
    let creates = STREAM_HTTP_SLOT_CREATES.load(Ordering::Relaxed);
    let resets = STREAM_HTTP_SLOT_RESETS.load(Ordering::Relaxed);
    let invalidates = STREAM_HTTP_SLOT_INVALIDATES.load(Ordering::Relaxed);
    let reuse_rate = if ops == 0 {
        0u32
    } else {
        ((hits as u64 * 100) / ops as u64) as u32
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
            STREAM_HTTP_SLOT_INVALIDATES.fetch_add(1, Ordering::Relaxed);
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
            STREAM_HTTP_SLOT_RESETS.fetch_add(1, Ordering::Relaxed);
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
            STREAM_HTTP_SLOT_CREATES.fetch_add(1, Ordering::Relaxed);
            log::info!("[{}] stream_http create op={}", TAG, op_name);
        } else {
            STREAM_HTTP_SLOT_REUSE_HITS.fetch_add(1, Ordering::Relaxed);
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
