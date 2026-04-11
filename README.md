<p align="center">
  <img src="configure-ui/public/logo.png" alt="Beetle" width="132" height="132" />
</p>

<h1 align="center">Beetle</h1>

<p align="center">
  <strong>Agent OS for ESP32-S3, ESP32-P4, and Linux</strong><br/>
  Rust · ReAct · Tools · Memory · Hardware control
</p>

<p align="center">
  <a href="README.zh-CN.md">中文</a> · <strong>English</strong>
</p>

<p align="center">
  <a href="docs/README.md"><img alt="Docs" src="https://img.shields.io/badge/Docs-index-1f6feb" /></a>
  <a href="#quick-start"><img alt="Quick Start" src="https://img.shields.io/badge/Quick%20Start-5%20minutes-2ea043" /></a>
  <a href="#supported-boards"><img alt="Boards" src="https://img.shields.io/badge/Boards-ESP32--S3%20%7C%20ESP32--P4-orange" /></a>
  <a href="LICENSE"><img alt="License" src="https://img.shields.io/badge/License-MIT%20OR%20Apache--2.0-blue.svg" /></a>
</p>

Beetle is an `Agent OS` for **ESP32-S3**, **ESP32-P4**, and **Linux**. It can connect chat channels, call tools, store memory, and control hardware.

Main capabilities:

- receive messages from chat channels
- call tools
- use memory
- expose a config UI and HTTP API
- control hardware on supported targets

Recommended use by platform:

| Target | Best suited for |
|--------|-----------------|
| ESP32-S3 | hardware control, peripherals, and always-on device agents |
| ESP32-P4 + on-board C6 | heavier ESP workloads with hosted Wi-Fi on the co-processor |
| Linux | fuller Agent OS capabilities, longer tasks, and complex integrations |

This README is the quick start.
For the full documentation map, go to [docs/README.md](docs/README.md).

## What Is In The Repo

| Part | What it is for |
|------|----------------|
| Core system | Shared Agent OS core for ESP32-S3 and Linux |
| `configure-ui` | Full web configuration frontend |
| HTTP config API | For custom frontends, scripts, and integrations |
| Display system | Optional SPI TFT dashboard |
| Linux Agent OS | Linux deployment and package docs |

## Current Support

Current supported targets:

- **ESP32-S3 with PSRAM**: best for hardware-facing deployments
- **ESP32-P4 + board-mounted ESP32-C6**: best for higher-end ESP deployments that still need Beetle on bare metal
- **Linux**: best for fuller Agent OS capabilities, integration, and deployment

Supported board presets:

- `esp32-s3-8mb`
- `esp32-s3-16mb`
- `esp32-s3-32mb`
- `esp32-p4-nano-16mb`

Linux already runs the full Agent OS stack stably, including tools, memory, config surfaces, and channel logic.
ESP focuses more on hardware and peripherals.

## Capabilities

