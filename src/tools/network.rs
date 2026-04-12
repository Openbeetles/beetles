//! Structured host network inspection and probing.

use crate::error::{Error, Result};
use crate::tools::{
    parse_tool_args, Tool, ToolContext, ToolEffectClass, ToolMetadata, ToolRiskLevel,
};
use serde_json::{json, Value};
use std::net::ToSocketAddrs;
use std::time::Instant;

#[cfg(target_os = "linux")]
use crate::host_observability::{
    list_linux_network_interfaces, read_linux_default_route, read_linux_dns_config,
};

#[cfg(target_os = "linux")]
use std::process::Command;

const DEFAULT_PING_COUNT: u32 = 3;
const MAX_PING_COUNT: u32 = 5;

#[derive(Default)]
pub struct NetworkTool;

impl Tool for NetworkTool {
    fn name(&self) -> &'static str {
        "network"
    }

    fn description(&self) -> &str {
        "Inspect Linux host networking. Op: interfaces, dns, route, resolve, ping, or http_probe. Returns structured JSON instead of raw command output."
    }

    fn schema(&self) -> &str {
        r#"{"type":"object","properties":{"op":{"type":"string","enum":["interfaces","dns","route","resolve","ping","http_probe"],"description":"Operation to perform"},"host":{"type":"string","description":"Host name for resolve/ping"},"url":{"type":"string","description":"URL for http_probe"},"port":{"type":"integer","description":"Port for resolve (default 80)"},"count":{"type":"integer","description":"Ping packet count (default 3, max 5)"}},"required":["op"]}"#
    }

    fn execute(&self, args: &str, ctx: &mut dyn ToolContext) -> Result<String> {
        let obj = parse_tool_args(args, "network_tool")?;
        let op = obj
            .get("op")
            .and_then(|x| x.as_str())
            .ok_or_else(|| Error::config("network_tool", "missing op"))?;

        match op {
            "interfaces" => Ok(json!({
                "op": "interfaces",
                "interfaces": list_interfaces_linux()?,
            })
            .to_string()),
            "dns" => Ok(json!({
                "op": "dns",
                "config": read_dns_config_linux()?,
            })
            .to_string()),
            "route" => Ok(json!({
                "op": "route",
                "default_route": read_default_route_linux()?,
            })
            .to_string()),
            "resolve" => {
                let host = obj
                    .get("host")
                    .and_then(|x| x.as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| Error::config("network_tool", "host required for resolve"))?;
                let port = obj
                    .get("port")
                    .and_then(|x| x.as_u64())
                    .unwrap_or(80)
                    .clamp(1, u16::MAX as u64) as u16;
                let resolved = resolve_host(host, port)?;
                Ok(json!({
                    "op": "resolve",
                    "host": host,
                    "port": port,
                    "addresses": resolved,
                })
                .to_string())
            }
            "ping" => {
                let host = obj
                    .get("host")
                    .and_then(|x| x.as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| Error::config("network_tool", "host required for ping"))?;
                let count = obj
                    .get("count")
                    .and_then(|x| x.as_u64())
                    .unwrap_or(DEFAULT_PING_COUNT as u64)
                    .clamp(1, MAX_PING_COUNT as u64) as u32;
                ping_host_linux(host, count)
            }
            "http_probe" => {
                let url = obj
                    .get("url")
                    .and_then(|x| x.as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| Error::config("network_tool", "url required for http_probe"))?;
                http_probe(url, ctx)
            }
            _ => Err(Error::config("network_tool", format!("invalid op: {}", op))),
        }
    }

    fn requires_network(&self) -> bool {
        true
    }

    fn requires_network_for(&self, args: &str) -> Result<bool> {
        let obj = parse_tool_args(args, "network_tool")?;
        let op = obj
            .get("op")
            .and_then(|x| x.as_str())
            .ok_or_else(|| Error::config("network_tool", "missing op"))?;
        Ok(matches!(op, "resolve" | "ping" | "http_probe"))
    }

    fn metadata(&self) -> ToolMetadata {
        ToolMetadata::task()
            .with_system_ingress(false)
            .with_effect_class(ToolEffectClass::HostInspection)
            .with_risk_level(ToolRiskLevel::Medium)
    }
}

#[cfg(target_os = "linux")]
fn list_interfaces_linux() -> Result<Vec<Value>> {
    Ok(list_linux_network_interfaces()
        .into_iter()
        .map(|iface| serde_json::to_value(iface).unwrap_or(Value::Null))
        .collect())
}

