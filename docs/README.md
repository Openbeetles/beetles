# Beetle Documentation Index

**中文** | [English below](#english)

这里是 Beetle 的总文档目录。
先按你的身份选文档，不要一上来就钻技术参考。

## 普通用户先看这些

| 你的目标 | 中文 | English |
|----------|------|---------|
| 第一次上手，连热点、打开配置页、完成配网 | [zh-cn/configuration.md](zh-cn/configuration.md) | [en-us/configuration.md](en-us/configuration.md) |
| 想知道 Beetle Agent OS 现在能做什么、会看到什么 | [zh-cn/tools.md](zh-cn/tools.md) | [en-us/tools.md](en-us/tools.md) |
| 配置大模型服务商和切换顺序 | [zh-cn/llm-providers.md](zh-cn/llm-providers.md) | [en-us/llm-providers.md](en-us/llm-providers.md) |
| 配 SPI 显示屏 | [zh-cn/display.md](zh-cn/display.md) | [en-us/display.md](en-us/display.md) |
| 看支持的板型和常见硬件问题 | [zh-cn/hardware.md](zh-cn/hardware.md) | [en-us/hardware.md](en-us/hardware.md) |

## 集成和部署再看这些

| 你的目标 | 中文 | English |
|----------|------|---------|
| 自己写前端、脚本或调用设备接口 | [zh-cn/config-api.md](zh-cn/config-api.md) | [en-us/config-api.md](en-us/config-api.md) |
| 用 `hardware.json` 定义外设并交给 Beetle 控制 | [zh-cn/hardware-device-config.md](zh-cn/hardware-device-config.md) | [en-us/hardware-device-config.md](en-us/hardware-device-config.md) |
| 看 Linux 版 Agent OS 的安装、打包和回滚说明 | [zh-cn/linux-release-rollback.md](zh-cn/linux-release-rollback.md) | [en-us/linux-release-rollback.md](en-us/linux-release-rollback.md) |

## 开发和扩展再看这些

| 你的目标 | 中文 | English |
|----------|------|---------|
| 看模块结构和扩展方式 | [zh-cn/architecture.md](zh-cn/architecture.md) | [en-us/architecture.md](en-us/architecture.md) |
| 查看品牌 Logo 资源说明 | [assets/README.md](assets/README.md) | [assets/README.md](assets/README.md) |

## 推荐阅读顺序

第一次上手：

1. `configuration`
2. `tools`
3. 需要屏幕或硬件时再看 `display`、`hardware`

ESP 硬件接入：

1. `configuration`
2. `hardware`
3. `hardware-device-config`
4. `display`

Linux 版 Agent OS：

1. `configuration`
2. `tools`
3. `llm-providers`
4. `linux-release-rollback`

前端、脚本和外部系统：

1. `configuration`
2. `config-api`
3. `llm-providers`
4. `tools`

二次开发：

1. `architecture`
2. `hardware`
3. `hardware-device-config`

---

## English

This is the Beetle documentation index.
Choose by audience first.

### Start Here: user guides

| What you want to do | English | 中文 |
|---------------------|---------|------|
| First setup, hotspot access, pairing code, WiFi setup | [en-us/configuration.md](en-us/configuration.md) | [zh-cn/configuration.md](zh-cn/configuration.md) |
| Understand what Beetle Agent OS can do and what you will see | [en-us/tools.md](en-us/tools.md) | [zh-cn/tools.md](zh-cn/tools.md) |
| Configure LLM providers and choose the order to try them | [en-us/llm-providers.md](en-us/llm-providers.md) | [zh-cn/llm-providers.md](zh-cn/llm-providers.md) |
| Set up an SPI display | [en-us/display.md](en-us/display.md) | [zh-cn/display.md](zh-cn/display.md) |
| Check supported boards and common hardware issues | [en-us/hardware.md](en-us/hardware.md) | [zh-cn/hardware.md](zh-cn/hardware.md) |

### Integration and deployment

| What you want to do | English | 中文 |
|---------------------|---------|------|
| Build your own frontend, script, or integration | [en-us/config-api.md](en-us/config-api.md) | [zh-cn/config-api.md](zh-cn/config-api.md) |
| Configure external devices with `hardware.json` | [en-us/hardware-device-config.md](en-us/hardware-device-config.md) | [zh-cn/hardware-device-config.md](zh-cn/hardware-device-config.md) |
| Install, package, or roll back Beetle on Linux | [en-us/linux-release-rollback.md](en-us/linux-release-rollback.md) | [zh-cn/linux-release-rollback.md](zh-cn/linux-release-rollback.md) |

### Development and extension

| What you want to do | English | 中文 |
|---------------------|---------|------|
| Understand module boundaries and extension points | [en-us/architecture.md](en-us/architecture.md) | [zh-cn/architecture.md](zh-cn/architecture.md) |
| Read logo asset notes | [assets/README.md](assets/README.md) | [assets/README.md](assets/README.md) |

### Suggested reading order

First-time setup:

1. `configuration`
2. `tools`
3. `display` and `hardware` only if needed

ESP hardware work:

1. `configuration`
2. `hardware`
3. `hardware-device-config`
4. `display`

Linux Agent OS use:

1. `configuration`
2. `tools`
3. `llm-providers`
4. `linux-release-rollback`

Custom frontend, scripts, and integrations:

1. `configuration`
2. `config-api`
3. `llm-providers`
4. `tools`

Code extension:

1. `architecture`
2. `hardware`
3. `hardware-device-config`
