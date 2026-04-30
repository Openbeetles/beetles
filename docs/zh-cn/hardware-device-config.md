# 硬件设备配置

[English](../en-us/hardware-device-config.md) | **中文** | [文档索引](../README.md)

本页讲的是用户应该怎么在 **Configure UI** 里配置硬件。
如果你是要写脚本、自定义前端，或者要看接口细节，再去看 [config-api.md](config-api.md)。

## 配置入口

硬件配置请直接在 **Configure UI** 里完成。

常见路径：

1. 打开 Configure UI，并连到目标设备
2. 完成配对码解锁
3. 进入 **设备配置**
4. 配 GPIO / DHT / PWM 等设备时打开 **GPIO 设备**
5. 配 AHT20 / SHT3x / raw I2C 传感器时打开 **I2C 传感器**
6. 添加或修改设备 / 传感器
7. 点击保存，并按提示重启设备

首次连设备、地址怎么填，先看 [configuration.md](configuration.md)。

## 这个页面能配什么

当前 Configure UI 的硬件配置覆盖两类内容：

- **GPIO 设备**：可直接控制或读取的板载硬件设备
- **I2C 传感器**：`i2c_bus` 与常见 I2C 传感器

**GPIO 设备** 页面里当前可直接添加的设备类型包括：

- `gpio_out`
- `gpio_in`
- `pwm_out`
- `adc_in`
- `buzzer`
- `dht`

如果你只是想让 Beetle 控灯、读开关、驱动蜂鸣器、读取 DHT，正常就都在这里配。

**I2C 传感器** 页面当前支持：

- `aht20`
- `sht3x`
- `raw`

AHT20 常见地址是 `56`（`0x38`）；SDA / SCL 请按实际接线填写，频率通常可用 `100000`。

## 推荐操作方式

- 一个设备填一条记录，先把 `设备 ID` 起清楚
- 引脚号务必和实际接线一致
- `是什么`、`怎么用` 这两个说明要写成人能看懂的话，方便后续模型和工具正确使用
- 保存后如果页面提示要重启，就直接重启设备

## 当前 UI 还没完全覆盖的部分

大多数用户不需要管这个。

如果你确实要配置下面这些进阶字段：

- `i2c_devices`
- 需要批量导入已有硬件配置

那就看 [config-api.md](config-api.md) 里的 `GET/POST /api/config/hardware`。

也就是说：

- 日常用户配置，先走 Configure UI
- 进阶集成或脚本化配置，再看 API

## 保存后怎么确认

- 页面保存成功
- 如有提示，完成一次重启
- 回到设备页或相关能力页，确认设备已经能被调用或读到数据

如果保存后还是没反应，先排查三件事：

- 引脚或接线填错
- 设备类型选错
- I2C 传感器需要的总线参数还没配置

## 相关文档

- 配置总览：[configuration.md](configuration.md)
- 平台与板型范围：[hardware.md](hardware.md)
- 配置接口参考：[config-api.md](config-api.md)
- 显示配置：[display.md](display.md)