#[cfg(not(target_os = "linux"))]
fn list_interfaces_linux() -> Result<Vec<Value>> {
    Err(Error::config(
        "network_tool",
        "network inspection is only available on Linux",
    ))
}

#[cfg(target_os = "linux")]
fn read_dns_config_linux() -> Result<Value> {
    serde_json::to_value(read_linux_dns_config())
        .map_err(|e| Error::config("network_dns", e.to_string()))
}

#[cfg(not(target_os = "linux"))]
fn read_dns_config_linux() -> Result<Value> {
    Err(Error::config(
        "network_tool",
        "network inspection is only available on Linux",
    ))
}

#[cfg(target_os = "linux")]
fn read_default_route_linux() -> Result<Value> {
    serde_json::to_value(read_linux_default_route())
        .map_err(|e| Error::config("network_route", e.to_string()))
}

#[cfg(not(target_os = "linux"))]
fn read_default_route_linux() -> Result<Value> {
    Err(Error::config(
        "network_tool",
        "network inspection is only available on Linux",
    ))
}

fn resolve_host(host: &str, port: u16) -> Result<Vec<String>> {
    let mut addresses = Vec::new();
    for addr in (host, port).to_socket_addrs().map_err(|e| Error::Other {
        source: Box::new(e),
        stage: "network_resolve",
    })? {
        let rendered = addr.ip().to_string();
        if !addresses.iter().any(|item| item == &rendered) {
            addresses.push(rendered);
        }
    }
    if addresses.is_empty() {
        return Err(Error::config(
            "network_resolve",
            format!("no addresses resolved for {}", host),
        ));
    }
    Ok(addresses)
}

#[cfg(target_os = "linux")]
fn ping_host_linux(host: &str, count: u32) -> Result<String> {
    let output = Command::new("ping")
        .args(["-c", &count.to_string(), "-W", "2", host])
        .output()
        .map_err(|e| Error::Other {
            source: Box::new(e),
            stage: "network_ping",
        })?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let summary = parse_ping_summary(&stdout);
    Ok(json!({
        "op": "ping",
        "host": host,
        "count": count,
        "success": output.status.success(),
        "summary": summary,
        "error": (!stderr.trim().is_empty()).then_some(stderr.trim()),
    })
    .to_string())
}

#[cfg(not(target_os = "linux"))]
fn ping_host_linux(_host: &str, _count: u32) -> Result<String> {
    Err(Error::config(
        "network_tool",
        "ping is only available on Linux",
    ))
}

fn http_probe(url: &str, ctx: &mut dyn ToolContext) -> Result<String> {
    let start = Instant::now();
    match ctx.get(url) {
        Ok((status, _body)) => Ok(json!({
            "op": "http_probe",
            "url": url,
            "reachable": (200..=399).contains(&status),
            "status_code": status,
            "latency_ms": start.elapsed().as_millis(),
        })
        .to_string()),
        Err(e) => Ok(json!({
            "op": "http_probe",
            "url": url,
            "reachable": false,
            "latency_ms": start.elapsed().as_millis(),
            "error": e.to_string(),
        })
        .to_string()),
    }
}

#[cfg(any(target_os = "linux", test))]
fn parse_ping_summary(stdout: &str) -> Value {
    let mut transmitted = None;
    let mut received = None;
    let mut packet_loss_percent = None;
    let mut avg_rtt_ms = None;

    for line in stdout.lines() {
        let trimmed = line.trim();
        if trimmed.contains("packet loss") {
            let numbers = extract_numbers(trimmed);
            if numbers.len() >= 3 {
                transmitted = numbers.first().copied().map(|n| n as u32);
                received = numbers.get(1).copied().map(|n| n as u32);
                packet_loss_percent = numbers.get(2).copied();
            }
        } else if trimmed.starts_with("rtt ") || trimmed.starts_with("round-trip ") {
            if let Some(values) = trimmed.split('=').nth(1) {
                let mut parts = values.trim().split('/').map(str::trim);
                let _min = parts.next();
                avg_rtt_ms = parts.next().and_then(|v| v.parse::<f64>().ok());
            }
        }
    }

    json!({
        "transmitted": transmitted,
        "received": received,
        "packet_loss_percent": packet_loss_percent,
        "avg_rtt_ms": avg_rtt_ms,
    })
}

