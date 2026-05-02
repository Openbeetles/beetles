//! SNTP 时间同步：`init_sntp()` 启动后台同步。
//! ESP 侧仍在 WiFi 栈 ready 后启动；Linux 侧允许在 WiFi/bootstrap 失败后继续启动，
//! 以便继承上行、以太网或其它可用链路时也能尽快修复墙钟。
//! ESP-IDF 5.x 使用 esp_netif_sntp API；同步成功后 gettimeofday / SystemTime 自动更新。
//! SNTP time sync: `init_sntp()` starts background sync. ESP still starts it after the
//! WiFi stack is ready, while Linux may start it after a failed WiFi/bootstrap attempt so
//! another usable uplink can still repair the wall clock.

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
use esp_idf_svc::sys;
#[cfg(all(
    not(any(target_arch = "xtensa", target_arch = "riscv32")),
    target_os = "linux"
))]
use std::net::{ToSocketAddrs, UdpSocket};
#[cfg(all(
    not(any(target_arch = "xtensa", target_arch = "riscv32")),
    target_os = "linux"
))]
use std::sync::Once;
#[cfg(all(
    not(any(target_arch = "xtensa", target_arch = "riscv32")),
    target_os = "linux"
))]
use std::time::Duration;

const TAG: &str = "platform::sntp";

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
unsafe extern "C" fn on_sntp_time_sync(tv: *mut sys::timeval) {
    let synced_secs = if tv.is_null() {
        None
    } else {
        u64::try_from(unsafe { (*tv).tv_sec }).ok()
    };
    if synced_secs.is_some_and(crate::platform::time::is_trustworthy_wall_clock_secs)
        || crate::platform::time::wall_clock_is_trustworthy()
    {
        crate::platform::time::notify_wall_clock_trustworthy();
    }
}

#[cfg(any(test, target_arch = "xtensa", target_arch = "riscv32"))]
pub(crate) fn should_restart_sntp_after_sta_connect(
    memory_system_kind: crate::memory::MemorySystemKind,
    was_connected: bool,
    sta_ip_ok: bool,
    wall_clock_trustworthy: bool,
) -> bool {
    memory_system_kind == crate::memory::MemorySystemKind::EspCompact
        && sta_ip_ok
        && !was_connected
        && !wall_clock_trustworthy
}

/// 启动 SNTP 后台同步（非阻塞）；ESP 在 WiFi 栈 ready 后调用一次即可。
/// ESP-IDF 5.x 使用 esp_sntp_setoperatingmode + esp_sntp_setservername + esp_sntp_init。
/// 同步成功后系统时钟自动更新，std::time::SystemTime 即为正确 UTC 时间。
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
pub fn init_sntp() {
    unsafe {
        // 停止已有实例（幂等）。
        sys::esp_sntp_stop();

        sys::esp_sntp_setoperatingmode(sys::esp_sntp_operatingmode_t_ESP_SNTP_OPMODE_POLL);

        // 设置 NTP 服务器（最多 CONFIG_LWIP_SNTP_MAX_SERVERS 个，默认 1）。
        let server = b"pool.ntp.org\0";
        sys::esp_sntp_setservername(0, server.as_ptr() as *const _);

        sys::sntp_set_time_sync_notification_cb(Some(on_sntp_time_sync));
        sys::esp_sntp_init();
    }
    log::info!(
        "[{}] SNTP started (pool.ntp.org), time will sync in background",
        TAG
    );
}

#[cfg(all(
    not(any(target_arch = "xtensa", target_arch = "riscv32")),
    target_os = "linux"
))]
static SNTP_ONCE: Once = Once::new();

#[cfg(all(
    not(any(target_arch = "xtensa", target_arch = "riscv32")),
    target_os = "linux"
))]
const NTP_UNIX_OFFSET_SECS: u64 = 2_208_988_800;
#[cfg(all(
    not(any(target_arch = "xtensa", target_arch = "riscv32")),
    target_os = "linux"
))]
const SNTP_RETRY_SECS: u64 = 5;
#[cfg(all(
    not(any(target_arch = "xtensa", target_arch = "riscv32")),
    target_os = "linux"
))]
const SNTP_QUERY_TIMEOUT_SECS: u64 = 5;
/// 每 6 小时重新同步一次，防止长时间运行后漂移。
#[cfg(all(
    not(any(target_arch = "xtensa", target_arch = "riscv32")),
    target_os = "linux"
))]
const SNTP_RESYNC_SECS: u64 = 6 * 3600;

