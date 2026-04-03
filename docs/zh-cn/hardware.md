# 硬件与板型说明

[English](../en-us/hardware.md) | **中文** | [文档索引](../README.md)

这篇文档不讲抽象愿景，只回答三个最实际的问题：

1. 当前固件到底支持哪些板型？
2. 内存、构建配置和可观测入口该怎么看？
3. 出问题时先查哪里？

## 支持板型

| BOARD | Flash | PSRAM | 说明 |
|------|-------|-------|------|
| `esp32-s3-8mb` | 8MB | 8MB | N8R8 |
| `esp32-s3-16mb` | 16MB | 8MB | 默认板型 |
| `esp32-s3-32mb` | 32MB | 16MB | N32R16 |

当前规则很明确，也很简单：

- 仓库里的板型预设只支持 **带 PSRAM 的 ESP32-S3**

## 内存与运行时行为

- 大块分配优先走 PSRAM
- orchestrator 会根据压力决定是否放行工作
- 内存紧张时，HTTP、LLM 和工具调用可能被限流
- 较长的 HTTP / LLM 请求需要和任务看门狗共存

## 构建配置

- `cargo build --release` 使用 `opt-level = 2`
- `cargo build --profile release-size` 是更偏体积的构建配置

## 看状态要看哪里

| 入口 | 能看到什么 |
|------|------------|
| `GET /api/health` | 整体健康状态 |
| `GET /api/resource` | 运行时资源快照 |
| 串口日志 | 启动日志、heartbeat、警告信息 |
| `cli` feature | 例如 `heap_info` 这类额外串口命令 |

精确 HTTP 字段请看 [config-api.md](config-api.md)。

## 可配置硬件设备

如果你希望 Agent 去控制 LED、继电器、蜂鸣器、传感器或 PWM 设备，就继续看：

- [hardware-device-config.md](hardware-device-config.md)
- [tools.md](tools.md) 里的 `device_control`

## 常见问题

- `spiffs partition could not be found`
  基本就是没有用项目里的板型预设或分区表。

- `esp_task_wdt_reset: task not found`
  通常表示某个发 HTTP 的线程没有注册到任务看门狗。

- `getaddrinfo() returns 202`
  一般说明 DNS 解析失败，或者网络栈还没准备好。
