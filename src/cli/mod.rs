//! Serial CLI：命令与 health；破坏性命令审计；无密钥输出。
//! Serial CLI: commands and health; audit for destructive commands; no secrets in output.
#![allow(clippy::too_many_arguments)]

use crate::config::{self, AppConfig};
use crate::error::Error;
use crate::memory::{MemoryStore, SessionStore, REL_PATH_SESSIONS_DIR};
use crate::platform::ConfigStore;
use crate::state;
use std::io::{self, BufRead, Write};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Instant;

const TAG: &str = "cli";

/// 设置最近错误摘要（仅用于 health；禁止写入密钥）。与 state 共用存储。
pub fn set_last_error(e: &Error) {
    state::set_last_error(e);
}

/// CLI 上下文：只读依赖，供 run_command / run_repl 使用。使用 Arc 以便跨线程 REPL。
pub struct CliContext {
    pub config: Arc<AppConfig>,
    pub config_store: Arc<dyn ConfigStore + Send + Sync>,
    pub memory: Arc<dyn MemoryStore + Send + Sync>,
    pub session: Arc<dyn SessionStore + Send + Sync>,
    pub platform: Arc<dyn crate::Platform>,
    pub tool_registry: Arc<crate::tools::ToolRegistry>,
    pub channel_capability_registry: Arc<crate::ChannelCapabilityRegistry>,
    pub capability_package_runtime_capabilities: Arc<crate::CapabilityPackageRuntimeCapabilities>,
    pub llm_stream_enabled: bool,
    /// 入站/出站队列深度（实时读取）；None 表示 bus 未暴露深度。
    pub inbound_depth: Option<Arc<std::sync::atomic::AtomicUsize>>,
    pub outbound_depth: Option<Arc<std::sync::atomic::AtomicUsize>>,
}

impl CliContext {
    /// 构建上下文（用于单线程调用 run_command）。
    pub fn new(
        config: Arc<AppConfig>,
        config_store: Arc<dyn ConfigStore + Send + Sync>,
        memory: Arc<dyn MemoryStore + Send + Sync>,
        session: Arc<dyn SessionStore + Send + Sync>,
        platform: Arc<dyn crate::Platform>,
        tool_registry: Arc<crate::tools::ToolRegistry>,
        channel_capability_registry: Arc<crate::ChannelCapabilityRegistry>,
        capability_package_runtime_capabilities: Arc<crate::CapabilityPackageRuntimeCapabilities>,
        llm_stream_enabled: bool,
        inbound_depth: Option<Arc<std::sync::atomic::AtomicUsize>>,
        outbound_depth: Option<Arc<std::sync::atomic::AtomicUsize>>,
    ) -> Self {
        Self {
            config,
            config_store,
            memory,
            session,
            platform,
            tool_registry,
            channel_capability_registry,
            capability_package_runtime_capabilities,
            llm_stream_enabled,
            inbound_depth,
            outbound_depth,
        }
    }
}

/// 解析命令行：首词为命令，其余为参数。
fn parse_args(line: &str) -> (Option<&str>, Vec<&str>) {
    let line = line.trim();
    if line.is_empty() {
        return (None, vec![]);
    }
    let parts: Vec<&str> = line.split_whitespace().collect();
    let cmd = parts.first().copied();
    let args = parts.get(1..).unwrap_or(&[]).to_vec();
    (cmd, args)
}

/// 执行单条命令，返回输出字符串（含换行）。
pub fn run_command(ctx: &CliContext, line: &str) -> String {
    let (cmd, args) = parse_args(line);
    let cmd = match cmd {
        Some(c) => c,
        None => return String::new(),
    };
    let out = match cmd {
        "wifi_status" => cmd_wifi_status(ctx),
        "memory_read" => cmd_memory_read(ctx),
        "memory_write" => cmd_memory_write(ctx, args),
        "session_list" => cmd_session_list(ctx),
        "session_clear" => cmd_session_clear(ctx, args),
        "heap_info" => cmd_heap_info(ctx),
        "restart" => cmd_restart(ctx),
        "health" => cmd_health(ctx),
        "ops_status" => cmd_ops_status(ctx),
        "package_status" => cmd_package_status(ctx),
        "package_install" => cmd_package_install(ctx, args),
        "package_enable" => cmd_package_enable(ctx, args),
        "package_disable" => cmd_package_disable(ctx, args),
        "package_uninstall" => cmd_package_uninstall(ctx, args),
        "package_rollback" => cmd_package_rollback(ctx, args),
        "baseline" => cmd_baseline(ctx),
        "spiffs_stress" => cmd_spiffs_stress(ctx, args),
        "config_show" => cmd_config_show(ctx),
        "config_reset" => cmd_config_reset(ctx, args),
        "help" | "?" => cmd_help(),
        #[cfg(feature = "ota")]
        "ota" => cmd_ota(ctx, args),
        #[cfg(not(feature = "ota"))]
        "ota" => "OTA not enabled (build with --features ota).\n".into(),
        _ => format!("Unknown command: {}. Use 'help' for list.\n", cmd),
    };
    out
}

