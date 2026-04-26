<p align="center">
  <img src="configure-ui/public/logo.png" alt="Beetle OS" width="132" height="132" />
</p>

<h1 align="center">Beetle OS</h1>

<p align="center">
  <strong>A chat-first device agent for ESP32 and Linux</strong><br/>
  Rust · Chat channels · Workflows · Hardware control
</p>

<p align="center">
  <a href="README.zh-CN.md">中文</a> · <strong>English</strong>
</p>

Beetle OS is a chat-first agent system for ESP32 and Linux devices, combining browser setup, chat interaction, model access, work-account integrations, and hardware control.

## What Beetle OS Can Do

- reply through chat channels such as Feishu, DingTalk, WeCom, and QQ Channel
- handle reminders, tasks, and lightweight daily workflows
- connect mail, calendar, contacts, and documents when office accounts are configured
- read sensors and control devices on supported hardware
- expose a browser setup flow plus configuration and status APIs

## Common Product Shapes

| Shape | What Beetle OS contributes |
|-------|-------------------------|
| Desk companion | chat, reminders, documents, lightweight workflow help |
| Front-desk device | visitor Q&A, display, routing, operational assistance |
| Reminder terminal | recurring reminders, simple schedules, voice, notifications |
| Monitoring node | sensors, threshold watch, alert delivery |
| Device controller | GPIO, PWM, I2C, and simple actuation flows |

## Start Here

| If you want to... | Read this |
|-------------------|-----------|
| Get Beetle OS running on ESP32 | [docs/en-us/getting-started-esp.md](docs/en-us/getting-started-esp.md) |
| Get Beetle OS running on Linux | [docs/en-us/getting-started-linux.md](docs/en-us/getting-started-linux.md) |
| See what Beetle OS can do | [docs/en-us/capabilities.md](docs/en-us/capabilities.md) |
| Build, flash, or deploy from a terminal | [docs/en-us/build-script.md](docs/en-us/build-script.md) |
| Configure Beetle OS after it is reachable | [docs/en-us/configuration.md](docs/en-us/configuration.md) |
| Operate Beetle OS on Linux | [docs/en-us/linux-release-rollback.md](docs/en-us/linux-release-rollback.md) |
| Build your own frontend or integration | [docs/en-us/config-api.md](docs/en-us/config-api.md) |
| Browse the full docs portal | [docs/README.md](docs/README.md) |

## Supported Boards

| BOARD | Flash | PSRAM | Notes |
|------|-------|-------|-------|
| `esp32-s3-8mb` | 8MB | 8MB | N8R8 |
| `esp32-s3-16mb` | 16MB | 8MB | common default |
| `esp32-s3-32mb` | 32MB | 16MB | N32R16 |
| `esp32-p4-nano-16mb` | 16MB | 32MB | dual-chip board |

## Docs Structure

- `Start`: first-time setup on ESP32 or Linux
- `Capabilities`: what Beetle OS can do in real product shapes
- `Configure`: model, channels, hardware, display, and setup flow
- `Operate`: build, flash, deploy, restart, stop, rollback
- `Reference`: API details and tool surface
- `Develop`: architecture and extension points

## License

Beetle OS is dual-licensed under **MIT OR Apache-2.0**. See [LICENSE-MIT](LICENSE-MIT) and [LICENSE-APACHE](LICENSE-APACHE).
