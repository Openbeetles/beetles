//! Linux SPI display backend.
//! 使用 `/dev/spidevX.Y` + GPIO 字符设备（sysfs 兜底）实现用户态面板驱动，复用既有 dashboard 渲染链路。

use crate::display::{DisplayColorOrder, DisplayConfig, DisplayDriver};
use crate::error::{Error, Result};
use embedded_graphics_core::{
    draw_target::DrawTarget,
    geometry::{OriginDimensions, Size},
    pixelcolor::{raw::RawU16, Rgb565},
    prelude::RawData,
    Pixel,
};
use gpio_cdev::{Chip, LineHandle, LineRequestFlags};
use spidev::{SpiModeFlags, Spidev, SpidevOptions};
use std::convert::Infallible;
use std::io::Write as _;
use std::path::{Path, PathBuf};

const SYSFS_GPIO_ROOT: &str = "/sys/class/gpio";

struct LinuxCdevGpio {
    handle: LineHandle,
}

impl LinuxCdevGpio {
    fn new_output(pin: i32, initial_high: bool) -> Result<Self> {
        let mut chips = gpiochip_paths()?;
        chips.sort();
        let value = if initial_high { 1 } else { 0 };
        let mut last_err: Option<Error> = None;
        for chip_path in chips {
            let mut chip = match Chip::new(&chip_path) {
                Ok(chip) => chip,
                Err(e) => {
                    last_err = Some(Error::config("display_gpiochip_open", e.to_string()));
                    continue;
                }
            };
            let line = match chip.get_line(pin as u32) {
                Ok(line) => line,
                Err(e) => {
                    last_err = Some(Error::config("display_gpiochip_line", e.to_string()));
                    continue;
                }
            };
            match line.request(LineRequestFlags::OUTPUT, value, "beetle-display") {
                Ok(handle) => {
                    log::info!(
                        "[display_spi] gpio pin {} via {} (cdev)",
                        pin,
                        chip_path.display()
                    );
                    return Ok(Self { handle });
                }
                Err(e) => {
                    last_err = Some(Error::config("display_gpio_request", e.to_string()));
                }
            }
        }
        Err(last_err.unwrap_or_else(|| {
            Error::config(
                "display_gpio_request",
                format!("no usable gpiochip found for pin {}", pin),
            )
        }))
    }

    fn write(&self, high: bool) -> Result<()> {
        self.handle
            .set_value(if high { 1 } else { 0 })
            .map_err(|e| Error::config("display_gpio_write", e.to_string()))
    }
}

struct LinuxSysfsGpio {
    value_path: PathBuf,
}

impl LinuxSysfsGpio {
    fn new_output(pin: i32, initial_high: bool) -> Result<Self> {
        let gpio_dir = PathBuf::from(format!("{SYSFS_GPIO_ROOT}/gpio{pin}"));
        if !gpio_dir.exists() {
            std::fs::write(format!("{SYSFS_GPIO_ROOT}/export"), format!("{pin}"))
                .or_else(ignore_busy_gpio_export)
                .map_err(|e| Error::io("display_gpio_export", e))?;
        }
        std::fs::write(gpio_dir.join("direction"), "out")
            .map_err(|e| Error::io("display_gpio_direction", e))?;
        let gpio = Self {
            value_path: gpio_dir.join("value"),
        };
        gpio.write(initial_high)?;
        Ok(gpio)
    }

    fn write(&self, high: bool) -> Result<()> {
        std::fs::write(&self.value_path, if high { "1" } else { "0" })
            .map_err(|e| Error::io("display_gpio_write", e))
    }
}

fn ignore_busy_gpio_export(error: std::io::Error) -> std::io::Result<()> {
    if error.kind() == std::io::ErrorKind::AlreadyExists {
        return Ok(());
    }
    if error.raw_os_error() == Some(libc::EBUSY) {
        return Ok(());
    }
    Err(error)
}

enum LinuxOutputGpio {
    Cdev(LinuxCdevGpio),
    Sysfs(LinuxSysfsGpio),
}

