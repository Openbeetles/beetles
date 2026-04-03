# Hardware and Board Notes

**English** | [中文](../zh-cn/hardware.md) | [Doc index](../README.md)

This page answers three basic questions:

1. Which boards are officially covered by the current firmware presets?
2. What should I know about memory, build profile, and observability?
3. Where should I look when something goes wrong?

## Supported Boards

| BOARD | Flash | PSRAM | Notes |
|------|-------|-------|------|
| `esp32-s3-8mb` | 8MB | 8MB | N8R8 |
| `esp32-s3-16mb` | 16MB | 8MB | Default preset |
| `esp32-s3-32mb` | 32MB | 16MB | N32R16 |

Current rule:

- only **ESP32-S3 with PSRAM** is supported by the board presets in this repo

## Memory and Runtime Behavior

- Large allocations prefer PSRAM.
- The orchestrator gates work based on runtime pressure.
- HTTP, LLM, and tool activity can be limited when memory is tight.
- Long HTTP or LLM operations must coexist with the task watchdog.

## Build Profiles

- `cargo build --release` uses `opt-level = 2`
- `cargo build --profile release-size` is the size-focused profile

## What To Monitor

| Where | What you learn |
|-------|----------------|
| `GET /api/health` | Overall status and health snapshot |
| `GET /api/resource` | Runtime resource snapshot |
| serial logs | Boot info, heartbeat, and warnings |
| `cli` feature | Extra serial inspection commands such as `heap_info` |

Exact HTTP field shapes are documented in [config-api.md](config-api.md).

## Hardware Devices

If you want the agent to control LEDs, relays, buzzers, sensors, or PWM devices, read:

- [hardware-device-config.md](hardware-device-config.md)
- [tools.md](tools.md) for `device_control`

## Common Problems

- `spiffs partition could not be found`
  Use the project's board preset and partition table.

- `esp_task_wdt_reset: task not found`
  A thread doing HTTP was probably not registered with the task watchdog.

- `getaddrinfo() returns 202`
  Usually means DNS resolution failed or the network stack was not ready.