#[cfg(any(target_os = "linux", test))]
fn extract_numbers(raw: &str) -> Vec<f64> {
    let mut out = Vec::new();
    let mut current = String::new();
    for ch in raw.chars() {
        if ch.is_ascii_digit() || ch == '.' {
            current.push(ch);
            continue;
        }
        if !current.is_empty() {
            if let Ok(value) = current.parse::<f64>() {
                out.push(value);
            }
            current.clear();
        }
    }
    if !current.is_empty() {
        if let Ok(value) = current.parse::<f64>() {
            out.push(value);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{http_probe, parse_ping_summary, NetworkTool};
    use crate::error::Result;
    use crate::host_observability::{parse_default_route, parse_resolv_conf};
    use crate::i18n::Locale;
    use crate::platform::ResponseBody;
    use crate::tools::{Tool, ToolContext};

    struct MockToolContext {
        status: u16,
        body: Vec<u8>,
    }

    impl ToolContext for MockToolContext {
        fn get_with_headers(
            &mut self,
            _url: &str,
            _headers: &[(&str, &str)],
        ) -> Result<(u16, ResponseBody)> {
            Ok((self.status, ResponseBody::Heap(self.body.clone())))
        }

        fn post_with_headers(
            &mut self,
            _url: &str,
            _headers: &[(&str, &str)],
            _body: &[u8],
        ) -> Result<(u16, ResponseBody)> {
            unreachable!()
        }

        fn user_locale(&self) -> Locale {
            Locale::Zh
        }
    }

    #[test]
    fn parse_resolver_config_extracts_core_fields() {
        let raw = "\
nameserver 1.1.1.1\n\
nameserver 8.8.8.8\n\
search lan local\n\
options timeout:2 attempts:3\n";
        let parsed = parse_resolv_conf(raw);
        assert_eq!(parsed.nameservers, vec!["1.1.1.1", "8.8.8.8"]);
        assert_eq!(parsed.search, vec!["lan", "local"]);
        assert_eq!(parsed.options, vec!["timeout:2", "attempts:3"]);
    }

    #[test]
    fn parse_default_route_reads_gateway() {
        let raw = "\
Iface\tDestination\tGateway\tFlags\tRefCnt\tUse\tMetric\tMask\n\
eth0\t00000000\t0101A8C0\t0003\t0\t0\t0\t00000000\n";
        let route = parse_default_route(raw).unwrap();
        assert_eq!(route.interface, "eth0");
        assert_eq!(route.gateway, "192.168.1.1");
    }

    #[test]
    fn parse_ping_summary_extracts_loss_and_latency() {
        let summary = parse_ping_summary(
            "\
3 packets transmitted, 3 received, 0% packet loss, time 2004ms\n\
rtt min/avg/max/mdev = 22.631/24.087/25.515/1.181 ms\n",
        );
        assert_eq!(summary["transmitted"], 3);
        assert_eq!(summary["received"], 3);
        assert_eq!(summary["packet_loss_percent"], 0.0);
        assert_eq!(summary["avg_rtt_ms"], 24.087);
    }

    #[test]
    fn http_probe_returns_structured_result() {
        let payload = http_probe(
            "https://example.com",
            &mut MockToolContext {
                status: 204,
                body: Vec::new(),
            },
        )
        .unwrap();
        assert!(payload.contains("\"reachable\":true"));
        assert!(payload.contains("\"status_code\":204"));
    }

    #[test]
    fn http_probe_operation_is_executable() {
        let tool = NetworkTool;
        let payload = tool
            .execute(
                r#"{"op":"http_probe","url":"https://example.com"}"#,
                &mut MockToolContext {
                    status: 200,
                    body: Vec::new(),
                },
            )
            .unwrap();
        assert!(payload.contains("\"op\":\"http_probe\""));
    }

    #[test]
    fn dynamic_network_requirement_only_marks_remote_ops() {
        let tool = NetworkTool;

        assert!(!tool.requires_network_for(r#"{"op":"interfaces"}"#).unwrap());
        assert!(!tool.requires_network_for(r#"{"op":"dns"}"#).unwrap());
        assert!(!tool.requires_network_for(r#"{"op":"route"}"#).unwrap());
        assert!(tool
            .requires_network_for(r#"{"op":"resolve","host":"example.com"}"#)
            .unwrap());
        assert!(tool
            .requires_network_for(r#"{"op":"ping","host":"example.com"}"#)
            .unwrap());
        assert!(tool
            .requires_network_for(r#"{"op":"http_probe","url":"https://example.com"}"#)
            .unwrap());
    }
}
