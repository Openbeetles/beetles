# 硬件与板型

[English](../en-us/hardware.md) | **中文** | [文档索引](../README.md)

这页只讲和选设备最相关的事实。

## 当前板型预设

仓库当前提供这些板型预设：

| BOARD | Flash | PSRAM | 说明 |
|------|-------|-------|------|
| `esp32-s3-8mb` | 8MB | 8MB | ESP32-S3 |
| `esp32-s3-16mb` | 16MB | 8MB | ESP32-S3 |
| `esp32-s3-32mb` | 32MB | 16MB | ESP32-S3 |
| `esp32-p4-nano-16mb` | 16MB | 32MB | ESP32-P4 NANO |

## 怎么选

- 主要是控外设、读传感器：选 ESP32-S3
- 想用更高配的 ESP 板：看 ESP32-P4
- 想接更多外部能力、跑更长的任务：选 Linux

## 你在硬件上最常会做的几件事

- 接 GPIO 设备
- 接 PWM 设备
- 接模拟量输入
- 接 DHT
- 接 I2C 设备或 I2C 传感器
- 接 SPI 屏幕

对应文档：

- 硬件控制配置： [hardware-device-config.md](hardware-device-config.md)
- 屏幕配置： [display.md](display.md)

## Linux 上额外有的方向

Linux 这边除了常规运行外，还提供了硬件发现入口。
当前公开的是 USB 方向的发现能力，常见分类包括：

- 音频输入
- 音频输出
- 摄像头
- 串口
- HID

## 遇到问题先看哪里

- 配置页：先确认设备有没有被识别、配置有没有保存
- [config-api.md](config-api.md)：需要自己查接口时再看
- 串口或服务日志：看启动失败、配置失败、硬件初始化失败

## 常见问题

- `spiffs partition could not be found`
  基本就是板型或分区表没用对
- 设备能启动但没有硬件能力
  先确认配置已经写对，而且当前设备真的接了对应硬件
- 屏幕亮了但显示不对
  直接看 [display.md](display.md)
