//! Process inspection tool for host Linux.

use crate::error::{Error, Result};
use crate::tools::{parse_tool_args, Tool, ToolContext, ToolMetadata};
use serde_json::{json, Value};

const DEFAULT_PROCESS_LIST_LIMIT: usize = 16;
const MAX_PROCESS_LIST_LIMIT: usize = 64;

#[derive(Default)]
pub struct ProcessTool;

impl Tool for ProcessTool {
    fn name(&self) -> &'static str {
        "process"
    }

    fn description(&self) -> &str {
        "Inspect running processes on Linux. Op: list (recent process overview) or inspect (details for one pid). Returns structured JSON instead of raw shell output."
    }

    fn schema(&self) -> &str {
        r#"{"type":"object","properties":{"op":{"type":"string","enum":["list","inspect"],"description":"Operation to perform"},"pid":{"type":"integer","description":"Target process ID for inspect"},"query":{"type":"string","description":"Optional substring filter for name/cmdline in list mode"},"limit":{"type":"integer","description":"Max processes to return in list mode (default 16, max 64)"},"include_cmdline":{"type":"boolean","description":"Whether to include full command lines in output"}},"required":["op"]}"#
    }

    fn execute(&self, args: &str, _ctx: &mut dyn ToolContext) -> Result<String> {
        let obj = parse_tool_args(args, "process_tool")?;
        let op = obj
            .get("op")
            .and_then(|x| x.as_str())
            .ok_or_else(|| Error::config("process_tool", "missing op"))?;
        let include_cmdline = obj
            .get("include_cmdline")
            .and_then(|x| x.as_bool())
            .unwrap_or(false);

        match op {
            "list" => {
                let limit = obj
                    .get("limit")
                    .and_then(|x| x.as_u64())
                    .map(|n| n as usize)
                    .unwrap_or(DEFAULT_PROCESS_LIST_LIMIT)
                    .clamp(1, MAX_PROCESS_LIST_LIMIT);
                let query = obj
                    .get("query")
                    .and_then(|x| x.as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty());
                let processes = list_processes_linux(query, limit, include_cmdline)?;
                Ok(json!({
                    "op": "list",
                    "query": query.unwrap_or_default(),
                    "count": processes.len(),
                    "processes": processes.into_iter().map(|p| p.to_json(include_cmdline)).collect::<Vec<_>>(),
                })
                .to_string())
            }
            "inspect" => {
                let pid = obj
                    .get("pid")
                    .and_then(|x| x.as_i64())
                    .ok_or_else(|| Error::config("process_tool", "pid required for inspect"))?;
                if pid <= 0 || pid > i32::MAX as i64 {
                    return Err(Error::config(
                        "process_tool",
                        "pid must be a positive integer",
                    ));
                }
                let snapshot = inspect_process_linux(pid as i32, true)?;
                Ok(json!({
                    "op": "inspect",
                    "process": snapshot.to_json(true),
                })
                .to_string())
            }
            _ => Err(Error::config("process_tool", format!("invalid op: {}", op))),
        }
    }

    fn metadata(&self) -> ToolMetadata {
        ToolMetadata::task().with_system_ingress(false)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ProcessSnapshot {
    pid: i32,
    ppid: i32,
    name: String,
    state: String,
    threads: u32,
    vm_rss_kb: u64,
    vm_size_kb: u64,
    cmdline: String,
}

impl ProcessSnapshot {
    fn to_json(&self, include_cmdline: bool) -> Value {
        json!({
            "pid": self.pid,
            "ppid": self.ppid,
            "name": self.name,
            "state": self.state,
            "threads": self.threads,
            "vm_rss_kb": self.vm_rss_kb,
            "vm_size_kb": self.vm_size_kb,
            "cmdline": include_cmdline.then_some(self.cmdline.as_str()),
        })
    }
}

#[cfg(target_os = "linux")]
fn list_processes_linux(
    query: Option<&str>,
    limit: usize,
    include_cmdline: bool,
) -> Result<Vec<ProcessSnapshot>> {
    let lowered_query = query.map(|q| q.to_ascii_lowercase());
    let need_cmdline = include_cmdline || lowered_query.is_some();
    let mut processes = Vec::new();

    let entries = std::fs::read_dir("/proc").map_err(|e| Error::Other {
        source: Box::new(e),
        stage: "process_list",
    })?;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(pid) = name.to_string_lossy().parse::<i32>().ok() else {
            continue;
        };
        let Ok(snapshot) = inspect_process_linux(pid, need_cmdline) else {
            continue;
        };
        if let Some(ref needle) = lowered_query {
            let matches_name = snapshot.name.to_ascii_lowercase().contains(needle);
            let matches_cmd = snapshot.cmdline.to_ascii_lowercase().contains(needle);
            if !matches_name && !matches_cmd {
                continue;
            }
        }
        processes.push(snapshot);
    }

    processes.sort_by(|a, b| {
        b.vm_rss_kb
            .cmp(&a.vm_rss_kb)
            .then_with(|| a.pid.cmp(&b.pid))
    });
    if processes.len() > limit {
        processes.truncate(limit);
    }
    Ok(processes)
}