fn cmd_wifi_status(_ctx: &CliContext) -> String {
    let status = if crate::state::wifi_sta_connected() {
        "yes"
    } else {
        "no"
    };
    format!("WiFi STA connected: {}\n", status)
}

fn cmd_memory_read(ctx: &CliContext) -> String {
    match ctx.memory.get_memory() {
        Ok(s) => {
            if s.is_empty() {
                "MEMORY.md is empty or not found.\n".into()
            } else {
                format!("=== MEMORY.md ===\n{}\n=================\n", s)
            }
        }
        Err(e) => format!("memory_read error: {}\n", state::sanitize_error_for_log(&e)),
    }
}

fn cmd_memory_write(ctx: &CliContext, args: Vec<&str>) -> String {
    let content = args.join(" ").trim().to_string();
    if content.is_empty() {
        return "Usage: memory_write <content>\n".into();
    }
    audit_log(
        "memory_write",
        Some(&format!("len={}", content.len())),
        None,
    );
    match ctx.memory.set_memory(&content) {
        Ok(()) => "MEMORY.md updated.\n".into(),
        Err(e) => format!(
            "memory_write error: {}\n",
            state::sanitize_error_for_log(&e)
        ),
    }
}

fn cmd_session_list(ctx: &CliContext) -> String {
    let mut out = "Sessions:\n".to_string();
    match ctx.platform.state_fs().list_dir(REL_PATH_SESSIONS_DIR) {
        Ok(names) => {
            let sessions: Vec<_> = names
                .into_iter()
                .filter(|n| n.ends_with(".jsonl"))
                .map(|n| n.trim_end_matches(".jsonl").to_string())
                .collect();
            if sessions.is_empty() {
                out.push_str("  No sessions found\n");
            } else {
                for s in sessions {
                    out.push_str(&format!("  Session: {}.jsonl\n", s));
                }
            }
        }
        Err(e) => out.push_str(&format!("  Error: {}\n", state::sanitize_error_for_log(&e))),
    }
    out
}

fn cmd_session_clear(ctx: &CliContext, args: Vec<&str>) -> String {
    let chat_id = match args.first().copied() {
        Some(id) => id,
        None => return "Usage: session_clear <chat_id>\n".into(),
    };
    audit_log("session_clear", None, Some(chat_id));
    match ctx.session.clear(chat_id) {
        Ok(()) => "Session cleared.\n".into(),
        Err(e) => format!(
            "Session clear error: {}\n",
            state::sanitize_error_for_log(&e)
        ),
    }
}

fn cmd_heap_info(ctx: &CliContext) -> String {
    let s = ctx.platform.memory_snapshot();
    let total = s.heap_free_internal.saturating_add(s.heap_free_spiram);
    format!(
        "Internal free: {} bytes\nPSRAM free:    {} bytes\nTotal free:    {} bytes\n",
        s.heap_free_internal, s.heap_free_spiram, total
    )
}

fn cmd_restart(ctx: &CliContext) -> String {
    log::info!("[{}] Restarting...", TAG);
    crate::runtime::request_restart_with_continuity_flush(
        Arc::clone(&ctx.platform),
        None,
        "cli_restart",
    );
    "restart: requested\n".into()
}

fn cmd_config_show(ctx: &CliContext) -> String {
    ctx.config
        .to_full_json()
        .map(|s| s + "\n")
        .unwrap_or_else(|e| format!("config_show error: {}\n", state::sanitize_error_for_log(&e)))
}

fn cmd_config_reset(ctx: &CliContext, args: Vec<&str>) -> String {
    if args != ["yes"] {
        return "Usage: config_reset yes (confirms reset)\n".into();
    }
    audit_log("config_reset", None, None);
    match config::reset_to_defaults(ctx.config_store.as_ref()) {
        Ok(()) => "Config reset. Restart to use env defaults.\n".into(),
        Err(e) => format!(
            "config_reset error: {}\n",
            state::sanitize_error_for_log(&e)
        ),
    }
}