- Run Beetle Agent OS on either ESP32-S3 or Linux.
- Use ESP32-S3 to sense and control real devices.
- Use Linux for fuller task execution, hardware orchestration, and complex integrations.
- Connect Feishu, DingTalk, WeCom, and QQ Channel to the same Agent OS.
- Enable `telegram` or `websocket` through optional Cargo features.
- Store summaries, long-term memory, reminders, tasks, and archive evidence on the device.
- Drive configured hardware through the `device_control` tool.
- Show Agent OS status on an SPI display.

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
BOARD=esp32-p4-nano-16mb ./build.sh --flash
./build.sh build-c6
./build.sh flash-c6
./build.sh flash-all
ESPFLASH_PORT=/dev/cu.usbserial-xxx ./build.sh --flash
ESP_HOSTED_C6_PORT=/dev/cu.usbserial-c6 ESPFLASH_PORT=/dev/cu.usbserial-p4 ./build.sh flash-all
```

If you do not set `BOARD` or `--target`, the ESP build scripts try `espflash board-info`
on the only detected serial port (or on `ESPFLASH_PORT` when it is set) and auto-select
one of these supported presets:

- `esp32-s3-8mb`
- `esp32-s3-16mb`
- `esp32-s3-32mb`
- `esp32-p4-nano-16mb`

If no supported board can be identified, or if multiple serial ports are present, set
`BOARD` manually.

Windows:

```powershell
.\build.ps1
.\build.ps1 --flash
$env:BOARD="esp32-s3-16mb"; .\build.ps1 --flash
$env:BOARD="esp32-p4-nano-16mb"; .\build.ps1 --flash
.\build.ps1 build-c6
.\build.ps1 flash-c6
.\build.ps1 flash-all
$env:ESPFLASH_PORT="COM3"; .\build.ps1 --flash
$env:ESP_HOSTED_C6_PORT="COM6"; $env:ESPFLASH_PORT="COM3"; .\build.ps1 flash-all
```

The same auto-detect rule applies on Windows: if `BOARD` and `--target` are both absent,
the script uses `espflash board-info` on the only detected COM port, or on
`ESPFLASH_PORT` if you set it explicitly.

For `ESP32-P4-NANO`, the board is only product-complete after both chips are flashed:

1. `flash-c6` flashes the board-mounted `ESP32-C6` hosted slave firmware
2. `BOARD=esp32-p4-nano-16mb ./build.sh --flash` flashes the Beetle main firmware on `ESP32-P4`
3. `flash-all` runs the full sequence in order

When flashing the on-board `ESP32-C6`, put the `ESP32-P4` into bootloader mode first so it does not interfere with the shared on-board wiring.
`build-c6` / `flash-c6` auto-detect both `espup` exports and standard official ESP-IDF installs (`IDF_PATH`, `~/.espressif/...`, `~/esp/...`) before invoking `idf.py`.

### 3. Open the config page

On first boot, the device opens a hotspot named **Beetle**.

1. Connect your phone or computer to that hotspot.
2. Open **http://192.168.4.1** in a browser.
3. Set the pairing code.
4. Configure WiFi, LLM, and the chat channel you want to use.

After the device joins your router, use its LAN IP for later access.

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
If `BOARD` and `--target` are both omitted for an ESP build, `build.sh` / `build.ps1`
first try `espflash board-info` auto-detection on the connected board and then fall back
to manual `BOARD` selection when the result is ambiguous or unsupported.

## Supported Boards

| BOARD | Flash | PSRAM | Notes |
|------|-------|-------|-------|
| `esp32-s3-8mb` | 8MB | 8MB | N8R8 |
| `esp32-s3-16mb` | 16MB | 8MB | Default preset |
| `esp32-s3-32mb` | 32MB | 16MB | N32R16 |
| `esp32-p4-nano-16mb` | 16MB | 32MB | Beetle runs on P4; hosted Wi-Fi runs on the board-mounted C6 |

Important:

- Use the project's partition table.
- `esp32-p4-nano-16mb` is a dual-chip board. Flash both the C6 hosted slave firmware and the P4 main firmware.
- If you see `spiffs partition could not be found`, the board preset or partition setup is wrong.

## Main Capabilities

| Area | What Beetle provides |
|------|----------------------|
| Channels | One Agent OS for multiple chat channels |
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
| Deploy or package Beetle on Linux | [docs/en-us/linux-release-rollback.md](docs/en-us/linux-release-rollback.md) |
| Call the HTTP API yourself | [docs/en-us/config-api.md](docs/en-us/config-api.md) |
| Understand what tools the agent can use | [docs/en-us/tools.md](docs/en-us/tools.md) |
| Configure LLM providers | [docs/en-us/llm-providers.md](docs/en-us/llm-providers.md) |
| Configure an SPI display | [docs/en-us/display.md](docs/en-us/display.md) |
| Configure hardware devices | [docs/en-us/hardware-device-config.md](docs/en-us/hardware-device-config.md) |
| Check board limits and troubleshooting | [docs/en-us/hardware.md](docs/en-us/hardware.md) |
| See the full docs map | [docs/README.md](docs/README.md) |

## Troubleshooting

- Flash fails: check the USB cable, port, and `ESPFLASH_PORT`.
- `flash-c6` fails: check the `PROG_C6` UART wiring, set `ESP_HOSTED_C6_PORT`, and put the P4 into bootloader mode first.
- Device is not reachable: reconnect to hotspot `Beetle` and open `http://192.168.4.1`.
- `spiffs partition could not be found`: use the project board preset and partition table.
- Need exact API behavior: see [docs/en-us/config-api.md](docs/en-us/config-api.md).
- Need board/resource details: see [docs/en-us/hardware.md](docs/en-us/hardware.md).
- Need Linux deployment/package notes: see [docs/en-us/linux-release-rollback.md](docs/en-us/linux-release-rollback.md).

## License

Beetle is dual-licensed under **MIT OR Apache-2.0**. See [LICENSE](LICENSE).
