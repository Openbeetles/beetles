# Display dashboard

**English** | [中文](../zh-cn/display.md) | [Doc index](../README.md)

This page explains how to attach an SPI TFT screen to Beetle.

If you only need the shortest path:

1. wire the screen over SPI
2. create `config/display.json`
3. choose the right `driver`, `width`, `height`, and SPI pins
4. reboot and check the dashboard

The display module renders a real-time status dashboard with no bitmap assets or external fonts.

---

## Supported controllers

| Controller | Typical resolution | Notes |
|------------|-------------------|-------|
| **ST7789** | 240x240, 240x320 | Default; color inversion ON by default (panel-native) |
| **ILI9341** | 240x320 | Color inversion OFF by default |
| **ST7735** | 128x160, 128x128, 80x160, etc. | ST7735 / ST7735R / ST7735S family (register-compatible); inversion OFF by default; uses frame-rate, power, and gamma init |

ST7789 and ILI9341 share a short init (SWRESET → SLPOUT → COLMOD `0x55` → MADCTL → INV → NORON → DISPON). **ST7735** sends frame-rate, power, and gamma registers after SLPOUT, uses COLMOD `0x05`, then MADCTL → INV → NORON → DISPON. The `invert_colors` flag flips each driver's default inversion behavior.

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

> All pin numbers are ESP32-S3 GPIO numbers. `spi.host`: **1** = `SPI2_HOST`, **2** = `SPI3_HOST` (ESP-IDF `spi_host_device_t`).

---

## Configuration

Display configuration is stored in `config/display.json` on the SPIFFS partition. It can also be edited via the configuration web UI (Display section).

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
| `bus` | string | — | `"spi"` (only option currently) |
| `width` | u16 | — | Panel width in pixels (1–480) |
| `height` | u16 | — | Panel height in pixels (1–480) |
| `rotation` | u16 | 0 | Display rotation: `0`, `90`, `180`, or `270` |
| `color_order` | string | `"rgb"` | `"rgb"` or `"bgr"` |
| `invert_colors` | bool | false | Flip the driver's default color inversion |
| `offset_x` | i16 | 0 | Horizontal pixel offset for the display window (-480 to 480) |
| `offset_y` | i16 | 0 | Vertical pixel offset for the display window (-480 to 480) |
| `spi.host` | u8 | 1 | SPI host: `1` = SPI2_HOST, `2` = SPI3_HOST (same as ESP-IDF `spi_host_device_t`) |
| `spi.sclk` | i32 | — | SPI clock GPIO pin |
| `spi.mosi` | i32 | — | SPI MOSI GPIO pin |
| `spi.cs` | i32 | — | Chip select GPIO pin |
| `spi.dc` | i32 | — | Data/command GPIO pin |
| `spi.rst` | i32? | null | Reset GPIO pin (optional) |
| `spi.bl` | i32? | null | Backlight GPIO pin (optional) |
| `spi.freq_hz` | u32 | 40000000 | SPI clock frequency (1–80 MHz) |

---

## Dashboard layout

The dashboard uses a dark base fill (`DISPLAY_BG`), then three full-width **panel cards** (header, channel block, footer) with a slightly lighter fill (`PANEL_BG`) and a thin border. A **3px** top bar uses the state accent color. **Divider lines** sit a few pixels above the channel and footer bands (`middle_top` / `footer_top` from `display::compute_layout`). The title row has a narrow **title strip** behind the text; a **status pill** (same label as the main state title) sits above the title line.

**Subtitle (non-Booting):** IP address. If panel width **≥ 200px** and uptime is non-zero, **IP** and **`Up:`** runtime (e.g. `Up:1d2h`) are on **two lines** so the uptime is not clipped. On narrower panels, a **single line** combines truncated IP and ` Up:…` (stack buffer limit).

**Channel block:** Five channels appear as small row tiles. Labels are **abbreviated**: **TG** (telegram), **FS** (feishu), **DT** (dingtalk), **WC** (wecom), **QQ** (qq_channel). Layout is **2 columns** when width is **below 200px**, **3 columns** when **≥ 200px**. Each row shows a status dot (green / red / gray), the short label, and a token on the right: **OK**, **OFF**, **xN** (consecutive failures), or **DOWN**.

```
┌──────────────────────────────────┐ y=0
│▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓│ State-color top bar (3px)
│ ┌──────┐ ┌──────┐  STATE_NAME   │ Status pill + title (bold, state color)
│ │beetle│        │  192.168.x.x  │ IP (line 1); or one line on narrow
│ │ ~64  │        │  Up:1d2h      │ Uptime line 2 when width ≥200 & uptime>0
│ └──────┘                        │
├──────────────────────────────────┤ y≈middle_top (e.g. 104 @ 240×240)
│  ● TG  OK    ● FS  OK   ● DT OFF │ 2 or 3 columns by width
│  ● WC  OK    ● QQ  x2            │
├──────────────────────────────────┤ y≈footer_top
│  NORMAL  ████████░░░░ 62%        │ Pressure badge + heap bar + %
│  In:12 Out:8 L:1.2s 14:30         │ Message stats + last activity time
└──────────────────────────────────┘ y=height
```

