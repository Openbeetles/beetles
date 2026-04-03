# 硬件设备配置

[English](../en-us/hardware-device-config.md) | **中文** | [文档索引](../README.md)

这页说明 `config/hardware.json` 应该怎么写，以及程序会怎样根据这份配置生成 `device_control`。

思路其实很简单：

- 你先用 JSON 把设备写清楚
- 程序启动时会检查这份配置能不能用
- 模型看到的是“设备名 + 设备说明”
- 真正的 GPIO、PWM、ADC、蜂鸣器操作由底层代码执行

## 适用场景

如果你希望 Beetle 通过“设备名称”和“设备说明”来控制硬件，而不是把引脚号直接暴露给模型，就应该用 `hardware.json`。

适合：

- LED
- 继电器
- 蜂鸣器
- 普通输入开关
- PWM 调光或调速
- 模拟量读取

不适合：

- 强实时控制
- 高频连续采样
- 需要专用驱动和复杂协议的设备

## 配置结构

文件路径是 `config/hardware.json`。

`hardware_devices` 里的每一项都代表一个设备：

| 字段 | 必填 | 含义 |
|------|------|------|
| `id` | 是 | 设备名，必须唯一 |
| `device_type` | 是 | `gpio_out`、`gpio_in`、`pwm_out`、`adc_in`、`buzzer` |
| `pins` | 是 | 引脚映射，目前是 `{"pin": <gpio>}` |
| `what` | 是 | 这个设备是什么，有什么用途 |
| `how` | 是 | 应该怎样调用它 |
| `options` | 否 | 额外选项，比如 PWM 频率 |

## 示例

```json
{
  "hardware_devices": [
    {
      "id": "onboard_led",
      "device_type": "gpio_out",
      "pins": { "pin": 2 },
      "what": "板载 LED 指示灯",
      "how": "传 value: 1=亮, 0=灭"
    },
    {
      "id": "desk_lamp",
      "device_type": "pwm_out",
      "pins": { "pin": 15 },
      "what": "可调光灯",
      "how": "传 duty: 0-100 表示亮度百分比",
      "options": { "frequency_hz": 5000 }
    }
  ]
}
```

## 模型实际看到什么

模型不会直接看到引脚映射。

`device_control` 暴露出去的重点只有：

- `id`
- `what`
- `how`

这样做的目的很明确：让模型按“设备能力”来理解硬件，而不是按引脚号来猜。

## 支持的设备类型

| 类型 | 典型用途 | 输入 / 输出 |
|------|----------|-------------|
| `gpio_out` | LED、继电器 | 写入 `value` 0/1 |
| `gpio_in` | 按键、门磁、接触开关 | 读取 `value` 0/1 |
| `pwm_out` | 调光、调速 | 写入 `duty` 0-100 |
| `adc_in` | 模拟量传感器、分压检测 | 读取 `raw` 0-4095 |
| `buzzer` | 蜂鸣器提醒 | `duration_ms` 或 `beep: true` |

## 校验和限制

程序会在启动或保存配置时做检查。比较重要的规则有这些：

- 会屏蔽 strapping 引脚
- 设备总数有限制
- `pwm_out` 数量有限制
- 引脚不能跨设备冲突
- `adc_in` 只能用 ADC1 可用引脚

更完整的读写规则和接口格式，见 [config-api.md](config-api.md)。

## 程序行为

启动时，大致会这样处理：

1. 固件读取 `config/hardware.json`
2. 校验配置是否合法
3. 合法就注册 `device_control`
4. 不合法就不注册该工具

另外还有几条保护规则：

- 操作会有速率限制
- 每个设备都有自己的锁
- 设备忙时会直接返回忙碌错误，而不是无限排队

## 相关文档

- [config-api.md](config-api.md)：HTTP 读写和校验规则
- [tools.md](tools.md)：工具列表
- [hardware.md](hardware.md)：板型说明和排错入口
