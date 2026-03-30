# 架构概要

[English](../en-us/architecture.md) | **中文** | [文档索引](../README.md)

本文档面向**希望了解模块划分、数据流或做二次扩展的读者**，简要说明各模块职责、消息流向与如何新增通道/工具/LLM，不涉及内部实现细节。

---

## 模块划分

| 模块 | 职责 |
|------|------|
| **config** | 编译时/环境变量与 NVS、SPIFFS 配置加载与校验；密钥不打印、不落盘。 |
| **error** | 统一错误类型（stage 为 `&'static str`）；公共 API 返回 `Result<T, Error>`。 |
| **bus** | 入站/出站消息队列（固定容量、背压）；通道与 Agent 解耦。 |
| **orchestrator** | 统一资源编排器：原子状态聚合（堆、连接数、压力等级、通道健康），带优先级的 HTTP 准入与 TLS 单并发控制，四维门禁（入站/出站/LLM/工具），通道熔断。零堆分配、零锁（除 TLS Mutex）、xtensa 兼容。 |
| **memory** | 长期记忆与会话存储；系统提示聚合。 |
| **platform** | 平台抽象（配置存储、技能存储、HTTP 客户端等）与 ESP32 实现；唯一直接依赖 esp-idf-svc 的模块。 |
| **llm** | LLM 客户端抽象；支持 Anthropic、OpenAI 兼容（含 Ollama）等。 |
| **tools** | 工具注册表；内置工具见 [Agent 工具说明](tools.md)；新工具实现 `Tool` trait 并在 `build_default_registry` 中注册。 |
| **agent** | 上下文构建、ReAct 循环；依赖 LlmClient、ToolRegistry、Memory、Session。 |
| **channels** | 通道抽象与分发；Telegram、飞书、钉钉、企微、QQ 频道、WebSocket 等；入站推 bus，出站由 dispatch 按 channel 分发；通道健康追踪委托给 orchestrator。 |
| **display** | 显示配置类型（`DisplayConfig`、`DisplayCommand`、`DisplaySystemState`）与渲染。ESP32 上的 SPI 后端（`display_driver.rs`）：PSRAM 帧缓冲、ST7789/ILI9341/ST7735 初始化、`DrawTarget` 实现、甲壳虫图标 + 仪表板通过 `embedded-graphics` 渲染。Host 编译返回 `available: false`。 |
| **metrics** | 运行指标与错误画像：消息进/出、LLM/tool 调用与错误、WDT feed、dispatch 成功/失败、按 stage 聚合错误（含 session 写入失败）；供 health API 与 heartbeat 基线日志暴露。 |
| **cli** (可选) | 串口命令：wifi_status、heap_info、session_list、restart、ota 等。 |
| **ota** (可选) | 从 URL 拉取固件、写 OTA 分区；失败不破坏当前分区。 |
| **cron / heartbeat / skills** | 定时任务、周期日志（含 metrics 基线）、SPIFFS 技能加载。 |

**平台边界补充**：除 `platform/` 外，`channels/wss_gateway/esp_conn.rs` 为 ESP 专用 WSS 传输，直接依赖 `esp-idf-svc`（需 `esp-idf-sys` 的 `extra_components` 提供 `esp_websocket_client` 且绑定一致）。相对「业务层经 `platform` 访问硬件」而言，这是**有意的例外**。

---

## 数据流

```
  通道（飞书/钉钉/企微/QQ/Telegram/WebSocket）
       ↓ push
  入站队列 (Inbound)
       ↓
  Agent（build_context → LlmClient → Tools → 写会话）
       ↓ push
  出站队列 (Outbound)
       ↓
  Dispatch 按 channel 分发给各 MessageSink
```

- **入站**：各通道（或 cron）将用户/系统消息推入 Inbound；Agent 从 Inbound 取消息处理。
- **Agent**：从 Memory/Session 聚合系统提示与历史消息，调用 LLM；若有 tool_use 则执行工具并追加结果，循环直至 end_turn；写会话并将回复推入 Outbound。
- **出站**：Dispatch 从 Outbound 取消息，按 channel 调用对应通道的发送接口；通道健康（连续失败与冷却）由 orchestrator 模块统一追踪。

**可观测与健康**：HTTP 字段与鉴权见 [配置 API：GET /api/health](config-api.md#get-apihealth)；heartbeat 周期性输出基线日志（与 `metrics` 对齐，具体以固件为准）。

---

## 扩展方式

- **新通道**：出站统一使用 `dispatch::QueuedSink`（`QueuedSink::new(tx, "stage")`），在 main 的 `run_app` 编排中注册到 dispatch 的 sink 列表；通道侧只需实现 `flush_*_sends` 从对应 rx 取消息并发 HTTP。入站时向 bus 的 Inbound 发送消息。若需自定义发送逻辑，可实现 `MessageSink` trait 并注册。
- **新工具**：实现 `Tool` trait（`name`、`description`、`schema` 含 parameters、`execute`），在 `tools/mod.rs` 中可用 `parse_tool_args(args, stage)` 解析 JSON 参数；在 [`build_default_registry`](../../src/tools/registry.rs)（或 main 中等价注册点）加入 `registry.register`；返回值由 Registry 统一截断至 `MAX_TOOL_RESULT_LEN`。网络/诊断类工具可挂在 Cargo feature **`tools_network_extra`** / **`tools_diagnostics`** 上，与现有一致。
- **新 LLM 后端**：实现 `LlmClient` trait，由 main 注入给 agent。

核心（agent、bus、llm、tools、memory）不依赖具体通道或平台实现，仅依赖抽象 trait，便于维护与扩展。
