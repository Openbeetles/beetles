# 架构概览

[English](../en-us/architecture.md) | **中文** | [文档索引](../README.md)

这页给准备读代码、扩功能、接新通道的人看。

如果你只是想把程序跑起来，先看别的文档即可；如果你想知道消息是怎么走的、配置从哪里进来、新功能应该接在哪一层，这页会比较有用。

这页主要说三件事：

- 主要模块分别负责什么
- 一条消息在程序里是怎么流动的
- 新通道、新工具、新模型接在哪里

## 一条消息的大致流程

从整体上看，Beetle 处理一条消息时会经过下面这几步：

1. 聊天通道把消息推进入站队列
2. 主处理循环取出消息，整理上下文
3. 按需要调用大模型、工具和记忆
4. 最终回复推到出站队列
5. 对应通道把回复发回去

## 主要模块

| 模块 | 作用 |
|------|------|
| `config` | 从环境变量、NVS、SPIFFS 加载并校验配置 |
| `error` | 统一错误类型和 stage 归因 |
| `bus` | 维护入站和出站消息队列 |
| `orchestrator` | 负责资源压力、TLS 碎片风险、健康状态和限流决策 |
| `memory` | 管理会话、archive evidence、shared factual plane、self continuity 和提示拼装 |
| `platform` | 平台抽象和平台相关实现 |
| `llm` | 各类大模型客户端和切换顺序 |
| `tools` | 工具定义、注册和执行入口 |
| `agent` | 负责主处理循环、上下文整理、工具调用和写回结果 |
| `channels` | 各聊天通道的收发逻辑 |
| `display` | SPI 屏幕配置和状态画面 |
| `metrics` | 计数、错误聚合和快照 |

另外还有一些按编译选项开启的部分，比如 `cli` 和 `ota`。

## 数据怎么流动

```text
channel -> inbound queue -> agent -> tools / memory / llm -> outbound queue -> dispatch -> channel sender
```

可以把它理解成：

- 入站消息来自聊天通道、Webhook 或定时任务
- 主处理循环取到消息后，会先整理这次对话需要的上下文，其中共享事实、archive evidence、私有连续性层会分别装配
- 处理过程中可能会查记忆、调工具、请求大模型；精确事实优先走 `factual_memory` / slot lookup，档案检索走 archive plane
- 结果写回会话和记忆，再交给对应通道发送出去；回复后还会触发 post-reply maintenance，必要时由 `self_runtime` 触发 boundary flush

在 ESP 上，运行态资源链路还把 internal heap 的最大连续空闲块当成一等公民信号：

- TLS admission 和运行压力都读取同一份 `heap_largest_block_internal` 快照
- `/api/resource` 会对外暴露推导后的 `tls_fragmentation_risk`，保证 operator 面、心跳日志和串口基线看到的是同一套口径
- `GET /api/channel_connectivity` 这类控制面诊断，在 WiFi 尚未稳定或碎片风险升高时必须退化为 stale 快照，不能为了“探测”再主动制造新的出站 TLS 压力

## 记忆主线

当前记忆主线不是单一 `MEMORY.md`，而是分层的：

- `archive plane`：聊天记录、daily note、turn log 等档案证据，用于检索和取证
- `shared factual plane`：canonical shared record，承载稳定事实、约束、项目/任务槽位
- `private continuity layers`：`self_model`、`inner_life`、`self_continuity`、`private_docs`、`private_garden`

这几层不会混成一个池子：

- archive 命中本身不是最终事实
- shared factual plane 优先提供可复用的 canonical 结论
- private layers 服务人格连续性，不默认对外暴露

连续性迁移和重启收口也已经接在这条链上：

- `continuity_snapshot` 可导出/导入 bootstrap 或 full restore 快照
- 设备重启前会尝试把最近活跃会话刷成 continuity bundle，降低 handoff / reboot 时的连续性断裂

## 扩展点

### 新增通道

要接一个新聊天通道，通常需要做三件事：

- 写好这个通道自己的收消息和发消息逻辑
- 把收到的消息送进入站队列
- 把发送器注册到通道分发流程里

### 新增工具

新增工具的入口比较直接：

- 实现 `Tool` trait
- 定义工具名、说明、参数结构和执行逻辑
- 在 `build_default_registry` 里注册

### 新增大模型后端

如果要接新的模型服务商：

- 实现 `LlmClient` trait
- 在 `llm` 模块的客户端构建流程里接进去
- 根据需要补上服务商默认地址、鉴权方式和响应适配

## 一个需要注意的边界

大部分模块都会通过平台抽象层去访问系统能力，这样同一套主程序才能同时跑在 ESP32-S3 和 Linux 上。

比较明显的例外，是 `channels/wss_gateway/esp_conn.rs` 这种明显只属于 ESP 的实现。像这类代码会直接依赖 ESP-IDF，本来就不需要兼容 Linux。

另外，ESP 上的持久化访问有两条必须遵守的约束：

- 状态文件和 SPIFFS 访问必须统一走 `platform::spiffs` / `StateFs` 门面，不要在业务模块里直接对 `/spiffs` 或 `state_mount_path()` 做裸 `std::fs` 读写。
- 显示刷新、presence 轮询这类热路径不能周期性扫 SPIFFS；需要展示的持久化状态要通过缓存、快照或显式恢复路径更新，避免把 flash/VFS 访问塞进高频循环。
- 像 `runtime_bundle` 这类仅在恢复/重启边界变化的持久化状态，在 ESP 运行态必须走显式失效缓存；禁止把 bundle 文件读取留在 display/presence 轮询路径里。
- ESP 启动顺序里，`soul_kernel recovery` 必须先于 WiFi bring-up；不要让重启恢复期的 SPIFFS 读取与 WiFi 异步启动窗口重叠。
- ESP 启动恢复不能直接压在 `main task` 上跑；`soul_kernel recovery` 必须在独立的 startup recovery 执行面内同步完成，再进入配置加载与 WiFi bring-up，避免把 continuity import / serde / SPIFFS 链路压进 `CONFIG_ESP_MAIN_TASK_STACK_SIZE`。

## 相关文档

- [tools.md](tools.md)
- [config-api.md](config-api.md)
- [hardware.md](hardware.md)
