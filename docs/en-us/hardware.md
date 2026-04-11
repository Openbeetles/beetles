# Hardware and Board Notes

**English** | [中文](../zh-cn/hardware.md) | [Doc index](../README.md)

This page covers three practical topics:

1. which ESP32-S3 boards are supported
2. what the Linux Agent OS currently provides
3. where to look when something goes wrong

## ESP32-S3 Supported Boards

| BOARD | Flash | PSRAM | Notes |
|------|-------|-------|------|
| `esp32-s3-8mb` | 8MB | 8MB | N8R8 |
| `esp32-s3-16mb` | 16MB | 8MB | Default preset |
| `esp32-s3-32mb` | 32MB | 16MB | N32R16 |

- only **ESP32-S3 with PSRAM** is supported by the board presets in this repo

## Linux Agent OS Status

The Linux Agent OS is stable.

- channels, memory, tools, and the config/API surface are part of that stable path
- it is the better fit for fuller Agent OS capabilities, deployment, and integration work

## Resource and Runtime Behavior

- Large allocations prefer PSRAM.
- The orchestrator gates work based on runtime pressure.
- HTTP, LLM, and tool activity can be limited when memory is tight.
- Long HTTP or LLM operations must coexist with the task watchdog.
- The lazy ESP config-plane route worker (`http_route_exec`) stays on the standard Rust thread surface. Do not move that worker onto the raw native-task path.
- Task watchdog registration and status checks must use the explicit current task handle. Do not rely on `NULL` status probes as a shortcut for "already subscribed".

## Build Profiles

- `cargo build --release` uses `opt-level = 2`
- `cargo build --profile release-size` is the size-focused profile

## Common Status Checks

| Where | What you learn |
|-------|----------------|
| `GET /api/health` | Overall status and health snapshot |
| `GET /api/resource` | Runtime resource snapshot |
| serial logs | Boot info, heartbeat, and warnings |
| `cli` feature | Extra serial inspection commands such as `heap_info` |

Exact HTTP field shapes are documented in [config-api.md](config-api.md).

## Hardware Device Config

If you want the agent to control LEDs, relays, buzzers, sensors, or PWM devices, read:

- [hardware-device-config.md](hardware-device-config.md)
- [tools.md](tools.md) for `device_control`

## Common Problems

- `spiffs partition could not be found`
  Use the project's board preset and partition table.
