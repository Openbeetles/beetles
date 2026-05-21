//! Display runtime state for platform implementations.
//! 平台层显示运行态封装 — SPI 硬件初始化 + embedded-graphics 渲染。
#![cfg_attr(
    not(any(target_arch = "xtensa", target_arch = "riscv32")),
    allow(dead_code)
)]

use crate::display::{
    compute_layout, DisplayChannelStatus, DisplayCommand, DisplayConfig, DisplayLayout,
    DisplayPressureLevel, DisplaySystemState, DISPLAY_LAYOUT_REF_PX,
};
use crate::error::{Error, Result};
use std::convert::Infallible;

const ESP_DISPLAY_SPI_DMA_ROWS_PER_CHUNK: usize = 4;

fn display_spi_max_transfer_size(width: u16, height: u16) -> usize {
    let framebuf_len = width as usize * height as usize * 2;
    if framebuf_len == 0 {
        return 0;
    }
    let chunk_bytes = width as usize * ESP_DISPLAY_SPI_DMA_ROWS_PER_CHUNK * 2;
    chunk_bytes.max(2).min(framebuf_len)
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//  FlushRgb565 — 抽象刷屏接口，SPI 与 framebuffer 均实现
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// 将内存帧缓冲写到物理显示器的统一接口。
/// `SpiDisplayBackend`（ESP）与 `LinuxFramebufferBackend` 均实现此 trait。
pub(crate) trait FlushRgb565 {
    /// 将整个逻辑帧写到硬件（委托 `flush_rows`）。
    fn flush(&mut self, offset_x: i16, offset_y: i16) -> Result<()>;
    /// 将行范围 `[ry, ry+rh)` 写到硬件。
    fn flush_rows(&mut self, offset_x: i16, offset_y: i16, ry: u16, rh: u16) -> Result<()>;
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//  ESP32 target — real SPI backend
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
mod esp_backend {
    use super::*;
    use crate::display::{display_lcd_row_window, DisplayColorOrder, DisplayDriver};
    use crate::platform::heap;
    use embedded_graphics_core::{
        draw_target::DrawTarget,
        geometry::{OriginDimensions, Size},
        pixelcolor::Rgb565,
        Pixel,
    };
    use esp_idf_svc::sys::*;

    /// SPI-connected display backend (ST7789 / ILI9341 family via ESP-IDF `esp_lcd`).
    /// Framebuffer lives in PSRAM; rendering via `embedded-graphics` `DrawTarget`.
    pub(super) struct SpiDisplayBackend {
        spi_host: u32,
        io_handle: esp_lcd_panel_io_handle_t,
        panel_handle: esp_lcd_panel_handle_t,
        width: u16,
        height: u16,
        framebuf: *mut u8,
        framebuf_len: usize,
    }

    // SAFETY: SpiDisplayBackend is only accessed from the display thread (behind Mutex).
    unsafe impl Send for SpiDisplayBackend {}

    impl Drop for SpiDisplayBackend {
        fn drop(&mut self) {
            unsafe {
                if !self.panel_handle.is_null() {
                    let ret = esp_lcd_panel_del(self.panel_handle);
                    if ret != ESP_OK {
                        log::warn!("[display] esp_lcd_panel_del failed: {}", ret);
                    }
                }
                if !self.io_handle.is_null() {
                    let ret = esp_lcd_panel_io_del(self.io_handle);
                    if ret != ESP_OK {
                        log::warn!("[display] esp_lcd_panel_io_del failed: {}", ret);
                    }
                }
                spi_bus_free(self.spi_host);
                heap::free_spiram_buffer(self.framebuf);
            }
        }
    }

    impl SpiDisplayBackend {
        fn esp_spi_host_device(config_host: u8) -> Result<u32> {
            match config_host {
                2 => Ok(spi_host_device_t_SPI2_HOST as u32),
                3 => Ok(spi_host_device_t_SPI3_HOST as u32),
                _ => Err(crate::error::Error::config(
                    "display_spi_host",
                    "DISPLAY_CONFIG_INVALID_SPI_HOST: host must be 2 (SPI2) or 3 (SPI3)",
                )),
            }
        }

        pub fn new(config: &DisplayConfig) -> Result<Self> {
            if matches!(config.driver, DisplayDriver::Framebuffer) {
                return Err(crate::error::Error::config(
                    "display_spi_init",
                    "driver=framebuffer is for Linux fbdev only; on ESP use St7789/ILI9341",
                ));
            }
            let spi = &config.spi;
            let width = config.width;
            let height = config.height;
            let framebuf_len = width as usize * height as usize * 2;
            // SPI DMA 分块传输：4 行 ≈ 2.5KB@320px，避免资源紧张时 IDF 临时 TX buffer 分配失败。
            let max_transfer_sz = display_spi_max_transfer_size(width, height);

            let framebuf = heap::alloc_spiram_buffer(framebuf_len).ok_or_else(|| {
                crate::error::Error::config(
                    "display_init",
                    format!("failed to allocate {}B PSRAM framebuffer", framebuf_len),
                )
            })?;
            // Zero the framebuffer
            unsafe { core::ptr::write_bytes(framebuf, 0, framebuf_len) };

            // --- Optional BL pin: set high (will be reconfigured to PWM if LEDC succeeds) ---
            if let Some(bl) = spi.bl {
                unsafe {
                    let bl_conf = gpio_config_t {
                        pin_bit_mask: 1u64 << bl,
                        mode: gpio_mode_t_GPIO_MODE_OUTPUT,
                        pull_up_en: gpio_pullup_t_GPIO_PULLUP_DISABLE,
                        pull_down_en: gpio_pulldown_t_GPIO_PULLDOWN_DISABLE,
                        intr_type: gpio_int_type_t_GPIO_INTR_DISABLE,
                        ..core::mem::zeroed()
                    };
                    let ret = gpio_config(&bl_conf);
                    if ret != ESP_OK {
                        heap::free_spiram_buffer(framebuf);
                        return Err(crate::error::Error::Esp {
                            code: ret,
                            stage: "display_bl_gpio",
                        });
                    }
                    gpio_set_level(bl, 1);
                }
            }

            // --- Initialize SPI bus ---
            // Bindgen layout differs: IDF 5.x flat-ish anon fields vs IDF 6 extra nesting
            // (same as `esp-idf-hal` `SpiDriver::new_internal`).
            #[cfg(not(esp_idf_version_at_least_6_0_0))]
            let bus_cfg = {
                let mut bus_cfg: spi_bus_config_t = unsafe { core::mem::zeroed() };
                bus_cfg.__bindgen_anon_1.mosi_io_num = spi.mosi;
                bus_cfg.__bindgen_anon_2.miso_io_num = -1;
                bus_cfg.sclk_io_num = spi.sclk;
                bus_cfg.__bindgen_anon_3.quadwp_io_num = -1;
                bus_cfg.__bindgen_anon_4.quadhd_io_num = -1;
                bus_cfg.data4_io_num = -1;
                bus_cfg.data5_io_num = -1;
                bus_cfg.data6_io_num = -1;
                bus_cfg.data7_io_num = -1;
                bus_cfg.max_transfer_sz = max_transfer_sz as i32;
                bus_cfg.flags = SPICOMMON_BUSFLAG_MASTER;
                bus_cfg
            };
            #[cfg(esp_idf_version_at_least_6_0_0)]
            let bus_cfg = spi_bus_config_t {
                __bindgen_anon_1: spi_bus_config_t__bindgen_ty_1 {
                    __bindgen_anon_1: spi_bus_config_t__bindgen_ty_1__bindgen_ty_1 {
                        sclk_io_num: spi.sclk,
                        data4_io_num: -1,
                        data5_io_num: -1,
                        data6_io_num: -1,
                        data7_io_num: -1,
                        __bindgen_anon_1:
                            spi_bus_config_t__bindgen_ty_1__bindgen_ty_1__bindgen_ty_1 {
                                mosi_io_num: spi.mosi,
                            },
                        __bindgen_anon_2:
                            spi_bus_config_t__bindgen_ty_1__bindgen_ty_1__bindgen_ty_2 {
                                miso_io_num: -1,
                            },
                        __bindgen_anon_3:
                            spi_bus_config_t__bindgen_ty_1__bindgen_ty_1__bindgen_ty_3 {
                                quadwp_io_num: -1,
                            },
                        __bindgen_anon_4:
                            spi_bus_config_t__bindgen_ty_1__bindgen_ty_1__bindgen_ty_4 {
                                quadhd_io_num: -1,
                            },
                    },
                },
                data_io_default_level: false,
                max_transfer_sz: max_transfer_sz as i32,
                flags: SPICOMMON_BUSFLAG_MASTER,
                isr_cpu_id: esp_intr_cpu_affinity_t_ESP_INTR_CPU_AFFINITY_AUTO,
                intr_flags: 0,
            };
            let spi_host = Self::esp_spi_host_device(spi.host)?;
            unsafe {
                let ret = spi_bus_initialize(spi_host, &bus_cfg, spi_common_dma_t_SPI_DMA_CH_AUTO);
                if ret != ESP_OK {
                    heap::free_spiram_buffer(framebuf);
                    return Err(crate::error::Error::Esp {
                        code: ret,
                        stage: "display_spi_bus_init",
                    });
                }
            }

            let mut io_config: esp_lcd_panel_io_spi_config_t = unsafe { core::mem::zeroed() };
            io_config.dc_gpio_num = spi.dc;
            io_config.cs_gpio_num = spi.cs;
            io_config.pclk_hz = spi.freq_hz;
            io_config.lcd_cmd_bits = 8;
            io_config.lcd_param_bits = 8;
            io_config.spi_mode = 0;
            io_config.trans_queue_depth = 1;

            let mut io_handle: esp_lcd_panel_io_handle_t = core::ptr::null_mut();
            unsafe {
                let ret = esp_lcd_new_panel_io_spi(
                    spi_host as esp_lcd_spi_bus_handle_t,
                    &io_config,
                    &mut io_handle,
                );
                if ret != ESP_OK {
                    spi_bus_free(spi_host);
                    heap::free_spiram_buffer(framebuf);
                    return Err(crate::error::Error::Esp {
                        code: ret,
                        stage: "display_lcd_panel_io_spi",
                    });
                }
            }

            let mut panel_config: esp_lcd_panel_dev_config_t = unsafe { core::mem::zeroed() };
            panel_config.reset_gpio_num = spi.rst.unwrap_or(-1);
            panel_config.rgb_ele_order = Self::rgb_element_order(&config.color_order);
            panel_config.data_endian = lcd_rgb_data_endian_t_LCD_RGB_DATA_ENDIAN_BIG;
            panel_config.bits_per_pixel = 16;
            panel_config
                .flags
                .set_reset_active_high(u32::from(spi.rst_active_high));

            let mut panel_handle: esp_lcd_panel_handle_t = core::ptr::null_mut();
            unsafe {
                let ret = match config.driver {
                    DisplayDriver::St7789 => {
                        esp_lcd_new_panel_st7789(io_handle, &panel_config, &mut panel_handle)
                    }
                    DisplayDriver::Ili9341 => {
                        esp_lcd_new_panel_ili9341(io_handle, &panel_config, &mut panel_handle)
                    }
                    DisplayDriver::Framebuffer => ESP_ERR_INVALID_ARG,
                };
                if ret != ESP_OK {
                    let _ = esp_lcd_panel_io_del(io_handle);
                    spi_bus_free(spi_host);
                    heap::free_spiram_buffer(framebuf);
                    return Err(crate::error::Error::Esp {
                        code: ret,
                        stage: "display_lcd_panel_new",
                    });
                }
            }

            let backend = Self {
                spi_host,
                io_handle,
                panel_handle,
                width,
                height,
                framebuf,
                framebuf_len,
            };

            backend.init_display_controller(config)?;

            log::info!(
                "[display] SPI backend ready: {}x{}, driver={:?}",
                width,
                height,
                config.driver
            );
            Ok(backend)
        }

        fn rgb_element_order(color_order: &DisplayColorOrder) -> lcd_rgb_element_order_t {
            match color_order {
                DisplayColorOrder::Rgb => lcd_rgb_element_order_t_LCD_RGB_ELEMENT_ORDER_RGB,
                DisplayColorOrder::Bgr => lcd_rgb_element_order_t_LCD_RGB_ELEMENT_ORDER_BGR,
            }
        }

        fn init_display_controller(&self, config: &DisplayConfig) -> Result<()> {
            unsafe {
                Self::check_esp(esp_lcd_panel_reset(self.panel_handle), "display_lcd_reset")?;
                Self::check_esp(esp_lcd_panel_init(self.panel_handle), "display_lcd_init")?;
                let needs_invon = match config.driver {
                    DisplayDriver::St7789 => !config.invert_colors,
                    DisplayDriver::Ili9341 => config.invert_colors,
                    DisplayDriver::Framebuffer => false,
                };
                Self::check_esp(
                    esp_lcd_panel_invert_color(self.panel_handle, needs_invon),
                    "display_lcd_invert",
                )?;
                let (swap_xy, mirror_x, mirror_y) = Self::orientation_for_rotation(config.rotation);
                Self::check_esp(
                    esp_lcd_panel_swap_xy(self.panel_handle, swap_xy),
                    "display_lcd_swap_xy",
                )?;
                Self::check_esp(
                    esp_lcd_panel_mirror(self.panel_handle, mirror_x, mirror_y),
                    "display_lcd_mirror",
                )?;
                Self::check_esp(
                    esp_lcd_panel_disp_on_off(self.panel_handle, true),
                    "display_lcd_disp_on",
                )?;
            }

            Ok(())
        }

        fn orientation_for_rotation(rotation: u16) -> (bool, bool, bool) {
            match rotation {
                90 => (true, true, false),
                180 => (false, true, true),
                270 => (true, false, true),
                _ => (false, false, false),
            }
        }

        fn check_esp(code: esp_err_t, stage: &'static str) -> Result<()> {
            if code != ESP_OK {
                return Err(crate::error::Error::Esp { code, stage });
            }
            Ok(())
        }

        /// Set column/row address window then push full framebuf via SPI.
        pub fn flush(&mut self, offset_x: i16, offset_y: i16) -> Result<()> {
            self.flush_rows(offset_x, offset_y, 0, self.height)
        }

        /// Push only the rows `[ry..ry+rh)` from the framebuffer, reducing SPI transfer.
        pub fn flush_rows(&mut self, offset_x: i16, offset_y: i16, ry: u16, rh: u16) -> Result<()> {
            let Some(window) =
                display_lcd_row_window(self.width, self.height, offset_x, offset_y, ry, rh)
            else {
                return Ok(());
            };
            let buf = unsafe { core::slice::from_raw_parts(self.framebuf, self.framebuf_len) };
            let color_data = buf[window.row_start_byte..window.row_end_byte].as_ptr();
            unsafe {
                Self::check_esp(
                    esp_lcd_panel_draw_bitmap(
                        self.panel_handle,
                        window.x_start,
                        window.y_start,
                        window.x_end,
                        window.y_end,
                        color_data as *const _,
                    ),
                    "display_lcd_draw_bitmap",
                )?;
            }
            Ok(())
        }

        /// Write a pixel at (x, y) into the framebuffer (no SPI transfer).
        #[inline]
        fn set_pixel(&mut self, x: u16, y: u16, color: Rgb565) {
            if x < self.width && y < self.height {
                let offset = (y as usize * self.width as usize + x as usize) * 2;
                let raw = RawU16::from(color).into_inner().to_be();
                unsafe {
                    let ptr = self.framebuf.add(offset) as *mut u16;
                    *ptr = raw;
                }
            }
        }
    }

    use embedded_graphics_core::pixelcolor::raw::RawU16;

    impl DrawTarget for SpiDisplayBackend {
        type Color = Rgb565;
        type Error = core::convert::Infallible;

        fn draw_iter<I>(&mut self, pixels: I) -> core::result::Result<(), Self::Error>
        where
            I: IntoIterator<Item = Pixel<Self::Color>>,
        {
            for Pixel(point, color) in pixels {
                if point.x >= 0
                    && point.y >= 0
                    && (point.x as u16) < self.width
                    && (point.y as u16) < self.height
                {
                    self.set_pixel(point.x as u16, point.y as u16, color);
                }
            }
            Ok(())
        }
    }

    impl OriginDimensions for SpiDisplayBackend {
        fn size(&self) -> Size {
            Size::new(self.width as u32, self.height as u32)
        }
    }

    impl super::FlushRgb565 for SpiDisplayBackend {
        fn flush(&mut self, offset_x: i16, offset_y: i16) -> Result<()> {
            SpiDisplayBackend::flush(self, offset_x, offset_y)
        }
        fn flush_rows(&mut self, offset_x: i16, offset_y: i16, ry: u16, rh: u16) -> Result<()> {
            SpiDisplayBackend::flush_rows(self, offset_x, offset_y, ry, rh)
        }
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//  Linux framebuffer backend — FlushRgb565 impl
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[cfg(all(
    target_os = "linux",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
use crate::platform::linux::display_fb::LinuxFramebufferBackend;
#[cfg(all(
    target_os = "linux",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
use crate::platform::linux::display_spi::LinuxSpiDisplayBackend;

#[cfg(all(
    target_os = "linux",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
impl FlushRgb565 for LinuxFramebufferBackend {
    fn flush(&mut self, offset_x: i16, offset_y: i16) -> Result<()> {
        LinuxFramebufferBackend::flush(self, offset_x, offset_y)
    }
    fn flush_rows(&mut self, offset_x: i16, offset_y: i16, ry: u16, rh: u16) -> Result<()> {
        LinuxFramebufferBackend::flush_rows(self, offset_x, offset_y, ry, rh)
    }
}

#[cfg(all(
    target_os = "linux",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
impl FlushRgb565 for LinuxSpiDisplayBackend {
    fn flush(&mut self, offset_x: i16, offset_y: i16) -> Result<()> {
        LinuxSpiDisplayBackend::flush(self, offset_x, offset_y)
    }
    fn flush_rows(&mut self, offset_x: i16, offset_y: i16, ry: u16, rh: u16) -> Result<()> {
        LinuxSpiDisplayBackend::flush_rows(self, offset_x, offset_y, ry, rh)
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//  dispatch_display_command — 平台无关显示指令派发（单份 match）
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// 将 `DisplayCommand` 渲染到 `backend` 并调用对应 flush。
/// `B` 须同时实现 `DrawTarget<Color=Rgb565>` 与 `FlushRgb565`，ESP 与 Linux 均可用。
fn dispatch_display_command<B>(
    backend: &mut B,
    config: &DisplayConfig,
    layout: &DisplayLayout,
    cmd: &DisplayCommand,
) -> Result<()>
where
    B: embedded_graphics_core::draw_target::DrawTarget<
            Color = embedded_graphics_core::pixelcolor::Rgb565,
            Error = Infallible,
        > + FlushRgb565,
{
    match cmd {
        DisplayCommand::RefreshDashboard {
            state,
            presence_subtitle,
            ip_address,
            channels,
            pressure,
            heap_percent,
            messages_in,
            messages_out,
            last_active_epoch_secs,
            uptime_secs: _,
            busy_phase,
            llm_last_ms,
            error_flash,
        } => {
            render_dashboard(
                backend,
                &DashboardParams {
                    layout,
                    state: *state,
                    presence_subtitle: presence_subtitle.as_deref(),
                    ip_address: ip_address.as_deref(),
                    channels,
                    pressure,
                    heap_percent: *heap_percent,
                    width: config.width,
                    height: config.height,
                    messages_in: *messages_in,
                    messages_out: *messages_out,
                    last_active_epoch_secs: *last_active_epoch_secs,
                    busy_phase: *busy_phase,
                    llm_last_ms: *llm_last_ms,
                    error_flash: *error_flash,
                },
            );
            backend.flush(config.offset_x, config.offset_y)?;
        }
        DisplayCommand::UpdateIp {
            ip,
            presence_subtitle,
            uptime_secs: _,
        } => {
            render_ip_partial(
                backend,
                ip.as_str(),
                presence_subtitle.as_deref(),
                config.width,
                config.height,
                layout,
            );
            let flush_h = hud_top_bar_rows(config.height);
            backend.flush_rows(config.offset_x, config.offset_y, 0, flush_h)?;
        }
        DisplayCommand::UpdateStateHeader {
            state,
            presence_subtitle: _,
            ip_address: _,
            uptime_secs: _,
            busy_phase,
        } => {
            render_state_header_partial(
                backend,
                *state,
                StateHeaderSnapshot {
                    busy_phase: *busy_phase,
                },
                config.width,
                config.height,
                layout,
            );
            backend.flush_rows(
                config.offset_x,
                config.offset_y,
                hud_status_top(config.height),
                hud_status_rows(config.height),
            )?;
        }
        DisplayCommand::UpdatePressure {
            level,
            heap_percent,
            messages_in,
            messages_out,
            last_active_epoch_secs,
            llm_last_ms,
            error_flash,
        } => {
            render_resource_partial(backend, config.width, config.height, level, *heap_percent);
            render_pressure_partial(
                backend,
                level,
                layout,
                &FooterPartialParams {
                    heap_percent: *heap_percent,
                    width: config.width,
                    height: config.height,
                    messages_in: *messages_in,
                    messages_out: *messages_out,
                    last_active_epoch_secs: *last_active_epoch_secs,
                    llm_last_ms: *llm_last_ms,
                    error_flash: *error_flash,
                },
            );
            backend.flush_rows(
                config.offset_x,
                config.offset_y,
                8,
                hud_top_bar_rows(config.height).saturating_add(4),
            )?;
            backend.flush_rows(
                config.offset_x,
                config.offset_y,
                hud_metrics_top(config.height),
                hud_metrics_rows(config.height),
            )?;
        }
        DisplayCommand::UpdateChannels { channels } => {
            render_channels_partial(backend, channels, config.width, config.height, layout);
            backend.flush_rows(
                config.offset_x,
                config.offset_y,
                hud_channels_top(config.height),
                hud_channels_rows(config.height),
            )?;
        }
    }
    Ok(())
}

#[cfg(any(
    test,
    all(
        target_os = "linux",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    )
))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LinuxDisplayBackendKind {
    Framebuffer,
    Spi,
}

#[cfg(any(
    test,
    all(
        target_os = "linux",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    )
))]
fn linux_display_backend_kind(config: &DisplayConfig) -> LinuxDisplayBackendKind {
    if crate::display::is_framebuffer_config(config) {
        LinuxDisplayBackendKind::Framebuffer
    } else {
        LinuxDisplayBackendKind::Spi
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//  DisplayState
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

pub struct DisplayState {
    pub config: DisplayConfig,
    /// 由 `config.width`/`height` 计算的仪表盘布局（与 SPI 是否启用无关）。
    pub layout: DisplayLayout,
    pub available: bool,
    /// BL GPIO pin number (if configured). Used for backlight on/off control.
    bl_pin: Option<i32>,
    /// F1: LEDC PWM 背光是否已初始化。
    bl_ledc_initialized: bool,
    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
    backend: Option<esp_backend::SpiDisplayBackend>,
    /// Linux framebuffer 后端（仅 Linux 非 ESP 目标编译）。
    #[cfg(all(
        target_os = "linux",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    ))]
    backend_fb: Option<LinuxFramebufferBackend>,
    #[cfg(all(
        target_os = "linux",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    ))]
    backend_spi: Option<LinuxSpiDisplayBackend>,
}

