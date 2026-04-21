//! HTTP config listen binding helpers.
//!
//! The startup path binds the listener here before `tiny_http` takes ownership,
//! so duplicate Beetle starts fail synchronously instead of racing with the
//! worker thread spawn.

use crate::error::{Error, Result};
use crate::platform::linux_owner::{self, LinuxSocketProtocol};
use std::io::ErrorKind;
use std::net::{SocketAddr, TcpListener, ToSocketAddrs};

#[derive(Clone, Debug, PartialEq, Eq)]
enum ListenProbe {
    BeetleRunning(String),
    Occupied(String),
    Unknown(String),
}

pub fn bind_tcp_listener(listen_addr: &str, stage: &'static str) -> Result<TcpListener> {
    let socket_addr = resolve_socket_addr(listen_addr, stage)?;
    match TcpListener::bind(socket_addr) {
        Ok(listener) => Ok(listener),
        Err(error) if error.kind() == ErrorKind::AddrInUse => {
            let probe = probe_listen_owner(listen_addr);
            Err(build_bind_conflict_error(
                stage,
                listen_addr,
                probe,
                error.to_string(),
            ))
        }
        Err(error) => Err(Error::io(stage, error)),
    }
}

fn resolve_socket_addr(listen_addr: &str, stage: &'static str) -> Result<SocketAddr> {
    let mut addrs = listen_addr.to_socket_addrs().map_err(|error| {
        Error::config(
            stage,
            format!("invalid listen address {}: {}", listen_addr, error),
        )
    })?;
    addrs.next().ok_or_else(|| {
        Error::config(
            stage,
            format!(
                "invalid listen address {}: resolved to no socket addresses",
                listen_addr
            ),
        )
    })
}

fn probe_listen_owner(listen_addr: &str) -> ListenProbe {
    let beetle_running =
        linux_owner::beetle_listener_running(listen_addr, LinuxSocketProtocol::Tcp)
            .unwrap_or(false);
    match linux_owner::resolve_listener_owners(listen_addr, LinuxSocketProtocol::Tcp) {
        Ok(owners) => {
            if beetle_running {
                if let Some(owner) = owners.iter().find(|owner| owner.is_beetle()).cloned() {
                    return ListenProbe::BeetleRunning(owner.summary());
                }
            }
            if let Some(owner) = owners.into_iter().next() {
                return ListenProbe::Occupied(owner.summary());
            }
            ListenProbe::Unknown(format!(
                "owner lookup found no TCP listener for {}",
                listen_addr
            ))
        }
        Err(error) => ListenProbe::Unknown(format!("owner lookup failed: {}", error)),
    }
}

fn build_bind_conflict_error(
    stage: &'static str,
    listen_addr: &str,
    probe: ListenProbe,
    bind_error: String,
) -> Error {
    match probe {
        ListenProbe::BeetleRunning(summary) => Error::config(
            stage,
            format!(
                "beetle HTTP config API already running on {} ({})",
                listen_addr, summary
            ),
        ),
        ListenProbe::Occupied(summary) => Error::config(
            stage,
            format!(
                "beetle HTTP config API listen address {} is already occupied by {}",
                listen_addr, summary
            ),
        ),
        ListenProbe::Unknown(detail) => Error::config(
            stage,
            match linux_owner::describe_listener_owners(listen_addr, LinuxSocketProtocol::Tcp)
                .ok()
                .filter(|summary| summary != "unknown owner")
            {
                Some(summary) => format!(
                    "beetle HTTP config API listen address {} is already in use, but owner lookup was unavailable ({detail}); current owners: {}; underlying bind error: {bind_error}",
                    listen_addr,
                    summary
                ),
                None => format!(
                    "beetle HTTP config API listen address {} is already in use, but owner lookup was unavailable ({detail}); underlying bind error: {bind_error}",
                    listen_addr
                ),
            },
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::linux_owner::{LinuxSocketOwner, LinuxSocketProtocol};

    #[test]
    fn bind_conflict_for_beetle_becomes_already_running() {
        let owner = LinuxSocketOwner {
            pid: 7,
            command: "beetle".to_string(),
            exe: Some(std::path::PathBuf::from("/usr/local/bin/beetle")),
            protocol: LinuxSocketProtocol::Tcp,
            port: 80,
        };
        let err = build_bind_conflict_error(
            "http_config_listen",
            "0.0.0.0:80",
            ListenProbe::BeetleRunning(owner.summary()),
            "Address already in use".to_string(),
        );
        let Error::Config { message, .. } = err else {
            panic!("expected config error");
        };
        assert!(message.contains("already running"));
        assert!(message.contains("pid=7"));
    }

    #[test]
    fn bind_conflict_for_external_owner_reports_occupant() {
        let owner = LinuxSocketOwner {
            pid: 99,
            command: "python".to_string(),
            exe: Some(std::path::PathBuf::from("/usr/bin/python")),
            protocol: LinuxSocketProtocol::Tcp,
            port: 8080,
        };
        let err = build_bind_conflict_error(
            "http_config_listen",
            "127.0.0.1:8080",
            ListenProbe::Occupied(owner.summary()),
            "Address already in use".to_string(),
        );
        let Error::Config { message, .. } = err else {
            panic!("expected config error");
        };
        assert!(message.contains("already occupied"));
        assert!(message.contains("pid=99"));
        assert!(message.contains("python"));
    }

    #[test]
    fn resolves_socket_addr_from_listen_string() {
        let addr = resolve_socket_addr("0.0.0.0:80", "http_config_listen").unwrap();
        assert_eq!(addr.port(), 80);
    }
}
