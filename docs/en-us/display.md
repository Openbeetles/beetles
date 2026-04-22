# Display Setup

**English** | [中文](../zh-cn/display.md) | [Doc index](../README.md)

Display setup starts only after you have chosen a screen for Beetls OS.
For normal use, the config UI is still the easiest place to set display options first.

## The Short Path

1. decide whether your display uses `spi` or `framebuffer`
2. choose the matching `driver`
3. fill resolution, rotation, and color settings
4. save the config
5. fix colors or offsets only if the image looks wrong

Saved data ends up in `config/display.json`.

## Supported `driver` Values

- `st7789`
- `ili9341`
- `st7735`
- `framebuffer`

## Supported `bus` Values

- `spi`
- `framebuffer`

If you use a small SPI panel, the first three drivers are the usual choices.
If your system already exposes a display device, `framebuffer` is the usual path.

## A Common SPI Example

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
  "sleep_timeout_secs": 0,
  "spi": {
    "host": 2,
    "sclk": 42,
    "mosi": 41,
    "cs": 21,
    "dc": 40,
    "rst": 39,
    "bl": 38,
    "freq_hz": 40000000
  },
  "fb_device": "/dev/fb0"
}
```

## A `framebuffer` Example

```json
{
  "version": 1,
  "enabled": true,
  "driver": "framebuffer",
  "bus": "framebuffer",
  "width": 240,
  "height": 240,
  "rotation": 0,
  "color_order": "rgb",
  "invert_colors": false,
  "linux_spi_swap_bytes": false,
  "offset_x": 0,
  "offset_y": 0,
  "sleep_timeout_secs": 30,
  "spi": {
    "host": 1,
    "sclk": 0,
    "mosi": 0,
    "cs": 0,
    "dc": 0,
    "rst": null,
    "bl": null,
    "freq_hz": 40000000
  },
  "fb_device": "/dev/fb0",
  "backlight_sysfs": "/sys/class/backlight/backlight0/brightness"
}
```

## Fields That Matter Most

| Field | What it controls |
|-------|------------------|
| `enabled` | whether the display is on |
| `driver` | display driver |
| `bus` | SPI or framebuffer |
| `width` / `height` | panel resolution |
| `rotation` | screen rotation; SPI supports `0/90/180/270` |
| `color_order` | usually `rgb` or `bgr` |
| `invert_colors` | fixes common inverted-color cases |
| `linux_spi_swap_bytes` | last Linux SPI fallback for wrong colors |
| `offset_x` / `offset_y` | image position adjustment |
| `sleep_timeout_secs` | auto backlight-off timeout; `0` disables it |
| `fb_device` | framebuffer device path |
| `backlight_sysfs` | Linux backlight path |

## What Usually Fixes Problems

- If you do not use a screen, set `enabled` to `false`
- `framebuffer` currently supports only `rotation = 0`
- If colors are wrong, try `color_order` first
- If it still looks inverted, try `invert_colors`
- If Linux SPI still looks wrong, try `linux_spi_swap_bytes` last
- If the image is shifted, adjust `offset_x` and `offset_y`

## What The Screen Shows

The screen mainly shows:

- current state
- network information
- channel status
- a few basic runtime details

## Read Next

- To get Beetls OS running first: [configuration.md](configuration.md)
- To connect hardware: [hardware-device-config.md](hardware-device-config.md)