/// F1: LEDC PWM 背光常量。Channel 7 / Timer 3，不与 tool pwm_out 的 0-5 冲突。
const BL_LEDC_CHANNEL: u32 = 7;
const BL_LEDC_TIMER: u32 = 3;
const BL_LEDC_FREQ_HZ: u32 = 5000;
const BL_LEDC_DUTY_RESOLUTION: u32 = 13; // 13-bit → max duty 8191
const BL_LEDC_MAX_DUTY: u32 = 8191;

impl DisplayState {
    // cfg-gated return chains require explicit `return` to prevent fall-through to other cfg blocks.
    #[allow(clippy::needless_return)]
    pub fn init(config: &DisplayConfig) -> Result<Self> {
        let layout = compute_layout(config.width, config.height);
        if !config.enabled {
            return Ok(Self {
                config: config.clone(),
                layout,
                available: false,
                bl_pin: None,
                bl_ledc_initialized: false,
                #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
                backend: None,
                #[cfg(all(
                    target_os = "linux",
                    not(any(target_arch = "xtensa", target_arch = "riscv32"))
                ))]
                backend_fb: None,
                #[cfg(all(
                    target_os = "linux",
                    not(any(target_arch = "xtensa", target_arch = "riscv32"))
                ))]
                backend_spi: None,
            });
        }

        let bl_pin = config.spi.bl;

        #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
        {
            let backend = esp_backend::SpiDisplayBackend::new(config)?;
            let mut state = Self {
                config: config.clone(),
                layout,
                available: true,
                bl_pin,
                bl_ledc_initialized: false,
                backend: Some(backend),
            };
            // F1: 尝试初始化 LEDC PWM 背光；失败则降级为 GPIO 开关
            state.try_init_ledc_backlight();
            return Ok(state);
        }

        #[cfg(all(
            target_os = "linux",
            not(any(target_arch = "xtensa", target_arch = "riscv32"))
        ))]
        {
            match linux_display_backend_kind(config) {
                LinuxDisplayBackendKind::Framebuffer => {
                    match LinuxFramebufferBackend::new(config) {
                        Ok(fb) => {
                            return Ok(Self {
                                config: config.clone(),
                                layout,
                                available: true,
                                bl_pin,
                                bl_ledc_initialized: false,
                                backend_fb: Some(fb),
                                backend_spi: None,
                            });
                        }
                        Err(e) => {
                            log::warn!("[display] framebuffer init failed: {}", e);
                            return Err(Error::config(
                                "display_init",
                                format!("framebuffer init failed: {e}"),
                            ));
                        }
                    }
                }
                LinuxDisplayBackendKind::Spi => match LinuxSpiDisplayBackend::new(config) {
                    Ok(spi) => {
                        return Ok(Self {
                            config: config.clone(),
                            layout,
                            available: true,
                            bl_pin,
                            bl_ledc_initialized: false,
                            backend_fb: None,
                            backend_spi: Some(spi),
                        });
                    }
                    Err(e) => {
                        log::warn!("[display] linux spi init failed: {}", e);
                        return Err(Error::config(
                            "display_init",
                            format!("linux spi init failed: {e}"),
                        ));
                    }
                },
            }
        }

        #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32", target_os = "linux")))]
        {
            log::info!("[display] host stub: init skipped (no SPI hardware, not Linux)");
            Ok(Self {
                config: config.clone(),
                layout,
                available: false,
                bl_pin,
                bl_ledc_initialized: false,
            })
        }
    }

    /// F1: 尝试用 LEDC 初始化 PWM 背光（channel 7 / timer 3, 5kHz, 13-bit）。
    fn try_init_ledc_backlight(&mut self) {
        #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
        {
            let bl = match self.bl_pin {
                Some(pin) => pin,
                None => return,
            };
            unsafe {
                use esp_idf_svc::sys::*;
                let timer_cfg = ledc_timer_config_t {
                    speed_mode: ledc_mode_t_LEDC_LOW_SPEED_MODE,
                    duty_resolution: BL_LEDC_DUTY_RESOLUTION,
                    timer_num: BL_LEDC_TIMER,
                    freq_hz: BL_LEDC_FREQ_HZ,
                    clk_cfg: soc_periph_ledc_clk_src_legacy_t_LEDC_AUTO_CLK,
                    ..core::mem::zeroed()
                };
                let ret = ledc_timer_config(&timer_cfg);
                if ret != ESP_OK {
                    log::warn!(
                        "[display] LEDC timer init failed ({}), fallback to GPIO BL",
                        ret
                    );
                    return;
                }
                let ch_cfg = ledc_channel_config_t {
                    gpio_num: bl,
                    speed_mode: ledc_mode_t_LEDC_LOW_SPEED_MODE,
                    channel: BL_LEDC_CHANNEL,
                    timer_sel: BL_LEDC_TIMER,
                    duty: BL_LEDC_MAX_DUTY, // 启动时全亮
                    hpoint: 0,
                    ..core::mem::zeroed()
                };
                let ret = ledc_channel_config(&ch_cfg);
                if ret != ESP_OK {
                    log::warn!(
                        "[display] LEDC channel init failed ({}), fallback to GPIO BL",
                        ret
                    );
                    return;
                }
            }
            self.bl_ledc_initialized = true;
            log::info!(
                "[display] LEDC PWM backlight initialized (ch{}, 5kHz, 13-bit)",
                BL_LEDC_CHANNEL
            );
        }
    }

    pub fn execute(&mut self, cmd: DisplayCommand) -> Result<()> {
        if !self.available {
            return Ok(());
        }

        #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
        {
            let backend = match self.backend.as_mut() {
                Some(b) => b,
                None => return Ok(()),
            };
            dispatch_display_command(backend, &self.config, &self.layout, &cmd)?;
        }

        #[cfg(all(
            target_os = "linux",
            not(any(target_arch = "xtensa", target_arch = "riscv32"))
        ))]
        {
            if let Some(backend) = self.backend_fb.as_mut() {
                dispatch_display_command(backend, &self.config, &self.layout, &cmd)?;
            } else if let Some(backend) = self.backend_spi.as_mut() {
                dispatch_display_command(backend, &self.config, &self.layout, &cmd)?;
            } else {
                return Ok(());
            }
        }

        #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32", target_os = "linux")))]
        {
            let _ = cmd;
        }

        Ok(())
    }

    /// 背光控制是否可用。
    /// ESP: 显示器已初始化且有 BL 引脚；Linux: 显示器已初始化且配置了 backlight_sysfs 路径。
    pub fn backlight_available(&self) -> bool {
        if !self.available {
            return false;
        }
        #[cfg(all(
            target_os = "linux",
            not(any(target_arch = "xtensa", target_arch = "riscv32"))
        ))]
        let result = self.config.backlight_sysfs.is_some() || self.bl_pin.is_some();
        #[cfg(not(all(
            target_os = "linux",
            not(any(target_arch = "xtensa", target_arch = "riscv32"))
        )))]
        let result = self.bl_pin.is_some();
        result
    }

    /// 设置背光开关。on=true 开启（GPIO HIGH 或 PWM 100%），on=false 关闭。
    /// Set backlight on/off. Uses PWM if LEDC initialized, otherwise GPIO level (ESP) or sysfs (Linux).
    pub fn set_backlight(&self, on: bool) -> Result<()> {
        self.set_brightness(if on { 100 } else { 0 })
    }

    /// F1: 设置背光亮度（0-100%）。
    /// ESP: LEDC PWM duty；Linux: sysfs brightness 文件；其它目标: no-op。
    pub fn set_brightness(&self, percent: u8) -> Result<()> {
        #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
        {
            if !self.bl_ledc_initialized {
                // 降级为 GPIO 开关
                if let Some(bl) = self.bl_pin {
                    let level = if percent > 0 { 1u32 } else { 0u32 };
                    unsafe {
                        esp_idf_svc::sys::gpio_set_level(bl, level);
                    }
                }
                return Ok(());
            }
            let duty = (percent.min(100) as u32) * BL_LEDC_MAX_DUTY / 100;
            unsafe {
                use esp_idf_svc::sys::*;
                let ret = ledc_set_duty(ledc_mode_t_LEDC_LOW_SPEED_MODE, BL_LEDC_CHANNEL, duty);
                if ret != ESP_OK {
                    log::warn!("[display] ledc_set_duty failed ({})", ret);
                }
                let ret = ledc_update_duty(ledc_mode_t_LEDC_LOW_SPEED_MODE, BL_LEDC_CHANNEL);
                if ret != ESP_OK {
                    log::warn!("[display] ledc_update_duty failed ({})", ret);
                }
            }
        }
        #[cfg(all(
            target_os = "linux",
            not(any(target_arch = "xtensa", target_arch = "riscv32"))
        ))]
        {
            if let Some(ref bl_path) = self.config.backlight_sysfs {
                sysfs_write_brightness(bl_path, percent);
            } else if let Some(backend) = self.backend_spi.as_ref() {
                backend.set_backlight(percent > 0)?;
            }
        }
        #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32", target_os = "linux")))]
        {
            let _ = percent;
        }
        Ok(())
    }

    /// F1: 背光渐变，20 步线性插值，阻塞在调用线程。
    /// Fade backlight from `from`% to `to`% over `duration_ms`, 20 steps, blocking.
    pub fn fade_brightness(&self, from: u8, to: u8, duration_ms: u32) -> Result<()> {
        const STEPS: u32 = 20;
        let step_ms = duration_ms / STEPS;
        let from_val = from.min(100) as i32;
        let to_val = to.min(100) as i32;
        for i in 0..=STEPS {
            let pct = from_val + (to_val - from_val) * i as i32 / STEPS as i32;
            self.set_brightness(pct as u8)?;
            if i < STEPS {
                std::thread::sleep(std::time::Duration::from_millis(step_ms as u64));
            }
        }
        Ok(())
    }
}

