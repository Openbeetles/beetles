# Hardware Configuration

**English** | [中文](../zh-cn/hardware-device-config.md) | [Doc index](../README.md)

This page explains the saved hardware configuration Beetle OS reads after you connect devices.
It is a field guide, not a wiring tutorial.

## The Short Path

Most hardware setups follow this pattern:

1. define direct-use devices under `hardware_devices`
2. define `i2c_bus` only if you actually use I2C
3. add `i2c_devices` or `i2c_sensors` only when needed
4. save the config
5. verify the capability appears in Beetle OS

Saved data ends up in `config/hardware.json`.

## What Lives In This Config

The current structure is:

- `hardware_devices`
- `i2c_bus`
- `i2c_devices`
- `i2c_sensors`

You do not need to use every section.

## What `hardware_devices` Is For

This section defines devices Beetle OS can use directly.
You describe the device name, purpose, and wiring here.

Current `device_type` values:

- `gpio_out`
- `gpio_in`
- `pwm_out`
- `adc_in`
- `buzzer`
- `dht`

Each device uses these core fields:

| Field | Meaning |
|-------|---------|
| `id` | unique device name |
| `device_type` | device type |
| `pins` | wiring info |
| `what` | what the device is |
| `how` | how Beetle OS should use it |
| `options` | extra options, depending on the device |

## `i2c_bus`, `i2c_devices`, And `i2c_sensors`

If you use I2C, the same file can also define:

- `i2c_bus`: the bus itself
- `i2c_devices`: general I2C devices
- `i2c_sensors`: I2C sensors

The most-used fields are:

| Section | Common fields |
|---------|---------------|
| `i2c_bus` | `sda_pin`, `scl_pin`, `freq_hz` |
| `i2c_devices` | `id`, `addr`, `what`, `how`, `options` |
| `i2c_sensors` | `id`, `addr`, `model`, `watch_field`, `what`, `how`, `options` |

Current `i2c_sensors.model` values are:

- `sht3x`
- `aht20`
- `raw`

If you use `raw`, you need its extra read and write options.
If you use `dht`, common `options.model` values are `dht11`, `dht22`, or `dht21`.

## Example Saved Shape

```json
{
  "hardware_devices": [
    {
      "id": "onboard_led",
      "device_type": "gpio_out",
      "pins": { "pin": 2 },
      "what": "Onboard indicator LED",
      "how": "Use value 1 for on and 0 for off"
    },
    {
      "id": "room_dht",
      "device_type": "dht",
      "pins": { "pin": 4 },
      "what": "Room temperature and humidity sensor",
      "how": "Read temperature and humidity",
      "options": { "model": "dht22", "watch_field": "temperature" }
    }
  ],
  "i2c_bus": {
    "sda_pin": 8,
    "scl_pin": 9,
    "freq_hz": 400000
  },
  "i2c_sensors": [
    {
      "id": "desk_temp",
      "addr": 68,
      "model": "aht20",
      "watch_field": "temperature",
      "what": "Desk temperature and humidity sensor",
      "how": "Read temperature and humidity"
    }
  ]
}
```

## What Happens After Save

- `hardware_devices` tells Beetle OS which devices it can use directly
- `i2c_devices` tells Beetle OS which I2C devices it can access
- `i2c_sensors` tells Beetle OS which I2C sensors it can read and monitor

If the config is invalid, those abilities do not appear normally.

## Real Limits Worth Knowing

- `hardware_devices`: up to 8 items
- `pwm_out`: up to 4 items
- one pin cannot be reused by multiple devices
- `adc_in` must use allowed ADC pins
- `i2c_sensors.id` cannot clash with `hardware_devices.id`

## Read Next

- To choose boards and hardware direction first: [hardware.md](hardware.md)
- To understand how those abilities appear at runtime: [tools.md](tools.md)
- To write config through the API: [config-api.md](config-api.md)
