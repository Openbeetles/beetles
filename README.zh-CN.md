# Beetls OS

**面向 ESP32 与 Linux 的聊天式设备 Agent**<br/>
Rust · 聊天通道 · 工作流 · 硬件控制

[English](README.md) · **中文**

Beetls OS 是面向 ESP32 与 Linux 的设备 Agent 运行时，集成浏览器配置、聊天交互、大模型接入、办公账号能力和硬件控制。
仓库里的命令、服务名和文件路径仍使用 `beetle`。

## Beetls OS 能做什么

- 通过飞书、钉钉、企微、QQ 频道等聊天通道回复和协作
- 处理提醒、任务和轻量日常流程
- 接好办公账号后处理邮箱、日历、联系人和文档
- 在支持的硬件上读取传感器、控制设备
- 提供浏览器配置流程，以及状态和配置接口

## 典型应用场景

| 形态 | Beetls OS 在里面承担什么 |
|------|-----------------------|
| 桌面助手 | 聊天、提醒、文档、轻量流程协助 |
| 前台终端 | 接待答疑、屏幕展示、访客分流、运营协助 |
| 提醒终端 | 日程、语音、屏幕、通知 |
| 监测节点 | 传感器监控、告警、渠道通知 |
| 设备控制器 | GPIO、PWM、I2C 和简单控制流程 |

## 文档入口

| 主题 | 文档 |
|------|------|
| ESP32 首次部署与接入 | [docs/zh-cn/getting-started-esp.md](docs/zh-cn/getting-started-esp.md) |
| Linux 首次部署与接入 | [docs/zh-cn/getting-started-linux.md](docs/zh-cn/getting-started-linux.md) |
| 能力概览与适用场景 | [docs/zh-cn/capabilities.md](docs/zh-cn/capabilities.md) |
| 终端构建、烧录与部署 | [docs/zh-cn/build-script.md](docs/zh-cn/build-script.md) |
| 浏览器配置流程 | [docs/zh-cn/configuration.md](docs/zh-cn/configuration.md) |
| Linux 部署与运维 | [docs/zh-cn/linux-release-rollback.md](docs/zh-cn/linux-release-rollback.md) |
| 配置与集成接口 | [docs/zh-cn/config-api.md](docs/zh-cn/config-api.md) |
| 文档总索引 | [docs/README.md](docs/README.md) |

## 支持板型

| BOARD | Flash | PSRAM | 说明 |
|------|-------|-------|------|
| `esp32-s3-8mb` | 8MB | 8MB | N8R8 |
| `esp32-s3-16mb` | 16MB | 8MB | 常见默认板型 |
| `esp32-s3-32mb` | 32MB | 16MB | N32R16 |
| `esp32-p4-nano-16mb` | 16MB | 32MB | 双芯片板 |

## 文档结构

- `Start`：ESP32 和 Linux 首次上手
- `Capabilities`：真实产品形态和能力范围
- `Configure`：模型、通道、硬件、屏幕和配置流程
- `Operate`：构建、烧录、部署、重启、停止、回滚
- `Reference`：API 细节和工具面
- `Develop`：架构和扩展点

## 许可

Beetls OS 使用 **MIT OR Apache-2.0** 双许可证，详见 [LICENSE-MIT](LICENSE-MIT) 和 [LICENSE-APACHE](LICENSE-APACHE)。
