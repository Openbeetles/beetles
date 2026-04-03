<p align="center">
  <img src="configure-ui/public/logo.png" alt="甲壳虫" width="132" height="132" />
</p>

<h1 align="center">Beetle（甲壳虫）</h1>

<p align="center">
  <strong>面向 ESP32-S3 的边缘 AI Agent 固件</strong><br/>
  Rust · ReAct · 工具调用 · 记忆 · 硬件控制
</p>

<p align="center">
  <a href="README.md">English</a> · <strong>中文</strong>
</p>

<p align="center">
  <a href="docs/README.md"><img alt="文档" src="https://img.shields.io/badge/%E6%96%87%E6%A1%A3-index-1f6feb" /></a>
  <a href="#快速开始"><img alt="快速开始" src="https://img.shields.io/badge/%E5%BF%AB%E9%80%9F%E5%90%AF%E5%8A%A8-5%E5%88%86%E9%92%9F-2ea043" /></a>
  <a href="#支持板型"><img alt="板型" src="https://img.shields.io/badge/%E6%9D%BF%E5%9E%8B-ESP32--S3-orange" /></a>
  <a href="LICENSE"><img alt="License" src="https://img.shields.io/badge/License-MIT%20OR%20Apache--2.0-blue.svg" /></a>
</p>

Beetle 是一套跑在 ESP32-S3 上的 AI Agent 固件运行时。它想做的事情其实不复杂：

- 从聊天通道接收消息
- 在设备上执行工具和记忆逻辑
- 控制真实硬件
- 提供浏览器配置页面和 HTTP 配置 API

如果你现在的目标是“先烧一块板子，然后尽快把配置页跑起来”，看这份 README 就够了。
如果你要自己接前端、脚本或者第三方流程，再去看 [docs/README.md](docs/README.md) 会更省事。

## 仓库里有什么

| 部分 | 用途 |
|------|------|
| 固件运行时 | ESP32-S3 主固件 |
| `configure-ui` | 完整 Web 配置前端 |
| HTTP 配置 API | 给自定义前端、脚本和集成系统使用 |
| 显示系统 | 可选的 SPI 屏幕仪表板 |
| Host 构建路径 | 开发调试、文档工具、Linux 打包 |

## 当前范围

这个仓库当前稳定支持的是 **带 PSRAM 的 ESP32-S3 固件路径**。也就是说，文档里的“快速开始”和“烧录流程”都是围绕这条主线写的。

现有板型预设：

- `esp32-s3-8mb`
- `esp32-s3-16mb`
- `esp32-s3-32mb`

仓库里也保留了非 ESP 的构建路径，主要给开发、集成和 Linux 发布物处理使用；但这份 README 还是以“怎么把 ESP32-S3 固件真正跑起来”为主。

## 它能拿来做什么

- 一块板子直接跑聊天型 AI Agent
- 飞书、钉钉、企微、QQ 频道共用一个运行时
- 通过可选 feature 打开 `telegram` 或 `websocket`
- 在设备上保存摘要、长期记忆、提醒、任务和档案证据
- 通过 `device_control` 工具控制 GPIO、PWM、ADC、蜂鸣器等硬件
- 在 SPI 屏幕上显示运行状态

## 快速开始

### 1. 准备工具链

- 安装 [esp-rs 工具链](https://docs.espressif.com/projects/rust-book/en/latest/introduction.html)，执行 `espup install`
- 安装 `espflash`，执行 `cargo install espflash`
- Windows 需要安装 Visual Studio，并勾选 Desktop development for C++

### 2. 编译或烧录

macOS / Linux：

```bash
./build.sh
./build.sh --flash
BOARD=esp32-s3-16mb ./build.sh --flash
ESPFLASH_PORT=/dev/cu.usbserial-xxx ./build.sh --flash
```

Windows：

```powershell
.\build.ps1
.\build.ps1 --flash
$env:BOARD="esp32-s3-16mb"; .\build.ps1 --flash
$env:ESPFLASH_PORT="COM3"; .\build.ps1 --flash
```

### 3. 打开配置页

首次上电后，设备会开启一个名为 **Beetle** 的热点。你可以把它理解成“设备给自己开了一个临时入口”。

1. 手机或电脑连接这个热点
2. 浏览器打开 **http://192.168.4.1**
3. 设置配对码
4. 配置 WiFi、LLM 和你要使用的聊天通道

设备连上家里路由器之后，就不用再连热点了，改用它的局域网 IP 继续访问配置页即可。

## 构建说明

默认启用的 Cargo feature：

- `config_api`
- `feishu`
- `tools_diagnostics`
- `tools_network_extra`
- `thread_panic_catch`

可选 feature：

- `telegram`
- `websocket`
- `cli`
- `ota`

示例：

```bash
cargo build --release --features telegram,ota
```

板型选择由 `BOARD` 控制。构建脚本会读取 `board_presets.toml`，自动选对 target、分区表和 Flash 大小。

## 支持板型

| BOARD | Flash | PSRAM | 说明 |
|------|-------|-------|------|
| `esp32-s3-8mb` | 8MB | 8MB | N8R8 |
| `esp32-s3-16mb` | 16MB | 8MB | 默认板型 |
| `esp32-s3-32mb` | 32MB | 16MB | N32R16 |

这里最容易出错的地方有两个：

- 必须使用项目自带的分区表
- 如果报 `spiffs partition could not be found`，通常是板型预设或分区表没用对

## 主要能力

| 维度 | Beetle 提供什么 |
|------|-----------------|
| 聊天通道 | 多个聊天通道共用一个运行时 |
| 记忆 | 会话摘要、长期记忆、档案证据检索 |
| 工具 | 时间、提醒、任务/日历、文件操作、板子信息、联网工具、硬件控制 |
| 硬件 | 基于配置生成 `device_control`，统一控制外设 |
| 配置 | 内置浏览器流程 + 完整 HTTP 配置 API |
| 显示 | SPI TFT 仪表板 |
| 健康状态 | 指标、资源快照、诊断、重启与恢复操作 |

## 接下来该看哪篇

| 你的目标 | 该看哪篇文档 |
|----------|--------------|
| 把板子烧起来并完成配置 | [docs/zh-cn/configuration.md](docs/zh-cn/configuration.md) |
| 自己调用 HTTP API | [docs/zh-cn/config-api.md](docs/zh-cn/config-api.md) |
| 了解 Agent 能调用哪些工具 | [docs/zh-cn/tools.md](docs/zh-cn/tools.md) |
| 配置 LLM 提供商 | [docs/zh-cn/llm-providers.md](docs/zh-cn/llm-providers.md) |
| 配置 SPI 屏幕 | [docs/zh-cn/display.md](docs/zh-cn/display.md) |
| 配置硬件设备 | [docs/zh-cn/hardware-device-config.md](docs/zh-cn/hardware-device-config.md) |
| 看板型限制和排错 | [docs/zh-cn/hardware.md](docs/zh-cn/hardware.md) |
| 看完整文档地图 | [docs/README.md](docs/README.md) |

## 常见问题

- 烧录失败：先检查 USB 线、串口，以及 `ESPFLASH_PORT`
- 设备打不开：先重新连接热点 `Beetle`，再打开 `http://192.168.4.1`
- `spiffs partition could not be found`：基本就是板型预设或分区表没用对
- 需要精确接口行为：看 [docs/zh-cn/config-api.md](docs/zh-cn/config-api.md)
- 需要板型和资源信息：看 [docs/zh-cn/hardware.md](docs/zh-cn/hardware.md)

## 许可

Beetle 使用 **MIT OR Apache-2.0** 双许可证，详见 [LICENSE](LICENSE)。
