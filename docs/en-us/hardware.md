# Hardware and Boards

**English** | [中文](../zh-cn/hardware.md) | [Doc index](../README.md)

This page keeps to the practical facts you need when choosing hardware.

## Current board presets

The repo currently ships these board presets:

| BOARD | Flash | PSRAM | Notes |
|------|-------|-------|------|
| `esp32-s3-8mb` | 8MB | 8MB | ESP32-S3 |
| `esp32-s3-16mb` | 16MB | 8MB | ESP32-S3 |
| `esp32-s3-32mb` | 32MB | 16MB | ESP32-S3 |
| `esp32-p4-nano-16mb` | 16MB | 32MB | ESP32-P4 NANO |

## How to choose

- If your main goal is device control and sensors, choose ESP32-S3
- If you want a higher-end ESP board path, look at ESP32-P4
- If you want broader expansion and longer-running work, choose Linux

## Common hardware work in Beetle

- GPIO devices
- PWM devices
- analog input
- DHT
- I2C devices and I2C sensors
- SPI displays

Read these next:

- hardware control config: [hardware-device-config.md](hardware-device-config.md)
- display setup: [display.md](display.md)

## Extra direction on Linux

Linux also exposes hardware discovery.
Right now the public discovery path is for USB, with common categories such as:

- audio input
- audio output
- camera
- serial
- HID

## Where to check first when something is wrong

- the config UI: check whether the device was found and whether settings were saved
- [config-api.md](config-api.md): only when you need to inspect the API directly
- serial or service logs: for boot failure, config failure, or hardware init failure

## Common problems

- `spiffs partition could not be found`
  This usually means the wrong board preset or partition table was used.
- Beetle starts but hardware features do not appear
  Check that the related config is present and the hardware is actually attached.
- The screen turns on but looks wrong
  Go straight to [display.md](display.md).
