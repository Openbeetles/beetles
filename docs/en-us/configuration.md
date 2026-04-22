# Configuration Overview

[中文](../zh-cn/configuration.md) | **English** | [Doc index](../README.md)

After Beetls OS is reachable, most day-to-day setup happens in the browser.
This page explains the setup areas and the order that usually works best.

## Setup Areas

| Area | What it controls |
|------|------------------|
| Pairing code | write protection for important operations |
| Network | how the device reaches the outside world |
| LLM | which model source is active |
| Chat channels | where conversations happen |
| Office accounts | mail, calendar, contacts, and documents |
| Hardware | devices and sensors |
| Display | screen output |
| Audio | input and output voice path |

## Recommended Order

For most installs, this order keeps setup simple:

1. pairing code
2. network
3. one LLM source
4. one chat channel
5. office accounts only if you need them
6. hardware, display, and audio after the base loop already works

If you need the exact first-time flow, go back to [getting-started-esp.md](getting-started-esp.md) or [getting-started-linux.md](getting-started-linux.md).

## Opening the Setup Page

There are two common entry points:

- on first use, connect to the default hotspot named **Beetle** and open `http://192.168.4.1`
- if the device is already on your local network, open its current local IP

## Pairing Code

The pairing code protects actions such as saving configuration, restarting Beetls OS, resetting settings, and starting online updates.

Set it once and keep it somewhere you can retrieve later.

## Common Problems

- Cannot open the setup page: make sure you are on the default **Beetle** hotspot or on the same local network
- The page opens but Beetls OS never replies: make sure both an LLM source and a chat channel are configured
- Saving fails: the pairing code is often wrong, or the page is stale, so reopen it and try again
- Office features do not appear: connect the related account first
- Hardware does not respond: check both wiring and saved hardware configuration

## Read Next

- To get the first setup working on ESP32: [getting-started-esp.md](getting-started-esp.md)
- To get the first setup working on Linux: [getting-started-linux.md](getting-started-linux.md)
- To see what Beetls OS can do after setup: [capabilities.md](capabilities.md)
- To configure model providers: [llm-providers.md](llm-providers.md)
- To configure hardware or a display: [hardware.md](hardware.md), [hardware-device-config.md](hardware-device-config.md), [display.md](display.md)
