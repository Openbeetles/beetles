# 硬件与板型说明

[English](../en-us/hardware.md) | **中文** | [文档索引](../README.md)

本页说明以下内容：

1. ESP32-S3 支持哪些板型
2. Linux 版程序现在能做什么
3. 出问题时先查哪里

## ESP32-S3 支持板型

| BOARD | Flash | PSRAM | 说明 |
|------|-------|-------|------|
| `esp32-s3-8mb` | 8MB | 8MB | N8R8 |
| `esp32-s3-16mb` | 16MB | 8MB | 默认板型 |
| `esp32-s3-32mb` | 32MB | 16MB | N32R16 |

- 仓库里的板型预设只支持 **带 PSRAM 的 ESP32-S3**

## Linux 支持情况

Linux 版本已经可以稳定运行主程序。

- 主程序已经能稳定跑
- 通道、记忆、工具、配置面和 API 都在这条线上
- 更适合做完整 Agent 程序、部署和集成

## 资源与程序行为

- 大块分配优先走 PSRAM
- orchestrator 会根据压力决定是否放行工作
- 内存紧张时，HTTP、LLM 和工具调用可能被限流
- 较长的 HTTP / LLM 请求需要和任务看门狗共存

## 构建配置

- `cargo build --release` 使用 `opt-level = 2`
- `cargo build --profile release-size` 是更偏体积的构建配置

## 状态检查入口

| 入口 | 能看到什么 |
|------|------------|
| `GET /api/health` | 整体健康状态 |
| `GET /api/resource` | 程序资源快照 |
| 串口日志 | 启动日志、heartbeat、警告信息 |
| `cli` feature | 例如 `heap_info` 这类额外串口命令 |

精确 HTTP 字段请看 [config-api.md](config-api.md)。

## 硬件设备配置

如果需要让 Agent 控制 LED、继电器、蜂鸣器、传感器或 PWM 设备，请继续阅读：

- [hardware-device-config.md](hardware-device-config.md)
- [tools.md](tools.md) 里的 `device_control`

## 常见问题

- `spiffs partition could not be found`
  基本就是没有用项目里的板型预设或分区表。