impl LinuxOutputGpio {
    fn new_output(pin: i32, initial_high: bool) -> Result<Self> {
        match LinuxCdevGpio::new_output(pin, initial_high) {
            Ok(gpio) => Ok(Self::Cdev(gpio)),
            Err(cdev_err) => {
                log::warn!(
                    "[display_spi] gpio-cdev unavailable for pin {}: {}; falling back to sysfs",
                    pin,
                    cdev_err
                );
                LinuxSysfsGpio::new_output(pin, initial_high).map(Self::Sysfs)
            }
        }
    }

    fn write(&self, high: bool) -> Result<()> {
        match self {
            Self::Cdev(gpio) => gpio.write(high),
            Self::Sysfs(gpio) => gpio.write(high),
        }
    }
}

fn gpiochip_paths() -> Result<Vec<PathBuf>> {
    let mut chips = Vec::new();
    let entries = std::fs::read_dir("/dev").map_err(|e| Error::io("display_gpiochip_scan", e))?;
    for entry in entries {
        let entry = entry.map_err(|e| Error::io("display_gpiochip_scan", e))?;
        let name = entry.file_name();
        if name.to_string_lossy().starts_with("gpiochip") {
            chips.push(entry.path());
        }
    }
    Ok(chips)
}

pub struct LinuxSpiDisplayBackend {
    spi: Spidev,
    dc: LinuxOutputGpio,
    rst: Option<LinuxOutputGpio>,
    bl: Option<LinuxOutputGpio>,
    width: u16,
    height: u16,
    framebuf: Vec<u8>,
    max_transfer_sz: usize,
}

unsafe impl Send for LinuxSpiDisplayBackend {}

impl LinuxSpiDisplayBackend {
    pub fn new(config: &DisplayConfig) -> Result<Self> {
        let width = config.width;
        let height = config.height;
        let framebuf_len = width as usize * height as usize * 2;
        let desired_transfer_sz = desired_linux_spi_transfer_size(width, height);
        let max_transfer_sz =
            effective_linux_spi_transfer_size(width, height, read_linux_spidev_bufsiz());
        let path = linux_spi_device_path(config)?;

        let mut spi =
            Spidev::open(Path::new(&path)).map_err(|e| Error::io("display_spi_open", e))?;
        let options = SpidevOptions::new()
            .bits_per_word(8)
            .max_speed_hz(config.spi.freq_hz)
            .mode(SpiModeFlags::SPI_MODE_0)
            .build();
        spi.configure(&options)
            .map_err(|e| Error::io("display_spi_configure", e))?;

        let dc = LinuxOutputGpio::new_output(config.spi.dc, false)?;
        let rst = match config.spi.rst {
            Some(pin) => Some(LinuxOutputGpio::new_output(pin, true)?),
            None => None,
        };
        let bl = match config.spi.bl {
            Some(pin) => Some(LinuxOutputGpio::new_output(pin, true)?),
            None => None,
        };

        let mut backend = Self {
            spi,
            dc,
            rst,
            bl,
            width,
            height,
            framebuf: vec![0; framebuf_len],
            max_transfer_sz,
        };
        backend.reset_panel()?;
        backend.init_display_controller(config)?;
        log::info!(
            "[display_spi] {} {}x{} driver={:?} transfer={}B desired={}B",
            path,
            width,
            height,
            config.driver,
            max_transfer_sz,
            desired_transfer_sz
        );
        Ok(backend)
    }

    pub fn set_backlight(&self, on: bool) -> Result<()> {
        if let Some(bl) = self.bl.as_ref() {
            bl.write(on)?;
        }
        Ok(())
    }

    pub fn flush(&mut self, offset_x: i16, offset_y: i16) -> Result<()> {
        self.flush_rows(offset_x, offset_y, 0, self.height)
    }