fn cmd_health(ctx: &CliContext) -> String {
    let wifi = if crate::state::wifi_sta_connected() {
        "connected"
    } else {
        "disconnected"
    };
    let inbound = ctx
        .inbound_depth
        .as_ref()
        .map(|a| a.load(Ordering::Relaxed).to_string())
        .unwrap_or_else(|| "N/A".into());
    let outbound = ctx
        .outbound_depth
        .as_ref()
        .map(|a| a.load(Ordering::Relaxed).to_string())
        .unwrap_or_else(|| "N/A".into());
    let last_err = state::get_current_error().unwrap_or_else(|| "none".into());
    let thread_snapshot = crate::runtime::thread_registry::snapshot();
    let metrics = crate::metrics::snapshot();
    format!(
        "health:\n  wifi: {}\n  inbound_depth: {}\n  outbound_depth: {}\n  last_error: {}\n  threads_alive: {}\n  spiffs_lock_ops: {}\n  spiffs_lock_contention: {}\n",
        wifi,
        inbound,
        outbound,
        last_err,
        thread_snapshot.alive_threads,
        metrics.spiffs_lock_ops_total,
        metrics.spiffs_lock_contention_total,
    )
}

fn cmd_baseline(_ctx: &CliContext) -> String {
    let resource = crate::orchestrator::snapshot();
    let metrics = crate::metrics::snapshot();
    let thread_line = crate::runtime::thread_registry::format_baseline_log_line();
    format!(
        "baseline:\n  pressure: {:?}\n  heap_internal: {}\n  heap_spiram: {}\n  active_http: {}\n  active_wss: {}\n  active_agent_tasks: {}\n  metrics: {}\n  threads: {}\n",
        resource.pressure,
        resource.heap_free_internal,
        resource.heap_free_spiram,
        resource.active_http_count,
        resource.active_wss_count,
        resource.active_agent_tasks,
        metrics.to_baseline_log_line(),
        thread_line,
    )
}

fn cmd_ops_status(ctx: &CliContext) -> String {
    match crate::platform::operator_status::build_operator_status(
        crate::platform::operator_status::OperatorStatusInput {
            config: &ctx.config,
            platform: ctx.platform.as_ref(),
            tool_registry: ctx.tool_registry.as_ref(),
        },
    ) {
        Ok(snapshot) => crate::platform::operator_status::render_operator_status_text(&snapshot),
        Err(error) => format!(
            "ops_status error: {}\n",
            state::sanitize_error_for_log(&error)
        ),
    }
}

fn cmd_package_status(ctx: &CliContext) -> String {
    match crate::build_capability_package_operator_snapshot(
        ctx.platform.state_fs().as_ref(),
        ctx.capability_package_runtime_capabilities.as_ref(),
        ctx.config.enabled_channel.as_str(),
    ) {
        Ok(snapshot) => {
            crate::capability_package::render_capability_package_operator_text(&snapshot)
        }
        Err(error) => format!(
            "package_status error: {}\n",
            state::sanitize_error_for_log(&error)
        ),
    }
}

fn cmd_package_install(ctx: &CliContext, args: Vec<&str>) -> String {
    let path = args.join(" ").trim().to_string();
    if path.is_empty() {
        return "Usage: package_install <json_file>\n".into();
    }
    let content = match std::fs::read_to_string(&path) {
        Ok(content) => content,
        Err(error) => return format!("package_install read error: {}\n", error),
    };
    let payload = match serde_json::from_str::<crate::CapabilityPackageInstallPayload>(&content) {
        Ok(payload) => payload,
        Err(error) => return format!("package_install parse error: {}\n", error),
    };
    audit_log("package_install", Some(&format!("path={}", path)), None);
    match crate::install_capability_package(
        ctx.platform.state_fs().as_ref(),
        ctx.capability_package_runtime_capabilities.as_ref(),
        &payload,
        crate::util::current_unix_secs(),
    ) {
        Ok(outcome) => format!(
            "package_install ok: id={} version={} enabled={}\n",
            outcome.package_id, outcome.version, outcome.enabled
        ),
        Err(error) => format!(
            "package_install error: {}\n",
            state::sanitize_error_for_log(&error)
        ),
    }
}

fn cmd_package_enable(ctx: &CliContext, args: Vec<&str>) -> String {
    cmd_package_toggle(ctx, args, true)
}

fn cmd_package_disable(ctx: &CliContext, args: Vec<&str>) -> String {
    cmd_package_toggle(ctx, args, false)
}

