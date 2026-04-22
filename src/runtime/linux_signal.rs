//! Linux runtime signal bridge for graceful Beetle shutdown.

use crate::error::{Error, Result};
use crate::Platform;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::Arc;
use std::time::Duration;

static SIGNAL_BRIDGE_INSTALLED: AtomicBool = AtomicBool::new(false);
static PENDING_SIGNAL: AtomicI32 = AtomicI32::new(0);

const SIGNAL_POLL_INTERVAL: Duration = Duration::from_millis(100);

/// Install Linux SIGTERM/SIGINT handlers that flush continuity before exiting.
pub fn install_linux_signal_bridge(platform: Arc<dyn Platform>) -> Result<()> {
    if SIGNAL_BRIDGE_INSTALLED.swap(true, Ordering::SeqCst) {
        return Ok(());
    }
    install_signal_handler(libc::SIGTERM)?;
    install_signal_handler(libc::SIGINT)?;
    std::thread::Builder::new()
        .name("linux_signal_bridge".to_string())
        .spawn(move || signal_bridge_loop(platform))
        .map_err(|error| Error::io("linux_signal_spawn", error))?;
    Ok(())
}

fn signal_bridge_loop(platform: Arc<dyn Platform>) {
    loop {
        let signal = PENDING_SIGNAL.swap(0, Ordering::SeqCst);
        if signal != 0 {
            handle_shutdown_signal(platform.as_ref(), signal);
            return;
        }
        std::thread::sleep(SIGNAL_POLL_INTERVAL);
    }
}

fn handle_shutdown_signal(platform: &dyn Platform, signal: i32) {
    let reason = shutdown_reason_for_signal(signal);
    let now_secs = crate::util::current_unix_secs();
    match crate::runtime::flush_reboot_continuity_bundle(platform, None, reason, now_secs) {
        Ok(count) => {
            log::info!(
                "[linux_signal] graceful shutdown reason={} snapshots={}",
                reason,
                count
            );
        }
        Err(error) => {
            log::warn!(
                "[linux_signal] graceful shutdown flush failed reason={}: {}",
                reason,
                error
            );
        }
    }
    std::process::exit(0);
}

fn shutdown_reason_for_signal(signal: i32) -> &'static str {
    match signal {
        libc::SIGINT => "signal_sigint",
        libc::SIGTERM => "signal_sigterm",
        _ => "signal_shutdown",
    }
}

fn install_signal_handler(signal: libc::c_int) -> Result<()> {
    let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
    action.sa_flags = 0;
    action.sa_sigaction = linux_runtime_signal_handler as *const () as usize;
    unsafe {
        libc::sigemptyset(&mut action.sa_mask);
    }
    let rc = unsafe { libc::sigaction(signal, &action, std::ptr::null_mut()) };
    if rc != 0 {
        return Err(Error::io(
            "linux_signal_install",
            std::io::Error::last_os_error(),
        ));
    }
    Ok(())
}

extern "C" fn linux_runtime_signal_handler(signal: libc::c_int) {
    let _ = PENDING_SIGNAL.compare_exchange(0, signal, Ordering::SeqCst, Ordering::SeqCst);
}

#[cfg(test)]
mod tests {
    use super::shutdown_reason_for_signal;

    #[test]
    fn shutdown_reason_maps_known_signals() {
        assert_eq!(shutdown_reason_for_signal(libc::SIGTERM), "signal_sigterm");
        assert_eq!(shutdown_reason_for_signal(libc::SIGINT), "signal_sigint");
        assert_eq!(shutdown_reason_for_signal(libc::SIGHUP), "signal_shutdown");
    }
}
