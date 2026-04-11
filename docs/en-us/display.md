# Display dashboard

**English** | [中文](../zh-cn/display.md) | [Doc index](../README.md)

This page explains how to attach an SPI TFT screen to Beetle.

If you only need the shortest path:

1. wire the screen over SPI
2. create `config/display.json`
3. choose the right `driver`, `width`, `height`, and SPI pins
4. reboot and check the dashboard

## Supported controllers

| Controller | Typical resolution | Notes |
|------------|-------------------|-------|
| **ST7789** | 240x240, 240x320 | Default choice |
| **ILI9341** | 240x320 | Common on 240x320 panels |
| **ST7735** | 128x160, 128x128, 80x160, etc. | Common on small displays such as 1.8-inch panels |

Use `invert_colors` when the panel looks like a photo negative.
If a Linux SPI panel still has wrong colors after trying `color_order` and `invert_colors`, enable `linux_spi_swap_bytes`.

If you are not sure which panel you have:

- many 240x240 / 240x320 modules are `st7789`
- many 240x320 TFT modules are `ili9341`
- many 1.8-inch 128x160 modules are `st7735`

---

## Hardware wiring

A typical SPI connection to ESP32-S3:

| Signal | Description | Required |
|--------|-------------|----------|
| SCLK | SPI clock | Yes |
| MOSI | SPI data out (MISO unused) | Yes |
| CS | Chip select | Yes |
| DC | Data/command select | Yes |
| RST | Hardware reset (pulse low→high on init) | Optional |
| BL | Backlight enable (set high on init) | Optional |

> All pin numbers are ESP32-S3 GPIO numbers. For `spi.host`, use `1` or `2`.

---

## Configuration

You can edit display settings directly in the config UI. If you prefer file-based config, use `config/display.json`.

On ESP, Beetle drives SPI panels directly from the configured pins.
On Linux, use `framebuffer` when the system already exposes a display device, or `st7789` / `ili9341` / `st7735` when you want Beetle to drive an SPI panel directly.
For Linux SPI panels, Beetle prefers `gpio-cdev`, falls back to sysfs GPIO when needed, and automatically chunks SPI writes to the kernel `spidev` buffer limit.

### Full config example

```json
{
  "version": 1,
  "enabled": true,
  "driver": "st7789",
  "bus": "spi",
  "width": 240,
  "height": 240,
  "rotation": 0,
  "color_order": "rgb",
  "invert_colors": false,
  "linux_spi_swap_bytes": false,
  "offset_x": 0,
  "offset_y": 0,
  "spi": {
    "host": 2,
    "sclk": 42,
    "mosi": 41,
    "cs": 21,
    "dc": 40,
    "rst": 39,
    "bl": 38,
    "freq_hz": 40000000
  }
}
```

### ST7735 example (1.8" 128×160)

```json
{
  "version": 1,
  "enabled": true,
  "driver": "st7735",
  "bus": "spi",
  "width": 128,
  "height": 160,
  "rotation": 0,
  "color_order": "bgr",
  "invert_colors": false,
  "linux_spi_swap_bytes": false,
  "offset_x": 2,
  "offset_y": 1,
  "spi": {
    "host": 2,
    "sclk": 42,
    "mosi": 41,
    "cs": 21,
    "dc": 40,
    "rst": null,
    "bl": null,
    "freq_hz": 15000000
  }
}
```

### Field reference

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `version` | u32 | 1 | Config schema version; must be `1` |
| `enabled` | bool | — | Enable/disable display. When `false`, no SPI hardware is initialized |
| `driver` | string | — | `"st7789"`, `"ili9341"`, or `"st7735"` |
| `bus` | string | — | `"spi"` or `"framebuffer"` |
| `width` | u16 | — | Panel width in pixels (1–480) |
| `height` | u16 | — | Panel height in pixels (1–480) |
| `rotation` | u16 | 0 | Display rotation: `0`, `90`, `180`, or `270` |
| `color_order` | string | `"rgb"` | `"rgb"` or `"bgr"` |
| `invert_colors` | bool | false | Flip the driver's default color inversion |
| `linux_spi_swap_bytes` | bool | false | Linux SPI only: swap RGB565 high/low bytes before transmission; use only when color order and invert do not fix colors |
| `offset_x` | i16 | 0 | Horizontal pixel offset for the display window (-480 to 480) |
| `offset_y` | i16 | 0 | Vertical pixel offset for the display window (-480 to 480) |
| `spi.host` | u8 | 1 | SPI host: `1` or `2` |
| `spi.sclk` | i32 | — | SPI clock GPIO pin |
| `spi.mosi` | i32 | — | SPI MOSI GPIO pin |
| `spi.cs` | i32 | — | Chip select GPIO pin |
| `spi.dc` | i32 | — | Data/command GPIO pin |
| `spi.rst` | i32? | null | Reset GPIO pin (optional) |
| `spi.bl` | i32? | null | Backlight GPIO pin (optional) |
| `spi.freq_hz` | u32 | 40000000 | SPI clock frequency (1–80 MHz) |
| `fb_device` | string | `"/dev/fb0"` | Linux device path: framebuffer mode uses `/dev/fbX`; SPI mode can use `/dev/spidevX.Y`, or be left empty |
| `backlight_sysfs` | string? | null | Linux backlight path; SPI panels can also use `spi.bl` GPIO instead |

---

## What appears on screen

By default, the screen shows:

- the current device state, such as booting, idle, busy, or fault
- current network information, such as hotspot address or LAN IP
- a simple summary of enabled channels
- a few basic runtime details

You do not need to understand the internal drawing layout. What matters is whether the screen is readable and whether the displayed information looks correct.

---

## Caveats and tips

1. **Get it working first, then fine-tune** — start with the correct `driver`, resolution, and pins. Only then adjust `rotation`, `offset_x`, and `offset_y`.

2. **Wrong colors** — if red and blue are swapped, switch `color_order` between `"rgb"` and `"bgr"`.

3. **Photo-negative look** — toggle `invert_colors`.

4. **Linux SPI colors still wrong after both steps above** — enable `linux_spi_swap_bytes`. This is a Linux SPI-only fallback for panels that expect swapped RGB565 byte order.

5. **Shifted or cropped image** — adjust `offset_x` / `offset_y`. A 240x240 ST7789 module often needs `offset_y: 80`.

6. **Artifacts or unstable image** — lower `spi.freq_hz` from `40000000` to `20000000`, then to `10000000` if needed.

7. **Linux path**:
   - use `/dev/fbX` for `framebuffer`
   - use `/dev/spidevX.Y` for SPI panels if you want to set it explicitly
   - leaving the SPI path empty is also fine

8. **No screen attached** — set `enabled` to `false`.

9. **Backlight turns on but no picture on Linux** — check that the OS has SPI enabled and that the configured GPIO pins are actually available to the application.