fn cmd_package_toggle(ctx: &CliContext, args: Vec<&str>, enabled: bool) -> String {
    let Some(package_id) = args.first().copied() else {
        return if enabled {
            "Usage: package_enable <package_id>\n".into()
        } else {
            "Usage: package_disable <package_id>\n".into()
        };
    };
    audit_log(
        if enabled {
            "package_enable"
        } else {
            "package_disable"
        },
        None,
        Some(package_id),
    );
    match crate::set_capability_package_enabled(
        ctx.platform.state_fs().as_ref(),
        ctx.capability_package_runtime_capabilities.as_ref(),
        package_id,
        enabled,
        crate::util::current_unix_secs(),
    ) {
        Ok(outcome) => format!(
            "{} ok: id={} version={} enabled={}\n",
            if enabled {
                "package_enable"
            } else {
                "package_disable"
            },
            outcome.package_id,
            outcome.version,
            outcome.enabled
        ),
        Err(error) => format!(
            "{} error: {}\n",
            if enabled {
                "package_enable"
            } else {
                "package_disable"
            },
            state::sanitize_error_for_log(&error)
        ),
    }
}

fn cmd_package_uninstall(ctx: &CliContext, args: Vec<&str>) -> String {
    let Some(package_id) = args.first().copied() else {
        return "Usage: package_uninstall <package_id>\n".into();
    };
    audit_log("package_uninstall", None, Some(package_id));
    match crate::uninstall_capability_package(
        ctx.platform.state_fs().as_ref(),
        package_id,
        crate::util::current_unix_secs(),
    ) {
        Ok(outcome) => format!(
            "package_uninstall ok: id={} version={}\n",
            outcome.package_id, outcome.version
        ),
        Err(error) => format!(
            "package_uninstall error: {}\n",
            state::sanitize_error_for_log(&error)
        ),
    }
}

fn cmd_package_rollback(ctx: &CliContext, args: Vec<&str>) -> String {
    let Some(package_id) = args.first().copied() else {
        return "Usage: package_rollback <package_id>\n".into();
    };
    audit_log("package_rollback", None, Some(package_id));
    match crate::rollback_capability_package(
        ctx.platform.state_fs().as_ref(),
        ctx.capability_package_runtime_capabilities.as_ref(),
        package_id,
        crate::util::current_unix_secs(),
    ) {
        Ok(outcome) => format!(
            "package_rollback ok: id={} version={} enabled={}\n",
            outcome.package_id, outcome.version, outcome.enabled
        ),
        Err(error) => format!(
            "package_rollback error: {}\n",
            state::sanitize_error_for_log(&error)
        ),
    }
}

fn cmd_spiffs_stress(ctx: &CliContext, args: Vec<&str>) -> String {
    const DEFAULT_WORKERS: usize = 4;
    const DEFAULT_ROUNDS: usize = 64;
    const DEFAULT_PAYLOAD_BYTES: usize = 1024;
    const MAX_WORKERS: usize = 16;
    const MAX_ROUNDS: usize = 1024;
    const MAX_PAYLOAD_BYTES: usize = 16 * 1024;

    let workers = args
        .first()
        .and_then(|raw| raw.parse::<usize>().ok())
        .unwrap_or(DEFAULT_WORKERS)
        .clamp(1, MAX_WORKERS);
    let rounds = args
        .get(1)
        .and_then(|raw| raw.parse::<usize>().ok())
        .unwrap_or(DEFAULT_ROUNDS)
        .clamp(1, MAX_ROUNDS);
    let payload_bytes = args
        .get(2)
        .and_then(|raw| raw.parse::<usize>().ok())
        .unwrap_or(DEFAULT_PAYLOAD_BYTES)
        .clamp(64, MAX_PAYLOAD_BYTES);

    let fs = ctx.platform.state_fs();
    let before = crate::metrics::snapshot();
    let start = Instant::now();
    let mut handles = Vec::with_capacity(workers);

    for worker_id in 0..workers {
        let state_fs = Arc::clone(&fs);
        let name = format!("spiffs_stress_{}", worker_id);
        let handle = std::thread::Builder::new()
            .name(name)
            .stack_size(4096)
            .spawn(move || -> crate::error::Result<()> {
                let rel_path = format!("diag/spiffs_stress_{}.bin", worker_id);
                let fill = b'a'.saturating_add((worker_id % 26) as u8);
                let payload = vec![fill; payload_bytes];
                for round in 0..rounds {
                    state_fs.write(&rel_path, &payload)?;
                    let _ = state_fs.read(&rel_path)?;
                    if round % 8 == 7 {
                        let _ = state_fs.remove(&rel_path);
                    }
                }
                let _ = state_fs.remove(&rel_path);
                Ok(())
            });
        match handle {
            Ok(handle) => handles.push(handle),
            Err(e) => {
                return format!("spiffs_stress spawn error: {}\n", e);
            }
        }
    }

    for handle in handles {
        match handle.join() {
            Ok(Ok(())) => {}
            Ok(Err(e)) => return format!("spiffs_stress worker error: {}\n", e),
            Err(_) => return "spiffs_stress worker panicked\n".into(),
        }
    }

    let elapsed_ms = start.elapsed().as_millis();
    let after = crate::metrics::snapshot();
    format!(
        "spiffs_stress:\n  workers: {}\n  rounds: {}\n  payload_bytes: {}\n  elapsed_ms: {}\n  lock_ops_delta: {}\n  contention_delta: {}\n  wait_total_us_delta: {}\n  hold_total_us_delta: {}\n",
        workers,
        rounds,
        payload_bytes,
        elapsed_ms,
        after
            .spiffs_lock_ops_total
            .saturating_sub(before.spiffs_lock_ops_total),
        after
            .spiffs_lock_contention_total
            .saturating_sub(before.spiffs_lock_contention_total),
        after
            .spiffs_lock_wait_total_us
            .saturating_sub(before.spiffs_lock_wait_total_us),
        after
            .spiffs_lock_hold_total_us
            .saturating_sub(before.spiffs_lock_hold_total_us),
    )
}