    pub fn flush_rows(&mut self, offset_x: i16, offset_y: i16, ry: u16, rh: u16) -> Result<()> {
        if rh == 0 || self.width == 0 {
            return Ok(());
        }
        let ry = ry.min(self.height);
        let rh = rh.min(self.height.saturating_sub(ry));
        if rh == 0 {
            return Ok(());
        }

        let x0 = offset_x.max(0) as u16;
        let y0 = offset_y.max(0) as u16 + ry;
        let x1 = x0 + self.width - 1;
        let y1 = y0 + rh - 1;

        self.send_cmd(0x2A)?;
        self.send_data(&[(x0 >> 8) as u8, x0 as u8, (x1 >> 8) as u8, x1 as u8])?;

        self.send_cmd(0x2B)?;
        self.send_data(&[(y0 >> 8) as u8, y0 as u8, (y1 >> 8) as u8, y1 as u8])?;

        self.send_cmd(0x2C)?;
        self.dc.write(true)?;

        let row_bytes = self.width as usize * 2;
        let start = ry as usize * row_bytes;
        let end = start + rh as usize * row_bytes;
        let spi = &mut self.spi;
        for chunk in self.framebuf[start..end].chunks(self.max_transfer_sz) {
            spi.write_all(chunk)
                .map_err(|e| Error::io("display_spi_write", e))?;
        }
        Ok(())
    }

    fn reset_panel(&mut self) -> Result<()> {
        let Some(rst) = self.rst.as_ref() else {
            return Ok(());
        };
        rst.write(false)?;
        std::thread::sleep(std::time::Duration::from_millis(20));
        rst.write(true)?;
        std::thread::sleep(std::time::Duration::from_millis(120));
        Ok(())
    }

    fn send_cmd(&mut self, cmd: u8) -> Result<()> {
        self.dc.write(false)?;
        self.spi_write(&[cmd])
    }

    fn send_data(&mut self, data: &[u8]) -> Result<()> {
        if data.is_empty() {
            return Ok(());
        }
        self.dc.write(true)?;
        self.spi_write(data)
    }

    fn spi_write(&mut self, data: &[u8]) -> Result<()> {
        self.spi
            .write_all(data)
            .map_err(|e| Error::io("display_spi_write", e))
    }

    fn init_display_controller(&mut self, config: &DisplayConfig) -> Result<()> {
        self.send_cmd(0x01)?;
        std::thread::sleep(std::time::Duration::from_millis(150));

        self.send_cmd(0x11)?;
        std::thread::sleep(std::time::Duration::from_millis(120));

        match config.driver {
            DisplayDriver::Framebuffer => {
                return Err(Error::config(
                    "display_spi_init",
                    "driver=framebuffer is not valid for Linux SPI backend",
                ));
            }
            DisplayDriver::St7735 => {
                self.send_cmd(0xB1)?;
                self.send_data(&[0x01, 0x2C, 0x2D])?;
                self.send_cmd(0xB2)?;
                self.send_data(&[0x01, 0x2C, 0x2D])?;
                self.send_cmd(0xB3)?;
                self.send_data(&[0x01, 0x2C, 0x2D, 0x01, 0x2C, 0x2D])?;
                self.send_cmd(0xB4)?;
                self.send_data(&[0x07])?;
                self.send_cmd(0xC0)?;
                self.send_data(&[0xA2, 0x02, 0x84])?;
                self.send_cmd(0xC1)?;
                self.send_data(&[0xC5])?;
                self.send_cmd(0xC2)?;
                self.send_data(&[0x0A, 0x00])?;
                self.send_cmd(0xC3)?;
                self.send_data(&[0x8A, 0x2A])?;
                self.send_cmd(0xC4)?;
                self.send_data(&[0x8A, 0xEE])?;
                self.send_cmd(0xC5)?;
                self.send_data(&[0x0E])?;
                self.send_cmd(0x3A)?;
                self.send_data(&[0x05])?;
                self.send_cmd(0xE0)?;
                self.send_data(&[
                    0x02, 0x1c, 0x07, 0x12, 0x37, 0x32, 0x29, 0x2d, 0x29, 0x25, 0x2B, 0x39, 0x00,
                    0x01, 0x03, 0x10,
                ])?;
                self.send_cmd(0xE1)?;
                self.send_data(&[
                    0x03, 0x1d, 0x07, 0x06, 0x2E, 0x2C, 0x29, 0x2D, 0x2E, 0x2E, 0x37, 0x3F, 0x00,
                    0x00, 0x02, 0x10,
                ])?;
            }
            DisplayDriver::St7789 | DisplayDriver::Ili9341 => {
                self.send_cmd(0x3A)?;
                self.send_data(&[0x55])?;
            }
        }

        let madctl = compute_madctl(config.rotation, &config.color_order);
        self.send_cmd(0x36)?;
        self.send_data(&[madctl])?;

        let needs_invon = match config.driver {
            DisplayDriver::Framebuffer => false,
            DisplayDriver::St7789 => !config.invert_colors,
            DisplayDriver::Ili9341 | DisplayDriver::St7735 => config.invert_colors,
        };
        if needs_invon {
            self.send_cmd(0x21)?;
        } else {
            self.send_cmd(0x20)?;
        }

        self.send_cmd(0x13)?;
        std::thread::sleep(std::time::Duration::from_millis(10));
        self.send_cmd(0x29)?;
        std::thread::sleep(std::time::Duration::from_millis(20));
        self.set_backlight(true)?;
        Ok(())
    }

