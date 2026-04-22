# Hardware and Boards

**English** | [中文](../zh-cn/hardware.md) | [Doc index](../README.md)

Choose platform and hardware scope here.
This page stays practical and avoids low-level configuration detail.

## Quick Decisions

- Choose **ESP32-S3** if your main job is device control, sensors, and always-on edge behavior
- Choose **ESP32-P4** if you want a higher-end ESP path
- Choose **Linux** if you want broader integrations, longer-running work, and more host-side expansion

## Current Board Presets

| BOARD | Flash | PSRAM | Notes |
|------|-------|-------|------|
| `esp32-s3-8mb` | 8MB | 8MB | ESP32-S3 |
| `esp32-s3-16mb` | 16MB | 8MB | ESP32-S3 |
| `esp32-s3-32mb` | 32MB | 16MB | ESP32-S3 |
| `esp32-p4-nano-16mb` | 16MB | 32MB | ESP32-P4 NANO |

## Common Hardware Work In Beetls OS

- GPIO devices
- PWM devices
- analog input
- DHT sensors
- I2C devices and I2C sensors
- SPI displays

If you already know what hardware you want to connect, the next page is usually [hardware-device-config.md](hardware-device-config.md).

## Extra Direction On Linux

Linux also exposes hardware discovery.
The discovery path available today is mainly for USB-related devices such as:

- audio input
- audio output
- camera
- serial
- HID

## When To Read Which Hardware Page

- Still choosing a platform or board: stay on this page
- Ready to describe devices and wiring: read [hardware-device-config.md](hardware-device-config.md)
- Adding a screen: read [display.md](display.md)

## Common Problems

- `spiffs partition could not be found`
  Usually means the wrong board preset or partition table was used.
- Beetls OS starts but hardware features do not appear
  Check that the related config exists and that the hardware is actually attached.
- The screen turns on but looks wrong
  Go straight to [display.md](display.md).

## Read Next

- To define hardware and sensor config: [hardware-device-config.md](hardware-device-config.md)
- To set up a screen: [display.md](display.md)
- To see how those capabilities appear at runtime: [tools.md](tools.md)
