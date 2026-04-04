//! Linux framebuffer display backend.
//! 使用 `/dev/fbX` + mmap 实现 `embedded-graphics` DrawTarget，支持 16bpp 与 32bpp ARGB。
//!
//! 平台隔离：所有 open/mmap/ioctl 调用**仅**出现在此文件中，上层通过 `LinuxFramebufferBackend`
//! 抽象使用，不暴露任何 libc 原语。

#[allow(unused_imports)]
use crate::display::DisplayConfig;
use crate::error::{Error, Result};
use embedded_graphics_core::{
    draw_target::DrawTarget,
    geometry::{OriginDimensions, Size},
    pixelcolor::Rgb565,
    Pixel,
};
use embedded_graphics_framebuf::{backends::FrameBufferBackend, FrameBuf};
use std::convert::Infallible;
use std::os::unix::io::RawFd;

// ── Linux framebuffer ioctl 常量（来自 include/uapi/linux/fb.h）────────────────
// 用 c_int：musl 下 Ioctl = c_int；通过 `as libc::c_int` 传递。
const FBIOGET_VSCREENINFO: libc::c_int = 0x4600_u32 as libc::c_int;
const FBIOGET_FSCREENINFO: libc::c_int = 0x4602_u32 as libc::c_int;

/// fb_bitfield（来自 uapi/linux/fb.h）
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct FbBitfield {
    offset: u32,
    length: u32,
    msb_right: u32,
}

/// fb_var_screeninfo（来自 uapi/linux/fb.h）
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct FbVarScreeninfo {
    xres: u32,
    yres: u32,
    xres_virtual: u32,
    yres_virtual: u32,
    xoffset: u32,
    yoffset: u32,
    bits_per_pixel: u32,
    grayscale: u32,
    red: FbBitfield,
    green: FbBitfield,
    blue: FbBitfield,
    transp: FbBitfield,
    nonstd: u32,
    activate: u32,
    height: u32,
    width: u32,
    accel_flags: u32,
    pixclock: u32,
    left_margin: u32,
    right_margin: u32,
    upper_margin: u32,
    lower_margin: u32,
    hsync_len: u32,
    vsync_len: u32,
    sync: u32,
    vmode: u32,
    rotate: u32,
    colorspace: u32,
    reserved: [u32; 4],
}

/// fb_fix_screeninfo（来自 uapi/linux/fb.h）
#[repr(C)]
#[derive(Clone, Copy)]
struct FbFixScreeninfo {
    id: [u8; 16],
    smem_start: libc::c_ulong,
    smem_len: u32,
    fb_type: u32,
    type_aux: u32,
    visual: u32,
    xpanstep: u16,
    ypanstep: u16,
    ywrapstep: u16,
    _pad: u16,
    line_length: u32,
    mmio_start: libc::c_ulong,
    mmio_len: u32,
    accel: u32,
    capabilities: u16,
    reserved: [u16; 2],
}

impl Default for FbFixScreeninfo {
    fn default() -> Self {
        // SAFETY: 全零是该 repr(C) POD 结构体的合法初始态。
        unsafe { core::mem::zeroed() }
    }
}

// ── Vec 像素缓冲区后端（供 FrameBuf 使用）───────────────────────────────────────

/// 动态大小的 Rgb565 像素缓冲区，实现 `FrameBufferBackend`。
/// 用于替代 const-generic 数组后端，支持运行时分辨率。
struct PixelVec(Vec<Rgb565>);

impl PixelVec {
    fn new(size: usize) -> Self {
        use embedded_graphics_core::pixelcolor::RgbColor;
        Self(vec![Rgb565::BLACK; size])
    }
}

impl FrameBufferBackend for PixelVec {
    type Color = Rgb565;

    fn set(&mut self, index: usize, color: Rgb565) {
        self.0[index] = color;
    }

    fn get(&self, index: usize) -> Rgb565 {
        self.0[index]
    }

    fn nr_elements(&self) -> usize {
        self.0.len()
    }
}

// ── LinuxFramebufferBackend ──────────────────────────────────────────────────

