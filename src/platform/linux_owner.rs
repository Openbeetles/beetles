//! Linux 启动占用者检查：用于识别 TCP/UDP 端口当前是谁占着，
//! 并区分是否是 Beetle 自己，避免重复启动时只看到黑盒 `Address already in use`。

use std::collections::HashMap;
use std::fmt::Write as _;
use std::net::{IpAddr, SocketAddr, ToSocketAddrs};
use std::path::{Path, PathBuf};

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LinuxSocketProtocol {
    Tcp,
    Udp,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LinuxSocketOwner {
    pub pid: u32,
    pub command: String,
    pub exe: Option<PathBuf>,
    pub protocol: LinuxSocketProtocol,
    pub port: u16,
}

impl LinuxSocketOwner {
    pub fn is_beetle(&self) -> bool {
        self.command == "beetle"
            || self
                .exe
                .as_ref()
                .and_then(|path| path.file_name())
                .is_some_and(|name| name == "beetle")
    }

    pub fn summary(&self) -> String {
        let mut rendered = format!("pid={} command={}", self.pid, self.command);
        if let Some(exe) = &self.exe {
            let _ = write!(rendered, " exe={}", exe.display());
        }
        rendered
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LinuxProcessIdentity {
    pub pid: u32,
    pub command: String,
    pub exe: Option<PathBuf>,
    pub args: Vec<String>,
}

impl LinuxProcessIdentity {
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    pub fn summary(&self) -> String {
        let mut rendered = format!("pid={} command={}", self.pid, self.command);
        if let Some(exe) = &self.exe {
            let _ = write!(rendered, " exe={}", exe.display());
        }
        if !self.args.is_empty() {
            let _ = write!(rendered, " cmdline={}", self.args.join(" "));
        }
        rendered
    }

    fn is_beetle_binary(&self) -> bool {
        self.command == "beetle"
            || self
                .exe
                .as_ref()
                .and_then(|path| path.file_name())
                .is_some_and(|name| name == "beetle")
            || self
                .args
                .first()
                .and_then(|arg| Path::new(arg).file_name())
                .is_some_and(|name| name == "beetle")
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ProcSocketEntry {
    local_ip: IpAddr,
    port: u16,
    inode: u64,
    listening: bool,
}

pub(crate) fn resolve_listener_owners(
    listen_addr: &str,
    protocol: LinuxSocketProtocol,
) -> std::io::Result<Vec<LinuxSocketOwner>> {
    let addrs = normalize_listener_addrs(listen_addr)?;
    let socket_entries = read_proc_socket_entries(protocol)?;
    let mut inode_to_owner = HashMap::new();
    let mut matches = Vec::new();

    for entry in socket_entries {
        if !entry.listening {
            continue;
        }
        if !listener_matches_any_addr(&entry, &addrs) {
            continue;
        }
        let owner = inode_to_owner
            .entry(entry.inode)
            .or_insert_with(|| resolve_socket_owner(entry.inode, protocol, entry.port))
            .clone();
        if let Some(owner) = owner {
            matches.push(owner);
        }
    }

    matches.sort_by_key(|owner| owner.pid);
    matches.dedup_by_key(|owner| owner.pid);
    Ok(matches)
}

pub(crate) fn describe_listener_owners(
    listen_addr: &str,
    protocol: LinuxSocketProtocol,
) -> std::io::Result<String> {
    let owners = resolve_listener_owners(listen_addr, protocol)?;
    if owners.is_empty() {
        return Ok("unknown owner".to_string());
    }
    Ok(owners
        .iter()
        .map(LinuxSocketOwner::summary)
        .collect::<Vec<_>>()
        .join(", "))
}

pub(crate) fn beetle_listener_running(
    listen_addr: &str,
    protocol: LinuxSocketProtocol,
) -> std::io::Result<bool> {
    Ok(resolve_listener_owners(listen_addr, protocol)?
        .iter()
        .any(LinuxSocketOwner::is_beetle))
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) fn other_beetle_run_processes() -> std::io::Result<Vec<LinuxProcessIdentity>> {
    let current_pid = std::process::id();
    let mut matches = Vec::new();
    for pid in proc_pids()? {
        if pid == current_pid {
            continue;
        }
        let Some(identity) = read_process_identity(pid) else {
            continue;
        };
        if is_beetle_run_process(&identity) {
            matches.push(identity);
        }
    }
    matches.sort_by_key(|identity| identity.pid);
    Ok(matches)
}

pub fn other_beetle_run_summaries() -> std::io::Result<Vec<String>> {
    Ok(other_beetle_run_processes()?
        .into_iter()
        .map(|identity| identity.summary())
        .collect())
}

fn normalize_listener_addrs(listen_addr: &str) -> std::io::Result<Vec<SocketAddr>> {
    let addr = if listen_addr.contains(':') {
        listen_addr.to_string()
    } else {
        format!("0.0.0.0:{listen_addr}")
    };
    addr.to_socket_addrs().map(|iter| iter.collect())
}

fn listener_matches_any_addr(entry: &ProcSocketEntry, addrs: &[SocketAddr]) -> bool {
    addrs
        .iter()
        .any(|listen| listener_matches_addr(entry, *listen))
}

fn listener_matches_addr(entry: &ProcSocketEntry, listen: SocketAddr) -> bool {
    if entry.port != listen.port() {
        return false;
    }
    if entry.local_ip.is_unspecified() {
        return true;
    }
    match (entry.local_ip, listen.ip()) {
        (IpAddr::V4(lhs), IpAddr::V4(rhs)) => lhs == rhs || rhs.is_unspecified(),
        (IpAddr::V6(lhs), IpAddr::V6(rhs)) => lhs == rhs || rhs.is_unspecified(),
        _ => false,
    }
}

fn resolve_socket_owner(
    inode: u64,
    protocol: LinuxSocketProtocol,
    port: u16,
) -> Option<LinuxSocketOwner> {
    let needle = format!("socket:[{inode}]");
    let proc = std::fs::read_dir("/proc").ok()?;
    for entry in proc.flatten() {
        let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() else {
            continue;
        };
        let fd_dir = entry.path().join("fd");
        let Ok(fds) = std::fs::read_dir(fd_dir) else {
            continue;
        };
        for fd in fds.flatten() {
            let Ok(link) = std::fs::read_link(fd.path()) else {
                continue;
            };
            if link.to_string_lossy() != needle {
                continue;
            }
            let identity = read_process_identity(pid)?;
            return Some(LinuxSocketOwner {
                pid,
                command: identity.command,
                exe: identity.exe,
                protocol,
                port,
            });
        }
    }
    None
}

fn read_proc_socket_entries(
    protocol: LinuxSocketProtocol,
) -> std::io::Result<Vec<ProcSocketEntry>> {
    let mut entries = read_proc_table(protocol_table_path(protocol, false), false)?;
    entries.extend(read_proc_table(protocol_table_path(protocol, true), true)?);
    Ok(entries)
}

fn protocol_table_path(protocol: LinuxSocketProtocol, ipv6: bool) -> &'static str {
    match (protocol, ipv6) {
        (LinuxSocketProtocol::Tcp, false) => "/proc/net/tcp",
        (LinuxSocketProtocol::Tcp, true) => "/proc/net/tcp6",
        (LinuxSocketProtocol::Udp, false) => "/proc/net/udp",
        (LinuxSocketProtocol::Udp, true) => "/proc/net/udp6",
    }
}

fn read_proc_table(path: &str, ipv6: bool) -> std::io::Result<Vec<ProcSocketEntry>> {
    let mut entries = Vec::new();
    let content = std::fs::read_to_string(path)?;
    for line in content.lines().skip(1) {
        if let Some(entry) = parse_proc_socket_line(line, ipv6) {
            entries.push(entry);
        }
    }
    Ok(entries)
}

fn parse_proc_socket_line(line: &str, ipv6: bool) -> Option<ProcSocketEntry> {
    let columns = line.split_whitespace().collect::<Vec<_>>();
    if columns.len() < 10 {
        return None;
    }
    let local = columns[1];
    let state = columns[3];
    let inode = columns[9].parse::<u64>().ok()?;
    let (local_ip, port) = parse_proc_local_addr(local, ipv6)?;
    Some(ProcSocketEntry {
        local_ip,
        port,
        inode,
        listening: matches!(state, "0A" | "07"),
    })
}

fn parse_proc_local_addr(raw: &str, ipv6: bool) -> Option<(IpAddr, u16)> {
    let (addr_hex, port_hex) = raw.split_once(':')?;
    let port = u16::from_str_radix(port_hex, 16).ok()?;
    let ip = if ipv6 {
        parse_ipv6_addr(addr_hex)?
    } else {
        parse_ipv4_addr(addr_hex)?
    };
    Some((ip, port))
}

fn parse_ipv4_addr(raw: &str) -> Option<IpAddr> {
    if raw.len() != 8 {
        return None;
    }
    let bytes = (0..4)
        .map(|idx| u8::from_str_radix(&raw[idx * 2..idx * 2 + 2], 16).ok())
        .collect::<Option<Vec<_>>>()?;
    Some(IpAddr::V4(std::net::Ipv4Addr::new(
        bytes[3], bytes[2], bytes[1], bytes[0],
    )))
}

fn parse_ipv6_addr(raw: &str) -> Option<IpAddr> {
    if raw.len() != 32 {
        return None;
    }
    let mut bytes = [0u8; 16];
    for idx in 0..16 {
        bytes[idx] = u8::from_str_radix(&raw[idx * 2..idx * 2 + 2], 16).ok()?;
    }
    Some(IpAddr::V6(std::net::Ipv6Addr::from(bytes)))
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn proc_pids() -> std::io::Result<Vec<u32>> {
    let mut pids = Vec::new();
    for entry in std::fs::read_dir("/proc")? {
        let entry = entry?;
        let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() else {
            continue;
        };
        pids.push(pid);
    }
    Ok(pids)
}

fn read_process_identity(pid: u32) -> Option<LinuxProcessIdentity> {
    if pid == 0 || process_state(pid) == Some('Z') {
        return None;
    }
    let proc_dir = Path::new("/proc").join(pid.to_string());
    let command = std::fs::read_to_string(proc_dir.join("comm"))
        .ok()
        .map(|raw| raw.trim().to_string())
        .filter(|raw| !raw.is_empty())
        .unwrap_or_else(|| "unknown".to_string());
    let exe = std::fs::read_link(proc_dir.join("exe")).ok();
    let args = read_process_cmdline(proc_dir.join("cmdline").as_path())?;
    Some(LinuxProcessIdentity {
        pid,
        command,
        exe,
        args,
    })
}

fn read_process_cmdline(path: &Path) -> Option<Vec<String>> {
    let raw = std::fs::read(path).ok()?;
    Some(
        raw.split(|byte| *byte == 0)
            .filter(|segment| !segment.is_empty())
            .map(|segment| String::from_utf8_lossy(segment).into_owned())
            .collect(),
    )
}

fn process_state(pid: u32) -> Option<char> {
    let stat =
        std::fs::read_to_string(Path::new("/proc").join(pid.to_string()).join("stat")).ok()?;
    let (_, rest) = stat.rsplit_once(") ")?;
    rest.chars().next()
}

fn is_beetle_run_process(identity: &LinuxProcessIdentity) -> bool {
    identity.is_beetle_binary()
        && identity
            .args
            .get(1)
            .is_some_and(|subcommand| subcommand == "run")
}

#[cfg(test)]
mod tests {
    use super::{
        is_beetle_run_process, listener_matches_addr, parse_proc_local_addr,
        parse_proc_socket_line, LinuxProcessIdentity, LinuxSocketProtocol, ProcSocketEntry,
    };
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use std::path::PathBuf;

    #[test]
    fn parse_tcp_ipv4_proc_line_extracts_listener_fields() {
        let entry = parse_proc_socket_line(
            "0: 00000000:0050 00000000:0000 0A 00000000:00000000 00:00000000 00000000 0 0 12345 1 0000000000000000 100 0 0 10 0",
            false,
        )
        .expect("entry");
        assert_eq!(entry.local_ip, IpAddr::V4(Ipv4Addr::UNSPECIFIED));
        assert_eq!(entry.port, 80);
        assert_eq!(entry.inode, 12345);
        assert!(entry.listening);
    }

    #[test]
    fn parse_udp_proc_line_is_treated_as_bound_socket() {
        let entry = parse_proc_socket_line(
            "12: 00000000:0043 00000000:0000 07 00000000:00000000 00:00000000 00000000 0 0 98765 1 0000000000000000 100 0 0 10 0",
            false,
        )
        .expect("entry");
        assert_eq!(entry.port, 67);
        assert_eq!(entry.inode, 98765);
        assert!(entry.listening);
    }

    #[test]
    fn parse_ipv4_proc_local_addr_decodes_little_endian_host_order() {
        let (ip, port) = parse_proc_local_addr("0100007F:1F90", false).expect("addr");
        assert_eq!(ip, IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)));
        assert_eq!(port, 8080);
    }

    #[test]
    fn wildcard_listener_matches_specific_bind() {
        let entry = ProcSocketEntry {
            local_ip: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            port: 80,
            inode: 1,
            listening: true,
        };
        assert!(listener_matches_addr(
            &entry,
            SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 80)
        ));
    }

    #[test]
    fn exact_listener_matches_same_address_only() {
        let entry = ProcSocketEntry {
            local_ip: IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
            port: 8080,
            inode: 1,
            listening: true,
        };
        assert!(listener_matches_addr(
            &entry,
            SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080)
        ));
        assert!(!listener_matches_addr(
            &entry,
            SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 8081)
        ));
    }

    #[test]
    fn protocol_table_path_matches_transport_families() {
        assert_eq!(
            super::protocol_table_path(LinuxSocketProtocol::Tcp, false),
            "/proc/net/tcp"
        );
        assert_eq!(
            super::protocol_table_path(LinuxSocketProtocol::Udp, true),
            "/proc/net/udp6"
        );
    }

    #[test]
    fn beetle_run_detection_requires_run_subcommand() {
        let run_identity = LinuxProcessIdentity {
            pid: 42,
            command: "beetle".to_string(),
            exe: Some(PathBuf::from("/opt/beetle/current/beetle")),
            args: vec!["beetle".to_string(), "run".to_string()],
        };
        let status_identity = LinuxProcessIdentity {
            pid: 43,
            command: "beetle".to_string(),
            exe: Some(PathBuf::from("/opt/beetle/current/beetle")),
            args: vec!["beetle".to_string(), "status".to_string()],
        };

        assert!(is_beetle_run_process(&run_identity));
        assert!(!is_beetle_run_process(&status_identity));
    }
}