Vertical positions (`header_top`, `subtitle_top`, `middle_top`, `footer_top`, `margin_x`) come from `compute_layout` (240×240 reference grid, aspect buckets).

### Beetle icon states

The beetle is drawn entirely with `embedded-graphics` primitives (circles, lines) — no bitmap or PNG resources.

| State | Beetle color | Visual cues |
|-------|-------------|-------------|
| **Booting** | Amber gold | Loading dots on body; dashed WiFi arcs above head; subtitle shows firmware version |
| **NoWifi** | Blue-gray | Solid WiFi arcs above head, crossed out with red X |
| **Idle** | Neon green | White checkmark on body |
| **Busy** | Cyan-blue | Membrane wings spread from under elytra; alternating white dots on body (breathing animation) |
| **Fault** | Bright red | Flipped upside-down; X eyes; exclamation mark on body |

### Channel status indicators

| Visual | Meaning |
|--------|---------|
| Filled green dot + normal text | Enabled channel, healthy |
| Filled red dot + normal text | Enabled channel, unhealthy |
| Hollow gray circle + dimmed text | Disabled channel |
| Right-side token **OK** / **OFF** / **xN** / **DOWN** | Healthy, disabled, failure count, or unhealthy with no count |

### Footer

- Pressure label: **NORMAL** (green) / **CAUTIOUS** (yellow) / **CRITICAL** (red); on **error flash** the badge fills with the accent color for one frame.
- Heap usage progress bar with percentage to the right of the bar.
- **Stats line:** `In:` / `Out:` message counts, optional **`L:`** LLM latency when available, and last-activity clock **`HH:MM`** (local time on ESP when SNTP is configured).

---

## Partial updates

To reduce SPI traffic, partial updates are used for frequently changing regions:

| Command | What updates | Rows flushed |
|---------|-------------|-------------|
| `UpdateIp` | IP subtitle (and on wide panels, second-line `Up:` when uptime &gt; 0) | ~16 rows, or ~30 rows when width ≥ 200px and uptime is shown |
| `UpdateChannels` | Channel block only | From `middle_top` to footer |
| `UpdatePressure` | Footer pressure bar, heap bar, and stats line | ~72 rows (footer band) |
| `RefreshDashboard` | Full screen | All rows |

Partial row flush reduces SPI data by ~85% compared to full-screen refresh.

---

## Memory impact

| Item | Size | Location |
|------|------|----------|
| Framebuffer (240x240 RGB565) | 115,200 bytes | PSRAM |
| `SpiDisplayBackend` struct | ~64 bytes | Heap (internal) |
| Rendering stack usage | ~200 bytes | Stack |
| **Net internal DRAM impact** | **~0** | PSRAM absorbs the framebuffer |

The display thread runs with a 6 KB stack and refreshes every 5 seconds from `orchestrator::snapshot()`.

---

## Caveats and tips

1. **`enabled: false` means zero hardware init** — no SPI bus, no GPIO, no PSRAM allocation. Safe to leave in config even without a display connected.

2. **Offset values** — Some panels (notably 240x240 ST7789 on 240x320 glass) need `offset_x` or `offset_y` to shift the visible window. Common: `offset_y: 80` for 240x240 on a 240x320 panel.

3. **Color order** — If colors appear inverted (red↔blue), toggle `color_order` between `"rgb"` and `"bgr"`.

4. **`invert_colors`** — If the display looks like a photo negative, toggle this flag. ST7789 panels typically need inversion ON (the default), while ILI9341 panels need it OFF.

5. **SPI frequency** — 40 MHz works for most ST7789/ILI9341 panels. If you see visual artifacts or corruption, try lowering to 20 MHz or 10 MHz. Maximum supported is 80 MHz. **ST7735** is best at ≤15 MHz (e.g. `15_000_000`); higher speeds often cause corruption.

6. **ST7735 (1.8" 128×160, etc.)** — Many modules use 132×162 glass; set `offset_x: 2`, `offset_y: 1` with `width`/`height` 128/160. Some “black tab” variants use `0`/`0`—tune to your module. `color_order` is often `bgr`; try `rgb` if red and blue are swapped.

7. **Display thread stack** — The rendering thread uses 6 KB stack. This is sufficient for the current dashboard layout. If you add significantly more complex rendering, monitor for stack overflow.

8. **Host compilation** — On non-ESP targets (`cargo check`, `cargo clippy`), the display backend returns `available: false` and all commands are no-ops. This ensures the codebase compiles cleanly on the host.

9. **Rotation** — The `rotation` field applies a MADCTL transform at the controller level. The framebuffer dimensions (`width` x `height`) should match the post-rotation visible area.
