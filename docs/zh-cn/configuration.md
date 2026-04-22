# 配置总览

[English](../en-us/configuration.md) | **中文** | [文档索引](../README.md)

本页说明 Beetls OS 的配置范围、访问方式与推荐配置顺序。

## 配置区块

| 区块 | 作用 |
|------|------|
| 配对码 | 给重要写操作加保护 |
| 网络 | 设备怎么连到外部世界 |
| 大模型 | 当前用哪个模型来源 |
| 聊天通道 | 你从哪里和设备交互 |
| 办公账号 | 邮箱、日历、联系人和文档 |
| 硬件 | 设备和传感器 |
| 显示 | 屏幕输出 |
| 音频 | 语音输入输出 |

## 推荐顺序

建议按以下顺序完成基础配置：

1. 配对码
2. 网络
3. 一个大模型来源
4. 一个聊天通道
5. 如需办公能力，再配置办公账号
6. 基础链路验证完成后，再配置硬件、显示和音频

首次接入流程参见 [getting-started-esp.md](getting-started-esp.md) 或 [getting-started-linux.md](getting-started-linux.md)。

## 打开配置页

配置页通常通过以下地址访问：

- 第一次使用时，连接默认热点 **Beetle**，打开 `http://192.168.4.1`
- 如果设备已经在局域网里，直接打开它当前的局域网地址

## 配对码

配对码保护的是保存配置、重启、恢复设置和在线升级这类操作。

设置完成后，后续受保护写操作均需提供该配对码。

## 常见问题

- 无法访问配置页：确认当前终端连接的是设备热点，或与设备处于同一局域网
- 页面可访问但系统无回复：确认大模型来源和聊天通道均已完成配置
- 保存失败：通常是配对码错误或页面状态过期，重新加载页面后重试
- 未显示办公能力：确认相关办公账号已经接入
- 硬件无响应：确认硬件配置已保存，且接线与配置一致

## 相关文档

- ESP32 首次部署：[getting-started-esp.md](getting-started-esp.md)
- Linux 首次部署：[getting-started-linux.md](getting-started-linux.md)
- 能力概览：[capabilities.md](capabilities.md)
- 大模型配置：[llm-providers.md](llm-providers.md)
- 硬件与显示配置：[hardware.md](hardware.md)、[hardware-device-config.md](hardware-device-config.md)、[display.md](display.md)
