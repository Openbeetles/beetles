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

## Choosing a platform

- Choose ESP32-S3 when local hardware control is the main goal.
- Choose Linux when you want fuller Beetle capability and easier integration.
- Choose ESP32-P4 when you need a higher-end ESP setup.

## Common Status Checks

| Where | What you learn |
|-------|----------------|
| Config UI | Best first stop for normal users |
| `GET /api/health` | Overall device status |
| serial logs | Boot info, heartbeat, and warnings |

If you need exact field definitions, read [config-api.md](config-api.md).

## Hardware Device Config

If you want the agent to control LEDs, relays, buzzers, sensors, or PWM devices, read:

- [hardware-device-config.md](hardware-device-config.md)
- [tools.md](tools.md) for `device_control`

## Common Problems

- `spiffs partition could not be found`
  Use the project's board preset and partition table.
- Linux is online but Beetle looks unreachable
  Make sure you are opening the device's current LAN address.
