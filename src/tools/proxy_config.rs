//! proxy_config 工具：运行时代理配置管理。
//! proxy_config tool: runtime proxy configuration management.

use crate::error::{Error, Result};
use crate::platform::ConfigStore;
use crate::tools::{Tool, ToolContext, ToolMetadata, parse_tool_args};
use serde_json::json;
use std::sync::Arc;

const NVS_KEY_PROXY_URL: &str = "proxy_url";

/// Redact a secret URL for display: show scheme + host, mask the rest.
fn redact_secret(url: &str) -> String {
    // Show scheme://host:port/*** for safety
    if let Some(scheme_end) = url.find("://") {
        let after_scheme = &url[scheme_end + 3..];
        let host_end = after_scheme.find('/').unwrap_or(after_scheme.len());
        let host = &after_scheme[..host_end];
        format!("{}://{}/<REDACTED>", &url[..scheme_end], host)
    } else {
        "***REDACTED***".to_string()
    }
}

pub struct ProxyConfigTool {
    config_store: Arc<dyn ConfigStore + Send + Sync>,
}

impl ProxyConfigTool {
    pub fn new(config_store: Arc<dyn ConfigStore + Send + Sync>) -> Self {
        Self { config_store }
    }
}

impl Tool for ProxyConfigTool {
    fn name(&self) -> &'static str {
        "proxy_config"
    }
    fn description(&self) -> &'static str {
        "Manage HTTP proxy configuration. Op: get (show current proxy, redacted), set (set proxy URL), clear (remove proxy). Changes take effect after restart."
    }
    fn schema(&self) -> &str {
        r#"{"type":"object","properties":{"op":{"type":"string","description":"Operation: get|set|clear"},"url":{"type":"string","description":"Proxy URL for set operation (e.g. http://proxy:8080)"}},"required":["op"]}"#
    }
    fn execute(&self, args: &str, _ctx: &mut dyn ToolContext) -> Result<String> {
        let obj = parse_tool_args(args, "tool_proxy_config")?;
        let op = obj
            .get("op")
            .and_then(|x| x.as_str())
            .ok_or_else(|| Error::config("tool_proxy_config", "missing op"))?;

        match op {
            "get" => {
                let url = self.config_store.read_string(NVS_KEY_PROXY_URL)?;
                match url {
                    Some(u) => Ok(json!({
                        "op": "get",
                        "proxy_url": redact_secret(&u),
                        "configured": true,
                        "note": "URL is redacted for security. Changes require restart."
                    })
                    .to_string()),
                    None => Ok(json!({
                        "op": "get",
                        "configured": false,
                        "note": "No proxy configured"
                    })
                    .to_string()),
                }
            }
            "set" => {
                let url = obj
                    .get("url")
                    .and_then(|x| x.as_str())
                    .ok_or_else(|| Error::config("tool_proxy_config", "missing url"))?;
                if url.is_empty() {
                    return Err(Error::config("tool_proxy_config", "url cannot be empty"));
                }
                if !url.starts_with("http://")
                    && !url.starts_with("https://")
                    && !url.starts_with("socks5://")
                {
                    return Err(Error::config(
                        "tool_proxy_config",
                        "url must start with http://, https://, or socks5://",
                    ));
                }
                self.config_store.write_string(NVS_KEY_PROXY_URL, url)?;
                Ok(json!({
                    "op": "set",
                    "ok": true,
                    "note": "Proxy configured. Restart required for changes to take effect."
                })
                .to_string())
            }
            "clear" => {
                self.config_store.erase_keys(&[NVS_KEY_PROXY_URL])?;
                Ok(json!({
                    "op": "clear",
                    "ok": true,
                    "note": "Proxy cleared. Restart required for changes to take effect."
                })
                .to_string())
            }
            _ => Err(Error::config(
                "tool_proxy_config",
                format!("unknown op: {}", op),
            )),
        }
    }

    fn metadata(&self) -> ToolMetadata {
        ToolMetadata::admin()
    }
}
