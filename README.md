<p align="center">
  <img src="configure-ui/public/logo.png" alt="Beetle" width="132" height="132" />
</p>

<h1 align="center">Beetle</h1>

<p align="center">
  <strong>A chat-first device agent for ESP32 and Linux</strong><br/>
  Rust · Chat channels · Daily work · Hardware control
</p>

<p align="center">
  <a href="README.zh-CN.md">中文</a> · <strong>English</strong>
</p>

Beetle is a device agent you can talk to from chat and manage from a browser.
It can reply, handle reminders and tasks, connect work accounts, and control hardware on supported devices.

Some abilities depend on your device, hardware, and setup.

## What Beetle Can Do

- talk to you through supported chat channels
- handle reminders, tasks, and simple daily work
- connect mail, calendar, documents, and contacts when those accounts are set up
- read sensors and control devices on supported boards
- offer a browser-based setup page and status API

Common chat channels include Feishu, DingTalk, WeCom, and QQ Channel.

## Where It Runs

| Target | Good fit |
|--------|----------|
| ESP32-S3 | device control, sensors, and always-on edge use |
| ESP32-P4 | more demanding board-side work |
| Linux | longer-running tasks, work-account integrations, and broader expansion |

## Quick Start

### 1. Flash or deploy Beetle

Most ESP users start with:

```bash
./build.sh --flash
```

Common board-specific examples:

```bash
BOARD=esp32-s3-16mb ./build.sh --flash
BOARD=esp32-p4-nano-16mb ./build.sh --flash
./build.sh flash-all
```

If you are deploying on Linux, go straight to [docs/en-us/linux-release-rollback.md](docs/en-us/linux-release-rollback.md).

### 2. Open the setup page

On first use, Beetle usually exposes a hotspot named **Beetle**.
Connect to it and open **http://192.168.4.1**.

If the device is already on your local network, open its local IP instead.

### 3. Finish the minimum setup

Set these first:

1. pairing code
2. network
3. one LLM source
4. one chat channel

After that, you can add work accounts, hardware, display, or audio if needed.

## Supported Boards

| BOARD | Flash | PSRAM | Notes |
|------|-------|-------|-------|
| `esp32-s3-8mb` | 8MB | 8MB | N8R8 |
| `esp32-s3-16mb` | 16MB | 8MB | common default |
| `esp32-s3-32mb` | 32MB | 16MB | N32R16 |
| `esp32-p4-nano-16mb` | 16MB | 32MB | dual-chip board |

## Read Next

| If you want to... | Read this |
|-------------------|-----------|
| Set Beetle up for the first time | [docs/en-us/configuration.md](docs/en-us/configuration.md) |
| See what Beetle can help with | [docs/en-us/tools.md](docs/en-us/tools.md) |
| Connect a model provider | [docs/en-us/llm-providers.md](docs/en-us/llm-providers.md) |
| Build, flash, or deploy from a terminal | [docs/en-us/build-script.md](docs/en-us/build-script.md) |
| Set up hardware or a display | [docs/en-us/hardware.md](docs/en-us/hardware.md), [docs/en-us/hardware-device-config.md](docs/en-us/hardware-device-config.md), [docs/en-us/display.md](docs/en-us/display.md) |
| Build your own frontend or script | [docs/en-us/config-api.md](docs/en-us/config-api.md) |
| Browse the full docs set | [docs/README.md](docs/README.md) |

## License

Beetle is dual-licensed under **MIT OR Apache-2.0**. See [LICENSE-MIT](LICENSE-MIT) and [LICENSE-APACHE](LICENSE-APACHE).
