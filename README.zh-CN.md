<div align="center">
  <a href="https://github.com/Openbeetles/beetles">
    <img src="docs/assets/beetles-os-lockup.png" alt="Beetles OS" width="520" style="max-width: 100%; height: auto;" />
  </a>

  <h3><strong>智能硬件的设备系统：对话入口、模型接入、工具执行、硬件控制与状态诊断</strong></h3>

  <p>
    <a href="https://github.com/Openbeetles/beetles"><img alt="Openbeetles" src="https://img.shields.io/badge/Openbeetles-beetles-00a99d" /></a>
    <a href="LICENSE-MIT"><img alt="License" src="https://img.shields.io/badge/License-MIT%20OR%20Apache--2.0-blue.svg" /></a>
  </p>

  <p>
    <a href="https://github.com/Openbeetles/beetles">主页</a> |
    <a href="docs/README.md">文档</a> |
    <a href="#quick-start">快速开始</a> |
    <a href="#build-from-source">从源码构建</a> |
    <a href="README.md">English</a>
  </p>
</div>

<p align="center">
  <img src="docs/assets/readme-hardware-matrix.svg" alt="Beetles OS hardware matrix" width="1000" style="max-width: 100%; height: auto;" />
</p>

**Beetles OS** 是面向 ESP32 与 Linux 边缘设备的智能设备系统。它把消息入口、模型服务、工具系统、硬件能力、记忆与运维诊断放在同一个设备系统里，使设备能够从自然语言请求进入明确的执行流程，而不是停留在聊天转发。

对使用者来说，它提供的是一个可配置、可追踪的设备智能层：用户可以通过 Telegram、飞书、钉钉、企业微信或 QQ 频道向设备提出目标；设备结合配置、记忆和现场状态，选择被允许的工具，执行硬件或系统动作，并把结果返回到聊天通道或本地界面。

当前主线支持 ESP32-S3、ESP32-P4-NANO 和 Linux。

## 定位

大模型让设备交互从固定指令转向目标表达，但真实设备不能把执行权交给模型本身。凭据、文件、引脚、网络连接、系统重启和硬件动作都必须由设备系统统一管理。Beetles OS 解决的是这些能力落到真实设备上以后的基本问题：如何接入、如何授权、如何执行、如何记录，失败时如何恢复。

| 目标 | Beetles OS 的处理方式 |
|------|------------------------|
| 自然语言进入设备现场 | 支持多种消息入口，把用户目标转入设备侧任务流程 |
| 模型参与判断但不越界 | 只向模型提供被允许的工具、上下文和约束，执行由 Beetles OS 完成 |
| 硬件能力可配置 | 将外设注册为受控能力，按配置和平台能力决定是否开放 |
| 状态和问题看得见 | 展示网络、资源压力、模型调用、工具调用、日志和恢复入口 |
| 换平台不重做整套系统 | 同一套设备系统覆盖 ESP32 与 Linux，按设备资源选择构建形态 |

Beetles OS 不替代 PLC、工控机或原有业务系统。它更适合部署在现有系统旁边，作为对话入口、工具调度层、设备控制层和运维可见性层。

## 工作流程

一次请求进入 Beetles OS 后，会经过固定的处理流程：

| 阶段 | 说明 |
|------|------|
| 入口识别 | 判断消息来自哪个通道、会话和用户，确定是否允许进入设备流程 |
| 上下文准备 | 合并当前配置、记忆、设备状态、网络状态和资源压力 |
| 确定可用能力 | 根据平台、功能开关、硬件配置和安全边界生成当次可用工具 |
| 模型决策 | 让模型理解目标、组织步骤、选择工具或生成回复 |
| 执行动作 | 配置保存、硬件控制、状态诊断、消息发送和系统动作由 Beetles OS 执行 |
| 结果回写 | 将回复、调用结果、失败原因和关键状态返回给通道或本地界面 |

这个分工是 Beetles OS 的核心：模型负责理解目标和组织行动，系统负责边界、执行和恢复。

## 核心能力

