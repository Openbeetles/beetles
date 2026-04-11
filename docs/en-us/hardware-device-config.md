# Hardware Device Configuration

**English** | [中文](../zh-cn/hardware-device-config.md) | [Doc index](../README.md)

This page explains how `config/hardware.json` becomes the `device_control` tool.

This page is mainly for people wiring hardware, writing config, or building integrations.

The flow is simple:

- you describe devices in JSON
- the firmware validates that JSON
- the agent sees device names and descriptions, not raw pin maps
- Beetle executes the actual GPIO / PWM / ADC / buzzer operations

## When To Use This

Use `hardware.json` when you want the agent to operate hardware by meaning, not by pin number.

Good fit:

- LEDs
- relays
- buzzers
- simple GPIO inputs
- PWM-controlled outputs
- ADC-based reads

Not a good fit:

- strict real-time loops
- high-rate continuous sampling
- chip-specific sensor drivers that need a richer protocol layer

## Config Shape

File path:

- `config/hardware.json`

Top-level key:

- `hardware_devices`

Each item in `hardware_devices` describes one device:

| Field | Required | Meaning |
|-------|----------|---------|
| `id` | Yes | Unique device name |
| `device_type` | Yes | `gpio_out`, `gpio_in`, `pwm_out`, `adc_in`, or `buzzer` |
| `pins` | Yes | Pin mapping, currently `{"pin": <gpio>}` |
| `what` | Yes | What the device is and what it does |
| `how` | Yes | How the agent should use it |
| `options` | No | Device-specific options such as PWM frequency |

## Example

```json
{
  "hardware_devices": [
    {
      "id": "onboard_led",
      "device_type": "gpio_out",
      "pins": { "pin": 2 },
      "what": "Onboard LED indicator",
      "how": "Pass value: 1=on, 0=off"
    },
    {
      "id": "desk_lamp",
      "device_type": "pwm_out",
      "pins": { "pin": 15 },
      "what": "Dimmable LED lamp",
      "how": "Pass duty: 0-100 for brightness percent",
      "options": { "frequency_hz": 5000 }
    }
  ]
}
```

## What The Agent Sees

The agent does **not** get raw pin mappings.

Instead, `device_control` is built from:

- `id`
- `what`
- `how`

That gives the model a safer, more semantic interface.

## Device Types

| Type | Typical use | Input / output |
|------|-------------|----------------|
| `gpio_out` | LED, relay | write `value` 0/1 |
| `gpio_in` | switch, contact sensor | read `value` 0/1 |
| `pwm_out` | dimming, fan speed | write `duty` 0-100 |
| `adc_in` | analog sensor, divider | read `raw` 0-4095 |
| `buzzer` | alert/beep output | `duration_ms` or `beep: true` |

## Validation and Limits

Important limits:

- strapping pins are blocked
- device count is capped
- `pwm_out` count is capped
- pins cannot collide across devices
- `adc_in` is restricted to ADC1-capable pins

The exact read/write contract is documented in [config-api.md](config-api.md).

## Behavior On Boot And During Use

On boot:

1. the firmware reads `config/hardware.json`
2. it validates the file
3. if valid, it registers `device_control`
4. if invalid, the tool is not registered

During use:

- operations are rate-limited
- each device has its own lock
- busy devices return a busy-style error instead of queueing indefinitely

## Related Docs

- [config-api.md](config-api.md) for HTTP read/write behavior
- [tools.md](tools.md) for the tool list
- [hardware.md](hardware.md) for board and troubleshooting notes