/// 将 `DisplayState::init` 结果写入 `slot`：成功则 `Some`，失败则清空并返回 `display_init` 错误。
/// Writes `DisplayState::init` into `slot`: `Some` on success; clears and returns on failure.
pub(crate) fn install_display_state(
    slot: &mut Option<DisplayState>,
    config: &DisplayConfig,
) -> Result<()> {
    match DisplayState::init(config) {
        Ok(state) => {
            *slot = Some(state);
            Ok(())
        }
        Err(e) => {
            *slot = None;
            Err(Error::config(
                "display_init",
                format!("display init failed: {e}"),
            ))
        }
    }
}

// ── sysfs 背光辅助（Linux 非 ESP）──────────────────────────────────────────

#[cfg(all(
    target_os = "linux",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
fn sysfs_write_brightness(bl_path: &str, percent: u8) {
    use std::io::Write;
    // 读取 max_brightness（与 brightness 文件同目录）。
    let max: u32 = (|| -> Option<u32> {
        let parent = std::path::Path::new(bl_path).parent()?;
        let max_path = parent.join("max_brightness");
        let s = std::fs::read_to_string(max_path).ok()?;
        s.trim().parse().ok()
    })()
    .unwrap_or(255);

    let value = (percent.min(100) as u32) * max / 100;
    match std::fs::OpenOptions::new().write(true).open(bl_path) {
        Ok(mut f) => {
            if let Err(e) = write!(f, "{}", value) {
                log::warn!("[display] sysfs backlight write failed: {}", e);
            }
        }
        Err(e) => {
            log::warn!("[display] sysfs backlight open failed ({}): {}", bl_path, e);
        }
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//  Platform-agnostic rendering (embedded-graphics)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

use embedded_graphics::{
    mono_font::{ascii::FONT_6X10, MonoTextStyle},
    prelude::*,
    primitives::{Circle, Ellipse, Line, PrimitiveStyle, Rectangle},
    text::Text,
};
use embedded_graphics_core::pixelcolor::Rgb565;

/// Beetle drawing options.
#[derive(Default)]
struct BeetleOpts {
    flipped: bool,
    x_eyes: bool,
    wings: bool,
    /// 录音态：触角张开、头部声波弧线。
    listening: bool,
}

/// RGB565 color helpers.
const fn rgb565(r: u8, g: u8, b: u8) -> Rgb565 {
    Rgb565::new(r >> 3, g >> 2, b >> 3)
}

fn darken(c: Rgb565, amt: u8) -> Rgb565 {
    let r = c.r().saturating_sub(amt >> 3);
    let g = c.g().saturating_sub(amt >> 2);
    let b = c.b().saturating_sub(amt >> 3);
    Rgb565::new(r, g, b)
}

fn lighten(c: Rgb565, amt: u8) -> Rgb565 {
    let r = c.r().saturating_add(amt >> 3).min(31);
    let g = c.g().saturating_add(amt >> 2).min(63);
    let b = c.b().saturating_add(amt >> 3).min(31);
    Rgb565::new(r, g, b)
}

/// Dim fill for beetle body/head glow halo on dark dashboard background.
/// 深色底上的甲壳虫光晕填充色（高对比下的柔和外圈）。
fn beetle_glow_fill(base: Rgb565) -> Rgb565 {
    // Dim to 1/4 intensity for a subtle glow
    Rgb565::new(base.r() >> 2, base.g() >> 2, base.b() >> 2)
}

/// Draw a beetle icon using embedded-graphics primitives.
///
/// `x`, `y` is the top-left corner of the bounding box; `size` is the box side length.
/// Returns (cx, body_cy, body_r, head_cy, head_r) for overlay placement.
fn draw_beetle<D: DrawTarget<Color = Rgb565>>(
    target: &mut D,
    x: i32,
    y: i32,
    size: i32,
    color: Rgb565,
    opts: &BeetleOpts,
) -> (i32, i32, i32, i32, i32) {
    let cx = x + size / 2;
    let dir: i32 = if opts.flipped { -1 } else { 1 };

    // Logical radii for external overlays (like busy dots, mic, etc.)
    let body_r = size * 28 / 100;
    let head_r = size * 14 / 100;

    // Mecha/Stag beetle proportions (elongated body, wider head)
    let body_rx = size * 24 / 100;
    let body_ry = size * 32 / 100;
    let head_rx = size * 16 / 100;
    let head_ry = size * 11 / 100;

    let head_cy = if opts.flipped {
        y + size * 72 / 100
    } else {
        y + size * 30 / 100
    };
    let body_cy = if opts.flipped {
        y + size * 40 / 100
    } else {
        y + size * 62 / 100
    };

    let line_style = PrimitiveStyle::with_stroke(color, 2);

    // --- Antennae (Radar style) ---
    let ant_tip_dy = if opts.listening {
        dir * size * 28 / 100
    } else {
        dir * size * 20 / 100
    };
    let ant_spread = if opts.listening {
        size * 30 / 100
    } else {
        size * 20 / 100
    };
    let ant_base_dy = dir * head_ry * 8 / 10;

    for &sx in &[-1i32, 1] {
        let ant_base = Point::new(cx + sx * head_rx / 2, head_cy - ant_base_dy);
        let ant_tip = Point::new(cx + sx * ant_spread, head_cy - ant_tip_dy);
        let _ = Line::new(ant_base, ant_tip)
            .into_styled(line_style)
            .draw(target);
        // Radar cross at tip
        let _ = Line::new(
            Point::new(ant_tip.x - 2, ant_tip.y),
            Point::new(ant_tip.x + 2, ant_tip.y),
        )
        .into_styled(line_style)
        .draw(target);
    }

    // --- Legs (Mechanical joints) ---
    let leg_attach_fracs: [i32; 3] = [-25, 0, 25]; // percent of body_ry
    let leg_angles_deg: [i32; 3] = [-20, 5, 25];
    let leg_len1 = size * 10 / 100;
    let leg_len2 = size * 7 / 100;

    for (i, &frac) in leg_attach_fracs.iter().enumerate() {
        let leg_y = body_cy + body_ry * frac / 100;
        let ang_deg = leg_angles_deg[i] * dir;
        let (cos_a, sin_a) = approx_cos_sin(ang_deg);

        for &side in &[-1i32, 1] {
            let ax = cx + side * body_rx * 95 / 100;
            let kx = ax + side * leg_len1 * cos_a / 100;
            let ky = leg_y + leg_len1 * sin_a / 100;
            let fx = kx + side * leg_len2 * 50 / 100;
            let fy = ky + leg_len2 * 90 / 100 * dir;

            let _ = Line::new(Point::new(ax, leg_y), Point::new(kx, ky))
                .into_styled(line_style)
                .draw(target);
            let _ = Line::new(Point::new(kx, ky), Point::new(fx, fy))
                .into_styled(line_style)
                .draw(target);

            // Mechanical joint dot
            let _ = Rectangle::new(Point::new(kx - 1, ky - 1), Size::new(3, 3))
                .into_styled(PrimitiveStyle::with_fill(color))
                .draw(target);
        }
    }

    // --- Wings (busy state: membrane wings peeking out from under elytra) ---
    if opts.wings {
        let wing_color = darken(color, 30);
        let wing_style = PrimitiveStyle::with_stroke(wing_color, 1);

        let wing_span = size * 18 / 100;
        let wing_h = size * 22 / 100;
        let wing_top = body_cy - wing_h * 6 / 10;

        for &side in &[-1i32, 1] {
            let base_x = cx + side * body_rx;
            let tip_x = base_x + side * wing_span;
            let mid_x = base_x + side * wing_span * 7 / 10;

            let t0 = Point::new(base_x, wing_top + wing_h * 2 / 10);
            let t1 = Point::new(mid_x, wing_top);
            let t2 = Point::new(tip_x, wing_top + wing_h * 3 / 10);
            let b1 = Point::new(mid_x, wing_top + wing_h);
            let b0 = Point::new(base_x, wing_top + wing_h * 7 / 10);

            let _ = Line::new(t0, t1).into_styled(wing_style).draw(target);
            let _ = Line::new(t1, t2).into_styled(wing_style).draw(target);
            let _ = Line::new(t2, b1).into_styled(wing_style).draw(target);
            let _ = Line::new(b1, b0).into_styled(wing_style).draw(target);
            let _ = Line::new(b0, t0).into_styled(wing_style).draw(target);

            let vein_style = PrimitiveStyle::with_stroke(wing_color, 1);
            let vein_mid = Point::new(mid_x - side * wing_span / 10, wing_top + wing_h / 2);
            let _ = Line::new(Point::new(base_x, wing_top + wing_h * 4 / 10), vein_mid)
                .into_styled(vein_style)
                .draw(target);
            let _ = Line::new(
                vein_mid,
                Point::new(tip_x - side * 2, wing_top + wing_h * 4 / 10),
            )
            .into_styled(vein_style)
            .draw(target);
        }
    }

    // --- Body glow halo (dim ring behind filled body) ---
    let glow_fill = PrimitiveStyle::with_fill(beetle_glow_fill(color));
    let body_glow_rx = body_rx + 4;
    let body_glow_ry = body_ry + 4;
    let _ = Ellipse::new(
        Point::new(cx - body_glow_rx, body_cy - body_glow_ry),
        Size::new((body_glow_rx * 2) as u32, (body_glow_ry * 2) as u32),
    )
    .into_styled(glow_fill)
    .draw(target);

    // --- Body (large filled ellipse) ---
    let body_fill = PrimitiveStyle::with_fill(color);
    let _ = Ellipse::new(
        Point::new(cx - body_rx, body_cy - body_ry),
        Size::new((body_rx * 2) as u32, (body_ry * 2) as u32),
    )
    .into_styled(body_fill)
    .draw(target);

    let body_highlight = lighten(color, 40);
    let body_outline = PrimitiveStyle::with_stroke(body_highlight, 1);
    let _ = Ellipse::new(
        Point::new(cx - body_rx, body_cy - body_ry),
        Size::new((body_rx * 2) as u32, (body_ry * 2) as u32),
    )
    .into_styled(body_outline)
    .draw(target);

    // --- Elytra seam (center line) ---
    let seam_color = darken(color, 50);
    let seam_style = PrimitiveStyle::with_stroke(seam_color, 1);
    let _ = Line::new(
        Point::new(cx, body_cy - body_ry + 3),
        Point::new(cx, body_cy + body_ry - 3),
    )
    .into_styled(seam_style)
    .draw(target);

    // Energy core (small diamond on the upper back)
    let core_y = body_cy - body_ry * 30 / 100;
    let _ = Line::new(Point::new(cx, core_y - 3), Point::new(cx - 3, core_y))
        .into_styled(seam_style)
        .draw(target);
    let _ = Line::new(Point::new(cx - 3, core_y), Point::new(cx, core_y + 3))
        .into_styled(seam_style)
        .draw(target);
    let _ = Line::new(Point::new(cx, core_y + 3), Point::new(cx + 3, core_y))
        .into_styled(seam_style)
        .draw(target);
    let _ = Line::new(Point::new(cx + 3, core_y), Point::new(cx, core_y - 3))
        .into_styled(seam_style)
        .draw(target);

    let glint_style = PrimitiveStyle::with_stroke(body_highlight, 1);
    for &sx in &[-1i32, 1] {
        let top = Point::new(cx + sx * body_rx * 24 / 100, body_cy - body_ry * 66 / 100);
        let bottom = Point::new(cx + sx * body_rx * 43 / 100, body_cy - body_ry * 14 / 100);
        let _ = Line::new(top, bottom).into_styled(glint_style).draw(target);
    }

    // --- Elytra ridges (Mecha panel lines) ---
    let ridge_color = darken(color, 30);
    let ridge_style = PrimitiveStyle::with_stroke(ridge_color, 1);
    for &sx in &[-1i32, 1] {
        let rx_top = cx + sx * body_rx * 40 / 100;
        let ry_top = body_cy - body_ry * 60 / 100;
        let rx_mid = cx + sx * body_rx * 70 / 100;
        let ry_mid = body_cy - body_ry * 10 / 100;
        let rx_bot = cx + sx * body_rx * 50 / 100;
        let ry_bot = body_cy + body_ry * 70 / 100;

        let _ = Line::new(Point::new(rx_top, ry_top), Point::new(rx_mid, ry_mid))
            .into_styled(ridge_style)
            .draw(target);
        let _ = Line::new(Point::new(rx_mid, ry_mid), Point::new(rx_bot, ry_bot))
            .into_styled(ridge_style)
            .draw(target);
    }

    // --- Head glow halo ---
    let head_glow_rx = head_rx + 3;
    let head_glow_ry = head_ry + 3;
    let _ = Ellipse::new(
        Point::new(cx - head_glow_rx, head_cy - head_glow_ry),
        Size::new((head_glow_rx * 2) as u32, (head_glow_ry * 2) as u32),
    )
    .into_styled(PrimitiveStyle::with_fill(beetle_glow_fill(color)))
    .draw(target);

    // --- Head (smaller filled ellipse, slightly darker) ---
    let head_color = darken(color, 20);
    let head_fill = PrimitiveStyle::with_fill(head_color);
    let _ = Ellipse::new(
        Point::new(cx - head_rx, head_cy - head_ry),
        Size::new((head_rx * 2) as u32, (head_ry * 2) as u32),
    )
    .into_styled(head_fill)
    .draw(target);

    let _ = Ellipse::new(
        Point::new(cx - head_rx, head_cy - head_ry),
        Size::new((head_rx * 2) as u32, (head_ry * 2) as u32),
    )
    .into_styled(PrimitiveStyle::with_stroke(lighten(head_color, 34), 1))
    .draw(target);

    // --- Eyes (round sensor optics on sides of head) ---
    let eye_r = (head_ry * 38 / 100).clamp(2, 4);
    let eye_spread = head_rx * 7 / 10;

    if opts.x_eyes {
        // X eyes (fault state)
        let x_style = PrimitiveStyle::with_stroke(Rgb565::WHITE, 2);
        for &sx in &[-1i32, 1] {
            let ex = cx + sx * eye_spread;
            let _ = Line::new(
                Point::new(ex - eye_r, head_cy - eye_r),
                Point::new(ex + eye_r, head_cy + eye_r),
            )
            .into_styled(x_style)
            .draw(target);
            let _ = Line::new(
                Point::new(ex + eye_r, head_cy - eye_r),
                Point::new(ex - eye_r, head_cy + eye_r),
            )
            .into_styled(x_style)
            .draw(target);
        }
    } else {
        // Round optics; keep as filled circles for cheap, readable pixels on ST7789.
        let eye_fill = PrimitiveStyle::with_fill(Rgb565::WHITE);
        for &sx in &[-1i32, 1] {
            let ex = cx + sx * eye_spread;
            let _ = Circle::new(Point::new(ex - eye_r, head_cy - eye_r), (eye_r * 2) as u32)
                .into_styled(eye_fill)
                .draw(target);
        }
    }

    // --- Mandibles (Mecha pincers extending from front of head) ---
    let mandible_style = PrimitiveStyle::with_stroke(darken(color, 10), 2);
    let jaw_base_y = head_cy - dir * head_ry * 8 / 10;
    let jaw_mid_y = head_cy - dir * (head_ry + size * 4 / 100);
    let jaw_tip_y = head_cy - dir * (head_ry + size * 10 / 100);

    let jaw_spread = head_rx * 4 / 10;
    let jaw_mid_spread = head_rx * 8 / 10;
    let jaw_tip_spread = head_rx * 6 / 10;

    for &sx in &[-1i32, 1] {
        let p_base = Point::new(cx + sx * jaw_spread, jaw_base_y);
        let p_mid = Point::new(cx + sx * jaw_mid_spread, jaw_mid_y);
        let p_tip = Point::new(cx + sx * jaw_tip_spread, jaw_tip_y);

        let _ = Line::new(p_base, p_mid)
            .into_styled(mandible_style)
            .draw(target);
        let _ = Line::new(p_mid, p_tip)
            .into_styled(mandible_style)
            .draw(target);

        let tooth_tip = Point::new(cx + sx * jaw_spread * 2 / 10, jaw_mid_y - dir * 2);
        let _ = Line::new(p_mid, tooth_tip)
            .into_styled(PrimitiveStyle::with_stroke(darken(color, 10), 1))
            .draw(target);
    }

    (cx, body_cy, body_r, head_cy, head_r)
}

/// Approximate cos/sin * 100 for small angles (integer arithmetic, no libm).
fn approx_cos_sin(deg: i32) -> (i32, i32) {
    match deg {
        -25..=-16 => (91, -37),
        -15..=-6 => (97, -17),
        -5..=5 => (100, deg * 2),
        6..=15 => (97, 17),
        16..=25 => (91, 37),
        26..=35 => (82, 50),
        _ => (100, 0),
    }
}

/// Draw a top-half arc (WiFi signal style) centered at (cx, cy) with radius `r`.
/// Uses 6 line segments covering roughly -150° to -30° (i.e. the upper arc).
fn draw_top_arc<D: DrawTarget<Color = Rgb565>>(
    target: &mut D,
    cx: i32,
    cy: i32,
    r: i32,
    style: &PrimitiveStyle<Rgb565>,
) {
    // Pre-computed (cos, sin) * 1000 for angles -150, -130, -110, -90, -70, -50, -30 degrees.
    const POINTS: [(i32, i32); 7] = [
        (-866, -500), // -150°
        (-643, -766), // -130°
        (-342, -940), // -110°
        (0, -1000),   // -90°
        (342, -940),  // -70°
        (643, -766),  // -50°
        (866, -500),  // -30°
    ];
    for pair in POINTS.windows(2) {
        let (c0, s0) = pair[0];
        let (c1, s1) = pair[1];
        let _ = Line::new(
            Point::new(cx + r * c0 / 1000, cy + r * s0 / 1000),
            Point::new(cx + r * c1 / 1000, cy + r * s1 / 1000),
        )
        .into_styled(*style)
        .draw(target);
    }
}

/// Draw a dashed top-half arc using dots along the arc path.
fn draw_dashed_top_arc<D: DrawTarget<Color = Rgb565>>(
    target: &mut D,
    cx: i32,
    cy: i32,
    r: i32,
    color: Rgb565,
) {
    // Same angle sample points as draw_top_arc, but render as individual dots.
    const POINTS: [(i32, i32); 7] = [
        (-866, -500),
        (-643, -766),
        (-342, -940),
        (0, -1000),
        (342, -940),
        (643, -766),
        (866, -500),
    ];
    let dot_style = PrimitiveStyle::with_fill(color);
    for &(c, s) in &POINTS {
        let px = cx + r * c / 1000;
        let py = cy + r * s / 1000;
        let _ = Circle::new(Point::new(px - 1, py - 1), 3)
            .into_styled(dot_style)
            .draw(target);
    }
}

/// Dashboard render parameters (avoids clippy::too_many_arguments).
struct DashboardParams<'a> {
    layout: &'a DisplayLayout,
    state: DisplaySystemState,
    presence_subtitle: Option<&'a str>,
    ip_address: Option<&'a str>,
    channels: &'a [DisplayChannelStatus; crate::DISPLAY_CHANNEL_CAPACITY],
    pressure: &'a DisplayPressureLevel,
    heap_percent: u8,
    width: u16,
    height: u16,
    messages_in: u32,
    messages_out: u32,
    last_active_epoch_secs: u32,
    /// F4: Busy 呼吸动画相位。
    busy_phase: bool,
    /// F6: 最近一次 LLM 调用延迟（毫秒）。
    llm_last_ms: u32,
    /// F7: 错误闪烁标志。
    error_flash: bool,
}

struct StateHeaderParams<'a> {
    layout: &'a DisplayLayout,
    state: DisplaySystemState,
    width: u16,
    height: u16,
    busy_phase: bool,
}

