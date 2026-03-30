# Architecture overview

**English** | [中文](../zh-cn/architecture.md) | [Doc index](../README.md)

This doc is for **readers who want to understand module layout, data flow, or how to add channels/tools/LLM**: it summarizes each module’s role, message flow, and extension points. No internal implementation detail.

---

## Module layout

| Module | Responsibility |
|--------|----------------|
| **config** | Build-time / env and NVS, SPIFFS config load and validation; secrets not logged or written to disk. |
| **error** | Unified error type (stage is `&'static str`); public APIs return `Result<T, Error>`. |
| **bus** | Inbound/outbound message queues (fixed capacity, backpressure); decouples channels from Agent. |
| **orchestrator** | Unified resource orchestrator: atomic state aggregation (heap, connections, pressure, channel health), HTTP admission with priority and TLS single-concurrency, four-dimensional gating (inbound/outbound/LLM/tool), channel circuit breaker. Zero heap alloc, lock-free (except TLS Mutex), xtensa compatible. |
| **memory** | Long-term memory and session storage; system prompt aggregation. |
| **platform** | Platform abstraction (config store, skill store, HTTP client, etc.) and ESP32 implementation; only module that directly depends on esp-idf-svc. |
| **llm** | LLM client abstraction; supports Anthropic, OpenAI-compatible (e.g. Ollama), etc. |
| **tools** | Tool registry; built-ins are listed in [Agent tools](tools.md); new tools implement `Tool` and register in `build_default_registry`. |
| **agent** | Context build, ReAct loop; depends on LlmClient, ToolRegistry, Memory, Session. |
| **channels** | Channel abstraction and dispatch; Telegram, Feishu, DingTalk, WeCom, QQ Channel, WebSocket; inbound pushes to bus, outbound dispatched by channel; channel health tracking delegated to orchestrator. |
| **display** | Display configuration types (`DisplayConfig`, `DisplayCommand`, `DisplaySystemState`) and rendering. SPI backend on ESP32 (`display_driver.rs`): PSRAM framebuffer, ST7789/ILI9341/ST7735 init, `DrawTarget` impl, beetle icon + dashboard rendering via `embedded-graphics`. Host stub returns `available: false`. |
| **metrics** | Runtime metrics and error profile: messages in/out, LLM/tool calls and errors, WDT feed, dispatch success/fail, per-stage error aggregation (incl. session write failures); exposed via health API and heartbeat baseline logs. |
| **cli** (optional) | Serial commands: wifi_status, heap_info, session_list, restart, ota, etc. |
| **ota** (optional) | Fetch firmware from URL, write to OTA partition; failure does not corrupt current partition. |
| **cron / heartbeat / skills** | Scheduled tasks, periodic logs (incl. metrics baseline), SPIFFS skill loading. |

**Platform boundary note**: Besides `platform/`, `channels/wss_gateway/esp_conn.rs` is ESP-only WSS transport and directly depends on `esp-idf-svc` (needs `esp_websocket_client` from `esp-idf-sys` `extra_components` with matching bindings). This is an **intentional exception** to routing hardware access through `platform` only.

---

## Data flow

```
  Channels (Feishu / DingTalk / WeCom / QQ / Telegram / WebSocket)
       ↓ push
  Inbound queue
       ↓
  Agent (build_context → LlmClient → Tools → write session)
       ↓ push
  Outbound queue
       ↓
  Dispatch to each MessageSink by channel
```

- **Inbound**: Channels (or cron) push user/system messages into Inbound; Agent consumes from Inbound.
- **Agent**: Aggregates system prompt and history from Memory/Session, calls LLM; on tool_use runs tools and appends results, loops until end_turn; writes session and pushes reply to Outbound.
- **Outbound**: Dispatch takes from Outbound and calls each channel's send; channel health (consecutive failures and cooldown) is tracked by the orchestrator module.

**Observability and health**: HTTP fields and auth are documented under [config-api: GET /api/health](config-api.md#get-apihealth); heartbeat emits periodic baseline logs aligned with `metrics` (details follow the firmware).

---

## How to extend

- **New channel**: Outbound uses `dispatch::QueuedSink` (`QueuedSink::new(tx, "stage")`); register in main's `run_app` into dispatch's sink list. Channel side implements `flush_*_sends` reading from the corresponding rx and sending HTTP. Inbound: send messages to the bus Inbound. For custom send logic, implement `MessageSink` and register.
- **New tool**: Implement `Tool` trait (`name`, `description`, `schema` with parameters, `execute`); in `tools/mod.rs` use `parse_tool_args(args, stage)` for JSON args; register in [`build_default_registry`](../../src/tools/registry.rs) (or the equivalent in `main`); return value is truncated to `MAX_TOOL_RESULT_LEN` by the registry. Network/diagnostic tools can use Cargo features **`tools_network_extra`** / **`tools_diagnostics`** like the built-ins.
- **New LLM backend**: Implement `LlmClient` trait; main injects it into the agent.

Core (agent, bus, llm, tools, memory) does not depend on concrete channel or platform; it only depends on abstract traits, which keeps maintenance and extension straightforward.