    #[inline]
    fn set_pixel(&mut self, x: u16, y: u16, color: Rgb565) {
        if x < self.width && y < self.height {
            let offset = (y as usize * self.width as usize + x as usize) * 2;
            let raw = RawU16::from(color).into_inner().to_be();
            self.framebuf[offset] = (raw >> 8) as u8;
            self.framebuf[offset + 1] = raw as u8;
        }
    }
}

impl DrawTarget for LinuxSpiDisplayBackend {
    type Color = Rgb565;
    type Error = Infallible;

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

impl OriginDimensions for LinuxSpiDisplayBackend {
    fn size(&self) -> Size {
        Size::new(self.width as u32, self.height as u32)
    }
}

fn desired_linux_spi_transfer_size(width: u16, height: u16) -> usize {
    let framebuf_len = width as usize * height as usize * 2;
    (width as usize * 20 * 2).min(framebuf_len.max(1))
}

fn effective_linux_spi_transfer_size(width: u16, height: u16, spidev_bufsiz: usize) -> usize {
    desired_linux_spi_transfer_size(width, height)
        .min(spidev_bufsiz.max(1))
        .max(1)
}

#[cfg(target_os = "linux")]
fn read_linux_spidev_bufsiz() -> usize {
    const DEFAULT_SPI_BUF_SIZE: usize = 4096;
    match std::fs::read_to_string("/sys/module/spidev/parameters/bufsiz") {
        Ok(raw) => raw
            .trim()
            .parse::<usize>()
            .ok()
            .filter(|v| *v > 0)
            .unwrap_or(DEFAULT_SPI_BUF_SIZE),
        Err(_) => DEFAULT_SPI_BUF_SIZE,
    }
}

fn linux_spi_device_path(config: &DisplayConfig) -> Result<String> {
    let configured = config.fb_device.trim();
    if configured.starts_with("/dev/spidev") {
        return Ok(configured.to_string());
    }
    if config.spi.cs < 0 {
        return Err(Error::config(
            "display_spi_path",
            "Linux SPI backend requires spi.cs >= 0 for /dev/spidevX.Y fallback",
        ));
    }
    let bus = config.spi.host.saturating_sub(1);
    Ok(format!("/dev/spidev{bus}.{}", config.spi.cs))
}

fn compute_madctl(rotation: u16, color_order: &DisplayColorOrder) -> u8 {
    let color_bit = match color_order {
        DisplayColorOrder::Rgb => 0x00,
        DisplayColorOrder::Bgr => 0x08,
    };
    let rot_bits = match rotation {
        90 => 0x60,
        180 => 0xC0,
        270 => 0xA0,
        _ => 0x00,
    };
    rot_bits | color_bit
}

#[cfg(test)]
mod tests {
    use super::{desired_linux_spi_transfer_size, effective_linux_spi_transfer_size};

    #[test]
    fn linux_spi_transfer_size_respects_spidev_bufsiz_limit() {
        assert_eq!(desired_linux_spi_transfer_size(240, 240), 9_600);
        assert_eq!(effective_linux_spi_transfer_size(240, 240, 4_096), 4_096);
    }

    #[test]
    fn linux_spi_transfer_size_keeps_small_panels_unchanged() {
        assert_eq!(desired_linux_spi_transfer_size(128, 160), 5_120);
        assert_eq!(effective_linux_spi_transfer_size(128, 160, 8_192), 5_120);
    }
}
