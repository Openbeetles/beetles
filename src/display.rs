//! Display configuration and command types.
//! 显示配置与指令模型（平台无关纯数据）。

use crate::channel_catalog::DISPLAY_CHANNEL_CAPACITY;
use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU32, Ordering};

pub const DISPLAY_CONFIG_VERSION: u32 = 1;
pub const DISPLAY_DIM_MIN: u16 = 1;
pub const DISPLAY_DIM_MAX: u16 = 480;
pub const DISPLAY_OFFSET_MIN: i16 = -480;
pub const DISPLAY_OFFSET_MAX: i16 = 480;
pub const DISPLAY_SPI_FREQ_MIN: u32 = 1_000_000;
pub const DISPLAY_SPI_FREQ_MAX: u32 = 80_000_000;
/// 参考布局坐标基准（240x240 设计网格）。
/// Layout reference grid baseline (240x240 design space).
pub const DISPLAY_LAYOUT_REF_PX: u32 = 240;
const DISPLAY_SECTION_DIVIDER_GAP_PX: u16 = 6;
const DISPLAY_HEADER_ICON_BOTTOM_BREATHING_PX: u16 = 8;
pub const DISPLAY_LEASE_TTL_MS: u64 = 3_000;

static DISPLAY_LEASE_DENIED_TOTAL: AtomicU32 = AtomicU32::new(0);

/// Logical owner for the display hardware lease.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DisplayOwner {
    DefaultDashboard,
    ConfigUi,
    Voice,
    Diagnostic,
    Script,
}

impl DisplayOwner {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DefaultDashboard => "default_dashboard",
            Self::ConfigUi => "config_ui",
            Self::Voice => "voice",
            Self::Diagnostic => "diagnostic",
            Self::Script => "script",
        }
    }

    pub const fn lease_owner(self) -> crate::runtime::lease::LeaseOwner {
        crate::runtime::lease::LeaseOwner::new("display", self.as_str())
    }
}

/// RAII guard for a held Display lease.
#[must_use]
pub struct DisplayLeaseGuard {
    owner: DisplayOwner,
    token: u64,
}

impl DisplayLeaseGuard {
    pub fn owner(&self) -> DisplayOwner {
        self.owner
    }
}

impl Drop for DisplayLeaseGuard {
    fn drop(&mut self) {
        let _ = crate::runtime::lease::release_token(
            crate::runtime::lease::LeaseKind::Display,
            self.owner.lease_owner(),
            self.token,
        );
    }
}

/// Try to acquire the display lease for the default display deadline.
pub fn try_acquire_display_lease(owner: DisplayOwner) -> Option<DisplayLeaseGuard> {
    try_acquire_display_lease_with_ttl(owner, Some(DISPLAY_LEASE_TTL_MS))
}

/// Try to acquire the display lease with a custom deadline.
pub fn try_acquire_display_lease_with_ttl(
    owner: DisplayOwner,
    ttl_ms: Option<u64>,
) -> Option<DisplayLeaseGuard> {
    try_acquire_display_lease_inner(owner, ttl_ms, None)
}

#[cfg(test)]
pub(crate) fn try_acquire_display_lease_at(
    owner: DisplayOwner,
    ttl_ms: Option<u64>,
    now_ms: u64,
) -> Option<DisplayLeaseGuard> {
    try_acquire_display_lease_inner(owner, ttl_ms, Some(now_ms))
}

fn try_acquire_display_lease_inner(
    owner: DisplayOwner,
    ttl_ms: Option<u64>,
    now_ms: Option<u64>,
) -> Option<DisplayLeaseGuard> {
    let decision = match now_ms {
        Some(now_ms) => crate::runtime::lease::try_acquire_at(
            crate::runtime::lease::LeaseKind::Display,
            owner.lease_owner(),
            crate::runtime::lease::LeaseMode::Exclusive,
            ttl_ms,
            now_ms,
        ),
        None => crate::runtime::lease::try_acquire(
            crate::runtime::lease::LeaseKind::Display,
            owner.lease_owner(),
            crate::runtime::lease::LeaseMode::Exclusive,
            ttl_ms,
        ),
    };

    match decision {
        crate::runtime::lease::LeaseDecision::Acquired(record)
        | crate::runtime::lease::LeaseDecision::Reentered(record)
        | crate::runtime::lease::LeaseDecision::ReplacedExpired {
            current: record, ..
        } => Some(DisplayLeaseGuard {
            owner,
            token: record.token,
        }),
        crate::runtime::lease::LeaseDecision::Denied(denial) => {
            DISPLAY_LEASE_DENIED_TOTAL.fetch_add(1, Ordering::Relaxed);
            log::debug!(
                "[display] display lease denied owner={} reason={} held_by={:?}",
                owner.as_str(),
                denial.reason,
                denial.held_by
            );
            None
        }
    }
}

