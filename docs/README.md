# Beetle Documentation Index

**中文** | [English below](#english)

这是 Beetle 的文档目录。
按下面的表选择对应文档。

## 文档目录

| 你的目标 | 中文 | English |
|----------|------|---------|
| 第一次上手，连热点、打开配置页、完成配网 | [zh-cn/configuration.md](zh-cn/configuration.md) | [en-us/configuration.md](en-us/configuration.md) |
| 自己写前端、脚本或集成设备 API | [zh-cn/config-api.md](zh-cn/config-api.md) | [en-us/config-api.md](en-us/config-api.md) |
| 想知道 Agent 现在能用哪些工具 | [zh-cn/tools.md](zh-cn/tools.md) | [en-us/tools.md](en-us/tools.md) |
| 配置 LLM 提供商和回退顺序 | [zh-cn/llm-providers.md](zh-cn/llm-providers.md) | [en-us/llm-providers.md](en-us/llm-providers.md) |
| 配 SPI 显示屏 | [zh-cn/display.md](zh-cn/display.md) | [en-us/display.md](en-us/display.md) |
| 看支持的板型、资源限制和排错入口 | [zh-cn/hardware.md](zh-cn/hardware.md) | [en-us/hardware.md](en-us/hardware.md) |
| 用 `hardware.json` 定义外设并交给 Agent 控制 | [zh-cn/hardware-device-config.md](zh-cn/hardware-device-config.md) | [en-us/hardware-device-config.md](en-us/hardware-device-config.md) |
| 看模块结构和扩展方式 | [zh-cn/architecture.md](zh-cn/architecture.md) | [en-us/architecture.md](en-us/architecture.md) |
| 了解 Linux 发布包现状 | [zh-cn/linux-release-rollback.md](zh-cn/linux-release-rollback.md) | [en-us/linux-release-rollback.md](en-us/linux-release-rollback.md) |
| 查看品牌 Logo 资源说明 | [assets/README.md](assets/README.md) | [assets/README.md](assets/README.md) |

## 推荐阅读顺序

首次配置：

1. `configuration`
2. `tools`
3. 需要硬件配置时再看 `hardware`、`hardware-device-config`、`display`

ESP 硬件接入：

1. `configuration`
2. `hardware`
3. `hardware-device-config`
4. `display`

Linux 部署与运行：

1. `configuration`
2. `tools`
3. `llm-providers`
4. `linux-release-rollback`

前端、脚本和外部系统集成：

1. `configuration`
2. `config-api`
3. `tools`
4. `llm-providers`

二次开发：

1. `architecture`
2. `hardware`
3. `hardware-device-config`

---

## English

This is the Beetle documentation index.
Use the table above to find the right document.

### Suggested Reading Order

First-time setup:

1. `configuration`
2. `tools`
3. `hardware`, `hardware-device-config`, and `display` if needed

ESP hardware work:

1. `configuration`
2. `hardware`
3. `hardware-device-config`
4. `display`

Linux deployment and runtime use:

1. `configuration`
2. `tools`
3. `llm-providers`
4. `linux-release-rollback`

Custom frontend, script, or integration:

1. `configuration`
2. `config-api`
3. `tools`
4. `llm-providers`

Code extension:

1. `architecture`
2. `hardware`
3. `hardware-device-config`
