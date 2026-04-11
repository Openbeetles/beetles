# 硬件与板型说明

[English](../en-us/hardware.md) | **中文** | [文档索引](../README.md)

这页主要说明三件事：

1. ESP32-S3 支持哪些板型
2. Linux 版现在能做什么
3. 出问题时先查哪里

## ESP32-S3 支持板型

| BOARD | Flash | PSRAM | 说明 |
|------|-------|-------|------|
| `esp32-s3-8mb` | 8MB | 8MB | N8R8 |
| `esp32-s3-16mb` | 16MB | 8MB | 默认板型 |
| `esp32-s3-32mb` | 32MB | 16MB | N32R16 |

- 仓库里的板型预设只支持 **带 PSRAM 的 ESP32-S3**

## Linux 支持情况

Linux 版已经可以稳定运行。

- 聊天、工具、记忆、配置页面和接口都可以正常使用
- 更适合承载更完整的 Agent OS 能力、安装和扩展

## 选择建议

- 主要接外设、做本地硬件联动：优先选 ESP32-S3
- 想跑更完整的 Beetle、做更复杂的集成：优先选 Linux
- 需要更高配的 ESP 方案：看 ESP32-P4

## 状态检查入口

| 入口 | 能看到什么 |
|------|------------|
| 配置页 | 最适合普通用户先看 |
| `GET /api/health` | 设备整体状态 |
| 串口日志 | 启动日志、heartbeat、警告信息 |

如果你需要精确字段说明，再去看 [config-api.md](config-api.md)。

## 硬件设备配置

如果需要让 Beetle 控制 LED、继电器、蜂鸣器、传感器或 PWM 设备，请继续阅读：

- [hardware-device-config.md](hardware-device-config.md)
- [tools.md](tools.md) 里的 `device_control`

## 常见问题

- `spiffs partition could not be found`
  基本就是没有用项目里的板型预设或分区表。
- Linux 能联网但 Beetle 看起来不在线
  先确认你访问的是设备当前的局域网地址。
