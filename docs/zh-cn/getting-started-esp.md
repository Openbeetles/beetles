# ESP32 首次部署

[English](../en-us/getting-started-esp.md) | **中文** | [文档索引](../README.md)

本文说明 Beetle OS 在 ESP32 上从构建、烧录到完成首次消息联通的最小流程。

## 前置条件

- 一块支持的板子
- 一根用于烧录的 USB 线
- 一个后续可接入的网络
- 一个大模型服务账号，或本地模型地址
- 一个准备先测试的聊天通道

## 1. 构建与烧录

常用命令如下：

```bash
./build.sh --flash
```

常见板型示例：

```bash
BOARD=esp32-s3-16mb ./build.sh --flash
BOARD=esp32-p4-nano-16mb ./build.sh --flash
./build.sh flash-all
```

完整参数说明参见 [build-script.md](build-script.md)。

## 2. 打开配置页

首次启动时，设备默认会提供名为 **Beetle** 的热点。

1. 先连上这个热点
2. 打开在线配置页、桌面壳或本地构建出的 Configure UI
3. 在 **「设备地址」** 中填写 `http://192.168.4.1`

如果设备已经进了局域网，就把它当前的局域网地址填为 **「设备地址」**。

## 3. 完成最小配置

建议按以下顺序完成基础配置：

1. 配对码
2. 网络
3. 一个大模型来源
4. 一个聊天通道

硬件、显示、音频和办公账号可在基础链路验证完成后补充。

## 4. 验证消息链路

配好第一个模型和第一个聊天通道之后：

1. 打开你选的聊天通道
2. 通过该聊天通道发送一条测试消息
3. 确认系统返回一次有效回复

该步骤完成后，即表示基础链路已建立。

## 如果没有收到回复

- 检查网络配置是否保存成功
- 检查模型来源是否可用、可连通
- 检查聊天通道配置是否完整
- 重新打开配置页，确认保存时配对码输入正确

## 相关文档

- 配置范围与顺序：[configuration.md](configuration.md)
- 大模型配置：[llm-providers.md](llm-providers.md)
- 能力概览：[capabilities.md](capabilities.md)
- 硬件与显示配置：[hardware.md](hardware.md)、[hardware-device-config.md](hardware-device-config.md)、[display.md](display.md)