#[cfg(all(
    not(any(target_arch = "xtensa", target_arch = "riscv32")),
    target_os = "linux"
))]
pub fn init_sntp() {
    SNTP_ONCE.call_once(|| {
        crate::util::spawn_guarded_with_stack("sntp", crate::util::STACK_SNTP_WORKER, || {
            log::info!("[{}] Linux SNTP background sync started", TAG);

            // 先判断系统时间是否已正确（RTC 或 NTP 已设置过）。
            if crate::platform::time::wall_clock_is_trustworthy() {
                log::info!("[{}] system time already looks valid (>2023)", TAG);
                crate::platform::time::notify_wall_clock_trustworthy();
            } else {
                // 不再严格等待 STA：先快速尝试几次（可能有有线网络、其它连接方式）。
                let mut synced = false;
                for attempt in 0u32..60 {
                    if crate::platform::time::wall_clock_is_trustworthy() {
                        log::info!("[{}] system time became valid during wait", TAG);
                        crate::platform::time::notify_wall_clock_trustworthy();
                        synced = true;
                        break;
                    }
                    match sync_once() {
                        Ok(epoch) => {
                            log::info!(
                                "[{}] SNTP synchronized on attempt {}, unix={}",
                                TAG,
                                attempt + 1,
                                epoch
                            );
                            crate::platform::time::notify_wall_clock_trustworthy();
                            synced = true;
                            break;
                        }
                        Err(_) => {
                            std::thread::sleep(Duration::from_secs(SNTP_RETRY_SECS));
                        }
                    }
                }
                if !synced {
                    log::warn!("[{}] SNTP initial sync failed after 60 attempts; will keep retrying in background", TAG);
                }
            }

            // 周期重同步：已同步则 6h 间隔；未同步则 30s 快轮询直到成功。
            let mut ever_synced = crate::platform::time::wall_clock_is_trustworthy();
            loop {
                let interval = if ever_synced {
                    SNTP_RESYNC_SECS
                } else {
                    30
                };
                std::thread::sleep(Duration::from_secs(interval));
                match sync_once() {
                    Ok(epoch) => {
                        if !ever_synced {
                            log::info!("[{}] SNTP late sync ok, unix={}", TAG, epoch);
                        }
                        crate::platform::time::notify_wall_clock_trustworthy();
                        ever_synced = true;
                    }
                    Err(e) => {
                        if !ever_synced {
                            log::debug!("[{}] SNTP retry failed: {}", TAG, e);
                        } else {
                            log::warn!("[{}] SNTP resync failed: {}", TAG, e);
                        }
                    }
                }
            }
        });
    });
}

#[cfg(all(
    not(any(target_arch = "xtensa", target_arch = "riscv32")),
    target_os = "linux"
))]
fn sync_once() -> crate::error::Result<u64> {
    let epoch = query_ntp_unix_secs()?;
    set_system_time(epoch)?;
    Ok(epoch)
}

#[cfg(all(
    not(any(target_arch = "xtensa", target_arch = "riscv32")),
    target_os = "linux"
))]
fn query_ntp_unix_secs() -> crate::error::Result<u64> {
    for server in ["pool.ntp.org:123", "time.google.com:123"] {
        let addrs = match server.to_socket_addrs() {
            Ok(v) => v.collect::<Vec<_>>(),
            Err(e) => {
                log::debug!("[{}] resolve {} failed: {}", TAG, server, e);
                continue;
            }
        };
        for addr in addrs {
            let socket = UdpSocket::bind("0.0.0.0:0")
                .map_err(|e| crate::error::Error::io("sntp_udp_bind", e))?;
            socket
                .set_read_timeout(Some(Duration::from_secs(SNTP_QUERY_TIMEOUT_SECS)))
                .map_err(|e| crate::error::Error::io("sntp_udp_timeout", e))?;
            socket
                .set_write_timeout(Some(Duration::from_secs(SNTP_QUERY_TIMEOUT_SECS)))
                .map_err(|e| crate::error::Error::io("sntp_udp_timeout", e))?;

            let mut req = [0u8; 48];
            req[0] = 0x1b; // LI=0, VN=3, Mode=3 (client)
            if let Err(e) = socket.send_to(&req, addr) {
                log::debug!("[{}] send {} failed: {}", TAG, addr, e);
                continue;
            }

            let mut resp = [0u8; 48];
            let Ok((n, _)) = socket.recv_from(&mut resp) else {
                continue;
            };
            if n < 48 {
                continue;
            }
            let secs = u32::from_be_bytes([resp[40], resp[41], resp[42], resp[43]]) as u64;
            if secs <= NTP_UNIX_OFFSET_SECS {
                continue;
            }
            return Ok(secs - NTP_UNIX_OFFSET_SECS);
        }
    }
    Err(crate::error::Error::config(
        "sntp_query",
        "all NTP servers failed",
    ))
}

#[cfg(all(
    not(any(target_arch = "xtensa", target_arch = "riscv32")),
    target_os = "linux"
))]
fn set_system_time(epoch_secs: u64) -> crate::error::Result<()> {
    let ts = libc::timespec {
        tv_sec: epoch_secs as _,
        tv_nsec: 0,
    };
    let rc = unsafe { libc::clock_settime(libc::CLOCK_REALTIME, &ts) };
    if rc != 0 {
        return Err(crate::error::Error::io(
            "sntp_settime",
            std::io::Error::last_os_error(),
        ));
    }
    Ok(())
}

#[cfg(all(
    not(any(target_arch = "xtensa", target_arch = "riscv32")),
    not(target_os = "linux")
))]
pub fn init_sntp() {
    log::info!("[{}] SNTP no-op on non-Linux host", TAG);
}

#[cfg(test)]
mod tests {
    use crate::memory::MemorySystemKind;

    #[test]
    fn esp_restarts_sntp_when_sta_first_gets_ip_before_wall_clock_is_valid() {
        assert!(super::should_restart_sntp_after_sta_connect(
            MemorySystemKind::EspCompact,
            false,
            true,
            false,
        ));
        assert!(!super::should_restart_sntp_after_sta_connect(
            MemorySystemKind::EspCompact,
            true,
            true,
            false,
        ));
        assert!(!super::should_restart_sntp_after_sta_connect(
            MemorySystemKind::EspCompact,
            false,
            true,
            true,
        ));
        assert!(!super::should_restart_sntp_after_sta_connect(
            MemorySystemKind::LinuxFull,
            false,
            true,
            false,
        ));
    }
}