fn hud_top_bar_rows(height: u16) -> u16 {
    (height as i32 * 28 / DISPLAY_LAYOUT_REF_PX as i32).clamp(24, 34) as u16
}

fn hud_beetle_box(width: u16, height: u16, layout: &DisplayLayout) -> (i32, i32, i32) {
    let _ = layout;
    let size = (width.min(height) as i32 * 40 / 100).clamp(72, 100);
    let center_y = height as i32 / 2 - 1;
    let x = (width as i32 - size) / 2;
    let y = center_y - size / 2;
    (x, y, size)
}

fn hud_resource_segments(heap_percent: u8) -> u8 {
    let pct = heap_percent.min(100) as u16;
    (pct * 13).div_ceil(100) as u8
}

fn pressure_label(level: &DisplayPressureLevel) -> &'static str {
    match level {
        DisplayPressureLevel::Normal => "NORMAL",
        DisplayPressureLevel::Cautious => "CAUTIOUS",
        DisplayPressureLevel::Critical => "CRITICAL",
    }
}

fn hud_resource_bar_y(height: u16) -> i32 {
    (height as i32 * 16 / DISPLAY_LAYOUT_REF_PX as i32).clamp(10, 18)
}

fn hud_top_text_baseline(height: u16) -> i32 {
    hud_resource_bar_y(height) + 8
}

fn hud_channels_top(height: u16) -> u16 {
    (height as i32 * 48 / DISPLAY_LAYOUT_REF_PX as i32).clamp(40, 56) as u16
}

fn hud_channels_rows(height: u16) -> u16 {
    (height as i32 * 102 / DISPLAY_LAYOUT_REF_PX as i32).clamp(86, 120) as u16
}

fn hud_metrics_top(height: u16) -> u16 {
    (height as i32 * 48 / DISPLAY_LAYOUT_REF_PX as i32).clamp(40, 56) as u16
}

fn hud_metrics_rows(height: u16) -> u16 {
    (height as i32 * 130 / DISPLAY_LAYOUT_REF_PX as i32).clamp(104, 145) as u16
}

fn hud_status_top(height: u16) -> u16 {
    (height as i32 * 36 / DISPLAY_LAYOUT_REF_PX as i32).clamp(30, 44) as u16
}

fn hud_status_rows(height: u16) -> u16 {
    height.saturating_sub(hud_status_top(height))
}

fn draw_hud_grid<D: DrawTarget<Color = Rgb565>>(target: &mut D, width: u16, height: u16) {
    draw_hud_grid_region(target, width, height, 0, 0, width as i32, height as i32);
}

fn draw_hud_grid_region<D: DrawTarget<Color = Rgb565>>(
    target: &mut D,
    width: u16,
    height: u16,
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
) {
    let h_style = PrimitiveStyle::with_stroke(rgb565(4, 24, 28), 1);
    let v_style = PrimitiveStyle::with_stroke(rgb565(3, 18, 22), 1);
    let left = (width as i32 * 44 / 1000).max(8);
    let right = width as i32 - left;
    let top = (height as i32 * 50 / 1000).max(8);
    let bottom = height as i32 - top;
    let rx0 = x0.max(left);
    let rx1 = x1.min(right);
    let ry0 = y0.max(top);
    let ry1 = y1.min(bottom);

    let mut y = (height as i32 * 84 / 1000).max(14);
    let y_step = (height as i32 * 42 / 1000).max(8);
    while y < bottom {
        if y >= y0 && y < y1 && rx0 <= rx1 {
            let _ = Line::new(Point::new(rx0, y), Point::new(rx1, y))
                .into_styled(h_style)
                .draw(target);
        }
        y += y_step;
    }

    let mut x = (width as i32 * 62 / 1000).max(12);
    let x_step = (width as i32 * 44 / 1000).max(10);
    while x < right {
        if x >= x0 && x < x1 && ry0 <= ry1 {
            let _ = Line::new(Point::new(x, ry0), Point::new(x, ry1))
                .into_styled(v_style)
                .draw(target);
        }
        x += x_step;
    }
}

fn clear_rect_with_hud_grid<D: DrawTarget<Color = Rgb565>>(
    target: &mut D,
    width: u16,
    height: u16,
    x: i32,
    y: i32,
    w: u32,
    h: u32,
) {
    if w == 0 || h == 0 {
        return;
    }
    let _ = Rectangle::new(Point::new(x, y), Size::new(w, h))
        .into_styled(PrimitiveStyle::with_fill(DISPLAY_BG))
        .draw(target);
    draw_hud_grid_region(target, width, height, x, y, x + w as i32, y + h as i32);
}

fn draw_hud_frame<D: DrawTarget<Color = Rgb565>>(
    target: &mut D,
    width: u16,
    height: u16,
    accent: Rgb565,
) {
    let w = width as i32;
    let h = height as i32;
    if w < 28 || h < 28 {
        return;
    }
    let style = PrimitiveStyle::with_stroke(darken(accent, 32), 1);
    let m = 8;
    let cut = 7;
    let points = [
        (m + cut, m),
        (w - m - cut, m),
        (w - m, m + cut),
        (w - m, h - m - cut),
        (w - m - cut, h - m),
        (m + cut, h - m),
        (m, h - m - cut),
        (m, m + cut),
        (m + cut, m),
    ];
    for pair in points.windows(2) {
        let _ = Line::new(
            Point::new(pair[0].0, pair[0].1),
            Point::new(pair[1].0, pair[1].1),
        )
        .into_styled(style)
        .draw(target);
    }
}

fn draw_hud_cyber_chrome<D: DrawTarget<Color = Rgb565>>(
    target: &mut D,
    width: u16,
    height: u16,
    accent: Rgb565,
) {
    let w = width as i32;
    let h = height as i32;
    if w < 120 || h < 120 {
        return;
    }

    let cyan = PrimitiveStyle::with_stroke(darken(accent, 18), 1);
    let magenta = PrimitiveStyle::with_stroke(CYBER_MAGENTA, 1);
    let amber = PrimitiveStyle::with_stroke(STATUS_WARNING, 1);

    for idx in 0..4 {
        let y = 29 + idx * 4;
        let len = 8 + (idx % 2) * 7;
        let style = if idx == 2 { amber } else { cyan };
        let _ = Line::new(Point::new(19, y), Point::new(19 + len, y))
            .into_styled(style)
            .draw(target);
    }

    for idx in 0..5 {
        let y = 48 + idx * 20;
        let style = if idx == 1 || idx == 4 { magenta } else { cyan };
        let _ = Line::new(Point::new(w - 13, y), Point::new(w - 13, y + 8))
            .into_styled(style)
            .draw(target);
    }

    for idx in 0..7 {
        let x = 23 + idx * 8;
        let style = if idx == 3 { magenta } else { cyan };
        let _ = Line::new(Point::new(x, h - 14), Point::new(x + 3, h - 14))
            .into_styled(style)
            .draw(target);
    }
}

fn draw_hud_top_ip<D: DrawTarget<Color = Rgb565>>(
    target: &mut D,
    width: u16,
    height: u16,
    layout: &DisplayLayout,
    ip_address: Option<&str>,
    presence_subtitle: Option<&str>,
) {
    let y = hud_top_text_baseline(height);
    let x = layout.margin_x as i32 + 10;
    let label_style = MonoTextStyle::new(&FONT_6X10, TEXT_SECONDARY);
    let value_style = MonoTextStyle::new(&FONT_6X10, TEXT_PRIMARY);
    let value = ip_address
        .or(presence_subtitle)
        .unwrap_or("---.---.---.---");
    let _ = Text::new("IP", Point::new(x, y), label_style).draw(target);
    let max_chars = ((width as usize / 2).saturating_sub(22) / 6).clamp(1, 24);
    let bytes = value.as_bytes();
    let mut buf = [0u8; 24];
    let n = bytes.len().min(max_chars).min(buf.len());
    buf[..n].copy_from_slice(&bytes[..n]);
    let value = core::str::from_utf8(&buf[..n]).unwrap_or("---");
    let _ = Text::new(value, Point::new(x + 18, y), value_style).draw(target);

    let rail_y = (y + 2).min(height as i32 - 2);
    let rail_end = (x + 112).min(width as i32 / 2 - 8);
    if rail_end > x + 26 {
        let _ = Line::new(Point::new(x, rail_y), Point::new(rail_end, rail_y))
            .into_styled(PrimitiveStyle::with_stroke(HUD_RAIL, 1))
            .draw(target);
    }
}

