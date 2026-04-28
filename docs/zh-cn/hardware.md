# 硬件与板型

[English](../en-us/hardware.md) | **中文** | [文档索引](../README.md)

本页用于确定 Beetle OS 的部署平台与硬件范围，不涉及具体配置字段。

## 平台选择建议

- 主要任务是控外设、读传感器、做常驻边缘设备：选 **ESP32-S3**
- 想走更高配的 ESP 路线：看 **ESP32-P4**
- 想接更多外部系统、跑更久的任务、做主机侧扩展：选 **Linux**

## 当前板型预设

| BOARD | Flash | PSRAM | 说明 |
|------|-------|-------|------|
| `esp32-s3-8mb` | 8MB | 8MB | ESP32-S3 |
| `esp32-s3-16mb` | 16MB | 8MB | ESP32-S3 |
| `esp32-s3-32mb` | 32MB | 16MB | ESP32-S3 |
| `esp32-p4-nano-16mb` | 16MB | 32MB | ESP32-P4 NANO |

`esp32-s3-16mb` 目前仍是最常见的 ESP32-S3 主线选择。若你要自定义固件布局或存储划分，这已经属于进阶固件工作，处理不当可能会清掉已有配置。

当前 Beetle 主线功能较多，系统包体较大，无法在同时保留现有功能与体验的前提下继续提供官方 OTA 升级能力。如果你需要 OTA，可以自行裁剪功能、重新规划分区表，或联系我们做定制方案。

主线现行换固件方式是浏览器 USB 烧录、串口烧录或工厂重刷。

## 当前硬件能力范围

- GPIO 设备
- PWM 设备
- 模拟量输入
- DHT 传感器
- I2C 设备和 I2C 传感器
- SPI 屏幕

如已明确设备类型和接线范围，继续阅读 [hardware-device-config.md](hardware-device-config.md)。

## Linux 硬件发现

Linux 还提供了硬件发现能力。
现在最常见的是 USB 方向，比如：

- 音频输入
- 音频输出
- 摄像头
- 串口
- HID

## 文档分工

- 平台或板型选型：本页
- 设备与接线配置：[hardware-device-config.md](hardware-device-config.md)
- 显示配置：[display.md](display.md)

## 常见问题

- `spiffs partition could not be found`
  基本就是板型或固件布局没用对。
- 设备能启动但没有硬件能力
  先确认配置已经存在，而且当前机器真的接了对应硬件。
- 屏幕亮了但显示不对
  参见 [display.md](display.md)。

## 相关文档

- 硬件与传感器配置：[hardware-device-config.md](hardware-device-config.md)
- 显示配置：[display.md](display.md)
- 运行时工具能力：[tools.md](tools.md)
