# 显示仪表板

[English](../en-us/display.md) | **中文** | [文档索引](../README.md)

Beetle 的 SPI 屏幕接入方式如下。

最短配置流程：

1. 按 SPI 接好屏幕
2. 写好 `config/display.json`
3. 选对 `driver`、分辨率和 SPI 引脚
4. 重启设备看仪表板

显示模块会实时渲染运行状态仪表板，不依赖位图资源或外部字体。

---

## 支持的显示控制器

| 控制器 | 典型分辨率 | 说明 |
|--------|-----------|------|
| **ST7789** | 240x240、240x320 | 默认选择；面板原生色彩反转 ON |
| **ILI9341** | 240x320 | 色彩反转默认 OFF |
| **ST7735** | 128x160、128x128、80x160 等 | ST7735 / ST7735R / ST7735S 家族（寄存器兼容）；色彩反转默认 OFF；需专用初始化（帧率/电源/伽马） |

ST7789 与 ILI9341 共用短初始化序列（SWRESET → SLPOUT → COLMOD `0x55` → MADCTL → INV → NORON → DISPON）。**ST7735** 在 SLPOUT 之后发送帧率、电源、伽马等寄存器，COLMOD 为 `0x05`，再接 MADCTL → INV → NORON → DISPON。`invert_colors` 用于翻转各驱动默认的反色行为。

如果无法确认屏幕控制器，可先按下列经验判断：

- 很多 240x240 / 240x320 模块是 `st7789`
- 很多 240x320 TFT 模块是 `ili9341`
- 很多 1.8 寸 128x160 模块是 `st7735`

---

## 硬件接线

ESP32-S3 的典型 SPI 接线：

| 信号 | 说明 | 必需 |
|------|------|------|
| SCLK | SPI 时钟 | 是 |
| MOSI | SPI 数据输出（MISO 未使用） | 是 |
| CS | 片选 | 是 |
| DC | 数据/命令选择 | 是 |
| RST | 硬件复位（初始化时拉低→拉高） | 可选 |
| BL | 背光使能（初始化时拉高） | 可选 |

> 所有引脚号为 ESP32-S3 GPIO 编号。`spi.host`：**1** = `SPI2_HOST`，**2** = `SPI3_HOST`（与 ESP-IDF `spi_host_device_t` 一致）。

---

## 配置

显示配置存储在 SPIFFS 分区的 `config/display.json` 中，也可通过配置 Web UI（显示 Display 页面）编辑。

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
| `bus` | string | — | `"spi"`（当前唯一选项） |
| `width` | u16 | — | 面板宽度（像素，1–480） |
| `height` | u16 | — | 面板高度（像素，1–480） |
| `rotation` | u16 | 0 | 显示旋转角度：`0`、`90`、`180` 或 `270` |
| `color_order` | string | `"rgb"` | `"rgb"` 或 `"bgr"` |
| `invert_colors` | bool | false | 翻转驱动器默认的色彩反转行为 |
| `offset_x` | i16 | 0 | 显示窗口水平偏移（-480 到 480） |
| `offset_y` | i16 | 0 | 显示窗口垂直偏移（-480 到 480） |
| `spi.host` | u8 | 1 | SPI 主机：`1` = SPI2_HOST，`2` = SPI3_HOST（与 ESP-IDF `spi_host_device_t` 一致） |
| `spi.sclk` | i32 | — | SPI 时钟 GPIO 引脚 |
| `spi.mosi` | i32 | — | SPI MOSI GPIO 引脚 |
| `spi.cs` | i32 | — | 片选 GPIO 引脚 |
| `spi.dc` | i32 | — | 数据/命令 GPIO 引脚 |
| `spi.rst` | i32? | null | 复位 GPIO 引脚（可选） |
| `spi.bl` | i32? | null | 背光 GPIO 引脚（可选） |
| `spi.freq_hz` | u32 | 40000000 | SPI 时钟频率（1–80 MHz） |

---

## 仪表板布局

仪表板先铺深色底（`DISPLAY_BG`），再分 **上 / 中 / 下** 三块 **面板卡片**（`PANEL_BG` + 细边框）。顶部 **3px** 为状态色强调条；在通道区、页脚区上方各有 **分隔线**（相对 `middle_top`、`footer_top` 略上移，与 `display::compute_layout` 一致）。标题行有 **标题条** 背景；主标题上方另有与状态同名的 **状态胶囊**。

**副标题（非 Booting）：** 显示 IP。屏宽 **≥ 200px** 且运行时间大于 0 时，**IP** 与 **`Up:`** 运行时长（如 `Up:1d2h`）分 **两行**，避免单行裁切；更窄的屏仍 **单行** 显示截断后的 IP 与 ` Up:…`（栈缓冲限制）。

**通道区：** 五个通道为行式小卡片；屏上缩写为 **TG / FS / DT / WC / QQ**（对应 telegram / feishu / dingtalk / wecom / qq_channel）。屏宽 **小于 200px** 为 **两列**，**≥ 200px** 为 **三列**。每行左侧色点、缩写，右侧令牌 **OK / OFF / xN / DOWN**。

```
┌──────────────────────────────────┐ y=0
│▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓│ 状态色顶条（3px）
│ ┌──────┐ ┌──────┐  状态名称     │ 状态胶囊 + 标题（粗体）
│ │甲壳虫│        │  192.168.x.x  │ IP（第 1 行；窄屏合并一行）
│ │约64px│        │  Up:1d2h      │ 宽屏第 2 行：运行时间
│ └──────┘                        │
├──────────────────────────────────┤ y≈middle_top（如 240×240 时约 104）
│  ● TG OK   ● FS OK   ● DT OFF   │ 按宽度 2 或 3 列
│  ● WC OK   ● QQ x2              │
├──────────────────────────────────┤ y≈footer_top
│  NORMAL  ████████░░░░ 62%       │ 压力胶囊 + 堆条 + 百分比
│  In:12 Out:8 L:1.2s 14:30       │ 消息统计 + 最近活动时间
└──────────────────────────────────┘ y=height
```

