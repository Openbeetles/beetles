# Hardware Device Setup

**English** | [中文](../zh-cn/hardware-device-config.md) | [Doc index](../README.md)

This page is for end users configuring hardware in **Configure UI**.
If you need API details for scripts or a custom frontend, use [config-api.md](config-api.md) instead.

## Where To Configure It

Use **Configure UI** as the normal setup path.

Typical flow:

1. Open Configure UI and connect to the target device
2. Unlock with the pairing code
3. Go to **Device Config**
4. Open **GPIO Devices** for GPIO / DHT / PWM and similar devices
5. Open **I2C Sensors** for AHT20 / SHT3x / raw I2C sensors
6. Add or edit the device / sensor entries you need
7. Save, then restart the device if the page asks for it

If you are still on first-time access, start with [configuration.md](configuration.md).

## What This Page Covers

Hardware setup in Configure UI now covers:

- **GPIO Devices**: onboard or attached programmable hardware devices
- **I2C Sensors**: `i2c_bus` plus common I2C sensors

The device types you can add on **GPIO Devices** today are:

- `gpio_out`
- `gpio_in`
- `pwm_out`
- `adc_in`
- `buzzer`
- `dht`

If your goal is to drive LEDs, relays, switches, buzzers, or read DHT sensors, this is the page you should use first.

The **I2C Sensors** page currently supports:

- `aht20`
- `sht3x`
- `raw`

AHT20 commonly uses address `56` (`0x38`). Fill SDA / SCL from your actual wiring; frequency `100000` is a common default.

## Recommended Usage

- Give every device a clear `Device ID`
- Make sure the GPIO pin matches the real wiring
- Write `what` and `how` in plain language so Beetle can use the device correctly later
- If the page says restart is required, restart the device right away

## What Still Requires The API

Most users do not need this.

If you need to configure advanced hardware fields such as:

- `i2c_devices`
- importing an existing hardware config in bulk

then use the `GET/POST /api/config/hardware` section in [config-api.md](config-api.md).

In short:

- normal user setup: use Configure UI first
- advanced integration or scripted setup: use the API docs

## How To Confirm It Worked

- the page saves successfully
- you restart if prompted
- the device or sensor becomes usable from the device page or related feature page

If it still does not work after save, check:

- wrong pin or wiring
- wrong device type
- missing I2C bus settings required by the sensor

## Read Next

- Configuration overview: [configuration.md](configuration.md)
- Boards and hardware scope: [hardware.md](hardware.md)
- API reference: [config-api.md](config-api.md)
- Display configuration: [display.md](display.md)