fn draw_hud_resource_bar<D: DrawTarget<Color = Rgb565>>(
    target: &mut D,
    width: u16,
    height: u16,
    pressure: &DisplayPressureLevel,
    heap_percent: u8,
) {
    let y = hud_resource_bar_y(height);
    let seg_w = 6;
    let seg_h = 10;
    let gap = 2;
    let count = 13;
    let total_w = count * seg_w + (count - 1) * gap;
    let x = (width as i32 * 54 / 100)
        .min(width as i32 - total_w - 34)
        .max(width as i32 / 2 + 8);
    let active = hud_resource_segments(heap_percent);
    let color = pressure_accent_color(pressure);
    let inactive = PrimitiveStyle::with_fill(rgb565(5, 24, 29));
    let active_style = PrimitiveStyle::with_fill(color);
    let label_style = MonoTextStyle::new(&FONT_6X10, color);
    let _ = Text::new(
        pressure_label(pressure),
        Point::new(x - 50, hud_top_text_baseline(height)),
        label_style,
    )
    .draw(target);

    let rail_y = y + seg_h + 1;
    let _ = Line::new(Point::new(x, rail_y), Point::new(x + total_w, rail_y))
        .into_styled(PrimitiveStyle::with_stroke(HUD_RAIL, 1))
        .draw(target);

    for idx in 0..count {
        let sx = x + idx * (seg_w + gap);
        let style = if (idx as u8) < active {
            active_style
        } else {
            inactive
        };
        let _ = Rectangle::new(Point::new(sx, y), Size::new(seg_w as u32, seg_h as u32))
            .into_styled(style)
            .draw(target);
    }

    let mut pct_buf = [0u8; 5];
    let pct_str = format_pct(heap_percent, &mut pct_buf);
    let _ = Text::new(
        pct_str,
        Point::new(
            (x + total_w + 7).min(width as i32 - 24),
            hud_top_text_baseline(height),
        ),
        MonoTextStyle::new(&FONT_6X10, TEXT_PRIMARY),
    )
    .draw(target);
}

fn clear_hud_top_ip<D: DrawTarget<Color = Rgb565>>(
    target: &mut D,
    width: u16,
    height: u16,
    layout: &DisplayLayout,
) {
    let clear_w = (width as u32 / 2).max(48);
    let y = hud_top_text_baseline(height).saturating_sub(10);
    clear_rect_with_hud_grid(
        target,
        width,
        height,
        layout.margin_x as i32 + 8,
        y,
        clear_w,
        13,
    );
}

fn clear_hud_resource_bar<D: DrawTarget<Color = Rgb565>>(target: &mut D, width: u16, height: u16) {
    let clear_x = (width as i32 / 2 - 14).max(0);
    let clear_w = (width as i32 - clear_x).max(0) as u32;
    let y = hud_top_text_baseline(height).saturating_sub(10);
    clear_rect_with_hud_grid(target, width, height, clear_x, y, clear_w, 13);
}

fn draw_mic_icon<D: DrawTarget<Color = Rgb565>>(
    target: &mut D,
    x: i32,
    y: i32,
    color: Rgb565,
    active: bool,
) {
    let stroke = PrimitiveStyle::with_stroke(color, 1);
    let fill = PrimitiveStyle::with_fill(if active { color } else { TEXT_WEAK });
    let _ = Rectangle::new(Point::new(x + 5, y), Size::new(8, 15))
        .into_styled(stroke)
        .draw(target);
    let _ = Line::new(Point::new(x + 1, y + 9), Point::new(x + 1, y + 14))
        .into_styled(stroke)
        .draw(target);
    let _ = Line::new(Point::new(x + 17, y + 9), Point::new(x + 17, y + 14))
        .into_styled(stroke)
        .draw(target);
    let _ = Line::new(Point::new(x + 1, y + 14), Point::new(x + 9, y + 20))
        .into_styled(stroke)
        .draw(target);
    let _ = Line::new(Point::new(x + 17, y + 14), Point::new(x + 9, y + 20))
        .into_styled(stroke)
        .draw(target);
    let _ = Line::new(Point::new(x + 9, y + 20), Point::new(x + 9, y + 24))
        .into_styled(stroke)
        .draw(target);
    let _ = Line::new(Point::new(x + 4, y + 24), Point::new(x + 14, y + 24))
        .into_styled(stroke)
        .draw(target);
    if active {
        for idx in 0..3 {
            let sx = x + 23 + idx * 6;
            let h = 4 + idx * 2;
            let _ = Rectangle::new(Point::new(sx, y + 19 - h), Size::new(3, h as u32))
                .into_styled(fill)
                .draw(target);
        }
    }
}

fn draw_speaker_icon<D: DrawTarget<Color = Rgb565>>(
    target: &mut D,
    x: i32,
    y: i32,
    color: Rgb565,
    active: bool,
) {
    let stroke = PrimitiveStyle::with_stroke(color, 1);
    let _ = Rectangle::new(Point::new(x, y + 9), Size::new(6, 10))
        .into_styled(stroke)
        .draw(target);
    let _ = Line::new(Point::new(x + 6, y + 9), Point::new(x + 16, y + 2))
        .into_styled(stroke)
        .draw(target);
    let _ = Line::new(Point::new(x + 16, y + 2), Point::new(x + 16, y + 26))
        .into_styled(stroke)
        .draw(target);
    let _ = Line::new(Point::new(x + 16, y + 26), Point::new(x + 6, y + 19))
        .into_styled(stroke)
        .draw(target);
    if active {
        let _ = Line::new(Point::new(x + 22, y + 9), Point::new(x + 25, y + 14))
            .into_styled(stroke)
            .draw(target);
        let _ = Line::new(Point::new(x + 25, y + 14), Point::new(x + 22, y + 19))
            .into_styled(stroke)
            .draw(target);
        let _ = Line::new(Point::new(x + 29, y + 6), Point::new(x + 35, y + 14))
            .into_styled(stroke)
            .draw(target);
        let _ = Line::new(Point::new(x + 35, y + 14), Point::new(x + 29, y + 22))
            .into_styled(stroke)
            .draw(target);
    }
}

fn draw_audio_status<D: DrawTarget<Color = Rgb565>>(
    target: &mut D,
    width: u16,
    height: u16,
    layout: &DisplayLayout,
    state: DisplaySystemState,
) {
    let _ = layout;
    let y = (height as i32 - 46).max(160);
    let mic_active = matches!(state, DisplaySystemState::Recording);
    let speaker_active = matches!(state, DisplaySystemState::Playing);
    let mic_color = if mic_active {
        STATUS_SUCCESS
    } else {
        AUDIO_INACTIVE
    };
    let speaker_color = if speaker_active {
        STATUS_INFO
    } else {
        AUDIO_INACTIVE
    };
    let inactive_bar = rgb565(8, 42, 50);
    let active_bar = pressure_accent_color(&DisplayPressureLevel::Normal);
    let bar_style = PrimitiveStyle::with_fill(if mic_active { active_bar } else { inactive_bar });
    draw_mic_icon(
        target,
        (width as i32 * 97 / 1000).max(24),
        y,
        mic_color,
        mic_active,
    );
    let mic_bar_x = (width as i32 * 172 / 1000).max(48);
    for idx in 0..7 {
        let _ = Rectangle::new(Point::new(mic_bar_x + idx * 6, y + 12), Size::new(4, 7))
            .into_styled(bar_style)
            .draw(target);
    }

    draw_speaker_icon(
        target,
        (width as i32 - 132).max(width as i32 / 2 + 12),
        y,
        speaker_color,
        speaker_active,
    );
    let speaker_bar_style = PrimitiveStyle::with_fill(if speaker_active {
        STATUS_INFO
    } else {
        inactive_bar
    });
    let speaker_bar_x = (width as i32 - 87).max(width as i32 / 2 + 56);
    for idx in 0..7 {
        let _ = Rectangle::new(Point::new(speaker_bar_x + idx * 6, y + 12), Size::new(4, 7))
            .into_styled(speaker_bar_style)
            .draw(target);
    }
}

/// Render the full dashboard UI.
fn render_dashboard<D: DrawTarget<Color = Rgb565>>(target: &mut D, p: &DashboardParams<'_>) {
    let layout = p.layout;

    // --- Background fill ---
    let bg_color = DISPLAY_BG;
    let _ = Rectangle::new(Point::new(0, 0), Size::new(p.width as u32, p.height as u32))
        .into_styled(PrimitiveStyle::with_fill(bg_color))
        .draw(target);

    let beetle_color = state_accent_color(p.state);

    draw_hud_grid(target, p.width, p.height);
    draw_hud_frame(target, p.width, p.height, beetle_color);
    draw_hud_cyber_chrome(target, p.width, p.height, beetle_color);
    draw_hud_top_ip(
        target,
        p.width,
        p.height,
        layout,
        p.ip_address,
        p.presence_subtitle,
    );
    draw_hud_resource_bar(target, p.width, p.height, p.pressure, p.heap_percent);

    render_state_header_content(
        target,
        &StateHeaderParams {
            layout,
            state: p.state,
            width: p.width,
            height: p.height,
            busy_phase: p.busy_phase,
        },
    );
    render_channels_inner(target, p.channels, p.width, layout);
    render_hud_metrics(
        target,
        layout,
        p.pressure,
        p.heap_percent,
        p.width,
        p.height,
        p.messages_in,
        p.messages_out,
        p.last_active_epoch_secs,
        p.llm_last_ms,
        p.error_flash,
    );
}

fn render_state_header_content<D: DrawTarget<Color = Rgb565>>(
    target: &mut D,
    p: &StateHeaderParams<'_>,
) {
    let layout = p.layout;
    let beetle_color = state_accent_color(p.state);

    let (icon_x, icon_y, icon_size) = hud_beetle_box(p.width, p.height, layout);
    let opts = match p.state {
        DisplaySystemState::Busy => BeetleOpts {
            wings: true,
            ..Default::default()
        },
        DisplaySystemState::Recovery => BeetleOpts {
            x_eyes: true,
            ..Default::default()
        },
        DisplaySystemState::Fault => BeetleOpts {
            flipped: true,
            x_eyes: true,
            ..Default::default()
        },
        DisplaySystemState::Recording => BeetleOpts {
            listening: true,
            ..Default::default()
        },
        _ => BeetleOpts::default(),
    };
    let (cx, body_cy, body_r, head_cy, head_r) =
        draw_beetle(target, icon_x, icon_y, icon_size, beetle_color, &opts);

    match p.state {
        DisplaySystemState::Booting => {
            let dot_color = Rgb565::WHITE;
            let dot_style = PrimitiveStyle::with_fill(dot_color);
            let dot_y = body_cy;
            let spacing = body_r * 35 / 100;
            for (i, &r) in [2u32, 3, 4].iter().enumerate() {
                let dx = (i as i32 - 1) * spacing;
                let _ = Circle::new(Point::new(cx + dx - r as i32, dot_y - r as i32), r * 2)
                    .into_styled(dot_style)
                    .draw(target);
            }
            let sig_y = head_cy - head_r - 4;
            draw_dashed_top_arc(target, cx, sig_y, 7, beetle_color);
            draw_dashed_top_arc(target, cx, sig_y, 13, beetle_color);
        }
        DisplaySystemState::NoWifi => {
            let sig_y = head_cy - head_r - 4;
            let arc_style = PrimitiveStyle::with_stroke(beetle_color, 2);
            for &r in &[7i32, 13] {
                draw_top_arc(target, cx, sig_y, r, &arc_style);
            }
            let x_style = PrimitiveStyle::with_stroke(rgb565(0xff, 0x44, 0x44), 2);
            let x_sz = 6i32;
            let _ = Line::new(
                Point::new(cx - x_sz, sig_y - 13 - x_sz),
                Point::new(cx + x_sz, sig_y - 13 + x_sz),
            )
            .into_styled(x_style)
            .draw(target);
            let _ = Line::new(
                Point::new(cx + x_sz, sig_y - 13 - x_sz),
                Point::new(cx - x_sz, sig_y - 13 + x_sz),
            )
            .into_styled(x_style)
            .draw(target);
        }
        DisplaySystemState::Idle => {}
        DisplaySystemState::Pairing => {
            let pair_style = PrimitiveStyle::with_stroke(beetle_color, 2);
            let dot_style = PrimitiveStyle::with_fill(Rgb565::WHITE);
            let spacing = body_r * 32 / 100;
            let _ = Line::new(
                Point::new(cx - spacing, body_cy),
                Point::new(cx + spacing, body_cy),
            )
            .into_styled(pair_style)
            .draw(target);
            for idx in [-1, 0, 1] {
                let dx = idx * spacing;
                let _ = Circle::new(Point::new(cx + dx - 2, body_cy - 2), 4)
                    .into_styled(dot_style)
                    .draw(target);
            }
        }
        DisplaySystemState::Recovery => {
            let recover_style = PrimitiveStyle::with_stroke(Rgb565::WHITE, 2);
            let arc_y = body_cy - body_r / 4;
            draw_top_arc(target, cx, arc_y, body_r / 2, &recover_style);
            let _ = Line::new(
                Point::new(cx + body_r / 3, arc_y - 1),
                Point::new(cx + body_r / 2, arc_y - 5),
            )
            .into_styled(recover_style)
            .draw(target);
            let _ = Line::new(
                Point::new(cx + body_r / 3, arc_y - 1),
                Point::new(cx + body_r / 2 - 1, arc_y + 7),
            )
            .into_styled(recover_style)
            .draw(target);
        }
        DisplaySystemState::Fault => {
            let ex_style = PrimitiveStyle::with_stroke(Rgb565::WHITE, 2);
            let _ = Line::new(
                Point::new(cx, body_cy - body_r * 40 / 100),
                Point::new(cx, body_cy + body_r * 15 / 100),
            )
            .into_styled(ex_style)
            .draw(target);
            let dot_fill = PrimitiveStyle::with_fill(Rgb565::WHITE);
            let _ = Circle::new(Point::new(cx - 2, body_cy + body_r * 30 / 100), 4)
                .into_styled(dot_fill)
                .draw(target);
        }
        DisplaySystemState::Busy => {
            let dot_color = Rgb565::WHITE;
            let dot_style = PrimitiveStyle::with_fill(dot_color);
            let dot_y = body_cy;
            let spacing = body_r * 35 / 100;
            let sizes: [u32; 3] = if p.busy_phase { [3, 4, 5] } else { [2, 3, 2] };
            for (i, &r) in sizes.iter().enumerate() {
                let dx = (i as i32 - 1) * spacing;
                let _ = Circle::new(Point::new(cx + dx - r as i32, dot_y - r as i32), r * 2)
                    .into_styled(dot_style)
                    .draw(target);
            }
        }
        DisplaySystemState::Recording => {
            let mic_style = PrimitiveStyle::with_stroke(Rgb565::WHITE, 2);
            let mic_h = body_r * 60 / 100;
            let mic_top = body_cy - mic_h / 2;
            let mic_bot = body_cy + mic_h / 2;
            let _ = Line::new(Point::new(cx, mic_top), Point::new(cx, mic_bot))
                .into_styled(mic_style)
                .draw(target);
            let head_sz = body_r * 28 / 100;
            let _ = Circle::new(
                Point::new(cx - head_sz, mic_top - head_sz),
                (head_sz * 2) as u32,
            )
            .into_styled(PrimitiveStyle::with_stroke(Rgb565::WHITE, 2))
            .draw(target);
            let base_w = body_r * 30 / 100;
            let _ = Line::new(
                Point::new(cx - base_w, mic_bot),
                Point::new(cx + base_w, mic_bot),
            )
            .into_styled(mic_style)
            .draw(target);
            let wave_color = beetle_color;
            let wave_style = PrimitiveStyle::with_stroke(wave_color, 1);
            for layer in 1..=3i32 {
                let r = head_r + layer * 5;
                let arc_pts = 6;
                for j in (0..arc_pts).step_by(2) {
                    let a0 = 120 + j * (60 / arc_pts);
                    let a1 = 120 + (j + 1) * (60 / arc_pts);
                    let (c0, s0) = approx_cos_sin(a0 - 180);
                    let (c1, s1) = approx_cos_sin(a1 - 180);
                    let _ = Line::new(
                        Point::new(cx - r * c0 / 100, head_cy + r * s0 / 100),
                        Point::new(cx - r * c1 / 100, head_cy + r * s1 / 100),
                    )
                    .into_styled(wave_style)
                    .draw(target);
                }
                for j in (0..arc_pts).step_by(2) {
                    let a0 = 120 + j * (60 / arc_pts);
                    let a1 = 120 + (j + 1) * (60 / arc_pts);
                    let (c0, s0) = approx_cos_sin(a0 - 180);
                    let (c1, s1) = approx_cos_sin(a1 - 180);
                    let _ = Line::new(
                        Point::new(cx + r * c0 / 100, head_cy + r * s0 / 100),
                        Point::new(cx + r * c1 / 100, head_cy + r * s1 / 100),
                    )
                    .into_styled(wave_style)
                    .draw(target);
                }
            }
        }
        DisplaySystemState::Playing => {
            let speaker_style = PrimitiveStyle::with_stroke(Rgb565::WHITE, 2);
            let horn_w = body_r * 25 / 100;
            let horn_h = body_r * 50 / 100;
            let horn_left = cx - horn_w;
            let horn_right = cx;
            let horn_top = body_cy - horn_h / 2;
            let horn_bot = body_cy + horn_h / 2;
            let narrow_w = horn_w * 40 / 100;
            let _ = Line::new(
                Point::new(horn_left, body_cy - narrow_w / 2),
                Point::new(horn_right, horn_top),
            )
            .into_styled(speaker_style)
            .draw(target);
            let _ = Line::new(
                Point::new(horn_left, body_cy + narrow_w / 2),
                Point::new(horn_right, horn_bot),
            )
            .into_styled(speaker_style)
            .draw(target);
            let _ = Line::new(
                Point::new(horn_left, body_cy - narrow_w / 2),
                Point::new(horn_left, body_cy + narrow_w / 2),
            )
            .into_styled(speaker_style)
            .draw(target);
            let wave_style = PrimitiveStyle::with_stroke(beetle_color, 1);
            for layer in 1..=3i32 {
                let r = body_r * 20 / 100 + layer * 6;
                let arc_pts = 8;
                for j in (0..arc_pts).step_by(2) {
                    let a0 = 60 + j * (60 / arc_pts);
                    let a1 = 60 + (j + 1) * (60 / arc_pts);
                    let (c0, s0) = approx_cos_sin(a0 - 180);
                    let (c1, s1) = approx_cos_sin(a1 - 180);
                    let _ = Line::new(
                        Point::new(cx + r * c0 / 100, body_cy + r * s0 / 100),
                        Point::new(cx + r * c1 / 100, body_cy + r * s1 / 100),
                    )
                    .into_styled(wave_style)
                    .draw(target);
                }
            }
        }
    }
    draw_audio_status(target, p.width, p.height, layout, p.state);
}

