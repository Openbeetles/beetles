# Agent Tools

[中文](../zh-cn/tools.md) | **English** | [Doc index](../README.md)

This page explains the tools Beetle can register at runtime.

Important:

- users do not manually type tool names in normal chat
- the model may call these tools automatically
- the exact visible tool set depends on target, features, config, and policy

The authoritative registry is [`build_default_registry`](../../src/tools/registry.rs).

## Always Registered

| Tool | What it is for |
|------|----------------|
| `get_time` | current UTC time |
| `env` | read environment values for runtime/debug scenarios |
| `message` | send a runtime-managed outbound message |
| `task` | persistent task management |
| `calendar` | persistent calendar events |
| `files` | list or read files under the state root |
| `file_edit` | patch an existing text file under the state root |
| `remind_at` | create a reminder |
| `remind_list` | list reminders in the current chat |
| `board_info` | chip, heap, PSRAM, uptime, WiFi, SPIFFS |
| `kv_store` | persistent key-value storage |
| `private_garden` | current-chat private workspace |
| `memory_search` | search archive evidence from transcripts, daily notes, and turn logs |
| `memory_get` | fetch one archive evidence record |
| `continuity_snapshot` | export/import continuity state |
| `file_write` | write or append to allowed files under the state root |

## Tools Behind `tools_network_extra`

| Tool | What it is for |
|------|----------------|
| `web_search` | web search |
| `analyze_image` | vision analysis for an image URL |
| `http_request` | public HTTP requests |
| `proxy_config` | read/write proxy config |
| `model_config` | read/write model config fields |

## Extra Non-ESP Tools With `tools_network_extra`

These appear only on non-ESP builds:

| Tool | What it is for |
|------|----------------|
| `document_search` | search stored documents |
| `document_read` | read a public URL or stored document |
| `document_extract` | extract lines, sections, or JSON fields |
| `web_fetch` | fetch a public web page as readable text |
| `pdf_read` | fetch and read a public PDF |

## Tools Behind `tools_diagnostics`

| Tool | What it is for |
|------|----------------|
| `memory_manage` | manage long-term memory and related text stores |
| `session_manage` | list, inspect, clear, or delete sessions |
| `system_control` | restart and storage-related system actions |
| `cron_manage` | persistent scheduled tasks |
| `network_scan` | WiFi and connectivity checks |

Conditionally registered with diagnostics enabled:

| Tool | When it appears |
|------|------------------|
| `device_control` | when `hardware_devices` is configured |
| `sensor_watch` | when watchable hardware or I2C sensors exist |
| `i2c_device` | when I2C bus and I2C devices are configured |
| `i2c_sensor` | when I2C bus and I2C sensors are configured |

## Audio Tools

These appear only when audio config and required credentials are present:

| Tool | What it is for |
|------|----------------|
| `voice_input` | speech-to-text |
| `voice_output` | text-to-speech |

## Host-Only Utilities

These are available only on non-ESP builds:

| Tool | What it is for |
|------|----------------|
| `shell` | restricted shell execution |
| `process` | host process operations |
| `network` | host network diagnostics |

## Usage Notes

- `files` is read-only; `file_write` and `file_edit` are limited to allowed mutable paths.
- `private_garden` is scoped to the current chat.
- `memory_search` and `memory_get` return archive evidence, not canonical memory truth.
- `http_request`, `web_fetch`, and `pdf_read` reject private/internal targets.
- `GET /api/tools` may not show every runtime-registered tool; the runtime registry is the source of truth.

Related docs:

- [hardware-device-config.md](hardware-device-config.md)
- [config-api.md](config-api.md)
