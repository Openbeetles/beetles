# 硬件配置

[English](../en-us/hardware-device-config.md) | **中文** | [文档索引](../README.md)

本页说明 Beetls OS 读取的硬件配置文件结构。
内容聚焦字段定义与约束，不涉及接线教程。

## 基本流程

硬件配置通常按以下顺序完成：

1. 在 `hardware_devices` 中定义运行时直接使用的设备
2. 只有用到 I2C 时才配置 `i2c_bus`
3. 需要时再补 `i2c_devices` 或 `i2c_sensors`
4. 保存配置
5. 保存后确认相关能力已在运行时出现

保存后的数据会落到 `config/hardware.json`。

## 配置结构

当前结构是：

- `hardware_devices`
- `i2c_bus`
- `i2c_devices`
- `i2c_sensors`

无需一次性启用全部区块。

## `hardware_devices`

该区块用于定义运行时可以直接调用的设备，包括设备标识、用途与接线信息。

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
| `what` | 设备用途说明 |
| `how` | 使用方式说明 |
| `options` | 额外选项，不同设备可不同 |

## `i2c_bus`、`i2c_devices`、`i2c_sensors`

如果你要用 I2C，还可以继续定义：

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

如果你用 `raw`，还要补它自己的读写选项。
如果你用 `dht`，常见 `options.model` 是 `dht11`、`dht22` 或 `dht21`。

## 示例

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

## 生效结果

- `hardware_devices` 定义可直接调用的设备
- `i2c_devices` 定义可访问的 I2C 设备
- `i2c_sensors` 定义可读取和监控的 I2C 传感器

如果配置不合法，这些能力不会正常出现。

## 限制

- `hardware_devices` 最多 8 个
- `pwm_out` 最多 4 个
- 同一个引脚不能被多个设备重复使用
- `adc_in` 只能用允许的 ADC 引脚
- `i2c_sensors.id` 不能和 `hardware_devices.id` 冲突

## 相关文档

- 板型与硬件范围：[hardware.md](hardware.md)
- 运行时工具能力：[tools.md](tools.md)
- 配置接口参考：[config-api.md](config-api.md)
