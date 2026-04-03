# 硬件设备配置

[English](../en-us/hardware-device-config.md) | **中文** | [文档索引](../README.md)

`config/hardware.json` 的写法，以及 `device_control` 的生成方式如下。

处理流程：

- 你用 JSON 描述设备
- 固件负责校验这份 JSON
- Agent 看到的是设备名和语义说明，而不是引脚细节
- 真正的 GPIO / PWM / ADC / 蜂鸣器操作由程序执行

## 适用场景

当你希望 Agent 通过“设备名称”和“设备说明”控制硬件，而不是直接操作 GPIO 编号时，应使用 `hardware.json`。

适合：

- LED
- 继电器
- 蜂鸣器
- 简单 GPIO 输入
- PWM 输出
- ADC 读取

不适合：

- 强实时控制回路
- 高频连续采样
- 需要复杂协议栈的专用传感器驱动

## 配置结构

文件路径：

- `config/hardware.json`

顶层字段：

- `hardware_devices`

`hardware_devices` 里的每一项都代表一个设备：

| 字段 | 必填 | 含义 |
|------|------|------|
| `id` | 是 | 程序内唯一设备名 |
| `device_type` | 是 | `gpio_out`、`gpio_in`、`pwm_out`、`adc_in`、`buzzer` |
| `pins` | 是 | 引脚映射，目前是 `{"pin": <gpio>}` |
| `what` | 是 | 设备是什么、能做什么 |
| `how` | 是 | Agent 该怎么用它 |
| `options` | 否 | 设备相关附加选项，例如 PWM 频率 |

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

## Agent 实际看到什么

Agent **不会**直接看到引脚映射。

`device_control` 暴露给 Agent 的主要信息只有：

- `id`
- `what`
- `how`

这样做的好处是，模型使用层和底层硬件细节被隔开了，接口更安全，也更容易扩展。

## 支持的设备类型

| 类型 | 典型用途 | 输入 / 输出 |
|------|----------|-------------|
| `gpio_out` | LED、继电器 | 写入 `value` 0/1 |
| `gpio_in` | 按键、门磁、接触开关 | 读取 `value` 0/1 |
| `pwm_out` | 调光、调速 | 写入 `duty` 0-100 |
| `adc_in` | 模拟量传感器、分压检测 | 读取 `raw` 0-4095 |
| `buzzer` | 蜂鸣器提醒 | `duration_ms` 或 `beep: true` |

## 校验和限制

关键限制包括：

- 会屏蔽 strapping 引脚
- 设备总数有限制
- `pwm_out` 数量有限制
- 引脚不能跨设备冲突
- `adc_in` 只能用 ADC1 可用引脚

精确的读写契约和校验规则看 [config-api.md](config-api.md)。

## 程序行为

启动时，流程大致是这样：

1. 固件读取 `config/hardware.json`
2. 校验配置是否合法
3. 合法就注册 `device_control`
4. 不合法就不注册该工具

程序还有几条保护规则：

- 操作会有速率限制
- 每个设备都有自己的锁
- 设备忙时会直接返回忙碌错误，而不是无限排队

## 相关文档

- [config-api.md](config-api.md)：HTTP 读写和校验规则
- [tools.md](tools.md)：工具列表
- [hardware.md](hardware.md)：板型说明和排错入口
