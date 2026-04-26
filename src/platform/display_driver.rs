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
    use crate::display::{DisplayColorOrder, DisplayDriver};
    use crate::platform::heap;
    use embedded_graphics_core::{
        draw_target::DrawTarget,
        geometry::{OriginDimensions, Size},
        pixelcolor::Rgb565,
        Pixel,
    };
    use esp_idf_svc::sys::*;

    /// SPI-connected display backend (ST7789 / ILI9341 / ST7735 family).
    /// Framebuffer lives in PSRAM; rendering via `embedded-graphics` `DrawTarget`.
    pub(super) struct SpiDisplayBackend {
        spi_host: u32,
        spi_handle: spi_device_handle_t,
        dc_pin: i32,
        width: u16,
        height: u16,
        framebuf: *mut u8,
        framebuf_len: usize,
        max_transfer_sz: usize,
    }

    // SAFETY: SpiDisplayBackend is only accessed from the display thread (behind Mutex).
    unsafe impl Send for SpiDisplayBackend {}

    impl Drop for SpiDisplayBackend {
        fn drop(&mut self) {
            unsafe {
                spi_bus_remove_device(self.spi_handle);
                spi_bus_free(self.spi_host);
                heap::free_spiram_buffer(self.framebuf);
            }
        }
    }

    impl SpiDisplayBackend {
        pub fn new(config: &DisplayConfig) -> Result<Self> {
            if matches!(config.driver, DisplayDriver::Framebuffer) {
                return Err(crate::error::Error::config(
                    "display_spi_init",
                    "driver=framebuffer is for Linux fbdev only; on ESP use St7789/ILI9341/ST7735",
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

            // --- Configure DC pin as GPIO output ---
            unsafe {
                let dc_conf = gpio_config_t {
                    pin_bit_mask: 1u64 << spi.dc,
                    mode: gpio_mode_t_GPIO_MODE_OUTPUT,
                    pull_up_en: gpio_pullup_t_GPIO_PULLUP_DISABLE,
                    pull_down_en: gpio_pulldown_t_GPIO_PULLDOWN_DISABLE,
                    intr_type: gpio_int_type_t_GPIO_INTR_DISABLE,
                    ..core::mem::zeroed()
                };
                let ret = gpio_config(&dc_conf);
                if ret != ESP_OK {
                    heap::free_spiram_buffer(framebuf);
                    return Err(crate::error::Error::Esp {
                        code: ret,
                        stage: "display_dc_gpio",
                    });
                }
            }

            // --- Optional RST pin: pulse low → high ---
            if let Some(rst) = spi.rst {
                unsafe {
                    let rst_conf = gpio_config_t {
                        pin_bit_mask: 1u64 << rst,
                        mode: gpio_mode_t_GPIO_MODE_OUTPUT,
                        pull_up_en: gpio_pullup_t_GPIO_PULLUP_DISABLE,
                        pull_down_en: gpio_pulldown_t_GPIO_PULLDOWN_DISABLE,
                        intr_type: gpio_int_type_t_GPIO_INTR_DISABLE,
                        ..core::mem::zeroed()
                    };
                    let ret = gpio_config(&rst_conf);
                    if ret != ESP_OK {
                        heap::free_spiram_buffer(framebuf);
                        return Err(crate::error::Error::Esp {
                            code: ret,
                            stage: "display_rst_gpio",
                        });
                    }
                    gpio_set_level(rst, 0);
                    std::thread::sleep(std::time::Duration::from_millis(20));
                    gpio_set_level(rst, 1);
                    std::thread::sleep(std::time::Duration::from_millis(120));
                }
            }

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
            let spi_host = spi.host as u32;
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

            // --- Add SPI device ---
            let dev_cfg = spi_device_interface_config_t {
                clock_speed_hz: spi.freq_hz as i32,
                mode: 0,
                spics_io_num: spi.cs,
                queue_size: 1,
                ..Default::default()
            };
            let mut spi_handle: spi_device_handle_t = core::ptr::null_mut();
            unsafe {
                let ret = spi_bus_add_device(spi_host, &dev_cfg, &mut spi_handle);
                if ret != ESP_OK {
                    spi_bus_free(spi_host);
                    heap::free_spiram_buffer(framebuf);
                    return Err(crate::error::Error::Esp {
                        code: ret,
                        stage: "display_spi_add_device",
                    });
                }
            }

            let mut backend = Self {
                spi_host,
                spi_handle,
                dc_pin: spi.dc,
                width,
                height,
                framebuf,
                framebuf_len,
                max_transfer_sz,
            };

            // --- Send display init commands ---
            backend.init_display_controller(config)?;

            log::info!(
                "[display] SPI backend ready: {}x{}, driver={:?}",
                width,
                height,
                config.driver
            );
            Ok(backend)
        }

        /// Send a command byte (DC=0).
        fn send_cmd(&self, cmd: u8) -> Result<()> {
            unsafe { gpio_set_level(self.dc_pin, 0) };
            self.spi_write(&[cmd])
        }

        /// Send data bytes (DC=1).
        fn send_data(&self, data: &[u8]) -> Result<()> {
            unsafe { gpio_set_level(self.dc_pin, 1) };
            self.spi_write(data)
        }

        fn spi_write(&self, data: &[u8]) -> Result<()> {
            if data.is_empty() {
                return Ok(());
            }
            let mut trans: spi_transaction_t = unsafe { core::mem::zeroed() };
            trans.length = data.len() * 8;
            trans.__bindgen_anon_1.tx_buffer = data.as_ptr() as *const _;
            let ret = unsafe { spi_device_transmit(self.spi_handle, &mut trans) };
            if ret != ESP_OK {
                return Err(crate::error::Error::Esp {
                    code: ret,
                    stage: "display_spi_write",
                });
            }
            Ok(())
        }

        fn init_display_controller(&mut self, config: &DisplayConfig) -> Result<()> {
            // SWRESET
            self.send_cmd(0x01)?;
            std::thread::sleep(std::time::Duration::from_millis(150));

            // SLPOUT
            self.send_cmd(0x11)?;
            std::thread::sleep(std::time::Duration::from_millis(120));

            match config.driver {
                DisplayDriver::Framebuffer => {
                    return Err(crate::error::Error::config(
                        "display_spi_init",
                        "driver=framebuffer is for Linux fbdev only; on ESP use St7789/ILI9341/ST7735",
                    ));
                }
                DisplayDriver::St7735 => {
                    // ST7735 / ST7735R / ST7735S: frame rate, power, gamma (not used on ST7789/ILI9341).
                    self.send_cmd(0xB1)?;
                    self.send_data(&[0x01, 0x2C, 0x2D])?; // FRMCTR1
                    self.send_cmd(0xB2)?;
                    self.send_data(&[0x01, 0x2C, 0x2D])?; // FRMCTR2
                    self.send_cmd(0xB3)?;
                    self.send_data(&[0x01, 0x2C, 0x2D, 0x01, 0x2C, 0x2D])?; // FRMCTR3
                    self.send_cmd(0xB4)?;
                    self.send_data(&[0x07])?; // INVCTR: no line inversion
                    self.send_cmd(0xC0)?;
                    self.send_data(&[0xA2, 0x02, 0x84])?; // PWCTR1
                    self.send_cmd(0xC1)?;
                    self.send_data(&[0xC5])?; // PWCTR2
                    self.send_cmd(0xC2)?;
                    self.send_data(&[0x0A, 0x00])?; // PWCTR3
                    self.send_cmd(0xC3)?;
                    self.send_data(&[0x8A, 0x2A])?; // PWCTR4
                    self.send_cmd(0xC4)?;
                    self.send_data(&[0x8A, 0xEE])?; // PWCTR5
                    self.send_cmd(0xC5)?;
                    self.send_data(&[0x0E])?; // VMCTR1
                                              // COLMOD: 16-bit RGB565 (ST7735 uses 0x05; ST7789 uses 0x55)
                    self.send_cmd(0x3A)?;
                    self.send_data(&[0x05])?;
                    self.send_cmd(0xE0)?;
                    self.send_data(&[
                        0x02, 0x1c, 0x07, 0x12, 0x37, 0x32, 0x29, 0x2d, 0x29, 0x25, 0x2B, 0x39,
                        0x00, 0x01, 0x03, 0x10,
                    ])?; // GMCTRP1
                    self.send_cmd(0xE1)?;
                    self.send_data(&[
                        0x03, 0x1d, 0x07, 0x06, 0x2E, 0x2C, 0x29, 0x2D, 0x2E, 0x2E, 0x37, 0x3F,
                        0x00, 0x00, 0x02, 0x10,
                    ])?; // GMCTRN1
                }
                DisplayDriver::St7789 | DisplayDriver::Ili9341 => {
                    // COLMOD: 16-bit RGB565 (ST7789/ILI9341)
                    self.send_cmd(0x3A)?;
                    self.send_data(&[0x55])?;
                }
            }

            // MADCTL: rotation + color order
            let madctl = Self::compute_madctl(config.rotation, &config.color_order);
            self.send_cmd(0x36)?;
            self.send_data(&[madctl])?;

            // INVON / INVOFF
            // ST7789: panel often inverted by default → INVON unless invert_colors.
            // ILI9341 / ST7735: typically non-inverted → INVOFF unless invert_colors.
            let needs_invon = match config.driver {
                DisplayDriver::Framebuffer => {
                    return Err(crate::error::Error::config(
                        "display_spi_init",
                        "driver=framebuffer is for Linux fbdev only",
                    ));
                }
                DisplayDriver::St7789 => !config.invert_colors,
                DisplayDriver::Ili9341 | DisplayDriver::St7735 => config.invert_colors,
            };
            if needs_invon {
                self.send_cmd(0x21)?; // INVON
            } else {
                self.send_cmd(0x20)?; // INVOFF
            }

            // NORON
            self.send_cmd(0x13)?;
            std::thread::sleep(std::time::Duration::from_millis(10));

            // DISPON
            self.send_cmd(0x29)?;
            std::thread::sleep(std::time::Duration::from_millis(20));

            Ok(())
        }

        fn compute_madctl(rotation: u16, color_order: &DisplayColorOrder) -> u8 {
            let color_bit = match color_order {
                DisplayColorOrder::Rgb => 0x00,
                DisplayColorOrder::Bgr => 0x08,
            };
            let rot_bits = match rotation {
                90 => 0x60,  // MX + MV
                180 => 0xC0, // MX + MY
                270 => 0xA0, // MY + MV
                _ => 0x00,   // 0 degrees
            };
            rot_bits | color_bit
        }

        /// Set column/row address window then push full framebuf via SPI.
        pub fn flush(&mut self, offset_x: i16, offset_y: i16) -> Result<()> {
            self.flush_rows(offset_x, offset_y, 0, self.height)
        }

        /// Push only the rows `[ry..ry+rh)` from the framebuffer, reducing SPI transfer.
        pub fn flush_rows(&mut self, offset_x: i16, offset_y: i16, ry: u16, rh: u16) -> Result<()> {
            if rh == 0 || self.width == 0 {
                return Ok(());
            }
            // Clamp to framebuffer bounds
            let ry = ry.min(self.height);
            let rh = rh.min(self.height.saturating_sub(ry));
            if rh == 0 {
                return Ok(());
            }

            // ST7789 / ILI9341 / ST7735: full-width rows for RAMWR; send only the dirty row band.
            let x0 = offset_x.max(0) as u16;
            let y0 = offset_y.max(0) as u16 + ry;
            let x1 = x0 + self.width - 1;
            let y1 = y0 + rh - 1;

            // CASET
            self.send_cmd(0x2A)?;
            self.send_data(&[(x0 >> 8) as u8, x0 as u8, (x1 >> 8) as u8, x1 as u8])?;

            // RASET
            self.send_cmd(0x2B)?;
            self.send_data(&[(y0 >> 8) as u8, y0 as u8, (y1 >> 8) as u8, y1 as u8])?;

            // RAMWR
            self.send_cmd(0x2C)?;

            // Send only the dirty rows from framebuf
            let row_bytes = self.width as usize * 2;
            let start = ry as usize * row_bytes;
            let end = start + rh as usize * row_bytes;
            let buf = unsafe { core::slice::from_raw_parts(self.framebuf, self.framebuf_len) };
            // 使用 max_transfer_sz 分块传输，避免超过 SPI DMA 缓冲区限制
            for chunk in buf[start..end].chunks(self.max_transfer_sz) {
                self.send_data(chunk)?;
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
            uptime_secs,
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
                    uptime_secs: *uptime_secs,
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
            uptime_secs,
        } => {
            render_ip_partial(
                backend,
                ip.as_str(),
                presence_subtitle.as_deref(),
                *uptime_secs,
                config.width,
                layout,
            );
            let flush_h = subtitle_ip_flush_rows(config.width, *uptime_secs);
            backend.flush_rows(
                config.offset_x,
                config.offset_y,
                layout.subtitle_top,
                flush_h,
            )?;
        }
        DisplayCommand::UpdateStateHeader {
            state,
            presence_subtitle,
            ip_address,
            uptime_secs,
            busy_phase,
        } => {
            render_state_header_partial(
                backend,
                *state,
                StateHeaderSnapshot {
                    presence_subtitle: presence_subtitle.as_deref(),
                    ip_address: ip_address.as_deref(),
                    uptime_secs: *uptime_secs,
                    busy_phase: *busy_phase,
                },
                config.width,
                layout,
            );
            backend.flush_rows(
                config.offset_x,
                config.offset_y,
                0,
                header_flush_rows(layout),
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
            let footer_h = config.height.saturating_sub(layout.footer_top);
            backend.flush_rows(
                config.offset_x,
                config.offset_y,
                layout.footer_top,
                footer_h,
            )?;
        }
        DisplayCommand::UpdateChannels { channels } => {
            render_channels_partial(backend, channels, config.width, layout);
            let ch_h = layout_middle_panel_height(layout) as u16;
            backend.flush_rows(config.offset_x, config.offset_y, layout.middle_top, ch_h)?;
        }
        DisplayCommand::UpdateBootProgress { stage } => {
            let bg = DISPLAY_BG;
            render_boot_progress(backend, *stage, config.width, config.height, bg, layout);
            let footer_h = config.height.saturating_sub(layout.footer_top);
            backend.flush_rows(
                config.offset_x,
                config.offset_y,
                layout.footer_top,
                footer_h,
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
    mono_font::{ascii::FONT_6X13, ascii::FONT_9X18_BOLD, MonoTextStyle},
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

    // --- Eyes (Sensor visors on sides of head) ---
    let eye_w = (head_rx * 5 / 10).max(3) as u32;
    let eye_h = (head_ry * 4 / 10).max(2) as u32;
    let eye_spread = head_rx * 7 / 10;
    let eye_y = head_cy - (eye_h as i32 / 2);

    if opts.x_eyes {
        // X eyes (fault state)
        let x_style = PrimitiveStyle::with_stroke(Rgb565::WHITE, 2);
        for &sx in &[-1i32, 1] {
            let ex = cx + sx * eye_spread;
            let _ = Line::new(Point::new(ex - 2, eye_y - 1), Point::new(ex + 2, eye_y + 3))
                .into_styled(x_style)
                .draw(target);
            let _ = Line::new(Point::new(ex + 2, eye_y - 1), Point::new(ex - 2, eye_y + 3))
                .into_styled(x_style)
                .draw(target);
        }
    } else {
        // Sensor visors (glowing rectangles)
        let eye_fill = PrimitiveStyle::with_fill(Rgb565::WHITE);
        for &sx in &[-1i32, 1] {
            let ex = cx + sx * eye_spread;
            let _ = Rectangle::new(
                Point::new(ex - (eye_w as i32 / 2), eye_y),
                Size::new(eye_w, eye_h),
            )
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
    /// F3: 系统运行时间（秒）。
    uptime_secs: u64,
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
    presence_subtitle: Option<&'a str>,
    ip_address: Option<&'a str>,
    width: u16,
    uptime_secs: u64,
    busy_phase: bool,
}

/// Title column strip: only covers the state title row (ends above `subtitle_top`).
/// 标题区窄背景；不覆盖副标题行，与 `UpdateIp` 局部刷新兼容。
fn draw_title_strip<D: DrawTarget<Color = Rgb565>>(
    target: &mut D,
    layout: &DisplayLayout,
    width: u16,
    fill: Rgb565,
) {
    let x = layout.title_left.saturating_sub(6) as i32;
    let y = layout.title_top as i32;
    let inner = layout.subtitle_top.saturating_sub(layout.title_top);
    let h_strip = inner.saturating_sub(3).max(10) as u32;
    let w_strip = (width as i32 - x).max(0) as u32;
    if w_strip == 0 {
        return;
    }
    let _ = Rectangle::new(Point::new(x, y), Size::new(w_strip, h_strip))
        .into_styled(PrimitiveStyle::with_fill(fill))
        .draw(target);
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

    // --- Top accent stripe (state color) ---
    let _ = Rectangle::new(Point::new(0, 0), Size::new(p.width as u32, 3))
        .into_styled(PrimitiveStyle::with_fill(beetle_color))
        .draw(target);

    // --- Section dividers ---
    let div_style = PrimitiveStyle::with_stroke(DIVIDER, 1);
    let mid_div_y = layout.middle_top.saturating_sub(6) as i32;
    let foot_div_y = layout.footer_top.saturating_sub(6) as i32;
    let div_margin = layout.margin_x as i32;
    let _ = Line::new(
        Point::new(div_margin, mid_div_y),
        Point::new(p.width as i32 - div_margin, mid_div_y),
    )
    .into_styled(div_style)
    .draw(target);
    let _ = Line::new(
        Point::new(div_margin, foot_div_y),
        Point::new(p.width as i32 - div_margin, foot_div_y),
    )
    .into_styled(div_style)
    .draw(target);

    // Header / middle / footer panels.
    let head_y = (layout.header_top as i32).saturating_sub(8).max(4);
    let middle_y = layout.middle_top as i32;
    let footer_y = layout.footer_top as i32;
    let head_h = (mid_div_y - head_y).max(24) as u32;
    let middle_h = layout_middle_panel_height(layout) as u32;
    let footer_h = (p.height as i32 - footer_y).max(1) as u32;
    draw_panel_fill(
        target,
        0,
        head_y,
        p.width as u32,
        head_h,
        PANEL_BG,
        PANEL_BORDER,
    );
    draw_panel_fill(
        target,
        0,
        middle_y,
        p.width as u32,
        middle_h,
        PANEL_BG,
        PANEL_BORDER,
    );
    draw_panel_fill(
        target,
        0,
        footer_y,
        p.width as u32,
        footer_h,
        PANEL_BG,
        PANEL_BORDER,
    );

    render_state_header_content(
        target,
        &StateHeaderParams {
            layout,
            state: p.state,
            presence_subtitle: p.presence_subtitle,
            ip_address: p.ip_address,
            width: p.width,
            uptime_secs: p.uptime_secs,
            busy_phase: p.busy_phase,
        },
    );

    // --- Channel status (middle section) with F5 failure count ---
    render_channels_inner(target, p.channels, p.width, layout);

    // --- Footer: pressure level + heap progress bar ---
    render_footer(
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
    draw_title_strip(target, layout, p.width, TITLE_STRIP_BG);

    let icon_size = layout.icon_size as i32;
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
    let (cx, body_cy, body_r, head_cy, head_r) = draw_beetle(
        target,
        layout.icon_left as i32,
        layout.header_top as i32,
        icon_size,
        beetle_color,
        &opts,
    );

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
        DisplaySystemState::Idle => {
            let check_style = PrimitiveStyle::with_stroke(Rgb565::WHITE, 3);
            let m = 8i32;
            let _ = Line::new(
                Point::new(cx - m, body_cy),
                Point::new(cx - 2, body_cy + m * 7 / 10),
            )
            .into_styled(check_style)
            .draw(target);
            let _ = Line::new(
                Point::new(cx - 2, body_cy + m * 7 / 10),
                Point::new(cx + m, body_cy - m / 2),
            )
            .into_styled(check_style)
            .draw(target);
        }
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

    let title_style = MonoTextStyle::new(&FONT_9X18_BOLD, beetle_color);
    let state_name = match p.state {
        DisplaySystemState::Booting => "BOOTING",
        DisplaySystemState::Pairing => "PAIRING",
        DisplaySystemState::Recovery => "RECOVERY",
        DisplaySystemState::NoWifi => "NO WIFI",
        DisplaySystemState::Idle => "IDLE",
        DisplaySystemState::Busy => "BUSY",
        DisplaySystemState::Fault => "FAULT",
        DisplaySystemState::Recording => "LISTEN",
        DisplaySystemState::Playing => "SPEAK",
    };
    let _ = Text::new(
        state_name,
        Point::new(layout.title_left as i32, layout.title_top as i32 + 14),
        title_style,
    )
    .draw(target);
    render_status_badge(
        target,
        layout.title_left as i32,
        layout.title_top as i32 - 16,
        state_name,
        beetle_color,
    );

    let subtitle_style = MonoTextStyle::new(&FONT_6X13, TEXT_SECONDARY);
    if let Some(subtitle) = p.presence_subtitle {
        let _ = Text::new(
            subtitle,
            Point::new(layout.title_left as i32, layout.subtitle_top as i32 + 11),
            subtitle_style,
        )
        .draw(target);
    } else if p.state == DisplaySystemState::Booting {
        let ip_text = p
            .ip_address
            .unwrap_or(concat!("beetle v", env!("CARGO_PKG_VERSION")));
        let _ = Text::new(
            ip_text,
            Point::new(layout.title_left as i32, layout.subtitle_top as i32 + 11),
            subtitle_style,
        )
        .draw(target);
    } else {
        let ip = p.ip_address.unwrap_or("---.---.---.---");
        let sx = layout.title_left as i32;
        let y0 = layout.subtitle_top as i32 + 11;
        if p.width >= DISPLAY_WIDE_LAYOUT_MIN_PX {
            let mut ip_line = [0u8; 40];
            let max_px = (p.width as usize).saturating_sub(layout.title_left as usize);
            let max_chars = (max_px / 6).clamp(1, 40);
            let ip_b = ip.as_bytes();
            let n = ip_b.len().min(max_chars);
            ip_line[..n].copy_from_slice(&ip_b[..n]);
            let ip_str = core::str::from_utf8(&ip_line[..n]).unwrap_or("---");
            let _ = Text::new(ip_str, Point::new(sx, y0), subtitle_style).draw(target);
            if p.uptime_secs > 0 {
                let mut up_buf = [0u8; 24];
                let up_str = format_uptime_line(p.uptime_secs, &mut up_buf);
                if !up_str.is_empty() {
                    let _ = Text::new(up_str, Point::new(sx, y0 + 13), subtitle_style).draw(target);
                }
            }
        } else {
            let mut sub_buf = [0u8; 30];
            let sub_str = format_subtitle_with_uptime(ip, p.uptime_secs, &mut sub_buf);
            let _ = Text::new(sub_str, Point::new(sx, y0), subtitle_style).draw(target);
        }
    }
}

/// Dashboard base background.
/// 仪表盘主背景色。
const DISPLAY_BG: Rgb565 = rgb565(11, 14, 20); // #0B0E14
/// Panel surface color (header/channels/footer cards).
/// 面板背景色（头部/通道/底部卡片）。
const PANEL_BG: Rgb565 = rgb565(21, 26, 34); // #151A22
/// Thin border color for panels.
/// 面板细边框色。
const PANEL_BORDER: Rgb565 = rgb565(42, 50, 65); // #2A3241
/// Section divider color.
/// 分区分割线颜色。
const DIVIDER: Rgb565 = rgb565(30, 37, 50); // #1E2532
/// Subtle title strip in header panel.
/// 头部标题条背景色。
const TITLE_STRIP_BG: Rgb565 = rgb565(26, 33, 45); // #1A212D
/// Primary text color.
/// 主文本色。
const TEXT_PRIMARY: Rgb565 = rgb565(226, 232, 240); // #E2E8F0
/// Secondary text color.
/// 次文本色。
const TEXT_SECONDARY: Rgb565 = rgb565(148, 163, 184); // #94A3B8
/// Weak text color.
/// 弱文本色。
const TEXT_WEAK: Rgb565 = rgb565(71, 85, 105); // #475569
/// Status colors.
/// 状态强调色。
const STATUS_SUCCESS: Rgb565 = rgb565(16, 185, 129); // #10B981
const STATUS_WARNING: Rgb565 = rgb565(245, 158, 11); // #F59E0B
const STATUS_DANGER: Rgb565 = rgb565(239, 68, 68); // #EF4444
const STATUS_INFO: Rgb565 = rgb565(59, 130, 246); // #3B82F6
const STATUS_OFF: Rgb565 = rgb565(51, 65, 85); // #334155
/// 宽屏阈值（px）：通道三列、副标题 IP 与 `Up:` 分两行，避免窄屏单行截断。
const DISPLAY_WIDE_LAYOUT_MIN_PX: u16 = 200;

/// 中间通道区像素高度：与 `render_dashboard` 里 `draw_panel_fill(..., middle_y, middle_h, ...)` 一致
///（`footer_top` 上方 6px 留给分隔线，不得用满 `footer_top - middle_top`，否则行内容会画出面板边框）。
fn layout_middle_panel_height(layout: &DisplayLayout) -> i32 {
    let foot_div_y = layout.footer_top.saturating_sub(6) as i32;
    let middle_y = layout.middle_top as i32;
    (foot_div_y - middle_y).max(20)
}

/// `UpdateIp` / 副标题区 `flush_rows` 高度：宽屏且显示 `Up:` 时为双行。
fn subtitle_ip_flush_rows(width: u16, uptime_secs: u64) -> u16 {
    if width >= DISPLAY_WIDE_LAYOUT_MIN_PX && uptime_secs > 0 {
        30
    } else {
        16
    }
}

/// `UpdateStateHeader` 局刷高度：覆盖顶栏、头部 panel 与中间分割线，避免 steady-state 状态切换重刷中下半屏。
fn header_flush_rows(layout: &DisplayLayout) -> u16 {
    layout.middle_top
}

#[inline]
fn state_accent_color(state: DisplaySystemState) -> Rgb565 {
    match state {
        DisplaySystemState::Booting => STATUS_WARNING,
        DisplaySystemState::Pairing => rgb565(245, 158, 11), // #F59E0B
        DisplaySystemState::Recovery => rgb565(249, 115, 22), // #F97316
        DisplaySystemState::NoWifi => rgb565(100, 116, 139), // #64748B
        DisplaySystemState::Idle => STATUS_SUCCESS,
        DisplaySystemState::Busy => STATUS_INFO,
        DisplaySystemState::Fault => STATUS_DANGER,
        DisplaySystemState::Recording => rgb565(34, 197, 94), // #22C55E
        DisplaySystemState::Playing => rgb565(14, 165, 233),  // #0EA5E9
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

fn draw_panel_fill<D: DrawTarget<Color = Rgb565>>(
    target: &mut D,
    x: i32,
    y: i32,
    w: u32,
    h: u32,
    fill: Rgb565,
    border: Rgb565,
) {
    if w == 0 || h == 0 {
        return;
    }
    let _ = Rectangle::new(Point::new(x, y), Size::new(w, h))
        .into_styled(PrimitiveStyle::with_fill(fill))
        .draw(target);
    let _ = Rectangle::new(Point::new(x, y), Size::new(w, h))
        .into_styled(PrimitiveStyle::with_stroke(border, 1))
        .draw(target);

    // Add a subtle top highlight for 3D HUD effect
    if h > 2 && w > 2 {
        let highlight = rgb565(35, 43, 56); // slightly brighter than PANEL_BG
        let _ = Line::new(
            Point::new(x + 1, y + 1),
            Point::new(x + w as i32 - 2, y + 1),
        )
        .into_styled(PrimitiveStyle::with_stroke(highlight, 1))
        .draw(target);
    }
}

fn render_status_badge<D: DrawTarget<Color = Rgb565>>(
    target: &mut D,
    x: i32,
    y: i32,
    label: &str,
    accent: Rgb565,
) {
    let text_w = label.len() as i32 * 6;
    let badge_w = (text_w + 10).max(30) as u32;
    let badge_h = 14u32;
    draw_panel_fill(target, x, y, badge_w, badge_h, PANEL_BG, accent);
    let style = MonoTextStyle::new(&FONT_6X13, accent);
    let _ = Text::new(label, Point::new(x + 5, y + 11), style).draw(target);
}

/// F3: 窄屏单行副标题 "IP Up:XdYh"（IP 截断为给 uptime 留位）。
fn format_subtitle_with_uptime<'a>(ip: &'a str, secs: u64, buf: &'a mut [u8; 30]) -> &'a str {
    let mut pos = 0usize;
    // 写入 IP（截断到留空间给 uptime）
    let ip_bytes = ip.as_bytes();
    let ip_max = ip_bytes.len().min(16);
    buf[pos..pos + ip_max].copy_from_slice(&ip_bytes[..ip_max]);
    pos += ip_max;

    if secs > 0 {
        let tag = b" Up:";
        let tag_len = tag.len().min(buf.len() - pos);
        buf[pos..pos + tag_len].copy_from_slice(&tag[..tag_len]);
        pos += tag_len;
        pos = append_uptime_duration(secs, buf, pos);
    }

    core::str::from_utf8(&buf[..pos]).unwrap_or(ip)
}

/// 第二行 `Up:1d2h` / `Up:3h45m`（`secs == 0` 时返回空串）。
fn format_uptime_line(secs: u64, buf: &mut [u8; 24]) -> &str {
    if secs == 0 {
        return "";
    }
    let mut pos = 0usize;
    for &b in b"Up:".iter() {
        if pos < buf.len() {
            buf[pos] = b;
            pos += 1;
        }
    }
    pos = append_uptime_duration(secs, buf, pos);
    core::str::from_utf8(&buf[..pos]).unwrap_or("")
}

/// 将运行时长写入 `buf[pos..]`（`XdYh` / `XhYm`），返回新 pos。
fn append_uptime_duration(secs: u64, buf: &mut [u8], mut pos: usize) -> usize {
    let days = secs / 86400;
    let hours = (secs % 86400) / 3600;
    let mins = (secs % 3600) / 60;

    if days > 0 {
        pos = write_u64_to_buf(days, buf, pos);
        if pos < buf.len() {
            buf[pos] = b'd';
            pos += 1;
        }
        pos = write_u64_to_buf(hours, buf, pos);
        if pos < buf.len() {
            buf[pos] = b'h';
            pos += 1;
        }
    } else {
        pos = write_u64_to_buf(hours, buf, pos);
        if pos < buf.len() {
            buf[pos] = b'h';
            pos += 1;
        }
        pos = write_u64_to_buf(mins, buf, pos);
        if pos < buf.len() {
            buf[pos] = b'm';
            pos += 1;
        }
    }
    pos
}

/// Write a u64 value into a byte buffer at `pos`, return new pos (generic version).
fn write_u64_to_buf(val: u64, buf: &mut [u8], mut pos: usize) -> usize {
    if val == 0 {
        if pos < buf.len() {
            buf[pos] = b'0';
            pos += 1;
        }
        return pos;
    }
    let mut tmp = [0u8; 20];
    let mut n = val;
    let mut i = 0;
    while n > 0 && i < 20 {
        tmp[i] = b'0' + (n % 10) as u8;
        n /= 10;
        i += 1;
    }
    for j in (0..i).rev() {
        if pos < buf.len() {
            buf[pos] = tmp[j];
            pos += 1;
        }
    }
    pos
}

/// Shared channel rendering logic (used by full dashboard and partial update).
fn render_channels_inner<D: DrawTarget<Color = Rgb565>>(
    target: &mut D,
    channels: &[DisplayChannelStatus; crate::DISPLAY_CHANNEL_CAPACITY],
    width: u16,
    layout: &DisplayLayout,
) {
    let middle_y = layout.middle_top as i32;
    let margin_x = layout.margin_x as i32;
    let text_style = MonoTextStyle::new(&FONT_6X13, TEXT_PRIMARY);
    let weak_style = MonoTextStyle::new(&FONT_6X13, TEXT_WEAK);
    let middle_h = layout_middle_panel_height(layout);
    let cols: usize = if width >= DISPLAY_WIDE_LAYOUT_MIN_PX {
        3
    } else {
        2
    };
    let visible_channels = channels
        .iter()
        .filter(|channel| channel.visible)
        .count()
        .max(1);
    let rows_needed = visible_channels.div_ceil(cols);
    // 行高不得超过 middle_h / rows，避免 `clamp(14,24)` 在矮中间区把行画出面板底边。
    let row_step = (middle_h / rows_needed as i32).clamp(8, 24);
    let row_h = (row_step - 4).clamp(6, 18).min(row_step.saturating_sub(2));
    let row_pad = ((row_step - row_h) / 2).max(1);
    let dot_d = (row_h.saturating_sub(2) / 2).clamp(4, 10) as u32;
    let text_base_dy = (row_h - 2).clamp(7, 13);

    let col_width = width as i32 / cols as i32;
    let mut col = 0i32;
    let mut row = 0i32;
    for ch in channels.iter() {
        if !ch.visible {
            continue;
        }
        let px = margin_x + col * col_width;
        let row_y = middle_y + row * row_step + row_pad;
        let py = row_y + row_h / 2;
        let row_x = px - 2;
        let row_w = (col_width - margin_x + 2).max(26) as u32;
        draw_panel_fill(
            target,
            row_x,
            row_y,
            row_w,
            row_h.max(1) as u32,
            DISPLAY_BG,
            DIVIDER,
        );

        let dot_color = match ch.runtime_status {
            crate::DisplayChannelRuntimeStatus::Disabled => STATUS_OFF,
            crate::DisplayChannelRuntimeStatus::Configured
            | crate::DisplayChannelRuntimeStatus::Online => STATUS_SUCCESS,
            crate::DisplayChannelRuntimeStatus::Waiting => STATUS_WARNING,
            crate::DisplayChannelRuntimeStatus::Failed => STATUS_DANGER,
        };
        let dot_y = py - (dot_d as i32 / 2);

        // Bullseye style: hollow outer circle, solid inner dot
        let _ = Circle::new(Point::new(px, dot_y), dot_d)
            .into_styled(PrimitiveStyle::with_stroke(dot_color, 1))
            .draw(target);

        if dot_d > 4 {
            let inner_d = dot_d - 4;
            let _ = Circle::new(Point::new(px + 2, dot_y + 2), inner_d)
                .into_styled(PrimitiveStyle::with_fill(dot_color))
                .draw(target);
        } else if dot_d > 2 {
            let inner_d = dot_d - 2;
            let _ = Circle::new(Point::new(px + 1, dot_y + 1), inner_d)
                .into_styled(PrimitiveStyle::with_fill(dot_color))
                .draw(target);
        }

        let name_style = if ch.enabled { text_style } else { weak_style };
        let label = if ch.display_label.is_empty() {
            ch.name
        } else {
            ch.display_label
        };
        let _ =
            Text::new(label, Point::new(px + 14, row_y + text_base_dy), name_style).draw(target);

        let mut token_buf = [0u8; 8];
        let (token, token_color) = match ch.runtime_status {
            crate::DisplayChannelRuntimeStatus::Disabled => ("OFF", STATUS_OFF),
            crate::DisplayChannelRuntimeStatus::Configured => ("OK", STATUS_SUCCESS),
            crate::DisplayChannelRuntimeStatus::Waiting => ("WAIT", STATUS_WARNING),
            crate::DisplayChannelRuntimeStatus::Online => ("ON", STATUS_SUCCESS),
            crate::DisplayChannelRuntimeStatus::Failed if ch.consecutive_failures > 0 => {
                token_buf[0] = b'x';
                let mut pos = 1;
                let mut tmp = [0u8; 10];
                let mut n = ch.consecutive_failures;
                let mut i = 0usize;
                while n > 0 && i < tmp.len() {
                    tmp[i] = b'0' + (n % 10) as u8;
                    n /= 10;
                    i += 1;
                }
                for j in (0..i).rev() {
                    if pos < token_buf.len() {
                        token_buf[pos] = tmp[j];
                        pos += 1;
                    }
                }
                (
                    core::str::from_utf8(&token_buf[..pos]).unwrap_or("DOWN"),
                    STATUS_DANGER,
                )
            }
            crate::DisplayChannelRuntimeStatus::Failed => ("DOWN", STATUS_DANGER),
        };
        let token_style = MonoTextStyle::new(&FONT_6X13, token_color);
        let token_x = px + col_width - margin_x - (token.len() as i32 * 6);
        let _ = Text::new(
            token,
            Point::new(token_x, row_y + text_base_dy),
            token_style,
        )
        .draw(target);

        col += 1;
        if col >= cols as i32 {
            col = 0;
            row += 1;
        }
    }
}

/// Partial update: repaint only the IP subtitle region。
fn render_ip_partial<D: DrawTarget<Color = Rgb565>>(
    target: &mut D,
    ip: &str,
    presence_subtitle: Option<&str>,
    uptime_secs: u64,
    width: u16,
    layout: &DisplayLayout,
) {
    let y = layout.subtitle_top as i32;
    let x = layout.title_left as i32;
    let clear_w = (width as i32 - x).max(0) as u32;
    let clear_h = subtitle_ip_flush_rows(width, uptime_secs) as u32;
    let _ = Rectangle::new(Point::new(x, y), Size::new(clear_w, clear_h))
        .into_styled(PrimitiveStyle::with_fill(TITLE_STRIP_BG))
        .draw(target);

    let subtitle_style = MonoTextStyle::new(&FONT_6X13, TEXT_SECONDARY);
    let y0 = y + 11;
    if let Some(subtitle) = presence_subtitle {
        let _ = Text::new(subtitle, Point::new(x, y0), subtitle_style).draw(target);
    } else if width >= DISPLAY_WIDE_LAYOUT_MIN_PX {
        let mut ip_line = [0u8; 40];
        let max_px = (width as usize).saturating_sub(layout.title_left as usize);
        let max_chars = (max_px / 6).clamp(1, 40);
        let ip_b = ip.as_bytes();
        let n = ip_b.len().min(max_chars);
        ip_line[..n].copy_from_slice(&ip_b[..n]);
        let ip_str = core::str::from_utf8(&ip_line[..n]).unwrap_or("---");
        let _ = Text::new(ip_str, Point::new(x, y0), subtitle_style).draw(target);
        if uptime_secs > 0 {
            let mut up_buf = [0u8; 24];
            let up_str = format_uptime_line(uptime_secs, &mut up_buf);
            if !up_str.is_empty() {
                let _ = Text::new(up_str, Point::new(x, y0 + 13), subtitle_style).draw(target);
            }
        }
    } else {
        let _ = Text::new(ip, Point::new(x, y0), subtitle_style).draw(target);
    }
}

/// Partial update: repaint only the state header region used by steady-state status flips.
struct StateHeaderSnapshot<'a> {
    presence_subtitle: Option<&'a str>,
    ip_address: Option<&'a str>,
    uptime_secs: u64,
    busy_phase: bool,
}

fn render_state_header_partial<D: DrawTarget<Color = Rgb565>>(
    target: &mut D,
    state: DisplaySystemState,
    snapshot: StateHeaderSnapshot<'_>,
    width: u16,
    layout: &DisplayLayout,
) {
    let flush_h = header_flush_rows(layout);
    let _ = Rectangle::new(Point::new(0, 0), Size::new(width as u32, flush_h as u32))
        .into_styled(PrimitiveStyle::with_fill(DISPLAY_BG))
        .draw(target);

    let accent = state_accent_color(state);
    let _ = Rectangle::new(Point::new(0, 0), Size::new(width as u32, 3))
        .into_styled(PrimitiveStyle::with_fill(accent))
        .draw(target);

    let head_y = (layout.header_top as i32).saturating_sub(8).max(4);
    let mid_div_y = layout.middle_top.saturating_sub(6) as i32;
    let div_margin = layout.margin_x as i32;
    let head_h = (mid_div_y - head_y).max(24) as u32;
    let div_style = PrimitiveStyle::with_stroke(DIVIDER, 1);
    draw_panel_fill(
        target,
        0,
        head_y,
        width as u32,
        head_h,
        PANEL_BG,
        PANEL_BORDER,
    );
    let _ = Line::new(
        Point::new(div_margin, mid_div_y),
        Point::new(width as i32 - div_margin, mid_div_y),
    )
    .into_styled(div_style)
    .draw(target);

    render_state_header_content(
        target,
        &StateHeaderParams {
            layout,
            state,
            presence_subtitle: snapshot.presence_subtitle,
            ip_address: snapshot.ip_address,
            width,
            uptime_secs: snapshot.uptime_secs,
            busy_phase: snapshot.busy_phase,
        },
    );
}

/// Partial update: repaint only the channel status (middle) region.
fn render_channels_partial<D: DrawTarget<Color = Rgb565>>(
    target: &mut D,
    channels: &[DisplayChannelStatus; crate::DISPLAY_CHANNEL_CAPACITY],
    width: u16,
    layout: &DisplayLayout,
) {
    let middle_y = layout.middle_top as i32;
    let ch_h = layout_middle_panel_height(layout) as u32;

    draw_panel_fill(
        target,
        0,
        middle_y,
        width as u32,
        ch_h,
        PANEL_BG,
        PANEL_BORDER,
    );

    // F5: 使用共享渲染逻辑（含失败计数）
    render_channels_inner(target, channels, width, layout);
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

/// Partial update: repaint only the footer pressure + progress bar region.
fn render_pressure_partial<D: DrawTarget<Color = Rgb565>>(
    target: &mut D,
    level: &DisplayPressureLevel,
    layout: &DisplayLayout,
    fp: &FooterPartialParams,
) {
    render_footer(
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
}

/// Shared footer rendering: pressure label + progress bar + percentage text + message stats.
#[allow(clippy::too_many_arguments)]
fn render_footer<D: DrawTarget<Color = Rgb565>>(
    target: &mut D,
    layout: &DisplayLayout,
    level: &DisplayPressureLevel,
    heap_percent: u8,
    width: u16,
    height: u16,
    messages_in: u32,
    messages_out: u32,
    last_active_epoch_secs: u32,
    llm_last_ms: u32,
    error_flash: bool,
) {
    let footer_y = layout.footer_top as i32;
    let margin_x = layout.margin_x as i32;
    let footer_h = (height as i32 - footer_y).max(1) as u32;
    draw_panel_fill(
        target,
        0,
        footer_y,
        width as u32,
        footer_h,
        PANEL_BG,
        PANEL_BORDER,
    );
    let pressure_text = match level {
        DisplayPressureLevel::Normal => "NORMAL",
        DisplayPressureLevel::Cautious => "CAUTIOUS",
        DisplayPressureLevel::Critical => "CRITICAL",
    };
    let pressure_color = pressure_accent_color(level);

    // Summary badge.
    if error_flash {
        let text_w = pressure_text.len() as u32 * 6 + 4; // FONT_6X13 + padding
        let _ = Rectangle::new(Point::new(margin_x - 2, footer_y), Size::new(text_w, 14))
            .into_styled(PrimitiveStyle::with_fill(pressure_color))
            .draw(target);
        let flash_style = MonoTextStyle::new(&FONT_6X13, Rgb565::WHITE);
        let _ = Text::new(
            pressure_text,
            Point::new(margin_x, footer_y + 11),
            flash_style,
        )
        .draw(target);
    } else {
        render_status_badge(target, margin_x, footer_y, pressure_text, pressure_color);
    }

    let dim_min = width.min(height) as i32;
    let bar_top_pad = (dim_min * 18 / DISPLAY_LAYOUT_REF_PX as i32).clamp(14, 22);
    let stats_gap = (dim_min * 12 / DISPLAY_LAYOUT_REF_PX as i32).clamp(9, 14);
    let bar_h = (dim_min * 8 / DISPLAY_LAYOUT_REF_PX as i32).clamp(6, 10) as u32;

    let bar_x = margin_x;
    let bar_y = footer_y + bar_top_pad;
    let bar_w = (width as i32 - 56).max(40) as u32;

    let bar_border = PrimitiveStyle::with_stroke(PANEL_BORDER, 1);
    let _ = Rectangle::new(Point::new(bar_x, bar_y), Size::new(bar_w, bar_h))
        .into_styled(bar_border)
        .draw(target);

    let fill_w = ((heap_percent as u32).min(100) * (bar_w - 2)) / 100;
    if fill_w > 0 {
        // Heap bar color based on percentage, not orchestrator pressure level.
        let heap_bar_color = if heap_percent < 70 {
            STATUS_SUCCESS // green: 0-69%
        } else if heap_percent < 85 {
            STATUS_WARNING // yellow: 70-84%
        } else {
            STATUS_DANGER // red: 85-100%
        };
        let _ = Rectangle::new(
            Point::new(bar_x + 1, bar_y + 1),
            Size::new(fill_w, bar_h - 2),
        )
        .into_styled(PrimitiveStyle::with_fill(heap_bar_color))
        .draw(target);

        // HUD Segmented effect: draw vertical background lines to cut the bar
        let seg_style = PrimitiveStyle::with_stroke(PANEL_BG, 1);
        let mut sx = bar_x + 4;
        while sx < bar_x + 1 + fill_w as i32 {
            let _ = Line::new(
                Point::new(sx, bar_y + 1),
                Point::new(sx, bar_y + bar_h as i32 - 2),
            )
            .into_styled(seg_style)
            .draw(target);
            sx += 4;
        }
    }

    let text_style = MonoTextStyle::new(&FONT_6X13, TEXT_PRIMARY);
    let mut pct_buf = [0u8; 5];
    let pct_str = format_pct(heap_percent, &mut pct_buf);
    let _ = Text::new(
        pct_str,
        Point::new(
            bar_x + bar_w as i32 + 4,
            bar_y + (bar_h as i32 - 1).clamp(6, 10),
        ),
        text_style,
    )
    .draw(target);

    // Stats line.
    let stats_y = bar_y + bar_h as i32 + stats_gap;
    draw_stats_line(
        target,
        margin_x,
        stats_y,
        messages_in,
        messages_out,
        last_active_epoch_secs,
        llm_last_ms,
    );
}

/// F8: 启动进度条渲染。4 段：WiFi → SNTP → Channels → Agent。
fn render_boot_progress<D: DrawTarget<Color = Rgb565>>(
    target: &mut D,
    stage: u8,
    width: u16,
    height: u16,
    bg: Rgb565,
    layout: &DisplayLayout,
) {
    let footer_y = layout.footer_top as i32;
    let margin_x = layout.margin_x as i32;
    let footer_h = (height as i32 - footer_y).max(1) as u32;

    // Clear footer
    let _ = Rectangle::new(Point::new(0, footer_y), Size::new(width as u32, footer_h))
        .into_styled(PrimitiveStyle::with_fill(bg))
        .draw(target);

    let stage = stage.min(4);
    let bar_x = margin_x;
    let bar_y = footer_y + 18;
    let total_w = (width as i32 - 2 * margin_x).max(40);
    let seg_w = total_w / 4;
    let bar_h = 10u32;

    let filled_color = STATUS_SUCCESS;
    let empty_color = DISPLAY_BG;
    let border_color = PANEL_BORDER;

    render_status_badge(target, margin_x, footer_y, "BOOTING", STATUS_INFO);

    // 4 segments
    for i in 0..4u8 {
        let sx = bar_x + i as i32 * seg_w;
        let color = if i < stage { filled_color } else { empty_color };
        let _ = Rectangle::new(Point::new(sx, bar_y), Size::new(seg_w as u32, bar_h))
            .into_styled(PrimitiveStyle::with_fill(color))
            .draw(target);
        let _ = Rectangle::new(Point::new(sx, bar_y), Size::new(seg_w as u32, bar_h))
            .into_styled(PrimitiveStyle::with_stroke(border_color, 1))
            .draw(target);
    }

    // Labels below bar
    let label_y = bar_y + bar_h as i32 + 11;
    let label_style = MonoTextStyle::new(&FONT_6X13, TEXT_SECONDARY);
    let labels = ["WiFi", "SNTP", "Chan", "Agent"];
    for (i, lbl) in labels.iter().enumerate() {
        let lx = bar_x + i as i32 * seg_w + (seg_w - lbl.len() as i32 * 6) / 2;
        let _ = Text::new(lbl, Point::new(lx, label_y), label_style).draw(target);
    }
}

/// 将仅含 ASCII 数字/标点的缓冲区转为 `&str`；异常时回退为 `"?"`（release 不 panic）。
fn utf8_ascii_digits_or_fallback(buf: &[u8]) -> &str {
    match std::str::from_utf8(buf) {
        Ok(s) => s,
        Err(_) => "?",
    }
}

/// Draw message stats line with separated label and value colors for better typography.
fn draw_stats_line<D: DrawTarget<Color = Rgb565>>(
    target: &mut D,
    x: i32,
    y: i32,
    msg_in: u32,
    msg_out: u32,
    epoch_secs: u32,
    llm_ms: u32,
) {
    let label_style = MonoTextStyle::new(&FONT_6X13, TEXT_WEAK);
    let value_style = MonoTextStyle::new(&FONT_6X13, TEXT_PRIMARY);
    let mut cx = x;

    let mut draw_part = |label: &str, val: &str| {
        if !label.is_empty() {
            let _ = Text::new(label, Point::new(cx, y), label_style).draw(target);
            cx += label.len() as i32 * 6;
        }
        let _ = Text::new(val, Point::new(cx, y), value_style).draw(target);
        cx += val.len() as i32 * 6;
        cx += 6; // space
    };

    // In
    let mut buf_in = [0u8; 10];
    let len_in = write_u32_to_buf(msg_in, &mut buf_in, 0);
    draw_part("In:", utf8_ascii_digits_or_fallback(&buf_in[..len_in]));

    // Out
    let mut buf_out = [0u8; 10];
    let len_out = write_u32_to_buf(msg_out, &mut buf_out, 0);
    draw_part("Out:", utf8_ascii_digits_or_fallback(&buf_out[..len_out]));

    // L
    if llm_ms > 0 {
        let mut buf_l = [0u8; 10];
        let mut pos = 0;
        if llm_ms >= 1000 {
            let secs = llm_ms / 1000;
            let tenths = (llm_ms % 1000) / 100;
            pos = write_u32_to_buf(secs, &mut buf_l, pos);
            buf_l[pos] = b'.';
            buf_l[pos + 1] = b'0' + tenths as u8;
            buf_l[pos + 2] = b's';
            pos += 3;
        } else {
            pos = write_u32_to_buf(llm_ms, &mut buf_l, pos);
            buf_l[pos] = b'm';
            buf_l[pos + 1] = b's';
            pos += 2;
        }
        draw_part("L:", utf8_ascii_digits_or_fallback(&buf_l[..pos]));
    }

    // Time
    let mut buf_t = [0u8; 5];
    if epoch_secs == 0 {
        buf_t.copy_from_slice(b"--:--");
        draw_part("", "--:--");
    } else {
        #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
        {
            use esp_idf_svc::sys::{localtime_r, time_t};
            let time_val: time_t = epoch_secs as time_t;
            let mut tm = unsafe { core::mem::zeroed::<esp_idf_svc::sys::tm>() };
            unsafe { localtime_r(&time_val, &mut tm) };
            let h = (tm.tm_hour as u8).min(23);
            let m = (tm.tm_min as u8).min(59);
            buf_t[0] = b'0' + h / 10;
            buf_t[1] = b'0' + h % 10;
            buf_t[2] = b':';
            buf_t[3] = b'0' + m / 10;
            buf_t[4] = b'0' + m % 10;
        }
        #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
        {
            let secs_of_day = epoch_secs % 86400;
            let h = ((secs_of_day / 3600) % 24) as u8;
            let m = ((secs_of_day % 3600) / 60) as u8;
            buf_t[0] = b'0' + h / 10;
            buf_t[1] = b'0' + h % 10;
            buf_t[2] = b':';
            buf_t[3] = b'0' + m / 10;
            buf_t[4] = b'0' + m % 10;
        }
        draw_part("", utf8_ascii_digits_or_fallback(&buf_t));
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
        pixelcolor::Rgb565,
        Pixel,
    };

    #[derive(Default)]
    struct FakeBackend {
        flush_calls: usize,
        flush_rows_calls: Vec<(u16, u16)>,
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
        assert_eq!(backend.flush_rows_calls[0].0, 0);
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