| 能力 | 说明 |
|------|------|
| 消息入口 | 支持 Telegram、飞书、钉钉、企业微信、QQ 频道等入口；语音和实时网页连接可作为附加交互面 |
| 模型服务 | 支持 OpenAI、Anthropic、Gemini、GLM、通义千问、DeepSeek、Moonshot、Ollama，以及兼容 OpenAI 接口的模型服务 |
| 工具系统 | 将提醒、任务、网络查询、状态诊断、硬件控制、办公集成等能力整理为可查看、可追踪的工具 |
| 记忆与连续性 | 分层保存会话上下文、长期事实、任务进度和证据记录，帮助设备在长任务和重启后恢复上下文 |
| 硬件控制 | 通过配置将 LED、继电器、蜂鸣器、传感器等外设注册为受控能力 |
| 本地配置 | 首次启动后通过热点或局域网访问配置页，维护网络、模型、通道、硬件和系统参数 |
| 运维诊断 | 提供健康状态、网络路径、资源压力、通道连通性、存储状态、日志和恢复入口 |
| 多平台支持 | ESP32 适合硬件设备与轻量控制，Linux 适合长任务、本地工具、办公集成和服务化部署 |

## 适用场景

| 场景 | 典型用途 |
|------|----------|
| 前台或值班终端 | 访客问答、消息转发、状态播报、值班提醒 |
| 设备控制器 | 通过对话触发灯光、电机、蜂鸣器、继电器、传感器等受控动作 |
| 工业现场辅助 | 告警解释、巡检记录、远程维保、SOP 提醒、环境监测 |
| Linux 边缘节点 | 常驻服务、办公集成、长任务、部署回滚和本地工具调用 |
| 自定义智能硬件 | 复用消息入口、模型接入、记忆、工具、配置页和诊断能力 |

## 消息入口

<p align="center">
  <img src="docs/assets/readme-channel-support.svg" alt="Beetles OS communication channel support" width="920" style="max-width: 100%; height: auto;" />
</p>

企业内部可以选择飞书、钉钉、企业微信或 QQ 频道；个人和海外场景可以选择 Telegram；语音与实时网页连接适合语音设备或自定义前端。设备最终可选哪些入口，取决于当前固件或 Linux 包实际包含的通道能力。

## 记忆与连续性

记忆不是聊天记录的堆积，而是设备理解现场、延续任务和解释行为的依据。Beetles OS 将不同性质的信息分层保存，避免临时对话覆盖长期事实，也避免把过期状态当作当前结论。

| 信息类型 | 用途 |
|----------|------|
| 会话上下文 | 保留当前对话目标、未完成事项和最近执行结果 |
| 长期事实 | 保存稳定的设备身份、项目背景、偏好和约束 |
| 任务进度 | 记录任务执行到哪里、中断原因和下一步入口 |
| 可复核证据 | 保存来源、时间和观察结果，便于解释与审计 |
| 诊断记录 | 保留系统异常、调用失败和恢复过程，便于排障 |

这样设计的目的不是让设备“更会聊天”，而是让设备在长期使用中知道哪些信息可以使用、哪些信息需要复核、哪些信息只适合作为历史参考。

## 模型服务

Beetles OS 不绑定单一模型厂商。你可以在配置页维护多个模型来源，并设置调用顺序；当前一个来源不可用时，系统可以按配置尝试后续来源。

<p align="center">
  <img src="docs/assets/readme-llm-support.svg" alt="Beetles OS large language model support" width="920" style="max-width: 100%; height: auto;" />
</p>

当前可配置的模型来源包括 OpenAI、Anthropic、Gemini、GLM、通义千问、DeepSeek、Moonshot、Ollama，以及兼容 OpenAI 接口的模型服务。密钥、服务地址、模型名、自定义请求头和回退顺序都可以在本地配置页维护，不需要把某一家服务写死在固件里。

<p align="center">
  <img src="docs/assets/readme-original-llm-config.png" alt="Beetles OS LLM source configuration" width="1100" style="max-width: 100%; height: auto;" />
</p>

第一次接入时，建议先用一个模型来源、一组密钥和一个模型名跑通完整链路，再增加其他来源并调整顺序。详细字段见 [模型服务配置](docs/zh-cn/llm-providers.md)。

## 工具与运维

工具是模型进入真实设备的执行边界。Beetles OS 向模型暴露的是经过注册的工具说明、参数要求和使用约束；实际的配置保存、硬件控制、状态诊断和消息发送由 Beetles OS 完成。

<p align="center">
  <img src="docs/assets/readme-original-tools-registry.png" alt="Beetles OS registered tools" width="1100" style="max-width: 100%; height: auto;" />
</p>

