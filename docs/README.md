# Beetle Documentation Index

**中文** | [English below](#english)

这里可以当成 Beetle 文档的导航页来用。
如果你还不知道应该先看哪篇，直接按下面这张表找就行。

## 我该看哪篇

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

## 建议怎么读

如果你只是想把设备先用起来：

1. `configuration`
2. `tools`
3. 需要时再看 `hardware` 或 `display`

如果你是在接前端、脚本或者外部系统：

1. `configuration`
2. `config-api`
3. `tools`
4. `llm-providers`

如果你准备动代码、做扩展或者排查底层问题：

1. `architecture`
2. `hardware`
3. `hardware-device-config`

## 维护说明

如果你在维护仓库文档，下面几个文件可以当成单一事实来源：

- HTTP 路由与鉴权：[`src/platform/http_server/router/dispatch.rs`](../src/platform/http_server/router/dispatch.rs)
- 工具注册表：[`src/tools/registry.rs`](../src/tools/registry.rs)
- LLM 客户端与回退链：[`src/llm/mod.rs`](../src/llm/mod.rs)、[`src/llm/fallback.rs`](../src/llm/fallback.rs)
- 板型预设：[`board_presets.toml`](../board_presets.toml)

---

## English

This is the navigation page for Beetle docs.
If you are not sure where to start, the table above is the fastest way in.

### Suggested reading order

End users:

1. `configuration`
2. `tools`
3. `hardware` or `display` if needed

Integrators:

1. `configuration`
2. `config-api`
3. `tools`
4. `llm-providers`

Developers extending the project:

1. `architecture`
2. `hardware`
3. `hardware-device-config`

### Maintainer notes

Single sources of truth:

- HTTP routes and auth: [`src/platform/http_server/router/dispatch.rs`](../src/platform/http_server/router/dispatch.rs)
- Tool registry: [`src/tools/registry.rs`](../src/tools/registry.rs)
- LLM routing and fallback: [`src/llm/mod.rs`](../src/llm/mod.rs), [`src/llm/fallback.rs`](../src/llm/fallback.rs)
- Board presets: [`board_presets.toml`](../board_presets.toml)
