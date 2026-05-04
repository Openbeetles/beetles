//! Doctor 式自检：纯逻辑，产出 DiagResult 列表，供 GET /api/diagnose 使用。
//! No platform/state dependency; host-testable.

use serde::Serialize;

/// 单条自检结果：severity + category + message。
#[derive(Debug, Clone, Serialize)]
pub struct DiagResult {
    pub severity: String,
    pub category: String,
    pub message: String,
}

const DEPTH_WARN_THRESHOLD: usize = 6;
const LAST_ERROR_MAX_LEN: usize = 200;

/// 根据当前状态生成自检列表。不依赖 platform/state，便于单测与 host 编译。
///
/// `wifi_sta_connected`：STA 已关联上游 AP 且有可用出站路径（与 SoftAP/配网页是否可达无关）。
#[allow(clippy::too_many_arguments)]
pub fn diagnose(
    wifi_sta_connected: bool,
    inbound_depth: usize,
    outbound_depth: usize,
    last_error: Option<String>,
    storage_ok: bool,
    storage_usage: Option<(u64, u64)>,
    nvs_ok: bool,
    memory_loaded: bool,
    skills_count: usize,
    last_errors_count: usize,
) -> Vec<DiagResult> {
    let mut out = Vec::with_capacity(12);

    if storage_ok {
        out.push(DiagResult {
            severity: "ok".into(),
            category: "storage".into(),
            message: "storage readable".into(),
        });
    } else {
        out.push(DiagResult {
            severity: "warn".into(),
            category: "storage".into(),
            message: "storage not readable".into(),
        });
    }

    if let Some((total, used)) = storage_usage {
        let free = total.saturating_sub(used);
        out.push(DiagResult {
            severity: "ok".into(),
            category: "storage".into(),
            message: format!("storage total={} used={} free={}", total, used, free),
        });
    } else {
        out.push(DiagResult {
            severity: "warn".into(),
            category: "storage".into(),
            message: "storage usage unavailable".into(),
        });
    }

    if nvs_ok {
        out.push(DiagResult {
            severity: "ok".into(),
            category: "config".into(),
            message: "nvs accessible".into(),
        });
    } else {
        out.push(DiagResult {
            severity: "warn".into(),
            category: "config".into(),
            message: "nvs not accessible".into(),
        });
    }

    if wifi_sta_connected {
        out.push(DiagResult {
            severity: "ok".into(),
            category: "config".into(),
            message: "wifi sta connected".into(),
        });
    } else {
        out.push(DiagResult {
            severity: "warn".into(),
            category: "config".into(),
            message: "wifi sta disconnected".into(),
        });
    }

    if inbound_depth <= DEPTH_WARN_THRESHOLD && outbound_depth <= DEPTH_WARN_THRESHOLD {
        out.push(DiagResult {
            severity: "ok".into(),
            category: "channel".into(),
            message: format!(
                "inbound_depth={} outbound_depth={}",
                inbound_depth, outbound_depth
            ),
        });
    } else {
        out.push(DiagResult {
            severity: "warn".into(),
            category: "channel".into(),
            message: format!(
                "inbound_depth={} outbound_depth={}",
                inbound_depth, outbound_depth
            ),
        });
    }

    if let Some(ref e) = last_error {
        let msg: String = if e.len() <= LAST_ERROR_MAX_LEN {
            e.clone()
        } else {
            e.chars()
                .take(LAST_ERROR_MAX_LEN)
                .chain("...".chars())
                .collect()
        };
        out.push(DiagResult {
            severity: "warn".into(),
            category: "channel".into(),
            message: format!("last_error: {}", msg),
        });
    }

    out.push(DiagResult {
        severity: if memory_loaded { "ok" } else { "warn" }.into(),
        category: "context".into(),
        message: format!("memory_loaded={}", memory_loaded),
    });
    out.push(DiagResult {
        severity: "ok".into(),
        category: "context".into(),
        message: format!("skills_count={}", skills_count),
    });
    out.push(DiagResult {
        severity: if last_errors_count > 0 { "warn" } else { "ok" }.into(),
        category: "context".into(),
        message: format!("last_errors_count={}", last_errors_count),
    });

    out
}
