//! http_request 工具：统一 HTTP 请求工具，支持 GET/POST/PUT/DELETE/PATCH。
//! http_request tool: unified HTTP request with GET/POST/PUT/DELETE/PATCH support.

use crate::error::{Error, Result};
use crate::tools::{parse_tool_args, Tool, ToolContext};
use serde_json::json;

pub struct HttpRequestTool;

impl Tool for HttpRequestTool {
    fn name(&self) -> &'static str {
        "http_request"
    }
    fn description(&self) -> &'static str {
        "Make HTTP requests. Supports GET, POST, PUT, DELETE, PATCH methods with custom headers and body. Private IPs are blocked (SSRF protection)."
    }
    fn schema(&self) -> &str {
        r#"{"type":"object","properties":{"url":{"type":"string","description":"Request URL (must be https:// or http://)"},"method":{"type":"string","description":"HTTP method: GET|POST|PUT|DELETE|PATCH (default GET)"},"headers":{"type":"object","description":"Optional headers as key-value pairs"},"body":{"type":"string","description":"Request body (for POST/PUT/PATCH)"},"content_type":{"type":"string","description":"Content-Type header (default application/json)"}},"required":["url"]}"#
    }
    fn requires_network(&self) -> bool {
        true
    }
    fn execute(&self, args: &str, ctx: &mut dyn ToolContext) -> Result<String> {
        let obj = parse_tool_args(args, "tool_http_request")?;
        let url = obj
            .get("url")
            .and_then(|x| x.as_str())
            .ok_or_else(|| Error::config("tool_http_request", "missing url"))?;
        let method = obj
            .get("method")
            .and_then(|x| x.as_str())
            .unwrap_or("GET")
            .to_uppercase();
        let content_type = obj
            .get("content_type")
            .and_then(|x| x.as_str())
            .unwrap_or("application/json");
        let body_str = obj.get("body").and_then(|x| x.as_str()).unwrap_or("");

        // SSRF protection: block private IPs
        if is_private_url(url) {
            return Err(Error::config(
                "tool_http_request",
                "private/internal URLs are blocked for security",
            ));
        }

        // Build headers
        let mut header_pairs: Vec<(String, String)> = Vec::new();
        if let Some(headers_obj) = obj.get("headers").and_then(|x| x.as_object()) {
            for (k, v) in headers_obj {
                if let Some(vs) = v.as_str() {
                    header_pairs.push((k.clone(), vs.to_string()));
                }
            }
        }

        // Add content-type for body-bearing methods
        if matches!(method.as_str(), "POST" | "PUT" | "PATCH") && !body_str.is_empty() {
            let has_ct = header_pairs
                .iter()
                .any(|(k, _)| k.eq_ignore_ascii_case("content-type"));
            if !has_ct {
                header_pairs.push(("Content-Type".to_string(), content_type.to_string()));
            }
        }

        let headers_ref: Vec<(&str, &str)> = header_pairs
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();

        let (status, resp_body) = match method.as_str() {
            "GET" => ctx.get_with_headers(url, &headers_ref)?,
            "POST" => ctx.post_with_headers(url, &headers_ref, body_str.as_bytes())?,
            "PUT" => ctx.put_with_headers(url, &headers_ref, body_str.as_bytes())?,
            "DELETE" => ctx.delete_with_headers(url, &headers_ref)?,
            "PATCH" => ctx.patch_with_headers(url, &headers_ref, body_str.as_bytes())?,
            _ => {
                return Err(Error::config(
                    "tool_http_request",
                    format!("unsupported method: {}", method),
                ));
            }
        };

        let body_text = String::from_utf8_lossy(resp_body.as_ref());
        // Truncate response to avoid excessive token usage
        let max_resp = 8 * 1024;
        let (truncated, body_out) = if body_text.len() > max_resp {
            (true, &body_text[..max_resp])
        } else {
            (false, body_text.as_ref())
        };

        Ok(json!({
            "status": status,
            "body": body_out,
            "truncated": truncated
        })
        .to_string())
    }
}

/// Check if a URL targets a private/internal IP address (SSRF protection).
pub(crate) fn is_private_url(url: &str) -> bool {
    // Extract host from URL
    let url_lower = url.to_lowercase();
    let after_scheme = if let Some(rest) = url_lower.strip_prefix("https://") {
        rest
    } else if let Some(rest) = url_lower.strip_prefix("http://") {
        rest
    } else {
        return true; // Block non-http(s)
    };

    let host = after_scheme
        .split('/')
        .next()
        .unwrap_or("")
        .split(':')
        .next()
        .unwrap_or("");

    if host.is_empty() {
        return true;
    }

    // Block localhost
    if host == "localhost" || host == "127.0.0.1" || host == "::1" || host == "[::1]" {
        return true;
    }

    // Block private IP ranges
    if let Some(ip) = parse_ipv4(host) {
        let [a, b, _, _] = ip;
        // 10.0.0.0/8
        if a == 10 {
            return true;
        }
        // 172.16.0.0/12
        if a == 172 && (16..=31).contains(&b) {
            return true;
        }
        // 192.168.0.0/16
        if a == 192 && b == 168 {
            return true;
        }
        // 169.254.0.0/16 (link-local)
        if a == 169 && b == 254 {
            return true;
        }
        // 0.0.0.0
        if a == 0 {
            return true;
        }
    }

    false
}

fn parse_ipv4(host: &str) -> Option<[u8; 4]> {
    let parts: Vec<&str> = host.split('.').collect();
    if parts.len() != 4 {
        return None;
    }
    let mut octets = [0u8; 4];
    for (i, part) in parts.iter().enumerate() {
        octets[i] = part.parse().ok()?;
    }
    Some(octets)
}

#[cfg(test)]
mod tests {
    use super::is_private_url;

    #[test]
    fn blocks_private_and_non_http_urls() {
        assert!(is_private_url("http://127.0.0.1/test"));
        assert!(is_private_url("https://192.168.1.8/api"));
        assert!(is_private_url("ftp://example.com/file"));
        assert!(is_private_url("not a url"));
    }

    #[test]
    fn allows_public_http_urls() {
        assert!(!is_private_url("https://example.com"));
        assert!(!is_private_url("http://8.8.8.8/resolve"));
    }
}
