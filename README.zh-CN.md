# Beetle（甲壳虫）

**面向 ESP32-S3、ESP32-P4 与 Linux 的 Agent OS**<br/>
Rust · ReAct · 工具调用 · 记忆 · 硬件控制

[English](README.md) · **中文**

Beetle 是一套能跑在 **ESP32-S3**、**ESP32-P4** 和 **Linux** 上的 `Agent OS`，支持聊天通道接入、工具调用、记忆保存和硬件控制。

你可以用它来：

- 接收和发送聊天消息
- 调用时间、提醒、任务、文件等工具
- 保存会话摘要和长期记忆
- 通过网页完成配置，也可以直接调用接口
- 控制灯、继电器、蜂鸣器、传感器等设备

平台定位：

| 目标 | 更适合做什么 |
|------|------------|
| ESP32-S3 | 接硬件、控外设、做常驻设备助理 |
| ESP32-P4 + 板载 C6 | 更高配的 ESP 方案，适合更重的本地负载 |
| Linux | 适合运行更完整的 Agent OS 能力、长任务和复杂集成 |

本页用于快速上手。完整文档目录见 [docs/README.md](docs/README.md)。

## 仓库里有什么

| 部分 | 用途 |
|------|------|
| 核心系统 | ESP32-S3 和 Linux 共用的一套 Agent OS 核心 |
| `configure-ui` | 完整 Web 配置前端 |
| 配置接口 | 给自定义前端、脚本和其他程序调用 |
| 显示系统 | 可选 SPI 屏幕仪表板 |
| Linux 版 Agent OS | Linux 安装、更新和发布包相关内容 |

## 当前支持

当前支持的平台：

- **ESP32-S3**
- **ESP32-P4**
- **Linux**

现有板型预设：

- `esp32-s3-8mb`
- `esp32-s3-16mb`
- `esp32-s3-32mb`
- `esp32-p4-nano-16mb`

如果你主要做硬件接入，优先选 ESP。
如果你更看重长任务、集成和部署便利，优先选 Linux。

## 使用场景

