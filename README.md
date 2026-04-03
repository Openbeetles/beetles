<p align="center">
  <img src="configure-ui/public/logo.png" alt="Beetle" width="132" height="132" />
</p>

<h1 align="center">Beetle</h1>

<p align="center">
  <strong>Edge AI Agent firmware for ESP32-S3</strong><br/>
  Rust · ReAct · Tools · Memory · Hardware control
</p>

<p align="center">
  <a href="README.zh-CN.md">中文</a> · <strong>English</strong>
</p>

<p align="center">
  <a href="docs/README.md"><img alt="Docs" src="https://img.shields.io/badge/Docs-index-1f6feb" /></a>
  <a href="#quick-start"><img alt="Quick Start" src="https://img.shields.io/badge/Quick%20Start-5%20minutes-2ea043" /></a>
  <a href="#supported-boards"><img alt="Boards" src="https://img.shields.io/badge/Boards-ESP32--S3-orange" /></a>
  <a href="LICENSE"><img alt="License" src="https://img.shields.io/badge/License-MIT%20OR%20Apache--2.0-blue.svg" /></a>
</p>

Beetle is a firmware runtime that lets one ESP32-S3 board act like an AI agent:

- It receives messages from chat channels.
- It calls tools and uses memory on the device.
- It can control hardware such as GPIO, PWM, ADC, and buzzer devices.
- It exposes a browser-based configuration flow and an HTTP config API.

If you want to flash a board and get to a working config page quickly, start here. If you are integrating the device into your own frontend or workflow, jump to [docs/README.md](docs/README.md).

## What Is In The Repo

| Part | What it is for |
|------|----------------|
| Firmware runtime | The main ESP32-S3 agent firmware |
| `configure-ui` | Full web configuration frontend |
| HTTP config API | For custom frontends, scripts, and integrations |
| Display system | Optional SPI TFT dashboard |
| Host-side build paths | Development, document tools, and Linux packaging |

## Current Scope

This repository currently documents and ships a stable firmware path for **ESP32-S3 boards with PSRAM**.

Supported board presets:

- `esp32-s3-8mb`
- `esp32-s3-16mb`
- `esp32-s3-32mb`

The repo also contains non-ESP build paths for development and integration work. Those paths are useful, but this README is primarily about the ESP32-S3 firmware flow.

## What You Can Do With It

- Build a chat-connected AI device that runs directly on one board.
- Connect Feishu, DingTalk, WeCom, and QQ Channel to the same runtime.
- Enable `telegram` or `websocket` through optional Cargo features.
- Store summaries, long-term memory, reminders, tasks, and archive evidence on the device.
- Drive configured hardware through the `device_control` tool.
- Show runtime status on an SPI display.

## Quick Start

### 1. Prepare the toolchain

- Install the [esp-rs toolchain](https://docs.espressif.com/projects/rust-book/en/latest/introduction.html) with `espup install`
- Install `espflash` with `cargo install espflash`
- On Windows, install Visual Studio with Desktop development for C++

### 2. Build or flash

macOS / Linux:

```bash
./build.sh
./build.sh --flash
BOARD=esp32-s3-16mb ./build.sh --flash
ESPFLASH_PORT=/dev/cu.usbserial-xxx ./build.sh --flash
```

Windows:

```powershell
.\build.ps1
.\build.ps1 --flash
$env:BOARD="esp32-s3-16mb"; .\build.ps1 --flash
$env:ESPFLASH_PORT="COM3"; .\build.ps1 --flash
```

### 3. Open the config page

On first boot, the device opens a hotspot named **Beetle**.

1. Connect your phone or computer to that hotspot.
2. Open **http://192.168.4.1** in a browser.
3. Set the pairing code.
4. Configure WiFi, LLM, and the chat channel you want to use.

After the device joins your router, open the config page again through the device's LAN IP.

## Build Notes

Default Cargo features:

- `config_api`
- `feishu`
- `tools_diagnostics`
- `tools_network_extra`
- `thread_panic_catch`

Optional features:

- `telegram`
- `websocket`
- `cli`
- `ota`

Example:

```bash
cargo build --release --features telegram,ota
```

Board selection is controlled by `BOARD`. The build scripts read `board_presets.toml` and choose the right target, partition table, and flash size.

## Supported Boards

| BOARD | Flash | PSRAM | Notes |
|------|-------|-------|-------|
| `esp32-s3-8mb` | 8MB | 8MB | N8R8 |
| `esp32-s3-16mb` | 16MB | 8MB | Default preset |
| `esp32-s3-32mb` | 32MB | 16MB | N32R16 |

Important:

- Use the project's partition table.
- If you see `spiffs partition could not be found`, the board preset or partition setup is wrong.

## Main Capabilities

| Area | What Beetle provides |
|------|----------------------|
| Channels | One runtime for multiple chat channels |
| Memory | Session summary, long-term memory, archive evidence search |
| Tools | Time, reminders, task/calendar, file ops, board info, networking, hardware control |
| Hardware | Config-driven `device_control` for GPIO, PWM, ADC, buzzer, and related device types |
| Config | Built-in browser flow plus a full HTTP config API |
| Display | SPI TFT status dashboard |
| Health | Metrics, resource snapshots, diagnostics, and restart/reset operations |

## Where To Read Next

| If you want to... | Read this |
|-------------------|-----------|
| Flash the board and configure it | [docs/en-us/configuration.md](docs/en-us/configuration.md) |
| Call the HTTP API yourself | [docs/en-us/config-api.md](docs/en-us/config-api.md) |
| Understand what tools the agent can use | [docs/en-us/tools.md](docs/en-us/tools.md) |
| Configure LLM providers | [docs/en-us/llm-providers.md](docs/en-us/llm-providers.md) |
| Configure an SPI display | [docs/en-us/display.md](docs/en-us/display.md) |
| Configure hardware devices | [docs/en-us/hardware-device-config.md](docs/en-us/hardware-device-config.md) |
| Check board limits and troubleshooting | [docs/en-us/hardware.md](docs/en-us/hardware.md) |
| See the full docs map | [docs/README.md](docs/README.md) |

## Troubleshooting

- Flash fails: check the USB cable, port, and `ESPFLASH_PORT`.
- Device is not reachable: reconnect to hotspot `Beetle` and open `http://192.168.4.1`.
- `spiffs partition could not be found`: use the project board preset and partition table.
- Need exact API behavior: see [docs/en-us/config-api.md](docs/en-us/config-api.md).
- Need board/resource details: see [docs/en-us/hardware.md](docs/en-us/hardware.md).

## License

Beetle is dual-licensed under **MIT OR Apache-2.0**. See [LICENSE](LICENSE).
