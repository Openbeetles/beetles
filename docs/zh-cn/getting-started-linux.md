# Linux 首次部署

[English](../en-us/getting-started-linux.md) | **中文** | [文档索引](../README.md)

本文说明 Beetls OS 在 Linux 上从构建、部署到完成首次消息联通的最小流程。

## 前置条件

- 一台 Linux 目标主机
- 如果构建和部署不在同一台机器上，要有 SSH 访问
- 一个大模型服务账号，或本地模型地址
- 一个准备先测试的聊天通道

## 1. 构建 Linux 产物

常用命令如下：

```bash
TARGET=linux ./build.sh
TARGET=linux-armv7 ./build.sh
TARGET=linux-aarch64 ./build.sh
```

如已有构建产物，可直接进入部署步骤。

## 2. 部署 Beetls OS

标准部署入口是：

```bash
./build.sh --deploy-linux
```

Linux 发布目录、重启方式和回滚流程参见 [linux-release-rollback.md](linux-release-rollback.md)。

## 3. 打开配置页

部署完成后，通过目标主机地址打开 Beetls OS。

浏览器配置阶段建议完成以下项目：

1. 配对码
2. 网络或代理配置
3. 一个大模型来源
4. 一个聊天通道

## 4. 验证消息链路

配好第一个模型和第一个聊天通道之后：

1. 打开你配置的聊天通道
2. 通过该聊天通道发送一条测试消息
3. 确认系统返回一次有效回复

## 5. 验证运行状态

建议在首次部署完成后检查一次运行状态：

```bash
beetle release status
```

常用运维命令如下：

- `beetle restart`
- `beetle stop`
- `beetle release rollback`

## 如果没有收到回复

- 检查 Linux 产物是否部署到了当前生效的版本路径
- 检查模型配置是否有效
- 检查聊天通道配置是否完整
- 检查发布状态和运行状态

## 相关文档

- Linux 运维与回滚：[linux-release-rollback.md](linux-release-rollback.md)
- 浏览器配置范围：[configuration.md](configuration.md)
- 能力概览：[capabilities.md](capabilities.md)
- 配置与集成接口：[config-api.md](config-api.md)