- 在 ESP32-S3 或 Linux 上直接运行 Beetle Agent OS
- 在 ESP32-S3 上接入 LED、继电器、蜂鸣器、传感器等设备
- 在 Linux 上运行更完整的 Agent OS 形态
- 飞书、钉钉、企微、QQ 频道共用一套 Agent OS
- 通过可选编译选项打开 `telegram` 或 `websocket`
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
BOARD=esp32-p4-nano-16mb ./build.sh --flash
./build.sh build-c6
./build.sh flash-c6
./build.sh flash-all
ESPFLASH_PORT=/dev/cu.usbserial-xxx ./build.sh --flash
ESP_HOSTED_C6_PORT=/dev/cu.usbserial-c6 ESPFLASH_PORT=/dev/cu.usbserial-p4 ./build.sh flash-all
```

如果你没有设置 `BOARD` 或 `--target`，ESP 构建脚本会先在唯一检测到的串口上
执行 `espflash board-info`（如果设置了 `ESPFLASH_PORT`，就用它），并自动识别为
下面这些受支持的板型之一：

- `esp32-s3-8mb`
- `esp32-s3-16mb`
- `esp32-s3-32mb`
- `esp32-p4-nano-16mb`

如果识别不到受支持板型，或者当前有多个串口，还是需要你手动设置 `BOARD`。

Windows：

```powershell
.\build.ps1
.\build.ps1 --flash
$env:BOARD="esp32-s3-16mb"; .\build.ps1 --flash
$env:BOARD="esp32-p4-nano-16mb"; .\build.ps1 --flash
.\build.ps1 build-c6
.\build.ps1 flash-c6
.\build.ps1 flash-all
$env:ESPFLASH_PORT="COM3"; .\build.ps1 --flash
$env:ESP_HOSTED_C6_PORT="COM6"; $env:ESPFLASH_PORT="COM3"; .\build.ps1 flash-all
```

Windows 下规则相同：如果 `BOARD` 和 `--target` 都没给，脚本会对唯一检测到的
COM 口执行 `espflash board-info`，或者直接使用你显式设置的 `ESPFLASH_PORT`。

如果你用的是 `ESP32-P4-NANO`，必须把双芯片都烧好才算完整：

1. `flash-c6` 烧板载 `ESP32-C6` 的 hosted slave firmware
2. `BOARD=esp32-p4-nano-16mb ./build.sh --flash` 烧 `ESP32-P4` 上的 Beetle 主固件
3. `flash-all` 按正确顺序一次性全烧

烧板载 `ESP32-C6` 前，先把 `ESP32-P4` 置于 bootloader 模式，避免共享板级连线互相干扰。
`build-c6` / `flash-c6` 会自动查找 `espup` 导出的环境，以及官方 ESP-IDF 的标准安装路径（`IDF_PATH`、`~/.espressif/...`、`~/esp/...`），再调用 `idf.py`。

### 3. 打开配置页

首次上电后，设备会开启一个名为 **Beetle** 的热点。

1. 手机或电脑连接这个热点
2. 浏览器打开 **[http://192.168.4.1](http://192.168.4.1)**
3. 设置配对码
4. 配置 WiFi、大模型和你要使用的聊天通道

设备连上路由器后，后续直接用它的局域网 IP 打开配置页即可。

## 构建说明

默认启用的 Cargo 编译选项：

- `config_api`
- `feishu`
- `tools_diagnostics`
- `tools_network_extra`
- `thread_panic_catch`

可选编译选项：

- `telegram`
- `websocket`
- `cli`
- `ota`

示例：

```bash
cargo build --release --features telegram,ota
```

板型选择由 `BOARD` 控制。构建脚本会读取 `board_presets.toml`，自动选对 target、分区表和 Flash 大小。
如果是 ESP 构建且没有提供 `BOARD` / `--target`，`build.sh` / `build.ps1` 会先尝试
用 `espflash board-info` 自动识别当前连接板型；识别结果不明确或不受支持时，再回到手动 `BOARD` 选择。

## 支持板型


| BOARD           | Flash | PSRAM | 说明     |
| --------------- | ----- | ----- | ------ |
| `esp32-s3-8mb`  | 8MB   | 8MB   | N8R8   |
| `esp32-s3-16mb` | 16MB  | 8MB   | 默认板型   |
| `esp32-s3-32mb` | 32MB  | 16MB  | N32R16 |
| `esp32-p4-nano-16mb` | 16MB | 32MB | Beetle 跑在 P4；板载 C6 提供 hosted WiFi |


这里最容易出错的地方有两个：

- 必须使用项目自带的分区表
- `esp32-p4-nano-16mb` 是双芯片板，C6 和 P4 都要烧
- 如果报 `spiffs partition could not be found`，通常是板型预设或分区表没用对

## 主要能力


| 维度   | Beetle 提供什么                     |
| ---- | ------------------------------- |
| 聊天通道 | 多个聊天通道共用一套 Agent OS                |
| 记忆   | 会话摘要、长期记忆、档案证据检索                |
| 工具   | 时间、提醒、任务/日历、文件操作、板子信息、联网工具、硬件控制 |
| 硬件   | 基于配置生成 `device_control`，统一控制外设  |
| 配置   | 内置浏览器流程 + 完整配置接口                |
| 显示   | SPI TFT 仪表板                     |
| 健康状态 | 诊断、重启与恢复操作              |


## 接下来该看哪篇


| 你的目标           | 该看哪篇文档                                                                       |
| -------------- | ---------------------------------------------------------------------------- |
| 把板子烧起来并完成配置    | [docs/zh-cn/configuration.md](docs/zh-cn/configuration.md)                   |
| 在 Linux 上安装或打包 | [docs/zh-cn/linux-release-rollback.md](docs/zh-cn/linux-release-rollback.md) |
| 自己调用 HTTP 接口   | [docs/zh-cn/config-api.md](docs/zh-cn/config-api.md)                         |
| 了解 Agent OS 能调用哪些工具 | [docs/zh-cn/tools.md](docs/zh-cn/tools.md)                                   |
| 配置大模型服务商       | [docs/zh-cn/llm-providers.md](docs/zh-cn/llm-providers.md)                   |
| 配置 SPI 屏幕      | [docs/zh-cn/display.md](docs/zh-cn/display.md)                               |
| 配置硬件设备         | [docs/zh-cn/hardware-device-config.md](docs/zh-cn/hardware-device-config.md) |
| 看支持板型和常见硬件问题 | [docs/zh-cn/hardware.md](docs/zh-cn/hardware.md)                             |
| 看完整文档地图        | [docs/README.md](docs/README.md)                                             |


## 常见问题

- 烧录失败：先检查 USB 线、串口，以及 `ESPFLASH_PORT`
- `flash-c6` 失败：先检查 `PROG_C6` 串口接线，设置 `ESP_HOSTED_C6_PORT`，并先让 P4 进入 bootloader 模式
- 设备打不开：先重新连接热点 `Beetle`，再打开 `http://192.168.4.1`
- `spiffs partition could not be found`：基本就是板型预设或分区表没用对
- 需要精确接口行为：看 [docs/zh-cn/config-api.md](docs/zh-cn/config-api.md)
- 需要板型和资源信息：看 [docs/zh-cn/hardware.md](docs/zh-cn/hardware.md)
- 需要看 Linux 安装与打包说明：看 [docs/zh-cn/linux-release-rollback.md](docs/zh-cn/linux-release-rollback.md)

## 许可

Beetle 使用 **MIT OR Apache-2.0** 双许可证，详见 [LICENSE](LICENSE)。