/// Linux `/dev/fbX` framebuffer 显示后端。
///
/// 内部维护与 framebuffer 分辨率一致的 `FrameBuf<Rgb565, PixelVec>`，渲染后通过
/// `flush` / `flush_rows` 将脏区域写入 mmap 映射的内核帧缓冲区。
pub struct LinuxFramebufferBackend {
    fd: RawFd,
    /// mmap 映射起始地址与字节长度。
    mmap_ptr: *mut u8,
    mmap_len: usize,
    /// 内核报告的行字节步长（可能大于 width * bytes_per_pixel）。
    line_length: u32,
    /// bits_per_pixel（16 或 32）。
    bits_per_pixel: u32,
    /// xoffset / yoffset（内核 fb 面板坐标偏移，通常为 0）。
    fb_xoffset: u32,
    fb_yoffset: u32,
    /// 配置宽高（用于 DrawTarget::size 与 flush 边界检查）。
    width: u16,
    height: u16,
    /// 内存 DrawTarget。
    fbuf: FrameBuf<Rgb565, PixelVec>,
}

// SAFETY: LinuxFramebufferBackend 仅在显示线程中使用（Mutex 保护）。mmap_ptr 不跨线程共享。
unsafe impl Send for LinuxFramebufferBackend {}

impl Drop for LinuxFramebufferBackend {
    fn drop(&mut self) {
        if !self.mmap_ptr.is_null() && self.mmap_len > 0 {
            // SAFETY: mmap_ptr 与 mmap_len 来自 mmap 成功返回。
            unsafe {
                libc::munmap(self.mmap_ptr as *mut libc::c_void, self.mmap_len);
            }
        }
        if self.fd >= 0 {
            // SAFETY: fd 是有效的打开文件描述符。
            unsafe {
                libc::close(self.fd);
            }
        }
    }
}

