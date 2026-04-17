# 架构概览

[English](../en-us/architecture.md) | **中文** | [文档索引](../README.md)

这篇给开发者看。
如果你要读代码、接新通道、加新工具，先从这里看全局。

## 整体分层

| 层 | 主要职责 |
|----|----------|
| `config` / `platform` | 读取配置，连接系统能力和硬件 |
| `channels` | 收消息、发消息 |
| `agent` | 理解输入，决定下一步，组织回复 |
| `tools` / `memory` | 调用外部能力，保存和读取重要信息 |
| `runtime` | 管理运行状态、资源和健康检查 |
| `display` / HTTP API | 提供屏幕显示、配置页和状态入口 |

## 一条消息怎么走

1. 消息从聊天通道进入
2. Agent 读取当前上下文和已保存信息
3. 需要时调用工具或其他服务
4. 生成回复并发回对应通道
5. 把重要结果写回存储

## 你通常会改哪里

### 接一个新通道

- 在 `channels` 里实现收发逻辑
- 把入站消息接进总线
- 把发送器注册到分发流程

### 加一个新工具

- 实现 `Tool` trait
- 定义说明、参数和执行逻辑
- 在 `build_default_registry` 里注册

### 接一个新模型

- 实现 `LlmClient` trait
- 接入现有客户端构建流程

### 接一个新平台

- 实现 `Platform` trait
- 在组装入口注入新的平台实现

## 开发时需要守住的边界

- 硬件相关代码放在 `platform`
- 业务层不要直接依赖硬件实现
- 错误统一走 `beetle::Error`
- 配置通过参数传递，不走全局可变状态
- 文档要和代码一起更新

## 相关文档

- [configuration.md](configuration.md)
- [tools.md](tools.md)
- [config-api.md](config-api.md)
- [hardware.md](hardware.md)