/// Run a display operation only while the caller owns the Display lease.
pub fn with_display_lease<T, F>(owner: DisplayOwner, operation: F) -> Option<T>
where
    F: FnOnce() -> T,
{
    let _lease = try_acquire_display_lease(owner)?;
    Some(operation())
}

pub fn display_lease_denied_total() -> u64 {
    DISPLAY_LEASE_DENIED_TOTAL.load(Ordering::Relaxed) as u64
}

pub fn format_display_lease_baseline_log_line() -> String {
    format!(
        "display_lease denied_total={}",
        display_lease_denied_total()
    )
}

#[cfg(test)]
pub(crate) fn reset_display_lease_denied_total_for_tests() {
    DISPLAY_LEASE_DENIED_TOTAL.store(0, Ordering::Relaxed);
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DisplayDriver {
    St7789,
    Ili9341,
    /// Linux framebuffer 驱动（通过 /dev/fb0 等 fbdev 接口）。
    /// Linux framebuffer driver via /dev/fbX fbdev interface.
    Framebuffer,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DisplayBus {
    Spi,
    /// Linux framebuffer 总线（与 DisplayDriver::Framebuffer 配对）。
    Framebuffer,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DisplayColorOrder {
    Rgb,
    Bgr,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "PascalCase")]
pub enum DisplayPressureLevel {
    Normal,
    Cautious,
    Critical,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DisplaySpiConfig {
    #[serde(default = "default_spi_host")]
    pub host: u8,
    pub sclk: i32,
    pub mosi: i32,
    pub cs: i32,
    pub dc: i32,
    #[serde(default)]
    pub rst: Option<i32>,
    #[serde(default)]
    pub rst_active_high: bool,
    #[serde(default)]
    pub bl: Option<i32>,
    #[serde(default = "default_spi_freq_hz")]
    pub freq_hz: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DisplayConfig {
    #[serde(default = "default_config_version")]
    pub version: u32,
    pub enabled: bool,
    pub driver: DisplayDriver,
    pub bus: DisplayBus,
    pub width: u16,
    pub height: u16,
    #[serde(default = "default_rotation")]
    pub rotation: u16,
    #[serde(default = "default_color_order")]
    pub color_order: DisplayColorOrder,
    #[serde(default)]
    pub invert_colors: bool,
    /// Linux SPI only: swap the two bytes of each RGB565 pixel before sending over SPI.
    /// 仅 Linux SPI 使用：发送前交换每个 RGB565 像素的高低字节。
    #[serde(default)]
    pub linux_spi_swap_bytes: bool,
    #[serde(default)]
    pub offset_x: i16,
    #[serde(default)]
    pub offset_y: i16,
    pub spi: DisplaySpiConfig,
    /// Linux 设备路径：framebuffer 模式下为 `/dev/fbX`，SPI 模式下可填 `/dev/spidevX.Y`。
    /// Linux device path: `/dev/fbX` for framebuffer mode, `/dev/spidevX.Y` for SPI mode.
    #[serde(default = "default_fb_device")]
    pub fb_device: String,
    /// Linux sysfs 背光亮度文件路径，如 /sys/class/backlight/backlight0/brightness。
    /// 为 None 时背光不可控（auto-sleep 不启用）。
    /// Linux sysfs backlight brightness file path. None means no backlight control.
    #[serde(default)]
    pub backlight_sysfs: Option<String>,
    /// 空闲自动熄屏超时（秒）。0 = 禁用。
    /// Auto-sleep timeout in seconds. 0 = disabled.
    #[serde(default)]
    pub sleep_timeout_secs: u16,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DisplayChannelRuntimeStatus {
    Disabled,
    Configured,
    Waiting,
    WaitingWallClock,
    Suspended,
    Connecting,
    Online,
    CoolingDown,
    Failed,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
pub struct DisplayChannelStatus {
    pub name: &'static str,
    pub display_label: &'static str,
    pub visible: bool,
    pub enabled: bool,
    pub healthy: bool,
    pub runtime_status: DisplayChannelRuntimeStatus,
    /// 连续失败次数（F5: 通道失败计数）。
    pub consecutive_failures: u32,
}

impl DisplayChannelStatus {
    pub const fn hidden() -> Self {
        Self {
            name: "",
            display_label: "",
            visible: false,
            enabled: false,
            healthy: false,
            runtime_status: DisplayChannelRuntimeStatus::Disabled,
            consecutive_failures: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum DisplaySystemState {
    Booting,
    Pairing,
    Recovery,
    NoWifi,
    Idle,
    Busy,
    Fault,
    /// 麦克风录音中（voice_input 工具采集时）。
    Recording,
    /// 喇叭播放中（voice_output 工具播放时）。
    Playing,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct DisplayLayout {
    pub header_top: u16,
    pub icon_left: u16,
    pub icon_size: u16,
    pub title_left: u16,
    pub title_top: u16,
    pub subtitle_top: u16,
    pub middle_top: u16,
    pub footer_top: u16,
    /// 水平边距（参考坐标 240，结合宽高比分档后的布局）。
    /// Horizontal margin scaled from reference 240 grid with aspect-bucket layout.
    pub margin_x: u16,
}

#[derive(Clone, Debug, Serialize)]
pub enum DisplayCommand {
    RefreshDashboard {
        state: DisplaySystemState,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        presence_subtitle: Option<String>,
        ip_address: Option<String>,
        channels: [DisplayChannelStatus; DISPLAY_CHANNEL_CAPACITY],
        pressure: DisplayPressureLevel,
        heap_percent: u8,
        messages_in: u32,
        messages_out: u32,
        last_active_epoch_secs: u32,
        /// F3: 系统运行时间（秒）。
        uptime_secs: u64,
        /// F4: Busy 呼吸动画相位。
        busy_phase: bool,
        /// F6: 最近一次 LLM 调用延迟（毫秒），0 表示无数据。
        llm_last_ms: u32,
        /// F7: 错误闪烁标志（本轮有新错误时为 true）。
        error_flash: bool,
    },
    /// 仅副标题 IP 行局部刷新；`uptime_secs` 与宽屏双行 `Up:` 对齐。
    UpdateIp {
        ip: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        presence_subtitle: Option<String>,
        uptime_secs: u64,
    },
    /// 仅头部状态区局部刷新；用于 steady-state 下的 `Idle/Busy/Listen/Speak` 等状态切换，
    /// 避免每次状态变化都重走整屏 dashboard 渲染。
    UpdateStateHeader {
        state: DisplaySystemState,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        presence_subtitle: Option<String>,
        ip_address: Option<String>,
        uptime_secs: u64,
        busy_phase: bool,
    },
    UpdatePressure {
        level: DisplayPressureLevel,
        heap_percent: u8,
        messages_in: u32,
        messages_out: u32,
        last_active_epoch_secs: u32,
        /// F6: 最近一次 LLM 调用延迟（毫秒），0 表示无数据。
        llm_last_ms: u32,
        /// F7: 错误闪烁标志。
        error_flash: bool,
    },
    UpdateChannels {
        channels: [DisplayChannelStatus; DISPLAY_CHANNEL_CAPACITY],
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AspectClass {
    Square,
    PortraitTall,
    LandscapeWide,
}

#[inline]
fn layout_aspect_class(w: u32, h: u32) -> AspectClass {
    if h.saturating_mul(100) > w.saturating_mul(115) {
        AspectClass::PortraitTall
    } else if w.saturating_mul(100) > h.saturating_mul(115) {
        AspectClass::LandscapeWide
    } else {
        AspectClass::Square
    }
}

#[inline]
fn layout_vertical_markers(aspect: AspectClass) -> (u32, u32, u32, u32, u32) {
    match aspect {
        // Keep 240x240 legacy layout unchanged.
        AspectClass::Square => (16, 18, 44, 104, 168),
        // Increase the middle information band on tall portrait panels.
        AspectClass::PortraitTall => (16, 18, 42, 96, 178),
        // Compress vertical occupancy on wide landscape panels.
        AspectClass::LandscapeWide => (14, 16, 36, 72, 132),
    }
}

/// 按 `width`×`height` 计算仪表盘布局：以 240 参考网格并按宽高比分三档（方屏/竖长/横宽）。
/// Computes dashboard layout on a 240-grid with 3 aspect buckets: square/portrait-tall/landscape-wide.
pub fn compute_layout(width: u16, height: u16) -> DisplayLayout {
    let w = width as u32;
    let h = height as u32;
    let dim_min = w.min(h);
    let aspect = layout_aspect_class(w, h);
    let (header_n, title_n, subtitle_n, middle_n, footer_n) = layout_vertical_markers(aspect);

    let header_top = (h * header_n / DISPLAY_LAYOUT_REF_PX) as u16;
    let title_top = (h * title_n / DISPLAY_LAYOUT_REF_PX) as u16;
    let subtitle_top = (h * subtitle_n / DISPLAY_LAYOUT_REF_PX) as u16;
    let mut middle_top = (h * middle_n / DISPLAY_LAYOUT_REF_PX) as u16;
    let footer_top = (h * footer_n / DISPLAY_LAYOUT_REF_PX) as u16;

    let icon_left = (w * 12 / DISPLAY_LAYOUT_REF_PX) as u16;
    let icon_size = (dim_min * 64 / DISPLAY_LAYOUT_REF_PX).max(16) as u16;
    // Keep the beetle size stable; widen the first region when the aspect bucket baseline
    // would otherwise cut through the icon on landscape/wide panels.
    let min_middle_top = header_top
        .saturating_add(icon_size)
        .saturating_add(DISPLAY_HEADER_ICON_BOTTOM_BREATHING_PX)
        .saturating_add(DISPLAY_SECTION_DIVIDER_GAP_PX);
    middle_top = middle_top.max(min_middle_top);
    let gap = icon_left;

    DisplayLayout {
        header_top,
        icon_left,
        icon_size,
        title_left: icon_left.saturating_add(icon_size).saturating_add(gap),
        title_top,
        subtitle_top,
        middle_top,
        footer_top,
        margin_x: ((w * 8 / DISPLAY_LAYOUT_REF_PX).max(2)) as u16,
    }
}

fn default_config_version() -> u32 {
    DISPLAY_CONFIG_VERSION
}

fn default_rotation() -> u16 {
    0
}

fn default_color_order() -> DisplayColorOrder {
    DisplayColorOrder::Rgb
}

fn default_spi_host() -> u8 {
    2
}

fn default_spi_freq_hz() -> u32 {
    40_000_000
}

fn default_fb_device() -> String {
    "/dev/fb0".to_string()
}

pub fn default_disabled_display_config() -> DisplayConfig {
    DisplayConfig {
        version: DISPLAY_CONFIG_VERSION,
        enabled: false,
        driver: DisplayDriver::St7789,
        bus: DisplayBus::Spi,
        width: 240,
        height: 240,
        rotation: 0,
        color_order: DisplayColorOrder::Rgb,
        invert_colors: false,
        linux_spi_swap_bytes: false,
        offset_x: 0,
        offset_y: 0,
        spi: DisplaySpiConfig {
            host: default_spi_host(),
            sclk: 42,
            mosi: 41,
            cs: 21,
            dc: 40,
            rst: None,
            rst_active_high: false,
            bl: None,
            freq_hz: default_spi_freq_hz(),
        },
        fb_device: default_fb_device(),
        backlight_sysfs: None,
        sleep_timeout_secs: 0,
    }
}

/// 判断配置是否使用 Linux framebuffer 后端。
#[inline]
pub fn is_framebuffer_config(cfg: &DisplayConfig) -> bool {
    matches!(
        (&cfg.driver, &cfg.bus),
        (DisplayDriver::Framebuffer, DisplayBus::Framebuffer)
    )
}

/// Map display config SPI host labels to Linux `/dev/spidevX.Y` bus indexes.
/// 将显示配置中的 SPI host 标签映射为 Linux `/dev/spidevX.Y` bus 序号。
pub fn display_spidev_bus_for_config_host(host: u8) -> Result<u8> {
    match host {
        2 => Ok(0),
        3 => Ok(1),
        _ => Err(Error::config(
            "display_spi_path",
            "DISPLAY_CONFIG_INVALID_SPI_HOST: host must be 2 (SPI2) or 3 (SPI3)",
        )),
    }
}

#[cfg(any(test, target_arch = "xtensa", target_arch = "riscv32"))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DisplayLcdRowWindow {
    pub x_start: i32,
    pub y_start: i32,
    pub x_end: i32,
    pub y_end: i32,
    pub row_start_byte: usize,
    pub row_end_byte: usize,
}

/// Compute the row band passed to `esp_lcd_panel_draw_bitmap`.
/// `x_end` / `y_end` are exclusive, matching ESP-IDF `esp_lcd`.
#[cfg(any(test, target_arch = "xtensa", target_arch = "riscv32"))]
pub(crate) fn display_lcd_row_window(
    width: u16,
    height: u16,
    offset_x: i16,
    offset_y: i16,
    ry: u16,
    rh: u16,
) -> Option<DisplayLcdRowWindow> {
    if width == 0 || rh == 0 {
        return None;
    }
    let ry = ry.min(height);
    let rh = rh.min(height.saturating_sub(ry));
    if rh == 0 {
        return None;
    }

    let row_bytes = width as usize * 2;
    let row_start_byte = ry as usize * row_bytes;
    let row_end_byte = row_start_byte + rh as usize * row_bytes;
    let x_start = offset_x.max(0) as i32;
    let y_start = offset_y.max(0) as i32 + ry as i32;

    Some(DisplayLcdRowWindow {
        x_start,
        y_start,
        x_end: x_start + width as i32,
        y_end: y_start + rh as i32,
        row_start_byte,
        row_end_byte,
    })
}

pub fn validate_display_config_core(cfg: &DisplayConfig) -> Result<()> {
    if cfg.version != DISPLAY_CONFIG_VERSION {
        return Err(Error::config(
            "display",
            format!(
                "DISPLAY_CONFIG_INVALID_VERSION: expected {}, got {}",
                DISPLAY_CONFIG_VERSION, cfg.version
            ),
        ));
    }
    if !cfg.enabled {
        return Ok(());
    }
    if !(DISPLAY_DIM_MIN..=DISPLAY_DIM_MAX).contains(&cfg.width)
        || !(DISPLAY_DIM_MIN..=DISPLAY_DIM_MAX).contains(&cfg.height)
    {
        return Err(Error::config(
            "display",
            "DISPLAY_CONFIG_INVALID_DIMENSION: width/height must be 1..=480",
        ));
    }
    // Framebuffer 首版仅支持 rotation=0；MADCTL 软件旋转留 TODO。
    if is_framebuffer_config(cfg) {
        #[cfg(target_os = "linux")]
        if cfg.fb_device.is_empty() || cfg.fb_device.bytes().any(|b| b == 0 || b < 0x20) {
            return Err(Error::config(
                "display",
                "DISPLAY_CONFIG_INVALID_DEVICE_PATH: fb_device must be non-empty and contain no control characters",
            ));
        }
        if cfg.rotation != 0 {
            return Err(Error::config(
                "display",
                "DISPLAY_CONFIG_FRAMEBUFFER_ROTATION: framebuffer mode only supports rotation=0 in this version",
            ));
        }
        // backlight_sysfs 路径安全检查（若有）。
        if let Some(ref bl) = cfg.backlight_sysfs {
            if bl.is_empty() || bl.bytes().any(|b| b == 0 || b < 0x20) {
                return Err(Error::config(
                    "display",
                    "DISPLAY_CONFIG_INVALID_BACKLIGHT_SYSFS: backlight_sysfs path must be non-empty and contain no control characters",
                ));
            }
        }
        return Ok(());
    }

    // SPI 驱动校验（仅 driver/bus != Framebuffer 时执行）。
    if !matches!(cfg.rotation, 0 | 90 | 180 | 270) {
        return Err(Error::config(
            "display",
            "DISPLAY_CONFIG_INVALID_ROTATION: must be one of 0/90/180/270",
        ));
    }
    if !(DISPLAY_OFFSET_MIN..=DISPLAY_OFFSET_MAX).contains(&cfg.offset_x)
        || !(DISPLAY_OFFSET_MIN..=DISPLAY_OFFSET_MAX).contains(&cfg.offset_y)
    {
        return Err(Error::config(
            "display",
            "DISPLAY_CONFIG_INVALID_OFFSET: offset must be -480..=480",
        ));
    }
    if cfg.spi.host != 2 && cfg.spi.host != 3 {
        return Err(Error::config(
            "display",
            "DISPLAY_CONFIG_INVALID_SPI_HOST: host must be 2 (SPI2) or 3 (SPI3)",
        ));
    }
    if !(DISPLAY_SPI_FREQ_MIN..=DISPLAY_SPI_FREQ_MAX).contains(&cfg.spi.freq_hz) {
        return Err(Error::config(
            "display",
            "DISPLAY_CONFIG_INVALID_TIMING: spi.freq_hz must be 1_000_000..=80_000_000",
        ));
    }
    #[cfg(target_os = "linux")]
    if !cfg.fb_device.is_empty() && cfg.fb_device.bytes().any(|b| b == 0 || b < 0x20) {
        return Err(Error::config(
            "display",
            "DISPLAY_CONFIG_INVALID_DEVICE_PATH: fb_device must contain no control characters",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_lease_allows_same_owner_reentry_and_releases_by_token() {
        let _guard = crate::runtime::lease::lease_test_guard();
        reset_display_lease_denied_total_for_tests();

        let first = try_acquire_display_lease_at(DisplayOwner::DefaultDashboard, Some(1_000), 100)
            .expect("first display lease");
        let second = try_acquire_display_lease_at(DisplayOwner::DefaultDashboard, Some(1_000), 101)
            .expect("reentrant display lease");

        let snapshot = crate::runtime::lease::snapshot_at(102);
        assert_eq!(snapshot.active_count, 1);
        assert_eq!(snapshot.records[0].hold_count, 2);

        drop(second);
        assert_eq!(crate::runtime::lease::snapshot_at(103).active_count, 1);
        drop(first);
        assert_eq!(crate::runtime::lease::snapshot_at(104).active_count, 0);
    }

    #[test]
    fn display_lease_denies_different_active_owner_and_counts_skip() {
        let _guard = crate::runtime::lease::lease_test_guard();
        reset_display_lease_denied_total_for_tests();

        let _default =
            try_acquire_display_lease_at(DisplayOwner::DefaultDashboard, Some(1_000), 100)
                .expect("default dashboard display lease");

        let denied = try_acquire_display_lease_at(DisplayOwner::ConfigUi, Some(1_000), 101);

        assert!(denied.is_none());
        assert_eq!(display_lease_denied_total(), 1);
    }

    #[test]
    fn display_lease_replaces_expired_script_owner() {
        let _guard = crate::runtime::lease::lease_test_guard();
        reset_display_lease_denied_total_for_tests();

        let script = try_acquire_display_lease_at(DisplayOwner::Script, Some(10), 100)
            .expect("script display lease");
        let default =
            try_acquire_display_lease_at(DisplayOwner::DefaultDashboard, Some(1_000), 111)
                .expect("default dashboard replaces expired lease");

        let snapshot = crate::runtime::lease::snapshot_at(112);
        assert_eq!(snapshot.active_count, 1);
        assert_eq!(
            snapshot.records[0].owner,
            DisplayOwner::DefaultDashboard.lease_owner()
        );

        drop(default);
        drop(script);
        assert_eq!(crate::runtime::lease::snapshot_at(113).active_count, 0);
    }

    #[test]
    fn default_display_config_keeps_linux_spi_swap_disabled() {
        let cfg = default_disabled_display_config();
        assert!(!cfg.linux_spi_swap_bytes);
    }

    #[test]
    fn default_display_config_uses_esp_idf_spi2_host() {
        let cfg = default_disabled_display_config();
        assert_eq!(cfg.spi.host, 2);
    }

    #[test]
    fn display_config_accepts_external_spi_hosts_only() {
        let mut cfg = default_disabled_display_config();
        cfg.enabled = true;

        cfg.spi.host = 2;
        assert!(validate_display_config_core(&cfg).is_ok());

        cfg.spi.host = 3;
        assert!(validate_display_config_core(&cfg).is_ok());

        cfg.spi.host = 1;
        assert!(validate_display_config_core(&cfg).is_err());
    }

    #[test]
    fn display_spidev_bus_mapping_matches_config_hosts() {
        assert_eq!(display_spidev_bus_for_config_host(2).unwrap(), 0);
        assert_eq!(display_spidev_bus_for_config_host(3).unwrap(), 1);
        assert!(display_spidev_bus_for_config_host(1).is_err());
    }

    #[test]
    fn display_lcd_row_window_uses_exclusive_esp_lcd_bounds() {
        let window = display_lcd_row_window(320, 240, 0, 0, 10, 4).unwrap();
        assert_eq!(window.x_start, 0);
        assert_eq!(window.x_end, 320);
        assert_eq!(window.y_start, 10);
        assert_eq!(window.y_end, 14);
        assert_eq!(window.row_start_byte, 320 * 2 * 10);
        assert_eq!(window.row_end_byte, 320 * 2 * 14);
    }

    #[test]
    fn display_lcd_row_window_clamps_dirty_rows() {
        let window = display_lcd_row_window(240, 240, -5, 3, 238, 8).unwrap();
        assert_eq!(window.x_start, 0);
        assert_eq!(window.x_end, 240);
        assert_eq!(window.y_start, 241);
        assert_eq!(window.y_end, 243);
        assert_eq!(window.row_start_byte, 240 * 2 * 238);
        assert_eq!(window.row_end_byte, 240 * 2 * 240);
        assert!(display_lcd_row_window(240, 240, 0, 0, 240, 1).is_none());
        assert!(display_lcd_row_window(240, 240, 0, 0, 0, 0).is_none());
    }

    #[test]
    fn compute_layout_square_240_legacy_markers() {
        let layout = compute_layout(240, 240);
        assert_eq!(layout.header_top, 16);
        assert_eq!(layout.title_top, 18);
        assert_eq!(layout.subtitle_top, 44);
        assert_eq!(layout.middle_top, 104);
        assert_eq!(layout.footer_top, 168);
        assert!(layout.title_top < layout.subtitle_top);
        assert!(layout.subtitle_top < layout.middle_top);
        assert!(layout.middle_top < layout.footer_top);
    }

    #[test]
    fn compute_layout_portrait_tall_bucket() {
        let square = compute_layout(240, 240);
        let portrait = compute_layout(240, 280);
        assert!(portrait.title_top < portrait.subtitle_top);
        assert!(portrait.subtitle_top < portrait.middle_top);
        assert!(portrait.middle_top < portrait.footer_top);
        assert_ne!(portrait.middle_top, square.middle_top);
        assert_ne!(portrait.footer_top, square.footer_top);
    }

    #[test]
    fn compute_layout_landscape_wide_bucket() {
        let square = compute_layout(240, 240);
        let landscape = compute_layout(280, 240);
        assert!(landscape.title_top < landscape.subtitle_top);
        assert!(landscape.subtitle_top < landscape.middle_top);
        assert!(landscape.middle_top < landscape.footer_top);
        assert_ne!(landscape.middle_top, square.middle_top);
        assert_ne!(landscape.footer_top, square.footer_top);
    }

    #[test]
    fn compute_layout_wide_header_expands_to_fit_icon() {
        let layout = compute_layout(320, 240);
        let header_bottom = layout.middle_top.saturating_sub(6);
        assert_eq!(layout.icon_size, 64);
        assert!(layout.header_top + layout.icon_size <= header_bottom);
    }

    #[test]
    fn compute_layout_wide_header_keeps_bottom_breathing_room() {
        let layout = compute_layout(320, 240);
        let header_bottom = layout.middle_top.saturating_sub(6);
        let bottom_gap = header_bottom.saturating_sub(layout.header_top + layout.icon_size);
        assert!(bottom_gap >= 8);
    }

    #[test]
    fn compute_layout_small_dims_no_panic() {
        let layout = compute_layout(120, 120);
        assert!(layout.icon_size >= 16);
        assert!(layout.footer_top >= layout.middle_top);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_spi_validation_allows_empty_device_path() {
        let mut cfg = default_disabled_display_config();
        cfg.enabled = true;
        cfg.fb_device.clear();

        assert!(validate_display_config_core(&cfg).is_ok());
    }
}