fn cmd_help() -> String {
    let ota_line = if cfg!(feature = "ota") {
        "  ota <url>        - OTA update from URL, then restart\n"
    } else {
        ""
    };
    format!(
        "Commands:\n  wifi_status      - WiFi connection status\n  memory_read      - Read MEMORY.md\n  memory_write <content> - Write MEMORY.md (audit)\n  session_list     - List all sessions\n  session_clear <chat_id> - Clear session (audit)\n  heap_info        - Heap usage\n  restart          - Restart device\n  health           - WiFi, queue depth, last error\n  ops_status       - Unified operator/platform/tool status\n  package_status   - Capability package snapshot\n  package_install <json_file> - Install capability package from JSON file\n  package_enable <package_id> - Enable capability package\n  package_disable <package_id> - Disable capability package\n  package_uninstall <package_id> - Uninstall capability package\n  package_rollback <package_id> - Roll back capability package\n  baseline         - Resource, metrics, thread baseline\n  spiffs_stress [workers] [rounds] [payload_bytes] - Stress SPIFFS lock and report deltas\n  config_show      - Show full config\n  config_reset yes - Reset config to env defaults (audit)\n{}  help|?         - This help\n",
        ota_line
    )
}

#[cfg(feature = "ota")]
fn cmd_ota(ctx: &CliContext, args: Vec<&str>) -> String {
    let url = match args.first().copied() {
        Some(u) => u,
        None => return "Usage: ota <url>\n".into(),
    };
    match ctx.platform.ota_from_url(url) {
        Ok(()) => {
            log::info!("[{}] OTA done, restarting", TAG);
            crate::runtime::request_restart_with_continuity_flush(
                Arc::clone(&ctx.platform),
                None,
                "cli_ota_restart",
            );
            "OTA successful. Restarting...\n".into()
        }
        Err(e) => format!("OTA failed: {}\n", state::sanitize_error_for_log(&e)),
    }
}

/// 审计日志：仅命令名、时间、chat_id/无敏感信息；不打印密钥。
fn audit_log(cmd: &str, extra: Option<&str>, chat_id: Option<&str>) {
    let mut msg = format!("AUDIT: {} (no secrets)", cmd);
    if let Some(id) = chat_id {
        msg.push_str(&format!(" chat_id={}", id));
    }
    if let Some(e) = extra {
        msg.push_str(&format!(" {}", e));
    }
    log::info!("[{}] {}", TAG, msg);
}

/// 阻塞式 REPL：从 reader 读行，执行命令并写输出到 stdout。用于串口/stdio。
pub fn run_repl<R: BufRead + Send>(ctx: CliContext, mut reader: R) {
    let mut line = String::new();
    let prompt = b"mimi> ";
    loop {
        let _ = io::stdout().write_all(prompt);
        let _ = io::stdout().flush();
        line.clear();
        if reader.read_line(&mut line).is_err() {
            continue;
        }
        let out = run_command(&ctx, line.trim_end());
        let _ = io::stdout().write_all(out.as_bytes());
    }
}
