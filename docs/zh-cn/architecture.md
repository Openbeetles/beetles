# 架构概览

[English](../en-us/architecture.md) | **中文** | [文档索引](../README.md)

本页说明 Beetle 的模块划分、数据流和扩展方式。

内容：

- 核心模块分别负责什么
- 数据是怎么流动的
- 新通道、新工具、新 LLM 后端该从哪里扩展

## 工作流程

从高层看，Beetle 的运行过程可以理解成：

1. 聊天通道把消息推进入站队列
2. Agent 构建上下文并调用 LLM
3. 在循环里结合工具和记忆
4. 最终回复推到出站队列
5. Dispatch 再按通道发出去

## 主要模块

| 模块 | 作用 |
|------|------|
| `config` | 从环境变量、NVS、SPIFFS 加载并校验配置 |
| `error` | 统一错误类型和 stage 归因 |
| `bus` | 入站、出站消息队列 |
| `orchestrator` | 资源门控、压力追踪、健康状态 |
| `memory` | 会话状态、长期记忆、摘要、提示上下文 |
| `platform` | 平台抽象和平台相关实现 |
| `llm` | LLM 客户端和回退路由 |
| `tools` | 工具定义和工具注册表 |
| `agent` | ReAct 循环、上下文构建、工具调用循环、会话写入 |
| `channels` | 通道入站、出站和 dispatch 管线 |
| `display` | SPI 显示配置与仪表板渲染 |
| `metrics` | 计数、错误聚合和快照 |

可选能力主要还有 `cli` 和 `ota`。

## 数据流

```text
channel -> inbound queue -> agent -> tools / memory / llm -> outbound queue -> dispatch -> channel sender
```

再往下一层看：

- 入站消息来自聊天通道或定时任务
- Agent 取一条消息，构建上下文，执行 LLM / 工具循环
- 会话和记忆状态被更新
- 最终回复进入出站分发

## 扩展点

### 新增通道

- 实现该通道自己的入站和出站逻辑
- 在 dispatch 初始化里注册 sink
- 把入站消息送进 bus

### 新增工具

- 实现 `Tool` trait
- 定义 `name`、`description`、`schema` 和执行逻辑
- 在 `build_default_registry` 里注册

### 新增 LLM 后端

- 实现 `LlmClient` trait
- 在 `llm` 模块里的客户端构建路径中接入

## 一个需要特别注意的边界

大多数模块都通过抽象层访问平台能力。

比较明显的例外，是 `channels/wss_gateway/esp_conn.rs` 这类 ESP 专用 WSS 传输，它会直接依赖 ESP-IDF 侧能力。

## 相关文档

- [tools.md](tools.md)
- [config-api.md](config-api.md)
- [hardware.md](hardware.md)