#[cfg(not(target_os = "linux"))]
fn list_processes_linux(
    _query: Option<&str>,
    _limit: usize,
    _include_cmdline: bool,
) -> Result<Vec<ProcessSnapshot>> {
    Err(Error::config(
        "process_tool",
        "process inspection is only available on Linux",
    ))
}

#[cfg(target_os = "linux")]
fn inspect_process_linux(pid: i32, include_cmdline: bool) -> Result<ProcessSnapshot> {
    let proc_root = format!("/proc/{pid}");
    let status = std::fs::read_to_string(format!("{proc_root}/status")).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            Error::config("process_tool", format!("pid {} not found", pid))
        } else {
            Error::Other {
                source: Box::new(e),
                stage: "process_inspect",
            }
        }
    })?;

    let name = status_field(&status, "Name:")
        .map(str::to_string)
        .unwrap_or_default();
    let state = status_field(&status, "State:")
        .map(str::to_string)
        .unwrap_or_default();
    let ppid = parse_i32_field(status_field(&status, "PPid:")).unwrap_or(0);
    let threads = parse_u32_field(status_field(&status, "Threads:")).unwrap_or(0);
    let vm_rss_kb = parse_kb_field(status_field(&status, "VmRSS:")).unwrap_or(0);
    let vm_size_kb = parse_kb_field(status_field(&status, "VmSize:")).unwrap_or(0);
    let cmdline = if include_cmdline {
        std::fs::read(format!("{proc_root}/cmdline"))
            .ok()
            .map(|raw| parse_cmdline_bytes(&raw))
            .filter(|s| !s.is_empty())
            .unwrap_or_default()
    } else {
        String::new()
    };

    Ok(ProcessSnapshot {
        pid,
        ppid,
        name,
        state,
        threads,
        vm_rss_kb,
        vm_size_kb,
        cmdline,
    })
}

#[cfg(not(target_os = "linux"))]
fn inspect_process_linux(_pid: i32, _include_cmdline: bool) -> Result<ProcessSnapshot> {
    Err(Error::config(
        "process_tool",
        "process inspection is only available on Linux",
    ))
}

#[cfg(any(target_os = "linux", test))]
fn status_field<'a>(status: &'a str, key: &str) -> Option<&'a str> {
    status.lines().find_map(|line| {
        line.strip_prefix(key)
            .map(str::trim)
            .filter(|value| !value.is_empty())
    })
}

#[cfg(any(target_os = "linux", test))]
fn parse_i32_field(value: Option<&str>) -> Option<i32> {
    value?.split_whitespace().next()?.parse().ok()
}

#[cfg(any(target_os = "linux", test))]
fn parse_u32_field(value: Option<&str>) -> Option<u32> {
    value?.split_whitespace().next()?.parse().ok()
}

#[cfg(any(target_os = "linux", test))]
fn parse_kb_field(value: Option<&str>) -> Option<u64> {
    value?.split_whitespace().next()?.parse().ok()
}

#[cfg(any(target_os = "linux", test))]
fn parse_cmdline_bytes(raw: &[u8]) -> String {
    raw.split(|b| *b == 0)
        .filter(|part| !part.is_empty())
        .map(|part| String::from_utf8_lossy(part).into_owned())
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::{
        parse_cmdline_bytes, parse_i32_field, parse_kb_field, parse_u32_field, status_field,
    };

    #[test]
    fn parse_cmdline_collapses_null_delimiters() {
        let raw = b"/usr/bin/python\0script.py\0--flag\0";
        assert_eq!(parse_cmdline_bytes(raw), "/usr/bin/python script.py --flag");
    }

    #[test]
    fn parse_status_fields_extracts_expected_values() {
        let sample = "\
Name:\tbash\n\
State:\tS (sleeping)\n\
PPid:\t1\n\
Threads:\t4\n\
VmRSS:\t1234 kB\n\
VmSize:\t4321 kB\n";
        assert_eq!(status_field(sample, "Name:"), Some("bash"));
        assert_eq!(status_field(sample, "State:"), Some("S (sleeping)"));
        assert_eq!(parse_i32_field(status_field(sample, "PPid:")), Some(1));
        assert_eq!(parse_u32_field(status_field(sample, "Threads:")), Some(4));
        assert_eq!(parse_kb_field(status_field(sample, "VmRSS:")), Some(1234));
        assert_eq!(parse_kb_field(status_field(sample, "VmSize:")), Some(4321));
    }
}