/// Dashboard base background.
/// 仪表盘主背景色。
const DISPLAY_BG: Rgb565 = rgb565(3, 7, 10); // #03070A
const HUD_RAIL: Rgb565 = rgb565(9, 48, 56);
/// Primary text color.
/// 主文本色。
const TEXT_PRIMARY: Rgb565 = rgb565(183, 255, 255); // #B7FFFF
/// Secondary text color.
/// 次文本色。
const TEXT_SECONDARY: Rgb565 = rgb565(104, 247, 255); // #68F7FF
/// Weak text color.
/// 弱文本色。
const TEXT_WEAK: Rgb565 = rgb565(49, 96, 108); // #31606C
/// Status colors.
/// 状态强调色。
const STATUS_SUCCESS: Rgb565 = rgb565(49, 246, 163); // #31F6A3
const STATUS_WARNING: Rgb565 = rgb565(244, 184, 74); // #F4B84A
const STATUS_DANGER: Rgb565 = rgb565(255, 82, 82); // #FF5252
const STATUS_INFO: Rgb565 = rgb565(69, 191, 255); // #45BFFF
const STATUS_OFF: Rgb565 = rgb565(32, 50, 58); // #20323A
const AUDIO_INACTIVE: Rgb565 = rgb565(76, 170, 190); // #4CAABE
const CYBER_MAGENTA: Rgb565 = rgb565(245, 78, 210); // #F54ED2
#[inline]
fn state_accent_color(state: DisplaySystemState) -> Rgb565 {
    match state {
        DisplaySystemState::Booting => STATUS_WARNING,
        DisplaySystemState::Pairing => STATUS_WARNING,
        DisplaySystemState::Recovery => rgb565(255, 130, 54), // #FF8236
        DisplaySystemState::NoWifi => rgb565(104, 128, 140),  // #68808C
        DisplaySystemState::Idle => STATUS_SUCCESS,
        DisplaySystemState::Busy => STATUS_INFO,
        DisplaySystemState::Fault => STATUS_DANGER,
        DisplaySystemState::Recording => STATUS_SUCCESS,
        DisplaySystemState::Playing => STATUS_INFO,
    }
}

#[inline]
fn pressure_accent_color(level: &DisplayPressureLevel) -> Rgb565 {
    match level {
        DisplayPressureLevel::Normal => STATUS_SUCCESS,
        DisplayPressureLevel::Cautious => STATUS_WARNING,
        DisplayPressureLevel::Critical => STATUS_DANGER,
    }
}

/// Shared channel rendering logic (used by full dashboard and partial update).
fn render_channels_inner<D: DrawTarget<Color = Rgb565>>(
    target: &mut D,
    channels: &[DisplayChannelStatus; crate::DISPLAY_CHANNEL_CAPACITY],
    width: u16,
    layout: &DisplayLayout,
) {
    let _ = layout;
    let x = (width as i32 * 78 / 1000).max(24);
    let text_style = MonoTextStyle::new(&FONT_6X10, TEXT_PRIMARY);
    let weak_style = MonoTextStyle::new(&FONT_6X10, TEXT_WEAK);

    let mut row = 0usize;
    for ch in channels.iter() {
        if !ch.visible {
            continue;
        }
        if row >= 5 {
            break;
        }
        let cy = 58 + row as i32 * 20;
        let dot_color = match ch.runtime_status {
            crate::DisplayChannelRuntimeStatus::Disabled => STATUS_OFF,
            crate::DisplayChannelRuntimeStatus::Online => STATUS_SUCCESS,
            crate::DisplayChannelRuntimeStatus::Configured
            | crate::DisplayChannelRuntimeStatus::Waiting
            | crate::DisplayChannelRuntimeStatus::WaitingWallClock
            | crate::DisplayChannelRuntimeStatus::Suspended
            | crate::DisplayChannelRuntimeStatus::Connecting
            | crate::DisplayChannelRuntimeStatus::CoolingDown => STATUS_WARNING,
            crate::DisplayChannelRuntimeStatus::Failed => STATUS_DANGER,
        };
        let _ = Circle::new(Point::new(x - 3, cy - 3), 6)
            .into_styled(PrimitiveStyle::with_fill(dot_color))
            .draw(target);

        let name_style = if ch.enabled { text_style } else { weak_style };
        let label = if ch.display_label.is_empty() {
            ch.name
        } else {
            ch.display_label
        };
        let line_end = (x + 42 - (row as i32 % 3) * 4).min(width as i32 / 2 - 38);
        let _ = Line::new(Point::new(x + 8, cy), Point::new(line_end, cy))
            .into_styled(PrimitiveStyle::with_stroke(dot_color, 1))
            .draw(target);
        let _ = Text::new(label, Point::new(x + 52, cy + 4), name_style).draw(target);

        row += 1;
    }
}

/// Partial update: repaint only the IP subtitle region。
fn render_ip_partial<D: DrawTarget<Color = Rgb565>>(
    target: &mut D,
    ip: &str,
    presence_subtitle: Option<&str>,
    width: u16,
    height: u16,
    layout: &DisplayLayout,
) {
    clear_hud_top_ip(target, width, height, layout);
    draw_hud_top_ip(target, width, height, layout, Some(ip), presence_subtitle);
}

/// Partial update: repaint only the state header region used by steady-state status flips.
struct StateHeaderSnapshot {
    busy_phase: bool,
}

fn render_state_header_partial<D: DrawTarget<Color = Rgb565>>(
    target: &mut D,
    state: DisplaySystemState,
    snapshot: StateHeaderSnapshot,
    width: u16,
    height: u16,
    layout: &DisplayLayout,
) {
    let (beetle_x, beetle_y, beetle_size) = hud_beetle_box(width, height, layout);
    let beetle_pad = 18;
    clear_rect_with_hud_grid(
        target,
        width,
        height,
        beetle_x - beetle_pad,
        beetle_y - beetle_pad,
        (beetle_size + beetle_pad * 2) as u32,
        (beetle_size + beetle_pad * 2) as u32,
    );
    let audio_y = (height as i32 - 54).max(hud_status_top(height) as i32);
    clear_rect_with_hud_grid(
        target,
        width,
        height,
        12,
        audio_y,
        width.saturating_sub(24) as u32,
        (height as i32 - audio_y).max(1) as u32,
    );

    render_state_header_content(
        target,
        &StateHeaderParams {
            layout,
            state,
            width,
            height,
            busy_phase: snapshot.busy_phase,
        },
    );
    draw_hud_cyber_chrome(target, width, height, state_accent_color(state));
}

/// Partial update: repaint only the channel status (middle) region.
fn render_channels_partial<D: DrawTarget<Color = Rgb565>>(
    target: &mut D,
    channels: &[DisplayChannelStatus; crate::DISPLAY_CHANNEL_CAPACITY],
    width: u16,
    height: u16,
    layout: &DisplayLayout,
) {
    let top = hud_channels_top(height);
    let rows = hud_channels_rows(height);
    let (beetle_x, _, _) = hud_beetle_box(width, height, layout);
    let clear_right = (beetle_x - 8).max(13);
    let clear_w = (clear_right - 12).max(1) as u32;
    clear_rect_with_hud_grid(target, width, height, 12, top as i32, clear_w, rows as u32);
    render_channels_inner(target, channels, width, layout);
    draw_hud_cyber_chrome(target, width, height, TEXT_SECONDARY);
}

/// Footer partial-update parameters (avoids clippy::too_many_arguments).
struct FooterPartialParams {
    heap_percent: u8,
    width: u16,
    height: u16,
    messages_in: u32,
    messages_out: u32,
    last_active_epoch_secs: u32,
    /// F6: LLM 延迟。
    llm_last_ms: u32,
    /// F7: 错误闪烁标志。
    error_flash: bool,
}

fn format_compact_ms(ms: u32, buf: &mut [u8; 10]) -> &str {
    if ms == 0 {
        return "--";
    }
    let mut pos = 0usize;
    if ms >= 1000 {
        pos = write_u32_to_buf(ms / 1000, buf, pos);
        if pos + 2 < buf.len() {
            buf[pos] = b'.';
            buf[pos + 1] = b'0' + ((ms % 1000) / 100) as u8;
            buf[pos + 2] = b's';
            pos += 3;
        }
    } else {
        pos = write_u32_to_buf(ms, buf, pos);
        if pos + 1 < buf.len() {
            buf[pos] = b'm';
            buf[pos + 1] = b's';
            pos += 2;
        }
    }
    utf8_ascii_digits_or_fallback(&buf[..pos])
}

fn format_epoch_hhmm(epoch_secs: u32, buf: &mut [u8; 5]) -> &str {
    if epoch_secs == 0 {
        buf.copy_from_slice(b"--:--");
        return utf8_ascii_digits_or_fallback(buf);
    }
    let secs_of_day = epoch_secs % 86400;
    let h = ((secs_of_day / 3600) % 24) as u8;
    let m = ((secs_of_day % 3600) / 60) as u8;
    buf[0] = b'0' + h / 10;
    buf[1] = b'0' + h % 10;
    buf[2] = b':';
    buf[3] = b'0' + m / 10;
    buf[4] = b'0' + m % 10;
    utf8_ascii_digits_or_fallback(buf)
}

#[allow(clippy::too_many_arguments)]
fn render_hud_metrics<D: DrawTarget<Color = Rgb565>>(
    target: &mut D,
    layout: &DisplayLayout,
    level: &DisplayPressureLevel,
    heap_percent: u8,
    width: u16,
    _height: u16,
    messages_in: u32,
    messages_out: u32,
    last_active_epoch_secs: u32,
    llm_last_ms: u32,
    error_flash: bool,
) {
    let _ = layout;
    let x = (width as i32 * 713 / 1000)
        .min(width as i32 - 92)
        .max(width as i32 / 2 + 48);
    let label_style = MonoTextStyle::new(&FONT_6X10, TEXT_SECONDARY);
    let value_style = MonoTextStyle::new(&FONT_6X10, TEXT_PRIMARY);
    let accent = if error_flash {
        STATUS_DANGER
    } else {
        pressure_accent_color(level)
    };
    let rail_x = x - 10;
    let rail_style = PrimitiveStyle::with_stroke(HUD_RAIL, 1);
    let _ = Line::new(Point::new(rail_x, 51), Point::new(rail_x, 169))
        .into_styled(rail_style)
        .draw(target);
    for y in [55, 73, 89, 118, 136, 166] {
        let _ = Line::new(Point::new(rail_x, y), Point::new(rail_x + 4, y))
            .into_styled(rail_style)
            .draw(target);
    }

    let _ = Text::new("IO", Point::new(x, 55), label_style).draw(target);

    let mut in_buf = [0u8; 10];
    let in_len = write_u32_to_buf(messages_in, &mut in_buf, 0);
    let _ = Text::new("IN", Point::new(x, 73), label_style).draw(target);
    let _ = Text::new(
        utf8_ascii_digits_or_fallback(&in_buf[..in_len]),
        Point::new(x + 18, 73),
        value_style,
    )
    .draw(target);

    let mut out_buf = [0u8; 10];
    let out_len = write_u32_to_buf(messages_out, &mut out_buf, 0);
    let _ = Text::new("OUT", Point::new(x, 89), label_style).draw(target);
    let _ = Text::new(
        utf8_ascii_digits_or_fallback(&out_buf[..out_len]),
        Point::new(x + 24, 89),
        value_style,
    )
    .draw(target);

    let mut llm_buf = [0u8; 10];
    let llm_str = format_compact_ms(llm_last_ms, &mut llm_buf);
    let _ = Text::new("COST", Point::new(x, 118), label_style).draw(target);
    let _ = Text::new(
        llm_str,
        Point::new(x + 34, 118),
        MonoTextStyle::new(&FONT_6X10, accent),
    )
    .draw(target);

    let mut t_buf = [0u8; 5];
    let time_str = format_epoch_hhmm(last_active_epoch_secs, &mut t_buf);
    let _ = Text::new("LAST", Point::new(x, 136), label_style).draw(target);
    let _ = Text::new(time_str, Point::new(x + 34, 136), value_style).draw(target);

    let _ = Text::new("LOAD", Point::new(x, 166), label_style).draw(target);
    let load_active = (heap_percent.min(100) as u16 * 6).div_ceil(100) as u8;
    let inactive = PrimitiveStyle::with_fill(rgb565(5, 24, 29));
    let active = PrimitiveStyle::with_fill(accent);
    let load_x = x + 34;
    for idx in 0..6 {
        let style = if (idx as u8) < load_active {
            active
        } else {
            inactive
        };
        let _ = Rectangle::new(Point::new(load_x + idx * 7, 157), Size::new(5, 10))
            .into_styled(style)
            .draw(target);
    }
}