商用设备不能只关注一次回复是否成功，还需要在长期使用中定位问题。配置页会展示健康状态、资源压力、模型调用、工具调用和日志；聊天通道也可以触发状态查询，让远程运维人员先获得平台、网络、资源压力和历史负载，再决定下一步动作。

<p align="center">
  <img src="docs/assets/readme-original-health-logs.png" alt="Beetles OS health metrics and logs" width="1100" style="max-width: 100%; height: auto;" />
</p>

<p align="center">
  <img src="docs/assets/readme-original-chat-status.png" alt="Beetles OS chat status response" width="900" style="max-width: 100%; height: auto;" />
</p>

## 平台与板型

Beetles OS 支持在不同硬件形态上部署同一套设备侧系统。

| 平台 | 状态 | 适合做什么 |
|------|------|------------|
| ESP32-S3 | 已支持 | 小型硬件设备、传感器节点、简单显示和外设控制 |
| ESP32-P4-NANO | 已支持 | P4 主控固件、更大内存余量的 ESP 设备验证 |
| Linux | 已支持 | 长任务、本地工具、办公集成、发布包部署和运维回滚 |

当前板型预设：

| BOARD | Flash | PSRAM | 说明 |
|-------|-------|-------|------|
| `esp32-s3-8mb` | 8MB | 8MB | S3 小容量预设 |
| `esp32-s3-16mb` | 16MB | 8MB | 常用默认选择 |
| `esp32-s3-32mb` | 32MB | 16MB | 更大 Flash / PSRAM 余量 |
| `esp32-p4-nano-16mb` | 16MB | 32MB | P4-NANO 预设 |

选择建议：硬件控制、传感器和状态屏优先从 ESP32-S3 或 ESP32-P4-NANO 开始；办公集成、长任务、本地脚本和服务化部署优先选择 Linux。

<a id="quick-start"></a>

## 快速开始

### 1. 准备工具链

