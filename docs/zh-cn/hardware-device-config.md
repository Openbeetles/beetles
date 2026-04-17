# 硬件配置

[English](../en-us/hardware-device-config.md) | **中文** | [文档索引](../README.md)

正常使用时，优先在配置页里添加和修改硬件。

这页只说明硬件配置现在有哪些字段，方便你看懂配置页、接口返回，或者排查保存结果。
保存之后，这些内容会落到 `config/hardware.json`。

它现在不只管简单 GPIO，也能放 I2C 设备和 I2C 传感器。

## 这组配置里有什么

当前结构是：

- `hardware_devices`
- `i2c_bus`
- `i2c_devices`
- `i2c_sensors`

你可以只用其中一部分，不需要一次全配。

## `hardware_devices` 是做什么的

这部分用来定义 Beetle 可以直接使用的设备。
你要写清楚设备名、用途和接线。

当前支持的 `device_type`：

- `gpio_out`
- `gpio_in`
- `pwm_out`
- `adc_in`
- `buzzer`
- `dht`

每个设备都有这些核心字段：

| 字段 | 说明 |
|------|------|
| `id` | 设备名，必须唯一 |
| `device_type` | 设备类型 |
| `pins` | 接线信息 |
| `what` | 这个设备是什么 |
| `how` | 希望 Beetle 怎么使用它 |
| `options` | 额外选项，不同设备可不同 |

## `i2c_bus`、`i2c_devices`、`i2c_sensors`

如果你要用 I2C，还可以在同一个文件里继续写：

- `i2c_bus`：I2C 总线本身
- `i2c_devices`：普通 I2C 设备
- `i2c_sensors`：I2C 传感器

最常用字段如下：

| 区块 | 常用字段 |
|------|----------|
| `i2c_bus` | `sda_pin`、`scl_pin`、`freq_hz` |
| `i2c_devices` | `id`、`addr`、`what`、`how`、`options` |
| `i2c_sensors` | `id`、`addr`、`model`、`watch_field`、`what`、`how`、`options` |

`i2c_sensors` 当前支持这些 `model`：

- `sht3x`
- `aht20`
- `raw`

如果你用 `raw`，需要额外填写它自己的读写参数。
如果你用 `dht`，常见 `options.model` 是 `dht11`、`dht22` 或 `dht21`。

## 如果你查看底层数据，样子大致是这样

```json
{
  "hardware_devices": [
    {
      "id": "onboard_led",
      "device_type": "gpio_out",
      "pins": { "pin": 2 },
      "what": "板载指示灯",
      "how": "传 value 1 表示开，0 表示关"
    },
    {
      "id": "room_dht",
      "device_type": "dht",
      "pins": { "pin": 4 },
      "what": "室内温湿度传感器",
      "how": "读取温度和湿度",
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
      "what": "桌面温湿度传感器",
      "how": "读取温度和湿度"
    }
  ]
}
```

## 配好之后会发生什么

- `hardware_devices` 让 Beetle 知道有哪些设备可以直接使用
- `i2c_devices` 让 Beetle 知道有哪些 I2C 设备可以访问
- `i2c_sensors` 让 Beetle 知道有哪些 I2C 传感器可以读取和监控

如果配置不合法，这些能力不会正常出现。

## 需要注意的真实限制

- `hardware_devices` 最多 8 个
- `pwm_out` 最多 4 个
- 同一个引脚不能被多个设备重复使用
- `adc_in` 只能用允许的 ADC 引脚
- `i2c_sensors` 的 `id` 不能和 `hardware_devices` 冲突

## 接下来读什么

- 想直接在页面里配置： [configuration.md](configuration.md)
- 想看 Beetle 会怎么用这些能力： [tools.md](tools.md)
- 想自己通过接口写配置： [config-api.md](config-api.md)
- 想看板型和硬件方向： [hardware.md](hardware.md)