/// Partial update: repaint only the footer pressure + progress bar region.
fn render_pressure_partial<D: DrawTarget<Color = Rgb565>>(
    target: &mut D,
    level: &DisplayPressureLevel,
    layout: &DisplayLayout,
    fp: &FooterPartialParams,
) {
    let x = (fp.width as i32 * 70 / 100).max(fp.width as i32 / 2 + 36);
    clear_rect_with_hud_grid(
        target,
        fp.width,
        fp.height,
        x - 8,
        hud_metrics_top(fp.height) as i32,
        (fp.width as i32 - x + 8).max(1) as u32,
        hud_metrics_rows(fp.height) as u32,
    );
    render_hud_metrics(
        target,
        layout,
        level,
        fp.heap_percent,
        fp.width,
        fp.height,
        fp.messages_in,
        fp.messages_out,
        fp.last_active_epoch_secs,
        fp.llm_last_ms,
        fp.error_flash,
    );
    draw_hud_cyber_chrome(target, fp.width, fp.height, pressure_accent_color(level));
}

fn render_resource_partial<D: DrawTarget<Color = Rgb565>>(
    target: &mut D,
    width: u16,
    height: u16,
    level: &DisplayPressureLevel,
    heap_percent: u8,
) {
    clear_hud_resource_bar(target, width, height);
    draw_hud_resource_bar(target, width, height, level, heap_percent);
}

/// 将仅含 ASCII 数字/标点的缓冲区转为 `&str`；异常时回退为 `"?"`（release 不 panic）。
fn utf8_ascii_digits_or_fallback(buf: &[u8]) -> &str {
    match std::str::from_utf8(buf) {
        Ok(s) => s,
        Err(_) => "?",
    }
}

/// Write a u32 value into a byte buffer at `pos`, return new pos.
fn write_u32_to_buf(val: u32, buf: &mut [u8], mut pos: usize) -> usize {
    if val == 0 {
        if pos < buf.len() {
            buf[pos] = b'0';
            pos += 1;
        }
        return pos;
    }
    // Max u32 is 10 digits; write into temp then copy.
    let mut tmp = [0u8; 10];
    let mut n = val;
    let mut i = 0;
    while n > 0 {
        tmp[i] = b'0' + (n % 10) as u8;
        n /= 10;
        i += 1;
    }
    // Reverse copy
    for j in (0..i).rev() {
        if pos < buf.len() {
            buf[pos] = tmp[j];
            pos += 1;
        }
    }
    pos
}

