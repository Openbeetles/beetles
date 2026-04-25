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

16MB S3 默认分区表 `partitions.csv` 保持 SPIFFS 起始地址在 `0xA20000`，当前迁移后的大小为 `0x5D0000`；旧的独立唤醒资源区已移除，并入 SPIFFS。`ota_0` 为 `0x540000`，`ota_1` 为 `0x4C0000`；这是非对称 OTA 布局，新固件若超过 `ota_1` 大小，应通过串口/工厂刷写到 `ota_0` 或先缩小固件体积。除非明确执行迁移或格式化决策，否则不要再次改变 SPIFFS extent，因为 ESP-IDF 可能在文件系统 extent 改变后格式化已有用户配置。

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
  基本就是板型或分区表没用对。
- 设备能启动但没有硬件能力
  先确认配置已经存在，而且当前机器真的接了对应硬件。
- 屏幕亮了但显示不对
  参见 [display.md](display.md)。

## 相关文档

- 硬件与传感器配置：[hardware-device-config.md](hardware-device-config.md)
- 显示配置：[display.md](display.md)
- 运行时工具能力：[tools.md](tools.md)
