//! POST /api/restart：配对后执行重启，带冷却。

use std::sync::Mutex;
use std::time::Instant;

use crate::platform::http_server::common::{ApiResponse, RESTART_COOLDOWN_SECS};

use super::HandlerContext;

static LAST_RESTART: Mutex<Option<Instant>> = Mutex::new(None);

/// 返回响应和延迟重启动作标记；transport 写出响应后交给 runtime 调度器执行重启。
pub fn post(_ctx: &HandlerContext) -> Result<(ApiResponse, bool), std::io::Error> {
    let should_restart = {
        let mut g = LAST_RESTART
            .lock()
            .map_err(|_| std::io::Error::other("lock"))?;
        let now = Instant::now();
        let allow = g
            .map(|t| now.duration_since(t).as_secs() >= RESTART_COOLDOWN_SECS)
            .unwrap_or(true);
        if allow {
            *g = Some(now);
            true
        } else {
            false
        }
    };
    Ok((ApiResponse::ok_200_json("{\"ok\":true}"), should_restart))
}