/// Format a percentage value into a static buffer (no heap alloc).
fn format_pct(val: u8, buf: &mut [u8; 5]) -> &str {
    let val = val.min(100);
    let mut pos = 0;
    if val >= 100 {
        buf[pos] = b'1';
        pos += 1;
        buf[pos] = b'0';
        pos += 1;
        buf[pos] = b'0';
        pos += 1;
    } else if val >= 10 {
        buf[pos] = b'0' + val / 10;
        pos += 1;
        buf[pos] = b'0' + val % 10;
        pos += 1;
    } else {
        buf[pos] = b'0' + val;
        pos += 1;
    }
    buf[pos] = b'%';
    pos += 1;
    core::str::from_utf8(&buf[..pos]).unwrap_or("?%")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::display::{default_disabled_display_config, DisplayBus, DisplayDriver};
    use embedded_graphics_core::{
        draw_target::DrawTarget,
        geometry::{OriginDimensions, Size},
        pixelcolor::{Rgb565, RgbColor},
        Pixel,
    };

    #[derive(Default)]
    struct FakeBackend {
        flush_calls: usize,
        flush_rows_calls: Vec<(u16, u16)>,
    }

    struct PixelProbeBackend {
        width: u32,
        height: u32,
        pixels: Vec<Pixel<Rgb565>>,
        flush_calls: usize,
        flush_rows_calls: Vec<(u16, u16)>,
    }

    struct FrameProbeBackend {
        width: u32,
        height: u32,
        pixels: Vec<Rgb565>,
        flush_calls: usize,
        flush_rows_calls: Vec<(u16, u16)>,
    }

    impl PixelProbeBackend {
        fn new(width: u16, height: u16) -> Self {
            Self {
                width: width as u32,
                height: height as u32,
                pixels: Vec::new(),
                flush_calls: 0,
                flush_rows_calls: Vec::new(),
            }
        }

        fn count_color_in_rect(&self, color: Rgb565, x0: i32, y0: i32, x1: i32, y1: i32) -> usize {
            self.pixels
                .iter()
                .filter(|Pixel(point, pixel_color)| {
                    *pixel_color == color
                        && point.x >= x0
                        && point.x < x1
                        && point.y >= y0
                        && point.y < y1
                })
                .count()
        }
    }

    impl FrameProbeBackend {
        fn new(width: u16, height: u16) -> Self {
            Self {
                width: width as u32,
                height: height as u32,
                pixels: vec![DISPLAY_BG; width as usize * height as usize],
                flush_calls: 0,
                flush_rows_calls: Vec::new(),
            }
        }

        fn index(&self, x: i32, y: i32) -> Option<usize> {
            if x < 0 || y < 0 || x as u32 >= self.width || y as u32 >= self.height {
                return None;
            }
            Some(y as usize * self.width as usize + x as usize)
        }

        fn count_color_in_rect(&self, color: Rgb565, x0: i32, y0: i32, x1: i32, y1: i32) -> usize {
            let x0 = x0.max(0);
            let y0 = y0.max(0);
            let x1 = x1.min(self.width as i32).max(x0);
            let y1 = y1.min(self.height as i32).max(y0);
            let mut count = 0usize;
            for y in y0..y1 {
                for x in x0..x1 {
                    if self.pixels[self.index(x, y).expect("bounded pixel index")] == color {
                        count += 1;
                    }
                }
            }
            count
        }

        fn write_bmp(&self, path: &std::path::Path) -> std::io::Result<()> {
            use std::io::Write;

            let row_stride = (self.width as usize * 3).div_ceil(4) * 4;
            let pixel_bytes = row_stride * self.height as usize;
            let file_size = 54 + pixel_bytes;
            let mut file = std::fs::File::create(path)?;

            file.write_all(b"BM")?;
            file.write_all(&(file_size as u32).to_le_bytes())?;
            file.write_all(&[0u8; 4])?;
            file.write_all(&(54u32).to_le_bytes())?;
            file.write_all(&(40u32).to_le_bytes())?;
            file.write_all(&(self.width as i32).to_le_bytes())?;
            file.write_all(&(self.height as i32).to_le_bytes())?;
            file.write_all(&(1u16).to_le_bytes())?;
            file.write_all(&(24u16).to_le_bytes())?;
            file.write_all(&(0u32).to_le_bytes())?;
            file.write_all(&(pixel_bytes as u32).to_le_bytes())?;
            file.write_all(&(2835i32).to_le_bytes())?;
            file.write_all(&(2835i32).to_le_bytes())?;
            file.write_all(&(0u32).to_le_bytes())?;
            file.write_all(&(0u32).to_le_bytes())?;

            let padding = vec![0u8; row_stride - self.width as usize * 3];
            for y in (0..self.height as usize).rev() {
                for x in 0..self.width as usize {
                    let color = self.pixels[y * self.width as usize + x];
                    let r = (color.r() as u16 * 255 / 31) as u8;
                    let g = (color.g() as u16 * 255 / 63) as u8;
                    let b = (color.b() as u16 * 255 / 31) as u8;
                    file.write_all(&[b, g, r])?;
                }
                file.write_all(&padding)?;
            }
            Ok(())
        }
    }

    impl OriginDimensions for FakeBackend {
        fn size(&self) -> Size {
            Size::new(240, 240)
        }
    }

    impl DrawTarget for FakeBackend {
        type Color = Rgb565;
        type Error = Infallible;

        fn draw_iter<I>(&mut self, _pixels: I) -> core::result::Result<(), Self::Error>
        where
            I: IntoIterator<Item = Pixel<Self::Color>>,
        {
            Ok(())
        }
    }

    impl OriginDimensions for PixelProbeBackend {
        fn size(&self) -> Size {
            Size::new(self.width, self.height)
        }
    }

    impl OriginDimensions for FrameProbeBackend {
        fn size(&self) -> Size {
            Size::new(self.width, self.height)
        }
    }

    impl DrawTarget for PixelProbeBackend {
        type Color = Rgb565;
        type Error = Infallible;

        fn draw_iter<I>(&mut self, pixels: I) -> core::result::Result<(), Self::Error>
        where
            I: IntoIterator<Item = Pixel<Self::Color>>,
        {
            self.pixels.extend(pixels);
            Ok(())
        }
    }

    impl DrawTarget for FrameProbeBackend {
        type Color = Rgb565;
        type Error = Infallible;

        fn draw_iter<I>(&mut self, pixels: I) -> core::result::Result<(), Self::Error>
        where
            I: IntoIterator<Item = Pixel<Self::Color>>,
        {
            for Pixel(point, color) in pixels {
                if let Some(index) = self.index(point.x, point.y) {
                    self.pixels[index] = color;
                }
            }
            Ok(())
        }
    }

    impl FlushRgb565 for FakeBackend {
        fn flush(&mut self, _offset_x: i16, _offset_y: i16) -> Result<()> {
            self.flush_calls += 1;
            Ok(())
        }

        fn flush_rows(&mut self, _offset_x: i16, _offset_y: i16, ry: u16, rh: u16) -> Result<()> {
            self.flush_rows_calls.push((ry, rh));
            Ok(())
        }
    }

    impl FlushRgb565 for PixelProbeBackend {
        fn flush(&mut self, _offset_x: i16, _offset_y: i16) -> Result<()> {
            self.flush_calls += 1;
            Ok(())
        }

        fn flush_rows(&mut self, _offset_x: i16, _offset_y: i16, ry: u16, rh: u16) -> Result<()> {
            self.flush_rows_calls.push((ry, rh));
            Ok(())
        }
    }

    impl FlushRgb565 for FrameProbeBackend {
        fn flush(&mut self, _offset_x: i16, _offset_y: i16) -> Result<()> {
            self.flush_calls += 1;
            Ok(())
        }

        fn flush_rows(&mut self, _offset_x: i16, _offset_y: i16, ry: u16, rh: u16) -> Result<()> {
            self.flush_rows_calls.push((ry, rh));
            Ok(())
        }
    }

    fn sample_channels() -> [DisplayChannelStatus; crate::DISPLAY_CHANNEL_CAPACITY] {
        [
            DisplayChannelStatus {
                name: "qq",
                display_label: "QQ",
                enabled: true,
                healthy: true,
                visible: true,
                runtime_status: crate::DisplayChannelRuntimeStatus::Online,
                consecutive_failures: 0,
            },
            DisplayChannelStatus {
                name: "telegram",
                display_label: "TG",
                enabled: true,
                healthy: true,
                visible: true,
                runtime_status: crate::DisplayChannelRuntimeStatus::Connecting,
                consecutive_failures: 0,
            },
            DisplayChannelStatus {
                name: "web",
                display_label: "WEB",
                enabled: true,
                healthy: false,
                visible: true,
                runtime_status: crate::DisplayChannelRuntimeStatus::Failed,
                consecutive_failures: 2,
            },
            DisplayChannelStatus {
                name: "feishu",
                display_label: "FS",
                enabled: false,
                healthy: false,
                visible: true,
                runtime_status: crate::DisplayChannelRuntimeStatus::Disabled,
                consecutive_failures: 0,
            },
            DisplayChannelStatus {
                name: "system",
                display_label: "SYS",
                enabled: true,
                healthy: true,
                visible: true,
                runtime_status: crate::DisplayChannelRuntimeStatus::Online,
                consecutive_failures: 0,
            },
        ]
    }

    fn wide_display_config() -> DisplayConfig {
        let mut config = default_disabled_display_config();
        config.width = 320;
        config.height = 240;
        config
    }

    fn render_preview_frame(
        state: DisplaySystemState,
        pressure: DisplayPressureLevel,
        heap_percent: u8,
    ) -> FrameProbeBackend {
        let config = wide_display_config();
        let layout = compute_layout(config.width, config.height);
        let mut backend = FrameProbeBackend::new(config.width, config.height);

        dispatch_display_command(
            &mut backend,
            &config,
            &layout,
            &DisplayCommand::RefreshDashboard {
                state,
                presence_subtitle: None,
                ip_address: Some("192.168.4.1".to_string()),
                channels: sample_channels(),
                pressure,
                heap_percent,
                messages_in: 128,
                messages_out: 122,
                last_active_epoch_secs: 52320,
                uptime_secs: 86,
                busy_phase: matches!(state, DisplaySystemState::Busy),
                llm_last_ms: 842,
                error_flash: false,
            },
        )
        .unwrap();

        backend
    }

    #[test]
    fn state_header_update_uses_partial_flush_instead_of_full_dashboard_flush() {
        let config = default_disabled_display_config();
        let layout = compute_layout(config.width, config.height);
        let mut backend = FakeBackend::default();

        dispatch_display_command(
            &mut backend,
            &config,
            &layout,
            &DisplayCommand::UpdateStateHeader {
                state: DisplaySystemState::Busy,
                presence_subtitle: None,
                ip_address: Some("192.168.2.101".to_string()),
                uptime_secs: 42,
                busy_phase: true,
            },
        )
        .unwrap();

        assert_eq!(backend.flush_calls, 0);
        assert_eq!(backend.flush_rows_calls.len(), 1);
        assert_eq!(
            backend.flush_rows_calls[0],
            (
                hud_status_top(config.height),
                hud_status_rows(config.height)
            )
        );
    }

    #[test]
    fn ip_update_flushes_top_hud_bar_without_full_dashboard_flush() {
        let config = default_disabled_display_config();
        let layout = compute_layout(config.width, config.height);
        let mut backend = FakeBackend::default();

        dispatch_display_command(
            &mut backend,
            &config,
            &layout,
            &DisplayCommand::UpdateIp {
                ip: "10.0.0.42".to_string(),
                presence_subtitle: None,
                uptime_secs: 3600,
            },
        )
        .unwrap();

        assert_eq!(backend.flush_calls, 0);
        assert_eq!(
            backend.flush_rows_calls,
            vec![(0, hud_top_bar_rows(config.height))]
        );
    }

    #[test]
    fn top_partial_updates_preserve_grid_backdrop() {
        let config = wide_display_config();
        let layout = compute_layout(config.width, config.height);
        let grid_color = rgb565(4, 24, 28);

        let mut ip_backend =
            render_preview_frame(DisplaySystemState::Idle, DisplayPressureLevel::Normal, 42);
        let left_grid_before = ip_backend.count_color_in_rect(grid_color, 120, 20, 160, 21);
        assert!(
            left_grid_before > 20,
            "full HUD render must have subtle top-left grid evidence"
        );
        dispatch_display_command(
            &mut ip_backend,
            &config,
            &layout,
            &DisplayCommand::UpdateIp {
                ip: "192.168.1.86".to_string(),
                presence_subtitle: None,
                uptime_secs: 3600,
            },
        )
        .unwrap();
        let left_grid_after = ip_backend.count_color_in_rect(grid_color, 120, 20, 160, 21);
        assert!(
            left_grid_after >= left_grid_before,
            "IP partial refresh must not leave a solid black top-left patch"
        );

        let mut pressure_backend =
            render_preview_frame(DisplaySystemState::Idle, DisplayPressureLevel::Normal, 42);
        let right_grid_before = pressure_backend.count_color_in_rect(grid_color, 300, 20, 306, 21);
        assert!(
            right_grid_before > 0,
            "full HUD render must have subtle top-right grid evidence"
        );
        dispatch_display_command(
            &mut pressure_backend,
            &config,
            &layout,
            &DisplayCommand::UpdatePressure {
                level: DisplayPressureLevel::Normal,
                heap_percent: 14,
                messages_in: 0,
                messages_out: 0,
                last_active_epoch_secs: 0,
                llm_last_ms: 0,
                error_flash: false,
            },
        )
        .unwrap();
        let right_grid_after = pressure_backend.count_color_in_rect(grid_color, 300, 20, 306, 21);
        assert!(
            right_grid_after >= right_grid_before,
            "resource partial refresh must not leave a solid black top-right patch"
        );
    }

    #[test]
    fn side_partial_updates_do_not_erase_center_beetle() {
        let config = wide_display_config();
        let layout = compute_layout(config.width, config.height);
        let (beetle_x, beetle_y, beetle_size) =
            hud_beetle_box(config.width, config.height, &layout);

        let mut channel_backend =
            render_preview_frame(DisplaySystemState::Idle, DisplayPressureLevel::Normal, 42);
        let channel_center_before = channel_backend.count_color_in_rect(
            STATUS_SUCCESS,
            beetle_x,
            beetle_y,
            beetle_x + beetle_size,
            beetle_y + beetle_size,
        );
        assert!(channel_center_before > 300);
        dispatch_display_command(
            &mut channel_backend,
            &config,
            &layout,
            &DisplayCommand::UpdateChannels {
                channels: sample_channels(),
            },
        )
        .unwrap();
        let channel_center_after = channel_backend.count_color_in_rect(
            STATUS_SUCCESS,
            beetle_x,
            beetle_y,
            beetle_x + beetle_size,
            beetle_y + beetle_size,
        );
        assert_eq!(
            channel_center_after, channel_center_before,
            "channel partial refresh must not erase the centered beetle"
        );

        let mut pressure_backend =
            render_preview_frame(DisplaySystemState::Idle, DisplayPressureLevel::Normal, 42);
        let pressure_center_before = pressure_backend.count_color_in_rect(
            STATUS_SUCCESS,
            beetle_x,
            beetle_y,
            beetle_x + beetle_size,
            beetle_y + beetle_size,
        );
        dispatch_display_command(
            &mut pressure_backend,
            &config,
            &layout,
            &DisplayCommand::UpdatePressure {
                level: DisplayPressureLevel::Normal,
                heap_percent: 14,
                messages_in: 0,
                messages_out: 0,
                last_active_epoch_secs: 0,
                llm_last_ms: 0,
                error_flash: false,
            },
        )
        .unwrap();
        let pressure_center_after = pressure_backend.count_color_in_rect(
            STATUS_SUCCESS,
            beetle_x,
            beetle_y,
            beetle_x + beetle_size,
            beetle_y + beetle_size,
        );
        assert_eq!(
            pressure_center_after, pressure_center_before,
            "pressure partial refresh must not erase the centered beetle"
        );
    }

    #[test]
    fn idle_beetle_has_round_eyes_and_no_body_check_mark() {
        let config = wide_display_config();
        let layout = compute_layout(config.width, config.height);
        let backend =
            render_preview_frame(DisplaySystemState::Idle, DisplayPressureLevel::Normal, 42);
        let (beetle_x, beetle_y, beetle_size) =
            hud_beetle_box(config.width, config.height, &layout);
        let cx = beetle_x + beetle_size / 2;
        let head_rx = beetle_size * 16 / 100;
        let head_ry = beetle_size * 11 / 100;
        let head_cy = beetle_y + beetle_size * 30 / 100;
        let eye_r = (head_ry * 38 / 100).clamp(2, 4);
        let eye_spread = head_rx * 7 / 10;

        for sx in [-1i32, 1] {
            let ex = cx + sx * eye_spread;
            let center_pixels =
                backend.count_color_in_rect(Rgb565::WHITE, ex, head_cy, ex + 1, head_cy + 1);
            assert_eq!(center_pixels, 1, "round eye center must be lit");

            for (corner_x, corner_y) in [
                (ex - eye_r, head_cy - eye_r),
                (ex + eye_r - 1, head_cy - eye_r),
                (ex - eye_r, head_cy + eye_r - 1),
                (ex + eye_r - 1, head_cy + eye_r - 1),
            ] {
                assert_eq!(
                    backend.count_color_in_rect(
                        Rgb565::WHITE,
                        corner_x,
                        corner_y,
                        corner_x + 1,
                        corner_y + 1
                    ),
                    0,
                    "round eye bounding-box corners must stay unfilled"
                );
            }
        }

        let body_cy = beetle_y + beetle_size * 62 / 100;
        let body_check_pixels = backend.count_color_in_rect(
            Rgb565::WHITE,
            cx - 18,
            body_cy - 10,
            cx + 18,
            body_cy + 14,
        );
        assert_eq!(
            body_check_pixels, 0,
            "idle beetle body must not carry the old white check mark"
        );
    }

    #[test]
    fn release_hud_surface_has_material_highlights_and_data_rails() {
        let backend =
            render_preview_frame(DisplaySystemState::Idle, DisplayPressureLevel::Normal, 42);
        let beetle_material_highlight = Rgb565::new(11, 63, 25);
        let hud_rail = rgb565(9, 48, 56);

        let material_pixels =
            backend.count_color_in_rect(beetle_material_highlight, 112, 56, 208, 172);
        assert!(
            material_pixels > 18,
            "release HUD beetle needs visible material highlights, not flat demo fill"
        );

        let rail_pixels = backend.count_color_in_rect(hud_rail, 0, 0, 320, 240);
        assert!(
            rail_pixels > 48,
            "release HUD needs restrained data rails to feel like a product surface"
        );
    }

    #[test]
    fn pressure_update_flushes_resource_bar_and_footer_without_full_dashboard_flush() {
        let config = default_disabled_display_config();
        let layout = compute_layout(config.width, config.height);
        let mut backend = FakeBackend::default();

        dispatch_display_command(
            &mut backend,
            &config,
            &layout,
            &DisplayCommand::UpdatePressure {
                level: DisplayPressureLevel::Critical,
                heap_percent: 91,
                messages_in: 21,
                messages_out: 18,
                last_active_epoch_secs: 3720,
                llm_last_ms: 5200,
                error_flash: true,
            },
        )
        .unwrap();

        assert_eq!(backend.flush_calls, 0);
        assert_eq!(backend.flush_rows_calls.len(), 2);
        assert_eq!(
            backend.flush_rows_calls[0],
            (8, hud_top_bar_rows(config.height).saturating_add(4))
        );
        assert_eq!(
            backend.flush_rows_calls[1],
            (
                hud_metrics_top(config.height),
                hud_metrics_rows(config.height)
            )
        );
    }

    #[test]
    fn refresh_dashboard_places_status_beetle_in_center_hud_region() {
        let config = default_disabled_display_config();
        let layout = compute_layout(config.width, config.height);
        let mut backend = PixelProbeBackend::new(config.width, config.height);

        dispatch_display_command(
            &mut backend,
            &config,
            &layout,
            &DisplayCommand::RefreshDashboard {
                state: DisplaySystemState::Idle,
                presence_subtitle: None,
                ip_address: Some("192.168.4.1".to_string()),
                channels: sample_channels(),
                pressure: DisplayPressureLevel::Normal,
                heap_percent: 42,
                messages_in: 7,
                messages_out: 5,
                last_active_epoch_secs: 3600,
                uptime_secs: 86,
                busy_phase: false,
                llm_last_ms: 842,
                error_flash: false,
            },
        )
        .unwrap();

        let center_accent_pixels = backend.count_color_in_rect(
            STATUS_SUCCESS,
            config.width as i32 / 2 - 14,
            layout.header_top as i32 + 28,
            config.width as i32 / 2 + 14,
            layout.middle_top as i32,
        );

        assert!(
            center_accent_pixels > 40,
            "HUD V2 keeps the beetle as the centered state object"
        );
        assert_eq!(backend.flush_calls, 1);
        assert!(backend.flush_rows_calls.is_empty());
    }

    #[test]
    fn refresh_dashboard_renders_top_resource_segments_from_heap_pressure() {
        let config = wide_display_config();
        let layout = compute_layout(config.width, config.height);
        let mut backend = PixelProbeBackend::new(config.width, config.height);

        dispatch_display_command(
            &mut backend,
            &config,
            &layout,
            &DisplayCommand::RefreshDashboard {
                state: DisplaySystemState::Busy,
                presence_subtitle: None,
                ip_address: Some("10.0.0.42".to_string()),
                channels: sample_channels(),
                pressure: DisplayPressureLevel::Cautious,
                heap_percent: 78,
                messages_in: 19,
                messages_out: 13,
                last_active_epoch_secs: 3720,
                uptime_secs: 7200,
                busy_phase: true,
                llm_last_ms: 1800,
                error_flash: false,
            },
        )
        .unwrap();

        let top_resource_pixels = backend.count_color_in_rect(STATUS_WARNING, 172, 16, 276, 26);

        assert!(
            top_resource_pixels > 80,
            "HUD V2 shows the confirmed 13-slot top resource strip"
        );

        let pressure_label_pixels = backend.count_color_in_rect(STATUS_WARNING, 118, 8, 170, 24);
        assert!(
            pressure_label_pixels > 24,
            "top resource heading should be the pressure status word, not a generic RESOURCE title"
        );
        let generic_title_pixels = backend.count_color_in_rect(TEXT_SECONDARY, 118, 8, 170, 24);
        assert_eq!(
            generic_title_pixels, 0,
            "top resource area must not render the old RESOURCE title color"
        );
    }

    #[test]
    fn wide_dashboard_matches_confirmed_hud_v2_coordinate_surface() {
        let config = wide_display_config();
        let layout = compute_layout(config.width, config.height);
        let mut backend = PixelProbeBackend::new(config.width, config.height);

        dispatch_display_command(
            &mut backend,
            &config,
            &layout,
            &DisplayCommand::RefreshDashboard {
                state: DisplaySystemState::Idle,
                presence_subtitle: None,
                ip_address: Some("192.168.4.1".to_string()),
                channels: sample_channels(),
                pressure: DisplayPressureLevel::Normal,
                heap_percent: 42,
                messages_in: 128,
                messages_out: 122,
                last_active_epoch_secs: 52320,
                uptime_secs: 86,
                busy_phase: false,
                llm_last_ms: 842,
                error_flash: false,
            },
        )
        .unwrap();

        let center_body = backend.count_color_in_rect(STATUS_SUCCESS, 102, 47, 218, 179);
        assert!(
            center_body > 400,
            "confirmed HUD V2 keeps the beetle as the large center object"
        );
        let lower_body = backend.count_color_in_rect(STATUS_SUCCESS, 122, 120, 198, 179);
        assert!(
            lower_body > 120,
            "the beetle must not collapse back into the old top header"
        );

        let left_channels = backend.count_color_in_rect(STATUS_SUCCESS, 25, 60, 95, 145);
        assert!(
            left_channels > 30,
            "confirmed HUD V2 uses the left radar-line channel column"
        );

        let right_metrics = backend.count_color_in_rect(STATUS_SUCCESS, 228, 100, 304, 164);
        assert!(
            right_metrics > 10,
            "confirmed HUD V2 uses the right IO/LLM/DONE/LOAD metric column"
        );

        let mic_bottom = backend.count_color_in_rect(AUDIO_INACTIVE, 27, 194, 95, 224);
        let speaker_bottom = backend.count_color_in_rect(AUDIO_INACTIVE, 188, 197, 273, 221);
        assert!(
            mic_bottom > 10 && speaker_bottom > 10,
            "mic and speaker status belong in the bottom audio strip"
        );

        let stale_top_audio = backend.count_color_in_rect(AUDIO_INACTIVE, 24, 60, 95, 90);
        assert_eq!(stale_top_audio, 0, "old upper audio icons must not return");
    }

    #[test]
    fn display_hud_v2_code_preview_has_subtle_grid_and_can_emit_bmp() {
        let backend =
            render_preview_frame(DisplaySystemState::Idle, DisplayPressureLevel::Normal, 42);

        assert_eq!(
            backend.count_color_in_rect(rgb565(10, 56, 65), 0, 0, 320, 240),
            0,
            "the old bright grid color must not survive in HUD V2"
        );
        assert!(
            backend.count_color_in_rect(DISPLAY_BG, 0, 0, 320, 240) > 52_000,
            "the confirmed HUD keeps the grid as a background texture, not the first visual layer"
        );
        assert!(
            backend.count_color_in_rect(AUDIO_INACTIVE, 27, 194, 95, 224) > 10,
            "inactive mic icon must remain visible before hardware flashing"
        );
        assert!(
            backend.count_color_in_rect(AUDIO_INACTIVE, 188, 197, 273, 221) > 10,
            "inactive speaker icon must remain visible before hardware flashing"
        );

        let Ok(dir) = std::env::var("BEETLE_DISPLAY_PREVIEW_DIR") else {
            return;
        };
        let dir = std::path::Path::new(&dir);
        std::fs::create_dir_all(dir).expect("create preview dir");

        let previews = [
            (
                "idle-v2-code.bmp",
                DisplaySystemState::Idle,
                DisplayPressureLevel::Normal,
                42,
            ),
            (
                "busy-v2-code.bmp",
                DisplaySystemState::Busy,
                DisplayPressureLevel::Cautious,
                78,
            ),
            (
                "voice-v2-code.bmp",
                DisplaySystemState::Recording,
                DisplayPressureLevel::Normal,
                57,
            ),
            (
                "critical-v2-code.bmp",
                DisplaySystemState::Fault,
                DisplayPressureLevel::Critical,
                96,
            ),
        ];

        for (name, state, pressure, heap_percent) in previews {
            let frame = render_preview_frame(state, pressure, heap_percent);
            let path = dir.join(name);
            frame.write_bmp(&path).expect("write preview bmp");
            println!("display HUD V2 code preview: {}", path.display());
        }
    }

    #[test]
    fn linux_backend_kind_stays_framebuffer_for_framebuffer_configs() {
        let mut config = default_disabled_display_config();
        config.enabled = true;
        config.driver = DisplayDriver::Framebuffer;
        config.bus = DisplayBus::Framebuffer;
        assert_eq!(
            linux_display_backend_kind(&config),
            LinuxDisplayBackendKind::Framebuffer
        );
    }

    #[test]
    fn linux_backend_kind_selects_spi_for_panel_configs() {
        let mut config = default_disabled_display_config();
        config.enabled = true;
        config.driver = DisplayDriver::St7789;
        config.bus = DisplayBus::Spi;
        assert_eq!(
            linux_display_backend_kind(&config),
            LinuxDisplayBackendKind::Spi
        );
    }

    #[test]
    fn esp_spi_transfer_chunk_stays_small_enough_for_fragmented_internal_heap() {
        assert_eq!(display_spi_max_transfer_size(320, 240), 320 * 4 * 2);
        assert!(
            display_spi_max_transfer_size(320, 240) < 4 * 1024,
            "ESP SPI display chunks must not rely on a large internal DMA bounce allocation"
        );
    }
}