impl LinuxFramebufferBackend {
    /// 打开 fb 设备、读取 vinfo/finfo、mmap，构建内存 DrawTarget。
    ///
    /// 若设备不存在或权限不足，返回 `Error::io`（上层可降级为 display 不可用）。
    pub fn new(config: &DisplayConfig) -> Result<Self> {
        let path = std::ffi::CString::new(config.fb_device.as_bytes())
            .map_err(|_| Error::config("display_init", "fb_device path contains NUL byte"))?;

        // SAFETY: path 是有效 C 字符串；O_RDWR = 2。
        let fd = unsafe { libc::open(path.as_ptr(), libc::O_RDWR) };
        if fd < 0 {
            return Err(Error::io("display_init", std::io::Error::last_os_error()));
        }

        // 读取变长屏幕信息。
        let mut vinfo = FbVarScreeninfo::default();
        let ret = unsafe { libc::ioctl(fd, FBIOGET_VSCREENINFO, &mut vinfo) };
        if ret < 0 {
            let e = std::io::Error::last_os_error();
            unsafe { libc::close(fd) };
            return Err(Error::io("display_init", e));
        }

        // 读取固定屏幕信息（获取 line_length 与 smem_len）。
        let mut finfo = FbFixScreeninfo::default();
        let ret = unsafe { libc::ioctl(fd, FBIOGET_FSCREENINFO, &mut finfo) };
        if ret < 0 {
            let e = std::io::Error::last_os_error();
            unsafe { libc::close(fd) };
            return Err(Error::io("display_init", e));
        }

        let bpp = vinfo.bits_per_pixel;
        if bpp != 16 && bpp != 32 {
            unsafe { libc::close(fd) };
            return Err(Error::config(
                "display_init",
                format!("framebuffer bpp={} is not supported (only 16 or 32)", bpp),
            ));
        }

        // 配置分辨率与 fb 报告值须匹配（首版不做拉伸）。
        let fb_w = vinfo.xres;
        let fb_h = vinfo.yres;
        if fb_w != config.width as u32 || fb_h != config.height as u32 {
            log::warn!(
                "[display_fb] config {}x{} != fb {}x{}; using fb dimensions",
                config.width,
                config.height,
                fb_w,
                fb_h
            );
        }
        let width = fb_w as u16;
        let height = fb_h as u16;

        let mmap_len = finfo.smem_len as usize;
        if mmap_len == 0 {
            unsafe { libc::close(fd) };
            return Err(Error::config(
                "display_init",
                "fb smem_len is 0, cannot mmap framebuffer",
            ));
        }

        // SAFETY: 参数来自内核 finfo；MAP_SHARED 将写入同步回帧缓冲区。
        let mmap_ptr = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                mmap_len,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                fd,
                0,
            )
        };
        if mmap_ptr == libc::MAP_FAILED {
            let e = std::io::Error::last_os_error();
            unsafe { libc::close(fd) };
            return Err(Error::io("display_init", e));
        }

        let pixel_count = width as usize * height as usize;
        let fbuf = FrameBuf::new(PixelVec::new(pixel_count), width as usize, height as usize);

        log::info!(
            "[display_fb] {} {}x{} {}bpp line_length={} mmap={}B xoff={} yoff={}",
            config.fb_device,
            width,
            height,
            bpp,
            finfo.line_length,
            mmap_len,
            vinfo.xoffset,
            vinfo.yoffset,
        );

        Ok(Self {
            fd,
            mmap_ptr: mmap_ptr as *mut u8,
            mmap_len,
            line_length: finfo.line_length,
            bits_per_pixel: bpp,
            fb_xoffset: vinfo.xoffset,
            fb_yoffset: vinfo.yoffset,
            width,
            height,
            fbuf,
        })
    }

    /// 将 `framebuf` 中的指定行范围 `[row_start, row_start + row_count)` 写入 mmap。
    /// `cfg_offset_x` / `cfg_offset_y` 来自 `DisplayConfig`（SPI 面板偏移；framebuffer 模式通常为 0）。
    pub fn flush_rows(
        &mut self,
        _cfg_offset_x: i16,
        _cfg_offset_y: i16,
        row_start: u16,
        row_count: u16,
    ) -> Result<()> {
        let w = self.width as usize;
        let h = self.height as usize;
        let r0 = row_start as usize;
        let rc = row_count as usize;

        if rc == 0 || r0 >= h {
            return Ok(());
        }
        let r_end = (r0 + rc).min(h);

        let bytes_per_pixel = self.bits_per_pixel as usize / 8;
        let line_len = self.line_length as usize;
        let fb_xoff = self.fb_xoffset as usize;
        let fb_yoff = self.fb_yoffset as usize;

        for row in r0..r_end {
            let src_row_start = row * w;
            let src_pixels = &self.fbuf.data.0[src_row_start..src_row_start + w];
            let dst_row = (fb_yoff + row) * line_len + fb_xoff * bytes_per_pixel;

            if dst_row + w * bytes_per_pixel > self.mmap_len {
                break;
            }

            // SAFETY: dst_row + slice len 已通过上述边界检查。
            let dst = unsafe {
                std::slice::from_raw_parts_mut(self.mmap_ptr.add(dst_row), w * bytes_per_pixel)
            };

            if self.bits_per_pixel == 16 {
                // RGB565 → 16bpp 行拷贝（小端与帧缓冲区一致）。
                for (i, px) in src_pixels.iter().enumerate() {
                    let raw: u16 =
                        embedded_graphics_core::pixelcolor::IntoStorage::into_storage(*px);
                    let le = raw.to_le_bytes();
                    dst[i * 2] = le[0];
                    dst[i * 2 + 1] = le[1];
                }
            } else {
                // RGB565 → 32bpp ARGB8888 转换（常见 Linux fb 格式）。
                for (i, px) in src_pixels.iter().enumerate() {
                    let (r, g, b) = rgb565_to_rgb888(*px);
                    // ARGB8888: B G R A（x86/ARM little-endian 内存顺序）
                    dst[i * 4] = b;
                    dst[i * 4 + 1] = g;
                    dst[i * 4 + 2] = r;
                    dst[i * 4 + 3] = 0xFF;
                }
            }
        }
        Ok(())
    }

    /// 刷新全屏（委托 `flush_rows`）。
    pub fn flush(&mut self, cfg_offset_x: i16, cfg_offset_y: i16) -> Result<()> {
        self.flush_rows(cfg_offset_x, cfg_offset_y, 0, self.height)
    }
}

/// RGB565 → (r8, g8, b8) 分量扩展（5→8 bit 填充低位）。
#[inline]
fn rgb565_to_rgb888(px: Rgb565) -> (u8, u8, u8) {
    use embedded_graphics_core::pixelcolor::RgbColor;
    let r5 = px.r();
    let g6 = px.g();
    let b5 = px.b();
    // 5-bit → 8-bit: 复制高位填低位，保持最大亮度为 0xFF。
    let r8 = (r5 << 3) | (r5 >> 2);
    let g8 = (g6 << 2) | (g6 >> 4);
    let b8 = (b5 << 3) | (b5 >> 2);
    (r8, g8, b8)
}

// ── DrawTarget impl ──────────────────────────────────────────────────────────

impl DrawTarget for LinuxFramebufferBackend {
    type Color = Rgb565;
    type Error = Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> std::result::Result<(), Infallible>
    where
        I: IntoIterator<Item = Pixel<Rgb565>>,
    {
        self.fbuf.draw_iter(pixels)
    }
}

impl OriginDimensions for LinuxFramebufferBackend {
    fn size(&self) -> Size {
        Size::new(self.width as u32, self.height as u32)
    }
}
