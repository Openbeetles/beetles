# 屏幕配置

[English](../en-us/display.md) | **中文** | [文档索引](../README.md)

正常使用时，优先在配置页里设置屏幕。

这页只说明屏幕配置现在有哪些字段，方便你看懂配置页、接口返回，或者排查保存结果。
保存之后，这些内容会落到 `config/display.json`。

## 当前支持的 `driver`

- `st7789`
- `ili9341`
- `st7735`
- `framebuffer`

## 当前支持的 `bus`

- `spi`
- `framebuffer`

如果你用的是 SPI 小屏，常见是前三种驱动。
如果系统已经有现成显示设备，一般用 `framebuffer`。

## 如果你查看底层数据

对应的是：

- `config/display.json`

## 一个常见 SPI 示例

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

## 一个 `framebuffer` 示例

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

## 你最需要关心的字段

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
| `fb_device` | framebuffer 设备路径 |
| `backlight_sysfs` | Linux 背光路径 |

## 几个直接结论

- 不用屏幕时，把 `enabled` 设为 `false`
- `framebuffer` 模式当前只支持 `rotation = 0`
- 颜色不对时，先试 `color_order`
- 还像反色时，再试 `invert_colors`
- Linux SPI 仍然不对时，最后再试 `linux_spi_swap_bytes`
- 画面偏了，再调 `offset_x` 和 `offset_y`

## 屏幕上会看到什么

屏幕主要会显示这些内容：

- 当前状态
- 网络信息
- 通道状态
- 一些基础运行信息

## 接下来读什么

- 想先把设备跑起来： [configuration.md](configuration.md)
- 想接硬件： [hardware-device-config.md](hardware-device-config.md)
