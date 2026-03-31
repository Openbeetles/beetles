//! network_scan 工具：WiFi 扫描与网络诊断。
//! network_scan tool: WiFi scanning and network diagnostics.

use crate::constants::NETWORK_SCAN_MIN_INTERVAL_MS;
use crate::error::{Error, Result};
use crate::tools::{parse_tool_args, Tool, ToolContext, ToolMetadata};
use crate::Platform;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use std::time::Instant;

pub struct NetworkScanTool {
    platform: Arc<dyn Platform>,
    last_scan: Mutex<Option<Instant>>,
}

impl NetworkScanTool {
    pub fn new(platform: Arc<dyn Platform>) -> Self {
        Self {
            platform,
            last_scan: Mutex::new(None),
        }
    }

    fn check_scan_rate_limit(&self) -> Result<()> {
        let last = self.last_scan.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(t) = *last {
            let elapsed = t.elapsed().as_millis() as u64;
            if elapsed < NETWORK_SCAN_MIN_INTERVAL_MS {
                return Err(Error::config(
                    "network_scan",
                    format!(
                        "rate limited: please wait {}ms before next scan",
                        NETWORK_SCAN_MIN_INTERVAL_MS - elapsed
                    ),
                ));
            }
        }
        Ok(())
    }

    fn update_scan_time(&self) {
        let mut last = self.last_scan.lock().unwrap_or_else(|e| e.into_inner());
        *last = Some(Instant::now());
    }

    fn do_wifi_scan(&self) -> Result<String> {
        self.check_scan_rate_limit()?;

        let scan_handle = self.platform.wifi_scan().ok_or_else(|| {
            Error::config("network_scan", "WiFi scan not available on this platform")
        })?;

        self.update_scan_time();
        let aps = scan_handle.request_scan()?;

        let ap_list: Vec<Value> = aps
            .iter()
            .map(|ap| {
                json!({
                    "ssid": ap.ssid,
                    "rssi": ap.rssi,
                })
            })
            .collect();

        Ok(json!({
            "op": "wifi_scan",
            "count": ap_list.len(),
            "access_points": ap_list
        })
        .to_string())
    }

    fn do_wifi_status(&self) -> Result<String> {
        let ip = self.platform.wifi_sta_ip();
        let board_info = self.platform.board_info_json().unwrap_or_default();

        Ok(json!({
            "op": "wifi_status",
            "connected": ip.is_some(),
            "ip": ip.unwrap_or_default(),
            "board_info": board_info
        })
        .to_string())
    }

    fn do_connectivity_check(&self, host: &str, ctx: &mut dyn ToolContext) -> Result<String> {
        let start = Instant::now();
        let result = ctx.get(host);
        let latency_ms = start.elapsed().as_millis() as u64;

        match result {
            Ok((status, _body)) => Ok(json!({
                "op": "connectivity_check",
                "reachable": (200..=399).contains(&status),
                "status_code": status,
                "latency_ms": latency_ms,
                "host": host
            })
            .to_string()),
            Err(e) => Ok(json!({
                "op": "connectivity_check",
                "reachable": false,
                "error": e.to_string(),
                "latency_ms": latency_ms,
                "host": host
            })
            .to_string()),
        }
    }
}

impl Tool for NetworkScanTool {
    fn name(&self) -> &'static str {
        "network_scan"
    }

    fn description(&self) -> &'static str {
        "WiFi scanning and network diagnostics. Op: wifi_scan (scan nearby APs), wifi_status (current connection info), connectivity_check (HTTP reachability test with latency)."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "op": {
                    "type": "string",
                    "description": "Operation: wifi_scan|wifi_status|connectivity_check"
                },
                "host": {
                    "type": "string",
                    "description": "URL for connectivity_check (default: http://captive.apple.com)"
                }
            },
            "required": ["op"]
        })
    }

    fn execute(&self, args: &str, ctx: &mut dyn ToolContext) -> Result<String> {
        let obj = parse_tool_args(args, "network_scan")?;
        let op = obj
            .get("op")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::config("network_scan", "missing op"))?;

        match op {
            "wifi_scan" => self.do_wifi_scan(),
            "wifi_status" => self.do_wifi_status(),
            "connectivity_check" => {
                let host = obj
                    .get("host")
                    .and_then(|v| v.as_str())
                    .unwrap_or("http://captive.apple.com");
                self.do_connectivity_check(host, ctx)
            }
            _ => Err(Error::config("network_scan", format!("unknown op: {}", op))),
        }
    }

    fn requires_network(&self) -> bool {
        // wifi_scan and wifi_status don't need network, but connectivity_check does.
        // Conservative: return false since most ops don't need it.
        false
    }

    fn metadata(&self) -> ToolMetadata {
        ToolMetadata::task().with_system_ingress(false)
    }
}