纵向坐标（`header_top`、`subtitle_top`、`middle_top`、`footer_top`、`margin_x`）由 `compute_layout` 按分辨率与宽高比分档计算（240 参考网格）。

### 甲壳虫图标状态

甲壳虫完全由 `embedded-graphics` 原语（圆、线）绘制，不使用任何位图或 PNG 资源。

| 状态 | 甲壳虫颜色 | 视觉特征 |
|------|-----------|----------|
| **启动中 (Booting)** | 琥珀金 | 身体上的加载圆点；头上方虚线 WiFi 弧线；副标题显示固件版本号 |
| **无 WiFi (NoWifi)** | 蓝灰 | 头上方实线 WiFi 弧线，被红色 X 划掉 |
| **空闲 (Idle)** | 霓虹绿 | 身体上的白色对勾 ✓ |
| **繁忙 (Busy)** | 赛博蓝 | 鞘翅下展开的膜翅；身体上的白点交替大小（呼吸动画） |
| **异常 (Fault)** | 鲜红 | 翻转（头朝下）；X 眼；身体上的白色感叹号 |

### 通道状态指示

| 视觉样式 | 含义 |
|----------|------|
| 绿色实心圆 + 正常文字 | 已启用通道，健康 |
| 红色实心圆 + 正常文字 | 已启用通道，不健康 |
| 灰色空心圆 + 暗淡文字 | 未启用通道 |
| 右侧 **OK** / **OFF** / **xN** / **DOWN** | 健康、未启用、连续失败次数、或不健康且无计数 |

### 底部栏

- 压力等级标签：**NORMAL**（绿色）/ **CAUTIOUS**（黄色）/ **CRITICAL**（红色）；**错误闪烁** 时一帧内用强调色填充胶囊背景。
- 堆使用率进度条，右侧为百分比。
- **统计行：** `In:` / `Out:` 消息计数；有数据时可选 **`L:`** LLM 延迟；以及最近活动时间 **`HH:MM`**（ESP 上在 SNTP 可用时为本地时间）。

---

## 局部更新

为减少 SPI 流量，常变区域使用局部行刷新：

| 命令 | 更新内容 | 刷新行数 |
|------|---------|----------|
| `UpdateIp` | IP 副标题（宽屏且 uptime&gt;0 时含第二行 `Up:`） | 约 16 行；宽屏展示运行时间时约 30 行 |
| `UpdateChannels` | 仅中间通道区 | 自 `middle_top` 至页脚上方 |
| `UpdatePressure` | 底部压力、堆条与统计行 | ~72 行（页脚带） |
| `RefreshDashboard` | 全屏 | 全部行 |

局部行刷新相比全屏刷新可减少约 85% 的 SPI 数据传输量。

---

## 内存影响

| 项目 | 大小 | 位置 |
|------|------|------|
| 帧缓冲 (240x240 RGB565) | 115,200 字节 | PSRAM |
| `SpiDisplayBackend` 结构体 | ~64 字节 | 堆（internal） |
| 渲染栈使用 | ~200 字节 | 栈 |
| **净 internal DRAM 影响** | **约 0** | PSRAM 承担帧缓冲开销 |

显示线程使用 6 KB 栈，每 5 秒从 `orchestrator::snapshot()` 读取快照并刷新。

---

## 注意事项与技巧

1. **`enabled: false` 表示零硬件初始化** — 不初始化 SPI 总线、不配置 GPIO、不分配 PSRAM。即使没有连接显示屏，也可以安全地保留在配置中。

2. **偏移值 (offset)** — 部分面板（尤其是 240x240 ST7789 使用 240x320 玻璃基板时）需要设置 `offset_x` 或 `offset_y` 来移动可视窗口。常见场景：240x240 面板在 240x320 基板上时设置 `offset_y: 80`。

3. **色彩顺序 (color_order)** — 如果颜色出现红蓝互换，尝试在 `"rgb"` 和 `"bgr"` 之间切换。

4. **色彩反转 (invert_colors)** — 如果显示效果像照片负片，切换此标志。ST7789 面板通常需要反转 ON（默认行为），ILI9341 面板通常需要反转 OFF。

5. **SPI 频率** — 40 MHz 适用于大多数 ST7789/ILI9341。如果出现花屏或显示异常，可尝试降低到 20 MHz 或 10 MHz。最高支持 80 MHz。**ST7735** 建议 ≤15 MHz（如 15_000_000），过高易花屏。

6. **ST7735（1.8 寸 128×160 等）** — 多数模块玻璃为 132×162，可视区需窗口偏移：典型为 `offset_x: 2`、`offset_y: 1`（与 `width`/`height` 填 128/160 配合）；少数「黑板」类模块为 `0`/`0`，需按实物调整。色序常为 `bgr`，若红蓝互换再试 `rgb`。

7. **显示线程栈** — 渲染线程使用 6 KB 栈，对当前仪表板布局已足够。如果增加显著复杂的渲染逻辑，需注意栈溢出风险。

8. **Host 编译** — 在非 ESP 目标上（`cargo check`、`cargo clippy`），显示后端返回 `available: false`，所有命令为空操作。确保代码库在 host 端干净编译。

9. **旋转 (rotation)** — `rotation` 字段在控制器层面应用 MADCTL 变换。帧缓冲尺寸（`width` x `height`）应匹配旋转后的可视区域。