- 安装 [esp-rs 工具链](https://docs.espressif.com/projects/rust-book/en/latest/introduction.html)：`espup install`
- 安装烧录工具：`cargo install espflash`
- Windows 需要安装 Visual Studio，并勾选 Desktop development for C++

如果你只想先了解配置和工作方式，可以先读 [ESP32 首次上手](docs/zh-cn/getting-started-esp.md) 或 [Linux 首次上手](docs/zh-cn/getting-started-linux.md)。

<a id="build-from-source"></a>

### 2. 从源码构建和烧录

macOS / Linux：

```bash
./build.sh
./build.sh --flash
BOARD=esp32-s3-16mb ./build.sh --flash
BOARD=esp32-p4-nano-16mb ./build.sh --flash
```

Windows：

```powershell
.\build.ps1
.\build.ps1 --flash
$env:BOARD="esp32-s3-16mb"; .\build.ps1 --flash
$env:BOARD="esp32-p4-nano-16mb"; .\build.ps1 --flash
```

不设置 `BOARD` 或 `--target` 时，构建脚本会尝试自动识别唯一连接的开发板。识别不到、识别结果不受支持，或同时连接了多个串口时，请手动设置 `BOARD`。

P4-NANO 完整烧录通常包含 C6 辅助固件和 P4 主固件两步：

```bash
./build.sh flash-c6
BOARD=esp32-p4-nano-16mb ./build.sh --flash
./build.sh flash-all
```

烧录 C6 辅助固件前，先让 P4 进入 bootloader 模式，避免共享板级连线互相干扰。

Linux 构建和打包：

```bash
TARGET=linux ./build.sh
TARGET=linux ./build.sh --package-linux
./build.sh --deploy-linux
```

Linux 版本适合做常驻服务和完整工具环境。发布、重启、停止和回滚见 [Linux 运维](docs/zh-cn/linux-release-rollback.md)。

### 3. 打开配置页

首次启动后，ESP 设备会开启名为 **Beetle** 的热点。

1. 手机或电脑连接热点 **Beetle**。
2. 浏览器打开 **http://192.168.4.1**。
3. 设置配对码。
4. 配置 WiFi、大模型和要使用的聊天通道。

设备连接路由器后，可以通过局域网 IP 打开配置页。Linux 版本如果启动时已有可用网络，会优先继承当前系统网络；此时请访问设备当前 LAN IP。

## 配置页

<p align="center">
  <img src="docs/assets/readme-original-pairing.png" alt="Beetles OS pairing page" width="900" style="max-width: 100%; height: auto;" />
</p>

首次访问配置页时，需要完成设备地址与配对码设置。之后配置页承担本地控制台职责：查看连接状态、设备信息、通道连通性、可用工具和操作记录；维护模型、通道、硬件与系统参数。

<p align="center">
  <img src="docs/assets/readme-original-dashboard.png" alt="Beetles OS local configuration dashboard" width="1100" style="max-width: 100%; height: auto;" />
</p>

<p align="center">
  <img src="docs/assets/readme-original-dashboard-menu.png" alt="Beetles OS configuration menu" width="1100" style="max-width: 100%; height: auto;" />
</p>

配置页覆盖常用设置：

| 配置区 | 作用 |
|--------|------|
| 网络 | 路由器 WiFi 名称和密码 |
| 模型 | 模型来源、模型名、密钥、服务地址、回退顺序 |
| 消息通道 | Telegram、飞书、钉钉、企业微信、QQ 频道等通道凭据 |
| 代理与搜索 | 网络代理和搜索服务配置 |
| 硬件 | 注册可被设备智能体控制的外设 |
| 屏幕 | SPI TFT 状态屏或 Linux 屏幕输出 |
| 系统 | 健康检查、重启、恢复和系统状态 |

自定义前端、脚本或集成程序可参考 [配置 API](docs/zh-cn/config-api.md)。

## 仓库结构

| 路径 | 说明 |
|------|------|
| `src/` | 核心系统、智能体主循环、通道、工具、记忆、平台抽象 |
| `configure-ui/` | Web 配置前端，也可以打包成桌面壳 |
| `docs/` | 公开文档、配置说明、集成说明和运维说明 |
| `components/` | ESP-IDF 侧组件与底层封装 |
| `scripts/` | 构建、烧录、检查和辅助脚本 |
| `storage_data/` | ESP 侧默认存储数据 |
| `packaging/` | Linux 发布包相关内容 |
| `tests/` | 系统、工具、记忆和智能体行为测试 |

## 后续阅读

| 你的目标 | 文档 |
|----------|------|
| 第一次配置 ESP32 设备 | [ESP32 首次上手](docs/zh-cn/getting-started-esp.md) |
| 第一次部署 Linux 版本 | [Linux 首次上手](docs/zh-cn/getting-started-linux.md) |
| 查看完整能力范围 | [能力概览](docs/zh-cn/capabilities.md) |
| 配置大模型服务商 | [模型服务配置](docs/zh-cn/llm-providers.md) |
| 配置硬件外设 | [硬件设备配置](docs/zh-cn/hardware-device-config.md) |
| 查看支持板型和硬件问题 | [硬件说明](docs/zh-cn/hardware.md) |
| 调用开放接口 | [配置 API](docs/zh-cn/config-api.md) |
| 部署、重启和回滚 Linux 版本 | [Linux 运维](docs/zh-cn/linux-release-rollback.md) |
| 了解系统结构和扩展点 | [架构说明](docs/zh-cn/architecture.md) |
| 浏览完整文档地图 | [docs/README.md](docs/README.md) |

## 常见问题

- 烧录失败：先检查 USB 线和串口是否被其他程序占用；需要手动指定串口时设置 `ESPFLASH_PORT`。
- `flash-c6` 失败：检查 C6 辅助固件烧录串口 `ESP_HOSTED_C6_PORT`，并让 P4 进入 bootloader 模式。
- 配置页打不开：重新连接热点 **Beetle**，再打开 `http://192.168.4.1`；Linux 版本请确认当前 LAN IP。
- 启动提示找不到存储分区：通常由板型预设或分区表不匹配导致，错误文本可能包含 `storage partition could not be found`。
- 通道无消息：检查模型配置、通道凭据、网络连通性和允许访问的会话范围。
- 硬件工具没有出现：先在配置页确认外设能力已经注册；需要手写配置时再检查 `hardware.json`。

## 关于我们

Beetles OS 由 **Openbeetles** 发起。这个项目关注的是大模型进入真实设备后的工程边界：设备要能接入模型，也要能配置、执行、诊断、恢复和长期维护。

我们欢迎对边缘智能体、硬件控制、Linux 部署、模型接入和设备运维感兴趣的开发者一起参与。

## 许可

Beetles OS 使用 **MIT OR Apache-2.0** 双许可证。详见 [LICENSE-MIT](LICENSE-MIT) 和 [LICENSE-APACHE](LICENSE-APACHE)。
