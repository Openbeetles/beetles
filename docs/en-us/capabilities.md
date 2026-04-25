# What Beetle OS Can Do

[中文](../zh-cn/capabilities.md) | **English** | [Doc index](../README.md)

Beetle OS is not just a chat bot and not just a hardware runtime.
It sits in the middle of chat, browser setup, model access, work accounts, and device control.

## Typical Product Shapes

| Product shape | What Beetle OS contributes |
|---------------|-------------------------|
| Desk companion | chat, reminders, documents, lightweight workflow help |
| Front-desk device | visitor Q&A, display, routing, operational assistance |
| Reminder terminal | recurring reminders, simple schedules, voice, notifications |
| Monitoring node | sensors, threshold watch, alert delivery |
| Device controller | GPIO, PWM, I2C, and simple actuation flows |

## Capability Areas

### Chat and Tasking

Beetle OS can receive input through supported chat channels, answer, send visible messages, manage reminders, and keep lightweight task flows moving.

### Work and Office

When office accounts are connected, Beetle OS can work with mail, calendars, contacts, and documents.
That is the path for office assistants, front-desk devices, and internal workflow helpers.

### Hardware and Automation

Beetle OS can control configured devices, read sensors, and watch thresholds.
That is the path for monitoring nodes, hardware controllers, and hybrid software-plus-device assistants.

### Voice and Display

With the right audio and display setup, Beetle OS can speak, listen, and surface status on a screen.
That is the path for tabletop devices, reminder terminals, and dedicated interaction hardware.

### Operations and Diagnostics

Beetle OS includes system status, diagnostics, scheduled work, and Linux release management surfaces.
That matters when Beetle OS is deployed as a long-running device or host service.

## Platform Fit

| Platform | Best fit |
|----------|----------|
| ESP32-S3 | always-on edge devices, sensors, and hardware control |
| ESP32-P4 | more demanding board-side flows |
| Linux | broader integrations, longer-running work, and host-side expansion |

## Where To Go Next

- To get Beetle OS running on ESP32: [getting-started-esp.md](getting-started-esp.md)
- To get Beetle OS running on Linux: [getting-started-linux.md](getting-started-linux.md)
- To configure setup areas and model providers: [configuration.md](configuration.md), [llm-providers.md](llm-providers.md)
- To inspect the exact tool surface behind these capabilities: [tools.md](tools.md)
