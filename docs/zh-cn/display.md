# 显示仪表板

[English](../en-us/display.md) | **中文** | [文档索引](../README.md)

这页讲的是 Beetle 的 SPI 屏幕怎么接、怎么配，以及屏幕上会显示什么。

如果你只是想先点亮屏幕，最短流程是：

1. 按 SPI 接好屏幕
2. 写好 `config/display.json`
3. 选对驱动、分辨率和 SPI 引脚
4. 重启设备看仪表板

## 支持的显示控制器

| 控制器 | 典型分辨率 | 说明 |
|--------|-----------|------|
| **ST7789** | 240x240、240x320 | 默认选择 |
| **ILI9341** | 240x320 | 常见 240x320 屏幕 |
| **ST7735** | 128x160、128x128、80x160 等 | 常见于 1.8 寸等小屏 |

`invert_colors` 用来切换常见的反色问题。

如果无法确认屏幕控制器，可先按下列经验判断：

- 很多 240x240 / 240x320 模块是 `st7789`
- 很多 240x320 TFT 模块是 `ili9341`
- 很多 1.8 寸 128x160 模块是 `st7735`

---

## 硬件接线

ESP32-S3 上常见的接线方式如下：

| 信号 | 说明 | 必需 |
|------|------|------|
| SCLK | SPI 时钟 | 是 |
| MOSI | SPI 数据输出（MISO 未使用） | 是 |
| CS | 片选 | 是 |
| DC | 数据/命令选择 | 是 |
| RST | 硬件复位（初始化时拉低→拉高） | 可选 |
| BL | 背光使能（初始化时拉高） | 可选 |

> 所有引脚号都按 ESP32-S3 的 GPIO 编号来填。`spi.host` 填 `1` 或 `2` 即可。

---

## 配置

显示配置可以在配置页里直接修改；如果你习惯手改文件，也可以改 `config/display.json`。

在 ESP 上，这套配置会直接驱动 SPI 总线和 GPIO。
在 Linux 上，`framebuffer` 适合系统已经提供显示设备的场景，`st7789` / `ili9341` / `st7735` 适合直接驱动 SPI 小屏。

### 完整配置示例

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

### ST7735 示例（1.8 寸 128×160）

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

### 字段说明

| 字段 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `version` | u32 | 1 | 配置模式版本号，必须为 `1` |
| `enabled` | bool | — | 启用/禁用显示。`false` 时不初始化任何 SPI 硬件 |
| `driver` | string | — | `"st7789"`、`"ili9341"` 或 `"st7735"` |
| `bus` | string | — | `"spi"` 或 `"framebuffer"` |
| `width` | u16 | — | 面板宽度（像素，1–480） |
| `height` | u16 | — | 面板高度（像素，1–480） |
| `rotation` | u16 | 0 | 显示旋转角度：`0`、`90`、`180` 或 `270` |
| `color_order` | string | `"rgb"` | `"rgb"` 或 `"bgr"` |
| `invert_colors` | bool | false | 翻转驱动器默认的色彩反转行为 |
| `offset_x` | i16 | 0 | 显示窗口水平偏移（-480 到 480） |
| `offset_y` | i16 | 0 | 显示窗口垂直偏移（-480 到 480） |
| `spi.host` | u8 | 1 | SPI 主机：`1` 或 `2` |
| `spi.sclk` | i32 | — | SPI 时钟 GPIO 引脚 |
| `spi.mosi` | i32 | — | SPI MOSI GPIO 引脚 |
| `spi.cs` | i32 | — | 片选 GPIO 引脚 |
| `spi.dc` | i32 | — | 数据/命令 GPIO 引脚 |
| `spi.rst` | i32? | null | 复位 GPIO 引脚（可选） |
| `spi.bl` | i32? | null | 背光 GPIO 引脚（可选） |
| `spi.freq_hz` | u32 | 40000000 | SPI 时钟频率（1–80 MHz） |
| `fb_device` | string | `"/dev/fb0"` | Linux 设备路径：framebuffer 模式填 `/dev/fbX`；SPI 模式可填 `/dev/spidevX.Y`，也可以留空 |
| `backlight_sysfs` | string? | null | Linux 背光路径；SPI 面板也可以直接用 `spi.bl` GPIO 控背光 |

---

## 屏幕上会看到什么

默认会看到这些内容：

- 设备当前状态，例如启动中、空闲、忙碌、异常
- 当前网络信息，例如热点地址或局域网 IP
- 已启用通道的大致状态
- 一些基础运行信息

你不需要理解内部布局或绘制方式，只要确认画面是否正常、信息是否清晰即可。

---

## 注意事项与技巧

1. **先点亮，再细调**：先把 `driver`、分辨率和引脚填对，再去调 `rotation`、`offset_x`、`offset_y`。

2. **颜色不对**：如果红蓝对调，就切换 `color_order` 为 `"rgb"` 或 `"bgr"`。

3. **像负片**：如果整屏像反色，切换 `invert_colors`。

4. **画面偏了**：如果内容被裁边或没居中，调 `offset_x` / `offset_y`。240x240 的 ST7789 模块常见需要 `offset_y: 80`。

5. **花屏或不稳定**：先把 `spi.freq_hz` 从 `40000000` 降到 `20000000`，不行再降到 `10000000`。

6. **Linux 路径怎么填**：
   - `framebuffer` 模式填 `/dev/fbX`
   - SPI 模式可以填 `/dev/spidevX.Y`
   - SPI 模式也可以留空，让程序按 `spi.host` 和 `spi.cs` 推导

7. **不用屏幕时**：把 `enabled` 设为 `false` 即可。

8. **Linux 下背光亮了但没画面**：先确认系统已经启用 SPI，并且你填写的 GPIO 引脚当前对应用程序可用。
