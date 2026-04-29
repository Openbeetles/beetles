# 屏幕配置

[English](../en-us/display.md) | **中文** | [文档索引](../README.md)

本页说明 Beetle OS 的显示配置。
仅在设备接入屏幕时需要使用该配置项。

## 基本流程

1. 先判断你的屏幕走 `spi` 还是 `framebuffer`
2. 选对应的 `driver`
3. 填好分辨率、旋转和颜色相关字段
4. 保存配置
5. 仅在显示异常时再调整颜色与偏移

保存后的数据会落到 `config/display.json`。

## 支持的 `driver`

- `st7789`
- `ili9341`
- `framebuffer`

## 支持的 `bus`

- `spi`
- `framebuffer`

SPI LCD 通常使用 `st7789` 或 `ili9341`；系统已提供显示设备时通常使用 `framebuffer`。
SPI 模式下 `spi.host` 仅填写 `2` 或 `3`，请按实际接线选择。

## SPI 配置示例

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
    "rst_active_high": false,
    "bl": 38,
    "freq_hz": 40000000
  },
  "fb_device": "/dev/fb0"
}
```

## `framebuffer` 配置示例

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
    "host": 2,
    "sclk": 0,
    "mosi": 0,
    "cs": 0,
    "dc": 0,
    "rst": null,
    "rst_active_high": false,
    "bl": null,
    "freq_hz": 40000000
  },
  "fb_device": "/dev/fb0",
  "backlight_sysfs": "/sys/class/backlight/backlight0/brightness"
}
```

## 关键字段

| 字段 | 作用 |
|------|------|
| `enabled` | 是否启用屏幕 |
| `driver` | 屏幕驱动 |
| `bus` | 走 SPI 还是 framebuffer |
| `width` / `height` | 分辨率 |
| `rotation` | 旋转角度，SPI 下可用 `0/90/180/270` |
| `color_order` | 常见是 `rgb` 或 `bgr` |
| `invert_colors` | 用来修正常见反色问题 |
| `linux_spi_swap_bytes` | Linux SPI 下颜色仍不对时再试 |
| `offset_x` / `offset_y` | 画面偏移修正 |
| `sleep_timeout_secs` | 背光自动熄灭时间，`0` 表示关闭 |
| `spi.host` | SPI 模式下仅支持 `2` 或 `3`，按实际接线选择 |
| `spi.rst_active_high` | RST 高电平有效 |
| `fb_device` | framebuffer 设备路径 |
| `backlight_sysfs` | Linux 背光路径 |

## 常见问题修正项

- 未接入屏幕时，将 `enabled` 设为 `false`
- `framebuffer` 当前只支持 `rotation = 0`
- 颜色异常时，优先检查 `color_order`
- 仍存在反色时，再检查 `invert_colors`
- Linux SPI 显示仍异常时，最后检查 `linux_spi_swap_bytes`
- 画面偏移时，调整 `offset_x` 和 `offset_y`

## 显示内容

屏幕主要会显示这些内容：

- 当前状态
- 网络信息
- 通道状态
- 一些基础运行信息

## 相关文档

- 配置范围与顺序：[configuration.md](configuration.md)
- 硬件配置：[hardware-device-config.md](hardware-device-config.md)
