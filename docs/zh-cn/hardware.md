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

## 资源与程序行为

- 大块分配优先走 PSRAM
- 程序会根据当前资源压力决定是否继续放行请求
- 内存紧张时，接口请求、大模型请求和工具调用可能被限流
- 较长的接口请求和大模型请求需要和任务看门狗共存
- 惰性拉起的 ESP 配置面路由 worker（`http_route_exec`）必须继续留在标准 Rust 线程执行面，不能再塞回 raw native task 路径。
- 任务看门狗的注册和状态检查必须使用显式当前 task handle，不能再把 `NULL` status probe 当成“已经订阅”的捷径。

## 构建配置

- `cargo build --release` 使用 `opt-level = 2`
- `cargo build --profile release-size` 是更偏体积的构建配置

## 状态检查入口

| 入口 | 能看到什么 |
|------|------------|
| `GET /api/health` | 整体健康状态 |
| `GET /api/resource` | 资源占用情况 |
| 串口日志 | 启动日志、heartbeat、警告信息 |
| `cli` 编译选项 | 例如 `heap_info` 这类额外串口命令 |

接口字段说明请看 [config-api.md](config-api.md)。

## 硬件设备配置

如果需要让 Beetle 控制 LED、继电器、蜂鸣器、传感器或 PWM 设备，请继续阅读：

- [hardware-device-config.md](hardware-device-config.md)
- [tools.md](tools.md) 里的 `device_control`

## 常见问题

- `spiffs partition could not be found`
  基本就是没有用项目里的板型预设或分区表。
