# Beetle（甲壳虫）

**面向 ESP32 与 Linux 的聊天式设备 Agent**<br/>
Rust · 聊天通道 · 日常事务 · 硬件控制

[English](README.md) · **中文**

Beetle 是一个可以通过聊天使用、通过网页管理的设备 Agent。
它能回复消息、处理提醒和任务、接入工作账号，也能在支持的设备上控制硬件。

有些能力和你使用的设备、硬件以及当前配置有关。

## Beetle 能做什么

- 通过聊天通道和你对话
- 处理提醒、任务和一些日常事务
- 在接好账号后处理邮箱、日历、文档和联系人
- 在支持的板子上读取传感器、控制设备
- 提供网页配置入口和状态接口

常见聊天通道包括飞书、钉钉、企微和 QQ 频道。

## 适合跑在哪里

| 目标 | 更适合做什么 |
|------|--------------|
| ESP32-S3 | 控设备、读传感器、做常驻边缘设备 |
| ESP32-P4 | 跑更重一些的板端任务 |
| Linux | 跑更久的任务、接工作账号、做更大扩展 |

## 快速开始

### 1. 烧录或部署 Beetle

大多数 ESP 用户可以先这样开始：

```bash
./build.sh --flash
```

常见板型示例：

```bash
BOARD=esp32-s3-16mb ./build.sh --flash
BOARD=esp32-p4-nano-16mb ./build.sh --flash
./build.sh flash-all
```

如果你要部署到 Linux，直接看 [docs/zh-cn/linux-release-rollback.md](docs/zh-cn/linux-release-rollback.md)。

### 2. 打开配置页

第一次使用时，Beetle 通常会提供一个名为 **Beetle** 的热点。
连上后在浏览器打开 **http://192.168.4.1**。

如果设备已经接入局域网，就直接打开它的局域网地址。

### 3. 先完成最小配置

优先配好这四项：

1. 配对码
2. 网络
3. 一个大模型来源
4. 一个聊天通道

之后再按需要补工作账号、硬件、屏幕或音频。

## 支持板型

| BOARD | Flash | PSRAM | 说明 |
|------|-------|-------|------|
| `esp32-s3-8mb` | 8MB | 8MB | N8R8 |
| `esp32-s3-16mb` | 16MB | 8MB | 常见默认板型 |
| `esp32-s3-32mb` | 32MB | 16MB | N32R16 |
| `esp32-p4-nano-16mb` | 16MB | 32MB | 双芯片板 |

## 接下来读什么

| 你想做什么 | 看这里 |
|------------|--------|
| 第一次把 Beetle 用起来 | [docs/zh-cn/configuration.md](docs/zh-cn/configuration.md) |
| 看看 Beetle 能帮你做什么 | [docs/zh-cn/tools.md](docs/zh-cn/tools.md) |
| 配置大模型服务 | [docs/zh-cn/llm-providers.md](docs/zh-cn/llm-providers.md) |
| 通过终端构建、烧录或部署 | [docs/zh-cn/build-script.md](docs/zh-cn/build-script.md) |
| 配硬件或屏幕 | [docs/zh-cn/hardware.md](docs/zh-cn/hardware.md)、[docs/zh-cn/hardware-device-config.md](docs/zh-cn/hardware-device-config.md)、[docs/zh-cn/display.md](docs/zh-cn/display.md) |
| 自己写前端或脚本 | [docs/zh-cn/config-api.md](docs/zh-cn/config-api.md) |
| 看完整文档目录 | [docs/README.md](docs/README.md) |

## 许可

Beetle 使用 **MIT OR Apache-2.0** 双许可证，详见 [LICENSE-MIT](LICENSE-MIT) 和 [LICENSE-APACHE](LICENSE-APACHE)。
